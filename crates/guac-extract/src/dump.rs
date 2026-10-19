//! Reading a dump's identity (design decision 4). Every file is opened
//! read-only and nothing here takes a path to write.
//!
//! The manifest hash is SHA-256 over every file under `PS3_GAME/USRDIR` but
//! the top-level `EBOOT.BIN`, sorted by relative path (`/`-separated, bytewise),
//! each fed as `path`, a NUL, `size` as u64 little-endian, then the file's own
//! 32-byte SHA-256. Paths can't hold a NUL, so the encoding is unambiguous.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

use guac_formats::sfo::ParamSfo;
use guac_ir::{DumpIdentity, Sha256};
use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::error::{ExtractError, Result};

/// Real PARAM.SFO files are about 1 KiB.
const MAX_SFO_LEN: u64 = 64 * 1024;
/// The encrypted executable, the one file the hash leaves out.
const EBOOT: &str = "EBOOT.BIN";
const READ_BUF: usize = 1 << 20;

/// One hashed dump file, as the manifest records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    /// Relative to `USRDIR`, `/`-separated.
    pub path: String,
    pub size: u64,
    /// Nanoseconds since the epoch; `None` when the filesystem gave none, and
    /// then the cache never matches.
    pub mtime_ns: Option<u64>,
    /// Inode and status-change time (ns). A disc's dumps all carry its fixed
    /// mtimes, and copy tools restore them, but no write can set ctime back.
    /// `None` off unix (until the Windows bring-up) or in an older manifest:
    /// the cache never matches.
    #[serde(default)]
    pub ino: Option<u64>,
    #[serde(default)]
    pub ctime_ns: Option<u64>,
    pub sha256: Sha256,
}

/// Earlier hashes by relative path, from previous manifests of the same dump.
/// A file whose size, mtime, inode and ctime all match isn't read again.
#[derive(Debug, Default)]
pub struct HashCache(HashMap<String, SourceFile>);

impl HashCache {
    pub fn extend(&mut self, files: impl IntoIterator<Item = SourceFile>) {
        self.0
            .extend(files.into_iter().map(|f| (f.path.clone(), f)));
    }

