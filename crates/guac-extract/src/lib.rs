//! guac-extract: reads the user's own disc dump (opened read-only, never written)
//! and writes the extract guacamole plays from, outside any git working tree.
//!
//! AB.4 is identity, the manifest and the extract root: [`root::ExtractRoot`]
//! resolves and checks where output goes, [`extract::open`] hashes the dump,
//! checks it against [`known::KNOWN_DUMPS`] and opens its identity dir.
//! [`census::census`] (AB.5) sniffs every file and IGA entry with
//! [`magic`], the list the game-magic guard checks the repo against.
//! [`intros`] (AB.6) maps each toy type to its intro movie into IR v1's
//! character table. [`transcode`] (AB.8) turns each intro movie into what
//! guacamole plays, through the ffmpeg [`ffmpeg`] finds. [`roster`] (AE.2)
//! reads the figures the disc defines into IR v2's roster, and [`models`]
//! (AE.6) each character's model, its textures and the 3D-UI rig, and
//! (AG.14) its hero side.
//! [`levels`] (AG.15) reads each level the build plays, chapter 1, in parts.
//! [`progression`] (AH.2) reads the level table, the save params and the
//! gold clamp, an extract-wide part of their own.
//! [`stages`] (AE.3) runs everything after identity in guacamole's order:
//! the roster and its portraits first, then the models with their hero side
//! (AG.14), then the levels, then ffmpeg and the intros.

pub mod census;
pub mod dump;
mod error;
pub mod extract;
pub mod ffmpeg;
pub mod intros;
pub mod known;
pub mod levels;
pub mod magic;
pub mod models;
pub mod progression;
mod report;
pub mod root;
pub mod roster;
pub mod stages;
pub mod transcode;

pub use error::{ExtractError, Result};
pub use extract::{
    DirState, IdentityDir, LevelRecord, Manifest, MovieRecord, Opened, Stale, TranscodeRecord,
    find_current, open,
};
pub use guac_ir::IR_VERSION;
pub use root::{ExtractRoot, RootMode};

/// The extract dir's layout version (design decision 5). A manifest written
/// under another layout or [`IR_VERSION`] is stale and its identity dir is
/// rebuilt. v2 (AE.3) adds `ir/roster.*` and `portraits/`; v3 (AE.6)
/// `models/`, `textures/` and `ir/rig.*`, so an extract made before the
/// models redoes itself; v4 (AG.14) `heroes/`, so one made before the hero
/// side does too, and (AG.15) `levels/`, each level's parts named in the
/// manifest, so a part a later build adds resumes alone.
pub const LAYOUT_VERSION: u32 = 4;
