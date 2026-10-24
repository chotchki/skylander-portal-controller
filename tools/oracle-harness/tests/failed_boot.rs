//! A boot that fails cleans up after itself. In a test binary of its own
//! because it spawns a process (openspec AQ.34): a spawn copies every
//! descriptor the process holds, close-on-exec ones included, and a spawn
//! that fails keeps its copies for up to a few ms after `spawn` returns. In the
//! library's test binary that copy held the lock tests' `RPCS3.buf` flock and
//! the launch test's dropped listeners past their `drop`, so those tests saw a
//! running RPCS3 or a live socket that wasn't there.

#![cfg(unix)]

use std::path::Path;

use oracle_harness::home::{InitOptions, OracleHome, init, rpcs3_config_dir, rpcs3_lock_file};
use oracle_harness::launch::{BootOptions, boot};
use oracle_harness::template::ConfigTemplate;

/// An oracle home over a fake dev HOME and Giants dump, all under `tmp`.
fn home(tmp: &Path) -> OracleHome {
    let real = rpcs3_config_dir(&tmp.join("real-home"));
    std::fs::create_dir_all(real.join("dev_flash/sys")).unwrap();
    std::fs::write(real.join("dev_flash/sys/x"), "x").unwrap();
    let eboot = tmp.join("dump/PS3_GAME/USRDIR/EBOOT.BIN");
    std::fs::create_dir_all(eboot.parent().unwrap()).unwrap();
    std::fs::write(&eboot, "x").unwrap();
    init(
        &tmp.join("oracle"),
        &InitOptions {
            real_home: tmp.join("real-home"),
            giants_dump: tmp.join("dump"),
            config: ConfigTemplate::default(),
        },
    )
    .unwrap()
}

#[test]
fn a_boot_that_fails_takes_its_run_dir_and_state_copy_with_it() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home(tmp.path());
    // A file that isn't a program, and a state to copy in.
    let exe = tmp.path().join("not-rpcs3");
    std::fs::write(&exe, "x").unwrap();
    let state = tmp.path().join("BLUS30968_1_0.SAVESTAT.zst");
    std::fs::write(&state, "not a state").unwrap();
    let opts = BootOptions {
        savestate: Some(state),
        ..BootOptions::default()
    };
    let leftovers = || {
        let prefix = format!("gor-{}-", std::process::id());
        std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .count()
    };

    // Refused at the preflight: an RPCS3 holds the home's lock.
    let held = std::fs::File::create(rpcs3_lock_file(home.root())).unwrap();
    held.lock().unwrap();
    let err = boot(&exe, &home, &opts).err().unwrap().to_string();
    assert!(err.contains("an RPCS3 is running"), "{err}");
    assert_eq!(leftovers(), 0);
    drop(held);

    // Past the copy, failing at the spawn (or at the GDB port, if another
    // RPCS3 on this machine holds it): nothing is left either way.
    assert!(boot(&exe, &home, &opts).is_err());
    assert_eq!(leftovers(), 0);
}
