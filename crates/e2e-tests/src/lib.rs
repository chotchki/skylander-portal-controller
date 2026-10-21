//! Helpers shared across the e2e regression tests.
//!
//! [`TestServer::spawn`] builds and launches the server with
//! `--features test-hooks`, scrapes the "serving on http://…" line from its
//! stdout to discover the URL, and hands back an owning handle whose `Drop`
//! kills the child. [`Phone`] wraps a fantoccini WebDriver client with
//! convenience selectors.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use fantoccini::{Client, ClientBuilder, Locator};
use tempfile::TempDir;

/// A running server instance plus its owned chromedriver. Both children are
/// killed when this drops.
pub struct TestServer {
    pub url: String,
    pub chromedriver_url: String,
    _child: ChildGuard,
    _chromedriver: ChildGuard,
    _tmpdir: TempDir,
}

/// Which binary `TestServer::spawn*` runs: `E2E_APP=product` (the default)
/// or `E2E_APP=guacamole` (AA design decision 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum E2eApp {
    Product,
    Guacamole,
}

impl E2eApp {
    /// Panics on anything but `product` or `guacamole`, so a typo can't
    /// quietly test the wrong binary.
    pub fn from_env() -> Self {
        match std::env::var("E2E_APP").as_deref() {
            Err(_) | Ok("") | Ok("product") => Self::Product,
            Ok("guacamole") => Self::Guacamole,
            Ok(other) => panic!("E2E_APP={other:?}: expected `product` or `guacamole`"),
        }
    }

    /// Games the phone's picker offers: the mock's six, or Giants alone.
    pub fn game_count(self) -> usize {
        match self {
            Self::Product => 6,
            Self::Guacamole => 1,
        }
    }
}

/// True (and says why on stderr) when a product-only test should skip
/// because `E2E_APP=guacamole`.
pub fn product_only(why: &str) -> bool {
    let skip = E2eApp::from_env() == E2eApp::Guacamole;
    if skip {
        eprintln!("SKIPPED under E2E_APP=guacamole, product-only: {why}");
    }
    skip
}

/// True (and says so) when a guacamole-only test should skip.
pub fn guacamole_only() -> bool {
    let skip = E2eApp::from_env() != E2eApp::Guacamole;
    if skip {
        eprintln!("SKIPPED: guacamole-only, run with E2E_APP=guacamole");
    }
    skip
}

/// The disc serial guacamole offers.
pub const GIANTS_SERIAL: &str = "BLUS30968";

impl TestServer {
    pub fn spawn() -> Result<Self> {
        Self::spawn_with_env_lines("")
    }

    /// Like [`spawn`] but appends `extra_env` to the generated `.env.dev`
    /// (newline-terminated lines, e.g. `"WINDOW_MODE=desktop\n"`). Used by
    /// the play-through recorder (PLAN 15.11), which needs the launcher in
    /// Phase-20 windowed Desktop mode. Runs the binary `E2E_APP` names.
    pub fn spawn_with_env_lines(extra_env: &str) -> Result<Self> {
        match E2eApp::from_env() {
            E2eApp::Product => Self::spawn_product(extra_env),
            E2eApp::Guacamole => Self::spawn_guacamole(&[GIANTS_SERIAL], extra_env),
        }
    }

    fn spawn_product(extra_env: &str) -> Result<Self> {
        let repo = repo_root()?;
        let firmware = resolve_firmware_pack(&repo)?;
        let phone_dist = built_phone_dist(&repo)?;

        let tmp = tempfile::tempdir().context("create temp dir")?;
        link_repo_images(tmp.path(), &repo);
        let port = pick_free_port()?;
        let mut env = format!(
            "RPCS3_EXE={rpcs3}\nFIRMWARE_PACK_ROOT={pack}\nBIND_PORT={port}\nSKYLANDER_PORTAL_DRIVER=mock\nPHONE_DIST={phone}\n",
            rpcs3 = repo.join("crates/e2e-tests/src/lib.rs").display(), // any real file — mock doesn't launch
            pack = firmware.display(),
            port = port,
            phone = phone_dist.display(),
        );
        env.push_str(extra_env);
        std::fs::write(tmp.path().join(".env.dev"), env)?;
        Self::run(tmp, &repo, "skylander-server")
    }

    /// guacamole with no firmware pack or RPCS3: `GUAC_DUMP_DIRS` points at
    /// one synthetic dump per title id, each a `PS3_GAME/PARAM.SFO` built in
    /// code. Giants' is an [`IntroFixture`] with no intros, so its roster
    /// ([`E2E_ROSTER`]) is current at start (AE.4). Its window opens
    /// desktop-sized unless `extra_env` says otherwise.
    pub fn spawn_guacamole(title_ids: &[&str], extra_env: &str) -> Result<Self> {
        let tmp = tempfile::tempdir().context("create temp dir")?;
        let mut env = String::new();
        let dumps = if title_ids.contains(&GIANTS_SERIAL) {
            let fixture = IntroFixture::build_in(&tmp.path().join("giants"), &[])?;
            env = fixture.env();
            fixture.dumps.clone()
        } else {
            tmp.path().join("dumps")
        };
        for title_id in title_ids.iter().filter(|t| **t != GIANTS_SERIAL) {
            let game = dumps.join(title_id).join("PS3_GAME");
            std::fs::create_dir_all(&game)?;
            let sfo = guac_formats::sfo::builder::SfoBuilder::disc_game(title_id, "01.00", "01.00")
                .build();
            std::fs::write(game.join("PARAM.SFO"), sfo)?;
        }
        std::fs::create_dir_all(&dumps)?;
        env.push_str(extra_env);
        Self::spawn_guacamole_over(tmp, &dumps, &env)
    }

    /// guacamole over a Giants dump whose [`E2E_WALKER`] walks the level
    /// (openspec AG.21), no intros.
    pub fn spawn_guacamole_walking() -> Result<Self> {
        let tmp = tempfile::tempdir().context("create temp dir")?;
        let fixture = IntroFixture::build_walking_in(&tmp.path().join("giants"), &[])?;
        let (dumps, env) = (fixture.dumps.clone(), fixture.env());
        Self::spawn_guacamole_over(tmp, &dumps, &env)
    }

    /// guacamole over `fixture`'s Giants dump and finished extract, the dump
    /// trusted through `GUAC_TEST_KNOWN_DUMP` (openspec AB.11).
    pub fn spawn_guacamole_with_intros(fixture: &IntroFixture) -> Result<Self> {
        let tmp = tempfile::tempdir().context("create temp dir")?;
        Self::spawn_guacamole_over(tmp, &fixture.dumps, &fixture.env())
    }

    fn spawn_guacamole_over(tmp: TempDir, dumps: &Path, extra_env: &str) -> Result<Self> {
        let repo = repo_root()?;
        let phone_dist = built_phone_dist(&repo)?;
        let port = pick_free_port()?;
        // No DATA_ROOT: guacamole reads no wiki data (AE.4).
        let mut env = format!(
            "BIND_PORT={port}\nPHONE_DIST={phone}\nGUAC_DUMP_DIRS={dumps}\nWINDOW_MODE=desktop\n",
            phone = phone_dist.display(),
            dumps = dumps.display(),
        );
        env.push_str(extra_env);
        std::fs::write(tmp.path().join(".env.dev"), env)?;
        Self::run(tmp, &repo, "guacamole")
    }

    /// `cargo run -p <package> --features test-hooks` in `tmp`, then a
    /// chromedriver beside it.
    fn run(tmp: TempDir, repo: &Path, package: &str) -> Result<Self> {
        let (url, guard) = start_server(tmp.path(), repo, package)?;
        let (chromedriver_url, chromedriver_guard) = spawn_chromedriver()?;

        Ok(Self {
            url,
            chromedriver_url,
            _child: guard,
            _chromedriver: chromedriver_guard,
            _tmpdir: tmp,
        })
    }

    /// Kills guacamole and starts it again in the same temp dir, so over the
    /// same `.env.dev`, port, database and extract: what a kid sees after
    /// closing it and opening it again (openspec AH.3). Phones connected
    /// before see the connection drop; chromedriver stays up.
    pub fn restart_guacamole(&mut self) -> Result<()> {
        self._child.kill_now();
        let repo = repo_root()?;
        let (url, guard) = start_server(self._tmpdir.path(), &repo, "guacamole")?;
        self.url = url;
        self._child = guard;
        Ok(())
    }

    /// URL to navigate the phone to. Bakes the HMAC key into a `#k=<hex>`
    /// fragment so the phone actually exercises the signed-request path
    /// end-to-end rather than falling back to the server's dev bypass.
    /// Fetched via the `/api/_test/hmac_key` hook; every `Phone::new` goes
    /// through this rather than the raw `url`.
    pub async fn phone_url(&self) -> anyhow::Result<String> {
        let hex = fetch_hmac_key_hex(&self.url).await?;
        Ok(format!("{}/#k={}", self.url, hex))
    }

    /// Path to this server's per-run `dev-data/` root. Lives inside the
    /// harness's temp dir (killed on drop), so tests that want to inspect
    /// working-copy files, DB state, etc. must do so before `TestServer`
    /// drops. Used by the live-multi-phone profile-isolation scenario to
    /// assert that each profile got its own `working/<profile_id>/` dir.
    pub fn dev_data_dir(&self) -> PathBuf {
        self._tmpdir.path().join("dev-data")
    }

    /// Everything the server has logged so far: a dev build's `./logs/`,
    /// in this server's temp dir, every file of it.
    pub fn log_text(&self) -> String {
        let Ok(dir) = std::fs::read_dir(self._tmpdir.path().join("logs")) else {
            return String::new();
        };
        let mut files: Vec<PathBuf> = dir.flatten().map(|e| e.path()).collect();
        files.sort();
        files
            .iter()
            .filter_map(|f| std::fs::read_to_string(f).ok())
            .collect()
    }

    /// Forcibly kill the server child while keeping the chromedriver and
    /// the rest of the harness alive. Tests that want to observe phone-side
    /// behavior when the server goes away (e.g. the ConnectionLost overlay
    /// — PLAN 4.18.21) call this to simulate a sudden WS drop. After this,
    /// `self.url` no longer responds; chromedriver and the connected phone
    /// session are unaffected and remain usable until normal Drop.
    pub fn kill_server(&mut self) {
        self._child.kill_now();
    }

