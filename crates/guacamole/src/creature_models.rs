//! What the first fight's creatures draw with (openspec AH.25), and the
//! clones of the level's skinned templates (AG.150, AS.63's rule: chapter
//! 1's push blocks' tortoises and cannon top, level 214's Chompy pods and
//! enemy gate lock): each skinned model they name, off the level's props
//! part,
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
use crate::props::clones_draw;
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

/// Each skinned model the first fight's placements ([`FIRST_FIGHT`]) on
/// chapter 1, or a later level's first encounter's placements and clones'
/// templates (openspec AS.54, [`crate::scripts::encounter`]), a hub's
/// people (openspec AU.28, [`crate::scripts::HUB_PEOPLE`]), a level's own
/// (AS.98, [`crate::scripts::LEVEL_PEOPLE`]) and the level's
/// templates whose clones draw ([`clones_draw`], openspec AS.63) name,
/// once, by its path, the fight's first, each in its list's order (the
/// people and the templates in the file's), as [`creature_model`] makes
/// it: what the TV draws its creatures, the hub's people and those
/// templates' clones with. A model the props part doesn't hold, or holds
/// static, is left out.
pub fn skinned_models(
    placements: &Placements,
    props: &Props,
    identity: &DumpIdentity,
) -> Vec<(String, Model)> {
    let mut out: Vec<(String, Model)> = Vec::new();
    // A level's first encounter past chapter 1's (openspec AS.54): its
    // placements and the templates it clones, by name in its own file.
    let encounter = crate::scripts::encounters(&placements.level)
        .flat_map(|e| e.placements.iter().chain(e.clones.iter()));
    let first_fight = guac_extract::levels::FIRST_RUN
        .contains(&placements.level.as_str())
        .then_some(FIRST_FIGHT.iter())
        .into_iter()
        .flatten();
    // The level's templates (AS.63): a clone draws its template's model.
    let templates = placements.actors.iter().filter(|a| {
        a.activity_state == guac_formats::level::ActivityState::Template.raw()
            && a.model.as_deref().is_some_and(|m| {
                props
                    .props
                    .iter()
                    .any(|p| p.path == m && p.skeleton.is_some() && clones_draw(&a.name, p))
            })
    });
    let named = first_fight.chain(encounter).filter_map(|name| {
        placements
            .actors
            .iter()
            .find(|a| a.name == *name)
            .and_then(|a| a.model.clone())
    });
    // A hub's people by their placements' scripts (openspec AU.28), a
    // level's own by their names (AS.98).
    let people = placements
        .actors
        .iter()
        .filter(|a| {
            crate::scripts::is_level_person(&placements.level, &a.name)
                || (crate::scripts::is_hub(&placements.level)
                    && a.script
                        .as_deref()
                        .is_some_and(crate::scripts::is_hub_person))
        })
        .filter_map(|a| a.model.clone());
    for path in named
        .chain(people)
        .chain(templates.filter_map(|a| a.model.clone()))
    {
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

/// Each skinned model of a character's spawn part, by its path, as
/// [`creature_model`] makes it (openspec AU.37: Pop Fizz's beast): what a
/// clone of his file's templates draws with. Static ones are
/// [`crate::props::spawn_models`]'.
pub fn spawn_models(
    spawns: &guac_ir::spawn::SpawnModels,
    identity: &DumpIdentity,
) -> Vec<(String, Model)> {
    spawns
        .props
        .iter()
        .filter_map(|p| Some((p.path.clone(), creature_model(p, identity.clone())?)))
        .collect()
}

/// Every key `models` draw in a level's `scene`, each once, the glow
/// masks after the colour keys ([`warm::level_keys`], as the hero's), and
/// each opaque colour key blending as a fading creature draws it (openspec
/// AH.139: a beaten Chompy's death takes its tint's alpha to nothing,
/// [`crate::character::key::StateKey::faded`]) after those.
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
    let mut faded: Vec<WarmKey> = Vec::new();
    for k in &colour {
        let mut f = *k;
        f.material.states = k.material.states.faded();
        if f != *k && !colour.contains(&f) && !faded.contains(&f) {
            faded.push(f);
        }
    }
    colour.extend(faded);
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

    /// A template placement (`_activityState` 5) naming `model`.
    fn template(name: &str, model: &str) -> Placement {
        Placement {
            activity_state: 5,
            ..placed(name, Some(model))
        }
    }

    /// AU.37: a character's spawn part gives a model for each skinned prop
    /// by its path, a static one being the props' to draw.
    #[test]
    fn a_spawn_parts_skinned_models_are_the_creatures_draw() {
        let spawns = guac_ir::spawn::SpawnModels {
            identity: identity(),
            archive: "character/108_alchemist.bld".into(),
            props: vec![prop("rock.mdl", false), prop("beast.mdl", true)],
            refused: Vec::new(),
        };
        let models = spawn_models(&spawns, &identity());
        let paths: Vec<&str> = models.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, ["beast.mdl"]);
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

    /// openspec AS.54: past chapter 1, a level's first encounter's models
    /// (its spawners', its clones' template's) in its list's order, chapter
    /// 1's fight's names left out of it (`Enemy_Chompy(1)`, off its list);
    /// the skinned templates' after them as on chapter 1.
    #[test]
    fn a_levels_encounter_draws_its_own_models() {
        let level = "level_214_tutotial_2";
        let placements = Placements {
            identity: identity(),
            level: level.into(),
            actors: vec![
                placed("Enemy_Chompy(1)", Some("chompy_placed.mdl")),
                placed("_Template_Undead_Chompy", Some("chompy_undead.mdl")),
                placed("Enemy_Chompy_Spawner(5)", Some("legs.mdl")),
                placed("Enemy_Chompy_Spawner(1)", Some("legs.mdl")),
                placed("Setup_001_Controller", None),
                template("2_Push_Block_Art_Sheep", "tortoise.mdl"),
            ],
            waypoints: Vec::new(),
        };
        let props = Props {
            identity: identity(),
            level: level.into(),
            props: vec![
                prop("chompy_placed.mdl", true),
                prop("chompy_undead.mdl", true),
                prop("legs.mdl", true),
                prop("tortoise.mdl", true),
            ],
            refused: Vec::new(),
        };
        let models = skinned_models(&placements, &props, &identity());
        assert_eq!(
            models.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            ["legs.mdl", "chompy_undead.mdl", "tortoise.mdl"]
        );
    }

    /// openspec AU.28: on a hub stage its people's models (Flynn's and
    /// Cali's by their placements' scripts) before the skinned templates',
    /// each once; on any other level a placement running one of their
    /// scripts' names draws nothing of its own.
    #[test]
    fn a_hubs_people_draw_their_own_models() {
        let hub = "C:/tfb/Content/Levels/Level_Hub_2/Scripts";
        let person = |name: &str, script: &str, model: &str| Placement {
            script: Some(format!("{hub}/{script}")),
            ..placed(name, Some(model))
        };
        let actors = vec![
            person("Flynn", "Flynn.ai", "balloonist.mdl"),
            person("CS_Flynn", "CS_Flynn.ai", "balloonist.mdl"),
            person("Cali", "Cali.ai", "adventure_girl.mdl"),
            person("Mabu", "Hub2_Mabu.ai", "mabu.mdl"),
            template("2_Push_Block_Art_Sheep", "tortoise.mdl"),
        ];
        let props = |level: &str| Props {
            identity: identity(),
            level: level.into(),
            props: vec![
                prop("balloonist.mdl", true),
                prop("adventure_girl.mdl", true),
                prop("mabu.mdl", true),
                prop("tortoise.mdl", true),
            ],
            refused: Vec::new(),
        };
        let models = |level: &str| {
            let placements = Placements {
                identity: identity(),
                level: level.into(),
                actors: actors.clone(),
                waypoints: Vec::new(),
            };
            skinned_models(&placements, &props(level), &identity())
                .into_iter()
                .map(|(p, _)| p)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            models("level_hub_2_stage_1"),
            ["balloonist.mdl", "adventure_girl.mdl", "tortoise.mdl"]
        );
        assert_eq!(models("level_201"), ["tortoise.mdl"]);
    }

    /// openspec AS.98: on level 214 its own people's models (Auric's in his
    /// scene and at his stall, one model) before the skinned templates';
    /// on any other level his stall's script draws nothing of its own.
    #[test]
    fn a_levels_people_draw_their_own_models() {
        let person = |name: &str, script: &str| Placement {
            script: Some(format!("C:/tfb/Content/Levels/Level_214/Scripts/{script}")),
            ..placed(name, Some("banker.mdl"))
        };
        let actors = vec![
            person("Moneybags_end", "214_Meet_Auric_IGC.ai"),
            person("Auric_store", "Auric.ai"),
            template("2_Push_Block_Art_Sheep", "tortoise.mdl"),
        ];
        let models = |level: &str| {
            let placements = Placements {
                identity: identity(),
                level: level.into(),
                actors: actors.clone(),
                waypoints: Vec::new(),
            };
            let props = Props {
                identity: identity(),
                level: level.into(),
                props: vec![prop("banker.mdl", true), prop("tortoise.mdl", true)],
                refused: Vec::new(),
            };
            skinned_models(&placements, &props, &identity())
                .into_iter()
                .map(|(p, _)| p)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            models("level_214_tutotial_2"),
            ["banker.mdl", "tortoise.mdl"]
        );
        assert_eq!(models("level_201"), ["tortoise.mdl"]);
    }

    /// The models the first fight's placements name, each once, in their
    /// order: two Chompies on one model, Norticus on another; one naming a
    /// static model and placements off the fight left out. Then the skinned
    /// templates' (AG.150, AS.63's rule), each once, in the file's order:
    /// the cannon top's, the tortoises', a hat's; a pebble
    /// `props::UNDRAWN_SPAWNS` names and a static template left out, and a
    /// skinned placement that isn't a template. Their keys are the hero's
    /// skinned key, it blending as a creature fades (AH.139) and its glow
    /// mask, once each.
    #[test]
    fn the_first_fights_models_each_once_and_their_keys() {
        let placements = Placements {
            identity: identity(),
            level: "level_213_tutotial".into(),
            actors: vec![
                template("Push_Canon_Art_Top", "cannon_top.mdl"),
                template("2_Push_Block_Art_Sheep", "tortoise.mdl"),
                template("2_Push_Block_Art_Sheep", "tortoise.mdl"),
                template("Turban_hat", "hat.mdl"),
                template(crate::props::UNDRAWN_SPAWNS[0], "pebble.mdl"),
                template("Bird_nest_egg", "barrel.mdl"),
                placed("Enemy_Chompy(1)", Some("chompy.mdl")),
                placed("Enemy_Chompy(2)", Some("chompy.mdl")),
                placed("1.NPC_Norticus_First_Encounter", Some("mabu.mdl")),
                placed("Chompie_Dummies(4)", Some("barrel.mdl")),
                placed("Enemy_Chompy(99)", Some("other.mdl")),
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
                prop("cannon_top.mdl", true),
                prop("tortoise.mdl", true),
                prop("hat.mdl", true),
                prop("pebble.mdl", true),
            ],
            refused: Vec::new(),
        };
        let models = skinned_models(&placements, &props, &identity());
        assert_eq!(
            models.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            [
                "chompy.mdl",
                "mabu.mdl",
                "cannon_top.mdl",
                "tortoise.mdl",
                "hat.mdl"
            ]
        );
        let scene = Scene {
            fog: true,
            point_lights: vec![crate::level_scene::DARK; crate::level_scene::MAX_POINT_LIGHTS],
            ..Scene::new()
        };
        let keys = keys(&models, &scene).unwrap();
        assert_eq!(keys.len(), 3, "{keys:?}");
        assert!(keys.iter().all(|k| k.mesh.skinned));
        // AH.139: its opaque colour key again, blending as a creature fades,
        // ahead of the glow mask.
        let mut faded = keys[0];
        faded.material.states = keys[0].material.states.faded();
        assert_ne!(faded, keys[0]);
        assert_eq!(keys[1], faded);
        assert_eq!(
            keys[2].material.pass,
            crate::character::key::Pass::LevelGlow
        );
    }
}
