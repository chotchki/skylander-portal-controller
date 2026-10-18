#![no_main]

use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{Image2, read_all};
use guac_formats::registry::{TEXTURE_WALK_BUDGET, walk_textures};
use guac_formats::texture::Texture;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = walk_textures(data);
    let Ok(file) = Igz::parse(data) else {
        return;
    };
    // Every texture that reads writes a KTX2 the ktx2 crate takes back whole,
    // under the walk's budget so images sharing a block can't stall the run.
    let mut left = TEXTURE_WALK_BUDGET;
    for (i, _) in read_all::<Image2>(&file) {
        let Ok(t) = Texture::read(&file, i) else {
            continue;
        };
        let bytes = t.to_ktx2();
        let r = ktx2::Reader::new(&bytes[..]).expect("our KTX2 parses");
        let h = r.header();
        assert_eq!((h.pixel_width, h.pixel_height), (t.width, t.height));
        assert!(r.levels().map(|l| l.data).eq(t.levels.iter().map(Vec::as_slice)));
        let n: usize = t.levels.iter().map(Vec::len).sum();
        left = left.saturating_sub(n.max(1));
        if left == 0 {
            break;
        }
    }
});
