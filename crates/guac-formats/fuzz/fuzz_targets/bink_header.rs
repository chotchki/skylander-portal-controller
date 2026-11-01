#![no_main]

use guac_formats::bink::Header;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(h) = Header::read(data) {
        // The lookup walks what read kept.
        let _ = (h.track(0), h.track(7));
    }
});
