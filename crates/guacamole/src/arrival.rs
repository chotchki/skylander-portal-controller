//! The arrival on the TV (openspec AG.72; level-scene's A character coming in
//! arrives; stand-in ledger: `arrival`): ours in place of the original's
//! arrival screen, in the TV's own look, until AM's menus.
//!
//! - What asks one ([`crate::play`]): a character becoming the hero after the
//!   level has started (a replacement, or the one the too-many alert held
//!   coming in as the hero comes off), and a magic item or a sidekick placed
//!   while the level has started, alone of its kind (a second holds under
//!   the too-many-items alert instead). None at the first entry: his intro
//!   stands in for it.
//! - When ([`Arrivals`]): each in the order asked, the level held from the
//!   ask; a lead-in of [`LEAD`] (the original's element burst in the level
//!   comes first: its arrival is on screen by 1.4-1.5 s, AG.6), then the
//!   card for [`SHOWN`], then the level again 5.2 s after what brought it in
//!   (AG.6: 5.4-8.0 s, the pose about 3 s). One whose figure isn't on the
//!   portal any more by its turn is dropped, and a card whose figure comes
//!   off ends there. A too-many or wrong-toy alert holds the card's clock
//!   and hides it; the call for a Skylander and the needs-rest alert don't
//!   (an item placed under the call plays its arrival, then the call is
//!   back: AH.52's `cap_off`).
//! - What it shows ([`spawn_card`]): the figure alone over its element's
//!   colours, its card's bezel at [`BEZEL_SCALE`] holding its turntable's
//!   image (its card's model turning, the card camera kept drawing while it
//!   shows) or, with none, its portrait; its name in gold and its element
//!   under it; a white flash over the last [`FLASH`]. Every surface is the
//!   card's own (its bezel's gradients, borders and shadows, text), so its
//!   pipelines are the ones the start's pass compiled for a card (AE.25).

use std::collections::VecDeque;
use std::time::Duration;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ComputedUiTargetCamera, UiGlobalTransform};
use serde::Serialize;
use skylander_core::{Element, SlotIndex};
use skylander_rpcs3_control::Placement;
use tracing::info;

use crate::card_view::element_label;
use crate::level::OverLevel;
use crate::pause::ShownAt;
use crate::play::{Instance, Play, PlaySystems, instance};
use crate::theme::{Role, Step, TvTheme};
use crate::turntable::Turntables;
use crate::tv::Portraits;
use crate::tv_state::{Pen, Redrawn, TvAssets, UiTree, node_rect};

/// From what brings an arrival in to its card (AG.6: the burst by 0.5-0.9
/// s, the arrival on screen by 1.4-1.5 s).
pub(crate) const LEAD: Duration = Duration::from_millis(1200);

/// The card on screen (AG.6: the pose about 3 s, 2.7-4.0 s; a flash, then
/// the level by 5.4-8.0 s after the placement): its pose and its flash.
pub(crate) const SHOWN: Duration = Duration::from_millis(4000);

/// The white flash over the card's last part.
pub(crate) const FLASH: Duration = Duration::from_millis(250);

/// Over an alert (940), under the corner's join code (960).
pub(crate) const ARRIVAL_Z: i32 = 950;

/// The card's bezel over a portal card's.
const BEZEL_SCALE: f32 = 2.5;

/// Whose arrival: a hero's comes before he does (his entry waits on it),
/// an item's leaves the hero as he was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    /// A character becoming the hero.
    Hero,
    /// A magic item or a sidekick.
    Item,
}

/// An arrival asked for: the slot and the figure in it then, what it is and
/// when it was asked, on `Time<Real>`'s clock.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Asked {
    pub(crate) slot: SlotIndex,
    pub(crate) figure: Instance,
    pub(crate) placement: Placement,
    pub(crate) kind: Kind,
    pub(crate) at: Duration,
}

/// The card on screen: its arrival, when it first showed and how long it
/// has shown (its clock held while a blocking alert hides it).
#[derive(Debug, Clone, PartialEq)]
struct Showing {
    asked: Asked,
    since: Duration,
    shown: Duration,
    last: Duration,
}

