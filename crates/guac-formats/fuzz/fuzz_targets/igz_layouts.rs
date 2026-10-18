#![no_main]

use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{Hero, PhysicsModel, read_all};
use guac_formats::registry::walk_layouts;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = walk_layouts(data);
    let Ok(file) = Igz::parse(data) else {
        return;
    };
    // A hero that reads must hang together: every key it hands out is held.
    for (i, _) in read_all::<PhysicsModel>(&file) {
        let Ok(hero) = Hero::read(&file, i) else {
            continue;
        };
        for mesh in &hero.meshes {
            let geometry = &hero.geometries[&mesh.geometry];
            assert!(geometry.segments.iter().all(|s| hero.segments.contains_key(s)));
            assert!(mesh.palette.is_none_or(|p| hero.palettes.contains_key(&p)));
        }
        for slot in hero.clip_slots.iter().flatten() {
            assert!(hero.clips.contains_key(slot));
        }
    }
});
