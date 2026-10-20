//! The level parts on the synthetic level `guac_formats`' builder makes
//! (AG.15): level 213's shapes in small, every value known.

use guac_formats::collision::builder::ramp;
use guac_formats::igz::Igz;
use guac_formats::level::builder::{STAGE_WORLD, StageSpec, stage_level, stage_sample};
use guac_formats::model::builder::AttrSpec;
use guac_ir::Sha256;
use guac_ir::model::{Blend, Glow, TextureRole};

use super::*;

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([5; 32]),
    }
}

const LEVEL: &str = "level_213_tutotial";

fn parsed(spec: &StageSpec) -> &'static Igz<'static> {
    let bytes: &'static [u8] = Vec::leak(stage_level(spec));
    Box::leak(Box::new(Igz::parse(bytes).unwrap()))
}

#[test]
fn the_scene_is_tfbmulti2s_and_the_other_effects_with_the_sky_beside() {
    let igz = parsed(&stage_sample());
    let r = scene(igz, LEVEL, identity()).unwrap();
    assert_eq!(
        (r.ir.level.as_str(), r.ir.identity.clone()),
        (LEVEL, identity())
    );
    let [g] = &r.ir.scenes[..] else {
        panic!("{} scenes", r.ir.scenes.len());
    };
    assert_eq!(g.world, STAGE_WORLD);
    let kinds: Vec<&str> = g
        .materials
        .iter()
        .map(|m| match m {
            LevelMaterial::Multi2(m) => m.name.as_str(),
            LevelMaterial::Effect(e) => e.name.as_str(),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "Ground",
            "Lava_Gloss",
            "grass_HIRES",
            "cave_painting",
            "Sky_morning"
        ]
    );
    // One segment a technique, in order: the unskinned fixture's six
    // vertices with normals, the grass's three without, none skinned.
    let shape: Vec<(u16, usize, usize)> = g
        .segments
        .iter()
        .map(|s| (s.material, s.positions.len(), s.normals.len()))
        .collect();
    assert_eq!(
        shape,
        [(0, 6, 6), (1, 6, 6), (2, 3, 0), (3, 6, 6), (4, 6, 6)]
    );
    assert!(
        g.segments
            .iter()
            .all(|s| s.joints.is_empty() && s.weights.is_empty())
    );
    assert_eq!(g.segments[2].colors.as_ref().map(Vec::len), Some(3));
    // The sky's group holds the last segment, by the group's name.
    assert_eq!(g.sky.len(), 1);
    assert_eq!(g.sky[0].segments, [4]);
    assert!(
        g.sky[0].name.ends_with("pPlane3_Userdata"),
        "{}",
        g.sky[0].name
    );

    // The gloss map is taken: layer 3's sampler holds it.
    let LevelMaterial::Multi2(gloss) = &g.materials[1] else {
        panic!("tfbmulti2");
    };
    assert_eq!(gloss.pixel.base, 0x9C);
    assert_eq!(
        gloss.texture(TextureRole::Layer3ColorMap).unwrap().image,
        "lava_gloss"
    );
    // The grass keeps its three textures by unit, its states and its
    // shader's constants by name.
    let LevelMaterial::Effect(grass) = &g.materials[2] else {
        panic!("another effect");
    };
    assert_eq!(grass.effect, 0xAA6);
    let units: Vec<(i16, &str)> = grass
        .textures
        .iter()
        .map(|t| (t.unit, t.image.as_str()))
        .collect();
    assert_eq!(
        units,
        [(1, "grass_hieght"), (2, "grass_random"), (0, "eruptor_c")]
    );
    assert_eq!(grass.pixel.map(|m| (m.base, m.ignore)), Some((0x50, 0x50)));
    assert_eq!(
        (
            grass.states.blend,
            grass.states.depth_write,
            grass.states.alpha_test
        ),
        (Blend::Alpha, false, Some(0.023529))
    );
    assert_eq!(grass.unread, ["igShaderParametersAttr"]);
    assert_eq!(grass.scrolls.len(), 1);
    assert_eq!(
        (grass.scrolls[0].scale, grass.scrolls[0].rotation_phase),
        ([0.25; 2], 0.25)
    );

    // The cave painting multiplies what's there; the sky's tint moves, its
    // packed colours unpacked as the material tint is (red low).
    let LevelMaterial::Multi2(cave) = &g.materials[3] else {
        panic!("tfbmulti2");
    };
    assert_eq!(
        (cave.states.blend, cave.states.depth_write),
        (Blend::Multiply, false)
    );
    let LevelMaterial::Multi2(sky) = &g.materials[4] else {
        panic!("tfbmulti2");
    };
    assert_eq!(
        sky.tracks,
        [guac_ir::model::MaterialTrack::Tint {
            style: 1,
            rate: 1.0,
            frames: vec![0, 2000],
            colors: vec![[0x6E, 0x6D, 0x70, 0xFF], [0x6E, 0x6D, 0x70, 0x96]],
        }]
    );

    // Eight images, each its own file: the colour map the grass and the
    // ground share is one.
    assert_eq!(r.textures.len(), 8);
    let mut paths: Vec<&str> = r.textures.iter().map(|(p, _)| p.as_str()).collect();
    paths.sort_unstable();
    paths.dedup();
    assert_eq!(paths.len(), 8);
    let LevelMaterial::Multi2(ground) = &g.materials[0] else {
        panic!("tfbmulti2");
    };
    assert_eq!(
        ground.texture(TextureRole::Layer1ColorMap).unwrap().texture,
        grass.textures[2].texture
    );
    for (path, ktx2) in &r.textures {
        assert_eq!(*path, models::texture_path(ktx2));
    }
}

