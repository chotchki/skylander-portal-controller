#![no_main]

use std::collections::BTreeSet;

use guac_formats::igz::Igz;
use guac_formats::registry::walk_script_ops;
use guac_formats::script::{Class, Op};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = walk_script_ops(data);
    let Ok(igz) = Igz::parse(data) else {
        return;
    };
    for (i, o) in igz.objects().iter().enumerate() {
        let Ok(op) = Op::read(&igz, i) else {
            continue;
        };
        // An op that reads is of its own class, and lists each registry
        // field once, `OpCode`'s three first.
        assert_eq!(Some(op.class()), Class::from_type(igz.type_name(o)));
        let fields = op.fields();
        let names: BTreeSet<_> = fields.iter().map(|(n, _)| *n).collect();
        assert_eq!(names.len(), fields.len());
        assert_eq!(fields[2].0, "_branchPC");
    }
});
