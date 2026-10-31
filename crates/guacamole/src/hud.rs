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
//! - A level-up (AH.248, the oracle's `fight-lu-1793463185`, a frame every
//!   4 flips): the original's HUD sends small green arrows up off the top of
//!   its portrait, `global.bld`'s `UI_LevelUp_ArrowParticle` (AH.244's
//!   image), from 0.45 s after the level rises to 1.92-1.98 s, three to six at a
//!   time, each about 22 x 26 px on its 720-line frame and rising about 70
//!   px as it fades; the HUD spawns it, not the flourish over him (no
//!   script of the TV's listing names it, AH.247). Ours rises off the top
//!   of our plate where the original's portrait stands (`level_up_arrows`),
//!   drawn from that image at the size the frames give, its aspect its own.
//!   The rest of the original's level-up display (the portrait's zoom, the
//!   burst, the "Level Up!" panel) isn't drawn.
//! - Drawn ([`draw_hud`]) in the bottom left over the level, where the
//!   original's sits (AH.10's shots), under the pause overlay, the portal's
//!   alerts and an intro; gone with the hero, and not there before he's in
//!   (his join pending, his entry beam falling: AH.224). Hidden, the bites'
//!   numbers with it, while the level's scripts hide the original's
//!   ([`guacamole::scripts::player_hud_hidden`]: the pit's intro and
//!   Norticus' scene), its numbers read on.

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

/// The level-up arrows (AH.248): when after the level rises the first is
/// sent up and the last; every how long one goes; how long each lives,
/// fading in over its first share and out over its last; how far it rises
/// and how far either side of the middle it starts, design px (the oracle's
/// 720-line frames times 1.5). The first and the end are the frames'; the
/// rest is ours, read off them by eye.
pub(crate) const ARROWS_FROM: Duration = Duration::from_millis(450);
pub(crate) const ARROWS_UNTIL: Duration = Duration::from_millis(1450);
pub(crate) const ARROW_EVERY: Duration = Duration::from_millis(100);
pub(crate) const ARROW_LIFE: Duration = Duration::from_millis(500);
const ARROW_FADE_IN: f32 = 0.2;
const ARROW_FADE_OUT: f32 = 0.4;
pub(crate) const ARROW_RISE: f32 = 105.0;
pub(crate) const ARROW_SPREAD: f32 = 52.0;
/// An arrow's size, design px: 22 x 26 px on the 720-line frames, the
/// image's own 109 x 128 aspect.
pub(crate) const ARROW_WIDTH: f32 = 33.0;
pub(crate) const ARROW_HEIGHT: f32 = ARROW_WIDTH * 128.0 / 109.0;
/// Where along our plate's top the arrows rise from, design px from its
/// left edge: where the original's portrait stands (its middle 107 px into
/// the 1280-wide frame, 160 design px from the screen's edge; our plate's
/// at 48).
const ARROWS_AT: f32 = 112.0;
/// The emitter's own area over the plate, design px.
const ARROWS_AREA: Vec2 = Vec2::new(
    2.0 * (ARROW_SPREAD + ARROW_WIDTH),
    ARROW_RISE + 2.0 * ARROW_HEIGHT,
);
/// The particle part's emitter the arrows are drawn from.
pub(crate) const ARROW_EMITTER: &str = "UI_LevelUp_ArrowParticle";

/// One level-up arrow at a moment: how far from the middle it rose from
/// (design px, right positive), how far it's risen and how much it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Arrow {
    pub(crate) x: f32,
    pub(crate) rise: f32,
    pub(crate) alpha: f32,
}

