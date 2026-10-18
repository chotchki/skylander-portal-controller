//! guac-extract: reads the user's own disc dump (opened read-only, never written)
//! and writes the extract guacamole plays from, outside any git working tree.
//!
//! AB.4 is identity, the manifest and the extract root: [`root::ExtractRoot`]
//! resolves and checks where output goes, [`extract::open`] hashes the dump,
//! checks it against [`known::KNOWN_DUMPS`] and opens its identity dir. The
//! census (AB.5), the intro index (AB.6) and the transcode (AB.8) land here.

pub mod dump;
mod error;
pub mod extract;
pub mod known;
mod report;
pub mod root;

pub use error::{ExtractError, Result};
pub use extract::{DirState, IdentityDir, Manifest, Opened, Stale, open};
pub use guac_ir::IR_VERSION;
pub use root::{ExtractRoot, RootMode};

/// The extract dir's layout version (design decision 5). A manifest written
/// under another layout or [`IR_VERSION`] is stale and its identity dir is
/// rebuilt.
pub const LAYOUT_VERSION: u32 = 1;
