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
//! 5); one inside is refused and nothing shot.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use tracing::{error, info};

use crate::cards::PortalCards;
use crate::extraction::Extraction;
use crate::intro::IntroQueue;
use crate::motion::ReducedMotion;
use crate::tv::{Current, Redrawn, TvCamera};

/// Long enough for a redrawn tree's layout and portraits to reach the GPU.
const SETTLE: Duration = Duration::from_millis(750);

pub struct Shots;

impl Plugin for Shots {
    fn build(&self, app: &mut App) {
        let shot_dir = std::env::var_os("GUAC_SHOT_DIR");
        app.insert_resource(ReducedMotion::for_shots(shot_dir.as_deref()));
        let Some(dir) = shot_dir.map(PathBuf::from) else {
            return;
        };
        let dir = match outside_checkouts(dir) {
            Ok(dir) => dir,
            Err((dir, repo)) => {
                error!(
                    dir = %dir.display(),
                    repo = %repo.display(),
                    "GUAC_SHOT_DIR is inside a git work tree and intro shots are game frames; no shots"
                );
                return;
            }
        };
        let size = std::env::var("GUAC_SHOT_SIZE")
            .ok()
            .and_then(|s| parse_size(&s))
            .unwrap_or(UVec2::new(1920, 1080));
        info!(dir = %dir.display(), %size, "rendering offscreen, a screenshot per TV change");
        app.insert_resource(ShotState {
            dir,
            size,
            target: None,
            seq: 0,
            due: None,
        })
        .add_systems(PostStartup, render_offscreen)
        .add_systems(Last, snap);
    }
}

#[derive(Resource)]
struct ShotState {
    dir: PathBuf,
    size: UVec2,
    target: Option<Handle<Image>>,
    seq: u32,
    due: Option<Instant>,
}

/// `dir` made absolute, or it and the work tree holding it.
fn outside_checkouts(dir: PathBuf) -> Result<PathBuf, (PathBuf, PathBuf)> {
    let dir = std::path::absolute(&dir).unwrap_or(dir);
    match guac_extract::root::enclosing_git_tree(&dir) {
        Some(repo) => Err((dir, repo)),
        None => Ok(dir),
    }
}

/// `1920x1080` → (1920, 1080).
fn parse_size(s: &str) -> Option<UVec2> {
    let (w, h) = s.split_once('x')?;
    Some(UVec2::new(w.trim().parse().ok()?, h.trim().parse().ok()?))
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
    let name = format!(
        "{:02}-{screen}-{}cards-{extract}{intro}.png",
        state.seq,
        cards.cards().count()
    );
    let path = state.dir.join(name);
    info!(path = %path.display(), "screenshot requested");
    let logged = path.clone();
    commands
        .spawn(Screenshot::image(target))
        .observe(save_to_disk(path))
        .observe(move |_: On<ScreenshotCaptured>| {
            info!(path = %logged.display(), "screenshot saved");
        });
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
}
