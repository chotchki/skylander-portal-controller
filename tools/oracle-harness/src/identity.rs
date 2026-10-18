//! The identity line every oracle observation carries (openspec AF, design
//! decision 1): the binary's SHA-256, the upstream base and series commit and
//! source tree `rpcs3-patches/oracle/build.sh` recorded in `oracle-build.txt`
//! beside the `.app`, the MoltenVK it was built against and the shader mode the
//! home's config runs. The hash is taken from the binary itself, so a binary
//! that no longer matches its `oracle-build.txt` says so in the line.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::outputs::hex;

/// What `build.sh` wrote, the fields the identity line takes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildRecord {
    pub sha256: Option<String>,
    pub upstream_base: Option<String>,
    pub series_commit: Option<String>,
    pub checkout: Option<String>,
    pub moltenvk: Option<String>,
}

impl BuildRecord {
    /// Reads `oracle-build.txt`'s `key   value` lines; unknown lines are skipped.
    pub fn parse(text: &str) -> Self {
        let mut rec = Self::default();
        for line in text.lines() {
            let field = |key: &str| line.strip_prefix(key).map(|v| v.trim().to_owned());
            if let Some(v) = field("sha256 ") {
                rec.sha256 = Some(v);
            } else if let Some(v) = field("upstream base ") {
                rec.upstream_base = Some(v);
            } else if let Some(v) = field("series commit ") {
                rec.series_commit = Some(v);
            } else if let Some(v) = field("checkout HEAD ") {
                rec.checkout = Some(v);
            } else if let Some(v) = field("MoltenVK ") {
                rec.moltenvk = v.split_whitespace().next().map(str::to_owned);
            }
        }
        rec
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub binary: PathBuf,
    pub sha256: String,
    /// `None` when there's no `oracle-build.txt` beside the `.app`.
    pub build: Option<BuildRecord>,
    pub shader_mode: Option<String>,
}

impl Identity {
    /// Hashes `exe` and reads the record beside its bundle and the shader mode
    /// in `config_dir/config.yml`.
    pub fn of(exe: &Path, config_dir: &Path) -> Result<Self> {
        let bytes = std::fs::read(exe).with_context(|| format!("reading {}", exe.display()))?;
        let build = build_record_path(exe)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| BuildRecord::parse(&t));
        let shader_mode = std::fs::read_to_string(config_dir.join("config.yml"))
            .ok()
            .and_then(|t| shader_mode(&t));
        Ok(Self {
            binary: exe.to_path_buf(),
            sha256: hex(&Sha256::digest(&bytes)),
            build,
            shader_mode,
        })
    }

    /// One line, `key=value` pairs; `?` for what isn't known.
    pub fn line(&self) -> String {
        let unknown = || "?".to_owned();
        let build = self.build.clone().unwrap_or_default();
        let mut line = format!(
            "oracle sha256={} base={} series=\"{}\" checkout=\"{}\" moltenvk={} shader=\"{}\"",
            self.sha256,
            build.upstream_base.unwrap_or_else(unknown),
            build.series_commit.unwrap_or_else(unknown),
            build.checkout.unwrap_or_else(unknown),
            build.moltenvk.unwrap_or_else(unknown),
            self.shader_mode.clone().unwrap_or_else(unknown),
        );
        match &self.build {
            None => line.push_str(" (no oracle-build.txt beside the .app)"),
            Some(b) if b.sha256.as_deref() != Some(self.sha256.as_str()) => {
                line.push_str(" (the binary doesn't match its oracle-build.txt)")
            }
            Some(_) => {}
        }
        line
    }
}

/// `<dir>/oracle-build.txt` for `<dir>/<name>.app/Contents/MacOS/rpcs3`.
fn build_record_path(exe: &Path) -> Option<PathBuf> {
    let app = exe.parent()?.parent()?.parent()?;
    Some(app.parent()?.join("oracle-build.txt"))
}

/// `Video: Shader Mode` from a `config.yml`.
fn shader_mode(config: &str) -> Option<String> {
    config
        .lines()
        .find_map(|l| l.trim().strip_prefix("Shader Mode: "))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORD: &str = "oracle build (openspec AF.4), 2026-10-18T21:39:05Z
binary        /x/RPCS3.app/Contents/MacOS/rpcs3
sha256        HASH
upstream base accfecd2d13ec634e96d0edf97777e657f2d9359
series commit adba9e22 (rpcs3-patches/ modified since)
checkout HEAD a9ed9184 (tree 3688db4f)
MoltenVK      1.4.2 (Homebrew's, loaded through /opt/homebrew/etc/vulkan/icd.d; not bundled)
LLVM          21.1.8 (static)
patches:
  91bd  rpcs3-patches/oracle/0001-O1.patch
";

    fn bundle(tmp: &Path, exe_bytes: &[u8], record: Option<&str>) -> PathBuf {
        let exe = tmp.join("oracle/RPCS3.app/Contents/MacOS/rpcs3");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, exe_bytes).unwrap();
        if let Some(r) = record {
            std::fs::write(tmp.join("oracle/oracle-build.txt"), r).unwrap();
        }
        let config = tmp.join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("config.yml"),
            "Video:\n  Disable FIFO Reordering: true\n  Shader Mode: Async Recompiler with Shader Interpreter\n",
        )
        .unwrap();
        exe
    }

    #[test]
    fn the_build_record_parses_build_shs_lines() {
        let rec = BuildRecord::parse(RECORD);
        assert_eq!(rec.sha256.as_deref(), Some("HASH"));
        assert_eq!(
            rec.upstream_base.as_deref(),
            Some("accfecd2d13ec634e96d0edf97777e657f2d9359")
        );
        assert_eq!(
            rec.series_commit.as_deref(),
            Some("adba9e22 (rpcs3-patches/ modified since)")
        );
        assert_eq!(rec.checkout.as_deref(), Some("a9ed9184 (tree 3688db4f)"));
        assert_eq!(rec.moltenvk.as_deref(), Some("1.4.2"));
    }

    #[test]
    fn the_line_names_the_binary_by_its_own_hash() {
        let tmp = tempfile::tempdir().unwrap();
        // SHA-256 of "abc".
        let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let exe = bundle(tmp.path(), b"abc", Some(&RECORD.replace("HASH", sha)));
        let id = Identity::of(&exe, &tmp.path().join("config")).unwrap();
        assert_eq!(
            id.line(),
            format!(
                "oracle sha256={sha} base=accfecd2d13ec634e96d0edf97777e657f2d9359 \
                 series=\"adba9e22 (rpcs3-patches/ modified since)\" \
                 checkout=\"a9ed9184 (tree 3688db4f)\" moltenvk=1.4.2 \
                 shader=\"Async Recompiler with Shader Interpreter\""
            )
        );
    }

    #[test]
    fn a_binary_off_its_record_or_without_one_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = bundle(tmp.path(), b"rebuilt", Some(RECORD));
        let line = Identity::of(&exe, &tmp.path().join("config"))
            .unwrap()
            .line();
        assert!(
            line.ends_with("(the binary doesn't match its oracle-build.txt)"),
            "{line}"
        );

        let tmp = tempfile::tempdir().unwrap();
        let exe = bundle(tmp.path(), b"bare", None);
        let id = Identity::of(&exe, &tmp.path().join("nowhere")).unwrap();
        let line = id.line();
        assert!(line.contains("base=? series=\"?\""), "{line}");
        assert!(line.contains("shader=\"?\""), "{line}");
        assert!(
            line.ends_with("(no oracle-build.txt beside the .app)"),
            "{line}"
        );
    }
}
