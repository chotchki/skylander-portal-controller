//! `/api/_test/tv` (design decision 8): a `PostUpdate` copy of what the TV
//! draws, so the e2e suite asserts on the TV without screenshots. Mounted
//! only under `test-hooks`. Read off the laid-out UI tree: the players line,
//! join address, join QR and every card's lines, portrait, pip and rect.
//! Copied from the resources that feed the tree: the screen, game, message,
//! Setup's lists, each card's figure and owner ids, and the intro queue. The
//! focused choice is `InputFocus`'s entity's `FocusId`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::rejection::JsonRejection;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform, UiSystems};
use serde::{Deserialize, Serialize};

use crate::cards::PortalCards;
use crate::extraction::{ExtractStatus, Extraction};
use crate::input::{FocusId, PostedPresses, TestPress};
use crate::intro::{IntroEntry, IntroEvent, IntroQueue};
use crate::intro_player::{self, IntroOverlay, IntroPlayer, IntroVideo};
use crate::screen::TvScreen;
use crate::turntable::{CardPlate, CardState, Turntables};
use crate::tv::{
    CardSlot, Current, EmptySlot, ExtractBar, ExtractDetail, ExtractTitle, Framing, JoinAddress,
    JoinQr, Pip, PlayersLine, Setup, TvAssets, TvCamera, contains, node_rect, safe_area,
};

/// What the TV shows this frame.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TvSnapshot {
    /// `TvScreen`'s variant: `Join`, `Portal`, `NoGiants`, ...
    pub screen: String,
    pub game: Option<String>,
    /// ServerError's message.
    pub message: Option<String>,
    /// The count in the drawn players line; none when it isn't drawn.
    pub players: Option<usize>,
    /// The drawn join address.
    pub join_address: Option<String>,
    /// The join QR image is drawn.
    pub join_qr: bool,
    /// Setup's lists (the resource, not the drawn text), for Setup, NoGiants and Found.
    pub found: Vec<String>,
    pub folders: Vec<String>,
    pub problems: Vec<String>,
    /// The focused choice's id (`choose-folder`, `continue`, `close`); none
    /// when no choice has the focus.
    pub focus: Option<String>,
    /// One per card drawn, in slot order.
    pub cards: Vec<CardShot>,
    /// Slots drawn as empty frames, 1-based.
    pub empty_slots: Vec<u8>,
    /// Physical window size the layout below is in.
    pub window: [f32; 2],
    /// Cards not wholly inside the inset (TV) or margin (desktop), 1-based.
    pub cards_outside: Vec<u8>,
    /// Pairs of cards that overlap.
    pub overlapping: usize,
    /// The first-run extraction (AB.9).
    pub extraction: ExtractionShot,
    /// The intro playing over the portal view (AB.10).
    pub intro: Option<IntroShot>,
    /// Intros waiting, in play order.
    pub intro_queue: Vec<QueuedShot>,
}

/// A queued intro, from `IntroQueue`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueuedShot {
    /// 1-based.
    pub slot: u8,
    pub figure_id: String,
    pub movie: String,
}

impl From<&IntroEntry> for QueuedShot {
    fn from(e: &IntroEntry) -> Self {
        Self {
            slot: e.slot.as_u8() + 1,
            figure_id: e.figure_id.clone(),
            movie: e.movie.clone(),
        }
    }
}

/// The playing intro: the queue's entry, the player's counts, the overlay as drawn.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntroShot {
    pub slot: u8,
    pub figure_id: String,
    pub movie: String,
    /// The video's frame count once open; frames presented and passed over.
    pub frames: Option<usize>,
    pub shown: usize,
    pub skipped: usize,
    /// `waiting`, `starting`, `playing` (a sink) or `none` (wall clock only).
    pub audio: String,
    /// The overlay is drawn and covers the whole window.
    pub full_screen: bool,
    /// The picture's x, y, width, height in physical pixels, once open.
    pub video_rect: Option<[f32; 4]>,
}

