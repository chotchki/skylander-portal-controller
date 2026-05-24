//! Diagnose why specific `.sky` dumps fail CRC validation.
//!
//! Usage: `cargo run --example crc_probe -p skylander-sky-parser -- <file.sky> [<file.sky> ...]`
//!
//! For each file: decrypts in place, then for both region A (blocks 0x08 and 0x24)
//! and region B (blocks 0x11 and 0x2D) prints area_sequence + the stored vs computed
//! CRCs the parser checks at `lib.rs:809-832`, plus a short hex dump of the
//! relevant area headers so we can eyeball blank-vs-populated regions.

use skylander_sky_parser::{SKY_FILE_LEN, block_off, crc16_ccitt_false, decrypt_figure};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: crc_probe <file.sky> [<file.sky> ...]");
        std::process::exit(2);
    }
    for arg in &args {
        let path = PathBuf::from(arg);
        match probe(&path) {
            Ok(()) => {}
            Err(e) => eprintln!("{}: {e}", path.display()),
        }
        println!();
    }
}

fn probe(path: &std::path::Path) -> std::io::Result<()> {
    let raw = std::fs::read(path)?;
    if raw.len() != SKY_FILE_LEN {
        eprintln!("{}: expected {SKY_FILE_LEN} bytes, got {}", path.display(), raw.len());
        return Ok(());
    }
    let mut bytes = [0u8; SKY_FILE_LEN];
    bytes.copy_from_slice(&raw);
    decrypt_figure(&mut bytes);

    println!("=== {} ===", path.display());
    probe_region_a(&bytes, 0x08, "A0 (0x08)");
    probe_region_a(&bytes, 0x24, "A1 (0x24)");
    probe_region_b(&bytes, 0x11, "B0 (0x11)");
    probe_region_b(&bytes, 0x2D, "B1 (0x2D)");
    Ok(())
}

fn probe_region_a(bytes: &[u8], base_block: usize, label: &str) {
    let b_base = block_off(base_block);
    let b09 = block_off(base_block + 1);
    let b0a = block_off(base_block + 2);
    let b0c = block_off(base_block + 4);

    let seq = bytes[b_base + 0x09];
    let crc30_stored = read_u16(bytes, b_base + 0x0C);
    let crc14_stored = read_u16(bytes, b_base + 0x0E);

    let mut c30 = Vec::with_capacity(0x30);
    c30.extend_from_slice(&bytes[b09..b09 + 16]);
    c30.extend_from_slice(&bytes[b0a..b0a + 16]);
    c30.extend_from_slice(&bytes[b0c..b0c + 16]);
    let crc30_computed = crc16_ccitt_false(&c30);

    let mut c14 = Vec::with_capacity(16);
    c14.extend_from_slice(&bytes[b_base..b_base + 14]);
    c14.extend_from_slice(&[0x05, 0x00]);
    let crc14_computed = crc16_ccitt_false(&c14);

    let all_zero_first = bytes[b_base..b_base + 14].iter().all(|&b| b == 0);
    let all_zero_payload = c30.iter().all(|&b| b == 0);

    println!(
        "  {label}: seq={seq:#04X}  blank_header={all_zero_first}  blank_payload={all_zero_payload}"
    );
    println!(
        "    crc14: stored={crc14_stored:#06X}  computed={crc14_computed:#06X}  {}",
        if crc14_stored == crc14_computed { "OK" } else { "MISMATCH" }
    );
    println!(
        "    crc30: stored={crc30_stored:#06X}  computed={crc30_computed:#06X}  {}",
        if crc30_stored == crc30_computed { "OK" } else { "MISMATCH" }
    );
    print!("    block0x{base_block:02X} bytes: ");
    for b in &bytes[b_base..b_base + 16] {
        print!("{b:02X} ");
    }
    println!();
}

fn probe_region_b(bytes: &[u8], base_block: usize, label: &str) {
    let b_base = block_off(base_block);
    let seq = bytes[b_base + 0x02];
    let all_zero = bytes[b_base..b_base + 16].iter().all(|&b| b == 0);
    println!("  {label}: seq={seq:#04X}  blank_block={all_zero}");
    print!("    block0x{base_block:02X} bytes: ");
    for b in &bytes[b_base..b_base + 16] {
        print!("{b:02X} ");
    }
    println!();
}

fn read_u16(bytes: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([bytes[off], bytes[off + 1]])
}
