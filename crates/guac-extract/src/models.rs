//! Character models off the dump (openspec AE.6, design decisions 6-8): a
//! character archive's portal model ([`guac_formats::model`], AE.5's rule)
//! into IR v2's [`Model`], its textures as KTX2 keyed by content, and
//! `global.bld`'s 3D-UI rig into the IR's [`Rig`]. Writing them is the model
//! stage's ([`crate::stages`]).
//!
//! What a model carries is what the decoders AC proved give: Edge segments
//! in bind pose, skinned to Havok's joints by name with Alchemy's inverse
//! joints (AC.17), each technique's tfbmulti2 material read by
//! [`guac_formats::material`], and slot 0's idle as its spline data.
//!
//! The body is the model the game draws for the figure in play
//! ([`fmodel::in_play`]), and beside it what a fresh figure carries (AE.23,
//! [`guac_formats::attachments`]): each model the hero's script hangs on it
//! with every upgrade unbought, read as a level's prop is
//! ([`crate::levels::prop`]), with the attach point it hangs on. A set the
//! rule can't settle is left off with why ([`Model::attachments_unsettled`]),
//! and the figure draws its body alone.
//!
//! Beside it the hero side (AG.14, design decisions 8 and 14), a [`Hero`]:
//! the placement the archive plays the character as, with its solid and
//! disc movement parameters, and the clips in the animation set's
//! locomotion slots. A character whose hero doesn't settle keeps its model
//! and is reported; it doesn't walk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use std::time::{Duration, Instant};

use guac_formats::FormatError;
use guac_formats::attachments::{self as fattach, Template};
use guac_formats::edge::{BoneMap, Segment as EdgeSegment, Semantic};
use guac_formats::exports::Exports;
use guac_formats::havok::spline::{Channel, QuatTrack, VecTrack as HavokVecTrack};
use guac_formats::havok::{self, QsTransform};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    ActorParameters, BoneAttachInfo, CollisionInfo, DirectionalLightInfo, EulerTransform,
    Shape as FShape, TextureAttr2, read_all,
};
use guac_formats::material::{self, Blend as FBlend, Glow as FGlow, Role};
use guac_formats::model::{self as fmodel, JointMap, PortalModel};
use guac_formats::texture::Texture;
use guac_ir::actor::{Movement, Shape, Solid};
use guac_ir::clip::{Clip, RotationTrack, Track, VecSpline, VecTrack};
use guac_ir::hero::{Gait, Locomotion, SlotClip};
use guac_ir::model::{
    Attachment, Blend, Filter, Glow, Joint, Mask, Material, MaterialTrack, Params, RenderStates,
    Sampler, Scroll, Segment, Skeleton, TextureRole, TextureSlot, Transform, Wrap,
};
use guac_ir::{Category, DumpIdentity, Hero, Model, Rig, RigLight, Roster, Sha256};
use sha2::Digest;

use crate::roster::{GLOBAL, LEVEL};
use crate::{ExtractError, Result};

/// Where models go in the extract, one `<load file>.<ron|postcard>` each.
pub const MODEL_DIR: &str = "models";
/// Where textures go, one `<sha256>.ktx2` per distinct file.
pub const TEXTURE_DIR: &str = "textures";
/// Where the hero side goes, one `<load file>.<ron|postcard>` each (AG.14).
pub const HERO_DIR: &str = "heroes";
/// Each gait's slot in the animation set ([`guac_formats::model`]'s).
pub const GAITS: [(Gait, usize); 5] = [
    (Gait::Walk, fmodel::WALK_SLOT),
    (Gait::Jog, fmodel::JOG_SLOT),
    (Gait::Run, fmodel::RUN_SLOT),
    (Gait::Falling, fmodel::FALL_SLOT),
    (Gait::Landing, fmodel::LANDING_SLOT),
];
/// The folder holding the character archives, under `PS3_GAME/USRDIR`.
pub const CHARACTER_DIR: &str = "character";
/// The rig's lights in `global.bld`, in order: key, fill, back.
pub const RIG_LIGHTS: [&str; 3] = ["_spriteLight1", "_spriteLight2", "_spriteLight3"];

/// One character archive's model and the texture files it names.
#[derive(Debug, Clone)]
pub struct Extracted {
    pub model: Model,
    /// Each distinct texture its materials bind: its extract path and its
    /// KTX2, in first-bind order.
    pub textures: Vec<(String, Vec<u8>)>,
    /// Texture binds over every material, distinct or not.
    pub binds: usize,
    /// The hero side, or why it didn't settle
    /// ([`ExtractError::HeroUnsettled`]'s message).
    pub hero: std::result::Result<Hero, String>,
    /// What reading the hero side took, of the whole read.
    pub hero_took: Duration,
}

