//! A turret's game on the TV's run (openspec AS.76): level 214's two turrets
//! run on the VM from their activators (`Turret_1_Base.ai`, `Turret_2_Base.ai`
//! and `global.bld`'s `commonTurretMacros.ai`), their update macros on the
//! game systems' tick ([`crate::player_loop::UPDATES`]). What the host adds:
//! the interface laid out each step one is active, so a pointer's `screen
//! location` reads where the stick has moved it, which `New Turret - Find
//! Target` turns into the ray it picks a target along; and a player getting
//! out carried along the get-out's arc to the spot it works out
//! ([`carry_out`], openspec AS.189).

use guac_motion::seam::Vec3;
use guac_script::{Handle, Value, Vm};
use serde::Serialize;

use crate::script_engine::{Engine, METRES_PER_FOOT};
use crate::scripts::ScriptWorld;

/// How near the pointer's ray a target is picked, metres: about a padlock's
/// size, so a pointer on one takes it (ours: stand-in ledger `turret-pick`).
pub const PICK_REACH: f32 = 1.5;

/// The string a turret's calibration shows on the PS3.
pub const CALIBRATION_LINE: &str = "PointAtTarget_360/PS3";

/// What a turret's shots are named from: each element's template.
pub const SHOTS: &str = "New_Turret_Projectile";

/// The turret the scripts run, if one is active (`Active Turret`).
pub fn active(vm: &Vm) -> Option<Value> {
    let g = vm.program().global_named("Active Turret")?;
    vm.global_peek(g)
        .map(|v| vm.value(v))
        .filter(|v| *v != Value::Null)
}

/// The turret state its players get out in.
pub const GETTING_OUT: &str = "New Turret - Turret State - Players Getting Out";

/// One step's turret, after the scripts' step: with one active, the
/// interface laid out and a player getting out carried along its arc; one
/// let go this step left where the arc had him.
pub fn step(vm: &Vm, world: &mut ScriptWorld, engine: &mut dyn Engine) {
    let_go(world, engine);
    let turret = active(vm);
    world.ray_picks = turret.is_some();
    world.turret_base = turret
        .and_then(|t| vm.field(t, "turret actor"))
        .and_then(|a| match vm.value(a) {
            Value::Entity(h) => Some(h),
            _ => None,
        });
    if world.ray_picks {
        world.lay_out_interface();
    }
    if let Some(t) = turret {
        carry_out(vm, world, engine, t);
    }
}

/// `turret state`'s macro name.
fn state(vm: &Vm, turret: Value) -> String {
    match vm.field(turret, "turret state").map(|s| vm.value(s)) {
        Some(Value::Object(o)) => vm.program().name_of(o).unwrap_or_default().to_owned(),
        _ => String::new(),
    }
}

/// The actor a field of `of` names, if it names one.
fn actor_in(vm: &Vm, world: &ScriptWorld, of: Value, name: &str) -> Option<Handle> {
    match vm.value(vm.field(of, name)?) {
        Value::Entity(h) => Some(h),
        Value::Object(o) => world.handles().get(&o).copied(),
        _ => None,
    }
}

/// Where the get-out's arc has a player `along` ft of the way from the
/// turret (`from`) to the spot he's let go at (`to`), `up` ft over it.
pub fn arc(from: Vec3, to: Vec3, along: f32, up: f32) -> Vec3 {
    let d = to - from;
    let length = d.length() / METRES_PER_FOOT;
    let k = if length > 0.0 {
        (along / length).clamp(0.0, 1.0)
    } else {
        1.0
    };
    from + d * k + Vec3::Y * (up * METRES_PER_FOOT)
}

