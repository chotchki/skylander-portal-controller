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
//! **Codecs: AV1 + H.265 dual-encode** (PLAN A.5). One decode/filter pass is
//! `split` into two encoders: **SVT-AV1** (`libsvtav1`, the primary `<video>`
//! source — smaller, broad modern-browser support) and **H.265** (`libx265`,
//! `hvc1` tag so QuickTime/Safari recognise the track, the fallback); both
//! `+faststart`. Emits `<stem>.av1.mp4` (primary) beside the bare
//! `<stem>.mp4` HEVC, so a `<video>` element lists AV1 first and falls back to
//! HEVC. (SVT-AV1 is now on the box — the old libaom-only blocker is gone.)
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

/// The reel fades to black over this many seconds at the very end — the outro
/// (PLAN A.5; the Kaos beat lands, then we fade). Applied after captions,
/// before the dual-encode split.
const FADE_OUT_S: f64 = 1.2;

/// `foo.mp4` → sibling `foo.timeline.json`. MUST mirror `write_timeline` in
/// `main.rs` — the recorder writes the manifest with the same
/// `with_extension` call, so the bare `-- render <raw>` default always finds
/// what a recording run produced.
pub fn default_manifest_path(raw: &Path) -> PathBuf {
    raw.with_extension("timeline.json")
}

/// `foo.mp4` → sibling `foo-final.mp4` (the CLI's defaulted output path — the
/// H.265 fallback; the AV1 primary lands beside it, see [`variant_path`]).
pub fn default_out_path(raw: &Path) -> PathBuf {
    let stem = raw
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "capture".to_owned());
    raw.with_file_name(format!("{stem}-final.mp4"))
}

/// `foo-final.mp4` + `"av1"` → sibling `foo-final.av1.mp4`. The dual-encode
/// (PLAN A.5) emits one file per codec; the `<video>` embed lists them in
/// preference order (AV1 first, the bare `foo-final.mp4` HEVC as fallback).
fn variant_path(out: &Path, codec_tag: &str) -> PathBuf {
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "final".to_owned());
    out.with_file_name(format!("{stem}.{codec_tag}.mp4"))
}

/// `foo.mp4` → sibling `foo-review.mp4` — the `render-review` default output
/// (the 1× beat-labelled tuning cut, A.9.2).
pub fn default_review_out_path(raw: &Path) -> PathBuf {
    let stem = raw
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "capture".to_owned());
    raw.with_file_name(format!("{stem}-review.mp4"))
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

