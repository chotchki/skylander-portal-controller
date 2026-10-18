//! The files RPCS3 writes for the O-verbs, found under the oracle home's config
//! dir and copied out to a run's output dir (openspec AF.5). RPCS3 names them
//! itself: `screenshots/<TITLE>/<TITLE>_screenshot_<time>.png`,
//! `captures/<TITLE>_<time>_capture.rrc.gz` and
//! `savestates/<TITLE>/<TITLE>_*.SAVESTAT.zst`, the time to the second. A
//! capture and a state are committed by rename (`fs::pending_file`), so one
//! that shows under its final name is whole; a screenshot is created first and
//! written after its PNG encode, so it counts only once its bytes end in the
//! `IEND` chunk. Everything copied out goes through the work-tree refusal: it's
//! game data (the HARD rule in docs/native/guacamole-design.md).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

/// Which kind of file, by where RPCS3 puts it and how its name ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Screenshot,
    Capture,
    SaveState,
}

impl Kind {
    /// The dir under RPCS3's config dir; screenshots and states sit one level
    /// deeper, in a dir per title.
    pub fn dir(self) -> &'static str {
        match self {
            Kind::Screenshot => "screenshots",
            Kind::Capture => "captures",
            Kind::SaveState => "savestates",
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Kind::Screenshot => ".png",
            Kind::Capture => "_capture.rrc.gz",
            Kind::SaveState => ".SAVESTAT.zst",
        }
    }
}

/// The oracle home's `screenshots/`, `captures/` and `savestates/`, watched
/// for what a verb makes appear.
#[derive(Debug, Clone)]
pub struct Rpcs3Outputs {
    config_dir: PathBuf,
}

