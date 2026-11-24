//! A level's mouths (openspec AS.820): each voice line's viseme stream the
//! level's archive holds, and the viseme set of each model its placements
//! name that has one, in a level part of its own,
//! `levels/<archive>/lips.<ron|postcard>`, a file an older build never
//! opens, so [`crate::IR_VERSION`] holds and an extract made before it
//! resumes for it alone.
//!
//! A line's stream is nine weights over its time, one per mouth shape; a
//! face is its model's nine shape clips in the same order (`_A`, `_E`,
//! `_FV`, `_L`, `_MBP`, `_O`, `_S`, `_TH`, `_UWQ` on every set the disc
//! holds). guac-formats' `viseme` module has the stream's layout.

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;
use crate::clip::{Clip, curve};
use crate::sound::DataHome;

/// Frames a stream's block holds at most.
pub const BLOCK_FRAMES: u32 = 255;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lips {
    pub identity: DumpIdentity,
    pub level: String,
    /// Each data info with a stream that reads, `level.bld`'s then the
    /// English pack's, each home in object order (the sounds part's order).
    pub lines: Vec<Line>,
    /// `global.bld`'s lines, its `level.bld`'s then its English pack's: the
    /// same in every level's part, kept with each since a level's scripts
    /// play them (level 201's arena commander's lines are global sounds).
    pub global: Vec<Line>,
    /// Each model with a viseme set that reads, by path as placements name
    /// it, in object order.
    pub faces: Vec<Face>,
    /// What didn't read, by name or object, with why.
    pub refused: Vec<(String, String)>,
}

/// One voice line's stream, keyed as the sounds part keys its data info.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub home: DataHome,
    pub object: u32,
    pub name: Option<String>,
    pub visemes: Visemes,
}

/// A stream: its blocks of [`BLOCK_FRAMES`], a track per shape in each.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Visemes {
    pub frames: u32,
    /// Seconds a frame: 1/24 on the disc.
    pub frame_time: f32,
    pub blocks: Vec<Vec<Track>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Track {
    Constant(f32),
    /// A clamped B-spline over frames into its block, its points
    /// dequantized.
    Spline {
        degree: u8,
        knots: Vec<u8>,
        points: Vec<f32>,
    },
}

/// A model's mouth shapes, in the stream's track order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// The model's path as its placements name it (a palette's owner).
    pub owner: String,
    pub shapes: Vec<Clip>,
}

impl Track {
    /// Its value `frame` frames into its block, clamped to its knots.
    pub fn value(&self, frame: f64) -> f32 {
        match self {
            Track::Constant(v) => *v,
            Track::Spline {
                degree,
                knots,
                points,
            } => {
                if points.is_empty() || knots.len() < points.len() + usize::from(*degree) + 1 {
                    return 0.0;
                }
                let p: Vec<[f32; 1]> = points.iter().map(|&v| [v]).collect();
                curve(*degree, knots, &p, frame)[0] as f32
            }
        }
    }
}

impl Visemes {
    /// Seconds from the first frame to the last.
    pub fn length(&self) -> f32 {
        self.frames.saturating_sub(1) as f32 * self.frame_time
    }

    /// Each shape's weight `seconds` into the line, its last frame's past
    /// the end (none before it plays: the caller's to tell).
    pub fn weights(&self, seconds: f32) -> Vec<f32> {
        if self.blocks.is_empty() || self.frame_time <= 0.0 {
            return Vec::new();
        }
        let last = self.frames.saturating_sub(1) as f64;
        let frame = (f64::from(seconds.max(0.0)) / f64::from(self.frame_time)).min(last);
        let b = ((frame / f64::from(BLOCK_FRAMES)) as usize).min(self.blocks.len() - 1);
        let local = frame - (b as f64) * f64::from(BLOCK_FRAMES);
        self.blocks[b].iter().map(|t| t.value(local)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{from_postcard, from_ron, to_postcard, to_ron};

    fn line() -> Visemes {
        Visemes {
            frames: 300,
            frame_time: 1.0 / 24.0,
            blocks: vec![
                vec![
                    Track::Spline {
                        degree: 1,
                        knots: vec![0, 0, 24, 255, 255],
                        points: vec![0.0, 1.0, 0.0],
                    },
                    Track::Constant(0.25),
                ],
                vec![
                    Track::Constant(0.5),
                    Track::Spline {
                        degree: 1,
                        knots: vec![0, 0, 44, 44],
                        points: vec![0.0, 0.8],
                    },
                ],
            ],
        }
    }

    #[test]
    fn a_stream_weighs_each_shape_by_its_block_and_frame() {
        let v = line();
        let w = v.weights(0.0);
        assert_eq!(w, [0.0, 0.25]);
        let w = v.weights(0.5);
        assert!((w[0] - 0.5).abs() < 1e-6, "{w:?}: halfway up to frame 24");
        let w = v.weights(1.0);
        assert!((w[0] - 1.0).abs() < 1e-6, "{w:?}: frame 24");
        // Frame 255 on is the second block's, its frames from 0.
        let w = v.weights(255.0 / 24.0 + 22.0 / 24.0);
        assert_eq!(w[0], 0.5);
        assert!((w[1] - 0.4).abs() < 1e-5, "{w:?}");
        // Past the end the last frame holds.
        let (past, last) = (v.weights(100.0), v.weights(299.0 / 24.0));
        assert!(
            past.iter().zip(&last).all(|(a, b)| (a - b).abs() < 1e-5),
            "{past:?} {last:?}"
        );
        assert!((past[1] - 0.8).abs() < 1e-5, "{past:?}");
        assert!((v.length() - 299.0 / 24.0).abs() < 1e-5);
    }

    #[test]
    fn a_quadratic_track_follows_its_curve() {
        let t = Track::Spline {
            degree: 2,
            knots: vec![0, 0, 0, 10, 10, 10],
            points: vec![0.0, 1.0, 0.0],
        };
        assert_eq!(t.value(0.0), 0.0);
        assert!((t.value(5.0) - 0.5).abs() < 1e-6);
        assert_eq!(t.value(10.0), 0.0);
        let short = Track::Spline {
            degree: 2,
            knots: vec![0, 0],
            points: vec![1.0, 1.0, 1.0],
        };
        assert_eq!(short.value(1.0), 0.0, "too few knots reads nothing");
    }

    #[test]
    fn lips_round_trip() {
        let l = Lips {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: crate::Sha256([7; 32]),
            },
            level: "level_201".into(),
            lines: vec![Line {
                home: DataHome::Pack,
                object: 22,
                name: Some("vo_x_001.wav".into()),
                visemes: line(),
            }],
            global: Vec::new(),
            faces: vec![Face {
                owner: "C:/m/Elf.mdl".into(),
                shapes: Vec::new(),
            }],
            refused: vec![("#3".into(), "bad".into())],
        };
        assert_eq!(from_ron::<Lips>(&to_ron(&l).unwrap()).unwrap(), l);
        assert_eq!(from_postcard::<Lips>(&to_postcard(&l).unwrap()).unwrap(), l);
    }
}
