//! PLAN 15.14 — window tiling + the manifest `stage` region.
//!
//! The raw capture is the whole primary monitor (capture.rs), so without
//! placement the frame shows the taskbar and whatever desktop windows happen
//! to surround the launcher. This module tiles the two recorded surfaces —
//! the egui launcher and the app-mode phone window — edge-to-edge across the
//! primary work area, and exposes that tiled union as the [`Layout::stage`]
//! crop the manifest carries for the `-- render` post-pass
//! (`docs/dev/recorder-beats-framework.md` §5; `timeline::TimelineFile`).
//!
//! Layout math ([`compute_layout`]) is portable and unit-tested on every CI
//! target; the Win32 calls follow capture.rs's cfg pattern (`cfg(windows)`
//! impl + compile-clean stubs elsewhere — the recorder is Windows-focused).
//!
//! **Coordinate space:** everything here is virtual-screen **physical
//! pixels** — [`set_dpi_aware`] must run before any window/monitor API so
//! `SPI_GETWORKAREA` / `SetWindowPos` aren't DPI-virtualised. For the
//! PRIMARY monitor (origin 0,0) these coincide with capture-frame pixels
//! (capture.rs records `Monitor::primary()`), which is what lets the
//! work-area-derived `stage` be used directly as an ffmpeg crop.
//!
//! **Placement targets the VISIBLE frame, verified by read-back.** On
//! Win10/11 a decorated resizable window's rect extends past its visible
//! frame by the DWM invisible resize borders (~7-11px DPI-scaled on
//! left/right/bottom), so [`place_window`] outsets the `SetWindowPos` rect
//! by the measured per-edge delta (`GetWindowRect` vs
//! `DWMWA_EXTENDED_FRAME_BOUNDS`) — naive outer-rect tiling would leave
//! wallpaper slivers between and around the tiles, inside the stage crop.
//! It then reads the achieved frame back and errors on a mismatch, because
//! `SetWindowPos` reports success even when the target clamps the request
//! via `WM_GETMINMAXINFO` (Chrome's minimum window size, the launcher's
//! min-inner-size) — the caller degrades `stage` to `None` rather than
//! cropping live content off a half-tiled frame.

/// A window rectangle in virtual-screen physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The tiled placement: launcher fills the left of the work area, the phone
/// takes a 1:2 column on the right, and `stage` is the union (= the work
/// area) as a manifest crop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub launcher: Rect,
    pub phone: Rect,
    pub stage: crate::timeline::CropRect,
}

/// Round down to the nearest even number, clamped at 0. yuv420p chroma
/// subsampling needs even crop offsets + dimensions, and the primary work
/// area never goes negative (its origin is the virtual-screen origin or a
/// docked taskbar's positive inset).
fn even_floor(v: i32) -> u32 {
    (v.max(0) as u32) & !1
}

/// Split `work` (the primary-monitor work area = screen minus taskbar) into
/// the launcher + phone tiles and the stage crop.
///
/// The phone column targets a 1:2 (w:h) aspect at full work-area height —
/// the SPA's portrait layout — clamped to a third of the width so narrow
/// screens still leave the launcher a usable majority. Width is
/// even-rounded; the launcher absorbs the remainder so the tiling stays
/// edge-to-edge (no desktop sliver between or around the windows).
pub fn compute_layout(work: Rect) -> Layout {
    let phone_w = (work.h / 2).min(work.w / 3) & !1;
    Layout {
        launcher: Rect {
            x: work.x,
            y: work.y,
            w: work.w - phone_w,
            h: work.h,
        },
        phone: Rect {
            x: work.x + work.w - phone_w,
            y: work.y,
            w: phone_w,
            h: work.h,
        },
        // Every field even-rounded independently (yuv420p crop alignment) —
        // at worst the crop loses a 1px sliver at an odd edge.
        stage: crate::timeline::CropRect {
            x: even_floor(work.x),
            y: even_floor(work.y),
            w: even_floor(work.w),
            h: even_floor(work.h),
        },
    }
}

