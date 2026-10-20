use leptos::prelude::*;

use crate::api::{fetch_figure_stats, post_load, StatsFetch};
use crate::components::{DisplayHeading, FigureHero, HeadingSize, HeroState};
use crate::model::{
    Category, GameOfOrigin, PublicFigure, ServerApp, Slot, SlotState, UnlockedProfile, SLOT_COUNT,
};
use crate::screens::figure_edit_sheet::GOLD_MAX;
use crate::screens::FigureEditSheet;
use crate::{
    element_slug, first_empty_slot, placed_names, push_toast_level, ResetTarget, ToastLevel,
    ToastMsg,
};

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
    /// PLAN 11.14 — bumps on every WS `Event::FigureUpdated`. The
    /// stats `LocalResource` reads this alongside the local
    /// `stats_rev`, so cross-phone edits + the same-phone race both
    /// trigger a re-fetch of the working copy.
    figure_updates_rev: RwSignal<u32>,
    /// The controller or guacamole: whose placements make PLACE refuse this
    /// figure follows its rule (AA.18).
    server_app: RwSignal<ServerApp>,
    /// PLAN 11.16 — variant cluster the current figure belongs to,
    /// sorted "base" first then alphabetical by `variant_tag`. Drives
    /// the APPEARANCE button's enable state + next-pick logic. A
    /// cluster of size 1 means no alternates exist and the button stays
    /// disabled.
    siblings: Vec<PublicFigure>,
    /// PLAN 11.16 — called with the next variant in `siblings` when
    /// the user taps APPEARANCE. Browser owns the `selected_figure`
    /// signal so the swap re-mounts this component with the new
    /// figure_id (fresh stats fetch, fresh edit/reset gates).
    on_pick_variant: Callback<PublicFigure>,
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
    // figure id are available. `Missing` (no working copy yet,
    // unparseable .sky, transport error) falls back to the placeholder
    // strip — the whole experience stays usable when stats aren't
    // available, the placeholder just makes that absence legible.
    // `Unavailable` (AA.9) also turns STATS and RESET off, see `edit_gate`.
    let stats_fig_id = fig_id.clone();
    let stats: LocalResource<StatsFetch> = LocalResource::new(move || {
        // Re-fetch when either rev bumps:
        // - `stats_rev`: local edit-sheet save (instant local feedback path).
        // - `figure_updates_rev`: WS `Event::FigureUpdated` from any phone
        //   (PLAN 11.14 — covers cross-phone refresh + the same-phone race
        //   where the server returns 202 before the working-copy write
        //   strictly hits disk; the WS event fires after the write).
        let _rev = stats_rev.get();
        let _ws_rev = figure_updates_rev.get();
        let profile_id = unlocked_profile.get().map(|p| p.id);
        let fig_id = stats_fig_id.clone();
        async move {
            match profile_id {
                Some(pid) => fetch_figure_stats(&pid, &fig_id).await,
                None => StatsFetch::Missing(None),
            }
        }
    });

    // PLAN 11 + AA.9 — STATS and RESET share one gate, `edit_gate`; under
    // guacamole RESET also resets the viewer's own figure where it stands on
    // the portal (AH.3, `reset_via`).
    let category = figure.category;
    let fig_id_for_portal_check = fig_id.clone();
    let on_portal = Signal::derive(move || {
        let viewer = unlocked_profile.get().map(|p| p.id);
        on_portal_for(
            &portal.get(),
            &fig_id_for_portal_check,
            server_app.get(),
            viewer.as_deref(),
        )
    });
    let gate = Signal::derive(move || {
        let fetched = stats.get();
        edit_gate(
            category,
            on_portal.get(),
            fetched.as_deref(),
            server_app.get(),
        )
    });
    let stats_editable = Signal::derive(move || gate.get().enabled());
    let stats_tooltip = move || gate.get().tooltip();
    let fig_id_for_own_slot = fig_id.clone();
    let reset = Signal::derive(move || {
        let viewer = unlocked_profile.get().map(|p| p.id);
        let own = own_loaded_slot(&portal.get(), &fig_id_for_own_slot, viewer.as_deref());
        reset_via(gate.get(), server_app.get(), own)
    });
    let reset_tooltip = move || match reset.get() {
        Some(ResetVia::Slot(_)) => "Reset to fresh where it stands".to_string(),
        _ => gate.get().tooltip(),
    };

    // A tap during the in-flight fetch can open the sheet or modal before the 409; close them.
    let gate_fig_id = fig_id.clone();
    Effect::new(move |_| {
        if gate.get() != EditGate::NotAvailableYet {
            return;
        }
        show_edit_sheet.set(false);
        let ours = reset_target.with_untracked(|t| is_detail_reset_of(t.as_ref(), &gate_fig_id));
        if ours {
            reset_target.set(None);
        }
    });

    // The steppers' caps: the product's by game of origin, guacamole's the
    // game's own from the stats answer (AH.3).
    let caps = move || edit_caps(server_app.get(), stats.get().as_deref(), game);

    // PLAN 11.16 — APPEARANCE button enable + next-variant resolution.
    // Cluster of size ≤ 1 means no alternates → keep the button disabled.
    // Otherwise tapping it picks the next sibling in display order, which
    // re-mounts this component with the new figure_id via Browser's
    // `selected_figure` signal.
    let variant_count = siblings.len();
    let has_alternates = variant_count > 1;
    let current_variant_idx = siblings.iter().position(|f| f.id == fig_id).unwrap_or(0);
    let appearance_tooltip = if has_alternates {
        format!("Switch appearance ({variant_count} variants)")
    } else {
        "No other appearances available".to_string()
    };
    let on_pick_next = {
        let siblings = siblings.clone();
        move |_| {
            if siblings.len() <= 1 {
                return;
            }
            let next_idx = (current_variant_idx + 1) % siblings.len();
            on_pick_variant.run(siblings[next_idx].clone());
        }
    };

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
            // Already on the portal: whoever placed it in the product, only
            // this viewer's own placement in guacamole (AA.18).
            let p = portal.get();
            let viewer = unlocked_profile.get().map(|u| u.id);
            if placed_names(&p, server_app.get(), viewer.as_deref()).contains(&name) {
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
                            disabled=!has_alternates
                            aria-label="Switch appearance"
                            title=appearance_tooltip
                            on:click=on_pick_next
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
                            disabled=move || reset.get().is_none()
                            aria-label="Reset to pack-fresh"
                            title=reset_tooltip
                            on:click=move |_| {
                                let pid = match unlocked_profile.get() {
                                    Some(p) => p.id,
                                    None => return,
                                };
                                let slot = match reset.get() {
                                    Some(ResetVia::Figure) => None,
                                    Some(ResetVia::Slot(n)) => Some(n),
                                    None => return,
                                };
                                reset_target.set(Some(ResetTarget {
                                    slot,
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
                // the fetch hasn't resolved yet or came back `Missing` (no
                // working copy, never played, parse failure), fall back to
                // the muted "NEVER PLACED" strip. `Unavailable` (AA.9: no
                // figure files on this server) says so instead, which is
                // the visible note for the disabled STATS + RESET above.
                {move || match stats.get().as_deref().cloned() {
                    Some(StatsFetch::Read(s)) => view! {
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
                    Some(StatsFetch::Unavailable) => view! {
                        <div class="detail-stats-strip detail-stats-soon">
                            <div class="detail-stats-soon-label">"NOT AVAILABLE YET"</div>
                        </div>
                    }.into_any(),
                    Some(StatsFetch::Missing(_)) | None => view! {
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
                        .and_then(StatsFetch::stats)
                        .map(|s| (s.level, s.gold))
                        .unwrap_or((1, 0));
                    let (max_level, max_gold) = caps();
                    view! {
                        <FigureEditSheet
                            figure_name=edit_name.clone()
                            profile_id=pid
                            figure_id=edit_fig_id.clone()
                            initial_level=initial_level
                            initial_gold=initial_gold
                            max_level=max_level
                            max_gold=max_gold
                            server_app=server_app.get_untracked()
                            toasts
                            on_close=Callback::new(move |_| show_edit_sheet.set(false))
                            on_saved=Callback::new(move |_| stats_rev.update(|n| *n += 1))
                        />
                    }.into_any()
                }
            }
        </div>
    }
}

/// Why STATS (level + gold edit) and RESET TO FRESH are on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditGate {
    Editable,
    /// No level/gold semantics (Trap, Vehicle, CreationCrystal,
    /// AdventurePack, Item, Other).
    UnsupportedCategory(Category),
    /// The server can't read or write this figure's progress yet (guacamole
    /// before its extract holds the game's table), so edit and reset would
    /// 409.
    NotAvailableYet,
    /// guacamole's stats haven't answered yet, and the steppers' caps come
    /// with them (AH.3).
    Checking,
    /// Editing the working copy under the game's nose would let it see
    /// stale state after the next read; the slot has to be cleared first.
    OnPortal,
}

impl EditGate {
    fn enabled(self) -> bool {
        self == EditGate::Editable
    }

    fn tooltip(self) -> String {
        match self {
            EditGate::Editable => "Edit level + gold".to_string(),
            EditGate::UnsupportedCategory(c) => format!("Editing not supported for {c:?}"),
            EditGate::NotAvailableYet => "Not available yet".to_string(),
            EditGate::Checking => "Checking\u{2026}".to_string(),
            EditGate::OnPortal => "Remove from portal before editing".to_string(),
        }
    }
}

/// The STATS + RESET gate. `stats` is the stats fetch's outcome, `None`
/// while it's in flight. Only a fetch the server refused outright
/// (`Unavailable`) turns the actions off: a 404 for a never-placed figure
/// (`Missing`) still edits, because the first edit makes its progress (the
/// product forks the working copy from the pack). In the product in-flight
/// counts as editable so its buttons render enabled immediately, as before
/// AA.9; guacamole waits for the answer, which carries the game's caps
/// (AH.3). Category wins over the rest (permanent), and "not available yet"
/// wins over "on portal" (clearing the slot wouldn't help).
fn edit_gate(
    category: Category,
    on_portal: bool,
    stats: Option<&StatsFetch>,
    app: ServerApp,
) -> EditGate {
    if !matches!(
        category,
        Category::Figure | Category::Sidekick | Category::Giant | Category::Kaos
    ) {
        return EditGate::UnsupportedCategory(category);
    }
    if matches!(stats, Some(StatsFetch::Unavailable)) {
        return EditGate::NotAvailableYet;
    }
    if app == ServerApp::Guacamole {
        match stats {
            None => return EditGate::Checking,
            Some(fetched) if fetched.caps().is_none() => return EditGate::NotAvailableYet,
            Some(_) => {}
        }
    }
    if on_portal {
        return EditGate::OnPortal;
    }
    EditGate::Editable
}

/// The edit sheet's level and gold caps: under guacamole the game's own,
/// from the stats answer (AH.3); in the product, and in guacamole before an
/// answer carried them, the product's by game of origin and [`GOLD_MAX`].
/// guacamole's gate keeps the sheet shut until the caps arrive.
fn edit_caps(app: ServerApp, stats: Option<&StatsFetch>, game: GameOfOrigin) -> (u8, u16) {
    match (app, stats.and_then(StatsFetch::caps)) {
        (ServerApp::Guacamole, Some(c)) => {
            (c.max_level, u16::try_from(c.max_gold).unwrap_or(u16::MAX))
        }
        _ => (max_level_for_game(game), GOLD_MAX),
    }
}

/// Where RESET sends the figure back to fresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResetVia {
    /// Off the portal: the figure-keyed reset; the next placement is fresh.
    Figure,
    /// guacamole, the viewer's own placement in this slot (1-based): the
    /// slot reset, fresh where it stands (AH.3).
    Slot(u8),
}

/// RESET follows the STATS gate, except that under guacamole the viewer's own
/// figure standing on the portal resets in its slot (the server refuses that
/// for the hero in a level). `own_slot` is [`own_loaded_slot`]'s; a figure
/// still loading has none, so RESET waits for it.
fn reset_via(gate: EditGate, app: ServerApp, own_slot: Option<u8>) -> Option<ResetVia> {
    match gate {
        EditGate::Editable => Some(ResetVia::Figure),
        EditGate::OnPortal if app == ServerApp::Guacamole => own_slot.map(ResetVia::Slot),
        _ => None,
    }
}

/// The slot (1-based) holding `viewer`'s own loaded placement of `figure_id`.
fn own_loaded_slot(portal: &[Slot], figure_id: &str, viewer: Option<&str>) -> Option<u8> {
    let viewer = viewer?;
    let i = portal.iter().position(|slot| {
        matches!(
            &slot.state,
            SlotState::Loaded { figure_id: Some(id), placed_by: Some(by), .. }
                if id == figure_id && by == viewer
        )
    })?;
    u8::try_from(i + 1).ok()
}

/// Whether `figure_id` is on the portal for `viewer`'s STATS and RESET, loaded
/// or loading, under `app`'s placement rule (AA.18): anyone's placement in the
/// product, only the viewer's own in guacamole, where another profile's copy
/// leaves this one's progress free to edit.
fn on_portal_for(portal: &[Slot], figure_id: &str, app: ServerApp, viewer: Option<&str>) -> bool {
    portal.iter().any(|slot| match &slot.state {
        SlotState::Loaded {
            figure_id: Some(id),
            placed_by,
            ..
        }
        | SlotState::Loading {
            figure_id: Some(id),
            placed_by,
        } => id == figure_id && app.blocks_place(placed_by.as_deref(), viewer),
        _ => false,
    })
}

/// True when `target` is this details screen's reset of `figure_id` (the portal's slot resets are left alone).
fn is_detail_reset_of(target: Option<&ResetTarget>, figure_id: &str) -> bool {
    target.is_some_and(|t| t.slot.is_none() && t.figure_id == figure_id)
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
        // CrossGame — permissive (matches server-side game_to_generation,
        // which maps it to SkyGeneration::Unknown → max 20).
        GameOfOrigin::CrossGame => 20,
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
    }
}

#[cfg(test)]
mod tests {
    //! AA.9 — the STATS + RESET gate. The buttons themselves are covered
    //! by `figure_edit.rs` e2e; this pins every input combination.
    use super::*;
    use crate::api::FigureStats;

    const ALL_CATEGORIES: [Category; 10] = [
        Category::Figure,
        Category::Sidekick,
        Category::Giant,
        Category::Item,
        Category::Trap,
        Category::AdventurePack,
        Category::CreationCrystal,
        Category::Vehicle,
        Category::Kaos,
        Category::Other,
    ];

    fn editable_category(c: Category) -> bool {
        matches!(
            c,
            Category::Figure | Category::Sidekick | Category::Giant | Category::Kaos
        )
    }

    fn read() -> StatsFetch {
        StatsFetch::Read(FigureStats {
            level: 3,
            gold: 120,
            playtime_secs: 600,
            nickname: String::new(),
            max_level: None,
            max_gold: None,
        })
    }

    /// guacamole's answer: the row's numbers with the game's caps.
    fn read_guac() -> StatsFetch {
        StatsFetch::Read(FigureStats {
            level: 3,
            gold: 120,
            playtime_secs: 600,
            nickname: String::new(),
            max_level: Some(15),
            max_gold: Some(65_000),
        })
    }

    const PRODUCT: ServerApp = ServerApp::Product;

    #[test]
    fn stats_present_follows_category_and_portal() {
        let stats = read();
        for c in ALL_CATEGORIES {
            let off = edit_gate(c, false, Some(&stats), PRODUCT);
            let on = edit_gate(c, true, Some(&stats), PRODUCT);
            if editable_category(c) {
                assert_eq!(off, EditGate::Editable, "{c:?} off portal");
                assert_eq!(on, EditGate::OnPortal, "{c:?} on portal");
            } else {
                assert_eq!(off, EditGate::UnsupportedCategory(c), "{c:?} off portal");
                assert_eq!(on, EditGate::UnsupportedCategory(c), "{c:?} on portal");
            }
        }
    }

    #[test]
    fn stats_unavailable_disables_every_category_on_and_off_portal() {
        for c in ALL_CATEGORIES {
            for on_portal in [false, true] {
                let gate = edit_gate(c, on_portal, Some(&StatsFetch::Unavailable), PRODUCT);
                assert!(!gate.enabled(), "{c:?} on_portal={on_portal}");
                if editable_category(c) {
                    assert_eq!(
                        gate,
                        EditGate::NotAvailableYet,
                        "{c:?} on_portal={on_portal}"
                    );
                } else {
                    assert_eq!(gate, EditGate::UnsupportedCategory(c));
                }
            }
        }
    }

    /// The product's never-placed figure (404) and a fetch still in flight
    /// gate exactly like stats that came back: the first edit forks the
    /// working copy, so the product's phone behaves as it did before AA.9.
    #[test]
    fn missing_and_in_flight_match_stats_present() {
        let stats = read();
        for c in ALL_CATEGORIES {
            for on_portal in [false, true] {
                let present = edit_gate(c, on_portal, Some(&stats), PRODUCT);
                let missing = StatsFetch::Missing(None);
                assert_eq!(edit_gate(c, on_portal, Some(&missing), PRODUCT), present);
                assert_eq!(edit_gate(c, on_portal, None, PRODUCT), present);
            }
        }
    }

    #[test]
    fn tooltips_name_the_reason() {
        assert_eq!(EditGate::Editable.tooltip(), "Edit level + gold");
        assert_eq!(EditGate::NotAvailableYet.tooltip(), "Not available yet");
        assert!(EditGate::OnPortal.tooltip().contains("portal"));
        assert_eq!(
            EditGate::UnsupportedCategory(Category::Trap).tooltip(),
            "Editing not supported for Trap"
        );
        assert!(EditGate::Editable.enabled());
        assert!(!EditGate::OnPortal.enabled());
    }

    /// Only this screen's reset of this figure closes when the gate turns off.
    #[test]
    fn detail_reset_matches_only_this_figure_off_portal() {
        let target = |slot: Option<u8>, figure_id: &str| ResetTarget {
            slot,
            profile_id: "p1".to_string(),
            figure_id: figure_id.to_string(),
            display_name: "Eruptor".to_string(),
        };
        assert!(is_detail_reset_of(
            Some(&target(None, "eruptor")),
            "eruptor"
        ));
        assert!(!is_detail_reset_of(
            Some(&target(Some(2), "eruptor")),
            "eruptor"
        ));
        assert!(!is_detail_reset_of(Some(&target(None, "spyro")), "eruptor"));
        assert!(!is_detail_reset_of(None, "eruptor"));
    }

    /// STATS and RESET lock on the same placements PLACE refuses: anyone's in
    /// the product, the viewer's own in guacamole, loaded or still loading.
    #[test]
    fn the_edit_lock_follows_the_apps_placement_rule() {
        let slot = |state| Slot { state };
        let portal = [
            slot(SlotState::Loaded {
                figure_id: Some("eruptor".to_string()),
                display_name: "Eruptor".to_string(),
                placed_by: Some("alice".to_string()),
            }),
            slot(SlotState::Loading {
                figure_id: Some("spyro".to_string()),
                placed_by: Some("bob".to_string()),
            }),
            slot(SlotState::Empty),
        ];
        for viewer in [Some("alice"), Some("bob"), None] {
            assert!(on_portal_for(
                &portal,
                "eruptor",
                ServerApp::Product,
                viewer
            ));
            assert!(on_portal_for(&portal, "spyro", ServerApp::Product, viewer));
            assert!(!on_portal_for(&portal, "bash", ServerApp::Product, viewer));
        }
        let guac = ServerApp::Guacamole;
        assert!(on_portal_for(&portal, "eruptor", guac, Some("alice")));
        assert!(!on_portal_for(&portal, "eruptor", guac, Some("bob")));
        assert!(on_portal_for(&portal, "spyro", guac, Some("bob")));
        assert!(!on_portal_for(&portal, "spyro", guac, Some("alice")));
        assert!(!on_portal_for(&portal, "bash", guac, Some("alice")));
    }

    /// AH.3: under guacamole the gate waits for the stats, whose caps the
    /// steppers need; a never-placed figure's 404 with the caps edits, one
    /// without them (a transport error) doesn't.
    #[test]
    fn guacamole_edits_once_the_stats_bring_the_caps() {
        let guac = ServerApp::Guacamole;
        let caps = crate::api::GameCaps {
            max_level: 15,
            max_gold: 65_000,
        };
        let never = StatsFetch::Missing(Some(caps));
        for c in ALL_CATEGORIES {
            if !editable_category(c) {
                assert_eq!(
                    edit_gate(c, false, None, guac),
                    EditGate::UnsupportedCategory(c)
                );
                continue;
            }
            assert_eq!(edit_gate(c, false, None, guac), EditGate::Checking);
            assert_eq!(edit_gate(c, true, None, guac), EditGate::Checking);
            for fetched in [read_guac(), never.clone()] {
                assert_eq!(
                    edit_gate(c, false, Some(&fetched), guac),
                    EditGate::Editable
                );
                assert_eq!(edit_gate(c, true, Some(&fetched), guac), EditGate::OnPortal);
            }
            for fetched in [StatsFetch::Missing(None), read(), StatsFetch::Unavailable] {
                assert_eq!(
                    edit_gate(c, false, Some(&fetched), guac),
                    EditGate::NotAvailableYet,
                    "{fetched:?}"
                );
            }
        }
        assert!(!EditGate::Checking.enabled());
        assert_eq!(EditGate::Checking.tooltip(), "Checking\u{2026}");
    }

    /// AH.3: the sheet's caps are the stats answer's under guacamole (the
    /// game's table and clamp) and the product's own by game of origin,
    /// whatever its answer carries.
    #[test]
    fn the_sheets_caps_follow_the_app() {
        let guac = ServerApp::Guacamole;
        let fixture_caps = crate::api::GameCaps {
            max_level: 15,
            max_gold: 50_000,
        };
        let never = StatsFetch::Missing(Some(fixture_caps));
        assert_eq!(
            edit_caps(guac, Some(&read_guac()), GameOfOrigin::Giants),
            (15, 65_000)
        );
        assert_eq!(
            edit_caps(guac, Some(&never), GameOfOrigin::Giants),
            (15, 50_000)
        );
        let past_u16 = StatsFetch::Missing(Some(crate::api::GameCaps {
            max_level: 15,
            max_gold: 70_000,
        }));
        assert_eq!(
            edit_caps(guac, Some(&past_u16), GameOfOrigin::Giants).1,
            u16::MAX
        );
        // The product: 10 for a Giants figure, 65,000 gold, even if an
        // answer carried caps.
        for fetched in [Some(&read()), Some(&read_guac()), Some(&never), None] {
            assert_eq!(
                edit_caps(PRODUCT, fetched, GameOfOrigin::Giants),
                (10, GOLD_MAX)
            );
        }
        assert_eq!(
            edit_caps(PRODUCT, Some(&read()), GameOfOrigin::SwapForce),
            (15, GOLD_MAX)
        );
        assert_eq!(GOLD_MAX, 65_000);
    }

    /// AH.3: RESET off the portal resets the figure; under guacamole the
    /// viewer's own figure standing on the portal resets in its slot, while
    /// the product keeps RESET off on the portal as before.
    #[test]
    fn reset_goes_to_the_slot_only_for_guacamoles_own_figure_on_the_portal() {
        let guac = ServerApp::Guacamole;
        assert_eq!(
            reset_via(EditGate::Editable, guac, None),
            Some(ResetVia::Figure)
        );
        assert_eq!(
            reset_via(EditGate::Editable, PRODUCT, None),
            Some(ResetVia::Figure)
        );
        assert_eq!(
            reset_via(EditGate::OnPortal, guac, Some(3)),
            Some(ResetVia::Slot(3))
        );
        assert_eq!(
            reset_via(EditGate::OnPortal, guac, None),
            None,
            "still loading"
        );
        assert_eq!(reset_via(EditGate::OnPortal, PRODUCT, Some(3)), None);
        for gate in [
            EditGate::Checking,
            EditGate::NotAvailableYet,
            EditGate::UnsupportedCategory(Category::Trap),
        ] {
            assert_eq!(reset_via(gate, guac, Some(3)), None, "{gate:?}");
        }

        let slot = |state| Slot { state };
        let portal = [
            slot(SlotState::Loaded {
                figure_id: Some("eruptor".to_string()),
                display_name: "Eruptor".to_string(),
                placed_by: Some("bob".to_string()),
            }),
            slot(SlotState::Loading {
                figure_id: Some("eruptor".to_string()),
                placed_by: Some("alice".to_string()),
            }),
            slot(SlotState::Loaded {
                figure_id: Some("spyro".to_string()),
                display_name: "Spyro".to_string(),
                placed_by: Some("alice".to_string()),
            }),
        ];
        assert_eq!(own_loaded_slot(&portal, "eruptor", Some("bob")), Some(1));
        assert_eq!(own_loaded_slot(&portal, "eruptor", Some("alice")), None);
        assert_eq!(own_loaded_slot(&portal, "spyro", Some("alice")), Some(3));
        assert_eq!(own_loaded_slot(&portal, "spyro", Some("bob")), None);
        assert_eq!(own_loaded_slot(&portal, "spyro", None), None);
    }
}
