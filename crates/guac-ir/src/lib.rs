//! Guacamole's intermediate representation: what guac-extract reads off a disc
//! dump and the engine loads, as serde types behind one version number.
//!
//! Every IR file is an envelope `{ ir_version, body }`. Reading checks the
//! version before the body, so an extract from older code fails with
//! [`IrError::VersionMismatch`] instead of a confusing shape error, and the
//! extractor re-extracts (design decision 5: a stale manifest rebuilds).
//! Bump [`IR_VERSION`] whenever a type here changes shape.

#![forbid(unsafe_code)]

pub mod actor;
mod characters;
pub mod clip;
pub mod hero;
mod identity;
pub mod model;
pub mod rig;
pub mod roster;
pub mod shading;

pub use characters::{Character, CharacterArchive, CharacterTable, Intro, IntroSource, VoiceTrack};
pub use clip::Clip;
pub use hero::Hero;
pub use identity::{DumpIdentity, Sha256};
pub use model::Model;
pub use rig::{Rig, RigLight};
pub use roster::{Category, Element, FigureKind, Language, Roster, RosterFigure, Text};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The IR's shape version. Any change to a serialized type bumps it. v2
/// (AE.2) adds the disc's roster, AE.6 the models, their clips and the
/// 3D-UI rig, and AG.14 the hero side (placements' solids and movement,
/// the locomotion clips): new files beside unchanged ones, so each time the
/// extract's layout version, not this one, made older extracts redo
/// themselves.
pub const IR_VERSION: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum IrError {
    #[error("IR version {found}, this build reads {expected}")]
    VersionMismatch { found: u32, expected: u32 },
    #[error("RON: {0}")]
    Ron(#[from] ron::Error),
    #[error("RON: {0}")]
    RonSpanned(#[from] ron::error::SpannedError),
    #[error("postcard: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("{count} trailing bytes after the postcard body")]
    TrailingBytes { count: usize },
}

#[derive(Serialize)]
struct EnvelopeRef<'a, T> {
    ir_version: u32,
    body: &'a T,
}

/// How [`Envelope`] says the version is another, through serde's custom
/// error; [`from_ron`] turns it back into [`IrError::VersionMismatch`].
const MISMATCH: &str = "guac-ir: IR version ";

/// The envelope read in one pass, `ir_version` before `body` as [`to_ron`]
/// writes it, so a file from another IR version stops at its version
/// without reading its body. Skipping a body instead costs ron 0.12 about
/// 11 s on a 1.3 MB model (its ignored-any guesses each value's shape by
/// parsing it; AE.6 measured), where reading it costs a fraction of that.
struct Envelope<T>(T);

#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum EnvelopeField {
    IrVersion,
    Body,
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Envelope<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};

        struct V<T>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = Envelope<T>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an IR envelope: ir_version, then body")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let (mut version, mut body) = (false, None);
                while let Some(key) = map.next_key::<EnvelopeField>()? {
                    match key {
                        EnvelopeField::IrVersion => {
                            let found: u32 = map.next_value()?;
                            if found != IR_VERSION {
                                return Err(A::Error::custom(format!("{MISMATCH}{found}")));
                            }
                            version = true;
                        }
                        EnvelopeField::Body => body = Some(map.next_value()?),
                    }
                }
                if !version {
                    return Err(A::Error::missing_field("ir_version"));
                }
                body.map(Envelope)
                    .ok_or_else(|| A::Error::missing_field("body"))
            }
        }

        d.deserialize_struct(
            "Envelope",
            &["ir_version", "body"],
            V(std::marker::PhantomData),
        )
    }
}

fn check_version(found: u32) -> Result<(), IrError> {
    if found == IR_VERSION {
        Ok(())
    } else {
        Err(IrError::VersionMismatch {
            found,
            expected: IR_VERSION,
        })
    }
}

