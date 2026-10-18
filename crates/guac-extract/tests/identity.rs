//! Identity, the manifest and the extract root (openspec AB.4) on synthetic
//! dumps built in tempdirs: a PARAM.SFO from the test builder and a few USRDIR
//! files of made-up bytes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use guac_extract::dump::{self, HashCache};
use guac_extract::known::KnownDump;
use guac_extract::{DirState, ExtractError, ExtractRoot, IR_VERSION, LAYOUT_VERSION, Stale, open};
use guac_formats::sfo::builder::SfoBuilder;
use guac_ir::Sha256;

const MARKER: &str = "synthetic-content-not-for-reports";

/// `<tmp>/dump/PS3_GAME/{PARAM.SFO,USRDIR/...}`.
fn make_dump(tmp: &Path, app_ver: &str) -> PathBuf {
    let dump = tmp.join("dump");
    let game = dump.join("PS3_GAME");
    let usr = game.join("USRDIR");
    for d in ["character", "movies", "level/sub"] {
        fs::create_dir_all(usr.join(d)).unwrap();
    }
    fs::write(
        game.join("PARAM.SFO"),
        SfoBuilder::disc_game("BLUS30968", app_ver, "01.00").build(),
    )
    .unwrap();
    fs::write(usr.join("EBOOT.BIN"), b"not really an executable").unwrap();
    fs::write(usr.join("character/009_x.arc"), format!("{MARKER} arc")).unwrap();
    fs::write(usr.join("movies/x.mov"), vec![7u8; 3000]).unwrap();
    fs::write(usr.join("level/sub/EBOOT.BIN"), b"nested, so hashed").unwrap();
    fs::write(usr.join("level/sub/empty"), b"").unwrap();
    dump
}

fn no_progress(_: dump::HashProgress) {}

fn scan(dump: &Path) -> dump::DumpScan {
    dump::scan(dump, &HashCache::default(), &no_progress).unwrap()
}

/// A known table holding exactly this dump at `app_ver`.
fn table_for(dump: &Path, app_ver: &'static str) -> Vec<KnownDump> {
    let hash = scan(dump).identity.manifest_hash.to_string();
    vec![KnownDump {
        title_id: "BLUS30968",
        app_ver,
        manifest_hash: Box::leak(hash.into_boxed_str()),
        source: "test",
    }]
}

fn root_in(tmp: &Path) -> ExtractRoot {
    ExtractRoot::checked(tmp.join("out/extract")).unwrap()
}

fn reports(root: &ExtractRoot) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root.report_dir()) else {
        return Vec::new();
    };
    entries
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("unknown-dump-")
        })
        .collect()
}

#[test]
fn identity_is_stable_and_changes_when_one_file_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), "01.00");
    let a = scan(&dump);
    let b = scan(&dump);
    assert_eq!(a.identity, b.identity);
    assert_eq!(a.identity.title_id, "BLUS30968");
    assert_eq!(a.identity.app_ver, "01.00");
    let paths: Vec<_> = a.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "character/009_x.arc",
            "level/sub/EBOOT.BIN",
            "level/sub/empty",
            "movies/x.mov"
        ]
    );

    // The top-level EBOOT is left out.
    let usr = dump.join("PS3_GAME/USRDIR");
    fs::write(usr.join("EBOOT.BIN"), b"a different executable").unwrap();
    assert_eq!(scan(&dump).identity, a.identity);

    // One byte, same size.
    let mut bytes = fs::read(usr.join("movies/x.mov")).unwrap();
    bytes[1500] ^= 1;
    fs::write(usr.join("movies/x.mov"), &bytes).unwrap();
    let c = scan(&dump);
    assert_eq!(c.identity.title_id, a.identity.title_id);
    assert_ne!(c.identity.manifest_hash, a.identity.manifest_hash);
}