/// The arrows showing `since` the level rose (AH.248): one sent up every
/// [`ARROW_EVERY`] from [`ARROWS_FROM`] to [`ARROWS_UNTIL`], each rising
/// [`ARROW_RISE`] over its [`ARROW_LIFE`], eased out, fading in and out,
/// the n-th from a fixed spread across [`ARROW_SPREAD`] either side.
pub(crate) fn level_up_arrows(since: Duration) -> Vec<Arrow> {
    let life = ARROW_LIFE.as_secs_f32();
    let every = ARROW_EVERY.as_secs_f32();
    let from = ARROWS_FROM.as_secs_f32();
    let sent = ((ARROWS_UNTIL - ARROWS_FROM).as_secs_f32() / every).floor() as u32 + 1;
    let t = since.as_secs_f32();
    (0..sent)
        .filter_map(|n| {
            let age = (t - (from + n as f32 * every)) / life;
            if !(0.0..1.0).contains(&age) {
                return None;
            }
            // The golden ratio's steps: an even spread that doesn't repeat.
            let across = (n as f32 * 0.618_034).fract() * 2.0 - 1.0;
            let alpha = (age / ARROW_FADE_IN)
                .min((1.0 - age) / ARROW_FADE_OUT)
                .clamp(0.0, 1.0);
            Some(Arrow {
                x: across * ARROW_SPREAD,
                rise: ARROW_RISE * (1.0 - (1.0 - age) * (1.0 - age)),
                alpha,
            })
        })
        .collect()
}

/// How long after the level rises the last arrow's gone.
pub(crate) fn arrows_over() -> Duration {
    ARROWS_UNTIL + ARROW_LIFE
}

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
    /// The scripts hide the player's HUD (a scene): nothing drawn.
    pub(crate) hidden: bool,
    /// The hero and the level shown last frame: a rise is a level-up.
    last_level: Option<(Entity, LevelShown)>,
    /// When the level last rose, on the real clock, while its arrows show
    /// (AH.248).
    pub(crate) level_up: Option<Duration>,
}

impl Hud {
    /// The health bar's edge is white at `now`: [`FLASH`] from a bite.
    pub(crate) fn flashing(&self, now: Duration) -> bool {
        self.flash_until.is_some_and(|t| now < t)
    }

    /// `hero`'s level shown this frame: higher than last frame's for the
    /// same hero, a level-up at `now` (a hero coming in at his level, or the
    /// table arriving, sends no arrows up).
    pub(crate) fn note_level(&mut self, hero: Entity, level: LevelShown, now: Duration) {
        if let Some((was_hero, was)) = self.last_level
            && was_hero == hero
            && rose(was, level)
        {
            self.level_up = Some(now);
        }
        self.last_level = Some((hero, level));
    }
}

pub(crate) struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hud>()
            .init_resource::<ArrowImage>()
            .add_systems(
                Update,
                (read_hud, find_arrow_image, draw_hud, draw_arrows)
                    .chain()
                    .after(PlaySystems),
            );
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
    let hidden = runtime
        .as_deref()
        .is_some_and(|rt| guacamole::scripts::player_hud_hidden(&rt.vm));
    if hud.hidden != hidden {
        hud.hidden = hidden;
    }
    if hud
        .level_up
        .is_some_and(|t| now.saturating_sub(t) >= arrows_over())
    {
        hud.level_up = None;
    }
    // A hero whose join is pending (his entry beam still falling, openspec
    // AH.224) is drawn nowhere and has no health yet: no HUD until he's in.
    let in_now = play
        .as_deref()
        .and_then(Play::hero_placement)
        .filter(|(hero, ..)| {
            !bodies
                .get(*hero)
                .is_ok_and(|(_, run)| run.is_some_and(|r| r.join.pending()))
        });
    let Some((hero, slot, placement)) = in_now else {
        if hud.shown.is_some() {
            hud.shown = None;
        }
        hud.last = None;
        hud.last_level = None;
        hud.level_up = None;
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
    hud.note_level(hero, values.level, now);
    if hud.shown.as_ref() != Some(&values) {
        hud.shown = Some(values);
    }
}

/// The level shown went up from `was` to `now`: a higher level, or the cap.
fn rose(was: LevelShown, now: LevelShown) -> bool {
    match (was, now) {
        (LevelShown::Level(a), LevelShown::Level(b)) => b > a,
        (LevelShown::Level(_), LevelShown::Max) => true,
        _ => false,
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
    /// The level-up arrows' area over the plate (AH.248).
    Arrows,
}

/// A level-up arrow drawn, by its place in the arrows of the moment.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LevelUpArrow(usize);

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
            &mut Visibility,
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
    let shows = if hud.hidden {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    match (&hud.shown, *root) {
        (None, Some(e)) => {
            if let Ok(mut c) = commands.get_entity(e) {
                c.despawn();
            }
            *root = None;
        }
        (Some(v), None) => *root = Some(spawn_hud(&mut commands, &pen, v, shows)),
        (Some(v), Some(_)) => patch(&mut parts, &theme, v, hud.flashing(now), shows),
        (None, None) => {}
    }
    // The bites' numbers: one bubble each, red then white, rising.
    let rise = !reduced.is_some_and(|r| r.0);
    let mut drawn = Vec::new();
    for (e, bubble, mut node, mut fill, children, mut seen) in &mut bubbles {
        let Some(d) = hud.damage.iter().find(|d| d.born == bubble.0) else {
            commands.entity(e).despawn();
            continue;
        };
        drawn.push(d.born);
        if *seen != shows {
            *seen = shows;
        }
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
                shows,
            ))
            .with_child(pen.inked(
                &d.amount.to_string(),
                Step::Md,
                theme.light,
                Justify::Center,
            ));
    }
}

