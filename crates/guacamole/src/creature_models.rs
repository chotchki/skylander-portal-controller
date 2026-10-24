//! What the first fight's creatures draw with (openspec AH.25): each
//! skinned model the fight's placements name, off the level's props part,
//! as a model the character pipeline draws and poses (its skeleton, its
//! tfbmulti2 parts, its idle), keyed in the hero's scene as the walking
//! hero is (the census's creatures table holds them, `tests/giants_census.rs`).
//! The TV poses one by its idle with the clips its scripts play laid over
//! it, by the slots of the level's palettes part (`crate::scripts`'
//! sequences; stand-in ledger: `creature-pose`).

use guac_ir::DumpIdentity;
use guac_ir::clip::Clip;
use guac_ir::level::{Placements, Props};
use guac_ir::model::{self, Model, Segment};
use guac_ir::shading::Scene;

use crate::character::warm::{self, WarmError, WarmKey};
use crate::level_scene::drawn_material;
use crate::scripts::FIRST_FIGHT;

/// `prop` as a model the character pipeline draws: its skeleton, the
/// materials that draw as tfbmulti2 (the level's rule, AG.24) with the
/// segments on them, and its idle (slot 0 where that poses its skeleton,
/// else its reference pose held). None for a static prop, which has no
/// skeleton to pose.
pub fn creature_model(prop: &guac_ir::level::Prop, identity: DumpIdentity) -> Option<Model> {
    let skeleton = prop.skeleton.clone()?;
    let mut materials: Vec<model::Material> = Vec::new();
    let of: Vec<Option<u16>> = prop
        .materials
        .iter()
        .map(|m| {
            drawn_material(m).and_then(|m| {
                materials.push(m);
                u16::try_from(materials.len() - 1).ok()
            })
        })
        .collect();
    let segments: Vec<Segment> = prop
        .segments
        .iter()
        .filter_map(|s| {
            let material = (*of.get(usize::from(s.material))?)?;
            Some(Segment {
                material,
                ..s.clone()
            })
        })
        .collect();
    let idle = prop.idle.clone().unwrap_or_else(|| Clip {
        name: String::new(),
        duration: 1.0,
        frame_duration: 1.0,
        frames: 1,
        tracks: Vec::new(),
    });
    let stem = prop
        .path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&prop.path)
        .to_owned();
    Some(Model {
        identity,
        load_file: stem.clone(),
        name: stem,
        skeleton,
        materials,
        segments,
        idle,
        attachments: Vec::new(),
        attachments_unsettled: None,
    })
}

/// Each skinned model the first fight's placements ([`FIRST_FIGHT`]) name,
/// once, by its path in the placements' order, as [`creature_model`] makes
/// it: what the TV draws its creatures with. A model the props part
/// doesn't hold, or holds static, is left out.
pub fn first_fight_models(
    placements: &Placements,
    props: &Props,
    identity: &DumpIdentity,
) -> Vec<(String, Model)> {
    let mut out: Vec<(String, Model)> = Vec::new();
    for name in FIRST_FIGHT {
        let Some(path) = placements
            .actors
            .iter()
            .find(|a| a.name == name)
            .and_then(|a| a.model.clone())
        else {
            continue;
        };
        if out.iter().any(|(p, _)| *p == path) {
            continue;
        }
        let model = props
            .props
            .iter()
            .find(|p| p.path == path)
            .and_then(|p| creature_model(p, identity.clone()));
        if let Some(model) = model {
            out.push((path, model));
        }
    }
    out
}

/// Every key `models` draw in a level's `scene`, each once, the glow
/// masks after the colour keys ([`warm::level_keys`], as the hero's).
pub fn keys(models: &[(String, Model)], scene: &Scene) -> Result<Vec<WarmKey>, WarmError> {
    let mut colour: Vec<WarmKey> = Vec::new();
    let mut glow: Vec<WarmKey> = Vec::new();
    for (_, m) in models {
        for (k, _) in warm::level_keys(m, scene)? {
            let into = match k.material.pass {
                crate::character::key::Pass::LevelGlow => &mut glow,
                _ => &mut colour,
            };
            if !into.contains(&k) {
                into.push(k);
            }
        }
    }
    colour.extend(glow);
    Ok(colour)
}

#[cfg(test)]
mod tests {
    use guac_ir::level::{EffectMaterial, LevelMaterial, Placement};
    use guac_ir::model::{Joint, Skeleton};
    use guac_ir::{DumpIdentity, Sha256};

