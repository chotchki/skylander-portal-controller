//! Dev builds only: with `GUAC_SHOT_DIR=<dir>` set, motion is reduced
//! ([`ReducedMotion`], so a screen's shot is the same on every run), the TV
//! renders offscreen (`GUAC_SHOT_SIZE`, default 1920x1080) and a PNG is saved
//! each time it settles after a redraw (a screen entered, a card or the
//! extraction band redrawn, a line patched, a choice's focus look turning on
//! or off) or an intro (an intro's shot lands
//! [`SETTLE`] into it). Offscreen because a locked or occluded Mac presents no
//! window frames, so a window capture comes back black; the window stays empty
//! meanwhile. An intro's shot is a frame of the disc's own FMV, so the folder
//! must be outside any git checkout (the extract root's rule, design decision
//! 5); one inside is refused and nothing shot. A folder that isn't there is
//! made, and a shot that doesn't land is logged as an error (openspec AE.29).
//! The level's overlays (AG.50) arm one as they come and go, named for the
//! one showing.
//! A card's turning model holds its idle's first pose at `GUAC_SHOT_YAW`
//! degrees of its turn (default 0, facing the camera), so a walk can shoot a
//! turn one run per angle. With `GUAC_SHOT_WALK=<ms>` too, a shot is taken
//! every that many milliseconds of the TV's clock that the hero has moved
//! while he walks the level (AG.21), at once (a walk doesn't settle), named
//! for where he stands and the way he faces: a walk as a sequence; and
//! (AH.24) at the same interval while anything his attacks spawned draws,
//! named `attack` (a throw's ball in flight, an eruption's lava; not the
//! helpers his join spawns). Off by default,
//! since a shot's PNG is written on the frame's thread.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};
use tracing::{error, info};

use crate::cards::PortalCards;
use crate::extraction::Extraction;
use crate::hero::HeroBody;
use crate::intro::IntroQueue;
use crate::motion::ReducedMotion;
use crate::pause::LevelOverlay;
use crate::play::Play;
use crate::turntable::FrozenYaw;
use crate::tv::{Current, Redrawn, TvCamera};

/// Long enough for a redrawn tree's layout and portraits to reach the GPU.
const SETTLE: Duration = Duration::from_millis(750);

/// He has moved since the last walk shot once he's this far from it, m.
const WALK_MOVED: f32 = 0.01;

pub struct Shots;

impl Plugin for Shots {
    fn build(&self, app: &mut App) {
        let shot_dir = std::env::var_os("GUAC_SHOT_DIR");
        app.insert_resource(ReducedMotion::for_shots(shot_dir.as_deref()));
        let Some(dir) = shot_dir.map(PathBuf::from) else {
            return;
        };
        let dir = match made_shot_dir(dir) {
            Ok(dir) => dir,
            Err(ShotDirError::InCheckout { dir, repo }) => {
                error!(
                    dir = %dir.display(),
                    repo = %repo.display(),
                    "GUAC_SHOT_DIR is inside a git work tree and intro shots are game frames; no shots"
                );
                return;
            }
            Err(ShotDirError::NotMade { dir, why }) => {
                error!(dir = %dir.display(), %why, "GUAC_SHOT_DIR couldn't be made; no shots");
                return;
            }
        };
        let size = std::env::var("GUAC_SHOT_SIZE")
            .ok()
            .and_then(|s| parse_size(&s))
            .unwrap_or(UVec2::new(1920, 1080));
        let yaw = std::env::var("GUAC_SHOT_YAW")
            .ok()
            .and_then(|s| parse_yaw(&s))
            .unwrap_or(0.0);
        let walk = std::env::var("GUAC_SHOT_WALK")
            .ok()
            .and_then(|s| parse_every(&s));
        info!(dir = %dir.display(), %size, yaw, walk_ms = walk.map(|w| w.as_millis() as u64),
            "rendering offscreen, a screenshot per TV change");
        app.insert_resource(FrozenYaw(yaw.to_radians()));
        app.insert_resource(ShotState {
            dir,
            size,
            walk,
            target: None,
            seq: 0,
            due: None,
            saved: 0,
            failed: 0,
        })
        .add_systems(PostStartup, render_offscreen)
        .add_systems(Last, snap);
    }
}

#[derive(Resource)]
struct ShotState {
    dir: PathBuf,
    size: UVec2,
    /// `GUAC_SHOT_WALK`: a shot this often while the hero walks.
    walk: Option<Duration>,
    target: Option<Handle<Image>>,
    seq: u32,
    due: Option<Instant>,
    /// Shots written so far, and those whose write failed.
    saved: u32,
    failed: u32,
}

/// `dir` made absolute, or it and the work tree holding it.
fn outside_checkouts(dir: PathBuf) -> Result<PathBuf, (PathBuf, PathBuf)> {
    let dir = std::path::absolute(&dir).unwrap_or(dir);
    match guac_extract::root::enclosing_git_tree(&dir) {
        Some(repo) => Err((dir, repo)),
        None => Ok(dir),
    }
}

