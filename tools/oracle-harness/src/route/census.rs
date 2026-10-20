//! What level 213's own placements are doing along the route (openspec AH.8,
//! design decisions 16 and 17), read at a pause: every actor placement's
//! wake state and the behaviour its script runs, the triggers, and the named
//! placements' script variables with their values. Read again at each
//! `Read` step, two reads say which scripts started on the way, which
//! triggers fired between them and what their scripts set; the per-frame half
//! is O5's (the named placements' wake state on the watch list,
//! [`crate::fight::FightWatch::with_triggers`]), from which a drive's
//! [`RouteRecord`] puts the triggers in the order they fired.
//!
//! The disc says where each placement sits in the level's data section
//! ([`Disc`], off `level_213_tutotial.bld`) and the live section's base is
//! voted on by the actor list ([`crate::level::section_base`]), AG.7's way;
//! the layout is [`crate::script`]'s.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::actors::{ActorFrame, Gather, be32, expect_vtable, f32_at};
use crate::fight::{self, Trigger};
use crate::level::{self, SectionBase};
use crate::script::{
    self, Instance, PLACEMENT_ACTIVE_TIME, PLACEMENT_BEHAVIOUR, PLACEMENT_FLAGS, PLACEMENT_LEN,
    PLACEMENT_RANGE, PLACEMENT_STATE, STARTED, Types, Value, vtable,
};

/// Level 213's archive, as the disc names it.
pub const LEVEL_213: &str = "level/level_213_tutotial.bld";

/// The placements a census reads whole, script variables and all, besides
/// the level's whitelisted ones (design decision 2): the first fight's, its
/// controller and the level's enemy system (AH.4's), and what the route
/// passes: its triggers, the Norticus pan, the spike gate, the push blocks
/// and their ends, the wooden fence, the cave and the zone over the pit.
pub const ROUTE: [&str; 21] = [
    "Setup_001_Trigger",
    "Activate_Norticus and Chompies",
    "Setup_001_Controller",
    "Enemy_Chompy(1)",
    "Enemy_Chompy(2)",
    "Enemy_Chompy(3)",
    "Enemy_Chompy(4)",
    "1.NPC_Norticus_First_Encounter",
    "Norticus_First_Encounter(1)",
    "EnemySystem_",
    "Eon_4_help_Norticus",
    "Spike_Gate_Wooden_Template(1)",
    "Push_Block_Template(1)",
    "Push_Block_Template(2)",
    "Push_Block_Template(3)",
    "Push_block_Water_End(1)",
    "Push_block_Water_End(2)",
    "Fence Spawner_Template(1)",
    "Interior_controller_Cave",
    "Name_Zone(4)",
    "Pop_to_Camera_5",
];

/// The placements O5 watches per frame on a route (their `_activityState`
/// to `_activeTime`): [`ROUTE`] less the enemy system, the zone and the
/// camera pop, so the list stays inside O5's 64 ranges beside the hero's,
/// the generator's and the fight's cells.
pub const WATCHED: [&str; 18] = [
    "Setup_001_Trigger",
    "Activate_Norticus and Chompies",
    "Setup_001_Controller",
    "Enemy_Chompy(1)",
    "Enemy_Chompy(2)",
    "Enemy_Chompy(3)",
    "Enemy_Chompy(4)",
    "1.NPC_Norticus_First_Encounter",
    "Norticus_First_Encounter(1)",
    "Eon_4_help_Norticus",
    "Spike_Gate_Wooden_Template(1)",
    "Push_Block_Template(1)",
    "Push_Block_Template(2)",
    "Push_Block_Template(3)",
    "Push_block_Water_End(1)",
    "Push_block_Water_End(2)",
    "Fence Spawner_Template(1)",
    "Interior_controller_Cave",
];

/// An actor placement where the level's data section holds it, read off the
/// disc before anything runs: what the live reads are found by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscActor {
    /// Its offset in the level's data section.
    pub offset: u32,
    pub name: String,
    /// Its script's source path, none for a placement without one.
    pub script: Option<String>,
    /// Its activation range, feet (0: the scripts wake it).
    pub range: f32,
    /// Its `_initMatrix` position, metres.
    pub position: [f32; 3],
}

/// Level 213's actor placements in section 0, in the file's object order.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Disc {
    pub actors: Vec<DiscActor>,
}

