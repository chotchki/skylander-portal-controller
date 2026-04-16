use leptos::prelude::*;

use crate::api::{
    create_profile, delete_profile, fetch_profiles, reset_pin, unlock_profile,
};
use crate::components::{FramedPanel, GoldBezel, BezelSize, BezelState, DisplayHeading, HeadingSize};
use crate::model::PublicProfile;
use crate::{event_target_value, push_toast, ToastMsg};

// --------- Profile picker / admin UI ---------

#[component]
pub(crate) fn ProfilePicker(
    toasts: RwSignal<Vec<ToastMsg>>,
    profiles_epoch: RwSignal<u32>,
) -> impl IntoView {
    let profiles = RwSignal::new(Vec::<PublicProfile>::new());
    let manage_mode = RwSignal::new(false);
    let picked = RwSignal::new(None::<PublicProfile>); // profile whose PIN we're entering
    let admin_target = RwSignal::new(None::<PublicProfile>);

    // Fetch whenever epoch bumps.
    Effect::new(move |_| {
        let _ = profiles_epoch.get();
        leptos::task::spawn_local(async move {
            profiles.set(fetch_profiles().await);
        });
    });

    view! {
        <section class="profile-picker">
            {move || {
                if let Some(p) = admin_target.get() {
                    view! {
                        <ProfileAdmin
                            profile=p
                            on_done=move || { admin_target.set(None); profiles_epoch.update(|v| *v += 1); }
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
                        <ProfileGrid profiles picked admin_target toasts manage_mode profiles_epoch />
                    }.into_any()
                }
            }}
        </section>
    }
}

