//! AE.5's model census (design decisions 6, 7 and 9) off the real Giants
//! dump. For every load file the roster names it finds the model the game
//! draws for the toy on its portal screen (the model the archive's
//! `<NNN>_Portal_Toy` sprite draws: the one sprite-drawn model with a
//! skeleton of its own, the sprite's name telling it from a skinned
//! attachment on the three archives that have one), the segments a fresh
//! figure draws (that model's own scene graph), each segment's material
//! (key, effect, feature bits, render states, UV tracks, the formats of the
//! images it binds) and the idle clip (the animation set's slot 0); the
//! models other sprites attach to it; and which item, adventure-pack and
//! sidekick archives carry a drawable model. It asserts Eruptor's facts
//! (AC.13's draws, AC.17's clip, ae-shader's masks) and prints the rest for
//! AE.5's tick. Ignored by default: it reads `GUAC_GIANTS_DUMP` from the
//! environment or the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --release --test giants_models -- --ignored --nocapture`

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::known::KNOWN_DUMPS;
use guac_extract::roster;
use guac_formats::edge::{BoneMap, Segment};
use guac_formats::fnv1a32;
use guac_formats::havok::{self, BlendHint};
use guac_formats::iga::Archive;
use guac_formats::igz::layouts::{
    self, Group, Hero, Image2, Layout, Node, NodeList, ObjectRef, PhysicsModel,
    RuntimeTechniqueInstance, SpriteInfo, is_a,
};
use guac_formats::igz::{Igz, Ref};
use guac_formats::texture::{Format, META_IMAGES};
use guac_ir::{Category, DumpIdentity, RosterFigure, Sha256};

fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        return Some(value);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

/// `_effect`'s import ids in `global.bld` (AE.1's disc test).
const TFBMULTI2: u32 = 0xAA7;
const TFBWARP: u32 = 0xAA9;
/// Pixel bits no character sets (rim by position, gloss map, reflections)
/// and the vertex skinning bit no variant compiles (ae-shader section 4).
const PS_UNSET: u32 = 1 << 6 | 1 << 7 | 1 << 9;
const VS_SKINNING: u32 = 1 << 11;
/// The one runtime bit the turntable sets (decision 14): fog on at zero.
const FOG: u32 = 1 << 10;

fn level(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let archive = Archive::parse(&bytes).unwrap();
    let entry = archive
        .entries()
        .iter()
        .find(|e| e.name().ends_with("level.bld"))
        .unwrap_or_else(|| panic!("{}: no level.bld", path.display()));
    archive.read(entry).unwrap().into_owned()
}

fn name<'a>(igz: &Igz<'a>, i: usize) -> Option<&'a str> {
    match igz.reference(igz.objects()[i].field(0x08)?, "_name").ok()? {
        Ref::String(b) => std::str::from_utf8(b).ok(),
        _ => None,
    }
}

fn object_at(igz: &Igz<'_>, i: usize, offset: u32) -> Option<usize> {
    match igz.reference(igz.objects()[i].field(offset)?, "ref").ok()? {
        Ref::Object(k) => Some(k),
        _ => None,
    }
}

fn u32_at(igz: &Igz<'_>, i: usize, offset: u32) -> u32 {
    igz.u32_at(igz.objects()[i].field(offset).unwrap(), "field")
        .unwrap()
}

fn bool_at(igz: &Igz<'_>, i: usize, offset: u32) -> bool {
    igz.bytes(igz.objects()[i].field(offset).unwrap(), 1, "field")
        .unwrap()[0]
        != 0
}

/// An object list outside the layout gate: count at +8, elements' memory
/// ref at +0x10.
fn list(igz: &Igz<'_>, i: usize) -> Vec<Option<usize>> {
    let o = igz.objects()[i];
    let count = igz.u32_at(o.field(0x08).unwrap(), "_count").unwrap();
    let Some(data) = igz.memory_ref(o.field(0x10).unwrap(), "_data").unwrap() else {
        return Vec::new();
    };
    (0..count)
        .map(
            |k| match igz.reference(data.at.offset_by(4 * k).unwrap(), "element") {
                Ok(Ref::Object(e)) => Some(e),
                _ => None,
            },
        )
        .collect()
}