/// While `turret`'s players get out, each hung on it carried along the
/// arc to the spot the get-out works out (openspec AS.189).
///
/// `New Turret - Update Player Getting Out` puts that spot (`get out
/// location`) at its controller's distance from the turret, 30° round (9.0
/// m off 214's first turret, 11.0 m off its second), and over 1.6 s writes
/// his distance from the turret as that distance less `turret state value`
/// (0 to 10 ft to the distance) and his height over it as `turret state
/// value 2` (up 25 ft and back). Read as written that ends him back at the
/// middle, where the ball, solid again, lifts him onto its top; the spot
/// and the scripts' own 5 ft check against it (`New Turret - Check If
/// Player Finished Getting Out`, which nothing calls) say he lands by it,
/// so the first value is carried as how far along the way he is and the
/// second as his height. Stand-in ledger: `turret-get-out`.
pub fn carry_out(vm: &Vm, world: &mut ScriptWorld, engine: &mut dyn Engine, turret: Value) {
    if state(vm, turret) != GETTING_OUT {
        return;
    }
    let (Some(base), Some(from)) = (
        actor_in(vm, world, turret, "turret actor"),
        actor_in(vm, world, turret, "get in location"),
    ) else {
        return;
    };
    let Some(from) = world.actor(from).map(|a| a.position) else {
        return;
    };
    for player in ["p1 player data", "p2 player data"] {
        let Some(p) = vm.field(turret, player).map(|v| vm.value(v)) else {
            continue;
        };
        let (Some(minion), Some(out)) = (
            actor_in(vm, world, p, "minion"),
            actor_in(vm, world, p, "get out location"),
        ) else {
            continue;
        };
        if world.actor(minion).map(|a| a.attach) != Some(Value::Entity(base)) {
            continue;
        }
        let Some(to) = world.actor(out).filter(|a| !a.removed).map(|a| a.position) else {
            continue;
        };
        let number = |name: &str| {
            vm.field(p, name)
                .and_then(|v| vm.value(v).as_f32())
                .unwrap_or(0.0)
        };
        let at = arc(
            from,
            to,
            number("turret state value"),
            number("turret state value 2"),
        );
        if world.hang_at(minion, at, engine) {
            world.turret_carried.push((minion, at));
        }
    }
}

/// Each player the last step carried who's been let go since, left where
/// the carry had him. The step that lets him go has the scripts write his
/// distance from the turret as nothing first (the arc's end as written),
/// which moves a hero still hung for that step alone: let go then, that's
/// where he'd stay, at the turret's middle.
fn let_go(world: &mut ScriptWorld, engine: &mut dyn Engine) {
    for (h, at) in std::mem::take(&mut world.turret_carried) {
        if world.actor(h).is_some_and(|a| a.attach == Value::Null)
            && let Some(hero) = engine.hero(h)
        {
            hero.pivot = at;
            hero.walk.velocity = Vec3::ZERO;
        }
    }
}

/// What a TV shows of the active turret: its state, where its first
/// player's pointer is, where it wants calibrating and the line that asks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurretView {
    /// `turret state`'s macro.
    pub state: String,
    /// The pointer's offset from the screen's middle, interface units.
    pub pointer: Option<[f32; 2]>,
    /// Calibrating, the circle's offset; its line's English when it reads.
    pub calibration: Option<[f32; 2]>,
    pub message: Option<String>,
    /// The padlocks hit so far (`DBG_Lockhit`, the turret's objective).
    pub hits: i32,
    /// Where each of its targets standing stands from the screen's middle,
    /// as the pointer's offset would be on it.
    pub targets: Vec<[f32; 2]>,
    /// Getting out, where the first player is let go (`get out location`),
    /// metres.
    pub get_out: Option<[f32; 3]>,
    /// Where the first player's barrel points as drawn, its heading and
    /// pitch in degrees (openspec AS.445).
    pub barrel: Option<[f32; 2]>,
    /// The heading and pitch from the first player's ball to the target
    /// `New Turret - Find Target` put where the pointer's ray lands: what
    /// `New Turret - Track Target With Barrel` aims the ball at.
    pub aim: Option<[f32; 2]>,
    /// Where that target stands, metres: on the pointer's ray from the
    /// camera, where it met the level or a prop, else 250 ft out (openspec
    /// AS.445's review, a corner the TV and headless aimed apart at).
    pub target: Option<[f32; 3]>,
}

/// The heading and pitch of `d`, degrees: heading 0 along +Z, 90 along +X.
pub fn heading_pitch(d: Vec3) -> [f32; 2] {
    [
        d.x.atan2(d.z).to_degrees().rem_euclid(360.0),
        d.y.atan2(d.x.hypot(d.z)).to_degrees(),
    ]
}

/// The screen word `New Turret - Find Target` takes off its pointer
/// graphic's place to find where the ray goes through, as the file holds it.
const FIND_TARGET_NUDGE: u32 = 0x00A0_0000;

/// [`FIND_TARGET_NUDGE`] in pixels, read as every disc-born screen word is
/// (guac-script's crate docs, Literals and rolls; openspec AS.560): 10 down,
/// none across.
fn find_target_nudge() -> (f32, f32) {
    let word = guac_script::Literal {
        class: guac_script::LiteralClass::Screen,
        bits: FIND_TARGET_NUDGE,
        random: false,
    };
    match word.value() {
        Value::Screen(s) => crate::viewport::unpack_screen(s),
        _ => (0.0, 0.0),
    }
}