/// Convert a physical-pixel rect into Chrome's DIP coordinate space (divide
/// by the monitor scale, round to nearest). Chrome reads `--window-position`
/// / `--window-size` in DIPs ([`Phone::new_headed_app`] docs), so passing a
/// physical tile straight through on a scaled display opens the window
/// scale× too large — flashing over the launcher in the captured pre-roll
/// until the Win32 placement corrects it. A non-finite or non-positive scale
/// (a hostile/failed DPI query) falls back to 1.0 rather than producing a
/// degenerate rect.
///
/// [`Phone::new_headed_app`]: skylander_e2e_tests::Phone::new_headed_app
pub fn to_dips(r: Rect, scale: f64) -> Rect {
    let s = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let d = |v: i32| (f64::from(v) / s).round() as i32;
    Rect {
        x: d(r.x),
        y: d(r.y),
        w: d(r.w),
        h: d(r.h),
    }
}

#[cfg(windows)]
mod imp {
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result, ensure};
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows::Win32::Graphics::Dwm::{
        DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
    };
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
        SetProcessDpiAwarenessContext,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowTextW, IsIconic, IsWindowVisible, SPI_GETWORKAREA,
        SWP_NOACTIVATE, SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetWindowPos,
        SystemParametersInfoW,
    };

    use super::Rect;

    /// Opt this process into PER_MONITOR_AWARE_V2 so `SPI_GETWORKAREA` and
    /// `SetWindowPos` speak physical pixels. MUST run before any
    /// window/monitor API call. Failure is ignored — the context can only be
    /// set once per process, and "already set" (e.g. via manifest) is fine.
    pub fn set_dpi_aware() {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    }

    /// The primary monitor's work area (screen minus taskbar), in physical
    /// pixels (given [`set_dpi_aware`] ran first).
    pub fn work_area() -> Result<Rect> {
        let mut rect = RECT::default();
        unsafe {
            SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut rect as *mut RECT as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .context("SystemParametersInfoW(SPI_GETWORKAREA)")?;
        }
        Ok(Rect {
            x: rect.left,
            y: rect.top,
            w: rect.right - rect.left,
            h: rect.bottom - rect.top,
        })
    }

    /// The primary monitor's effective DPI scale (1.0 = 96 DPI), for
    /// converting physical-pixel tiles into Chrome's DIP launch hints
    /// ([`super::to_dips`]). The primary monitor's origin is always (0,0),
    /// so `MonitorFromPoint` there can't miss. Falls back to 1.0 if the DPI
    /// query declines — placement then still corrects the window, just from
    /// a worse starting rect.
    pub fn primary_scale() -> f64 {
        unsafe {
            let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
            match GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) {
                Ok(()) if dpi_x > 0 => f64::from(dpi_x) / 96.0,
                _ => 1.0,
            }
        }
    }

    /// Poll (250ms) for a visible, non-minimized, non-cloaked top-level
    /// window whose title EXACTLY equals `title`. Exact-match is
    /// load-bearing: dev boxes have editor / terminal windows whose titles
    /// merely CONTAIN "skylander-portal-controller", and a substring match
    /// would tile one of those instead. The minimized/cloaked filters are
    /// too — `IsWindowVisible` alone returns TRUE for a minimized window and
    /// for one parked on another virtual desktop (DWM-cloaked), so a stale
    /// launcher instance could soak up the placement while the recorded one
    /// stays untiled, and no rect read-back would catch it (the wrong window
    /// really would sit at the rect). Returns the HWND as `isize` so
    /// `windows` types don't leak into the portable caller.
    pub async fn wait_find_window_exact(title: &str, timeout: Duration) -> Option<isize> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(hwnd) = find_window_exact(title) {
                return Some(hwnd);
            }
            if Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    fn find_window_exact(title: &str) -> Option<isize> {
        use windows::core::BOOL;

        struct State<'a> {
            wanted: &'a str,
            hwnd: Option<isize>,
        }

        extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
            unsafe {
                if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                    return BOOL(1);
                }
                // DWM-cloaked = "visible" but not on this desktop (another
                // virtual desktop, a suspended UWP shell). A cloak-query
                // failure keeps the window candidate — only a positive
                // cloak flag disqualifies.
                let mut cloaked = 0u32;
                if DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_CLOAKED,
                    &mut cloaked as *mut u32 as *mut _,
                    size_of::<u32>() as u32,
                )
                .is_ok()
                    && cloaked != 0
                {
                    return BOOL(1);
                }
                // 512 UTF-16 units is plenty — our titles are short
                // ("Skylander Portal Controller" / "Skylander Portal Phone").
                let mut buf = [0u16; 512];
                let len = GetWindowTextW(hwnd, &mut buf);
                if len > 0 {
                    let title = String::from_utf16_lossy(&buf[..len as usize]);
                    let state = &mut *(lparam.0 as *mut State);
                    if title == state.wanted {
                        state.hwnd = Some(hwnd.0 as isize);
                        return BOOL(0); // stop enumeration
                    }
                }
                BOOL(1)
            }
        }

        let mut state = State {
            wanted: title,
            hwnd: None,
        };
        // SAFETY: `state` outlives the EnumWindows call; the callback
        // dereferences the pointer only while EnumWindows is on the stack.
        // The Err return on early-stop is the documented FALSE-from-callback
        // path, not a failure.
        unsafe {
            let _ = EnumWindows(Some(enum_proc), LPARAM(&mut state as *mut State as isize));
        }
        state.hwnd
    }

    /// Per-edge slack between the requested tile and the achieved visible
    /// frame before a placement counts as failed. ±2px absorbs DWM rounding;
    /// a real `WM_GETMINMAXINFO` clamp (Chrome's minimum width, the
    /// launcher's min-inner-size) is tens of pixels and must fail so the
    /// caller degrades `stage` to full-frame.
    const PLACE_TOLERANCE_PX: i32 = 2;

    /// The window's VISIBLE frame in virtual-screen physical pixels.
    /// `DWMWA_EXTENDED_FRAME_BOUNDS` is documented to always be physical
    /// (never DPI-virtualised) and excludes the invisible resize borders;
    /// `GetWindowRect` is the fallback should DWM decline (composition is
    /// always on under Win10/11, so in practice it doesn't).
    fn visible_bounds(hwnd: HWND) -> Result<RECT> {
        let mut frame = RECT::default();
        unsafe {
            if DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut frame as *mut RECT as *mut _,
                size_of::<RECT>() as u32,
            )
            .is_ok()
            {
                return Ok(frame);
            }
            GetWindowRect(hwnd, &mut frame).context("GetWindowRect")?;
        }
        Ok(frame)
    }

    /// Move + resize `hwnd` (from [`wait_find_window_exact`]) so its VISIBLE
    /// frame lands on `r`, then verify by read-back (module docs: the
    /// invisible-border outset and the `WM_GETMINMAXINFO`-clamp failure
    /// mode). The border deltas are size-independent, so measuring them
    /// before the resize stays valid after it — and the read-back catches
    /// any drift regardless. NOACTIVATE so placement never steals focus from
    /// whatever the beats are driving; NOZORDER so the launcher/phone
    /// stacking is untouched.
    pub fn place_window(hwnd: isize, r: Rect) -> Result<()> {
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);

        // Invisible-border deltas: how far the outer window rect extends
        // past the visible frame on each edge (all >= 0; top is usually 0).
        let mut outer = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut outer).context("GetWindowRect")? };
        let frame = visible_bounds(hwnd)?;
        let (dl, dt, dr, db) = (
            frame.left - outer.left,
            frame.top - outer.top,
            outer.right - frame.right,
            outer.bottom - frame.bottom,
        );

        unsafe {
            SetWindowPos(
                hwnd,
                None,
                r.x - dl,
                r.y - dt,
                r.w + dl + dr,
                r.h + dt + db,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .context("SetWindowPos")?;
        }

        // Read-back: SetWindowPos returns success even when the target
        // clamped the request, and a clamped window under the stage crop
        // means silently cut-off content in the final cut.
        let got = visible_bounds(hwnd)?;
        let achieved = Rect {
            x: got.left,
            y: got.top,
            w: got.right - got.left,
            h: got.bottom - got.top,
        };
        let deltas = [
            achieved.x - r.x,
            achieved.y - r.y,
            achieved.w - r.w,
            achieved.h - r.h,
        ];
        ensure!(
            deltas.iter().all(|d| d.abs() <= PLACE_TOLERANCE_PX),
            "window clamped the placement: requested {r:?}, visible frame is {achieved:?}"
        );
        Ok(())
    }
}

