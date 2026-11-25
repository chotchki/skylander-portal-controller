//! Who is speaking and how his mouth stands (openspec AS.820): a voice
//! line's viseme stream, off the level's lips part, weighs the speaker's
//! mouth shapes while the line plays on him.
//!
//! The speaker is the actor the line's sound copy plays on (the original's
//! dialogue starts a line on whoever says it: level 201's arena plays its
//! commander's lines on `Enemy_Elf_Heavy_S2` and its scenes' on their
//! `Brock`). Of the copies playing on him, the newest whose sound's data
//! carries a stream speaks; its stream runs on the copy's own clock, so a
//! pause holds the mouth where it stands, and a line stopped (its end, a
//! scene's skip, the actor taken off) closes it. A muted TV runs the same:
//! the stream drives the mouth, never the audio's level. A line whose
//! stream is shorter than its audio leaves the mouth at its last frame's
//! shape, closed on the disc's lines.
//!
//! The level's own lines and `global.bld`'s (level 201's arena commander
//! says global lines), and the level's own faces: a hero's face is in his
//! own archive, which this doesn't read yet.

use std::collections::HashMap;

use guac_ir::clip::Clip;
use guac_ir::lips::{Lips, Visemes};
use guac_ir::sound::{DataHome, DataRef};
use guac_script::Handle;
use guac_sound::Lookup;

use crate::scripts::ScriptWorld;

/// A level's line streams by their data info and its faces by model path.
#[derive(Debug, Clone, Default)]
pub struct Mouths {
    /// The level's archive stem (`level_201`): a sound id of another
    /// archive's isn't one of its lines.
    stem: String,
    lines: HashMap<(DataHome, u32), (Option<String>, Visemes)>,
    /// `global.bld`'s, under its stem [`GLOBAL`].
    global: HashMap<(DataHome, u32), (Option<String>, Visemes)>,
    faces: HashMap<String, Vec<Clip>>,
}

/// A line being said: its name, how far into it, and each shape's weight
/// there, in the face's order.
#[derive(Debug, Clone, PartialEq)]
pub struct Speaking {
    pub line: Option<String>,
    pub seconds: f32,
    pub weights: Vec<f32>,
}

impl Speaking {
    /// The shape weighed most, if any weighs anything: what a strip names
    /// under each frame.
    pub fn shape(&self) -> Option<usize> {
        self.weights
            .iter()
            .enumerate()
            .filter(|(_, w)| **w > 0.0)
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
    }
}

/// `global.bld`'s archive stem, as a sound id names it.
pub const GLOBAL: &str = "global";

impl Mouths {
    pub fn new(part: Lips) -> Self {
        let keyed = |lines: Vec<guac_ir::lips::Line>| {
            lines
                .into_iter()
                .map(|l| ((l.home, l.object), (l.name, l.visemes)))
                .collect()
        };
        Self {
            stem: part.level,
            lines: keyed(part.lines),
            global: keyed(part.global),
            faces: part
                .faces
                .into_iter()
                .map(|f| (f.owner, f.shapes))
                .collect(),
        }
    }

    /// The mouth shapes of the model at `path`, as placements name it.
    pub fn face(&self, path: &str) -> Option<&[Clip]> {
        self.faces.get(path).map(Vec::as_slice)
    }

    /// The level's lines and `global.bld`'s.
    pub fn lines(&self) -> usize {
        self.lines.len() + self.global.len()
    }

    pub fn faces(&self) -> usize {
        self.faces.len()
    }

    /// The line `actor` is saying now, if one with a stream plays on him.
    pub fn speaking(&self, world: &ScriptWorld, actor: Handle) -> Option<Speaking> {
        let book = world.sounds.as_deref()?;
        let sequences = world.sequences();
        world.plays().iter().rev().find_map(|p| {
            let s = sequences.get(p.sequence as usize)?;
            if s.actor != actor || s.done || s.stopped {
                return None;
            }
            let lines = if p.id.stem == self.stem {
                &self.lines
            } else if p.id.stem == GLOBAL {
                &self.global
            } else {
                return None;
            };
            let Lookup::Playable(found) = book.lookup_id(&p.id) else {
                return None;
            };
            let DataRef::Here(home, object) = found.sound.data else {
                return None;
            };
            let (name, visemes) = lines.get(&(home, object))?;
            Some(Speaking {
                line: name.clone(),
                seconds: s.time,
                weights: visemes.weights(s.time),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use guac_ir::lips::{Face, Line, Track};

    use super::*;

    fn lips() -> Lips {
        Lips {
            identity: guac_ir::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([0; 32]),
            },
            level: "level_201".into(),
            lines: vec![Line {
                home: DataHome::Pack,
                object: 22,
                name: Some("vo_x.wav".into()),
                visemes: Visemes {
                    frames: 25,
                    frame_time: 1.0 / 24.0,
                    blocks: vec![vec![Track::Constant(0.0), Track::Constant(0.75)]],
                },
            }],
            global: Vec::new(),
            faces: vec![Face {
                owner: "C:/m/Elf.mdl".into(),
                shapes: Vec::new(),
            }],
            refused: Vec::new(),
        }
    }

    #[test]
    fn a_part_is_looked_up_by_its_keys() {
        let m = Mouths::new(lips());
        assert_eq!((m.lines(), m.faces()), (1, 1));
        assert!(m.face("C:/m/Elf.mdl").is_some());
        assert!(m.face("C:/m/Other.mdl").is_none());
    }

    #[test]
    fn the_shape_named_is_the_one_weighed_most() {
        let s = |weights: Vec<f32>| Speaking {
            line: None,
            seconds: 0.0,
            weights,
        };
        assert_eq!(s(vec![0.0, 0.75, 0.25]).shape(), Some(1));
        assert_eq!(s(vec![0.0, 0.0]).shape(), None, "a closed mouth");
    }
}
