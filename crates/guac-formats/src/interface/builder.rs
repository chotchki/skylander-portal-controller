//! A synthetic interface archive for tests and fuzz seeds (`test-fixtures`):
//! one template of each kind (a container, an image held here, an image
//! streamed from elsewhere, a movie, text on this file's font and on
//! `global.bld`'s by import, a model, an emitter) and the font with three
//! glyphs. Fields are written at AT.1's offsets as literals.

use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;

/// Where [`sample`] put each object.
#[derive(Debug, Clone)]
pub struct Sample {
    pub bytes: Vec<u8>,
    /// In this order: container, image, streamed, movie, text, imported
    /// text, model, emitter.
    pub templates: [Location; 8],
    /// The local image's `igImage2`.
    pub image: Location,
    pub model: Location,
    pub emitter: Location,
    /// Two keys; the text template holds the second.
    pub strings: [Location; 2],
    pub font_data: Location,
    pub font: Location,
    pub atlas: Location,
    /// 'A', 'B' and ' ', in list order.
    pub glyphs: [Location; 3],
}

pub fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

pub fn floats(b: &mut IgzBuilder, base: Location, offset: u32, values: &[f32]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    b.put_bytes(at(base, offset), &bytes);
}

fn u16_be(b: &mut IgzBuilder, base: Location, offset: u32, values: &[u16]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    b.put_bytes(at(base, offset), &bytes);
}

/// A template's own fields: `properties` and a place, size and colours
/// derived from `n` so each template's differ.
pub fn template(b: &mut IgzBuilder, o: Location, name: &str, properties: u32, n: u16) {
    b.string(at(o, 0x08), name);
    b.put_u32(at(o, 0xA0), u32::from(n));
    u16_be(b, o, 0xA4, &[16 * n, 0xFFF0 - n]);
    b.put_u32(at(o, 0xA8), properties);
    let f = f32::from(n);
    floats(
        b,
        o,
        0xAC,
        &[f, -f, 10.0 * f, 5.0 * f, 100.0, 100.0 + f, 100.0],
    );
    let c = n as u8;
    b.put_bytes(at(o, 0xC8), &[0x80, 0x80, c, 0xFF, 0x80, 0x80, 0x80, c]);
    b.put_u32(at(o, 0xD8), u32::from(n) * 3);
    floats(b, o, 0xDC, &[100.0]);
}

/// An `AnimatableTextureDataInfo` named `source`; with `image`, its chain
/// down to it, else a bind to an image-less texture.
fn art(
    b: &mut IgzBuilder,
    data: Location,
    source: &str,
    stream: u32,
    desired: [i32; 2],
    image: Option<Location>,
) {
    let bind = b.object(0, "tfbAlphaTextureUnitIDAttr", 0x20);
    let texture = b.object(0, "igTextureAttr2", 0x40);
    b.string(at(data, 0x08), source);
    b.put_bytes(at(data, 0x10), &[1]);
    b.null_memory_ref(at(data, 0x14), 0);
    b.put_u32(at(data, 0x1C), stream);
    b.put_u32(at(data, 0x24), desired[0] as u32);
    b.put_u32(at(data, 0x28), desired[1] as u32);
    b.pointer(at(data, 0x30), bind);
    b.pointer(at(bind, 0x10), texture);
    b.put_u32(at(bind, 0x14), u32::MAX);
    b.put_u32(at(bind, 0x1C), u32::MAX);
    if let Some(image) = image {
        b.pointer(at(texture, 0x30), image);
    }
}

/// A 4x4 DXT5 `igImage2` named `name`.
pub fn image(b: &mut IgzBuilder, o: Location, blocks: u8, name: &str, fill: u8) {
    b.string(at(o, 0x08), name);
    // Colour scale 1: a texture refuses a zero one.
    b.put_bytes(at(o, 0x10), &1f32.to_be_bytes().repeat(4));
    u16_be(b, o, 0x30, &[4, 4, 1, 1, 1]);
    b.import(at(o, 0x3C), crate::igz::layouts::METAIMAGES, "dxt5_big_ps3");
    b.handle(at(o, 0x48), blocks, &[fill; 0x80], 5);
}

