#![no_main]

use guac_formats::igz::{Igz, Location, Ref};
use guac_formats::registry::walk_igz;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = walk_igz(data);
    let Ok(file) = Igz::parse(data) else {
        return;
    };
    // Whatever parse accepted must resolve: every marker, and every pointer
    // to an object must land on one.
    for &(at, _) in file.markers() {
        match file.reference(at, "fuzz") {
            Ok(Ref::Object(i)) => assert!(i < file.objects().len()),
            Ok(_) => {}
            Err(e) => panic!("parse accepted a marker it can't resolve: {e}"),
        }
    }
    for object in file.objects() {
        assert!(object.size >= 8);
        assert!(object.at.offset_by(object.size).is_some());
        let _ = file.u32_at(object.at, "fuzz").expect("object in range");
    }
    let _ = file.object_containing(Location(0));
});
