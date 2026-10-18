//! Writes every registered parser's seeds into a cargo-fuzz corpus, so the
//! fuzzer starts past each format's magic instead of hunting for it:
//!
//! `cargo run -p guac-formats --features test-fixtures --example fuzz_seeds`
//!
//! Output lands in `crates/guac-formats/fuzz/corpus/<target>/` (gitignored),
//! or under the directory given as the first argument.

use std::path::PathBuf;

use guac_formats::registry::PARSERS;

fn main() -> std::io::Result<()> {
    let root = std::env::args().nth(1).map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fuzz/corpus"),
        PathBuf::from,
    );
    for (pick, parser) in PARSERS.iter().enumerate() {
        let own = root.join(parser.name);
        let all = root.join("all_parsers");
        std::fs::create_dir_all(&own)?;
        std::fs::create_dir_all(&all)?;
        for (i, seed) in (parser.seeds)().iter().enumerate() {
            std::fs::write(own.join(format!("seed-{i}")), seed)?;
            // all_parsers picks the parser by the first byte.
            let picked = [&[pick as u8][..], seed].concat();
            std::fs::write(all.join(format!("{}-seed-{i}", parser.name)), picked)?;
        }
        println!("{}: seeds written to {}", parser.name, own.display());
    }
    Ok(())
}
