//! The interface reader (openspec AT.2): an archive's sprite templates and
//! bitmap fonts, each followed to what it draws.
//!
//! A template is a `tfbSpriteInfo` the scripts clone; it's read whole (its
//! place, `_properties`' bitfields, sizes, colours, value and depth) with its
//! content by `_contextDataInfo`: nothing for a container, an
//! `AnimatableTextureDataInfo` for art (its chain down to the `igImage2`, or
//! a movie by its source's `.mov`), a `FontDataInfo` for text, a
//! `tfbPhysicsModel` for a model, a `tfbParticleInfo` for an emitter. A
//! context another file holds (an import, `global.bld`'s font on every text
//! template outside it) is kept by its id for the caller to resolve. A
//! template whose visual type disagrees with what its context is, or whose
//! context is a type this reader doesn't know, is refused by name.
//!
//! A font is an `igBitmapFont`: its metrics, its glyphs and its atlas image.
//! A glyph whose box leaves the atlas (a coordinate outside 0-1, or a
//! minimum past its maximum) is refused.
//!
//! A string is a `StringInfo`, read in address order (the order a template's
//! [`TextKey::Local`] and a language pack's strings follow,
//! [`crate::language`]) for its settings: the flags and the integer and
//! decimal digits a number fills its `@`s by (openspec AT.27). Its text is
//! the language pack's.

use std::collections::HashMap;

use crate::igz::layouts::{
    AlphaTextureUnitIdAttr, AnimatableTextureDataInfo, BitmapFont, CharMetrics, CharMetricsList,
    EulerTransform, FontDataInfo, KernCorners, Layout, ObjectRef, ParticleInfo, PhysicsModel,
    Rgba8, SpriteBits, SpriteInfo, StringInfo, TextureAttr2, VisualType, is_a, read_all,
};
use crate::igz::{ExternalId, Igz};
use crate::language::string_infos;
use crate::{ErrorKind, FormatError};

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
#[cfg(test)]
mod tests;

/// The format name refusals carry.
pub const FORMAT: &str = "interface";

/// What [`ErrorKind::WrongType`] names as wanted when a context is a type
/// the reader doesn't know.
pub const CONTEXT: &str = "a sprite context";

/// An archive's templates and fonts, in object order, and its strings'
/// settings in address order.
#[derive(Debug, Clone, PartialEq)]
pub struct Interface<'a> {
    pub templates: Vec<Template<'a>>,
    pub fonts: Vec<Face<'a>>,
    /// Every `StringInfo`, in address order: [`TextKey::Local`]`(n)` is
    /// `strings[n]`.
    pub strings: Vec<Settings>,
}

/// A `StringInfo`'s settings, as the disc stores them: what a number fills
/// the string's `@`s by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settings {
    /// `_flags`, raw.
    pub flags: i32,
    pub integer_digits: i32,
    pub decimal_digits: i32,
}

/// One `tfbSpriteInfo`, every field read.
#[derive(Debug, Clone, PartialEq)]
pub struct Template<'a> {
    pub object: usize,
    pub name: Option<&'a str>,
    pub bits: SpriteBits,
    pub flags: i32,
    /// 12.4 fixed point, x then y.
    pub position: [i16; 2],
    pub z_depth: f32,
    pub priority: f32,
    pub width: f32,
    pub height: f32,
    /// Percent per axis: 100 is unit scale.
    pub scale: [f32; 3],
    pub color: Rgba8,
    pub replace_color: Rgba8,
    pub num_value: i32,
    pub depth: f32,
    /// The placement's `_initMatrix` and `_currentMatrix` (openspec AT.25):
    /// where it was placed and where it stands, a place and three turns.
    pub init_matrix: EulerTransform,
    pub matrix: EulerTransform,
    pub content: Content<'a>,
    /// Its text key, if it holds one.
    pub string: Option<TextKey>,
}

/// A template's `StringInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKey {
    /// One of this archive's, by its place among them in address order:
    /// the index a language pack's strings follow ([`crate::language`]).
    Local(usize),
    /// One in another file (`global.bld`'s, by export id), for the caller
    /// to resolve.
    Imported(ExternalId),
}

/// What a template draws.
#[derive(Debug, Clone, PartialEq)]
pub enum Content<'a> {
    Container,
    /// No context, under a visual type that draws one: what the type says.
    Empty {
        kind: VisualType,
    },
    Image(Art<'a>),
    Movie(Art<'a>),
    /// An index into [`Interface::fonts`].
    Text {
        face: usize,
    },
    /// The model's build path.
    Model {
        path: Option<&'a str>,
    },
    /// The emitter's placement name.
    Emitter {
        name: Option<&'a str>,
    },
    /// A context in another file, by its import: what the template's visual
    /// type says it is.
    Imported {
        kind: VisualType,
        id: ExternalId,
    },
}

/// An `AnimatableTextureDataInfo`: its source and, when this file holds it,
/// its image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Art<'a> {
    /// The source image's or movie's path.
    pub source: Option<&'a str>,
    /// The local `igImage2`; `None` for a movie and for an image streamed
    /// from another archive (`stream_archive_hash` set).
    pub image: Option<usize>,
    pub stream_archive_hash: i32,
    /// Pixels; -1 on a movie.
    pub desired: [i32; 2],
}

