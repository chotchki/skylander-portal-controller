//! One replay against the real oracle, as the module doc lays it out.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;

use super::{
    CheckpointShot, EndShot, EndState, Host, Observation, Plan, Pose, ReplayRecord, RngObservation,
    SAVE_RETRY_FLIPS, SAVE_TRIES, WatchRun, actors_file, peek_file,
};
use crate::actors::{self, ActorFrame};
use crate::chains::{self, GiantsChains};
use crate::control::{Control, SAVESTATE_REFUSED, Saved};
use crate::home::OracleHome;
use crate::identity::Identity;
use crate::launch::{self, BootOptions};
use crate::outputs::{Kind, Rpcs3Outputs, hex};
use crate::pine::PineClient;
use crate::places::{PlaceSpec, PlaceWatch, PlacesFile};
use crate::rng::{self, Peek};
use crate::route::census::Disc;
use crate::watch::{GiantsWatch, RawRecord, RingCheck, WatchFile};

/// How long a savestate boot gets to reach Start Paused's pause unless the
/// caller says otherwise: a warm home takes 4 to 7 s, and AF.11 saw a restore
/// hang after the pad threads came up (status `ready`, no flip, no CPU), which
/// only a timeout ends. A cold home compiles for minutes first, so a first run
/// there passes more ([`Inputs::boot_wait`]).
pub const BOOT_WAIT: Duration = Duration::from_secs(120);
/// A shot, a save, a resume's pause: AF.5's live runs took up to 14 s for a
/// save under a load average of 40.
pub(crate) const STEP_WAIT: Duration = Duration::from_secs(180);
/// What RPCS3 logs when the template's silent audio took (AF.3's review).
pub(crate) const SILENT: &[u8] = b"cellAudio initializing. Backend: Null";
/// What RPCS3 logs when a guest thread dies: AF.11 saw the main PPU thread's
/// lwmutex check fail on a savestate's restore, before any flip (then
/// `STATE` stays at Start Paused's frame) or after the end state's reboot.
pub(crate) const FATAL: &[u8] = b"Thread terminated due to fatal error";

/// What every replay of a measurement shares.
pub struct Inputs<'a> {
    pub exe: &'a Path,
    pub home: &'a OracleHome,
    /// The gameplay savestate each replay boots.
    pub state: &'a Path,
    /// The `.sky` a fresh copy of goes on the portal at F0.
    pub figure: &'a Path,
    /// From the spawn to Start Paused's pause ([`BOOT_WAIT`] for a warm home).
    pub boot_wait: Duration,
    /// Level placements a set W replay watches too, with the disc they're
    /// found by (AG.176); none watches none.
    pub places: &'a [PlaceSpec],
    pub disc: Option<&'a Disc>,
}

