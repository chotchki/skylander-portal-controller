//! PLAN 15.10 — whole-desktop MP4 capture for the play-through recorder.
//!
//! Records the primary monitor to an MP4 on a background thread (via the
//! `windows-capture` crate: `Windows.Graphics.Capture` + a built-in H.264
//! encoder — no external ffmpeg binary) while the async scenario drives the
//! launcher + browser. Start it once both windows are up; `stop()` finalises
//! and flushes the file.
//!
//! Two backends behind one `DesktopCapture` API: Windows uses `windows-capture`
//! (whole monitor); **macOS** uses the `screencapturekit` crate to capture the
//! main display to an HEVC MP4 via `SCRecordingOutput` (PLAN A.1 — the B-spike
//! winner; the crate handles the SCKit threading / run-loop / idle-frame
//! filtering that raw objc2 stumbled on). Per-window capture + the 2-pane
//! composite are PLAN A.5. Other targets keep a no-op stub so the workspace
//! still builds.

#[cfg(all(not(windows), not(target_os = "macos")))]
use std::path::Path;

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use anyhow::{Result, anyhow};
    use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
    use windows_capture::encoder::{
        AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder,
    };
    use windows_capture::frame::Frame;
    use windows_capture::graphics_capture_api::InternalCaptureControl;
    use windows_capture::monitor::Monitor;
    use windows_capture::settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    };

    type CapErr = Box<dyn std::error::Error + Send + Sync>;

    /// Capture handler: forwards every arrived frame to the encoder; on close
    /// (external `stop()`) it finalises the MP4. Flags carry the encode
    /// dimensions + output path (the handler runs on the capture thread, so we
    /// can't close over them directly).
    struct Recorder {
        encoder: Option<VideoEncoder>,
    }

    impl GraphicsCaptureApiHandler for Recorder {
        type Flags = (u32, u32, String);
        type Error = CapErr;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            let (w, h, path) = ctx.flags;
            let encoder = VideoEncoder::new(
                VideoSettingsBuilder::new(w, h),
                AudioSettingsBuilder::default().disabled(true),
                ContainerSettingsBuilder::default(),
                path,
            )?;
            Ok(Self {
                encoder: Some(encoder),
            })
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            _capture_control: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            if let Some(enc) = self.encoder.as_mut() {
                enc.send_frame(frame)?;
            }
            Ok(())
        }

        fn on_closed(&mut self) -> Result<(), Self::Error> {
            if let Some(enc) = self.encoder.take() {
                enc.finish()?;
            }
            Ok(())
        }
    }

    /// RAII-ish handle: holds the free-threaded capture; `stop()` joins the
    /// capture thread (running `on_closed` → `encoder.finish()`) so the MP4 is
    /// flushed before it returns.
    pub struct DesktopCapture {
        control: Option<CaptureControl<Recorder, CapErr>>,
    }

    impl DesktopCapture {
        pub fn start(out_path: &Path) -> Result<Self> {
            let monitor = Monitor::primary().map_err(|e| anyhow!("primary monitor: {e}"))?;
            let w = monitor.width().map_err(|e| anyhow!("monitor width: {e}"))?;
            let h = monitor
                .height()
                .map_err(|e| anyhow!("monitor height: {e}"))?;
            let settings = Settings::new(
                monitor,
                CursorCaptureSettings::Default,
                DrawBorderSettings::Default,
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Rgba8,
                (w, h, out_path.to_string_lossy().into_owned()),
            );
            let control = Recorder::start_free_threaded(settings)
                .map_err(|e| anyhow!("start desktop capture: {e}"))?;
            Ok(Self {
                control: Some(control),
            })
        }

        /// Per-window capture is macOS-only (Phase B.1 spike); stubbed on Windows.
        #[allow(dead_code)]
        pub fn start_window(_app: &str, _title: &str, _out_path: &Path) -> Result<Self> {
            Err(anyhow!(
                "per-window capture is macOS-only (Phase B.1 spike)"
            ))
        }

        pub fn stop(mut self) -> Result<()> {
            if let Some(control) = self.control.take() {
                control
                    .stop()
                    .map_err(|e| anyhow!("stop/flush desktop capture: {e}"))?;
            }
            Ok(())
        }
    }
}

