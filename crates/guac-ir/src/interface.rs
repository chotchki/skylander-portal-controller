//! An archive's interface (openspec AT.8): every sprite template its
//! `level.bld` holds, its bitmap fonts and its English text with each
//! string's settings. A record in files of its own, for the extract-wide
//! archives and each level's `sprites` part, the images beside them in the
//! characters' texture store, so [`crate::IR_VERSION`] holds; its own shape
//! is [`RECORD`], which the files' names carry.
//!
//! Numbers are kept as the disc stores them: places in 12.4 fixed point,
//! sizes in interface units, the scale in percent, colours as their four
//! bytes (0x80 a channel at full), `_properties` raw (guac-formats'
//! `SpriteBits` names its fields). What they mean on screen is
//! `guac-interface`'s.

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;

/// The record's shape inside [`crate::IR_VERSION`], carried in its files'
/// names (`interface/<stem>.v2.ron`, a level's `sprites.v2.ron`), so an
/// extract holding another shape reads the two parts again and nothing
/// else. AT.8's was 1, its files named without it; 2 (AT.27) gives each
/// string its settings ([`Str`]), which a 1's strings can't say. Bump it
/// whenever a type here changes shape.
pub const RECORD: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Interface {
    pub identity: DumpIdentity,
    /// The archive's path under `PS3_GAME/USRDIR` (`permanent/global.bld`).
    pub archive: String,
    /// In object order.
    pub templates: Vec<Template>,
    pub fonts: Vec<Font>,
    /// The archive's `StringInfo`s in address order, each with its English
    /// text: a [`TextKey::Local`] indexes it. Empty when the archive has no
    /// English pack.
    pub strings: Vec<Str>,
    /// What didn't extract (an image that won't decode, a streamed one its
    /// `.arc` lacks), by name, with why.
    pub refused: Vec<(String, String)>,
}

/// One `tfbSpriteInfo`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    /// Its object in the archive's `level.bld`: what a script's reference
    /// to it holds.
    pub object: u32,
    /// `_properties`, raw.
    pub properties: u32,
    pub flags: i32,
    /// 12.4 fixed point, x then y.
    pub position: [i16; 2],
    pub z_depth: f32,
    pub priority: f32,
    pub width: f32,
    pub height: f32,
    /// Percent per axis.
    pub scale: [f32; 3],
    pub color: [u8; 4],
    pub replace_color: [u8; 4],
    pub num_value: i32,
    pub depth: f32,
    pub content: Content,
    pub string: Option<TextKey>,
}

/// What a sprite's visual type says it draws, the raw `_visualType` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Art,
    Text,
    Model,
    Container,
    Particles,
}

/// What a template draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Content {
    Container,
    /// No context under a visual type that draws one.
    Empty(Kind),
    Image(Art),
    Movie(Art),
    /// An index into [`Interface::fonts`].
    Text(usize),
    /// The model's build path.
    Model(Option<String>),
    /// The emitter's placement name.
    Emitter(Option<String>),
    /// A context in another archive (`global.bld`'s font, by export id).
    Imported(Kind, Import),
}

/// An object another archive holds, as an import names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    /// The namespace's FNV-1a 32 (`global.bld`'s).
    pub namespace: u32,
    /// The export id there.
    pub object: u32,
}

/// An image or movie source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Art {
    pub source: Option<String>,
    /// `textures/<sha256>.ktx2`, when the archive or its `.arc` holds the
    /// image and it decoded.
    pub texture: Option<String>,
    /// The image's size in pixels, with the texture.
    pub size: Option<[u32; 2]>,
    pub stream_archive_hash: i32,
    pub desired: [i32; 2],
}

/// One `StringInfo`: its English text and what a number fills its `@`s by,
/// as the disc stores them (`guac-interface`'s number fill says what each
/// means).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Str {
    pub text: String,
    /// `_flags`, raw: 0x400 a time, 0x800 has an `@`, 0x2000 commas.
    pub flags: i32,
    pub integer_digits: i32,
    pub decimal_digits: i32,
}

