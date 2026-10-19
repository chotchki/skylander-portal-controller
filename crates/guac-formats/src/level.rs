//! A level's scenes, placements and lights (openspec AG.3, design decisions
//! 8 and 10), read off its IGZ beside its collision ([`crate::collision`]).
//!
//! - [`Scene`]: an `igSceneInfo`'s graph walked from its root to AC's
//!   technique instances and their Edge segments, each technique with the
//!   matrix its groups put it in. On the disc (ag-levels): 75 scenes,
//!   16,912 segments, 11,385,121 triangles, every graph a shallow tree
//!   already in the level's frame bar 28 transforms in 10 levels. No node,
//!   geometry or segment is reached twice there, so a second reach is
//!   refused: a scene draws each thing once and the walk stays linear.
//! - [`Placements`]: every actor, emitter and placed light, each actor with
//!   its script's path, its solid, its movement numbers, its model and its
//!   waypoints. Scripts aren't run here (AJ's VM runs them).
//! - [`directional_lights`]: the level's light rig (`_worldLight1`-`3` and
//!   `_causticLight` on level 213).

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::igz::Igz;
use crate::igz::layouts::{
    ActorInfo, ActorParameters, ActorWaypoint, ActorWaypointList, CollisionInfo,
    DirectionalLightInfo, EdgeGeometry, EdgeGeometryAttr, EdgeGeometrySegment, Group, Layout,
    LightInfo, Mat44, ModelInfo, NodeList, ParticleInfo, Placement, RuntimeTechniqueInstance,
    SceneInfo, ScriptInfo, SpatialNode, Transform, UserInfo, is_a, read_all,
};
use crate::{ErrorKind, FormatError};

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
#[cfg(test)]
mod tests;

pub const IDENTITY: Mat44 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// What a scene graph may hold, as a refusal names it.
pub const SCENE_NODES: &str =
    "igGroup, igSpatialNode, igTransform, igUserInfo or tfbRuntimeTechniqueInstance";

/// What a placement may be, as a refusal names it.
pub const PLACEMENTS: &str = "tfbActorInfo, tfbParticleInfo or tfbLightInfo";

/// `a` then `b` for row vectors: p * mul(a, b) is (p * a) * b.
pub fn mul(a: &Mat44, b: &Mat44) -> Mat44 {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..4).map(|k| a[r][k] * b[k][c]).sum()))
}

/// A point through a row-vector affine matrix (translation in row 3).
pub fn transform_point(p: [f32; 3], m: &Mat44) -> [f32; 3] {
    std::array::from_fn(|c| p[0] * m[0][c] + p[1] * m[1][c] + p[2] * m[2][c] + m[3][c])
}

fn required(
    igz: &Igz<'_>,
    object: usize,
    field: &'static str,
    value: Option<usize>,
) -> Result<usize, FormatError> {
    value.ok_or_else(|| igz.err_at(igz.objects()[object].at, field, ErrorKind::Missing))
}

fn duplicate(igz: &Igz<'_>, object: usize, field: &'static str) -> FormatError {
    igz.err_at(igz.objects()[object].at, field, ErrorKind::Duplicate)
}