/// The image the level-up arrows are drawn from (AH.248): the particle
/// part's arrow, once a level that holds it is loaded.
#[derive(Resource, Debug, Default)]
pub(crate) struct ArrowImage(pub(crate) Option<Handle<Image>>);

/// Looks the arrow's image up in the level once a level-up asks for it,
/// and lets it go when no hero is in the level.
fn find_arrow_image(
    hud: Res<Hud>,
    level: Option<Res<crate::level::Level>>,
    mut image: ResMut<ArrowImage>,
) {
    if hud.shown.is_none() {
        if image.0.is_some() {
            image.0 = None;
        }
        return;
    }
    if image.0.is_none() && hud.level_up.is_some() {
        image.0 = level.and_then(|l| l.emitter_image(ARROW_EMITTER));
    }
}

/// The level-up arrows (AH.248): while [`Hud::level_up`]'s arrows show,
/// each drawn from [`ArrowImage`] in the arrows' area over the plate,
/// rising and fading as [`level_up_arrows`] has it; none with no image (no
/// level loaded, or a part without the emitter) or no HUD drawn. They rise
/// under [`ReducedMotion`] too: that's the dev shots' switch
/// (`GUAC_SHOT_DIR`, not a user setting), which holds the TV's ambient
/// motion and entrances so a screen's shot repeats; these are the game's
/// level-up, as its particles are, and their rise is what a level-up's
/// shots look at. The bites' numbers (AH.26) do hold under it.
fn draw_arrows(
    mut commands: Commands,
    hud: Res<Hud>,
    time: Res<Time<Real>>,
    image: Res<ArrowImage>,
    areas: Query<(Entity, &HudPart)>,
    mut drawn: Query<(Entity, &LevelUpArrow, &mut Node, &mut ImageNode)>,
) {
    if hud.level_up.is_none() && drawn.is_empty() {
        return;
    }
    let now = time.elapsed();
    let area = areas
        .iter()
        .find(|(_, p)| **p == HudPart::Arrows)
        .map(|(e, _)| e);
    let image = image.0.clone();
    let arrows = match (hud.level_up, &image, area) {
        (Some(t), Some(_), Some(_)) => level_up_arrows(now.saturating_sub(t)),
        _ => Vec::new(),
    };
    // Placed from the area's top: its bottom edge on the plate's top.
    let place = |a: &Arrow| {
        (
            px(ARROWS_AREA.x / 2.0 + a.x - ARROW_WIDTH / 2.0),
            px(arrow_top(a.rise)),
        )
    };
    let mut have = vec![false; arrows.len()];
    for (e, arrow, mut node, mut ink) in &mut drawn {
        let Some(a) = arrows.get(arrow.0) else {
            commands.entity(e).despawn();
            continue;
        };
        have[arrow.0] = true;
        let (left, top) = place(a);
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
        let want = Color::WHITE.with_alpha(a.alpha);
        if ink.color != want {
            ink.color = want;
        }
    }
    let (Some(image), Some(area)) = (image, area) else {
        return;
    };
    for (i, a) in arrows.iter().enumerate().filter(|(i, _)| !have[*i]) {
        let (left, top) = place(a);
        commands.spawn((
            LevelUpArrow(i),
            Node {
                position_type: PositionType::Absolute,
                left,
                top,
                width: px(ARROW_WIDTH),
                height: px(ARROW_HEIGHT),
                ..default()
            },
            // The image's rows run bottom up as the particle part stores
            // them: drawn as it is, the arrow points down.
            ImageNode {
                color: Color::WHITE.with_alpha(a.alpha),
                flip_y: true,
                ..ImageNode::new(image.clone())
            },
            ChildOf(area),
        ));
    }
}

