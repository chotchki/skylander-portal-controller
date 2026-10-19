//! A hero's whole model, walked from its `tfbPhysicsModel`:
//! `_tfbBody` -> `tfbPhysicsBody` (Havok skeleton), its `_node` -> `igActor2`
//! -> scene graph -> `igBlendMatrixSelect` -> techniques -> Edge geometry,
//! its `_combinerPrototype` -> `igSkeleton2`, and `_animationSet` -> clips.
//! Each technique's `_materialPass` -> texture binds -> `igTextureAttr2`.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, HashSet};

use super::{
    Actor2, AlphaTextureUnitIdAttr, AnimationInfo, AnimationInfoList, AttrList, BlendMatrixSelect,
    EdgeGeometry, EdgeGeometryAttr, EdgeGeometrySegment, Group, IntList, Layout, NodeList,
    ObjectRef, PhysicsAnimation, PhysicsBody, PhysicsCombinerLink, PhysicsModel,
    RuntimeTechniqueInstance, Skeleton2, SkeletonBone, SkeletonBoneList, TextureAttr2, is_a,
};
use crate::igz::Igz;
use crate::{ErrorKind, FormatError};

/// A skin palette: the select node and its slot -> blend-matrix map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette<'a> {
    pub select: BlendMatrixSelect<'a>,
    /// A key of [`Hero::int_lists`].
    pub indices: Option<usize>,
}

/// One drawn technique and the geometry it draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mesh<'a> {
    pub technique_index: usize,
    pub technique: RuntimeTechniqueInstance<'a>,
    /// The nearest `igBlendMatrixSelect` above it, a key of [`Hero::palettes`].
    pub palette: Option<usize>,
    pub attr_index: usize,
    pub attr: EdgeGeometryAttr,
    /// A key of [`Hero::geometries`].
    pub geometry: usize,
    /// `_materialPass` when it's an `igAttrList`, a key of [`Hero::materials`].
    pub material: Option<usize>,
}

/// One clip-table entry and the clip it wraps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clip<'a> {
    pub info: AnimationInfo<'a>,
    /// `None` when `_tfbAnimation` is null or an animation network.
    pub animation: Option<(usize, PhysicsAnimation<'a>)>,
}

/// A walk's material lists, the texture binds they hold and those binds'
/// textures, each keyed by object index: what [`crate::material`] reads a
/// technique's material out of. [`Hero::passes`] lends a hero's; a level's
/// scene or prop collects its own in a [`MaterialLists`].
#[derive(Debug, Clone, Copy)]
pub struct Passes<'r, 'a> {
    pub lists: &'r BTreeMap<usize, AttrList<'a>>,
    pub binds: &'r BTreeMap<usize, AlphaTextureUnitIdAttr<'a>>,
    pub textures: &'r BTreeMap<usize, TextureAttr2>,
}

/// Material lists read as a walk meets them, each once (AG.15: a level's
/// techniques share 104 of their 249 lists).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaterialLists<'a> {
    pub lists: BTreeMap<usize, AttrList<'a>>,
    pub binds: BTreeMap<usize, AlphaTextureUnitIdAttr<'a>>,
    pub textures: BTreeMap<usize, TextureAttr2>,
}

impl<'a> MaterialLists<'a> {
    /// Reads `technique`'s `_materialPass` when it's an `igAttrList`, and
    /// returns it; any other pass is left alone and `None` comes back.
    pub fn technique(
        &mut self,
        igz: &Igz<'a>,
        technique: &RuntimeTechniqueInstance<'a>,
    ) -> Result<Option<usize>, FormatError> {
        match technique.material_pass {
            ObjectRef::Object(m) if is_a(igz.type_name(&igz.objects()[m]), AttrList::TYPE) => {
                read_list(igz, m, &mut self.lists, &mut self.binds, &mut self.textures)?;
                Ok(Some(m))
            }
            _ => Ok(None),
        }
    }

    pub fn passes(&self) -> Passes<'_, 'a> {
        Passes {
            lists: &self.lists,
            binds: &self.binds,
            textures: &self.textures,
        }
    }
}

/// Reads a material list once, and its texture binds; attributes of other
/// types stay unread. A bind other than `tfbAlphaTextureUnitIDAttr` (none
/// on the disc) is refused.
fn read_list<'a>(
    igz: &Igz<'a>,
    list: usize,
    lists: &mut BTreeMap<usize, AttrList<'a>>,
    binds: &mut BTreeMap<usize, AlphaTextureUnitIdAttr<'a>>,
    textures: &mut BTreeMap<usize, TextureAttr2>,
) -> Result<(), FormatError> {
    let Entry::Vacant(slot) = lists.entry(list) else {
        return Ok(());
    };
    let attrs = slot.insert(AttrList::read(igz, list)?);
    for &a in attrs.attrs.iter().flatten() {
        if !is_a(igz.type_name(&igz.objects()[a]), "igTextureBindAttr2") || binds.contains_key(&a) {
            continue;
        }
        let bind = AlphaTextureUnitIdAttr::read(igz, a)?;
        if let Some(t) = bind.bind.texture
            && let Entry::Vacant(slot) = textures.entry(t)
        {
            slot.insert(TextureAttr2::read(igz, t)?);
        }
        binds.insert(a, bind);
    }
    Ok(())
}

