//! Synthetic emitters for tests and fuzz seeds (`test-fixtures`): one with
//! two sprite generators and a ribbon (the shape of the disc's ball trail),
//! two of them sharing a texture, plus a lightning generator the reader
//! keeps unread; one with no generator list. Every field holds a value of
//! its own, written at the offsets AH.87's layouts give as literals, so a
//! wrong reader constant shows.

use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;

/// Where [`sample`] put each object.
#[derive(Debug, Clone)]
pub struct Sample {
    pub bytes: Vec<u8>,
    /// "Trail": the list below.
    pub emitter: Location,
    /// "Quiet": no generator list.
    pub quiet: Location,
    pub list: Location,
    /// Two `GeneratorInfo`s: the first textured "smoke", the second
    /// "spark".
    pub sprites: [Location; 2],
    /// Their generation, movement and appearance records.
    pub records: [[Location; 3]; 2],
    /// A `RibbonGenerator` drawing "smoke" too.
    pub ribbon: Location,
    /// A `LightningGenerator`, past the reader's kinds.
    pub lightning: Location,
    /// The two `AnimatableTextureDataInfo`s and their images.
    pub textures: [Location; 2],
    pub images: [Location; 2],
}

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

fn floats(b: &mut IgzBuilder, base: Location, offset: u32, values: &[f32]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    b.put_bytes(at(base, offset), &bytes);
}

/// A texture chain: the data info named `path`, its bind, its texture
/// attribute (mag 1, min 5, wrap 0 and 1) and a 4x4 DXT5 image.
fn texture(b: &mut IgzBuilder, blocks: u8, path: &str, fill: u8) -> (Location, Location) {
    texture_sized(b, blocks, path, fill, 4, 4)
}

/// [`texture`] with a `width` x `height` image (at most 16 x 8).
fn texture_sized(
    b: &mut IgzBuilder,
    blocks: u8,
    path: &str,
    fill: u8,
    width: u8,
    height: u8,
) -> (Location, Location) {
    let data = b.object(0, "AnimatableTextureDataInfo", 0x34);
    let bind = b.object(0, "tfbAlphaTextureUnitIDAttr", 0x20);
    let attr = b.object(0, "igTextureAttr2", 0x40);
    let image = b.object(0, "igImage2", 0x60);
    b.string(at(data, 0x08), path);
    b.put_bytes(at(data, 0x10), &[1]);
    b.null_memory_ref(at(data, 0x14), 0);
    b.put_u32(at(data, 0x24), 128);
    b.put_u32(at(data, 0x28), 64);
    b.pointer(at(data, 0x30), bind);
    b.pointer(at(bind, 0x10), attr);
    b.put_u32(at(attr, 0x18), 1);
    b.put_u32(at(attr, 0x1C), 5);
    b.put_u32(at(attr, 0x24), 1);
    b.pointer(at(attr, 0x30), image);
    b.string(at(image, 0x08), path);
    b.put_bytes(at(image, 0x10), &1f32.to_be_bytes().repeat(4));
    b.put_bytes(at(image, 0x30), &[0, width, 0, height, 0, 1, 0, 1, 0, 1]);
    b.import(at(image, 0x3C), "metaimages", "dxt5_big_ps3");
    b.handle(at(image, 0x48), blocks, &[fill; 0x80], 5);
    (data, image)
}

/// `AbstractVFXGenerator`'s fields: the name (none when empty), `properties`, a bloom of
/// `bloom`, an initial matrix and a start delay.
fn vfx(b: &mut IgzBuilder, o: Location, name: &str, properties: u32, bloom: f32, data: Location) {
    if !name.is_empty() {
        b.string(at(o, 0x08), name);
    }
    b.put_u32(at(o, 0x1C), properties);
    floats(b, o, 0x20, &[bloom, 0.25, 0.5, 0.75, 10.0, 180.0, 30.0]);
    b.pointer(at(o, 0x3C), data);
    floats(b, o, 0x40, &[0.125]);
}

