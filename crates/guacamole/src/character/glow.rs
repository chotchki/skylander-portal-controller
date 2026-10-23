//! The game's glow (openspec AE.11, design decision 13; the
//! character-shading spec's glow requirements), on the remaster's linear
//! frame before tonemapping.
//!
//! Two pieces. The mask: each part is drawn a second time with its
//! material's glow pass ([`CharacterMaterial::glow_mask`]) by a second
//! camera ([`spawn_glow_camera`]) into an `Rgba16Float` image the frame's
//! size, on a render layer of its own ([`spawn_glow_masks`]), cleared to
//! zero, so only the nearest surface writes. The chain: on the frame's
//! camera ([`Glow`]), after every part is drawn and before tonemapping, the
//! frame times the mask goes into a buffer 512/960 of its width and half
//! its height, is blurred across and down and is added back twice over
//! ([`glow.wesl`](glow.wesl)), the arithmetic [`guac_ir::glow`] checks.
//!
//! A level (openspec AH.65) runs the same chain on its camera's frame with
//! the mask in the frame's own alpha ([`FrameGlow`]), the original's order
//! kept in one camera: everything draws, a fullscreen [`AlphaClear`] then
//! clears the alpha (it sorts after every blended part, [`CLEAR_BIAS`]),
//! and each glowing part's mask ([`super::Pass::LevelGlow`]) writes alpha
//! alone, tested against the level's depth, sorting after that
//! ([`LEVEL_GLOW_BIAS`]); the chain reads the frame times its alpha and adds
//! back with the alpha set opaque again.

use bevy::app::{App, Plugin};
use bevy::asset::{
    Asset, AssetServer, Assets, Handle, RenderAssetUsages, embedded_asset, uuid_handle,
};
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::camera::{Camera, Camera3d, ClearColorConfig, Hdr, Projection, RenderTarget};
use bevy::color::Color;
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping, tonemapping};
use bevy::ecs::prelude::*;
use bevy::ecs::query::QueryItem;
use bevy::image::Image;
use bevy::math::{UVec2, Vec4};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::mesh::{Indices, Mesh, Mesh3d, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin, MeshMaterial3d};
use bevy::prelude::{ChildOf, Transform};
use bevy::reflect::TypePath;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    AsBindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    BlendComponent, BlendFactor, BlendOperation, BlendState, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, CompareFunction, FragmentState, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, ShaderStages,
    ShaderType, SpecializedMeshPipelineError, TextureDescriptor, TextureDimension, TextureFormat,
    TextureSampleType, TextureUsages, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::sync_component::SyncComponent;
