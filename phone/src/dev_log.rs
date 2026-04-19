//! Phone-side dev log channel (PLAN 4.18.21 supporting infra).
//!
//! Mirrors `dev_log!` / `dev_warn!` calls into the browser console **and**
//! queues them for periodic POST to `/api/_dev/log` on the server. The
//! server re-emits each entry through its `tracing` sink, so they land in
//! whatever the launcher process is using (PowerShell on the dev HTPC).
//! Lets us debug real-device behavior — iOS Safari, real Wi-Fi paths —
//! without needing a Mac + Web Inspector tether.
//!
//! The endpoint is gated on the server's `dev-tools` feature (default-on
//! in dev, off in release), so production builds physically can't accept
//! these. The flusher tolerates HTTP errors silently — best-effort only.
//!
//! ## Disconnect-window retention
//!
//! Critical contract: messages generated while the server is unreachable
//! must NOT be lost. The flusher takes a snapshot, attempts the POST, and
//! only drains the snapshot's prefix on success. On failure (server dead,
//! transient network blip) the entries stay buffered and the next tick
//! retries. A cap prevents unbounded growth if the server stays down — we
//! drop the oldest entries first so the most recent debug context is
//! preserved. See `tests::*` for the exact contract.

use std::cell::RefCell;

use serde::Serialize;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{Request, RequestInit, Response};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Entry {
    /// `Date.now()` at record time. Browser ms since epoch.
    pub t: f64,
    pub level: &'static str,
    pub msg: String,
}

/// Hard cap so a permanently dead server can't grow the buffer without
/// bound. Picked to comfortably fit several minutes of disconnect-period
/// chatter (`[ws]` / `[overlay]` lines fire on the order of a handful per
/// disconnect cycle, plus any future `dev_log!`s callers add).
const BUFFER_CAP: usize = 500;

/// Pure-data buffer with the retain-on-failure contract. Lives inside a
/// thread-local `RefCell` for the actual phone runtime; tests construct
/// instances directly.
pub struct DevLogBuffer {
    entries: Vec<Entry>,
    cap: usize,
}