#[cfg(windows)]
pub use imp::DesktopCapture;

// --- macOS: ScreenCaptureKit via the `screencapturekit` crate (PLAN A.1) ---
#[cfg(target_os = "macos")]
mod imp_mac {
    use std::path::Path;
    use std::time::Duration;

    use anyhow::{Context, Result, anyhow};
    use screencapturekit::prelude::*;
    use screencapturekit::recording_output::{
        SCRecordingOutput, SCRecordingOutputCodec, SCRecordingOutputConfiguration,
        SCRecordingOutputFileType,
    };

    /// Locate ONE on-screen window owned by `app` whose title contains
    /// `title_contains`. The shared predicate for per-window capture
    /// (`start_window`) and the classifier's per-window grab
    /// (`screen::grab_frame`) — both must target the SAME RPCS3 game viewport
    /// (NOT the RPCS3 main GUI window), so the predicate lives here once.
    ///
    /// Emits a `debug` line per non-empty candidate (incl. `is_on_screen` for
    /// occluded windows) to aid live triage of which window resolves.
    pub fn find_window(app: &str, title_contains: &str) -> Result<SCWindow> {
        let content = SCShareableContent::get().map_err(|e| anyhow!("shareable content: {e:?}"))?;
        let windows = content.windows();
        for w in &windows {
            let a = w
                .owning_application()
                .map(|x| x.application_name())
                .unwrap_or_default();
            if !a.is_empty() {
                tracing::debug!(app = %a, title = ?w.title(), on_screen = w.is_on_screen(), "sckit candidate");
            }
        }
        windows
            .into_iter()
            .find(|w| {
                w.is_on_screen()
                    && w.owning_application()
                        .map(|a| a.application_name())
                        .as_deref()
                        == Some(app)
                    && w.title()
                        .map(|t| t.contains(title_contains))
                        .unwrap_or(false)
            })
            .with_context(|| {
                format!("no on-screen window for app {app:?} with title ~{title_contains:?}")
            })
    }

    /// CG/window-server init for a bare CLI — without it `SCStream` aborts with
    /// `CGS_REQUIRE_INIT`. AppKit's lightweight loader does it (no heavy
    /// objc2-app-kit dep). Must run on the main thread, which the recorder's
    /// `current_thread` tokio runtime guarantees; idempotent (AppKit caches).
    fn ensure_cg_init() {
        #[link(name = "AppKit", kind = "framework")]
        unsafe extern "C" {
            fn NSApplicationLoad() -> bool;
        }
        unsafe {
            NSApplicationLoad();
        }
    }

    /// Captures the main display to an HEVC MP4 through `SCRecordingOutput`; the
    /// crate handles the SCKit threading / run-loop / idle-frame filtering
    /// internally. `stop()` halts the stream + lets the file flush.
    pub struct DesktopCapture {
        stream: Option<SCStream>,
        /// Held for the stream's lifetime — the recording writes through it.
        /// `None` when capture is skipped (`SKYLANDER_RECORDER_NO_CAPTURE`).
        _recording: Option<SCRecordingOutput>,
    }

    impl DesktopCapture {
        /// Dev knob shared by BOTH capture entry points: when
        /// `SKYLANDER_RECORDER_NO_CAPTURE` is set, validate the recorder flow
        /// (boot → beats → game launch) WITHOUT capturing — e.g. when the
        /// Screen Recording grant is missing. No file is written; the render
        /// pass is skipped by the caller, and `stop()` is a no-op on the result.
        fn no_capture() -> Option<Self> {
            if std::env::var_os("SKYLANDER_RECORDER_NO_CAPTURE").is_some() {
                tracing::warn!(
                    "SKYLANDER_RECORDER_NO_CAPTURE set — skipping screen capture (flow validation only)"
                );
                return Some(Self {
                    stream: None,
                    _recording: None,
                });
            }
            None
        }

