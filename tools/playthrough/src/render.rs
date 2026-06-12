//! PLAN 15.13.4 — the `-- render` editorial post-pass (design
//! `docs/dev/recorder-beats-framework.md` §5/§6, phase 4).
//!
//! Consumes a raw capture MP4 + its `<out>.timeline.json` manifest
//! ([`crate::timeline`]) and emits ONE edited MP4 via a single ffmpeg
//! `filter_complex` invocation: per-beat speed-ramp (head/tail @1×, dead
//! middle @`filler_speed`), gap-fill so the whole input is covered (pre-roll
//! / inter-beat slack / trailing hold @1× — the recorder holds ~3s after the
//! last beat with NO manifest entry, by design), per-segment crop (beat
//! override, else the narrative `stage`), coalescing, trim+setpts+concat, and
//! the delivery transcode.
//!
//! **Codec: H.265** (libx265, `hvc1` tag so QuickTime/Safari recognise the
//! track, `+faststart` for streamable playback). AV1 stays a documented door:
//! the gyan "essentials" ffmpeg build only carries the slow libaom encoder,
//! so switching means a build with SVT-AV1 — revisit when that's on the box.
//!
//! Layering: everything that *decides* — segment planning
//! ([`plan_segments`]), filtergraph text ([`build_filtergraph`]), probe-JSON
//! decoding — is pure and unit-tested on Win+mac CI; only [`probe`] and
//! [`encode`] shell out (`std::process::Command`, no shell). This file
//! carries no `cfg(windows)` — the no-ffmpeg rule applies to *capture* only
//! (design §5); the render stage is dev-only and ffmpeg is assumed on the box
//! (or pointed at via `FFMPEG` / `FFPROBE`).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::timeline::{CropRect, TimelineEntry, TimelineFile};

/// Delivery-width cap: the raw capture is a full desktop (often 4K-ish) but
/// the demo deliverable targets ~1080p — a canvas wider than this gets one
/// final downscale after the concat.
const SCALE_CAP_W: u32 = 1920;

/// Post-coalesce minimum segment length. A sub-50ms trim is ≤3 frames at
/// 60fps — visually nothing — but it bloats the graph and risks zero-frame
/// trims at lower capture rates, so such slivers are dropped (and logged).
const MIN_SEG_MS: u64 = 50;

/// `foo.mp4` → sibling `foo.timeline.json`. MUST mirror `write_timeline` in
/// `main.rs` — the recorder writes the manifest with the same
/// `with_extension` call, so the bare `-- render <raw>` default always finds
/// what a recording run produced.
pub fn default_manifest_path(raw: &Path) -> PathBuf {
    raw.with_extension("timeline.json")
}

/// `foo.mp4` → sibling `foo-final.mp4` (the CLI's defaulted output path).
pub fn default_out_path(raw: &Path) -> PathBuf {
    let stem = raw
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "capture".to_owned());
    raw.with_file_name(format!("{stem}-final.mp4"))
}

/// Resolve a tool binary: env override first (`FFMPEG` / `FFPROBE`), else the
/// bare name from PATH.
fn tool(env_var: &str, default: &str) -> String {
    std::env::var(env_var).unwrap_or_else(|_| default.to_owned())
}

/// Context for a failed spawn — the overwhelmingly common cause is the binary
/// not being installed, so say what to install / which env var to set.
fn spawn_hint(bin: &str, env_var: &str) -> String {
    format!(
        "spawn {bin:?} — install ffmpeg (ffmpeg + ffprobe on PATH) or point the {env_var} \
         env var at the binary"
    )
}

/// What the render pass needs to know about the raw capture, via ffprobe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoMeta {
    pub w: u32,
    pub h: u32,
    /// Decoded from the `r_frame_rate` rational (`"60/1"`, `"30000/1001"`).
    pub fps: f64,
    /// Stream duration when present, else the container (`format`) duration.
    pub duration_ms: u64,
}

