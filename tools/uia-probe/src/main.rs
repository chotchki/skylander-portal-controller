//! UI Automation probe — survey what UIA + the MSAA bridge expose
//! on a target Qt/native window.
//!
//! Usage:
//!   uia-probe                        — dump all top-level windows (smoke)
//!   uia-probe "Skylanders Manager"   — dump the element tree rooted at that window
//!   uia-probe "RPCS3"                — dump the tree rooted at the main RPCS3 window
//!   uia-probe "RPCS3" --menus        — restrict output to MenuBar / Menu / MenuItem
//!
//! Prints a tree of each element's control type, name, class, AutomationId,
//! bounding rect, and the **full** set of UIA patterns advertised by each
//! element (Invoke, ExpandCollapse, SelectionItem, LegacyIAccessible, Toggle,
//! Value, Window, Transform, …). The expanded pattern coverage exists so we
//! can ground-truth which patterns Qt menu items actually expose before
//! refactoring `crates/rpcs3-control/src/uia.rs` away from keystroke synthesis
//! (PLAN 10.8.4).
//!
//! This is pure research: no clicks, no edits. Drive utility lives in
//! `tools/uia-drive/`.

use std::env;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Result, anyhow};
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIInvokePattern, UILegacyIAccessiblePattern, UISelectionItemPattern,
    UITogglePattern, UITransformPattern, UIValuePattern, UIWindowPattern,
};
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let menus_only = args.iter().any(|a| a == "--menus");
    // --expand "A,B,C" — find each named MenuItem in turn, ExpandCollapse.expand()
    // it, wait briefly for the children to populate, then walk the next level.
    // This validates whether the keystroke nav can be replaced by pattern calls.
    let expand_chain: Vec<String> = flag_value(&args, "--expand")
        .map(|s| s.split(',').map(|n| n.trim().to_string()).collect())
        .unwrap_or_default();
    // --invoke <leaf-name> — after expanding the chain, find the named MenuItem
    // and call InvokePattern.invoke() on it. End-to-end smoke for the
    // keystroke-free menu nav.
    let invoke_target = flag_value(&args, "--invoke");
    // --context-menu <element-name> — find the element by name (DataItem,
    // Button, anything), call its UIA `ShowContextMenu()` to pop the
    // right-click menu programmatically, then walk for the resulting
    // context-menu items so we can see what's available.
    let context_target = flag_value(&args, "--context-menu");
    let positional: Vec<String> = (1..args.len())
        .filter(|i| {
            let prev = i.checked_sub(1).and_then(|p| args.get(p)).map(|s| s.as_str());
            !args[*i].starts_with("--")
                && !matches!(prev, Some("--expand" | "--invoke" | "--context-menu"))
        })
        .map(|i| args[i].clone())
        .collect();
    let automation = UIAutomation::new()?;

    match positional.first().map(String::as_str) {
        None => list_top_level(&automation)?,
        Some(title) if context_target.is_some() => {
            context_menu_dump(&automation, title, context_target.as_deref().unwrap())?
        }
        Some(title) if expand_chain.is_empty() && invoke_target.is_none() => {
            dump_by_title(&automation, title, menus_only)?
        }
        Some(title) => {
            expand_and_dump(&automation, title, &expand_chain, invoke_target.as_deref())?
        }
    }
    Ok(())
}

fn context_menu_dump(automation: &UIAutomation, title: &str, target_name: &str) -> Result<()> {
    let root = automation.get_root_element()?;
    let walker = automation.create_tree_walker()?;
    let main = find_window_by_title(&walker, &root, title)
        .ok_or_else(|| anyhow!("no top-level window matching {title:?}"))?;

    let target = find_descendant_by_name(&walker, &main, target_name)
        .ok_or_else(|| anyhow!("no element named {target_name:?} under {title:?}"))?;
    println!("found target:");
    print_element(&target, 0);

    println!("\nbaseline top-level windows:");
    let baseline = top_level_titles(&walker, &root);
    for t in &baseline {
        println!("  {:?}", t);
    }
    println!("\ncalling show_context_menu()...");
    target
        .show_context_menu()
        .map_err(|e| anyhow!("show_context_menu failed: {e}"))?;
    sleep(Duration::from_millis(500));
    println!("\nall top-level windows after call:");
    let mut cur = walker.get_first_child(&root).ok();
    while let Some(el) = cur {
        print_element(&el, 0);
        cur = walker.get_next_sibling(&el).ok();
    }

    // Qt context menus typically appear as a top-level Menu/Window under
    // the desktop root, not as a child of the originating widget. Walk
    // both places.
    println!("\n-- top-level windows after show_context_menu (searching for new Menus): --");
    let mut cur = walker.get_first_child(&root).ok();
    while let Some(el) = cur {
        let ct = el.get_control_type().ok();
        let name = el.get_name().unwrap_or_default();
        if matches!(ct, Some(ControlType::Menu)) {
            print_element(&el, 0);
            // dump its children too — those are the menu items
            if let Ok(child) = walker.get_first_child(&el) {
                print_element(&child, 1);
                let mut c = child;
                while let Ok(next) = walker.get_next_sibling(&c) {
                    print_element(&next, 1);
                    c = next;
                }
            }
        } else if name.starts_with("RPCS3 ") {
            // also check children of RPCS3 main, in case the menu attached there
            println!("\n  (looking under RPCS3 main for newly-appeared Menus)");
            scan_for_menus(&walker, &el, 1);
        }
        cur = walker.get_next_sibling(&el).ok();
    }
    Ok(())
}

