//! Shadows on a level (openspec AG.23, design decision 9): Bevy's cascaded
//! shadow map of a directional light that draws nothing but its cascades,
//! which the material's shadow-receive term reads through
//! [`super::remaster::shadow_sample`]. The original draws its own map (its
//! casters in grey into a 960x540 target, blurred 3x3) and projects it onto
//! the receivers nearly straight down, not along its key light
//! ([`SHADOW_TRAVEL`]), so a character's shadow lies under him; the
//! remaster takes Bevy's cascades along that projection instead, the start
//! of the Enhanced line.
//!
//! What casts: a character's skinned parts whose key casts
//! ([`MaterialKey::casts_shadow`]): the hero's, the creatures', and the
//! warm's triangles of their keys, which compile the cascades' depth
//! pipelines before play. Everything else the character material draws
//! (a level's scenery, its props and what its scripts spawn, glow masks,
//! particles, what a figure carries on a static mesh) casts none
//! ([`mark_casters`]). Which of its draws the original casts is unread
//! (AC.13's frame drew 113 into its map): this is a reading for the look,
//! chotchki's to judge. A light reaches only the views on its render
//! layers, so a card's turntable gets no cascades.
//!
//! [`MaterialKey::casts_shadow`]: super::key::MaterialKey::casts_shadow

use bevy::asset::Assets;
use bevy::ecs::prelude::*;
use bevy::light::{
    CascadeShadowConfig, CascadeShadowConfigBuilder, DirectionalLight, NotShadowCaster,
};
use bevy::math::{Dir3, Vec3};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::MeshMaterial3d;
use bevy::transform::components::Transform;

use super::material::CharacterMaterial;

/// The cascades over the level camera's view, in metres along it: three
/// of them out to 60 m, the first to 12 m (the following camera stands
/// about 8-10 m from the hero), overlapping a fifth. A look choice, not a
/// measurement: past 60 m nothing casts, and the fog is thick there.
pub const CASCADES: usize = 3;
pub const FIRST_CASCADE_FAR: f32 = 12.0;
pub const SHADOW_DISTANCE: f32 = 60.0;

/// [`CASCADES`] to [`SHADOW_DISTANCE`], the near one ending at
/// [`FIRST_CASCADE_FAR`].
pub fn cascades() -> CascadeShadowConfig {
    CascadeShadowConfigBuilder {
        num_cascades: CASCADES,
        minimum_distance: 0.1,
        maximum_distance: SHADOW_DISTANCE,
        first_cascade_far_bound: FIRST_CASCADE_FAR,
        overlap_proportion: 0.2,
    }
    .build()
}

/// The way the original projects its shadow map onto chapter 1, in the
/// level's frame: 2.97 degrees off straight down, leaning toward heading
/// -29.9 (the axis of `tfb_shadowMapTextureMatrix`'s two rows on the 33
/// level draws that receive, identical in all three of AC.13's captures).
/// Its key light travels 30 degrees down toward 73 and the level file's
/// `_worldLight1` 45 down toward 0, neither of them this: the original's
/// shadow is a drop shadow, under the character, as the oracle's frames
/// show it. A constant off the captures until the engine's own setting is
/// read (stand-in ledger: `shadow-travel`).
pub const SHADOW_TRAVEL: [f32; 3] = [-0.0258, -0.9987, 0.0448];

/// A light travelling `travel` (any length; straight down if it has none)
/// that draws shadow maps: what the level spawns on its layer, along
/// [`SHADOW_TRAVEL`]. Its colour and illuminance light nothing, since the
/// character material reads its own lights; only its cascades are read.
pub fn shadow_light(travel: Vec3) -> (DirectionalLight, Transform, CascadeShadowConfig) {
    let travel = Dir3::new(travel).unwrap_or(Dir3::NEG_Y);
    // Up along the world's Y unless the light travels along it.
    let up = if travel.y.abs() > 0.99 {
        Dir3::Z
    } else {
        Dir3::Y
    };
    (
        DirectionalLight {
            shadow_maps_enabled: true,
            ..Default::default()
        },
        Transform::default().looking_to(travel, up),
        cascades(),
    )
}

/// Whether a part casts: only a character's skinned parts do, since a
/// static mesh drawn on the character material is a level's (its light
/// painted into its colour map) or what a figure carries.
pub fn casts(skinned: bool, material: Option<&CharacterMaterial>) -> bool {
    skinned && material.is_some_and(|m| m.key.casts_shadow())
}

/// A part [`mark_casters`] reads: its material, whether it's skinned and
/// whether it casts none now.
type Part = (
    Entity,
    &'static MeshMaterial3d<CharacterMaterial>,
    Has<SkinnedMesh>,
    Has<NotShadowCaster>,
);

