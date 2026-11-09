//! The level parts on the synthetic level `guac_formats`' builder makes
//! (AG.15): level 213's shapes in small, every value known.

use guac_formats::collision::builder::ramp;
use guac_formats::igz::Igz;
use guac_formats::level::builder::{
    STAGE_WORLD, StageSpec, colour_only, stage_level, stage_sample,
};
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

/// A level material asking for bit 6 (layer 4 by positions, six levels'
/// water) reads with the bit kept (openspec AS.5); one asking for bit 9 is
/// refused by name.
#[test]
fn a_level_material_keeps_bit_6_and_bit_9_is_refused_by_name() {
    let mut spec = stage_sample();
    spec.scene[0].pixel = Some((0x5C, 0x3FF));
    let r = scene(parsed(&spec), LEVEL, identity()).unwrap();
    let LevelMaterial::Multi2(ground) = &r.ir.scenes[0].materials[0] else {
        panic!("tfbmulti2");
    };
    assert_eq!(ground.pixel.base, 0x5C);
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
    // The builder gives every placement a 160 ft range (AG.25 keeps it).
    let ranges: Vec<f32> = p.actors.iter().map(|a| a.activation_range).collect();
    assert_eq!(ranges, [160.0; 4]);

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
/// gives them, its start the zone set up; a level without one has none.
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

    // A level no script adds a zone to (an arena challenge, a ship part)
    // sets none up: no zones, no definition (openspec AS.6).
    let mut bare = stage_sample();
    bare.atmosphere = None;
    let igz = parsed(&bare);
    let a = atmosphere(igz, LEVEL, identity()).unwrap().ir;
    assert_eq!((a.script.as_str(), a.zones.len(), a.start), ("", 0, None));
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
            "palettes",
            "nodes",
            "particles",
            "sprites"
        ]
    );
    // AT.26: the TV reads every part but the sprites, so a part added to
    // the list is one the hand-off waits for until it says otherwise.
    assert_eq!(PLAYED[..], PARTS[..PARTS.len() - 1]);
    assert_eq!(PARTS[PARTS.len() - 1], SPRITES);
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
    let file = |part: &str| vec![part_path(crate::intros::IrFormat::Ron, LEVELS[0], part)];
    let entry = m.levels.entry(LEVELS[0].to_owned()).or_default();
    for part in &PARTS[..4] {
        entry.parts.insert((*part).to_owned(), file(part));
    }
    assert!(!complete(&m), "lights missing");
    m.levels
        .get_mut(LEVELS[0])
        .unwrap()
        .parts
        .insert(LIGHTS.to_owned(), file(LIGHTS));
    // AG.15's five parts: an extract written before AG.41 resumes for its
    // atmosphere alone.
    assert!(!complete(&m), "the atmosphere missing");
    m.levels
        .get_mut(LEVELS[0])
        .unwrap()
        .parts
        .insert(ATMOSPHERE.to_owned(), file(ATMOSPHERE));
    // AH.22's two: an extract written before them resumes for those alone.
    assert!(!complete(&m), "the script and the palettes missing");
    for part in [SCRIPT, PALETTES] {
        m.levels
            .get_mut(LEVELS[0])
            .unwrap()
            .parts
            .insert(part.to_owned(), file(part));
    }
    // AH.138's nodes: an extract written before them resumes for those alone.
    assert!(!complete(&m), "the nodes missing");
    m.levels
        .get_mut(LEVELS[0])
        .unwrap()
        .parts
        .insert(NODES.to_owned(), file(NODES));
    // AT.8's sprites: an extract written before them resumes for those alone.
    assert!(!complete(&m), "the sprites missing");
    m.levels
        .get_mut(LEVELS[0])
        .unwrap()
        .parts
        .insert(SPRITES.to_owned(), file(SPRITES));
    // AS.17's particles: an extract written before them resumes for those
    // alone.
    assert!(!complete(&m), "the particles missing");
    m.levels
        .get_mut(LEVELS[0])
        .unwrap()
        .parts
        .insert(PARTICLES.to_owned(), file(PARTICLES));
    assert!(complete(&m));
    // AS.52: a part recorded under an older name than its rule's (chapter
    // 1's palettes as `palettes.ron`, before `palettes.v2`) is a part to
    // read again; its file in the other IR format holds it too.
    let palettes = &mut m.levels.get_mut(LEVELS[0]).unwrap().parts;
    palettes.insert(
        PALETTES.to_owned(),
        vec![crate::intros::IrFormat::Ron.level_path(LEVELS[0], PALETTES)],
    );
    assert!(!complete(&m), "the palettes under their old name");
    m.levels.get_mut(LEVELS[0]).unwrap().parts.insert(
        PALETTES.to_owned(),
        vec![part_path(
            crate::intros::IrFormat::Postcard,
            LEVELS[0],
            PALETTES,
        )],
    );
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
            ir.level_path(LEVELS[0], "palettes.v5")
        );
    }
    // Named for its rule since AS.52 (its outro's actors), v3 since AS.78
    // (its camera rigs), v4 since AU.33 (every skinned model it places), v5
    // since AS.180 (a random network's clips), so an extract before it reads
    // it again.
    assert_eq!(
        part_path(crate::intros::IrFormat::Ron, LEVELS[0], PALETTES),
        "levels/level_213_tutotial/palettes.v5.ron"
    );
    // A v4 file, AU.33's, is a part to read again.
    m.levels.get_mut(LEVELS[0]).unwrap().parts.insert(
        PALETTES.to_owned(),
        vec![crate::intros::IrFormat::Ron.level_path(LEVELS[0], "palettes.v4")],
    );
    assert!(!complete(&m), "the palettes under AU.33's name");
    // A v2 file, the rule before AS.78's, is a part to read again too.
    m.levels.get_mut(LEVELS[0]).unwrap().parts.insert(
        PALETTES.to_owned(),
        vec![crate::intros::IrFormat::Ron.level_path(LEVELS[0], "palettes.v2")],
    );
    assert!(!complete(&m), "the palettes under AS.52's name");
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
fn the_palettes_read_every_skinned_model_a_placement_names() {
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
                SlotEntry::Random { choices } => format!("random of {}", choices.len()),
                SlotEntry::Blocks { clip, .. } => format!("blocks {}", clip.name),
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
    // AG.60 lists the chest's own script: its palette is in by it too.
    let r = palettes(
        parsed(&creature_level(Some("Levels/Synthetic/Scripts/Chest.ai"))),
        LEVEL,
        identity(),
    )
    .unwrap();
    assert_eq!(
        r.ir.palettes
            .iter()
            .map(|p| p.owner.as_str())
            .collect::<Vec<_>>(),
        [CHEST],
        "a newly listed script's creature"
    );
    // AG.177 lists the jumping goats' script: a creature running it is
    // followed to its palette too (the goats' clip lengths on the TV).
    let r = palettes(
        parsed(&creature_level(Some(
            "C:/tfb/Content/Levels/Level_213/Scripts/Goat_Jumping_Pit.ai",
        ))),
        LEVEL,
        identity(),
    )
    .unwrap();
    assert_eq!(
        r.ir.palettes
            .iter()
            .map(|p| p.owner.as_str())
            .collect::<Vec<_>>(),
        [CHEST],
        "the jumping goats' creature"
    );
    // AG.162 lists the walkers' scripts: a creature running one (a portal
    // entry, a definition, by file name any case) is followed to its palette.
    for script in [
        "Levels/Synthetic/Scripts/PortalEntry_Air.ai",
        "C:/tfb/Minions/AirDragon/scripts/000_airdragon_DEF.AI",
    ] {
        let r = palettes(parsed(&creature_level(Some(script))), LEVEL, identity()).unwrap();
        assert_eq!(
            r.ir.palettes
                .iter()
                .map(|p| p.owner.as_str())
                .collect::<Vec<_>>(),
            [CHEST],
            "{script}"
        );
    }
    // On every level (openspec AS.54; chapter 1's since AU.33), every skinned
    // model a placement names, its script listed, off the whitelist or none
    // (a template a spawner clones): the TV runs each level's own scripts.
    for (level, script) in [
        (
            "level_999_elsewhere",
            Some("Levels/Synthetic/Scripts/Chest_Unlisted.ai"),
        ),
        (
            "level_999_elsewhere",
            Some("Levels/Synthetic/Scripts/PortalEntry_Air.ai"),
        ),
        ("level_999_elsewhere", None),
        (LEVEL, Some("Levels/Synthetic/Scripts/Chest_Unlisted.ai")),
        (LEVEL, None),
    ] {
        let r = palettes(parsed(&creature_level(script)), level, identity()).unwrap();
        assert_eq!(
            r.ir.palettes
                .iter()
                .map(|p| p.owner.as_str())
                .collect::<Vec<_>>(),
            [CHEST],
            "{level} {script:?}"
        );
    }
    // Its file named for its rule there, chapter 1's for its own.
    assert_eq!(
        part_path(
            crate::intros::IrFormat::Ron,
            "level_999_elsewhere",
            PALETTES
        ),
        "levels/level_999_elsewhere/palettes.v4.ron"
    );
    assert_eq!(
        part_path(crate::intros::IrFormat::Postcard, LEVEL, PALETTES),
        format!("levels/{LEVEL}/palettes.v{PALETTES_FIRST_RUN}.postcard")
    );
}