/// Runs replay `replay` of `plan` into `out/<label>/` (refused inside any
/// work tree, and never reused) and writes its `record.json` there. What RPCS3
/// wrote under the home (the end shot, the end state) is removed once copied
/// out, so the home doesn't grow by a state per replay. On an error the dir
/// keeps whatever the replay wrote, RPCS3's log included.
pub fn run_replay(inputs: &Inputs, plan: &Plan, replay: u32, out: &Path) -> Result<ReplayRecord> {
    let label = plan.label(replay);
    let dir = rrc_dump::output::refuse_repo_output(&out.join(&label))?;
    if dir.exists() {
        bail!("{} exists: a replay's dir is never reused", dir.display());
    }
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let started = Instant::now();
    let before = Host::now();
    let identity = Identity::of(inputs.exe, &inputs.home.config_dir())?.line();
    let (state_sha256, figure_sha256) = (sha256_file(inputs.state)?, sha256_file(inputs.figure)?);
    let outputs = Rpcs3Outputs::new(inputs.home.config_dir());
    let kinds = [Kind::Screenshot, Kind::SaveState];
    let had = kinds.map(|k| outputs.list(k));

    let driven = drive(inputs, plan, &dir, &identity);

    for (kind, had) in kinds.into_iter().zip(&had) {
        for path in outputs.list(kind).difference(had) {
            let _ = fs::remove_file(path);
        }
    }
    // RPCS3's log isn't always UTF-8 (a fatal error's dump), so it's searched as bytes.
    let log = inputs.home.cache_dir().join("RPCS3.log");
    let text = fs::read(&log).unwrap_or_default();
    let silent = !log_lines(&text, SILENT).is_empty();
    let fatal = log_lines(&text, FATAL);
    let _ = fs::copy(&log, dir.join("RPCS3.log"));
    let driven = driven?;
    let record = ReplayRecord {
        label,
        script: plan.script_name.clone(),
        set: plan.set,
        replay,
        identity,
        state: inputs
            .state
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        state_sha256,
        figure_sha256,
        before,
        after: Host::now(),
        boot_secs: driven.boot_secs,
        f0: driven.f0,
        base: driven.base,
        run_secs: driven.run_secs,
        observations: driven.observations,
        shot: driven.shot,
        shots: driven.shots,
        watch: driven.watch,
        saved: driven.saved,
        save_refusals: driven.save_refusals,
        save_error: driven.save_error,
        silent,
        fatal,
        wall_secs: started.elapsed().as_secs_f64(),
        shutdown: driven.shutdown,
    };
    fs::write(dir.join("script.ron"), &plan.script_ron)?;
    record.write(&dir)?;
    Ok(record)
}

struct Driven {
    boot_secs: f64,
    f0: u64,
    base: u64,
    run_secs: f64,
    observations: Vec<Observation>,
    shot: Option<EndShot>,
    shots: Vec<CheckpointShot>,
    watch: Option<WatchRun>,
    saved: Option<EndState>,
    save_refusals: Vec<String>,
    save_error: Option<String>,
    shutdown: String,
}