/// Shared objects are read once and keyed by object index.
#[derive(Debug, Clone, PartialEq)]
pub struct Hero<'a> {
    pub model_index: usize,
    pub model: PhysicsModel<'a>,
    pub body_index: usize,
    pub body: PhysicsBody<'a>,
    pub actor_index: usize,
    pub actor: Actor2<'a>,
    pub combiner_index: usize,
    pub combiner: PhysicsCombinerLink<'a>,
    pub skeleton_index: usize,
    pub skeleton: Skeleton2<'a>,
    pub bones: Vec<SkeletonBone<'a>>,
    /// In scene-graph order, depth first.
    pub meshes: Vec<Mesh<'a>>,
    pub palettes: BTreeMap<usize, Palette<'a>>,
    /// The palettes' slot maps; selects can share one.
    pub int_lists: BTreeMap<usize, IntList<'a>>,
    pub geometries: BTreeMap<usize, EdgeGeometry<'a>>,
    pub segments: BTreeMap<usize, EdgeGeometrySegment<'a>>,
    /// Techniques can share a list, and lists a bind.
    pub materials: BTreeMap<usize, AttrList<'a>>,
    /// Every texture bind the materials hold.
    pub texture_binds: BTreeMap<usize, AlphaTextureUnitIdAttr<'a>>,
    /// The binds' `igTextureAttr2`s.
    pub textures: BTreeMap<usize, TextureAttr2>,
    /// The clip table: each slot's `AnimationInfo`, a key of [`Hero::clips`].
    pub clip_slots: Vec<Option<usize>>,
    pub clips: BTreeMap<usize, Clip<'a>>,
}

fn required(
    igz: &Igz<'_>,
    object: usize,
    field: &'static str,
    value: Option<usize>,
) -> Result<usize, FormatError> {
    value.ok_or_else(|| igz.err_at(igz.objects()[object].at, field, ErrorKind::Missing))
}

impl<'a> Hero<'a> {
    pub fn read(igz: &Igz<'a>, model_index: usize) -> Result<Self, FormatError> {
        let model = PhysicsModel::read(igz, model_index)?;
        let body_index = required(igz, model_index, "_tfbBody", model.model.tfb_body)?;
        let body = PhysicsBody::read(igz, body_index)?;
        let actor_index = required(igz, body_index, "_node", body.body.node)?;
        let actor = Actor2::read(igz, actor_index)?;
        let combiner_index = required(
            igz,
            body_index,
            "_combinerPrototype",
            body.body.combiner_prototype,
        )?;
        let combiner = PhysicsCombinerLink::read(igz, combiner_index)?;
        let skeleton_index = required(
            igz,
            combiner_index,
            "_skeleton",
            combiner.combiner.system.skeleton,
        )?;
        let skeleton = Skeleton2::read(igz, skeleton_index)?;
        let bones = match skeleton.bone_list {
            Some(list) => SkeletonBoneList::read(igz, list)?
                .bones
                .into_iter()
                .map(|b| SkeletonBone::read(igz, b))
                .collect::<Result<_, _>>()?,
            None => Vec::new(),
        };

        let mut hero = Self {
            model_index,
            model,
            body_index,
            body,
            actor_index,
            actor,
            combiner_index,
            combiner,
            skeleton_index,
            skeleton,
            bones,
            meshes: Vec::new(),
            palettes: BTreeMap::new(),
            int_lists: BTreeMap::new(),
            geometries: BTreeMap::new(),
            segments: BTreeMap::new(),
            materials: BTreeMap::new(),
            texture_binds: BTreeMap::new(),
            textures: BTreeMap::new(),
            clip_slots: Vec::new(),
            clips: BTreeMap::new(),
        };
        hero.walk(igz)?;
        hero.read_clips(igz)?;
        Ok(hero)
    }