/// A group of a scene graph, by its type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GroupKind<'a> {
    Group(Group<'a>),
    Spatial(SpatialNode<'a>),
    Transform(Transform<'a>),
    UserInfo(UserInfo<'a>),
}

impl<'a> GroupKind<'a> {
    pub fn group(&self) -> &Group<'a> {
        match self {
            GroupKind::Group(g) => g,
            GroupKind::Spatial(s) => &s.group,
            GroupKind::Transform(t) => &t.group,
            GroupKind::UserInfo(u) => &u.group,
        }
    }

    fn read(igz: &Igz<'a>, node: usize, field: &'static str) -> Result<Option<Self>, FormatError> {
        let o = &igz.objects()[node];
        let ty = igz.type_name(o);
        Ok(Some(if is_a(ty, Transform::TYPE) {
            GroupKind::Transform(Transform::read(igz, node)?)
        } else if is_a(ty, SpatialNode::TYPE) {
            GroupKind::Spatial(SpatialNode::read(igz, node)?)
        } else if is_a(ty, UserInfo::TYPE) {
            GroupKind::UserInfo(UserInfo::read(igz, node)?)
        } else if ty == Group::TYPE {
            GroupKind::Group(Group::read(igz, node)?)
        } else if is_a(ty, RuntimeTechniqueInstance::TYPE) {
            return Ok(None);
        } else {
            let kind = ErrorKind::WrongType {
                expected: SCENE_NODES,
                found: ty.to_owned(),
            };
            return Err(igz.err_at(o.at, field, kind));
        }))
    }
}

/// A group with its place in the tree and the frame it gives its children.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneGroup<'a> {
    pub kind: GroupKind<'a>,
    /// Its parent, a key of [`Scene::groups`]; `None` at the root.
    pub parent: Option<usize>,
    /// Its child nodes, groups and techniques, in list order.
    pub children: Vec<usize>,
    /// Where its children sit: its own matrix, if it's a transform, then
    /// every one above it.
    pub world: Mat44,
    /// The nearest `igUserInfo` at or above it (the sky's groups on level
    /// 213), a key of [`Scene::groups`].
    pub user_info: Option<usize>,
}

/// One technique instance and the geometry it draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Draw<'a> {
    pub technique_index: usize,
    pub technique: RuntimeTechniqueInstance<'a>,
    /// The group it hangs under, a key of [`Scene::groups`].
    pub parent: usize,
    pub attr_index: usize,
    pub attr: EdgeGeometryAttr,
    /// A key of [`Scene::geometries`].
    pub geometry: usize,
    /// Its segments' frame: a vertex p sits at p * world in the level.
    pub world: Mat44,
    /// Its parent's [`SceneGroup::user_info`].
    pub user_info: Option<usize>,
}

/// One scene, walked: every group and draw, keyed or listed by object index.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene<'a> {
    pub index: usize,
    pub info: SceneInfo<'a>,
    /// `_sceneGraph`, a key of [`Scene::groups`].
    pub root: usize,
    pub groups: BTreeMap<usize, SceneGroup<'a>>,
    /// In scene-graph order, depth first.
    pub draws: Vec<Draw<'a>>,
    pub geometries: BTreeMap<usize, EdgeGeometry<'a>>,
    pub segments: BTreeMap<usize, EdgeGeometrySegment<'a>>,
}

impl<'a> Scene<'a> {
    /// Walks the `igSceneInfo` at `index`. Refused: no root, a root that
    /// isn't a group, more roots in `_sceneGraphList` (empty on the disc), a
    /// node that isn't one of [`SCENE_NODES`], a node, non-empty child list,
    /// geometry or segment reached twice, a transform that isn't finite and
    /// a technique without geometry.
    pub fn read(igz: &Igz<'a>, index: usize) -> Result<Self, FormatError> {
        let info = SceneInfo::read(igz, index)?;
        if let Some(list) = info.scene_graph_list {
            let extra = NodeList::read(igz, list)?.nodes.len();
            if extra != 0 {
                let kind = ErrorKind::CountMismatch {
                    found: extra as u64,
                    expected: 0,
                };
                return Err(igz.err_at(igz.objects()[list].at, "_sceneGraphList", kind));
            }
        }
        let root = required(igz, index, "_sceneGraph", info.scene_graph)?;
        let mut scene = Self {
            index,
            info,
            root,
            groups: BTreeMap::new(),
            draws: Vec::new(),
            geometries: BTreeMap::new(),
            segments: BTreeMap::new(),
        };
        scene.walk(igz)?;
        Ok(scene)
    }

