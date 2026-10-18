//! AB.7's video playback spike (openspec AB, design decision 7): measures the
//! intro path, ships nothing. Reads an extract AB.8 wrote, outside the tree.
//!
//! `cargo run --release -p guacamole --features video-spike --example video_spike -- <cmd>`
//! - `decode <mp4>...`: MP4 demux, OpenH264 decode and YUV→RGBA, ms per frame.
//! - `audio <mp4> <mp3> [--ref <pcm>|-]`: the MP3 through Bevy's own decoder
//!   against the video's timestamps (drift from decode timestamps, no audio
//!   device); `--ref` takes the source's audio as 44.1 kHz stereo s16le (piped
//!   from ffmpeg, so nothing lands on disk) and finds the MP3's offset from it.
//! - `play <mp4> <mp3> [--no-upload] [--wall] [--hz N] [--audible] [--window] [--video-lag-ms N] [--csv <file>]`: headless
//!   Bevy (no window, an offscreen camera) shows each frame through an
//!   `ImageNode` while the MP3 plays through Bevy audio, muted unless
//!   `--audible`; `--wall` presents by the wall clock anchored to the sink's
//!   position steps (decision 7), otherwise by the raw position; prints upload
//!   cost, frame times and A/V drift (`--csv <file>`
//!   writes each update's wall clock, audio clock and shown frame; `--window`
//!   plays in a real window instead, for an unlocked display, and
//!   `--video-lag-ms` holds the video back to judge lip sync).

use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::RenderAssetUsages;
use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, Decodable, Source};
use bevy::camera::RenderTarget;
use bevy::prelude::{
    App, AppExit, Assets, Camera2d, Commands, DefaultPlugins, Handle, Image, ImageNode,
    IntoScheduleConfigs, Local, MessageWriter, Node, PlaybackSettings, PluginGroup, Query, Res,
    ResMut, Resource, Startup, Update, WindowPlugin, default, percent,
};
use bevy::render::render_asset::{ExtractedAssets, prepare_assets};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use crossbeam_channel::{Receiver, TryRecvError, bounded};
use openh264::decoder::{DecodedYUV, Decoder};
use openh264::formats::YUVSource;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("decode") if args.len() > 1 => decode_cmd(&args[1..]),
        Some("audio") if args.len() == 3 || (args.len() == 5 && args[3] == "--ref") => audio_cmd(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args.get(4).map(Path::new),
        ),
        Some("play") if args.len() >= 3 => play_cmd(&args[1..]),
        _ => bail!(
            "usage: video_spike decode <mp4>... | audio <mp4> <mp3> [--ref <s16le>|-] | play <mp4> <mp3> [--no-upload] [--wall] [--hz N] [--audible] [--window] [--video-lag-ms N] [--csv <file>]"
        ),
    }
}

// ---- demux ----------------------------------------------------------------

/// One access unit in Annex B, ready for OpenH264.
struct Packet {
    pts: f64,
    dur: f64,
    annexb: Vec<u8>,
}

struct Video {
    width: usize,
    height: usize,
    packets: Vec<Packet>,
}

impl Video {
    fn end(&self) -> f64 {
        self.packets.last().map_or(0.0, |p| p.pts + p.dur)
    }
}

