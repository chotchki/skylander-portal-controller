use leptos::prelude::*;

use crate::api::{clear_resume, post_load, post_reset, post_reset_figure};
use crate::components::{BezelSize, DisplayHeading, FramedPanel, GoldBezel, HeadingSize};
use crate::gloo_timer;
use crate::model::{
    FigureAction, PublicProfile, ServerApp, Slot, SlotState, UnlockedProfile, SLOT_COUNT,
};
use crate::{push_toast, push_toast_level, ResetTarget, ResumeOffer, ToastLevel, ToastMsg};

/// What the phone says when guacamole refuses one of RESUME's loads (a 409:
/// another player's figure holds that slot, openspec AB.22), in words a kid
/// can read (AB.23): the figure that stayed off, and whose figure is in its
/// spot as this phone's portal shows it, or "another player's" when the phone
/// can't tell. The product never answers 409 on a load.
pub(crate) fn resume_refused_message(
    figure: &str,
    held: &SlotState,
    resumer: Option<&str>,
    profiles: &[PublicProfile],
) -> String {
    let holder = match held {
        SlotState::Loaded {
            display_name,
            placed_by: Some(owner),
            ..
        } if Some(owner.as_str()) != resumer => match profiles.iter().find(|p| &p.id == owner) {
            Some(p) => format!("{}'s {display_name}", p.display_name),
            None => format!("another player's {display_name}"),
        },
        _ => "another player's figure".to_string(),
    };
    format!("{figure} stayed off the portal: {holder} is in that spot.")
}

