//! Giants' random number generators, read and written at a pause through PINE
//! (openspec AF.8, design decision 4): `oracle rng census|peek|seed|distance|origin`.
//!
//! What the census found in BLUS30968 01.00, and what a [`Peek`] covers:
//! - ONE `igMersenneTwisterRandomNumber`, the static `tfbApplication::_random`
//!   ([`MT_GLOBAL`] points at it), carries every gameplay roll and the
//!   particles', lightning's and sound variation's draws besides. MT19937,
//!   seeded with the 1998 `sgenrand` once per power-on from the time since the
//!   game's timer started; nothing reseeds it. Peeked: `_mti` and the 624 words.
//! - The animation networks' weighted clip picks step an LCG pair at
//!   [`ANIM_LCG`] (a seed word and the current word, Numerical Recipes'
//!   constants, both 0x12345 at power-on). Peeked.
//! - FMOD keeps an LCG word in its global struct, never drawn in any state read
//!   so far (0). Peeked as a check.
//! - Not peeked, audio only: a thread-local `rand()` FMOD's microphone init
//!   calls, and an LCG inside one of FMOD's SPU DSP programs, in SPU local
//!   store, which PINE can't reach.
//!
//! Every address here is the 01.00 EBOOT's, so a read checks PINE's title and
//! version first ([`check_title`]). Nothing game-specific lives in the O-series.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use guac_oracle::rng::{self, Lcg32, Mt19937, Origin};
use serde::{Deserialize, Serialize};

use crate::pine::PineClient;
use crate::scan::{self, Area, Found};

pub const TITLE_ID: &str = "BLUS30968";
/// The disc's `APP_VER`; an update's EBOOT would move every address below.
pub const APP_VERSION: &str = "01.00";

/// The slot holding `igMersenneTwisterRandomNumber`'s meta object, whose +0x18
/// counts the class's live instances and whose +0x20 is the class's vtable.
pub const MT_META_SLOT: u32 = 0x008F_09E8;
const META_INSTANCES: u32 = 0x18;
const META_VTABLE: u32 = 0x20;
/// `tfbApplication::_random`: the pointer to the one generator.
pub const MT_GLOBAL: u32 = 0x008E_1B9C;
/// `Utils::igMersenneTwisterRandomNumber`'s vtable, every instance's first word.
pub const MT_VTABLE: u32 = 0x0064_6638;
/// An instance: vtable, refcount, `_mti` (i32), `_mt[624]`, big-endian.
pub const MT_SIZE: usize = 0x9CC;
const MTI_AT: usize = 8;
const WORDS_AT: usize = 0xC;
/// The animation variety's LCG: the seed word, then the current one.
pub const ANIM_LCG: u32 = 0x008E_20F0;
/// What a static constructor puts in both words at power-on.
pub const ANIM_LCG_POWER_ON: u32 = 0x12345;
/// The pointer to FMOD's global struct, whose +0x28 is its LCG word.
pub const FMOD_GLOBAL: u32 = 0x0089_8760;
const FMOD_LCG_AT: u32 = 0x28;

/// How far [`Peek::apart`] steps the MT by default: 100,000 twists, 62.4 M draws.
pub const MAX_TWISTS: u64 = 100_000;
/// How far [`Peek::origin`] untwists by default: 20,000 twists, 12.5 M draws
/// (the states read so far sat 94 to 371 twists from their seeding).
pub const ORIGIN_TWISTS: u64 = 20_000;
/// How far the animation LCG is stepped: 2^24 steps (it had stepped 5 to 55).
pub const ANIM_STEPS: u64 = 1 << 24;

/// Guest memory as the RNG verbs see it: PINE's paused-only calls on the
/// oracle ([`PineClient::read_paused`], [`PineClient::write_paused`]), a map in
/// the unit tests.
pub trait GuestMemory {
    fn read(&mut self, addr: u32, len: usize) -> Result<Vec<u8>>;
    fn write(&mut self, addr: u32, bytes: &[u8]) -> Result<()>;
}

impl GuestMemory for PineClient {
    fn read(&mut self, addr: u32, len: usize) -> Result<Vec<u8>> {
        Ok(self.read_paused(addr, len)?)
    }

