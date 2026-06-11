//! PLAN 15.12 P4 portal-event diagnostic — tail the patched-RPCS3 portal-event
//! push stream, and optionally fire one `LOAD` mid-stream.
//!
//! Boot a game (or **resume a save state**) on the patched RPCS3, then run this
//! to see what the *guest game* asks of the emulated portal:
//!   * `PE cmd=activate` — the game turned the portal on (reached the screen).
//!   * `PE cmd=status`   — the interrupt presence-poll (rate-limited to ~4 Hz on
//!                         the emulator side); a steady stream means the game IS
//!                         polling the portal.
//!   * `PE cmd=query`    — a block read; a *burst* right after a `LOAD` means the
//!                         game noticed the new figure and read it back.
//!
//! With `--load`, it fires one `LOAD` on a side connection at `--at` seconds, so
//! a single run produces the before/after timeline that answers the open
//! save-state question (PLAN 15.12 (d)): **does a resumed game reflect a *late*
//! LOAD, or does it stop polling the portal after resume?**
//!
//! Usage:
//!   cargo run -p skylander-rpcs3-control --example portal_tail -- \
//!       [SECONDS] [--load <abs .sky path>] [--at <SECONDS>]
//!
//! Socket: `$SKYLANDER_IPC_PATH` or the per-platform default (same resolution as
//! the driver). Pass an **absolute** `.sky` path to `--load` — RPCS3 opens it
//! against its own working directory.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;
use skylander_rpcs3_control::ipc::{IpcPortalDriver, default_socket_path, proto};

fn main() -> anyhow::Result<()> {
    let mut secs = 30u64;
    let mut load: Option<PathBuf> = None;
    let mut at = 8u64;
    let mut reconnect_at: Option<u64> = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--load" => load = args.next().map(PathBuf::from),
            "--at" => at = args.next().and_then(|s| s.parse().ok()).unwrap_or(at),
            "--reconnect-at" => reconnect_at = args.next().and_then(|s| s.parse().ok()),
            "-h" | "--help" => {
                eprintln!(
                    "usage: portal_tail [SECONDS] [--reconnect-at <SECONDS>] [--load <abs .sky>] [--at <SECONDS>]"
                );
                return Ok(());
            }
            other => match other.parse::<u64>() {
                Ok(n) => secs = n,
                Err(_) => eprintln!("portal_tail: ignoring unrecognized arg {other:?}"),
            },
        }
    }

    let sock = default_socket_path();
    eprintln!("portal_tail: socket {} — watching {secs}s", sock.display());

    let driver = IpcPortalDriver::with_path(&sock);
    // Fail fast with a clear message if nothing is listening.
    driver.ping().map_err(|e| {
        anyhow::anyhow!(
            "no RPCS3 IPC listener at {} ({e:#}) — is the patched RPCS3 running?",
            sock.display()
        )
    })?;

    // Optionally hot-plug-cycle the portal (P5) at `--reconnect-at`, on its own
    // connection — used after a save-state resume to refresh the guest's USB
    // handles (else its transfers fail CELL_EINVAL and it can't see the portal).
    if let Some(rt) = reconnect_at {
        let sock = sock.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(rt));
            let d = IpcPortalDriver::with_path(&sock);
            match d.reconnect() {
                Ok(()) => {
                    eprintln!("\n>>> [{rt:>3}.0s] RECONNECT -> ok (portal hot-plug cycled)\n")
                }
                Err(e) => eprintln!("\n>>> [{rt:>3}.0s] RECONNECT FAILED: {e:#}\n"),
            }
        });
    }

    // Optionally fire one LOAD partway through, on its own connection.
    let load_at = load.as_ref().map(|_| at);
    if let Some(path) = load.clone() {
        let sock = sock.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(at));
            let d = IpcPortalDriver::with_path(&sock);
            match d.load(SlotIndex::new(0).unwrap(), &path) {
                Ok(name) => {
                    eprintln!(
                        "\n>>> [{at:>3}.0s] LOAD {} -> ok ({name})\n",
                        path.display()
                    )
                }
                Err(e) => {
                    eprintln!("\n>>> [{at:>3}.0s] LOAD {} FAILED: {e:#}\n", path.display())
                }
            }
        });
    }

    // cmd -> [count before LOAD, count after LOAD].
    let mut tally: BTreeMap<String, [u32; 2]> = BTreeMap::new();
    let mut heartbeats = 0u32;

    driver.watch_events(Duration::from_secs(secs), |elapsed, line| {
        if proto::is_heartbeat(line) {
            heartbeats += 1;
            return;
        }
        if proto::is_portal_event(line) {
            let after = load_at.map_or(false, |t| elapsed.as_secs() >= t);
            let cmd = proto::parse_portal_event(line)
                .map(|pe| pe.cmd)
                .unwrap_or_else(|_| "??".to_string());
            tally.entry(cmd).or_insert([0, 0])[after as usize] += 1;
            println!("[{:>6.1}s] {line}", elapsed.as_secs_f64());
        }
    })?;

    println!("\n--- summary ({secs}s watched, {heartbeats} heartbeats) ---");
    if load_at.is_some() {
        println!("{:<12} {:>8} {:>8}", "cmd", "before", "after");
        for (cmd, [b, a]) in &tally {
            println!("{cmd:<12} {b:>8} {a:>8}");
        }
        println!("\nLOAD fired at {at}s. Reading the `after` column:");
        println!("  query burst       => resumed game re-read the portal, saw the figure (works).");
        println!("  status, no query  => AMBIGUOUS: polling but no re-read. Could be a dead end,");
        println!("                       OR the restored RAM already accounts for that slot.");
        println!("  status stops      => the game isn't polling the portal after resume.");
        println!("To make a null (`status, no query`) result conclusive:");
        println!("  1. positive baseline: run the same LOAD on a FRESH boot + activate — it");
        println!("     SHOULD produce a query burst, proving the load path itself works.");
        println!("  2. load into a slot the saved game saw EMPTY (a presence edge it never saw).");
        println!("  3. run portal_tail as the SOLE IPC client — the emulator's PE queue is");
        println!("     split across connections, so a recorder polling STATUS would steal PEs.");
    } else {
        println!("{:<12} {:>8}", "cmd", "count");
        for (cmd, [b, a]) in &tally {
            println!("{cmd:<12} {:>8}", b + a);
        }
    }
    Ok(())
}
