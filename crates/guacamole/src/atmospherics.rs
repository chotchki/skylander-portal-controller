//! The scripts' atmospheric zones as the TV follows them (openspec AS.127;
//! level-scene's A level's zone lights it). A level's definition adds its
//! zones, the extract reads them statically into its `atmosphere` part, and
//! from the level's start on `Game - Switch Atmospherics Zone` moves the level
//! between them: the hub's sky and deck to the stage's exterior, an interior
//! to its room, 214's cutscenes. `Atmospherics - Manage Atmospherics` runs that
//! on the VM, a zone's lights and fog sliding to the next over its transition
//! with `atmospheric transition pct` going 100 to 0.
//!
//! The TV reads where that stands, [`read`], and lights the level from the
//! part's zones of those ids, [`Lit`]: a zone between two is each byte, angle
//! and distance between theirs, as the manager's sliders leave them
//! ([`blended`]), a fog going off sliding to the far start the manager gives
//! it and coming on from there. Relighting rewrites every material's uniform,
//! so a transition relights at [`STEPS`] points of its way, never each frame;
//! a zone the same as the one lit rewrites nothing.
//!
//! The sky dome takes its share of the fog as the manager leaves it (openspec
//! AS.164, [`sky`]): its colour, `fog color of sky dome`, slides over every
//! transition as the fog's does, from the zone left to the one gone to (set
//! at once when the one left has its fog off, kept when neither has any);
//! its share, `fog of sky dome`, is set only where a zone is set up and
//! never over a transition. The TV holds the share its start's zone sets up
//! for the level's run: a trigger volume's zone or a switch with no
//! transition time sets another up in the original, and a zone with its fog
//! off left at a transition's end keeps the colour it had, which the TV
//! doesn't follow (stand-in ledger: `zone-sky`). A zone's depth of field,
//! shadow volume and yon-ignore colour aren't drawn.

use guac_ir::level::{Atmosphere, AtmosphericZone, ZoneFog, ZoneLight};
use guac_script::{Value, Vm};

use crate::level_scene::{LevelError, LevelFog};

/// How many points of a transition's way relight the level.
pub const STEPS: f32 = 8.0;

/// Where the fog stands while it goes off or comes on, feet: the manager's
/// start and range for a zone with none.
const FOG_OFF_START: i32 = 450;
const FOG_OFF_RANGE: i32 = 650;

/// The zone the scripts have the level in: `to`, or on the way there from
/// `from`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct ZoneNow {
    pub to: i32,
    pub from: Option<i32>,
    /// How much of the way is left, percent.
    pub remaining: f32,
}

impl ZoneNow {
    /// What `current`, a transition and its trigger's zones read (the module
    /// docs): `transition`, the manager inside one, with its front and back
    /// zones' ids and its percent left.
    pub fn of(current: i32, transition: Option<(i32, i32, f32)>) -> Self {
        match transition {
            Some((front, back, remaining)) if remaining > 0.0 => Self {
                to: back,
                from: Some(front),
                remaining: remaining.min(100.0),
            },
            Some((_, back, _)) => Self {
                to: back,
                from: None,
                remaining: 0.0,
            },
            None => Self {
                to: current,
                from: None,
                remaining: 0.0,
            },
        }
    }

    /// `self` at the [`STEPS`] points a transition relights at: rounded up,
    /// so its last relight is the zone itself.
    pub fn stepped(self) -> Self {
        let step = 100.0 / STEPS;
        Self {
            remaining: (self.remaining / step).ceil() * step,
            ..self
        }
    }
}

fn int(vm: &Vm, v: Value, name: &str) -> Option<i32> {
    match vm.field(v, name).map(|c| vm.value(c))? {
        Value::Int(i) => Some(i),
        Value::Float(f) => Some(f as i32),
        _ => None,
    }
}

fn float(vm: &Vm, v: Value, name: &str) -> Option<f32> {
    match vm.field(v, name).map(|c| vm.value(c))? {
        Value::Int(i) => Some(i as f32),
        Value::Float(f) => Some(f),
        _ => None,
    }
}

