//! An emitter's particle generators (openspec AH.87), read off the file
//! that places it: each `tfbParticleInfo`'s `GeneratorInfoList` followed to
//! its sprite generators (with their generation, movement and appearance
//! records) and its ribbons, each with the texture it draws
//! (`AnimatableTextureDataInfo` -> `tfbAlphaTextureUnitIDAttr` ->
//! `igTextureAttr2` -> `igImage2`). The layouts and what their bits mean
//! are [`crate::igz::layouts::GeneratorInfo`]'s module's.
//!
//! On the disc (AH.87's read of `character/009_eruptor.bld`): 26 emitters,
//! every generator a `GeneratorInfo` or a `RibbonGenerator`, every one
//! textured. A generator of another kind is kept by its object and type in
//! [`Emitter::unread`], so a level whose emitters hold lightning still
//! reads.

use crate::igz::Igz;
use crate::igz::layouts::{
    AlphaTextureUnitIdAttr, AnimatableTextureDataInfo, AppearanceFields, GenerationFields,
    GeneratorInfo, GeneratorInfoList, Layout, MovementFields, ObjectRef, ParticleInfo,
    RibbonGenerator, TextureAttr2, is_a,
};
use crate::{ErrorKind, FormatError};

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
#[cfg(test)]
mod tests;

/// The format name refusals carry.
pub const FORMAT: &str = "particles";

/// The most generators one emitter is read with: Eruptor's largest holds
/// 6, so a list past this is a mutated file.
pub const MAX_GENERATORS: usize = 64;

/// One `tfbParticleInfo`, its generators read.
#[derive(Debug, Clone, PartialEq)]
pub struct Emitter<'a> {
    /// The placement's object.
    pub index: usize,
    pub name: &'a str,
    pub info: ParticleInfo<'a>,
    /// In list order.
    pub generators: Vec<Generator<'a>>,
    /// Each generator of a kind this reader doesn't draw: its object and
    /// type.
    pub unread: Vec<(usize, &'a str)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Generator<'a> {
    /// Both boxed: a sprite generator's three records make it twice a
    /// ribbon's size, and either is large beside a box.
    Sprite(Box<Sprite<'a>>),
    Ribbon(Box<Ribbon<'a>>),
}

impl<'a> Generator<'a> {
    pub fn index(&self) -> usize {
        match self {
            Generator::Sprite(s) => s.index,
            Generator::Ribbon(r) => r.index,
        }
    }

    pub fn texture(&self) -> Option<&Texture<'a>> {
        match self {
            Generator::Sprite(s) => s.texture.as_ref(),
            Generator::Ribbon(r) => r.texture.as_ref(),
        }
    }
}

/// A sprite generator: camera-facing quads born at a rate, each living
/// its lifespan.
#[derive(Debug, Clone, PartialEq)]
pub struct Sprite<'a> {
    pub index: usize,
    pub info: GeneratorInfo<'a>,
    pub generation: GenerationFields,
    pub movement: MovementFields,
    pub appearance: AppearanceFields,
    pub texture: Option<Texture<'a>>,
}

/// A ribbon: one strip laid along the path its emitter has taken.
#[derive(Debug, Clone, PartialEq)]
pub struct Ribbon<'a> {
    pub index: usize,
    pub info: RibbonGenerator<'a>,
    pub texture: Option<Texture<'a>>,
}

/// What a generator draws with: its `AnimatableTextureDataInfo` (its name
/// the source image's path), the `igTextureAttr2` it binds and that
/// attribute's `igImage2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Texture<'a> {
    pub data: usize,
    pub path: Option<&'a str>,
    pub attr: TextureAttr2,
    pub image: usize,
}

fn missing(field: &'static str) -> FormatError {
    FormatError::new(FORMAT, field, 0, ErrorKind::Missing)
}