    /// Spawn the server configured for **real RPCS3 + real UIA driver**. Used
    /// by the live-integration test (`tests/live_integration.rs`) — the
    /// regular mock-driver tests use [`spawn`].
    ///
    /// Requires `RPCS3_EXE` in the environment (same contract as the live
    /// lifecycle tests in `crates/rpcs3-control/tests/`). The `test-hooks`
    /// feature is still enabled so the phone's `#k=` auth flow works via
    /// `/api/_test/hmac_key`, but no `inject_load_outcomes` call should be
    /// made — the real driver handles its own failure modes.
    pub fn spawn_live() -> Result<Self> {
        ensure_product("spawn_live")?;
        let repo = repo_root()?;

        let rpcs3_exe = std::env::var("RPCS3_EXE")
            .map(PathBuf::from)
            .context("RPCS3_EXE env var required for live-integration tests")?;
        if !rpcs3_exe.is_file() {
            bail!(
                "RPCS3_EXE does not point to a file: {}",
                rpcs3_exe.display()
            );
        }

        let firmware = resolve_firmware_pack(&repo)?;

        let phone_dist = repo.join("phone").join("dist");
        if !phone_dist.join("index.html").is_file() {
            bail!(
                "phone SPA not built — run `cd phone && trunk build` first (looking in {})",
                phone_dist.display()
            );
        }

        // If a previous run's forced shutdown left RPCS3.buf behind, next
        // launch fails with "Another instance of RPCS3 is running". Same
        // defensive clear the live-lifecycle tests do.
        if let Some(dir) = rpcs3_exe.parent() {
            let _ = std::fs::remove_file(dir.join("RPCS3.buf"));
        }

        let tmp = tempfile::tempdir().context("create temp dir")?;
        link_repo_images(tmp.path(), &repo);
        let port = pick_free_port()?;
        // Omit SKYLANDER_PORTAL_DRIVER so config's default (`Uia`) wins —
        // setting it to anything else would downgrade back to the mock.
        let env = format!(
            "RPCS3_EXE={rpcs3}\nFIRMWARE_PACK_ROOT={pack}\nBIND_PORT={port}\nPHONE_DIST={phone}\n",
            rpcs3 = rpcs3_exe.display(),
            pack = firmware.display(),
            port = port,
            phone = phone_dist.display(),
        );
        std::fs::write(tmp.path().join(".env.dev"), env)?;

        let mut cmd = Command::new("cargo");
        cmd.current_dir(tmp.path())
            .env("CARGO_MANIFEST_DIR", &repo)
            .env("CARGO_TARGET_DIR", repo.join("target"))
            .args([
                "run",
                "--manifest-path",
                repo.join("Cargo.toml").to_str().unwrap(),
                "-p",
                "skylander-server",
                "--features",
                "test-hooks",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Same BUILD_TOKEN pin as `spawn` — see comment there.
            .env("BUILD_TOKEN", "e2e-test");
        // Process group + group-kill, same rationale as `spawn`.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let mut child = cmd.spawn().context("spawn server via cargo run")?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let (tx, rx) = mpsc::channel::<String>();
        spawn_reader("stdout", stdout, tx.clone());
        spawn_reader("stderr", stderr, tx);

        let url = wait_for_url(&rx, Duration::from_secs(120))?;
        let guard = ChildGuard::new(child);

        let (chromedriver_url, chromedriver_guard) = spawn_chromedriver()?;

        Ok(Self {
            url,
            chromedriver_url,
            _child: guard,
            _chromedriver: chromedriver_guard,
            _tmpdir: tmp,
        })
    }

    /// Spawn the server for the **IPC driver + a real patched RPCS3 booting a
    /// save state** — the play-through recorder's in-game tier (PLAN 15.12).
    /// Reads three env vars: `RPCS3_EXE` (the patched binary), `RPCS3_CONFIG_DIR`
    /// (the user's RPCS3 install — firmware, `games.yml`, the save state; pass it
    /// trailing-slashed per the RPCS3 quirk), and `SKYLANDER_BOOT_SAVESTATE` (the
    /// `.SAVESTAT(.zst)` path). The server boots that save state straight to the
    /// in-game portal with the save-state RPCS3 config (ASMJIT + Compatible
    /// Savestate Mode) swapped in transiently. `test-hooks` stays on for profile
    /// inject / unlock; the phone then drives a real `/api/launch`, which is what
    /// triggers the save-state boot.
    /// Shared IPC-server bringup. `savestate = Some(path)` boots that save state
    /// (straight to the in-game portal); `None` cold-boots the EBOOT the server
    /// resolves from games.yml (A.2.4 — the recorder then pad-navs to the portal).
    fn spawn_ipc_inner(savestate: Option<String>) -> Result<Self> {
        ensure_product("spawn_ipc_*")?;
        let repo = repo_root()?;

        let rpcs3_exe = std::env::var("RPCS3_EXE")
            .map(PathBuf::from)
            .context("RPCS3_EXE env var required for the in-game recorder scenario")?;
        if !rpcs3_exe.is_file() {
            bail!(
                "RPCS3_EXE does not point to a file: {}",
                rpcs3_exe.display()
            );
        }
        let config_dir = std::env::var("RPCS3_CONFIG_DIR").context(
            "RPCS3_CONFIG_DIR env var required (your RPCS3 install: firmware + games.yml + savestates/)",
        )?;

        let firmware = resolve_firmware_pack(&repo)?;
        let phone_dist = repo.join("phone").join("dist");
        if !phone_dist.join("index.html").is_file() {
            bail!(
                "phone SPA not built — run `cd phone && BUILD_TOKEN=e2e-test trunk build` first (looking in {})",
                phone_dist.display()
            );
        }
        // Defensive RPCS3.buf clear — a prior forced shutdown otherwise blocks
        // the next launch with "Another instance of RPCS3 is running".
        if let Some(dir) = rpcs3_exe.parent() {
            let _ = std::fs::remove_file(dir.join("RPCS3.buf"));
        }

        let tmp = tempfile::tempdir().context("create temp dir")?;
        link_repo_images(tmp.path(), &repo);
        let port = pick_free_port()?;
        // Savestate boot is opt-in: present → boot that state; absent → cold boot.
        let savestate_line = match &savestate {
            Some(ss) => format!("SKYLANDER_BOOT_SAVESTATE={ss}\n"),
            None => String::new(),
        };
        let env = format!(
            // WINDOW_MODE=desktop so the launcher is windowed and 20.4 fits the
            // RPCS3 game window to it — both visible in the capture (TV mode would
            // fullscreen the launcher over RPCS3, hiding the in-game portal).
            "RPCS3_EXE={rpcs3}\nRPCS3_CONFIG_DIR={cfg}\nFIRMWARE_PACK_ROOT={pack}\nBIND_PORT={port}\nSKYLANDER_PORTAL_DRIVER=ipc\n{ss}WINDOW_MODE=desktop\nPHONE_DIST={phone}\n",
            rpcs3 = rpcs3_exe.display(),
            cfg = config_dir,
            pack = firmware.display(),
            port = port,
            ss = savestate_line,
            phone = phone_dist.display(),
        );
        std::fs::write(tmp.path().join(".env.dev"), env)?;

        let mut cmd = Command::new("cargo");
        cmd.current_dir(tmp.path())
            .env("CARGO_MANIFEST_DIR", &repo)
            .env("CARGO_TARGET_DIR", repo.join("target"))
            .args([
                "run",
                "--manifest-path",
                repo.join("Cargo.toml").to_str().unwrap(),
                "-p",
                "skylander-server",
                "--features",
                "test-hooks",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("BUILD_TOKEN", "e2e-test");
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let mut child = cmd.spawn().context("spawn server via cargo run")?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel::<String>();
        spawn_reader("stdout", stdout, tx.clone());
        spawn_reader("stderr", stderr, tx);

        let url = wait_for_url(&rx, Duration::from_secs(120))?;
        let guard = ChildGuard::new(child);
        let (chromedriver_url, chromedriver_guard) = spawn_chromedriver()?;

        Ok(Self {
            url,
            chromedriver_url,
            _child: guard,
            _chromedriver: chromedriver_guard,
            _tmpdir: tmp,
        })
    }

    /// Boot the IPC server resuming a real save state straight to the in-game
    /// portal. Requires `RPCS3_EXE`, `RPCS3_CONFIG_DIR`, `SKYLANDER_BOOT_SAVESTATE`.
    pub fn spawn_ipc_savestate() -> Result<Self> {
        let savestate = std::env::var("SKYLANDER_BOOT_SAVESTATE")
            .context("SKYLANDER_BOOT_SAVESTATE env var required (path to the .SAVESTAT(.zst))")?;
        Self::spawn_ipc_inner(Some(savestate))
    }

    /// Cold-boot the real game (no save state) for the A.2.4 recorder flow: the
    /// server boots the EBOOT resolved from games.yml; the recorder pad-navs to
    /// the portal. Requires `RPCS3_EXE`, `RPCS3_CONFIG_DIR` (firmware + games.yml).
    pub fn spawn_ipc_cold() -> Result<Self> {
        Self::spawn_ipc_inner(None)
    }
}

/// The RPCS3 spawns drive the product's emulator; guacamole has none.
fn ensure_product(spawn: &str) -> Result<()> {
    if E2eApp::from_env() == E2eApp::Guacamole {
        bail!("{spawn} runs RPCS3, so it's product-only; unset E2E_APP=guacamole");
    }
    Ok(())
}

fn built_phone_dist(repo: &Path) -> Result<PathBuf> {
    let phone_dist = repo.join("phone").join("dist");
    if !phone_dist.join("index.html").is_file() {
        bail!(
            "phone SPA not built — run `cd phone && trunk build` first (looking in {})",
            phone_dist.display()
        );
    }
    Ok(phone_dist)
}

async fn fetch_hmac_key_hex(base: &str) -> anyhow::Result<String> {
    #[derive(serde::Deserialize)]
    struct Body {
        hmac_key: String,
    }
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/_test/hmac_key"))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("hmac_key hook returned {}", resp.status());
    }
    let body: Body = resp.json().await?;
    Ok(body.hmac_key)
}

/// Spawn a dedicated chromedriver on a free port and wait for it to accept
/// connections. Returns the base URL and an owning guard that kills the
/// process on drop.
///
/// Symlink the repo's scraped figure portraits into a temp server's data_root so
/// the phone's `/api/figures/:id/image?size=…` requests resolve in the recorder
/// and e2e harness (#26). The server's `data_root` defaults to `./data` relative
/// to its CWD (the temp dir), but the ~474 scraped thumbnails live in the repo's
/// `data/images/`; without this link every portrait 404s and the phone shows
/// blank bezels. Best-effort, unix-only (the recorder is macOS; a real install
/// ships the images inside its own data_root).
fn link_repo_images(tmp_path: &std::path::Path, repo: &std::path::Path) {
    let repo_images = repo.join("data").join("images");
    if !repo_images.is_dir() {
        return;
    }
    let link = tmp_path.join("data").join("images");
    if link.exists() {
        return;
    }
    if let Some(parent) = link.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(&repo_images, &link);
}

/// Resolution order for the chromedriver binary:
///   1. `$CHROMEDRIVER` env var (explicit override).
///   2. macOS: a driver pinned to the installed Chrome's major version,
///      `~/Library/Application Support/skylander-portal-controller/chromedriver-<major>`
///      (Chrome and ChromeDriver must share a major; brew's cask often doesn't).
///   3. `chromedriver` on PATH, then Homebrew's two prefixes.
///   4. The winget install location
///      (`%LOCALAPPDATA%/Microsoft/WinGet/Packages/Chromium.ChromeDriver_*/chromedriver-win64/chromedriver.exe`).
fn spawn_chromedriver() -> Result<(String, ChildGuard)> {
    let port = pick_free_port()?;
    let bin = locate_chromedriver()?;
    let mut cmd = Command::new(&bin);
    cmd.arg(format!("--port={port}"))
        .arg("--silent")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // PLAN 10.3.6: spawn into our own process group so killing the
    // chromedriver guard takes down its forked Chromes too. Without
    // this a panicked test leaves dozens of `Google Chrome Helper`
    // processes orphaned (Chris flagged this 2026-05-02).
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let child = cmd.spawn().with_context(|| {
        format!(
            "spawn chromedriver from {} (install via `winget install --id=Chromium.ChromeDriver` \
             or grab a matching build from https://googlechromelabs.github.io/chrome-for-testing/)",
            bin.display()
        )
    })?;
    let guard = ChildGuard::new(child);

    let url = format!("http://127.0.0.1:{port}");
    let addr = format!("127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_millis(200))
            .is_ok()
        {
            // Port accepts connections — chromedriver is up. The first
            // fantoccini handshake will surface any deeper issues.
            return Ok((url, guard));
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(anyhow!(
        "chromedriver at {url} didn't become ready within 10s"
    ))
}

fn spawn_reader(
    tag: &'static str,
    stream: impl std::io::Read + Send + 'static,
    tx: mpsc::Sender<String>,
) {
    thread::spawn(move || {
        let reader = BufReader::new(stream);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[{tag}] {line}");
            let _ = tx.send(line);
        }
    });
}

/// `cargo run -p <package> --features test-hooks` in `dir`; returns once the
/// server logs the URL it serves on.
fn start_server(dir: &Path, repo: &Path, package: &str) -> Result<(String, ChildGuard)> {
    // Build once up front so subsequent spawns are fast; re-invoking
    // cargo run also rebuilds incrementally if source changed.
    let mut cmd = Command::new("cargo");
    cmd.current_dir(dir)
        .env("CARGO_MANIFEST_DIR", repo)
        .env("CARGO_TARGET_DIR", repo.join("target"))
        // Pin BUILD_TOKEN so the spawned server's stale-bundle
        // check matches the phone bundle. Without this the phone's
        // baked-in `<git-hash>-dirty` value drifts away from the
        // server's whenever the working tree's dirty state changes
        // independently (e.g., editing tests after the last
        // `trunk build`), which raises a StaleVersion overlay
        // mid-test and blocks every click. Contributors should
        // build the phone the same way: `BUILD_TOKEN=e2e-test
        // trunk build`. Documented in
        // `crates/e2e-tests/README.md`.
        .env("BUILD_TOKEN", "e2e-test")
        // guacamole's intros play muted: nobody listens to an e2e run.
        .env("GUAC_MUTE", "1")
        // guacamole's TV takes only the posted pads (`/api/_test/tv/pad`),
        // neither gilrs's nor GameController's: a pad paired with the
        // machine would walk the hero first (AG.52, AG.53).
        .env("GUAC_TEST_POSTED_PADS_ONLY", "1")
        .args([
            "run",
            "--manifest-path",
            repo.join("Cargo.toml").to_str().unwrap(),
            "-p",
            package,
            "--features",
            "test-hooks",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Spawn into our own process group so ChildGuard can kill the
    // entire chain (cargo + rustc + the eventual server binary)
    // atomically. Without this, killing `cargo` doesn't reliably
    // take down its descendants. PLAN 10.3.6 process-hygiene.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd.spawn().context("spawn server via cargo run")?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Read both streams in parallel; scrape URL from either.
    let (tx, rx) = mpsc::channel::<String>();
    spawn_reader("stdout", stdout, tx.clone());
    spawn_reader("stderr", stderr, tx);

    let url = wait_for_url(&rx, Duration::from_secs(120))?;
    Ok((url, ChildGuard::new(child)))
}

fn wait_for_url(rx: &mpsc::Receiver<String>, timeout: Duration) -> Result<String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining) {
            Ok(line) => {
                if let Some(idx) = line.find("serving on http") {
                    let tail = &line[idx + "serving on ".len()..];
                    let url = tail.split_whitespace().next().unwrap_or(tail).trim();
                    return Ok(url.to_string());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("server exited before logging a URL");
            }
        }
    }
    Err(anyhow!(
        "server didn't print 'serving on http://…' within {timeout:?}"
    ))
}

fn locate_chromedriver() -> Result<PathBuf> {
    if let Ok(s) = std::env::var("CHROMEDRIVER") {
        let p = PathBuf::from(s);
        if p.is_file() {
            return Ok(p);
        }
        bail!("CHROMEDRIVER points at {} which doesn't exist", p.display());
    }
    #[cfg(target_os = "macos")]
    if let Some(p) = pinned_chromedriver_for_installed_chrome() {
        return Ok(p);
    }
    if let Ok(p) = which::which("chromedriver") {
        return Ok(p);
    }
    // macOS: Homebrew installs to /opt/homebrew/bin (Apple Silicon) or
    // /usr/local/bin (Intel). Cargo tests don't always inherit a shell
    // PATH that includes brew, so check both directly before failing.
    #[cfg(target_os = "macos")]
    {
        for cand in [
            "/opt/homebrew/bin/chromedriver",
            "/usr/local/bin/chromedriver",
        ] {
            let pb = PathBuf::from(cand);
            if pb.is_file() {
                return Ok(pb);
            }
        }
    }
    // Windows: winget installs ChromeDriver under
    // %LOCALAPPDATA%\Microsoft\WinGet\Packages\Chromium.ChromeDriver_*
    // — walk that one dir-deep to find the versioned subfolder.
    #[cfg(windows)]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let winget_root = PathBuf::from(local).join("Microsoft/WinGet/Packages");
        if let Ok(entries) = std::fs::read_dir(&winget_root) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_s = name.to_string_lossy();
                if name_s.starts_with("Chromium.ChromeDriver_") {
                    let candidate = entry
                        .path()
                        .join("chromedriver-win64")
                        .join("chromedriver.exe");
                    if candidate.is_file() {
                        return Ok(candidate);
                    }
                }
            }
        }
    }
    Err(anyhow!(
        "chromedriver not found — set $CHROMEDRIVER, add it to PATH, or install \
         via `brew install --cask chromedriver` (macOS) / \
         `winget install --id=Chromium.ChromeDriver` (Windows)"
    ))
}

/// macOS: the chromedriver pinned beside the app's runtime data whose name
/// carries the installed Chrome's major version (`chromedriver-155` for
/// Chrome 155.x). `None` when Chrome isn't in /Applications or no pinned
/// driver matches, so the caller falls back to PATH / Homebrew.
#[cfg(target_os = "macos")]
fn pinned_chromedriver_for_installed_chrome() -> Option<PathBuf> {
    let out = Command::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        .arg("--version")
        .output()
        .ok()?;
    let major = chrome_major_version(&String::from_utf8_lossy(&out.stdout))?;
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home)
        .join("Library/Application Support/skylander-portal-controller")
        .join(format!("chromedriver-{major}"));
    p.is_file().then_some(p)
}

