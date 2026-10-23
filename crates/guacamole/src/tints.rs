//! A placement's tint on what the scripts spawn (openspec AH.67): each
//! clone's `placement.tint` as the scripts leave it, drawn on its parts.
//!
//! The shader takes a placement tint already (the character-shading spec's
//! tints: four times it and the material's, neutral at 128, its alpha times
//! the part's), but a spawn model's parts share one material a model
//! (`Level::spawn_models`), so a clone the scripts tint draws on copies of
//! its parts' materials, one a part, its tint in the copy's uniform: each
//! colour channel's ratio to neutral raised to 2.2, since the original
//! multiplies stored values (`remaster::tint_in_light`). The eruption's
//! lava is the case on the disc (AH.54's recordings): each blob's alpha
//! from 0 to 255 over 0.25 s as it comes, then its colour down to (34, 20,
//! 20) over 0.5 s once the eruption's going black. Its parts are opaque on
//! the disc, and the oracle's frames show the blobs coming in see through,
//! so while a tint's alpha is under full an opaque part blends by it
//! ([`StateKey::faded`]): the census holds those keys too
//! (`props::faded_keys`), so the start's pass has compiled them.
//!
//! On the drawing side: [`tint_clones`] runs in `PostUpdate` on what the
//! last step left, a copy kept while its clone lives (its parts go with the
//! clone) and rewritten only when the tint or its shared material's numbers
//! (a relight) moved. A clone left neutral draws on the shared material.

use bevy::math::Vec4;
use bevy::prelude::*;
use guacamole::character::key::StateKey;
use guacamole::character::{CharacterMaterial, remaster};
use guacamole::scripts::{ScriptClone, ScriptWorld};

use crate::hero_scripts::CloneDrawn;

/// A placement's tint as the scripts start one: grey 128, alpha 255.
pub(crate) const NEUTRAL: u32 = 0x8080_80FF;

/// A clone's part drawing on a material of its own: the shared one it was
/// spawned with, which the copy follows.
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) struct TintedPart {
    pub(crate) shared: Handle<CharacterMaterial>,
}

/// A packed tint (RGBA, red in the high byte, as the scripts hold one) as
/// the uniform takes it: in linear light, each colour channel's ratio to
/// neutral raised to 2.2, alpha as stored ([`remaster::tint_in_light`]).
pub(crate) fn tint_of(packed: u32) -> Vec4 {
    Vec4::from_array(remaster::tint_in_light(packed.to_be_bytes()))
}

/// The states `shared`'s part draws with at the tint's alpha: its own at
/// full, faded under it.
pub(crate) fn states_at(shared: StateKey, packed: u32) -> StateKey {
    if packed & 0xFF < 0xFF {
        shared.faded()
    } else {
        shared
    }
}

/// A copy of `shared` carrying `packed` as its placement tint, on the states
/// its alpha gives ([`states_at`]).
pub(crate) fn tinted(shared: &CharacterMaterial, packed: u32) -> CharacterMaterial {
    let mut m = shared.clone();
    m.uniform.placement_tint = tint_of(packed);
    m.key.states = states_at(shared.key.states, packed);
    m
}

/// Whether `copy` draws as `want`: its numbers and its key.
fn same(copy: &CharacterMaterial, want: &CharacterMaterial) -> bool {
    copy.uniform == want.uniform && copy.key == want.key
}

pub(crate) struct CloneTintsPlugin;

impl Plugin for CloneTintsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            tint_clones.run_if(resource_exists::<ScriptWorld>),
        );
    }
}

/// Each drawn clone's tint onto its parts, as the scripts' world holds it.
pub(crate) fn tint_clones(
    commands: Commands,
    world: Res<ScriptWorld>,
    clones: Query<(Entity, &CloneDrawn, &Children), With<ScriptClone>>,
    parts: Query<(&mut MeshMaterial3d<CharacterMaterial>, Option<&TintedPart>)>,
    materials: ResMut<Assets<CharacterMaterial>>,
) {
    tint_parts(
        |clone| world.actor(clone.to_bits()).map(|a| a.tint),
        commands,
        clones,
        parts,
        materials,
    );
}