/// The texture an `AnimatableTextureDataInfo` binds.
pub fn texture<'a>(igz: &Igz<'a>, data: usize) -> Result<Texture<'a>, FormatError> {
    let info = AnimatableTextureDataInfo::read(igz, data)?;
    let bind = info.tex_attr.ok_or_else(|| missing("_texAttr"))?;
    let at = AlphaTextureUnitIdAttr::read(igz, bind)?
        .bind
        .texture
        .ok_or_else(|| missing("_texture"))?;
    let attr = TextureAttr2::read(igz, at)?;
    let image = attr.image.ok_or_else(|| missing("_image"))?;
    Ok(Texture {
        data,
        path: info.data_info.info.name,
        attr,
        image,
    })
}

/// The `tfbParticleInfo` at `index`, its generators read. Refused: a
/// generator list that isn't a `GeneratorInfoList` (a named or imported
/// one included), one longer than [`MAX_GENERATORS`], a sprite generator
/// missing a record, and any record or texture chain that doesn't read.
pub fn emitter<'a>(igz: &Igz<'a>, index: usize) -> Result<Emitter<'a>, FormatError> {
    let info = ParticleInfo::read(igz, index)?;
    let name = info.placement.placement.name.unwrap_or("");
    let mut out = Emitter {
        index,
        name,
        info,
        generators: Vec::new(),
        unread: Vec::new(),
    };
    let list = match info.generator_info_list {
        ObjectRef::Null => return Ok(out),
        ObjectRef::Object(l) => GeneratorInfoList::read(igz, l)?,
        _ => return Err(missing("_generatorInfoList")),
    };
    if list.generators.len() > MAX_GENERATORS {
        return Err(FormatError::new(
            FORMAT,
            "_generatorInfoList",
            0,
            ErrorKind::OutOfRange {
                value: list.generators.len() as u64,
                limit: MAX_GENERATORS as u64,
            },
        ));
    }
    for g in list.generators.into_iter().flatten() {
        let ty = igz.type_name(&igz.objects()[g]);
        if is_a(ty, GeneratorInfo::TYPE) {
            let info = GeneratorInfo::read(igz, g)?;
            let texture = info.vfx.texture_data.map(|d| texture(igz, d)).transpose()?;
            out.generators.push(Generator::Sprite(Box::new(Sprite {
                index: g,
                generation: GenerationFields::read(
                    igz,
                    info.generation
                        .ok_or_else(|| missing("_generationFields"))?,
                )?,
                movement: MovementFields::read(
                    igz,
                    info.movement.ok_or_else(|| missing("_movementFields"))?,
                )?,
                appearance: AppearanceFields::read(
                    igz,
                    info.appearance
                        .ok_or_else(|| missing("_appearanceFields"))?,
                )?,
                info,
                texture,
            })));
        } else if is_a(ty, RibbonGenerator::TYPE) {
            let info = RibbonGenerator::read(igz, g)?;
            let texture = info.vfx.texture_data.map(|d| texture(igz, d)).transpose()?;
            out.generators.push(Generator::Ribbon(Box::new(Ribbon {
                index: g,
                info,
                texture,
            })));
        } else {
            out.unread.push((g, ty));
        }
    }
    Ok(out)
}

/// Every `tfbParticleInfo` in `igz`, in object order, each read or with
/// why it isn't, one at a time: an emitter reads as up to
/// [`MAX_GENERATORS`] generators of a few hundred bytes each off a list any
/// number of emitters can share, so holding every one at once grows past
/// any bound on the input's size; a caller keeps what it needs of each.
pub fn emitters<'s, 'a>(
    igz: &'s Igz<'a>,
) -> impl Iterator<Item = (usize, Result<Emitter<'a>, FormatError>)> + 's {
    igz.objects()
        .iter()
        .enumerate()
        .filter(|(_, o)| is_a(igz.type_name(o), ParticleInfo::TYPE))
        .map(|(i, _)| (i, emitter(igz, i)))
}
