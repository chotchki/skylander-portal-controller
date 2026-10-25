//! Level placements on Giants' watch list (openspec AG.176): each named
//! placement's matrix and wake state, and the script numbers named for it,
//! every frame. A script that keeps a draw in a variable leaves it there for
//! the record (`Falling_Rocks.ai`'s `$Timer` is `system.script timer` plus its
//! `random(1)`), and the generator's words on the same record say where in
//! the stream that draw fell.
//!
//! A place is found as the route census finds it ([`crate::route::census`]):
//! its offset in level 213's data section off the disc ([`Disc`]) at the base
//! the actor list votes for ([`crate::level::section_base`]); its numbers are
//! `ValueInfo`s on its `_localVarList` by name ([`crate::script`]). On the
//! list, `place.<name>` runs from the placement's `_currentMatrix` (+0x3C:
//! position, then tilt, heading and roll) through `_activeTime` (+0xB8), so it
//! carries `_activityState`, `_execBehavior` and the clock too, and
//! `cells.<n>` holds the numbers in blocks, each from its first cell's type
//! word to its last one's value, no wider than [`MAX_SPAN`].

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::actors::{ActorFrame, Gather, be32, f32_at};
use crate::level;
use crate::route::census::{self, Disc};
use crate::script::{self, Instance, Number, PLACEMENT_ACTIVE_TIME, Types, vtable};
use crate::verbs::WatchRecord;
use crate::watch::{Field, WatchList};

/// The placement as watched: its matrix to its clock's end.
const PLACE_FIRST: u32 = crate::actors::PLACEMENT_MATRIX as u32;
const PLACE_END: u32 = PLACEMENT_ACTIVE_TIME as u32 + 4;
const VALUE_TYPE: u32 = 0x1C;
const VALUE_END: u32 = 0x24;
/// A block of cells is no wider than this: one placement's variables sit
/// together on the heap, and a wider block would spend O5's bytes on the
/// gaps between them.
pub const MAX_SPAN: u32 = 0x400;

/// A place to watch and the numbers on it: `<name>` or
/// `<name>=<variable>,<variable>...`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceSpec {
    pub name: String,
    pub variables: Vec<String>,
}

impl PlaceSpec {
    /// Refused: an empty name or an empty variable.
    pub fn parse(text: &str) -> Result<Self> {
        let (name, vars) = match text.split_once('=') {
            Some((n, v)) => (n, v.split(',').map(str::to_owned).collect::<Vec<_>>()),
            None => (text, Vec::new()),
        };
        if name.is_empty() || vars.iter().any(String::is_empty) {
            bail!("a place is <name> or <name>=<variable>,<variable>..., not `{text}`");
        }
        Ok(Self {
            name: name.to_owned(),
            variables: vars,
        })
    }
}

/// The places on a watch list, resolved at a pause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaceWatch {
    /// Each place by name, with its placement.
    pub places: Vec<(String, u32)>,
    /// Each number: its place, its variable and its `ValueInfo`.
    pub cells: Vec<(String, String, u32)>,
    /// The variables named that their place didn't hold at the resolve
    /// (place, variable): left off the list rather than losing a run to a
    /// name.
    #[serde(default)]
    pub missing: Vec<(String, String)>,
    /// The `FloatMeasurement` metaobject: a cell whose type word is this
    /// reads as a float, anything else as an integer.
    pub float_type: u32,
}

/// One place as a record found it at its flip's pad read: where it stood,
/// its wake state and the numbers named for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaceAt {
    pub name: String,
    pub position: [f32; 3],
    /// Tilt, heading, roll, degrees.
    pub rotation: [f32; 3],
    pub activity_state: u32,
    /// `_execBehavior`: the `OpUserBehavior` its script runs (0 before its
    /// STARTUP picks one).
    pub behaviour: u32,
    pub active_seconds: f32,
    pub variables: BTreeMap<String, Number>,
}

