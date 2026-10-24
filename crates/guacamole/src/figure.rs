//! The figure in the scripts (openspec AH.23, design decision 9): the data
//! the original's scripts read and write a figure's progress through, made
//! from the values the TV holds for it when its scripts join and read back
//! after every step. No file, no encryption, no checksum: nothing here
//! reaches a `.sky` or RPCS3.
//!
//! - The layout ([`Layout`]): where experience, gold and playtime sit in a
//!   tag's save area, from the disc's own save-param table as AH.2 reads it
//!   statically. The area is 32-bit words read as one little-endian bit
//!   stream, as the scripts' bit-field macros read it; experience runs in
//!   two areas, the first up to its row's limit and the rest in the
//!   spillover's, summed as `Save - Portal - Get Portal Param Total Value`
//!   sums them.
//! - The tag on the portal ([`put_on_portal`]), in place of the portal's tag
//!   read and its save control's tag handling, neither of which runs
//!   (stand-in ledger: `join`): a `Portal - Tag Data` grown in the save
//!   control's `tag data` as its tag handling grows one, its save sets made
//!   by the disc's own `Save - Set up Tag Data Fields` over the portal's two
//!   regions ([`REGION_1`], [`REGION_2`]), the figure's words put in (the
//!   rest a fresh figure's zeros), its toy id, serial and subtype, its load
//!   done; then handed to the first player's slot as the portal manager
//!   hands a new tag to a free slot (the slot's `tag data`, and its `tag`'s
//!   `rfid` and `prodid`). The character file's definition then links the
//!   minion's description to it itself (`Portal - Define Minion`).
//! - Its entry ([`enter`]): the links `Portal - Enter Object` makes before
//!   the entry beam activates the minion, the rest of that macro not run:
//!   the slot's object info's `tag data`, the first player's `loaded data`
//!   and the slot's `entered`. From then on both awards (`Game - Award
//!   Experience` and its money twin) and every `Save - Get/Set Minion Save
//!   Val` reach the words.
//! - Reading back ([`read`]): the words through the layout.

use guac_ir::progression::{Progression, SaveKind};
use guac_script::{Halt, Host, Value, Vm};

use crate::scripts::{ScriptWorld, outside, portal_manager, slot_zero};

/// A figure's tag's first data region in bytes, as the portal answers
/// `size of region 1`: AH.11's live `save data area` holds 44 words, 12 of
/// them past the first region's 16 and 12 past the second's 4, so 112 and
/// 64 bytes.
pub const REGION_1: i32 = 112;
/// The second region, the sky2 area experience spills into ([`REGION_1`]
/// has the derivation).
pub const REGION_2: i32 = 64;

/// Where one value sits in the save area: its first bit in the area's
/// stream (32 × word + bit) and its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub bit: u32,
    pub width: u32,
}

impl Field {
    /// Bounds the save area a layout needs: a field may run on into the
    /// next word.
    fn last_word(self) -> usize {
        ((self.bit + self.width - 1) / 32) as usize
    }

    fn mask(self) -> u64 {
        (1u64 << self.width) - 1
    }

    /// Read across into the next word when the field runs on, as the
    /// scripts' getter reads it.
    fn get(self, words: &[u32]) -> u32 {
        let (w, b) = ((self.bit / 32) as usize, self.bit % 32);
        let pair = u64::from(words[w]) | words.get(w + 1).map_or(0, |&n| u64::from(n) << 32);
        ((pair >> b) & self.mask()) as u32
    }

    /// `v` into `words`, held to the width's top as `Save - Bit Field - Set
    /// Bit Field` holds a value past it, the rest of each word kept.
    fn put(self, words: &mut [u32], v: u32) {
        let v = u64::from(v).min(self.mask());
        let (w, b) = ((self.bit / 32) as usize, self.bit % 32);
        let mut pair = u64::from(words[w]) | words.get(w + 1).map_or(0, |&n| u64::from(n) << 32);
        pair = (pair & !(self.mask() << b)) | (v << b);
        words[w] = pair as u32;
        if let Some(next) = words.get_mut(w + 1) {
            *next = (pair >> 32) as u32;
        }
    }
}

/// Where a figure's experience, gold and playtime sit in its save area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The first area experience fills.
    pub experience: Field,
    /// The second area it runs on into, and how much the first holds before
    /// it does (its row's limit), when the table has one.
    pub spillover: Option<(Field, u32)>,
    pub gold: Field,
    pub playtime: Field,
}

