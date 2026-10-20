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
use guac_extract::stages::{self, Event, Files};
use guac_extract::transcode::{self, Done, Step};
use guac_extract::{DirState, ExtractRoot, IdentityDir, Opened, RootMode, roster};

const USAGE: &str = "\
usage: guac-extract identify <dump-dir>
       guac-extract census <dump-dir>
       guac-extract roster <dump-dir>
       guac-extract models <dump-dir>
       guac-extract levels <dump-dir>
       guac-extract scripts <dump-dir>
       guac-extract intros <dump-dir>
       guac-extract transcode <dump-dir>
       guac-extract extract <dump-dir>
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
  roster     identify, then read the figures the disc defines into
             ir/roster.ron and each one's portrait into portraits/<id>.png in
             the dump's folder
  models     the roster, then each character's model into
             models/<load file>.ron, the textures its materials bind into
             textures/<sha256>.ktx2, its hero side into heroes/ and every
             slot of its animation set into palettes/<load file>.ron, and
             the 3D-UI rig into ir/rig.ron in the dump's folder; a model
             that won't read is listed, not fatal
  levels     identify, then each level guacamole plays (chapter 1) into
             levels/<archive>/<part>.ron, part by part (scene, props,
             collision, placements, lights, atmosphere, palettes), its
             entry as levels/<archive>/script.igz, its textures beside the
             models'; a part already there is kept, one that won't read is
             listed, not fatal
  scripts    identify, then the fight's script data outside a level: the
             level.bld of bootstrap.bld, global.bld, init_setup.bld and each
             fighter's archive into scripts/<archive>.igz, and the models
             global.bld's and each fighter's placements name into
             spawns/<archive>.ron; a part already there is kept, one that
             won't read is listed, not fatal
  intros     identify, then map each toy type's character archives to its
             intro movie by their voice-over name, the roster's names as the
             fallback, and write the character table to ir/characters.ron in
             the dump's folder
  transcode  intros, then turn each intro movie into video/intro/<name>.mp4
             and audio/intro/<name>.mp3 in the dump's folder (ffmpeg:
             GUAC_FFMPEG, else beside guac-extract, else PATH, else Homebrew's
             bin folders on macOS); leaves the extract unmarked, since it
             writes no roster
  extract    every stage, in guacamole's order: identify, the roster, the
             models, the levels, the scripts, then ffmpeg and the intros; a
             missing ffmpeg leaves the rest in place and the extract
             unmarked
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

fn open_dev(dump: &Path) -> Result<Opened, guac_extract::ExtractError> {
    let root = ExtractRoot::resolve(RootMode::Dev {
        extract_dir: extract_dir(),
    })?;
    guac_extract::open(dump, &root, KNOWN_DUMPS, &|_| {})
}

/// The roster's base names, the intro index's fallback, read off the dump.
fn fallback_names(
    dir: &IdentityDir,
) -> Result<std::collections::BTreeMap<u32, String>, guac_extract::ExtractError> {
    let m = dir.manifest();
    let roster = roster::read(Path::new(&m.source_dir), m.identity.clone())?;
    Ok(roster::base_names(&roster))
}

fn files_line(files: &Files) -> String {
    format!(
        "{} written, {} already there, {} bytes",
        files.written, files.kept, files.bytes
    )
}

fn roster(dump: &Path) -> Result<(), guac_extract::ExtractError> {
    let mut opened = open_dev(dump)?;
    let stage = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {})?;
    let r = &stage.roster;
    println!(
        "roster      {} figures, {} languages, {} portraits",
        r.figures.len(),
        r.languages.len(),
        stage.portraits
    );
    println!("files       {}", files_line(&stage.files));
    println!("extract     {}", opened.dir.path().display());
    println!("took        {:.2} s", stage.took.as_secs_f64());
    Ok(())
}

