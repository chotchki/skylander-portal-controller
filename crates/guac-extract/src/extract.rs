//! The per-identity extract dir and its `manifest.json` (design decision 5):
//! `<root>/<TITLE_ID>-<APP_VER>-<hash8>/`. Every file in it, the manifest
//! included, is written to `<file>.partial` and renamed when complete.

use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use guac_ir::{DumpIdentity, IR_VERSION, Sha256};
use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::LAYOUT_VERSION;
use crate::dump::{self, DumpScan, HashCache, HashProgress, SourceFile};
use crate::error::{ExtractError, Result};
use crate::known::{self, KnownDump};
use crate::report;
use crate::root::{self, ExtractRoot};

pub const MANIFEST: &str = "manifest.json";
const PARTIAL: &str = ".partial";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// guac-extract's crate version that wrote it. Recorded, not compared.
    pub extractor: String,
    pub layout: u32,
    pub ir_version: u32,
    pub identity: DumpIdentity,
    /// The dump folder, as resolved; the hash cache only trusts its own dump.
    pub source_dir: String,
    /// Set once every output is written; until then a start resumes.
    pub complete: bool,
    /// Sorted by path. Doubles as the hash cache for the next start.
    pub sources: Vec<SourceFile>,
    pub outputs: Vec<OutputFile>,
    /// The intro transcode's target and the ffmpeg that ran it (AB.8).
    #[serde(default)]
    pub transcode: Option<TranscodeRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscodeRecord {
    /// [`crate::transcode::Target::tag`]; another tag redoes the intros.
    pub target: String,
    /// ffmpeg's version line. Recorded, not compared.
    pub ffmpeg: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputFile {
    /// Relative to the identity dir, `/`-separated.
    pub path: String,
    pub size: u64,
    pub sha256: Sha256,
}

/// Read before the full manifest, so a shape change still reads as stale.
#[derive(Deserialize)]
struct VersionProbe {
    layout: u32,
    ir_version: u32,
}

/// Why an identity dir was thrown away and rebuilt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stale {
    Layout {
        found: u32,
    },
    IrVersion {
        found: u32,
    },
    /// No manifest, or one that doesn't parse or names another dump.
    Unusable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirState {
    /// No identity dir existed.
    Fresh,
    /// A complete extract by this layout and IR version: nothing to do.
    Current,
    /// An interrupted extract: outputs already done are kept.
    Resume,
    /// A stale or unusable dir was deleted and started over.
    Rebuilt(Stale),
}

#[derive(Debug)]
pub struct Opened {
    pub dir: IdentityDir,
    pub state: DirState,
    pub files_hashed: usize,
    pub files_cached: usize,
    pub bytes_hashed: u64,
}

#[derive(Debug)]
pub struct IdentityDir {
    path: PathBuf,
    manifest: Manifest,
}

/// `<TITLE_ID>-<APP_VER>-<first 8 hex of the manifest hash>`.
pub fn dir_name(id: &DumpIdentity) -> String {
    format!(
        "{}-{}-{}",
        id.title_id,
        id.app_ver,
        &id.manifest_hash.to_string()[..8]
    )
}

/// Identifies the dump and, when it's known, opens or makes its identity dir.
/// An unknown dump writes its report beside `root` and extracts nothing.
pub fn open(
    dump_dir: &Path,
    root: &ExtractRoot,
    known: &[KnownDump],
    progress: &(dyn Fn(HashProgress) + Sync),
) -> Result<Opened> {
    // Again at use: the root may have been resolved long before.
    if let Some(repo) = root::enclosing_git_tree(root.path()) {
        return Err(ExtractError::InCheckout {
            root: root.path().to_path_buf(),
            repo,
        });
    }
    root::check_disjoint(root, dump_dir)?;
    let dump_dir = dump_dir
        .canonicalize()
        .map_err(|e| ExtractError::unreadable(dump_dir, e))?;
    let source_dir = dump_dir.to_string_lossy().into_owned();

    let (title_id, app_ver) = dump::read_sfo(&dump_dir)?;
    let cache = load_cache(root.path(), &title_id, &app_ver, &source_dir);
    let scan = dump::scan(&dump_dir, &cache, progress)?;
    if known::lookup(known, &scan.identity).is_none() {
        let report = report::write(root.report_dir(), &dump_dir, &scan, known)?;
        return Err(ExtractError::UnknownDump {
            identity: scan.identity,
            report,
        });
    }
    let DumpScan {
        identity,
        files,
        bytes_hashed,
        files_cached,
    } = scan;
    let files_hashed = files.len() - files_cached;

    let path = root.path().join(dir_name(&identity));
    let existing = if path.exists() {
        Some(read_manifest(&path, &identity))
    } else {
        None
    };
    let (state, mut manifest) = match existing {
        None => (DirState::Fresh, None),
        // Complete, unless the intros were transcoded for another target.
        Some(Ok(m))
            if m.complete
                && m.transcode
                    .as_ref()
                    .is_none_or(|t| t.target == crate::transcode::TARGET.tag()) =>
        {
            (DirState::Current, Some(m))
        }
        Some(Ok(m)) => (DirState::Resume, Some(m)),
        Some(Err(stale)) => {
            fs::remove_dir_all(&path).map_err(ExtractError::io(&path))?;
            (DirState::Rebuilt(stale), None)
        }
    };
    fs::create_dir_all(&path).map_err(ExtractError::io(&path))?;
    if state == DirState::Resume {
        sweep_partials(&path)?;
    }
    let manifest = match manifest.take() {
        Some(mut m) => {
            m.source_dir = source_dir;
            m.sources = files;
            m.complete = state == DirState::Current;
            m
        }
        None => Manifest {
            extractor: env!("CARGO_PKG_VERSION").into(),
            layout: LAYOUT_VERSION,
            ir_version: IR_VERSION,
            identity,
            source_dir,
            complete: false,
            sources: files,
            outputs: Vec::new(),
            transcode: None,
        },
    };
    let dir = IdentityDir { path, manifest };
    dir.save()?;
    Ok(Opened {
        dir,
        state,
        files_hashed,
        files_cached,
        bytes_hashed,
    })
}

fn read_manifest(dir: &Path, id: &DumpIdentity) -> Result<Manifest, Stale> {
    let text = fs::read_to_string(dir.join(MANIFEST)).map_err(|_| Stale::Unusable)?;
    let probe: VersionProbe = serde_json::from_str(&text).map_err(|_| Stale::Unusable)?;
    if probe.layout != LAYOUT_VERSION {
        return Err(Stale::Layout {
            found: probe.layout,
        });
    }
    if probe.ir_version != IR_VERSION {
        return Err(Stale::IrVersion {
            found: probe.ir_version,
        });
    }
    let m: Manifest = serde_json::from_str(&text).map_err(|_| Stale::Unusable)?;
    if &m.identity != id {
        return Err(Stale::Unusable);
    }
    Ok(m)
}

/// Source records from this dump's earlier manifests under `root`, any
/// identity: a dump that changed since still re-reads only what changed.
fn load_cache(root: &Path, title_id: &str, app_ver: &str, source_dir: &str) -> HashCache {
    let mut cache = HashCache::default();
    if !known::path_safe(title_id) || !known::path_safe(app_ver) {
        return cache;
    }
    let prefix = format!("{title_id}-{app_ver}-");
    let Ok(entries) = fs::read_dir(root) else {
        return cache;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_str().is_some_and(|n| n.starts_with(&prefix)) {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path().join(MANIFEST)) else {
            continue;
        };
        if let Ok(m) = serde_json::from_str::<Manifest>(&text)
            && m.source_dir == source_dir
        {
            cache.extend(m.sources);
        }
    }
    cache
}

fn sweep_partials(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(ExtractError::io(dir))? {
        let path = entry.map_err(ExtractError::io(dir))?.path();
        if path.is_dir() {
            sweep_partials(&path)?;
        } else if path.to_str().is_some_and(|p| p.ends_with(PARTIAL)) {
            fs::remove_file(&path).map_err(ExtractError::io(&path))?;
        }
    }
    Ok(())
}

fn partial_of(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(PARTIAL);
    PathBuf::from(p)
}

fn hash_path(path: &Path) -> Result<(u64, Sha256)> {
    let mut f = fs::File::open(path).map_err(ExtractError::io(path))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(ExtractError::io(path))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((size, Sha256(hasher.finalize().into())))
}

impl IdentityDir {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// `rel` as a path in this dir, refused unless it's plain relative names.
    fn output_path(&self, rel: &str) -> Result<PathBuf> {
        let bad = || ExtractError::BadOutputPath { path: rel.into() };
        let p = Path::new(rel);
        let plain = !rel.is_empty()
            && !rel.contains('\\')
            && p.components().all(|c| matches!(c, Component::Normal(_)))
            && rel != MANIFEST
            && !rel.ends_with(PARTIAL);
        if !plain {
            return Err(bad());
        }
        Ok(self.path.join(p))
    }

    /// Where a writer (ffmpeg, say) puts `rel` before [`Self::commit`]; parent
    /// dirs are made.
    pub fn partial_path(&self, rel: &str) -> Result<PathBuf> {
        let path = self.output_path(rel)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ExtractError::io(parent))?;
        }
        Ok(partial_of(&path))
    }

    /// Renames `rel`'s finished `.partial` into place and records it.
    pub fn commit(&mut self, rel: &str) -> Result<OutputFile> {
        let path = self.output_path(rel)?;
        let partial = partial_of(&path);
        let (size, sha256) = hash_path(&partial)?;
        fs::rename(&partial, &path).map_err(ExtractError::io(&path))?;
        let out = OutputFile {
            path: rel.into(),
            size,
            sha256,
        };
        self.manifest.outputs.retain(|o| o.path != rel);
        self.manifest.outputs.push(out.clone());
        self.manifest.outputs.sort_by(|a, b| a.path.cmp(&b.path));
        self.save()?;
        Ok(out)
    }

    pub fn write_output(&mut self, rel: &str, bytes: &[u8]) -> Result<OutputFile> {
        let partial = self.partial_path(rel)?;
        write_file(&partial, bytes)?;
        self.commit(rel)
    }

    /// Recorded, present, and its bytes still hash to the record.
    pub fn output_done(&self, rel: &str) -> Result<bool> {
        let path = self.output_path(rel)?;
        let Some(rec) = self.manifest.outputs.iter().find(|o| o.path == rel) else {
            return Ok(false);
        };
        if !path.is_file() {
            return Ok(false);
        }
        Ok(hash_path(&path)? == (rec.size, rec.sha256))
    }

    /// Hard-links (or copies) the done output `from` to `to` and records it.
    pub(crate) fn link_output(&mut self, from: &str, to: &str) -> Result<OutputFile> {
        let src = self.output_path(from)?;
        let partial = self.partial_path(to)?;
        if partial.exists() {
            fs::remove_file(&partial).map_err(ExtractError::io(&partial))?;
        }
        if fs::hard_link(&src, &partial).is_err() {
            fs::copy(&src, &partial).map_err(ExtractError::io(&partial))?;
        }
        self.commit(to)
    }

    /// Drops every output under `prefix` (`video/intro/`), record and file.
    pub(crate) fn forget_outputs(&mut self, prefix: &str) -> Result<()> {
        let (gone, kept) = std::mem::take(&mut self.manifest.outputs)
            .into_iter()
            .partition(|o| o.path.starts_with(prefix));
        self.manifest.outputs = kept;
        let gone: Vec<OutputFile> = gone;
        for o in gone {
            let path = self.output_path(&o.path)?;
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(ExtractError::io(&path)(e)),
            }
        }
        self.save()
    }

    pub(crate) fn set_transcode(&mut self, record: TranscodeRecord) -> Result<()> {
        self.manifest.transcode = Some(record);
        self.save()
    }

    pub fn mark_complete(&mut self) -> Result<()> {
        self.manifest.complete = true;
        self.save()
    }

    fn save(&self) -> Result<()> {
        let mut text =
            serde_json::to_string_pretty(&self.manifest).expect("the manifest serializes");
        text.push('\n');
        let path = self.path.join(MANIFEST);
        let partial = partial_of(&path);
        write_file(&partial, text.as_bytes())?;
        fs::rename(&partial, &path).map_err(ExtractError::io(&path))
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = fs::File::create(path).map_err(ExtractError::io(path))?;
    f.write_all(bytes).map_err(ExtractError::io(path))?;
    f.sync_all().map_err(ExtractError::io(path))
}
