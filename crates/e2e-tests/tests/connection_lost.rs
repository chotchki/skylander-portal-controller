//! ConnectionLost overlay regression (PLAN 4.18.21).
//!
//! Two contracts pinned here:
//!   1. Server-up → no overlay; server killed → overlay appears past the
//!      grace window with the right heading.
//!   2. **Persistence**: once the overlay is visible, it stays visible
//!      across the WS reconnect cycle. The first cut of the component
//!      hid the overlay every time `conn` flipped to `Connecting` during
//!      a backoff tick — visually a "flash and vanish" that this assertion
//!      now catches in CI instead of needing on-device repro.
//!
//! Reconnect-success → overlay-vanishes is still on-device-only because
//! the harness can't bring a dead server back on the same port.
//!
//! Browser console output (`[ws]` / `[overlay]` lines from the SPA's
//! instrumentation) is mirrored into a `window.__capturedLogs` array so
//! the test can dump them on failure — saves a manual repro round-trip.
//!
//! Prerequisites: chromedriver running at http://localhost:4444, phone SPA
//! built (`cd phone && trunk build`). See crates/e2e-tests/README.md.

use std::time::Duration;

use fantoccini::Locator;
use serde_json::Value;
use skylander_e2e_tests::{Phone, TestServer};

/// JS shim that mirrors `console.{log,warn,error}` into `window.__capturedLogs`
/// so we can fish them back out via `client.execute(...)`. Idempotent — safe
/// to inject more than once.
const INSTALL_CONSOLE_HOOK: &str = r#"
    if (!window.__hookInstalled) {
        window.__capturedLogs = [];
        const wrap = (level, orig) => function(...args) {
            try {
                window.__capturedLogs.push({
                    t: Date.now(),
                    level,
                    msg: args.map(a => {
                        try { return typeof a === 'string' ? a : JSON.stringify(a); }
                        catch (_) { return String(a); }
                    }).join(' '),
                });
            } catch (_) {}
            orig.apply(console, args);
        };
        const origLog = console.log.bind(console);
        const origWarn = console.warn.bind(console);
        const origErr = console.error.bind(console);
        console.log = wrap('log', origLog);
        console.warn = wrap('warn', origWarn);
        console.error = wrap('error', origErr);
        window.__hookInstalled = true;
    }
    return true;
"#;

const FETCH_LOGS: &str = r#"return JSON.stringify(window.__capturedLogs || []);"#;

async fn fetch_console_logs(phone: &Phone) -> String {
    let raw = phone
        .client
        .execute(FETCH_LOGS, vec![])
        .await
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "[]".to_string());
    // Pretty-print one entry per line for human-readable failure output.
    serde_json::from_str::<Vec<Value>>(&raw)
        .map(|entries| {
            entries
                .into_iter()
                .map(|e| {
                    let level = e.get("level").and_then(Value::as_str).unwrap_or("?");
                    let t = e.get("t").and_then(Value::as_i64).unwrap_or(0);
                    let msg = e.get("msg").and_then(Value::as_str).unwrap_or("");
                    format!("  [{level:5}] t={t} {msg}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or(raw)
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + built phone SPA"]
async fn overlay_appears_and_persists_through_reconnect_cycle() {
    let mut server = TestServer::spawn().expect("spawn server");
    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .expect("connect phone");

    // Wait until the SPA reaches a steady-state screen — confirms the WS
    // handshake completed. Profile picker is the first surface a fresh
    // session lands on (no profile unlocked, no game launched).
    phone
        .wait_for(
            Locator::Css(".profile-picker, .game-picker"),
            Duration::from_secs(15),
        )
        .await
        .expect("phone reached a steady state");

    // Install the console capture *after* the SPA has mounted. The first
    // WS lifecycle log lines (initial onopen) happen before this hook is
    // in place — that's fine, we only care about events from the kill
    // forward, and those happen well after this point.
    phone
        .client
        .execute(INSTALL_CONSOLE_HOOK, vec![])
        .await
        .expect("install console hook");

    // Sanity: while the server is healthy, the overlay must not be shown.
    assert!(
        phone
            .client
            .find(Locator::Css("[data-testid=connection-lost]"))
            .await
            .is_err(),
        "ConnectionLost overlay must not be visible while the server is up"
    );

    // Kill only the server. The chromedriver and the SPA in the headless
    // browser stay alive; the phone's WS sees onclose and starts the
    // reconnect dance against the now-dead address.
    server.kill_server();

    // Grace is 1s in `connection_lost.rs`; allow generous slack for the
    // backoff timer + browser repaint.
    let overlay = match phone
        .wait_for(
            Locator::Css("[data-testid=connection-lost]"),
            Duration::from_secs(8),
        )
        .await
    {
        Ok(el) => el,
        Err(e) => {
            let logs = fetch_console_logs(&phone).await;
            panic!(
                "ConnectionLost overlay should appear after server dies: {e}\n--- captured browser console ---\n{logs}"
            );
        }
    };

    let text = overlay.text().await.unwrap();
    assert!(
        text.contains("LOST CONNECTION"),
        "overlay should carry the LOST CONNECTION heading; got: {text}"
    );
    assert!(
        text.contains("reconnecting") || text.contains("TRY AGAIN"),
        "overlay should show either the auto-reconnect spinner or the manual retry button; got: {text}"
    );

    // ---- Persistence assertion ------------------------------------------
    //
    // The full backoff sequence (500ms, 1s, 2s, 4s, 8s) tops out at ~15.5s
    // for five attempts. Sleep 18s to cover several cycles plus a TCP
    // timeout slack window. If the overlay vanishes during this stretch
    // it's the same bug Chris hit on-device: the SPA flashes the overlay
    // off when `conn` transitions to `Connecting` mid-backoff.
    //
    // Poll the DOM during the wait so a vanish-then-reappear cycle still
    // counts as a failure (we want continuous visibility, not eventual).
    let persistence_window = Duration::from_secs(18);
    let poll_interval = Duration::from_millis(250);
    let deadline = std::time::Instant::now() + persistence_window;
    let mut polls = 0u32;
    while std::time::Instant::now() < deadline {
        polls += 1;
        if phone
            .client
            .find(Locator::Css("[data-testid=connection-lost]"))
            .await
            .is_err()
        {
            let elapsed = persistence_window
                .saturating_sub(deadline.saturating_duration_since(std::time::Instant::now()));
            let logs = fetch_console_logs(&phone).await;
            panic!(
                "ConnectionLost overlay vanished during the reconnect cycle \
                 (after {elapsed:?}, poll {polls}). Server is still dead, so \
                 the overlay should remain visible until reconnect succeeds.\n\
                 --- captured browser console ---\n{logs}"
            );
        }
        tokio::time::sleep(poll_interval).await;
    }

    phone.close().await.unwrap();
}
