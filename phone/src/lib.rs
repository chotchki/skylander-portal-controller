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
    ConnectionLost, GameCrashScreen, Header, KaosOverlay, PairingRequired, Rpcs3SettingsOverlay,
    StaleVersion,
};
use crate::model::{
    Category, ConnState, Element, GameLaunched, GameOfOrigin, PublicProfile, RosterState,
    ServerApp, Slot, SlotState, UnlockedProfile, SLOT_COUNT,
};
use crate::screens::*;

/// A session that got forcibly evicted (server sent `Event::TakenOver`).
/// When this is `Some`, the phone renders the Kaos takeover screen instead
/// of the normal profile/game/portal flow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TakeoverReason {
    pub by_kaos: String,
    /// Wall-clock seconds the kickback button should stay greyed
    /// out before a reload could even reach a free seat. Captured
    /// from `Event::TakenOver`'s `cooldown_remaining_secs` at the
    /// instant we received it; the overlay component re-runs a
    /// 1Hz timer from there. PLAN 8.2a.
    pub cooldown_remaining_secs: u32,
}

/// Pending "Resume last setup?" offer from `Event::ResumePrompt`. Set on
/// unlock when the profile has a saved layout; cleared when the user picks
/// "Resume" or "Start fresh" (or just dismisses).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResumeOffer {
    pub slots: Vec<SlotState>,
}

/// Pending Kaos mid-game swap (PLAN 8.2b.5). Set when the server
/// broadcasts `Event::KaosTaunt`; cleared by a 5s auto-dismiss timer
/// inside the overlay component, or on manual tap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KaosSwapAnnouncement {
    pub taunt: String,
    /// Profile id whose slot got swapped. Today the overlay shows
    /// the same copy for everyone; surfacing the id here lets a
    /// future refinement tint the overlay in the target player's
    /// colour so co-op players can tell it's Alice's figure that
    /// was yanked, not Bob's.
    #[allow(dead_code)]
    pub profile_id: String,
}

/// The emulator died or hung. Set when the server broadcasts
/// `Event::GameCrashed` (terminal — PLAN 4.15.14) or `Event::GameRecovering`
/// (transient auto-recovery — PLAN 16.7.2/.3); cleared when a new
/// `GameChanged { current: Some(_) }` arrives or the user taps "RETURN TO
/// GAMES" in the terminal overlay. The `message` is the short diagnostic the
/// server produced (`"<Game> exited unexpectedly"` / `"… stopped responding"`);
/// rendered underneath the heading for players who want context.
///
/// `recovering` switches `GameCrashScreen` between its two forms: `true` →
/// transient "RECONNECTING…" (spinner, no action button, the server is
/// relaunching); `false` → terminal "GAME CRASHED" (RETURN TO GAMES button).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GameCrashReason {
    pub message: String,
    pub recovering: bool,
}

