//! The "flag this" field (openspec AS.440): a playtester's note for what the
//! TV shows now, sent with a capture of it. Shown only when the server says
//! it takes flags (`GET /api/_test/flag`), which only guacamole's
//! `test-hooks` build does; the product and a plain guacamole answer no
//! route, so no field.

use leptos::prelude::*;

use crate::api::post_flag;
use crate::{push_toast, ToastMsg};

#[component]
pub(crate) fn FlagThis(toasts: RwSignal<Vec<ToastMsg>>) -> impl IntoView {
    let note = RwSignal::new(String::new());
    let sending = RwSignal::new(false);
    let on_send = move |_| {
        if sending.get_untracked() {
            return;
        }
        sending.set(true);
        let text = note.get_untracked();
        leptos::task::spawn_local(async move {
            match post_flag(&text).await {
                Ok(taken) => {
                    note.set(String::new());
                    let how = if taken.joined { "added to" } else { "saved as" };
                    push_toast(toasts, &format!("Flag {how} {}", taken.folder));
                }
                Err(e) => push_toast(toasts, &format!("Flag not saved: {e}")),
            }
            sending.set(false);
        });
    };
    view! {
        <div class="flag-this flex flex-col gap-2 mb-4 p-3 rounded-2xl bg-sf-1 border-2 border-gold-mid">
            <label class="font-display text-gold-bright text-sm tracking-wide" for="flag-note">
                "\u{2691} FLAG THIS"
            </label>
            <textarea
                id="flag-note"
                class="flag-note w-full min-h-16 p-2 rounded-lg text-base text-black bg-white"
                placeholder="What's wrong on the TV right now?"
                prop:value=move || note.get()
                on:input=move |ev| note.set(event_target_value(&ev))
            ></textarea>
            <button
                class="flag-send self-end px-4 py-2 rounded-lg font-display text-sm bg-gold-bright text-black disabled:opacity-50"
                disabled=move || sending.get()
                on:click=on_send
            >
                "SEND"
            </button>
        </div>
    }
}
