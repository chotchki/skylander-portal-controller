//! A.2.3 — image classifier for the recorder's menu-nav loop.
//!
//! Matches a live game frame against a set of labeled reference screens (via a
//! perceptual **dHash**, robust to minor animation) and returns the **gate
//! action** the nav loop should take — press a button / wait / stop. The
//! reference screens + their gates live in a JSON manifest the user fills after
//! reviewing the A.2.2 capture (`tools/playthrough/assets/screens/gates.json`).
//!
//! Pipeline (A.2.4 nav loop): `grab_frame()` → `ScreenLibrary::classify()` →
//! `GateAction` → drive the IPC (`press_button` / wait) → repeat until `Stop`.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use image::{DynamicImage, GrayImage};
use serde::Deserialize;
use skylander_rpcs3_control::ipc::IpcPortalDriver;
use skylander_rpcs3_control::ipc::proto::PadButton;

/// Default dHash Hamming-distance threshold for a match. Two frames of the same
/// screen (minor animation) differ by only a few bits; distinct screens differ
/// by ~20+. 10 is a safe middle — tune against the real captures.
pub const DEFAULT_THRESHOLD: u32 = 10;

/// What the nav loop should do once a screen is recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateAction {
    /// Press this button (held `ms` ms) to advance past this screen.
    Press(PadButton, u32),
    /// Recognised but transient (loading / cutscene) — wait + re-check.
    Wait,
    /// The in-game portal is up — stop the nav loop (success).
    Stop,
}

/// A 64-bit perceptual hash (dHash) of a cropped, downscaled, grayscale frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenHash(pub u64);

impl ScreenHash {
    /// Hamming distance — the count of differing bits.
    pub fn distance(self, other: ScreenHash) -> u32 {
        (self.0 ^ other.0).count_ones()
    }
}

/// dHash of a grayscale image: resize to 9×8, then for each row compare each
/// pixel to its right neighbour → 8×8 = 64 bits. Coarse + brightness-invariant,
/// so it shrugs off minor animation, scaling, and compression noise.
pub fn dhash(gray: &GrayImage) -> ScreenHash {
    let small = image::imageops::resize(gray, 9, 8, image::imageops::FilterType::Triangle);
    let mut bits = 0u64;
    let mut i = 0u32;
    for y in 0..8u32 {
        for x in 0..8u32 {
            let left = small.get_pixel(x, y).0[0];
            let right = small.get_pixel(x + 1, y).0[0];
            if left > right {
                bits |= 1u64 << i;
            }
            i += 1;
        }
    }
    ScreenHash(bits)
}

/// Optional crop rectangle `[x, y, w, h]` applied before hashing — so the match
/// keys on the game window, not the surrounding desktop. `None` = whole frame.
pub type CropRect = Option<[u32; 4]>;

/// Hash a frame: crop to the game region (if given), grayscale, dHash.
pub fn hash_frame(img: &DynamicImage, crop: CropRect) -> ScreenHash {
    let gray = match crop {
        // crop_imm clamps to the image bounds, so a slightly-too-big rect is safe.
        Some([x, y, w, h]) => img.crop_imm(x, y, w, h).to_luma8(),
        None => img.to_luma8(),
    };
    dhash(&gray)
}

/// Grab the current screen to a temp PNG via macOS `screencapture` and load it.
/// A lightweight single-frame grab — unlike continuous SCKit capture it does NOT
/// perturb RPCS3 (which crashed the save-state boot under continuous capture).
/// Whole display; the library's `crop` narrows the match to the game window.
#[allow(dead_code)] // wired into the nav loop in A.2.4
pub fn grab_frame() -> Result<DynamicImage> {
    let tmp = std::env::temp_dir().join(format!("sky-navframe-{}.png", std::process::id()));
    let status = Command::new("/usr/sbin/screencapture")
        .arg("-x") // silent (no shutter sound)
        .arg(&tmp)
        .status()
        .context("spawn /usr/sbin/screencapture")?;
    if !status.success() {
        bail!("screencapture exited with {status}");
    }
    let img = image::open(&tmp).with_context(|| format!("load grabbed frame {}", tmp.display()))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(img)
}

// --- manifest (the JSON the user fills after reviewing the A.2.2 capture) ---