#[test]
fn a_cached_start_reads_only_what_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), "01.00");
    let table = table_for(&dump, "01.00");
    let root = root_in(tmp.path());
    let first = open(&dump, &root, &table, &no_progress).unwrap();
    assert_eq!((first.files_hashed, first.files_cached), (4, 0));
    assert_eq!(first.state, DirState::Fresh);
    let second = open(&dump, &root, &table, &no_progress).unwrap();
    assert_eq!((second.files_hashed, second.files_cached), (0, 4));
    assert_eq!(second.bytes_hashed, 0);
    assert_eq!(
        second.dir.manifest().identity,
        first.dir.manifest().identity
    );

    // A changed file with a new mtime is read again, and the dump is now unknown.
    let f = dump.join("PS3_GAME/USRDIR/character/009_x.arc");
    fs::write(&f, format!("{MARKER} arc, changed")).unwrap();
    let later = SystemTime::now() + Duration::from_secs(5);
    fs::File::options()
        .write(true)
        .open(&f)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let err = open(&dump, &root, &table, &no_progress).unwrap_err();
    assert!(matches!(err, ExtractError::UnknownDump { .. }), "{err}");
}

#[test]
fn an_unknown_dump_writes_a_report_and_extracts_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), "01.00");
    let root = root_in(tmp.path());
    let expected = scan(&dump);

    let err = open(&dump, &root, &[], &no_progress).unwrap_err();
    assert_eq!(err.code(), "GUAC-E-UNKNOWN-DUMP");
    let ExtractError::UnknownDump { identity, report } = err else {
        unreachable!()
    };
    assert_eq!(identity, expected.identity);
    assert_eq!(reports(&root), std::slice::from_ref(&report));
    assert_eq!(report.parent().unwrap(), root.report_dir());
    assert!(
        !root.path().exists(),
        "nothing extracted, not even the root"
    );

    let text = fs::read_to_string(&report).unwrap();
    assert!(!text.contains(MARKER), "a report holds no content");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["code"], "GUAC-E-UNKNOWN-DUMP");
    assert_eq!(json["title_id"], "BLUS30968");
    assert_eq!(json["app_ver"], "01.00");
    assert_eq!(
        json["manifest_hash"],
        expected.identity.manifest_hash.to_string()
    );
    let listed: Vec<(String, u64, String)> = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().into(),
                f["size"].as_u64().unwrap(),
                f["sha256"].as_str().unwrap().into(),
            )
        })
        .collect();
    let want: Vec<(String, u64, String)> = expected
        .files
        .iter()
        .map(|f| (f.path.clone(), f.size, f.sha256.to_string()))
        .collect();
    assert_eq!(listed, want);

    // A second refusal writes a second report rather than overwriting.
    open(&dump, &root, &[], &no_progress).unwrap_err();
    assert_eq!(reports(&root).len(), 2);
}

#[test]
fn an_update_merged_into_the_dump_is_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    // Same USRDIR bytes, PARAM.SFO says 01.01.
    let dump = make_dump(tmp.path(), "01.01");
    let table = table_for(&dump, "01.00");
    let root = root_in(tmp.path());
    let err = open(&dump, &root, &table, &no_progress).unwrap_err();
    match err {
        ExtractError::UnknownDump { identity, .. } => assert_eq!(identity.app_ver, "01.01"),
        other => panic!("{other}"),
    }

    // The control: the same tree at 01.00 is known.
    let tmp2 = tempfile::tempdir().unwrap();
    let dump2 = make_dump(tmp2.path(), "01.00");
    open(&dump2, &root_in(tmp2.path()), &table, &no_progress).unwrap();
}

fn rewrite_manifest(dir: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = dir.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    edit(&mut json);
    fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
}