/// Pretty RON, the dev format: stable field order, one field per line.
pub fn to_ron<T: Serialize>(body: &T) -> Result<String, IrError> {
    let envelope = EnvelopeRef {
        ir_version: IR_VERSION,
        body,
    };
    Ok(ron::ser::to_string_pretty(
        &envelope,
        ron::ser::PrettyConfig::default(),
    )?)
}

pub fn from_ron<T: DeserializeOwned>(text: &str) -> Result<T, IrError> {
    match ron::from_str::<Envelope<T>>(text) {
        Ok(Envelope(body)) => Ok(body),
        Err(e) => match &e.code {
            ron::Error::Message(m) if m.starts_with(MISMATCH) => Err(IrError::VersionMismatch {
                found: m[MISMATCH.len()..].parse().map_err(|_| e.clone())?,
                expected: IR_VERSION,
            }),
            _ => Err(e.into()),
        },
    }
}

/// postcard, the install format. The version is the first varint, so it reads
/// back without knowing the body's shape.
pub fn to_postcard<T: Serialize>(body: &T) -> Result<Vec<u8>, IrError> {
    let envelope = EnvelopeRef {
        ir_version: IR_VERSION,
        body,
    };
    Ok(postcard::to_stdvec(&envelope)?)
}

pub fn from_postcard<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, IrError> {
    let (found, rest) = postcard::take_from_bytes::<u32>(bytes)?;
    check_version(found)?;
    let (body, rest) = postcard::take_from_bytes::<T>(rest)?;
    if !rest.is_empty() {
        return Err(IrError::TrailingBytes { count: rest.len() });
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn identity() -> DumpIdentity {
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256(std::array::from_fn(|i| (i * 7) as u8)),
        }
    }

    /// The serde shapes the IR will lean on (maps keyed by toy type, enums
    /// with and without data, options, nested vecs), so a postcard quirk shows
    /// up here before a real table hits it.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Source {
        Disc,
        NameFallback,
        Other { note: String },
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Shapes {
        identity: DumpIdentity,
        by_toy_type: BTreeMap<u16, Vec<String>>,
        source: Vec<Source>,
        intro: Option<String>,
        missing: Option<u32>,
        signed: i64,
        ratio: f32,
    }

    fn shapes() -> Shapes {
        Shapes {
            identity: identity(),
            by_toy_type: BTreeMap::from([
                (9, vec!["009_codename".into()]),
                (16, vec!["016_codename".into(), "016_codename_dark".into()]),
            ]),
            source: vec![
                Source::Disc,
                Source::NameFallback,
                Source::Other { note: "x".into() },
            ],
            intro: Some("name".into()),
            missing: None,
            signed: -3,
            ratio: 0.5,
        }
    }

    #[test]
    fn identity_round_trips_through_ron() {
        let text = to_ron(&identity()).unwrap();
        assert_eq!(from_ron::<DumpIdentity>(&text).unwrap(), identity());
    }

    #[test]
    fn identity_round_trips_through_postcard() {
        let bytes = to_postcard(&identity()).unwrap();
        assert_eq!(from_postcard::<DumpIdentity>(&bytes).unwrap(), identity());
    }

    #[test]
    fn ir_shapes_round_trip_through_both() {
        let text = to_ron(&shapes()).unwrap();
        assert_eq!(from_ron::<Shapes>(&text).unwrap(), shapes());
        let bytes = to_postcard(&shapes()).unwrap();
        assert_eq!(from_postcard::<Shapes>(&bytes).unwrap(), shapes());
    }

    fn table() -> CharacterTable {
        CharacterTable {
            identity: identity(),
            characters: BTreeMap::from([
                (
                    16,
                    Character {
                        archives: vec![
                            CharacterArchive {
                                stem: "016_spyro".into(),
                                variant: None,
                            },
                            CharacterArchive {
                                stem: "016_spyro_dark".into(),
                                variant: Some("dark".into()),
                            },
                        ],
                        vo_name: Some("spyro".into()),
                        intro: Some(Intro {
                            movie: "spyro".into(),
                            source: IntroSource::Disc,
                            voice: VoiceTrack::Only,
                        }),
                    },
                ),
                (
                    100,
                    Character {
                        archives: vec![],
                        vo_name: None,
                        intro: Some(Intro {
                            movie: "jetvac".into(),
                            source: IntroSource::NameFallback,
                            voice: VoiceTrack::Stream {
                                index: 3,
                                bed: Some(13),
                            },
                        }),
                    },
                ),
                (
                    28,
                    Character {
                        archives: vec![],
                        vo_name: None,
                        intro: Some(Intro {
                            movie: "spyro".into(),
                            source: IntroSource::Variant { base: 16 },
                            voice: VoiceTrack::Stream {
                                index: 2,
                                bed: None,
                            },
                        }),
                    },
                ),
                (
                    113,
                    Character {
                        archives: vec![CharacterArchive {
                            stem: "113_sniper".into(),
                            variant: None,
                        }],
                        vo_name: None,
                        intro: None,
                    },
                ),
            ]),
        }
    }

    #[test]
    fn the_character_table_round_trips_through_both() {
        let text = to_ron(&table()).unwrap();
        assert_eq!(from_ron::<CharacterTable>(&text).unwrap(), table());
        let bytes = to_postcard(&table()).unwrap();
        assert_eq!(from_postcard::<CharacterTable>(&bytes).unwrap(), table());
    }

    #[test]
    fn ron_carries_the_version_and_a_hex_hash() {
        let text = to_ron(&identity()).unwrap();
        assert!(text.contains("ir_version: 2"), "{text}");
        assert!(
            text.contains(&format!("\"{}\"", identity().manifest_hash)),
            "{text}"
        );
    }

    #[test]
    fn postcard_starts_with_the_version() {
        let bytes = to_postcard(&identity()).unwrap();
        assert_eq!(bytes[0], IR_VERSION as u8);
    }

    #[test]
    fn another_ir_version_is_refused_in_ron() {
        let text = to_ron(&identity())
            .unwrap()
            .replace("ir_version: 2", "ir_version: 1");
        assert!(matches!(
            from_ron::<DumpIdentity>(&text),
            Err(IrError::VersionMismatch {
                found: 1,
                expected: 2
            })
        ));
    }

    #[test]
    fn another_versions_ron_stops_at_its_version() {
        // A body this build couldn't read at all: the version is what's
        // reported, so the body was never read.
        let text = "(ir_version: 1, body: (no: [such, (shape)]))";
        assert!(matches!(
            from_ron::<DumpIdentity>(text),
            Err(IrError::VersionMismatch {
                found: 1,
                expected: 2
            })
        ));
        // No version, a stray field, or no body: refused as malformed.
        let body = to_ron(&identity()).unwrap();
        let unversioned = body.replace("ir_version: 2,", "");
        assert!(matches!(
            from_ron::<DumpIdentity>(&unversioned),
            Err(IrError::RonSpanned(_))
        ));
        let stray = body.replace("ir_version: 2,", "ir_version: 2, extra: 3,");
        assert!(from_ron::<DumpIdentity>(&stray).is_err());
        assert!(from_ron::<DumpIdentity>("(ir_version: 2)").is_err());
    }

    #[test]
    fn another_ir_version_is_refused_in_postcard() {
        // An old file whose body no longer parses as today's type still
        // reports the version, not a shape error.
        let bytes = postcard::to_stdvec(&(0u32, "an older body")).unwrap();
        assert!(matches!(
            from_postcard::<DumpIdentity>(&bytes),
            Err(IrError::VersionMismatch {
                found: 0,
                expected: 2
            })
        ));
    }

    #[test]
    fn postcard_trailing_bytes_are_refused() {
        let mut bytes = to_postcard(&identity()).unwrap();
        bytes.extend_from_slice(&[0, 0]);
        assert!(matches!(
            from_postcard::<DumpIdentity>(&bytes),
            Err(IrError::TrailingBytes { count: 2 })
        ));
    }
}
