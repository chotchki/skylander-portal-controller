//! WebSocket client. Connects on mount, auto-reconnects with backoff,
//! deserialises incoming `Event`s and updates the portal signal.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, WebSocket};

use crate::api::{observe_boot_id, set_session_id};
use crate::model::{ConnState, Event, GameLaunched, Slot, SlotState, UnlockedProfile, SLOT_COUNT};
use crate::{dev_log, dev_warn, push_toast, GameCrashReason, ResumeOffer, TakeoverReason, ToastMsg};

/// Pending backoff timer handle. Tracked so a manual TRY AGAIN can cancel
/// the in-flight wait and reconnect immediately. Single-threaded WASM so
/// `Rc<Cell<_>>` is fine; never crosses a thread.
type PendingTimer = Rc<Cell<Option<i32>>>;

#[allow(clippy::too_many_arguments)]
pub fn connect(
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    conn: RwSignal<ConnState>,
    toasts: RwSignal<Vec<ToastMsg>>,
    current_game: RwSignal<Option<GameLaunched>>,
    unlocked_profile: RwSignal<Option<UnlockedProfile>>,
    takeover: RwSignal<Option<TakeoverReason>>,
    resume_offer: RwSignal<Option<ResumeOffer>>,
    game_crash: RwSignal<Option<GameCrashReason>>,
    scan_overlay: RwSignal<crate::ScanOverlayState>,
    reconnect_attempts: RwSignal<u32>,
    manual_retry: RwSignal<u32>,
) {
    let pending: PendingTimer = Rc::new(Cell::new(None));

    // Manual retry: when the UI bumps `manual_retry`, cancel the pending
    // backoff timer (if any) and reconnect immediately. The Effect's prev
    // arg lets us only act on changes after the initial run, so mounting
    // the component doesn't trigger a spurious reconnect.
    {
        let pending = pending.clone();
        Effect::new(move |prev: Option<u32>| {
            let now = manual_retry.get();
            if let Some(p) = prev {
                if now != p {
                    cancel_pending_timer(&pending);
                    reconnect_attempts.set(0);
                    spawn_connect(
                        portal,
                        conn,
                        toasts,
                        current_game,
                        unlocked_profile,
                        takeover,
                        resume_offer,
                        game_crash,
                        scan_overlay,
                        reconnect_attempts,
                        manual_retry,
                        pending.clone(),
                        0,
                    );
                }
            }
            now
        });
    }

    spawn_connect(
        portal,
        conn,
        toasts,
        current_game,
        unlocked_profile,
        takeover,
        resume_offer,
        game_crash,
        scan_overlay,
        reconnect_attempts,
        manual_retry,
        pending,
        0,
    );
}

