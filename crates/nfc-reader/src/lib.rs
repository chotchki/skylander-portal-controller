//! Skylander NFC reader — dumps a Skylanders figure through an ACR122U (or
//! any PC/SC reader that speaks the same ACS Direct-Send envelope).
//!
//! Two layers:
//! - **Transport / protocol** ([`Reader`], [`dump_figure`]): open the reader,
//!   speak PN532 over `SCardControl(IOCTL_CCID_ESCAPE=3500)`, authenticate
//!   each Mifare Classic 1K sector with the derived Key A, read all 64
//!   blocks into a 1024-byte buffer.
//! - **Long-running worker** ([`run_scanner_worker`]): runs a blocking poll
//!   loop, debounces repeated taps of the same figure, emits
//!   [`skylander_core::Event::FigureScanned`] on a broadcast channel, writes
//!   `<uid>.sky` dumps to an output dir. Handles reader unplug/replug by
//!   backing off and reopening.
//!
//! Platform notes (verified 2026-04-22):
//! - **macOS**: needs the ACS Unified CCID Driver
//!   (<https://www.acs.com.hk/en/driver/3/acr122u-nfc-reader/>). The Apple
//!   inbox CCID driver silently rejects `SCardControl` escape commands.
//! - **Windows**: works with the inbox ACR122U driver.
//! - **Linux**: needs pcsc-lite + `libacsccid1`.
//! All three share the same IOCTL value (`SCARD_CTL_CODE(3500)`).

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use pcsc::{Card, Context as PcscContext, Protocols, Scope, ShareMode, ctl_code};
use skylander_core::Event;
use tokio::sync::broadcast;

pub const MIFARE_1K_BLOCKS: usize = 64;
pub const BLOCK_BYTES: usize = 16;
pub const SKY_SIZE: usize = MIFARE_1K_BLOCKS * BLOCK_BYTES; // 1024

/// CRC48 initial value per blog Appendix A:
/// `2 * 2 * 3 * 1103 * 12_868_356_821` → fits in 48 bits.
const CRC48_INIT: u64 = 2u64 * 2u64 * 3u64 * 1103u64 * 12_868_356_821u64;
/// ECMA-182 polynomial, reduced from 64-bit to left-shift-48-bit form.
const CRC48_POLY: u64 = 0x42F0_E1EB_A9EA_3693;
/// Sector-0 Key A: product of the three primes in the blog.
const SECTOR0_KEY_U64: u64 = 73u64 * 2017u64 * 560_381_651u64;

/// 4-byte NUID (Mifare Classic 1K's UID is always 4 bytes).
pub type Uid = [u8; 4];

/// A dumped Skylanders figure — its UID and the full 1024-byte tag image.
#[derive(Clone)]
pub struct SkyDump {
    pub uid: Uid,
    pub bytes: [u8; SKY_SIZE],
}

impl SkyDump {
    /// Hex-encode the UID as an 8-character uppercase string
    /// (e.g. `"7FC1ADA3"`) — canonical filename stem.
    pub fn uid_hex(&self) -> String {
        self.uid.iter().map(|b| format!("{:02X}", b)).collect()
    }
}

// ---------------- PC/SC reader open ----------------

/// Open the first PC/SC reader whose name contains `"acr122"`
/// (case-insensitive), or the first reader if none match.
pub fn open_reader() -> Result<Reader> {
    let ctx = PcscContext::establish(Scope::User)
        .context("PC/SC establish (is pcsc-lite / winscard running?)")?;
    let reader_name = pick_reader_name(&ctx)?;
    let card = connect_direct(&ctx, &reader_name)?;
    Ok(Reader {
        _ctx: ctx,
        card,
        reader_name: reader_name.to_string_lossy().into_owned(),
    })
}

fn pick_reader_name(ctx: &PcscContext) -> Result<std::ffi::CString> {
    let mut buf = [0u8; 2048];
    let readers = ctx.list_readers(&mut buf).context("list_readers")?;
    let names: Vec<std::ffi::CString> = readers.map(|r| r.to_owned()).collect();
    if names.is_empty() {
        bail!("no PC/SC readers found — is the ACR122U plugged in?");
    }
    Ok(names
        .iter()
        .find(|n| n.to_string_lossy().to_lowercase().contains("acr122"))
        .cloned()
        .unwrap_or_else(|| names[0].clone()))
}

