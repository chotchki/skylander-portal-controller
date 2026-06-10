use leptos::prelude::*;

use crate::api::{create_profile, fetch_profiles, reset_pin, unlock_profile};
use crate::components::{
    BezelSize, BezelState, CreditsOverlay, DisplayHeading, FramedPanel, GoldBezel, HeadingSize,
    PwaHint,
};
use crate::model::{PublicProfile, WindowMode};
use crate::{event_target_value, push_toast, push_toast_level, ToastLevel, ToastMsg};

// --------- Constants ---------

const KONAMI: [&str; 10] = [
    "up", "up", "down", "down", "left", "right", "left", "right", "b", "a",
];

/// Available profile colours (element-inspired).
const COLOR_SWATCHES: [(&str, &str); 8] = [
    ("magic", "#da5ad6"),
    ("fire", "#ff6b2a"),
    ("water", "#2aa6ff"),
    ("life", "#5ac96b"),
    ("tech", "#ffb84d"),
    ("undead", "#9a5aaa"),
    ("earth", "#a77b3a"),
    ("air", "#c6e6ff"),
];

/// Prefilled names for the "Create profile" flow (PLAN 4.18.7). Kid can
/// keep the default, tap the ↻ button to reroll, or type their own. No
/// whitelist at submit time per PLAN 4.2.8 ("if a kid names themselves
/// poop that's okay"). List mirrors the 20 names in
/// `docs/aesthetic/mocks/profile_create.html`.
const SKYLANDER_NAMES: [&str; 20] = [
    "Spyro",
    "Eruptor",
    "Stealth Elf",
    "Trigger Happy",
    "Gill Grunt",
    "Pop Fizz",
    "Chop Chop",
    "Cynder",
    "Wrecking Ball",
    "Hex",
    "Drobot",
    "Boomer",
    "Whirlwind",
    "Flashwing",
    "Jet-Vac",
    "Terrafin",
    "Bash",
    "Dino-Rang",
    "Zook",
    "Shroomboom",
];

/// Return a random Skylander name from `SKYLANDER_NAMES`. Used to seed
/// the initial Name field in CreateProfileForm and to power the reroll
/// button. `js_sys::Math::random()` is the pragmatic RNG on wasm — no
/// need to drag in a crypto-grade source for a UI prefill.
fn random_skylander_name() -> &'static str {
    let idx = (js_sys::Math::random() * SKYLANDER_NAMES.len() as f64) as usize;
    SKYLANDER_NAMES[idx.min(SKYLANDER_NAMES.len() - 1)]
}

// --------- Profile picker / admin UI ---------

#[component]
pub(crate) fn ProfilePicker(
    toasts: RwSignal<Vec<ToastMsg>>,
    profiles_epoch: RwSignal<u32>,
    manage_gate: RwSignal<bool>,
) -> impl IntoView {
    let profiles = RwSignal::new(Vec::<PublicProfile>::new());
    let manage_mode = RwSignal::new(false);
    let picked = RwSignal::new(None::<PublicProfile>); // profile whose PIN we're entering
    let show_admin = RwSignal::new(false); // true = past konami gate

    // Fetch whenever epoch bumps.
    Effect::new(move |_| {
        let _ = profiles_epoch.get();
        leptos::task::spawn_local(async move {
            profiles.set(fetch_profiles().await);
        });
    });

    view! {
        <section class="profile-picker">
            <PwaHint />
            {move || {
                if show_admin.get() {
                    view! {
                        <ProfileAdminHub
                            profiles=profiles
                            profiles_epoch=profiles_epoch
                            on_lock=move || show_admin.set(false)
                            toasts
                        />
                    }.into_any()
                } else if let Some(p) = picked.get() {
                    view! {
                        <PinEntry
                            profile=p
                            on_cancel=move || picked.set(None)
                            toasts
                        />
                    }.into_any()
                } else {
                    view! {
                        <ProfileGrid
                            profiles
                            picked
                            show_admin
                            toasts
                            _manage_mode=manage_mode
                            profiles_epoch
                            manage_gate
                        />
                    }.into_any()
                }
            }}
        </section>
    }
}

// --------- Profile grid (main picker) ---------

#[component]
fn ProfileGrid(
    profiles: RwSignal<Vec<PublicProfile>>,
    picked: RwSignal<Option<PublicProfile>>,
    show_admin: RwSignal<bool>,
    toasts: RwSignal<Vec<ToastMsg>>,
    _manage_mode: RwSignal<bool>,
    profiles_epoch: RwSignal<u32>,
    manage_gate: RwSignal<bool>,
) -> impl IntoView {
    let show_create = RwSignal::new(false);
    let show_credits = RwSignal::new(false);
    let default_state: Signal<BezelState> = Signal::derive(|| BezelState::Default);
    let disabled_state: Signal<BezelState> = Signal::derive(|| BezelState::Disabled);

    view! {
        <Show when=move || manage_gate.get() fallback=move || {
            // Sub-branch: create-form is mutually exclusive with the
            // welcome+grid view. Chris flagged 2026-04-21 that the
            // form was stacking on top of the grid — fixed by wrapping
            // both surfaces in a <Show> rather than rendering the form
            // beside the grid.
            view! {
                <Show
                    when=move || show_create.get()
                    fallback=move || view! {
                        <div class="pp-welcome-wrap">
                            <DisplayHeading size=HeadingSize::Lg with_rays=true>
                                "PORTAL "
                                <span class="pp-welcome-line2">"MASTER"</span>
                            </DisplayHeading>
                            <div class="pp-welcome-sub">"welcome"</div>
                        </div>

                        <div class="profile-grid">
                            {move || {
                                let list = profiles.get();
                                let can_add = list.len() < 4;
                                view! {
                                    <>
                                    {list.into_iter().map(|p| {
                                        let p_for_click = p.clone();
                                        let color = p.color.clone();
                                        let initial = p.display_name.chars().next().unwrap_or('?').to_string();
                                        view! {
                                            <button
                                                class="profile-card"
                                                on:click=move |_| {
                                                    picked.set(Some(p_for_click.clone()));
                                                }
                                            >
                                                <div style=format!("--profile-color:{color}")>
                                                    <GoldBezel size=BezelSize::Lg state=default_state>
                                                        <span class="pp-initial">{initial}</span>
                                                    </GoldBezel>
                                                </div>
                                                <div class="profile-name">{p.display_name.clone()}</div>
                                            </button>
                                        }
                                    }).collect_view()}
                                    {if can_add {
                                        Some(view! {
                                            <button
                                                class="profile-card add"
                                                on:click=move |_| show_create.set(true)
                                            >
                                                <GoldBezel size=BezelSize::Lg state=disabled_state>
                                                    <span class="pp-initial pp-add-glyph">"+"</span>
                                                </GoldBezel>
                                                <div class="profile-name pp-add-name">"ADD"</div>
                                            </button>
                                        })
                                    } else {
                                        None
                                    }}
                                    </>
                                }
                            }}
                        </div>
                        <button
                            class="pp-tagline"
                            type="button"
                            on:click=move |_| show_credits.set(true)
                        >"CREDITS"</button>
                        <CreditsOverlay open=show_credits />
                    }
                >
                    <CreateProfileForm
                        on_done=move || { show_create.set(false); profiles_epoch.update(|v| *v += 1); }
                        toasts
                    />
                </Show>
                {let _ = toasts; view! { <></> }}
            }
        }>
            <KonamiGate
                on_success=move || { manage_gate.set(false); show_admin.set(true); }
                on_back=move || manage_gate.set(false)
            />
        </Show>
    }
}