/// The extraction's state from the resource; the band's lines and bar as drawn.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ExtractionShot {
    /// `ExtractStatus`'s variant: `Idle`, `Checking`, `Converting`, `Ready`, `Failed`, ...
    pub state: String,
    /// A failure's `GUAC-E-*` code.
    pub code: Option<String>,
    /// Converting: intros done, of how many.
    pub converted: Option<[usize; 2]>,
    /// Ready: toy types with an intro, and whether this start extracted.
    pub intros: Option<usize>,
    pub ran: Option<bool>,
    pub title: Option<String>,
    pub detail: Option<String>,
    /// The drawn bar's fill.
    pub percent: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CardShot {
    /// 1-based, as the phone numbers slots.
    pub slot: u8,
    /// From `PortalCards`, not drawn.
    pub figure_id: String,
    /// Every line drawn on the card outside the pip, top to bottom.
    pub texts: Vec<String>,
    /// An image drawn in the portrait frame: the portrait, or the model.
    pub portrait: bool,
    /// The card's turntable (AE.13): `model` (its model is on the card),
    /// `portrait` (no model to show, or one that couldn't be) or `loading`
    /// (the portrait while the model is read or the extract makes it).
    pub state: String,
    /// Why a `portrait` card has no model, as the log has it.
    pub reason: Option<String>,
    /// The bezel plate the portrait or the model draws in: x, y, width,
    /// height in physical pixels.
    pub plate: Option<[f32; 4]>,
    pub pip: Option<PipShot>,
    /// x, y, width, height in physical pixels.
    pub rect: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PipShot {
    pub initial: String,
    /// The pip's fill as drawn, `#RRGGBB`.
    pub color: String,
    /// From `PortalCards`, not drawn.
    pub profile_id: String,
}

/// Shared with the routes: the last snapshot, skips asked for over HTTP and
/// presses posted for the input router.
#[derive(Resource, Clone, Default)]
pub struct TvMirror(
    pub Arc<Mutex<TvSnapshot>>,
    pub Arc<AtomicUsize>,
    pub(crate) PostedPresses,
);

pub struct Mirror;

impl Plugin for Mirror {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, mirror_tv.after(UiSystems::Layout))
            .add_systems(Update, skip_asked.before(intro_player::play_intros));
    }
}

/// `POST /api/_test/tv/press`'s body.
#[derive(Deserialize)]
struct PressBody {
    button: String,
}

/// `/api/_test/tv` serves whatever the last frame wrote; `POST
/// /api/_test/tv/skip` skips the playing intro as a key would, for a run
/// with nobody at the keyboard; `POST /api/_test/tv/press` hands the input
/// router a press (`{"button": "south"}`: south, east, mode, up, down, left
/// or right; anything else is a 400). A test hook, not a phone remote.
/// Generic so a test needs no `AppState`.
pub fn routes<S: Clone + Send + Sync + 'static>(mirror: &TvMirror) -> axum::Router<S> {
    let shared = mirror.0.clone();
    let skips = mirror.1.clone();
    let presses = mirror.2.clone();
    axum::Router::new()
        .route(
            "/api/_test/tv",
            axum::routing::get(move || {
                let shared = shared.clone();
                async move { axum::Json(shared.lock().unwrap_or_else(|e| e.into_inner()).clone()) }
            }),
        )
        .route(
            "/api/_test/tv/skip",
            axum::routing::post(move || {
                skips.fetch_add(1, Ordering::Relaxed);
                async { axum::http::StatusCode::ACCEPTED }
            }),
        )
        .route(
            "/api/_test/tv/press",
            axum::routing::post(move |body: Result<axum::Json<PressBody>, JsonRejection>| {
                let pressed = body
                    .ok()
                    .and_then(|axum::Json(b)| TestPress::parse(&b.button))
                    .map(|press| presses.push(press));
                async move {
                    match pressed {
                        Some(()) => axum::http::StatusCode::ACCEPTED,
                        None => axum::http::StatusCode::BAD_REQUEST,
                    }
                }
            }),
        )
}

fn skip_asked(mirror: Res<TvMirror>, queue: Option<ResMut<IntroQueue>>) {
    if mirror.1.swap(0, Ordering::Relaxed) == 0 {
        return;
    }
    if let Some(mut queue) = queue.filter(|q| q.playing().is_some()) {
        info!("intro skipped over /api/_test/tv/skip");
        intro_player::apply(&mut queue, IntroEvent::Skip);
    }
}