/// Map an INPUT-time millisecond to its OUTPUT-time after the editorial
/// speed-ramps (PLAN A.5 captions): walk the gap-filled, ordered segments
/// accumulating each one's output duration `(end-start)/speed`. Consistent with
/// [`planned_output_ms`] (its return for the last segment's end). A caption's
/// on-screen window is `[map(beat.t_start), map(beat.t_end)]`.
fn input_ms_to_output_ms(input_ms: u64, segs: &[Segment]) -> f64 {
    let mut out = 0.0f64;
    for s in segs {
        let speed = f64::from(s.speed);
        if input_ms < s.end_ms {
            let within = input_ms.saturating_sub(s.start_ms) as f64;
            return out + within / speed;
        }
        out += (s.end_ms - s.start_ms) as f64 / speed;
    }
    out
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

/// Controller pane width on a `canvas_w`×`canvas_h` canvas: the controller
/// scaled to full height, even-rounded (yuv420p), capped so the game pane keeps
/// ≥2px. Its natural width at full height IS the left/right split (PLAN A.5).
fn ctrl_pane_width(ctrl_w: u32, ctrl_h: u32, canvas_w: u32, canvas_h: u32) -> u32 {
    let w = even(((u64::from(canvas_h) * u64::from(ctrl_w)) / u64::from(ctrl_h.max(1))) as u32);
    w.clamp(2, canvas_w - 2)
}

/// PLAN A.5 — the 2-pane composite (the layout chotchki chose): game (launcher,
/// landscape — the "TV") fit into the LEFT column with its macOS title bar cropped
/// so it matches the chrome-free phone, and controller (portrait SPA — the
/// "phone") at full canvas height on the RIGHT. 16:9 canvas, nothing else cropped.
/// Emits a high-quality H.264 intermediate that the editorial [`run`] pass then
/// speed-ramps + dual-encodes (composite + editorial stay decoupled, one concern
/// each). The controller's natural width at full height is the split.
pub fn composite(controller: &Path, game: &Path, out: &Path) -> Result<()> {
    const CANVAS_W: u32 = 1920;
    const CANVAS_H: u32 = 1080;
    // macOS title-bar height to crop off the TOP of BOTH panes so they read
    // chrome-free + matched (28pt title bar × the capture's 2× backing, plus a
    // few px so no sliver remains — chotchki saw a sliver at 56). Tunable.
    const TITLEBAR_PX: u32 = 72;
    let cm = probe(controller).context("probe controller pane")?;
    // Split from the CROPPED phone height so the forced ctrl_w×H scale below
    // preserves its aspect (no vertical squish).
    let ctrl_h = cm.h.saturating_sub(TITLEBAR_PX).max(1);
    let ctrl_w = ctrl_pane_width(cm.w, ctrl_h, CANVAS_W, CANVAS_H);
    let game_w = CANVAS_W - ctrl_w; // the game column width (LEFT); ctrl is the rest (RIGHT)
    // Two equal-height columns `hstack`ed — game (launcher) LEFT, controller (phone)
    // RIGHT. BOTH panes have their title bar cropped so they match; the game is then
    // letterbox-fit + centred in a game_w×H column, and the phone scaled to exactly
    // ctrl_w×H, so the two columns sum to exactly CANVAS_W×CANVAS_H. hstack avoids the
    // pad/overlay-at-an-odd-offset chroma quirk that yields a 1-px-odd canvas under
    // yuv420p. `\` line-continuations strip to one contiguous filtergraph.
    let graph = format!(
        "[1:v]crop=iw:ih-{TITLEBAR_PX}:0:{TITLEBAR_PX},\
         scale={game_w}:{CANVAS_H}:force_original_aspect_ratio=decrease,\
         pad={game_w}:{CANVAS_H}:(ow-iw)/2:(oh-ih)/2:black[gcol];\
         [0:v]crop=iw:ih-{TITLEBAR_PX}:0:{TITLEBAR_PX},scale={ctrl_w}:{CANVAS_H}[pcol];\
         [gcol][pcol]hstack=inputs=2,fps=60[canvas]"
    );
    let bin = tool("FFMPEG", "ffmpeg");
    let status = Command::new(&bin)
        .args(["-y", "-v", "error", "-stats", "-i"])
        .arg(controller)
        .arg("-i")
        .arg(game)
        .args(["-filter_complex", &graph, "-map", "[canvas]", "-an"])
        // High-quality intermediate — the editorial pass re-encodes to AV1/HEVC.
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "14",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ])
        .arg(out)
        .status()
        .with_context(|| spawn_hint(&bin, "FFMPEG"))?;
    ensure!(status.success(), "ffmpeg compositing exited with {status}");
    tracing::info!(
        controller = %controller.display(),
        game = %game.display(),
        out = %out.display(),
        ctrl_w,
        game_w,
        "composite: 2-pane reel written"
    );
    Ok(())
}

/// One timed caption: a pre-rendered PNG ([`crate::caption`]) + its OUTPUT-time
/// window in seconds (PLAN A.5).
struct CaptionOverlay {
    png: PathBuf,
    start_s: f64,
    end_s: f64,
}