#[test]
fn a_manifest_from_another_ir_version_or_layout_is_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), "01.00");
    let table = table_for(&dump, "01.00");
    let root = root_in(tmp.path());

    let cases: [(&str, u32, Stale); 2] = [
        (
            "ir_version",
            IR_VERSION - 1,
            Stale::IrVersion {
                found: IR_VERSION - 1,
            },
        ),
        (
            "layout",
            LAYOUT_VERSION + 1,
            Stale::Layout {
                found: LAYOUT_VERSION + 1,
            },
        ),
    ];
    for (field, value, why) in cases {
        let mut opened = open(&dump, &root, &table, &no_progress).unwrap();
        opened
            .dir
            .write_output("video/intro/x.mp4", b"old")
            .unwrap();
        opened.dir.mark_complete().unwrap();
        let dir = opened.dir.path().to_path_buf();
        rewrite_manifest(&dir, |j| j[field] = value.into());

        let again = open(&dump, &root, &table, &no_progress).unwrap();
        assert_eq!(again.state, DirState::Rebuilt(why));
        assert!(
            !dir.join("video/intro/x.mp4").exists(),
            "the old dir is gone"
        );
        let m = again.dir.manifest();
        assert_eq!((m.ir_version, m.layout), (IR_VERSION, LAYOUT_VERSION));
        assert!(!m.complete && m.outputs.is_empty());
    }

    // A manifest that doesn't parse is rebuilt too.
    let opened = open(&dump, &root, &table, &no_progress).unwrap();
    fs::write(opened.dir.path().join("manifest.json"), "{").unwrap();
    let again = open(&dump, &root, &table, &no_progress).unwrap();
    assert_eq!(again.state, DirState::Rebuilt(Stale::Unusable));
}

#[test]
fn outputs_go_through_partial_and_an_interrupted_run_resumes() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), "01.00");
    let table = table_for(&dump, "01.00");
    let root = root_in(tmp.path());

    let mut opened = open(&dump, &root, &table, &no_progress).unwrap();
    let id = opened.dir.manifest().identity.clone();
    assert_eq!(
        opened.dir.path(),
        root.path().join(format!(
            "BLUS30968-01.00-{}",
            &id.manifest_hash.to_string()[..8]
        ))
    );
    let rec = opened
        .dir
        .write_output("video/intro/a.mp4", b"aaaa")
        .unwrap();
    assert_eq!(rec.size, 4);
    assert!(opened.dir.output_done("video/intro/a.mp4").unwrap());
    // A writer died mid-file: the partial exists, nothing is recorded.
    let partial = opened.dir.partial_path("video/intro/b.mp4").unwrap();
    assert!(partial.ends_with("video/intro/b.mp4.partial"));
    fs::write(&partial, b"half").unwrap();
    assert!(!opened.dir.output_done("video/intro/b.mp4").unwrap());
    drop(opened);

    let mut resumed = open(&dump, &root, &table, &no_progress).unwrap();
    assert_eq!(resumed.state, DirState::Resume);
    assert!(!partial.exists(), "stray partials are swept");
    assert!(resumed.dir.output_done("video/intro/a.mp4").unwrap());
    assert!(!resumed.dir.output_done("video/intro/b.mp4").unwrap());
    resumed
        .dir
        .write_output("video/intro/b.mp4", b"bbbbbb")
        .unwrap();
    resumed.dir.mark_complete().unwrap();

    // A tampered output no longer counts as done.
    fs::write(resumed.dir.path().join("video/intro/a.mp4"), b"AAAA").unwrap();
    assert!(!resumed.dir.output_done("video/intro/a.mp4").unwrap());

    let current = open(&dump, &root, &table, &no_progress).unwrap();
    assert_eq!(current.state, DirState::Current);
    let outputs: Vec<_> = current
        .dir
        .manifest()
        .outputs
        .iter()
        .map(|o| (o.path.as_str(), o.size))
        .collect();
    assert_eq!(
        outputs,
        [("video/intro/a.mp4", 4), ("video/intro/b.mp4", 6)]
    );

    for bad in [
        "",
        "/abs",
        "../x",
        "a/../b",
        "./a",
        "a\\b",
        "manifest.json",
        "x.partial",
    ] {
        let mut dir = open(&dump, &root, &table, &no_progress).unwrap().dir;
        let err = dir.write_output(bad, b"x").unwrap_err();
        assert_eq!(err.code(), "GUAC-E-BAD-OUTPUT-PATH", "{bad}: {err}");
    }
}