        /// Capture the whole main display (the A.1 baseline).
        pub fn start(out_path: &Path) -> Result<Self> {
            ensure_cg_init();
            if let Some(noop) = Self::no_capture() {
                return Ok(noop);
            }
            let content =
                SCShareableContent::get().map_err(|e| anyhow!("shareable content: {e:?}"))?;
            let display = content
                .displays()
                .into_iter()
                .next()
                .context("no display to capture")?;
            let (w, h) = (display.width(), display.height());
            let filter = SCContentFilter::create()
                .with_display(&display)
                .with_excluding_windows(&[])
                .build();
            Self::start_with_filter(filter, w, h, out_path)
        }

        /// Capture ONE on-screen window, matched by owning-application name +
        /// a title substring (PLAN A.5 per-window 2-stream capture). The two
        /// demo panes — the SPA phone window and the RPCS3 game window — are
        /// each captured this way, then composited side-by-side by the render.
        /// Not yet wired into the recorder (the 2-stream Boot refactor); kept
        /// ready + verified via the spike.
        #[allow(dead_code)]
        pub fn start_window(app: &str, title_contains: &str, out_path: &Path) -> Result<Self> {
            ensure_cg_init();
            if let Some(noop) = Self::no_capture() {
                return Ok(noop);
            }
            let window = find_window(app, title_contains)?;
            let f = window.frame();
            // Capture at backing pixels (≈2× points) for a crisp pane.
            let (w, h) = ((f.size.width as u32) * 2, (f.size.height as u32) * 2);
            let filter = SCContentFilter::create().with_window(&window).build();
            Self::start_with_filter(filter, w, h, out_path)
        }

        /// Shared: build the stream + recording output for a filter + dims and
        /// start it. Dims are forced even (yuv420p / HEVC).
        fn start_with_filter(
            filter: SCContentFilter,
            w: u32,
            h: u32,
            out_path: &Path,
        ) -> Result<Self> {
            let config = SCStreamConfiguration::new()
                .with_width((w & !1).max(2))
                .with_height((h & !1).max(2))
                // Hide the OS cursor — it gets captured resting over a pane and
                // reads as an accidental artifact. Intentional interaction is shown
                // by the injected tap-ripple on the phone instead (boot()).
                .with_shows_cursor(false);
            let rec_config = SCRecordingOutputConfiguration::new()
                .with_output_url(out_path)
                .with_video_codec(SCRecordingOutputCodec::HEVC)
                .with_output_file_type(SCRecordingOutputFileType::MP4);
            let recording =
                SCRecordingOutput::new(&rec_config).context("create SCRecordingOutput")?;
            let stream = SCStream::new(&filter, &config);
            stream
                .add_recording_output(&recording)
                .map_err(|e| anyhow!("add_recording_output: {e:?}"))?;
            stream
                .start_capture()
                .map_err(|e| anyhow!("start_capture: {e:?}"))?;
            Ok(Self {
                stream: Some(stream),
                _recording: Some(recording),
            })
        }

