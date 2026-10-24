//! play's TV app over the turntables' made-up extract (a made-up Eruptor
//! who walks, his hero side and the synthetic script parts beside it) and
//! the frames that drive it, from play's tests (openspec AQ.33's rule); the
//! dev entry's and the creatures' tests stand on them too (AH.25).

use std::time::{Duration, Instant};

use bevy::input::gamepad::GamepadButton;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use guac_extract::intros::IrFormat;
use guac_ir::Category;
use guac_ir::clip::Clip;
use guac_ir::palette::Palette;
use guac_motion::hero::Hero;
use guacamole::scripts::ScriptRuntime;
use skylander_core::SlotIndex;

use crate::cards::PortalChanged;
use crate::driver::PortalChange;
use crate::hero::HeroBody;
use crate::hero_scripts::HeroRun;
use crate::intro::IntroQueue;
use crate::level::{Level, LevelPlugin, level_channel};
use crate::pad::LevelPads;
use crate::physics::SeamPlugin;
use crate::play::{Mode, Play, PlayPlugin};
use crate::screen::TvScreen;
use crate::test_support::level::{FLOOR, START, lights, write_level};
use crate::test_support::test_pad;
use crate::test_support::turntable::{app, extract, model, placed};
use crate::tv_state::{Current, Redrawn, TvCamera};

pub(crate) const ERUPTOR: u32 = 9;

/// The made-up extract's: a character that doesn't walk, a magic item.
pub(crate) const ONE: u32 = 1;

pub(crate) const ITEM: u32 = 5;

/// Added to the roster by [`play_app`].
pub(crate) const SIDEKICK: u32 = 6;

/// On no roster: a figure Giants can't use (a later game's).
pub(crate) const FRYNO: u32 = 0x1c2;

/// Where the made-up hero side holds the top joint along x in the walk,
/// the run and the fall: a pose tells their weights. The idle sways it
/// from 0 to 0.5 over its second.
pub(crate) const WALK_X: f32 = 1.0;

/// Where the made-up attach list puts his hands on the top joint (AH.50).
pub(crate) const HAND_Z: f32 = 0.5;

pub(crate) const RUN_X: f32 = 2.0;

pub(crate) const FALL_X: f32 = 3.0;

/// A made-up hero side for `load` on the turntables' made-up model's
/// skeleton: its walk, run and falling clips Eruptor's lengths (1.0667,
/// 0.6667 and 0.8 s), each holding the top joint at its own x.
pub(crate) fn hero_side(load: &str) -> guac_ir::hero::Hero {
    use guac_ir::actor::{Movement, Shape, Solid};
    use guac_ir::clip::{RotationTrack, Track, VecTrack};
    use guac_ir::hero::{Locomotion, SlotClip};
    let slot = |slot: u16, name: &str, duration: f32, x: f32| {
        Some(SlotClip {
            slot,
            label: None,
            clip: Clip {
                name: name.into(),
                duration,
                frame_duration: 1.0 / 30.0,
                frames: (duration * 30.0).round() as u32 + 1,
                tracks: vec![Track {
                    joint: 1,
                    translation: VecTrack {
                        constant: [x, 2.0, 0.0],
                        spline: None,
                    },
                    rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
                    scale: VecTrack {
                        constant: [1.0; 3],
                        spline: None,
                    },
                }],
            },
        })
    };
    guac_ir::hero::Hero {
        identity: crate::test_support::turntable::identity(),
        load_file: load.into(),
        actor: load.into(),
        scale: [100.0; 3],
        solid: Solid {
            shape: Shape::Sphere,
            solid: true,
            world: true,
            actor_surface: false,
            offset: [0.0; 3],
            rotation: [0.0; 3],
            width: 4.0,
            height: 6.0,
            length: 3.0,
            weight: 10.0,
            rebound: 0.0,
        },
        movement: Movement {
            ground_max_speed: 18.0,
            ground_acceleration: 160.0,
            ground_deceleration: 80.0,
            ground_turn_rate: 360.0,
            air_max_speed: 30.0,
            air_acceleration: 160.0,
            air_deceleration: 20.0,
            air_turn_rate: 360.0,
            gravity_max_speed: 120.0,
            gravity_acceleration: 128.0,
            jump_max_speed: 0.0,
            jump_acceleration: 0.0,
            jump_initial_speed: 0.0,
            motion_type: 0,
            tilts_with_ground: true,
            time_scale: 1.0,
        },
        locomotion: Locomotion {
            walk: slot(1, "MadeUp_Walk.ma", 1.0667, WALK_X),
            jog: None,
            run: slot(3, "MadeUp_Run.ma", 0.6667, RUN_X),
            falling: slot(57, "MadeUp_Falling.ma", 0.8, FALL_X),
            landing: None,
        },
    }
}

