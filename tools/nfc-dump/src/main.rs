//! PLAN 6.5.0 spike — dump a Skylanders figure via PC/SC.
//!
//! Usage:
//!   `cargo run -p skylander-nfc-dump -- [output-dir]` — full dump flow.
//!   `cargo run -p skylander-nfc-dump -- --probe`     — reader-only escape probe.
//!
//! Approach: connect in `ShareMode::Direct` so we can send PN532 commands via
//! `SCardControl` escape without needing PC/SC to successfully activate the
//! card (Mifare Classic 1K is ISO14443-3 only — no native ATS — so PC/SC
//! never marks it PRESENT on its own). Direct mode lets us talk straight to
//! the PN532: `InListPassiveTarget` → grab UID → `InDataExchange` for Mifare
//! authenticate + read, block-by-block.
//!
//! macOS requires the ACS Unified CCID Driver
//! (https://www.acs.com.hk/en/driver/3/acr122u-nfc-reader/) — the inbox
//! Apple CCID driver silently rejects `SCardControl` escape commands. The
//! ACS driver also mirrors the Windows IOCTL convention:
//! `IOCTL_CCID_ESCAPE = SCARD_CTL_CODE(3500)` (not pcsc-lite's
//! `IFD_ESCAPE = 1`), which is the value we use in `escape()` below.
//! Windows works out-of-the-box with the same IOCTL. Linux needs pcsc-lite
//! plus the `libacsccid1` driver package (same IOCTL value).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use pcsc::{Card, Context as PcscContext, Protocols, Scope, ShareMode, ctl_code};

const MIFARE_1K_BLOCKS: usize = 64;
const BLOCK_BYTES: usize = 16;
const SKY_SIZE: usize = MIFARE_1K_BLOCKS * BLOCK_BYTES; // 1024

/// CRC48 initial value per blog Appendix A:
/// `2 * 2 * 3 * 1103 * 12_868_356_821` → fits in 48 bits.
const CRC48_INIT: u64 = 2u64 * 2u64 * 3u64 * 1103u64 * 12_868_356_821u64;
/// ECMA-182 polynomial, reduced from 64-bit to left-shift-48-bit form.
const CRC48_POLY: u64 = 0x42F0_E1EB_A9EA_3693;
/// Sector-0 Key A: product of the three primes in the blog.
const SECTOR0_KEY_U64: u64 = 73u64 * 2017u64 * 560_381_651u64;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let probe_only = args.iter().any(|a| a == "--probe");
    let out_dir = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./dev-data/scanned"));
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("create output dir {}", out_dir.display()))?;

    let ctx = PcscContext::establish(Scope::User)
        .context("PC/SC establish (is pcsc-lite running?)")?;

    let reader_name = pick_reader(&ctx)?;
    println!("reader: {}", reader_name.to_string_lossy());

    // Direct mode attaches to the reader itself, no card required. This is
    // what lets us send PN532 commands before (or instead of) PC/SC card
    // activation.
    // macOS PC/SC is picky about `ShareMode::Direct` + protocol combos.
    // Try the documented variants in order and use whichever is accepted.
    let reader = connect_direct(&ctx, &reader_name)?;

    // Step 1: prove escape works at all by asking for the firmware version.
    // Failure here = macOS CCID driver has IFD_ESCAPE disabled → fall back
    // to option 1 (install ACS driver) or option 3 (raw USB).
    let fw = get_firmware_version(&reader)?;
    println!("firmware: {}", fw);

    if probe_only {
        println!("probe OK — escape path is live. Re-run without --probe to dump a figure.");
        return Ok(());
    }

    println!("tap a Skylanders figure on the reader…");
    let (uid, wait_elapsed) = poll_target(&reader)?;
    println!(
        "target found after {:.2?}, uid={}",
        wait_elapsed,
        uid.iter().map(|b| format!("{:02X}", b)).collect::<String>()
    );

    let t0 = Instant::now();
    let mut dump = [0u8; SKY_SIZE];
    for sector in 0..16u8 {
        let key = calculate_key_a(sector, uid);
        for block_in_sector in 0..4u8 {
            let block = sector * 4 + block_in_sector;
            mifare_authenticate(&reader, block, &key, &uid)?;
            let data = mifare_read(&reader, block)?;
            let off = (block as usize) * BLOCK_BYTES;
            dump[off..off + BLOCK_BYTES].copy_from_slice(&data);
        }
    }
    let read_elapsed = t0.elapsed();
    println!(
        "read {} bytes in {:.3?} ({} blocks)",
        SKY_SIZE, read_elapsed, MIFARE_1K_BLOCKS
    );

    let uid_hex: String = uid.iter().map(|b| format!("{:02X}", b)).collect();
    let path = out_dir.join(format!("{}.sky", uid_hex));
    std::fs::write(&path, &dump).with_context(|| format!("write {}", path.display()))?;
    println!("wrote {}", path.display());

    match skylander_sky_parser::parse(&dump) {
        Ok(stats) => {
            println!(
                "parser: figure_id=0x{:06X} variant=0x{:04X} kind={:?} level={} gold={} nickname={:?} crc_ok={}",
                stats.figure_id,
                stats.variant,
                stats.figure_kind,
                stats.level,
                stats.gold,
                stats.nickname,
                stats.checksums_valid,
            );
            if !stats.checksums_valid {
                eprintln!(
                    "note: parsed without structural error but CRC16 failed — compare to the same figure's existing sample in ./dev-data/validation-figures/ to tell scan-error from layout-gap."
                );
            }
        }
        Err(e) => {
            eprintln!("parser error: {:?}", e);
            eprintln!("  bytes still saved to {}", path.display());
        }
    }

    Ok(())
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
    for (share, proto, label) in attempts {
        match ctx.connect(reader, *share, *proto) {
            Ok(card) => {
                eprintln!("  connected via {}", label);
                return Ok(card);
            }
            Err(e) => {
                eprintln!("  {} rejected: {:?}", label, e);
                last = Some(format!("{:?}", e));
            }
        }
    }
    bail!(
        "no Direct-mode combo accepted by macOS PC/SC — last err: {}",
        last.unwrap_or_default()
    )
}

