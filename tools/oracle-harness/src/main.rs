//! `oracle`: the RPCS3 oracle from the command line (openspec AF).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use oracle_harness::env;
use oracle_harness::home::{self, InitOptions, OracleHome};
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::pine::PineClient;
use oracle_harness::snapshot::{Snapshot, dev_rpcs3_dirs};
use oracle_harness::template::ConfigTemplate;

const USAGE: &str = "\
usage:
  oracle home init [--home <dir>] [--shader-mode <mode>] [--sys-fs-trace]
      build the oracle home ($GUAC_ORACLE_HOME unless --home): a dev_flash copy from
      the dev's RPCS3 dir, games.yml at $GUAC_GIANTS_DUMP, PINE on, two pads, the config
  oracle boot [--savestate <file>] [--windowed] [--ready-secs <n>] [--sample-secs <n>]
      boot $GUAC_ORACLE_EXE from the oracle home to playable (a savestate: to its
      Start Paused pause), sample the fps, shut down, and check the dev's RPCS3
      dirs came through untouched
  oracle pine --socket <path> (id | title | version | status | read <addr> <len>)
      query a running oracle's PINE server; read prints hex";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["home", "init", rest @ ..] => home_init(rest),
        ["boot", rest @ ..] => boot(rest),
        ["pine", "--socket", socket, rest @ ..] => pine(PathBuf::from(socket), rest),
        _ => bail!("{USAGE}"),
    }
}

fn real_home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is unset, so the dev's RPCS3 dir can't be found")
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
            real_home: real_home()?,
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
    while let [flag, tail @ ..] = rest {
        rest = tail;
        match (*flag, rest) {
            ("--savestate", [file, tail @ ..]) => {
                opts.savestate = Some(PathBuf::from(file));
                rest = tail;
            }
            ("--windowed", _) => opts.borderless = false,
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
    let dev_dirs = dev_rpcs3_dirs(&real_home()?);
    let before = Snapshot::take(&dev_dirs)?;

    let mut oracle = launch::boot(&exe, &home, &opts)?;
    println!(
        "booted pid {} (P1 {}, PINE {})",
        oracle.pid(),
        oracle.run_paths().p1_socket.display(),
        oracle.run_paths().pine_socket.display()
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
            let bytes = client.read_bytes(addr, len.parse()?)?;
            for (i, row) in bytes.chunks(16).enumerate() {
                let hex: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
                println!("{:08x}  {}", addr as usize + i * 16, hex.join(" "));
            }
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}
