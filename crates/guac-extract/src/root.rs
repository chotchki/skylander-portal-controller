//! Where extracted files go (design decision 5), and the refusal that keeps
//! them out of any git checkout.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::error::{ExtractError, Result};

/// How the extract root is chosen. The caller picks by its own build: a dev
/// build of guacamole has its data root inside the checkout, so it can't
/// derive one from it.
#[derive(Debug, Clone)]
pub enum RootMode {
    /// `extract/` under guacamole's data root. `GUAC_EXTRACT_DIR` is ignored.
    Release { data_root: PathBuf },
    /// `GUAC_EXTRACT_DIR`'s value, required: no default.
    Dev { extract_dir: Option<OsString> },
}

/// An extract root that was outside every git working tree when it was
/// resolved. The only way to get one is through that check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractRoot(PathBuf);

impl ExtractRoot {
    pub fn resolve(mode: RootMode) -> Result<Self> {
        let path = match mode {
            RootMode::Release { data_root } => data_root.join("extract"),
            RootMode::Dev { extract_dir } => match extract_dir {
                Some(dir) if !dir.is_empty() => PathBuf::from(dir),
                _ => return Err(ExtractError::NoExtractDir),
            },
        };
        Self::checked(path)
    }

    /// Makes `path` absolute and refuses it inside a git working tree.
    pub fn checked(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let abs = normalize(&std::path::absolute(path).map_err(ExtractError::io(path))?);
        if let Some(repo) = enclosing_git_tree(&abs) {
            return Err(ExtractError::InCheckout { root: abs, repo });
        }
        Ok(Self(abs))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Where an unknown-dump report goes: beside the root (its parent).
    pub fn report_dir(&self) -> &Path {
        self.0.parent().unwrap_or(&self.0)
    }
}

/// The nearest folder at or above `path` holding a `.git` (a repo's dir, or a
/// worktree's or submodule's file), looked for both along `path` as written and
/// along its resolved form, so a symlink into a checkout is caught too. `path`
/// needn't exist.
pub fn enclosing_git_tree(path: &Path) -> Option<PathBuf> {
    let has_git = |dir: &Path| dir.join(".git").symlink_metadata().is_ok();
    let lexical = path.ancestors().find(|a| has_git(a));
    lexical.map(Path::to_path_buf).or_else(|| {
        resolved(path)?
            .ancestors()
            .find(|a| has_git(a))
            .map(Path::to_path_buf)
    })
}

/// Drops `.` and folds `..` lexically, so the ancestor walk sees real names.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `path` with its longest existing prefix canonicalized and the rest appended.
pub(crate) fn resolved(path: &Path) -> Option<PathBuf> {
    let mut rest = Vec::new();
    let mut base = path;
    loop {
        if let Ok(canon) = base.canonicalize() {
            return Some(rest.iter().rev().fold(canon, |p, c| p.join(c)));
        }
        rest.push(base.file_name()?);
        base = base.parent()?;
    }
}

/// Refuses a root inside the dump or a dump inside the root: the first would
/// write into the dump, the second could delete from it when a stale identity
/// dir is rebuilt.
pub(crate) fn check_disjoint(root: &ExtractRoot, dump: &Path) -> Result<()> {
    let overlap = || ExtractError::OverlapsDump {
        root: root.path().to_path_buf(),
        dump: dump.to_path_buf(),
    };
    let r = resolved(root.path()).ok_or_else(overlap)?;
    let d = dump
        .canonicalize()
        .map_err(|e| ExtractError::unreadable(dump, e))?;
    if r.starts_with(&d) || d.starts_with(&r) {
        return Err(overlap());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_without_the_variable_is_refused() {
        for extract_dir in [None, Some(OsString::new())] {
            assert!(matches!(
                ExtractRoot::resolve(RootMode::Dev { extract_dir }),
                Err(ExtractError::NoExtractDir)
            ));
        }
    }

    #[test]
    fn release_is_extract_under_the_data_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = ExtractRoot::resolve(RootMode::Release {
            data_root: tmp.path().join("guacamole"),
        })
        .unwrap();
        assert_eq!(root.path(), tmp.path().join("guacamole/extract"));
        assert_eq!(root.report_dir(), tmp.path().join("guacamole"));
    }

    #[test]
    fn a_worktree_git_file_counts() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".git"), "gitdir: elsewhere\n").unwrap();
        let err = ExtractRoot::checked(tmp.path().join("a/b")).unwrap_err();
        assert!(matches!(err, ExtractError::InCheckout { .. }), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_into_a_checkout_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(repo.join("sub"), outside.join("link")).unwrap();
        let err = ExtractRoot::checked(outside.join("link/extract")).unwrap_err();
        match err {
            ExtractError::InCheckout { repo: found, .. } => {
                assert_eq!(found, repo.canonicalize().unwrap())
            }
            other => panic!("{other}"),
        }
    }
}
