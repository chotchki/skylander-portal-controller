//! The intro transcode (AB.8, design decision 8): one ffmpeg run per intro
//! movie, Bink 1 in, [`TARGET`] out, as `video/intro/<movie>.<ext>` and
//! `audio/intro/<movie>.<ext>` in the identity dir. No scaling, the source
//! frame rate, the IR's voice track. Progress comes from `-progress pipe:1`.
//!
//! Done outputs are kept (resume); a movie whose source bytes and voice match
//! one already done is linked, not re-run (content-hash dedupe). Intros done
//! for another [`Target::tag`] are redone.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use guac_ir::{CharacterTable, Sha256, VoiceTrack};
use sha2::Digest;

use crate::error::{ExtractError, Result};
use crate::extract::{IdentityDir, TranscodeRecord};
use crate::ffmpeg::Ffmpeg;
use crate::intros::MOVIE_DIR;

pub const VIDEO_DIR: &str = "video/intro";
pub const AUDIO_DIR: &str = "audio/intro";

/// What an intro becomes. The one place the format is chosen: AB.7's spike
/// may change it, and [`Target::tag`] then redoes every intro.
#[derive(Debug)]
pub struct Target {
    pub video_ext: &'static str,
    /// Codec and muxer options for the video output (`-f` included: outputs
    /// are written as `.partial`, so ffmpeg can't guess from the name).
    pub video_args: &'static [&'static str],
    pub audio_ext: &'static str,
    pub audio_args: &'static [&'static str],
    /// What `ffmpeg -encoders` / `-decoders` must list.
    pub encoders: &'static [&'static str],
    pub decoders: &'static [&'static str],
}

/// Decision 7's lean: H.264 Constrained Baseline (the `openh264` decoder) in
/// MP4, and MP3 for Bevy's `mp3` feature.
pub const TARGET: Target = Target {
    video_ext: "mp4",
    video_args: &[
        "-c:v",
        "libx264",
        "-profile:v",
        "baseline",
        "-preset",
        "medium",
        "-crf",
        "18",
        "-pix_fmt",
        "yuv420p",
        "-fps_mode",
        "passthrough",
        "-map_metadata",
        "-1",
        "-f",
        "mp4",
        "-movflags",
        "+faststart",
    ],
    audio_ext: "mp3",
    audio_args: &[
        "-c:a",
        "libmp3lame",
        "-b:a",
        "192k",
        "-map_metadata",
        "-1",
        "-f",
        "mp3",
    ],
    encoders: &["libx264", "libmp3lame"],
    decoders: &["binkvideo", "binkaudio_dct", "binkaudio_rdft"],
};

impl Target {
    /// 16 hex of SHA-256 over every option and extension, so any change to
    /// the target is a new tag.
    pub fn tag(&self) -> String {
        let mut h = sha2::Sha256::new();
        for part in [self.video_ext]
            .iter()
            .chain(self.video_args)
            .chain([&self.audio_ext])
            .chain(self.audio_args)
        {
            h.update(part.as_bytes());
            h.update([0]);
        }
        Sha256(h.finalize().into()).to_string()[..16].to_owned()
    }

    /// `(video, audio)` output paths for `movie`, relative to the identity dir.
    pub fn outputs(&self, movie: &str) -> (String, String) {
        (
            format!("{VIDEO_DIR}/{movie}.{}", self.video_ext),
            format!("{AUDIO_DIR}/{movie}.{}", self.audio_ext),
        )
    }
}

/// `file:` keeps a path from being read as a protocol or an option.
fn file_arg(p: &Path) -> OsString {
    let mut s = OsString::from("file:");
    s.push(p);
    s
}

/// ffmpeg's arguments for one intro: `src` in, `video` and `audio` out.
pub fn command_line(
    src: &Path,
    video: &Path,
    audio: &Path,
    voice: VoiceTrack,
    target: &Target,
) -> Vec<OsString> {
    let mut a: Vec<OsString> = [
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostats",
        "-progress",
        "pipe:1",
        "-y",
        "-i",
    ]
    .map(OsString::from)
    .into();
    a.push(file_arg(src));
    let audio_map = match voice {
        VoiceTrack::Only => "0:a:0".to_owned(),
        VoiceTrack::Stream { index, bed: None } => format!("0:{index}"),
        // The bed first: amix takes the first input's layout, so stereo.
        VoiceTrack::Stream {
            index,
            bed: Some(bed),
        } => {
            a.push("-filter_complex".into());
            a.push(
                format!("[0:{bed}][0:{index}]amix=inputs=2:duration=first:normalize=0[voice]")
                    .into(),
            );
            "[voice]".to_owned()
        }
    };
    a.extend(["-map", "0:v:0"].map(OsString::from));
    a.extend(target.video_args.iter().map(OsString::from));
    a.push(file_arg(video));
    a.push("-map".into());
    a.push(audio_map.into());
    a.extend(target.audio_args.iter().map(OsString::from));
    a.push(file_arg(audio));
    a
}

