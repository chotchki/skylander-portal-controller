//! The hero's numbers over the level (openspec AH.26, AH.57, AH.71, AH
//! design decision 14; tv-hud's The hero's numbers on the TV): his health
//! against full, warning under 20 %, his level, his experience toward the
//! next level and his figure's gold, in the TV's own look, sized for a TV
//! at 10 ft. Ours, drawn by the TV, until the original's interface scripts
//! run over its sprite members (stand-in ledger: `hud`).
//!
//! - What it reads ([`read_hud`]), every frame the hero is in the level: his
//!   health and full health as his scripts hold them once they've joined
//!   (AH.24's report; none before, or for a hero walking without them), his
//!   figure's experience and gold as the TV holds them for his slot (what
//!   his figure's words held after the last step, AH.23), his level off the
//!   game's experience table (AH.2), the experience stopping at the table's
//!   last entry, where level and experience both read MAX (AH.57,
//!   chotchki's call: the design doc's departures).
//! - A bite (AH.10, `ah10-probe3`): each drop in his health shows the
//!   amount by him, a red bubble that turns white and rises, and the health
//!   bar's edge flashes white with it. His own white-then-red flash isn't
//!   drawn.
//! - Drawn ([`draw_hud`]) in the bottom left over the level, where the
//!   original's sits (AH.10's shots), under the pause overlay, the portal's
//!   alerts and an intro; gone with the hero.

use std::time::Duration;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use guac_server::progress::Progression;
use guacamole::scripts::ScriptRuntime;
use serde::Serialize;
use skylander_rpcs3_control::FigureProgress;

use crate::hero::HeroBody;
use crate::hero_scripts::{HeroRun, Join, hero_numbers};
use crate::motion::ReducedMotion;
use crate::play::{Play, PlaySystems};
use crate::progress::{Earning, ProgressionIn};
use crate::theme::{Role, Step, TvTheme};
use crate::tv_state::{Pen, TvAssets, UiTree};

/// Under the pause overlay (900).
const HUD_Z: i32 = 800;

/// A damage number over the HUD, under the pause overlay.
const DAMAGE_Z: i32 = 850;

/// The bars' width and the health bar's height, design px.
const BAR_WIDTH: f32 = 520.0;
const HEALTH_HEIGHT: f32 = 44.0;
const XP_HEIGHT: f32 = 20.0;

/// How long a damage number shows, how long it's red first and how far it
/// rises (AH.10's shots: a red bubble by him that turns white and rises).
pub(crate) const DAMAGE_LIFE: Duration = Duration::from_millis(1000);
pub(crate) const DAMAGE_RED: Duration = Duration::from_millis(250);
const DAMAGE_RISE: f32 = 80.0;

/// How long the health bar's edge flashes white after a bite (AH.10: he
/// flashes white, then red, his own colour 0.22 s on).
pub(crate) const FLASH: Duration = Duration::from_millis(220);

/// How far above his pivot a damage number starts, metres: about his head.
const HEAD: f32 = 1.6;

/// His level off the game's table: MAX at its last level, none before
/// the extract's table is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LevelShown {
    Level(u8),
    /// The table's last level (AH.57).
    Max,
    /// No table to read it off yet.
    Unknown,
}

/// The hero's numbers off his scripts, his figure's progress and the
/// game's table, as the HUD shows them this frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HudValues {
    /// His health and full health, once his scripts hold them.
    pub(crate) health: Option<(i32, i32)>,
    pub(crate) level: LevelShown,
    /// His experience, held at the cap, and where his level starts and the
    /// next one does (none at the cap or with no table).
    pub(crate) xp: u32,
    pub(crate) xp_floor: u32,
    pub(crate) xp_next: Option<u32>,
    pub(crate) gold: u32,
}