/// AE.11's glow beside AG.15's tracks and multiply on one record (IR v4,
/// the merge of both branches' v3): a level's tfbmulti2 reads its glow
/// attribute as a character's does.
#[test]
fn a_level_material_carries_its_glow_beside_its_tracks() {
    let mut spec = stage_sample();
    spec.scene[4].attrs.push(AttrSpec::Glow(64, true));
    spec.scene[3].attrs.push(AttrSpec::Glow(0, false));
    let r = scene(parsed(&spec), LEVEL, identity()).unwrap();
    let g = &r.ir.scenes[0];
    let LevelMaterial::Multi2(sky) = &g.materials[4] else {
        panic!("tfbmulti2");
    };
    assert_eq!(sky.params.glow, Glow::Amount(64));
    assert!(
        matches!(sky.tracks[..], [guac_ir::model::MaterialTrack::Tint { .. }]),
        "{:?}",
        sky.tracks
    );
    let LevelMaterial::Multi2(cave) = &g.materials[3] else {
        panic!("tfbmulti2");
    };
    assert_eq!(
        (cave.params.glow, cave.states.blend),
        (Glow::LayerAlpha, Blend::Multiply)
    );
    // Without the attribute, none, as Eruptor's eyes.
    let LevelMaterial::Multi2(ground) = &g.materials[0] else {
        panic!("tfbmulti2");
    };
    assert_eq!(ground.params.glow, Glow::None);
}

#[test]
fn a_level_material_asking_for_bit_6_is_refused_by_name() {
    let mut spec = stage_sample();
    spec.scene[0].pixel = Some((0x5C, 0x3FF));
    let e = scene(parsed(&spec), LEVEL, identity()).unwrap_err();
    assert!(e.contains("Ground"), "{e}");
    assert!(e.contains("layer4UsesPositions"), "{e}");
    // Bit 9 too.
    let mut spec = stage_sample();
    spec.scene[4].pixel = Some((0x200, 0x3FF));
    let e = scene(parsed(&spec), LEVEL, identity()).unwrap_err();
    assert!(
        e.contains("Sky_morning") && e.contains("receivesReflections"),
        "{e}"
    );
}