/// An arrow's top in the arrows' area, design px, `rise` over the plate.
fn arrow_top(rise: f32) -> f32 {
    ARROWS_AREA.y - ARROW_HEIGHT - rise
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
    shows: Visibility,
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
            HudPart::Root => {
                if let Some(mut vis) = visibility
                    && *vis != shows
                {
                    *vis = shows;
                }
            }
            HudPart::Arrows => {}
        }
    }
}

/// The warning's words while it shows, none otherwise (so the mirror's
/// lines don't carry a hidden one).
fn low_text(v: &HudValues) -> &'static str {
    if v.low() { "LOW HEALTH" } else { "" }
}

/// The HUD: his level beside his health bar, the experience bar under it,
/// then his gold, on a dimmed plate in the bottom left; `shows` hidden while
/// the scripts hide the player's.
fn spawn_hud(commands: &mut Commands, pen: &Pen, v: &HudValues, shows: Visibility) -> Entity {
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
            shows,
        ))
        .with_children(|hud| {
            // The level-up arrows' area, on the plate's top edge where the
            // original's portrait stands (AH.248).
            hud.spawn((
                HudPart::Arrows,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(ARROWS_AT - ARROWS_AREA.x / 2.0),
                    bottom: percent(100),
                    width: px(ARROWS_AREA.x),
                    height: px(ARROWS_AREA.y),
                    ..default()
                },
            ));
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
    /// The scripts hide the player's HUD (a scene): nothing drawn, the
    /// numbers read on.
    pub hidden: bool,
    /// Every line drawn and seen, top to bottom; empty when nothing's drawn
    /// (a headless app) or the drawn HUD is hidden.
    pub texts: Vec<String>,
    /// The bites' numbers on screen, oldest first, hidden with the HUD
    /// while `hidden`.
    pub damage: Vec<DamageShot>,
    /// How long since the level rose while its arrows show (AH.248), ms on
    /// the real clock, and the arrows drawn now.
    pub level_up_ms: Option<f64>,
    pub arrows: usize,
}

/// [`Hud`] and the drawn HUD's lines, for the mirror.
#[derive(SystemParam)]
pub(crate) struct HudShots<'w, 's> {
    hud: Option<Res<'w, Hud>>,
    time: Res<'w, Time<Real>>,
    roots: Query<'w, 's, (Entity, &'static HudPart, Option<&'static Visibility>)>,
    arrows: Query<'w, 's, &'static LevelUpArrow>,
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
            .find(|(_, p, seen)| {
                **p == HudPart::Root && seen.is_none_or(|v| *v != Visibility::Hidden)
            })
            .map(|(e, ..)| self.tree.walk(e).0)
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
            hidden: hud.hidden,
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
            level_up_ms: hud
                .level_up
                .map(|t| now.saturating_sub(t).as_secs_f64() * 1e3),
            arrows: self.arrows.iter().count(),
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

