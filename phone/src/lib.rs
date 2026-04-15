//! Skylander Portal — phone SPA.
//!
//! MVP (Phase 2): connect to the server's WS, show the full figure collection,
//! let the user tap a slot → pick a figure → watch the slot flip
//! Loading → Loaded. The Skylanders aesthetic polish pass comes in Phase 3.

mod api;
mod model;
mod ws;

use leptos::prelude::*;

use crate::api::{
    create_profile, delete_profile, fetch_games, fetch_profiles, fetch_status, post_clear,
    post_launch, post_load, post_quit, reset_pin, unlock_profile,
};
use crate::model::{
    ConnState, Element, GameLaunched, InstalledGame, PublicFigure, PublicProfile, Slot, SlotState,
    UnlockedProfile, SLOT_COUNT,
};

#[component]
pub fn App() -> impl IntoView {
    let portal = RwSignal::new(empty_portal());
    let picking_for = RwSignal::new(None::<u8>);
    let conn = RwSignal::new(ConnState::Connecting);
    let toasts = RwSignal::new(Vec::<ToastMsg>::new());
    let element_filter = RwSignal::new(None::<Element>);
    let search = RwSignal::new(String::new());
    let current_game = RwSignal::new(None::<GameLaunched>);
    let unlocked_profile = RwSignal::new(None::<UnlockedProfile>);
    // Bumps on every profile CRUD so the ProfilePicker re-fetches.
    let profiles_epoch = RwSignal::new(0u32);

    let figures = LocalResource::new(api::fetch_figures);
    let games = LocalResource::new(fetch_games);

    // Fetch the current game on boot; the WS will keep it updated after.
    leptos::task::spawn_local(async move {
        if let Some(g) = fetch_status().await {
            current_game.set(Some(g));
        }
    });

    ws::connect(portal, conn, toasts, current_game, unlocked_profile);

    view! {
        <div class="app">
            <Header conn current_game toasts unlocked_profile />
            <Show
                when=move || unlocked_profile.get().is_some()
                fallback=move || view! {
                    <ProfilePicker toasts profiles_epoch />
                }
            >
            <Show
                when=move || current_game.get().is_some()
                fallback=move || view! {
                    <Suspense fallback=|| view! { <div class="empty-msg">"Loading games…"</div> }>
                        {move || games.get().map(|gs| view! {
                            <GamePicker games=gs.take() toasts />
                        })}
                    </Suspense>
                }
            >
                <Picking picking_for />
                <Portal portal picking_for />
                <Suspense fallback=|| view! { <div class="empty-msg">"Loading figures…"</div> }>
                    {move || figures.get().map(|figs| view! {
                        <Browser
                            figures=figs.take()
                            picking_for
                            portal
                            element_filter
                            search
                            toasts
                        />
                    })}
                </Suspense>
            </Show>
            </Show>
            <ToastStack toasts />
        </div>
    }
}