#[test]
fn props_read_static_and_skinned_and_one_that_doesnt_is_listed() {
    let igz = parsed(&stage_sample());
    let r = props(igz, LEVEL, identity()).unwrap();
    let paths: Vec<&str> = r.ir.props.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "C:/tfb/Content/Models/Objects/gourd_01.mdl",
            "C:/tfb/Content/Models/Objects/Treasure_Chest.mdl",
        ]
    );
    // The gourd: one technique, no skeleton or idle, in its model's frame.
    let gourd = &r.ir.props[0];
    assert!(gourd.skeleton.is_none() && gourd.idle.is_none());
    assert_eq!((gourd.materials.len(), gourd.segments.len()), (1, 1));
    assert!(gourd.segments[0].joints.is_empty());
    // The chest, read as a character's model is: three joints, its idle,
    // two skinned materials.
    let chest = &r.ir.props[1];
    assert_eq!(chest.skeleton.as_ref().map(|s| s.joints.len()), Some(3));
    assert_eq!(
        chest.idle.as_ref().map(|c| c.name.as_str()),
        Some("Eruptor_Idle.ma")
    );
    assert_eq!(chest.materials.len(), 2);
    assert!(
        chest
            .segments
            .iter()
            .all(|s| s.joints.len() == s.positions.len())
    );
    // The fairy gem sits under an animated transform: listed, not read.
    let [refused] = &r.ir.refused[..] else {
        panic!("{:?}", r.ir.refused);
    };
    assert_eq!(
        refused.path,
        "C:/tfb/Content/Models/Objects/fairy_upgrade_gem.mdl"
    );
    assert!(
        refused.reason.contains("sits under a transform"),
        "{}",
        refused.reason
    );
    // Its texture (the sky's image) stays out of the part; the gourd's and
    // the chest's are in, each once.
    let mut paths: Vec<&str> = r.textures.iter().map(|(p, _)| p.as_str()).collect();
    let n = paths.len();
    paths.sort_unstable();
    paths.dedup();
    assert_eq!(paths.len(), n);
    let sky = scene(igz, LEVEL, identity()).unwrap();
    let LevelMaterial::Multi2(sky) = &sky.ir.scenes[0].materials[4] else {
        panic!("tfbmulti2");
    };
    let sky_texture = &sky.textures[0].texture;
    assert!(!paths.contains(&sky_texture.as_str()));
    let LevelMaterial::Multi2(g) = &gourd.materials[0] else {
        panic!("tfbmulti2");
    };
    assert!(paths.contains(&g.textures[0].texture.as_str()));
}

#[test]
fn collision_placements_and_lights_read_as_stored() {
    let igz = parsed(&stage_sample());
    let c = collision(igz, LEVEL, identity()).unwrap().ir;
    let [mesh] = &c.meshes[..] else {
        panic!("{} meshes", c.meshes.len());
    };
    assert_eq!(mesh.world, STAGE_WORLD);
    // The ramp at the body's translation, its triangles and materials,
    // the welding words dropped.
    let t = [-425.5f32, -37.625, -16.0];
    let local = [
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [2.0, 1.0, 2.0],
        [0.0, 1.0, 2.0],
    ];
    let world: Vec<[f32; 3]> = local
        .iter()
        .map(|v: &[f32; 3]| [v[0] + t[0], v[1] + t[1], v[2] + t[2]])
        .collect();
    assert_eq!(mesh.vertices, world);
    assert_eq!(mesh.triangles, [[0, 2, 1], [0, 3, 2]]);
    assert_eq!(mesh.materials, [0, 40]);
    assert_eq!(ramp().materials, mesh.materials);

    let p = placements(igz, LEVEL, identity()).unwrap().ir;
    assert_eq!(p.actors.len(), 4);
    let spawn = &p.actors[0];
    assert_eq!(spawn.name, "213_Init_Location");
    assert_eq!((spawn.id, spawn.activity_state), (7, 4));
    assert_eq!(spawn.position, [10.191352, 43.37944, -93.393616]);
    assert_eq!(spawn.scale, [150.0, 125.0, 100.0]);
    assert_eq!(
        spawn.script.as_deref(),
        Some("Levels/Synthetic/Scripts/InitializeLevel.ai")
    );
    let solid = spawn.solid.unwrap();
    assert_eq!(
        (solid.shape, solid.solid, solid.world),
        (guac_ir::actor::Shape::Sphere, true, true)
    );
    assert_eq!(
        (solid.width, solid.height, solid.length, solid.weight),
        (4.0, 6.0, 3.0, 10.0)
    );
    let m = spawn.movement.unwrap();
    assert_eq!((m.ground_max_speed, m.ground_turn_rate), (18.0, 360.0));
    assert!(m.tilts_with_ground);
    let list = &p.waypoints[spawn.waypoints.unwrap() as usize];
    let names: Vec<&str> = list.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["wp0", "wp1"]);
    assert_eq!(list[1].rotation, [0.0, 45.0, 0.0]);
    assert_eq!(list[1].height, 1.0);
    // Each prop's actor names its model's path, as the props part keys it.
    let models: Vec<Option<&str>> = p.actors.iter().map(|a| a.model.as_deref()).collect();
    assert_eq!(
        models,
        [
            None,
            Some("C:/tfb/Content/Models/Objects/gourd_01.mdl"),
            Some("C:/tfb/Content/Models/Objects/Treasure_Chest.mdl"),
            Some("C:/tfb/Content/Models/Objects/fairy_upgrade_gem.mdl"),
        ]
    );
    let states: Vec<u32> = p.actors.iter().map(|a| a.activity_state).collect();
    assert_eq!(states, [4, 4, 5, 4]);

    let l = lights(igz, LEVEL, identity()).unwrap().ir;
    let names: Vec<&str> = l.directional.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["_worldLight1", "_worldLight2"]);
    assert_eq!(l.directional[1].rotation, [45.0, 30.0, 0.0]);
    let [placed] = &l.placed[..] else {
        panic!("{:?}", l.placed);
    };
    assert_eq!(
        (placed.name.as_str(), placed.light_type, placed.visible),
        ("213_EvilRobot_Beam_light", 1, true)
    );
    assert_eq!(placed.attenuation, [1.0, 0.5, 0.125]);
    assert_eq!(
        (placed.falloff_rate, placed.cutoff_angle, placed.multiplier),
        (0.25, 30.0, 1.5)
    );
    assert_eq!(placed.diffuse, [0x55, 0x2A, 0x11, 0xFF]);
}

