//! An IGZ holding one `igImage2` per metaimage, for tests and fuzz seeds
//! (`test-fixtures`). Byte `i` of each pixel block is `seed + i`.

use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::kind;
use crate::igz::layouts::METAIMAGES;

/// One image: metaimage, width, height, levels, pixel bytes, first byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub format: &'static str,
    pub width: u16,
    pub height: u16,
    pub levels: u16,
    pub len: usize,
    pub seed: u8,
}

/// Every metaimage once: tiled with mips, linear and not power-of-two.
pub const SPECS: [Spec; 5] = [
    spec("dxt1_tile_big_ps3", 8, 8, 4, 128, 0x10),
    spec("dxt5_tile_big_ps3", 16, 8, 2, 256, 0x20),
    spec("dxt5_big_ps3", 12, 6, 1, 128, 0x30),
    spec("dxt1_big_ps3", 6, 10, 1, 128, 0x40),
    spec("l8_tile_big_ps3", 8, 2, 1, 128, 0x50),
];

const fn spec(
    format: &'static str,
    width: u16,
    height: u16,
    levels: u16,
    len: usize,
    seed: u8,
) -> Spec {
    Spec {
        format,
        width,
        height,
        levels,
        len,
        seed,
    }
}

impl Spec {
    pub fn data(&self) -> Vec<u8> {
        (0..self.len)
            .map(|i| self.seed.wrapping_add(i as u8))
            .collect()
    }
}

pub struct Images {
    pub bytes: Vec<u8>,
    /// In [`SPECS`] order.
    pub images: Vec<Location>,
}

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

/// An `igImage2` at a fresh object, as Giants writes one.
pub fn image(b: &mut IgzBuilder, blocks: u8, s: &Spec, data: &[u8]) -> Location {
    let image = image_without_data(b, s);
    b.handle(at(image, 0x48), blocks, data, 5);
    image
}

/// [`image`] named `name`, as a character's textures are (`eruptor_n`).
pub fn named_image(b: &mut IgzBuilder, blocks: u8, s: &Spec, data: &[u8], name: &str) -> Location {
    let image = image_named(b, s, name);
    b.handle(at(image, 0x48), blocks, data, 5);
    image
}

/// [`image`] with `_data` left zero and unmarked.
fn image_without_data(b: &mut IgzBuilder, s: &Spec) -> Location {
    image_named(b, s, s.format)
}

fn image_named(b: &mut IgzBuilder, s: &Spec, name: &str) -> Location {
    let image = b.object(0, "igImage2", 0x60);
    b.string(at(image, 0x08), name);
    let one = 1f32.to_be_bytes().repeat(4);
    b.put_bytes(at(image, 0x10), &one);
    let dims: Vec<u8> = [s.width, s.height, 1, s.levels, 1]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    b.put_bytes(at(image, 0x30), &dims);
    b.import(at(image, 0x3C), METAIMAGES, s.format);
    b.put_u32(at(image, 0x40), 1);
    b.put_u32(at(image, 0x50), u32::MAX);
    b.put_bytes(at(image, 0x58), &[1]);
    image
}

pub fn images() -> Images {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x80);
    let images = SPECS
        .iter()
        .map(|s| image(&mut b, blocks, s, &s.data()))
        .collect();
    Images {
        bytes: b.build(),
        images,
    }
}

/// All five together, then each alone.
pub fn seeds() -> Vec<Vec<u8>> {
    let alone = SPECS.iter().map(|s| {
        let mut b = IgzBuilder::new();
        let blocks = b.section(0x80);
        image(&mut b, blocks, s, &s.data());
        b.build()
    });
    std::iter::once(images().bytes).chain(alone).collect()
}

/// `count` square `l8_tile` images all pointing at one `side` x `side` block,
/// through one TMHN handle: input that asks for `count` untiles of it.
pub fn shared_l8(count: usize, side: u16) -> Vec<u8> {
    let s = Spec {
        format: "l8_tile_big_ps3",
        width: side,
        height: side,
        levels: 1,
        len: usize::from(side) * usize::from(side),
        seed: 0x60,
    };
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x80);
    let first = image_without_data(&mut b, &s);
    let handle = b.handle(at(first, 0x48), blocks, &s.data(), 5);
    for _ in 1..count {
        let other = image_without_data(&mut b, &s);
        b.put_u32(at(other, 0x48), handle);
        b.marker(kind::HANDLES, at(other, 0x48));
    }
    b.build()
}