use bevy::render::texture::{CachedTexture, GpuImage, TextureCache};
use bevy::render::view::{Msaa, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::{Shader, ShaderRef};
use guac_ir::model::Model;

use super::material::CharacterMaterial;
use super::mesh::SpawnedCharacter;

/// The shader, embedded by [`GlowPlugin`].
pub const SHADER: &str = "embedded://guacamole/character/glow.wesl";
/// [`AlphaClear`]'s shader, embedded by [`GlowPlugin`].
pub const CLEAR_SHADER: &str = "embedded://guacamole/character/alpha_clear.wesl";

/// Where a level's [`AlphaClear`] sorts among the blended parts (Bevy sorts
/// them by view depth plus their bias, nearest last): after every one of
/// them, its own depth a few metres either side of nothing.
pub const CLEAR_BIAS: f32 = 1.0e6;
/// Where a level's glow masks sort: after the [`AlphaClear`], in their own
/// depth order among themselves.
pub const LEVEL_GLOW_BIAS: f32 = 2.0e6;

/// The fullscreen triangle an [`AlphaClear`] draws, in clip space.
pub const CLEAR_MESH: Handle<Mesh> = uuid_handle!("5a7e0b8c-2f61-4d1e-8c3a-9b0d4e6f1a01");
/// The one [`AlphaClear`] material.
pub const CLEAR_MATERIAL: Handle<AlphaClear> = uuid_handle!("5a7e0b8c-2f61-4d1e-8c3a-9b0d4e6f1a02");

/// On an HDR 3D camera that draws a level: run the glow chain on its frame
/// with the mask its own alpha holds ([`spawn_alpha_clear`] clears it, the
/// level's glow masks write it), and leave the frame opaque.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct FrameGlow;

impl SyncComponent<RenderApp> for FrameGlow {
    type Target = Self;
}

impl ExtractComponent<RenderApp> for FrameGlow {
    type QueryData = &'static Self;
    type QueryFilter = With<Hdr>;
    type Out = Self;

    fn extract_component(glow: QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        Some(*glow)
    }
}

/// Clears a level frame's alpha to none, colour untouched, wherever it
/// draws (a triangle over the whole view, every depth): what the level's
/// glow masks then write into. Blended, so it sorts with the blended parts,
/// and after them all ([`CLEAR_BIAS`]).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct AlphaClear {}

impl Material for AlphaClear {
    fn vertex_shader() -> ShaderRef {
        CLEAR_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        CLEAR_SHADER.into()
    }

    fn alpha_mode(&self) -> bevy::material::AlphaMode {
        bevy::material::AlphaMode::Blend
    }

    fn depth_bias(&self) -> f32 {
        CLEAR_BIAS
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![
            layout
                .0
                .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?,
        ];
        clear_states(descriptor);
        Ok(())
    }
}

/// [`AlphaClear`]'s states: both faces, every depth passing and none
/// written, alpha alone written, unblended.
fn clear_states(descriptor: &mut RenderPipelineDescriptor) {
    descriptor.primitive.cull_mode = None;
    if let Some(depth) = descriptor.depth_stencil.as_mut() {
        depth.depth_write_enabled = Some(false);
        depth.depth_compare = Some(CompareFunction::Always);
    }
    if let Some(fragment) = descriptor.fragment.as_mut() {
        for target in fragment.targets.iter_mut().flatten() {
            target.blend = None;
            target.write_mask = ColorWrites::ALPHA;
        }
    }
}

/// [`CLEAR_MESH`]: one triangle whose corners cover clip space.
pub fn clear_mesh() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[-1.0f32, -1.0, 0.0], [3.0, -1.0, 0.0], [-1.0, 3.0, 0.0]],
    )
    .with_inserted_indices(Indices::U16(vec![0, 1, 2]))
}

/// The level's [`AlphaClear`] for `camera`, on `layer` (the camera's): a
/// child, so it goes with it.
pub fn spawn_alpha_clear(commands: &mut Commands, camera: Entity, layer: usize) -> Entity {
    commands
        .spawn((
            Mesh3d(CLEAR_MESH),
            MeshMaterial3d(CLEAR_MATERIAL),
            Transform::IDENTITY,
            NoFrustumCulling,
            RenderLayers::layer(layer),
            ChildOf(camera),
        ))
        .id()
}

/// The mask, the buffer and the frame they're added back into: half
/// floats, so the chain stays linear and unclamped.
pub const FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// On an HDR 3D camera: run the glow chain on its frame with the mask its
/// glow camera draws into `mask`.
#[derive(Component, Debug, Clone)]
pub struct Glow {
    pub mask: Handle<Image>,
}

impl SyncComponent<RenderApp> for Glow {
    type Target = Self;
}

impl ExtractComponent<RenderApp> for Glow {
    type QueryData = &'static Self;
    type QueryFilter = With<Hdr>;
    type Out = Self;

    fn extract_component(glow: QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        Some(glow.clone())
    }
}

/// Registers [`Glow`] and runs the chain on every camera that has it.
pub struct GlowPlugin;

impl Plugin for GlowPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "glow.wesl");
        embedded_asset!(app, "alpha_clear.wesl");
        app.add_plugins((
            ExtractComponentPlugin::<Glow>::default(),
            ExtractComponentPlugin::<FrameGlow>::default(),
            MaterialPlugin::<AlphaClear>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(
                Render,
                prepare_textures.in_set(RenderSystems::PrepareResources),
            )
            .add_systems(
                Core3d,
                glow.before(tonemapping).in_set(Core3dSystems::PostProcess),
            );
    }

    // After every plugin's build, so the asset stores are there.
    fn finish(&self, app: &mut App) {
        if let Some(mut meshes) = app.world_mut().get_resource_mut::<Assets<Mesh>>() {
            let _ = meshes.insert(CLEAR_MESH.id(), clear_mesh());
        }
        if let Some(mut clears) = app.world_mut().get_resource_mut::<Assets<AlphaClear>>() {
            let _ = clears.insert(CLEAR_MATERIAL.id(), AlphaClear {});
        }
    }
}

