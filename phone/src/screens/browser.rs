use leptos::ev::PointerEvent;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::{BezelSize, BezelState, GoldBezel};
use crate::model::{Element, PublicFigure, Slot, SlotState, SLOT_COUNT};
use crate::screens::FigureDetail;
use crate::{event_target_value, ToastMsg};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoxState {
    Closed,
    Compact,
    Expanded,
}

impl BoxState {
    fn css_modifier(self) -> &'static str {
        match self {
            BoxState::Closed => "closed",
            BoxState::Compact => "",
            BoxState::Expanded => "expanded",
        }
    }
}

const SWIPE_THRESHOLD_PX: f64 = 48.0;
const TAP_MAX_TRAVEL_PX: f64 = 10.0;

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

    let box_state = RwSignal::new(BoxState::Compact);

    let on_grid_scroll = move |_| {
        // WHY: per 4.3.1 gesture model, scrolling the figure grid collapses
        // expanded filters back to compact ("looking deeper into the box").
        if box_state.get_untracked() == BoxState::Expanded {
            box_state.set(BoxState::Compact);
        }
    };

    view! {
        <Show
            when=move || selected_figure.get().is_some()
            fallback=move || view! {
                <BrowserHead element_filter search box_state />
                <div class="box-body-bg">
                    <div class="box-body-fade-top"></div>
                    <div class="box-body-fade-bottom"></div>
                    <div class="box-body-scroll" on:scroll=on_grid_scroll>
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
                                                        <span class="fig-initial">{initial.clone()}</span>
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
                    </div>
                </div>
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

#[component]
fn BrowserHead(
    element_filter: RwSignal<Option<Element>>,
    search: RwSignal<String>,
    box_state: RwSignal<BoxState>,
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

    let gesture_start_y = StoredValue::new(None::<f64>);
    let gesture_pointer_id = StoredValue::new(None::<i32>);
    let gesture_committed = StoredValue::new(false);

    let apply_swipe_down = move || match box_state.get_untracked() {
        BoxState::Closed => {}
        BoxState::Compact => box_state.set(BoxState::Expanded),
        BoxState::Expanded => box_state.set(BoxState::Closed),
    };
    let apply_swipe_up = move || match box_state.get_untracked() {
        BoxState::Closed => box_state.set(BoxState::Compact),
        BoxState::Compact => {}
        BoxState::Expanded => box_state.set(BoxState::Compact),
    };
    let apply_tap = move || match box_state.get_untracked() {
        BoxState::Closed => box_state.set(BoxState::Compact),
        BoxState::Compact => box_state.set(BoxState::Expanded),
        BoxState::Expanded => box_state.set(BoxState::Closed),
    };

    let on_lid_pointerdown = move |ev: PointerEvent| {
        gesture_start_y.set_value(Some(ev.client_y() as f64));
        gesture_pointer_id.set_value(Some(ev.pointer_id()));
        gesture_committed.set_value(false);
        // WHY: capture pointer so pointermove/up keep firing on the lid even
        // if the finger drifts onto the grid or off-screen.
        if let Some(target) = ev.target() {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                let _ = el.set_pointer_capture(ev.pointer_id());
            }
        }
    };

    let on_lid_pointermove = move |ev: PointerEvent| {
        let Some(start_y) = gesture_start_y.get_value() else {
            return;
        };
        if gesture_committed.get_value() {
            return;
        }
        let dy = ev.client_y() as f64 - start_y;
        if dy.abs() < SWIPE_THRESHOLD_PX {
            return;
        }
        gesture_committed.set_value(true);
        if dy > 0.0 {
            apply_swipe_down();
        } else {
            apply_swipe_up();
        }
    };

    let on_lid_pointerup = move |ev: PointerEvent| {
        let start_y = gesture_start_y.get_value();
        let pointer_id = gesture_pointer_id.get_value();
        let committed = gesture_committed.get_value();
        gesture_start_y.set_value(None);
        gesture_pointer_id.set_value(None);
        gesture_committed.set_value(false);

        if let (Some(pid), Some(target)) = (pointer_id, ev.target()) {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                let _ = el.release_pointer_capture(pid);
            }
        }

        if committed {
            return;
        }
        if let Some(sy) = start_y {
            let dy = (ev.client_y() as f64 - sy).abs();
            if dy <= TAP_MAX_TRAVEL_PX {
                apply_tap();
            }
        }
    };

    let on_lid_pointercancel = move |_: PointerEvent| {
        gesture_start_y.set_value(None);
        gesture_pointer_id.set_value(None);
        gesture_committed.set_value(false);
    };

    let on_search_pointerdown = move |ev: PointerEvent| {
        // WHY: SEARCH button owns its own click — don't let the lid-level
        // gesture tracker interpret the press as a lid tap/swipe.
        ev.stop_propagation();
    };

    view! {
        <div class=move || format!("lid-open-p4 {}", box_state.get().css_modifier())>
            <div
                class="lid-grabber-p4"
                on:pointerdown=on_lid_pointerdown
                on:pointermove=on_lid_pointermove
                on:pointerup=on_lid_pointerup
                on:pointercancel=on_lid_pointercancel
            ></div>
            <div
                class="lid-top-row-p4"
                on:pointerdown=on_lid_pointerdown
                on:pointermove=on_lid_pointermove
                on:pointerup=on_lid_pointerup
                on:pointercancel=on_lid_pointercancel
            >
                <span class="lid-open-title-p4">"COLLECTION"</span>
                <button
                    class="search-toggle-p4"
                    on:pointerdown=on_search_pointerdown
                    on:click=move |_| {
                        if box_state.get_untracked() == BoxState::Expanded {
                            box_state.set(BoxState::Compact);
                        } else {
                            box_state.set(BoxState::Expanded);
                        }
                    }
                >
                    <span class="search-toggle-mag">{"\u{2315}"}</span>
                    " SEARCH"
                </button>
            </div>

            <Show when=move || box_state.get() == BoxState::Expanded fallback=|| ()>
                <div class="search-expanded-p4">
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
                </div>
            </Show>
        </div>
    }
}
