//! `guacamole --setup <folder>` as a process: it stores the folder under the
//! dev data root (relative to its working directory) and exits, no window.
#![cfg(feature = "dev-tools")]

use std::path::Path;
use std::process::{Command, Output};

use guac_formats::sfo::builder::SfoBuilder;

fn dump(dir: &Path, title_id: &str) {
    let game = dir.join("PS3_GAME");
    std::fs::create_dir_all(game.join("USRDIR")).unwrap();
    let sfo = SfoBuilder::disc_game(title_id, "01.00", "01.00").build();
    std::fs::write(game.join("PARAM.SFO"), sfo).unwrap();
}

fn guacamole(cwd: &Path, args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guacamole"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run guacamole")
}

#[test]
fn setup_folder_stores_it_reports_the_dumps_and_exits() {
    let cwd = tempfile::tempdir().unwrap();
    let discs = tempfile::tempdir().unwrap();
    dump(&discs.path().join("Giants"), "BLUS30968");
    dump(&discs.path().join("Imaginators/Disc"), "BLUS31600");

    let out = guacamole(cwd.path(), &["--setup".as_ref(), discs.path().as_os_str()]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{out:?}");
    assert!(stdout.contains("Found 2 game dumps"), "{stdout}");
    assert!(stdout.contains("BLUS30968 Skylanders: Giants"), "{stdout}");
    assert!(stdout.contains("Giants is ready"), "{stdout}");

    let root = cwd.path().join("dev-data/guacamole");
    assert!(root.join("db.sqlite").is_file());
    let loaded = guac_server::setup::load_with(&root, &Default::default()).unwrap();
    assert!(!loaded.needs_setup());
    assert_eq!(
        loaded.dump_dirs,
        [std::path::absolute(discs.path()).unwrap()]
    );
    assert_eq!(loaded.scan().games().len(), 1);
}

#[test]
fn setup_with_a_missing_folder_fails_and_stores_nothing() {
    let cwd = tempfile::tempdir().unwrap();
    let missing = cwd.path().join("nope");
    let out = guacamole(cwd.path(), &["--setup".as_ref(), missing.as_os_str()]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("isn't a folder"));
    let root = cwd.path().join("dev-data/guacamole");
    let loaded = guac_server::setup::load_with(&root, &Default::default()).unwrap();
    assert!(loaded.needs_setup());
}

#[test]
fn a_bad_argument_prints_usage_and_exits_2() {
    let cwd = tempfile::tempdir().unwrap();
    let out = guacamole(cwd.path(), &["--setp".as_ref()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: guacamole"));
    // Nothing was created: no logs, no data root.
    assert_eq!(std::fs::read_dir(cwd.path()).unwrap().count(), 0);
}
