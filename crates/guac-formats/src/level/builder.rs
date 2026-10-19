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

/// Seeds for the property test and the fuzz corpus: the sample, and a
/// handful of scenes sharing one graph.
pub fn seeds() -> Vec<Vec<u8>> {
    vec![sample().bytes, shared_root(8, 16)]
}