/// A figure's values as the TV and the store hold them: experience as a
/// total, gold, playtime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Values {
    pub xp: u32,
    pub gold: u32,
    pub playtime_secs: u32,
}

impl Layout {
    /// The layout AH.2's rows give: refused, naming what's missing, when the
    /// table has no plain `pp_Exp`, `pp_Money` or `pp_CumulativeTime` row, a
    /// spillover with no limit on the first area, or a width outside 1-32.
    pub fn of(p: &Progression) -> Result<Self, String> {
        let row = |param: &str, kind: SaveKind| {
            p.save_params
                .iter()
                .find(|s| s.param == param && s.kind == kind)
        };
        let field = |param: &str, kind: SaveKind| -> Result<Option<Field>, String> {
            let Some(r) = row(param, kind) else {
                return Ok(None);
            };
            let f = r
                .fields
                .first()
                .ok_or_else(|| format!("the {kind:?} `{param}` row has no field"))?;
            if r.width == 0 || r.width > 32 {
                return Err(format!(
                    "the {kind:?} `{param}` row is {} bits wide",
                    r.width
                ));
            }
            Ok(Some(Field {
                bit: 32 * f.word + f.bit,
                width: r.width,
            }))
        };
        let need = |param: &str| {
            field(param, SaveKind::Plain)?
                .ok_or_else(|| format!("AH.2's tables have no plain `{param}` row"))
        };
        let experience = need("pp_Exp")?;
        let spillover = match field("pp_Exp", SaveKind::Spillover)? {
            None => None,
            Some(f) => match row("pp_Exp", SaveKind::Plain).map(|r| r.limit) {
                Some(limit) if limit > 0 => Some((f, limit)),
                _ => {
                    return Err(
                        "`pp_Exp` runs on into a spillover with no limit on its first area".into(),
                    );
                }
            },
        };
        Ok(Self {
            experience,
            spillover,
            gold: need("pp_Money")?,
            playtime: need("pp_CumulativeTime")?,
        })
    }

    /// How many words the save area has to hold for every field.
    pub fn words(&self) -> usize {
        [Some(self.experience), self.spillover.map(|(f, _)| f)]
            .into_iter()
            .flatten()
            .chain([self.gold, self.playtime])
            .map(Field::last_word)
            .max()
            .unwrap_or(0)
            + 1
    }

    /// `v` into `words` (at least [`Layout::words`] long), every other bit
    /// kept: experience split as `Save - Portal - Set Portal Param Total
    /// Value` splits it, the first area up to its limit and the rest in the
    /// spillover; each value held to its width.
    pub fn pack(&self, v: Values, words: &mut [u32]) {
        match self.spillover {
            Some((spill, limit)) => {
                let first = v.xp.min(limit);
                self.experience.put(words, first);
                spill.put(words, v.xp - first);
            }
            None => self.experience.put(words, v.xp),
        }
        self.gold.put(words, v.gold);
        self.playtime.put(words, v.playtime_secs);
    }

    /// The values out of `words` (at least [`Layout::words`] long):
    /// experience both areas summed.
    pub fn unpack(&self, words: &[u32]) -> Values {
        let spill = self.spillover.map_or(0, |(f, _)| f.get(words));
        Values {
            xp: self.experience.get(words).saturating_add(spill),
            gold: self.gold.get(words),
            playtime_secs: self.playtime.get(words),
        }
    }
}

/// What a figure's tag holds as the portal reads it: the kind of toy, its
/// subtype (the figure's variant), a serial standing for the toy's own
/// (each instance its own for the session) and its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    pub toy_id: u32,
    pub subtype: u32,
    pub serial: u32,
    pub values: Values,
}

