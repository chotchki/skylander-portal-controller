use leptos::prelude::*;

use crate::components::{
    BezelSize, BezelState, BoxState, GoldBezel, ToyBoxInterior, ToyBoxLid,
};
use crate::model::{Category, Element, GameOfOrigin, PublicFigure, Slot, SlotState, SLOT_COUNT};
use crate::screens::FigureDetail;
use crate::{event_target_value, ScanOverlayState, ToastMsg};

#[component]
pub(crate) fn Browser(
    figures: Vec<PublicFigure>,
    picking_for: RwSignal<Option<u8>>,
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    element_filter: RwSignal<Option<Element>>,
    game_filter: RwSignal<Option<GameOfOrigin>>,
    category_filter: RwSignal<Option<Category>>,
    search: RwSignal<String>,
    toasts: RwSignal<Vec<ToastMsg>>,
    scan_overlay: RwSignal<ScanOverlayState>,
) -> impl IntoView {
    let all_figures = StoredValue::new(figures);
    let selected_figure = RwSignal::new(None::<PublicFigure>);

    let filtered = Memo::new(move |_| {
        let ef = element_filter.get();
        let gf = game_filter.get();
        let cf = category_filter.get();
        let q = search.get().trim().to_lowercase();
        all_figures.with_value(|figs| {
            figs.iter()
                .filter(|f| ef.map_or(true, |e| f.element == Some(e)))
                .filter(|f| gf.map_or(true, |g| f.game == g))
                .filter(|f| cf.map_or(true, |c| f.category == c))
                .filter(|f| q.is_empty() || f.canonical_name.to_lowercase().contains(&q))
                .take(400) // Phase 3 will virtualize.
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    // Two sets tracked separately so the card UI can tell "currently loading"
    // apart from "already loaded":
    //   - `loaded_names`  — canonical-name matches for fully-Loaded slots.
    //   - `loading_ids`   — figure_id markers for Loading slots.
    let loaded_names = Memo::new(move |_| {
        portal
            .get()
            .iter()
            .filter_map(|s| match &s.state {
                SlotState::Loaded { display_name, .. } => Some(display_name.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    let loading_ids = Memo::new(move |_| {
        portal
            .get()
            .iter()
            .filter_map(|s| match &s.state {
                SlotState::Loading {
                    figure_id: Some(id),
                    ..
                } => Some(id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    });

    let is_empty = Memo::new(move |_| filtered.get().is_empty());

    // Default is `Closed` so the portal grid owns the viewport on arrival
    // (design_language.md §6.7: closed = wooden plank at the bottom,
    // portal visible above). User taps/swipes the lid to open. Prior
    // default was `Compact`, which hid the portal behind the figure grid
    // and pushed the lid off-screen on any iPhone where the 2×4 bezel
    // grid (~940px) exceeded the available viewport (~660px). Chris
    // flagged 2026-04-21 ("the lid is not there"). PLAN 4.18 round 3.
    let box_state = RwSignal::new(BoxState::Closed);

    view! {
        <Show
            when=move || selected_figure.get().is_some()
            fallback=move || view! {
                <ToyBoxLid box_state>
                    <BrowserFilters element_filter game_filter category_filter search />
                </ToyBoxLid>
                <ToyBoxInterior box_state>
                    <Show
                        when=move || !is_empty.get()
                        fallback=move || view! {
                            <div class="browser-empty">
                                <span class="browser-empty-icon">"?"</span>
                                <span class="browser-empty-text">"No figures match your search."</span>
                            </div>
                        }
                    >
                        <div class="figure-grid-p4">
                            // Plus-card (PLAN 6.5.2): pinned at position 0
                            // so adding a figure feels like "there's
                            // always a slot for a new one" rather than an
                            // off-screen button. Opens the scan overlay;
                            // the 6.5.1 scanner worker is already polling,
                            // so success arrives as Event::FigureScanned.
                            <button
                                class="fig-card-p4 scan-new"
                                aria-label="Add a figure by tapping it on the scanner"
                                on:click=move |_| scan_overlay.set(ScanOverlayState::Prompt)
                            >
                                <div class="fig-bezel-p4">
                                    <div class="scan-new-bezel"></div>
                                </div>
                                <div class="fig-name-p4 scan-new-label">"SCAN NEW"</div>
                            </button>
                            <For
                                each=move || filtered.get()
                                key=|f: &PublicFigure| f.id.clone()
                                children=move |f: PublicFigure| {
                                    let fig_for_select = f.clone();
                                    let id = f.id.clone();
                                    let name = f.canonical_name.clone();
                                    let elem = f.element;
                                    let variant = f.variant_tag.clone();
                                    let variant_for_show = variant.clone();
                                    let initial = name.chars().next().unwrap_or('?').to_uppercase().to_string();

                                    let name_for_loaded = name.clone();
                                    let id_for_loading = id.clone();
                                    let is_loaded_this = move || {
                                        loaded_names.get().iter().any(|n| n == &name_for_loaded)
                                    };
                                    let is_loading_this =
                                        move || loading_ids.get().iter().any(|id| id == &id_for_loading);

                                    let loaded_for_class = is_loaded_this.clone();
                                    let loaded_for_badge = is_loaded_this.clone();
                                    let loaded_for_bezel = is_loaded_this.clone();
                                    let loading_for_class = is_loading_this.clone();

                                    // Bezel state: on-portal figures get Disabled (desaturated)
                                    let bezel_state = Signal::derive(move || {
                                        if loaded_for_bezel() {
                                            BezelState::Disabled
                                        } else {
                                            BezelState::Default
                                        }
                                    });

                                    view! {
                                        <button
                                            class=move || {
                                                if loaded_for_class() {
                                                    "fig-card-p4 on-portal"
                                                } else if loading_for_class() {
                                                    "fig-card-p4 loading"
                                                } else {
                                                    "fig-card-p4"
                                                }
                                            }
                                            on:click={
                                                let fig = fig_for_select.clone();
                                                move |_| {
                                                    selected_figure.set(Some(fig.clone()));
                                                }
                                            }
                                        >
                                            <div class="fig-bezel-p4">
                                                <GoldBezel
                                                    size=BezelSize::Sm
                                                    element=elem
                                                    state=bezel_state
                                                >
                                                    // Initial sits behind the portrait so a missing
                                                    // scrape (rare — server falls back to the element
                                                    // icon) still shows something readable.
                                                    <span class="fig-initial">{initial.clone()}</span>
                                                    <img
                                                        class="fig-image-p4"
                                                        src=format!("/api/figures/{}/image?size=thumb", id)
                                                        alt=""
                                                        loading="lazy"
                                                        decoding="async"
                                                    />
                                                </GoldBezel>
                                            </div>
                                            <div class="fig-name-p4">{name}</div>
                                            <Show when=move || variant_for_show != "base" fallback=|| ()>
                                                <div class="fig-variant-p4">{variant.clone()}</div>
                                            </Show>
                                            <Show when=move || loaded_for_badge() fallback=|| ()>
                                                <div class="fig-on-portal-ribbon">"ON PORTAL"</div>
                                            </Show>
                                        </button>
                                    }
                                }
                            />
                        </div>
                    </Show>
                </ToyBoxInterior>
            }
        >
            {move || selected_figure.get().map(|fig| view! {
                <FigureDetail
                    figure=fig
                    picking_for
                    portal
                    toasts
                    on_close=Callback::new(move |_| selected_figure.set(None))
                    on_placed=Callback::new(move |_| {
                        // PLACE success: dismiss detail AND close the lid
                        // so the user lands back on the portal with the
                        // toy box shut — navigation.md §1. BACK uses the
                        // `on_close` path above, which leaves the lid in
                        // whatever open state the user had while browsing.
                        selected_figure.set(None);
                        box_state.set(BoxState::Closed);
                    })
                />
            })}
        </Show>
    }
}

/// Search + drill-section filter rows that fill the toy-box lid's expanded
/// area. Owns no gesture state — purely renders the filter UI driven by the
/// signals passed in. Three sections — GAMES / ELEMENTS / CATEGORY — match
/// `docs/aesthetic/mocks/portal_with_box.html`.
#[component]
fn BrowserFilters(
    element_filter: RwSignal<Option<Element>>,
    game_filter: RwSignal<Option<GameOfOrigin>>,
    category_filter: RwSignal<Option<Category>>,
    search: RwSignal<String>,
) -> impl IntoView {
    let all_games: [(Option<GameOfOrigin>, &'static str); 7] = [
        (None, "All"),
        (Some(GameOfOrigin::SpyrosAdventure), "SSA"),
        (Some(GameOfOrigin::Giants), "Giants"),
        (Some(GameOfOrigin::SwapForce), "Swap Force"),
        (Some(GameOfOrigin::TrapTeam), "Trap Team"),
        (Some(GameOfOrigin::Superchargers), "SuperChargers"),
        (Some(GameOfOrigin::Imaginators), "Imaginators"),
    ];
    let all_elements: [(Option<Element>, &'static str); 11] = [
        (None, "All"),
        (Some(Element::Air), "Air"),
        (Some(Element::Earth), "Earth"),
        (Some(Element::Fire), "Fire"),
        (Some(Element::Water), "Water"),
        (Some(Element::Life), "Life"),
        (Some(Element::Undead), "Undead"),
        (Some(Element::Tech), "Tech"),
        (Some(Element::Magic), "Magic"),
        (Some(Element::Light), "Light"),
        (Some(Element::Dark), "Dark"),
    ];
    // CATEGORY filter covers the big sub-types the mock calls out. We skip
    // the plain `Figure` + `Sidekick` / `Giant` / `Kaos` / `CreationCrystal`
    // / `Other` rows: those are either the default case (Figure) or niche
    // enough that a flat chip row would add clutter without helping a kid
    // find what they want. Revisit once we have real feedback.
    let all_categories: [(Option<Category>, &'static str); 5] = [
        (None, "All"),
        (Some(Category::Vehicle), "Vehicles"),
        (Some(Category::Trap), "Traps"),
        (Some(Category::AdventurePack), "Adventure Packs"),
        (Some(Category::Item), "Items"),
    ];

    view! {
        <input
            class="search-input-p4"
            type="search"
            placeholder="Search heroes\u{2026}"
            prop:value=move || search.get()
            on:input=move |e| search.set(event_target_value(&e))
        />
        <div class="drill-section-p4">
            <div class="drill-label-p4">"GAMES"</div>
            <div class="drill-row-p4">
                {all_games.into_iter().map(|(val, label)| {
                    let v = val;
                    view! {
                        <button
                            class=move || if game_filter.get() == v {
                                "drill-chip-p4 active"
                            } else {
                                "drill-chip-p4"
                            }
                            on:click=move |_| game_filter.set(v)
                        >
                            {label}
                        </button>
                    }
                }).collect_view()}
            </div>
        </div>
        <div class="drill-section-p4">
            <div class="drill-label-p4">"ELEMENTS"</div>
            <div class="drill-row-p4">
                {all_elements.into_iter().map(|(val, label)| {
                    let v = val;
                    view! {
                        <button
                            class=move || if element_filter.get() == v {
                                "drill-chip-p4 active"
                            } else {
                                "drill-chip-p4"
                            }
                            on:click=move |_| element_filter.set(v)
                        >
                            {label}
                        </button>
                    }
                }).collect_view()}
            </div>
        </div>
        <div class="drill-section-p4">
            <div class="drill-label-p4">"CATEGORY"</div>
            <div class="drill-row-p4">
                {all_categories.into_iter().map(|(val, label)| {
                    let v = val;
                    view! {
                        <button
                            class=move || if category_filter.get() == v {
                                "drill-chip-p4 active"
                            } else {
                                "drill-chip-p4"
                            }
                            on:click=move |_| category_filter.set(v)
                        >
                            {label}
                        </button>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}