/// A render-state attr's `_enabled` at its registry offset, +0xD. The
/// files give these types an MTSZ 4 over the registry (ae-materials); AC.14
/// settled such bytes as padding past the layout, so the census records
/// what they hold and refuses a fixup in them.
fn enabled(igz: &Igz<'_>, i: usize, registered: u32, census: &mut Census) -> bool {
    let o = igz.objects()[i];
    let ty = igz.type_name(&o).to_owned();
    let pad = match o.size.checked_sub(registered) {
        Some(0) => None,
        Some(4) => {
            let at = o.at.offset_by(registered).unwrap();
            if !igz.markers_in(at, 4).is_empty() {
                census
                    .failures
                    .push(format!("{ty} #{i}: a fixup in its pad"));
            }
            // From `_readOnlyCopy` (+0xC) to the object's end.
            let tail = o.at.offset_by(0x0C).unwrap();
            Some(
                igz.bytes(tail, (o.size - 0x0C) as usize, "tail")
                    .unwrap()
                    .to_vec(),
            )
        }
        _ => {
            census
                .failures
                .push(format!("{ty} #{i}: MTSZ {:#x}", o.size));
            None
        }
    };
    census
        .state_pads
        .entry((ty, o.size))
        .or_default()
        .insert(pad);
    bool_at(igz, i, 0x0D)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Blend {
    Opaque,
    /// Blending on, no function attr.
    Alpha,
    Func {
        src: u32,
        dst: u32,
    },
}

/// What the material's attrs set of the pipeline (decision 9's render
/// states); absent attrs leave blending off, depth writes on, culling on
/// and no alpha test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct States {
    blend: Blend,
    depth_write: bool,
    two_sided: bool,
    /// `igAlphaFunctionAttr._func` (4 is GREATER) when the test is on.
    alpha_test: Option<u32>,
}

impl Default for States {
    fn default() -> Self {
        Self {
            blend: Blend::Opaque,
            depth_write: true,
            two_sided: false,
            alpha_test: None,
        }
    }
}

/// A material as stored: both variant masks and the states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Key {
    ps: u32,
    ps_ignore: u32,
    vs: u32,
    vs_ignore: u32,
    states: States,
}

impl Key {
    /// The variants the turntable draws: the material's bits plus fog where
    /// the runtime owns bit 10 (decision 9's selector).
    fn drawn(&self) -> (u32, u32, States) {
        let fog = |base: u32, ignore: u32| if ignore & FOG == 0 { base | FOG } else { base };
        (
            fog(self.ps, self.ps_ignore),
            fog(self.vs, self.vs_ignore),
            self.states,
        )
    }
}

/// ae-shader's existence rules: pixel bit 6 or 7 needs 2, 12 needs 11, 13
/// needs 12, never 1 with 7; vertex 6 or 7 needs 2, never 11.
fn variant_exists(ps: u32, vs: u32) -> bool {
    let has = |m: u32, b: u32| m & (1 << b) != 0;
    let ps_ok = (!has(ps, 6) || has(ps, 2))
        && (!has(ps, 7) || has(ps, 2))
        && (!has(ps, 12) || has(ps, 11))
        && (!has(ps, 13) || has(ps, 12))
        && !(has(ps, 1) && has(ps, 7));
    let vs_ok = (!has(vs, 6) || has(vs, 2)) && (!has(vs, 7) || has(vs, 2)) && !has(vs, 11);
    ps_ok && vs_ok
}

/// A technique's material: its effect, both masks and the states, every
/// attr type, the bound images' formats and its animation tracks.
#[derive(Debug, Clone, PartialEq)]
struct Material {
    name: String,
    effect: Option<u32>,
    key: Option<Key>,
    /// Every attr type in list order.
    attrs: Vec<String>,
    /// The bound images' formats in list order; `None` for a bind without
    /// an image.
    images: Vec<Option<Format>>,
    /// (track type, layer) per material-animation track.
    tracks: Vec<(String, Option<i32>)>,
}

/// One model the roster's figures load, read through the census's rule.
struct Model {
    load: String,
    figures: Vec<String>,
    /// The portal sprite and its model's name.
    sprite: String,
    model: String,
    segments: Vec<usize>,
    vertices: usize,
    triangles: usize,
    skinned: usize,
    bones: usize,
    /// Havok's joints when each names one Alchemy bone, else why not.
    joints: Result<usize, String>,
    materials: Vec<Material>,
    /// The scene graph's node types and flags under the model's actor.
    nodes: BTreeMap<(String, i32), usize>,
    ti_flags: BTreeSet<u32>,
    idle: Result<(String, f32), String>,
    other_idles: Vec<(usize, String)>,
    /// Other sprites' models: (sprite, model).
    attached: Vec<(String, String)>,
    /// Actor infos whose `_model` is the portal model, and the one named
    /// like the load file if its model differs.
    actors: Vec<String>,
    actor_mismatch: Option<(String, String)>,
}

#[derive(Default)]
struct Census {
    failures: Vec<String>,
    /// Per render-state type and file size, the bytes from +0xC to the end
    /// of each padded one.
    state_pads: BTreeMap<(String, u32), BTreeSet<Option<Vec<u8>>>>,
    /// Archives with two or more sprite-drawn models carrying a skeleton,
    /// told apart by the portal sprite's name.
    tie_broken: Vec<String>,
    /// What the portal sprites and the attaching ones hold beyond their
    /// model, to tell a fresh figure's attachments from an upgrade's.
    sprite_shapes: BTreeMap<SpriteShape, usize>,
}

/// A model sprite's fields that could mark it shown or hidden at load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SpriteShape {
    portal: bool,
    /// `_properties`: the visual type, blend, alignment and visibility bits.
    properties: u32,
    flags: i32,
    flag_pool: u32,
    activity_state: u32,
    attach_node: bool,
    attached_objects: bool,
}

