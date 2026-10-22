//! A hero's attach nodes posed by the clips playing (openspec AH.50): what
//! the scripts hang on `actorinfo.attach nodes [i]` sits where the model's
//! attach list puts node `i` on its bone, the bone posed by the clips on
//! him, the way the original's engine poses it.
//!
//! The oracle settled it (AH.50's recordings): Eruptor's throw hangs his
//! `009_LavaCollider` on `lHand` (node 7, `J_palm_L`) or `rHand` (node 8,
//! `J_palm_R`), and each frame the collider stands where that node is with
//! his locomotion's clips at their times (his idle) and the throw laid over
//! them at its time, his model's scale (1.5) taken, to 5 µm over every
//! frame of 20 throws; the ball leaves from the collider's place at the
//! start of the frame it's thrown in. The node moves up to 1.9 m a frame
//! through the throw, so the place it leaves from follows the clip's time
//! then. [`posed`] is that pose; the embedder sets it after each engine
//! frame ([`crate::scripts::ScriptWorld::set_nodes`], then
//! [`crate::scripts::ScriptWorld::place_hung`]).

use guac_ir::Model;
use guac_ir::clip::Clip;
use guac_ir::model::Skeleton;
use guac_ir::nodes::AttachNodes;
use guac_ir::palette::Palette;
use guac_motion::seam::{Quat, Vec3};
use guac_script::Handle;

use crate::script_engine::NodePose;
use crate::scripts::ScriptWorld;

/// Each of `nodes`' places and turns in the actor's own frame (metres, +Z
/// its facing, as [`ScriptWorld::set_nodes`] takes them): the model's joints
/// posed by `base` (each clip at its time in seconds and its weight: his
/// locomotion) with `over` laid on it (each clip at its time: what his
/// scripts play on him), every node on its joint at its offset, its place
/// at the actor's `scale`. A node on no joint sits at its offset from the
/// model's origin; one whose joint the skeleton lacks, at the pivot turned
/// as the actor.
pub fn posed(
    skeleton: &Skeleton,
    nodes: &AttachNodes,
    base: &[(&Clip, f64, f64)],
    over: &[(&Clip, f64)],
    scale: f32,
) -> Vec<NodePose> {
    let pose = skeleton.blend_over(base, over);
    (0..nodes.nodes.len())
        .map(|i| match nodes.transform(i, &pose) {
            Some(m) => NodePose {
                at: Vec3::from_array(m.translation) * scale,
                turn: Quat::from_array(m.rotation).normalize(),
            },
            None => NodePose::at(Vec3::ZERO),
        })
        .collect()
}

/// The clips `actor`'s scripts play on him out of `palette`, each at its
/// time this step, oldest first (a later one lies over it): every sequence
/// on him that hasn't played out or been stopped, by its palette slot.
pub fn playing<'p>(
    world: &ScriptWorld,
    actor: Handle,
    palette: &'p Palette,
) -> Vec<(&'p Clip, f64)> {
    world
        .sequences()
        .iter()
        .filter(|s| s.actor == actor && !s.done && !s.stopped && s.length > 0.0)
        .filter_map(|s| {
            let (_, slot) = world.clip_of(s)?;
            let clip = palette.clip(u16::try_from(slot).ok()?)?;
            Some((clip, f64::from(s.time)))
        })
        .collect()
}

/// What poses an actor's attach nodes where nothing picks his locomotion
/// (the headless runs): his model (its skeleton and idle), the palette his
/// scripts play from, his model's attach list and his scale.
#[derive(Debug, Clone)]
pub struct Rig {
    pub model: Model,
    pub palette: Palette,
    pub nodes: AttachNodes,
    pub scale: f32,
}

impl Rig {
    /// `actor`'s nodes set on `world`: his idle at `idle` seconds with what
    /// his scripts play on him laid over it. [`ScriptWorld::place_hung`]
    /// then sets what hangs from them there.
    pub fn pose(&self, world: &mut ScriptWorld, actor: Handle, idle: f64) {
        let over = playing(world, actor, &self.palette);
        let base = [(&self.model.idle, idle, 1.0)];
        let nodes = posed(&self.model.skeleton, &self.nodes, &base, &over, self.scale);
        world.set_nodes(actor, nodes);
    }
}

#[cfg(test)]
mod tests {
    use guac_ir::model::{Joint, Transform};
    use guac_ir::nodes::AttachNode;
    use guac_ir::{DumpIdentity, Sha256};

    use super::*;