/// Major version out of `Google Chrome 155.0.8059.85` (the `--version` line).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn chrome_major_version(version_line: &str) -> Option<u32> {
    version_line
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))?
        .split('.')
        .next()?
        .parse()
        .ok()
}

fn pick_free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// Resolve the firmware pack to feed the spawned server. The phone SPA
/// renders one `.card` per indexed figure, so the e2e suite needs a
/// real pack — `tools/inventory` has no `.sky` files and the bundled
/// fixtures don't cover enough surface area.
///
/// Resolution order:
/// 1. `$SKYLANDER_PACK_ROOT` env var (explicit override).
/// 2. `<repo>/dev-data/firmware-pack/` — the standard contributor
///    layout. Mac users + Windows users with the dev-data sibling
///    directory both land here without configuration.
/// 3. The Windows HTPC default (`C:\Users\chris\workspace\Skylanders
///    Characters Pack for RPCS3`) — kept because Chris's HTPC has
///    the pack outside the repo tree, predates the dev-data
///    convention, and is documented in CLAUDE.md.
///
/// Errors with a hint about `SKYLANDER_PACK_ROOT` if none of the
/// candidates exist.
fn resolve_firmware_pack(repo: &Path) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("SKYLANDER_PACK_ROOT") {
        let pb = PathBuf::from(&p);
        if !pb.is_dir() {
            bail!(
                "SKYLANDER_PACK_ROOT={} is not an existing directory",
                pb.display()
            );
        }
        return Ok(pb);
    }
    let dev_data = repo.join("dev-data").join("firmware-pack");
    if dev_data.is_dir() {
        return Ok(dev_data);
    }
    let chris_htpc =
        PathBuf::from(r"C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3");
    if chris_htpc.is_dir() {
        return Ok(chris_htpc);
    }
    bail!(
        "no firmware pack found — tried {} and {}. Set SKYLANDER_PACK_ROOT to your local pack.",
        dev_data.display(),
        chris_htpc.display(),
    )
}

fn repo_root() -> Result<PathBuf> {
    // crates/e2e-tests/ → repo root.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .ancestors()
        .nth(2)
        .ok_or_else(|| anyhow!("couldn't locate repo root from {manifest:?}"))?;
    Ok(root.to_path_buf())
}

/// RAII guard for spawned subprocesses. On Drop (or explicit
/// `kill_now`), kills the whole process group, not just the immediate
/// child. The leverage matters for chromedriver: a panicked test
/// would leave the chromedriver-spawned headless Chromes orphaned
/// because they don't share chromedriver's parent (each `Phone::new`
/// creates a new Chrome via the WebDriver session). PLAN 10.3.6
/// process-hygiene note tracked this for ages — the fix is to spawn
/// chromedriver into its own process group (`Command::process_group(0)`)
/// and `kill -KILL -<pgid>` on cleanup.
///
/// Same treatment for the server child — `cargo run` forks a chain of
/// rustc + the eventual server binary; killing only `cargo` itself
/// can leave the server process running.
struct ChildGuard {
    child: Option<Child>,
    /// Process group id of the spawned child. On Unix the child is
    /// spawned with `process_group(0)`, which makes its PID the
    /// pgid. On Windows we don't track this — std::process doesn't
    /// expose process-group control, and the existing CI doesn't
    /// run e2e on Windows. None when not on Unix.
    #[cfg(unix)]
    pgid: Option<i32>,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        #[cfg(unix)]
        let pgid = Some(child.id() as i32);
        Self {
            child: Some(child),
            #[cfg(unix)]
            pgid,
        }
    }

    /// Kill the wrapped child + its entire process group immediately
    /// and detach. Used by `TestServer::kill_server` to simulate a
    /// server crash mid-test, and by Drop to ensure no Chrome /
    /// cargo-rustc / server zombies survive a panicked test.
    fn kill_now(&mut self) {
        if let Some(mut child) = self.child.take() {
            #[cfg(unix)]
            if let Some(pgid) = self.pgid.take() {
                // `kill -KILL -<pgid>` (negative = group). SIGKILL is
                // uncatchable so the whole group dies in one syscall;
                // ignore the kill subprocess's exit (errors here mean
                // "group already gone" — fine for cleanup).
                let _ = std::process::Command::new("kill")
                    .arg("-KILL")
                    .arg(format!("-{pgid}"))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            // Belt-and-suspenders: kill the leader directly too, in
            // case the group-kill missed it (e.g. the leader had
            // already escaped the original group).
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill_now();
    }
}

// ---------------------------------------------------------------- Phone

/// A fantoccini `Client` bound to the server URL.
pub struct Phone {
    pub client: Client,
}

impl Phone {
    /// Connect a new headless Chrome session via the given chromedriver URL,
    /// navigate to `server_url`.
    pub async fn new(server_url: &str, chromedriver_url: &str) -> Result<Self> {
        let caps = serde_json::from_str::<serde_json::Value>(
            r#"{"goog:chromeOptions": {"args": ["--headless=new", "--no-sandbox", "--disable-gpu", "--window-size=420,900"]}}"#,
        )?;
        let client = ClientBuilder::native()
            .capabilities(caps.as_object().unwrap().clone())
            .connect(chromedriver_url)
            .await
            .with_context(|| format!("connect to chromedriver at {chromedriver_url}"))?;
        client.goto(server_url).await?;
        Ok(Self { client })
    }

    /// Like [`Phone::new`] with Chrome's phone emulation at `width` x
    /// `height` CSS px: the page's viewport is exactly that, smaller than a
    /// desktop window may go, as a phone browser with its toolbars on
    /// screen leaves it (openspec AA.20).
    pub async fn new_sized(
        server_url: &str,
        chromedriver_url: &str,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let caps = serde_json::json!({
            "goog:chromeOptions": {
                "args": ["--headless=new", "--no-sandbox", "--disable-gpu", "--window-size=600,900"],
                "mobileEmulation": {
                    "deviceMetrics": { "width": width, "height": height, "pixelRatio": 3.0, "touch": true }
                }
            }
        });
        let client = ClientBuilder::native()
            .capabilities(caps.as_object().unwrap().clone())
            .connect(chromedriver_url)
            .await
            .with_context(|| format!("connect to chromedriver at {chromedriver_url}"))?;
        client.goto(server_url).await?;
        Ok(Self { client })
    }

    /// Like [`Phone::new`] but a VISIBLE (non-headless) Chrome window pinned
    /// at a fixed screen position + size, keeping the normal tab strip /
    /// omnibox. Legacy fallback with no current callers: the play-through
    /// recorder that motivated it (PLAN 15.11) moved to
    /// [`Phone::new_headed_app`] (chromeless app mode, PLAN 15.14) — kept
    /// for flows where visible browser chrome is the point (e.g. debugging
    /// navigation by hand against a harness-managed server).
    pub async fn new_headed(
        server_url: &str,
        chromedriver_url: &str,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    ) -> Result<Self> {
        let args = format!(
            r#"["--no-sandbox", "--disable-gpu", "--window-position={x},{y}", "--window-size={w},{h}", "--disable-infobars"]"#
        );
        let caps = serde_json::from_str::<serde_json::Value>(&format!(
            r#"{{"goog:chromeOptions": {{"args": {args}}}}}"#
        ))?;
        let client = ClientBuilder::native()
            .capabilities(caps.as_object().unwrap().clone())
            .connect(chromedriver_url)
            .await
            .with_context(|| format!("connect to chromedriver at {chromedriver_url}"))?;
        client.goto(server_url).await?;
        Ok(Self { client })
    }

    /// Like [`Phone::new_headed`] but as a **chromeless app-mode window**
    /// (`--app=<url>`: no tab strip / omnibox) with the "controlled by
    /// automated test software" banner suppressed
    /// (`excludeSwitches: ["enable-automation"]`). For the play-through
    /// recorder's framing cleanup (PLAN 15.14) — the captured phone column
    /// must show only the SPA, not browser chrome.
    ///
    /// Chrome interprets `--window-position` / `--window-size` in **DIPs**,
    /// not physical pixels, so on a scaled display the window lands off the
    /// requested rect. Callers needing physical-pixel placement must correct
    /// via Win32 afterwards (the recorder does — `stage::place_window`);
    /// `x/y/w/h` here are only the initial hint.
    ///
    /// The `goto` after connect is idempotent (the app window already opened
    /// `server_url`) but binds the WebDriver session to the app window, and
    /// app-mode windows navigate normally afterwards — mid-run `goto` calls
    /// (e.g. the mock place-figure beat's reload) need no special handling.
    pub async fn new_headed_app(
        server_url: &str,
        chromedriver_url: &str,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    ) -> Result<Self> {
        let caps = serde_json::json!({
            "goog:chromeOptions": {
                "args": [
                    "--no-sandbox",
                    "--disable-gpu",
                    format!("--app={server_url}"),
                    format!("--window-position={x},{y}"),
                    format!("--window-size={w},{h}"),
                ],
                "excludeSwitches": ["enable-automation"],
            }
        });
        let client = ClientBuilder::native()
            .capabilities(caps.as_object().unwrap().clone())
            .connect(chromedriver_url)
            .await
            .with_context(|| format!("connect to chromedriver at {chromedriver_url}"))?;
        client.goto(server_url).await?;
        Ok(Self { client })
    }

    pub async fn wait_for(
        &self,
        loc: Locator<'_>,
        timeout: Duration,
    ) -> Result<fantoccini::elements::Element> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.client.find(loc).await {
                Ok(e) => return Ok(e),
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    pub async fn text_in(&self, loc: Locator<'_>) -> Result<String> {
        let el = self.client.find(loc).await?;
        Ok(el.text().await.unwrap_or_default())
    }

    pub async fn close(self) -> Result<()> {
        self.client.close().await?;
        Ok(())
    }

    /// Capture a full-viewport PNG of the current page and write it to
    /// `path`. Used by the docs-site screenshot tour
    /// (`tests/screenshot_tour.rs`) — fantoccini's `screenshot()`
    /// returns the PNG bytes already encoded, so we just write them
    /// out. Parent directory must exist.
    pub async fn screenshot(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let bytes = self.client.screenshot().await?;
        std::fs::write(path.as_ref(), bytes)
            .with_context(|| format!("write screenshot to {}", path.as_ref().display()))?;
        Ok(())
    }

    /// Dispatch a synthetic `pointerdown` + `pointerup` at the centre of the
    /// first element matching `selector`, via JS. Some controls (the portal
    /// lid grabber) listen to PointerEvents rather than `click`, and a
    /// synthetic WebDriver click doesn't reach them. Used by the screenshot
    /// tour + the play-through recorder.
    pub async fn tap_pointer(&self, selector: &str) -> Result<()> {
        let js = format!(
            r#"
            const el = document.querySelector('{sel}');
            if (!el) return null;
            const r = el.getBoundingClientRect();
            const opts = {{
                pointerId: 1, isPrimary: true, bubbles: true,
                clientX: r.left + r.width / 2,
                clientY: r.top + r.height / 2,
            }};
            el.dispatchEvent(new PointerEvent('pointerdown', opts));
            el.dispatchEvent(new PointerEvent('pointerup',   opts));
            return true;
            "#,
            sel = selector,
        );
        let _ = self.client.execute(&js, vec![]).await?;
        Ok(())
    }

    /// Click the first element matching `selector` via JS (`el.click()`),
    /// bypassing WebDriver's interactability check. Robust against controls
    /// caught mid-animation or transiently covered by a closing overlay, which
    /// fail a normal `click()` with "element not interactable" — common in a
    /// headed (non-headless) browser. Returns `false` if nothing matched.
    pub async fn js_click(&self, selector: &str) -> Result<bool> {
        let js = format!(
            "const el = document.querySelector('{sel}'); if (!el) {{ return false; }} el.click(); return true;",
            sel = selector,
        );
        Ok(self
            .client
            .execute(&js, vec![])
            .await?
            .as_bool()
            .unwrap_or(false))
    }
}

// ---------------------------------------------------------------- REST

/// Queue injected outcomes for the server's upcoming loads or placements.
pub async fn inject_load_outcomes(base: &str, outcomes: serde_json::Value) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/inject_load"))
        .json(&serde_json::json!({ "outcomes": outcomes }))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "inject_load returned {}: {}",
            resp.status(),
            resp.text().await?
        );
    }
    Ok(())
}

