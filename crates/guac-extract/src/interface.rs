//! The interface (openspec AT.8, design decision 12): each archive's sprite
//! templates, bitmap fonts and English text through guac-formats' interface
//! reader ([`guac_formats::interface`]) into [`guac_ir::interface`], the
//! images they draw into the characters' texture store (AC.6's decoder, the
//! extract's KTX2).
//!
//! Two homes, each resuming alone (AG.15's rule):
//! - the extract-wide part ([`PART`]): `global.bld`, `bootstrap.bld`, every
//!   character archive on the dump (each one's upgrade images out of the
//!   `.arc` beside it) and the 16 menu archives (`misc/ui_*.bld`,
//!   `misc/upgradeui.bld`), each into `interface/<stem>.<ron|postcard>`;
//! - each level's `sprites` part ([`crate::levels::SPRITES`]), read with
//!   the level's other parts at the first run or when a level is asked for,
//!   never as a walk of its own; a level's streamed images (its `.arc`, not
//!   read) keep no texture.
//!
//! Every archive gets a file, an archive with no templates an empty one, so
//! the part knows what it holds without reading the archive. The files'
//! names carry the record's shape ([`guac_ir::interface::RECORD`], AT.27):
//! an extract holding another shape's isn't current ([`record_current`]),
//! and its next start reads both parts again, nothing else. An archive the
//! dump lacks is listed and the part lands without it (the e2e fixture's
//! dump holds none); one that's there and won't read refuses the part, and
//! the other stages land.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use guac_formats::iga::Archive;
use guac_formats::igz::layouts::{Image2, Layout, VisualType, is_a};
use guac_formats::igz::{ExternalId, Igz};
use guac_formats::interface::{self as finterface, Art as FArt, Content as FContent, TextKey};
use guac_formats::language::{LanguagePack, join};
use guac_ir::DumpIdentity;
use guac_ir::interface::{
    Art, Content, Font, Glyph, Import, Interface, Kind, Matrix, RECORD, Str, Template,
};

use crate::extract::Manifest;
use crate::intros::IrFormat;
use crate::models::Textures;
use crate::stages::{Event, Files, count_kept, held, put};
use crate::{ExtractError, IdentityDir, Result, models, roster, scripts, sounds};

/// The extract-wide part ([`crate::PARTS`]).
pub const PART: &str = "interface";
/// The folder each archive's IR file sits in.
pub const DIR: &str = "interface";
/// Where the menu archives sit, under `PS3_GAME/USRDIR`.
pub const MENU_DIR: &str = "misc";
/// The upgrade screen's archive, the one menu archive not named `ui_*`.
pub const UPGRADE_UI: &str = "misc/upgradeui.bld";

fn ext(ir: IrFormat) -> &'static str {
    match ir {
        IrFormat::Ron => "ron",
        IrFormat::Postcard => "postcard",
    }
}

/// `interface/<stem>.v<RECORD>.<ron|postcard>`.
pub fn path(ir: IrFormat, archive: &str) -> String {
    format!("{DIR}/{}.v{RECORD}.{}", scripts::stem(archive), ext(ir))
}

/// A level's `sprites` part's file, `levels/<level>/sprites.v<RECORD>.<ron|postcard>`.
pub fn level_path(ir: IrFormat, level: &str) -> String {
    format!(
        "{}/{level}/{}.v{RECORD}.{}",
        crate::levels::LEVEL_DIR,
        crate::levels::SPRITES,
        ext(ir)
    )
}

/// Whether every IR file the interface part and each first-run level's
/// `sprites` part record is named for this build's [`RECORD`] (AT.27): the
/// manifest's currency check asks it beside the parts' keys, so an extract
/// holding another shape's files opens as a resume and its next start reads
/// the two parts again. A part not recorded is the keys' check's to refuse.
pub fn record_current(m: &Manifest) -> bool {
    let tag = format!(".v{RECORD}.");
    let named = |files: &Vec<String>| {
        files
            .iter()
            .filter(|f| !f.starts_with(models::TEXTURE_DIR))
            .all(|f| f.contains(&tag))
    };
    m.parts.get(PART).is_none_or(named)
        && crate::levels::FIRST_RUN.iter().all(|level| {
            m.levels
                .get(*level)
                .and_then(|l| l.parts.get(crate::levels::SPRITES))
                .is_none_or(named)
        })
}

