//! Every character swept in the oracle (openspec AE.22): each roster figure
//! with a character model placed, one per boot, over AF.8's gameplay state, and
//! what the game then holds for it read at a pause: the model it draws, what
//! it has attached to the hero and where (by bone or node name), the hero's
//! scale, `CollisionInfo`, `ActorParameters` and solid, beside a `SCREENSHOT`.
//! AE.23 takes its rule for what a fresh figure shows from the table this
//! writes, and AG.27 checks every character's static tuning against it.
//!
//! The figure list ([`list`]) comes off the disc (the roster at
//! `GUAC_GIANTS_DUMP`) and the dev's pack (each figure's `.sky`, read only:
//! every boot gets a fresh copy, and the pack is hashed before and after).
//!
//! One figure's boot ([`run`]): a fresh RPCS3 from the state to Start Paused's
//! pause (F0) and the actors read there; P5's `RECONNECT` and the copy through
//! P1's `LOAD` (the portal comes back from a restore empty); the run resumed,
//! and `RECONNECT` again until the game queries the portal (one more did it in
//! AE.22's held boots, where F0's alone left the guest's USB thread on its
//! stale handle); then, at pauses half a second apart, until RPCS3's log shows
//! the figure's character archive opened and AF.9's reader has a new hero (for
//! the state's own character, Eruptor's figures, the hero counts 300 flips
//! after the queries, archive or not: the game swapped Eruptor for himself and
//! reopened his in AE.22's sweep, but nothing requires it), pressing CROSS when the archive is open and three
//! polls in a row find no hero at all (a prompt holding the swap: the pack's
//! Eye-Brawl is "visiting from another adventure", and the game asks about
//! visiting nicknames with NO highlighted); then [`SETTLE`] flips more, the
//! reads ([`attach`], [`crate::hero`], every actor) and the shot.
//!
//! The run ([`run_sweep`]): one figure per boot, `gap` after the last
//! RPCS3's exit (AF.12's CoreMIDI hang), silent (the home's template), a
//! figure tried `attempts` times and listed when every try fails. The game
//! saves when a new figure arrives (`savedata/BLUS30968`'s `E_HEADER` and
//! `E_SLOT_1` in AE.22's held boots), and a state's load doesn't put the home's
//! saves back, so they're copied out at the start and put back before every
//! boot that would find them changed: every figure boots the same home. A
//! figure whose `figure.json` is already in the output is kept, so a run that
//! stopped picks up where it was. Out: `<out>/<label>/` per figure (`figure.json`,
//! `hero.png`, `actors.json`, RPCS3's log), `figures.json` (the list, figures
//! with no `.sky` among it), `summary.json` and `summary.txt` (the table and its
//! counts), `run.txt`; all of it game data, so never inside a work tree.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use guac_ir::roster::{Category, FigureKind, Roster};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::divergence::{Host, carried_dirs};
use crate::home::{self, OracleHome};
use crate::identity::Identity;
use crate::outputs::hex;
use crate::snapshot::{Snapshot, dev_rpcs3_dirs};

pub mod attach;
pub mod list;
mod run;

pub use list::{PackSky, Picked, SweepFigure, figures, reads_as, scan_pack};
pub use run::{
    ACTORS_FILE, BOOT_WAIT, FigureRecord, Inputs, Opened, RECORD_FILE, SETTLE, SHOT_FILE,
    run_figure,
};

/// The roster on the dump at `dump`, read as guac-extract reads it.
pub fn roster(dump: &Path) -> Result<Roster> {
    let known = &guac_extract::known::KNOWN_DUMPS[0];
    let identity = guac_ir::DumpIdentity {
        title_id: known.title_id.into(),
        app_ver: known.app_ver.into(),
        manifest_hash: guac_ir::Sha256([0; 32]),
    };
    guac_extract::roster::read(dump, identity)
        .with_context(|| format!("the roster on {}", dump.display()))
}

