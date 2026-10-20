#![no_main]

use guac_formats::igz::Igz;
use guac_formats::progression::gold_clamp;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(global) = Igz::parse(data) else {
        return;
    };
    // A clamp that reads is a positive amount.
    if let Ok(clamp) = gold_clamp(&global) {
        assert!(clamp > 0 && clamp <= i32::MAX as u32);
    }
});
