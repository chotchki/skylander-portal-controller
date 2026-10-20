//! The session's generator (openspec AH.15, design decision 11): every roll
//! a script makes draws from one MT19937, as every roll the original's
//! scripts make draws from its one `igMersenneTwisterRandomNumber` (AF.8).
//!
//! - The state is 624 words and how many of them have been drawn since the
//!   last twist (624: the next draw twists first). A twist regenerates all
//!   624 words at once; a draw tempers one. The twist and the tempering are
//!   Matsumoto and Nishimura's, the same in every version they published.
//! - The seeding is the original's: `sgenrand` as their `mt19937int.c` of
//!   1999/10/28 has it (the design's "1998 seeding"): a `69069x + 1` LCG
//!   from the seed, each word the high half of one step and the high half of
//!   the next. Not the 2002 `init_genrand` (C++'s `std::mt19937`), and not
//!   the 1998/4/6 file's one full step a word: both give other streams from
//!   the same seed.
//! - One generator per session, seeded from the clock ([`session_seed`]).
//!   Tests seed it ([`Generator::seeded`]) or load a state an `rng peek` read
//!   at the oracle ([`Generator::from_state`]). The host hands the VM its
//!   next word ([`crate::Host::draw`]); what a roll makes of the word is
//!   [`crate::random_literal`]'s.
//!
//! The sequence is never compared with the original's: the original's
//! particles draw from the same stream on host time and guacamole's draw
//! nothing, so the two sides' rolls land at other places in it. What's
//! specified is the generator, each roll's range and that two sessions roll
//! differently.

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::value::{Value, random_literal};

/// MT19937's state, in 32-bit words.
const WORDS: usize = 624;
const SHIFT: usize = 397;
const MATRIX_A: u32 = 0x9908_B0DF;
const UPPER: u32 = 0x8000_0000;
const LOWER: u32 = 0x7FFF_FFFF;

/// A state that isn't one: what [`Generator::from_state`] refuses.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    #[error("{0} words drawn of the {WORDS} a state holds")]
    Drawn(u32),
    #[error("{0} words where MT19937 keeps {WORDS}")]
    Words(usize),
}

/// One MT19937 stream and where it stands.
#[derive(Clone, PartialEq, Eq)]
pub struct Generator {
    words: [u32; WORDS],
    /// The next word to draw; [`WORDS`] when the next draw twists first.
    next: usize,
}

impl fmt::Debug for Generator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Generator {{ drawn: {}, words: [{:#010x}, {:#010x}, .., {:#010x}] }}",
            self.next,
            self.words[0],
            self.words[1],
            self.words[WORDS - 1]
        )
    }
}

impl Generator {
    /// The original's seeding (`sgenrand`): leaves every word undrawn, so the
    /// first draw twists.
    pub fn seeded(seed: u32) -> Self {
        let step = |x: u32| x.wrapping_mul(69_069).wrapping_add(1);
        let mut words = [0u32; WORDS];
        let mut x = seed;
        for word in &mut words {
            let high = x & 0xFFFF_0000;
            x = step(x);
            *word = high | (x >> 16);
            x = step(x);
        }
        Self { words, next: WORDS }
    }

    /// A state as the oracle's `rng peek` reads it out of the original's
    /// memory: `_mti`, the words drawn since the last twist, and the 624
    /// words. Refused when `drawn` is past 624 (nothing leaves one) or the
    /// words aren't 624.
    pub fn from_state(drawn: u32, words: &[u32]) -> Result<Self, StateError> {
        let words: [u32; WORDS] = words
            .try_into()
            .map_err(|_| StateError::Words(words.len()))?;
        if drawn as usize > WORDS {
            return Err(StateError::Drawn(drawn));
        }
        Ok(Self {
            words,
            next: drawn as usize,
        })
    }

    /// Words drawn since the last twist, 0 to 624: the peek's `_mti`.
    pub fn drawn(&self) -> u32 {
        self.next as u32
    }

    pub fn words(&self) -> &[u32; WORDS] {
        &self.words
    }

    /// The next 32-bit number: what [`crate::Host::draw`] hands the VM.
    pub fn next_u32(&mut self) -> u32 {
        if self.next >= WORDS {
            self.twist();
        }
        let y = self.words[self.next];
        self.next += 1;
        temper(y)
    }

    /// `random(n)` on the next number ([`crate::random_literal`]): a whole
    /// number n rolls 0 to n with both ends, a fraction r from 0 up to r.
    /// One number a roll; anything but a number rolls nothing and draws
    /// nothing.
    pub fn random(&mut self, n: Value) -> Option<Value> {
        n.is_number()
            .then(|| random_literal(n, self.next_u32()))
            .flatten()
    }