fn scan_for_menus(walker: &UITreeWalker, root: &UIElement, depth: usize) {
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if matches!(node.get_control_type().ok(), Some(ControlType::Menu)) {
            print_element(&node, depth);
            if let Ok(child) = walker.get_first_child(&node) {
                let mut cur = Some(child);
                while let Some(c) = cur {
                    print_element(&c, depth + 1);
                    cur = walker.get_next_sibling(&c).ok();
                }
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

fn find_descendant_by_name(
    walker: &UITreeWalker,
    root: &UIElement,
    name: &str,
) -> Option<UIElement> {
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node.get_name().ok().as_deref() == Some(name) {
            return Some(node);
        }
        if let Ok(child) = walker.get_first_child(&node) {
            let mut cur = Some(child);
            while let Some(c) = cur {
                stack.push(c.clone());
                cur = walker.get_next_sibling(&c).ok();
            }
        }
    }
    None
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find_map(|w| (w[0] == name).then(|| w[1].clone()))
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

fn dump_by_title(automation: &UIAutomation, title: &str, menus_only: bool) -> Result<()> {
    let root = automation.get_root_element()?;
    let walker = automation.create_tree_walker()?;

    let target = find_window_by_title(&walker, &root, title);
    match target {
        Some(el) => {
            println!("== tree for '{}' ==", title);
            dump_tree(&walker, &el, 0, 2000, menus_only);
        }
        None => eprintln!("no top-level window whose name contains {title:?}"),
    }
    Ok(())
}

fn expand_and_dump(
    automation: &UIAutomation,
    title: &str,
    chain: &[String],
    invoke_target: Option<&str>,
) -> Result<()> {
    let root = automation.get_root_element()?;
    let walker = automation.create_tree_walker()?;
    let target = find_window_by_title(&walker, &root, title)
        .ok_or_else(|| anyhow!("no top-level window matching {title:?}"))?;
    println!("== tree for '{}' ==", title);
    print_element(&target, 0);

    let mut current = target.clone();
    for (i, name) in chain.iter().enumerate() {
        println!("\n-- expanding {:?} --", name);
        let item = find_descendant_menuitem(&walker, &current, name)
            .ok_or_else(|| anyhow!("no MenuItem named {name:?} under current scope"))?;
        print_element(&item, 0);

        let pat = item.get_pattern::<UIExpandCollapsePattern>().map_err(|e| {
            anyhow!("MenuItem {name:?} doesn't expose ExpandCollapsePattern: {e}")
        })?;
        pat.expand()
            .map_err(|e| anyhow!("ExpandCollapse.expand() on {name:?} failed: {e}"))?;

        // Lazy-populate: Qt builds the submenu on-show. Give it a beat.
        sleep(Duration::from_millis(150));

        // Re-resolve the item from the live tree — the post-expand subtree
        // may attach to a fresh element rather than mutate the one we hold.
        let refreshed = find_descendant_menuitem(&walker, &target, name)
            .ok_or_else(|| anyhow!("post-expand: lost MenuItem {name:?}"))?;
        println!("\ndescendants of {:?}:", name);
        // Walk a couple of levels deep — the populated submenu often
        // sits one indirection (QMenu child) below the MenuItem.
        dump_tree(&walker, &refreshed, 1, 200, false);

        // Some Qt menus open their submenu under a separate top-level Menu/Popup
        // rather than as a tree-child of the MenuItem. Look for it under the
        // desktop root too on the last expand, so we can see the leaf items.
        if i + 1 == chain.len() {
            println!("\nsearching desktop-root for matching popup Menus:");
            let mut cur = walker.get_first_child(&root).ok();
            while let Some(c) = cur {
                let ct = c.get_control_type().ok();
                let name = c.get_name().unwrap_or_default();
                if matches!(
                    ct,
                    Some(ControlType::Menu) | Some(ControlType::Window) | Some(ControlType::Pane)
                ) && (name.is_empty() || !name.starts_with("RPCS3"))
                {
                    // surface anything menu-shaped under desktop root that
                    // wasn't there before
                    print_element(&c, 1);
                }
                cur = walker.get_next_sibling(&c).ok();
            }
        }

        current = refreshed;
    }

    if let Some(leaf) = invoke_target {
        println!("\n-- invoking {:?} --", leaf);
        // Give Qt a moment after the last expand — menu render happens
        // async on the GUI thread and an immediate invoke after expand
        // sometimes races into a "not yet ready" rejection.
        sleep(Duration::from_millis(200));
        let item = find_descendant_menuitem(&walker, &target, leaf)
            .ok_or_else(|| anyhow!("no MenuItem named {leaf:?} after expand chain"))?;
        print_element(&item, 0);

        // Try InvokePattern first; fall back to LegacyIAccessible.DoDefaultAction
        // if it fails. Qt occasionally rejects InvokePattern on actions that
        // pop modal dialogs, but the MSAA bridge's accDoDefaultAction often
        // works in that case (different code path inside Qt's accessibility
        // bridge).
        let invoke_result = item
            .get_pattern::<UIInvokePattern>()
            .map_err(|e| anyhow!("no InvokePattern: {e}"))
            .and_then(|p| p.invoke().map_err(|e| anyhow!("invoke failed: {e}")));
        match invoke_result {
            Ok(()) => println!("InvokePattern.invoke() returned Ok"),
            Err(e) => {
                println!("InvokePattern path failed ({e}); trying LegacyIAccessible");
                let legacy = item
                    .get_pattern::<UILegacyIAccessiblePattern>()
                    .map_err(|e| anyhow!("no LegacyIAccessiblePattern: {e}"))?;
                legacy
                    .do_default_action()
                    .map_err(|e| anyhow!("do_default_action failed: {e}"))?;
                println!("LegacyIAccessible.do_default_action() returned Ok");
            }
        }
        println!("invoke() returned Ok — waiting up to 3s for a new top-level window...");
        let baseline_titles = top_level_titles(&walker, &root);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut new_window: Option<String> = None;
        while std::time::Instant::now() < deadline {
            let now_titles = top_level_titles(&walker, &root);
            if let Some(t) = now_titles.iter().find(|t| !baseline_titles.contains(t)) {
                new_window = Some(t.clone());
                break;
            }
            sleep(Duration::from_millis(100));
        }
        match new_window {
            Some(t) => println!("✓ new top-level window appeared: {:?}", t),
            None => println!("✗ no new top-level window within 3s"),
        }
    }
    Ok(())
}

fn top_level_titles(walker: &UITreeWalker, root: &UIElement) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = walker.get_first_child(root).ok();
    while let Some(el) = cur {
        if let Ok(name) = el.get_name() {
            out.push(name);
        }
        cur = walker.get_next_sibling(&el).ok();
    }
    out
}

fn find_descendant_menuitem(
    walker: &UITreeWalker,
    root: &UIElement,
    name: &str,
) -> Option<UIElement> {
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if matches!(
            node.get_control_type().ok(),
            Some(ControlType::MenuItem) | Some(ControlType::Menu)
        ) && node.get_name().ok().as_deref() == Some(name)
        {
            return Some(node);
        }
        if let Ok(child) = walker.get_first_child(&node) {
            let mut cur = Some(child);
            while let Some(c) = cur {
                stack.push(c.clone());
                cur = walker.get_next_sibling(&c).ok();
            }
        }
    }
    None
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

fn dump_tree(
    walker: &UITreeWalker,
    el: &UIElement,
    depth: usize,
    budget: usize,
    menus_only: bool,
) -> usize {
    if depth > 20 || budget == 0 {
        return budget;
    }
    let show = !menus_only || is_menu_kind(el);
    if show {
        print_element(el, depth);
    }
    let mut budget = budget.saturating_sub(1);

    if let Ok(child) = walker.get_first_child(el) {
        budget = dump_tree(walker, &child, depth + 1, budget, menus_only);
        let mut cur = child;
        while let Ok(next) = walker.get_next_sibling(&cur) {
            if budget == 0 {
                return 0;
            }
            budget = dump_tree(walker, &next, depth + 1, budget, menus_only);
            cur = next;
        }
    }
    budget
}

fn is_menu_kind(el: &UIElement) -> bool {
    matches!(
        el.get_control_type().ok(),
        Some(ControlType::Menu) | Some(ControlType::MenuBar) | Some(ControlType::MenuItem)
    )
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

    // Try every pattern relevant to menu navigation + general controls.
    // The menu refactor (PLAN 10.8.4) hinges on whether Qt6 menu items
    // expose Invoke / ExpandCollapse / LegacyIAccessible — keystroke
    // synthesis goes away if any do.
    let mut tags = Vec::new();
    if el.get_pattern::<UIInvokePattern>().is_ok() {
        tags.push("Invoke");
    }
    if el.get_pattern::<UIExpandCollapsePattern>().is_ok() {
        tags.push("ExpandCollapse");
    }
    if el.get_pattern::<UISelectionItemPattern>().is_ok() {
        tags.push("SelectionItem");
    }
    if el.get_pattern::<UILegacyIAccessiblePattern>().is_ok() {
        tags.push("LegacyIAccessible");
    }
    if el.get_pattern::<UIValuePattern>().is_ok() {
        tags.push("Value");
    }
    if el.get_pattern::<UITogglePattern>().is_ok() {
        tags.push("Toggle");
    }
    if el.get_pattern::<UIWindowPattern>().is_ok() {
        tags.push("Window");
    }
    if el.get_pattern::<UITransformPattern>().is_ok() {
        tags.push("Transform");
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
