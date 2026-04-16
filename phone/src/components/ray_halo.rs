use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaloSpeed {
    Slow,
    Fast,
}

/// Rotating conic-gradient halo rendered behind a `<GoldBezel>`.
/// Used for picking / loading states and the hero figure reveal.
///
/// Tight to the bezel (`inset: -8px`), masked to a ring so it
/// fades at the edges. `prefers-reduced-motion` aware.
///
/// See `design_language.md` §5.2.
#[component]
pub fn RayHalo(
    #[prop(default = HaloSpeed::Slow)]
    speed: HaloSpeed,
) -> impl IntoView {
    let cls = match speed {
        HaloSpeed::Slow => "ray-halo ray-halo-slow",
        HaloSpeed::Fast => "ray-halo ray-halo-fast",
    };
    view! { <div class=cls></div> }
}
