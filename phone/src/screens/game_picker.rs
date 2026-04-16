use leptos::prelude::*;

use crate::api::post_launch;
use crate::model::InstalledGame;
use crate::{push_toast, ToastMsg};

#[component]
pub(crate) fn GamePicker(games: Vec<InstalledGame>, toasts: RwSignal<Vec<ToastMsg>>) -> impl IntoView {
    let launching = RwSignal::new(None::<String>);
    let is_empty = games.is_empty();
    view! {
        <section class="game-picker">
            <h2>"Pick a game"</h2>
            <Show when=move || is_empty fallback=|| ()>
                <div class="empty-msg">
                    "No Skylanders games found in RPCS3. Add them to the emulator first."
                </div>
            </Show>
            <div class="game-grid">
                {games.into_iter().map(|g| {
                    let serial = g.serial.clone();
                    let display_name = g.display_name.clone();
                    view! {
                        <button
                            class="game-card"
                            disabled=move || launching.get().is_some()
                            on:click=move |_| {
                                let s = serial.clone();
                                let n = display_name.clone();
                                launching.set(Some(s.clone()));
                                leptos::task::spawn_local(async move {
                                    if let Err(e) = post_launch(&s).await {
                                        push_toast(toasts, &format!("Launch failed: {e}"));
                                        launching.set(None);
                                    } else {
                                        push_toast(toasts, &format!("Launched {n}"));
                                        // WS GameChanged will flip the UI; keep the button
                                        // disabled until then.
                                    }
                                });
                            }
                        >
                            <div class="game-title">{g.display_name.clone()}</div>
                            <div class="game-serial">{g.serial.clone()}</div>
                        </button>
                    }
                }).collect_view()}
            </div>
        </section>
    }
}
