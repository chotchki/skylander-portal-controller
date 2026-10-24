//! The level waiting (openspec AG.20, design decision 12; pad-input's Start
//! pauses the level and One pad walks the hero): the clock and the two
//! overlays, in place of the original's pause menu and controller-loss
//! screen until AM's menus (stand-in ledger: `pause`).
//!
//! - While the level waits, `Time<Virtual>` stops, so no fixed step runs (the
//!   hero and the camera stand), the hero's drawn transform holds where the
//!   interpolation left it and the level's scrolls hold. The frame the level
//!   comes to wait still runs the steps already due, on the pad as it was
//!   the frame before ([`crate::pad::Seat::step`]).
//! - Start's overlay names the one button that resumes it, the bottom one,
//!   as printed on the pad that paused ([`crate::pad::prompt`]); the
//!   controller message asks for a pad and a press.
//!
//! Both are TV UI over everything but an intro, and the TV's UI draws over
//! the level (AG.50, [`crate::level::tv_over_level`]): the level's camera
//! comes first on the TV's target and the TV's lays its UI over it, so the
//! level shows dimmed under the overlay, held where it stopped. Under the
//! level's camera, as AG.21 left them, the controller message never showed
//! and a held pose read as a freeze (AG.51).

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::Serialize;

use crate::level::OverLevel;
use crate::pad::{Face, FacesPrinted, Labels, LevelPads, Pause, Prompt, labels, prompt};
use crate::theme::{Role, Step, TvTheme};
use crate::tv::{Pen, Redrawn, TvAssets, UiTree};

/// Under an intro's overlay (1,000) and the portal's alerts, over everything
/// else.
pub(crate) const OVERLAY_Z: i32 = 900;

/// The prompt glyph's side, and a position dot's, in design px.
const GLYPH: f32 = 56.0;
const DOT: f32 = 18.0;
const GLYPH_BORDER: f32 = 4.0;

/// What the level shows while it waits.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LevelOverlay {
    /// Start's: the button that resumes, named.
    Paused(Prompt),
    /// No pad walks the hero.
    Controller,
}

impl LevelOverlay {
    pub(crate) fn name(self) -> &'static str {
        match self {
            LevelOverlay::Paused(_) => "paused",
            LevelOverlay::Controller => "controller",
        }
    }
}

/// A drawn prompt, for the mirror.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PromptGlyph(pub(crate) Prompt);

/// When an overlay was spawned, on `Time<Real>`'s clock (the frame log's),
/// for the mirror.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShownAt(pub(crate) std::time::Duration);

/// The overlay the level's state asks for: Start's names the resume button
/// as printed on the pad that paused, by `labels_of`.
pub(crate) fn wanted(
    pads: &LevelPads,
    labels_of: impl Fn(Entity) -> Labels,
) -> Option<LevelOverlay> {
    match pads.seat()?.pause()? {
        Pause::Start { by } => Some(LevelOverlay::Paused(prompt(labels_of(by), Face::South))),
        Pause::NoPad => Some(LevelOverlay::Controller),
    }
}

/// Spawns the overlay the level's state asks for and despawns one it no
/// longer does, telling the shots ([`Redrawn::Overlay`]). Needs the TV's
/// font and theme; the headless tests without them draw nothing.
pub(crate) fn show_overlay(
    mut commands: Commands,
    pads: Res<LevelPads>,
    gamepads: Query<(&Gamepad, Option<&FacesPrinted>)>,
    (assets, theme, time): (Option<Res<TvAssets>>, Option<Res<TvTheme>>, Res<Time<Real>>),
    mut redrawn: MessageWriter<Redrawn>,
    mut shown: Local<Option<(LevelOverlay, Entity)>>,
) {
    let want = wanted(&pads, |pad| {
        gamepads
            .get(pad)
            .map_or(Labels::Position, |(pad, printed)| labels(pad, printed))
    });
    if shown.map(|(o, _)| o) == want {
        return;
    }
    if let Some((_, entity)) = shown.take()
        && let Ok(mut overlay) = commands.get_entity(entity)
    {
        overlay.despawn();
        redrawn.write(Redrawn::Overlay);
    }
    let (Some(want), Some(assets), Some(theme)) = (want, assets, theme) else {
        return;
    };
    let pen = Pen::new(&assets, &theme);
    let entity = spawn(&mut commands, &pen, want);
    commands.entity(entity).insert(ShownAt(time.elapsed()));
    *shown = Some((want, entity));
    redrawn.write(Redrawn::Overlay);
}

