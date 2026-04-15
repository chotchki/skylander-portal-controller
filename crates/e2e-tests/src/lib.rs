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

const CHROMEDRIVER_URL: &str = "http://localhost:4444";

/// A running server instance. The child is killed on drop.
pub struct TestServer {
    pub url: String,
    _child: ChildGuard,
    _tmpdir: TempDir,
}

impl TestServer {
    pub fn spawn() -> Result<Self> {
        let repo = repo_root()?;
        let firmware = repo.join("tools").join("inventory");
        // We don't actually need the firmware pack for these tests since the
        // mock driver ignores the file contents. Point FIRMWARE_PACK_ROOT at
        // the repo itself so the indexer finds at least the pack if available,
        // otherwise just anything. Override to the real pack via env.
        let firmware = std::env::var("SKYLANDER_PACK_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| firmware);

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
        Ok(Self {
            url,
            _child: guard,
            _tmpdir: tmp,
        })
    }
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
    pub async fn new(url: &str) -> Result<Self> {
        let caps = serde_json::from_str::<serde_json::Value>(
            r#"{"goog:chromeOptions": {"args": ["--headless=new", "--no-sandbox", "--disable-gpu", "--window-size=420,900"]}}"#,
        )?;
        let client = ClientBuilder::native()
            .capabilities(caps.as_object().unwrap().clone())
            .connect(CHROMEDRIVER_URL)
            .await
            .with_context(|| {
                format!("connect to chromedriver at {CHROMEDRIVER_URL} (is it running?)")
            })?;
        client.goto(url).await?;
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
        Ok(self.client.find_all(Locator::Css(".toast")).await?.len())
    }

    pub async fn last_toast_text(&self) -> Result<Option<String>> {
        let toasts = self.client.find_all(Locator::Css(".toast")).await?;
        Ok(match toasts.last() {
            Some(t) => Some(t.text().await.unwrap_or_default()),
            None => None,
        })
    }

    /// Poll until `predicate` returns true or timeout expires.
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
