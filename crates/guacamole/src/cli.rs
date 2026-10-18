//! `guacamole [--setup [folder...]]`: the arguments, and `--setup`'s store
//! and report, kept apart from any window so they're testable.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use guac_server::dumps::{DumpScan, GIANTS_TITLE_ID, MAX_DEPTH};
use guac_server::setup;

pub const USAGE: &str = "\
usage: guacamole                      start the TV and serve phones
       guacamole --setup              choose the disc dumps folder, then start
       guacamole --setup <folder>...  store the dumps folder(s) and exit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run,
    /// Ask with the folder picker before the window opens, then run.
    SetupPick,
    /// Store these folders without a picker, report what they hold, exit.
    SetupFolders(Vec<PathBuf>),
    Help,
}

/// The arguments after the program name.
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Command, String> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Command::Run);
    };
    match first.to_str() {
        Some("-h" | "--help") => Ok(Command::Help),
        Some("--setup") => {
            let folders: Vec<PathBuf> = args.map(PathBuf::from).collect();
            if let Some(flag) = folders
                .iter()
                .find(|f| f.to_str().is_some_and(|s| s.starts_with('-')))
            {
                return Err(format!("unknown option {}", flag.display()));
            }
            Ok(if folders.is_empty() {
                Command::SetupPick
            } else {
                Command::SetupFolders(folders)
            })
        }
        _ => Err(format!("unknown argument {}", first.to_string_lossy())),
    }
}

/// Replaces the stored folders, so later starts skip setup; the scan is for the caller to show.
pub fn store(root: &Path, folders: &[PathBuf]) -> Result<DumpScan> {
    setup::Setup::open(root)?.set_dump_dirs(folders)
}

/// What a stored folder holds, for a person at a terminal.
pub fn report(
    out: &mut dyn Write,
    folders: &[PathBuf],
    scan: &DumpScan,
    env_overrides: bool,
) -> std::io::Result<()> {
    for folder in folders {
        writeln!(out, "Dumps folder: {}", folder.display())?;
    }
    for folder in &scan.unlisted {
        writeln!(out, "Couldn't list {}", folder.display())?;
    }
    for sfo in &scan.unreadable {
        writeln!(out, "Couldn't read {}", sfo.display())?;
    }
    match scan.found.len() {
        0 => writeln!(
            out,
            "No game dumps found (looked {MAX_DEPTH} folders deep)."
        )?,
        1 => writeln!(out, "Found 1 game dump:")?,
        n => writeln!(out, "Found {n} game dumps:")?,
    }
    for dump in &scan.found {
        let name = dump.display_name().unwrap_or("");
        writeln!(out, "  {} {name}  ({})", dump.title_id, dump.dir.display())?;
    }
    if scan.offered().is_some() {
        writeln!(out, "Skylanders: Giants is ready. Start guacamole to play.")?;
    } else {
        writeln!(
            out,
            "No Skylanders: Giants dump ({GIANTS_TITLE_ID}). Run --setup again with the folder that holds it."
        )?;
    }
    if env_overrides {
        writeln!(
            out,
            "Note: GUAC_DUMP_DIRS in .env.dev overrides the stored folder in dev builds."
        )?;
    }
    Ok(())
}

