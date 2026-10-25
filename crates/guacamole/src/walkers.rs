//! Who walks the level (openspec AG.21, AG.27, design decision 1): a
//! character walks only on numbers that are his.
//!
//! - Eruptor's are measured (AG.8's live reads, [`WALKERS`]): his scripts
//!   write the same numbers on the VM, and these stay the walk he falls back
//!   to (stand-in ledger: `walker`) and what draws and builds him.
//! - Every other modelled character walks on his own scripts alone (AG.27):
//!   his file's setup runs on the VM and writes his movement parameters and
//!   his default scale, which the mover update reads. What AG.27 knows of
//!   him before that is his solid as the disc authors it ([`Walk::scripted`]),
//!   and no walk of his own: the fall-back after his script stops in play is
//!   a [`Tuning`] read off the `movement parameters` his scripts had written
//!   into his description ([`Movement`]), never a number of ours beside
//!   them, his fall the mover update's own (openspec AG.173). One whose
//!   setup doesn't run through keeps his card ([`Fallback::Card`], listed
//!   with why).
//!
//! The hero's drawn model and its keys are the card's model (AE's), drawn
//! in [`crate::level_scene::hero_scene`]: the keys the census's card tables
//! hold, so the start's pass has compiled them (`tests/giants_census.rs`).

use guac_ir::clip::Clip;
use guac_ir::palette::Palette;
use guac_motion::locomotion::{Gaits, Locomotion, Loops};
use guac_motion::solid::{HavokSolid, SolidInfo};
use guac_motion::tuning::{self, Tuning};

use guac_script::{Handle, PlacementId, Value, Vm};

/// A character that walks on measured numbers: its load file (the roster's,
/// the model's key) and the numbers it walks on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Walker {
    pub load_file: &'static str,
    pub tuning: Tuning,
    /// Its `CollisionInfo`, before its scale.
    pub shape: SolidInfo,
    /// Its scale in the level, which its solid and its drawn model take.
    pub scale: f32,
    /// Its walk's and run's authored speeds, which its locomotion mixes
    /// them by (AG.30).
    pub gaits: Gaits,
}

impl Walker {
    /// The solid the original builds for it: its shape at its scale.
    pub fn solid(&self) -> Option<HavokSolid> {
        self.shape.havok([self.scale * 100.0; 3])
    }
}

/// Every character that walks: Eruptor on AG.8's numbers (21.5 ft/s,
/// 1440°/s, a 4 x 6 x 3 ft sphere at 150 %) and AG.11's gaits (walk 3.658,
/// run 6.401 m/s).
pub const WALKERS: &[Walker] = &[Walker {
    load_file: "009_Eruptor",
    tuning: Tuning::ERUPTOR,
    shape: tuning::ERUPTOR_SHAPE,
    scale: tuning::ERUPTOR_SCALE,
    gaits: Gaits::ERUPTOR,
}];

/// `None` for a character that keeps the portal view.
pub fn walker(load_file: &str) -> Option<&'static Walker> {
    WALKERS.iter().find(|w| w.load_file == load_file)
}

/// Where a walker's numbers come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Measured in the oracle ([`WALKERS`]); his scripts write the same on
    /// the VM.
    Measured,
    /// Written by his own scripts on the VM (AG.27).
    Scripts,
}

/// What the TV builds a hero from: [`Walker`]'s fields, owned, for a
/// character whose numbers aren't compiled in.
#[derive(Debug, Clone, PartialEq)]
pub struct Walk {
    pub load_file: String,
    pub origin: Origin,
    pub tuning: Tuning,
    /// His `CollisionInfo`, before his scale.
    pub shape: SolidInfo,
    pub scale: f32,
    pub gaits: Gaits,
}

impl Walk {
    /// A measured walker as it is.
    pub fn measured(w: &Walker) -> Walk {
        Walk {
            load_file: w.load_file.to_owned(),
            origin: Origin::Measured,
            tuning: w.tuning,
            shape: w.shape,
            scale: w.scale,
            gaits: w.gaits,
        }
    }

