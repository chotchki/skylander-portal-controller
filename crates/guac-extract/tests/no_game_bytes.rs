//! The game-magic guard (openspec AB, design decision 10; the HARD rule in
//! docs/native/guacamole-design.md, "No game bytes in the repo"): fails when any
//! file git tracks starts with a game magic, is a gzip whose inflated head does
//! (RPCS3 writes RSX captures as `.rrc.gz`) or is a big-endian PPC64 ELF (a
//! decrypted EBOOT).
//!
//! It reads git's index, not the working tree, because the index is what the
//! next commit holds. A guest memory dump has no magic at all; only keeping it
//! outside the tree stops that one.
//!
//! Fixtures are built in code at test time in throwaway repos, so nothing here
//! trips the guard on this repo.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

/// Prefixes no file in this repo may start with.
const MAGICS: &[(&[u8], &str)] = &[
    (b"IGA\x1a", "IGA archive (Alchemy .arc/.bld/.pak)"),
    (b"IGZ", "IGZ object file"),
    (b"BIKi", "Bink 1 video"),
    (b"KB2", "Bink 2 video"),
    (b"FSB5", "FMOD sound bank"),
    (
        b"SCE\0",
        "SCE container (SELF/SPRX/PKG, an encrypted executable)",
    ),
    (b"NPD\0", "NPDRM EDAT/SDAT"),
    (b"\0PSF", "PARAM.SFO"),
    (b"RRC", "RPCS3 RSX capture"),
];

const EM_PPC64: u16 = 21;
const GZIP: &[u8] = &[0x1f, 0x8b];
/// Enough inflated bytes to hold any head above, and a nested gzip's header.
const INFLATE_HEAD: u64 = 4096;
/// gzip inside gzip inside gzip is as deep as the guard looks.
const MAX_GZIP_DEPTH: usize = 3;

fn is_ppc64_be_elf(head: &[u8]) -> bool {
    head.len() >= 20
        && head.starts_with(b"\x7fELF")
        && head[4] == 2 // ELFCLASS64
        && head[5] == 2 // ELFDATA2MSB
        && u16::from_be_bytes([head[18], head[19]]) == EM_PPC64
}

fn classify(head: &[u8]) -> Option<&'static str> {
    if let Some((_, what)) = MAGICS.iter().find(|(magic, _)| head.starts_with(magic)) {
        return Some(what);
    }
    is_ppc64_be_elf(head).then_some("big-endian PPC64 ELF (a decrypted EBOOT)")
}

/// What a blob would leak, or `None` when it's clean.
fn offence(blob: &[u8], depth: usize) -> Option<String> {
    if let Some(what) = classify(blob) {
        return Some(what.to_string());
    }
    if depth < MAX_GZIP_DEPTH && blob.starts_with(GZIP) {
        let mut head = Vec::new();
        // A truncated or corrupt stream keeps whatever inflated before the error.
        let _ = GzDecoder::new(blob)
            .take(INFLATE_HEAD)
            .read_to_end(&mut head);
        return offence(&head, depth + 1).map(|what| format!("gzip of a {what}"));
    }
    None
}

fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("the guard needs git on PATH");
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        repo.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// `(object id, path)` for every regular file in the index. Submodule
/// gitlinks (mode 160000) and symlinks (120000) hold no file bytes.
fn tracked_blobs(repo: &Path) -> Vec<(String, String)> {
    let listing = git(repo, &["ls-files", "--stage", "-z"]);
    listing
        .split(|b| *b == 0)
        .filter(|record| !record.is_empty())
        .filter_map(|record| {
            // "<mode> <oid> <stage>\t<path>"
            let record = String::from_utf8_lossy(record);
            let (meta, path) = record.split_once('\t')?;
            let mut meta = meta.split(' ');
            let (mode, oid) = (meta.next()?, meta.next()?);
            (mode != "160000" && mode != "120000").then(|| (oid.to_string(), path.to_string()))
        })
        .collect()
}

/// Every tracked file that starts with a game magic, as `(path, what)`.
fn scan(repo: &Path) -> Vec<(String, String)> {
    let blobs = tracked_blobs(repo);
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the guard needs git on PATH");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut findings = Vec::new();
    // One object per round trip: git flushes after each when --buffer is off,
    // so neither pipe fills up and deadlocks.
    for (oid, path) in blobs {
        writeln!(stdin, "{oid}").unwrap();
        stdin.flush().unwrap();
        let mut header = String::new();
        stdout.read_line(&mut header).unwrap();
        // "<oid> blob <size>", or "<oid> missing" for an intent-to-add entry.
        let mut fields = header.split_whitespace().skip(1);
        if fields.next() != Some("blob") {
            continue;
        }
        let size: usize = fields.next().and_then(|s| s.parse().ok()).unwrap();
        let mut blob = vec![0; size + 1]; // + the newline after each object
        stdout.read_exact(&mut blob).unwrap();
        blob.pop();
        if let Some(what) = offence(&blob, 0) {
            findings.push((path, what));
        }
    }
    drop(stdin);
    assert!(child.wait().unwrap().success(), "git cat-file failed");
    findings
}

