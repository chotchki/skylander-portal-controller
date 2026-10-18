//! AC.6's textures off the Giants dump: every `igImage2` in the gate hero's
//! archives, then in every character archive, untiled and written as KTX2
//! that the `ktx2` crate reads back, with the evidence for the untile rules.
//! Ignored by default; it reads `GUAC_GIANTS_DUMP` like `giants_igz`:
//!
//! `cargo test -p guac-formats --release --test giants_textures -- --ignored --nocapture`

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::usrdir;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{Image2, read_all};
use guac_formats::texture::{Format, MetaImage, Texture, morton};

#[derive(Default)]
struct Census {
    igz: u64,
    images: u64,
    ktx2_bytes: u64,
    failures: Vec<String>,
    counts: BTreeMap<String, u64>,
    /// Per format (DXT1, DXT5): color blocks with distinct endpoints, and
    /// those with c0 > c1 read little-endian, then big-endian.
    endpoints: [[u64; 3]; 2],
    /// Tiled DXT images whose blocks are smoother as rows than as Morton.
    rows_smoother: [u64; 2],
    /// Mip 1 endpoint distance to the same spot of mip 0, and to the mirror.
    mip_distance: [f64; 2],
    /// Tiled L8 rows: identical after the unswizzle, out of all.
    l8_rows_identical: [u64; 2],
}

/// Endpoint 0 of block `i` as 5:6:5 RGB, little-endian.
fn endpoint(level: &[u8], format: Format, i: usize) -> [i32; 3] {
    let at = i * format.block_bytes() as usize + if format == Format::Bc3Unorm { 8 } else { 0 };
    let v = u16::from_le_bytes([level[at], level[at + 1]]);
    [
        (v >> 11) as i32,
        ((v >> 5) & 63) as i32 / 2,
        (v & 31) as i32,
    ]
}

fn distance(a: [i32; 3], b: [i32; 3]) -> f64 {
    a.iter().zip(&b).map(|(x, y)| (x - y).abs()).sum::<i32>() as f64
}

impl Census {
    fn bump(&mut self, key: impl Into<String>) {
        *self.counts.entry(key.into()).or_default() += 1;
    }

    fn take(&mut self, name: &str, ext: &str, bytes: &[u8]) {
        let igz = match Igz::parse(bytes) {
            Ok(igz) => igz,
            Err(e) => return self.failures.push(format!("{name}: {e}")),
        };
        self.igz += 1;
        for (i, image) in read_all::<Image2>(&igz) {
            self.images += 1;
            let image = match image {
                Ok(image) => image,
                Err(e) => {
                    self.failures.push(format!("{name}: igImage2 #{i}: {e}"));
                    continue;
                }
            };
            let fmt = image.format_name().unwrap_or("unknown");
            self.bump(format!("format {fmt} in .{ext}"));
            let t = match Texture::read(&igz, i) {
                Ok(t) => t,
                Err(e) => {
                    self.failures
                        .push(format!("{name}: {:?} ({fmt}): {e}", image.name));
                    continue;
                }
            };
            let meta = MetaImage::by_name(fmt).unwrap();
            let pow2 = t.width.is_power_of_two() && t.height.is_power_of_two();
            self.bump(format!(
                "{} images: power of two {pow2}, levels {}",
                if meta.tiled { "tiled" } else { "linear" },
                if t.levels.len() > 1 { "> 1" } else { "1" }
            ));
            if !self.ktx2_holds(&t) {
                self.failures
                    .push(format!("{name}: {:?}: KTX2 read back differs", image.name));
            }
            self.evidence(&t, meta);
        }
    }

    fn ktx2_holds(&mut self, t: &Texture) -> bool {
        let file = t.to_ktx2();
        self.ktx2_bytes += file.len() as u64;
        let Ok(r) = ktx2::Reader::new(&file[..]) else {
            return false;
        };
        let h = r.header();
        h.format.map(|f| f.value()) == Some(t.format.vk_format())
            && (h.pixel_width, h.pixel_height, h.level_count)
                == (t.width, t.height, t.levels.len() as u32)
            && r.levels()
                .map(|l| l.data)
                .eq(t.levels.iter().map(Vec::as_slice))
    }

