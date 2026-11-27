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
//! Setup_001's fight before the gate is [`WAY`]; the lower walkway past the
//! teleporter to its gate is [`WALKWAY`] and Setup_002's fight there
//! [`WAY_2`] (openspec AS.765).

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

/// [`START`] on 203 alone, with [`FIRST_FIGHT_PARTS`], [`WALKWAY`] and
/// [`SECOND_FIGHT_PARTS`].
pub fn start(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    START
        .iter()
        .chain(&FIRST_FIGHT_PARTS)
        .chain(&WALKWAY)
        .chain(&SECOND_FIGHT_PARTS)
        .copied()
        .filter(move |_| on)
}

/// What Setup_001's fight runs that isn't a creature (openspec AS.766,
/// AS.787): the Chompy pipe by the first gate, whose trigger and launch
/// both run inside its `wait for trigger` as the controller hands it each
/// Chompy (so the behaviour's name never changes) and which makes its two
/// flame clones at its node at each launch; and the riveter Troll's
/// bullet, a clone that runs under its template's name, whose STARTUP
/// spins it and hangs its glow clone on it (off the listing it flew,
/// hit and was taken off by the scripts' projectile manager all the same,
/// undrawn of either).
pub const FIRST_FIGHT_PARTS: [&str; 2] = ["GasJet_Pipe(13)", "Troll_BFG_Bullet"];

/// The lower walkway from the introduction teleporter's end to its gate
/// (openspec AS.765), what runs a script, in the order he meets it: the
/// teleporter's start pad brings the lower crushers back
/// (`trigger_lower_crushers`, a reset of the set the start area's control
/// took off); by the end pad the start area's sets are taken off
/// (`_KILL_start_stuff`); the Molekin by the first crusher talks on the
/// talk button; the reset point before the gate (`reset_point_ON_3`) and
/// past it (`reset_point_OFF_3`); Setup_002's Chompy pipe ([`WAY_2`]'s
/// fight); the two crushers over the conveyor past the gate and that
/// conveyor. The gate (`enemy_gate_lower_conveyor_01`) is on the generic
/// listing.
pub const WALKWAY: [&str; 9] = [
    "trigger_lower_crushers",
    "_KILL_start_stuff",
    "molekin_first_crusher",
    "reset_point_ON_3",
    "GasJet_Pipe(14)",
    "203_Crusher_lower_a",
    "203_Crusher_lower_b",
    "new_conveyor(4)",
    "reset_point_OFF_3",
];

/// What [`WAY_2`]'s lobber makes that runs a script (openspec AS.1001): its
/// sticky bomb, a clone that runs under its template's name. A bomb that
/// meets a hero spawns one stuck to him (the projectile manager's
/// `PandoranLobber - Proj - Stick to something`), which blows up on its
/// timer, hurting him and whoever stands within 10 ft by the lobber's own
/// damage, unless he shakes it off by the gesture its manager prompts, when
/// it's thrown back at a foe in front of him. Off the listing it stayed
/// inert: every bomb that met him stuck, and none ever hurt him.
pub const SECOND_FIGHT_PARTS: [&str; 1] = ["PandoranLobber_BombSticky"];

/// Setup_002's fight at the lower walkway's gate (openspec AS.765): its
/// controller wakes by its own range as he lands, sends three Fire
/// Chompies out of `GasJet_Pipe(14)`, wakes its lobber on the ledge past
/// the gate's side ramp and sends six more; with all ten gone it pans to
/// the gate and takes its opener off, which lowers the gate's spikes. The
/// lobber sees him only where its sight ray clears its ledge's lip
/// (`lifted-reference` on 203), and bombs him from there.
pub const WAY_2: crate::scripts::Encounter = crate::scripts::Encounter {
    level: LEVEL,
    placements: &[
        "Setup_002_Controller",
        "Enemy_PandoranLobber(1)",
        "Enemy_Chompy_Fire(70)",
        "Enemy_Chompy_Fire(71)",
        "Enemy_Chompy_Fire(72)",
        "Enemy_Chompy_Fire(18)",
        "Enemy_Chompy_Fire(19)",
        "Enemy_Chompy_Fire(20)",
        "Enemy_Chompy_Fire(21)",
        "Enemy_Chompy_Fire(22)",
        "Enemy_Chompy_Fire(23)",
    ],
    clones: &[],
    steered: &[],
};

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
/// (`GasJet_Pipe(13)`, listed in [`FIRST_FIGHT_PARTS`]), then takes itself
/// off. The gate
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
        let all: Vec<&str> = START
            .iter()
            .chain(&FIRST_FIGHT_PARTS)
            .chain(&WALKWAY)
            .chain(&SECOND_FIGHT_PARTS)
            .copied()
            .collect();
        assert_eq!(start(LEVEL).collect::<Vec<_>>(), all);
        for other in ["level_201", "level_205_piratecards", "level_208_ice"] {
            let off = level_config(Mode::Play, &files, 5, other);
            assert_eq!(start(other).count(), 0);
            for &name in &all {
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

    /// openspec AS.1001: the lobber's sticky bomb runs its own script on
    /// 203, by its template's name, and on no other level of these.
    #[test]
    fn level_203s_sticky_bomb_runs_its_script_there_alone() {
        let files = Files::default();
        const STICKY: &str = "C:/l/Enemy_PandoranLobber_Sticky.ai";
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert!(on.whitelist.lists("PandoranLobber_BombSticky", STICKY));
        for other in ["level_201", "level_205_piratecards", "level_208_ice"] {
            let off = level_config(Mode::Play, &files, 5, other);
            assert!(
                !off.whitelist.lists("PandoranLobber_BombSticky", STICKY),
                "{other}"
            );
        }
    }

    /// openspec AS.705: Setup_001's fight is 203's encounter, walked there
    /// alone.
    #[test]
    fn level_203s_first_fight_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(encounters(LEVEL).collect::<Vec<_>>(), vec![&WAY, &WAY_2]);
        for name in [WAY, WAY_2]
            .iter()
            .flat_map(|w| w.placements.iter().chain(w.clones))
        {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
        }
        for other in ["level_202", "level_208_ice"] {
            assert!(
                encounters(other).all(|e| *e != WAY && *e != WAY_2),
                "{other}"
            );
            let off = level_config(Mode::Play, &files, 5, other);
            for name in [
                "Enemy_Troll_BFG(1)",
                "GasJet_Pipe(13)",
                "Enemy_PandoranLobber(1)",
            ] {
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
        }
    }
}
