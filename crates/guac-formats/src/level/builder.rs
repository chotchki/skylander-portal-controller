//! Synthetic levels for tests and fuzz seeds (`test-fixtures`): a scene
//! shaped as the disc's are (a root group over a spatial node, the sky
//! under a user-info group) plus two nested transforms the disc has in only
//! 10 levels, and the placements AG walks among (a spawn with its
//! waypoints, a camera volume, an emitter, a point light, the light rig).
//! Every field is a known value, written at the offsets AG.3's layouts give
//! as literals, so a wrong reader constant shows.

use crate::edge::builder::{segment, unskinned};
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;

/// Where [`sample`] put each object.
#[derive(Debug, Clone)]
pub struct Sample {
    pub bytes: Vec<u8>,
    pub scene: Location,
    /// `igGroup` "sceneGraph": the spatial node, then the outer transform.
    pub root: Location,
    /// `igSpatialNode`: technique 0, then the sky.
    pub spatial: Location,
    /// `igSimpleUserInfo` "SKYDOME": technique 1.
    pub sky: Location,
    /// `igTransform`, a translation by (10, 20, 30), over the inner one.
    pub outer: Location,
    /// `igTransform`, a quarter turn about y, keyframed: technique 2.
    pub inner: Location,
    pub keyframes: Location,
    pub techniques: [Location; 3],
    pub attrs: [Location; 3],
    pub geometries: [Location; 3],
    /// Geometry 0 holds the first two.
    pub segments: [Location; 4],
    /// "213_Init_Location": script, solid sphere, parameters, model and
    /// three waypoints.
    pub actor: Location,
    /// "Camera_Transition_36-37": a non-solid box sharing the actor's model.
    pub volume: Location,
    pub script: Location,
    /// The actor's, then the volume's.
    pub collisions: [Location; 2],
    pub parameters: Location,
    pub model: Location,
    pub waypoint_list: Location,
    pub waypoints: [Location; 3],
    pub emitter: Location,
    pub light: Location,
    pub rig: Location,
}

/// The inner transform's matrix: x goes to -z, z to x.
pub const QUARTER_TURN: [[f32; 4]; 4] = [
    [0.0, 0.0, -1.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// The outer transform's matrix.
pub const TRANSLATION: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [10.0, 20.0, 30.0, 1.0],
];

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

fn floats(b: &mut IgzBuilder, base: Location, offset: u32, values: &[f32]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    b.put_bytes(at(base, offset), &bytes);
}

/// `igNode`'s fields: name, null bound and parent list, weights -1, handle
/// -1, flags 0x10000.
fn node(b: &mut IgzBuilder, o: Location, name: &str) {
    b.string(at(o, 0x08), name);
    b.put_u32(at(o, 0x14), u32::MAX);
    b.put_u32(at(o, 0x18), u32::MAX);
    b.put_u32(at(o, 0x1C), 0x1_0000);
}

/// An `igPS3EdgeGeometry` of `segments`.
pub fn geometry(b: &mut IgzBuilder, blocks: u8, segments: &[Location]) -> Location {
    let g = b.object(0, "igPS3EdgeGeometry", 0x1C);
    let elements: Vec<_> = segments.iter().copied().map(Some).collect();
    list(b, g, blocks, &elements);
    b.put_u32(at(g, 0x18), 0xC0FF_EE00 + segments.len() as u32);
    g
}

/// A technique and its `igEdgeGeometryAttr` drawing `geometry`, their
/// fields as the layouts sample writes them.
pub fn technique(b: &mut IgzBuilder, name: &str, geometry: Location) -> (Location, Location) {
    let t = b.object(0, "tfbRuntimeTechniqueInstance", 0x38);
    let attr = b.object(0, "igEdgeGeometryAttr", 0x14);
    node(b, t, name);
    b.put_u32(at(t, 0x20), 0x11);
    b.put_u32(at(t, 0x24), 0x22);
    b.pointer(at(t, 0x28), attr);
    b.import(at(t, 0x34), "effects", "tfbmulti2");
    b.put_bytes(at(attr, 0x0A), &[0xFF, 0xFF]);
    b.pointer(at(attr, 0x10), geometry);
    (t, attr)
}

/// A group of `type_name` (`size` bytes) over `children`, with a child list
/// of its own.
pub fn group(
    b: &mut IgzBuilder,
    blocks: u8,
    (type_name, size): (&str, u32),
    name: &str,
    children: &[Location],
) -> Location {
    let g = b.object(0, type_name, size);
    let child_list = b.object(0, "igNodeList", 0x18);
    node(b, g, name);
    b.pointer(at(g, 0x20), child_list);
    let elements: Vec<_> = children.iter().copied().map(Some).collect();
    list(b, child_list, blocks, &elements);
    g
}

/// The registry's groups, as [`group`] takes them.
pub const GROUP: (&str, u32) = ("igGroup", 0x24);
pub const SPATIAL_NODE: (&str, u32) = ("igSpatialNode", 0x28);
pub const SIMPLE_USER_INFO: (&str, u32) = ("igSimpleUserInfo", 0x28);
pub const TRANSFORM: (&str, u32) = ("igTransform", 0x80);

/// A file of one `igSceneInfo` over what `root` builds (`None` leaves
/// `_sceneGraph` null), with an empty `_sceneGraphList`.
pub fn scene_over(root: impl FnOnce(&mut IgzBuilder, u8) -> Option<Location>) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let info = b.object(0, "igSceneInfo", 0x48);
    let blocks = b.section(0x10);
    if let Some(r) = root(&mut b, blocks) {
        b.pointer(at(info, 0x14), r);
    }
    b.build()
}