fn connect_direct(ctx: &PcscContext, reader: &std::ffi::CStr) -> Result<Card> {
    // Single-protocol requests only: pcsc 2.9.0's `Protocol::from_raw`
    // panics when macOS returns T0|T1 together as the active-protocol
    // field (which it does on a Direct connect with ANY). Requesting a
    // single protocol makes the returned active-protocol single too.
    let attempts: &[(ShareMode, Protocols, &str)] = &[
        (ShareMode::Direct, Protocols::T1, "Direct + T1"),
        (ShareMode::Direct, Protocols::T0, "Direct + T0"),
        (ShareMode::Direct, Protocols::UNDEFINED, "Direct + UNDEFINED"),
    ];
    let mut last: Option<String> = None;
    for (share, proto, _label) in attempts {
        match ctx.connect(reader, *share, *proto) {
            Ok(card) => return Ok(card),
            Err(e) => last = Some(format!("{:?}", e)),
        }
    }
    bail!(
        "no Direct-mode combo accepted by PC/SC — last err: {}",
        last.unwrap_or_default()
    )
}

/// Owns a PC/SC `Card` handle plus the context that minted it; kept together
/// because the handle is invalid once the context drops.
pub struct Reader {
    // Kept alive for the handle's lifetime.
    _ctx: PcscContext,
    card: Card,
    reader_name: String,
}

impl Reader {
    pub fn name(&self) -> &str {
        &self.reader_name
    }

    /// PN532 GetFirmwareVersion, formatted `ic=0xXX ver=V rev=R support=0xXX`.
    pub fn firmware_version(&self) -> Result<String> {
        // PN532 GetFirmwareVersion: D4 02 → D5 03 <IC> <Ver> <Rev> <Support>
        let resp = self.escape(&[0xD4, 0x02])?;
        let body = expect_pn532_reply(&resp, 0x03)?;
        if body.len() < 4 {
            bail!("firmware response too short: {:?}", body);
        }
        Ok(format!(
            "ic=0x{:02X} ver={} rev={} support=0x{:02X}",
            body[0], body[1], body[2], body[3]
        ))
    }

    /// One-shot: ask the PN532 whether a Type-A 106kbps target is in the
    /// field. Returns `Some(uid)` if so, `None` for empty field.
    pub fn list_passive_target(&self) -> Result<Option<Uid>> {
        let resp = self.escape(&[0xD4, 0x4A, 0x01, 0x00])?;
        let body = expect_pn532_reply(&resp, 0x4B)?;
        if body.is_empty() {
            bail!("InListPassiveTarget: empty body");
        }
        if body[0] == 0 {
            return Ok(None);
        }
        // TargetData for Type A:
        //   TgNum(1) SENS_RES(2) SEL_RES(1) NFCIDLength(1) NFCID(L) [ATS...]
        if body.len() < 6 {
            bail!("InListPassiveTarget: body too short ({:?})", body);
        }
        let nfcid_len = body[5] as usize;
        if body.len() < 6 + nfcid_len {
            bail!(
                "InListPassiveTarget: claimed NFCID len {} but body is {} bytes",
                nfcid_len,
                body.len()
            );
        }
        if nfcid_len < 4 {
            bail!(
                "NFCID too short ({} bytes) — not a 4-byte NUID card",
                nfcid_len
            );
        }
        let mut nuid = [0u8; 4];
        nuid.copy_from_slice(&body[6..6 + 4]);
        Ok(Some(nuid))
    }

    /// Authenticate one Mifare block with `key` (Key A) + `uid`.
    fn mifare_authenticate(&self, block: u8, key: &[u8; 6], uid: &Uid) -> Result<()> {
        let mut payload = Vec::with_capacity(16);
        payload.extend_from_slice(&[0xD4, 0x40, 0x01, 0x60, block]);
        payload.extend_from_slice(key);
        payload.extend_from_slice(uid);
        let resp = self.escape(&payload)?;
        let body = expect_pn532_reply(&resp, 0x41)?;
        if body.is_empty() {
            bail!("auth block {}: empty body", block);
        }
        if body[0] != 0x00 {
            bail!("auth block {}: status 0x{:02X}", block, body[0]);
        }
        Ok(())
    }