/// An arrival done, for `/api/_test/tv`: when it was asked, first shown and
/// ended (none for one dropped before its turn), on `Time<Real>`'s clock.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Done {
    pub(crate) slot: u8,
    pub(crate) figure: String,
    pub(crate) kind: Kind,
    pub(crate) asked_ms: f64,
    pub(crate) shown_ms: Option<f64>,
    pub(crate) ended_ms: f64,
    /// `shown`, `dropped` (its figure off by its turn) or `cut` (off while
    /// it showed).
    pub(crate) how: &'static str,
}

/// The arrivals: those waiting their turn, the one on screen and the last
/// few done.
#[derive(Debug, Default, Clone)]
pub(crate) struct Arrivals {
    queue: VecDeque<Asked>,
    showing: Option<Showing>,
    done: VecDeque<Done>,
}

/// How many done arrivals `/api/_test/tv` keeps.
const DONE_KEPT: usize = 8;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

impl Arrivals {
    /// `asked` waits its turn; one already waiting or showing for the same
    /// slot and figure isn't asked twice.
    pub(crate) fn ask(&mut self, asked: Asked) {
        let same = |a: &Asked| a.slot == asked.slot && a.figure == asked.figure;
        if self.queue.iter().any(same) || self.showing.as_ref().is_some_and(|s| same(&s.asked)) {
            return;
        }
        info!(
            slot = asked.slot.display(),
            figure = %asked.placement.name,
            kind = ?asked.kind,
            "an arrival asked"
        );
        self.queue.push_back(asked);
    }

    /// One frame at `now`: the card on screen ends after [`SHOWN`] of
    /// showing, or as its figure comes off (`on` false for its slot and
    /// figure); with none on screen and no blocking alert up, the next whose
    /// figure is still on and whose lead-in is over shows, those whose
    /// figure came off dropped. `blocked`: a too-many or wrong-toy alert is
    /// up, which hides the card and holds its clock. True when what's on
    /// screen changed.
    pub(crate) fn tick(
        &mut self,
        now: Duration,
        blocked: bool,
        on: impl Fn(SlotIndex, &Instance) -> bool,
    ) -> bool {
        let mut changed = false;
        if let Some(s) = self.showing.as_mut() {
            if !blocked {
                s.shown += now.saturating_sub(s.last);
            }
            s.last = now;
            let off = !on(s.asked.slot, &s.asked.figure);
            if off || s.shown >= SHOWN {
                let s = self.showing.take().expect("matched above");
                self.finish(
                    &s.asked,
                    Some(s.since),
                    now,
                    if off { "cut" } else { "shown" },
                );
                changed = true;
            }
        }
        if self.showing.is_some() || blocked {
            return changed;
        }
        while let Some(front) = self.queue.front() {
            if on(front.slot, &front.figure) {
                break;
            }
            let gone = self.queue.pop_front().expect("matched above");
            info!(slot = gone.slot.display(), figure = %gone.placement.name, "an arrival dropped: its figure left the portal before its turn");
            self.finish(&gone, None, now, "dropped");
            changed = true;
        }
        if let Some(asked) = self.queue.pop_front_if(|f| now >= f.at + LEAD) {
            info!(
                slot = asked.slot.display(),
                figure = %asked.placement.name,
                after_ms = ms(now.saturating_sub(asked.at)),
                "an arrival shows"
            );
            self.showing = Some(Showing {
                asked,
                since: now,
                shown: Duration::ZERO,
                last: now,
            });
            changed = true;
        }
        changed
    }

    fn finish(&mut self, asked: &Asked, since: Option<Duration>, now: Duration, how: &'static str) {
        if how != "dropped" {
            info!(
                slot = asked.slot.display(),
                figure = %asked.placement.name,
                how,
                after_ms = ms(now.saturating_sub(asked.at)),
                "an arrival ends"
            );
        }
        self.done.push_back(Done {
            slot: asked.slot.as_u8() + 1,
            figure: asked.placement.name.clone(),
            kind: asked.kind,
            asked_ms: ms(asked.at),
            shown_ms: since.map(ms),
            ended_ms: ms(now),
            how,
        });
        while self.done.len() > DONE_KEPT {
            self.done.pop_front();
        }
    }