/// Where the run's manager stands; none for a run that holds no level
/// control data yet.
pub fn read(vm: &Vm) -> Option<ZoneNow> {
    let global = |name: &str| {
        let g = vm.program().global_named(name)?;
        vm.global_peek(g)
    };
    let current = int(
        vm,
        global("level control data")?,
        "atmospheric zone current",
    )?;
    let transition = global("atmospherics control data").and_then(|control| {
        (int(vm, control, "in atmospheric transition")? != 0).then_some(())?;
        let trigger = vm
            .field(control, "atmospheric trigger")
            .map(|c| vm.value(c))?;
        let zone = |name: &str| {
            let z = vm.field(trigger, name).map(|c| vm.value(c))?;
            int(vm, z, "id")
        };
        Some((
            zone("zone front")?,
            zone("zone back")?,
            float(vm, control, "atmospheric transition pct")?,
        ))
    });
    Some(ZoneNow::of(current, transition))
}

/// `a` at `left` (0-1) of the way back from `b`.
fn mix(a: i32, b: i32, left: f32) -> i32 {
    (b as f32 + (a - b) as f32 * left).round() as i32
}

fn bytes(a: [u8; 4], b: [u8; 4], left: f32) -> [u8; 4] {
    std::array::from_fn(|i| mix(i32::from(a[i]), i32::from(b[i]), left).clamp(0, 255) as u8)
}

/// An angle the short way round, as `Calc - Slide Angle` slides one.
fn angle(a: i32, b: i32, left: f32) -> i32 {
    let turn = (a - b).rem_euclid(360);
    let turn = if turn > 180 { turn - 360 } else { turn };
    b + (turn as f32 * left).round() as i32
}

fn light(a: &ZoneLight, b: &ZoneLight, left: f32) -> ZoneLight {
    ZoneLight {
        diffuse: bytes(a.diffuse, b.diffuse, left),
        specular: bytes(a.specular, b.specular, left),
        facing: angle(a.facing, b.facing, left),
        tilt: angle(a.tilt, b.tilt, left),
    }
}

fn fog(a: &ZoneFog, b: &ZoneFog, left: f32) -> ZoneFog {
    // A fog going off or coming on slides from where the manager leaves one
    // with none, in the colour of the one that's on.
    let off = |on: &ZoneFog| ZoneFog {
        start: FOG_OFF_START,
        range: FOG_OFF_RANGE,
        ..*on
    };
    let left_sky = a.sky_dome_colour;
    let (a, b) = match (a.on != 0, b.on != 0) {
        (true, false) => (*a, off(a)),
        (false, true) => (off(b), *b),
        _ => (*a, *b),
    };
    ZoneFog {
        on: a.on.max(b.on),
        start: mix(a.start, b.start, left),
        range: mix(a.range, b.range, left),
        colour: bytes(a.colour, b.colour, left),
        // Its share isn't the manager's to slide ([`sky`]).
        sky_dome_percent: b.sky_dome_percent,
        // Slid as the colour is, nothing moving it between two fogs off.
        sky_dome_colour: if a.on == 0 && b.on == 0 {
            left_sky
        } else {
            bytes(a.sky_dome_colour, b.sky_dome_colour, left)
        },
        yon_colour: bytes(a.yon_colour, b.yon_colour, left),
    }
}

/// `to` with `remaining` percent of the way from `from` left.
pub fn blended(from: &AtmosphericZone, to: &AtmosphericZone, remaining: f32) -> AtmosphericZone {
    let left = remaining / 100.0;
    AtmosphericZone {
        id: to.id,
        ambient: bytes(from.ambient, to.ambient, left),
        lights: std::array::from_fn(|i| light(&from.lights[i], &to.lights[i], left)),
        fog: fog(&from.fog, &to.fog, left),
    }
}

