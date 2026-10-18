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

mod error;
pub mod iga;
mod reader;
pub mod sfo;

#[cfg(feature = "test-fixtures")]
pub mod registry;

pub use error::{ErrorKind, FormatError};
