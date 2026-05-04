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
use uiautomation::patterns::{UIExpandCollapsePattern, UIInvokePattern, UIValuePattern};
use uiautomation::types::{ControlType, UIProperty};
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, SWP_NOSIZE,
    SWP_NOZORDER, SetWindowPos,
};
use windows::core::BOOL;

const READ_VALUE_TIMEOUT: Duration = Duration::from_secs(5);
const LOAD_TIMEOUT: Duration = Duration::from_secs(10);
const CLEAR_TIMEOUT: Duration = Duration::from_secs(3);
const DIALOG_OPEN_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(30);
const OFFSCREEN_POS: (i32, i32) = (-4000, -4000);

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
        // Current RPCS3 (Qt 6.11) gives every Qt window the same generic
        // class `Qt6110QWindowIcon` — main, dialog, and game viewport are
        // only distinguishable by title. Use the dialog's exact title.
        // (Older RPCS3 builds had `classname = "skylander_dialog"`; that
        // detection broke silently when Qt unified its class names.)
        find_descendant(walker, main, |el| {
            el.get_name()
                .map(|n| n == "Skylanders Manager")
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

    /// Open the Skylanders Manager dialog by driving the Manage menu via
    /// pure UIA pattern calls — `ExpandCollapse.expand()` on each parent
    /// MenuItem, then `Invoke.invoke()` on the leaf.
    ///
    /// History (PLAN 10.8.4): older RPCS3 + Qt 6 builds didn't honour
    /// UIA `Invoke` / `ExpandCollapse` on menu items, so we drove the
    /// menu with synthesised Alt + arrow keys. That worked but failed
    /// often in the field — game viewport, RPCS3 update popup, and Steam
    /// Overlay all competed for keyboard focus mid-navigation. Current
    /// RPCS3 (Qt ~6.11) exposes both patterns natively (verified via
    /// `tools/uia-probe`), so we drop the keystroke pipeline entirely.
    /// Pattern calls don't depend on focus, so the dialog opens reliably
    /// even when RPCS3 was launched directly with an EBOOT.BIN argument
    /// and the game viewport is fullscreen.
    ///
    /// Runs once per RPCS3 session; subsequent `open_dialog` calls hit
    /// the short-circuit in the caller. Once opened the dialog is slung
    /// off-screen via Win32 SetWindowPos so the user doesn't see it.
    fn trigger_dialog_via_menu(&self, walker: &UITreeWalker, main: &UIElement) -> Result<()> {
        let manage = find_menu_item_by_name(walker, main, "Manage")
            .ok_or_else(|| anyhow!("'Manage' menu item not found"))?;
        manage
            .get_pattern::<UIExpandCollapsePattern>()
            .context("Manage MenuItem doesn't expose ExpandCollapsePattern")?
            .expand()
            .context("ExpandCollapse.expand() on 'Manage' failed")?;
        // Qt populates submenu children asynchronously on the GUI thread
        // after expand. The 100ms beat lets the children land in the UIA
        // tree before we walk for "Portals and Gates".
        sleep(Duration::from_millis(100));

        let portals = find_menu_item_by_name(walker, main, "Portals and Gates")
            .ok_or_else(|| anyhow!("'Portals and Gates' menu item not found"))?;
        portals
            .get_pattern::<UIExpandCollapsePattern>()
            .context("'Portals and Gates' doesn't expose ExpandCollapsePattern")?
            .expand()
            .context("ExpandCollapse.expand() on 'Portals and Gates' failed")?;
        sleep(Duration::from_millis(100));

        let leaf = find_menu_item_by_name(walker, main, "Skylanders Portal")
            .ok_or_else(|| anyhow!("'Skylanders Portal' menu item not found"))?;
        leaf.get_pattern::<UIInvokePattern>()
            .context("'Skylanders Portal' doesn't expose InvokePattern")?
            .invoke()
            .context("Invoke.invoke() on 'Skylanders Portal' failed")?;

        // Dialog window appears asynchronously after invoke. Poll for it
        // and sling it off-screen as soon as it shows so the user doesn't
        // see a flash on top of the game viewport.
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
                info!("Skylanders Manager dialog opened via UIA + moved off-screen");
                return Ok(());
            }
            sleep(POLL_INTERVAL);
        }
        Err(anyhow!(
            "Skylanders Manager dialog didn't appear within {DIALOG_OPEN_TIMEOUT:?} \
             after Invoke (UIA call returned Ok but Qt action may have been suppressed)"
        ))
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

        // Same UIA pattern path as `trigger_dialog_via_menu` (PLAN 10.8.4).
        // ExpandCollapse on File populates the submenu, then Invoke on
        // "Exit" fires. No focus dependency, no key synthesis.
        let file = find_menu_item_by_name(&walker, &main, "File")
            .ok_or_else(|| anyhow!("'File' menu item not found"))?;
        file.get_pattern::<UIExpandCollapsePattern>()
            .context("'File' has no ExpandCollapsePattern")?
            .expand()
            .context("ExpandCollapse.expand() on 'File' failed")?;
        sleep(Duration::from_millis(100));

        let exit = find_menu_item_by_name(&walker, &main, "Exit")
            .ok_or_else(|| anyhow!("'Exit' menu item not found under File"))?;
        exit.get_pattern::<UIInvokePattern>()
            .context("'Exit' has no InvokePattern")?
            .invoke()
            .context("Invoke.invoke() on 'Exit' failed")?;
        info!("invoked File → Exit via UIA");

        // RPCS3 may pop a "confirm quit" dialog when a game is running.
        // The default focused button is "Yes", which has Invoke. Look for
        // it briefly; harmless no-op if no dialog appeared.
        sleep(Duration::from_millis(200));
        if let Ok(root) = self.automation.get_root_element() {
            // The confirmation is a top-level dialog; walk one level deep
            // to find a Yes button, click it.
            if let Some(yes) = find_descendant(&walker, &root, |el| {
                matches!(el.get_control_type().ok(), Some(ControlType::Button))
                    && el
                        .get_name()
                        .map(|n| n.eq_ignore_ascii_case("yes") || n == "&Yes")
                        .unwrap_or(false)
            }) && let Ok(inv) = yes.get_pattern::<UIInvokePattern>()
            {
                let _ = inv.invoke();
                debug!("invoked confirmation dialog Yes");
            }
        }
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

/// Walk the UIA subtree and return the first MenuItem (or Menu) whose Name
/// equals `name`. Used by the menu-pattern nav in `trigger_dialog_via_menu`.
/// Submenus populate lazily on `ExpandCollapse.expand()`, so call this
/// AFTER expanding the parent.
fn find_menu_item_by_name(
    walker: &UITreeWalker,
    root: &UIElement,
    name: &str,
) -> Option<UIElement> {
    find_descendant(walker, root, |el| {
        matches!(
            el.get_control_type().ok(),
            Some(ControlType::MenuItem) | Some(ControlType::Menu)
        ) && el.get_name().map(|n| n == name).unwrap_or(false)
    })
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

// The viewport-title verifier (`verify_viewport_title`) and its tests
// were retired alongside the library-view boot path (PLAN 10.8.4).
// Direct-boot via EBOOT.BIN means the launched game is whichever serial
// the user picked in the phone picker — there's no library-cell click
// to land on the wrong row, so the wrong-game-detection logic those
// tests covered is no longer applicable.
