//! 3.6b (continued) — probe the RPCS3 game library UIA structure to learn
//! how to find a game by serial and boot it programmatically.
//!
//!   cargo run -p skylander-rpcs3-control --example library_probe
//!
//! Prereq: RPCS3 is running with NO game booted (just the library view).
//! Dumps candidate elements that look like the game list — tables, lists,
//! data items with serials like "BLUS30968" in their names.

#![cfg(windows)]

use anyhow::{anyhow, Context, Result};
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

const SKYLANDERS_SERIALS: &[(&str, &str)] = &[
    ("BLUS30906", "Spyro's Adventure"),
    ("BLUS30968", "Giants"),
    ("BLUS31076", "SWAP Force"),
    ("BLUS31442", "Trap Team"),
    ("BLUS31545", "SuperChargers"),
    ("BLUS31600", "Imaginators"),
];

fn main() -> Result<()> {
    let automation = UIAutomation::new().context("UIA init")?;
    let walker = automation.create_tree_walker().context("walker")?;
    let main = find_main_window(&automation, &walker)?;
    eprintln!(
        "main: {:?} class={:?}",
        main.get_name().ok(),
        main.get_classname().ok()
    );

    // 1. Dump any Table / DataGrid / List controls, and their child rows.
    eprintln!("\n--- Table / List / DataGrid controls under main ---");
    walk(&walker, &main, 0, &mut |el, depth| {
        let ct = el.get_control_type().ok();
        if matches!(
            ct,
            Some(ControlType::Table)
                | Some(ControlType::DataGrid)
                | Some(ControlType::List)
                | Some(ControlType::DataItem)
                | Some(ControlType::TreeItem)
                | Some(ControlType::ListItem)
        ) {
            eprintln!(
                "{:indent$}[{:?}] name={:?} class={:?}",
                "",
                ct.unwrap(),
                el.get_name().unwrap_or_default(),
                el.get_classname().unwrap_or_default(),
                indent = depth * 2
            );
        }
    });

    // 2. Find any element whose name contains a Skylanders serial.
    eprintln!("\n--- Elements containing a Skylanders serial ---");
    for (serial, short) in SKYLANDERS_SERIALS {
        let mut hits = 0;
        walk(&walker, &main, 0, &mut |el, _| {
            let name = el.get_name().unwrap_or_default();
            if name.contains(serial) {
                hits += 1;
                eprintln!(
                    "  {short} ({serial}): [{:?}] name={name:?} class={:?}",
                    el.get_control_type().ok(),
                    el.get_classname().unwrap_or_default()
                );
            }
        });
        if hits == 0 {
            eprintln!("  {short} ({serial}): (not found)");
        }
    }

    Ok(())
}

fn walk<F: FnMut(&UIElement, usize)>(
    walker: &UITreeWalker,
    root: &UIElement,
    depth: usize,
    visit: &mut F,
) {
    let mut stack: Vec<(UIElement, usize)> = vec![(root.clone(), depth)];
    while let Some((node, d)) = stack.pop() {
        visit(&node, d);
        if d > 20 {
            continue;
        }
        if let Ok(child) = walker.get_first_child(&node) {
            let mut cur = Some(child);
            while let Some(c) = cur {
                stack.push((c.clone(), d + 1));
                cur = walker.get_next_sibling(&c).ok();
            }
        }
    }
}

fn find_main_window(automation: &UIAutomation, walker: &UITreeWalker) -> Result<UIElement> {
    let root = automation.get_root_element()?;
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
    Err(anyhow!("RPCS3 main window not found"))
}