    /// All 624 words regenerated in place, each from itself, the next and
    /// the one 397 on, in order: the last word reads the already new first.
    fn twist(&mut self) {
        let mt = &mut self.words;
        for i in 0..WORDS {
            let y = (mt[i] & UPPER) | (mt[(i + 1) % WORDS] & LOWER);
            let odd = if y & 1 == 1 { MATRIX_A } else { 0 };
            mt[i] = mt[(i + SHIFT) % WORDS] ^ (y >> 1) ^ odd;
        }
        self.next = 0;
    }
}

fn temper(mut y: u32) -> u32 {
    y ^= y >> 11;
    y ^= (y << 7) & 0x9D2C_5680;
    y ^= (y << 15) & 0xEFC6_0000;
    y ^ (y >> 18)
}

/// Seeds this process has made, so two sessions started within one tick of
/// the clock still differ.
static SESSIONS: AtomicU32 = AtomicU32::new(0);

/// A new session's seed: the wall clock's nanoseconds folded to 32 bits,
/// plus this process's count of seeds made times an odd constant. Two
/// sessions started in one tick of the clock never share a seed; any two
/// others do about one time in 2^32. The crate's one read of the world
/// outside it. The original seeds once per power-on from its timer (AF.8);
/// a session here is whatever the embedder makes a generator for.
pub fn session_seed() -> u32 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let folded = (nanos as u64) ^ ((nanos >> 64) as u64);
    let clock = (folded ^ (folded >> 32)) as u32;
    let made = SESSIONS.fetch_add(1, Ordering::Relaxed);
    clock.wrapping_add(made.wrapping_mul(0x9E37_79B9))
}

#[cfg(test)]
mod tests {
    use super::*;
    use guac_oracle::rng::Mt19937;

    fn draws(g: &mut Generator, n: usize) -> Vec<u32> {
        (0..n).map(|_| g.next_u32()).collect()
    }

    #[test]
    fn the_seeding_reproduces_the_published_output() {
        // mt19937int.c of 1999/10/28, whose `sgenrand` is the original's:
        // its published output for sgenrand(4357), the first five, the 624th
        // and 625th either side of the second twist, and the 1000th, its
        // last.
        let mut g = Generator::seeded(4357);
        assert_eq!(g.drawn(), 624);
        let out = draws(&mut g, 1000);
        assert_eq!(
            out[..5],
            [
                2_867_219_139,
                1_585_203_162,
                3_113_124_129,
                2_953_900_839,
                2_463_794_868
            ]
        );
        assert_eq!((out[623], out[624]), (589_403_970, 1_044_672_925));
        assert_eq!(out[999], 1_030_650_439);
    }

    #[test]
    fn the_twist_and_tempering_reproduce_the_2002_published_output() {
        // The 2002 reference's vector, its own seeding done by guac_oracle
        // and loaded here: seed 5489's first number and its 10,000th
        // (C++'s [rand.predef]). The twist and the tempering are this
        // crate's.
        let reference = Mt19937::init_genrand(5489);
        let mut g = Generator::from_state(reference.mti(), reference.words()).unwrap();
        assert_eq!(g.next_u32(), 3_499_211_612);
        let out = draws(&mut g, 9_999);
        assert_eq!(out[9_998], 4_123_659_995);
    }

    #[test]
    fn a_seeding_is_guac_oracles_word_for_word_and_draw_for_draw() {
        for seed in [0, 1, 4357, 5150, 10_554, 0xDEAD_BEEF, u32::MAX] {
            let mut g = Generator::seeded(seed);
            let mut reference = Mt19937::sgenrand(seed);
            assert_eq!(g.words(), reference.words(), "seed {seed}");
            assert_eq!(g.drawn(), reference.mti());
            // Five twists' worth, one at a time.
            for i in 0..5 * WORDS + 7 {
                assert_eq!(g.next_u32(), reference.next_u32(), "seed {seed}, draw {i}");
            }
            assert_eq!(g.drawn(), reference.mti());
            assert_eq!(g.words(), reference.words());
        }
    }