fn sprite_shape(igz: &Igz<'_>, sprite: usize, portal: bool) -> SpriteShape {
    let s = SpriteInfo::read(igz, sprite).unwrap();
    let p = &s.placement;
    let some = |r: ObjectRef<'_>| r != ObjectRef::Null;
    let attached = match p.attached_objects {
        ObjectRef::Object(l) => !list(igz, l).is_empty(),
        other => some(other),
    };
    SpriteShape {
        portal,
        properties: s.properties,
        flags: s.flags,
        flag_pool: p.flag_pool,
        activity_state: p.activity_state,
        attach_node: some(p.attach_node),
        attached_objects: attached,
    }
}

/// A sprite and the model it draws, by object index.
type Drawn = (usize, usize);

/// The archive's sprite-drawn models: those with a skeleton of their own
/// (Hero walks them), then the rest.
fn sprite_models(igz: &Igz<'_>, path: &str, census: &mut Census) -> (Vec<Drawn>, Vec<Drawn>) {
    let mut heroes = Vec::new();
    let mut props = Vec::new();
    for (i, s) in layouts::read_all::<SpriteInfo>(igz) {
        let s = match s {
            Ok(s) => s,
            Err(e) => {
                census.failures.push(format!("{path}: sprite #{i}: {e}"));
                continue;
            }
        };
        let ObjectRef::Object(m) = s.context_data_info else {
            continue;
        };
        if !is_a(igz.type_name(&igz.objects()[m]), PhysicsModel::TYPE) {
            continue;
        }
        if Hero::read(igz, m).is_ok() {
            heroes.push((i, m));
        } else {
            props.push((i, m));
        }
    }
    (heroes, props)
}

/// The portal model: the archive's one sprite-drawn model with a skeleton,
/// or, where a skinned attachment has one too (Tree Rex's woodpecker,
/// Eye-Brawl's eyeball), the one whose sprite is named `…Portal_Toy…`.
fn portal_model(
    igz: &Igz<'_>,
    path: &str,
    heroes: &mut Vec<Drawn>,
    census: &mut Census,
) -> Option<Drawn> {
    if let [one] = heroes[..] {
        heroes.clear();
        return Some(one);
    }
    let named =
        |s: usize| name(igz, s).is_some_and(|n| n.to_ascii_lowercase().contains("portal_toy"));
    let picked: Vec<usize> = (0..heroes.len()).filter(|&k| named(heroes[k].0)).collect();
    let [k] = picked[..] else {
        census.failures.push(format!(
            "{path}: {} sprite-drawn models with a skeleton, {} on a Portal_Toy sprite",
            heroes.len(),
            picked.len()
        ));
        return None;
    };
    let one = heroes.remove(k);
    census.tie_broken.push(format!(
        "{path}: {} over {:?}",
        model_name(igz, one.1),
        heroes
            .iter()
            .map(|&(_, m)| model_name(igz, m))
            .collect::<Vec<_>>()
    ));
    Some(one)
}

fn model_name(igz: &Igz<'_>, m: usize) -> String {
    PhysicsModel::read(igz, m)
        .ok()
        .and_then(|p| p.model.info.name)
        .map(|n| n.rsplit('/').next().unwrap_or(n).to_owned())
        .unwrap_or_else(|| format!("#{m}"))
}

