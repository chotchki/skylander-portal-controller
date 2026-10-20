//! A level's atmospheric zones, read statically (openspec AG.41, design
//! decision 2): the fog and the three directional lights a level's
//! definition script sets up at its start.
//!
//! A level's `Level_Definition_<n>.ai` calls `global.bld`'s "_ART - Add
//! Atmospheric Zone" in its STARTUP once per zone: an id, an ambient and
//! three lights (diffuse, specular, facing and tilt in degrees), the fog (on,
//! where it starts and how far it runs, in feet, its colour, the sky dome's
//! share and colour, the colour past the far clip) and depth-of-field and
//! shadow-volume settings. The macro keeps one zone per id, a later call for
//! the same id rewriting it, and sets zone 0 up as it adds it ("Game - Setup
//! Atmospherics": directional light 1's ambient, each light's colours and
//! angles, the viewport's fog). Level 213 adds its first two zones under
//! `if system.hardware platform == 3`, the Wii's branch, and the PS3's in its
//! else; the PS3 is 1 (the member is the enum "XBox 360|PS 3|PC|Wii|WiiU",
//! aj-vm).
//!
//! This reader steps STARTUP's ops as a PS3 runs them: a check on the
//! hardware platform is evaluated (ah-progression's comparison codes), each
//! zone call's arguments are read (literals, as level 213's are) and the
//! macro body's rules applied (the fog colour as the colour past the far
//! clip when the call says so). It refuses what it doesn't model, by name,
//! rather than guess: a zone call under any other condition or inside
//! another op's block, an argument that isn't a literal of its parameter's
//! class, a parameter the macro doesn't have or one the reader needs that a
//! call leaves out, "use lights from zone" or "use fog from zone" naming a
//! zone (copying one), and in STARTUP the macros that remove, switch or set
//! up zones by other ways. Colours are packed words, low byte red: AG.13
//! read the fog's against the live viewport. Replaying the macro by hand is
//! the narrow rewrite AE.2's toy table makes, on its terms: AJ's VM runs the
//! definition itself and must leave the same zone set up.

use crate::igz::Igz;
use crate::igz::layouts::{
    BRANCH_END, Layout, NO_OPERATOR, ObjectRef, OpCode, OpCodeList, OpDefineMacro,
    OpMacroParameter, OpUseMacro, RhsValueStack, ScriptInfo, Stack, TypeRef, ValueRhsVariant, is_a,
    read_all,
};
use crate::script::{Class, OpCheckValue};
use crate::{ErrorKind, FormatError};

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

#[cfg(test)]
mod tests;

const FORMAT: &str = "atmosphere";

/// A level's definition script, by the start of its file name
/// (`Level_Definition_213.ai`).
pub const SCRIPT_PREFIX: &str = "Level_Definition_";
pub const ADD_ZONE: &str = "_ART - Add Atmospheric Zone";
/// The other macros that change the zones or which one is set up; a call
/// to one in STARTUP is refused.
pub const OTHER_MACROS: [&str; 4] = [
    "_ART - Remove Atmospheric Zone",
    "Game - Switch Atmospherics Zone",
    "Game - Setup Atmospherics",
    "Game - Setup Atmospherics From Zone ID",
];
/// The zone the add macro sets up as it adds it.
pub const SET_UP: i32 = 0;
/// `system.hardware platform` on a PS3.
pub const PS3: i32 = 1;
/// The member a platform check reads: its namespace and name.
pub const PLATFORM: (&str, &str) = ("system", "hardware platform");

