//! The progression tables off the dump (openspec AH.2, design decisions 2
//! and 10): `character/init_setup.bld`'s level table and save params and
//! `permanent/global.bld`'s gold clamp, through `permanent/bootstrap.bld`'s
//! export ids, into IR's [`Progression`]. The extract keeps them as a part of
//! their own (AG.15's per-part rule), so an extract made before them resumes
//! for them alone; writing it is the progression stage's ([`crate::stages`]).

use std::path::{Path, PathBuf};

use guac_formats::exports::Exports;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::progression::{self as fprogression, SaveTable, gold_clamp};
use guac_ir::DumpIdentity;
use guac_ir::progression::{Progression, SaveField, SaveKind, SaveParam};

use crate::extract::{IdentityDir, Manifest};
use crate::intros::IrFormat;
use crate::roster::{BOOTSTRAP, GLOBAL, INIT_SETUP, LEVEL};
use crate::{ExtractError, Result};

/// The part's name in the manifest.
pub const PART: &str = "progression";
/// The extract-wide parts this build reads, each recorded once its files
/// are in.
pub const PARTS: [&str; 1] = [PART];

/// Whether `m` holds every extract-wide part this build reads.
pub fn complete(m: &Manifest) -> bool {
    PARTS.iter().all(|p| m.parts.contains_key(*p))
}

fn unreadable(archive: &str, e: impl ToString) -> ExtractError {
    ExtractError::ProgressionUnreadable {
        path: PathBuf::from(archive),
        reason: e.to_string(),
    }
}

/// The level an archive under `usrdir` keeps as `level.bld`.
fn level(usrdir: &Path, archive: &str) -> Result<Vec<u8>> {
    let path = usrdir.join(archive);
    let bytes = std::fs::read(&path).map_err(ExtractError::io(&path))?;
    let arc = Archive::parse(&bytes).map_err(|e| unreadable(archive, e))?;
    let entry = arc
        .find(LEVEL)
        .ok_or_else(|| unreadable(archive, "no level.bld"))?;
    Ok(arc
        .read(entry)
        .map_err(|e| unreadable(archive, e))?
        .into_owned())
}

/// Reads the tables off the dump's `PS3_GAME/USRDIR`. A table that doesn't
/// read is [`ExtractError::ProgressionUnreadable`], naming its archive.
pub fn read(usrdir: &Path, identity: DumpIdentity) -> Result<Progression> {
    let (init, global, boot) = (
        level(usrdir, INIT_SETUP)?,
        level(usrdir, GLOBAL)?,
        level(usrdir, BOOTSTRAP)?,
    );
    let parse = |archive, bytes| Igz::parse(bytes).map_err(|e| unreadable(archive, e));
    let init = parse(INIT_SETUP, &init)?;
    let global = parse(GLOBAL, &global)?;
    let boot = parse(BOOTSTRAP, &boot)?;
    let exports = Exports::new(&boot, &global).map_err(|e| unreadable(GLOBAL, e))?;
    let table = SaveTable::read(&init, &global, &exports).map_err(|e| unreadable(INIT_SETUP, e))?;
    let gold_cap = gold_clamp(&global).map_err(|e| unreadable(GLOBAL, e))?;
    Ok(Progression {
        identity,
        level_xp: table.level_xp,
        save_params: table.params.into_iter().map(save_param).collect(),
        gold_cap,
    })
}

fn save_param(p: fprogression::SaveParam) -> SaveParam {
    SaveParam {
        param: p.param,
        kind: match p.kind {
            fprogression::SaveKind::Plain => SaveKind::Plain,
            fprogression::SaveKind::Spillover => SaveKind::Spillover,
            fprogression::SaveKind::MirrorAndClear => SaveKind::MirrorAndClear,
            fprogression::SaveKind::ExtendedBitfield => SaveKind::ExtendedBitfield,
        },
        fields: p
            .fields
            .into_iter()
            .map(|f| SaveField {
                word: f.word,
                bit: f.bit,
                member: f.member,
            })
            .collect(),
        width: p.width,
        bits_per_field: p.bits_per_field,
        limit: p.limit,
    }
}

/// The progression a current extract holds, refused when it doesn't read or
/// names another dump.
pub fn load(dir: &IdentityDir, ir: IrFormat) -> Result<Progression> {
    let progression: Progression = ir.decode(dir, ir.progression_path())?;
    if progression.identity != dir.manifest().identity {
        let path = dir.path().join(ir.progression_path());
        return Err(ExtractError::Io {
            source: std::io::Error::other(format!("{} names another dump", path.display())),
            path,
        });
    }
    Ok(progression)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guac_formats::iga::builder::IgaBuilder;
    use guac_formats::roster::builder::{disc, sample};

    fn identity() -> DumpIdentity {
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: guac_ir::Sha256([7; 32]),
        }
    }

    fn usrdir(tmp: &Path, d: &guac_formats::roster::builder::Disc) -> PathBuf {
        let usr = tmp.join("PS3_GAME/USRDIR");
        for (path, bytes) in d.archives() {
            let path = usr.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        usr
    }

    #[test]
    fn the_sample_dump_reads_into_ir() {
        let tmp = tempfile::tempdir().unwrap();
        let usr = usrdir(tmp.path(), &disc(&sample()));
        let p = read(&usr, identity()).unwrap();
        assert_eq!(p.identity, identity());
        assert_eq!(p.level_xp, [0, 400, 1100, 2500, 4500]);
        assert_eq!(p.gold_cap, 9000);
        assert_eq!(p.save_params.len(), 10);
        let spill = &p.save_params[7];
        assert_eq!(
            (spill.param.as_str(), spill.kind, spill.limit),
            ("pp_Exp", SaveKind::Spillover, 1500)
        );
        let bits = &p.save_params[3];
        assert_eq!(bits.fields.len(), 4);
        assert_eq!(bits.fields[1].member.as_deref(), Some("pp_Upgrade(1)"));
    }

    #[test]
    fn a_table_that_doesnt_read_names_its_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let mut spec = sample();
        spec.progression = None;
        let usr = usrdir(tmp.path(), &disc(&spec));
        let e = read(&usr, identity()).unwrap_err();
        assert_eq!(e.code(), "GUAC-E-PROGRESSION-UNREADABLE");
        assert!(e.to_string().contains(INIT_SETUP), "{e}");
        // An archive with no level in it.
        std::fs::write(
            usr.join(INIT_SETUP),
            IgaBuilder::new().stored("other.igz", b"x").build(),
        )
        .unwrap();
        let e = read(&usr, identity()).unwrap_err();
        assert!(e.to_string().contains("no level.bld"), "{e}");
    }
}