/// The character load files a roster names, in roster order: each figure
/// and Giant's, once.
pub fn character_loads(roster: &Roster) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for f in &roster.figures {
        if matches!(f.category, Category::Figure | Category::Giant)
            && !out.contains(&f.load_file.as_str())
        {
            out.push(&f.load_file);
        }
    }
    out
}

/// `character/<load file>.bld`, lower-cased as the disc names them, under
/// the dump's `PS3_GAME/USRDIR`.
pub fn archive_path(usrdir: &Path, load_file: &str) -> PathBuf {
    usrdir
        .join(CHARACTER_DIR)
        .join(format!("{}.bld", load_file.to_ascii_lowercase()))
}

/// The archive at `path`. One that isn't there is `load_file`'s model
/// refused, not the run stopped; any other read error stops it.
pub fn archive(path: &Path, load_file: &str) -> Result<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(refused(
            load_file,
            format!("no archive at {}", path.display()),
        )),
        Err(e) => Err(ExtractError::io(path)(e)),
    }
}

fn refused(load_file: &str, reason: impl ToString) -> ExtractError {
    ExtractError::ModelUnreadable {
        load_file: load_file.to_owned(),
        reason: reason.to_string(),
    }
}

/// The `level.bld` a character archive keeps its model in; an archive
/// without one, or that won't read, is refused under `name`.
pub fn level(archive: &[u8], name: &str) -> Result<Vec<u8>> {
    let arc = Archive::parse(archive).map_err(|e| refused(name, e))?;
    let entry = arc
        .find(LEVEL)
        .ok_or_else(|| refused(name, "no level.bld"))?;
    Ok(arc.read(entry).map_err(|e| refused(name, e))?.into_owned())
}

/// `global.bld`'s level and the export ids into it: where a character's
/// clones of a global template (`Dummy_Inviso`) find what they draw.
pub struct Globals<'a> {
    pub global: Igz<'a>,
    pub exports: Exports,
}

impl<'a> Globals<'a> {
    /// Over `global.bld`'s and `bootstrap.bld`'s `level.bld`s.
    pub fn new(global: &'a [u8], bootstrap: &[u8]) -> std::result::Result<Self, FormatError> {
        let global = Igz::parse(global)?;
        let exports = Exports::new(&Igz::parse(bootstrap)?, &global)?;
        Ok(Self { global, exports })
    }
}

/// The model `load_file`'s archive level holds, what a fresh figure of it
/// carries and its hero side. Without `globals`, a set that clones a global
/// template is left unsettled.
pub fn read(
    level: &[u8],
    load_file: &str,
    identity: DumpIdentity,
    globals: Option<&Globals<'_>>,
) -> Result<Extracted> {
    let bad = |e: FormatError| refused(load_file, e);
    let igz = Igz::parse(level).map_err(bad)?;
    let portal = fmodel::portal_model(&igz).map_err(bad)?;
    let portal = fmodel::in_play(&igz, portal, load_file).map_err(bad)?;
    let hero = &portal.hero;
    let havok_skeleton = fmodel::skeleton(&igz, hero).map_err(bad)?;
    let joints = JointMap::new(&havok_skeleton, &hero.bones).map_err(bad)?;
    let inverse = joints
        .inverse_joints(&hero.bones, &hero.skeleton.inverse_joints)
        .map_err(bad)?;
    let skeleton = skeleton(&havok_skeleton, &inverse).map_err(|e| refused(load_file, e))?;
    let idle = fmodel::idle(&igz, hero, &havok_skeleton).map_err(bad)?;
    let idle = clip(&idle.clip, idle.file.unwrap_or_default());

    let mut textures = Textures::default();
    let mut binds = 0;
    let mut materials = Vec::with_capacity(hero.meshes.len());
    let mut segments = Vec::new();
    for mesh in &hero.meshes {
        let m = material::Material::read(&igz, hero, mesh).map_err(bad)?;
        binds += m.textures.len();
        let index =
            u16::try_from(materials.len()).map_err(|_| refused(load_file, "65,536 materials"))?;
        materials.push(multi2(&igz, &m, &mut textures).map_err(|e| refused(load_file, e))?);

        let palette = mesh
            .palette
            .and_then(|p| hero.palettes.get(&p)?.indices)
            .and_then(|i| hero.int_lists.get(&i))
            .ok_or_else(|| refused(load_file, "a technique without a skin palette"))?;
        let bone_map = BoneMap::new(&palette.values, &hero.bones);
        for (i, layout) in hero.mesh_segments(mesh) {
            let s = EdgeSegment::decode(&igz, i, layout).map_err(bad)?;
            let seg = segment(&s, index, i as u32, &bone_map, &joints)
                .map_err(|e| refused(load_file, format!("segment #{i}: {e}")))?;
            segments.push(seg);
        }
    }
    let mut global_textures = Textures::default();
    let (attachments, attachments_unsettled) = match self::attachments(
        &igz,
        &portal,
        load_file,
        &skeleton,
        globals,
        (&mut textures, &mut global_textures),
    ) {
        Ok(a) => (a, None),
        Err(why) => (Vec::new(), Some(why)),
    };
    for (path, ktx2) in global_textures.files {
        if !textures.files.iter().any(|(p, _)| *p == path) {
            textures.files.push((path, ktx2));
        }
    }
    let started = Instant::now();
    let hero = self::hero(&igz, &portal, &havok_skeleton, load_file, identity.clone())
        .map_err(|e| e.to_string());
    let hero_took = started.elapsed();
    Ok(Extracted {
        model: Model {
            identity,
            load_file: load_file.to_owned(),
            name: portal.name().unwrap_or_default().to_owned(),
            skeleton,
            materials,
            segments,
            idle,
            attachments,
            attachments_unsettled,
        },
        textures: textures.files,
        binds,
        hero,
        hero_took,
    })
}

