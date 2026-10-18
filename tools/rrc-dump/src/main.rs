//! `rrc-dump <capture.rrc[.gz]> (--out <dir> | --list)`: dump or list an RRC v6 capture's draws.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use rrc_dump::output::{DumpWriter, list_line};
use rrc_dump::{Capture, replay};

const USAGE: &str = concat!(
    "usage: rrc-dump <capture.rrc[.gz]> (--out <dir> | --list)\n",
    "  --out <dir>  write each draw's buffers + draw.json under <dir> (must be outside this repo)\n",
    "  --list       print one line per draw, write nothing",
);

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (input, out) = match args.as_slice() {
        [input, flag] if flag == "--list" => (PathBuf::from(input), None),
        [input, flag, dir] if flag == "--out" => (PathBuf::from(input), Some(PathBuf::from(dir))),
        _ => bail!("{USAGE}"),
    };

    // Refuse before the (possibly long) read and decompress.
    let mut writer = out.as_deref().map(DumpWriter::create).transpose()?;

    let bytes = std::fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
    let capture =
        Capture::from_file_bytes(&bytes).with_context(|| format!("parsing {}", input.display()))?;
    eprintln!(
        "RRC v{}: {} commands, {} memory blocks, {} data blobs ({} unreferenced), starting constants: {}",
        capture.version,
        capture.commands.len(),
        capture.blocks.len(),
        capture.data.len(),
        capture.unreferenced_blobs(),
        capture.reg_state.transform_constants.is_some()
    );

    let stats = replay(&capture, |draw| -> Result<()> {
        match writer.as_mut() {
            Some(w) => w.write_draw(&draw),
            None => {
                println!("{}", list_line(&draw));
                Ok(())
            }
        }
    })?;
    eprintln!(
        "{} draws; {} unresolved memory ids, {} orphan commands, {} stray ends",
        stats.draws, stats.unresolved_blocks, stats.orphan_commands, stats.stray_ends
    );
    if let Some(w) = writer {
        w.finish(&input, &stats)?;
    }
    Ok(())
}
