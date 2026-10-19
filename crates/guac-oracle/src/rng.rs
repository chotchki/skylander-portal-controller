//! The random number generators an oracle comparison reads (openspec AF.8,
//! design decision 4): MT19937, and the 32-bit LCG shape the census also found.
//! Pure like the rest of the crate: the harness reads a generator's state out of
//! guest memory and hands it here.
//!
//! MT19937 is Matsumoto and Nishimura's: 624 words of state, a twist that
//! regenerates all 624 at once, and a tempering of each word drawn. Two seedings
//! exist and put a seed in different places: the 2002 reference
//! [`Mt19937::init_genrand`], which the published output covers, and the 1998
//! [`Mt19937::sgenrand`] (two `69069x + 1` steps per word), which is the one
//! Giants runs. The twist and the tempering are the same in both, so the
//! published vector pins the generator and `sgenrand` adds only its seeding.
//!
//! A position in the stream is the 624 words plus `mti`, how many of them have
//! been drawn since the last twist (624: the next draw twists first). So
//! `(words, 624)` and `(twist(words), 0)` are the same position, and
//! [`draws_between`] normalises both sides before it compares.

use std::fmt;

/// Words of state.
pub const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_B0DF;
const UPPER: u32 = 0x8000_0000;
const LOWER: u32 = 0x7FFF_FFFF;
const SGENRAND_MUL: u32 = 69_069;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RngError {
    #[error("mti {0} is past the {N} words of state")]
    Mti(u32),
    #[error("{0} words of state where MT19937 has {N}")]
    Words(usize),
}

/// A position in an MT19937 stream: the 624 words and `mti`, the next one to
/// draw (624 when the next draw twists first).
#[derive(Clone, PartialEq, Eq)]
pub struct Mt19937 {
    words: [u32; N],
    mti: usize,
}

