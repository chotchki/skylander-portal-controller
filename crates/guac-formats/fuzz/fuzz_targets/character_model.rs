#![no_main]

use guac_formats::igz::Igz;
use guac_formats::material::{Material, pixel_variant_exists, samplers};
use guac_formats::model::{
    FALL_SLOT, HERO_SOLID, JOG_SLOT, JointMap, LANDING_SLOT, RUN_SLOT, WALK_SLOT, hero_actor,
    portal_model, skeleton, slot_clip,
};
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
    // A slot clip that reads is from its own slot and poses the skeleton.
    if let Ok(s) = skeleton(&file, &p.hero) {
        for slot in [WALK_SLOT, JOG_SLOT, RUN_SLOT, FALL_SLOT, LANDING_SLOT] {
            if let Ok(Some(c)) = slot_clip(&file, &p.hero, &s, slot) {
                assert_eq!(c.slot, slot);
                assert_eq!(p.hero.clip_slots[slot], Some(c.info));
                let local = c.clip.local_pose(&s, 0.0).expect("checked to pose");
                assert!(s.model_pose(&local).is_some());
            }
        }
    }
    // The hero found has a solid that blocks and meets the level.
    if let Ok(a) = hero_actor(&file, &p, "009_Eruptor") {
        let c = a.collision.expect("a hero has a solid");
        assert_eq!(c.properties & HERO_SOLID, HERO_SOLID);
    }
});