fn drive(inputs: &Inputs, plan: &Plan, dir: &Path, identity: &str) -> Result<Driven> {
    let launched = Instant::now();
    let mut oracle = launch::boot(
        inputs.exe,
        inputs.home,
        &BootOptions {
            savestate: Some(inputs.state.to_path_buf()),
            borderless: true,
            ..BootOptions::default()
        },
    )?;
    let held = oracle.wait_paused(inputs.boot_wait)?;
    let boot_secs = launched.elapsed().as_secs_f64();
    let f0 = held.frames;
    let control = oracle.control();
    control.driver().reconnect()?;
    let copy = launch::figure_copy(&oracle.run_paths().tmp, "figure.sky", inputs.figure)?;
    control
        .driver()
        .load(SlotIndex::new(0).expect("slot 0"), &copy)?;
    let mut pine = pine_connect(&oracle.run_paths().pine_socket, Duration::from_secs(30))?;
    rng::check_title(&mut pine)?;
    let mut observations = vec![observe(&mut pine, 0, f0, dir)?];
    // Set C: a shot at every pause, after the reads, so it pauses where set B
    // does, only for longer (AF.12: its trajectories matched set B's).
    let mut shots = Vec::new();
    if plan.shoots_checkpoints() {
        shots.push(checkpoint_shot(&control, 0, dir)?);
    }
    // Set W: Giants' list resolved at F0 and armed before the script loads,
    // so the first record is F0's own first pad read once the run resumes.
    // Set O: O6's chains armed first, so each record carries them too.
    let watched = if plan.watches() {
        let watch = GiantsWatch::resolve(&mut pine).context("the watch list at F0")?;
        let watch = if inputs.places.is_empty() {
            watch
        } else {
            let disc = inputs
                .disc
                .context("watching places needs the disc ($GUAC_GIANTS_DUMP)")?;
            let frame = actors::read(&mut pine)?;
            let float_type = watch.mover.as_ref().map(|m| m.float_type);
            let places = PlaceWatch::resolve(&mut pine, &frame, disc, inputs.places, float_type)
                .context("the places at F0")?;
            watch.with_places(places)?
        };
        let chains = if plan.chains() {
            let chains = GiantsChains::resolve(&mut pine, &watch, &chains::fight_kinds())
                .context("Giants' chains at F0")?;
            control.chains_arm(&chains.chains)?;
            Some(chains)
        } else {
            None
        };
        let armed = chains
            .as_ref()
            .map(|c| c.chains.clone())
            .unwrap_or_default();
        control.watch_arm_chained(&watch.list, &armed, watch.port, plan.ring_frames())?;
        Some((watch, chains))
    } else {
        None
    };
    let loaded = control.load_script(&plan.script)?;

    // Only the running stretches count: a checkpoint's reads happen at a pause.
    let mut running = Duration::ZERO;
    let mut last = 0;
    for offset in plan.checkpoints() {
        // Allow 20 fps, a third of the game's, plus a minute.
        let wait = Duration::from_secs(60 + (offset - last) / 20);
        let t = Instant::now();
        let paused = control
            .pause_at(f0 + offset, wait)
            .with_context(|| format!("the checkpoint at F0 + {offset}"))?;
        running += t.elapsed();
        observations.push(observe(&mut pine, offset, paused.frames, dir)?);
        if plan.shoots_checkpoints() {
            shots.push(checkpoint_shot(&control, offset, dir)?);
        }
        last = offset;
    }

    let watch = match &watched {
        Some((w, c)) => Some(
            read_watch(&control, w, c.as_ref(), &mut pine, identity, dir)
                .context("reading the watch")?,
        ),
        None => None,
    };
    let shot = match shots.last() {
        Some(last) => last.shot.clone(),
        None => end_shot(&control, dir)?,
    };
    // RPCS3's PINE thread blocks reading its one client, and a stop (a save,
    // the shutdown) waits on that thread: a held connection hangs it (AF.8).
    drop(pine);
    let (saved, save_refusals, save_error) = if plan.saves_at_end() {
        let tried = save_retrying(&control, dir);
        // A state written whose reboot then died is still the state asked for.
        let written = match (&tried.saved, tried.asked_at) {
            (Some(saved), _) => Some((saved.path.clone(), saved.saved_at.frames)),
            (None, Some(asked_at)) => saved_in(dir).map(|p| (p, asked_at)),
            (None, None) => None,
        };
        let saved = match written {
            Some((path, saved_at)) => Some(EndState {
                file: file_name(&path),
                saved_at,
                bytes: fs::metadata(&path)?.len(),
                sha256: sha256_file(&path)?,
            }),
            None => None,
        };
        (saved, tried.refusals, tried.error)
    } else {
        (None, Vec::new(), None)
    };
    let shutdown = format!("{:?}", oracle.shutdown()?);
    Ok(Driven {
        boot_secs,
        f0,
        base: loaded.base,
        run_secs: running.as_secs_f64(),
        observations,
        shot: Some(shot),
        shots,
        watch,
        saved,
        save_refusals,
        save_error,
        shutdown,
    })
}

