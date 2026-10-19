use super::builder::{Disc, disc, sample};
use super::*;
use crate::exports::Exports;
use crate::language::{ENGLISH, LanguagePack, Texts};

/// The sample's figures, with their English names and sprite names.
#[derive(Debug)]
struct Read {
    figures: Vec<Figure>,
    /// `<word> <name>` or `<name>`, as the IR composes it.
    names: Vec<String>,
    portraits: Vec<Option<String>>,
}

fn read(d: &Disc) -> Result<Read, FormatError> {
    let init = Igz::parse(&d.init_setup).unwrap();
    let global = Igz::parse(&d.global).unwrap();
    let boot = Igz::parse(&d.bootstrap).unwrap();
    let collection = Igz::parse(&d.collection).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    let packs = d
        .packs
        .iter()
        .map(|p| LanguagePack::read(&Igz::parse(p).unwrap()).unwrap())
        .collect();
    let texts = Texts::new(&global, packs).unwrap();
    let table = ToyTable::read(&init, &global, &exports)?;
    let figures = figures(&table, &global, &collection)?;
    let text = |k| texts.text(k, ENGLISH).unwrap().to_owned();
    let names = figures
        .iter()
        .map(|f| match f.word {
            Some(w) => format!("{} {}", text(w), text(f.name)),
            None => text(f.name),
        })
        .collect();
    let portraits = figures
        .iter()
        .map(|f| {
            f.portrait.map(|p| {
                SpriteInfo::read(&collection, p)
                    .unwrap()
                    .placement
                    .name
                    .unwrap()
                    .to_owned()
            })
        })
        .collect();
    Ok(Read {
        figures,
        names,
        portraits,
    })
}

#[test]
fn the_sample_composes_every_row() {
    let r = read(&disc(&sample())).unwrap();
    let rows: Vec<(String, &str, Kind, Category, Option<u8>)> = r
        .figures
        .iter()
        .zip(&r.names)
        .map(|(f, n)| (f.id(), n.as_str(), f.kind, f.category, f.element))
        .collect();
    use Category::*;
    use Kind::*;
    assert_eq!(
        rows,
        [
            ("000000-0000".into(), "Whirlwind", Base, Figure, Some(2)),
            (
                "000000-0800".into(),
                "Series 2 Whirlwind",
                Series2,
                Figure,
                Some(2)
            ),
            (
                "000000-0402".into(),
                "Polar Whirlwind",
                Alt,
                Figure,
                Some(2)
            ),
            ("000007-0000".into(), "Prism Break", Base, Figure, Some(6)),
            (
                "000007-0800".into(),
                "Series 2 Prism Break",
                Series2,
                Figure,
                Some(6)
            ),
            (
                "000007-0200".into(),
                "LightCore Prism Break",
                LightCore,
                Figure,
                Some(6)
            ),
            ("000010-0000".into(), "Spyro", Base, Figure, Some(0)),
            (
                "000010-0800".into(),
                "Series 2 Spyro",
                Series2,
                Figure,
                Some(0)
            ),
            ("00001c-0000".into(), "Dark Spyro", Base, Figure, Some(0)),
            ("000064-0000".into(), "Jet-Vac", Base, Figure, Some(2)),
            (
                "000064-0200".into(),
                "LightCore Jet-Vac",
                LightCore,
                Figure,
                Some(2)
            ),
            (
                "000064-0403".into(),
                "Legendary Jet-Vac",
                Alt,
                Figure,
                Some(2)
            ),
            ("00006a-0000".into(), "Chill", Base, Figure, Some(1)),
            (
                "00006a-0200".into(),
                "LightCore Chill",
                LightCore,
                Figure,
                Some(1)
            ),
            (
                "00006a-0603".into(),
                "Legendary Chill",
                Alt,
                Figure,
                Some(1)
            ),
            ("000070-0000".into(), "Tree Rex", Base, Giant, Some(7)),
            ("000070-0402".into(), "Gnarly Tree Rex", Alt, Giant, Some(7)),
            ("0000c8-0000".into(), "Anvil Rain", Base, Item, None),
            ("0000d0-0000".into(), "Cannon", Base, Item, None),
            (
                "0000d0-0402".into(),
                "Golden Dragonfire Cannon",
                Alt,
                Item,
                None
            ),
            (
                "00012c-0000".into(),
                "Dragon's Peak",
                Base,
                AdventurePack,
                None
            ),
            (
                "000130-0000".into(),
                "Volcanic Vault",
                Base,
                AdventurePack,
                Some(5)
            ),
            ("000202-0000".into(), "Gill Runt", Base, Sidekick, Some(1)),
            ("00021e-0000".into(), "Mini-Jini", Base, Sidekick, Some(0)),
        ]
    );
}

#[test]
fn debug_is_dropped() {
    let r = read(&disc(&sample())).unwrap();
    assert!(r.figures.iter().all(|f| f.toy_type != DEBUG_TOY as u32));
    assert!(!r.names.iter().any(|n| n == "DEBUG"));
}