/// The turntables' made-up extract with Eruptor's load file on toy 9,
/// a made-up model and hero side of its own, and a chapter 1 on
/// [`write_level`]'s walk; the TV on the portal view, each figure's
/// values followed (AH.1) with no server to send them to.
pub(crate) fn play_app(dir: &std::path::Path) -> (App, crate::level::LevelSender) {
    play_app_with(dir, |_| {})
}

/// [`play_app`] with `more` added before the app's plugins finish building
/// (openspec AH.25: the creatures' bodies, the dev entry).
pub(crate) fn play_app_with(
    dir: &std::path::Path,
    more: impl FnOnce(&mut App),
) -> (App, crate::level::LevelSender) {
    play_app_shelf(dir, &[], more)
}

/// [`play_app`] with more characters on the shelf, each `(toy, load
/// file)` with a made-up model and hero side of its own (AG.27).
pub(crate) fn play_app_walkers(
    dir: &std::path::Path,
    walkers: &[(u32, &str)],
) -> (App, crate::level::LevelSender) {
    play_app_shelf(dir, walkers, |_| {})
}

fn play_app_shelf(
    dir: &std::path::Path,
    walkers: &[(u32, &str)],
    more: impl FnOnce(&mut App),
) -> (App, crate::level::LevelSender) {
    let mut shelf = extract(dir);
    for (toy, load) in std::iter::once((ERUPTOR, "009_Eruptor")).chain(walkers.iter().copied()) {
        let ron = guac_ir::to_ron(&model(load)).unwrap();
        std::fs::write(dir.join(IrFormat::Ron.model_path(load)), ron).unwrap();
        let hero = IrFormat::Ron.hero_path(load);
        std::fs::create_dir_all(dir.join(&hero).parent().unwrap()).unwrap();
        std::fs::write(dir.join(hero), guac_ir::to_ron(&hero_side(load)).unwrap()).unwrap();
        shelf.loads.insert(format!("{toy:06x}-0000"), load.into());
        shelf
            .kinds
            .insert(format!("{toy:06x}-0000"), Category::Figure);
    }
    // On the roster beside the made-up extract's: Eruptor, and a
    // sidekick, which Giants counts as a magic item.
    shelf
        .kinds
        .insert(format!("{ERUPTOR:06x}-0000"), Category::Figure);
    shelf
        .kinds
        .insert(format!("{SIDEKICK:06x}-0000"), Category::Sidekick);
    let mut app = app(shelf);
    app.add_message::<Redrawn>()
        .add_message::<PortalChanged>()
        .add_plugins((bevy::input::InputPlugin, crate::input::InputRouter))
        .add_plugins((
            LevelPlugin,
            SeamPlugin,
            PlayPlugin,
            crate::hero_scripts::HeroScriptsPlugin,
            crate::progress::ProgressPlugin,
        ))
        .init_resource::<IntroQueue>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .insert_resource(Current {
            screen: TvScreen::Portal {
                game: "Skylanders Giants".into(),
            },
            since: std::time::Instant::now(),
            clients: 1,
        });
    more(&mut app);
    // Avian's plugins finish building here, as `App::run` would.
    app.finish();
    app.cleanup();
    app.world_mut().spawn((Camera2d, TvCamera));
    let (tx, feed) = level_channel();
    app.insert_resource(feed);
    (app, tx)
}

pub(crate) fn place(app: &mut App, slot: u8, toy: u32, name: &str) {
    let change = placed(slot, toy, name);
    crate::test_support::turntable::set(app, [change.clone()]);
    app.world_mut().write_message(PortalChanged(change));
}

pub(crate) fn clear(app: &mut App, slot: u8) {
    let change = PortalChange::Cleared {
        slot: SlotIndex::new(slot).unwrap(),
    };
    crate::test_support::turntable::set(app, [change.clone()]);
    app.world_mut().write_message(PortalChanged(change));
}

pub(crate) fn mode(app: &App) -> Mode {
    let pads = app.world().resource::<LevelPads>();
    app.world().resource::<Play>().mode(pads)
}

pub(crate) fn level_state(app: &App) -> String {
    app.world().resource::<Level>().report().state.clone()
}