    /// The level holds for it: an arrival waiting or on screen.
    pub(crate) fn holding(&self) -> bool {
        self.showing.is_some() || !self.queue.is_empty()
    }

    /// The arrival on screen: none during a lead-in or between arrivals.
    pub(crate) fn showing(&self) -> Option<&Asked> {
        self.showing.as_ref().map(|s| &s.asked)
    }

    /// How far into its time on screen the card is, 0 to 1.
    pub(crate) fn progress(&self) -> Option<f32> {
        self.showing
            .as_ref()
            .map(|s| (s.shown.as_secs_f32() / SHOWN.as_secs_f32()).clamp(0.0, 1.0))
    }

    /// Those done, oldest first.
    pub(crate) fn done(&self) -> impl Iterator<Item = &Done> {
        self.done.iter()
    }

    /// Those asked and not shown yet (in their lead-in, or behind the one on
    /// screen).
    pub(crate) fn waiting(&self) -> usize {
        self.queue.len()
    }

    /// Every arrival forgotten: the level ended.
    pub(crate) fn clear(&mut self) {
        self.queue.clear();
        self.showing = None;
    }
}

/// An arrival asked for `placement` in `slot` at `at`.
pub(crate) fn asked(slot: SlotIndex, placement: &Placement, kind: Kind, at: Duration) -> Asked {
    Asked {
        slot,
        figure: instance(placement),
        placement: placement.clone(),
        kind,
        at,
    }
}

/// The card drawn, its root.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArrivalScreen;

/// The plate the card's figure shows on: its slot and its portrait, the
/// turntable's image over it once that's up.
#[derive(Component, Debug, Clone)]
pub(crate) struct ArrivalPlate {
    slot: u8,
    portrait: Option<Handle<Image>>,
}

/// The flash over the card's end.
#[derive(Component, Debug)]
pub(crate) struct ArrivalFlash;

pub(crate) struct ArrivalPlugin;

impl Plugin for ArrivalPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (show_arrival, show_figure, flash)
                .chain()
                .after(PlaySystems),
        );
    }
}

/// Spawns the card the arrival on screen asks for and despawns one no
/// longer asked, telling the shots ([`Redrawn::Alert`]). Needs the TV's
/// font and theme; the headless tests without them draw nothing.
fn show_arrival(
    mut commands: Commands,
    play: Option<Res<Play>>,
    (assets, theme, time): (Option<Res<TvAssets>>, Option<Res<TvTheme>>, Res<Time<Real>>),
    mut portraits: Option<ResMut<Portraits>>,
    mut images: ResMut<Assets<Image>>,
    mut redrawn: MessageWriter<Redrawn>,
    mut shown: Local<Option<((SlotIndex, Instance), Entity)>>,
) {
    let want = play
        .as_deref()
        .filter(|p| !p.arrival_hidden())
        .and_then(|p| p.arrivals().showing())
        .cloned();
    let key = want.as_ref().map(|a| (a.slot, a.figure.clone()));
    if shown.as_ref().map(|(k, _)| k) == key.as_ref() {
        return;
    }
    if let Some((_, entity)) = shown.take()
        && let Ok(mut card) = commands.get_entity(entity)
    {
        card.despawn();
        redrawn.write(Redrawn::Alert);
    }
    let (Some(want), Some(assets), Some(theme)) = (want, assets, theme) else {
        return;
    };
    let pen = Pen::new(&assets, &theme);
    let portrait = want
        .placement
        .portrait
        .as_deref()
        .zip(portraits.as_deref_mut())
        .and_then(|(path, p)| p.get(std::path::Path::new(path), &mut images));
    let entity = spawn_card(&mut commands, &pen, &want, portrait);
    commands.entity(entity).insert(ShownAt(time.elapsed()));
    *shown = Some(((want.slot, want.figure.clone()), entity));
    redrawn.write(Redrawn::Alert);
}

