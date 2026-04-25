//! Windows UIA-backed portal driver. Ported from the Phase 1 `tools/uia-drive`
//! spike, with the trait-based API and Win32 off-screen helper added.

// The UIA automation core is deliberately kept in a deeply-nested `if let`
// style that mirrors the tree-walk it performs — collapsing the `if let
// chains` breaks readability of the Qt-specific traversal logic and this
// file is treated as sensitive (see CLAUDE.md on UIA session gotchas).
#![allow(clippy::collapsible_if)]

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use skylander_core::{SLOT_COUNT, SlotIndex, SlotState};
use tracing::{debug, info, instrument, warn};
use uiautomation::patterns::{UIInvokePattern, UIScrollItemPattern, UIValuePattern};
use uiautomation::types::{ControlType, UIProperty};
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_DOWN, VK_ESCAPE, VK_MENU, VK_RETURN, VK_RIGHT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, PostMessageW, SW_MINIMIZE, SW_RESTORE, SWP_NOSIZE,
    SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP,
};
use windows::core::BOOL;

const READ_VALUE_TIMEOUT: Duration = Duration::from_secs(5);
const LOAD_TIMEOUT: Duration = Duration::from_secs(10);
const CLEAR_TIMEOUT: Duration = Duration::from_secs(3);
const DIALOG_OPEN_TIMEOUT: Duration = Duration::from_secs(5);
const MENU_STEP_PAUSE: Duration = Duration::from_millis(200);
const POLL_INTERVAL: Duration = Duration::from_millis(30);
const OFFSCREEN_POS: (i32, i32) = (-4000, -4000);

/// Which kind of RPCS3 window a UIElement represents. Distinguished by Qt
/// classname — matches the observations in docs/research/rpcs3-control.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    /// The RPCS3 main window (`classname = "main_window"`).
    Main,
    /// Skylanders Manager dialog (`classname = "skylander_dialog"`).
    SkylanderDialog,
    /// "Select Skylander File" native common dialog (`classname = "#32770"`).
    FileDialog,
    /// The boot game viewport — exact class TBD on first real-game launch;
    /// for now we return `Other` so Phase 3 can refine.
    Other,
}

pub fn window_kind(el: &UIElement) -> WindowKind {
    match el.get_classname().as_deref().unwrap_or("") {
        "main_window" => WindowKind::Main,
        "skylander_dialog" => WindowKind::SkylanderDialog,
        "#32770" => WindowKind::FileDialog,
        _ => WindowKind::Other,
    }
}

/// Drives the Skylanders Manager dialog via Windows UI Automation.
///
/// Cheap to construct. Re-resolves widgets on every call; the dialog is a
/// singleton that RPCS3 deletes on close, so caching handles is risky.
///
/// `UIAutomation`'s underlying COM interface is free-threaded (MTA) but the
/// Rust wrapper doesn't expose `Send`/`Sync`. We supply them here because
/// the server serialises all driver calls through a single worker, so there
/// is no real concurrency despite the type living inside an `Arc`.
pub struct UiaPortalDriver {
    automation: UIAutomation,
}

// SAFETY: IUIAutomation is free-threaded. The server guarantees one-at-a-time
// access via its driver job queue.
unsafe impl Send for UiaPortalDriver {}
unsafe impl Sync for UiaPortalDriver {}

impl UiaPortalDriver {
    pub fn new() -> Result<Self> {
        let automation = UIAutomation::new().context("init Windows UI Automation")?;
        Ok(Self { automation })
    }

    fn walker(&self) -> Result<UITreeWalker> {
        self.automation
            .create_tree_walker()
            .context("create UIA tree walker")
    }

    fn main_window(&self, walker: &UITreeWalker) -> Result<UIElement> {
        let root = self.automation.get_root_element()?;
        let mut cur = walker.get_first_child(&root).ok();
        while let Some(el) = cur.clone() {
            if el
                .get_name()
                .map(|n| n.starts_with("RPCS3 "))
                .unwrap_or(false)
            {
                return Ok(el);
            }
            cur = walker.get_next_sibling(&el).ok();
        }
        Err(anyhow!("RPCS3 main window not found (is RPCS3 running?)"))
    }

    fn find_dialog(&self, walker: &UITreeWalker, main: &UIElement) -> Option<UIElement> {
        find_descendant(walker, main, |el| {
            el.get_classname()
                .map(|c| c == "skylander_dialog")
                .unwrap_or(false)
        })
    }