/// Where an attachment's template lives: in the archive or `global.bld`,
/// its placement and the model it draws.
#[derive(Debug, Clone, Copy)]
struct Drawn {
    global: bool,
    placement: usize,
    model: usize,
}

/// What a fresh figure of `portal`'s character carries (AE.23): the
/// models [`fattach::hung`] keeps, each read as a prop (the archive's
/// textures into the first of `textures`, `global.bld`'s into the second)
/// with the attach point it hangs on. Why it can't settle is the error.
fn attachments(
    igz: &Igz<'_>,
    portal: &PortalModel<'_>,
    load_file: &str,
    body: &Skeleton,
    globals: Option<&Globals<'_>>,
    textures: (&mut Textures, &mut Textures),
) -> std::result::Result<Vec<Attachment>, String> {
    let hero = fmodel::hero_actor(igz, portal, load_file).map_err(|e| format!("no hero: {e}"))?;
    let fresh = fattach::fresh(igz, &hero).map_err(|e| e.to_string())?;
    let drawn = |t: &Template| -> std::result::Result<Option<Drawn>, String> {
        let (global, file, placement) = match t {
            Template::Local(p) => (false, igz, *p),
            Template::Global(id) => {
                let g = globals.ok_or("a global template without global.bld")?;
                let placement = g
                    .exports
                    .resolve_import(id)
                    .ok_or("an import of another file")?
                    .map_err(|e| e.to_string())?
                    .object()
                    .ok_or("a global template global.bld imports")?;
                (true, &g.global, placement)
            }
        };
        Ok(fattach::placement_model(file, placement)
            .map_err(|e| e.to_string())?
            .map(|model| Drawn {
                global,
                placement,
                model,
            }))
    };
    let hung =
        fattach::hung(&fresh, |t| !matches!(drawn(t), Ok(None))).map_err(|e| e.to_string())?;
    let (local_textures, global_textures) = textures;
    let mut out: Vec<Attachment> = Vec::with_capacity(hung.len());
    let mut parts: Vec<Drawn> = Vec::with_capacity(hung.len());
    for h in &hung {
        let d = drawn(&h.template)?.ok_or("a hung clone that draws nothing")?;
        let file = match (d.global, globals) {
            (true, Some(g)) => &g.global,
            _ => igz,
        };
        let name = |o: usize| {
            file.objects()[o]
                .field(8)
                .and_then(|at| file.string(at, "_name").ok().flatten())
                .unwrap_or_default()
                .to_owned()
        };
        let path = name(d.model);
        let texture_store = if d.global {
            &mut *global_textures
        } else {
            &mut *local_textures
        };
        let model = if draws_anything(file, d.model) {
            crate::levels::prop(file, d.model, &path, texture_store)
                .map_err(|e| format!("{path}: {e}"))?
        } else {
            // `inviso.mdl`: hung, drawing nothing here.
            guac_ir::level::Prop {
                path: path.clone(),
                skeleton: None,
                materials: Vec::new(),
                segments: Vec::new(),
                idle: None,
            }
        };
        // The point it hangs on: an entry of the parent's attach list, a
        // joint of the parent's skeleton by name.
        let (owner_file, owner_model, owner_joints): (&Igz<'_>, usize, Option<&Skeleton>) =
            match h.parent {
                None => (igz, hero.info.model.unwrap_or(portal.model), Some(body)),
                Some(k) => {
                    let p = parts[k];
                    let f = match (p.global, globals) {
                        (true, Some(g)) => &g.global,
                        _ => igz,
                    };
                    (f, p.model, out[k].model.skeleton.as_ref())
                }
            };
        let (bone, joint, offset) = match h.slot {
            None => (
                None,
                None,
                euler_transform(&EulerTransform {
                    position: [0.0; 3],
                    rotation: [0.0; 3],
                }),
            ),
            Some(slot) => {
                let points =
                    fattach::attach_points(owner_file, owner_model).map_err(|e| e.to_string())?;
                let p = points.get(slot).ok_or_else(|| {
                    format!("attach point {slot} past the parent's {}", points.len())
                })?;
                let bone = p.bone_name.unwrap_or_default();
                let joint = if bone == BoneAttachInfo::ROOT {
                    None
                } else {
                    let joints = owner_joints.ok_or_else(|| {
                        format!("{path} hangs on {bone} of a model without bones")
                    })?;
                    let j = joints
                        .joints
                        .iter()
                        .position(|j| j.name == bone)
                        .ok_or_else(|| format!("{path} hangs on {bone}, which its parent lacks"))?;
                    Some(u16::try_from(j).map_err(|_| "65,536 joints")?)
                };
                (Some(bone.to_owned()), joint, euler_transform(&p.transform))
            }
        };
        out.push(Attachment {
            placement: name(d.placement),
            parent: h
                .parent
                .map(|k| u16::try_from(k).map_err(|_| "65,536 attachments"))
                .transpose()?,
            bone,
            joint,
            offset,
            model,
        });
        parts.push(d);
    }
    Ok(out)
}

/// Whether a static model has a technique whose effect the material
/// reader follows (an import of `global.bld`'s effects). The global
/// `inviso.mdl` a dummy hangs (Whale's anchor, Shroomboom's slingshot)
/// has one technique naming `global.bld`'s own effect object instead,
/// which the reader doesn't follow: it's kept as an attachment drawing
/// nothing, as the invisible helper it is in the oracle's shots (AE.22). A
/// model that doesn't walk as a static one is left to
/// [`crate::levels::prop`].
fn draws_anything(igz: &Igz<'_>, model: usize) -> bool {
    match guac_formats::level::StaticModel::read(igz, model) {
        Ok(m) => m
            .graph
            .draws
            .iter()
            .any(|d| material::effect(igz, d.technique_index, &d.technique).is_ok()),
        Err(_) => true,
    }
}

/// An attach point's offset (`tfbEulerTransform`: metres, then tilt,
/// heading and roll in degrees) as the IR's transform: the position, and
/// [`guac_formats::level::euler_rows`]'s rotation (row vectors) as a
/// quaternion for column vectors, so heading 90 turns +Z to +X.
pub fn euler_transform(t: &EulerTransform) -> Transform {
    let m = guac_formats::level::euler_rows(t.rotation);
    // Column-vector R is the row-vector matrix transposed: R[i][j] = m[j][i].
    let r = |i: usize, j: usize| f64::from(m[j][i]);
    let trace = r(0, 0) + r(1, 1) + r(2, 2);
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (r(2, 1) - r(1, 2)) / s,
            (r(0, 2) - r(2, 0)) / s,
            (r(1, 0) - r(0, 1)) / s,
            0.25 * s,
        ]
    } else if r(0, 0) > r(1, 1) && r(0, 0) > r(2, 2) {
        let s = (1.0 + r(0, 0) - r(1, 1) - r(2, 2)).sqrt() * 2.0;
        [
            0.25 * s,
            (r(0, 1) + r(1, 0)) / s,
            (r(0, 2) + r(2, 0)) / s,
            (r(2, 1) - r(1, 2)) / s,
        ]
    } else if r(1, 1) > r(2, 2) {
        let s = (1.0 + r(1, 1) - r(0, 0) - r(2, 2)).sqrt() * 2.0;
        [
            (r(0, 1) + r(1, 0)) / s,
            0.25 * s,
            (r(1, 2) + r(2, 1)) / s,
            (r(0, 2) - r(2, 0)) / s,
        ]
    } else {
        let s = (1.0 + r(2, 2) - r(0, 0) - r(1, 1)).sqrt() * 2.0;
        [
            (r(0, 2) + r(2, 0)) / s,
            (r(1, 2) + r(2, 1)) / s,
            0.25 * s,
            (r(1, 0) - r(0, 1)) / s,
        ]
    };
    Transform {
        translation: t.position,
        rotation: q.map(|c| c as f32),
        scale: [1.0; 3],
    }
}