    fn write(&mut self, addr: u32, bytes: &[u8]) -> Result<()> {
        Ok(self.write_paused(addr, bytes)?)
    }
}

/// Refuses a game other than Giants 01.00, whose addresses these are.
pub fn check_title(pine: &mut PineClient) -> Result<()> {
    let id = pine.title_id()?;
    let version = pine.game_version()?;
    if id != TITLE_ID || version != APP_VERSION {
        bail!("PINE reports {id} {version}; the RNG addresses are {TITLE_ID} {APP_VERSION}'s");
    }
    Ok(())
}

fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn read32(guest: &mut impl GuestMemory, addr: u32) -> Result<u32> {
    Ok(be32(&guest.read(addr, 4)?, 0))
}

/// The animation LCG as it sits in memory: the word a reset would start from
/// and the one each clip pick steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimLcg {
    pub seed: u32,
    pub current: u32,
}

impl AnimLcg {
    /// Steps since power-on's 0x12345, within [`ANIM_STEPS`].
    pub fn steps_since_power_on(&self) -> Option<u64> {
        Lcg32::NUMERICAL_RECIPES.steps_between(ANIM_LCG_POWER_ON, self.current, ANIM_STEPS)
    }
}

/// Every generator a peek covers, read at one pause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "PeekRecord", try_from = "PeekRecord")]
pub struct Peek {
    /// The flip counter P1's `STATE` read at the pause, when the caller had it.
    pub flip: Option<u64>,
    /// The class's meta object and the live instances its count says (the
    /// census found 1; more would be generators a peek doesn't cover).
    pub meta: u32,
    pub instances: i32,
    /// `tfbApplication::_random`'s target and its refcount.
    pub instance: u32,
    pub refcount: u32,
    pub mt: Mt19937,
    pub anim: AnimLcg,
    /// FMOD's LCG word; `None` while FMOD's global isn't set up.
    pub fmod: Option<u32>,
}

/// How two peeks stand: numbers drawn from the MT between them (negative when
/// the second is the earlier one) and steps of the animation LCG, each `None`
/// when not within the bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Apart {
    pub mt: Option<i64>,
    pub anim: Option<i64>,
}

impl Peek {
    /// Reads every generator. Fails when the meta or the instance doesn't
    /// carry the class's vtable, or `_mti` is out of range: the reads would be
    /// of something else.
    pub fn read(guest: &mut impl GuestMemory) -> Result<Self> {
        let meta = read32(guest, MT_META_SLOT)?;
        let instance = read32(guest, MT_GLOBAL)?;
        if meta == 0 || instance == 0 {
            bail!(
                "no generator yet: the meta slot reads {meta:#010x} and tfbApplication::_random {instance:#010x}"
            );
        }
        let head = guest.read(meta, (META_VTABLE + 4) as usize)?;
        if be32(&head, META_VTABLE as usize) != MT_VTABLE {
            bail!(
                "the meta at {meta:#010x} names vtable {:#010x}, not igMersenneTwisterRandomNumber's {MT_VTABLE:#010x}",
                be32(&head, META_VTABLE as usize)
            );
        }
        let instances = be32(&head, META_INSTANCES as usize) as i32;
        let object = guest.read(instance, MT_SIZE)?;
        let mt = parse_instance(instance, &object)?;
        let lcg = guest.read(ANIM_LCG, 8)?;
        let fmod_global = read32(guest, FMOD_GLOBAL)?;
        let fmod = match fmod_global {
            0 => None,
            at => Some(read32(guest, at + FMOD_LCG_AT)?),
        };
        Ok(Self {
            flip: None,
            meta,
            instances,
            instance,
            refcount: be32(&object, 4),
            mt,
            anim: AnimLcg {
                seed: be32(&lcg, 0),
                current: be32(&lcg, 4),
            },
            fmod,
        })
    }

    /// The MT's `sgenrand` seed and the numbers drawn since, within `max_twists`.
    pub fn origin(&self, max_twists: u64) -> Option<Origin> {
        rng::sgenrand_origin(&self.mt, max_twists)
    }