/// `AbstractPlacement`'s and `Placement`'s fields bar `_script` and
/// `_collision`: the name, `_ID`, the init matrix, the activity state and
/// the scale; tint grey, replace colour grey and clear.
pub fn placement(
    b: &mut IgzBuilder,
    o: Location,
    name: &str,
    id: u32,
    matrix: &[f32; 6],
    state: u32,
) {
    b.string(at(o, 0x08), name);
    b.put_u32(at(o, 0x20), id);
    floats(b, o, 0x24, matrix);
    b.put_u32(at(o, 0x54), state);
    b.put_u32(at(o, 0x68), u32::MAX);
    b.put_u32(at(o, 0x9C), 3);
    b.put_u32(at(o, 0xA0), 0x800);
    b.put_u32(at(o, 0xA4), 2);
    floats(b, o, 0xB4, &[0.5]);
    floats(b, o, 0xB8, &[150.0, 125.0, 100.0]);
    b.put_bytes(
        at(o, 0xC8),
        &[0x80, 0x80, 0x80, 0xFF, 0x80, 0x80, 0x80, 0x00],
    );
    floats(b, o, 0xD0, &[160.0, 8.0]);
}

pub fn sample() -> Sample {
    let mut b = IgzBuilder::new();
    let scene = b.object(0, "igSceneInfo", 0x48);
    let extra_roots = b.object(0, "igNodeList", 0x18);
    let keyframes = b.object(0, "igTransformSource3", 0x50);
    let blocks = b.section(0x10);
    let wire = unskinned().wire;
    let segments = [(); 4].map(|_| segment(&mut b, blocks, &wire));
    let geometries = [
        geometry(&mut b, blocks, &segments[..2]),
        geometry(&mut b, blocks, &segments[2..3]),
        geometry(&mut b, blocks, &segments[3..]),
    ];
    let [(t0, a0), (t1, a1), (t2, a2)] = [
        ("Ground", geometries[0]),
        ("Sky_morning", geometries[1]),
        ("Windmill_Blades", geometries[2]),
    ]
    .map(|(name, g)| technique(&mut b, name, g));
    let sky = group(
        &mut b,
        blocks,
        SIMPLE_USER_INFO,
        "|Sky_morning:SKYDOME|Sky_morning:pPlane3_Userdata",
        &[t1],
    );
    let spatial = group(&mut b, blocks, SPATIAL_NODE, "spatial", &[t0, sky]);
    let inner = group(&mut b, blocks, TRANSFORM, "inner", &[t2]);
    floats(&mut b, inner, 0x30, QUARTER_TURN.as_flattened());
    b.put_u32(at(inner, 0x70), (-1i32) as u32);
    b.pointer(at(inner, 0x78), keyframes);
    let outer = group(&mut b, blocks, TRANSFORM, "outer", &[inner]);
    floats(&mut b, outer, 0x30, TRANSLATION.as_flattened());
    b.put_u32(at(outer, 0x70), 4);
    let root = group(&mut b, blocks, GROUP, "sceneGraph", &[spatial, outer]);

    // igSceneInfo: igInfo's fields, the root, both i64s, up, no more roots.
    b.string(at(scene, 0x08), "igSceneInfo0");
    b.put_bytes(at(scene, 0x10), &[1]);
    b.pointer(at(scene, 0x14), root);
    b.put_u32(at(scene, 0x20), 1);
    b.put_u32(at(scene, 0x24), 2);
    b.put_u32(at(scene, 0x28), u32::MAX);
    b.put_u32(at(scene, 0x2C), u32::MAX - 1);
    floats(&mut b, scene, 0x30, &[0.0, 1.0, 0.0]);
    b.pointer(at(scene, 0x40), extra_roots);
    list(&mut b, extra_roots, blocks, &[]);

    let placements = placements(&mut b, blocks);
    Sample {
        bytes: b.build(),
        scene,
        root,
        spatial,
        sky,
        outer,
        inner,
        keyframes,
        techniques: [t0, t1, t2],
        attrs: [a0, a1, a2],
        geometries,
        segments,
        actor: placements.actor,
        volume: placements.volume,
        script: placements.script,
        collisions: placements.collisions,
        parameters: placements.parameters,
        model: placements.model,
        waypoint_list: placements.waypoint_list,
        waypoints: placements.waypoints,
        emitter: placements.emitter,
        light: placements.light,
        rig: placements.rig,
    }
}

