use leptos::prelude::*;

use crate::components::{BezelSize, GoldBezel};
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
                {move || unlocked_profile.get().map(|profile| {
                    let color = profile.color.clone();
                    let initial = profile
                        .display_name
                        .chars()
                        .next()
                        .unwrap_or('?')
                        .to_uppercase()
                        .collect::<String>();
                    let name = profile.display_name.clone();
                    view! {
                        <div class="header-swatch" style=format!("--profile-color:{color}")>
                            <GoldBezel size=BezelSize::Sm>
                                <span class="header-swatch-initial">{initial}</span>
                            </GoldBezel>
                        </div>
                        <div class="header-identity">
                            <span class="header-profile-name">{name}</span>
                            {move || current_game.get().map(|g| view! {
                                <span class="header-game-name">{g.display_name}</span>
                            })}
                        </div>
                    }.into_any()
                })}
            </div>
            <div class="header-right">
                <span
                    class=move || {
                        let cls = match conn.get() {
                            ConnState::Connecting => "connecting",
                            ConnState::Connected => "connected",
                            ConnState::Disconnected => "disconnected",
                        };
                        format!("status-dot {cls}")
                    }
                    aria-label=move || match conn.get() {
                        ConnState::Connecting => "connecting",
                        ConnState::Connected => "connected",
                        ConnState::Disconnected => "disconnected",
                    }
                    title=move || match conn.get() {
                        ConnState::Connecting => "connecting\u{2026}",
                        ConnState::Connected => "connected",
                        ConnState::Disconnected => "disconnected",
                    }
                ></span>
            </div>
        </header>
    }
}
