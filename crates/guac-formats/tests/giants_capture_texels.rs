//! AC.21: textures checked against RSX's own reading of a bound unit. RPCS3's reading is
//! `rrc_dump::texels`: its level layout and, for an `LN`-clear B8 unit, its Morton walk, in code
//! that shares nothing with this crate's untile. For DXT, which RSX never reorders, that reading is
//! the unit's bytes in rows, so a DXT check is those bytes plus the unit's `LN` bit, pitch, size
//! and level count; only L8 moves bytes, and only the first test below exercises that walk until a
//! capture binds an L8 (AC.24).
//!
//! - [`untile_agrees_with_rpcs3s_reading_on_every_power_of_two_l8`] runs in `cargo test`: the
//!   crate's L8 untile against RPCS3's walk on every power-of-two shape up to 512 a side, whole
//!   mip chains.
//! - [`eruptor_upgrade_art_matches_the_captures`] is `#[ignore]`d: Eruptor's 16 upgrade-menu
//!   images (`character/009_eruptor.arc`) against four RSX captures of the Skylander Info >
//!   Upgrades page, taken unattended on AF's oracle in AC.21's session (2026-10-18). It reads
//!   `GUAC_GIANTS_DUMP` and `GUAC_AC21_CAPTURE_DIR` (the folder holding that session's
//!   captures, pinned by name in [`CAPTURES`]; its own key, as AC.13's `GUAC_AC13_CAPTURE_DIR` is
//!   `giants_capture`'s), each from the environment, else `.env.dev`:
//!
//! `cargo test -p guac-formats --release --test giants_capture_texels -- --ignored --nocapture`
//!
//! Everything read stays in memory; nothing is written.

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::time::Instant;

use common::{env_or_dotenv, usrdir};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{self, Image2};
use guac_formats::texture::{Format, MetaImage, Texture};
use rrc_dump::Capture;
use rrc_dump::replay::TextureUnit;
use rrc_dump::texels;

/// AC.21's four captures of the Upgrades page, by name (timestamps, not game bytes). The first
/// three hold a fresh Series 1 Eruptor with the carousel at its start, a third of the way along
/// and at its end; the fourth a fresh Series 2 Eruptor at the end, where the Wow Pow sits.
const CAPTURES: [&str; 4] = [
    "BLUS30968_20261018165718_capture.rrc.gz",
    "BLUS30968_20261018165828_capture.rrc.gz",
    "BLUS30968_20261018165844_capture.rrc.gz",
    "BLUS30968_20261018170138_capture.rrc.gz",
];
const KEY: &str = "GUAC_AC21_CAPTURE_DIR";
/// The two branch images (1024x236): `tfbSpriteInfo` `009_UpgradeBranchImage_1`/`_2` in
/// `009_eruptor.bld`, drawn where a path is offered or bought, so not on a fresh figure's page.
const BRANCHES: [&str; 2] = ["eruptor_upgradesa.png", "eruptor_upgradesb.png"];

/// Deterministic noise, so a misplaced texel can't hide among equal ones.
fn noise(n: usize, seed: u32) -> Vec<u8> {
    let mut s = seed.wrapping_mul(0x9E37_79B9) | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s >> 24) as u8
        })
        .collect()
}

/// A bound 2D unit as `rrc_dump` reports it, around `bytes`.
fn unit(format_raw: u32, w: u32, h: u32, mips: u32, pitch: u32, bytes: Vec<u8>) -> TextureUnit {
    let base = format_raw & !0x60;
    let linear = format_raw & 0x20 != 0;
    TextureUnit {
        unit: 0,
        offset: 0,
        location: 0,
        format_raw,
        format_name: rrc_dump::nv4097::texture_format_name(base),
        linear,
        swizzled: !linear && !rrc_dump::nv4097::never_swizzled(base),
        unnormalized: false,
        dimension: 2,
        cubemap: false,
        border_texels: false,
        mipmaps: mips,
        width: w,
        height: h,
        depth: 1,
        pitch,
        remap: 0xaae4,
        sampler: Default::default(),
        captured: true,
        bytes,
    }
}

