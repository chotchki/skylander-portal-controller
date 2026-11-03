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
//! [`scripts`] (AH.22) holds the whitelist and reads the fight's script
//! data outside a level: the script entries and the models they spawn.
//! [`particles`] (AH.87) reads the emitters those archives place.
//! [`interface`] (AT.8) reads the sprite templates, fonts and English text
//! of the shared, character and menu archives, and each level's.
//! [`qos`] (AG.298) keeps what the extractor starts at the class of the
//! thread that runs it, so guacamole's extraction stays below the TV.
//! [`stages`] (AE.3) runs everything after identity in guacamole's order:
//! the roster and its portraits first, then the models with their hero side
//! (AG.14) and palettes (AH.22), then the levels, then the scripts, then
//! ffmpeg and the intros.

pub mod census;
pub mod dump;
mod error;
pub mod extract;
pub mod ffmpeg;
pub mod interface;
pub mod intros;
pub mod known;
pub mod levels;
pub mod magic;
pub mod models;
pub mod particles;
pub mod progression;
pub mod qos;
mod report;
pub mod root;
pub mod roster;
pub mod screens;
pub mod scripts;
pub mod sounds;
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
/// manifest, so a part a later build adds resumes alone. AH.2's
/// `ir/progression.*` is such a part outside the levels (the manifest's
/// `parts`), so v4 holds, and so are AH.22's `palettes/`, `scripts/` and
/// `spawns/`, beside each level's `script.igz` and `palettes.*`, AE.31's
/// `portraits` (the same files, now the right way up), AH.50's `nodes/` and
/// AH.87's `particles/` and AT.8's `interface/`, beside each level's
/// `sprites.*`, and AT.30's `models` (the files v3 and v4 added, with the
/// stage's record `ir/models.*`).
pub const LAYOUT_VERSION: u32 = 4;

/// The extract-wide parts this build reads (the manifest's `parts`), each
/// recorded once its files are in: AE.31's upright portraits, AH.2's
/// progression, AH.22's palettes, script entries and spawn models, AS.53's
/// screen scripts, AH.50's attach nodes, AH.87's emitters, AR.5's sounds,
/// AT.8's interface, AT.30's models. A start is current only once every
/// one is, so an extract made before one resumes for it alone.
pub const PARTS: [&str; 11] = [
    roster::PORTRAITS,
    progression::PART,
    models::PART,
    models::PALETTES,
    scripts::SCRIPTS,
    scripts::SPAWNS,
    screens::PART,
    models::NODES,
    particles::PART,
    sounds::PART,
    interface::PART,
];