    fn find_group_box(&self, walker: &UITreeWalker, dialog: &UIElement) -> Result<UIElement> {
        find_descendant(walker, dialog, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::Group)
                .unwrap_or(false)
                && el
                    .get_classname()
                    .map(|c| c == "QGroupBox")
                    .unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("'Active Portal Skylanders' group box not found"))
    }

    /// Find a slot row by its "Skylander N" label. Returns the row's four
    /// actionable widgets in layout order: (edit, clear_btn, create_btn, load_btn).
    fn find_row(
        &self,
        walker: &UITreeWalker,
        group: &UIElement,
        slot: SlotIndex,
    ) -> Result<(UIElement, UIElement, UIElement, UIElement)> {
        let want = format!("Skylander {}", slot.display());
        let mut cur = walker.get_first_child(group).ok();
        while let Some(el) = cur.clone() {
            let name = el.get_name().unwrap_or_default();
            let ct = el.get_control_type().ok();
            if name == want && ct == Some(ControlType::Text) {
                let edit = walker.get_next_sibling(&el)?;
                let clear = walker.get_next_sibling(&edit)?;
                let create = walker.get_next_sibling(&clear)?;
                let load = walker.get_next_sibling(&create)?;
                return Ok((edit, clear, create, load));
            }
            cur = walker.get_next_sibling(&el).ok();
        }
        Err(anyhow!("row for {slot} not found in the portal dialog"))
    }

    /// Open the Skylanders Manager dialog by driving the Manage menu with
    /// synthesised keystrokes.
    ///
    /// Why not UIA patterns? Qt6 menus don't honour UIA `Invoke`/
    /// `ExpandCollapse` — both patterns return success but the submenu never
    /// populates in the UIA tree. Keyboard navigation is the only reliable
    /// mechanism we found (see `docs/research/game-launch-window-mgmt.md`).
    ///
    /// Sequence: minimise the game viewport → move the main window off-screen
    /// → AttachThreadInput + SetForegroundWindow on main → `Alt` → `Right`×3
    /// (to Manage) → `Down` (open submenu) → `Down`×3 (to "Portals and
    /// Gates") → `Right` (expand) → `Enter`. After each keystroke we verify
    /// via UIA `has_keyboard_focus` that the expected MenuItem is focused —
    /// if RPCS3 ever reorders its menu this fails fast with a clear error.
    /// As soon as the dialog appears we sling it off-screen so the user
    /// doesn't see it linger.
    ///
    /// Runs once per RPCS3 session; subsequent `open_dialog` calls hit the
    /// short-circuit in the caller.
    fn trigger_dialog_via_menu(&self, walker: &UITreeWalker, main: &UIElement) -> Result<()> {
        let main_hwnd: isize = main
            .get_native_window_handle()
            .context("main window has no native HWND")?
            .into();
        let main_hwnd = HWND(main_hwnd as _);
        let _guard = prep_main_offscreen(main_hwnd);

        // Retry the whole nav — RPCS3 drops menu events during heavy work
        // (shader compile, update check popup, etc.). On failure we Esc out of
        // any partial menu state, back off briefly, then try again. Total
        // budget: `NAV_BUDGET`. First attempt has no idle wait; subsequent
        // attempts wait 250ms * attempt_number.
        const NAV_BUDGET: Duration = Duration::from_secs(30);
        let start = Instant::now();
        let mut attempt = 0u32;
        let mut last_err: Option<anyhow::Error> = None;
        while Instant::now().saturating_duration_since(start) < NAV_BUDGET {
            attempt += 1;
            if attempt > 1 {
                // Dismiss whatever partial state we left — two Esc presses
                // exit menu focus mode regardless of nesting depth, and
                // `SetForegroundWindow` + short sleep lets the GUI settle.
                let _ = send_key(VK_ESCAPE);
                let _ = send_key(VK_ESCAPE);
                sleep(Duration::from_millis(250 * attempt as u64));
            }
            match self.attempt_menu_nav(walker, main, main_hwnd) {
                Ok(()) => {
                    // Navigation complete; wait for the dialog window. If it
                    // doesn't appear, treat as a full-nav failure and retry.
                    let deadline = Instant::now() + DIALOG_OPEN_TIMEOUT;
                    while Instant::now() < deadline {
                        if let Ok(dialog_hwnd) = crate::hide::find_dialog_hwnd() {
                            unsafe {
                                let _ = SetWindowPos(
                                    dialog_hwnd,
                                    None,
                                    OFFSCREEN_POS.0,
                                    OFFSCREEN_POS.1,
                                    0,
                                    0,
                                    SWP_NOSIZE | SWP_NOZORDER,
                                );
                            }
                            info!(
                                attempt,
                                "Skylanders Manager dialog opened and moved off-screen"
                            );
                            return Ok(());
                        }
                        sleep(POLL_INTERVAL);
                    }
                    last_err = Some(anyhow!(
                        "attempt {attempt}: Enter sent but dialog never appeared"
                    ));
                }
                Err(e) => {
                    debug!(attempt, "nav failed: {e}");
                    last_err = Some(e);
                }
            }
        }

        // Budget exhausted — dismiss menu so we don't leave RPCS3 weird.
        let _ = send_key(VK_ESCAPE);
        let _ = send_key(VK_ESCAPE);
        Err(last_err.unwrap_or_else(|| {
            anyhow!("Skylanders Manager dialog didn't appear within {NAV_BUDGET:?}")
        }))
    }

    /// One attempt at the keyboard navigation sequence (Alt → Right×3 → Down
    /// → Down×3 → Right → Enter). Returns Err if any focus-verification step
    /// fails so the caller can retry.
    fn attempt_menu_nav(
        &self,
        walker: &UITreeWalker,
        main: &UIElement,
        main_hwnd: HWND,
    ) -> Result<()> {
        focus_main_window(main_hwnd).context("focus main window")?;
        sleep(MENU_STEP_PAUSE);

        send_key(VK_MENU)?;
        sleep(MENU_STEP_PAUSE);
        expect_focused_menu_item(walker, main, "File", "Alt tap")?;

        for _ in 0..3 {
            focus_main_window(main_hwnd).ok();
            send_key(VK_RIGHT)?;
            sleep(MENU_STEP_PAUSE);
        }
        expect_focused_menu_item(walker, main, "Manage", "Right×3 to Manage")?;

        focus_main_window(main_hwnd).ok();
        send_key(VK_DOWN)?;
        sleep(MENU_STEP_PAUSE);
        expect_focused_menu_item(walker, main, "Virtual File System", "open Manage submenu")?;

        for _ in 0..3 {
            focus_main_window(main_hwnd).ok();
            send_key(VK_DOWN)?;
            sleep(MENU_STEP_PAUSE);
        }
        expect_focused_menu_item(
            walker,
            main,
            "Portals and Gates",
            "Down×3 to Portals and Gates",
        )?;

        focus_main_window(main_hwnd).ok();
        send_key(VK_RIGHT)?;
        sleep(MENU_STEP_PAUSE);
        expect_focused_menu_item(
            walker,
            main,
            "Skylanders Portal",
            "expand Portals and Gates",
        )?;

        send_key(VK_RETURN)?;
        Ok(())
    }

    /// Wait for a top-level or nested window matching `title` to appear.
    fn wait_for_child_window(
        &self,
        walker: &UITreeWalker,
        title: &str,
        timeout: Duration,
    ) -> Result<UIElement> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(root) = self.automation.get_root_element() {
                if let Some(hit) = find_descendant(walker, &root, |el| {
                    el.get_name().map(|n| n.contains(title)).unwrap_or(false)
                        && el
                            .get_control_type()
                            .map(|c| c == ControlType::Window)
                            .unwrap_or(false)
                }) {
                    return Ok(hit);
                }
            }
            sleep(POLL_INTERVAL);
        }
        Err(anyhow!(
            "window containing '{title}' didn't appear within {timeout:?}"
        ))
    }

    /// Hide the dialog by moving it far off-screen via raw Win32
    /// `SetWindowPos`. UIA's TransformPattern reports success but doesn't
    /// actually move the Qt window (see docs/research/rpcs3-control.md).
    ///
    /// Idempotent: if the dialog is already near the off-screen sentinel
    /// coordinates (within 100px) this is a no-op.
    pub fn hide_dialog_offscreen(&self) -> Result<()> {
        self.move_dialog_to(OFFSCREEN_POS.0, OFFSCREEN_POS.1)
    }

    /// Put the dialog back on-screen at the given coordinates and raise it.
    pub fn restore_dialog_visible(&self, x: i32, y: i32) -> Result<()> {
        let hwnd = crate::hide::find_dialog_hwnd()?;
        crate::hide::set_position_and_show(hwnd, x, y)?;
        info!(x, y, "dialog restored and raised");
        Ok(())
    }

    fn move_dialog_to(&self, x: i32, y: i32) -> Result<()> {
        let hwnd = crate::hide::find_dialog_hwnd()?;
        crate::hide::set_position_raw(hwnd, x, y)?;
        info!(x, y, "dialog moved");
        Ok(())
    }

    /// Boot a game from RPCS3's library view by its serial (e.g. `BLUS30968`).
    ///
    /// Prereq: RPCS3 was launched via `RpcsProcess::launch_library` so the
    /// library grid is visible. UIA-`select()`s the `DataItem` with the given
    /// `serial`, then sends keyboard `Space` + `Enter` to the main window.
    ///
    /// **Why keystrokes instead of Play-button Invoke**: UIA
    /// `SelectionItemPattern.select()` on a Qt `QListView` `DataItem` sets
    /// the item's visual "marked" state but does not update the list
    /// widget's `currentIndex`. The toolbar Play button is bound to
    /// `currentIndex`, so after bare `select()` the Play button stays
    /// disabled (greyed out) and UIA `invoke()` on it is a no-op. Pressing
    /// `Space` on the keyboard promotes the marked item to current, making
    /// Play active; `Enter` then activates Play and boots the game. With
    /// the Skylanders Manager dialog already open (normal flow order),
    /// synthesised mouse events are absorbed by the dialog but keyboard
    /// input still routes to the library grid, so this keystroke-based
    /// path works in both cold-library and dialog-open states.
    ///
    /// Succeeds when a viewport window (`Qt6110QWindowIcon` class, title
    /// prefix `"FPS:"`) appears within `timeout`.
    /// Read the title of the currently-running game viewport window, if any.
    /// Returns `None` when no game is booted. The title has format
    /// `"FPS: <n> | <MHz-spec> | <frametime> | <game name>"` (Qt 6.11 RPCS3) —
    /// callers can substring-search for the expected game name.
    pub fn running_viewport_title(&self) -> Option<String> {
        let hwnd = find_viewport_hwnd()?;
        read_title(hwnd)
    }

    /// Quit RPCS3 via the File → Exit menu path. Mirrors the Manage-menu
    /// approach in `trigger_dialog_via_menu`: minimises the game viewport so
    /// it can't steal focus, moves the main window off-screen so the Alt
    /// highlight isn't visible, then drives the menu via synthesised
    /// keystrokes verifying `has_keyboard_focus` at each step.
    ///
    /// Does NOT wait for the process to exit — pair with
    /// `RpcsProcess::wait_for_exit_or_force` for that.
    pub fn quit_via_file_menu(&self) -> Result<()> {
        let walker = self.walker()?;
        let main = self.main_window(&walker)?;
        let main_hwnd: isize = main.get_native_window_handle().context("main HWND")?.into();
        let main_hwnd = HWND(main_hwnd as _);
        let _guard = prep_main_offscreen(main_hwnd);

        focus_main_window(main_hwnd).context("focus main window")?;
        sleep(MENU_STEP_PAUSE);

        // Alt tap: File is the leftmost menu, so it takes focus immediately.
        send_key(VK_MENU)?;
        sleep(MENU_STEP_PAUSE);
        expect_focused_menu_item(&walker, &main, "File", "Alt tap")?;

        // Down opens the File submenu (first item focused), then Up wraps to
        // the last item — which is "Exit" on every RPCS3 build we've seen.
        // Beats walking the full menu with Down×N. If Qt ever stops wrapping
        // or Exit moves off the bottom, the expect_focused_menu_item check
        // below catches it.
        focus_main_window(main_hwnd).ok();
        send_key(VK_DOWN)?;
        sleep(MENU_STEP_PAUSE);
        focus_main_window(main_hwnd).ok();
        send_key(VK_UP)?;
        sleep(MENU_STEP_PAUSE);

        // Collect focused items across main + desktop-popup trees — Qt keeps
        // "File" keyboard-focused on the menubar *in addition to* the focused
        // dropdown item, so a single-match helper returned "File" repeatedly.
        let mut focused = Vec::new();
        collect_focused_menu_items(&walker, &main, &mut focused);
        if let Ok(root) = self.automation.get_root_element() {
            collect_focused_menu_items(&walker, &root, &mut focused);
        }
        let on_exit = focused
            .iter()
            .any(|n| normalise_menu_name(n).eq_ignore_ascii_case("exit"));
        if !on_exit {
            // Dismiss so we don't leave RPCS3 in menu mode.
            let _ = send_key(VK_ESCAPE);
            let _ = send_key(VK_ESCAPE);
            bail!(
                "expected Up-wrap to land on 'Exit' at bottom of File menu; \
                 focused items: {focused:?}"
            );
        }
        debug!(?focused, "File → Exit focused via Up-wrap");

        send_key(VK_RETURN)?;
        info!("sent Enter on File → Exit");

        // RPCS3 sometimes pops a "confirm quit" dialog when a game is
        // running. A second Enter after a brief settle lands on the default
        // (Yes) button. If there's no dialog it's harmlessly ignored.
        sleep(MENU_STEP_PAUSE);
        let _ = send_key(VK_RETURN);
        Ok(())
    }

    /// One boot attempt: fresh UIA tree walk → find cell for `serial` →
    /// post coord-click at the cell's bounding rect + synthesised Enter
    /// → poll for viewport → verify title. Called in a retry loop by
    /// [`PortalDriver::boot_game_by_serial`]; each invocation re-walks
    /// UIA so a stale-rect mis-click on attempt N gets a fresh lookup on
    /// attempt N+1.
    ///
    /// History 2026-04-24: briefly swapped to `UIInvokePattern::invoke()`
    /// after a run where coord-click kept hitting the wrong row — that
    /// turned out to be the documented RDP-degrades-eframe / UIA-coord
    /// drift gremlin, not a real code bug. On a physical console, Invoke
    /// alone doesn't boot (CLAUDE.md RPCS3 notes) and coord-click + Enter
    /// is the working path. Reverted.
    #[instrument(skip(self), fields(attempt))]
    fn try_boot_once(
        &self,
        serial: &str,
        expected_name: &str,
        timeout: Duration,
        _attempt: u32,
    ) -> Result<()> {
        let walker = self.walker()?;
        let main = self.main_window(&walker)?;
        let main_hwnd: isize = main.get_native_window_handle().context("main HWND")?.into();
        let main_hwnd = HWND(main_hwnd as _);

        let cell = find_descendant(&walker, &main, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::DataItem)
                .unwrap_or(false)
                && el.get_name().map(|n| n == serial).unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("no DataItem named {serial} in RPCS3 library"))?;
        debug!(serial, "found library cell");

        // Scroll the target cell into the visible viewport before reading
        // its rect. 2026-04-24 Chris hit a consistent mis-click where the
        // UIA-reported coords for SuperChargers landed on Imaginators'
        // rendered row — the library had scrolled enough that the target
        // row was off-screen, and UIA's rect was in the un-scrolled
        // coordinate system. `ScrollItemPattern::scroll_into_view()` tells
        // Qt to update the list's scroll position so the cell is actually
        // on screen; the subsequent rect re-query returns a coordinate
        // that matches what's rendered. Best-effort — if the pattern
        // isn't exposed we still have the retry loop.
        if let Ok(scroll) = cell.get_pattern::<UIScrollItemPattern>() {
            if let Err(e) = scroll.scroll_into_view() {
                debug!(serial, "scroll_into_view errored: {e}");
            } else {
                debug!(serial, "scrolled library cell into view");
            }
        }
        sleep(Duration::from_millis(150));

        // Re-fetch the cell AFTER scrolling — bounding rect now reflects
        // the scrolled position rather than the stale pre-scroll value.
        let cell_after = find_descendant(&walker, &main, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::DataItem)
                .unwrap_or(false)
                && el.get_name().map(|n| n == serial).unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("cell for {serial} disappeared after scroll"))?;

        let rect = cell_after
            .get_bounding_rectangle()
            .context("cell bounding rectangle")?;
        let cx_screen = rect.get_left() + (rect.get_right() - rect.get_left()) / 2;
        let cy_screen = rect.get_top() + (rect.get_bottom() - rect.get_top()) / 2;

        // 2026-04-24 — on the HTPC's 4K display we were seeing UIA rects
        // at physical pixels (cx=1600, cy=1293) while PostMessage WM_
        // LBUTTONDOWN's lParam needs CLIENT-area coords that respect the
        // window's DPI context. Naive `cx_screen - wrect.left` gave the
        // wrong row because `GetWindowRect` returns the non-client area
        // (title bar etc.) offset, and the window's internal client-area
        // coord system may be at a different DPI scale than the outer
        // screen. `ScreenToClient` does the right conversion in one
        // shot — it applies the window's DPI context to translate
        // physical screen pixels into the coord system the window's
        // message loop expects.
        let mut pt = POINT {
            x: cx_screen,
            y: cy_screen,
        };
        unsafe {
            let _ = ScreenToClient(main_hwnd, &mut pt);
        }
        let lx = pt.x;
        let ly = pt.y;
        debug!(
            cx_screen,
            cy_screen,
            lx,
            ly,
            rect_left = rect.get_left(),
            rect_top = rect.get_top(),
            rect_right = rect.get_right(),
            rect_bottom = rect.get_bottom(),
            "posting WM_LBUTTONDOWN/UP to library cell"
        );

        post_click(main_hwnd, lx, ly).context("post library-cell click")?;
        sleep(MENU_STEP_PAUSE);
        post_key(main_hwnd, VK_RETURN).context("post Enter to boot selected game")?;

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(vp) = find_viewport_hwnd() {
                let title = read_title(vp).unwrap_or_default();
                if let Err(e) = verify_viewport_title(&title, expected_name) {
                    info!(
                        serial,
                        ?title,
                        expected = expected_name,
                        "viewport title mismatch — booted wrong game",
                    );
                    return Err(e);
                }
                info!(serial, "game viewport detected — boot succeeded");
                return Ok(());
            }
            sleep(POLL_INTERVAL);
        }
        bail!("game viewport didn't appear within {timeout:?} after boot attempt");
    }
}

