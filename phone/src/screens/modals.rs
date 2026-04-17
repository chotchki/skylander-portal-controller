use leptos::prelude::*;

use crate::api::post_load;
use crate::components::{BezelSize, DisplayHeading, FramedPanel, GoldBezel, HeadingSize};
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

    // Build a static list of figure preview data from the offer slots.
    let figures = move || {
        resume_offer.get().map(|offer| {
            offer
                .slots
                .iter()
                .filter_map(|s| {
                    if let SlotState::Loaded { display_name, .. } = s {
                        let initial = display_name
                            .chars()
                            .next()
                            .unwrap_or('?')
                            .to_uppercase()
                            .to_string();
                        Some((initial, display_name.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
    };

    view! {
        <section class="resume-scrim">
            <div class="resume-panel-wrap">
                <FramedPanel class="panel-in resume-panel">
                    <DisplayHeading size=HeadingSize::Md>
                        "WELCOME BACK"
                    </DisplayHeading>
                    <p class="resume-sub">"pick up where you left off?"</p>
                    <p class="resume-context">{"\u{2014} YOUR LAST ADVENTURE \u{2014}"}</p>

                    <div class="resume-fig-row">
                        {move || figures().unwrap_or_default().into_iter().map(|(initial, name)| {
                            view! {
                                <div class="resume-fig">
                                    <GoldBezel size=BezelSize::Md>
                                        <span>{initial}</span>
                                    </GoldBezel>
                                    <span class="resume-fig-name">{name}</span>
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                    </div>

                    <p class="resume-when">"saved layout"</p>

                    <div class="resume-actions">
                        <button
                            class="resume-btn resume-btn-primary"
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
                        >"RESUME"</button>
                        <button
                            class="resume-btn resume-btn-secondary"
                            on:click=move |_| resume_offer.set(None)
                        >"START FRESH"</button>
                    </div>
                </FramedPanel>
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
        <section class="takeover-void">
            <div class="takeover-hexgrid"></div>
            <div class="takeover-sparks"></div>

            <div class="takeover-viewport">
                // Kaos sigil placeholder — actual SVG mask wiring deferred
                <div class="kaos-sigil"></div>

                <h1 class="kaos-title">
                    "KAOS"
                    <span class="kaos-title-line2">"REIGNS!"</span>
                </h1>

                <div class="takeover-quote-card">
                    <div class="takeover-quote-open">{"\u{201C}"}</div>
                    <div class="takeover-quote-body">
                        {move || takeover
                            .get()
                            .map(|t| t.by_kaos.clone())
                            .unwrap_or_else(|| "Behold my magnificent wickedness!".into())}
                    </div>
                    <div class="takeover-quote-close">{"\u{201D}"}</div>
                    <div class="takeover-quote-attrib">{"\u{2014} KAOS"}</div>
                </div>

                <p class="takeover-info">
                    "your seat has been claimed \u{00B7} enter your pin to return"
                </p>

                <button
                    class="takeover-kick-btn"
                    on:click=move |_| {
                        if let Some(loc) = web_sys::window().map(|w| w.location()) {
                            let _ = loc.reload();
                        }
                    }
                >
                    "KICK BACK IN"
                </button>
            </div>

            <div class="takeover-vignette"></div>
        </section>
    }
}
