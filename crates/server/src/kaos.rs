//! Kaos swap selection (PLAN 8.2b).
//!
//! Given the current portal state + the profile whose turn it is to be
//! swapped + the library, picks a random eligible (slot, replacement)
//! pair or returns `None` if nothing is eligible. Pure logic — no I/O,
//! no broadcasts. The timer task in `state.rs` drives the selection and
//! then executes the clear+load pair; this module deliberately has
//! neither an `AppState` dependency nor any mention of the driver, so
//! the selection rules stay testable without spinning up the full
//! server.

use std::time::Duration;

use rand_core::RngCore;
use skylander_core::{
    Event, Figure, FigureId, GameOfOrigin, SLOT_COUNT, SlotIndex, SlotState, compat::is_compatible,
};

/// One chosen swap: pull `old_figure_id` out of `slot` and push
/// `new_figure_id` in its place. Emitted by [`select_swap`] and fed
/// into the execute path in `state.rs` (8.2b.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KaosSwap {
    pub slot: SlotIndex,
    pub old_figure_id: FigureId,
    pub new_figure_id: FigureId,
}

/// Pick a (slot, replacement) pair for `profile_id`. The slot must be
/// a currently-`Loaded` entry placed by that profile; the replacement
/// must be a figure in `figures` that is:
///
/// - Compatible with `current_game` per `compat::is_compatible`
/// - Not currently placed on any portal slot (avoids instant
///   "swap Eruptor for Eruptor" no-ops + respects "each physical
///   figure is on the portal at most once" physics)
/// - Different from the figure we're pulling out (same-figure swap
///   would read as a no-op to the user even if the compat logic
///   technically allowed it — e.g. two identical Eruptor variants
///   across the library)
///
/// Returns `None` when either the eligible-slot set or the eligible-
/// replacement pool is empty. Callers should back off and re-arm the
/// timer; this is the expected outcome for "portal is empty", "the
/// profile has nothing on the portal", or "current game has no
/// compatible library matches" (e.g. a fresh install with only
/// Imaginators-era figures and the user booted SpyrosAdventure).
pub fn select_swap<R: RngCore>(
    current_game: GameOfOrigin,
    portal: &[SlotState; SLOT_COUNT],
    figures: &[Figure],
    profile_id: &str,
    rng: &mut R,
) -> Option<KaosSwap> {
    // --- 1. Eligible slots (Loaded + owned by profile) -----------------
    let eligible_slots: Vec<(usize, FigureId)> = portal
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            SlotState::Loaded {
                figure_id: Some(fid),
                placed_by: Some(pid),
                ..
            } if pid == profile_id => Some((i, fid.clone())),
            _ => None,
        })
        .collect();
    if eligible_slots.is_empty() {
        return None;
    }

    // --- 2. Figures currently ANY-slot (so we don't dupe) --------------
    let on_portal: Vec<&FigureId> = portal
        .iter()
        .filter_map(|s| match s {
            SlotState::Loaded {
                figure_id: Some(fid),
                ..
            }
            | SlotState::Loading {
                figure_id: Some(fid),
                ..
            } => Some(fid),
            _ => None,
        })
        .collect();

    // --- 3. Pick the slot first, then its replacement ------------------
    let slot_idx = (rng.next_u32() as usize) % eligible_slots.len();
    let (slot_i, old_fid) = eligible_slots[slot_idx].clone();

    let pool: Vec<&Figure> = figures
        .iter()
        .filter(|f| {
            if f.id == old_fid {
                return false;
            }
            if on_portal.contains(&&f.id) {
                return false;
            }
            is_compatible(f.game, f.category, current_game)
        })
        .collect();
    if pool.is_empty() {
        return None;
    }

    let pick = (rng.next_u32() as usize) % pool.len();
    let new_figure = pool[pick];
    Some(KaosSwap {
        slot: SlotIndex::new(slot_i as u8).ok()?,
        old_figure_id: old_fid,
        new_figure_id: new_figure.id.clone(),
    })
}

/// Kaos taunt catchphrases for the mid-game swap (PLAN 8.2b.4). Text-
/// only — voice lines can't ship without stepping on Activision's IP,
/// so these are Kaos-voice paraphrases rather than verbatim quotes.
/// Shared with the session-takeover surface so the overlay reads
/// consistently across the two Kaos moments.
pub const KAOS_SWAP_TAUNTS: &[&str] = &[
    "Ha HAAA! A new champion serves KAOS now!",
    "Your precious Skylander? Dismissed!",
    "Behold — I have IMPROVED your little team!",
    "One of yours, one of mine! Such is the KAOS bargain!",
    "You call that a portal master? THIS is a portal master!",
    "The mighty KAOS giveth, and the mighty KAOS taketh away!",
    "Your pathetic collection bends to my whim!",
];

/// Pick a random Kaos swap taunt.
pub fn random_swap_taunt<R: RngCore>(rng: &mut R) -> &'static str {
    let idx = (rng.next_u32() as usize) % KAOS_SWAP_TAUNTS.len();
    KAOS_SWAP_TAUNTS[idx]
}

// ---- Timer cadence (PLAN 8.2b.2) ----------------------------------------