/// At the end pause: the ring and O4's readback on the watched port for its
/// flips (with the port's reads in each), both written raw beside the record
/// (`watch.json`) with the recording decoded from them (`recording.json`), and
/// AG.1's and AG.35's checks on it. A set O ring's records carry O6's chains
/// after the list: split off, named through `pine` and written beside it
/// ([`chains::CHAIN_FILE`] and the rest).
pub(crate) fn read_watch(
    control: &Control,
    watch: &GiantsWatch,
    chained: Option<&GiantsChains>,
    pine: &mut PineClient,
    identity: &str,
    dir: &Path,
) -> Result<WatchRun> {
    let t = Instant::now();
    let mut ring = control.watch_read()?;
    let (pads, reads) = control.pad_readback(watch.port, &ring.flips())?;
    let read_ms = t.elapsed().as_secs_f64() * 1e3;
    let (chain, clips) = match chained {
        Some(c) => {
            let written = c.write_outputs(watch, &ring.records, pine, identity, dir)?;
            ring.records = written.o5;
            (Some(written.run), Some(written.clips))
        }
        None => (None, None),
    };
    let file = WatchFile {
        identity: identity.to_owned(),
        watch: watch.clone(),
        held: ring.summary.held,
        total: ring.summary.total,
        frames: ring.summary.frames,
        armed: ring.summary.armed,
        records: ring.records.iter().map(RawRecord::of).collect(),
        pads,
        reads,
    };
    file.write(&dir.join(WatchFile::FILE))?;
    // The places record by record, their behaviours named at this pause
    // (AG.176).
    if let Some(places) = &watch.places {
        let placed = PlacesFile::of(pine, places, &watch.list, &ring.records)?;
        let path = rrc_dump::output::refuse_repo_output(&dir.join(PlacesFile::FILE))?;
        fs::write(
            &path,
            serde_json::to_string(&placed).expect("the places serialise"),
        )
        .with_context(|| format!("writing {}", path.display()))?;
    }
    let recording = watch.recording(&ring.records, &file.pads)?;
    let frames_without_clips = recording
        .frames()
        .iter()
        .filter(|f| f.clips.is_empty())
        .count();
    // O6's clips where O5's list dropped them (AH.9).
    let (recording, clips_from_chain) = match &clips {
        Some(c) => crate::chains::fill_clips(recording, c)?,
        None => (recording, 0),
    };
    let path = rrc_dump::output::refuse_repo_output(&dir.join(RECORDING_FILE))?;
    fs::write(
        &path,
        serde_json::to_string(&recording).expect("a recording serialises"),
    )
    .with_context(|| format!("writing {}", path.display()))?;
    Ok(WatchRun {
        held: ring.summary.held,
        total: ring.summary.total,
        bytes: ring.summary.bytes,
        frames: ring.summary.frames,
        ranges: watch.list.ranges().len(),
        fields: watch.list.fields().len(),
        armed: ring.summary.armed,
        check: RingCheck::of(&recording, &ring.records, &file.reads),
        frames_without_clips,
        clips_from_chain,
        read_ms,
        chain,
    })
}

/// A set W replay's decoded recording, beside its `watch.json`.
pub const RECORDING_FILE: &str = "recording.json";

fn end_shot(control: &Control, dir: &Path) -> Result<EndShot> {
    let shot = control.screenshot(dir, STEP_WAIT)?;
    Ok(EndShot {
        file: file_name(&shot.path),
        flip: shot.flip,
        pixels_sha256: shot.frame.pixels_sha256.clone(),
        lit_share: shot.frame.lit_share(),
    })
}

fn checkpoint_shot(control: &Control, offset: u64, dir: &Path) -> Result<CheckpointShot> {
    let shot = end_shot(control, dir).with_context(|| format!("the shot at F0 + {offset}"))?;
    Ok(CheckpointShot { offset, shot })
}

/// Reads the actor frame and the RNG at the pause the run is in, writes both
/// beside the record and reduces them to an [`Observation`].
fn observe(pine: &mut PineClient, offset: u64, flip: u64, dir: &Path) -> Result<Observation> {
    let t = Instant::now();
    let mut frame = actors::read_frame(pine).with_context(|| format!("actors at {flip}"))?;
    let mut peek = Peek::read(pine).with_context(|| format!("the RNG at {flip}"))?;
    let read_ms = t.elapsed().as_secs_f64() * 1e3;
    frame.flip = Some(flip);
    peek.flip = Some(flip);
    frame.write(&dir.join(actors_file(offset)))?;
    peek.write(&dir.join(peek_file(offset)))?;
    Ok(observation(offset, flip, &frame, &peek, read_ms))
}