    /// Read one authenticated 16-byte block.
    fn mifare_read(&self, block: u8) -> Result<[u8; 16]> {
        let resp = self.escape(&[0xD4, 0x40, 0x01, 0x30, block])?;
        let body = expect_pn532_reply(&resp, 0x41)?;
        if body.is_empty() {
            bail!("read block {}: empty body", block);
        }
        if body[0] != 0x00 {
            bail!("read block {}: status 0x{:02X}", block, body[0]);
        }
        if body.len() != 1 + 16 {
            bail!(
                "read block {}: expected 17-byte body (status + 16 data), got {}",
                block,
                body.len()
            );
        }
        let mut out = [0u8; 16];
        out.copy_from_slice(&body[1..17]);
        Ok(out)
    }

    /// Wrap a PN532 payload in ACS "Direct Send" APDU:
    /// `FF 00 00 00 <Lc> <payload>` and send via SCardControl IOCTL_CCID_ESCAPE.
    ///
    /// Returns the PN532 response bytes with ACR122U trailing `SW1 SW2`
    /// (`90 00`) stripped.
    fn escape(&self, payload: &[u8]) -> Result<Vec<u8>> {
        if payload.len() > 255 {
            bail!("escape payload too long ({} bytes)", payload.len());
        }
        let mut apdu = Vec::with_capacity(5 + payload.len());
        apdu.extend_from_slice(&[0xFF, 0x00, 0x00, 0x00]);
        apdu.push(payload.len() as u8);
        apdu.extend_from_slice(payload);

        // IOCTL_CCID_ESCAPE = SCARD_CTL_CODE(3500) is the ACS-family value,
        // mirroring Windows's convention; ACS's CCID driver (which we
        // require on macOS because the inbox Apple driver rejects escape)
        // accepts this code. pcsc-lite's stock IFD_ESCAPE = 1 isn't
        // accepted by the ACS driver on macOS, so we don't try it.
        const IOCTL_CCID_ESCAPE: u32 = 3500;

        let mut buf = [0u8; 512];
        let out = self
            .card
            .control(ctl_code(IOCTL_CCID_ESCAPE), &apdu, &mut buf)
            .context(
                "SCardControl(CCID_ESCAPE=3500) failed — on macOS this needs the ACS Unified CCID Driver installed (the inbox Apple CCID driver rejects escape commands).",
            )?;
        if out.len() < 2 {
            bail!("escape response too short ({} bytes)", out.len());
        }
        let (body, sw) = out.split_at(out.len() - 2);
        if sw[0] != 0x90 || sw[1] != 0x00 {
            bail!("escape returned SW={:02X}{:02X}", sw[0], sw[1]);
        }
        Ok(body.to_vec())
    }
}

/// PN532 response framing check: a valid reply starts with `D5 <cmd+1>`.
/// Strips and validates.
fn expect_pn532_reply(resp: &[u8], expected_cmd_echo: u8) -> Result<&[u8]> {
    if resp.len() < 2 {
        bail!("pn532 reply too short: {:?}", resp);
    }
    if resp[0] != 0xD5 {
        bail!("pn532 reply missing 0xD5 preamble: {:02X?}", resp);
    }
    if resp[1] != expected_cmd_echo {
        bail!(
            "pn532 reply cmd echo mismatch: expected {:02X}, got {:02X} (full: {:02X?})",
            expected_cmd_echo,
            resp[1],
            resp
        );
    }
    Ok(&resp[2..])
}

// ---------------- Key A derivation (blog Appendix A port) ----------------

fn compute_crc48(data: &[u8]) -> u64 {
    let mut crc = CRC48_INIT;
    for &b in data {
        crc ^= (b as u64) << 40;
        for _ in 0..8 {
            if crc & 0x8000_0000_0000 != 0 {
                crc = (crc << 1) ^ CRC48_POLY;
            } else {
                crc <<= 1;
            }
        }
    }
    crc & 0x0000_FFFF_FFFF_FFFF
}

