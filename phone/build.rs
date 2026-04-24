//! Mirror of `crates/server/build.rs`: stamps the same short git hash
//! (+ `-dirty` suffix when the tree is modified) into `BUILD_TOKEN`.
//! The phone's boot-time version check fetches `/api/version` and
//! compares the server's token to this one — mismatch raises a
//! "stale bundle" overlay so iOS PWAs holding cached wasm don't
//! silently diverge from the current server build.

use std::process::Command;

fn main() {
    // `../.git` because `phone/` lives directly beneath the workspace
    // root (unlike `crates/server/` which is one deeper).
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");
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
