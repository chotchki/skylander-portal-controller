//! Skylander Portal — phone SPA.
//!
//! MVP (Phase 2): connect to the server's WS, show the full figure collection,
//! let the user tap a slot → pick a figure → watch the slot flip
//! Loading → Loaded. The Skylanders aesthetic polish pass comes in Phase 3.

mod api;
pub mod components;
pub mod dev_log;
mod model;
mod pwa;
mod screens;
mod ws;

use leptos::prelude::*;

use crate::api::{fetch_games, fetch_status, fetch_version_check, VersionCheck, BUILD_TOKEN};
use crate::components::{
    ConnectionLost, GameCrashScreen, Header, KaosOverlay, PairingRequired, ScanOverlay,
    StaleVersion,
};
use crate::model::{
    Category, ConnState, Element, GameLaunched, GameOfOrigin, PublicProfile, Slot, SlotState,
    UnlockedProfile, SLOT_COUNT,
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

/// The emulator died. Set when the server broadcasts `Event::GameCrashed`
/// (PLAN 4.15.14); cleared when a new `GameChanged { current: Some(_) }`
/// arrives or the user taps "RETURN TO GAMES" in the overlay. The `message`
/// is the short diagnostic the server produced (`"<Game> exited
/// unexpectedly"`); rendered underneath the heading for players who want
/// context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GameCrashReason {
    pub message: String,
}

/// NFC scan-to-import overlay state (PLAN 6.5.2). `Closed` is the default;
/// tapping the `+` card in the toy box flips to `Prompt`; the WS handler
/// for [`Event::FigureScanned`] flips `Prompt → Success` so the user sees
/// what landed. The overlay component owns its own timeout timer that
/// bumps `Prompt → Timeout` after ~30s of no scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScanOverlayState {
    Closed,
    Prompt,
    Success {
        display_name: String,
        /// Server says the tag has been scanned before (the `<uid>.sky`
        /// existed prior to this dump). Drives copy: "Added to your
        /// collection" vs "Already in your collection".
        is_duplicate: bool,
    },
    Timeout,
}

/// In-flight "reset this figure to a fresh copy?" prompt. Set when the user
/// taps RESET on a loaded slot; cleared on cancel, fire, or modal dismiss.
/// `slot` is 1-indexed (matches the server route). `display_name` is what
/// the modal heading uses ("All of <NAME>'s treasure ...").
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResetTarget {
    pub slot: u8,
    pub figure_id: String,
    pub display_name: String,
}

/// Direction of the most recent screen-stack transition. Drives the
/// per-screen entrance animation (slide-up for going deeper, slide-down
/// for coming back). Set by effects that watch `unlocked_profile` and
/// `current_game` for None↔Some flips. PLAN 4.14.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavDir {
    Forward,
    Back,
}
impl NavDir {
    fn class(self) -> &'static str {
        match self {
            NavDir::Forward => "screen-fwd",
            NavDir::Back => "screen-back",
        }
    }
}