/// The atmosphere part is the definition's zones as the format's reader
/// gives them, its start the zone set up; a level without a definition
/// script fails this part alone.
#[test]
fn the_atmosphere_is_the_definitions_zones() {
    use guac_formats::atmosphere::builder::{SCRIPT, ZoneCall};
    let igz = parsed(&stage_sample());
    let r = atmosphere(igz, LEVEL, identity()).unwrap();
    let a = &r.ir;
    assert_eq!((a.level.as_str(), a.identity.clone()), (LEVEL, identity()));
    assert_eq!(a.script, SCRIPT);
    assert!(r.textures.is_empty());
    assert_eq!(a.zones.iter().map(|z| z.id).collect::<Vec<_>>(), [0, 1]);
    assert_eq!(a.start, Some(0));
    let call = ZoneCall::sample(0, 1);
    let z = a.start_zone().unwrap();
    assert_eq!(z.ambient, call.ambient.to_le_bytes());
    let (diffuse, specular, facing, tilt) = call.lights[2];
    assert_eq!(
        z.lights[2],
        ZoneLight {
            diffuse: diffuse.to_le_bytes(),
            specular: specular.to_le_bytes(),
            facing,
            tilt,
        }
    );
    assert_eq!(
        z.fog,
        ZoneFog {
            on: call.fog_on,
            start: call.start,
            range: call.range,
            colour: call.colour.to_le_bytes(),
            sky_dome_percent: call.sky_dome_percent,
            sky_dome_colour: call.sky_dome_colour.to_le_bytes(),
            yon_colour: call.colour.to_le_bytes(),
        }
    );

    let mut bare = stage_sample();
    bare.atmosphere = None;
    let igz = parsed(&bare);
    let e = atmosphere(igz, LEVEL, identity()).unwrap_err();
    assert_eq!(
        e,
        "atmosphere: tfbScriptInfo at offset 0x0: required but missing"
    );
}

#[test]
fn a_scene_no_world_names_is_refused() {
    // AG.3's sample: a scene, but no physics world naming it.
    let bytes: &'static [u8] = Vec::leak(guac_formats::level::builder::sample().bytes);
    let igz = Igz::parse(bytes).unwrap();
    let e = scene(&igz, LEVEL, identity()).unwrap_err();
    assert!(e.contains("no physics world names it"), "{e}");
}

