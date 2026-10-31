//! What an archive's emitters make (openspec AH.87): every
//! `tfbParticleInfo` placement's generators as the disc stores them, each
//! with the texture it draws. A new record in files of its own,
//! `particles/<archive>.<ron|postcard>` beside the spawn models, so
//! [`crate::IR_VERSION`] holds.
//!
//! Numbers are kept as stored: lengths in metres, angles and spins in
//! radians, colours as their four codes (0x7F is full strength on a sprite
//! generator, 0xFF on a ribbon), percentages as percentages. What they mean
//! in play is the engine's (guacamole's `particles`).

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;
use crate::model::{Sampler, Transform};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Emitters {
    pub identity: DumpIdentity,
    /// The archive's path under `PS3_GAME/USRDIR` (`character/009_eruptor.bld`).
    pub archive: String,
    /// Each that reads, in object order.
    pub emitters: Vec<Emitter>,
    /// Those that don't, by name, each with why.
    pub refused: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Emitter {
    /// Its placement's name, what a clone of it is found by.
    pub name: String,
    /// Its placement's object in the archive's file.
    pub object: u32,
    pub generators: Vec<Generator>,
    /// Generators of a kind not kept (lightning), by type.
    pub unread: Vec<String>,
}

/// How a generator's draws land on the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Blend {
    /// Opaque where its texture's alpha passes.
    Knockout,
    /// By its alpha.
    Blend,
    /// Added, by its alpha.
    Add,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Generator {
    pub blend: Blend,
    pub overlay_layer: u8,
    /// 0 for none.
    pub bloom: f32,
    /// Where it sits on its emitter.
    pub offset: Transform,
    /// Seconds before it starts.
    pub start_delay: f32,
    pub texture: Option<Texture>,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    Sprite(Sprite),
    Ribbon(Ribbon),
}

/// The texture a generator draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Texture {
    /// `textures/<sha256>.ktx2` in the extract.
    pub texture: String,
    /// The image's name as the disc gives it.
    pub image: String,
    pub sampler: Sampler,
}

/// A value at the start, middle and end of a life, each with how far a
/// particle's own draw may take it past that.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Keyed<T> {
    pub start: T,
    pub start_random: T,
    pub mid: T,
    pub mid_random: T,
    pub end: T,
    pub end_random: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Volume {
    Box,
    Sphere,
    Cylinder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    /// Along the generator's own turn.
    Orientation,
    Outward,
    Inward,
}

/// A sprite generator: quads born at a rate, each living its lifespan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sprite {
    /// Particles a second.
    pub spawn_rate: f32,
    pub max_alive: u16,
    pub lifespan: f32,
    pub lifespan_random: f32,
    pub max_distance: f32,
    /// Where in a life the middle key sits, percent; negative for none.
    pub midpoint_percent: f32,
    pub next_gen_scale: f32,
    pub volume: Option<Volume>,
    pub volume_size: [f32; 3],
    pub uniform: bool,
    /// Whether its particles move with the emitter.
    pub inherits_translation: bool,
    pub direction: Option<Direction>,
    pub speed: f32,
    pub speed_random: f32,
    pub spread: f32,
    /// Start, middle, end.
    pub drag: [f32; 3],
    /// Start, middle, end; down positive.
    pub gravity: [f32; 3],
    pub colour: Keyed<[u8; 4]>,
    pub size: Keyed<[f32; 3]>,
    pub rotation: f32,
    pub rotation_random: f32,
    pub spin: Keyed<f32>,
    pub random_spin_direction: bool,
    pub view_alignment: u8,
}

/// A ribbon: one strip along where its emitter has been.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ribbon {
    /// 0 none, 1 the end, 2 the start, 3 both.
    pub taper: u8,
    pub segments: u16,
    pub scroll_uv_percent: f32,
    pub scale_uv_percent: f32,
    pub width: f32,
    pub head_colour: [u8; 4],
    pub tail_colour: [u8; 4],
    pub granularity: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Filter, Wrap};
    use crate::{Sha256, from_postcard, from_ron, to_postcard, to_ron};

    fn keyed<T: Copy>(a: T, b: T, c: T) -> Keyed<T> {
        Keyed {
            start: a,
            start_random: b,
            mid: c,
            mid_random: a,
            end: b,
            end_random: c,
        }
    }

    #[test]
    fn emitters_round_trip_through_both() {
        let offset = Transform {
            translation: [0.0, 0.5, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        };
        let texture = Texture {
            texture: "textures/ab.ktx2".into(),
            image: "particles/textures/splash.png".into(),
            sampler: Sampler {
                wrap_u: Wrap::Clamp,
                wrap_v: Wrap::Repeat,
                mag: Filter::Linear,
                min: Filter::Linear,
                mip: Some(Filter::Linear),
            },
        };
        let e = Emitters {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: Sha256([4; 32]),
            },
            archive: "character/009_eruptor.bld".into(),
            emitters: vec![Emitter {
                name: "Trail".into(),
                object: 679,
                generators: vec![
                    Generator {
                        blend: Blend::Add,
                        overlay_layer: 5,
                        bloom: 5.0,
                        offset,
                        start_delay: 0.0,
                        texture: Some(texture.clone()),
                        kind: Kind::Sprite(Sprite {
                            spawn_rate: 50.0,
                            max_alive: 24,
                            lifespan: 0.3,
                            lifespan_random: 0.1,
                            max_distance: 30.0,
                            midpoint_percent: 35.0,
                            next_gen_scale: 1.0,
                            volume: Some(Volume::Cylinder),
                            volume_size: [0.1, 0.2, 0.3],
                            uniform: true,
                            inherits_translation: false,
                            direction: Some(Direction::Inward),
                            speed: 1.5,
                            speed_random: 0.25,
                            spread: 0.1,
                            drag: [0.5, 0.6, 0.7],
                            gravity: [0.0, -12.0, -6.0],
                            colour: keyed([127, 64, 0, 255], [0; 4], [127, 57, 0, 0]),
                            size: keyed([0.45; 3], [0.15; 3], [0.3; 3]),
                            rotation: 1.5,
                            rotation_random: 6.0,
                            spin: keyed(5.0, 0.0, 13.9),
                            random_spin_direction: true,
                            view_alignment: 1,
                        }),
                    },
                    Generator {
                        blend: Blend::Blend,
                        overlay_layer: 5,
                        bloom: 0.0,
                        offset,
                        start_delay: 0.25,
                        texture: None,
                        kind: Kind::Ribbon(Ribbon {
                            taper: 1,
                            segments: 7,
                            scroll_uv_percent: 0.0,
                            scale_uv_percent: 100.0,
                            width: 1.0,
                            head_colour: [255, 102, 0, 255],
                            tail_colour: [255, 157, 0, 45],
                            granularity: 100.0,
                        }),
                    },
                ],
                unread: vec!["LightningGenerator".into()],
            }],
            refused: vec![("Bent".into(), "why".into())],
        };
        assert_eq!(from_ron::<Emitters>(&to_ron(&e).unwrap()).unwrap(), e);
        assert_eq!(
            from_postcard::<Emitters>(&to_postcard(&e).unwrap()).unwrap(),
            e
        );
    }
}
