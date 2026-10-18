//! guac-extract: reads the user's own disc dump (opened read-only, never written)
//! and writes the extract guacamole plays from, outside any git working tree.
//!
//! AB.1 is the scaffold. Identity and the manifest (AB.4), the census (AB.5),
//! the intro index (AB.6) and the transcode (AB.8) land here.

pub use guac_ir::IR_VERSION;

/// The extract dir's layout version (design decision 5). A manifest written
/// under another layout or [`IR_VERSION`] is stale and its identity dir is
/// rebuilt.
pub const LAYOUT_VERSION: u32 = 1;