    /// A character on his own scripts (AG.27), before they've run: his
    /// solid as the disc authors it at the file's 100 % (his scripts set
    /// his scale), no walk of his own (the controller's tuning is read off
    /// his scripts if they stop, [`Movement`]) and no gaits (his
    /// locomotion is the VM's, AH.49).
    pub fn scripted(load_file: &str, solid: &guac_ir::actor::Solid) -> Walk {
        let shape = SolidInfo {
            kind: crate::props::solid_kind(solid.shape),
            offset: solid.offset,
            rotation: solid.rotation,
            width: solid.width,
            height: solid.height,
            length: solid.length,
        };
        Walk {
            load_file: load_file.to_owned(),
            origin: Origin::Scripts,
            tuning: Tuning {
                width: solid.width,
                ..Tuning::NONE
            },
            shape,
            scale: 1.0,
            gaits: Gaits {
                walk_speed: 0.0,
                run_speed: 0.0,
            },
        }
    }

    /// The solid the original builds for him: his shape at his scale.
    pub fn solid(&self) -> Option<HavokSolid> {
        self.shape.havok([self.scale * 100.0; 3])
    }
}

/// His scale as his scripts set it on his actor (percent per axis, AG.27),
/// as a fraction: `None` for one that isn't uniform or isn't set, which no
/// solid is built at.
pub fn uniform_scale(scale: [f32; 3]) -> Option<f32> {
    (scale[0] > 0.0 && scale[0] == scale[1] && scale[1] == scale[2]).then(|| scale[0] / 100.0)
}

/// What a scripted walker's setup wrote into his description's `movement
/// parameters` (AG.27), each member's `value` in the scripts' own units
/// (ft/s, ft/s², °/s): what the mover update reads, and what the fall-back
/// walks him on once his script stops. Read, never chosen. His actor's own
/// `ActorParameters` aren't these: the engine's, they stay the
/// unconstrained 10,000 ft/s on every character (AG.12's live read). His
/// description's gravity isn't read: the mover update falls every mover at
/// its own 136.21 ft/s² to 120 ft/s ([`tuning::MOVER_GRAVITY`], openspec
/// AG.173), so the fall-back falls him there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Movement {
    pub ground_max_speed: f32,
    pub ground_acceleration: f32,
    pub ground_deceleration: f32,
    pub ground_turn_rate: f32,
    pub air_deceleration: f32,
}

impl Movement {
    /// The members read, in the fields' order.
    pub const MEMBERS: [&'static str; 5] = [
        "ground max speed",
        "ground acceleration",
        "ground deceleration",
        "ground turn rate",
        "air deceleration",
    ];

    /// `minion`'s, off his `description` local on `vm`; `None` while any of
    /// [`Self::MEMBERS`] holds no number (a setup that hasn't written them).
    pub fn read(vm: &Vm, minion: PlacementId) -> Option<Movement> {
        let d = vm.value(vm.local(minion, "description")?);
        let params = vm.value(vm.field(d, "movement parameters")?);
        let member = |name: &str| {
            let m = vm.value(vm.field(params, name)?);
            vm.value(vm.field(m, "value")?).as_f32()
        };
        let [a, b, c, d, e] = Self::MEMBERS;
        Some(Movement {
            ground_max_speed: member(a)?,
            ground_acceleration: member(b)?,
            ground_deceleration: member(c)?,
            ground_turn_rate: member(d)?,
            air_deceleration: member(e)?,
        })
    }

    /// AG.16's controller's numbers off these, under his solid's authored
    /// `width` (feet), falling as every mover falls (AG.173).
    pub fn tuning(&self, width: f32) -> Tuning {
        Tuning {
            max_speed: self.ground_max_speed,
            push: self.ground_acceleration,
            let_go: self.ground_deceleration,
            air_let_go: self.air_deceleration,
            turn_rate: self.ground_turn_rate,
            gravity: tuning::MOVER_GRAVITY,
            terminal: tuning::MOVER_TERMINAL,
            width,
        }
    }
}

/// One state of a walker's locomotion as his scripts define it on the VM
/// (`Minion - Define Locomotion States`, each `Anim - Add Locomotion State`
/// it calls): the palette slot of its clip and its movement speed, ft/s
/// (the speed he plays the clip at its own rate, at his scale where his
/// call adjusts for it), as `global.bld`'s locomotion script keeps them for
/// him and picks his clips by (AH.49, AG.161).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocomotionState {
    pub slot: Option<u32>,
    pub speed: f32,
}

