//! Loopback integration test for `IpcPortalDriver` (PLAN 16.5).
//!
//! Spins up an **in-process fake AF_UNIX server** that speaks the patched-RPCS3
//! P1 protocol (first-free `LOAD`, `STATUS`, `CLEAR`, plus interleaved `HB`
//! heartbeats), points the driver at it, and asserts the connect → send → parse
//! round-trips. No real RPCS3 needed, so unlike the `live*` suites this is **not
//! `#[ignore]`d** — it runs in CI on Windows + macOS (both ship AF_UNIX).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(windows)]
use uds_windows::UnixListener;

use skylander_core::{SlotIndex, SlotState};
use skylander_rpcs3_control::PortalDriver;
use skylander_rpcs3_control::ipc::IpcPortalDriver;
use skylander_rpcs3_control::ipc::proto::PadButton;

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
/// exercise the driver's HB-skip. Returns the bound path plus a shared log of
/// every `LOAD` arg the server received — so a test can assert the *exact* path
/// that reached the wire (the patched RPCS3 opens it against its own CWD, so the
/// driver must send it absolute).
fn spawn_fake_server_pushes(
    prepend_hb: bool,
    prepend_pe: bool,
) -> (PathBuf, Arc<Mutex<Vec<String>>>) {
    let path = unique_sock();
    let _ = std::fs::remove_file(&path); // AF_UNIX bind needs no stale file
    let listener = UnixListener::bind(&path).expect("bind fake IPC socket");

    let loads: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let loads_srv = Arc::clone(&loads);

    // Emulator-side portal model: serial per occupied slot, persists across the
    // (one-per-op) connections the driver opens. Behind a Mutex so each connection
    // can be serviced on its own thread (below).
    let portal: Arc<Mutex<[Option<u32>; 8]>> = Arc::new(Mutex::new([None; 8]));

    thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(stream) = conn else { break };
            // One thread per connection. The driver opens a fresh connection per op;
            // a single-threaded accept loop that stays inside one connection's read
            // loop can starve acceptance of the next op's connection under parallel
            // CI load, which surfaced as a uds_windows read timeout (os error 10060)
            // on `load_clear_status_roundtrip`. Per-connection threads accept + reply
            // immediately regardless of any lingering connection.
            let loads_conn = Arc::clone(&loads_srv);
            let portal_conn = Arc::clone(&portal);
            thread::spawn(move || {
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
                    if let Some(arg) = cmd.strip_prefix("LOAD ") {
                        loads_conn.lock().unwrap().push(arg.to_string());
                    }
                    let reply = {
                        let mut p = portal_conn.lock().unwrap();
                        handle(cmd, &mut p)
                    };
                    if prepend_hb {
                        let _ = writer.write_all(b"HB status=running frames=1 progr=8/8 seg=0/0\n");
                    }
                    if prepend_pe {
                        // P4 portal-event push interleaved before the reply.
                        let _ = writer.write_all(b"PE cmd=status\n");
                    }
                    if writer.write_all(reply.as_bytes()).is_err() {
                        break;
                    }
                }
            });
        }
    });

    (path, loads)
}

/// Back-compat shim: a fake server with no `PE` pushes (most tests use this).
fn spawn_fake_server(prepend_hb: bool) -> (PathBuf, Arc<Mutex<Vec<String>>>) {
    spawn_fake_server_pushes(prepend_hb, false)
}

/// Bind a server that, on each connection, immediately writes the given push
/// lines (each newline-terminated) and then idles — modelling the patched
/// emulator's *unsolicited* push feed (1 Hz `HB` + `PE`), which `watch_events`
/// reads on a connection that sends nothing.
fn spawn_pushing_server(lines: &'static [&'static str]) -> PathBuf {
    let path = unique_sock();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind fake IPC socket");
    thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut stream) = conn else { break };
            thread::spawn(move || {
                for l in lines {
                    if stream.write_all(l.as_bytes()).is_err() {
                        return;
                    }
                    if stream.write_all(b"\n").is_err() {
                        return;
                    }
                }
                // Idle so the client read times out and re-checks its deadline,
                // exactly like the real emulator between pushes.
                thread::sleep(Duration::from_secs(2));
            });
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
        "RECONNECT" => "OK\n".to_string(),
        // P6: the real emulator presses+holds+releases the pad; the fake just
        // validates the wire shape ("BUTTON_PRESS <NAME> <MS>") and acks.
        "BUTTON_PRESS" => {
            let mut parts = arg.split_whitespace();
            match (
                parts.next(),
                parts.next().and_then(|m| m.parse::<u32>().ok()),
            ) {
                (Some(name), Some(_ms)) if !name.is_empty() => "OK\n".to_string(),
                _ => "ERR bad_args\n".to_string(),
            }
        }
        // P8: the real emulator publishes its CAMetalLayer's CAContextID (decimal);
        // the fake replies with a fixed non-zero id.
        "SURFACE" => "OK context=456586159\n".to_string(),
        // P7: the real emulator re-frames its game window; the fake validates the wire
        // shape ("WINDOW_SET <x> <y> <w> <h>", w/h >= the emulator minimum) and acks.
        "WINDOW_SET" => {
            let mut parts = arg.split_whitespace();
            match (
                parts.next().and_then(|v| v.parse::<i32>().ok()),
                parts.next().and_then(|v| v.parse::<i32>().ok()),
                parts.next().and_then(|v| v.parse::<u32>().ok()),
                parts.next().and_then(|v| v.parse::<u32>().ok()),
            ) {
                (Some(_x), Some(_y), Some(w), Some(h)) if w >= 160 && h >= 90 => "OK\n".to_string(),
                _ => "ERR invalid_geometry\n".to_string(),
            }
        }
        _ => "ERR unknown_cmd\n".to_string(),
    }
}

