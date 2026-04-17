//! Skylander Portal — phone SPA.
//!
//! MVP (Phase 2): connect to the server's WS, show the full figure collection,
//! let the user tap a slot → pick a figure → watch the slot flip
//! Loading → Loaded. The Skylanders aesthetic polish pass comes in Phase 3.

mod api;
pub mod components;
mod model;
mod screens;
mod ws;

use leptos::prelude::*;

use crate::api::{fetch_games, fetch_status};
use crate::model::{
    ConnState, Element, GameLaunched, Slot, SlotState, UnlockedProfile, SLOT_COUNT,
};
use crate::screens::*;

/// A session that got forcibly evicted (server sent `Event::TakenOver`).
/// When this is `Some`, the phone renders the Kaos takeover screen instead
/// of the normal profile/game/portal flow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TakeoverReason {
    pub by_kaos: String,
}

/// Pending "Resume last setup?" offer from `Event::ResumePrompt`. Set on
/// unlock when the profile has a saved layout; cleared when the user picks
/// "Resume" or "Start fresh" (or just dismisses).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResumeOffer {
    pub slots: Vec<SlotState>,
}

#[component]
pub fn App() -> impl IntoView {
    // Read the HMAC key out of `#k=<hex>` before anything else hits the
    // network — `api::sign()` looks at the thread-local this populates, so
    // it must happen before the first fetch. Called on every App() render;
    // the function is idempotent (reads the hash each time, strips it after
    // successful install).
    api::install_key_from_hash();

    let portal = RwSignal::new(empty_portal());
    let picking_for = RwSignal::new(None::<u8>);
    let conn = RwSignal::new(ConnState::Connecting);
    let toasts = RwSignal::new(Vec::<ToastMsg>::new());
    let element_filter = RwSignal::new(None::<Element>);
    let search = RwSignal::new(String::new());
    let current_game = RwSignal::new(None::<GameLaunched>);
    let unlocked_profile = RwSignal::new(None::<UnlockedProfile>);
    let takeover = RwSignal::new(None::<TakeoverReason>);
    let resume_offer = RwSignal::new(None::<ResumeOffer>);
    let menu_open = RwSignal::new(false);
    // Bumps on every profile CRUD so the ProfilePicker re-fetches.
    let profiles_epoch = RwSignal::new(0u32);

    let figures = LocalResource::new(api::fetch_figures);
    let games = LocalResource::new(fetch_games);

    // Fetch the current game on boot; the WS will keep it updated after.
    leptos::task::spawn_local(async move {
        if let Some(g) = fetch_status().await {
            current_game.set(Some(g));
        }
    });

    ws::connect(
        portal,
        conn,
        toasts,
        current_game,
        unlocked_profile,
        takeover,
        resume_offer,
    );

    view! {
        <div class="app">
            <Header conn current_game unlocked_profile menu_open />
            <Show
                when=move || takeover.get().is_none()
                fallback=move || view! { <TakeoverScreen takeover /> }
            >
            <Show
                when=move || unlocked_profile.get().is_some()
                fallback=move || view! {
                    <ProfilePicker toasts profiles_epoch />
                }
            >
            <Show
                when=move || current_game.get().is_some()
                fallback=move || view! {
                    <Suspense fallback=|| view! { <div class="empty-msg">"Loading games…"</div> }>
                        {move || games.get().map(|gs| view! {
                            <GamePicker games=gs.take() toasts />
                        })}
                    </Suspense>
                }
            >
                <Picking picking_for />
                <Portal portal picking_for toasts />
                <Suspense fallback=|| view! { <div class="empty-msg">"Loading figures…"</div> }>
                    {move || figures.get().map(|figs| view! {
                        <Browser
                            figures=figs.take()
                            picking_for
                            portal
                            element_filter
                            search
                            toasts
                        />
                    })}
                </Suspense>
            </Show>
            </Show>
            </Show>
            <Show when=move || resume_offer.get().is_some() fallback=|| ()>
                <ResumeModal resume_offer toasts />
            </Show>
            <MenuOverlay
                open=menu_open
                unlocked_profile
                current_game
                toasts
            />
            <ToastStack toasts />
        </div>
    }
}

// ---------- helpers ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ToastLevel {
    Error,
    Warn,
    Success,
    Info,
}

#[derive(Clone, Debug)]
pub(crate) struct ToastMsg {
    pub(crate) id: u64,
    pub(crate) message: String,
    pub(crate) level: ToastLevel,
}

/// Push an error-level toast (default — matches existing call sites).
pub(crate) fn push_toast(toasts: RwSignal<Vec<ToastMsg>>, message: &str) {
    push_toast_level(toasts, message, ToastLevel::Error);
}

/// Push a toast with an explicit level.
#[allow(dead_code)]
pub(crate) fn push_toast_level(
    toasts: RwSignal<Vec<ToastMsg>>,
    message: &str,
    level: ToastLevel,
) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    // Deduplicate: if an active toast already has this exact message, skip.
    // Prevents spam-click patterns (e.g. repeatedly tapping an already-
    // on-portal card) from stacking identical toasts.
    if toasts.with_untracked(|v| v.iter().any(|t| t.message == message)) {
        return;
    }
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let message = message.to_string();
    toasts.update(|v| v.push(ToastMsg { id, message, level }));
    leptos::task::spawn_local(async move {
        gloo_timer(4000).await;
        toasts.update(|v| v.retain(|t| t.id != id));
    });
}

pub(crate) async fn gloo_timer(ms: i32) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let cb = Closure::once_into_js(move || {
            let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
        });
        let _ = web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                ms,
            );
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

pub(crate) fn first_empty_slot(p: &[Slot; SLOT_COUNT]) -> Option<u8> {
    for (i, s) in p.iter().enumerate() {
        if matches!(s.state, SlotState::Empty) {
            return Some(i as u8 + 1);
        }
    }
    None
}

pub(crate) fn empty_portal() -> [Slot; SLOT_COUNT] {
    std::array::from_fn(|_| Slot {
        state: SlotState::Empty,
    })
}

pub(crate) fn element_slug(e: Option<Element>) -> &'static str {
    match e {
        Some(Element::Air) => "air",
        Some(Element::Dark) => "dark",
        Some(Element::Earth) => "earth",
        Some(Element::Fire) => "fire",
        Some(Element::Life) => "life",
        Some(Element::Light) => "light",
        Some(Element::Magic) => "magic",
        Some(Element::Tech) => "tech",
        Some(Element::Undead) => "undead",
        Some(Element::Water) => "water",
        None => "none",
    }
}

pub(crate) fn element_short(e: Option<Element>) -> &'static str {
    match e {
        Some(Element::Air) => "AIR",
        Some(Element::Dark) => "DRK",
        Some(Element::Earth) => "ERT",
        Some(Element::Fire) => "FIR",
        Some(Element::Life) => "LIF",
        Some(Element::Light) => "LGT",
        Some(Element::Magic) => "MAG",
        Some(Element::Tech) => "TEC",
        Some(Element::Undead) => "UND",
        Some(Element::Water) => "WAT",
        None => "—",
    }
}

pub(crate) fn event_target_value(e: &leptos::ev::Event) -> String {
    use wasm_bindgen::JsCast;
    e.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}

// ---------- entry ----------

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
