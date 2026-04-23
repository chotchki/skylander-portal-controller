//! Figures and games.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------- Tag-level identity newtypes (PLAN 6.6.1a) ----------------
//
// Block 0 of every Skylanders tag carries a 24-bit "toy type" (called
// `figure_id` in `SkylanderFormat.md`) plus a 16-bit variant word. These
// newtypes exist to keep those primitives from being passed around as bare
// `u32` / `u16` — too easy to mix them up with array lengths, block offsets,
// or each other. `ToyTypeId` is deliberately NOT named `FigureId` to avoid
// collision with the [`FigureId`] string we use to key pack + scan entries
// across the library.

/// 24-bit "toy type" field from tag block 0 (`SkylanderFormat.md` — stored
/// as 3 little-endian bytes at offset 0x10). Two figures with different
/// toy_type are always different SKUs; same toy_type + different variant
/// *might* still be the same SKU depending on which variant bits differ
/// (see [`VARIANT_IDENTITY_MASK`] logic in `skylander-sky-parser`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToyTypeId(pub u32);

impl ToyTypeId {
    pub const fn new(v: u32) -> Self {
        ToyTypeId(v)
    }
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for ToyTypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 24-bit hex, zero-padded — matches the "0x0001CE" form we use
        // everywhere else for this field.
        write!(f, "0x{:06X}", self.0)
    }
}

/// Raw 16-bit variant word from tag block 0 offset 0x13. Encodes deco id,
/// game-generation year code, and a handful of repose / light-core /
/// supercharger flags. Use [`MaskedVariant`] when comparing for dedup —
/// raw variants include runtime state (year code, in-game-variant bit)
/// that isn't part of canonical identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TagVariant(pub u16);

impl TagVariant {
    pub const fn new(v: u16) -> Self {
        TagVariant(v)
    }
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl std::fmt::Display for TagVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:04X}", self.0)
    }
}

/// Bits of the variant word that encode the figure's *canonical identity*.
/// AND a raw [`TagVariant`] with this before keying a dedup lookup; we keep
/// deco_id (bits 0..8), is_supercharger (8), is_lightcore (9), and
/// is_reposed (11); we drop is_in_game_variant (10) and year_code
/// (12..16) because those encode **runtime/game state**, not identity.
///
/// Concrete: pack Snap Shot stores `variant=0x0000` (canonical, no game-
/// generation tag), while a live-scanned physical Snap Shot stores
/// `variant=0x3000` (year_code 3 = Trap Team). Masked, both collapse
/// to `0x0000` and dedup correctly. PLAN 6.5.5a.
pub const VARIANT_IDENTITY_MASK: u16 = 0x0BFF;

impl TagVariant {
    /// Strip the non-identity bits. Two tags with the same `mask_to_identity()`
    /// are the same canonical SKU regardless of which game-generation last
    /// touched them.
    pub const fn mask_to_identity(self) -> MaskedVariant {
        MaskedVariant(self.0 & VARIANT_IDENTITY_MASK)
    }
}

/// Variant word with the non-identity bits (year_code, is_in_game_variant)
/// stripped via [`VARIANT_IDENTITY_MASK`]. Deliberately distinct from
/// [`TagVariant`] so you can't accidentally compare a raw variant to an
/// identity-masked one and miss a dedup hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MaskedVariant(pub u16);

impl MaskedVariant {
    pub const fn new(v: u16) -> Self {
        MaskedVariant(v)
    }
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl std::fmt::Display for MaskedVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:04X}", self.0)
    }
}

/// Canonical identity pair used for pack-vs-scan dedup. Two physical tags
/// with the same [`TagIdentity`] are the same canonical SKU (e.g. a pack-
/// fresh Fire Reactor crystal and a scanned user-customized DELFOX both
/// resolve to the same TagIdentity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TagIdentity {
    pub toy_type: ToyTypeId,
    pub variant: MaskedVariant,
}

impl TagIdentity {
    pub const fn new(toy_type: ToyTypeId, variant: MaskedVariant) -> Self {
        TagIdentity { toy_type, variant }
    }

    /// Canonical string form used as the pack-figure [`FigureId`] after
    /// PLAN 6.6's rekey: `"{toy_type:06x}-{variant:04x}"`, e.g.
    /// `"0001ce-0000"`. Hyphen-separated lowercase hex, URL-safe, sortable.
    pub fn to_canonical_id_string(self) -> String {
        format!("{:06x}-{:04x}", self.toy_type.0, self.variant.0)
    }
}

impl std::fmt::Display for TagIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_canonical_id_string())
    }
}

/// 4-byte Mifare NUID (the physical tag's UID). Used to key scan-only
/// library entries so two physical copies of the same SKU stay distinct
/// even though they'd collapse to one [`TagIdentity`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MifareNuid(pub [u8; 4]);

impl MifareNuid {
    pub const fn new(bytes: [u8; 4]) -> Self {
        MifareNuid(bytes)
    }
    pub const fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }
    /// Canonical hex form: 8 uppercase hex chars, no separator. Matches
    /// the scanned-dump filename stem and the canonical form we've used
    /// everywhere since 6.5.0 (`7FC1ADA3`).
    pub fn to_hex_string(self) -> String {
        self.0.iter().map(|b| format!("{:02X}", b)).collect()
    }
}

impl std::fmt::Display for MifareNuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex_string())
    }
}

// ---------------- Library-level identifier ----------------