/// Simulate a game launch without touching RPCS3. Sets the server's current
/// game via the `test-hooks` backdoor; the phone flips out of the GamePicker.
pub async fn set_game(base: &str, current: Option<serde_json::Value>) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/set_game"))
        .json(&serde_json::json!({ "current": current }))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "set_game returned {}: {}",
            resp.status(),
            resp.text().await?
        );
    }
    Ok(())
}

/// Inject a profile via the test-hook. Returns the new profile id.
pub async fn inject_profile(base: &str, name: &str, pin: &str, color: &str) -> Result<String> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/inject_profile"))
        .json(&serde_json::json!({
            "name": name,
            "pin": pin,
            "color": color,
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "inject_profile returned {}: {}",
            resp.status(),
            resp.text().await?
        );
    }
    let body: serde_json::Value = resp.json().await?;
    Ok(body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("inject_profile: no id in response"))?
        .to_string())
}

/// Flip the server session into the given profile (bypasses PIN entry).
/// When there's only one session (the typical single-phone test), this
/// seeds `pending_unlock` and also updates the most recent existing
/// session. For 2-phone tests, call this between `Phone::new`s to target
/// each phone in sequence.
pub async fn unlock_session(base: &str, profile_id: &str) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/unlock_session"))
        .json(&serde_json::json!({ "profile_id": profile_id }))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "unlock_session returned {}: {}",
            resp.status(),
            resp.text().await?
        );
    }
    Ok(())
}

/// Clear the server's 1-minute forced-eviction cooldown so
/// `third_connection_evicts_oldest`-style tests can back-to-back evict
/// without sleeping. Behind `test-hooks` on the server.
pub async fn clear_eviction_cooldown(base: &str) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/clear_eviction_cooldown"))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "clear_eviction_cooldown returned {}: {}",
            resp.status(),
            resp.text().await?
        );
    }
    Ok(())
}

/// Trigger the launcher's graceful shutdown — flips the eframe surface into the
/// Farewell screen (GOODBYE badge → ~3s countdown → window close). Posts
/// `/api/shutdown` unsigned, which the server accepts under `dev-tools` (the
/// signed-request dev bypass). Used by the Tour's `farewell` closer beat to
/// guarantee the Farewell surface renders regardless of the captured phone's
/// state (it can't always sign post-takeover). The real phone path is the
/// kebab's HOLD TO SHUT DOWN, exercised separately in-beat for the visual.
pub async fn post_shutdown(base: &str) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/shutdown"))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "post_shutdown returned {}: {}",
            resp.status(),
            resp.text().await?
        );
    }
    Ok(())
}

/// Bind a specific session to a profile. Used by 2-phone tests that need to
/// give each phone its own independent unlock — the lighter-touch
/// `unlock_session` helper seeds `pending_unlock` which only affects the
/// next-registered session, so for phones already connected you need this
/// one. Caller supplies the session id from the phone's DOM
/// (`Phone::session_id()`).
pub async fn set_session_profile(base: &str, session_id: u64, profile_id: &str) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/set_session_profile"))
        .json(&serde_json::json!({
            "session_id": session_id,
            "profile_id": profile_id,
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "set_session_profile returned {}: {}",
            resp.status(),
            resp.text().await?
        );
    }
    Ok(())
}

/// Inject a default "Player 1" profile and unlock the session under it. The
/// existing game-picker/portal regression scenarios use this in setup now
/// that the profile-picker is the first screen.
pub async fn unlock_default_profile(base: &str) -> Result<String> {
    // Idempotent: reuse an existing Player 1 if the test already created one
    // (e.g. this is a second call after `location.reload()`). Creating a
    // fresh profile every call breaks the resume-prompt flow because the
    // saved layout is keyed on the *old* profile id.
    let existing: Vec<serde_json::Value> = reqwest::Client::new()
        .get(format!("{base}/api/profiles"))
        .send()
        .await?
        .json()
        .await?;
    let id = match existing
        .iter()
        .find(|p| p.get("display_name").and_then(|v| v.as_str()) == Some("Player 1"))
        .and_then(|p| p.get("id").and_then(|v| v.as_str()))
        .map(String::from)
    {
        Some(id) => id,
        None => inject_profile(base, "Player 1", "1234", "#39d39f").await?,
    };
    unlock_session(base, &id).await?;
    Ok(id)
}

/// Fire a synthetic Kaos taunt on the screenshot-tour test-hook so
/// the kaos_swap overlay renders without waiting on the real 20-min
/// warmup timer. Hook lives behind `test-hooks` on the server.
pub async fn fire_kaos_taunt(
    base: &str,
    profile_id: &str,
    slot: u8,
    old_figure_id: &str,
    new_figure_id: &str,
    taunt: &str,
) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/fire_kaos_taunt"))
        .json(&serde_json::json!({
            "profile_id": profile_id,
            "slot": slot,
            "old_figure_id": old_figure_id,
            "new_figure_id": new_figure_id,
            "taunt": taunt,
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "fire_kaos_taunt returned {}: {}",
            resp.status(),
            resp.text().await?
        );
    }
    Ok(())
}

/// Fire a REAL Kaos swap on demand (PLAN A.3 demo beat) — POSTs to the
/// server's `fire_kaos_swap` test hook, which runs the genuine
/// `select_swap` + `execute_kaos_swap` path: a real ClearSlot -> LoadFigure
/// on the portal (the placed figure visibly changes, in-game too) plus the
/// overlay/taunt broadcast. Unlike `fire_kaos_taunt` (overlay only), this
/// actually swaps the figure. A 200 with body "no eligible swap" means the
/// portal had nothing swappable — the caller can treat that as non-fatal.
pub async fn fire_kaos_swap(base: &str, profile_id: &str) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/fire_kaos_swap"))
        .json(&serde_json::json!({ "profile_id": profile_id }))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "fire_kaos_swap returned {}: {}",
            resp.status(),
            resp.text().await?
        );
    }
    Ok(())
}

/// Fire a synthetic TakenOver event for the targeted session, so the
/// Kaos takeover overlay can be screenshotted without running the
/// FIFO eviction path with a 3rd phone. `cooldown_remaining_secs = 0`
/// captures the enabled-button variant.
pub async fn fire_takeover(
    base: &str,
    session_id: u64,
    by_kaos: &str,
    cooldown_remaining_secs: u32,
) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/fire_takeover"))
        .json(&serde_json::json!({
            "session_id": session_id,
            "by_kaos": by_kaos,
            "cooldown_remaining_secs": cooldown_remaining_secs,
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "fire_takeover returned {}: {}",
            resp.status(),
            resp.text().await?
        );
    }
    Ok(())
}

/// One intro in an [`IntroFixture`]: the toy type that plays it, its movie
/// name and its length.
#[derive(Debug, Clone, Copy)]
pub struct FixtureIntro {
    pub toy_type: u32,
    pub movie: &'static str,
    pub seconds: u32,
}

/// One figure of [`E2E_ROSTER`].
#[derive(Debug, Clone, Copy)]
pub struct FixtureFigure {
    pub toy_type: u32,
    /// The subtype word; 0 on a base.
    pub variant: u16,
    pub name: &'static str,
    /// The disc's variant word, on a variant.
    pub word: Option<&'static str>,
    pub element: Option<guac_ir::Element>,
    pub category: guac_ir::Category,
    /// A Spyro's Adventure toy.
    pub sky1: bool,
}

const fn fig(
    toy_type: u32,
    name: &'static str,
    element: Option<guac_ir::Element>,
    category: guac_ir::Category,
    sky1: bool,
) -> FixtureFigure {
    FixtureFigure {
        toy_type,
        variant: 0,
        name,
        word: None,
        element,
        category,
        sky1,
    }
}

