//! `ios-inspect` — CLI for driving the iOS Simulator + Safari Web Inspector.
//!
//! Workflow: `boot` once at the start of a session; then iterate with
//! `open`, `eval`, `computed-style`, `dump-dom`, `screenshot`. `shutdown`
//! tears everything down.
//!
//! Lifecycle state (proxy PID, device UDID, webinspectord_sim socket path)
//! lives in `/tmp/ios-inspect-state.json` so subsequent invocations pick up
//! where `boot` left off.

mod protocol;
mod proxy;
mod simulator;
mod state;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ios-inspect", about = "Drive the iOS Simulator + Safari Web Inspector")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Boot a simulator and start ios-webkit-debug-proxy pointed at it.
    Boot {
        /// Device name to boot (e.g. "iPhone 17 Pro"). Defaults to the most
        /// recent Dynamic-Island iPhone available.
        #[arg(long)]
        device: Option<String>,
    },
    /// Open a URL in the booted simulator's Safari.
    Open {
        url: String,
    },
    /// Evaluate a JS expression in the current page. Prints the result
    /// value (or the full protocol response if --raw).
    Eval {
        expression: String,
        #[arg(long)]
        raw: bool,
    },
    /// Dump computed CSS styles for the first element matching `selector`.
    /// Prints every resolved property as JSON unless --filter is given.
    ComputedStyle {
        selector: String,
        /// Comma-separated list of property names to include.
        #[arg(long)]
        filter: Option<String>,
    },
    /// Dump the DOM tree. With --selector, dumps just the matching subtree.
    DumpDom {
        #[arg(long)]
        selector: Option<String>,
        /// How deep to walk (default 8; -1 = unlimited).
        #[arg(long, default_value_t = 8)]
        depth: i32,
    },
    /// Save a PNG screenshot. Default path uses `xcrun simctl io` (full
    /// device frame). `--web-only` uses the inspector's Page.snapshotRect
    /// (web content only, no sim chrome).
    Screenshot {
        #[arg(short, long)]
        output: std::path::PathBuf,
        #[arg(long)]
        web_only: bool,
    },
    /// List Safari tabs currently visible to the proxy.
    Tabs,
    /// Tear down: kill the proxy and shut down the simulator.
    Shutdown,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Boot { device } => cmd_boot(device).await,
        Cmd::Open { url } => cmd_open(&url).await,
        Cmd::Eval { expression, raw } => cmd_eval(&expression, raw).await,
        Cmd::ComputedStyle { selector, filter } => cmd_computed_style(&selector, filter).await,
        Cmd::DumpDom { selector, depth } => cmd_dump_dom(selector.as_deref(), depth).await,
        Cmd::Screenshot { output, web_only } => cmd_screenshot(&output, web_only).await,
        Cmd::Tabs => cmd_tabs().await,
        Cmd::Shutdown => cmd_shutdown().await,
    }
}

async fn cmd_boot(device: Option<String>) -> Result<()> {
    // If we already booted, noop and re-print the state.
    if let Some(s) = state::load()? {
        if proxy::is_running(s.proxy_pid) {
            println!("already booted: {}", s.summary());
            return Ok(());
        }
        // Proxy died but state file lingered — clean up before re-bootstrapping.
        state::clear()?;
    }

    let dev = simulator::pick_device(device.as_deref())
        .context("pick simulator device")?;
    println!("device: {} ({}, {})", dev.name, dev.udid, dev.runtime);

    simulator::boot_if_needed(&dev.udid).await?;
    simulator::launch_simulator_app().await?;

    // webinspectord_sim socket is created when the sim boots; may take a moment.
    let socket = proxy::wait_for_sim_socket(std::time::Duration::from_secs(15))
        .await
        .context("locate webinspectord_sim.socket")?;
    println!("socket: {}", socket.display());

    let pid = proxy::spawn(&socket).await?;
    println!("proxy pid: {pid}");

    // Wait for the proxy to accept HTTP queries.
    proxy::wait_for_ready(std::time::Duration::from_secs(5)).await?;

    state::save(&state::State {
        udid: dev.udid.clone(),
        device_name: dev.name.clone(),
        runtime: dev.runtime.clone(),
        socket_path: socket,
        proxy_pid: pid,
    })?;

    println!("ready. `ios-inspect open <url>` to load a page.");
    Ok(())
}

async fn cmd_open(url: &str) -> Result<()> {
    simulator::openurl(url).await?;
    println!("opened {url} in sim Safari");
    Ok(())
}

