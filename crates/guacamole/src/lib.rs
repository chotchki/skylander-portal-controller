//! guacamole's library half: the 3D pieces the binary, its examples and its
//! GPU lanes share (openspec AE.8, AG.19), the census of what they draw
//! Giants with (AE.25) and the level's script part linked at its load
//! (AH.22). The binary's own modules (the TV, the driver, the
//! extraction) stay in `main.rs`.

pub mod census;
pub mod character;
pub mod level_scene;
pub mod level_scripts;