#[component]
pub fn App() -> impl IntoView {
    // Read the HMAC key out of `#k=<hex>` before anything else hits the
    // network — `api::sign()` looks at the thread-local this populates, so
    // it must happen before the first fetch. Called on every App() render;
    // the function is idempotent.
    api::install_key_from_hash();
    // Version handshake state — driven by `fetch_version_check()` at
    // app mount + after every successful WS reconnect. `VersionCheck`
    // collapses the two failure modes we want to raise loudly:
    //   * Unauthorized → key is missing or the server rejected it →
    //     fold into the existing PairingRequired overlay so both
    //     "never paired" and "paired but key invalidated" read the
    //     same to the user (re-scan the QR).
    //   * Stale { server_token } → our bundle's BUILD_TOKEN doesn't
    //     match the server's → show the StaleVersion overlay so the
    //     user refreshes (fixes iOS PWA cached-wasm drift that
    //     previously caused silent "the PIN doesn't work" dead-ends).
    let version_check = RwSignal::new(VersionCheck::Pending);
    let run_version_check = move || {
        leptos::task::spawn_local(async move {
            let outcome = fetch_version_check().await;
            version_check.set(outcome);
        });
    };
    run_version_check();
    // Re-run the handshake on every WS reconnect. A server that
    // restarts with a new build will drop existing connections and
    // come back up; when the phone reconnects we re-probe so a stale
    // bundle becomes visible within seconds of the new server
    // binary's first boot, not on next manual refresh.
    let pairing_required: Signal<bool> = Signal::derive(move || {
        // Local miss OR server-side rejection both route here. During
        // the handshake's in-flight window (`Pending`) we fall back to
        // the local-only check so the overlay doesn't flicker on
        // every load.
        match version_check.get() {
            VersionCheck::Unauthorized => true,
            VersionCheck::Pending => !api::has_hmac_key(),
            _ => false,
        }
    });
    let stale_version_visible: Signal<bool> = Signal::derive(move || {
        matches!(version_check.get(), VersionCheck::Stale { .. })
    });
    let server_build_token: Signal<Option<String>> = Signal::derive(move || match version_check
        .get()
    {
        VersionCheck::Stale { server_token } => Some(server_token),
        _ => None,
    });
    // Fire up the phone→server log forwarder. Mirrors console output to
    // the launcher process so on-device debugging doesn't need a Mac +
    // Web Inspector. See `dev_log.rs`.
    dev_log::start_flusher();

    let portal = RwSignal::new(empty_portal());
    let picking_for = RwSignal::new(None::<u8>);
    let conn = RwSignal::new(ConnState::Connecting);
    let toasts = RwSignal::new(Vec::<ToastMsg>::new());
    let element_filter = RwSignal::new(None::<Element>);
    let game_filter = RwSignal::new(None::<GameOfOrigin>);
    let category_filter = RwSignal::new(None::<Category>);
    let search = RwSignal::new(String::new());
    let current_game = RwSignal::new(None::<GameLaunched>);
    let unlocked_profile = RwSignal::new(None::<UnlockedProfile>);
    let takeover = RwSignal::new(None::<TakeoverReason>);
    let resume_offer = RwSignal::new(None::<ResumeOffer>);
    let game_crash = RwSignal::new(None::<GameCrashReason>);
    let scan_overlay = RwSignal::new(ScanOverlayState::Closed);
    let reset_target = RwSignal::new(None::<ResetTarget>);
    let menu_open = RwSignal::new(false);
    // Konami-gate trigger shared between the kebab menu's MANAGE PROFILES action
    // and the ProfilePicker that owns the gate UI. Set from MenuOverlay,
    // read+reset inside ProfilePicker.
    let manage_gate = RwSignal::new(false);
    let nav_dir = RwSignal::new(NavDir::Forward);
    // Bumps on every profile CRUD so the ProfilePicker re-fetches.
    let profiles_epoch = RwSignal::new(0u32);
    // Flat list of known profiles, refreshed on mount and every time
    // `profiles_epoch` bumps. Read by the portal's per-slot ownership
    // indicator (PLAN 4.18.17) which resolves `placed_by` → color +
    // initial so both co-op players can tell whose figure is whose.
    let known_profiles: RwSignal<Vec<PublicProfile>> = RwSignal::new(Vec::new());
    Effect::new(move |_| {
        // Track so a CRUD bump re-fetches. The initial run hits the
        // server once on mount; subsequent runs only on explicit bumps.
        let _ = profiles_epoch.get();
        leptos::task::spawn_local(async move {
            known_profiles.set(api::fetch_profiles().await);
        });
    });
    // Failed-WS-reconnect counter (written by ws.rs, read by ConnectionLost
    // to decide when to surface the manual TRY AGAIN button) and a bump
    // counter the user fires from that button (watched by ws.rs to cancel
    // the pending backoff and reconnect immediately). PLAN 4.18.21.
    let reconnect_attempts = RwSignal::new(0u32);
    let manual_retry = RwSignal::new(0u32);

    // Re-fetch the figure list whenever the current game changes —
    // the server filters `/api/figures` by what's compatible with the
    // running game, so a stale list (fetched before launch, or
    // fetched while a different game was running) shows figures the
    // current game doesn't support. Tracking `current_game` inside
    // the LocalResource fetcher kicks a refresh on every transition.
    // Chris flagged 2026-04-25 ("still seeing sensei in Trap Team").
    let figures = LocalResource::new(move || {
        let _ = current_game.get();
        api::fetch_figures()
    });
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
        game_crash,
        scan_overlay,
        reconnect_attempts,
        manual_retry,
    );

    // Watch `conn` for transitions into Connected (i.e. WS onopen after
    // any reconnect or the initial boot) and re-fire the version
    // handshake. Uses a Cell-held prior state so every transition
    // fires once — not on unrelated re-renders.
    {
        use std::cell::Cell;
        let prev = Cell::new(conn.get_untracked());
        Effect::new(move |_| {
            let now = conn.get();
            let was = prev.replace(now);
            if matches!(now, ConnState::Connected) && !matches!(was, ConnState::Connected) {
                run_version_check();
            }
        });
    }

    // Track depth-stack direction for screen entrance animations.
    // unlocked_profile None→Some and current_game None→Some are "deeper";
    // the reverse flips are "back". Effects use Cell to remember prior
    // value so we can detect the direction of change without a separate
    // signal. PLAN 4.14.1.
    {
        use std::cell::Cell;
        let prev_unlocked = Cell::new(unlocked_profile.get_untracked().is_some());
        Effect::new(move |_| {
            let now = unlocked_profile.get().is_some();
            if now != prev_unlocked.get() {
                nav_dir.set(if now { NavDir::Forward } else { NavDir::Back });
                prev_unlocked.set(now);
            }
        });
        let prev_game = Cell::new(current_game.get_untracked().is_some());
        Effect::new(move |_| {
            let now = current_game.get().is_some();
            if now != prev_game.get() {
                nav_dir.set(if now { NavDir::Forward } else { NavDir::Back });
                prev_game.set(now);
            }
        });
    }

    // PLAN 4.18.29: reset browser filters on any game change. The search
    // field + element/game/category drill-downs persist across game
    // transitions by default (they live at app level), which means a
    // query typed while Giants was booted keeps filtering the library
    // after switching to Trap Team. Fire an Effect that clears all four
    // on every `current_game` transition *after* the app's initial mount,
    // so booting into an already-running game doesn't clobber a fresh
    // Signal value the user never touched. Compares by serial so
    // re-picking the same game (unlikely but harmless) is a no-op.
    {
        use std::cell::{Cell, RefCell};
        let first_run = Cell::new(true);
        let prev_serial: RefCell<Option<String>> =
            RefCell::new(current_game.get_untracked().map(|g| g.serial));
        Effect::new(move |_| {
            let now_serial = current_game.get().map(|g| g.serial);
            if first_run.replace(false) {
                *prev_serial.borrow_mut() = now_serial;
                return;
            }
            let prev = prev_serial.replace(now_serial.clone());
            if prev != now_serial {
                search.set(String::new());
                element_filter.set(None);
                game_filter.set(None);
                category_filter.set(None);
            }
        });
    }

    // Helper: capture nav_dir at the moment the wrapper mounts so the class
    // (and its CSS animation) reflects the direction of the transition that
    // brought this screen on-screen, not subsequent direction changes.
    let screen_cls = move |extra: &str| {
        let dir = nav_dir.get_untracked().class();
        format!("screen {dir} {extra}")
    };

    view! {
        <div class="app">
            <MagicDust />
            <Header conn current_game unlocked_profile menu_open />
            // Modal stacking per `docs/aesthetic/navigation.md` §3.8:
            //   1. ConnectionLost  (rendered last → highest z; preempts all)
            //   2. GameCrashed     (full-screen, preempts takeover + normal flow)
            //   3. KaosTakeover    (below)
            // GameCrashed is placed *outside* takeover so a crash during a
            // takeover still wins — the portal is dead, nothing else matters.
            // ConnectionLost lives outside this Show stack entirely (see the
            // bottom of this view) so it can overlay any of these states
            // without restructuring the route flow.
            <Show
                when=move || game_crash.get().is_none()
                fallback=move || view! {
                    <div class={screen_cls("screen-game-crash")}>
                        <GameCrashScreen game_crash current_game toasts />
                    </div>
                }
            >
            <Show
                when=move || takeover.get().is_none()
                fallback=move || view! {
                    <div class={screen_cls("screen-takeover")}>
                        <KaosOverlay takeover />
                    </div>
                }
            >
            <Show
                when=move || unlocked_profile.get().is_some()
                fallback=move || view! {
                    <div class={screen_cls("screen-profile-picker")}>
                        <ProfilePicker toasts profiles_epoch manage_gate />
                    </div>
                }
            >
            <Show
                when=move || current_game.get().is_some()
                fallback=move || view! {
                    <div class={screen_cls("screen-game-picker")}>
                        <Suspense fallback=|| view! { <div class="empty-msg">"Loading games…"</div> }>
                            {move || games.get().map(|gs| view! {
                                <GamePicker games=gs.take() toasts />
                            })}
                        </Suspense>
                    </div>
                }
            >
                <div class={screen_cls("screen-portal")}>
                    <Picking picking_for />
                    <Portal portal picking_for known_profiles />
                    <Suspense fallback=|| view! { <div class="empty-msg">"Loading figures…"</div> }>
                        {move || figures.get().map(|figs| view! {
                            <Browser
                                figures=figs.take()
                                picking_for
                                portal
                                element_filter
                                game_filter
                                category_filter
                                search
                                toasts
                                scan_overlay
                            />
                        })}
                    </Suspense>
                </div>
            </Show>
            </Show>
            </Show>
            </Show>
            <Show when=move || resume_offer.get().is_some() fallback=|| ()>
                <ResumeModal resume_offer unlocked_profile toasts />
            </Show>
            <ResetConfirmModal reset_target toasts />
            <ScanOverlay scan_overlay toasts />
            <MenuOverlay
                open=menu_open
                unlocked_profile
                current_game
                manage_gate
                toasts
            />
            <ToastStack toasts />
            <ConnectionLost reconnect_attempts manual_retry />
            // PairingRequired + StaleVersion render LAST so they sit
            // above everything else in the stacking context. Both are
            // blocking — when either is visible, no other surface can
            // do useful work (signed POSTs would 401; a stale bundle
            // would misbehave unpredictably), so blocking them
            // wholesale is the honest UX.
            <PairingRequired visible=pairing_required />
            <StaleVersion
                visible=stale_version_visible
                local=BUILD_TOKEN
                server=server_build_token
            />
        </div>
    }
}

