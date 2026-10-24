//! A dump folder holding only its PARAM.SFO, from cli's tests; picker's
//! tests stand on it too.

use std::path::Path;

use guac_formats::sfo::builder::SfoBuilder;

pub(crate) fn dump(dir: &Path, title_id: &str) {
    let game = dir.join("PS3_GAME");
    std::fs::create_dir_all(game.join("USRDIR")).unwrap();
    let sfo = SfoBuilder::disc_game(title_id, "01.00", "01.00").build();
    std::fs::write(game.join("PARAM.SFO"), sfo).unwrap();
}
