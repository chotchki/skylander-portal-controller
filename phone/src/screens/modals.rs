use leptos::prelude::*;

use crate::api::post_load;
use crate::model::SlotState;
use crate::{push_toast, ResumeOffer, TakeoverReason, ToastMsg};

#[component]
pub(crate) fn ResumeModal(
    resume_offer: RwSignal<Option<ResumeOffer>>,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    // Overlay modal offering to reload the profile's last portal layout.
    // "Resume" issues one `/api/portal/slot/:n/load` per non-empty slot;
    // "Start fresh" just dismisses. Either way we clear `resume_offer` —
    // the next unlock will re-fire if there's a (possibly different) layout.
    //
    // 2-phone nuance (SPEC Round 4): if the portal *isn't* empty (the other
    // phone already has figures down), a blanket resume would double-load
    // or collide. For now we label the button accordingly but always go
    // through the same per-slot calls; the server's slot-busy back-pressure
    // handles collisions. Proper 3-option modal (`clear + resume` vs
    // `alongside current` vs `fresh`) lands with PLAN 3.10.9 follow-up.
    view! {
        <section class="resume-modal">
            <div class="resume-card">
                <h3 class="resume-title">"Resume last setup?"</h3>
                <p class="resume-body">"You left these figures on the portal last time."</p>
                <div class="resume-actions">
                    <button
                        class="resume-yes"
                        on:click=move |_| {
                            let offer = match resume_offer.get() {
                                Some(o) => o,
                                None => return,
                            };
                            resume_offer.set(None);
                            let slots = offer.slots.clone();
                            leptos::task::spawn_local(async move {
                                for (i, state) in slots.iter().enumerate() {
                                    if let SlotState::Loaded { figure_id: Some(id), .. } = state {
                                        let slot_1_indexed = (i + 1) as u8;
                                        let id = id.clone();
                                        match post_load(slot_1_indexed, &id).await {
                                            Ok(()) => {}
                                            Err(e) if e.contains("429") => {}
                                            Err(e) => {
                                                push_toast(
                                                    toasts,
                                                    &format!("Resume slot {slot_1_indexed}: {e}"),
                                                );
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    >"Resume"</button>
                    <button
                        class="resume-no"
                        on:click=move |_| resume_offer.set(None)
                    >"Start fresh"</button>
                </div>
            </div>
        </section>
    }
}

#[component]
pub(crate) fn TakeoverScreen(takeover: RwSignal<Option<TakeoverReason>>) -> impl IntoView {
    // Kaos took the slot. "Kick back" does a full page reload — the browser
    // opens a fresh WS, server tries to re-admit via the FIFO path. If the
    // 1-minute cooldown is still active, the reload-WS gets an `Error` event
    // and closes. If cooldown elapsed, we land back at the ProfilePicker
    // (server re-locks all profiles on a fresh session so PIN re-entry is
    // required — SPEC Q46).
    view! {
        <section class="takeover">
            <h2 class="takeover-title">"You've been taken over!"</h2>
            <p class="takeover-quote">
                {move || takeover
                    .get()
                    .map(|t| format!("— {}", t.by_kaos))
                    .unwrap_or_default()}
            </p>
            <p class="takeover-body">
                "Another portal master claimed your seat. You can kick back, "
                "but you'll need to re-enter your PIN."
            </p>
            <button
                class="takeover-kick"
                on:click=move |_| {
                    if let Some(loc) = web_sys::window().map(|w| w.location()) {
                        let _ = loc.reload();
                    }
                }
            >
                "Kick back"
            </button>
        </section>
    }
}
