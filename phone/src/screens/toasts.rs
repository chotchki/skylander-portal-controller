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
                    let level_class = match t.level {
                        ToastLevel::Error => "toast toast-error",
                        ToastLevel::Warn => "toast toast-warn",
                        ToastLevel::Success => "toast toast-success",
                        ToastLevel::Info => "toast toast-info",
                    };
                    view! { <div class=level_class>{t.message}</div> }
                }
            />
        </div>
    }
}
