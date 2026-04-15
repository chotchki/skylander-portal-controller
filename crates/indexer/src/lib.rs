//! Firmware pack indexer.
//!
//! Populated in 2.4. For now, just a placeholder so the workspace compiles.

use std::path::Path;

use anyhow::Result;
use skylander_core::Figure;

pub fn scan(_root: &Path) -> Result<Vec<Figure>> {
    Ok(Vec::new())
}
