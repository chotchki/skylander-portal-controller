use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::post_edit_figure;
use crate::components::{DisplayHeading, FramedPanel, HeadingSize};

/// Modal/sheet overlay for editing a figure's level + gold (PLAN 11).
///
/// Mounted by [`super::FigureDetail`] when the user taps the STATS button,
/// which is only enabled for editable categories (`Figure / Sidekick /
/// Giant / Kaos`) when the figure is off the portal. Seeds the steppers
/// from the current stats; on SAVE posts to
/// `/api/profiles/:profile_id/figures/:figure_id/edit` and fires
/// `on_saved` so the caller can re-fetch its stats.
#[component]
pub(crate) fn FigureEditSheet(
    /// Display name shown in the sheet heading ("Spyro").
    figure_name: String,
    /// Profile id keying the working-copy edit endpoint.
    profile_id: String,
    /// Figure id keying the same endpoint.
    figure_id: String,
    /// Current level read from the stats fetch — seeds the level stepper.
    initial_level: u8,
    /// Current gold — seeds the gold stepper.
    initial_gold: u16,
    /// Max level for the figure's generation (10 / 15 / 20). Clamps the level
    /// stepper's upper bound — earlier-generation figures can't exceed their
    /// in-game cap, see `sky-parser::max_level_for`.
    max_level: u8,
    /// Dismiss the sheet without saving.
    on_close: Callback<()>,
    /// Fired after a successful POST so the caller can re-fetch its stats.
    on_saved: Callback<()>,
) -> impl IntoView {
    let level = RwSignal::new(initial_level.max(1).min(max_level));
    let gold = RwSignal::new(initial_gold);
    let saving = RwSignal::new(false);
    let error_msg = RwSignal::new(String::new());

    let on_cancel = move |_| on_close.run(());

    let on_save = {
        let profile_id = profile_id.clone();
        let figure_id = figure_id.clone();
        move |_| {
            if saving.get() {
                return;
            }
            saving.set(true);
            error_msg.set(String::new());
            let pid = profile_id.clone();
            let fid = figure_id.clone();
            let lvl = level.get();
            let g = gold.get();
            spawn_local(async move {
                match post_edit_figure(&pid, &fid, lvl, g).await {
                    Ok(()) => {
                        on_saved.run(());
                        on_close.run(());
                    }
                    Err(e) => {
                        saving.set(false);
                        error_msg.set(e);
                    }
                }
            });
        }
    };

    let dec_level = move |_| level.update(|v| *v = v.saturating_sub(1).max(1));
    let inc_level = move |_| level.update(|v| *v = (*v + 1).min(max_level));
    let dec_gold_small = move |_| gold.update(|v| *v = v.saturating_sub(100));
    let inc_gold_small = move |_| gold.update(|v| *v = v.saturating_add(100));
    let dec_gold_big = move |_| gold.update(|v| *v = v.saturating_sub(1000));
    let inc_gold_big = move |_| gold.update(|v| *v = v.saturating_add(1000));

    view! {
        <section class="edit-scrim">
            <div class="edit-panel-wrap">
                <FramedPanel class="panel-in edit-panel">
                    <DisplayHeading size=HeadingSize::Md>
                        "EDIT STATS"
                    </DisplayHeading>
                    <p class="edit-sub">{figure_name}</p>

                    <div class="edit-stepper-row">
                        <div class="edit-stepper-label">"LEVEL"</div>
                        <div class="edit-stepper-controls">
                            <button
                                class="edit-stepper-btn"
                                on:click=dec_level
                                aria-label="Decrease level"
                                disabled=Signal::derive(move || level.get() <= 1 || saving.get())
                            >
                                "\u{2212}"
                            </button>
                            <div class="edit-stepper-value">{move || level.get().to_string()}</div>
                            <button
                                class="edit-stepper-btn"
                                on:click=inc_level
                                aria-label="Increase level"
                                disabled=Signal::derive(move || level.get() >= max_level || saving.get())
                            >
                                "+"
                            </button>
                        </div>
                        <div class="edit-stepper-meta">{format!("max {max_level}")}</div>
                    </div>

                    <div class="edit-stepper-row">
                        <div class="edit-stepper-label">"GOLD"</div>
                        <div class="edit-stepper-controls edit-stepper-gold">
                            <button
                                class="edit-stepper-btn edit-stepper-btn-small"
                                on:click=dec_gold_big
                                aria-label="Decrease gold by 1000"
                                disabled=Signal::derive(move || gold.get() == 0 || saving.get())
                            >
                                "\u{226A}"
                            </button>
                            <button
                                class="edit-stepper-btn"
                                on:click=dec_gold_small
                                aria-label="Decrease gold by 100"
                                disabled=Signal::derive(move || gold.get() == 0 || saving.get())
                            >
                                "\u{2212}"
                            </button>
                            <div class="edit-stepper-value">{move || gold.get().to_string()}</div>
                            <button
                                class="edit-stepper-btn"
                                on:click=inc_gold_small
                                aria-label="Increase gold by 100"
                                disabled=Signal::derive(move || gold.get() == u16::MAX || saving.get())
                            >
                                "+"
                            </button>
                            <button
                                class="edit-stepper-btn edit-stepper-btn-small"
                                on:click=inc_gold_big
                                aria-label="Increase gold by 1000"
                                disabled=Signal::derive(move || gold.get() == u16::MAX || saving.get())
                            >
                                "\u{226B}"
                            </button>
                        </div>
                        <div class="edit-stepper-meta">"max 65535"</div>
                    </div>

                    {move || {
                        let msg = error_msg.get();
                        (!msg.is_empty()).then(|| view! {
                            <p class="edit-error">{msg}</p>
                        })
                    }}

                    <div class="edit-actions">
                        <button
                            class="edit-btn-secondary"
                            on:click=on_cancel
                            disabled=move || saving.get()
                        >
                            "CANCEL"
                        </button>
                        <button
                            class="edit-btn-primary"
                            on:click=on_save
                            disabled=move || saving.get()
                        >
                            {move || if saving.get() { "SAVING…" } else { "SAVE" }}
                        </button>
                    </div>
                </FramedPanel>
            </div>
        </section>
    }
}