/// The hero side of `portal`'s character: its placement
/// ([`fmodel::hero_actor`]) with its solid and movement, and each gait's
/// slot clip on `skeleton`. An empty slot is a gait the character lacks;
/// it doesn't settle without an actor, its parameters, a walk or a run to
/// move in and a falling clip, or with a slot holding something other than
/// a full NORMAL clip on its skeleton.
pub fn hero(
    igz: &Igz<'_>,
    portal: &PortalModel<'_>,
    skeleton: &havok::Skeleton<'_>,
    load_file: &str,
    identity: DumpIdentity,
) -> Result<Hero> {
    let unsettled = |reason: String| ExtractError::HeroUnsettled {
        load_file: load_file.to_owned(),
        reason,
    };
    let actor = fmodel::hero_actor(igz, portal, load_file).map_err(|e| unsettled(e.to_string()))?;
    let name = actor.name().unwrap_or_default();
    let collision = actor
        .collision
        .ok_or_else(|| unsettled(format!("{name} has no CollisionInfo")))?;
    let parameters = actor
        .parameters
        .ok_or_else(|| unsettled(format!("{name} has no ActorParameters")))?;
    let mut locomotion = Locomotion {
        walk: None,
        jog: None,
        run: None,
        falling: None,
        landing: None,
    };
    for (gait, slot) in GAITS {
        let c = fmodel::slot_clip(igz, &portal.hero, skeleton, slot)
            .map_err(|e| unsettled(format!("its {gait:?} clip (slot {slot}): {e}")))?;
        *locomotion.get_mut(gait) = c.map(|c| SlotClip {
            slot: slot as u16,
            label: c.label.map(str::to_owned),
            clip: clip(&c.clip, c.file.unwrap_or_default()),
        });
    }
    if locomotion.walk.is_none() && locomotion.run.is_none() {
        return Err(unsettled(
            "neither a walk (slot 1) nor a run (slot 3) to move in".into(),
        ));
    }
    if locomotion.falling.is_none() {
        return Err(unsettled("no falling clip (slot 57)".into()));
    }
    Ok(Hero {
        identity,
        load_file: load_file.to_owned(),
        actor: name.to_owned(),
        scale: actor.placement().scale,
        solid: solid(&collision),
        movement: movement(&parameters),
        locomotion,
    })
}