    /// Depth first from the root, each node read once.
    fn walk(&mut self, igz: &Igz<'a>) -> Result<(), FormatError> {
        let mut stack: Vec<(usize, Option<usize>)> = vec![(self.root, None)];
        let mut seen = HashSet::new();
        // Child lists already expanded, and whether they held anything.
        let mut lists: HashMap<usize, bool> = HashMap::new();
        while let Some((node, parent)) = stack.pop() {
            let field = if parent.is_some() {
                "_childList"
            } else {
                "_sceneGraph"
            };
            if !seen.insert(node) {
                return Err(duplicate(igz, node, field));
            }
            let Some(kind) = GroupKind::read(igz, node, field)? else {
                let Some(parent) = parent else {
                    let kind = ErrorKind::WrongType {
                        expected: Group::TYPE,
                        found: RuntimeTechniqueInstance::TYPE.to_owned(),
                    };
                    return Err(igz.err_at(igz.objects()[node].at, field, kind));
                };
                self.draw(igz, node, parent)?;
                continue;
            };
            let (above, above_info) = parent
                .and_then(|p| self.groups.get(&p))
                .map_or((IDENTITY, None), |g| (g.world, g.user_info));
            let world = match &kind {
                GroupKind::Transform(t) => {
                    if let Some(x) = t.m.as_flattened().iter().find(|x| !x.is_finite()) {
                        let kind = ErrorKind::Unsupported {
                            value: x.to_bits().into(),
                        };
                        return Err(igz.err_at(igz.objects()[node].at, "_m", kind));
                    }
                    mul(&t.m, &above)
                }
                _ => above,
            };
            let user_info = match kind {
                GroupKind::UserInfo(_) => Some(node),
                _ => above_info,
            };
            let children = match kind.group().child_list {
                None => Vec::new(),
                Some(list) => match lists.get(&list) {
                    Some(true) => return Err(duplicate(igz, list, "_childList")),
                    Some(false) => Vec::new(),
                    None => {
                        let nodes = NodeList::read(igz, list)?.nodes;
                        lists.insert(list, !nodes.is_empty());
                        nodes
                    }
                },
            };
            stack.extend(children.iter().rev().map(|&c| (c, Some(node))));
            self.groups.insert(
                node,
                SceneGroup {
                    kind,
                    parent,
                    children,
                    world,
                    user_info,
                },
            );
        }
        Ok(())
    }

    fn draw(&mut self, igz: &Igz<'a>, node: usize, parent: usize) -> Result<(), FormatError> {
        let technique = RuntimeTechniqueInstance::read(igz, node)?;
        let attr_index = required(igz, node, "_geomAttr", technique.geom_attr)?;
        let attr = EdgeGeometryAttr::read(igz, attr_index)?;
        let geometry = required(igz, attr_index, "_geometry", attr.geometry)?;
        let Entry::Vacant(slot) = self.geometries.entry(geometry) else {
            return Err(duplicate(igz, geometry, "_geometry"));
        };
        let g = slot.insert(EdgeGeometry::read(igz, geometry)?);
        for &s in &g.segments {
            let Entry::Vacant(seg) = self.segments.entry(s) else {
                return Err(duplicate(igz, s, "segment"));
            };
            seg.insert(EdgeGeometrySegment::read(igz, s)?);
        }
        // The parent was inserted before its children were pushed.
        let (world, user_info) = self
            .groups
            .get(&parent)
            .map_or((IDENTITY, None), |g| (g.world, g.user_info));
        self.draws.push(Draw {
            technique_index: node,
            technique,
            parent,
            attr_index,
            attr,
            geometry,
            world,
            user_info,
        });
        Ok(())
    }

    pub fn draw_segments<'s>(
        &'s self,
        draw: &'s Draw<'a>,
    ) -> impl Iterator<Item = (usize, &'s EdgeGeometrySegment<'a>)> + 's {
        self.geometries
            .get(&draw.geometry)
            .into_iter()
            .flat_map(|g| &g.segments)
            .filter_map(|s| self.segments.get(s).map(|seg| (*s, seg)))
    }

    /// The disc's scenes hold 54: 28 aren't identity, 40 are animated
    /// (ag-levels).
    pub fn transforms(&self) -> impl Iterator<Item = (usize, &Transform<'a>)> + '_ {
        self.groups.iter().filter_map(|(i, g)| match &g.kind {
            GroupKind::Transform(t) => Some((*i, t)),
            _ => None,
        })
    }
}

/// Every `igSceneInfo` in a level's IGZ, in object order, each walked.
pub fn scenes<'s, 'a>(
    igz: &'s Igz<'a>,
) -> impl Iterator<Item = (usize, Result<Scene<'a>, FormatError>)> + 's {
    igz.objects()
        .iter()
        .enumerate()
        .filter(|(_, o)| is_a(igz.type_name(o), SceneInfo::TYPE))
        .map(|(i, _)| (i, Scene::read(igz, i)))
}

/// One `tfbActorInfo` and what it points at, read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Actor<'a> {
    pub index: usize,
    pub info: ActorInfo<'a>,
    /// `_script`'s source path (`.../Scripts/Start_Level.ai`).
    pub script: Option<&'a str>,
    pub collision: Option<CollisionInfo<'a>>,
    pub parameters: Option<ActorParameters<'a>>,
}

impl<'a> Actor<'a> {
    pub fn name(&self) -> Option<&'a str> {
        self.info.placement.placement.name
    }

    pub fn placement(&self) -> &Placement<'a> {
        &self.info.placement
    }
}