// Non-Windows stubs (capture.rs precedent): compile-clean so the workspace
// builds on mac CI; the recorder only tiles on Windows. `work_area` erring
// makes `boot()` take its warn-and-degrade path (no placement, stage=None).
#[cfg(not(windows))]
mod imp {
    use std::time::Duration;

    use super::Rect;

    pub fn set_dpi_aware() {}

    pub fn primary_scale() -> f64 {
        1.0
    }

    pub fn work_area() -> anyhow::Result<Rect> {
        anyhow::bail!("window tiling is Windows-only — recording proceeds untiled")
    }

    pub async fn wait_find_window_exact(_title: &str, _timeout: Duration) -> Option<isize> {
        None
    }

    pub fn place_window(_hwnd: isize, _r: Rect) -> anyhow::Result<()> {
        anyhow::bail!("window tiling is Windows-only")
    }
}

pub use imp::{place_window, primary_scale, set_dpi_aware, wait_find_window_exact, work_area};

#[cfg(test)]
mod tests {
    use super::*;

    /// The HTPC-like case (a 3214x2104 monitor with a bottom taskbar →
    /// 3214x2050 work area at the origin): edge-to-edge tiling, a 1:2 phone
    /// column, and an all-even stage.
    #[test]
    fn htpc_work_area_tiles_edge_to_edge() {
        let work = Rect {
            x: 0,
            y: 0,
            w: 3214,
            h: 2050,
        };
        let l = compute_layout(work);

        // Height/2 (1025) wins over width/3 (1071), even-rounded down.
        assert_eq!(l.phone.w, 1024);
        assert_eq!(l.phone.h, work.h);
        // Edge-to-edge: no desktop sliver between or beside the tiles.
        assert_eq!(l.launcher.w + l.phone.w, work.w);
        assert_eq!(l.launcher.x, 0);
        assert_eq!(l.launcher.x + l.launcher.w, l.phone.x);
        assert_eq!(l.phone.x + l.phone.w, work.x + work.w);

        // Stage covers the full work area, every field even.
        assert_eq!(l.stage.x, 0);
        assert_eq!(l.stage.y, 0);
        assert_eq!(l.stage.w, 3214);
        assert_eq!(l.stage.h, 2050);
        for v in [l.stage.x, l.stage.y, l.stage.w, l.stage.h] {
            assert_eq!(v % 2, 0, "stage field {v} must be even");
        }
    }

