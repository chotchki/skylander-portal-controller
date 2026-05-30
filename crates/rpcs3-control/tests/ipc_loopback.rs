//! Loopback integration test for `IpcPortalDriver` (PLAN 16.5).
//!
//! Spins up an **in-process fake AF_UNIX server** that speaks the patched-RPCS3
//! P1 protocol (first-free `LOAD`, `STATUS`, `CLEAR`, plus interleaved `HB`
//! heartbeats), points the driver at it, and asserts the connect → send → parse
//! round-trips. No real RPCS3 needed, so unlike the `live*` suites this is **not
//! `#[ignore]`d** — it runs in CI on Windows + macOS (both ship AF_UNIX).

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;

#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(windows)]
use uds_windows::UnixListener;

use skylander_core::{SlotIndex, SlotState};
use skylander_rpcs3_control::PortalDriver;
use skylander_rpcs3_control::ipc::IpcPortalDriver;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Unique short socket path under the temp dir (AF_UNIX sun_path is ~108 bytes,
/// so keep it short). Per-process + per-call counter avoids collisions.
fn unique_sock() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("sky-ipc-{}-{}.sock", std::process::id(), n))
}

/// Bind a fake P1 server at `path` (synchronously, before returning, so the
/// driver can't lose a connect race), then service connections on a detached
/// thread. `prepend_hb` makes it emit a heartbeat line before every reply, to
/// exercise the driver's HB-skip. Returns the bound path.
fn spawn_fake_server(prepend_hb: bool) -> PathBuf {
    let path = unique_sock();
    let _ = std::fs::remove_file(&path); // AF_UNIX bind needs no stale file
    let listener = UnixListener::bind(&path).expect("bind fake IPC socket");

    thread::spawn(move || {
        // Emulator-side portal model: serial per occupied slot, persists across
        // the (one-per-op) connections the driver opens.
        let mut portal: [Option<u32>; 8] = [None; 8];
        for conn in listener.incoming() {
            let Ok(stream) = conn else { break };
            let mut writer = stream.try_clone().expect("clone fake conn");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break, // client closed after its one op
                    Ok(_) => {}
                }
                let cmd = line.trim_end_matches(['\r', '\n']);
                if cmd.is_empty() {
                    continue;
                }
                let reply = handle(cmd, &mut portal);
                if prepend_hb {
                    let _ = writer.write_all(b"HB status=running frames=1 progr=8/8 seg=0/0\n");
                }
                if writer.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });

    path
}

/// Minimal P1 server command handler over the fake portal.
fn handle(cmd: &str, portal: &mut [Option<u32>; 8]) -> String {
    let (verb, arg) = cmd.split_once(' ').unwrap_or((cmd, ""));
    match verb {
        "PING" => "PONG\n".to_string(),
        "STATUS" => {
            let mut s = String::from("OK");
            for (i, slot) in portal.iter().enumerate() {
                match slot {
                    Some(serial) => s.push_str(&format!(" {i}:{serial:08X}")),
                    None => s.push_str(&format!(" {i}:empty")),
                }
            }
            s.push('\n');
            s
        }
        "LOAD" => match portal.iter().position(Option::is_none) {
            // first-free, exactly like the real load_skylander
            Some(i) => {
                let serial = arg
                    .bytes()
                    .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
                portal[i] = Some(serial);
                format!("OK slot={i}\n")
            }
            None => "ERR portal_full\n".to_string(),
        },
        "CLEAR" => match arg.trim().parse::<usize>() {
            Ok(n) if n < 8 => {
                if portal[n].take().is_some() {
                    "OK\n".to_string()
                } else {
                    "ERR not_loaded\n".to_string()
                }
            }
            _ => "ERR bad_slot\n".to_string(),
        },
        _ => "ERR unknown_cmd\n".to_string(),
    }
}

#[test]
fn load_clear_status_roundtrip() {
    let sock = spawn_fake_server(false);
    let d = IpcPortalDriver::with_path(&sock);

    // All empty initially.
    let slots = d.read_slots().unwrap();
    assert!(slots.iter().all(|s| matches!(s, SlotState::Empty)));

    // Load with a *hint* of slot 3 — the emulator assigns slot 0 (first-free).
    let name = d
        .load(
            SlotIndex::new(3).unwrap(),
            &PathBuf::from("/pack/Fire/Spyro.sky"),
        )
        .unwrap();
    assert_eq!(name, "Spyro");

    // Read back: figure landed in the emulator-assigned slot 0, named Spyro;
    // the hint slot 3 stays empty — the emulator owns numbering.
    let slots = d.read_slots().unwrap();
    match &slots[0] {
        SlotState::Loaded { display_name, .. } => assert_eq!(display_name, "Spyro"),
        other => panic!("expected slot 0 Loaded, got {other:?}"),
    }
    assert!(
        matches!(slots[3], SlotState::Empty),
        "hint slot must stay empty"
    );

    // Clear the assigned slot.
    d.clear(SlotIndex::new(0).unwrap()).unwrap();
    assert!(
        d.read_slots()
            .unwrap()
            .iter()
            .all(|s| matches!(s, SlotState::Empty))
    );

    let _ = std::fs::remove_file(&sock);
}

#[test]
fn heartbeats_are_skipped_before_reply() {
    let sock = spawn_fake_server(true); // server prepends HB before every reply
    let d = IpcPortalDriver::with_path(&sock);

    let name = d
        .load(SlotIndex::new(0).unwrap(), &PathBuf::from("Eruptor.sky"))
        .unwrap();
    assert_eq!(name, "Eruptor", "HB noise must not corrupt the reply");

    let slots = d.read_slots().unwrap();
    assert!(
        matches!(&slots[0], SlotState::Loaded { display_name, .. } if display_name == "Eruptor")
    );

    let _ = std::fs::remove_file(&sock);
}

#[test]
fn clear_empty_slot_surfaces_emulator_error() {
    let sock = spawn_fake_server(false);
    let d = IpcPortalDriver::with_path(&sock);

    let err = d.clear(SlotIndex::new(2).unwrap()).unwrap_err().to_string();
    assert!(err.contains("not_loaded"), "got: {err}");

    let _ = std::fs::remove_file(&sock);
}

#[test]
fn ping_pongs() {
    let sock = spawn_fake_server(false);
    let d = IpcPortalDriver::with_path(&sock);
    d.ping().expect("PING should PONG");
    let _ = std::fs::remove_file(&sock);
}

#[test]
fn portal_full_is_an_error() {
    let sock = spawn_fake_server(false);
    let d = IpcPortalDriver::with_path(&sock);
    // Fill all 8 slots, then the 9th load must error.
    for i in 0..8 {
        d.load(
            SlotIndex::new(0).unwrap(),
            &PathBuf::from(format!("/p/F{i}.sky")),
        )
        .unwrap();
    }
    let err = d
        .load(
            SlotIndex::new(0).unwrap(),
            &PathBuf::from("/p/overflow.sky"),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("portal_full"), "got: {err}");
    let _ = std::fs::remove_file(&sock);
}
