use leptos::prelude::*;

use crate::components::{
    BezelSize, BezelState, BoxState, GoldBezel, ToyBoxInterior, ToyBoxLid,
};
use crate::model::{Element, PublicFigure, Slot, SlotState, SLOT_COUNT};
use crate::screens::FigureDetail;
use crate::{event_target_value, ToastMsg};

#[component]
pub(crate) fn Browser(
    figures: Vec<PublicFigure>,
    picking_for: RwSignal<Option<u8>>,
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    element_filter: RwSignal<Option<Element>>,
    search: RwSignal<String>,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let all_figures = StoredValue::new(figures);
    let selected_figure = RwSignal::new(None::<PublicFigure>);

    let filtered = Memo::new(move |_| {
        let ef = element_filter.get();
        let q = search.get().trim().to_lowercase();
        all_figures.with_value(|figs| {
            figs.iter()
                .filter(|f| ef.map_or(true, |e| f.element == Some(e)))
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
                    <BrowserFilters element_filter search />
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
                />
            })}
        </Show>
    }
}

/// Search + element-chip row that fills the toy-box lid's expanded area.
/// Owns no gesture state — purely renders the filter UI driven by the
/// signals passed in.
#[component]
fn BrowserFilters(
    element_filter: RwSignal<Option<Element>>,
    search: RwSignal<String>,
) -> impl IntoView {
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

    view! {
        <input
            class="search-input-p4"
            type="search"
            placeholder="Search heroes\u{2026}"
            prop:value=move || search.get()
            on:input=move |e| search.set(event_target_value(&e))
        />
        <div class="drill-label-p4">"ELEMENTS"</div>
        <div class="chip-row-p4">
            {all_elements.into_iter().map(|(val, label)| {
                let v = val;
                let el_class = val.map(|e| e.css_class()).unwrap_or("");
                view! {
                    <button
                        class=move || {
                            if element_filter.get() == v {
                                format!("el-chip-p4 active {el_class}")
                            } else {
                                format!("el-chip-p4 {el_class}")
                            }
                        }
                        on:click=move |_| element_filter.set(v)
                    >
                        {label}
                    </button>
                }
            }).collect_view()}
        </div>
    }
}
