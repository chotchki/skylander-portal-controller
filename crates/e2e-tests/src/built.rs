//! The server binary a spawn runs, built once per run (openspec AQ.15, AG.78):
//! `cargo build -p <package> --features test-hooks` before the run's first
//! spawn, with no timeout on it, then the built file spawned directly. A
//! `cargo run` per spawn cost 0.33 s of freshness check each (20.9-22.2 s over
//! AQ.4's 63 spawns), and a cold target's build ran inside the 120 s wait for
//! the server's URL, so a fresh worktree's first spawns timed out (AG.78).
//!
//! Never a stale binary: before each spawn the built one is checked against
//! what it was built from ([`Built::is_current`]: every file under the repo in
//! cargo's dep-info for it, the manifests holding them and `Cargo.lock`, none
//! modified since the build started, and the file still the one the build
//! left), and built again when it isn't. The run's other test binaries take
//! the build from a record under `target/e2e-built/` that names the run (the
//! parent process they share, `cargo test`'s or nextest's, with its start
//! time); a record from another run, or one that isn't current, is built
//! over, so nothing carries across runs.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// A build this process ran, kept so later spawns skip cargo while
/// [`Built::is_current`] holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Built {
    pub(crate) exe: PathBuf,
    /// When the build that made `exe` started: a source modified at or after
    /// this may not be in it.
    started: SystemTime,
    /// `exe`'s length and modification time as the build left it, so a
    /// build of the same binary without `test-hooks` (cargo uplifts both to
    /// one path) is seen.
    stamp: (u64, SystemTime),
    /// What [`sources`] reads out of the dep-info: the repo's files only.
    sources: Vec<PathBuf>,
}

impl Built {
    /// Whether `exe` is still the build of the sources as they are now.
    pub(crate) fn is_current(&self) -> bool {
        stamp(&self.exe).ok() == Some(self.stamp)
            && self.sources.iter().all(|source| {
                std::fs::metadata(source)
                    .and_then(|m| m.modified())
                    .is_ok_and(|modified| modified < self.started)
            })
    }
}

/// The built binaries of this test process, one per package.
static BUILT: Mutex<Vec<(String, Built)>> = Mutex::new(Vec::new());

/// `package`'s server binary with `test-hooks`: this process's build or the
/// run's while current, else built. Blocks for as long as the build takes.
pub(crate) fn server_binary(repo: &Path, package: &str) -> Result<PathBuf> {
    let mut built = BUILT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, b)) = built.iter().find(|(p, _)| p == package)
        && b.is_current()
    {
        return Ok(b.exe.clone());
    }
    let run = run_id();
    let record = repo
        .join("target/e2e-built")
        .join(format!("{package}.json"));
    let fresh = match run.as_deref().and_then(|run| adopt(&record, run)) {
        Some(adopted) => adopted,
        None => {
            let fresh = build(repo, package)?;
            if let Some(run) = &run
                && let Err(e) = keep(&record, run, &fresh)
            {
                eprintln!("[e2e] the build stays this test binary's alone: {e:#}");
            }
            fresh
        }
    };
    let exe = fresh.exe.clone();
    built.retain(|(p, _)| p != package);
    built.push((package.to_owned(), fresh));
    Ok(exe)
}

/// A build some test binary of the run left for the others.
#[derive(Serialize, Deserialize)]
struct Record {
    run: String,
    built: Built,
}

/// The run this test binary is part of: the parent process every test binary
/// of a `cargo test` (or nextest) run shares, with its start time so a pid
/// used again isn't the same run. None where that can't be read.
#[cfg(unix)]
fn run_id() -> Option<String> {
    let parent = std::os::unix::process::parent_id();
    let out = Command::new("ps")
        .args(["-o", "lstart=", "-p", &parent.to_string()])
        .output()
        .ok()?;
    let started = String::from_utf8(out.stdout).ok()?;
    let started = started.trim();
    (out.status.success() && !started.is_empty()).then(|| format!("{parent} {started}"))
}

#[cfg(not(unix))]
fn run_id() -> Option<String> {
    None
}

/// The build `record` holds, if it's `run`'s and still current.
fn adopt(record: &Path, run: &str) -> Option<Built> {
    let record: Record = serde_json::from_slice(&std::fs::read(record).ok()?).ok()?;
    (record.run == run && record.built.is_current()).then_some(record.built)
}

/// Writes `built` as `run`'s record, whole or not at all.
fn keep(record: &Path, run: &str, built: &Built) -> Result<()> {
    std::fs::create_dir_all(record.parent().context("a record has a folder")?)?;
    let partial = record.with_extension(format!("json.{}", std::process::id()));
    let body = serde_json::to_vec(&Record {
        run: run.to_owned(),
        built: built.clone(),
    })?;
    std::fs::write(&partial, body)?;
    std::fs::rename(&partial, record)?;
    Ok(())
}

