//! `ios-inspect` — CLI for driving the iOS Simulator + Safari Web Inspector.
//!
//! Workflow: `boot` once at the start of a session (one or more
//! `--device` flags); then iterate with `open`, `eval`,
//! `computed-style`, `dump-dom`, `screenshot`. `shutdown` tears
//! everything down.
//!
//! Multi-device (PLAN 10.2): each booted simulator gets its own proxy
//! on a dedicated port pair. Per-command `--device <name|udid>`
//! filters to one device; without the flag, commands fan out to all
//! booted devices and prefix output with the device label.
//!
//! Lifecycle state (per-device proxy PID, UDID, sockets, ports) lives
//! in `/tmp/ios-inspect-state.json` so subsequent invocations pick up
//! where `boot` left off.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use ios_inspect::state::{self, DeviceState};
use ios_inspect::{protocol, proxy, simulator};

#[derive(Parser)]
#[command(name = "ios-inspect", about = "Drive the iOS Simulator + Safari Web Inspector")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Boot one or more simulators and start an `ios_webkit_debug_proxy`
    /// per device. Repeat `--device` to boot multiple. With no flag,
    /// auto-picks the most recent Dynamic-Island iPhone.
    Boot {
        /// Device name(s) to boot (e.g. "iPhone 17 Pro", "iPad Pro").
        /// Substring + case-insensitive match against `xcrun simctl
        /// list`. Repeatable.
        #[arg(long)]
        device: Vec<String>,
    },
    /// Open a URL in Safari on the booted simulator(s). Without
    /// `--device`, fans out to all booted sims (the typical "load this
    /// SPA on both iPad + iPhone" flow).
    Open {
        url: String,
        /// Limit to one specific device.
        #[arg(long)]
        device: Option<String>,
    },
    /// Evaluate a JS expression against the current page. Without
    /// `--device`, fans out to all booted sims.
    Eval {
        expression: String,
        #[arg(long)]
        raw: bool,
        #[arg(long)]
        device: Option<String>,
    },
    /// Dump computed CSS styles for the first element matching
    /// `selector`. Without `--device`, fans out to all booted sims.
    ComputedStyle {
        selector: String,
        /// Comma-separated list of property names to include.
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        device: Option<String>,
    },
    /// Dump the DOM tree. With `--selector`, dumps just the matching
    /// subtree. Without `--device`, fans out to all booted sims.
    DumpDom {
        #[arg(long)]
        selector: Option<String>,
        /// How deep to walk (default 8; -1 = unlimited).
        #[arg(long, default_value_t = 8)]
        depth: i32,
        #[arg(long)]
        device: Option<String>,
    },
    /// Save a PNG screenshot. With one device, `--output` writes a
    /// single file. With multiple devices and no `--device` filter,
    /// `--output` is treated as a directory and per-device PNGs
    /// (`<device-label>.png`) are written into it.
    Screenshot {
        #[arg(short, long)]
        output: std::path::PathBuf,
        #[arg(long)]
        web_only: bool,
        #[arg(long)]
        device: Option<String>,
    },
    /// List Safari tabs visible to each booted device's proxy.
    Tabs {
        #[arg(long)]
        device: Option<String>,
    },
    /// Tear down every booted device + its proxy. Idempotent.
    Shutdown,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Boot { device } => cmd_boot(device).await,
        Cmd::Open { url, device } => cmd_open(&url, device.as_deref()).await,
        Cmd::Eval { expression, raw, device } => {
            cmd_eval(&expression, raw, device.as_deref()).await
        }
        Cmd::ComputedStyle { selector, filter, device } => {
            cmd_computed_style(&selector, filter, device.as_deref()).await
        }
        Cmd::DumpDom { selector, depth, device } => {
            cmd_dump_dom(selector.as_deref(), depth, device.as_deref()).await
        }
        Cmd::Screenshot { output, web_only, device } => {
            cmd_screenshot(&output, web_only, device.as_deref()).await
        }
        Cmd::Tabs { device } => cmd_tabs(device.as_deref()).await,
        Cmd::Shutdown => cmd_shutdown().await,
    }
}

