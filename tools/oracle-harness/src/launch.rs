//! The launch wrapper (openspec AF, design decision 5): RPCS3 no-GUI with
//! `HOME` set to the oracle home, `TMPDIR` set to a fresh per-run dir outside it
//! (RPCS3 refuses a config dir under its temp dir, and PINE puts its socket
//! there), a per-run P1 socket and `GUAC_ORACLE=1`, the gate every O-series verb
//! checks. It refuses to start while an RPCS3 runs from the same home (its
//! `RPCS3.buf` lock), holds the GDB stub's port or answers on either socket, and
//! re-checks the home's isolation first. The process itself is
//! `skylander-rpcs3-control`'s [`RpcsProcess`], so shutdown and socket cleanup
//! are the product's.
//!
//! The child inherits the harness's environment minus the keys [`spec`] sets
//! or removes. What else it inherits that can change a frame (MoltenVK, Vulkan,
//! Metal, Qt, dyld, the patches' own `SKYLANDER_*`) is listed by
//! [`inherited_render_env`] and printed with the run, rather than cleared: an
//! allowlist would also drop a Vulkan ICD a dev's setup points at.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use skylander_rpcs3_control::ipc::IpcPortalDriver;
use skylander_rpcs3_control::ipc::proto::EmuState;
use skylander_rpcs3_control::{LaunchSpec, RpcsProcess, ShutdownPath};

use crate::control::Control;
use crate::home::{self, OracleHome};

/// RPCS3's GDB stub at its default address, on in every build: a listener here
/// means another RPCS3 is running (the dev's own, say), and two would share
/// the Mac's cores and the GPU.
pub const GDB_STUB: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 2345);
/// The oracle's own GDB stub ([`crate::template::ORACLE_GDB_PORT`]): a listener
/// here means another oracle is running, from another home.
pub const ORACLE_GDB_STUB: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
    crate::template::ORACLE_GDB_PORT,
);
/// PINE's socket name under `$TMPDIR` on its default slot.
pub const PINE_SOCKET: &str = "rpcs3.sock";
/// The longest path an AF_UNIX socket takes on macOS (`sun_path` is 104 bytes
/// with its NUL).
const MAX_SOCKET_PATH: usize = 103;
/// Inherited keys under these prefixes can change what RPCS3 renders.
const RENDER_ENV_PREFIXES: [&str; 8] = [
    "MVK_",
    "VK_",
    "MTL_",
    "METAL_",
    "DYLD_",
    "QT_",
    "SKYLANDER_",
    "RPCS3_",
];

static RUN_COUNTER: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Default)]
pub struct BootOptions {
    /// Boot this state (`--savestate`, a copy taken into the run dir first)
    /// instead of Giants' EBOOT. With the template's `Start Paused` it stops 2
    /// flips in.
    pub savestate: Option<PathBuf>,
    /// P8's surface embed (`SKYLANDER_BORDERLESS=1`): rendering goes to a
    /// published CAContext and nothing appears on screen.
    pub borderless: bool,
    /// P8's 2560x1440 borderless surface (`SKYLANDER_SURFACE_2X=1`) instead of
    /// 1280x720. P8 checks only that the key is set, so off removes it.
    pub surface_2x: bool,
    /// Boots without `GUAC_ORACLE=1`, so every O-verb answers `ERR unknown_cmd`
    /// and P1 speaks exactly the product protocol (AF.5's gate check).
    pub ungated: bool,
}

/// The per-run paths: the dir RPCS3 gets as `TMPDIR` and the two sockets in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPaths {
    pub tmp: PathBuf,
    pub p1_socket: PathBuf,
    pub pine_socket: PathBuf,
}