/// Each character-material mesh whose material changed (or came) marked
/// to cast or not ([`casts`]): Bevy's `NotShadowCaster` on every part but
/// a character's opaque skinned ones. In `Last`, so a part spawned or given
/// a new material (a fade's copy) anywhere in the frame has it before the
/// render world extracts it. A material whose key changes in place keeps
/// its mark until its handle does.
pub fn mark_casters(
    mut commands: Commands,
    parts: Query<Part, Changed<MeshMaterial3d<CharacterMaterial>>>,
    materials: Res<Assets<CharacterMaterial>>,
) {
    for (entity, material, skinned, off) in &parts {
        match (casts(skinned, materials.get(&material.0)), off) {
            (true, true) => {
                commands.entity(entity).remove::<NotShadowCaster>();
            }
            (false, false) => {
                commands.entity(entity).insert(NotShadowCaster);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::key::{MaterialKey, Pass, ShaderKey, StateKey};
    use crate::character::material::CharacterTextures;
    use bevy::app::{App, Last};
    use bevy::asset::{AssetApp, AssetPlugin, Handle};
    use bevy::mesh::Mesh3d;
    use bevy::prelude::MinimalPlugins;
    use guac_ir::model::Blend;
    use guac_ir::shading::Scene;

    fn material(states: StateKey, pass: Pass) -> CharacterMaterial {
        CharacterMaterial::new(
            MaterialKey {
                shader: ShaderKey::new(0, 0).unwrap(),
                states,
                pass,
            },
            &guac_ir::shading::Material::new(),
            &Scene::new(),
            0.5,
            CharacterTextures {
                colour: Some(Handle::default()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<CharacterMaterial>()
            .add_systems(Last, mark_casters);
        app
    }

    fn part(app: &mut App, m: CharacterMaterial, skinned: bool) -> Entity {
        let h = app
            .world_mut()
            .resource_mut::<Assets<CharacterMaterial>>()
            .add(m);
        let mut e = app
            .world_mut()
            .spawn((Mesh3d(Handle::default()), MeshMaterial3d(h)));
        if skinned {
            e.insert(SkinnedMesh::default());
        }
        e.id()
    }

    fn cast(app: &App, e: Entity) -> bool {
        !app.world().entity(e).contains::<NotShadowCaster>()
    }

    #[test]
    fn only_a_characters_opaque_skinned_parts_cast() {
        let mut app = app();
        let opaque = StateKey::OPAQUE;
        let blended = StateKey {
            blend: Blend::Alpha,
            ..opaque
        };
        let tested = StateKey {
            alpha_test: true,
            ..opaque
        };
        let unwritten = StateKey {
            depth_write: false,
            ..opaque
        };
        let body = part(&mut app, material(opaque, Pass::Colour), true);
        let scenery = part(&mut app, material(opaque, Pass::Colour), false);
        let veil = part(&mut app, material(blended, Pass::Colour), true);
        let leaves = part(&mut app, material(tested, Pass::Colour), true);
        let flat = part(&mut app, material(unwritten, Pass::Colour), true);
        let mask = part(&mut app, material(opaque, Pass::LevelGlow), true);
        let card_mask = part(&mut app, material(opaque, Pass::GlowMask), true);
        app.update();
        assert!(cast(&app, body), "an opaque skinned part casts");
        for (e, what) in [
            (scenery, "a static part"),
            (veil, "a blended part"),
            (leaves, "an alpha-tested part"),
            (flat, "a part writing no depth"),
            (mask, "a level glow mask"),
            (card_mask, "a card's glow mask"),
        ] {
            assert!(!cast(&app, e), "{what} casts none");
        }

        // A fade's copy on the body (a new handle, blended) stops it
        // casting, and the opaque one back starts it again.
        let faded = app
            .world_mut()
            .resource_mut::<Assets<CharacterMaterial>>()
            .add(material(blended, Pass::Colour));
        let solid = app
            .world()
            .entity(body)
            .get::<MeshMaterial3d<CharacterMaterial>>()
            .unwrap()
            .0
            .clone();
        app.world_mut()
            .entity_mut(body)
            .insert(MeshMaterial3d(faded));
        app.update();
        assert!(!cast(&app, body), "faded, it casts none");
        app.world_mut()
            .entity_mut(body)
            .insert(MeshMaterial3d(solid));
        app.update();
        assert!(cast(&app, body), "opaque again, it casts");

        // A part whose material isn't there casts none.
        let lost = app
            .world_mut()
            .spawn((
                Mesh3d(Handle::default()),
                MeshMaterial3d::<CharacterMaterial>(Handle::default()),
                SkinnedMesh::default(),
            ))
            .id();
        app.update();
        assert!(!cast(&app, lost));
    }

    #[test]
    fn the_shadow_light_travels_its_way_and_draws_cascades() {
        let travel = Vec3::new(0.2, -0.5, 0.8);
        let (light, transform, cascades) = shadow_light(travel);
        assert!(light.shadow_maps_enabled);
        assert!(
            transform.forward().dot(travel.normalize()) > 1.0 - 1e-6,
            "{:?}",
            transform.forward()
        );
        assert_eq!(cascades.bounds.len(), CASCADES);
        assert!((cascades.bounds[CASCADES - 1] - SHADOW_DISTANCE).abs() < 1e-3);
        // Straight down, and with no length (down too).
        let (_, down, _) = shadow_light(Vec3::NEG_Y * 3.0);
        assert!(down.forward().dot(Vec3::NEG_Y) > 1.0 - 1e-6);
        let (_, none, _) = shadow_light(Vec3::ZERO);
        assert!(none.forward().dot(Vec3::NEG_Y) > 1.0 - 1e-6);
        // Chapter 1's: under three degrees off straight down, leaning
        // toward heading -30.
        let t = Vec3::from_array(SHADOW_TRAVEL);
        assert!((t.length() - 1.0).abs() < 1e-3);
        let tilt = (-t.y / t.length()).acos().to_degrees();
        let heading = t.x.atan2(t.z).to_degrees();
        assert!(
            (tilt - 2.97).abs() < 0.02 && (heading + 29.9).abs() < 0.1,
            "{tilt} {heading}"
        );
        let (_, level, _) = shadow_light(t);
        assert!(level.forward().dot(t.normalize()) > 1.0 - 1e-6);
    }
}
