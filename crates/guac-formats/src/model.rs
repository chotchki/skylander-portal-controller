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
//! - The locomotion clips (AG.14) are the set's other fixed slots: walk 1,
//!   jog 2, run 3 and falling 57, the numbers `permanent/global.bld`'s
//!   `AnimSlot_*` values give and its locomotion macros play from, and the
//!   landing 61, which the macro plays by number as a hero touches down. An
//!   empty slot is a clip the character doesn't have.
//! - The hero is the placement drawing the portal model with a solid that
//!   blocks and meets the level ([`hero_actor`]), which carries the
//!   character's `CollisionInfo` and its disc `ActorParameters`.
//! - The model a figure shows is the one the game draws for it in play
//!   ([`in_play`], AE.23): the hero's, which is the portal model on 66 of
//!   the 67 archives. Punch Pop Fizz's hero draws `Alchemist_Legendary.mdl`
//!   where her portal sprite draws `AlchemistRed.mdl`, and the oracle's
//!   sweep (AE.22) shows the first in play.

use crate::havok::{self, BlendHint};
use crate::igz::Igz;
use crate::igz::layouts::{
    Hero, Layout, Mat44, ObjectRef, Payload, PhysicsAnimation, PhysicsModel, SkeletonBone,
    SpriteInfo, is_a, read_all,
};
use crate::level::{Actor, Placements};
use crate::{ErrorKind, FormatError};

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
#[cfg(test)]
mod tests;

const FORMAT: &str = "model";

/// What the portal sprite's name holds, any case.
pub const PORTAL_TOY: &str = "portal_toy";

/// The animation set's slots: the idle, and the locomotion's by
/// `global.bld`'s `AnimSlot_Idle`, `_Walk`, `_Jog`, `_Run` and `_Fall`
/// (0, 1, 2, 3 and 57). The landing is slot 61, played by number.
pub const IDLE_SLOT: usize = 0;
pub const WALK_SLOT: usize = 1;
pub const JOG_SLOT: usize = 2;
pub const RUN_SLOT: usize = 3;
pub const FALL_SLOT: usize = 57;
pub const LANDING_SLOT: usize = 61;

/// A `CollisionInfo` that blocks (bit 3) and meets the level (bit 4): the
/// hero's own, not a clone's (0x12), an upgrade avatar's (0x4) or a
/// trigger's.
pub const HERO_SOLID: u32 = 0x18;

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

/// `portal`'s character as the game draws it in play: its hero's model
/// when that's another one with a skeleton of its own, walked as the portal
/// model is; otherwise `portal` as it is (no hero found, or one drawing the
/// portal model). A hero model that doesn't walk is refused.
pub fn in_play<'a>(
    igz: &Igz<'a>,
    portal: PortalModel<'a>,
    load_file: &str,
) -> Result<PortalModel<'a>, FormatError> {
    let Ok(actor) = hero_actor(igz, &portal, load_file) else {
        return Ok(portal);
    };
    match actor.info.model {
        Some(m) if m != portal.model => Ok(PortalModel {
            model: m,
            hero: Hero::read(igz, m)?,
            ..portal
        }),
        _ => Ok(portal),
    }
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
    /// Every Havok joint must name exactly one Alchemy bone, and no two
    /// joints the same one (`cargo fuzz` found a pair that did: the second
    /// overwrote the first and the map stopped being one to one).
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
            if joint_of_bone[bone].replace(j).is_some() {
                return Err(joint_error("hkaBone name", ErrorKind::Duplicate));
            }
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

/// One animation-set slot's clip, checked to pose the skeleton
/// ([`slot_clip`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SlotClip<'a> {
    pub slot: usize,
    /// The `AnimationInfo` in the slot and the `tfbPhysicsAnimation` it
    /// wraps.
    pub info: usize,
    pub animation: usize,
    /// The clip's file name without its folders (`Eruptor_Idle.ma`).
    pub file: Option<&'a str>,
    /// The entry's own label (`_mediaName`), which some locomotion slots
    /// use for their authored speed (`3.658 m/sec` on Eruptor's walk).
    pub label: Option<&'a str>,
    pub clip: havok::Clip<'a>,
}

/// Slot 0 of the animation set ([`idle`]).
pub type Idle<'a> = SlotClip<'a>;

