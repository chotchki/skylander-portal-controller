//! A character archive's model (openspec AE.5's rule, read here for AE.6):
//! the model the game draws for the toy on its portal screen, its skeleton
//! against Havok's, and its idle.
//!
//! - The portal model is the one `tfbPhysicsModel` a `tfbSpriteInfo` draws
//!   (through `_contextDataInfo`) with a skeleton of its own; where a skinned
//!   attachment has one too (Tree Rex's woodpecker, Eye-Brawl's eyeball),
//!   the one whose sprite is named `…Portal_Toy…`. AE.5 found it on all 67
//!   character archives.
//! - A fresh figure draws every segment of its scene graph: nothing in it
//!   switches a part off (AE.5). The models other sprites draw are
//!   [`PortalModel::attached`]; which of those a fresh figure shows is
//!   AE.20's.
//! - Joints map by name between Havok's skeleton and `igSkeleton2`'s bones
//!   ([`JointMap`]), and skin with Alchemy's inverse joints (AC.17).
//! - The idle is slot 0 of the model's animation set, a NORMAL clip that
//!   poses the whole skeleton (AE.5: 67 of 67; 62 named `<character>_Idle`).

use crate::havok::{self, BlendHint};
use crate::igz::Igz;
use crate::igz::layouts::{
    Hero, Layout, Mat44, ObjectRef, Payload, PhysicsAnimation, PhysicsModel, SkeletonBone,
    SpriteInfo, is_a, read_all,
};
use crate::{ErrorKind, FormatError};

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
#[cfg(test)]
mod tests;

const FORMAT: &str = "model";

/// What the portal sprite's name holds, any case.
pub const PORTAL_TOY: &str = "portal_toy";

/// The model a character archive draws for its toy.
#[derive(Debug, Clone, PartialEq)]
pub struct PortalModel<'a> {
    /// The `tfbSpriteInfo` and its name (`009_Portal_Toy`).
    pub sprite: usize,
    pub sprite_name: Option<&'a str>,
    /// The `tfbPhysicsModel`, walked.
    pub model: usize,
    pub hero: Hero<'a>,
    /// Every other sprite and the `tfbPhysicsModel` it draws, skinned or not,
    /// in sprite order.
    pub attached: Vec<(usize, usize)>,
}

impl PortalModel<'_> {
    /// The model's file name without its folders (`Eruptor.mdl`).
    pub fn name(&self) -> Option<&str> {
        self.hero
            .model
            .model
            .info
            .name
            .map(|n| n.rsplit(['/', '\\']).next().unwrap_or(n))
    }
}

/// The one sprite-drawn model with a skeleton, or the Portal_Toy sprite's
/// when attachments have skeletons too; none, or a tie, is refused.
pub fn portal_model<'a>(igz: &Igz<'a>) -> Result<PortalModel<'a>, FormatError> {
    let mut heroes = Vec::new();
    let mut attached = Vec::new();
    for (sprite, s) in read_all::<SpriteInfo>(igz) {
        let s = s?;
        let ObjectRef::Object(m) = s.context_data_info else {
            continue;
        };
        if !is_a(igz.type_name(&igz.objects()[m]), PhysicsModel::TYPE) {
            continue;
        }
        match Hero::read(igz, m) {
            Ok(hero) => heroes.push((sprite, s.placement.name, m, hero)),
            Err(_) => attached.push((sprite, m)),
        }
    }
    let k = match heroes.len() {
        0 => {
            return Err(FormatError::new(
                FORMAT,
                "a sprite-drawn model with a skeleton",
                0,
                ErrorKind::Missing,
            ));
        }
        1 => 0,
        n => {
            let named: Vec<usize> = (0..n)
                .filter(|&k| {
                    heroes[k]
                        .1
                        .is_some_and(|name| name.to_ascii_lowercase().contains(PORTAL_TOY))
                })
                .collect();
            match named[..] {
                [k] => k,
                _ => {
                    return Err(FormatError::new(
                        FORMAT,
                        "a Portal_Toy sprite's model",
                        0,
                        ErrorKind::CountMismatch {
                            found: named.len() as u64,
                            expected: 1,
                        },
                    ));
                }
            }
        }
    };
    let (sprite, sprite_name, model, hero) = heroes.swap_remove(k);
    attached.extend(heroes.into_iter().map(|(s, _, m, _)| (s, m)));
    attached.sort_unstable();
    Ok(PortalModel {
        sprite,
        sprite_name,
        model,
        hero,
        attached,
    })
}

/// The hero body's Havok skeleton: exactly one `hkaSkeleton`.
pub fn skeleton<'a>(igz: &Igz<'a>, hero: &Hero<'a>) -> Result<havok::Skeleton<'a>, FormatError> {
    let at = igz.objects()[hero.body_index].at;
    let Some(Payload::Havok(bytes)) = hero.body.body.data_info.payload() else {
        return Err(igz.err_at(at, "_data", ErrorKind::Missing));
    };
    let mut skeletons = havok::Container::parse(bytes)?.skeletons;
    match skeletons.len() {
        1 => Ok(skeletons.remove(0)),
        n => Err(igz.err_at(
            at,
            "hkaSkeleton",
            ErrorKind::CountMismatch {
                found: n as u64,
                expected: 1,
            },
        )),
    }
}