fn models_line(m: &stages::ModelStage) -> String {
    format!(
        "{} models, {} failed; {} heroes, {} unsettled ({:.2} s); {} textures for {} binds; {}; {:.2} s",
        m.models,
        m.failed.len(),
        m.heroes,
        m.unsettled.len(),
        m.heroes_took.as_secs_f64(),
        m.textures,
        m.binds,
        files_line(&m.files),
        m.took.as_secs_f64()
    )
}

fn palettes_line(p: &stages::PaletteCount) -> String {
    format!(
        "{} palettes: {} clips, {} networks, {} slots refused; {}; {:.2} s",
        p.palettes,
        p.clips,
        p.networks,
        p.refused,
        files_line(&p.files),
        p.took.as_secs_f64()
    )
}

fn scripts_line(s: &stages::ScriptStage) -> String {
    format!(
        "{} parts read, {} kept, {} failed; {} entries, {} scripts ({}); {} spawn models, {} refused ({}); {:.2} s",
        s.parts_read,
        s.parts_kept,
        s.failed.len(),
        s.entries,
        s.scripts,
        files_line(&s.script_files),
        s.props,
        s.refused_props.len(),
        files_line(&s.spawn_files),
        s.took.as_secs_f64()
    )
}

/// The stage's failed parts and refused spawn models, a line each.
fn script_failures(s: &stages::ScriptStage) {
    for f in &s.failed {
        println!("  failed    {}", f.reason);
    }
    for (prop, why) in &s.refused_props {
        println!("  refused   {prop}: {why}");
    }
}

fn scripts(dump: &Path) -> Result<(), guac_extract::ExtractError> {
    let mut opened = open_dev(dump)?;
    let s = stages::script_stage(&mut opened.dir, IrFormat::Ron, &|_| {})?;
    println!("scripts     {}", scripts_line(&s));
    script_failures(&s);
    println!("extract     {}", opened.dir.path().display());
    Ok(())
}

/// The stage's failures and unsettled heroes, a line each.
fn model_failures(m: &stages::ModelStage) {
    for f in &m.failed {
        println!("  failed    {}: {}", f.load_file, f.reason);
    }
    for f in &m.unsettled {
        println!("  unsettled {}: {}", f.load_file, f.reason);
    }
}

fn models(dump: &Path) -> Result<(), guac_extract::ExtractError> {
    let mut opened = open_dev(dump)?;
    let roster = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {})?;
    println!(
        "roster      {} figures; {:.2} s",
        roster.roster.figures.len(),
        roster.took.as_secs_f64()
    );
    let m = stages::model_stage(&mut opened.dir, &roster.roster, IrFormat::Ron, &|_| {})?;
    println!("models      {}", models_line(&m));
    println!("palettes    {}", palettes_line(&m.palettes));
    model_failures(&m);
    println!("extract     {}", opened.dir.path().display());
    Ok(())
}

fn levels_line(l: &stages::LevelStage) -> String {
    format!(
        "{} whole; {} parts read, {} kept, {} failed; {} props, {} refused; {} textures, {} already in the store; {}; {:.2} s",
        l.levels,
        l.parts_read,
        l.parts_kept,
        l.failed.len(),
        l.props,
        l.refused_props.len(),
        l.textures,
        l.shared,
        files_line(&l.files),
        l.took.as_secs_f64()
    )
}

/// The stage's failed parts and refused props, a line each.
fn level_failures(l: &stages::LevelStage) {
    for f in &l.failed {
        println!("  failed    {}", f.reason);
    }
    for (prop, why) in &l.refused_props {
        println!("  refused   {prop}: {why}");
    }
}

fn levels(dump: &Path) -> Result<(), guac_extract::ExtractError> {
    let mut opened = open_dev(dump)?;
    let l = stages::level_stage(&mut opened.dir, IrFormat::Ron, &|_| {})?;
    println!("levels      {}", levels_line(&l));
    println!(
        "  scripts {}, creatures {}",
        l.scripts,
        palettes_line(&l.palettes)
    );
    level_failures(&l);
    println!("extract     {}", opened.dir.path().display());
    Ok(())
}

