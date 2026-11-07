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

use guac_extract::intros::IrFormat;
use guac_extract::{levels, roster, scripts};
use guac_formats::igz::Igz;
use guac_formats::script::Graph;
use guac_ir::progression::Progression;
use guac_script::{Program, Role, Unit};

use crate::figure::Layout;
use crate::scripts::Files;

/// The figure's layout off the progression tables in the extract at `dir`
/// (AH.2's part, AH.23): why not when the part isn't there or its rows
/// don't lay a figure out (the e2e fixture's tables have none).
pub fn figure_layout(dir: &Path, ir: IrFormat) -> Result<Layout, String> {
    let rel = ir.progression_path();
    let bytes = std::fs::read(dir.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    let p: Progression = match ir {
        IrFormat::Ron => std::str::from_utf8(&bytes)
            .map_err(|e| e.to_string())
            .and_then(|t| guac_ir::from_ron(t).map_err(|e| e.to_string())),
        IrFormat::Postcard => guac_ir::from_postcard(&bytes).map_err(|e| e.to_string()),
    }
    .map_err(|e| format!("{rel}: {e}"))?;
    Layout::of(&p)
}

/// Each animation palette the extract's level palettes part holds (AH.22:
/// every slot of every model a placement of the level names), by its
/// model's path, as each slot's clip length at 100 %, 0 where a slot holds
/// no clip (openspec AH.27: what the TV sets for the level's placements, as
/// the headless runs set them). Why not when the part isn't there or is
/// another dump's or level's.
pub fn level_palettes(
    dir: &Path,
    level: &str,
    ir: IrFormat,
) -> Result<Vec<(String, Vec<f32>)>, String> {
    let rel = levels::part_path(ir, level, levels::PALETTES);
    let bytes = std::fs::read(dir.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
    let p: guac_ir::palette::LevelPalettes = match ir {
        IrFormat::Ron => std::str::from_utf8(&bytes)
            .map_err(|e| e.to_string())
            .and_then(|t| guac_ir::from_ron(t).map_err(|e| e.to_string())),
        IrFormat::Postcard => guac_ir::from_postcard(&bytes).map_err(|e| e.to_string()),
    }
    .map_err(|e| format!("{rel}: {e}"))?;
    if p.level != level {
        return Err(format!("{rel} holds {}'s palettes", p.level));
    }
    Ok(p.palettes
        .iter()
        .map(|palette| (palette.owner.clone(), lengths(palette)))
        .collect())
}

/// What `load_file`'s skinned spawns play off the extract (openspec AU.37):
/// his beast's or his knives' palette, read when he's placed as the rest of
/// his spawns are ([`crate::scripts::ScriptWorld::set_rigs`]). Why not when
/// the part isn't there or is another character's.
pub fn spawn_rigs(
    dir: &Path,
    load_file: &str,
    ir: IrFormat,
) -> Result<guac_ir::spawn::SpawnRigs, String> {
    let archive = guac_extract::scripts::character_archive(load_file);
    let rel = ir.spawn_rig_path(&archive);
    let bytes = std::fs::read(dir.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
    let r: guac_ir::spawn::SpawnRigs = match ir {
        IrFormat::Ron => std::str::from_utf8(&bytes)
            .map_err(|e| e.to_string())
            .and_then(|t| guac_ir::from_ron(t).map_err(|e| e.to_string())),
        IrFormat::Postcard => guac_ir::from_postcard(&bytes).map_err(|e| e.to_string()),
    }
    .map_err(|e| format!("{rel}: {e}"))?;
    if r.archive != archive {
        return Err(format!("{rel} holds {}'s rigs", r.archive));
    }
    Ok(r)
}

/// Each slot's clip length at 100 %, 0 where a slot holds no clip; a clip
/// past one Havok block its whole length (openspec AS.78: the animator
/// cameras' shots last it).
pub fn lengths(palette: &guac_ir::palette::Palette) -> Vec<f32> {
    let mut out = vec![0.0; usize::from(palette.slots)];
    for e in &palette.entries {
        if let Some(d) = palette.duration(e.slot)
            && let Some(l) = out.get_mut(usize::from(e.slot))
        {
            *l = d;
        }
    }
    out
}

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
    /// Each walker's (AG.27: every other modelled character the part holds
    /// an entry of) load file, unit and scripts: his file's placements join
    /// the run the same way, to walk on his own scripts, never to attack.
    pub walkers: Vec<crate::scripts::WalkerScripts>,
    /// The load files the part was made without, with why: each keeps his
    /// card.
    pub left_out: Vec<(String, String)>,
    /// Where a figure's values sit in its tag's words, from the extract's
    /// progression tables ([`figure_layout`], AH.23), or why not: [`load`]
    /// leaves it unread, the level's load reads it beside.
    pub layout: Result<Layout, String>,
    /// The level's placements' palettes by model ([`level_palettes`]), or
    /// why not: [`load`] leaves them unread, the level's load reads them
    /// beside.
    pub palettes: Result<Vec<(String, Vec<f32>)>, String>,
    /// The sounds part for the linked units ([`crate::sounds::load`],
    /// openspec AR.9): [`load`] leaves it unread, the level's load reads it
    /// beside.
    pub sounds: Option<crate::sounds::SoundPart>,
    /// What the interface's model needs of each unit's file, read here where
    /// the files are parsed (openspec AT.9), and `global.bld`'s exports.
    pub interface_keys: Vec<guac_interface::load::Keys>,
    pub interface_imports: Option<guac_interface::load::Imports>,
    /// The model the run starts from ([`crate::interface_host::load`]):
    /// [`load`] leaves it unread, the level's load reads it beside.
    pub interface: Option<crate::interface_host::InterfacePart>,
    /// The disc's levels by archive name, off the extract's manifest
    /// (openspec AS.15): a script's request naming one changes level. Empty
    /// when the manifest doesn't read.
    pub levels: Vec<String>,
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
/// link order, with what it is to the export ids: the base's, then the
/// level's (no walkers; [`files_with`] has them).
pub fn files(level: &str) -> Vec<(String, Role)> {
    files_with(&[], level)
}

/// [`files`] with the walkers' entries (`walkers`: load files, as
/// [`scripts::WalkerList::listed`]) between the fighters' and the level's.
pub fn files_with(walkers: &[String], level: &str) -> Vec<(String, Role)> {
    files_with_screens(walkers, &[], level)
}

/// [`files_with`] and then the screens' entries the extract holds
/// (`screens`: their files, [`guac_extract::screens::path`], openspec
/// AS.53) before the level's, which stays last: linked, their placements
/// never run, the level's end calling their macros.
pub fn files_with_screens(
    walkers: &[String],
    screens: &[String],
    level: &str,
) -> Vec<(String, Role)> {
    let role = |archive: &str| match archive {
        guac_extract::roster::BOOTSTRAP => Role::Bootstrap,
        guac_extract::roster::GLOBAL => Role::Global,
        _ => Role::Other,
    };
    scripts::archives_with(walkers)
        .iter()
        .map(|a| (scripts::script_path(a), role(a)))
        .chain(screens.iter().map(|s| (s.clone(), Role::Other)))
        .chain(std::iter::once((levels::script_path(level), Role::Other)))
        .collect()
}

/// The screens' entries the extract at `dir` holds (openspec AS.53): each
/// of the screens part's files there, none for an extract the part hasn't
/// reached yet (an older one resuming, a dump without the archive).
pub fn screens_in(dir: &Path) -> Vec<String> {
    guac_extract::screens::ARCHIVES
        .iter()
        .map(|a| guac_extract::screens::path(a))
        .filter(|p| dir.join(p).is_file())
        .collect()
}

/// The disc's levels by archive name, off the manifest of the extract at
/// `dir` (`guac_extract::levels::on_disc_at`), the ones a player is taken
/// into alone (`Kind::is_level`: a stream a script names, the voice pack
/// `challengevo` among them, or the title or the credits isn't a level
/// change; AS.15's review): none, logged, when it doesn't read.
pub fn disc_levels(dir: &Path) -> Vec<String> {
    match levels::on_disc_at(dir) {
        Ok(l) => l
            .into_iter()
            .filter(|l| l.kind.is_level())
            .map(|l| l.name)
            .collect(),
        Err(why) => {
            tracing::warn!(reason = %why, "the disc's level list didn't read: no script's request changes level");
            Vec::new()
        }
    }
}

/// `level`'s script part from the extract at `dir`, parsed and linked. Why
/// it can't be is the error, naming the file.
pub fn load(dir: &Path, level: &str) -> Result<Linked, String> {
    // The walkers the part was made with (AG.27); none for an extract made
    // before them.
    let list = scripts::read_walkers(dir)?;
    let files = files_with_screens(&list.listed, &screens_in(dir), level);
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
    let walker_units: Vec<(String, u16)> = list
        .listed
        .iter()
        .map(|w| {
            Ok((
                w.clone(),
                unit(&scripts::script_path(&scripts::character_archive(w)))?,
            ))
        })
        .collect::<Result<_, String>>()?;
    let units: Vec<(u16, &Igz<'_>)> = order
        .iter()
        .copied()
        .chain(fighters.iter().map(|(_, u)| *u))
        .chain(walker_units.iter().map(|(_, u)| *u))
        .map(|u| (u, &parsed[u as usize]))
        .collect();
    let placements = Files::read(&units).map_err(|e| format!("placements: {e}"))?;
    // Each walker's scripts for the whitelist, and the asks of the gameplay
    // viewport they add (AG.27, AH.107).
    let walkers = crate::scripts::walker_scripts(&program, &placements, &walker_units);
    // What the interface's model needs of the files (openspec AT.9).
    let interface_keys = parsed
        .iter()
        .enumerate()
        .map(|(u, igz)| guac_interface::load::Keys::read(u as u16, igz))
        .collect();
    let role_unit = |r: Role| entries.iter().position(|(_, role, _)| *role == r);
    let interface_imports = match (role_unit(Role::Bootstrap), role_unit(Role::Global)) {
        (Some(b), Some(g)) => guac_formats::exports::Exports::new(&parsed[b], &parsed[g])
            .ok()
            .map(|exports| guac_interface::load::Imports {
                exports,
                bootstrap: b as u16,
                global: g as u16,
            }),
        _ => None,
    };
    Ok(Linked {
        program,
        placements,
        order,
        fighters,
        walkers,
        left_out: list.left_out,
        layout: Err("not read".into()),
        palettes: Err("not read".into()),
        sounds: None,
        interface_keys,
        interface_imports,
        interface: None,
        levels: disc_levels(dir),
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

    /// AS.15's review: the disc's level list a script's request is held
    /// against holds the levels a player is taken into, off the manifest,
    /// and none of what levels stream or menus load (the voice pack,
    /// `global.bld` names in English, a ship part, the title, the credits);
    /// a folder with no manifest gives none.
    #[test]
    fn the_disc_levels_a_request_is_held_against_are_the_levels_alone() {
        use guac_extract::dump::SourceFile;
        use guac_extract::extract::{MANIFEST, Manifest};
        let tmp = tempfile::tempdir().unwrap();
        let source = |path: &str| SourceFile {
            path: path.into(),
            size: 1,
            mtime_ns: None,
            ino: None,
            ctime_ns: None,
            sha256: guac_ir::Sha256([0; 32]),
        };
        let manifest = Manifest {
            extractor: "test".into(),
            layout: 0,
            ir_version: 0,
            identity: guac_ir::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([0; 32]),
            },
            source_dir: String::new(),
            complete: true,
            sources: [
                "level/challengevo.bld",
                "level/credits_giants.bld",
                "level/level_214_tutotial_2.bld",
                "level/pvp_level_001_cubedungeon.bld",
                "level/ship_part_003.bld",
                "level/title.bld",
                "permanent/global.bld",
            ]
            .map(source)
            .to_vec(),
            outputs: Vec::new(),
            transcode: None,
            levels: Default::default(),
            parts: Default::default(),
        };
        assert!(disc_levels(tmp.path()).is_empty(), "no manifest, no list");
        std::fs::write(
            tmp.path().join(MANIFEST),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(
            disc_levels(tmp.path()),
            ["level_214_tutotial_2", "pvp_level_001_cubedungeon"]
        );
    }

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

    /// The level's palettes part (openspec AH.27): each palette by its
    /// model's path, each slot's clip length at 100 % (0 where a slot holds
    /// none); another level's refused, a missing part named.
    #[test]
    fn the_levels_palettes_by_model() {
        use guac_ir::palette::{LevelPalettes, Palette, PaletteSlot, SlotEntry};
        let tmp = tempfile::tempdir().unwrap();
        let ir = IrFormat::Ron;
        let rel = levels::part_path(ir, LEVEL, levels::PALETTES);
        let e = level_palettes(tmp.path(), LEVEL, ir).unwrap_err();
        assert!(e.starts_with(&rel), "{e}");
        let identity = guac_ir::DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: guac_ir::Sha256([0; 32]),
        };
        let clip = guac_ir::clip::Clip {
            name: "Coin_Spin.ma".into(),
            duration: 0.5,
            frame_duration: 1.0 / 30.0,
            frames: 16,
            tracks: Vec::new(),
        };
        let palettes = LevelPalettes {
            identity: identity.clone(),
            level: LEVEL.into(),
            palettes: vec![Palette {
                identity,
                owner: "C:/x/Coin.mdl".into(),
                slots: 3,
                entries: vec![PaletteSlot {
                    slot: 1,
                    name: None,
                    label: None,
                    entry: SlotEntry::Clip {
                        additive: false,
                        partial: false,
                        layered: false,
                        motion_extraction: 0,
                        clip,
                    },
                }],
                refused: Vec::new(),
            }],
        };
        let path = tmp.path().join(&rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, guac_ir::to_ron(&palettes).unwrap()).unwrap();
        assert_eq!(
            level_palettes(tmp.path(), LEVEL, ir).unwrap(),
            [("C:/x/Coin.mdl".to_owned(), vec![0.0, 0.5, 0.0])]
        );
        // Another level's, where level 212's should be: past the first
        // run's levels, under the name its rule carries (openspec AS.54).
        let past = levels::part_path(ir, "level_212", levels::PALETTES);
        assert_eq!(past, "levels/level_212/palettes.v3.ron");
        let other = tmp.path().join(&past);
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::copy(&path, &other).unwrap();
        let e = level_palettes(tmp.path(), "level_212", ir).unwrap_err();
        assert!(e.contains(&format!("holds {LEVEL}'s palettes")), "{e}");
    }

    /// An extract dir holding the synthetic part with the walkers (AG.27)
    /// `walkers` between the fighter's entry and the level's, as the script
    /// stage writes them, and the list that names them.
    fn extract_with_walkers(
        walkers: &[(&str, Vec<u8>)],
        left_out: &[(&str, &str)],
    ) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let d = disc(&sample());
        let (creature, _) = guac_formats::script::builder::creature();
        let level = guac_formats::level::builder::stage_level(
            &guac_formats::level::builder::stage_sample(),
        );
        let mut bytes: Vec<&[u8]> = vec![&d.bootstrap, &d.global, &d.init_setup, &creature];
        bytes.extend(walkers.iter().map(|(_, b)| b.as_slice()));
        bytes.push(&level);
        let loads: Vec<String> = walkers.iter().map(|(l, _)| (*l).to_owned()).collect();
        for ((rel, _), bytes) in files_with(&loads, LEVEL).iter().zip(bytes) {
            let path = tmp.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let list = scripts::WalkerList {
            listed: loads,
            left_out: left_out
                .iter()
                .map(|(l, w)| ((*l).to_owned(), (*w).to_owned()))
                .collect(),
        };
        std::fs::write(
            tmp.path().join(scripts::WALKERS_FILE),
            serde_json::to_vec(&list).unwrap(),
        )
        .unwrap();
        tmp
    }

    /// AG.27: the walkers' entries link between the fighter's and the
    /// level's; a walker's scripts are the ones his file's placements run at
    /// the join and his minion's (the template his definition's `Portal -
    /// Define Minion` names, whose script isn't named for his load file),
    /// his attacks' and spawns' left off; the TV's listing gains them; the
    /// characters the extract left out come through with why.
    #[test]
    fn the_walkers_link_and_their_scripts_join_the_listing() {
        use guac_formats::script::builder::{
            WALKER_LOAD, WALKER_MINION, WALKER_TEMPLATE, WalkerFault, walker,
        };
        let tmp = extract_with_walkers(
            &[(WALKER_LOAD, walker(WalkerFault::None))],
            &[("070_TreeRex", "no level.bld")],
        );
        let linked = load(tmp.path(), LEVEL).unwrap();
        assert_eq!(linked.files, 6, "the base, the walker and the level");
        assert_eq!(
            linked.left_out,
            [("070_TreeRex".to_owned(), "no level.bld".to_owned())]
        );
        let [w] = linked.walkers.as_slice() else {
            panic!("{:?}", linked.walkers);
        };
        assert_eq!(w.load_file, WALKER_LOAD);
        assert_eq!(w.unit, 4, "after the base's four, before the level's");
        let minion = format!("/{WALKER_MINION}");
        assert_eq!(w.scripts, ["/026_Def_Synthetic.ai", minion.as_str()]);
        assert_eq!(w.minion.as_deref(), Some(minion.as_str()));
        // The template is the placement his definition defines, not one
        // named for him.
        let template = linked
            .placements
            .placed
            .iter()
            .find(|p| Some(p.obj) == w.template)
            .expect("his minion's template");
        assert_eq!(template.name, WALKER_TEMPLATE);
        assert_eq!(w.asks, 0);
        // The level's own entry is the last unit still.
        assert_eq!(linked.order.last(), Some(&5));
        // Only the walker's scripts join the TV's listing, and with them the
        // minion's whole tick.
        let base = crate::scripts::smash_config(guac_script::Mode::Play);
        let with = crate::scripts::with_walkers(base.clone(), &linked.walkers);
        for script in [
            format!("C:/x/{WALKER_MINION}"),
            "C:/x/026_Def_Synthetic.ai".to_owned(),
        ] {
            assert!(!base.whitelist.lists("", &script), "{script}");
            assert!(with.whitelist.lists("", &script), "{script}");
        }
        // Nothing is listed for being named like him.
        assert!(!with.whitelist.lists("", "C:/x/026_Synthetic.ai"));
        assert!(!with.whitelist.lists("", "C:/x/026_Synthetic_Attack.ai"));
        assert!(
            !with
                .whitelist
                .lists("", &format!("C:/x/XX_{WALKER_MINION}"))
        );
    }

    /// AG.27: a walker whose definition defines no minion has none to list,
    /// and none of his scripts join the TV's listing: he keeps his card.
    #[test]
    fn a_walker_whose_definition_defines_no_minion_isnt_listed() {
        use guac_formats::script::builder::{WALKER_LOAD, WalkerFault, walker};
        let tmp = extract_with_walkers(&[(WALKER_LOAD, walker(WalkerFault::NoMinion))], &[]);
        let linked = load(tmp.path(), LEVEL).unwrap();
        let [w] = linked.walkers.as_slice() else {
            panic!("{:?}", linked.walkers);
        };
        assert_eq!((w.minion.as_deref(), w.asks), (None, 0));
        assert_eq!(w.scripts, ["/026_Def_Synthetic.ai"]);
        let base = crate::scripts::smash_config(guac_script::Mode::Play);
        let with = crate::scripts::with_walkers(base, &linked.walkers);
        assert!(!with.whitelist.lists("", "C:/x/026_Def_Synthetic.ai"));
    }

    /// AG.27 with AH.107: a walker whose scripts ask a point against the
    /// gameplay viewport, which the host answers "on screen" for every
    /// point, is counted (1 ask) and left off the listing, with the reason
    /// listed; one that doesn't isn't.
    #[test]
    fn a_walker_asking_the_viewport_is_counted_and_not_listed() {
        use guac_formats::script::builder::{WALKER_LOAD, WalkerFault, walker};
        let tmp = extract_with_walkers(&[(WALKER_LOAD, walker(WalkerFault::Asks))], &[]);
        let linked = load(tmp.path(), LEVEL).unwrap();
        assert_eq!(linked.walkers[0].asks, 1);
        let base = crate::scripts::smash_config(guac_script::Mode::Play);
        let with = crate::scripts::with_walkers(base, &linked.walkers);
        assert!(!with.whitelist.lists(
            "",
            &format!("C:/x/{}", guac_formats::script::builder::WALKER_MINION)
        ));
        assert!(!with.whitelist.lists("", "C:/x/026_Def_Synthetic.ai"));
    }

    /// AG.27: an extract made before the walkers (no list) links the base
    /// alone, as it always did.
    #[test]
    fn an_extract_without_the_walkers_list_links_the_base() {
        let tmp = extract();
        let linked = load(tmp.path(), LEVEL).unwrap();
        assert!(linked.walkers.is_empty() && linked.left_out.is_empty());
        assert_eq!(linked.files, 5);
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