#[test]
fn untile_agrees_with_rpcs3s_reading_on_every_power_of_two_l8() {
    let l8 = MetaImage::by_name("l8_tile_big_ps3").unwrap();
    let mut shapes = 0;
    for lw in 0..=9 {
        for lh in 0..=9 {
            let (w, h) = (1u32 << lw, 1u32 << lh);
            let levels = 1 + lw.max(lh);
            let chain = Format::R8Unorm.chain_size(w, h, levels) as usize;
            let data = noise(chain.next_multiple_of(128), w * 1031 + h);
            let ours = l8.untile(w as u16, h as u16, levels as u16, &data).unwrap();
            // RSX's reading of the same bytes bound as an LN-clear B8 unit.
            let rsx = texels::levels(&unit(0x81, w, h, levels, 0, data)).unwrap();
            assert_eq!(ours.levels, rsx, "{w}x{h}, {levels} levels");
            shapes += 1;
        }
    }
    assert_eq!(shapes, 100);
}

struct DiscImage {
    name: String,
    texture: Texture,
    /// The KTX2 we write, read back level by level.
    ktx: Vec<Vec<u8>>,
}

/// Every `igImage2` in Eruptor's `.arc`, named by its file (the images carry no `_name`).
fn upgrade_art(usrdir: &Path) -> Vec<DiscImage> {
    let bytes = std::fs::read(usrdir.join("character/009_eruptor.arc")).unwrap();
    let archive = Archive::parse(&bytes).unwrap();
    let mut out = Vec::new();
    for entry in archive.entries() {
        let data = archive.read(entry).unwrap();
        if !data.starts_with(b"IGZ\x01") {
            continue;
        }
        let igz = Igz::parse(&data).unwrap();
        for (i, image) in layouts::read_all::<Image2>(&igz) {
            let image = image.unwrap();
            assert_eq!(
                image.format_name(),
                Some("dxt5_big_ps3"),
                "{}",
                entry.name()
            );
            let texture = Texture::read(&igz, i).unwrap();
            let file = texture.to_ktx2();
            let reader = ktx2::Reader::new(&file[..]).unwrap();
            let ktx = reader.levels().map(|l| l.data.to_vec()).collect();
            let name = entry.name().rsplit('/').nth(1).unwrap().to_lowercase();
            out.push(DiscImage { name, texture, ktx });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The crate's reading of a captured surface, picked by the unit's own description (`LN` clear =
/// `_tile`), as `giants_capture` reads AC.13's: `None` where the crate has no reading (a format it
/// doesn't convert, a linear pitch wider than the row).
fn crate_reading(t: &TextureUnit) -> Option<Texture> {
    let format = match t.format_raw & !0x60 {
        0x86 => Format::Bc1RgbaUnorm,
        0x88 => Format::Bc3Unorm,
        0x81 => Format::R8Unorm,
        _ => return None,
    };
    let meta = MetaImage {
        name: "rsx",
        format,
        tiled: !t.linear,
    };
    if t.linear && u64::from(t.pitch) != format.level_size(t.width, 1) {
        return None;
    }
    let chain = format.chain_size(t.width, t.height, t.mipmaps) as usize;
    let mut data = t.bytes.get(..chain)?.to_vec();
    data.resize(chain.next_multiple_of(128), 0);
    meta.untile(t.width as u16, t.height as u16, t.mipmaps as u16, &data)
        .ok()
}

fn key(t: &TextureUnit) -> (u32, u32, u32, u32, u32, u32, u32, u64) {
    let mut h = DefaultHasher::new();
    t.bytes.hash(&mut h);
    (
        t.location,
        t.offset,
        t.format_raw,
        t.width,
        t.height,
        t.mipmaps,
        t.pitch,
        h.finish(),
    )
}

fn load_average() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into())
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP and AC.21's captures at GUAC_AC21_CAPTURE_DIR"]
fn eruptor_upgrade_art_matches_the_captures() {
    let t0 = Instant::now();
    let dir = env_or_dotenv(KEY).unwrap_or_else(|| {
        panic!("set {KEY} (env or .env.dev) to the folder holding AC.21's captures")
    });
    let art = upgrade_art(&usrdir());
    assert_eq!(art.len(), 16);
    for a in &art {
        let t = &a.texture;
        assert_eq!(
            (t.format, t.levels.len()),
            (Format::Bc3Unorm, 1),
            "{}",
            a.name
        );
        let wide = BRANCHES.contains(&a.name.as_str());
        assert_eq!(
            (t.width, t.height),
            if wide { (1024, 236) } else { (512, 360) }
        );
    }
    println!(
        "disc: Eruptor's upgrade art, {} images, all dxt5_big (linear), 1 level ({:.2} s)",
        art.len(),
        t0.elapsed().as_secs_f64()
    );

    let mut covered: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (c, name) in CAPTURES.iter().enumerate() {
        let t1 = Instant::now();
        let path = Path::new(&dir).join(name);
        assert!(path.is_file(), "no {name} in {dir}");
        let capture = Capture::from_file_bytes(&std::fs::read(&path).unwrap()).unwrap();
        // Every distinct surface any draw binds, with how many unit states held it.
        let mut surfaces: HashMap<_, (TextureUnit, usize)> = HashMap::new();
        let stats = rrc_dump::replay(&capture, |d| -> Result<(), ()> {
            for t in d.textures {
                surfaces
                    .entry(key(&t))
                    .and_modify(|e: &mut (TextureUnit, usize)| e.1 += 1)
                    .or_insert((t, 1));
            }
            Ok(())
        })
        .unwrap();
        let (mut read, mut agree, mut b8) = (0, 0, 0);
        let mut unread: BTreeMap<String, usize> = BTreeMap::new();
        let mut here = BTreeSet::new();
        for (t, states) in surfaces.values() {
            assert!(
                t.captured,
                "{name}: unit {} at {:#x} not captured",
                t.unit, t.offset
            );
            b8 += usize::from(t.format_raw & !0x60 == 0x81);
            let rsx = match texels::levels(t) {
                Ok(l) => l,
                Err(e) => {
                    *unread.entry(e.to_string()).or_default() += 1;
                    continue;
                }
            };
            read += 1;
            // Two readings written apart: where both read a surface, a difference is a bug in one.
            if let Some(ours) = crate_reading(t) {
                assert_eq!(
                    ours.levels, rsx,
                    "{name}: {:#x} {}x{}",
                    t.offset, t.width, t.height
                );
                agree += 1;
            }
            let Some(a) = art.iter().find(|a| a.ktx == rsx) else {
                // A DXT5 unit the size of one of his images is one of them, or a mismatch would
                // pass as "not bound".
                let size = (t.width, t.height);
                assert!(
                    t.format_raw & !0x60 != 0x88
                        || !art
                            .iter()
                            .any(|a| (a.texture.width, a.texture.height) == size),
                    "{name}: DXT5 {}x{} at {:#x} is the size of his upgrade art but equals none of it",
                    t.width,
                    t.height,
                    t.offset
                );
                continue;
            };
            // One of his: bound as the linear `_big` image it is on the disc, rows at the packed
            // pitch, and RSX's reading equal to the KTX2 level we write.
            let at = format!("{name}: {} at {:#x}", a.name, t.offset);
            assert_eq!(t.format_raw, 0x88 | 0x20, "{at}: DXT5 with LN");
            assert_eq!(
                (t.width, t.height, t.mipmaps),
                (a.texture.width, a.texture.height, 1),
                "{at}"
            );
            assert_eq!(t.pitch, a.texture.width / 4 * 16, "{at}: packed pitch");
            assert_eq!((t.remap, t.border_texels), (0xaae4, false), "{at}");
            here.insert(a.name.as_str());
            covered.entry(a.name.as_str()).or_default().push(c);
            println!(
                "  {}: u{} {:#010x} fmt {:#x} {}x{} pitch {} mips {} remap {:#x}, {states} unit states, RSX's reading = our KTX2",
                a.name,
                t.unit,
                t.offset,
                t.format_raw,
                t.width,
                t.height,
                t.pitch,
                t.mipmaps,
                t.remap
            );
        }
        println!(
            "{name}: {} draws bind {} distinct surfaces; RSX's reading read {read} ({agree} also read by the crate, equal), B8 units {b8}, not read {unread:?}; his upgrade art {} ({:.2} s, load average {})",
            stats.draws,
            surfaces.len(),
            here.len(),
            t1.elapsed().as_secs_f64(),
            load_average()
        );
        assert!(!here.is_empty(), "{name} binds none of his upgrade art");
    }
    let missing: Vec<&str> = art
        .iter()
        .map(|a| a.name.as_str())
        .filter(|n| !covered.contains_key(n))
        .collect();
    println!(
        "Eruptor's upgrade art bound and equal: {} of {}; not bound in these frames: {missing:?} (the branch images, AC.25)",
        covered.len(),
        art.len()
    );
    // Everything but the two branch images, which no unit binds at their size (the check above),
    // so a capture that adds them shows up here.
    assert_eq!(missing, BRANCHES);
    assert_eq!(
        covered["eruptor_upgradepowwow.png"],
        [3],
        "the Wow Pow is Series 2's"
    );
    println!("AC.21 done in {:.2} s", t0.elapsed().as_secs_f64());
}