fn pick_reader(ctx: &PcscContext) -> Result<std::ffi::CString> {
    let mut buf = [0u8; 2048];
    let readers = ctx.list_readers(&mut buf).context("list_readers")?;
    let names: Vec<std::ffi::CString> = readers.map(|r| r.to_owned()).collect();
    if names.is_empty() {
        bail!(
            "no PC/SC readers found. macOS: plug in ACR122U (inbox CCID driver handles detection). Linux: start pcscd."
        );
    }
    Ok(names
        .iter()
        .find(|n| n.to_string_lossy().to_lowercase().contains("acr122"))
        .cloned()
        .unwrap_or_else(|| names[0].clone()))
}

// ---------------- ACR122U / PN532 escape transport ----------------

/// Wrap a PN532 payload in ACS "Direct Send" APDU:
/// `FF 00 00 00 <Lc> <payload>` and send via SCardControl IFD_ESCAPE.
///
/// Returns the PN532 response bytes with ACR122U trailing `SW1 SW2` (90 00)
/// stripped. Does NOT strip the PN532 preamble/D5/cmd bytes — callers check
/// those themselves so they can see errors verbatim.
fn escape(reader: &Card, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > 255 {
        bail!("escape payload too long ({} bytes)", payload.len());
    }
    let mut apdu = Vec::with_capacity(5 + payload.len());
    apdu.extend_from_slice(&[0xFF, 0x00, 0x00, 0x00]);
    apdu.push(payload.len() as u8);
    apdu.extend_from_slice(payload);

    // IOCTL_CCID_ESCAPE = SCARD_CTL_CODE(3500) is the ACS-family value,
    // mirroring Windows's convention; ACS's own CCID driver (which we
    // require on macOS because the inbox CCID driver has escape locked
    // down) accepts this code. pcsc-lite's stock IFD_ESCAPE = 1 isn't
    // accepted by the ACS driver on macOS, so we don't bother trying it.
    const IOCTL_CCID_ESCAPE: u32 = 3500;

    let mut buf = [0u8; 512];
    let out = reader
        .control(ctl_code(IOCTL_CCID_ESCAPE), &apdu, &mut buf)
        .context(
            "SCardControl(CCID_ESCAPE=3500) failed — on macOS this needs the ACS Unified CCID Driver installed (the inbox Apple CCID driver rejects escape commands).",
        )?;
    if out.len() < 2 {
        bail!("escape response too short ({} bytes)", out.len());
    }
    let (body, sw) = out.split_at(out.len() - 2);
    let sw1 = sw[0];
    let sw2 = sw[1];
    if sw1 != 0x90 || sw2 != 0x00 {
        bail!("escape returned SW={:02X}{:02X}", sw1, sw2);
    }
    Ok(body.to_vec())
}

