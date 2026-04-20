use leptos::prelude::*;

use crate::api::post_clear;
use crate::components::{BezelSize, BezelState, DisplayHeading, GoldBezel, HeadingSize};
use crate::model::{PublicProfile, Slot, SlotState, SLOT_COUNT};
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
    /// Known profiles, used to resolve a slot's `placed_by` id into the
    /// coloured-initial ownership pip (PLAN 4.18.17). Driven by App().
    known_profiles: RwSignal<Vec<PublicProfile>>,
) -> impl IntoView {
    view! {
        <section class="portal-p4">
            <DisplayHeading size=HeadingSize::Md>"PORTAL"</DisplayHeading>
            <div class="portal-p4-grid">
                {(0..SLOT_COUNT).map(|i| {
                    view! { <SlotView idx=i portal picking_for reset_target known_profiles /> }
                }).collect_view()}
            </div>
        </section>
    }
}

/// Resolve a slot's `placed_by` profile id into its public metadata so
/// the ownership pip can colour itself + show the right initial. Returns
/// `None` when the owner isn't in the fetched list (e.g. profile deleted
/// mid-session, or the load event predates the phone's profile fetch) —
/// the indicator is purely informational, so we'd rather render nothing
/// than a stale/misleading chip.
fn resolve_owner<'a>(
    placed_by: &str,
    profiles: &'a [PublicProfile],
) -> Option<&'a PublicProfile> {
    profiles.iter().find(|p| p.id == placed_by)
}

#[component]
fn SlotView(
    idx: usize,
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    picking_for: RwSignal<Option<u8>>,
    reset_target: RwSignal<Option<ResetTarget>>,
    known_profiles: RwSignal<Vec<PublicProfile>>,
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
                // Ownership pip (PLAN 4.18.17). Loaded or Loading slots
                // that carry a `placed_by` id render a small coloured
                // circle with the owner's initial, so in 2-player sessions
                // each kid can tell at a glance whose figure is whose.
                // Placed on the opposite corner from the "?" badge so the
                // two never overlap on the same slot.
                {move || {
                    let (owner_id_opt, is_loaded) = match portal.get()[idx].state.clone() {
                        SlotState::Loaded { placed_by, .. } => (placed_by, true),
                        SlotState::Loading { placed_by, .. } => (placed_by, false),
                        _ => (None, false),
                    };
                    let Some(owner_id) = owner_id_opt else {
                        return view! { <span></span> }.into_any();
                    };
                    let profiles = known_profiles.get();
                    let Some(owner) = resolve_owner(&owner_id, &profiles) else {
                        return view! { <span></span> }.into_any();
                    };
                    let initial = owner.display_name
                        .chars()
                        .next()
                        .unwrap_or('?')
                        .to_uppercase()
                        .to_string();
                    let style = format!(
                        "background: {color}; border-color: {color};",
                        color = owner.color,
                    );
                    let cls = if is_loaded {
                        "p4-slot-owner"
                    } else {
                        "p4-slot-owner p4-slot-owner--pending"
                    };
                    let title = format!("Placed by {}", owner.display_name);
                    view! {
                        <span
                            class=cls
                            style=style
                            title=title.clone()
                            aria-label=title
                        >
                            {initial}
                        </span>
                    }.into_any()
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

#[cfg(test)]
mod tests {
    //! Pure-function tests for the ownership-pip resolver. The rest of
    //! the portal view is a Leptos component and can only be driven from
    //! a browser harness — covered by the e2e suite. `resolve_owner` is
    //! the one piece of logic that decides whose colour/initial lands
    //! on a loaded slot, so a regression here would be hard to spot
    //! visually in a mixed-owner session.
    use super::*;

    fn profile(id: &str, name: &str, color: &str) -> PublicProfile {
        PublicProfile {
            id: id.into(),
            display_name: name.into(),
            color: color.into(),
        }
    }

    #[test]
    fn resolve_owner_finds_known_profile() {
        let ps = vec![
            profile("p1", "Alice", "#da5ad6"),
            profile("p2", "Bob", "#2aa6ff"),
        ];
        let found = resolve_owner("p2", &ps).expect("owner resolves");
        assert_eq!(found.display_name, "Bob");
        assert_eq!(found.color, "#2aa6ff");
    }

    #[test]
    fn resolve_owner_returns_none_for_unknown_id() {
        // Deleted profile mid-session, or a load event that predates the
        // phone's profile fetch — both end up here. Pip should be hidden,
        // not rendered with misleading stale data.
        let ps = vec![profile("p1", "Alice", "#da5ad6")];
        assert!(resolve_owner("p-missing", &ps).is_none());
    }

    #[test]
    fn resolve_owner_returns_none_on_empty_profile_list() {
        let ps: Vec<PublicProfile> = Vec::new();
        assert!(resolve_owner("anyone", &ps).is_none());
    }

    #[test]
    fn resolve_owner_matches_exact_id_not_substring() {
        // Belt-and-braces: ids are opaque server-minted strings, but make
        // sure we never accidentally render the wrong owner from a prefix
        // collision.
        let ps = vec![
            profile("p1", "Alice", "#fff"),
            profile("p10", "Dana", "#000"),
        ];
        let found = resolve_owner("p1", &ps).unwrap();
        assert_eq!(found.display_name, "Alice");
    }
}
