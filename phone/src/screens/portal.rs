use leptos::prelude::*;

use crate::api::post_clear;
use crate::components::{BezelSize, BezelState, DisplayHeading, GoldBezel, HeadingSize};
use crate::model::{PublicProfile, Slot, SlotState, SLOT_COUNT};

#[component]
pub(crate) fn Picking(picking_for: RwSignal<Option<u8>>) -> impl IntoView {
    // PLAY_TEST #22 (Chris 2026-04-24): the "Pick a Skylander for slot N"
    // banner is gone — per-slot targeting was noisy, and the `+` glyphs
    // on empty slots were redundant. The toy-box lid below is the single
    // entry point for adding a figure now (see `.portal-empty-hint` in
    // `portal.rs`). The Picking component stays as a no-op so its call
    // site in lib.rs doesn't need editing; if we ever want a per-slot
    // target flow back, this is the spot.
    let _ = picking_for;
    view! { <></> }
}

#[component]
pub(crate) fn Portal(
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    picking_for: RwSignal<Option<u8>>,
    /// Known profiles, used to resolve a slot's `placed_by` id into the
    /// coloured-initial ownership pip (PLAN 4.18.17). Driven by App().
    known_profiles: RwSignal<Vec<PublicProfile>>,
) -> impl IntoView {
    // Which loaded slot currently has its REMOVE bar revealed. Tap-to-arm,
    // 5s auto-dismiss, tap elsewhere to cancel — `selection_token` gives
    // each arming a unique cookie so a stale timer can't kill a fresh
    // selection. Matches the mock in `docs/aesthetic/mocks/portal_with_box.html`.
    let selected_slot = RwSignal::new(None::<u8>);
    let selection_token = StoredValue::new(0u32);

    view! {
        <section class="portal-p4">
            <DisplayHeading size=HeadingSize::Md>"PORTAL"</DisplayHeading>
            // PLAY_TEST round 2 (PLAN 8.3): kids tapped empty slot
            // bezels expecting something to happen — they're inert
            // (placement happens through the toy box). Pull empty
            // slots out of the DOM entirely; only Loaded / Loading /
            // Error slots render. The toy-box arrow hint below the
            // grid is then the only call-to-action when nothing is
            // placed, and naturally pushes down once any slot fills.
            <div class="portal-p4-grid">
                {(0..SLOT_COUNT).map(|i| {
                    view! {
                        <Show when=move || !matches!(portal.get()[i].state, SlotState::Empty)>
                            <SlotView idx=i portal picking_for known_profiles selected_slot selection_token />
                        </Show>
                    }
                }).collect_view()}
            </div>
            <div class="portal-empty-hint" aria-live="polite">
                <span class="portal-empty-hint-arrow">"\u{2193}"</span>
                <span>"open the toy box to add a figure"</span>
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
fn resolve_owner<'a>(placed_by: &str, profiles: &'a [PublicProfile]) -> Option<&'a PublicProfile> {
    profiles.iter().find(|p| p.id == placed_by)
}