impl crate::PortalDriver for UiaPortalDriver {
    #[instrument(skip(self))]
    fn open_dialog(&self) -> Result<()> {
        let walker = self.walker()?;
        let main = self.main_window(&walker)?;

        if self.find_dialog(&walker, &main).is_some() {
            debug!("dialog already open");
            return Ok(());
        }
        info!("opening dialog via Manage menu (keyboard navigation)");
        self.trigger_dialog_via_menu(&walker, &main)?;

        // `trigger_dialog_via_menu` already waits for the dialog and slings
        // it off-screen as soon as it appears. Nothing more to do here.
        Ok(())
    }

    #[instrument(skip(self))]
    fn read_slots(&self) -> Result<[SlotState; SLOT_COUNT]> {
        let walker = self.walker()?;
        let main = self.main_window(&walker)?;
        let dialog = self
            .find_dialog(&walker, &main)
            .ok_or_else(|| anyhow!("dialog not open"))?;
        let group = self.find_group_box(&walker, &dialog)?;

        let mut out: [SlotState; SLOT_COUNT] = std::array::from_fn(|_| SlotState::Empty);
        for (i, out_slot) in out.iter_mut().enumerate().take(SLOT_COUNT) {
            let slot = SlotIndex::new(i as u8).unwrap();
            let (edit, _, _, _) = self.find_row(&walker, &group, slot)?;
            let value = read_value(&edit, READ_VALUE_TIMEOUT)?;
            *out_slot = interpret_slot_value(&value);
        }
        Ok(out)
    }

