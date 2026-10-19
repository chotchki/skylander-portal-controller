//! Plays what [`IntroQueue`] says (openspec AB, design decisions 7 and 9):
//! the extract's MP4 decoded on its own thread into a Bevy `Image`, full
//! screen over the portal view, and its MP3 through Bevy audio. Frames are
//! presented by the wall clock anchored to the audio sink's position steps,
//! held back by [`OutputLatency`]; when the intro ends the overlay goes and
//! the cards are there under it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackSettings};
use bevy::ecs::entity::Entities;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use crossbeam_channel::{Receiver, TryRecvError};
use tracing::{error, info, warn};

use crate::cards::PortalChanged;
use crate::driver::PortalChange;
use crate::extraction::IntroLibrary;
use crate::intro::{self, Change, IntroEntry, IntroEvent, IntroQueue};
use crate::screen::TvScreen;
use crate::tv::Current;
use crate::video::{self, Feed};

/// The speaker's lag behind the audio clock when none is stored. PROVISIONAL
/// until the TV pass in `openspec/backlog.md` (ex-AB.16 and ex-AB.19) measures
/// M0's path (HDMI to the TV) and sets it: one rodio device buffer (2048
/// frames at 48 kHz, 42.7 ms, AB.7's step), without the output path's own
/// latency.
pub const DEFAULT_OUTPUT_LATENCY: Duration = Duration::from_millis(43);

/// Waited for an audio sink before playing the video by the wall clock alone
/// (no output device). A sound that won't open never gets a player.
const NO_SINK_AFTER: Duration = Duration::from_millis(500);

/// Past the video's end by this, the intro ends whatever the sink says.
const END_GRACE: Duration = Duration::from_secs(2);

/// An intro that never opens its video by now has failed.
const OPEN_WITHIN: Duration = Duration::from_secs(10);

/// Over the portal screen, whose cards keep changing under it.
const OVERLAY_Z: i32 = 1_000;

/// The intro's video held back by this (`output_latency_ms` in settings).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputLatency(pub Duration);

impl Default for OutputLatency {
    fn default() -> Self {
        Self(DEFAULT_OUTPUT_LATENCY)
    }
}

/// The full-screen backdrop an intro plays on.
#[derive(Component)]
pub(crate) struct IntroOverlay;

/// The intro's picture inside the overlay.
#[derive(Component)]
pub(crate) struct IntroVideo;

/// Decision 7's clock: the wall clock since the sink appeared, anchored to
/// the smallest wall − position seen. The position steps a device buffer at a
/// time, so presenting by it skips frames; the anchor keeps the wall clock on
/// the steps' leading edge without drifting from the audio.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct AnchoredClock {
    t0: Option<Instant>,
    /// Seconds; none until a position is seen.
    anchor: Option<f64>,
}

impl AnchoredClock {
    /// Seconds into the intro at `now`. `position` is the sink's; `None`
    /// runs on the wall clock alone.
    pub fn tick(&mut self, now: Instant, position: Option<Duration>) -> f64 {
        let t0 = *self.t0.get_or_insert(now);
        let wall = now.saturating_duration_since(t0).as_secs_f64();
        if let Some(position) = position {
            let offset = wall - position.as_secs_f64();
            self.anchor = Some(self.anchor.map_or(offset, |a| a.min(offset)));
        }
        wall - self.anchor.unwrap_or(0.0)
    }

    pub fn anchor(&self) -> Option<f64> {
        self.anchor
    }
}

enum Audio {
    /// Starts with the first decoded frame, so the video isn't late.
    Waiting,
    Spawned {
        entity: Entity,
        at: Instant,
    },
    Sink(Entity),
    /// Wall clock only.
    Silent,
}

struct Opened {
    frames: usize,
    /// Seconds.
    end: f64,
}

struct Playback {
    entry: IntroEntry,
    rx: Receiver<Feed>,
    mp3: Option<PathBuf>,
    started: Instant,
    overlay: Entity,
    image: Option<Handle<Image>>,
    opened: Option<Opened>,
    pending: Option<(f64, Vec<u8>)>,
    decoded: bool,
    audio: Audio,
    clock: AnchoredClock,
    shown: usize,
    skipped: usize,
}

/// Why a playback ended on its own.
#[derive(Debug)]
enum Over {
    Finished,
    Failed(String),
}

/// The playing intro, if any.
#[derive(Resource, Default)]
pub struct IntroPlayer {
    current: Option<Playback>,
    muted: bool,
}

/// The playing intro as the mirror reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayingNow {
    pub entry: IntroEntry,
    /// The video's frame count, once open.
    pub frames: Option<usize>,
    pub shown: usize,
    pub skipped: usize,
    pub audio: &'static str,
}

impl IntroPlayer {
    pub fn now(&self) -> Option<PlayingNow> {
        let p = self.current.as_ref()?;
        Some(PlayingNow {
            entry: p.entry.clone(),
            frames: p.opened.as_ref().map(|o| o.frames),
            shown: p.shown,
            skipped: p.skipped,
            audio: match p.audio {
                Audio::Waiting => "waiting",
                Audio::Spawned { .. } => "starting",
                Audio::Sink(_) => "playing",
                Audio::Silent => "none",
            },
        })
    }
}

pub struct IntroPlugin;

