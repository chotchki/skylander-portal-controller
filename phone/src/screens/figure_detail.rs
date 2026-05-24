use leptos::prelude::*;

use crate::api::{fetch_figure_stats, post_load, FigureStats};
use crate::components::{DisplayHeading, FigureHero, HeadingSize, HeroState};
use crate::model::{Category, GameOfOrigin, PublicFigure, Slot, SlotState, UnlockedProfile, SLOT_COUNT};
use crate::screens::FigureEditSheet;
use crate::{element_slug, first_empty_slot, push_toast_level, ResetTarget, ToastLevel, ToastMsg};

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
    /// Currently unlocked profile — its `id` keys the per-figure
    /// stats fetch (PLAN 6.3). Working copies are stored at
    /// `working/<profile_id>/<figure_id>.sky`, so without the
    /// profile we'd have nothing to read. `None` while the user
    /// is on the join screen.
    unlocked_profile: RwSignal<Option<UnlockedProfile>>,
    /// Shared signal driving the app-root `ResetConfirmModal`. RESET
    /// on this screen sets it with `slot: None` so the modal routes
    /// to the figure-keyed `/reset` endpoint (PLAN 11.12).
    reset_target: RwSignal<Option<ResetTarget>>,
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

    // PLAN 11 — bump on edit save so the stats LocalResource re-runs and
    // picks up the new working-copy values without a manual refetch handle.
    let stats_rev = RwSignal::new(0u32);

    // PLAN 11 — set true when the user taps STATS to open the edit sheet
    // overlay. Sheet closes by setting back to false.
    let show_edit_sheet = RwSignal::new(false);

    // Per-figure stats (PLAN 6.3). Fetches the working copy's parsed
    // level/gold/playtime/nickname when both an unlocked profile and
    // figure id are available. `None` outcome (no working copy yet,
    // unparseable .sky, transport error) falls back to the placeholder
    // strip — the whole experience stays usable when stats aren't
    // available, the placeholder just makes that absence legible.
    let stats_fig_id = fig_id.clone();
    let stats: LocalResource<Option<FigureStats>> = LocalResource::new(move || {
        // Re-fetch when stats_rev bumps (post-edit-save).
        let _rev = stats_rev.get();
        let profile_id = unlocked_profile.get().map(|p| p.id);
        let fig_id = stats_fig_id.clone();
        async move {
            match profile_id {
                Some(pid) => fetch_figure_stats(&pid, &fig_id).await,
                None => None,
            }
        }
    });

    // PLAN 11 — editability gate for the STATS button.
    // (a) Category must be a player-controllable kind (figure / sidekick /
    //     giant / kaos). Categories with no level/gold semantics (Trap,
    //     Vehicle, CreationCrystal, AdventurePack, Item, Other) stay disabled.
    // (b) Figure must not currently occupy any portal slot — editing the
    //     working copy under the game's nose would let it see stale state
    //     after the next read; user must clear the slot first.
    let editable_category = matches!(
        figure.category,
        Category::Figure | Category::Sidekick | Category::Giant | Category::Kaos
    );
    let fig_id_for_portal_check = fig_id.clone();
    let on_portal = Signal::derive(move || {
        portal.get().iter().any(|slot| match &slot.state {
            SlotState::Loaded {
                figure_id: Some(id),
                ..
            } => id == &fig_id_for_portal_check,
            SlotState::Loading {
                figure_id: Some(id),
                ..
            } => id == &fig_id_for_portal_check,
            _ => false,
        })
    });
    let stats_editable = Signal::derive(move || editable_category && !on_portal.get());
    let stats_tooltip = move || {
        if !editable_category {
            format!("Editing not supported for {:?}", figure.category)
        } else if on_portal.get() {
            "Remove from portal before editing".to_string()
        } else {
            "Edit level + gold".to_string()
        }
    };

    let max_level = max_level_for_game(game);

    // Owned clones for the edit sheet + reset closures; the view! macro
    // below moves the original `name_display` and `fig_id` into the main
    // render closure.
    let edit_name = name_display.clone();
    let edit_fig_id = fig_id.clone();
    let reset_name = name_display.clone();
    let reset_fig_id = fig_id.clone();

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
                        push_toast_level(
                            toasts,
                            "Portal is full \u{2014} remove a figure first.",
                            ToastLevel::Warn,
                        );
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
                // PLAN 9.7 playtest 2026-05-04 — opaque blue card behind
                // the badge/name/actions/stats so the content reads on a
                // solid surface instead of fighting the framed-panel's
                // gold pseudo. Same width as the PLACE/BACK buttons
                // below; spacer + buttons stay outside the card so they
                // sit on the gold panel as before.
                <div class="detail-content-card">
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
                            disabled=move || !stats_editable.get()
                            aria-label="Edit stats"
                            title=stats_tooltip
                            on:click=move |_| show_edit_sheet.set(true)
                        >
                            "\u{2630}"
                        </button>
                        <div class="detail-action-label">"STATS"</div>
                    </div>
                    <div class="detail-action">
                        <button
                            class="detail-action-btn"
                            disabled=move || !stats_editable.get()
                            aria-label="Reset to pack-fresh"
                            title=stats_tooltip
                            on:click=move |_| {
                                let pid = match unlocked_profile.get() {
                                    Some(p) => p.id,
                                    None => return,
                                };
                                reset_target.set(Some(ResetTarget {
                                    slot: None,
                                    profile_id: pid,
                                    figure_id: reset_fig_id.clone(),
                                    display_name: reset_name.clone(),
                                }));
                            }
                        >
                            "\u{21BA}"
                        </button>
                        <div class="detail-action-label">"RESET"</div>
                    </div>
                </div>

                // PLAN 6.3 — three stat cells (Level / Gold / Playtime)
                // populated from the per-figure `.sky` working copy. When
                // the fetch hasn't resolved yet, returns None (no working
                // copy, never played, parse failure), or the figure
                // simply doesn't have stats yet, fall back to the muted
                // "STATS COMING SOON" strip — that path is still useful
                // for figures the profile has never placed.
                {move || match stats.get().as_deref().and_then(|opt| opt.as_ref().cloned()) {
                    Some(s) => view! {
                        <div class="detail-stats-strip">
                            <div class="detail-stat-cell">
                                <div class="detail-stat-l">"LEVEL"</div>
                                <div class="detail-stat-v">{s.level.to_string()}</div>
                            </div>
                            <div class="detail-stat-cell">
                                <div class="detail-stat-l">"GOLD"</div>
                                <div class="detail-stat-v">{s.gold.to_string()}</div>
                            </div>
                            <div class="detail-stat-cell">
                                <div class="detail-stat-l">"PLAYTIME"</div>
                                <div class="detail-stat-v">{format_playtime(s.playtime_secs)}</div>
                            </div>
                        </div>
                    }.into_any(),
                    None => view! {
                        <div class="detail-stats-strip detail-stats-soon">
                            <div class="detail-stats-soon-label">"NEVER PLACED"</div>
                        </div>
                    }.into_any(),
                }}
                </div>  // end .detail-content-card

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

            // PLAN 11 — edit sheet overlay. Mounted only when the user taps
            // STATS. Seeds steppers from the current stats (or 1/0 if the
            // figure has no working copy yet — first edit forks from pack).
            {
                let edit_pid_fn = move || unlocked_profile.get().map(|p| p.id);
                move || {
                    if !show_edit_sheet.get() {
                        return ().into_any();
                    }
                    let Some(pid) = edit_pid_fn() else {
                        // Should not happen — STATS button is hidden until
                        // a profile is unlocked, but be defensive.
                        show_edit_sheet.set(false);
                        return ().into_any();
                    };
                    let (initial_level, initial_gold) = stats
                        .get()
                        .as_deref()
                        .and_then(|opt| opt.as_ref())
                        .map(|s| (s.level, s.gold))
                        .unwrap_or((1, 0));
                    view! {
                        <FigureEditSheet
                            figure_name=edit_name.clone()
                            profile_id=pid
                            figure_id=edit_fig_id.clone()
                            initial_level=initial_level
                            initial_gold=initial_gold
                            max_level=max_level
                            on_close=Callback::new(move |_| show_edit_sheet.set(false))
                            on_saved=Callback::new(move |_| stats_rev.update(|n| *n += 1))
                        />
                    }.into_any()
                }
            }
        </div>
    }
}