#[test]
fn every_level_part_is_named_and_complete_wants_them_all() {
    assert_eq!(
        PARTS,
        [
            "scene",
            "props",
            "collision",
            "placements",
            "lights",
            "atmosphere",
            "script",
            "palettes"
        ]
    );
    let mut m: crate::extract::Manifest = serde_json::from_value(serde_json::json!({
        "extractor": "0", "layout": 4, "ir_version": 3,
        "identity": identity(), "source_dir": "/d", "complete": true,
        "sources": [], "outputs": []
    }))
    .unwrap();
    assert!(
        m.levels.is_empty(),
        "an older manifest reads with no levels"
    );
    assert!(!complete(&m));
    let entry = m.levels.entry(LEVELS[0].to_owned()).or_default();
    for part in &PARTS[..4] {
        entry.parts.insert((*part).to_owned(), Vec::new());
    }
    assert!(!complete(&m), "lights missing");
    m.levels
        .get_mut(LEVELS[0])
        .unwrap()
        .parts
        .insert(LIGHTS.to_owned(), Vec::new());
    // AG.15's five parts: an extract written before AG.41 resumes for its
    // atmosphere alone.
    assert!(!complete(&m), "the atmosphere missing");
    m.levels
        .get_mut(LEVELS[0])
        .unwrap()
        .parts
        .insert(ATMOSPHERE.to_owned(), Vec::new());
    // AH.22's two: an extract written before them resumes for those alone.
    assert!(!complete(&m), "the script and the palettes missing");
    for part in [SCRIPT, PALETTES] {
        m.levels
            .get_mut(LEVELS[0])
            .unwrap()
            .parts
            .insert(part.to_owned(), Vec::new());
    }
    assert!(complete(&m));
    // The script is the entry as the disc holds it, in either format.
    for ir in [
        crate::intros::IrFormat::Ron,
        crate::intros::IrFormat::Postcard,
    ] {
        assert_eq!(
            part_path(ir, LEVELS[0], SCRIPT),
            "levels/level_213_tutotial/script.igz"
        );
        assert_eq!(
            part_path(ir, LEVELS[0], PALETTES),
            ir.level_path(LEVELS[0], PALETTES)
        );
    }
    assert_eq!(
        part_path(crate::intros::IrFormat::Ron, LEVELS[0], PALETTES),
        "levels/level_213_tutotial/palettes.ron"
    );
    assert_eq!(
        archive_path(Path::new("/u"), LEVELS[0]),
        Path::new("/u/level/level_213_tutotial.bld")
    );
}

#[test]
fn a_scene_two_worlds_name_is_refused() {
    use guac_formats::igz::builder::IgzBuilder;
    let mut b = IgzBuilder::new();
    let worlds = [(); 2].map(|_| b.object(0, "tfbPhysicsWorld", 0x2C));
    let scene = b.object(0, "igSceneInfo", 0x48);
    for (k, w) in worlds.into_iter().enumerate() {
        b.string(w.offset_by(0x08).unwrap(), &format!("w{k}.ma"));
        b.null_memory_ref(w.offset_by(0x14).unwrap(), 0);
        b.pointer(w.offset_by(0x20).unwrap(), scene);
    }
    let bytes: &'static [u8] = Vec::leak(b.build());
    let igz = Igz::parse(bytes).unwrap();
    let e = super::scene(&igz, LEVEL, identity()).unwrap_err();
    assert!(e.contains("named by two physics worlds"), "{e}");
}

#[test]
fn the_script_part_holds_the_levels_scripts_once_its_graph_reads() {
    // The spawn's, the chest's and the level definition's.
    assert_eq!(script(parsed(&stage_sample())), Ok(3));
    // AG.3's sample names a script with no op list: AH.13's graph refuses
    // it, so the entry isn't written.
    let bytes: &'static [u8] = Vec::leak(guac_formats::level::builder::sample().bytes);
    let e = script(&Igz::parse(bytes).unwrap()).unwrap_err();
    assert!(e.contains("_opList"), "{e}");
}

