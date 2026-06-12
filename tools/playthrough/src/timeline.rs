//! PLAN 15.13.4 — the editorial-manifest schema (`<out>.timeline.json`)
//! shared by the recorder (writer, `main.rs`) and the `-- render` post-pass
//! (reader, `render.rs`).
//!
//! Two on-disk shapes exist:
//!   - **v2 (current):** an object `{ "stage": CropRect|null, "beats": [...] }`
//!     — `stage` is the tiled launcher+phone region in physical capture pixels
//!     (PLAN 15.14); the render pass crops every segment to it unless a beat
//!     carries its own `crop` override.
//!   - **v1 (legacy):** a bare array of beat entries (what phases 1-3 wrote).
//!     [`TimelineFile::parse`] accepts it (`stage: None`) so raw captures
//!     recorded before 15.14 stay renderable.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A crop/framing rectangle in **physical capture pixels** (the coordinate
/// space of the recorded MP4). Used both per-beat (`TimelineEntry::crop`) and
/// narrative-wide (`TimelineFile::stage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// One row of the editorial manifest: a beat's measured boundaries (wall-clock
/// ms relative to `DesktopCapture` start — design §5) plus its editorial
/// intent for the render pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub beat: String,
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    /// Keep this much at 1× at the start of the beat (the action).
    pub realtime_head_ms: u64,
    /// Keep this much at 1× at the end of the beat (the reveal).
    pub realtime_tail_ms: u64,
    /// Play the dead middle at this speed (1.0 = no speed-up).
    pub filler_speed: f32,
    /// Per-beat framing override; `None` falls back to [`TimelineFile::stage`].
    pub crop: Option<CropRect>,
}

/// The whole manifest. See the module docs for the v1/v2 shapes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineFile {
    /// The tiled launcher+phone region (PLAN 15.14), in physical capture
    /// pixels. `None` = no narrative-wide crop (v1 manifests, or window
    /// placement failed and the recorder degraded to full-frame).
    #[serde(default)]
    pub stage: Option<CropRect>,
    pub beats: Vec<TimelineEntry>,
}

impl TimelineFile {
    pub fn load(path: &Path) -> Result<Self> {
        let body = std::fs::read_to_string(path)
            .with_context(|| format!("read timeline manifest {}", path.display()))?;
        Self::parse(&body).with_context(|| format!("parse timeline manifest {}", path.display()))
    }

    /// Accept the v2 object first, then fall back to the v1 bare array.
    pub fn parse(body: &str) -> Result<Self> {
        if let Ok(v2) = serde_json::from_str::<TimelineFile>(body) {
            return Ok(v2);
        }
        let beats = serde_json::from_str::<Vec<TimelineEntry>>(body)
            .context("manifest is neither a v2 {stage, beats} object nor a v1 entry array")?;
        Ok(Self { stage: None, beats })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let body = serde_json::to_string_pretty(self).context("serialize timeline.json")?;
        std::fs::write(path, body)
            .with_context(|| format!("write timeline manifest to {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact shape phases 1-3 wrote (a bare array, u128-ish ints, null
    /// crops) still loads — `stage` comes back `None`.
    #[test]
    fn v1_bare_array_parses_with_no_stage() {
        let v1 = r#"[
          { "beat": "connect", "t_start_ms": 2522, "t_end_ms": 3228,
            "realtime_head_ms": 1000, "realtime_tail_ms": 2000,
            "filler_speed": 1.0, "crop": null },
          { "beat": "see_in_game", "t_start_ms": 44581, "t_end_ms": 60592,
            "realtime_head_ms": 1000, "realtime_tail_ms": 6000,
            "filler_speed": 8.0, "crop": null }
        ]"#;
        let tl = TimelineFile::parse(v1).expect("v1 manifest should parse");
        assert_eq!(tl.stage, None);
        assert_eq!(tl.beats.len(), 2);
        assert_eq!(tl.beats[0].beat, "connect");
        assert_eq!(tl.beats[1].t_end_ms, 60_592);
        assert_eq!(tl.beats[1].filler_speed, 8.0);
    }

    #[test]
    fn v2_object_roundtrips_with_stage() {
        let tl = TimelineFile {
            stage: Some(CropRect {
                x: 0,
                y: 0,
                w: 3214,
                h: 2050,
            }),
            beats: vec![TimelineEntry {
                beat: "connect".into(),
                t_start_ms: 100,
                t_end_ms: 900,
                realtime_head_ms: 500,
                realtime_tail_ms: 200,
                filler_speed: 4.0,
                crop: Some(CropRect {
                    x: 10,
                    y: 20,
                    w: 640,
                    h: 480,
                }),
            }],
        };
        let body = serde_json::to_string(&tl).unwrap();
        let back = TimelineFile::parse(&body).expect("v2 manifest should parse");
        assert_eq!(back.stage, tl.stage);
        assert_eq!(back.beats.len(), 1);
        assert_eq!(back.beats[0].crop, tl.beats[0].crop);
    }

    #[test]
    fn garbage_is_a_clear_error() {
        let err = TimelineFile::parse("{\"nope\": true}").expect_err("must reject");
        assert!(err.to_string().contains("neither"), "got: {err}");
    }
}