pub(crate) fn hero_model(app: &App) -> Option<&'static str> {
    app.world().resource::<Play>().candidate_model()
}

/// Frames until `done`, up to 5 s of the wall clock (the reading tasks
/// run on it).
pub(crate) fn until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        assert!(
            Instant::now() < deadline,
            "{what}: still {:?}, the level {} ({:?}), the hero's model {:?}",
            mode(app),
            level_state(app),
            app.world().resource::<Level>().report().reason,
            hero_model(app)
        );
        app.update();
    }
}

pub(crate) fn hero(app: &App) -> Hero {
    let e = app.world().resource::<Play>().hero().expect("in the level");
    app.world()
        .entity(e)
        .get::<HeroBody>()
        .unwrap()
        .hero
        .clone()
}

/// AH.24's extract beside [`play_app`]'s: the synthetic script part
/// (its attack), Eruptor's palette with his arrival's slot, the throw's
/// and the second attack's on the made-up skeleton, and the spawn part
/// holding the ball's model.
pub(crate) fn write_attack_parts(dir: &std::path::Path) {
    use guac_formats::script::builder::{
        ATTACK_ARRIVAL_SLOT, ATTACK_BALL_MODEL, ATTACK_SECOND_SLOT, ATTACK_THROW_SLOT,
    };
    use guac_ir::palette::{PaletteSlot, SlotEntry};
    crate::test_support::hero_scripts::write_attack(dir);
    let side = hero_side("009_Eruptor");
    let walk = side.locomotion.walk.unwrap().clip;
    let clip = |name: &str, duration: f32| Clip {
        name: name.into(),
        duration,
        ..walk.clone()
    };
    let palette = Palette {
        identity: crate::test_support::turntable::identity(),
        owner: "009_Eruptor".into(),
        slots: 40,
        entries: vec![
            PaletteSlot {
                slot: ATTACK_ARRIVAL_SLOT as u16,
                name: None,
                label: None,
                entry: SlotEntry::Clip {
                    additive: false,
                    partial: false,
                    layered: false,
                    motion_extraction: 0,
                    clip: clip("MadeUp_Arrival.ma", 0.1),
                },
            },
            PaletteSlot {
                slot: ATTACK_THROW_SLOT as u16,
                name: None,
                label: None,
                entry: SlotEntry::Clip {
                    additive: false,
                    partial: true,
                    layered: false,
                    motion_extraction: 0,
                    clip: clip("MadeUp_Throw.ma", 0.5),
                },
            },
            PaletteSlot {
                slot: ATTACK_SECOND_SLOT as u16,
                name: None,
                label: None,
                entry: SlotEntry::Clip {
                    additive: false,
                    partial: false,
                    layered: false,
                    motion_extraction: 0,
                    clip: clip("MadeUp_Eruption.ma", 2.0),
                },
            },
        ],
        refused: Vec::new(),
    };
    let rel = IrFormat::Ron.palette_path("009_Eruptor");
    std::fs::create_dir_all(dir.join(&rel).parent().unwrap()).unwrap();
    std::fs::write(dir.join(rel), guac_ir::to_ron(&palette).unwrap()).unwrap();
    // His attach list (AH.50): nodes 0-6 on his model's origin, his
    // hands 7 and 8 on the top joint half a metre ahead of it and behind.
    let node = |bone: &str, joint: Option<u16>, z: f32| guac_ir::nodes::AttachNode {
        name: None,
        bone: Some(bone.into()),
        joint,
        offset: guac_ir::model::Transform {
            translation: [0.0, 0.0, z],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        },
    };
    let mut nodes: Vec<_> = (0..7).map(|_| node("*root*", None, 0.0)).collect();
    nodes.push(node("top", Some(1), HAND_Z));
    nodes.push(node("top", Some(1), -HAND_Z));
    let nodes = guac_ir::nodes::AttachNodes {
        identity: crate::test_support::turntable::identity(),
        load_file: "009_Eruptor".into(),
        nodes,
    };
    let rel = IrFormat::Ron.nodes_path("009_Eruptor");
    std::fs::create_dir_all(dir.join(&rel).parent().unwrap()).unwrap();
    std::fs::write(dir.join(rel), guac_ir::to_ron(&nodes).unwrap()).unwrap();
    let key = guacamole::census::SPAWNS[0].key(false).unwrap();
    let material = guacamole::census::material_for(&key);
    // Its textures, one 4x4 BC1 block of one colour each: built here.
    let ktx2 = guac_formats::texture::Texture {
        format: guac_formats::texture::Format::Bc1RgbaUnorm,
        width: 4,
        height: 4,
        levels: vec![vec![0x1F, 0xF8, 0x1F, 0xF8, 0, 0, 0, 0]],
    }
    .to_ktx2();
    for t in &material.textures {
        let path = dir.join(&t.texture);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &ktx2).unwrap();
    }
    let mut segment = key.mesh.triangle(0);
    segment.positions = vec![[0.0, 0.0, 0.1], [0.1, 0.0, 0.1], [0.0, 0.1, 0.1]];
    for archive in guac_extract::scripts::spawn_archives() {
        let props = if archive.contains("009_eruptor") {
            vec![guac_ir::level::Prop {
                path: ATTACK_BALL_MODEL.into(),
                skeleton: None,
                materials: vec![guac_ir::level::LevelMaterial::Multi2(material.clone())],
                segments: vec![segment.clone()],
                idle: None,
            }]
        } else {
            Vec::new()
        };
        let spawns = guac_ir::spawn::SpawnModels {
            identity: crate::test_support::turntable::identity(),
            archive: archive.clone(),
            props,
            refused: Vec::new(),
        };
        let rel = IrFormat::Ron.spawn_path(&archive);
        std::fs::create_dir_all(dir.join(&rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), guac_ir::to_ron(&spawns).unwrap()).unwrap();
    }
}