/// A `CollisionInfo` as the IR keeps it, field for field.
pub fn solid(c: &CollisionInfo<'_>) -> Solid {
    Solid {
        shape: match c.shape() {
            FShape::None => Shape::None,
            FShape::Box => Shape::Box,
            FShape::Capsule => Shape::Capsule,
            FShape::Sphere => Shape::Sphere,
            FShape::Cylinder => Shape::Cylinder,
            FShape::Mesh => Shape::Mesh,
        },
        solid: c.is_solid(),
        world: c.interacts_with_world(),
        actor_surface: c.is_actor_surface(),
        offset: c.transform.position,
        rotation: c.transform.rotation,
        width: c.width,
        height: c.height,
        length: c.length,
        weight: c.weight,
        rebound: c.rebound,
    }
}

/// `ActorParameters` as the IR keeps them, field for field.
pub fn movement(a: &ActorParameters<'_>) -> Movement {
    let p = &a.parameters;
    Movement {
        ground_max_speed: p.ground_max_speed,
        ground_acceleration: p.ground_acceleration,
        ground_deceleration: p.ground_deceleration,
        ground_turn_rate: p.ground_turn_rate,
        air_max_speed: p.air_max_speed,
        air_acceleration: p.air_acceleration,
        air_deceleration: p.air_deceleration,
        air_turn_rate: p.air_turn_rate,
        gravity_max_speed: p.gravity_max_speed,
        gravity_acceleration: p.gravity_acceleration,
        jump_max_speed: p.jump_max_speed,
        jump_acceleration: p.jump_acceleration,
        jump_initial_speed: p.jump_initial_speed,
        motion_type: p.motion_type,
        tilts_with_ground: p.tilts_with_ground,
        time_scale: a.object_time_scale,
    }
}

/// `textures/<sha256 of the KTX2>.ktx2`: equal files share one name.
pub fn texture_path(ktx2: &[u8]) -> String {
    let sha = Sha256(sha2::Sha256::digest(ktx2).into());
    format!("{TEXTURE_DIR}/{sha}.ktx2")
}

/// The texture files one read binds, each image converted once and each
/// distinct file kept once, in first-bind order.
#[derive(Debug, Clone, Default)]
pub struct Textures {
    /// Its extract path and its KTX2.
    pub files: Vec<(String, Vec<u8>)>,
    by_image: BTreeMap<usize, String>,
}

impl Textures {
    /// The `igImage2` at `image` as a KTX2, its path in the extract.
    pub fn path(
        &mut self,
        igz: &Igz<'_>,
        image: usize,
    ) -> std::result::Result<String, FormatError> {
        if let Some(p) = self.by_image.get(&image) {
            return Ok(p.clone());
        }
        let ktx2 = Texture::read(igz, image)?.to_ktx2();
        let path = texture_path(&ktx2);
        if !self.files.iter().any(|(p, _)| *p == path) {
            self.files.push((path.clone(), ktx2));
        }
        self.by_image.insert(image, path.clone());
        Ok(path)
    }
}

/// A tfbmulti2 material as the IR keeps it, its textures converted into
/// `textures`.
pub fn multi2(
    igz: &Igz<'_>,
    m: &material::Material<'_>,
    textures: &mut Textures,
) -> std::result::Result<Material, String> {
    let mut slots = Vec::with_capacity(m.textures.len());
    for t in &m.textures {
        slots.push(TextureSlot {
            role: role(t.role),
            texture: textures.path(igz, t.image).map_err(|e| e.to_string())?,
            image: t.image_name.unwrap_or_default().to_owned(),
            sampler: sampler(&t.sampler)?,
        });
    }
    Ok(ir_material(m, slots))
}

