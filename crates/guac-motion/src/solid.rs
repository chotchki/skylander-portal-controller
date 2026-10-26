//! Collision shapes from a `CollisionInfo` (openspec AG.16, design decision 4;
//! ag-motion section 1's table, checked live by AG.7 against 31 phantoms to
//! 9.65e-7 m and by AG.8 on Eruptor's sphere).
//!
//! The original builds each primitive from full extents in feet times the
//! placement's scale, 0.1 m smaller all round: a box or cylinder keeps 0.05 m
//! of that as Havok's convex radius (so its outer size is the authored one), a
//! sphere or capsule doesn't (so it's 0.05 m smaller than authored). No size
//! goes under 0.001 m. The pivot is at the bottom centre: the shape sits half
//! its height up (half its width for a sphere) along its own up, which its own
//! angles turn, plus the `CollisionInfo`'s own offset times the scale; the
//! placement's turn and place then pose the lot (AG.170: a capsule tipped 90°
//! onto its side, as four characters' are, lies with its middle at the
//! offset's height, AE.22's live read of every figure to 2e-7 m, where the
//! half height straight up stood it 1.5-1.8 m too high).
//!
//! The level's triangle mesh carries a 0.05 m radius of its own (AG.7: the
//! live `tfbSimpleMeshShape`'s), which parry's meshes can't. So a hero's world
//! holds the level's triangles as they are and every other solid 0.05 m
//! smaller ([`HavokSolid::in_world`]), and the hero queries it with his shape
//! 0.05 m larger ([`HavokSolid::grown`]): the same sums, so he meets each where
//! the original meets it (AG.9: his pivot rides 5 mm above the triangles, the
//! proxy's plane distance 5 mm too). A sphere or capsule under 0.051 m in
//! radius can't shrink by the whole 0.05 m, so the floor keeps it 0.001 m and
//! it stops a hero up to 0.049 m early: none is in level 213's reach (AG.7's
//! solids are 0.5 m and over).

use crate::seam::{Pose, Quat, Shape, Vec3};
use crate::tuning::FOOT;

/// Metres off every authored size: the 0.05 m all round the proxy's keep
/// distance and convex radius live in.
pub const SHRINK: f32 = 0.1;
/// Havok's convex radius on a box or cylinder.
pub const CONVEX_RADIUS: f32 = 0.05;
/// The level mesh's radius (AG.7, live).
pub const MESH_RADIUS: f32 = 0.05;
/// The smallest size the original builds.
pub const MIN_SIZE: f32 = 0.001;

/// A `CollisionInfo`'s primitive, its properties' bits 0-2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolidKind {
    /// No shape: the original's shared 1 m sphere, never in the world (AG.7).
    None,
    Box,
    Capsule,
    Sphere,
    Cylinder,
    /// None on level 213; built as `None` (ag-motion, a hypothesis until a
    /// level with one is read).
    Mesh,
}

/// A `CollisionInfo` as stored: sizes in feet before the scale, the offset in
/// metres before it, angles in degrees `[tilt, heading, roll]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidInfo {
    pub kind: SolidKind,
    pub offset: [f32; 3],
    pub rotation: [f32; 3],
    pub width: f32,
    pub height: f32,
    pub length: f32,
}

/// The Havok shape the original builds, posed in its placement's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HavokSolid {
    pub shape: Shape,
    /// The shape's centre from the pivot.
    pub centre: Vec3,
    pub rotation: Quat,
}

/// The rotation Euler angles `[tilt, heading, roll]` in degrees stand for:
/// the roll about Z, the tilt about X negated, then the heading about Y, so a
/// heading θ faces (sin θ, 0, cos θ) (AF.9's convention, as
/// `guac_formats::level::euler_rows` reads it).
pub fn euler(rotation: [f32; 3]) -> Quat {
    let [tilt, heading, roll] = rotation.map(f32::to_radians);
    Quat::from_rotation_y(heading) * Quat::from_rotation_x(-tilt) * Quat::from_rotation_z(roll)
}

fn floored(v: f32) -> f32 {
    v.max(MIN_SIZE)
}

