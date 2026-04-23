//! PLAN 6.6.2 — one-shot migration from SHA-of-path `FigureId` to the
//! canonical tag-identity form `"{toy_type:06x}-{variant:04x}"`.
//!
//! Usage:
//!   `cargo run -p skylander-rekey-figure-ids`                     — uses
//!       `FIRMWARE_PACK_ROOT` from `.env.dev` (reads `.env.dev` directly;
//!       no dep on the config crate) and writes to `./data/`.
//!   `cargo run -p skylander-rekey-figure-ids -- --pack <dir>`
//!   `cargo run -p skylander-rekey-figure-ids -- --data <dir>`
//!   `cargo run -p skylander-rekey-figure-ids -- --dry-run`        — report
//!       what would change without touching disk.
//!
//! The tool uses the *indexer* to compute both old (SHA) and new (tag-id)
//! identifiers for each pack `.sky` in one pass — `Figure.id` holds the
//! old SHA, `Figure.tag_identity` the new canonical form (populated by
//! 6.6.1d). That gives us the SHA→tag-id map without re-implementing
//! `classify()` + `stable_id()`.
//!
//! Collisions are expected (15 on Chris's pack — Sidekicks duplicated at
//! both the top level and under Giants/); first-wins by indexer walk
//! order. The loser's image dir gets renamed only if its target isn't
//! already claimed; otherwise it's left in place and noted in the log.
//!
//! Idempotent: on rerun, entries whose `figure_id` already matches the
//! new format are left alone.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use skylander_core::FigureId;
use tracing::{info, warn};

#[derive(Debug)]
struct Args {
    pack: Option<PathBuf>,
    data: PathBuf,
    dry_run: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = parse_args()?;

    let pack = match args.pack.clone() {
        Some(p) => p,
        None => read_dotenv_dev_firmware_pack()?
            .context("--pack not set and FIRMWARE_PACK_ROOT missing from .env.dev")?,
    };
    if !pack.is_dir() {
        bail!("pack path doesn't exist or isn't a dir: {}", pack.display());
    }
    if !args.data.is_dir() {
        bail!("data path doesn't exist or isn't a dir: {}", args.data.display());
    }
    let figures_json_path = args.data.join("figures.json");
    let images_dir = args.data.join("images");

    if args.dry_run {
        info!("--dry-run: no files will be touched");
    }

    info!(pack = %pack.display(), data = %args.data.display(), "starting rekey");

    // --- Step 1: build SHA → tag-id map from the pack walk. ------------
    let figures = skylander_indexer::scan(&pack).context("scan pack")?;
    info!(count = figures.len(), "indexed pack");

    let mut sha_to_new: BTreeMap<String, String> = BTreeMap::new();
    let mut unknown_pack_figures: Vec<String> = Vec::new();
    for f in &figures {
        let old_id = f.id.as_str().to_string();
        match f.tag_identity {
            Some(tid) => {
                sha_to_new.insert(old_id, tid.to_canonical_id_string());
            }
            None => {
                warn!(
                    id = %old_id,
                    path = %f.sky_path.display(),
                    "no tag_identity on indexer output — parse failed, keeping SHA in place",
                );
                unknown_pack_figures.push(old_id);
            }
        }
    }
    info!(
        known = sha_to_new.len(),
        unknown = unknown_pack_figures.len(),
        "SHA → tag-id map"
    );

    // --- Step 2: figures.json. -----------------------------------------
    let raw = fs::read_to_string(&figures_json_path)
        .with_context(|| format!("read {}", figures_json_path.display()))?;
    let mut entries: Vec<serde_json::Value> =
        serde_json::from_str(&raw).context("parse figures.json (expected JSON array)")?;

    // Idempotency: if every entry's `figure_id` already matches the new
    // canonical form `{6-hex}-{4-hex}`, skip the JSON rewrite but still
    // run the image-rename pass — the previous invocation may have
    // crashed between phases, leaving a mixed-state `data/images/`.
    let figures_json_already_migrated = entries.iter().all(|e| {
        e.get("figure_id")
            .and_then(|v| v.as_str())
            .map(looks_like_new_id)
            .unwrap_or(false)
    });
    if figures_json_already_migrated {
        info!(
            count = entries.len(),
            "figures.json already in new format — will only reconcile images",
        );
    }

