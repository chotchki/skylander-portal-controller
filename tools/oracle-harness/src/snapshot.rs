//! Proof that an oracle run left the dev's own RPCS3 alone: every file under the
//! dev's RPCS3 config and cache dirs with its size and mtime, taken before a run
//! and compared after it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;

use crate::home::{rpcs3_cache_dir, rpcs3_config_dir};

/// The two dirs RPCS3 writes under the dev's real HOME.
pub fn dev_rpcs3_dirs(real_home: &Path) -> [PathBuf; 2] {
    [rpcs3_config_dir(real_home), rpcs3_cache_dir(real_home)]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot(BTreeMap<PathBuf, (u64, Option<SystemTime>)>);

impl Snapshot {
    /// Walks `dirs` without following links (a link is listed, not entered). A
    /// missing dir lists as empty.
    pub fn take(dirs: &[PathBuf]) -> Result<Self> {
        let mut files = BTreeMap::new();
        for dir in dirs {
            walk(dir, &mut files)?;
        }
        Ok(Self(files))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// What a run touched, as the report a failed check prints; empty when nothing.
    pub fn diff(&self, after: &Snapshot) -> Vec<String> {
        let mut out = Vec::new();
        for (path, before) in &self.0 {
            match after.0.get(path) {
                None => out.push(format!("removed {}", path.display())),
                Some(now) if now != before => out.push(format!("changed {}", path.display())),
                Some(_) => {}
            }
        }
        for path in after.0.keys().filter(|p| !self.0.contains_key(*p)) {
            out.push(format!("added {}", path.display()));
        }
        out
    }
}

fn walk(path: &Path, out: &mut BTreeMap<PathBuf, (u64, Option<SystemTime>)>) -> Result<()> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    out.insert(path.to_path_buf(), (meta.len(), meta.modified().ok()));
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            walk(&entry?.path(), out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_anywhere_under_the_dirs_shows_up() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = dev_rpcs3_dirs(tmp.path());
        fs::create_dir_all(dirs[0].join("dev_hdd0")).unwrap();
        fs::write(dirs[0].join("dev_hdd0/save"), "a").unwrap();
        fs::write(dirs[0].join("config.yml"), "a").unwrap();
        let before = Snapshot::take(&dirs).unwrap();
        assert_eq!(before.len(), 4); // the dir, dev_hdd0, two files; no cache dir
        assert_eq!(
            before.diff(&Snapshot::take(&dirs).unwrap()),
            Vec::<String>::new()
        );

        fs::write(dirs[0].join("dev_hdd0/save"), "ab").unwrap();
        fs::remove_file(dirs[0].join("config.yml")).unwrap();
        fs::create_dir_all(&dirs[1]).unwrap();
        let diff = before.diff(&Snapshot::take(&dirs).unwrap());
        let has = |prefix: &str, end: &str| {
            diff.iter()
                .any(|l| l.starts_with(prefix) && l.ends_with(end))
        };
        assert!(has("changed", "save"), "{diff:?}");
        assert!(has("removed", "config.yml"), "{diff:?}");
        assert!(has("added", "rpcs3"), "{diff:?}");
    }
}