/// `actor`'s locomotion states in the order his scripts defined them: the
/// base states of the entry `Macros_Animation.ai`'s `locomotions` keeps
/// for him (the newest, where his locomotion was initialized again). `None`
/// until his setup has initialized his locomotion, or where no locomotion
/// script runs.
pub fn locomotion_states(
    vm: &Vm,
    world: &crate::scripts::ScriptWorld,
    actor: Handle,
) -> Option<Vec<LocomotionState>> {
    let animation = crate::scripts::running(vm, world, "Macros_Animation.ai")?;
    let entries = vm.set(vm.local(animation, "locomotions")?)?;
    let field = |v: Value, name: &str| vm.field(v, name).map(|f| vm.value(f));
    let entry = entries
        .iter()
        .rev()
        .map(|e| vm.value(*e))
        .find(|e| field(*e, "character") == Some(Value::Entity(actor)))?;
    let states = vm.set(field(entry, "base locomotion states")?)?;
    Some(
        states
            .iter()
            .map(|s| {
                let s = vm.value(*s);
                LocomotionState {
                    slot: field(s, "animation")
                        .and_then(|a| world.clip_named(a))
                        .map(|(_, slot)| slot),
                    speed: field(s, "movement speed")
                        .and_then(|v| v.as_f32())
                        .unwrap_or(0.0),
                }
            })
            .collect(),
    )
}

/// The walk and the run AG.30's locomotion mixes by speed off a walker's own
/// locomotion states ([`locomotion_states`]; openspec AG.219): his walk at
/// the speed of the state whose clip is his hero side's walk, his run at
/// the one's whose clip is his run (`walk` and `run` each his hero side's
/// clip name and its slot, `clip` naming a state's slot's clip; the faster
/// where two play one), so AG.30's draws only clips both his scripts and
/// his hero side hold. A state draws one gait: the one his hero side puts
/// at its slot, else the one whose clip it plays (AH.198's review: Double
/// Trouble's and Drill Sergeant's walk and run are one clip, at slots 1 and
/// 3, and their scripts define only a run, on 3). With one of the two his
/// scripts define (a giant's walk, a runner's run) that one's speed is
/// both; `None` with neither.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptedGaits {
    pub gaits: Gaits,
    /// Whether his scripts define his hero side's walk and run.
    pub walk: bool,
    pub run: bool,
}

impl ScriptedGaits {
    pub fn of<'a>(
        states: &[LocomotionState],
        clip: impl Fn(u32) -> Option<&'a str>,
        walk: Option<(&str, Option<u32>)>,
        run: Option<(&str, Option<u32>)>,
    ) -> Option<ScriptedGaits> {
        // The states playing `gait`'s clip, at its own slot only where
        // `at_slot`, leaving out those another gait has at its own.
        let playing = |gait: Option<(&str, Option<u32>)>, at_slot: bool, taken: &[usize]| {
            let Some((name, slot)) = gait else {
                return Vec::new();
            };
            (0..states.len())
                .filter(|i| {
                    let s = &states[*i];
                    s.speed > 0.0
                        && !taken.contains(i)
                        && s.slot.and_then(&clip) == Some(name)
                        && (!at_slot || (slot.is_some() && s.slot == slot))
                })
                .collect::<Vec<usize>>()
        };
        let (mut ws, mut rs) = (playing(walk, true, &[]), playing(run, true, &[]));
        let taken: Vec<usize> = ws.iter().chain(&rs).copied().collect();
        if ws.is_empty() {
            ws = playing(walk, false, &taken);
        }
        if rs.is_empty() {
            rs = playing(run, false, &taken);
        }
        let speed_of = |i: Vec<usize>| i.into_iter().map(|i| states[i].speed).reduce(f32::max);
        let (w, r) = (speed_of(ws), speed_of(rs));
        let (walk_speed, run_speed) = match (w, r) {
            (Some(w), Some(r)) => (w, r),
            (Some(one), None) | (None, Some(one)) => (one, one),
            (None, None) => return None,
        };
        Some(ScriptedGaits {
            gaits: Gaits {
                walk_speed,
                run_speed,
            },
            walk: w.is_some(),
            run: r.is_some(),
        })
    }
}

