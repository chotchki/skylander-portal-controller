use leptos::prelude::*;

use crate::ToastMsg;

#[component]
pub(crate) fn ToastStack(toasts: RwSignal<Vec<ToastMsg>>) -> impl IntoView {
    view! {
        <div class="toast-stack">
            <For
                each=move || toasts.get()
                key=|t: &ToastMsg| t.id
                children=|t: ToastMsg| view! { <div class="toast">{t.message}</div> }
            />
        </div>
    }
}