/// Each drawn clone's tint (`tint_of_clone`) onto its parts: a tinted
/// clone's parts on copies of their shared materials carrying it, made the
/// first time it isn't neutral and rewritten when it or the shared one's
/// numbers move.
fn tint_parts(
    tint_of_clone: impl Fn(Entity) -> Option<u32>,
    mut commands: Commands,
    clones: Query<(Entity, &CloneDrawn, &Children), With<ScriptClone>>,
    mut parts: Query<(&mut MeshMaterial3d<CharacterMaterial>, Option<&TintedPart>)>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
) {
    for (clone, drawn, children) in &clones {
        if !drawn.draws {
            continue;
        }
        let Some(tint) = tint_of_clone(clone) else {
            continue;
        };
        for &part in children {
            let Ok((mut material, tinted_part)) = parts.get_mut(part) else {
                continue;
            };
            let shared = match tinted_part {
                Some(t) => t.shared.clone(),
                None if tint == NEUTRAL => continue,
                None => material.0.clone(),
            };
            let Some(want) = materials.get(&shared).map(|m| tinted(m, tint)) else {
                continue;
            };
            if tinted_part.is_none() {
                material.0 = materials.add(want);
                commands.entity(part).insert(TintedPart { shared });
            } else if materials.get(&material.0).is_none_or(|m| !same(m, &want))
                && let Some(mut copy) = materials.get_mut(&material.0)
            {
                *copy = want;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guac_ir::model::Blend;
    use guacamole::character::key::{MaterialKey, Pass, ShaderKey};
    use guacamole::character::material::{CharacterTextures, ShadowLookup};

    /// The lava's part as the disc has it: opaque, depth written, its own
    /// colour map, neutral tints.
    fn lava() -> CharacterMaterial {
        let key = MaterialKey {
            shader: ShaderKey::new(0, 0).unwrap(),
            states: StateKey::OPAQUE,
            pass: Pass::Colour,
        };
        let ir = guac_ir::shading::Material::new();
        let textures = CharacterTextures {
            colour: Some(Handle::default()),
            ..CharacterTextures::default()
        };
        CharacterMaterial::new(
            key,
            &ir,
            &guac_ir::shading::Scene::new(),
            ShadowLookup::NONE,
            0.0,
            textures,
        )
        .unwrap()
    }

    #[test]
    fn a_tint_reaches_the_uniform_in_light_and_fades_an_opaque_part() {
        let shared = lava();
        assert_eq!(
            tint_of(NEUTRAL),
            Vec4::new(128.0, 128.0, 128.0, 255.0) / 255.0
        );
        // AH.54's first blob five records in: alpha 83, the part blends.
        let coming = tinted(&shared, 0x8080_8053);
        assert_eq!(
            coming.uniform.placement_tint,
            Vec4::new(128.0, 128.0, 128.0, 83.0) / 255.0
        );
        assert_eq!(coming.key.states.blend, Blend::Alpha);
        assert!(coming.key.states.depth_write, "its own depth write kept");
        // Going black: full alpha, opaque again, the colour down in light.
        let black = tinted(&shared, 0x2214_14FF);
        assert_eq!(black.key.states, StateKey::OPAQUE);
        assert_eq!(
            black.uniform.placement_tint,
            Vec4::from_array(remaster::tint_in_light([34, 20, 20, 255]))
        );
        assert!(black.uniform.placement_tint.x < 0.25 * 34.0 / 255.0);
        // Everything else is the shared material's.
        let mut back = black.clone();
        back.uniform.placement_tint = shared.uniform.placement_tint;
        assert!(same(&back, &shared));
        // A part that blends already keeps its own blend.
        let mut added = shared.clone();
        added.key.states.blend = Blend::Additive;
        assert_eq!(
            tinted(&added, 0x8080_8000).key.states.blend,
            Blend::Additive
        );
    }

    /// The tints a test hands the parts, by clone.
    #[derive(Resource, Default)]
    struct Tints(std::collections::HashMap<Entity, u32>);

    fn tint_from_test(
        tints: Res<Tints>,
        commands: Commands,
        clones: Query<(Entity, &CloneDrawn, &Children), With<ScriptClone>>,
        parts: Query<(&mut MeshMaterial3d<CharacterMaterial>, Option<&TintedPart>)>,
        materials: ResMut<Assets<CharacterMaterial>>,
    ) {
        tint_parts(
            |e| tints.0.get(&e).copied(),
            commands,
            clones,
            parts,
            materials,
        );
    }

    /// A clone's tint as the scripts leave it reaches what draws its parts:
    /// a neutral one on the shared material, a tinted one on a copy of its
    /// own carrying the tint and faded while its alpha's under full, kept
    /// and rewritten as the tint moves; the shared material untouched, and a
    /// clone that draws nothing left alone.
    #[test]
    fn a_clones_tint_and_alpha_reach_what_draws_its_parts() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<CharacterMaterial>>()
            .init_resource::<Tints>()
            .add_systems(PostUpdate, tint_from_test);
        let shared = app
            .world_mut()
            .resource_mut::<Assets<CharacterMaterial>>()
            .add(lava());
        let mut spawn = |draws: bool| {
            let clone = app
                .world_mut()
                .spawn((
                    ScriptClone(guac_script::Value::Int(1)),
                    CloneDrawn {
                        draws,
                        attack: true,
                    },
                ))
                .id();
            let part = app
                .world_mut()
                .spawn((MeshMaterial3d(shared.clone()), ChildOf(clone)))
                .id();
            (clone, part)
        };
        let (clone, part) = spawn(true);
        let (hidden, hidden_part) = spawn(false);
        let set_tint = |app: &mut App, tint: u32| {
            let mut tints = app.world_mut().resource_mut::<Tints>();
            tints.0.insert(clone, tint);
            tints.0.insert(hidden, tint);
            app.update();
        };
        let drawn = |app: &App, part: Entity| {
            let handle = &app
                .world()
                .entity(part)
                .get::<MeshMaterial3d<CharacterMaterial>>()
                .unwrap()
                .0;
            let m = app
                .world()
                .resource::<Assets<CharacterMaterial>>()
                .get(handle)
                .unwrap();
            (handle.clone(), m.uniform.placement_tint, m.key.states.blend)
        };
        let n = |c: f32| c / 255.0;
        set_tint(&mut app, NEUTRAL);
        assert_eq!(drawn(&app, part).0, shared, "neutral: the shared material");

        set_tint(&mut app, 0x8080_8000);
        let (copy, t, blend) = drawn(&app, part);
        assert_ne!(copy, shared);
        assert_eq!(
            (t.w, blend),
            (0.0, Blend::Alpha),
            "comes in unseen, blending"
        );
        assert_eq!(
            app.world().entity(part).get::<TintedPart>(),
            Some(&TintedPart {
                shared: shared.clone()
            })
        );
        assert_eq!(
            drawn(&app, hidden_part).0,
            shared,
            "a hidden clone's left alone"
        );
        set_tint(&mut app, 0x8080_80A8);
        assert_eq!(
            drawn(&app, part),
            (
                copy.clone(),
                Vec4::new(n(128.0), n(128.0), n(128.0), n(168.0)),
                Blend::Alpha
            )
        );
        set_tint(&mut app, 0x2214_14FF);
        assert_eq!(
            drawn(&app, part),
            (copy, tint_of(0x2214_14FF), Blend::Opaque),
            "gone black on the same copy, opaque again"
        );
        let shared_now = app
            .world()
            .resource::<Assets<CharacterMaterial>>()
            .get(&shared)
            .unwrap()
            .clone();
        assert_eq!(shared_now.uniform.placement_tint, tint_of(NEUTRAL));
        assert_eq!(shared_now.key.states, StateKey::OPAQUE);
    }
}
