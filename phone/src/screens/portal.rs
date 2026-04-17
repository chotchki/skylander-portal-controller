use leptos::prelude::*;

use crate::api::post_clear;
use crate::components::{BezelSize, BezelState, GoldBezel};
use crate::model::{Slot, SlotState, SLOT_COUNT};
use crate::ResetTarget;

#[component]
pub(crate) fn Picking(picking_for: RwSignal<Option<u8>>) -> impl IntoView {
    view! {
        <Show when=move || picking_for.get().is_some() fallback=|| ()>
            {move || {
                let slot = picking_for.get().unwrap_or(1);
                view! {
                    <div class="picking-banner">
                        <span>{format!("Pick a Skylander for slot {slot}")}</span>
                        <button on:click=move |_| picking_for.set(None)>"Cancel"</button>
                    </div>
                }
            }}
        </Show>
    }
}

#[component]
pub(crate) fn Portal(
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    picking_for: RwSignal<Option<u8>>,
    reset_target: RwSignal<Option<ResetTarget>>,
) -> impl IntoView {
    view! {
        <section class="portal-p4">
            {(0..SLOT_COUNT).map(|i| {
                view! { <SlotView idx=i portal picking_for reset_target /> }
            }).collect_view()}
        </section>
    }
}

#[component]
fn SlotView(
    idx: usize,
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    picking_for: RwSignal<Option<u8>>,
    reset_target: RwSignal<Option<ResetTarget>>,
) -> impl IntoView {
    let slot_num = (idx + 1) as u8;

    let bezel_state = Signal::derive(move || {
        if picking_for.get() == Some(slot_num) {
            return BezelState::Picking;
        }
        match portal.get()[idx].state {
            SlotState::Empty => BezelState::Default,
            SlotState::Loading { .. } => BezelState::Loading,
            SlotState::Loaded { .. } => BezelState::Loaded,
            SlotState::Error { .. } => BezelState::Errored,
        }
    });

    let is_empty = move || {
        matches!(
            portal.get()[idx].state,
            SlotState::Empty | SlotState::Error { .. }
        )
    };

    let slot_class = move || {
        let base = "p4-slot";
        let state = match portal.get()[idx].state {
            SlotState::Empty => "p4-slot--empty",
            SlotState::Loading { .. } => "p4-slot--loading",
            SlotState::Loaded { .. } => "p4-slot--loaded",
            SlotState::Error { .. } => "p4-slot--errored",
        };
        format!("{base} {state}")
    };

    view! {
        <div class=slot_class
             on:click=move |_| {
                 if is_empty() {
                     picking_for.set(Some(slot_num));
                 }
             }>
            <div class="p4-slot-inner">
                <span class="p4-slot-index">{slot_num}</span>
                // 3.8.2 — when a figure is on the portal but didn't match any
                // indexed figure (i.e. RPCS3 reported a name we don't know),
                // render a "?" badge so the phone makes the mismatch visible
                // instead of silently showing a raw RPCS3 string.
                {move || {
                    match portal.get()[idx].state.clone() {
                        SlotState::Loaded { figure_id: None, .. } => {
                            view! {
                                <span
                                    class="p4-slot-badge p4-slot-badge--unmatched"
                                    title="This figure isn't in your collection"
                                >"?"</span>
                            }.into_any()
                        }
                        _ => view! { <span></span> }.into_any(),
                    }
                }}
                <GoldBezel size=BezelSize::Lg state=bezel_state>
                    {move || {
                        match portal.get()[idx].state.clone() {
                            SlotState::Empty => {
                                view! { <span class="p4-plus-glyph">"+"</span> }.into_any()
                            }
                            SlotState::Loading { .. } => {
                                view! { <span class="p4-slot-initial">{"\u{2026}"}</span> }.into_any()
                            }
                            SlotState::Loaded { ref display_name, .. } => {
                                let initial = display_name
                                    .chars()
                                    .next()
                                    .unwrap_or('?')
                                    .to_uppercase()
                                    .to_string();
                                view! { <span class="p4-slot-initial">{initial}</span> }.into_any()
                            }
                            SlotState::Error { .. } => {
                                view! { <span class="p4-slot-initial">"!"</span> }.into_any()
                            }
                        }
                    }}
                </GoldBezel>
                // REMOVE overlay for loaded slots
                {move || {
                    match portal.get()[idx].state.clone() {
                        SlotState::Loaded { figure_id: Some(fig), display_name, .. } => {
                            let fig_for_reset = fig.clone();
                            let name_for_reset = display_name.clone();
                            view! {
                                <div class="p4-slot-actions">
                                    <button class="p4-slot-action p4-slot-action--remove" on:click=move |e| {
                                        e.stop_propagation();
                                        leptos::task::spawn_local(async move {
                                            let _ = post_clear(slot_num).await;
                                        });
                                    }>
                                        "REMOVE"
                                    </button>
                                    <button
                                        class="p4-slot-action p4-slot-action--reset"
                                        title="Reset this figure to a fresh copy (wipes save progress)"
                                        on:click=move |e| {
                                            e.stop_propagation();
                                            reset_target.set(Some(ResetTarget {
                                                slot: slot_num,
                                                figure_id: fig_for_reset.clone(),
                                                display_name: name_for_reset.clone(),
                                            }));
                                        }
                                    >"RESET"</button>
                                </div>
                            }.into_any()
                        }
                        SlotState::Loaded { figure_id: None, .. } => {
                            view! {
                                <div class="p4-slot-actions">
                                    <button class="p4-slot-action p4-slot-action--remove" on:click=move |e| {
                                        e.stop_propagation();
                                        leptos::task::spawn_local(async move {
                                            let _ = post_clear(slot_num).await;
                                        });
                                    }>
                                        "REMOVE"
                                    </button>
                                </div>
                            }.into_any()
                        }
                        _ => view! { <span></span> }.into_any(),
                    }
                }}
            </div>
            // Slot label
            {move || {
                match portal.get()[idx].state.clone() {
                    SlotState::Empty => {
                        view! { <div class="p4-slot-label p4-slot-label--empty">"empty"</div> }.into_any()
                    }
                    SlotState::Loading { .. } => {
                        view! { <div class="p4-slot-label">{"\u{2026}"}</div> }.into_any()
                    }
                    SlotState::Loaded { display_name, .. } => {
                        view! { <div class="p4-slot-label">{display_name}</div> }.into_any()
                    }
                    SlotState::Error { message } => {
                        view! { <div class="p4-slot-label p4-slot-label--error">{message}</div> }.into_any()
                    }
                }
            }}
        </div>
    }
}
