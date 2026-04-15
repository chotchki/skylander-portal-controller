//! Windows UIA-backed portal driver. Ported from the Phase 1 `tools/uia-drive`
//! spike, with the trait-based API and Win32 off-screen helper added.

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use skylander_core::{SlotIndex, SlotState, SLOT_COUNT};
use tracing::{debug, info, instrument, warn};
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIInvokePattern, UIValuePattern,
};
use uiautomation::types::{ControlType, UIProperty};
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

const READ_VALUE_TIMEOUT: Duration = Duration::from_secs(5);
const LOAD_TIMEOUT: Duration = Duration::from_secs(10);
const CLEAR_TIMEOUT: Duration = Duration::from_secs(3);
const DIALOG_OPEN_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(30);

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

    fn trigger_dialog_via_menu(&self, walker: &UITreeWalker, main: &UIElement) -> Result<()> {
        let menu_item = find_descendant(walker, main, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::MenuItem)
                .unwrap_or(false)
                && el.get_name().map(|n| n == "Manage").unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("'Manage' menu item not found"))?;

        // Expand the menu.
        if let Ok(ec) = menu_item.get_pattern::<UIExpandCollapsePattern>() {
            let _ = ec.expand();
        } else if let Ok(inv) = menu_item.get_pattern::<UIInvokePattern>() {
            inv.invoke().context("invoke Manage menu")?;
        }

        // Find and invoke "Manage Skylanders Portal". The submenu may be
        // rooted at the desktop during expansion; search the whole root.
        let deadline = Instant::now() + DIALOG_OPEN_TIMEOUT;
        while Instant::now() < deadline {
            let root = self.automation.get_root_element()?;
            if let Some(sub) = find_descendant(walker, &root, |el| {
                el.get_control_type()
                    .map(|c| c == ControlType::MenuItem)
                    .unwrap_or(false)
                    && el
                        .get_name()
                        .map(|n| n == "Manage Skylanders Portal")
                        .unwrap_or(false)
            }) {
                sub.get_pattern::<UIInvokePattern>()?
                    .invoke()
                    .context("invoke Manage Skylanders Portal")?;
                return Ok(());
            }
            sleep(POLL_INTERVAL);
        }
        Err(anyhow!("Manage Skylanders Portal submenu didn't appear"))
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
    #[cfg(windows)]
    pub fn hide_dialog_offscreen(&self) -> Result<()> {
        let walker = self.walker()?;
        let main = self.main_window(&walker)?;
        let dialog = self
            .find_dialog(&walker, &main)
            .ok_or_else(|| anyhow!("dialog not open"))?;
        crate::hide::set_position(&dialog, -4000, -4000)?;
        info!("dialog moved off-screen");
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
        info!("opening dialog via Manage menu");
        self.trigger_dialog_via_menu(&walker, &main)?;

        // Poll for the dialog to appear.
        let deadline = Instant::now() + DIALOG_OPEN_TIMEOUT;
        while Instant::now() < deadline {
            if self.find_dialog(&walker, &main).is_some() {
                return Ok(());
            }
            sleep(POLL_INTERVAL);
        }
        Err(anyhow!("Skylanders Manager dialog didn't appear"))
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

        let after = poll_until_changes(&edit, "None", LOAD_TIMEOUT)
            .context("slot value didn't change after Open")?;

        if let Some(err) = find_error_modal(&self.automation, &walker) {
            // Dismiss and bubble up.
            if let Some(ok) = find_descendant(&walker, &err, |el| {
                el.get_control_type()
                    .map(|c| c == ControlType::Button)
                    .unwrap_or(false)
                    && el.get_name().map(|n| n == "OK").unwrap_or(false)
            }) {
                let _ = ok.get_pattern::<UIInvokePattern>().and_then(|p| p.invoke());
            }
            let msg = err.get_name().unwrap_or_else(|_| "RPCS3 error".into());
            bail!("RPCS3 reported: {msg}");
        }

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
        SlotState::Loaded {
            figure_id: None,
            display_name: value.to_string(),
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

fn poll_until_changes(el: &UIElement, old: &str, timeout: Duration) -> Result<String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let cur = read_value(el, Duration::ZERO).unwrap_or_default();
        if cur != old {
            return Ok(cur);
        }
        sleep(POLL_INTERVAL);
    }
    Err(anyhow!("value stayed '{old}' for {timeout:?}"))
}

fn find_error_modal(automation: &UIAutomation, walker: &UITreeWalker) -> Option<UIElement> {
    let root = automation.get_root_element().ok()?;
    find_descendant(walker, &root, |el| {
        el.get_control_type()
            .map(|c| c == ControlType::Window)
            .unwrap_or(false)
            && el
                .get_classname()
                .map(|c| c.starts_with("QMessageBox") || c == "Qt651QWindowIcon" /* best-effort */)
                .unwrap_or(false)
            && el
                .get_name()
                .map(|n| !n.contains("Skylanders Manager") && !n.starts_with("RPCS3"))
                .unwrap_or(false)
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
