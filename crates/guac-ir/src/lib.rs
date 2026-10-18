//! Guacamole's intermediate representation: what guac-extract reads off a disc
//! dump and the engine loads, as serde types behind one version number.
//!
//! Every IR file is an envelope `{ ir_version, body }`. Reading checks the
//! version before the body, so an extract from older code fails with
//! [`IrError::VersionMismatch`] instead of a confusing shape error, and the
//! extractor re-extracts (design decision 5: a stale manifest rebuilds).
//! Bump [`IR_VERSION`] whenever a type here changes shape.

#![forbid(unsafe_code)]

mod characters;
mod identity;

pub use characters::{Character, CharacterArchive, CharacterTable, Intro, IntroSource};
pub use identity::{DumpIdentity, Sha256};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The IR's shape version. Any change to a serialized type bumps it.
pub const IR_VERSION: u32 = 1;

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

/// The body alone, read once [`VersionProbe`] has passed.
#[derive(Deserialize)]
struct BodyOnly<T> {
    body: T,
}

/// The version alone: serde skips `body`, so a file from another IR version is
/// read just far enough to say which.
#[derive(Deserialize)]
struct VersionProbe {
    ir_version: u32,
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
    let probe: VersionProbe = ron::from_str(text)?;
    check_version(probe.ir_version)?;
    let envelope: BodyOnly<T> = ron::from_str(text)?;
    Ok(envelope.body)
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
        assert!(text.contains("ir_version: 1"), "{text}");
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
            .replace("ir_version: 1", "ir_version: 2");
        assert!(matches!(
            from_ron::<DumpIdentity>(&text),
            Err(IrError::VersionMismatch {
                found: 2,
                expected: 1
            })
        ));
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
                expected: 1
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
