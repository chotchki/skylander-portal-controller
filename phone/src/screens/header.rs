use leptos::prelude::*;

use crate::model::{ConnState, GameLaunched, UnlockedProfile};

#[component]
pub(crate) fn Header(
    conn: RwSignal<ConnState>,
    current_game: RwSignal<Option<GameLaunched>>,
    unlocked_profile: RwSignal<Option<UnlockedProfile>>,
    menu_open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <header class="app-header">
            <div class="header-left">
                <button
                    class="kebab-btn"
                    aria-label="Open menu"
                    on:click=move |_| menu_open.update(|o| *o = !*o)
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
            </div>
        </header>
    }
}