/// The clip in `slot` of `hero`'s animation set, `None` when the slot is
/// empty or past the set's end. Refused: an entry wrapping no
/// `tfbPhysicsAnimation` (an animation network or nothing), a clip that is
/// additive or partial or not a NORMAL blend, and one that doesn't pose
/// `skeleton` at its start and halfway.
pub fn slot_clip<'a>(
    igz: &Igz<'a>,
    hero: &Hero<'a>,
    skeleton: &havok::Skeleton<'_>,
    slot: usize,
) -> Result<Option<SlotClip<'a>>, FormatError> {
    let Some(info) = hero.clip_slots.get(slot).copied().flatten() else {
        return Ok(None);
    };
    let at = igz.objects()[info].at;
    let entry = &hero.clips[&info];
    let (animation, a): (usize, PhysicsAnimation<'a>) = entry
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
    let clip = decode_clip(igz, animation, &a)?;
    if clip.binding.blend_hint != BlendHint::Normal {
        return Err(igz.err_at(anim_at, "blendHint", ErrorKind::Unsupported { value: 1 }));
    }
    poses(igz, animation, &clip, skeleton)?;
    Ok(Some(SlotClip {
        slot,
        info,
        animation,
        file: clip_file(&a),
        label: entry.info.media_name,
        clip,
    }))
}

/// Refused when the wrapper's `_data` holds no Havok payload, or the payload
/// no clip with its binding.
fn decode_clip<'a>(
    igz: &Igz<'a>,
    animation: usize,
    a: &PhysicsAnimation<'a>,
) -> Result<havok::Clip<'a>, FormatError> {
    let anim_at = igz.objects()[animation].at;
    let Some(Payload::Havok(bytes)) = a.info.data_info.payload() else {
        return Err(igz.err_at(anim_at, "_data", ErrorKind::Missing));
    };
    havok::Container::parse(bytes)?
        .clip
        .ok_or_else(|| igz.err_at(anim_at, "hkaAnimationBinding", ErrorKind::Missing))
}

/// Refused unless a NORMAL clip poses `skeleton` at its start and halfway:
/// every track bound to one of its bones.
fn poses(
    igz: &Igz<'_>,
    animation: usize,
    clip: &havok::Clip<'_>,
    skeleton: &havok::Skeleton<'_>,
) -> Result<(), FormatError> {
    for t in [0.0, f64::from(clip.animation.duration) / 2.0] {
        let posed = clip
            .local_pose(skeleton, t)
            .and_then(|p| skeleton.model_pose(&p));
        if posed.is_none() {
            return Err(igz.err_at(
                igz.objects()[animation].at,
                "transformTrackToBoneIndices",
                ErrorKind::OutOfRange {
                    value: clip.binding.track_to_bone.len() as u64,
                    limit: skeleton.bones.len() as u64,
                },
            ));
        }
    }
    Ok(())
}

fn clip_file<'a>(a: &PhysicsAnimation<'a>) -> Option<&'a str> {
    a.info
        .data_info
        .info
        .name
        .map(|n| n.rsplit(['/', '\\']).next().unwrap_or(n))
}

/// One slot of an animation set as the palette keeps it (openspec AH.22):
/// whatever the slot holds, where [`slot_clip`] takes only a full NORMAL
/// clip. A script plays a slot by number (`animation palette [20]`), so the
/// palette is every slot, additive and partial clips among them (Eruptor's
/// throw, slots 20 and 21, is partial).
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteSlot<'a> {
    pub slot: usize,
    /// The `AnimationInfo` in the slot.
    pub info: usize,
    /// The entry's `_name` and its label (`_mediaName`).
    pub name: Option<&'a str>,
    pub label: Option<&'a str>,
    /// `None` for an entry wrapping no `tfbPhysicsAnimation`: an animation
    /// network, or nothing.
    pub clip: Option<PaletteClip<'a>>,
}

/// A `tfbPhysicsAnimation`'s clip, with the wrapper's flags (additive,
/// partial, layered, motion extraction) it's played by.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteClip<'a> {
    pub animation: usize,
    pub flags: crate::igz::layouts::TfbAnimationInfo<'a>,
    /// The clip's file name without its folders.
    pub file: Option<&'a str>,
    pub clip: havok::Clip<'a>,
}