/// One `-progress` block, reported at its `progress=` line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    pub frame: u64,
    /// Output time so far; `None` while ffmpeg says `N/A`.
    pub out_time_us: Option<u64>,
    pub total_size: Option<u64>,
    /// Encode speed against real time (`9.35x`).
    pub speed: Option<f64>,
    /// `progress=end`: the last block.
    pub end: bool,
}

/// Reads ffmpeg's `-progress` stream line by line.
#[derive(Debug, Default)]
pub struct ProgressParser {
    cur: Progress,
}

impl ProgressParser {
    pub fn line(&mut self, line: &str) -> Option<Progress> {
        let (k, v) = line.trim().split_once('=')?;
        let v = v.trim();
        match k {
            "frame" => self.cur.frame = v.parse().unwrap_or(self.cur.frame),
            // `out_time_ms` is microseconds too; `out_time_us` is the honest name.
            "out_time_us" => self.cur.out_time_us = v.parse().ok(),
            "total_size" => self.cur.total_size = v.parse().ok(),
            "speed" => self.cur.speed = v.strip_suffix('x').and_then(|s| s.trim().parse().ok()),
            "progress" => {
                self.cur.end = v == "end";
                return Some(self.cur.clone());
            }
            _ => {}
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Started,
    Encoding(Progress),
    Done(Done),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    Transcoded,
    /// Its outputs were already there and still hash to their records.
    Resumed,
    /// Linked from `from`'s outputs: same source bytes, same voice.
    Linked {
        from: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranscodeEvent {
    /// 0-based, of `total` distinct movies.
    pub index: usize,
    pub total: usize,
    pub movie: String,
    pub step: Step,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub transcoded: usize,
    pub resumed: usize,
    pub linked: usize,
}

/// The distinct intro movies of `table`, each with its voice choice.
pub fn jobs(table: &CharacterTable) -> Result<BTreeMap<String, VoiceTrack>> {
    let mut jobs = BTreeMap::new();
    for intro in table.characters.values().filter_map(|c| c.intro.as_ref()) {
        if !movie_name_ok(&intro.movie) {
            return Err(ExtractError::BadOutputPath {
                path: intro.movie.clone(),
            });
        }
        match jobs.insert(intro.movie.clone(), intro.voice) {
            Some(v) if v != intro.voice => {
                return Err(ExtractError::TranscodeFailed {
                    movie: intro.movie.clone(),
                    reason: format!("two voice choices, {v:?} and {:?}", intro.voice),
                });
            }
            _ => {}
        }
    }
    Ok(jobs)
}

fn movie_name_ok(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 64
        && m.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Transcodes every intro in `table` into `dir`, one ffmpeg at a time (x264
/// already takes every core). Records the target and ffmpeg in the manifest;
/// marking the extract complete is the caller's.
pub fn transcode_intros(
    dir: &mut IdentityDir,
    table: &CharacterTable,
    ffmpeg: &Ffmpeg,
    progress: &dyn Fn(TranscodeEvent),
) -> Result<Summary> {
    let jobs = jobs(table)?;
    let tag = TARGET.tag();
    if dir.manifest().transcode.as_ref().map(|t| &t.target) != Some(&tag) {
        dir.forget_outputs(&format!("{VIDEO_DIR}/"))?;
        dir.forget_outputs(&format!("{AUDIO_DIR}/"))?;
    }
    dir.set_transcode(TranscodeRecord {
        target: tag,
        ffmpeg: ffmpeg.version.clone(),
    })?;

    let movie_dir = PathBuf::from(&dir.manifest().source_dir).join(MOVIE_DIR);
    let mut done_by_key: BTreeMap<(Sha256, VoiceTrack), String> = BTreeMap::new();
    let mut summary = Summary::default();
    let total = jobs.len();
    for (index, (movie, voice)) in jobs.into_iter().enumerate() {
        let event = |step| {
            progress(TranscodeEvent {
                index,
                total,
                movie: movie.clone(),
                step,
            })
        };
        let rel = format!("movies/{movie}.mov");
        let sha = dir
            .manifest()
            .sources
            .iter()
            .find(|s| s.path == rel)
            .map(|s| s.sha256)
            .ok_or_else(|| ExtractError::TranscodeFailed {
                movie: movie.clone(),
                reason: format!("{rel} isn't in the dump's manifest"),
            })?;
        let (video, audio) = TARGET.outputs(&movie);
        event(Step::Started);
        let done = if dir.output_done(&video)? && dir.output_done(&audio)? {
            summary.resumed += 1;
            Done::Resumed
        } else if let Some(from) = done_by_key.get(&(sha, voice)) {
            let (fv, fa) = TARGET.outputs(from);
            dir.link_output(&fv, &video)?;
            dir.link_output(&fa, &audio)?;
            summary.linked += 1;
            Done::Linked { from: from.clone() }
        } else {
            let src = movie_dir.join(format!("{movie}.mov"));
            let args = command_line(
                &src,
                &dir.partial_path(&video)?,
                &dir.partial_path(&audio)?,
                voice,
                &TARGET,
            );
            run(&ffmpeg.path, &args, |p| event(Step::Encoding(p))).map_err(|reason| {
                ExtractError::TranscodeFailed {
                    movie: movie.clone(),
                    reason,
                }
            })?;
            dir.commit(&video)?;
            dir.commit(&audio)?;
            summary.transcoded += 1;
            Done::Transcoded
        };
        done_by_key.entry((sha, voice)).or_insert(movie.clone());
        event(Step::Done(done));
    }
    Ok(summary)
}

/// Runs ffmpeg, feeding each `-progress` block to `on`. `Err` carries the
/// exit status and the tail of stderr.
fn run(exe: &Path, args: &[OsString], mut on: impl FnMut(Progress)) -> Result<(), String> {
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't start {}: {e}", exe.display()))?;
    let mut stderr = child.stderr.take().expect("piped");
    let errs = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        let text = String::from_utf8_lossy(&buf).into_owned();
        let cut = text.len().saturating_sub(4096);
        let cut = (cut..=text.len())
            .find(|&i| text.is_char_boundary(i))
            .unwrap_or(text.len());
        text[cut..].trim().to_owned()
    });
    let mut parser = ProgressParser::default();
    for line in BufReader::new(child.stdout.take().expect("piped")).lines() {
        let Ok(line) = line else { break };
        if let Some(p) = parser.line(&line) {
            on(p);
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let tail = errs.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg {status}: {tail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(a: &[OsString]) -> Vec<String> {
        a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    /// The flag pairs in `args` from the first `-i`, as one string, so the
    /// tests read like the command line.
    fn joined(a: &[OsString]) -> String {
        strs(a).join(" ")
    }

    #[test]
    fn one_track_command_line() {
        let a = command_line(
            Path::new("/d/movies/eruptor.mov"),
            Path::new("/x/video/intro/eruptor.mp4.partial"),
            Path::new("/x/audio/intro/eruptor.mp3.partial"),
            VoiceTrack::Only,
            &TARGET,
        );
        let s = joined(&a);
        assert!(
            s.starts_with(
                "-nostdin -hide_banner -loglevel error -nostats -progress pipe:1 -y \
                 -i file:/d/movies/eruptor.mov -map 0:v:0 -c:v libx264 -profile:v baseline "
            ),
            "{s}"
        );
        assert!(
            s.ends_with(
                "-f mp4 -movflags +faststart file:/x/video/intro/eruptor.mp4.partial \
                 -map 0:a:0 -c:a libmp3lame -b:a 192k -map_metadata -1 -f mp3 \
                 file:/x/audio/intro/eruptor.mp3.partial"
            ),
            "{s}"
        );
        // No scaling, no frame rate forced, nothing else read.
        for not in ["-vf", "-s", "-r", "scale", "-filter_complex"] {
            assert!(!strs(&a).iter().any(|x| x == not), "{not} in {s}");
        }
        assert_eq!(strs(&a).iter().filter(|x| *x == "-i").count(), 1);
        assert_eq!(strs(&a).iter().filter(|x| *x == "-map").count(), 2);
    }

    #[test]
    fn multi_track_command_lines() {
        let src = Path::new("/d/fmv01.mov");
        let (v, au) = (Path::new("/x/v"), Path::new("/x/a"));
        let picked = joined(&command_line(
            src,
            v,
            au,
            VoiceTrack::Stream {
                index: 3,
                bed: None,
            },
            &TARGET,
        ));
        assert!(
            picked.contains("file:/x/v -map 0:3 -c:a libmp3lame"),
            "{picked}"
        );
        assert!(!picked.contains("-filter_complex"));

        let mixed = joined(&command_line(
            src,
            v,
            au,
            VoiceTrack::Stream {
                index: 3,
                bed: Some(13),
            },
            &TARGET,
        ));
        assert!(
            mixed.contains(
                "-i file:/d/fmv01.mov -filter_complex \
                 [0:13][0:3]amix=inputs=2:duration=first:normalize=0[voice] -map 0:v:0 "
            ),
            "{mixed}"
        );
        assert!(
            mixed.contains("file:/x/v -map [voice] -c:a libmp3lame"),
            "{mixed}"
        );
    }

    #[test]
    fn the_tag_follows_the_target() {
        let t = TARGET.tag();
        assert_eq!(t.len(), 16);
        assert_eq!(t, TARGET.tag());
        let other = Target {
            video_args: &["-c:v", "libx264", "-crf", "20", "-f", "mp4"],
            ..TARGET
        };
        assert_ne!(other.tag(), t);
        assert_eq!(
            TARGET.outputs("eruptor"),
            (
                "video/intro/eruptor.mp4".to_owned(),
                "audio/intro/eruptor.mp3".to_owned()
            )
        );
    }

    /// ffmpeg 9.0.2's `-progress pipe:1`, verbatim, from this module's
    /// command line over a 2 s `testsrc` + `sine` clip read at `-readrate 1`
    /// (no disc file): four blocks, two before x264 emitted a frame.
    const CAPTURED: &str = "\
frame=0
fps=0.00
stream_0_0_q=0.0
bitrate=   0.0kbits/s
total_size=0
out_time_us=131678
out_time_ms=131678
out_time=00:00:00.131678
dup_frames=0
drop_frames=0
speed=0.258x
progress=continue
frame=0
fps=0.00
stream_0_0_q=0.0
bitrate=   0.0kbits/s
total_size=0
out_time_us=131678
out_time_ms=131678
out_time=00:00:00.131678
dup_frames=0
drop_frames=0
speed=0.129x
progress=continue
frame=40
fps=26.14
stream_0_0_q=-1.0
bitrate=   0.3kbits/s
total_size=48
out_time_us=1437800
out_time_ms=1437800
out_time=00:00:01.437800
dup_frames=0
drop_frames=0
speed=0.94x
progress=continue
frame=60
fps=38.70
stream_0_0_q=-1.0
bitrate= 282.3kbits/s
total_size=70656
out_time_us=2002000
out_time_ms=2002000
out_time=00:00:02.002000
dup_frames=0
drop_frames=0
speed=1.29x
progress=end
";

    #[test]
    fn the_progress_parser_reads_captured_output() {
        let mut p = ProgressParser::default();
        let blocks: Vec<Progress> = CAPTURED.lines().filter_map(|l| p.line(l)).collect();
        let at = |frame, us, size, speed, end| Progress {
            frame,
            out_time_us: Some(us),
            total_size: Some(size),
            speed: Some(speed),
            end,
        };
        assert_eq!(
            blocks,
            [
                at(0, 131_678, 0, 0.258, false),
                at(0, 131_678, 0, 0.129, false),
                at(40, 1_437_800, 48, 0.94, false),
                at(60, 2_002_000, 70_656, 1.29, true),
            ]
        );
    }

    #[test]
    fn the_progress_parser_takes_na_and_padding() {
        // Hand-written: older builds print N/A before the first packet and
        // pad the speed.
        let mut p = ProgressParser::default();
        for l in ["frame=0", "out_time_us=N/A", "total_size=N/A", "speed=N/A"] {
            assert_eq!(p.line(l), None);
        }
        assert_eq!(p.line("progress=continue"), Some(Progress::default()));
        assert_eq!(p.line("speed= 1.5x"), None);
        assert_eq!(p.line("garbage"), None);
        assert_eq!(p.line("frame=x"), None);
        assert_eq!(p.line("progress=continue").unwrap().speed, Some(1.5));
    }

    #[test]
    fn movie_names_are_plain() {
        assert!(movie_name_ok("eruptor"));
        assert!(movie_name_ok("tfb_logo_640x456"));
        for bad in ["", "../x", "a/b", "a.b", "a b", &"x".repeat(65)] {
            assert!(!movie_name_ok(bad), "{bad}");
        }
    }
}