/// `--setup <folder>...`: store, report to `out`, and return the exit code.
pub fn setup_folders(root: &Path, folders: &[PathBuf], out: &mut dyn Write) -> i32 {
    match store(root, folders) {
        Ok(scan) => {
            tracing::info!(
                found = scan.found.len(),
                giants = scan.offered().is_some(),
                "--setup stored the dumps folders"
            );
            let env_overrides = setup::env().contains_key("GUAC_DUMP_DIRS");
            match report(out, folders, &scan, env_overrides) {
                Ok(()) => 0,
                Err(_) => 1,
            }
        }
        Err(e) => {
            tracing::error!("--setup: {e:#}");
            let _ = writeln!(out, "guacamole --setup: {e:#}");
            1
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use guac_formats::sfo::builder::SfoBuilder;

    fn args(list: &[&str]) -> Result<Command, String> {
        parse(list.iter().map(OsString::from))
    }

    pub(crate) fn dump(dir: &Path, title_id: &str) {
        let game = dir.join("PS3_GAME");
        std::fs::create_dir_all(game.join("USRDIR")).unwrap();
        let sfo = SfoBuilder::disc_game(title_id, "01.00", "01.00").build();
        std::fs::write(game.join("PARAM.SFO"), sfo).unwrap();
    }

    #[test]
    fn no_arguments_runs() {
        assert_eq!(args(&[]), Ok(Command::Run));
    }

    #[test]
    fn setup_alone_asks_with_the_picker() {
        assert_eq!(args(&["--setup"]), Ok(Command::SetupPick));
    }

    #[test]
    fn setup_with_folders_stores_them() {
        assert_eq!(
            args(&["--setup", "/d/discs", "D:\\more discs"]),
            Ok(Command::SetupFolders(vec![
                "/d/discs".into(),
                "D:\\more discs".into()
            ]))
        );
    }

    #[test]
    fn help_and_unknown_arguments() {
        assert_eq!(args(&["--help"]), Ok(Command::Help));
        assert_eq!(args(&["-h"]), Ok(Command::Help));
        assert!(args(&["--setp"]).unwrap_err().contains("--setp"));
        assert!(args(&["/d/discs"]).is_err());
        assert!(
            args(&["--setup", "--verbose"])
                .unwrap_err()
                .contains("--verbose")
        );
    }

    #[test]
    fn setup_folder_stores_and_a_restart_does_not_ask_again() {
        let root = tempfile::tempdir().unwrap();
        let discs = tempfile::tempdir().unwrap();
        dump(&discs.path().join("Giants"), GIANTS_TITLE_ID);
        dump(&discs.path().join("Swap Force"), "BLUS31076");
        dump(&discs.path().join("Other/Disc"), "BLES00001");

        let mut out = Vec::new();
        let code = setup_folders(root.path(), &[discs.path().to_path_buf()], &mut out);
        let out = String::from_utf8(out).unwrap();
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Found 3 game dumps"), "{out}");
        assert!(out.contains("BLUS30968 Skylanders: Giants"), "{out}");
        assert!(out.contains("BLUS31076 Skylanders: SWAP Force"), "{out}");
        assert!(out.contains("BLES00001 "), "{out}");
        assert!(out.contains("Giants is ready"), "{out}");

        // The next start reads the stored folder, not an env override.
        let loaded = setup::load_with(root.path(), &Default::default()).unwrap();
        assert!(!loaded.needs_setup());
        assert!(!loaded.dump_dirs_from_env);
        assert_eq!(loaded.scan().games().len(), 1);
        assert_eq!(loaded.scan().found.len(), 3);
    }

    #[test]
    fn setup_folder_without_giants_says_so() {
        let root = tempfile::tempdir().unwrap();
        let discs = tempfile::tempdir().unwrap();
        dump(&discs.path().join("Swap Force"), "BLUS31076");
        let mut out = Vec::new();
        let code = setup_folders(root.path(), &[discs.path().to_path_buf()], &mut out);
        let out = String::from_utf8(out).unwrap();
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("No Skylanders: Giants dump (BLUS30968)"),
            "{out}"
        );
        let loaded = setup::load_with(root.path(), &Default::default()).unwrap();
        assert!(!loaded.needs_setup());
        assert!(loaded.scan().games().is_empty());
    }

    #[test]
    fn setup_folder_that_is_missing_stores_nothing() {
        let root = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        let missing = root.path().join("nope");
        let code = setup_folders(root.path(), &[missing], &mut out);
        let out = String::from_utf8(out).unwrap();
        assert_eq!(code, 1);
        assert!(out.contains("isn't a folder"), "{out}");
        let loaded = setup::load_with(root.path(), &Default::default()).unwrap();
        assert!(loaded.needs_setup());
    }
}
