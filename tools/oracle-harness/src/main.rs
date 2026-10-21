//! `oracle`: the RPCS3 oracle from the command line (openspec AF).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use guac_oracle::input::InputScript;
use oracle_harness::actors::{self, ActorFrame};
use oracle_harness::chains::{self, GiantsChains};
use oracle_harness::control::Control;
use oracle_harness::divergence::calibrate::{self, Reference};
use oracle_harness::divergence::observing::Choice;
use oracle_harness::divergence::{self, Inputs, Perturbation, Plan, ReplayRecord, Report, Set};
use oracle_harness::env;
use oracle_harness::home::{self, InitOptions, OracleHome};
use oracle_harness::identity::Identity;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::pine::PineClient;
use oracle_harness::rng::{self, Hit, Peek};
use oracle_harness::route;
use oracle_harness::scan;
use oracle_harness::snapshot::{Snapshot, dev_rpcs3_dirs};
use oracle_harness::sweep;
use oracle_harness::template::ConfigTemplate;
use skylander_rpcs3_control::PortalDriver;

const USAGE: &str = "\
usage:
  oracle home init [--home <dir>] [--shader-mode <mode>] [--sys-fs-trace] [--audio]
      build the oracle home ($GUAC_ORACLE_HOME unless --home): a dev_flash copy from
      the dev's RPCS3 dir, games.yml at $GUAC_GIANTS_DUMP, PINE on, two pads, the config
      (silent unless --audio: RPCS3's Null audio renderer, and silence where a
      screenshot would beep)
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
  oracle pad-load --socket <p1> <script.ron> [--base <flip>]
      at a pause: PAD_CLEAR, then the input script's PAD lines, its offset 0 on the
      first flip after the pause (or on <flip>)
  oracle pad-clear --socket <p1>
      both pads back to the host's input
  oracle pad-state --socket <p1> <port> [<flip>]
      what the guest last read on that port, or the first read of <flip>, with the
      flip's read count and how many reads differed from its first
  oracle pine --socket <path> (id | title | version | status | read <addr> <len>)
      query a running oracle's PINE server; read prints hex, and only while paused
  oracle rng census --pine <socket>
      at a pause: Giants' MT (its meta's live count, tfbApplication::_random), every
      word equal to its vtable in the PPU's areas, and what points at each instance
  oracle rng peek --pine <socket> [--p1 <socket>] [--out <file.json>]
      at a pause: the MT (_mti and its 624 words), the animation LCG and FMOD's LCG
      word, printed with the MT's seed and draws since, and written to <file>
      (outside every work tree); --p1 stamps it with STATE's flip
  oracle rng seed --pine <socket> (<n> | --from <peek.json>)
      at a pause: sgenrand(<n>) into the MT with _mti 624, as the game's own seeding
      leaves it; or put back the MT and the animation LCG a peek read
  oracle rng distance <a.json> <b.json> [--max-twists <n>]
      numbers the MT drew and animation LCG steps from peek a to peek b
      (negative: b is the earlier)
  oracle rng origin <peek.json> [--max-twists <n>]
      the MT's sgenrand seed and the numbers drawn since it
  oracle actors --pine <socket> [--p1 <socket>] [--out <file.json>] [--all]
      at a pause: every game actor on Giants' list (position, heading, scale,
      velocity, skinned, the pad driving it), the camera and the hero (the one
      skinned actor a pad drives); --all prints every actor, --out writes the
      frame (outside every work tree), --p1 stamps it with STATE's flip
  oracle watch arm --socket <p1> --pine <socket> --list <list.json> [--frames <n>]
              [--chains <chains.json>]
      at a pause: Giants' watch list resolved through PINE (the frame time and its
      scales, the hero's placement, corpus, matrix, parameters and pad floats, the
      camera, the hero's clips, the generator's _mti and first word), written to
      <list.json> (outside every work tree), and O5 armed with a ring of <n> (1024)
      records of it, one at every read of pad 0 (once a game frame in Giants), the
      first the next flip's. --chains (AH.5) resolves Giants' O6 chains too (the
      actor list keeping the hero, the Chompies, Norticus, the breakables in reach,
      their debris and Eruptor's projectiles; the orbs; the loot), writes them to
      <chains.json>
      and arms them before the watch, so each record carries their blocks
  oracle watch read --socket <p1> --list <list.json> --out <dir>
              [--chains <chains.json> --pine <socket>]
      at a pause: the ring and O4's readback on the list's port for its flips, into
      <dir>/watch.json (raw) and <dir>/recording.json (decoded); prints the ring's
      checks (_curFrame a step a record, every range readable, a record for every
      read O4 counted) and the flips that ran two game frames. With --chains each
      record's chain blocks are split off and named through PINE: <dir>/chain.json,
      chain.bin (raw) and chain-records.json (guac_oracle's chain records)
  oracle watch unchain --socket <p1>
      O6's CHAIN_CLEAR, refused while a watch is armed
  oracle watch clear --socket <p1>
  oracle watch residuals <recording.json> [--from <flip>] [--to <flip>]
      each position step against velocity times the clamped step, under both
      alignments and both integration orders, over the records one game frame
      apart whose earlier one's flip is in range
  oracle sweep list --pack <dir>
      AE.22's figure list: the roster's figures with a character model (off
      $GUAC_GIANTS_DUMP) and the .sky each takes from <dir> (the dev's pack, read
      only): the file Giants reads as that figure, lowest year code first; a figure
      no file reads as is listed with none
  oracle sweep run --out <dir> --pack <dir> [--figures <id|name|load file>,...]
              [--state <file>] [--settle-flips <n>] [--attempts <n>] [--boot-secs <n>]
              [--gap-secs <n>]
      AE.22's sweep, one figure per boot: a fresh RPCS3 from the gameplay state
      ($GUAC_ORACLE_SAVESTATE unless --state), P5's RECONNECT and a fresh copy of the
      figure's .sky through P1's LOAD at F0, RECONNECT again once it runs until the
      game queries the portal, the figure's archive opened and AF.9's reader holding a
      new hero, <settle-flips> (300) more; then at a pause the hero's model, what the
      game attached to him and where (bone or node), his scale, CollisionInfo,
      ActorParameters and solid, every actor and a SCREENSHOT, into
      <dir>/<id>-<load file>[-s2|-lc]/ (figure.json, hero.png, actors.json, RPCS3's
      log), with figures.json, summary.json and summary.txt beside them. Each figure
      is tried <attempts> (2) times, <gap-secs> (30) after the last RPCS3's exit, and
      listed when every try fails; one whose figure.json is in <dir> already is kept.
      The home's saves, which the game writes when a new figure arrives, are copied
      to <dir>/home-pristine at the start and put back before any boot that finds
      them changed; the pack and the dev's RPCS3 dirs are checked unchanged at the end
  oracle goto --out <dir> --route <route.ron> [--state <file>] [--figure <file>]
              [--watch | --chains] [--ring <n>] [--knobs <knobs.ron>] [--boot-secs <n>]
              [--max-load <x>] [--seed <n>] [--poke <placement>:<path>=<value>]...
      AH.7's route driver: a fresh RPCS3 from the gameplay state ($GUAC_ORACLE_SAVESTATE
      unless --state) with a fresh copy of the figure ($GUAC_ORACLE_FIGURE unless
      --figure), F0 read (actors-f0.json, peek-f0.json, and with $GUAC_GIANTS_DUMP set
      the fight's state, fight-f0.json, and AH.8's census of level 213's placements,
      census-f0.json), then the route's steps closed loop: Via and Stop walk to a point
      (pausing every few flips to read the hero and the camera and steer through O4),
      Hold holds the stick toward a point for so many flips whatever he does, Face
      turns him where he stands, Settle waits until he and the eye hold still, Press
      and Wait run as one schedule, Shot and Read take a SCREENSHOT or the reads at the
      pause; save: true takes a SAVESTATE at the end. With the dump and a watch, the
      route's placements' wake state rides the watch per frame (route-frames.json),
      and route-record.json puts the steps, what fired in order, what changed between
      reads and the scripts that started beside the last read's.
      --watch arms O5's watch on Giants' list at F0 (a ring of <n> (6000) records),
      --chains O6's chains with it. Writes <dir> (new, outside every work tree):
      record.json, route.json (every pause), input.ron (what port 0 held from F0 + 1,
      for the headless runner), the reads, the shots, the ring and the end state.
      Waits while the 1-minute load is over <max-load> (32) before the boot; the dev's
      RPCS3 dirs and the home's saves and trophies are checked unchanged at the end.
      --seed (AH.10) writes sgenrand(<n>) into the game's generator at F0, as
      `rng seed` does, before F0 is read; each --poke sets a placement's script
      variable (a number, its path dotted) at F0 after the seed
  oracle divergence run --out <dir> [--scripts idle,walk|<file.ron>,...] [--sets A,B]
              [--replays <n>] [--first <n>] [--end <flips>] [--every <flips>]
              [--warmup <script>] [--state <file>] [--figure <file>] [--attempts <n>]
              [--boot-secs <n>] [--gap-secs <n>] [--perturb <change>]...
      AF.11's measurement: <n> replays (10) of each script under each set, interleaved
      (replay 0 of every script and set, then replay 1, ...), each a fresh RPCS3 from
      the gameplay state ($GUAC_ORACLE_SAVESTATE unless --state) with a fresh copy of
      the figure ($GUAC_ORACLE_FIGURE unless --figure): set A observed at F0 + <end>
      (1800) only and saved there, set B every <every> (60) flips; a SCREENSHOT at the
      end of both; set C (AF.12) set B's pauses with a SCREENSHOT at F0 and at each;
      set W (AG.1) set A with O5's watch armed at F0 on Giants' list and read at the
      end (<replay>/watch.json and recording.json).
      Each --perturb (AF.12) replaces the scripts by their perturbed copies, named
      <script>_<change>: shift:<at>[,<at>...]:<by> moves those steps <by> flips (a
      press and its release together when both are named), stick:<at>:<x>,<y> sets
      port 0's left stick in the step at <at>. Every boot after the first waits
      <gap-secs> (30) from the last RPCS3's exit (AF.12: a boot straight after one
      fails its CoreMIDI client and hangs). --warmup runs one set B replay of
      <script> first, into <dir>/warmup,
      left out of the report. Each replay writes <dir>/<script>-<set>-<nn>/ (record.json,
      the actor frames and peeks, the end shot, the end state, RPCS3's log); a failed
      one is renamed .failed-<k> and tried again, up to <attempts> (3), a boot that
      hasn't reached Start Paused's pause in <boot-secs> (120; more for a cold home)
      counting as failed. <dir> must sit outside every work tree; the dev's RPCS3 dirs
      are checked unchanged at the end, and the home's saves and trophies (which a
      state's load doesn't put back) after every replay, a change stopping the run
  oracle divergence report <dir> [--paused <dir>] [--rule d|a] [--out <dir>]
      the numbers from <dir>'s records: per observable the spread at each checkpoint
      and the usable horizon by floor, the RNG distances, every actor by corpus, FLIP
      between end shots, set A against set B at the end flip, set W or O against set A
      on AF.11's discriminators judged against set B (<dir>'s set B of the script, else
      --paused's, AF.11's measurement) and the spreads against the host's load; written
      to <out> (<dir> by default)/report.txt and report.json. The rule: d (AH.40's, the
      default) the draws by rank-sum |z| < 2 against set A, the camera's offset and the
      slide's stop z by that and |z| >= 2 against set B, the slide's stop x printed
      only; a (AG.34's) the draws by rank-sum, the camera's offset and both slide stops
      by rank-sum and no replay inside set B's range
  oracle divergence calibrate <dir>
      AF.12's leave-one-out over <dir>'s groups with guac_oracle::tolerance's
      defaults: each replay against the envelope of the others inside the
      others' horizon on each observable (and, reported, over the whole run), and
      each checkpoint's shots against the band of the others; written to
      <dir>/calibrate.txt and calibrate.json, failing when any group does
  oracle divergence check --reference <dir> --script <s> --set <A|B|C> <candidates>
      every replay under <candidates> whose script isn't <s> against <dir>'s group
      <s>, <set>: each trajectory inside the group's horizon and over the whole run,
      and each frame the two share a checkpoint for; written to <candidates>/check.txt
      and check.json
  oracle fight report --rules <rules.ron> --out <dir> <replay dir>... [--candidate <dir>]...
      AH.9: each replay dir (a drive's or a divergence replay's, with O5's watch and
      O6's chains) read back as guac_oracle's fight trace: its per-actor event logs,
      chain links, breaks, totals and the generator's draws per game frame; over
      the group (3 or more replays of one script from one anchor) the anchor's
      horizon, each event kind's floor by AF.12's method (the least that passes
      every replay's leave-one-out, rounded up to the rules' quantum), and each
      candidate against the whole group at those floors; written to
      <dir>/report.txt and report.json";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["home", "init", rest @ ..] => home_init(rest),
        ["boot", rest @ ..] => boot(rest),
        ["pine", "--socket", socket, rest @ ..] => pine(PathBuf::from(socket), rest),
        ["rng", rest @ ..] => rng_cmd(rest),
        ["actors", "--pine", socket, rest @ ..] => actors_cmd(socket.as_ref(), rest),
        ["watch", rest @ ..] => watch_cmd(rest),
        ["sweep", "list", "--pack", pack] => sweep_list(pack.as_ref()),
        ["sweep", "run", rest @ ..] => sweep_run(rest),
        ["goto", rest @ ..] => goto(rest),
        ["divergence", "run", rest @ ..] => divergence_run(rest),
        ["divergence", "report", rest @ ..] => divergence_report(rest),
        ["divergence", "calibrate", dir] => divergence_calibrate(dir.as_ref()),
        ["divergence", "check", rest @ ..] => divergence_check(rest),
        ["fight", "report", rest @ ..] => fight_report(rest),
        [verb, "--socket", socket, rest @ ..] => run_verb(verb, &PathBuf::from(socket), rest),
        _ => bail!("{USAGE}"),
    }
}