/// One `igBitmapFont`, with its glyphs.
#[derive(Debug, Clone, PartialEq)]
pub struct Face<'a> {
    pub object: usize,
    /// The `FontDataInfo` naming it, and that one's source path.
    pub data: Option<usize>,
    pub source: Option<&'a str>,
    pub name: Option<&'a str>,
    pub height: f32,
    pub base_line: f32,
    pub char_padding: f32,
    pub kern_amount: f32,
    pub color: [f32; 4],
    /// The atlas's `igImage2`.
    pub atlas: usize,
    /// In list order.
    pub glyphs: Vec<Glyph>,
}

/// One `igCharMetrics`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    /// A UTF-16 unit.
    pub char: u16,
    pub step: f32,
    pub offset: [f32; 2],
    pub size: [f32; 2],
    /// The box in the atlas, 0-1.
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub kern: KernCorners,
}

impl<'a> Interface<'a> {
    /// Every font, then every template, then every string's settings, of
    /// `igz`. A `StringInfo` whose shape differs from its layout is refused
    /// by its type name.
    pub fn read(igz: &Igz<'a>) -> Result<Self, FormatError> {
        let mut fonts = Vec::new();
        let mut by_object = HashMap::new();
        let datas: Vec<(usize, FontDataInfo<'a>)> = read_all::<FontDataInfo>(igz)
            .map(|(i, d)| d.map(|d| (i, d)))
            .collect::<Result<_, _>>()?;
        for (i, font) in read_all::<BitmapFont>(igz) {
            let font = font?;
            let data = datas.iter().find(|(_, d)| d.font == Some(i));
            by_object.insert(i, fonts.len());
            fonts.push(face(igz, i, font, data)?);
        }
        let keys = string_infos(igz);
        let strings: HashMap<usize, usize> =
            keys.iter().enumerate().map(|(n, &o)| (o, n)).collect();
        let mut templates = Vec::new();
        for (i, sprite) in read_all::<SpriteInfo>(igz) {
            templates.push(template(igz, i, sprite?, &datas, &by_object, &strings)?);
        }
        let strings = keys
            .into_iter()
            .map(|o| {
                StringInfo::read(igz, o).map(|s| Settings {
                    flags: s.flags,
                    integer_digits: s.integer_digits,
                    decimal_digits: s.decimal_digits,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            templates,
            fonts,
            strings,
        })
    }
}

fn missing(field: &'static str) -> FormatError {
    FormatError::new(FORMAT, field, 0, ErrorKind::Missing)
}

fn template<'a>(
    igz: &Igz<'a>,
    object: usize,
    s: SpriteInfo<'a>,
    datas: &[(usize, FontDataInfo<'a>)],
    faces: &HashMap<usize, usize>,
    strings: &HashMap<usize, usize>,
) -> Result<Template<'a>, FormatError> {
    let bits = s.bits();
    let at = igz.objects()[object].at;
    let disagrees = || {
        igz.err_at(
            at,
            "_visualType",
            ErrorKind::Unsupported {
                value: bits.visual_type.into(),
            },
        )
    };
    let (content, wanted) = match s.context_data_info {
        ObjectRef::Null => match bits.kind() {
            VisualType::Container => (Content::Container, VisualType::Container),
            VisualType::Other(_) => return Err(disagrees()),
            kind => (Content::Empty { kind }, kind),
        },
        ObjectRef::Object(c) => {
            let ty = igz.type_name(&igz.objects()[c]);
            if is_a(ty, AnimatableTextureDataInfo::TYPE) {
                let art = art(igz, c)?;
                let movie = art
                    .source
                    .is_some_and(|p| p.to_ascii_lowercase().ends_with(".mov"));
                let content = if movie {
                    Content::Movie(art)
                } else {
                    Content::Image(art)
                };
                (content, VisualType::Art)
            } else if is_a(ty, FontDataInfo::TYPE) {
                let face = datas
                    .iter()
                    .find(|(d, _)| *d == c)
                    .and_then(|(_, d)| d.font)
                    .and_then(|f| faces.get(&f).copied())
                    .ok_or_else(|| missing("_font"))?;
                (Content::Text { face }, VisualType::Text)
            } else if is_a(ty, PhysicsModel::TYPE) {
                let path = PhysicsModel::read(igz, c)?.model.info.name;
                (Content::Model { path }, VisualType::Model)
            } else if is_a(ty, ParticleInfo::TYPE) {
                let name = ParticleInfo::read(igz, c)?.placement.placement.name;
                (Content::Emitter { name }, VisualType::Particles)
            } else {
                return Err(igz.err_at(
                    igz.objects()[c].at,
                    "_contextDataInfo",
                    ErrorKind::WrongType {
                        expected: CONTEXT,
                        found: ty.to_owned(),
                    },
                ));
            }
        }
        ObjectRef::Import(import) => {
            let kind = bits.kind();
            if matches!(kind, VisualType::Container | VisualType::Other(_)) {
                return Err(disagrees());
            }
            (
                Content::Imported {
                    kind,
                    id: import.id,
                },
                kind,
            )
        }
        ObjectRef::Named { .. } => {
            return Err(igz.err_at(
                at,
                "_contextDataInfo",
                ErrorKind::WrongType {
                    expected: CONTEXT,
                    found: "named ref".to_owned(),
                },
            ));
        }
    };
    if bits.kind() != wanted {
        return Err(disagrees());
    }
    let string = match s.string_info {
        ObjectRef::Null => None,
        ObjectRef::Object(o) => Some(TextKey::Local(*strings.get(&o).ok_or_else(|| {
            igz.err_at(
                igz.objects()[o].at,
                "_stringInfo",
                ErrorKind::WrongType {
                    expected: "StringInfo",
                    found: igz.type_name(&igz.objects()[o]).to_owned(),
                },
            )
        })?)),
        ObjectRef::Import(import) => Some(TextKey::Imported(import.id)),
        ObjectRef::Named { .. } => {
            return Err(igz.err_at(
                at,
                "_stringInfo",
                ErrorKind::WrongType {
                    expected: "StringInfo",
                    found: "named ref".to_owned(),
                },
            ));
        }
    };
    Ok(Template {
        object,
        name: s.placement.name,
        bits,
        flags: s.flags,
        position: s.position,
        z_depth: s.z_depth,
        priority: s.priority,
        width: s.width,
        height: s.height,
        scale: s.scale,
        color: s.color,
        replace_color: s.replace_color,
        num_value: s.num_value,
        depth: s.depth,
        init_matrix: s.placement.init_matrix,
        matrix: s.placement.current_matrix,
        content,
        string,
    })
}

/// An `AnimatableTextureDataInfo` followed to its image, if it has one here.
fn art<'a>(igz: &Igz<'a>, data: usize) -> Result<Art<'a>, FormatError> {
    let d = AnimatableTextureDataInfo::read(igz, data)?;
    let image = match d.tex_attr {
        None => None,
        Some(bind) => {
            let texture = AlphaTextureUnitIdAttr::read(igz, bind)?.bind.texture;
            match texture {
                None => None,
                Some(t) => TextureAttr2::read(igz, t)?.image,
            }
        }
    };
    Ok(Art {
        source: d.data_info.info.name,
        image,
        stream_archive_hash: d.data_info.stream_archive_hash,
        desired: [d.desired_width, d.desired_height],
    })
}

fn face<'a>(
    igz: &Igz<'a>,
    object: usize,
    font: BitmapFont<'a>,
    data: Option<&(usize, FontDataInfo<'a>)>,
) -> Result<Face<'a>, FormatError> {
    let bind = font.texture_bind.ok_or_else(|| missing("_textureBind"))?;
    let texture = AlphaTextureUnitIdAttr::read(igz, bind)?
        .bind
        .texture
        .ok_or_else(|| missing("_texture"))?;
    let atlas = TextureAttr2::read(igz, texture)?
        .image
        .ok_or_else(|| missing("_image"))?;
    let list = font
        .char_metrics_list
        .ok_or_else(|| missing("_charMetricsList"))?;
    let mut glyphs = Vec::new();
    for g in CharMetricsList::read(igz, list)?.glyphs {
        let c = CharMetrics::read(igz, g)?;
        let at = igz.objects()[g].at;
        for (field, uv) in [
            ("_texCoordMin", c.tex_coord_min),
            ("_texCoordMax", c.tex_coord_max),
        ] {
            if let Some(v) = uv.iter().find(|v| !(0.0..=1.0).contains(*v)) {
                return Err(igz.err_at(
                    at,
                    field,
                    ErrorKind::Unsupported {
                        value: v.to_bits().into(),
                    },
                ));
            }
        }
        if c.tex_coord_min[0] > c.tex_coord_max[0] || c.tex_coord_min[1] > c.tex_coord_max[1] {
            return Err(igz.err_at(
                at,
                "_texCoordMax",
                ErrorKind::Unsupported {
                    value: c.tex_coord_max[0].to_bits().into(),
                },
            ));
        }
        glyphs.push(Glyph {
            char: c.char_number,
            step: c.step,
            offset: c.offset,
            size: c.size,
            uv_min: c.tex_coord_min,
            uv_max: c.tex_coord_max,
            kern: c.kern(),
        });
    }
    Ok(Face {
        object,
        data: data.map(|(i, _)| *i),
        source: data.and_then(|(_, d)| d.data_info.info.name),
        name: font.font.name,
        height: font.font.height,
        base_line: font.font.base_line,
        char_padding: font.font.char_padding,
        kern_amount: font.kern_amount,
        color: font.font.color,
        atlas,
        glyphs,
    })
}
