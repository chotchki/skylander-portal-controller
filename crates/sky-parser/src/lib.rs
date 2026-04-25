//! Read-only parser for Skylanders `.sky` firmware dumps.
//!
//! A `.sky` file is a 1024-byte dump of the NFC chip on the back of a physical
//! Skylander figure: 64 blocks of 16 bytes each (the Mifare Classic 1K layout
//! the toy uses).
//!
//! The authoritative format reference lives at
//! `docs/research/sky-format/SkylanderFormat.md` (mirrored from the Runes
//! project: <https://github.com/NefariousTechSupport/Runes/blob/master/Docs/SkylanderFormat.md>).
//!
//! # Encryption
//!
//! RPCS3 preserves the full Mifare Classic encryption layer: blocks 0–7 and
//! every sector trailer (`(i + 1) % 4 == 0`) are plaintext; all other blocks
//! are AES-128-ECB with per-block keys derived as
//! `MD5(block_0 || block_1 || block_index_byte || " Copyright (C) 2010 Activision. All Rights Reserved.")`.
//! [`parse`] decrypts before reading payload fields — the 0x56-byte hash-input
//! is built once per figure, then the single-byte block index is flipped for
//! each block. Zero-filled blocks are passed through unchanged (matches the
//! reference encrypt path, which doesn't "encrypt" all-zero blocks either).
//! This is the algorithm from the blog post linked in PLAN 6.2, Appendix D.
//!
//! # Scope
//!
//! Read-only. We never write, never regenerate CRCs, never move files around.
//!
//! # What is parsed
//!
//! * Header (block 0): serial, figure id (toy type), variant, trading-card id
//!   (+ derived web code), error byte, header CRC.
//! * Variant decomposition: deco id, supercharger/lightcore/in-game/reposed
//!   flags, year code.
//! * Standard-layout figure data (non-Trap, non-Vehicle, non-CYOS, non-Racing-
//!   Pack) — XP for 2011/2012/2013 games, gold, nickname, hero points,
//!   playtime, current hat + hat history, trinket, last-placed / last-reset
//!   timestamps, Heroic Challenges, Giants quests, Swap Force quests.
//! * Checksum validation for the header + active data area.
//!
//! # What is stubbed
//!
//! Trap, Vehicle, Racing Pack, and CYOS/Imaginator-crystal layouts classify
//! as non-Standard [`FigureKind`] variants and their payload-specific fields
//! stay defaulted. PLAN 6.2.3–6.2.5 extend each kind's parser.
//! See the corresponding sections of `SkylanderFormat.md` for a future pass.

#![warn(missing_docs)]
// This crate deliberately writes offsets as `block_base + 0x00` even when
// the offset within a block is zero. The `+ 0x00` documents the spec-level
// field offset inside each 16-byte block and keeps every read/write line
// lined up for eyeball comparison against `SkylanderFormat.md`. Collapsing
// them to bare `block_base` destroys that parallelism, so silence the
// pedantic identity_op lint for the whole file.
#![allow(clippy::identity_op)]

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// `.sky` dumps are always exactly 1024 bytes — 64 × 16.
pub const SKY_FILE_LEN: usize = 1024;
/// Number of 16-byte blocks in a Mifare Classic 1K dump.
pub const BLOCK_COUNT: usize = 64;
/// Bytes per block.
pub const BLOCK_LEN: usize = 16;

// ---------------------------------------------------------------------------
// Header offsets — `SkylanderFormat.md` "tfbSpyroTag_TagHeader" section.
// All offsets are within the first 0x20-byte header block pair (blocks 0-1).
// ---------------------------------------------------------------------------

/// Serial / non-unique identifier (uint32 at 0x00).
pub const OFFSET_SERIAL: usize = 0x00;
/// Error byte (uint8 at 0x13). Any non-zero value causes the game to reject.
pub const OFFSET_ERROR_BYTE: usize = 0x13;
/// Trading card ID (two uint32 LE halves at 0x14 / 0x18).
pub const OFFSET_TRADING_CARD: usize = 0x14;
/// `figure_id` (toy type) — 24-bit LE at 0x10.
pub const OFFSET_FIGURE_ID: usize = 0x10;
/// Variant bitfield — uint16 LE at 0x1C.
pub const OFFSET_VARIANT: usize = 0x1C;
/// Header CRC16 — uint16 LE at 0x1E; covers bytes 0x00..0x1E.
pub const OFFSET_HEADER_CRC: usize = 0x1E;

// ---------------------------------------------------------------------------
// AES-128-ECB + MD5 per-block decrypt (see module-level encryption docs).
// ---------------------------------------------------------------------------

/// " Copyright (C) 2010 Activision. All Rights Reserved. " (leading AND
/// trailing space — the blog's hex dump ends in 0x20). Trailing 0x35 bytes
/// of the 0x56-byte hash input.
const HASH_CONST: [u8; 0x35] = *b" Copyright (C) 2010 Activision. All Rights Reserved. ";

/// Return `true` for blocks the encryption algorithm leaves as plaintext:
/// blocks 0..=7 (sector 0 + sector 1) and every sector trailer (every 4th
/// block starting at 3).
fn is_plaintext_block(i: usize) -> bool {
    i < 8 || (i + 1).is_multiple_of(4)
}

/// Build the 0x56-byte hash-input template: `block_0 || block_1 || 0x00 ||
/// HASH_CONST`. The single zero byte at offset 0x20 is overwritten with the
/// block index per block.
fn hash_in_template(bytes: &[u8; SKY_FILE_LEN]) -> [u8; 0x56] {
    let mut out = [0u8; 0x56];
    out[0x00..0x20].copy_from_slice(&bytes[0x00..0x20]);
    // out[0x20] left as 0; callers overwrite with block index.
    out[0x21..0x56].copy_from_slice(&HASH_CONST);
    out
}

fn block_key(hash_in_template: &[u8; 0x56], block_index: u8) -> [u8; 16] {
    use md5::{Digest, Md5};
    let mut hash_in = *hash_in_template;
    hash_in[0x20] = block_index;
    let digest = Md5::digest(hash_in);
    let mut key = [0u8; 16];
    key.copy_from_slice(&digest);
    key
}

/// Decrypt a whole figure in place. See module docs for the algorithm; matches
/// the blog's Appendix D `DecryptFigure`.
pub fn decrypt_figure(bytes: &mut [u8; SKY_FILE_LEN]) {
    use aes::Aes128;
    use aes::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};

    let template = hash_in_template(bytes);
    for i in 0..BLOCK_COUNT {
        if is_plaintext_block(i) {
            continue;
        }
        let off = i * BLOCK_LEN;
        let block = &mut bytes[off..off + BLOCK_LEN];
        if block.iter().all(|&b| b == 0) {
            // Matches the reference: zero-filled blocks aren't decrypted so
            // they remain zero (their encrypted form is also all-zero because
            // the encrypt path skips them).
            continue;
        }
        let key = block_key(&template, i as u8);
        let cipher = Aes128::new(GenericArray::from_slice(&key));
        let mut buf = GenericArray::clone_from_slice(block);
        cipher.decrypt_block(&mut buf);
        block.copy_from_slice(&buf);
    }
}

