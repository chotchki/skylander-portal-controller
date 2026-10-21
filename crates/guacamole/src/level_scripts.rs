//! The script part at level load (openspec AH.22, design decision 7): the
//! entries the extract holds as the disc does (`bootstrap.bld`'s,
//! `global.bld`'s, `init_setup.bld`'s, each fighter's and the level's own),
//! parsed by `guac-formats` and linked into one `guac-script` program on
//! the level's reading task, behind the portal like the rest of the load,
//! with what the host reads off the files (AH.16, [`crate::scripts::Files`]):
//! the TV installs the run from them ([`crate::scripts::install`]). A level
//! reports what it linked and how long it took; one that won't load leaves
//! the level drawn without it, the reason reported.

use std::path::Path;
use std::time::{Duration, Instant};

use guac_extract::{levels, roster, scripts};
use guac_formats::igz::Igz;
use guac_formats::script::Graph;
use guac_script::{Program, Role, Unit};

use crate::scripts::Files;

/// A level's script part linked, with what the load cost: the level's
/// reading task makes one beside the parts it draws.
#[derive(Debug)]
pub struct Linked {
    pub program: Program,
    /// What the host reads off `global.bld`'s, `init_setup.bld`'s, each
    /// fighter's and the level's files, and the units whose placements run
    /// from the level's start, in the order they run.
    pub placements: Files,
    pub order: Vec<u16>,
    /// Each fighter's load file and its unit: his file's placements join the
    /// run when he's the hero (openspec AH.24).
    pub fighters: Vec<(String, u16)>,
    /// Every entry, the level's own included, and their bytes in all.
    pub files: usize,
    pub bytes: u64,
    /// Reading the files, parsing them and their graphs, and linking.
    pub read: Duration,
    pub parsed: Duration,
    pub linked: Duration,
}

impl Linked {
    pub fn took(&self) -> Duration {
        self.read + self.parsed + self.linked
    }
}

/// Each entry the level's scripts load with, by its file in the extract, in
/// link order, with what it is to the export ids.
pub fn files(level: &str) -> Vec<(String, Role)> {
    let role = |archive: &str| match archive {
        guac_extract::roster::BOOTSTRAP => Role::Bootstrap,
        guac_extract::roster::GLOBAL => Role::Global,
        _ => Role::Other,
    };
    scripts::archives()
        .iter()
        .map(|a| (scripts::script_path(a), role(a)))
        .chain(std::iter::once((levels::script_path(level), Role::Other)))
        .collect()
}