    #[instrument(skip(self), fields(slot = %slot, path = %path.display()))]
    fn load(&self, slot: SlotIndex, path: &Path) -> Result<String> {
        let abs = std::fs::canonicalize(path)
            .with_context(|| format!("cannot canonicalize {}", path.display()))?;
        let abs_str = abs
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();

        let walker = self.walker()?;
        let main = self.main_window(&walker)?;
        let dialog = self
            .find_dialog(&walker, &main)
            .ok_or_else(|| anyhow!("dialog not open (call open_dialog first)"))?;
        let group = self.find_group_box(&walker, &dialog)?;
        let (edit, clear_btn, _create_btn, load_btn) = self.find_row(&walker, &group, slot)?;

        let before = read_value(&edit, READ_VALUE_TIMEOUT)?;
        if before != "None" && !before.is_empty() {
            debug!(previous = %before, "slot occupied; clearing before load");
            clear_btn.get_pattern::<UIInvokePattern>()?.invoke()?;
            wait_for_value(&edit, "None", CLEAR_TIMEOUT)?;
        }

        load_btn.get_pattern::<UIInvokePattern>()?.invoke()?;

        // Poll `EnumWindows` for any newly-visible `#32770` common-dialog
        // frame(s) — RPCS3's "Select Skylander File" is a native Windows
        // common dialog, and Qt clamps it to on-screen coords whenever its
        // parent (the off-screen Skylanders Manager) is off-screen. We
        // sling every visible #32770 off-screen ourselves, then re-check
        // on each poll iteration because Windows can re-parent/reposition
        // the dialog slightly after creation. Polling at 5ms beats the
        // 30ms sampler cadence in tests and stays well ahead of any
        // paint that the user would perceive.
        let sling_deadline = Instant::now() + DIALOG_OPEN_TIMEOUT;
        let mut slung_any = false;
        loop {
            let hits = find_visible_file_dialogs();
            for hwnd in &hits {
                unsafe {
                    if SetWindowPos(
                        *hwnd,
                        None,
                        OFFSCREEN_POS.0,
                        OFFSCREEN_POS.1,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER,
                    )
                    .is_ok()
                    {
                        slung_any = true;
                    }
                }
            }
            // Once something was slung AND nothing on-screen remains, stop.
            if slung_any
                && !hits.iter().any(|h| {
                    let mut r = RECT::default();
                    unsafe {
                        GetWindowRect(*h, &mut r).ok();
                    }
                    r.left > -1000 || r.top > -1000
                })
            {
                break;
            }
            if Instant::now() >= sling_deadline {
                if !slung_any {
                    bail!("file dialog #32770 HWND didn't appear within {DIALOG_OPEN_TIMEOUT:?}");
                }
                break;
            }
            sleep(Duration::from_millis(5));
        }
        debug!(?OFFSCREEN_POS, "file dialog(s) slung off-screen");

        let file_dlg =
            self.wait_for_child_window(&walker, "Select Skylander File", DIALOG_OPEN_TIMEOUT)?;

        // Retry loop for file_edit + open_btn (PLAY_TEST #19 —
        // 2026-04-24). Immediately after Windows creates the file
        // dialog and we sling it off-screen, the UIA subtree can take
        // a few frames to settle — the first enumeration sometimes
        // misses the Open button. Poll up to 500ms at 50ms intervals.
        //
        // Also: match the Open button by AutomationId alone. The id
        // "1" is IDOK — a Win32 constant, locale-independent. The
        // Name ("Open") is translated on non-English Windows builds
        // and races an empty initial value right after window
        // creation, both of which masked as "Open button not found"
        // in Chris's test session.
        let find_deadline = Instant::now() + Duration::from_millis(500);
        let (file_edit, open_btn) = loop {
            let edit = find_descendant(&walker, &file_dlg, |el| {
                el.get_control_type()
                    .map(|c| c == ControlType::Edit)
                    .unwrap_or(false)
                    && el.get_automation_id().map(|a| a == "1148").unwrap_or(false)
            });
            let btn = find_descendant(&walker, &file_dlg, |el| {
                el.get_control_type()
                    .map(|c| c == ControlType::Button)
                    .unwrap_or(false)
                    && el.get_automation_id().map(|a| a == "1").unwrap_or(false)
            });
            match (edit, btn) {
                (Some(e), Some(b)) => break (e, b),
                _ if Instant::now() < find_deadline => {
                    sleep(Duration::from_millis(50));
                    continue;
                }
                (None, _) => bail!("file-name edit not found (dialog subtree never settled)"),
                (_, None) => {
                    bail!("file-dialog Open button not found (dialog subtree never settled)")
                }
            }
        };

        file_edit
            .get_pattern::<UIValuePattern>()?
            .set_value(&abs_str)?;
        open_btn.get_pattern::<UIInvokePattern>()?.invoke()?;

        // Race the slot value change against three failure modes:
        //  1. RPCS3's QMessageBox ("Failed to open the skylander file!").
        //  2. Windows shell TaskDialog ("This file is in use") nested inside
        //     the file dialog — triggered when the same .sky is already open
        //     on another slot and the shell can't grab an exclusive handle.
        //  3. Bare timeout — neither value change nor any modal.
        let after = match poll_load_outcome(
            &self.automation,
            &walker,
            &main,
            &file_dlg,
            &edit,
            LOAD_TIMEOUT,
        )? {
            LoadOutcome::Changed(v) => v,
            LoadOutcome::QtModal { title, body, modal } => {
                dismiss_modal(&walker, &modal);
                bail!("RPCS3 reported: {}", format_err(title, body));
            }
            LoadOutcome::ShellFileInUse { body } => {
                // Dismiss the task dialog, then cancel the outer file dialog
                // so the next load isn't stuck inside a half-open file picker.
                dismiss_shell_task_dialog(&walker, &file_dlg);
                cancel_file_dialog(&walker, &file_dlg);
                bail!("Windows file in use: {}", body);
            }
            LoadOutcome::Timeout => {
                // Try to back out of the file dialog so the next load can
                // recover without user intervention.
                cancel_file_dialog(&walker, &file_dlg);
                bail!("slot value didn't change after Open (no error modal either)")
            }
        };

        info!(figure = %after, "loaded");
        Ok(after)
    }

    #[instrument(skip(self), fields(slot = %slot))]
    fn clear(&self, slot: SlotIndex) -> Result<()> {
        let walker = self.walker()?;
        let main = self.main_window(&walker)?;
        let dialog = self
            .find_dialog(&walker, &main)
            .ok_or_else(|| anyhow!("dialog not open"))?;
        let group = self.find_group_box(&walker, &dialog)?;
        let (edit, clear_btn, _, _) = self.find_row(&walker, &group, slot)?;

        clear_btn.get_pattern::<UIInvokePattern>()?.invoke()?;
        wait_for_value(&edit, "None", CLEAR_TIMEOUT)?;
        Ok(())
    }

    #[instrument(skip(self))]
    fn boot_game_by_serial(
        &self,
        serial: &str,
        expected_name: &str,
        timeout: Duration,
    ) -> Result<()> {
        // Up to 3 attempts. Each attempt: fresh UIA walk → coord-click at
        // reported rect + Enter → poll for viewport → verify title. If the
        // viewport title identifies a different game (stale UIA rect landed
        // on a sibling row), stop emulation and retry. If the title is
        // correct, success; if it's unrecognisable, trust it and succeed.
        //
        // Earlier strategies that didn't survive live testing 2026-04-24:
        //   - `SelectionItemPattern.select()` + `Space` + `Enter`: Space
        //     resurfaced a QListView quirk that pinned currentIndex at
        //     row 0 (Digimon).
        //   - `select()` + `set_focus()` + `Enter`: `set_focus` hung
        //     indefinitely on the Qt DataItem.
        //   - Keyboard-only arrow-nav tracking `is_selected`: keys
        //     synthesised into main_hwnd didn't reach the library
        //     widget's selection model — `is_selected` stayed false on
        //     every row across 100 Down-arrow presses.
        //
        // Coord-click with a freshly-walked UIA rect works the vast
        // majority of the time; the rare mis-click is self-correcting via
        // the retry loop below.
        const MAX_ATTEMPTS: u32 = 3;
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 1..=MAX_ATTEMPTS {
            match self.try_boot_once(serial, expected_name, timeout, attempt) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    warn!(
                        attempt,
                        serial,
                        expected = expected_name,
                        "boot attempt failed: {e}",
                    );
                    last_err = Some(e);
                    if attempt < MAX_ATTEMPTS {
                        // Recover: stop whatever did boot so the next
                        // attempt starts from a clean library view.
                        // Best-effort; errors here don't mask the
                        // underlying boot failure.
                        if let Err(se) = self.stop_emulation(Duration::from_secs(5)) {
                            warn!(
                                attempt,
                                "recovery stop_emulation before retry errored: {se}",
                            );
                        }
                        sleep(Duration::from_millis(300));
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow!("boot_game_by_serial: no attempts made")))
    }

