//! Cached tailwindcss CLI runner (PLAN 9.1).
//!
//! Trunk's pre_build hook calls this; the hook does:
//!
//! ```toml
//! [[hooks]]
//! stage = "pre_build"
//! command = "cargo"
//! command_arguments = ["run", "--quiet", "--manifest-path", "../Cargo.toml",
//!                      "-p", "tailwind-build"]
//! ```
//!
//! Behaviour:
//!
//! - Detects host OS + arch.
//! - Looks for the pinned `tailwindcss` standalone binary in
//!   `phone/.tailwind-cache/`.
//! - Downloads it from GitHub releases on first miss; ~50MB, but a
//!   one-time cost. The cache filename includes the version so a
//!   `TAILWIND_VERSION` bump invalidates the old binary cleanly.
//! - On Unix, `chmod +x` after download.
//! - Invokes the binary with `--input phone/styles/input.css
//!   --output phone/styles/tailwind.css --minify`. The matching
//!   `<link data-trunk rel="copy-file" href="styles/tailwind.css">`
//!   in `phone/index.html` carries the bundle into trunk's staged
//!   `dist/` (writing straight to `dist/` doesn't survive trunk's
//!   "apply new distribution" wholesale-replace step).
//!
//! CI hint: `actions/cache` keyed on `TAILWIND_VERSION + os` should
//! point at `phone/.tailwind-cache/` so the download doesn't repeat.
//!
//! No async runtime. No retry logic. Failures bubble up as `anyhow`
//! errors and the trunk build aborts — same as any other pre_build
//! hook failure.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};

/// Pinned tailwindcss release. Bump deliberately; CI cache key
/// includes this string so a bump triggers a fresh download.
const TAILWIND_VERSION: &str = "4.1.7";

fn main() -> Result<()> {
    let repo_root = repo_root()?;
    let phone_dir = repo_root.join("phone");
    let cache_dir = phone_dir.join(".tailwind-cache");
    std::fs::create_dir_all(&cache_dir)
        .with_context(|| format!("create cache dir {}", cache_dir.display()))?;

    let target = host_target()?;
    let binary_filename = format!(
        "tailwindcss-{}-{}{}",
        TAILWIND_VERSION, target.suffix, target.exe_ext
    );
    let binary_path = cache_dir.join(&binary_filename);

    if !binary_path.exists() {
        let url = format!(
            "https://github.com/tailwindlabs/tailwindcss/releases/download/v{}/tailwindcss-{}{}",
            TAILWIND_VERSION, target.suffix, target.exe_ext
        );
        eprintln!(
            "[tailwind-build] downloading {url} → {}",
            binary_path.display()
        );
        download(&url, &binary_path).with_context(|| format!("download {url}"))?;
        #[cfg(unix)]
        make_executable(&binary_path)?;
        eprintln!("[tailwind-build] cached {}", binary_path.display());
    }

    // Output goes into the tracked `phone/styles/` source dir, NOT
    // straight into `dist/`. Trunk's post-build "apply new
    // distribution" step replaces the dist directory wholesale, so
    // anything we drop there gets blown away. The matching
    // `<link data-trunk rel="copy-file" href="styles/tailwind.css">`
    // in `phone/index.html` carries the generated bundle into the
    // staged dist + final dist atomically with the wasm.
    let styles_dir = phone_dir.join("styles");
    std::fs::create_dir_all(&styles_dir)
        .with_context(|| format!("create styles dir {}", styles_dir.display()))?;

    let input = styles_dir.join("input.css");
    let output = styles_dir.join("tailwind.css");
    if !input.exists() {
        bail!(
            "tailwind input not found at {} — set up phone/styles/input.css",
            input.display()
        );
    }

    let status = Command::new(&binary_path)
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--minify")
        .current_dir(&phone_dir)
        .status()
        .with_context(|| format!("invoke {}", binary_path.display()))?;
    if !status.success() {
        bail!("tailwindcss exited with status {status}");
    }
    Ok(())
}

struct HostTarget {
    /// `windows-x64`, `macos-arm64`, etc. — matches the GH release
    /// asset naming.
    suffix: &'static str,
    /// `.exe` on Windows, empty elsewhere.
    exe_ext: &'static str,
}

fn host_target() -> Result<HostTarget> {
    let suffix = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("macos", "x86_64") => "macos-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        (os, arch) => bail!("unsupported host {os}/{arch} — add a tailwindcss release suffix"),
    };
    let exe_ext = if cfg!(windows) { ".exe" } else { "" };
    Ok(HostTarget { suffix, exe_ext })
}

fn download(url: &str, dest: &Path) -> Result<()> {
    let response = ureq::get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    if response.status() != 200 {
        bail!("GET {url} returned {}", response.status());
    }
    let mut reader = response.into_reader();
    let tmp = dest.with_extension("partial");
    {
        let mut file =
            std::fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).context("read response")?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).context("write to cache")?;
        }
        file.flush().ok();
    }
    std::fs::rename(&tmp, dest)
        .with_context(|| format!("rename {} → {}", tmp.display(), dest.display()))?;
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
        .with_context(|| format!("chmod 0755 {}", path.display()))?;
    Ok(())
}

/// Resolve the workspace root from this tool's `CARGO_MANIFEST_DIR`.
/// Layout: `<repo>/tools/tailwind-build/Cargo.toml`, so `..\..` is
/// the repo root.
fn repo_root() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("can't resolve repo root from {}", manifest.display()))
}