// --------- Konami Gate ---------

#[component]
fn KonamiGate<S: Fn() + Send + Sync + 'static + Clone, B: Fn() + Send + Sync + 'static + Clone>(
    on_success: S,
    on_back: B,
) -> impl IntoView {
    let sequence = RwSignal::new(Vec::<String>::new());
    let error_anim = RwSignal::new(false);
    let success_flash = RwSignal::new(false);
    // Pressed-key flash state — set on pointerdown, cleared after
    // ~150ms. iOS Safari's CSS `:active` fires unreliably on fast
    // taps; a signal-driven `.pressed` class lets us guarantee the
    // press visual lands even if the user releases in <1 frame.
    let pressed_key = RwSignal::new(None::<String>);

    let press_key = move |key: &str| {
        if sequence.with(|s| s.len()) >= 10 {
            return;
        }
        sequence.update(|s| s.push(key.to_string()));
        let k = key.to_string();
        pressed_key.set(Some(k.clone()));
        leptos::task::spawn_local(async move {
            crate::gloo_timer(150).await;
            pressed_key.update(|cur| {
                if cur.as_deref() == Some(k.as_str()) {
                    *cur = None;
                }
            });
        });
    };

    let on_clear = move |_| {
        sequence.set(Vec::new());
    };

    let on_success_inner = on_success.clone();
    let on_submit = move |_| {
        let seq = sequence.get();
        if seq.len() != 10 {
            return;
        }
        let correct = seq.iter().zip(KONAMI.iter()).all(|(a, b)| a.as_str() == *b);
        if correct {
            success_flash.set(true);
            let on_success = on_success_inner.clone();
            leptos::task::spawn_local(async move {
                crate::gloo_timer(800).await;
                on_success();
            });
        } else {
            error_anim.set(true);
            sequence.set(Vec::new());
            leptos::task::spawn_local(async move {
                crate::gloo_timer(600).await;
                error_anim.set(false);
            });
        }
    };

    // Helper to make dpad/ab button click handlers.
    let make_key_handler = move |key: &'static str| move |_| press_key(key);

    view! {
        <div class="konami-gate">
            <div class=move || {
                if success_flash.get() { "konami-unlock-flash active" } else { "konami-unlock-flash" }
            }></div>

            <button class="btn-back" on:click=move |_| on_back()>"BACK"</button>

            <div class="konami-header">
                <div class="title-sub">"grown-ups only"</div>
                <DisplayHeading size=HeadingSize::Md>
                    "ENTER"
                    <br/>
                    "THE CODE"
                </DisplayHeading>
            </div>

            <div class=move || {
                if error_anim.get() { "gate-progress error" } else { "gate-progress" }
            }>
                {move || {
                    let len = sequence.with(|s| s.len());
                    let is_error = error_anim.get();
                    (0..10).map(|i| {
                        let filled = i < len;
                        let cls = if is_error && filled {
                            "gate-dot was-filled"
                        } else if filled {
                            "gate-dot filled"
                        } else {
                            "gate-dot"
                        };
                        view! { <div class=cls></div> }
                    }).collect_view()
                }}
            </div>
            <div class="gate-hint">"Contra was such an easy game"</div>

            <div class="gate-pad">
                <div class="dpad">
                    {["up", "down", "left", "right"].iter().map(|k| {
                        let k = *k;
                        let glyph = match k {
                            "up" => "\u{25B2}",
                            "down" => "\u{25BC}",
                            "left" => "\u{25C0}",
                            "right" => "\u{25B6}",
                            _ => "",
                        };
                        let dir_cls = k;
                        let class_fn = move || {
                            let mut s = format!("dpad-btn {dir_cls}");
                            if pressed_key.get().as_deref() == Some(k) {
                                s.push_str(" pressed");
                            }
                            s
                        };
                        view! {
                            <button class=class_fn on:click=make_key_handler(k)>
                                {glyph}
                            </button>
                        }
                    }).collect_view()}
                </div>
                <div class="ab-wrap">
                    {["b", "a"].iter().map(|k| {
                        let k = *k;
                        let ab_cls = if k == "a" { "ab-a" } else { "ab-b" };
                        let class_fn = move || {
                            let mut s = format!("ab-btn {ab_cls}");
                            if pressed_key.get().as_deref() == Some(k) {
                                s.push_str(" pressed");
                            }
                            s
                        };
                        view! {
                            <button class=class_fn on:click=make_key_handler(k)>
                                {k.to_uppercase()}
                            </button>
                        }
                    }).collect_view()}
                </div>
            </div>

            <div class="gate-actions">
                <button class="btn btn-clear" on:click=on_clear>"CLEAR"</button>
                <button
                    class="btn btn-submit"
                    disabled=move || sequence.with(|s| s.len()) != 10
                    on:click=on_submit
                >"SUBMIT"</button>
            </div>
        </div>
    }
}