#[test]
fn press_button_round_trips() {
    let (sock, _loads) = spawn_fake_server(false);
    let d = IpcPortalDriver::with_path(&sock);
    d.press_button(PadButton::Cross, 120)
        .expect("BUTTON_PRESS CROSS 120 should ack OK");
}

#[test]
fn window_set_round_trips() {
    let (sock, _loads) = spawn_fake_server(false);
    let d = IpcPortalDriver::with_path(&sock);
    d.window_set(0, 24, 1280, 720)
        .expect("WINDOW_SET 0 24 1280 720 should ack OK");
}

#[test]
fn surface_context_id_round_trips() {
    let (sock, _loads) = spawn_fake_server(false);
    let d = IpcPortalDriver::with_path(&sock);
    // The fake publishes the fixed id 0x1B36F3AF == 456_586_159.
    assert_eq!(d.surface_context_id().unwrap(), 456_586_159);
    // The trait method maps a non-zero id to Some (game_surface_context_id is
    // what the launcher's state poller calls).
    assert_eq!(d.game_surface_context_id().unwrap(), Some(456_586_159));
}

#[test]
fn load_clear_status_roundtrip() {
    let (sock, _loads) = spawn_fake_server(false);
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
    let (sock, _loads) = spawn_fake_server(true); // server prepends HB before every reply
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
    let (sock, _loads) = spawn_fake_server(false);
    let d = IpcPortalDriver::with_path(&sock);

    let err = d.clear(SlotIndex::new(2).unwrap()).unwrap_err().to_string();
    assert!(err.contains("not_loaded"), "got: {err}");

    let _ = std::fs::remove_file(&sock);
}

#[test]
fn ping_pongs() {
    let (sock, _loads) = spawn_fake_server(false);
    let d = IpcPortalDriver::with_path(&sock);
    d.ping().expect("PING should PONG");
    let _ = std::fs::remove_file(&sock);
}

#[test]
fn portal_full_is_an_error() {
    let (sock, _loads) = spawn_fake_server(false);
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

#[test]
fn load_path_is_sent_absolute() {
    // The patched RPCS3 P1 `LOAD` handler opens the path arg against *its own*
    // process CWD — which is not the server's. A server-relative working-copy path
    // (`dev-data/working/…/x.sky`) therefore resolves to nothing and the emulator
    // answers `ERR open_failed`. The driver must absolutize before sending. (Live
    // bug, 2026-05-30: figures failed to place with exactly that error.)
    let (sock, loads) = spawn_fake_server(false);
    let d = IpcPortalDriver::with_path(&sock);

    d.load(
        SlotIndex::new(0).unwrap(),
        &PathBuf::from("dev-data/working/profile/fig.sky"), // deliberately relative
    )
    .unwrap();

    let received = loads.lock().unwrap();
    assert_eq!(received.len(), 1, "expected exactly one LOAD on the wire");
    assert!(
        Path::new(&received[0]).is_absolute(),
        "driver must send an absolute path, got {:?}",
        received[0]
    );

    drop(received);
    let _ = std::fs::remove_file(&sock);
}

#[test]
fn portal_events_are_skipped_before_reply() {
    // A `PE` portal-event push (P4) interleaved before a reply must be skipped by
    // the driver's roundtrip exactly like an `HB` — otherwise a live run
    // mis-parses the push as the command's answer. Push BOTH before every reply.
    let (sock, _loads) = spawn_fake_server_pushes(true, true);
    let d = IpcPortalDriver::with_path(&sock);

    d.ping().expect("HB+PE pushes before PONG must be skipped");
    let name = d
        .load(SlotIndex::new(0).unwrap(), &PathBuf::from("Eruptor.sky"))
        .unwrap();
    assert_eq!(name, "Eruptor", "PE noise must not corrupt the reply");

    let _ = std::fs::remove_file(&sock);
}

#[test]
fn reconnect_roundtrips_ok() {
    // P5: the RECONNECT command (portal hot-plug cycle) round-trips to OK.
    let (sock, _loads) = spawn_fake_server(false);
    let d = IpcPortalDriver::with_path(&sock);
    d.reconnect().expect("RECONNECT should return OK");
    let _ = std::fs::remove_file(&sock);
}

#[test]
fn watch_events_surfaces_portal_events() {
    // The diagnostic tail must receive the emulator's *unsolicited* pushes
    // (HB + PE) on a connection that sends nothing. (PLAN 15.12 P4.)
    static PUSHES: &[&str] = &[
        "HB status=running frames=10 progr=8/8 seg=0/0",
        "PE cmd=activate",
        "PE cmd=status",
        "PE cmd=query block=0",
    ];
    let sock = spawn_pushing_server(PUSHES);
    let d = IpcPortalDriver::with_path(&sock);

    let seen = Mutex::new(Vec::<String>::new());
    d.watch_events(Duration::from_millis(900), |_elapsed, line| {
        seen.lock().unwrap().push(line.to_string());
    })
    .unwrap();

    let seen = seen.into_inner().unwrap();
    assert!(
        seen.iter().any(|l| l == "PE cmd=activate"),
        "activate push not surfaced; got {seen:?}"
    );
    assert!(
        seen.iter().any(|l| l.starts_with("PE cmd=query")),
        "query push not surfaced; got {seen:?}"
    );

    let _ = std::fs::remove_file(&sock);
}
