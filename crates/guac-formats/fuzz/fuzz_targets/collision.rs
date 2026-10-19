#![no_main]

use guac_formats::collision;
use guac_formats::havok::physics::Collision;
use guac_formats::igz::Igz;
use guac_formats::registry::walk_collision;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = walk_collision(data);
    // The packfile on its own too, as a world's `_data` hands it over.
    let _ = Collision::parse(data);
    let Ok(file) = Igz::parse(data) else {
        return;
    };
    // A world that reads hangs together: every index names a vertex, every
    // triangle has its material and every number is finite.
    for (_, world) in collision::worlds(&file).take(16) {
        let Ok(world) = world else { continue };
        let Some(body) = &world.collision.body else {
            continue;
        };
        let mesh = &body.mesh;
        assert_eq!(mesh.materials.len(), mesh.triangles.len());
        assert!(mesh.triangles.iter().all(|t| {
            t.indices
                .iter()
                .all(|&i| usize::from(i) < mesh.vertices.len())
        }));
        assert!(
            mesh.vertices
                .iter()
                .flatten()
                .chain(&body.translation)
                .all(|x| x.is_finite())
        );
        assert!(mesh.radius.is_finite());
        assert_eq!(body.world_vertices().len(), mesh.vertices.len());
        assert_eq!(
            mesh.material_counts().values().sum::<usize>(),
            mesh.triangles.len()
        );
        assert_eq!(mesh.bounds().is_some(), !mesh.vertices.is_empty());
    }
});
