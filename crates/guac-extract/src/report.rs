//! The unknown-dump report (design decision 4): `unknown-dump-<UTC stamp>.json`
//! beside the extract root, so two dumps diff by hand. Names, sizes and hashes,
//! never content.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use guac_ir::Sha256;
use serde::Serialize;

use crate::dump::DumpScan;
use crate::error::{ExtractError, Result};
use crate::known::KnownDump;

#[derive(Serialize)]
struct Report<'a> {
    code: &'static str,
    extractor: &'static str,
    dump_dir: String,
    title_id: &'a str,
    app_ver: &'a str,
    manifest_hash: Sha256,
    file_count: usize,
    total_bytes: u64,
    files: Vec<ReportFile<'a>>,
    /// What it was compared with.
    known: &'a [KnownDump],
}

#[derive(Serialize)]
struct ReportFile<'a> {
    path: &'a str,
    size: u64,
    sha256: Sha256,
}

pub(crate) fn write(
    dir: &Path,
    dump: &Path,
    scan: &DumpScan,
    known: &[KnownDump],
) -> Result<PathBuf> {
    let report = Report {
        code: "GUAC-E-UNKNOWN-DUMP",
        extractor: env!("CARGO_PKG_VERSION"),
        dump_dir: dump.to_string_lossy().into_owned(),
        title_id: &scan.identity.title_id,
        app_ver: &scan.identity.app_ver,
        manifest_hash: scan.identity.manifest_hash,
        file_count: scan.files.len(),
        total_bytes: scan.files.iter().map(|f| f.size).sum(),
        files: scan
            .files
            .iter()
            .map(|f| ReportFile {
                path: &f.path,
                size: f.size,
                sha256: f.sha256,
            })
            .collect(),
        known,
    };
    let mut text = serde_json::to_string_pretty(&report).expect("the report serializes");
    text.push('\n');

    std::fs::create_dir_all(dir).map_err(ExtractError::io(dir))?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    for n in 0u32.. {
        let name = match n {
            0 => format!("unknown-dump-{stamp}.json"),
            n => format!("unknown-dump-{stamp}-{n}.json"),
        };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(text.as_bytes())
                    .map_err(ExtractError::io(&path))?;
                return Ok(path);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(ExtractError::io(&path)(e)),
        }
    }
    unreachable!("u32 names run out")
}