/// The roster every guacamole e2e run's extract holds (AE.4): each figure the
/// tests place, by name or toy type, under its disc id, plus one variant. Ids,
/// names and elements as the 01.00 disc gives them; written in code, no disc
/// bytes.
pub const E2E_ROSTER: &[FixtureFigure] = {
    use guac_ir::Category::{Figure, Giant, Item};
    use guac_ir::Element::{Air, Earth, Fire, Life, Magic, Tech, Water};
    &[
        fig(0x04, "Bash", Some(Earth), Figure, true),
        fig(0x09, "Eruptor", Some(Fire), Figure, true),
        fig(0x0a, "Ignitor", Some(Fire), Figure, true),
        FixtureFigure {
            variant: 0x0403,
            word: Some("Legendary"),
            ..fig(0x0a, "Ignitor", Some(Fire), Figure, true)
        },
        fig(0x0e, "Gill Grunt", Some(Water), Figure, true),
        fig(0x10, "Spyro", Some(Magic), Figure, true),
        fig(0x13, "Trigger Happy", Some(Tech), Figure, true),
        fig(0x64, "Jet-Vac", Some(Air), Figure, false),
        fig(0x70, "Tree Rex", Some(Life), Giant, false),
        fig(0xcd, "Sky-Iron Shield", None, Item, true),
    ]
};

/// The toy type whose figure has a character model in every e2e extract
/// (AE.13): Gill Grunt's card turns.
pub const E2E_MODELLED: u32 = 0x0e;

/// The toy type whose extracted model is damaged (AE.13): Bash's card keeps
/// its portrait and the log says why.
pub const E2E_DAMAGED: u32 = 0x04;

/// The toy type that walks the level in a walking fixture (AG.21,
/// [`IntroFixture::build_walking_in`]): Eruptor, on the load file the walk
/// takes for him, with the synthetic model of its own. Elsewhere he keeps
/// his portrait and the portal view, so the other tests' placements of him
/// never take the TV into the level.
pub const E2E_WALKER: u32 = 0x09;

/// The made-up chapter 1's start (`213_Init_Location`'s first waypoint) and
/// its floor's height: every e2e extract's.
pub const E2E_START: [f32; 3] = [10.191352, 43.37944, -93.393616];
pub const E2E_FLOOR: f32 = E2E_START[1] - 0.01;

/// How far ahead of the start along +z the made-up chapter 1's wall stands,
/// where its floor ends.
pub const E2E_WALL: f32 = 6.0;

/// A synthetic Giants dump and a finished extract of it (openspec AB.11,
/// AE.4, AE.13, AG.15, AH.2, AH.22): [`E2E_ROSTER`] with a flat-colour
/// portrait each, the progression tables ([`E2E_LEVEL_XP`],
/// [`E2E_GOLD_CAP`]), one synthetic character model ([`E2E_MODELLED`]) and
/// its palette, one damaged one ([`E2E_DAMAGED`]), a light rig, a chapter 1
/// of a floor and a spawn ([`write_level`]), the fight's script entries
/// ([`write_scripts`]) and the intros asked for. No game bytes: the dump is a
/// PARAM.SFO and a text file, the model is a few triangles on two joints with
/// a two-key idle and a flat-colour texture, all made up here, and each intro
/// is ffmpeg's `testsrc` picture over a `sine` tone, encoded at run time to
/// guacamole's own target. guacamole trusts the dump through
/// `GUAC_TEST_KNOWN_DUMP` and finds the extract current, so it runs no ffmpeg
/// itself and lists the roster from the start.
pub struct IntroFixture {
    _dir: Option<TempDir>,
    /// What `GUAC_DUMP_DIRS` names: holds `BLUS30968/PS3_GAME`.
    pub dumps: PathBuf,
    /// `GUAC_EXTRACT_DIR`.
    pub extract: PathBuf,
    /// `GUAC_TEST_KNOWN_DUMP`'s value: title id, version, manifest hash.
    pub known: String,
}

impl IntroFixture {
    /// In a temp dir of its own, dropped with the fixture.
    pub fn build(intros: &[FixtureIntro]) -> Result<Self> {
        let dir = tempfile::tempdir().context("create fixture dir")?;
        let mut fixture = Self::build_in(dir.path(), intros)?;
        fixture._dir = Some(dir);
        Ok(fixture)
    }

    /// The `.env.dev` lines that point guacamole at it.
    pub fn env(&self) -> String {
        format!(
            "GUAC_EXTRACT_DIR={}\nGUAC_TEST_KNOWN_DUMP={}\n",
            self.extract.display(),
            self.known
        )
    }

    /// Under `dir`, which must outlive the run (a server's own temp dir).
    pub fn build_in(dir: &Path, intros: &[FixtureIntro]) -> Result<Self> {
        Self::build_with(dir, intros, false)
    }

    /// [`Self::build_in`], with [`E2E_WALKER`] walking the level (AG.21).
    pub fn build_walking_in(dir: &Path, intros: &[FixtureIntro]) -> Result<Self> {
        Self::build_with(dir, intros, true)
    }