async fn cmd_boot(device_args: Vec<String>) -> Result<()> {
    // If a session already exists, top up rather than replacing — let
    // the user add a second device to a single-device session by re-
    // running `boot --device <new>`.
    let mut state = state::load()?.unwrap_or(state::State { devices: Vec::new() });

    // Reap any state entries whose proxy died since the last boot — a
    // crashed proxy shouldn't leave its UDID + port pair "claimed".
    state.devices.retain(|d| {
        if proxy::is_running(d.proxy_pid) {
            true
        } else {
            eprintln!(
                "(reaping stale state for {}: proxy pid {} no longer running)",
                d.device_name, d.proxy_pid,
            );
            false
        }
    });

    let requested = simulator::pick_devices(&device_args)?;

    // Filter out devices that are already in state (so a repeat boot
    // is a no-op for them, not a re-spawn).
    let to_boot: Vec<_> = requested
        .into_iter()
        .filter(|d| !state.devices.iter().any(|ds| ds.udid == d.udid))
        .collect();

    if to_boot.is_empty() {
        println!("nothing new to boot. current session:");
        println!("{}", state.summary());
        return Ok(());
    }

    simulator::launch_simulator_app().await?;

    for dev in to_boot {
        println!("device: {} ({}, {})", dev.name, dev.udid, dev.runtime);

        // Snapshot existing webinspectord_sim sockets BEFORE booting
        // this device so we can identify the new socket as belonging
        // to this UDID. (lsof doesn't tell us socket → UDID directly,
        // but boot-time diff does.)
        let before: std::collections::HashSet<_> =
            proxy::find_live_sim_sockets().await?.into_iter().collect();

        simulator::boot_if_needed(&dev.udid).await?;

        let socket = proxy::wait_for_new_socket(&before, Duration::from_secs(15))
            .await
            .with_context(|| {
                format!("locate webinspectord_sim socket for {}", dev.name)
            })?;
        println!("  socket: {}", socket.display());

        let (control_port, device_port) = state::next_port_pair(&state.devices);
        let pid = proxy::spawn(&socket, control_port, device_port).await?;
        proxy::wait_for_ready(control_port, Duration::from_secs(5)).await?;
        println!(
            "  proxy pid: {pid} · control :{control_port} · device :{device_port}"
        );

        state.devices.push(DeviceState {
            udid: dev.udid,
            device_name: dev.name,
            runtime: dev.runtime,
            socket_path: socket,
            proxy_pid: pid,
            control_port,
            device_port,
        });
        state::save(&state)?;
    }

    println!("\nready. Try `ios-inspect open <url>` to load a page on every device.");
    Ok(())
}

/// Resolve a `--device` filter to a concrete list of device states.
/// `None` → every booted device. `Some(key)` → exact UDID or
/// case-insensitive substring match against device name.
fn resolve_devices<'a>(
    state: &'a state::State,
    filter: Option<&str>,
) -> Result<Vec<&'a DeviceState>> {
    if state.devices.is_empty() {
        bail!("not booted — run `ios-inspect boot` first");
    }
    match filter {
        None => Ok(state.devices.iter().collect()),
        Some(key) => {
            let d = state
                .find(key)
                .ok_or_else(|| anyhow::anyhow!("no booted device matches {key:?}"))?;
            Ok(vec![d])
        }
    }
}

