//! A collected item's box over the level (openspec AS.441; stand-in ledger:
//! `collected-box`): while the scripts hold the level on the box a pickup
//! made ([`guacamole::collected`]), the TV shows what the box says: its
//! title, the item's name and its Continue prompt, read off the call the
//! scripts made, in `global.bld`'s bitmap font on a box of the clean text
//! box's colour where the extract's interface part holds the font, the
//! TV's own look without it.
//!
//! It draws and takes nothing: the router's pads go to the scripts as
//! before, and the press that takes the box off is theirs (the system's
//! update macro reading the first joined player's accept button). It steps
//! aside under the portal's hold, as the completion screen and the
//! yes-or-no modal do, so an alert reads whole. Where the original's box
//! stands and how it slides in is unrecorded; it sits low and centred here
//! so the part rising in the camera's middle stays in view.

use bevy::image::CompressedImageFormats;
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use guacamole::collected::{self, Collected};
use guacamole::scripts::{ScriptRuntime, ScriptWorld};
use serde::Serialize;
use tracing::{info, warn};

use crate::completion::{Art, FONT_ARCHIVE, bitmap_text, load_art_of};
use crate::hero_scripts::ScriptSource;
use crate::pad::{Face, FacesPrinted, LevelPads, prompt};
use crate::pause::{ShownAt, glyph};
use crate::theme::{Role, Step, TvTheme};
use crate::tv_state::{Pen, TvAssets, UiTree};

/// Over the HUD (800), the scripts' sprites (850) and the announcements
/// (860); under the pause overlays (900), so a pause asked during the hold
/// reads over it.
pub(crate) const BOX_Z: i32 = 880;

/// The box's lines in design pixels, its fill (the clean text box's blur
/// backdrop, as the yes-or-no modal's) and its corners.
const TITLE_SIZE: f32 = 40.0;
const NAME_SIZE: f32 = 56.0;
const PROMPT_SIZE: f32 = 34.0;
const BOX_FILL: Color = Color::srgba(0.03, 0.05, 0.16, 0.86);
const BOX_CORNER: f32 = 32.0;

/// Our words where the scripts' strings don't read (no interface model).
pub(crate) const UNREAD_TITLE: &str = "Collected!";
pub(crate) const CONTINUE: &str = "Continue";

/// The box the run holds the level on, as its last step left it.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub(crate) struct BoxUp(pub(crate) Option<Collected>);

/// The box's root, for the collectible it was made for; drawn in the
/// disc's font or not.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct CollectedBox(guac_script::Value, bool);

pub(crate) struct CollectedBoxPlugin;

impl Plugin for CollectedBoxPlugin {
    fn build(&self, app: &mut App) {
        guacamole::frame::ensure(app);
        app.init_resource::<BoxUp>()
            .add_systems(
                FixedUpdate,
                read_box
                    .in_set(guacamole::frame::Frame::Scripts)
                    .after(crate::hero_scripts::step_scripts)
                    .run_if(resource_exists::<ScriptRuntime>)
                    .run_if(resource_exists::<ScriptWorld>),
            )
            .add_systems(Update, (forget_box, show).chain());
    }
}

/// After the scripts' step: the box they hold the level on, if any.
fn read_box(runtime: Res<ScriptRuntime>, world: Res<ScriptWorld>, mut up: ResMut<BoxUp>) {
    let now = collected::up(&runtime.vm, &world);
    if up.0 != now {
        match (&up.0, &now) {
            (_, Some(c)) => info!(
                item = ?c.description.name,
                kind = c.kind,
                "a collected item's box holds the level until a press"
            ),
            (Some(c), None) => {
                info!(item = ?c.description.name, "the collected item's box taken off")
            }
            (None, None) => {}
        }
        up.0 = now;
    }
}

/// With the run taken down (a level change, Restart chapter) the box is
/// forgotten; a run still holding one reads it again at its next step.
fn forget_box(runtime: Option<Res<ScriptRuntime>>, mut ran: Local<bool>, mut up: ResMut<BoxUp>) {
    let gone = std::mem::replace(&mut *ran, runtime.is_some()) && runtime.is_none();
    if gone && up.0.is_some() {
        up.0 = None;
    }
}

