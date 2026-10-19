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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use guac_formats::FormatError;
use guac_formats::edge::{BoneMap, Segment as EdgeSegment, Semantic};
use guac_formats::havok::spline::{Channel, QuatTrack, VecTrack as HavokVecTrack};
use guac_formats::havok::{self, QsTransform};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{DirectionalLightInfo, TextureAttr2, read_all};
use guac_formats::material::{self, Blend as FBlend, Role};
use guac_formats::model::{self as fmodel, JointMap};
use guac_formats::texture::Texture;
use guac_ir::clip::{Clip, RotationTrack, Track, VecSpline, VecTrack};
use guac_ir::model::{
    Blend, Filter, Joint, Mask, Material, Params, RenderStates, Sampler, Scroll, Segment, Skeleton,
    TextureRole, TextureSlot, Transform, Wrap,
};
use guac_ir::{Category, DumpIdentity, Model, Rig, RigLight, Roster, Sha256};
use sha2::Digest;

use crate::roster::{GLOBAL, LEVEL};
use crate::{ExtractError, Result};

/// Where models go in the extract, one `<load file>.<ron|postcard>` each.
pub const MODEL_DIR: &str = "models";
/// Where textures go, one `<sha256>.ktx2` per distinct file.
pub const TEXTURE_DIR: &str = "textures";
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

/// The model `load_file`'s archive level holds.
pub fn read(level: &[u8], load_file: &str, identity: DumpIdentity) -> Result<Extracted> {
    let bad = |e: FormatError| refused(load_file, e);
    let igz = Igz::parse(level).map_err(bad)?;
    let portal = fmodel::portal_model(&igz).map_err(bad)?;
    let hero = &portal.hero;
    let havok_skeleton = fmodel::skeleton(&igz, hero).map_err(bad)?;
    let joints = JointMap::new(&havok_skeleton, &hero.bones).map_err(bad)?;
    let inverse = joints
        .inverse_joints(&hero.bones, &hero.skeleton.inverse_joints)
        .map_err(bad)?;
    let skeleton = skeleton(&havok_skeleton, &inverse).map_err(|e| refused(load_file, e))?;
    let idle = fmodel::idle(&igz, hero, &havok_skeleton).map_err(bad)?;
    let idle = clip(&idle.clip, idle.file.unwrap_or_default());

    let mut textures: Vec<(String, Vec<u8>)> = Vec::new();
    let mut by_image: BTreeMap<usize, String> = BTreeMap::new();
    let mut binds = 0;
    let mut materials = Vec::with_capacity(hero.meshes.len());
    let mut segments = Vec::new();
    for mesh in &hero.meshes {
        let m = material::Material::read(&igz, hero, mesh).map_err(bad)?;
        binds += m.textures.len();
        let mut slots = Vec::with_capacity(m.textures.len());
        for t in &m.textures {
            let path = match by_image.get(&t.image) {
                Some(p) => p.clone(),
                None => {
                    let ktx2 = Texture::read(&igz, t.image).map_err(bad)?.to_ktx2();
                    let path = texture_path(&ktx2);
                    if !textures.iter().any(|(p, _)| *p == path) {
                        textures.push((path.clone(), ktx2));
                    }
                    by_image.insert(t.image, path.clone());
                    path
                }
            };
            slots.push(TextureSlot {
                role: role(t.role),
                texture: path,
                image: t.image_name.unwrap_or_default().to_owned(),
                sampler: sampler(&t.sampler).map_err(|e| refused(load_file, e))?,
            });
        }
        let index =
            u16::try_from(materials.len()).map_err(|_| refused(load_file, "65,536 materials"))?;
        materials.push(ir_material(&m, slots));

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
    Ok(Extracted {
        model: Model {
            identity,
            load_file: load_file.to_owned(),
            name: portal.name().unwrap_or_default().to_owned(),
            skeleton,
            materials,
            segments,
            idle,
        },
        textures,
        binds,
    })
}

/// `textures/<sha256 of the KTX2>.ktx2`: equal files share one name.
pub fn texture_path(ktx2: &[u8]) -> String {
    let sha = Sha256(sha2::Sha256::digest(ktx2).into());
    format!("{TEXTURE_DIR}/{sha}.ktx2")
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
            glow: p.glow,
        },
        states: RenderStates {
            blend: match m.states.blend {
                FBlend::Opaque => Blend::Opaque,
                FBlend::Alpha => Blend::Alpha,
                FBlend::Additive => Blend::Additive,
            },
            depth_write: m.states.depth_write,
            two_sided: m.states.two_sided,
            alpha_test: m.states.alpha_test,
        },
        scrolls: m
            .scrolls
            .iter()
            .map(|s| Scroll {
                layer: s.layer,
                scale: s.data.scale,
                velocity: s.data.velocity,
                amplitude: s.data.amplitude,
                frequency: s.data.frequency,
                phase: s.data.phase,
                rotation_phase: s.data.rotation_phase,
            })
            .collect(),
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
    let normals = fixed(need(Semantic::Normal)?, Semantic::Normal)?;
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
    let skin = s.skin.as_ref().ok_or("unskinned")?;
    let influences = skin.bones(bone_map).map_err(|e| e.to_string())?;
    let mut joint_indices = Vec::with_capacity(n);
    let mut weights = Vec::with_capacity(n);
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