/// Encrypt a whole figure in place. Inverse of [`decrypt_figure`]; used by
/// tests so fixtures can exercise the full decrypt path.
pub fn encrypt_figure(bytes: &mut [u8; SKY_FILE_LEN]) {
    use aes::Aes128;
    use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};

    let template = hash_in_template(bytes);
    for i in 0..BLOCK_COUNT {
        if is_plaintext_block(i) {
            continue;
        }
        let off = i * BLOCK_LEN;
        let block = &mut bytes[off..off + BLOCK_LEN];
        if block.iter().all(|&b| b == 0) {
            continue;
        }
        let key = block_key(&template, i as u8);
        let cipher = Aes128::new(GenericArray::from_slice(&key));
        let mut buf = GenericArray::clone_from_slice(block);
        cipher.encrypt_block(&mut buf);
        block.copy_from_slice(&buf);
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Reasons a `.sky` blob can fail to parse.
#[derive(Debug, Error)]
pub enum ParseError {
    /// File wasn't exactly 1024 bytes.
    #[error("invalid length: expected {SKY_FILE_LEN} bytes, got {0}")]
    BadLength(usize),
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Game that the figure originated in.
///
/// Derived from the 4-bit year code in the variant bitfield (bits 12..16), per
/// `SkylanderFormat.md` "Variant ID". Unknown codes fall through to
/// [`SkyGeneration::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkyGeneration {
    /// Spyro's Adventure (2011).
    SpyrosAdventure,
    /// Skylanders Giants (2012).
    Giants,
    /// Skylanders Swap Force (2013).
    SwapForce,
    /// Skylanders Trap Team (2014).
    TrapTeam,
    /// Skylanders SuperChargers (2015).
    SuperChargers,
    /// Skylanders Imaginators (2016).
    Imaginators,
    /// Year code didn't map.
    Unknown,
}

impl SkyGeneration {
    /// Map the 4-bit year code (variant bits 12..16) to a game.
    ///
    /// Values come from `ESkylandersGame.hpp` in the Runes source tree (see
    /// `SkylanderFormat.md`): 0 = unused, 1 = SSA, 2 = Giants, 3 = Swap Force,
    /// 4 = Trap Team, 5 = SuperChargers, 6 = Imaginators.
    pub fn from_year_code(code: u8) -> Self {
        match code {
            1 => Self::SpyrosAdventure,
            2 => Self::Giants,
            3 => Self::SwapForce,
            4 => Self::TrapTeam,
            5 => Self::SuperChargers,
            6 => Self::Imaginators,
            _ => Self::Unknown,
        }
    }
}

/// Kind of figure inferred from its `figure_id` range. Drives which "data
/// area" layout applies — see `SkylanderFormat.md`. Only [`FigureKind::Standard`]
/// currently has a payload parser; the non-Standard variants exist so the UI
/// can branch per kind and future per-kind parsers can slot in without API
/// churn (PLAN 6.2.2 split this from the prior single `Other` variant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FigureKind {
    /// Standard Skylander / Giant / Swapper / Trap Master / Sensei — the
    /// `tfbSpyroTagData` layout that [`parse`] reads in full.
    Standard,
    /// Trap Team elemental trap crystal (figure_id 0x1F4..=0x225). Payload
    /// layout lives at `SkylanderFormat.md` §"Trap".
    Trap,
    /// SuperChargers vehicle (figure_id 0x2BC..=0x2ED). Payload layout at
    /// `SkylanderFormat.md` §"Vehicle".
    Vehicle,
    /// SuperChargers Racing Pack trophy. Range unknown in the public spec —
    /// no files classify here until the range is confirmed against real
    /// dumps; variant reserved so downstream code can match it exhaustively.
    RacingPack,
    /// Imaginators Creation Crystal (figure_id 0x320..=0x383). Spec warns
    /// this layout "may be incorrect, actively being worked on"
    /// (`SkylanderFormat.md` §"CYOS").
    Cyos,
    /// Fallthrough for figure_ids outside any known range.
    Other,
}

/// Re-export of [`skylander_core::VARIANT_IDENTITY_MASK`] for consumers
/// that were already reaching into sky-parser for this. The const itself
/// moved to `core` in PLAN 6.6.1b alongside the newtype wrappers it
/// operates on — `TagVariant::mask_to_identity()` is the idiomatic form
/// new code should use instead of ANDing with the raw u16.
pub use skylander_core::VARIANT_IDENTITY_MASK;

/// Decomposition of the 16-bit variant bitfield.
///
/// See `SkylanderFormat.md` "Variant ID". Note the comment there that some
/// flags are set inconsistently on real tags and the SuperCharger flag is
/// never acted upon by the games.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantInfo {
    /// Raw 16-bit value as stored on the tag.
    pub raw: u16,
    /// Deco ID (low 8 bits).
    pub deco_id: u8,
    /// Whether this figure is flagged as a SuperCharger.
    pub is_supercharger: bool,
    /// Whether this figure has LightCore tech.
    pub is_lightcore: bool,
    /// Whether this is an in-game variant.
    pub is_in_game_variant: bool,
    /// Whether this figure is a repose (usually implies Wow Pow).
    pub is_reposed: bool,
    /// Game of origin (year code — bits 12..16).
    pub year_code: SkyGeneration,
}

impl VariantInfo {
    /// Decompose a raw variant word.
    pub fn from_raw(raw: u16) -> Self {
        // Offsets per the spec table: deco 0..8, sc at 8, lc at 9, in-game at
        // 10, reposed at 11, year 12..16.
        Self {
            raw,
            deco_id: (raw & 0x00FF) as u8,
            is_supercharger: (raw >> 8) & 1 != 0,
            is_lightcore: (raw >> 9) & 1 != 0,
            is_in_game_variant: (raw >> 10) & 1 != 0,
            is_reposed: (raw >> 11) & 1 != 0,
            year_code: SkyGeneration::from_year_code(((raw >> 12) & 0x0F) as u8),
        }
    }
}

/// Opaque identifier for a hat cosmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HatId(pub u16);

impl HatId {
    /// Sentinel for "no hat".
    pub const NONE: HatId = HatId(0);
}

/// Derive the base-29 Web Code from the 64-bit Trading Card ID.
///
/// Per `SkylanderFormat.md` "Web Code": repeatedly take id mod 29 as the next
/// (least-significant) character, then divide. Alphabet is
/// `23456789BCDFGHJKLMNPQRSTVWXYZ` (0 → '2' … 28 → 'Z'). The spec points at
/// `Runes::PortalTag::StoreHeader()` for a C++ reference. We right-pad up to
/// 12 chars — real web codes are 10 chars but the spec doesn't nail a fixed
/// length, so we just emit until the value hits 0. Output is uppercase.
pub fn web_code_from_trading_card(trading_card_id: u64) -> String {
    const ALPHABET: &[u8] = b"23456789BCDFGHJKLMNPQRSTVWXYZ";
    if trading_card_id == 0 {
        return String::from("2");
    }
    let mut v = trading_card_id;
    let mut out: Vec<u8> = Vec::new();
    while v > 0 {
        out.push(ALPHABET[(v % 29) as usize]);
        v /= 29;
    }
    out.reverse();
    // Safety: all bytes come from ALPHABET (ASCII).
    String::from_utf8(out).expect("alphabet is ASCII")
}

/// Resolve a character level (1..=20) from cumulative XP.
///
/// Table lifted directly from `SkylanderFormat.md` "Experience". Anything
/// below 1000 XP is level 1; anything at or above 199535 is level 20.
pub fn level_from_xp(xp: u32) -> u8 {
    const THRESHOLDS: [u32; 20] = [
        0, 1000, 2200, 3800, 6000, 9000, 13000, 18200, 24800, 33000, 42700, 53900, 66600, 80800,
        96500, 115735, 134435, 154635, 176335, 199535,
    ];
    let mut level: u8 = 1;
    for (idx, threshold) in THRESHOLDS.iter().enumerate() {
        if xp >= *threshold {
            level = (idx as u8) + 1;
        } else {
            break;
        }
    }
    level
}

/// CRC16-CCITT/FALSE (poly 0x1021, init 0xFFFF, no reflect, no xorout).
/// Used for both the header CRC and the per-area checksums.
pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

// ---------------------------------------------------------------------------
// Public output type
// ---------------------------------------------------------------------------