impl SolidInfo {
    /// The original's shape at `scale` percent per axis, `None` for a solid
    /// that stops nothing (no shape, or a mesh).
    pub fn havok(&self, scale: [f32; 3]) -> Option<HavokSolid> {
        let s = scale.map(|p| p / 100.0);
        let (w, h, l) = (
            self.width * FOOT * s[0],
            self.height * FOOT * s[1],
            self.length * FOOT * s[2],
        );
        let (shape, up) = match self.kind {
            SolidKind::None | SolidKind::Mesh => return None,
            SolidKind::Box => (
                Shape::Box {
                    half_extents: Vec3::new(w - SHRINK, h - SHRINK, l - SHRINK)
                        .map(|v| floored(v / 2.0)),
                    margin: CONVEX_RADIUS,
                },
                h / 2.0,
            ),
            SolidKind::Capsule => (
                Shape::Capsule {
                    radius: floored((w - SHRINK) / 2.0),
                    half_segment: floored((h - w) / 2.0),
                },
                h / 2.0,
            ),
            SolidKind::Sphere => (
                Shape::Sphere {
                    radius: floored((w - SHRINK) / 2.0),
                },
                w / 2.0,
            ),
            SolidKind::Cylinder => (
                Shape::Cylinder {
                    radius: floored((w - SHRINK) / 2.0),
                    half_height: floored((h - SHRINK) / 2.0),
                    margin: CONVEX_RADIUS,
                },
                h / 2.0,
            ),
        };
        let offset = Vec3::from(self.offset) * Vec3::from(s);
        let rotation = euler(self.rotation);
        Some(HavokSolid {
            shape,
            centre: rotation * Vec3::new(0.0, up, 0.0) + offset,
            rotation,
        })
    }
}

/// `shape` larger all round by `d` (smaller for a negative `d`), its sizes
/// floored at [`MIN_SIZE`]: a box's or cylinder's convex radius takes the
/// change first.
pub fn inflate(shape: Shape, d: f32) -> Shape {
    match shape {
        Shape::Sphere { radius } => Shape::Sphere {
            radius: floored(radius + d),
        },
        Shape::Capsule {
            radius,
            half_segment,
        } => Shape::Capsule {
            radius: floored(radius + d),
            half_segment,
        },
        Shape::Box {
            half_extents,
            margin,
        } => {
            let m = margin + d;
            if m >= 0.0 {
                Shape::Box {
                    half_extents,
                    margin: m,
                }
            } else {
                Shape::Box {
                    half_extents: half_extents.map(|v| floored(v + m)),
                    margin: 0.0,
                }
            }
        }
        Shape::Cylinder {
            radius,
            half_height,
            margin,
        } => {
            let m = margin + d;
            if m >= 0.0 {
                Shape::Cylinder {
                    radius,
                    half_height,
                    margin: m,
                }
            } else {
                Shape::Cylinder {
                    radius: floored(radius + m),
                    half_height: floored(half_height + m),
                    margin: 0.0,
                }
            }
        }
    }
}

impl HavokSolid {
    /// The shape a hero's world holds for this solid: [`MESH_RADIUS`] smaller.
    pub fn in_world(&self) -> Shape {
        inflate(self.shape, -MESH_RADIUS)
    }

    /// A hero's query shape against a world built by [`HavokSolid::in_world`],
    /// `skin` past where the original's shapes would touch.
    pub fn grown(&self, skin: f32) -> Shape {
        inflate(self.shape, MESH_RADIUS + skin)
    }