/// The record's view of one pause's reads.
pub(super) fn observation(
    offset: u64,
    flip: u64,
    frame: &ActorFrame,
    peek: &Peek,
    read_ms: f64,
) -> Observation {
    let origin = peek.origin(rng::ORIGIN_TWISTS);
    let mut state = Sha256::new();
    state.update(peek.mt.mti().to_be_bytes());
    for word in peek.mt.words() {
        state.update(word.to_be_bytes());
    }
    Observation {
        offset,
        flip,
        game_frame: frame.game_frame,
        frame_delta: frame.frame_delta,
        counts: frame.counts(),
        hero: frame.hero().ok().map(|a| Pose {
            position: a.position,
            rotation: a.rotation,
            velocity: Some(a.velocity),
        }),
        camera: frame.camera.as_ref().map(|c| Pose {
            position: c.position,
            rotation: c.rotation,
            velocity: None,
        }),
        rng: RngObservation {
            instances: peek.instances,
            mti: peek.mt.mti(),
            seed: origin.map(|o| o.seed),
            drawn: origin.map(|o| o.draws),
            anim_steps: peek.anim.steps_since_power_on(),
            fmod: peek.fmod,
            state_sha256: hex(&state.finalize()),
        },
        read_ms,
    }
}

pub(crate) struct SaveTries {
    pub(crate) saved: Option<Saved>,
    pub(crate) refusals: Vec<String>,
    pub(crate) error: Option<String>,
    /// `STATE`'s counter when the last `SAVESTATE` went out.
    pub(crate) asked_at: Option<u64>,
}

/// `SAVESTATE`, tried again [`SAVE_RETRY_FLIPS`] on when RPCS3 turns it down,
/// up to [`SAVE_TRIES`] times; after that, or on any other failure, the
/// replay keeps its observations and goes without an end state (unless the
/// state was written and only its reboot failed: the caller finds the copy).
pub(crate) fn save_retrying(control: &Control, dir: &Path) -> SaveTries {
    let mut refusals = Vec::new();
    let fail = |refusals, error: String, asked_at| SaveTries {
        saved: None,
        refusals,
        error: Some(error),
        asked_at,
    };
    for attempt in 1..=SAVE_TRIES {
        let asked_at = control.state().ok().map(|s| s.frames);
        match control.savestate(dir, STEP_WAIT) {
            Ok(saved) => {
                return SaveTries {
                    saved: Some(saved),
                    refusals,
                    error: None,
                    asked_at,
                };
            }
            Err(e) => {
                let text = format!("{e:#}");
                if !text.contains(SAVESTATE_REFUSED) {
                    return fail(refusals, text, asked_at);
                }
                refusals.push(text);
                if attempt == SAVE_TRIES {
                    return fail(
                        refusals,
                        format!("turned down {SAVE_TRIES} times: the replay has no end state"),
                        None,
                    );
                }
                let next = match control.state() {
                    Ok(s) => s.frames + SAVE_RETRY_FLIPS,
                    Err(e) => return fail(refusals, format!("{e:#}"), None),
                };
                if let Err(e) = control.pause_at(next, STEP_WAIT) {
                    return fail(
                        refusals,
                        format!("pausing at {next} to try again: {e:#}"),
                        None,
                    );
                }
            }
        }
    }
    unreachable!("the last try returns")
}

/// The state a save copied into a replay's dir, if one is there.
pub(crate) fn saved_in(dir: &Path) -> Option<std::path::PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().ends_with(Kind::SaveState.suffix()))
}

/// What a replay can leave in the oracle home for the next one: a savestate's
/// load puts back `dev_hdd0/game/<dir>` and hdd1's caches from the state
/// (`Emulator::Load`'s tars) but not the users' saves and trophies under
/// `dev_hdd0/home`, so a replay that saved or unlocked a trophy would boot the
/// next one into a different home. `divergence run` snapshots these before the
/// first replay and stops when one changes them.
pub fn carried_dirs(config_dir: &Path) -> Vec<std::path::PathBuf> {
    vec![config_dir.join("dev_hdd0").join("home")]
}

