//! `oracle`: the RPCS3 oracle from the command line (openspec AF).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use oracle_harness::control::Control;
use oracle_harness::env;
use oracle_harness::home::{self, InitOptions, OracleHome};
use oracle_harness::identity::Identity;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::pine::PineClient;
use oracle_harness::snapshot::{Snapshot, dev_rpcs3_dirs};
use oracle_harness::template::ConfigTemplate;
use skylander_rpcs3_control::PortalDriver;

const USAGE: &str = "\
usage:
  oracle home init [--home <dir>] [--shader-mode <mode>] [--sys-fs-trace] [--audio]
      build the oracle home ($GUAC_ORACLE_HOME unless --home): a dev_flash copy from
      the dev's RPCS3 dir, games.yml at $GUAC_GIANTS_DUMP, PINE on, two pads, the config
      (silent, RPCS3's Null audio renderer, unless --audio)
  oracle boot [--savestate <file>] [--windowed] [--surface-2x] [--ungated] [--hold]
              [--ready-secs <n>] [--sample-secs <n>]
      boot $GUAC_ORACLE_EXE from the oracle home to playable (a savestate: to its
      Start Paused pause), sample the fps, shut down, and check the dev's RPCS3
      dirs came through untouched; prints the identity line and what RPCS3
      inherited that can change a frame. --hold keeps it running, its P1 socket
      printed for the verbs below, until stdin closes; --ungated leaves
      GUAC_ORACLE unset, so the O-verbs answer ERR unknown_cmd
  oracle pause-at --socket <p1> <flip>
      pause when the flip counter reaches <flip>; at a pause the target is set
      first, then the run resumed
  oracle resume --socket <p1>
  oracle screenshot --socket <p1> --out <dir>
      a PNG of the frame the counter names, copied into <dir>
  oracle capture --socket <p1> --out <dir>
      an RSX capture (RESUMEd first at a pause), the .rrc.gz copied into <dir>
  oracle savestate --socket <p1> --out <dir>
      a savestate, copied into <dir>, then P5's RECONNECT at the reboot's pause
      (the last three find RPCS3's files under $GUAC_ORACLE_HOME; <dir> must sit
      outside every work tree)
  oracle pine --socket <path> (id | title | version | status | read <addr> <len>)
      query a running oracle's PINE server; read prints hex, and only while paused";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["home", "init", rest @ ..] => home_init(rest),
        ["boot", rest @ ..] => boot(rest),
        ["pine", "--socket", socket, rest @ ..] => pine(PathBuf::from(socket), rest),
        [verb, "--socket", socket, rest @ ..] => run_verb(verb, &PathBuf::from(socket), rest),
        _ => bail!("{USAGE}"),
    }
}

fn required(key: &str) -> Result<PathBuf> {
    env::path(key).with_context(|| format!("set {key} (env or .env.dev)"))
}

fn home_init(mut rest: &[&str]) -> Result<()> {
    let mut home = None;
    let mut config = ConfigTemplate::default();
    while let [flag, tail @ ..] = rest {
        rest = tail;
        match (*flag, rest) {
            ("--home", [dir, tail @ ..]) => {
                home = Some(PathBuf::from(dir));
                rest = tail;
            }
            ("--shader-mode", [mode, tail @ ..]) => {
                config.shader_mode = (*mode).to_owned();
                rest = tail;
            }
            ("--sys-fs-trace", _) => config.sys_fs_trace = true,
            ("--audio", _) => config.audio = true,
            _ => bail!("{USAGE}"),
        }
    }
    let home = match home {
        Some(h) => h,
        None => required(env::ORACLE_HOME)?,
    };
    let built = home::init(
        &home,
        &InitOptions {
            real_home: home::real_home()?,
            giants_dump: required(env::GIANTS_DUMP)?,
            config,
        },
    )?;
    println!(
        "oracle home ready: {} (RPCS3 config dir {})",
        built.root().display(),
        built.config_dir().display()
    );
    Ok(())
}

