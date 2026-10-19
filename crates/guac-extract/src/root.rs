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

/// An extract root that was outside every git working tree it's checked
/// against when it was resolved. The only way to get one is through that check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractRoot {
    path: PathBuf,
    /// Release only: the data root, where the walk for a `.git` stops (AB.18).
    ceiling: Option<PathBuf>,
}

impl ExtractRoot {
    /// Release refuses only a `.git` at or below the data root: the OS data
    /// dir is never inside this repo, which is what the refusal protects, and
    /// a home folder kept under git (`git init ~`) mustn't block every Giants
    /// pick (openspec AB.18). Dev refuses one in any ancestor.
    pub fn resolve(mode: RootMode) -> Result<Self> {
        match mode {
            RootMode::Release { data_root } => {
                let ceiling = absolute(&data_root)?;
                Self::check(ceiling.join("extract"), Some(ceiling))
            }
            RootMode::Dev { extract_dir } => match extract_dir {
                Some(dir) if !dir.is_empty() => Self::checked(PathBuf::from(dir)),
                _ => Err(ExtractError::NoExtractDir),
            },
        }
    }

    /// Makes `path` absolute and refuses it inside any git working tree.
    pub fn checked(path: impl AsRef<Path>) -> Result<Self> {
        Self::check(absolute(path.as_ref())?, None)
    }

    fn check(path: PathBuf, ceiling: Option<PathBuf>) -> Result<Self> {
        let root = Self { path, ceiling };
        root.ensure_outside_checkouts()?;
        Ok(root)
    }