fn role(r: Role) -> TextureRole {
    match r {
        Role::Layer1BaseNormalMap => TextureRole::Layer1BaseNormalMap,
        Role::Layer1BaseSpecMap => TextureRole::Layer1BaseSpecMap,
        Role::Layer1ColorMap => TextureRole::Layer1ColorMap,
        Role::Layer2BaseNormalMap => TextureRole::Layer2BaseNormalMap,
        Role::Layer2BaseSpecMap => TextureRole::Layer2BaseSpecMap,
        Role::Layer2ColorMap => TextureRole::Layer2ColorMap,
        Role::Layer3ColorMap => TextureRole::Layer3ColorMap,
        Role::Layer4ColorMap => TextureRole::Layer4ColorMap,
    }
}

/// `igTextureAttr2`'s wraps and filters, for the values the characters
/// use: wrap 0 clamps and 1 repeats; filters run nearest, linear, then the
/// mipmapped ones, of which the disc's heroes use 5, linear-linear (AC.13:
/// RSX's LINEAR_MIPMAP_LINEAR). Others are refused.
pub fn sampler(attr: &TextureAttr2) -> std::result::Result<Sampler, String> {
    let wrap = |v: u32| match v {
        0 => Ok(Wrap::Clamp),
        1 => Ok(Wrap::Repeat),
        _ => Err(format!("texture wrap {v}")),
    };
    let mag = match attr.mag_filter {
        0 => Filter::Nearest,
        1 => Filter::Linear,
        v => return Err(format!("mag filter {v}")),
    };
    let (min, mip) = match attr.min_filter {
        0 => (Filter::Nearest, None),
        1 => (Filter::Linear, None),
        5 => (Filter::Linear, Some(Filter::Linear)),
        v => return Err(format!("min filter {v}")),
    };
    Ok(Sampler {
        wrap_u: wrap(attr.wrap_s)?,
        wrap_v: wrap(attr.wrap_t)?,
        mag,
        min,
        mip,
    })
}

fn ir_material(m: &material::Material<'_>, textures: Vec<TextureSlot>) -> Material {
    let p = m.params;
    Material {
        name: m.name.unwrap_or_default().to_owned(),
        pixel: Mask {
            base: m.pixel.base_mask,
            ignore: m.pixel.ignore_mask,
        },
        vertex: Mask {
            base: m.vertex.base_mask,
            ignore: m.vertex.ignore_mask,
        },
        textures,
        params: Params {
            fixed_alpha: p.fixed_alpha,
            layer1_extra: p.layer1_extra,
            layer2_extra: p.layer2_extra,
            layer2_blend: p.layer2_blend,
            layer3_blend: p.layer3_blend,
            layer4_blend: p.layer4_blend,
            layer4_values: p.layer4_values,
            material_tint: p.material_tint,
            glow: match p.glow {
                FGlow::None => Glow::None,
                FGlow::Amount(a) => Glow::Amount(a),
                FGlow::LayerAlpha => Glow::LayerAlpha,
            },
        },
        states: states(&m.states),
        scrolls: m.scrolls.iter().map(scroll).collect(),
        tracks: m.tracks.iter().map(track).collect(),
    }
}

/// A scroll track as the IR keeps it.
pub fn scroll(s: &material::Scroll) -> Scroll {
    Scroll {
        layer: s.layer,
        scale: s.data.scale,
        velocity: s.data.velocity,
        amplitude: s.data.amplitude,
        frequency: s.data.frequency,
        phase: s.data.phase,
        rotation_phase: s.data.rotation_phase,
    }
}

/// Render states as the IR keeps them.
pub fn states(s: &material::States) -> RenderStates {
    RenderStates {
        blend: match s.blend {
            FBlend::Opaque => Blend::Opaque,
            FBlend::Alpha => Blend::Alpha,
            FBlend::Additive => Blend::Additive,
            FBlend::Multiply => Blend::Multiply,
        },
        depth_write: s.depth_write,
        two_sided: s.two_sided,
        alpha_test: s.alpha_test,
    }
}

/// A level material's other track (AG.15), as stored; a tint's packed
/// colours unpacked as the material tint is.
pub fn track(t: &material::Track) -> MaterialTrack {
    match t {
        material::Track::Transform { layer, data: d } => MaterialTrack::Transform {
            layer: *layer,
            center: d.center,
            rotation_rate: d.rotation_rate,
            rotation_min: d.rotation_min,
            rotation_max: d.rotation_max,
            rotation_style: d.rotation_style,
            scale_rate: d.scale_rate,
            scale_min: d.scale_min,
            scale_max: d.scale_max,
            scale_style: d.scale_style,
        },
        material::Track::FlipBook { layer, data, cells } => MaterialTrack::FlipBook {
            layer: *layer,
            style: data.style,
            rows: data.rows,
            columns: data.columns,
            rate: data.rate,
            cells: cells.clone(),
        },
        material::Track::Tint {
            data,
            frames,
            colors,
        } => MaterialTrack::Tint {
            style: data.style,
            rate: data.rate,
            frames: frames.clone(),
            colors: colors.iter().map(|&c| material::unpack_tint(c)).collect(),
        },
    }
}

