//! Level 203's start area (openspec AS.704), as [`crate::level_208`] lists
//! 208's: what of the level's own scripts it needs, by placement name.

/// The archive the story reaches after 202.
pub const LEVEL: &str = "level_203_the_machine";

/// The start area's placements, by name, listed and never walked (openspec
/// AS.704).
pub const START: [&str; 18] = [
    "molekin_at_start",
    "miner_hat",
    "_TRIGGER_crushers_begining",
    "203_Crusher_begining_a",
    "203_Crusher_begining_b",
    "Chain_203_old",
    "Chain_203_old_pole",
    "DamageZone",
    "new_conveyor",
    "new_conveyor(1)",
    "new_conveyor(2)",
    "new_conveyor(3)",
    "set_reset_point_to_1",
    "reset_point_ON_1",
    "reset_point_ON_2",
    "reset_point_OFF_1",
    "reset_point_OFF_2",
    "DamageZone_Crushers_203",
];

/// [`START`] on 203 alone.
pub fn start(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    START.iter().copied().filter(move |_| on)
}

/// The first fight, before the start area's gate (openspec AS.705).
pub const WAY: crate::scripts::Encounter = crate::scripts::Encounter {
    level: LEVEL,
    placements: &[
        "Setup_001_Controller",
        "Setup_001_Trigger(1)",
        "Setup_001_Intro_Path_A1",
        "Enemy_Troll_BFG(1)",
        "Enemy_Chompy_Fire(1)",
        "Enemy_Chompy_Fire(2)",
        "Enemy_Chompy_Fire(3)",
        "Enemy_Chompy_Fire(4)",
        "Enemy_Chompy_Fire(5)",
        "Enemy_Chompy_Fire(66)",
        "Enemy_Chompy_Fire(67)",
        "Enemy_Chompy_Fire(68)",
        "GasJet_Pipe(13)",
    ],
    clones: &[
        "203_Chompy_Pipe_Fire_p1",
        "203_Chompy_Pipe_Fire_p2",
        "Troll_BF_Gun",
        "Troll_BFG_Bullet",
        "Troll_BFG_Mine",
        "Troll_BFG_Mine_Claw",
    ],
    steered: &[],
};
