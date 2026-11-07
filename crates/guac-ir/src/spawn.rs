//! The models what the fight's scripts spawn comes from outside a level
//! (openspec AH.22, design decision 7): every model a placement names in
//! `permanent/global.bld` (the level-up's rings, the death stars, the
//! portal's entry effects) and in each fighting character's archive (the
//! lava ball and its splash), read as a level's props are ([`Prop`]). A
//! level's own spawn templates draw from its props part. One file per
//! archive, `spawns/<archive>.<ron|postcard>`, beside unchanged ones, so
//! [`crate::IR_VERSION`] holds. The clips and attach lists of a character
//! archive's skinned models are a part of their own ([`SpawnRigs`], openspec
//! AU.37), new files again.

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;
use crate::level::{Prop, RefusedProp};
use crate::nodes::AttachNodes;
use crate::palette::Palette;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpawnModels {
    pub identity: DumpIdentity,
    /// The archive's path under `PS3_GAME/USRDIR` (`permanent/global.bld`).
    pub archive: String,
    /// Each model that reads, in object order; a character's own body is
    /// its model file's, not here.
    pub props: Vec<Prop>,
    /// Those that don't, each with why.
    pub refused: Vec<RefusedProp>,
}

/// What a character archive's skinned models play and hang things on past
/// his own body (openspec AU.37: Pop Fizz's beast, Stealth Elf's knives):
/// one file per character archive, `spawn_rigs/<archive>.<ron|postcard>`,
/// none in it for an archive with none. A model's own [`Prop`] is the spawn
/// models part's; [`Palette::owner`] is its path there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpawnRigs {
    pub identity: DumpIdentity,
    /// The archive's path under `PS3_GAME/USRDIR`.
    pub archive: String,
    pub rigs: Vec<SpawnRig>,
    /// Those whose attach list or skeleton doesn't read, each with why.
    pub refused: Vec<RefusedProp>,
}

/// One skinned model's clips, by the slots its scripts play them from, and
/// its attach list, where it holds one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpawnRig {
    pub palette: Palette,
    pub nodes: Option<AttachNodes>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Sha256, from_postcard, from_ron, to_postcard, to_ron};

    #[test]
    fn spawn_rigs_round_trip_through_both() {
        let identity = DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([3; 32]),
        };
        let s = SpawnRigs {
            identity: identity.clone(),
            archive: "character/108_alchemist.bld".into(),
            rigs: vec![SpawnRig {
                palette: Palette {
                    identity: identity.clone(),
                    owner: "C:/tfb/Content/Models/characters/Beast.mdl".into(),
                    slots: 2,
                    entries: Vec::new(),
                    refused: Vec::new(),
                },
                nodes: Some(AttachNodes {
                    identity,
                    load_file: "C:/tfb/Content/Models/characters/Beast.mdl".into(),
                    nodes: Vec::new(),
                }),
            }],
            refused: vec![RefusedProp {
                path: "x.mdl".into(),
                reason: "why".into(),
            }],
        };
        assert_eq!(from_ron::<SpawnRigs>(&to_ron(&s).unwrap()).unwrap(), s);
        assert_eq!(
            from_postcard::<SpawnRigs>(&to_postcard(&s).unwrap()).unwrap(),
            s
        );
    }

    #[test]
    fn spawn_models_round_trip_through_both() {
        let s = SpawnModels {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: Sha256([3; 32]),
            },
            archive: "character/009_eruptor.bld".into(),
            props: vec![Prop {
                path: "C:/tfb/Content/Models/Objects/Fireblob.mdl".into(),
                skeleton: None,
                materials: Vec::new(),
                segments: Vec::new(),
                idle: None,
            }],
            refused: vec![RefusedProp {
                path: "x.mdl".into(),
                reason: "why".into(),
            }],
        };
        assert_eq!(from_ron::<SpawnModels>(&to_ron(&s).unwrap()).unwrap(), s);
        assert_eq!(
            from_postcard::<SpawnModels>(&to_postcard(&s).unwrap()).unwrap(),
            s
        );
    }
}