impl HudValues {
    /// From his scripts' health and full health, his figure's progress and
    /// the game's table, if it's read yet.
    pub(crate) fn of(
        health: Option<i32>,
        full: Option<i32>,
        progress: &FigureProgress,
        table: Option<&Progression>,
    ) -> Self {
        let health = health.zip(full);
        let Some(t) = table.filter(|t| !t.level_xp.is_empty()) else {
            return Self {
                health,
                level: LevelShown::Unknown,
                xp: progress.xp,
                xp_floor: 0,
                xp_next: None,
                gold: progress.gold,
            };
        };
        let xp = progress.xp.min(t.caps().xp);
        let level = t.level_for(xp);
        let floor = t.threshold(level).unwrap_or(0);
        let (shown, next) = match level >= t.max_level() {
            true => (LevelShown::Max, None),
            false => (LevelShown::Level(level), t.threshold(level + 1)),
        };
        Self {
            health,
            level: shown,
            xp,
            xp_floor: floor,
            xp_next: next,
            gold: progress.gold,
        }
    }

    /// His health is under 20 % of full (tv-hud's Low health).
    pub(crate) fn low(&self) -> bool {
        self.health
            .is_some_and(|(h, full)| full > 0 && i64::from(h) * 5 < i64::from(full))
    }

    pub(crate) fn health_text(&self) -> String {
        match self.health {
            Some((h, full)) => format!("{} / {}", grouped(h.max(0) as u32), grouped(full as u32)),
            None => "-".to_owned(),
        }
    }

    /// The share of the health bar filled, 0 to 1.
    pub(crate) fn health_fill(&self) -> f32 {
        match self.health {
            Some((h, full)) if full > 0 => (h as f32 / full as f32).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }

    pub(crate) fn level_text(&self) -> String {
        match self.level {
            LevelShown::Level(n) => format!("LEVEL {n}"),
            LevelShown::Max => "LEVEL MAX".to_owned(),
            LevelShown::Unknown => "LEVEL".to_owned(),
        }
    }

    /// His experience toward the next level: `1,240 / 2,200 XP`, MAX at the
    /// cap, the total alone with no table.
    pub(crate) fn xp_text(&self) -> String {
        match (self.level, self.xp_next) {
            (LevelShown::Max, _) => "MAX".to_owned(),
            (_, Some(next)) => format!("{} / {} XP", grouped(self.xp), grouped(next)),
            (_, None) => format!("{} XP", grouped(self.xp)),
        }
    }

    /// The share of the experience bar filled: his way through his level,
    /// full at the cap.
    pub(crate) fn xp_fill(&self) -> f32 {
        match (self.level, self.xp_next) {
            (LevelShown::Max, _) => 1.0,
            (_, Some(next)) if next > self.xp_floor => {
                (self.xp.saturating_sub(self.xp_floor) as f32 / (next - self.xp_floor) as f32)
                    .clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }

    pub(crate) fn gold_text(&self) -> String {
        grouped(self.gold)
    }
}

/// `n` with its thousands grouped by commas.
pub(crate) fn grouped(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(d);
    }
    out
}

/// A bite's number on screen (AH.10).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Damage {
    pub(crate) amount: i32,
    /// When it came, on the TV's real clock.
    pub(crate) born: Duration,
    /// Where by him it starts on the screen, design px; none when his head
    /// is off the level camera's picture (it then sits by the health bar).
    pub(crate) at: Option<Vec2>,
}

impl Damage {
    /// Still red at `now`; white after [`DAMAGE_RED`].
    pub(crate) fn red(&self, now: Duration) -> bool {
        now.saturating_sub(self.born) < DAMAGE_RED
    }

