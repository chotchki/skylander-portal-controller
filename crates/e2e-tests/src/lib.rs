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

impl TestServer {
    pub fn spawn() -> Result<Self> {
        Self::spawn_with_env_lines("")
    }

    /// Like [`spawn`] but appends `extra_env` to the generated `.env.dev`
    /// (newline-terminated lines, e.g. `"WINDOW_MODE=desktop\n"`). Used by
    /// the play-through recorder (PLAN 15.11), which needs the launcher in
    /// Phase-20 windowed Desktop mode.
    pub fn spawn_with_env_lines(extra_env: &str) -> Result<Self> {
        let repo = repo_root()?;
        let firmware = resolve_firmware_pack(&repo)?;

        let phone_dist = repo.join("phone").join("dist");
        if !phone_dist.join("index.html").is_file() {
            bail!(
                "phone SPA not built — run `cd phone && trunk build` first (looking in {})",
                phone_dist.display()
            );
        }

        let tmp = tempfile::tempdir().context("create temp dir")?;
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

        // Build once up front so subsequent spawns are fast; re-invoking
        // cargo run also rebuilds incrementally if source changed.
        let mut cmd = Command::new("cargo");
        cmd.current_dir(tmp.path())
            .env("CARGO_MANIFEST_DIR", &repo)
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
    pub fn spawn_ipc_savestate() -> Result<Self> {
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
        let savestate = std::env::var("SKYLANDER_BOOT_SAVESTATE")
            .context("SKYLANDER_BOOT_SAVESTATE env var required (path to the .SAVESTAT(.zst))")?;

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
        let port = pick_free_port()?;
        let env = format!(
            // WINDOW_MODE=desktop so the launcher is windowed and 20.4 fits the
            // RPCS3 game window to it — both visible in the capture (TV mode would
            // fullscreen the launcher over RPCS3, hiding the in-game portal).
            "RPCS3_EXE={rpcs3}\nRPCS3_CONFIG_DIR={cfg}\nFIRMWARE_PACK_ROOT={pack}\nBIND_PORT={port}\nSKYLANDER_PORTAL_DRIVER=ipc\nSKYLANDER_BOOT_SAVESTATE={ss}\nWINDOW_MODE=desktop\nPHONE_DIST={phone}\n",
            rpcs3 = rpcs3_exe.display(),
            cfg = config_dir,
            pack = firmware.display(),
            port = port,
            ss = savestate,
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
/// Resolution order for the chromedriver binary:
///   1. `$CHROMEDRIVER` env var (explicit override).
///   2. `chromedriver` on PATH.
///   3. The winget install location
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

/// Inject a sequence of MockOutcomes into the server's upcoming `load` calls.
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
    /// Skips the synthetic `.scan-new` "SCAN NEW" sentinel card so
    /// callers can search by figure name without false positives.
    /// Note: clicking a card now opens the [`FigureDetail`] panel
    /// (Phase-4 changed the placement model from one-tap-to-place
    /// to two-tap detail + confirm). For end-to-end "place this
    /// figure into a slot" use [`Phone::place_figure_named`].
    pub async fn tap_figure_named(&self, name: &str) -> Result<()> {
        let cards = self
            .client
            .find_all(Locator::Css(".fig-card-p4:not(.scan-new)"))
            .await?;
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

    /// Place the first non-`.scan-new` figure card the browser shows.
    /// Useful for tests that don't care which figure goes onto the
    /// portal, only that *something* gets placed (back-pressure
    /// regressions, ws-reconnect-state-survives, etc.). Same two-step
    /// shape as `place_figure_named`.
    pub async fn place_first_figure(&self) -> Result<()> {
        let card = self
            .client
            .find(Locator::Css(".fig-card-p4:not(.scan-new)"))
            .await?;
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
}