/// How a figure's place in the sweep ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Outcome {
    Swept,
    /// No file in the pack reads as it.
    NoSky,
    /// Every try failed: the last one's error.
    Failed {
        error: String,
    },
}

/// One figure's line in `summary.txt` and entry in `summary.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub load_file: String,
    pub kind: FigureKind,
    pub category: Category,
    /// Pack-relative.
    pub sky: Option<String>,
    pub outcome: Outcome,
    /// The figure's dir under the run's output.
    pub dir: Option<String>,
    /// The hero's placement name.
    pub hero: Option<String>,
    /// The model the game draws for the hero, by file.
    pub model: Option<String>,
    /// Attachments that draw a model, `model@bone`, nested ones included.
    pub attached: Vec<String>,
    /// Attachments that draw none (effects, sounds), labelled the same way.
    pub other_attached: Vec<String>,
    /// Actors naming the hero as owner outside his list.
    pub unlisted: Vec<String>,
    pub scale: Option<[f32; 3]>,
    /// `shape w x h x l ft, weight lb`.
    pub collision: Option<String>,
    /// Ground and air speed, turn rates, motion type.
    pub parameters: Option<String>,
    /// What the reads couldn't settle.
    pub notes: Vec<String>,
}

impl Row {
    pub fn of(figure: &SweepFigure, outcome: Result<&FigureRecord, &str>) -> Self {
        let mut row = Row {
            id: figure.id.clone(),
            name: figure.name.clone(),
            load_file: figure.load_file.clone(),
            kind: figure.kind,
            category: figure.category,
            sky: figure.sky.as_ref().map(|s| s.path.display().to_string()),
            outcome: Outcome::NoSky,
            dir: None,
            hero: None,
            model: None,
            attached: Vec::new(),
            other_attached: Vec::new(),
            unlisted: Vec::new(),
            scale: None,
            collision: None,
            parameters: None,
            notes: Vec::new(),
        };
        let r = match (figure.sky.is_some(), outcome) {
            (false, _) => return row,
            (true, Err(error)) => {
                row.outcome = Outcome::Failed {
                    error: error.to_owned(),
                };
                return row;
            }
            (true, Ok(r)) => r,
        };
        row.outcome = Outcome::Swept;
        row.dir = Some(figure.label());
        row.hero = r.hero.name.clone();
        if !r.hero_is_load_file {
            row.notes
                .push(format!("hero isn't named {}", figure.load_file));
        }
        if r.archive.is_none() && r.f0_hero.as_deref() != Some(figure.load_file.as_str()) {
            row.notes.push("no character archive opened".into());
        }
        if let Some(a) = &r.attachments {
            row.model = a.model.as_ref().and_then(|m| m.file().map(str::to_owned));
            for at in a.all() {
                if at.model.is_some() {
                    row.attached.push(at.label());
                    // Listed under a placement its `_attachNodeOwner` doesn't
                    // name: the list and the back link disagree, so where it
                    // hangs is in doubt. AE.22's 27 such entries are dummies,
                    // effects and lights, none drawing a model.
                    if !at.owned_by_parent {
                        row.notes
                            .push(format!("{} not owned by what lists it", at.label()));
                    }
                } else {
                    row.other_attached.push(at.label());
                }
            }
            row.unlisted = a.unlisted.clone();
        }
        if let Some(e) = &r.attachments_error {
            row.notes.push(format!("attachments unread: {e}"));
        }
        if let Some(t) = &r.tuning {
            row.scale = Some(t.scale);
            row.collision = t.collision.map(|c| {
                let shape = match c.shape() {
                    0 => "none",
                    1 => "box",
                    2 => "capsule",
                    3 => "sphere",
                    4 => "cylinder",
                    5 => "mesh",
                    _ => "shape?",
                };
                format!(
                    "{shape} {} x {} x {} ft, {} lb",
                    c.width, c.height, c.length, c.weight
                )
            });
            row.parameters = t.parameters.map(|p| {
                format!(
                    "ground {} ft/s {} deg/s, air {} ft/s, jump {} ft/s, motion {}",
                    p.ground_max_speed,
                    p.ground_turn_rate,
                    p.air_max_speed,
                    p.jump_initial_speed,
                    p.motion_type
                )
            });
        }
        if let Some(e) = &r.tuning_error {
            row.notes.push(format!("tuning partly read: {e}"));
        }
        if r.presses > 0 {
            row.notes
                .push(format!("{} CROSS to a prompt before the swap", r.presses));
        }
        if r.reconnects > 1 {
            row.notes
                .push(format!("{} RECONNECTs before the queries", r.reconnects));
        }
        row
    }
}