    /// How far it has risen at `now`, px, eased out over its life.
    pub(crate) fn risen(&self, now: Duration) -> f32 {
        let t = (now.saturating_sub(self.born).as_secs_f32() / DAMAGE_LIFE.as_secs_f32()).min(1.0);
        DAMAGE_RISE * (1.0 - (1.0 - t) * (1.0 - t))
    }
}

/// What the HUD shows, for the drawing and the mirror.
#[derive(Resource, Debug, Default)]
pub(crate) struct Hud {
    /// None while no hero is in the level.
    pub(crate) shown: Option<HudValues>,
    /// The hero and his health last frame: a drop is a bite.
    last: Option<(Entity, i32)>,
    /// The bites' numbers on screen, oldest first.
    pub(crate) damage: Vec<Damage>,
    /// Until when the health bar's edge flashes, on the real clock.
    flash_until: Option<Duration>,
}

impl Hud {
    /// The health bar's edge is white at `now`: [`FLASH`] from a bite.
    pub(crate) fn flashing(&self, now: Duration) -> bool {
        self.flash_until.is_some_and(|t| now < t)
    }
}

pub(crate) struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hud>()
            .add_systems(Update, (read_hud, draw_hud).chain().after(PlaySystems));
    }
}

/// The hero's numbers into [`Hud`] (the module docs), a drop in his health
/// a damage number by him.
fn read_hud(
    play: Option<Res<Play>>,
    bodies: Query<(&HeroBody, Option<&HeroRun>)>,
    runtime: Option<Res<ScriptRuntime>>,
    earning: Option<Res<Earning>>,
    table: Option<Res<ProgressionIn>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    (time, ui_scale): (Res<Time<Real>>, Option<Res<UiScale>>),
    mut hud: ResMut<Hud>,
) {
    let now = time.elapsed();
    // The level camera's logical px to the UI's design px (`tv::scale_ui`).
    let scale = ui_scale.map_or(1.0, |s| s.0).max(f32::EPSILON);
    let hud = &mut *hud;
    hud.damage
        .retain(|d| now.saturating_sub(d.born) < DAMAGE_LIFE);
    let Some((hero, slot, placement)) = play.as_deref().and_then(Play::hero_placement) else {
        if hud.shown.is_some() {
            hud.shown = None;
        }
        hud.last = None;
        hud.damage.clear();
        return;
    };
    let (body, run) = bodies.get(hero).map_or((None, None), |(b, r)| (Some(b), r));
    let numbers = match (run.map(|r| &r.join), runtime.as_deref()) {
        (Some(Join::Joined(minion)), Some(rt)) => hero_numbers(&rt.vm, *minion),
        _ => Default::default(),
    };
    // His slot's values as the TV holds them, else the row he was placed
    // with (play's report reads them so too).
    let progress = earning
        .as_deref()
        .and_then(|e| e.slots.get(slot.as_usize()))
        .and_then(Option::as_ref)
        .map(|e| e.progress)
        .or_else(|| placement.owner.as_ref().map(|o| o.progress))
        .unwrap_or_default();
    let table = table.and_then(|t| t.0.get());
    let values = HudValues::of(
        numbers.health,
        numbers.health_max,
        &progress,
        table.as_deref(),
    );
    let health = values.health.map(|(h, _)| h);
    if let (Some((was_hero, was)), Some(h)) = (hud.last, health)
        && was_hero == hero
        && h < was
    {
        let head = body.map(|b| b.hero.pivot + Vec3::Y * HEAD);
        let at = play
            .as_deref()
            .and_then(Play::level_camera)
            .and_then(|c| cameras.get(c).ok())
            .zip(head)
            .and_then(|((camera, place), head)| camera.world_to_viewport(place, head).ok())
            .map(|at| at / scale);
        hud.damage.push(Damage {
            amount: was - h,
            born: now,
            at,
        });
        hud.flash_until = Some(now + FLASH);
    }
    hud.last = health.map(|h| (hero, h));
    if hud.shown.as_ref() != Some(&values) {
        hud.shown = Some(values);
    }
}

/// A part of the drawn HUD, for patching it in place.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HudPart {
    Root,
    Level,
    HealthBar,
    HealthFill,
    HealthText,
    Low,
    XpFill,
    XpText,
    Gold,
}

/// A damage number drawn, by when it came.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct DamageBubble(Duration);

