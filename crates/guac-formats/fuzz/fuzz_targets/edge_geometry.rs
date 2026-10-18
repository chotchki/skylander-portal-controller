#![no_main]

use guac_formats::edge::indexes::{compress, decompress};
use guac_formats::edge::{Kind, Segment};
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{EdgeGeometrySegment, read_all};
use guac_formats::registry::{EDGE_WALK_BUDGET, edge_cost, walk_edge};
use libfuzzer_sys::fuzz_target;

fn canon(tris: impl Iterator<Item = [u16; 3]>) -> Vec<[u16; 3]> {
    tris.map(|t| {
        let r = (0..3).min_by_key(|&r| t[r]).unwrap();
        [t[r], t[(r + 1) % 3], t[(r + 2) % 3]]
    })
    .collect()
}

fuzz_target!(|data: &[u8]| {
    let _ = walk_edge(data);
    let Ok(file) = Igz::parse(data) else {
        return;
    };
    // Every segment that decodes holds the findings' invariants, and its
    // triangles survive the reference encoder.
    let mut left = EDGE_WALK_BUDGET;
    for (i, layout) in read_all::<EdgeGeometrySegment>(&file) {
        let Ok(layout) = layout else { continue };
        left = left.saturating_sub(edge_cost(&layout).max(1));
        if let Ok(s) = Segment::decode(&file, i, &layout) {
            let n = s.vertex_count();
            assert_eq!(s.indexes.indexes.len(), usize::from(s.config.num_indexes));
            assert!(s.indexes.indexes.iter().all(|&x| usize::from(x) < n));
            for a in &s.attributes {
                assert_eq!(a.values.len(), n * usize::from(a.components));
            }
            assert_eq!(s.skin.is_some(), s.config.kind == Kind::Skinned);
            if let Some(skin) = &s.skin {
                assert_eq!(skin.influences.len(), n);
                for v in &skin.influences {
                    assert_eq!(v.iter().map(|i| u32::from(i.weight)).sum::<u32>(), 255);
                    assert!(v
                        .iter()
                        .filter(|i| i.weight > 0)
                        .all(|i| usize::from(i.local) < skin.palette_slots.len()));
                }
            }
            let tris: Vec<[u16; 3]> = s.triangles().collect();
            if let Some(again) = compress(&tris) {
                let back = decompress(&again, s.config.num_indexes, s.config.num_vertexes)
                    .expect("our encoding decodes");
                // A code-3 triangle can come back rotated onto a shared edge.
                assert_eq!(canon(back.triangles()), canon(tris.iter().copied()));
            }
        }
        if left == 0 {
            break;
        }
    }
});