/// Render a caption PNG for each captioned beat + compute its OUTPUT-time window
/// (map the beat's input window through the speed-ramps). PNGs land beside `out`
/// (`<stem>.capN.png`) so the render's `overlay` can consume them as inputs.
fn build_caption_overlays(
    beats: &[TimelineEntry],
    segs: &[Segment],
    out: &Path,
) -> Result<Vec<CaptionOverlay>> {
    const CAPTION_PX: f32 = 56.0; // ~5% of a 1080 delivery; tunable
    // Wrap budget: keep the box within the ≤1920 delivery with side margins (A.7).
    const CAPTION_MAX_W: f32 = 1600.0;
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "reel".to_owned());
    let mut overlays = Vec::new();
    for (i, b) in beats.iter().enumerate() {
        let Some(text) = b
            .caption
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        else {
            continue;
        };
        let png = out.with_file_name(format!("{stem}.cap{i}.png"));
        crate::caption::render_caption_png(text, CAPTION_PX, CAPTION_MAX_W, &png)
            .with_context(|| format!("render caption for beat {:?}", b.beat))?;
        overlays.push(CaptionOverlay {
            png,
            start_s: input_ms_to_output_ms(b.t_start_ms, segs) / 1000.0,
            end_s: input_ms_to_output_ms(b.t_end_ms, segs) / 1000.0,
        });
    }
    if !overlays.is_empty() {
        tracing::info!(captions = overlays.len(), "render: caption overlays");
    }
    Ok(overlays)
}

/// Run the dual-encode (PLAN A.5), overlaying any timed captions first. The
/// editorial graph is decoded + run ONCE; captions are `overlay`-ed onto its
/// output (this ffmpeg has no `drawtext`) at a centred lower-third, each gated
/// to its beat's output window via `enable=between(t,…)`; then `split` feeds
/// both encoders (SVT-AV1 primary + libx265/hvc1 fallback). Stdio is inherited
/// so ffmpeg's `-stats` line shows; everything else is `-v error`-quiet.
fn encode(
    raw: &Path,
    graph: &str,
    out_label: &str,
    captions: &[CaptionOverlay],
    planned_ms: u64,
    av1_out: &Path,
    hevc_out: &Path,
) -> Result<()> {
    const MARGIN: u32 = 80; // px from the bottom edge (lower third)
    let bin = tool("FFMPEG", "ffmpeg");
    // Chain caption overlays onto out_label, then split to both encoders.
    let mut g = graph.to_string();
    let mut cur = out_label.to_string();
    for (i, c) in captions.iter().enumerate() {
        let inp = i + 1; // input 0 is `raw`; captions are inputs 1..=N
        g.push_str(&format!(
            ";{cur}[{inp}:v]overlay=x=(W-w)/2:y=H-h-{MARGIN}:\
             enable='between(t,{:.3},{:.3})'[cap{i}]",
            c.start_s, c.end_s
        ));
        cur = format!("[cap{i}]");
    }
    // Fade to black over the final FADE_OUT_S — the reel's outro (the Kaos
    // beat lands, then we fade). `st` is OUTPUT time (post speed-ramp), so it
    // keys off planned_ms; clamp so a very short reel still fades from t=0.
    let fade_st = (planned_ms as f64 / 1000.0 - FADE_OUT_S).max(0.0);
    g.push_str(&format!(
        ";{cur}fade=t=out:st={fade_st:.3}:d={FADE_OUT_S:.3}[faded];\
         [faded]split=2[venc_av1][venc_hevc]"
    ));

    let mut cmd = Command::new(&bin);
    cmd.args(["-y", "-v", "error", "-stats", "-i"]).arg(raw);
    for c in captions {
        cmd.arg("-i").arg(&c.png);
    }
    cmd.args(["-filter_complex", &g])
        // AV1 primary (SVT-AV1): preset 6 ≈ the libx265 crf 22 below.
        .args([
            "-map",
            "[venc_av1]",
            "-an",
            "-c:v",
            "libsvtav1",
            "-preset",
            "6",
            "-crf",
            "32",
        ])
        .args([
            "-pix_fmt",
            "yuv420p",
            "-g",
            "120",
            "-movflags",
            "+faststart",
        ])
        .arg(av1_out)
        // H.265 fallback (hvc1 tag so QuickTime/Safari recognise the track).
        .args([
            "-map",
            "[venc_hevc]",
            "-an",
            "-c:v",
            "libx265",
            "-preset",
            "medium",
            "-crf",
            "22",
        ])
        .args([
            "-tag:v",
            "hvc1",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ])
        .arg(hevc_out);
    let status = cmd.status().with_context(|| spawn_hint(&bin, "FFMPEG"))?;
    ensure!(status.success(), "ffmpeg exited with {status}");
    Ok(())
}

