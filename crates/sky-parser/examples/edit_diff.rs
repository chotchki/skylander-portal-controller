//! Apply our write path to a real `.sky` and dump the byte diff.
//!
//! Diagnostic for PLAN 11.11 — the game-refused-to-load bug.
//! Reads a real dump, decrypts, applies `set_xp` + `set_gold`,
//! prints every byte that differs from the original (both plaintext
//! and ciphertext views). Anything OUTSIDE our intended mutation
//! offsets is a smoking gun.
//!
//! Usage:
//!   cargo run --example edit_diff -p skylander-sky-parser -- \
//!     <file.sky> <target_level> <target_gold>

use skylander_sky_parser::{
    BLOCK_LEN, SKY_FILE_LEN, SkyGeneration, decrypt_figure, distribute_xp,
    encrypt_figure_preserving_unwritten, set_gold, set_xp, xp_for_level,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: edit_diff <file.sky> <level> <gold>");
    let level: u8 = args
        .next()
        .expect("missing level")
        .parse()
        .expect("level must be a u8");
    let gold: u16 = args
        .next()
        .expect("missing gold")
        .parse()
        .expect("gold must be a u16");

    let raw = std::fs::read(&path).expect("read");
    assert_eq!(raw.len(), SKY_FILE_LEN, "expected 1024 bytes");
    let mut source_cipher = [0u8; SKY_FILE_LEN];
    source_cipher.copy_from_slice(&raw);

    // Decrypt original.
    let mut orig_plain = source_cipher;
    decrypt_figure(&mut orig_plain);

    // Pull the figure's generation from the variant year_code (best guess).
    let variant = u16::from_le_bytes([orig_plain[0x1C], orig_plain[0x1D]]);
    let year_code = ((variant >> 12) & 0x0F) as u8;
    let generation = match year_code {
        1 => SkyGeneration::SpyrosAdventure,
        2 => SkyGeneration::Giants,
        3 => SkyGeneration::SwapForce,
        4 => SkyGeneration::TrapTeam,
        5 => SkyGeneration::SuperChargers,
        6 => SkyGeneration::Imaginators,
        _ => SkyGeneration::Unknown,
    };
    println!(
        "file={path}\n  variant=0x{variant:04X}  year_code={year_code}  generation={generation:?}\n  target_level={level}  target_gold={gold}"
    );

    // Apply our edit pipeline to a copy.
    let mut edited_plain = orig_plain;
    let target_xp = xp_for_level(level);
    let slots = distribute_xp(target_xp, generation);
    println!(
        "  distribute_xp({target_xp}, {generation:?}) = xp_2011={} xp_2012={} xp_2013={}",
        slots.xp_2011, slots.xp_2012, slots.xp_2013
    );
    set_gold(&mut edited_plain, gold);
    set_xp(&mut edited_plain, slots);

    println!("\n=== PLAINTEXT DIFF (decrypted) ===");
    dump_diff(&orig_plain, &edited_plain);

    // Re-encrypt and diff the ciphertext (what actually lands on disk).
    let mut orig_cipher = orig_plain;
    encrypt_figure_preserving_unwritten(&mut orig_cipher, &source_cipher);
    let mut edited_cipher = edited_plain;
    encrypt_figure_preserving_unwritten(&mut edited_cipher, &source_cipher);

    println!("\n=== CIPHERTEXT DIFF (encrypted) ===");
    dump_diff(&orig_cipher, &edited_cipher);

    // Sanity: does the re-encrypted original match the original file bytes?
    // If not, our decrypt/encrypt round-trip is lossy.
    if orig_cipher.as_slice() != raw.as_slice() {
        println!(
            "\n!!! WARNING: decrypt → encrypt of the source file does NOT match the source bytes."
        );
        println!("    This is a parser round-trip bug independent of any write-path mutation.");
        let mut delta = 0usize;
        for (i, (a, b)) in raw.iter().zip(orig_cipher.iter()).enumerate() {
            if a != b {
                delta += 1;
                if delta <= 16 {
                    println!("    file_off=0x{i:03X}  src={a:02X}  re-encrypt={b:02X}");
                }
            }
        }
        println!("    total mismatched bytes: {delta} / {SKY_FILE_LEN}");
    } else {
        println!("\nDecrypt → encrypt is byte-identical to source ✓");
    }
}

fn dump_diff(a: &[u8; SKY_FILE_LEN], b: &[u8; SKY_FILE_LEN]) {
    let mut changes: Vec<(usize, u8, u8)> = Vec::new();
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            changes.push((i, *x, *y));
        }
    }
    if changes.is_empty() {
        println!("  no differences");
        return;
    }
    println!("  total bytes changed: {}", changes.len());
    // Group by block for readability.
    let mut current_block: Option<usize> = None;
    for (off, before, after) in &changes {
        let block = off / BLOCK_LEN;
        if current_block != Some(block) {
            println!("  --- block 0x{block:02X} (file_off 0x{:03X}) ---", block * 16);
            current_block = Some(block);
        }
        let in_block = off % BLOCK_LEN;
        println!(
            "    +0x{off:03X} (block 0x{block:02X} +0x{in_block:02X}): {before:02X} → {after:02X}"
        );
    }
}