fn boot(mut rest: &[&str]) -> Result<()> {
    let mut opts = BootOptions {
        borderless: true,
        ..BootOptions::default()
    };
    let (mut ready, mut sample) = (Duration::from_secs(600), Duration::from_secs(30));
    let mut hold = false;
    while let [flag, tail @ ..] = rest {
        rest = tail;
        match (*flag, rest) {
            ("--savestate", [file, tail @ ..]) => {
                opts.savestate = Some(PathBuf::from(file));
                rest = tail;
            }
            ("--windowed", _) => opts.borderless = false,
            ("--surface-2x", _) => opts.surface_2x = true,
            ("--ungated", _) => opts.ungated = true,
            ("--hold", _) => hold = true,
            ("--ready-secs", [n, tail @ ..]) => {
                ready = Duration::from_secs(n.parse()?);
                rest = tail;
            }
            ("--sample-secs", [n, tail @ ..]) => {
                sample = Duration::from_secs(n.parse()?);
                rest = tail;
            }
            _ => bail!("{USAGE}"),
        }
    }
    let exe = required(env::ORACLE_EXE)?;
    let home = OracleHome::open(&required(env::ORACLE_HOME)?)?;
    let dev_dirs = dev_rpcs3_dirs(&home::real_home()?);
    let before = Snapshot::take(&dev_dirs)?;

    println!("{}", Identity::of(&exe, &home.config_dir())?.line());
    let mut oracle = launch::boot(&exe, &home, &opts)?;
    println!(
        "booted pid {} (P1 {}, PINE {})",
        oracle.pid(),
        oracle.run_paths().p1_socket.display(),
        oracle.run_paths().pine_socket.display()
    );
    let inherited: Vec<String> = oracle
        .render_env()
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    println!(
        "inherited render env: {}",
        if inherited.is_empty() {
            "none".to_owned()
        } else {
            inherited.join(" ")
        }
    );
    let state = if opts.savestate.is_some() {
        oracle.wait_paused(ready)?
    } else {
        oracle.wait_playable(ready)?
    };
    println!("ready: status={} frames={}", state.status, state.frames);
    if opts.savestate.is_none() && !sample.is_zero() {
        let fps = oracle.sample_fps(sample)?;
        println!(
            "sustained: {} frames in {:.1} s, {:.1} fps",
            fps.frames,
            fps.seconds,
            fps.fps()
        );
    }
    if hold {
        println!(
            "holding: oracle <verb> --socket {} ...; close stdin (Ctrl-D) to shut down",
            oracle.run_paths().p1_socket.display()
        );
        let mut sink = String::new();
        while std::io::stdin().read_line(&mut sink)? > 0 {
            sink.clear();
        }
    }
    println!("shutdown: {:?}", oracle.shutdown()?);

    let changed = before.diff(&Snapshot::take(&dev_dirs)?);
    if !changed.is_empty() {
        bail!(
            "the dev's RPCS3 dirs changed during the run:\n{}",
            changed.join("\n")
        );
    }
    println!("the dev's RPCS3 dirs: {} entries, unchanged", before.len());
    Ok(())
}

/// The O-verbs against an oracle `oracle boot --hold` (or a live test) runs.
fn run_verb(verb: &str, socket: &std::path::Path, rest: &[&str]) -> Result<()> {
    let wait = Duration::from_secs(120);
    let config_dir =
        || -> Result<PathBuf> { Ok(OracleHome::open(&required(env::ORACLE_HOME)?)?.config_dir()) };
    match (verb, rest) {
        ("pause-at", [flip]) => {
            let control = Control::new(socket, &config_dir()?);
            let state = control.pause_at(flip.parse().context("<flip> is a decimal")?, wait)?;
            println!("paused: frames={}", state.frames);
        }
        ("resume", []) => {
            Control::new(socket, &config_dir()?).resume()?;
            println!("resumed");
        }
        ("screenshot", ["--out", dir]) => {
            let shot = Control::new(socket, &config_dir()?).screenshot(dir.as_ref(), wait)?;
            println!(
                "{} flip={} {}x{} lit={:.1}% pixels sha256={}",
                shot.path.display(),
                shot.flip,
                shot.frame.width,
                shot.frame.height,
                shot.frame.lit_share() * 100.0,
                shot.frame.pixels_sha256
            );
        }
        ("capture", ["--out", dir]) => {
            let cap = Control::new(socket, &config_dir()?).capture(dir.as_ref(), wait)?;
            println!(
                "{} armed at frames={} ({}), paused at frames={}",
                cap.path.display(),
                cap.armed_at.frames,
                cap.armed_at.status,
                cap.paused_at.frames
            );
        }
        ("savestate", ["--out", dir]) => {
            let control = Control::new(socket, &config_dir()?);
            let saved = control.savestate(dir.as_ref(), wait)?;
            control.driver().reconnect()?;
            println!(
                "{} saved at frames={}, rebooted to {} frames={}, portal reconnected",
                saved.path.display(),
                saved.saved_at.frames,
                saved.rebooted.status,
                saved.rebooted.frames
            );
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

fn pine(socket: PathBuf, rest: &[&str]) -> Result<()> {
    let mut client = PineClient::connect(&socket)?;
    match rest {
        ["id"] => println!("{}", client.title_id()?),
        ["title"] => println!("{}", client.title()?),
        ["version"] => println!("{}", client.version()?),
        ["status"] => println!("{:?}", client.status()?),
        ["read", addr, len] => {
            let addr = u32::from_str_radix(addr.trim_start_matches("0x"), 16)
                .with_context(|| format!("address {addr:?} isn't hex"))?;
            let bytes = client.read_paused(addr, len.parse()?)?;
            for (i, row) in bytes.chunks(16).enumerate() {
                let hex: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
                println!("{:08x}  {}", addr as usize + i * 16, hex.join(" "));
            }
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}
