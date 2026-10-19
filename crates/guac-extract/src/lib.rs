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
//! reads the figures the disc defines into IR v2's roster. [`stages`] (AE.3)
//! runs everything after identity in guacamole's order: the roster and its
//! portraits first, then ffmpeg and the intros.

pub mod census;
pub mod dump;
mod error;
pub mod extract;
pub mod ffmpeg;
pub mod intros;
pub mod known;
pub mod magic;
mod report;
pub mod root;
pub mod roster;
pub mod stages;
pub mod transcode;

pub use error::{ExtractError, Result};
pub use extract::{
    DirState, IdentityDir, Manifest, MovieRecord, Opened, Stale, TranscodeRecord, open,
};
pub use guac_ir::IR_VERSION;
pub use root::{ExtractRoot, RootMode};

/// The extract dir's layout version (design decision 5). A manifest written
/// under another layout or [`IR_VERSION`] is stale and its identity dir is
/// rebuilt. v2 (AE.3) adds `ir/roster.*` and `portraits/`.
pub const LAYOUT_VERSION: u32 = 2;
