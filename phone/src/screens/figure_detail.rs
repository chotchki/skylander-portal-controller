use leptos::prelude::*;

use crate::api::post_load;
use crate::components::{DisplayHeading, FigureHero, HeadingSize, HeroState};
use crate::model::{GameOfOrigin, PublicFigure, Slot, SlotState, SLOT_COUNT};
use crate::{element_slug, first_empty_slot, push_toast, ToastMsg};

/// Detail view state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailState {
    Default,
    Loading,
    Errored,
}

/// Full-screen "lifted" figure detail overlay. Shows the selected figure's
/// hero bezel, metadata, placeholder action buttons, stats strip, and the
/// two primary actions: PLACE ON PORTAL and BACK TO BOX.
///
/// Ghost grid behind is handled by the caller (Browser) via CSS opacity.
#[component]
pub(crate) fn FigureDetail(
    figure: PublicFigure,
    picking_for: RwSignal<Option<u8>>,
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    toasts: RwSignal<Vec<ToastMsg>>,
    /// Set to `None` to close the detail view.
    on_close: Callback<()>,
) -> impl IntoView {
    let state = RwSignal::new(DetailState::Default);

    let hero_state = Signal::derive(move || match state.get() {
        DetailState::Default => HeroState::Default,
        DetailState::Loading => HeroState::Loading,
        DetailState::Errored => HeroState::Errored,
    });

    let viewport_class = move || match state.get() {
        DetailState::Default => "detail-viewport",
        DetailState::Loading => "detail-viewport detail-loading",
        DetailState::Errored => "detail-viewport detail-errored",
    };

    let name = figure.canonical_name.clone();
    let name_display = name.clone();
    let element = figure.element;
    let initial = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    let game = figure.game;
    let fig_id = figure.id.clone();

    let element_label = element
        .map(|e| element_slug(Some(e)).to_uppercase())
        .unwrap_or_else(|| "NONE".to_string());
    let game_label = game_display_name(game);
    let meta_line = format!("{element_label} \u{00B7} {game_label}");

    let error_msg = RwSignal::new(String::new());

    let on_place = {
        let fig_id = fig_id.clone();
        let name = name.clone();
        move |_| {
            if state.get() == DetailState::Loading {
                return;
            }
            // Check if already on portal
            let p = portal.get();
            let already = p.iter().any(|s| match &s.state {
                SlotState::Loaded { display_name, .. } => display_name == &name,
                _ => false,
            });
            if already {
                error_msg.set(format!("{name} is already on the portal."));
                state.set(DetailState::Errored);
                return;
            }
            let slot = match picking_for.get() {
                Some(s) => s,
                None => match first_empty_slot(&p) {
                    Some(s) => s,
                    None => {
                        push_toast(toasts, "Portal is full \u{2014} remove a figure first.");
                        return;
                    }
                },
            };
            picking_for.set(None);
            state.set(DetailState::Loading);

            let fig_id = fig_id.clone();
            let name = name.clone();
            leptos::task::spawn_local(async move {
                let result = post_load(slot, &fig_id).await;
                match result {
                    Ok(()) => {
                        state.set(DetailState::Default);
                    }
                    Err(e) if e.contains("429") => {
                        state.set(DetailState::Default);
                    }
                    Err(e) => {
                        error_msg.set(format!("Failed to place {name}: {e}"));
                        state.set(DetailState::Errored);
                    }
                }
            });
        }
    };

    let on_back = {
        let cb = on_close.clone();
        move |_| {
            cb.run(());
        }
    };

    view! {
        <div class=viewport_class>
            // Error banner
            <div class="detail-error-banner">
                <div class="detail-err-icon">"!"</div>
                <div>{move || error_msg.get()}</div>
            </div>

            <div class="detail-surface framed-panel panel-in">
                <div class="detail-hero-wrap">
                    {match element {
                        Some(el) => view! {
                            <FigureHero element=el state=hero_state>
                                <span>{initial.clone()}</span>
                            </FigureHero>
                        }.into_any(),
                        None => view! {
                            <FigureHero state=hero_state>
                                <span>{initial.clone()}</span>
                            </FigureHero>
                        }.into_any(),
                    }}
                </div>

                <div class="detail-name-wrap">
                    <DisplayHeading size=HeadingSize::Md>
                        {name_display.to_uppercase()}
                    </DisplayHeading>
                    <div class="detail-meta">{meta_line}</div>
                </div>

                <div class="detail-action-row">
                    <button class="detail-action-btn" disabled=true title="Appearance">
                        "\u{2726}"
                    </button>
                    <button class="detail-action-btn" disabled=true title="Stats">
                        "\u{2630}"
                    </button>
                    <button class="detail-action-btn" disabled=true title="Reset">
                        "\u{21BA}"
                    </button>
                </div>

                <div class="detail-stats-strip">
                    <div class="detail-stat-cell">
                        <div class="detail-stat-v">"--"</div>
                        <div class="detail-stat-k">"LEVEL"</div>
                    </div>
                    <div class="detail-stat-cell">
                        <div class="detail-stat-v">"--"</div>
                        <div class="detail-stat-k">"GOLD"</div>
                    </div>
                    <div class="detail-stat-cell">
                        <div class="detail-stat-v">"--"</div>
                        <div class="detail-stat-k">"PLAYED"</div>
                    </div>
                </div>

                <div class="detail-spacer"></div>

                <button
                    class="detail-btn-primary"
                    on:click=on_place
                    disabled=move || state.get() == DetailState::Loading
                >
                    "PLACE ON PORTAL"
                </button>
                <button
                    class="detail-btn-secondary"
                    on:click=on_back
                >
                    "BACK TO BOX"
                </button>
            </div>
        </div>
    }
}

fn game_display_name(g: GameOfOrigin) -> &'static str {
    match g {
        GameOfOrigin::SpyrosAdventure => "SPYRO'S ADVENTURE",
        GameOfOrigin::Giants => "GIANTS",
        GameOfOrigin::SwapForce => "SWAP FORCE",
        GameOfOrigin::TrapTeam => "TRAP TEAM",
        GameOfOrigin::Superchargers => "SUPERCHARGERS",
        GameOfOrigin::Imaginators => "IMAGINATORS",
        GameOfOrigin::CrossGame => "CROSS-GAME",
    }
}