    fn build_with(dir: &Path, intros: &[FixtureIntro], walking: bool) -> Result<Self> {
        use guac_extract::intros::{self, IrFormat};
        use guac_extract::known::KnownDump;
        use guac_extract::transcode::TARGET;
        use guac_ir::{Character, CharacterTable, Intro, IntroSource, VoiceTrack};

        let dumps = dir.join("dumps");
        let game = dumps.join(GIANTS_SERIAL);
        let usrdir = game.join("PS3_GAME").join("USRDIR");
        std::fs::create_dir_all(&usrdir)?;
        let sfo =
            guac_formats::sfo::builder::SfoBuilder::disc_game(GIANTS_SERIAL, "01.00", "01.00")
                .build();
        std::fs::write(game.join("PS3_GAME").join("PARAM.SFO"), sfo)?;
        std::fs::write(usrdir.join("e2e-fixture.txt"), "guacamole e2e fixture\n")?;

        let scan = guac_extract::dump::scan(&game, &Default::default(), &|_| {})?;
        let hash = scan.identity.manifest_hash.to_string();
        let known = KnownDump {
            title_id: GIANTS_SERIAL,
            app_ver: "01.00",
            manifest_hash: Box::leak(hash.clone().into_boxed_str()),
            source: "e2e fixture",
        };
        let extract = dir.join("extract");
        let root = guac_extract::ExtractRoot::checked(&extract)?;
        let mut opened = guac_extract::open(&game, &root, &[known], &|_| {})?;

        let characters = intros
            .iter()
            .map(|i| {
                let intro = Intro {
                    movie: i.movie.into(),
                    source: IntroSource::NameFallback,
                    voice: VoiceTrack::Only,
                };
                let character = Character {
                    archives: Vec::new(),
                    vo_name: None,
                    intro: Some(intro),
                };
                (i.toy_type, character)
            })
            .collect();
        let table = CharacterTable {
            identity: opened.dir.manifest().identity.clone(),
            characters,
        };
        // A test-hooks build is a dev build, which reads RON.
        intros::write(&mut opened.dir, &table, IrFormat::Ron)?;
        write_roster(&mut opened.dir, walking)?;
        write_progression(&mut opened.dir)?;
        write_models(&mut opened.dir, walking)?;
        write_level(&mut opened.dir)?;
        write_scripts(&mut opened.dir)?;

        // Only the intros need ffmpeg.
        let ffmpeg = match intros {
            [] => None,
            _ => Some(
                guac_extract::ffmpeg::Ffmpeg::locate(None)
                    .context("the intro fixture encodes with ffmpeg (`brew install ffmpeg`)")?,
            ),
        };
        for (i, ffmpeg) in intros.iter().filter_map(|i| Some((i, ffmpeg.as_ref()?))) {
            let (video, audio) = TARGET.outputs(i.movie);
            let s = i.seconds;
            let jobs = [
                (
                    video,
                    format!("testsrc=size=640x360:rate=30:duration={s}"),
                    TARGET.video_args,
                ),
                (
                    audio,
                    format!("sine=frequency=440:sample_rate=48000:duration={s}"),
                    TARGET.audio_args,
                ),
            ];
            for (rel, source, args) in jobs {
                let partial = opened.dir.partial_path(&rel)?;
                let out = Command::new(&ffmpeg.path)
                    .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"])
                    .args(["-f", "lavfi", "-i", &source])
                    .args(args)
                    .arg(format!("file:{}", partial.display()))
                    .output()
                    .context("run ffmpeg")?;
                if !out.status.success() {
                    bail!(
                        "ffmpeg made no {rel}: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    );
                }
                opened.dir.commit(&rel)?;
            }
        }
        opened.dir.mark_complete()?;
        Ok(Self {
            _dir: None,
            dumps,
            extract,
            known: format!("{GIANTS_SERIAL} 01.00 {hash}"),
        })
    }
}

/// [`E2E_ROSTER`] into the extract as the roster stage leaves it: the IR, then
/// a 16x16 flat-colour PNG per figure, recorded as the portraits part.
fn write_roster(dir: &mut guac_extract::IdentityDir, walking: bool) -> Result<()> {
    use guac_extract::intros::IrFormat;
    use guac_ir::{FigureKind, Language, Roster, RosterFigure, Text};

    let text = |key: &str, en: &str| Text {
        key: key.into(),
        texts: vec![en.into()],
    };
    let figures: Vec<RosterFigure> = E2E_ROSTER
        .iter()
        .map(|f| RosterFigure {
            toy_type: f.toy_type,
            variant: f.variant,
            kind: match f.variant {
                0 => FigureKind::Base,
                _ => FigureKind::Alt,
            },
            deco: (f.variant != 0).then_some(f.variant as u8),
            name: text(&format!("{:03}_Toy_Name", f.toy_type), f.name),
            variant_word: f.word.map(|w| text("ToyType_Variant", w)),
            element: f.element,
            category: f.category,
            load_file: if walking && f.toy_type == E2E_WALKER {
                WALKING_LOAD.into()
            } else {
                load_file(f.toy_type)
            },
            portrait: Some(format!(
                "portraits/{:06x}-{:04x}.png",
                f.toy_type, f.variant
            )),
            sky1: f.sky1,
        })
        .collect();
    let roster = Roster {
        identity: dir.manifest().identity.clone(),
        languages: vec![Language {
            id: guac_ir::roster::ENGLISH,
            name: "ENGLISH".into(),
        }],
        figures,
    };
    let ir = IrFormat::Ron;
    let bytes = ir.encode(&roster, dir, ir.roster_path())?;
    dir.write_output(ir.roster_path(), &bytes)?;
    for f in &roster.figures {
        let shade = (f.toy_type % 200) as u8;
        let image = image::RgbaImage::from_pixel(16, 16, image::Rgba([shade, 96, 200, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png)?;
        dir.write_output(f.portrait.as_deref().unwrap(), png.get_ref())?;
    }
    let portraits = roster.figures.iter().filter_map(|f| f.portrait.clone());
    dir.record_part(guac_extract::roster::PORTRAITS, portraits.collect())?;
    Ok(())
}

/// Chapter 1 into the extract as the level stage leaves it (AG.15), every
/// part recorded so the extract is current: a floor at the spawn's height
/// ending at a wall (AG.21's walk), `213_Init_Location` with its first
/// waypoint, the three world lights (AG.19) and the start's atmospheric
/// zone the TV's loader lights and fogs the level with (AG.41), nothing
/// drawn and no props. Its camera is camera 0's definition, which guacamole
/// holds until AG.26 reads them off the scripts. Every number made up.
fn write_level(dir: &mut guac_extract::IdentityDir) -> Result<()> {
    use guac_extract::intros::IrFormat;
    use guac_extract::levels::{ATMOSPHERE, COLLISION, LEVELS, LIGHTS, PLACEMENTS, PROPS, SCENE};
    use guac_ir::RigLight;
    use guac_ir::level::{
        Atmosphere, AtmosphericZone, Collision, CollisionMesh, Lights, Placement, Placements,
        Props, Scene, Waypoint, ZoneFog, ZoneLight,
    };

    let ir = IrFormat::Ron;
    let identity = dir.manifest().identity.clone();
    let level = LEVELS[0].to_owned();
    let spawn = E2E_START;
    // The floor, 20 m wide, runs from 10 m behind the start to the wall
    // [`E2E_WALL`] ahead of it along +z, which shares its edge (level 213's
    // mesh is indexed so too) and stands 3 m: up, then the wall's face
    // toward the start.
    let floor = CollisionMesh {
        world: "Levels/E2E/E2E_ART_MASTER.ma".into(),
        vertices: [
            [-10.0, 0.0, -10.0],
            [10.0, 0.0, -10.0],
            [10.0, 0.0, E2E_WALL],
            [-10.0, 0.0, E2E_WALL],
            [10.0, 3.0, E2E_WALL],
            [-10.0, 3.0, E2E_WALL],
        ]
        .map(|[x, y, z]| [spawn[0] + x, E2E_FLOOR + y, spawn[2] + z])
        .to_vec(),
        triangles: vec![[0, 2, 1], [0, 3, 2], [3, 4, 2], [3, 5, 4]],
        materials: vec![0; 4],
        radius: 0.05,
        welding: 4,
    };
    let light = |facing| ZoneLight {
        diffuse: [120, 110, 100, 0],
        specular: [90, 90, 90, 0],
        facing,
        tilt: -40,
    };
    let zone = AtmosphericZone {
        id: 0,
        ambient: [30, 30, 30, 0],
        lights: [light(60), light(250), light(200)],
        fog: ZoneFog {
            on: 1,
            start: 120,
            range: 900,
            colour: [100, 90, 80, 0],
            sky_dome_percent: 0,
            sky_dome_colour: [128, 128, 128, 0],
            yon_colour: [100, 90, 80, 0],
        },
    };
    let parts: [(&str, Vec<u8>); 6] = [
        (
            SCENE,
            ir.encode(
                &Scene {
                    identity: identity.clone(),
                    level: level.clone(),
                    scenes: Vec::new(),
                },
                dir,
                SCENE,
            )?,
        ),
        (
            PROPS,
            ir.encode(
                &Props {
                    identity: identity.clone(),
                    level: level.clone(),
                    props: Vec::new(),
                    refused: Vec::new(),
                },
                dir,
                PROPS,
            )?,
        ),
        (
            COLLISION,
            ir.encode(
                &Collision {
                    identity: identity.clone(),
                    level: level.clone(),
                    meshes: vec![floor],
                },
                dir,
                COLLISION,
            )?,
        ),
        (
            PLACEMENTS,
            ir.encode(
                &Placements {
                    identity: identity.clone(),
                    level: level.clone(),
                    actors: vec![Placement {
                        object: 1,
                        id: 0,
                        name: "213_Init_Location".into(),
                        script: None,
                        position: spawn,
                        rotation: [0.0; 3],
                        scale: [100.0; 3],
                        model: None,
                        solid: None,
                        movement: None,
                        waypoints: Some(0),
                        activity_state: 4,
                    }],
                    waypoints: vec![vec![Waypoint {
                        name: "wp0".into(),
                        position: spawn,
                        rotation: [0.0; 3],
                        height: 0.0,
                    }]],
                },
                dir,
                PLACEMENTS,
            )?,
        ),
        (
            LIGHTS,
            ir.encode(
                &Lights {
                    identity: identity.clone(),
                    level: level.clone(),
                    directional: [
                        ("_worldLight1", [-45.0, 0.0, 0.0]),
                        ("_worldLight2", [45.0, 180.0, 0.0]),
                        ("_worldLight3", [-45.0, 90.0, 0.0]),
                    ]
                    .map(|(name, rotation)| RigLight {
                        name: name.into(),
                        diffuse: [120, 110, 100, 255],
                        ambient: [30, 30, 30, 255],
                        specular: [90, 90, 90, 255],
                        multiplier: 1.0,
                        position: [0.0; 3],
                        rotation,
                    })
                    .to_vec(),
                    placed: Vec::new(),
                },
                dir,
                LIGHTS,
            )?,
        ),
        (
            ATMOSPHERE,
            ir.encode(
                &Atmosphere {
                    identity: identity.clone(),
                    level: level.clone(),
                    script: "C:/tfb/Content/Levels/Level_E2E/Scripts/Level_Definition_E2E.ai"
                        .into(),
                    zones: vec![zone],
                    start: Some(0),
                },
                dir,
                ATMOSPHERE,
            )?,
        ),
    ];
    for (part, bytes) in parts {
        let rel = ir.level_path(&level, part);
        dir.write_output(&rel, &bytes)?;
        dir.record_level_part(&level, part, vec![rel])?;
    }
    // AH.22's two: the level's entry (AH.13's creature, our numbers) and no
    // creature palettes.
    let rel = guac_extract::levels::script_path(&level);
    dir.write_output(&rel, &guac_formats::script::builder::creature().0)?;
    dir.record_level_part(&level, guac_extract::levels::SCRIPT, vec![rel])?;
    let palettes = guac_ir::palette::LevelPalettes {
        identity,
        level: level.clone(),
        palettes: Vec::new(),
    };
    let rel = ir.level_path(&level, guac_extract::levels::PALETTES);
    dir.write_output(&rel, &ir.encode(&palettes, dir, &rel)?)?;
    dir.record_level_part(&level, guac_extract::levels::PALETTES, vec![rel])?;
    Ok(())
}

/// How many scripts [`write_scripts`] and [`write_level`]'s entries link
/// into (AH.22): the roster builder's portal manager and three Init_Setup
/// scripts, and AH.13's creature as the fighter's and the level's.
pub const E2E_SCRIPTS: usize = 6;

/// The fight's script data as the script stage leaves it (AH.22), both
/// parts recorded: the roster builder's bootstrap, global and init_setup
/// entries and AH.13's creature as the fighter's, built here, and spawn
/// models of none.
fn write_scripts(dir: &mut guac_extract::IdentityDir) -> Result<()> {
    use guac_extract::intros::IrFormat;
    use guac_extract::scripts;

    let d = guac_formats::roster::builder::disc(&guac_formats::roster::builder::sample());
    let (creature, _) = guac_formats::script::builder::creature();
    let mut files = Vec::new();
    for (archive, bytes) in
        scripts::archives()
            .iter()
            .zip([&d.bootstrap, &d.global, &d.init_setup, &creature])
    {
        let rel = scripts::script_path(archive);
        dir.write_output(&rel, bytes)?;
        files.push(rel);
    }
    dir.record_part(scripts::SCRIPTS, files)?;
    let ir = IrFormat::Ron;
    let identity = dir.manifest().identity.clone();
    let mut files = Vec::new();
    for archive in scripts::spawn_archives() {
        let rel = ir.spawn_path(&archive);
        let none = guac_ir::spawn::SpawnModels {
            identity: identity.clone(),
            archive: archive.clone(),
            props: Vec::new(),
            refused: Vec::new(),
        };
        dir.write_output(&rel, &ir.encode(&none, dir, &rel)?)?;
        files.push(rel);
    }
    dir.record_part(scripts::SPAWNS, files)?;
    Ok(())
}

/// The level table every e2e extract holds (AH.2): fifteen levels, as the
/// game has, at thresholds of our own.
pub const E2E_LEVEL_XP: [u32; 15] = [
    0, 500, 1100, 1800, 2600, 3500, 4500, 5600, 6800, 8100, 9500, 11000, 12600, 14300, 16100,
];

/// The gold clamp every e2e extract holds (AH.2), ours.
pub const E2E_GOLD_CAP: u32 = 50_000;

/// The progression tables into the extract as the progression stage leaves
/// them (AH.2): [`E2E_LEVEL_XP`] and [`E2E_GOLD_CAP`], no save params
/// (nothing lays a figure's data out yet), recorded as the part it is.
fn write_progression(dir: &mut guac_extract::IdentityDir) -> Result<()> {
    use guac_extract::intros::IrFormat;
    use guac_ir::progression::Progression;

    let progression = Progression {
        identity: dir.manifest().identity.clone(),
        level_xp: E2E_LEVEL_XP.to_vec(),
        save_params: Vec::new(),
        gold_cap: E2E_GOLD_CAP,
    };
    let ir = IrFormat::Ron;
    let rel = ir.progression_path();
    let bytes = ir.encode(&progression, dir, rel)?;
    dir.write_output(rel, &bytes)?;
    dir.record_part(guac_extract::progression::PART, vec![rel.to_owned()])?;
    Ok(())
}

/// The roster's load file for `toy_type`, as [`write_roster`] names it.
fn load_file(toy_type: u32) -> String {
    format!("{toy_type:03}_e2e")
}

/// [`E2E_WALKER`]'s load file in a walking fixture: the one guacamole walks
/// on Eruptor's numbers (`guacamole::walkers`).
const WALKING_LOAD: &str = "009_Eruptor";

/// What the model stage leaves for [`E2E_MODELLED`] and [`E2E_DAMAGED`]: the
/// first's model and texture, the second's model file cut short, and the
/// rig. Every number made up here.
fn write_models(dir: &mut guac_extract::IdentityDir, walking: bool) -> Result<()> {
    use guac_extract::intros::IrFormat;
    use guac_formats::texture::{Format, Texture};
    use guac_ir::clip::{Clip, RotationTrack, Track, VecSpline, VecTrack};
    use guac_ir::model::{
        Blend, Filter, Glow, Joint, Mask, Material, Params, RenderStates, Sampler, Segment,
        Skeleton, TextureRole, TextureSlot, Transform, Wrap,
    };
    use guac_ir::{Model, Rig, RigLight};

    let ir = IrFormat::Ron;
    let identity = dir.manifest().identity.clone();
    // A 4x4 BC1 block of one colour.
    let ktx2 = Texture {
        format: Format::Bc1RgbaUnorm,
        width: 4,
        height: 4,
        levels: vec![vec![0x1F, 0xF8, 0x1F, 0xF8, 0, 0, 0, 0]],
    }
    .to_ktx2();
    let texture = "textures/e2e-flat.ktx2";
    dir.write_output(texture, &ktx2)?;

    // Two joints a unit apart; the top one sways sideways over a second.
    let joint = |name: &str, parent, y: f32| Joint {
        name: name.into(),
        parent,
        reference: Transform {
            translation: [0.0, y, 0.0],
            ..Transform::IDENTITY
        },
        inverse_bind: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, -y, 0.0, 1.0],
        ],
    };
    let sway = Track {
        joint: 1,
        translation: VecTrack {
            constant: [0.0, 1.0, 0.0],
            spline: Some(VecSpline {
                channels: [true, false, false],
                degree: 1,
                knots: vec![0, 0, 30, 30],
                points: vec![[0.0; 3], [0.25, 0.0, 0.0]],
            }),
        },
        rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
        scale: VecTrack {
            constant: [1.0; 3],
            spline: None,
        },
    };
    let model = Model {
        identity: identity.clone(),
        load_file: load_file(E2E_MODELLED),
        name: "E2e.mdl".into(),
        skeleton: Skeleton {
            joints: vec![joint("foot", None, 0.0), joint("top", Some(0), 1.0)],
        },
        materials: vec![Material {
            name: "e2e".into(),
            pixel: Mask {
                base: 0,
                ignore: 0x2FF,
            },
            vertex: Mask {
                base: 0,
                ignore: 0x1AFF,
            },
            textures: vec![TextureSlot {
                role: TextureRole::Layer1ColorMap,
                texture: texture.into(),
                image: "e2e_flat".into(),
                sampler: Sampler {
                    wrap_u: Wrap::Repeat,
                    wrap_v: Wrap::Repeat,
                    mag: Filter::Linear,
                    min: Filter::Linear,
                    mip: None,
                },
            }],
            params: Params {
                fixed_alpha: [-1.0; 4],
                layer1_extra: [1.0, 1.0, 1.0, 0.25],
                layer2_extra: None,
                layer2_blend: None,
                layer3_blend: None,
                layer4_blend: [0.0, 1.0, 0.0, 0.0],
                layer4_values: [0.0, 1.0, 0.0, 0.0],
                material_tint: [128, 128, 128, 255],
                glow: Glow::None,
            },
            states: RenderStates {
                blend: Blend::Opaque,
                depth_write: true,
                two_sided: false,
                alpha_test: None,
            },
            scrolls: Vec::new(),
            tracks: Vec::new(),
        }],
        segments: vec![Segment {
            material: 0,
            source: 0,
            positions: vec![
                [-0.3, 0.0, 0.0],
                [0.3, 0.0, 0.0],
                [-0.3, 1.0, 0.0],
                [0.3, 1.0, 0.0],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            tangents: Some(vec![[1.0, 0.0, 0.0, 1.0]; 4]),
            uvs: vec![vec![[0.0, 1.0], [1.0, 1.0], [0.0, 0.0], [1.0, 0.0]]],
            colors: None,
            joints: vec![[0, 0, 0, 0], [0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]],
            weights: vec![[255, 0, 0, 0]; 4],
            indices: vec![0, 1, 2, 2, 1, 3],
        }],
        idle: Clip {
            name: "E2e_Idle.ma".into(),
            duration: 1.0,
            frame_duration: 1.0 / 30.0,
            frames: 31,
            tracks: vec![sway],
        },
        attachments: Vec::new(),
        attachments_unsettled: None,
    };
    let rel = ir.model_path(&model.load_file);
    let bytes = ir.encode(&model, dir, &rel)?;
    dir.write_output(&rel, &bytes)?;
    if walking {
        let walker = Model {
            load_file: WALKING_LOAD.into(),
            ..model.clone()
        };
        let rel = ir.model_path(WALKING_LOAD);
        dir.write_output(&rel, &ir.encode(&walker, dir, &rel)?)?;
        let hero = walking_hero(identity.clone());
        let rel = ir.hero_path(WALKING_LOAD);
        dir.write_output(&rel, &ir.encode(&hero, dir, &rel)?)?;
    }
    // Its palette (AH.22): the idle alone, in slot 0.
    let palette = guac_ir::palette::Palette {
        identity: identity.clone(),
        owner: model.load_file.clone(),
        slots: 1,
        entries: vec![guac_ir::palette::PaletteSlot {
            slot: 0,
            name: Some("Idle".into()),
            label: None,
            entry: guac_ir::palette::SlotEntry::Clip {
                additive: false,
                partial: false,
                layered: false,
                motion_extraction: 0,
                clip: model.idle.clone(),
            },
        }],
        refused: Vec::new(),
    };
    let palette_rel = ir.palette_path(&model.load_file);
    dir.write_output(&palette_rel, &ir.encode(&palette, dir, &palette_rel)?)?;
    dir.record_part(guac_extract::models::PALETTES, vec![palette_rel])?;
    // The damaged one: the same file cut off halfway.
    let damaged = ir.model_path(&load_file(E2E_DAMAGED));
    dir.write_output(&damaged, &bytes[..bytes.len() / 2])?;

    let light = |name: &str, rotation| RigLight {
        name: name.into(),
        diffuse: [160, 160, 160, 255],
        ambient: [40, 40, 40, 255],
        specular: [80, 80, 80, 255],
        multiplier: 1.0,
        position: [0.0; 3],
        rotation,
    };
    let rig = Rig {
        identity,
        lights: vec![
            light("_spriteLight1", [-45.0, 0.0, 0.0]),
            light("_spriteLight2", [45.0, 180.0, 0.0]),
            light("_spriteLight3", [-45.0, 90.0, 0.0]),
        ],
    };
    let bytes = ir.encode(&rig, dir, ir.rig_path())?;
    dir.write_output(ir.rig_path(), &bytes)?;
    Ok(())
}

/// Where the walking fixture's hero side holds its top joint along x in the
/// walk, the run and the fall (AG.30): the e2e reads his locomotion's
/// weights off `/api/_test/tv`, and these make them show on the model.
pub const E2E_GAIT_X: [f32; 3] = [0.3, 0.6, 0.9];

/// The walking fixture's hero side (AG.14's record, AG.30 plays it): walk,
/// run and falling clips on the e2e model's skeleton, Eruptor's lengths
/// (1.0667, 0.6667 and 0.8 s), each holding the top joint at its
/// [`E2E_GAIT_X`]; the solid and movement Eruptor's disc ones. Made up.
fn walking_hero(identity: guac_ir::DumpIdentity) -> guac_ir::hero::Hero {
    use guac_ir::actor::{Movement, Shape, Solid};
    use guac_ir::clip::{Clip, RotationTrack, Track, VecTrack};
    use guac_ir::hero::{Hero, Locomotion, SlotClip};
    let slot = |slot: u16, name: &str, duration: f32, x: f32| {
        Some(SlotClip {
            slot,
            label: None,
            clip: Clip {
                name: name.into(),
                duration,
                frame_duration: 1.0 / 30.0,
                frames: (duration * 30.0).round() as u32 + 1,
                tracks: vec![Track {
                    joint: 1,
                    translation: VecTrack {
                        constant: [x, 1.0, 0.0],
                        spline: None,
                    },
                    rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
                    scale: VecTrack {
                        constant: [1.0; 3],
                        spline: None,
                    },
                }],
            },
        })
    };
    Hero {
        identity,
        load_file: WALKING_LOAD.into(),
        actor: WALKING_LOAD.into(),
        scale: [100.0; 3],
        solid: Solid {
            shape: Shape::Sphere,
            solid: true,
            world: true,
            actor_surface: false,
            offset: [0.0; 3],
            rotation: [0.0; 3],
            width: 4.0,
            height: 6.0,
            length: 3.0,
            weight: 10.0,
            rebound: 0.0,
        },
        movement: Movement {
            ground_max_speed: 18.0,
            ground_acceleration: 160.0,
            ground_deceleration: 80.0,
            ground_turn_rate: 360.0,
            air_max_speed: 30.0,
            air_acceleration: 160.0,
            air_deceleration: 20.0,
            air_turn_rate: 360.0,
            gravity_max_speed: 120.0,
            gravity_acceleration: 128.0,
            jump_max_speed: 0.0,
            jump_acceleration: 0.0,
            jump_initial_speed: 0.0,
            motion_type: 0,
            tilts_with_ground: true,
            time_scale: 1.0,
        },
        locomotion: Locomotion {
            walk: slot(1, "E2e_Walk.ma", 1.0667, E2E_GAIT_X[0]),
            jog: None,
            run: slot(3, "E2e_Run.ma", 0.6667, E2E_GAIT_X[1]),
            falling: slot(57, "E2e_Falling.ma", 0.8, E2E_GAIT_X[2]),
            landing: None,
        },
    }
}

/// guacamole's `GET /api/_test/tv/gaits?since=<step>`: the hero's
/// locomotion step by step after `since` (AG.30), `{"now", "steps"}`.
pub async fn gaits_tv(base: &str, since: u64) -> Result<serde_json::Value> {
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/_test/tv/gaits?since={since}"))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("/api/_test/tv/gaits returned {}", resp.status());
    }
    Ok(resp.json().await?)
}

/// Skips the intro the TV is playing, as a key would (`/api/_test/tv/skip`).
pub async fn skip_intro(base: &str) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/tv/skip"))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("/api/_test/tv/skip returned {}", resp.status());
    }
    Ok(())
}