impl DevLogBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: Vec::new(),
            cap,
        }
    }

    /// Append an entry. If the cap is exceeded, drop the oldest (preserves
    /// the most recent debug context — the pre-incident chatter is less
    /// useful than the immediate-aftermath stream).
    pub fn push(&mut self, entry: Entry) {
        self.entries.push(entry);
        if self.entries.len() > self.cap {
            let drop = self.entries.len() - self.cap;
            self.entries.drain(0..drop);
        }
    }

    /// Snapshot of pending entries. Cloned so the caller can `.await`
    /// without holding the lock and without races against new pushes.
    pub fn snapshot(&self) -> Vec<Entry> {
        self.entries.clone()
    }

    /// After a successful POST of N entries, drain that many from the
    /// front. Caller passes the snapshot length, not the current buffer
    /// length — between snapshot and confirm, new entries may have been
    /// pushed; those must NOT be discarded.
    pub fn confirm_sent(&mut self, n: usize) {
        let n = n.min(self.entries.len());
        self.entries.drain(0..n);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

thread_local! {
    static BUFFER: RefCell<DevLogBuffer> = RefCell::new(DevLogBuffer::new(BUFFER_CAP));
}

/// Push one entry into the forward buffer and mirror it to the browser
/// console. The console mirror keeps DevTools-side debugging unaffected;
/// the buffer entry feeds the server-side stream.
pub fn record(level: &'static str, msg: String) {
    let console_msg: JsValue = format!("[phone] {msg}").into();
    match level {
        "warn" => web_sys::console::warn_1(&console_msg),
        "error" => web_sys::console::error_1(&console_msg),
        _ => web_sys::console::log_1(&console_msg),
    }
    BUFFER.with(|b| {
        b.borrow_mut().push(Entry {
            t: js_sys::Date::now(),
            level,
            msg,
        })
    });
}

/// Spawn the flusher loop. Drains the buffer every ~1s and POSTs the
/// batch as JSON. On POST failure, the snapshot stays in the buffer for
/// the next tick (this is the disconnect-window retention contract — see
/// module docs). Idempotent: safe to call once at App() mount.
pub fn start_flusher() {
    spawn_local(async move {
        loop {
            crate::gloo_timer(1000).await;
            let snapshot = BUFFER.with(|b| b.borrow().snapshot());
            if snapshot.is_empty() {
                continue;
            }
            if post_logs(&snapshot).await.is_ok() {
                let n = snapshot.len();
                BUFFER.with(|b| b.borrow_mut().confirm_sent(n));
            }
            // On failure: leave snapshot in the buffer; the cap on the
            // push side prevents unbounded growth if the server stays
            // unreachable.
        }
    });
}

async fn post_logs(entries: &[Entry]) -> Result<(), JsValue> {
    use wasm_bindgen::JsCast;

    let body = serde_json::to_string(entries).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let opts = RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&body));

    let req = Request::new_with_str_and_init("/api/_dev/log", &opts)?;
    req.headers().set("Content-Type", "application/json")?;

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let resp_val = JsFuture::from(window.fetch_with_request(&req)).await?;
    let resp: Response = resp_val.dyn_into()?;
    if !resp.ok() {
        // 4xx/5xx counts as a transport failure for retention purposes —
        // we'd rather hold the entries than drop them on a server that
        // briefly returned a 503. Release builds 404 the route, so this
        // branch keeps the cap-bounded buffer growing until either dev
        // mode flips on or the user navigates away.
        return Err(JsValue::from_str(&format!(
            "dev_log POST returned {}",
            resp.status()
        )));
    }
    Ok(())
}

/// `format!`-style log line. Mirrors to console.log, queues for forward.
#[macro_export]
macro_rules! dev_log {
    ($($arg:tt)*) => {
        $crate::dev_log::record("log", format!($($arg)*))
    };
}

