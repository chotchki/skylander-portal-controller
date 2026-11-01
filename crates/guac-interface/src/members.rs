//! The 64 interface members as the original registers them (aj-vm's live
//! member table) with the handlers the crate docs read them at. AT.5 answers
//! each one; this is the list it answers.

use guac_script::{Kind, MemberSpec};

/// One registered member and its handlers' addresses in the EBOOT.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Member {
    /// The class namespace less any `tfb`, lowercased, as `guac-script`'s
    /// registry spells it (`spriteinfo`).
    pub namespace: &'static str,
    /// As registered, case kept (`Take a Picture`).
    pub name: &'static str,
    /// The registered measurement class.
    pub class: &'static str,
    pub get: Option<u32>,
    pub set: Option<u32>,
    pub reset: Option<u32>,
    /// The registered range; a write clamps to it only when either end is
    /// nonzero (aj-vm's rule, `guac_script::convert`).
    pub min: f32,
    pub max: f32,
}

impl Member {
    /// How a write converts, from the registered class, as `guac-script`'s
    /// registry has it; a sprite's `numeric value` takes any number (its
    /// handler decides integer or float), a set none.
    pub fn kind(&self) -> Kind {
        match self.class {
            "FloatMeasurement" | "PairedFloatMeasurement" => Kind::Float,
            "IntMeasurement" | "EnumMeasurement" => Kind::Int,
            "ColorMeasurement" => Kind::Color,
            "ScreenMeasurement" => Kind::Screen,
            "SpriteValueMeasurement" => Kind::Any,
            "SetVariant" => Kind::Set,
            _ => Kind::Reference,
        }
    }

    pub fn spec(&self) -> MemberSpec {
        MemberSpec::new(self.kind(), self.min, self.max)
    }

    /// `namespace.name` as `guac-script` spells it: lowercased, a leading
    /// `-` dropped.
    pub fn path(&self) -> String {
        format!(
            "{}.{}",
            self.namespace,
            self.name.trim_start_matches('-').to_lowercase()
        )
    }
}

/// The member `guac-script` names `namespace.name` (its spelling: lowercased,
/// no leading `-`), if it's an interface member.
pub fn find(namespace: &str, name: &str) -> Option<&'static Member> {
    MEMBERS.iter().find(|m| {
        m.namespace == namespace && m.name.trim_start_matches('-').eq_ignore_ascii_case(name)
    })
}

#[allow(clippy::too_many_arguments)]
const fn m(
    namespace: &'static str,
    name: &'static str,
    class: &'static str,
    get: u32,
    set: u32,
    reset: u32,
    min: f32,
    max: f32,
) -> Member {
    const fn some(a: u32) -> Option<u32> {
        if a == 0 { None } else { Some(a) }
    }
    Member {
        namespace,
        name,
        class,
        get: some(get),
        set: some(set),
        reset: some(reset),
        min,
        max,
    }
}