fn material(
    igz: &Igz<'_>,
    hero: &Hero<'_>,
    mesh: &layouts::Mesh<'_>,
    census: &mut Census,
) -> Material {
    let t = &mesh.technique;
    let effect = match t.effect {
        ObjectRef::Import(imp) if imp.id.namespace == fnv1a32(b"global.bld") => Some(imp.id.object),
        _ => None,
    };
    let mut attrs = Vec::new();
    let mut images = Vec::new();
    let (mut ps, mut vs) = (None, None);
    let mut states = States::default();
    let mut alpha_func = None;
    if let Some(list) = mesh.material.and_then(|m| hero.materials.get(&m)) {
        for &a in list.attrs.iter().flatten() {
            let ty = igz.type_name(&igz.objects()[a]).to_owned();
            match ty.as_str() {
                "igPixelShaderVariantMaskAttr" => {
                    ps = Some((u32_at(igz, a, 0x10), u32_at(igz, a, 0x14)))
                }
                "igVertexShaderVariantMaskAttr" => {
                    vs = Some((u32_at(igz, a, 0x10), u32_at(igz, a, 0x14)))
                }
                "igBlendStateAttr" => {
                    if enabled(igz, a, 0x10, census) && states.blend == Blend::Opaque {
                        states.blend = Blend::Alpha;
                    }
                }
                "igBlendFunctionAttr" => {
                    states.blend = Blend::Func {
                        src: u32_at(igz, a, 0x10),
                        dst: u32_at(igz, a, 0x14),
                    }
                }
                "igDepthWriteStateAttr" => states.depth_write = enabled(igz, a, 0x10, census),
                "igCullFaceAttr" => states.two_sided = !enabled(igz, a, 0x14, census),
                "igAlphaStateAttr" => {
                    if enabled(igz, a, 0x10, census) {
                        states.alpha_test = Some(u32::MAX);
                    }
                }
                "igAlphaFunctionAttr" => alpha_func = Some(u32_at(igz, a, 0x10)),
                _ => {}
            }
            if let Some(bind) = hero.texture_binds.get(&a) {
                let image = bind
                    .bind
                    .texture
                    .and_then(|t| hero.textures.get(&t))
                    .and_then(|t| t.image);
                images.push(image.map(|img| {
                    let f = Image2::read(igz, img).unwrap().format;
                    META_IMAGES
                        .iter()
                        .find(|m| f.id.is("metaimages", m.name))
                        .map(|m| m.format)
                        .unwrap_or_else(|| panic!("an image format outside AC.6's table: {f:?}"))
                }));
            }
            attrs.push(ty);
        }
    }
    // A test with no function attr keeps u32::MAX, which the print shows.
    if let (Some(test), Some(f)) = (states.alpha_test.as_mut(), alpha_func) {
        *test = f;
    }
    let mut tracks = Vec::new();
    if let ObjectRef::Object(anim) = t.material_anim
        && let Some(track_list) = object_at(igz, anim, 0x08)
    {
        for track in list(igz, track_list).into_iter().flatten() {
            let ty = igz.type_name(&igz.objects()[track]).to_owned();
            let layer = ty
                .contains("UVAnimTrack")
                .then(|| u32_at(igz, track, 0x24) as i32);
            tracks.push((ty, layer));
        }
    }
    let key = match (ps, vs) {
        (Some((ps, ps_ignore)), Some((vs, vs_ignore))) => Some(Key {
            ps,
            ps_ignore,
            vs,
            vs_ignore,
            states,
        }),
        _ => None,
    };
    Material {
        name: t.node.name.unwrap_or("?").to_owned(),
        effect,
        key,
        attrs,
        images,
        tracks,
    }
}

/// Node types (with `igNode._flags`) under the hero's actor, and the
/// techniques' `_tiFlags`.
fn graph(igz: &Igz<'_>, actor: usize) -> (BTreeMap<(String, i32), usize>, BTreeSet<u32>) {
    let mut nodes = BTreeMap::new();
    let mut ti = BTreeSet::new();
    let mut stack = vec![actor];
    let mut seen = HashSet::new();
    while let Some(n) = stack.pop() {
        if !seen.insert(n) {
            continue;
        }
        let ty = igz.type_name(&igz.objects()[n]).to_owned();
        let flags = Node::read(igz, n).map(|n| n.flags).unwrap_or(i32::MIN);
        *nodes.entry((ty.clone(), flags)).or_default() += 1;
        if is_a(&ty, RuntimeTechniqueInstance::TYPE) {
            ti.insert(RuntimeTechniqueInstance::read(igz, n).unwrap().ti_flags);
            continue;
        }
        if let Some(l) = Group::read(igz, n).unwrap().child_list {
            stack.extend(NodeList::read(igz, l).unwrap().nodes);
        }
    }
    (nodes, ti)
}

/// The animation set's slot 0 (where every character file keeps its idle,
/// walk in 1) when it's a full-body clip that poses the model's skeleton;
/// otherwise why not.
fn idle(hero: &Hero<'_>) -> Result<(String, f32), String> {
    let info = hero
        .clip_slots
        .first()
        .copied()
        .flatten()
        .ok_or("slot 0 empty")?;
    let clip = &hero.clips[&info];
    let (_, anim) = clip
        .animation
        .ok_or("slot 0 holds no tfbPhysicsAnimation")?;
    let file = anim
        .info
        .data_info
        .info
        .name
        .ok_or("slot 0's clip is unnamed")?;
    let file = file.rsplit('/').next().unwrap_or(file).to_owned();
    if anim.info.additive_blend() || anim.info.partial_animation() {
        return Err(format!("{file} is additive or partial"));
    }
    let Some(layouts::Payload::Havok(bytes)) = anim.info.data_info.payload() else {
        return Err(format!("{file} holds no Havok packfile"));
    };
    let container = havok::Container::parse(bytes).map_err(|e| format!("{file}: {e}"))?;
    let c = container
        .clip
        .ok_or_else(|| format!("{file} has no clip"))?;
    if c.binding.blend_hint != BlendHint::Normal {
        return Err(format!("{file}'s binding is additive"));
    }
    let skeleton = match hero.body.body.data_info.payload() {
        Some(layouts::Payload::Havok(b)) => havok::Container::parse(b)
            .ok()
            .and_then(|c| c.skeletons.into_iter().next()),
        _ => None,
    }
    .ok_or("no Havok skeleton")?;
    for t in [0.0, f64::from(c.animation.duration) / 2.0] {
        c.local_pose(&skeleton, t)
            .and_then(|p| skeleton.model_pose(&p))
            .ok_or_else(|| format!("{file} doesn't pose at {t} s"))?;
    }
    Ok((file, c.animation.duration))
}

