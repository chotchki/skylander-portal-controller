//! Library surface of the `ios-inspect` tool.
//!
//! Keeps the iOS-Simulator-driving primitives (boot/open/eval/etc.)
//! reusable from the CLI in `src/main.rs` and from the e2e harness in
//! `crates/e2e-tests` (PLAN 10.4.1). The CLI owns command parsing and
//! output formatting; everything else lives here.
//!
//! State location is shared with the CLI (`/tmp/ios-inspect-state.json`)
//! so an e2e session and a manual `ios-inspect …` session must not run
//! concurrently — they'd clobber each other's UDID + port allocations.

pub mod protocol;
pub mod proxy;
pub mod simulator;
pub mod state;

use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::state::{DeviceState, State};

/// Boot one or more simulators (by `xcrun simctl` substring match) and
/// spawn an `ios_webkit_debug_proxy` per device on a unique port pair.
/// Tops up an existing session when called against already-booted
/// devices — re-running `boot` adds new devices without re-spawning
/// proxies for ones that are already healthy.
///
/// Returns the full session state after the boot completes. Caller
/// owns the returned state and is responsible for `shutdown_all` when
/// done (or `tear_down(state)`).
pub async fn boot_devices(device_names: &[String]) -> Result<State> {
    let mut state = state::load()?.unwrap_or(State {
        devices: Vec::new(),
    });

    // Reap stale state silently from the lib path — the CLI does its
    // own reaping with stderr breadcrumbs (see main.rs::cmd_boot); the
    // lib doesn't pull `tracing` in just to log this.
    state.devices.retain(|d| proxy::is_running(d.proxy_pid));

    let requested = simulator::pick_devices(device_names)?;
    let to_boot: Vec<_> = requested
        .into_iter()
        .filter(|d| !state.devices.iter().any(|ds| ds.udid == d.udid))
        .collect();

    if to_boot.is_empty() {
        return Ok(state);
    }

    simulator::launch_simulator_app().await?;

    for dev in to_boot {
        let before: std::collections::HashSet<_> =
            proxy::find_live_sim_sockets().await?.into_iter().collect();

        simulator::boot_if_needed(&dev.udid).await?;

        let socket = proxy::wait_for_new_socket(&before, Duration::from_secs(60))
            .await
            .with_context(|| format!("locate webinspectord_sim socket for {}", dev.name))?;

        let (control_port, device_port) = state::next_port_pair(&state.devices);
        let pid = proxy::spawn(&socket, control_port, device_port).await?;
        proxy::wait_for_ready(control_port, Duration::from_secs(10)).await?;

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

    Ok(state)
}

/// Open `url` in Safari on a specific device.
pub async fn open_url(device: &DeviceState, url: &str) -> Result<()> {
    simulator::openurl(&device.udid, url).await
}

/// Open `url` on every device in the session — the typical "load this
/// SPA on both iPad + iPhone" shape from PLAN 10.4.4.
pub async fn open_url_on_all(state: &State, url: &str) -> Result<()> {
    for d in &state.devices {
        open_url(d, url).await?;
    }
    Ok(())
}

/// Wait for `selector` to match at least one element on `device`'s
/// current page. Polls the WebKit Web Inspector via `Runtime.evaluate`
/// every 200 ms (single-shot WS connect per attempt — the underlying
/// session is not yet pooled).
///
/// Forgives transient "no tabs visible" and "WS connect refused"
/// states for the first several seconds — Safari takes a moment to
/// register the just-opened URL with `webinspectord_sim` after
/// `simctl openurl` returns. Only the final "no progress before
/// `timeout`" condition surfaces an error.
pub async fn wait_for_selector(
    device: &DeviceState,
    selector: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    let expr = format!(
        "!!document.querySelector({})",
        serde_json::to_string(selector)?
    );
    loop {
        // try_eval_bool returns Err for transient states (tab not
        // registered yet, WS hand-off racing) AND for genuine failures.
        // Either way, keep polling until the deadline; on timeout,
        // surface the most recent error (if any) for diagnostic value.
        let attempt = try_eval_bool(device, &expr).await;
        if let Ok(true) = attempt {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            let detail = match attempt {
                Err(e) => format!(" — last error: {e}"),
                _ => String::new(),
            };
            return Err(anyhow::anyhow!(
                "timed out after {timeout:?} waiting for `{selector}` on {}{}",
                device.device_name,
                detail,
            ));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn try_eval_bool(device: &DeviceState, expr: &str) -> Result<bool> {
    let v = eval_js(device, expr).await?;
    Ok(v.as_bool().unwrap_or(false))
}

/// Evaluate `expr` against `device`'s current page and return the
/// `Runtime.evaluate` result.value. Caller decodes from
/// `serde_json::Value`. Errors propagate from the underlying WS
/// transport / protocol session — typically "no tabs visible" when
/// Safari hasn't registered the active page yet.
pub async fn eval_js(device: &DeviceState, expr: &str) -> Result<serde_json::Value> {
    let tab = proxy::pick_current_tab(device.device_port).await?;
    let mut sess = protocol::Session::connect(&tab.ws_url).await?;
    let result = sess.runtime_evaluate(expr).await?;
    Ok(result
        .pointer("/result/value")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

/// Read the phone session id the server assigned this device, exposed
/// as `<body data-session-id="…">`. Returns `None` until the WS
/// `Event::Welcome` lands. Polls inline with `wait_for_session_id` for
/// the wait-loop variant.
pub async fn session_id(device: &DeviceState) -> Result<Option<u64>> {
    let v = eval_js(
        device,
        "document.body.getAttribute('data-session-id') || ''",
    )
    .await?;
    let s = v.as_str().unwrap_or("");
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(s.parse::<u64>().ok())
    }
}

/// Wait for `device` to receive a session id (Event::Welcome) and
/// return it. Mirrors the chromedriver harness's `Phone::session_id`
/// polling loop.
pub async fn wait_for_session_id(device: &DeviceState, timeout: Duration) -> Result<u64> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(Some(id)) = session_id(device).await {
            return Ok(id);
        }
        if std::time::Instant::now() >= deadline {
            bail!(
                "{} never received Event::Welcome within {timeout:?}",
                device.device_name
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Count elements matching `selector` on `device`'s current page.
pub async fn query_selector_count(device: &DeviceState, selector: &str) -> Result<usize> {
    let tab = proxy::pick_current_tab(device.device_port).await?;
    let mut sess = protocol::Session::connect(&tab.ws_url).await?;
    let expr = format!(
        "document.querySelectorAll({}).length",
        serde_json::to_string(selector)?
    );
    let result = sess.runtime_evaluate(&expr).await?;
    let n = result
        .pointer("/result/value")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("querySelectorAll length returned non-numeric"))?;
    Ok(n as usize)
}

/// Read the visible text of the first element matching `selector`.
/// Uses `innerText` rather than WebDriver's getElementText, so visually
/// hidden text reports "" (intentional — match the user-visible
/// rendering, not the DOM-source string). On no match returns `None`.
pub async fn query_selector_text(device: &DeviceState, selector: &str) -> Result<Option<String>> {
    let tab = proxy::pick_current_tab(device.device_port).await?;
    let mut sess = protocol::Session::connect(&tab.ws_url).await?;
    let expr = format!(
        "(function(){{ const el = document.querySelector({}); return el ? el.innerText : null; }})()",
        serde_json::to_string(selector)?,
    );
    let result = sess.runtime_evaluate(&expr).await?;
    Ok(result
        .pointer("/result/value")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string))
}

/// Capture the web-content viewport as a PNG (no simulator chrome).
/// Returns the raw PNG bytes — callers `tokio::fs::write` to disk if
/// they want a file.
pub async fn screenshot_web(device: &DeviceState) -> Result<Vec<u8>> {
    let tab = proxy::pick_current_tab(device.device_port).await?;
    let mut sess = protocol::Session::connect(&tab.ws_url).await?;
    let (w, h) = sess.viewport_size().await?;
    sess.snapshot_rect(0.0, 0.0, w, h).await
}

/// Tear down every booted device + its proxy, clear the state file.
/// Idempotent — safe to call when nothing is booted.
pub async fn shutdown_all() -> Result<()> {
    let Some(state) = state::load()? else {
        return Ok(());
    };
    for d in &state.devices {
        if proxy::is_running(d.proxy_pid) {
            proxy::kill(d.proxy_pid)?;
        }
        simulator::shutdown(&d.udid).await?;
    }
    state::clear()
}

/// Helper: tear down the session in `state`. Use when the caller owns
/// a State and wants the e2e RAII pattern (drop the State, shutdown).
/// Calling this multiple times against the same state is safe.
pub async fn tear_down(_state: State) -> Result<()> {
    shutdown_all().await
}