/// PN532 response framing check: a valid InDataExchange reply starts with
/// `D5 <cmd+1>` followed by command-specific bytes. Strips and validates.
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

fn get_firmware_version(reader: &Card) -> Result<String> {
    // PN532 GetFirmwareVersion: D4 02 → D5 03 <IC> <Ver> <Rev> <Support>
    let resp = escape(reader, &[0xD4, 0x02])?;
    let body = expect_pn532_reply(&resp, 0x03)?;
    if body.len() < 4 {
        bail!("firmware response too short: {:?}", body);
    }
    Ok(format!(
        "ic=0x{:02X} ver={} rev={} support=0x{:02X}",
        body[0], body[1], body[2], body[3]
    ))
}

fn poll_target(reader: &Card) -> Result<([u8; 4], Duration)> {
    let t0 = Instant::now();
    let mut last_err: Option<String> = None;
    loop {
        match list_passive_target(reader) {
            Ok(Some(uid)) => return Ok((uid, t0.elapsed())),
            Ok(None) => {} // No target in field yet.
            Err(e) => {
                let msg = format!("{:?}", e);
                if last_err.as_deref() != Some(&msg) {
                    eprintln!("  poll (non-fatal): {}", msg);
                    last_err = Some(msg);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
        if t0.elapsed().as_secs() > 0 && t0.elapsed().as_secs() % 10 == 0 && t0.elapsed().subsec_millis() < 200 {
            eprintln!("  ({:.0?} waited — still no target)", t0.elapsed());
        }
    }
}

/// PN532 InListPassiveTarget (1 target, 106 kbps Type A).
/// Returns Some(uid) if a card is in the field, None if not.
fn list_passive_target(reader: &Card) -> Result<Option<[u8; 4]>> {
    // D4 4A <MaxTg=01> <BrTy=00: 106 kbps Type A>
    let resp = escape(reader, &[0xD4, 0x4A, 0x01, 0x00])?;
    let body = expect_pn532_reply(&resp, 0x4B)?;
    if body.is_empty() {
        bail!("InListPassiveTarget: empty body");
    }
    let nb_tg = body[0];
    if nb_tg == 0 {
        return Ok(None);
    }
    // TargetData layout for Type A:
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

fn mifare_authenticate(reader: &Card, block: u8, key: &[u8; 6], uid: &[u8; 4]) -> Result<()> {
    // InDataExchange: D4 40 <Tg=01> <Mifare Auth A = 60> <block> <key(6)> <uid(4)>
    let mut payload = Vec::with_capacity(16);
    payload.extend_from_slice(&[0xD4, 0x40, 0x01, 0x60, block]);
    payload.extend_from_slice(key);
    payload.extend_from_slice(uid);
    let resp = escape(reader, &payload)?;
    let body = expect_pn532_reply(&resp, 0x41)?;
    if body.is_empty() {
        bail!("auth block {}: empty body", block);
    }
    if body[0] != 0x00 {
        bail!("auth block {}: status 0x{:02X}", block, body[0]);
    }
    Ok(())
}

fn mifare_read(reader: &Card, block: u8) -> Result<[u8; 16]> {
    // InDataExchange: D4 40 <Tg=01> <Mifare Read = 30> <block>
    let resp = escape(reader, &[0xD4, 0x40, 0x01, 0x30, block])?;
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

fn calculate_key_a(sector: u8, nuid: [u8; 4]) -> [u8; 6] {
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
}