/// Depth-first text and images under `root`, the pip's subtree apart.
fn walk(
    root: Entity,
    children: &Query<&Children>,
    texts: &Query<&Text>,
    images: &Query<(), With<ImageNode>>,
    pips: &Query<(&Pip, &BackgroundColor)>,
    out: &mut (Vec<String>, bool, Option<(String, Color)>),
) {
    let Ok(kids) = children.get(root) else {
        return;
    };
    for kid in kids.iter() {
        if let Ok((_, fill)) = pips.get(kid) {
            let mut inner = (Vec::new(), false, None);
            walk(kid, children, texts, images, pips, &mut inner);
            out.2 = Some((inner.0.join(""), fill.0));
            continue;
        }
        if let Ok(text) = texts.get(kid) {
            out.0.push(text.0.clone());
        }
        if images.contains(kid) {
            out.1 = true;
        }
        walk(kid, children, texts, images, pips, out);
    }
}

fn mirror_tv(
    mirror: Res<TvMirror>,
    current: Option<Res<Current>>,
    assets: Option<Res<TvAssets>>,
    setup: Res<Setup>,
    framing: Res<Framing>,
    portal: Res<PortalCards>,
    camera: Query<&Camera, With<TvCamera>>,
    cards: Query<(Entity, &CardSlot, &ComputedNode, &UiGlobalTransform)>,
    empties: Query<&EmptySlot>,
    children: Query<&Children>,
    texts: Query<&Text>,
    images: Query<(), With<ImageNode>>,
    pips: Query<(&Pip, &BackgroundColor)>,
    (players_line, address, qr, input_focus, choices): (
        Query<&Text, With<PlayersLine>>,
        Query<&Text, With<JoinAddress>>,
        Query<&ImageNode, With<JoinQr>>,
        Option<Res<InputFocus>>,
        Query<&FocusId>,
    ),
    (extraction, band_title, band_detail, bar): (
        Option<Res<Extraction>>,
        Query<&Text, With<ExtractTitle>>,
        Query<&Text, With<ExtractDetail>>,
        Query<&ExtractBar>,
    ),
    (queue, player, overlays, videos, turntables, plates): (
        Option<Res<IntroQueue>>,
        Option<Res<IntroPlayer>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<IntroOverlay>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<IntroVideo>>,
        Option<Res<Turntables>>,
        Query<(&CardPlate, &ComputedNode, &UiGlobalTransform)>,
    ),
) {
    let Some(current) = current else {
        return;
    };
    let size = camera
        .single()
        .ok()
        .and_then(Camera::physical_target_size)
        .unwrap_or_default();
    let safe = safe_area(size.x, size.y, framing.0);
    let mut drawn: Vec<(CardShot, Rect)> = cards
        .iter()
        .map(|(entity, slot, node, at)| {
            let mut found = (Vec::new(), false, None);
            walk(entity, &children, &texts, &images, &pips, &mut found);
            let placement = portal.slots.get(slot.0 as usize).and_then(Option::as_ref);
            let rect = node_rect(node, at);
            let shot = CardShot {
                slot: slot.0 + 1,
                figure_id: placement
                    .map(|p| p.figure_id.as_str().to_string())
                    .unwrap_or_default(),
                texts: found.0,
                portrait: found.1,
                state: turntables
                    .as_ref()
                    .and_then(|t| t.card_state(slot.0))
                    .unwrap_or(CardState::Portrait)
                    .name()
                    .into(),
                reason: turntables
                    .as_ref()
                    .and_then(|t| t.portrait_reason(slot.0))
                    .map(str::to_owned),
                plate: plates
                    .iter()
                    .find(|(p, ..)| p.slot == slot.0)
                    .map(|(_, n, t)| {
                        let r = node_rect(n, t);
                        [r.min.x, r.min.y, r.width(), r.height()]
                    }),
                pip: found.2.map(|(initial, fill)| PipShot {
                    initial,
                    color: Srgba::from(fill).to_hex(),
                    profile_id: placement
                        .and_then(|p| p.owner.as_ref())
                        .map(|o| o.profile_id.clone())
                        .unwrap_or_default(),
                }),
                rect: [rect.min.x, rect.min.y, rect.width(), rect.height()],
            };
            (shot, rect)
        })
        .collect();
    drawn.sort_by_key(|(c, _)| c.slot);
    let cards_outside = drawn
        .iter()
        .filter(|(_, r)| !contains(safe, *r))
        .map(|(c, _)| c.slot)
        .collect();
    let overlapping = drawn
        .iter()
        .enumerate()
        .flat_map(|(i, (_, a))| drawn[i + 1..].iter().map(move |(_, b)| (*a, *b)))
        .filter(|(a, b)| !a.intersect(*b).is_empty())
        .count();
    let mut empty_slots: Vec<u8> = empties.iter().map(|e| e.0 + 1).collect();
    empty_slots.sort();
    let (game, message) = match &current.screen {
        TvScreen::Portal { game } => (Some(game.clone()), None),
        TvScreen::ServerError { message } => (None, Some(message.clone())),
        _ => (None, None),
    };
    let status = extraction.map(|e| e.status.clone()).unwrap_or_default();
    let extraction = ExtractionShot {
        state: status.kind().to_string(),
        code: match &status {
            ExtractStatus::Failed(f) => Some(f.code().to_string()),
            _ => None,
        },
        converted: match &status {
            ExtractStatus::Converting { done, total, .. } => Some([*done, *total]),
            _ => None,
        },
        intros: match &status {
            ExtractStatus::Ready { intros, .. } => Some(*intros),
            _ => None,
        },
        ran: match &status {
            ExtractStatus::Ready { ran, .. } => Some(*ran),
            _ => None,
        },
        title: band_title.iter().next().map(|t| t.0.clone()),
        detail: band_detail.iter().next().map(|t| t.0.clone()),
        percent: bar.iter().next().map(|b| b.0),
    };
    let window = Rect::new(0.0, 0.0, size.x as f32, size.y as f32);
    let intro = queue.as_ref().and_then(|q| q.playing()).map(|entry| {
        let now = player
            .as_ref()
            .and_then(|p| p.now())
            .filter(|n| n.entry.id == entry.id);
        let full_screen = overlays.iter().any(|(node, at)| {
            let r = node_rect(node, at);
            // A pixel of slack, as for the cards.
            (r.min - window.min).abs().max_element() <= 1.0
                && (r.max - window.max).abs().max_element() <= 1.0
        });
        IntroShot {
            slot: entry.slot.as_u8() + 1,
            figure_id: entry.figure_id.clone(),
            movie: entry.movie.clone(),
            frames: now.as_ref().and_then(|n| n.frames),
            shown: now.as_ref().map_or(0, |n| n.shown),
            skipped: now.as_ref().map_or(0, |n| n.skipped),
            audio: now.as_ref().map_or("waiting", |n| n.audio).to_string(),
            full_screen,
            video_rect: videos.iter().next().map(|(node, at)| {
                let r = node_rect(node, at);
                [r.min.x, r.min.y, r.width(), r.height()]
            }),
        }
    });
    let intro_queue = queue
        .as_ref()
        .map(|q| q.queued().map(QueuedShot::from).collect())
        .unwrap_or_default();
    let snapshot = TvSnapshot {
        screen: current.screen.kind().to_string(),
        game,
        message,
        players: players_line
            .iter()
            .next()
            .and_then(|t| t.0.split_whitespace().next()?.parse().ok()),
        join_address: address.iter().next().map(|t| t.0.clone()),
        join_qr: assets
            .and_then(|a| a.qr.clone())
            .is_some_and(|handle| qr.iter().any(|image| image.image == handle)),
        found: setup.0.found_titles.clone(),
        folders: setup.0.folders.clone(),
        problems: setup.0.problems.clone(),
        focus: input_focus
            .and_then(|f| f.get())
            .and_then(|e| choices.get(e).ok())
            .map(|id| id.0.to_string()),
        cards: drawn.into_iter().map(|(c, _)| c).collect(),
        empty_slots,
        window: [size.x as f32, size.y as f32],
        cards_outside,
        overlapping,
        extraction,
        intro,
        intro_queue,
    };
    let mut shared = mirror.0.lock().unwrap_or_else(|e| e.into_inner());
    if *shared != snapshot {
        *shared = snapshot;
    }
}

