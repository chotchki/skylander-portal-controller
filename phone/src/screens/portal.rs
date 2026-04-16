use leptos::prelude::*;

use crate::api::post_clear;
use crate::model::{Slot, SlotState, SLOT_COUNT};
use crate::{push_toast, ToastMsg};

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
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    view! {
        <section class="portal">
            {(0..SLOT_COUNT).map(|i| {
                view! { <SlotView idx=i portal picking_for toasts /> }
            }).collect_view()}
        </section>
    }
}

#[component]
fn SlotView(
    idx: usize,
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    picking_for: RwSignal<Option<u8>>,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let slot_num = (idx + 1) as u8;

    let state_class = move || -> &'static str {
        if picking_for.get() == Some(slot_num) {
            return "picking";
        }
        match portal.get()[idx].state {
            SlotState::Empty => "empty",
            SlotState::Loading { .. } => "loading",
            SlotState::Loaded { .. } => "loaded",
            SlotState::Error { .. } => "errored",
        }
    };

    view! {
        <div class={move || format!("slot {}", state_class())}
             on:click=move |_| {
                 let is_empty = matches!(portal.get()[idx].state, SlotState::Empty | SlotState::Error { .. });
                 if is_empty {
                     picking_for.set(Some(slot_num));
                 }
             }>
            <div class="slot-index">{format!("Slot {slot_num}")}</div>
            {move || {
                match portal.get()[idx].state.clone() {
                    SlotState::Empty => view! { <div class="slot-name empty">"Empty"</div> }.into_any(),
                    SlotState::Loading { .. } => view! { <div class="slot-name">"Loading…"</div> }.into_any(),
                    SlotState::Loaded { display_name, .. } => {
                        view! { <div class="slot-name">{display_name}</div> }.into_any()
                    }
                    SlotState::Error { message } => {
                        view! { <div class="slot-err">{message}</div> }.into_any()
                    }
                }
            }}
            {move || {
                match portal.get()[idx].state.clone() {
                    SlotState::Empty | SlotState::Error { .. } => view! {
                        <div class="slot-actions">
                            <button class="slot-btn primary" on:click=move |e| {
                                e.stop_propagation();
                                picking_for.set(Some(slot_num));
                            }>
                                "Pick"
                            </button>
                        </div>
                    }.into_any(),
                    SlotState::Loading { .. } => view! {
                        <div class="slot-actions">
                            <button class="slot-btn" disabled=true>"…"</button>
                        </div>
                    }.into_any(),
                    SlotState::Loaded { figure_id: Some(fig), .. } => {
                        let fig_for_reset = fig.clone();
                        let toasts_for_reset = toasts;
                        view! {
                            <div class="slot-actions">
                                <button class="slot-btn danger" on:click=move |e| {
                                    e.stop_propagation();
                                    leptos::task::spawn_local(async move {
                                        let _ = post_clear(slot_num).await;
                                    });
                                }>
                                    "Remove"
                                </button>
                                <button
                                    class="slot-btn reset"
                                    title="Reset this figure to a fresh copy (wipes save progress)"
                                    on:click=move |e| {
                                        e.stop_propagation();
                                        let confirm = web_sys::window()
                                            .and_then(|w| {
                                                w.confirm_with_message(
                                                    "Reset this figure? All progress will be lost.",
                                                )
                                                .ok()
                                            })
                                            .unwrap_or(false);
                                        if !confirm {
                                            return;
                                        }
                                        let fig = fig_for_reset.clone();
                                        leptos::task::spawn_local(async move {
                                            if let Err(e) = crate::api::post_reset(slot_num, &fig).await {
                                                push_toast(
                                                    toasts_for_reset,
                                                    &format!("Reset failed: {e}"),
                                                );
                                            }
                                        });
                                    }
                                >"Reset"</button>
                            </div>
                        }.into_any()
                    }
                    SlotState::Loaded { figure_id: None, .. } => view! {
                        <div class="slot-actions">
                            <button class="slot-btn danger" on:click=move |e| {
                                e.stop_propagation();
                                leptos::task::spawn_local(async move {
                                    let _ = post_clear(slot_num).await;
                                });
                            }>
                                "Remove"
                            </button>
                        </div>
                    }.into_any(),
                }
            }}
        </div>
    }
}
