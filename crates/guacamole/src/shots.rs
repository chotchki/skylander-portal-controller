//! Dev builds only: with `GUAC_SHOT_DIR=<dir>` set, the TV renders offscreen
//! (`GUAC_SHOT_SIZE`, default 1920x1080) and a PNG is saved each time it
//! settles on a new screen or card set. Offscreen because a locked or occluded
//! Mac presents no window frames, so a window capture comes back black; the
//! window stays empty meanwhile.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use tracing::info;

use crate::cards::PortalCards;
use crate::extraction::{ExtractBand, Extraction};
use crate::tv::Current;

/// Long enough for a rebuilt tree's layout and portraits to reach the GPU.
const SETTLE: Duration = Duration::from_millis(750);

pub struct Shots;

impl Plugin for Shots {
    fn build(&self, app: &mut App) {
        let Some(dir) = std::env::var_os("GUAC_SHOT_DIR").map(PathBuf::from) else {
            return;
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
    camera: Query<Entity, With<Camera2d>>,
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
        // Without a window target bevy_ui picks no default camera; name it.
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
    band: Option<Res<ExtractBand>>,
    extraction: Option<Res<Extraction>>,
) {
    let Some(current) = current else {
        return;
    };
    let band_changed = band.is_some_and(|b| b.is_changed());
    if current.is_changed() || cards.is_changed() || band_changed {
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
    let name = format!(
        "{:02}-{screen}-{}cards-{extract}.png",
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
}