/// Sparse floating-particle ambient layer (PLAN 4.5.2). Pure-CSS animation
/// per particle; positions and timings are randomised once at App mount so
/// each load has a slightly different rhythm. 24 particles is sparse enough
/// to feel ambient without competing with content.
#[component]
fn MagicDust() -> impl IntoView {
    let particles: Vec<String> = (0..24)
        .map(|_| {
            let left = js_sys::Math::random() * 100.0;
            let size = 1.5 + js_sys::Math::random() * 2.5;
            let dur = 14.0 + js_sys::Math::random() * 16.0;
            let delay = -(js_sys::Math::random() * dur);
            let drift = js_sys::Math::random() * 40.0 - 20.0;
            let opacity = 0.25 + js_sys::Math::random() * 0.45;
            format!(
                "left:{left:.2}%;width:{size:.2}px;height:{size:.2}px;\
                 --drift:{drift:.1}px;--peak-opacity:{opacity:.2};\
                 animation: dust-float {dur:.1}s {delay:.1}s linear infinite;"
            )
        })
        .collect();
    view! {
        <div class="magic-dust" aria-hidden="true">
            {particles.into_iter().map(|style| view! {
                <span class="dust-particle" style=style></span>
            }.into_any()).collect_view()}
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
pub(crate) fn push_toast_level(toasts: RwSignal<Vec<ToastMsg>>, message: &str, level: ToastLevel) {
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
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let cb = Closure::once_into_js(move || {
            let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
        });
        let _ = web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), ms);
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