/// Over every row: what was swept, missing or failed, and the swept
/// figures' attachments that draw a model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Counts {
    pub listed: usize,
    pub with_sky: usize,
    pub no_sky: Vec<String>,
    pub swept: usize,
    pub failed: Vec<String>,
    /// Swept figures with at least one attachment that draws a model.
    pub with_models_attached: usize,
    /// Attachments drawing a model over every swept figure.
    pub models_attached: usize,
    /// Swept figures by how many attachments that draw a model they have.
    pub by_count: BTreeMap<usize, usize>,
    /// Swept figures whose attachments couldn't be read.
    pub unread: Vec<String>,
}

impl Counts {
    pub fn of(rows: &[Row]) -> Self {
        let swept: Vec<&Row> = rows
            .iter()
            .filter(|r| r.outcome == Outcome::Swept)
            .collect();
        let mut by_count = BTreeMap::new();
        for r in &swept {
            *by_count.entry(r.attached.len()).or_insert(0) += 1;
        }
        Counts {
            listed: rows.len(),
            with_sky: rows.iter().filter(|r| r.sky.is_some()).count(),
            no_sky: rows
                .iter()
                .filter(|r| r.outcome == Outcome::NoSky)
                .map(|r| r.name.clone())
                .collect(),
            swept: swept.len(),
            failed: rows
                .iter()
                .filter(|r| matches!(r.outcome, Outcome::Failed { .. }))
                .map(|r| r.name.clone())
                .collect(),
            with_models_attached: swept.iter().filter(|r| !r.attached.is_empty()).count(),
            models_attached: swept.iter().map(|r| r.attached.len()).sum(),
            by_count,
            unread: swept
                .iter()
                .filter(|r| r.notes.iter().any(|n| n.starts_with("attachments unread")))
                .map(|r| r.name.clone())
                .collect(),
        }
    }
}

/// `summary.txt`: the identity line, a row a figure, then the counts.
pub fn render(identity: &str, rows: &[Row]) -> String {
    let mut out = format!("{identity}\n");
    for r in rows {
        let what = match &r.outcome {
            Outcome::NoSky => "no .sky in the pack".to_owned(),
            Outcome::Failed { error } => format!("FAILED: {error}"),
            Outcome::Swept => format!(
                "hero {} model {}; attached [{}]; other [{}]; scale {}; {}; {}{}{}",
                r.hero.as_deref().unwrap_or("?"),
                r.model.as_deref().unwrap_or("?"),
                r.attached.join(", "),
                r.other_attached.join(", "),
                r.scale
                    .map_or("?".into(), |s| format!("{} {} {}", s[0], s[1], s[2])),
                r.collision.as_deref().unwrap_or("no collision"),
                r.parameters.as_deref().unwrap_or("no parameters"),
                if r.unlisted.is_empty() {
                    String::new()
                } else {
                    format!("; unlisted [{}]", r.unlisted.join(", "))
                },
                if r.notes.is_empty() {
                    String::new()
                } else {
                    format!("; notes: {}", r.notes.join("; "))
                }
            ),
        };
        out.push_str(&format!(
            "{} {} ({}, {:?}): {}\n",
            r.id, r.name, r.load_file, r.kind, what
        ));
    }
    let c = Counts::of(rows);
    out.push_str(&format!(
        "figures {} (with a .sky {}, without {}: {:?}); swept {}; failed {}: {:?}; \
         {} swept with a model attached, {} attached models; by attached models {:?}; \
         attachments unread on {:?}\n",
        c.listed,
        c.with_sky,
        c.no_sky.len(),
        c.no_sky,
        c.swept,
        c.failed.len(),
        c.failed,
        c.with_models_attached,
        c.models_attached,
        c.by_count,
        c.unread
    ));
    out
}