/// In-flight "reset this figure to a fresh copy?" prompt. Set when the user
/// taps RESET on a loaded slot OR on the figure-detail screen; cleared on
/// cancel, fire, or modal dismiss.
///
/// `slot` is 1-indexed when present (matches the server route for
/// slot-context reset). `None` means the figure is being reset from the
/// figure-detail screen with no portal involvement — the modal routes to
/// the figure-keyed endpoint instead. `profile_id` is required in both
/// cases (the server keys working copies by `(profile_id, figure_id)`).
/// `display_name` is what the modal heading uses ("All of <NAME>'s
/// treasure ...").
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResetTarget {
    pub slot: Option<u8>,
    pub profile_id: String,
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
    let stale_version_visible: Signal<bool> =
        Signal::derive(move || matches!(version_check.get(), VersionCheck::Stale { .. }));
    let server_build_token: Signal<Option<String>> =
        Signal::derive(move || match version_check.get() {
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
    // The controller or guacamole (AA.18, AA.19), from `/api/status` below.
    let server_app = RwSignal::new(ServerApp::default());
    // guacamole's roster state (AE.4); `None` on the product.
    let roster = RwSignal::new(None::<RosterState>);
    let unlocked_profile = RwSignal::new(None::<UnlockedProfile>);
    let takeover = RwSignal::new(None::<TakeoverReason>);
    let kaos_swap = RwSignal::new(None::<KaosSwapAnnouncement>);
    let resume_offer = RwSignal::new(None::<ResumeOffer>);
    let game_crash = RwSignal::new(None::<GameCrashReason>);
    // PLAN 16.9.3 — true while RPCS3's settings GUI is open on the TV (set by the
    // WS `Rpcs3SettingsChanged` handler). Drives the "configuring on the TV…"
    // overlay; the portal is unavailable for its duration.
    let rpcs3_settings_open = RwSignal::new(false);
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
    // PLAN 11.14 — bumps on every WS `Event::FigureUpdated`. figure_detail
    // subscribes alongside its local `stats_rev` so cross-phone edits +
    // the local same-phone race both surface in the stats strip.
    let figure_updates_rev = RwSignal::new(0u32);

    // Re-fetch the figure list whenever the current game changes —
    // the server filters `/api/figures` by what's compatible with the
    // running game, so a stale list (fetched before launch, or
    // fetched while a different game was running) shows figures the
    // current game doesn't support. Tracking `current_game` inside
    // the LocalResource fetcher kicks a refresh on every transition.
    // Chris flagged 2026-04-25 ("still seeing sensei in Trap Team").
    // guacamole's list also lands after boot (AE.4): a roster change refetches.
    let figures = LocalResource::new(move || {
        let _ = current_game.get();
        let _ = roster.get();
        api::fetch_figures()
    });
    let games = LocalResource::new(fetch_games);

    // Fetch the current game on boot; the WS will keep it updated after.
    // The app can't change under a live page: a restarted server sends a new
    // `boot_id`, which reloads the page (`ws.rs`).
    leptos::task::spawn_local(async move {
        if let Some(status) = fetch_status().await {
            server_app.set(status.app);
            roster.set(status.roster);
            if let Some(g) = status.current_game {
                current_game.set(Some(g));
            }
        }
    });

    // AE.4: while guacamole's collection waits on its roster, poll the status
    // so the figures fill in within 2 s of it landing, with no reload.
    {
        let polling = StoredValue::new(false);
        Effect::new(move |_| {
            let waiting = roster.get().and_then(RosterState::note).is_some();
            if !waiting || current_game.get().is_none() || polling.get_value() {
                return;
            }
            polling.set_value(true);
            leptos::task::spawn_local(async move {
                loop {
                    gloo_timer(ROSTER_POLL_MS).await;
                    if let Some(status) = fetch_status().await {
                        if status.roster != roster.get_untracked() {
                            roster.set(status.roster);
                        }
                        if status.roster.and_then(RosterState::note).is_none() {
                            break;
                        }
                    }
                }
                polling.set_value(false);
            });
        });
    }
    let roster_note = Signal::derive(move || roster.get().and_then(RosterState::note));

    ws::connect(
        portal,
        conn,
        toasts,
        current_game,
        unlocked_profile,
        takeover,
        resume_offer,
        game_crash,
        reconnect_attempts,
        manual_retry,
        kaos_swap,
        figure_updates_rev,
        rpcs3_settings_open,
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
                when=move || takeover.get().is_none() && kaos_swap.get().is_none()
                fallback=move || view! {
                    <div class={screen_cls("screen-takeover")}>
                        <KaosOverlay takeover kaos_swap />
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
                                unlocked_profile
                                reset_target
                                figure_updates_rev
                                server_app
                                roster_note
                            />
                        })}
                    </Suspense>
                </div>
            </Show>
            </Show>
            </Show>
            </Show>
            // PLAN 11.15 — only surface the resume prompt once a game
            // has actually launched. Server emits `ResumePrompt` on
            // profile unlock, but RPCS3 isn't up yet so RESUME would
            // fire `post_load`s into a 500. Modal stays queued in
            // `resume_offer` until the user picks + launches a game.
            <Show when=move || resume_offer.get().is_some() && current_game.get().is_some() fallback=|| ()>
                <ResumeModal resume_offer portal known_profiles unlocked_profile toasts />
            </Show>
            <ResetConfirmModal reset_target toasts server_app />
            <Rpcs3SettingsOverlay visible=rpcs3_settings_open />
            <MenuOverlay
                open=menu_open
                unlocked_profile
                current_game
                manage_gate
                toasts
                server_app
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

/// How often the phone asks whether guacamole's roster has landed (AE.4).
const ROSTER_POLL_MS: i32 = 1000;

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

/// Names of the loaded figures `viewer` can't place again under `app`'s rule
/// (AA.18): every one in the product, only the viewer's own in guacamole.
pub(crate) fn placed_names(portal: &[Slot], app: ServerApp, viewer: Option<&str>) -> Vec<String> {
    portal
        .iter()
        .filter_map(|s| match &s.state {
            SlotState::Loaded {
                display_name,
                placed_by,
                ..
            } if app.blocks_place(placed_by.as_deref(), viewer) => Some(display_name.clone()),
            _ => None,
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(name: &str, placed_by: Option<&str>) -> Slot {
        Slot {
            state: SlotState::Loaded {
                figure_id: Some(format!("{}-id", name.to_lowercase())),
                display_name: name.to_string(),
                placed_by: placed_by.map(str::to_string),
            },
        }
    }

    fn loading(placed_by: &str) -> Slot {
        Slot {
            state: SlotState::Loading {
                figure_id: Some("spyro-id".to_string()),
                placed_by: Some(placed_by.to_string()),
            },
        }
    }

    fn empty() -> Slot {
        Slot {
            state: SlotState::Empty,
        }
    }

    /// The product's rule, unchanged: a loaded figure blocks every viewer,
    /// whoever placed it; a loading slot blocks nothing.
    #[test]
    fn the_product_blocks_a_placed_figure_for_everyone() {
        let portal = [
            loaded("Eruptor", Some("alice")),
            loaded("Spyro", None),
            loading("bob"),
            empty(),
        ];
        for viewer in [Some("alice"), Some("bob"), None] {
            assert_eq!(
                placed_names(&portal, ServerApp::Product, viewer),
                ["Eruptor", "Spyro"],
                "{viewer:?}"
            );
        }
    }

    /// guacamole: Bob can put his own Eruptor next to Alice's, and neither
    /// can put theirs on twice.
    #[test]
    fn guacamole_blocks_only_the_viewers_own_placement() {
        let app = ServerApp::Guacamole;
        let alices = [loaded("Eruptor", Some("alice")), empty()];
        assert_eq!(placed_names(&alices, app, Some("alice")), ["Eruptor"]);
        assert!(placed_names(&alices, app, Some("bob")).is_empty());

        let both = [
            loaded("Eruptor", Some("alice")),
            loaded("Eruptor", Some("bob")),
            loaded("Spyro", Some("bob")),
            loading("alice"),
        ];
        assert_eq!(placed_names(&both, app, Some("alice")), ["Eruptor"]);
        assert_eq!(placed_names(&both, app, Some("bob")), ["Eruptor", "Spyro"]);
        assert!(placed_names(&both, app, Some("carol")).is_empty());

        // A placement no profile owns (only a raw load request from a locked
        // session makes one; PLACE shows only with a profile unlocked)
        // leaves every profile free to place theirs.
        let unowned = [loaded("Eruptor", None)];
        assert!(placed_names(&unowned, app, Some("alice")).is_empty());
        assert_eq!(placed_names(&unowned, app, None), ["Eruptor"]);
    }

    /// AH.3: guacamole's 403 and 409 on a figure action in a kid's words, the
    /// slot reset's 409 saying the figure's in the level; anything else, and
    /// everything in the product, keeps the error as it was.
    #[test]
    fn guacamoles_refusals_are_in_a_kids_words() {
        use model::FigureAction::{Edit, Reset, SlotReset};
        let guac = ServerApp::Guacamole;
        for action in [Edit, Reset, SlotReset] {
            let words = guac.refusal(action, "HTTP 403", "Eruptor").unwrap();
            assert!(words.contains("own profile"), "{words}");
            for status in ["HTTP 404", "HTTP 422", "HTTP 500", "no window"] {
                assert_eq!(guac.refusal(action, status, "Eruptor"), None, "{status}");
            }
            for status in ["HTTP 403", "HTTP 409"] {
                assert_eq!(ServerApp::Product.refusal(action, status, "Eruptor"), None);
            }
        }
        for action in [Edit, Reset] {
            let words = guac.refusal(action, "HTTP 409", "Eruptor").unwrap();
            assert!(words.starts_with("Eruptor is on the portal!"), "{words}");
            assert!(words.contains("Take Eruptor off"), "{words}");
        }
        let words = guac.refusal(SlotReset, "HTTP 409", "Tree Rex").unwrap();
        assert!(
            words.starts_with("Tree Rex is out in the level!"),
            "{words}"
        );
    }

    /// AH.37: guacamole's 409 naming the slot that already holds the viewer's
    /// figure reads as already on the portal, in the words the phone's own
    /// check uses; AB.22's 409 and every other answer keep `HTTP <status>`.
    #[test]
    fn a_load_refused_for_a_figure_already_on_the_portal_says_so() {
        use model::{already_on_portal, LoadError};
        assert_eq!(
            LoadError::from_answer(409, r#"{"already_on_portal":3}"#),
            LoadError::AlreadyOnPortal
        );
        assert_eq!(
            already_on_portal("Eruptor"),
            "Eruptor is already on the portal."
        );
        for (status, body) in [
            (409, "slot 2 holds another player's figure"),
            (409, ""),
            (409, r#"{"already_on_portal":0}"#),
            (409, r#"{"already_on_portal":9}"#),
            (429, "slot busy"),
            (404, r#"{"already_on_portal":3}"#),
            (500, "working-copy resolve failed: x"),
        ] {
            assert_eq!(
                LoadError::from_answer(status, body),
                LoadError::Failed(format!("HTTP {status}")),
                "{status} {body}"
            );
        }
    }

    /// AH.3: guacamole's stats answer carries the game's caps, its
    /// never-placed 404 too; the product's `.sky` stats carry none.
    #[test]
    fn the_stats_carry_guacamoles_caps() {
        use api::{FigureStats, GameCaps, StatsFetch};
        let guac: FigureStats = serde_json::from_str(
            r#"{"level":10,"xp":33000,"gold":1234,"playtime_secs":61,"max_level":15,"max_gold":65000}"#,
        )
        .unwrap();
        assert_eq!((guac.level, guac.gold, guac.playtime_secs), (10, 1234, 61));
        let caps = GameCaps {
            max_level: 15,
            max_gold: 65_000,
        };
        assert_eq!(StatsFetch::Read(guac).caps(), Some(caps));
        let product: FigureStats = serde_json::from_str(
            r#"{"level":5,"gold":300,"playtime_secs":0,"nickname":"Spyro","xp_2011":6000}"#,
        )
        .unwrap();
        assert_eq!(product.nickname, "Spyro");
        assert_eq!(StatsFetch::Read(product).caps(), None);
        let never: Option<GameCaps> =
            serde_json::from_str(r#"{"max_level":15,"max_gold":65000}"#).ok();
        assert_eq!(StatsFetch::Missing(never).caps(), Some(caps));
        assert_eq!(StatsFetch::Missing(None).caps(), None);
        assert_eq!(StatsFetch::Unavailable.caps(), None);
    }

    #[test]
    fn only_the_product_offers_the_rpcs3_settings() {
        assert!(ServerApp::Product.offers_rpcs3_settings());
        assert!(!ServerApp::Guacamole.offers_rpcs3_settings());
    }

    /// `/api/status` as each app answers it; a server without `app` is the
    /// product.
    #[test]
    fn the_status_names_the_app() {
        let read = |json: &str| serde_json::from_str::<api::StatusBody>(json).unwrap();
        let guac = read(r#"{"rpcs3_running":false,"current_game":null,"app":"guacamole"}"#);
        assert_eq!(guac.app, ServerApp::Guacamole);
        assert!(guac.current_game.is_none());
        let product = read(
            r#"{"rpcs3_running":true,"current_game":{"serial":"BLUS30968","display_name":"Giants"},"app":"product"}"#,
        );
        assert_eq!(product.app, ServerApp::Product);
        assert_eq!(product.current_game.unwrap().serial, "BLUS30968");
        let old = read(r#"{"rpcs3_running":true,"current_game":null}"#);
        assert_eq!(old.app, ServerApp::Product);
        assert_eq!(old.roster, None);
        assert_eq!(product.roster, None);
    }

    /// AE.4: guacamole's status names its roster state, and the collection
    /// says why it's empty until `ready`.
    #[test]
    fn the_roster_state_reads_and_each_waiting_state_has_its_note() {
        let read = |state: &str| {
            let json = format!(
                r#"{{"rpcs3_running":false,"current_game":null,"app":"guacamole","roster":"{state}"}}"#
            );
            serde_json::from_str::<api::StatusBody>(&json)
                .unwrap()
                .roster
                .unwrap()
        };
        let reading = read("reading");
        assert_eq!(reading, RosterState::Reading);
        assert_eq!(
            reading.note(),
            Some("Reading the figures off your disc\u{2026}")
        );
        let unrecognized = read("unrecognized").note().unwrap();
        assert!(
            unrecognized.contains("isn't one guacamole recognizes"),
            "{unrecognized}"
        );
        let unreadable = read("unreadable").note().unwrap();
        assert!(unreadable.contains("couldn't be read"), "{unreadable}");
        assert_eq!(read("ready").note(), None);
    }
}

// ---------- entry ----------

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