impl fmt::Debug for Mt19937 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Mt19937 {{ mti: {}, words: [{:#010x}, {:#010x}, .., {:#010x}] }}",
            self.mti,
            self.words[0],
            self.words[1],
            self.words[N - 1]
        )
    }
}

impl Mt19937 {
    /// The 2002 reference seeding (`init_genrand`): `mt[i] = 1812433253 *
    /// (mt[i-1] ^ mt[i-1] >> 30) + i`. What the published test output and C++'s
    /// `std::mt19937` use; not what Giants runs.
    pub fn init_genrand(seed: u32) -> Self {
        let mut words = [0u32; N];
        words[0] = seed;
        for i in 1..N {
            let prev = words[i - 1];
            words[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        Self { words, mti: N }
    }

    /// The 1998 seeding (`sgenrand`), Giants' (AF.8's census): a `69069x + 1`
    /// LCG from `seed`, each word the high half of one value and the high half
    /// of the next, two steps a word. Leaves `mti` at 624, so the first draw
    /// twists.
    pub fn sgenrand(seed: u32) -> Self {
        let mut words = [0u32; N];
        let mut x = seed;
        for word in &mut words {
            let high = x & 0xFFFF_0000;
            x = sgenrand_step(x);
            *word = high | (x >> 16);
            x = sgenrand_step(x);
        }
        Self { words, mti: N }
    }

    /// A state read out of memory. `mti` above 624 is refused: no generator
    /// leaves one.
    pub fn from_parts(mti: u32, words: &[u32]) -> Result<Self, RngError> {
        let words: [u32; N] = words.try_into().map_err(|_| RngError::Words(words.len()))?;
        if mti as usize > N {
            return Err(RngError::Mti(mti));
        }
        Ok(Self {
            words,
            mti: mti as usize,
        })
    }

    /// Words drawn since the last twist, 0 to 624.
    pub fn mti(&self) -> u32 {
        self.mti as u32
    }

    pub fn words(&self) -> &[u32; N] {
        &self.words
    }

    /// Regenerates the 624 words in place, as the reference does: the last
    /// word mixes in the already new `mt[0]`. Sets `mti` to 0.
    pub fn twist(&mut self) {
        self.words = twisted(&self.words);
        self.mti = 0;
    }

    /// The next number, twisting first when all 624 words are drawn.
    pub fn next_u32(&mut self) -> u32 {
        if self.mti >= N {
            self.twist();
        }
        let y = self.words[self.mti];
        self.mti += 1;
        temper(y)
    }

    /// `draws` numbers on, a twist per 624 rather than a draw at a time.
    pub fn advance(&mut self, mut draws: u64) {
        let left = (N - self.mti) as u64;
        if draws <= left {
            self.mti += draws as usize;
            return;
        }
        draws -= left;
        let twists = draws.div_ceil(N as u64);
        for _ in 0..twists {
            self.twist();
        }
        self.mti = (draws - (twists - 1) * N as u64) as usize;
    }

    /// The same position with `mti` below 624: `(words, 624)` becomes
    /// `(twist(words), 0)`.
    fn normalized(&self) -> Self {
        let mut out = self.clone();
        if out.mti == N {
            out.twist();
        }
        out
    }
}

fn sgenrand_step(x: u32) -> u32 {
    x.wrapping_mul(SGENRAND_MUL).wrapping_add(1)
}

fn twisted(old: &[u32; N]) -> [u32; N] {
    let mut mt = *old;
    for i in 0..N {
        let y = (mt[i] & UPPER) | (mt[(i + 1) % N] & LOWER);
        mt[i] = mt[(i + M) % N] ^ (y >> 1) ^ if y & 1 == 1 { MATRIX_A } else { 0 };
    }
    mt
}

fn temper(mut y: u32) -> u32 {
    y ^= y >> 11;
    y ^= (y << 7) & 0x9D2C_5680;
    y ^= (y << 15) & 0xEFC6_0000;
    y ^ (y >> 18)
}

/// The words one twist before `new`. The old `mt[0]`'s low 31 bits never reach
/// the twisted words, so they come back 0; untwisting again recovers them,
/// since the twist's last step mixed them in.
pub fn untwist(new: &[u32; N]) -> [u32; N] {
    let mut new = *new;
    let mut old = [0u32; N];
    for i in (0..N).rev() {
        // What the twist XORed in at step i: an old word below 227, a new one above.
        let mixed = if i < N - M {
            old[i + M]
        } else {
            new[i + M - N]
        };
        let z = new[i] ^ mixed;
        // y >> 1 has its top bit clear and MATRIX_A has it set, so the top bit
        // says whether y was odd.
        let y = if z & UPPER != 0 {
            ((z ^ MATRIX_A) << 1) | 1
        } else {
            z << 1
        };
        if i == N - 1 {
            // Step 623 took its low bits from the new mt[0].
            new[0] = (new[0] & UPPER) | (y & LOWER);
        } else {
            old[i + 1] = (old[i + 1] & UPPER) | (y & LOWER);
        }
        old[i] = (old[i] & LOWER) | (y & UPPER);
    }
    old
}

/// How many numbers were drawn between `from` and `to`, stepping `from`
/// forward at most `max_twists` twists. `None` when `to` isn't within that, or
/// is behind `from`.
pub fn draws_between(from: &Mt19937, to: &Mt19937, max_twists: u64) -> Option<u64> {
    let mut a = from.normalized();
    let b = to.normalized();
    let (from_mti, to_mti) = (a.mti as u64, b.mti as u64);
    for twists in 0..=max_twists {
        if a.words == b.words {
            return (twists > 0 || to_mti >= from_mti)
                .then(|| twists * N as u64 + to_mti - from_mti);
        }
        a.twist();
    }
    None
}

/// Where a generator's stream began: the `sgenrand` seed and the numbers drawn
/// since.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Origin {
    pub seed: u32,
    pub draws: u64,
}

/// Untwists `state` until the words have `sgenrand`'s shape, at most
/// `max_twists` times, and checks the find forward: `sgenrand(seed)` twisted
/// back up has to equal `state` word for word. `None` when no such seeding is
/// within reach, which is also how a generator seeded some other way reads.
pub fn sgenrand_origin(state: &Mt19937, max_twists: u64) -> Option<Origin> {
    let mut words = state.words;
    for twists in 0..=max_twists {
        if let Some(seed) = sgenrand_seed_of(&words) {
            let mut forward = Mt19937::sgenrand(seed);
            for _ in 0..twists {
                forward.twist();
            }
            // Untwisted words carry no mt[0] low bits, so the find is only a
            // candidate until the whole state matches.
            let exact = forward.words == state.words;
            // Before the first twist nothing has been drawn: mti is still 624.
            if exact && (twists > 0 || state.mti == N) {
                return Some(Origin {
                    seed,
                    draws: (twists * N as u64 + state.mti as u64).saturating_sub(N as u64),
                });
            }
        }
        words = untwist(&words);
    }
    None
}

/// The seed whose `sgenrand` would give `words[1..5]`, if one does. Word 1
/// holds the high halves of the LCG's third and fourth values, so the third's
/// low half is the only unknown: each of its 65,536 candidates is stepped
/// through words 1-4 (seven 16-bit checks), and the seed is two steps back.
/// Word 0 is checked only by its top bit, all an untwisted state knows of it.
pub fn sgenrand_seed_of(words: &[u32; N]) -> Option<u32> {
    let inverse = mul_inverse(SGENRAND_MUL);
    let want = [
        words[1] & 0xFFFF,
        words[2] >> 16,
        words[2] & 0xFFFF,
        words[3] >> 16,
        words[3] & 0xFFFF,
        words[4] >> 16,
        words[4] & 0xFFFF,
    ];
    (0..=0xFFFFu32).find_map(|low| {
        let x2 = (words[1] & 0xFFFF_0000) | low;
        let mut x = x2;
        for &w in &want {
            x = sgenrand_step(x);
            if x >> 16 != w {
                return None;
            }
        }
        let x1 = x2.wrapping_sub(1).wrapping_mul(inverse);
        let seed = x1.wrapping_sub(1).wrapping_mul(inverse);
        (seed & UPPER == words[0] & UPPER).then_some(seed)
    })
}

/// `a`'s inverse mod 2^32 (`a` odd), by Newton's iteration.
fn mul_inverse(a: u32) -> u32 {
    let mut x: u32 = 1;
    for _ in 0..5 {
        x = x.wrapping_mul(2u32.wrapping_sub(a.wrapping_mul(x)));
    }
    x
}

/// A 32-bit linear congruential generator, `x' = a x + c` mod 2^32.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lcg32 {
    pub multiplier: u32,
    pub increment: u32,
}

impl Lcg32 {
    /// Numerical Recipes' constants (1664525, 1013904223): Giants' animation
    /// variety steps a pair of these (AF.8's census). Full period, 2^32.
    pub const NUMERICAL_RECIPES: Lcg32 = Lcg32 {
        multiplier: 1_664_525,
        increment: 1_013_904_223,
    };

