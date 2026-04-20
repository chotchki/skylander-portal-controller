//! `<ActionButton>` — the menu-action / blue-card pattern from
//! design_language.md §6.6.
//!
//! Stack: optional leading icon + bold title + optional description, all
//! inside the existing `.menu-action` blue-card surface (gold border, blue
//! gradient — see app.css). Two modes:
//!
//! - **Single-tap.** `hold_duration = None`. Plain `on:click` fires
//!   `on_fire` immediately.
//! - **Press-and-hold.** `hold_duration = Some(d)`. Pointer-down starts a
//!   timer; if the user holds past `d` (default `--dur-hold-confirm`,
//!   1200ms), the `.hold-fill` sweep completes, `.fired` flashes, and
//!   `on_fire` runs once. Lifting before `d` cancels with no fire.
//!
//! Reuses the existing `.menu-action*` CSS classes so MenuOverlay's
//! actions migrate without a CSS rewrite (PLAN 4.20.1). ResetConfirmModal's
//! custom red-bezel hold button stays inline — its visual treatment + post-
//! fire cascade animations are too far from this pattern to share cleanly.
//!
//! `on_fire` runs the moment hold completes (or instantly on tap). Any
//! post-fire choreography (close menu, dismiss modal, network call) lives
//! in the caller's callback.

use std::time::Duration;

use leptos::ev::PointerEvent;
use leptos::prelude::*;

use crate::gloo_timer;

/// Visual variant for the blue-card surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionVariant {
    /// Standard blue-gradient + gold border (SWITCH PROFILE, MANAGE
    /// PROFILES, HOLD TO SWITCH GAMES).
    Default,
    /// Red-tinted variant for irreversible actions (HOLD TO SHUT DOWN).
    Danger,
}

impl ActionVariant {
    fn css_modifier(self) -> &'static str {
        match self {
            ActionVariant::Default => "",
            ActionVariant::Danger => "menu-action--danger",
        }
    }
}

/// How long the `.fired` class lingers after a successful hold-fire so the
/// `fired` keyframe can play. The owning callback typically dismisses the
/// surrounding panel before this elapses, in which case the unmount
/// supersedes the timer; otherwise the button visually returns to rest.
const FIRED_LINGER_MS: u32 = 1000;

#[component]
pub fn ActionButton(
    /// Bold title line (Titan One). E.g. "SWITCH PROFILE",
    /// "HOLD TO SHUT DOWN". Required.
    #[prop(into)]
    title: String,
    /// Optional smaller subtitle line (Fraunces italic). Pass `""` or
    /// omit for no description.
    #[prop(into, optional)]
    description: Option<String>,
    /// Optional single-glyph leading icon. Pass `""` or omit to skip the
    /// icon column entirely.
    #[prop(into, optional)]
    icon: Option<String>,
    /// Visual variant. Defaults to `Default`.
    #[prop(default = ActionVariant::Default)]
    variant: ActionVariant,
    /// `None` = single-tap; `Some(d)` = press-and-hold for `d` before
    /// firing. Use `Duration::from_millis(1200)` to match the design's
    /// `--dur-hold-confirm` token.
    #[prop(default = None)]
    hold_duration: Option<Duration>,
    /// Fire handler. Runs once per successful tap (single-tap) or once
    /// per completed hold (hold mode).
    on_fire: Callback<()>,
) -> impl IntoView {
    let holding = RwSignal::new(false);
    let fired = RwSignal::new(false);

    let is_hold = hold_duration.is_some();
    let hold_ms = hold_duration
        .map(|d| d.as_millis() as u32)
        .unwrap_or(1200);

    let class = move || {
        let mut cls = String::from("menu-action");
        if is_hold {
            cls.push_str(" menu-action--hold");
        }
        let modifier = variant.css_modifier();
        if !modifier.is_empty() {
            cls.push(' ');
            cls.push_str(modifier);
        }
        if holding.get() {
            cls.push_str(" holding");
        }
        if fired.get() {
            cls.push_str(" fired");
        }
        cls
    };

    let on_fire_for_tap = on_fire;
    let on_click = move |_| {
        if is_hold {
            // Hold mode owns the fire — pointerdown's timer is the only
            // path. Stray clicks (e.g. keyboard Enter on focus) are
            // ignored to keep the hold contract honest.
            return;
        }
        on_fire_for_tap.run(());
    };

    let on_fire_for_hold = on_fire;
    let on_pointerdown = move |_: PointerEvent| {
        if !is_hold {
            return;
        }
        if fired.get_untracked() {
            return;
        }
        holding.set(true);
        leptos::task::spawn_local(async move {
            gloo_timer(hold_ms as i32).await;
            if !holding.get_untracked() || fired.get_untracked() {
                return;
            }
            holding.set(false);
            fired.set(true);
            on_fire_for_hold.run(());
            // Let the .fired keyframe play out, then return to rest if
            // the surrounding panel hasn't already unmounted us.
            leptos::task::spawn_local(async move {
                gloo_timer(FIRED_LINGER_MS as i32).await;
                fired.set(false);
            });
        });
    };

    let on_pointercancel = move |_: PointerEvent| {
        if !is_hold {
            return;
        }
        holding.set(false);
    };

    let icon_view = icon
        .filter(|s| !s.is_empty())
        .map(|i| view! { <div class="menu-action-icon">{i}</div> });

    let description_view = description
        .filter(|s| !s.is_empty())
        .map(|d| view! { <div class="menu-action-desc">{d}</div> });

    view! {
        <button
            class=class
            on:click=on_click
            on:pointerdown=on_pointerdown
            on:pointerup=on_pointercancel
            on:pointerleave=on_pointercancel
            on:pointercancel=on_pointercancel
        >
            <Show when=move || is_hold fallback=|| ()>
                <span class="hold-fill"></span>
            </Show>
            {icon_view}
            <div>
                <div class="menu-action-title">{title}</div>
                {description_view}
            </div>
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_modifier_produces_expected_class() {
        assert_eq!(ActionVariant::Default.css_modifier(), "");
        assert_eq!(ActionVariant::Danger.css_modifier(), "menu-action--danger");
    }
}