fn spawn(commands: &mut Commands, pen: &Pen, overlay: LevelOverlay) -> Entity {
    let root = commands
        .spawn((
            overlay,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(32),
                ..default()
            },
            BackgroundColor(pen.theme.shadow.with_alpha(0.6)),
            GlobalZIndex(OVERLAY_Z),
        ))
        .id();
    commands.entity(root).with_children(|parent| match overlay {
        LevelOverlay::Paused(resume) => {
            parent.spawn(pen.text("Paused", Step::Lg, Role::Accent));
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(16),
                    ..default()
                })
                .with_children(|line| {
                    line.spawn(pen.text("Press", Step::Body, Role::Primary));
                    glyph(line, pen, resume);
                    line.spawn(pen.text("to play on", Step::Body, Role::Primary));
                });
        }
        LevelOverlay::Controller => {
            parent.spawn(pen.text("Connect a controller", Step::Lg, Role::Accent));
            parent.spawn(pen.text("then press any button on it", Step::Body, Role::Primary));
        }
    });
    root
}

/// A letter in a gold ring, or for a pad whose labels aren't known, four
/// dots in a diamond with the button's own filled.
fn glyph(parent: &mut ChildSpawnerCommands, pen: &Pen, prompt: Prompt) {
    let theme = &pen.theme;
    let ring = Node {
        width: px(GLYPH),
        height: px(GLYPH),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    match prompt {
        Prompt::Letter(letter) => {
            parent
                .spawn((
                    PromptGlyph(prompt),
                    Node {
                        border: UiRect::all(px(GLYPH_BORDER)),
                        border_radius: BorderRadius::MAX,
                        ..ring
                    },
                    BorderColor::all(theme.gold),
                    BackgroundColor(theme.panel[1]),
                ))
                .with_child(pen.text(letter, Step::Body, Role::Accent));
        }
        Prompt::Position(face) => {
            parent
                .spawn((PromptGlyph(prompt), ring))
                .with_children(|diamond| {
                    for dot in Face::ALL {
                        let (left, top) = dot_at(dot);
                        let (fill, edge) = if dot == face {
                            (theme.gold_bright, theme.gold)
                        } else {
                            (Color::NONE, theme.role(Role::Muted))
                        };
                        diamond.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(left),
                                top: px(top),
                                width: px(DOT),
                                height: px(DOT),
                                border: UiRect::all(px(3)),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BorderColor::all(edge),
                            BackgroundColor(fill),
                        ));
                    }
                });
        }
    }
}

/// A dot's top-left corner in the glyph: the four sides' middles.
fn dot_at(face: Face) -> (f32, f32) {
    let middle = (GLYPH - DOT) / 2.0;
    let far = GLYPH - DOT;
    match face {
        Face::South => (middle, far),
        Face::East => (far, middle),
        Face::West => (0.0, middle),
        Face::North => (middle, 0.0),
    }
}