/// Why `GUAC_SHOT_DIR` takes no shots.
#[derive(Debug, PartialEq)]
enum ShotDirError {
    /// Inside a git work tree: refused before anything is made.
    InCheckout { dir: PathBuf, repo: PathBuf },
    /// Outside one, but it couldn't be made.
    NotMade { dir: PathBuf, why: String },
}

/// The shot folder: `dir` made absolute, refused inside a git work tree,
/// else made with its parents.
fn made_shot_dir(dir: PathBuf) -> Result<PathBuf, ShotDirError> {
    let dir =
        outside_checkouts(dir).map_err(|(dir, repo)| ShotDirError::InCheckout { dir, repo })?;
    match std::fs::create_dir_all(&dir) {
        Ok(()) => Ok(dir),
        Err(e) => Err(ShotDirError::NotMade {
            dir,
            why: e.to_string(),
        }),
    }
}

/// Writes a captured shot to `path` as an 8-bit RGB PNG, its alpha dropped
/// as Bevy's `save_to_disk` drops it.
fn save(shot: &Image, path: &Path) -> Result<(), String> {
    let image = shot.clone().try_into_dynamic().map_err(|e| e.to_string())?;
    image
        .to_rgb8()
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| e.to_string())
}

/// The observer a shot's capture lands on: written to `path`, counted and
/// logged as saved, or counted and logged as an error when the write fails.
fn store(path: PathBuf) -> impl FnMut(On<ScreenshotCaptured>, ResMut<ShotState>) {
    move |captured, mut state| match save(&captured.image, &path) {
        Ok(()) => {
            state.saved += 1;
            info!(path = %path.display(), "screenshot saved");
        }
        Err(why) => {
            state.failed += 1;
            error!(path = %path.display(), %why, "screenshot not saved");
        }
    }
}

