#![no_main]

use guac_formats::iga::Archive;
use guac_formats::registry::{IGA_WALK_BUDGET, walk_iga};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Every entry chunk by chunk, as the property test runs it.
    let _ = walk_iga(data);
    // Then whole-entry reads, which a successful one must return at exactly
    // the declared size, under the same budget so aliased chunks can't stall
    // the run.
    let Ok(archive) = Archive::parse(data) else {
        return;
    };
    let mut left = IGA_WALK_BUDGET;
    for entry in archive.entries() {
        let size = entry.size() as usize;
        if size > left {
            break;
        }
        left -= size;
        if let Ok(bytes) = archive.read(entry) {
            assert_eq!(bytes.len(), size, "{}", entry.name());
        }
    }
});
