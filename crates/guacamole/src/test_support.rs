//! The binary's shared test harness (openspec AQ.33): every helper more than
//! one module's tests use, moved out of the module it was written beside, so
//! no module's tests reach into another module's file. One submodule per
//! module the helpers came from; a helper only its own module's tests use
//! stays there.

pub(crate) mod cli;
pub(crate) mod heard;
pub(crate) mod hero_scripts;
pub(crate) mod level;
pub(crate) mod physics;
pub(crate) mod play;
pub(crate) mod starfield;
pub(crate) mod surfaces;
pub(crate) mod test_pad;
pub(crate) mod turntable;
pub(crate) mod tv;
pub(crate) mod video;
