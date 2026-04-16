//! Helpers shared across the e2e regression tests.
//!
//! [`TestServer::spawn`] builds and launches the server with
//! `--features test-hooks`, scrapes the "serving on http://…" line from its
//! stdout to discover the URL, and hands back an owning handle whose `Drop`
//! kills the child. [`Phone`] wraps a fantoccini WebDriver client with
//! convenience selectors.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
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
        let repo = repo_root()?;
        // The phone SPA renders one `.card` per indexed figure, so the tests
        // need a real pack — `tools/inventory` has no `.sky` files. Use the
        // dev pack at the path documented in CLAUDE.md, overridable with
        // `SKYLANDER_PACK_ROOT`.
        let default_pack = PathBuf::from(r"C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3");
        let firmware = std::env::var("SKYLANDER_PACK_ROOT")
            .map(PathBuf::from)
            .unwrap_or(default_pack);
        if !firmware.is_dir() {
            bail!(
                "firmware pack not found at {} — set SKYLANDER_PACK_ROOT to your local pack",
                firmware.display()
            );
        }

        let phone_dist = repo.join("phone").join("dist");
        if !phone_dist.join("index.html").is_file() {
            bail!(
                "phone SPA not built — run `cd phone && trunk build` first (looking in {})",
                phone_dist.display()
            );
        }

        let tmp = tempfile::tempdir().context("create temp dir")?;
        let port = pick_free_port()?;
        let env = format!(
            "RPCS3_EXE={rpcs3}\nFIRMWARE_PACK_ROOT={pack}\nBIND_PORT={port}\nSKYLANDER_PORTAL_DRIVER=mock\nPHONE_DIST={phone}\n",
            rpcs3 = repo.join("crates/e2e-tests/src/lib.rs").display(), // any real file — mock doesn't launch
            pack = firmware.display(),
            port = port,
            phone = phone_dist.display(),
        );
        std::fs::write(tmp.path().join(".env.dev"), env)?;

        // Build once up front so subsequent spawns are fast; re-invoking
        // cargo run also rebuilds incrementally if source changed.
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
            .stderr(Stdio::piped());

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
        if std::net::TcpStream::connect_timeout(
            &addr.parse().unwrap(),
            Duration::from_millis(200),
        )
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

fn spawn_reader(tag: &'static str, stream: impl std::io::Read + Send + 'static, tx: mpsc::Sender<String>) {
    thread::spawn(move || {
        let reader = BufReader::new(stream);
        for line in reader.lines().flatten() {
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
        "chromedriver not found — set $CHROMEDRIVER, add it to PATH, or install via \
         `winget install --id=Chromium.ChromeDriver`"
    ))
}

fn pick_free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
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

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
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

    pub async fn wait_for(&self, loc: Locator<'_>, timeout: Duration) -> Result<fantoccini::elements::Element> {
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
        bail!("inject_load returned {}: {}", resp.status(), resp.text().await?);
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
        bail!("set_game returned {}: {}", resp.status(), resp.text().await?);
    }
    Ok(())
}

/// Inject a profile via the test-hook. Returns the new profile id.
pub async fn inject_profile(
    base: &str,
    name: &str,
    pin: &str,
    color: &str,
) -> Result<String> {
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
    /// Wait for the portal grid to appear (post-GamePicker).
    pub async fn wait_for_portal(&self, timeout: Duration) -> Result<()> {
        self.wait_for(Locator::Css(".portal"), timeout).await?;
        Ok(())
    }

    /// Text inside a specific slot's name label (1-indexed).
    pub async fn slot_text(&self, slot: u8) -> Result<String> {
        let slots = self
            .client
            .find_all(Locator::Css(".portal .slot .slot-name"))
            .await?;
        let idx = (slot as usize).saturating_sub(1);
        let el = slots
            .get(idx)
            .ok_or_else(|| anyhow!("no slot {slot} found"))?;
        Ok(el.text().await.unwrap_or_default())
    }

    /// Tap the Nth slot (1-indexed).
    pub async fn tap_slot(&self, slot: u8) -> Result<()> {
        let slots = self.client.find_all(Locator::Css(".portal .slot")).await?;
        let idx = (slot as usize).saturating_sub(1);
        let el = slots
            .get(idx)
            .ok_or_else(|| anyhow!("no slot {slot} to tap"))?;
        el.clone().click().await?;
        Ok(())
    }

    /// Tap the first figure card whose visible name matches.
    pub async fn tap_figure_named(&self, name: &str) -> Result<()> {
        let cards = self.client.find_all(Locator::Css(".card")).await?;
        for card in cards {
            let label = card
                .find(Locator::Css(".card-name"))
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

    /// Filter the browser by typing into the search box.
    pub async fn search(&self, q: &str) -> Result<()> {
        let input = self.client.find(Locator::Css(".search")).await?;
        input.send_keys(q).await?;
        Ok(())
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
