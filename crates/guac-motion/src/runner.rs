//! The headless runner (openspec AG.17, design decisions 5 and 7): the hero
//! and the camera stepped from an AF.7 input script at the original's nominal
//! 1/60 s into a trace keyed on game time, the [`Motion`] AG.8's extractors
//! read off an O5 recording, so AG.31 compares the two by the same metrics.
//!
//! The script's flip offsets become game frames one to one: offset k is the
//! frame from game time k/60 s to (k + 1)/60 s, port 0 walks the hero and a
//! port before its first step rests (the oracle hands it the host's input,
//! which an unattended run leaves at rest). Each frame the hero steps first,
//! against the view the camera showed after the frame before (The stick held
//! through a switch), then the camera steps on his pivot as the frame began
//! ([`crate::camera`]).
//!
//! Sample k holds the state as frame k begins, as a record does at its pad
//! read: his pivot and facing, the view's eye, heading and pitch, and the
//! stick that drives frame k.

use guac_oracle::MetricError;
use guac_oracle::input::{Button, InputScript, PadState};
use guac_oracle::motion::{Eye, Motion, Sample};

use crate::camera::{GameCamera, TransitionPlane};
use crate::hero::{Hero, Input};
use crate::seam::{BodyId, PhysicsSeam};
use crate::stick::Stick;
use crate::tuning::NOMINAL_STEP;

/// The hero's port: an AF.7 script's port 0.
pub const PORT: usize = 0;

/// The world a run steps through.
pub struct Runner<'a> {
    pub seam: &'a dyn PhysicsSeam,
    /// Bodies the hero's queries skip.
    pub exclude: &'a [BodyId],
    pub planes: &'a [TransitionPlane],
}

/// What frame `k` of `script` reads of the hero's pad.
pub fn input(script: &InputScript, k: u64, view_heading: f32) -> Input {
    let pad = script.state_at(PORT, k).unwrap_or_default();
    Input {
        stick: Stick::from_bytes(pad.left.x, pad.left.y),
        view_heading,
        buttons: buttons(&pad),
    }
}

/// The four face buttons by position, as [`Input`] holds them: bottom,
/// right, left, top.
fn buttons(pad: &PadState) -> [bool; 4] {
    [Button::South, Button::East, Button::West, Button::North].map(|b| pad.buttons.contains(&b))
}

impl Runner<'_> {
    /// `frames` game frames of `script` from `hero` and `camera` as they
    /// stand, at 1/60 s each: `frames + 1` samples, the last the state after
    /// the last frame.
    pub fn run(
        &self,
        hero: &mut Hero,
        camera: &mut GameCamera,
        script: &InputScript,
        frames: u64,
    ) -> Result<Motion, MetricError> {
        let mut samples = Vec::with_capacity(frames as usize + 1);
        for k in 0..=frames {
            let input = input(script, k, camera.view().heading());
            samples.push(sample(k, hero, camera, &input));
            if k == frames {
                break;
            }
            let feet = hero.pivot;
            hero.step(self.seam, &input, NOMINAL_STEP, self.exclude);
            camera.step(feet, NOMINAL_STEP, self.planes);
        }
        Motion::new(samples)
    }
}

/// Frame `k`'s sample as it begins.
fn sample(k: u64, hero: &Hero, camera: &GameCamera, input: &Input) -> Sample {
    let view = camera.view();
    Sample {
        time: k as f64 / 60.0,
        position: hero.pivot.to_array().map(f64::from),
        heading: f64::from(hero.facing),
        stick: [f64::from(input.stick.x), f64::from(input.stick.y)],
        camera: Some(Eye {
            position: view.eye.to_array().map(f64::from),
            heading: f64::from(view.heading()),
            pitch: f64::from(view.pitch()),
        }),
    }
}