/// The box's font off the run's extract, once a session.
fn box_art(
    source: Option<&ScriptSource>,
    images: &mut Assets<Image>,
    device: Option<&RenderDevice>,
) -> Result<Art, String> {
    let formats = device.map_or(CompressedImageFormats::BC, |d| {
        CompressedImageFormats::from_features(d.features())
    });
    match source.and_then(ScriptSource::extract) {
        Some((dir, ir)) => load_art_of(dir, *ir, FONT_ARCHIVE, &[], images, formats),
        None => Err("the run's extract isn't known".into()),
    }
}

/// Whether the box shows: one held on, the level on the TV and the portal
/// not holding it.
pub(crate) fn wanted<'a>(up: &'a BoxUp, pads: Option<&LevelPads>) -> Option<&'a Collected> {
    let shows = pads.is_some_and(|p| p.shows() && !p.held());
    up.0.as_ref().filter(|_| shows)
}

/// Spawns the box for the one held on and despawns one no longer held.
/// Needs the TV's font and theme; headless tests without them draw nothing.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    up: Res<BoxUp>,
    pads: Option<Res<LevelPads>>,
    gamepads: Query<(&Gamepad, Option<&FacesPrinted>)>,
    (assets, theme, time): (Option<Res<TvAssets>>, Option<Res<TvTheme>>, Res<Time<Real>>),
    (source, mut images, device): (
        Option<Res<ScriptSource>>,
        ResMut<Assets<Image>>,
        Option<Res<RenderDevice>>,
    ),
    mut shown: Local<Option<(guac_script::Value, Entity)>>,
    mut art: Local<Option<Result<Art, String>>>,
) {
    let want = wanted(&up, pads.as_deref());
    if shown.map(|(v, _)| v) == want.map(|c| c.collectible) {
        return;
    }
    if let Some((_, entity)) = shown.take()
        && let Ok(mut root) = commands.get_entity(entity)
    {
        root.despawn();
    }
    let (Some(want), Some(assets), Some(theme), Some(pads)) = (want, assets, theme, pads) else {
        return;
    };
    let pen = Pen::new(&assets, &theme);
    let loaded = art.get_or_insert_with(|| {
        let a = box_art(source.as_deref(), &mut images, device.as_deref());
        if let Err(why) = &a {
            warn!(reason = %why, "the collected box's font didn't load: the TV's own look");
        }
        a
    });
    let labels = crate::button_prompt::walking_labels(&pads, &gamepads);
    let entity = spawn(&mut commands, &pen, loaded.as_ref().ok(), want, labels);
    commands.entity(entity).insert(ShownAt(time.elapsed()));
    *shown = Some((want.collectible, entity));
}

/// The box's lines as drawn: its title, the item's name and its prompt
/// (the disc's own with the font, which draws its button; ours without).
pub(crate) fn lines(c: &Collected, art: bool) -> [String; 3] {
    let title = c.title.text.clone().unwrap_or_else(|| UNREAD_TITLE.into());
    let name = c.description.text.clone().unwrap_or_default();
    let prompt = match c.prompt.as_ref().and_then(|p| p.text.clone()) {
        Some(p) if art => p,
        _ => CONTINUE.into(),
    };
    [title, name, prompt]
}

/// The box over the level: low, so the part rising in the camera's middle
/// stays in view.
fn spawn(
    commands: &mut Commands,
    pen: &Pen,
    art: Option<&Art>,
    c: &Collected,
    labels: crate::pad::Labels,
) -> Entity {
    let theme = &pen.theme;
    let [title, name, prompt_line] = lines(c, art.is_some());
    commands
        .spawn((
            CollectedBox(c.collectible, art.is_some()),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::FlexEnd,
                align_items: AlignItems::Center,
                padding: UiRect::bottom(percent(8)),
                ..default()
            },
            GlobalZIndex(BOX_Z),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(20),
                    padding: UiRect::axes(px(64), px(32)),
                    border: UiRect::all(px(3)),
                    border_radius: BorderRadius::all(px(BOX_CORNER)),
                    ..default()
                },
                BackgroundColor(BOX_FILL),
                BorderColor::all(theme.gold),
            ))
            .with_children(|panel| match art {
                Some(a) => {
                    bitmap_text(panel, a, &title, TITLE_SIZE, theme.gold_bright);
                    if !name.is_empty() {
                        bitmap_text(panel, a, &name, NAME_SIZE, Color::WHITE);
                    }
                    bitmap_text(panel, a, &prompt_line, PROMPT_SIZE, Color::WHITE);
                }
                None => {
                    panel.spawn(pen.text(&title, Step::Body, Role::Accent));
                    if !name.is_empty() {
                        panel.spawn(pen.text(&name, Step::Lg, Role::Primary));
                    }
                    panel
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: px(16),
                            ..default()
                        })
                        .with_children(|line| {
                            glyph(line, pen, prompt(labels, Face::South));
                            line.spawn(pen.text(&prompt_line, Step::Body, Role::Primary));
                        });
                }
            });
        })
        .id()
}

