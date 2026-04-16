use leptos::prelude::*;

use crate::api::post_launch;
use crate::components::{DisplayHeading, HeadingSize};
use crate::model::InstalledGame;
use crate::{push_toast, ToastMsg};

#[component]
pub(crate) fn GamePicker(games: Vec<InstalledGame>, toasts: RwSignal<Vec<ToastMsg>>) -> impl IntoView {
    let launching = RwSignal::new(None::<String>);
    let is_empty = games.is_empty();
    view! {
        <section class="game-picker">
            <DisplayHeading size=HeadingSize::Lg with_rays=true>
                "PICK A GAME"
            </DisplayHeading>
            <div class="gp-subtitle">"choose your adventure"</div>
            <Show when=move || is_empty fallback=|| ()>
                <div class="empty-msg">
                    "No Skylanders games found in RPCS3. Add them to the emulator first."
                </div>
            </Show>
            <div class="game-grid">
                {games.into_iter().enumerate().map(|(i, g)| {
                    let serial = g.serial.clone();
                    let display_name = g.display_name.clone();
                    let slug = game_slug(&g.display_name);
                    let short_name = game_short_name(&g.display_name);
                    let serial_for_class = serial.clone();
                    let delay_style = format!("animation-delay: {}ms", i * 80);
                    view! {
                        <button
                            class=move || {
                                let base = format!("game-card game-card--{slug}");
                                if launching.get().as_deref() == Some(&serial_for_class) {
                                    format!("{base} launching")
                                } else if launching.get().is_some() {
                                    format!("{base} dimmed")
                                } else {
                                    base
                                }
                            }
                            style=delay_style
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
                                    }
                                });
                            }
                        >
                            <span class="game-name">{short_name}</span>
                        </button>
                    }
                }).collect_view()}
            </div>
        </section>
    }
}

/// Map a display name like "Skylanders: Spyro's Adventure" to a CSS slug.
fn game_slug(display_name: &str) -> &'static str {
    let lower = display_name.to_lowercase();
    if lower.contains("spyro") { "ssa" }
    else if lower.contains("giant") { "giants" }
    else if lower.contains("swap") { "swap" }
    else if lower.contains("trap") { "trap" }
    else if lower.contains("supercharger") { "super" }
    else if lower.contains("imaginator") { "imag" }
    else { "unknown" }
}

/// Strip the "Skylanders: " prefix for the big card label.
fn game_short_name(display_name: &str) -> String {
    display_name
        .strip_prefix("Skylanders: ")
        .unwrap_or(display_name)
        .to_string()
}
