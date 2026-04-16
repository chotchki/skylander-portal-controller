use leptos::prelude::*;

use crate::model::Element;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BezelSize {
    Sm,
    Md,
    Lg,
    Hero,
}

impl BezelSize {
    fn css_class(self) -> &'static str {
        match self {
            Self::Sm => "bezel-sm",
            Self::Md => "bezel-md",
            Self::Lg => "bezel-lg",
            Self::Hero => "bezel-hero",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BezelState {
    #[default]
    Default,
    Picking,
    Loading,
    Loaded,
    Errored,
    Disabled,
}

impl BezelState {
    fn css_class(self) -> &'static str {
        match self {
            Self::Default => "",
            Self::Picking => "bezel-picking",
            Self::Loading => "bezel-loading",
            Self::Loaded => "bezel-loaded",
            Self::Errored => "bezel-errored",
            Self::Disabled => "bezel-disabled",
        }
    }
}

/// The signature visual motif. A circular gold-gradient bezel ring wrapping
/// an element-tinted inner plate. The plate hosts arbitrary children — a
/// figure thumbnail, a profile initial, a "+" glyph, etc.
///
/// Sizes: Sm (32px) · Md (56px) · Lg (100–128px) · Hero (160px).
/// States drive visual treatment (glow, loading sweep, red errored, dimmed).
///
/// See `design_language.md` §3.1 for the full material spec.
#[component]
pub fn GoldBezel(
    #[prop(default = BezelSize::Lg)]
    size: BezelSize,
    #[prop(default = None)]
    element: Option<Element>,
    #[prop(optional, into)]
    state: Signal<BezelState>,
    children: Children,
) -> impl IntoView {
    let class = move || {
        let el = element.map(|e| e.css_class()).unwrap_or("");
        format!(
            "gold-bezel {} {} {}",
            size.css_class(),
            state.get().css_class(),
            el,
        )
    };

    view! {
        <div class=class>
            <div class="bezel-ring"></div>
            <div class="bezel-plate">
                {children()}
            </div>
        </div>
    }
}