impl Rpcs3Outputs {
    pub fn new(config_dir: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: config_dir.into(),
        }
    }

    pub fn dir(&self, kind: Kind) -> PathBuf {
        self.config_dir.join(kind.dir())
    }

    /// Every file of `kind` there now, a title dir deep. A dir that doesn't
    /// exist yet lists empty.
    pub fn list(&self, kind: Kind) -> BTreeSet<PathBuf> {
        let mut out = BTreeSet::new();
        list_into(&self.dir(kind), kind.suffix(), 2, &mut out);
        out
    }

    /// Waits for a file of `kind` that isn't in `before` and is whole; the
    /// newest-named when several are.
    pub fn wait_new(
        &self,
        kind: Kind,
        before: &BTreeSet<PathBuf>,
        timeout: Duration,
    ) -> Result<PathBuf> {
        let deadline = Instant::now() + timeout;
        loop {
            let fresh = self
                .list(kind)
                .into_iter()
                .filter(|p| !before.contains(p) && is_whole(kind, p))
                .max();
            if let Some(path) = fresh {
                return Ok(path);
            }
            if Instant::now() > deadline {
                bail!(
                    "no new {:?} under {} after {timeout:?} (RPCS3's log says why)",
                    kind,
                    self.dir(kind).display()
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn list_into(dir: &Path, suffix: &str, depth: u32, out: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() && depth > 1 {
            list_into(&path, suffix, depth - 1, out);
        } else if kind.is_file() && entry.file_name().to_string_lossy().ends_with(suffix) {
            out.insert(path);
        }
    }
}

/// The 12 bytes a PNG ends with: `IEND`'s empty chunk and its CRC.
const PNG_END: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];

fn is_whole(kind: Kind, path: &Path) -> bool {
    match kind {
        Kind::Screenshot => png_complete(path),
        Kind::Capture | Kind::SaveState => true,
    }
}

/// True once the file's bytes end in `IEND` (`gs_frame::take_screenshot`
/// creates the file before it encodes, then writes the PNG in one call).
pub fn png_complete(path: &Path) -> bool {
    fs::read(path).is_ok_and(|b| b.len() > PNG_END.len() && b.ends_with(&PNG_END))
}

/// Copies `file` into `out_dir` under its own name, refusing an `out_dir`
/// inside any work tree of this repo.
pub fn copy_out(file: &Path, out_dir: &Path) -> Result<PathBuf> {
    let out_dir = rrc_dump::output::refuse_repo_output(out_dir)?;
    fs::create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    let name = file.file_name().context("output file has no name")?;
    let copy = out_dir.join(name);
    fs::copy(file, &copy)
        .with_context(|| format!("copying {} to {}", file.display(), copy.display()))?;
    Ok(copy)
}

/// What a screenshot shows, enough to tell a real frame from a black one and
/// two shots of one frame from shots of two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Pixels with any of R, G or B above 16 (RPCS3 writes alpha as 255).
    pub lit: u64,
    /// SHA-256 of the RGBA pixels, hex.
    pub pixels_sha256: String,
}

impl Frame {
    pub fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// The share of pixels lit, 0 to 1.
    pub fn lit_share(&self) -> f64 {
        self.lit as f64 / self.pixels().max(1) as f64
    }
}

/// Decodes a PNG and summarises it.
pub fn read_frame(path: &Path) -> Result<Frame> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .with_context(|| format!("decoding {}", path.display()))?
        .to_rgba8();
    let lit = image
        .pixels()
        .filter(|p| p.0[..3].iter().any(|&c| c > 16))
        .count() as u64;
    Ok(Frame {
        width: image.width(),
        height: image.height(),
        lit,
        pixels_sha256: hex(&Sha256::digest(image.as_raw())),
    })
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, lit_x: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_fn(width, height, |x, _| {
            if x < lit_x {
                image::Rgba([200, 40, 10, 255])
            } else {
                image::Rgba([3, 3, 3, 255])
            }
        });
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_png_counts_once_its_iend_is_written() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("s.png");
        let bytes = png(8, 4, 2);
        assert!(!png_complete(&path), "no file yet");
        std::fs::write(&path, b"").unwrap();
        assert!(!png_complete(&path), "created, not yet written");
        std::fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        assert!(!png_complete(&path), "cut short");
        std::fs::write(&path, &bytes).unwrap();
        assert!(png_complete(&path));
    }

    #[test]
    fn a_frame_reports_its_size_lit_pixels_and_pixel_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b, c) = (
            tmp.path().join("a.png"),
            tmp.path().join("b.png"),
            tmp.path().join("c.png"),
        );
        std::fs::write(&a, png(8, 4, 2)).unwrap();
        std::fs::write(&b, png(8, 4, 2)).unwrap();
        std::fs::write(&c, png(8, 4, 3)).unwrap();
        let fa = read_frame(&a).unwrap();
        assert_eq!((fa.width, fa.height, fa.lit, fa.pixels()), (8, 4, 8, 32));
        assert_eq!(fa.lit_share(), 0.25);
        assert_eq!(fa, read_frame(&b).unwrap(), "same pixels, same frame");
        assert_ne!(fa.pixels_sha256, read_frame(&c).unwrap().pixels_sha256);
        std::fs::write(&c, b"not a png").unwrap();
        assert!(read_frame(&c).is_err());
    }

    #[test]
    fn new_files_are_found_by_kind_a_title_dir_deep_and_only_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let outputs = Rpcs3Outputs::new(tmp.path());
        // Nothing there yet: every list is empty, not an error.
        for kind in [Kind::Screenshot, Kind::Capture, Kind::SaveState] {
            assert!(outputs.list(kind).is_empty());
        }
        let shots = tmp.path().join("screenshots/BLUS30968");
        let states = tmp.path().join("savestates/BLUS30968");
        std::fs::create_dir_all(&shots).unwrap();
        std::fs::create_dir_all(&states).unwrap();
        std::fs::create_dir_all(tmp.path().join("captures")).unwrap();
        let old_shot = shots.join("BLUS30968_screenshot_2026_10_18_10_00_00.png");
        std::fs::write(&old_shot, png(2, 2, 1)).unwrap();
        let before = outputs.list(Kind::Screenshot);
        assert_eq!(before.len(), 1);

        // A shot still encoding and a temp state aren't found; the whole ones are.
        let new_shot = shots.join("BLUS30968_screenshot_2026_10_18_10_00_01.png");
        std::fs::write(&new_shot, b"").unwrap();
        let err = outputs
            .wait_new(Kind::Screenshot, &before, Duration::from_millis(150))
            .unwrap_err()
            .to_string();
        assert!(err.contains("no new Screenshot"), "{err}");
        std::fs::write(&new_shot, png(2, 2, 1)).unwrap();
        let found = outputs
            .wait_new(Kind::Screenshot, &before, Duration::from_secs(1))
            .unwrap();
        assert_eq!(found, new_shot);

        let states_before = outputs.list(Kind::SaveState);
        std::fs::write(
            states.join("\u{ff04}BLUS30968_1_0.SAVESTAT.zst.7xQ.tmp"),
            b"partial",
        )
        .unwrap();
        assert!(outputs.list(Kind::SaveState).is_empty());
        let state = states.join("BLUS30968_1_0.SAVESTAT.zst");
        std::fs::write(&state, b"state").unwrap();
        assert_eq!(
            outputs
                .wait_new(Kind::SaveState, &states_before, Duration::from_secs(1))
                .unwrap(),
            state
        );
        let capture = tmp
            .path()
            .join("captures/BLUS30968_2026_10_18_10_00_02_capture.rrc.gz");
        std::fs::write(&capture, b"rrc").unwrap();
        assert_eq!(
            outputs.list(Kind::Capture),
            BTreeSet::from([capture.clone()])
        );
    }

    #[test]
    fn copies_go_outside_the_work_tree_only() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("BLUS30968_capture.rrc.gz");
        std::fs::write(&file, b"rrc").unwrap();
        let copy = copy_out(&file, &tmp.path().join("out/run")).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"rrc");
        assert_eq!(copy.file_name(), file.file_name());

        let inside = Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-output");
        let err = copy_out(&file, &inside).unwrap_err().to_string();
        assert!(
            err.contains("refusing to write capture data inside"),
            "{err}"
        );
        assert!(!inside.exists());
    }
}