    /// A narrow work area: the w/3 clamp beats the h/2 aspect so the
    /// launcher keeps a usable majority of the width.
    #[test]
    fn narrow_work_area_clamps_phone_to_a_third() {
        let work = Rect {
            x: 0,
            y: 0,
            w: 1280,
            h: 1000,
        };
        let l = compute_layout(work);

        // w/3 = 426 < h/2 = 500; 426 is already even.
        assert_eq!(l.phone.w, 426);
        assert_eq!(l.launcher.w, 1280 - 426);
        assert_eq!(l.launcher.w + l.phone.w, work.w);
        assert!(l.launcher.w >= 2 * l.phone.w, "launcher keeps the majority");
    }

    /// Taskbar docked left/top: the work area starts at a non-zero (odd,
    /// here) offset — tiles inherit the offset exactly; the stage
    /// even-rounds every field down.
    #[test]
    fn offset_work_area_keeps_origin_and_even_rounds_stage() {
        let work = Rect {
            x: 63,
            y: 31,
            w: 1601,
            h: 1169,
        };
        let l = compute_layout(work);

        assert_eq!(l.launcher.x, 63);
        assert_eq!(l.launcher.y, 31);
        assert_eq!(l.phone.y, 31);
        assert_eq!(l.phone.x + l.phone.w, work.x + work.w);
        assert_eq!(l.launcher.w + l.phone.w, work.w);

        assert_eq!(l.stage.x, 62);
        assert_eq!(l.stage.y, 30);
        assert_eq!(l.stage.w, 1600);
        assert_eq!(l.stage.h, 1168);
    }

    /// The 150%-scaled dev-box case: Chrome's DIP launch hint is the
    /// physical tile divided by the scale, rounded to nearest (1707.33 →
    /// 1707; 683.33 → 683).
    #[test]
    fn to_dips_divides_by_scale_and_rounds() {
        let phys = Rect {
            x: 2561,
            y: 0,
            w: 1025,
            h: 2050,
        };
        let d = to_dips(phys, 1.5);
        assert_eq!(
            d,
            Rect {
                x: 1707,
                y: 0,
                w: 683,
                h: 1367,
            }
        );
    }

    /// Unscaled displays and degenerate scales (0, negative, NaN — a failed
    /// DPI query must never produce a degenerate hint) pass through 1:1.
    #[test]
    fn to_dips_identity_at_1x_and_on_degenerate_scales() {
        let r = Rect {
            x: 10,
            y: 20,
            w: 470,
            h: 940,
        };
        for s in [1.0, 0.0, -2.0, f64::NAN] {
            assert_eq!(to_dips(r, s), r, "scale {s}");
        }
    }
}