/// Whether `archive` (a path under `USRDIR`) is one of the menu archives.
pub fn is_menu(archive: &str) -> bool {
    let Some(file) = archive
        .strip_prefix(MENU_DIR)
        .and_then(|r| r.strip_prefix('/'))
    else {
        return false;
    };
    !file.contains('/')
        && file.ends_with(".bld")
        && (file.starts_with("ui_") || archive == UPGRADE_UI)
}

/// The extract-wide part's archives: `global.bld`, `bootstrap.bld`, then
/// every character archive and every menu archive the dump scan recorded,
/// each sorted by path.
pub fn archives(m: &Manifest) -> Vec<String> {
    let prefix = format!("{}/", models::CHARACTER_DIR);
    let mut characters: Vec<String> = Vec::new();
    let mut menus: Vec<String> = Vec::new();
    for s in &m.sources {
        let p = s.path.as_str();
        if let Some(file) = p.strip_prefix(&prefix)
            && !file.contains('/')
            && file.ends_with(".bld")
        {
            characters.push(p.to_owned());
        } else if is_menu(p) {
            menus.push(p.to_owned());
        }
    }
    characters.sort();
    menus.sort();
    [roster::GLOBAL.to_owned(), roster::BOOTSTRAP.to_owned()]
        .into_iter()
        .chain(characters)
        .chain(menus)
        .collect()
}

/// One archive read; its textures' bytes are KTX2s out of its `level.bld`
/// or its `.arc`, each once.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub ir: Interface,
    pub textures: Vec<(String, Vec<u8>)>,
}

/// The archive's `level.bld` and, if it has one, `ENGLISH.pak`.
fn entries(bld: &[u8]) -> std::result::Result<(Vec<u8>, Option<Vec<u8>>), String> {
    let arc = Archive::parse(bld).map_err(|e| e.to_string())?;
    let read = |name: &str| -> std::result::Result<Option<Vec<u8>>, String> {
        arc.find(name)
            .map(|e| {
                arc.read(e)
                    .map(|d| d.into_owned())
                    .map_err(|e| e.to_string())
            })
            .transpose()
    };
    let level = read(roster::LEVEL)?.ok_or("no level.bld")?;
    Ok((level, read(sounds::ENGLISH_PACK)?))
}

/// An image's size in pixels, off its layout rather than its decode.
fn size(igz: &Igz<'_>, image: usize) -> std::result::Result<[u32; 2], String> {
    let i = Image2::read(igz, image).map_err(|e| e.to_string())?;
    Ok([u32::from(i.width), u32::from(i.height)])
}

fn kind(k: VisualType) -> Option<Kind> {
    Some(match k {
        VisualType::Art => Kind::Art,
        VisualType::Text => Kind::Text,
        VisualType::Model => Kind::Model,
        VisualType::Container => Kind::Container,
        VisualType::Particles => Kind::Particles,
        VisualType::Other(_) => return None,
    })
}

fn matrix(m: guac_formats::igz::layouts::EulerTransform) -> Matrix {
    Matrix {
        position: m.position,
        rotation: m.rotation,
    }
}

fn import(id: ExternalId) -> Import {
    Import {
        namespace: id.namespace,
        object: id.object,
    }
}

/// The `.arc` entry a streamed image comes from: the one whose name holds
/// the image's source path as a folder (`.../eruptor_up5b.png/0x....igz`),
/// case ignored.
fn arc_entry(arc: &Archive<'_>, source: &str) -> std::result::Result<usize, String> {
    let needle = format!("/{}/", source.to_ascii_lowercase());
    let hits: Vec<usize> = arc
        .entries()
        .iter()
        .enumerate()
        .filter(|(_, e)| e.name().to_ascii_lowercase().contains(&needle))
        .map(|(i, _)| i)
        .collect();
    match hits[..] {
        [one] => Ok(one),
        [] => Err("no .arc entry".into()),
        _ => Err(format!("{} .arc entries", hits.len())),
    }
}

