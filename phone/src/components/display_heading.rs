use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingSize {
    Hero,
    Lg,
    Md,
    Sm,
    Xs,
}

impl HeadingSize {
    fn css_class(self) -> &'static str {
        match self {
            Self::Hero => "dh-hero",
            Self::Lg => "dh-lg",
            Self::Md => "dh-md",
            Self::Sm => "dh-sm",
            Self::Xs => "dh-xs",
        }
    }
}

/// Gold-fill + dark-gold stroke + drop-shadow display heading.
/// Wraps children in the full heraldic title treatment from
/// `design_language.md` §2.
///
/// Optionally includes a `.title-rays` halo behind the text
/// (set `with_rays = true`).
#[component]
pub fn DisplayHeading(
    #[prop(default = HeadingSize::Md)]
    size: HeadingSize,
    #[prop(default = false)]
    with_rays: bool,
    children: Children,
) -> impl IntoView {
    let cls = format!("display-heading {}", size.css_class());

    view! {
        <div class=cls>
            {if with_rays {
                Some(view! { <div class="title-rays"></div> })
            } else {
                None
            }}
            <span class="dh-text">{children()}</span>
        </div>
    }
}