/// The box's part of `/api/_test/tv` (and so of a flag's `capture.json`):
/// the strings the scripts handed it and the lines drawn from them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CollectedShot {
    /// `collected type` (1 a ship part).
    pub kind: i32,
    /// The strings the scripts handed it, by their objects' names, and
    /// their text as read.
    pub title_name: Option<String>,
    pub description_name: Option<String>,
    pub prompt_name: Option<String>,
    /// The lines drawn: title, name, prompt; none while it isn't drawn.
    pub texts: Vec<String>,
    /// Drawn in the disc's font, not the TV's own look.
    pub art: bool,
    /// When it was spawned, on the frame log's clock.
    pub shown_ms: Option<f64>,
}

/// What `/api/_test/tv` reads of the box the run holds the level on.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct CollectedShots<'w, 's> {
    up: Option<Res<'w, BoxUp>>,
    boxes: Query<'w, 's, (Entity, &'static CollectedBox, Option<&'static ShownAt>)>,
    tree: UiTree<'w, 's>,
}

impl CollectedShots<'_, '_> {
    /// None while no box holds the level.
    pub(crate) fn shot(&self) -> Option<CollectedShot> {
        let c = self.up.as_ref()?.0.as_ref()?;
        let drawn = self.boxes.iter().find(|(_, b, _)| b.0 == c.collectible);
        Some(CollectedShot {
            kind: c.kind,
            title_name: c.title.name.clone(),
            description_name: c.description.name.clone(),
            prompt_name: c.prompt.as_ref().and_then(|p| p.name.clone()),
            texts: match drawn {
                // The disc's font draws its lines glyph by glyph.
                Some((_, b, _)) if b.1 => lines(c, true)
                    .into_iter()
                    .filter(|l| !l.is_empty())
                    .collect(),
                Some((root, _, _)) => self.tree.walk(root).0,
                None => Vec::new(),
            },
            art: drawn.is_some_and(|(_, b, _)| b.1),
            shown_ms: drawn.and_then(|(_, _, at)| at.map(|a| a.0.as_secs_f64() * 1e3)),
        })
    }
}

#[cfg(test)]
mod tests {
    use guac_server::config::WindowMode;
    use guacamole::collected::Words;

    use bevy::input::gamepad::GamepadButton;

    use super::*;
    use crate::cards::PortalCards;
    use crate::test_support::test_pad;
    use crate::test_support::tv::portal_app;

    /// The test's own words (a disc's string is never in the tree).
    const TITLE: &str = "The test's title";
    const ITEM: &str = "The test's item";

    fn boxed(collectible: u64) -> Collected {
        let w = |name: &str, text: &str| Words {
            name: Some(name.into()),
            text: Some(text.into()),
        };
        Collected {
            collectible: guac_script::Value::Entity(collectible),
            kind: 1,
            title: w("title", TITLE),
            description: w("item", ITEM),
            prompt: Some(w("prompt", "< go")),
            prompt_strings: None,
        }
    }

    /// The TV with the level showing, a pad walking.
    fn box_app() -> (App, Entity) {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        app.add_plugins(CollectedBoxPlugin);
        let walker = test_pad::connect(&mut app, None, None);
        app.update();
        app.world_mut().resource_mut::<LevelPads>().show();
        (app, walker)
    }

