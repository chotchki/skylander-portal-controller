//! Level 203's start (openspec AS.704 to AS.706), as [`crate::level_208`]
//! lists 208's: what of the level's own scripts its way needs, by placement
//! name. Its intro (on `scripts::INTROS`, AS.703) lands him by pod on the
//! start platform; from there:
//! - **The Molekin** (`molekin_at_start`) puts on its hat (a `miner_hat`
//!   clone on its node), idles and talks to a player in reach on the talk
//!   button; unkillable, never in the way.
//! - **The crushers** (`203_Crusher_begining_a`, `_b`) over the way down:
//!   their control (`_TRIGGER_crushers_begining`, woken at the start)
//!   flags each to start every 7 s; each drops and lifts on its own timings,
//!   lays its damage zone (a `DamageZone` clone) while a player is within
//!   70 ft, and a player in it is hurt, knocked back and put back at the
//!   level's reset point. The control's start also takes off the later
//!   stretches' crushers, blocks and props by set (`crushers_ALPHA` and the
//!   rest), as the original does at the level's start; their own triggers,
//!   off the listing, would bring them back.
//! - **The conveyors** (`new_conveyor` to `new_conveyor(3)`) give the
//!   mover update a directional force over their box while a player is on
//!   one, carrying him down toward the yard.
//! - **The reset points**: `set_reset_point_to_1` sets the level's current
//!   reset point at the start, `reset_point_ON_1` and `_2` move it as he
//!   passes, the `OFF` pair clear it.
//! - **The introduction teleporter** (`TeleporterStart_introduction` to
//!   `TeleporterEnd_introduction`, 201's `Teleporter.ai` family with its
//!   helper): past Setup_001's gate the next two conveyors lead onto it,
//!   and it carries him to the lower walkway, the only way on.
//!
//! Setup_001's fight before the gate is [`WAY`].

/// The archive the story reaches after 202.
pub const LEVEL: &str = "level_203_the_machine";

/// The start area's placements and the introduction teleporter, by name,
/// listed and never walked (openspec AS.704, AS.706): only what runs a
/// script, since a listing picks whose scripts run (the hat, chains and
/// damage zones they clone are templates with none).
pub const START: [&str; 16] = [
    "molekin_at_start",
    "_TRIGGER_crushers_begining",
    "203_Crusher_begining_a",
    "203_Crusher_begining_b",
    "new_conveyor",
    "new_conveyor(1)",
    "new_conveyor(2)",
    "new_conveyor(3)",
    "set_reset_point_to_1",
    "reset_point_ON_1",
    "reset_point_ON_2",
    "reset_point_OFF_1",
    "reset_point_OFF_2",
    "TeleporterStart_introduction",
    "TeleporterEnd_introduction",
    "TeleporterHelper",
];

/// [`START`] on 203 alone.
pub fn start(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    START.iter().copied().filter(move |_| on)
}

/// The first fight, before the start area's gate (openspec AS.705), each
/// placement that runs a script (its trigger volume and the Troll's path are
/// data its controller reads). The riveter Troll (`S2_Enemy_TrollBFG.ai`:
/// his gun a `Troll_BF_Gun` clone, his shots `Troll_BFG_Bullet` clones on
/// the projectile system, AS.766) wakes by his own 250 ft range and stands
/// by the gate from the hand-over (his controller's taunts lost, AS.785);
/// once a player is in Setup_001's trigger (at the second conveyor's foot)
/// the controller moves him along his path and the three Fire Chompies in
/// the yard wake to the hero (each attaches to him and explodes); with all
/// three gone it launches five more one at a time by the Chompy pipe
/// (`GasJet_Pipe(13)`, off the listing: its own script waits all fight for
/// a trigger that never comes, AS.787), then takes itself off. The gate
/// (`enemy_gate_start_area`, on the generic listing) lowers its spikes once
/// the yard's foes are gone.
pub const WAY: crate::scripts::Encounter = crate::scripts::Encounter {
    level: LEVEL,
    placements: &[
        "Setup_001_Controller",
        "Enemy_Troll_BFG(1)",
        "Enemy_Chompy_Fire(1)",
        "Enemy_Chompy_Fire(2)",
        "Enemy_Chompy_Fire(3)",
        "Enemy_Chompy_Fire(4)",
        "Enemy_Chompy_Fire(5)",
        "Enemy_Chompy_Fire(66)",
        "Enemy_Chompy_Fire(67)",
        "Enemy_Chompy_Fire(68)",
    ],
    clones: &[],
    steered: &[],
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::{Files, encounters, level_config};
    use guac_script::Mode;

    /// openspec AS.704, AS.706: the start area's placements and the
    /// introduction teleporter are on 203's listing and on no other level's.
    #[test]
    fn level_203s_start_area_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(start(LEVEL).collect::<Vec<_>>(), START);
        for other in ["level_201", "level_205_piratecards", "level_208_ice"] {
            let off = level_config(Mode::Play, &files, 5, other);
            assert_eq!(start(other).count(), 0);
            for name in START {
                assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
                // 201 places its own teleporter helper by the same name.
                if name == "TeleporterHelper" && other == "level_201" {
                    continue;
                }
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
        }
    }

    /// openspec AS.705: Setup_001's fight is 203's encounter, walked there
    /// alone.
    #[test]
    fn level_203s_first_fight_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(encounters(LEVEL).collect::<Vec<_>>(), vec![&WAY]);
        for name in WAY.placements.iter().chain(WAY.clones) {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
        }
        for other in ["level_202", "level_208_ice"] {
            assert!(encounters(other).all(|e| *e != WAY), "{other}");
            let off = level_config(Mode::Play, &files, 5, other);
            for name in ["Enemy_Troll_BFG(1)", "GasJet_Pipe(13)"] {
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
        }
    }
}