/// The buffer's size for a frame (the spec's 512/960 of its width by half
/// its height), from the CPU reference so the two can't disagree.
pub fn buffer_size(frame: UVec2) -> UVec2 {
    let (w, h) = guac_ir::glow::buffer_size(frame.x as usize, frame.y as usize);
    UVec2::new(w as u32, h as u32)
}

/// What every glow step reads, as `glow.wesl`'s `Glow`: the frame's and
/// the buffer's sizes and the taps' weights.
#[derive(Debug, Clone, Copy, PartialEq, ShaderType)]
pub struct GlowUniform {
    pub frame: UVec2,
    pub buffer: UVec2,
    /// Tap 0 (the centre) through tap 7.
    pub weights: [Vec4; 2],
}

impl GlowUniform {
    pub fn new(frame: UVec2) -> Self {
        let w = |k| guac_ir::glow::tap_weight(k);
        Self {
            frame,
            buffer: buffer_size(frame),
            weights: [
                Vec4::new(w(0), w(1), w(2), w(3)),
                Vec4::new(w(4), w(5), w(6), w(7)),
            ],
        }
    }
}

#[derive(Resource)]
struct GlowPipelines {
    layout: BindGroupLayoutDescriptor,
    extract: CachedRenderPipelineId,
    across: CachedRenderPipelineId,
    down: CachedRenderPipelineId,
    add_back: CachedRenderPipelineId,
    /// A level's ([`FrameGlow`]): the mask off the frame's alpha, and the
    /// add-back leaving the alpha opaque.
    extract_alpha: CachedRenderPipelineId,
    add_back_opaque: CachedRenderPipelineId,
}

/// Adds colour and sets alpha to what's written (the add-back writes 1).
const ADD_COLOUR_OPAQUE: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::Zero,
        operation: BlendOperation::Add,
    },
};

/// Adds colour and leaves what's there in alpha.
const ADD_COLOUR: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::Zero,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
};

fn init_pipelines(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    pipeline_cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "glow_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: false }),
                uniform_buffer::<GlowUniform>(false),
            ),
        ),
    );
    let shader: Handle<Shader> = asset_server.load(SHADER);
    let pipeline = |label: &'static str, entry: &'static str, blend: Option<BlendState>| {
        pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(label.into()),
            layout: vec![layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: shader.clone(),
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format: FORMAT,
                    blend,
                    write_mask: ColorWrites::ALL,
                })],
                ..Default::default()
            }),
            ..Default::default()
        })
    };
    let pipelines = GlowPipelines {
        extract: pipeline("glow_extract", "extract", None),
        across: pipeline("glow_across", "across", None),
        down: pipeline("glow_down", "down", None),
        add_back: pipeline("glow_add_back", "add_back", Some(ADD_COLOUR)),
        extract_alpha: pipeline("glow_extract_alpha", "extract_alpha", None),
        add_back_opaque: pipeline(
            "glow_add_back_opaque",
            "add_back_opaque",
            Some(ADD_COLOUR_OPAQUE),
        ),
        layout,
    };
    commands.insert_resource(pipelines);
}

/// A view's two buffers (the extract and the down pass write one, the
/// across pass the other) and its uniform.
#[derive(Component)]
struct GlowTextures {
    first: CachedTexture,
    second: CachedTexture,
    uniform: UniformBuffer<GlowUniform>,
}

/// The views a chain runs on: a card's ([`Glow`]) or a level's
/// ([`FrameGlow`]).
type GlowViews = Or<(With<Glow>, With<FrameGlow>)>;