/// The entry in `slot` of `hero`'s animation set, `None` when the slot is
/// empty or past the set's end. Refused: a clip whose binding's blend hint
/// disagrees with the wrapper's additive flag, and a NORMAL one (partial
/// or not) that doesn't pose `skeleton` at its start and halfway; an
/// additive clip's tracks must each name one of its bones.
pub fn palette_slot<'a>(
    igz: &Igz<'a>,
    hero: &Hero<'a>,
    skeleton: &havok::Skeleton<'_>,
    slot: usize,
) -> Result<Option<PaletteSlot<'a>>, FormatError> {
    let Some(info) = hero.clip_slots.get(slot).copied().flatten() else {
        return Ok(None);
    };
    let entry = &hero.clips[&info];
    let slot_of = |clip| PaletteSlot {
        slot,
        info,
        name: entry.info.name,
        label: entry.info.media_name,
        clip,
    };
    let Some((animation, a)) = entry.animation else {
        return Ok(Some(slot_of(None)));
    };
    let clip = decode_clip(igz, animation, &a)?;
    let additive = clip.binding.blend_hint == BlendHint::Additive;
    if additive != a.info.additive_blend() {
        return Err(igz.err_at(
            igz.objects()[animation].at,
            "blendHint",
            ErrorKind::Unsupported {
                value: u64::from(additive),
            },
        ));
    }
    if additive {
        let bones = skeleton.bones.len();
        if let Some(&b) = clip
            .binding
            .track_to_bone
            .iter()
            .find(|&&b| usize::from(b) >= bones)
        {
            return Err(igz.err_at(
                igz.objects()[animation].at,
                "transformTrackToBoneIndices",
                ErrorKind::OutOfRange {
                    value: u64::from(b),
                    limit: bones as u64,
                },
            ));
        }
    } else {
        poses(igz, animation, &clip, skeleton)?;
    }
    Ok(Some(slot_of(Some(PaletteClip {
        animation,
        flags: a.info,
        file: clip_file(&a),
        clip,
    }))))
}

/// Slot 0 of `hero`'s animation set: a NORMAL clip, neither additive nor
/// partial, that poses `skeleton` at its start and halfway.
pub fn idle<'a>(
    igz: &Igz<'a>,
    hero: &Hero<'a>,
    skeleton: &havok::Skeleton<'_>,
) -> Result<Idle<'a>, FormatError> {
    slot_clip(igz, hero, skeleton, IDLE_SLOT)?.ok_or_else(|| {
        let model_at = igz.objects()[hero.model_index].at;
        igz.err_at(model_at, "_animationSet slot 0", ErrorKind::Missing)
    })
}

/// The placement the archive plays `portal`'s character as: the one actor
/// drawing the portal model whose solid blocks and meets the level
/// ([`HERO_SOLID`]); where no actor draws it, the one named as the load file,
/// any case, with such a solid (Punch Pop Fizz's archive, whose actor draws
/// another model than her portal sprite's). None, or more than one, is
/// refused.
pub fn hero_actor<'a>(
    igz: &Igz<'a>,
    portal: &PortalModel<'a>,
    load_file: &str,
) -> Result<Actor<'a>, FormatError> {
    let placements = Placements::read(igz)?;
    let solid = |a: &Actor<'_>| {
        a.collision
            .is_some_and(|c| c.properties & HERO_SOLID == HERO_SOLID)
    };
    let pick = |by: &dyn Fn(&Actor<'a>) -> bool| -> Vec<Actor<'a>> {
        placements
            .actors
            .iter()
            .filter(|a| solid(a) && by(a))
            .copied()
            .collect()
    };
    let mut found = pick(&|a| a.info.model == Some(portal.model));
    if found.is_empty() {
        found = pick(&|a| a.name().is_some_and(|n| n.eq_ignore_ascii_case(load_file)));
    }
    match found[..] {
        [actor] => Ok(actor),
        _ => Err(FormatError::new(
            FORMAT,
            "the hero's tfbActorInfo",
            0,
            ErrorKind::CountMismatch {
                found: found.len() as u64,
                expected: 1,
            },
        )),
    }
}