/// A name a trigger goes by (AH.4's live read): `Trigger` in it, or
/// `trg_`, `Transition `, `Activate_` or `Pop_` before it.
pub fn is_trigger(name: &str) -> bool {
    name.contains("Trigger")
        || name.contains("trigger")
        || ["trg_", "Transition ", "Activate_", "Pop_"]
            .iter()
            .any(|p| name.starts_with(p))
}

impl Disc {
    /// Level 213's `level.bld` under `usrdir`, its actors read.
    pub fn read(usrdir: &Path) -> Result<Self> {
        use guac_formats::iga::Archive;
        use guac_formats::igz::Igz;
        use guac_formats::level::Placements;
        let path = usrdir.join(LEVEL_213);
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let arc = Archive::parse(&bytes)?;
        let entry = arc.find("level.bld").context("no level.bld in level 213")?;
        let data = arc.read(entry)?.into_owned();
        let igz = Igz::parse(&data)?;
        let placements = Placements::read(&igz)?;
        let mut actors = Vec::new();
        for a in &placements.actors {
            let at = igz.objects()[a.index].at;
            if at.section() != 0 {
                continue;
            }
            let p = a.placement();
            actors.push(DiscActor {
                offset: at.offset(),
                name: a.name().unwrap_or("").to_owned(),
                script: a.script.map(str::to_owned),
                range: p.activation_range,
                position: p.placement.init_matrix.position,
            });
        }
        Ok(Self { actors })
    }

    /// The names listed once, by offset: a name two placements share can't
    /// be told apart by name.
    pub fn offsets(&self) -> BTreeMap<String, u32> {
        let mut seen: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
        for a in &self.actors {
            if !a.name.is_empty() {
                seen.entry(&a.name).or_default().push(a.offset);
            }
        }
        seen.into_iter()
            .filter(|(_, v)| v.len() == 1)
            .map(|(n, v)| (n.to_owned(), v[0]))
            .collect()
    }

    /// The trigger placements listed once, in name order.
    pub fn triggers(&self) -> Vec<String> {
        self.offsets()
            .into_keys()
            .filter(|n| is_trigger(n))
            .collect()
    }

    /// The placements listed once whose script the level's whitelist names
    /// (`guac_extract::scripts`, design decision 2), in name order.
    pub fn whitelisted(&self) -> Vec<String> {
        let scripts = guac_extract::scripts::level_scripts("level_213_tutotial");
        let once = self.offsets();
        self.actors
            .iter()
            .filter(|a| once.contains_key(&a.name))
            .filter(|a| {
                a.script
                    .as_deref()
                    .is_some_and(|s| guac_extract::scripts::listed(scripts, s))
            })
            .map(|a| a.name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// The script's file name a placement runs (`S2_Enemy_Chompy.ai`).
    pub fn script_file(&self, offset: u32) -> Option<&str> {
        self.actors
            .iter()
            .find(|a| a.offset == offset)
            .and_then(|a| a.script.as_deref())
            .map(|s| s.rsplit(['/', '\\']).next().unwrap_or(s))
    }
}

/// One placement's wake state at a pause, as briefly as a whole level reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Brief {
    pub offset: u32,
    pub name: String,
    /// The script's file name, off the disc.
    pub script: Option<String>,
    pub activity_state: u32,
    /// Its STARTUP has run.
    pub started: bool,
    pub active_seconds: f32,
    /// Its live activation range, feet.
    pub range: f32,
    /// The behaviour its script runs, by name.
    pub behaviour: Option<String>,
}

/// Every disc actor at `base`, in two batched reads and one for the
/// behaviours' names. Refused: a placement whose first word isn't a
/// `tfbActorInfo`'s (the base is wrong), or a behaviour pointer at anything
/// but an `OpUserBehavior`.
pub fn briefs(g: &mut impl Gather, base: u32, disc: &Disc) -> Result<Vec<Brief>> {
    let ranges: Vec<(u32, usize)> = disc
        .actors
        .iter()
        .map(|a| {
            base.checked_add(a.offset)
                .map(|at| (at, PLACEMENT_LEN))
                .context("a placement's address passes 4 GiB")
        })
        .collect::<Result<_>>()?;
    let read = g.gather(&ranges)?;
    let mut out = Vec::with_capacity(ranges.len());
    let mut behaviours: BTreeSet<u32> = BTreeSet::new();
    let mut pointers = Vec::with_capacity(ranges.len());
    for ((a, &(at, _)), bytes) in disc.actors.iter().zip(&ranges).zip(read) {
        let b = bytes.with_context(|| format!("`{}` at {at:#010x} isn't mapped", a.name))?;
        expect_vtable(&b, vtable::ACTOR_INFO, at, "tfbActorInfo")
            .with_context(|| format!("`{}`", a.name))?;
        let behaviour = be32(&b, PLACEMENT_BEHAVIOUR);
        if behaviour != 0 {
            behaviours.insert(behaviour);
        }
        pointers.push(behaviour);
        out.push(Brief {
            offset: a.offset,
            name: a.name.clone(),
            script: disc.script_file(a.offset).map(str::to_owned),
            activity_state: be32(&b, PLACEMENT_STATE),
            started: be32(&b, PLACEMENT_FLAGS) & STARTED != 0,
            active_seconds: f32_at(&b, PLACEMENT_ACTIVE_TIME),
            range: f32_at(&b, PLACEMENT_RANGE),
            behaviour: None,
        });
    }
    let names = behaviour_names(g, &behaviours)?;
    for (brief, p) in out.iter_mut().zip(pointers) {
        if p != 0 {
            brief.behaviour = Some(names[&p].clone());
        }
    }
    Ok(out)
}

/// Each behaviour pointer's name, its class checked.
pub fn behaviour_names(
    g: &mut impl Gather,
    pointers: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, String>> {
    let at: Vec<u32> = pointers.iter().copied().collect();
    let named = script::named(g, &at)?;
    let mut out = BTreeMap::new();
    for n in named {
        if n.vtable != vtable::USER_BEHAVIOR {
            bail!(
                "a behaviour pointer names {:#010x}, vtable {:#010x}, not an OpUserBehavior",
                n.at,
                n.vtable
            );
        }
        out.insert(n.at, n.name);
    }
    if let Some(p) = at.iter().find(|p| !out.contains_key(p)) {
        bail!("the behaviour at {p:#010x} has no readable name");
    }
    Ok(out)
}

/// A script variable and what it holds; a value that wouldn't read keeps
/// the reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub value: Option<Value>,
    pub error: Option<String>,
}

/// A placement read whole: its instance and every variable's value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Whole {
    pub name: String,
    pub placement: u32,
    pub activity_state: u32,
    pub started: bool,
    pub active_seconds: f32,
    pub range: f32,
    pub behaviour: Option<String>,
    /// None while its script never started.
    pub variables: Option<Vec<Variable>>,
}