#[derive(Debug, Clone, Deserialize)]
struct GatesManifest {
    /// `[x, y, w, h]` crop applied to every frame before hashing — the game
    /// window's rect within the captured display. Omit to hash the whole frame.
    #[serde(default)]
    crop: Option<[u32; 4]>,
    /// Action for a frame that matches no gate (e.g. "mash CROSS through the
    /// menus"). Absent => `Wait` (the safe default — don't blind-press).
    #[serde(default)]
    default: Option<GateActionSpec>,
    gates: Vec<GateEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct GateEntry {
    /// Human label, e.g. `"title"`, `"save_select"`, `"loading"`, `"portal_ready"`.
    label: String,
    /// Reference PNG filenames relative to the manifest's directory. Multiple
    /// per label (animation variants) — a frame matches the label if it is
    /// within threshold of ANY of them.
    refs: Vec<String>,
    /// `{"do":"press","button":"CROSS","ms":150}` | `{"do":"wait"}` | `{"do":"stop"}`.
    action: GateActionSpec,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
enum GateActionSpec {
    Press { button: String, ms: u32 },
    Wait,
    Stop,
}

/// A loaded reference screen: its precomputed hash + the resolved gate action.
#[derive(Debug, Clone)]
struct LoadedRef {
    label: String,
    hash: ScreenHash,
    action: GateAction,
}

/// The classifier: the game-window crop + every reference hash + its gate.
#[derive(Debug, Clone)]
pub struct ScreenLibrary {
    crop: CropRect,
    default: GateAction,
    refs: Vec<LoadedRef>,
    threshold: u32,
}

impl ScreenLibrary {
    /// Load the JSON manifest at `path` and the reference PNGs it names
    /// (relative to the manifest's directory), precomputing each hash.
    pub fn load(path: &Path) -> Result<Self> {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read gates manifest {}", path.display()))?;
        let manifest: GatesManifest =
            serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;

        let mut refs = Vec::new();
        for g in &manifest.gates {
            let action = resolve_action(&g.action)?;
            for r in &g.refs {
                let p = dir.join(r);
                let img =
                    image::open(&p).with_context(|| format!("open reference {}", p.display()))?;
                refs.push(LoadedRef {
                    label: g.label.clone(),
                    hash: hash_frame(&img, manifest.crop),
                    action,
                });
            }
        }
        if refs.is_empty() {
            bail!(
                "gates manifest {} names no reference screens",
                path.display()
            );
        }
        let default = match &manifest.default {
            Some(a) => resolve_action(a)?,
            None => GateAction::Wait,
        };
        Ok(Self {
            crop: manifest.crop,
            default,
            refs,
            threshold: DEFAULT_THRESHOLD,
        })
    }

    /// Override the match threshold (default [`DEFAULT_THRESHOLD`]).
    #[allow(dead_code)] // used by the nav loop / tuning in A.2.4
    pub fn with_threshold(mut self, t: u32) -> Self {
        self.threshold = t;
        self
    }

    /// The action for a frame that matches no gate (the manifest `default`).
    pub fn default_action(&self) -> GateAction {
        self.default
    }

    /// Classify a frame → the nearest matching `(label, action)` whose reference
    /// is within threshold, or `None` if nothing matches (unknown screen — the
    /// nav loop should wait + retry, not blindly press).
    pub fn classify(&self, frame: &DynamicImage) -> Option<(&str, GateAction)> {
        let h = hash_frame(frame, self.crop);
        self.refs
            .iter()
            .map(|r| (r, r.hash.distance(h)))
            .filter(|(_, d)| *d <= self.threshold)
            .min_by_key(|(_, d)| *d)
            .map(|(r, _)| (r.label.as_str(), r.action))
    }
}

fn resolve_action(a: &GateActionSpec) -> Result<GateAction> {
    Ok(match a {
        GateActionSpec::Press { button, ms } => GateAction::Press(parse_button(button)?, *ms),
        GateActionSpec::Wait => GateAction::Wait,
        GateActionSpec::Stop => GateAction::Stop,
    })
}

fn parse_button(s: &str) -> Result<PadButton> {
    Ok(match s.to_ascii_uppercase().as_str() {
        "CROSS" => PadButton::Cross,
        "CIRCLE" => PadButton::Circle,
        "SQUARE" => PadButton::Square,
        "TRIANGLE" => PadButton::Triangle,
        "START" => PadButton::Start,
        "SELECT" => PadButton::Select,
        "UP" => PadButton::Up,
        "DOWN" => PadButton::Down,
        "LEFT" => PadButton::Left,
        "RIGHT" => PadButton::Right,
        "L1" => PadButton::L1,
        "R1" => PadButton::R1,
        other => bail!("unknown pad button {other:?} in gates manifest"),
    })
}

/// A.2.4 nav loop: grab the screen, classify it, and act (press / wait) until a
/// `Stop` gate (portal ready) — or `timeout`. The boot + the live driver are the
/// caller's; this is the closed-loop nav itself. Once `gates.json` is filled from
/// the A.2.2 capture, this replaces the fragile fixed save-state wait in the
/// `ingame` narrative's `pick_game` beat.
#[allow(dead_code)] // wired into the ingame narrative once gates.json is filled
pub fn nav_to_portal(
    driver: &IpcPortalDriver,
    lib: &ScreenLibrary,
    timeout: std::time::Duration,
) -> Result<()> {
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    let start = Instant::now();
    loop {
        if start.elapsed() > timeout {
            bail!("nav-to-portal timed out after {timeout:?} before reaching portal_ready");
        }
        let frame = grab_frame()?;
        // A matched gate, or the manifest default (Giants: mash CROSS through everything).
        let (label, action) = match lib.classify(&frame) {
            Some((l, a)) => (l.to_string(), a),
            None => ("unrecognised".to_string(), lib.default_action()),
        };
        match action {
            GateAction::Stop => {
                tracing::info!(screen = %label, "nav: portal ready, done");
                return Ok(());
            }
            GateAction::Press(button, ms) => {
                tracing::info!(screen = %label, ?button, ms, "nav: press to advance");
                driver.press_button(button, ms)?;
                sleep(Duration::from_millis(900)); // let the screen transition
            }
            GateAction::Wait => {
                tracing::info!(screen = %label, "nav: waiting");
                sleep(Duration::from_millis(1000));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};

    /// A grayscale "hump": brightness peaks at the centre column (or troughs, if
    /// `invert`). Gives a non-degenerate dHash (a left-rising / right-falling
    /// row → distinct bit pattern); `invert` flips it for a far-apart pair.
    fn hump(w: u32, h: u32, invert: bool) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for (x, _y, p) in img.enumerate_pixels_mut() {
            let d = ((x as f32 / w as f32) - 0.5).abs() * 2.0; // 0 centre … 1 edges
            let mut v = (255.0 * (1.0 - d)) as u8; // bright centre
            if invert {
                v = 255 - v;
            }
            *p = Rgb([v, v, v]);
        }
        DynamicImage::ImageRgb8(img)
    }

    fn lib(refs: Vec<LoadedRef>, threshold: u32) -> ScreenLibrary {
        ScreenLibrary {
            crop: None,
            default: GateAction::Wait,
            refs,
            threshold,
        }
    }

    #[test]
    fn identical_frames_hash_equal() {
        let a = hump(160, 90, false);
        assert_eq!(hash_frame(&a, None), hash_frame(&a, None));
    }

    #[test]
    fn distinct_screens_are_far_apart() {
        let a = hash_frame(&hump(160, 90, false), None);
        let b = hash_frame(&hump(160, 90, true), None);
        assert!(
            a.distance(b) > DEFAULT_THRESHOLD,
            "expected distinct screens far apart, got distance {}",
            a.distance(b)
        );
    }

    #[test]
    fn classify_picks_nearest_label_within_threshold() {
        let lib = lib(
            vec![
                LoadedRef {
                    label: "title".into(),
                    hash: hash_frame(&hump(160, 90, false), None),
                    action: GateAction::Press(PadButton::Cross, 150),
                },
                LoadedRef {
                    label: "portal".into(),
                    hash: hash_frame(&hump(160, 90, true), None),
                    action: GateAction::Stop,
                },
            ],
            DEFAULT_THRESHOLD,
        );
        let (label, action) = lib.classify(&hump(160, 90, false)).expect("matches title");
        assert_eq!(label, "title");
        assert_eq!(action, GateAction::Press(PadButton::Cross, 150));

        let (label2, action2) = lib.classify(&hump(160, 90, true)).expect("matches portal");
        assert_eq!(label2, "portal");
        assert_eq!(action2, GateAction::Stop);
    }

    #[test]
    fn unrecognised_frame_is_none() {
        let lib = lib(
            vec![LoadedRef {
                label: "portal".into(),
                hash: hash_frame(&hump(160, 90, true), None),
                action: GateAction::Stop,
            }],
            2, // tight threshold
        );
        // The opposite hump is far from the only reference → no match.
        assert!(lib.classify(&hump(160, 90, false)).is_none());
    }

    #[test]
    fn loads_manifest_and_resolves_gates() {
        // Build a tiny on-disk library: two reference PNGs + a gates.json.
        let dir = std::env::temp_dir().join(format!("sky-gates-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        hump(160, 90, false).save(dir.join("title.png")).unwrap();
        hump(160, 90, true).save(dir.join("portal.png")).unwrap();
        let manifest = r#"{
            "gates": [
                {"label": "title",  "refs": ["title.png"],  "action": {"do": "press", "button": "START", "ms": 120}},
                {"label": "portal", "refs": ["portal.png"], "action": {"do": "stop"}}
            ]
        }"#;
        let mpath = dir.join("gates.json");
        std::fs::write(&mpath, manifest).unwrap();

        let lib = ScreenLibrary::load(&mpath).expect("load manifest");
        let (label, action) = lib.classify(&hump(160, 90, false)).expect("classify title");
        assert_eq!(label, "title");
        assert_eq!(action, GateAction::Press(PadButton::Start, 120));
        let (l2, a2) = lib.classify(&hump(160, 90, true)).expect("classify portal");
        assert_eq!(l2, "portal");
        assert_eq!(a2, GateAction::Stop);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loads_the_real_gates_manifest() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/screens/gates.json");
        let lib = ScreenLibrary::load(&path).expect("load real gates.json");
        // default = mash CROSS through the menus.
        assert_eq!(
            lib.default_action(),
            GateAction::Press(PadButton::Cross, 120)
        );
        // The saved portal reference classifies as the stop gate.
        let portal = image::open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/screens/refs/portal_ready.png"),
        )
        .expect("open portal ref");
        let (label, action) = lib.classify(&portal).expect("portal classifies");
        assert_eq!(label, "portal_ready");
        assert_eq!(action, GateAction::Stop);
    }
}
