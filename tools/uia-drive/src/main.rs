//! Phase 1 spike 1a — drive the Skylanders Manager dialog via UIA.
//!
//! End-to-end test of: find dialog → find row N → read current slot value →
//! click Clear → click Load → wait for file dialog → type absolute path →
//! click Open → poll until row's edit value changes. Prints per-step timings.
//!
//! Usage:
//!   uia-drive <slot 1-8> <absolute .sky path>      — normal load
//!   uia-drive --offscreen                          — move the dialog to -4000,-4000 and re-probe
//!   uia-drive --onscreen                           — move the dialog back to 400,400
//!
//! Run while the Skylanders Manager dialog is open in RPCS3.

use std::env;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use uiautomation::{UIAutomation, UIElement, UITreeWalker};
use uiautomation::patterns::{UIInvokePattern, UITransformPattern, UIValuePattern};
use uiautomation::types::{ControlType, UIProperty};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let automation = UIAutomation::new()?;

    match args.first().map(String::as_str) {
        Some("--offscreen") => move_dialog(&automation, -4000.0, -4000.0),
        Some("--onscreen") => move_dialog(&automation, 400.0, 400.0),
        Some(slot_s) => {
            let slot: usize = slot_s.parse().context("slot must be 1..=8")?;
            let path = args.get(1).context("missing .sky path")?;
            drive_load(&automation, slot, path)
        }
        None => bail!("usage: uia-drive <slot 1-8> <.sky path>  |  --offscreen  |  --onscreen"),
    }
}

fn drive_load(automation: &UIAutomation, slot: usize, sky_path: &str) -> Result<()> {
    if !(1..=8).contains(&slot) {
        bail!("slot must be 1..=8");
    }
    let abs = std::fs::canonicalize(sky_path)
        .with_context(|| format!("cannot canonicalize {sky_path}"))?;
    let abs_str = abs.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    println!("using absolute path: {abs_str}");

    let walker = automation.create_tree_walker()?;
    let dialog = find_dialog(automation, &walker)?;
    let group = find_group_box(&walker, &dialog)?;

    let t_total = Instant::now();

    // Find the target row by its "Skylander N" label, then collect the next
    // four siblings as edit, clear, create, load.
    let label_name = format!("Skylander {slot}");
    let (edit, clear_btn, _create_btn, load_btn) = find_row(&walker, &group, &label_name)?;

    let before = value_of(&edit).unwrap_or_default();
    println!("slot {slot} current value: {before:?}");

    if !before.is_empty() && before != "None" {
        println!("clearing first…");
        invoke(&clear_btn)?;
        wait_for_value(&edit, "None", Duration::from_secs(3))?;
    }

    // Click Load. File dialog appears as a nested window under main RPCS3.
    let t_load = Instant::now();
    invoke(&load_btn)?;
    let file_dlg = wait_for_child(automation, &walker, "Select Skylander File", Duration::from_secs(3))?;
    println!("file dialog appeared in {:?}", t_load.elapsed());

    // Locate the file-name edit (AutomationId 1148) and Open button (AutomationId 1).
    let file_edit = find_descendant(&walker, &file_dlg, |el| {
        el.get_control_type().map(|c| c == ControlType::Edit).unwrap_or(false)
            && el.get_automation_id().map(|a| a == "1148").unwrap_or(false)
    })
    .ok_or_else(|| anyhow!("no file-name edit"))?;
    let open_btn = find_descendant(&walker, &file_dlg, |el| {
        el.get_control_type().map(|c| c == ControlType::Button).unwrap_or(false)
            && el.get_automation_id().map(|a| a == "1").unwrap_or(false)
            && el.get_name().map(|n| n == "Open").unwrap_or(false)
    })
    .ok_or_else(|| anyhow!("no Open button"))?;

    let t_set = Instant::now();
    let value = file_edit.get_pattern::<UIValuePattern>()?;
    value.set_value(&abs_str)?;
    println!("path set in {:?}", t_set.elapsed());

    let t_open = Instant::now();
    invoke(&open_btn)?;
    let new_value = poll_until_changes(&edit, &before, Duration::from_secs(10))?;
    println!("slot {slot} value changed to {new_value:?} in {:?}", t_open.elapsed());
    println!("TOTAL elapsed: {:?}", t_total.elapsed());
    Ok(())
}