/// Derive the Mifare Classic Key A for a given sector, given the 4-byte
/// NUID. Sector 0 uses a fixed, published constant. Other sectors hash
/// (NUID || sector_index) through CRC48 and byte-reverse the 48-bit result.
pub fn calculate_key_a(sector: u8, nuid: Uid) -> [u8; 6] {
    let key_u64 = if sector == 0 {
        SECTOR0_KEY_U64
    } else {
        let data = [nuid[0], nuid[1], nuid[2], nuid[3], sector];
        let be_crc = compute_crc48(&data);
        let mut rev = 0u64;
        for i in 0..6u64 {
            let byte = (be_crc >> (i * 8)) & 0xff;
            rev |= byte << ((5 - i) * 8);
        }
        rev
    };
    [
        ((key_u64 >> 40) & 0xff) as u8,
        ((key_u64 >> 32) & 0xff) as u8,
        ((key_u64 >> 24) & 0xff) as u8,
        ((key_u64 >> 16) & 0xff) as u8,
        ((key_u64 >> 8) & 0xff) as u8,
        (key_u64 & 0xff) as u8,
    ]
}

// ---------------- Whole-figure dump ----------------

/// Block 0 fields are always plaintext Mifare manufacturer data; skip auth
/// for the first block... actually no, every block requires auth on Mifare
/// Classic 1K. We auth with the sector-level Key A.
///
/// Iterates 16 sectors × 4 blocks. On any auth/read error, returns early;
/// caller decides whether to retry or surface the failure.
pub fn dump_figure(reader: &Reader, uid: Uid) -> Result<[u8; SKY_SIZE]> {
    let mut dump = [0u8; SKY_SIZE];
    for sector in 0..16u8 {
        let key = calculate_key_a(sector, uid);
        for block_in_sector in 0..4u8 {
            let block = sector * 4 + block_in_sector;
            reader.mifare_authenticate(block, &key, &uid)?;
            let data = reader.mifare_read(block)?;
            let off = block as usize * BLOCK_BYTES;
            dump[off..off + BLOCK_BYTES].copy_from_slice(&data);
        }
    }
    Ok(dump)
}