/// MP4 (AVCC, length-prefixed NALs) to Annex B, with SPS/PPS before each sync sample.
fn demux(path: &Path) -> Result<Video> {
    let bytes =
        std::fs::read(path).map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let size = bytes.len() as u64;
    let mut mp4 = mp4::Mp4Reader::read_header(Cursor::new(bytes), size)?;
    let (id, ts, count, width, height, len_size, params) = {
        let track = mp4
            .tracks()
            .values()
            .find(|t| matches!(t.media_type(), Ok(mp4::MediaType::H264)))
            .context("no H.264 track")?;
        let avcc = &track
            .trak
            .mdia
            .minf
            .stbl
            .stsd
            .avc1
            .as_ref()
            .context("no avc1 box")?
            .avcc;
        let mut params = Vec::new();
        for nal in avcc
            .sequence_parameter_sets
            .iter()
            .chain(&avcc.picture_parameter_sets)
        {
            params.extend_from_slice(&[0, 0, 0, 1]);
            params.extend_from_slice(&nal.bytes);
        }
        (
            track.track_id(),
            f64::from(track.timescale()),
            track.sample_count(),
            usize::from(track.width()),
            usize::from(track.height()),
            usize::from(avcc.length_size_minus_one & 3) + 1,
            params,
        )
    };
    let mut packets = Vec::with_capacity(count as usize);
    for i in 1..=count {
        let s = mp4.read_sample(id, i)?.context("missing sample")?;
        let mut annexb = if s.is_sync {
            params.clone()
        } else {
            Vec::new()
        };
        let mut rest: &[u8] = &s.bytes;
        while rest.len() >= len_size {
            let n = rest[..len_size]
                .iter()
                .fold(0usize, |a, b| (a << 8) | usize::from(*b));
            rest = &rest[len_size..];
            if n > rest.len() {
                bail!("sample {i}: NAL of {n} bytes overruns the sample");
            }
            annexb.extend_from_slice(&[0, 0, 0, 1]);
            annexb.extend_from_slice(&rest[..n]);
            rest = &rest[n..];
        }
        packets.push(Packet {
            pts: (s.start_time as f64 + f64::from(s.rendering_offset)) / ts,
            dur: f64::from(s.duration) / ts,
            annexb,
        });
    }
    // The IR's intros are 1280x720; anything bigger is a bad extract.
    if width == 0 || height == 0 || width > MAX_W || height > MAX_H {
        bail!("{width}x{height} track, outside 1x1..{MAX_W}x{MAX_H}");
    }
    Ok(Video {
        width,
        height,
        packets,
    })
}

const MAX_W: usize = 1280;
const MAX_H: usize = 720;

/// `write_rgba8` asserts the buffer fits the decoded picture, so check the
/// picture against the track's size first and fail instead of panicking.
fn to_rgba(img: &DecodedYUV, (w, h): (usize, usize), rgba: &mut [u8]) -> Result<()> {
    if img.dimensions() != (w, h) {
        let (dw, dh) = img.dimensions();
        bail!("decoded {dw}x{dh}, the track says {w}x{h}");
    }
    img.write_rgba8(rgba);
    Ok(())
}

// ---- decode ---------------------------------------------------------------

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// mean, p50, p95, max in ms.
fn stats(v: &[Duration]) -> String {
    if v.is_empty() {
        return "n/a".into();
    }
    let mut s: Vec<f64> = v.iter().copied().map(ms).collect();
    s.sort_by(f64::total_cmp);
    let pick = |q: f64| s[((s.len() - 1) as f64 * q).round() as usize];
    format!(
        "mean {:.3} p50 {:.3} p95 {:.3} max {:.3} ms (n={})",
        s.iter().sum::<f64>() / s.len() as f64,
        pick(0.5),
        pick(0.95),
        s[s.len() - 1],
        s.len()
    )
}

fn decode_cmd(paths: &[String]) -> Result<()> {
    let (mut all_dec, mut all_rgba) = (Vec::new(), Vec::new());
    for p in paths {
        let path = Path::new(p);
        let video = demux(path)?;
        let mut dec = Decoder::new()?;
        let mut rgba = vec![0u8; video.width * video.height * 4];
        let (mut dec_t, mut rgba_t) = (Vec::new(), Vec::new());
        let start = Instant::now();
        for pkt in &video.packets {
            let t = Instant::now();
            let out = dec.decode(&pkt.annexb)?;
            let d = t.elapsed();
            if let Some(img) = out {
                dec_t.push(d);
                let t = Instant::now();
                to_rgba(&img, (video.width, video.height), &mut rgba)?;
                rgba_t.push(t.elapsed());
            }
        }
        let late = dec.flush_remaining()?.len();
        let wall = start.elapsed();
        let frame = video.end() / video.packets.len() as f64 * 1e3;
        println!(
            "{}: {}x{} {} packets, {} frames (+{late} at flush), budget {frame:.2} ms/frame, {:.2}x real time\n  decode {}\n  rgba   {}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            video.width,
            video.height,
            video.packets.len(),
            dec_t.len(),
            video.end() / wall.as_secs_f64(),
            stats(&dec_t),
            stats(&rgba_t),
        );
        all_dec.extend(dec_t);
        all_rgba.extend(rgba_t);
    }
    if paths.len() > 1 {
        println!(
            "all {} files:\n  decode {}\n  rgba   {}",
            paths.len(),
            stats(&all_dec),
            stats(&all_rgba)
        );
    }
    Ok(())
}

