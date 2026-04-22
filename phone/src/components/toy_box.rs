//! Toy-box lid + interior pair (design_language.md §6.7 gesture model;
//! §3.4 wood material).
//!
//! These two components always coexist on the portal screen: the slatted-
//! wood lid sits at the top below the header, the dark well below it
//! contains the figure grid. Both react to a shared `BoxState` signal
//! that the owning screen passes in.
//!
//! Extracted from `phone/src/screens/browser.rs` per PLAN 4.20.2 — the
//! gesture state machine + scroll-collapse rule belong with the markup
//! they drive, not with the figures collection.

use leptos::ev::PointerEvent;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// Three visual states the lid + interior co-render through. Pure data;
/// owning screen drives the signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxState {
    Closed,
    Compact,
    Expanded,
}

impl BoxState {
    /// CSS class suffix appended to `lid-open-p4 ` so the same selector
    /// owns all three visual variants.
    pub fn css_modifier(self) -> &'static str {
        match self {
            BoxState::Closed => "closed",
            BoxState::Compact => "",
            BoxState::Expanded => "expanded",
        }
    }
}

/// Vertical-drag distance on the lid that commits to a swipe (px). Below
/// this the gesture is treated as either an in-progress drag (no state
/// change yet) or, on release, a tap.
const SWIPE_THRESHOLD_PX: f64 = 48.0;

/// Cursor travel cap that still counts the pointerup as a tap. Above
/// this we treat the gesture as a too-short swipe and ignore.
const TAP_MAX_TRAVEL_PX: f64 = 10.0;

/// Wooden lid that sits at the top of the portal screen below the header.
/// Owns the gesture state machine (tap → cycle, swipe up → up-state, swipe
/// down → down-state) per `design_language.md` §6.7. The expanded-area
/// `children` only mount when `box_state == Expanded`.
///
/// Caller passes whatever filter/search UI belongs inside the open lid;
/// the lid itself has no opinion on what fills the expanded slot.
#[component]
pub fn ToyBoxLid(
    box_state: RwSignal<BoxState>,
    /// Title shown in the compact top row (defaults to "COLLECTION").
    #[prop(into, optional)]
    title: Option<String>,
    /// Expanded-area content. Mounted inside a `<Show when=Expanded>` so
    /// it doesn't render at all in Compact / Closed states. `ChildrenFn`
    /// (not `Children`) because Show's body closure must be `Sync` and
    /// `FnOnce` doesn't satisfy that.
    children: ChildrenFn,
) -> impl IntoView {
    let title = title.unwrap_or_else(|| "COLLECTION".to_string());

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
        // if the finger drifts onto the grid or off-screen. Anchor on
        // `current_target` (the listener element) not `target` — the hit
        // target is often a descendant text node/span, and capturing on a
        // descendant breaks once the finger drifts off that sub-element.
        if let Some(target) = ev.current_target() {
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

        if let (Some(pid), Some(target)) = (pointer_id, ev.current_target()) {
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
                <span class="lid-open-title-p4">{title}</span>
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
                    {children()}
                </div>
            </Show>
        </div>
    }
}

/// Dark well that sits below `ToyBoxLid` and renders the actual collection.
/// Auto-collapses the lid from Expanded → Compact when the scroll container
/// is scrolled (per §6.7's "scrolling looks deeper into the box" rule).
///
/// Caller's `children` go inside the inner scroll container — typically a
/// figure grid or an empty-state placeholder.
#[component]
pub fn ToyBoxInterior(
    box_state: RwSignal<BoxState>,
    children: Children,
) -> impl IntoView {
    let on_grid_scroll = move |_| {
        if box_state.get_untracked() == BoxState::Expanded {
            box_state.set(BoxState::Compact);
        }
    };

    view! {
        <div class="box-body-bg">
            <div class="box-body-fade-top"></div>
            <div class="box-body-fade-bottom"></div>
            <div class="box-body-scroll" on:scroll=on_grid_scroll>
                {children()}
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_modifier_matches_state() {
        assert_eq!(BoxState::Closed.css_modifier(), "closed");
        assert_eq!(BoxState::Compact.css_modifier(), "");
        assert_eq!(BoxState::Expanded.css_modifier(), "expanded");
    }
}