/// A streamed image out of the `.arc`: its one `igImage2`, as a texture.
fn streamed(
    arc: &Archive<'_>,
    source: &str,
) -> std::result::Result<(String, Vec<u8>, [u32; 2]), String> {
    let entry = &arc.entries()[arc_entry(arc, source)?];
    let bytes = arc.read(entry).map_err(|e| e.to_string())?;
    let igz = Igz::parse(&bytes).map_err(|e| e.to_string())?;
    let image = igz
        .objects()
        .iter()
        .position(|o| is_a(igz.type_name(o), Image2::TYPE))
        .ok_or("no igImage2 in its .arc entry")?;
    let mut t = Textures::default();
    let path = t.path(&igz, image).map_err(|e| e.to_string())?;
    let size = size(&igz, image)?;
    let (_, ktx2) = t.files.pop().ok_or("no texture written")?;
    Ok((path, ktx2, size))
}

/// Reads one archive's interface: `bld` is its `.bld`, `arc` its `.arc` (the
/// character archives' upgrade images), if read. An image that won't
/// decode, and a streamed one the `.arc` lacks, are listed in the IR's
/// `refused` and keep no texture; the error is an archive that won't read.
pub fn read(
    archive: &str,
    bld: &[u8],
    arc: Option<&[u8]>,
    identity: DumpIdentity,
) -> std::result::Result<Read, String> {
    let (level, pack) = entries(bld)?;
    let igz = Igz::parse(&level).map_err(|e| format!("level.bld: {e}"))?;
    read_igz(archive, &igz, pack.as_deref(), arc, identity)
}

/// The English pack of an archive's `.bld`, if it has one: what a level's
/// `sprites` part reads beside the `level.bld` its other parts share.
pub fn english_pack(bld: &[u8]) -> std::result::Result<Option<Vec<u8>>, String> {
    let arc = Archive::parse(bld).map_err(|e| e.to_string())?;
    arc.find(sounds::ENGLISH_PACK)
        .map(|e| {
            arc.read(e)
                .map(|d| d.into_owned())
                .map_err(|e| e.to_string())
        })
        .transpose()
}