/// Stable figure identifier used across the library — opaque string. The
/// format varies by origin:
/// - Pack figures: `"{toy_type:06x}-{variant:04x}"` (via [`TagIdentity`]).
///   PLAN 6.6 switched this from the prior SHA-of-path scheme.
/// - Scan-only figures: `"scan:{uid_hex}"` so each physical tag is its
///   own library card even when SKUs collide.
/// - Pack figures whose block 0 can't be parsed: `"sha:{old-hash}"` —
///   escape valve so nothing orphans silently.
/// Consumers should continue to treat the inner string as opaque.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FigureId(pub String);

impl FigureId {
    pub fn new(s: impl Into<String>) -> Self {
        FigureId(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Build a pack-figure id from its [`TagIdentity`] — the canonical
    /// post-6.6 form.
    pub fn from_tag_identity(id: TagIdentity) -> Self {
        FigureId(id.to_canonical_id_string())
    }

    /// Build a scan-only id from its Mifare NUID.
    pub fn from_scanned_nuid(uid: MifareNuid) -> Self {
        FigureId(format!("scan:{}", uid.to_hex_string()))
    }
}

impl std::fmt::Display for FigureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// PS3 game serial (e.g. `BLUS30968` for Skylanders Giants).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameSerial(pub String);

impl GameSerial {
    pub fn new(s: impl Into<String>) -> Self {
        GameSerial(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for GameSerial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Skylander element. `None` at the outer layer means the item isn't
/// elementally typed (Items, Adventure Packs, Kaos).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    Air,
    Dark,
    Earth,
    Fire,
    Life,
    Light,
    Magic,
    Tech,
    Undead,
    Water,
}

/// Broad classification derived from folder/category at index time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// Regular player-controlled skylander.
    Figure,
    /// Sidekick (mini helper).
    Sidekick,
    /// Giants-generation giant (e.g. Tree Rex, Bouncer).
    Giant,
    /// Consumable/support item.
    Item,
    /// Trap Team elemental trap.
    Trap,
    /// Adventure-pack place.
    AdventurePack,
    /// Imaginators creation crystal.
    CreationCrystal,
    /// SuperChargers vehicle.
    Vehicle,
    /// Kaos-specific entry.
    Kaos,
    /// Fallback for anything the indexer couldn't classify.
    Other,
}

/// Game origin, used in `Figure` and surfaced in the filter UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameOfOrigin {
    SpyrosAdventure,
    Giants,
    SwapForce,
    TrapTeam,
    Superchargers,
    Imaginators,
    /// Cross-cutting Items / Adventure Packs / Sidekicks collections at the
    /// firmware pack root that don't tie to a single game of origin.
    CrossGame,
    /// Scan-discovered figure whose `(figure_id, variant)` isn't in the pack
    /// and for which we haven't resolved the game-of-origin yet. PLAN 6.5.5
    /// landing spot; 6.5.5b will fill this in via a tag-id → metadata table.
    Unknown,
}

/// Server-side figure record. Holds filesystem details — **never** serialize
/// this directly to the phone; convert to `PublicFigure`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Figure {
    pub id: FigureId,
    /// Display name shown to users (from the filename, with punctuation/casing fixed).
    pub canonical_name: String,
    /// Groups reposes together. For reposes this is the base figure name;
    /// for base figures it's the same as `canonical_name`.
    pub variant_group: String,
    /// `"base"` for the primary figure; a short tag (`"Legendary"`, `"Dark"`,
    /// etc.) for reposes. Used by the phone UI to cycle variants.
    pub variant_tag: String,
    pub game: GameOfOrigin,
    pub element: Option<Element>,
    pub category: Category,
    /// Absolute path on disk. **Server-private.**
    pub sky_path: PathBuf,
    /// Path to the element-symbol PNG, if present in the pack. **Server-private.**
    pub element_icon_path: Option<PathBuf>,
    /// Canonical tag-level identity (PLAN 6.6.1c). Populated by the indexer
    /// from a parse of `sky_path`'s block 0. `None` only for the rare
    /// parse-failure case — the indexer falls back to a SHA-prefixed
    /// [`FigureId`] in that path and logs a warning. Consumers that want
    /// to dedup pack-vs-scan (or build a `TagIdentity → FigureId` reverse
    /// map) should read this directly rather than re-parsing `sky_path`.
    #[serde(default)]
    pub tag_identity: Option<TagIdentity>,
}

impl Figure {
    pub fn to_public(&self) -> PublicFigure {
        PublicFigure {
            id: self.id.clone(),
            canonical_name: self.canonical_name.clone(),
            variant_group: self.variant_group.clone(),
            variant_tag: self.variant_tag.clone(),
            game: self.game,
            element: self.element,
            category: self.category,
        }
    }
}

/// Phone-safe figure. No filesystem paths. No relative_path. This is what
/// `GET /api/figures` returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicFigure {
    pub id: FigureId,
    pub canonical_name: String,
    pub variant_group: String,
    pub variant_tag: String,
    pub game: GameOfOrigin,
    pub element: Option<Element>,
    pub category: Category,
}

/// Game the user can launch. MVP-scope: Phase 2 just lists games; Phase 3
/// wires up the CLI boot via `sky_root`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: GameSerial,
    pub display_name: String,
    /// Filesystem root of the game (from `RPCS3/config/games.yml`).
    /// Server-private.
    #[serde(skip)]
    pub sky_root: Option<PathBuf>,
}