#[component]
pub(crate) fn ResumeModal(
    resume_offer: RwSignal<Option<ResumeOffer>>,
    /// The live portal and the profiles behind its owner pips: a refused
    /// RESUME load names whose figure holds the slot from these (AB.23).
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    known_profiles: RwSignal<Vec<PublicProfile>>,
    /// The currently unlocked profile — its `id` is what
    /// `/api/profiles/:id/clear_resume` keys on. Lifted from the lib
    /// signal so the Start Fresh handler can drop the saved layout
    /// server-side; without that, the modal only dismissed locally
    /// and re-fired on every subsequent unlock.
    unlocked_profile: RwSignal<Option<UnlockedProfile>>,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    // Overlay modal offering to reload the profile's last portal layout.
    // "Resume" issues one `/api/portal/slot/:n/load` per non-empty slot;
    // "Start fresh" just dismisses. Either way we clear `resume_offer` —
    // the next unlock will re-fire if there's a (possibly different) layout.
    //
    // 2-phone nuance (SPEC Round 4): if the portal *isn't* empty (the other
    // phone already has figures down), a blanket resume would double-load
    // or collide. For now we label the button accordingly but always go
    // through the same per-slot calls; the server's slot-busy back-pressure
    // handles collisions. Proper 3-option modal (`clear + resume` vs
    // `alongside current` vs `fresh`) lands with PLAN 3.10.9 follow-up.

    // Build a static list of figure preview data from the offer slots.
    // `figure_id` is carried so the bezel can render the real portrait
    // (server `/api/figures/{id}/image?size=thumb`) on top of the initial
    // fallback — same pattern as `.p4-slot-image` + `.fig-image-p4`.
    let figures = move || {
        resume_offer.get().map(|offer| {
            offer
                .slots
                .iter()
                .filter_map(|s| {
                    if let SlotState::Loaded {
                        display_name,
                        figure_id,
                        ..
                    } = s
                    {
                        let initial = display_name
                            .chars()
                            .next()
                            .unwrap_or('?')
                            .to_uppercase()
                            .to_string();
                        Some((initial, display_name.clone(), figure_id.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
    };

    view! {
        <section class="resume-scrim">
            <div class="resume-panel-wrap">
                <FramedPanel class="panel-in resume-panel">
                    <DisplayHeading size=HeadingSize::Md>
                        "WELCOME BACK"
                    </DisplayHeading>
                    <p class="resume-sub">"pick up where you left off?"</p>
                    <p class="resume-context">{"\u{2014} YOUR LAST ADVENTURE \u{2014}"}</p>

                    <div class="resume-fig-row">
                        {move || figures().unwrap_or_default().into_iter().map(|(initial, name, figure_id)| {
                            let img_view = figure_id.map(|id| {
                                view! {
                                    <img
                                        class="resume-fig-image"
                                        src=format!("/api/figures/{id}/image?size=thumb")
                                        alt=""
                                        loading="eager"
                                        decoding="async"
                                    />
                                }
                            });
                            view! {
                                <div class="resume-fig">
                                    <GoldBezel size=BezelSize::Md>
                                        <span class="resume-fig-initial">{initial}</span>
                                        {img_view}
                                    </GoldBezel>
                                    <span class="resume-fig-name">{name}</span>
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                    </div>

                    <p class="resume-when">"saved layout"</p>

                    <div class="resume-actions">
                        <button
                            class="resume-btn resume-btn-primary"
                            on:click=move |_| {
                                let offer = match resume_offer.get() {
                                    Some(o) => o,
                                    None => return,
                                };
                                resume_offer.set(None);
                                let slots = offer.slots.clone();
                                let resumer = unlocked_profile.get_untracked().map(|p| p.id);
                                leptos::task::spawn_local(async move {
                                    for (i, state) in slots.iter().enumerate() {
                                        if let SlotState::Loaded { figure_id: Some(id), display_name, .. } = state {
                                            let slot_1_indexed = (i + 1) as u8;
                                            let id = id.clone();
                                            match post_load(slot_1_indexed, &id).await {
                                                Ok(()) => {}
                                                Err(e) if e.contains("429") => {}
                                                // guacamole only: another player's figure
                                                // holds the slot (AB.22), so say whose.
                                                Err(e) if e == "HTTP 409" => {
                                                    let held = portal.with_untracked(|p| p[i].state.clone());
                                                    let message = known_profiles.with_untracked(|ps| {
                                                        resume_refused_message(display_name, &held, resumer.as_deref(), ps)
                                                    });
                                                    push_toast_level(toasts, &message, ToastLevel::Warn);
                                                }
                                                Err(e) => {
                                                    push_toast(
                                                        toasts,
                                                        &format!("Resume slot {slot_1_indexed}: {e}"),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                });
                            }
                        >"RESUME"</button>
                        <button
                            class="resume-btn resume-btn-secondary"
                            on:click=move |_| {
                                resume_offer.set(None);
                                // Drop the saved layout on the server
                                // so the next unlock won't re-offer
                                // the same resume. Best-effort —
                                // failure here just means the prompt
                                // re-appears on the next unlock,
                                // which is recoverable.
                                if let Some(profile) = unlocked_profile.get_untracked() {
                                    leptos::task::spawn_local(async move {
                                        if let Err(e) = clear_resume(&profile.id).await {
                                            crate::dev_warn!(
                                                "clear_resume({}): {e}",
                                                profile.id,
                                            );
                                        }
                                    });
                                }
                            }
                        >"START FRESH"</button>
                    </div>
                </FramedPanel>
            </div>
        </section>
    }
}

/// PLAN 4.12.2 — "Reset this figure to fresh?" confirm.
///
/// Triggered when the user taps RESET on a loaded portal slot. Replaces the
/// browser's `window.confirm()` placeholder with a Skylanders-themed danger
/// modal: red-bezeled `<FramedPanel>`, hold-to-confirm primary, gold-flake
/// fall + bezel desaturation on fire (per PLAN 4.2.14.a). Mock:
/// `docs/aesthetic/mocks/reset_confirm.html`.
///
/// Server call (`post_reset`) fires immediately on hold-fire; the ~1500ms
/// animation budget masks the file-IO latency on the server side. The phone
/// gets a fresh `SlotChanged(Loaded)` over WS once the working copy has
/// been replaced — no extra UI wiring needed here.
#[component]
pub(crate) fn ResetConfirmModal(
    reset_target: RwSignal<Option<ResetTarget>>,
    toasts: RwSignal<Vec<ToastMsg>>,
    /// guacamole's refusals read in a kid's words (AH.3).
    server_app: RwSignal<ServerApp>,
) -> impl IntoView {
    let holding = RwSignal::new(false);
    let fired = RwSignal::new(false);

    // Reset transient signals every time a fresh target arrives so a second
    // RESET tap doesn't open the modal already in `.fired` state.
    Effect::new(move |_| {
        if reset_target.get().is_some() {
            holding.set(false);
            fired.set(false);
        }
    });

    let initial = Signal::derive(move || {
        reset_target
            .get()
            .and_then(|t| t.display_name.chars().next())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
    });
    let display_name = Signal::derive(move || {
        reset_target
            .get()
            .map(|t| t.display_name)
            .unwrap_or_default()
    });

    let on_cancel = move |_| {
        if fired.get_untracked() {
            return;
        }
        reset_target.set(None);
    };

    let on_hold_down = move |_| {
        if fired.get_untracked() || reset_target.get_untracked().is_none() {
            return;
        }
        holding.set(true);
        leptos::task::spawn_local(async move {
            gloo_timer(1200).await;
            if !holding.get_untracked() || fired.get_untracked() {
                return;
            }
            holding.set(false);
            fired.set(true);

            // Fire-and-forget the reset. For slot-context resets (figure on
            // portal), the WS broadcasts SlotChanged once the working copy
            // is rebuilt. For figure-detail resets, the figure isn't on a
            // slot so we hit the figure-keyed endpoint (PLAN 11.12);
            // FigureUpdated drives the stats-strip refresh. Animation runs
            // in parallel in either case.
            let target = match reset_target.get_untracked() {
                Some(t) => t,
                None => return,
            };
            leptos::task::spawn_local(async move {
                let (outcome, action) = match target.slot {
                    Some(slot) => (
                        post_reset(slot, &target.figure_id).await,
                        FigureAction::SlotReset,
                    ),
                    None => (
                        post_reset_figure(&target.profile_id, &target.figure_id).await,
                        FigureAction::Reset,
                    ),
                };
                if let Err(e) = outcome {
                    let app = server_app.get_untracked();
                    match app.refusal(action, &e, &target.display_name) {
                        Some(words) => push_toast_level(toasts, &words, ToastLevel::Warn),
                        None => push_toast(toasts, &format!("Reset failed: {e}")),
                    }
                }
            });

            // 380ms after .fired (white flash crests), the bezel/title drain
            // begins via CSS class. Total drain is 1100ms; dismiss after
            // 1100 + 400 buffer so the user reads the desaturation.
            gloo_timer(1500).await;
            reset_target.set(None);
            // Reset transient signals after the modal is gone.
            gloo_timer(120).await;
            fired.set(false);
        });
    };
    let on_hold_cancel = move |_| holding.set(false);

    let panel_wrap_class = move || {
        let mut cls = String::from("reset-panel-wrap");
        if fired.get() {
            cls.push_str(" dismissing");
        }
        cls
    };
    let bezel_class = move || {
        let mut cls = String::from("reset-hero-bezel");
        if fired.get() {
            cls.push_str(" resetting");
        }
        cls
    };
    let plate_class = move || {
        let mut cls = String::from("reset-hero-plate");
        if fired.get() {
            cls.push_str(" resetting");
        }
        cls
    };
    let title_class = move || {
        let mut cls = String::from("reset-fig-title");
        if fired.get() {
            cls.push_str(" resetting");
        }
        cls
    };
    let scrim_class = move || {
        let mut cls = String::from("reset-scrim");
        if fired.get() {
            cls.push_str(" dismissing");
        }
        cls
    };
    let hold_btn_class = move || {
        let mut cls = String::from("reset-btn reset-btn-danger");
        if holding.get() {
            cls.push_str(" holding");
        }
        if fired.get() {
            cls.push_str(" fired");
        }
        cls
    };

    view! {
        <Show when=move || reset_target.get().is_some() fallback=|| ()>
            <div class=scrim_class on:click=on_cancel></div>
            <div class=panel_wrap_class>
                <FramedPanel class="reset-panel">
                    <DisplayHeading size=HeadingSize::Md>
                        "RESET FIGURE?"
                    </DisplayHeading>
                    <div class="reset-warning-mark">
                        {"\u{25B3} THIS CAN'T BE UNDONE \u{25B3}"}
                    </div>

                    <div class="reset-target">
                        <div class=bezel_class>
                            <GoldBezel size=BezelSize::Md>
                                <div class=plate_class>
                                    <span>{move || initial.get()}</span>
                                </div>
                            </GoldBezel>
                        </div>
                        <div class=title_class>{move || display_name.get()}</div>
                    </div>

                    <div class="reset-warning-card">
                        <div class="reset-body-copy">
                            "All of "
                            <strong>{move || format!("{}'s", display_name.get())}</strong>
                            " treasure \u{2014} level, gold, upgrades \u{2014} will be gone forever."
                        </div>
                        <div class="reset-grown-up">"ask a grown-up first"</div>
                    </div>

                    <div class="reset-actions">
                        <button class="reset-btn reset-btn-tertiary" on:click=on_cancel>
                            {move || format!("KEEP {}", display_name.get().to_uppercase())}
                        </button>
                        <button
                            class=hold_btn_class
                            on:pointerdown=on_hold_down
                            on:pointerup=on_hold_cancel
                            on:pointerleave=on_hold_cancel
                            on:pointercancel=on_hold_cancel
                        >
                            <span class="hold-fill"></span>
                            <span class="reset-btn-label">"HOLD TO RESET"</span>
                        </button>
                    </div>

                    // Gold-flake fall (PLAN 4.2.14.a). Rendered only while
                    // `.fired` is true; CSS `flake-fall` runs once and the
                    // modal dismisses before the next frame is needed.
                    <Show when=move || fired.get() fallback=|| ()>
                        <ResetFlakes />
                    </Show>
                </FramedPanel>
            </div>
        </Show>
    }
}

/// Spawns the 14 gold flakes around the bezel rim. Each flake's drift +
/// rotation are computed once at mount via `js_sys::Math::random()` and
/// baked into inline CSS variables (`--fx`, `--fy`, `--fr`); the
/// `flake-fall` keyframe consumes them. Position-on-rim is encoded as a
/// rotated parent + radial child so we don't need DOM-rect math.
#[component]
fn ResetFlakes() -> impl IntoView {
    let flakes: Vec<String> = (0..14)
        .map(|_| {
            let angle = js_sys::Math::random() * std::f64::consts::TAU;
            let drift_x = js_sys::Math::random() * 90.0 - 45.0;
            let drift_y = 70.0 + js_sys::Math::random() * 90.0;
            let rot = js_sys::Math::random() * 360.0 - 180.0;
            let dur = 700.0 + js_sys::Math::random() * 400.0;
            let delay = js_sys::Math::random() * 220.0;
            format!(
                "--fx:{drift_x:.1}px;--fy:{drift_y:.1}px;--fr:{rot:.1}deg;\
                 --rim-angle:{angle:.4}rad;\
                 animation: flake-fall {dur:.0}ms {delay:.0}ms ease-in forwards;"
            )
        })
        .collect();

    view! {
        <div class="reset-flake-layer">
            {flakes.into_iter().map(|style| view! {
                <div class="reset-flake-orbit" style=style>
                    <div class="reset-flake"></div>
                </div>
            }.into_any()).collect_view()}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, name: &str) -> PublicProfile {
        PublicProfile {
            id: id.to_string(),
            display_name: name.to_string(),
            color: "#e8423f".to_string(),
            kaos_enabled: false,
        }
    }

    fn loaded(name: &str, placed_by: Option<&str>) -> SlotState {
        SlotState::Loaded {
            figure_id: Some(format!("{}-id", name.to_lowercase())),
            display_name: name.to_string(),
            placed_by: placed_by.map(str::to_string),
        }
    }

    /// AB.23: the toast names the figure that stayed off and whose figure
    /// holds its spot, when the phone's portal and profiles say so.
    #[test]
    fn a_refused_resume_names_the_figure_and_who_holds_its_spot() {
        let profiles = [profile("alice", "Alice"), profile("bob", "Bob")];
        assert_eq!(
            resume_refused_message(
                "Bash",
                &loaded("Gill Grunt", Some("alice")),
                Some("bob"),
                &profiles
            ),
            "Bash stayed off the portal: Alice's Gill Grunt is in that spot."
        );
        // A holder this phone has no profile for yet.
        assert_eq!(
            resume_refused_message(
                "Bash",
                &loaded("Gill Grunt", Some("carol")),
                Some("bob"),
                &profiles
            ),
            "Bash stayed off the portal: another player's Gill Grunt is in that spot."
        );
    }

    /// A portal view that hasn't caught up (the slot empty, loading, the
    /// resumer's own or nobody's) still says which figure stayed off and why.
    #[test]
    fn a_stale_portal_view_still_says_why() {
        let profiles = [profile("alice", "Alice")];
        let generic = "Spyro stayed off the portal: another player's figure is in that spot.";
        for held in [
            SlotState::Empty,
            SlotState::Loading {
                figure_id: None,
                placed_by: Some("alice".to_string()),
            },
            loaded("Eruptor", Some("bob")),
            loaded("Eruptor", None),
        ] {
            assert_eq!(
                resume_refused_message("Spyro", &held, Some("bob"), &profiles),
                generic,
                "{held:?}"
            );
        }
    }
}
