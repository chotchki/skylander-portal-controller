//! Live (openspec AF.5): the O-series is inert without its gate. The oracle
//! boots with `GUAC_ORACLE` removed from its environment, and once RSX is
//! flipping, each of the five O-verbs answers `ERR unknown_cmd` while P1's own
//! words (`PING`, `STATE`, `STATUS`) work as in a product build. Mac, against
//! `GUAC_ORACLE_EXE` from `GUAC_ORACLE_HOME`; skips when a key is unset.
//!
//! `cargo test -p oracle-harness --test live_gate -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use oracle_harness::launch::{self, BootOptions};
use oracle_harness::snapshot::Snapshot;
use oracle_harness::verbs::{Refusal, ReplyError, Verb};
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
    for verb in [
        Verb::PauseAt(state.frames + 600),
        Verb::Resume,
        Verb::RsxCap,
        Verb::Screenshot,
        Verb::SaveState,
    ] {
        let err = control.verbs().send(verb).unwrap_err();
        let refusal = err
            .downcast_ref::<ReplyError>()
            .and_then(ReplyError::refusal);
        assert_eq!(refusal, Some(Refusal::UnknownCmd), "{verb:?}: {err:#}");
        eprintln!("{} -> ERR unknown_cmd", verb.encode().trim_end());
    }
    control.driver().ping().unwrap();
    control.driver().read_slots().unwrap();
    // Nothing paused it: the counter moves on (early boot can sit on one frame for a while).
    let after = oracle
        .wait_for(std::time::Duration::from_secs(60), |s| {
            s.frames > state.frames
        })
        .unwrap();
    eprintln!(
        "PING, STATUS and STATE answer; frames {} -> {}",
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
