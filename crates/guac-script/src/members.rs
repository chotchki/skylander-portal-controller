//! The engine members the scripts reach, each as the original registers it
//! (aj-vm's live member table: its measurement class, units and range) and
//! where guacamole keeps it (design decision 21): the VM's own, the host's
//! (guacamole's state, read and written through [`crate::Host`]), or a value
//! cell (a cosmetic member nothing draws: it reads its registered default
//! and keeps what's written). Grown root by root, each member box adding
//! what its roots reach (design decision 4); [`INIT_CHAIN`] is AH.16's.

use crate::host::{Kind, MemberSpec, Members};
use crate::value::Value;

/// Which side answers a member's reads and writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keeper {
    /// The VM computes it (a packed value's part, the tick's own numbers).
    Vm,
    /// The host answers it.
    Host,
    /// A value cell: cosmetic, nothing draws or plays it yet.
    Cell,
}

/// A member as the original registers it, and who keeps it here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Registered {
    /// The class namespace less any `tfb`, lowercased (`system`).
    pub namespace: &'static str,
    /// Lowercased, any leading `-` dropped (`hardware platform`).
    pub name: &'static str,
    /// The registered measurement class (`EnumMeasurement`).
    pub class: &'static str,
    /// The registered units: a unit, an enum's labels, or none.
    pub units: &'static str,
    /// The registered range; writes clamp to it only when either end is
    /// nonzero (crate docs, Members).
    pub min: f32,
    pub max: f32,
    pub keeper: Keeper,
}

impl Registered {
    /// How a write converts, from the registered class.
    pub fn kind(&self) -> Kind {
        match self.class {
            "FloatMeasurement" | "PairedFloatMeasurement" => Kind::Float,
            "IntMeasurement"
            | "EnumMeasurement"
            | "ButtonMeasurement"
            | "PairedEnumMeasurement" => Kind::Int,
            "ColorMeasurement" => Kind::Color,
            "ScreenMeasurement" => Kind::Screen,
            // An engine set the host keeps is read whole; a value cell
            // standing for one reads none.
            "SetVariant" if self.keeper == Keeper::Host => Kind::Set,
            _ => Kind::Reference,
        }
    }

    pub fn spec(&self) -> MemberSpec {
        MemberSpec::new(self.kind(), self.min, self.max)
    }

    /// `namespace.name`, as halts and logs name it.
    pub fn path(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }
}

const fn m(
    namespace: &'static str,
    name: &'static str,
    class: &'static str,
    units: &'static str,
    min: f32,
    max: f32,
    keeper: Keeper,
) -> Registered {
    Registered {
        namespace,
        name,
        class,
        units,
        min,
        max,
        keeper,
    }
}

/// Up to 2^31 - 1, as the registry writes an open top.
const OPEN: f32 = 2_147_483_647.0;

use Keeper::{Cell, Host, Vm};

