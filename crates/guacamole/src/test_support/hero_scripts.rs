//! AH.24's and AH.27's synthetic script parts and Giants' progression rows,
//! from hero_scripts' tests; play's tests stand on them too.

use std::path::Path;

use bevy::prelude::*;
use guac_formats::script::builder as attack_builder;
use guacamole::level_scripts;

pub(crate) const LEVEL: &str = "level_213_tutotial";

/// AH.24's synthetic script part in an extract at `dir`: the roster
/// builder's bootstrap entry, AH.24's attack (global, init_setup and the
/// fighter's) and AH.13's creature as the level's, at the paths the TV
/// links them from.
pub(crate) fn write_attack(dir: &Path) {
    let (creature, _) = attack_builder::creature();
    write_parts(dir, &creature);
}

/// AH.27's beside it: AH.24's attack with AH.27's breakable as the
/// level's entry, the barrel at `at`; the barrel's object index.
pub(crate) fn write_smash(dir: &Path, at: [f32; 3]) -> u32 {
    let s = attack_builder::smash(at);
    write_parts(dir, &s.level);
    s.barrel
}

/// The roster builder's bootstrap entry, AH.24's attack (global,
/// init_setup and the fighter's) and `level` as the level's.
fn write_parts(dir: &Path, level: &[u8]) {
    let d = guac_formats::roster::builder::disc(&guac_formats::roster::builder::sample());
    let a = attack_builder::attack();
    let bytes: [&[u8]; 5] = [&d.bootstrap, &a.global, &a.init_setup, &a.fighter, level];
    for ((rel, _), bytes) in level_scripts::files(LEVEL).iter().zip(bytes) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}

/// Giants' rows as AH.2 reads them, the table ours: what lays a
/// figure's words out in the synthetic part's tests.
pub(crate) fn progression_rows() -> guac_ir::progression::Progression {
    use guac_ir::progression::{Progression, SaveField, SaveKind, SaveParam};
    let row = |param: &str, kind, word, bit, width, limit| SaveParam {
        param: param.into(),
        kind,
        fields: vec![SaveField {
            word,
            bit,
            member: None,
        }],
        width,
        bits_per_field: 0,
        limit,
    };
    Progression {
        identity: crate::test_support::turntable::identity(),
        level_xp: vec![0, 1_000],
        save_params: vec![
            row("pp_Exp", SaveKind::Plain, 0, 0, 24, 33_000),
            row("pp_Money", SaveKind::Plain, 0, 24, 16, 65_535),
            row("pp_CumulativeTime", SaveKind::Plain, 1, 8, 32, 0),
            row("pp_Exp", SaveKind::Spillover, 28, 24, 16, 63_500),
        ],
        gold_cap: 65_000,
    }
}