/// A glyph for `c`: advance `step`, its box `uv` (min x, min y, max x,
/// max y) and kern corners `kern`.
pub fn glyph(b: &mut IgzBuilder, o: Location, c: u16, kern: u16, step: f32, uv: [f32; 4]) {
    u16_be(b, o, 0x08, &[c, kern]);
    floats(
        b,
        o,
        0x0C,
        &[step, 1.0, 2.0, step - 1.0, 42.0, uv[0], uv[1], uv[2], uv[3]],
    );
}

pub fn sample() -> Sample {
    let mut b = IgzBuilder::new();
    let names = [
        "Generic_Container",
        "Autosave_Icon",
        "batteries_are_low",
        "ChampionIntroVideo_(000)",
        "Generic_TextBox",
        "UI_Text_Imported",
        "Damage_CritExclamation",
        "UI_LevelUp_ArrowParticle",
    ];
    let templates = names.map(|_| b.object(0, "tfbSpriteInfo", 0xE4));
    let datas = [(); 3].map(|_| b.object(0, "AnimatableTextureDataInfo", 0x34));
    let image_object = b.object(0, "igImage2", 0x60);
    let model = b.object(0, "tfbPhysicsModel", 0x30);
    let emitter = b.object(0, "tfbParticleInfo", 0xE0);
    let strings = [
        b.object(0, "StringInfo", 0x28),
        b.object(0, "StringInfo", 0x28),
    ];
    let font_data = b.object(0, "FontDataInfo", 0x24);
    let font = b.object(0, "igBitmapFont", 0x50);
    let glyph_list = b.object(0, "igCharMetricsList", 0x18);
    let glyphs = [(); 3].map(|_| b.object(0, "igCharMetrics", 0x30));
    let font_bind = b.object(0, "tfbAlphaTextureUnitIDAttr", 0x20);
    let font_texture = b.object(0, "igTextureAttr2", 0x40);
    let atlas = b.object(0, "igImage2", 0x60);
    let blocks = b.section(0x10);

    // Container, art (image, streamed, movie), text twice, model, particles;
    // anchors 6 on 6 for the box, 4 on 4 for the rest; visible, dirty.
    let props = [
        0x2001_660B,
        0x2001_4408,
        0x2001_4408,
        0x2001_4408,
        0x20A9_4449,
        0x20A9_4449,
        0x2001_440A,
        0x2001_440C,
    ];
    for (i, ((o, name), p)) in templates.iter().zip(names).zip(props).enumerate() {
        template(&mut b, *o, name, p, i as u16 + 1);
    }
    image(
        &mut b,
        image_object,
        blocks,
        "ui/autosave_portrait.png",
        0x5A,
    );
    art(
        &mut b,
        datas[0],
        "Levels/Includes/UI_Main/autoSave_portrait.png",
        0,
        [128, 128],
        Some(image_object),
    );
    art(
        &mut b,
        datas[1],
        "Levels/Includes/PortalMessaging/batteries_low_1.png",
        0x3E79_592D,
        [512, 512],
        None,
    );
    art(
        &mut b,
        datas[2],
        "Movies/Whirlwind.mov",
        0x41A9_AFEE,
        [-1, -1],
        None,
    );
    for (t, d) in templates[1..4].iter().zip(datas) {
        b.pointer(at(*t, 0xD0), d);
    }
    b.pointer(at(templates[4], 0xD0), font_data);
    b.pointer(at(templates[4], 0xD4), strings[1]);
    // global.bld's FontDataInfo, by export id.
    b.import_raw(
        at(templates[5], 0xD0),
        0x33,
        crate::fnv1a32(crate::exports::NAMESPACE.as_bytes()),
    );
    // And global.bld's string, by export id.
    b.import_raw(
        at(templates[5], 0xD4),
        0x40,
        crate::fnv1a32(crate::exports::NAMESPACE.as_bytes()),
    );
    b.pointer(at(templates[6], 0xD0), model);
    b.pointer(at(templates[7], 0xD0), emitter);

    b.string(
        at(model, 0x08),
        "C:/tfb/Content/Models/UI/Exclamation_Point3.mdl",
    );
    b.put_bytes(at(model, 0x10), &[1]);
    b.string(at(emitter, 0x08), "UI_LevelUp_ArrowParticle_VFX");
    b.string(at(strings[0], 0x08), "UI_Unused");
    b.string(at(strings[1], 0x08), "UI_Gold");
    // Their settings, each field distinct: a time of four digits, and a
    // number with an `@`, commas, five digits and one decimal.
    for (s, [flags, integer, decimal]) in strings.iter().zip([[0x400, 4, 0], [0x2800, 5, 1]]) {
        for (offset, v) in [(0x1C, flags), (0x20, integer), (0x24, decimal)] {
            b.put_u32(at(*s, offset), v);
        }
    }

    b.string(at(font_data, 0x08), "fonts/contemp.png");
    b.put_bytes(at(font_data, 0x10), &[1]);
    b.null_memory_ref(at(font_data, 0x14), 0);
    b.pointer(at(font_data, 0x20), font);
    b.string(at(font, 0x08), "hdd:/tfb/content/fonts/contemp");
    floats(&mut b, font, 0x0C, &[82.0, 9.0]);
    floats(&mut b, font, 0x30, &[0.5, 2.0]);
    b.pointer(at(font, 0x38), font_bind);
    b.pointer(at(font, 0x3C), glyph_list);
    list(&mut b, glyph_list, blocks, &glyphs.map(Some));
    glyph(&mut b, glyphs[0], 0x41, 0x0000, 30.0, [0.0, 0.0, 0.25, 0.5]);
    glyph(&mut b, glyphs[1], 0x42, 0x1100, 28.0, [0.25, 0.0, 0.5, 0.5]);
    glyph(&mut b, glyphs[2], 0x20, 0x2222, 16.0, [0.5, 0.5, 0.5, 0.5]);
    b.pointer(at(font_bind, 0x10), font_texture);
    b.put_u32(at(font_bind, 0x14), u32::MAX);
    b.put_u32(at(font_bind, 0x1C), u32::MAX);
    b.pointer(at(font_texture, 0x30), atlas);
    image(&mut b, atlas, blocks, "contemp.png", 0x33);

    Sample {
        bytes: b.build(),
        templates,
        image: image_object,
        model,
        emitter,
        strings,
        font_data,
        font,
        atlas,
        glyphs,
    }
}

/// The sample, plus a font alone and a file with neither.
pub fn seeds() -> Vec<Vec<u8>> {
    let mut b = IgzBuilder::new();
    let font = b.object(0, "igBitmapFont", 0x50);
    let glyph_list = b.object(0, "igCharMetricsList", 0x18);
    let one = b.object(0, "igCharMetrics", 0x30);
    let bind = b.object(0, "tfbAlphaTextureUnitIDAttr", 0x20);
    let texture = b.object(0, "igTextureAttr2", 0x40);
    let atlas = b.object(0, "igImage2", 0x60);
    let blocks = b.section(0x10);
    b.pointer(at(font, 0x38), bind);
    b.pointer(at(font, 0x3C), glyph_list);
    list(&mut b, glyph_list, blocks, &[Some(one)]);
    glyph(&mut b, one, 0x61, 0x4321, 12.0, [0.0, 0.0, 1.0, 1.0]);
    b.pointer(at(bind, 0x10), texture);
    b.pointer(at(texture, 0x30), atlas);
    image(&mut b, atlas, blocks, "simple.png", 0x11);
    let font_alone = b.build();

    let mut b = IgzBuilder::new();
    b.object(0, "igObjectList", 0x18);
    vec![sample().bytes, font_alone, b.build()]
}
