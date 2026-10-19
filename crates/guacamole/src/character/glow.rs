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

use bevy::app::{App, Plugin};
use bevy::asset::{AssetServer, Assets, Handle, RenderAssetUsages, embedded_asset};
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
use bevy::mesh::Mesh3d;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::{ChildOf, Transform};
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent,
    BlendFactor, BlendOperation, BlendState, CachedRenderPipelineId, ColorTargetState, ColorWrites,
    FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, ShaderStages, ShaderType, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages, UniformBuffer,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::sync_component::SyncComponent;
use bevy::render::texture::{CachedTexture, GpuImage, TextureCache};
use bevy::render::view::{Msaa, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;
use guac_ir::model::Model;

use super::material::CharacterMaterial;
use super::mesh::SpawnedCharacter;

/// The shader, embedded by [`GlowPlugin`].
pub const SHADER: &str = "embedded://guacamole/character/glow.wesl";

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
        app.add_plugins(ExtractComponentPlugin::<Glow>::default());
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
}

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

fn prepare_textures(
    mut commands: Commands,
    mut cache: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    views: Query<(Entity, &ExtractedCamera), With<Glow>>,
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
    view: ViewQuery<(&ExtractedCamera, &ViewTarget, &Glow, &GlowTextures)>,
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
    let get = |id| pipeline_cache.get_render_pipeline(id);
    let (Some(extract), Some(across), Some(down), Some(add_back)) = (
        get(pipelines.extract),
        get(pipelines.across),
        get(pipelines.down),
        get(pipelines.add_back),
    ) else {
        return;
    };
    let (Some(mask), Some(uniform)) = (images.get(&glow.mask), textures.uniform.binding()) else {
        return;
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
        &mask.texture_view,
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

/// The glow camera for `camera`: a child that sees only `layer`, draws the
/// glow masks there into a new image `size` big (zero where nothing glows)
/// before the frame's camera draws, with its projection and sample count.
/// Puts [`Glow`] on `camera` and returns the mask image.
pub fn spawn_glow_camera(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    camera: Entity,
    size: UVec2,
    projection: Projection,
    msaa: Msaa,
    layer: usize,
) -> Handle<Image> {
    let mut image = Image::new_target_texture(size.x, size.y, FORMAT, None);
    image.asset_usage = RenderAssetUsages::default();
    let mask = images.add(image);
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
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
/// pass (`masks`, by the model's material index), skinned by the same
/// joints.
pub fn spawn_glow_masks(
    commands: &mut Commands,
    model: &Model,
    spawned: &SpawnedCharacter,
    masks: &[Handle<CharacterMaterial>],
    layer: usize,
) -> Vec<Entity> {
    model
        .segments
        .iter()
        .zip(&spawned.meshes)
        .map(|(s, mesh)| {
            commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(masks[usize::from(s.material)].clone()),
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
