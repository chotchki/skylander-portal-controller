//! Windows UIA-backed portal driver. Ported from the Phase 1 `tools/uia-drive`
//! spike, with the trait-based API and Win32 off-screen helper added.

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use skylander_core::{SlotIndex, SlotState, SLOT_COUNT};
use tracing::{debug, info, instrument, warn};
use uiautomation::patterns::{UIInvokePattern, UIValuePattern};
use uiautomation::types::{ControlType, UIProperty};
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_DOWN, VK_ESCAPE, VK_MENU, VK_RETURN, VK_RIGHT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, SetForegroundWindow, SetWindowPos, ShowWindow,
    SWP_NOSIZE, SWP_NOZORDER, SW_MINIMIZE, SW_RESTORE,
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
    fn trigger_dialog_via_menu(
        &self,
        walker: &UITreeWalker,
        main: &UIElement,
    ) -> Result<()> {
        let main_hwnd: isize = main
            .get_native_window_handle()
            .context("main window has no native HWND")?
            .into();
        let main_hwnd = HWND(main_hwnd as _);
        let viewport_hwnd = find_viewport_hwnd();
        let mut saved_main_rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(main_hwnd, &mut saved_main_rect);
        }

        // Minimise the viewport so it can't steal focus while we drive the
        // main window's menu bar. No-op if there's no game running.
        if let Some(vp) = viewport_hwnd {
            unsafe {
                let _ = ShowWindow(vp, SW_MINIMIZE);
            }
            sleep(MENU_STEP_PAUSE);
        }

        // Move main off-screen so the Alt-menu highlight isn't visible. The
        // menu popups Qt spawns during navigation will still render at
        // visible screen coords (Qt clamps popups to the screen); the dialog
        // does the same until we hide it below.
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

        // RAII: always restore main's original rect + viewport even on error.
        struct RestoreGuard {
            main_hwnd: HWND,
            rect: RECT,
            viewport: Option<HWND>,
        }
        impl Drop for RestoreGuard {
            fn drop(&mut self) {
                unsafe {
                    let _ = SetWindowPos(
                        self.main_hwnd,
                        None,
                        self.rect.left,
                        self.rect.top,
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
        let _guard = RestoreGuard {
            main_hwnd,
            rect: saved_main_rect,
            viewport: viewport_hwnd,
        };

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
        expect_focused_menu_item(walker, main, "Skylanders Portal", "expand Portals and Gates")?;

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
    /// library grid is visible. Uses the recipe proven in
    /// `examples/boot_game.rs`: find the `DataItem` whose name equals
    /// `serial` under the main window, call `SelectionItemPattern::select()`,
    /// set keyboard focus on the cell, then send `Enter`. UIA `Invoke` alone
    /// does *not* boot the game — the selection + focus + keystroke combo is
    /// required.
    ///
    /// Succeeds when a viewport window (`Qt6110QWindowIcon` class, title
    /// prefix `"FPS:"`) appears within `timeout`.
    pub fn boot_game_by_serial(&self, serial: &str, timeout: Duration) -> Result<()> {
        use uiautomation::patterns::UISelectionItemPattern;

        let walker = self.walker()?;
        let main = self.main_window(&walker)?;
        let main_hwnd: isize = main
            .get_native_window_handle()
            .context("main HWND")?
            .into();
        let main_hwnd = HWND(main_hwnd as _);

        let cell = find_descendant(&walker, &main, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::DataItem)
                .unwrap_or(false)
                && el.get_name().map(|n| n == serial).unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("no DataItem named {serial} in RPCS3 library"))?;
        debug!(serial, "found library cell");

        // Bring main forward so focus/keystroke calls target it.
        let _ = focus_main_window(main_hwnd);
        sleep(MENU_STEP_PAUSE);

        let sel = cell
            .get_pattern::<UISelectionItemPattern>()
            .context("SelectionItemPattern on library cell")?;
        sel.select().context("select library cell")?;
        cell.set_focus().context("focus library cell")?;
        sleep(MENU_STEP_PAUSE);
        send_key(VK_RETURN).context("send Enter to boot")?;

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if find_viewport_hwnd().is_some() {
                info!(serial, "game viewport detected — boot succeeded");
                return Ok(());
            }
            sleep(POLL_INTERVAL);
        }
        bail!("game viewport didn't appear within {timeout:?} after boot attempt");
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
        let main_hwnd: isize = main
            .get_native_window_handle()
            .context("main HWND")?
            .into();
        let main_hwnd = HWND(main_hwnd as _);
        let viewport_hwnd = find_viewport_hwnd();
        let mut saved_main_rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(main_hwnd, &mut saved_main_rect);
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

        struct RestoreGuard {
            main_hwnd: HWND,
            rect: RECT,
            viewport: Option<HWND>,
        }
        impl Drop for RestoreGuard {
            fn drop(&mut self) {
                unsafe {
                    let _ = SetWindowPos(
                        self.main_hwnd,
                        None,
                        self.rect.left,
                        self.rect.top,
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
        let _guard = RestoreGuard {
            main_hwnd,
            rect: saved_main_rect,
            viewport: viewport_hwnd,
        };

        focus_main_window(main_hwnd).context("focus main window")?;
        sleep(MENU_STEP_PAUSE);

        // Alt tap: File is the leftmost menu, so it takes focus immediately.
        send_key(VK_MENU)?;
        sleep(MENU_STEP_PAUSE);
        expect_focused_menu_item(&walker, &main, "File", "Alt tap")?;

        // Down opens the File submenu and focuses its first item. We don't
        // know that item's name ahead of time (varies by RPCS3 build), so
        // just step Down until a menu item whose name normalises to "exit"
        // is focused, capped at MAX_STEPS to avoid running forever if the
        // menu ever changes.
        const MAX_STEPS: u32 = 20;
        focus_main_window(main_hwnd).ok();
        send_key(VK_DOWN)?;
        sleep(MENU_STEP_PAUSE);

        let mut steps = 0;
        loop {
            let focused_name = current_focused_menu_name(&walker, &main);
            let on_exit = focused_name
                .as_deref()
                .map(|n| normalise_menu_name(n).eq_ignore_ascii_case("exit"))
                .unwrap_or(false);
            if on_exit {
                debug!(steps, "File → Exit focused");
                break;
            }
            if steps >= MAX_STEPS {
                // Dismiss so we don't leave RPCS3 in menu mode.
                let _ = send_key(VK_ESCAPE);
                let _ = send_key(VK_ESCAPE);
                bail!(
                    "walked {MAX_STEPS} File-menu items without finding 'Exit' \
                     (last focused: {:?})",
                    focused_name
                );
            }
            focus_main_window(main_hwnd).ok();
            send_key(VK_DOWN)?;
            sleep(MENU_STEP_PAUSE);
            steps += 1;
        }

        send_key(VK_RETURN)?;
        info!("sent Enter on File → Exit");

        // RPCS3 sometimes pops a "confirm quit" dialog when a game is
        // running. A second Enter after a brief settle lands on the default
        // (Yes) button. If there's no dialog it's harmlessly ignored.
        sleep(MENU_STEP_PAUSE);
        let _ = send_key(VK_RETURN);
        Ok(())
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
        for i in 0..SLOT_COUNT {
            let slot = SlotIndex::new(i as u8).unwrap();
            let (edit, _, _, _) = self.find_row(&walker, &group, slot)?;
            let value = read_value(&edit, READ_VALUE_TIMEOUT)?;
            out[i] = interpret_slot_value(&value);
        }
        Ok(out)
    }

    #[instrument(skip(self), fields(slot = %slot, path = %path.display()))]
    fn load(&self, slot: SlotIndex, path: &Path) -> Result<String> {
        let abs = std::fs::canonicalize(path)
            .with_context(|| format!("cannot canonicalize {}", path.display()))?;
        let abs_str = abs.to_string_lossy().trim_start_matches(r"\\?\").to_string();

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
        let file_dlg =
            self.wait_for_child_window(&walker, "Select Skylander File", DIALOG_OPEN_TIMEOUT)?;
        let file_edit = find_descendant(&walker, &file_dlg, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::Edit)
                .unwrap_or(false)
                && el
                    .get_automation_id()
                    .map(|a| a == "1148")
                    .unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("file-name edit not found"))?;
        let open_btn = find_descendant(&walker, &file_dlg, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::Button)
                .unwrap_or(false)
                && el.get_automation_id().map(|a| a == "1").unwrap_or(false)
                && el.get_name().map(|n| n == "Open").unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("Open button not found"))?;

        file_edit.get_pattern::<UIValuePattern>()?.set_value(&abs_str)?;
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
    Err(anyhow!("value didn't become '{expected}' within {timeout:?}"))
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
        Some(btn) => {
            match btn.get_pattern::<UIInvokePattern>() {
                Ok(inv) => {
                    if let Err(e) = inv.invoke() {
                        warn!("failed to invoke TaskDialog OK button: {e}");
                    } else {
                        debug!("clicked TaskDialog OK");
                    }
                }
                Err(e) => warn!("TaskDialog OK button has no invoke pattern: {e}"),
            }
        }
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
    if let Some(hit) = find_descendant(walker, main, |el| is_error_modal(el)) {
        return Some(hit);
    }
    let root = automation.get_root_element().ok()?;
    find_descendant(walker, &root, |el| is_error_modal(el))
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
    if name.starts_with("RPCS3 ")
        || name == "Skylanders Manager"
        || name == "Select Skylander File"
    {
        return false;
    }
    // QMessageBox titles in RPCS3's skylander dialog are all "Failed to …" or
    // "Error …" — require one of those prefixes to avoid false positives from
    // random other Qt dialogs that might appear.
    let lower = name.to_lowercase();
    lower.starts_with("failed")
        || lower.starts_with("error")
        || lower.contains("skylander")
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

/// Enumerate top-level windows, return the HWND of the RPCS3 game viewport
/// if one is present (only exists while a game is running).
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
    let target_thread =
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut target_pid)) };
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

/// After a navigation keystroke, confirm via UIA that the expected menu item
/// has keyboard focus. If the menu ever gets reordered we'll get a clear
/// error here naming both what we expected and what was actually focused.
fn expect_focused_menu_item(
    walker: &UITreeWalker,
    main: &UIElement,
    expected: &str,
    step: &str,
) -> Result<()> {
    // Qt moves submenu items out from under the main window once they're
    // expanded, so search desktop-wide.
    let automation = UIAutomation::new().context("UIA init")?;
    let root = automation.get_root_element().context("UIA root")?;
    for search_root in [main, &root] {
        if let Some(hit) = find_descendant(walker, search_root, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::MenuItem)
                .unwrap_or(false)
                && el.has_keyboard_focus().unwrap_or(false)
        }) {
            let name = hit.get_name().unwrap_or_default();
            if name == expected {
                debug!(step, expected, "menu item focused");
                return Ok(());
            }
            bail!(
                "at step {step:?}: expected {expected:?} focused, got {name:?}"
            );
        }
    }
    bail!("at step {step:?}: expected {expected:?} focused, no menu item has focus")
}

/// Find the currently-focused menu item (if any) under main or desktop-root
/// and return its raw UIA name. Used by `quit_via_file_menu` to walk the File
/// submenu by name without knowing its layout ahead of time.
fn current_focused_menu_name(walker: &UITreeWalker, main: &UIElement) -> Option<String> {
    let automation = UIAutomation::new().ok()?;
    let root = automation.get_root_element().ok()?;
    for search_root in [main, &root] {
        if let Some(hit) = find_descendant(walker, search_root, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::MenuItem)
                .unwrap_or(false)
                && el.has_keyboard_focus().unwrap_or(false)
        }) {
            return hit.get_name().ok();
        }
    }
    None
}

/// Strip Qt-style accelerator markers (`&`) and trim tab-separated shortcut
/// hints (e.g. "Exit\tCtrl+Q") so menu items can be matched by their human
/// name regardless of accelerator/shortcut decoration.
fn normalise_menu_name(raw: &str) -> String {
    let without_shortcut = raw.split('\t').next().unwrap_or(raw);
    without_shortcut.replace('&', "").trim().to_string()
}