/// Which string a template shows: one of its archive's, or another's by import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextKey {
    /// An index into [`Interface::strings`].
    Local(usize),
    Imported(Import),
}

/// One `igBitmapFont`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Font {
    pub name: Option<String>,
    /// Its `FontDataInfo`'s source (`fonts/contemp.png`).
    pub source: Option<String>,
    /// Pixels at the font's own size.
    pub height: f32,
    pub base_line: f32,
    pub char_padding: f32,
    pub kern_amount: f32,
    pub color: [f32; 4],
    /// `textures/<sha256>.ktx2`.
    pub atlas: String,
    pub atlas_size: [u32; 2],
    /// In list order.
    pub glyphs: Vec<Glyph>,
}

/// One `igCharMetrics`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Glyph {
    /// A UTF-16 unit.
    pub char: u16,
    pub step: f32,
    pub offset: [f32; 2],
    pub size: [f32; 2],
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    /// Top-left, top-right, bottom-left, bottom-right.
    pub kern: [u8; 4],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{from_postcard, from_ron, to_postcard, to_ron};

    fn sample() -> Interface {
        Interface {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: crate::Sha256([7; 32]),
            },
            archive: "permanent/global.bld".into(),
            templates: vec![Template {
                name: "Generic_TextBox".into(),
                object: 7,
                properties: 0x20A9_4649,
                flags: 0,
                position: [0x140, -0x140],
                z_depth: 0.0,
                priority: -2.0,
                width: 237.0,
                height: 96.0,
                scale: [100.0; 3],
                color: [0x80, 0x80, 0x80, 0xFF],
                replace_color: [0x80, 0x80, 0x80, 0],
                num_value: 0,
                depth: 100.0,
                content: Content::Imported(
                    Kind::Text,
                    Import {
                        namespace: 0x2694_BE8E,
                        object: 0x33,
                    },
                ),
                string: Some(TextKey::Local(3)),
            }],
            fonts: vec![Font {
                name: Some("hdd:/tfb/content/fonts/contemp".into()),
                source: Some("fonts/contemp.png".into()),
                height: 82.0,
                base_line: 0.0,
                char_padding: 0.0,
                kern_amount: 0.0,
                color: [0.0; 4],
                atlas: "textures/ab.ktx2".into(),
                atlas_size: [1024, 1024],
                glyphs: vec![Glyph {
                    char: 0x41,
                    step: 31.0,
                    offset: [0.0, 0.0],
                    size: [31.0, 82.0],
                    uv_min: [0.0, 0.0],
                    uv_max: [0.03, 0.08],
                    kern: [0, 0, 1, 1],
                }],
            }],
            strings: vec![Str {
                text: "@ Gold".into(),
                flags: 0x2800,
                integer_digits: 5,
                decimal_digits: 1,
            }],
            refused: vec![("eruptor_up5b.png".into(), "no .arc entry".into())],
        }
    }

    #[test]
    fn an_interface_round_trips_in_both_formats() {
        let i = sample();
        assert_eq!(from_ron::<Interface>(&to_ron(&i).unwrap()).unwrap(), i);
        assert_eq!(
            from_postcard::<Interface>(&to_postcard(&i).unwrap()).unwrap(),
            i
        );
    }

    /// A string's settings come back as written, each field its own.
    #[test]
    fn a_strings_settings_round_trip() {
        let mut i = sample();
        i.strings.push(Str {
            text: "@".into(),
            flags: 0x400,
            integer_digits: 4,
            decimal_digits: 0,
        });
        for back in [
            from_ron::<Interface>(&to_ron(&i).unwrap()).unwrap(),
            from_postcard::<Interface>(&to_postcard(&i).unwrap()).unwrap(),
        ] {
            let got: Vec<(i32, i32, i32)> = back
                .strings
                .iter()
                .map(|s| (s.flags, s.integer_digits, s.decimal_digits))
                .collect();
            assert_eq!(got, [(0x2800, 5, 1), (0x400, 4, 0)]);
            assert_eq!(back.strings[0].text, "@ Gold");
        }
    }
}