/// Verify a single device's proxy is alive + the socket path still
/// matches. If the proxy died (sim webinspectord_sim restarted, etc.)
/// re-spawn transparently and update state. Mirrors the pre-10.2
/// `ensure_proxy_healthy` but scoped per-device.
async fn ensure_device_healthy(udid: &str) -> Result<DeviceState> {
    let mut state = state::load()?.context("not booted — run `ios-inspect boot` first")?;
    let dev = state
        .find_mut(udid)
        .ok_or_else(|| anyhow::anyhow!("device {udid} not in session state"))?;

    let live = proxy::find_live_sim_sockets().await?;
    let proxy_alive = proxy::is_running(dev.proxy_pid);
    let socket_matches = live.iter().any(|p| p == &dev.socket_path);

    if proxy_alive && socket_matches {
        let snapshot = dev.clone();
        return Ok(snapshot);
    }

    if proxy_alive {
        let _ = proxy::kill(dev.proxy_pid);
    }
    // Socket may have rotated under us; we don't know which of the
    // live sockets is the "right" one for this UDID anymore. Best we
    // can do is fail loudly — the user re-runs `ios-inspect boot
    // --device <name>` to re-attribute.
    if !socket_matches {
        bail!(
            "device {} ({}) lost its webinspectord_sim socket — \
             re-run `ios-inspect boot --device \"{}\"` to re-attach",
            dev.device_name,
            dev.udid,
            dev.device_name,
        );
    }
    let pid = proxy::spawn(&dev.socket_path, dev.control_port, dev.device_port).await?;
    proxy::wait_for_ready(dev.control_port, Duration::from_secs(5)).await?;
    dev.proxy_pid = pid;
    let snapshot = dev.clone();
    state::save(&state)?;
    Ok(snapshot)
}

async fn cmd_open(url: &str, device_filter: Option<&str>) -> Result<()> {
    let state = state::load()?.context("not booted — run `ios-inspect boot` first")?;
    let targets = resolve_devices(&state, device_filter)?;
    for d in targets {
        simulator::openurl(&d.udid, url).await?;
        println!("[{}] opened {url}", d.label());
    }
    Ok(())
}

async fn cmd_eval(expression: &str, raw: bool, device_filter: Option<&str>) -> Result<()> {
    let state = state::load()?.context("not booted — run `ios-inspect boot` first")?;
    let targets = resolve_devices(&state, device_filter)?;
    for d in targets {
        let healthy = ensure_device_healthy(&d.udid).await?;
        let tab = proxy::pick_current_tab(healthy.device_port).await?;
        let mut sess = protocol::Session::connect(&tab.ws_url).await?;
        let result = sess.runtime_evaluate(expression).await?;
        let prefix = format!("[{}]", healthy.label());
        if raw {
            println!("{prefix} {}", serde_json::to_string_pretty(&result)?);
        } else if let Some(v) = result.pointer("/result/value") {
            match v {
                serde_json::Value::String(s) => println!("{prefix} {s}"),
                other => println!("{prefix} {other}"),
            }
        } else {
            println!("{prefix} {}", serde_json::to_string_pretty(&result)?);
        }
    }
    Ok(())
}

async fn cmd_computed_style(
    selector: &str,
    filter: Option<String>,
    device_filter: Option<&str>,
) -> Result<()> {
    let state = state::load()?.context("not booted — run `ios-inspect boot` first")?;
    let targets = resolve_devices(&state, device_filter)?;
    let filter_set: Option<std::collections::HashSet<String>> = filter
        .map(|f| f.split(',').map(|s| s.trim().to_string()).collect());
    for d in targets {
        let healthy = ensure_device_healthy(&d.udid).await?;
        let tab = proxy::pick_current_tab(healthy.device_port).await?;
        let mut sess = protocol::Session::connect(&tab.ws_url).await?;
        let node_id = sess
            .query_selector(selector)
            .await?
            .ok_or_else(|| anyhow::anyhow!("[{}] no element matches {selector:?}", healthy.label()))?;
        let style = sess.computed_style(node_id).await?;
        let mut out = serde_json::Map::new();
        for prop in style {
            if let Some(ref set) = filter_set {
                if !set.contains(&prop.name) {
                    continue;
                }
            }
            out.insert(prop.name, serde_json::Value::String(prop.value));
        }
        println!("[{}] {}", healthy.label(), serde_json::to_string_pretty(&out)?);
    }
    Ok(())
}