#[cfg(test)]
mod tests {
    use guac_server::config::WindowMode;
    use skylander_core::SlotIndex;

    use super::*;
    use crate::driver::PortalChange;
    use crate::tv::layout_tests::{placement, portal_app};

    fn snapshot(cards: PortalCards, screen: TvScreen) -> TvSnapshot {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, cards);
        app.world_mut().resource_mut::<Current>().screen = screen;
        let mirror = TvMirror::default();
        app.insert_resource(mirror.clone()).add_plugins(Mirror);
        for _ in 0..3 {
            app.update();
        }
        mirror.0.lock().unwrap().clone()
    }

    fn giants() -> TvScreen {
        TvScreen::Portal {
            game: "Skylanders: Giants".into(),
        }
    }

    #[test]
    fn a_card_reads_back_as_drawn() {
        let cards = PortalCards::default().reduce([
            PortalChange::Placed {
                slot: SlotIndex::new(0).unwrap(),
                placement: placement("Eruptor", Some("#e8423f")),
            },
            PortalChange::Placed {
                slot: SlotIndex::new(5).unwrap(),
                placement: placement("Tree Rex", None),
            },
        ]);
        let shot = snapshot(cards, giants());
        assert_eq!(shot.screen, "Portal");
        assert_eq!(shot.game.as_deref(), Some("Skylanders: Giants"));
        assert_eq!(shot.players, Some(2));
        assert_eq!(
            shot.join_address.as_deref(),
            Some("http://christophers-macbook-pro.local:8765")
        );
        // The harness has no QR.
        assert!(!shot.join_qr);
        assert_eq!(shot.cards.len(), 2);
        let first = &shot.cards[0];
        assert_eq!(first.slot, 1);
        assert_eq!(first.figure_id, "000009-0000");
        assert_eq!(first.texts, ["Eruptor", "Fire"]);
        assert!(!first.portrait);
        let pip = first.pip.as_ref().unwrap();
        assert_eq!(
            (
                pip.initial.as_str(),
                pip.color.as_str(),
                pip.profile_id.as_str()
            ),
            ("A", "#E8423F", "p")
        );
        assert_eq!(shot.cards[1].slot, 6);
        assert!(shot.cards[1].pip.is_none());
        assert_eq!(shot.empty_slots, [2, 3, 4, 5, 7, 8]);
        assert_eq!(shot.window, [1920.0, 1080.0]);
        assert!(shot.cards_outside.is_empty());
        assert_eq!(shot.overlapping, 0);
        assert!(
            shot.cards
                .iter()
                .all(|c| c.rect[2] > 0.0 && c.rect[3] > 0.0)
        );
    }

    #[test]
    fn off_the_portal_screen_no_card_is_drawn() {
        let cards = PortalCards::default().reduce([PortalChange::Placed {
            slot: SlotIndex::new(0).unwrap(),
            placement: placement("Eruptor", Some("#e8423f")),
        }]);
        let shot = snapshot(cards, TvScreen::Join);
        assert_eq!(shot.screen, "Join");
        assert!(shot.game.is_none());
        assert!(shot.cards.is_empty() && shot.empty_slots.is_empty());
    }

    #[test]
    fn the_join_screen_reads_back_its_qr_and_a_new_phone() {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        let qr = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        app.world_mut().resource_mut::<TvAssets>().qr = Some(qr);
        {
            let mut current = app.world_mut().resource_mut::<Current>();
            current.screen = TvScreen::Join;
            current.clients = 0;
        }
        let mirror = TvMirror::default();
        app.insert_resource(mirror.clone()).add_plugins(Mirror);
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().clone();
        assert!(shot.join_qr);
        assert_eq!(shot.players, Some(0));
        assert_eq!(
            shot.join_address.as_deref(),
            Some("http://christophers-macbook-pro.local:8765")
        );

        // Read off the redrawn line, so a tree that missed the change shows 0.
        app.world_mut().resource_mut::<Current>().clients = 1;
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(mirror.0.lock().unwrap().players, Some(1));

        // Screens without the join block draw none of it.
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Farewell;
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().clone();
        assert_eq!(
            (shot.players, shot.join_address, shot.join_qr),
            (None, None, false)
        );
    }

    #[test]
    fn the_extraction_reads_back_its_state_and_drawn_band() {
        use crate::extraction::{ExtractBand, ExtractStatus, Extraction, Failure, band};
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        let mirror = TvMirror::default();
        app.insert_resource(mirror.clone()).add_plugins(Mirror);
        app.update();
        let idle = mirror.0.lock().unwrap().extraction.clone();
        assert_eq!(idle.state, "Idle");
        assert_eq!((idle.title, idle.percent), (None, None));

        let converting = ExtractStatus::Converting {
            done: 12,
            total: 48,
            movie: "eruptor".into(),
        };
        app.insert_resource(ExtractBand(band(&converting)));
        app.insert_resource(Extraction {
            status: converting,
            ..Default::default()
        });
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().extraction.clone();
        assert_eq!(shot.state, "Converting");
        assert_eq!(shot.converted, Some([12, 48]));
        assert_eq!(shot.detail.as_deref(), Some("Converting intro 13 of 48…"));
        assert_eq!(shot.percent, Some(25));
        assert_eq!(shot.code, None);

        let failed = ExtractStatus::Failed(Failure::NoFfmpeg {
            configured: None,
            install: "brew install ffmpeg".into(),
        });
        app.insert_resource(ExtractBand(band(&failed)));
        app.insert_resource(Extraction {
            status: failed,
            ..Default::default()
        });
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().extraction.clone();
        assert_eq!(shot.state, "Failed");
        assert_eq!(shot.code.as_deref(), Some("GUAC-E-NO-FFMPEG"));
        assert_eq!(shot.title.as_deref(), Some("No intros: ffmpeg is needed"));
        assert!(shot.detail.unwrap().contains("brew install ffmpeg"));
        assert_eq!(shot.percent, None);

        app.insert_resource(ExtractBand(None));
        app.insert_resource(Extraction {
            status: ExtractStatus::Ready {
                intros: 53,
                ran: false,
            },
            ..Default::default()
        });
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().extraction.clone();
        assert_eq!(
            (shot.state.as_str(), shot.intros, shot.ran),
            ("Ready", Some(53), Some(false))
        );
        assert_eq!(shot.title, None);
    }

    #[tokio::test]
    async fn the_route_serves_the_last_snapshot() {
        use tower::ServiceExt;
        let mirror = TvMirror::default();
        mirror.0.lock().unwrap().screen = "NoGiants".into();
        let app = routes::<()>(&mirror);
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/_test/tv")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["screen"], "NoGiants");
        assert_eq!(json["cards"], serde_json::json!([]));
        assert_eq!(json["intro"], serde_json::Value::Null);
        assert_eq!(json["intro_queue"], serde_json::json!([]));

        let response = app
            .oneshot(
                axum::http::Request::post("/api/_test/tv/skip")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 202);
        assert_eq!(mirror.1.load(Ordering::Relaxed), 1);
    }

    /// AP.5: `south` is queued and the router takes it as the screen's
    /// Activate; a name it doesn't know, or no name, is a 400 and queues nothing.
    #[tokio::test]
    async fn a_posted_press_reaches_the_router_and_an_unknown_one_is_refused() {
        use crate::input::TvInput;
        use crate::input::heard::{listen, take};
        use tower::ServiceExt;
        let mirror = TvMirror::default();
        let routes = routes::<()>(&mirror);
        for (body, status) in [
            (r#"{"button":"south"}"#, 202),
            (r#"{"button":"jump"}"#, 400),
            (r#"{"press":"south"}"#, 400),
            ("south", 400),
        ] {
            let response = routes
                .clone()
                .oneshot(
                    axum::http::Request::post("/api/_test/tv/press")
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{body}");
        }
        assert_eq!(mirror.2.len(), 1);

        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        app.insert_resource(mirror.clone());
        listen(&mut app);
        app.update();
        assert_eq!(take(&mut app), [TvInput::Activate]);
        assert_eq!(mirror.2.len(), 0);
        app.update();
        assert_eq!(take(&mut app), []);
    }
}