/// Spawns the HUD while it shows and patches it in place as his numbers
/// change; draws each bite's number. Needs the TV's font and theme; the
/// headless tests without them draw nothing, [`Hud`] holding the numbers
/// all the same.
#[allow(clippy::too_many_arguments)]
fn draw_hud(
    mut commands: Commands,
    hud: Res<Hud>,
    (assets, theme, reduced): (
        Option<Res<TvAssets>>,
        Option<Res<TvTheme>>,
        Option<Res<ReducedMotion>>,
    ),
    time: Res<Time<Real>>,
    mut parts: Query<(
        &HudPart,
        Option<&mut Text>,
        Option<&mut Node>,
        Option<&mut BackgroundColor>,
        Option<&mut BorderColor>,
        Option<&mut Visibility>,
    )>,
    mut bubbles: Query<
        (
            Entity,
            &DamageBubble,
            &mut Node,
            &mut BackgroundColor,
            &Children,
        ),
        Without<HudPart>,
    >,
    mut inks: Query<&mut TextColor, (Without<HudPart>, Without<DamageBubble>)>,
    mut root: Local<Option<Entity>>,
) {
    let (Some(assets), Some(theme)) = (assets, theme) else {
        return;
    };
    let now = time.elapsed();
    let pen = Pen::new(&assets, &theme);
    match (&hud.shown, *root) {
        (None, Some(e)) => {
            if let Ok(mut c) = commands.get_entity(e) {
                c.despawn();
            }
            *root = None;
        }
        (Some(v), None) => *root = Some(spawn_hud(&mut commands, &pen, v)),
        (Some(v), Some(_)) => patch(&mut parts, &theme, v, hud.flashing(now)),
        (None, None) => {}
    }
    // The bites' numbers: one bubble each, red then white, rising.
    let rise = !reduced.is_some_and(|r| r.0);
    let mut drawn = Vec::new();
    for (e, bubble, mut node, mut fill, children) in &mut bubbles {
        let Some(d) = hud.damage.iter().find(|d| d.born == bubble.0) else {
            commands.entity(e).despawn();
            continue;
        };
        drawn.push(d.born);
        let (start, lift) = (bubble_at(d), if rise { d.risen(now) } else { 0.0 });
        node.left = px(start.x);
        node.top = px(start.y - lift);
        let red = d.red(now);
        let want = if red { theme.hurt } else { theme.light };
        if fill.0 != want {
            fill.0 = want;
            for child in children.iter() {
                if let Ok(mut ink) = inks.get_mut(child) {
                    ink.0 = if red { theme.light } else { theme.hurt };
                }
            }
        }
    }
    for d in hud.damage.iter().filter(|d| !drawn.contains(&d.born)) {
        let at = bubble_at(d);
        commands
            .spawn((
                DamageBubble(d.born),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(at.x),
                    top: px(at.y),
                    padding: UiRect::axes(px(20), px(8)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(theme.hurt),
                GlobalZIndex(DAMAGE_Z),
            ))
            .with_child(pen.inked(
                &d.amount.to_string(),
                Step::Md,
                theme.light,
                Justify::Center,
            ));
    }
}

/// Where a bubble starts: by his head, a little to his right, or above the
/// health bar when his head is off the picture.
fn bubble_at(d: &Damage) -> Vec2 {
    d.at.map_or(
        Vec2::new(48.0 + BAR_WIDTH / 2.0, crate::tv::DESIGN_HEIGHT - 260.0),
        |at| at + Vec2::new(40.0, -40.0),
    )
}

/// The HUD's look for `v`: its texts, its bars' fills, the warning and the
/// flash.
fn patch(
    parts: &mut Query<(
        &HudPart,
        Option<&mut Text>,
        Option<&mut Node>,
        Option<&mut BackgroundColor>,
        Option<&mut BorderColor>,
        Option<&mut Visibility>,
    )>,
    theme: &TvTheme,
    v: &HudValues,
    flashing: bool,
) {
    for (part, text, node, fill, edge, visibility) in parts.iter_mut() {
        let set_text = |text: Option<Mut<Text>>, s: String| {
            if let Some(mut t) = text
                && t.0 != s
            {
                t.0 = s;
            }
        };
        let set_width = |node: Option<Mut<Node>>, share: f32| {
            if let Some(mut n) = node
                && n.width != percent(share * 100.0)
            {
                n.width = percent(share * 100.0);
            }
        };
        match part {
            HudPart::Level => set_text(text, v.level_text()),
            HudPart::HealthText => set_text(text, v.health_text()),
            HudPart::XpText => set_text(text, v.xp_text()),
            HudPart::Gold => set_text(text, v.gold_text()),
            HudPart::HealthFill => {
                set_width(node, v.health_fill());
                let want = if v.low() { theme.hurt } else { theme.health };
                if let Some(mut f) = fill
                    && f.0 != want
                {
                    f.0 = want;
                }
            }
            HudPart::XpFill => set_width(node, v.xp_fill()),
            HudPart::HealthBar => {
                let want = BorderColor::all(if flashing { theme.light } else { theme.gold });
                if let Some(mut e) = edge
                    && *e != want
                {
                    *e = want;
                }
            }
            HudPart::Low => {
                set_text(text, low_text(v).to_owned());
                let want = if v.low() {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                if let Some(mut vis) = visibility
                    && *vis != want
                {
                    *vis = want;
                }
            }
            HudPart::Root => {}
        }
    }
}

/// The warning's words while it shows, none otherwise (so the mirror's
/// lines don't carry a hidden one).
fn low_text(v: &HudValues) -> &'static str {
    if v.low() { "LOW HEALTH" } else { "" }
}

/// The HUD: his level beside his health bar, the experience bar under it,
/// then his gold, on a dimmed plate in the bottom left.
fn spawn_hud(commands: &mut Commands, pen: &Pen, v: &HudValues) -> Entity {
    let theme = &pen.theme;
    let bar = |height: f32, edge: f32| Node {
        width: px(BAR_WIDTH),
        height: px(height),
        border: UiRect::all(px(edge)),
        border_radius: BorderRadius::all(px(height / 2.0)),
        overflow: Overflow::clip(),
        ..default()
    };
    let fill = |share: f32| Node {
        width: percent(share * 100.0),
        height: percent(100),
        ..default()
    };
    commands
        .spawn((
            HudPart::Root,
            Node {
                position_type: PositionType::Absolute,
                left: px(48),
                bottom: px(48),
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                padding: UiRect::all(px(20)),
                border_radius: BorderRadius::all(px(24)),
                ..default()
            },
            BackgroundColor(theme.shadow.with_alpha(0.5)),
            GlobalZIndex(HUD_Z),
        ))
        .with_children(|hud| {
            hud.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(20),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    HudPart::Level,
                    pen.text_at(&v.level_text(), Step::Md, Role::Accent, Justify::Left),
                ));
                row.spawn((
                    HudPart::HealthBar,
                    bar(HEALTH_HEIGHT, 4.0),
                    BackgroundColor(theme.panel[2]),
                    BorderColor::all(theme.gold),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        HudPart::HealthFill,
                        fill(v.health_fill()),
                        BackgroundColor(if v.low() { theme.hurt } else { theme.health }),
                    ));
                    bar.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(0),
                            top: px(0),
                            bottom: px(0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        children![(
                            HudPart::HealthText,
                            pen.text(&v.health_text(), Step::Caption, Role::Primary),
                        )],
                    ));
                });
                row.spawn((
                    HudPart::Low,
                    pen.inked(low_text(v), Step::Body, theme.hurt, Justify::Left),
                    if v.low() {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                ));
            });
            hud.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(20),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    bar(XP_HEIGHT, 3.0),
                    BackgroundColor(theme.panel[2]),
                    BorderColor::all(theme.gold_mid),
                ))
                .with_child((
                    HudPart::XpFill,
                    fill(v.xp_fill()),
                    BackgroundColor(theme.gold_bright),
                ));
                row.spawn((
                    HudPart::XpText,
                    pen.text_at(&v.xp_text(), Step::Caption, Role::Primary, Justify::Left),
                ));
            });
            hud.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Baseline,
                column_gap: px(16),
                ..default()
            })
            .with_children(|row| {
                row.spawn(pen.text_at("GOLD", Step::Caption, Role::Muted, Justify::Left));
                row.spawn((
                    HudPart::Gold,
                    pen.text_at(&v.gold_text(), Step::Md, Role::Accent, Justify::Left),
                ));
            });
        })
        .id()
}