struct Placed {
    actor: Location,
    volume: Location,
    script: Location,
    collisions: [Location; 2],
    parameters: Location,
    model: Location,
    waypoint_list: Location,
    waypoints: [Location; 3],
    emitter: Location,
    light: Location,
    rig: Location,
}

fn placements(b: &mut IgzBuilder, blocks: u8) -> Placed {
    let actor = b.object(0, "tfbActorInfo", 0xF8);
    let volume = b.object(0, "tfbActorInfo", 0xF8);
    let script = b.object(0, "tfbScriptInfo", 0x1C);
    let collisions = [(); 2].map(|_| b.object(0, "CollisionInfo", 0x58));
    let parameters = b.object(0, "ActorParameters", 0x4C);
    let model = b.object(0, "tfbPhysicsModel", 0x30);
    let waypoint_list = b.object(0, "ActorWaypointList", 0x18);
    let waypoints = [(); 3].map(|_| b.object(0, "ActorWaypoint", 0x38));
    let emitter = b.object(0, "tfbParticleInfo", 0xE0);
    let light = b.object(0, "tfbLightInfo", 0x10C);
    let rig = b.object(0, "DirectionalLightInfo", 0x40);

    // The spawn: every reference set.
    placement(
        b,
        actor,
        "213_Init_Location",
        7,
        &[10.191, 43.379, -93.394, 7.0, 310.0, -50.0],
        4,
    );
    b.pointer(at(actor, 0xA8), script);
    b.pointer(at(actor, 0xC4), collisions[0]);
    b.pointer(at(actor, 0xDC), model);
    b.pointer(at(actor, 0xE0), parameters);
    b.pointer(at(actor, 0xE4), waypoint_list);
    floats(b, actor, 0xE8, &[1.0, 2.0, 3.0]);

    // A camera volume: no script, parameters or waypoints.
    placement(
        b,
        volume,
        "Camera_Transition_36-37",
        0,
        &[-111.049, -141.954, 259.518, 7.0, 168.0, 0.0],
        5,
    );
    b.pointer(at(volume, 0xC4), collisions[1]);
    b.pointer(at(volume, 0xDC), model);

    b.string(
        at(script, 0x08),
        "Levels/Synthetic/Scripts/InitializeLevel.ai",
    );
    b.put_bytes(at(script, 0x10), &[1]);

    // A solid sphere that interacts with the world, lifted 1.5 and turned.
    b.put_u32(at(collisions[0], 0x18), 0x1B);
    floats(
        b,
        collisions[0],
        0x1C,
        &[0.0, 1.5, 0.0, 0.0, 90.0, 0.0, 4.0, 6.0, 3.0, 10.0, 25.0],
    );
    // A box that isn't solid, 60 x 21 x 100.
    b.put_u32(at(collisions[1], 0x18), 0x01);
    floats(b, collisions[1], 0x34, &[60.0, 21.0, 100.0, 1.0, 0.0]);

    floats(
        b,
        parameters,
        0x0C,
        &[
            18.0, 160.0, 80.0, 360.0, 20.0, 150.0, 70.0, 1440.0, 120.0, 128.0, 300.0, 4.8, 32.0,
        ],
    );
    b.put_u32(at(parameters, 0x40), 2);
    b.put_bytes(at(parameters, 0x44), &[1]);
    floats(b, parameters, 0x48, &[1.25]);

    b.string(at(model, 0x08), "Models/Objects/Door_Breakable.mdl");
    b.put_bytes(at(model, 0x10), &[1]);
    b.put_u32(at(model, 0x2C), 0x1234);

    list(b, waypoint_list, blocks, &waypoints.map(Some));
    let points: [(&str, f32, [f32; 6]); 3] = [
        ("wp0", 0.0, [10.191, 43.379, -93.394, 0.0, 0.0, 0.0]),
        ("wp1", 2.5, [12.0, 43.0, -90.0, 0.0, 45.0, 0.0]),
        ("wp2", -1.0, [14.0, 44.0, -88.0, 0.0, 90.0, 0.0]),
    ];
    for (o, (name, height, matrix)) in waypoints.into_iter().zip(points) {
        b.string(at(o, 0x08), name);
        floats(b, o, 0x1C, &[height]);
        floats(b, o, 0x20, &matrix);
    }

    placement(
        b,
        emitter,
        "fire_house(1)",
        0,
        &[111.728, 19.05, 78.483, 0.0, 0.0, 0.0],
        5,
    );
    b.meta_object(at(emitter, 0xD8), "metaobject", "GeneratorInfoList");

    placement(
        b,
        light,
        "213_EvilRobot_Beam_light",
        0,
        &[1.0, 2.0, 3.0, 0.0, 0.0, 0.0],
        5,
    );
    b.put_u32(at(light, 0xD8), 1);
    b.put_bytes(at(light, 0xDC), &[1]);
    floats(b, light, 0xE0, &[0.25, 30.0, 1.0, 0.5, 0.125]);
    b.put_bytes(
        at(light, 0xF4),
        &[
            0x55, 0x2A, 0x11, 0xFF, 0, 0, 0, 0xFF, 0x2C, 0x1F, 0x13, 0xFF,
        ],
    );
    floats(b, light, 0x100, &[1.5]);

    b.string(at(rig, 0x08), "_worldLight1");
    b.put_bytes(
        at(rig, 0x18),
        &[200, 190, 170, 255, 40, 45, 60, 255, 255, 250, 240, 255],
    );
    floats(b, rig, 0x24, &[1.0, 0.0, 0.0, 0.0, -45.0, 30.0, 0.0]);

    Placed {
        actor,
        volume,
        script,
        collisions,
        parameters,
        model,
        waypoint_list,
        waypoints,
        emitter,
        light,
        rig,
    }
}