#[component]
fn ProfileGrid(
    profiles: RwSignal<Vec<PublicProfile>>,
    picked: RwSignal<Option<PublicProfile>>,
    admin_target: RwSignal<Option<PublicProfile>>,
    toasts: RwSignal<Vec<ToastMsg>>,
    manage_mode: RwSignal<bool>,
    profiles_epoch: RwSignal<u32>,
) -> impl IntoView {
    let show_create = RwSignal::new(false);
    let default_state: Signal<BezelState> = Signal::derive(|| BezelState::Default);
    let disabled_state: Signal<BezelState> = Signal::derive(|| BezelState::Disabled);

    view! {
        <button
            class="pp-manage-toggle"
            on:click=move |_| manage_mode.update(|m| *m = !*m)
        >
            {move || if manage_mode.get() { "Done" } else { "Manage" }}
        </button>

        <div class="pp-welcome-wrap">
            <DisplayHeading size=HeadingSize::Lg with_rays=true>
                "PORTAL "
                <span class="pp-welcome-line2">"MASTER"</span>
            </DisplayHeading>
            <div class="pp-welcome-sub">"welcome"</div>
        </div>

        <Show when=move || show_create.get() fallback=|| ()>
            <CreateProfileForm
                on_done=move || { show_create.set(false); profiles_epoch.update(|v| *v += 1); }
                toasts
            />
        </Show>
        <div class="profile-grid">
            {move || {
                let list = profiles.get();
                let in_manage = manage_mode.get();
                let can_add = list.len() < 4;
                view! {
                    <>
                    {list.into_iter().map(|p| {
                        let p_for_click = p.clone();
                        let p_for_manage = p.clone();
                        let color = p.color.clone();
                        let initial = p.display_name.chars().next().unwrap_or('?').to_string();
                        view! {
                            <button
                                class="profile-card"
                                on:click=move |_| {
                                    if in_manage {
                                        admin_target.set(Some(p_for_manage.clone()));
                                    } else {
                                        picked.set(Some(p_for_click.clone()));
                                    }
                                }
                            >
                                <div style=format!("--profile-color:{color}")>
                                    <GoldBezel size=BezelSize::Lg state=default_state>
                                        <span class="pp-initial">{initial}</span>
                                    </GoldBezel>
                                </div>
                                <div class="profile-name">{p.display_name.clone()}</div>
                                <Show when=move || in_manage fallback=|| ()>
                                    <div class="pp-manage-hint">"Tap to manage"</div>
                                </Show>
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
        <div class="pp-tagline">"data & images from the skylanders wiki \u{00b7} cc by-sa"</div>
        {let _ = toasts; view! { <></> }}
    }
}

#[component]
fn CreateProfileForm<F: Fn() + Send + Sync + 'static + Clone>(
    on_done: F,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let name = RwSignal::new(String::new());
    let color = RwSignal::new("#7a4bff".to_string());
    let pin = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    let palette: [&str; 6] = ["#7a4bff", "#ff5a88", "#39d39f", "#ffb033", "#4ea8ff", "#e25858"];

    let submit = {
        let on_done = on_done.clone();
        move |_| {
            if busy.get() { return; }
            let n = name.get().trim().to_string();
            let p = pin.get();
            let c = color.get();
            if n.is_empty() {
                push_toast(toasts, "Name required.");
                return;
            }
            if p.len() != 4 || !p.chars().all(|c| c.is_ascii_digit()) {
                push_toast(toasts, "PIN must be 4 digits.");
                return;
            }
            busy.set(true);
            let on_done = on_done.clone();
            leptos::task::spawn_local(async move {
                match create_profile(&n, &p, &c).await {
                    Ok(_) => {
                        push_toast(toasts, "Profile created.");
                        on_done();
                    }
                    Err(e) => push_toast(toasts, &format!("Couldn't create profile: {e}")),
                }
                busy.set(false);
            });
        }
    };

    view! {
        <div class="create-profile-form">
            <h3>"Create profile"</h3>
            <label>
                "Name"
                <input
                    type="text"
                    prop:value=move || name.get()
                    on:input=move |e| name.set(event_target_value(&e))
                />
            </label>
            <label>"Color"</label>
            <div class="color-picker">
                {palette.iter().map(|hex| {
                    let hex = hex.to_string();
                    let hex_for_style = hex.clone();
                    let hex_for_click = hex.clone();
                    let hex_for_class = hex.clone();
                    view! {
                        <button
                            type="button"
                            class=move || {
                                if color.get() == hex_for_class { "swatch active" } else { "swatch" }
                            }
                            style=format!("background:{hex_for_style}")
                            on:click=move |_| color.set(hex_for_click.clone())
                        ></button>
                    }
                }).collect_view()}
            </div>
            <label>"PIN (4 digits)"</label>
            <PinPad pin />
            <div class="form-actions">
                <button
                    class="primary"
                    disabled=move || busy.get()
                    on:click=submit
                >
                    "Create"
                </button>
                <button on:click=move |_| on_done()>"Cancel"</button>
            </div>
        </div>
    }
}

#[component]
fn ProfileAdmin<F: Fn() + Send + Sync + 'static + Clone>(
    profile: PublicProfile,
    on_done: F,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let mode = RwSignal::new("menu"); // menu | reset | delete
    let current_pin = RwSignal::new(String::new());
    let new_pin = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let name = profile.display_name.clone();
    let id = profile.id.clone();

    let on_done_for_menu = on_done.clone();
    let on_done_for_reset = on_done.clone();
    let on_done_for_delete = on_done.clone();

    let id_for_reset = id.clone();
    let id_for_delete = id.clone();

    view! {
        <div class="profile-admin">
            <h3>{format!("Manage {name}")}</h3>
            {move || match mode.get() {
                "reset" => {
                    let on_done_inner = on_done_for_reset.clone();
                    let id_inner = id_for_reset.clone();
                    let submit = move |_| {
                        if busy.get() { return; }
                        if current_pin.get().len() != 4 || new_pin.get().len() != 4 {
                            push_toast(toasts, "Both PINs must be 4 digits.");
                            return;
                        }
                        busy.set(true);
                        let id = id_inner.clone();
                        let cur = current_pin.get();
                        let new_ = new_pin.get();
                        let on_done = on_done_inner.clone();
                        leptos::task::spawn_local(async move {
                            match reset_pin(&id, &cur, &new_).await {
                                Ok(()) => { push_toast(toasts, "PIN updated."); on_done(); }
                                Err(e) => push_toast(toasts, &format!("Reset failed: {e}")),
                            }
                            busy.set(false);
                        });
                    };
                    view! {
                        <div class="admin-form">
                            <label>"Current PIN"</label>
                            <PinPad pin=current_pin />
                            <label>"New PIN"</label>
                            <PinPad pin=new_pin />
                            <div class="form-actions">
                                <button class="primary" on:click=submit disabled=move || busy.get()>"Update"</button>
                                <button on:click=move |_| mode.set("menu")>"Back"</button>
                            </div>
                        </div>
                    }.into_any()
                }
                "delete" => {
                    let on_done_inner = on_done_for_delete.clone();
                    let id_inner = id_for_delete.clone();
                    let submit = move |_| {
                        if busy.get() { return; }
                        if current_pin.get().len() != 4 {
                            push_toast(toasts, "Enter 4-digit PIN.");
                            return;
                        }
                        busy.set(true);
                        let id = id_inner.clone();
                        let cur = current_pin.get();
                        let on_done = on_done_inner.clone();
                        leptos::task::spawn_local(async move {
                            match delete_profile(&id, &cur).await {
                                Ok(()) => { push_toast(toasts, "Profile deleted."); on_done(); }
                                Err(e) => push_toast(toasts, &format!("Delete failed: {e}")),
                            }
                            busy.set(false);
                        });
                    };
                    view! {
                        <div class="admin-form">
                            <label>"Confirm with current PIN"</label>
                            <PinPad pin=current_pin />
                            <div class="form-actions">
                                <button class="danger" on:click=submit disabled=move || busy.get()>"Delete"</button>
                                <button on:click=move |_| mode.set("menu")>"Back"</button>
                            </div>
                        </div>
                    }.into_any()
                }
                _ => {
                    let on_back = on_done_for_menu.clone();
                    view! {
                        <div class="admin-menu">
                            <button on:click=move |_| mode.set("reset")>"Reset PIN"</button>
                            <button class="danger" on:click=move |_| mode.set("delete")>"Delete profile"</button>
                            <button on:click=move |_| on_back()>"Back"</button>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn PinEntry<F: Fn() + Send + Sync + 'static + Clone>(
    profile: PublicProfile,
    on_cancel: F,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let pin = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let lockout_secs = RwSignal::new(0u32);
    let id = profile.id.clone();
    let name = profile.display_name.clone();
    let name_upper = name.to_uppercase();
    let initial = name.chars().next().unwrap_or('?').to_uppercase().to_string();
    let _color = profile.color.clone();

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
                        // WS ProfileChanged will flip us out of the picker.
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

    view! {
        <div class="pin-entry-screen">
            <button class="pin-back-btn" on:click=move |_| on_cancel()>"BACK"</button>

            // Identity section on starfield (not inside the panel).
            <div class="pin-identity">
                <div class="pin-profile-bezel">
                    <GoldBezel size=BezelSize::Lg state=Signal::derive(|| BezelState::Default)>
                        <span class="pin-profile-initial" style=format!(
                            "color: #fff; font-size: 40px; font-family: 'Titan One', sans-serif; text-shadow: 0 3px 6px rgba(0,0,0,0.6);"
                        )>{initial}</span>
                    </GoldBezel>
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

/// Four-digit touch keypad. Writes into the shared `pin` signal.
///
/// When used inside `PinEntry` the reskinned variant renders: heraldic gold
/// keys inside a `FramedPanel`, mini-bezel PIN dots, and a lockout prop.
/// When used inside `CreateProfileForm` / `ProfileAdmin` (no `locked_out`
/// prop supplied) it keeps the old inline dot display + plain keypad.
#[component]
fn PinPad(
    pin: RwSignal<String>,
    #[prop(optional)]
    locked_out: Option<Signal<bool>>,
) -> impl IntoView {
    let is_locked = locked_out.unwrap_or(Signal::derive(|| false));
    let has_reskin = locked_out.is_some();

    let digits: [&str; 12] = ["1","2","3","4","5","6","7","8","9","","0","\u{232b}"];
    view! {
        // Legacy inline dots for non-reskinned callers (CreateProfileForm, ProfileAdmin).
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
                let cls = if !has_reskin {
                    "pin-key"
                } else if is_ghost {
                    "pin-hkey pin-hkey-ghost"
                } else if is_backspace {
                    "pin-hkey pin-hkey-backspace"
                } else {
                    "pin-hkey"
                };
                view! {
                    <button
                        class=cls
                        disabled=move || is_ghost || is_locked.get()
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