fn transform(q: &QsTransform) -> Transform {
    Transform {
        translation: q.translation.map(|c| c as f32),
        rotation: q.rotation.map(|c| c as f32),
        scale: q.scale.map(|c| c as f32),
    }
}

/// Havok's joints in Havok's order, each with its Alchemy inverse joint.
/// The stored inverse joint is row-vector with the translation in row 3, so
/// its rows are the column-vector matrix's columns, as the IR keeps them.
pub fn skeleton(
    s: &havok::Skeleton<'_>,
    inverse: &[[[f32; 4]; 4]],
) -> std::result::Result<Skeleton, String> {
    let joints = s
        .bones
        .iter()
        .zip(&s.parents)
        .zip(&s.reference_pose)
        .zip(inverse)
        .enumerate()
        .map(|(j, (((bone, &parent), reference), inverse_bind))| {
            Ok(Joint {
                name: bone
                    .name
                    .ok_or_else(|| format!("Havok joint {j} has no name"))?
                    .to_owned(),
                parent: u16::try_from(parent).ok(),
                reference: transform(reference),
                inverse_bind: *inverse_bind,
            })
        })
        .collect::<std::result::Result<Vec<_>, String>>()?;
    if joints.len() != s.bones.len() {
        return Err(format!(
            "{} inverse joints for {} joints",
            inverse.len(),
            s.bones.len()
        ));
    }
    Ok(Skeleton { joints })
}

fn vec_track(t: &HavokVecTrack) -> VecTrack {
    VecTrack {
        constant: t.constant.map(|c| c as f32),
        spline: t.curve.as_ref().map(|c| VecSpline {
            channels: t.channels.map(|ch| ch == Channel::Spline),
            degree: c.degree,
            knots: c.knots.clone(),
            points: c.points.iter().map(|p| p.map(|c| c as f32)).collect(),
        }),
    }
}

/// A decoded clip as the IR keeps it: each track on the joint its binding
/// names, control points dequantized to f32.
pub fn clip(c: &havok::Clip<'_>, name: &str) -> Clip {
    let a = &c.animation;
    Clip {
        name: name.to_owned(),
        duration: a.duration,
        frame_duration: a.frame_duration,
        frames: a.num_frames,
        tracks: a
            .tracks
            .iter()
            .zip(&c.binding.track_to_bone)
            .map(|(t, &joint)| Track {
                joint,
                translation: vec_track(&t.position),
                rotation: match &t.rotation {
                    QuatTrack::Identity => RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
                    QuatTrack::Static(q) => RotationTrack::Constant(q.map(|c| c as f32)),
                    QuatTrack::Spline(curve) => RotationTrack::Spline {
                        degree: curve.degree,
                        knots: curve.knots.clone(),
                        points: curve.points.iter().map(|p| p.map(|c| c as f32)).collect(),
                    },
                },
                scale: vec_track(&t.scale),
            })
            .collect(),
    }
}

/// A decoded segment as the IR keeps it: positions, normals, UV0 and a skin
/// on every vertex; tangents (w = +1 from a 3-component word), UV sets 1
/// and 2 and colours where the segment has them. A weighted influence on a
/// bone Havok lacks is refused.
pub fn segment(
    s: &EdgeSegment,
    material: u16,
    source: u32,
    bone_map: &BoneMap,
    joints: &JointMap,
) -> std::result::Result<Segment, String> {
    segment_of(s, material, source, Some((bone_map, joints)), true)
}

/// A segment of a level's scene or a static prop (AG.15): as [`segment`],
/// but without a skin (one there is refused), and its normals left empty
/// when `normals` is false and it has none (the grass's and shadows'
/// effects carry none).
pub fn static_segment(
    s: &EdgeSegment,
    material: u16,
    source: u32,
    normals: bool,
) -> std::result::Result<Segment, String> {
    if s.skin.is_some() {
        return Err("a skin outside a skinned model".into());
    }
    segment_of(s, material, source, None, normals)
}