    /// The check again, as it was resolved: at use, since the root may have
    /// been resolved long before.
    pub(crate) fn ensure_outside_checkouts(&self) -> Result<()> {
        match git_tree_within(&self.path, self.ceiling.as_deref()) {
            Some(repo) => Err(ExtractError::InCheckout {
                root: self.path.clone(),
                repo,
            }),
            None => Ok(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where an unknown-dump report goes: beside the root (its parent).
    pub fn report_dir(&self) -> &Path {
        self.path.parent().unwrap_or(&self.path)
    }
}

/// `path` made absolute with `.` and `..` folded.
fn absolute(path: &Path) -> Result<PathBuf> {
    Ok(normalize(
        &std::path::absolute(path).map_err(ExtractError::io(path))?,
    ))
}

/// The nearest folder at or above `path` holding a `.git` (a repo's dir, or a
/// worktree's or submodule's file), looked for both along `path` as written and
/// along its resolved form, so a symlink into a checkout is caught too. `path`
/// needn't exist.
pub fn enclosing_git_tree(path: &Path) -> Option<PathBuf> {
    git_tree_within(path, None)
}

/// [`enclosing_git_tree`], looking no higher than `ceiling` (itself included)
/// when there is one: along `path` as written up to `ceiling` as written, and
/// along their resolved forms. The resolved ceiling is the ceiling's parent
/// resolved plus its own name, so a symlink at or below it (the ceiling
/// itself, or a folder under it) that leads out from under it gets every
/// ancestor of where it leads checked: otherwise `extract/` linked into a
/// checkout would pass.
fn git_tree_within(path: &Path, ceiling: Option<&Path>) -> Option<PathBuf> {
    let has_git = |dir: &Path| dir.join(".git").symlink_metadata().is_ok();
    let find = |path: &Path, ceiling: Option<&Path>| {
        path.ancestors()
            .take_while(|a| ceiling.is_none_or(|c| a.starts_with(c)))
            .find(|a| has_git(a))
            .map(Path::to_path_buf)
    };
    find(path, ceiling).or_else(|| {
        let path = resolved(path)?;
        let ceiling = ceiling
            .and_then(|c| Some(resolved(c.parent()?)?.join(c.file_name()?)))
            .filter(|c| path.starts_with(c));
        find(&path, ceiling.as_deref())
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
/// dir is rebuilt. On unix, by (dev, inode) as well as by path: macOS
/// `realpath` keeps a firmlink spelling (`/System/Volumes/Data/...`), so two
/// spellings of one folder can differ as strings.
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let id = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.dev(), m.ino()));
        let dump_id = id(&d).ok_or_else(overlap)?;
        if r.ancestors().filter_map(id).any(|a| a == dump_id) {
            return Err(overlap());
        }
        if let Some(root_id) = id(&r)
            && d.ancestors().filter_map(id).any(|a| a == root_id)
        {
            return Err(overlap());
        }
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

    /// AB.18: release looks for a `.git` no higher than the data root, dev in
    /// every ancestor.
    #[test]
    fn release_stops_at_the_data_root_and_dev_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(home.join(".git")).unwrap();
        let data_root = home.join("Library/Application Support/guacamole");
        let release = || {
            ExtractRoot::resolve(RootMode::Release {
                data_root: data_root.clone(),
            })
        };
        assert_eq!(release().unwrap().path(), data_root.join("extract"));
        let dev = ExtractRoot::resolve(RootMode::Dev {
            extract_dir: Some(data_root.join("extract").into()),
        });
        match dev.unwrap_err() {
            ExtractError::InCheckout { repo, .. } => assert_eq!(repo, home),
            other => panic!("{other}"),
        }
        assert!(ExtractRoot::checked(data_root.join("extract")).is_err());

        for at in [data_root.clone(), data_root.join("extract")] {
            std::fs::create_dir_all(&at).unwrap();
            std::fs::write(at.join(".git"), "gitdir: elsewhere\n").unwrap();
            match release().unwrap_err() {
                ExtractError::InCheckout { repo, .. } => assert_eq!(repo, at),
                other => panic!("{other}"),
            }
            std::fs::remove_file(at.join(".git")).unwrap();
        }
        release().unwrap();
    }

    /// AB.18's review: the release walk stops at the data root only while the
    /// root's real folder is under the data root's. A symlink at or below the
    /// data root that leads elsewhere (`extract/` itself, or the data root)
    /// gets every ancestor checked, so neither can point into a checkout.
    #[cfg(unix)]
    #[test]
    fn a_release_root_symlinked_out_of_the_data_root_gets_the_full_walk() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let in_repo = repo.join("dev-data/guacamole/extract");
        std::fs::create_dir_all(&in_repo).unwrap();
        let repo_canon = repo.canonicalize().unwrap();
        let release = |data_root: &Path| {
            ExtractRoot::resolve(RootMode::Release {
                data_root: data_root.to_path_buf(),
            })
        };
        let refused_by_repo = |r: Result<ExtractRoot>| match r {
            Err(ExtractError::InCheckout { repo: found, .. }) => assert_eq!(found, repo_canon),
            other => panic!("{other:?}"),
        };

        // `extract/` a symlink into the checkout.
        let data_root = tmp.path().join("support/guacamole");
        std::fs::create_dir_all(&data_root).unwrap();
        symlink(&in_repo, data_root.join("extract")).unwrap();
        refused_by_repo(release(&data_root));

        // The data root itself a symlink into the checkout.
        let linked = tmp.path().join("support/linked");
        symlink(repo.join("dev-data/guacamole"), &linked).unwrap();
        refused_by_repo(release(&linked));

        // Swapped in after the resolve: refused at use.
        let later = tmp.path().join("support/later");
        std::fs::create_dir_all(&later).unwrap();
        let root = release(&later).unwrap();
        symlink(&in_repo, later.join("extract")).unwrap();
        refused_by_repo(root.ensure_outside_checkouts().map(|()| root.clone()));

        // A symlink out of the data root to a folder in no checkout is fine,
        // and so is one that stays under the data root with the home in git.
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let out = tmp.path().join("support/out");
        std::fs::create_dir_all(&out).unwrap();
        symlink(&elsewhere, out.join("extract")).unwrap();
        release(&out).unwrap();
        let home = tmp.path().join("home");
        let inner = home.join("Library/Application Support/guacamole");
        std::fs::create_dir_all(home.join(".git")).unwrap();
        std::fs::create_dir_all(inner.join("cache")).unwrap();
        symlink(inner.join("cache"), inner.join("extract")).unwrap();
        release(&inner).unwrap();
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
