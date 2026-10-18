//! **Windows resources** (PLAN 10.9.3) — embed
//! `assets/branding/icon.ico` and a `VERSIONINFO` block into the
//! `.exe` via `winresource`. The icon is what File Explorer / the
//! MSI shortcut / Big Picture's library tile fall back to when no
//! Steam Grid artwork is set, so this is the load-bearing piece
//! for the "shortcut shows the wrong icon" half of PLAN 10.8.5.
//! The `VERSIONINFO` strings surface in Properties → Details and
//! Add/Remove Programs (once the MSI lands in 10.9.1). Gated to
//! `cfg(windows)` since `winresource` is a Windows-only build-dep.
//!
//! The `BUILD_TOKEN` stamp `/api/version` serves moved to
//! `crates/guac-server/build.rs` with the HTTP routes (openspec AA.1).

fn main() {
    #[cfg(windows)]
    embed_windows_resources();
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