/// `oracle fight report` (AH.9).
fn fight_report(mut rest: &[&str]) -> Result<()> {
    let (mut rules, mut out) = (None, None);
    let (mut dirs, mut candidates) = (Vec::new(), Vec::new());
    while let [arg, tail @ ..] = rest {
        rest = tail;
        match (*arg, rest) {
            ("--rules", [at, tail @ ..]) => {
                rules = Some(PathBuf::from(at));
                rest = tail;
            }
            ("--out", [at, tail @ ..]) => {
                out = Some(PathBuf::from(at));
                rest = tail;
            }
            ("--candidate", [at, tail @ ..]) => {
                candidates.push(PathBuf::from(at));
                rest = tail;
            }
            (flag, _) if flag.starts_with("--") => bail!("{USAGE}"),
            (dir, _) => dirs.push(PathBuf::from(dir)),
        }
    }
    let (Some(rules), Some(out)) = (rules, out) else {
        bail!("{USAGE}");
    };
    let report = oracle_harness::fightlog::report(&rules, &dirs, &candidates, &out)?;
    print!("{}", report.text());
    println!("written to {}", out.display());
    Ok(())
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
            let home = OracleHome::open(&required(env::ORACLE_HOME)?)?;
            let control = Control::new(socket, &home.config_dir())
                .with_log(home.cache_dir().join("RPCS3.log"));
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
        ("pad-load", [file, base @ ..]) => {
            let text = std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?;
            let script = InputScript::from_ron(&text).with_context(|| format!("parsing {file}"))?;
            let control = Control::new(socket, &config_dir()?);
            let loaded = match base {
                [] => control.load_script(&script)?,
                ["--base", flip] => {
                    control.load_script_at(&script, flip.parse().context("<flip> is a decimal")?)?
                }
                _ => bail!("{USAGE}"),
            };
            println!(
                "loaded {} PAD entries at the pause at frames={}, offset 0 on flip {}",
                loaded.entries, loaded.paused_at.frames, loaded.base
            );
        }
        ("pad-clear", []) => {
            Control::new(socket, &config_dir()?).pad_clear()?;
            println!("schedules dropped");
        }
        ("pad-state", [port, flip @ ..]) => {
            let port = port.parse().context("<port> is 0 or 1")?;
            let flip = match flip {
                [] => None,
                [f] => Some(f.parse().context("<flip> is a decimal")?),
                _ => bail!("{USAGE}"),
            };
            match Control::new(socket, &config_dir()?).pad_state(port, flip)? {
                None => println!("no read"),
                Some(read) => println!(
                    "flip={} reads={} differ={} {:?}",
                    read.flip,
                    read.reads,
                    read.differ,
                    read.pad.state()
                ),
            }
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

fn twists(rest: &[&str], default: u64) -> Result<u64> {
    match rest {
        [] => Ok(default),
        ["--max-twists", n] => n.parse().context("--max-twists takes a count"),
        _ => bail!("{USAGE}"),
    }
}

/// Giants' RNG through PINE (AF.8), and two peeks or one read back from files.
fn rng_cmd(rest: &[&str]) -> Result<()> {
    match rest {
        ["census", "--pine", socket] => {
            let mut pine = PineClient::connect(socket.as_ref())?;
            let census = rng::census(&mut pine, &scan::PPU_AREAS)?;
            println!("{}", census.peek.summary());
            for (i, found) in census.searches.iter().enumerate() {
                println!(
                    "search {}: {} bytes in {} mapped ranges, {} unreadable 4 KiB pages, {} PINE messages",
                    i + 1,
                    found.bytes,
                    found.mapped.len(),
                    found.unreadable.len(),
                    found.messages
                );
            }
            for hit in &census.hits {
                match hit {
                    Hit::Instance { at, refcount, mti } => {
                        let owners: Vec<String> = census.owners[at]
                            .iter()
                            .map(|o| format!("{o:#010x}"))
                            .collect();
                        println!(
                            "instance {at:#010x} refcount {refcount} mti {mti}, pointed at from [{}]",
                            owners.join(", ")
                        );
                    }
                    Hit::MetaField { at } => println!("{at:#010x}: the meta's vtable field"),
                    Hit::Other { at } => println!("{at:#010x}: the vtable word, not an object"),
                }
            }
        }
        ["peek", "--pine", socket, opts @ ..] => {
            let (mut p1, mut out) = (None, None);
            let mut opts = opts;
            while let [flag, value, tail @ ..] = opts {
                match *flag {
                    "--p1" => p1 = Some(PathBuf::from(value)),
                    "--out" => out = Some(PathBuf::from(value)),
                    _ => bail!("{USAGE}"),
                }
                opts = tail;
            }
            if !opts.is_empty() {
                bail!("{USAGE}");
            }
            let mut pine = PineClient::connect(socket.as_ref())?;
            rng::check_title(&mut pine)?;
            let mut peek = Peek::read(&mut pine)?;
            if let Some(p1) = p1 {
                let state =
                    skylander_rpcs3_control::ipc::IpcPortalDriver::with_path(&p1).read_state()?;
                peek.flip = Some(state.frames);
            }
            println!(
                "{}{}",
                peek.flip.map_or(String::new(), |f| format!("flip {f}: ")),
                peek.summary()
            );
            match peek.origin(rng::ORIGIN_TWISTS) {
                Some(o) => println!("seeded {} (sgenrand), {} drawn since", o.seed, o.draws),
                None => println!(
                    "no sgenrand seeding within {} twists back",
                    rng::ORIGIN_TWISTS
                ),
            }
            if let Some(out) = out {
                println!("written to {}", peek.write(&out)?.display());
            }
        }
        ["seed", "--pine", socket, how @ ..] => {
            let mut pine = PineClient::connect(socket.as_ref())?;
            rng::check_title(&mut pine)?;
            match how {
                ["--from", file] => {
                    let peek = Peek::load(file.as_ref())?;
                    rng::restore(&mut pine, &peek)?;
                    println!("put back {file}: {}", peek.summary());
                }
                [n] => {
                    let seed = n.parse().context("<n> is a u32")?;
                    let at = rng::seed(&mut pine, seed)?;
                    println!("sgenrand({seed}) written to the MT at {at:#010x}, mti 624");
                }
                _ => bail!("{USAGE}"),
            }
            println!("now: {}", Peek::read(&mut pine)?.summary());
        }
        ["distance", a, b, rest @ ..] => {
            let max = twists(rest, rng::MAX_TWISTS)?;
            let (a, b) = (Peek::load(a.as_ref())?, Peek::load(b.as_ref())?);
            let apart = a.apart(&b, max);
            let show = |n: Option<i64>, what: &str| {
                n.map_or(format!("not within reach ({what})"), |n| n.to_string())
            };
            println!(
                "MT: {} draws; animation LCG: {} steps",
                show(apart.mt, &format!("{max} twists")),
                show(apart.anim, &format!("{} steps", rng::ANIM_STEPS))
            );
        }
        ["origin", file, rest @ ..] => {
            let max = twists(rest, rng::ORIGIN_TWISTS)?;
            let peek = Peek::load(file.as_ref())?;
            match peek.origin(max) {
                Some(o) => println!("seeded {} (sgenrand), {} drawn since", o.seed, o.draws),
                None => println!("no sgenrand seeding within {max} twists back"),
            }
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

/// Giants' actors and camera through PINE at a pause (AF.9).
fn actors_cmd(socket: &std::path::Path, mut rest: &[&str]) -> Result<()> {
    let (mut p1, mut out, mut all) = (None, None, false);
    while let [flag, tail @ ..] = rest {
        rest = tail;
        match (*flag, rest) {
            ("--p1", [path, tail @ ..]) => {
                p1 = Some(PathBuf::from(path));
                rest = tail;
            }
            ("--out", [path, tail @ ..]) => {
                out = Some(PathBuf::from(path));
                rest = tail;
            }
            ("--all", _) => all = true,
            _ => bail!("{USAGE}"),
        }
    }
    let mut pine = PineClient::connect(socket)?;
    let mut frame: ActorFrame = actors::read_frame(&mut pine)?;
    if let Some(p1) = p1 {
        frame.flip = Some(
            skylander_rpcs3_control::ipc::IpcPortalDriver::with_path(&p1)
                .read_state()?
                .frames,
        );
    }
    println!("{}", frame.summary());
    match frame.hero() {
        Ok(hero) => println!("hero: {}", hero.summary()),
        Err(e) => println!("hero: {e}"),
    }
    if all {
        for actor in &frame.actors {
            println!("  {:#010x} {}", actor.corpus, actor.summary());
        }
    }
    for s in &frame.skipped {
        println!("  skipped {:#010x}: {}", s.corpus, s.why);
    }
    if let Some(out) = out {
        println!("written to {}", frame.write(&out)?.display());
    }
    Ok(())
}

/// O5's watch (see the usage).
fn watch_cmd(rest: &[&str]) -> Result<()> {
    use guac_oracle::recording::{Alignment, Order, Recording};
    use oracle_harness::watch::{GiantsWatch, RawRecord, RingCheck, WatchFile};

    let flag = |rest: &[&str], name: &str| -> Option<String> {
        rest.iter()
            .position(|a| *a == name)
            .and_then(|i| rest.get(i + 1))
            .map(|v| (*v).to_owned())
    };
    let control = |socket: &str| -> Result<Control> {
        Ok(Control::new(
            socket.as_ref(),
            &OracleHome::open(&required(env::ORACLE_HOME)?)?.config_dir(),
        ))
    };
    match rest {
        ["arm", "--socket", socket, opts @ ..] => {
            let pine_at = flag(opts, "--pine").with_context(|| USAGE.to_owned())?;
            let list_at = flag(opts, "--list").with_context(|| USAGE.to_owned())?;
            let frames: u32 = match flag(opts, "--frames") {
                Some(n) => n.parse().context("--frames takes a count")?,
                None => 1024,
            };
            let mut pine = PineClient::connect(pine_at.as_ref())?;
            rng::check_title(&mut pine)?;
            let watch = GiantsWatch::resolve(&mut pine)?;
            let chained = match flag(opts, "--chains") {
                Some(at) => {
                    let chains = GiantsChains::resolve(&mut pine, &watch, &chains::fight_kinds())?;
                    let path = rrc_dump::output::refuse_repo_output(at.as_ref())?;
                    std::fs::write(&path, serde_json::to_string_pretty(&chains)?)?;
                    Some(chains)
                }
                None => None,
            };
            drop(pine);
            let path = rrc_dump::output::refuse_repo_output(list_at.as_ref())?;
            std::fs::write(&path, serde_json::to_string_pretty(&watch)?)?;
            let control = control(socket)?;
            let armed = match &chained {
                Some(c) => {
                    for a in control.chains_arm(&c.chains)? {
                        println!(
                            "chain {}: {} listed, {} kept when armed",
                            a.slot, a.listed, a.kept
                        );
                    }
                    c.chains.clone()
                }
                None => Default::default(),
            };
            control.watch_arm_chained(&watch.list, &armed, watch.port, frames)?;
            println!(
                "armed: {} fields in {} ranges, {} bytes a record, {frames} records at every read \
                 of port {}; hero {} at {:#010x}, {} clips; list in {}",
                watch.list.fields().len(),
                watch.list.ranges().len(),
                watch.list.bytes(),
                watch.port,
                watch.hero_name.as_deref().unwrap_or("?"),
                watch.hero_corpus,
                watch.clips.len(),
                path.display()
            );
        }
        ["read", "--socket", socket, opts @ ..] => {
            let list_at = flag(opts, "--list").with_context(|| USAGE.to_owned())?;
            let out = PathBuf::from(flag(opts, "--out").with_context(|| USAGE.to_owned())?);
            let watch: GiantsWatch = serde_json::from_str(
                &std::fs::read_to_string(&list_at).with_context(|| format!("reading {list_at}"))?,
            )
            .with_context(|| format!("parsing {list_at}"))?;
            let control = control(socket)?;
            let mut ring = control.watch_read()?;
            let (pads, reads) = control.pad_readback(watch.port, &ring.flips())?;
            let identity = match env::path(env::ORACLE_EXE) {
                Some(exe) => Identity::of(
                    &exe,
                    &OracleHome::open(&required(env::ORACLE_HOME)?)?.config_dir(),
                )?
                .line(),
                None => "?".into(),
            };
            let mut chain_clips = None;
            if let Some(at) = flag(opts, "--chains") {
                let chains: GiantsChains = serde_json::from_str(
                    &std::fs::read_to_string(&at).with_context(|| format!("reading {at}"))?,
                )
                .with_context(|| format!("parsing {at}"))?;
                let pine_at = flag(opts, "--pine").context("--chains needs --pine to name them")?;
                let mut pine = PineClient::connect(pine_at.as_ref())?;
                rng::check_title(&mut pine)?;
                let written =
                    chains.write_outputs(&watch, &ring.records, &mut pine, &identity, &out)?;
                drop(pine);
                ring.records = written.o5;
                println!("chains: {}", serde_json::to_string(&written.run)?);
                chain_clips = Some(written.clips);
            }
            let file = WatchFile {
                identity,
                watch,
                held: ring.summary.held,
                total: ring.summary.total,
                frames: ring.summary.frames,
                armed: ring.summary.armed,
                records: ring.records.iter().map(RawRecord::of).collect(),
                pads,
                reads,
            };
            let dir = rrc_dump::output::refuse_repo_output(&out)?;
            std::fs::create_dir_all(&dir)?;
            file.write(&dir.join(WatchFile::FILE))?;
            let recording = file.recording()?;
            let recording = match &chain_clips {
                Some(c) => oracle_harness::chains::fill_clips(recording, c)?.0,
                None => recording,
            };
            std::fs::write(
                dir.join(divergence::RECORDING_FILE),
                serde_json::to_string(&recording)?,
            )?;
            let check = RingCheck::of(&recording, &ring.records, &file.reads);
            println!(
                "{} held of {} taken (armed at {} on port {}): {check:?}; {}",
                ring.summary.held,
                ring.summary.total,
                ring.summary.armed,
                ring.summary.port,
                if check.holds() {
                    "every game frame once"
                } else {
                    "GAPS"
                }
            );
        }
        ["clear", "--socket", socket] => {
            control(socket)?.watch_clear()?;
            println!("cleared");
        }
        ["unchain", "--socket", socket] => {
            control(socket)?.chain_clear()?;
            println!("chains cleared");
        }
        ["residuals", file, opts @ ..] => {
            let recording: Recording = serde_json::from_str(
                &std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?,
            )
            .with_context(|| format!("parsing {file}"))?;
            let bound = |name: &str| -> Result<Option<u64>> {
                flag(opts, name)
                    .map(|v| v.parse().with_context(|| format!("{name} takes a flip")))
                    .transpose()
            };
            let (from, to) = (
                bound("--from")?.unwrap_or(0),
                bound("--to")?.unwrap_or(u64::MAX),
            );
            for alignment in Alignment::ALL {
                for order in Order::ALL {
                    let r = recording.residuals(alignment, order, |f| f >= from && f < to);
                    println!(
                        "{alignment:?} {order:?}: {} pairs, ground max {:.3e} m (at flip {}) rms \
                         {:.3e} m, height max {:.3e} m",
                        r.pairs,
                        r.max_ground,
                        r.worst_flip.map_or("-".into(), |f| f.to_string()),
                        r.rms_ground,
                        r.max_height
                    );
                }
            }
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

/// AH.7's `oracle goto`: the route driven on a fresh RPCS3, as the usage says.
fn goto(mut rest: &[&str]) -> Result<()> {
    let (mut out, mut route, mut state, mut figure, mut knobs) = (None, None, None, None, None);
    let (mut watch, mut chained) = (false, false);
    let mut ring = 6_000u32;
    let mut boot_wait = divergence::BOOT_WAIT;
    let mut max_load = 32.0f64;
    let mut seed = None;
    let mut pokes = Vec::new();
    loop {
        match rest {
            [] => break,
            ["--watch", tail @ ..] => {
                watch = true;
                rest = tail;
            }
            ["--chains", tail @ ..] => {
                (watch, chained) = (true, true);
                rest = tail;
            }
            [flag, value, tail @ ..] => {
                rest = tail;
                match *flag {
                    "--out" => out = Some(PathBuf::from(value)),
                    "--route" => route = Some(PathBuf::from(value)),
                    "--state" => state = Some(PathBuf::from(value)),
                    "--figure" => figure = Some(PathBuf::from(value)),
                    "--knobs" => knobs = Some(PathBuf::from(value)),
                    "--ring" => ring = value.parse().context("--ring takes a count")?,
                    "--boot-secs" => {
                        boot_wait =
                            Duration::from_secs(value.parse().context("--boot-secs takes seconds")?)
                    }
                    "--max-load" => max_load = value.parse().context("--max-load takes a load")?,
                    "--seed" => seed = Some(value.parse().context("--seed takes a u32")?),
                    "--poke" => pokes.push(route::session::Poke::parse(value)?),
                    _ => bail!("{USAGE}"),
                }
            }
            _ => bail!("{USAGE}"),
        }
    }
    let out = rrc_dump::output::refuse_repo_output(&out.context("--out <dir> is required")?)?;
    let route_file = route.context("--route <route.ron> is required")?;
    let plan = route::RoutePlan::from_ron(
        &std::fs::read_to_string(&route_file)
            .with_context(|| format!("reading {}", route_file.display()))?,
    )?;
    let knobs = match knobs {
        Some(k) => ron::from_str(&std::fs::read_to_string(&k)?).context("parsing the knobs")?,
        None => route::Knobs::default(),
    };
    let state = match state {
        Some(s) => s,
        None => required("GUAC_ORACLE_SAVESTATE")?,
    };
    let figure = match figure {
        Some(f) => f,
        None => required("GUAC_ORACLE_FIGURE")?,
    };
    let (rows, disc) = match env::path(env::GIANTS_DUMP) {
        Some(dump) => {
            let usrdir = dump.join("PS3_GAME/USRDIR");
            let progression = guac_extract::progression::read(
                &usrdir,
                guac_ir::DumpIdentity {
                    title_id: "BLUS30968".into(),
                    app_ver: "01.00".into(),
                    manifest_hash: guac_ir::Sha256([0; 32]),
                },
            )?;
            (
                Some(oracle_harness::fight::Rows::of(&progression)?),
                Some(route::census::Disc::read(&usrdir)?),
            )
        }
        None => (None, None),
    };
    let exe = required(env::ORACLE_EXE)?;
    let home = OracleHome::open(&required(env::ORACLE_HOME)?)?;
    let dev_dirs = dev_rpcs3_dirs(&home::real_home()?);
    let before = Snapshot::take(&dev_dirs)?;
    let carried_dirs = divergence::carried_dirs(&home.config_dir());
    let carried = Snapshot::take(&carried_dirs)?;
    let figure_hash = sha256_hex(&figure)?;
    let mut lines = vec![
        Identity::of(&exe, &home.config_dir())?.line(),
        format!(
            "goto {}: home {}, state {} ({}), figure {} ({}), watch {watch}, chains {chained}, \
             rows {}",
            route_file.display(),
            home.root().display(),
            state.display(),
            &sha256_hex(&state)?[..16],
            figure.display(),
            &figure_hash[..16],
            rows.is_some()
        ),
    ];
    loop {
        let host = divergence::Host::now();
        match host.load_average {
            Some([l1, ..]) if l1 > max_load => {
                let held = format!("hold: 1-minute load {l1} over {max_load}; {}", host.uptime);
                println!("{held}");
                lines.push(held);
                std::thread::sleep(Duration::from_secs(30));
            }
            _ => {
                lines.push(format!(
                    "start: {}; screen locked {:?}",
                    host.uptime, host.screen_locked
                ));
                break;
            }
        }
    }
    let inputs = route::session::Inputs {
        exe: &exe,
        home: &home,
        state: &state,
        figure: &figure,
        boot_wait,
        rows: rows.as_ref(),
        disc: disc.as_ref(),
        watch,
        chains: chained,
        ring,
        seed,
        pokes: &pokes,
    };
    let result = route::session::run(&inputs, &plan, knobs, &out);
    match &result {
        Ok(r) => lines.push(format!(
            "{:?} after {} flips over {} looks; F0 {}, boot {:.1} s, wall {:.0} s; shots {}, reads \
             {}; watch {}; state {}; silent {}{}; {} -> {}",
            r.outcome,
            r.flips,
            r.looks,
            r.f0,
            r.boot_secs,
            r.wall_secs,
            r.shots.len(),
            r.reads.len(),
            r.watch.as_ref().map_or("none".into(), |w| format!(
                "{} records, check {:?}",
                w.held, w.check
            )),
            r.saved.as_ref().map_or_else(
                || r.save_error.clone().unwrap_or_else(|| "not saved".into()),
                |s| format!("{} at {} ({}…)", s.file, s.saved_at, &s.sha256[..16])
            ),
            r.silent,
            if r.fatal.is_empty() {
                String::new()
            } else {
                format!(
                    "; RPCS3 logged {} fatal error(s): {}",
                    r.fatal.len(),
                    r.fatal[0]
                )
            },
            r.before.uptime,
            r.after.uptime
        )),
        Err(e) => lines.push(format!("failed: {e:#}")),
    }
    let changed = before.diff(&Snapshot::take(&dev_dirs)?);
    let home_changed = carried.diff(&Snapshot::take(&carried_dirs)?);
    lines.push(format!(
        "end: {}; the home's saves and trophies {}; the figure source {}; the dev's RPCS3 dirs {}",
        divergence::Host::now().uptime,
        if home_changed.is_empty() {
            format!("unchanged, {} entries", carried.len())
        } else {
            format!("CHANGED:\n{}", home_changed.join("\n"))
        },
        if sha256_hex(&figure)? == figure_hash {
            "unchanged"
        } else {
            "CHANGED"
        },
        if changed.is_empty() {
            format!("unchanged, {} entries", before.len())
        } else {
            format!("CHANGED:\n{}", changed.join("\n"))
        }
    ));
    for l in &lines[2..] {
        println!("{l}");
    }
    if out.exists() {
        std::fs::write(out.join("run.txt"), lines.join("\n") + "\n")?;
    }
    if !changed.is_empty() {
        bail!("the dev's RPCS3 dirs changed during the drive");
    }
    result.map(|_| ())
}

/// AF.11's replays (see the usage).
fn divergence_run(mut rest: &[&str]) -> Result<()> {
    let (mut out, mut state, mut figure, mut warmup) = (None, None, None, None);
    let mut scripts = vec!["idle".to_owned(), "walk".to_owned()];
    let mut sets = vec![Set::A, Set::B];
    let (mut replays, mut first, mut attempts) = (10u32, 0u32, 3u32);
    let mut boot_wait = divergence::BOOT_WAIT;
    let (mut end, mut every) = (divergence::END, divergence::EVERY);
    let mut perturbations = Vec::new();
    let mut gap = Duration::from_secs(30);
    while let [flag, value, tail @ ..] = rest {
        rest = tail;
        match *flag {
            "--gap-secs" => {
                gap = Duration::from_secs(value.parse().context("--gap-secs takes seconds")?)
            }
            "--perturb" => perturbations.push(Perturbation::parse(value)?),
            "--out" => out = Some(PathBuf::from(value)),
            "--state" => state = Some(PathBuf::from(value)),
            "--figure" => figure = Some(PathBuf::from(value)),
            "--warmup" => warmup = Some((*value).to_owned()),
            "--scripts" => scripts = value.split(',').map(str::to_owned).collect(),
            "--sets" => sets = value.split(',').map(Set::parse).collect::<Result<_>>()?,
            "--replays" => replays = value.parse().context("--replays takes a count")?,
            "--first" => first = value.parse().context("--first takes an index")?,
            "--attempts" => attempts = value.parse().context("--attempts takes a count")?,
            "--end" => end = value.parse().context("--end takes flips")?,
            "--every" => every = value.parse().context("--every takes flips")?,
            "--boot-secs" => {
                boot_wait = Duration::from_secs(value.parse().context("--boot-secs takes seconds")?)
            }
            _ => bail!("{USAGE}"),
        }
    }
    if !rest.is_empty() || attempts == 0 {
        bail!("{USAGE}");
    }
    let out = rrc_dump::output::refuse_repo_output(&out.context("--out <dir> is required")?)?;
    let state = match state {
        Some(s) => s,
        None => required("GUAC_ORACLE_SAVESTATE")?,
    };
    let figure = match figure {
        Some(f) => f,
        None => required("GUAC_ORACLE_FIGURE")?,
    };
    // Every plan is checked before the first boot, so a bad script fails at once.
    let plans: Vec<Plan> = scripts
        .iter()
        .flat_map(|s| sets.iter().map(move |&set| (s, set)))
        .map(|(s, set)| Plan::named(s, set, end, every))
        .collect::<Result<_>>()?;
    let plans = if perturbations.is_empty() {
        plans
    } else {
        plans
            .iter()
            .flat_map(|plan| perturbations.iter().map(move |p| plan.perturbed(p)))
            .collect::<Result<_>>()?
    };
    let warmup = warmup
        .map(|w| Plan::named(&w, Set::B, end, every))
        .transpose()?;
    let exe = required(env::ORACLE_EXE)?;
    let home = OracleHome::open(&required(env::ORACLE_HOME)?)?;
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
    let dev_dirs = dev_rpcs3_dirs(&home::real_home()?);
    let before = Snapshot::take(&dev_dirs)?;
    let figure_hash = sha256_hex(&figure)?;
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out.join("run.txt"))?;
    let mut line = |text: String| -> Result<()> {
        use std::io::Write;
        println!("{text}");
        writeln!(log, "{text}")?;
        Ok(())
    };
    line(Identity::of(&exe, &home.config_dir())?.line())?;
    let host = divergence::Host::now();
    line(format!(
        "start: home {}, state {} ({}), figure {} ({}); load average {:?}; screen locked {:?}; \
         the dev's RPCS3 dirs: {} entries",
        home.root().display(),
        state.display(),
        &sha256_hex(&state)?[..16],
        figure.display(),
        &figure_hash[..16],
        host.load_average,
        host.screen_locked,
        before.len()
    ))?;
    let inputs = Inputs {
        exe: &exe,
        home: &home,
        state: &state,
        figure: &figure,
        boot_wait,
    };
    // Replays share the home: what a state's load doesn't put back has to come
    // through each replay unchanged, or the next one isn't independent.
    let carried_dirs = divergence::carried_dirs(&home.config_dir());
    let carried = Snapshot::take(&carried_dirs)?;
    let independent = |label: &str| -> Result<()> {
        let changed = carried.diff(&Snapshot::take(&carried_dirs)?);
        if !changed.is_empty() {
            bail!(
                "{label} left the home's saves or trophies changed, so the replays after it \
                 wouldn't be independent; stopping:\n{}",
                changed.join("\n")
            );
        }
        Ok(())
    };
    let mut failed = Vec::new();
    let mut spacing = Spacing { gap, last: None };
    if let Some(plan) = &warmup {
        let dir = out.join("warmup");
        if run_attempts(&inputs, plan, 0, &dir, attempts, &mut spacing, &mut line)?.is_none() {
            failed.push(format!("warmup {}", plan.label(0)));
        }
        independent(&format!("the warm-up {}", plan.label(0)))?;
    }
    for replay in first..first + replays {
        for plan in &plans {
            if run_attempts(
                &inputs,
                plan,
                replay,
                &out,
                attempts,
                &mut spacing,
                &mut line,
            )?
            .is_none()
            {
                failed.push(plan.label(replay));
            }
            independent(&plan.label(replay))?;
        }
    }
    let host = divergence::Host::now();
    let changed = before.diff(&Snapshot::take(&dev_dirs)?);
    line(format!(
        "end: load average {:?}; screen locked {:?}; failed {:?}; the home's saves and trophies \
         unchanged, {} entries; the figure source {}; the dev's RPCS3 dirs {}",
        host.load_average,
        host.screen_locked,
        failed,
        carried.len(),
        if sha256_hex(&figure)? == figure_hash {
            "unchanged"
        } else {
            "CHANGED"
        },
        if changed.is_empty() {
            format!("unchanged, {} entries", before.len())
        } else {
            format!("CHANGED:\n{}", changed.join("\n"))
        }
    ))?;
    if !changed.is_empty() {
        bail!("the dev's RPCS3 dirs changed during the measurement");
    }
    Ok(())
}

fn sweep_list(pack: &std::path::Path) -> Result<()> {
    let roster = sweep::roster(&required(env::GIANTS_DUMP)?)?;
    let list = sweep::figures(&roster, &sweep::scan_pack(pack)?);
    for f in &list {
        match &f.sky {
            Some(s) => println!(
                "{} {} ({}, {:?}): {} ({:#06x}){}",
                f.id,
                f.name,
                f.load_file,
                f.kind,
                s.path.display(),
                s.word,
                if s.others.is_empty() {
                    String::new()
                } else {
                    format!(", {} more read as it", s.others.len())
                }
            ),
            None => println!(
                "{} {} ({}, {:?}): no .sky in the pack",
                f.id, f.name, f.load_file, f.kind
            ),
        }
    }
    let loads: std::collections::BTreeSet<&str> =
        list.iter().map(|f| f.load_file.as_str()).collect();
    println!(
        "{} figures with a character model over {} load files; {} with a .sky, {} without",
        list.len(),
        loads.len(),
        list.iter().filter(|f| f.sky.is_some()).count(),
        list.iter().filter(|f| f.sky.is_none()).count()
    );
    Ok(())
}

fn sweep_run(mut rest: &[&str]) -> Result<()> {
    let (mut out, mut pack, mut state) = (None, None, None);
    let mut keys = Vec::new();
    let (mut attempts, mut settle) = (2u32, sweep::SETTLE);
    let (mut boot_wait, mut gap) = (sweep::BOOT_WAIT, Duration::from_secs(30));
    while let [flag, value, tail @ ..] = rest {
        rest = tail;
        match *flag {
            "--out" => out = Some(PathBuf::from(value)),
            "--pack" => pack = Some(PathBuf::from(value)),
            "--state" => state = Some(PathBuf::from(value)),
            "--figures" => keys = value.split(',').map(str::to_owned).collect(),
            "--attempts" => attempts = value.parse().context("--attempts takes a count")?,
            "--settle-flips" => settle = value.parse().context("--settle-flips takes flips")?,
            "--boot-secs" => {
                boot_wait = Duration::from_secs(value.parse().context("--boot-secs takes seconds")?)
            }
            "--gap-secs" => {
                gap = Duration::from_secs(value.parse().context("--gap-secs takes seconds")?)
            }
            _ => bail!("{USAGE}"),
        }
    }
    if !rest.is_empty() {
        bail!("{USAGE}");
    }
    let out = out.context("--out <dir> is required")?;
    let opts = sweep::SweepOptions {
        out: out.clone(),
        pack: pack.context("--pack <dir> is required")?,
        state: match state {
            Some(s) => s,
            None => required("GUAC_ORACLE_SAVESTATE")?,
        },
        dump: required(env::GIANTS_DUMP)?,
        keys,
        attempts,
        gap,
        boot_wait,
        settle,
    };
    let exe = required(env::ORACLE_EXE)?;
    let home = OracleHome::open(&required(env::ORACLE_HOME)?)?;
    let out = rrc_dump::output::refuse_repo_output(&out)?;
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out.join("run.txt"))?;
    let rows = sweep::run_sweep(&exe, &home, &opts, |text| {
        use std::io::Write;
        println!("{text}");
        writeln!(log, "{text}")?;
        Ok(())
    })?;
    let failed = sweep::Counts::of(&rows).failed;
    if !failed.is_empty() {
        bail!("{} figure(s) failed every try: {failed:?}", failed.len());
    }
    Ok(())
}

/// The wait between one RPCS3's exit and the next boot. AF.12: each of the 8
/// boots the runner started straight after the last replay's RPCS3 exited
/// failed to create its CoreMIDI client (`MidiInCore::initialize ... (-304)`
/// on RPCS3's stderr) and hung at `ready` with no flip; none of the 46 started
/// 30 s after an exit, or first in a run, did. AF.11's one hang (1 of 50
/// boots) printed the same line.
struct Spacing {
    gap: Duration,
    last: Option<std::time::Instant>,
}

impl Spacing {
    fn wait(&self) {
        if let Some(last) = self.last {
            std::thread::sleep(self.gap.saturating_sub(last.elapsed()));
        }
    }
}

/// One replay, tried up to `attempts` times: a failed try's dir is renamed
/// `<label>.failed-<k>` with its error beside what it wrote, and the next try
/// waits out the spacing (another lane's oracle holding the GDB ports, say,
/// or the CoreMIDI hang).
fn run_attempts(
    inputs: &Inputs,
    plan: &Plan,
    replay: u32,
    out: &std::path::Path,
    attempts: u32,
    spacing: &mut Spacing,
    line: &mut impl FnMut(String) -> Result<()>,
) -> Result<Option<ReplayRecord>> {
    let label = plan.label(replay);
    for attempt in 1..=attempts {
        spacing.wait();
        let result = divergence::run_replay(inputs, plan, replay, out);
        spacing.last = Some(std::time::Instant::now());
        match result {
            Ok(record) => {
                line(summary(&record))?;
                return Ok(Some(record));
            }
            Err(e) => {
                line(format!(
                    "{label}: try {attempt} of {attempts} failed: {e:#}"
                ))?;
                let dir = out.join(&label);
                if dir.exists() {
                    let failed = out.join(format!("{label}.failed-{attempt}"));
                    std::fs::rename(&dir, &failed)?;
                    std::fs::write(failed.join("error.txt"), format!("{e:#}\n"))?;
                }
            }
        }
    }
    Ok(None)
}

/// One line for `run.txt`.
fn summary(r: &ReplayRecord) -> String {
    let end = r.observations.last();
    let f0 = r.observations.first();
    let hero = end
        .and_then(|o| o.hero.as_ref())
        .map_or("no hero".into(), |h| {
            format!(
                "hero ({:.4}, {:.4}, {:.4}) heading {:.2}",
                h.position[0], h.position[1], h.position[2], h.rotation[1]
            )
        });
    let drawn = match (end.and_then(|o| o.rng.drawn), f0.and_then(|o| o.rng.drawn)) {
        (Some(e), Some(s)) => format!("MT +{} since F0", e as i64 - s as i64),
        _ => "MT ?".into(),
    };
    format!(
        "{}: F0 {}, boot {:.1} s, running {:.1} s, wall {:.0} s; load {:?} -> {:?}; end flip {} \
         (game frame {}): {hero}, {} listed, {drawn}; shot {}; state {}{}; silent {}{}",
        r.label,
        r.f0,
        r.boot_secs,
        r.run_secs,
        r.wall_secs,
        r.before.load_average,
        r.after.load_average,
        end.map_or(0, |o| o.flip),
        end.map_or(0, |o| o.game_frame),
        end.map_or(0, |o| o.counts.listed),
        r.shot
            .as_ref()
            .map_or("none".into(), |s| format!("flip {}", s.flip)),
        r.saved.as_ref().map_or_else(
            || r.save_error.clone().unwrap_or_else(|| "not asked".into()),
            |s| format!("at {} ({} bytes)", s.saved_at, s.bytes)
        ),
        if r.save_refusals.is_empty() {
            String::new()
        } else {
            format!(", {} refusal(s)", r.save_refusals.len())
        },
        r.silent,
        if r.fatal.is_empty() {
            String::new()
        } else {
            format!(
                "; RPCS3 logged {} fatal error(s): {}",
                r.fatal.len(),
                r.fatal[0]
            )
        }
    )
}

fn divergence_report(rest: &[&str]) -> Result<()> {
    let [dir, tail @ ..] = rest else {
        bail!("{USAGE}");
    };
    let mut rest = tail;
    let (mut paused, mut out, mut choice) = (None, None, Choice::default());
    while let [flag, value, tail @ ..] = rest {
        rest = tail;
        match *flag {
            "--paused" => paused = Some(PathBuf::from(value)),
            "--rule" => choice = Choice::parse(value)?,
            "--out" => out = Some(PathBuf::from(value)),
            _ => bail!("{USAGE}"),
        }
    }
    if !rest.is_empty() {
        bail!("{USAGE}");
    }
    let dir = rrc_dump::output::refuse_repo_output(dir.as_ref())?;
    // Another output dir keeps <dir>'s own report (another rule's) as it was.
    let out = match out {
        Some(o) => rrc_dump::output::refuse_repo_output(&o)?,
        None => dir.clone(),
    };
    std::fs::create_dir_all(&out)?;
    let records = divergence::load_records(&dir)?;
    let paused = match paused {
        Some(p) => divergence::load_records(&p)?,
        None => Vec::new(),
    };
    let report = Report::build_with(&records, Some(&dir), &paused, choice)?;
    let text = report.text();
    std::fs::write(out.join("report.txt"), &text)?;
    std::fs::write(
        out.join("report.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    print!("{text}");
    Ok(())
}

fn divergence_calibrate(dir: &std::path::Path) -> Result<()> {
    let dir = rrc_dump::output::refuse_repo_output(dir)?;
    let records = divergence::load_records(&dir)?;
    let groups = calibrate::leave_one_out(&records, Some(&dir))?;
    let mut text = String::new();
    for g in &groups {
        g.write_text(&mut text);
    }
    std::fs::write(dir.join("calibrate.txt"), &text)?;
    std::fs::write(
        dir.join("calibrate.json"),
        serde_json::to_string_pretty(&groups)?,
    )?;
    print!("{text}");
    let failed: Vec<String> = groups
        .iter()
        .filter(|g| !g.passed())
        .map(|g| format!("{} set {}", g.script, g.set))
        .collect();
    if !failed.is_empty() {
        bail!("leave-one-out failed: {}", failed.join(", "));
    }
    Ok(())
}

fn divergence_check(mut rest: &[&str]) -> Result<()> {
    let (mut reference, mut script, mut set) = (None, None, None);
    while let [flag, value, tail @ ..] = rest {
        match *flag {
            "--reference" => reference = Some(PathBuf::from(value)),
            "--script" => script = Some((*value).to_owned()),
            "--set" => set = Some(Set::parse(value)?),
            _ => break,
        }
        rest = tail;
    }
    let [candidates] = rest else {
        bail!("{USAGE}");
    };
    let (Some(reference), Some(script), Some(set)) = (reference, script, set) else {
        bail!("{USAGE}");
    };
    let candidates = rrc_dump::output::refuse_repo_output(candidates.as_ref())?;
    let reference = Reference::load(&reference, &script, set)?;
    let mut checks = Vec::new();
    let mut text = String::new();
    for record in divergence::load_records(&candidates)? {
        if record.script == script {
            continue;
        }
        let check = calibrate::check(&reference, &record, &candidates)?;
        let mut one = String::new();
        check.write_text(&mut one);
        print!("{one}");
        text.push_str(&one);
        checks.push(check);
    }
    std::fs::write(candidates.join("check.txt"), &text)?;
    std::fs::write(
        candidates.join("check.json"),
        serde_json::to_string_pretty(&checks)?,
    )?;
    Ok(())
}

fn sha256_hex(path: &std::path::Path) -> Result<String> {
    use sha2::Digest;
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