/// AS.78: on a first run's level a camera rig's palette is read with no
/// script of its own (the cinematics' animator cameras are unscripted
/// templates), its clip past one Havok block kept block by block, its
/// length the whole clip's and its pose each block's at the time within
/// it, as the decoder samples it.
#[test]
fn a_first_runs_camera_rig_has_its_palette_its_long_shot_in_blocks() {
    use guac_formats::havok::builder::blocked_clip;
    use guac_formats::model::builder::SlotSpec;
    use guac_ir::palette::SlotEntry;
    const CAMERA: &str = "C:/tfb/Content/Models/Cameras/SKY2_cinematics/999_Outro.mdl";
    assert!(is_camera_model(CAMERA) && is_camera_model("c:\\x\\models\\CAMERAS\\a.mdl"));
    assert!(!is_camera_model(
        "C:/tfb/Content/Models/Objects/Treasure_Chest.mdl"
    ));
    let mut spec = creature_level(None);
    let (path, chest) = spec.skinned.as_mut().unwrap();
    *path = CAMERA;
    chest.slots.push(SlotSpec {
        clip: Some(blocked_clip()),
        ..SlotSpec::new(3, 1.0 / 30.0, "999_Outro_Cam1.ma")
    });
    let r = palettes(parsed(&spec), LEVEL, identity()).unwrap();
    let [p] = &r.ir.palettes[..] else {
        panic!(
            "{:?}",
            r.ir.palettes.iter().map(|p| &p.owner).collect::<Vec<_>>()
        );
    };
    assert_eq!(p.owner, CAMERA);
    let Some(SlotEntry::Blocks { clip, .. }) = p.get(3).map(|e| &e.entry) else {
        panic!("{:?}", p.get(3));
    };
    assert_eq!(
        (
            clip.frames,
            clip.blocks.len(),
            clip.blocks.iter().map(|b| b.frames).collect::<Vec<_>>()
        ),
        (8, 2, vec![5, 4])
    );
    let whole = 7.0 / 30.0;
    assert!((p.duration(3).unwrap() - whole).abs() < 1e-6);
    assert!(p.clip(3).is_none(), "no one-block clip in it");
    // Each block's pose, at the time within it, the decoder's.
    let built = guac_formats::havok::builder::payload(&guac_formats::havok::builder::PayloadSpec {
        clip: Some(blocked_clip()),
        ..Default::default()
    });
    let decoded = guac_formats::havok::Container::parse(&built.bytes).unwrap();
    let a = &decoded.clip.as_ref().unwrap().animation;
    for frame in [0.0, 2.0, 3.9, 4.5, 6.0, 6.9] {
        let t = frame / 30.0;
        let (block, local) = p.clip_at(3, t).unwrap();
        let ours = block.sample(local);
        let theirs = a.sample(t);
        for ((joint, q), d) in ours.iter().zip(&theirs) {
            let gap = (0..3)
                .map(|i| (f64::from(q.translation[i]) - d.translation[i]).abs())
                .fold(0.0, f64::max);
            assert!(gap < 1e-5, "frame {frame}, joint {joint}: {gap:e}");
        }
    }
}