    #[instrument(skip(self))]
    fn enumerate_games(&self, _timeout: Duration) -> Result<Vec<String>> {
        let walker = self.walker()?;
        let main = self.main_window(&walker)?;
        let items = collect_descendants(&walker, &main, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::DataItem)
                .unwrap_or(false)
        });
        let mut serials: Vec<String> = items
            .iter()
            .filter_map(|el| el.get_name().ok())
            .filter(|n| !n.is_empty())
            .collect();
        serials.sort();
        serials.dedup();
        debug!(count = serials.len(), "enumerated library serials");
        Ok(serials)
    }

    #[instrument(skip(self))]
    fn stop_emulation(&self, timeout: Duration) -> Result<()> {
        let walker = self.walker()?;
        let main = self.main_window(&walker)?;
        let main_hwnd: isize = main.get_native_window_handle().context("main HWND")?.into();
        let main_hwnd = HWND(main_hwnd as _);

        // The Stop control lives on the main RPCS3 window, which is
        // hidden behind the game viewport while a game runs. Same
        // off-screen + viewport-minimize dance as `quit_via_file_menu`
        // so UIA can see the main window's widgets and we can
        // PostMessage a click without the egui cover or game frame
        // getting in the way.
        let _guard = prep_main_offscreen(main_hwnd);

        // Find a Button or MenuItem whose name looks like "Stop" — RPCS3
        // offers both a toolbar "Stop" button and a "Stop Emulation"
        // menu item, plus a few naming variations across versions. We
        // accept any of them and stop at the first hit; Button is
        // preferred (toolbar, directly clickable) but we'll walk the
        // full tree which finds the toolbar button first on RPCS3
        // 0.0.40 because it's a shallower child of the main window
        // than the menu items.
        let target = find_descendant(&walker, &main, |el| {
            let kind = match el.get_control_type() {
                Ok(k) => k,
                Err(_) => return false,
            };
            if !matches!(kind, ControlType::Button | ControlType::MenuItem) {
                return false;
            }
            let name = el.get_name().unwrap_or_default();
            let norm = name.trim().to_lowercase();
            matches!(
                norm.as_str(),
                "stop" | "stop emulation" | "stop game" | "&stop" | "&stop emulation"
            )
        })
        .ok_or_else(|| anyhow!("no Stop button or menu item found on RPCS3 main window"))?;

        // Try UIInvokePattern first — RPCS3's toolbar button responds
        // to it on the Qt 6 builds we've tested (CLAUDE.md's
        // "Invoke doesn't work" caveat was specifically for menu items).
        //
        // Do NOT post Enter afterwards to "dismiss a confirm dialog"
        // (older pattern from quit_via_file_menu): the toolbar Stop
        // button morphs into a Play button the instant emulation
        // halts, and pressing Enter after the invoke lands on the
        // now-focused Play and restarts the game — exactly the
        // "killed and restarted" path Chris flagged 2026-04-21 during
        // HTPC validation. If an RPCS3 build ever defaults to a
        // stop-confirmation dialog, handle it by finding the dialog
        // window + invoking its default button, not by blindly
        // posting Enter to main.
        if let Ok(inv) = target.get_pattern::<UIInvokePattern>() {
            if inv.invoke().is_ok() {
                debug!("invoked Stop via UIInvokePattern");
                return wait_for_viewport_gone(timeout);
            }
        }

        // Fallback: PostMessage click at the button's centre. Matches
        // `boot_game_by_serial`'s pattern — bypasses Z-order so the
        // egui cover doesn't swallow the click. Same "no Enter post"
        // rule applies here: after the click the button has morphed,
        // so a follow-up keypress would hit the wrong control.
        let rect = target
            .get_bounding_rectangle()
            .context("Stop control bounding rect")?;
        let cx = rect.get_left() + (rect.get_right() - rect.get_left()) / 2;
        let cy = rect.get_top() + (rect.get_bottom() - rect.get_top()) / 2;
        let mut wrect = RECT::default();
        unsafe {
            let _ = GetWindowRect(main_hwnd, &mut wrect);
        }
        let lx = cx - wrect.left;
        let ly = cy - wrect.top;
        post_click(main_hwnd, lx, ly).context("post Stop click")?;
        debug!(lx, ly, "posted Stop click to main window");

        wait_for_viewport_gone(timeout)
    }
}

/// Poll until the game viewport window disappears, or `timeout` elapses.
/// Used by `stop_emulation` to confirm the game actually stopped —
/// returns Ok once RPCS3 is back at library view, Err if the viewport
/// is still present after `timeout`.
fn wait_for_viewport_gone(timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if find_viewport_hwnd().is_none() {
            debug!("game viewport gone — stop_emulation confirmed");
            return Ok(());
        }
        sleep(POLL_INTERVAL);
    }
    bail!("game viewport still present after {timeout:?}");
}

fn interpret_slot_value(value: &str) -> SlotState {
    if value.is_empty() || value == "None" {
        SlotState::Empty
    } else {
        // `placed_by` is a server-layer concept (the REST caller's profile);
        // the driver only observes what RPCS3 shows. `None` here is correct
        // — `set_and_broadcast` paths at the server level fill it in on
        // actions that originated through a REST handler.
        SlotState::Loaded {
            figure_id: None,
            display_name: value.to_string(),
            placed_by: None,
        }
    }
}

fn read_value(el: &UIElement, _timeout: Duration) -> Result<String> {
    let variant = el.get_property_value(UIProperty::ValueValue)?;
    Ok(variant.get_string().unwrap_or_default())
}

fn wait_for_value(el: &UIElement, expected: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if read_value(el, Duration::ZERO).unwrap_or_default() == expected {
            return Ok(());
        }
        sleep(POLL_INTERVAL);
    }
    Err(anyhow!(
        "value didn't become '{expected}' within {timeout:?}"
    ))
}

enum LoadOutcome {
    Changed(String),
    /// RPCS3-side Qt modal (QMessageBox).
    QtModal {
        title: String,
        body: String,
        modal: UIElement,
    },
    /// Windows shell TaskDialog nested inside the "Select Skylander File"
    /// dialog — the "file is in use" prompt.
    ShellFileInUse {
        body: String,
    },
    Timeout,
}

/// Poll the slot edit for a value change, the RPCS3 main window for a Qt
/// error modal, and the open file dialog for a nested shell TaskDialog — all
/// in parallel.
fn poll_load_outcome(
    automation: &UIAutomation,
    walker: &UITreeWalker,
    main: &UIElement,
    file_dlg: &UIElement,
    slot_edit: &UIElement,
    timeout: Duration,
) -> Result<LoadOutcome> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let cur = read_value(slot_edit, Duration::ZERO).unwrap_or_default();
        if !cur.is_empty() && cur != "None" {
            return Ok(LoadOutcome::Changed(cur));
        }
        if let Some(body) = find_shell_file_in_use(walker, file_dlg) {
            return Ok(LoadOutcome::ShellFileInUse { body });
        }
        if let Some(modal) = find_error_modal(automation, walker, main) {
            let title = modal.get_name().unwrap_or_default();
            let body = read_modal_body(walker, &modal);
            return Ok(LoadOutcome::QtModal { title, body, modal });
        }
        sleep(POLL_INTERVAL);
    }
    Ok(LoadOutcome::Timeout)
}

fn format_err(title: String, body: String) -> String {
    if body.is_empty() {
        title
    } else {
        format!("{title} — {body}")
    }
}

/// Windows shell's "This file is in use" TaskDialog appears as a nested
/// pane inside the file dialog. Detection keys off the TaskDialog's
/// `ContentText` element — more stable than the classname check which can
/// return empty strings in race windows.
fn find_shell_file_in_use(walker: &UITreeWalker, file_dlg: &UIElement) -> Option<String> {
    // Primary signal: a descendant whose AutomationId is "ContentText" and
    // whose name mentions "in use".
    if let Some(el) = find_descendant(walker, file_dlg, |el| {
        el.get_automation_id()
            .map(|a| a == "ContentText")
            .unwrap_or(false)
    }) {
        let body = el.get_name().unwrap_or_default();
        if body.to_lowercase().contains("in use") {
            debug!(body = %body, "shell TaskDialog detected via ContentText");
            return Some(body);
        }
    }
    // Fallback: look for any descendant Pane with classname "TaskDialog".
    if let Some(pane) = find_descendant(walker, file_dlg, |el| {
        el.get_classname()
            .map(|c| c == "TaskDialog")
            .unwrap_or(false)
    }) {
        debug!("shell TaskDialog detected via classname");
        let body = find_descendant(walker, &pane, |el| {
            el.get_automation_id()
                .map(|a| a == "ContentText")
                .unwrap_or(false)
        })
        .and_then(|el| el.get_name().ok())
        .unwrap_or_else(|| "File is in use.".to_string());
        return Some(body);
    }
    None
}