/// [`read`] over a `level.bld` already parsed and its English pack's bytes.
pub fn read_igz(
    archive: &str,
    igz: &Igz<'_>,
    pack: Option<&[u8]>,
    arc: Option<&[u8]>,
    identity: DumpIdentity,
) -> std::result::Result<Read, String> {
    let ui = finterface::Interface::read(igz).map_err(|e| format!("level.bld: {e}"))?;
    let strings = match pack {
        None => Vec::new(),
        // Text it names none of, so none read off its pack: the voice
        // pack's (`level/challengevo.bld`) holds no language pack info.
        Some(_) if ui.strings.is_empty() => Vec::new(),
        Some(pack) => {
            let pack = Igz::parse(pack).map_err(|e| format!("{}: {e}", sounds::ENGLISH_PACK))?;
            let pack = LanguagePack::read(&pack).map_err(|e| e.to_string())?;
            // The pack's strings follow the same keys in the same order as
            // the reader's settings, and `join` holds their counts equal.
            join(igz, &pack)
                .map_err(|e| e.to_string())?
                .into_iter()
                .zip(&ui.strings)
                .map(|((_, text), s)| Str {
                    text: text.to_owned(),
                    flags: s.flags,
                    integer_digits: s.integer_digits,
                    decimal_digits: s.decimal_digits,
                })
                .collect()
        }
    };
    let arc = arc
        .map(Archive::parse)
        .transpose()
        .map_err(|e| format!(".arc: {e}"))?;
    let mut textures = Textures::default();
    let mut extra: Vec<(String, Vec<u8>)> = Vec::new();
    let mut refused = Vec::new();
    let mut fonts = Vec::with_capacity(ui.fonts.len());
    for f in &ui.fonts {
        let label = f.name.unwrap_or("a font").to_owned();
        let atlas = textures
            .path(igz, f.atlas)
            .map_err(|e| format!("{label}'s atlas: {e}"))?;
        fonts.push(Font {
            name: f.name.map(str::to_owned),
            source: f.source.map(str::to_owned),
            height: f.height,
            base_line: f.base_line,
            char_padding: f.char_padding,
            kern_amount: f.kern_amount,
            color: f.color,
            atlas,
            atlas_size: size(igz, f.atlas)?,
            glyphs: f
                .glyphs
                .iter()
                .map(|g| Glyph {
                    char: g.char,
                    step: g.step,
                    offset: g.offset,
                    size: g.size,
                    uv_min: g.uv_min,
                    uv_max: g.uv_max,
                    kern: [
                        g.kern.top_left,
                        g.kern.top_right,
                        g.kern.bottom_left,
                        g.kern.bottom_right,
                    ],
                })
                .collect(),
        });
    }
    let mut templates = Vec::with_capacity(ui.templates.len());
    for t in &ui.templates {
        let label = t
            .name
            .map_or_else(|| format!("#{}", t.object), str::to_owned);
        let mut art = |a: &FArt<'_>| -> Art {
            let mut out = Art {
                source: a.source.map(str::to_owned),
                texture: None,
                size: None,
                stream_archive_hash: a.stream_archive_hash,
                desired: a.desired,
            };
            let got = match (a.image, a.stream_archive_hash, &arc, a.source) {
                (Some(image), ..) => textures
                    .path(igz, image)
                    .map_err(|e| e.to_string())
                    .and_then(|p| Ok((p, size(igz, image)?))),
                (None, hash, Some(arc), Some(source)) if hash != 0 => {
                    streamed(arc, source).map(|(p, ktx2, size)| {
                        if !extra.iter().any(|(e, _)| *e == p) {
                            extra.push((p.clone(), ktx2));
                        }
                        (p, size)
                    })
                }
                _ => return out,
            };
            match got {
                Ok((p, s)) => {
                    out.texture = Some(p);
                    out.size = Some(s);
                }
                Err(why) => refused.push((label.clone(), why)),
            }
            out
        };
        let content = match &t.content {
            FContent::Container => Content::Container,
            FContent::Empty { kind: k } => Content::Empty(kind(*k).ok_or("a visual type past 4")?),
            FContent::Image(a) => Content::Image(art(a)),
            FContent::Movie(a) => {
                // A movie plays from its own file, not an image.
                Content::Movie(Art {
                    source: a.source.map(str::to_owned),
                    texture: None,
                    size: None,
                    stream_archive_hash: a.stream_archive_hash,
                    desired: a.desired,
                })
            }
            FContent::Text { face } => Content::Text(*face),
            FContent::Model { path } => Content::Model(path.map(str::to_owned)),
            FContent::Emitter { name } => Content::Emitter(name.map(str::to_owned)),
            FContent::Imported { kind: k, id } => {
                Content::Imported(kind(*k).ok_or("a visual type past 4")?, import(*id))
            }
        };
        templates.push(Template {
            name: label,
            object: t.object as u32,
            properties: t.bits.to_word(),
            flags: t.flags,
            position: t.position,
            z_depth: t.z_depth,
            priority: t.priority,
            width: t.width,
            height: t.height,
            scale: t.scale,
            color: t.color,
            replace_color: t.replace_color,
            num_value: t.num_value,
            depth: t.depth,
            init_matrix: matrix(t.init_matrix),
            matrix: matrix(t.matrix),
            content,
            string: t.string.map(|k| match k {
                TextKey::Local(n) => guac_ir::interface::TextKey::Local(n),
                TextKey::Imported(id) => guac_ir::interface::TextKey::Imported(import(id)),
            }),
        });
    }
    let mut out = textures.files;
    for (p, ktx2) in extra {
        if !out.iter().any(|(o, _)| *o == p) {
            out.push((p, ktx2));
        }
    }
    Ok(Read {
        ir: Interface {
            identity,
            archive: archive.to_owned(),
            templates,
            fonts,
            strings,
            refused,
        },
        textures: out,
    })
}

/// What the part did.
#[derive(Debug, Clone, Default)]
pub struct InterfaceStage {
    /// The part was in already, and kept.
    pub kept: bool,
    /// Read off the dump this run rather than kept.
    pub read: bool,
    /// The archive that stopped the part, and why.
    pub failed: Option<(String, String)>,
    /// Archives the part reads that the dump lacks.
    pub missing: Vec<String>,
    /// Archives that read, and their templates, fonts and strings.
    pub archives: usize,
    pub templates: usize,
    pub fonts: usize,
    pub strings: usize,
    /// Distinct textures the part's archives draw.
    pub textures: usize,
    /// What didn't extract, by archive.
    pub refused: Vec<(String, String)>,
    /// Of [`InterfaceStage::files`], the textures'.
    pub texture_files: Files,
    pub files: Files,
    pub took: Duration,
}