impl RunPaths {
    /// A fresh dir under `base` (the harness's own temp dir unless a test says
    /// otherwise), short enough that both sockets fit `sun_path`. `base` goes
    /// through the work-tree refusal (a savestate is copied in here) and comes
    /// out physical, so the run dir is spelled the way the home is.
    pub fn create(base: &Path) -> Result<Self> {
        let base = rrc_dump::output::refuse_repo_output(base)?;
        let n = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = base.join(format!("gor-{}-{n}", std::process::id()));
        if tmp.exists() {
            std::fs::remove_dir_all(&tmp)?;
        }
        std::fs::create_dir_all(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        Self::in_dir(tmp)
    }

    /// The paths inside `tmp`, which the caller has created; only the socket
    /// lengths are checked.
    pub fn in_dir(tmp: PathBuf) -> Result<Self> {
        let paths = Self {
            p1_socket: tmp.join("p1.sock"),
            pine_socket: tmp.join(PINE_SOCKET),
            tmp,
        };
        for sock in [&paths.p1_socket, &paths.pine_socket] {
            if sock.as_os_str().len() > MAX_SOCKET_PATH {
                bail!(
                    "socket path {} is longer than AF_UNIX takes ({MAX_SOCKET_PATH} bytes); use a shorter temp dir",
                    sock.display()
                );
            }
        }
        Ok(paths)
    }
}

/// The argv and env for one boot. Pure apart from resolving the run dir's
/// physical path, which the apart check compares with the home's.
pub fn spec(
    exe: &Path,
    home: &OracleHome,
    run: &RunPaths,
    opts: &BootOptions,
) -> Result<LaunchSpec> {
    let root = home.root();
    let physical = run.tmp.canonicalize().unwrap_or_else(|_| run.tmp.clone());
    if physical.starts_with(root) || root.starts_with(&physical) {
        bail!(
            "the run's TMPDIR {} and the oracle home {} must sit apart (RPCS3 refuses a config dir under its temp dir)",
            run.tmp.display(),
            root.display()
        );
    }
    let mut args: Vec<std::ffi::OsString> = vec!["--no-gui".into()];
    match &opts.savestate {
        Some(state) => {
            args.push("--savestate".into());
            args.push(state.into());
        }
        None => args.push(home.giants_eboot().into()),
    }
    let mut tmpdir = run.tmp.clone().into_os_string();
    tmpdir.push("/");
    let set = |k: &str, v: std::ffi::OsString| (k.into(), Some(v));
    let unset = |k: &str| (k.into(), None);
    let mut env = vec![
        set("HOME", root.into()),
        set("TMPDIR", tmpdir),
        if opts.ungated {
            unset("GUAC_ORACLE")
        } else {
            set("GUAC_ORACLE", "1".into())
        },
        // Each would point RPCS3 somewhere other than the oracle home.
        unset("RPCS3_CONFIG_DIR"),
        unset("XDG_CONFIG_HOME"),
        unset("XDG_CACHE_HOME"),
    ];
    env.push(if opts.borderless {
        set("SKYLANDER_BORDERLESS", "1".into())
    } else {
        unset("SKYLANDER_BORDERLESS")
    });
    env.push(if opts.surface_2x {
        set("SKYLANDER_SURFACE_2X", "1".into())
    } else {
        unset("SKYLANDER_SURFACE_2X")
    });
    if cfg!(all(unix, not(target_os = "macos"))) {
        // PINE takes XDG_RUNTIME_DIR, not TMPDIR, off the Mac.
        env.push(set("XDG_RUNTIME_DIR", run.tmp.clone().into()));
    }
    Ok(LaunchSpec {
        exe: exe.to_path_buf(),
        args,
        ipc_path: run.p1_socket.clone(),
        env,
    })
}

/// What the child inherits under [`RENDER_ENV_PREFIXES`] from `vars` (the
/// harness's own environment), less every key `spec` sets or removes; sorted.
pub fn inherited_render_env(
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
    spec: &LaunchSpec,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vars
        .into_iter()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .filter(|(k, _)| RENDER_ENV_PREFIXES.iter().any(|p| k.starts_with(p)))
        // launch_spec sets this one itself, from `ipc_path`.
        .filter(|(k, _)| k != "SKYLANDER_IPC_PATH")
        .filter(|(k, _)| !spec.env.iter().any(|(s, _)| s.to_str() == Some(k)))
        .collect();
    out.sort();
    out
}

/// Refuses to start while an RPCS3 runs from `home` (it holds the home's
/// `RPCS3.buf`), something holds one of the `gdb` ports or either socket of
/// `run` is live; a socket file nobody answers on is a dead run's, and goes.
/// The lock is what stops a second oracle on one home: [`boot`]'s run dir is
/// always new, so its sockets are only ever live for a caller that made its own
/// [`RunPaths`].
pub fn preflight(home: &OracleHome, run: &RunPaths, gdb: &[SocketAddr]) -> Result<()> {
    home::refuse_running(home.root())?;
    if let Some(held) = gdb.iter().find(|a| port_held(**a)) {
        bail!("something holds {held}, an RPCS3 GDB stub: another RPCS3 is running; stop it first");
    }
    for sock in [&run.p1_socket, &run.pine_socket] {
        if socket_live(sock) {
            bail!(
                "{} is live: another RPCS3 is using it; stop it first",
                sock.display()
            );
        }
        if sock.exists() {
            std::fs::remove_file(sock)?;
        }
    }
    Ok(())
}

/// Whether something listens on `addr`, found by binding it, never by
/// connecting: RPCS3's GDB stub pauses the emulator on every connection and
/// freezes it when the peer hangs up without a word (AF.5's live run lost a
/// boot to another work tree's preflight that connected).
fn port_held(addr: SocketAddr) -> bool {
    matches!(
        std::net::TcpListener::bind(addr),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse
    )
}

#[cfg(unix)]
fn socket_live(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

#[cfg(not(unix))]
fn socket_live(_path: &Path) -> bool {
    false
}

/// A running oracle. Dropping it shuts RPCS3 down, so a failed test or a `?`
/// doesn't leave an emulator behind.
pub struct Oracle {
    process: Option<RpcsProcess>,
    run: RunPaths,
    driver: IpcPortalDriver,
    render_env: Vec<(String, String)>,
    config_dir: PathBuf,
}

/// Boots the oracle at `exe` from `home` after the preflight, in a fresh run
/// dir that [`Oracle::shutdown`] (or a drop) removes again, as does a boot that
/// fails. The home's isolation is checked again against the harness's own
/// `HOME` first.
pub fn boot(exe: &Path, home: &OracleHome, opts: &BootOptions) -> Result<Oracle> {
    if cfg!(windows) {
        bail!("the oracle is Mac only until the Windows/HTPC bring-up");
    }
    if !exe.is_file() {
        bail!("no oracle binary at {}", exe.display());
    }
    home.check_isolated(&home::real_home()?)?;
    let run = RunPaths::create(&std::env::temp_dir())?;
    let (process, render_env) = match start(exe, home, &run, opts) {
        Ok(started) => started,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&run.tmp);
            return Err(e);
        }
    };
    let driver = IpcPortalDriver::with_path(&run.p1_socket).with_timeout(Duration::from_secs(5));
    Ok(Oracle {
        process: Some(process),
        run,
        driver,
        render_env,
        config_dir: home.config_dir(),
    })
}

