#![no_main]

use guac_formats::igz::Igz;
use guac_formats::interface::{Content, Interface};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(file) = Igz::parse(data) else {
        return;
    };
    let Ok(ui) = Interface::read(&file) else {
        return;
    };
    // What a read hands out hangs together: a text template names a font
    // it holds, and every glyph's box sits inside its atlas.
    for t in &ui.templates {
        if let Content::Text { face } = t.content {
            assert!(face < ui.fonts.len());
        }
    }
    for f in &ui.fonts {
        assert!(f.atlas < file.objects().len());
        for g in &f.glyphs {
            for axis in 0..2 {
                assert!((0.0..=1.0).contains(&g.uv_min[axis]));
                assert!(g.uv_min[axis] <= g.uv_max[axis] && g.uv_max[axis] <= 1.0);
            }
        }
    }
});
