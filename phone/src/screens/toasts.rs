use leptos::prelude::*;

use crate::{ToastLevel, ToastMsg};

#[component]
pub(crate) fn ToastStack(toasts: RwSignal<Vec<ToastMsg>>) -> impl IntoView {
    view! {
        <div class="toast-stack">
            <For
                each=move || toasts.get()
                key=|t: &ToastMsg| t.id
                children=|t: ToastMsg| {
                    // Class + icon glyph both keyed to level so the status
                    // reads on colour-blind vision AND at-a-glance without
                    // squinting at the 2px left strip. Glyphs are left as
                    // semantic text (no aria-hidden) so screen readers
                    // narrate them alongside the message.
                    let (level_class, icon) = match t.level {
                        ToastLevel::Error   => ("toast toast-error",   "✕"),
                        ToastLevel::Warn    => ("toast toast-warn",    "⚠"),
                        ToastLevel::Success => ("toast toast-success", "✓"),
                        ToastLevel::Info    => ("toast toast-info",    "ℹ"),
                    };
                    view! {
                        <div class=level_class>
                            <span class="toast-icon">{icon}</span>
                            <span class="toast-msg">{t.message}</span>
                        </div>
                    }
                }
            />
        </div>
    }
}