/// AS.52: on a first run's level the outro's actors' palettes are read,
/// by their scripts on the whitelist.
#[test]
fn the_outros_actors_have_their_palettes() {
    const CHEST: &str = "C:/tfb/Content/Models/Objects/Treasure_Chest.mdl";
    for script in [
        "C:/tfb/Content/Levels/Level_213/cutscenes/scripts/213_Mabu_Generic.ai",
        "C:/tfb/Content/Levels/Level_213/cutscenes/scripts/213_Norticus_NPC.ai",
    ] {
        let r = palettes(parsed(&creature_level(Some(script))), LEVEL, identity()).unwrap();
        assert_eq!(
            r.ir.palettes
                .iter()
                .map(|p| p.owner.as_str())
                .collect::<Vec<_>>(),
            [CHEST],
            "{script}"
        );
    }
}

/// The nodes part (openspec AH.138): the skinned chest's attach list on its
/// Havok joints by name (`J_head` joint 2, `J_spine` 1, `*root*` and a bone
/// it lacks on none), its offsets as stored; the gourd's on no joint, a
/// static model having none; the models without a list left out.
#[test]
fn the_nodes_part_puts_a_skinned_models_nodes_on_its_joints() {
    use guac_formats::attachments::builder::PointSpec;
    const CHEST: &str = "C:/tfb/Content/Models/Objects/Treasure_Chest.mdl";
    const GOURD: &str = "C:/tfb/Content/Models/Objects/gourd_01.mdl";
    let mut spec = stage_sample();
    spec.skinned_points = vec![
        PointSpec::bone("J_spine", 1),
        PointSpec::root(0.5),
        PointSpec {
            bone: "J_head",
            index: 2,
            transform: [0.0, 0.25, 0.0, 0.0, 0.0, 0.0],
        },
        PointSpec::bone("J_tail", 7),
    ];
    spec.prop_points = vec![PointSpec::bone("J_spine", 1)];
    let r = nodes(parsed(&spec), LEVEL, identity()).unwrap();
    assert_eq!(
        (r.ir.level.as_str(), r.ir.identity.clone()),
        (LEVEL, identity())
    );
    assert!(r.textures.is_empty() && r.ir.refused.is_empty());
    let paths: Vec<&str> = r.ir.models.iter().map(|m| m.load_file.as_str()).collect();
    assert_eq!(paths, [GOURD, CHEST]);
    let chest = r.ir.get(CHEST).unwrap();
    let joints: Vec<Option<u16>> = chest.nodes.iter().map(|n| n.joint).collect();
    assert_eq!(joints, [Some(1), None, Some(2), None]);
    let bones: Vec<Option<&str>> = chest.nodes.iter().map(|n| n.bone.as_deref()).collect();
    assert_eq!(
        bones,
        [
            Some("J_spine"),
            Some("*root*"),
            Some("J_head"),
            Some("J_tail")
        ]
    );
    assert_eq!(chest.nodes[1].offset.translation, [0.0, 0.0, 0.5]);
    assert_eq!(chest.nodes[2].offset.translation, [0.0, 0.25, 0.0]);
    let gourd = r.ir.get(GOURD).unwrap();
    assert_eq!(gourd.nodes.len(), 1);
    assert_eq!(gourd.nodes[0].joint, None, "a static model has no joints");
    // With no lists, none.
    let r = nodes(parsed(&stage_sample()), LEVEL, identity()).unwrap();
    assert!(r.ir.models.is_empty());
}