fn dismiss_shell_task_dialog(walker: &UITreeWalker, file_dlg: &UIElement) {
    // Look for the OK button restricted to the TaskDialog's subtree, not the
    // full file-dialog (otherwise we might grab the file dialog's Open
    // button by name-matching accident).
    let pane_or_dlg = find_descendant(walker, file_dlg, |el| {
        el.get_classname()
            .map(|c| c == "TaskDialog")
            .unwrap_or(false)
    })
    .or_else(|| {
        // No classname match — use the nearest ancestor of the ContentText.
        find_descendant(walker, file_dlg, |el| {
            el.get_automation_id()
                .map(|a| a == "ContentText")
                .unwrap_or(false)
        })
    })
    .unwrap_or_else(|| file_dlg.clone());

    let btn_opt = find_descendant(walker, &pane_or_dlg, |el| {
        el.get_control_type()
            .map(|c| c == ControlType::Button)
            .unwrap_or(false)
            && (el
                .get_automation_id()
                .map(|a| a.starts_with("CommandButton_"))
                .unwrap_or(false)
                || el
                    .get_name()
                    .map(|n| matches!(n.as_str(), "OK" | "Close"))
                    .unwrap_or(false))
    });

    match btn_opt {
        Some(btn) => match btn.get_pattern::<UIInvokePattern>() {
            Ok(inv) => {
                if let Err(e) = inv.invoke() {
                    warn!("failed to invoke TaskDialog OK button: {e}");
                } else {
                    debug!("clicked TaskDialog OK");
                }
            }
            Err(e) => warn!("TaskDialog OK button has no invoke pattern: {e}"),
        },
        None => warn!("TaskDialog OK button not found"),
    }
    sleep(Duration::from_millis(120));
}

fn cancel_file_dialog(walker: &UITreeWalker, file_dlg: &UIElement) {
    // Cancel button inside the standard Win32 file dialog. Match by
    // AutomationId "2" OR name "Cancel". Fall back to any Button whose name
    // is "Cancel" if the AutomationId search returns nothing.
    let btn_opt = find_descendant(walker, file_dlg, |el| {
        el.get_control_type()
            .map(|c| c == ControlType::Button)
            .unwrap_or(false)
            && el.get_automation_id().map(|a| a == "2").unwrap_or(false)
    })
    .or_else(|| {
        find_descendant(walker, file_dlg, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::Button)
                .unwrap_or(false)
                && el.get_name().map(|n| n == "Cancel").unwrap_or(false)
        })
    });

    match btn_opt {
        Some(btn) => match btn.get_pattern::<UIInvokePattern>() {
            Ok(inv) => {
                if let Err(e) = inv.invoke() {
                    warn!("failed to invoke file-dialog Cancel: {e}");
                } else {
                    debug!("clicked file-dialog Cancel");
                }
            }
            Err(e) => warn!("file-dialog Cancel has no invoke pattern: {e}"),
        },
        None => warn!("file-dialog Cancel button not found"),
    }
    // Give the shell enough time to actually close the dialog window so
    // subsequent load jobs don't fight the residual one.
    sleep(Duration::from_millis(200));
}

fn find_error_modal(
    automation: &UIAutomation,
    walker: &UITreeWalker,
    main: &UIElement,
) -> Option<UIElement> {
    // Candidate windows: either nested under main (Qt parented dialogs) or
    // top-level (defensive — some Qt builds float message boxes). We accept
    // anything that's a Window whose name is neither "Skylanders Manager",
    // "Select Skylander File", nor starts with "RPCS3 " — that leaves
    // exactly the QMessageBox.
    if let Some(hit) = find_descendant(walker, main, is_error_modal) {
        return Some(hit);
    }
    let root = automation.get_root_element().ok()?;
    find_descendant(walker, &root, is_error_modal)
}

fn is_error_modal(el: &UIElement) -> bool {
    if el.get_control_type().ok() != Some(ControlType::Window) {
        return false;
    }
    let name = match el.get_name() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if name.is_empty() {
        return false;
    }
    if name.starts_with("RPCS3 ") || name == "Skylanders Manager" || name == "Select Skylander File"
    {
        return false;
    }
    // QMessageBox titles in RPCS3's skylander dialog are all "Failed to …" or
    // "Error …" — require one of those prefixes to avoid false positives from
    // random other Qt dialogs that might appear.
    let lower = name.to_lowercase();
    lower.starts_with("failed") || lower.starts_with("error") || lower.contains("skylander")
}

fn read_modal_body(walker: &UITreeWalker, modal: &UIElement) -> String {
    let mut bits: Vec<String> = Vec::new();
    if let Some(el) = find_descendant(walker, modal, |el| {
        el.get_control_type()
            .map(|c| c == ControlType::Text)
            .unwrap_or(false)
    }) {
        if let Ok(name) = el.get_name() {
            if !name.is_empty() {
                bits.push(name);
            }
        }
    }
    bits.join(" ")
}

fn dismiss_modal(walker: &UITreeWalker, modal: &UIElement) {
    if let Some(btn) = find_descendant(walker, modal, |el| {
        el.get_control_type()
            .map(|c| c == ControlType::Button)
            .unwrap_or(false)
            && el
                .get_name()
                .map(|n| matches!(n.as_str(), "OK" | "Ok" | "Close"))
                .unwrap_or(false)
    }) {
        if let Ok(inv) = btn.get_pattern::<UIInvokePattern>() {
            let _ = inv.invoke();
        }
    }
}

fn find_descendant<F>(walker: &UITreeWalker, root: &UIElement, pred: F) -> Option<UIElement>
where
    F: Fn(&UIElement) -> bool,
{
    fn recurse<F: Fn(&UIElement) -> bool>(
        walker: &UITreeWalker,
        el: &UIElement,
        pred: &F,
        depth: usize,
    ) -> Option<UIElement> {
        if depth > 15 {
            return None;
        }
        if pred(el) {
            return Some(el.clone());
        }
        let mut child = walker.get_first_child(el).ok();
        while let Some(c) = child.clone() {
            if let Some(hit) = recurse(walker, &c, pred, depth + 1) {
                return Some(hit);
            }
            child = walker.get_next_sibling(&c).ok();
        }
        None
    }
    recurse(walker, root, &pred, 0)
}

/// Walk the entire descendant tree (depth-first) and collect every element
/// matching `pred`. Mirrors `find_descendant`'s recursion + 15-deep cap;
/// used by `enumerate_games` to gather all `DataItem`s under the library.
fn collect_descendants<F>(walker: &UITreeWalker, root: &UIElement, pred: F) -> Vec<UIElement>
where
    F: Fn(&UIElement) -> bool,
{
    fn recurse<F: Fn(&UIElement) -> bool>(
        walker: &UITreeWalker,
        el: &UIElement,
        pred: &F,
        depth: usize,
        out: &mut Vec<UIElement>,
    ) {
        if depth > 15 {
            return;
        }
        if pred(el) {
            out.push(el.clone());
        }
        let mut child = walker.get_first_child(el).ok();
        while let Some(c) = child.clone() {
            recurse(walker, &c, pred, depth + 1, out);
            child = walker.get_next_sibling(&c).ok();
        }
    }
    let mut out = Vec::new();
    recurse(walker, root, &pred, 0, &mut out);
    out
}