// ---- audio vs. video timestamps ---------------------------------------------

fn audio_cmd(mp4: &Path, mp3: &Path, reference: Option<&Path>) -> Result<()> {
    let video = demux(mp4)?;
    let source = AudioSource {
        bytes: Arc::from(std::fs::read(mp3)?),
    };
    // Bevy's own decoder, as `AudioPlayer` builds it (rodio, gapless on).
    let decoder = source.decoder();
    let (rate, channels) = (decoder.sample_rate().get(), decoder.channels().get());
    let pcm: Vec<f32> = decoder.collect();
    let audio_end = pcm.len() as f64 / f64::from(channels) / f64::from(rate);
    let frame = video.end() / video.packets.len() as f64;
    println!(
        "video: {} frames, ends {:.6} s ({:.3} ms/frame)\naudio: {rate} Hz x{channels}, {} samples, ends {audio_end:.6} s\nend delta (audio - video): {:+.3} ms = {:+.3} frames",
        video.packets.len(),
        video.end(),
        frame * 1e3,
        pcm.len(),
        (audio_end - video.end()) * 1e3,
        (audio_end - video.end()) / frame,
    );
    if let Some(r) = reference {
        let raw = if r == Path::new("-") {
            let mut v = Vec::new();
            std::io::Read::read_to_end(&mut std::io::stdin(), &mut v)?;
            v
        } else {
            std::fs::read(r)?
        };
        let reference: Vec<f32> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
            .collect();
        let mono = |v: &[f32]| -> Vec<f32> {
            v.chunks_exact(usize::from(channels))
                .map(|c| c.iter().sum::<f32>())
                .collect()
        };
        let (a, b) = (mono(&pcm), mono(&reference));
        // Best lag within ±100 ms over the first 4 s (normalised correlation).
        let max_lag = (rate / 10) as i64;
        let n = (rate as usize * 4).min(a.len()).min(b.len());
        let mut best = (0i64, f64::MIN);
        for lag in -max_lag..=max_lag {
            let (mut dot, mut ea, mut eb) = (0f64, 0f64, 0f64);
            for (i, y) in b.iter().enumerate().take(n) {
                let j = i as i64 + lag;
                if j < 0 || j as usize >= a.len() {
                    continue;
                }
                let (x, y) = (f64::from(a[j as usize]), f64::from(*y));
                dot += x * y;
                ea += x * x;
                eb += y * y;
            }
            let c = dot / (ea.sqrt() * eb.sqrt()).max(1e-12);
            if c > best.1 {
                best = (lag, c);
            }
        }
        println!(
            "source: {} samples/ch; MP3 - source {:+} samples/ch; best lag {:+} samples ({:+.3} ms), correlation {:.4}",
            b.len(),
            a.len() as i64 - b.len() as i64,
            best.0,
            best.0 as f64 / f64::from(rate) * 1e3,
            best.1
        );
    }
    Ok(())
}

// ---- headless Bevy player ---------------------------------------------------

struct Frame {
    pts: f64,
    rgba: Vec<u8>,
}

#[derive(Default)]
struct Log {
    set_data: Vec<Duration>,
    /// Spans on render frames that got a new video frame, and on the rest.
    prepare: Vec<Duration>,
    prepare_idle: Vec<Duration>,
    render_gap: Vec<Duration>,
    main_gap: Vec<Duration>,
    decode: Vec<Duration>,
    /// (wall s, audio s, shown pts) per update once the sink exists.
    samples: Vec<(f64, f64, f64)>,
    /// The wall clock's anchor: the smallest wall - position seen (s).
    anchor: f64,
    shown: usize,
    skipped: usize,
    audio_start: Option<Duration>,
}

#[derive(Resource, Clone)]
struct Shared(Arc<Mutex<Log>>);

#[derive(Resource)]
struct Player {
    rx: Receiver<Frame>,
    pending: Option<Frame>,
    video: Handle<Image>,
    upload: bool,
    wall_clock: bool,
    lag: f64,
    spawned: Instant,
    t0: Option<Instant>,
    /// Smallest wall - position seen; `--wall` presents by wall - anchor,
    /// decision 7's clock.
    anchor: f64,
    shown_pts: f64,
    total: usize,
    last_frame: Option<Instant>,
}

