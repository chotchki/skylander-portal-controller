//! Two unrelated jobs:
//!
//! 1. **`BUILD_TOKEN`** — stamp a short git commit hash (+ `-dirty`
//!    if the tree has uncommitted changes) into the binary,
//!    consumed by `http.rs`'s `/api/version` handler. The phone
//!    bakes the same token via its own `build.rs`; if they mismatch
//!    at runtime the phone raises a "stale bundle" overlay.
//!    Goal: detect when the *compiled* phone bundle and the
//!    *compiled* server binary drifted — both artifacts lock in
//!    the hash they were compiled against.
//!
//! 2. **Windows resources** (PLAN 10.9.3) — embed
//!    `assets/branding/icon.ico` and a `VERSIONINFO` block into the
//!    `.exe` via `winresource`. The icon is what File Explorer / the
//!    MSI shortcut / Big Picture's library tile fall back to when no
//!    Steam Grid artwork is set, so this is the load-bearing piece
//!    for the "shortcut shows the wrong icon" half of PLAN 10.8.5.
//!    The `VERSIONINFO` strings surface in Properties → Details and
//!    Add/Remove Programs (once the MSI lands in 10.9.1). Gated to
//!    `cfg(windows)` since `winresource` is a Windows-only build-dep.

use std::process::Command;

fn main() {
    stamp_build_token();
    #[cfg(windows)]
    embed_windows_resources();
}

fn stamp_build_token() {
    // Re-run when HEAD moves or the index changes (uncommitted edits).
    // Both are relative to the crate dir; `..` hops to workspace root,
    // `.git` holds the live state. Harmless if the user builds outside
    // a git checkout — we just fall back to "unknown".
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
    println!("cargo:rerun-if-env-changed=BUILD_TOKEN");

    let token = std::env::var("BUILD_TOKEN").unwrap_or_else(|_| compute_token());
    println!("cargo:rustc-env=BUILD_TOKEN={token}");
}

#[cfg(windows)]
fn embed_windows_resources() {
    // Re-link if either the script or the icon changes — without
    // this, edits to the .ico get silently ignored on incremental
    // builds (cargo only watches sources by default).
    println!("cargo:rerun-if-changed=../../assets/branding/icon.ico");

    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/branding/icon.ico");
    res.set("ProductName", "Skylander Portal Controller");
    res.set("FileDescription", "Skylander Portal Controller");
    res.set("CompanyName", "Christopher Hotchkiss");
    res.set("LegalCopyright", "Copyright (C) Christopher Hotchkiss");
    // OriginalFilename matches the binary cargo emits so resource-
    // inspection tools (PowerShell `Get-Item`, sigcheck) match this
    // .exe to its VERSIONINFO by filename.
    res.set("OriginalFilename", "skylander-portal-controller.exe");
    // CARGO_PKG_VERSION is the workspace `version` field — `0.1.0`
    // today, kept as a permanent placeholder. Real release versions
    // come from the git tag at MSI-build time (10.9.1 / 10.9.5);
    // the WiX template can override these strings if it cares.
    if let Ok(v) = std::env::var("CARGO_PKG_VERSION") {
        res.set("ProductVersion", &v);
        res.set("FileVersion", &v);
    }
    if let Err(e) = res.compile() {
        // `windres` ships with both the MSVC and GNU Rust toolchains —
        // CI's windows-latest runner has it, and a stock
        // `rustup default stable-msvc` install does too. If it's
        // missing locally, fail loudly rather than silently producing
        // an .exe with no icon + generic metadata.
        panic!(
            "embed icon + VERSIONINFO via winresource: {e}\n\
             (requires the windres tool from the active Rust toolchain)"
        );
    }
}

fn compute_token() -> String {
    let hash = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Dirty check: any tracked change (staged or unstaged) flips the
    // suffix. Untracked files don't count — they're irrelevant to what
    // the compiler will see. `git diff --quiet HEAD` is the cheapest
    // way to ask.
    let dirty = Command::new("git")
        .args(["diff", "--quiet", "HEAD"])
        .status()
        .ok()
        .map(|s| !s.success())
        .unwrap_or(false);

    if dirty { format!("{hash}-dirty") } else { hash }
}