impl Plugin for IntroPlugin {
    fn build(&self, app: &mut App) {
        // Dev only: intros muted on a machine nobody is listening to.
        let muted = cfg!(feature = "dev-tools") && std::env::var_os("GUAC_MUTE").is_some();
        app.init_resource::<IntroQueue>()
            .init_resource::<OutputLatency>()
            .insert_resource(IntroPlayer {
                current: None,
                muted,
            })
            .add_systems(
                Update,
                (queue_intros, play_intros, overlay_holds_focus).chain(),
            );
    }
}

/// While an intro plays, its overlay holds the input focus, so an Enter or
/// Space the widgets would take lands on a node with no button; the focus
/// goes back to what had it when the last intro in a row stops, unless
/// something else took it meanwhile (a new screen's button, focused as it
/// spawned).
fn overlay_holds_focus(
    overlays: Query<Entity, With<IntroOverlay>>,
    focus: Option<ResMut<InputFocus>>,
    entities: &Entities,
    // What had the focus before the first intro in a row, and the overlay
    // that has it now.
    mut held: Local<Option<(Option<Entity>, Entity)>>,
) {
    let Some(mut focus) = focus else {
        return;
    };
    match overlays.iter().next() {
        Some(overlay) => {
            if focus.get() != Some(overlay) {
                // The next intro in a row keeps what the first one took.
                let before = held.map_or(focus.get(), |(before, _)| before);
                *held = Some((before, overlay));
                focus.set(overlay, FocusCause::Navigated);
            }
        }
        None => {
            let Some((was, overlay)) = held.take() else {
                return;
            };
            if focus.get().is_some_and(|now| now != overlay) {
                return;
            }
            match was.filter(|e| entities.contains(*e)) {
                Some(entity) => focus.set(entity, FocusCause::Navigated),
                None if focus.get().is_some() => focus.clear(),
                None => {}
            }
        }
    }
}

/// bevy_audio opens a sound the same way and unwraps it on the main thread,
/// so a file that fails here must never reach it.
fn sound_opens(bytes: &Arc<[u8]>) -> Result<(), rodio::decoder::DecoderError> {
    rodio::Decoder::builder()
        .with_byte_len(bytes.len() as u64)
        .with_data(std::io::Cursor::new(bytes.clone()))
        .build()
        .map(drop)
}

fn log(changes: &[Change]) {
    for change in changes {
        let (what, e) = match change {
            Change::Queued(e) => ("intro queued", e),
            Change::Started(e) => ("intro starts", e),
            Change::Dropped(e) => ("intro dropped before its turn", e),
            Change::Stopped(e, why) => {
                info!(id = e.id, slot = e.slot.display(), movie = %e.movie, ?why, "intro stops");
                continue;
            }
        };
        info!(id = e.id, slot = e.slot.display(), movie = %e.movie, "{what}");
    }
}

pub(crate) fn apply(queue: &mut ResMut<IntroQueue>, event: IntroEvent) {
    let changes = queue.bypass_change_detection().apply(event);
    if !changes.is_empty() {
        log(&changes);
        queue.set_changed();
    }
}

/// Placements and removals into the queue; a skip comes from the input
/// router (`input.rs`). A placement made before the extract is ready has no
/// movie, so it stays a card.
fn queue_intros(
    mut heard: MessageReader<PortalChanged>,
    current: Option<Res<Current>>,
    library: Option<Res<IntroLibrary>>,
    mut queue: ResMut<IntroQueue>,
) {
    for PortalChanged(change) in heard.read() {
        let event = match change {
            PortalChange::Placed { slot, placement } => IntroEvent::Placed {
                slot: *slot,
                figure_id: placement.figure_id.as_str().to_string(),
                movie: intro::movie_for(library.as_deref(), placement),
            },
            PortalChange::Cleared { slot } => IntroEvent::Cleared { slot: *slot },
        };
        apply(&mut queue, event);
    }
    let on_portal = current.is_some_and(|c| matches!(c.screen, TvScreen::Portal { .. }));
    if !on_portal && (queue.playing().is_some() || queue.queued().next().is_some()) {
        apply(&mut queue, IntroEvent::Reset);
    }
}

/// Starts, steps and stops the playback so it follows the queue.
pub(crate) fn play_intros(
    mut commands: Commands,
    mut queue: ResMut<IntroQueue>,
    mut player: ResMut<IntroPlayer>,
    library: Option<Res<IntroLibrary>>,
    latency: Res<OutputLatency>,
    mut images: ResMut<Assets<Image>>,
    mut sounds: Option<ResMut<Assets<AudioSource>>>,
    sinks: Query<&AudioSink>,
) {
    let now = Instant::now();
    // Twice at most: an intro that ends starts the next in the same frame.
    for _ in 0..2 {
        let want = queue.playing().cloned();
        if player.current.as_ref().map(|p| p.entry.id) != want.as_ref().map(|e| e.id) {
            if let Some(old) = player.current.take() {
                old.stop(&mut commands, &mut images, &sinks, "the queue moved on");
            }
            player.current = want.map(|entry| start(&mut commands, entry, library.as_deref(), now));
        }
        let muted = player.muted;
        let Some(playback) = player.current.as_mut() else {
            return;
        };
        let over = playback.step(
            now,
            latency.0,
            muted,
            &mut commands,
            &mut images,
            sounds.as_deref_mut(),
            &sinks,
        );
        let Some(over) = over else {
            return;
        };
        let done = player.current.take().expect("stepped above");
        let why = match over {
            Over::Finished => "finished".to_string(),
            Over::Failed(why) => {
                error!(movie = %done.entry.movie, "intro failed: {why}");
                format!("failed: {why}")
            }
        };
        let id = done.entry.id;
        done.stop(&mut commands, &mut images, &sinks, &why);
        apply(&mut queue, IntroEvent::Ended { id });
    }
}

