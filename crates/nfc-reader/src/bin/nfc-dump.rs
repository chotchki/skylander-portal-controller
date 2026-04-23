//! Ad-hoc CLI: scan one figure through the attached PC/SC reader and write
//! `<uid>.sky` to an output directory. Debug tool — the server consumes the
//! same library via its long-running scanner worker.
//!
//! Usage:
//!   `cargo run -p skylander-nfc-reader --bin nfc-dump -- [output-dir]`
//!                                                        — full dump flow.
//!   `cargo run -p skylander-nfc-reader --bin nfc-dump -- --probe`
//!                                                        — reader-only probe.

use std::path::PathBuf;

use anyhow::{Context, Result};
use skylander_nfc_reader::{dump_figure, open_reader};

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

    let reader = open_reader()?;
    println!("reader: {}", reader.name());
    let fw = reader.firmware_version()?;
    println!("firmware: {}", fw);

    if probe_only {
        println!("probe OK — escape path is live. Re-run without --probe to dump a figure.");
        return Ok(());
    }

    println!("tap a Skylanders figure on the reader…");
    let t0 = std::time::Instant::now();
    let uid = loop {
        if let Some(uid) = reader.list_passive_target()? {
            break uid;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    let uid_hex = uid.to_hex_string();
    println!("target found after {:.2?}, uid={}", t0.elapsed(), uid_hex);

    let dump_t = std::time::Instant::now();
    let bytes = dump_figure(&reader, uid)?;
    println!(
        "read {} bytes in {:.3?}",
        bytes.len(),
        dump_t.elapsed()
    );

    let path = out_dir.join(format!("{}.sky", uid_hex));
    std::fs::write(&path, &bytes).with_context(|| format!("write {}", path.display()))?;
    println!("wrote {}", path.display());

    match skylander_sky_parser::parse(&bytes) {
        Ok(stats) => {
            println!(
                "parser: figure_id=0x{:06X} variant=0x{:04X} kind={:?} level={} gold={} nickname={:?} crc_ok={}",
                stats.figure_id.get(),
                stats.variant.get(),
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