// --------- Profile admin hub (list + edit + pin reset) ---------

/// Sub-screen enum for the admin hub.
#[derive(Clone, PartialEq)]
enum AdminScreen {
    List,
    Edit(PublicProfile),
    PinReset(PublicProfile),
}

#[component]
fn ProfileAdminHub<F: Fn() + Send + Sync + 'static + Clone>(
    profiles: RwSignal<Vec<PublicProfile>>,
    profiles_epoch: RwSignal<u32>,
    on_lock: F,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let screen = RwSignal::new(AdminScreen::List);

    view! {
        <div class="admin-hub">
            {move || match screen.get() {
                AdminScreen::List => {
                    let on_lock = on_lock.clone();
                    view! {
                        <AdminList
                            profiles=profiles
                            profiles_epoch=profiles_epoch
                            screen=screen
                            on_lock=move || on_lock()
                            toasts
                        />
                    }.into_any()
                }
                AdminScreen::Edit(p) => {
                    let profile = p.clone();
                    view! {
                        <AdminEdit
                            profile=profile
                            on_back=move || { screen.set(AdminScreen::List); profiles_epoch.update(|v| *v += 1); }
                            toasts
                        />
                    }.into_any()
                }
                AdminScreen::PinReset(p) => {
                    let profile = p.clone();
                    view! {
                        <AdminPinReset
                            profile=profile
                            on_back=move || { screen.set(AdminScreen::List); profiles_epoch.update(|v| *v += 1); }
                            toasts
                        />
                    }.into_any()
                }
            }}
        </div>
    }
}

// --------- Admin list ---------

#[component]
fn AdminList<F: Fn() + Send + Sync + 'static + Clone>(
    profiles: RwSignal<Vec<PublicProfile>>,
    /// Threaded down so the per-row HOLD-TO-DELETE button can bump the
    /// epoch on success and force the parent to re-fetch the profile
    /// list (PLAN 9.7 playtest 2026-05-04).
    profiles_epoch: RwSignal<u32>,
    screen: RwSignal<AdminScreen>,
    on_lock: F,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    // PLAN 20.6 — app-level window-mode toggle (Konami-gated, grown-ups).
    // Fetched once on mount; flipping it rewrites config.json server-side and
    // takes effect on the next launcher restart.
    let window_mode = RwSignal::new(None::<WindowMode>);
    leptos::task::spawn_local(async move {
        if let Some(m) = crate::api::fetch_window_mode().await {
            window_mode.set(Some(m));
        }
    });
    let on_toggle_mode = move |_| {
        let Some(cur) = window_mode.get_untracked() else {
            return;
        };
        let next = match cur {
            WindowMode::Tv => WindowMode::Desktop,
            WindowMode::Desktop => WindowMode::Tv,
        };
        window_mode.set(Some(next)); // optimistic
        leptos::task::spawn_local(async move {
            match crate::api::set_window_mode(next).await {
                Ok(()) => push_toast_level(
                    toasts,
                    "Saved \u{2014} restart the launcher to apply the new window mode.",
                    ToastLevel::Success,
                ),
                Err(e) => {
                    push_toast(toasts, &format!("Window mode change failed: {e}"));
                    window_mode.set(Some(cur)); // roll back
                }
            }
        });
    };

    view! {
        <button class="btn-back" on:click=move |_| on_lock()>"LOCK"</button>

        <div class="admin-header">
            <div class="title-sub">"the grown-up side"</div>
            <DisplayHeading size=HeadingSize::Md>
                "PROFILE MANAGEMENT"
            </DisplayHeading>
        </div>

        <FramedPanel class="admin-list-panel">
            <div class="manage-list">
                {move || {
                    let list = profiles.get();
                    view! {
                        <>
                        {list.into_iter().map(|p| {
                            let p_edit = p.clone();
                            let p_pin = p.clone();
                            let initial = p.display_name.chars().next().unwrap_or('?').to_uppercase().to_string();
                            let color_attr = color_to_element(&p.color);
                            let name_upper = p.display_name.to_uppercase();
                            let deleting = RwSignal::new(false);
                            // Hold-to-confirm wiring (PLAN 9.7 playtest 2026-05-04 —
                            // restyle to match the destructive `<ActionButton
                            // variant=Danger hold_duration=...>` pattern). Holding
                            // animates `.hold-fill` and after `--dur-hold-confirm`
                            // (1200ms) fires `delete_profile`. Lifting cancels.
                            let holding = RwSignal::new(false);
                            let fired = RwSignal::new(false);
                            let p_id = p.id.clone();
                            let p_name = p.display_name.clone();
                            let on_hold_start = move |_: leptos::ev::PointerEvent| {
                                if fired.get_untracked() { return; }
                                holding.set(true);
                                let id = p_id.clone();
                                let name = p_name.clone();
                                leptos::task::spawn_local(async move {
                                    crate::gloo_timer(1200).await;
                                    if !holding.get_untracked() || fired.get_untracked() { return; }
                                    holding.set(false);
                                    fired.set(true);
                                    match crate::api::delete_profile(&id).await {
                                        Ok(()) => {
                                            push_toast_level(toasts, &format!("Deleted {name}."), ToastLevel::Success);
                                            profiles_epoch.update(|v| *v += 1);
                                        }
                                        Err(e) => {
                                            fired.set(false);
                                            deleting.set(false);
                                            push_toast(toasts, &format!("Delete failed: {e}"));
                                        }
                                    }
                                });
                            };
                            let on_hold_end = move |_: leptos::ev::PointerEvent| {
                                holding.set(false);
                            };
                            let confirm_class = move || {
                                let mut s = String::from("del-confirm menu-action menu-action--hold menu-action--danger");
                                if holding.get() { s.push_str(" holding"); }
                                if fired.get() { s.push_str(" fired"); }
                                s
                            };
                            view! {
                                <div class=move || if deleting.get() { "profile-row deleting" } else { "profile-row" }>
                                    <div class="profile-bezel" data-el=color_attr.clone() data-initial=initial.clone()></div>
                                    <div class="profile-meta">
                                        <div class="profile-name">{name_upper}</div>
                                    </div>
                                    <div class="profile-actions">
                                        <button class="act-btn" on:click=move |_| screen.set(AdminScreen::Edit(p_edit.clone()))>
                                            "EDIT"
                                        </button>
                                        <button class="act-btn" on:click=move |_| screen.set(AdminScreen::PinReset(p_pin.clone()))>
                                            "PIN"
                                        </button>
                                        <button class="act-btn danger" on:click=move |_| deleting.set(true)>
                                            "DEL"
                                        </button>
                                    </div>
                                    <button
                                        class=confirm_class
                                        on:pointerdown=on_hold_start
                                        on:pointerup=on_hold_end
                                        on:pointerleave=on_hold_end
                                        on:pointercancel=on_hold_end
                                    >
                                        <span class="hold-fill"></span>
                                        <span class="del-confirm-label">
                                            {format!("HOLD TO DELETE {}", p.display_name.to_uppercase())}
                                        </span>
                                        <button class="del-cancel" on:click=move |e: leptos::ev::MouseEvent| {
                                            e.stop_propagation();
                                            deleting.set(false);
                                        }>
                                            "\u{00d7}"
                                        </button>
                                    </button>
                                </div>
                            }
                        }).collect_view()}
                        </>
                    }
                }}
                // ADD PROFILE intentionally absent (Chris 2026-04-23):
                // creation lives on the main picker "+" card only.
                // Profile management is edit/PIN/delete only.
            </div>
        </FramedPanel>

        // PLAN 20.6 — app-level window-mode toggle. TV = fullscreen living-room
        // launcher; Desktop = a resizable window. Restart-to-apply (the viewport
        // flags are fixed at eframe::run_native). Mirrors the AdminEdit toggle.
        <FramedPanel class="admin-list-panel">
            <div class="edit-color-label">"window mode (this PC)"</div>
            <Show
                when=move || window_mode.get().is_some()
                fallback=|| view! { <div class="edit-color-label">"checking\u{2026}"</div> }
            >
                <button
                    type="button"
                    class=move || if matches!(window_mode.get(), Some(WindowMode::Desktop)) {
                        "edit-toggle edit-toggle-on"
                    } else {
                        "edit-toggle"
                    }
                    aria-pressed=move || if matches!(window_mode.get(), Some(WindowMode::Desktop)) { "true" } else { "false" }
                    on:click=on_toggle_mode
                >
                    <span class="edit-toggle-track">
                        <span class="edit-toggle-knob"></span>
                    </span>
                    <span class="edit-toggle-label">
                        {move || match window_mode.get() {
                            Some(WindowMode::Desktop) => "DESKTOP",
                            _ => "TV",
                        }}
                    </span>
                </button>
            </Show>
            <div class="edit-color-label">"restart the launcher to apply"</div>
        </FramedPanel>
    }
}