/// Hands guacamole's input router a press as a pad would make it
/// (`/api/_test/tv/press`): `south`, `east`, `mode`, `up`, `down`, `left` or
/// `right`.
pub async fn press_tv(base: &str, button: &str) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/tv/press"))
        .json(&serde_json::json!({ "button": button }))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("/api/_test/tv/press {button} returned {}", resp.status());
    }
    Ok(())
}

/// guacamole's `POST /api/_test/tv/pad`: one act on one of four pads, as a
/// hand would (`{"pad": 0, "connect": {..}}`, `"disconnect": true`,
/// `"stick": [x, y]` or `"press": "start"`; `drive_posted_pads` in
/// `crates/guacamole/src/mirror.rs`).
pub async fn pad_tv(base: &str, body: serde_json::Value) -> Result<()> {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/_test/tv/pad"))
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("/api/_test/tv/pad {body} returned {}", resp.status());
    }
    Ok(())
}

/// guacamole's `/api/_test/tv`: what its TV shows (`TvSnapshot` in
/// `crates/guacamole/src/mirror.rs`).
pub async fn tv(base: &str) -> Result<serde_json::Value> {
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/_test/tv"))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("/api/_test/tv returned {}", resp.status());
    }
    Ok(resp.json().await?)
}

/// Polls the TV every 25 ms until `pred` holds; the snapshot and when it
/// first held. The error carries the last snapshot.
pub async fn wait_for_tv(
    base: &str,
    timeout: Duration,
    pred: impl Fn(&serde_json::Value) -> bool,
) -> Result<(serde_json::Value, Instant)> {
    let deadline = Instant::now() + timeout;
    loop {
        let snap = tv(base).await?;
        if pred(&snap) {
            return Ok((snap, Instant::now()));
        }
        if Instant::now() >= deadline {
            bail!("TV didn't match within {timeout:?}; last: {snap}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// A JS expression that reads every action in the open kebab menu as a
/// finger reaches it, as a JSON string (both WebDriver and the iOS
/// Simulator's `Runtime.evaluate` return a string by value). Each action is
/// scrolled into view only when the menu is a scroll container a finger can
/// drag (`overflow-y` auto or scroll: script can scroll a hidden overflow, a
/// finger can't). Per action its title, its rect then and whether a tap at
/// its centre lands on it; with the viewport (`innerWidth` x `innerHeight`
/// and the visual viewport's height), the panel's rect, whether its content
/// overflows and whether a finger can scroll it (openspec AA.20).
pub const MENU_REACH_JS: &str = "(() => {
  const panel = document.querySelector('.menu-overlay-panel');
  const scroller = panel.querySelector('.menu-overlay-scroll') || panel;
  const box = r => [r.left, r.top, r.right, r.bottom];
  const overflows = scroller.scrollHeight > scroller.clientHeight + 1;
  const scrollable = ['auto', 'scroll'].includes(getComputedStyle(scroller).overflowY);
  const actions = Array.from(panel.querySelectorAll('.menu-action')).map(el => {
    if (scrollable) el.scrollIntoView({ block: 'nearest' });
    const r = el.getBoundingClientRect();
    const hit = document.elementFromPoint((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    return {
      title: el.querySelector('.menu-action-title').textContent.trim(),
      rect: box(r),
      tappable: !!hit && el.contains(hit),
    };
  });
  scroller.scrollTop = 0;
  return JSON.stringify({
    viewport: [window.innerWidth, window.innerHeight],
    visual_height: window.visualViewport ? window.visualViewport.height : null,
    screen: [screen.width, screen.height],
    standalone: !!navigator.standalone,
    panel: box(panel.getBoundingClientRect()),
    overflows,
    scrollable,
    actions,
  });
})()";

/// Convenience for the common "launch Giants" setup.
pub async fn launch_giants(base: &str) -> Result<()> {
    set_game(
        base,
        Some(serde_json::json!({
            "serial": "BLUS30968",
            "display_name": "Skylanders: Giants",
        })),
    )
    .await
}

impl Phone {
    /// Wait for the portal grid to appear (post-GamePicker). Phase 4
    /// renamed `.portal` to `.portal-p4` (PLAN 10.3.6); the old
    /// selector still appears in stale tests and gets fixed file-
    /// by-file.
    pub async fn wait_for_portal(&self, timeout: Duration) -> Result<()> {
        self.wait_for(Locator::Css(".portal-p4"), timeout).await?;
        Ok(())
    }

    /// Text inside a specific slot's name label (1-indexed).
    /// Resolves the slot by its rendered `.p4-slot-index` value rather
    /// than DOM position — PLAY_TEST PLAN 8.3 hides empty slots from
    /// the DOM (`<Show when=!is_empty>`), so a positional `:nth-of-type`
    /// would land on the wrong slot any time the lower-numbered ones
    /// were unoccupied. Reads via `innerText` so `-webkit-text-stroke`
    /// titles surface their text (WebDriver `getElementText` returns
    /// "" for those). Returns `Err` if no slot with that index is
    /// currently visible — the slot is empty (and therefore not in the
    /// DOM), or the portal hasn't rendered yet.
    pub async fn slot_text(&self, slot: u8) -> Result<String> {
        let script = format!(
            r#"
            const target = {slot};
            const slots = document.querySelectorAll('.portal-p4 .p4-slot');
            for (const s of slots) {{
              const idx = s.querySelector('.p4-slot-index');
              if (idx && Number(idx.textContent.trim()) === target) {{
                const label = s.querySelector('.p4-slot-label');
                return label ? label.innerText : '';
              }}
            }}
            return null;
            "#
        );
        let val = self.client.execute(&script, vec![]).await?;
        val.as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("no slot {slot} visible in the portal"))
    }

    /// Tap the Nth slot (1-indexed).
    pub async fn tap_slot(&self, slot: u8) -> Result<()> {
        let slots = self
            .client
            .find_all(Locator::Css(".portal-p4 .p4-slot"))
            .await?;
        let idx = (slot as usize).saturating_sub(1);
        let el = slots
            .get(idx)
            .ok_or_else(|| anyhow!("no slot {slot} to tap"))?;
        el.clone().click().await?;
        Ok(())
    }

    /// Tap the first figure card whose visible name matches.
    /// Note: clicking a card now opens the [`FigureDetail`] panel
    /// (Phase-4 changed the placement model from one-tap-to-place
    /// to two-tap detail + confirm). For end-to-end "place this
    /// figure into a slot" use [`Phone::place_figure_named`].
    pub async fn tap_figure_named(&self, name: &str) -> Result<()> {
        let cards = self.client.find_all(Locator::Css(".fig-card-p4")).await?;
        for card in cards {
            let label = card
                .find(Locator::Css(".fig-name-p4"))
                .await?
                .text()
                .await
                .unwrap_or_default();
            if label == name {
                card.click().await?;
                return Ok(());
            }
        }
        Err(anyhow!("no card named {name:?}"))
    }

    /// End-to-end placement helper: tap the named card → wait for the
    /// FigureDetail panel → click the primary "place" button. Mirrors
    /// the single-user gesture of "I want this figure on the portal."
    /// PLAY_TEST PLAN 8.3 + the toy-box-lid UX moved placement off the
    /// portal-tap interaction; this helper hides the two-step dance
    /// from each test.
    pub async fn place_figure_named(&self, name: &str) -> Result<()> {
        self.tap_figure_named(name).await?;
        let place = self
            .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
            .await?;
        place.click().await?;
        Ok(())
    }

    /// Place the first figure card the browser shows.
    /// Useful for tests that don't care which figure goes onto the
    /// portal, only that *something* gets placed (back-pressure
    /// regressions, ws-reconnect-state-survives, etc.). Same two-step
    /// shape as `place_figure_named`.
    pub async fn place_first_figure(&self) -> Result<()> {
        let card = self.client.find(Locator::Css(".fig-card-p4")).await?;
        card.click().await?;
        let place = self
            .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
            .await?;
        place.click().await?;
        Ok(())
    }

    /// Click the remove button on a portal slot (tap the loaded slot
    /// to arm it, then tap the remove action that appears). Caller
    /// supplies the slot index; if the slot isn't currently visible
    /// (empty, hence not in the DOM per PLAY_TEST PLAN 8.3) returns
    /// `Err`.
    pub async fn remove_slot(&self, slot: u8) -> Result<()> {
        // Find the .p4-slot whose .p4-slot-index reads `slot`.
        let script = format!(
            r#"
            const target = {slot};
            const slots = document.querySelectorAll('.portal-p4 .p4-slot');
            for (const s of slots) {{
              const idx = s.querySelector('.p4-slot-index');
              if (idx && Number(idx.textContent.trim()) === target) {{
                s.click();
                return true;
              }}
            }}
            return false;
            "#
        );
        let armed = self.client.execute(&script, vec![]).await?;
        if !armed.as_bool().unwrap_or(false) {
            return Err(anyhow!("slot {slot} not visible — already empty?"));
        }
        // Wait for the remove action to surface inside that slot.
        let remove = self
            .wait_for(
                Locator::Css(".portal-p4 .p4-slot .p4-slot-action--remove"),
                Duration::from_secs(2),
            )
            .await?;
        remove.click().await?;
        Ok(())
    }

    /// Wait until slot `n` no longer appears in the portal — i.e. it
    /// went empty (`<Show when=!is_empty>` removes it from the DOM).
    /// Mirror of [`Phone::slot_text`]'s find-by-index logic.
    pub async fn wait_for_slot_empty(&self, slot: u8, timeout: Duration) -> Result<()> {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if self.slot_text(slot).await.is_err() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(anyhow!(
            "slot {slot} still visible after {timeout:?} — expected to vanish"
        ))
    }

    /// Tap the toy-box lid grabber so the lid opens (Closed → Compact).
    /// Idempotent: if the lid is already open (no `.closed` modifier
    /// on `.lid-open-p4`), no-op. Required before [`Phone::open_search`]
    /// since the search toggle button is part of the lid header.
    /// Waits up to 2 s for the grabber to appear — the Browser screen
    /// briefly remounts during navigation transitions (place → portal,
    /// remove → empty), and a too-eager find races the re-render.
    pub async fn open_toy_box_lid(&self) -> Result<()> {
        // `apply_tap` cycles Closed → Compact → Expanded → Closed; we
        // only want to advance from Closed → Compact, so check first.
        let already_open = self
            .client
            .find(Locator::Css(".lid-open-p4:not(.closed)"))
            .await
            .is_ok();
        if already_open {
            return Ok(());
        }
        let grabber = self
            .wait_for(Locator::Css(".lid-grabber-p4"), Duration::from_secs(2))
            .await?;
        grabber.click().await?;
        Ok(())
    }

    /// Tap the lid grabber until the toy box is CLOSED — the portal grid then
    /// owns the viewport. Inverse of [`Phone::open_toy_box_lid`], and the only
    /// place-independent way to REVEAL the portal: otherwise the lid only
    /// closes on a successful place's `on_placed` (browser.rs), which a remove /
    /// ownership beat can't rely on. `apply_tap` cycles Closed → Compact →
    /// Expanded → Closed, so the close takes one tap from Expanded and two from
    /// Compact — loop + recheck rather than assume the entry state. Idempotent:
    /// already-closed is a no-op.
    pub async fn close_toy_box_lid(&self) -> Result<()> {
        for _ in 0..4 {
            if self
                .client
                .find(Locator::Css(".lid-open-p4.closed"))
                .await
                .is_ok()
            {
                return Ok(());
            }
            if let Ok(grabber) = self
                .wait_for(Locator::Css(".lid-grabber-p4"), Duration::from_secs(2))
                .await
            {
                grabber.click().await.ok();
            }
            tokio::time::sleep(Duration::from_millis(350)).await;
        }
        if self
            .client
            .find(Locator::Css(".lid-open-p4.closed"))
            .await
            .is_ok()
        {
            Ok(())
        } else {
            bail!("toy-box lid never reached the closed state after 4 grabber taps")
        }
    }

    /// Surface the search input so [`Phone::search`] can type into it.
    /// Two-step: open the lid (if not already open), then click
    /// `.search-toggle-p4` if `.search-input-p4` isn't already
    /// rendered. Idempotent.
    pub async fn open_search(&self) -> Result<()> {
        self.open_toy_box_lid().await?;
        // search-input-p4 lives inside `.search-expanded-p4`, which is
        // gated by `box_state == Expanded`. If the input is already
        // there we're done; otherwise toggle.
        if self
            .client
            .find(Locator::Css(".search-input-p4"))
            .await
            .is_ok()
        {
            return Ok(());
        }
        let toggle = self.client.find(Locator::Css(".search-toggle-p4")).await?;
        toggle.click().await?;
        // Wait for the input to actually mount.
        self.wait_for(Locator::Css(".search-input-p4"), Duration::from_secs(2))
            .await?;
        Ok(())
    }

    /// Opens the toy box's search, replaces its query with `name` and waits
    /// up to 10 s for the grid to filter: a card named `name` shows and every
    /// card's name contains it (case-insensitive). Tapping after a fixed beat
    /// could catch a grid that hadn't filtered yet on a loaded machine.
    pub async fn search_named(&self, name: &str) -> Result<()> {
        self.open_search().await?;
        let input = self.client.find(Locator::Css(".search-input-p4")).await?;
        input.clear().await?;
        input.send_keys(name).await?;
        let want = name.to_lowercase();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let read = self
                .client
                .execute(
                    "return Array.from(document.querySelectorAll('.fig-card-p4 .fig-name-p4'))
                       .map(e => e.textContent.trim());",
                    vec![],
                )
                .await?;
            let names: Vec<String> = serde_json::from_value(read)?;
            if names.iter().any(|n| n == name)
                && names.iter().all(|n| n.to_lowercase().contains(&want))
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "the toy box never filtered to {name:?}; it shows {names:?}"
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Filter the browser by typing into the search box. Caller is
    /// responsible for ensuring the search input is visible — call
    /// [`Phone::open_search`] first if you've just landed on the
    /// portal screen (lid starts Closed).
    pub async fn search(&self, q: &str) -> Result<()> {
        let input = self.client.find(Locator::Css(".search-input-p4")).await?;
        input.send_keys(q).await?;
        Ok(())
    }

    /// Read `element.innerText` for the first match of `selector`.
    /// Bypasses WebDriver's `getElementText`, which returns "" for
    /// elements styled with `-webkit-text-stroke` (the gold-stroked
    /// `<DisplayHeading>` titles all over the SPA hit this). Returns
    /// `None` if no element matches. PLAN 10.3.6.
    pub async fn inner_text(&self, selector: &str) -> Result<Option<String>> {
        let script = format!(
            "var el = document.querySelector({}); return el ? el.innerText : null;",
            serde_json::to_string(selector)?,
        );
        let val = self.client.execute(&script, vec![]).await?;
        Ok(val.as_str().map(str::to_string))
    }

    /// Count currently-rendered toasts.
    pub async fn toast_count(&self) -> Result<usize> {
        // `.toast` elements can briefly appear in the DOM with empty text
        // during Leptos's `<For>` transitions (the retain() on timeout vs.
        // the next render). Filter those out so tests measure only user-
        // visible toasts.
        let mut n = 0;
        for t in self.client.find_all(Locator::Css(".toast")).await? {
            if !t.text().await.unwrap_or_default().is_empty() {
                n += 1;
            }
        }
        Ok(n)
    }

    pub async fn last_toast_text(&self) -> Result<Option<String>> {
        let toasts = self.client.find_all(Locator::Css(".toast")).await?;
        Ok(match toasts.last() {
            Some(t) => Some(t.text().await.unwrap_or_default()),
            None => None,
        })
    }

    /// Poll until `predicate` returns true or timeout expires.
    /// Read the session id the server assigned this phone. Populated by the
    /// phone's `ws.rs` on receipt of `Event::Welcome`, exposed in the DOM as
    /// `<body data-session-id="..">`. Returns `None` until the WS handshake
    /// completes — callers typically `wait_until` it's non-None before
    /// using it.
    pub async fn session_id(&self) -> Result<Option<u64>> {
        let body = self.client.find(Locator::Css("body")).await?;
        let attr = body.attr("data-session-id").await?.unwrap_or_default();
        Ok(attr.parse::<u64>().ok())
    }

    pub async fn wait_until<F, Fut>(&self, timeout: Duration, mut predicate: F) -> Result<()>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if predicate().await {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(anyhow!("wait_until timed out after {timeout:?}"))
    }

    /// [`MENU_REACH_JS`] on this page (openspec AA.20).
    pub async fn menu_reach(&self) -> Result<serde_json::Value> {
        let read = self
            .client
            .execute(&format!("return {MENU_REACH_JS};"), vec![])
            .await?;
        Ok(serde_json::from_str(
            read.as_str().context("a JSON string")?,
        )?)
    }

    /// The open kebab menu's action titles, read until `done` holds and
    /// MANAGE PROFILES shows, for up to 4 s (the app's `/api/status` can land
    /// after the menu's first draw). Returns the last read for the caller to
    /// assert on.
    pub async fn menu_actions_until(
        &self,
        done: impl Fn(&[String]) -> bool,
    ) -> Result<Vec<String>> {
        self.wait_for(Locator::Css(".menu-overlay-panel"), Duration::from_secs(4))
            .await?;
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let read = self
                .client
                .execute(
                    "return Array.from(document.querySelectorAll('.menu-overlay-panel .menu-action-title'))
                       .map(e => e.textContent.trim());",
                    vec![],
                )
                .await?;
            let titles: Vec<String> = serde_json::from_value(read)?;
            let drawn = titles.iter().any(|t| t == "MANAGE PROFILES");
            if (drawn && done(&titles)) || Instant::now() >= deadline {
                return Ok(titles);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[cfg(test)]
mod chrome_version_tests {
    use super::chrome_major_version;

    #[test]
    fn reads_the_major_from_chromes_version_line() {
        assert_eq!(
            chrome_major_version("Google Chrome 155.0.8059.85 \n"),
            Some(155)
        );
        assert_eq!(
            chrome_major_version("Google Chrome for Testing 147.0.7727.138"),
            Some(147)
        );
        assert_eq!(chrome_major_version(""), None);
        assert_eq!(chrome_major_version("Google Chrome"), None);
    }
}