/// The stage sample with the chest's set holding a partial clip, an
/// additive one and a network beside its idle, and its placement running
/// `script`.
fn creature_level(script: Option<&'static str>) -> StageSpec {
    use guac_formats::havok::builder::clip;
    use guac_formats::model::builder::SlotSpec;
    let mut spec = stage_sample();
    spec.skinned_script = script;
    let (_, chest) = spec.skinned.as_mut().unwrap();
    let mut partial = clip();
    partial.tracks.truncate(1);
    partial.track_to_bone = vec![1];
    let mut additive = clip();
    additive.blend_hint = 1;
    chest.slots = vec![
        SlotSpec {
            clip: Some(partial),
            properties: 0x620,
            ..SlotSpec::new(20, 1.0 / 30.0, "Chest_Bite.ma")
        },
        SlotSpec {
            clip: Some(additive),
            properties: 0x611,
            ..SlotSpec::new(11, 1.0 / 30.0, "Chest_Lean.ma")
        },
        SlotSpec {
            clip: None,
            ..SlotSpec::new(6, 1.0 / 30.0, "Chest_Network.ma")
        },
    ];
    // A slot that can't play: the wrapper says additive, the binding not.
    let mut mismatched = clip();
    mismatched.blend_hint = 0;
    chest.slots.push(SlotSpec {
        clip: Some(mismatched),
        properties: 0x610,
        ..SlotSpec::new(25, 1.0 / 30.0, "Chest_Odd.ma")
    });
    spec
}

#[test]
fn the_palettes_follow_a_whitelisted_script_to_its_creature() {
    use guac_ir::palette::SlotEntry;
    const CHEST: &str = "C:/tfb/Content/Models/Objects/Treasure_Chest.mdl";
    // The chest runs the Chompy's script (by its file name, any case): its
    // palette is in, the gourd and the gem have no skeleton and none.
    let spec = creature_level(Some("Levels/Elsewhere/s2_enemy_CHOMPY.AI"));
    let r = palettes(parsed(&spec), LEVEL, identity()).unwrap();
    assert_eq!(
        (r.ir.level.as_str(), r.ir.identity.clone()),
        (LEVEL, identity())
    );
    assert!(r.textures.is_empty());
    let [p] = &r.ir.palettes[..] else {
        panic!("{} palettes", r.ir.palettes.len());
    };
    assert_eq!((p.owner.as_str(), p.slots), (CHEST, 26));
    let seen: Vec<(u16, String)> = p
        .entries
        .iter()
        .map(|e| {
            let what = match &e.entry {
                SlotEntry::Network => "network".to_owned(),
                SlotEntry::Clip {
                    additive,
                    partial,
                    layered,
                    motion_extraction,
                    clip,
                } => format!(
                    "{} {additive} {partial} {layered} {motion_extraction} {:?}",
                    clip.name,
                    clip.tracks.iter().map(|t| t.joint).collect::<Vec<_>>()
                ),
            };
            (e.slot, what)
        })
        .collect();
    assert_eq!(
        seen,
        [
            (
                0,
                "Eruptor_Idle.ma false false false 0 [0, 1, 2]".to_owned()
            ),
            (6, "network".to_owned()),
            (11, "Chest_Lean.ma true false false 1 [0, 1, 2]".to_owned()),
            (20, "Chest_Bite.ma false true false 0 [1]".to_owned()),
        ]
    );
    let [refused] = &p.refused[..] else {
        panic!("{:?}", p.refused);
    };
    assert_eq!(refused.slot, 25);
    assert!(refused.reason.contains("blendHint"), "{}", refused.reason);
    // Off the whitelist, or no script at all: no palette.
    for script in [Some("Levels/Synthetic/Scripts/Chest.ai"), None] {
        let r = palettes(parsed(&creature_level(script)), LEVEL, identity()).unwrap();
        assert!(r.ir.palettes.is_empty(), "{script:?}");
    }
    // A level off the whitelist runs none of its scripts.
    let r = palettes(parsed(&spec), "level_999_elsewhere", identity()).unwrap();
    assert!(r.ir.palettes.is_empty());
}
