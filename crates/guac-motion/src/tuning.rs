//! A hero's numbers (openspec AG.16; hero-movement's Top speed, Speeding up
//! and slowing down, Turning and hard reversals, Falling and landing). The
//! walk runs on numbers Giants' movement script holds for him, not on his
//! `ActorParameters` (AG.8: those read 10,000 ft/s, no acceleration, 10,000°/s
//! through every walk), so these take its place until AJ's VM runs the script
//! (stand-in ledger: `tuning`): Eruptor's live values (AG.8, AG.10, AG.38), in
//! the script's own units, feet and seconds.

use crate::solid::{HavokSolid, SolidInfo, SolidKind};

/// Metres in a foot: the script's and the disc's lengths are feet, the
/// world's metres.
pub const FOOT: f32 = 0.3048;

/// The movement script's float tolerance: a length is "over" 21.5 ft/s only
/// by more than this (AG.38: at once up to 1.00e-4 ft/s over, steering from
/// 1.01e-4).
pub const FLOAT_TOLERANCE: f32 = 1.0e-4;

/// The band above the ground a frame off it counts as a crest's (AG.38): 0.4
/// ft, in which an unsupported frame adds [`BAND_KICK`] downward instead of
/// gravity.
pub const BAND: f32 = 0.4;
/// What a frame off the ground within [`BAND`] adds downward, ft/s.
pub const BAND_KICK: f32 = 10.0;
/// The band adds nothing to a walk rising faster than this, ft/s (as read,
/// AG.38: never seen).
pub const BAND_RISE: f32 = 20.0;

/// Seconds in the original's nominal frame: a step at full speed.
pub const NOMINAL_STEP: f32 = 1.0 / 60.0;

/// How fast every scripted mover falls off the ground above [`BAND`], ft/s²,
/// and the fastest it falls, ft/s: the movement update's own numbers
/// (`movementUpdate.ai`'s `Movement - Update Force on Mover`: 136.21 times
/// the mover's time since its last update, the vertical speed set to -120
/// past it), literals the same for every character, which AG.10 measured as
/// Eruptor's fall. A character's description holds a gravity of its own
/// (128 to 120 on each of the 46 AG.27's audit read one off), which the
/// movement update never falls a mover by: `Movement - Remove Mover` ties
/// it to his `ActorParameters` once a script takes him off the movers, the
/// engine's fall for those frames (an action's, AG.1's 128 and 120 cap), so
/// it's never his walk's (openspec AG.173).
pub const MOVER_GRAVITY: f32 = 136.21;
pub const MOVER_TERMINAL: f32 = 120.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// Top speed at full stick, ft/s (`Speed_Slow`'s live 21.5 for Eruptor).
    pub max_speed: f32,
    /// The push along the stick, ft/s² times the stick's strength, on the
    /// ground and in the air.
    pub push: f32,
    /// What a let-go takes off the walk's across-the-ground part, ft/s², on
    /// the ground and up in the air.
    pub let_go: f32,
    pub air_let_go: f32,
    /// °/s at full stick; a part stick turns him slower in proportion (AG.8:
    /// no S² and no angle factor, the read model's were wrong).
    pub turn_rate: f32,
    /// Off the ground above [`BAND`], ft/s² and the fastest fall, ft/s.
    pub gravity: f32,
    pub terminal: f32,
    /// His collision's width as authored, feet, before his scale: the edge
    /// probe's slow point is half of it plus a foot along the stick.
    pub width: f32,
}

impl Tuning {
    /// No walk of its own: what an actor the engine alone moves carries
    /// ([`crate::hero::Hero::actor`]); nothing reads it there.
    pub const NONE: Tuning = Tuning {
        max_speed: 0.0,
        push: 0.0,
        let_go: 0.0,
        air_let_go: 0.0,
        turn_rate: 0.0,
        gravity: 0.0,
        terminal: 0.0,
        width: 0.0,
    };

    /// Eruptor in chapter 1 with no hat and no heroics: 21.5 ft/s, 160 ft/s²
    /// up, 80 down (50 in the air), 1440°/s (AG.8, AG.38, AG.39); the
    /// movers' fall, 136.21 ft/s² off the ground (AG.10) to 120 ft/s (as
    /// read: no fall in reach is long enough to show it, AG.47); 4 ft wide
    /// (AG.8's `live_hero`).
    pub const ERUPTOR: Tuning = Tuning {
        max_speed: 21.5,
        push: 160.0,
        let_go: 80.0,
        air_let_go: 50.0,
        turn_rate: 1440.0,
        gravity: MOVER_GRAVITY,
        terminal: MOVER_TERMINAL,
        width: 4.0,
    };
}

/// Eruptor's scale in chapter 1, AG.8's live read: 150 %, which his solid and
/// his drawn model both take. A script sets it; the disc's placement holds
/// 100 (AG.14).
pub const ERUPTOR_SCALE: f32 = 1.5;

/// Eruptor's `CollisionInfo` (AG.8's live read): a 4 x 6 x 3 ft sphere,
/// before his scale.
pub const ERUPTOR_SHAPE: SolidInfo = SolidInfo {
    kind: SolidKind::Sphere,
    offset: [0.0; 3],
    rotation: [0.0; 3],
    width: 4.0,
    height: 6.0,
    length: 3.0,
};

/// Eruptor's solid as the original builds it: [`ERUPTOR_SHAPE`] at
/// [`ERUPTOR_SCALE`], a 0.8644 m sphere centred 0.9144 m up.
pub fn eruptor_solid() -> HavokSolid {
    ERUPTOR_SHAPE
        .havok([ERUPTOR_SCALE * 100.0; 3])
        .expect("a sphere has a shape")
}