    fn identity() -> DumpIdentity {
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([0; 32]),
        }
    }

    fn at(t: [f32; 3]) -> Transform {
        Transform {
            translation: t,
            ..Transform::IDENTITY
        }
    }

    fn joint(name: &str, parent: Option<u16>, t: [f32; 3]) -> Joint {
        Joint {
            name: name.into(),
            parent,
            reference: at(t),
            inverse_bind: [[0.0; 4]; 4],
        }
    }

    /// A root, a shoulder 1 m up and a palm 0.5 m along +Z from it; the
    /// attach list's node 0 on the palm 0.1 m further, node 1 on the
    /// model's origin and node 2 on a bone the skeleton lacks.
    fn rig() -> (Skeleton, AttachNodes) {
        let skeleton = Skeleton {
            joints: vec![
                joint("J_root", None, [0.0; 3]),
                joint("J_shoulder", Some(0), [0.0, 1.0, 0.0]),
                joint("J_palm_L", Some(1), [0.0, 0.0, 0.5]),
            ],
        };
        let node = |bone: &str, joint, t| AttachNode {
            name: None,
            bone: Some(bone.into()),
            joint,
            offset: at(t),
        };
        let nodes = AttachNodes {
            identity: identity(),
            load_file: "009_Eruptor".into(),
            nodes: vec![
                node("J_palm_L", Some(2), [0.0, 0.0, 0.1]),
                node("*root*", None, [0.0, 0.2, 0.0]),
                node("J_gone", Some(9), [1.0, 1.0, 1.0]),
            ],
        };
        (skeleton, nodes)
    }

    /// A clip of `seconds` that swings the shoulder from where it rests to
    /// 0.5 m left of it, linearly over its two frames.
    fn swing() -> Clip {
        use guac_ir::clip::{RotationTrack, Track, VecSpline, VecTrack};
        Clip {
            name: "Swing.ma".into(),
            duration: 1.0,
            frame_duration: 1.0,
            frames: 2,
            tracks: vec![Track {
                joint: 1,
                translation: VecTrack {
                    constant: [0.0, 1.0, 0.0],
                    spline: Some(VecSpline {
                        channels: [true, false, false],
                        degree: 1,
                        knots: vec![0, 0, 1, 1],
                        points: vec![[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]],
                    }),
                },
                rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
                scale: VecTrack {
                    constant: [1.0; 3],
                    spline: None,
                },
            }],
        }
    }

    #[test]
    fn a_node_rides_its_bone_through_the_clip_at_the_actors_scale() {
        let (skeleton, mut nodes) = rig();
        // The palm's node turned a quarter about +Y on it: its turn, which
        // doesn't move it.
        let s = std::f32::consts::FRAC_1_SQRT_2;
        nodes.nodes[0].offset.rotation = [0.0, s, 0.0, s];
        let clip = swing();
        let rest = posed(&skeleton, &nodes, &[], &[], 1.5);
        let to = |v: [f32; 3]| Vec3::from_array(v) * 1.5;
        assert!(
            rest[0].at.distance(to([0.0, 1.0, 0.6])) < 1e-6,
            "{:?}",
            rest[0]
        );
        let quarter = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        assert!(rest[0].turn.angle_between(quarter) < 1e-5, "{:?}", rest[0]);
        assert!(
            rest[1].at.distance(to([0.0, 0.2, 0.0])) < 1e-6,
            "{:?}",
            rest[1]
        );
        assert_eq!(
            rest[2],
            NodePose::at(Vec3::ZERO),
            "a joint the skeleton lacks: the pivot, turned as the actor"
        );
        // Half way through the swing laid over the rest, the palm 0.25 m
        // left; the node on the origin doesn't move.
        let half = posed(&skeleton, &nodes, &[], &[(&clip, 0.5)], 1.5);
        assert!(
            half[0].at.distance(to([0.25, 1.0, 0.6])) < 1e-6,
            "{:?}",
            half[0]
        );
        assert_eq!(half[1], rest[1]);
        // Under a base clip at full weight the same: the over clip tracks
        // the shoulder, so the base sets nothing it moves.
        let based = posed(
            &skeleton,
            &nodes,
            &[(&clip, 0.0, 1.0)],
            &[(&clip, 0.5)],
            1.5,
        );
        assert!(based[0].at.distance(half[0].at) < 1e-6);
        // With nothing laid over it the base clip moves the palm, at the
        // base's own time: three quarters through, 0.375 m left.
        let base_only = posed(&skeleton, &nodes, &[(&clip, 0.75, 1.0)], &[], 1.0);
        assert!(
            base_only[0].at.distance(Vec3::new(0.375, 1.0, 0.6)) < 1e-6,
            "{:?}",
            base_only[0]
        );
    }
}