/// The add macro's parameters, as the disc names them. The depth of field's
/// and the shadow volume's are taken and left unread.
pub mod params {
    pub const ZONE_ID: &str = "zone ID";
    pub const LIGHTS_FROM: &str = "use lights from zone";
    pub const AMBIENT: &str = "light ambient";
    /// Per light, 1 to 3: diffuse, specular, facing and tilt.
    pub const LIGHTS: [[&str; 4]; 3] = [
        [
            "light 1 diffuse",
            "light 1 specular",
            "light 1 facing",
            "light 1 tilt",
        ],
        [
            "light 2 diffuse",
            "light 2 specular",
            "light 2 facing",
            "light 2 tilt",
        ],
        [
            "light 3 diffuse",
            "light 3 specular",
            "light 3 facing",
            "light 3 tilt",
        ],
    ];
    pub const FOG_FROM: &str = "use fog from zone";
    pub const FOG_ON: &str = "fog on";
    pub const FOG_START: &str = "fog range start";
    pub const FOG_RANGE: &str = "fog range";
    pub const FOG_COLOUR: &str = "fog color";
    pub const SKY_DOME_PERCENT: &str = "fog skydome percent";
    pub const SKY_DOME_COLOUR: &str = "fog skydome color";
    pub const YON_FROM_FOG: &str = "use fog color as yon ignore fog color";
    pub const YON_COLOUR: &str = "fog yon ignore color";
    pub const UNREAD: [&str; 12] = [
        "use depth of field from zone",
        "depth of field on",
        "depth of field level",
        "depth of field focus distance",
        "depth of field focus width",
        "depth of field focus ramp type",
        "use shadow volume from zone",
        "shadow volume facing",
        "shadow volume tilt",
        "shadow volume distance",
        "shadow volume field of view",
        "shadow volume intensity",
    ];
}

/// A packed colour, low byte first: red, green, blue and the top byte.
pub type Colour = [u8; 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Light {
    pub diffuse: Colour,
    pub specular: Colour,
    /// Degrees, as a `tfbEulerTransform`'s heading and tilt.
    pub facing: i32,
    pub tilt: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fog {
    /// What the viewport's fog max is set to: 1 on, 0 off.
    pub on: i32,
    /// Feet along the view where it starts, and how far past that it runs.
    pub start: i32,
    pub range: i32,
    pub colour: Colour,
    /// The sky dome's share of the fog, in percent, and its colour.
    pub sky_dome_percent: i32,
    pub sky_dome_colour: Colour,
    /// The colour past the far clip.
    pub yon_colour: Colour,
}

/// One zone as the last call for its id leaves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    pub id: i32,
    /// The last call's place in the op list.
    pub position: usize,
    /// Directional light 1's ambient; the zone sets no other.
    pub ambient: Colour,
    pub lights: [Light; 3],
    pub fog: Fog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atmosphere<'a> {
    /// The definition script's source path.
    pub script: &'a str,
    /// Every zone the PS3's start adds, in the order each id was first
    /// added.
    pub zones: Vec<Zone>,
    /// Into `zones`: the zone set up at the start ([`SET_UP`]), if added.
    pub start: Option<usize>,
}

impl<'a> Atmosphere<'a> {
    /// Reads the zones out of a level's `level.bld`.
    pub fn read(igz: &Igz<'a>) -> Result<Self, FormatError> {
        let (script, ops) = definition(igz)?;
        let mut out = Atmosphere {
            script,
            zones: Vec::new(),
            start: None,
        };
        Reader { igz, ops: &ops }.startup(&mut out)?;
        Ok(out)
    }

    /// The zone set up at the start; `None` when STARTUP adds no zone 0.
    pub fn start_zone(&self) -> Option<&Zone> {
        self.zones.get(self.start?)
    }
}

fn refuse(field: &'static str, kind: ErrorKind) -> FormatError {
    FormatError::new(FORMAT, field, 0, kind)
}

fn unsupported(field: &'static str, position: usize) -> FormatError {
    refuse(
        field,
        ErrorKind::Unsupported {
            value: position as u64,
        },
    )
}

/// The one script whose file name starts with [`SCRIPT_PREFIX`], and its
/// op list.
fn definition<'a>(igz: &Igz<'a>) -> Result<(&'a str, Vec<usize>), FormatError> {
    let mut found = None;
    for (i, _) in read_all::<ScriptInfo>(igz) {
        let script = ScriptInfo::read(igz, i)?;
        let Some(name) = script.info.name else {
            continue;
        };
        let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
        if !(file.starts_with(SCRIPT_PREFIX) && file.ends_with(".ai")) {
            continue;
        }
        if found.is_some() {
            return Err(refuse("tfbScriptInfo", ErrorKind::Duplicate));
        }
        found = Some((name, script.op_list));
    }
    let (name, list) = found.ok_or_else(|| refuse("tfbScriptInfo", ErrorKind::Missing))?;
    let list = list.ok_or_else(|| refuse("_opList", ErrorKind::Missing))?;
    Ok((name, OpCodeList::read(igz, list)?.ops))
}

/// An argument followed to its literal: its class and bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Literal<'a> {
    class: &'a str,
    bits: i32,
}

