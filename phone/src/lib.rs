//! Skylander Portal — phone SPA.
//!
//! MVP (Phase 2): connect to the server's WS, show the full figure collection,
//! let the user tap a slot → pick a figure → watch the slot flip
//! Loading → Loaded. The Skylanders aesthetic polish pass comes in Phase 3.

mod api;
mod model;
mod ws;

use leptos::prelude::*;

use crate::api::{fetch_games, fetch_status, post_clear, post_launch, post_load, post_quit};
use crate::model::{
    ConnState, Element, GameLaunched, InstalledGame, PublicFigure, Slot, SlotState, SLOT_COUNT,
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

    let figures = LocalResource::new(api::fetch_figures);
    let games = LocalResource::new(fetch_games);

    // Fetch the current game on boot; the WS will keep it updated after.
    leptos::task::spawn_local(async move {
        if let Some(g) = fetch_status().await {
            current_game.set(Some(g));
        }
    });

    ws::connect(portal, conn, toasts, current_game);

    view! {
        <div class="app">
            <Header conn current_game toasts />
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
) -> impl IntoView {
    let quitting = RwSignal::new(false);
    view! {
        <header class="app-header">
            <div class="brand">
                "Skylander Portal"
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

    // Set of display names currently loaded (or mid-load) on the portal.
    // We compare by canonical_name ↔ RPCS3's display_name because the server
    // doesn't yet echo a figure_id back in Loaded events (Phase 3 reconciles).
    let loaded_names = Memo::new(move |_| {
        let p = portal.get();
        let mut names: Vec<String> = Vec::new();
        for s in p.iter() {
            match &s.state {
                SlotState::Loaded { display_name, .. } => names.push(display_name.clone()),
                SlotState::Loading {
                    figure_id: Some(id),
                } => names.push(format!("__id:{id}")),
                _ => {}
            }
        }
        names
    });

    view! {
        <BrowserHead element_filter search />
        <div class="grid">
            <For
                each=move || filtered.get()
                key=|f: &PublicFigure| f.id.clone()
                children=move |f: PublicFigure| {
                    let id = f.id.clone();
                    let name = f.canonical_name.clone();
                    let elem = f.element;
                    let variant = f.variant_tag.clone();
                    let variant_for_show = variant.clone();

                    let name_for_check = name.clone();
                    let id_for_check = id.clone();
                    let is_on_portal = move || {
                        let loaded = loaded_names.get();
                        let id_marker = format!("__id:{id_for_check}");
                        loaded
                            .iter()
                            .any(|n| n == &name_for_check || n == &id_marker)
                    };
                    let on_portal_for_class = is_on_portal.clone();
                    let on_portal_for_disabled = is_on_portal.clone();
                    let on_portal_for_click = is_on_portal.clone();
                    let on_portal_for_badge = is_on_portal.clone();

                    view! {
                        <button
                            class=move || {
                                if on_portal_for_class() { "card on-portal" } else { "card" }
                            }
                            disabled=move || on_portal_for_disabled()
                            on:click=move |_| {
                                if on_portal_for_click() {
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
                                let id = id.clone();
                                leptos::task::spawn_local(async move {
                                    match post_load(slot, &id).await {
                                        Ok(()) => {}
                                        Err(e) if e.contains("429") => {}
                                        Err(e) => push_toast(toasts, &format!("Load failed: {e}")),
                                    }
                                });
                            }
                        >
                            <div class="card-icon" data-element=element_slug(elem)>
                                {element_short(elem)}
                            </div>
                            <div class="card-name">{name}</div>
                            <Show when=move || variant_for_show != "base" fallback=|| ()>
                                <div class="card-variant">{variant.clone()}</div>
                            </Show>
                            <Show when=move || on_portal_for_badge() fallback=|| ()>
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