impl PlaceWatch {
    /// The places `specs` name, at the section's base `frame`'s actors vote
    /// for. `float_type` is the `FloatMeasurement` metaobject when the caller
    /// knows it; otherwise the cells' own types must hold one. A variable the
    /// place doesn't hold goes in `missing`. Refused: a name the disc doesn't
    /// list once, a placement carrying another name, a variable named on a
    /// place whose script never started, or one that isn't a number.
    pub fn resolve(
        g: &mut impl Gather,
        frame: &ActorFrame,
        disc: &Disc,
        specs: &[PlaceSpec],
        float_type: Option<u32>,
    ) -> Result<Self> {
        if specs.is_empty() {
            bail!("no places to watch");
        }
        let offsets = disc.offsets();
        let base = level::section_base(frame, &offsets)?;
        let mut types = Types::default();
        let mut places = Vec::new();
        let mut cells = Vec::new();
        let mut missing = Vec::new();
        for spec in specs {
            if places.iter().any(|(n, _)| *n == spec.name) {
                bail!("`{}` is named twice", spec.name);
            }
            let off = offsets.get(&spec.name).with_context(|| {
                format!("`{}` isn't a placement the disc lists once", spec.name)
            })?;
            let placement = base.base + off;
            let i = Instance::read(g, placement)?;
            if i.name.as_deref() != Some(spec.name.as_str()) {
                bail!(
                    "the placement at {placement:#010x} is {:?}, not `{}`",
                    i.name,
                    spec.name
                );
            }
            if !spec.variables.is_empty() {
                let vars = i.variables.as_ref().with_context(|| {
                    format!(
                        "`{}`'s script never started: it holds no variables",
                        spec.name
                    )
                })?;
                for v in &spec.variables {
                    // By name whatever its case, as the VM finds a variable
                    // (`$Open`, which the activator finds as "open").
                    let mut found = vars.iter().filter(|c| c.name.eq_ignore_ascii_case(v));
                    let cell = match (found.next(), found.next()) {
                        (Some(one), None) => one,
                        (None, _) => {
                            missing.push((spec.name.clone(), v.clone()));
                            continue;
                        }
                        (Some(_), Some(_)) => {
                            bail!("`{}` holds more than one `{v}`", spec.name)
                        }
                    };
                    if cell.vtable != vtable::VALUE_INFO {
                        bail!("`{}`'s `{v}` isn't a number", spec.name);
                    }
                    script::number(g, &mut types, cell.at, &format!("`{}`.`{v}`", spec.name))?;
                    cells.push((spec.name.clone(), v.clone(), cell.at));
                }
                // Its other numbers too, for a float among them to read the
                // named ones by: a wait not drawn yet is still an integer 0.
                for cell in vars.iter().filter(|c| c.vtable == vtable::VALUE_INFO) {
                    let _ = script::number(g, &mut types, cell.at, &cell.name);
                }
            }
            places.push((spec.name.clone(), placement));
        }
        let float_type = float_type
            .or(types.float_type())
            .context("no FloatMeasurement to read the places' numbers by")?;
        let watch = Self {
            places,
            cells,
            missing,
            float_type,
        };
        // Every cell its own block at worst; a duplicate address would be one
        // variable watched under two names.
        let at: BTreeSet<u32> = watch.cells.iter().map(|c| c.2).collect();
        if at.len() != watch.cells.len() {
            bail!("a variable is named twice");
        }
        Ok(watch)
    }

    /// The cells grouped into blocks by address: `(first type word, last
    /// value's end, members)`.
    fn blocks(&self) -> Vec<(u32, u32, Vec<usize>)> {
        let mut order: Vec<usize> = (0..self.cells.len()).collect();
        order.sort_by_key(|&i| self.cells[i].2);
        let mut out: Vec<(u32, u32, Vec<usize>)> = Vec::new();
        for i in order {
            let at = self.cells[i].2;
            match out.last_mut() {
                Some((lo, hi, members)) if at + VALUE_END - *lo <= MAX_SPAN => {
                    *hi = (*hi).max(at + VALUE_END);
                    members.push(i);
                }
                _ => out.push((at + VALUE_TYPE, at + VALUE_END, vec![i])),
            }
        }
        out
    }

