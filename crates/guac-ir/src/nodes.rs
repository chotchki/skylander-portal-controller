//! A character model's attach nodes (openspec AH.50): the list a script
//! indexes as `actorinfo.attach nodes`, in the model's own order
//! (`tfbModelInfo._attachList`), each a bone of the model's skeleton and an
//! offset from it. Eruptor's `lHand` (7) and `rHand` (8) sit on `J_palm_L`
//! and `J_palm_R`; his throw hangs its collider on one and the ball leaves
//! from it. The engine poses a node by the clips playing, as it poses the
//! bone: [`AttachNodes::place`].
//!
//! One file per character load file, `nodes/<load file>.<ron|postcard>`:
//! a new file beside unchanged ones, so [`crate::IR_VERSION`] holds.

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;
use crate::model::Transform;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachNodes {
    pub identity: DumpIdentity,
    /// The roster's load file (`009_Eruptor`).
    pub load_file: String,
    /// In the attach list's order: a script's index names one.
    pub nodes: Vec<AttachNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachNode {
    /// The node's own name (`lHand`), if it has one.
    pub name: Option<String>,
    /// The bone it follows (`J_palm_L`), or `*root*`.
    pub bone: Option<String>,
    /// That bone's joint in the model's skeleton; `None` on the model's own
    /// origin (`*root*`), or a bone the skeleton lacks.
    pub joint: Option<u16>,
    /// The offset from the bone: the attach point's position and its Euler
    /// angles as a rotation, scale 1.
    pub offset: Transform,
}

impl AttachNodes {
    /// Where node `i` sits in the model's space with its joints at `pose`
    /// (model space, as [`crate::model::Skeleton::blend_over`] gives them):
    /// its offset composed onto its joint's transform; one on the model's
    /// origin at its offset from it. `None` past the list, or a joint past
    /// the pose.
    pub fn place(&self, i: usize, pose: &[Transform]) -> Option<[f32; 3]> {
        self.transform(i, pose).map(|m| m.translation)
    }

    /// Node `i`'s whole transform in the model's space with its joints at
    /// `pose`: [`AttachNodes::place`]'s, its turn as well (what hangs from it
    /// is turned so, AH.60).
    pub fn transform(&self, i: usize, pose: &[Transform]) -> Option<Transform> {
        let node = self.nodes.get(i)?;
        match node.joint {
            Some(j) => Some(pose.get(usize::from(j))?.then(&node.offset)),
            None => Some(node.offset),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sha256;
    use crate::model::{Joint, Skeleton};

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

    /// A root and an arm turned 90° about +Y, a hand node on the arm 1 m
    /// along its local +Z and one on the model's origin.
    fn nodes() -> AttachNodes {
        AttachNodes {
            identity: identity(),
            load_file: "009_Eruptor".into(),
            nodes: vec![
                AttachNode {
                    name: Some("origin".into()),
                    bone: Some("*root*".into()),
                    joint: None,
                    offset: at([0.0, 0.5, 0.0]),
                },
                AttachNode {
                    name: Some("lHand".into()),
                    bone: Some("J_arm".into()),
                    joint: Some(1),
                    offset: at([0.0, 0.0, 1.0]),
                },
            ],
        }
    }

    #[test]
    fn a_node_follows_its_joint_and_one_on_the_origin_holds_its_offset() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let skeleton = Skeleton {
            joints: vec![
                Joint {
                    name: "J_root".into(),
                    parent: None,
                    reference: at([0.0, 1.0, 0.0]),
                    inverse_bind: [[0.0; 4]; 4],
                },
                Joint {
                    name: "J_arm".into(),
                    parent: Some(0),
                    reference: Transform {
                        translation: [0.0, 0.0, 0.5],
                        rotation: [0.0, s, 0.0, s],
                        scale: [1.0; 3],
                    },
                    inverse_bind: [[0.0; 4]; 4],
                },
            ],
        };
        let pose = skeleton.reference_pose();
        let n = nodes();
        let hand = n.place(1, &pose).unwrap();
        // The arm at (0, 1, 0.5), turned so its +Z points along +X.
        for (got, want) in hand.iter().zip([1.0, 1.0, 0.5]) {
            assert!((got - want).abs() < 1e-6, "{hand:?}");
        }
        assert_eq!(n.place(0, &pose), Some([0.0, 0.5, 0.0]));
        // Its turn is its joint's: the arm's quarter turn about +Y.
        let turn = n.transform(1, &pose).unwrap().rotation;
        for (got, want) in turn.iter().zip([0.0, s, 0.0, s]) {
            assert!((got - want).abs() < 1e-6, "{turn:?}");
        }
        assert_eq!(n.place(2, &pose), None, "past the list");
        assert_eq!(n.place(1, &pose[..1]), None, "a joint past the pose");
    }

    #[test]
    fn the_list_round_trips_through_both_formats() {
        let n = nodes();
        let ron = crate::to_ron(&n).unwrap();
        assert_eq!(crate::from_ron::<AttachNodes>(&ron).unwrap(), n);
        let bytes = crate::to_postcard(&n).unwrap();
        assert_eq!(crate::from_postcard::<AttachNodes>(&bytes).unwrap(), n);
    }
}
