//! Live (openspec AF.5, AF.7, AG.1): the O-series is inert without its gate.
//! The oracle boots with `GUAC_ORACLE` removed from its environment, and once
//! RSX is flipping, each of the eleven O-verbs (O3's five, O4's three, O5's
//! three) answers `ERR unknown_cmd` while P1's own words (`PING`, `STATE`,
//! `STATUS`) work as in a product build, and none of them took: the counter
//! runs on past the `PAUSE_AT` target without a pause, and RPCS3 writes no
//! screenshot, capture or state. Mac, against `GUAC_ORACLE_EXE` from `GUAC_ORACLE_HOME`; skips when a
//! key is unset.
//!
//! `cargo test -p oracle-harness --test live_gate -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use guac_oracle::input::Ds3Pad;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::outputs::Kind;
use oracle_harness::snapshot::Snapshot;
use oracle_harness::verbs::{Refusal, ReplyError, Verb, Watch, WatchRange};
use skylander_rpcs3_control::PortalDriver;

#[test]
#[ignore = "live: boots GUAC_ORACLE_EXE without GUAC_ORACLE=1 and sends the O-verbs (Mac)"]
fn without_the_gate_every_o_verb_is_an_unknown_command() {
    let Some(live) = common::live() else { return };
    let before = Snapshot::take(&live.dev_dirs).unwrap();
    let mut oracle = launch::boot(
        &live.exe,
        &live.home,
        &BootOptions {
            borderless: true,
            ungated: true,
            ..BootOptions::default()
        },
    )
    .unwrap();
    let state = oracle
        .wait_for(common::secs("ORACLE_READY_SECS", 600), |s| s.frames > 0)
        .unwrap();
    eprintln!("RSX flipping: {state:?}");

    let control = oracle.control();
    const KINDS: [Kind; 3] = [Kind::Screenshot, Kind::Capture, Kind::SaveState];
    let written = KINDS.map(|k| control.outputs().list(k));
    // Near enough that the run below has to go past it.
    let target = state.frames + 30;
    for verb in [
        Verb::PauseAt(target),
        Verb::Resume,
        Verb::RsxCap,
        Verb::Screenshot,
        Verb::SaveState,
        Verb::Pad {
            port: 0,
            flip: target,
            pad: Ds3Pad::REST,
        },
        Verb::PadClear,
        Verb::PadState {
            port: 0,
            flip: None,
        },
    ] {
        let err = control.verbs().send(verb).unwrap_err();
        let refusal = err
            .downcast_ref::<ReplyError>()
            .and_then(ReplyError::refusal);
        assert_eq!(refusal, Some(Refusal::UnknownCmd), "{verb:?}: {err:#}");
        eprintln!("{} -> ERR unknown_cmd", verb.encode().trim_end());
    }
    for watch in [
        Watch::Arm {
            frames: 16,
            port: 0,
            ranges: vec![WatchRange {
                addr: oracle_harness::actors::APP,
                len: 4,
            }],
        },
        Watch::Summary,
        Watch::Read { first: 0, count: 1 },
        Watch::Clear,
    ] {
        let err = control.verbs().send_watch(&watch).unwrap_err();
        let refusal = err
            .downcast_ref::<ReplyError>()
            .and_then(ReplyError::refusal);
        assert_eq!(refusal, Some(Refusal::UnknownCmd), "{watch:?}: {err:#}");
        eprintln!("{} -> ERR unknown_cmd", watch.encode().trim_end());
    }
    control.driver().ping().unwrap();
    control.driver().read_slots().unwrap();
    // Nothing took: the counter runs past the target without a pause (early
    // boot can sit on one frame for a while), and no file appeared.
    let after = oracle
        .wait_for(std::time::Duration::from_secs(120), |s| {
            s.frames > target + 30
        })
        .unwrap();
    assert_eq!(after.status, "running", "{after:?}");
    for (kind, before) in KINDS.iter().zip(&written) {
        assert_eq!(&control.outputs().list(*kind), before, "{kind:?} written");
    }
    eprintln!(
        "PING, STATUS and STATE answer; frames {} -> {}, past PAUSE_AT {target} running; no file written",
        state.frames, after.frames
    );
    eprintln!("shutdown: {:?}", oracle.shutdown().unwrap());

    let changed = before.diff(&Snapshot::take(&live.dev_dirs).unwrap());
    assert!(
        changed.is_empty(),
        "the dev's RPCS3 dirs changed:\n{}",
        changed.join("\n")
    );
}