/// PINE's server starts with the game (and again after a savestate's reboot),
/// so its socket can trail the pause: retried until `within` is up.
pub fn pine_connect(socket: &Path, within: Duration) -> Result<PineClient> {
    let deadline = Instant::now() + within;
    loop {
        match PineClient::connect(socket) {
            Ok(c) => return Ok(c),
            Err(e) if Instant::now() > deadline => {
                return Err(e).with_context(|| format!("PINE at {}", socket.display()));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

impl Host {
    /// The load averages, `uptime` and the screen lock, now. A command that
    /// fails leaves its field empty rather than failing the replay.
    pub fn now() -> Self {
        let run = |cmd: &str, args: &[&str]| {
            Command::new(cmd)
                .args(args)
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        };
        let loadavg = run("sysctl", &["-n", "vm.loadavg"]).unwrap_or_default();
        let ioreg = run("ioreg", &["-n", "Root", "-d1"]);
        Host {
            load_average: parse_loadavg(&loadavg),
            uptime: run("uptime", &[]).unwrap_or_default(),
            screen_locked: ioreg.map(|t| t.contains("\"CGSSessionScreenIsLocked\"=Yes")),
        }
    }
}

/// `sysctl -n vm.loadavg`'s `{ 1.23 4.56 7.89 }`.
fn parse_loadavg(text: &str) -> Option<[f64; 3]> {
    let values: Vec<f64> = text
        .trim_matches(|c: char| c == '{' || c == '}' || c.is_whitespace())
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    values.try_into().ok()
}

/// The lines of `log` holding `needle`, trimmed, lossily decoded.
pub(crate) fn log_lines(log: &[u8], needle: &[u8]) -> Vec<String> {
    log.split(|&b| b == b'\n')
        .filter(|line| line.windows(needle.len()).any(|w| w == needle))
        .map(|line| String::from_utf8_lossy(line).trim().to_owned())
        .collect()
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(hex(&Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::{Actor, Camera};
    use crate::rng::AnimLcg;
    use guac_oracle::rng::Mt19937;

    fn actor(name: &str, skinned: bool, pad: Option<i32>) -> Actor {
        Actor {
            corpus: 0x3000_0000,
            placement: 0x3000_1000,
            name: Some(name.into()),
            position: [10.0, 43.0, -93.0],
            rotation: [0.0, 15.0, 0.0],
            scale: [150.0; 3],
            world: None,
            velocity: [1.5, 0.0, 6.0],
            skinned,
            pad,
        }
    }

    #[test]
    fn an_observation_reduces_the_frame_and_the_peek() {
        let mut mt = Mt19937::sgenrand(10554);
        mt.advance(116_862);
        let peek = Peek {
            flip: Some(62),
            meta: 0x3028_E734,
            instances: 1,
            instance: 0x3028_E7F0,
            refcount: 1,
            mt: mt.clone(),
            anim: AnimLcg {
                seed: 0x12345,
                current: guac_oracle::rng::Lcg32::NUMERICAL_RECIPES.step(0x12345),
            },
            fmod: Some(0),
        };
        let frame = ActorFrame {
            flip: Some(62),
            game_frame: 6_022,
            frame_delta: 0.0167,
            listed: 3,
            actors: vec![
                actor("009_Eruptor", true, Some(-1)),
                actor("sheep", true, None),
                actor("follow", false, Some(-1)),
            ],
            skipped: vec![],
            camera: Some(Camera {
                viewport: 0x3100_0000,
                name: Some("default".into()),
                position: [6.5, 52.3, -107.1],
                rotation: [-23.4, 15.0, 0.0],
            }),
        };
        let obs = observation(60, 62, &frame, &peek, 4.5);
        assert_eq!((obs.offset, obs.flip, obs.game_frame), (60, 62, 6_022));
        assert_eq!((obs.counts.listed, obs.counts.skinned), (3, 2));
        let hero = obs.hero.as_ref().unwrap();
        assert_eq!(hero.position, [10.0, 43.0, -93.0]);
        assert_eq!(hero.velocity, Some([1.5, 0.0, 6.0]));
        assert_eq!(obs.camera.as_ref().unwrap().rotation, [-23.4, 15.0, 0.0]);
        assert_eq!(
            (obs.rng.seed, obs.rng.drawn, obs.rng.anim_steps),
            (Some(10554), Some(116_862), Some(1))
        );
        // Equal states hash equal, a draw apart they don't.
        let again = observation(60, 62, &frame, &peek, 9.0);
        assert_eq!(again.rng, obs.rng);
        let mut moved = peek.clone();
        moved.mt.advance(1);
        let next = observation(60, 62, &frame, &moved, 4.5);
        assert_ne!(next.rng.state_sha256, obs.rng.state_sha256);
        assert_eq!(next.rng.drawn, Some(116_863));
        // Two pad-driven skinned actors: no single hero, read as none.
        let mut two = frame.clone();
        two.actors[1].pad = Some(0);
        assert!(observation(0, 2, &two, &peek, 1.0).hero.is_none());
    }

    #[test]
    fn the_log_is_searched_as_bytes() {
        let mut log =
            b"cellAudio: cellAudio initializing. Backend: Null\n[0094639c] \xa9 sc\n".to_vec();
        log.extend_from_slice(
            b"F {PPU} SIG: Thread terminated due to fatal error: Verification failed\n",
        );
        assert!(std::str::from_utf8(&log).is_err());
        assert_eq!(log_lines(&log, SILENT).len(), 1);
        assert_eq!(
            log_lines(&log, FATAL),
            ["F {PPU} SIG: Thread terminated due to fatal error: Verification failed"]
        );
        assert!(log_lines(b"Backend: Cubeb\n", SILENT).is_empty());
    }

    #[test]
    fn a_save_or_a_trophy_in_the_home_shows_and_a_restored_game_dir_doesnt() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path();
        let trophy = config.join("dev_hdd0/home/00000001/trophy/NPWR00000_00");
        fs::create_dir_all(&trophy).unwrap();
        fs::write(trophy.join("TROPUSR.DAT"), b"a").unwrap();
        fs::create_dir_all(config.join("dev_hdd0/game/BLUS30968")).unwrap();
        let dirs = carried_dirs(config);
        let before = crate::snapshot::Snapshot::take(&dirs).unwrap();
        // The state's load rewrites the game dir: not carried.
        fs::write(config.join("dev_hdd0/game/BLUS30968/data"), b"b").unwrap();
        assert!(
            before
                .diff(&crate::snapshot::Snapshot::take(&dirs).unwrap())
                .is_empty()
        );
        // A save lands beside the trophies: carried.
        let save = config.join("dev_hdd0/home/00000001/savedata/BLUS30968");
        fs::create_dir_all(&save).unwrap();
        fs::write(save.join("E_SLOT_1"), b"c").unwrap();
        let changed = before.diff(&crate::snapshot::Snapshot::take(&dirs).unwrap());
        assert!(
            changed.iter().any(|c| c.ends_with("E_SLOT_1")),
            "{changed:?}"
        );
    }

    #[test]
    fn the_load_averages_parse_from_sysctl() {
        assert_eq!(
            parse_loadavg("{ 2.37 9.45 13.15 }"),
            Some([2.37, 9.45, 13.15])
        );
        assert_eq!(parse_loadavg(""), None);
        assert_eq!(parse_loadavg("{ 1 2 }"), None);
        assert_eq!(parse_loadavg("{ a b c }"), None);
    }
}
