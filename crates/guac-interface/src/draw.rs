//! The draw list and the colour maths the adapter's material runs (the
//! crate docs, Colour and blends and Draw order; openspec AT.7).

use guac_ir::interface::Content;

use crate::layout::Frame;
use crate::model::{LETTER, Model};
use crate::sprite::{SpriteId, flags, kind, props};

/// `blend mode`'s values, as the EBOOT's enum names them (0x5B97E0).
pub mod blend {
    pub const OPAQUE: u32 = 0;
    pub const CROSSFADE: u32 = 1;
    pub const ADDITIVE: u32 = 2;
    pub const CUTS_OUT: u32 = 3;
    pub const GETS_CUT_OUT: u32 = 4;
    pub const CLEARS_CUT_OUT: u32 = 5;
    pub const GETS_CUT_OUT_ADD: u32 = 6;
    pub const CUTS_OUT_INVERT: u32 = 7;
}

/// How a quad colours what it draws: its colour (the tint), its replace
/// colour with the percentage in its fourth byte, its blend and whether the
/// program inverts its alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paint {
    pub color: [u8; 4],
    pub replace: [u8; 4],
    pub blend: u32,
    pub invert: bool,
}

/// One thing the frame draws, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// An image over a rectangle: its texture (`textures/<sha256>.ktx2`,
    /// none when the extract holds no image) and UV box (u, v at the box's
    /// top-left, then at its bottom-right).
    Art {
        sprite: SpriteId,
        texture: Option<String>,
        rect: [f32; 4],
        uv: [f32; 4],
        paint: Paint,
    },
    /// One glyph from its font's atlas, its letter's colour beside the
    /// sprite's (the text colour converter's, 0x808080FF until set).
    Glyph {
        sprite: SpriteId,
        atlas: String,
        rect: [f32; 4],
        uv: [f32; 4],
        letter: [u8; 4],
        paint: Paint,
    },
    /// A model, an emitter or a movie the adapter draws over the rectangle
    /// (AT.13).
    Model {
        sprite: SpriteId,
        path: Option<String>,
        rect: [f32; 4],
    },
    Emitter {
        sprite: SpriteId,
        name: Option<String>,
        rect: [f32; 4],
    },
    Movie {
        sprite: SpriteId,
        source: Option<String>,
        rect: [f32; 4],
    },
}

impl Entry {
    pub fn sprite(&self) -> SpriteId {
        match self {
            Entry::Art { sprite, .. }
            | Entry::Glyph { sprite, .. }
            | Entry::Model { sprite, .. }
            | Entry::Emitter { sprite, .. }
            | Entry::Movie { sprite, .. } => *sprite,
        }
    }

    pub fn rect(&self) -> [f32; 4] {
        match self {
            Entry::Art { rect, .. }
            | Entry::Glyph { rect, .. }
            | Entry::Model { rect, .. }
            | Entry::Emitter { rect, .. }
            | Entry::Movie { rect, .. } => *rect,
        }
    }
}

/// The UV box a `texture flip` gives (the crate docs' Hypothesis, AT.28):
/// 1 swaps the u ends, 2 the v ends, 3 both.
pub fn flipped(uv: [f32; 4], flip: u32) -> [f32; 4] {
    let [mut u0, mut v0, mut u1, mut v1] = uv;
    if flip & 1 != 0 {
        std::mem::swap(&mut u0, &mut u1);
    }
    if flip & 2 != 0 {
        std::mem::swap(&mut v0, &mut v1);
    }
    [u0, v0, u1, v1]
}

/// Whether the program inverts a blend's alpha: cuts out (invert), and
/// clears cut out by the crate docs' Hypothesis.
pub fn inverts(mode: u32) -> bool {
    mode == blend::CUTS_OUT_INVERT || mode == blend::CLEARS_CUT_OUT
}

/// What the fragment program makes of a texel (AT.3): rgb = texel × tint ×
/// 2, alpha = texel alpha × tint alpha, the tint the colour bytes over 255;
/// rgb then mixed toward the replace colour by its fourth byte over 255; the
/// alpha inverted when told to.
pub fn shade(texel: [f32; 4], paint: &Paint) -> [f32; 4] {
    let byte = |b: u8| f32::from(b) / 255.0;
    let w = byte(paint.replace[3]);
    let mut out = [0.0; 4];
    for c in 0..3 {
        let rgb = texel[c] * byte(paint.color[c]) * 2.0;
        out[c] = rgb + (byte(paint.replace[c]) - rgb) * w;
    }
    let a = texel[3] * byte(paint.color[3]);
    out[3] = if paint.invert { 1.0 - a } else { a };
    out
}