/// Verify the proxy is pointed at the live webinspectord_sim socket. If
/// the socket path has drifted (simulatord restarted its inspector daemon)
/// or the proxy process is gone, spawn a fresh one transparently and update
/// the state file. Called from every non-boot, non-shutdown command so the
/// user doesn't have to track proxy health.
async fn ensure_proxy_healthy() -> Result<()> {
    let Some(mut s) = state::load()? else {
        bail!("not booted — run `ios-inspect boot` first");
    };
    let live = proxy::find_live_sim_socket().await?;
    let proxy_alive = proxy::is_running(s.proxy_pid);
    let socket_matches = live.as_deref() == Some(s.socket_path.as_path());
    if proxy_alive && socket_matches {
        return Ok(());
    }
    // Restart: kill the stale proxy (if any), pick up the live socket, spawn fresh.
    if proxy_alive {
        let _ = proxy::kill(s.proxy_pid);
    }
    let socket = live
        .ok_or_else(|| anyhow::anyhow!("no live webinspectord_sim socket — is the sim still booted?"))?;
    let pid = proxy::spawn(&socket).await?;
    proxy::wait_for_ready(std::time::Duration::from_secs(5)).await?;
    s.proxy_pid = pid;
    s.socket_path = socket;
    state::save(&s)?;
    Ok(())
}

async fn cmd_eval(expression: &str, raw: bool) -> Result<()> {
    ensure_proxy_healthy().await?;
    let tab = proxy::pick_current_tab().await?;
    let mut sess = protocol::Session::connect(&tab.ws_url).await?;
    let result = sess.runtime_evaluate(expression).await?;
    if raw {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        // Unwrap result.result.value if present; else pretty-print the object.
        if let Some(v) = result.pointer("/result/value") {
            match v {
                serde_json::Value::String(s) => println!("{s}"),
                other => println!("{other}"),
            }
        } else {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }
    Ok(())
}

async fn cmd_computed_style(selector: &str, filter: Option<String>) -> Result<()> {
    ensure_proxy_healthy().await?;
    let tab = proxy::pick_current_tab().await?;
    let mut sess = protocol::Session::connect(&tab.ws_url).await?;
    let node_id = sess.query_selector(selector).await?
        .ok_or_else(|| anyhow::anyhow!("no element matches {selector:?}"))?;
    let style = sess.computed_style(node_id).await?;
    let filter_set: Option<std::collections::HashSet<String>> = filter
        .map(|f| f.split(',').map(|s| s.trim().to_string()).collect());
    let mut out = serde_json::Map::new();
    for prop in style {
        if let Some(ref set) = filter_set {
            if !set.contains(&prop.name) {
                continue;
            }
        }
        out.insert(prop.name, serde_json::Value::String(prop.value));
    }
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

async fn cmd_dump_dom(selector: Option<&str>, depth: i32) -> Result<()> {
    ensure_proxy_healthy().await?;
    let tab = proxy::pick_current_tab().await?;
    let mut sess = protocol::Session::connect(&tab.ws_url).await?;
    let tree = sess.dump_dom(selector, depth).await?;
    // With a selector, the protocol returns {"outerHTML": "..."} — unwrap the
    // string so the caller sees real HTML, not a JSON-escaped one-line blob.
    if let Some(html) = tree.get("outerHTML").and_then(serde_json::Value::as_str) {
        println!("{html}");
    } else {
        println!("{}", serde_json::to_string_pretty(&tree)?);
    }
    Ok(())
}

async fn cmd_screenshot(output: &std::path::Path, web_only: bool) -> Result<()> {
    if web_only {
        ensure_proxy_healthy().await?;
        let tab = proxy::pick_current_tab().await?;
        let mut sess = protocol::Session::connect(&tab.ws_url).await?;
        let (w, h) = sess.viewport_size().await?;
        let png = sess.snapshot_rect(0.0, 0.0, w, h).await?;
        tokio::fs::write(output, &png).await?;
        println!("wrote {} ({w}x{h}, {} bytes)", output.display(), png.len());
    } else {
        simulator::screenshot(output).await?;
        println!("wrote {} (via simctl)", output.display());
    }
    Ok(())
}

async fn cmd_tabs() -> Result<()> {
    ensure_proxy_healthy().await?;
    let tabs = proxy::list_tabs().await?;
    if tabs.is_empty() {
        println!("(no tabs — open a page with `ios-inspect open <url>`)");
        return Ok(());
    }
    for t in tabs {
        println!("{:>3}  {}  ({})", t.page_num, t.url, t.title);
    }
    Ok(())
}

async fn cmd_shutdown() -> Result<()> {
    let Some(s) = state::load()? else {
        println!("nothing to shut down (no state file)");
        return Ok(());
    };
    if proxy::is_running(s.proxy_pid) {
        proxy::kill(s.proxy_pid)?;
        println!("killed proxy (pid {})", s.proxy_pid);
    }
    simulator::shutdown(&s.udid).await?;
    println!("shut down sim {}", s.device_name);
    state::clear()?;
    Ok(())
}