fn start(
    commands: &mut Commands,
    entry: IntroEntry,
    library: Option<&IntroLibrary>,
    now: Instant,
) -> Playback {
    let overlay = commands
        .spawn((
            IntroOverlay,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::BLACK),
            GlobalZIndex(OVERLAY_Z),
        ))
        .id();
    let (rx, mp3) = match library {
        Some(library) => {
            let (mp4, mp3) = intro::files(library, &entry.movie);
            info!(id = entry.id, video = %mp4.display(), "intro opening");
            (video::spawn_decoder(mp4), Some(mp3))
        }
        // A movie only ever comes from a library; fail rather than panic.
        None => {
            let (tx, rx) = crossbeam_channel::bounded(1);
            let _ = tx.send(Feed::Failed("no extract to play from".into()));
            (rx, None)
        }
    };
    Playback {
        entry,
        rx,
        mp3,
        started: now,
        overlay,
        image: None,
        opened: None,
        pending: None,
        decoded: false,
        audio: Audio::Waiting,
        clock: AnchoredClock::default(),
        shown: 0,
        skipped: 0,
    }
}

impl Playback {
    /// Fills `pending` from the decode thread if it's empty.
    fn pull(&mut self, commands: &mut Commands, images: &mut Assets<Image>) -> Result<(), String> {
        while self.pending.is_none() && !self.decoded {
            match self.rx.try_recv() {
                Ok(Feed::Opened {
                    width,
                    height,
                    frames,
                    end,
                }) => {
                    let image = images.add(Image::new_fill(
                        Extent3d {
                            width: width as u32,
                            height: height as u32,
                            depth_or_array_layers: 1,
                        },
                        TextureDimension::D2,
                        &[0, 0, 0, 255],
                        TextureFormat::Rgba8UnormSrgb,
                        RenderAssetUsages::RENDER_WORLD,
                    ));
                    commands.spawn((
                        IntroVideo,
                        ImageNode::new(image.clone()),
                        Node {
                            width: percent(100),
                            max_height: percent(100),
                            aspect_ratio: Some(width as f32 / height as f32),
                            ..default()
                        },
                        ChildOf(self.overlay),
                    ));
                    self.image = Some(image);
                    self.opened = Some(Opened { frames, end });
                    info!(
                        id = self.entry.id,
                        width,
                        height,
                        frames,
                        end_s = end,
                        after_ms = self.started.elapsed().as_secs_f64() * 1e3,
                        "intro video open"
                    );
                }
                Ok(Feed::Frame { pts, rgba }) => self.pending = Some((pts, rgba)),
                Ok(Feed::Failed(why)) => return Err(why),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => self.decoded = true,
            }
        }
        Ok(())
    }

    /// One frame of playback; `Some` when it's over.
    fn step(
        &mut self,
        now: Instant,
        latency: Duration,
        muted: bool,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        sounds: Option<&mut Assets<AudioSource>>,
        sinks: &Query<&AudioSink>,
    ) -> Option<Over> {
        if let Err(why) = self.pull(commands, images) {
            return Some(Over::Failed(why));
        }
        let Some((frames, end)) = self.opened.as_ref().map(|o| (o.frames, o.end)) else {
            if self.decoded {
                return Some(Over::Failed(
                    "the decode thread ended before opening".into(),
                ));
            }
            if now.saturating_duration_since(self.started) > OPEN_WITHIN {
                return Some(Over::Failed(format!("not open after {OPEN_WITHIN:?}")));
            }
            return None;
        };
        if matches!(self.audio, Audio::Waiting) && (self.pending.is_some() || self.decoded) {
            self.audio = self.start_audio(now, muted, commands, sounds);
        }
        if let Audio::Spawned { entity, at } = self.audio {
            if sinks.contains(entity) {
                self.audio = Audio::Sink(entity);
            } else if now.saturating_duration_since(at) > NO_SINK_AFTER {
                warn!(
                    id = self.entry.id,
                    "no audio sink; the intro plays by the wall clock"
                );
                // Else it holds the sound for good, and plays if a sink comes late.
                commands.entity(entity).despawn();
                self.audio = Audio::Silent;
            }
        }
        let clock = match self.audio {
            Audio::Sink(entity) => {
                let position = sinks.get(entity).ok().map(|s| s.position());
                self.clock.tick(now, position)
            }
            Audio::Silent => self.clock.tick(now, None),
            Audio::Waiting | Audio::Spawned { .. } => return None,
        };
        let at = clock - latency.as_secs_f64();
        // The newest frame due; older due ones are skipped.
        let mut due = None;
        loop {
            if let Err(why) = self.pull(commands, images) {
                return Some(Over::Failed(why));
            }
            match &self.pending {
                Some((pts, _)) if *pts <= at => {
                    if due.is_some() {
                        self.skipped += 1;
                    }
                    due = self.pending.take();
                }
                _ => break,
            }
        }
        if let Some((pts, rgba)) = due {
            self.shown += 1;
            if self.shown == 1 {
                info!(
                    id = self.entry.id,
                    movie = %self.entry.movie,
                    pts,
                    since_start_ms = now.saturating_duration_since(self.started).as_secs_f64() * 1e3,
                    latency_ms = latency.as_secs_f64() * 1e3,
                    "intro first frame"
                );
            }
            if let Some(mut image) = self.image.as_ref().and_then(|h| images.get_mut(h)) {
                image.data = Some(rgba);
            }
        }
        if self.decoded && self.pending.is_none() {
            let out = self.shown + self.skipped;
            if out < frames {
                return Some(Over::Failed(format!(
                    "the decode thread stopped after {out} of {frames} frames"
                )));
            }
            let audio_done = match self.audio {
                Audio::Sink(entity) => sinks.get(entity).map_or(true, |s| s.empty()),
                _ => true,
            };
            if audio_done {
                return Some(Over::Finished);
            }
        }
        if at > end + END_GRACE.as_secs_f64() {
            warn!(id = self.entry.id, "intro ran past its end; ending it");
            return Some(Over::Finished);
        }
        None
    }