    /// From `self` to `later`, trying both directions.
    pub fn apart(&self, later: &Peek, max_twists: u64) -> Apart {
        let signed = |fwd: Option<u64>, back: Option<u64>| {
            fwd.map(|n| n as i64).or_else(|| back.map(|n| -(n as i64)))
        };
        let lcg = Lcg32::NUMERICAL_RECIPES;
        Apart {
            mt: signed(
                rng::draws_between(&self.mt, &later.mt, max_twists),
                rng::draws_between(&later.mt, &self.mt, max_twists),
            ),
            anim: signed(
                lcg.steps_between(self.anim.current, later.anim.current, ANIM_STEPS),
                lcg.steps_between(later.anim.current, self.anim.current, ANIM_STEPS),
            ),
        }
    }

    /// One line for a log.
    pub fn summary(&self) -> String {
        let words = self.mt.words();
        format!(
            "MT at {:#010x} ({} live, refcount {}): mti {} words [{:#010x} {:#010x} .. {:#010x}]; \
             animation LCG {:#010x} (seed word {:#010x}, {}); FMOD LCG {}",
            self.instance,
            self.instances,
            self.refcount,
            self.mt.mti(),
            words[0],
            words[1],
            words[rng::N - 1],
            self.anim.current,
            self.anim.seed,
            self.anim
                .steps_since_power_on()
                .map_or("not within reach of 0x12345".into(), |n| format!(
                    "{n} steps since 0x12345"
                )),
            self.fmod.map_or("unset".into(), |w| format!("{w:#x}")),
        )
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a peek serialises")
    }

    pub fn from_json(text: &str) -> Result<Self> {
        Ok(serde_json::from_str(text)?)
    }

