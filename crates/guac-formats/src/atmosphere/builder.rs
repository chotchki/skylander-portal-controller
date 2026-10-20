//! Writes a level's definition script in code (openspec AG.41,
//! `test-fixtures`): STARTUP calling the add macro, through its stub as a
//! level file does, the PS3's zones in the else of a check on the hardware
//! platform and more after it. The numbers are made up; nothing here comes
//! from a disc.

use super::{ADD_ZONE, PLATFORM, params};
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::script::Class;
use crate::script::builder::{El, Writer};

/// The script's source path in the builders' files.
pub const SCRIPT: &str = "C:/tfb/Content/Levels/Level_900/Scripts/Level_Definition_900.ai";

/// One zone call's arguments, colours packed low byte red.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneCall {
    pub id: i32,
    pub ambient: u32,
    /// Per light: diffuse, specular, facing, tilt.
    pub lights: [(u32, u32, i32, i32); 3],
    pub fog_on: i32,
    pub start: i32,
    pub range: i32,
    pub colour: u32,
    pub sky_dome_percent: i32,
    pub sky_dome_colour: u32,
    pub yon_from_fog: i32,
    pub yon: u32,
}

impl ZoneCall {
    /// A zone with made-up numbers, `seed` moving every one.
    pub fn sample(id: i32, seed: u32) -> Self {
        let c = |n: u32| 0x0010_2030 + seed * 0x0101_0101 + n * 0x0003_0507;
        Self {
            id,
            ambient: c(0),
            lights: [
                (c(1), c(2), 40 + seed as i32, -20),
                (c(3), c(4), 200, 25),
                (c(5), c(6), 300, -60),
            ],
            fog_on: 1,
            start: 50 + seed as i32,
            range: 700,
            colour: c(7),
            sky_dome_percent: 0,
            sky_dome_colour: 0x0080_8080,
            yon_from_fog: 1,
            yon: 0x0080_8080,
        }
    }
}

/// How a call writes its arguments.
#[derive(Debug, Clone, Default)]
pub struct Shape {
    /// Parameters every call leaves out.
    pub omit: Vec<&'static str>,
    /// A parameter the macro doesn't have, added.
    pub stranger: Option<&'static str>,
    /// Colours written as `IntMeasurement` literals, not `ColorMeasurement`.
    pub colours_as_ints: bool,
    /// "use lights from zone", else -1.
    pub lights_from: Option<i32>,
}

/// One call of the add macro (`add`, its stub) with `z`'s arguments, the
/// unread ones too, written as `shape` says.
pub fn zone_call(w: &mut Writer<'_>, add: Location, z: &ZoneCall, shape: &Shape) {
    let mut args: Vec<(&str, Location)> = Vec::new();
    let colour_class = if shape.colours_as_ints {
        "IntMeasurement"
    } else {
        "ColorMeasurement"
    };
    let colour = |w: &mut Writer<'_>, name: &'static str, bits: u32| {
        let l = w.literal(colour_class, bits, false);
        (name, l)
    };
    let mut ints: Vec<(&'static str, i32)> = vec![
        (params::ZONE_ID, z.id),
        (params::LIGHTS_FROM, shape.lights_from.unwrap_or(-1)),
        (params::FOG_FROM, -1),
        (params::FOG_ON, z.fog_on),
        (params::FOG_START, z.start),
        (params::FOG_RANGE, z.range),
        (params::SKY_DOME_PERCENT, z.sky_dome_percent),
        (params::YON_FROM_FOG, z.yon_from_fog),
    ];
    let mut colours: Vec<(&'static str, u32)> = vec![
        (params::AMBIENT, z.ambient),
        (params::FOG_COLOUR, z.colour),
        (params::SKY_DOME_COLOUR, z.sky_dome_colour),
        (params::YON_COLOUR, z.yon),
    ];
    for ([diffuse, specular, facing, tilt], (d, s, f, t)) in params::LIGHTS.iter().zip(z.lights) {
        colours.push((diffuse, d));
        colours.push((specular, s));
        ints.push((facing, f));
        ints.push((tilt, t));
    }
    for (name, value) in ints {
        if !shape.omit.contains(&name) {
            let l = w.int(value);
            args.push((name, l));
        }
    }
    for (name, bits) in colours {
        if !shape.omit.contains(&name) {
            args.push(colour(w, name, bits));
        }
    }
    // The ones the reader leaves unread, a float among them.
    for name in params::UNREAD {
        let l = if name == "shadow volume intensity" {
            w.float(37.5)
        } else {
            w.int(-1)
        };
        args.push((name, l));
    }
    if let Some(name) = shape.stranger {
        let l = w.int(1);
        args.push((name, l));
    }
    w.call(add, &args);
}

/// A level file whose definition script's STARTUP calls another macro,
/// then adds `wii` under `if system.hardware platform == 3` and `ps3` in its
/// else, then `after`; `shape` writes every call. Returns the file.
pub fn level(wii: &[ZoneCall], ps3: &[ZoneCall], after: &[ZoneCall], shape: &Shape) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    definition(&mut b, wii, ps3, after, shape);
    b.build()
}

/// [`level`]'s definition script, written into `b` beside what else it
/// holds (a synthetic level's scene and placements).
pub fn definition(
    b: &mut IgzBuilder,
    wii: &[ZoneCall],
    ps3: &[ZoneCall],
    after: &[ZoneCall],
    shape: &Shape,
) {
    let mut w = Writer::new(b);
    // Stubs: definitions at no position, as a level file names global.bld's.
    let add = w.op(Class::DefineMacro, ADD_ZONE);
    let other = w.op(Class::DefineMacro, "_Level Defintion");
    w.script(
        SCRIPT,
        |w| {
            let far = w.int(500);
            w.call(other, &[("level far clip", far)]);
            let three = w.int(3);
            let check = w.check(&[El::Member(PLATFORM.0, PLATFORM.1)], 1, three);
            for z in wii {
                zone_call(w, add, z, shape);
            }
            let skip = w.otherwise(check);
            for z in ps3 {
                zone_call(w, add, z, shape);
            }
            w.close(skip);
            for z in after {
                zone_call(w, add, z, shape);
            }
            let one = w.int(1);
            w.set(&[El::Member("system", "Level_Def_Loaded")], one);
        },
        |_| {},
        |_| {},
    );
}

/// Level 900's sample zones: 0 and 6 per platform, 1 and 2 after.
pub fn sample() -> Vec<u8> {
    level(
        &[ZoneCall::sample(0, 9), ZoneCall::sample(6, 8)],
        &[ZoneCall::sample(0, 1), ZoneCall::sample(6, 2)],
        &[ZoneCall::sample(1, 3), ZoneCall::sample(2, 4)],
        &Shape::default(),
    )
}

/// Seeds for the property test and the fuzz corpus.
pub fn seeds() -> Vec<Vec<u8>> {
    vec![sample()]
}
