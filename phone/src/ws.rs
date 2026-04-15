//! WebSocket client. Connects on mount, auto-reconnects with backoff,
//! deserialises incoming `Event`s and updates the portal signal.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{MessageEvent, WebSocket};

use crate::model::{ConnState, Event, Slot, SlotState, SLOT_COUNT};
use crate::{ToastMsg, push_toast};

pub fn connect(
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    conn: RwSignal<ConnState>,
    toasts: RwSignal<Vec<ToastMsg>>,
) {
    spawn_connect(portal, conn, toasts, 0);
}

fn spawn_connect(
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    conn: RwSignal<ConnState>,
    toasts: RwSignal<Vec<ToastMsg>>,
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
            schedule_reconnect(portal, conn, toasts, attempt);
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
        let on_msg = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Some(text) = e.data().as_string() {
                match serde_json::from_str::<Event>(&text) {
                    Ok(Event::PortalSnapshot { slots }) => {
                        let mut arr: [Slot; SLOT_COUNT] =
                            std::array::from_fn(|_| Slot { state: SlotState::Empty });
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
                    Err(err) => {
                        web_sys::console::warn_1(
                            &format!("bad ws message: {err} — {text}").into(),
                        );
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
        let on_close = Closure::<dyn FnMut()>::new(move || {
            conn.set(ConnState::Disconnected);
            schedule_reconnect(portal, conn, toasts, attempt + 1);
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

fn schedule_reconnect(
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    conn: RwSignal<ConnState>,
    toasts: RwSignal<Vec<ToastMsg>>,
    attempt: u32,
) {
    // Exponential backoff, clamped: 500ms, 1s, 2s, 4s, 8s (max).
    let delay = 500u32.saturating_mul(1 << attempt.min(4));
    let cb = Closure::once_into_js(move || {
        spawn_connect(portal, conn, toasts, attempt);
    });
    let _ = web_sys::window()
        .unwrap()
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            delay as i32,
        );
}