// --------- Admin edit (name + color) ---------

#[component]
fn AdminEdit<F: Fn() + Send + Sync + 'static + Clone>(
    profile: PublicProfile,
    on_back: F,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let name = RwSignal::new(profile.display_name.clone());
    let color = RwSignal::new(profile.color.clone());
    // Kaos toggle — local mirror of the server-side per-profile flag,
    // initialised from the row the picker handed us. Tapping the
    // switch fires `/api/profiles/:id/kaos` immediately (no SAVE
    // step — the flag is cheap to flip either way and there's no
    // confirmation flow). PLAN 9.7 playtest 2026-05-04 (relocated
    // from the kebab overlay).
    let kaos_enabled = RwSignal::new(profile.kaos_enabled);
    let kaos_profile_id = profile.id.clone();
    let on_toggle_kaos = move |_| {
        let new_enabled = !kaos_enabled.get_untracked();
        kaos_enabled.set(new_enabled);
        let pid = kaos_profile_id.clone();
        leptos::task::spawn_local(async move {
            if let Err(e) = crate::api::set_kaos_enabled(&pid, new_enabled).await {
                push_toast(toasts, &format!("Kaos toggle failed: {e}"));
                // Roll back the optimistic flip so the switch stays
                // truthful when the network call fails.
                kaos_enabled.update(|v| *v = !*v);
            }
        });
    };
    let initial = Signal::derive(move || {
        name.with(|n| n.chars().next().unwrap_or('?').to_uppercase().to_string())
    });
    let name_upper = Signal::derive(move || name.with(|n| n.to_uppercase()));
    let color_el = Signal::derive(move || color_to_element(&color.get()));

    let on_save = on_back.clone();
    let on_cancel = on_back.clone();

    view! {
        <div class="admin-edit">
            <button class="btn-back" on:click=move |_| on_cancel()>"BACK"</button>

            <div class="pin-heading">
                <div class="identity-bezel" data-el=move || color_el.get() data-initial=move || initial.get()></div>
                <div class="pin-heading-text">
                    <div class="pin-heading-sub">"editing"</div>
                    <div class="pin-heading-title">{move || format!("EDIT {}", name_upper.get())}</div>
                </div>
            </div>

            <div class="edit-wrap">
                <div class="edit-input-row">
                    <input
                        class="edit-input"
                        type="text"
                        maxlength="16"
                        autocomplete="off"
                        spellcheck="false"
                        prop:value=move || name.get()
                        on:input=move |e| name.set(event_target_value(&e))
                    />
                </div>
                <div class="edit-color-label">"portal color"</div>
                <div class="edit-color-row">
                    {COLOR_SWATCHES.iter().map(|(swatch_name, _hex)| {
                        let swatch_name = swatch_name.to_string();
                        let sn_class = swatch_name.clone();
                        // Map swatch name to its hex for setting color.
                        let hex_val = COLOR_SWATCHES.iter()
                            .find(|(n, _)| *n == swatch_name)
                            .map(|(_, h)| h.to_string())
                            .unwrap_or_default();
                        view! {
                            <div
                                class=move || {
                                    if color_to_element(&color.get()) == sn_class {
                                        "edit-swatch selected"
                                    } else {
                                        "edit-swatch"
                                    }
                                }
                                data-color=swatch_name.clone()
                                on:click=move |_| color.set(hex_val.clone())
                            ></div>
                        }
                    }).collect_view()}
                </div>
                <div class="edit-color-label">"kaos"</div>
                <button
                    type="button"
                    class=move || if kaos_enabled.get() {
                        "edit-toggle edit-toggle-on"
                    } else {
                        "edit-toggle"
                    }
                    aria-pressed=move || if kaos_enabled.get() { "true" } else { "false" }
                    on:click=on_toggle_kaos
                >
                    <span class="edit-toggle-track">
                        <span class="edit-toggle-knob"></span>
                    </span>
                    <span class="edit-toggle-label">
                        {move || if kaos_enabled.get() { "ON" } else { "OFF" }}
                    </span>
                </button>
            </div>

            <div class="actions">
                <button class="btn btn-cancel" on:click=move |_| on_back()>"CANCEL"</button>
                <button class="btn btn-primary" on:click=move |_| {
                    // TODO: wire to update_profile API when available
                    push_toast_level(toasts, "Profile edit saved (UI only - API pending).", ToastLevel::Success);
                    on_save();
                }>"SAVE"</button>
            </div>
        </div>
    }
}