/// `placement`'s instance and its variables' values.
pub fn whole(g: &mut impl Gather, types: &mut Types, name: &str, placement: u32) -> Result<Whole> {
    let i = Instance::read(g, placement)?;
    if i.name.as_deref() != Some(name) {
        bail!(
            "the placement at {placement:#010x} is {:?}, not `{name}`",
            i.name
        );
    }
    let variables = i.variables.as_ref().map(|cells| {
        cells
            .iter()
            .map(|c| match script::value(g, types, c) {
                Ok(v) => Variable {
                    name: c.name.clone(),
                    value: Some(v),
                    error: None,
                },
                Err(e) => Variable {
                    name: c.name.clone(),
                    value: None,
                    error: Some(format!("{e:#}")),
                },
            })
            .collect()
    });
    Ok(Whole {
        name: name.to_owned(),
        placement,
        activity_state: i.activity_state,
        started: i.started,
        active_seconds: i.active_seconds,
        range: i.activation_range,
        behaviour: i.behaviour,
        variables,
    })
}

/// The section's base, every actor placement's brief, the triggers and the
/// placements read whole, at one pause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Census {
    pub flip: u64,
    pub game_frame: u32,
    pub base: SectionBase,
    pub briefs: Vec<Brief>,
    pub triggers: Vec<Trigger>,
    pub whole: Vec<Whole>,
}

impl Census {
    /// Every placement's brief by name (the names listed once).
    pub fn brief(&self, name: &str) -> Option<&Brief> {
        self.briefs.iter().find(|b| b.name == name)
    }

    /// Whether a trigger by this name has fired (`None`: no range, or not
    /// read).
    pub fn fired(&self, name: &str) -> Option<bool> {
        self.triggers
            .iter()
            .find(|t| t.name == name)
            .and_then(|t| t.fired)
    }

