use leptos::prelude::*;

use crate::api::post_quit;
use crate::model::{ConnState, GameLaunched, UnlockedProfile};
use crate::{push_toast, ToastMsg};

#[component]
pub(crate) fn Header(
    conn: RwSignal<ConnState>,
    current_game: RwSignal<Option<GameLaunched>>,
    toasts: RwSignal<Vec<ToastMsg>>,
    unlocked_profile: RwSignal<Option<UnlockedProfile>>,
) -> impl IntoView {
    let quitting = RwSignal::new(false);
    view! {
        <header class="app-header">
            <div class="header-left">
                <button
                    class="kebab-btn"
                    on:click=move |_| {
                        web_sys::console::log_1(&"kebab menu clicked".into());
                    }
                >
                    {"\u{22EE}"}
                </button>
                <div class="brand">
                    "Skylander Portal"
                    <Show when=move || unlocked_profile.get().is_some() fallback=|| ()>
                        <span class="profile-chip">
                            {move || unlocked_profile.get().map(|p| p.display_name).unwrap_or_default()}
                        </span>
                    </Show>
                    <Show when=move || current_game.get().is_some() fallback=|| ()>
                        <span class="game-name">
                            {move || current_game.get().map(|g| g.display_name).unwrap_or_default()}
                        </span>
                    </Show>
                </div>
            </div>
            <div class="header-right">
                <span class={move || {
                    let cls = match conn.get() {
                        ConnState::Connecting => "connecting",
                        ConnState::Connected => "connected",
                        ConnState::Disconnected => "disconnected",
                    };
                    format!("status-dot {cls}")
                }}></span>
                <span class="status-label">{move || match conn.get() {
                    ConnState::Connecting => "connecting\u{2026}",
                    ConnState::Connected => "connected",
                    ConnState::Disconnected => "disconnected",
                }}</span>
                <Show when=move || current_game.get().is_some() fallback=|| ()>
                    <button
                        class="quit-btn"
                        disabled=move || quitting.get()
                        on:click=move |_| {
                            quitting.set(true);
                            leptos::task::spawn_local(async move {
                                if let Err(e) = post_quit(false).await {
                                    push_toast(toasts, &format!("Quit failed: {e}"));
                                }
                                quitting.set(false);
                            });
                        }
                    >
                        "Quit game"
                    </button>
                </Show>
            </div>
        </header>
    }
}