/// The subset of `ffprobe -of json` output the render pass reads.
#[derive(Deserialize)]
struct ProbeOut {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Deserialize)]
struct ProbeStream {
    width: u32,
    height: u32,
    r_frame_rate: String,
    /// Absent in some containers — `format.duration` is the fallback.
    duration: Option<String>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

/// `"60/1"` → 60.0, `"30000/1001"` → 29.97…; a bare number passes through.
fn parse_fps_rational(s: &str) -> Result<f64> {
    let (num, den) = match s.split_once('/') {
        Some((n, d)) => (
            n.trim()
                .parse::<f64>()
                .with_context(|| format!("fps numerator in {s:?}"))?,
            d.trim()
                .parse::<f64>()
                .with_context(|| format!("fps denominator in {s:?}"))?,
        ),
        None => (
            s.trim()
                .parse::<f64>()
                .with_context(|| format!("fps {s:?}"))?,
            1.0,
        ),
    };
    ensure!(num > 0.0 && den > 0.0, "non-positive frame rate {s:?}");
    Ok(num / den)
}

/// Pure half of [`probe`] — unit-tested against canned ffprobe JSON.
fn parse_probe_json(body: &str) -> Result<VideoMeta> {
    let probe: ProbeOut = serde_json::from_str(body).context("ffprobe output is not JSON")?;
    let stream = probe
        .streams
        .first()
        .context("ffprobe found no video stream")?;
    let fps = parse_fps_rational(&stream.r_frame_rate)?;
    let duration_s = stream
        .duration
        .as_deref()
        .or_else(|| probe.format.as_ref().and_then(|f| f.duration.as_deref()))
        .context("ffprobe reported no duration (neither stream nor format)")?
        .trim()
        .parse::<f64>()
        .context("ffprobe duration is not a number")?;
    Ok(VideoMeta {
        w: stream.width,
        h: stream.height,
        fps,
        duration_ms: (duration_s * 1000.0).round() as u64,
    })
}

/// `ffprobe` the first video stream of `path`. No shell — args go straight to
/// the process, so paths with spaces need no quoting.
fn probe(path: &Path) -> Result<VideoMeta> {
    let bin = tool("FFPROBE", "ffprobe");
    let out = Command::new(&bin)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,r_frame_rate,duration",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .with_context(|| spawn_hint(&bin, "FFPROBE"))?;
    ensure!(
        out.status.success(),
        "ffprobe failed on {}: {}",
        path.display(),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    parse_probe_json(&String::from_utf8_lossy(&out.stdout))
        .with_context(|| format!("probe {}", path.display()))
}

/// One contiguous input span with uniform playback speed + framing — the unit
/// the filtergraph is built from. Half-open `[start_ms, end_ms)` in
/// raw-capture milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    /// 1.0 = realtime; >1 = fast-forward (a `setpts` division).
    pub speed: f32,
    /// Framing: the beat override, else the narrative `stage`, else `None`
    /// (full frame).
    pub crop: Option<CropRect>,
}

/// `filler_speed` is editorial data from a JSON file — treat any non-finite
/// or non-positive value as "no speed-up" rather than corrupting `setpts`.
fn sanitize_speed(speed: f32) -> f32 {
    if speed.is_finite() && speed > 0.0 {
        speed
    } else {
        1.0
    }
}

/// Turn the manifest's beat brackets into a gap-free segment plan covering
/// the whole input `[0, duration_ms)` (design §5):
///
/// - Each bracket is clamped to the input; empty/inverted brackets are
///   skipped. A bracket starting before the previous one ends is clamped to
///   start at the previous end — the recorder stamps monotonically, so
///   overlap means a hand-edited/corrupt manifest: degrade, don't fail.
/// - A beat shorter than `head + tail` has no dead middle → ONE realtime
///   span. Otherwise head @1× (omitted when 0ms), filler @`filler_speed`,
///   tail @1× (omitted when 0ms). Span framing = `beat.crop.or(stage)`.
/// - Everything the brackets don't cover — pre-roll before the first beat,
///   inter-beat slack, the recorder's ~3s trailing hold (no manifest entry by
///   design) — gap-fills as a 1× span framed by `stage`.
/// - Adjacent contiguous spans with identical (speed, crop) coalesce; spans
///   still shorter than `min_seg_ms` after coalescing are dropped (logged) —
///   sub-frame trims bloat the graph for zero visual gain.
pub fn plan_segments(
    beats: &[TimelineEntry],
    stage: Option<CropRect>,
    duration_ms: u64,
    min_seg_ms: u64,
) -> Vec<Segment> {
    let mut spans: Vec<Segment> = Vec::new();
    // Coverage watermark: end of the last span pushed so far.
    let mut cursor = 0u64;

    for beat in beats {
        let mut start = beat.t_start_ms.min(duration_ms);
        let end = beat.t_end_ms.min(duration_ms);
        if start < cursor {
            start = cursor;
        }
        if end <= start {
            continue; // empty or inverted after clamping
        }
        if start > cursor {
            // Gap-fill the uncovered run-up (pre-roll / inter-beat slack).
            spans.push(Segment {
                start_ms: cursor,
                end_ms: start,
                speed: 1.0,
                crop: stage,
            });
        }
        let crop = beat.crop.or(stage);
        let dur = end - start;
        let (head, tail) = (beat.realtime_head_ms, beat.realtime_tail_ms);
        if head + tail >= dur {
            // No dead middle to compress — the whole beat plays realtime.
            spans.push(Segment {
                start_ms: start,
                end_ms: end,
                speed: 1.0,
                crop,
            });
        } else {
            if head > 0 {
                spans.push(Segment {
                    start_ms: start,
                    end_ms: start + head,
                    speed: 1.0,
                    crop,
                });
            }
            spans.push(Segment {
                start_ms: start + head,
                end_ms: end - tail,
                speed: sanitize_speed(beat.filler_speed),
                crop,
            });
            if tail > 0 {
                spans.push(Segment {
                    start_ms: end - tail,
                    end_ms: end,
                    speed: 1.0,
                    crop,
                });
            }
        }
        cursor = end;
    }
    if cursor < duration_ms {
        // The trailing post-beat hold (no manifest entry — design §5).
        spans.push(Segment {
            start_ms: cursor,
            end_ms: duration_ms,
            speed: 1.0,
            crop: stage,
        });
    }

    // Coalesce contiguous spans with identical (speed, crop) — degenerate
    // beats, gap-fills, and 1× head/tail brackets routinely chain together.
    let mut segs: Vec<Segment> = Vec::with_capacity(spans.len());
    for span in spans {
        match segs.last_mut() {
            Some(prev)
                if prev.end_ms == span.start_ms
                    && prev.speed == span.speed
                    && prev.crop == span.crop =>
            {
                prev.end_ms = span.end_ms;
            }
            _ => segs.push(span),
        }
    }

    let mut dropped_ms = 0u64;
    segs.retain(|s| {
        let keep = s.end_ms - s.start_ms >= min_seg_ms;
        if !keep {
            dropped_ms += s.end_ms - s.start_ms;
        }
        keep
    });
    if dropped_ms > 0 {
        tracing::info!(
            dropped_ms,
            min_seg_ms,
            "plan: dropped sliver segments after coalescing"
        );
    }
    segs
}

/// Expected output duration: each span contributes `dur / speed`. Logged
/// before the encode and compared (±10%) against the actual output after.
pub fn planned_output_ms(segs: &[Segment]) -> u64 {
    segs.iter()
        .map(|s| (s.end_ms - s.start_ms) as f64 / f64::from(s.speed))
        .sum::<f64>()
        .round() as u64
}

/// Round down to even — libx265 + yuv420p need even frame dims, and crop
/// offsets must be even for clean 4:2:0 chroma siting.
fn even(v: u32) -> u32 {
    v & !1
}

/// `fps` filter argument: integer when whole (the common 60/30 case), full
/// `f64` precision otherwise (e.g. 29.97002997… from `30000/1001`).
fn fmt_fps(fps: f64) -> String {
    if fps.fract() == 0.0 {
        format!("{fps:.0}")
    } else {
        format!("{fps}")
    }
}

/// Build the single `filter_complex` graph; returns `(graph, output label)`
/// for ffmpeg's `-filter_complex` / `-map`.
///
/// Canvas: if every segment's effective output dims (its crop, else the full
/// frame, both even-rounded) agree, that's the canvas; any mix of dims falls
/// back to the full frame. Each segment whose *produced* dims differ from the
/// canvas is normalised onto it with an aspect-preserving scale + centred pad
/// — `concat` requires every input to share one geometry.
///
/// Per segment: `trim` (seconds, 3 decimals) → `setpts=(PTS-STARTPTS)/speed`
/// → optional `crop` (all four values even-rounded) → optional scale+pad →
/// `setsar=1` → `fps` (re-quantise to a constant rate so the speed-divided
/// timestamps land on a uniform grid before concat). The `setsar` is
/// unconditional and load-bearing: `scale=…:force_original_aspect_ratio=
/// decrease` rounds the fitted dims and compensates by emitting a non-1:1
/// SAR, and `concat` hard-errors unless every input shares one SAR — so any
/// mixed-framing plan (a per-beat crop next to stage-framed segments) would
/// otherwise fail the encode. Pinning every segment back to square pixels
/// makes the inputs uniform regardless of which path produced them. Then
/// `concat`, then — only when the canvas is wider than `scale_cap_w` — one
/// delivery downscale whose target height is computed HERE in Rust: a literal
/// `W:H` keeps lavfi expressions (and their comma-escaping) out of the graph.
pub fn build_filtergraph(segs: &[Segment], meta: &VideoMeta, scale_cap_w: u32) -> (String, String) {
    let full = (even(meta.w), even(meta.h));
    let effective = |s: &Segment| s.crop.map_or(full, |c| (even(c.w), even(c.h)));
    let first = segs.first().map_or(full, effective);
    let (cw, ch) = if segs.iter().all(|s| effective(s) == first) {
        first
    } else {
        full
    };

    let fps = fmt_fps(meta.fps);
    let mut graph = String::new();
    for (i, seg) in segs.iter().enumerate() {
        let start = seg.start_ms as f64 / 1000.0;
        let end = seg.end_ms as f64 / 1000.0;
        graph.push_str(&format!(
            "[0:v]trim=start={start:.3}:end={end:.3},setpts=(PTS-STARTPTS)/{}",
            seg.speed
        ));
        // Dims this segment actually produces going into concat: the
        // even-rounded crop, or the raw frame (a raw frame with odd dims also
        // normalises onto the even canvas via the scale+pad below).
        let produced = match seg.crop {
            Some(c) => {
                let (w, h, x, y) = (even(c.w), even(c.h), even(c.x), even(c.y));
                graph.push_str(&format!(",crop={w}:{h}:{x}:{y}"));
                (w, h)
            }
            None => (meta.w, meta.h),
        };
        if produced != (cw, ch) {
            graph.push_str(&format!(
                ",scale={cw}:{ch}:force_original_aspect_ratio=decrease,\
                 pad={cw}:{ch}:(ow-iw)/2:(oh-ih)/2"
            ));
        }
        graph.push_str(&format!(",setsar=1,fps={fps}[s{i}];"));
    }
    for i in 0..segs.len() {
        graph.push_str(&format!("[s{i}]"));
    }
    graph.push_str(&format!("concat=n={}:v=1:a=0[cat]", segs.len()));

    if cw > scale_cap_w {
        let out_h = even((u64::from(ch) * u64::from(scale_cap_w) / u64::from(cw)) as u32);
        graph.push_str(&format!(";[cat]scale={scale_cap_w}:{out_h}[outv]"));
        (graph, "[outv]".to_owned())
    } else {
        (graph, "[cat]".to_owned())
    }
}

/// Run the encode. Stdio is inherited so ffmpeg's `-stats` progress line is
/// visible in the terminal (everything else is `-v error`-quiet). H.265
/// deliverable — see the module docs for the AV1 door.
fn encode(raw: &Path, graph: &str, out_label: &str, out: &Path) -> Result<()> {
    let bin = tool("FFMPEG", "ffmpeg");
    let status = Command::new(&bin)
        .args(["-y", "-v", "error", "-stats", "-i"])
        .arg(raw)
        .args(["-filter_complex", graph, "-map", out_label, "-an"])
        .args(["-c:v", "libx265", "-preset", "medium", "-crf", "22"])
        .args([
            "-tag:v",
            "hvc1",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ])
        .arg(out)
        .status()
        .with_context(|| spawn_hint(&bin, "FFMPEG"))?;
    ensure!(status.success(), "ffmpeg exited with {status}");
    Ok(())
}

/// The `-- render` entry point: load the manifest (v1 bare arrays parse
/// transparently — [`TimelineFile::parse`]), probe the raw capture, plan the
/// segments, log the editorial table, build the graph, encode, and
/// sanity-check the output duration against the plan.
pub fn run(raw: &Path, manifest: &Path, out: &Path) -> Result<()> {
    let tl = TimelineFile::load(manifest)?;
    let meta = probe(raw)?;
    tracing::info!(
        raw = %raw.display(),
        w = meta.w,
        h = meta.h,
        fps = meta.fps,
        duration_ms = meta.duration_ms,
        beats = tl.beats.len(),
        stage = ?tl.stage,
        "render: probed raw capture"
    );

    let segs = plan_segments(&tl.beats, tl.stage, meta.duration_ms, MIN_SEG_MS);
    ensure!(
        !segs.is_empty(),
        "segment plan is empty — zero-length capture?"
    );
    for (i, s) in segs.iter().enumerate() {
        tracing::info!(
            seg = i,
            start_ms = s.start_ms,
            end_ms = s.end_ms,
            speed = s.speed,
            crop = ?s.crop,
            "render: segment"
        );
    }
    let planned_ms = planned_output_ms(&segs);
    tracing::info!(
        planned_ms,
        segments = segs.len(),
        "render: planned output duration"
    );

    let (graph, out_label) = build_filtergraph(&segs, &meta, SCALE_CAP_W);
    tracing::debug!(%graph, "render: filtergraph");
    encode(raw, &graph, &out_label, out)?;

    // >10% drift between plan and output means the trim maths and the encode
    // disagree (bad bracket clamps / fps mismatch) — warn, don't fail: the
    // file may still be perfectly watchable and worth inspecting.
    match probe(out) {
        Ok(m) => {
            let drift = (m.duration_ms as f64 - planned_ms as f64).abs();
            if drift > planned_ms as f64 * 0.10 {
                tracing::warn!(
                    planned_ms,
                    actual_ms = m.duration_ms,
                    "render: output duration drifts >10% from the plan"
                );
            }
        }
        Err(err) => {
            tracing::warn!(%err, "render: could not probe the output for the duration sanity check");
        }
    }

    let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    tracing::info!(out = %out.display(), bytes, "render: final cut written");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        beat: &str,
        t_start_ms: u64,
        t_end_ms: u64,
        realtime_head_ms: u64,
        realtime_tail_ms: u64,
        filler_speed: f32,
        crop: Option<CropRect>,
    ) -> TimelineEntry {
        TimelineEntry {
            beat: beat.into(),
            t_start_ms,
            t_end_ms,
            realtime_head_ms,
            realtime_tail_ms,
            filler_speed,
            crop,
        }
    }