    /// What changed from `before` to here: placements whose wake state,
    /// started flag or behaviour moved, in disc order.
    pub fn changes_since(&self, before: &Census) -> Vec<Change> {
        let was: BTreeMap<u32, &Brief> = before.briefs.iter().map(|b| (b.offset, b)).collect();
        self.briefs
            .iter()
            .filter_map(|now| {
                let then = was.get(&now.offset)?;
                (then.activity_state != now.activity_state
                    || then.started != now.started
                    || then.behaviour != now.behaviour)
                    .then(|| Change {
                        name: now.name.clone(),
                        script: now.script.clone(),
                        from: (then.activity_state, then.started, then.behaviour.clone()),
                        to: (now.activity_state, now.started, now.behaviour.clone()),
                    })
            })
            .collect()
    }
}

/// A placement's wake state, started flag and behaviour before and after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub name: String,
    pub script: Option<String>,
    pub from: (u32, bool, Option<String>),
    pub to: (u32, bool, Option<String>),
}

/// The census at the pause `frame` was read at: the section's base voted on
/// by `frame`'s actors, every disc actor's brief, every trigger's wake state
/// and the placements named in `whole_names` read whole (a name not listed
/// once refused).
pub fn read(
    g: &mut impl Gather,
    frame: &ActorFrame,
    disc: &Disc,
    whole_names: &[String],
) -> Result<Census> {
    let offsets = disc.offsets();
    let base = level::section_base(frame, &offsets)?;
    let briefs = briefs(g, base.base, disc)?;
    let trigger_at: Vec<(String, u32)> = disc
        .triggers()
        .into_iter()
        .map(|n| {
            let at = base.base + offsets[&n];
            (n, at)
        })
        .collect();
    let named: Vec<(&str, u32)> = trigger_at.iter().map(|(n, a)| (n.as_str(), *a)).collect();
    let triggers = fight::triggers(g, &named)?;
    let mut types = Types::default();
    let mut whole_reads = Vec::with_capacity(whole_names.len());
    for name in whole_names {
        let off = offsets
            .get(name)
            .with_context(|| format!("`{name}` isn't a placement the disc lists once"))?;
        whole_reads.push(whole(g, &mut types, name, base.base + off)?);
    }
    Ok(Census {
        flip: frame.flip.unwrap_or(0),
        game_frame: frame.game_frame,
        base,
        briefs,
        triggers,
        whole: whole_reads,
    })
}

/// [`read`] over PINE: Giants 01.00 only, paused before and after.
pub fn read_paused(
    pine: &mut crate::pine::PineClient,
    frame: &ActorFrame,
    disc: &Disc,
    whole_names: &[String],
) -> Result<Census> {
    crate::rng::check_title(pine)?;
    let c = read(pine, frame, disc, whole_names)?;
    level::still_paused(pine, "the census")?;
    Ok(c)
}

/// The names a route's census reads whole: [`ROUTE`], then the level's
/// whitelisted placements not among them.
pub fn whole_names(disc: &Disc) -> Vec<String> {
    let mut out: Vec<String> = ROUTE.iter().map(|s| (*s).to_owned()).collect();
    for n in disc.whitelisted() {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    let once = disc.offsets();
    out.retain(|n| once.contains_key(n));
    out
}

/// A watched placement's wake state as one record holds it (O5, AH.4's
/// trigger range): its state, its behaviour pointer and its clock.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Wake {
    pub activity_state: u32,
    pub behaviour: u32,
    pub active_seconds: f32,
}

/// When something happened to a watched placement, record by record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fired {
    pub name: String,
    /// The flip and game frame of the first record that shows it.
    pub flip: u64,
    pub game_frame: u32,
    /// What: `active` (its clock started, or its state went to 1), a
    /// `state a -> b`, or a `behaviour a -> b` by name.
    pub what: String,
}

/// From each watched placement's wake state record by record (flip, game
/// frame, the states in `names`' order), what happened and when, in the
/// order it happened (ties in `names`' order): a clock that starts, a state
/// that changes, a behaviour that changes. The first record is the baseline;
/// a placement already running there shows nothing until it changes.
pub fn fired(
    names: &[String],
    records: &[(u64, u32, Vec<Wake>)],
    behaviour: &BTreeMap<u32, String>,
) -> Result<Vec<Fired>> {
    let name_of = |p: u32| -> String {
        if p == 0 {
            "none".into()
        } else {
            behaviour
                .get(&p)
                .cloned()
                .unwrap_or_else(|| format!("{p:#010x}"))
        }
    };
    let mut out = Vec::new();
    let Some((_, _, first)) = records.first() else {
        return Ok(out);
    };
    for (_, _, wakes) in records {
        if wakes.len() != names.len() {
            bail!(
                "a record holds {} wake states for {} names",
                wakes.len(),
                names.len()
            );
        }
    }
    let mut last = first.clone();
    for &(flip, game_frame, ref wakes) in &records[1..] {
        for (i, (now, then)) in wakes.iter().zip(&last).enumerate() {
            let mut push = |what: String| {
                out.push(Fired {
                    name: names[i].clone(),
                    flip,
                    game_frame,
                    what,
                })
            };
            if then.active_seconds <= 0.0 && now.active_seconds > 0.0 {
                push("active".into());
            }
            if then.activity_state != now.activity_state {
                push(format!(
                    "state {} -> {}",
                    then.activity_state, now.activity_state
                ));
            }
            if then.behaviour != now.behaviour {
                push(format!(
                    "behaviour {} -> {}",
                    name_of(then.behaviour),
                    name_of(now.behaviour)
                ));
            }
        }
        last.clone_from(wakes);
    }
    Ok(out)
}