/// The part, unless the extract already holds it
/// ([`IdentityDir::part_holds`]): each archive's interface into
/// `interface/<stem>.<ron|postcard>` and its textures into the store,
/// recorded once every archive's is in. A file that won't write stops the
/// run. Its progress goes out as [`Event::Interface`]: none of the archives
/// the dump holds done, then each as it lands (all at once when the part is
/// kept), openspec AT.26.
pub fn interface_stage(
    dir: &mut IdentityDir,
    ir: IrFormat,
    send: &(dyn Fn(Event) + Sync),
) -> Result<InterfaceStage> {
    let started = Instant::now();
    let usrdir = PathBuf::from(&dir.manifest().source_dir).join("PS3_GAME/USRDIR");
    let identity = dir.manifest().identity.clone();
    let mut stage = InterfaceStage::default();
    let (present, missing): (Vec<String>, Vec<String>) = archives(dir.manifest())
        .into_iter()
        .partition(|a| usrdir.join(a).is_file());
    stage.missing = missing;
    let total = present.len();
    send(Event::Interface { done: 0, total });
    let irs: Vec<String> = present.iter().map(|a| path(ir, a)).collect();
    if dir.part_holds(PART, &irs)? {
        let recorded = dir.manifest().parts[PART].clone();
        count_kept(dir, &recorded, &mut stage.files);
        stage.kept = true;
        stage.took = started.elapsed();
        send(Event::Interface { done: total, total });
        return Ok(stage);
    }
    let mut files: Vec<String> = Vec::new();
    let mut counted: HashSet<String> = HashSet::new();
    for (i, (archive, rel)) in present.iter().zip(&irs).enumerate() {
        let r = match read_archive(&usrdir, archive, identity.clone()) {
            Ok(r) => r,
            Err(ExtractError::ScriptsUnreadable { reason, .. }) => {
                let e = ExtractError::ScriptsUnreadable {
                    part: PART.to_owned(),
                    archive: archive.clone(),
                    reason,
                };
                stage.failed = Some((archive.clone(), e.to_string()));
                stage.took = started.elapsed();
                return Ok(stage);
            }
            Err(e) => return Err(e),
        };
        let mut new: Vec<(String, &[u8])> = Vec::new();
        for (file, ktx2) in &r.textures {
            if counted.insert(file.clone()) {
                files.push(file.clone());
                if !held(dir, file, ktx2, &mut stage.texture_files)? {
                    new.push((file.clone(), ktx2));
                }
            }
        }
        // The archive's new textures under one manifest save (AS.26).
        dir.write_outputs(&new)?;
        let bytes = ir.encode(&r.ir, dir, rel)?;
        put(dir, rel, &bytes, &mut stage.files)?;
        files.push(rel.clone());
        stage.archives += 1;
        stage.templates += r.ir.templates.len();
        stage.fonts += r.ir.fonts.len();
        stage.strings += r.ir.strings.len();
        stage.refused.extend(
            r.ir.refused
                .iter()
                .map(|(n, why)| (format!("{archive}: {n}"), why.clone())),
        );
        send(Event::Interface { done: i + 1, total });
    }
    stage.textures = counted.len();
    // `held` counted each texture's bytes once, kept or written.
    stage.files.written += stage.texture_files.written;
    stage.files.kept += stage.texture_files.kept;
    stage.files.bytes += stage.texture_files.bytes;
    dir.record_part(PART, files)?;
    stage.read = true;
    stage.took = started.elapsed();
    Ok(stage)
}

/// One archive off the dump, with its `.arc` when a character archive has
/// one. An archive that won't read is [`ExtractError::ScriptsUnreadable`]
/// (the stage names the part); one that won't open is an I/O error.
fn read_archive(usrdir: &Path, archive: &str, identity: DumpIdentity) -> Result<Read> {
    let bld_path = usrdir.join(archive);
    let bld = std::fs::read(&bld_path).map_err(ExtractError::io(&bld_path))?;
    let arc = if archive.starts_with(models::CHARACTER_DIR) {
        let arc_path = bld_path.with_extension("arc");
        match std::fs::read(&arc_path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(ExtractError::io(&arc_path)(e)),
        }
    } else {
        None
    };
    read(archive, &bld, arc.as_deref(), identity).map_err(|reason| {
        ExtractError::ScriptsUnreadable {
            part: PART.to_owned(),
            archive: archive.to_owned(),
            reason,
        }
    })
}

#[cfg(test)]
mod tests;