/// One triangle facing +z (counter-clockwise from the front), its normal
/// and a tangent along +x, for [`bake`]'s closed forms.
fn triangle() -> Segment {
    Segment {
        material: 0,
        source: 0,
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        tangents: Some(vec![[1.0, 0.0, 0.0, 1.0]; 3]),
        uvs: vec![vec![[0.0; 2]; 3]],
        colors: None,
        joints: Vec::new(),
        weights: Vec::new(),
        indices: vec![0, 1, 2],
    }
}

fn row_affine(rows: [[f32; 3]; 3], at: [f32; 3]) -> Mat44 {
    let r = |i: usize| [rows[i][0], rows[i][1], rows[i][2], 0.0];
    [r(0), r(1), r(2), [at[0], at[1], at[2], 1.0]]
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
}

/// The face a triangle's winding makes, from its positions: (b - a) × (c - a).
fn face(s: &Segment) -> [f32; 3] {
    let [a, b, c] = [0, 1, 2].map(|k| s.positions[usize::from(s.indices[k])]);
    let (u, v) = (
        [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
        [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
    );
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

/// AS.3: a translation moves the positions alone; a quarter turn about y
/// turns positions, normals and tangents alike; a stretch along x leans the
/// normals by its inverse transpose, unit length; a mirror flips the
/// tangents' handedness and swaps each triangle's winding so its front
/// still faces along its normal; a flat matrix is refused.
#[test]
fn a_draw_under_a_transform_is_baked_into_the_levels_frame() {
    let mut s = triangle();
    bake(
        &mut s,
        &row_affine(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [10.0, 20.0, 30.0],
        ),
    )
    .unwrap();
    assert_eq!(s.positions[1], [11.0, 20.0, 30.0]);
    assert_eq!(s.normals[0], [0.0, 0.0, 1.0]);
    assert_eq!(s.indices, [0, 1, 2]);

    // x to -z, z to x: the triangle's +z face turns to +x.
    let mut s = triangle();
    bake(&mut s, &guac_formats::level::builder::QUARTER_TURN).unwrap();
    assert!(
        close(s.positions[1], [0.0, 0.0, -1.0]),
        "{:?}",
        s.positions[1]
    );
    assert!(close(s.normals[0], [1.0, 0.0, 0.0]), "{:?}", s.normals[0]);
    let t = s.tangents.as_ref().unwrap()[0];
    assert!(
        close([t[0], t[1], t[2]], [0.0, 0.0, -1.0]) && t[3] == 1.0,
        "{t:?}"
    );
    assert_eq!(s.indices, [0, 1, 2]);

    // A stretch by 2 along x: a normal (1, 0, 1)/√2 leans to (1, 0, 2)/√5.
    let mut s = triangle();
    let h = std::f32::consts::FRAC_1_SQRT_2;
    s.normals = vec![[h, 0.0, h]; 3];
    bake(
        &mut s,
        &row_affine(
            [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.0; 3],
        ),
    )
    .unwrap();
    let r5 = 5.0f32.sqrt();
    assert!(
        close(s.normals[0], [1.0 / r5, 0.0, 2.0 / r5]),
        "{:?}",
        s.normals[0]
    );
    assert!(close(s.positions[1], [2.0, 0.0, 0.0]));

    // A mirror through x = 0: the front still faces along the normal.
    let mut s = triangle();
    bake(
        &mut s,
        &row_affine(
            [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.0; 3],
        ),
    )
    .unwrap();
    assert_eq!(s.positions[1], [-1.0, 0.0, 0.0]);
    assert_eq!(s.indices, [0, 2, 1]);
    let (f, n) = (face(&s), s.normals[0]);
    assert!(
        f[0] * n[0] + f[1] * n[1] + f[2] * n[2] > 0.0,
        "{f:?} against {n:?}"
    );
    let t = s.tangents.as_ref().unwrap()[0];
    assert_eq!((t[0], t[3]), (-1.0, -1.0));

    // Flat: nothing to invert.
    let mut s = triangle();
    let flat = row_affine(
        [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        [0.0; 3],
    );
    assert!(bake(&mut s, &flat).is_err());
    assert_eq!(
        s.positions,
        triangle().positions,
        "a refused bake moves nothing"
    );
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

/// `v` through `m`'s 3 × 3 part, a direction under a row-vector affine.
fn through(v: [f32; 3], m: &Mat44) -> [f32; 3] {
    std::array::from_fn(|c| v[0] * m[0][c] + v[1] * m[1][c] + v[2] * m[2][c])
}

/// AS.3 off the axes: a sheared, unevenly scaled and turned matrix, the
/// same with two rows swapped (a mirror no axis lines up with) and a half
/// turn (two axes negated, no mirror), each on a triangle whose normal and
/// tangent aren't unit length and whose tangent lies off the axes. The
/// normal stays square to the moved surface; the tangent runs along the
/// matrix's 3 × 3, not its inverse transpose, which only a shear or an
/// uneven scale tells apart; each keeps its length; the winding's face
/// points along the normal and the bitangent the handedness makes along the
/// moved one; the winding and the handedness flip exactly where the
/// determinant is negative.
#[test]
fn a_bake_off_the_axes_keeps_the_surface_its_lengths_and_its_front() {
    let general = row_affine(
        [[2.0, 0.5, 0.0], [0.3, 1.0, 0.2], [0.0, -0.4, 3.0]],
        [1.0, 2.0, 3.0],
    );
    let mut mirrored = general;
    mirrored.swap(0, 1);
    let half_turn = row_affine(
        [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]],
        [0.0; 3],
    );
    let row = |m: &Mat44, r: usize| [m[r][0], m[r][1], m[r][2]];
    for (m, mirror) in [(general, false), (mirrored, true), (half_turn, false)] {
        let det = dot(cross(row(&m, 0), row(&m, 1)), row(&m, 2));
        assert_eq!(det < 0.0, mirror, "{m:?}");
        let mut s = triangle();
        let (n, t) = ([0.0, 0.0, 0.5], [0.48, 0.64, 0.0]);
        s.normals = vec![n; 3];
        s.tangents = Some(vec![[t[0], t[1], t[2], 1.0]; 3]);
        let before = s.clone();
        bake(&mut s, &m).unwrap();
        for (p, q) in before.positions.iter().zip(&s.positions) {
            assert_eq!(*q, transform_point(*p, &m));
        }
        let [a, b, c] = [0, 1, 2].map(|k| s.positions[k]);
        let edges = [
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
            [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
        ];
        let n2 = s.normals[0];
        for e in edges {
            assert!(dot(n2, e).abs() < 1e-5, "{n2:?} off the surface {e:?}");
        }
        assert!((len(n2) - 0.5).abs() < 1e-6, "{n2:?} not at its length");
        let [x, y, z, w] = s.tangents.as_ref().unwrap()[0];
        let (t2, along) = ([x, y, z], through(t, &m));
        assert!((len(t2) - 0.8).abs() < 1e-6, "{t2:?} not at its length");
        assert!(
            (dot(t2, along) - len(t2) * len(along)).abs() < 1e-5,
            "{t2:?} not along {along:?}"
        );
        assert_eq!(w, if mirror { -1.0 } else { 1.0 }, "{m:?}");
        assert!(
            dot(cross(n2, t2), through(cross(n, t), &m)) * w > 0.0,
            "the bitangent turned under {m:?}"
        );
        assert_eq!(s.indices, if mirror { [0, 2, 1] } else { [0, 1, 2] });
        assert!(dot(face(&s), n2) > 0.0, "the front turned away under {m:?}");
    }
}

/// AS.3 on a level: the scene's techniques under a still transform and
/// under a keyframed one both read, moved into the level's frame and the
/// keyframed listed; a prop under a still transform reads moved, the fairy
/// gem's keyframed one stays listed; with no transform the scene reads as
/// before, bit for bit.
#[test]
fn a_levels_transforms_are_baked_and_the_animated_listed() {
    let plain = scene(parsed(&stage_sample()), LEVEL, identity()).unwrap();
    let over = |matrix, animated| {
        let spec = StageSpec {
            scene_transform: Some((matrix, animated)),
            ..stage_sample()
        };
        let igz = parsed(&spec);
        (
            scene(igz, LEVEL, identity()).unwrap(),
            animated_draws(igz).unwrap(),
        )
    };
    let shift = guac_formats::level::builder::TRANSLATION;
    for animated in [false, true] {
        let (moved, listed) = over(shift, animated);
        let (a, b) = (&plain.ir.scenes[0], &moved.ir.scenes[0]);
        assert_eq!(a.segments.len(), b.segments.len());
        // The ground's four techniques moved by (10, 20, 30); the sky, off
        // the transform, where it was.
        for (k, (sa, sb)) in a.segments.iter().zip(&b.segments).enumerate() {
            let d = if k < 4 { [10.0, 20.0, 30.0] } else { [0.0; 3] };
            for (pa, pb) in sa.positions.iter().zip(&sb.positions) {
                assert!(
                    close([pa[0] + d[0], pa[1] + d[1], pa[2] + d[2]], *pb),
                    "{k}: {pa:?} {pb:?}"
                );
            }
            assert_eq!(sa.normals, sb.normals);
        }
        let names: Vec<&str> = listed.iter().map(|(_, n, _)| n.as_str()).collect();
        if animated {
            assert_eq!(
                names,
                ["Ground", "Lava_Gloss", "grass_HIRES", "cave_painting"]
            );
            assert!(listed.iter().all(|(_, _, identity)| !identity));
        } else {
            assert!(names.is_empty(), "{names:?}");
        }
    }
    // An identity transform keyframed: read as before, bit for bit.
    let (same, listed) = over(IDENTITY, true);
    assert_eq!(same.ir, plain.ir);
    assert_eq!(listed.len(), 4);

    // A prop under a still transform reads, moved; the gem stays listed.
    let spec = StageSpec {
        moved: Some((
            "C:/tfb/Content/Models/Objects/statue.mdl",
            colour_only("statue", 2),
            shift,
        )),
        ..stage_sample()
    };
    let p = props(parsed(&spec), LEVEL, identity()).unwrap();
    let statue =
        p.ir.props
            .iter()
            .find(|p| p.path.ends_with("statue.mdl"))
            .expect("the statue reads");
    let gourd = &p.ir.props[0];
    for (a, b) in gourd.segments[0]
        .positions
        .iter()
        .zip(&statue.segments[0].positions)
    {
        assert!(
            close([a[0] + 10.0, a[1] + 20.0, a[2] + 30.0], *b),
            "{a:?} {b:?}"
        );
    }
    let [refused] = &p.ir.refused[..] else {
        panic!("{:?}", p.ir.refused);
    };
    assert!(refused.path.ends_with("fairy_upgrade_gem.mdl"));
}