pub fn sample() -> Sample {
    let mut b = IgzBuilder::new();
    let emitter = b.object(0, "tfbParticleInfo", 0xE0);
    let quiet = b.object(0, "tfbParticleInfo", 0xE0);
    let gil = b.object(0, "GeneratorInfoList", 0x18);
    let sprites = [(); 2].map(|_| b.object(0, "GeneratorInfo", 0x54));
    let records = [(); 2].map(|_| {
        [
            b.object(0, "GeneratorGenerationFields", 0x34),
            b.object(0, "GeneratorMovementFields", 0x4C),
            b.object(0, "GeneratorAppearanceFields", 0x9C),
        ]
    });
    let ribbon = b.object(0, "RibbonGenerator", 0x60);
    let lightning = b.object(0, "LightningGenerator", 0x68);
    let blocks = b.section(0x10);
    let smoke = texture(&mut b, blocks, "Particles/textures/smoke.png", 0x11);
    let spark = texture(&mut b, blocks, "Particles/textures/spark.png", 0x22);

    crate::level::builder::placement(
        &mut b,
        emitter,
        "Trail",
        9,
        &[1.0, 2.0, 3.0, 0.0, 90.0, 0.0],
        5,
    );
    b.pointer(at(emitter, 0xD8), gil);
    crate::level::builder::placement(
        &mut b,
        quiet,
        "Quiet",
        10,
        &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        5,
    );
    list(
        &mut b,
        gil,
        blocks,
        &[
            Some(sprites[0]),
            Some(lightning),
            Some(sprites[1]),
            Some(ribbon),
        ],
    );

    // Additive (2) on overlay layer 5; blended (1) on layer 0.
    vfx(&mut b, sprites[0], "", 0x16, 0.0, smoke.0);
    vfx(&mut b, sprites[1], "sparks", 0x01, 5.0, spark.0);
    for (s, r) in sprites.iter().zip(&records) {
        b.pointer(at(*s, 0x44), r[0]);
        b.pointer(at(*s, 0x48), r[1]);
        b.pointer(at(*s, 0x50), r[2]);
    }
    for (k, r) in records.iter().enumerate() {
        let k = k as f32;
        // Generation: rate, max alive 24 + k, life, its randomness, the
        // distance, the midpoint and the scale.
        floats(&mut b, r[0], 0x18, &[40.0 + k]);
        b.put_u32(at(r[0], 0x1C), (24 + k as u32) << 16);
        floats(&mut b, r[0], 0x20, &[0.5 + k, 0.125, 25.0, 35.0 + k, 1.0]);
        // Movement: inherits translation, a sphere, outward, on the first;
        // uniform, a cylinder, inward, on the second.
        b.put_u32(at(r[1], 0x18), if k == 0.0 { 0x16 } else { 0x29 });
        floats(
            &mut b,
            r[1],
            0x1C,
            &[
                0.1,
                0.2,
                0.3,
                1.5 + k,
                0.25,
                0.0873,
                0.5,
                0.75,
                -6.0,
                -12.0,
                0.625,
                -9.0,
            ],
        );
        // Appearance: start, its randomness, end, its randomness (bytes);
        // sizes; spins; the middle's; the properties.
        b.put_bytes(
            at(r[2], 0x18),
            &[
                0x7F, 0x40, 0x00, 0xFF, 0x10, 0x20, 0x30, 0x00, 0x7F, 0x3A, 0x01, 0x00, 0x01, 0x02,
                0x03, 0x04,
            ],
        );
        floats(
            &mut b,
            r[2],
            0x28,
            &[
                0.45, 0.46, 0.47, 0.15, 0.16, 0.17, 0.06, 0.07, 0.08, 0.11, 0.12, 0.13, 1.57, 6.0,
                5.06, 0.5, 13.96, 0.25,
            ],
        );
        b.put_bytes(
            at(r[2], 0x70),
            &[0x7F, 0x39, 0x00, 0xFF, 0x05, 0x06, 0x07, 0x08],
        );
        floats(
            &mut b,
            r[2],
            0x78,
            &[0.30, 0.31, 0.32, 0.01, 0.02, 0.03, 5.24, 0.75],
        );
        b.put_u32(at(r[2], 0x98), if k == 0.0 { 0x09 } else { 0x04 });
    }

    // The ribbon: blended, taper end, 7 segments; its scroll, scale, width,
    // head and tail colours and granularity.
    vfx(&mut b, ribbon, "ribbon", 0x15, 0.0, smoke.0);
    b.put_u32(at(ribbon, 0x44), 0x39);
    floats(&mut b, ribbon, 0x48, &[12.5, 100.0, 1.0]);
    b.put_bytes(
        at(ribbon, 0x54),
        &[0xFF, 0x66, 0x00, 0xFF, 0xFF, 0x9D, 0x00, 0x2D],
    );
    floats(&mut b, ribbon, 0x5C, &[100.0]);

    Sample {
        bytes: b.build(),
        emitter,
        quiet,
        list: gil,
        sprites,
        records,
        ribbon,
        lightning,
        textures: [smoke.0, spark.0],
        images: [smoke.1, spark.1],
    }
}

/// One emitter, "Arrow", with one sprite generator whose image is 6 x 4:
/// no whole number of 4 x 4 blocks across, as `global.bld`'s level-up
/// arrow (109 x 128) isn't.
pub fn odd_image() -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let emitter = b.object(0, "tfbParticleInfo", 0xE0);
    let gil = b.object(0, "GeneratorInfoList", 0x18);
    let sprite = b.object(0, "GeneratorInfo", 0x54);
    let records = [
        b.object(0, "GeneratorGenerationFields", 0x34),
        b.object(0, "GeneratorMovementFields", 0x4C),
        b.object(0, "GeneratorAppearanceFields", 0x9C),
    ];
    let blocks = b.section(0x10);
    let (data, _) = texture_sized(&mut b, blocks, "levelup_arrow.png", 0x33, 6, 4);
    crate::level::builder::placement(&mut b, emitter, "Arrow", 1, &[0.0; 6], 5);
    b.pointer(at(emitter, 0xD8), gil);
    list(&mut b, gil, blocks, &[Some(sprite)]);
    vfx(&mut b, sprite, "", 0x16, 0.0, data);
    for (r, offset) in records.iter().zip([0x44, 0x48, 0x50]) {
        b.pointer(at(sprite, offset), *r);
    }
    b.build()
}

/// The fuzz seeds: the sample, and the sample's emitter alone with its list
/// pointing at a record instead of a list.
pub fn seeds() -> Vec<Vec<u8>> {
    let mut b = IgzBuilder::new();
    let emitter = b.object(0, "tfbParticleInfo", 0xE0);
    let record = b.object(0, "GeneratorGenerationFields", 0x34);
    crate::level::builder::placement(
        &mut b,
        emitter,
        "Bent",
        1,
        &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        5,
    );
    b.pointer(at(emitter, 0xD8), record);
    vec![sample().bytes, b.build(), odd_image()]
}