/// Named `<character>_Idle.ma`, as Eruptor's is.
fn plainly_idle(file: &str) -> bool {
    file.to_ascii_lowercase().ends_with("_idle.ma")
}

/// The clip files outside slot 0 named as idles, with their slots.
fn other_idles(hero: &Hero<'_>) -> Vec<(usize, String)> {
    hero.clip_slots
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(|(slot, info)| {
            let (_, a) = hero.clips[&(*info)?].animation?;
            let file = a.info.data_info.info.name?;
            let file = file.rsplit('/').next().unwrap_or(file);
            file.to_ascii_lowercase()
                .contains("idle")
                .then(|| (slot, file.to_owned()))
        })
        .collect()
}

/// Havok joints, each naming exactly one Alchemy bone (hero_view's map).
fn joints(hero: &Hero<'_>) -> Result<usize, String> {
    let Some(layouts::Payload::Havok(b)) = hero.body.body.data_info.payload() else {
        return Err("no Havok skeleton".into());
    };
    let c = havok::Container::parse(b).map_err(|e| e.to_string())?;
    let s = c.skeletons.first().ok_or("no hkaSkeleton")?;
    for b in &s.bones {
        let n = b.name.ok_or("an unnamed Havok bone")?;
        let found = hero.bones.iter().filter(|a| a.name == Some(n)).count();
        if found != 1 {
            let alchemy: Vec<&str> = hero.bones.iter().filter_map(|a| a.name).collect();
            return Err(format!(
                "{n} names {found} of the Alchemy bones {alchemy:?}"
            ));
        }
    }
    Ok(s.bones.len())
}

fn census_model(
    dump: &Path,
    dir: &str,
    load: &str,
    figures: Vec<String>,
    census: &mut Census,
) -> Option<Model> {
    let path = format!("PS3_GAME/USRDIR/{dir}/{}.bld", load.to_ascii_lowercase());
    let bytes = level(&dump.join(&path));
    let igz = Igz::parse(&bytes).unwrap();
    let (mut heroes, mut props) = sprite_models(&igz, &path, census);
    let (sprite, m) = portal_model(&igz, &path, &mut heroes, census)?;
    // A skinned attachment is still an attachment.
    props.extend(heroes);
    props.sort_unstable();
    for (s, portal) in std::iter::once((sprite, true)).chain(props.iter().map(|&(s, _)| (s, false)))
    {
        *census
            .sprite_shapes
            .entry(sprite_shape(&igz, s, portal))
            .or_default() += 1;
    }
    let hero = Hero::read(&igz, m).unwrap();
    let mut segments = Vec::new();
    let (mut vertices, mut triangles, mut skinned) = (0, 0, 0);
    let mut materials = Vec::new();
    for mesh in &hero.meshes {
        materials.push(material(&igz, &hero, mesh, census));
        let map = mesh
            .palette
            .and_then(|p| hero.palettes.get(&p)?.indices)
            .and_then(|i| hero.int_lists.get(&i))
            .map(|l| BoneMap::new(&l.values, &hero.bones));
        for (i, layout) in hero.mesh_segments(mesh) {
            match Segment::decode(&igz, i, layout) {
                Ok(s) => {
                    vertices += s.vertex_count();
                    triangles += s.triangles().count();
                    if let Some(skin) = &s.skin {
                        skinned += 1;
                        let mapped = match &map {
                            Some(map) => skin.bones(map).map(|_| ()).map_err(|e| e.to_string()),
                            None => Err("skinned without a palette".to_owned()),
                        };
                        if let Err(e) = mapped {
                            census.failures.push(format!("{path}: segment #{i}: {e}"));
                        }
                    }
                }
                Err(e) => census.failures.push(format!("{path}: segment #{i}: {e}")),
            }
            segments.push(i);
        }
    }
    let (nodes, ti_flags) = graph(&igz, hero.actor_index);
    let squash = |s: &str| -> String {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase()
    };
    let mut actors = Vec::new();
    let mut actor_mismatch = None;
    for (i, o) in igz.objects().iter().enumerate() {
        if igz.type_name(o) != "tfbActorInfo" {
            continue;
        }
        let n = name(&igz, i).unwrap_or("?").to_owned();
        let model = object_at(&igz, i, 0xDC);
        if model == Some(m) {
            actors.push(n);
        } else if squash(&n) == squash(load)
            && let Some(other) = model
        {
            actor_mismatch = Some((n, model_name(&igz, other)));
        }
    }
    Some(Model {
        load: load.to_owned(),
        figures,
        sprite: name(&igz, sprite).unwrap_or("?").to_owned(),
        model: model_name(&igz, m),
        segments,
        vertices,
        triangles,
        skinned,
        bones: hero.bones.len(),
        joints: joints(&hero),
        materials,
        nodes,
        ti_flags,
        idle: idle(&hero),
        other_idles: other_idles(&hero),
        attached: props
            .iter()
            .map(|&(s, pm)| {
                (
                    name(&igz, s).unwrap_or("?").to_owned(),
                    model_name(&igz, pm),
                )
            })
            .collect(),
        actors,
        actor_mismatch,
    })
}