fn move_dialog(automation: &UIAutomation, x: f64, y: f64) -> Result<()> {
    let walker = automation.create_tree_walker()?;
    let dialog = find_dialog(automation, &walker)?;
    let tf = dialog.get_pattern::<UITransformPattern>()?;
    tf.move_to(x, y)?;
    println!("moved dialog to ({x}, {y})");

    let rect = dialog.get_bounding_rectangle()?;
    println!(
        "new bounding rect: {}x{}@{},{}",
        rect.get_width(),
        rect.get_height(),
        rect.get_left(),
        rect.get_top()
    );

    // Re-probe: walk the group to confirm children are still visible/addressable.
    let group = find_group_box(&walker, &dialog)?;
    let row = find_row(&walker, &group, "Skylander 1");
    match row {
        Ok((edit, _, _, _)) => {
            println!(
                "re-probe OK: slot 1 current value = {:?}",
                value_of(&edit).unwrap_or_default()
            );
        }
        Err(e) => println!("re-probe FAILED: {e}"),
    }
    Ok(())
}

fn find_dialog(automation: &UIAutomation, walker: &UITreeWalker) -> Result<UIElement> {
    let root = automation.get_root_element()?;
    // Find RPCS3 main window.
    let main = {
        let mut cur = walker.get_first_child(&root).ok();
        loop {
            match cur {
                None => bail!("RPCS3 main window not found"),
                Some(el) => {
                    let name = el.get_name().unwrap_or_default();
                    if name.starts_with("RPCS3 ") {
                        break el;
                    }
                    cur = walker.get_next_sibling(&el).ok();
                }
            }
        }
    };
    // Skylanders Manager is a child window of the main window.
    find_descendant(walker, &main, |el| {
        el.get_classname().map(|c| c == "skylander_dialog").unwrap_or(false)
    })
    .ok_or_else(|| anyhow!("Skylanders Manager dialog not found (open it via Manage menu)"))
}

fn find_group_box(walker: &UITreeWalker, dialog: &UIElement) -> Result<UIElement> {
    find_descendant(walker, dialog, |el| {
        el.get_control_type().map(|c| c == ControlType::Group).unwrap_or(false)
            && el.get_classname().map(|c| c == "QGroupBox").unwrap_or(false)
    })
    .ok_or_else(|| anyhow!("Active Portal Skylanders group not found"))
}

fn find_row(
    walker: &UITreeWalker,
    group: &UIElement,
    label_name: &str,
) -> Result<(UIElement, UIElement, UIElement, UIElement)> {
    // Walk children of the group in order; when we hit the label, next four
    // relevant siblings are edit, clear, create, load (skipping QFrame).
    let mut cur = walker.get_first_child(group).ok();
    while let Some(el) = cur.clone() {
        let name = el.get_name().unwrap_or_default();
        let ct = el.get_control_type().ok();
        if name == label_name && ct == Some(ControlType::Text) {
            let edit = walker.get_next_sibling(&el)?;
            let clear = walker.get_next_sibling(&edit)?;
            let create = walker.get_next_sibling(&clear)?;
            let load = walker.get_next_sibling(&create)?;
            return Ok((edit, clear, create, load));
        }
        cur = walker.get_next_sibling(&el).ok();
    }
    Err(anyhow!("row '{label_name}' not found"))
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

fn invoke(el: &UIElement) -> Result<()> {
    let pat = el.get_pattern::<UIInvokePattern>()?;
    pat.invoke()?;
    Ok(())
}

fn value_of(el: &UIElement) -> Result<String> {
    // ValuePattern Value is the actual text; UIA's Name is the labeledBy name.
    let variant = el.get_property_value(UIProperty::ValueValue)?;
    Ok(variant.get_string().unwrap_or_default())
}

fn wait_for_child(
    automation: &UIAutomation,
    walker: &UITreeWalker,
    title: &str,
    timeout: Duration,
) -> Result<UIElement> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(root) = automation.get_root_element() {
            if let Some(hit) = find_descendant(walker, &root, |el| {
                el.get_name().map(|n| n.contains(title)).unwrap_or(false)
                    && el.get_control_type().map(|c| c == ControlType::Window).unwrap_or(false)
            }) {
                return Ok(hit);
            }
        }
        sleep(Duration::from_millis(50));
    }
    Err(anyhow!("child window '{title}' didn't appear within {timeout:?}"))
}

fn wait_for_value(el: &UIElement, expected: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if value_of(el).unwrap_or_default() == expected {
            return Ok(());
        }
        sleep(Duration::from_millis(30));
    }
    Err(anyhow!("value didn't become {expected:?} within {timeout:?}"))
}

fn poll_until_changes(el: &UIElement, old: &str, timeout: Duration) -> Result<String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let cur = value_of(el).unwrap_or_default();
        if cur != old {
            return Ok(cur);
        }
        sleep(Duration::from_millis(30));
    }
    Err(anyhow!("value stayed {old:?} for {timeout:?}"))
}