/// The figure's tag on the portal, handed to the first player's slot (the
/// module docs): returns its `Portal - Tag Data`, which [`read`] reads back.
/// Refused, naming why, when the program lacks the shapes it builds on (the
/// save control, `Save - Set up Tag Data Fields`, a save area the layout
/// fits, the slot's `tag data`).
pub fn put_on_portal(
    vm: &mut Vm,
    host: &mut dyn Host,
    layout: &Layout,
    tag: &Tag,
) -> Result<Value, Halt> {
    let g = vm
        .program()
        .global_named("portal save control")
        .ok_or_else(|| outside("no portal save control".into()))?;
    let control = vm
        .global(g)
        .ok_or_else(|| outside("the portal save control reads nothing".into()))?;
    let tags = vm
        .field(control, "tag data")
        .ok_or_else(|| outside("the portal save control keeps no tag data".into()))?;
    let data = vm
        .grow(tags, host)
        .ok_or_else(|| outside("the portal save control's tag data didn't grow".into()))?;
    vm.set_field(data, "portal tag index", Value::Int(0));
    vm.call_with(
        None,
        "Save - Set up Tag Data Fields",
        &[("set up tag", data)],
        &[],
        host,
    )?;
    let cells: Vec<Value> = vm
        .field(data, "save data area")
        .and_then(|a| vm.set(a))
        .ok_or_else(|| outside("the tag data has no save area".into()))?
        .to_vec();
    if cells.len() < layout.words() {
        return Err(outside(format!(
            "the save area holds {} words, the layout reads {}",
            cells.len(),
            layout.words()
        )));
    }
    let mut words = vec![0u32; cells.len()];
    layout.pack(tag.values, &mut words);
    for (&c, &w) in cells.iter().zip(&words) {
        if !vm.put(c, Value::Int(w as i32)) {
            return Err(outside(format!("a save word isn't a number ({c:?})")));
        }
    }
    for (name, v) in [
        ("toy id", tag.toy_id),
        ("toy serial", tag.serial),
        ("toy subtype", tag.subtype),
        ("initial load complete", 1),
    ] {
        vm.set_field(data, name, Value::Int(v as i32));
    }
    let loaded = slot_zero(vm)?;
    if !vm.set_reference(loaded, "tag data", data) {
        return Err(outside("the first player's slot keeps no tag data".into()));
    }
    if let Some(pair) = vm.field(loaded, "tag") {
        vm.set_field(pair, "rfid", Value::Int(tag.serial as i32));
        vm.set_field(pair, "prodid", Value::Int(tag.toy_id as i32));
    }
    Ok(data)
}

/// `Portal - Enter Object`'s links for the first player's slot (the module
/// docs), made before the minion activates: its object info's `tag data`
/// when that's unset, the first player's `loaded data` (the portal
/// manager's `player data` at the slot's owner) and its `entered`. Each is
/// made where the program has its place, so a part whose slot keeps no tag
/// links nothing. True when the slot holds a tag.
pub fn enter(vm: &mut Vm, world: &ScriptWorld) -> Result<bool, Halt> {
    let loaded = slot_zero(vm)?;
    let data = vm
        .field(loaded, "tag data")
        .map_or(Value::Null, |v| vm.value(v));
    if data == Value::Null {
        return Ok(false);
    }
    if let Some(info) = vm.field(loaded, "object info").map(|v| vm.value(v))
        && vm.field(info, "tag data").map(|v| vm.value(v)) == Some(Value::Null)
    {
        vm.set_reference(info, "tag data", data);
    }
    let pm = portal_manager(vm, world)?;
    let owner = vm
        .field(loaded, "owner")
        .and_then(|v| vm.value(v).as_i32())
        .unwrap_or(0);
    let player = vm
        .local(pm, "player data")
        .and_then(|s| vm.set(s))
        .and_then(|s| s.get(usize::try_from(owner).ok()?).copied());
    if let Some(player) = player {
        vm.set_reference(player, "loaded data", loaded);
    }
    vm.set_field(loaded, "entered", Value::Int(1));
    Ok(true)
}

/// The values in the tag data [`put_on_portal`] made, through `layout`;
/// `None` when its save area no longer holds the layout's words.
pub fn read(vm: &Vm, layout: &Layout, data: Value) -> Option<Values> {
    let cells = vm.field(data, "save data area").and_then(|a| vm.set(a))?;
    if cells.len() < layout.words() {
        return None;
    }
    let words: Vec<u32> = cells
        .iter()
        .map(|&c| vm.value(c).as_i32().unwrap_or(0) as u32)
        .collect();
    Some(layout.unpack(&words))
}

#[cfg(test)]
mod tests {
    use guac_ir::progression::{SaveField, SaveParam};

    use super::*;

    fn row(param: &str, kind: SaveKind, word: u32, bit: u32, width: u32, limit: u32) -> SaveParam {
        SaveParam {
            param: param.into(),
            kind,
            fields: vec![SaveField {
                word,
                bit,
                member: None,
            }],
            width,
            bits_per_field: 0,
            limit,
        }
    }

