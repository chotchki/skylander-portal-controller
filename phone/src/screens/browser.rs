use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::{
    BezelSize, BezelState, BoxState, GoldBezel, ToyBoxInterior, ToyBoxLid,
};
use crate::model::{Category, Element, GameOfOrigin, PublicFigure, Slot, SlotState, SLOT_COUNT};
use crate::screens::FigureDetail;
use crate::{event_target_value, ScanOverlayState, ToastMsg};

/// A cluster of figures that share a `variant_group` — e.g. every Spyro
/// repose, every scanned Creation Crystal. Rendered as a single card in
/// the toy-box grid; the user cycles between the cluster's variants by
/// tapping the N-variants badge. PLAN 3.14.1.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GroupedCard {
    /// `variant_group` value; also the dedup key for `<For>`.
    key: String,
    /// Variants in display order — "base" first, then alphabetical by
    /// `variant_tag`. Stable so `current_idx` round-trips predictably
    /// across filter changes.
    variants: Vec<PublicFigure>,
}

/// Group `figs` by `variant_group` and sort each cluster so "base"
/// comes first, then alphabetically by `variant_tag`. Outer ordering
/// preserves the input order (first-seen wins) so upstream sort
/// choices (e.g. compat ranking) carry through.
fn group_variants(figs: Vec<PublicFigure>) -> Vec<GroupedCard> {
    use std::collections::HashMap;
    // Preserve first-seen ordering: index into `order` is the display
    // position; the HashMap just lets us append variants in O(1).
    let mut order: Vec<String> = Vec::new();
    let mut buckets: HashMap<String, Vec<PublicFigure>> = HashMap::new();
    for f in figs {
        let key = f.variant_group.clone();
        if !buckets.contains_key(&key) {
            order.push(key.clone());
        }
        buckets.entry(key).or_default().push(f);
    }
    order
        .into_iter()
        .map(|key| {
            let mut variants = buckets.remove(&key).unwrap_or_default();
            variants.sort_by(|a, b| {
                let a_base = a.variant_tag == "base";
                let b_base = b.variant_tag == "base";
                match (a_base, b_base) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.variant_tag.cmp(&b.variant_tag),
                }
            });
            GroupedCard { key, variants }
        })
        .collect()
}

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

    let grouped = Memo::new(move |_| {
        let ef = element_filter.get();
        let gf = game_filter.get();
        let cf = category_filter.get();
        let q = search.get().trim().to_lowercase();
        let filtered: Vec<PublicFigure> = all_figures.with_value(|figs| {
            figs.iter()
                .filter(|f| ef.map_or(true, |e| f.element == Some(e)))
                .filter(|f| gf.map_or(true, |g| f.game == g))
                .filter(|f| cf.map_or(true, |c| f.category == c))
                // Search hits canonical_name (main card text), variant_tag
                // (the small subtitle — e.g. "DELFOX" under "Creation
                // Crystal" for a scan-only CC, or "Legendary" for a Spyro
                // repose), OR variant_group (so searching "spyro" matches
                // every reposed Spyro whose canonical_name has a prefix).
                .filter(|f| {
                    q.is_empty()
                        || f.canonical_name.to_lowercase().contains(&q)
                        || f.variant_tag.to_lowercase().contains(&q)
                        || f.variant_group.to_lowercase().contains(&q)
                })
                .take(400) // Phase 3 will virtualize.
                .cloned()
                .collect()
        });
        group_variants(filtered)
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

    let is_empty = Memo::new(move |_| grouped.get().is_empty());

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
                                each=move || grouped.get()
                                key=|g: &GroupedCard| g.key.clone()
                                children=move |g: GroupedCard| {
                                    let group_key = g.key.clone();
                                    // Element tint is per-group (not per-variant):
                                    // reposes share an element by definition, and
                                    // <GoldBezel> takes `element` as a plain value
                                    // not a Signal. For mixed-element groups
                                    // (shouldn't happen in real data) the first
                                    // variant's element wins.
                                    let group_element = g.variants.first().and_then(|f| f.element);
                                    let variants = StoredValue::new(g.variants);
                                    let variant_count = variants.with_value(|v| v.len());
                                    // Per-card cycle state; persists across
                                    // filter changes because `<For>` reuses
                                    // this closure when the key is stable.
                                    let current_idx = RwSignal::new(0usize);

                                    // Derived "current variant" snapshot.
                                    // Every card-body field reads through this
                                    // so cycling advances the whole card
                                    // (thumbnail, name, tag, place-target) in
                                    // one step.
                                    let current = Signal::derive(move || {
                                        variants.with_value(|v| {
                                            let i = current_idx.get() % v.len().max(1);
                                            v[i].clone()
                                        })
                                    });

                                    let current_id = Signal::derive(move || current.get().id);
                                    let current_name = Signal::derive(move || current.get().canonical_name);
                                    let current_variant_tag = Signal::derive(move || current.get().variant_tag);
                                    let initial = Signal::derive(move || {
                                        current.get()
                                            .canonical_name
                                            .chars()
                                            .next()
                                            .unwrap_or('?')
                                            .to_uppercase()
                                            .to_string()
                                    });

                                    // Loaded-state tracking is variant-level:
                                    // the ribbon + desat reflect the currently-
                                    // displayed variant, not the group.
                                    let is_loaded_this = move || {
                                        let name = current_name.get();
                                        loaded_names.get().iter().any(|n| n == &name)
                                    };
                                    let is_loading_this = move || {
                                        let id = current_id.get();
                                        loading_ids.get().iter().any(|x| *x == id)
                                    };

                                    let loaded_for_class = is_loaded_this.clone();
                                    let loaded_for_badge = is_loaded_this.clone();
                                    let loaded_for_bezel = is_loaded_this.clone();
                                    let loading_for_class = is_loading_this.clone();

                                    let bezel_state = Signal::derive(move || {
                                        if loaded_for_bezel() {
                                            BezelState::Disabled
                                        } else {
                                            BezelState::Default
                                        }
                                    });

                                    let on_place_click = move |ev: leptos::web_sys::MouseEvent| {
                                        // Variant-badge click bubbles up with
                                        // a data attr set — bail so cycling
                                        // doesn't also open the detail view.
                                        if let Some(target) = ev.target() {
                                            if let Ok(el) = target.dyn_into::<leptos::web_sys::Element>() {
                                                if el.closest(".fig-variant-badge").ok().flatten().is_some() {
                                                    return;
                                                }
                                            }
                                        }
                                        selected_figure.set(Some(current.get_untracked()));
                                    };

                                    let on_cycle = move |ev: leptos::web_sys::MouseEvent| {
                                        ev.stop_propagation();
                                        current_idx.update(|i| {
                                            *i = (*i + 1) % variant_count.max(1);
                                        });
                                    };

                                    view! {
                                        <div
                                            class=move || {
                                                if loaded_for_class() {
                                                    "fig-card-p4 on-portal"
                                                } else if loading_for_class() {
                                                    "fig-card-p4 loading"
                                                } else {
                                                    "fig-card-p4"
                                                }
                                            }
                                            role="button"
                                            tabindex="0"
                                            on:click=on_place_click
                                        >
                                            <div class="fig-bezel-p4">
                                                <GoldBezel
                                                    size=BezelSize::Sm
                                                    element=group_element
                                                    state=bezel_state
                                                >
                                                    <span class="fig-initial">{move || initial.get()}</span>
                                                    <img
                                                        class="fig-image-p4"
                                                        src=move || format!("/api/figures/{}/image?size=thumb", current_id.get())
                                                        alt=""
                                                        loading="lazy"
                                                        decoding="async"
                                                    />
                                                </GoldBezel>
                                            </div>
                                            // Group name above the variant tag; stays
                                            // the same across cycles so the card keeps
                                            // visual identity.
                                            <div class="fig-name-p4">{group_key.clone()}</div>
                                            <Show
                                                when=move || current_variant_tag.get() != "base"
                                                fallback=|| ()
                                            >
                                                <div class="fig-variant-p4">{move || current_variant_tag.get()}</div>
                                            </Show>
                                            <Show when=move || loaded_for_badge() fallback=|| ()>
                                                <div class="fig-on-portal-ribbon">"ON PORTAL"</div>
                                            </Show>
                                            <Show when=move || (variant_count > 1) fallback=|| ()>
                                                <button
                                                    class="fig-variant-badge"
                                                    type="button"
                                                    title="Next variant"
                                                    aria-label=move || {
                                                        format!(
                                                            "Cycle variant — {count} variants",
                                                            count = variant_count,
                                                        )
                                                    }
                                                    on:click=on_cycle
                                                >
                                                    {format!("{variant_count}")}
                                                </button>
                                            </Show>
                                        </div>
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
    // the plain `Figure` + `Sidekick` / `Giant` / `Kaos` / `Other` rows:
    // those are either the default case (Figure) or niche enough that a
    // flat chip row would add clutter without helping a kid find what
    // they want. `CreationCrystal` joined the list once 6.5.5a started
    // surfacing scan-only CCs in the library — previously nothing in
    // the pack fell in that bucket.
    let all_categories: [(Option<Category>, &'static str); 6] = [
        (None, "All"),
        (Some(Category::Vehicle), "Vehicles"),
        (Some(Category::Trap), "Traps"),
        (Some(Category::CreationCrystal), "Crystals"),
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

#[cfg(test)]
mod tests {
    //! PLAN 3.14.1: the browser collapses figures sharing a
    //! `variant_group` into one card. Tests cover the pure grouping
    //! helper so cycling order (base first, then alphabetical) stays
    //! stable against future filter-pipeline changes.

    use super::*;
    use crate::model::{Category, GameOfOrigin};

    fn fig(id: &str, group: &str, tag: &str, name: &str) -> PublicFigure {
        PublicFigure {
            id: id.into(),
            canonical_name: name.into(),
            variant_group: group.into(),
            variant_tag: tag.into(),
            game: GameOfOrigin::SpyrosAdventure,
            element: None,
            category: Category::Figure,
        }
    }

    #[test]
    fn singletons_become_single_variant_groups() {
        let out = group_variants(vec![
            fig("a", "Spyro", "base", "Spyro"),
            fig("b", "Eruptor", "base", "Eruptor"),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "Spyro");
        assert_eq!(out[0].variants.len(), 1);
        assert_eq!(out[1].key, "Eruptor");
        assert_eq!(out[1].variants.len(), 1);
    }

    #[test]
    fn multiple_variants_merge_into_one_group() {
        let out = group_variants(vec![
            fig("a", "Spyro", "base", "Spyro"),
            fig("b", "Spyro", "Dark", "Dark Spyro"),
            fig("c", "Spyro", "Legendary", "Legendary Spyro"),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "Spyro");
        assert_eq!(out[0].variants.len(), 3);
    }

    #[test]
    fn base_variant_sorts_first_then_alphabetical() {
        let out = group_variants(vec![
            // Deliberately out-of-order on input.
            fig("c", "Spyro", "Legendary", "Legendary Spyro"),
            fig("b", "Spyro", "Dark", "Dark Spyro"),
            fig("a", "Spyro", "base", "Spyro"),
        ]);
        let tags: Vec<&str> = out[0]
            .variants
            .iter()
            .map(|v| v.variant_tag.as_str())
            .collect();
        assert_eq!(tags, vec!["base", "Dark", "Legendary"]);
    }

    #[test]
    fn outer_order_is_first_seen() {
        // Input order: Eruptor, Spyro base, Spyro Dark, Bash.
        // Output should keep Eruptor → Spyro → Bash even though
        // Spyro has a second variant appearing after Bash wouldn't
        // move it.
        let out = group_variants(vec![
            fig("e", "Eruptor", "base", "Eruptor"),
            fig("s1", "Spyro", "base", "Spyro"),
            fig("b", "Bash", "base", "Bash"),
            fig("s2", "Spyro", "Dark", "Dark Spyro"),
        ]);
        let keys: Vec<&str> = out.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, vec!["Eruptor", "Spyro", "Bash"]);
        // Spyro group carries both variants.
        let spyro = out.iter().find(|g| g.key == "Spyro").unwrap();
        assert_eq!(spyro.variants.len(), 2);
    }

    #[test]
    fn empty_input_produces_empty_output() {
        assert!(group_variants(vec![]).is_empty());
    }

    #[test]
    fn scanned_creation_crystals_group_by_kind() {
        // Scan figures share `variant_group = canonical_name = kind`
        // (indexer::scan_runtime). Nicknames live in variant_tag.
        let out = group_variants(vec![
            fig("scan:1", "Creation Crystal", "DELFOX", "Creation Crystal"),
            fig("scan:2", "Creation Crystal", "WAVEY", "Creation Crystal"),
            fig("scan:3", "Creation Crystal", "ICEBLAST", "Creation Crystal"),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "Creation Crystal");
        assert_eq!(out[0].variants.len(), 3);
        // All nicknames end up sorted alphabetically since none is "base".
        let tags: Vec<&str> = out[0]
            .variants
            .iter()
            .map(|v| v.variant_tag.as_str())
            .collect();
        assert_eq!(tags, vec!["DELFOX", "ICEBLAST", "WAVEY"]);
    }
}
