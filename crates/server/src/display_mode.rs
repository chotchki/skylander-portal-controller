//! Display-mode get/set helpers for resolution persistence (PLAN
//! 4.20.x). Goal: pre-set Windows' primary display to a game's
//! preferred resolution before launching RPCS3 so the boot doesn't
//! trigger a mode flicker, which has been masking egui-side animations
//! (Chris 2026-04-24).
//!
//! Strategy:
//!   - Capture the current mode after a game stabilises and persist
//!     it per game serial in SQLite (`game_display_modes` table).
//!   - On the next launch, read the saved mode and call
//!     `ChangeDisplaySettingsEx` to set the primary display to it
//!     before spawning RPCS3.
//!   - Don't restore the desktop mode on game quit (would cause
//!     another flicker mid-session). Restore only on launcher exit
//!     (Farewell completion path).
//!
//! All Win32 APIs are wrapped in safe Rust functions that do their own
//! error mapping; the rest of the server doesn't see `windows::*` types.

use anyhow::{Result, anyhow};

#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    CDS_TYPE, ChangeDisplaySettingsExW, DEVMODEW, DISP_CHANGE_BADDUALVIEW, DISP_CHANGE_BADFLAGS,
    DISP_CHANGE_BADMODE, DISP_CHANGE_BADPARAM, DISP_CHANGE_FAILED, DISP_CHANGE_NOTUPDATED,
    DISP_CHANGE_RESTART, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH,
    ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW,
};

/// Snapshot of a primary-display mode. Stored per-game so subsequent
/// launches can pre-set the same mode and avoid a flicker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

/// Read the current mode of the primary display. Returns `Ok(None)` on
/// non-Windows platforms (the project is Windows-only but the cfg
/// gate keeps the file portable).
#[cfg(windows)]
pub fn get_current() -> Result<Option<DisplayMode>> {
    unsafe {
        let mut dm: DEVMODEW = std::mem::zeroed();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        // `None` for device name = primary display.
        let ok = EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut dm);
        if !ok.as_bool() {
            return Ok(None);
        }
        Ok(Some(DisplayMode {
            width: dm.dmPelsWidth,
            height: dm.dmPelsHeight,
            refresh_hz: dm.dmDisplayFrequency,
        }))
    }
}

#[cfg(not(windows))]
pub fn get_current() -> Result<Option<DisplayMode>> {
    Ok(None)
}

/// Apply `mode` to the primary display. Sets only width/height/refresh
/// — colour depth and orientation are left at their current values
/// (`ChangeDisplaySettingsEx` reads from `dm` only the fields whose
/// `dmFields` bits are set).
///
/// Returns Ok on a confirmed mode change. The "RESTART" outcome
/// (display driver wants a reboot) is mapped to `Err` — we'd rather
/// fail loudly than half-apply a mode and have the user wonder why
/// the next game still flickers.
#[cfg(windows)]
pub fn set(mode: DisplayMode) -> Result<()> {
    unsafe {
        let mut dm: DEVMODEW = std::mem::zeroed();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
        dm.dmPelsWidth = mode.width;
        dm.dmPelsHeight = mode.height;
        dm.dmDisplayFrequency = mode.refresh_hz;

        let result = ChangeDisplaySettingsExW(
            None, // primary display
            Some(&dm),
            None,        // hwnd not needed for global mode set
            CDS_TYPE(0), // apply immediately, no `CDS_TEST`
            None,
        );

        match result {
            DISP_CHANGE_SUCCESSFUL => Ok(()),
            DISP_CHANGE_RESTART => Err(anyhow!(
                "display change requires reboot (mode {}x{}@{}Hz)",
                mode.width,
                mode.height,
                mode.refresh_hz,
            )),
            DISP_CHANGE_BADMODE => Err(anyhow!(
                "display rejected mode {}x{}@{}Hz (BADMODE)",
                mode.width,
                mode.height,
                mode.refresh_hz,
            )),
            DISP_CHANGE_BADPARAM => Err(anyhow!("ChangeDisplaySettingsEx: BADPARAM")),
            DISP_CHANGE_BADFLAGS => Err(anyhow!("ChangeDisplaySettingsEx: BADFLAGS")),
            DISP_CHANGE_BADDUALVIEW => Err(anyhow!("ChangeDisplaySettingsEx: BADDUALVIEW")),
            DISP_CHANGE_NOTUPDATED => Err(anyhow!("ChangeDisplaySettingsEx: NOTUPDATED")),
            DISP_CHANGE_FAILED => Err(anyhow!("ChangeDisplaySettingsEx: FAILED")),
            other => Err(anyhow!("ChangeDisplaySettingsEx: unknown code {:?}", other)),
        }
    }
}

#[cfg(not(windows))]
pub fn set(_mode: DisplayMode) -> Result<()> {
    Err(anyhow!("display_mode::set is Windows-only"))
}

/// Best-effort apply: logs failure but doesn't propagate it. Used by
/// callers where a mode mismatch is preferable to refusing to launch.
pub fn try_set(mode: DisplayMode) -> bool {
    match set(mode) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(?mode, "display_mode::set failed: {e}");
            false
        }
    }
}

// Suppress unused-import warning under non-windows builds where the
// imports above aren't used in practice but the trait `Result` needs
// `anyhow::*` in scope.
#[cfg(not(windows))]
#[allow(dead_code)]
fn _unused() -> Result<()> {
    Err(anyhow::anyhow!("placeholder"))
}