/// Every interface member, namespace by namespace in the docs' order. A
/// reference member's get is the member's own getter, which the shared
/// reference getter (0x5E524) calls.
pub const MEMBERS: [Member; 64] = [
    m(
        "spriteinfo",
        "type",
        "EnumMeasurement",
        0xB32DC,
        0,
        0,
        0.0,
        4.0,
    ),
    m(
        "spriteinfo",
        "visible",
        "EnumMeasurement",
        0xB3BCC,
        0xB3C7C,
        0,
        0.0,
        2.0,
    ),
    m(
        "spriteinfo",
        "color",
        "ColorMeasurement",
        0xB32F4,
        0xB3308,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "scale",
        "ScaleMeasurement",
        0xB3430,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "location",
        "ScreenMeasurement",
        0xB3444,
        0xB3458,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "screen location",
        "ScreenMeasurement",
        0xB3480,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "screen width",
        "FloatMeasurement",
        0xB36C4,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "screen height",
        "FloatMeasurement",
        0xB3734,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "text",
        "ReferenceVariant",
        0xB36B0,
        0xB07F0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "priority",
        "FloatMeasurement",
        0xB3690,
        0xB36A4,
        0,
        -10000.0,
        10000.0,
    ),
    m(
        "spriteinfo",
        "width",
        "FloatMeasurement",
        0xB37A0,
        0xB3814,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "height",
        "FloatMeasurement",
        0xB3854,
        0xB38CC,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "depth",
        "FloatMeasurement",
        0xB390C,
        0xB3984,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "natural size",
        "ScreenMeasurement",
        0xB39C8,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "origin",
        "SpriteMatrixMeasurement",
        0xB3B34,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "blend mode",
        "EnumMeasurement",
        0xB3B48,
        0xB3B60,
        0,
        0.0,
        7.0,
    ),
    m(
        "spriteinfo",
        "alignment",
        "EnumMeasurement",
        0xB3B74,
        0xB3B8C,
        0,
        0.0,
        8.0,
    ),
    m(
        "spriteinfo",
        "align to parent",
        "EnumMeasurement",
        0xB3BA0,
        0xB3BB8,
        0,
        0.0,
        8.0,
    ),
    m(
        "spriteinfo",
        "texture flip",
        "EnumMeasurement",
        0xB3CD0,
        0xB3CEC,
        0,
        0.0,
        3.0,
    ),
    m(
        "spriteinfo",
        "clipping",
        "EnumMeasurement",
        0xB3D30,
        0xB3D48,
        0,
        0.0,
        1.0,
    ),
    m(
        "spriteinfo",
        "Take a Picture",
        "EnumMeasurement",
        0xB3D80,
        0xB3DA4,
        0,
        0.0,
        1.0,
    ),
    m(
        "spriteinfo",
        "numeric value",
        "SpriteValueMeasurement",
        0xB3E40,
        0xB3FB8,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "justify",
        "EnumMeasurement",
        0xB3DE4,
        0xB3DFC,
        0,
        0.0,
        2.0,
    ),
    m(
        "spriteinfo",
        "point size",
        "IntMeasurement",
        0xB4750,
        0xB4768,
        0,
        0.0,
        2147483647.0,
    ),
    m(
        "spriteinfo",
        "playing animations",
        "SetVariant",
        0x6AA68,
        0,
        0x7BB04,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "model attach nodes",
        "SetVariant",
        0xB47C4,
        0,
        0x7BB04,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "generators",
        "SetVariant",
        0xB3EB4,
        0,
        0x7BB04,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "Z Depth",
        "FloatMeasurement",
        0xB3670,
        0xB3684,
        0,
        -10000.0,
        10000.0,
    ),
    m(
        "spriteinfo",
        "replace color",
        "ColorMeasurement",
        0xB3358,
        0xB336C,
        0,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "replace color percentage",
        "IntMeasurement",
        0xB33AC,
        0xB33F8,
        0,
        0.0,
        100.0,
    ),
    m(
        "spriteinfo",
        "bloom intensity",
        "IntMeasurement",
        0xB40DC,
        0xB4100,
        0,
        0.0,
        100.0,
    ),
    m(
        "spriteinfo",
        "active",
        "SetVariant",
        0xB32A4,
        0,
        0x7BB04,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "removed",
        "SetVariant",
        0xB32C0,
        0,
        0x7BB04,
        0.0,
        0.0,
    ),
    m(
        "spriteinfo",
        "Text Color Converter",
        "TextColorConverter",
        0x3A8DF0,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spritematrixmeasurement",
        "-orientation",
        "OrientationMeasurement",
        0xB01D4,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spritematrixmeasurement",
        "attach node",
        "ReferenceVariant",
        0x6ABE8,
        0x6A198,
        0,
        0.0,
        0.0,
    ),
    m(
        "spritematrixmeasurement",
        "attached objects",
        "SetVariant",
        0x6AC60,
        0,
        0x7BB04,
        0.0,
        0.0,
    ),
    m(
        "spritematrixmeasurement",
        "ID",
        "IntMeasurement",
        0x6A810,
        0x6A824,
        0x6A830,
        0.0,
        0.0,
    ),
    m(
        "spritematrixmeasurement",
        "origin",
        "MatrixMeasurement",
        0x6A914,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spritematrixmeasurement",
        "clones",
        "SetVariant",
        0x6A928,
        0,
        0x7BB04,
        0.0,
        0.0,
    ),
    m(
        "spritematrixmeasurement",
        "parent",
        "ReferenceVariant",
        0x6AB0C,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spritematrixmeasurement",
        "activator",
        "ReferenceVariant",
        0x6AB50,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spritepositionmeasurement",
        "screen distance",
        "PairedFloatMeasurement",
        0xAE8D8,
        0xAE9D8,
        0,
        0.0,
        2147483647.0,
    ),
    m(
        "spritepositionmeasurement",
        "heading",
        "PairedFloatMeasurement",
        0xAECE8,
        0xAEED0,
        0,
        0.0,
        0.0,
    ),
    m(
        "spritepositionmeasurement",
        "X offset",
        "PairedFloatMeasurement",
        0xAF45C,
        0xAF618,
        0,
        0.0,
        0.0,
    ),
    m(
        "spritepositionmeasurement",
        "Y offset",
        "PairedFloatMeasurement",
        0xAF15C,
        0xAF318,
        0,
        0.0,
        0.0,
    ),
    m(
        "spritepositionmeasurement",
        "priority offset",
        "PairedFloatMeasurement",
        0xAF754,
        0xAF820,
        0,
        0.0,
        0.0,
    ),
    m(
        "stringconverter",
        "value set",
        "ScriptSetReference",
        0x9E650,
        0x9E770,
        0x80E0C,
        0.0,
        0.0,
    ),
    m(
        "stringconverter",
        "text",
        "ReferenceVariant",
        0x9EB3C,
        0x9EB54,
        0,
        0.0,
        0.0,
    ),
    m(
        "stringconverter",
        "'@' representation",
        "EnumMeasurement",
        0x9EC04,
        0x9EC28,
        0,
        0.0,
        1.0,
    ),
    m(
        "stringconverter",
        "integer digits",
        "IntMeasurement",
        0x9EC44,
        0x9EC5C,
        0,
        0.0,
        2147483647.0,
    ),
    m(
        "stringconverter",
        "decimal digits",
        "IntMeasurement",
        0x9EC6C,
        0x9EC84,
        0,
        0.0,
        2147483647.0,
    ),
    m(
        "stringconverter",
        "use comma",
        "EnumMeasurement",
        0x9EC94,
        0x9ECB8,
        0,
        0.0,
        1.0,
    ),
    m(
        "stringinfo",
        "String Converter",
        "StringConverter",
        0x3A8DF0,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "textcolorconverter",
        "color set",
        "ScriptSetReference",
        0xB066C,
        0xB0698,
        0x80E0C,
        0.0,
        0.0,
    ),
    m(
        "textcolorconverter",
        "text sprite",
        "ReferenceVariant",
        0xB0724,
        0xB073C,
        0,
        0.0,
        0.0,
    ),
    m(
        "nameextractor",
        "Object to name",
        "ReferenceVariant",
        0x9DBE8,
        0x9DC04,
        0,
        0.0,
        0.0,
    ),
    m(
        "nameextractor",
        "Name checksum",
        "IntMeasurement",
        0x9DC50,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "nameextractor",
        "Instantiation size",
        "IntMeasurement",
        0x9DCD0,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "nameextractor",
        "type count",
        "IntMeasurement",
        0x9DD40,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "viewporttitlesafeinfo",
        "left margin",
        "IntMeasurement",
        0x1DE74,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "viewporttitlesafeinfo",
        "right margin",
        "IntMeasurement",
        0x1DE88,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "viewporttitlesafeinfo",
        "upper margin",
        "IntMeasurement",
        0x1DE9C,
        0,
        0,
        0.0,
        0.0,
    ),
    m(
        "viewporttitlesafeinfo",
        "lower margin",
        "IntMeasurement",
        0x1DEB0,
        0,
        0,
        0.0,
        0.0,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The crate docs: what each member's section says.
    const DOCS: &str = include_str!("lib.rs");

    /// The docs' section for `namespace`: from the `## ` heading naming it to
    /// the next heading.
    fn section(namespace: &str) -> &'static str {
        let tick = format!("`{namespace}`");
        let start = DOCS
            .lines()
            .position(|l| l.starts_with("//! ## ") && l.contains(&tick))
            .unwrap_or_else(|| panic!("no section names {namespace}"));
        let lines: Vec<&str> = DOCS.lines().collect();
        let end = lines[start + 1..]
            .iter()
            .position(|l| l.starts_with("//! #"))
            .map_or(lines.len(), |n| start + 1 + n);
        let from = lines[..start].iter().map(|l| l.len() + 1).sum::<usize>();
        let to = lines[..end].iter().map(|l| l.len() + 1).sum::<usize>();
        &DOCS[from..to]
    }

    #[test]
    fn every_member_is_in_its_namespaces_section_with_its_handlers() {
        let mut counts = std::collections::BTreeMap::new();
        for m in &MEMBERS {
            *counts.entry(m.namespace).or_insert(0) += 1;
            let s = section(m.namespace);
            // Its name, ticked, its own or in a pair (`a`, `b` (...)).
            assert!(
                s.contains(&format!("`{}`", m.name)),
                "{}.{}",
                m.namespace,
                m.name
            );
            for a in [m.get, m.set, m.reset].into_iter().flatten() {
                assert!(
                    s.contains(&format!("0x{a:X}")),
                    "{}.{}'s 0x{a:X} not in its section",
                    m.namespace,
                    m.name
                );
            }
        }
        // The counts each heading gives.
        for (namespace, n) in counts {
            let s = section(namespace);
            let heading = s.lines().next().unwrap();
            assert!(
                heading.contains(&format!("`{namespace}` ({n})")),
                "{heading} for {n}"
            );
        }
    }

    #[test]
    fn the_list_is_the_live_tables_with_no_member_twice() {
        let mut seen = std::collections::BTreeSet::new();
        for m in &MEMBERS {
            assert!(seen.insert((m.namespace, m.name)), "{} twice", m.name);
            assert!(m.get.is_some(), "{} has a getter", m.name);
        }
        let read_only = MEMBERS.iter().filter(|m| m.set.is_none()).count();
        assert_eq!(read_only, 27);
    }
}