// --------- Admin PIN reset ---------

#[component]
fn AdminPinReset<F: Fn() + Send + Sync + 'static + Clone>(
    profile: PublicProfile,
    on_back: F,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    // Single-step PIN reset (PLAN 9.7 playtest 2026-05-04). This is
    // the Konami-gated "I forgot the PIN" recovery; Konami is the
    // auth gate, no current-PIN re-entry. The previous 2-step flow
    // (CURRENT PIN → NEW PIN) defeated the recovery purpose. Layout +
    // behaviour mirror `PinEntry`: coloured `<GoldBezel>` with the
    // profile's element, name, instruction, dots, keypad — and
    // auto-fires the reset API on 4 digits (no SAVE button row to
    // visually break from the rest of the PIN screens).
    let new_pin = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let success = RwSignal::new(false);

    let initial = profile
        .display_name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    let name_upper = profile.display_name.to_uppercase();
    let bezel_element = color_to_element_enum(&profile.color);
    let id = profile.id.clone();

    let on_done = on_back.clone();
    let on_cancel = on_back;

    // Auto-submit on 4 digits, mirroring PinEntry. Success → toast +
    // on_done() (closes the screen). Failure → clear + error toast,
    // user can retype.
    let id_for_effect = id.clone();
    Effect::new(move |_| {
        let p = new_pin.get();
        if p.len() == 4 && !busy.get() {
            busy.set(true);
            let id = id_for_effect.clone();
            let pin_value = p.clone();
            let on_done_inner = on_done.clone();
            leptos::task::spawn_local(async move {
                match reset_pin(&id, &pin_value).await {
                    Ok(()) => {
                        push_toast_level(toasts, "PIN updated.", ToastLevel::Success);
                        success.set(true);
                        on_done_inner();
                    }
                    Err(e) => {
                        new_pin.set(String::new());
                        push_toast(toasts, &format!("Reset failed: {e}"));
                    }
                }
                busy.set(false);
            });
        }
    });
    let _ = success;

    view! {
        <div class="pin-entry-screen">
            <button class="btn-back" on:click=move |_| on_cancel()>"BACK"</button>

            // Identity section on starfield (matches `PinEntry`).
            <div class="pin-identity">
                <div class="pin-profile-bezel">
                    {
                        let initial_for_bezel = initial.clone();
                        match bezel_element {
                            Some(el) => view! {
                                <GoldBezel size=BezelSize::Lg element=Some(el) state=Signal::derive(|| BezelState::Default)>
                                    <span class="pin-profile-initial" style="color: #fff; font-size: 40px; font-family: 'Titan One', sans-serif; text-shadow: 0 3px 6px rgba(0,0,0,0.6);">
                                        {initial_for_bezel}
                                    </span>
                                </GoldBezel>
                            }.into_any(),
                            None => view! {
                                <GoldBezel size=BezelSize::Lg state=Signal::derive(|| BezelState::Default)>
                                    <span class="pin-profile-initial" style="color: #fff; font-size: 40px; font-family: 'Titan One', sans-serif; text-shadow: 0 3px 6px rgba(0,0,0,0.6);">
                                        {initial_for_bezel}
                                    </span>
                                </GoldBezel>
                            }.into_any(),
                        }
                    }
                </div>
                <div class="pin-prompt-name">{name_upper}</div>
                <div class="pin-prompt-label">"type a new pin"</div>
                <div class="pin-dots">
                    {move || {
                        let p = new_pin.get();
                        (0..4).map(|i| {
                            let filled = i < p.len();
                            let cls = if filled { "pin-dot filled" } else { "pin-dot" };
                            view! {
                                <span class=cls>
                                    <span class="pin-dot-ring"></span>
                                    <span class="pin-dot-fill"></span>
                                </span>
                            }
                        }).collect_view()
                    }}
                </div>
            </div>

            // `locked_out` passed (constant `false`) so PinPad takes the
            // heraldic-reskin branch and DOESN'T render its own legacy
            // `.pin-display` dots — without this the screen showed two
            // dot rows (one mine, one PinPad's). Same trick PinEntry uses.
            <FramedPanel class="pin-keypad-panel panel-in">
                <PinPad pin=new_pin locked_out=Signal::derive(|| false) />
            </FramedPanel>
        </div>
    }
}

/// Four heraldic PIN dots (gold bezel + fill-on-filled) driven by the
/// shared `pin` signal. Same visual treatment as PinEntry's dots but
/// self-contained — no need to wrap the caller in `.pin-entry-screen`
/// just to get the right CSS scope. CreateProfileForm and any future
/// heraldic-keypad callers reuse this directly.
#[component]
fn HeraldicPinDots(pin: RwSignal<String>) -> impl IntoView {
    view! {
        <div class="pin-dots pin-dots-heraldic">
            {move || {
                let p = pin.get();
                (0..4).map(|i| {
                    let filled = i < p.len();
                    let cls = if filled { "pin-dot filled" } else { "pin-dot" };
                    view! {
                        <span class=cls>
                            <span class="pin-dot-ring"></span>
                            <span class="pin-dot-fill"></span>
                        </span>
                    }
                }).collect_view()
            }}
        </div>
    }
}