/// Stops the level's clock while it waits or the portal holds it, and
/// starts it again after; a pause it didn't make it leaves alone.
pub(crate) fn hold_time(
    pads: Res<LevelPads>,
    mut time: ResMut<Time<Virtual>>,
    mut held: Local<bool>,
) {
    let wait = pads.waiting();
    if wait && !*held {
        time.pause();
        *held = true;
    } else if !wait && *held {
        time.unpause();
        *held = false;
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OverlayShot {
    /// `paused` or `controller`.
    pub kind: String,
    /// Every line drawn on it, top to bottom.
    pub texts: Vec<String>,
    /// The drawn prompt: `A`, `B` ... or `position:south`.
    pub prompt: Option<String>,
    /// It draws over the level's camera (AG.50): its camera after the
    /// level's on one target, cleared to no colour and laid over
    /// ([`crate::level::draws_over`]); false with no level camera.
    pub over_level: bool,
    /// When it was spawned, on the frame log's clock (`/api/_test/tv/frames`).
    pub shown_ms: Option<f64>,
}

/// The overlays as the mirror reads them: each one's root, when it was
/// spawned and the camera its UI draws through.
pub(crate) type Overlays<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static LevelOverlay,
        Option<&'static ShownAt>,
        Option<&'static ComputedUiTargetCamera>,
    ),
>;

/// The level's overlay as drawn, its part of `/api/_test/tv` (openspec
/// AQ.8): its lines, its prompt and whether it draws over the level.
#[derive(SystemParam)]
pub(crate) struct OverlayShots<'w, 's> {
    overlays: Overlays<'w, 's>,
    glyphs: Query<'w, 's, &'static PromptGlyph>,
    over_level: OverLevel<'w, 's>,
    tree: UiTree<'w, 's>,
}

impl OverlayShots<'_, '_> {
    pub(crate) fn shot(&self) -> Option<OverlayShot> {
        self.overlays.iter().next().map(|(root, kind, at, target)| {
            let found = self.tree.walk(root);
            OverlayShot {
                kind: kind.name().to_owned(),
                texts: found.0,
                prompt: self.glyphs.iter().next().map(|g| g.0.name()),
                over_level: self.over_level.over(target),
                shown_ms: at.map(|a| a.0.as_secs_f64() * 1e3),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::gamepad::GamepadButton;

    use super::*;
    use crate::pad::PadFrame;

    /// The level shown, `pads` connected, the first walking; `pressing` on
    /// pad `by` this frame.
    fn level(pads: &[Entity], by: Entity, pressing: GamepadButton) -> LevelPads {
        let mut level = LevelPads::default();
        level.show();
        let frame = |id: Entity, pressed: Vec<GamepadButton>| PadFrame {
            id,
            stick: Vec2::ZERO,
            pressed,
            held: [false; 4],
            shoulders: [false; 4],
        };
        let seat = level.seat_mut().unwrap();
        seat.step(
            &pads
                .iter()
                .map(|p| frame(*p, Vec::new()))
                .collect::<Vec<_>>(),
        );
        seat.step(
            &pads
                .iter()
                .map(|p| frame(*p, if *p == by { vec![pressing] } else { Vec::new() }))
                .collect::<Vec<_>>(),
        );
        level
    }

    /// pad-input's The pause overlay on each 8BitDo, by the labels the
    /// resolver gives the pad that paused: "A" on the PC layout, "B" on the
    /// Switch one, the bottom position on a pad it can't place; the
    /// controller message once no pad walks.
    #[test]
    fn the_overlay_names_the_bottom_button_as_printed_on_the_pad_that_paused() {
        let mut world = World::new();
        let [pc, switch, other] = [(); 3].map(|_| world.spawn_empty().id());
        let pads = [pc, switch, other];
        let labels_of = |pad: Entity| match pad {
            p if p == pc => Labels::Xbox,
            p if p == switch => Labels::Nintendo,
            _ => Labels::Position,
        };
        for (by, want) in [
            (pc, Prompt::Letter("A")),
            (switch, Prompt::Letter("B")),
            (other, Prompt::Position(Face::South)),
        ] {
            let level = level(&pads, by, GamepadButton::Start);
            assert_eq!(wanted(&level, labels_of), Some(LevelOverlay::Paused(want)));
        }
        let running = level(&pads, pc, GamepadButton::Select);
        assert_eq!(wanted(&running, labels_of), None);
        let mut lost = level(&pads, pc, GamepadButton::Select);
        lost.seat_mut().unwrap().step(&[]);
        assert_eq!(wanted(&lost, labels_of), Some(LevelOverlay::Controller));
        assert_eq!(wanted(&LevelPads::default(), labels_of), None, "no level");
    }

    #[test]
    fn the_dots_sit_on_the_four_sides() {
        let at: Vec<_> = Face::ALL.map(dot_at).to_vec();
        assert_eq!(at[0], (19.0, 38.0), "bottom");
        assert_eq!(at[1], (38.0, 19.0), "right");
        assert_eq!(at[2], (0.0, 19.0), "left");
        assert_eq!(at[3], (19.0, 0.0), "top");
    }
}