/// Enumerate top-level windows, return the HWND of the RPCS3 game viewport
/// if one is present (only exists while a game is running).
/// Enumerate top-level VISIBLE `#32770` common-dialog frames (file pickers,
/// color pickers, etc.). RPCS3 may have multiple such dialogs live across
/// a session, so we return every visible one and let the caller filter or
/// move them all. Visibility filter matches what the 3.7.4 sampler uses —
/// otherwise we'd move a stale/hidden #32770 and leave the user-visible
/// one in place.
fn find_visible_file_dialogs() -> Vec<HWND> {
    use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

    struct Ctx {
        hits: Vec<HWND>,
    }
    extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
        }
        let cls = read_class(hwnd).unwrap_or_default();
        if cls == "#32770" {
            ctx.hits.push(hwnd);
        }
        BOOL(1)
    }
    let mut ctx = Ctx { hits: Vec::new() };
    unsafe {
        let lp = LPARAM(&mut ctx as *mut _ as isize);
        let _ = EnumWindows(Some(proc), lp);
    }
    ctx.hits
}

/// Validate that `title` (the FPS-prefixed viewport window title) agrees
/// with `expected_name` (the canonical display name from the game
/// catalogue, e.g. `"Skylanders: Trap Team"`).
///
/// The check is deliberately lenient: exact string equality is fragile
/// against RPCS3's title format drift, and a loose substring match on
/// expected_name's full string can fail for punctuation reasons
/// ("Trap Team" vs "Skylanders Trap Team"). The reliable failure mode
/// we DO want to catch is "boot clicked the wrong library cell and
/// launched a different known Skylanders game" — so we scan the title
/// for any *other* known game's distinguishing keyword and bail if we
/// find one. An unrecognisable title (neither expected nor a known
/// peer) is trusted.
fn verify_viewport_title(title: &str, expected_name: &str) -> Result<()> {
    // One distinguishing keyword per known game. Chosen to be unique
    // against the others (e.g. "Spyro" is fine because no other game
    // has "Spyro" in its title; "Trap Team" works; "SuperChargers" and
    // "Swap Force" likewise).
    const KNOWN_GAME_KEYWORDS: &[&str] = &[
        "Spyro",
        "Giants",
        "SWAP",
        "Trap Team",
        "SuperChargers",
        "Imaginators",
    ];
    let title_lower = title.to_lowercase();
    let expected_lower = expected_name.to_lowercase();

    // Fast-path: title contains the full expected display name. Matches
    // the common case where RPCS3 echoes the library cell's name into
    // the viewport title verbatim.
    if title_lower.contains(&expected_lower) {
        return Ok(());
    }
    // Scan for peer games' keywords. Finding one that ISN'T in our
    // expected name is definitive evidence of a wrong boot.
    for kw in KNOWN_GAME_KEYWORDS {
        let kw_lower = kw.to_lowercase();
        if expected_lower.contains(&kw_lower) {
            // The expected game also owns this keyword — skip; it can
            // only match "correct" above.
            continue;
        }
        if title_lower.contains(&kw_lower) {
            bail!(
                "booted wrong game: requested {expected_name}, \
                 viewport title is {title:?}",
            );
        }
    }
    // No peer keyword AND no expected-name match. If the user's library
    // contains non-Skylanders games (Digimon etc., 2026-04-24) this
    // branch catches them — the request was always for a Skylanders
    // game, so a title with zero Skylanders keywords means we booted
    // the wrong thing regardless of whether we can identify what
    // specifically it is. At least one of our keywords must appear in
    // any correctly-booted Skylanders title.
    let any_keyword = KNOWN_GAME_KEYWORDS
        .iter()
        .any(|kw| title_lower.contains(&kw.to_lowercase()));
    if !any_keyword {
        bail!(
            "booted non-Skylanders game: requested {expected_name}, \
             viewport title is {title:?} (no Skylanders keyword found)",
        );
    }
    // Title contains the expected keyword but not the full display name
    // string — RPCS3 title format variant (punctuation, trademark
    // symbol, etc.). Trust the boot.
    Ok(())
}

/// RAII guard that reverses [`prep_main_offscreen`]'s side effects on
/// drop. Restores the main window's saved rect (so it's back where the
/// user had it before we slung it off-screen) and unminimizes the game
/// viewport if there was one. Always-fire-on-drop semantics let
/// callers `?` through their menu-nav body without leaking the
/// off-screen state on the error path.
struct OffscreenMainGuard {
    main_hwnd: HWND,
    saved_rect: RECT,
    viewport: Option<HWND>,
}

impl Drop for OffscreenMainGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = SetWindowPos(
                self.main_hwnd,
                None,
                self.saved_rect.left,
                self.saved_rect.top,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
            if let Some(vp) = self.viewport {
                let _ = ShowWindow(vp, SW_RESTORE);
            }
        }
    }
}

/// Minimize the game viewport (if any) and slide RPCS3's main window
/// off-screen so subsequent UIA / synthesised-keyboard menu nav can
/// drive it without:
///   - the game viewport stealing focus,
///   - the user seeing the Alt-menu highlight bouncing around the
///     main window,
///   - Qt menu popups landing over the player's view.
///
/// Returns an [`OffscreenMainGuard`] that reverses both moves on drop.
/// Three call-sites used to copy this whole dance inline (the original
/// `RestoreGuard` declared once per function); extracted 2026-04-25.
fn prep_main_offscreen(main_hwnd: HWND) -> OffscreenMainGuard {
    let viewport_hwnd = find_viewport_hwnd();
    let mut saved_rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(main_hwnd, &mut saved_rect);
    }
    if let Some(vp) = viewport_hwnd {
        unsafe {
            let _ = ShowWindow(vp, SW_MINIMIZE);
        }
        sleep(MENU_STEP_PAUSE);
    }
    unsafe {
        let _ = SetWindowPos(
            main_hwnd,
            None,
            OFFSCREEN_POS.0,
            OFFSCREEN_POS.1,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER,
        );
    }
    sleep(MENU_STEP_PAUSE);
    OffscreenMainGuard {
        main_hwnd,
        saved_rect,
        viewport: viewport_hwnd,
    }
}

fn find_viewport_hwnd() -> Option<HWND> {
    struct Ctx {
        hit: Option<HWND>,
    }
    extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        let cls = read_class(hwnd).unwrap_or_default();
        let title = read_title(hwnd).unwrap_or_default();
        // Both the main window and the viewport use class `Qt6110QWindowIcon`
        // in Qt 6.11; the viewport is distinguished by an `FPS:` title prefix.
        if cls == "Qt6110QWindowIcon" && title.starts_with("FPS:") {
            ctx.hit = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut ctx = Ctx { hit: None };
    unsafe {
        let lp = LPARAM(&mut ctx as *mut _ as isize);
        let _ = EnumWindows(Some(proc), lp);
    }
    ctx.hit
}

fn read_title(hwnd: HWND) -> Option<String> {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let read = GetWindowTextW(hwnd, &mut buf);
        if read <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..read as usize]))
    }
}

fn read_class(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        if n <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

/// AttachThreadInput + SetForegroundWindow dance. Windows' foreground-lock
/// rules make this flaky on the first attempt after the user clicks elsewhere;
/// callers should be tolerant of transient failures on re-focus (we re-focus
/// before every keystroke in the nav loop anyway).
fn focus_main_window(hwnd: HWND) -> Result<()> {
    let our_thread = unsafe { GetCurrentThreadId() };
    let fg = unsafe { GetForegroundWindow() };
    if fg.0 == hwnd.0 {
        return Ok(());
    }
    let mut fg_tid = 0u32;
    unsafe {
        let _ = GetWindowThreadProcessId(fg, Some(&mut fg_tid));
    }
    let mut target_pid = 0u32;
    let target_thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut target_pid)) };
    let mut fg_attached = false;
    let mut target_attached = false;
    unsafe {
        if fg_tid != 0 && fg_tid != our_thread {
            fg_attached = AttachThreadInput(our_thread, fg_tid, true).as_bool();
        }
        if target_thread != 0 && target_thread != our_thread {
            target_attached = AttachThreadInput(our_thread, target_thread, true).as_bool();
        }
        let ok = SetForegroundWindow(hwnd).as_bool();
        if fg_attached {
            let _ = AttachThreadInput(our_thread, fg_tid, false);
        }
        if target_attached {
            let _ = AttachThreadInput(our_thread, target_thread, false);
        }
        if !ok {
            bail!("SetForegroundWindow returned false for {hwnd:?}");
        }
    }
    Ok(())
}

fn send_key(vk: VIRTUAL_KEY) -> Result<()> {
    let inputs = [key_input(vk, false), key_input(vk, true)];
    unsafe {
        let n = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if n as usize != inputs.len() {
            bail!("SendInput only dispatched {n}/{} events", inputs.len());
        }
    }
    Ok(())
}

