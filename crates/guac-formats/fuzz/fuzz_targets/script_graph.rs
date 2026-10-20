#![no_main]

use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{BRANCH_END, ObjectRef};
use guac_formats::script::{Graph, Op};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(igz) = Igz::parse(data) else {
        return;
    };
    let Ok(g) = Graph::read(&igz) else {
        return;
    };
    // A graph that reads holds every position's op, and each block stays
    // inside its list.
    for s in &g.scripts {
        for (p, o) in s.ops.iter().enumerate() {
            let op = g.ops.get(o).expect("every position's op is read");
            let n = op.code().branch;
            match n {
                0 => {}
                BRANCH_END => assert!(op.class().is_flow()),
                n => assert!(op.class().opens_block() && p + usize::from(n) < s.ops.len()),
            }
        }
    }
    // Every operand an op's `_LHS` or `_RHS` names is read.
    for op in g.ops.values() {
        if let Op::SetValue(set) = op {
            for r in [set.lhs, set.rhs] {
                if let ObjectRef::Object(o) = r {
                    assert!(g.operands.contains_key(&o));
                }
            }
        }
    }
});