async fn cmd_dump_dom(
    selector: Option<&str>,
    depth: i32,
    device_filter: Option<&str>,
) -> Result<()> {
    let state = state::load()?.context("not booted — run `ios-inspect boot` first")?;
    let targets = resolve_devices(&state, device_filter)?;
    for d in targets {
        let healthy = ensure_device_healthy(&d.udid).await?;
        let tab = proxy::pick_current_tab(healthy.device_port).await?;
        let mut sess = protocol::Session::connect(&tab.ws_url).await?;
        let tree = sess.dump_dom(selector, depth).await?;
        let prefix = format!("[{}]", healthy.label());
        if let Some(html) = tree.get("outerHTML").and_then(serde_json::Value::as_str) {
            println!("{prefix} {html}");
        } else {
            println!("{prefix} {}", serde_json::to_string_pretty(&tree)?);
        }
    }
    Ok(())
}

async fn cmd_screenshot(
    output: &Path,
    web_only: bool,
    device_filter: Option<&str>,
) -> Result<()> {
    let state = state::load()?.context("not booted — run `ios-inspect boot` first")?;
    let targets = resolve_devices(&state, device_filter)?;

    let multi = targets.len() > 1;
    if multi && !output.is_dir() {
        bail!(
            "with multiple devices, --output must be an existing directory \
             (per-device PNGs are written as <label>.png inside it). got: {}",
            output.display()
        );
    }

    for d in targets {
        let path = if multi {
            output.join(format!("{}.png", d.label()))
        } else {
            output.to_path_buf()
        };
        if web_only {
            let healthy = ensure_device_healthy(&d.udid).await?;
            let tab = proxy::pick_current_tab(healthy.device_port).await?;
            let mut sess = protocol::Session::connect(&tab.ws_url).await?;
            let (w, h) = sess.viewport_size().await?;
            let png = sess.snapshot_rect(0.0, 0.0, w, h).await?;
            tokio::fs::write(&path, &png).await?;
            println!(
                "[{}] wrote {} ({w}x{h}, {} bytes, web-only)",
                d.label(),
                path.display(),
                png.len()
            );
        } else {
            simulator::screenshot(&d.udid, &path).await?;
            println!("[{}] wrote {} (via simctl)", d.label(), path.display());
        }
    }
    Ok(())
}

async fn cmd_tabs(device_filter: Option<&str>) -> Result<()> {
    let state = state::load()?.context("not booted — run `ios-inspect boot` first")?;
    let targets = resolve_devices(&state, device_filter)?;
    for d in targets {
        let healthy = ensure_device_healthy(&d.udid).await?;
        let tabs = proxy::list_tabs(healthy.device_port).await?;
        if tabs.is_empty() {
            println!(
                "[{}] (no tabs — open a page with `ios-inspect open <url>`)",
                d.label()
            );
            continue;
        }
        for t in tabs {
            println!("[{}] {:>3}  {}  ({})", d.label(), t.page_num, t.url, t.title);
        }
    }
    Ok(())
}

async fn cmd_shutdown() -> Result<()> {
    let Some(state) = state::load()? else {
        println!("nothing to shut down (no state file)");
        return Ok(());
    };
    if state.devices.is_empty() {
        println!("nothing to shut down (state file empty)");
        state::clear()?;
        return Ok(());
    }
    for d in &state.devices {
        if proxy::is_running(d.proxy_pid) {
            proxy::kill(d.proxy_pid)?;
            println!("[{}] killed proxy (pid {})", d.label(), d.proxy_pid);
        }
        simulator::shutdown(&d.udid).await?;
        println!("[{}] shut down sim", d.label());
    }
    state::clear()?;
    Ok(())
}