/// `format!`-style warning. Mirrors to console.warn, queues for forward.
#[macro_export]
macro_rules! dev_warn {
    ($($arg:tt)*) => {
        $crate::dev_log::record("warn", format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    //! Pure-Rust tests of the buffer contract. The flusher loop itself is
    //! untestable here (it depends on web_sys + the JS runtime), but the
    //! retention semantics — what stays vs. what drains — live in
    //! `DevLogBuffer` and can be exercised natively. Requires
    //! `crate-type = ["cdylib", "rlib"]` in Cargo.toml so the lib has a
    //! native target alongside the wasm one.

    use super::*;

    fn entry(msg: &str) -> Entry {
        Entry {
            t: 0.0,
            level: "log",
            msg: msg.to_string(),
        }
    }

    #[test]
    fn push_grows_buffer() {
        let mut b = DevLogBuffer::new(100);
        assert!(b.is_empty());
        b.push(entry("a"));
        b.push(entry("b"));
        assert_eq!(b.len(), 2);
        assert_eq!(b.snapshot()[0].msg, "a");
        assert_eq!(b.snapshot()[1].msg, "b");
    }

    #[test]
    fn cap_drops_oldest_first() {
        let mut b = DevLogBuffer::new(3);
        for ch in ['a', 'b', 'c', 'd', 'e'] {
            b.push(entry(&ch.to_string()));
        }
        // Cap = 3 so only the last 3 survive — most recent context wins.
        assert_eq!(b.len(), 3);
        let snap = b.snapshot();
        assert_eq!(snap[0].msg, "c");
        assert_eq!(snap[1].msg, "d");
        assert_eq!(snap[2].msg, "e");
    }

    #[test]
    fn confirm_sent_drains_prefix() {
        let mut b = DevLogBuffer::new(100);
        b.push(entry("a"));
        b.push(entry("b"));
        b.push(entry("c"));
        b.confirm_sent(2);
        assert_eq!(b.len(), 1);
        assert_eq!(b.snapshot()[0].msg, "c");
    }

    #[test]
    fn confirm_sent_n_larger_than_buffer_clears_all() {
        let mut b = DevLogBuffer::new(100);
        b.push(entry("a"));
        b.confirm_sent(99);
        assert!(b.is_empty());
    }

    /// The disconnect-window contract: snapshot N entries, simulate a
    /// failed POST (no confirm), then push more entries, then succeed
    /// later. Only the original snapshot's count drains — the entries
    /// generated DURING the disconnect window are preserved for the next
    /// flush tick. This is the regression that motivated the buffer
    /// rewrite — the first cut drained optimistically and lost
    /// disconnect-period messages.
    #[test]
    fn entries_pushed_during_failed_flush_survive_to_next_tick() {
        let mut b = DevLogBuffer::new(100);
        b.push(entry("before-disconnect"));
        b.push(entry("during-disconnect-1"));

        let snapshot = b.snapshot();
        assert_eq!(snapshot.len(), 2);

        // Simulate "POST failed" — nothing gets confirmed. Meanwhile new
        // dev_log!s fire (e.g. [ws] onclose, [overlay] effect runs).
        b.push(entry("during-disconnect-2"));
        b.push(entry("during-disconnect-3"));
        assert_eq!(b.len(), 4);

        // Server comes back; next flush tick takes a fresh snapshot and
        // the POST succeeds. confirm_sent uses the snapshot's count, NOT
        // the current buffer length, so the new entries are kept.
        let next_snapshot = b.snapshot();
        assert_eq!(next_snapshot.len(), 4);
        b.confirm_sent(next_snapshot.len());
        assert!(b.is_empty());
    }

    /// Even more pointed: simulate the exact ws.rs scenario — server
    /// dies, several flush ticks fail, more dev_log!s accumulate, then
    /// server comes back. Verifies that ALL disconnect-window messages
    /// reach the server (in order, no dropouts up to the cap).
    #[test]
    fn long_disconnect_then_reconnect_delivers_full_history() {
        let mut b = DevLogBuffer::new(100);

        // Pre-disconnect: one steady-state log.
        b.push(entry("steady-state"));

        // 5 failed flush attempts, each with new disconnect-period logs
        // arriving between attempts.
        for tick in 0..5 {
            let snapshot = b.snapshot();
            // POST fails — DON'T call confirm_sent.
            let _ = snapshot;
            b.push(entry(&format!("[ws] onclose tick={tick}")));
            b.push(entry(&format!("[overlay] effect tick={tick}")));
        }

        // Buffer should now have: steady-state + 5*2 = 11 entries.
        assert_eq!(b.len(), 11);

        // Server reachable again — successful flush drains everything.
        let final_snapshot = b.snapshot();
        b.confirm_sent(final_snapshot.len());
        assert!(b.is_empty());

        // Order is preserved.
        assert_eq!(final_snapshot[0].msg, "steady-state");
        assert_eq!(final_snapshot[1].msg, "[ws] onclose tick=0");
        assert_eq!(final_snapshot[10].msg, "[overlay] effect tick=4");
    }

    /// Cap eviction during a long disconnect: the oldest entries get
    /// dropped, but the most recent (which carry the actual diagnostic
    /// signal) are preserved.
    #[test]
    fn disconnect_overflow_drops_oldest_keeps_recent() {
        let mut b = DevLogBuffer::new(5);
        for i in 0..20 {
            // POST fails every tick — buffer keeps growing until cap.
            b.push(entry(&format!("msg-{i}")));
        }
        assert_eq!(b.len(), 5);
        let snap = b.snapshot();
        assert_eq!(snap[0].msg, "msg-15");
        assert_eq!(snap[4].msg, "msg-19");
    }
}
