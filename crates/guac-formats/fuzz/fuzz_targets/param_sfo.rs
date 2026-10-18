#![no_main]

use guac_formats::sfo::ParamSfo;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(sfo) = ParamSfo::parse(data) {
        // The accessors walk what parse kept; they must hold up too.
        let _ = (sfo.title_id(), sfo.app_ver(), sfo.version(), sfo.category());
    }
});
