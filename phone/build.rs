//! Mirror of `crates/server/build.rs`: stamps the same short git hash
//! (+ `-dirty` suffix when the tree is modified) into `BUILD_TOKEN`.
//! The phone's boot-time version check fetches `/api/version` and
//! compares the server's token to this one — mismatch raises a
//! "stale bundle" overlay so iOS PWAs holding cached wasm don't
//! silently diverge from the current server build.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Re-run when HEAD moves or the index changes (uncommitted edits).
    for name in ["HEAD", "index"] {
        if let Some(path) = git_path(name) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
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

/// `git rev-parse --git-path`: right in a linked worktree too, where `.git`
/// is a file and a literal `.git/HEAD` never exists (cargo then reruns this
/// script, and rebuilds everything downstream, on every build). None when
/// the path is missing, so a non-git build doesn't rerun forever either.
fn git_path(name: &str) -> Option<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--git-path", name])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let path = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    // Relative paths are relative to the cwd, the crate dir.
    let path = if path.is_relative() {
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?).join(path)
    } else {
        path
    };
    path.exists().then_some(path)
}