#[component]
fn SlotView(
    idx: usize,
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    picking_for: RwSignal<Option<u8>>,
    known_profiles: RwSignal<Vec<PublicProfile>>,
    selected_slot: RwSignal<Option<u8>>,
    selection_token: StoredValue<u32>,
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

    let is_selected = move || selected_slot.get() == Some(slot_num);

    let slot_class = move || {
        let base = "p4-slot";
        let state = match portal.get()[idx].state {
            SlotState::Empty => "p4-slot--empty",
            SlotState::Loading { .. } => "p4-slot--loading",
            SlotState::Loaded { .. } => "p4-slot--loaded",
            SlotState::Error { .. } => "p4-slot--errored",
        };
        let sel = if is_selected() {
            " p4-slot--selected"
        } else {
            ""
        };
        format!("{base} {state}{sel}")
    };

    // Tap arms a loaded slot for REMOVE; tap again (or wait 5s) unarms.
    // Empty and loading slots are no-ops — the user adds figures through
    // the toy-box lid below, pointed at by `.portal-empty-hint`. Errored
    // slots stay tappable as an escape hatch (clear + retry via picker).
    // PLAY_TEST #22 dropped the per-slot picker flow. `picking_for` is
    // still threaded through for future re-introduction but no longer
    // set from the portal.
    let _ = picking_for;
    let on_slot_click = move |_| match portal.get()[idx].state {
        SlotState::Empty | SlotState::Loading { .. } => {}
        SlotState::Error { .. } => {
            // Clear the errored slot so the user can retry without a
            // dead slot blocking progress. Non-destructive — `post_clear`
            // just drops the server-side record.
            selection_token.update_value(|t| *t += 1);
            selected_slot.set(None);
            leptos::task::spawn_local(async move {
                let _ = post_clear(slot_num).await;
            });
        }
        SlotState::Loaded { .. } => {
            selection_token.update_value(|t| *t += 1);
            if is_selected() {
                selected_slot.set(None);
            } else {
                let token = selection_token.get_value();
                selected_slot.set(Some(slot_num));
                leptos::task::spawn_local(async move {
                    crate::gloo_timer(5000).await;
                    if selection_token.get_value() == token {
                        selected_slot.set(None);
                    }
                });
            }
        }
    };

    view! {
        <div class=slot_class on:click=on_slot_click>
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
                    // Inline `--profile-color` so the heraldic plate inside
                    // the mini bezel tints to the owner's swatch. Gold ring
                    // is fixed (all owners get the same frame); only the
                    // plate varies.
                    let style = format!("--profile-color: {};", owner.color);
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
                            <span class="p4-slot-owner-ring"></span>
                            <span class="p4-slot-owner-plate">{initial}</span>
                        </span>
                    }.into_any()
                }}
                <GoldBezel size=BezelSize::Lg state=bezel_state>
                    {move || {
                        match portal.get()[idx].state.clone() {
                            SlotState::Empty => {
                                // Empty = blank bezel. PLAY_TEST #22 dropped
                                // the "+" placeholder in favour of the
                                // single `.portal-empty-hint` pointing at
                                // the toy-box lid below.
                                view! { <span></span> }.into_any()
                            }
                            SlotState::Loading { .. } => {
                                view! { <span class="p4-slot-initial">{"\u{2026}"}</span> }.into_any()
                            }
                            SlotState::Loaded { ref display_name, ref figure_id, .. } => {
                                let initial = display_name
                                    .chars()
                                    .next()
                                    .unwrap_or('?')
                                    .to_uppercase()
                                    .to_string();
                                // Portrait on top of the initial fallback so
                                // matched figures show their wiki thumb and
                                // unmatched (`figure_id: None`) figures still
                                // show *something* readable. Same pattern as
                                // `.fig-bezel-p4 .fig-image-p4` in the grid
                                // card, scoped to `.p4-slot-image` so the
                                // portal-sized bezel gets its own sizing.
                                let img_view = figure_id.clone().map(|id| {
                                    view! {
                                        <img
                                            class="p4-slot-image"
                                            src=format!("/api/figures/{id}/image?size=thumb")
                                            alt=""
                                            loading="eager"
                                            decoding="async"
                                        />
                                    }
                                });
                                view! {
                                    <span class="p4-slot-initial">{initial}</span>
                                    {img_view}
                                }.into_any()
                            }
                            SlotState::Error { .. } => {
                                view! { <span class="p4-slot-initial">"!"</span> }.into_any()
                            }
                        }
                    }}
                </GoldBezel>
                // REMOVE overlay for loaded slots. Always rendered (for both
                // matched and unmatched figures) so CSS can animate its
                // reveal/hide via the parent's `.p4-slot--selected` class;
                // RESET moved off the portal slot to the figure-detail view
                // — the portal slot should be a single, low-friction "take
                // it off" action (portal_with_box.html mock).
                {move || {
                    if matches!(portal.get()[idx].state, SlotState::Loaded { .. }) {
                        view! {
                            <div class="p4-slot-actions">
                                <button class="p4-slot-action p4-slot-action--remove" on:click=move |e| {
                                    e.stop_propagation();
                                    selection_token.update_value(|t| *t += 1);
                                    selected_slot.set(None);
                                    leptos::task::spawn_local(async move {
                                        let _ = post_clear(slot_num).await;
                                    });
                                }>
                                    "REMOVE"
                                </button>
                            </div>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
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
