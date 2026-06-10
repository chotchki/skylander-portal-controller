//! PLAN 15.10 — whole-desktop MP4 capture for the play-through recorder.
//!
//! Records the primary monitor to an MP4 on a background thread (via the
//! `windows-capture` crate: `Windows.Graphics.Capture` + a built-in H.264
//! encoder — no external ffmpeg binary) while the async scenario drives the
//! launcher + browser. Start it once both windows are up; `stop()` finalises
//! and flushes the file.
//!
//! Windows-only. On other targets `DesktopCapture` is a no-op stub so the
//! workspace still builds (mac capture would use the ffmpeg avfoundation path,
//! PLAN 15.2.2 — not wired here).

#[cfg(not(windows))]
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

#[cfg(not(windows))]
pub struct DesktopCapture;

#[cfg(not(windows))]
impl DesktopCapture {
    pub fn start(_out_path: &Path) -> anyhow::Result<Self> {
        tracing::warn!("desktop capture is Windows-only — recording skipped on this platform");
        Ok(Self)
    }
    pub fn stop(self) -> anyhow::Result<()> {
        Ok(())
    }
}
