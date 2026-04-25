//! Probe: enumerate candidate "Stop" controls on RPCS3's main window
//! and optionally drive `stop_emulation` to verify the UIA match works
//! (PLAN 4.15.16 validation).
//!
//! Prereq: RPCS3 is already running. For the driver-try path, a game
//! should be booted (viewport window visible). For the list-only path,
//! any state works.
//!
//! Usage:
//!     cargo run -p skylander-rpcs3-control --example stop_probe
//!     cargo run -p skylander-rpcs3-control --example stop_probe -- --try
//!
//! The list run walks every Button and MenuItem under RPCS3's main
//! window and prints each one's name + automation ID + parent control
//! type. Look for anything with "stop" in the name — that's the
//! candidate `stop_emulation` should match on. If the UIA-visible name
//! differs from our candidate list (`stop`, `stop emulation`,
//! `stop game`, `&stop`, `&stop emulation`), update the list in
//! `crates/rpcs3-control/src/uia.rs`.
//!
//! The `--try` run calls `stop_emulation` with a 10s timeout and
//! reports success/failure. Pair with a game booted so the driver has
//! something to stop; on success the viewport window disappears and
//! RPCS3 returns to library view.

#![cfg(windows)]

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

use skylander_rpcs3_control::{PortalDriver, UiaPortalDriver};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let try_stop = args.iter().any(|a| a == "--try");

    eprintln!("== stop_probe ==");

    let automation = UIAutomation::new().context("UIA init")?;
    let walker = automation.create_tree_walker().context("walker")?;
    let main = find_main_window(&automation, &walker)?;
    let name = main.get_name().unwrap_or_default();
    eprintln!("main window: {name:?}");

    eprintln!("\n-- all Button + MenuItem controls under main --");
    let mut count = 0usize;
    let mut stop_candidates: Vec<String> = Vec::new();
    walk(&walker, &main, 0, &mut |el, depth| {
        let kind = match el.get_control_type().ok() {
            Some(k) => k,
            None => return,
        };
        if !matches!(kind, ControlType::Button | ControlType::MenuItem) {
            return;
        }
        count += 1;
        let nm = el.get_name().unwrap_or_default();
        let aid = el.get_automation_id().unwrap_or_default();
        let indent = "  ".repeat(depth);
        eprintln!(
            "{indent}{kind:?}  name={nm:?}  automation_id={aid:?}",
            kind = kind
        );
        let lower = nm.to_ascii_lowercase();
        if lower.contains("stop") || lower.contains("halt") {
            stop_candidates.push(nm.clone());
        }
    });

    eprintln!(
        "\n-- {} Button/MenuItem controls total; {} matched 'stop' or 'halt' --",
        count,
        stop_candidates.len()
    );
    for c in &stop_candidates {
        eprintln!("    candidate: {c:?}");
    }

    if !try_stop {
        eprintln!("\n(pass --try to attempt stop_emulation() with a 10s timeout)");
        return Ok(());
    }

    eprintln!("\n-- attempting stop_emulation(10s) via UiaPortalDriver --");
    let driver = UiaPortalDriver::new().context("construct UiaPortalDriver")?;
    match driver.stop_emulation(Duration::from_secs(10)) {
        Ok(()) => eprintln!("✅ stop_emulation returned Ok — viewport confirmed gone"),
        Err(e) => eprintln!("❌ stop_emulation failed: {e:#}"),
    }
    Ok(())
}

fn walk<F: FnMut(&UIElement, usize)>(
    walker: &UITreeWalker,
    root: &UIElement,
    depth: usize,
    f: &mut F,
) {
    if depth > 15 {
        return;
    }
    f(root, depth);
    let mut child = walker.get_first_child(root).ok();
    while let Some(c) = child.clone() {
        walk(walker, &c, depth + 1, f);
        child = walker.get_next_sibling(&c).ok();
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
    Err(anyhow!("RPCS3 main window not found — is it running?"))
}