    pub fn step(self, x: u32) -> u32 {
        x.wrapping_mul(self.multiplier).wrapping_add(self.increment)
    }

    /// Steps from `from` to `to`, at most `max_steps`; `None` past it.
    pub fn steps_between(self, from: u32, to: u32, max_steps: u64) -> Option<u64> {
        let mut x = from;
        for steps in 0..=max_steps {
            if x == to {
                return Some(steps);
            }
            x = self.step(x);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(mut mt: Mt19937, draws: u64) -> Mt19937 {
        for _ in 0..draws {
            mt.next_u32();
        }
        mt
    }

    #[test]
    fn the_reference_seeding_reproduces_the_published_output() {
        // Seed 5489 is `std::mt19937`'s default; C++ ([rand.predef]) fixes its
        // 10,000th number at 4123659995, and its first is 3499211612.
        let mut mt = Mt19937::init_genrand(5489);
        assert_eq!(mt.next_u32(), 3_499_211_612);
        for _ in 1..9_999 {
            mt.next_u32();
        }
        assert_eq!(mt.next_u32(), 4_123_659_995);
    }

    #[test]
    fn sgenrand_is_the_1998_routine() {
        // The routine's definition, worked by hand for seed 5150 (the seed the
        // game's constructor uses): word 0 is the seed's high half and the high
        // half of 5150 * 69069 + 1 = 355,705,351.
        let mt = Mt19937::sgenrand(5150);
        assert_eq!(mt.mti(), 624);
        assert_eq!(mt.words()[0], 355_705_351 >> 16);
        // Self-generated, not published: the research model's output, which
        // three savestates' generators reproduced word for word (AF.8's census).
        let mut mt = mt;
        let first: Vec<u32> = (0..5).map(|_| mt.next_u32()).collect();
        assert_eq!(
            first,
            [
                3_451_403_731,
                3_506_832_599,
                4_045_271_892,
                3_440_458_153,
                881_774_694
            ]
        );
        assert_eq!(drawn(mt, 9_994).next_u32(), 2_797_486_301);
        let mut other = Mt19937::sgenrand(4357);
        assert_eq!(other.next_u32(), 2_867_219_139);
        // The two seedings of one seed are different states.
        assert_ne!(Mt19937::sgenrand(5489), Mt19937::init_genrand(5489));
    }

    #[test]
    fn advance_is_drawing_one_at_a_time() {
        let start = drawn(Mt19937::sgenrand(10_554), 300);
        for draws in [0, 1, 323, 324, 325, 947, 948, 949, 5_000] {
            let mut skipped = start.clone();
            skipped.advance(draws);
            let mut stepped = drawn(start.clone(), draws);
            assert_eq!(skipped, stepped, "{draws} draws");
            assert_eq!(skipped.next_u32(), stepped.next_u32(), "{draws} draws");
        }
        let mut fresh = Mt19937::sgenrand(1);
        fresh.advance(624);
        assert_eq!(fresh, drawn(Mt19937::sgenrand(1), 624));
        assert_eq!(fresh.mti(), 624);
    }

    #[test]
    fn the_distance_steps_across_twists() {
        let base = Mt19937::sgenrand(5150);
        let a = drawn(base.clone(), 600);
        // 600 -> 700 crosses the twist at 624.
        let b = drawn(base.clone(), 700);
        assert_eq!(draws_between(&a, &b, 4), Some(100));
        assert_eq!(draws_between(&a, &a, 0), Some(0));
        let mut far = a.clone();
        far.advance(3 * 624 + 17);
        assert_eq!(draws_between(&a, &far, 4), Some(3 * 624 + 17));
        // Three twists apart is out of reach with two allowed.
        assert_eq!(draws_between(&a, &far, 2), None);
        // Behind, in the same block and in an earlier one.
        assert_eq!(draws_between(&b, &a, 10), None);
        assert_eq!(draws_between(&far, &a, 10), None);
        // (words, 624) and (twist(words), 0) are one position, either way round.
        let full = drawn(base.clone(), 624);
        let mut turned = full.clone();
        turned.twist();
        assert_eq!((full.mti(), turned.mti()), (624, 0));
        assert_eq!(draws_between(&full, &turned, 0), Some(0));
        assert_eq!(draws_between(&turned, &full, 0), Some(0));
        // A fresh seeding to its first draw: one twist, one number.
        assert_eq!(draws_between(&base, &drawn(base.clone(), 1), 1), Some(1));
        // An unrelated stream is never reached.
        assert_eq!(draws_between(&a, &Mt19937::sgenrand(5151), 50), None);
    }

    #[test]
    fn untwisting_inverts_a_twist_but_for_the_old_first_words_low_bits() {
        let mut mt = drawn(Mt19937::sgenrand(77), 1);
        let older = *mt.words();
        mt.twist();
        let back = untwist(mt.words());
        assert_eq!(back[1..], older[1..]);
        assert_eq!(back[0], older[0] & UPPER);
        // Two levels back the lost bits come back from the level above.
        let oldest = *Mt19937::sgenrand(77).words();
        let mut twice = Mt19937::sgenrand(77);
        twice.twist();
        let middle = *twice.words();
        twice.twist();
        let mut level = untwist(twice.words());
        assert_eq!(level[1..], middle[1..]);
        level = untwist(&level);
        assert_eq!(level[1..], oldest[1..]);
    }

    #[test]
    fn the_origin_is_the_seed_and_the_draws_since() {
        // The AF.5 state's numbers: seeded 10554, 104,201 drawn.
        let mut state = Mt19937::sgenrand(10_554);
        state.advance(104_201);
        assert_eq!(
            sgenrand_origin(&state, 200),
            Some(Origin {
                seed: 10_554,
                draws: 104_201
            })
        );
        // Not within reach: 167 twists back, 100 allowed.
        assert_eq!(sgenrand_origin(&state, 100), None);
        for draws in [0, 1, 623, 624, 625] {
            let mut s = Mt19937::sgenrand(0xDEAD_BEEF);
            s.advance(draws);
            assert_eq!(
                sgenrand_origin(&s, 3),
                Some(Origin {
                    seed: 0xDEAD_BEEF,
                    draws
                }),
                "{draws} draws"
            );
        }
        // A stream the reference seeding started isn't sgenrand's.
        let mut reference = Mt19937::init_genrand(5489);
        reference.advance(1_000);
        assert_eq!(sgenrand_origin(&reference, 5), None);
        assert_eq!(
            sgenrand_seed_of(Mt19937::sgenrand(123_456).words()),
            Some(123_456)
        );
    }

    #[test]
    fn a_state_out_of_memory_is_checked() {
        let words = *Mt19937::sgenrand(9).words();
        assert_eq!(
            Mt19937::from_parts(624, &words).unwrap(),
            Mt19937::sgenrand(9)
        );
        assert_eq!(Mt19937::from_parts(625, &words), Err(RngError::Mti(625)));
        assert_eq!(
            Mt19937::from_parts(0, &words[..623]),
            Err(RngError::Words(623))
        );
    }

    #[test]
    fn an_lcg_counts_its_steps() {
        let lcg = Lcg32::NUMERICAL_RECIPES;
        let mut x = 0x12345;
        for _ in 0..13 {
            x = lcg.step(x);
        }
        assert_eq!(lcg.steps_between(0x12345, x, 100), Some(13));
        assert_eq!(lcg.steps_between(0x12345, 0x12345, 0), Some(0));
        assert_eq!(lcg.steps_between(0x12345, x, 12), None);
        assert_eq!(lcg.step(0), 1_013_904_223);
    }
}
