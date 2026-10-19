use super::*;
use crate::roster::builder::{
    AltSpec, Disc, DiscSpec, ElementSpec, NameSpec, ToySpec, disc, sample,
};

/// The sample disc, parsed: (init_setup, global, exports) borrow `d`.
fn read(d: &Disc) -> Result<(ToyTable, Vec<String>), FormatError> {
    let init = Igz::parse(&d.init_setup).unwrap();
    let global = Igz::parse(&d.global).unwrap();
    let boot = Igz::parse(&d.bootstrap).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    let table = ToyTable::read(&init, &global, &exports)?;
    // Key names by object, for asserting on.
    let names = (0..global.objects().len())
        .map(|i| {
            StringInfo::read(&global, i)
                .ok()
                .and_then(|s| s.name)
                .unwrap_or("")
                .to_owned()
        })
        .collect();
    Ok((table, names))
}

#[test]
fn each_macros_calls_become_rows() {
    let (table, key) = read(&disc(&sample())).unwrap();
    assert_eq!(table.toys.len(), 14);
    assert_eq!(table.alts.len(), 5);
    assert_eq!(table.alt_characters.len(), 1);

    // Literal flags and a Minion_Names name with its index.
    let whirlwind = &table.toys[1];
    assert_eq!(whirlwind.item_code, 0);
    assert_eq!(key[whirlwind.level_load], "MinionLoad_000_AirDragon");
    assert_eq!(key[whirlwind.name], "000_Toy_Name");
    assert!(whirlwind.sky1 && whirlwind.known_repose && !whirlwind.magic_item && !whirlwind.giant);
    // A literal inside a ValueRHSVariant.
    assert_eq!(whirlwind.element, 2);
    assert_eq!(key[table.toys[2].name], "007_Toy_Name");
    // An omitted element reads 0.
    assert_eq!(table.toys[3].element, 0);
    // A ValueInfo element, a direct name.
    let jet_vac = &table.toys[5];
    assert_eq!((jet_vac.item_code, jet_vac.element), (100, 2));
    assert_eq!(key[jet_vac.name], "100_Toy_Name");
    assert_eq!(table.toys[6].element, 1);
    // DEBUG is a call like any other here; the roster drops it.
    assert_eq!(table.toys[0].item_code, DEBUG_TOY);

    // The flags' sets, and the after-return block's.
    assert_eq!(
        whirlwind.sets(),
        [sets::CHARACTER, sets::SKY1_CHARACTER, sets::REPOSE]
    );
    assert_eq!(table.toys[7].sets(), [sets::CHARACTER, sets::GIANT]);
    let gill_runt = &table.toys[12];
    assert_eq!(gill_runt.added_to, [sets::PET]);
    assert_eq!(gill_runt.sets(), [sets::MAGIC_ITEM, sets::PET]);
    // A magic item flagged sky1 stays out of the sky1 characters.
    assert_eq!(table.toys[11].sets(), [sets::MAGIC_ITEM]);
}

#[test]
fn an_alt_attaches_to_the_toy_defined_before_it() {
    let (table, key) = read(&disc(&sample())).unwrap();
    let owners: Vec<(i32, i32, Option<&str>)> = table
        .alts
        .iter()
        .map(|a| {
            (
                table.toys[a.toy].item_code,
                a.deco,
                a.display.map(|k| key[k].as_str()),
            )
        })
        .collect();
    assert_eq!(
        owners,
        [
            (0, 2, Some("ToyType_Polar")),
            (100, 3, None),
            (106, 3, None),
            (112, 2, Some("ToyType_Gnarly")),
            (208, 2, Some("ToyType_CannonSpecial")),
        ]
    );
    assert_eq!(
        key[table.alts[0].level_load],
        "MinionLoad_000_AirDragonLegendary"
    );
}

