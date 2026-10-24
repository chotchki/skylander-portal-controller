//! guacamole's library half: the 3D pieces the binary, its examples and its
//! GPU lanes share (openspec AE.8, AG.19), the census of what they draw
//! Giants with (AE.25), the level's script part linked at its load (AH.22),
//! its props (AG.25), who walks it (AG.21) and a hero's attach nodes posed by
//! his clips (AH.50), where the dev entry and the headless fight take him
//! (AH.25), and on macOS the pads from GameController (AG.53). The
//! binary's own modules (the TV, the driver, the extraction) stay in
//! `main.rs`.

pub mod anchors;
pub mod census;
pub mod character;
pub mod creature_models;
pub mod figure;
pub mod frame;
pub mod game_controller;
pub mod hands;
pub mod level_scene;
pub mod level_scripts;
pub mod player_loop;
pub mod props;
pub mod script_engine;
mod script_members;
pub mod scripts;
pub mod walkers;