fn key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Post a left mouse click to `hwnd` at window-client-relative `(x, y)` via
/// `PostMessageW`. Bypasses screen Z-order (synthesised input via
/// `SendInput` lands on whichever window is topmost at those screen coords;
/// `PostMessage` routes directly to the target HWND's message queue
/// regardless of covers). Used by `boot_game_by_serial` so the boot flow
/// survives an egui `WS_EX_TOPMOST` shell over RPCS3 — see PLAN 4.15.15
/// and the `zorder_probe` example for the validation.
fn post_click(hwnd: HWND, x: i32, y: i32) -> Result<()> {
    let lparam = pack_xy_lparam(x, y);
    unsafe {
        // wParam = MK_LBUTTON (0x0001) while the button is held; 0 on release.
        PostMessageW(Some(hwnd), WM_LBUTTONDOWN, WPARAM(0x0001), lparam)
            .context("PostMessage WM_LBUTTONDOWN")?;
        sleep(Duration::from_millis(50));
        PostMessageW(Some(hwnd), WM_LBUTTONUP, WPARAM(0), lparam)
            .context("PostMessage WM_LBUTTONUP")?;
    }
    Ok(())
}

/// Post a virtual-key press (down + up) to `hwnd` via `PostMessageW`. Pairs
/// with `post_click` — both bypass Z-order so a topmost egui cover doesn't
/// swallow the input (PLAN 4.15.15).
fn post_key(hwnd: HWND, vk: VIRTUAL_KEY) -> Result<()> {
    let wparam = WPARAM(vk.0 as usize);
    unsafe {
        // WM_KEYDOWN lParam bits: 0-15 repeat count (1), 16-23 scan code,
        // 24 extended, 29 context (Alt=1), 30 previous key state, 31 transition.
        // 0x0001 is "repeat count = 1, no other flags" — Qt accepts that.
        PostMessageW(Some(hwnd), WM_KEYDOWN, wparam, LPARAM(0x0001))
            .context("PostMessage WM_KEYDOWN")?;
        sleep(Duration::from_millis(30));
        // WM_KEYUP canonical lParam: repeat=1, prev-key-state=1 (bit 30),
        // transition=1 (bit 31). 0xC0000001 packs those.
        PostMessageW(
            Some(hwnd),
            WM_KEYUP,
            wparam,
            LPARAM(0xC0000001u32 as i32 as isize),
        )
        .context("PostMessage WM_KEYUP")?;
    }
    Ok(())
}

/// Pack screen-relative or window-relative `(x, y)` into the low/high
/// words of an `LPARAM` for `WM_LBUTTONDOWN/UP`.
fn pack_xy_lparam(x: i32, y: i32) -> LPARAM {
    let xl = (x as u16 as u32) & 0xFFFF;
    let yl = (y as u16 as u32) & 0xFFFF;
    LPARAM((xl | (yl << 16)) as isize)
}

/// After a navigation keystroke, confirm via UIA that the expected menu item
/// has keyboard focus *somewhere* in the tree. Polls for up to
/// `FOCUS_POLL_BUDGET` because UIA's focus-change events can lag a few hundred
/// ms behind the actual keystroke, especially when the owning window is
/// off-screen.
///
/// **Why "somewhere"**: when Qt auto-opens a dropdown during Right-arrow
/// navigation across the menubar, both the menubar item (e.g. "Manage") and
/// the dropdown's first item (e.g. "Virtual File System") report
/// `has_keyboard_focus == true`. A single-match search returns whichever the
/// tree walker encounters first — usually the menubar item, because it's
/// under the main window while the dropdown is in a detached popup. So we
/// collect *all* focused menu items across both main and the desktop root
/// and succeed if any of them matches. If the menu ever gets reordered, the
/// error reports every focused item so diagnostics are obvious.
fn expect_focused_menu_item(
    walker: &UITreeWalker,
    main: &UIElement,
    expected: &str,
    step: &str,
) -> Result<()> {
    const FOCUS_POLL_BUDGET: Duration = Duration::from_millis(1500);
    const FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(50);

    let automation = UIAutomation::new().context("UIA init")?;
    let root = automation.get_root_element().context("UIA root")?;

    let deadline = Instant::now() + FOCUS_POLL_BUDGET;
    let last_seen = loop {
        let mut seen: Vec<String> = Vec::new();
        for search_root in [main, &root] {
            collect_focused_menu_items(walker, search_root, &mut seen);
        }
        if seen.iter().any(|n| n == expected) {
            debug!(step, expected, ?seen, "menu item focused");
            return Ok(());
        }
        if Instant::now() >= deadline {
            break seen;
        }
        sleep(FOCUS_POLL_INTERVAL);
    };
    if last_seen.is_empty() {
        bail!("at step {step:?}: expected {expected:?} focused, no menu item has focus");
    }
    bail!("at step {step:?}: expected {expected:?} focused, focused items: {last_seen:?}")
}

/// Walk `root`, pushing the name of every `MenuItem` with keyboard focus into
/// `out`. Deduplicates by name (Qt may expose the same item under both the
/// menubar tree and a popup window).
fn collect_focused_menu_items(walker: &UITreeWalker, root: &UIElement, out: &mut Vec<String>) {
    let mut stack: Vec<UIElement> = vec![root.clone()];
    while let Some(node) = stack.pop() {
        let is_menu_item = node
            .get_control_type()
            .map(|c| c == ControlType::MenuItem)
            .unwrap_or(false);
        if is_menu_item && node.has_keyboard_focus().unwrap_or(false) {
            let name = node.get_name().unwrap_or_default();
            if !out.contains(&name) {
                out.push(name);
            }
        }
        if let Ok(child) = walker.get_first_child(&node) {
            let mut cur = Some(child);
            while let Some(c) = cur {
                stack.push(c.clone());
                cur = walker.get_next_sibling(&c).ok();
            }
        }
    }
}

/// Strip Qt-style accelerator markers (`&`) and trim tab-separated shortcut
/// hints (e.g. "Exit\tCtrl+Q") so menu items can be matched by their human
/// name regardless of accelerator/shortcut decoration.
fn normalise_menu_name(raw: &str) -> String {
    let without_shortcut = raw.split('\t').next().unwrap_or(raw);
    without_shortcut.replace('&', "").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::verify_viewport_title;

    #[test]
    fn accepts_title_containing_expected_name() {
        let ok = verify_viewport_title(
            "FPS: 60 | Skylanders: Trap Team [BLUS31442]",
            "Skylanders: Trap Team",
        );
        assert!(ok.is_ok(), "verbatim display name match should pass");
    }

    #[test]
    fn accepts_loose_title_with_expected_keyword() {
        // RPCS3 sometimes drops the punctuation — "Skylanders Trap Team"
        // without the colon. That doesn't match the full expected string,
        // but it also doesn't match any OTHER known keyword, so trust it.
        let ok = verify_viewport_title("FPS: 60 | Skylanders Trap Team", "Skylanders: Trap Team");
        assert!(
            ok.is_ok(),
            "title without colon should still be trusted — no peer keyword present",
        );
    }

    #[test]
    fn rejects_title_identifying_different_known_game() {
        // The exact bug Chris hit 2026-04-24: asked for Trap Team, RPCS3
        // booted SuperChargers. Title contains "SuperChargers" keyword,
        // which isn't in the expected name — that's the failure signal.
        let err = verify_viewport_title(
            "FPS: 60 | Skylanders SuperChargers [BLUS31545]",
            "Skylanders: Trap Team",
        );
        assert!(err.is_err(), "peer game keyword should flag wrong boot");
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("wrong game"),
            "error should name the bug: {msg}",
        );
    }

    #[test]
    fn rejects_non_skylanders_title() {
        // Library can contain non-Skylanders games (Chris hit this
        // 2026-04-24 with Digimon as library row 0 — Enter with a stale
        // currentIndex activated Digimon when SuperChargers was asked
        // for). Zero Skylanders keywords in the title must fail the
        // check — we never boot non-Skylanders games, so this is
        // always a wrong-game signal.
        let err = verify_viewport_title(
            "FPS: 60 | 3.20 GHz | Digimon World Re:Digitize [NPUB99999]",
            "Skylanders: SuperChargers",
        );
        assert!(err.is_err(), "non-Skylanders title must bail");
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("non-Skylanders"),
            "error should name the non-Skylanders condition: {msg}",
        );
    }
}