/// Per-figure level cap derived from game-of-origin. Mirrors
/// `skylander_sky_parser::max_level_for(SkyGeneration)` server-side. Earlier
/// generations cap lower because the parser only reads earlier-era XP slots
/// for those figures (a Giants figure's "level" is computed from xp_2011
/// alone — see `docs/research/sky-format/SkylanderFormat.md` "Write path notes").
fn max_level_for_game(g: GameOfOrigin) -> u8 {
    match g {
        GameOfOrigin::SpyrosAdventure | GameOfOrigin::Giants => 10,
        GameOfOrigin::SwapForce => 15,
        GameOfOrigin::TrapTeam | GameOfOrigin::Superchargers | GameOfOrigin::Imaginators => 20,
        // CrossGame / Unknown — permissive (matches server-side game_to_generation
        // which maps to SkyGeneration::Unknown → max 20).
        _ => 20,
    }
}

/// Compact playtime formatter for the stats strip. The `.sky` payload
/// stores total seconds and fresh figures will be on the order of
/// minutes, while well-played ones can pile up to dozens of hours, so
/// the format adapts: < 1h = "Xm", < 24h = "Xh Ym", longer = "Xh".
fn format_playtime(secs: u32) -> String {
    let total_min = secs / 60;
    let h = total_min / 60;
    let m = total_min % 60;
    if h == 0 {
        format!("{m}m")
    } else if h < 100 {
        format!("{h}h {m}m")
    } else {
        format!("{h}h")
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
