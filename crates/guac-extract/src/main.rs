//! `guac-extract`: the extractor from a terminal, over the same library
//! guacamole calls. A dev tool until guacamole ships it: the extract root is
//! `GUAC_EXTRACT_DIR` (the environment, else `.env.dev` in the current folder),
//! required, outside any git working tree.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use guac_extract::census::find_ffprobe;
use guac_extract::ffmpeg::Ffmpeg;
use guac_extract::intros::{self, IrFormat};
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::transcode::{self, Done, Step};
use guac_extract::{DirState, ExtractRoot, RootMode};

const USAGE: &str = "\
usage: guac-extract identify <dump-dir>
       guac-extract census <dump-dir>
       guac-extract intros <dump-dir>
       guac-extract transcode <dump-dir>
       guac-extract [--help | --version]

Extracts what guacamole plays from your own Skylanders disc dump.

  identify   hash the dump (the folder holding PS3_GAME), check it against the
             known dumps and open its folder under GUAC_EXTRACT_DIR; an unknown
             dump gets a report of names, sizes and hashes beside that folder
  census     sniff every file of any dump and every IGA entry, measure the
             entropy of whatever has no known magic, count each Bink file's
             audio tracks (ffprobe: GUAC_FFPROBE, else beside guac-extract,
             else PATH, else Homebrew's bin folders on macOS) and write
             census-<title>-<version>-<stamp>.json into GUAC_EXTRACT_DIR
  intros     identify, then map each toy type's character archives to its
             intro movie by their voice-over name and write the character
             table to ir/characters.ron in the dump's folder; guacamole adds
             the interim catalogue's names as a fallback, this doesn't
  transcode  intros, then turn each intro movie into video/intro/<name>.mp4
             and audio/intro/<name>.mp3 in the dump's folder (ffmpeg:
             GUAC_FFMPEG, else beside guac-extract, else PATH, else Homebrew's
             bin folders on macOS); leaves the extract unmarked, since
             guacamole's has the fallback names too
";

/// The environment, else `.env.dev` in the current folder.
fn dev_var(key: &str) -> Option<OsString> {
    if let Some(v) = std::env::var_os(key) {
        return Some(v);
    }
    let text = std::fs::read_to_string(".env.dev").ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').into())
    })
}

fn extract_dir() -> Option<OsString> {
    dev_var("GUAC_EXTRACT_DIR")
}

fn census(dump: &Path) -> Result<(), guac_extract::ExtractError> {
    let root = ExtractRoot::resolve(RootMode::Dev {
        extract_dir: extract_dir(),
    })?;
    let ffprobe = find_ffprobe(dev_var("GUAC_FFPROBE").map(PathBuf::from));
    let started = Instant::now();
    let (c, report) = guac_extract::census::census(dump, &root, ffprobe.as_deref())?;
    let s = &c.summary;
    println!("dump        {} {}", c.title_id, c.app_ver);
    println!("files       {} ({} bytes)", s.files, s.bytes);
    for (what, n) in &s.files_by_what {
        println!("  {n:6}  {what}");
    }
    println!(
        "iga         {} archives ({} unread), {} entries, {} unknown, {} entry errors",
        s.iga_archives,
        s.iga_unread.len(),
        s.iga_entries,
        s.unknown_entries,
        s.entry_errors
    );
    for (what, n) in &s.entries_by_what {
        println!("  {n:6}  {what}");
    }
    println!(
        "unknown     {} files, {} high-entropy; {} high-entropy entries",
        s.unknown_files.len(),
        s.high_entropy_unknown_files.len(),
        s.high_entropy_unknown_entries.len()
    );
    println!(
        "bink        {} files, {} multi-track, {} unprobed ({})",
        s.bink_files,
        s.multi_track_bink.len(),
        s.bink_unprobed.len(),
        c.ffprobe.as_deref().unwrap_or("no ffprobe")
    );
    println!("report      {}", report.display());
    println!("took        {:.1} s", started.elapsed().as_secs_f64());
    Ok(())
}