/// A bite's number on screen, `/api/_test/tv`'s `hud.damage`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DamageShot {
    pub amount: i32,
    /// `red`, then `white`.
    pub color: String,
    /// How long it has shown, ms on the real clock.
    pub age_ms: f64,
}

/// The HUD, its part of `/api/_test/tv` (AH.26; `test-hooks` builds).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HudShot {
    /// His health and full health as shown, none before his scripts hold
    /// them; whether the warning shows.
    pub health: Option<i32>,
    pub health_max: Option<i32>,
    pub low: bool,
    /// His level: `1` to the table's last but one, `MAX` at the cap, `?`
    /// with no table.
    pub level: String,
    /// His experience (held at the cap) and the next level's, none at the
    /// cap or with no table.
    pub xp: u32,
    pub xp_next: Option<u32>,
    pub gold: u32,
    /// The health bar's edge flashing white after a bite.
    pub flash: bool,
    /// Every line drawn, top to bottom; empty when nothing's drawn (a
    /// headless app).
    pub texts: Vec<String>,
    /// The bites' numbers on screen, oldest first.
    pub damage: Vec<DamageShot>,
}

/// [`Hud`] and the drawn HUD's lines, for the mirror.
#[derive(SystemParam)]
pub(crate) struct HudShots<'w, 's> {
    hud: Option<Res<'w, Hud>>,
    time: Res<'w, Time<Real>>,
    roots: Query<'w, 's, (Entity, &'static HudPart)>,
    tree: UiTree<'w, 's>,
}

impl HudShots<'_, '_> {
    pub(crate) fn shot(&self) -> Option<HudShot> {
        let hud = self.hud.as_deref()?;
        let v = hud.shown.as_ref()?;
        let now = self.time.elapsed();
        let texts = self
            .roots
            .iter()
            .find(|(_, p)| **p == HudPart::Root)
            .map(|(e, _)| self.tree.walk(e).0)
            .unwrap_or_default()
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect();
        Some(HudShot {
            health: v.health.map(|(h, _)| h),
            health_max: v.health.map(|(_, f)| f),
            low: v.low(),
            level: match v.level {
                LevelShown::Level(n) => n.to_string(),
                LevelShown::Max => "MAX".to_owned(),
                LevelShown::Unknown => "?".to_owned(),
            },
            xp: v.xp,
            xp_next: v.xp_next,
            gold: v.gold,
            flash: hud.flashing(now),
            texts,
            damage: hud
                .damage
                .iter()
                .map(|d| DamageShot {
                    amount: d.amount,
                    color: if d.red(now) { "red" } else { "white" }.to_owned(),
                    age_ms: now.saturating_sub(d.born).as_secs_f64() * 1e3,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Giants' experience table (figure-progress' Levels follow the
    /// original's experience table), its gold clamp.
    fn giants() -> Progression {
        Progression {
            level_xp: vec![
                0, 1_000, 2_200, 3_800, 6_000, 9_000, 13_000, 18_200, 24_800, 33_000, 42_700,
                53_900, 66_600, 80_800, 96_500,
            ],
            gold_cap: 65_000,
        }
    }

    fn at(xp: u32) -> HudValues {
        let p = FigureProgress {
            xp,
            gold: 17,
            ..FigureProgress::default()
        };
        HudValues::of(Some(290), Some(290), &p, Some(&giants()))
    }

    /// AH.57 on the TV: one short of the cap, level 14 and the experience
    /// toward 96,500; at it, MAX for the level and the experience, the bar
    /// full; past it (the original's toy keeps counting, AH.52) held at
    /// 96,500 and MAX. Level 1 at 0, level 2 at exactly 1,000.
    #[test]
    fn the_level_and_experience_read_max_at_the_cap() {
        let short = at(96_499);
        assert_eq!(
            (short.level, short.xp_next),
            (LevelShown::Level(14), Some(96_500))
        );
        assert_eq!(short.level_text(), "LEVEL 14");
        assert_eq!(short.xp_text(), "96,499 / 96,500 XP");
        assert!(short.xp_fill() > 0.99 && short.xp_fill() < 1.0);
        for xp in [96_500, 96_518] {
            let cap = at(xp);
            assert_eq!(
                (cap.level, cap.xp, cap.xp_next),
                (LevelShown::Max, 96_500, None)
            );
            assert_eq!(
                (cap.level_text(), cap.xp_text()),
                ("LEVEL MAX".into(), "MAX".into())
            );
            assert_eq!(cap.xp_fill(), 1.0);
        }
        assert_eq!(at(0).level, LevelShown::Level(1));
        assert_eq!(at(999).xp_text(), "999 / 1,000 XP");
        let two = at(1_000);
        assert_eq!((two.level, two.xp_fill()), (LevelShown::Level(2), 0.0));
        assert_eq!(at(1_600).xp_fill(), 0.5, "halfway from 1,000 to 2,200");
        // No table yet: the total alone.
        let p = FigureProgress {
            xp: 16,
            ..FigureProgress::default()
        };
        let unread = HudValues::of(None, None, &p, None);
        assert_eq!(
            (unread.level, unread.xp_text()),
            (LevelShown::Unknown, "16 XP".into())
        );
        assert_eq!(unread.health_text(), "-");
    }

    /// tv-hud's Low health: the warning under 20 % of full, not at it; a
    /// beaten hero's 0 too; none with no health read.
    #[test]
    fn the_warning_shows_under_a_fifth_of_full_health() {
        let p = FigureProgress::default();
        let with = |h: i32, full: i32| HudValues::of(Some(h), Some(full), &p, Some(&giants()));
        assert!(!with(58, 290).low(), "58 of 290 is 20 %");
        assert!(with(57, 290).low());
        assert!(with(0, 290).low());
        assert!(!with(290, 290).low());
        assert!(!HudValues::of(None, None, &p, None).low());
        assert_eq!(with(278, 290).health_text(), "278 / 290");
        assert_eq!(with(145, 290).health_fill(), 0.5);
    }

    /// AH.10's bite: red for its first quarter second, then white, rising
    /// eased to its height at a second.
    #[test]
    fn a_damage_number_turns_white_and_rises() {
        let born = Duration::from_secs(3);
        let d = Damage {
            amount: 14,
            born,
            at: None,
        };
        assert!(d.red(born) && d.red(born + DAMAGE_RED - Duration::from_millis(1)));
        assert!(!d.red(born + DAMAGE_RED));
        assert_eq!(d.risen(born), 0.0);
        assert!(d.risen(born + DAMAGE_LIFE / 2) > DAMAGE_RISE / 2.0);
        assert_eq!(d.risen(born + DAMAGE_LIFE), DAMAGE_RISE);
        assert_eq!(d.risen(born + DAMAGE_LIFE * 2), DAMAGE_RISE);
    }

    #[test]
    fn numbers_are_grouped_by_thousands() {
        for (n, want) in [
            (0, "0"),
            (999, "999"),
            (1_000, "1,000"),
            (65_000, "65,000"),
            (96_500, "96,500"),
            (1_234_567, "1,234,567"),
        ] {
            assert_eq!(grouped(n), want);
        }
    }
}