/// The card full screen: the figure's element's colours, its bezel holding
/// its figure, its name and its element.
fn spawn_card(
    commands: &mut Commands,
    pen: &Pen,
    asked: &Asked,
    portrait: Option<Handle<Image>>,
) -> Entity {
    let theme = &pen.theme;
    let element = asked.placement.element;
    let backdrop = element_backdrop(theme, element);
    commands
        .spawn((
            ArrivalScreen,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(48),
                ..default()
            },
            backdrop,
            GlobalZIndex(ARRIVAL_Z),
        ))
        .with_children(|root| {
            // The bezel laid out at the card's size, scaled up round its
            // centre, with room made for it.
            root.spawn(Node {
                width: px(crate::surfaces::BEZEL * BEZEL_SCALE),
                height: px(crate::surfaces::BEZEL * BEZEL_SCALE),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|room| {
                let (ring, plate) =
                    crate::surfaces::spawn_bezel(room, theme, element, portrait.clone());
                room.commands()
                    .entity(ring)
                    .insert(UiTransform::from_scale(Vec2::splat(BEZEL_SCALE)));
                room.commands().entity(plate).insert(ArrivalPlate {
                    slot: asked.slot.as_u8(),
                    portrait,
                });
            });
            root.spawn(pen.text(&asked.placement.name, Step::Hero, Role::Accent));
            if let Some(element) = element {
                root.spawn(pen.text(element_label(element), Step::Md, Role::Primary));
            }
            root.spawn((
                ArrivalFlash,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::WHITE.with_alpha(0.0)),
            ));
        })
        .id()
}

/// The figure's element's two stops on the diagonal, as its ring wears
/// them (the card's own gradient, so its pipeline is the one the start's
/// pass compiled for a card); the plain plate's for a figure with none.
fn element_backdrop(theme: &TvTheme, element: Option<Element>) -> BackgroundGradient {
    let stops = theme.element(element).unwrap_or(theme.plate);
    BackgroundGradient(vec![crate::surfaces::diagonal(stops)])
}

/// The card's plate shows its figure's turntable once that's up, its
/// portrait until then (or for good, an item's).
fn show_figure(
    mut commands: Commands,
    turntables: Option<Res<Turntables>>,
    plates: Query<(Entity, &ArrivalPlate, Option<&ImageNode>)>,
) {
    for (entity, plate, node) in &plates {
        let wanted = turntables
            .as_deref()
            .and_then(|t| t.image(plate.slot))
            .or_else(|| plate.portrait.clone());
        if node.map(|n| n.image.id()) == wanted.as_ref().map(Handle::id) {
            continue;
        }
        match wanted {
            Some(image) => commands.entity(entity).insert(ImageNode::new(image)),
            None => commands.entity(entity).remove::<ImageNode>(),
        };
    }
}

/// The flash rising over the card's last [`FLASH`].
fn flash(play: Option<Res<Play>>, mut flashes: Query<&mut BackgroundColor, With<ArrivalFlash>>) {
    let Some(progress) = play.as_deref().and_then(|p| p.arrivals().progress()) else {
        return;
    };
    let from = 1.0 - FLASH.as_secs_f32() / SHOWN.as_secs_f32();
    let alpha = ((progress - from) / (1.0 - from)).clamp(0.0, 1.0);
    for mut fill in &mut flashes {
        let want = Color::WHITE.with_alpha(alpha);
        if fill.0 != want {
            fill.0 = want;
        }
    }
}

