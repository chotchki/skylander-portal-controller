#![no_main]

use guac_formats::igz::Igz;
use guac_formats::level::{GroupKind, Placements, StaticModel, scenes};
use guac_formats::registry::walk_level;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = walk_level(data);
    let Ok(file) = Igz::parse(data) else {
        return;
    };
    // A scene that walks hangs together: every key it hands out is held,
    // every group but the root has its parent above it, and a draw sits in
    // its parent's frame.
    for (_, scene) in scenes(&file).take(16) {
        let Ok(scene) = scene else { continue };
        assert!(scene.groups[&scene.root].parent.is_none());
        for (&i, group) in &scene.groups {
            if let Some(p) = group.parent {
                assert!(scene.groups[&p].children.contains(&i));
            } else {
                assert_eq!(i, scene.root);
            }
            if let Some(u) = group.user_info {
                assert!(matches!(scene.groups[&u].kind, GroupKind::UserInfo(_)));
            }
        }
        for draw in &scene.draws {
            // A scene's root is a group, so every draw has a parent.
            let parent = &scene.groups[&draw.parent.expect("a scene draw's parent")];
            assert!(parent.children.contains(&draw.technique_index));
            // Bit for bit: composed matrices can overflow to NaN.
            let bits = |m: &[[f32; 4]; 4]| {
                m.as_flattened()
                    .iter()
                    .map(|x| x.to_bits())
                    .collect::<Vec<_>>()
            };
            assert_eq!(bits(&draw.world), bits(&parent.world));
            assert_eq!(draw.user_info, parent.user_info);
            let geometry = &scene.geometries[&draw.geometry];
            assert!(
                geometry
                    .segments
                    .iter()
                    .all(|s| scene.segments.contains_key(s))
            );
        }
    }
    if let Ok(p) = Placements::read(&file) {
        for actor in &p.actors {
            assert!(actor.info.model.is_none_or(|m| p.models.contains_key(&m)));
            assert!(
                actor
                    .info
                    .waypoints
                    .is_none_or(|l| p.waypoint_lists.contains_key(&l))
            );
        }
        // A static model's graph hangs together as a scene's does; a
        // technique alone is its own root, with no parent and no groups.
        for &model in p.models.keys().take(16) {
            let Ok(m) = StaticModel::read(&file, model) else {
                continue;
            };
            let g = &m.graph;
            for draw in &g.draws {
                match draw.parent {
                    Some(parent) => {
                        assert!(g.groups[&parent].children.contains(&draw.technique_index));
                    }
                    None => {
                        assert_eq!(draw.technique_index, g.root);
                        assert!(g.groups.is_empty());
                    }
                }
            }
        }
    }
});