/// Every engine member AH.16's init chain reaches past the VM's own path
/// builtins (`my`, `null`, `undefined`, `its`, `valuedereferencer`, the
/// set's `count`, `first` and `last`, `cast to integer`): `init_setup`'s
/// first tick, `global.bld`'s 17 running placements' first ticks, level
/// 213's definition and its enemy system's stats, over 600 ticks.
pub const INIT_CHAIN: &[Registered] = &[
    // The VM's: a packed screen position's halves and a colour's bytes,
    // and the placement's own clock.
    m(
        "screenmeasurement",
        "x",
        "FloatMeasurement",
        "",
        0.0,
        0.0,
        Vm,
    ),
    m(
        "screenmeasurement",
        "y",
        "FloatMeasurement",
        "",
        0.0,
        0.0,
        Vm,
    ),
    m(
        "colormeasurement",
        "red",
        "IntMeasurement",
        "",
        0.0,
        255.0,
        Vm,
    ),
    m(
        "colormeasurement",
        "green",
        "IntMeasurement",
        "",
        0.0,
        255.0,
        Vm,
    ),
    m(
        "colormeasurement",
        "blue",
        "IntMeasurement",
        "",
        0.0,
        255.0,
        Vm,
    ),
    m(
        "colormeasurement",
        "alpha",
        "IntMeasurement",
        "",
        0.0,
        255.0,
        Vm,
    ),
    m(
        "system",
        "script timer",
        "FloatMeasurement",
        "seconds",
        0.0,
        OPEN,
        Vm,
    ),
    // The host's: the console and the build, the game's speed,
    // the level a script asks for, placements' numbers and the objects the
    // scripts name (the pads, the portal).
    m(
        "system",
        "hardware platform",
        "EnumMeasurement",
        "XBox 360|PS 3|PC|Wii|WiiU",
        0.0,
        4.0,
        Host,
    ),
    m(
        "system",
        "game configuration",
        "EnumMeasurement",
        "Demo|Development|QA|Golden Master",
        0.0,
        3.0,
        Host,
    ),
    m(
        "system",
        "game language",
        "EnumMeasurement",
        LANGUAGES,
        0.0,
        22.0,
        Host,
    ),
    m(
        "system",
        "console language",
        "EnumMeasurement",
        LANGUAGES,
        0.0,
        22.0,
        Host,
    ),
    m(
        "system",
        "game time scale",
        "FloatMeasurement",
        "percent",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "system",
        "load level",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "placement",
        "deactivation buffer",
        "FloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "abstractplacement",
        "id",
        "IntMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "spritematrixmeasurement",
        "id",
        "IntMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m("actorinfo", "waypoints", "SetVariant", "", 0.0, 0.0, Host),
    m(
        "scriptcontroller",
        "controller 1",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "controller 2",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "physical controller 1",
        "tfbPS3Controller",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "physical controller 2",
        "tfbPS3Controller",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "physical controller 3",
        "tfbPS3Controller",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "physical controller 4",
        "tfbPS3Controller",
        "",
        0.0,
        0.0,
        Host,
    ),
    // Not in aj-vm's interface table; the system's portal object, by the
    // name the disc imports it with.
    m(
        "redoctaneperipheralsystemobject",
        "portal",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    // Value cells: the viewport and its effects, sprites, lights, sounds,
    // strings, the save's name, the console's presence and the portal's
    // antenna. Nothing in guacamole draws, plays or shows them yet.
    m(
        "viewportinfo",
        "default",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "viewportinfo",
        "container sprite",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "viewportinfo",
        "depth of field effect",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "viewportinfo",
        "fog/caustic effect",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "viewportinfo",
        "shadows control",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "viewportinfo",
        "title safe zone",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "viewportinfo",
        "far clip",
        "FloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Cell,
    ),
    m(
        "viewportinfo",
        "3d scene refresh",
        "EnumMeasurement",
        "fast|slow",
        0.0,
        1.0,
        Cell,
    ),
    m(
        "lightinfo",
        "directional light 1",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "lightinfo",
        "directional light 2",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "lightinfo",
        "directional light 3",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "lightinfo",
        "sprite light 1",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "lightinfo",
        "sprite light 2",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "lightinfo",
        "sprite light 3",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "directionallightinfo",
        "ambient color",
        "ColorMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "directionallightinfo",
        "diffuse color",
        "ColorMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "directionallightinfo",
        "specular color",
        "ColorMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "directionallightinfo",
        "multiplier",
        "FloatMeasurement",
        "",
        0.0,
        OPEN,
        Cell,
    ),
    m(
        "directionallightinfo",
        "orientation",
        "OrientationMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "visible",
        "EnumMeasurement",
        "off|on|loading",
        0.0,
        2.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "scale",
        "ScaleMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "width",
        "FloatMeasurement",
        "pixels",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "height",
        "FloatMeasurement",
        "pixels",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "color",
        "ColorMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "location",
        "ScreenMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "priority",
        "FloatMeasurement",
        "",
        -10_000.0,
        10_000.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "alignment",
        "EnumMeasurement",
        ALIGNMENTS,
        0.0,
        8.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "align to parent",
        "EnumMeasurement",
        ALIGNMENTS,
        0.0,
        8.0,
        Cell,
    ),
    m(
        "spritematrixmeasurement",
        "attach node",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "spritematrixmeasurement",
        "attached objects",
        "SetVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "scalemeasurement",
        "uniform",
        "FloatMeasurement",
        "percent",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "soundinfo",
        "category templates",
        "SetVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "soundinfo",
        "priority",
        "IntMeasurement",
        "(0-100)",
        0.0,
        100.0,
        Cell,
    ),
    m(
        "soundinfo",
        "volume",
        "FloatMeasurement",
        "percent",
        0.0,
        OPEN,
        Cell,
    ),
    m(
        "stringinfo",
        "string converter",
        "StringConverter",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "stringconverter",
        "text",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "stringconverter",
        "value set",
        "ScriptSetReference",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "system",
        "save name",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "scriptcontroller",
        "rich presence",
        "EnumMeasurement",
        "watching presence|story mode presence|pvp mode presence|in menu presence",
        0.0,
        3.0,
        Cell,
    ),
    m(
        "redoctaneperipheralscriptobject",
        "antenna enable",
        "EnumMeasurement",
        "off|on",
        0.0,
        1.0,
        Cell,
    ),
    m(
        "sequence",
        "playback rate",
        "FloatMeasurement",
        "percent",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "sequence",
        "time remaining",
        "FloatMeasurement",
        "seconds",
        0.0,
        0.0,
        Cell,
    ),
    // Reached since AH.17's reading of `find variable`'s filter 0 links the
    // portal manager's joined data at its STARTUP: the join prompt's check
    // of the controllers no player holds.
    m(
        "scriptcontroller",
        "main stick",
        "JoystickMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "joystickmeasurement",
        "connection",
        "EnumMeasurement",
        "disconnected|connected|distressed",
        0.0,
        2.0,
        Host,
    ),
];

/// Every engine member Eruptor's scripts reach past [`INIT_CHAIN`]'s: the
/// host's (an actor's place, turn, scale, movement numbers, solid and
/// velocity, its ground, nodes, clones, contacts and sequences, the
/// measurements between two points, the ray caster, the camera and the
/// pads; what each answers is guacamole's `script_members`) and the value
/// cells his throws and eruptions set (openspec AH.17). Four take over
/// init-chain cells the host now answers.
pub const HERO: &[Registered] = &[
    m(
        "placement",
        "activation range",
        "FloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "matrixmeasurement",
        "orientation",
        "OrientationMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "orientationmeasurement",
        "facing",
        "FloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m(
        "orientationmeasurement",
        "tilt",
        "FloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m(
        "orientationmeasurement",
        "roll",
        "FloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m(
        "orientationmeasurement",
        "relative facing",
        "FloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m(
        "orientationmeasurement",
        "relative tilt",
        "FloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m(
        "orientationmeasurement",
        "relative roll",
        "FloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m("placement", "scale", "ScaleMeasurement", "", 0.0, 0.0, Host),
    m(
        "scalemeasurement",
        "uniform",
        "FloatMeasurement",
        "percent",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scalemeasurement",
        "x",
        "FloatMeasurement",
        "percent",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scalemeasurement",
        "y",
        "FloatMeasurement",
        "percent",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scalemeasurement",
        "z",
        "FloatMeasurement",
        "percent",
        0.0,
        0.0,
        Host,
    ),
    m("placement", "tint", "ColorMeasurement", "", 0.0, 0.0, Host),
    m(
        "actorinfo",
        "movement",
        "tfbScriptObject",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "actorphysics",
        "ground max speed",
        "FloatMeasurement",
        "ft. / sec.",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "ground acceleration",
        "FloatMeasurement",
        "ft. / (sec. * sec.)",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "ground deceleration",
        "FloatMeasurement",
        "ft. / (sec. * sec.)",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "ground turn rate",
        "FloatMeasurement",
        "deg. / sec.",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "air max speed",
        "FloatMeasurement",
        "ft. / sec.",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "air acceleration",
        "FloatMeasurement",
        "ft. / (sec. * sec.)",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "air deceleration",
        "FloatMeasurement",
        "ft. / (sec. * sec.)",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "air turn rate",
        "FloatMeasurement",
        "deg. / sec.",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "gravity max speed",
        "FloatMeasurement",
        "ft. / sec.",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "gravity acceleration",
        "FloatMeasurement",
        "ft. / (sec. * sec.)",
        0.0,
        0.0,
        Host,
    ),
    m(
        "actorphysics",
        "jump max speed",
        "FloatMeasurement",
        "ft. / sec.",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "jump acceleration",
        "FloatMeasurement",
        "ft. / (sec. * sec.)",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "jump initial speed",
        "FloatMeasurement",
        "ft. / sec.",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorphysics",
        "move type",
        "EnumMeasurement",
        "on foot|wheeled|unconstrained|physical",
        0.0,
        3.0,
        Host,
    ),
    m(
        "actorphysics",
        "tilts with ground",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Host,
    ),
    m(
        "actorphysics",
        "time scale",
        "FloatMeasurement",
        "percent",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorinfo",
        "collision volume",
        "tfbScriptObject",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "collisioninfo",
        "width",
        "FloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "collisioninfo",
        "length",
        "FloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "collisioninfo",
        "height",
        "FloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "collisioninfo",
        "local x offset",
        "FloatMeasurement",
        "feet",
        0.0,
        0.0,
        Host,
    ),
    m(
        "collisioninfo",
        "local y offset",
        "FloatMeasurement",
        "feet",
        0.0,
        0.0,
        Host,
    ),
    m(
        "collisioninfo",
        "local z offset",
        "FloatMeasurement",
        "feet",
        0.0,
        0.0,
        Host,
    ),
    m(
        "collisioninfo",
        "weight",
        "FloatMeasurement",
        "pounds",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "collisioninfo",
        "rebound",
        "FloatMeasurement",
        "percent",
        0.0,
        0.0,
        Host,
    ),
    m(
        "collisioninfo",
        "interacts with actors",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Host,
    ),
    m(
        "collisioninfo",
        "interacts with level mesh",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Host,
    ),
    m(
        "collisioninfo",
        "acts as actor surface",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Host,
    ),
    m(
        "collisioninfo",
        "shape",
        "EnumMeasurement",
        "none|box|capsule|sphere|cylinder|mesh",
        0.0,
        5.0,
        Host,
    ),
    m(
        "actorinfo",
        "velocity",
        "VectorMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "vectormeasurement",
        "length",
        "FloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "vectormeasurement",
        "heading",
        "FloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m(
        "vectormeasurement",
        "pitch",
        "FloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m(
        "vectormeasurement",
        "vertical difference",
        "FloatMeasurement",
        "feet",
        0.0,
        0.0,
        Host,
    ),
    m(
        "vectormeasurement",
        "horizontal length",
        "FloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "actorinfo",
        "controller",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "actorinfo",
        "attach nodes",
        "SetVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "abstractplacement",
        "attach node",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "abstractplacement",
        "attached objects",
        "SetVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "abstractplacement",
        "ground",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m("contactinfo", "type", "IntMeasurement", "", 0.0, 0.0, Host),
    m(
        "contactinfo",
        "normal",
        "VectorMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "contactinfo",
        "magnitude",
        "FloatMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "contactinfo",
        "is trigger",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Host,
    ),
    m(
        "abstractplacement",
        "clones",
        "SetVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "abstractplacement",
        "parent",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "abstractplacement",
        "activator",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "actorinfo",
        "playing animations",
        "SetVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "actorinfo",
        "playing sounds",
        "SetVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "animationinfo",
        "animation palette",
        "SetVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "sequence",
        "time remaining",
        "FloatMeasurement",
        "seconds",
        0.0,
        0.0,
        Host,
    ),
    m(
        "sequence",
        "percent remaining",
        "FloatMeasurement",
        "percent",
        0.0,
        0.0,
        Host,
    ),
    m(
        "sequence",
        "playback rate",
        "FloatMeasurement",
        "percent",
        0.0,
        0.0,
        Host,
    ),
    m(
        "sequence",
        "looping",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Host,
    ),
    m(
        "sequence",
        "raw duration",
        "FloatMeasurement",
        "seconds",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "positionmeasurement",
        "distance",
        "PairedFloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "positionmeasurement",
        "heading",
        "PairedFloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m(
        "positionmeasurement",
        "pitch",
        "PairedFloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m(
        "positionmeasurement",
        "vertical offset",
        "PairedFloatMeasurement",
        "feet",
        0.0,
        0.0,
        Host,
    ),
    m(
        "positionmeasurement",
        "horizontal distance",
        "PairedFloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "positionmeasurement",
        "separation",
        "PairedFloatMeasurement",
        "feet",
        0.0,
        0.0,
        Host,
    ),
    m(
        "positionmeasurement",
        "collision",
        "PairedEnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Host,
    ),
    m(
        "system",
        "normalized angle",
        "FloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m("system", "ray cast", "ReferenceVariant", "", 0.0, 0.0, Host),
    m("raycontact", "start", "RayEndPoint", "", 0.0, 0.0, Host),
    m("raycontact", "end", "RayEndPoint", "", 0.0, 0.0, Host),
    m(
        "raycontact",
        "has contact",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Host,
    ),
    m(
        "viewportinfo",
        "default",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "inactive controller",
        "tfbPS3Controller",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "action 1",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "action 2",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "action 3",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "action 4",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "accept button",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "cancel button",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "alt button",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "pause button",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "start button",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "select button",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "dpad left",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "dpad right",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "dpad up",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "scriptcontroller",
        "dpad down",
        "ButtonMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "buttonmeasurement",
        "pressure state",
        "EnumMeasurement",
        "just released|released|pressed|just pressed",
        0.0,
        3.0,
        Host,
    ),
    m(
        "buttonmeasurement",
        "pressure value",
        "FloatMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "joystickmeasurement",
        "x",
        "FloatMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "joystickmeasurement",
        "y",
        "FloatMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "joystickmeasurement",
        "heading",
        "FloatMeasurement",
        "degrees",
        0.0,
        0.0,
        Host,
    ),
    m(
        "joystickmeasurement",
        "magnitude",
        "FloatMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    // What a throw and an eruption reach past the walk: a projectile's
    // contacts and where it set off from, a reference's own point.
    m(
        "collisioninfo",
        "environment contacts",
        "SetVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "abstractplacement",
        "origin",
        "MatrixMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "placementreference",
        "remembered",
        "MatrixMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    // Cosmetic: the HUD's sprites, bloom, shadows, particles' generators.
    // A placed light's colours, multiplier and attenuation are the host's
    // (openspec AH.65): the point-light manager copies them onto its pool's
    // lights and the TV draws what they hold.
    m(
        "actorinfo",
        "bloom intensity",
        "IntMeasurement",
        "percent",
        0.0,
        100.0,
        Cell,
    ),
    m(
        "placement",
        "shadow cast",
        "EnumMeasurement",
        "none|normal|no fade",
        0.0,
        2.0,
        Cell,
    ),
    m(
        "particleinfo",
        "generators",
        "SetVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "lightinfo",
        "ambient color",
        "ColorMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "lightinfo",
        "diffuse color",
        "ColorMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "lightinfo",
        "specular color",
        "ColorMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "lightinfo",
        "multiplier",
        "FloatMeasurement",
        "",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "lightinfo",
        "constant attenuation factor",
        "FloatMeasurement",
        "",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "lightinfo",
        "linear attenuation factor",
        "FloatMeasurement",
        "",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "lightinfo",
        "quadratic attenuation factor",
        "FloatMeasurement",
        "",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "spriteinfo",
        "blend mode",
        "EnumMeasurement",
        "opaque|crossfade|additive|cuts out|gets",
        0.0,
        7.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "justify",
        "EnumMeasurement",
        "left|center|right",
        0.0,
        2.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "natural size",
        "ScreenMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "numeric value",
        "SpriteValueMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "point size",
        "IntMeasurement",
        "pixels",
        0.0,
        OPEN,
        Cell,
    ),
    m(
        "spriteinfo",
        "replace color",
        "ColorMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "screen location",
        "ScreenMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m("spriteinfo", "text", "ReferenceVariant", "", 0.0, 0.0, Cell),
    m(
        "spriteinfo",
        "texture flip",
        "EnumMeasurement",
        "none|horizontal|vertical|both",
        0.0,
        3.0,
        Cell,
    ),
    m(
        "spritematrixmeasurement",
        "orientation",
        "OrientationMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "spritematrixmeasurement",
        "parent",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "system",
        "object namer",
        "NameExtractor",
        "",
        0.0,
        0.0,
        Cell,
    ),
    // A clip's or a sequence's animation info (openspec AH.49), the host's:
    // `type` (read only) from the palette's partial and additive flags,
    // `is layered` from those or the layered bit a script sets on a clip
    // (a sequence started from it takes it), `blend percent` the weight the
    // engine blends a sequence by (100 until written), as the locomotion
    // script writes them every tick.
    m(
        "animationinfo",
        "is layered",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Host,
    ),
    m(
        "animationinfo",
        "type",
        "EnumMeasurement",
        "Base|Partial|Additive",
        0.0,
        2.0,
        Host,
    ),
    m(
        "animationinfo",
        "blend percent",
        "FloatMeasurement",
        "percent",
        0.0,
        100.0,
        Host,
    ),
    // The sounds playing (the landing's check for its stinger, AH.49):
    // cosmetic, nothing plays a sound, so the set reads none.
    m(
        "soundinfo",
        "active sound effects",
        "SetVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
];

/// Every engine member chapter 1's first fight reaches past [`INIT_CHAIN`]'s
/// and [`HERO`]'s (openspec AH.18): the four Chompies' script, Setup_001's
/// controller, Norticus as far as the encounter reaches him, the level's
/// enemy system and `global.bld`'s mover update over the fight.
pub const CREATURES: &[Registered] = &[
    // The host's: the engine's sets of placements asleep and taken off, and
    // the actors whose solids touch one's.
    m("actorinfo", "inactive", "SetVariant", "", 0.0, 0.0, Host),
    m("actorinfo", "removed", "SetVariant", "", 0.0, 0.0, Host),
    m(
        "collisioninfo",
        "contacts with me",
        "SetVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    // Value cells: what's drawn (a hurt's tint, shadows, a cutscene's view)
    // and what a sound or a cutscene the whitelist doesn't run reads.
    m(
        "placement",
        "shadow receive",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Cell,
    ),
    m(
        "actorinfo",
        "replace color",
        "ColorMeasurement",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "actorinfo",
        "replace color percentage",
        "IntMeasurement",
        "percent",
        0.0,
        100.0,
        Cell,
    ),
    m(
        "actorinfo",
        "draw behind enabled",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Cell,
    ),
    m("actorinfo", "npcs", "SetVariant", "", 0.0, 0.0, Cell),
    m(
        "spriteinfo",
        "replace color percentage",
        "IntMeasurement",
        "percent",
        0.0,
        100.0,
        Cell,
    ),
    m(
        "clonedsequence",
        "parent",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "animationinfo",
        "ambient",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "viewportinfo",
        "view converter",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "viewportinfo",
        "field of view",
        "FloatMeasurement",
        "degrees",
        0.0,
        180.0,
        Cell,
    ),
];

/// What AH.19's hurt and kill chain reach past [`INIT_CHAIN`], [`HERO`] and
/// [`CREATURES`]: a slider's own members, the VM's (crate docs, Slides). A
/// push eases its target's deceleration through a slider and the hurt
/// flash and health bar's sprite shakes loop theirs; the rest of a
/// slider's (`time remaining`, `percent remaining`, `playback rate`,
/// `looping`, `raw duration`) are a sequence's too, registered as the
/// hero's sequences have them, and the VM answers them on a slider first.
pub const COMBAT: &[Registered] = &[
    m(
        "sequence",
        "playback mode",
        "EnumMeasurement",
        "one way|ping pong",
        0.0,
        1.0,
        Vm,
    ),
    m(
        "slider",
        "ignores time scales",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Vm,
    ),
];

/// What AH.20's player loop, orbs and coins reach past AH.16-AH.19's
/// tables, and a defeated hero's kill (openspec AH.20): each as the
/// original registers it.
pub const LOOP: &[Registered] = &[
    // A coin's fade by distance as it's placed: cosmetic.
    m(
        "placement",
        "near/far alpha fade",
        "EnumMeasurement",
        "off|on",
        0.0,
        1.0,
        Cell,
    ),
    // The world's origin as a point (a hero's death puts the measurement
    // actor there): the host's.
    m(
        "system",
        "absolute zero",
        "PositionMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    // A hero's death snapshot for the data mining: the portal's battery
    // (no battery: the phone is the portal), the console's online object
    // (none) and the save data it fills. Value cells: nothing reads them
    // back but the snapshot.
    m(
        "redoctaneperipheralscriptobject",
        "battery remaining",
        "FloatMeasurement",
        "per cent",
        0.0,
        100.0,
        Cell,
    ),
    m(
        "system",
        "online",
        "tfbOnlineScriptObject",
        "",
        0.0,
        0.0,
        Cell,
    ),
    m(
        "system",
        "save data",
        "ScriptSetReference",
        "",
        0.0,
        0.0,
        Cell,
    ),
];

/// What AH.23's figure on the portal reaches past AH.16-AH.21's tables:
/// the portal's two data regions, which `Save - Set up Tag Data Fields`
/// sizes a tag's save area by, each as the original registers it (bytes,
/// 0 to 448). The host's: a figure's tag holds 112 bytes in its first
/// region and 64 in its second, as AH.11's live `save data area` of 44
/// words gives (12 words past the first region's 16 and 12 past the
/// second's 4). Nothing on the disc writes either.
pub const FIGURE: &[Registered] = &[
    m(
        "redoctaneperipheralscriptobject",
        "size of region 1",
        "IntMeasurement",
        "bytes",
        0.0,
        448.0,
        Host,
    ),
    m(
        "redoctaneperipheralscriptobject",
        "size of region 2",
        "IntMeasurement",
        "bytes",
        0.0,
        448.0,
        Host,
    ),
];

/// What AG.28's gameplay camera reaches past the earlier tables: the
/// camera's scripts (`CameraSystem.ai`, `CameraControl.ai`, run each tick
/// by the portal manager's) place and turn the viewport, set its picture,
/// project points through it for the framing zoom and ask for a second
/// player for the co-op tether, each as the original registers it (aj-vm's
/// live table). The viewport's field of view, far clip, view converter
/// and container sprite were cells of the init chain's and the fight's
/// tables; the camera's host takes them over, registered the same.
pub const CAMERA: &[Registered] = &[
    m("actorinfo", "players", "SetVariant", "", 0.0, 0.0, Host),
    m(
        "viewportinfo",
        "field of view",
        "FloatMeasurement",
        "degrees",
        0.0,
        180.0,
        Host,
    ),
    m(
        "viewportinfo",
        "far clip",
        "FloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Host,
    ),
    m(
        "viewportinfo",
        "view converter",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "viewportinfo",
        "container sprite",
        "ReferenceVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "viewconverter",
        "world position",
        "PositionMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "viewconverter",
        "screen location",
        "ScreenMeasurement",
        "",
        0.0,
        0.0,
        Host,
    ),
    // Which side the converter holds still: the camera's scripts write
    // the world position's (0) before every projection, nothing reads it.
    m(
        "viewconverter",
        "lock type",
        "EnumMeasurement",
        "lock world position|lock screen location",
        0.0,
        1.0,
        Cell,
    ),
    m(
        "spriteinfo",
        "screen width",
        "FloatMeasurement",
        "pixels",
        0.0,
        0.0,
        Host,
    ),
    m(
        "spriteinfo",
        "screen height",
        "FloatMeasurement",
        "pixels",
        0.0,
        0.0,
        Host,
    ),
    // How far off a sound falls silent, a value cell as the sounds' are
    // (nothing plays them yet): the sliding rocks' activator reads it as it
    // starts the slide's sound (`Sound - Ultra Sound`, `MusicController.ai`
    // #1227), after opening the rocks and before its switch.
    m(
        "soundinfo",
        "attenuation distance silent",
        "FloatMeasurement",
        "feet",
        0.0,
        OPEN,
        Cell,
    ),
];

/// What the walkers' scripts reach past every table above (openspec AG.160):
/// the members a strict run of each modelled character over level 213 stopped
/// on, as aj-vm's live table registers them. A Giant's footstep reads the
/// actors his solid touches (`Minion - Titan Abilities`: past 5 ft/s, a
/// destructible touching him is hurt); a particle clone's draw order is
/// cosmetic here, nothing drawing particles yet; whether a sequence may start
/// again while it plays is the host's, which a play of a clip already playing
/// on its actor reads (openspec AH.184; the walkers set it only on sounds,
/// which play nothing yet).
pub const WALKERS: &[Registered] = &[
    m(
        "collisioninfo",
        "object collisions",
        "SetVariant",
        "",
        0.0,
        0.0,
        Host,
    ),
    m(
        "placement",
        "bring to front",
        "EnumMeasurement",
        "nope|expensive|cheap",
        0.0,
        2.0,
        Cell,
    ),
    m(
        "sequence",
        "allow restart",
        "EnumMeasurement",
        "false|true",
        0.0,
        1.0,
        Host,
    ),
];

/// `system.game language`'s and `console language`'s labels: English is 6.
pub const LANGUAGES: &str = "Arabic|Bulgarian|Chinese|Czech|Danish|Dutch|English|Finnish|French|German|Greek|Hebrew|Hindi|Hungarian|Italian|Japanese|Korean|Norwegian|Polish|Portuguese|Russian|Spanish|Swedish";

/// A sprite's alignments, nine.
const ALIGNMENTS: &str = "top-left|top-middle|top-right|center-left|center-middle|center-right|bottom-left|bottom-middle|bottom-right";

/// An enum member's value for `label`, by its registered labels.
pub fn label(r: &Registered, label: &str) -> Option<Value> {
    r.units
        .split('|')
        .position(|l| l == label)
        .map(|i| Value::Int(i as i32))
}

/// The table's entry for `namespace.name`.
pub fn find(table: &[Registered], namespace: &str, name: &str) -> Option<Registered> {
    table
        .iter()
        .find(|r| r.namespace == namespace && r.name == name)
        .copied()
}

/// Adds `table`'s host and value-cell members to `members` (the VM's own
/// need no entry).
pub fn register(members: &mut Members, table: &[Registered]) {
    for r in table {
        match r.keeper {
            Keeper::Vm => {
                members.known(r.namespace, r.name);
            }
            Keeper::Host => {
                members.host(r.namespace, r.name, r.spec());
            }
            Keeper::Cell => {
                members.value_cell(r.namespace, r.name, r.spec());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::host::{Builtin, MemberId, Place};
    use crate::value::convert;

    /// The kind a write converts as, by registered class.
    fn kind_of(r: &Registered) -> Kind {
        match (r.class, r.keeper) {
            ("FloatMeasurement" | "PairedFloatMeasurement", _) => Kind::Float,
            (
                "IntMeasurement"
                | "EnumMeasurement"
                | "PairedEnumMeasurement"
                | "ButtonMeasurement",
                _,
            ) => Kind::Int,
            ("ColorMeasurement", _) => Kind::Color,
            ("ScreenMeasurement", _) => Kind::Screen,
            ("SetVariant", Keeper::Host) => Kind::Set,
            _ => Kind::Reference,
        }
    }

    #[test]
    fn every_member_is_named_once_as_the_registry_spells_it() {
        // A member both tables name is a cell of the init chain's the hero's
        // host takes over, registered the same.
        for r in HERO {
            if let Some(i) = find(INIT_CHAIN, r.namespace, r.name) {
                assert_eq!(
                    (i.keeper, r.keeper),
                    (Keeper::Cell, Keeper::Host),
                    "{}",
                    r.path()
                );
                assert_eq!(
                    (i.class, i.units, i.min, i.max),
                    (r.class, r.units, r.min, r.max)
                );
            }
        }
        // The camera's take over cells the earlier tables registered, the
        // same class, units and range, the host's now.
        for r in CAMERA {
            for earlier in [INIT_CHAIN, HERO, CREATURES, COMBAT, LOOP, FIGURE] {
                if let Some(i) = find(earlier, r.namespace, r.name) {
                    assert_eq!(
                        (i.keeper, r.keeper),
                        (Keeper::Cell, Keeper::Host),
                        "{}",
                        r.path()
                    );
                    assert_eq!(
                        (i.class, i.units, i.min, i.max),
                        (r.class, r.units, r.min, r.max),
                        "{}",
                        r.path()
                    );
                }
            }
        }
        // The fight's members are none of the earlier tables'.
        for r in CREATURES.iter().chain(COMBAT).chain(LOOP).chain(FIGURE) {
            assert!(
                find(INIT_CHAIN, r.namespace, r.name).is_none(),
                "{}",
                r.path()
            );
            assert!(find(HERO, r.namespace, r.name).is_none(), "{}", r.path());
        }
        let mut seen = HashSet::new();
        let hero = HERO
            .iter()
            .filter(|r| find(INIT_CHAIN, r.namespace, r.name).is_none());
        let earlier: Vec<&Registered> = INIT_CHAIN
            .iter()
            .chain(hero)
            .chain(CREATURES)
            .chain(COMBAT)
            .chain(LOOP)
            .chain(FIGURE)
            .filter(|r| find(CAMERA, r.namespace, r.name).is_none())
            .collect();
        // The walkers' are none of the earlier tables' (AG.160).
        for r in WALKERS {
            for t in [INIT_CHAIN, HERO, CREATURES, COMBAT, LOOP, FIGURE, CAMERA] {
                assert!(find(t, r.namespace, r.name).is_none(), "{}", r.path());
            }
        }
        for r in earlier.into_iter().chain(CAMERA).chain(WALKERS) {
            for part in [r.namespace, r.name] {
                assert_eq!(part, part.to_ascii_lowercase(), "{}", r.path());
                assert!(!part.starts_with('-'), "{}", r.path());
            }
            assert!(!r.namespace.starts_with("tfb"), "{}", r.path());
            assert!(seen.insert(r.path()), "{} twice", r.path());
            let id = MemberId::named(r.namespace, r.name);
            // Only the placement's clock is both a path builtin and listed.
            if let Some(b) = Builtin::of(id) {
                assert_eq!(r.keeper, Keeper::Vm, "{}", b.name());
            }
        }
    }

    /// AG.160's rows by aj-vm's live table: what a Giant's footstep reads
    /// a set the host keeps, read whole; a particle's draw order and a
    /// sequence's restart enums by their labels, held to their ranges, the
    /// draw order a cell reading its first label until written, the restart
    /// the host's (AH.184).
    #[test]
    fn the_walkers_members_register_by_their_class_and_labels() {
        let objects = find(WALKERS, "collisioninfo", "object collisions").unwrap();
        assert_eq!((objects.kind(), objects.keeper), (Kind::Set, Keeper::Host));
        let front = find(WALKERS, "placement", "bring to front").unwrap();
        assert_eq!((front.kind(), front.keeper), (Kind::Int, Keeper::Cell));
        assert_eq!(label(&front, "cheap"), Some(Value::Int(2)));
        assert_eq!(label(&front, "nope"), Some(Value::Int(0)));
        assert_eq!(convert(&front.spec(), Value::Int(7)), Some(Value::Int(2)));
        assert_eq!(front.spec().default, Value::Int(0));
        let restart = find(WALKERS, "sequence", "allow restart").unwrap();
        assert_eq!((restart.kind(), restart.keeper), (Kind::Int, Keeper::Host));
        assert_eq!(label(&restart, "true"), Some(Value::Int(1)));
        assert_eq!(
            convert(&restart.spec(), Value::Float(1.0)),
            Some(Value::Int(1))
        );
        let mut members = Members::new();
        register(&mut members, WALKERS);
        for r in WALKERS {
            let (spec, place) = members.get(MemberId::named(r.namespace, r.name)).unwrap();
            assert_eq!(
                place,
                if r.keeper == Keeper::Cell {
                    Place::ValueCell
                } else {
                    Place::Host
                },
                "{}",
                r.path()
            );
            assert_eq!((spec.min, spec.max), (r.min, r.max), "{}", r.path());
        }
    }

    #[test]
    fn each_member_registers_with_its_class_range_and_keeper() {
        let mut members = Members::new();
        register(&mut members, INIT_CHAIN);
        register(&mut members, HERO);
        register(&mut members, CREATURES);
        register(&mut members, COMBAT);
        register(&mut members, LOOP);
        register(&mut members, FIGURE);
        register(&mut members, CAMERA);
        // The hero's entry wins where both tables name a member, and the
        // camera's over any earlier.
        let init = INIT_CHAIN
            .iter()
            .filter(|r| find(HERO, r.namespace, r.name).is_none());
        let earlier: Vec<&Registered> = init
            .chain(HERO)
            .chain(CREATURES)
            .chain(COMBAT)
            .chain(LOOP)
            .chain(FIGURE)
            .filter(|r| find(CAMERA, r.namespace, r.name).is_none())
            .collect();
        for r in earlier.into_iter().chain(CAMERA) {
            assert_eq!(r.kind(), kind_of(r), "{}", r.path());
            let id = MemberId::named(r.namespace, r.name);
            match r.keeper {
                // The VM's own aren't modelled entries, only named.
                Keeper::Vm => {
                    assert!(members.get(id).is_none(), "{}", r.path());
                    assert_eq!(members.name(id), r.path());
                }
                keeper => {
                    let (spec, place) = members.get(id).expect("registered");
                    let want = if keeper == Keeper::Host {
                        Place::Host
                    } else {
                        Place::ValueCell
                    };
                    assert_eq!(place, want, "{}", r.path());
                    assert_eq!(*spec, MemberSpec::new(kind_of(r), r.min, r.max));
                }
            }
        }
    }

    #[test]
    fn an_enums_range_is_its_labels() {
        // The registry's labels for a sprite's blend mode stop at five of
        // its eight values.
        let labelled = |r: &&Registered| {
            r.units.contains('|')
                && r.class == "EnumMeasurement"
                && r.path() != "spriteinfo.blend mode"
        };
        for r in INIT_CHAIN
            .iter()
            .chain(HERO)
            .chain(CREATURES)
            .chain(COMBAT)
            .chain(LOOP)
            .chain(FIGURE)
            .chain(CAMERA)
            .filter(labelled)
        {
            let n = r.units.split('|').count();
            assert_eq!(r.class, "EnumMeasurement", "{}", r.path());
            assert_eq!((r.min, r.max), (0.0, (n - 1) as f32), "{}", r.path());
            let spec = r.spec();
            assert_eq!(
                convert(&spec, Value::Int(n as i32 + 3)),
                Some(Value::Int(n as i32 - 1)),
                "{}",
                r.path()
            );
            assert_eq!(convert(&spec, Value::Int(-1)), Some(Value::Int(0)));
            // A float written to an enum truncates first.
            assert_eq!(convert(&spec, Value::Float(1.9)), Some(Value::Int(1)));
        }
        let platform = find(INIT_CHAIN, "system", "hardware platform").unwrap();
        assert_eq!(label(&platform, "PS 3"), Some(Value::Int(1)));
        let config = find(INIT_CHAIN, "system", "game configuration").unwrap();
        assert_eq!(label(&config, "Golden Master"), Some(Value::Int(3)));
        let language = find(INIT_CHAIN, "system", "game language").unwrap();
        assert_eq!(label(&language, "English"), Some(Value::Int(6)));
        assert_eq!(label(&language, "Klingon"), None);
    }

    #[test]
    fn writes_clamp_to_a_range_with_either_end_set_and_convert_by_class() {
        let buffer = find(INIT_CHAIN, "placement", "deactivation buffer").unwrap();
        assert_eq!(buffer.units, "feet");
        let spec = buffer.spec();
        assert_eq!(convert(&spec, Value::Float(-3.0)), Some(Value::Float(0.0)));
        assert_eq!(convert(&spec, Value::Int(12)), Some(Value::Float(12.0)));
        assert_eq!(convert(&spec, Value::Float(1e12)), Some(Value::Float(OPEN)));
        let priority = find(INIT_CHAIN, "spriteinfo", "priority").unwrap();
        let spec = priority.spec();
        assert_eq!(
            convert(&spec, Value::Float(-20_000.0)),
            Some(Value::Float(-10_000.0))
        );
        let volume = find(INIT_CHAIN, "soundinfo", "priority").unwrap();
        assert_eq!(volume.units, "(0-100)");
        assert_eq!(
            convert(&volume.spec(), Value::Int(250)),
            Some(Value::Int(100))
        );
        // A range of 0 to 0 is open both ways.
        let width = find(INIT_CHAIN, "spriteinfo", "width").unwrap();
        assert_eq!(
            convert(&width.spec(), Value::Float(-5.0)),
            Some(Value::Float(-5.0))
        );
        // A colour takes only a colour, a screen position only a screen
        // position, an object member only an object.
        let colour = find(INIT_CHAIN, "spriteinfo", "color").unwrap();
        assert_eq!(convert(&colour.spec(), Value::Int(3)), None);
        assert_eq!(
            convert(&colour.spec(), Value::Color(7)),
            Some(Value::Color(7))
        );
        let location = find(INIT_CHAIN, "spriteinfo", "location").unwrap();
        assert_eq!(convert(&location.spec(), Value::Float(1.0)), None);
        let light = find(INIT_CHAIN, "lightinfo", "directional light 1").unwrap();
        assert_eq!(convert(&light.spec(), Value::Int(1)), None);
        assert_eq!(
            convert(&light.spec(), Value::Entity(9)),
            Some(Value::Entity(9))
        );
        // The host's engine set is read whole and never written.
        let waypoints = find(INIT_CHAIN, "actorinfo", "waypoints").unwrap();
        assert_eq!(waypoints.kind(), Kind::Set);
        assert_eq!(convert(&waypoints.spec(), Value::Set(0)), None);
        // A value cell reads its class's zero until written.
        let visible = find(INIT_CHAIN, "spriteinfo", "visible").unwrap();
        assert_eq!(visible.spec().default, Value::Int(0));
        assert_eq!(colour.spec().default, Value::Color(0));
        assert_eq!(light.spec().default, Value::Null);
    }

    #[test]
    fn heros_members_convert_by_their_units_and_range() {
        // Feet and degrees per second keep a float, never under 0, open on top.
        let speed = find(HERO, "actorphysics", "ground max speed").unwrap();
        assert_eq!(speed.units, "ft. / sec.");
        assert_eq!(
            convert(&speed.spec(), Value::Int(20)),
            Some(Value::Float(20.0))
        );
        assert_eq!(
            convert(&speed.spec(), Value::Float(-1.0)),
            Some(Value::Float(0.0))
        );
        let turn = find(HERO, "actorphysics", "ground turn rate").unwrap();
        assert_eq!(turn.units, "deg. / sec.");
        assert_eq!(
            convert(&turn.spec(), Value::Float(1e12)),
            Some(Value::Float(OPEN))
        );
        // Gravity's registered range is open both ways.
        let gravity = find(HERO, "actorphysics", "gravity acceleration").unwrap();
        assert_eq!(
            convert(&gravity.spec(), Value::Float(-200.0)),
            Some(Value::Float(-200.0))
        );
        // A percent with a top clamps there.
        let bloom = find(HERO, "actorinfo", "bloom intensity").unwrap();
        assert_eq!(
            convert(&bloom.spec(), Value::Int(250)),
            Some(Value::Int(100))
        );
        // A button's state by its labels, the way the original lists them.
        let state = find(HERO, "buttonmeasurement", "pressure state").unwrap();
        assert_eq!(label(&state, "just pressed"), Some(Value::Int(3)));
        assert_eq!(label(&state, "just released"), Some(Value::Int(0)));
        // An object member takes only an object; a velocity is the host's
        // vector, never a number.
        let velocity = find(HERO, "actorinfo", "velocity").unwrap();
        assert_eq!(convert(&velocity.spec(), Value::Float(3.0)), None);
        let contacts = find(HERO, "collisioninfo", "environment contacts").unwrap();
        assert_eq!(contacts.kind(), Kind::Set);
        assert_eq!(contacts.keeper, Keeper::Host);
        // A clip's animation info is the host's (AH.49): its flags by
        // their labels, a weight in percent held to 0-100; the sounds
        // playing a cell that reads none.
        let layered = find(HERO, "animationinfo", "is layered").unwrap();
        assert_eq!((layered.kind(), layered.keeper), (Kind::Int, Keeper::Host));
        assert_eq!(label(&layered, "true"), Some(Value::Int(1)));
        let kind = find(HERO, "animationinfo", "type").unwrap();
        assert_eq!(kind.keeper, Keeper::Host);
        assert_eq!(label(&kind, "Partial"), Some(Value::Int(1)));
        assert_eq!(label(&kind, "Additive"), Some(Value::Int(2)));
        let blend = find(HERO, "animationinfo", "blend percent").unwrap();
        assert_eq!(
            (blend.class, blend.units, blend.keeper),
            ("FloatMeasurement", "percent", Keeper::Host)
        );
        assert_eq!(
            convert(&blend.spec(), Value::Float(130.0)),
            Some(Value::Float(100.0))
        );
        assert_eq!(
            convert(&blend.spec(), Value::Float(-3.0)),
            Some(Value::Float(0.0))
        );
        assert_eq!(
            convert(&blend.spec(), Value::Int(55)),
            Some(Value::Float(55.0))
        );
        let sounds = find(HERO, "soundinfo", "active sound effects").unwrap();
        assert_eq!(
            (sounds.kind(), sounds.keeper),
            (Kind::Reference, Keeper::Cell)
        );
        assert_eq!(sounds.spec().default, Value::Null);
    }

    /// A slider's own members, the VM's: named as the registry spells them,
    /// enums by their labels, and none a host or a cell answers.
    #[test]
    fn a_sliders_members_are_the_vms_by_their_labels() {
        let mode = find(COMBAT, "sequence", "playback mode").unwrap();
        assert_eq!(mode.keeper, Keeper::Vm);
        assert_eq!(label(&mode, "ping pong"), Some(Value::Int(1)));
        assert_eq!(label(&mode, "one way"), Some(Value::Int(0)));
        let ignores = find(COMBAT, "slider", "ignores time scales").unwrap();
        assert_eq!(ignores.keeper, Keeper::Vm);
        assert_eq!(label(&ignores, "true"), Some(Value::Int(1)));
        let mut members = Members::new();
        register(&mut members, COMBAT);
        for r in COMBAT {
            let id = MemberId::named(r.namespace, r.name);
            assert!(members.get(id).is_none(), "{}", r.path());
            assert_eq!(members.name(id), r.path());
        }
        // The sequence members a slider shares with the hero's sequences
        // stay his table's, the host's for a sequence.
        for name in [
            "time remaining",
            "percent remaining",
            "playback rate",
            "looping",
        ] {
            let r = find(HERO, "sequence", name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(r.keeper, Keeper::Host, "{name}");
        }
    }

    /// AH.20's rows by the registry's own entries (aj-vm's live table): a
    /// coin's fade by its labels; the world's origin the host's, a point
    /// nothing writes; the portal's battery a percent clamped to 0-100; the
    /// online object and the save data references reading nothing until
    /// written.
    #[test]
    fn the_loops_members_convert_by_their_units_and_range() {
        let fade = find(LOOP, "placement", "near/far alpha fade").unwrap();
        assert_eq!((fade.class, fade.keeper), ("EnumMeasurement", Keeper::Cell));
        assert_eq!(label(&fade, "on"), Some(Value::Int(1)));
        assert_eq!(convert(&fade.spec(), Value::Int(5)), Some(Value::Int(1)));
        let zero = find(LOOP, "system", "absolute zero").unwrap();
        assert_eq!(
            (zero.class, zero.kind(), zero.keeper),
            ("PositionMeasurement", Kind::Reference, Keeper::Host)
        );
        assert_eq!(convert(&zero.spec(), Value::Float(1.0)), None);
        let battery = find(LOOP, "redoctaneperipheralscriptobject", "battery remaining").unwrap();
        assert_eq!((battery.units, battery.kind()), ("per cent", Kind::Float));
        assert_eq!(battery.spec().default, Value::Float(0.0));
        assert_eq!(
            convert(&battery.spec(), Value::Int(250)),
            Some(Value::Float(100.0))
        );
        assert_eq!(
            convert(&battery.spec(), Value::Float(-3.0)),
            Some(Value::Float(0.0))
        );
        for (ns, name, class) in [
            ("system", "online", "tfbOnlineScriptObject"),
            ("system", "save data", "ScriptSetReference"),
        ] {
            let r = find(LOOP, ns, name).unwrap();
            assert_eq!(r.class, class, "{}", r.path());
            assert_eq!(
                (r.kind(), r.keeper, r.spec().default),
                (Kind::Reference, Keeper::Cell, Value::Null),
                "{}",
                r.path()
            );
        }
        let mut members = Members::new();
        register(&mut members, LOOP);
        let (_, place) = members
            .get(MemberId::named("system", "absolute zero"))
            .unwrap();
        assert_eq!(place, Place::Host);
    }

    /// AH.23's rows by the registry's own entries (aj-vm's live table): the
    /// portal's regions in bytes, 0 to 448, the host's.
    #[test]
    fn the_figures_members_convert_by_their_units_and_range() {
        for name in ["size of region 1", "size of region 2"] {
            let r = find(FIGURE, "redoctaneperipheralscriptobject", name).unwrap();
            assert_eq!(
                (r.class, r.units, r.kind(), r.keeper),
                ("IntMeasurement", "bytes", Kind::Int, Keeper::Host),
                "{}",
                r.path()
            );
            assert_eq!(convert(&r.spec(), Value::Int(500)), Some(Value::Int(448)));
            assert_eq!(
                convert(&r.spec(), Value::Float(112.9)),
                Some(Value::Int(112))
            );
            assert_eq!(convert(&r.spec(), Value::Int(-4)), Some(Value::Int(0)));
        }
        let mut members = Members::new();
        register(&mut members, FIGURE);
        let (_, place) = members
            .get(MemberId::named(
                "redoctaneperipheralscriptobject",
                "size of region 2",
            ))
            .unwrap();
        assert_eq!(place, Place::Host);
    }

    #[test]
    fn the_fights_members_convert_by_their_units_and_range() {
        // The engine's sets the host keeps read whole, never written.
        for name in ["inactive", "removed"] {
            let set = find(CREATURES, "actorinfo", name).unwrap();
            assert_eq!((set.kind(), set.keeper), (Kind::Set, Keeper::Host));
        }
        let contacts = find(CREATURES, "collisioninfo", "contacts with me").unwrap();
        assert_eq!(contacts.kind(), Kind::Set);
        // A set the whitelist's scripts only name stands as a cell reading
        // nothing, not a set.
        let npcs = find(CREATURES, "actorinfo", "npcs").unwrap();
        assert_eq!((npcs.kind(), npcs.keeper), (Kind::Reference, Keeper::Cell));
        // A hurt's tint: a percent clamped at its top, a colour packed.
        let tint = find(CREATURES, "actorinfo", "replace color percentage").unwrap();
        assert_eq!(tint.units, "percent");
        assert_eq!(
            convert(&tint.spec(), Value::Int(250)),
            Some(Value::Int(100))
        );
        let colour = find(CREATURES, "actorinfo", "replace color").unwrap();
        assert_eq!(colour.kind(), Kind::Color);
        assert_eq!(convert(&colour.spec(), Value::Int(3)), None);
        // The camera's field of view in degrees, 0 to 180.
        let fov = find(CREATURES, "viewportinfo", "field of view").unwrap();
        assert_eq!(
            convert(&fov.spec(), Value::Float(200.0)),
            Some(Value::Float(180.0))
        );
        // A shadow's flag by its labels.
        let shadow = find(CREATURES, "placement", "shadow receive").unwrap();
        assert_eq!(label(&shadow, "true"), Some(Value::Int(1)));
        // What a sequence came from and an ambient clip read nothing until
        // written: references.
        for (ns, name) in [("clonedsequence", "parent"), ("animationinfo", "ambient")] {
            let r = find(CREATURES, ns, name).unwrap();
            assert_eq!((r.kind(), r.spec().default), (Kind::Reference, Value::Null));
        }
    }

    /// AG.28's rows by the registry's own entries (aj-vm's live table): the
    /// heroes a set the host reads whole; the viewport's field of view in
    /// degrees to 180 and far clip in feet open on top, the host's; the
    /// converter's point an object, its screen location a screen position,
    /// its lock by its labels; the container's size in pixels, open.
    #[test]
    fn the_cameras_members_convert_by_their_units_and_range() {
        let players = find(CAMERA, "actorinfo", "players").unwrap();
        assert_eq!((players.kind(), players.keeper), (Kind::Set, Keeper::Host));
        let fov = find(CAMERA, "viewportinfo", "field of view").unwrap();
        assert_eq!((fov.units, fov.keeper), ("degrees", Keeper::Host));
        assert_eq!(
            convert(&fov.spec(), Value::Float(200.0)),
            Some(Value::Float(180.0))
        );
        let far = find(CAMERA, "viewportinfo", "far clip").unwrap();
        assert_eq!(far.units, "feet");
        assert_eq!(
            convert(&far.spec(), Value::Int(1000)),
            Some(Value::Float(1000.0))
        );
        assert_eq!(
            convert(&far.spec(), Value::Float(-1.0)),
            Some(Value::Float(0.0))
        );
        let world = find(CAMERA, "viewconverter", "world position").unwrap();
        assert_eq!(world.kind(), Kind::Reference);
        assert_eq!(convert(&world.spec(), Value::Float(1.0)), None);
        let screen = find(CAMERA, "viewconverter", "screen location").unwrap();
        assert_eq!(screen.kind(), Kind::Screen);
        assert_eq!(convert(&screen.spec(), Value::Float(1.0)), None);
        let lock = find(CAMERA, "viewconverter", "lock type").unwrap();
        assert_eq!(lock.keeper, Keeper::Cell);
        assert_eq!(label(&lock, "lock world position"), Some(Value::Int(0)));
        for name in ["screen width", "screen height"] {
            let r = find(CAMERA, "spriteinfo", name).unwrap();
            assert_eq!(
                (r.units, r.kind(), r.keeper),
                ("pixels", Kind::Float, Keeper::Host),
                "{name}"
            );
            assert_eq!(
                convert(&r.spec(), Value::Float(-5.0)),
                Some(Value::Float(-5.0))
            );
        }
        // The slide's sound's falloff: a cell, feet from 0 up.
        let silent = find(CAMERA, "soundinfo", "attenuation distance silent").unwrap();
        assert_eq!((silent.units, silent.keeper), ("feet", Keeper::Cell));
        assert_eq!(
            convert(&silent.spec(), Value::Int(-3)),
            Some(Value::Float(0.0))
        );
        // Registered after the tables whose cells it takes over, the
        // camera's host answers.
        let mut members = Members::new();
        register(&mut members, INIT_CHAIN);
        register(&mut members, CREATURES);
        register(&mut members, CAMERA);
        let (_, place) = members
            .get(MemberId::named("soundinfo", "attenuation distance silent"))
            .unwrap();
        assert_eq!(place, Place::ValueCell);
        for (ns, name) in [
            ("viewportinfo", "far clip"),
            ("viewportinfo", "field of view"),
            ("viewportinfo", "view converter"),
            ("viewportinfo", "container sprite"),
        ] {
            let (_, place) = members.get(MemberId::named(ns, name)).unwrap();
            assert_eq!(place, Place::Host, "{ns}.{name}");
        }
    }
}