/// `1920x1080` → (1920, 1080).
fn parse_size(s: &str) -> Option<UVec2> {
    let (w, h) = s.split_once('x')?;
    Some(UVec2::new(w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// `GUAC_SHOT_WALK`'s milliseconds, over 0.
fn parse_every(s: &str) -> Option<Duration> {
    s.trim()
        .parse::<u64>()
        .ok()
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis)
}

/// `GUAC_SHOT_YAW`'s degrees, finite.
fn parse_yaw(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().filter(|d| d.is_finite())
}

/// Points the TV's camera at an image instead of the window.
fn render_offscreen(
    mut commands: Commands,
    mut state: ResMut<ShotState>,
    mut images: ResMut<Assets<Image>>,
    camera: Query<Entity, With<TvCamera>>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let image = Image::new_target_texture(
        state.size.x,
        state.size.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    let handle = images.add(image);
    commands
        .entity(camera)
        // The TV's camera names itself the default UI camera already
        // (`tv::spawn_camera`); without a window target bevy_ui would pick none.
        .insert((
            RenderTarget::Image(handle.clone().into()),
            IsDefaultUiCamera,
        ));
    state.target = Some(handle);
}

fn snap(
    mut commands: Commands,
    mut state: ResMut<ShotState>,
    current: Option<Res<Current>>,
    cards: Res<PortalCards>,
    extraction: Option<Res<Extraction>>,
    intros: Option<Res<IntroQueue>>,
    mut redrawn: MessageReader<Redrawn>,
    (play, heroes, time, overlays, clones): (
        Option<Res<Play>>,
        Query<&HeroBody>,
        Res<Time>,
        Query<&LevelOverlay>,
        Query<&crate::hero_scripts::CloneDrawn>,
    ),
    mut walked: Local<Option<(Duration, Vec3)>>,
) {
    let redrew = redrawn.read().count() > 0;
    let Some(current) = current else {
        return;
    };
    let intro_changed = intros.as_ref().is_some_and(|q| q.is_changed());
    if redrew || intro_changed {
        state.due = Some(Instant::now() + SETTLE);
        return;
    }
    let Some(target) = state.target.clone() else {
        return;
    };
    let hero = play
        .as_deref()
        .and_then(Play::hero)
        .and_then(|e| heroes.get(e).ok())
        .map(|b| (b.hero.pivot, b.hero.facing));
    if state.due.is_none()
        && let (Some(every), Some((pivot, facing))) = (state.walk, hero)
    {
        let now = time.elapsed();
        let (at, from) = *walked.get_or_insert((now, pivot));
        let attacking = clones.iter().any(|c| c.draws && c.attack);
        if now.saturating_sub(at) >= every && (attacking || pivot.distance(from) >= WALK_MOVED) {
            *walked = Some((now, pivot));
            state.seq += 1;
            let name = format!(
                "{:02}-{}-{:.2}_{:.2}_{:.2}-facing-{:.0}.png",
                state.seq,
                if attacking { "attack" } else { "walk" },
                pivot.x,
                pivot.y,
                pivot.z,
                facing.rem_euclid(360.0)
            );
            let path = state.dir.join(name);
            info!(path = %path.display(), "walk screenshot requested");
            commands
                .spawn(Screenshot::image(target))
                .observe(store(path));
            return;
        }
    }
    if state.due.is_none_or(|due| Instant::now() < due) {
        return;
    }
    state.due = None;
    state.seq += 1;
    let screen = format!("{:?}", current.screen);
    let screen: String = screen.chars().take_while(|c| c.is_alphanumeric()).collect();
    let extract = extraction.map_or("Idle", |e| e.status.kind());
    let intro = intros
        .as_ref()
        .and_then(|q| q.playing())
        .map(|e| format!("-intro-{}", e.movie))
        .unwrap_or_default();
    let overlay = overlays
        .iter()
        .next()
        .map(|o| format!("-overlay-{}", o.name()))
        .unwrap_or_default();
    let name = format!(
        "{:02}-{screen}-{}cards-{extract}{intro}{overlay}.png",
        state.seq,
        cards.cards().count()
    );
    let path = state.dir.join(name);
    info!(path = %path.display(), "screenshot requested");
    commands
        .spawn(Screenshot::image(target))
        .observe(store(path));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shot_sizes_parse() {
        assert_eq!(parse_size("3840x2160"), Some(UVec2::new(3840, 2160)));
        assert_eq!(parse_size("1920 x 1080"), Some(UVec2::new(1920, 1080)));
        assert_eq!(parse_size("big"), None);
    }

    #[test]
    fn walk_shot_intervals_parse() {
        assert_eq!(parse_every(" 500 "), Some(Duration::from_millis(500)));
        assert_eq!(parse_every("0"), None);
        assert_eq!(parse_every("-5"), None);
        assert_eq!(parse_every("often"), None);
    }

    #[test]
    fn shot_yaws_parse() {
        assert_eq!(parse_yaw(" 45 "), Some(45.0));
        assert_eq!(parse_yaw("-22.5"), Some(-22.5));
        assert_eq!(parse_yaw("inf"), None);
        assert_eq!(parse_yaw("left"), None);
    }

    #[test]
    fn a_shot_folder_in_a_checkout_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("shots");
        assert_eq!(outside_checkouts(outside.clone()), Ok(outside));
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let inside = repo.join("shots");
        assert_eq!(outside_checkouts(inside.clone()), Err((inside, repo)));
        // Relative to the cwd, which is this crate in the repo.
        let (dir, _) = outside_checkouts("shots".into()).unwrap_err();
        assert!(dir.is_absolute(), "{}", dir.display());
    }

    /// AE.29: a folder that isn't there is made, parents and all; one in a
    /// checkout is refused and nothing made; one that can't be made says so.
    #[test]
    fn the_shot_folder_is_made_unless_its_in_a_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let fresh = tmp.path().join("shots/walk-1");
        assert_eq!(made_shot_dir(fresh.clone()), Ok(fresh.clone()));
        assert!(fresh.is_dir());
        // Made already: fine.
        assert_eq!(made_shot_dir(fresh.clone()), Ok(fresh));
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let inside = repo.join("shots");
        assert_eq!(
            made_shot_dir(inside.clone()),
            Err(ShotDirError::InCheckout {
                dir: inside.clone(),
                repo
            })
        );
        assert!(!inside.exists(), "nothing made in a checkout");
        let file = tmp.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        let under_a_file = file.join("shots");
        assert!(matches!(
            made_shot_dir(under_a_file),
            Err(ShotDirError::NotMade { .. })
        ));
    }

    /// AE.29: a capture is written as a PNG and counted saved; one whose
    /// write fails (its folder gone) is counted failed, not saved.
    #[test]
    fn a_shot_that_doesnt_land_is_a_failure() {
        use bevy::asset::RenderAssetUsages;
        use bevy::render::render_resource::{Extent3d, TextureDimension};
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.insert_resource(ShotState {
            dir: tmp.path().to_path_buf(),
            size: UVec2::new(2, 2),
            walk: None,
            target: None,
            seq: 0,
            due: None,
            saved: 0,
            failed: 0,
        });
        let shot = Image::new_fill(
            Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[200, 60, 20, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::all(),
        );
        let capture = |app: &mut App, path: PathBuf| {
            let world = app.world_mut();
            let entity = world.spawn_empty().observe(store(path)).id();
            world.trigger(ScreenshotCaptured {
                entity,
                image: shot.clone(),
            });
            world.flush();
        };
        let landed = tmp.path().join("01-Portal.png");
        capture(&mut app, landed.clone());
        let read = image::open(&landed).unwrap().to_rgb8();
        assert_eq!(read.dimensions(), (2, 2));
        assert_eq!(read.get_pixel(1, 1).0, [200, 60, 20]);
        let lost = tmp.path().join("gone/02-Portal.png");
        capture(&mut app, lost.clone());
        assert!(!lost.exists());
        let state = app.world().resource::<ShotState>();
        assert_eq!((state.saved, state.failed), (1, 1));
    }
}