#[test]
fn alt_characters_link_toys_by_load_and_lightcore_comes_from_the_loop() {
    let (table, key) = read(&disc(&sample())).unwrap();
    let a = &table.alt_characters[0];
    assert_eq!(
        (table.toys[a.normal].item_code, table.toys[a.alt].item_code),
        (16, 28)
    );
    assert_eq!(key[a.prefix], "ToyType_Dark");
    let loads: Vec<&str> = table
        .lightcore_loads
        .iter()
        .map(|&k| key[k].as_str())
        .collect();
    assert_eq!(
        loads,
        [
            "MinionLoad_007_GemGolem",
            "MinionLoad_106_IceSorc",
            "MinionLoad_100_Eagle"
        ]
    );
    let lightcore: Vec<i32> = table
        .toys
        .iter()
        .filter(|t| table.is_lightcore(t))
        .map(|t| t.item_code)
        .collect();
    assert_eq!(lightcore, [7, 100, 106]);
}

fn one_toy(t: ToySpec) -> DiscSpec {
    let mut spec = sample();
    spec.toys.push(t);
    spec
}

#[test]
fn an_unknown_argument_shape_is_refused() {
    // A name the table doesn't model.
    let mut spec = sample();
    spec.toys[1].name = NameSpec::Minion(3);
    assert!(matches!(
        read(&disc(&spec)).unwrap_err().kind,
        ErrorKind::OutOfRange { value: 3, limit: 3 }
    ));
    // A name that's a ValueInfo, not text.
    let mut spec = one_toy(ToySpec::new(
        "MinionLoad_050_X",
        NameSpec::Key("Name_As_Value"),
        50,
    ));
    spec.values.push(("Name_As_Value", 3));
    assert_eq!(read(&disc(&spec)).unwrap_err().field, "name");
    // An element that's text.
    let mut spec = sample();
    spec.toys[1].element = ElementSpec::Value("ToyType_Polar");
    assert_eq!(read(&disc(&spec)).unwrap_err().field, "element");
}

/// A `ValueRHSVariant` chain is unwrapped to a bounded depth, so one that
/// points back at itself is refused rather than recursed into forever.
#[test]
fn a_value_wrapped_in_itself_or_too_deep_is_refused() {
    let mut spec = sample();
    spec.toys[1].element = ElementSpec::WrappedIn(2, MAX_WRAPPING);
    assert_eq!(read(&disc(&spec)).unwrap().0.toys[1].element, 2);
    for element in [
        ElementSpec::WrappedIn(2, MAX_WRAPPING + 1),
        ElementSpec::SelfWrapped,
    ] {
        spec.toys[1].element = element;
        let e = read(&disc(&spec)).unwrap_err();
        assert_eq!(e.field, "_varOp1", "{element:?}");
        assert!(
            matches!(e.kind, ErrorKind::OutOfRange { limit, .. } if limit == MAX_WRAPPING as u64),
            "{element:?}: {e}"
        );
    }
}

#[test]
fn every_alt_in_a_toys_block_is_that_toys() {
    let mut spec = sample();
    let alt = spec.toys[1].alts[0].clone();
    spec.toys.clear();
    spec.alt_characters.clear();
    spec.lightcore = vec!["MinionLoad_007_GemGolem"];
    spec.toys.push(ToySpec {
        alts: vec![alt.clone(), AltSpec { deco: 9, ..alt }],
        ..ToySpec::new("MinionLoad_007_GemGolem", NameSpec::Minion(1), 7)
    });
    let (table, _) = read(&disc(&spec)).unwrap();
    assert_eq!(
        table
            .alts
            .iter()
            .map(|a| (a.toy, a.deco))
            .collect::<Vec<_>>(),
        [(0, 2), (0, 9)]
    );
}

#[test]
fn a_table_without_its_script_or_lightcore_loop_is_refused() {
    let d = disc(&sample());
    // The level of another archive: no Init_Setup_Minions.ai.
    let global = Igz::parse(&d.global).unwrap();
    let boot = Igz::parse(&d.bootstrap).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    let e = ToyTable::read(&global, &global, &exports).unwrap_err();
    assert_eq!((e.field, e.kind), ("tfbScriptInfo", ErrorKind::Missing));

    let mut spec = sample();
    spec.lightcore.clear();
    assert_eq!(read(&disc(&spec)).unwrap_err().kind, ErrorKind::Missing);
}

#[test]
fn an_alt_character_naming_an_undefined_toy_is_refused() {
    let mut spec = sample();
    spec.alt_characters[0].alt = "MinionLoad_000_AirDragonLegendary";
    let e = read(&disc(&spec)).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        (DEFINE_ALT_CHARACTER, ErrorKind::Missing)
    );
}