fn play_cmd(args: &[String]) -> Result<()> {
    let (mp4, mp3) = (Path::new(&args[0]), Path::new(&args[1]));
    let flag = |f: &str| args.iter().any(|a| a == f);
    let hz: f64 = args
        .iter()
        .position(|a| a == "--hz")
        .and_then(|i| args.get(i + 1))
        .map_or(Ok(60.0), |s| s.parse())?;
    let (upload, wall_clock, audible) = (!flag("--no-upload"), flag("--wall"), flag("--audible"));
    let windowed = flag("--window");
    // Holds video back behind the clock, for judging lip sync by eye.
    let lag_ms: f64 = args
        .iter()
        .position(|a| a == "--video-lag-ms")
        .and_then(|i| args.get(i + 1))
        .map_or(Ok(0.0), |s| s.parse())?;

    let video = demux(mp4)?;
    let (w, h, total) = (video.width, video.height, video.packets.len());
    let frame_dur = video.end() / total as f64;
    let log = Shared(Arc::new(Mutex::new(Log::default())));
    // Decode ahead on its own thread, as the real player would.
    let (tx, rx) = bounded::<Frame>(4);
    let dlog = log.clone();
    std::thread::spawn(move || {
        let run = || -> Result<()> {
            let mut dec = Decoder::new()?;
            for pkt in &video.packets {
                let t = Instant::now();
                if let Some(img) = dec.decode(&pkt.annexb)? {
                    let mut rgba = vec![0u8; w * h * 4];
                    to_rgba(&img, (w, h), &mut rgba)?;
                    dlog.0.lock().unwrap().decode.push(t.elapsed());
                    if tx.send(Frame { pts: pkt.pts, rgba }).is_err() {
                        break;
                    }
                }
            }
            Ok(())
        };
        // The player waits on every frame, so a decode error ends the run.
        if let Err(e) = run() {
            eprintln!("decode: {e:#}");
            std::process::exit(1);
        }
    });

    let mp3_bytes: Arc<[u8]> = Arc::from(std::fs::read(mp3)?);
    let wait = if hz > 0.0 {
        Duration::from_secs_f64(1.0 / hz)
    } else {
        Duration::ZERO
    };
    let mut app = App::new();
    if windowed {
        // Vsync-paced in a real window; needs an unlocked display to mean anything.
        app.add_plugins(DefaultPlugins);
    } else {
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(wait));
    }
    app.insert_resource(log.clone())
        .add_systems(
            Startup,
            move |mut commands: Commands,
                  mut images: ResMut<Assets<Image>>,
                  mut sounds: ResMut<Assets<AudioSource>>| {
                if windowed {
                    commands.spawn(Camera2d);
                } else {
                    // A 1080p offscreen target stands in for the TV.
                    let target = images.add(Image::new_target_texture(
                        1920,
                        1080,
                        TextureFormat::Rgba8UnormSrgb,
                        None,
                    ));
                    commands.spawn((Camera2d, RenderTarget::Image(target.into())));
                }
                let frame = images.add(Image::new_fill(
                    Extent3d {
                        width: w as u32,
                        height: h as u32,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    &[0, 0, 0, 255],
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::RENDER_WORLD,
                ));
                commands.spawn((
                    Node {
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    ImageNode::new(frame.clone()),
                ));
                let sound = sounds.add(AudioSource {
                    bytes: mp3_bytes.clone(),
                });
                let settings = if audible {
                    PlaybackSettings::ONCE
                } else {
                    PlaybackSettings::ONCE.muted()
                };
                commands.spawn((AudioPlayer(sound), settings));
                commands.insert_resource(Player {
                    rx: rx.clone(),
                    pending: None,
                    video: frame,
                    upload,
                    wall_clock,
                    lag: lag_ms / 1e3,
                    spawned: Instant::now(),
                    t0: None,
                    anchor: f64::MAX,
                    shown_pts: -1.0,
                    total,
                    last_frame: None,
                });
            },
        )
        .add_systems(Update, present);

    // Render-world timing: the asset-prepare span (where a changed Image's
    // bytes go to `write_texture`) and the gap between render frames.
    let render = app.sub_app_mut(RenderApp);
    render
        .insert_resource(log.clone())
        .init_resource::<PrepStart>()
        .add_systems(
            Render,
            (
                prepare_start.before(prepare_assets::<GpuImage>),
                prepare_end.after(prepare_assets::<GpuImage>),
            )
                .in_set(RenderSystems::PrepareAssets),
        );
    app.run();

    let log = log.0.lock().unwrap();
    report(&log, upload, wall_clock, hz, frame_dur);
    if let Some(path) = args
        .iter()
        .position(|a| a == "--csv")
        .and_then(|i| args.get(i + 1))
    {
        let rows: String = log
            .samples
            .iter()
            .map(|(w, a, p)| format!("{w:.6},{a:.6},{p:.6}\n"))
            .collect();
        std::fs::write(path, format!("wall,audio,shown_pts\n{rows}"))?;
    }
    Ok(())
}

/// When this render frame's asset-prepare span began.
#[derive(Resource, Default)]
struct PrepStart(Option<Instant>, bool);

fn prepare_start(mut start: ResMut<PrepStart>, extracted: Res<ExtractedAssets<GpuImage>>) {
    *start = PrepStart(Some(Instant::now()), !extracted.extracted.is_empty());
}

fn prepare_end(start: Res<PrepStart>, log: Res<Shared>, mut last: Local<Option<Instant>>) {
    let now = Instant::now();
    let mut l = log.0.lock().unwrap();
    if let PrepStart(Some(s), image) = *start {
        if image {
            l.prepare.push(now - s);
        } else {
            l.prepare_idle.push(now - s);
        }
    }
    if let Some(prev) = last.replace(now) {
        l.render_gap.push(now - prev);
    }
}

fn present(
    mut player: ResMut<Player>,
    mut images: ResMut<Assets<Image>>,
    sinks: Query<&AudioSink>,
    log: Res<Shared>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(sink) = sinks.single() else {
        return;
    };
    let now = Instant::now();
    let t0 = *player.t0.get_or_insert(now);
    let mut l = log.0.lock().unwrap();
    if l.audio_start.is_none() {
        l.audio_start = Some(now - player.spawned);
    }
    if let Some(prev) = player.last_frame.replace(now) {
        l.main_gap.push(now - prev);
    }
    let wall = (now - t0).as_secs_f64();
    let audio = sink.position().as_secs_f64();
    player.anchor = player.anchor.min(wall - audio);
    l.anchor = player.anchor;
    let clock = if player.wall_clock {
        wall - player.anchor
    } else {
        audio
    } - player.lag;

    // The newest decoded frame due by `clock`; older due ones are skipped.
    let mut due = None;
    loop {
        if player.pending.is_none() {
            match player.rx.try_recv() {
                Ok(f) => player.pending = Some(f),
                // The decode thread died (a panic) before the last frame;
                // `due` holds one not yet counted.
                Err(TryRecvError::Disconnected)
                    if l.shown + l.skipped + usize::from(due.is_some()) < player.total =>
                {
                    eprintln!(
                        "decode thread ended after {} of {} frames",
                        l.shown + l.skipped + usize::from(due.is_some()),
                        player.total
                    );
                    exit.write(AppExit::error());
                    return;
                }
                Err(_) => {}
            }
        }
        match &player.pending {
            Some(f) if f.pts <= clock => {
                if due.is_some() {
                    l.skipped += 1;
                }
                due = player.pending.take();
            }
            _ => break,
        }
    }
    if let Some(f) = due {
        player.shown_pts = f.pts;
        l.shown += 1;
        if player.upload {
            let t = Instant::now();
            if let Some(mut img) = images.get_mut(&player.video) {
                img.data = Some(f.rgba);
            }
            l.set_data.push(t.elapsed());
        }
    }
    l.samples.push((wall, audio, player.shown_pts));
    if l.shown + l.skipped >= player.total && sink.empty() {
        exit.write(AppExit::Success);
    }
}

fn report(l: &Log, upload: bool, wall_clock: bool, hz: f64, frame: f64) {
    println!(
        "mode: upload={upload} clock={} loop={}",
        if wall_clock { "wall" } else { "audio" },
        if hz > 0.0 {
            format!("{hz} Hz")
        } else {
            "uncapped".into()
        }
    );
    println!("decode+rgba (thread): {}", stats(&l.decode));
    println!("set Image data (main): {}", stats(&l.set_data));
    println!(
        "prepare_assets span (render), frames with an Image: {}",
        stats(&l.prepare)
    );
    println!("  without: {}", stats(&l.prepare_idle));
    // The first second holds pipeline compiles; the steady state is the rest.
    let warm = |v: &[Duration]| {
        let mut t = Duration::ZERO;
        let skip = v.iter().take_while(|d| {
            t += **d;
            t < Duration::from_secs(1)
        });
        v.len().min(skip.count())
    };
    println!(
        "main frame gap: {}\n  after 1 s: {}",
        stats(&l.main_gap),
        stats(&l.main_gap[warm(&l.main_gap)..])
    );
    println!(
        "render frame gap: {}\n  after 1 s: {}",
        stats(&l.render_gap),
        stats(&l.render_gap[warm(&l.render_gap)..])
    );
    println!(
        "frames shown {} skipped {}; audio sink after {:.1} ms",
        l.shown,
        l.skipped,
        l.audio_start.map_or(f64::NAN, ms)
    );
    // Drift: the audio clock against the shown frame and against the wall.
    let lag: Vec<f64> = l
        .samples
        .iter()
        .filter(|s| s.2 >= 0.0)
        .map(|s| (s.1 - s.2) * 1e3)
        .collect();
    if let (Some(first), Some(last)) = (l.samples.first(), l.samples.last()) {
        let last_shown = l
            .samples
            .iter()
            .rev()
            .find(|s| s.2 >= 0.0)
            .copied()
            .unwrap_or(*last);
        println!(
            "at first update: wall {:.3} audio {:.3}; at the end: wall {:.3} s audio {:.3} s (wall - audio {:+.2} ms)",
            first.0,
            first.1,
            last.0,
            last.1,
            (last.0 - last.1) * 1e3
        );
        // The last update before the final frame changed: audio vs shown pts.
        let last_frame_on = l
            .samples
            .iter()
            .find(|s| s.2 == last_shown.2)
            .copied()
            .unwrap_or(last_shown);
        println!(
            "last frame (pts {:.3}) shown at audio {:.3} wall {:.3}: audio - pts {:+.2} ms ({:+.2} frames), wall - pts {:+.2} ms",
            last_frame_on.2,
            last_frame_on.1,
            last_frame_on.0,
            (last_frame_on.1 - last_frame_on.2) * 1e3,
            (last_frame_on.1 - last_frame_on.2) / frame,
            (last_frame_on.0 - last_frame_on.2) * 1e3,
        );
    }
    if !lag.is_empty() {
        let max = lag.iter().copied().fold(f64::MIN, f64::max);
        let min = lag.iter().copied().fold(f64::MAX, f64::min);
        println!("audio - shown pts over the run: min {min:+.2} max {max:+.2} ms");
    }
    println!(
        "wall clock anchor (min wall - position): {:+.2} ms",
        l.anchor * 1e3
    );
    clock_edges(&l.samples);
}

/// The sink's position moves in whole device buffers, so the raw clock steps.
/// Each step is a moment where position and wall are both known (to within
/// one update), so wall - position at the steps gives the clocks' drift.
fn clock_edges(samples: &[(f64, f64, f64)]) {
    let mut edges = Vec::new();
    for pair in samples.windows(2) {
        if pair[1].1 > pair[0].1 && pair[1].1 < samples.last().map_or(0.0, |s| s.1) {
            edges.push((pair[1].0, pair[1].1));
        }
    }
    if edges.len() < 20 {
        println!("clock edges: too few ({})", edges.len());
        return;
    }
    let steps: Vec<f64> = edges.windows(2).map(|e| (e[1].1 - e[0].1) * 1e3).collect();
    let step = steps.iter().sum::<f64>() / steps.len() as f64;
    // Detection only ever lags an edge, so the minimum offset in a window is
    // the best estimate of the true one.
    let window = |part: &[(f64, f64)]| {
        part.iter()
            .map(|e| (e.0 - e.1) * 1e3)
            .fold(f64::MAX, f64::min)
    };
    let n = edges.len() / 4;
    let (start, end) = (window(&edges[..n]), window(&edges[edges.len() - n..]));
    let span = edges[edges.len() - 1].1 - edges[0].1;
    println!(
        "clock edges: {} steps of {step:.2} ms; wall - audio at the first quarter {start:+.2} ms, last quarter {end:+.2} ms: drift {:+.2} ms over {span:.2} s ({:+.0} ppm)",
        edges.len(),
        end - start,
        (end - start) / 1e3 / span * 1e6,
    );
}
