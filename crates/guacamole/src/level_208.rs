//! Level 208's way out of its start plaza (openspec AS.344), as
//! [`crate::level_205`] lists 205's: what of the level's own scripts it
//! needs, by placement name.
//!
//! The plaza's south exit looks open but two scripted solids close it, and
//! the original gets a hero through each by its own script:
//! - **The reveal's trigger** (`Ice_Chompy_IGC_Trigger`, a box 40 by 20 by
//!   8 ft across the exit, solid to actors and the level). Woken, it makes
//!   the first room's Ice Chompies (the level's `Setup_001_Enemies`)
//!   invulnerable, then waits for a player to come into it; one does as he
//!   walks into the box, and it plays the reveal (`IGC_Ice_Chompy_Reveal`:
//!   the camera on one Ice Chompy past the exit, its freeze attack and the
//!   enemy intro's card the first time), waits for the cutscene's end,
//!   makes them vulnerable again and takes itself off, its solid with it.
//!   Its first step reads each of those Chompies' creature info and goes
//!   no further on one that has none, so it waits on all five standing.
//! - **The brick wall** (`Wall_Spawner_Template(5)`, `Brick_Wall_Spawner`,
//!   past the trigger). At its start it lays its bricks (clones of `brick`
//!   and `brick2`) along its box, which it keeps invisible, and makes
//!   itself a 40-hp destructible the hero's attacks hurt; a heavy hit or
//!   its health run out blows the bricks apart (their own script flies
//!   them off and they go), turns its box off for actors and takes itself
//!   off a few seconds on. Nothing else spends it: a hero who doesn't
//!   attack it stays behind it.
//!
//! Past both lies Setup_002's place, whose fight is openspec AS.345's, as is
//! the five Chompies' (the reveal needs their scripts running, not their
//! bodies).

/// The archive the story reaches after 205 (openspec AS.340).
pub const LEVEL: &str = "level_208_ice";

/// The exit's two solids and the wall's brick templates (a clone runs under
/// its template's name), by name, listed and never walked: without the
/// bricks' script a beaten wall's bricks stand where they were laid until
/// the wall takes itself off.
pub const EXIT: [&str; 4] = [
    "Ice_Chompy_IGC_Trigger",
    "Wall_Spawner_Template(5)",
    "brick",
    "brick2",
];

/// The five Ice Chompies of the first room's set, which the reveal's
/// trigger waits on, by name, listed and never walked (taken from AS.345's
/// fights; walking them, and the rest of their fight, is AS.345's), drawn
/// as the level's people ([`people`]): on their scripts' clips, hopping
/// where their scripts move them and biting a hero beside them. Walked,
/// the reveal's turn of its Chompy toward its mark never finishes and the
/// reveal holds the hero for good (openspec AS.385); unwalked, a creature's
/// `turn to` turns it at once.
pub const REVEAL_FOES: [&str; 5] = [
    "Enemy_Chompy_Ice(2)",
    "Enemy_Chompy_Ice(3)",
    "Enemy_Chompy_Ice(20)",
    "Enemy_Chompy_Ice(21)",
    "Enemy_Chompy_Ice(40)",
];

/// [`REVEAL_FOES`] on 208, drawn as the level's people are (unwalked, on
/// their scripts' clips: the TV draws an unwalked creature no other way),
/// nothing on any other level.
pub fn people(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &REVEAL_FOES,
        _ => &[],
    }
}

/// [`EXIT`] and [`REVEAL_FOES`] on 208 alone: another level's `brick`
/// templates are its own.
pub fn exit(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    EXIT.iter().chain(&REVEAL_FOES).copied().filter(move |_| on)
}

#[cfg(test)]
mod tests {
    use guac_script::Mode;

    use super::*;
    use crate::scripts::{Files, encounters, level_config};

    /// openspec AS.344: the exit's solids, the wall's bricks and the
    /// reveal's five Chompies are on 208's listing and on no other level's
    /// (205 holds walls and Chompies of its own, 202 none), and nothing of
    /// 208's is walked as a fight.
    #[test]
    fn level_208s_exit_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(exit(LEVEL).count(), 9);
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            for name in EXIT.iter().chain(&REVEAL_FOES) {
                assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
            assert_eq!(exit(other).count(), 0);
        }
        assert_eq!(encounters(LEVEL).count(), 0);
        // Drawn as its people, there alone.
        for name in REVEAL_FOES {
            assert!(crate::scripts::is_level_person(LEVEL, name), "{name}");
            assert!(!crate::scripts::is_level_person(
                "level_205_piratecards",
                name
            ));
        }
        // The Ice Chompies' own script stays off for the level's others
        // (AS.345's).
        let ice = "C:/tfb/Content/Levels/_Enemies/Chompy_Ice/scripts/S2_Enemy_IceChompy.ai";
        assert!(!on.whitelist.lists("Enemy_Chompy_Ice(1)", ice));
    }
}
