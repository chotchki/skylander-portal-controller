//! 3.6b — dump every MenuItem UIA can see under the RPCS3 main window right
//! now, plus every top-level MenuItem/Window under the desktop root.
//!
//!   cargo run -p skylander-rpcs3-control --example dump_menu_tree
//!
//! Useful to answer: does the "Manage Skylanders Portal" submenu item exist
//! in the UIA tree at all without invoking the Manage menu first?

#![cfg(windows)]

use anyhow::{Context, Result, anyhow};
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

fn main() -> Result<()> {
    let automation = UIAutomation::new().context("UIA init")?;
    let walker = automation.create_tree_walker().context("walker")?;

    let root = automation.get_root_element()?;
    let main = find_main_window(&automation, &walker)?;
    eprintln!(
        "Main: name={:?} class={:?}",
        main.get_name().ok(),
        main.get_classname().ok()
    );

    eprintln!("\n--- all MenuItems under MAIN ---");
    dump_menu_items(&walker, &main, 0);

    eprintln!("\n--- all MenuItems under ROOT (desktop-wide) ---");
    dump_menu_items(&walker, &root, 0);

    Ok(())
}

fn dump_menu_items(walker: &UITreeWalker, root: &UIElement, max_depth: usize) {
    let mut stack: Vec<(UIElement, usize)> = vec![(root.clone(), 0)];
    while let Some((node, depth)) = stack.pop() {
        let ct = node.get_control_type().ok();
        if matches!(ct, Some(ControlType::MenuItem) | Some(ControlType::Menu)) {
            eprintln!(
                "{:indent$}[{:?}] name={:?}",
                "",
                ct.unwrap(),
                node.get_name().unwrap_or_default(),
                indent = depth * 2,
            );
        }
        if max_depth != 0 && depth >= max_depth {
            continue;
        }
        if let Ok(child) = walker.get_first_child(&node) {
            let mut cur = Some(child);
            while let Some(c) = cur {
                stack.push((c.clone(), depth + 1));
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
