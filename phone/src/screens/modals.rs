use leptos::prelude::*;

use crate::api::{post_load, post_quit, post_reset};
use crate::components::{BezelSize, DisplayHeading, FramedPanel, GoldBezel, HeadingSize};
use crate::gloo_timer;
use crate::model::{GameLaunched, SlotState, UnlockedProfile};
use crate::{push_toast, GameCrashReason, ResetTarget, ResumeOffer, TakeoverReason, ToastMsg};

#[component]
pub(crate) fn ResumeModal(
    resume_offer: RwSignal<Option<ResumeOffer>>,
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
    let figures = move || {
        resume_offer.get().map(|offer| {
            offer
                .slots
                .iter()
                .filter_map(|s| {
                    if let SlotState::Loaded { display_name, .. } = s {
                        let initial = display_name
                            .chars()
                            .next()
                            .unwrap_or('?')
                            .to_uppercase()
                            .to_string();
                        Some((initial, display_name.clone()))
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
                        {move || figures().unwrap_or_default().into_iter().map(|(initial, name)| {
                            view! {
                                <div class="resume-fig">
                                    <GoldBezel size=BezelSize::Md>
                                        <span>{initial}</span>
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
                                leptos::task::spawn_local(async move {
                                    for (i, state) in slots.iter().enumerate() {
                                        if let SlotState::Loaded { figure_id: Some(id), .. } = state {
                                            let slot_1_indexed = (i + 1) as u8;
                                            let id = id.clone();
                                            match post_load(slot_1_indexed, &id).await {
                                                Ok(()) => {}
                                                Err(e) if e.contains("429") => {}
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
                            on:click=move |_| resume_offer.set(None)
                        >"START FRESH"</button>
                    </div>
                </FramedPanel>
            </div>
        </section>
    }
}

#[component]
pub(crate) fn TakeoverScreen(takeover: RwSignal<Option<TakeoverReason>>) -> impl IntoView {
    // Kaos took the slot. "Kick back" does a full page reload — the browser
    // opens a fresh WS, server tries to re-admit via the FIFO path. If the
    // 1-minute cooldown is still active, the reload-WS gets an `Error` event
    // and closes. If cooldown elapsed, we land back at the ProfilePicker
    // (server re-locks all profiles on a fresh session so PIN re-entry is
    // required — SPEC Q46).
    view! {
        <section class="takeover-void">
            <div class="takeover-hexgrid"></div>
            <div class="takeover-sparks"></div>

            <div class="takeover-viewport">
                // Kaos sigil placeholder — actual SVG mask wiring deferred
                <div class="kaos-sigil"></div>

                <h1 class="kaos-title">
                    "KAOS"
                    <span class="kaos-title-line2">"REIGNS!"</span>
                </h1>

                <div class="takeover-quote-card">
                    <div class="takeover-quote-open">{"\u{201C}"}</div>
                    <div class="takeover-quote-body">
                        {move || takeover
                            .get()
                            .map(|t| t.by_kaos.clone())
                            .unwrap_or_else(|| "Behold my magnificent wickedness!".into())}
                    </div>
                    <div class="takeover-quote-close">{"\u{201D}"}</div>
                    <div class="takeover-quote-attrib">{"\u{2014} KAOS"}</div>
                </div>

                <p class="takeover-info">
                    "your seat has been claimed \u{00B7} enter your pin to return"
                </p>

                <button
                    class="takeover-kick-btn"
                    on:click=move |_| {
                        if let Some(loc) = web_sys::window().map(|w| w.location()) {
                            let _ = loc.reload();
                        }
                    }
                >
                    "KICK BACK IN"
                </button>
            </div>

            <div class="takeover-vignette"></div>
        </section>
    }
}

/// PLAN 4.15.14 — Phone-side game-crash overlay.
///
/// Full-screen (NOT a toast — this is a session-breaking event). Rendered
/// when the server broadcasts `Event::GameCrashed`; preempts every other
/// screen in the phone's stack except `ConnectionLost` (which has higher
/// z and renders independently from the bottom of `App()`'s view).
/// Auto-dismisses on the next `GameChanged { current: Some(_) }` (the
/// WS handler in `ws.rs` clears `game_crash` when a new game boots).
///
/// MVP copy follows `docs/aesthetic/navigation.md` §3.8:
///   - Heading: "GAME CRASHED" (gold display treatment)
///   - Body: short reassurance + the server-supplied diagnostic
///   - Action: "RETURN TO GAMES" gold button → clear the overlay; the
///     underlying GamePicker renders because the crash watchdog has
///     already broadcast `GameChanged { current: None }`.
///
/// Auto-restart path (spinner + "Restarting...") is deferred — the server
/// doesn't auto-restart RPCS3 yet (PLAN 4.15.10). When it does, this
/// component can branch on a `restarting: bool` flag.
#[component]
pub(crate) fn GameCrashScreen(
    game_crash: RwSignal<Option<GameCrashReason>>,
    current_game: RwSignal<Option<GameLaunched>>,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let message = Signal::derive(move || {
        game_crash
            .get()
            .map(|c| c.message)
            .unwrap_or_else(|| "The emulator stopped unexpectedly.".into())
    });

    // "RETURN TO GAMES" — just dismiss. The crash-watchdog already sent
    // `GameChanged { current: None }` alongside `GameCrashed`, so the
    // GamePicker will render under us as soon as we clear the overlay.
    // Belt-and-braces: also clear `current_game` locally in case a
    // future server change stops sending the implicit game-ended event.
    // Fire a best-effort `/api/quit?force=true` so any half-alive RPCS3
    // process that the watchdog missed gets cleaned up server-side.
    let on_return = move |_| {
        game_crash.set(None);
        current_game.set(None);
        leptos::task::spawn_local(async move {
            if let Err(e) = post_quit(true).await {
                // Not fatal — the server may already have torn down. Log
                // via toast so dev builds see the error, but the user has
                // already moved on to the game picker.
                push_toast(toasts, &format!("Cleanup quit failed: {e}"));
            }
        });
    };

    view! {
        <section class="crash-overlay">
            <div class="crash-backdrop"></div>
            <div class="crash-sparks"></div>

            <div class="crash-viewport">
                <div class="crash-warning-mark">{"\u{26A0}"}</div>

                <DisplayHeading size=HeadingSize::Lg>
                    "GAME CRASHED"
                </DisplayHeading>

                <p class="crash-body">
                    "The emulator stopped unexpectedly. \
                     Hang tight \u{2014} we\u{2019}ll get you back to your adventure."
                </p>

                <p class="crash-detail">{move || message.get()}</p>

                <button
                    class="crash-return-btn"
                    on:click=on_return
                >
                    "RETURN TO GAMES"
                </button>
            </div>
        </section>
    }
}

/// Header kebab → single surface that consolidates: current-profile chip,
/// join-QR (shell only — real QR content wiring is a post-Phase-4 follow-up),
/// and three action buttons. PLAN 4.12.4b. Mock: `docs/aesthetic/mocks/menu_overlay.html`.
///
/// Actions:
///   - SWITCH PROFILE  — single-tap, local re-lock. Clears `unlocked_profile`;
///     the ProfilePicker re-renders. Server-side re-lock endpoint is a
///     follow-up — for now the phone's view falls back to the picker and
///     the next unlock goes through the normal PIN flow.
///   - CHOOSE ANOTHER GAME — hold-to-confirm, server-impactful. Calls
///     `post_quit(false)` → server quits RPCS3 → WS broadcasts GameStopped
///     → every phone sees the game picker.
///   - SHUT DOWN — hold-to-confirm, danger styling. POSTs `/api/shutdown`
///     which flips the TV launcher into the Farewell surface; egui runs
///     its own farewell countdown and closes the viewport. PLAN 4.15.11.
#[component]
pub(crate) fn MenuOverlay(
    open: RwSignal<bool>,
    unlocked_profile: RwSignal<Option<UnlockedProfile>>,
    current_game: RwSignal<Option<GameLaunched>>,
    manage_gate: RwSignal<bool>,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let close = move || open.set(false);

    // Hold-to-confirm state — one pair of signals per hold button. Using
    // two signals instead of a single enum so the CSS class string stays
    // an Fn closure that reads them independently. `spawn_local` task
    // awaits 1200ms then checks `holding` — if still held, fires.
    let game_holding = RwSignal::new(false);
    let game_fired = RwSignal::new(false);
    let shutdown_holding = RwSignal::new(false);
    let shutdown_fired = RwSignal::new(false);

    let initial = Signal::derive(move || {
        unlocked_profile
            .get()
            .and_then(|p| p.display_name.chars().next())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
    });
    let name = Signal::derive(move || {
        unlocked_profile
            .get()
            .map(|p| p.display_name)
            .unwrap_or_default()
    });
    let game_name = Signal::derive(move || {
        current_game
            .get()
            .map(|g| g.display_name)
            .unwrap_or_default()
    });

    // SWITCH PROFILE — single tap. Local re-lock for now.
    let on_switch = move |_| {
        unlocked_profile.set(None);
        close();
    };

    // MANAGE PROFILES — enters the Konami gate (admin profile management).
    // Locks the current session so ProfilePicker mounts, and raises the
    // shared `manage_gate` signal that ProfilePicker reads to open the gate.
    let on_manage = move |_| {
        unlocked_profile.set(None);
        manage_gate.set(true);
        close();
    };

    // CHOOSE ANOTHER GAME — hold-to-confirm.
    let on_game_down = move |_| {
        if game_fired.get_untracked() {
            return;
        }
        game_holding.set(true);
        leptos::task::spawn_local(async move {
            gloo_timer(1200).await;
            if !game_holding.get_untracked() || game_fired.get_untracked() {
                return;
            }
            game_holding.set(false);
            game_fired.set(true);
            leptos::task::spawn_local(async move {
                if let Err(e) = post_quit(false).await {
                    push_toast(toasts, &format!("Quit failed: {e}"));
                }
            });
            // Give the fire animation time to play, then close the menu.
            gloo_timer(380).await;
            open.set(false);
            gloo_timer(600).await;
            game_fired.set(false);
        });
    };
    let on_game_cancel = move |_| game_holding.set(false);

    // SHUT DOWN — hold-to-confirm. POSTs `/api/shutdown` which flips
    // the TV launcher into the Farewell surface; the egui side runs its
    // own ~3s countdown and then closes the viewport. We fire-and-forget
    // because the launcher's death is the user-visible signal — no need
    // to wait for the response on the phone.
    let on_shutdown_down = move |_| {
        if shutdown_fired.get_untracked() {
            return;
        }
        shutdown_holding.set(true);
        leptos::task::spawn_local(async move {
            gloo_timer(1200).await;
            if !shutdown_holding.get_untracked() || shutdown_fired.get_untracked() {
                return;
            }
            shutdown_holding.set(false);
            shutdown_fired.set(true);
            // Best-effort POST. On failure we toast so the user knows
            // their tap didn't take effect; success is silent because
            // the launcher's farewell + viewport-close speaks for itself.
            if let Err(e) = crate::api::post_shutdown().await {
                push_toast(toasts, &format!("Shutdown failed: {e}"));
            }
            gloo_timer(380).await;
            open.set(false);
            gloo_timer(600).await;
            shutdown_fired.set(false);
        });
    };
    let on_shutdown_cancel = move |_| shutdown_holding.set(false);

    let game_class = move || {
        let mut cls = String::from("menu-action");
        cls.push_str(" menu-action--hold");
        if game_holding.get() {
            cls.push_str(" holding");
        }
        if game_fired.get() {
            cls.push_str(" fired");
        }
        cls
    };
    let shutdown_class = move || {
        let mut cls = String::from("menu-action menu-action--hold menu-action--danger");
        if shutdown_holding.get() {
            cls.push_str(" holding");
        }
        if shutdown_fired.get() {
            cls.push_str(" fired");
        }
        cls
    };

    view! {
        <Show when=move || open.get() fallback=|| ()>
            <div class="menu-scrim" on:click=move |_| close()></div>
            <div class="menu-overlay-panel">
                <button class="menu-close" on:click=move |_| close()>"\u{2715}"</button>

                <Show when=move || unlocked_profile.get().is_some() fallback=|| ()>
                    <div class="menu-current-chip">
                        <div class="menu-current-swatch">{move || initial.get()}</div>
                        <div class="menu-current-meta">
                            <div class="menu-current-name">{move || name.get()}</div>
                            <Show when=move || current_game.get().is_some() fallback=|| ()>
                                <div class="menu-current-game">{move || game_name.get()}</div>
                            </Show>
                        </div>
                    </div>
                </Show>

                <div class="menu-join-card">
                    <div class="menu-join-label">"\u{2316} INVITE A PLAYER"</div>
                    <div class="menu-qr-frame">
                        <div class="menu-qr-inner">"QR"</div>
                    </div>
                    <div class="menu-join-hint">"scan to join this portal"</div>
                </div>

                <div class="menu-actions">
                    <Show when=move || unlocked_profile.get().is_some() fallback=|| ()>
                        <button class="menu-action" on:click=on_switch>
                            <div class="menu-action-icon">"\u{21C4}"</div>
                            <div>
                                <div class="menu-action-title">"SWITCH PROFILE"</div>
                                <div class="menu-action-desc">"Sign back in as someone else"</div>
                            </div>
                        </button>
                    </Show>

                    <button class="menu-action" on:click=on_manage>
                        <div class="menu-action-icon">"\u{2699}"</div>
                        <div>
                            <div class="menu-action-title">"MANAGE PROFILES"</div>
                            <div class="menu-action-desc">"Grown-ups only \u{00B7} reset PINs, add or remove profiles"</div>
                        </div>
                    </button>

                    <Show when=move || current_game.get().is_some() fallback=|| ()>
                        <button
                            class=game_class
                            on:pointerdown=on_game_down
                            on:pointerup=on_game_cancel
                            on:pointerleave=on_game_cancel
                            on:pointercancel=on_game_cancel
                        >
                            <span class="hold-fill"></span>
                            <div class="menu-action-icon">"\u{25C9}"</div>
                            <div>
                                <div class="menu-action-title">"HOLD TO SWITCH GAMES"</div>
                                <div class="menu-action-desc">
                                    "Quit the current game and pick a different adventure"
                                </div>
                            </div>
                        </button>
                    </Show>

                    <button
                        class=shutdown_class
                        on:pointerdown=on_shutdown_down
                        on:pointerup=on_shutdown_cancel
                        on:pointerleave=on_shutdown_cancel
                        on:pointercancel=on_shutdown_cancel
                    >
                        <span class="hold-fill"></span>
                        <div class="menu-action-icon">"\u{23FB}"</div>
                        <div>
                            <div class="menu-action-title">"HOLD TO SHUT DOWN"</div>
                            <div class="menu-action-desc">
                                "Closes everything \u{00B7} ask a grown-up first"
                            </div>
                        </div>
                    </button>
                </div>
            </div>
        </Show>
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

            // Fire-and-forget the reset; the WS will broadcast SlotChanged
            // once the working copy is rebuilt. Animation runs in parallel.
            let target = match reset_target.get_untracked() {
                Some(t) => t,
                None => return,
            };
            leptos::task::spawn_local(async move {
                if let Err(e) = post_reset(target.slot, &target.figure_id).await {
                    push_toast(toasts, &format!("Reset failed: {e}"));
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