    fn start_audio(
        &self,
        now: Instant,
        muted: bool,
        commands: &mut Commands,
        sounds: Option<&mut Assets<AudioSource>>,
    ) -> Audio {
        let (Some(path), Some(sounds)) = (&self.mp3, sounds) else {
            return Audio::Silent;
        };
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!(path = %path.display(), "intro sound didn't load, playing it silent: {e}");
                return Audio::Silent;
            }
        };
        let bytes: Arc<[u8]> = Arc::from(bytes);
        if let Err(e) = sound_opens(&bytes) {
            warn!(path = %path.display(), "intro sound won't open, playing it silent: {e}");
            return Audio::Silent;
        }
        let sound = sounds.add(AudioSource { bytes });
        let settings = if muted {
            PlaybackSettings::ONCE.muted()
        } else {
            PlaybackSettings::ONCE
        };
        let entity = commands.spawn((AudioPlayer(sound), settings)).id();
        Audio::Spawned { entity, at: now }
    }

    /// Takes the overlay, sound and picture down. Dropping `rx` stops the
    /// decode thread at its next frame.
    fn stop(
        self,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        sinks: &Query<&AudioSink>,
        why: &str,
    ) {
        commands.entity(self.overlay).despawn();
        if let Audio::Spawned { entity, .. } | Audio::Sink(entity) = self.audio {
            if let Ok(sink) = sinks.get(entity) {
                sink.stop();
            }
            commands.entity(entity).despawn();
        }
        if let Some(image) = &self.image {
            images.remove(image);
        }
        info!(
            id = self.entry.id,
            movie = %self.entry.movie,
            shown = self.shown,
            skipped = self.skipped,
            frames = self.opened.as_ref().map(|o| o.frames),
            anchor_ms = self.clock.anchor().map(|a| a * 1e3),
            played_ms = self.started.elapsed().as_secs_f64() * 1e3,
            why,
            "intro over"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn the_clock_anchors_to_the_smallest_offset_and_holds_it() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let pos = |ms: u64| Some(Duration::from_millis(ms));
        let mut clock = AnchoredClock::default();
        assert_eq!(clock.tick(at(0), pos(0)), 0.0);
        assert!(near(clock.tick(at(10), pos(0)), 0.010));
        // The sink steps to 42 ms at wall 30: anchored at -12 ms, the clock
        // jumps to the step's leading edge.
        assert!(near(clock.tick(at(30), pos(42)), 0.042));
        assert!(near(clock.anchor().unwrap(), -0.012));
        // Between steps the position stands still and the clock runs on.
        assert!(near(clock.tick(at(50), pos(42)), 0.062));
        // A step seen late moves nothing: wall − position only grew.
        assert!(near(clock.tick(at(80), pos(84)), 0.092));
        assert!(near(clock.anchor().unwrap(), -0.012));
    }

    #[test]
    fn without_a_sink_the_clock_is_the_wall() {
        let t0 = Instant::now();
        let mut clock = AnchoredClock::default();
        assert_eq!(clock.tick(t0, None), 0.0);
        assert!(near(
            clock.tick(t0 + Duration::from_millis(250), None),
            0.25
        ));
        assert_eq!(clock.anchor(), None);
    }

    /// Silent MPEG-1 Layer III, 128 kb/s mono at 44.1 kHz: 417-byte frames,
    /// side info all zero (no main data).
    fn silent_mp3(frames: usize) -> Vec<u8> {
        let mut frame = vec![0u8; 417];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0xC0]);
        frame.repeat(frames)
    }

    #[test]
    fn a_sound_that_wont_open_is_caught_before_bevy() {
        assert!(sound_opens(&Arc::from(silent_mp3(20))).is_ok());
        for junk in [
            Vec::new(),
            b"not an mp3 at all, a hand edit\n".to_vec(),
            vec![0u8; 64 << 10],
        ] {
            assert!(sound_opens(&Arc::from(junk)).is_err());
        }
    }

    #[test]
    fn the_default_latency_is_one_device_buffer() {
        let buffer = 2048.0 / 48_000.0 * 1e3;
        let ms = OutputLatency::default().0.as_secs_f64() * 1e3;
        assert!((ms - buffer).abs() < 1.0, "{ms} vs {buffer}");
    }

    /// Headless: the portal screen laid out by `bevy_ui` with the intro
    /// plugin and the mirror, over an extract built in code. No audio plugin,
    /// so intros play by the wall clock.
    mod flow {
        use std::path::Path;

        use bevy::input::ButtonState;
        use bevy::input::gamepad::{
            GamepadButton, GamepadConnection, GamepadConnectionEvent, RawGamepadButtonChangedEvent,
            RawGamepadEvent,
        };
        use bevy::input::keyboard::{Key, KeyboardInput};
        use guac_ir::{Character, CharacterTable, DumpIdentity, Intro, IntroSource, VoiceTrack};
        use guac_server::config::WindowMode;
        use skylander_core::{FigureId, SlotIndex};
        use skylander_rpcs3_control::Placement;

        use super::*;
        use crate::cards::PortalCards;
        use crate::mirror::{Mirror, TvMirror, TvSnapshot};
        use crate::tv::layout_tests::portal_app;
        use crate::video::test_clip;

        /// Toy types 9 and 10 have intros, 205 (a magic item) none.
        fn library(dir: &Path, clips: &[(&str, usize)]) -> IntroLibrary {
            let video = dir.join("video/intro");
            std::fs::create_dir_all(&video).unwrap();
            for (movie, frames) in clips {
                std::fs::write(
                    video.join(format!("{movie}.mp4")),
                    test_clip(64, 36, *frames, 25),
                )
                .unwrap();
            }
            let intro = |movie: &str| Character {
                archives: Vec::new(),
                vo_name: None,
                intro: Some(Intro {
                    movie: movie.into(),
                    source: IntroSource::Disc,
                    voice: VoiceTrack::Only,
                }),
            };
            IntroLibrary {
                dir: dir.to_path_buf(),
                table: CharacterTable {
                    identity: DumpIdentity {
                        title_id: "BLUS30968".into(),
                        app_ver: "01.00".into(),
                        manifest_hash: guac_ir::Sha256([0; 32]),
                    },
                    characters: [(9, intro("eruptor")), (10, intro("ignitor"))].into(),
                },
            }
        }

        struct Tv {
            app: App,
            mirror: TvMirror,
        }

        fn tv(library: Option<IntroLibrary>) -> Tv {
            let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
            let mirror = TvMirror::default();
            app.add_message::<PortalChanged>()
                .insert_resource(mirror.clone())
                .add_plugins((IntroPlugin, Mirror));
            if let Some(library) = library {
                app.insert_resource(library);
            }
            app.update();
            Tv { app, mirror }
        }

        fn figure(id: &str) -> Placement {
            let figure_id = FigureId::new(id);
            let (toy_type, variant) = figure_id.subtype_parts().unwrap();
            Placement {
                figure_id,
                toy_type,
                variant,
                name: id.into(),
                element: None,
                portrait: None,
                owner: None,
            }
        }

        impl Tv {
            fn shot(&self) -> TvSnapshot {
                self.mirror.0.lock().unwrap().clone()
            }

            /// As the driver's channel would: the card and the message.
            fn change(&mut self, change: PortalChange) {
                let world = self.app.world_mut();
                let cards = world
                    .resource::<PortalCards>()
                    .clone()
                    .reduce([change.clone()]);
                world.insert_resource(cards);
                world.write_message(PortalChanged(change));
                self.app.update();
            }

            fn place(&mut self, slot: u8, id: &str) {
                let slot = SlotIndex::new(slot).unwrap();
                self.change(PortalChange::Placed {
                    slot,
                    placement: figure(id),
                });
            }

            fn clear(&mut self, slot: u8) {
                let slot = SlotIndex::new(slot).unwrap();
                self.change(PortalChange::Cleared { slot });
            }

            /// Frames until `done` holds for the mirror, up to 5 s.
            fn until(&mut self, what: &str, done: impl Fn(&TvSnapshot) -> bool) -> TvSnapshot {
                let start = Instant::now();
                loop {
                    self.app.update();
                    let shot = self.shot();
                    if done(&shot) {
                        return shot;
                    }
                    assert!(
                        start.elapsed() < Duration::from_secs(5),
                        "{what}: {shot:#?}"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
            }

            fn movie(&self) -> Option<String> {
                self.shot().intro.map(|i| i.movie)
            }

            fn overlays(&mut self) -> usize {
                let world = self.app.world_mut();
                world.query::<&IntroOverlay>().iter(world).count()
            }

            fn players(&mut self) -> usize {
                let world = self.app.world_mut();
                world.query::<&AudioPlayer>().iter(world).count()
            }
        }

        /// Sound assets but no output device, as on a machine with none.
        fn tv_with_sound(dir: &Path, mp3: &[u8]) -> Tv {
            let lib = library(dir, &[("eruptor", 100)]);
            let audio = dir.join("audio/intro");
            std::fs::create_dir_all(&audio).unwrap();
            std::fs::write(audio.join("eruptor.mp3"), mp3).unwrap();
            let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
            let mirror = TvMirror::default();
            app.add_message::<PortalChanged>()
                .init_asset::<AudioSource>()
                .insert_resource(mirror.clone())
                .insert_resource(lib)
                .add_plugins((IntroPlugin, Mirror));
            app.update();
            Tv { app, mirror }
        }

        #[test]
        fn a_sound_that_wont_open_gets_no_player() {
            let tmp = tempfile::tempdir().unwrap();
            let mut tv = tv_with_sound(tmp.path(), b"");
            tv.place(0, "000009-0000");
            let shot = tv.until("frames", |s| s.intro.as_ref().is_some_and(|i| i.shown > 0));
            assert_eq!(shot.intro.unwrap().audio, "none");
            assert_eq!(tv.players(), 0);
        }

        #[test]
        fn with_no_sink_the_player_goes_and_the_video_plays_on() {
            let tmp = tempfile::tempdir().unwrap();
            let mut tv = tv_with_sound(tmp.path(), &super::silent_mp3(20));
            tv.place(0, "000009-0000");
            tv.until("a player", |s| {
                s.intro.as_ref().is_some_and(|i| i.audio == "starting")
            });
            assert_eq!(tv.players(), 1);
            tv.until("no sink", |s| {
                s.intro.as_ref().is_some_and(|i| i.audio == "none")
            });
            tv.app.update();
            assert_eq!(tv.players(), 0, "the player outlived its sink's wait");
            assert!(tv.shot().intro.is_some());
        }

        #[test]
        fn a_placement_plays_full_screen_then_the_card_is_there() {
            let tmp = tempfile::tempdir().unwrap();
            let mut tv = tv(Some(library(tmp.path(), &[("eruptor", 10)])));
            let placed = Instant::now();
            tv.place(0, "000009-0000");
            let shot = tv.until("the first frame", |s| {
                s.intro.as_ref().is_some_and(|i| i.shown > 0)
            });
            assert!(placed.elapsed() < Duration::from_secs(2));
            let intro = shot.intro.unwrap();
            assert_eq!((intro.slot, intro.movie.as_str()), (1, "eruptor"));
            assert_eq!(intro.figure_id, "000009-0000");
            assert_eq!(intro.frames, Some(10));
            assert_eq!(intro.audio, "none");
            assert!(intro.full_screen, "{intro:?}");
            // 16:9 filling the 1920x1080 window.
            let [x, y, w, h] = intro.video_rect.unwrap();
            assert!(x.abs() <= 1.0 && y.abs() <= 1.0, "{x},{y}");
            assert!(
                (w - 1920.0).abs() <= 1.0 && (h - 1080.0).abs() <= 1.0,
                "{w}x{h}"
            );
            // The card is drawn under it all along.
            assert_eq!(shot.cards.len(), 1);

            let shot = tv.until("the end", |s| s.intro.is_none());
            assert!(
                placed.elapsed() >= Duration::from_millis(400),
                "ended early"
            );
            assert_eq!(tv.overlays(), 0);
            assert_eq!(shot.cards.len(), 1);
            assert_eq!(shot.cards[0].figure_id, "000009-0000");
            assert!(shot.intro_queue.is_empty());
        }

        #[test]
        fn a_second_placement_plays_right_after_the_first() {
            let tmp = tempfile::tempdir().unwrap();
            let lib = library(tmp.path(), &[("eruptor", 6), ("ignitor", 6)]);
            let mut tv = tv(Some(lib));
            tv.place(0, "000009-0000");
            // Legendary Ignitor: a variant plays its base's intro.
            tv.place(1, "00000a-0803");
            let shot = tv.shot();
            assert_eq!(shot.intro.as_ref().unwrap().movie, "eruptor");
            let queued: Vec<_> = shot
                .intro_queue
                .iter()
                .map(|q| (q.slot, q.movie.as_str()))
                .collect();
            assert_eq!(queued, [(2, "ignitor")]);
            let shot = tv.until("both played", |s| s.intro.is_none());
            assert_eq!(shot.cards.len(), 2);
            assert_eq!(tv.overlays(), 0);
        }

        #[test]
        fn the_next_intro_starts_in_the_frame_the_first_ends() {
            let tmp = tempfile::tempdir().unwrap();
            let lib = library(tmp.path(), &[("eruptor", 4), ("ignitor", 30)]);
            let mut tv = tv(Some(lib));
            tv.place(0, "000009-0000");
            tv.place(1, "00000a-0803");
            let mut movies = Vec::new();
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(5) {
                tv.app.update();
                let movie = tv.movie();
                if movies.last() != Some(&movie) {
                    movies.push(movie.clone());
                }
                if movie.as_deref() == Some("ignitor") {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(
                movies,
                [Some("eruptor".to_string()), Some("ignitor".to_string())],
                "the portal showed between them"
            );
            assert_eq!(tv.overlays(), 1);
        }

        #[test]
        fn removing_a_figure_drops_its_queued_intro_and_stops_its_playing_one() {
            let tmp = tempfile::tempdir().unwrap();
            let lib = library(tmp.path(), &[("eruptor", 100), ("ignitor", 100)]);
            let mut tv = tv(Some(lib));
            tv.place(0, "000009-0000");
            tv.place(1, "00000a-0803");
            tv.clear(1);
            let shot = tv.shot();
            assert_eq!(shot.intro.as_ref().unwrap().movie, "eruptor");
            assert!(shot.intro_queue.is_empty(), "ignitor never plays");
            tv.until("frames", |s| s.intro.as_ref().is_some_and(|i| i.shown > 0));
            tv.clear(0);
            let shot = tv.shot();
            assert!(shot.intro.is_none());
            tv.app.update();
            assert_eq!(tv.overlays(), 0);
            assert!(tv.shot().cards.is_empty());
        }

        fn key(tv: &mut Tv, code: KeyCode, state: ButtonState) {
            tv.app.world_mut().write_message(KeyboardInput {
                key_code: code,
                logical_key: Key::Character("x".into()),
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            tv.app.update();
        }

        #[test]
        fn a_key_skips_to_the_next_and_then_to_the_portal() {
            let tmp = tempfile::tempdir().unwrap();
            let lib = library(tmp.path(), &[("eruptor", 100), ("ignitor", 100)]);
            let mut tv = tv(Some(lib));
            tv.place(0, "000009-0000");
            tv.place(1, "00000a-0803");
            key(&mut tv, KeyCode::KeyX, ButtonState::Pressed);
            assert_eq!(tv.movie().as_deref(), Some("ignitor"));
            // Held: still one press.
            tv.app.update();
            assert_eq!(tv.movie().as_deref(), Some("ignitor"));
            key(&mut tv, KeyCode::KeyX, ButtonState::Released);
            key(&mut tv, KeyCode::Escape, ButtonState::Pressed);
            assert_eq!(tv.movie(), None);
            tv.app.update();
            assert_eq!(tv.overlays(), 0);
            assert_eq!(tv.shot().cards.len(), 2);
        }

        #[test]
        fn the_test_hook_skips_like_a_key() {
            let tmp = tempfile::tempdir().unwrap();
            let mut tv = tv(Some(library(tmp.path(), &[("eruptor", 100)])));
            // Asked while idle: nothing to skip, and it isn't kept for later.
            tv.mirror
                .1
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tv.app.update();
            tv.place(0, "000009-0000");
            assert_eq!(tv.movie().as_deref(), Some("eruptor"));
            tv.mirror
                .1
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tv.app.update();
            assert_eq!(tv.movie(), None);
        }

        /// A pad, connected.
        fn pad(tv: &mut Tv) -> Entity {
            let pad = tv.app.world_mut().spawn_empty().id();
            tv.app
                .world_mut()
                .write_message(GamepadConnectionEvent::new(
                    pad,
                    GamepadConnection::Connected {
                        name: "pad".into(),
                        vendor_id: None,
                        product_id: None,
                    },
                ));
            tv.app.update();
            assert!(tv.app.world().entity(pad).contains::<Gamepad>());
            pad
        }

        /// `button` to `value` (1 down, 0 up), one frame.
        fn button(tv: &mut Tv, pad: Entity, button: GamepadButton, value: f32) {
            tv.app.world_mut().write_message(RawGamepadEvent::Button(
                RawGamepadButtonChangedEvent::new(pad, button, value),
            ));
            tv.app.update();
        }

        #[test]
        fn a_controller_button_skips() {
            let tmp = tempfile::tempdir().unwrap();
            let lib = library(tmp.path(), &[("eruptor", 100)]);
            let mut tv = tv(Some(lib));
            let pad = pad(&mut tv);
            tv.place(0, "000009-0000");
            assert_eq!(tv.movie().as_deref(), Some("eruptor"));
            button(&mut tv, pad, GamepadButton::South, 1.0);
            assert_eq!(tv.movie(), None);
        }

        /// AP.5's layer pick, through the router: South during an intro
        /// skips it and the screen under it hears nothing; with no intro the
        /// screen hears it.
        #[test]
        fn south_during_an_intro_reaches_nothing_under_it() {
            use crate::input::TvInput;
            use crate::input::heard::{listen, take};
            let tmp = tempfile::tempdir().unwrap();
            let lib = library(tmp.path(), &[("eruptor", 100), ("ignitor", 100)]);
            let mut tv = tv(Some(lib));
            listen(&mut tv.app);
            let pad = pad(&mut tv);
            tv.place(0, "000009-0000");
            tv.place(1, "00000a-0803");
            for movie in [Some("ignitor"), None] {
                button(&mut tv, pad, GamepadButton::South, 1.0);
                assert_eq!(tv.movie().as_deref(), movie);
                button(&mut tv, pad, GamepadButton::South, 0.0);
            }
            assert_eq!(take(&mut tv.app), []);
            tv.app.update();
            assert_eq!(tv.overlays(), 0);
            button(&mut tv, pad, GamepadButton::South, 1.0);
            assert_eq!(take(&mut tv.app), [TvInput::Activate]);
            assert_eq!(tv.shot().cards.len(), 2);
        }

        /// AP.5: the overlay holds the focus while intros play back to back
        /// and gives it back when the last one stops.
        #[test]
        fn an_intro_holds_the_focus_and_gives_it_back() {
            use bevy::input_focus::InputFocus;
            let focus = |tv: &Tv| tv.app.world().resource::<InputFocus>().get();
            let overlay = |tv: &mut Tv| {
                let world = tv.app.world_mut();
                world
                    .query_filtered::<Entity, With<IntroOverlay>>()
                    .single(world)
                    .ok()
            };
            let tmp = tempfile::tempdir().unwrap();
            let lib = library(tmp.path(), &[("eruptor", 100), ("ignitor", 100)]);
            let mut tv = tv(Some(lib));
            let had = tv.app.world_mut().spawn_empty().id();
            tv.app
                .world_mut()
                .resource_mut::<InputFocus>()
                .set(had, FocusCause::Navigated);
            tv.place(0, "000009-0000");
            tv.place(1, "00000a-0803");
            let first = overlay(&mut tv).unwrap();
            assert_eq!(focus(&tv), Some(first));
            key(&mut tv, KeyCode::KeyX, ButtonState::Pressed);
            assert_eq!(tv.movie().as_deref(), Some("ignitor"));
            tv.app.update();
            let second = overlay(&mut tv).unwrap();
            assert_ne!(second, first);
            assert_eq!(focus(&tv), Some(second));
            key(&mut tv, KeyCode::KeyX, ButtonState::Released);
            key(&mut tv, KeyCode::KeyX, ButtonState::Pressed);
            tv.app.update();
            assert_eq!(overlay(&mut tv), None);
            assert_eq!(focus(&tv), Some(had));
        }

        /// AP.5-AP.6 review: a screen with a choice taking over from an
        /// intro (a crash on the portal shows ServerError) keeps the focus its
        /// button took on spawn; the overlay going gives back nothing.
        #[test]
        fn a_choice_that_took_the_focus_keeps_it_when_the_intro_stops() {
            use crate::input::FocusId;
            use crate::tv::CLOSE;
            let focused = |tv: &Tv| {
                let world = tv.app.world();
                let entity = world.resource::<InputFocus>().get()?;
                world.get::<FocusId>(entity).map(|id| id.0)
            };
            let tmp = tempfile::tempdir().unwrap();
            let lib = library(tmp.path(), &[("eruptor", 100)]);
            let mut tv = tv(Some(lib));
            tv.place(0, "000009-0000");
            assert_eq!(tv.overlays(), 1);
            tv.app.world_mut().resource_mut::<Current>().screen = TvScreen::ServerError {
                message: "the driver stopped".into(),
            };
            // Checked after every frame, so a frame that took it away and
            // a later one that put it back still fail.
            for frame in 0..4 {
                tv.app.update();
                assert_eq!(focused(&tv), Some(CLOSE), "frame {frame}");
            }
            assert_eq!(tv.overlays(), 0);
        }

        #[test]
        fn a_figure_without_an_intro_and_one_placed_before_the_extract_stay_cards() {
            let tmp = tempfile::tempdir().unwrap();
            let mut tv = tv(None);
            // Extraction still running: no library.
            tv.place(0, "000009-0000");
            assert!(tv.shot().intro.is_none());
            tv.app
                .world_mut()
                .insert_resource(library(tmp.path(), &[("eruptor", 100)]));
            tv.app.update();
            assert!(tv.shot().intro.is_none(), "an earlier placement played");
            // Sky-Iron Shield: no character, no intro.
            tv.place(1, "0000cd-0000");
            assert!(tv.shot().intro.is_none());
            assert_eq!(tv.overlays(), 0);
            // The next placement with an intro plays.
            tv.place(2, "000009-0000");
            assert_eq!(tv.movie().as_deref(), Some("eruptor"));
            assert_eq!(tv.shot().cards.len(), 3);
        }

        #[test]
        fn a_missing_video_ends_its_intro_and_the_queue_goes_on() {
            let tmp = tempfile::tempdir().unwrap();
            // Ignitor's file isn't there.
            let lib = library(tmp.path(), &[("eruptor", 3)]);
            let mut tv = tv(Some(lib));
            tv.place(1, "00000a-0803");
            tv.place(0, "000009-0000");
            tv.until("eruptor", |s| {
                s.intro.as_ref().is_some_and(|i| i.movie == "eruptor")
            });
            tv.until("idle", |s| s.intro.is_none());
            assert_eq!(tv.overlays(), 0);
        }

        #[test]
        fn leaving_the_portal_screen_stops_everything() {
            let tmp = tempfile::tempdir().unwrap();
            let lib = library(tmp.path(), &[("eruptor", 100), ("ignitor", 100)]);
            let mut tv = tv(Some(lib));
            tv.place(0, "000009-0000");
            tv.place(1, "00000a-0803");
            tv.app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
            tv.app.update();
            let shot = tv.shot();
            assert!(shot.intro.is_none() && shot.intro_queue.is_empty());
            tv.app.update();
            assert_eq!(tv.overlays(), 0);
        }

        /// AP.1 review: an intro over the portal is no screen of its own;
        /// leaving the portal under it leaves the next screen's root alone.
        #[test]
        fn an_intro_over_the_portal_leaves_one_screen_root() {
            use crate::tv::ScreenRoot;
            let roots = |tv: &mut Tv| {
                let world = tv.app.world_mut();
                let mut q = world.query_filtered::<Entity, With<ScreenRoot>>();
                q.iter(world).collect::<Vec<_>>()
            };
            let tmp = tempfile::tempdir().unwrap();
            let lib = library(tmp.path(), &[("eruptor", 100), ("ignitor", 100)]);
            let mut tv = tv(Some(lib));
            let portal = roots(&mut tv);
            assert_eq!(portal.len(), 1);
            tv.place(0, "000009-0000");
            tv.until("playing", |s| s.intro.as_ref().is_some_and(|i| i.shown > 0));
            tv.place(1, "00000a-0803");
            assert_eq!((roots(&mut tv), tv.overlays()), (portal.clone(), 1));
            assert_eq!(tv.shot().cards.len(), 2);
            for screen in [
                TvScreen::ServerError {
                    message: "open profile store".into(),
                },
                TvScreen::Join,
            ] {
                tv.app.world_mut().resource_mut::<Current>().screen = screen.clone();
                tv.app.update();
                tv.app.update();
                let now = roots(&mut tv);
                assert!(now.len() == 1 && now != portal, "{screen:?}: {now:?}");
                assert_eq!(tv.overlays(), 0, "{screen:?}");
            }
        }

        #[test]
        fn a_4_by_3_window_letterboxes_the_picture() {
            let tmp = tempfile::tempdir().unwrap();
            let mut app = portal_app(1440, 1080, WindowMode::Tv, PortalCards::default());
            let mirror = TvMirror::default();
            app.add_message::<PortalChanged>()
                .insert_resource(mirror.clone())
                .insert_resource(library(tmp.path(), &[("eruptor", 100)]))
                .add_plugins((IntroPlugin, Mirror));
            let mut tv = Tv { app, mirror };
            tv.app.update();
            tv.place(0, "000009-0000");
            let shot = tv.until("open", |s| {
                s.intro.as_ref().is_some_and(|i| i.video_rect.is_some())
            });
            let intro = shot.intro.unwrap();
            assert!(intro.full_screen);
            let [x, y, w, h] = intro.video_rect.unwrap();
            assert!((w - 1440.0).abs() <= 1.0, "{w}");
            assert!((h - 810.0).abs() <= 1.0, "{h}");
            assert!(x.abs() <= 1.0 && (y - 135.0).abs() <= 1.0, "{x},{y}");
        }
    }
}