/// A walker's AG.30 locomotion on his own scripts' (openspec AG.219): AG.30's
/// mix at the speeds his states give his hero side's `walk` and `run`
/// ([`ScriptedGaits`]; each with the slot his hero side gives it), over
/// those of the two clips his scripts play (found in his `palette`), his
/// idle and his fall as `loops` has them. `None` where his scripts play
/// neither: he keeps his idle.
pub fn stand_in(
    states: &[LocomotionState],
    palette: &Palette,
    walk: Option<(&Clip, Option<u16>)>,
    run: Option<(&Clip, Option<u16>)>,
    loops: Loops,
) -> Option<Locomotion> {
    fn side(g: Option<(&Clip, Option<u16>)>) -> Option<(&str, Option<u32>)> {
        g.map(|(c, s)| (c.name.as_str(), s.map(u32::from)))
    }
    let g = ScriptedGaits::of(
        states,
        |slot| {
            palette
                .clip(u16::try_from(slot).ok()?)
                .map(|c| c.name.as_str())
        },
        side(walk),
        side(run),
    )?;
    Some(Locomotion::new(
        g.gaits,
        Loops {
            walk: walk.filter(|_| g.walk).map(|(c, _)| c.duration),
            run: run.filter(|_| g.run).map(|(c, _)| c.duration),
            ..loops
        },
    ))
}

/// Where a hero's scripts stood when they went off, and so what he does
/// (stand-in ledger: `walker`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// His join didn't complete: his setup stopped on a member, his file
    /// wouldn't join, his minion didn't activate.
    Joining,
    /// His script stopped once he was in.
    Playing,
}

/// What a hero does once his scripts are off.
#[derive(Debug, Clone, PartialEq)]
pub enum Fallback {
    /// Walks on AG.16's controller on the numbers he carries: Eruptor's
    /// measured ones.
    Stand,
    /// Walks on AG.16's controller on what his scripts had set (the stage
    /// is [`Stage::Playing`]).
    OnWhatTheySet,
    /// Keeps his card, listed with why: he never walks on a number of ours.
    Card(String),
}

/// The fallback for a hero of `origin` whose scripts went off at `stage`
/// for `why` (AG.27; stand-in ledger: `walker`).
pub fn fallback(origin: Origin, stage: Stage, why: &str) -> Fallback {
    match (origin, stage) {
        (Origin::Measured, _) => Fallback::Stand,
        (Origin::Scripts, Stage::Playing) => Fallback::OnWhatTheySet,
        (Origin::Scripts, Stage::Joining) => Fallback::Card(why.to_owned()),
    }
}

