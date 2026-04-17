//! WebSocket client. Connects on mount, auto-reconnects with backoff,
//! deserialises incoming `Event`s and updates the portal signal.

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, WebSocket};

use crate::api::set_session_id;
use crate::model::{ConnState, Event, GameLaunched, Slot, SlotState, UnlockedProfile, SLOT_COUNT};
use crate::{push_toast, GameCrashReason, ResumeOffer, TakeoverReason, ToastMsg};

pub fn connect(
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    conn: RwSignal<ConnState>,
    toasts: RwSignal<Vec<ToastMsg>>,
    current_game: RwSignal<Option<GameLaunched>>,
    unlocked_profile: RwSignal<Option<UnlockedProfile>>,
    takeover: RwSignal<Option<TakeoverReason>>,
    resume_offer: RwSignal<Option<ResumeOffer>>,
    game_crash: RwSignal<Option<GameCrashReason>>,
) {
    spawn_connect(
        portal,
        conn,
        toasts,
        current_game,
        unlocked_profile,
        takeover,
        resume_offer,
        game_crash,
        0,
    );
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
                attempt,
            );
            return;
        }
    };

    // onopen
    {
        let conn = conn;
        let on_open = Closure::<dyn FnMut()>::new(move || {
            conn.set(ConnState::Connected);
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
        let on_msg = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Some(text) = e.data().as_string() {
                match serde_json::from_str::<Event>(&text) {
                    Ok(Event::Welcome { session_id }) => {
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
                    Err(err) => {
                        web_sys::console::warn_1(&format!("bad ws message: {err} — {text}").into());
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
        let on_close = Closure::<dyn FnMut()>::new(move || {
            conn.set(ConnState::Disconnected);
            schedule_reconnect(
                portal,
                conn,
                toasts,
                current_game,
                unlocked_profile,
                takeover,
                resume_offer,
                game_crash,
                attempt + 1,
            );
        });
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        on_close.forget();
    }

    // onerror — let onclose handle reconnect.
    let on_err = Closure::<dyn FnMut()>::new(|| {
        web_sys::console::warn_1(&"ws error".into());
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
    attempt: u32,
) {
    // Exponential backoff, clamped: 500ms, 1s, 2s, 4s, 8s (max).
    let delay = 500u32.saturating_mul(1 << attempt.min(4));
    let cb = Closure::once_into_js(move || {
        spawn_connect(
            portal,
            conn,
            toasts,
            current_game,
            unlocked_profile,
            takeover,
            resume_offer,
            game_crash,
            attempt,
        );
    });
    let _ = web_sys::window()
        .unwrap()
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            delay as i32,
        );
}