/// Everything the parser managed to pull out of a `.sky` blob.
///
/// For non-standard kinds (every `FigureKind` other than `Standard`) only the header fields and
/// variant decomposition are meaningful — payload fields stay at their
/// defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkyFigureStats {
    // --- Header ---------------------------------------------------------
    /// Activision-assigned toy type ("figure id"), 24-bit. Newtyped via
    /// [`skylander_core::ToyTypeId`] (PLAN 6.6.1b) so it can't be mixed
    /// up with the library-level `FigureId` string or other u32 fields.
    pub figure_id: skylander_core::ToyTypeId,
    /// Raw variant word as stored on the tag. Use
    /// [`skylander_core::TagVariant::mask_to_identity`] before dedup.
    pub variant: skylander_core::TagVariant,
    /// Decomposition of the variant word.
    pub variant_decoded: VariantInfo,
    /// Non-unique identifier / serial from header byte 0x00 (uint32 LE).
    pub serial: u32,
    /// Trading card ID from header bytes 0x14..0x1C.
    pub trading_card_id: u64,
    /// Base-29 web code derived from [`Self::trading_card_id`].
    pub web_code: String,
    /// Error byte (header 0x13). Non-zero makes games reject the figure.
    pub error_byte: u8,
    /// Classification of the data-area layout.
    pub figure_kind: FigureKind,

    // --- Active data area (standard layout) ----------------------------
    /// 2011-era XP (SSA / Giants). u24 LE at block 0x08/0x24 offset 0x00.
    pub xp_2011: u32,
    /// 2012-era XP (Swap Force). u16 LE at block 0x11/0x2D offset 0x03.
    pub xp_2012: u16,
    /// 2013-era XP (Trap Team / SuperChargers). u32 LE at block 0x11/0x2D
    /// offset 0x08.
    pub xp_2013: u32,
    /// Character level (1..=20) derived from whichever XP pool matches the
    /// figure's year code. Defaults to 1.
    pub level: u8,
    /// Gold coins (u16 LE at block 0x08/0x24 offset 0x03).
    pub gold: u16,
    /// Playtime in seconds (u32 LE at block 0x08/0x24 offset 0x05).
    pub playtime_secs: u32,
    /// Player-chosen nickname, null-terminated UTF-16 LE.
    pub nickname: String,
    /// Hero points (u16 LE at block 0x0D/0x29 offset 0x0A).
    pub hero_points: u16,
    /// Resolved current hat per the spec's per-year lookup algorithm.
    pub hat_current: HatId,
    /// Hat history `[2011, 2012, 2013, 2015]`.
    pub hat_history: [HatId; 4],
    /// Trinket (u8 at block 0x11/0x2D offset 0x0D).
    pub trinket: u8,
    /// Timestamp of last portal placement (None if the tag has no placement
    /// record yet).
    pub last_placed: Option<NaiveDateTime>,
    /// Timestamp of last reset (or first-ever placement if never reset).
    pub last_reset: Option<NaiveDateTime>,
    /// SSA Heroic Challenges bitfield (u32 at block 0x0D/0x29 offset 0x06).
    pub heroic_challenges_ssa: u32,
    /// Giants Heroic Challenges bitfield (u24 at block 0x12/0x2E offset 0x04).
    pub heroic_challenges_sg: u32,
    /// Battlegrounds flags (u32 at block 0x12/0x2E offset 0x00).
    pub battlegrounds_flags: u32,
    /// Giants quests (72-bit, stored little-endian in 9 bytes).
    pub quests_giants: u128,
    /// Swap Force quests (72-bit, stored little-endian in 9 bytes).
    pub quests_swap_force: u128,

    // --- Validation ----------------------------------------------------
    /// True if the header + active-area checksums all match their stored
    /// values. False means the tag is damaged, tampered with, or the layout
    /// mismatch is real (e.g. a Trap file parsed as Standard).
    pub checksums_valid: bool,

    /// Best-effort mapping of year code to source game. Mirrors
    /// `variant_decoded.year_code` for backwards compatibility.
    pub source_game_gen: SkyGeneration,

    /// All 64 blocks verbatim. Useful for debug dumps and test fixtures.
    /// Intentionally **not** surfaced by the server's public JSON endpoints.
    pub raw_blocks: Vec<[u8; BLOCK_LEN]>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Absolute file offset of the first byte of `block`.
#[inline]
pub const fn block_off(block: usize) -> usize {
    block * BLOCK_LEN
}

fn read_u16(bytes: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([bytes[off], bytes[off + 1]])
}
fn read_u32(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
}
fn read_u24(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], 0])
}
fn read_u72(bytes: &[u8], off: usize) -> u128 {
    // 9 bytes LE -> u128.
    let mut buf = [0u8; 16];
    buf[..9].copy_from_slice(&bytes[off..off + 9]);
    u128::from_le_bytes(buf)
}

/// Decode the 32-byte nickname region, auto-detecting which encoding the
/// figure used. Standard figures store UTF-16 LE (`'S' 00 'n' 00 'a' 00
/// 'p' 00 …` for "Snap Shot"); Creation Crystals and some custom-named
/// variants store densely-packed single-byte ASCII (`'D' 'E' 'L' 'F' 'O'
/// 'X' 00 00 …` for "DELFOX"). Feeding ASCII through the UTF-16 path
/// produces CJK mojibake (`䕄䙌塏`), so we pick based on byte shape.
///
/// Heuristic: inspect the populated run (bytes before the first null,
/// or the whole buffer if there is none). If every byte at an odd index
/// is `0x00`, treat the buffer as UTF-16 LE — that's the distinguishing
/// signature of ASCII-codepoints-as-UTF-16. Otherwise, treat it as
/// packed ASCII. A buffer with a 1-char string (`'S' 00 00 00 …`) is
/// ambiguous but resolves to UTF-16 LE "S" either way, since the ASCII
/// path also stops at the first null and produces the same single char.
///
/// Fallback for non-ASCII bytes in the packed path (rare — would imply
/// Latin-1 or other 8-bit encoding): pass through `char::from` per byte
/// so we don't lose information, but the string may render oddly.
fn decode_nickname(bytes: &[u8]) -> String {
    // Find the populated run length. For UTF-16 LE it terminates at the
    // first `00 00` word pair; for packed ASCII it terminates at the
    // first `00` byte. Using "first null byte" is a superset that
    // covers both without false positives.
    let populated_len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    if populated_len == 0 {
        return String::new();
    }

    let looks_like_utf16_le = bytes[..populated_len]
        .iter()
        .enumerate()
        .all(|(i, &b)| i % 2 == 0 || b == 0);

    if looks_like_utf16_le {
        let mut words = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            let w = u16::from_le_bytes([chunk[0], chunk[1]]);
            if w == 0 {
                break;
            }
            words.push(w);
        }
        String::from_utf16_lossy(&words)
    } else {
        bytes[..populated_len]
            .iter()
            .map(|&b| char::from(b))
            .collect()
    }
}

/// Is `a` "higher than" `b` under wraparound per `SkylanderFormat.md` "Area
/// Sequence" — `a == b + 1 (mod 256)` makes `a` the newer area.
fn seq_newer(a: u8, b: u8) -> bool {
    a.wrapping_sub(b) == 1
}

