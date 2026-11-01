//! The loader on guac-formats' synthetic interface archive (AT.9): each
//! template under its object, its string the `StringInfo` at its place in
//! address order, its local font, its `_ID`; what doesn't resolve listed.

use guac_formats::igz::{Igz, Location};
use guac_formats::interface::builder::sample;
use guac_ir::interface::{Interface, Str as IrStr};
use guac_ir::{DumpIdentity, Sha256};
use guac_script::{Obj, Value};

use super::*;

const UNIT: u16 = 3;

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([0; 32]),
    }
}

fn object(igz: &Igz<'_>, at: Location) -> u32 {
    igz.objects().iter().position(|o| o.at == at).unwrap() as u32
}

/// The sample's record as the extract reads it, its two strings given
/// English text by hand (the sample has no language pack).
fn record(igz: &Igz<'_>, strings: usize) -> Interface {
    let mut ir = guac_extract::interface::read_igz("character/x.bld", igz, None, None, identity())
        .unwrap()
        .ir;
    ir.strings = (0..strings)
        .map(|n| IrStr {
            text: format!("string {n}"),
            flags: 0x800 * n as i32,
            integer_digits: 0,
            decimal_digits: n as i32,
        })
        .collect();
    ir
}

#[test]
fn each_template_loads_under_its_object_with_its_string_and_font() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let ir = record(&igz, 2);
    let loaded = load_igz(&[Archive {
        unit: UNIT,
        igz: &igz,
        interface: &ir,
    }]);
    let m = loaded.keeper.model();
    // The eight and the viewport's container.
    assert_eq!(m.len(), 9);
    for (n, &at) in s.templates.iter().enumerate() {
        let obj = Obj {
            unit: UNIT,
            index: object(&igz, at),
        };
        let id = m
            .sprite_of(Value::Object(obj))
            .expect("found by its object");
        assert_eq!(m.sprite(id).name, ir.templates[n].name);
    }
    let text = m
        .sprite_of(Value::Object(Obj {
            unit: UNIT,
            index: object(&igz, s.templates[4]),
        }))
        .unwrap();
    let key = Obj {
        unit: UNIT,
        index: object(&igz, s.strings[1]),
    };
    assert_eq!(
        m.sprite(text).text,
        Value::Object(key),
        "its key, the second"
    );
    let st = m.string(Value::Object(key)).unwrap();
    assert_eq!(
        (
            String::from_utf16_lossy(&st.units),
            st.flags,
            st.decimal_digits
        ),
        ("string 1".to_owned(), 0x800, 1)
    );
    assert_eq!(m.sprite(text).font, Some(0), "the archive's own font");
    // The imported template names global.bld's string and font, which
    // aren't loaded: each listed, the template kept without.
    assert_eq!(
        loaded.refused,
        [
            (
                "character/x.bld".to_owned(),
                "UI_Text_Imported's imported string 0x40".to_owned()
            ),
            (
                "character/x.bld".to_owned(),
                "UI_Text_Imported's imported font 0x33".to_owned()
            )
        ]
    );
}

#[test]
fn strings_that_dont_pair_with_the_files_keys_load_none() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let ir = record(&igz, 1);
    let loaded = load_igz(&[Archive {
        unit: UNIT,
        igz: &igz,
        interface: &ir,
    }]);
    let key = Obj {
        unit: UNIT,
        index: object(&igz, s.strings[0]),
    };
    assert!(loaded.keeper.model().string(Value::Object(key)).is_none());
    assert!(
        loaded
            .refused
            .iter()
            .any(|(_, why)| why == "1 strings for 2 StringInfos: none kept"),
        "{:?}",
        loaded.refused
    );
}

/// A template is a placement: its `_ID` read off its object, which a clone
/// copies (`spritematrixmeasurement.id`).
#[test]
fn a_templates_id_is_its_placements() {
    use guac_formats::interface::builder::{at, template};
    let mut b = guac_formats::igz::builder::IgzBuilder::new();
    let o = b.object(0, "tfbSpriteInfo", 0xE4);
    // A container: visual type 3, no context.
    template(&mut b, o, "Generic_Container", 3, 1);
    b.put_u32(at(o, 0x20), 77);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let ir = record(&igz, 0);
    let loaded = load_igz(&[Archive {
        unit: UNIT,
        igz: &igz,
        interface: &ir,
    }]);
    let m = loaded.keeper.model();
    let id = m
        .sprite_of(Value::Object(Obj {
            unit: UNIT,
            index: object(&igz, o),
        }))
        .unwrap();
    assert_eq!(m.sprite(id).id, 77);
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
}
