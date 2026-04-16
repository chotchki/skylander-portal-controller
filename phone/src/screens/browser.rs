use leptos::prelude::*;

use crate::api::post_load;
use crate::model::{Element, PublicFigure, Slot, SlotState, SLOT_COUNT};
use crate::{element_short, element_slug, event_target_value, first_empty_slot, push_toast, ToastMsg};

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
    //     Used to render the "on portal" visual + fire the "Already on the
    //     portal" toast when the user taps an already-loaded card.
    //   - `loading_ids`   — figure_id markers for Loading slots. Used to
    //     silently suppress repeat taps during the Empty → Loading → Loaded
    //     transition (the spam-click case in 3.6.1) without firing a toast
    //     that the user didn't cause.
    //
    // We compare Loaded by display_name because the server doesn't echo a
    // figure_id back on Loaded events yet (see PLAN 3.8 — name reconciliation).
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
                SlotState::Loading { figure_id: Some(id), .. } => Some(id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    });

    view! {
        <BrowserHead element_filter search />
        <div class="grid">
            <For
                each=move || filtered.get()
                key=|f: &PublicFigure| f.id.clone()
                children=move |f: PublicFigure| {
                    let id = f.id.clone();
                    let id_for_img = id.clone();
                    let name = f.canonical_name.clone();
                    let name_for_img = name.clone();
                    let elem = f.element;
                    let variant = f.variant_tag.clone();
                    let variant_for_show = variant.clone();

                    let name_for_loaded = name.clone();
                    let id_for_loading = id.clone();
                    let is_loaded_this = move || {
                        loaded_names.get().iter().any(|n| n == &name_for_loaded)
                    };
                    let is_loading_this =
                        move || loading_ids.get().iter().any(|id| id == &id_for_loading);

                    let loaded_for_class = is_loaded_this.clone();
                    let loaded_for_click = is_loaded_this.clone();
                    let loaded_for_badge = is_loaded_this.clone();
                    let loading_for_class = is_loading_this.clone();
                    let loading_for_click = is_loading_this.clone();

                    // Per-card transient back-pressure: goes true between the
                    // click firing and the load response returning (ok or
                    // 429). While true, the button reports `disabled` — the
                    // DOM-level disable swallows extra clicks without running
                    // the handler, so spam taps don't pile up either load
                    // requests or toasts. The "Already on the portal" toast
                    // still fires correctly for on-portal cards because
                    // `on_portal` is a separate state (Loaded, not
                    // Submitting).
                    let submitting = RwSignal::new(false);
                    let submitting_for_disabled = submitting;

                    view! {
                        <button
                            class=move || {
                                // `.card.on-portal` is terminal-state only
                                // (the figure is Loaded on a slot), so e2e
                                // tests can wait for it to know a load has
                                // fully completed. `.card.loading` is the
                                // transient state — same visual, different
                                // selector. Lets the spam-click test sit
                                // silent during Loading and the "already"
                                // test distinguish when the toast is due.
                                if loaded_for_class() {
                                    "card on-portal"
                                } else if loading_for_class() {
                                    "card loading"
                                } else {
                                    "card"
                                }
                            }
                            disabled=move || submitting_for_disabled.get()
                            on:click=move |_| {
                                // Three gates, silent → toast:
                                //   1. local submitting — this card just fired
                                //      a load and the 202 hasn't returned yet.
                                //   2. any slot currently Loading this figure —
                                //      the server accepted a prior tap but the
                                //      load hasn't completed. Silent swallow so
                                //      spam taps during Empty→Loading→Loaded
                                //      don't generate toasts.
                                //   3. any slot Loaded with this figure — user
                                //      is trying to re-add; surface "Already".
                                if submitting.get() || loading_for_click() {
                                    return;
                                }
                                if loaded_for_click() {
                                    push_toast(toasts, "Already on the portal.");
                                    return;
                                }
                                let slot = match picking_for.get() {
                                    Some(s) => s,
                                    None => match first_empty_slot(&portal.get()) {
                                        Some(s) => s,
                                        None => {
                                            push_toast(toasts, "Portal is full — remove a figure first.");
                                            return;
                                        }
                                    },
                                };
                                picking_for.set(None);
                                submitting.set(true);
                                let id = id.clone();
                                leptos::task::spawn_local(async move {
                                    let res = post_load(slot, &id).await;
                                    submitting.set(false);
                                    match res {
                                        Ok(()) => {}
                                        Err(e) if e.contains("429") => {}
                                        Err(e) => push_toast(toasts, &format!("Load failed: {e}")),
                                    }
                                });
                            }
                        >
                            <div class="card-icon" data-element=element_slug(elem)>
                                <img
                                    class="card-thumb"
                                    src=format!("/api/figures/{id_for_img}/image?size=thumb")
                                    alt=name_for_img
                                    loading="lazy"
                                    decoding="async"
                                />
                                <span class="card-icon-label">{element_short(elem)}</span>
                            </div>
                            <div class="card-name">{name}</div>
                            <Show when=move || variant_for_show != "base" fallback=|| ()>
                                <div class="card-variant">{variant.clone()}</div>
                            </Show>
                            <Show when=move || loaded_for_badge() fallback=|| ()>
                                <div class="on-portal-badge">"On portal"</div>
                            </Show>
                        </button>
                    }
                }
            />
        </div>
    }
}

#[component]
fn BrowserHead(
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
        <div class="browser-head">
            <input
                class="search"
                type="search"
                placeholder="Search…"
                prop:value=move || search.get()
                on:input=move |e| search.set(event_target_value(&e))
            />
        </div>
        <div class="chip-row">
            {all_elements.into_iter().map(|(val, label)| {
                let v = val;
                view! {
                    <button
                        class={move || if element_filter.get() == v { "chip active" } else { "chip" }}
                        on:click=move |_| element_filter.set(v)
                    >
                        {label}
                    </button>
                }
            }).collect_view()}
        </div>
    }
}