#[test]
fn tracked_files_carry_no_game_bytes() {
    let top = git(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &["rev-parse", "--show-toplevel"],
    );
    let top = String::from_utf8(top).unwrap();
    let findings = scan(Path::new(top.trim()));
    assert!(
        findings.is_empty(),
        "game bytes are tracked (HARD rule, docs/native/guacamole-design.md): \
         `git rm --cached` each, and purge any commit that carried one\n{}",
        findings
            .iter()
            .map(|(path, what)| format!("  {path}: {what}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// The guard tested on throwaway repos.

/// A fresh repo with these files staged (`git add` is enough for ls-files).
fn repo_with(files: &[(&str, Vec<u8>)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    for (path, bytes) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    git(dir.path(), &["add", "-A"]);
    dir
}

/// A magic followed by zeros, so git sees a binary file.
fn starting_with(magic: &[u8]) -> Vec<u8> {
    let mut bytes = magic.to_vec();
    bytes.resize(64, 0);
    bytes
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(bytes).unwrap();
    enc.finish().unwrap()
}

/// An ELF header: `class` 1 = 32-bit, 2 = 64-bit; `data` 1 = little-endian,
/// 2 = big-endian.
fn elf(class: u8, data: u8, machine: u16) -> Vec<u8> {
    let mut head = vec![0u8; 64];
    head[..4].copy_from_slice(b"\x7fELF");
    head[4] = class;
    head[5] = data;
    head[6] = 1;
    let machine = if data == 2 {
        machine.to_be_bytes()
    } else {
        machine.to_le_bytes()
    };
    head[18..20].copy_from_slice(&machine);
    head
}

fn paths(findings: &[(String, String)]) -> Vec<&str> {
    findings.iter().map(|(path, _)| path.as_str()).collect()
}

#[test]
fn flags_a_file_starting_with_iga() {
    let repo = repo_with(&[("assets/a.arc", starting_with(b"IGA\x1a"))]);
    let findings = scan(repo.path());
    assert_eq!(paths(&findings), ["assets/a.arc"]);
    assert!(findings[0].1.contains("IGA archive"), "{findings:?}");
}

#[test]
fn flags_a_gzip_whose_inflated_head_is_rrc() {
    let repo = repo_with(&[("capture.rrc.gz", gzip(&starting_with(b"RRC")))]);
    let findings = scan(repo.path());
    assert_eq!(paths(&findings), ["capture.rrc.gz"]);
    assert_eq!(findings[0].1, "gzip of a RPCS3 RSX capture");
}

#[test]
fn flags_a_big_endian_ppc64_elf() {
    let repo = repo_with(&[("EBOOT.elf", elf(2, 2, EM_PPC64))]);
    let findings = scan(repo.path());
    assert_eq!(paths(&findings), ["EBOOT.elf"]);
    assert!(findings[0].1.contains("PPC64"), "{findings:?}");
}

#[test]
fn flags_every_listed_magic() {
    let names: Vec<String> = (0..MAGICS.len()).map(|i| format!("f{i}")).collect();
    let files: Vec<(&str, Vec<u8>)> = names
        .iter()
        .zip(MAGICS)
        .map(|(name, (magic, _))| (name.as_str(), starting_with(magic)))
        .collect();
    assert_eq!(scan(repo_with(&files).path()).len(), MAGICS.len());
}

#[test]
fn flags_nested_gzip() {
    let repo = repo_with(&[("x.gz.gz", gzip(&gzip(&starting_with(b"FSB5"))))]);
    let findings = scan(repo.path());
    assert_eq!(paths(&findings), ["x.gz.gz"]);
    assert_eq!(findings[0].1, "gzip of a gzip of a FMOD sound bank");
}

#[test]
fn reads_the_index_not_the_working_tree() {
    let repo = repo_with(&[("staged.bin", starting_with(b"BIKi"))]);
    fs::write(
        repo.path().join("staged.bin"),
        b"clean now, but not in the index",
    )
    .unwrap();
    assert_eq!(paths(&scan(repo.path())), ["staged.bin"]);
}

#[test]
fn passes_a_repo_with_none_of_them() {
    let repo = repo_with(&[
        (
            "README.md",
            b"docs name IGA\x1a and RRC mid-file\n".to_vec(),
        ),
        ("notes.txt.gz", gzip(b"plain text, gzipped")),
        ("broken.gz", vec![0x1f, 0x8b, 0, 0]),
        ("host.elf", elf(2, 1, 62)),    // little-endian x86-64
        ("ppc32.elf", elf(1, 2, 20)),   // big-endian 32-bit PPC
        ("sparc64.elf", elf(2, 2, 43)), // big-endian 64-bit, not PPC
        ("empty", Vec::new()),
        ("nested/dir/data.bin", vec![0xde, 0xad, 0xbe, 0xef]),
    ]);
    // Untracked files aren't the guard's business: nothing pushes them.
    fs::write(repo.path().join("untracked.arc"), starting_with(b"IGA\x1a")).unwrap();
    assert_eq!(scan(repo.path()), Vec::<(String, String)>::new());
}