/// Decode a 6-byte minute/hour/day/month/year-LE timestamp into a
/// `NaiveDateTime`. Returns None if the fields are zero (tag never placed) or
/// if the decoded values are out of range.
fn decode_timestamp(minute: u8, hour: u8, day: u8, month: u8, year: u16) -> Option<NaiveDateTime> {
    if year == 0 && month == 0 && day == 0 && hour == 0 && minute == 0 {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
        .and_then(|d| d.and_hms_opt(hour as u32, minute as u32, 0))
}

/// Resolve the current hat per `SkylanderFormat.md` "Hat value" lookup
/// algorithm. `year_code` is the figure's game-of-origin.
fn resolve_current_hat(history: &[HatId; 4], year_code: SkyGeneration) -> HatId {
    // history = [2011, 2012, 2013, 2015]
    // Giants & Trap Team: oldest-first.
    // Swap Force / SuperChargers / Imaginators: newest-first.
    // SSA: only the 2011 slot is meaningful.
    let ordered: [HatId; 4] = match year_code {
        SkyGeneration::SpyrosAdventure | SkyGeneration::Giants | SkyGeneration::TrapTeam => {
            *history
        }
        SkyGeneration::SwapForce | SkyGeneration::SuperChargers | SkyGeneration::Imaginators => {
            [history[3], history[2], history[1], history[0]]
        }
        SkyGeneration::Unknown => *history,
    };
    for h in ordered {
        if h != HatId::NONE {
            return h;
        }
    }
    HatId::NONE
}

// ---------------------------------------------------------------------------
// Parser entry point
// ---------------------------------------------------------------------------

/// Parse a raw `.sky` blob.
pub fn parse(bytes: &[u8]) -> Result<SkyFigureStats, ParseError> {
    if bytes.len() != SKY_FILE_LEN {
        return Err(ParseError::BadLength(bytes.len()));
    }

    // Real `.sky` dumps are always encrypted — decrypt unconditionally. Test
    // fixtures run their synthetic plaintext through `encrypt_figure` inside
    // `Fixture::build()` so this path is the single source of truth for how
    // payloads get read.
    let mut buf = [0u8; SKY_FILE_LEN];
    buf.copy_from_slice(bytes);
    decrypt_figure(&mut buf);
    let bytes: &[u8] = &buf;

    // Split into 64 fixed-size blocks.
    let mut raw_blocks = Vec::with_capacity(BLOCK_COUNT);
    for chunk in bytes.chunks_exact(BLOCK_LEN) {
        let mut b = [0u8; BLOCK_LEN];
        b.copy_from_slice(chunk);
        raw_blocks.push(b);
    }

    // --- Header -------------------------------------------------------
    let serial = read_u32(bytes, OFFSET_SERIAL);
    let figure_id = read_u24(bytes, OFFSET_FIGURE_ID);
    let error_byte = bytes[OFFSET_ERROR_BYTE];
    // Trading card: 2 × u32 LE (the spec notes it's split to dodge alignment).
    let tc_lo = read_u32(bytes, OFFSET_TRADING_CARD) as u64;
    let tc_hi = read_u32(bytes, OFFSET_TRADING_CARD + 4) as u64;
    let trading_card_id = (tc_hi << 32) | tc_lo;
    let variant_raw = read_u16(bytes, OFFSET_VARIANT);
    let header_crc_stored = read_u16(bytes, OFFSET_HEADER_CRC);
    let header_crc_computed = crc16_ccitt_false(&bytes[0..OFFSET_HEADER_CRC]);

    let variant_decoded = VariantInfo::from_raw(variant_raw);
    let web_code = web_code_from_trading_card(trading_card_id);

    // --- Figure kind classification ----------------------------------
    // Ranges pinned against real RPCS3 dumps from `dev-data/validation-figures/`
    // (see PLAN 6.2.0 validation + the `validate_samples` example). The old
    // community-sourced ranges (Traps at 0x1F4..=0x225, Vehicles at
    // 0x2BC..=0x2ED, CYOS at 0x320..=0x383) don't match what RPCS3 actually
    // writes — all 151 real samples classified as Standard under those
    // ranges. What we've observed:
    //
    //   0x0D2..=0x0DC → Trap crystals. 11 values, one per element (Magic
    //                    through Kaos). Confirmed against 11 element traps
    //                    + 10 villain-named trap dumps (villain-loaded
    //                    traps keep the trap's element-encoded figure_id
    //                    and stash the captured villain in the payload —
    //                    see spec §"Trap" 0x0010 `VillainType`).
    //   0x0C8x..=0x0CAx → mixed SuperChargers-era: characters (e.g. Sheep
    //                    Creep 0x0C82) AND vehicles (Hot Streak 0x0C98,
    //                    Reef Ripper 0x0C96, Sun Runner 0x0CA4). Vehicle
    //                    vs character disambiguation can't be done on
    //                    figure_id alone from the samples we have; needs
    //                    either a lookup table or a variant-bit check.
    //                    Deferred to a follow-up plan item.
    //   CYOS (Creation Crystal) range: unknown. Our sample dir's
    //                    `creation crystal/` folder was empty when 6.2.0
    //                    ran; range will be pinned when Chris provides CC
    //                    dumps.
    //
    // TODO(PLAN 6.2.x vehicle-range): split Vehicle from Standard in the
    // 0xC8x..=0xCAx block.
    // TODO(PLAN 6.2.x cyos-range): verify CYOS range against real dumps.
    let figure_kind = match figure_id {
        0x0D2..=0x0DC => FigureKind::Trap,
        _ => FigureKind::Standard,
    };

    // Default-initialised; only filled for Standard.
    let mut stats = SkyFigureStats {
        figure_id: skylander_core::ToyTypeId::new(figure_id),
        variant: skylander_core::TagVariant::new(variant_raw),
        variant_decoded,
        serial,
        trading_card_id,
        web_code,
        error_byte,
        figure_kind,
        xp_2011: 0,
        xp_2012: 0,
        xp_2013: 0,
        level: 1,
        gold: 0,
        playtime_secs: 0,
        nickname: String::new(),
        hero_points: 0,
        hat_current: HatId::NONE,
        hat_history: [HatId::NONE; 4],
        trinket: 0,
        last_placed: None,
        last_reset: None,
        heroic_challenges_ssa: 0,
        heroic_challenges_sg: 0,
        battlegrounds_flags: 0,
        quests_giants: 0,
        quests_swap_force: 0,
        checksums_valid: header_crc_stored == header_crc_computed,
        source_game_gen: variant_decoded.year_code,
        raw_blocks,
    };

    if matches!(figure_kind, FigureKind::Standard) {
        parse_standard(bytes, &mut stats);
    }

    Ok(stats)
}

/// Fill the Standard-layout fields on `stats` from `bytes`.
fn parse_standard(bytes: &[u8], stats: &mut SkyFigureStats) {
    // Area-sequence bytes live at block 0x08 offset 0x09 and block 0x24
    // offset 0x09 — these govern the "first" data region (blocks 0x08..=0x10
    // and mirror 0x24..=0x2C).
    let seq_a0 = bytes[block_off(0x08) + 0x09];
    let seq_a1 = bytes[block_off(0x24) + 0x09];
    // area_base is the starting block for the live "first" region.
    let (area_base_a, _other_base_a) = if seq_newer(seq_a1, seq_a0) {
        (0x24, 0x08)
    } else {
        (0x08, 0x24)
    };

    // Second data region (blocks 0x11..=0x15 and mirror 0x2D..=0x31) has its
    // own sequence at block 0x11/0x2D offset 0x02 (absolute struct offset
    // 0x72 inside tfbSpyroTagData).
    let seq_b0 = bytes[block_off(0x11) + 0x02];
    let seq_b1 = bytes[block_off(0x2D) + 0x02];
    let (area_base_b, _other_base_b) = if seq_newer(seq_b1, seq_b0) {
        (0x2D, 0x11)
    } else {
        (0x11, 0x2D)
    };

    // Offsets within Region A: blocks 0x08, 0x09, 0x0A, 0x0C, 0x0D, 0x0E.
    // Region B mirrors with +0x1C: 0x24, 0x25, 0x26, 0x28, 0x29, 0x2A.
    let delta_a: isize = area_base_a as isize - 0x08;
    let b08 = block_off((0x08_isize + delta_a) as usize);
    let b09 = block_off((0x09_isize + delta_a) as usize);
    let b0a = block_off((0x0A_isize + delta_a) as usize);
    let b0c = block_off((0x0C_isize + delta_a) as usize);
    let b0d = block_off((0x0D_isize + delta_a) as usize);
    let b0e = block_off((0x0E_isize + delta_a) as usize);

    // Region B offsets: blocks 0x11, 0x12, 0x14.
    let delta_b: isize = area_base_b as isize - 0x11;
    let b11 = block_off((0x11_isize + delta_b) as usize);
    let b12 = block_off((0x12_isize + delta_b) as usize);
    let b14 = block_off((0x14_isize + delta_b) as usize);

    // --- Block 0x08/0x24 --------------------------------------------
    stats.xp_2011 = read_u24(bytes, b08 + 0x00);
    stats.gold = read_u16(bytes, b08 + 0x03);
    stats.playtime_secs = read_u32(bytes, b08 + 0x05);

    // --- Nickname (wchar_t[8] at block 0x0A + wchar_t[8] at block 0x0C) ---
    let mut nick_buf = Vec::with_capacity(32);
    nick_buf.extend_from_slice(&bytes[b0a..b0a + 16]);
    nick_buf.extend_from_slice(&bytes[b0c..b0c + 16]);
    stats.nickname = decode_nickname(&nick_buf);

    // --- Block 0x0D/0x29 --------------------------------------------
    stats.heroic_challenges_ssa = read_u32(bytes, b0d + 0x06);
    stats.hero_points = read_u16(bytes, b0d + 0x0A);

    // last_placed stored in block 0x0D offsets 0x00..0x06.
    let lp_min = bytes[b0d + 0x00];
    let lp_hr = bytes[b0d + 0x01];
    let lp_day = bytes[b0d + 0x02];
    let lp_mon = bytes[b0d + 0x03];
    let lp_year = read_u16(bytes, b0d + 0x04);
    stats.last_placed = decode_timestamp(lp_min, lp_hr, lp_day, lp_mon, lp_year);

    // last_reset stored in block 0x0E offsets 0x00..0x06.
    let lr_min = bytes[b0e + 0x00];
    let lr_hr = bytes[b0e + 0x01];
    let lr_day = bytes[b0e + 0x02];
    let lr_mon = bytes[b0e + 0x03];
    let lr_year = read_u16(bytes, b0e + 0x04);
    stats.last_reset = decode_timestamp(lr_min, lr_hr, lr_day, lr_mon, lr_year);

    // --- Hats --------------------------------------------------------
    let hat_2011 = HatId(read_u16(bytes, b09 + 0x04));
    let hat_2012 = HatId(bytes[b11 + 0x05] as u16);
    let hat_2013 = HatId(bytes[b11 + 0x0C] as u16);
    // Per spec: the 2015 hat byte needs +256 to get the true hat id.
    let hat_2015_raw = bytes[b11 + 0x0E];
    let hat_2015 = if hat_2015_raw == 0 {
        HatId::NONE
    } else {
        HatId(hat_2015_raw as u16 + 256)
    };
    stats.hat_history = [hat_2011, hat_2012, hat_2013, hat_2015];
    stats.hat_current = resolve_current_hat(&stats.hat_history, stats.variant_decoded.year_code);

    // --- Block 0x11/0x2D --------------------------------------------
    stats.xp_2012 = read_u16(bytes, b11 + 0x03);
    stats.xp_2013 = read_u32(bytes, b11 + 0x08);
    stats.trinket = bytes[b11 + 0x0D];

    // --- Block 0x12/0x2E --------------------------------------------
    stats.battlegrounds_flags = read_u32(bytes, b12 + 0x00);
    stats.heroic_challenges_sg = read_u24(bytes, b12 + 0x04);
    stats.quests_giants = read_u72(bytes, b12 + 0x07);
    // Swap Force quests start at block 0x14 offset 0x07 per spec.
    stats.quests_swap_force = read_u72(bytes, b14 + 0x07);

    // --- Level from XP (pick the pool matching the year of origin) ---
    let xp_for_level = match stats.variant_decoded.year_code {
        SkyGeneration::SpyrosAdventure | SkyGeneration::Giants => stats.xp_2011,
        SkyGeneration::SwapForce => stats.xp_2011 + stats.xp_2012 as u32,
        SkyGeneration::TrapTeam | SkyGeneration::SuperChargers | SkyGeneration::Imaginators => {
            stats.xp_2011 + stats.xp_2012 as u32 + stats.xp_2013
        }
        SkyGeneration::Unknown => stats.xp_2011,
    };
    stats.level = level_from_xp(xp_for_level);

    // --- Checksums ---------------------------------------------------
    // The per-area checksum layout is complex; we check the "0x30 bytes from
    // 0x10" checksum (stored at block 0x08/0x24 offset 0x0C) and the
    // top-level struct CRC at 0x0E. See `SkylanderFormat.md` "None of the
    // above data structures" for the exact byte ranges.
    //
    // 0x30 bytes starting from struct offset 0x10 spans blocks 0x09, 0x0A,
    // 0x0C (skipping sector trailer 0x0B). We stitch those three blocks.
    let mut c30 = Vec::with_capacity(0x30);
    c30.extend_from_slice(&bytes[b09..b09 + 16]);
    c30.extend_from_slice(&bytes[b0a..b0a + 16]);
    c30.extend_from_slice(&bytes[b0c..b0c + 16]);
    let crc30_stored = read_u16(bytes, b08 + 0x0C);
    let crc30_computed = crc16_ccitt_false(&c30);

    // The 14-byte struct-header CRC at offset 0x0E is over the first 14 bytes
    // of this struct followed by the bytes "05 00" (see spec).
    let mut c14 = Vec::with_capacity(16);
    c14.extend_from_slice(&bytes[b08..b08 + 14]);
    c14.extend_from_slice(&[0x05, 0x00]);
    let crc14_stored = read_u16(bytes, b08 + 0x0E);
    let crc14_computed = crc16_ccitt_false(&c14);

    stats.checksums_valid =
        stats.checksums_valid && crc30_stored == crc30_computed && crc14_stored == crc14_computed;
}

// ---------------------------------------------------------------------------
// Test fixture generator + tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(missing_docs)]
mod fixture {
    //! Synthetic `.sky` blob builder for tests. Produces a 1024-byte blob
    //! with whatever standard-layout fields you care to set, and writes
    //! correct CRC16/CCITT-FALSE checksums so [`super::parse`] reports
    //! `checksums_valid: true`.
    use super::{
        BLOCK_LEN, OFFSET_ERROR_BYTE, OFFSET_FIGURE_ID, OFFSET_HEADER_CRC, OFFSET_SERIAL,
        OFFSET_TRADING_CARD, OFFSET_VARIANT, SKY_FILE_LEN, block_off, crc16_ccitt_false,
        encrypt_figure,
    };

    pub struct Fixture {
        pub serial: u32,
        pub figure_id: u32,
        pub error_byte: u8,
        pub trading_card_id: u64,
        pub variant: u16,
        // Standard-layout payload (region A at block 0x08).
        pub xp_2011: u32,
        pub gold: u16,
        pub playtime_secs: u32,
        pub area_seq_a0: u8,
        pub area_seq_a1: u8,
        pub area_seq_b0: u8,
        pub area_seq_b1: u8,
        pub nickname: String,
        pub hero_points: u16,
        pub heroic_challenges_ssa: u32,
        pub hat_2011: u16,
        pub hat_2012: u8,
        pub hat_2013: u8,
        pub hat_2015: u8,
        pub xp_2012: u16,
        pub xp_2013: u32,
        pub trinket: u8,
        pub battlegrounds_flags: u32,
        pub heroic_challenges_sg: u32,
        pub quests_giants: u128,
        pub quests_swap_force: u128,
        pub last_placed: Option<(u8, u8, u8, u8, u16)>, // (min, hr, day, mon, year)
        pub last_reset: Option<(u8, u8, u8, u8, u16)>,
    }

    impl Default for Fixture {
        fn default() -> Self {
            Self {
                serial: 0,
                figure_id: 0,
                error_byte: 0,
                trading_card_id: 0,
                variant: 0,
                xp_2011: 0,
                gold: 0,
                playtime_secs: 0,
                // seq_a0 = 1 so it's "newer" than seq_a1 = 0 → active region = 0x08.
                area_seq_a0: 1,
                area_seq_a1: 0,
                area_seq_b0: 1,
                area_seq_b1: 0,
                nickname: String::new(),
                hero_points: 0,
                heroic_challenges_ssa: 0,
                hat_2011: 0,
                hat_2012: 0,
                hat_2013: 0,
                hat_2015: 0,
                xp_2012: 0,
                xp_2013: 0,
                trinket: 0,
                battlegrounds_flags: 0,
                heroic_challenges_sg: 0,
                quests_giants: 0,
                quests_swap_force: 0,
                last_placed: None,
                last_reset: None,
            }
        }
    }

    impl Fixture {
        pub fn build(&self) -> Vec<u8> {
            let mut buf = vec![0u8; SKY_FILE_LEN];
            // Header.
            buf[OFFSET_SERIAL..OFFSET_SERIAL + 4].copy_from_slice(&self.serial.to_le_bytes());
            buf[OFFSET_FIGURE_ID..OFFSET_FIGURE_ID + 3]
                .copy_from_slice(&self.figure_id.to_le_bytes()[..3]);
            buf[OFFSET_ERROR_BYTE] = self.error_byte;
            buf[OFFSET_TRADING_CARD..OFFSET_TRADING_CARD + 8]
                .copy_from_slice(&self.trading_card_id.to_le_bytes());
            buf[OFFSET_VARIANT..OFFSET_VARIANT + 2].copy_from_slice(&self.variant.to_le_bytes());
            // Header CRC over first 0x1E bytes.
            let hdr_crc = crc16_ccitt_false(&buf[0..OFFSET_HEADER_CRC]);
            buf[OFFSET_HEADER_CRC..OFFSET_HEADER_CRC + 2].copy_from_slice(&hdr_crc.to_le_bytes());

            // Pick the active region given the sequence pairs.
            let area_a_base = if self.area_seq_a1.wrapping_sub(self.area_seq_a0) == 1 {
                0x24
            } else {
                0x08
            };
            let area_b_base = if self.area_seq_b1.wrapping_sub(self.area_seq_b0) == 1 {
                0x2D
            } else {
                0x11
            };
            self.write_region_a(&mut buf, area_a_base);
            self.write_region_b(&mut buf, area_b_base);

            // Write the area-sequence bytes themselves to both mirrors so
            // parse picks the right one.
            buf[block_off(0x08) + 0x09] = self.area_seq_a0;
            buf[block_off(0x24) + 0x09] = self.area_seq_a1;
            buf[block_off(0x11) + 0x02] = self.area_seq_b0;
            buf[block_off(0x2D) + 0x02] = self.area_seq_b1;

            // Recompute the active area CRCs.
            write_region_a_crcs(&mut buf, area_a_base);

            debug_assert_eq!(buf.len(), SKY_FILE_LEN);
            debug_assert_eq!(buf.len() / BLOCK_LEN, super::BLOCK_COUNT);

            // Encrypt so `parse()` runs the full decrypt path end-to-end. The
            // fixture is the test-side canonical "plaintext synthetic"; the
            // ciphertext that comes out is what RPCS3 would write.
            let mut arr = [0u8; SKY_FILE_LEN];
            arr.copy_from_slice(&buf);
            encrypt_figure(&mut arr);
            arr.to_vec()
        }

        fn write_region_a(&self, buf: &mut [u8], base: usize) {
            let delta: isize = base as isize - 0x08;
            let b08 = block_off((0x08_isize + delta) as usize);
            let b09 = block_off((0x09_isize + delta) as usize);
            let b0a = block_off((0x0A_isize + delta) as usize);
            let b0c = block_off((0x0C_isize + delta) as usize);
            let b0d = block_off((0x0D_isize + delta) as usize);
            let b0e = block_off((0x0E_isize + delta) as usize);

            buf[b08 + 0x00..b08 + 0x03].copy_from_slice(&self.xp_2011.to_le_bytes()[..3]);
            buf[b08 + 0x03..b08 + 0x05].copy_from_slice(&self.gold.to_le_bytes());
            buf[b08 + 0x05..b08 + 0x09].copy_from_slice(&self.playtime_secs.to_le_bytes());

            buf[b09 + 0x04..b09 + 0x06].copy_from_slice(&self.hat_2011.to_le_bytes());

            // Nickname into blocks 0x0A + 0x0C (wchar_t[8] each = 16 bytes).
            let utf16: Vec<u16> = self.nickname.encode_utf16().collect();
            for (i, &w) in utf16.iter().take(16).enumerate() {
                let b = if i < 8 { b0a } else { b0c };
                let off = b + (i % 8) * 2;
                buf[off..off + 2].copy_from_slice(&w.to_le_bytes());
            }

            buf[b0d + 0x06..b0d + 0x0A].copy_from_slice(&self.heroic_challenges_ssa.to_le_bytes());
            buf[b0d + 0x0A..b0d + 0x0C].copy_from_slice(&self.hero_points.to_le_bytes());
            if let Some((mi, hr, d, mo, y)) = self.last_placed {
                buf[b0d + 0x00] = mi;
                buf[b0d + 0x01] = hr;
                buf[b0d + 0x02] = d;
                buf[b0d + 0x03] = mo;
                buf[b0d + 0x04..b0d + 0x06].copy_from_slice(&y.to_le_bytes());
            }
            if let Some((mi, hr, d, mo, y)) = self.last_reset {
                buf[b0e + 0x00] = mi;
                buf[b0e + 0x01] = hr;
                buf[b0e + 0x02] = d;
                buf[b0e + 0x03] = mo;
                buf[b0e + 0x04..b0e + 0x06].copy_from_slice(&y.to_le_bytes());
            }
        }

        fn write_region_b(&self, buf: &mut [u8], base: usize) {
            let delta: isize = base as isize - 0x11;
            let b11 = block_off((0x11_isize + delta) as usize);
            let b12 = block_off((0x12_isize + delta) as usize);
            let b14 = block_off((0x14_isize + delta) as usize);

            buf[b11 + 0x03..b11 + 0x05].copy_from_slice(&self.xp_2012.to_le_bytes());
            buf[b11 + 0x05] = self.hat_2012;
            buf[b11 + 0x08..b11 + 0x0C].copy_from_slice(&self.xp_2013.to_le_bytes());
            buf[b11 + 0x0C] = self.hat_2013;
            buf[b11 + 0x0D] = self.trinket;
            buf[b11 + 0x0E] = self.hat_2015;

            buf[b12 + 0x00..b12 + 0x04].copy_from_slice(&self.battlegrounds_flags.to_le_bytes());
            let sg = self.heroic_challenges_sg.to_le_bytes();
            buf[b12 + 0x04..b12 + 0x07].copy_from_slice(&sg[..3]);

            // Giants quests — u72 at b12+0x07.
            let qg = self.quests_giants.to_le_bytes();
            buf[b12 + 0x07..b12 + 0x10].copy_from_slice(&qg[..9]);
            // Swap Force quests — u72 at b14+0x07.
            let qsf = self.quests_swap_force.to_le_bytes();
            buf[b14 + 0x07..b14 + 0x10].copy_from_slice(&qsf[..9]);
        }
    }

    /// Recompute the two CRCs in region A (the 0x30-byte and 14-byte ones)
    /// so that parse(...) sees a valid area.
    fn write_region_a_crcs(buf: &mut [u8], base: usize) {
        let delta: isize = base as isize - 0x08;
        let b08 = block_off((0x08_isize + delta) as usize);
        let b09 = block_off((0x09_isize + delta) as usize);
        let b0a = block_off((0x0A_isize + delta) as usize);
        let b0c = block_off((0x0C_isize + delta) as usize);

        // 0x30-byte CRC.
        let mut c30 = Vec::with_capacity(0x30);
        c30.extend_from_slice(&buf[b09..b09 + 16]);
        c30.extend_from_slice(&buf[b0a..b0a + 16]);
        c30.extend_from_slice(&buf[b0c..b0c + 16]);
        let crc30 = crc16_ccitt_false(&c30);
        buf[b08 + 0x0C..b08 + 0x0E].copy_from_slice(&crc30.to_le_bytes());

        // 14-byte + "05 00" CRC (note: b08 range must be written AFTER the
        // other region-A fields but BEFORE this CRC is written).
        let mut c14 = Vec::with_capacity(16);
        c14.extend_from_slice(&buf[b08..b08 + 14]);
        c14.extend_from_slice(&[0x05, 0x00]);
        let crc14 = crc16_ccitt_false(&c14);
        buf[b08 + 0x0E..b08 + 0x10].copy_from_slice(&crc14.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::Fixture;
    use super::*;

    #[test]
    fn rejects_short_input() {
        let err = parse(&[0u8; 512]).unwrap_err();
        assert!(matches!(err, ParseError::BadLength(512)));
    }

    #[test]
    fn rejects_long_input() {
        let err = parse(&[0u8; 2048]).unwrap_err();
        assert!(matches!(err, ParseError::BadLength(2048)));
    }

    #[test]
    fn decode_nickname_utf16_le_standard() {
        // "Snap Shot" — what Standard figures store: ASCII codepoints
        // widened to UTF-16 LE with null high bytes.
        let bytes: Vec<u8> = "Snap Shot"
            .chars()
            .flat_map(|c| [c as u8, 0])
            .chain(std::iter::repeat_n(0, 32 - 18))
            .collect();
        assert_eq!(decode_nickname(&bytes), "Snap Shot");
    }

    #[test]
    fn decode_nickname_ascii_packed_creation_crystal() {
        // CCs pack the user-chosen name as single-byte ASCII instead of
        // UTF-16 LE. Before the auto-detect, this decoded as CJK
        // mojibake ("䕄䙌塏" for "DELFOX"). Exact raw bytes observed in
        // the live scan 7FC1ADA3.sky at block 0x0A+0x0C.
        let mut bytes = [0u8; 32];
        bytes[..6].copy_from_slice(b"DELFOX");
        assert_eq!(decode_nickname(&bytes), "DELFOX");
    }

    #[test]
    fn decode_nickname_empty() {
        assert_eq!(decode_nickname(&[0u8; 32]), "");
    }

    #[test]
    fn decode_nickname_single_char_ambiguous_resolves_to_utf16_path() {
        // A 1-char string `'A' 00 00 00 …` satisfies both heuristics.
        // Either path produces "A"; the UTF-16 LE branch wins by the
        // "every odd byte is 0" rule. Test pins the behaviour.
        let mut bytes = [0u8; 32];
        bytes[0] = b'A';
        assert_eq!(decode_nickname(&bytes), "A");
    }

    #[test]
    fn parses_header_identity_roundtrip() {
        let bytes = Fixture {
            serial: 0xDEADBEEF,
            figure_id: 0x000005, // Trigger Happy
            error_byte: 0,
            trading_card_id: 0x1234_5678_9ABC_DEF0,
            variant: 0x1000, // year code = SSA
            ..Default::default()
        }
        .build();
        let stats = parse(&bytes).unwrap();
        assert_eq!(stats.serial, 0xDEADBEEF);
        assert_eq!(stats.figure_id, skylander_core::ToyTypeId(0x000005));
        assert_eq!(stats.trading_card_id, 0x1234_5678_9ABC_DEF0);
        assert_eq!(
            stats.variant_decoded.year_code,
            SkyGeneration::SpyrosAdventure
        );
        assert_eq!(stats.source_game_gen, SkyGeneration::SpyrosAdventure);
        assert_eq!(stats.figure_kind, FigureKind::Standard);
        assert_eq!(stats.raw_blocks.len(), BLOCK_COUNT);
        assert!(stats.checksums_valid);
    }

    #[test]
    fn variant_decomposition_matches_spec_bitfield() {
        // raw = 0xFFAB:
        //   deco = 0xAB (low 8 bits)
        //   bits 8..12 = 0xF → all four flags set
        //   year code = 0xF → Unknown
        let v = VariantInfo::from_raw(0xFFAB);
        assert_eq!(v.deco_id, 0xAB);
        assert!(v.is_supercharger);
        assert!(v.is_lightcore);
        assert!(v.is_in_game_variant);
        assert!(v.is_reposed);
        assert_eq!(v.year_code, SkyGeneration::Unknown);

        // year = 2 (Giants), reposed = 0
        let v2 = VariantInfo::from_raw(0x2000);
        assert_eq!(v2.year_code, SkyGeneration::Giants);
        assert!(!v2.is_reposed);
    }

    #[test]
    fn web_code_base29_lookup() {
        // Two trivial cases anchored on the alphabet.
        assert_eq!(web_code_from_trading_card(0), "2");
        assert_eq!(web_code_from_trading_card(1), "3");
        assert_eq!(web_code_from_trading_card(28), "Z");
        // 29 -> "32" (carry).
        assert_eq!(web_code_from_trading_card(29), "32");
        // 30 -> "33".
        assert_eq!(web_code_from_trading_card(30), "33");
    }

    #[test]
    fn experience_table_boundaries() {
        assert_eq!(level_from_xp(0), 1);
        assert_eq!(level_from_xp(999), 1);
        assert_eq!(level_from_xp(1000), 2);
        assert_eq!(level_from_xp(33000), 10);
        assert_eq!(level_from_xp(199_534), 19);
        assert_eq!(level_from_xp(199_535), 20);
        assert_eq!(level_from_xp(u32::MAX), 20);
    }

    #[test]
    fn parses_standard_payload_roundtrip() {
        let bytes = Fixture {
            figure_id: 0x000005,
            variant: 0x1000, // SSA
            xp_2011: 5000,
            gold: 1234,
            playtime_secs: 3600,
            nickname: "Spyro".to_string(),
            hero_points: 42,
            heroic_challenges_ssa: 0xDEAD_BEEF,
            hat_2011: 9, // Straw Hat
            xp_2012: 0,
            xp_2013: 0,
            trinket: 0x1F,
            battlegrounds_flags: 0xCAFEBABE,
            heroic_challenges_sg: 0x00ABCDEF,
            // u72 values — fit in 9 bytes (max 0xFF_FFFF_FFFF_FFFF_FFFF).
            quests_giants: 0x12_3456_789A_BCDE_F012,
            quests_swap_force: 0xAA_BBCC_DDEE_FF00_1122,
            ..Default::default()
        }
        .build();

        let stats = parse(&bytes).unwrap();
        assert!(stats.checksums_valid);
        assert_eq!(stats.xp_2011, 5000);
        assert_eq!(stats.gold, 1234);
        assert_eq!(stats.playtime_secs, 3600);
        assert_eq!(stats.nickname, "Spyro");
        assert_eq!(stats.hero_points, 42);
        assert_eq!(stats.heroic_challenges_ssa, 0xDEAD_BEEF);
        assert_eq!(stats.hat_history[0], HatId(9));
        // SSA uses oldest-first lookup → current = 2011 hat = 9.
        assert_eq!(stats.hat_current, HatId(9));
        assert_eq!(stats.trinket, 0x1F);
        assert_eq!(stats.battlegrounds_flags, 0xCAFEBABE);
        assert_eq!(stats.heroic_challenges_sg, 0x00ABCDEF);
        assert_eq!(stats.quests_giants, 0x12_3456_789A_BCDE_F012);
        assert_eq!(stats.quests_swap_force, 0xAA_BBCC_DDEE_FF00_1122);
        // SSA level from 5000 xp → level 4.
        assert_eq!(stats.level, 4);
    }

    #[test]
    fn hat_lookup_giants_prefers_oldest() {
        // Giants: oldest-first (2011 → 2012 → 2013 → 2015). If 2011 = 0 but
        // 2012 = 5, current should be 5.
        let bytes = Fixture {
            variant: 0x2000, // Giants
            hat_2011: 0,
            hat_2012: 5,
            hat_2013: 7,
            hat_2015: 9,
            ..Default::default()
        }
        .build();
        let stats = parse(&bytes).unwrap();
        assert_eq!(stats.hat_current, HatId(5));
    }

    #[test]
    fn hat_lookup_swapforce_prefers_newest() {
        // Swap Force: newest-first. 2015 present → use it (with +256 offset).
        let bytes = Fixture {
            variant: 0x3000, // Swap Force
            hat_2011: 1,
            hat_2012: 2,
            hat_2013: 3,
            hat_2015: 10, // → HatId 266
            ..Default::default()
        }
        .build();
        let stats = parse(&bytes).unwrap();
        assert_eq!(stats.hat_current, HatId(266));
    }

    #[test]
    fn hat_lookup_swapforce_falls_through_when_2015_empty() {
        let bytes = Fixture {
            variant: 0x3000, // Swap Force
            hat_2011: 1,
            hat_2012: 2,
            hat_2013: 3,
            hat_2015: 0,
            ..Default::default()
        }
        .build();
        let stats = parse(&bytes).unwrap();
        // Newest present → 2013 → HatId(3).
        assert_eq!(stats.hat_current, HatId(3));
    }

    #[test]
    fn xp_level_aggregates_for_later_games() {
        // Trap Team figure: level derives from xp_2011 + xp_2012 + xp_2013.
        let bytes = Fixture {
            variant: 0x4000, // Trap Team
            xp_2011: 32_000,
            xp_2012: 1_000,
            xp_2013: 0,
            ..Default::default()
        }
        .build();
        let stats = parse(&bytes).unwrap();
        // Combined = 33_000 → level 10.
        assert_eq!(stats.level, 10);
    }

    #[test]
    fn timestamps_parse() {
        let bytes = Fixture {
            variant: 0x1000,
            last_placed: Some((30, 14, 15, 4, 2026)), // 2026-04-15 14:30
            last_reset: Some((0, 10, 1, 1, 2024)),    // 2024-01-01 10:00
            ..Default::default()
        }
        .build();
        let stats = parse(&bytes).unwrap();
        let p = stats.last_placed.expect("placed set");
        assert_eq!(p.to_string(), "2026-04-15 14:30:00");
        let r = stats.last_reset.expect("reset set");
        assert_eq!(r.to_string(), "2024-01-01 10:00:00");
    }

    #[test]
    fn timestamps_none_when_zero() {
        let bytes = Fixture {
            variant: 0x1000,
            last_placed: None,
            last_reset: None,
            ..Default::default()
        }
        .build();
        let stats = parse(&bytes).unwrap();
        assert_eq!(stats.last_placed, None);
        assert_eq!(stats.last_reset, None);
    }

    #[test]
    fn header_crc_tamper_flips_valid_flag() {
        let mut bytes = Fixture {
            figure_id: 5,
            variant: 0x1000,
            ..Default::default()
        }
        .build();
        // Tamper a byte inside the header-CRC coverage window.
        bytes[0x05] ^= 0xFF;
        let stats = parse(&bytes).unwrap();
        assert!(!stats.checksums_valid);
    }

    #[test]
    fn area_crc_tamper_flips_valid_flag() {
        let mut bytes = Fixture {
            figure_id: 5,
            variant: 0x1000,
            xp_2011: 5000,
            ..Default::default()
        }
        .build();
        // Tamper a nickname byte → region-A 0x30 CRC should fail.
        bytes[block_off(0x0A)] ^= 0x42;
        let stats = parse(&bytes).unwrap();
        assert!(!stats.checksums_valid);
    }

    #[test]
    fn area_sequence_picks_higher_region() {
        // Put a DIFFERENT xp value in the alternate region (0x24) and make it
        // the newer one.
        let bytes = Fixture {
            variant: 0x1000,
            xp_2011: 1000, // goes into the active region
            area_seq_a0: 0,
            area_seq_a1: 1, // a1 newer → active = 0x24 mirror
            ..Default::default()
        }
        .build();
        let stats = parse(&bytes).unwrap();
        // xp_2011 should have been written to 0x24 (active), not 0x08.
        assert_eq!(stats.xp_2011, 1000);
    }

    #[test]
    fn area_sequence_wraparound_ff_to_00() {
        // Region A: seq_a0 = 0 (at block 0x08), seq_a1 = 255 (at block 0x24).
        // Under wraparound, 0 is "one higher" than 255 → active = 0x08.
        let bytes = Fixture {
            variant: 0x1000,
            xp_2011: 2222,
            area_seq_a0: 0,
            area_seq_a1: 255,
            ..Default::default()
        }
        .build();
        let stats = parse(&bytes).unwrap();
        assert_eq!(stats.xp_2011, 2222);
    }

    #[test]
    fn trap_kind_skips_standard_payload() {
        // Boundaries + middle of the observed trap range 0x0D2..=0x0DC.
        // Each should classify as Trap and leave Standard payload defaults.
        for figure_id in [0x0D2u32, 0x0D7, 0x0DC] {
            let bytes = Fixture {
                figure_id,
                variant: 0x4000,
                xp_2011: 9999,
                gold: 500,
                ..Default::default()
            }
            .build();
            let stats = parse(&bytes).unwrap();
            assert_eq!(stats.figure_kind, FigureKind::Trap, "id=0x{figure_id:X}");
            assert_eq!(stats.xp_2011, 0, "id=0x{figure_id:X}");
            assert_eq!(stats.gold, 0, "id=0x{figure_id:X}");
        }
    }

    #[test]
    fn stats_serialize_json_round_trip() {
        let bytes = Fixture {
            figure_id: 7,
            variant: 0x1000,
            nickname: "Kaos".to_string(),
            ..Default::default()
        }
        .build();
        let stats = parse(&bytes).unwrap();
        let json = serde_json::to_string(&stats).unwrap();
        let round: SkyFigureStats = serde_json::from_str(&json).unwrap();
        assert_eq!(round.figure_id, skylander_core::ToyTypeId(7));
        assert_eq!(round.nickname, "Kaos");
    }

    #[test]
    fn crc16_ccitt_false_known_vector() {
        // Standard test vector: "123456789" → 0x29B1.
        assert_eq!(crc16_ccitt_false(b"123456789"), 0x29B1);
    }

    #[test]
    fn empty_file_all_zero_defaults() {
        // All zeros is a valid (if empty) blob. Header parses, figure_kind
        // = Standard because id 0 falls in the standard range, but the
        // header CRC over 30 zero bytes won't be zero → checksums_valid
        // should be false.
        let bytes = vec![0u8; SKY_FILE_LEN];
        let stats = parse(&bytes).unwrap();
        assert_eq!(stats.figure_id, skylander_core::ToyTypeId(0));
        assert_eq!(stats.variant, skylander_core::TagVariant(0));
        assert_eq!(stats.variant_decoded.year_code, SkyGeneration::Unknown);
        // All-zero bytes fail the header CRC (CRC of 30 zeros is 0xE1F0).
        assert!(!stats.checksums_valid);
    }

    #[test]
    fn unknown_year_code_does_not_panic_on_level() {
        let bytes = Fixture {
            variant: 0xF000, // unknown year code
            xp_2011: 2200,
            ..Default::default()
        }
        .build();
        let stats = parse(&bytes).unwrap();
        // Unknown → falls back to xp_2011 (2200 → level 3).
        assert_eq!(stats.level, 3);
    }
}