/// What a sweep takes beyond the oracle and its home.
pub struct SweepOptions {
    pub out: PathBuf,
    pub pack: PathBuf,
    pub state: PathBuf,
    pub dump: PathBuf,
    /// Ids, names or load files; empty is every figure.
    pub keys: Vec<String>,
    pub attempts: u32,
    pub gap: Duration,
    pub boot_wait: Duration,
    pub settle: u64,
}

/// The sweep the module doc lays out. `line` gets every line `run.txt` does.
pub fn run_sweep(
    exe: &Path,
    home: &OracleHome,
    opts: &SweepOptions,
    mut line: impl FnMut(&str) -> Result<()>,
) -> Result<Vec<Row>> {
    if opts.attempts == 0 {
        bail!("a sweep tries each figure at least once");
    }
    let out = rrc_dump::output::refuse_repo_output(&opts.out)?;
    fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
    let roster = roster(&opts.dump)?;
    let pack = scan_pack(&opts.pack)?;
    let all = figures(&roster, &pack);
    let chosen: Vec<SweepFigure> = if opts.keys.is_empty() {
        all.clone()
    } else {
        for key in &opts.keys {
            if !all.iter().any(|f| f.matches(key)) {
                bail!("{key} names no figure with a character model");
            }
        }
        all.iter()
            .filter(|f| opts.keys.iter().any(|k| f.matches(k)))
            .cloned()
            .collect()
    };
    fs::write(
        out.join("figures.json"),
        serde_json::to_string_pretty(&all).expect("a list serialises"),
    )?;
    let identity = Identity::of(exe, &home.config_dir())?.line();
    line(&identity)?;
    let pack_before = tree_digest(&opts.pack, true)?;
    let dev_dirs = dev_rpcs3_dirs(&home::real_home()?);
    let dev_before = Snapshot::take(&dev_dirs)?;
    let [carried] = <[PathBuf; 1]>::try_from(carried_dirs(&home.config_dir()))
        .map_err(|_| anyhow::anyhow!("one carried dir"))?;
    let pristine = out.join("home-pristine");
    if !pristine.exists() {
        copy_tree(&carried, &pristine)?;
    }
    let home_digest = tree_digest(&pristine, false)?;
    let host = Host::now();
    line(&format!(
        "start: home {}, state {} ({}), pack {} ({}); {} figures with a character model, {} \
         chosen, {} with a .sky; load average {:?}; screen locked {:?}; the dev's RPCS3 dirs: {} \
         entries; the home's saves {}",
        home.root().display(),
        opts.state.display(),
        &run::sha256_file(&opts.state)?[..16],
        opts.pack.display(),
        pack_before.line(),
        all.len(),
        chosen.len(),
        chosen.iter().filter(|f| f.sky.is_some()).count(),
        host.load_average,
        host.screen_locked,
        dev_before.len(),
        &home_digest.contents[..16],
    ))?;

    let inputs = Inputs {
        exe,
        home,
        state: &opts.state,
        pack: &opts.pack,
        boot_wait: opts.boot_wait,
        settle: opts.settle,
    };
    let mut rows = Vec::new();
    let mut last_exit: Option<Instant> = None;
    for figure in &chosen {
        let label = figure.label();
        let done = out.join(&label).join(RECORD_FILE);
        let outcome: Result<FigureRecord, String> = if figure.sky.is_none() {
            line(&format!("{label}: {} has no .sky in the pack", figure.name))?;
            Err(String::new())
        } else if let Ok(record) = FigureRecord::load(&done) {
            line(&format!("{label}: kept from an earlier run"))?;
            Ok(record)
        } else {
            let mut last = String::new();
            let mut got = None;
            for attempt in 1..=opts.attempts {
                if tree_digest(&carried, false)?.contents != home_digest.contents {
                    fs::remove_dir_all(&carried)?;
                    copy_tree(&pristine, &carried)?;
                    if tree_digest(&carried, false)?.contents != home_digest.contents {
                        bail!("putting the home's saves back didn't take");
                    }
                    line("the home's saves put back from the start's copy")?;
                }
                if let Some(t) = last_exit {
                    std::thread::sleep(opts.gap.saturating_sub(t.elapsed()));
                }
                let result = run_figure(&inputs, figure, &out);
                last_exit = Some(Instant::now());
                match result {
                    Ok(record) => {
                        got = Some(record);
                        break;
                    }
                    Err(e) => {
                        last = format!("{e:#}");
                        line(&format!(
                            "{label}: try {attempt} of {} failed: {last}",
                            opts.attempts
                        ))?;
                        let dir = out.join(&label);
                        if dir.exists() {
                            // A rerun into the same dir keeps an earlier run's failures.
                            let failed = (attempt..)
                                .map(|k| out.join(format!("{label}.failed-{k}")))
                                .find(|p| !p.exists())
                                .expect("a free name");
                            fs::rename(&dir, &failed)?;
                            fs::write(failed.join("error.txt"), format!("{last}\n"))?;
                        }
                    }
                }
            }
            got.ok_or(last)
        };
        let row = Row::of(figure, outcome.as_ref().map_err(String::as_str));
        if let Ok(r) = &outcome
            && row.outcome == Outcome::Swept
            && r.wall_secs > 0.0
        {
            line(&format!(
                "{label}: boot {:.1} s, wall {:.0} s, load {:?} -> {:?}; {} RECONNECT(s), {} \
                 queries; archive {}; hero {} at {}, read at {}; model {}; attached [{}]{}",
                r.boot_secs,
                r.wall_secs,
                r.before.load_average,
                r.after.load_average,
                1 + r.reconnects,
                r.queries,
                r.archive.as_ref().map_or("none".into(), |a| format!(
                    "{} at +{:.1} s",
                    a.path, a.after_secs
                )),
                row.hero.as_deref().unwrap_or("?"),
                r.hero_at,
                r.read_at,
                row.model.as_deref().unwrap_or("?"),
                row.attached.join(", "),
                if r.silent { "" } else { "; NOT SILENT" }
            ))?;
        }
        rows.push(row);
        write_summary(&out, &identity, &rows)?;
    }

    if tree_digest(&carried, false)?.contents != home_digest.contents {
        fs::remove_dir_all(&carried)?;
        copy_tree(&pristine, &carried)?;
        line("the home's saves put back from the start's copy")?;
    }
    let pack_after = tree_digest(&opts.pack, true)?;
    let dev_changed = dev_before.diff(&Snapshot::take(&dev_dirs)?);
    let host = Host::now();
    line(&format!(
        "end: load average {:?}; screen locked {:?}; pack {} ({}); the dev's RPCS3 dirs {}",
        host.load_average,
        host.screen_locked,
        pack_after.line(),
        if pack_after == pack_before {
            "unchanged"
        } else {
            "CHANGED"
        },
        if dev_changed.is_empty() {
            format!("unchanged, {} entries", dev_before.len())
        } else {
            format!("CHANGED:\n{}", dev_changed.join("\n"))
        }
    ))?;
    line(render(&identity, &rows).lines().last().unwrap_or(""))?;
    if pack_after != pack_before {
        bail!("the pack changed during the sweep");
    }
    if !dev_changed.is_empty() {
        bail!("the dev's RPCS3 dirs changed during the sweep");
    }
    Ok(rows)
}