/// Every placement of a level, in object order, and what the actors share.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Placements<'a> {
    pub actors: Vec<Actor<'a>>,
    pub emitters: Vec<(usize, ParticleInfo<'a>)>,
    pub lights: Vec<(usize, LightInfo<'a>)>,
    /// The actors' models, keyed by object index.
    pub models: BTreeMap<usize, ModelInfo<'a>>,
    /// The actors' waypoint lists, keyed by object index, each in path
    /// order.
    pub waypoint_lists: BTreeMap<usize, Vec<ActorWaypoint<'a>>>,
}

impl<'a> Placements<'a> {
    /// Reads every object that is a `Placement`. Refused: one that isn't
    /// one of [`PLACEMENTS`], and a waypoint in two lists (none on the disc).
    pub fn read(igz: &Igz<'a>) -> Result<Self, FormatError> {
        let mut out = Self::default();
        let mut waypoints = HashSet::new();
        for (i, o) in igz.objects().iter().enumerate() {
            let ty = igz.type_name(o);
            if !is_a(ty, Placement::TYPE) {
                continue;
            }
            if is_a(ty, ActorInfo::TYPE) {
                let actor = out.read_actor(igz, i, &mut waypoints)?;
                out.actors.push(actor);
            } else if is_a(ty, ParticleInfo::TYPE) {
                out.emitters.push((i, ParticleInfo::read(igz, i)?));
            } else if is_a(ty, LightInfo::TYPE) {
                out.lights.push((i, LightInfo::read(igz, i)?));
            } else {
                let kind = ErrorKind::WrongType {
                    expected: PLACEMENTS,
                    found: ty.to_owned(),
                };
                return Err(igz.err_at(o.at, "placement", kind));
            }
        }
        Ok(out)
    }

    fn read_actor(
        &mut self,
        igz: &Igz<'a>,
        index: usize,
        seen: &mut HashSet<usize>,
    ) -> Result<Actor<'a>, FormatError> {
        let info = ActorInfo::read(igz, index)?;
        let placement = &info.placement;
        let script = match placement.script {
            Some(s) => ScriptInfo::read(igz, s)?.info.name,
            None => None,
        };
        let collision = placement
            .collision
            .map(|c| CollisionInfo::read(igz, c))
            .transpose()?;
        let parameters = info
            .actor_parameters
            .map(|p| ActorParameters::read(igz, p))
            .transpose()?;
        if let Some(m) = info.model
            && let Entry::Vacant(slot) = self.models.entry(m)
        {
            slot.insert(ModelInfo::read(igz, m)?);
        }
        if let Some(l) = info.waypoints
            && let Entry::Vacant(slot) = self.waypoint_lists.entry(l)
        {
            let list = ActorWaypointList::read(igz, l)?;
            let mut points = Vec::with_capacity(list.waypoints.len());
            for w in list.waypoints {
                if !seen.insert(w) {
                    return Err(duplicate(igz, w, "_wayPoints"));
                }
                points.push(ActorWaypoint::read(igz, w)?);
            }
            slot.insert(points);
        }
        Ok(Actor {
            index,
            info,
            script,
            collision,
            parameters,
        })
    }

    /// The actor's model's path (`.../Door_Breakable_Quarry.mdl`).
    pub fn model_path(&self, actor: &Actor<'a>) -> Option<&'a str> {
        self.models.get(&actor.info.model?)?.info.name
    }

    /// The actor's waypoints in path order; empty without a list.
    pub fn waypoints(&self, actor: &Actor<'a>) -> &[ActorWaypoint<'a>] {
        actor
            .info
            .waypoints
            .and_then(|l| self.waypoint_lists.get(&l))
            .map_or(&[], Vec::as_slice)
    }

    /// The first actor of that name: nothing makes names unique.
    pub fn actor(&self, name: &str) -> Option<&Actor<'a>> {
        self.actors.iter().find(|a| a.name() == Some(name))
    }
}

/// Every `DirectionalLightInfo` in a level's IGZ, in object order: the
/// three the level's materials read (`_worldLight1`-`3`) and
/// `_causticLight` on level 213.
pub fn directional_lights<'a>(
    igz: &Igz<'a>,
) -> Result<Vec<(usize, DirectionalLightInfo<'a>)>, FormatError> {
    read_all::<DirectionalLightInfo>(igz)
        .map(|(i, l)| l.map(|l| (i, l)))
        .collect()
}