    fn evidence(&mut self, t: &Texture, meta: &MetaImage) {
        let l0 = &t.levels[0];
        if t.format == Format::R8Unorm {
            if t.height == 2 {
                let w = t.width as usize;
                self.l8_rows_identical[0] += u64::from(l0[..w] == l0[w..]);
                self.l8_rows_identical[1] += 1;
            }
            return;
        }
        let (bw, bh) = (t.width.div_ceil(4), t.height.div_ceil(4));
        let bs = t.format.block_bytes() as usize;
        let color = if t.format == Format::Bc3Unorm { 8 } else { 0 };
        let tally = &mut self.endpoints[usize::from(color == 8)];
        for b in 0..(bw * bh) as usize {
            let at = b * bs + color;
            let (c0, c1) = (&l0[at..at + 2], &l0[at + 2..at + 4]);
            if c0 == c1 {
                continue;
            }
            tally[0] += 1;
            let le = |c: &[u8]| u16::from_le_bytes([c[0], c[1]]);
            let be = |c: &[u8]| u16::from_be_bytes([c[0], c[1]]);
            tally[1] += u64::from(le(c0) > le(c1));
            tally[2] += u64::from(be(c0) > be(c1));
        }
        if !meta.tiled || bw < 2 || bh < 2 {
            return;
        }
        // Neighbours along each row, as stored and as if Morton-ordered.
        let (mut rows, mut z) = (0.0, 0.0);
        for y in 0..bh {
            for x in 0..bw - 1 {
                let at = |i: u32| endpoint(l0, t.format, i as usize);
                rows += distance(at(y * bw + x), at(y * bw + x + 1));
                let m = |x: u32| endpoint(l0, t.format, morton(x, y, bw, bh));
                z += distance(m(x), m(x + 1));
            }
        }
        self.rows_smoother[0] += u64::from(rows < z);
        self.rows_smoother[1] += 1;
        if t.levels.len() > 1 && bw >= 4 && bh >= 4 {
            let l1 = &t.levels[1];
            let (bw1, bh1) = (bw / 2, bh / 2);
            let n = f64::from(bw1 * bh1);
            for y in 0..bh1 {
                for x in 0..bw1 {
                    let a = endpoint(l1, t.format, (y * bw1 + x) as usize);
                    let same = endpoint(l0, t.format, (2 * y * bw + 2 * x) as usize);
                    let mirror = (bh - 1 - 2 * y) * bw + (bw - 1 - 2 * x);
                    self.mip_distance[0] += distance(a, same) / n;
                    self.mip_distance[1] +=
                        distance(a, endpoint(l0, t.format, mirror as usize)) / n;
                }
            }
        }
    }

    fn take_archive(&mut self, path: &Path) {
        let ext = path.extension().unwrap().to_string_lossy().into_owned();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(path).unwrap();
        let archive = Archive::parse(&bytes).unwrap();
        for entry in archive.entries() {
            let data = archive.read(entry).unwrap();
            if data.starts_with(b"IGZ\x01") {
                self.take(&format!("{name}/{}", entry.name()), &ext, &data);
            }
        }
    }

    fn print(&self, title: &str) {
        println!(
            "{title}: {} IGZ, {} igImage2, {} KTX2 bytes",
            self.igz, self.images, self.ktx2_bytes
        );
        for (k, n) in &self.counts {
            println!("  {k}: {n}");
        }
        for (name, [d, le, be]) in ["DXT1", "DXT5"].iter().zip(self.endpoints) {
            println!(
                "  {name} color blocks with distinct endpoints: {d}, c0 > c1 little-endian {le}, big-endian {be}"
            );
        }
        let [r, n] = self.rows_smoother;
        println!("  tiled DXT smoother as rows than as Morton: {r} of {n}");
        let [same, mirror] = self.mip_distance;
        println!("  mip 1 endpoint distance to mip 0, same spot {same:.0}, mirrored {mirror:.0}");
        let [i, n] = self.l8_rows_identical;
        println!("  tiled L8 two-row images with identical rows after the unswizzle: {i} of {n}");
        println!("  failures: {}", self.failures.len());
        for f in self.failures.iter().take(40) {
            println!("    {f}");
        }
    }
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_eruptor_texture_converts() {
    let dir = usrdir().join("character");
    let mut c = Census::default();
    for file in ["009_eruptor.bld", "009_eruptor.arc"] {
        c.take_archive(&dir.join(file));
    }
    c.print("Eruptor (009_eruptor.bld + .arc)");
    assert!(c.failures.is_empty());
    assert_eq!(c.images, 60);
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_character_texture_converts() {
    let mut paths: Vec<_> = std::fs::read_dir(usrdir().join("character"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "bld" || e == "arc"))
        .collect();
    paths.sort();
    let mut c = Census::default();
    for path in &paths {
        c.take_archive(path);
    }
    c.print(&format!("{} character archives", paths.len()));
    assert!(c.failures.is_empty());
    assert!(c.images > 0);
    // Read little-endian, every DXT5 color block is four-color and most DXT1
    // ones are: the bytes are already in BC order.
    let [[d1, le1, _], [d5, le5, _]] = c.endpoints;
    assert_eq!(le5, d5);
    assert!(le1 * 100 > d1 * 80, "{le1} of {d1}");
    let [r, n] = c.rows_smoother;
    assert!(r * 100 > n * 80, "{r} of {n}");
    assert!(c.mip_distance[0] * 2.0 < c.mip_distance[1]);
    assert_eq!(c.l8_rows_identical[0], c.l8_rows_identical[1]);
}