    // Rewrite keys; track first-wins winners and the ones we dropped.
    let mut winners: HashMap<String, (String, usize)> = HashMap::new(); // new_id → (old_sha, kept_index)
    let mut collisions: Vec<CollisionLog> = Vec::new();
    let mut kept: Vec<serde_json::Value> = Vec::with_capacity(entries.len());

    for mut entry in entries.drain(..) {
        let old_id = entry
            .get("figure_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        if old_id.is_empty() {
            warn!(?entry, "figures.json entry missing figure_id — dropping");
            continue;
        }
        // Already new? (partial-migration scenario) — carry through as-is.
        if looks_like_new_id(&old_id) {
            if winners.contains_key(&old_id) {
                collisions.push(CollisionLog {
                    new_id: old_id.clone(),
                    winner_old_sha: winners[&old_id].0.clone(),
                    loser_old_sha: "<already-new>".into(),
                });
            } else {
                winners.insert(old_id.clone(), (old_id.clone(), kept.len()));
                kept.push(entry);
            }
            continue;
        }
        let Some(new_id) = sha_to_new.get(&old_id) else {
            warn!(
                old = %old_id,
                "figures.json entry has no matching pack file — dropping (stale scrape?)",
            );
            continue;
        };
        if let Some((prev_sha, _)) = winners.get(new_id) {
            collisions.push(CollisionLog {
                new_id: new_id.clone(),
                winner_old_sha: prev_sha.clone(),
                loser_old_sha: old_id.clone(),
            });
            continue;
        }
        entry["figure_id"] = serde_json::Value::String(new_id.clone());
        winners.insert(new_id.clone(), (old_id, kept.len()));
        kept.push(entry);
    }

    info!(
        kept = kept.len(),
        collisions = collisions.len(),
        "figures.json rewrite plan ready"
    );
    for c in &collisions {
        info!(
            new_id = %c.new_id,
            winner = %c.winner_old_sha,
            loser = %c.loser_old_sha,
            "collision: loser dropped"
        );
    }

    // --- Step 3: image-dir renames. ------------------------------------
    // Planning loop tracks which new_id destinations have already been
    // claimed — a collision loser whose winner ALSO had an image dir
    // would otherwise plan to rename into a not-yet-existing-on-disk
    // destination during planning (passes `new_dir.exists()` check) but
    // then collide at execute-time once the winner has been renamed.
    let mut image_renames: Vec<RenameLog> = Vec::new();
    let mut claimed_destinations: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    if images_dir.is_dir() {
        for (old_sha, new_id) in &sha_to_new {
            let old_dir = images_dir.join(old_sha);
            let new_dir = images_dir.join(new_id);
            if !old_dir.is_dir() {
                // Missing source is fine — not every pack entry has a
                // scraped image (e.g. figures with the wiki lookup
                // skipped).
                continue;
            }
            if new_dir.exists() || claimed_destinations.contains(new_id) {
                // Collision at the destination (another SHA already
                // mapped here and either got renamed first or is
                // already in its canonical spot). Leave the loser's
                // dir in place untouched; the orphan is preserved for
                // forensics and nothing consumes it.
                image_renames.push(RenameLog {
                    old: old_sha.clone(),
                    new: new_id.clone(),
                    outcome: RenameOutcome::SkippedDestExists,
                });
                continue;
            }
            claimed_destinations.insert(new_id.clone());
            image_renames.push(RenameLog {
                old: old_sha.clone(),
                new: new_id.clone(),
                outcome: RenameOutcome::Renamed,
            });
        }
    } else {
        info!(dir = %images_dir.display(), "no images dir — skipping renames");
    }
    info!(
        planned = image_renames.iter().filter(|r| matches!(r.outcome, RenameOutcome::Renamed)).count(),
        skipped = image_renames.iter().filter(|r| matches!(r.outcome, RenameOutcome::SkippedDestExists)).count(),
        "image-rename plan"
    );

