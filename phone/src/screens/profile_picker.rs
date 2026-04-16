use leptos::prelude::*;

use crate::api::{
    create_profile, delete_profile, fetch_profiles, reset_pin, unlock_profile,
};
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
            <h2>"Welcome, portal master"</h2>
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
    view! {
        <div class="profile-controls">
            <button
                class="manage-toggle"
                on:click=move |_| manage_mode.update(|m| *m = !*m)
            >
                {move || if manage_mode.get() { "Done" } else { "Manage profiles" }}
            </button>
            <button
                class="create-profile-btn"
                on:click=move |_| show_create.set(true)
            >
                "+ Create profile"
            </button>
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
                if list.is_empty() {
                    view! { <div class="empty-msg">"No profiles yet. Create one to get started."</div> }.into_any()
                } else {
                    let in_manage = manage_mode.get();
                    view! {
                        <>
                        {list.into_iter().map(|p| {
                            let p_for_click = p.clone();
                            let p_for_manage = p.clone();
                            let swatch = p.color.clone();
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
                                    <div class="profile-swatch" style=format!("background:{swatch}")>
                                        {initial}
                                    </div>
                                    <div class="profile-name">{p.display_name.clone()}</div>
                                    <Show when=move || in_manage fallback=|| ()>
                                        <div class="profile-manage-hint">"Tap to manage"</div>
                                    </Show>
                                </button>
                            }
                        }).collect_view()}
                        </>
                    }.into_any()
                }
            }}
        </div>
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
    let id = profile.id.clone();
    let name = profile.display_name.clone();

    let submit = move |_| {
        if busy.get() { return; }
        let pin_value = pin.get();
        if pin_value.len() != 4 {
            push_toast(toasts, "Enter 4 digits.");
            return;
        }
        busy.set(true);
        let id = id.clone();
        leptos::task::spawn_local(async move {
            match unlock_profile(&id, &pin_value).await {
                Ok(_) => {
                    // WS ProfileChanged will flip us out of the picker.
                }
                Err(e) => {
                    pin.set(String::new());
                    push_toast(toasts, &format!("Unlock failed: {e}"));
                }
            }
            busy.set(false);
        });
    };

    view! {
        <div class="pin-entry">
            <h3>{format!("Enter PIN for {name}")}</h3>
            <PinPad pin />
            <div class="form-actions">
                <button
                    class="primary"
                    disabled=move || busy.get() || pin.get().len() != 4
                    on:click=submit
                >
                    "Unlock"
                </button>
                <button on:click=move |_| on_cancel()>"Cancel"</button>
            </div>
        </div>
    }
}

/// Four-digit touch keypad. Writes into the shared `pin` signal.
#[component]
fn PinPad(pin: RwSignal<String>) -> impl IntoView {
    let digits: [&str; 12] = ["1","2","3","4","5","6","7","8","9","","0","⌫"];
    view! {
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
        <div class="pin-keypad">
            {digits.iter().map(|d| {
                let d = d.to_string();
                let label = d.clone();
                view! {
                    <button
                        class="pin-key"
                        disabled=d.is_empty()
                        on:click=move |_| {
                            let k = d.clone();
                            if k.is_empty() { return; }
                            if k == "⌫" {
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
