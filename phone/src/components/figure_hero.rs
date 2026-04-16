use leptos::prelude::*;

use crate::model::Element;

use super::{BezelSize, BezelState, GoldBezel, RayHalo};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum HeroState {
    #[default]
    Default,
    Loading,
    Errored,
}

/// The lifted figure presentation used in the detail view and the
/// Kaos swap overlay. Composes an oversized `<GoldBezel>` with a
/// soft aura, slow-rotating `<RayHalo>`, and an optional loading
/// ring overlay.
///
/// See `design_language.md` §6.5.
#[component]
pub fn FigureHero(
    #[prop(optional)]
    element: Option<Element>,
    #[prop(optional, into)]
    state: Signal<HeroState>,
    children: Children,
) -> impl IntoView {
    let bezel_state = Signal::derive(move || match state.get() {
        HeroState::Default => BezelState::Default,
        HeroState::Loading => BezelState::Loading,
        HeroState::Errored => BezelState::Errored,
    });

    let wrapper_class = move || {
        let base = "figure-hero";
        match state.get() {
            HeroState::Default => base.to_string(),
            HeroState::Loading => format!("{base} hero-loading"),
            HeroState::Errored => format!("{base} hero-errored"),
        }
    };

    view! {
        <div class=wrapper_class>
            <div class="hero-aura"></div>
            <div class="hero-rays-wrap">
                <RayHalo />
            </div>
            <GoldBezel size=BezelSize::Hero element state=bezel_state>
                {children()}
            </GoldBezel>
            <div class="loading-ring"></div>
        </div>
    }
}