/// The preflight, the savestate's copy into `run` and the spawn.
fn start(
    exe: &Path,
    home: &OracleHome,
    run: &RunPaths,
    opts: &BootOptions,
) -> Result<(RpcsProcess, Vec<(String, String)>)> {
    preflight(home, run, &[GDB_STUB, ORACLE_GDB_STUB])?;
    let mut opts = opts.clone();
    if let Some(state) = &opts.savestate {
        // RPCS3 only reads a state it boots, but the source may be the dev's own.
        let name = state
            .file_name()
            .context("savestate path has no file name")?;
        let copy = run.tmp.join(name);
        std::fs::copy(state, &copy).with_context(|| format!("copying {}", state.display()))?;
        opts.savestate = Some(copy);
    }
    let spec = spec(exe, home, run, &opts)?;
    let render_env = inherited_render_env(std::env::vars_os(), &spec);
    Ok((RpcsProcess::launch_spec(&spec)?, render_env))
}

/// Frames counted over a window, for the fps a run sustains.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FpsSample {
    pub frames: u64,
    pub seconds: f64,
}

impl FpsSample {
    pub fn fps(&self) -> f64 {
        self.frames as f64 / self.seconds.max(1e-3)
    }
}

impl Oracle {
    pub fn run_paths(&self) -> &RunPaths {
        &self.run
    }

    /// P1's driver on this run's socket (`STATE`, `LOAD`, `RECONNECT`, ...).
    pub fn driver(&self) -> &IpcPortalDriver {
        &self.driver
    }

