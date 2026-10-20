//! Pure parsers over the byte formats Guacamole reads off the user's disc dump.
//!
//! Every parser takes `&[u8]` and returns a typed value or a [`FormatError`]
//! naming the format, the field and the offset. No I/O, no `unsafe`, and every
//! length read from a file is checked against what the input holds before
//! anything is allocated for it.
//!
//! Each parser registers in [`registry`] (`test-fixtures` feature), which the
//! property test (`tests/properties.rs`) and the cargo-fuzz targets (`fuzz/`)
//! walk, so a new parser is fuzzed from the day it lands.

#![forbid(unsafe_code)]

pub mod collision;
pub mod edge;
mod error;
pub mod exports;
pub mod havok;
pub mod iga;
pub mod igz;
pub mod language;
pub mod level;
pub mod material;
pub mod model;
pub mod progression;
mod reader;
pub mod roster;
pub mod sfo;
pub mod texture;
pub mod toys;

#[cfg(feature = "test-fixtures")]
pub mod registry;

pub use error::{ErrorKind, FormatError};

/// FNV-1a 32: a `.bld`'s TOC hashes names with it, and IGZ imports name
/// objects and namespaces by it.
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811C_9DC5, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(0x0100_0193)
    })
}
