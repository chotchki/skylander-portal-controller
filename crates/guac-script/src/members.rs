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
            ("FloatMeasurement", _) => Kind::Float,
            ("IntMeasurement" | "EnumMeasurement", _) => Kind::Int,
            ("ColorMeasurement", _) => Kind::Color,
            ("ScreenMeasurement", _) => Kind::Screen,
            ("SetVariant", Keeper::Host) => Kind::Set,
            _ => Kind::Reference,
        }
    }

    #[test]
    fn every_member_is_named_once_as_the_registry_spells_it() {
        let mut seen = HashSet::new();
        for r in INIT_CHAIN {
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

    #[test]
    fn each_member_registers_with_its_class_range_and_keeper() {
        let mut members = Members::new();
        register(&mut members, INIT_CHAIN);
        for r in INIT_CHAIN {
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
        for r in INIT_CHAIN.iter().filter(|r| r.units.contains('|')) {
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
}
