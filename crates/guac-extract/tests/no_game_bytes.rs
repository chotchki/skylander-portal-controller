//! The game-magic guard (openspec AB, design decision 10; the HARD rule in
//! docs/native/guacamole-design.md, "No game bytes in the repo"): fails when any
//! file git would commit or push starts with a game magic, is a gzip whose
//! inflated head does (RPCS3 writes RSX captures as `.rrc.gz`) or is a
//! big-endian PPC64 ELF (a decrypted EBOOT).
//!
//! It reads git, not the working tree: the index, because that's what the next
//! commit holds, plus every blob reachable from HEAD or a local branch that no
//! remote-tracking ref reaches yet, because that's what a push can send (a file
//! committed and then deleted still ships with its commit). A guest memory dump
//! has no magic at all; only keeping it outside the tree stops that one.
//!
//! The magic list and `classify` live in `guac_extract::magic`, shared with
//! the census (AB.5). Fixtures are built in code at test time in throwaway
//! repos, so nothing here trips the guard on this repo.

use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use guac_extract::magic::{EM_PPC64, GAME_MAGICS, classify};

const GZIP: &[u8] = &[0x1f, 0x8b];
/// How much of a blob, or of an inflated gzip, the guard reads: every check
/// needs only the first bytes, and 64 KiB covers a gzip header's name and
/// comment. A staged multi-GB disc image costs this, not its size.
const HEAD: u64 = 64 * 1024;
/// gzip inside gzip inside gzip is as deep as the guard looks.
const MAX_GZIP_DEPTH: usize = 3;

/// What a blob's head would leak, or `None` when it's clean.
fn offence(head: &[u8], depth: usize) -> Option<String> {
    if let Some(what) = classify(head) {
        return Some(what.to_string());
    }
    if depth < MAX_GZIP_DEPTH && head.starts_with(GZIP) {
        let mut inflated = Vec::new();
        // A truncated or corrupt stream keeps whatever inflated before the error.
        let _ = GzDecoder::new(head).take(HEAD).read_to_end(&mut inflated);
        return offence(&inflated, depth + 1).map(|what| format!("gzip of a {what}"));
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

/// `(object id, path)` for every regular file in the index, then for every blob
/// HEAD or a local branch reaches and no remote-tracking ref does, each object
/// once. Submodule gitlinks (mode 160000) and symlinks (120000) in the index
/// hold no file bytes.
fn blobs_to_check(repo: &Path) -> Vec<(String, String)> {
    let listing = git(repo, &["ls-files", "--stage", "-z"]);
    let mut blobs: Vec<(String, String)> = listing
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
        .collect();

    // HEAD covers a detached checkout (CI's); a repo with only `git add`
    // behind it has none, and naming it there would fail.
    let has_head = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify", "-q", "HEAD"])
        .output()
        .is_ok_and(|out| out.status.success());
    let mut rev_list = vec!["rev-list", "--objects", "--filter=object:type=blob"];
    if has_head {
        rev_list.push("HEAD");
    }
    rev_list.extend(["--branches", "--not", "--remotes"]);
    let history = git(repo, &rev_list);
    // "<oid> <path>" per blob; a commit's line is its bare oid.
    blobs.extend(
        String::from_utf8_lossy(&history)
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(oid, path)| (oid.to_string(), path.to_string())),
    );

    let mut seen = HashSet::new();
    blobs.retain(|(oid, _)| seen.insert(oid.clone()));
    blobs
}

/// Every blob to check that starts with a game magic, as `(path, what)`.
fn scan(repo: &Path) -> Vec<(String, String)> {
    let blobs = blobs_to_check(repo);
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
        // "<oid> <type> <size>", or "<oid> missing" (an intent-to-add entry)
        // with no body after it.
        let mut fields = header.split_whitespace().skip(1);
        let (Some(kind), Some(size)) = (fields.next(), fields.next()) else {
            continue;
        };
        let size: u64 = size.parse().unwrap();
        // Only the head decides; the rest is drained unread.
        let mut head = Vec::new();
        (&mut stdout)
            .take(size.min(HEAD))
            .read_to_end(&mut head)
            .unwrap();
        let rest = size - head.len() as u64 + 1; // + the newline after each object
        assert_eq!(
            io::copy(&mut (&mut stdout).take(rest), &mut io::sink()).unwrap(),
            rest,
            "git cat-file cut {oid} short"
        );
        if kind != "blob" {
            continue;
        }
        if let Some(what) = offence(&head, 0) {
            findings.push((path, what));
        }
    }
    drop(stdin);
    assert!(child.wait().unwrap().success(), "git cat-file failed");
    findings
}

