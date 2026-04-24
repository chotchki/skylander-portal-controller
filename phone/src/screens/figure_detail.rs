use leptos::prelude::*;

use crate::api::post_load;
use crate::components::{DisplayHeading, FigureHero, HeadingSize, HeroState};
use crate::model::{GameOfOrigin, PublicFigure, Slot, SlotState, SLOT_COUNT};
use crate::{element_slug, first_empty_slot, push_toast_level, ToastLevel, ToastMsg};

/// Detail view state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailState {
    Default,
    Loading,
    /// Post-load succeeded; playing the reverse-lift exit animation
    /// (`navigation.md` §1 — "PLACE → Portal, reverse lift"). The state
    /// is a brief transitional hold so the animation has time to run
    /// before `on_close` unmounts the overlay.
    Placing,
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
    /// Dismiss the detail view (BACK button path; browse state is
    /// preserved — toy-box lid stays in whatever open state it was).
    on_close: Callback<()>,
    /// Fired after a successful `/api/load` completes + the reverse-lift
    /// animation runs. Browser uses this to both unmount the detail AND
    /// close the toy-box lid (navigation.md §1 — "PLACE → Portal, lid
    /// closes"), which `on_close` deliberately doesn't do.
    on_placed: Callback<()>,
) -> impl IntoView {
    let state = RwSignal::new(DetailState::Default);

    let hero_state = Signal::derive(move || match state.get() {
        DetailState::Default => HeroState::Default,
        DetailState::Loading => HeroState::Loading,
        DetailState::Placing => HeroState::Default,
        DetailState::Errored => HeroState::Errored,
    });

    let viewport_class = move || match state.get() {
        DetailState::Default => "detail-viewport",
        DetailState::Loading => "detail-viewport detail-loading",
        DetailState::Placing => "detail-viewport detail-placing",
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
                        push_toast_level(toasts, "Portal is full \u{2014} remove a figure first.", ToastLevel::Warn);
                        return;
                    }
                },
            };
            picking_for.set(None);
            state.set(DetailState::Loading);

            let fig_id = fig_id.clone();
            let name = name.clone();
            let placed_cb = on_placed.clone();
            leptos::task::spawn_local(async move {
                let result = post_load(slot, &fig_id).await;
                match result {
                    Ok(()) => {
                        // Hold the "placing" state long enough for the
                        // reverse-lift CSS animation in `.detail-placing`
                        // to finish, then unmount + close the lid. Matches
                        // the 560ms animation duration below plus a small
                        // buffer so the fade fully resolves on slower
                        // devices before the overlay unmounts.
                        state.set(DetailState::Placing);
                        crate::gloo_timer(620).await;
                        placed_cb.run(());
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
                    {
                        // Hero portrait + initial fallback. Initial sits
                        // behind the img so a missing scrape (rare —
                        // server falls back to the element icon) still
                        // shows something readable inside the bezel.
                        let hero_id = fig_id.clone();
                        let hero_initial = initial.clone();
                        let hero_src = format!("/api/figures/{}/image?size=hero", hero_id);
                        match element {
                            Some(el) => view! {
                                <FigureHero element=el state=hero_state>
                                    <span class="detail-hero-initial">{hero_initial}</span>
                                    <img
                                        class="detail-hero-image"
                                        src=hero_src
                                        alt=""
                                        loading="eager"
                                        decoding="async"
                                    />
                                </FigureHero>
                            }.into_any(),
                            None => view! {
                                <FigureHero state=hero_state>
                                    <span class="detail-hero-initial">{hero_initial}</span>
                                    <img
                                        class="detail-hero-image"
                                        src=hero_src
                                        alt=""
                                        loading="eager"
                                        decoding="async"
                                    />
                                </FigureHero>
                            }.into_any(),
                        }
                    }
                </div>

                <div class="detail-name-wrap">
                    <DisplayHeading size=HeadingSize::Md>
                        {name_display.to_uppercase()}
                    </DisplayHeading>
                    <div class="detail-meta">{meta_line}</div>
                </div>

                <div class="detail-action-row">
                    <div class="detail-action">
                        <button
                            class="detail-action-btn"
                            disabled=true
                            aria-label="Appearance"
                            title="Appearance"
                        >
                            "\u{2726}"
                        </button>
                        <div class="detail-action-label">"APPEARANCE"</div>
                    </div>
                    <div class="detail-action">
                        <button
                            class="detail-action-btn"
                            disabled=true
                            aria-label="Stats"
                            title="Stats"
                        >
                            "\u{2630}"
                        </button>
                        <div class="detail-action-label">"STATS"</div>
                    </div>
                    <div class="detail-action">
                        <button
                            class="detail-action-btn"
                            disabled=true
                            aria-label="Reset"
                            title="Reset"
                        >
                            "\u{21BA}"
                        </button>
                        <div class="detail-action-label">"RESET"</div>
                    </div>
                </div>

                // Real level/gold/playtime wiring is PLAN 6.3 (post-Kaos
                // polish, Phase 6). The parser + endpoint already exist
                // (6.2); only the phone fetch/render is pending. Until
                // then, show a single muted "coming soon" strip rather
                // than three `--` cells that read as broken data.
                <div class="detail-stats-strip detail-stats-soon">
                    <div class="detail-stats-soon-label">"STATS COMING SOON"</div>
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
        GameOfOrigin::Unknown => "SCANNED",
    }
}
