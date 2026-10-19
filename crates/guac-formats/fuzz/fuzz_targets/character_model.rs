#![no_main]

use guac_formats::igz::Igz;
use guac_formats::material::{Material, pixel_variant_exists, samplers};
use guac_formats::model::{JointMap, portal_model, skeleton};
use guac_formats::registry::walk_model;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = walk_model(data);
    let Ok(file) = Igz::parse(data) else {
        return;
    };
    let Ok(p) = portal_model(&file) else {
        return;
    };
    // A material that reads binds one texture per sampler, each in its role,
    // and names a variant that exists.
    for mesh in &p.hero.meshes {
        if let Ok(m) = Material::read(&file, &p.hero, mesh) {
            let roles: Vec<_> = m.textures.iter().map(|t| t.role).collect();
            assert_eq!(roles, samplers(m.pixel.base_mask));
            assert!(pixel_variant_exists(m.pixel.base_mask));
            for t in &m.textures {
                assert_eq!(t.sampler.format_hint == guac_formats::material::TFN1, t.role.is_normal_map());
            }
        }
    }
    // A joint map that builds is a bijection onto the bones it names.
    if let Ok(s) = skeleton(&file, &p.hero)
        && let Ok(map) = JointMap::new(&s, &p.hero.bones)
    {
        for (j, &b) in map.bone_of_joint.iter().enumerate() {
            assert_eq!(map.joint_of_bone[b], Some(j));
        }
    }
});