/// The `metaobject` class a type word names.
fn class_name(t: TypeRef<'_>) -> Option<&str> {
    match t {
        TypeRef::Named { name, .. }
        | TypeRef::Indexed {
            name: Some(name), ..
        } if name.namespace == "metaobject" => Some(name.name),
        _ => None,
    }
}

struct Reader<'f, 'a> {
    igz: &'f Igz<'a>,
    ops: &'f [usize],
}

impl<'a> Reader<'_, 'a> {
    fn type_of(&self, object: usize) -> &'a str {
        self.igz.type_name(&self.igz.objects()[object])
    }

    fn class(&self, p: usize) -> Option<Class> {
        Class::from_type(self.type_of(self.ops[p]))
    }

    /// The last position of the block the op at `p` opens, inside
    /// `within`'s; `None` for one that opens none. A `flow end` (whose
    /// length is [`BRANCH_END`]) opens none.
    fn block_end(&self, p: usize, within: usize) -> Result<Option<usize>, FormatError> {
        let code = OpCode::read(self.igz, self.ops[p])?;
        match code.block_end(p) {
            Some(end) if end <= within => Ok(Some(end)),
            Some(end) => Err(refuse(
                "_branchPC",
                ErrorKind::OutOfRange {
                    value: end as u64,
                    limit: within as u64,
                },
            )),
            None => Ok(None),
        }
    }

    /// The macro a call names, by its definition's name; `None` for one
    /// named by anything but a definition (or its stub) in this file.
    fn macro_name(&self, p: usize) -> Result<Option<&'a str>, FormatError> {
        let call = OpUseMacro::read(self.igz, self.ops[p])?;
        let Some(np) = call.np else { return Ok(None) };
        let stack = Stack::read(self.igz, np)?;
        let [ObjectRef::Object(def)] = stack.path[..] else {
            return Ok(None);
        };
        if !is_a(self.type_of(def), OpDefineMacro::TYPE) {
            return Ok(None);
        }
        Ok(OpDefineMacro::read(self.igz, def)?.op.name)
    }

    /// Whether the op at `p` calls [`ADD_ZONE`] or one of [`OTHER_MACROS`].
    fn atmospheric(&self, p: usize) -> Result<bool, FormatError> {
        if self.class(p) != Some(Class::UseMacro) {
            return Ok(false);
        }
        let name = self.macro_name(p)?;
        Ok(name.is_some_and(|n| n == ADD_ZONE || OTHER_MACROS.contains(&n)))
    }

    /// Refuses a zone call, or another atmosphere macro's, anywhere in
    /// `from..=to`: a block this reader doesn't run.
    fn none_in(&self, from: usize, to: usize) -> Result<(), FormatError> {
        for q in from..=to {
            if self.atmospheric(q)? {
                return Err(unsupported("an atmosphere macro inside a block", q));
            }
        }
        Ok(())
    }

    /// A value operand followed to its literal: an `RHSValueStack` with an
    /// empty path, perhaps inside `ValueRHSVariant`s with no operator.
    fn literal(&self, r: ObjectRef<'_>, field: &'static str) -> Result<Literal<'a>, FormatError> {
        let ObjectRef::Object(mut o) = r else {
            return Err(refuse(field, ErrorKind::Missing));
        };
        let mut depth = 0;
        while is_a(self.type_of(o), ValueRhsVariant::TYPE) {
            depth += 1;
            if depth > crate::toys::MAX_WRAPPING {
                return Err(refuse(
                    field,
                    ErrorKind::OutOfRange {
                        value: depth as u64,
                        limit: crate::toys::MAX_WRAPPING as u64,
                    },
                ));
            }
            let v = ValueRhsVariant::read(self.igz, o)?;
            match (v.op1, v.operator) {
                (ObjectRef::Object(inner), NO_OPERATOR) => o = inner,
                _ => return Err(unsupported(field, o)),
            }
        }
        let stack = RhsValueStack::read(self.igz, o)?;
        if !stack.stack.path.is_empty() {
            return Err(unsupported(field, o));
        }
        let class =
            class_name(stack.value_type).ok_or_else(|| refuse(field, ErrorKind::Missing))?;
        Ok(Literal {
            class,
            bits: stack.value,
        })
    }

    /// Whether the check at `p` is on the hardware platform and, if so,
    /// whether it holds on a PS3.
    fn platform(&self, p: usize) -> Result<Option<bool>, FormatError> {
        let check = OpCheckValue::read(self.igz, self.ops[p])?;
        let ObjectRef::Object(lhs) = check.lhs else {
            return Ok(None);
        };
        if !is_a(self.type_of(lhs), Stack::TYPE) {
            return Ok(None);
        }
        let path = Stack::read(self.igz, lhs)?.path;
        let [ObjectRef::Import(member)] = path[..] else {
            return Ok(None);
        };
        if !member.id.is(PLATFORM.0, PLATFORM.1) {
            return Ok(None);
        }
        let rhs = self.literal(check.rhs, "_RHS")?;
        if rhs.class != "IntMeasurement" {
            return Err(unsupported("_RHS", p));
        }
        let holds = match check.relation {
            0 => PS3 <= rhs.bits,
            1 => PS3 == rhs.bits,
            2 => PS3 >= rhs.bits,
            3 => PS3 < rhs.bits,
            4 => PS3 > rhs.bits,
            5 => PS3 != rhs.bits,
            other => {
                return Err(refuse(
                    "_relOperator",
                    ErrorKind::Unsupported {
                        value: other.into(),
                    },
                ));
            }
        };
        Ok(Some(holds))
    }

    /// The else after a check whose block ends at `last`: the last op of the
    /// block, a skip, and the end of the block it skips (`last` itself when
    /// it skips none). `None` when the block has no else.
    fn else_end(&self, last: usize, within: usize) -> Result<Option<usize>, FormatError> {
        if self.class(last) != Some(Class::Skip) {
            return Ok(None);
        }
        let code = OpCode::read(self.igz, self.ops[last])?;
        if code.branch == BRANCH_END {
            return Err(unsupported("_branchPC", last));
        }
        Ok(Some(self.block_end(last, within)?.unwrap_or(last)))
    }

    /// STARTUP's ops, run as a PS3 runs them, each zone call read.
    fn startup(&self, out: &mut Atmosphere<'a>) -> Result<(), FormatError> {
        let Some(at) = (0..self.ops.len()).find(|&p| self.class(p) == Some(Class::StartUp)) else {
            return Err(refuse("STARTUP", ErrorKind::Missing));
        };
        let Some(end) = self.block_end(at, self.ops.len() - 1)? else {
            return Ok(());
        };
        // Each platform check that held and has an else: where its block's
        // else sits, and the end of the else's block, which is skipped.
        let mut held: Vec<(usize, usize)> = Vec::new();
        let mut p = at + 1;
        while p <= end {
            held.retain(|&(skip, _)| p <= skip);
            match self.class(p) {
                Some(Class::CheckValue) => {
                    let last = self.block_end(p, end)?.unwrap_or(p);
                    let other = self.else_end(last, end)?;
                    match self.platform(p)? {
                        Some(true) => {
                            if let Some(e) = other {
                                held.push((last, e));
                            }
                            p += 1;
                        }
                        // The else, if any, runs on from the block's end.
                        Some(false) => p = last + 1,
                        None => {
                            let to = other.unwrap_or(last);
                            self.none_in(p + 1, to)?;
                            p = to + 1;
                        }
                    }
                }
                Some(Class::Skip) => match held.pop() {
                    Some((skip, e)) if skip == p => p = e + 1,
                    _ => return Err(unsupported("an else this reader didn't open", p)),
                },
                Some(Class::UseMacro) => {
                    let last = self
                        .block_end(p, end)?
                        .ok_or_else(|| unsupported("a call without its block", p))?;
                    match self.macro_name(p)? {
                        Some(ADD_ZONE) => self.zone(p, last, out)?,
                        Some(n) if OTHER_MACROS.contains(&n) => {
                            return Err(unsupported("an atmosphere macro in STARTUP", p));
                        }
                        _ => self.none_in(p + 1, last)?,
                    }
                    p = last + 1;
                }
                // `flow end` ends the run.
                Some(Class::FlowEnd) => break,
                _ => match self.block_end(p, end)? {
                    Some(last) => {
                        self.none_in(p + 1, last)?;
                        p = last + 1;
                    }
                    None => p += 1,
                },
            }
        }
        Ok(())
    }

    /// The zone call at `p`, whose block ends at `last`: its arguments, the
    /// macro body's rules, the zone kept by id.
    fn zone(&self, p: usize, last: usize, out: &mut Atmosphere<'a>) -> Result<(), FormatError> {
        let iface = p + 1;
        if iface > last || self.class(iface) != Some(Class::MacroInterface) {
            return Err(refuse("INTERFACE", ErrorKind::Missing));
        }
        let iface_end = self
            .block_end(iface, last)?
            .ok_or_else(|| refuse("INTERFACE", ErrorKind::Missing))?;
        let mut args: Vec<(&'a str, Literal<'a>)> = Vec::new();
        for q in iface + 1..=iface_end {
            match self.class(q) {
                Some(Class::FlowMacro) => {}
                Some(Class::MacroParameter) => {
                    let param = OpMacroParameter::read(self.igz, self.ops[q])?;
                    let name = param.var.name.unwrap_or("");
                    if args.iter().any(|(n, _)| *n == name) {
                        return Err(refuse("_varName", ErrorKind::Duplicate));
                    }
                    if params::UNREAD.contains(&name) {
                        continue;
                    }
                    if !known(name) {
                        return Err(unsupported("_varName", q));
                    }
                    if param.index_rhs != ObjectRef::Null {
                        return Err(unsupported("_indexRHS", q));
                    }
                    args.push((name, self.literal(param.rhs, "_RHS")?));
                }
                _ => return Err(unsupported("INTERFACE", q)),
            }
        }
        // Anything the caller runs after the return.
        self.none_in(iface_end + 1, last)?;
        let get = |name: &'static str, class: &'static str| -> Result<i32, FormatError> {
            let (_, l) = args
                .iter()
                .find(|(n, _)| *n == name)
                .ok_or_else(|| refuse(name, ErrorKind::Missing))?;
            if l.class != class {
                return Err(refuse(
                    name,
                    ErrorKind::WrongType {
                        expected: class,
                        found: l.class.to_owned(),
                    },
                ));
            }
            Ok(l.bits)
        };
        let int = |name| get(name, "IntMeasurement");
        let colour = |name| get(name, "ColorMeasurement").map(|bits| bits.to_le_bytes());
        for from in [params::LIGHTS_FROM, params::FOG_FROM] {
            if int(from)? >= 0 {
                return Err(unsupported(from, p));
            }
        }
        let mut lights = [Light {
            diffuse: [0; 4],
            specular: [0; 4],
            facing: 0,
            tilt: 0,
        }; 3];
        for (light, [diffuse, specular, facing, tilt]) in lights.iter_mut().zip(params::LIGHTS) {
            *light = Light {
                diffuse: colour(diffuse)?,
                specular: colour(specular)?,
                facing: int(facing)?,
                tilt: int(tilt)?,
            };
        }
        let fog_colour = colour(params::FOG_COLOUR)?;
        let zone = Zone {
            id: int(params::ZONE_ID)?,
            position: p,
            ambient: colour(params::AMBIENT)?,
            lights,
            fog: Fog {
                on: int(params::FOG_ON)?,
                start: int(params::FOG_START)?,
                range: int(params::FOG_RANGE)?,
                colour: fog_colour,
                sky_dome_percent: int(params::SKY_DOME_PERCENT)?,
                sky_dome_colour: colour(params::SKY_DOME_COLOUR)?,
                // The body's rule: over 0 takes the fog's colour.
                yon_colour: if int(params::YON_FROM_FOG)? > 0 {
                    fog_colour
                } else {
                    colour(params::YON_COLOUR)?
                },
            },
        };
        let at = match out.zones.iter().position(|z| z.id == zone.id) {
            Some(i) => {
                out.zones[i] = zone;
                i
            }
            None => {
                out.zones.push(zone);
                out.zones.len() - 1
            }
        };
        if zone.id == SET_UP {
            out.start = Some(at);
        }
        Ok(())
    }
}

/// A parameter of the add macro's that the reader reads.
fn known(name: &str) -> bool {
    [
        params::ZONE_ID,
        params::LIGHTS_FROM,
        params::AMBIENT,
        params::FOG_FROM,
        params::FOG_ON,
        params::FOG_START,
        params::FOG_RANGE,
        params::FOG_COLOUR,
        params::SKY_DOME_PERCENT,
        params::SKY_DOME_COLOUR,
        params::YON_FROM_FOG,
        params::YON_COLOUR,
    ]
    .contains(&name)
        || params::LIGHTS.iter().flatten().any(|n| *n == name)
}