        pub fn stop(mut self) -> Result<()> {
            if let Some(stream) = self.stream.take() {
                stream
                    .stop_capture()
                    .map_err(|e| anyhow!("stop_capture: {e:?}"))?;
                // Let SCRecordingOutput flush + finalise the moov atom.
                std::thread::sleep(Duration::from_millis(800));
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
pub use imp_mac::DesktopCapture;
// The classifier's per-window grab (`screen::grab_frame`) reuses the same
// game-window predicate as the recorder's per-window capture.
#[cfg(target_os = "macos")]
pub use imp_mac::find_window;

// --- other non-Windows (e.g. Linux/CI): no-op stub so the workspace builds ---
#[cfg(all(not(windows), not(target_os = "macos")))]
pub struct DesktopCapture;

#[cfg(all(not(windows), not(target_os = "macos")))]
impl DesktopCapture {
    pub fn start(_out_path: &Path) -> anyhow::Result<Self> {
        tracing::warn!("desktop capture unimplemented on this platform — recording skipped");
        Ok(Self)
    }
    #[allow(dead_code)]
    pub fn start_window(_app: &str, _title: &str, _out: &Path) -> anyhow::Result<Self> {
        anyhow::bail!("per-window capture unimplemented on this platform")
    }
    pub fn stop(self) -> anyhow::Result<()> {
        Ok(())
    }
}

// --- SceneCapture: the recorder's 2-pane scene capture (PLAN A.5) -------------
// Encapsulates the platform split so `boot()` / `run_narrative` stay platform-
// agnostic:
//   * macOS — TWO per-window SCKit streams (the launcher window, which now hosts
//     the embedded game, + the phone/Chrome window), composited side-by-side into
//     the raw by `finalize` (`render::composite`: phone portrait left, launcher
//     landscape right, 16:9, nothing cropped). True per-surface — no desktop
//     bleed, independent of where the windows sit.
//   * Windows / other — the whole-desktop stream (the existing path); the Win32
//     stage-crop frames it later in the render.
//
// `start` requires BOTH windows to already be on-screen + titled (the per-window
// find matches by app + title), so the recorder opens the launcher + phone first.
pub struct SceneCapture {
    #[cfg(target_os = "macos")]
    game: DesktopCapture,
    #[cfg(target_os = "macos")]
    phone: DesktopCapture,
    #[cfg(target_os = "macos")]
    game_mp4: std::path::PathBuf,
    #[cfg(target_os = "macos")]
    phone_mp4: std::path::PathBuf,
    #[cfg(not(target_os = "macos"))]
    desktop: DesktopCapture,
}

impl SceneCapture {
    /// Start capturing the 2-pane scene. `stem` names the per-pane temp files
    /// (macOS); `final_mp4` is where the desktop stream writes (non-macOS) and the
    /// composite lands (macOS, via `finalize`). Window targets are env-overridable
    /// (`SKYLANDER_GAME_WINDOW_{APP,TITLE}` for the launcher pane — shared with the
    /// classifier — and `SKYLANDER_PHONE_WINDOW_{APP,TITLE}` for the phone).
    pub fn start(stem: &str, final_mp4: &std::path::Path) -> anyhow::Result<Self> {
        #[cfg(target_os = "macos")]
        {
            let game_app = std::env::var("SKYLANDER_GAME_WINDOW_APP")
                .unwrap_or_else(|_| "skylander-portal-controller".to_string());
            let game_title = std::env::var("SKYLANDER_GAME_WINDOW_TITLE")
                .unwrap_or_else(|_| "Skylander Portal Controller".to_string());
            let phone_app = std::env::var("SKYLANDER_PHONE_WINDOW_APP")
                .unwrap_or_else(|_| "Google Chrome".to_string());
            let phone_title = std::env::var("SKYLANDER_PHONE_WINDOW_TITLE")
                .unwrap_or_else(|_| "Skylander Portal Phone".to_string());
            let game_mp4 = std::env::temp_dir().join(format!("{stem}-game.mp4"));
            let phone_mp4 = std::env::temp_dir().join(format!("{stem}-phone.mp4"));
            let game = DesktopCapture::start_window(&game_app, &game_title, &game_mp4)?;
            let phone = DesktopCapture::start_window(&phone_app, &phone_title, &phone_mp4)?;
            tracing::info!(
                game = %game_mp4.display(),
                phone = %phone_mp4.display(),
                "2-pane capture: launcher + phone per-window streams"
            );
            let _ = final_mp4;
            Ok(Self {
                game,
                phone,
                game_mp4,
                phone_mp4,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = stem;
            Ok(Self {
                desktop: DesktopCapture::start(final_mp4)?,
            })
        }
    }

    /// Stop the capture(s) and produce the raw `final_mp4`. macOS: stop both panes
    /// then composite them (phone = controller/left, launcher = game/right). Else
    /// the desktop stream already wrote `final_mp4`; just stop + flush it.
    pub fn finalize(self, final_mp4: &std::path::Path) -> anyhow::Result<()> {
        #[cfg(target_os = "macos")]
        {
            self.game.stop()?;
            self.phone.stop()?;
            crate::render::composite(&self.phone_mp4, &self.game_mp4, final_mp4)?;
            let _ = (&self.game_mp4, &self.phone_mp4);
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.desktop.stop()?;
            let _ = final_mp4;
        }
        Ok(())
    }
}