/// The arrival as drawn, and those done.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArrivalShot {
    /// The card on screen: its figure's slot (1-based), name and kind,
    /// what it says top to bottom, whether it covers the window and draws
    /// over the level, whether its figure shows as its model (`model`) or
    /// portrait (`portrait`), and when it was spawned on the frame log's
    /// clock.
    pub showing: Option<CardShot>,
    /// How many wait their turn.
    pub waiting: usize,
    /// The last few done, oldest first.
    pub done: Vec<DoneShot>,
    /// The frame log's clock now.
    pub now_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CardShot {
    pub slot: u8,
    pub figure: String,
    pub kind: String,
    pub texts: Vec<String>,
    pub full_screen: bool,
    pub over_level: bool,
    pub image: Option<String>,
    pub shown_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DoneShot {
    pub slot: u8,
    pub figure: String,
    pub kind: String,
    pub asked_ms: f64,
    pub shown_ms: Option<f64>,
    pub ended_ms: f64,
    pub how: String,
}

/// The arrival's part of `/api/_test/tv` (openspec AQ.8).
#[derive(SystemParam)]
pub(crate) struct ArrivalShots<'w, 's> {
    play: Option<Res<'w, Play>>,
    cards: Query<
        'w,
        's,
        (
            Entity,
            Option<&'static ShownAt>,
            &'static ComputedNode,
            &'static UiGlobalTransform,
            Option<&'static ComputedUiTargetCamera>,
        ),
        With<ArrivalScreen>,
    >,
    plates: Query<'w, 's, (&'static ArrivalPlate, Option<&'static ImageNode>)>,
    turntables: Option<Res<'w, Turntables>>,
    over_level: OverLevel<'w, 's>,
    tree: UiTree<'w, 's>,
    time: Res<'w, Time<Real>>,
}

impl ArrivalShots<'_, '_> {
    /// The arrival, its cover measured against `window`; none before the
    /// walk's state exists.
    pub(crate) fn shot(&self, window: Rect) -> Option<ArrivalShot> {
        let play = self.play.as_deref()?;
        let arrivals = play.arrivals();
        let showing = arrivals.showing().and_then(|a| {
            let (root, at, node, place, target) = self.cards.iter().next()?;
            let found = self.tree.walk(root);
            let r = node_rect(node, place);
            let image = self.plates.iter().next().and_then(|(plate, image)| {
                let image = image?;
                let model = self.turntables.as_deref().and_then(|t| t.image(plate.slot));
                Some(match model {
                    Some(m) if m.id() == image.image.id() => "model",
                    _ => "portrait",
                })
            });
            Some(CardShot {
                slot: a.slot.as_u8() + 1,
                figure: a.placement.name.clone(),
                kind: format!("{:?}", a.kind).to_lowercase(),
                texts: found.0,
                full_screen: (r.min - window.min).abs().max_element() <= 1.0
                    && (r.max - window.max).abs().max_element() <= 1.0,
                over_level: self.over_level.over(target),
                image: image.map(str::to_owned),
                shown_ms: at.map(|a| ms(a.0)),
            })
        });
        Some(ArrivalShot {
            showing,
            waiting: arrivals.waiting(),
            done: arrivals
                .done()
                .map(|d| DoneShot {
                    slot: d.slot,
                    figure: d.figure.clone(),
                    kind: format!("{:?}", d.kind).to_lowercase(),
                    asked_ms: d.asked_ms,
                    shown_ms: d.shown_ms,
                    ended_ms: d.ended_ms,
                    how: d.how.to_owned(),
                })
                .collect(),
            now_ms: ms(self.time.elapsed()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(id: &str, name: &str) -> Placement {
        let figure_id = skylander_core::FigureId::new(id);
        let (toy_type, variant) = figure_id.subtype_parts().unwrap();
        Placement {
            figure_id,
            toy_type,
            variant,
            name: name.into(),
            element: Some(Element::Life),
            portrait: None,
            owner: None,
        }
    }

    fn slot(n: u8) -> SlotIndex {
        SlotIndex::new(n).unwrap()
    }

    fn s(secs: f32) -> Duration {
        Duration::from_secs_f32(secs)
    }

    /// AG.72: one asked holds the level from the ask, shows after its
    /// lead-in and for [`SHOWN`], then ends; the next waits its turn and
    /// shows straight after.
    #[test]
    fn an_arrival_shows_after_its_lead_in_for_its_time_then_the_next() {
        let tree = placement("000014-0000", "Tree Rex");
        let shield = placement("000210-0000", "Sky-Iron Shield");
        let mut a = Arrivals::default();
        a.ask(asked(slot(1), &tree, Kind::Hero, s(10.0)));
        a.ask(asked(slot(1), &tree, Kind::Hero, s(10.1)));
        a.ask(asked(slot(2), &shield, Kind::Item, s(10.5)));
        assert_eq!(a.waiting(), 2, "the same arrival asked once");
        assert!(a.holding());
        let on = |_: SlotIndex, _: &Instance| true;
        assert!(!a.tick(s(11.0), false, on), "in its lead-in");
        assert!(a.showing().is_none());
        assert!(a.tick(s(11.25), false, on));
        assert_eq!(a.showing().map(|x| x.kind), Some(Kind::Hero));
        assert!(!a.tick(s(15.0), false, on));
        assert!(a.tick(s(15.3), false, on), "its time up");
        assert_eq!(
            a.showing().map(|x| x.kind),
            Some(Kind::Item),
            "the next at once"
        );
        assert!(a.tick(s(19.4), false, on));
        assert!(a.showing().is_none());
        assert!(!a.holding());
        let done: Vec<_> = a.done().map(|d| (d.kind, d.how)).collect();
        assert_eq!(done, vec![(Kind::Hero, "shown"), (Kind::Item, "shown")]);
    }

    /// AG.72: an arrival whose figure isn't on the portal any more by its
    /// turn is dropped, and one whose figure comes off while it shows ends
    /// there.
    #[test]
    fn a_figure_gone_by_its_turn_has_no_arrival() {
        let tree = placement("000014-0000", "Tree Rex");
        let shield = placement("000210-0000", "Sky-Iron Shield");
        let mut a = Arrivals::default();
        a.ask(asked(slot(1), &tree, Kind::Hero, s(0.0)));
        a.ask(asked(slot(2), &shield, Kind::Item, s(0.0)));
        let only_shield = |at: SlotIndex, _: &Instance| at == slot(2);
        assert!(a.tick(s(2.0), false, only_shield));
        assert_eq!(a.showing().map(|x| x.kind), Some(Kind::Item));
        let none = |_: SlotIndex, _: &Instance| false;
        assert!(a.tick(s(3.0), false, none));
        assert!(a.showing().is_none());
        let done: Vec<_> = a.done().map(|d| (d.kind, d.how)).collect();
        assert_eq!(done, vec![(Kind::Hero, "dropped"), (Kind::Item, "cut")]);
        // The same figure back in its slot is no longer the one asked for:
        // another profile's placement of it.
        let mut b = Arrivals::default();
        b.ask(asked(slot(1), &tree, Kind::Hero, s(0.0)));
        let other = (
            Some("someone".to_owned()),
            tree.figure_id.as_str().to_owned(),
        );
        let theirs = |_: SlotIndex, i: &Instance| *i == other;
        b.tick(s(2.0), false, theirs);
        assert!(b.showing().is_none());
    }

    /// AG.72: the card's backdrop is its element's diagonal as a card's
    /// bezel wears it, in the colour space the card's gradients interpolate
    /// in (the TV's first run compiled a gradient pipeline as the card came
    /// up, its backdrop in Bevy's default space), the plain plate's for an
    /// item with no element.
    #[test]
    fn the_cards_backdrop_is_a_cards_own_gradient() {
        let theme = TvTheme::default();
        for element in [Some(Element::Life), Some(Element::Fire), None] {
            let stops = theme.element(element).unwrap_or(theme.plate);
            let backdrop = element_backdrop(&theme, element);
            assert_eq!(backdrop.0, vec![crate::surfaces::diagonal(stops)]);
            let bevy::ui::Gradient::Linear(linear) = &backdrop.0[0] else {
                panic!("{:?}", backdrop.0)
            };
            assert_eq!(linear.color_space, bevy::ui::InterpolationColorSpace::Srgba);
        }
    }

    /// AG.72: a too-many alert hides the card and holds its clock; the
    /// call for a Skylander doesn't block one waiting (the caller's rule).
    #[test]
    fn a_blocking_alert_holds_the_cards_clock() {
        let tree = placement("000014-0000", "Tree Rex");
        let mut a = Arrivals::default();
        a.ask(asked(slot(1), &tree, Kind::Hero, s(0.0)));
        let on = |_: SlotIndex, _: &Instance| true;
        assert!(!a.tick(s(2.0), true, on), "blocked: not shown");
        assert!(a.tick(s(2.1), false, on));
        a.tick(s(3.0), false, on);
        a.tick(s(10.0), true, on);
        assert!(a.showing().is_some(), "its clock held under the alert");
        a.tick(s(13.0), false, on);
        assert!(
            a.showing().is_some(),
            "0.9 s, then 3 s more: under its time"
        );
        a.tick(s(13.2), false, on);
        assert!(a.showing().is_none());
    }
}