// --------- Create profile form ---------

/// Staged steps for profile creation (PLAN 4.18.27). Splits the long
/// form in `4.2.8/profile_create.html` into four narrow-viewport-
/// friendly panels so the iPhone confirm keypad doesn't scroll off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CreateStep {
    Name,
    Color,
    Pin,
    Confirm,
}

impl CreateStep {
    fn number(self) -> u8 {
        match self {
            Self::Name => 1,
            Self::Color => 2,
            Self::Pin => 3,
            Self::Confirm => 4,
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Name => "NAME",
            Self::Color => "COLOR",
            Self::Pin => "CHOOSE A PIN",
            Self::Confirm => "CONFIRM PIN",
        }
    }
    fn back(self) -> Option<Self> {
        match self {
            Self::Name => None,
            Self::Color => Some(Self::Name),
            Self::Pin => Some(Self::Color),
            Self::Confirm => Some(Self::Pin),
        }
    }
}

#[component]
fn CreateProfileForm<F: Fn() + Send + Sync + 'static + Clone>(
    on_done: F,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    // Initial name is a random Skylander (PLAN 4.18.7). Kid can keep it,
    // reroll via the ↻ button, or type anything they like over the top.
    let name = RwSignal::new(random_skylander_name().to_string());
    let color = RwSignal::new("#da5ad6".to_string());
    let pin = RwSignal::new(String::new());
    let pin_confirm = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let step = RwSignal::new(CreateStep::Name);
    // Visible inline error for PIN mismatch (PLAN 4.18.8). `None` → no
    // error panel; `Some(_)` → render the banner + attach the `shake`
    // class to the confirm keypad so the mismatch is unmistakable. Any
    // edit to either PIN clears the error so the next attempt starts clean.
    let error: RwSignal<Option<String>> = RwSignal::new(None);

    // Clear error whenever either PIN is edited.
    Effect::new(move |_| {
        pin.track();
        pin_confirm.track();
        if error.get_untracked().is_some() {
            error.set(None);
        }
    });

    // Constant-false locked_out signal flips PinPad into the heraldic
    // reskin (gold-bezel keys + Titan One glyphs) — matches PinEntry's
    // look so the create + unlock flows feel continuous. PLAN 4.18.6a.
    let never_locked: Signal<bool> = Signal::derive(|| false);

    let submit = {
        let on_done = on_done.clone();
        move || {
            if busy.get() {
                return;
            }
            let n = name.get().trim().to_string();
            let p = pin.get();
            let pc = pin_confirm.get();
            let c = color.get();
            if n.is_empty() {
                push_toast_level(toasts, "Name required.", ToastLevel::Warn);
                step.set(CreateStep::Name);
                return;
            }
            if p.len() != 4 || !p.chars().all(|c| c.is_ascii_digit()) {
                push_toast_level(toasts, "PIN must be 4 digits.", ToastLevel::Warn);
                step.set(CreateStep::Pin);
                return;
            }
            if pc.len() != 4 {
                error.set(Some("Re-enter your PIN to confirm.".into()));
                return;
            }
            if p != pc {
                // Wipe the confirm entry so the user can retry without a
                // backspace-marathon. The first PIN survives — typos are
                // far more common on the second entry than the first.
                pin_confirm.set(String::new());
                error.set(Some("PINs don't match. Try the confirm again.".into()));
                return;
            }
            busy.set(true);
            let on_done = on_done.clone();
            leptos::task::spawn_local(async move {
                match create_profile(&n, &p, &c).await {
                    Ok(_) => {
                        push_toast_level(toasts, "Profile created.", ToastLevel::Success);
                        on_done();
                    }
                    Err(e) => push_toast(toasts, &format!("Couldn't create profile: {e}")),
                }
                busy.set(false);
            });
        }
    };

    // Per-step NEXT eligibility. Name requires non-empty; Pin requires
    // 4 digits. Color + Confirm always allow NEXT/CREATE (Confirm's
    // validation lives inside submit).
    let can_advance = Signal::derive(move || match step.get() {
        CreateStep::Name => !name.with(|n| n.trim().is_empty()),
        CreateStep::Color => true,
        CreateStep::Pin => pin.with(|p| p.len() == 4),
        CreateStep::Confirm => pin_confirm.with(|p| p.len() == 4) && !busy.get(),
    });

    let on_next = {
        let submit = submit.clone();
        move |_| match step.get() {
            CreateStep::Name => step.set(CreateStep::Color),
            CreateStep::Color => step.set(CreateStep::Pin),
            CreateStep::Pin => step.set(CreateStep::Confirm),
            CreateStep::Confirm => submit(),
        }
    };

    let make_back_handler = || {
        let on_done = on_done.clone();
        move |_| match step.get().back() {
            Some(prev) => step.set(prev),
            None => on_done(),
        }
    };
    let on_back_top = make_back_handler();

    view! {
        <>
        <button class="btn-back" on:click=on_back_top>"BACK"</button>
        <FramedPanel class="create-profile-panel">
            <div class="create-profile-wizard">
                <div class="create-step-chip">
                    {move || format!("Step {} of 4", step.get().number())}
                </div>
                <div class="create-step-title">{move || step.get().title()}</div>

                // -------- Step 1: Name --------
                <Show when=move || step.get() == CreateStep::Name fallback=|| ()>
                    <div class="create-step-body">
                        <div class="edit-input-row">
                            <input
                                class="edit-input"
                                type="text"
                                maxlength="16"
                                autocomplete="off"
                                spellcheck="false"
                                prop:value=move || name.get()
                                on:input=move |e| name.set(event_target_value(&e))
                            />
                            <button
                                class="roll-btn"
                                type="button"
                                title="pick another"
                                aria-label="Pick a random Skylander name"
                                on:click=move |_| name.set(random_skylander_name().to_string())
                            >"\u{21BB}"</button>
                        </div>
                        <div class="create-name-hint">
                            "anything you like \u{00B7} or tap \u{21BB} for a random one"
                        </div>
                    </div>
                </Show>

                // -------- Step 2: Color --------
                <Show when=move || step.get() == CreateStep::Color fallback=|| ()>
                    <div class="create-step-body">
                        <div class="edit-color-row">
                            {COLOR_SWATCHES.iter().map(|(swatch_name, hex)| {
                                let hex = hex.to_string();
                                let hex_click = hex.clone();
                                let hex_class = hex.clone();
                                let sn = swatch_name.to_string();
                                view! {
                                    <div
                                        class=move || {
                                            if color.get() == hex_class { "edit-swatch selected" } else { "edit-swatch" }
                                        }
                                        data-color=sn
                                        on:click=move |_| color.set(hex_click.clone())
                                    ></div>
                                }
                            }).collect_view()}
                        </div>
                    </div>
                </Show>

                // -------- Step 3: Pin --------
                <Show when=move || step.get() == CreateStep::Pin fallback=|| ()>
                    <div class="create-step-body">
                        <HeraldicPinDots pin />
                        <PinPad pin locked_out=never_locked />
                    </div>
                </Show>

                // -------- Step 4: Confirm --------
                <Show when=move || step.get() == CreateStep::Confirm fallback=|| ()>
                    <div class=move || {
                        let mut s = String::from("create-step-body pin-confirm-wrap");
                        if error.get().is_some() {
                            s.push_str(" shake");
                        }
                        s
                    }>
                        <HeraldicPinDots pin=pin_confirm />
                        <PinPad pin=pin_confirm locked_out=never_locked />
                        <Show when=move || error.get().is_some() fallback=|| ()>
                            <div class="pin-mismatch-banner" role="alert">
                                {move || error.get().unwrap_or_default()}
                            </div>
                        </Show>
                    </div>
                </Show>

                <div class="actions" style="margin-top: 12px;">
                    // BACK moved to the floating `.btn-back` at the top
                    // (next to the kebab) so every flow — Konami, Admin,
                    // PIN entry, create-profile — has one way back in one
                    // spot. Only the forward primary stays in the actions
                    // row. Chris 2026-04-24.
                    <button
                        class="btn btn-primary"
                        disabled=move || !can_advance.get()
                        on:click=on_next
                    >
                        {move || if step.get() == CreateStep::Confirm { "CREATE" } else { "NEXT" }}
                    </button>
                </div>
            </div>
        </FramedPanel>
        </>
    }
}

