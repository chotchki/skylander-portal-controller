//! Phase 1 spike 1a — UI Automation probe for RPCS3's Skylanders Manager dialog.
//!
//! Usage:
//!   uia-probe                        — dump all top-level windows (smoke)
//!   uia-probe "Skylanders Manager"   — dump the element tree rooted at that window
//!   uia-probe "RPCS3"                — dump the tree rooted at the main RPCS3 window
//!
//! Prints a tree of each element's name, control type, class name, AutomationId,
//! bounding rect, and — for buttons/edits — Invoke/ValuePattern availability.
//!
//! This is pure research: no clicks, no edits. We want to know what we can see
//! before we try to drive anything.

use std::env;

use anyhow::Result;
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let automation = UIAutomation::new()?;

    match args.get(1).map(String::as_str) {
        None => list_top_level(&automation)?,
        Some(title) => dump_by_title(&automation, title)?,
    }
    Ok(())
}

fn list_top_level(automation: &UIAutomation) -> Result<()> {
    let root = automation.get_root_element()?;
    let walker = automation.create_tree_walker()?;

    println!("== top-level windows ==");
    if let Ok(first) = walker.get_first_child(&root) {
        print_element(&first, 0);
        let mut cur = first;
        while let Ok(next) = walker.get_next_sibling(&cur) {
            print_element(&next, 0);
            cur = next;
        }
    }
    Ok(())
}

fn dump_by_title(automation: &UIAutomation, title: &str) -> Result<()> {
    let root = automation.get_root_element()?;
    let walker = automation.create_tree_walker()?;

    let target = find_window_by_title(&walker, &root, title);
    match target {
        Some(el) => {
            println!("== tree for '{}' ==", title);
            dump_tree(&walker, &el, 0, 200);
        }
        None => eprintln!("no top-level window whose name contains {title:?}"),
    }
    Ok(())
}

fn find_window_by_title(walker: &UITreeWalker, root: &UIElement, needle: &str) -> Option<UIElement> {
    let needle_lc = needle.to_lowercase();
    let mut cur = walker.get_first_child(root).ok()?;
    loop {
        if let Ok(name) = cur.get_name() {
            if name.to_lowercase().contains(&needle_lc) {
                return Some(cur);
            }
        }
        cur = walker.get_next_sibling(&cur).ok()?;
    }
}

fn dump_tree(walker: &UITreeWalker, el: &UIElement, depth: usize, budget: usize) -> usize {
    if depth > 20 || budget == 0 {
        return budget;
    }
    print_element(el, depth);
    let mut budget = budget.saturating_sub(1);

    if let Ok(child) = walker.get_first_child(el) {
        budget = dump_tree(walker, &child, depth + 1, budget);
        let mut cur = child;
        while let Ok(next) = walker.get_next_sibling(&cur) {
            if budget == 0 {
                return 0;
            }
            budget = dump_tree(walker, &next, depth + 1, budget);
            cur = next;
        }
    }
    budget
}

fn print_element(el: &UIElement, depth: usize) {
    let indent = "  ".repeat(depth);
    let name = el.get_name().unwrap_or_default();
    let class = el.get_classname().unwrap_or_default();
    let ctrl = el
        .get_control_type()
        .map(|c| format!("{c:?}"))
        .unwrap_or_default();
    let aid = el.get_automation_id().unwrap_or_default();
    let rect = el
        .get_bounding_rectangle()
        .map(|r| format!("{}x{}@{},{}", r.get_width(), r.get_height(), r.get_left(), r.get_top()))
        .unwrap_or_default();

    let mut tags = Vec::new();
    if el.get_pattern::<uiautomation::patterns::UIInvokePattern>().is_ok() {
        tags.push("Invoke");
    }
    if el.get_pattern::<uiautomation::patterns::UIValuePattern>().is_ok() {
        tags.push("Value");
    }
    if el.get_pattern::<uiautomation::patterns::UITogglePattern>().is_ok() {
        tags.push("Toggle");
    }
    let tags = if tags.is_empty() {
        String::new()
    } else {
        format!("  [{}]", tags.join(","))
    };

    let name_short = truncate(&name, 80);
    let class_suffix = if class.is_empty() {
        String::new()
    } else {
        format!(" <{}>", class)
    };
    let aid_suffix = if aid.is_empty() {
        String::new()
    } else {
        format!(" #{aid}")
    };

    println!(
        "{indent}{ctrl} \"{name_short}\"{class_suffix}{aid_suffix}  {rect}{tags}"
    );
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}
