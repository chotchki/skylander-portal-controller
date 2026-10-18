//! `guac-extract`: the extractor from a terminal, over the same library
//! guacamole calls. A dev tool until guacamole ships it: the extract root is
//! `GUAC_EXTRACT_DIR` (the environment, else `.env.dev` in the current folder),
//! required, outside any git working tree.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use guac_extract::known::KNOWN_DUMPS;
use guac_extract::{DirState, ExtractRoot, RootMode};

const USAGE: &str = "\
usage: guac-extract identify <dump-dir>
       guac-extract [--help | --version]

Extracts what guacamole plays from your own Skylanders disc dump.

  identify   hash the dump (the folder holding PS3_GAME), check it against the
             known dumps and open its folder under GUAC_EXTRACT_DIR; an unknown
             dump gets a report of names, sizes and hashes beside that folder
";

fn extract_dir() -> Option<OsString> {
    if let Some(v) = std::env::var_os("GUAC_EXTRACT_DIR") {
        return Some(v);
    }
    let text = std::fs::read_to_string(".env.dev").ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == "GUAC_EXTRACT_DIR").then(|| v.trim().trim_matches('"').into())
    })
}

fn identify(dump: &Path) -> Result<(), guac_extract::ExtractError> {
    let root = ExtractRoot::resolve(RootMode::Dev {
        extract_dir: extract_dir(),
    })?;
    let started = Instant::now();
    let opened = guac_extract::open(dump, &root, KNOWN_DUMPS, &|_| {})?;
    let m = opened.dir.manifest();
    println!(
        "identity    {} {} {}",
        m.identity.title_id, m.identity.app_ver, m.identity.manifest_hash
    );
    println!("extract     {}", opened.dir.path().display());
    let state = match &opened.state {
        DirState::Fresh => "fresh".to_owned(),
        DirState::Current => "current".to_owned(),
        DirState::Resume => "resume".to_owned(),
        DirState::Rebuilt(why) => format!("rebuilt ({why:?})"),
    };
    println!("state       {state}");
    println!(
        "hashed      {} files, {} bytes; {} from the cache; {:.1} s",
        opened.files_hashed,
        opened.bytes_hashed,
        opened.files_cached,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["-h" | "--help"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["-V" | "--version"] => {
            println!(
                "guac-extract {} (layout v{}, IR v{})",
                env!("CARGO_PKG_VERSION"),
                guac_extract::LAYOUT_VERSION,
                guac_extract::IR_VERSION
            );
            ExitCode::SUCCESS
        }
        ["identify", dump] => match identify(Path::new(dump)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("guac-extract: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprint!(
                "guac-extract: unknown arguments `{}`\n\n{USAGE}",
                args.join(" ")
            );
            ExitCode::from(2)
        }
    }
}