    /// Writes the peek to `path`, refused inside any work tree: it's guest
    /// memory.
    pub fn write(&self, path: &Path) -> Result<PathBuf> {
        let path = rrc_dump::output::refuse_repo_output(path)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        std::fs::write(&path, self.to_json())
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_json(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

fn parse_instance(at: u32, object: &[u8]) -> Result<Mt19937> {
    if be32(object, 0) != MT_VTABLE {
        bail!(
            "the object at {at:#010x} has vtable {:#010x}, not igMersenneTwisterRandomNumber's {MT_VTABLE:#010x}",
            be32(object, 0)
        );
    }
    let mti = be32(object, MTI_AT);
    let words: Vec<u32> = object[WORDS_AT..MT_SIZE]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_be_bytes(*w))
        .collect();
    Mt19937::from_parts(mti, &words).with_context(|| format!("the generator at {at:#010x}"))
}

/// The bytes from an instance's +8 on: `_mti`, then the 624 words.
fn state_bytes(mt: &Mt19937) -> Vec<u8> {
    let mut out = Vec::with_capacity(MT_SIZE - MTI_AT);
    out.extend_from_slice(&mt.mti().to_be_bytes());
    for w in mt.words() {
        out.extend_from_slice(&w.to_be_bytes());
    }
    out
}

/// Writes `state` (`_mti` and the 624 words) into the generator
/// `tfbApplication::_random` points at, once its vtable checks out. Returns
/// the instance's address.
pub fn write_mt(guest: &mut impl GuestMemory, state: &Mt19937) -> Result<u32> {
    let instance = read32(guest, MT_GLOBAL)?;
    let vtable = read32(guest, instance)?;
    if instance == 0 || vtable != MT_VTABLE {
        bail!(
            "tfbApplication::_random points at {instance:#010x} (vtable {vtable:#010x}); not writing a generator there"
        );
    }
    guest.write(instance + MTI_AT as u32, &state_bytes(state))?;
    Ok(instance)
}

/// `sgenrand(seed)` into the generator, `_mti` 624, as the game's own seeding
/// leaves it. The animation LCG is left alone.
pub fn seed(guest: &mut impl GuestMemory, seed: u32) -> Result<u32> {
    write_mt(guest, &Mt19937::sgenrand(seed))
}

/// Puts back what `peek` read: the MT and both animation LCG words. Refused
/// when the generator has moved to another address since.
pub fn restore(guest: &mut impl GuestMemory, peek: &Peek) -> Result<()> {
    let instance = read32(guest, MT_GLOBAL)?;
    if instance != peek.instance {
        bail!(
            "the generator is at {instance:#010x} now and the peek read it at {:#010x}",
            peek.instance
        );
    }
    write_mt(guest, &peek.mt)?;
    let mut lcg = peek.anim.seed.to_be_bytes().to_vec();
    lcg.extend_from_slice(&peek.anim.current.to_be_bytes());
    guest.write(ANIM_LCG, &lcg)
}

/// A word equal to the class's vtable, and what it turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// An object: refcount at least 1 and `_mti` in range.
    Instance { at: u32, refcount: u32, mti: u32 },
    /// The meta object's own vtable field, which isn't an object.
    MetaField { at: u32 },
    /// Neither.
    Other { at: u32 },
}

/// The live census: the peek, every word in [`scan::PPU_AREAS`] equal to the
/// vtable and, for each instance among them, the words pointing at it (its
/// owners).
#[derive(Debug, Clone)]
pub struct Census {
    pub peek: Peek,
    pub hits: Vec<Hit>,
    pub owners: BTreeMap<u32, Vec<u32>>,
    /// The vtable search and the owner search.
    pub searches: [Found; 2],
}

/// Peeks, then searches [`scan::PPU_AREAS`]-style `areas` twice: for the
/// vtable, and for each instance's address. PINE must report a pause
/// throughout.
pub fn census(pine: &mut PineClient, areas: &[Area]) -> Result<Census> {
    check_title(pine)?;
    let peek = Peek::read(pine)?;
    let vtables = scan::find_words(pine, areas, &BTreeSet::from([MT_VTABLE]))?;
    let mut hits = Vec::new();
    for &at in vtables.hits.get(&MT_VTABLE).into_iter().flatten() {
        hits.push(if at == peek.meta + META_VTABLE {
            Hit::MetaField { at }
        } else {
            let head = pine.read(at, 12)?;
            let (refcount, mti) = (be32(&head, 4), be32(&head, MTI_AT));
            if refcount >= 1 && mti as usize <= rng::N {
                Hit::Instance { at, refcount, mti }
            } else {
                Hit::Other { at }
            }
        });
    }
    let instances: BTreeSet<u32> = hits
        .iter()
        .filter_map(|h| match h {
            Hit::Instance { at, .. } => Some(*at),
            _ => None,
        })
        .collect();
    let referrers = if instances.is_empty() {
        Found::default()
    } else {
        scan::find_words(pine, areas, &instances)?
    };
    let owners = instances
        .iter()
        .map(|i| (*i, referrers.hits.get(i).cloned().unwrap_or_default()))
        .collect();
    Ok(Census {
        peek,
        hits,
        owners,
        searches: [vtables, referrers],
    })
}

/// A peek as it's written to a file: plain numbers.
#[derive(Serialize, Deserialize)]
struct PeekRecord {
    flip: Option<u64>,
    meta: u32,
    instances: i32,
    instance: u32,
    refcount: u32,
    mti: u32,
    mt: Vec<u32>,
    anim: AnimLcg,
    fmod: Option<u32>,
}

impl From<Peek> for PeekRecord {
    fn from(p: Peek) -> Self {
        Self {
            flip: p.flip,
            meta: p.meta,
            instances: p.instances,
            instance: p.instance,
            refcount: p.refcount,
            mti: p.mt.mti(),
            mt: p.mt.words().to_vec(),
            anim: p.anim,
            fmod: p.fmod,
        }
    }
}

impl TryFrom<PeekRecord> for Peek {
    type Error = rng::RngError;