#[test]
fn the_index_and_unpushed_history_carry_no_game_bytes() {
    let top = git(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &["rev-parse", "--show-toplevel"],
    );
    let top = String::from_utf8(top).unwrap();
    let findings = scan(Path::new(top.trim()));
    assert!(
        findings.is_empty(),
        "game bytes would be committed or pushed (HARD rule, docs/native/guacamole-design.md): \
         `git rm --cached` each, and rewrite every unpushed commit that carried one before \
         pushing\n{}",
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

/// Commits what's staged, whoever runs the test and whatever their git config.
fn commit(repo: &Path, message: &str) {
    git(
        repo,
        &[
            "-c",
            "user.name=guard",
            "-c",
            "user.email=guard@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "--no-verify",
            "--allow-empty",
            "-m",
            message,
        ],
    );
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
fn flags_a_big_endian_iga() {
    // Every .pak on Swap Force, SuperChargers and Imaginators starts this way.
    let repo = repo_with(&[("a.pak", starting_with(b"\x1aAGI"))]);
    let findings = scan(repo.path());
    assert_eq!(paths(&findings), ["a.pak"]);
    assert!(findings[0].1.contains("big-endian"), "{findings:?}");
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
    let names: Vec<String> = (0..GAME_MAGICS.len()).map(|i| format!("f{i}")).collect();
    let files: Vec<(&str, Vec<u8>)> = names
        .iter()
        .zip(GAME_MAGICS)
        .map(|(name, (magic, _))| (name.as_str(), starting_with(magic)))
        .collect();
    assert_eq!(scan(repo_with(&files).path()).len(), GAME_MAGICS.len());
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
fn reads_blobs_bigger_than_the_head_and_stays_in_step() {
    // Each big blob is drained past its head; if it weren't, the next object's
    // header would be read out of the middle of it.
    let mut big_clean = vec![0u8; 3 * HEAD as usize];
    big_clean[..5].copy_from_slice(b"plain");
    let mut big_bink = starting_with(b"BIKi");
    big_bink.resize(3 * HEAD as usize + 7, 0xaa);
    let repo = repo_with(&[
        ("a.bin", big_clean),
        ("b.bik", big_bink),
        ("c.arc", starting_with(b"IGA\x1a")),
    ]);
    assert_eq!(paths(&scan(repo.path())), ["b.bik", "c.arc"]);
}

#[test]
fn flags_a_file_committed_then_deleted() {
    // The index is clean again, but the push would still send commit A.
    let repo = repo_with(&[
        ("README.md", b"hi\n".to_vec()),
        ("x.arc", starting_with(b"IGA\x1a")),
    ]);
    commit(repo.path(), "A");
    git(repo.path(), &["rm", "-q", "x.arc"]);
    commit(repo.path(), "B");
    assert_eq!(paths(&scan(repo.path())), ["x.arc"]);
}

#[test]
fn flags_a_committed_file_only_unstaged() {
    // `git rm --cached` alone cleans the index, not HEAD.
    let repo = repo_with(&[("x.arc", starting_with(b"IGA\x1a"))]);
    commit(repo.path(), "A");
    git(repo.path(), &["rm", "-q", "--cached", "x.arc"]);
    assert_eq!(paths(&scan(repo.path())), ["x.arc"]);
}

#[test]
fn flags_a_blob_on_another_local_branch() {
    // HEAD never had it, but `git push origin side` or `--all` would send it.
    let repo = repo_with(&[("README.md", b"hi\n".to_vec())]);
    commit(repo.path(), "base");
    git(repo.path(), &["checkout", "-q", "-b", "side"]);
    fs::write(repo.path().join("x.bik"), starting_with(b"BIKi")).unwrap();
    git(repo.path(), &["add", "x.bik"]);
    commit(repo.path(), "side");
    git(repo.path(), &["checkout", "-q", "-"]);
    assert_eq!(paths(&scan(repo.path())), ["x.bik"]);
}

#[test]
fn leaves_history_a_remote_already_has() {
    // Already pushed is past what a guard can stop, and walking it would make
    // every run cost the whole history.
    let repo = repo_with(&[("x.arc", starting_with(b"IGA\x1a"))]);
    commit(repo.path(), "A");
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git(repo.path(), &["rm", "-q", "x.arc"]);
    commit(repo.path(), "B");
    assert_eq!(scan(repo.path()), Vec::<(String, String)>::new());
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
    // Committed, so the history walk runs too and finds the same clean blobs.
    commit(repo.path(), "clean");
    // Untracked files aren't the guard's business: nothing pushes them.
    fs::write(repo.path().join("untracked.arc"), starting_with(b"IGA\x1a")).unwrap();
    assert_eq!(scan(repo.path()), Vec::<(String, String)>::new());
}