    fn hit(&self, l: &Listed) -> Option<Sha256> {
        let f = self.0.get(&l.rel)?;
        let stamped = l.mtime_ns.is_some() && l.ino.is_some() && l.ctime_ns.is_some();
        let same = f.size == l.size
            && f.mtime_ns == l.mtime_ns
            && f.ino == l.ino
            && f.ctime_ns == l.ctime_ns;
        (stamped && same).then_some(f.sha256)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HashProgress {
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// A dump read and hashed.
#[derive(Debug, Clone)]
pub struct DumpScan {
    pub identity: DumpIdentity,
    /// Sorted by path, the hash's order.
    pub files: Vec<SourceFile>,
    /// Bytes actually read this time (cache misses).
    pub bytes_hashed: u64,
    /// Files the cache answered.
    pub files_cached: usize,
}

/// TITLE_ID and APP_VER from `<dump>/PS3_GAME/PARAM.SFO`.
pub fn read_sfo(dump: &Path) -> Result<(String, String)> {
    let path = dump.join("PS3_GAME").join("PARAM.SFO");
    let file = File::open(&path).map_err(|e| ExtractError::unreadable(&path, e))?;
    let mut bytes = Vec::new();
    file.take(MAX_SFO_LEN + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ExtractError::unreadable(&path, e))?;
    if bytes.len() as u64 > MAX_SFO_LEN {
        return Err(ExtractError::unreadable(&path, "larger than a PARAM.SFO"));
    }
    let sfo = ParamSfo::parse(&bytes).map_err(|e| ExtractError::unreadable(&path, e))?;
    let get = |key, value: Option<&str>| {
        value
            .map(str::to_owned)
            .ok_or_else(|| ExtractError::unreadable(&path, format!("no {key}")))
    };
    Ok((
        get("TITLE_ID", sfo.title_id())?,
        get("APP_VER", sfo.app_ver())?,
    ))
}

struct Listed {
    rel: String,
    abs: PathBuf,
    size: u64,
    mtime_ns: Option<u64>,
    ino: Option<u64>,
    ctime_ns: Option<u64>,
}

#[cfg(unix)]
fn ino_ctime(meta: &fs::Metadata) -> (Option<u64>, Option<u64>) {
    use std::os::unix::fs::MetadataExt;
    let ns = i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec());
    (Some(meta.ino()), u64::try_from(ns).ok())
}

#[cfg(not(unix))]
fn ino_ctime(_: &fs::Metadata) -> (Option<u64>, Option<u64>) {
    (None, None)
}

/// Every file under `usrdir` but the EBOOT, sorted by relative path.
fn list(usrdir: &Path) -> Result<Vec<Listed>> {
    let mut out = Vec::new();
    let mut stack = vec![(usrdir.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let entries = fs::read_dir(&dir).map_err(|e| ExtractError::unreadable(&dir, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| ExtractError::unreadable(&dir, e))?;
            let abs = entry.path();
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| ExtractError::unreadable(&abs, "a file name that isn't UTF-8"))?;
            let rel = format!("{prefix}{name}");
            // symlink_metadata: a link is refused, never followed out of the dump.
            let meta = fs::symlink_metadata(&abs).map_err(|e| ExtractError::unreadable(&abs, e))?;
            if meta.is_dir() {
                stack.push((abs, format!("{rel}/")));
            } else if meta.is_file() {
                if rel == EBOOT {
                    continue;
                }
                let mtime_ns = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .and_then(|d| u64::try_from(d.as_nanos()).ok());
                let (ino, ctime_ns) = ino_ctime(&meta);
                out.push(Listed {
                    rel,
                    abs,
                    size: meta.len(),
                    mtime_ns,
                    ino,
                    ctime_ns,
                });
            } else {
                return Err(ExtractError::unreadable(
                    &abs,
                    "not a regular file or folder",
                ));
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

fn hash_file(path: &Path, expected: u64, buf: &mut [u8]) -> Result<Sha256> {
    let mut file = File::open(path).map_err(|e| ExtractError::unreadable(path, e))?;
    let mut hasher = sha2::Sha256::new();
    let mut read = 0u64;
    loop {
        let n = file
            .read(buf)
            .map_err(|e| ExtractError::unreadable(path, e))?;
        if n == 0 {
            break;
        }
        read += n as u64;
        hasher.update(&buf[..n]);
    }
    if read != expected {
        return Err(ExtractError::unreadable(
            path,
            format!("changed while hashing ({expected} bytes listed, {read} read)"),
        ));
    }
    Ok(Sha256(hasher.finalize().into()))
}

/// The hash over the sorted records (the module docs give the encoding).
pub fn manifest_hash(files: &[SourceFile]) -> Sha256 {
    let mut hasher = sha2::Sha256::new();
    for f in files {
        hasher.update(f.path.as_bytes());
        hasher.update([0]);
        hasher.update(f.size.to_le_bytes());
        hasher.update(f.sha256.0);
    }
    Sha256(hasher.finalize().into())
}

/// [`scan`] when `cache` answers for every file, reading none of them; `None`
/// when one would have to be hashed (new, changed or never extracted).
pub fn scan_cached(dump: &Path, cache: &HashCache) -> Result<Option<DumpScan>> {
    let (title_id, app_ver) = read_sfo(dump)?;
    let listed = list(&dump.join("PS3_GAME").join("USRDIR"))?;
    let mut files = Vec::with_capacity(listed.len());
    for l in listed {
        let Some(sha256) = cache.hit(&l) else {
            return Ok(None);
        };
        files.push(SourceFile {
            path: l.rel,
            size: l.size,
            mtime_ns: l.mtime_ns,
            ino: l.ino,
            ctime_ns: l.ctime_ns,
            sha256,
        });
    }
    Ok(Some(DumpScan {
        identity: DumpIdentity {
            title_id,
            app_ver,
            manifest_hash: manifest_hash(&files),
        },
        files_cached: files.len(),
        files,
        bytes_hashed: 0,
    }))
}

/// Reads the dump's PARAM.SFO and hashes its USRDIR, reading only the files
/// `cache` can't answer, on a few threads.
pub fn scan(
    dump: &Path,
    cache: &HashCache,
    progress: &(dyn Fn(HashProgress) + Sync),
) -> Result<DumpScan> {
    let (title_id, app_ver) = read_sfo(dump)?;
    let usrdir = dump.join("PS3_GAME").join("USRDIR");
    let listed = list(&usrdir)?;

    let mut hashes: Vec<Option<Sha256>> = listed.iter().map(|l| cache.hit(l)).collect();
    let files_cached = hashes.iter().flatten().count();
    let todo: Vec<usize> = (0..listed.len()).filter(|&i| hashes[i].is_none()).collect();

    let totals = HashProgress {
        files_done: files_cached,
        files_total: listed.len(),
        bytes_done: 0,
        bytes_total: todo.iter().map(|&i| listed[i].size).sum(),
    };
    progress(totals);
    let next = AtomicUsize::new(0);
    let files_done = AtomicUsize::new(files_cached);
    let bytes_done = AtomicU64::new(0);
    let results = Mutex::new(Vec::with_capacity(todo.len()));
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, 4)
        .min(todo.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut buf = vec![0u8; READ_BUF];
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&i) = todo.get(k) else { break };
                    let l = &listed[i];
                    let r = hash_file(&l.abs, l.size, &mut buf);
                    let failed = r.is_err();
                    results.lock().unwrap().push((i, r));
                    if failed {
                        // Stop handing out work; the first error is returned.
                        next.store(todo.len(), Ordering::Relaxed);
                        break;
                    }
                    progress(HashProgress {
                        files_done: files_done.fetch_add(1, Ordering::Relaxed) + 1,
                        bytes_done: bytes_done.fetch_add(l.size, Ordering::Relaxed) + l.size,
                        ..totals
                    });
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(i, _)| *i);
    for (i, r) in results {
        hashes[i] = Some(r?);
    }

    let files: Vec<SourceFile> = listed
        .into_iter()
        .zip(hashes)
        .map(|(l, h)| SourceFile {
            path: l.rel,
            size: l.size,
            mtime_ns: l.mtime_ns,
            ino: l.ino,
            ctime_ns: l.ctime_ns,
            sha256: h.expect("every listed file is hashed or cached"),
        })
        .collect();
    Ok(DumpScan {
        identity: DumpIdentity {
            title_id,
            app_ver,
            manifest_hash: manifest_hash(&files),
        },
        files,
        bytes_hashed: totals.bytes_total,
        files_cached,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(path: &str, size: u64, byte: u8) -> SourceFile {
        SourceFile {
            path: path.into(),
            size,
            mtime_ns: Some(1),
            ino: Some(10),
            ctime_ns: Some(100),
            sha256: Sha256([byte; 32]),
        }
    }

    fn listed(path: &str, size: u64, mtime: u64, ino: u64, ctime: u64) -> Listed {
        Listed {
            rel: path.into(),
            abs: PathBuf::new(),
            size,
            mtime_ns: Some(mtime),
            ino: Some(ino),
            ctime_ns: Some(ctime),
        }
    }

    #[test]
    fn the_hash_covers_path_size_and_content_but_not_mtime() {
        let base = vec![rec("a", 1, 1), rec("b/c", 2, 2)];
        let h = manifest_hash(&base);
        let mut renamed = base.clone();
        renamed[1].path = "b/d".into();
        let mut resized = base.clone();
        resized[0].size = 9;
        let mut rehashed = base.clone();
        rehashed[0].sha256 = Sha256([7; 32]);
        let mut touched = base.clone();
        touched[0].mtime_ns = Some(99);
        assert_ne!(manifest_hash(&renamed), h);
        assert_ne!(manifest_hash(&resized), h);
        assert_ne!(manifest_hash(&rehashed), h);
        assert_eq!(manifest_hash(&touched), h);
    }

    #[test]
    fn the_cache_needs_size_mtime_inode_and_ctime_to_match() {
        let mut cache = HashCache::default();
        cache.extend([rec("a", 1, 1)]);
        assert_eq!(
            cache.hit(&listed("a", 1, 1, 10, 100)),
            Some(Sha256([1; 32]))
        );
        assert_eq!(cache.hit(&listed("a", 2, 1, 10, 100)), None);
        assert_eq!(cache.hit(&listed("a", 1, 2, 10, 100)), None);
        assert_eq!(cache.hit(&listed("a", 1, 1, 11, 100)), None);
        assert_eq!(cache.hit(&listed("a", 1, 1, 10, 101)), None);
        assert_eq!(cache.hit(&listed("b", 1, 1, 10, 100)), None);
        let unstamped = |f: fn(&mut Listed)| {
            let mut l = listed("a", 1, 1, 10, 100);
            f(&mut l);
            cache.hit(&l)
        };
        assert_eq!(unstamped(|l| l.mtime_ns = None), None);
        assert_eq!(unstamped(|l| l.ino = None), None);
        assert_eq!(unstamped(|l| l.ctime_ns = None), None);
    }

    #[test]
    fn a_manifest_without_inode_or_ctime_reads_and_misses() {
        let old = r#"{"path":"a","size":1,"mtime_ns":1,"sha256":"0101010101010101010101010101010101010101010101010101010101010101"}"#;
        let f: SourceFile = serde_json::from_str(old).unwrap();
        assert_eq!((f.ino, f.ctime_ns), (None, None));
        let mut cache = HashCache::default();
        cache.extend([f]);
        assert_eq!(cache.hit(&listed("a", 1, 1, 10, 100)), None);
    }
}