/// One step of a drive as it went: the plan's step, the flips from its first
/// look to the next step's, and where he stood at each end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepRecord {
    pub index: usize,
    pub step: String,
    pub first_flip: u64,
    pub last_flip: u64,
    pub looks: usize,
    pub from: [f64; 3],
    pub to: [f64; 3],
}

/// The reads at a drive's last `Read`: where they went and the numbers
/// they're taken for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchorReads {
    pub label: String,
    pub flip: u64,
    pub game_frame: u32,
    pub pivot: [f64; 3],
    pub facing: f64,
    /// The files, in the drive's dir.
    pub files: Vec<String>,
    pub seed: Option<u32>,
    pub drawn: Option<u64>,
    /// Range triggers fired there, by name.
    pub fired: Vec<String>,
}

/// A drive's record (AH.8): its steps, what fired on the way in order, the
/// scripts that started, and the last read's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteRecord {
    pub identity: String,
    pub route: String,
    pub steps: Vec<StepRecord>,
    /// The watched placements' events in the order they happened.
    pub fired: Vec<Fired>,
    /// Between each read and the next, what started or changed.
    pub changes: Vec<(String, String, Vec<Change>)>,
    /// Every script that ran by the last read and hadn't by the first, by
    /// file name, with the placements that ran it.
    pub scripts: BTreeMap<String, Vec<String>>,
    pub anchor: Option<AnchorReads>,
}

pub const RECORD_FILE: &str = "route-record.json";

impl RouteRecord {
    /// The steps out of a drive's looks: a step runs from the first look
    /// made on it to the first made on the next.
    pub fn steps(looks: &[super::Look], plan: &super::RoutePlan) -> Vec<StepRecord> {
        let mut out: Vec<StepRecord> = Vec::new();
        for look in looks {
            match out.last_mut() {
                Some(s) if s.index == look.step => {
                    s.last_flip = look.seen.flip;
                    s.looks += 1;
                    s.to = look.seen.pivot;
                }
                _ => {
                    if let Some(s) = out.last_mut() {
                        s.last_flip = look.seen.flip;
                        s.to = look.seen.pivot;
                    }
                    out.push(StepRecord {
                        index: look.step,
                        step: plan
                            .steps
                            .get(look.step)
                            .map_or_else(|| "done".into(), |s| format!("{s:?}")),
                        first_flip: look.seen.flip,
                        last_flip: look.seen.flip,
                        looks: 1,
                        from: look.seen.pivot,
                        to: look.seen.pivot,
                    });
                }
            }
        }
        out
    }

    /// The scripts that ran by `last` and hadn't by `first`, by file name.
    pub fn scripts_between(first: &Census, last: &Census) -> BTreeMap<String, Vec<String>> {
        let was: BTreeMap<u32, &Brief> = first.briefs.iter().map(|b| (b.offset, b)).collect();
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for b in &last.briefs {
            let ran_now = b.started || b.active_seconds > 0.0;
            let ran_then = was
                .get(&b.offset)
                .is_some_and(|t| t.started || t.active_seconds > 0.0);
            if ran_now && !ran_then {
                out.entry(b.script.clone().unwrap_or_else(|| "(no script)".into()))
                    .or_default()
                    .push(b.name.clone());
            }
        }
        out
    }

    pub fn write(&self, dir: &Path) -> Result<()> {
        let path = rrc_dump::output::refuse_repo_output(&dir.join(RECORD_FILE))?;
        std::fs::write(&path, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))
    }

    pub fn read(dir: &Path) -> Result<Self> {
        let path = dir.join(RECORD_FILE);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

#[cfg(test)]
mod tests;