    fn seg(start_ms: u64, end_ms: u64, speed: f32, crop: Option<CropRect>) -> Segment {
        Segment {
            start_ms,
            end_ms,
            speed,
            crop,
        }
    }

    const CROP_A: CropRect = CropRect {
        x: 10,
        y: 20,
        w: 470,
        h: 940,
    };

    // --- ffprobe JSON parsing ------------------------------------------------

    #[test]
    fn probe_json_prefers_stream_duration() {
        let body = r#"{
            "streams": [{ "width": 3840, "height": 2160,
                          "r_frame_rate": "60/1", "duration": "62.933000" }],
            "format": { "duration": "99.000000" }
        }"#;
        let m = parse_probe_json(body).unwrap();
        assert_eq!(
            m,
            VideoMeta {
                w: 3840,
                h: 2160,
                fps: 60.0,
                duration_ms: 62_933
            }
        );
    }

    #[test]
    fn probe_json_falls_back_to_format_duration_and_parses_ntsc_fps() {
        let body = r#"{
            "streams": [{ "width": 1920, "height": 1080, "r_frame_rate": "30000/1001" }],
            "format": { "duration": "12.500000" }
        }"#;
        let m = parse_probe_json(body).unwrap();
        assert_eq!((m.w, m.h, m.duration_ms), (1920, 1080, 12_500));
        assert!((m.fps - 30_000.0 / 1_001.0).abs() < 1e-9, "got {}", m.fps);
    }

    #[test]
    fn probe_json_without_any_duration_is_an_error() {
        let body = r#"{ "streams": [{ "width": 10, "height": 10, "r_frame_rate": "60/1" }],
                        "format": {} }"#;
        let err = parse_probe_json(body).expect_err("no duration anywhere must error");
        assert!(err.to_string().contains("duration"), "got: {err}");
    }

    // --- segment planning ----------------------------------------------------

    /// The REAL manifest from the surviving 2026-06-11 marquee run (v1 bare
    /// array, `stage: None`) — the fixture the whole render pass was sized
    /// against. Raw capture duration: 62 933 ms.
    const MARQUEE_MANIFEST: &str = r#"[
      { "beat": "connect", "t_start_ms": 2522, "t_end_ms": 3228,
        "realtime_head_ms": 1000, "realtime_tail_ms": 2000, "filler_speed": 1.0, "crop": null },
      { "beat": "pick_profile", "t_start_ms": 3228, "t_end_ms": 3237,
        "realtime_head_ms": 500, "realtime_tail_ms": 1000, "filler_speed": 1.0, "crop": null },
      { "beat": "pick_game", "t_start_ms": 3237, "t_end_ms": 22967,
        "realtime_head_ms": 1000, "realtime_tail_ms": 3000, "filler_speed": 8.0, "crop": null },
      { "beat": "settle_after_reconnect", "t_start_ms": 22967, "t_end_ms": 42982,
        "realtime_head_ms": 1000, "realtime_tail_ms": 1000, "filler_speed": 8.0, "crop": null },
      { "beat": "open_toybox", "t_start_ms": 42982, "t_end_ms": 43748,
        "realtime_head_ms": 1000, "realtime_tail_ms": 2000, "filler_speed": 1.0, "crop": null },
      { "beat": "place_figure", "t_start_ms": 43748, "t_end_ms": 44581,
        "realtime_head_ms": 1000, "realtime_tail_ms": 2000, "filler_speed": 1.0, "crop": null },
      { "beat": "see_in_game", "t_start_ms": 44581, "t_end_ms": 60592,
        "realtime_head_ms": 1000, "realtime_tail_ms": 6000, "filler_speed": 8.0, "crop": null }
    ]"#;

    #[test]
    fn marquee_fixture_plans_full_coverage() {
        let tl = TimelineFile::parse(MARQUEE_MANIFEST).expect("v1 fixture parses");
        assert_eq!(tl.stage, None);
        let segs = plan_segments(&tl.beats, tl.stage, 62_933, 50);

        // Full coverage [0..62933]: contiguous, no holes, no overlaps.
        assert_eq!(segs.first().unwrap().start_ms, 0);
        assert_eq!(segs.last().unwrap().end_ms, 62_933);
        for pair in segs.windows(2) {
            assert_eq!(pair[0].end_ms, pair[1].start_ms, "hole/overlap: {pair:?}");
        }

        assert_eq!(
            segs,
            vec![
                // Pre-roll [0..2522] + degenerate connect/pick_profile
                // (head+tail >= dur → one realtime span each) + pick_game's
                // 1000ms head, all @1× with the same (None) crop → one span.
                seg(0, 4_237, 1.0, None),
                seg(4_237, 19_967, 8.0, None), // pick_game filler
                // pick_game's 3000ms tail + settle_after_reconnect's head.
                seg(19_967, 23_967, 1.0, None),
                seg(23_967, 41_982, 8.0, None), // settle filler
                // settle tail + degenerate open_toybox/place_figure +
                // see_in_game's head.
                seg(41_982, 45_581, 1.0, None),
                seg(45_581, 54_592, 8.0, None), // see_in_game filler
                // see_in_game's 6000ms tail + the recorder's trailing hold
                // [60592..62933], gap-filled @1× with no manifest entry.
                seg(54_592, 62_933, 1.0, None),
            ]
        );

        // Hand-derived: 4237/1 + 15730/8 + 4000/1 + 18015/8 + 3599/1
        //             + 9011/8 + 8341/1
        //             = 4237 + 1966.25 + 4000 + 2251.875 + 3599
        //             + 1126.375 + 8341 = 25521.5 → rounds to 25 522
        //             (~25.5s cut from a 62.9s raw).
        assert_eq!(planned_output_ms(&segs), 25_522);
    }

    #[test]
    fn degenerate_beat_is_one_realtime_span() {
        // head+tail (1500+1500) >= dur (1000) → no filler split; the beat's
        // crop override survives (distinct from the stage=None gap-fills, so
        // it can't coalesce away).
        let beats = vec![entry("b", 1_000, 2_000, 1_500, 1_500, 8.0, Some(CROP_A))];
        let segs = plan_segments(&beats, None, 2_500, 50);
        assert_eq!(
            segs,
            vec![
                seg(0, 1_000, 1.0, None),
                seg(1_000, 2_000, 1.0, Some(CROP_A)),
                seg(2_000, 2_500, 1.0, None),
            ]
        );
    }

    #[test]
    fn head_filler_tail_split_with_zero_head_omitted() {
        let beats = vec![entry("b", 0, 10_000, 0, 2_000, 4.0, None)];
        let segs = plan_segments(&beats, None, 10_000, 50);
        assert_eq!(
            segs,
            vec![seg(0, 8_000, 4.0, None), seg(8_000, 10_000, 1.0, None)]
        );
    }

    #[test]
    fn nonpositive_or_nan_filler_speed_degrades_to_realtime() {
        for bad in [0.0_f32, -3.0, f32::NAN] {
            let beats = vec![entry("b", 0, 10_000, 1_000, 1_000, bad, None)];
            let segs = plan_segments(&beats, None, 10_000, 50);
            // The sanitised 1× filler coalesces with head + tail into one span.
            assert_eq!(segs, vec![seg(0, 10_000, 1.0, None)], "speed {bad}");
        }
    }

    #[test]
    fn brackets_clamp_to_duration_and_overlaps_clamp_to_previous_end() {
        let beats = vec![
            // Degenerate (head alone exceeds dur) → realtime.
            entry("a", 0, 5_000, 6_000, 0, 1.0, Some(CROP_A)),
            // Starts inside `a` (start clamps to 5000) and runs past EOF
            // (end clamps to 10000); no head/tail → all filler.
            entry("b", 4_000, 20_000, 0, 0, 8.0, None),
            // Wholly past EOF → skipped.
            entry("c", 30_000, 40_000, 0, 0, 8.0, None),
        ];
        let segs = plan_segments(&beats, None, 10_000, 50);
        assert_eq!(
            segs,
            vec![
                seg(0, 5_000, 1.0, Some(CROP_A)),
                seg(5_000, 10_000, 8.0, None)
            ]
        );
    }

    #[test]
    fn beat_crop_falls_back_to_stage() {
        let beats = vec![
            entry("framed", 0, 1_000, 2_000, 0, 1.0, None), // stage framing
            entry("zoomed", 1_000, 2_000, 2_000, 0, 1.0, Some(CROP_A)), // override
        ];
        let stage = CropRect {
            x: 0,
            y: 0,
            w: 1600,
            h: 900,
        };
        let segs = plan_segments(&beats, Some(stage), 2_000, 50);
        assert_eq!(
            segs,
            vec![
                seg(0, 1_000, 1.0, Some(stage)),
                seg(1_000, 2_000, 1.0, Some(CROP_A)),
            ]
        );
    }

    #[test]
    fn slivers_drop_after_coalescing() {
        // The 30ms beat is bracketed by spans whose crop differs from its
        // own, so it survives coalescing — and is then dropped as
        // sub-min_seg_ms (leaving an imperceptible sub-frame hole).
        let beats = vec![
            entry("a", 0, 1_000, 2_000, 0, 1.0, None),
            entry("b", 1_000, 1_030, 2_000, 0, 1.0, Some(CROP_A)),
            entry("c", 1_030, 2_000, 2_000, 0, 1.0, None),
        ];
        let segs = plan_segments(&beats, None, 2_000, 50);
        assert_eq!(
            segs,
            vec![seg(0, 1_000, 1.0, None), seg(1_030, 2_000, 1.0, None)]
        );
    }

    #[test]
    fn empty_manifest_is_one_realtime_span() {
        let segs = plan_segments(&[], None, 5_000, 50);
        assert_eq!(segs, vec![seg(0, 5_000, 1.0, None)]);
    }

    // --- filtergraph golden strings -------------------------------------------

    const META_4K60: VideoMeta = VideoMeta {
        w: 3840,
        h: 2160,
        fps: 60.0,
        duration_ms: 62_933,
    };

    #[test]
    fn filtergraph_single_segment_no_crop() {
        let meta = VideoMeta {
            w: 1920,
            h: 1080,
            fps: 60.0,
            duration_ms: 5_000,
        };
        let segs = vec![seg(0, 5_000, 1.0, None)];
        let (graph, label) = build_filtergraph(&segs, &meta, 1_920);
        assert_eq!(label, "[cat]");
        assert_eq!(
            graph,
            "[0:v]trim=start=0.000:end=5.000,setpts=(PTS-STARTPTS)/1,setsar=1,fps=60[s0];\
             [s0]concat=n=1:v=1:a=0[cat]"
        );
    }

    #[test]
    fn filtergraph_mixed_crops_normalise_via_scale_pad() {
        // Stage crop on the outer spans; one odd-valued per-beat crop in the
        // middle (1181,41,471,941 → even-rounded 1180,40,470,940). Mixed dims
        // → the canvas falls back to the full frame and EVERY segment
        // normalises onto it via scale+pad. scale_cap equals the canvas width
        // → no delivery downscale, so the out label stays [cat].
        let stage = CropRect {
            x: 100,
            y: 50,
            w: 1600,
            h: 900,
        };
        let zoom = CropRect {
            x: 1181,
            y: 41,
            w: 471,
            h: 941,
        };
        let segs = vec![
            seg(0, 2_000, 1.0, Some(stage)),
            seg(2_000, 10_000, 8.0, Some(zoom)),
            seg(10_000, 12_000, 1.0, Some(stage)),
        ];
        let (graph, label) = build_filtergraph(&segs, &META_4K60, 3_840);
        assert_eq!(label, "[cat]");
        assert_eq!(
            graph,
            "[0:v]trim=start=0.000:end=2.000,setpts=(PTS-STARTPTS)/1,\
             crop=1600:900:100:50,\
             scale=3840:2160:force_original_aspect_ratio=decrease,\
             pad=3840:2160:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=60[s0];\
             [0:v]trim=start=2.000:end=10.000,setpts=(PTS-STARTPTS)/8,\
             crop=470:940:1180:40,\
             scale=3840:2160:force_original_aspect_ratio=decrease,\
             pad=3840:2160:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=60[s1];\
             [0:v]trim=start=10.000:end=12.000,setpts=(PTS-STARTPTS)/1,\
             crop=1600:900:100:50,\
             scale=3840:2160:force_original_aspect_ratio=decrease,\
             pad=3840:2160:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=60[s2];\
             [s0][s1][s2]concat=n=3:v=1:a=0[cat]"
        );
    }

    #[test]
    fn filtergraph_caps_delivery_width() {
        // Uniform stage crop wider than the cap → the canvas is the crop and
        // one Rust-computed literal downscale lands after the concat
        // (3214x2050 → 1920 x even(2050*1920/3214 = 1224) = 1224).
        let stage = CropRect {
            x: 0,
            y: 0,
            w: 3214,
            h: 2050,
        };
        let segs = vec![seg(0, 3_000, 1.0, Some(stage))];
        let (graph, label) = build_filtergraph(&segs, &META_4K60, 1_920);
        assert_eq!(label, "[outv]");
        assert_eq!(
            graph,
            "[0:v]trim=start=0.000:end=3.000,setpts=(PTS-STARTPTS)/1,\
             crop=3214:2050:0:0,setsar=1,fps=60[s0];\
             [s0]concat=n=1:v=1:a=0[cat];\
             [cat]scale=1920:1224[outv]"
        );
    }

    #[test]
    fn fps_formats_integer_when_whole() {
        assert_eq!(fmt_fps(60.0), "60");
        assert_eq!(fmt_fps(30.0), "30");
        assert!(fmt_fps(30_000.0 / 1_001.0).starts_with("29.97"));
    }

    // --- CLI path defaulting ---------------------------------------------------

    #[test]
    fn default_paths_derive_from_the_raw_capture() {
        let raw = Path::new("captures/playthrough-ingame.mp4");
        assert_eq!(
            default_manifest_path(raw),
            Path::new("captures/playthrough-ingame.timeline.json")
        );
        assert_eq!(
            default_out_path(raw),
            Path::new("captures/playthrough-ingame-final.mp4")
        );
    }
}