fn cancel_pending_timer(pending: &PendingTimer) {
    if let Some(handle) = pending.take() {
        if let Some(window) = web_sys::window() {
            window.clear_timeout_with_handle(handle);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_connect(
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    conn: RwSignal<ConnState>,
    toasts: RwSignal<Vec<ToastMsg>>,
    current_game: RwSignal<Option<GameLaunched>>,
    unlocked_profile: RwSignal<Option<UnlockedProfile>>,
    takeover: RwSignal<Option<TakeoverReason>>,
    resume_offer: RwSignal<Option<ResumeOffer>>,
    game_crash: RwSignal<Option<GameCrashReason>>,
    scan_overlay: RwSignal<crate::ScanOverlayState>,
    reconnect_attempts: RwSignal<u32>,
    manual_retry: RwSignal<u32>,
    pending: PendingTimer,
    attempt: u32,
) {
    let loc = web_sys::window().unwrap().location();
    let host = loc.host().unwrap_or_else(|_| "localhost".into());
    let scheme = if loc.protocol().unwrap_or_default() == "https:" {
        "wss://"
    } else {
        "ws://"
    };
    let url = format!("{scheme}{host}/ws");

    dev_log!("[ws] spawn_connect attempt={attempt} url={url}");
    conn.set(ConnState::Connecting);
    let ws = match WebSocket::new(&url) {
        Ok(w) => w,
        Err(_) => {
            schedule_reconnect(
                portal,
                conn,
                toasts,
                current_game,
                unlocked_profile,
                takeover,
                resume_offer,
                game_crash,
                scan_overlay,
                reconnect_attempts,
                manual_retry,
                pending,
                attempt,
            );
            return;
        }
    };

    // onopen — successful connection clears the attempt counter so the
    // next disconnect's overlay starts fresh (TRY AGAIN won't be premature).
    {
        let conn = conn;
        let on_open = Closure::<dyn FnMut()>::new(move || {
            dev_log!(
                "[ws] onopen — attempts→0 (was {})",
                reconnect_attempts.get_untracked()
            );
            conn.set(ConnState::Connected);
            reconnect_attempts.set(0);
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget();
    }

    // onmessage
    {
        let portal = portal;
        let toasts = toasts;
        let current_game = current_game;
        let unlocked_profile = unlocked_profile;
        let takeover = takeover;
        let scan_overlay = scan_overlay;
        let on_msg = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Some(text) = e.data().as_string() {
                match serde_json::from_str::<Event>(&text) {
                    Ok(Event::Welcome { session_id, boot_id }) => {
                        // Server restart detection: if the boot id changed
                        // from a known prior, reload the page so any cached
                        // UI state (unlocked profile, current screen, etc.)
                        // can't drift from the server's empty post-restart
                        // state. First connect after page load just stores
                        // the id.
                        if observe_boot_id(boot_id) {
                            dev_log!(
                                "[ws] boot_id changed → server restarted, reloading"
                            );
                            if let Some(loc) = web_sys::window().map(|w| w.location()) {
                                let _ = loc.reload();
                            }
                            return;
                        }
                        set_session_id(session_id);
                        // Expose the session id in the DOM so e2e tests can
                        // look it up without calling into WASM. Harmless to
                        // production — just an extra `data-` attr on <body>.
                        if let Some(body) = web_sys::window()
                            .and_then(|w| w.document())
                            .and_then(|d| d.body())
                        {
                            let _ = body.set_attribute("data-session-id", &session_id.to_string());
                        }
                    }
                    Ok(Event::PortalSnapshot { slots }) => {
                        let mut arr: [Slot; SLOT_COUNT] = std::array::from_fn(|_| Slot {
                            state: SlotState::Empty,
                        });
                        for (i, s) in slots.into_iter().take(SLOT_COUNT).enumerate() {
                            arr[i] = Slot { state: s };
                        }
                        portal.set(arr);
                    }
                    Ok(Event::SlotChanged { slot, state }) => {
                        portal.update(|p| {
                            let i = slot as usize;
                            if i < SLOT_COUNT {
                                p[i] = Slot { state };
                            }
                        });
                    }
                    Ok(Event::Error { message }) => {
                        push_toast(toasts, &message);
                    }
                    Ok(Event::GameChanged { current }) => {
                        // A new game booting implicitly dismisses the crash
                        // overlay (we're back up and running). Clearing to
                        // None (game ended) does NOT clear the crash — the
                        // server pushes GameChanged { current: None } right
                        // after GameCrashed, and we want the overlay to stay
                        // until either the user taps "RETURN TO GAMES" or a
                        // new game launches.
                        if current.is_some() {
                            game_crash.set(None);
                        }
                        current_game.set(current);
                    }
                    Ok(Event::GameCrashed { message }) => {
                        game_crash.set(Some(GameCrashReason { message }));
                    }
                    Ok(Event::ProfileChanged {
                        session_id,
                        profile,
                    }) => {
                        // Session-filtered: only apply if it's addressed to us.
                        // Other phones' unlock changes don't affect this client.
                        if Some(session_id) == crate::api::current_session_id() {
                            unlocked_profile.set(profile);
                        }
                    }
                    Ok(Event::TakenOver {
                        session_id,
                        by_kaos,
                    }) => {
                        if Some(session_id) == crate::api::current_session_id() {
                            takeover.set(Some(TakeoverReason { by_kaos }));
                        }
                    }
                    Ok(Event::ResumePrompt { session_id, slots }) => {
                        if Some(session_id) == crate::api::current_session_id() {
                            resume_offer.set(Some(ResumeOffer { slots }));
                        }
                    }
                    Ok(Event::FigureScanned {
                        uid: _,
                        figure_id: _,
                        variant: _,
                        display_name,
                        is_duplicate,
                    }) => {
                        // Broadcast to all sessions. Two paths depending on
                        // whether the user is actively using the scan-import
                        // flow: if the overlay is open in Prompt, flip it to
                        // Success so they see confirmation; otherwise fire a
                        // passive toast so ambient scans don't vanish
                        // silently. Name falls back to "a new figure" when
                        // the parser couldn't extract a nickname (unknown
                        // figure_id, CYOS layout gap, etc. — see 6.2.9).
                        // `is_duplicate` comes from the server — true when
                        // the same uid has been scanned before. We branch
                        // copy + toast level on it so repeated taps read
                        // differently ("Already scanned" vs "Scanned").
                        let name_for_show = if display_name.trim().is_empty() {
                            "a new figure".to_string()
                        } else {
                            display_name.clone()
                        };
                        if scan_overlay.get_untracked() == crate::ScanOverlayState::Prompt {
                            scan_overlay.set(crate::ScanOverlayState::Success {
                                display_name: name_for_show,
                                is_duplicate,
                            });
                        } else {
                            let (msg, level) = if is_duplicate {
                                (
                                    format!("Already scanned: {name_for_show}"),
                                    crate::ToastLevel::Info,
                                )
                            } else {
                                (
                                    format!("Scanned: {name_for_show}"),
                                    crate::ToastLevel::Success,
                                )
                            };
                            crate::push_toast_level(toasts, &msg, level);
                        }
                    }
                    Err(err) => {
                        dev_warn!("bad ws message: {err} — {text}");
                    }
                }
            }
        });
        ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
        on_msg.forget();
    }

    // onclose — reconnect.
    {
        let portal = portal;
        let conn = conn;
        let toasts = toasts;
        let current_game = current_game;
        let unlocked_profile = unlocked_profile;
        let takeover = takeover;
        let resume_offer = resume_offer;
        let game_crash = game_crash;
        let scan_overlay = scan_overlay;
        let pending = pending;
        let on_close = Closure::<dyn FnMut()>::new(move || {
            let prev = reconnect_attempts.get_untracked();
            dev_log!("[ws] onclose — attempts {prev}→{}", prev + 1);
            conn.set(ConnState::Disconnected);
            // Bump the attempt counter so the ConnectionLost overlay can
            // promote the TRY AGAIN button after enough failed retries.
            reconnect_attempts.update(|n| *n = n.saturating_add(1));
            schedule_reconnect(
                portal,
                conn,
                toasts,
                current_game,
                unlocked_profile,
                takeover,
                resume_offer,
                game_crash,
                scan_overlay,
                reconnect_attempts,
                manual_retry,
                pending.clone(),
                attempt + 1,
            );
        });
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        on_close.forget();
    }

    // onerror — let onclose handle reconnect.
    let on_err = Closure::<dyn FnMut()>::new(move || {
        dev_warn!("[ws] onerror (attempt={attempt})");
    });
    ws.set_onerror(Some(on_err.as_ref().unchecked_ref()));
    on_err.forget();
}

#[allow(clippy::too_many_arguments)]
fn schedule_reconnect(
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    conn: RwSignal<ConnState>,
    toasts: RwSignal<Vec<ToastMsg>>,
    current_game: RwSignal<Option<GameLaunched>>,
    unlocked_profile: RwSignal<Option<UnlockedProfile>>,
    takeover: RwSignal<Option<TakeoverReason>>,
    resume_offer: RwSignal<Option<ResumeOffer>>,
    game_crash: RwSignal<Option<GameCrashReason>>,
    scan_overlay: RwSignal<crate::ScanOverlayState>,
    reconnect_attempts: RwSignal<u32>,
    manual_retry: RwSignal<u32>,
    pending: PendingTimer,
    attempt: u32,
) {
    // Exponential backoff, clamped: 500ms, 1s, 2s, 4s, 8s (max).
    let delay = 500u32.saturating_mul(1 << attempt.min(4));
    let pending_for_cb = pending.clone();
    let cb = Closure::once_into_js(move || {
        // Timer fired — clear the stored handle so a follow-up TRY AGAIN
        // doesn't try to cancel an already-fired timer (no-op but tidy).
        pending_for_cb.set(None);
        spawn_connect(
            portal,
            conn,
            toasts,
            current_game,
            unlocked_profile,
            takeover,
            resume_offer,
            game_crash,
            scan_overlay,
            reconnect_attempts,
            manual_retry,
            pending_for_cb,
            attempt,
        );
    });
    let handle = web_sys::window()
        .unwrap()
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            delay as i32,
        )
        .ok();
    pending.set(handle);
}