/// Counts over a set of models' materials.
#[derive(Default)]
struct MaterialTotals {
    materials: usize,
    keys: BTreeSet<Key>,
    effects: BTreeMap<String, usize>,
    ps_bases: BTreeMap<u32, usize>,
    states: BTreeMap<States, usize>,
    tracks: BTreeMap<String, usize>,
    images: BTreeMap<String, usize>,
    attrs: BTreeMap<String, usize>,
    /// tfbwarp, pixel bits 6/7/9, vertex bit 11, a variant the existence
    /// rules rule out, no masks, an L8 image.
    odd: Vec<String>,
}

impl MaterialTotals {
    fn of(models: &[Model]) -> Self {
        let mut t = Self::default();
        for m in models {
            for mat in &m.materials {
                t.materials += 1;
                let effect = match mat.effect {
                    Some(TFBMULTI2) => "tfbmulti2".to_owned(),
                    Some(TFBWARP) => "tfbwarp".to_owned(),
                    other => format!("{other:?}"),
                };
                if effect != "tfbmulti2" {
                    t.odd.push(format!("{}: {} on {effect}", m.load, mat.name));
                }
                *t.effects.entry(effect).or_default() += 1;
                for a in &mat.attrs {
                    *t.attrs.entry(a.clone()).or_default() += 1;
                }
                for i in &mat.images {
                    *t.images.entry(format!("{i:?}")).or_default() += 1;
                }
                if mat.images.contains(&Some(Format::R8Unorm)) {
                    t.odd
                        .push(format!("{}: {} binds an L8 image", m.load, mat.name));
                }
                for (ty, l) in &mat.tracks {
                    *t.tracks.entry(format!("{ty} layer {l:?}")).or_default() += 1;
                }
                let Some(k) = mat.key else {
                    t.odd
                        .push(format!("{}: {} has no variant masks", m.load, mat.name));
                    continue;
                };
                t.keys.insert(k);
                *t.states.entry(k.states).or_default() += 1;
                *t.ps_bases.entry(k.ps).or_default() += 1;
                let (ps, vs, _) = k.drawn();
                if k.ps & PS_UNSET != 0 || k.vs & VS_SKINNING != 0 || !variant_exists(ps, vs) {
                    t.odd
                        .push(format!("{}: {} {}", m.load, mat.name, key_text(&k)));
                }
            }
        }
        t
    }

    fn print(&self) {
        let drawn: BTreeSet<(u32, u32, States)> = self.keys.iter().map(Key::drawn).collect();
        println!(
            "  {} materials, {} distinct stored keys (masks + states), {} distinct as the turntable draws them (fog on)",
            self.materials,
            self.keys.len(),
            drawn.len()
        );
        println!("  effects: {:?}", self.effects);
        let bases: Vec<String> = self
            .ps_bases
            .iter()
            .map(|(k, n)| format!("{}:{n}", hex(*k)))
            .collect();
        println!("  pixel base masks: {bases:?}");
        println!("  render states: {:?}", self.states);
        println!("  material-animation tracks: {:?}", self.tracks);
        println!("  bound images by format: {:?}", self.images);
        println!("  attr types: {:?}", self.attrs);
        println!(
            "  tfbwarp, pixel bits 6/7/9, vertex bit 11, no such variant, no masks or an L8 image: {:?}",
            self.odd
        );
        println!("  drawn keys:");
        for (ps, vs, s) in &drawn {
            println!("    ps {} vs {} {s:?}", hex(*ps), hex(*vs));
        }
    }
}

fn hex(mask: u32) -> String {
    format!("{mask:#06x}")
}