fn git_init(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let ok = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

#[test]
fn an_extract_root_inside_a_git_repo_is_refused_with_its_path() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    git_init(&repo);
    let inside = repo.join("dev-data/guacamole/extract");
    let err = ExtractRoot::checked(&inside).unwrap_err();
    assert_eq!(err.code(), "GUAC-E-EXTRACT-IN-CHECKOUT");
    let msg = err.to_string();
    assert!(msg.contains(&inside.display().to_string()), "{msg}");
    assert!(
        msg.contains("may not live inside a source checkout"),
        "{msg}"
    );
    assert!(!inside.exists(), "nothing created");
    match err {
        ExtractError::InCheckout { repo: found, .. } => assert_eq!(found, repo),
        other => panic!("{other}"),
    }

    // The repo itself, the dev env route and the release route too.
    assert!(ExtractRoot::checked(&repo).is_err());
    let dev = ExtractRoot::resolve(guac_extract::RootMode::Dev {
        extract_dir: Some(inside.clone().into()),
    });
    assert!(matches!(dev, Err(ExtractError::InCheckout { .. })));
    let release = ExtractRoot::resolve(guac_extract::RootMode::Release {
        data_root: repo.join("dev-data/guacamole"),
    });
    assert!(matches!(release, Err(ExtractError::InCheckout { .. })));

    // A root checked before a repo appeared around it is refused at use.
    let later = tmp.path().join("later");
    let root = ExtractRoot::checked(later.join("extract")).unwrap();
    git_init(&later);
    let dump = make_dump(&tmp.path().join("d"), "01.00");
    let err = open(&dump, &root, &[], &no_progress).unwrap_err();
    assert_eq!(err.code(), "GUAC-E-EXTRACT-IN-CHECKOUT");
}

#[test]
fn an_extract_root_overlapping_the_dump_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), "01.00");
    let table = table_for(&dump, "01.00");
    for root in [
        dump.join("PS3_GAME/extract"),
        dump.clone(),
        tmp.path().to_path_buf(),
    ] {
        let root = ExtractRoot::checked(&root).unwrap();
        let err = open(&dump, &root, &table, &no_progress).unwrap_err();
        assert_eq!(err.code(), "GUAC-E-EXTRACT-OVERLAPS-DUMP", "{err}");
    }
}

/// macOS firmlinks: `/System/Volumes/Data/private/...` is `/private/...`, but
/// `realpath` doesn't fold it, so only an identity check sees the overlap.
#[cfg(target_os = "macos")]
#[test]
fn an_extract_root_spelled_through_a_firmlink_is_still_inside_the_dump() {
    let data = Path::new("/System/Volumes/Data");
    if !data.is_dir() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), "01.00").canonicalize().unwrap();
    let table = table_for(&dump, "01.00");
    let firm = data.join(dump.strip_prefix("/").unwrap());
    assert!(firm.is_dir(), "{}", firm.display());
    for root in [firm.join("extract"), firm.clone()] {
        let root = ExtractRoot::checked(&root).unwrap();
        for known in [&table[..], &[]] {
            let err = open(&dump, &root, known, &no_progress).unwrap_err();
            assert_eq!(err.code(), "GUAC-E-EXTRACT-OVERLAPS-DUMP", "{err}");
        }
    }
    // And a dump spelled through the firmlink, inside a plainly spelled root.
    let err = open(
        &firm,
        &ExtractRoot::checked(tmp.path()).unwrap(),
        &table,
        &no_progress,
    )
    .unwrap_err();
    assert_eq!(err.code(), "GUAC-E-EXTRACT-OVERLAPS-DUMP", "{err}");
    let mut names: Vec<_> = fs::read_dir(&dump)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["PS3_GAME"]);
}