/// `scenes` scenes all rooted at one group over `groups` empty groups, as a
/// mutated file can: the walk's budget, not the input's size, has to bound
/// the work.
pub fn shared_root(scenes: usize, groups: usize) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let infos: Vec<Location> = (0..scenes)
        .map(|_| b.object(0, "igSceneInfo", 0x48))
        .collect();
    let root = b.object(0, "igGroup", 0x24);
    let children = b.object(0, "igNodeList", 0x18);
    let leaves: Vec<Location> = (0..groups).map(|_| b.object(0, "igGroup", 0x24)).collect();
    let blocks = b.section(0x10);
    for &info in &infos {
        b.pointer(at(info, 0x14), root);
    }
    b.pointer(at(root, 0x20), children);
    let elements: Vec<_> = leaves.iter().copied().map(Some).collect();
    list(&mut b, children, blocks, &elements);
    b.build()
}

/// Seeds for the property test and the fuzz corpus: the sample, a handful
/// of scenes sharing one graph, and the level stage's sample (AG.15: its
/// materials, its static, skinned and transformed props).
pub fn seeds() -> Vec<Vec<u8>> {
    vec![
        sample().bytes,
        shared_root(8, 16),
        stage_level(&stage_sample()),
    ]
}

/// What [`stage_level`] builds (AG.15): a level the level stage reads
/// whole, shaped as level 213's is. One physics world names the scene and
/// holds the collision; the scene's techniques hang under a spatial node,
/// the last [`StageSpec::sky`] of them under the sky's group; each prop is
/// an actor drawing its own model.
#[derive(Debug, Clone)]
pub struct StageSpec {
    /// The physics world's name: its scene's and its collision's.
    pub world: &'static str,
    pub collision: crate::collision::builder::MeshSpec,
    /// Images the materials bind, by index; equal specs make equal images.
    pub images: Vec<(crate::texture::builder::Spec, &'static str)>,
    /// The scene's techniques, in order. Another effect than tfbmulti2 draws
    /// the unlit fixture (no normals, as the grass), tfbmulti2 the unskinned.
    pub scene: Vec<crate::model::builder::MaterialSpec>,
    pub sky: usize,
    /// Static props: each model's path and its one technique's material.
    pub props: Vec<(&'static str, crate::model::builder::MaterialSpec)>,
    /// A skinned prop, built as a character's model is, at this path.
    pub skinned: Option<(&'static str, crate::model::builder::CharacterSpec)>,
    /// The script the skinned prop's placement runs (AH.22's palettes
    /// follow a whitelisted one to its creature), an empty frame.
    pub skinned_script: Option<&'static str>,
    /// A prop whose technique sits under an animated transform.
    pub transformed: Option<(&'static str, crate::model::builder::MaterialSpec)>,
    /// The PS3's zones its definition script adds (AG.41); `None` writes no
    /// definition script.
    pub atmosphere: Option<Vec<crate::atmosphere::builder::ZoneCall>>,
}

/// The collision world's name in [`stage_sample`].
pub const STAGE_WORLD: &str = "Levels/Synthetic/Synthetic_ART_MASTER.ma";

/// A tfbmulti2 material binding `colour` alone (variant 0).
pub fn colour_only(name: &'static str, colour: usize) -> crate::model::builder::MaterialSpec {
    use crate::model::builder::{BindSpec, MaterialSpec};
    MaterialSpec {
        name,
        effect: crate::material::TFBMULTI2,
        pixel: Some((0, 0x3FF)),
        vertex: Some((0x1000, 0x1BFF)),
        binds: vec![BindSpec::new(0, colour)],
        attrs: Vec::new(),
        scroll: None,
        tracks: Vec::new(),
    }
}

/// Level 213's shapes in small: the ground on the body variant, whose
/// colour map is the character sample's (`eruptor_c`, so the two share a
/// texture), the one gloss-map material, a grass technique on its own
/// effect (scrolling, as the water's warp does), a cave painting that
/// multiplies and a sky whose tint moves; a static gourd, a skinned chest
/// built as a character is, and a fairy gem under an animated transform; a
/// ramp to stand on.
pub fn stage_sample() -> StageSpec {
    use crate::material::GRASS_EFFECT;
    use crate::model::builder::{AttrSpec, BindSpec, MaterialSpec, TrackSpec};
    use crate::texture::builder::{SPECS, Spec};
    let spec = |i: usize, seed: u8| Spec { seed, ..SPECS[i] };
    let mut ground = MaterialSpec::body("Ground");
    ground.pixel = Some((0x1C, 0x3FF));
    ground.vertex = Some((0x101C, 0x1BFF));
    ground.attrs.clear();
    let gloss = MaterialSpec {
        name: "Lava_Gloss",
        pixel: Some((0x9C, 0x3FF)),
        vertex: Some((0x109C, 0x1BFF)),
        binds: vec![
            BindSpec::new(4, 0).normal_map(),
            BindSpec::new(6, 1),
            BindSpec::new(0, 2),
            BindSpec::new(2, 4),
            BindSpec::new(3, 3),
        ],
        attrs: vec![AttrSpec::Vec4(
            "tfbMulti2FixedAlphaValuesAttr",
            [-1.0, -1.0, 0.5, 1.0],
        )],
        ..MaterialSpec::body("Lava_Gloss")
    };
    let grass = MaterialSpec {
        name: "grass_HIRES",
        effect: GRASS_EFFECT,
        pixel: Some((0x50, 0x50)),
        vertex: Some((0x50, 0x50)),
        binds: vec![
            BindSpec::new(1, 5),
            BindSpec::new(2, 6),
            BindSpec::new(0, 2),
        ],
        attrs: vec![
            AttrSpec::DepthWrite(false),
            AttrSpec::BlendState(true),
            AttrSpec::AlphaState(true),
            AttrSpec::AlphaFunction(4, 0.023529),
            AttrSpec::Other(crate::material::SHADER_PARAMETERS, 0x18),
        ],
        scroll: Some((0, [0.25; 11], "tfbScrollUVAnimTrack")),
        tracks: Vec::new(),
    };
    let mut cave = colour_only("cave_painting", 2);
    cave.attrs = vec![
        AttrSpec::BlendState(true),
        AttrSpec::BlendFunction(6, 0),
        AttrSpec::DepthWrite(false),
    ];
    let mut sky = colour_only("Sky_morning", 7);
    sky.tracks = vec![TrackSpec::Tint {
        style: 1,
        rate: 1.0,
        frames: vec![0, 2000],
        colors: vec![0xFF70_6D6E, 0x9670_6D6E],
    }];
    let mut chest = crate::model::builder::sample();
    chest.actors.clear();
    chest.attachments.clear();
    chest.slots.clear();
    chest.portal_sprite = "Chest_Sprite";
    StageSpec {
        world: STAGE_WORLD,
        collision: crate::collision::builder::ramp(),
        images: vec![
            (spec(1, 0x21), "ground_n"),
            (spec(0, 0x61), "ground_sp"),
            (spec(0, 0x10), "eruptor_c"),
            (spec(0, 0x71), "ground_rim"),
            (spec(0, 0x41), "lava_gloss"),
            (spec(0, 0x51), "grass_hieght"),
            (spec(1, 0x31), "grass_random"),
            (spec(0, 0x11), "sky"),
        ],
        scene: vec![ground, gloss, grass, cave, sky],
        sky: 1,
        props: vec![(
            "C:/tfb/Content/Models/Objects/gourd_01.mdl",
            colour_only("gourd", 2),
        )],
        skinned: Some(("C:/tfb/Content/Models/Objects/Treasure_Chest.mdl", chest)),
        skinned_script: Some("C:/tfb/Content/Levels/Synthetic/Scripts/S2_Enemy_Chompy.ai"),
        transformed: Some((
            "C:/tfb/Content/Models/Objects/fairy_upgrade_gem.mdl",
            colour_only("m_butterfly_wings", 7),
        )),
        atmosphere: Some(vec![
            crate::atmosphere::builder::ZoneCall::sample(0, 1),
            crate::atmosphere::builder::ZoneCall::sample(1, 2),
        ]),
    }
}

/// A `tfbPhysicsModel` named `path` whose body's `_node` is `node`, no
/// skeleton or clips: a static prop.
pub(crate) fn static_model(b: &mut IgzBuilder, path: &str, node: Location) -> Location {
    let model = b.object(0, "tfbPhysicsModel", 0x30);
    let body = b.object(0, "tfbPhysicsBody", 0x40);
    crate::model::builder::info(b, model, path);
    b.pointer(at(model, 0x14), body);
    crate::model::builder::info(b, body, path);
    b.null_memory_ref(at(body, 0x14), 0);
    b.pointer(at(body, 0x20), node);
    b.null_memory_ref(at(body, 0x28), 0);
    model
}

/// `spec`'s level as its `level.bld` IGZ.
pub fn stage_level(spec: &StageSpec) -> Vec<u8> {
    use crate::collision::builder::{body_spec, world_packfile, world_spec};
    use crate::edge::builder::{unlit, unskinned};
    use crate::model::builder::technique as material_technique;
    use crate::texture::builder::named_image;

    let mut b = IgzBuilder::new();
    let blocks = b.section(0x80);
    let images: Vec<Location> = spec
        .images
        .iter()
        .map(|(s, name)| named_image(&mut b, blocks, s, &s.data(), name))
        .collect();
    let lit = unskinned();
    let flat = unlit();
    let fixture = |m: &crate::model::builder::MaterialSpec| {
        if m.effect == crate::material::TFBMULTI2 {
            &lit
        } else {
            &flat
        }
    };

    // The world, its collision and its scene.
    let world = b.object(0, "tfbPhysicsWorld", 0x2C);
    let scene = b.object(0, "igSceneInfo", 0x48);
    let extra_roots = b.object(0, "igNodeList", 0x18);
    b.string(at(world, 0x08), spec.world);
    b.put_bytes(at(world, 0x10), &[1]);
    let packfile = world_packfile(&world_spec(vec![body_spec(spec.collision.clone())])).bytes;
    b.memory_ref(at(world, 0x14), blocks, &packfile, 2);
    b.put_u32(at(world, 0x1C), 0x5EED);
    b.pointer(at(world, 0x20), scene);
    b.put_u32(at(world, 0x28), 0x1234);
    let techniques: Vec<Location> = spec
        .scene
        .iter()
        .map(|m| material_technique(&mut b, blocks, m, &images, fixture(m)))
        .collect();
    let (ground, sky) = techniques.split_at(techniques.len() - spec.sky);
    let mut under_spatial = ground.to_vec();
    if !sky.is_empty() {
        under_spatial.push(group(
            &mut b,
            blocks,
            SIMPLE_USER_INFO,
            "|Sky_morning_213:SKYDOME|Sky_morning_213:pPlane3_Userdata",
            sky,
        ));
    }
    let spatial = group(&mut b, blocks, SPATIAL_NODE, "", &under_spatial);
    let root = group(&mut b, blocks, GROUP, "sceneGraph", &[spatial]);
    b.string(at(scene, 0x08), "igSceneInfo0");
    b.put_bytes(at(scene, 0x10), &[1]);
    b.pointer(at(scene, 0x14), root);
    floats(&mut b, scene, 0x30, &[0.0, 1.0, 0.0]);
    b.pointer(at(scene, 0x40), extra_roots);
    list(&mut b, extra_roots, blocks, &[]);

    // The props' models.
    let mut models: Vec<Location> = spec
        .props
        .iter()
        .map(|(path, m)| {
            let t = material_technique(&mut b, blocks, m, &images, fixture(m));
            static_model(&mut b, path, t)
        })
        .collect();
    if let Some((path, character)) = &spec.skinned {
        let mut character = character.clone();
        character.model = path;
        models.push(crate::model::builder::character_in(
            &mut b, blocks, &character,
        ));
    }
    if let Some((path, m)) = &spec.transformed {
        let t = material_technique(&mut b, blocks, m, &images, fixture(m));
        let keyframes = b.object(0, "igTransformSource3", 0x50);
        let spin = group(&mut b, blocks, TRANSFORM, "spin", &[t]);
        floats(&mut b, spin, 0x30, TRANSLATION.as_flattened());
        b.put_u32(at(spin, 0x70), (-1i32) as u32);
        b.pointer(at(spin, 0x78), keyframes);
        models.push(static_model(&mut b, path, spin));
    }

    // The actors: the spawn with its waypoints, then one per prop.
    let spawn = b.object(0, "tfbActorInfo", 0xF8);
    // An empty frame, so the file's every script reads (AH.13's graph).
    let script = crate::script::builder::Writer::new(&mut b).script(
        "Levels/Synthetic/Scripts/InitializeLevel.ai",
        |_| {},
        |_| {},
        |_| {},
    );
    let solid = b.object(0, "CollisionInfo", 0x58);
    let parameters = b.object(0, "ActorParameters", 0x4C);
    let waypoint_list = b.object(0, "ActorWaypointList", 0x18);
    let waypoints = [(); 2].map(|_| b.object(0, "ActorWaypoint", 0x38));
    placement(
        &mut b,
        spawn,
        "213_Init_Location",
        7,
        &[10.191352, 43.37944, -93.393616, 0.0, 0.0, 0.0],
        4,
    );
    b.pointer(at(spawn, 0xA8), script);
    b.pointer(at(spawn, 0xC4), solid);
    b.pointer(at(spawn, 0xE0), parameters);
    b.pointer(at(spawn, 0xE4), waypoint_list);
    b.put_u32(at(solid, 0x18), 0x1B);
    floats(
        &mut b,
        solid,
        0x1C,
        &[0.0, 1.5, 0.0, 0.0, 90.0, 0.0, 4.0, 6.0, 3.0, 10.0, 25.0],
    );
    floats(
        &mut b,
        parameters,
        0x0C,
        &[
            18.0, 160.0, 80.0, 360.0, 20.0, 150.0, 70.0, 1440.0, 120.0, 128.0, 0.0, 0.0, 0.0,
        ],
    );
    b.put_bytes(at(parameters, 0x44), &[1]);
    floats(&mut b, parameters, 0x48, &[1.0]);
    list(&mut b, waypoint_list, blocks, &waypoints.map(Some));
    for (k, o) in waypoints.into_iter().enumerate() {
        b.string(at(o, 0x08), &format!("wp{k}"));
        floats(&mut b, o, 0x1C, &[k as f32]);
        floats(
            &mut b,
            o,
            0x20,
            &[
                10.191352 + k as f32,
                43.37944,
                -93.393616,
                0.0,
                45.0 * k as f32,
                0.0,
            ],
        );
    }
    let skinned = spec.skinned.as_ref().map(|_| spec.props.len());
    for (k, &model) in models.iter().enumerate() {
        let a = b.object(0, "tfbActorInfo", 0xF8);
        let c = b.object(0, "CollisionInfo", 0x58);
        if let (Some(path), true) = (spec.skinned_script, skinned == Some(k)) {
            let info =
                crate::script::builder::Writer::new(&mut b).script(path, |_| {}, |_| {}, |_| {});
            b.pointer(at(a, 0xA8), info);
        }
        placement(
            &mut b,
            a,
            &format!("prop({k})"),
            100 + k as u32,
            &[k as f32, 44.0, -90.0, 0.0, 180.0, 0.0],
            4 + (k % 2) as u32,
        );
        b.pointer(at(a, 0xC4), c);
        b.pointer(at(a, 0xDC), model);
        b.put_u32(at(c, 0x18), 0x19);
        floats(&mut b, c, 0x34, &[4.0, 21.0, 34.0, 1.0, 0.0]);
    }

    // A placed point light and two of the rig.
    let light = b.object(0, "tfbLightInfo", 0x10C);
    placement(
        &mut b,
        light,
        "213_EvilRobot_Beam_light",
        0,
        &[1.0, 2.0, 3.0, 0.0, 0.0, 0.0],
        5,
    );
    b.put_u32(at(light, 0xD8), 1);
    b.put_bytes(at(light, 0xDC), &[1]);
    floats(&mut b, light, 0xE0, &[0.25, 30.0, 1.0, 0.5, 0.125]);
    b.put_bytes(
        at(light, 0xF4),
        &[
            0x55, 0x2A, 0x11, 0xFF, 0, 0, 0, 0xFF, 0x2C, 0x1F, 0x13, 0xFF,
        ],
    );
    floats(&mut b, light, 0x100, &[1.5]);
    for (k, name) in ["_worldLight1", "_worldLight2"].into_iter().enumerate() {
        let rig = b.object(0, "DirectionalLightInfo", 0x40);
        b.string(at(rig, 0x08), name);
        b.put_bytes(
            at(rig, 0x18),
            &[200, 190, 170, 255, 40, 45, 60, 255, 255, 250, 240, 255],
        );
        floats(
            &mut b,
            rig,
            0x24,
            &[1.0, 0.0, 0.0, 0.0, -45.0 + 90.0 * k as f32, 30.0, 0.0],
        );
    }
    if let Some(zones) = &spec.atmosphere {
        let shape = crate::atmosphere::builder::Shape::default();
        crate::atmosphere::builder::definition(&mut b, &[], zones, &[], &shape);
    }
    b.build()
}
