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
        _recording: SCRecordingOutput,
    }

    impl DesktopCapture {
        pub fn start(out_path: &Path) -> Result<Self> {
            ensure_cg_init();
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
            let config = SCStreamConfiguration::new().with_width(w).with_height(h);

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
                _recording: recording,
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

// --- other non-Windows (e.g. Linux/CI): no-op stub so the workspace builds ---
#[cfg(all(not(windows), not(target_os = "macos")))]
pub struct DesktopCapture;

#[cfg(all(not(windows), not(target_os = "macos")))]
impl DesktopCapture {
    pub fn start(_out_path: &Path) -> anyhow::Result<Self> {
        tracing::warn!("desktop capture unimplemented on this platform — recording skipped");
        Ok(Self)
    }
    pub fn stop(self) -> anyhow::Result<()> {
        Ok(())
    }
}