/// A character kept on his card and why (AG.27), as `/api/_test/tv` lists
/// them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Kept {
    pub load_file: String,
    pub reason: String,
    /// The portal slot (1-based, as the phone numbers slots) of a figure
    /// kept by what happened in the level (a join that didn't run through,
    /// openspec AH.166: the TV takes him out the frame it fails, so a read
    /// finds no hero and this is where his reason is); none for the part's
    /// own list, which no join reached.
    pub slot: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eruptor_walks_on_his_live_numbers_and_the_rest_on_their_scripts() {
        let eruptor = walker("009_Eruptor").expect("Eruptor walks");
        assert_eq!(eruptor.tuning, Tuning::ERUPTOR);
        assert_eq!(eruptor.solid(), Some(tuning::eruptor_solid()));
        assert_eq!(eruptor.scale, 1.5);
        assert_eq!(eruptor.gaits, Gaits::ERUPTOR);
        // The measured table is Eruptor's alone; every other character's
        // numbers are his scripts' (AG.27).
        assert!(walker("070_TreeRex").is_none());
        assert!(walker("009_e2e").is_none());
        assert_eq!(WALKERS.len(), 1);
        assert_eq!(Walk::measured(eruptor).solid(), eruptor.solid());
    }

    /// AG.27: what a scripted walker's setup wrote into his description
    /// reads as the controller's numbers in the scripts' own units, his
    /// width from his authored solid (Tree Rex's, AG.12's), and a scale as a
    /// fraction; one that isn't uniform, or isn't set, reads as none.
    /// AG.173: he falls as every mover does, 136.21 ft/s² to 120 ft/s, not
    /// at the 128 his description holds.
    #[test]
    fn his_movement_parameters_are_the_controllers_numbers() {
        let tree_rex = Movement {
            ground_max_speed: 20.0,
            ground_acceleration: 160.0,
            ground_deceleration: 80.0,
            ground_turn_rate: 360.0,
            air_deceleration: 50.0,
        };
        assert_eq!(
            tree_rex.tuning(14.0),
            Tuning {
                max_speed: 20.0,
                push: 160.0,
                let_go: 80.0,
                air_let_go: 50.0,
                turn_rate: 360.0,
                gravity: 136.21,
                terminal: 120.0,
                width: 14.0,
            }
        );
        // Eruptor's description (AG.27's disc read) is AG.8's live table, and
        // read through it his fall is AG.10's measured one.
        let eruptor = Movement {
            ground_max_speed: 21.5,
            ground_turn_rate: 1440.0,
            ..tree_rex
        };
        assert_eq!(eruptor.tuning(Tuning::ERUPTOR.width), Tuning::ERUPTOR);
        assert_eq!(uniform_scale([100.0; 3]), Some(1.0));
        assert_eq!(uniform_scale([150.0; 3]), Some(1.5));
        assert_eq!(uniform_scale([150.0, 100.0, 150.0]), None);
        assert_eq!(uniform_scale([0.0; 3]), None);
    }

    /// AG.173: a walker fallen back on what his scripts set falls off a ledge
    /// as AG.10 measured Eruptor's fall, frame for frame: 136.21 ft/s² over
    /// the step before, the same on Tree Rex's walk as on Eruptor's measured
    /// table (his description's 128 would gain 0.137 ft/s a frame less).
    #[test]
    fn the_fall_backs_fall_is_the_measured_one() {
        use guac_motion::parry::ParryWorld;
        use guac_motion::scenarios::{bodies, open_ground};
        use guac_motion::seam::Vec3;
        let world = ParryWorld::new(&bodies(&open_ground(), &[])).unwrap();
        let tree_rex = Movement {
            ground_max_speed: 20.0,
            ground_acceleration: 160.0,
            ground_deceleration: 80.0,
            ground_turn_rate: 360.0,
            air_deceleration: 50.0,
        };
        let solid = tuning::eruptor_solid();
        let at = Vec3::new(0.0, 6.0, 0.0);
        let mut fallen = guac_motion::hero::Hero::new(solid, tree_rex.tuning(14.0), at, 0.0);
        let mut measured = guac_motion::hero::Hero::new(solid, Tuning::ERUPTOR, at, 0.0);
        let input = guac_motion::hero::Input::stick(guac_motion::stick::Stick::CENTRED, 0.0);
        let dt = tuning::NOMINAL_STEP;
        // Up in the air the whole way down to the crest's band, where each
        // walk is cut to its own top speed.
        let mut falling = 0;
        while fallen.footing.high() {
            let before = fallen.walk.velocity.y;
            fallen.step(&world, &input, dt, &[]);
            measured.step(&world, &input, dt, &[]);
            assert_eq!(
                (fallen.walk.velocity, fallen.pivot),
                (measured.walk.velocity, measured.pivot),
                "frame {falling}: as the measured fall"
            );
            let gained = before - fallen.walk.velocity.y;
            assert!(
                (gained - 136.21 * dt).abs() < 1e-4,
                "frame {falling}: {gained} ft/s gained"
            );
            falling += 1;
            assert!(falling < 120, "still falling");
        }
        assert!(falling > 20, "{falling} frames up in the air");
    }

    /// AG.27: only Eruptor falls back to numbers the build holds; a walker
    /// on his scripts walks on what they set once his script stops in play,
    /// and keeps his card when his join never ran through, the reason kept.
    #[test]
    fn the_fallback_never_walks_a_scripted_walker_on_a_number_of_ours() {
        use Origin::{Measured, Scripts};
        for stage in [Stage::Joining, Stage::Playing] {
            assert_eq!(fallback(Measured, stage, "stopped"), Fallback::Stand);
        }
        assert_eq!(
            fallback(Scripts, Stage::Playing, "stopped"),
            Fallback::OnWhatTheySet
        );
        assert_eq!(
            fallback(Scripts, Stage::Joining, "unmodelled member x"),
            Fallback::Card("unmodelled member x".into())
        );
    }

    /// AG.27: a scripted walker before his scripts run is his authored solid
    /// at the file's scale and no tuning, no gaits: nothing of ours to walk
    /// on.
    #[test]
    fn a_scripted_walk_starts_with_nothing_of_ours() {
        use guac_ir::actor::{Shape, Solid};
        let solid = Solid {
            shape: Shape::Capsule,
            solid: true,
            world: true,
            actor_surface: false,
            offset: [0.0, 1.0, 0.0],
            rotation: [0.0; 3],
            width: 14.0,
            height: 21.25,
            length: 14.0,
            weight: 1.0,
            rebound: 0.0,
        };
        let w = Walk::scripted("070_TreeRex", &solid);
        assert_eq!(w.origin, Origin::Scripts);
        assert_eq!(w.scale, 1.0);
        assert_eq!(
            w.tuning,
            Tuning {
                width: 14.0,
                ..Tuning::NONE
            }
        );
        assert_eq!((w.gaits.walk_speed, w.gaits.run_speed), (0.0, 0.0));
        assert_eq!(w.shape.width, 14.0);
        assert!(w.solid().is_some());
    }

    /// AG.219: AG.30's gaits for him are his scripts' states' speeds, each
    /// gait the state whose clip his hero side names for it (on the dump's
    /// own shapes: Eruptor's walk and run; Ninjini's walk alone; a Bouncer's
    /// hero side holding a run no state of his plays; a runner with no
    /// walk; one clip for all three gaits), by name, not slot; a state
    /// drawing one gait, the one his hero side puts at its slot where one
    /// does (AH.198's review: Double Trouble's walk and run one clip at
    /// slots 1 and 3, his scripts' one state on 3, his run alone).
    #[test]
    fn the_stand_ins_gaits_are_his_scripts_states() {
        let state = |slot: u32, speed: f32| LocomotionState {
            slot: Some(slot),
            speed,
        };
        let names = ["Idle.ma", "Walk.ma", "Jog.ma", "Run.ma", "Run.ma"];
        let clip = |slot: u32| names.get(slot as usize).copied();
        let n = |name: &'static str| Some((name, None));
        let eruptor = [state(0, 0.0), state(1, 12.001898), state(3, 21.00168)];
        let g = ScriptedGaits::of(&eruptor, clip, n("Walk.ma"), n("Run.ma")).unwrap();
        assert_eq!(g.gaits, Gaits::ERUPTOR, "his Define call's, read back");
        assert!(g.walk && g.run);
        let ninjini = [state(0, 0.0), state(1, 20.493124)];
        let g = ScriptedGaits::of(&ninjini, clip, n("Walk.ma"), None).unwrap();
        assert_eq!(
            (g.gaits.walk_speed, g.gaits.run_speed),
            (20.493124, 20.493124)
        );
        assert!(g.walk && !g.run);
        let bouncer = ScriptedGaits::of(&ninjini, clip, n("Walk.ma"), n("Run.ma")).unwrap();
        assert!(bouncer.walk && !bouncer.run, "a run his scripts don't play");
        let runner = [state(0, 0.0), state(3, 16.0769)];
        let g = ScriptedGaits::of(&runner, clip, None, n("Run.ma")).unwrap();
        assert_eq!((g.gaits.walk_speed, g.gaits.run_speed), (16.0769, 16.0769));
        assert!(!g.walk && g.run);
        // Two states playing his run: the faster is his run.
        let g = ScriptedGaits::of(&[state(3, 15.0), state(4, 19.686)], clip, None, n("Run.ma"))
            .unwrap();
        assert_eq!(g.gaits.run_speed, 19.686, "the faster of two");
        // Slot 4 holds slot 3's clip: by name, either plays his run.
        let g = ScriptedGaits::of(&[state(4, 19.686)], clip, n("Run.ma"), n("Run.ma")).unwrap();
        assert!(g.walk && g.run && g.gaits.walk_speed == g.gaits.run_speed);
        // Double Trouble: his walk and run one clip, at slots 1, 2 and 3;
        // his scripts' run state on 3, his hero side's run there.
        let trouble = ["Idle.ma", "Run.ma", "Run.ma", "Run.ma"];
        let at = |slot: u32| trouble.get(slot as usize).copied();
        let one = [state(0, 0.0), state(3, 19.686)];
        let g = ScriptedGaits::of(
            &one,
            at,
            Some(("Run.ma", Some(1))),
            Some(("Run.ma", Some(3))),
        )
        .unwrap();
        assert!(!g.walk && g.run, "his one state his run's: {g:?}");
        assert_eq!((g.gaits.walk_speed, g.gaits.run_speed), (19.686, 19.686));
        let g = ScriptedGaits::of(&one, at, n("Run.ma"), n("Run.ma")).unwrap();
        assert!(g.walk && g.run, "no slots given: by name, both");
        // Dino-Rang: his run at 2 and 3, his scripts' on 3; his walk on 1.
        let dino = ["Idle.ma", "Walk.ma", "Run.ma", "Run.ma"];
        let at = |slot: u32| dino.get(slot as usize).copied();
        let states = [state(0, 0.0), state(1, 5.718783), state(3, 18.055342)];
        let g = ScriptedGaits::of(
            &states,
            at,
            Some(("Walk.ma", Some(1))),
            Some(("Run.ma", Some(3))),
        )
        .unwrap();
        assert!(g.walk && g.run);
        assert_eq!(
            (g.gaits.walk_speed, g.gaits.run_speed),
            (5.718783, 18.055342)
        );
        assert_eq!(
            ScriptedGaits::of(&[state(0, 0.0)], clip, n("Walk.ma"), n("Run.ma")),
            None,
            "standing only"
        );
        assert_eq!(
            ScriptedGaits::of(&eruptor, clip, n("Sneak.ma"), None),
            None,
            "no state plays his hero side's"
        );
    }

    /// AG.219: a walker's AG.30 locomotion on his scripts' states walks and runs his
    /// hero side's clips by speed, at the rates their speeds give, where the
    /// one AG.27 built him (no walk or run of ours) draws his idle at
    /// any speed; a run his scripts don't play stays out of it; with
    /// neither he keeps his idle.
    #[test]
    fn a_walkers_stand_in_walks_and_runs_his_scripts_gaits() {
        use guac_motion::locomotion::{Clip as Gait, Frame};
        let at = |speed: f32| Frame {
            speed,
            falling: 0.0,
            height: Some(0.005),
            held: false,
        };
        let palette = crate::picks::tests::palette();
        let (walk, run) = (palette.clip(1).unwrap(), palette.clip(3).unwrap());
        let joined = Loops {
            idle: 1.3333,
            walk: None,
            run: None,
            fall: Some(0.8),
        };
        let state = |slot: u32, speed: f32| LocomotionState {
            slot: Some(slot),
            speed,
        };
        let states = [state(0, 0.0), state(1, 12.0), state(3, 21.0)];
        let (walk_at, run_at) = (Some((walk, Some(1))), Some((run, Some(3))));
        let mut l = stand_in(&states, &palette, walk_at, run_at, joined).unwrap();
        assert_eq!(
            *l.loops(),
            Loops {
                walk: Some(walk.duration),
                run: Some(run.duration),
                ..joined
            }
        );
        for _ in 0..60 {
            l.step(1.0 / 60.0, at(24.0));
        }
        assert_eq!(l.weights(), [0.0, 0.0, 1.0, 0.0]);
        assert!((l.play(Gait::Run).rate - 24.0 / 21.0).abs() < 1e-6);
        for _ in 0..60 {
            l.step(1.0 / 60.0, at(12.0));
        }
        assert_eq!(l.weights(), [0.0, 1.0, 0.0, 0.0]);
        // His walk alone (a giant's): his hero side's run stays out.
        let mut l = stand_in(&states[..2], &palette, walk_at, run_at, joined).unwrap();
        assert_eq!((l.loops().walk, l.loops().run), (Some(walk.duration), None));
        for _ in 0..60 {
            l.step(1.0 / 60.0, at(24.0));
        }
        assert_eq!(l.weights(), [0.0, 1.0, 0.0, 0.0]);
        assert!((l.play(Gait::Walk).rate - 2.0).abs() < 1e-6, "24 over 12");
        // AG.27's, before: his idle at any speed.
        let mut before = Locomotion::new(
            Gaits {
                walk_speed: 0.0,
                run_speed: 0.0,
            },
            joined,
        );
        for _ in 0..60 {
            before.step(1.0 / 60.0, at(24.0));
        }
        assert_eq!(before.weights(), [1.0, 0.0, 0.0, 0.0]);
        assert!(stand_in(&states[..1], &palette, walk_at, run_at, joined).is_none());
    }
}