    #[test]
    fn a_peeked_state_continues_as_guac_oracles_does() {
        // The AF.5 state's numbers (seeded 10554, 104,201 drawn: `_mti`
        // 617), as a peek of it would hold them, and the census's other two
        // seeds at a block's end: every word drawn (624) and one short (623).
        for (seed, drawn) in [
            (10_554, 104_201),
            (11_179, 624 * 3),
            (11_421, 624 * 5 + 623),
        ] {
            let mut peek = Mt19937::sgenrand(seed);
            peek.advance(drawn);
            let mut g = Generator::from_state(peek.mti(), peek.words()).unwrap();
            assert_eq!(g.drawn(), peek.mti());
            for i in 0..2_000 {
                assert_eq!(g.next_u32(), peek.next_u32(), "{seed} at {drawn}, draw {i}");
            }
        }
        // A twisted block with nothing drawn yet (`_mti` 0), as a peek right
        // after a twist reads it.
        let mut peek = Mt19937::sgenrand(77);
        peek.twist();
        let mut g = Generator::from_state(0, peek.words()).unwrap();
        assert_eq!(
            draws(&mut g, 700),
            (0..700).map(|_| peek.next_u32()).collect::<Vec<_>>()
        );

        let words = *Mt19937::sgenrand(9).words();
        assert_eq!(
            Generator::from_state(625, &words),
            Err(StateError::Drawn(625))
        );
        assert_eq!(
            Generator::from_state(0, &words[..623]),
            Err(StateError::Words(623))
        );
        assert_eq!(
            Generator::from_state(624, &words).unwrap(),
            Generator::seeded(9)
        );
    }

    #[test]
    fn random_3_reaches_0_and_3_and_never_4() {
        let mut g = Generator::seeded(5150);
        let mut seen = [0u32; 4];
        for _ in 0..100_000 {
            match g.random(Value::Int(3)) {
                Some(Value::Int(n @ 0..=3)) => seen[n as usize] += 1,
                other => panic!("random(3) rolled {other:?}"),
            }
        }
        // Uniform: each within 2 % of 25,000 (a count's sd is 137).
        for (n, count) in seen.iter().enumerate() {
            assert!(
                (24_500..=25_500).contains(count),
                "{n} came up {count} times"
            );
        }
    }

    #[test]
    fn random_half_never_rolls_a_half() {
        // A million rolls: none reaches 0.5, the spread runs from under 1e-5
        // to within 1e-5 of it. The run holds none of the 128 words that read
        // 0.5 itself (value's tests pin that corner: one draw in 2^25).
        let mut g = Generator::seeded(5150);
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for _ in 0..1_000_000 {
            let Some(Value::Float(r)) = g.random(Value::Float(0.5)) else {
                panic!("random(0.5) rolled something else");
            };
            assert!((0.0..0.5).contains(&r), "random(0.5) rolled {r}");
            low = low.min(r);
            high = high.max(r);
        }
        assert!(low < 1e-5 && high > 0.5 - 1e-5, "{low} to {high}");
    }

    #[test]
    fn a_thousand_chompy_bites_land_as_the_spec_says() {
        // script-runtime's Chompy bites: 11 and a whole-number roll to 3, a
        // thousand times, gives each of 11 to 14 between 209 and 291 times
        // (the expected 250 +- 3 sd) and nothing else.
        for seed in [5150, 10_554, 11_179, 11_421] {
            let mut g = Generator::seeded(seed);
            let mut seen = [0u32; 4];
            for _ in 0..1_000 {
                let Some(Value::Int(n)) = g.random(Value::Int(3)) else {
                    panic!("random(3) rolled something else");
                };
                let bite = 11 + n;
                assert!((11..=14).contains(&bite), "a bite of {bite}");
                seen[(bite - 11) as usize] += 1;
            }
            for (i, count) in seen.iter().enumerate() {
                assert!(
                    (209..=291).contains(count),
                    "seed {seed}: {} came up {count} times",
                    11 + i
                );
            }
        }
    }

    #[test]
    fn a_roll_of_anything_but_a_number_draws_nothing() {
        let mut g = Generator::seeded(1);
        let before = g.clone();
        assert_eq!(g.random(Value::Color(0xFF00_00FF)), None);
        assert_eq!(g.random(Value::Null), None);
        assert_eq!(g, before);
        // A number draws exactly one.
        g.random(Value::Int(10));
        let mut one = before.clone();
        one.next_u32();
        assert_eq!(g, one);
    }

    #[test]
    fn two_sessions_roll_differently() {
        let (a, b) = (session_seed(), session_seed());
        assert_ne!(a, b);
        let (mut x, mut y) = (Generator::seeded(a), Generator::seeded(b));
        assert_ne!(draws(&mut x, 64), draws(&mut y, 64));
    }
}