fn segment_of(
    s: &EdgeSegment,
    material: u16,
    source: u32,
    skin: Option<(&BoneMap, &JointMap)>,
    need_normals: bool,
) -> std::result::Result<Segment, String> {
    let n = s.vertex_count();
    let vectors = |sem: Semantic| -> Option<Vec<Vec<f32>>> {
        s.attribute(sem)
            .map(|a| a.vertices().map(<[f32]>::to_vec).collect())
    };
    let need = |sem: Semantic| vectors(sem).ok_or_else(|| format!("no {sem:?}"));
    let fixed = |v: Vec<Vec<f32>>, sem: Semantic| -> std::result::Result<Vec<Vec<f32>>, String> {
        if v.len() == n {
            Ok(v)
        } else {
            Err(format!("{} {sem:?} values for {n} vertices", v.len()))
        }
    };
    if s.attribute(Semantic::Binormal).is_some() || s.attribute(Semantic::Uv3).is_some() {
        return Err("a binormal or a fourth UV set".into());
    }
    let positions = fixed(need(Semantic::Position)?, Semantic::Position)?;
    let normals = match vectors(Semantic::Normal) {
        Some(v) => fixed(v, Semantic::Normal)?,
        None if need_normals => return Err(format!("no {:?}", Semantic::Normal)),
        None => Vec::new(),
    };
    let tangents = vectors(Semantic::Tangent)
        .map(|v| fixed(v, Semantic::Tangent))
        .transpose()?;
    let mut uvs = vec![fixed(need(Semantic::Uv0)?, Semantic::Uv0)?];
    for sem in [Semantic::Uv1, Semantic::Uv2] {
        match vectors(sem) {
            Some(v) => uvs.push(fixed(v, sem)?),
            None => break,
        }
    }
    if uvs.len() < 3 && s.attribute(Semantic::Uv2).is_some() {
        return Err("UV set 2 without set 1".into());
    }
    let colors = vectors(Semantic::Color)
        .map(|v| fixed(v, Semantic::Color))
        .transpose()?;
    let (mut joint_indices, mut weights) = (Vec::new(), Vec::new());
    if let Some((bone_map, joints)) = skin {
        let skin = s.skin.as_ref().ok_or("unskinned")?;
        let influences = skin.bones(bone_map).map_err(|e| e.to_string())?;
        joint_indices.reserve(n);
        weights.reserve(n);
        for (v, inf) in influences.iter().enumerate() {
            let mut j = [0u16; 4];
            let mut w = [0u8; 4];
            for (k, b) in inf.iter().enumerate() {
                if b.weight == 0 {
                    continue;
                }
                let joint = joints
                    .joint_of_bone
                    .get(b.bone)
                    .copied()
                    .flatten()
                    .ok_or_else(|| format!("vertex {v}: bone {} has no Havok joint", b.bone))?;
                j[k] = u16::try_from(joint).map_err(|_| format!("joint {joint}"))?;
                w[k] = b.weight;
            }
            joint_indices.push(j);
            weights.push(w);
        }
    }
    let three = |v: &[f32]| [v[0], v[1], v[2]];
    Ok(Segment {
        material,
        source,
        positions: positions.iter().map(|p| three(p)).collect(),
        normals: normals.iter().map(|p| three(p)).collect(),
        tangents: tangents.map(|t| {
            t.iter()
                .map(|v| [v[0], v[1], v[2], v.get(3).copied().unwrap_or(1.0)])
                .collect()
        }),
        uvs: uvs
            .iter()
            .map(|set| set.iter().map(|v| [v[0], v[1]]).collect())
            .collect(),
        colors: colors.map(|c| {
            c.iter()
                .map(|v| {
                    std::array::from_fn(|i| {
                        (v.get(i).copied().unwrap_or(1.0) * 255.0).round() as u8
                    })
                })
                .collect()
        }),
        joints: joint_indices,
        weights,
        indices: s.triangles().flatten().collect(),
    })
}

/// `global.bld`'s level: the three `_spriteLight`s, refused unless exactly
/// those three are there, each once.
pub fn rig(global_level: &[u8], identity: DumpIdentity) -> Result<Rig> {
    let bad = |e: &dyn ToString| refused(GLOBAL, e.to_string());
    let igz = Igz::parse(global_level).map_err(|e| bad(&e))?;
    let mut lights: Vec<Option<RigLight>> = vec![None; RIG_LIGHTS.len()];
    for (_, l) in read_all::<DirectionalLightInfo>(&igz) {
        let l = l.map_err(|e| bad(&e))?;
        let Some(k) = RIG_LIGHTS.iter().position(|n| Some(*n) == l.name) else {
            continue;
        };
        let light = RigLight {
            name: RIG_LIGHTS[k].to_owned(),
            diffuse: l.diffuse,
            ambient: l.ambient,
            specular: l.specular,
            multiplier: l.multiplier,
            position: l.init_matrix.position,
            rotation: l.init_matrix.rotation,
        };
        if lights[k].replace(light).is_some() {
            return Err(bad(&format!("{} twice", RIG_LIGHTS[k])));
        }
    }
    let lights = lights
        .into_iter()
        .zip(RIG_LIGHTS)
        .map(|(l, name)| l.ok_or_else(|| bad(&format!("no {name}"))))
        .collect::<Result<Vec<_>>>()?;
    Ok(Rig { identity, lights })
}

#[cfg(test)]
mod tests;
