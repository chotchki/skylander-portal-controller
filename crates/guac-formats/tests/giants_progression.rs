//! AH.2's progression readers off the Giants dump: the level table, the 24
//! save params `init_setup.bld`'s save-param table sets up, and the gold
//! clamp `global.bld`'s "Game - Award Money" applies, each as ah-progression
//! read them (section 1 of its findings).
//! Ignored by default; it reads `GUAC_GIANTS_DUMP` like `giants_igz`:
//!
//! `cargo test -p guac-formats --release --test giants_progression -- --ignored --nocapture`

mod common;

use std::time::Instant;

use common::usrdir;
use guac_formats::exports::Exports;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::progression::{SaveKind, SaveTable, gold_clamp};

fn level(archive: &str) -> Vec<u8> {
    let path = usrdir().join(archive);
    let bytes = std::fs::read(&path).unwrap();
    let arc = Archive::parse(&bytes).unwrap();
    let entry = arc
        .find("level.bld")
        .unwrap_or_else(|| panic!("no level.bld in {}", path.display()));
    arc.read(entry).unwrap().into_owned()
}

/// (param, kind, word, first bit, passes, bit step, width, bits per field, limit).
type Expected = (&'static str, SaveKind, u32, u32, u32, u32, u32, u32, u32);

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn giants_progression_tables() {
    let started = Instant::now();
    let init = level("character/init_setup.bld");
    let global = level("permanent/global.bld");
    let boot = level("permanent/bootstrap.bld");
    let init = Igz::parse(&init).unwrap();
    let global = Igz::parse(&global).unwrap();
    let boot = Igz::parse(&boot).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    let parsed = started.elapsed();

    let started = Instant::now();
    let table = SaveTable::read(&init, &global, &exports).unwrap();
    let clamp = gold_clamp(&global).unwrap();
    let read = started.elapsed();
    println!(
        "parsed in {} ms, read in {} ms",
        parsed.as_millis(),
        read.as_millis()
    );
    println!("level table: {:?}", table.level_xp);
    for p in &table.params {
        let fields: Vec<String> = p
            .fields
            .iter()
            .map(|f| match &f.member {
                Some(m) => format!("{}/{} {m}", f.word, f.bit),
                None => format!("{}/{}", f.word, f.bit),
            })
            .collect();
        println!(
            "op {:>4} {:<40} {:<16} width {:>3} per field {:>2} limit {:>5}: {}",
            p.position,
            p.param,
            format!("{:?}", p.kind),
            p.width,
            p.bits_per_field,
            p.limit,
            fields.join(", ")
        );
    }
    println!("gold clamp: {clamp}");

    assert_eq!(
        table.level_xp,
        [
            0, 1000, 2200, 3800, 6000, 9000, 13000, 18200, 24800, 33000, 42700, 53900, 66600,
            80800, 96500
        ]
    );
    assert_eq!(clamp, 65_000);

    use SaveKind::*;
    // ah-progression's table, the loops spelled out: pp_Flags1_UpgradeBits'
    // ten bits from word 4's bit 0, the platform bits from bit 24, the name
    // characters two to a word from word 8.
    let expected: [Expected; 24] = [
        ("pp_Exp", Plain, 0, 0, 1, 0, 24, 0, 33000),
        ("pp_Money", Plain, 0, 24, 1, 0, 16, 0, 65535),
        ("pp_CumulativeTime", Plain, 1, 8, 1, 0, 32, 0, 0),
        ("pp_Flags1_UpgradeBits", Plain, 4, 0, 10, 1, 1, 0, 0),
        (
            "pp_Flags1_CollectionBoost_ElementCt",
            Plain,
            4,
            10,
            1,
            0,
            2,
            0,
            3,
        ),
        ("pp_PlatformUse", Plain, 4, 24, 8, 1, 1, 0, 0),
        ("pp_Hat", Plain, 5, 0, 1, 0, 16, 0, 45),
        ("pp_OwnerID", Plain, 6, 0, 1, 0, 64, 32, 0),
        ("pp_NameChars", Plain, 8, 0, 16, 16, 16, 0, 0),
        ("pp_NameVals", Plain, 8, 0, 1, 0, 256, 16, 0),
        ("pp_LastSeenDate", Plain, 16, 0, 1, 0, 32, 8, 0),
        ("pp_LastSeenDate_Year", Plain, 17, 0, 1, 0, 16, 0, 0),
        ("pp_ChallengeLevelsComplete", Plain, 17, 16, 1, 0, 32, 1, 0),
        ("pp_HeroPoints", Plain, 18, 16, 1, 0, 16, 0, 0),
        ("pp_OwnerCount", Plain, 19, 24, 1, 0, 8, 0, 255),
        ("pp_BirthDate", Plain, 20, 0, 1, 0, 32, 8, 0),
        ("pp_BirthDate_Year", Plain, 21, 0, 1, 0, 16, 0, 0),
        ("pp_Upgrade_WowPow", Plain, 29, 16, 1, 0, 1, 0, 0),
        ("pp_Upgrade_AltPath_Stored", Plain, 29, 17, 1, 0, 3, 1, 0),
        ("pp_Flags1_ToyAccolade_Rank", Plain, 29, 20, 1, 0, 2, 0, 0),
        // The second area experience runs on into: 96,500 - 33,000.
        ("pp_Exp", Spillover, 28, 24, 1, 0, 16, 0, 63500),
        ("pp_Hat", MirrorAndClear, 29, 8, 1, 0, 8, 0, 0),
        (
            "pp_Flags1_CollectionBoost_ElementCt",
            MirrorAndClear,
            29,
            22,
            1,
            0,
            3,
            0,
            0,
        ),
        (
            "pp_ChallengeLevelsComplete",
            ExtendedBitfield,
            33,
            0,
            1,
            0,
            24,
            1,
            0,
        ),
    ];
    assert_eq!(table.params.len(), expected.len());
    for (p, e) in table.params.iter().zip(expected) {
        let (param, kind, word, bit, passes, step, width, per_field, limit) = e;
        assert_eq!(
            (p.param.as_str(), p.kind, p.width, p.bits_per_field, p.limit),
            (param, kind, width, per_field, limit),
            "{param}"
        );
        // The name characters step their word every second pass.
        let fields: Vec<(u32, u32)> = p.fields.iter().map(|f| (f.word, f.bit)).collect();
        let want: Vec<(u32, u32)> = (0..passes)
            .map(|i| {
                if param == "pp_NameChars" {
                    (word + i / 2, bit + (i % 2) * step)
                } else {
                    (word, bit + i * step)
                }
            })
            .collect();
        assert_eq!(fields, want, "{param}");
    }
    // The upgrade bits name their members: the path, its choice, then the
    // eight upgrades, the soul gem last.
    let upgrades: Vec<&str> = table.params[3]
        .fields
        .iter()
        .map(|f| f.member.as_deref().unwrap_or(""))
        .collect();
    assert_eq!(upgrades[..2], ["pp__UpgradePath", "pp__UpgradePath_Choice"]);
    assert_eq!(upgrades[9], "pp_Upgrade(8)");
}