/// A re-dump or a `cp -p` of another dump keeps the disc's mtimes, so a
/// same-size change with its mtime put back must still be read.
#[test]
fn a_same_size_change_with_its_mtime_restored_is_read_again() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), "01.00");
    let f = dump.join("PS3_GAME/USRDIR/movies/x.mov");
    // A whole-second 2012 stamp, like every file on the Giants disc.
    let disc_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_344_622_654);
    let stamp = |t| {
        fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(t)
            .unwrap()
    };
    stamp(disc_time);
    let table = table_for(&dump, "01.00");
    let root = root_in(tmp.path());
    let first = open(&dump, &root, &table, &no_progress).unwrap();
    assert_eq!(first.state, DirState::Fresh);
    assert_eq!(
        open(&dump, &root, &table, &no_progress)
            .unwrap()
            .files_cached,
        4
    );

    fs::write(&f, vec![0u8; 3000]).unwrap();
    stamp(disc_time);
    assert_eq!(fs::metadata(&f).unwrap().modified().unwrap(), disc_time);
    let err = open(&dump, &root, &table, &no_progress).unwrap_err();
    assert_eq!(err.code(), "GUAC-E-UNKNOWN-DUMP", "{err}");
}

#[test]
fn a_dump_missing_its_pieces_is_unreadable_and_named() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), "01.00");
    let root = root_in(tmp.path());
    fs::remove_dir_all(dump.join("PS3_GAME/USRDIR")).unwrap();
    let err = open(&dump, &root, &[], &no_progress).unwrap_err();
    assert_eq!(err.code(), "GUAC-E-DUMP-UNREADABLE");
    assert!(err.to_string().contains("USRDIR"), "{err}");
    fs::remove_file(dump.join("PS3_GAME/PARAM.SFO")).unwrap();
    let err = open(&dump, &root, &[], &no_progress).unwrap_err();
    assert!(err.to_string().contains("PARAM.SFO"), "{err}");
    assert!(reports(&root).is_empty());
}

type Snapshot = BTreeMap<PathBuf, Option<(u64, Sha256, SystemTime)>>;

fn snapshot(dir: &Path, out: &mut Snapshot) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let meta = fs::symlink_metadata(&p).unwrap();
        if meta.is_dir() {
            out.insert(p.clone(), None);
            snapshot(&p, out);
        } else {
            let bytes = fs::read(&p).unwrap();
            let sha = Sha256(<sha2::Sha256 as sha2::Digest>::digest(&bytes).into());
            out.insert(p, Some((meta.len(), sha, meta.modified().unwrap())));
        }
    }
}

#[cfg(unix)]
fn set_read_only(dir: &Path, read_only: bool) {
    use std::os::unix::fs::PermissionsExt;
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            set_read_only(&p, read_only);
        } else {
            let mode = if read_only { 0o444 } else { 0o644 };
            fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
        }
    }
    let mode = if read_only { 0o555 } else { 0o755 };
    fs::set_permissions(dir, fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn a_run_leaves_the_dump_exactly_as_it_was() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = make_dump(tmp.path(), "01.00");
    let table = table_for(&dump, "01.00");
    let root = root_in(tmp.path());
    let mut before = Snapshot::new();
    snapshot(&dump, &mut before);

    // Read-only on disk, so any write into the dump would fail the run.
    #[cfg(unix)]
    set_read_only(&dump, true);
    let run = || -> guac_extract::Result<()> {
        let mut opened = open(&dump, &root, &table, &no_progress)?;
        opened.dir.write_output("ir/characters.ron", b"()")?;
        opened.dir.mark_complete()?;
        open(&dump, &root, &table, &no_progress)?;
        open(&dump, &root, &[], &no_progress).unwrap_err();
        Ok(())
    };
    let result = run();
    #[cfg(unix)]
    set_read_only(&dump, false);
    result.unwrap();

    let mut after = Snapshot::new();
    snapshot(&dump, &mut after);
    assert_eq!(before, after);
}
