//! Writes a draw's buffers and JSON description, and refuses any output dir inside this repo.
//! Captured bytes are game data, which the repo never holds (openspec config, HARD RULE).

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::replay::{Draw, ReplayStats};

/// Substring of this repo's remote URL; a clone anywhere else is still this repo.
const REPO_NAME: &str = "skylander-portal-controller";

/// Canonical path of the git dir shared by every worktree of the repo whose work tree holds `root`.
fn common_git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let text = fs::read_to_string(&dot_git).ok()?;
        let target = PathBuf::from(text.strip_prefix("gitdir:")?.trim());
        if target.is_absolute() {
            target
        } else {
            root.join(target)
        }
    };
    let common = match fs::read_to_string(git_dir.join("commondir")) {
        Ok(rel) => git_dir.join(rel.trim()),
        Err(_) => git_dir,
    };
    common.canonicalize().ok()
}

fn enclosing_work_tree(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|a| a.join(".git").exists())
        .map(Path::to_path_buf)
}

/// This crate's own repo, when the binary runs on the machine that built it.
fn build_repo_common_dir() -> Option<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    common_git_dir(&enclosing_work_tree(manifest)?)
}

/// A work tree of this repo: the build checkout, any of its worktrees, or another clone whose
/// remote names it.
fn is_this_repo(root: &Path) -> bool {
    let common = common_git_dir(root);
    let same_checkout = common.is_some() && common == build_repo_common_dir();
    let remote_names_repo = common
        .and_then(|c| fs::read_to_string(c.join("config")).ok())
        .is_some_and(|config| {
            config.lines().any(|l| {
                let l = l.trim();
                l.starts_with("url") && l.contains(REPO_NAME)
            })
        });
    same_checkout || remote_names_repo
}

/// Resolves `out` to the physical path to write to, erroring when it sits inside any work tree of
/// this repo. Every enclosing work tree is checked, not just the innermost: a submodule or nested
/// repo (`vendor/rpcs3`) still sits inside this checkout. `..` is resolved by the OS through
/// symlinks, never lexically, and refused below the deepest dir that exists.
pub fn refuse_repo_output(out: &Path) -> Result<PathBuf> {
    let abs = std::path::absolute(out)?;
    let existing = abs
        .ancestors()
        .find(|a| a.exists())
        .context("output path has no existing ancestor")?;
    let tail = abs.strip_prefix(existing)?.to_path_buf();
    if tail.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!(
            "refusing an output path with `..` below its deepest existing dir: {}",
            out.display()
        );
    }
    let target = existing.canonicalize()?.join(&tail);
    if let Some(root) = target
        .ancestors()
        .filter(|a| a.join(".git").exists())
        .find(|a| is_this_repo(a))
    {
        bail!(
            "refusing to write capture data inside the {REPO_NAME} repo (work tree {}); pick a dir outside it, e.g. $GUAC_CAPTURE_DIR",
            root.display()
        );
    }
    Ok(target)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Writes draws under one output dir, sharing texture files across draws by content.
pub struct DumpWriter {
    root: PathBuf,
    textures: HashMap<(u32, u32, usize, u64), String>,
    summary: Vec<Value>,
}

impl DumpWriter {
    pub fn create(root: &Path) -> Result<Self> {
        let root = refuse_repo_output(root)?;
        fs::create_dir_all(root.join("textures"))
            .with_context(|| format!("creating {}", root.display()))?;
        Ok(DumpWriter {
            root,
            textures: HashMap::new(),
            summary: Vec::new(),
        })
    }

    pub fn write_draw(&mut self, draw: &Draw) -> Result<()> {
        let dir_name = format!("draw_{:05}", draw.draw);
        let dir = self.root.join(&dir_name);
        fs::create_dir_all(&dir)?;
        let mut desc = serde_json::to_value(draw)?;

        for (i, va) in draw.vertex_arrays.iter().enumerate() {
            let file = format!("attr_{:02}.bin", va.slot);
            fs::write(dir.join(&file), &va.bytes)?;
            desc["vertex_arrays"][i]["file"] = json!(file);
            desc["vertex_arrays"][i]["bytes"] = json!(va.bytes.len());
        }
        if let Some(ia) = &draw.index_array {
            fs::write(dir.join("index.bin"), &ia.bytes)?;
            desc["index_array"]["file"] = json!("index.bin");
            desc["index_array"]["bytes"] = json!(ia.bytes.len());
        }
        for (i, tex) in draw.textures.iter().enumerate() {
            if tex.bytes.is_empty() {
                continue;
            }
            let key = (
                tex.location,
                tex.offset,
                tex.bytes.len(),
                fnv1a64(&tex.bytes),
            );
            let file = match self.textures.get(&key) {
                Some(f) => f.clone(),
                None => {
                    let f = format!("textures/loc{}_{:08x}_{:016x}.bin", key.0, key.1, key.3);
                    fs::write(self.root.join(&f), &tex.bytes)?;
                    self.textures.insert(key, f.clone());
                    f
                }
            };
            desc["textures"][i]["file"] = json!(format!("../{file}"));
            desc["textures"][i]["bytes"] = json!(tex.bytes.len());
        }

        let constants: Vec<u8> = draw
            .constants
            .values
            .iter()
            .flatten()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        fs::write(dir.join("constants.bin"), constants)?;
        desc["constants"]["file"] = json!("constants.bin");
        desc["constants"]["encoding"] = json!("468 x vec4, f32 little-endian");
        desc["byte_order"] = json!("vertex and index files are raw RSX memory (big-endian)");

        fs::write(dir.join("draw.json"), serde_json::to_vec_pretty(&desc)?)?;
        self.summary.push(json!({
            "draw": draw.draw,
            "dir": dir_name,
            "primitive": draw.primitive_name,
            "kind": draw.kind,
            "vertex_count": draw.vertex_count,
            "index_count": draw.index_array.as_ref().map(|ia| ia.index_count),
            "vertex_arrays": draw.vertex_arrays.len(),
            "textures": draw.textures.iter().map(|t| t.unit).collect::<Vec<_>>(),
        }));
        Ok(())
    }

    pub fn finish(self, source: &Path, stats: &ReplayStats) -> Result<()> {
        let capture = json!({
            "source": source.display().to_string(),
            "rrc_version": crate::capture::SUPPORTED_VERSION,
            "stats": stats,
            "draws": self.summary,
        });
        fs::write(
            self.root.join("capture.json"),
            serde_json::to_vec_pretty(&capture)?,
        )?;
        Ok(())
    }
}