    /// The watch-list fields: `place.<name>` per place, `cells.<n>` per
    /// block.
    pub fn list_fields(&self) -> Vec<Field> {
        self.places
            .iter()
            .map(|(name, at)| {
                Field::new(
                    format!("place.{name}"),
                    at + PLACE_FIRST,
                    PLACE_END - PLACE_FIRST,
                )
            })
            .chain(
                self.blocks()
                    .iter()
                    .enumerate()
                    .map(|(n, (lo, hi, _))| Field::new(format!("cells.{n}"), *lo, hi - lo)),
            )
            .collect()
    }

    /// The places in one record: none when a field wasn't readable.
    pub fn read(&self, list: &WatchList, record: &WatchRecord) -> Option<Vec<PlaceAt>> {
        let mut numbers: BTreeMap<(&str, &str), Number> = BTreeMap::new();
        for (n, (lo, _, members)) in self.blocks().iter().enumerate() {
            let bytes = list.field(record, &format!("cells.{n}"))?;
            for &i in members {
                let (place, var, at) = &self.cells[i];
                let off = (at + VALUE_TYPE - lo) as usize;
                let value = if be32(bytes, off) == self.float_type {
                    Number::Float(f32_at(bytes, off + 4))
                } else {
                    Number::Int(be32(bytes, off + 4) as i32)
                };
                numbers.insert((place.as_str(), var.as_str()), value);
            }
        }
        let rel = |at: u32| (at - PLACE_FIRST) as usize;
        self.places
            .iter()
            .map(|(name, _)| {
                let b = list.field(record, &format!("place.{name}"))?;
                let f = |k: usize| f32_at(b, 4 * k);
                Some(PlaceAt {
                    name: name.clone(),
                    position: [f(0), f(1), f(2)],
                    rotation: [f(3), f(4), f(5)],
                    activity_state: be32(b, rel(script::PLACEMENT_STATE as u32)),
                    behaviour: be32(b, rel(script::PLACEMENT_BEHAVIOUR as u32)),
                    active_seconds: f32_at(b, rel(PLACEMENT_ACTIVE_TIME as u32)),
                    variables: numbers
                        .iter()
                        .filter(|((p, _), _)| *p == name.as_str())
                        .map(|((_, v), n)| ((*v).to_owned(), *n))
                        .collect(),
                })
            })
            .collect()
    }
}

/// The places record by record, beside a replay's `watch.json`: each record's
/// flip and places, the behaviours named once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacesFile {
    pub behaviours: BTreeMap<u32, String>,
    pub records: Vec<(u64, Vec<PlaceAt>)>,
}

impl PlacesFile {
    pub const FILE: &'static str = "places.json";

    /// Every record's places through `watch`, the behaviours they run named
    /// through `g` (at the end pause). Refused when a record's places weren't
    /// readable or a behaviour pointer isn't an `OpUserBehavior`.
    pub fn of(
        g: &mut impl Gather,
        watch: &PlaceWatch,
        list: &WatchList,
        records: &[WatchRecord],
    ) -> Result<Self> {
        let records: Vec<(u64, Vec<PlaceAt>)> = records
            .iter()
            .map(|r| {
                watch
                    .read(list, r)
                    .map(|p| (r.flip, p))
                    .with_context(|| format!("the places weren't readable at flip {}", r.flip))
            })
            .collect::<Result<_>>()?;
        let pointers: BTreeSet<u32> = records
            .iter()
            .flat_map(|(_, p)| p.iter().map(|a| a.behaviour))
            .filter(|&b| b != 0)
            .collect();
        Ok(Self {
            behaviours: census::behaviour_names(g, &pointers)?,
            records,
        })
    }
}

#[cfg(test)]
mod tests;