    // --- Step 4: write outputs. ----------------------------------------
    if args.dry_run {
        info!("dry-run — skipping all writes");
    } else {
        if figures_json_already_migrated {
            info!(
                path = %figures_json_path.display(),
                "figures.json already migrated; not rewriting"
            );
        } else {
            let pretty = serde_json::to_string_pretty(&kept)?;
            fs::write(&figures_json_path, pretty).context("write figures.json")?;
            info!(path = %figures_json_path.display(), "wrote figures.json");
        }

        for r in &image_renames {
            if !matches!(r.outcome, RenameOutcome::Renamed) {
                continue;
            }
            let old_dir = images_dir.join(&r.old);
            let new_dir = images_dir.join(&r.new);
            fs::rename(&old_dir, &new_dir)
                .with_context(|| format!("rename {} → {}", old_dir.display(), new_dir.display()))?;
        }
        info!("renamed image dirs");

        let log_path = args.data.join("rekey-log.json");
        let log = RekeyLog {
            sha_to_new: sha_to_new
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            collisions,
            image_renames,
            unknown_pack_figures,
            dropped_entries_without_pack_match: 0, // filled above via warn-and-skip — counted by log line
        };
        fs::write(&log_path, serde_json::to_string_pretty(&log)?)
            .context("write rekey-log.json")?;
        info!(path = %log_path.display(), "wrote rekey-log.json");
    }

    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        pack: None,
        data: PathBuf::from("./data"),
        dry_run: false,
    };
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--pack" => {
                i += 1;
                args.pack = Some(PathBuf::from(
                    raw.get(i).context("--pack requires an argument")?,
                ));
            }
            "--data" => {
                i += 1;
                args.data = PathBuf::from(raw.get(i).context("--data requires an argument")?);
            }
            "--dry-run" => args.dry_run = true,
            "--help" | "-h" => {
                eprintln!(
                    "{}",
                    "usage: skylander-rekey-figure-ids [--pack <dir>] [--data <dir>] [--dry-run]"
                );
                std::process::exit(0);
            }
            other => bail!("unknown arg: {}", other),
        }
        i += 1;
    }
    Ok(args)
}

/// Read `.env.dev` at the repo root (cwd) and return the `FIRMWARE_PACK_ROOT`
/// value, if present. Returns Ok(None) if the file or key is missing.
fn read_dotenv_dev_firmware_pack() -> Result<Option<PathBuf>> {
    let raw = match fs::read_to_string(".env.dev") {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("FIRMWARE_PACK_ROOT=") {
            return Ok(Some(PathBuf::from(rest.trim())));
        }
    }
    Ok(None)
}

/// Pattern check: canonical `FigureId` post-6.6 is `"{6-hex}-{4-hex}"`. Raw
/// check rather than a FigureId parse because we want to accept the `sha:`
/// and `scan:` prefixes as "not a pack tag-id, pass through untouched"
/// further up.
fn looks_like_new_id(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 6 + 1 + 4 {
        return false;
    }
    if bytes[6] != b'-' {
        return false;
    }
    bytes.iter().enumerate().all(|(i, &b)| {
        if i == 6 {
            b == b'-'
        } else {
            b.is_ascii_hexdigit() && !b.is_ascii_uppercase()
        }
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct CollisionLog {
    new_id: String,
    winner_old_sha: String,
    loser_old_sha: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RenameLog {
    old: String,
    new: String,
    outcome: RenameOutcome,
}

#[derive(Debug, Serialize, Deserialize)]
enum RenameOutcome {
    Renamed,
    SkippedDestExists,
}

#[derive(Debug, Serialize, Deserialize)]
struct RekeyLog {
    sha_to_new: Vec<(String, String)>,
    collisions: Vec<CollisionLog>,
    image_renames: Vec<RenameLog>,
    unknown_pack_figures: Vec<String>,
    /// Kept for forward compat; not currently populated because we warn-
    /// and-skip inline above rather than accumulating.
    dropped_entries_without_pack_match: usize,
}

/// Use the shared core newtype so we link against it and so a future refactor
/// of the canonical-id format only needs one touch. Not a real consumer; just
/// keeps the dep honest.
#[allow(dead_code)]
fn _figure_id_touch(s: String) -> FigureId {
    FigureId::new(s)
}