    fn try_from(r: PeekRecord) -> Result<Self, Self::Error> {
        Ok(Self {
            flip: r.flip,
            meta: r.meta,
            instances: r.instances,
            instance: r.instance,
            refcount: r.refcount,
            mt: Mt19937::from_parts(r.mti, &r.mt)?,
            anim: r.anim,
            fmod: r.fmod,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guest memory as a map of regions, big-endian like the PS3's.
    #[derive(Default)]
    struct Fake {
        regions: BTreeMap<u32, Vec<u8>>,
        writes: Vec<(u32, usize)>,
    }

    impl Fake {
        fn put(&mut self, addr: u32, bytes: &[u8]) {
            self.regions.insert(addr, bytes.to_vec());
        }

        fn slot(&mut self, addr: u32, len: usize) -> Result<&mut [u8]> {
            let (base, bytes) = self
                .regions
                .range_mut(..=addr)
                .next_back()
                .with_context(|| format!("unmapped {addr:#x}"))?;
            let at = (addr - base) as usize;
            bytes
                .get_mut(at..at + len)
                .with_context(|| format!("unmapped {addr:#x}+{len}"))
        }
    }

    impl GuestMemory for Fake {
        fn read(&mut self, addr: u32, len: usize) -> Result<Vec<u8>> {
            Ok(self.slot(addr, len)?.to_vec())
        }

        fn write(&mut self, addr: u32, bytes: &[u8]) -> Result<()> {
            self.slot(addr, bytes.len())?.copy_from_slice(bytes);
            self.writes.push((addr, bytes.len()));
            Ok(())
        }
    }

    const META: u32 = 0x3028_E734;
    const INSTANCE: u32 = 0x3028_E7F0;

    /// Giants' layout with the AF.5 state's shape: seed 10554, 104,201 drawn,
    /// the LCG 13 steps on, FMOD's word 0.
    fn giants() -> (Fake, Mt19937, u32) {
        let mut mt = Mt19937::sgenrand(10_554);
        mt.advance(104_201);
        let mut lcg = ANIM_LCG_POWER_ON;
        for _ in 0..13 {
            lcg = Lcg32::NUMERICAL_RECIPES.step(lcg);
        }
        let mut g = Fake::default();
        g.put(MT_META_SLOT, &META.to_be_bytes());
        g.put(MT_GLOBAL, &INSTANCE.to_be_bytes());
        let mut meta = vec![0u8; 0x24];
        meta[0x18..0x1C].copy_from_slice(&1u32.to_be_bytes());
        meta[0x20..0x24].copy_from_slice(&MT_VTABLE.to_be_bytes());
        g.put(META, &meta);
        let mut object = MT_VTABLE.to_be_bytes().to_vec();
        object.extend_from_slice(&1u32.to_be_bytes());
        object.extend_from_slice(&state_bytes(&mt));
        assert_eq!(object.len(), MT_SIZE);
        g.put(INSTANCE, &object);
        let mut words = ANIM_LCG_POWER_ON.to_be_bytes().to_vec();
        words.extend_from_slice(&lcg.to_be_bytes());
        g.put(ANIM_LCG, &words);
        g.put(FMOD_GLOBAL, &0x0090_0000u32.to_be_bytes());
        g.put(0x0090_0000, &[0; 0x40]);
        (g, mt, lcg)
    }

    #[test]
    fn a_peek_reads_every_generator_out_of_giants_layout() {
        let (mut g, mt, lcg) = giants();
        let peek = Peek::read(&mut g).unwrap();
        assert_eq!(
            peek,
            Peek {
                flip: None,
                meta: META,
                instances: 1,
                instance: INSTANCE,
                refcount: 1,
                mt,
                anim: AnimLcg {
                    seed: ANIM_LCG_POWER_ON,
                    current: lcg
                },
                fmod: Some(0),
            }
        );
        assert_eq!(peek.anim.steps_since_power_on(), Some(13));
        assert_eq!(
            peek.origin(ORIGIN_TWISTS),
            Some(Origin {
                seed: 10_554,
                draws: 104_201
            })
        );
        // 104,201 = 166 x 624 + 617, the AF.5 state's _mti.
        assert_eq!(peek.mt.mti(), 617);
        // FMOD's global not up yet.
        g.put(FMOD_GLOBAL, &[0; 4]);
        assert_eq!(Peek::read(&mut g).unwrap().fmod, None);
    }

    #[test]
    fn a_peek_refuses_what_isnt_the_generator() {
        let (mut g, ..) = giants();
        g.slot(INSTANCE, 4)
            .unwrap()
            .copy_from_slice(&[0, 0x64, 0x66, 0x3C]);
        let err = Peek::read(&mut g).unwrap_err().to_string();
        assert!(err.contains("vtable 0x0064663c"), "{err}");

        let (mut g, ..) = giants();
        g.slot(META + 0x20, 4).unwrap().copy_from_slice(&[0; 4]);
        assert!(
            Peek::read(&mut g)
                .unwrap_err()
                .to_string()
                .contains("the meta")
        );

        let (mut g, ..) = giants();
        g.slot(INSTANCE + 8, 4)
            .unwrap()
            .copy_from_slice(&625u32.to_be_bytes());
        let err = format!("{:#}", Peek::read(&mut g).unwrap_err());
        assert!(err.contains("mti 625"), "{err}");

        let (mut g, ..) = giants();
        g.slot(MT_GLOBAL, 4).unwrap().copy_from_slice(&[0; 4]);
        assert!(
            Peek::read(&mut g)
                .unwrap_err()
                .to_string()
                .contains("no generator")
        );
        // A refused read has to leave the guest as it was.
        assert!(g.writes.is_empty());
    }

    #[test]
    fn seeding_writes_the_1998_state_and_a_restore_puts_the_peek_back() {
        let (mut g, ..) = giants();
        let before = Peek::read(&mut g).unwrap();
        assert_eq!(seed(&mut g, 5150).unwrap(), INSTANCE);
        // One write: _mti then the 624 words, from the instance's +8.
        assert_eq!(g.writes, [(INSTANCE + 8, 4 + 624 * 4)]);
        let seeded = Peek::read(&mut g).unwrap();
        assert_eq!(seeded.mt, Mt19937::sgenrand(5150));
        assert_eq!(seeded.mt.mti(), 624);
        assert_eq!(seeded.anim, before.anim, "seeding leaves the LCG alone");
        assert_eq!(seeded.refcount, 1);
        assert_eq!(
            seeded.origin(0),
            Some(Origin {
                seed: 5150,
                draws: 0
            })
        );

        restore(&mut g, &before).unwrap();
        assert_eq!(Peek::read(&mut g).unwrap(), before);
        // A peek of a generator that has since moved isn't written back.
        let mut moved = before.clone();
        moved.instance += 0x10;
        assert!(restore(&mut g, &moved).is_err());
        // Nor is a state written where the pointer leads to something else.
        g.slot(INSTANCE, 4).unwrap().copy_from_slice(&[0; 4]);
        let writes = g.writes.len();
        assert!(seed(&mut g, 1).is_err());
        assert_eq!(g.writes.len(), writes);
    }

    #[test]
    fn two_peeks_measure_the_draws_and_steps_between_them() {
        let (mut g, mut mt, mut lcg) = giants();
        let first = Peek::read(&mut g).unwrap();
        // The game draws 1,000 numbers (across a twist) and steps the LCG twice.
        mt.advance(1_000);
        for _ in 0..2 {
            lcg = Lcg32::NUMERICAL_RECIPES.step(lcg);
        }
        write_mt(&mut g, &mt).unwrap();
        g.slot(ANIM_LCG + 4, 4)
            .unwrap()
            .copy_from_slice(&lcg.to_be_bytes());
        let later = Peek::read(&mut g).unwrap();
        assert_eq!(
            first.apart(&later, MAX_TWISTS),
            Apart {
                mt: Some(1_000),
                anim: Some(2)
            }
        );
        assert_eq!(
            later.apart(&first, MAX_TWISTS),
            Apart {
                mt: Some(-1_000),
                anim: Some(-2)
            }
        );
        assert_eq!(first.apart(&later, 1).mt, None);
        assert_eq!(later.origin(ORIGIN_TWISTS).unwrap().draws, 105_201);
    }

    #[test]
    fn a_peek_round_trips_through_its_file() {
        let (mut g, ..) = giants();
        let mut peek = Peek::read(&mut g).unwrap();
        peek.flip = Some(602);
        let text = peek.to_json();
        assert!(text.contains("\"flip\": 602"), "{text}");
        assert_eq!(Peek::from_json(&text).unwrap(), peek);
        let short = text.replacen("\"mti\": ", "\"mti\": 1", 1);
        assert!(
            Peek::from_json(&short).is_err(),
            "an mti past 624 is refused"
        );
    }
}