/// `level`'s script part from the extract at `dir`, parsed and linked. Why
/// it can't be is the error, naming the file.
pub fn load(dir: &Path, level: &str) -> Result<Linked, String> {
    let files = files(level);
    let started = Instant::now();
    let mut entries = Vec::with_capacity(files.len());
    for (rel, role) in &files {
        let bytes = std::fs::read(dir.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        entries.push((rel.as_str(), *role, bytes));
    }
    let read = started.elapsed();
    let started = Instant::now();
    let parsed = entries
        .iter()
        .map(|(rel, _, bytes)| Igz::parse(bytes).map_err(|e| format!("{rel}: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let graphs = parsed
        .iter()
        .zip(&entries)
        .map(|(igz, (rel, ..))| Graph::read(igz).map_err(|e| format!("{rel}: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let parse_took = started.elapsed();
    let started = Instant::now();
    let units: Vec<Unit<'_, '_>> = entries
        .iter()
        .zip(parsed.iter().zip(&graphs))
        .map(|((rel, role, _), (igz, graph))| Unit {
            name: rel,
            role: *role,
            igz,
            graph,
        })
        .collect();
    let program = Program::link(&units).map_err(|e| e.to_string())?;
    // global.bld's placements run first (the init setup writes through the
    // references their STARTUPs set), then init_setup.bld's, then the
    // level's.
    let unit = |rel: &str| {
        files
            .iter()
            .position(|(r, _)| r == rel)
            .map(|u| u as u16)
            .ok_or_else(|| format!("{rel}: not in the part"))
    };
    let order = vec![
        unit(&scripts::script_path(roster::GLOBAL))?,
        unit(&scripts::script_path(roster::INIT_SETUP))?,
        (files.len() - 1) as u16,
    ];
    let fighters: Vec<(String, u16)> = scripts::FIGHTERS
        .iter()
        .map(|f| {
            Ok((
                (*f).to_owned(),
                unit(&scripts::script_path(&scripts::character_archive(f)))?,
            ))
        })
        .collect::<Result<_, String>>()?;
    let units: Vec<(u16, &Igz<'_>)> = order
        .iter()
        .copied()
        .chain(fighters.iter().map(|(_, u)| *u))
        .map(|u| (u, &parsed[u as usize]))
        .collect();
    let placements = Files::read(&units).map_err(|e| format!("placements: {e}"))?;
    Ok(Linked {
        program,
        placements,
        order,
        fighters,
        files: entries.len(),
        bytes: entries.iter().map(|(_, _, b)| b.len() as u64).sum(),
        read,
        parsed: parse_took,
        linked: started.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guac_formats::roster::builder::{disc, sample};

    const LEVEL: &str = "level_213_tutotial";

    /// An extract dir holding a synthetic script part: the roster
    /// builder's bootstrap, global and init_setup entries, AH.13's creature
    /// as the fighter's and the level builder's chapter 1 as the level's.
    fn extract() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let d = disc(&sample());
        let (creature, _) = guac_formats::script::builder::creature();
        let level = guac_formats::level::builder::stage_level(
            &guac_formats::level::builder::stage_sample(),
        );
        let bytes = [&d.bootstrap, &d.global, &d.init_setup, &creature, &level];
        for ((rel, _), bytes) in files(LEVEL).iter().zip(bytes) {
            let path = tmp.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        tmp
    }

    #[test]
    fn the_part_is_five_entries_in_link_order() {
        let f = files(LEVEL);
        let names: Vec<(&str, Role)> = f.iter().map(|(r, role)| (r.as_str(), *role)).collect();
        assert_eq!(
            names,
            [
                ("scripts/bootstrap.igz", Role::Bootstrap),
                ("scripts/global.igz", Role::Global),
                ("scripts/init_setup.igz", Role::Other),
                ("scripts/009_eruptor.igz", Role::Other),
                ("levels/level_213_tutotial/script.igz", Role::Other),
            ]
        );
    }

    #[test]
    fn a_levels_script_part_links_into_one_program() {
        let tmp = extract();
        let linked = load(tmp.path(), LEVEL).unwrap();
        assert_eq!(linked.files, 5);
        let scripts: Vec<&str> = linked
            .program
            .scripts
            .iter()
            .map(|s| s.name.rsplit('/').next().unwrap())
            .collect();
        assert_eq!(
            scripts,
            [
                "Spyro_PortalManager.ai",
                "Init_Setup.ai",
                "Init_Setup_Minions.ai",
                "Init_Setup_SaveGame_Portal.ai",
                "Creature.ai",
                "InitializeLevel.ai",
                "S2_Enemy_Chompy.ai",
                "Level_Definition_900.ai",
            ]
        );
        assert!(linked.program.script_named("Creature.ai").is_some());
        assert!(linked.took() >= linked.linked);
    }

    #[test]
    fn a_part_that_wont_load_names_its_file() {
        let tmp = extract();
        std::fs::remove_file(tmp.path().join("scripts/009_eruptor.igz")).unwrap();
        let e = load(tmp.path(), LEVEL).unwrap_err();
        assert!(e.starts_with("scripts/009_eruptor.igz: "), "{e}");
        std::fs::write(tmp.path().join("scripts/009_eruptor.igz"), b"not an igz").unwrap();
        let e = load(tmp.path(), LEVEL).unwrap_err();
        assert!(e.starts_with("scripts/009_eruptor.igz: "), "{e}");
        // A script with no op list: AH.13's graph refuses it.
        std::fs::write(
            tmp.path().join("levels/level_213_tutotial/script.igz"),
            guac_formats::level::builder::sample().bytes,
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("scripts/009_eruptor.igz"),
            guac_formats::script::builder::creature().0,
        )
        .unwrap();
        let e = load(tmp.path(), LEVEL).unwrap_err();
        assert!(e.contains("_opList"), "{e}");
    }
}