fn extract(dump: &Path) -> Result<(), guac_extract::ExtractError> {
    let started = Instant::now();
    let mut opened = open_dev(dump)?;
    println!(
        "identity    {:?} in {:.2} s",
        opened.state,
        started.elapsed().as_secs_f64()
    );
    if opened.state == DirState::Current {
        println!("extract     {} (current)", opened.dir.path().display());
        return Ok(());
    }
    let ffmpeg = dev_var("GUAC_FFMPEG").map(PathBuf::from);
    let done = stages::run(&mut opened.dir, ffmpeg.as_deref(), IrFormat::Ron, &|e| {
        if let Event::Transcode(t) = e
            && let Step::Done(_) = t.step
        {
            println!("  {:2}/{}  {}", t.index + 1, t.total, t.movie);
        }
    })?;
    let r = &done.roster;
    println!(
        "roster      {} figures, {} portraits; {}; {:.2} s",
        r.roster.figures.len(),
        r.portraits,
        files_line(&r.files),
        r.took.as_secs_f64()
    );
    let p = &done.progression;
    match &p.progression {
        Ok(t) => println!(
            "progression {} levels to {} xp, {} save params, gold to {}; {}; {:.2} s",
            t.level_xp.len(),
            t.level_xp.last().copied().unwrap_or(0),
            t.save_params.len(),
            t.gold_cap,
            files_line(&p.files),
            p.took.as_secs_f64()
        ),
        Err(why) => println!("progression didn't read: {why}"),
    }
    println!("models      {}", models_line(&done.models));
    println!("palettes    {}", palettes_line(&done.models.palettes));
    model_failures(&done.models);
    println!("levels      {}", levels_line(&done.levels));
    println!(
        "  scripts {}, creatures {}",
        done.levels.scripts,
        palettes_line(&done.levels.palettes)
    );
    level_failures(&done.levels);
    println!("scripts     {}", scripts_line(&done.scripts));
    script_failures(&done.scripts);
    match &done.intros {
        Ok(i) => println!(
            "intros      {} of {} toy types; {} transcoded, {} already done, {} linked; {:.1} s",
            i.index
                .table
                .characters
                .values()
                .filter(|c| c.intro.is_some())
                .count(),
            i.index.table.characters.len(),
            i.summary.transcoded,
            i.summary.resumed,
            i.summary.linked,
            i.took.as_secs_f64()
        ),
        Err(e) => println!("intros      none ({})", e.code()),
    }
    let complete = opened.dir.manifest().complete;
    println!(
        "extract     {} ({})",
        opened.dir.path().display(),
        if complete { "complete" } else { "unmarked" }
    );
    println!("took        {:.1} s", started.elapsed().as_secs_f64());
    done.intros.map(drop)
}

fn intros(dump: &Path) -> Result<(), guac_extract::ExtractError> {
    let mut opened = open_dev(dump)?;
    let started = Instant::now();
    let idx = intros::index(&opened.dir, &fallback_names(&opened.dir)?)?;
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
    let idx = intros::index(&opened.dir, &fallback_names(&opened.dir)?)?;
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
        ["roster", dump] => match roster(Path::new(dump)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("guac-extract: {e}");
                ExitCode::FAILURE
            }
        },
        ["models", dump] => match models(Path::new(dump)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("guac-extract: {e}");
                ExitCode::FAILURE
            }
        },
        ["scripts", dump] => match scripts(Path::new(dump)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("guac-extract: {e}");
                ExitCode::FAILURE
            }
        },
        ["levels", dump] => match levels(Path::new(dump)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("guac-extract: {e}");
                ExitCode::FAILURE
            }
        },
        ["extract", dump] => match extract(Path::new(dump)) {
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