// --------- PIN entry (for unlocking a profile) ---------

#[component]
fn PinEntry<F: Fn() + Send + Sync + 'static + Clone>(
    profile: PublicProfile,
    on_cancel: F,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let pin = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let success = RwSignal::new(false);
    let lockout_secs = RwSignal::new(0u32);
    let id = profile.id.clone();
    let name = profile.display_name.clone();
    let name_upper = name.to_uppercase();
    let initial = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    // Tint the bezel plate with the profile's chosen colour so the PIN
    // screen reads as the player whose PIN they're entering. Was
    // dropped before (`let _color = ...`) leaving every PIN screen
    // showing the default dark-blue plate (PLAN 9.7 playtest 2026-05-04).
    let bezel_element = color_to_element_enum(&profile.color);

    // Auto-submit when 4 digits entered.
    let id_for_effect = id.clone();
    Effect::new(move |_| {
        let p = pin.get();
        if p.len() == 4 && !busy.get() && lockout_secs.get() == 0 {
            busy.set(true);
            let id = id_for_effect.clone();
            let pin_value = p.clone();
            leptos::task::spawn_local(async move {
                match unlock_profile(&id, &pin_value).await {
                    Ok(_) => {
                        // 4.9.3 — trigger unlock-success animation. WS
                        // ProfileChanged will unmount this view shortly.
                        success.set(true);
                    }
                    Err(e) => {
                        pin.set(String::new());
                        // Check for lockout (429).
                        if e.contains("429") || e.to_lowercase().contains("too many") {
                            lockout_secs.set(30);
                            leptos::task::spawn_local(async move {
                                while lockout_secs.get() > 0 {
                                    crate::gloo_timer(1000).await;
                                    lockout_secs.update(|s| *s = s.saturating_sub(1));
                                }
                            });
                        }
                        push_toast(toasts, &format!("Unlock failed: {e}"));
                    }
                }
                busy.set(false);
            });
        }
    });

    let is_locked_out = Signal::derive(move || lockout_secs.get() > 0);

    let screen_class = move || {
        if success.get() {
            "pin-entry-screen pin-success"
        } else {
            "pin-entry-screen"
        }
    };

    view! {
        <div class=screen_class>
            <button class="btn-back" on:click=move |_| on_cancel()>"BACK"</button>

            // Identity section on starfield (not inside the panel).
            <div class="pin-identity">
                <div class="pin-profile-bezel">
                    {
                        // Local copies for the closure-friendly element prop.
                        let initial_for_bezel = initial.clone();
                        match bezel_element {
                            Some(el) => view! {
                                <GoldBezel size=BezelSize::Lg element=Some(el) state=Signal::derive(|| BezelState::Default)>
                                    <span class="pin-profile-initial" style=format!(
                                        "color: #fff; font-size: 40px; font-family: 'Titan One', sans-serif; text-shadow: 0 3px 6px rgba(0,0,0,0.6);"
                                    )>{initial_for_bezel}</span>
                                </GoldBezel>
                            }.into_any(),
                            None => view! {
                                <GoldBezel size=BezelSize::Lg state=Signal::derive(|| BezelState::Default)>
                                    <span class="pin-profile-initial" style=format!(
                                        "color: #fff; font-size: 40px; font-family: 'Titan One', sans-serif; text-shadow: 0 3px 6px rgba(0,0,0,0.6);"
                                    )>{initial_for_bezel}</span>
                                </GoldBezel>
                            }.into_any(),
                        }
                    }
                </div>
                <div class="pin-prompt-name">{name_upper}</div>
                <div class="pin-prompt-label">"enter your pin"</div>
                <div class="pin-dots">
                    {move || {
                        let p = pin.get();
                        (0..4).map(|i| {
                            let filled = i < p.len();
                            let cls = if filled { "pin-dot filled" } else { "pin-dot" };
                            view! {
                                <span class=cls>
                                    <span class="pin-dot-ring"></span>
                                    <span class="pin-dot-fill"></span>
                                </span>
                            }
                        }).collect_view()
                    }}
                </div>
            </div>

            // Keypad inside a framed panel.
            <FramedPanel class="pin-keypad-panel panel-in">
                <PinPad pin locked_out=is_locked_out />
            </FramedPanel>

            // Lockout banner.
            <Show when=move || is_locked_out.get() fallback=|| ()>
                <div class="pin-lockout-banner">
                    "Too many tries \u{00b7} wait "
                    <span class="pin-lockout-countdown">{move || lockout_secs.get()}</span>
                    "s"
                </div>
            </Show>
        </div>
    }
}

