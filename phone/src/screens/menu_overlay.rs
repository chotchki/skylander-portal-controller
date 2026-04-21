//! Header-kebab menu overlay (PLAN 4.12.4b, design_language.md §6.8).
//!
//! Single surface that consolidates: current-profile chip, join-QR (shell
//! only — real QR content wiring is a post-Phase-4 follow-up tracked as
//! 3.10.8 carryover), and three or four action buttons depending on
//! session state. Mock: `docs/aesthetic/mocks/menu_overlay.html`.
//!
//! Extracted from `screens/modals.rs` per PLAN 4.20.3 — modals.rs was
//! housing 5 unrelated components in one file, MenuOverlay is the largest
//! and the most independent of them.

use std::time::Duration;

use leptos::prelude::*;

use crate::api::{post_quit, post_shutdown};
use crate::components::{ActionButton, ActionVariant};
use crate::gloo_timer;
use crate::model::{GameLaunched, UnlockedProfile};
use crate::{push_toast, ToastMsg};

/// Actions:
///   - SWITCH PROFILE  — single-tap, local re-lock. Clears `unlocked_profile`;
///     the ProfilePicker re-renders. Server-side re-lock endpoint is a
///     follow-up — for now the phone's view falls back to the picker and
///     the next unlock goes through the normal PIN flow.
///   - MANAGE PROFILES — single-tap, raises `manage_gate` so ProfilePicker
///     opens the Konami gate (PLAN 4.18.5a).
///   - HOLD TO SWITCH GAMES — hold-to-confirm, server-impactful. Calls
///     `post_quit(false)` → server quits RPCS3 → WS broadcasts GameStopped
///     → every phone sees the game picker.
///   - HOLD TO SHUT DOWN — hold-to-confirm, danger styling. POSTs
///     `/api/shutdown` which flips the TV launcher into the Farewell
///     surface; egui runs its own farewell countdown and closes the
///     viewport. PLAN 4.15.11.
#[component]
pub(crate) fn MenuOverlay(
    open: RwSignal<bool>,
    unlocked_profile: RwSignal<Option<UnlockedProfile>>,
    current_game: RwSignal<Option<GameLaunched>>,
    manage_gate: RwSignal<bool>,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let initial = Signal::derive(move || {
        unlocked_profile
            .get()
            .and_then(|p| p.display_name.chars().next())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
    });
    let name = Signal::derive(move || {
        unlocked_profile
            .get()
            .map(|p| p.display_name)
            .unwrap_or_default()
    });
    let game_name = Signal::derive(move || {
        current_game
            .get()
            .map(|g| g.display_name)
            .unwrap_or_default()
    });

    let hold_dur = Duration::from_millis(1200);

    // SWITCH PROFILE — single tap. Local re-lock for now.
    let on_switch = Callback::new(move |_| {
        unlocked_profile.set(None);
        open.set(false);
    });

    // MANAGE PROFILES — single tap. Enters the Konami gate (admin profile
    // management). Locks the current session so ProfilePicker mounts, and
    // raises the shared `manage_gate` signal that ProfilePicker reads.
    let on_manage = Callback::new(move |_| {
        unlocked_profile.set(None);
        manage_gate.set(true);
        open.set(false);
    });

    // HOLD TO SWITCH GAMES — fires after 1200ms hold. ActionButton owns
    // the hold timer + .fired animation; we just do the post-fire work:
    // network request + delayed menu close so the .fired flash plays.
    let on_switch_games = Callback::new(move |_| {
        leptos::task::spawn_local(async move {
            if let Err(e) = post_quit(false).await {
                push_toast(toasts, &format!("Quit failed: {e}"));
            }
        });
        leptos::task::spawn_local(async move {
            gloo_timer(380).await;
            open.set(false);
        });
    });

    // HOLD TO SHUT DOWN — POSTs `/api/shutdown` which flips the TV
    // launcher into the Farewell surface; the egui side runs its own
    // ~3s countdown and then closes the viewport. Fire-and-forget — the
    // launcher's death is the user-visible signal.
    let on_shutdown = Callback::new(move |_| {
        leptos::task::spawn_local(async move {
            if let Err(e) = post_shutdown().await {
                push_toast(toasts, &format!("Shutdown failed: {e}"));
            }
        });
        leptos::task::spawn_local(async move {
            gloo_timer(380).await;
            open.set(false);
        });
    });

    view! {
        <Show when=move || open.get() fallback=|| ()>
            <div class="menu-scrim" on:click=move |_| open.set(false)></div>
            <div class="menu-overlay-panel">
                <button class="menu-close" on:click=move |_| open.set(false)>"\u{2715}"</button>

                <Show when=move || unlocked_profile.get().is_some() fallback=|| ()>
                    <div class="menu-current-chip">
                        <div class="menu-current-swatch">{move || initial.get()}</div>
                        <div class="menu-current-meta">
                            <div class="menu-current-name">{move || name.get()}</div>
                            <Show when=move || current_game.get().is_some() fallback=|| ()>
                                <div class="menu-current-game">{move || game_name.get()}</div>
                            </Show>
                        </div>
                    </div>
                </Show>

                <div class="menu-join-card">
                    <div class="menu-join-label">"\u{2316} INVITE A PLAYER"</div>
                    <div class="menu-qr-frame">
                        <div class="menu-qr-inner">"QR"</div>
                    </div>
                    <div class="menu-join-hint">"scan to join this portal"</div>
                </div>

                <div class="menu-actions">
                    <Show when=move || unlocked_profile.get().is_some() fallback=|| ()>
                        <ActionButton
                            title="SWITCH PROFILE"
                            description="Sign back in as someone else"
                            icon="\u{21C4}"
                            on_fire=on_switch
                        />
                    </Show>

                    <ActionButton
                        title="MANAGE PROFILES"
                        description="Grown-ups only \u{00B7} reset PINs, add or remove profiles"
                        icon="\u{2699}"
                        on_fire=on_manage
                    />

                    <Show when=move || current_game.get().is_some() fallback=|| ()>
                        <ActionButton
                            title="HOLD TO SWITCH GAMES"
                            description="Quit the current game and pick a different adventure"
                            icon="\u{25C9}"
                            hold_duration=Some(hold_dur)
                            on_fire=on_switch_games
                        />
                    </Show>

                    <ActionButton
                        title="HOLD TO SHUT DOWN"
                        description="Closes everything \u{00B7} ask a grown-up first"
                        icon="\u{23FB}"
                        variant=ActionVariant::Danger
                        hold_duration=Some(hold_dur)
                        on_fire=on_shutdown
                    />
                </div>
            </div>
        </Show>
    }
}