/// One blend of a shaded source onto the destination, as AC.13's GPU states
/// draw each (the crate docs, Colour and blends), clamped to 0-1 as the
/// frame stores it. Opaque, additive and gets cut out (add) by their names
/// (the crate docs' Hypothesis).
pub fn blend(mode: u32, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    let mut out = dst;
    match mode {
        blend::OPAQUE => out[..3].copy_from_slice(&src[..3]),
        blend::CROSSFADE => {
            for c in 0..3 {
                out[c] = src[c] * src[3] + dst[c] * (1.0 - src[3]);
            }
        }
        blend::ADDITIVE => {
            for c in 0..3 {
                out[c] = dst[c] + src[c] * src[3];
            }
        }
        blend::CUTS_OUT | blend::CUTS_OUT_INVERT => out[3] = dst[3] + src[3],
        blend::GETS_CUT_OUT => {
            for c in 0..3 {
                out[c] = src[c] * (1.0 - dst[3]) + dst[c] * dst[3];
            }
        }
        blend::CLEARS_CUT_OUT => out[3] = dst[3] - src[3],
        blend::GETS_CUT_OUT_ADD => {
            for c in 0..3 {
                out[c] = dst[c] + src[c] * (1.0 - dst[3]);
            }
        }
        _ => {}
    }
    out.map(|v| v.clamp(0.0, 1.0))
}

impl Model {
    /// The frame's draw list (the crate docs, What it draws and Draw order):
    /// the frame's sprite list by computed priority, higher first (behind),
    /// the list's own order breaking ties; each sprite's quads, glyphs or
    /// entry.
    pub fn draw_list(&self, frame: &Frame) -> Vec<Entry> {
        let mut order: Vec<SpriteId> = self
            .drawing
            .iter()
            .copied()
            .filter(|&id| frame.get(id).is_some_and(|p| p.drawn))
            .collect();
        order.sort_by(|a, b| {
            let (pa, pb) = (frame.get(*a).unwrap(), frame.get(*b).unwrap());
            pb.priority.total_cmp(&pa.priority)
        });
        let mut out = Vec::new();
        for id in order {
            let p = frame.get(id).unwrap();
            let s = self.sprite(id);
            let mode = props::BLEND.get(s.properties);
            let paint = Paint {
                color: p.color,
                replace: p.replace,
                blend: mode,
                invert: inverts(mode),
            };
            let rect = p.rect();
            match (s.kind(), &s.content) {
                (kind::ART, Content::Image(a)) => out.push(Entry::Art {
                    sprite: id,
                    texture: a.texture.clone(),
                    rect,
                    uv: flipped([0.0, 0.0, 1.0, 1.0], flags::FLIP.get(s.flags)),
                    paint,
                }),
                (kind::ART, Content::Movie(a)) => out.push(Entry::Movie {
                    sprite: id,
                    source: a.source.clone(),
                    rect,
                }),
                (kind::TEXT, _) => {
                    let Some((glyphs, text)) = &p.text else {
                        continue;
                    };
                    let Some(font) = s.font.and_then(|f| self.font(f)) else {
                        continue;
                    };
                    let [x, _, y] = p.point;
                    let sx = text.scale[0] * p.scale[0];
                    let sy = text.scale[1] * p.scale[2];
                    for (i, g) in glyphs.iter().enumerate() {
                        out.push(Entry::Glyph {
                            sprite: id,
                            atlas: font.atlas.clone(),
                            rect: [
                                x + g.rect[0] * sx,
                                y + g.rect[1] * sy,
                                x + g.rect[2] * sx,
                                y + g.rect[3] * sy,
                            ],
                            uv: g.uv,
                            letter: s.letters.get(i).copied().unwrap_or(LETTER),
                            paint,
                        });
                    }
                }
                (kind::MODEL, Content::Model(path)) => out.push(Entry::Model {
                    sprite: id,
                    path: path.clone(),
                    rect,
                }),
                (kind::PARTICLES, Content::Emitter(name)) => out.push(Entry::Emitter {
                    sprite: id,
                    name: name.clone(),
                    rect,
                }),
                (kind::MODEL, _) => out.push(Entry::Model {
                    sprite: id,
                    path: None,
                    rect,
                }),
                (kind::PARTICLES, _) => out.push(Entry::Emitter {
                    sprite: id,
                    name: None,
                    rect,
                }),
                // Art with no image here (imported, empty): an image the
                // adapter can't name.
                (kind::ART, _) => out.push(Entry::Art {
                    sprite: id,
                    texture: None,
                    rect,
                    uv: flipped([0.0, 0.0, 1.0, 1.0], flags::FLIP.get(s.flags)),
                    paint,
                }),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