/// The hero's join, once he's in.
pub(crate) fn join(app: &App) -> Option<String> {
    let e = app.world().resource::<Play>().hero()?;
    app.world()
        .entity(e)
        .get::<HeroRun>()
        .map(|r| r.join.name().to_owned())
}

/// The extract's progression tables beside [`write_attack_parts`]'s:
/// what lays his figure's words out.
pub(crate) fn write_figure_rows(dir: &std::path::Path) {
    let p = crate::test_support::hero_scripts::progression_rows();
    let rel = IrFormat::Ron.progression_path();
    std::fs::write(dir.join(rel), guac_ir::to_ron(&p).unwrap()).unwrap();
}

/// Eruptor in the level on AH.24's synthetic part, his scripts joined:
/// the app and the pad that took him over.
pub(crate) fn joined_eruptor(tmp: &std::path::Path) -> (App, Entity) {
    let (mut app, tx) = play_app(tmp);
    write_attack_parts(tmp);
    tx.send(write_level(tmp, &lights())).unwrap();
    let pad = test_pad::connect(&mut app, None, None);
    until(&mut app, "the level and its scripts", |a| {
        level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
    });
    place(&mut app, 0, ERUPTOR, "Eruptor");
    until(&mut app, "the level", |a| mode(a) != Mode::Portal);
    test_pad::press(&mut app, pad, GamepadButton::East);
    until(&mut app, "his scripts", |a| {
        join(a).as_deref() == Some("joined")
    });
    (app, pad)
}

/// A number of the part's files by its name, as the run holds it; none
/// until something has made it.
pub(crate) fn file_int(app: &App, name: &str) -> Option<guac_script::Value> {
    let vm = &app.world().resource::<ScriptRuntime>().vm;
    let g = vm.program().global_named(name)?;
    vm.global_peek(g)
}

/// The first player's slot's `entered` (`Portal - Enter Object`'s and
/// `Portal - Exit Object`'s mark), as the run holds it.
pub(crate) fn entered(app: &App) -> Option<guac_script::Value> {
    let vm = &app.world().resource::<ScriptRuntime>().vm;
    let g = vm.program().global_named("portal control data")?;
    let pcd = vm.global_peek(g)?;
    let slot = vm
        .field(pcd, "loaded objects")
        .and_then(|s| vm.set(vm.value(s)).and_then(|e| e.first().copied()))?;
    vm.field(vm.value(slot), "entered").map(|v| vm.value(v))
}

/// A placement of the level's part by its name: its object and handle.
pub(crate) fn placement_named(app: &App, name: &str) -> (guac_script::Obj, guac_script::Handle) {
    let source = app.world().resource::<crate::hero_scripts::ScriptSource>();
    let obj = source
        .files()
        .placed
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no {name}"))
        .obj;
    let world = app.world().resource::<guacamole::scripts::ScriptWorld>();
    (obj, world.handles()[&obj])
}