/// `render-concat` (A.8.11) — stitch the standalone `install` wizard clip in
/// FRONT of the already-rendered Tour body. The body (`*-final.mp4`) is complete
/// (captioned + end-faded); the wizard is a raw single-window capture that needs
/// (a) its own caption and (b) normalising onto the body's 1920×1080 delivery
/// canvas. One ffmpeg pass: scale+pad the wizard, overlay its caption for the
/// whole clip, scale+pad the body (a no-op when it's already 1920×1080), `concat`
/// (hard cut), then `split` to the AV1 + H.265 dual-encode (matching [`encode`]).
///
/// The wizard clip carries NO fade — the body already ends on the Tour's
/// fade-to-black, so the only fade is the real outro.
pub fn concat_tour(out: &Path, wizard_raw: &Path, caption: &str, body_final: &Path) -> Result<()> {
    const W: u32 = 1920;
    const H: u32 = 1080;
    const FPS: &str = "60";
    const MARGIN: u32 = 80; // px from bottom — matches `encode`'s lower-third
    const CAPTION_PX: f32 = 56.0;
    const CAPTION_MAX_W: f32 = 1600.0;

    ensure!(
        wizard_raw.is_file(),
        "wizard clip not found: {}",
        wizard_raw.display()
    );
    ensure!(
        body_final.is_file(),
        "Tour body not found: {}",
        body_final.display()
    );

    // Caption PNG for the wizard (same renderer as the body's captions).
    let cap_png = out.with_file_name(format!(
        "{}.install-cap.png",
        out.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "tour".to_owned())
    ));
    crate::caption::render_caption_png(caption.trim(), CAPTION_PX, CAPTION_MAX_W, &cap_png)
        .context("render install caption")?;

    let norm = |label: &str, out_label: &str| {
        format!(
            "{label}scale={W}:{H}:force_original_aspect_ratio=decrease,\
             pad={W}:{H}:(ow-iw)/2:(oh-ih)/2:black,setsar=1,fps={FPS}{out_label}"
        )
    };
    let graph = format!(
        "{wiz};[w0][2:v]overlay=x=(W-w)/2:y=H-h-{MARGIN}[w1];{body};[w1][b0]concat=n=2:v=1[catv];\
         [catv]split=2[venc_av1][venc_hevc]",
        wiz = norm("[0:v]", "[w0]"),
        body = norm("[1:v]", "[b0]"),
    );

    let bin = tool("FFMPEG", "ffmpeg");
    let av1_out = variant_path(out, "av1");
    let status = Command::new(&bin)
        .args(["-y", "-v", "error", "-stats", "-i"])
        .arg(wizard_raw)
        .arg("-i")
        .arg(body_final)
        .arg("-i")
        .arg(&cap_png)
        .args(["-filter_complex", &graph])
        .args([
            "-map",
            "[venc_av1]",
            "-an",
            "-c:v",
            "libsvtav1",
            "-preset",
            "6",
            "-crf",
            "32",
            "-pix_fmt",
            "yuv420p",
            "-g",
            "120",
            "-movflags",
            "+faststart",
        ])
        .arg(&av1_out)
        .args([
            "-map",
            "[venc_hevc]",
            "-an",
            "-c:v",
            "libx265",
            "-preset",
            "medium",
            "-crf",
            "22",
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
    ensure!(status.success(), "ffmpeg concat exited with {status}");
    tracing::info!(
        out = %out.display(),
        av1 = %av1_out.display(),
        wizard = %wizard_raw.display(),
        body = %body_final.display(),
        "render-concat: install opener stitched onto the Tour (AV1 + HEVC)"
    );
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
    // `out` is the H.265 fallback (back-compat default path); AV1 is a sibling
    // `<stem>.av1.mp4` and the primary `<video>` source (PLAN A.5).
    let av1_out = variant_path(out, "av1");
    let captions = build_caption_overlays(&tl.beats, &segs, out)?;
    encode(
        raw, &graph, &out_label, &captions, planned_ms, &av1_out, out,
    )?;

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

    let av1_bytes = std::fs::metadata(&av1_out).map(|m| m.len()).unwrap_or(0);
    let hevc_bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    tracing::info!(
        av1 = %av1_out.display(),
        av1_bytes,
        hevc = %out.display(),
        hevc_bytes,
        "render: dual-encode written (AV1 primary + HEVC fallback)"
    );
    Ok(())
}

// ---------------------------------------------------------------- review cut
//
// A.9.2 (option B): a `render-review` sibling pass for TUNING the speed-ramps.
// It re-emits the raw at 1× — NO trims/setpts/concat, because the whole point is
// that OUTPUT time == raw time, so QuickTime's scrubber timecode maps 1:1 to a
// beat — with each beat's name + current plan (head / filler× / tail) banner-ed
// across its [t_start,t_end] window. chotchki scrubs it, calls the speed-ups by
// beat, then edits the manifest + re-`render`s. This is a tuning AID, NOT a
// speed editor (that's the deferred option C).

/// Review banner font size + distance from the TOP edge (the real captions sit
/// at the lower third, so the debug banner reads clearly up top).
const REVIEW_BANNER_PX: f32 = 40.0;
const REVIEW_BANNER_Y: u32 = 24;
/// Review banners are debug overlays — a wider wrap budget is fine.
const REVIEW_BANNER_MAX_W: f32 = 1840.0;

/// `8.0` → `"8"`, `1.5` → `"1.5"` — speed label without a trailing `.0`.
fn fmt_speed(speed: f32) -> String {
    let s = sanitize_speed(speed);
    if s.fract() == 0.0 {
        format!("{s:.0}")
    } else {
        format!("{s}")
    }
}

/// `6000` → `"6"`, `500` → `"0.5"` — seconds label without a trailing `.0`.
fn fmt_secs_ms(ms: u64) -> String {
    let s = ms as f64 / 1000.0;
    if s.fract() == 0.0 {
        format!("{s:.0}")
    } else {
        format!("{s}")
    }
}

/// One review banner: a pre-rendered name+plan PNG ([`crate::caption`]) and its
/// RAW-time window in seconds (== output time, since review applies no speed-up).
struct ReviewBanner {
    png: PathBuf,
    start_s: f64,
    end_s: f64,
}

/// Build the review filtergraph: optionally crop to `stage`, then chain one
/// centred TOP `overlay` per beat gated to its raw window. Pure (no PNG render,
/// no shell) → unit-tested with golden strings. Returns `(graph, map_label)`;
/// with no banners and no stage the graph is empty and the label is the bare
/// source stream `0:v` (the encode maps it directly).
pub fn build_review_filtergraph(
    banner_windows: &[(f64, f64)],
    stage: Option<CropRect>,
    banner_y: u32,
) -> (String, String) {
    let mut chains: Vec<String> = Vec::new();
    let mut cur = "[0:v]".to_string();
    if let Some(c) = stage {
        chains.push(format!(
            "[0:v]crop={}:{}:{}:{}[base]",
            even(c.w),
            even(c.h),
            even(c.x),
            even(c.y)
        ));
        cur = "[base]".to_string();
    }
    if banner_windows.is_empty() {
        if chains.is_empty() {
            return (String::new(), "0:v".to_string());
        }
        return (chains.join(";"), "[base]".to_string());
    }
    for (i, (start, end)) in banner_windows.iter().enumerate() {
        let inp = i + 1; // input 0 is the raw; banners are inputs 1..=N
        let label = format!("[r{i}]");
        chains.push(format!(
            "{cur}[{inp}:v]overlay=x=(W-w)/2:y={banner_y}:\
             enable='between(t,{start:.3},{end:.3})'{label}"
        ));
        cur = label;
    }
    (chains.join(";"), cur)
}

/// The `-- render-review` entry point (A.9.2): load the manifest, render a
/// name+plan banner PNG per beat, overlay them onto the 1× raw → one cheap H.264
/// `<stem>-review.mp4`. The durable raw (A.9.1) + this cut are the two halves of
/// the tune→`render` loop.
pub fn review(raw: &Path, manifest: &Path, out: &Path) -> Result<()> {
    let tl = TimelineFile::load(manifest)?;
    let meta = probe(raw)?;
    tracing::info!(
        raw = %raw.display(),
        w = meta.w,
        h = meta.h,
        duration_ms = meta.duration_ms,
        beats = tl.beats.len(),
        stage = ?tl.stage,
        "review: probed raw capture (1x beat-label pass)"
    );

    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "review".to_owned());
    let mut banners = Vec::with_capacity(tl.beats.len());
    for (i, b) in tl.beats.iter().enumerate() {
        // Name + the CURRENT plan, so a scrub tells chotchki where he is AND what
        // `render` will do here (e.g. `pick_game · h1s 10× t3s`).
        let text = format!(
            "{} · h{}s {}× t{}s",
            b.beat,
            fmt_secs_ms(b.realtime_head_ms),
            fmt_speed(b.filler_speed),
            fmt_secs_ms(b.realtime_tail_ms),
        );
        let png = out.with_file_name(format!("{stem}.rev{i}.png"));
        crate::caption::render_caption_png(&text, REVIEW_BANNER_PX, REVIEW_BANNER_MAX_W, &png)
            .with_context(|| format!("render review banner for beat {:?}", b.beat))?;
        banners.push(ReviewBanner {
            png,
            start_s: b.t_start_ms as f64 / 1000.0,
            end_s: b.t_end_ms as f64 / 1000.0,
        });
    }

    let windows: Vec<(f64, f64)> = banners.iter().map(|b| (b.start_s, b.end_s)).collect();
    let (graph, label) = build_review_filtergraph(&windows, tl.stage, REVIEW_BANNER_Y);
    tracing::debug!(%graph, "review: filtergraph");
    encode_review(raw, &banners, &graph, &label, out)?;

    let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    ensure!(
        bytes > 0,
        "review encode produced an empty file: {}",
        out.display()
    );
    tracing::info!(out = %out.display(), bytes, "review: 1x beat-labelled cut written");
    Ok(())
}