    /// Depth first from the actor; each node and child list is read once,
    /// so a list shared by many groups stays linear.
    fn walk(&mut self, igz: &Igz<'a>) -> Result<(), FormatError> {
        let mut stack = vec![(self.actor_index, None)];
        let mut seen = HashSet::new();
        let mut expanded = HashSet::new();
        while let Some((node, palette)) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            let ty = igz.type_name(&igz.objects()[node]);
            if is_a(ty, RuntimeTechniqueInstance::TYPE) {
                self.mesh(igz, node, palette)?;
                continue;
            }
            let mut palette = palette;
            if is_a(ty, BlendMatrixSelect::TYPE) {
                let select = BlendMatrixSelect::read(igz, node)?;
                let indices = select.blend_matrix_indices;
                if let Some(i) = indices
                    && let Entry::Vacant(slot) = self.int_lists.entry(i)
                {
                    slot.insert(IntList::read(igz, i)?);
                }
                self.palettes.insert(node, Palette { select, indices });
                palette = Some(node);
            }
            // Leaves other than techniques aren't gate types, so refuse them.
            let group = Group::read(igz, node)?;
            if let Some(list) = group.child_list
                && expanded.insert(list)
            {
                let children = NodeList::read(igz, list)?.nodes;
                let fresh = children.into_iter().rev().filter(|c| !seen.contains(c));
                stack.extend(fresh.map(|c| (c, palette)));
            }
        }
        Ok(())
    }

    fn mesh(
        &mut self,
        igz: &Igz<'a>,
        technique_index: usize,
        palette: Option<usize>,
    ) -> Result<(), FormatError> {
        let technique = RuntimeTechniqueInstance::read(igz, technique_index)?;
        let attr_index = required(igz, technique_index, "_geomAttr", technique.geom_attr)?;
        let attr = EdgeGeometryAttr::read(igz, attr_index)?;
        let geometry = required(igz, attr_index, "_geometry", attr.geometry)?;
        if !self.geometries.contains_key(&geometry) {
            let g = EdgeGeometry::read(igz, geometry)?;
            for &s in &g.segments {
                if let Entry::Vacant(slot) = self.segments.entry(s) {
                    slot.insert(EdgeGeometrySegment::read(igz, s)?);
                }
            }
            self.geometries.insert(geometry, g);
        }
        let material = match technique.material_pass {
            ObjectRef::Object(m) if is_a(igz.type_name(&igz.objects()[m]), AttrList::TYPE) => {
                self.material(igz, m)?;
                Some(m)
            }
            _ => None,
        };
        self.meshes.push(Mesh {
            technique_index,
            technique,
            palette,
            attr_index,
            attr,
            geometry,
            material,
        });
        Ok(())
    }

    /// Reads a material list once, and its texture binds ([`read_list`]).
    fn material(&mut self, igz: &Igz<'a>, list: usize) -> Result<(), FormatError> {
        read_list(
            igz,
            list,
            &mut self.materials,
            &mut self.texture_binds,
            &mut self.textures,
        )
    }

    /// Its material lists, binds and textures, as [`crate::material`]
    /// reads them.
    pub fn passes(&self) -> Passes<'_, 'a> {
        Passes {
            lists: &self.materials,
            binds: &self.texture_binds,
            textures: &self.textures,
        }
    }

    fn read_clips(&mut self, igz: &Igz<'a>) -> Result<(), FormatError> {
        let Some(set) = self.model.model.animation_set else {
            return Ok(());
        };
        self.clip_slots = AnimationInfoList::read(igz, set)?.entries;
        for &info_index in self.clip_slots.iter().flatten() {
            if self.clips.contains_key(&info_index) {
                continue;
            }
            let info = AnimationInfo::read(igz, info_index)?;
            let animation = info
                .tfb_animation
                .filter(|&a| is_a(igz.type_name(&igz.objects()[a]), PhysicsAnimation::TYPE))
                .map(|a| PhysicsAnimation::read(igz, a).map(|p| (a, p)))
                .transpose()?;
            self.clips.insert(info_index, Clip { info, animation });
        }
        Ok(())
    }

    /// The texture a mesh binds at `unit` (0 is colour), with its index.
    /// `None` when nothing binds there; the first bind wins.
    pub fn texture_at(&self, mesh: &Mesh<'a>, unit: i16) -> Option<(usize, &TextureAttr2)> {
        let list = self.materials.get(&mesh.material?)?;
        list.attrs
            .iter()
            .flatten()
            .filter_map(|a| self.texture_binds.get(a))
            .find(|b| b.bind.unit() == unit)
            .and_then(|b| b.bind.texture)
            .and_then(|t| self.textures.get(&t).map(|attr| (t, attr)))
    }

    /// Every segment the meshes draw, in mesh then segment order.
    pub fn mesh_segments<'s>(
        &'s self,
        mesh: &'s Mesh<'a>,
    ) -> impl Iterator<Item = (usize, &'s EdgeGeometrySegment<'a>)> + 's {
        self.geometries
            .get(&mesh.geometry)
            .into_iter()
            .flat_map(|g| &g.segments)
            .filter_map(|s| self.segments.get(s).map(|seg| (*s, seg)))
    }
}