/// Block-until-present + dump. Polls `list_passive_target` every 200ms.
pub fn scan_once(reader: &Reader) -> Result<SkyDump> {
    loop {
        if let Some(uid) = reader.list_passive_target()? {
            let bytes = dump_figure(reader, uid)?;
            return Ok(SkyDump { uid, bytes });
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

// ---------------- Long-running scanner worker ----------------

/// How long a just-scanned UID stays in the "already seen" set before we'd
/// re-dump it. Prevents spamming the broadcast channel when a figure is
/// left on the pad.
const DEDUP_WINDOW: Duration = Duration::from_secs(5);

/// Delay between reader-reopen attempts when the reader is unplugged or
/// otherwise gone.
const RECONNECT_BACKOFF: Duration = Duration::from_secs(2);

/// Poll the reader continuously, dumping each distinct figure and emitting
/// [`Event::FigureScanned`] on `events`. Writes raw `<uid>.sky` bytes under
/// `out_dir` (created if missing). Intended to run on its own OS thread.
///
/// Infinite loop — returns only if `events` has been dropped by every
/// subscriber *and* sends start to fail with `SendError`. Process-shutdown
/// is signalled by dropping all event receivers + the Tokio runtime.
pub fn run_scanner_worker(events: broadcast::Sender<Event>, out_dir: std::path::PathBuf) {
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        tracing::error!(error = %e, dir = %out_dir.display(), "nfc-scanner: create out_dir failed — worker exiting");
        return;
    }

    let mut last_seen: Option<(Uid, Instant)> = None;
    loop {
        let reader = match open_reader() {
            Ok(r) => {
                tracing::info!(reader = %r.name(), "nfc-scanner: opened");
                if let Ok(fw) = r.firmware_version() {
                    tracing::info!(firmware = %fw, "nfc-scanner: PN532 firmware");
                }
                r
            }
            Err(e) => {
                tracing::debug!(error = %e, "nfc-scanner: no reader yet — retrying");
                std::thread::sleep(RECONNECT_BACKOFF);
                continue;
            }
        };

        // Inner poll loop — on any hard error, drop the reader and reopen.
        loop {
            match reader.list_passive_target() {
                Ok(Some(uid)) => {
                    let now = Instant::now();
                    let fresh = match last_seen {
                        Some((prev_uid, prev_t)) => {
                            prev_uid != uid || now.duration_since(prev_t) > DEDUP_WINDOW
                        }
                        None => true,
                    };
                    if !fresh {
                        std::thread::sleep(Duration::from_millis(200));
                        continue;
                    }

                    match dump_figure(&reader, uid) {
                        Ok(bytes) => {
                            let dump = SkyDump { uid, bytes };
                            last_seen = Some((uid, now));
                            if let Err(e) = persist_and_broadcast(&dump, &out_dir, &events) {
                                tracing::warn!(error = %e, uid = %dump.uid_hex(), "nfc-scanner: post-dump handling failed");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, uid = %format_uid(&uid), "nfc-scanner: dump failed — skipping this tap");
                            // Don't update last_seen — let the user retry
                            // without the dedupe window swallowing it.
                            std::thread::sleep(Duration::from_millis(500));
                        }
                    }
                }
                Ok(None) => {
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(e) => {
                    tracing::info!(error = %e, "nfc-scanner: reader went away — reopening");
                    std::thread::sleep(RECONNECT_BACKOFF);
                    break;
                }
            }
        }
    }
}

fn persist_and_broadcast(
    dump: &SkyDump,
    out_dir: &Path,
    events: &broadcast::Sender<Event>,
) -> Result<()> {
    let path = out_dir.join(format!("{}.sky", dump.uid_hex()));
    std::fs::write(&path, &dump.bytes)
        .with_context(|| format!("write {}", path.display()))?;

    // Parse identity fields — figure_id + variant are plaintext block 0,
    // always decodable. Nickname is encrypted-payload territory and can
    // fail (wrong FigureKind classification, CYOS layout gap, etc.); we
    // surface whatever the parser gives us and let the phone fall back
    // to the uid when empty.
    let (figure_id, variant, display_name) = match skylander_sky_parser::parse(&dump.bytes) {
        Ok(stats) => (stats.figure_id, stats.variant, stats.nickname.clone()),
        Err(e) => {
            tracing::warn!(error = ?e, uid = %dump.uid_hex(), "nfc-scanner: parse failed — emitting scan event anyway with unknown identity");
            (0, 0, String::new())
        }
    };

    tracing::info!(
        uid = %dump.uid_hex(),
        figure_id = format!("0x{:06X}", figure_id),
        variant = format!("0x{:04X}", variant),
        display_name = %display_name,
        path = %path.display(),
        "nfc-scanner: figure dumped"
    );

    // send() only errs if all receivers have dropped — not fatal, the worker
    // is still useful on reconnect.
    let _ = events.send(Event::FigureScanned {
        uid: dump.uid_hex(),
        figure_id,
        variant,
        display_name,
    });
    Ok(())
}

fn format_uid(uid: &Uid) -> String {
    uid.iter().map(|b| format!("{:02X}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sector0_key_matches_known_static() {
        assert_eq!(
            calculate_key_a(0, [0x00; 4]),
            [0x4B, 0x0B, 0x20, 0x10, 0x7C, 0xCB]
        );
    }

    #[test]
    fn crc48_init_fits_in_48_bits() {
        assert_eq!(CRC48_INIT & !0x0000_FFFF_FFFF_FFFF, 0);
    }

    #[test]
    fn compute_crc48_is_deterministic() {
        let a = compute_crc48(&[0x01, 0x02, 0x03, 0x04, 0x01]);
        let b = compute_crc48(&[0x01, 0x02, 0x03, 0x04, 0x01]);
        assert_eq!(a, b);
        let c = compute_crc48(&[0x01, 0x02, 0x03, 0x04, 0x02]);
        assert_ne!(a, c);
    }

    #[test]
    fn key_a_sector1_differs_from_sector2() {
        let k1 = calculate_key_a(1, [0xDE, 0xAD, 0xBE, 0xEF]);
        let k2 = calculate_key_a(2, [0xDE, 0xAD, 0xBE, 0xEF]);
        assert_ne!(k1, k2);
    }

    #[test]
    fn sky_dump_uid_hex_is_uppercase_eight_chars() {
        let dump = SkyDump {
            uid: [0x7F, 0xC1, 0xAD, 0xA3],
            bytes: [0u8; SKY_SIZE],
        };
        assert_eq!(dump.uid_hex(), "7FC1ADA3");
    }
}
