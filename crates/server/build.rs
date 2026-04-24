//! Stamp a short git commit hash (+ `-dirty` if the tree has
//! uncommitted changes) into `BUILD_TOKEN`, consumed by `http.rs`'s
//! `/api/version` handler. The phone bakes the same token via its own
//! `build.rs`; if they mismatch at runtime the phone raises a
//! "stale bundle" overlay.
//!
//! Why build.rs not runtime: the goal is to detect when the *compiled*
//! phone bundle and the *compiled* server binary drifted, so computing
//! the hash at each boot from a repo on disk would miss the case where
//! the phone was built from an older checkout and served from a newer
//! server. Both artifacts lock in the hash they were compiled against.

use std::process::Command;

fn main() {
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

    if dirty {
        format!("{hash}-dirty")
    } else {
        hash
    }
}