/// The portal manager's seconds of ticks: its native tick counts each step
/// it runs, none once it's stopped.
pub(crate) fn pm_seconds(app: &App) -> f32 {
    let w = app.world();
    let vm = &w.resource::<ScriptRuntime>().vm;
    let world = w.resource::<guacamole::scripts::ScriptWorld>();
    let pm = guacamole::scripts::portal_manager_placement(vm, world).expect("it runs");
    vm.active_seconds(pm).unwrap()
}

/// The scripts' log's last step.
pub(crate) fn log_last(app: &App) -> u64 {
    app.world()
        .resource::<crate::hero_scripts::ScriptLog>()
        .last()
}

/// Where AH.25's synthetic creature stands: 3.2 m ahead of the start along
/// the hero's facing, on the floor, where his ball comes down.
pub(crate) const CREATURE_AT: [f32; 3] = [START[0], FLOOR, START[2] + 3.2];

/// AH.25's synthetic triggers: 10 m to either side of the start and 10 m
/// back, on the floor, 20 m apart.
pub(crate) const TRIGGERS: [[f32; 3]; 2] = [
    [START[0] - 10.0, FLOOR, START[2] - 10.0],
    [START[0] + 10.0, FLOOR, START[2] - 10.0],
];

/// [`play_app`] with the creatures' bodies and the dev entry, on AH.24's
/// synthetic part with AH.25's fight as the level's entry and the figure's
/// rows: `asked` asked over the entry's mailbox once the level's run is
/// made (none: no entry), then Eruptor placed and taken in, his scripts
/// joined. The app and the pad that took him over.
pub(crate) fn fight_app(
    tmp: &std::path::Path,
    asked: Option<crate::dev_entry::Asked>,
) -> (App, Entity) {
    let asks = crate::dev_entry::EntryAsks::default();
    let entry = crate::dev_entry::plugin(true, asks.clone()).expect("a dev build's entry");
    let (mut app, tx) = play_app_with(tmp, |app| {
        app.add_plugins((crate::creatures::CreaturesPlugin, entry));
    });
    write_attack_parts(tmp);
    write_figure_rows(tmp);
    crate::test_support::hero_scripts::write_fight(tmp, CREATURE_AT, TRIGGERS);
    let source = write_level(tmp, &lights());
    write_creature_model(tmp);
    tx.send(source).unwrap();
    let pad = test_pad::connect(&mut app, None, None);
    until(&mut app, "the level and its scripts", |a| {
        level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
    });
    if let Some(asked) = asked {
        *asks.0.lock().unwrap() = Some(asked);
        app.update();
        app.update();
    }
    place(&mut app, 0, ERUPTOR, "Eruptor");
    until(&mut app, "the level", |a| mode(a) != Mode::Portal);
    test_pad::press(&mut app, pad, GamepadButton::East);
    until(&mut app, "his scripts", |a| {
        join(a).as_deref() == Some("joined")
    });
    (app, pad)
}

/// AG.27's synthetic walker on the shelf: his toy.
pub(crate) const WALKER_TOY: u32 = 26;

/// [`fight_app`] with AG.27's synthetic walker placed and taken in instead
/// of Eruptor (openspec AH.167), his scripts joined: the app and the pad.
pub(crate) fn fight_app_walker(
    tmp: &std::path::Path,
    asked: Option<crate::dev_entry::Asked>,
) -> (App, Entity) {
    use guac_formats::script::builder::WALKER_LOAD;
    let asks = crate::dev_entry::EntryAsks::default();
    let entry = crate::dev_entry::plugin(true, asks.clone()).expect("a dev build's entry");
    let (mut app, tx) = play_app_shelf(tmp, &[(WALKER_TOY, WALKER_LOAD)], |app| {
        app.add_plugins((crate::creatures::CreaturesPlugin, entry));
    });
    write_attack_parts(tmp);
    write_figure_rows(tmp);
    // His palette, Eruptor's under his name: what the join sets on his
    // minion's template.
    let rel = IrFormat::Ron.palette_path("009_Eruptor");
    let mut palette: Palette =
        guac_ir::from_ron(&std::fs::read_to_string(tmp.join(rel)).unwrap()).unwrap();
    palette.owner = WALKER_LOAD.into();
    std::fs::write(
        tmp.join(IrFormat::Ron.palette_path(WALKER_LOAD)),
        guac_ir::to_ron(&palette).unwrap(),
    )
    .unwrap();
    crate::test_support::hero_scripts::write_fight_with_walker(tmp, CREATURE_AT, TRIGGERS);
    let source = write_level(tmp, &lights());
    write_creature_model(tmp);
    tx.send(source).unwrap();
    let pad = test_pad::connect(&mut app, None, None);
    until(&mut app, "the level and its scripts", |a| {
        level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
    });
    if let Some(asked) = asked {
        *asks.0.lock().unwrap() = Some(asked);
        app.update();
        app.update();
    }
    place(&mut app, 0, WALKER_TOY, "Synthetic");
    until(&mut app, "the level", |a| mode(a) != Mode::Portal);
    test_pad::press(&mut app, pad, GamepadButton::East);
    until(&mut app, "his scripts", |a| {
        join(a).as_deref() == Some("joined")
    });
    (app, pad)
}