/// Havok's joints against `igSkeleton2`'s bones, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JointMap {
    /// Alchemy bone -> Havok joint; `None` for a bone Havok lacks (the
    /// unnamed root every hero has).
    pub joint_of_bone: Vec<Option<usize>>,
    /// Indexed by Havok joint, the order clips and skins use.
    pub bone_of_joint: Vec<usize>,
}

fn joint_error(field: &'static str, kind: ErrorKind) -> FormatError {
    FormatError::new(FORMAT, field, 0, kind)
}

impl JointMap {
    /// Every Havok joint must name exactly one Alchemy bone.
    pub fn new(
        skeleton: &havok::Skeleton<'_>,
        bones: &[SkeletonBone<'_>],
    ) -> Result<Self, FormatError> {
        let mut joint_of_bone = vec![None; bones.len()];
        let mut bone_of_joint = Vec::with_capacity(skeleton.bones.len());
        for (j, b) in skeleton.bones.iter().enumerate() {
            let name = b
                .name
                .ok_or_else(|| joint_error("hkaBone name", ErrorKind::Missing))?;
            let mut found = bones
                .iter()
                .enumerate()
                .filter(|(_, a)| a.name == Some(name));
            let bone = match (found.next(), found.next()) {
                (Some((i, _)), None) => i,
                (None, _) => return Err(joint_error("igSkeletonBone name", ErrorKind::Missing)),
                (Some(_), Some(_)) => {
                    return Err(joint_error("igSkeletonBone name", ErrorKind::Duplicate));
                }
            };
            joint_of_bone[bone] = Some(j);
            bone_of_joint.push(bone);
        }
        Ok(Self {
            joint_of_bone,
            bone_of_joint,
        })
    }

    /// Each joint's Alchemy inverse joint, through its bone's
    /// `_blendMatrixIndex`: as stored, row-vector with the translation in
    /// row 3.
    pub fn inverse_joints(
        &self,
        bones: &[SkeletonBone<'_>],
        inverse_joints: &[Mat44],
    ) -> Result<Vec<Mat44>, FormatError> {
        self.bone_of_joint
            .iter()
            .map(|&b| {
                let blend = bones[b].blend_matrix_index;
                usize::try_from(blend)
                    .ok()
                    .and_then(|i| inverse_joints.get(i))
                    .copied()
                    .ok_or_else(|| {
                        joint_error(
                            "_blendMatrixIndex",
                            ErrorKind::OutOfRange {
                                value: u64::from(blend as u32),
                                limit: inverse_joints.len() as u64,
                            },
                        )
                    })
            })
            .collect()
    }
}

/// Slot 0 of the animation set, checked to pose the skeleton ([`idle`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Idle<'a> {
    /// The `AnimationInfo` in slot 0 and the `tfbPhysicsAnimation` it wraps.
    pub info: usize,
    pub animation: usize,
    /// The clip's file name without its folders (`Eruptor_Idle.ma`).
    pub file: Option<&'a str>,
    pub clip: havok::Clip<'a>,
}

/// Slot 0 of `hero`'s animation set: a NORMAL clip, neither additive nor
/// partial, that poses `skeleton` at its start and halfway.
pub fn idle<'a>(
    igz: &Igz<'a>,
    hero: &Hero<'a>,
    skeleton: &havok::Skeleton<'_>,
) -> Result<Idle<'a>, FormatError> {
    let model_at = igz.objects()[hero.model_index].at;
    let info = hero
        .clip_slots
        .first()
        .copied()
        .flatten()
        .ok_or_else(|| igz.err_at(model_at, "_animationSet slot 0", ErrorKind::Missing))?;
    let at = igz.objects()[info].at;
    let (animation, a): (usize, PhysicsAnimation<'a>) = hero.clips[&info]
        .animation
        .ok_or_else(|| igz.err_at(at, "_tfbAnimation", ErrorKind::Missing))?;
    let anim_at = igz.objects()[animation].at;
    if a.info.additive_blend() || a.info.partial_animation() {
        return Err(igz.err_at(
            anim_at,
            "_properties",
            ErrorKind::Unsupported {
                value: a.info.properties.into(),
            },
        ));
    }
    let Some(Payload::Havok(bytes)) = a.info.data_info.payload() else {
        return Err(igz.err_at(anim_at, "_data", ErrorKind::Missing));
    };
    let clip = havok::Container::parse(bytes)?
        .clip
        .ok_or_else(|| igz.err_at(anim_at, "hkaAnimationBinding", ErrorKind::Missing))?;
    if clip.binding.blend_hint != BlendHint::Normal {
        return Err(igz.err_at(anim_at, "blendHint", ErrorKind::Unsupported { value: 1 }));
    }
    for t in [0.0, f64::from(clip.animation.duration) / 2.0] {
        let posed = clip
            .local_pose(skeleton, t)
            .and_then(|p| skeleton.model_pose(&p));
        if posed.is_none() {
            return Err(igz.err_at(
                anim_at,
                "transformTrackToBoneIndices",
                ErrorKind::OutOfRange {
                    value: clip.binding.track_to_bone.len() as u64,
                    limit: skeleton.bones.len() as u64,
                },
            ));
        }
    }
    let file = a
        .info
        .data_info
        .info
        .name
        .map(|n| n.rsplit(['/', '\\']).next().unwrap_or(n));
    Ok(Idle {
        info,
        animation,
        file,
        clip,
    })
}
