//! Phase 1 spike 1e — Leptos CSR smoke test.
//!
//! Mounts a tiny Leptos component to <body>. Opens a WebSocket to `/ws` on the
//! same host, shows a ping button, prints incoming/outgoing messages. Proves
//! that Leptos builds to wasm32-unknown-unknown via trunk and can round-trip
//! messages with the Axum server in the main crate.

use leptos::prelude::*;
use leptos::web_sys::{self, MessageEvent, WebSocket};
use leptos::wasm_bindgen::{JsCast, prelude::Closure};

#[component]
fn App() -> impl IntoView {
    let (status, set_status) = signal("connecting…".to_string());
    let (log, set_log) = signal(String::new());
    let ws_holder: std::rc::Rc<std::cell::RefCell<Option<WebSocket>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    let href = web_sys::window().unwrap().location();
    let host = href.host().unwrap_or_else(|_| "localhost".into());
    let scheme = if href.protocol().unwrap_or_default() == "https:" {
        "wss://"
    } else {
        "ws://"
    };
    let url = format!("{scheme}{host}/ws");

    // Connect.
    {
        let ws = WebSocket::new(&url).expect("ws new");
        let ws_clone = ws.clone();

        let on_open = Closure::<dyn FnMut()>::new({
            let set_status = set_status;
            let set_log = set_log;
            move || {
                set_status.set("connected".into());
                set_log.update(|s| s.push_str("ws open\n"));
            }
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget();

        let on_msg = Closure::<dyn FnMut(MessageEvent)>::new({
            let set_log = set_log;
            move |e: MessageEvent| {
                if let Some(txt) = e.data().as_string() {
                    set_log.update(|s| {
                        s.push_str("server: ");
                        s.push_str(&txt);
                        s.push('\n');
                    });
                }
            }
        });
        ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
        on_msg.forget();

        *ws_holder.borrow_mut() = Some(ws_clone);
    }

    let send_ping = {
        let ws_holder = ws_holder.clone();
        move |_| {
            if let Some(ws) = ws_holder.borrow().as_ref() {
                let msg = format!("ping from leptos");
                if ws.send_with_str(&msg).is_ok() {
                    set_log.update(|s| {
                        s.push_str("me: ");
                        s.push_str(&msg);
                        s.push('\n');
                    });
                }
            }
        }
    };

    view! {
        <h1>"Skylander Portal — Phone Smoke"</h1>
        <div>{move || status.get()}</div>
        <button on:click=send_ping>"send ping"</button>
        <div class="log">{move || log.get()}</div>
    }
}

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