fn build(repo: &Path, package: &str) -> Result<Built> {
    let started = SystemTime::now();
    eprintln!("[e2e] cargo build -p {package} --features test-hooks (no timeout)");
    let mut child = Command::new("cargo")
        .current_dir(repo)
        .env("CARGO_TARGET_DIR", repo.join("target"))
        // The phone bundle's token (crates/e2e-tests/README.md): the server
        // stamps it at compile time, so it's the build's to set.
        .env("BUILD_TOKEN", "e2e-test")
        .args([
            "build",
            "--manifest-path",
            repo.join("Cargo.toml")
                .to_str()
                .context("repo path isn't UTF-8")?,
            "-p",
            package,
            "--features",
            "test-hooks",
            "--message-format=json-render-diagnostics",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("run cargo build")?;
    let stderr = child.stderr.take().context("cargo's stderr")?;
    let errors = std::thread::spawn(move || {
        let mut kept = Vec::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            eprintln!("[cargo] {line}");
            kept.push(line);
        }
        kept
    });
    let mut exes = Vec::new();
    let stdout = child.stdout.take().context("cargo's stdout")?;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(exe) = bin_executable(&line) {
            exes.push(exe);
        }
    }
    let status = child.wait().context("wait for cargo build")?;
    let errors = errors.join().unwrap_or_default();
    if !status.success() {
        let tail = errors[errors.len().saturating_sub(40)..].join("\n");
        bail!("cargo build -p {package} --features test-hooks failed ({status}):\n{tail}");
    }
    let [exe] = exes.as_slice() else {
        bail!(
            "cargo build -p {package} built {} binaries, not one: {exes:?}",
            exes.len()
        );
    };
    let dep_info = std::fs::read_to_string(exe.with_extension("d"))
        .with_context(|| format!("read {}'s dep-info", exe.display()))?;
    Ok(Built {
        exe: exe.clone(),
        started,
        stamp: stamp(exe)?,
        sources: sources(&dep_info, repo),
    })
}

/// The binary a `compiler-artifact` message names, if it's a bin's.
fn bin_executable(line: &str) -> Option<PathBuf> {
    let message: serde_json::Value = serde_json::from_str(line).ok()?;
    if message["reason"] != "compiler-artifact" {
        return None;
    }
    let kinds = message["target"]["kind"].as_array()?;
    if !kinds.iter().any(|k| k == "bin") {
        return None;
    }
    message["executable"].as_str().map(PathBuf::from)
}

fn stamp(exe: &Path) -> Result<(u64, SystemTime)> {
    let meta = std::fs::metadata(exe).with_context(|| format!("stat {}", exe.display()))?;
    Ok((meta.len(), meta.modified()?))
}