/// What the level is lit from for `now`, or why not: a zone the part doesn't
/// hold, or a fog the TV can't draw.
pub fn lit(atmosphere: &Atmosphere, now: &ZoneNow) -> Result<AtmosphericZone, String> {
    let zone = |id: i32| {
        atmosphere
            .zones
            .iter()
            .find(|z| z.id == id)
            .ok_or_else(|| format!("zone {id}: not in the level's atmosphere part"))
    };
    let to = zone(now.to)?;
    let lit = match now.from {
        Some(from) if now.remaining > 0.0 => blended(zone(from)?, to, now.remaining),
        _ => *to,
    };
    LevelFog::from_zone(&lit.fog).map_err(|e: LevelError| format!("zone {}: {e}", now.to))?;
    Ok(lit)
}

/// The sky's fog (the module docs): `share` (0 to 1, the zone set up's) of
/// the sky colour `lit` stands at.
pub fn sky(share: f32, lit: &AtmosphericZone) -> LevelFog {
    LevelFog::sky(lit.fog.sky_dome_colour, share)
}

#[cfg(test)]
mod tests {
    use guac_ir::{DumpIdentity, Sha256};

    use super::*;

    fn zone(id: i32, level: u8, fog_on: i32) -> AtmosphericZone {
        let light = ZoneLight {
            diffuse: [level; 4],
            specular: [level / 2; 4],
            facing: 350,
            tilt: -45,
        };
        AtmosphericZone {
            id,
            ambient: [level; 4],
            lights: [light; 3],
            fog: ZoneFog {
                on: fog_on,
                start: 100,
                range: 1000,
                colour: [level, 0, 0, 0],
                sky_dome_percent: 0,
                sky_dome_colour: [0; 4],
                yon_colour: [level, 0, 0, 0],
            },
        }
    }

