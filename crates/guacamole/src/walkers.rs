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
//!   them. One whose setup doesn't run through keeps his card
//!   ([`Fallback::Card`], listed with why).
//!
//! The hero's drawn model and its keys are the card's model (AE's), drawn
//! in [`crate::level_scene::hero_scene`]: the keys the census's card tables
//! hold, so the start's pass has compiled them (`tests/giants_census.rs`).

use guac_motion::locomotion::Gaits;
use guac_motion::solid::{HavokSolid, SolidInfo};
use guac_motion::tuning::{self, Tuning};

use guac_script::{PlacementId, Vm};

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
/// unconstrained 10,000 ft/s on every character (AG.12's live read).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Movement {
    pub ground_max_speed: f32,
    pub ground_acceleration: f32,
    pub ground_deceleration: f32,
    pub ground_turn_rate: f32,
    pub air_deceleration: f32,
    pub gravity_acceleration: f32,
    pub gravity_max_speed: f32,
}

impl Movement {
    /// The members read, in the fields' order.
    pub const MEMBERS: [&'static str; 7] = [
        "ground max speed",
        "ground acceleration",
        "ground deceleration",
        "ground turn rate",
        "air deceleration",
        "gravity acceleration",
        "gravity max speed",
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
        let [a, b, c, d, e, f, g] = Self::MEMBERS;
        Some(Movement {
            ground_max_speed: member(a)?,
            ground_acceleration: member(b)?,
            ground_deceleration: member(c)?,
            ground_turn_rate: member(d)?,
            air_deceleration: member(e)?,
            gravity_acceleration: member(f)?,
            gravity_max_speed: member(g)?,
        })
    }

    /// AG.16's controller's numbers off these, under his solid's authored
    /// `width` (feet).
    pub fn tuning(&self, width: f32) -> Tuning {
        Tuning {
            max_speed: self.ground_max_speed,
            push: self.ground_acceleration,
            let_go: self.ground_deceleration,
            air_let_go: self.air_deceleration,
            turn_rate: self.ground_turn_rate,
            gravity: self.gravity_acceleration,
            terminal: self.gravity_max_speed,
            width,
        }
    }
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
    #[test]
    fn his_movement_parameters_are_the_controllers_numbers() {
        let tree_rex = Movement {
            ground_max_speed: 20.0,
            ground_acceleration: 160.0,
            ground_deceleration: 80.0,
            ground_turn_rate: 360.0,
            air_deceleration: 50.0,
            gravity_acceleration: 128.0,
            gravity_max_speed: 120.0,
        };
        assert_eq!(
            tree_rex.tuning(14.0),
            Tuning {
                max_speed: 20.0,
                push: 160.0,
                let_go: 80.0,
                air_let_go: 50.0,
                turn_rate: 360.0,
                gravity: 128.0,
                terminal: 120.0,
                width: 14.0,
            }
        );
        // Eruptor's description (AG.27's disc read) is AG.8's live table but
        // for gravity: it holds 128 ft/s², where AG.10 measured his fall at
        // 136.21. The fall-back walks on the description's.
        let eruptor = Movement {
            ground_max_speed: 21.5,
            ground_turn_rate: 1440.0,
            ..tree_rex
        };
        assert_eq!(
            eruptor.tuning(Tuning::ERUPTOR.width),
            Tuning {
                gravity: 128.0,
                ..Tuning::ERUPTOR
            }
        );
        assert_eq!(uniform_scale([100.0; 3]), Some(1.0));
        assert_eq!(uniform_scale([150.0; 3]), Some(1.5));
        assert_eq!(uniform_scale([150.0, 100.0, 150.0]), None);
        assert_eq!(uniform_scale([0.0; 3]), None);
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
}