fn identify(dump: &Path) -> Result<(), guac_extract::ExtractError> {
    let root = ExtractRoot::resolve(RootMode::Dev {
        extract_dir: extract_dir(),
    })?;
    let started = Instant::now();
    let opened = guac_extract::open(dump, &root, KNOWN_DUMPS, &|_| {})?;
    let m = opened.dir.manifest();
    println!(
        "identity    {} {} {}",
        m.identity.title_id, m.identity.app_ver, m.identity.manifest_hash
    );
    println!("extract     {}", opened.dir.path().display());
    let state = match &opened.state {
        DirState::Fresh => "fresh".to_owned(),
        DirState::Current => "current".to_owned(),
        DirState::Resume => "resume".to_owned(),
        DirState::Rebuilt(why) => format!("rebuilt ({why:?})"),
    };
    println!("state       {state}");
    println!(
        "hashed      {} files, {} bytes; {} from the cache; {:.1} s",
        opened.files_hashed,
        opened.bytes_hashed,
        opened.files_cached,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn intros(dump: &Path) -> Result<(), guac_extract::ExtractError> {
    let root = ExtractRoot::resolve(RootMode::Dev {
        extract_dir: extract_dir(),
    })?;
    let mut opened = guac_extract::open(dump, &root, KNOWN_DUMPS, &|_| {})?;
    let started = Instant::now();
    let idx = intros::index(&opened.dir, &Default::default())?;
    let out = intros::write(&mut opened.dir, &idx.table, IrFormat::Ron)?;
    let with_intro = idx
        .table
        .characters
        .values()
        .filter(|c| c.intro.is_some())
        .count();
    println!(
        "characters  {} toy types, {with_intro} with an intro",
        idx.table.characters.len()
    );
    for (toy, c) in &idx.table.characters {
        if c.intro.is_none() {
            println!(
                "  no intro  {toy} ({})",
                c.vo_name.as_deref().unwrap_or("no voice-over")
            );
        }
    }
    println!("unclaimed   {}", idx.unclaimed_movies.join(" "));
    for (movie, toys) in &idx.claimed_twice {
        println!("  claimed twice  {movie} by {toys:?}");
    }
    println!(
        "wrote       {}",
        opened.dir.path().join(&out.path).display()
    );
    println!("took        {:.1} s", started.elapsed().as_secs_f64());
    Ok(())
}

fn transcode(dump: &Path) -> Result<(), guac_extract::ExtractError> {
    let root = ExtractRoot::resolve(RootMode::Dev {
        extract_dir: extract_dir(),
    })?;
    let ffmpeg = Ffmpeg::locate(dev_var("GUAC_FFMPEG").map(PathBuf::from).as_deref())?;
    println!("ffmpeg      {} ({:?})", ffmpeg.path.display(), ffmpeg.how);
    let mut opened = guac_extract::open(dump, &root, KNOWN_DUMPS, &|_| {})?;
    let idx = intros::index(&opened.dir, &Default::default())?;
    intros::write(&mut opened.dir, &idx.table, IrFormat::Ron)?;
    let started = Instant::now();
    let summary = transcode::transcode_intros(&mut opened.dir, &idx.table, &ffmpeg, &|e| {
        if let Step::Done(d) = &e.step {
            let how = match d {
                Done::Transcoded => "transcoded".to_owned(),
                Done::Resumed => "already done".to_owned(),
                Done::Linked { from } => format!("linked from {from}"),
            };
            println!("  {:2}/{}  {:16} {how}", e.index + 1, e.total, e.movie);
        }
    })?;
    let bytes: u64 = opened
        .dir
        .manifest()
        .outputs
        .iter()
        .filter(|o| o.path.starts_with("video/") || o.path.starts_with("audio/"))
        .map(|o| o.size)
        .sum();
    println!(
        "intros      {} transcoded, {} already done, {} linked; {bytes} bytes",
        summary.transcoded, summary.resumed, summary.linked
    );
    println!("extract     {}", opened.dir.path().display());
    println!("took        {:.1} s", started.elapsed().as_secs_f64());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["-h" | "--help"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["-V" | "--version"] => {
            println!(
                "guac-extract {} (layout v{}, IR v{})",
                env!("CARGO_PKG_VERSION"),
                guac_extract::LAYOUT_VERSION,
                guac_extract::IR_VERSION
            );
            ExitCode::SUCCESS
        }
        ["census", dump] => match census(Path::new(dump)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("guac-extract: {e}");
                ExitCode::FAILURE
            }
        },
        ["intros", dump] => match intros(Path::new(dump)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("guac-extract: {e}");
                ExitCode::FAILURE
            }
        },
        ["transcode", dump] => match transcode(Path::new(dump)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("guac-extract: {e}");
                ExitCode::FAILURE
            }
        },
        ["identify", dump] => match identify(Path::new(dump)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("guac-extract: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprint!(
                "guac-extract: unknown arguments `{}`\n\n{USAGE}",
                args.join(" ")
            );
            ExitCode::from(2)
        }
    }
}