#[test]
fn a_lightcore_set_toy_gets_0x200_and_one_outside_it_doesnt() {
    let r = read(&disc(&sample())).unwrap();
    let lightcore = |toy: u32| {
        r.figures
            .iter()
            .any(|f| f.toy_type == toy && f.variant == bits::LIGHTCORE)
    };
    assert!(lightcore(7) && lightcore(100) && lightcore(106));
    // Spyro is Series 2 but not in the loop's loads.
    assert!(!lightcore(16) && !lightcore(0));
}

#[test]
fn a_null_display_string_reads_legendary_and_a_legendary_lightcore_portrait_sets_bit_9() {
    let r = read(&disc(&sample())).unwrap();
    let by_id = |id: &str| r.figures.iter().position(|f| f.id() == id).unwrap();
    assert_eq!(r.names[by_id("000064-0403")], "Legendary Jet-Vac");
    let chill = by_id("00006a-0603");
    assert_eq!(r.names[chill], "Legendary Chill");
    assert_eq!(r.figures[chill].deco, Some(3));
    assert_eq!(
        r.portraits[chill].as_deref(),
        Some("Collection_MinionIcon_LegLight_106_IceSorc")
    );
}

#[test]
fn the_element_rule() {
    let r = read(&disc(&sample())).unwrap();
    let element = |id: &str| r.figures.iter().find(|f| f.id() == id).unwrap().element;
    // 0 on an item or adventure pack is none.
    assert_eq!(element("0000c8-0000"), None);
    assert_eq!(element("00012c-0000"), None);
    // A pack's own element stays; 0 is Magic on anything else.
    assert_eq!(element("000130-0000"), Some(5));
    assert_eq!(element("00021e-0000"), Some(0));
    assert_eq!(element("000010-0000"), Some(0));
    // An element past Life is refused.
    let mut spec = sample();
    spec.toys[2].element = super::builder::ElementSpec::Literal(8);
    let e = read(&disc(&spec)).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("element", ErrorKind::OutOfRange { value: 8, limit: 8 })
    );
}

#[test]
fn every_figure_has_its_own_portrait() {
    let r = read(&disc(&sample())).unwrap();
    let portraits: Vec<&str> = r.portraits.iter().map(|p| p.as_deref().unwrap()).collect();
    let mut unique = portraits.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), portraits.len());
    let of = |id: &str| {
        let i = r.figures.iter().position(|f| f.id() == id).unwrap();
        portraits[i]
    };
    // The old legendaries go by the alt-character calls' order: Dark Spyro's
    // sprite is named under 416.
    assert_eq!(of("00001c-0000"), "Collection_MinionIcon_416_Leg_SpyroDark");
    assert_eq!(of("000000-0800"), "Collection_MinionIcon_Series2_000");
    assert_eq!(of("0000d0-0402"), "Collection_ItemIcon_208_Leg_Cannon");
    assert_eq!(of("000202-0000"), "Collection_ItemIcon_514_Pet_GillGrunt");

    // A missing sprite leaves the figure without one.
    let mut spec = sample();
    spec.sprites[0]
        .1
        .retain(|s| *s != "Collection_MinionIcon_112");
    let r = read(&disc(&spec)).unwrap();
    let rex = r
        .figures
        .iter()
        .position(|f| f.id() == "000070-0000")
        .unwrap();
    assert_eq!(r.portraits[rex], None);
}

#[test]
fn a_portrait_sprite_leads_to_its_image() {
    let d = disc(&sample());
    let r = read(&d).unwrap();
    let collection = Igz::parse(&d.collection).unwrap();
    let image = portrait_image(&collection, r.figures[0].portrait.unwrap()).unwrap();
    let image = crate::igz::layouts::Image2::read(&collection, image).unwrap();
    assert_eq!(image.name, Some("Collection_MinionIcon_000.png"));
}

#[test]
fn toy_ids_come_out_of_sprite_names() {
    assert_eq!(toy_id_in("Collection_MinionIcon_009"), Some(9));
    assert_eq!(
        toy_id_in("Collection_MinionIcon_Leg_000_AirDragon"),
        Some(0)
    );
    assert_eq!(
        toy_id_in("Collection_MinionIcon_419_LegTriggerHappy"),
        Some(419)
    );
    assert_eq!(toy_id_in("Collection_ItemIcon_208_Leg_Cannon"), Some(208));
    assert_eq!(toy_id_in("Collection_MinionIcon_Back_Owned"), None);
    assert_eq!(toy_id_in("Something_009"), None);
}

#[test]
fn ids_are_six_and_four_hex_digits() {
    let f = Figure {
        toy_type: 0x1A0,
        variant: 0x603,
        kind: Kind::Alt,
        deco: Some(3),
        name: 0,
        word: None,
        element: None,
        category: Category::Figure,
        level_load: 0,
        portrait: None,
    };
    assert_eq!(f.id(), "0001a0-0603");
}
