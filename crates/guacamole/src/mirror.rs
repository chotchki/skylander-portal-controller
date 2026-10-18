//! `/api/_test/tv` (design decision 8): a `PostUpdate` copy of what the TV
//! draws, read off the laid-out UI tree, so the e2e suite asserts on the TV
//! without screenshots. Mounted only under `test-hooks`.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform, UiSystems};
use serde::Serialize;

use crate::cards::PortalCards;
use crate::screen::TvScreen;
use crate::tv::{
    CardSlot, Current, EmptySlot, Framing, Pip, ServerLink, Setup, contains, join_address,
    node_rect, safe_area,
};

/// What the TV shows this frame.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TvSnapshot {
    /// `TvScreen`'s variant: `Join`, `Portal`, `NoGiants`, ...
    pub screen: String,
    pub game: Option<String>,
    /// ServerError's message.
    pub message: Option<String>,
    pub players: usize,
    pub join_address: String,
    /// Setup's lists, for Setup, NoGiants and Found.
    pub found: Vec<String>,
    pub folders: Vec<String>,
    pub problems: Vec<String>,
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
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CardShot {
    /// 1-based, as the phone numbers slots.
    pub slot: u8,
    pub figure_id: String,
    /// Every line drawn on the card outside the pip, top to bottom.
    pub texts: Vec<String>,
    /// An image drawn in the portrait frame.
    pub portrait: bool,
    pub pip: Option<PipShot>,
    /// x, y, width, height in physical pixels.
    pub rect: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PipShot {
    pub initial: String,
    /// The pip's fill as drawn, `#RRGGBB`.
    pub color: String,
    pub profile_id: String,
}

/// Shared with the route.
#[derive(Resource, Clone, Default)]
pub struct TvMirror(pub Arc<Mutex<TvSnapshot>>);

pub struct Mirror;

impl Plugin for Mirror {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, mirror_tv.after(UiSystems::Layout));
    }
}

/// Serves whatever the last frame wrote; generic so a test needs no `AppState`.
pub fn routes<S: Clone + Send + Sync + 'static>(mirror: &TvMirror) -> axum::Router<S> {
    let shared = mirror.0.clone();
    axum::Router::new().route(
        "/api/_test/tv",
        axum::routing::get(move || {
            let shared = shared.clone();
            async move { axum::Json(shared.lock().unwrap_or_else(|e| e.into_inner()).clone()) }
        }),
    )
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
    link: Res<ServerLink>,
    setup: Res<Setup>,
    framing: Res<Framing>,
    portal: Res<PortalCards>,
    camera: Query<&Camera, With<Camera2d>>,
    cards: Query<(Entity, &CardSlot, &ComputedNode, &UiGlobalTransform)>,
    empties: Query<&EmptySlot>,
    children: Query<&Children>,
    texts: Query<&Text>,
    images: Query<(), With<ImageNode>>,
    pips: Query<(&Pip, &BackgroundColor)>,
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
    let snapshot = TvSnapshot {
        screen: current.screen.kind().to_string(),
        game,
        message,
        players: current.clients,
        join_address: join_address(&link.phone_url).to_string(),
        found: setup.0.found_titles.clone(),
        folders: setup.0.folders.clone(),
        problems: setup.0.problems.clone(),
        cards: drawn.into_iter().map(|(c, _)| c).collect(),
        empty_slots,
        window: [size.x as f32, size.y as f32],
        cards_outside,
        overlapping,
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
        assert_eq!(shot.players, 2);
        assert_eq!(
            shot.join_address,
            "http://christophers-macbook-pro.local:8765"
        );
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

    #[tokio::test]
    async fn the_route_serves_the_last_snapshot() {
        use tower::ServiceExt;
        let mirror = TvMirror::default();
        mirror.0.lock().unwrap().screen = "NoGiants".into();
        let app = routes::<()>(&mirror);
        let response = app
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
    }
}