    fn atmosphere(zones: Vec<AtmosphericZone>) -> Atmosphere {
        Atmosphere {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: Sha256([4; 32]),
            },
            level: "level".into(),
            script: "x".into(),
            zones,
            start: Some(0),
        }
    }

    /// AS.127: the manager's readings: no transition reads the current
    /// zone; one under way its back zone with what's left of it; one run
    /// out its back zone whole.
    #[test]
    fn the_zone_reads_off_the_manager() {
        let none = ZoneNow::of(201, None);
        assert_eq!((none.to, none.from, none.remaining), (201, None, 0.0));
        let under_way = ZoneNow::of(0, Some((0, 201, 66.0)));
        assert_eq!(
            (under_way.to, under_way.from, under_way.remaining),
            (201, Some(0), 66.0)
        );
        let over = ZoneNow::of(0, Some((0, 201, 0.0)));
        assert_eq!((over.to, over.from), (201, None));
    }

    /// AS.127: a transition relights at its steps, its last the zone itself.
    #[test]
    fn a_transition_stands_at_its_steps() {
        let at = |remaining| {
            ZoneNow {
                to: 1,
                from: Some(0),
                remaining,
            }
            .stepped()
            .remaining
        };
        assert_eq!(at(100.0), 100.0);
        assert_eq!(at(99.0), 100.0);
        assert_eq!(at(87.6), 100.0);
        assert_eq!(at(87.4), 87.5);
        assert_eq!(at(0.1), 12.5);
        assert_eq!(at(0.0), 0.0);
    }

    /// AS.127: halfway between two zones each byte and distance stands
    /// halfway, and an angle the short way round 0.
    #[test]
    fn a_zone_between_two_stands_between_them() {
        let (a, mut b) = (zone(0, 100, 1), zone(1, 200, 1));
        b.lights[0].facing = 10;
        b.fog.start = 300;
        let half = blended(&a, &b, 50.0);
        assert_eq!(half.id, 1);
        assert_eq!(half.ambient[0], 150);
        assert_eq!(half.lights[1].diffuse[0], 150);
        assert_eq!(half.lights[0].facing, 0, "350 to 10 by the short way");
        assert_eq!(half.fog.start, 200);
        assert_eq!(half.fog.colour[0], 150);
        assert_eq!(blended(&a, &b, 100.0).ambient, a.ambient);
        assert_eq!(blended(&a, &b, 0.0).ambient, b.ambient);
        assert_eq!(blended(&a, &b, 0.0).fog, b.fog);
    }

    /// AS.127: a fog going off slides toward the manager's far start in its
    /// own colour, one coming on from there in the new one's.
    #[test]
    fn a_fog_going_off_or_on_slides_from_the_managers_far_start() {
        let (on, off) = (zone(0, 100, 1), zone(1, 200, 0));
        let going = blended(&on, &off, 50.0).fog;
        assert_eq!(going.on, 1, "on until the zone's reached");
        assert_eq!((going.start, going.range), (275, 825));
        assert_eq!(going.colour[0], 100);
        let coming = blended(&off, &on, 50.0).fog;
        assert_eq!(coming.on, 1);
        assert_eq!(coming.colour[0], 100);
    }

    /// AS.127: a zone the part doesn't hold, or a fog the TV can't draw, is
    /// refused by name; one asking a sky share is lit (AS.164).
    #[test]
    fn a_zone_the_tv_cant_draw_is_refused_by_name() {
        let mut odd = zone(2, 10, 2);
        odd.fog.sky_dome_percent = 0;
        let mut sky = zone(3, 10, 1);
        sky.fog.sky_dome_percent = 100;
        let a = atmosphere(vec![zone(0, 100, 1), odd, sky]);
        let now = |to| ZoneNow::of(to, None);
        assert_eq!(lit(&a, &now(0)).unwrap().id, 0);
        assert!(lit(&a, &now(9)).unwrap_err().contains("zone 9"));
        assert!(lit(&a, &now(2)).unwrap_err().contains("fog on"));
        assert_eq!(lit(&a, &now(3)).unwrap(), sky);
    }

    /// AS.164: the sky's colour slides over a switch as the fog's does,
    /// from the zone left to the one gone to; the share is the one set up,
    /// whatever the zones on the way ask: chapter 1's start's 0 % through
    /// its death area's 100 % zones fogs no sky, a start's 100 % fogs it
    /// whole in each zone's colour.
    #[test]
    fn the_skys_colour_slides_and_its_share_stays_the_one_set_up() {
        let (mut start, mut storm) = (zone(0, 100, 1), zone(1, 20, 1));
        start.fog.sky_dome_colour = [128, 128, 128, 0];
        storm.fog.sky_dome_percent = 100;
        storm.fog.sky_dome_colour = [41, 0, 29, 0];
        let half = blended(&start, &storm, 50.0);
        assert_eq!(half.fog.sky_dome_colour, [85, 64, 79, 0]);
        assert_eq!(
            blended(&start, &storm, 100.0).fog.sky_dome_colour,
            [128, 128, 128, 0]
        );
        assert_eq!(
            blended(&start, &storm, 0.0).fog.sky_dome_colour,
            [41, 0, 29, 0]
        );
        // The start's 0 % set up: no sky fog anywhere on the way.
        let share = crate::level_scene::sky_share(&start).unwrap();
        for at in [100.0, 50.0, 0.0] {
            assert_eq!(
                sky(share, &blended(&start, &storm, at)),
                LevelFog::NONE,
                "{at}"
            );
        }
        // A 100 % start: the sky whole in the colour of the way.
        let whole = sky(1.0, &half);
        assert_eq!(whole.colour, [85, 64, 79]);
        assert!(whole.far <= 0.0, "{whole:?}");
    }

    /// AS.164: from a zone with its fog off the sky's colour is the next
    /// one's at once; into one it stays the one it leaves; between two
    /// with none nothing moves it.
    #[test]
    fn the_skys_colour_follows_the_managers_fog_rules() {
        let (mut on, mut off) = (zone(0, 100, 1), zone(1, 200, 0));
        on.fog.sky_dome_colour = [10, 20, 30, 0];
        off.fog.sky_dome_colour = [200, 100, 0, 0];
        assert_eq!(
            blended(&off, &on, 75.0).fog.sky_dome_colour,
            [10, 20, 30, 0]
        );
        assert_eq!(
            blended(&on, &off, 25.0).fog.sky_dome_colour,
            [10, 20, 30, 0]
        );
        let mut other = off;
        other.fog.sky_dome_colour = [0, 0, 0, 0];
        assert_eq!(
            blended(&off, &other, 50.0).fog.sky_dome_colour,
            [200, 100, 0, 0]
        );
    }
}