/// What a binary was built from, out of cargo's dep-info for it (`<exe>:
/// <path> <path> ...`, a space in a path escaped): the files under `repo`
/// (cargo's own check reads a dependency outside it by its version, never its
/// files), each one's package manifest and the workspace's `Cargo.lock`.
fn sources(dep_info: &str, repo: &Path) -> Vec<PathBuf> {
    let rule = dep_info
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    let deps = split_target(rule).unwrap_or("");
    let mut out: Vec<PathBuf> = vec![repo.join("Cargo.toml"), repo.join("Cargo.lock")];
    let mut path = String::new();
    let mut chars = deps.chars().peekable();
    loop {
        match chars.next() {
            Some('\\') if chars.peek().is_some() => path.push(chars.next().unwrap()),
            Some(c) if !c.is_whitespace() => path.push(c),
            next => {
                if !path.is_empty() {
                    let file = PathBuf::from(std::mem::take(&mut path));
                    if file.starts_with(repo) {
                        if let Some(manifest) = manifest_of(&file, repo) {
                            out.push(manifest);
                        }
                        out.push(file);
                    }
                }
                if next.is_none() {
                    break;
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The dependency list after a rule's `<target>:`, skipping escaped colons
/// and a drive letter's.
fn split_target(rule: &str) -> Option<&str> {
    let bytes = rule.as_bytes();
    (0..bytes.len())
        .find(|&i| {
            bytes[i] == b':'
                && (i == 0 || bytes[i - 1] != b'\\')
                && bytes.get(i + 1).is_none_or(|b| b.is_ascii_whitespace())
        })
        .map(|i| &rule[i + 1..])
}

/// The nearest `Cargo.toml` above `file`, at or under `repo`.
fn manifest_of(file: &Path, repo: &Path) -> Option<PathBuf> {
    file.ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(repo))
        .map(|dir| dir.join("Cargo.toml"))
        .find(|manifest| manifest.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::time::Duration;

    /// A repo with one package and its source, an exe beside it, and a
    /// `Built` whose build started `start` after the epoch.
    fn fixture(start: u64) -> (tempfile::TempDir, Built) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let src = repo.join("crates/app/src");
        std::fs::create_dir_all(&src).unwrap();
        for f in [
            "Cargo.toml",
            "Cargo.lock",
            "crates/app/Cargo.toml",
            "crates/app/src/main.rs",
        ] {
            std::fs::write(repo.join(f), f).unwrap();
            set_mtime(&repo.join(f), 100);
        }
        let exe = dir.path().join("app");
        std::fs::write(&exe, "binary").unwrap();
        set_mtime(&exe, 200);
        let dep_info = format!("{}: {}\n", exe.display(), src.join("main.rs").display());
        let built = Built {
            stamp: stamp(&exe).unwrap(),
            exe,
            started: SystemTime::UNIX_EPOCH + Duration::from_secs(start),
            sources: sources(&dep_info, &repo),
        };
        (dir, built)
    }

    fn set_mtime(path: &Path, secs: u64) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn a_build_of_unchanged_sources_is_current() {
        let (_dir, built) = fixture(150);
        assert_eq!(built.sources.len(), 4, "{:?}", built.sources);
        assert!(built.is_current());
    }

    #[test]
    fn a_source_modified_after_the_build_started_makes_it_stale() {
        let (dir, built) = fixture(150);
        set_mtime(&dir.path().join("repo/crates/app/src/main.rs"), 160);
        assert!(!built.is_current());
    }

    #[test]
    fn a_source_modified_as_the_build_started_makes_it_stale() {
        let (dir, built) = fixture(150);
        set_mtime(&dir.path().join("repo/crates/app/src/main.rs"), 150);
        assert!(!built.is_current());
    }

    #[test]
    fn an_edited_manifest_or_lock_makes_it_stale() {
        for file in ["repo/crates/app/Cargo.toml", "repo/Cargo.lock"] {
            let (dir, built) = fixture(150);
            set_mtime(&dir.path().join(file), 170);
            assert!(!built.is_current(), "{file}");
        }
    }

    #[test]
    fn a_removed_source_makes_it_stale() {
        let (dir, built) = fixture(150);
        std::fs::remove_file(dir.path().join("repo/crates/app/src/main.rs")).unwrap();
        assert!(!built.is_current());
    }

    #[test]
    fn the_binary_rebuilt_under_it_makes_it_stale() {
        let (_dir, built) = fixture(150);
        std::fs::write(&built.exe, "another build").unwrap();
        assert!(!built.is_current());
        let (_dir, built) = fixture(150);
        set_mtime(&built.exe, 300);
        assert!(!built.is_current());
    }

    #[test]
    fn the_runs_record_is_taken_while_current_and_only_by_its_run() {
        let (dir, built) = fixture(150);
        let record = dir.path().join("target/e2e-built/app.json");
        assert!(adopt(&record, "7 then").is_none(), "no record yet");
        keep(&record, "7 then", &built).unwrap();
        let adopted = adopt(&record, "7 then").expect("the run's own record");
        assert_eq!(adopted.exe, built.exe);
        assert!(adopted.is_current());
        assert!(adopt(&record, "7 later").is_none(), "another run's");
        set_mtime(&dir.path().join("repo/crates/app/src/main.rs"), 160);
        assert!(adopt(&record, "7 then").is_none(), "a source edited since");
        std::fs::write(&record, "half a reco").unwrap();
        assert!(adopt(&record, "7 then").is_none(), "unreadable");
    }

    #[cfg(unix)]
    #[test]
    fn a_run_is_this_processs_parent_and_when_it_started() {
        let run = run_id().expect("ps reads the parent");
        let parent = std::os::unix::process::parent_id().to_string();
        assert!(run.starts_with(&format!("{parent} ")), "{run}");
        assert_eq!(
            run_id().as_deref(),
            Some(run.as_str()),
            "the same run each time"
        );
    }

    #[test]
    fn dep_info_keeps_the_repos_files_with_their_manifests() {
        let repo = Path::new("/w/my repo");
        let dep_info = "/w/my\\ repo/target/debug/app: /w/my\\ repo/crates/app/src/main.rs \
            /home/u/.cargo/registry/src/x-1.0/src/lib.rs /w/my\\ repo/crates/app/src/a\\ b.rs\n\
            \n/w/my\\ repo/crates/app/src/main.rs:\n";
        let got = sources(dep_info, repo);
        assert_eq!(
            got,
            [
                "/w/my repo/Cargo.lock",
                "/w/my repo/Cargo.toml",
                "/w/my repo/crates/app/src/a b.rs",
                "/w/my repo/crates/app/src/main.rs",
            ]
            .map(PathBuf::from),
            "a registry file is left to cargo; no manifest found on disk here"
        );
    }

    #[test]
    fn a_bin_artifact_names_its_executable_and_nothing_else_does() {
        let bin = r#"{"reason":"compiler-artifact","target":{"kind":["bin"],"name":"app"},"executable":"/t/debug/app"}"#;
        let lib = r#"{"reason":"compiler-artifact","target":{"kind":["lib"],"name":"x"},"executable":null}"#;
        let script = r#"{"reason":"build-script-executed","package_id":"x"}"#;
        assert_eq!(bin_executable(bin), Some(PathBuf::from("/t/debug/app")));
        assert_eq!(bin_executable(lib), None);
        assert_eq!(bin_executable(script), None);
        assert_eq!(bin_executable("   Compiling x"), None);
    }
}