    use super::*;
    use crate::census;

    fn identity() -> DumpIdentity {
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([3; 32]),
        }
    }

    /// A made-up prop: one segment on a census key's material, one on a
    /// warp effect the level doesn't draw; skinned when `skinned`.
    fn prop(path: &str, skinned: bool) -> guac_ir::level::Prop {
        let key = census::HEROES[0].key(true).unwrap();
        let material = census::material_for(&key);
        let warp = LevelMaterial::Effect(EffectMaterial {
            name: "warp".into(),
            effect: 0xAA9,
            pixel: None,
            vertex: None,
            textures: Vec::new(),
            states: material.states,
            scrolls: Vec::new(),
            tracks: Vec::new(),
            unread: Vec::new(),
        });
        let mut body = key.mesh.triangle(1);
        body.source = 7;
        let mut water = key.mesh.triangle(0);
        water.source = 8;
        guac_ir::level::Prop {
            path: path.into(),
            skeleton: skinned.then(|| Skeleton {
                joints: vec![Joint {
                    name: "root".into(),
                    parent: None,
                    reference: guac_ir::model::Transform::IDENTITY,
                    inverse_bind: bevy::math::Mat4::IDENTITY.to_cols_array_2d(),
                }],
            }),
            materials: vec![warp, LevelMaterial::Multi2(material)],
            segments: vec![body, water],
            idle: None,
        }
    }

    fn placed(name: &str, model: Option<&str>) -> Placement {
        Placement {
            object: 0,
            id: 0,
            name: name.into(),
            script: Some("S2_Enemy_Chompy.ai".into()),
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [100.0; 3],
            model: model.map(str::to_owned),
            solid: None,
            movement: None,
            waypoints: None,
            activity_state: 4,
            activation_range: 0.0,
        }
    }

    /// A skinned prop becomes a model: its skeleton, the tfbmulti2
    /// material alone (the warp left out with its segment), the body's
    /// segment on it, its reference pose held as its idle, named by its
    /// file; a static prop makes none.
    #[test]
    fn a_skinned_prop_becomes_a_creatures_model() {
        let m = creature_model(&prop("C:/tfb/Models/ChompyUndead.mdl", true), identity()).unwrap();
        assert_eq!(m.name, "ChompyUndead.mdl");
        assert_eq!(m.skeleton.joints.len(), 1);
        assert_eq!(m.materials.len(), 1);
        assert_eq!(m.segments.len(), 1);
        assert_eq!((m.segments[0].source, m.segments[0].material), (7, 0));
        assert!(m.idle.tracks.is_empty());
        assert!(m.attachments.is_empty() && m.attachments_unsettled.is_none());
        assert!(creature_model(&prop("barrel.mdl", false), identity()).is_none());
    }

    /// The models the first fight's placements name, each once, in their
    /// order: two Chompies on one model, Norticus on another; one naming a
    /// static model and placements off the fight left out. Their keys are
    /// the hero's skinned key and its glow mask, once each.
    #[test]
    fn the_first_fights_models_each_once_and_their_keys() {
        let placements = Placements {
            identity: identity(),
            level: "level_213_tutotial".into(),
            actors: vec![
                placed("Enemy_Chompy(1)", Some("chompy.mdl")),
                placed("Enemy_Chompy(2)", Some("chompy.mdl")),
                placed("1.NPC_Norticus_First_Encounter", Some("mabu.mdl")),
                placed("Chompie_Dummies(4)", Some("barrel.mdl")),
                placed("Enemy_Chompy(9)", Some("other.mdl")),
            ],
            waypoints: Vec::new(),
        };
        let props = Props {
            identity: identity(),
            level: "level_213_tutotial".into(),
            props: vec![
                prop("chompy.mdl", true),
                prop("mabu.mdl", true),
                prop("barrel.mdl", false),
                prop("other.mdl", true),
            ],
            refused: Vec::new(),
        };
        let models = first_fight_models(&placements, &props, &identity());
        assert_eq!(
            models.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            ["chompy.mdl", "mabu.mdl"]
        );
        let scene = Scene {
            fog: true,
            point_lights: vec![crate::level_scene::DARK; crate::level_scene::MAX_POINT_LIGHTS],
            ..Scene::new()
        };
        let keys = keys(&models, &scene).unwrap();
        assert_eq!(keys.len(), 2, "{keys:?}");
        assert!(keys.iter().all(|k| k.mesh.skinned));
    }
}