    /// A VM over the builder's `UI Containers` (the UI's scripts' shape)
    /// and the player HUD's container in it.
    fn ui_runtime() -> (guacamole::scripts::ScriptRuntime, guac_script::Value) {
        use std::sync::Arc;

        use guac_formats::igz::Igz;
        use guac_formats::script::Graph;
        use guac_formats::script::builder::{self, UI_CONTAINERS, UI_PLAYER_HUD};
        use guac_script::{Config, Generator, Mode, Program, Role, Unit, Vm};

        let bytes = builder::ui_containers();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = Unit {
            name: "global",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        let program = Arc::new(Program::link(&[unit]).unwrap());
        let mut vm = Vm::new(program, Config::new(Mode::Strict));
        let g = vm.program().global_named(UI_CONTAINERS).unwrap();
        let containers = vm.global(g).map(|v| vm.value(v)).unwrap();
        let hud = vm
            .field(containers, UI_PLAYER_HUD)
            .map(|v| vm.value(v))
            .unwrap();
        let runtime = guacamole::scripts::ScriptRuntime {
            vm,
            generator: Generator::seeded(1),
            player_loop: Default::default(),
            steps: 0,
            stopped: Vec::new(),
            entry_stopped: None,
        };
        (runtime, hud)
    }

    fn set_hide(app: &mut App, hud: guac_script::Value, state: i32) {
        use guac_formats::script::builder::UI_HIDE_STATE;
        let mut rt = app
            .world_mut()
            .resource_mut::<guacamole::scripts::ScriptRuntime>();
        assert!(
            rt.vm
                .set_field(hud, UI_HIDE_STATE, guac_script::Value::Int(state))
        );
    }

    /// The HUD hides while the scripts hide the player's (a scene: the
    /// pit's intro and Norticus' scene hold the player HUD container's hide
    /// state at 1 or 2, mergefix4's read on AH.8's route), and shows again
    /// at 0, the read running whether or not a hero is in.
    #[test]
    fn the_hud_hides_while_the_scripts_hide_the_players() {
        let (runtime, hud) = ui_runtime();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Hud>()
            .insert_resource(runtime)
            .add_systems(Update, read_hud);
        app.update();
        assert!(!app.world().resource::<Hud>().hidden);
        for (state, hidden) in [(1, true), (2, true), (0, false), (-1, false)] {
            set_hide(&mut app, hud, state);
            app.update();
            assert_eq!(app.world().resource::<Hud>().hidden, hidden, "{state}");
        }
    }

    /// Whether `e` is drawn by Bevy's visibility rule: its own `Hidden` or
    /// the nearest ancestor's that isn't `Inherited` decides, a root
    /// inheriting nothing seen.
    fn is_drawn(world: &World, e: Entity) -> bool {
        let mut at = e;
        loop {
            match world.get::<Visibility>(at).copied() {
                Some(Visibility::Hidden) => return false,
                Some(Visibility::Visible) => return true,
                _ => match world.get::<ChildOf>(at) {
                    Some(parent) => at = parent.parent(),
                    None => return true,
                },
            }
        }
    }

    /// `root` first, then what hangs under it, breadth first.
    fn tree(world: &World, root: Entity) -> Vec<Entity> {
        let mut out = vec![root];
        let mut i = 0;
        while i < out.len() {
            if let Some(children) = world.get::<Children>(out[i]) {
                out.extend(children.iter());
            }
            i += 1;
        }
        out
    }

    /// Hidden, the drawn HUD and a bite's number go unseen, the numbers
    /// patched on underneath; shown again, both are seen. Every node under
    /// the plate and the bubble is judged as drawn, so nothing in them (the
    /// low warning showing at 50 of 290 among them) can show through the
    /// hide (the review's bend: the warning `Visible` passed the root-only
    /// check).
    #[test]
    fn a_hidden_hud_draws_nothing_seen() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::image::ImagePlugin::default(),
        ));
        let stars =
            crate::starfield::Sprites::new(&mut app.world_mut().resource_mut::<Assets<Image>>());
        app.insert_resource(TvAssets {
            font: Handle::default(),
            qr: None,
            stars,
        })
        .insert_resource(TvTheme::default())
        .init_resource::<Hud>()
        .add_systems(Update, draw_hud);
        let shown = HudValues {
            health: Some((50, 290)),
            ..at(1_000)
        };
        assert!(shown.low());
        {
            let mut hud = app.world_mut().resource_mut::<Hud>();
            hud.shown = Some(shown.clone());
            hud.hidden = true;
            hud.damage.push(Damage {
                amount: 14,
                born: Duration::ZERO,
                at: None,
            });
        }
        app.update();
        // The plate's and the bubbles' roots as set, and how many of all
        // their nodes are drawn of how many.
        let seen = |app: &mut App| {
            let world = app.world_mut();
            let mut root = world.query::<(Entity, &HudPart, &Visibility)>();
            let (root, set) = root
                .iter(world)
                .find(|(_, p, _)| **p == HudPart::Root)
                .map(|(e, _, v)| (e, *v))
                .expect("the HUD drawn");
            let mut bubbles = world.query::<(Entity, &DamageBubble, &Visibility)>();
            let bubbles: Vec<(Entity, Visibility)> =
                bubbles.iter(world).map(|(e, _, v)| (e, *v)).collect();
            let nodes: Vec<Entity> = std::iter::once(root)
                .chain(bubbles.iter().map(|b| b.0))
                .flat_map(|e| tree(world, e))
                .collect();
            let drawn = nodes.iter().filter(|e| is_drawn(world, **e)).count();
            let sets: Vec<Visibility> = bubbles.iter().map(|b| b.1).collect();
            ((set, sets), (drawn, nodes.len()))
        };
        let ((set, sets), (drawn, nodes)) = seen(&mut app);
        assert_eq!(
            (set, sets),
            (Visibility::Hidden, vec![Visibility::Hidden]),
            "hidden as drawn"
        );
        assert_eq!(drawn, 0, "none of its {nodes} nodes drawn");
        app.update();
        let ((set, sets), (drawn, nodes)) = seen(&mut app);
        assert_eq!(
            (set, sets),
            (Visibility::Hidden, vec![Visibility::Hidden]),
            "and patched"
        );
        assert_eq!(drawn, 0, "none of its {nodes} nodes drawn");
        app.world_mut().resource_mut::<Hud>().hidden = false;
        app.update();
        let ((set, sets), (drawn, nodes)) = seen(&mut app);
        assert_eq!(
            (set, sets),
            (Visibility::Inherited, vec![Visibility::Inherited])
        );
        assert_eq!(drawn, nodes, "every node drawn, the low warning too");
        app.world_mut().resource_mut::<Hud>().hidden = true;
        app.update();
        let ((set, sets), (drawn, nodes)) = seen(&mut app);
        assert_eq!(
            (set, sets),
            (Visibility::Hidden, vec![Visibility::Hidden]),
            "hidden once drawn"
        );
        assert_eq!(drawn, 0, "none of its {nodes} nodes drawn");
    }

    /// The level-up's arrows (AH.248) on the oracle's frames' clock: none
    /// before 0.45 s after the level rises (its first 27 flips on), then one
    /// sent up every 0.1 s, three to six showing at a time once the stream's
    /// going (from 0.7 s) as the frames show, each rising and fading off the
    /// middle within the spread either side, the oldest highest; the last
    /// gone at 1.95 s (the frames' last at 1.92 s, none at 1.98 s).
    #[test]
    fn the_level_ups_arrows_rise_from_045_s_to_19_s() {
        let at = |ms: u64| level_up_arrows(Duration::from_millis(ms));
        for ms in [0, 200, 449] {
            assert!(at(ms).is_empty(), "{ms} ms");
        }
        let first = at(460);
        assert_eq!(first.len(), 1, "{first:?}");
        assert!(first[0].rise > 0.0 && first[0].rise < 10.0 && first[0].alpha < 0.5);
        let (mut left, mut right) = (0.0f32, 0.0f32);
        for ms in (460..1450).step_by(20) {
            let now = at(ms);
            let least = if ms >= 700 { 3 } else { 1 };
            assert!((least..=6).contains(&now.len()), "{ms} ms: {}", now.len());
            for a in &now {
                assert!(a.x.abs() <= ARROW_SPREAD + 1e-3, "{a:?}");
                assert!(a.rise > 0.0 && a.rise < ARROW_RISE, "{a:?}");
                assert!(a.alpha > 0.0 && a.alpha <= 1.0, "{a:?}");
                left = left.min(a.x);
                right = right.max(a.x);
            }
            assert!(
                now.windows(2).all(|w| w[0].rise > w[1].rise),
                "the oldest highest: {now:?}"
            );
        }
        assert!(left < -ARROW_SPREAD / 2.0 && right > ARROW_SPREAD / 2.0);
        assert!(!at(1_900).is_empty() && at(1_900).iter().all(|a| a.alpha < 0.5));
        assert_eq!(arrows_over(), Duration::from_millis(1_950));
        assert!(at(1_950).is_empty() && at(5_000).is_empty());
    }

    /// A level-up is the same hero's level shown higher than last frame's:
    /// a level or the cap; not his first frame, another hero's coming in
    /// higher, the table arriving or the same level.
    #[test]
    fn the_same_heros_level_rising_is_a_level_up() {
        let (eruptor, tree_rex) = (Entity::from_bits(7), Entity::from_bits(8));
        let mut hud = Hud::default();
        let t = |s: u64| Duration::from_secs(s);
        hud.note_level(eruptor, LevelShown::Level(1), t(1));
        assert_eq!(hud.level_up, None, "his first frame");
        hud.note_level(eruptor, LevelShown::Level(1), t(2));
        assert_eq!(hud.level_up, None, "the same level");
        hud.note_level(eruptor, LevelShown::Level(2), t(3));
        assert_eq!(hud.level_up, Some(t(3)));
        hud.note_level(tree_rex, LevelShown::Level(5), t(4));
        assert_eq!(hud.level_up, Some(t(3)), "another hero coming in higher");
        hud.note_level(tree_rex, LevelShown::Max, t(5));
        assert_eq!(hud.level_up, Some(t(5)), "the cap");
        hud.note_level(tree_rex, LevelShown::Unknown, t(6));
        hud.note_level(tree_rex, LevelShown::Level(3), t(7));
        assert_eq!(hud.level_up, Some(t(5)), "the table arriving");
        hud.note_level(tree_rex, LevelShown::Level(2), t(8));
        assert_eq!(hud.level_up, Some(t(5)), "lower");
    }

    /// The arrows draw in their area on the plate's top edge, where the
    /// original's portrait stands, while they show: as many as
    /// [`level_up_arrows`] gives, each from the arrow's image at the size
    /// the oracle's frames give and placed by it, rising with reduced motion
    /// on (a shot run's) as without; none once they're over, and none with
    /// no image.
    #[test]
    fn the_arrows_draw_over_the_plate_while_they_show() {
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::image::ImagePlugin::default(),
        ));
        let stars =
            crate::starfield::Sprites::new(&mut app.world_mut().resource_mut::<Assets<Image>>());
        let arrow = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        app.insert_resource(TvAssets {
            font: Handle::default(),
            qr: None,
            stars,
        })
        .insert_resource(TvTheme::default())
        .insert_resource(ReducedMotion(true))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )))
        .init_resource::<Hud>()
        .insert_resource(ArrowImage(Some(arrow.clone())))
        .add_systems(Update, (draw_hud, draw_arrows).chain());
        {
            let mut hud = app.world_mut().resource_mut::<Hud>();
            hud.shown = Some(at(1_000));
            hud.level_up = Some(Duration::ZERO);
        }
        let drawn = |app: &mut App| {
            let world = app.world_mut();
            let now = world.resource::<Time<Real>>().elapsed();
            let mut q = world.query::<(&LevelUpArrow, &Node, &ImageNode, &ChildOf)>();
            let got: Vec<(usize, Node, ImageNode, Entity)> = q
                .iter(world)
                .map(|(a, n, i, p)| (a.0, n.clone(), i.clone(), p.parent()))
                .collect();
            (now, got)
        };
        let mut seen = 0;
        for _ in 0..25 {
            app.update();
            let (now, got) = drawn(&mut app);
            let want = level_up_arrows(now);
            assert_eq!(got.len(), want.len(), "at {now:?}");
            for (i, node, image, parent) in &got {
                let a = want[*i];
                assert_eq!(image.image, arrow);
                assert!(image.flip_y, "pointing up");
                assert!((image.color.alpha() - a.alpha).abs() < 1e-6);
                assert_eq!(node.width, px(ARROW_WIDTH));
                assert_eq!(node.height, px(ARROW_HEIGHT));
                assert_eq!(node.left, px(ARROWS_AREA.x / 2.0 + a.x - ARROW_WIDTH / 2.0));
                assert_eq!(node.top, px(ARROWS_AREA.y - ARROW_HEIGHT - a.rise));
                let world = app.world();
                assert_eq!(world.get::<HudPart>(*parent), Some(&HudPart::Arrows));
                let area = world.get::<Node>(*parent).unwrap();
                assert_eq!(area.left, px(ARROWS_AT - ARROWS_AREA.x / 2.0));
                assert_eq!(area.bottom, percent(100));
                let root = world.get::<ChildOf>(*parent).unwrap().parent();
                assert_eq!(world.get::<HudPart>(root), Some(&HudPart::Root));
            }
            seen += got.len();
        }
        assert!(seen > 20, "{seen}");
        let (now, got) = drawn(&mut app);
        assert!(now >= arrows_over() && got.is_empty(), "over at {now:?}");
        // No image: none drawn.
        app.insert_resource(ArrowImage(None));
        app.world_mut().resource_mut::<Hud>().level_up =
            Some(app.world().resource::<Time<Real>>().elapsed());
        for _ in 0..10 {
            app.update();
            assert!(drawn(&mut app).1.is_empty());
        }
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