fn write_summary(out: &Path, identity: &str, rows: &[Row]) -> Result<()> {
    #[derive(Serialize)]
    struct Summary<'a> {
        identity: &'a str,
        counts: Counts,
        rows: &'a [Row],
    }
    let summary = Summary {
        identity,
        counts: Counts::of(rows),
        rows,
    };
    fs::write(
        out.join("summary.json"),
        serde_json::to_string_pretty(&summary).expect("a summary serialises"),
    )?;
    fs::write(out.join("summary.txt"), render(identity, rows))?;
    Ok(())
}

/// A dir's files hashed: how many, and with `listing`, every entry's path,
/// size and mtime too, so a touched file shows even when its bytes didn't
/// change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeDigest {
    pub entries: usize,
    pub listing: Option<String>,
    pub contents: String,
}

impl TreeDigest {
    pub fn line(&self) -> String {
        format!(
            "{} entries, listing {}, contents {}",
            self.entries,
            self.listing.as_deref().map_or("-", |l| &l[..16]),
            &self.contents[..16]
        )
    }
}

/// [`TreeDigest`] of `root`, links listed and not followed, in path order.
pub fn tree_digest(root: &Path, listing: bool) -> Result<TreeDigest> {
    let mut paths = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))? {
            let path = entry?.path();
            if fs::symlink_metadata(&path)?.is_dir() {
                dirs.push(path.clone());
            }
            paths.push(path);
        }
    }
    paths.sort();
    let (mut list, mut contents) = (Sha256::new(), Sha256::new());
    for path in &paths {
        let meta = fs::symlink_metadata(path)?;
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        list.update(format!("{rel}|{}|{mtime}\n", meta.len()));
        if meta.is_file() {
            contents.update(format!("{rel}\n"));
            contents.update(Sha256::digest(fs::read(path)?));
        }
    }
    Ok(TreeDigest {
        entries: paths.len(),
        listing: listing.then(|| hex(&list.finalize())),
        contents: hex(&contents.finalize()),
    })
}

