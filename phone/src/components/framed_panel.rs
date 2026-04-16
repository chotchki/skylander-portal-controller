use leptos::prelude::*;

/// A parchment-blue panel with a multi-stop gold gradient border.
/// Used for every modal / overlay surface: PIN keypad, figure detail,
/// menu overlay, resume prompt, confirms, Kaos takeover.
///
/// No corner brackets — the gradient border does the framing.
/// Entrance animation handled by the caller (add `panel-in` class).
///
/// See `design_language.md` §3.3.
#[component]
pub fn FramedPanel(
    #[prop(optional)]
    class: &'static str,
    children: Children,
) -> impl IntoView {
    let cls = if class.is_empty() {
        "framed-panel".to_string()
    } else {
        format!("framed-panel {class}")
    };

    view! {
        <div class=cls>
            {children()}
        </div>
    }
}