    /// The O-verbs on this run's socket, with the home's RPCS3 config dir to
    /// find what they make RPCS3 write.
    pub fn control(&self) -> Control {
        Control::new(&self.run.p1_socket, &self.config_dir)
    }

    pub fn pid(&self) -> u32 {
        self.process.as_ref().map_or(0, RpcsProcess::pid)
    }

    /// What RPCS3 inherited that can change a frame ([`inherited_render_env`]),
    /// for the run's record next to the identity line.
    pub fn render_env(&self) -> &[(String, String)] {
        &self.render_env
    }

    fn alive(&mut self) -> bool {
        self.process.as_mut().is_some_and(RpcsProcess::is_alive)
    }

    /// Polls `STATE` until `done` holds, failing fast when RPCS3 exits or reports
    /// `frozen` or `stopped`.
    pub fn wait_for(
        &mut self,
        timeout: Duration,
        done: impl Fn(&EmuState) -> bool,
    ) -> Result<EmuState> {
        let deadline = Instant::now() + timeout;
        let mut last = None;
        while Instant::now() < deadline {
            if !self.alive() {
                bail!("RPCS3 exited (last state {last:?})");
            }
            if let Ok(state) = self.driver.read_state() {
                if state.status == "frozen" || state.status == "stopped" {
                    bail!("RPCS3 reports {} at frame {}", state.status, state.frames);
                }
                if done(&state) {
                    return Ok(state);
                }
                last = Some(state);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        bail!("timed out after {timeout:?} (last state {last:?})")
    }

    /// Running, rendering and done compiling: a cold boot's ready signal.
    pub fn wait_playable(&mut self, timeout: Duration) -> Result<EmuState> {
        self.wait_for(timeout, EmuState::is_playable)
    }

    /// Paused with at least one flip behind it: where `Start Paused` leaves a
    /// savestate boot.
    pub fn wait_paused(&mut self, timeout: Duration) -> Result<EmuState> {
        self.wait_for(timeout, |s| s.status == "paused" && s.frames > 0)
    }

    /// Counts flips over `window`, failing on a stall (no flip at all).
    pub fn sample_fps(&mut self, window: Duration) -> Result<FpsSample> {
        let first = self.wait_for(Duration::from_secs(5), |_| true)?;
        let start = Instant::now();
        std::thread::sleep(window);
        let last = self.wait_for(Duration::from_secs(5), |_| true)?;
        let sample = FpsSample {
            frames: last.frames.saturating_sub(first.frames),
            seconds: start.elapsed().as_secs_f64(),
        };
        if sample.frames == 0 {
            bail!(
                "no flip in {window:?}: RPCS3 is {} at frame {}",
                last.status,
                last.frames
            );
        }
        Ok(sample)
    }

    /// SIGTERM, SIGKILL after 10 s, then the run dir goes.
    pub fn shutdown(mut self) -> Result<ShutdownPath> {
        self.stop()
    }

    fn stop(&mut self) -> Result<ShutdownPath> {
        let outcome = match self.process.take() {
            Some(mut p) => p.shutdown_graceful(Duration::from_secs(10))?,
            None => ShutdownPath::AlreadyExited,
        };
        let _ = std::fs::remove_dir_all(&self.run.tmp);
        Ok(outcome)
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::home::{InitOptions, init};
    use crate::template::ConfigTemplate;
    use std::ffi::OsString;

    fn home(tmp: &Path) -> OracleHome {
        let real = crate::home::rpcs3_config_dir(&tmp.join("real-home"));
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

    fn env_of(spec: &LaunchSpec, key: &str) -> Option<Option<OsString>> {
        spec.env
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    /// A base dir with a short name, so the sockets under it fit `sun_path`.
    fn short_base() -> tempfile::TempDir {
        tempfile::Builder::new().prefix("gor").tempdir().unwrap()
    }

    #[test]
    fn a_cold_boot_runs_in_the_oracle_home_with_the_gate_on() {
        let tmp = tempfile::tempdir().unwrap();
        let home = home(tmp.path());
        let base = short_base();
        let run = RunPaths::create(base.path()).unwrap();
        let spec = spec(Path::new("/x/rpcs3"), &home, &run, &BootOptions::default()).unwrap();
        assert_eq!(
            spec.args,
            [OsString::from("--no-gui"), home.giants_eboot().into()]
        );
        assert_eq!(spec.ipc_path, run.p1_socket);
        assert_eq!(env_of(&spec, "HOME"), Some(Some(home.root().into())));
        let mut tmpdir = run.tmp.clone().into_os_string();
        tmpdir.push("/");
        assert_eq!(env_of(&spec, "TMPDIR"), Some(Some(tmpdir)));
        assert_eq!(env_of(&spec, "GUAC_ORACLE"), Some(Some("1".into())));
        assert_eq!(env_of(&spec, "SKYLANDER_BORDERLESS"), Some(None));
        assert_eq!(env_of(&spec, "SKYLANDER_SURFACE_2X"), Some(None));
        assert_eq!(env_of(&spec, "RPCS3_CONFIG_DIR"), Some(None));
        assert_eq!(run.pine_socket, run.tmp.join("rpcs3.sock"));
    }

    #[test]
    fn a_savestate_boot_names_the_state_and_no_eboot() {
        let tmp = tempfile::tempdir().unwrap();
        let home = home(tmp.path());
        let base = short_base();
        let run = RunPaths::create(base.path()).unwrap();
        let opts = BootOptions {
            savestate: Some(PathBuf::from("/s/BLUS30968_1_0.SAVESTAT.zst")),
            borderless: true,
            surface_2x: true,
            ungated: false,
        };
        let spec = spec(Path::new("/x/rpcs3"), &home, &run, &opts).unwrap();
        assert_eq!(
            spec.args,
            ["--no-gui", "--savestate", "/s/BLUS30968_1_0.SAVESTAT.zst"].map(OsString::from)
        );
        assert_eq!(
            env_of(&spec, "SKYLANDER_BORDERLESS"),
            Some(Some("1".into()))
        );
        assert_eq!(
            env_of(&spec, "SKYLANDER_SURFACE_2X"),
            Some(Some("1".into()))
        );
    }

    #[test]
    fn an_ungated_boot_removes_the_gate_rather_than_set_it() {
        let tmp = tempfile::tempdir().unwrap();
        let home = home(tmp.path());
        let base = short_base();
        let run = RunPaths::create(base.path()).unwrap();
        let opts = BootOptions {
            ungated: true,
            ..BootOptions::default()
        };
        let spec = spec(Path::new("/x/rpcs3"), &home, &run, &opts).unwrap();
        // Removed, so a GUAC_ORACLE=1 in the harness's own env can't leak through.
        assert_eq!(env_of(&spec, "GUAC_ORACLE"), Some(None));
        assert_eq!(env_of(&spec, "HOME"), Some(Some(home.root().into())));
    }

    #[test]
    fn what_can_change_a_frame_is_reported_unless_the_spec_sets_it() {
        let tmp = tempfile::tempdir().unwrap();
        let home = home(tmp.path());
        let base = short_base();
        let run = RunPaths::create(base.path()).unwrap();
        let spec = spec(Path::new("/x/rpcs3"), &home, &run, &BootOptions::default()).unwrap();
        let vars = [
            ("PATH", "/bin"),
            ("HOME", "/Users/dev"),
            ("MVK_CONFIG_LOG_LEVEL", "2"),
            ("VK_ICD_FILENAMES", "/opt/mvk.json"),
            ("QT_QPA_PLATFORM", "offscreen"),
            ("SKYLANDER_PORTAL_DRIVER", "mock"),
            // Set or removed by the spec, or by launch_spec itself.
            ("SKYLANDER_SURFACE_2X", "1"),
            ("SKYLANDER_BORDERLESS", "1"),
            ("SKYLANDER_IPC_PATH", "/elsewhere.sock"),
            ("RPCS3_CONFIG_DIR", "/Users/dev/rpcs3"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let got = inherited_render_env(vars, &spec);
        let want = [
            ("MVK_CONFIG_LOG_LEVEL", "2"),
            ("QT_QPA_PLATFORM", "offscreen"),
            ("SKYLANDER_PORTAL_DRIVER", "mock"),
            ("VK_ICD_FILENAMES", "/opt/mvk.json"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()));
        assert_eq!(got, want);
    }

    #[test]
    fn the_run_dir_may_not_sit_inside_the_home() {
        let tmp = tempfile::tempdir().unwrap();
        let home = home(tmp.path());
        let run = RunPaths::in_dir(home.root().join("t")).unwrap();
        let err = spec(Path::new("/x/rpcs3"), &home, &run, &BootOptions::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("must sit apart"), "{err}");
    }

    #[test]
    fn the_apart_check_sees_through_a_link_to_the_home() {
        let tmp = tempfile::tempdir().unwrap();
        let home = home(tmp.path());
        let alias = tmp.path().join("a");
        std::os::unix::fs::symlink(home.root(), &alias).unwrap();
        std::fs::create_dir(alias.join("t")).unwrap();
        let run = RunPaths::in_dir(alias.join("t")).unwrap();
        let err = spec(Path::new("/x/rpcs3"), &home, &run, &BootOptions::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("must sit apart"), "{err}");
    }

    #[test]
    fn a_run_dir_is_physical_and_never_in_a_work_tree() {
        let base = short_base();
        let alias = base.path().join("l");
        std::fs::create_dir(base.path().join("d")).unwrap();
        std::os::unix::fs::symlink(base.path().join("d"), &alias).unwrap();
        let run = RunPaths::create(&alias).unwrap();
        let physical = base.path().join("d").canonicalize().unwrap();
        assert_eq!(run.tmp.parent(), Some(physical.as_path()));
        assert!(run.p1_socket.starts_with(&physical));

        let inside = Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-run-base");
        let err = RunPaths::create(&inside).unwrap_err().to_string();
        assert!(
            err.contains("refusing to write capture data inside"),
            "{err}"
        );
        assert!(!inside.exists());
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
        let held = std::fs::File::create(crate::home::rpcs3_lock_file(home.root())).unwrap();
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

    #[test]
    fn a_socket_path_too_long_for_af_unix_is_refused() {
        let long = PathBuf::from("/tmp").join("d".repeat(100));
        let err = RunPaths::in_dir(long).unwrap_err().to_string();
        assert!(err.contains("longer than AF_UNIX takes"), "{err}");
    }

    #[test]
    fn a_running_rpcs3_a_live_socket_or_the_gdb_port_blocks_a_launch() {
        // A port nobody listens on stands in for a free GDB stub.
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let free_addr = free.local_addr().unwrap();
        drop(free);

        let tmp = tempfile::tempdir().unwrap();
        let home = home(tmp.path());
        let base = short_base();
        let run = RunPaths::create(base.path()).unwrap();
        preflight(&home, &run, &[free_addr]).unwrap();

        // An RPCS3 running from the home holds RPCS3.buf's flock all its life.
        let held = std::fs::File::create(crate::home::rpcs3_lock_file(home.root())).unwrap();
        held.lock().unwrap();
        let err = preflight(&home, &run, &[free_addr])
            .unwrap_err()
            .to_string();
        assert!(err.contains("an RPCS3 is running"), "{err}");
        drop(held);
        preflight(&home, &run, &[free_addr]).unwrap();

        // A stale socket file nobody answers on is cleared, not refused.
        let stale = std::os::unix::net::UnixListener::bind(&run.p1_socket).unwrap();
        drop(stale);
        assert!(run.p1_socket.exists());
        preflight(&home, &run, &[free_addr]).unwrap();
        assert!(!run.p1_socket.exists());

        for sock in [&run.p1_socket, &run.pine_socket] {
            let live = std::os::unix::net::UnixListener::bind(sock).unwrap();
            let err = preflight(&home, &run, &[free_addr])
                .unwrap_err()
                .to_string();
            assert!(err.contains("is live"), "{err}");
            drop(live);
            std::fs::remove_file(sock).unwrap();
        }

        // A held GDB port refuses, found without connecting: a connection
        // would pause (or freeze) the RPCS3 behind it.
        let gdb = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        gdb.set_nonblocking(true).unwrap();
        let err = preflight(&home, &run, &[free_addr, gdb.local_addr().unwrap()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("GDB stub"), "{err}");
        assert_eq!(
            gdb.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "the probe connected"
        );
    }
}