/// Copies the tree at `from` to `to` (which mustn't exist), files and dirs
/// only: a link is refused, so nothing outside the home comes along.
pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if to.exists() {
        bail!("{} exists", to.display());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from).with_context(|| format!("listing {}", from.display()))? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let dest = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &dest)?;
        } else {
            bail!("{} is a link: not copied", entry.path().display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::attach::{Attachment, HeroAttachments, ModelRef, Node};
    use super::*;
    use crate::actors::Actor;
    use crate::divergence::EndShot;
    use crate::hero::{Collision, Hero, Parameters};

    fn figure(id: &str, name: &str, load: &str, sky: bool) -> SweepFigure {
        SweepFigure {
            id: id.into(),
            name: name.into(),
            toy_type: 26,
            variant: 0,
            kind: FigureKind::Base,
            category: Category::Figure,
            load_file: load.into(),
            sky: sky.then(|| Picked {
                path: format!("Life/{name}.sky").into(),
                word: 0,
                others: vec![],
            }),
        }
    }

    fn knife(bone: &str) -> Attachment {
        Attachment {
            placement: 0x3000_1000,
            class: "tfbActor::tfbActorInfo".into(),
            name: Some("026_Knife_reg1".into()),
            activity_state: 1,
            node: Node::Bone {
                address: 0x3000_2000,
                name: Some(bone.into()),
                index: 20,
                transform: [0.0; 6],
            },
            owned_by_parent: true,
            attach_xform: [0.0; 6],
            model: Some(ModelRef {
                address: 0x3000_3000,
                class: "tfbPhysicsLink::tfbPhysicsModel".into(),
                name: Some("C:/tfb/Content/Models/Characters/StealthElfKnife.mdl".into()),
            }),
            corpus: Some(0x3000_4000),
            listed: true,
            corpus_flags: Some(0x0001_0000),
            scale: Some([120.0; 3]),
            attached: vec![],
        }
    }

    fn record(f: &SweepFigure, attached: Vec<Attachment>) -> FigureRecord {
        let host = Host {
            load_average: Some([3.0, 4.0, 5.0]),
            uptime: String::new(),
            screen_locked: Some(true),
        };
        FigureRecord {
            identity: "oracle sha256=…".into(),
            figure: f.clone(),
            sky_sha256: "00".into(),
            state: "BLUS30968_1_1.SAVESTAT.zst".into(),
            state_sha256: "bd".into(),
            before: host.clone(),
            after: host,
            boot_secs: 4.4,
            f0: 2,
            f0_hero: Some("009_Eruptor".into()),
            reconnects: 1,
            queries: 40,
            presses: 0,
            archive: Some(Opened {
                path: "/dev_bdvd/PS3_GAME/USRDIR/character/026_stealthelf.arc".into(),
                after_secs: 6.0,
            }),
            hero_at: 900,
            read_at: 1200,
            game_frame: 7000,
            hero: Actor {
                corpus: 0x3000_0000,
                placement: 0x3000_0100,
                name: Some(f.load_file.clone()),
                position: [0.0; 3],
                rotation: [0.0; 3],
                scale: [120.0; 3],
                world: None,
                velocity: [0.0; 3],
                skinned: true,
                pad: Some(-1),
            },
            hero_is_load_file: true,
            tuning: Some(Hero {
                name: Some(f.load_file.clone()),
                corpus: 0x3000_0000,
                placement: 0x3000_0100,
                scale: [120.0; 3],
                collision: Some(Collision {
                    address: 1,
                    properties: 0x1B,
                    position: [0.0; 3],
                    angles: [0.0; 3],
                    width: 4.0,
                    height: 6.0,
                    length: 3.0,
                    weight: 10.0,
                    rebound: 0.0,
                }),
                parameters: Some(Parameters::decode(2, &[0; 0x4C])),
                solid: None,
            }),
            tuning_error: None,
            attachments: Some(HeroAttachments {
                placement: 0x3000_0100,
                name: Some(f.load_file.clone()),
                model: Some(ModelRef {
                    address: 5,
                    class: "tfbPhysicsLink::tfbPhysicsModel".into(),
                    name: Some("C:/tfb/Content/Models/Characters/StealthElf.mdl".into()),
                }),
                attached,
                unlisted: vec![],
            }),
            attachments_error: None,
            shot: EndShot {
                file: SHOT_FILE.into(),
                flip: 1200,
                pixels_sha256: "ab".into(),
                lit_share: 0.99,
            },
            silent: true,
            fatal: vec![],
            wall_secs: 30.0,
            shutdown: "Graceful".into(),
        }
    }

    #[test]
    fn the_summary_counts_swept_failed_and_missing_figures_and_their_attachments() {
        let elf = figure("00001a-0000", "Stealth Elf", "026_StealthElf", true);
        let eruptor = figure("000009-0000", "Eruptor", "009_Eruptor", true);
        let zap = figure("00000c-0000", "Zap", "012_WaterDragon", true);
        let eye = figure("000072-0000", "Eye-Brawl", "114_Eyebrawl", false);
        let elf_record = record(&elf, vec![knife("J_attach_R"), knife("J_attach_L")]);
        let eruptor_record = record(&eruptor, vec![]);
        let rows = vec![
            Row::of(&elf, Ok(&elf_record)),
            Row::of(&eruptor, Ok(&eruptor_record)),
            Row::of(&zap, Err("no new hero")),
            Row::of(&eye, Err("")),
        ];
        assert_eq!(
            rows[0].attached,
            [
                "StealthElfKnife.mdl@J_attach_R",
                "StealthElfKnife.mdl@J_attach_L"
            ]
        );
        assert_eq!(rows[0].model.as_deref(), Some("StealthElf.mdl"));
        assert_eq!(
            rows[0].collision.as_deref(),
            Some("sphere 4 x 6 x 3 ft, 10 lb")
        );
        assert_eq!(rows[0].dir.as_deref(), Some("00001a-0000-026_StealthElf"));
        assert_eq!(
            rows[2].outcome,
            Outcome::Failed {
                error: "no new hero".into()
            }
        );
        assert_eq!(rows[3].outcome, Outcome::NoSky);
        let c = Counts::of(&rows);
        assert_eq!((c.listed, c.with_sky, c.swept), (4, 3, 2));
        assert_eq!(c.no_sky, ["Eye-Brawl"]);
        assert_eq!(c.failed, ["Zap"]);
        assert_eq!((c.with_models_attached, c.models_attached), (1, 2));
        assert_eq!(c.by_count, BTreeMap::from([(0, 1), (2, 1)]));
        let text = render("identity", &rows);
        assert!(text.starts_with("identity\n"));
        assert!(text.contains(
            "00001a-0000 Stealth Elf (026_StealthElf, Base): hero 026_StealthElf model \
             StealthElf.mdl; attached [StealthElfKnife.mdl@J_attach_R, \
             StealthElfKnife.mdl@J_attach_L]"
        ));
        assert!(text.contains("000072-0000 Eye-Brawl (114_Eyebrawl, Base): no .sky in the pack"));
        assert!(text.contains("00000c-0000 Zap (012_WaterDragon, Base): FAILED: no new hero"));
        assert!(text.ends_with(
            "figures 4 (with a .sky 3, without 1: [\"Eye-Brawl\"]); swept 2; failed 1: [\"Zap\"]; \
             1 swept with a model attached, 2 attached models; by attached models {0: 1, 2: 1}; \
             attachments unread on []\n"
        ));
        assert!(rows[0].notes.is_empty(), "{:?}", rows[0].notes);
        // A knife whose back link names someone else: kept, and noted.
        let mut stray = knife("J_attach_L");
        stray.owned_by_parent = false;
        let r = record(&elf, vec![knife("J_attach_R"), stray]);
        assert_eq!(
            Row::of(&elf, Ok(&r)).notes,
            ["StealthElfKnife.mdl@J_attach_L not owned by what lists it"]
        );
        // A record round-trips through figure.json.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(RECORD_FILE);
        fs::write(&path, serde_json::to_string(&elf_record).unwrap()).unwrap();
        assert_eq!(FigureRecord::load(&path).unwrap(), elf_record);
    }

    #[test]
    fn a_home_copied_out_digests_the_same_and_a_save_changes_it() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let saves = home.join("00000001/savedata/BLUS30968");
        fs::create_dir_all(&saves).unwrap();
        fs::write(saves.join("E_HEADER"), b"header").unwrap();
        fs::write(saves.join("E_SLOT_1"), b"slot").unwrap();
        let copy = tmp.path().join("pristine");
        copy_tree(&home, &copy).unwrap();
        let (a, b) = (
            tree_digest(&home, false).unwrap(),
            tree_digest(&copy, false).unwrap(),
        );
        assert_eq!(a.contents, b.contents);
        assert_eq!(a.entries, 5);
        assert!(
            copy_tree(&home, &copy).is_err(),
            "never over an existing dir"
        );
        fs::write(saves.join("E_SLOT_1"), b"slot after a new figure").unwrap();
        assert_ne!(tree_digest(&home, false).unwrap().contents, b.contents);
        // Put back, it matches again.
        fs::remove_dir_all(&home).unwrap();
        copy_tree(&copy, &home).unwrap();
        assert_eq!(tree_digest(&home, false).unwrap().contents, b.contents);
        // The listing sees a touch the contents don't.
        let listed = tree_digest(&home, true).unwrap();
        let file = fs::File::options()
            .write(true)
            .open(saves.join("E_HEADER"))
            .unwrap();
        file.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1))
            .unwrap();
        let touched = tree_digest(&home, true).unwrap();
        assert_eq!(touched.contents, listed.contents);
        assert_ne!(touched.listing, listed.listing);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&saves, home.join("link")).unwrap();
            assert!(copy_tree(&home, &tmp.path().join("again")).is_err());
        }
    }
}