// --------- PIN pad ---------

/// Four-digit touch keypad. Writes into the shared `pin` signal.
#[component]
fn PinPad(
    pin: RwSignal<String>,
    #[prop(optional)] locked_out: Option<Signal<bool>>,
) -> impl IntoView {
    let is_locked = locked_out.unwrap_or(Signal::derive(|| false));
    let has_reskin = locked_out.is_some();

    let digits: [&str; 12] = [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "", "0", "\u{232b}",
    ];

    // Tracks which key is currently visually "pressed" so a fast tap on
    // iOS still flashes the press state (CSS `:active` is unreliable
    // there — the active style can come and go faster than the screen
    // refresh). Set on `pointerdown`, cleared after `PRESS_FLASH_MS`
    // even if the finger is still down, which gives a guaranteed visible
    // pulse per tap. Chris flagged 2026-04-19 alongside double-tap zoom.
    let pressed: RwSignal<Option<String>> = RwSignal::new(None);
    const PRESS_FLASH_MS: i32 = 140;

    view! {
        // Legacy inline dots for non-reskinned callers (CreateProfileForm, AdminPinReset).
        <Show when=move || !has_reskin fallback=|| ()>
            <div class="pin-display">
                {move || {
                    let p = pin.get();
                    (0..4).map(|i| {
                        let filled = i < p.len();
                        view! {
                            <span class={if filled { "pin-dot filled" } else { "pin-dot" }}></span>
                        }
                    }).collect_view()
                }}
            </div>
        </Show>
        <div class={if has_reskin { "pin-keypad-heraldic" } else { "pin-keypad" }}>
            {digits.iter().map(|d| {
                let d = d.to_string();
                let label = d.clone();
                let is_ghost = d.is_empty();
                let is_backspace = d == "\u{232b}";
                let base_cls = if !has_reskin {
                    "pin-key"
                } else if is_ghost {
                    "pin-hkey pin-hkey-ghost"
                } else if is_backspace {
                    "pin-hkey pin-hkey-backspace"
                } else {
                    "pin-hkey"
                };
                let d_for_class = d.clone();
                let class_fn = move || {
                    let mut s = String::from(base_cls);
                    if pressed.get().as_deref() == Some(d_for_class.as_str()) {
                        s.push_str(" pressed");
                    }
                    s
                };
                let d_for_press = d.clone();
                view! {
                    <button
                        class=class_fn
                        disabled=move || is_ghost || is_locked.get()
                        on:pointerdown=move |_| {
                            if is_ghost || is_locked.get() { return; }
                            pressed.set(Some(d_for_press.clone()));
                            let key = d_for_press.clone();
                            leptos::task::spawn_local(async move {
                                crate::gloo_timer(PRESS_FLASH_MS).await;
                                pressed.update(|cur| {
                                    if cur.as_deref() == Some(key.as_str()) {
                                        *cur = None;
                                    }
                                });
                            });
                        }
                        on:click=move |_| {
                            let k = d.clone();
                            if k.is_empty() || is_locked.get() { return; }
                            if k == "\u{232b}" {
                                pin.update(|p| { p.pop(); });
                            } else if pin.with(|p| p.len()) < 4 {
                                pin.update(|p| p.push_str(&k));
                            }
                        }
                    >{label}</button>
                }
            }).collect_view()}
        </div>
    }
}

// --------- Helpers ---------

/// Map a hex color string to an element name for CSS data-attributes.
/// Typed counterpart to `color_to_element` — resolves a profile colour
/// string to the matching `Element` enum so callers that need to feed
/// `<GoldBezel element=...>` can do it without a string round-trip.
/// `None` for unknown colours; the bezel falls back to its default
/// dark-blue plate.
fn color_to_element_enum(color: &str) -> Option<crate::model::Element> {
    use crate::model::Element;
    match color_to_element(color).as_str() {
        "air" => Some(Element::Air),
        "dark" => Some(Element::Dark),
        "earth" => Some(Element::Earth),
        "fire" => Some(Element::Fire),
        "life" => Some(Element::Life),
        "light" => Some(Element::Light),
        "magic" => Some(Element::Magic),
        "tech" => Some(Element::Tech),
        "undead" => Some(Element::Undead),
        "water" => Some(Element::Water),
        _ => None,
    }
}

fn color_to_element(color: &str) -> String {
    for (name, hex) in COLOR_SWATCHES.iter() {
        if color.eq_ignore_ascii_case(hex) {
            return name.to_string();
        }
    }
    // Fallback: try matching partial colour names.
    let c = color.to_lowercase();
    if c.contains("magic") || c.contains("da5a") || c.contains("7a4b") {
        "magic".to_string()
    } else if c.contains("fire") || c.contains("ff6b") || c.contains("ff5a") {
        "fire".to_string()
    } else if c.contains("water") || c.contains("2aa6") || c.contains("4ea8") {
        "water".to_string()
    } else if c.contains("life") || c.contains("5ac9") || c.contains("39d3") {
        "life".to_string()
    } else if c.contains("tech") || c.contains("ffb8") || c.contains("ffb0") {
        "tech".to_string()
    } else {
        "magic".to_string() // default
    }
}