    /// Its pose for a placement at `placement`: the centre is in the
    /// placement's frame, so the placement turns it.
    pub fn pose(&self, placement: Pose) -> Pose {
        Pose::new(
            placement.position + placement.rotation * self.centre,
            placement.rotation * self.rotation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(kind: SolidKind, w: f32, h: f32, l: f32) -> SolidInfo {
        SolidInfo {
            kind,
            offset: [0.0; 3],
            rotation: [0.0; 3],
            width: w,
            height: h,
            length: l,
        }
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    /// ag-motion's live sizes: the push block, Norticus and Sheep_1.
    #[test]
    fn the_three_live_shapes() {
        let block = info(SolidKind::Box, 15.0, 12.0, 15.0)
            .havok([100.0; 3])
            .unwrap();
        let Shape::Box {
            half_extents,
            margin,
        } = block.shape
        else {
            panic!("{block:?}")
        };
        assert!(near(half_extents.x, 2.236) && near(half_extents.y, 1.7788));
        assert!(near(half_extents.z, 2.236) && near(margin, 0.05));
        assert!(near(block.centre.y, 1.8288), "{block:?}");

        for (w, h, radius, centre) in [(3.0, 6.0, 0.59008, 1.28016), (5.0, 8.0, 1.0168, 1.70688)] {
            let capsule = info(SolidKind::Capsule, w, h, 3.0)
                .havok([140.0; 3])
                .unwrap();
            let Shape::Capsule {
                radius: r,
                half_segment,
            } = capsule.shape
            else {
                panic!("{capsule:?}")
            };
            assert!(
                near(r, radius) && near(half_segment, 0.64008),
                "{capsule:?}"
            );
            assert!(near(capsule.centre.y, centre), "{capsule:?}");
        }
    }

    /// AG.8's live read: radius 0.8644 m, centre 0.9144 m up.
    #[test]
    fn eruptors_sphere() {
        let eruptor = info(SolidKind::Sphere, 4.0, 6.0, 3.0)
            .havok([150.0; 3])
            .unwrap();
        let radius = |s: Shape| match s {
            Shape::Sphere { radius } => radius,
            other => panic!("{other:?}"),
        };
        assert!(near(radius(eruptor.shape), 0.8644), "{eruptor:?}");
        assert!(near(eruptor.centre.y, 0.9144), "{eruptor:?}");
        // His query shape: the authored sphere, plus the skin.
        assert!(near(radius(eruptor.grown(0.0)), 0.9144));
    }

    /// AG.170: a capsule tipped onto its side by its own angles lies along
    /// the ground with its middle at its offset's height, as AE.22 read the
    /// built shapes of two characters live: 7 x 10.5 ft tipped back at 100 %
    /// (offset 1.0668 m up and 1.6002 m back), and 6 x 10 ft tipped forward
    /// at 120 % (0.9144 m up and 1.524 m ahead). Its half height up along its
    /// own up, not straight up: that stood it 1.6-1.8 m too high.
    #[test]
    fn a_capsule_tipped_on_its_side_lies_at_its_offsets_height() {
        let tipped = |rotation: [f32; 3], offset: [f32; 3], w: f32, h: f32, scale: f32| {
            SolidInfo {
                rotation,
                offset,
                ..info(SolidKind::Capsule, w, h, 3.0)
            }
            .havok([scale; 3])
            .unwrap()
        };
        let back = tipped([-90.0, 0.0, 0.0], [0.0, 1.0668, -1.6002], 7.0, 10.5, 100.0);
        let forward = tipped([90.0, 0.0, 0.0], [0.0, 0.9144, 1.524], 6.0, 10.0, 120.0);
        for (solid, centre, quat, radius, segment) in [
            (
                back,
                [0.0, 1.0668, 0.0],
                [0.707_106_77, 0.0, 0.0, 0.707_106_77],
                1.0168,
                0.5334,
            ),
            (
                forward,
                [0.0, 1.097_28, 0.0],
                [-0.707_106_77, 0.0, 0.0, 0.707_106_77],
                1.047_28,
                0.731_52,
            ),
        ] {
            assert!(
                (solid.centre - Vec3::from(centre)).abs().max_element() < 1e-6,
                "{solid:?}"
            );
            let q = solid.rotation.to_array();
            assert!(q.iter().zip(quat).all(|(a, b)| near(*a, b)), "{q:?}");
            let Shape::Capsule {
                radius: r,
                half_segment,
            } = solid.shape
            else {
                panic!("{solid:?}")
            };
            assert!(near(r, radius) && near(half_segment, segment), "{solid:?}");
            // His pivot at the bottom: the capsule's lowest point 0.05 m
            // over it, as an upright one's is.
            let axis = solid.rotation * Vec3::Y;
            let lowest = solid.centre.y - axis.y.abs() * half_segment - r;
            assert!(near(lowest, 0.05), "{lowest}");
        }
    }

    /// AG.7: the gate's door, 4 x 21 x 34 ft with a quarter turn of its own,
    /// runs its 34 ft along x at heading 180, across the gateway; without its
    /// own turn, or with width and length swapped, along z.
    #[test]
    fn the_door_runs_across_the_gateway() {
        let door = SolidInfo {
            rotation: [0.0, 90.0, 0.0],
            ..info(SolidKind::Box, 4.0, 21.0, 34.0)
        };
        let at = Pose::new(Vec3::new(14.83, 43.4, -82.22), euler([0.0, 180.0, 0.0]));
        let span = |d: &SolidInfo| {
            let solid = d.havok([100.0; 3]).unwrap();
            let pose = solid.pose(at);
            let Shape::Box {
                half_extents,
                margin,
            } = solid.shape
            else {
                panic!()
            };
            let outer = half_extents + Vec3::splat(margin);
            let axes = [Vec3::X, Vec3::Y, Vec3::Z].map(|a| pose.rotation * a);
            let along = |world: Vec3| -> f32 {
                (0..3)
                    .map(|i| (axes[i].dot(world) * outer[i]).abs())
                    .sum::<f32>()
                    * 2.0
            };
            (along(Vec3::X), along(Vec3::Z), pose.position.y)
        };
        let (x, z, y) = span(&door);
        assert!(near(x, 34.0 * FOOT) && near(z, 4.0 * FOOT), "x {x} z {z}");
        assert!(near(y, 43.4 + 3.2004), "{y}");
        let unturned = SolidInfo {
            rotation: [0.0; 3],
            ..door
        };
        let (x, z, _) = span(&unturned);
        assert!(near(z, 34.0 * FOOT) && near(x, 4.0 * FOOT), "x {x} z {z}");
        let swapped = SolidInfo {
            width: 34.0,
            length: 4.0,
            ..door
        };
        let (x, _, _) = span(&swapped);
        assert!(!near(x, 34.0 * FOOT), "a swap must move the 34 ft: {x}");
    }

    #[test]
    fn no_size_under_a_millimetre() {
        // A side of 0.1 m or less shrinks to nothing; a capsule with h <= w
        // has no segment: both take the original's 0.001 m.
        let thin = info(SolidKind::Box, 0.3, 6.0, 0.2)
            .havok([100.0; 3])
            .unwrap();
        let Shape::Box { half_extents, .. } = thin.shape else {
            panic!()
        };
        assert_eq!(half_extents.x, MIN_SIZE);
        assert_eq!(half_extents.z, MIN_SIZE);
        let squat = info(SolidKind::Capsule, 6.0, 4.0, 1.0)
            .havok([100.0; 3])
            .unwrap();
        let Shape::Capsule { half_segment, .. } = squat.shape else {
            panic!()
        };
        assert_eq!(half_segment, MIN_SIZE);
        assert!(squat.shape.is_valid() && thin.shape.is_valid());
        assert!(
            info(SolidKind::None, 3.0, 3.0, 3.0)
                .havok([100.0; 3])
                .is_none()
        );
        assert!(
            info(SolidKind::Mesh, 3.0, 3.0, 3.0)
                .havok([100.0; 3])
                .is_none()
        );
    }

    /// The world's shapes and the hero's grown one sum to the original's.
    #[test]
    fn the_mesh_radius_moves_onto_the_hero() {
        let gourd = info(SolidKind::Sphere, 7.25, 7.25, 7.25)
            .havok([100.0; 3])
            .unwrap();
        let Shape::Sphere { radius: havok } = gourd.shape else {
            panic!()
        };
        let Shape::Sphere { radius: world } = gourd.in_world() else {
            panic!()
        };
        assert!(near(havok - world, MESH_RADIUS));
        let door = info(SolidKind::Box, 4.0, 21.0, 34.0)
            .havok([100.0; 3])
            .unwrap();
        let Shape::Box {
            half_extents,
            margin,
        } = door.in_world()
        else {
            panic!()
        };
        assert!((half_extents - Vec3::new(0.5596, 3.1504, 5.1316)).length() < 1e-5);
        assert_eq!(margin, 0.0);
    }
}