    fn shot(app: &mut App) -> Option<CollectedShot> {
        let mut state = bevy::ecs::system::SystemState::<CollectedShots>::new(app.world_mut());
        state.get(app.world()).expect("the shots' params").shot()
    }

    fn holds_the_pads(app: &mut App) -> bool {
        let mut state =
            bevy::ecs::system::SystemState::<crate::input::Modals>::new(app.world_mut());
        state.get(app.world()).expect("the modals' params").hold()
    }

    /// openspec AS.441: a box the run holds the level on is drawn over it,
    /// its title, the item's name and our Continue (the TV's own look: no
    /// extract here, so no font; the prompt's button drawn as the pad's),
    /// and taken down when the run's box goes; it steps aside while the
    /// level's off the TV, and never takes the pads, which go on to the
    /// scripts, whose own press ends the hold.
    #[test]
    fn a_collected_box_is_drawn_while_it_holds_and_takes_no_pads() {
        let (mut app, walker) = box_app();
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(shot(&mut app), None);
        app.world_mut().resource_mut::<BoxUp>().0 = Some(boxed(0x41));
        for _ in 0..3 {
            app.update();
        }
        let s = shot(&mut app).expect("up");
        assert_eq!(s.texts, [TITLE, ITEM, CONTINUE], "{s:?}");
        assert!(!s.art && s.shown_ms.is_some(), "{s:?}");
        assert_eq!(
            (s.title_name.as_deref(), s.description_name.as_deref()),
            (Some("title"), Some("item"))
        );
        assert!(!holds_the_pads(&mut app), "the pads stay the scripts'");
        // The level's layer still on top: Start under the box pauses, as
        // in play (under a modal it pauses nothing).
        test_pad::press(&mut app, walker, GamepadButton::Start);
        for _ in 0..3 {
            app.update();
        }
        let start_paused = |app: &App| {
            app.world()
                .resource::<LevelPads>()
                .seat()
                .is_some_and(|s| s.pause().is_some())
        };
        assert!(start_paused(&app), "the level's layer on top");
        test_pad::press(&mut app, walker, GamepadButton::Start);
        for _ in 0..3 {
            app.update();
        }
        assert!(!start_paused(&app), "Start resumes");

        // The level off the TV: the box steps aside, the run's still held.
        app.world_mut().resource_mut::<LevelPads>().hide();
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(shot(&mut app).map(|s| s.texts), Some(Vec::new()));
        app.world_mut().resource_mut::<LevelPads>().show();
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(shot(&mut app).map(|s| s.texts.len()), Some(3));

        // The press taken: the run's box gone, so is the drawn one.
        app.world_mut().resource_mut::<BoxUp>().0 = None;
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(shot(&mut app), None);
        let mut q = app.world_mut().query::<&CollectedBox>();
        assert_eq!(q.iter(app.world()).count(), 0, "despawned");

        // The run taken down under a box (a level change, Restart chapter):
        // forgotten, a later run's read afresh.
        app.insert_resource(a_run());
        app.world_mut().resource_mut::<BoxUp>().0 = Some(boxed(0x42));
        app.update();
        app.world_mut().remove_resource::<ScriptRuntime>();
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<BoxUp>().0, None);
        assert_eq!(shot(&mut app), None);
    }

    /// A run of no scripts of its own, as the TV installs one (the
    /// yes-or-no modal's tests' own).
    fn a_run() -> ScriptRuntime {
        use std::sync::Arc;

        use guac_formats::igz::Igz;
        use guac_formats::script::{Graph, builder};
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
        ScriptRuntime {
            vm: Vm::new(program, Config::new(Mode::Strict)),
            generator: Generator::seeded(1),
            player_loop: Default::default(),
            steps: 0,
            stopped: Vec::new(),
            entry_stopped: None,
        }
    }

    /// The lines: an unread title ours, the disc's prompt only in its font.
    #[test]
    fn a_boxes_lines_fall_back_to_ours() {
        let mut c = boxed(1);
        assert_eq!(lines(&c, true), [TITLE, ITEM, "< go"]);
        assert_eq!(lines(&c, false), [TITLE, ITEM, CONTINUE]);
        c.title.text = None;
        c.prompt = None;
        assert_eq!(lines(&c, true), [UNREAD_TITLE, ITEM, CONTINUE]);
    }
}