fn key_text(k: &Key) -> String {
    format!(
        "ps {}/{} vs {}/{} {:?}",
        hex(k.ps),
        hex(k.ps_ignore),
        hex(k.vs),
        hex(k.vs_ignore),
        k.states
    )
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn the_roster_models_census() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let known = KNOWN_DUMPS[0];
    let identity = DumpIdentity {
        title_id: known.title_id.into(),
        app_ver: known.app_ver.into(),
        manifest_hash: Sha256::from_hex(known.manifest_hash).unwrap(),
    };
    let started = Instant::now();
    let r = roster::read(&dump, identity).unwrap();

    // Load files in roster order, each with the figures that load it.
    let mut loads: Vec<(String, bool, Vec<String>)> = Vec::new();
    for f in &r.figures {
        let character = matches!(f.category, Category::Figure | Category::Giant);
        match loads.iter_mut().find(|(l, _, _)| *l == f.load_file) {
            Some((_, _, ids)) => ids.push(f.id()),
            None => loads.push((f.load_file.clone(), character, vec![f.id()])),
        }
    }
    let figure = |id: &str| -> &RosterFigure { r.figure(id).unwrap() };
    let mut census = Census::default();
    let mut characters = Vec::new();
    let mut others = Vec::new();
    for (load, character, ids) in &loads {
        let dir = if *character { "character" } else { "item" };
        if let Some(m) = census_model(&dump, dir, load, ids.clone(), &mut census) {
            if *character {
                characters.push(m);
            } else {
                others.push(m);
            }
        }
    }
    let took = started.elapsed();
    let character_figures: usize = characters.iter().map(|m| m.figures.len()).sum();
    println!(
        "{} load files: {} character ({} figures), {} item, adventure-pack and sidekick; {:.2} s",
        loads.len(),
        characters.len(),
        character_figures,
        others.len(),
        took.as_secs_f64()
    );

    // The rule, per character.
    println!(
        "\n== characters: portal sprite -> model, fresh segments, idle, what other sprites attach"
    );
    for m in &characters {
        println!(
            "{} ({}): {} -> {} segs {:?} ({} v, {} t, {} skinned), {} bones, joints {:?}, idle {}, actors {:?}{}",
            m.load,
            m.figures.join(" "),
            m.sprite,
            m.model,
            m.segments,
            m.vertices,
            m.triangles,
            m.skinned,
            m.bones,
            m.joints,
            match &m.idle {
                Ok((f, d)) => format!("{f} {d:.4} s"),
                Err(e) => format!("UNCLEAR ({e})"),
            },
            m.actors,
            match &m.actor_mismatch {
                Some((a, other)) => format!(", actor {a} loads {other} instead"),
                None => String::new(),
            }
        );
        for mat in &m.materials {
            println!(
                "    {}: effect {:?} key {} images {:?} tracks {:?}",
                mat.name,
                mat.effect.map(hex),
                mat.key
                    .as_ref()
                    .map(key_text)
                    .unwrap_or_else(|| "none".into()),
                mat.images,
                mat.tracks
            );
        }
        if !m.attached.is_empty() {
            let attached: Vec<String> = m
                .attached
                .iter()
                .map(|(s, pm)| format!("{s} -> {pm}"))
                .collect();
            println!("    attached: {}", attached.join("; "));
        }
    }

    // Totals over the character models.
    let sum = |f: fn(&Model) -> usize| -> usize { characters.iter().map(f).sum() };
    println!(
        "\n== {} character models: {} segments ({} skinned), {} vertices, {} triangles",
        characters.len(),
        sum(|m| m.segments.len()),
        sum(|m| m.skinned),
        sum(|m| m.vertices),
        sum(|m| m.triangles)
    );
    let unmapped: Vec<String> = characters
        .iter()
        .chain(&others)
        .filter_map(|m| m.joints.as_ref().err().map(|e| format!("{}: {e}", m.load)))
        .collect();
    println!("  Havok joints naming no single Alchemy bone, every model: {unmapped:?}");
    let totals = MaterialTotals::of(&characters);
    totals.print();
    let pads: Vec<String> = census
        .state_pads
        .iter()
        .map(|((t, size), pads)| format!("{t} MTSZ {size:#x}: bytes from +0xC {pads:?}"))
        .collect();
    println!("  render-state attrs: {pads:?}");
    println!(
        "  tie broken by the Portal_Toy sprite's name: {:?}",
        census.tie_broken
    );
    println!(
        "  model sprites, every archive (portal or attaching): {:?}",
        census.sprite_shapes
    );

    // Scene-graph structure under the portal models.
    let mut nodes: BTreeMap<(String, i32), usize> = BTreeMap::new();
    let mut ti: BTreeMap<u32, usize> = BTreeMap::new();
    for m in &characters {
        for (k, n) in &m.nodes {
            *nodes.entry(k.clone()).or_default() += n;
        }
        for f in &m.ti_flags {
            *ti.entry(*f).or_default() += 1;
        }
    }
    println!("  scene graphs, (node type, igNode._flags): {nodes:?}");
    let ti: Vec<String> = ti.iter().map(|(k, n)| format!("{k:#x}:{n}")).collect();
    println!("  technique _tiFlags (models carrying each): {ti:?}");

    // Idles and what the rule can't settle.
    let no_idle: Vec<String> = characters
        .iter()
        .filter_map(|m| m.idle.as_ref().err().map(|e| format!("{}: {e}", m.load)))
        .collect();
    let qualified: Vec<String> = characters
        .iter()
        .filter_map(|m| {
            let (file, _) = m.idle.as_ref().ok()?;
            (!plainly_idle(file)).then(|| format!("{}: {file}, also {:?}", m.load, m.other_idles))
        })
        .collect();
    let durations: Vec<f32> = characters
        .iter()
        .filter_map(|m| m.idle.as_ref().ok().map(|(_, d)| *d))
        .collect();
    println!(
        "\n== idles: slot 0 a full-body clip posing the skeleton on {} of {}, {} named <character>_Idle.ma, {:.4}-{:.4} s",
        characters.len() - no_idle.len(),
        characters.len(),
        characters.len() - no_idle.len() - qualified.len(),
        durations.iter().copied().fold(f32::INFINITY, f32::min),
        durations.iter().copied().fold(0.0, f32::max),
    );
    println!("  no idle: {no_idle:?}");
    println!("  named otherwise: {qualified:?}");
    let with_attachments: Vec<&str> = characters
        .iter()
        .filter(|m| !m.attached.is_empty())
        .map(|m| m.load.as_str())
        .collect();
    let mismatched: Vec<String> = characters
        .iter()
        .filter_map(|m| {
            m.actor_mismatch
                .as_ref()
                .map(|(a, o)| format!("{} ({a} -> {o}, sprite -> {})", m.load, m.model))
        })
        .collect();
    let repose: Vec<String> = characters
        .iter()
        .filter(|m| m.figures.iter().any(|id| figure(id).variant == 0x0800))
        .filter_map(|m| {
            let named: Vec<&str> = m
                .attached
                .iter()
                .filter(|(s, _)| s.to_ascii_lowercase().contains("repose"))
                .map(|(s, _)| s.as_str())
                .collect();
            (!named.is_empty()).then(|| format!("{}: {named:?}", m.load))
        })
        .collect();
    let series_2 = characters
        .iter()
        .filter(|m| m.figures.iter().any(|id| figure(id).variant == 0x0800))
        .count();
    println!("\n== what the rule can't settle");
    println!(
        "  {} of {} archives attach other models to the portal model through sprites: {with_attachments:?}",
        with_attachments.len(),
        characters.len()
    );
    println!("  gameplay actor's model differs from the portal sprite's: {mismatched:?}");
    println!("  of {series_2} Series 2 toys' archives, those naming a repose sprite: {repose:?}");

    // Items, adventure packs and sidekicks (Open question 6).
    println!("\n== items, adventure packs and sidekicks");
    let drawable = |m: &Model| {
        m.vertices > 0
            && m.materials
                .iter()
                .all(|x| x.effect == Some(TFBMULTI2) && x.key.is_some())
    };
    for m in &others {
        let kinds: Vec<String> = m
            .figures
            .iter()
            .map(|id| format!("{:?}", figure(id).category))
            .collect();
        let keys: Vec<String> = m
            .materials
            .iter()
            .filter_map(|x| x.key.as_ref().map(key_text))
            .collect();
        println!(
            "{} ({}): {} -> {} segs {:?} ({} v, {} t, {} skinned), {} bones, joints {:?}, drawable {}, keys {keys:?}, idle {}{}",
            m.load,
            kinds.join(" "),
            m.sprite,
            m.model,
            m.segments,
            m.vertices,
            m.triangles,
            m.skinned,
            m.bones,
            m.joints,
            drawable(m),
            match &m.idle {
                Ok((f, d)) => format!("{f} {d:.4} s"),
                Err(e) => format!("none ({e})"),
            },
            if m.attached.is_empty() {
                String::new()
            } else {
                format!(", attached {:?}", m.attached)
            }
        );
    }
    let n = others.iter().filter(|m| drawable(m)).count();
    println!("{n} of {} carry a drawable model", others.len());
    MaterialTotals::of(&others).print();

    println!("\nfailures: {}", census.failures.len());
    for f in &census.failures {
        println!("  {f}");
    }

    // Eruptor: AC.13's draws, AC.17's clip, ae-shader's masks.
    let eruptor = characters.iter().find(|m| m.load == "009_Eruptor").unwrap();
    assert_eq!(eruptor.sprite, "009_Portal_Toy");
    assert_eq!(eruptor.model, "Eruptor.mdl");
    assert_eq!(eruptor.segments, [122, 123, 135]);
    assert_eq!((eruptor.vertices, eruptor.triangles), (2_861, 4_812));
    assert_eq!((eruptor.bones, eruptor.joints.clone()), (34, Ok(33)));
    assert_eq!(
        eruptor.idle.as_ref().map(|(f, _)| f.as_str()),
        Ok("Eruptor_Idle.ma")
    );
    let body = Key {
        ps: 0x1C,
        ps_ignore: 0x2FF,
        vs: 0x1C,
        vs_ignore: 0x1AFF,
        states: States::default(),
    };
    let materials: Vec<(&str, Option<Key>, Option<u32>)> = eruptor
        .materials
        .iter()
        .map(|x| (x.name.as_str(), x.key, x.effect))
        .collect();
    assert_eq!(
        materials,
        [
            ("EruptorBody", Some(body), Some(TFBMULTI2)),
            ("EruptorEyes_Teeth", Some(body), Some(TFBMULTI2)),
        ]
    );
    assert_eq!(body.drawn(), (0x41C, 0x41C, States::default()));
    // The spikes are another sprite's model, outside the fresh set.
    assert_eq!(
        eruptor.attached,
        [(
            "009_ReposeSpikes".to_owned(),
            "Eruptor_UpgradeSpike_L1.mdl".to_owned()
        )]
    );
    assert!(census.failures.is_empty());
}