#[component]
fn GamePicker(games: Vec<InstalledGame>, toasts: RwSignal<Vec<ToastMsg>>) -> impl IntoView {
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

#[component]
fn Header(
    conn: RwSignal<ConnState>,
    current_game: RwSignal<Option<GameLaunched>>,
    toasts: RwSignal<Vec<ToastMsg>>,
    unlocked_profile: RwSignal<Option<UnlockedProfile>>,
) -> impl IntoView {
    let quitting = RwSignal::new(false);
    view! {
        <header class="app-header">
            <div class="brand">
                "Skylander Portal"
                <Show when=move || unlocked_profile.get().is_some() fallback=|| ()>
                    <span class="profile-chip">
                        {move || unlocked_profile.get().map(|p| p.display_name).unwrap_or_default()}
                    </span>
                </Show>
                <Show when=move || current_game.get().is_some() fallback=|| ()>
                    <span class="game-name">
                        {move || current_game.get().map(|g| g.display_name).unwrap_or_default()}
                    </span>
                </Show>
            </div>
            <div class="header-right">
                <span class={move || {
                    let cls = match conn.get() {
                        ConnState::Connecting => "connecting",
                        ConnState::Connected => "connected",
                        ConnState::Disconnected => "disconnected",
                    };
                    format!("status-dot {cls}")
                }}></span>
                <span class="status-label">{move || match conn.get() {
                    ConnState::Connecting => "connecting…",
                    ConnState::Connected => "connected",
                    ConnState::Disconnected => "disconnected",
                }}</span>
                <Show when=move || current_game.get().is_some() fallback=|| ()>
                    <button
                        class="quit-btn"
                        disabled=move || quitting.get()
                        on:click=move |_| {
                            quitting.set(true);
                            leptos::task::spawn_local(async move {
                                if let Err(e) = post_quit(false).await {
                                    push_toast(toasts, &format!("Quit failed: {e}"));
                                }
                                quitting.set(false);
                            });
                        }
                    >
                        "Quit game"
                    </button>
                </Show>
            </div>
        </header>
    }
}

#[component]
fn Picking(picking_for: RwSignal<Option<u8>>) -> impl IntoView {
    view! {
        <Show when=move || picking_for.get().is_some() fallback=|| ()>
            {move || {
                let slot = picking_for.get().unwrap_or(1);
                view! {
                    <div class="picking-banner">
                        <span>{format!("Pick a Skylander for slot {slot}")}</span>
                        <button on:click=move |_| picking_for.set(None)>"Cancel"</button>
                    </div>
                }
            }}
        </Show>
    }
}

#[component]
fn Portal(portal: RwSignal<[Slot; SLOT_COUNT]>, picking_for: RwSignal<Option<u8>>) -> impl IntoView {
    view! {
        <section class="portal">
            {(0..SLOT_COUNT).map(|i| {
                view! { <SlotView idx=i portal picking_for /> }
            }).collect_view()}
        </section>
    }
}

#[component]
fn SlotView(
    idx: usize,
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    picking_for: RwSignal<Option<u8>>,
) -> impl IntoView {
    let slot_num = (idx + 1) as u8;

    let state_class = move || -> &'static str {
        if picking_for.get() == Some(slot_num) {
            return "picking";
        }
        match portal.get()[idx].state {
            SlotState::Empty => "empty",
            SlotState::Loading { .. } => "loading",
            SlotState::Loaded { .. } => "loaded",
            SlotState::Error { .. } => "errored",
        }
    };

    view! {
        <div class={move || format!("slot {}", state_class())}
             on:click=move |_| {
                 let is_empty = matches!(portal.get()[idx].state, SlotState::Empty | SlotState::Error { .. });
                 if is_empty {
                     picking_for.set(Some(slot_num));
                 }
             }>
            <div class="slot-index">{format!("Slot {slot_num}")}</div>
            {move || {
                match portal.get()[idx].state.clone() {
                    SlotState::Empty => view! { <div class="slot-name empty">"Empty"</div> }.into_any(),
                    SlotState::Loading { .. } => view! { <div class="slot-name">"Loading…"</div> }.into_any(),
                    SlotState::Loaded { display_name, .. } => {
                        view! { <div class="slot-name">{display_name}</div> }.into_any()
                    }
                    SlotState::Error { message } => {
                        view! { <div class="slot-err">{message}</div> }.into_any()
                    }
                }
            }}
            {move || {
                match portal.get()[idx].state.clone() {
                    SlotState::Empty | SlotState::Error { .. } => view! {
                        <div class="slot-actions">
                            <button class="slot-btn primary" on:click=move |e| {
                                e.stop_propagation();
                                picking_for.set(Some(slot_num));
                            }>
                                "Pick"
                            </button>
                        </div>
                    }.into_any(),
                    SlotState::Loading { .. } => view! {
                        <div class="slot-actions">
                            <button class="slot-btn" disabled=true>"…"</button>
                        </div>
                    }.into_any(),
                    SlotState::Loaded { .. } => view! {
                        <div class="slot-actions">
                            <button class="slot-btn danger" on:click=move |e| {
                                e.stop_propagation();
                                leptos::task::spawn_local(async move {
                                    let _ = post_clear(slot_num).await;
                                });
                            }>
                                "Remove"
                            </button>
                        </div>
                    }.into_any(),
                }
            }}
        </div>
    }
}