/// Shell out the review encode: raw + one PNG per banner → a single H.264 cut.
fn encode_review(
    raw: &Path,
    banners: &[ReviewBanner],
    graph: &str,
    label: &str,
    out: &Path,
) -> Result<()> {
    let bin = tool("FFMPEG", "ffmpeg");
    let mut cmd = Command::new(&bin);
    cmd.args(["-y", "-v", "error", "-stats", "-i"]).arg(raw);
    for b in banners {
        cmd.arg("-i").arg(&b.png);
    }
    if graph.is_empty() {
        cmd.args(["-map", label]);
    } else {
        cmd.args(["-filter_complex", graph, "-map", label]);
    }
    cmd.args([
        "-an",
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "20",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
    ])
    .arg(out);
    let status = cmd.status().with_context(|| spawn_hint(&bin, "FFMPEG"))?;
    ensure!(
        status.success(),
        "ffmpeg review encode exited with {status}"
    );
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
            caption: None,
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

    // --- output paths --------------------------------------------------------

    #[test]
    fn variant_path_inserts_codec_tag_before_mp4() {
        let out = Path::new("/tmp/demo-final.mp4");
        assert_eq!(
            variant_path(out, "av1"),
            PathBuf::from("/tmp/demo-final.av1.mp4")
        );
        assert_eq!(
            variant_path(out, "hevc"),
            PathBuf::from("/tmp/demo-final.hevc.mp4")
        );
    }

    #[test]
    fn ctrl_pane_width_portrait_phone_sets_split() {
        // 1:2 portrait phone at 1080 tall → 540 wide; the game gets the rest.
        assert_eq!(ctrl_pane_width(470, 940, 1920, 1080), 540);
        // square controller → 1080.
        assert_eq!(ctrl_pane_width(1080, 1080, 1920, 1080), 1080);
        // an absurdly wide controller is capped so the game pane keeps ≥2px.
        assert_eq!(ctrl_pane_width(4000, 1000, 1920, 1080), 1918);
    }

    #[test]
    fn input_to_output_time_respects_speed_ramps() {
        let segs = vec![
            seg(0, 1000, 1.0, None),
            seg(1000, 3000, 2.0, None),
            seg(3000, 4000, 1.0, None),
        ];
        assert_eq!(input_ms_to_output_ms(0, &segs), 0.0);
        assert_eq!(input_ms_to_output_ms(1000, &segs), 1000.0); // start of the 2× span
        assert_eq!(input_ms_to_output_ms(2000, &segs), 1500.0); // 1000ms into 2× → +500
        assert_eq!(input_ms_to_output_ms(3000, &segs), 2000.0); // end of the 2× span
        // end of input → the full planned output duration.
        assert_eq!(
            input_ms_to_output_ms(4000, &segs),
            planned_output_ms(&segs) as f64
        );
    }

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

    // --- review cut (A.9.2) --------------------------------------------------

    #[test]
    fn review_filtergraph_single_banner_no_stage() {
        let (graph, label) = build_review_filtergraph(&[(1.5, 3.2)], None, 24);
        assert_eq!(label, "[r0]");
        assert_eq!(
            graph,
            "[0:v][1:v]overlay=x=(W-w)/2:y=24:enable='between(t,1.500,3.200)'[r0]"
        );
    }

    #[test]
    fn review_filtergraph_two_banners_with_stage() {
        let stage = CropRect {
            x: 100,
            y: 50,
            w: 1600,
            h: 900,
        };
        let (graph, label) = build_review_filtergraph(&[(0.0, 1.0), (1.0, 2.0)], Some(stage), 24);
        assert_eq!(label, "[r1]");
        assert_eq!(
            graph,
            "[0:v]crop=1600:900:100:50[base];\
             [base][1:v]overlay=x=(W-w)/2:y=24:enable='between(t,0.000,1.000)'[r0];\
             [r0][2:v]overlay=x=(W-w)/2:y=24:enable='between(t,1.000,2.000)'[r1]"
        );
    }

    #[test]
    fn review_filtergraph_no_banners_maps_source_or_crop() {
        // No beats + no stage → map the source stream directly (empty graph).
        assert_eq!(
            build_review_filtergraph(&[], None, 24),
            (String::new(), "0:v".to_string())
        );
        // No beats but a stage crop → just the crop, label the cropped pad.
        let stage = CropRect {
            x: 0,
            y: 0,
            w: 1600,
            h: 900,
        };
        let (graph, label) = build_review_filtergraph(&[], Some(stage), 24);
        assert_eq!(graph, "[0:v]crop=1600:900:0:0[base]");
        assert_eq!(label, "[base]");
    }

    #[test]
    fn default_review_out_path_inserts_suffix() {
        assert_eq!(
            default_review_out_path(Path::new("captures/playthrough-ingame.mp4")),
            PathBuf::from("captures/playthrough-ingame-review.mp4")
        );
    }

    #[test]
    fn fmt_speed_and_secs_trim_whole_numbers() {
        assert_eq!(fmt_speed(8.0), "8");
        assert_eq!(fmt_speed(1.0), "1");
        assert_eq!(fmt_secs_ms(6_000), "6");
        assert_eq!(fmt_secs_ms(500), "0.5");
    }
}