/// Where the made-up creature's battle clip holds its top joint, in its
/// model's space: its reference pose holds it at (0, 1, 0).
pub(crate) const BATTLE_TOP: [f32; 3] = [1.5, 1.0, 0.0];

/// AH.25's creature's drawing beside [`write_level`]'s parts: its placement
/// in the level's placements part naming its model, a made-up skinned
/// model at that path in the props part (a root and a top joint a metre up,
/// a triangle on the census's first creature key over the root), and its
/// palette in the level's palettes part, whose battle slot holds the top
/// joint at [`BATTLE_TOP`].
pub(crate) fn write_creature_model(dir: &std::path::Path) {
    use guac_formats::script::builder::{FIGHT_BATTLE_SLOT, FIGHT_CREATURE, FIGHT_CREATURE_MODEL};
    use guac_ir::clip::{RotationTrack, Track, VecTrack};
    use guac_ir::model::{Joint, Skeleton, Transform as IrTransform};
    use guac_ir::palette::{LevelPalettes, PaletteSlot, SlotEntry};
    let key = guacamole::census::CREATURES[0].key(true).unwrap();
    let material = guacamole::census::material_for(&key);
    let joint = |name: &str, parent: Option<u16>, y: f32| Joint {
        name: name.into(),
        parent,
        reference: IrTransform {
            translation: [0.0, y, 0.0],
            ..IrTransform::IDENTITY
        },
        inverse_bind: bevy::math::Mat4::from_translation(Vec3::new(0.0, -y, 0.0))
            .to_cols_array_2d(),
    };
    let prop = guac_ir::level::Prop {
        path: FIGHT_CREATURE_MODEL.into(),
        skeleton: Some(Skeleton {
            joints: vec![joint("root", None, 0.0), joint("top", Some(0), 1.0)],
        }),
        materials: vec![guac_ir::level::LevelMaterial::Multi2(material)],
        segments: vec![key.mesh.triangle(0)],
        idle: None,
    };
    let placement = guac_ir::level::Placement {
        object: 40,
        id: 0,
        name: FIGHT_CREATURE.into(),
        script: Some("Synthetic_Fighter.ai".into()),
        position: CREATURE_AT,
        rotation: [0.0; 3],
        scale: [100.0; 3],
        model: Some(FIGHT_CREATURE_MODEL.into()),
        solid: None,
        movement: None,
        waypoints: None,
        activity_state: 4,
        activation_range: 160.0,
    };
    crate::test_support::level::write_props(dir, vec![prop], vec![placement]);
    let clip = Clip {
        name: "MadeUp_Battle.ma".into(),
        duration: 1.0,
        frame_duration: 1.0 / 30.0,
        frames: 31,
        tracks: vec![Track {
            joint: 1,
            translation: VecTrack {
                constant: BATTLE_TOP,
                spline: None,
            },
            rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
            scale: VecTrack {
                constant: [1.0; 3],
                spline: None,
            },
        }],
    };
    let palettes = LevelPalettes {
        identity: crate::test_support::turntable::identity(),
        level: crate::test_support::hero_scripts::LEVEL.into(),
        palettes: vec![Palette {
            identity: crate::test_support::turntable::identity(),
            owner: FIGHT_CREATURE_MODEL.into(),
            slots: 40,
            entries: vec![PaletteSlot {
                slot: FIGHT_BATTLE_SLOT as u16,
                name: None,
                label: None,
                entry: SlotEntry::Clip {
                    additive: false,
                    partial: false,
                    layered: false,
                    motion_extraction: 0,
                    clip,
                },
            }],
            refused: Vec::new(),
        }],
    };
    let rel = IrFormat::Ron.level_path(
        crate::test_support::hero_scripts::LEVEL,
        guac_extract::levels::PALETTES,
    );
    std::fs::write(dir.join(rel), guac_ir::to_ron(&palettes).unwrap()).unwrap();
}