#[component]
fn Browser(
    figures: Vec<PublicFigure>,
    picking_for: RwSignal<Option<u8>>,
    portal: RwSignal<[Slot; SLOT_COUNT]>,
    element_filter: RwSignal<Option<Element>>,
    search: RwSignal<String>,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let all_figures = StoredValue::new(figures);

    let filtered = Memo::new(move |_| {
        let ef = element_filter.get();
        let q = search.get().trim().to_lowercase();
        all_figures.with_value(|figs| {
            figs.iter()
                .filter(|f| ef.map_or(true, |e| f.element == Some(e)))
                .filter(|f| q.is_empty() || f.canonical_name.to_lowercase().contains(&q))
                .take(400) // Phase 3 will virtualize.
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    // Two sets tracked separately so the card UI can tell "currently loading"
    // apart from "already loaded":
    //   - `loaded_names`  — canonical-name matches for fully-Loaded slots.
    //     Used to render the "on portal" visual + fire the "Already on the
    //     portal" toast when the user taps an already-loaded card.
    //   - `loading_ids`   — figure_id markers for Loading slots. Used to
    //     silently suppress repeat taps during the Empty → Loading → Loaded
    //     transition (the spam-click case in 3.6.1) without firing a toast
    //     that the user didn't cause.
    //
    // We compare Loaded by display_name because the server doesn't echo a
    // figure_id back on Loaded events yet (see PLAN 3.8 — name reconciliation).
    let loaded_names = Memo::new(move |_| {
        portal
            .get()
            .iter()
            .filter_map(|s| match &s.state {
                SlotState::Loaded { display_name, .. } => Some(display_name.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    let loading_ids = Memo::new(move |_| {
        portal
            .get()
            .iter()
            .filter_map(|s| match &s.state {
                SlotState::Loading { figure_id: Some(id), .. } => Some(id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    });

    view! {
        <BrowserHead element_filter search />
        <div class="grid">
            <For
                each=move || filtered.get()
                key=|f: &PublicFigure| f.id.clone()
                children=move |f: PublicFigure| {
                    let id = f.id.clone();
                    let id_for_img = id.clone();
                    let name = f.canonical_name.clone();
                    let name_for_img = name.clone();
                    let elem = f.element;
                    let variant = f.variant_tag.clone();
                    let variant_for_show = variant.clone();

                    let name_for_loaded = name.clone();
                    let id_for_loading = id.clone();
                    let is_loaded_this = move || {
                        loaded_names.get().iter().any(|n| n == &name_for_loaded)
                    };
                    let is_loading_this =
                        move || loading_ids.get().iter().any(|id| id == &id_for_loading);

                    let loaded_for_class = is_loaded_this.clone();
                    let loaded_for_click = is_loaded_this.clone();
                    let loaded_for_badge = is_loaded_this.clone();
                    let loading_for_class = is_loading_this.clone();
                    let loading_for_click = is_loading_this.clone();

                    // Per-card transient back-pressure: goes true between the
                    // click firing and the load response returning (ok or
                    // 429). While true, the button reports `disabled` — the
                    // DOM-level disable swallows extra clicks without running
                    // the handler, so spam taps don't pile up either load
                    // requests or toasts. The "Already on the portal" toast
                    // still fires correctly for on-portal cards because
                    // `on_portal` is a separate state (Loaded, not
                    // Submitting).
                    let submitting = RwSignal::new(false);
                    let submitting_for_disabled = submitting;

                    view! {
                        <button
                            class=move || {
                                // `.card.on-portal` is terminal-state only
                                // (the figure is Loaded on a slot), so e2e
                                // tests can wait for it to know a load has
                                // fully completed. `.card.loading` is the
                                // transient state — same visual, different
                                // selector. Lets the spam-click test sit
                                // silent during Loading and the "already"
                                // test distinguish when the toast is due.
                                if loaded_for_class() {
                                    "card on-portal"
                                } else if loading_for_class() {
                                    "card loading"
                                } else {
                                    "card"
                                }
                            }
                            disabled=move || submitting_for_disabled.get()
                            on:click=move |_| {
                                // Three gates, silent → toast:
                                //   1. local submitting — this card just fired
                                //      a load and the 202 hasn't returned yet.
                                //   2. any slot currently Loading this figure —
                                //      the server accepted a prior tap but the
                                //      load hasn't completed. Silent swallow so
                                //      spam taps during Empty→Loading→Loaded
                                //      don't generate toasts.
                                //   3. any slot Loaded with this figure — user
                                //      is trying to re-add; surface "Already".
                                if submitting.get() || loading_for_click() {
                                    return;
                                }
                                if loaded_for_click() {
                                    push_toast(toasts, "Already on the portal.");
                                    return;
                                }
                                let slot = match picking_for.get() {
                                    Some(s) => s,
                                    None => match first_empty_slot(&portal.get()) {
                                        Some(s) => s,
                                        None => {
                                            push_toast(toasts, "Portal is full — remove a figure first.");
                                            return;
                                        }
                                    },
                                };
                                picking_for.set(None);
                                submitting.set(true);
                                let id = id.clone();
                                leptos::task::spawn_local(async move {
                                    let res = post_load(slot, &id).await;
                                    submitting.set(false);
                                    match res {
                                        Ok(()) => {}
                                        Err(e) if e.contains("429") => {}
                                        Err(e) => push_toast(toasts, &format!("Load failed: {e}")),
                                    }
                                });
                            }
                        >
                            <div class="card-icon" data-element=element_slug(elem)>
                                <img
                                    class="card-thumb"
                                    src=format!("/api/figures/{id_for_img}/image?size=thumb")
                                    alt=name_for_img
                                    loading="lazy"
                                    decoding="async"
                                />
                                <span class="card-icon-label">{element_short(elem)}</span>
                            </div>
                            <div class="card-name">{name}</div>
                            <Show when=move || variant_for_show != "base" fallback=|| ()>
                                <div class="card-variant">{variant.clone()}</div>
                            </Show>
                            <Show when=move || loaded_for_badge() fallback=|| ()>
                                <div class="on-portal-badge">"On portal"</div>
                            </Show>
                        </button>
                    }
                }
            />
        </div>
    }
}

#[component]
fn BrowserHead(
    element_filter: RwSignal<Option<Element>>,
    search: RwSignal<String>,
) -> impl IntoView {
    let all_elements: [(Option<Element>, &'static str); 11] = [
        (None, "All"),
        (Some(Element::Air), "Air"),
        (Some(Element::Earth), "Earth"),
        (Some(Element::Fire), "Fire"),
        (Some(Element::Water), "Water"),
        (Some(Element::Life), "Life"),
        (Some(Element::Undead), "Undead"),
        (Some(Element::Tech), "Tech"),
        (Some(Element::Magic), "Magic"),
        (Some(Element::Light), "Light"),
        (Some(Element::Dark), "Dark"),
    ];

    view! {
        <div class="browser-head">
            <input
                class="search"
                type="search"
                placeholder="Search…"
                prop:value=move || search.get()
                on:input=move |e| search.set(event_target_value(&e))
            />
        </div>
        <div class="chip-row">
            {all_elements.into_iter().map(|(val, label)| {
                let v = val;
                view! {
                    <button
                        class={move || if element_filter.get() == v { "chip active" } else { "chip" }}
                        on:click=move |_| element_filter.set(v)
                    >
                        {label}
                    </button>
                }
            }).collect_view()}
        </div>
    }
}

// --------- Profile picker / admin UI ---------

#[component]
fn ProfilePicker(
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

#[component]
fn ToastStack(toasts: RwSignal<Vec<ToastMsg>>) -> impl IntoView {
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

// ---------- helpers ----------

#[derive(Clone, Debug)]
pub(crate) struct ToastMsg {
    id: u64,
    message: String,
}

pub(crate) fn push_toast(toasts: RwSignal<Vec<ToastMsg>>, message: &str) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    // Deduplicate: if an active toast already has this exact message, skip.
    // Prevents spam-click patterns (e.g. repeatedly tapping an already-
    // on-portal card) from stacking identical toasts.
    if toasts.with_untracked(|v| v.iter().any(|t| t.message == message)) {
        return;
    }
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let message = message.to_string();
    toasts.update(|v| v.push(ToastMsg { id, message }));
    leptos::task::spawn_local(async move {
        gloo_timer(4000).await;
        toasts.update(|v| v.retain(|t| t.id != id));
    });
}

async fn gloo_timer(ms: i32) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let cb = Closure::once_into_js(move || {
            let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
        });
        let _ = web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                ms,
            );
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

fn first_empty_slot(p: &[Slot; SLOT_COUNT]) -> Option<u8> {
    for (i, s) in p.iter().enumerate() {
        if matches!(s.state, SlotState::Empty) {
            return Some(i as u8 + 1);
        }
    }
    None
}

fn empty_portal() -> [Slot; SLOT_COUNT] {
    std::array::from_fn(|_| Slot {
        state: SlotState::Empty,
    })
}

fn element_slug(e: Option<Element>) -> &'static str {
    match e {
        Some(Element::Air) => "air",
        Some(Element::Dark) => "dark",
        Some(Element::Earth) => "earth",
        Some(Element::Fire) => "fire",
        Some(Element::Life) => "life",
        Some(Element::Light) => "light",
        Some(Element::Magic) => "magic",
        Some(Element::Tech) => "tech",
        Some(Element::Undead) => "undead",
        Some(Element::Water) => "water",
        None => "none",
    }
}

fn element_short(e: Option<Element>) -> &'static str {
    match e {
        Some(Element::Air) => "AIR",
        Some(Element::Dark) => "DRK",
        Some(Element::Earth) => "ERT",
        Some(Element::Fire) => "FIR",
        Some(Element::Life) => "LIF",
        Some(Element::Light) => "LGT",
        Some(Element::Magic) => "MAG",
        Some(Element::Tech) => "TEC",
        Some(Element::Undead) => "UND",
        Some(Element::Water) => "WAT",
        None => "—",
    }
}

fn event_target_value(e: &leptos::ev::Event) -> String {
    use wasm_bindgen::JsCast;
    e.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}

// ---------- entry ----------

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