/// A pointer's offset that lands on `(x, y)` on the screen: `New Turret -
/// Find Target` sends the ray through its graphic's place less
/// [`find_target_nudge`], at 1.5 pixels a unit.
pub fn pointer_at(x: f32, y: f32) -> [f32; 2] {
    let (w, h) = (
        crate::viewport::SCREEN_WIDTH,
        crate::viewport::SCREEN_HEIGHT,
    );
    let (nx, ny) = find_target_nudge();
    [(x + nx - w / 2.0) / 1.5, (y + ny - h / 2.0) / 1.5]
}

/// The first player's data of `turret`.
fn player(vm: &Vm, turret: Value) -> Option<Value> {
    vm.field(turret, "p1 player data").map(|p| vm.value(p))
}

/// [`TurretView`] of the turret running now.
pub fn view(vm: &Vm, world: &ScriptWorld) -> Option<TurretView> {
    let turret = active(vm)?;
    let state = state(vm, turret);
    let number = |v: Value, name: &str| vm.field(v, name).and_then(|f| vm.value(f).as_f32());
    let pointer = player(vm, turret)
        .and_then(|p| vm.field(p, "wii pointer data"))
        .map(|p| vm.value(p))
        .filter(|p| *p != Value::Null)
        .and_then(|p| Some([number(p, "xrem")?, number(p, "yrem")?]));
    let calibrating = vm
        .field(turret, "calibration data")
        .map(|c| vm.value(c))
        .filter(|c| *c != Value::Null);
    let calibration = calibrating
        .and_then(|c| vm.field(c, "calibration location"))
        .and_then(|l| match vm.value(l) {
            Value::Screen(s) => {
                let (x, y) = crate::viewport::unpack_screen(s);
                Some([x, y])
            }
            _ => None,
        });
    // The line this platform's screen shows; what the data remembers is the
    // Wii's, whatever it put up.
    let message = calibrating
        .and_then(|_| vm.program().object_named(CALIBRATION_LINE))
        .and_then(|o| world.text_of(Value::Object(o)));
    let hits = vm
        .program()
        .global_named("DBG_Lockhit")
        .and_then(|g| vm.global_peek(g))
        .and_then(|v| vm.value(v).as_i32())
        .unwrap_or(0);
    let targets = vm
        .field(turret, "turret targets")
        .and_then(|t| vm.set(t).map(<[Value]>::to_vec))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| match v {
            Value::Entity(h) => Some(h),
            Value::Object(o) => world.handles().get(&o).copied(),
            _ => None,
        })
        .filter(|h| !world.removed(*h))
        .filter_map(|h| world.actor(h).map(|a| a.position))
        .filter_map(|p| world.viewport().project(p))
        .map(|(x, y)| pointer_at(x, y))
        .collect();
    let get_out = (state == GETTING_OUT)
        .then(|| player(vm, turret))
        .flatten()
        .and_then(|p| actor_in(vm, world, p, "get out location"))
        .and_then(|h| world.actor(h))
        .map(|a| a.position.to_array());
    let p1 = player(vm, turret);
    let barrel = p1
        .and_then(|p| actor_in(vm, world, p, "turret barrel"))
        .and_then(|h| world.actor(h))
        .map(|a| heading_pitch(crate::props::actor_transform(a).rotation * Vec3::Z));
    let target = p1.and_then(|p| {
        let data = vm.value(vm.field(p, "attack data ref")?);
        Some(world.actor(actor_in(vm, world, data, "target")?)?.position)
    });
    let aim = p1.and_then(|p| {
        let ball = world
            .actor(actor_in(vm, world, p, "turret ball")?)?
            .position;
        Some(heading_pitch(target? - ball))
    });
    Some(TurretView {
        state,
        pointer,
        calibration,
        message,
        hits,
        targets,
        get_out,
        barrel,
        aim,
        target: target.map(|t| t.to_array()),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use guac_motion::hero::Hero;
    use guac_motion::parry::ParryWorld;
    use guac_motion::seam::PhysicsSeam;
    use guac_motion::tuning::{Tuning, eruptor_solid};

    use super::*;
    use crate::script_engine::{Actor, Pad};
    use crate::scripts::{Files, headless_objects};

    const TURRET: Handle = 0x6000_0041;
    const HERO: Handle = 0x6000_0042;

    struct OneHero {
        hero: Hero,
        world: ParryWorld,
    }

    impl Engine for OneHero {
        fn hero(&mut self, h: Handle) -> Option<&mut Hero> {
            (h == HERO).then_some(&mut self.hero)
        }
        fn heroes(&self) -> Vec<Handle> {
            vec![HERO]
        }
        fn seam(&self) -> &dyn PhysicsSeam {
            &self.world
        }
        fn view_heading(&self) -> f32 {
            0.0
        }
        fn pad(&self, _: usize) -> Pad {
            Pad::default()
        }
    }

    /// openspec AS.189: a player the get-out carried is left where it had
    /// him once he's let go, wherever the scripts' write on that step put
    /// him; one still hung is the carry's to place.
    #[test]
    fn a_player_let_go_stays_where_the_arc_had_him() {
        let files = Files {
            lights: HashMap::new(),
            emitters: HashMap::new(),
            placed: Vec::new(),
            ids: HashMap::new(),
            points: HashMap::new(),
            turns: HashMap::new(),
        };
        let handles = HashMap::new();
        let mut w = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let mut e = OneHero {
            hero: Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 0.0),
            world: ParryWorld::new(&[]).unwrap(),
        };
        w.actors.insert(TURRET, Actor::blank());
        let mut him = Actor::blank();
        him.attach = Value::Entity(TURRET);
        w.actors.insert(HERO, him);
        let carried = Vec3::new(5.0, 0.2, 7.0);
        let written = Vec3::new(0.0, 0.1, 0.0);
        e.hero.pivot = written;
        w.turret_carried.push((HERO, carried));
        let_go(&mut w, &mut e);
        assert_eq!(e.hero.pivot, written, "still hung: the carry's");
        assert!(w.turret_carried.is_empty());
        w.turret_carried.push((HERO, carried));
        w.actors.get_mut(&HERO).unwrap().attach = Value::Null;
        e.hero.walk.velocity = Vec3::X;
        let_go(&mut w, &mut e);
        assert_eq!(e.hero.pivot, carried, "let go where the arc had him");
        assert_eq!(e.hero.walk.velocity, Vec3::ZERO);
        e.hero.pivot = written;
        let_go(&mut w, &mut e);
        assert_eq!(e.hero.pivot, written, "once");
    }

    /// openspec AS.189: the get-out's arc runs from the turret's middle to
    /// its spot by how far along the way the first value has come, the
    /// second its height over the way, and stops at the spot.
    #[test]
    fn the_get_outs_arc_runs_from_the_turret_to_its_spot() {
        let from = Vec3::new(665.2, -15.88, 452.81);
        let to = Vec3::new(670.59, -15.88, 460.0);
        let length = from.distance(to) / METRES_PER_FOOT;
        let near = |a: Vec3, b: Vec3| a.distance(b) < 1e-3;
        assert!(near(arc(from, to, 0.0, 0.0), from), "at its middle");
        assert!(near(arc(from, to, length, 0.0), to), "at the spot");
        assert!(near(arc(from, to, length * 2.0, 0.0), to), "and no further");
        let half = arc(from, to, length / 2.0, 25.0);
        assert!(near(
            half,
            (from + to) / 2.0 + Vec3::Y * 25.0 * METRES_PER_FOOT
        ));
        assert!(near(arc(from, from, 10.0, 0.0), from), "a spot on it");
    }

    /// openspec AS.445: a direction's heading and pitch, as the barrel's
    /// and the aim's are read: 0 along +Z, 90 along +X, up positive.
    #[test]
    fn a_directions_heading_and_pitch() {
        let near =
            |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3;
        assert!(near(heading_pitch(Vec3::Z), [0.0, 0.0]));
        assert!(near(heading_pitch(Vec3::X), [90.0, 0.0]));
        assert!(near(heading_pitch(-Vec3::X), [270.0, 0.0]));
        assert!(near(heading_pitch(Vec3::new(0.0, 1.0, 1.0)), [0.0, 45.0]));
        let turned = crate::props::actor_transform(&{
            let mut a = crate::script_engine::Actor::blank();
            a.angles.facing = 107.0;
            a.angles.tilt = 18.0;
            a
        });
        let [h, p] = heading_pitch(turned.rotation * Vec3::Z);
        assert!((h - 107.0).abs() < 1e-2, "{h}");
        assert!((p.abs() - 18.0).abs() < 1e-2, "{p}");
    }

    /// openspec AS.76, AS.560: a pointer's offset for a place on the screen
    /// is what `New Turret - Find Target` turns back into that place: 1.5
    /// pixels a unit, the ray through its graphic's place less the file's
    /// nudge, which reads 10 down (x the file word's low half), not 10 across.
    #[test]
    fn a_pointers_offset_lands_on_its_place() {
        assert_eq!(find_target_nudge(), (0.0, 10.0));
        assert_eq!(pointer_at(640.0, 350.0), [0.0, 0.0]);
        let [x, y] = pointer_at(516.15, 373.47);
        assert!((x * 1.5 - (516.15 - 640.0)).abs() < 1e-3);
        assert!((y * 1.5 - 10.0 - (373.47 - 360.0)).abs() < 1e-3);
    }
}