fn prepare_textures(
    mut commands: Commands,
    mut cache: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    views: Query<(Entity, &ExtractedCamera), GlowViews>,
) {
    for (entity, camera) in &views {
        let Some(frame) = camera.physical_viewport_size else {
            continue;
        };
        if frame.x == 0 || frame.y == 0 {
            continue;
        }
        let size = buffer_size(frame);
        let descriptor = |label| TextureDescriptor {
            label: Some(label),
            size: bevy::render::render_resource::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let mut uniform = UniformBuffer::from(GlowUniform::new(frame));
        uniform.write_buffer(&device, &queue);
        commands.entity(entity).insert(GlowTextures {
            first: cache.get(&device, descriptor("glow_first")),
            second: cache.get(&device, descriptor("glow_second")),
            uniform,
        });
    }
}

fn glow(
    view: ViewQuery<(&ExtractedCamera, &ViewTarget, Option<&Glow>, &GlowTextures)>,
    pipelines: Option<Res<GlowPipelines>>,
    pipeline_cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
) {
    let (camera, target, glow, textures) = view.into_inner();
    let Some(pipelines) = pipelines else { return };
    if !camera.hdr || target.main_texture_format() != FORMAT {
        return;
    }
    // A card's chain reads its glow camera's mask; a level's, its frame's
    // alpha (`FrameGlow`, the only other view prepared).
    let (extract, add_back) = match glow {
        Some(_) => (pipelines.extract, pipelines.add_back),
        None => (pipelines.extract_alpha, pipelines.add_back_opaque),
    };
    let get = |id| pipeline_cache.get_render_pipeline(id);
    let (Some(extract), Some(across), Some(down), Some(add_back)) = (
        get(extract),
        get(pipelines.across),
        get(pipelines.down),
        get(add_back),
    ) else {
        return;
    };
    let Some(uniform) = textures.uniform.binding() else {
        return;
    };
    let mask = match glow {
        Some(glow) => match images.get(&glow.mask) {
            Some(mask) => Some(mask),
            None => return,
        },
        None => None,
    };
    let layout = pipeline_cache.get_bind_group_layout(&pipelines.layout);
    let (first, second) = (&textures.first.default_view, &textures.second.default_view);
    let device = ctx.render_device();
    let bind = |label, a, b| {
        device.create_bind_group(
            label,
            &layout,
            &BindGroupEntries::sequential((a, b, uniform.clone())),
        )
    };
    let from_frame = bind(
        "glow_extract",
        target.main_texture_view(),
        mask.map_or(target.main_texture_view(), |m| &m.texture_view),
    );
    let from_first = bind("glow_from_first", first, first);
    let from_second = bind("glow_from_second", second, second);

    let encoder = ctx.command_encoder();
    encoder.push_debug_group("glow");
    for (label, view, pipeline, group) in [
        ("glow_extract", first, extract, &from_frame),
        ("glow_across", second, across, &from_first),
        ("glow_down", first, down, &from_second),
    ] {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
    {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("glow_add_back"),
            color_attachments: &[Some(target.get_unsampled_color_attachment())],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(add_back);
        pass.set_bind_group(0, &from_first, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.pop_debug_group();
}

/// What a glow camera takes from the frame's camera ([`spawn_glow_camera`]).
#[derive(Debug, Clone)]
pub struct GlowView {
    /// The frame's size, which the mask takes.
    pub size: UVec2,
    pub projection: Projection,
    pub msaa: Msaa,
    /// The glow camera's order: before the frame camera's own, so the mask
    /// is drawn when the chain reads it (-1 beside a camera at the default 0).
    pub order: isize,
}

/// The glow camera for `camera`: a child that sees only `layer`, draws the
/// glow masks there into a new image the frame's size (zero where nothing
/// glows) with the frame's projection and sample count, at `view`'s order.
/// Puts [`Glow`] on `camera` and returns the mask image.
pub fn spawn_glow_camera(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    camera: Entity,
    view: GlowView,
    layer: usize,
) -> Handle<Image> {
    let GlowView {
        size,
        projection,
        msaa,
        order,
    } = view;
    let mut image = Image::new_target_texture(size.x, size.y, FORMAT, None);
    image.asset_usage = RenderAssetUsages::default();
    let mask = images.add(image);
    commands.spawn((
        Camera3d::default(),
        Camera {
            order,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..Default::default()
        },
        RenderTarget::Image(mask.clone().into()),
        Hdr,
        Tonemapping::None,
        DebandDither::Disabled,
        msaa,
        projection,
        Transform::IDENTITY,
        RenderLayers::layer(layer),
        ChildOf(camera),
    ));
    commands.entity(camera).insert(Glow { mask: mask.clone() });
    mask
}

/// Each of `spawned`'s segments again on `layer`, with its material's glow
/// pass (`masks`, by the model's material index; `None` for a material
/// [`CharacterMaterial::glow_mask`] leaves out, whose segments aren't
/// drawn), skinned by the same joints.
pub fn spawn_glow_masks(
    commands: &mut Commands,
    model: &Model,
    spawned: &SpawnedCharacter,
    masks: &[Option<Handle<CharacterMaterial>>],
    layer: usize,
) -> Vec<Entity> {
    model
        .segments
        .iter()
        .zip(&spawned.meshes)
        .filter_map(|(s, mesh)| Some((masks[usize::from(s.material)].clone()?, mesh)))
        .map(|(mask, mesh)| {
            commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(mask),
                    SkinnedMesh {
                        inverse_bindposes: spawned.skin.clone(),
                        joints: spawned.joints.clone(),
                    },
                    NoFrustumCulling,
                    RenderLayers::layer(layer),
                    ChildOf(spawned.root),
                ))
                .id()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uniform_carries_the_frame_the_buffer_and_the_references_weights() {
        let u = GlowUniform::new(UVec2::new(960, 540));
        assert_eq!(
            (u.frame, u.buffer),
            (UVec2::new(960, 540), UVec2::new(512, 270))
        );
        let weights: Vec<f32> = u.weights.iter().flat_map(|v| v.to_array()).collect();
        let want: Vec<f32> = (0..8).map(guac_ir::glow::tap_weight).collect();
        assert_eq!(weights, want);
        let sum = weights[0] + 2.0 * weights[1..].iter().sum::<f32>();
        assert!((sum - 1.0).abs() < 1e-6);
        assert_eq!(buffer_size(UVec2::new(3840, 2160)), UVec2::new(2048, 1080));
    }

    /// AH.65: a level's add-back adds colour as a card's does and leaves
    /// the frame opaque (its alpha held the mask); its alpha clear writes
    /// alpha alone at every depth over the whole view, after the blended
    /// parts and before the masks.
    #[test]
    fn a_levels_chain_clears_its_alpha_and_leaves_the_frame_opaque() {
        let c = ADD_COLOUR_OPAQUE.color;
        assert_eq!(
            (c.src_factor, c.dst_factor, c.operation),
            (BlendFactor::One, BlendFactor::One, BlendOperation::Add)
        );
        let a = ADD_COLOUR_OPAQUE.alpha;
        assert_eq!(
            (a.src_factor, a.dst_factor),
            (BlendFactor::One, BlendFactor::Zero)
        );
        let mut d = RenderPipelineDescriptor {
            depth_stencil: Some(bevy::render::render_resource::DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            fragment: Some(FragmentState {
                targets: vec![Some(ColorTargetState {
                    format: FORMAT,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                ..Default::default()
            }),
            ..Default::default()
        };
        clear_states(&mut d);
        let depth = d.depth_stencil.as_ref().unwrap();
        assert_eq!(
            (depth.depth_write_enabled, depth.depth_compare),
            (Some(false), Some(CompareFunction::Always))
        );
        let target = d.fragment.as_ref().unwrap().targets[0].as_ref().unwrap();
        assert_eq!(
            (target.blend, target.write_mask),
            (None, ColorWrites::ALPHA)
        );
        assert_eq!(d.primitive.cull_mode, None);
        const { assert!(CLEAR_BIAS > 1e5 && LEVEL_GLOW_BIAS > CLEAR_BIAS + 1e5) };
        // Its triangle covers clip space.
        let mesh = clear_mesh();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions")
        };
        assert_eq!(p.len(), 3);
        for corner in [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
            // Inside x >= -1, y >= -1, x + y <= 2.
            assert!(corner[0] >= p[0][0] && corner[1] >= p[0][1]);
            assert!(corner[0] + corner[1] <= p[1][0] + p[1][1]);
        }
    }

    #[test]
    fn the_add_back_adds_colour_and_keeps_alpha() {
        let c = ADD_COLOUR.color;
        assert_eq!(
            (c.src_factor, c.dst_factor, c.operation),
            (BlendFactor::One, BlendFactor::One, BlendOperation::Add)
        );
        let a = ADD_COLOUR.alpha;
        assert_eq!(
            (a.src_factor, a.dst_factor),
            (BlendFactor::Zero, BlendFactor::One)
        );
    }
}