/// First Kaos fire lands ≥ this long after a session unlocks into a
/// Kaos-enabled profile. Gives the user a grace window to settle into
/// gameplay before a disruption happens.
pub const WARMUP: Duration = Duration::from_secs(20 * 60);

/// Minimum spacing between consecutive fires on the same session.
/// Prevents a second fire landing within a minute of the previous —
/// the feature is disruption, not spam.
pub const MIN_GAP: Duration = Duration::from_secs(60);

/// Maximum spacing between consecutive fires. "Uniformly-random fire
/// within each hour window" — the next fire is somewhere in
/// `[MIN_GAP, MAX_GAP]` after the previous, inclusive.
pub const MAX_GAP: Duration = Duration::from_secs(60 * 60);

/// Pick a uniformly-random gap within `[MIN_GAP, MAX_GAP]`. Returned as a
/// `Duration` the caller can `now + gap` to get the next fire instant.
pub fn random_gap<R: RngCore>(rng: &mut R) -> Duration {
    let min = MIN_GAP.as_secs();
    let max = MAX_GAP.as_secs();
    let span = max - min + 1;
    let secs = min + (rng.next_u64() % span);
    Duration::from_secs(secs)
}

/// Build the `KaosTaunt` wire event from a selected swap + a taunt +
/// the targeted profile. Separated from `select_swap` so tests can
/// pin the payload shape without also exercising selection randomness.
pub fn build_taunt_event(swap: &KaosSwap, taunt: &str, profile_id: &str) -> Event {
    Event::KaosTaunt {
        profile_id: profile_id.to_string(),
        slot: swap.slot,
        old_figure_id: swap.old_figure_id.clone(),
        new_figure_id: swap.new_figure_id.clone(),
        taunt: taunt.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skylander_core::{Category, Element, Figure, FigureId, GameOfOrigin, SlotIndex, SlotState};
    use std::path::PathBuf;

    fn fig(id: &str, game: GameOfOrigin, category: Category) -> Figure {
        Figure {
            id: FigureId::new(id),
            canonical_name: id.to_string(),
            variant_group: id.to_string(),
            variant_tag: "base".to_string(),
            game,
            element: Some(Element::Fire),
            category,
            sky_path: PathBuf::from(format!("/fake/{id}.sky")),
            element_icon_path: None,
            tag_identity: None,
        }
    }

    fn empty_portal() -> [SlotState; SLOT_COUNT] {
        std::array::from_fn(|_| SlotState::Empty)
    }

    fn loaded(fid: &str, pid: &str) -> SlotState {
        SlotState::Loaded {
            figure_id: Some(FigureId::new(fid)),
            display_name: fid.to_string(),
            placed_by: Some(pid.to_string()),
        }
    }

    /// Minimal seedable PRNG for tests — 64-bit xorshift. Avoids pulling
    /// in the full `rand` crate just for deterministic unit tests (and
    /// sidesteps the `rand_core` version skew that bites every Rust
    /// codebase eventually). Good enough for "pick a random slot/
    /// replacement with a fixed seed so assertions are stable".
    struct XorShift64(u64);
    impl XorShift64 {
        fn new(seed: u64) -> Self {
            Self(if seed == 0 { 0xC0FFEE } else { seed })
        }
    }
    impl RngCore for XorShift64 {
        fn next_u32(&mut self) -> u32 {
            self.next_u64() as u32
        }
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn fill_bytes(&mut self, dest: &mut [u8]) {
            for chunk in dest.chunks_mut(8) {
                let v = self.next_u64().to_le_bytes();
                chunk.copy_from_slice(&v[..chunk.len()]);
            }
        }
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    fn seeded_rng() -> XorShift64 {
        XorShift64::new(0xC0FFEE)
    }

    #[test]
    fn none_when_profile_has_no_placed_figures() {
        let mut portal = empty_portal();
        // A figure owned by someone else — shouldn't match alice.
        portal[0] = loaded("eruptor", "bob");
        let figures = vec![
            fig("eruptor", GameOfOrigin::SpyrosAdventure, Category::Figure),
            fig("spyro", GameOfOrigin::SpyrosAdventure, Category::Figure),
        ];
        let mut rng = seeded_rng();
        let swap = select_swap(
            GameOfOrigin::Imaginators,
            &portal,
            &figures,
            "alice",
            &mut rng,
        );
        assert!(swap.is_none());
    }

    #[test]
    fn none_when_no_compatible_replacement() {
        let mut portal = empty_portal();
        portal[0] = loaded("eruptor", "alice");
        // Library has nothing compatible with SpyrosAdventure except
        // Eruptor itself, which is disqualified as a same-figure swap.
        let figures = vec![
            fig("eruptor", GameOfOrigin::SpyrosAdventure, Category::Figure),
            fig("sheep-creep", GameOfOrigin::TrapTeam, Category::Figure),
            fig("fiesta", GameOfOrigin::Superchargers, Category::Vehicle),
        ];
        let mut rng = seeded_rng();
        let swap = select_swap(
            GameOfOrigin::SpyrosAdventure,
            &portal,
            &figures,
            "alice",
            &mut rng,
        );
        assert!(
            swap.is_none(),
            "trap-team + a Superchargers-vehicle exclude the only portal figure; no pool",
        );
    }

    #[test]
    fn vehicle_pool_for_superchargers() {
        let mut portal = empty_portal();
        portal[0] = loaded("spyro", "alice");
        let figures = vec![
            fig("spyro", GameOfOrigin::SpyrosAdventure, Category::Figure),
            fig("fiesta", GameOfOrigin::Superchargers, Category::Vehicle),
            fig("shark-tank", GameOfOrigin::Superchargers, Category::Vehicle),
        ];
        let mut rng = seeded_rng();
        let swap = select_swap(
            GameOfOrigin::Superchargers,
            &portal,
            &figures,
            "alice",
            &mut rng,
        )
        .expect("sc-compatible pool non-empty");
        assert_eq!(swap.slot, SlotIndex::new(0).unwrap());
        assert_eq!(swap.old_figure_id, FigureId::new("spyro"));
        assert!(
            swap.new_figure_id == FigureId::new("fiesta")
                || swap.new_figure_id == FigureId::new("shark-tank"),
            "replacement must come from the compatible pool",
        );
    }

    #[test]
    fn vehicles_excluded_from_non_supercharger_pool() {
        let mut portal = empty_portal();
        portal[0] = loaded("spyro", "alice");
        let figures = vec![
            fig("spyro", GameOfOrigin::SpyrosAdventure, Category::Figure),
            fig("fiesta", GameOfOrigin::Superchargers, Category::Vehicle),
            fig("eruptor", GameOfOrigin::SpyrosAdventure, Category::Figure),
        ];
        let mut rng = seeded_rng();
        let swap = select_swap(
            GameOfOrigin::Imaginators,
            &portal,
            &figures,
            "alice",
            &mut rng,
        )
        .expect("eruptor is compatible with imaginators");
        assert_eq!(swap.new_figure_id, FigureId::new("eruptor"));
    }

    #[test]
    fn already_on_portal_excluded_from_pool() {
        let mut portal = empty_portal();
        portal[0] = loaded("spyro", "alice");
        portal[1] = loaded("eruptor", "bob"); // bob's slot — still occupies portal
        let figures = vec![
            fig("spyro", GameOfOrigin::SpyrosAdventure, Category::Figure),
            fig("eruptor", GameOfOrigin::SpyrosAdventure, Category::Figure),
            fig(
                "wham-shell",
                GameOfOrigin::SpyrosAdventure,
                Category::Figure,
            ),
        ];
        let mut rng = seeded_rng();
        let swap = select_swap(GameOfOrigin::Giants, &portal, &figures, "alice", &mut rng)
            .expect("wham-shell is available");
        // Only wham-shell is eligible: eruptor is on the portal (even
        // though bob placed it), spyro is the swap target.
        assert_eq!(swap.new_figure_id, FigureId::new("wham-shell"));
    }

    #[test]
    fn taunt_event_payload_shape() {
        let swap = KaosSwap {
            slot: SlotIndex::new(3).unwrap(),
            old_figure_id: FigureId::new("spyro"),
            new_figure_id: FigureId::new("wham-shell"),
        };
        let evt = build_taunt_event(&swap, "HA HAAA!", "alice");
        match evt {
            Event::KaosTaunt {
                profile_id,
                slot,
                old_figure_id,
                new_figure_id,
                taunt,
            } => {
                assert_eq!(profile_id, "alice");
                assert_eq!(slot, SlotIndex::new(3).unwrap());
                assert_eq!(old_figure_id, FigureId::new("spyro"));
                assert_eq!(new_figure_id, FigureId::new("wham-shell"));
                assert_eq!(taunt, "HA HAAA!");
            }
            other => panic!("expected KaosTaunt, got {other:?}"),
        }
    }

    #[test]
    fn random_gap_stays_within_bounds() {
        let mut rng = seeded_rng();
        for _ in 0..1000 {
            let g = random_gap(&mut rng);
            assert!(g >= MIN_GAP, "gap {g:?} below MIN_GAP");
            assert!(g <= MAX_GAP, "gap {g:?} above MAX_GAP");
        }
    }

    #[test]
    fn random_gap_rotates_across_large_sample() {
        // Pin the distribution is non-trivial — picked values over
        // a thousand draws should span more than a single fixed
        // second. Guards against a stuck modulo that always returns
        // the same value.
        let mut rng = seeded_rng();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            seen.insert(random_gap(&mut rng).as_secs());
        }
        assert!(
            seen.len() > 50,
            "gap distribution too tight — seen only {} distinct values",
            seen.len()
        );
    }

    #[test]
    fn taunt_rotation_has_multiple_entries() {
        // Pin the coverage — add more taunts freely, but don't ship
        // a one-liner rotation (would feel scripted fast).
        assert!(KAOS_SWAP_TAUNTS.len() >= 5);
        let mut rng = seeded_rng();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            seen.insert(random_swap_taunt(&mut rng));
        }
        assert!(
            seen.len() >= 2,
            "100 picks should have rotated through >=2 taunts",
        );
    }
}
