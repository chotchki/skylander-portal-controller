//! AH.24's, AH.27's and AH.105's synthetic script parts and Giants'
//! progression rows, from hero_scripts' tests; play's and swap's tests stand
//! on them too.

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

/// AH.105's beside it: AH.24's attack with AH.105's creature and its rock
/// as the level's entry, the creature at `at`.
pub(crate) fn write_swap(dir: &Path, at: [f32; 3]) {
    write_parts(dir, &attack_builder::swap(at));
}

/// AH.25's beside it: AH.24's attack with AH.25's creature, its two
/// triggers and the controller waiting on them as the level's entry.
pub(crate) fn write_fight(dir: &Path, creature_at: [f32; 3], triggers: [[f32; 3]; 2]) {
    write_parts(dir, &attack_builder::fight(creature_at, triggers));
}

/// AH.25's fight beside AG.27's synthetic walker: the dev entry's level with
/// a character who walks and doesn't fight (openspec AH.167).
pub(crate) fn write_fight_with_walker(dir: &Path, creature_at: [f32; 3], triggers: [[f32; 3]; 2]) {
    write_parts_with(
        dir,
        &attack_builder::fight(creature_at, triggers),
        &[(
            attack_builder::WALKER_LOAD,
            attack_builder::walker(attack_builder::WalkerFault::None),
        )],
    );
}

/// The roster builder's bootstrap entry, AH.24's attack (global,
/// init_setup and the fighter's) and `level` as the level's.
fn write_parts(dir: &Path, level: &[u8]) {
    write_parts_with(dir, level, &[]);
}

/// [`write_parts`] with the walkers' entries (AG.27) between the fighter's
/// and the level's, and the list that names them, as the extract's script
/// stage writes them.
fn write_parts_with(dir: &Path, level: &[u8], walkers: &[(&str, Vec<u8>)]) {
    let a = attack_builder::attack();
    write_files(dir, [&a.global, &a.init_setup, &a.fighter], level, walkers);
}

/// AG.28's synthetic camera ([`attack_builder::camera::camera`]) as the
/// part: its `global.bld`, `init_setup.bld` and fighter's entries and its
/// level's, the level's placements at `at`.
pub(crate) fn write_camera(
    dir: &Path,
    at: attack_builder::camera::CameraAt,
    fault: attack_builder::camera::CameraFault,
) {
    let c = attack_builder::camera::camera(at, fault);
    write_files(dir, [&c.global, &c.init_setup, &c.fighter], &c.level, &[]);
}

/// [`write_camera`] with AG.27's synthetic walker beside the fighter
/// ([`attack_builder::walker`], stopping as `walker` says), the character
/// [`attack_builder::WALKER_LOAD`] (openspec AG.190).
pub(crate) fn write_camera_with_walker(
    dir: &Path,
    at: attack_builder::camera::CameraAt,
    fault: attack_builder::camera::CameraFault,
    walker: attack_builder::WalkerFault,
) {
    let c = attack_builder::camera::camera(at, fault);
    write_files(
        dir,
        [&c.global, &c.init_setup, &c.fighter],
        &c.level,
        &[(attack_builder::WALKER_LOAD, attack_builder::walker(walker))],
    );
}

/// The roster builder's bootstrap entry, `global.bld`'s, `init_setup.bld`'s
/// and the fighter's (`entries`), the walkers' and `level` as the level's.
fn write_files(dir: &Path, entries: [&[u8]; 3], level: &[u8], walkers: &[(&str, Vec<u8>)]) {
    let d = guac_formats::roster::builder::disc(&guac_formats::roster::builder::sample());
    let mut bytes: Vec<&[u8]> = vec![&d.bootstrap];
    bytes.extend(entries);
    bytes.extend(walkers.iter().map(|(_, b)| b.as_slice()));
    bytes.push(level);
    let loads: Vec<String> = walkers.iter().map(|(l, _)| (*l).to_owned()).collect();
    for ((rel, _), bytes) in level_scripts::files_with(&loads, LEVEL).iter().zip(bytes) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let list = guac_extract::scripts::WalkerList {
        listed: loads,
        left_out: Vec::new(),
    };
    std::fs::write(
        dir.join(guac_extract::scripts::WALKERS_FILE),
        serde_json::to_vec(&list).unwrap(),
    )
    .unwrap();
}

/// AG.27's beside AH.24's: the same part with the synthetic walker
/// ([`attack_builder::walker`], stopping as `fault` says) as a character the
/// extract holds, [`attack_builder::WALKER_LOAD`].
pub(crate) fn write_walker(dir: &Path, fault: attack_builder::WalkerFault) {
    let (creature, _) = attack_builder::creature();
    write_parts_with(
        dir,
        &creature,
        &[(attack_builder::WALKER_LOAD, attack_builder::walker(fault))],
    );
}

/// [`write_walker`] over AH.151's synthetic mover update
/// ([`attack_builder::attack_with_mover`]): the first player's velocity
/// handed what his mover's tick kept, each tick.
pub(crate) fn write_walker_moving(dir: &Path, fault: attack_builder::WalkerFault) {
    let (creature, _) = attack_builder::creature();
    let a = attack_builder::attack_with_mover();
    write_files(
        dir,
        [&a.global, &a.init_setup, &a.fighter],
        &creature,
        &[(attack_builder::WALKER_LOAD, attack_builder::walker(fault))],
    );
}

/// [`write_walker_moving`] with AG.219's synthetic locomotion definitions
/// ([`attack_builder::attack_with_locomotion`]): the first player's walk
/// and run as his scripts would define them.
pub(crate) fn write_walker_locomotion(dir: &Path, fault: attack_builder::WalkerFault) {
    let (creature, _) = attack_builder::creature();
    let a = attack_builder::attack_with_locomotion();
    write_files(
        dir,
        [&a.global, &a.init_setup, &a.fighter],
        &creature,
        &[(attack_builder::WALKER_LOAD, attack_builder::walker(fault))],
    );
}

/// AH.191's: two walkers with numbers of their own
/// ([`attack_builder::WALKER`], [`attack_builder::WALKER_OTHER`]), each
/// finding or making his description in `combat control data` as Giants'
/// minions do ([`attack_builder::walker_described`] over
/// [`attack_builder::attack_with_creatures`]).
pub(crate) fn write_walkers_described(dir: &Path) {
    let (creature, _) = attack_builder::creature();
    let a = attack_builder::attack_with_creatures();
    let walkers: Vec<(&str, Vec<u8>)> = [attack_builder::WALKER, attack_builder::WALKER_OTHER]
        .iter()
        .map(|s| (s.load, attack_builder::walker_described(s)))
        .collect();
    write_files(
        dir,
        [&a.global, &a.init_setup, &a.fighter],
        &creature,
        &walkers,
    );
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