    /// Giants' rows as AH.2 reads them: experience in the first area's 24
    /// bits to 33,000 and on into the second's 16; gold across words 0
    /// and 1; playtime across words 1 and 2.
    pub(crate) fn giants() -> Progression {
        Progression {
            identity: guac_ir::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([0; 32]),
            },
            level_xp: vec![
                0, 1000, 2200, 3800, 6000, 9000, 13000, 18200, 24800, 33000, 42700, 53900, 66600,
                80800, 96500,
            ],
            save_params: vec![
                row("pp_Exp", SaveKind::Plain, 0, 0, 24, 33_000),
                row("pp_Money", SaveKind::Plain, 0, 24, 16, 65_535),
                row("pp_CumulativeTime", SaveKind::Plain, 1, 8, 32, 0),
                row("pp_Hat", SaveKind::Plain, 5, 0, 16, 45),
                row("pp_Exp", SaveKind::Spillover, 28, 24, 16, 63_500),
            ],
            gold_cap: 65_000,
        }
    }

    /// AH.23's round trips: experience at 0, 32,999, 33,000, 33,001 and
    /// 96,500 across the two areas, gold at the clamp (its low byte's top
    /// bit set, so word 0 reads negative as the scripts hold it), playtime
    /// across words 1 and 2, every other bit kept.
    #[test]
    fn experience_round_trips_across_the_two_areas_and_gold_at_the_clamp() {
        let layout = Layout::of(&giants()).unwrap();
        assert_eq!(layout.words(), 30, "word 29 holds the spillover's top");
        for (xp, first, second) in [
            (0, 0, 0),
            (32_999, 32_999, 0),
            (33_000, 33_000, 0),
            (33_001, 33_000, 1),
            (96_500, 33_000, 63_500),
        ] {
            let v = Values {
                xp,
                gold: 65_000,
                playtime_secs: 0x0123_4567,
            };
            let mut words = vec![0u32; 44];
            words[5] = 0xDEAD_BEEF;
            words[43] = 7;
            layout.pack(v, &mut words);
            assert_eq!(words[0] & 0x00FF_FFFF, first, "{xp}: the first area");
            assert_eq!(
                (words[28] >> 24) | ((words[29] & 0xFF) << 8),
                second,
                "{xp}: the second"
            );
            assert_eq!(
                (words[0] >> 24) | ((words[1] & 0xFF) << 8),
                65_000,
                "gold across words 0 and 1"
            );
            assert!((words[0] as i32) < 0, "65,000's low byte sets bit 31");
            assert_eq!((words[1] >> 8) | (words[2] << 24), 0x0123_4567);
            assert_eq!((words[5], words[43]), (0xDEAD_BEEF, 7), "the rest kept");
            assert_eq!(layout.unpack(&words), v, "{xp}");
        }
    }

    /// A value past its width stops at the width's top, as the scripts'
    /// bit-field setter holds it; a figure's own words past the second
    /// area's 63,500 (the original writes 96,506, AH.52) read back as they
    /// are.
    #[test]
    fn a_value_past_its_width_stops_at_the_top_and_the_spillover_reads_whole() {
        let layout = Layout::of(&giants()).unwrap();
        let mut words = vec![0u32; 44];
        layout.pack(
            Values {
                xp: 200_000,
                gold: 70_000,
                playtime_secs: u32::MAX,
            },
            &mut words,
        );
        let v = layout.unpack(&words);
        assert_eq!(v.xp, 33_000 + 65_535);
        assert_eq!((v.gold, v.playtime_secs), (65_535, u32::MAX));
        layout.pack(Values { xp: 96_506, ..v }, &mut words);
        assert_eq!(layout.unpack(&words).xp, 96_506);
    }

    /// The layout names what the table lacks.
    #[test]
    fn a_table_missing_a_row_is_refused_by_name() {
        for param in ["pp_Exp", "pp_Money", "pp_CumulativeTime"] {
            let mut p = giants();
            p.save_params
                .retain(|s| !(s.param == param && s.kind == SaveKind::Plain));
            let err = Layout::of(&p).unwrap_err();
            assert!(err.contains(param), "{err}");
        }
        let mut p = giants();
        p.save_params[0].limit = 0;
        assert!(Layout::of(&p).unwrap_err().contains("spillover"));
        let mut p = giants();
        p.save_params[1].width = 33;
        assert!(Layout::of(&p).unwrap_err().contains("33 bits"));
        // No spillover row: experience in one area, held to its width.
        let mut p = giants();
        p.save_params.retain(|s| s.kind != SaveKind::Spillover);
        let layout = Layout::of(&p).unwrap();
        assert_eq!(layout.spillover, None);
        assert_eq!(layout.words(), 3);
        let mut words = vec![0u32; 3];
        layout.pack(
            Values {
                xp: 96_500,
                ..Values::default()
            },
            &mut words,
        );
        assert_eq!(layout.unpack(&words).xp, 96_500);
    }
}
