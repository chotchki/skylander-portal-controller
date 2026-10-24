//! The turntables' made-up extract (two good models, a damaged one, one
//! that didn't extract and an item) and their headless app, from
//! turntable's tests; the level's, play's, warm's, prepare's, the TV's and
//! the scripts' tests stand on them too.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use guac_extract::intros::IrFormat;
use guac_extract::stages::ModelFailure;
use guac_formats::texture::{Format, Texture};
use guac_ir::clip::{Clip, RotationTrack, Track, VecTrack};
use guac_ir::model::{
    self, Blend, Filter, Glow, Mask, Params, RenderStates, Sampler, Segment, TextureRole,
    TextureSlot, Wrap,
};
use guac_ir::{
    Category, DumpIdentity, Element, FigureKind, Language, Model, Rig, RigLight, Roster,
    RosterFigure, Sha256, Text,
};
use guacamole::character::CharacterMaterial;
use skylander_core::{FigureId, SlotIndex};
use skylander_rpcs3_control::Placement;

use crate::cards::PortalCards;
use crate::driver::PortalChange;
use crate::motion::ReducedMotion;
use crate::turntable::*;

pub(crate) fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([7; 32]),
    }
}

/// A 4x4 BC1 KTX2, every texel one colour: built here, none of the disc's.
pub(crate) fn ktx2() -> Vec<u8> {
    Texture {
        format: Format::Bc1RgbaUnorm,
        width: 4,
        height: 4,
        levels: vec![vec![0x1F, 0xF8, 0x1F, 0xF8, 0, 0, 0, 0]],
    }
    .to_ktx2()
}

/// A made-up model on `load`: a quad on two joints, a colour map alone,
/// a two-key idle swaying the top joint. `002_two`'s also draws a
/// blended, two-sided, alpha-tested material with normal and spec maps
/// on a quad with vertex colours, and its first material on one too, so
/// the two models draw three keys between them (AE.14's warm list).
pub(crate) fn model(load: &str) -> Model {
    let joint = |name: &str, parent, y: f32| model::Joint {
        name: name.into(),
        parent,
        reference: model::Transform {
            translation: [0.0, y, 0.0],
            ..model::Transform::IDENTITY
        },
        inverse_bind: Mat4::from_translation(Vec3::new(0.0, -y, 0.0)).to_cols_array_2d(),
    };
    let sampler = Sampler {
        wrap_u: Wrap::Repeat,
        wrap_v: Wrap::Repeat,
        mag: Filter::Linear,
        min: Filter::Linear,
        mip: None,
    };
    let material = model::Material {
        name: "made_up".into(),
        pixel: Mask {
            base: 0,
            ignore: 0x2FF,
        },
        vertex: Mask {
            base: 0,
            ignore: 0x1AFF,
        },
        textures: vec![TextureSlot {
            role: TextureRole::Layer1ColorMap,
            texture: "textures/made_up.ktx2".into(),
            image: "made_up".into(),
            sampler,
        }],
        params: Params {
            fixed_alpha: [-1.0; 4],
            layer1_extra: [1.0, 1.0, 1.0, 0.25],
            layer2_extra: None,
            layer2_blend: None,
            layer3_blend: None,
            layer4_blend: [0.0, 1.0, 0.0, 0.0],
            layer4_values: [0.0, 1.0, 0.0, 0.0],
            material_tint: [128, 128, 128, 255],
            glow: Glow::Amount(64),
        },
        states: RenderStates {
            blend: Blend::Opaque,
            depth_write: true,
            two_sided: false,
            alpha_test: None,
        },
        scrolls: Vec::new(),
        tracks: Vec::new(),
    };
    let carried_material = material.clone();
    let top = Track {
        joint: 1,
        translation: VecTrack {
            constant: [0.0, 2.0, 0.0],
            spline: Some(guac_ir::clip::VecSpline {
                channels: [true, false, false],
                degree: 1,
                knots: vec![0, 0, 30, 30],
                points: vec![[0.0; 3], [0.5, 0.0, 0.0]],
            }),
        },
        rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
        scale: VecTrack {
            constant: [1.0; 3],
            spline: None,
        },
    };
    let quad = |material: u16, colours: bool| Segment {
        material,
        source: 1,
        positions: vec![
            [-0.5, 0.0, 0.0],
            [0.5, 0.0, 0.0],
            [-0.5, 2.0, 0.0],
            [0.5, 2.0, 0.0],
        ],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        tangents: Some(vec![[1.0, 0.0, 0.0, 1.0]; 4]),
        uvs: vec![vec![[0.0, 1.0], [1.0, 1.0], [0.0, 0.0], [1.0, 0.0]]],
        colors: colours.then(|| vec![[200, 128, 64, 255]; 4]),
        joints: vec![[0, 0, 0, 0], [0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]],
        weights: vec![[255, 0, 0, 0]; 4],
        indices: vec![0, 1, 2, 2, 1, 3],
    };
    let (materials, segments) = if load == "002_two" {
        let mut fins = material.clone();
        fins.name = "made_up_fins".into();
        fins.pixel.base = 0x18;
        fins.vertex.base = 0x18;
        for role in [
            TextureRole::Layer1BaseNormalMap,
            TextureRole::Layer1BaseSpecMap,
        ] {
            fins.textures.push(TextureSlot {
                role,
                ..fins.textures[0].clone()
            });
        }
        fins.states = RenderStates {
            blend: Blend::Alpha,
            depth_write: false,
            two_sided: true,
            alpha_test: Some(0.5),
        };
        fins.params.glow = Glow::None;
        (
            vec![material, fins],
            vec![quad(0, false), quad(1, true), quad(0, true)],
        )
    } else {
        (vec![material], vec![quad(0, false)])
    };
    // Two's top carries a made-up knife a quarter out and turned, and
    // the knife a jewel on its root (AE.23).
    let attachments = if load == "002_two" {
        let mut flat = quad(0, false);
        flat.joints.clear();
        flat.weights.clear();
        let prop = |path: &str| guac_ir::level::Prop {
            path: path.into(),
            skeleton: None,
            materials: vec![guac_ir::level::LevelMaterial::Multi2(
                carried_material.clone(),
            )],
            segments: vec![flat.clone()],
            idle: None,
        };
        vec![
            model::Attachment {
                placement: "002_Knife".into(),
                parent: None,
                bone: Some("top".into()),
                joint: Some(1),
                offset: model::Transform {
                    translation: [0.25, 0.0, 0.0],
                    rotation: Quat::from_rotation_y(0.5).to_array(),
                    scale: [1.0; 3],
                },
                model: prop("Knife.mdl"),
            },
            model::Attachment {
                placement: "002_Jewel".into(),
                parent: Some(0),
                bone: Some("*root*".into()),
                joint: None,
                offset: model::Transform {
                    translation: [0.0, 0.0, 0.5],
                    ..model::Transform::IDENTITY
                },
                model: prop("Jewel.mdl"),
            },
        ]
    } else {
        Vec::new()
    };
    Model {
        identity: identity(),
        load_file: load.into(),
        name: "MadeUp.mdl".into(),
        skeleton: model::Skeleton {
            joints: vec![joint("foot", None, 0.0), joint("top", Some(0), 2.0)],
        },
        materials,
        segments,
        idle: Clip {
            name: "MadeUp_Idle.ma".into(),
            duration: 1.0,
            frame_duration: 1.0 / 30.0,
            frames: 31,
            tracks: vec![top],
        },
        attachments,
        attachments_unsettled: None,
    }
}

/// Made-up lights at the sprite rig's angles.
pub(crate) fn rig() -> Rig {
    let light = |name: &str, rotation| RigLight {
        name: name.into(),
        diffuse: [90, 90, 90, 255],
        ambient: [20, 20, 20, 255],
        specular: [60, 60, 60, 255],
        multiplier: 1.0,
        position: [0.0; 3],
        rotation,
    };
    Rig {
        identity: identity(),
        lights: vec![
            light("_spriteLight1", [-45.0, 0.0, 0.0]),
            light("_spriteLight2", [45.0, 180.0, 0.0]),
            light("_spriteLight3", [-45.0, 90.0, 0.0]),
        ],
    }
}

pub(crate) fn figure(toy: u32, name: &str, category: Category, load: &str) -> RosterFigure {
    let text = |key: &str, en: &str| Text {
        key: key.into(),
        texts: vec![en.into()],
    };
    RosterFigure {
        toy_type: toy,
        variant: 0,
        kind: FigureKind::Base,
        deco: None,
        name: text("name", name),
        variant_word: None,
        element: Some(Element::Fire),
        category,
        load_file: load.into(),
        portrait: None,
        sky1: false,
    }
}

/// An extract holding two good models (toys 1 and 2), one damaged (3),
/// one that didn't extract (4) and an item (5); its shelf.
pub(crate) fn extract(dir: &Path) -> Shelf {
    let ir = IrFormat::Ron;
    std::fs::create_dir_all(dir.join("models")).unwrap();
    std::fs::create_dir_all(dir.join("textures")).unwrap();
    std::fs::create_dir_all(dir.join("ir")).unwrap();
    for load in ["001_one", "002_two"] {
        let ron = guac_ir::to_ron(&model(load)).unwrap();
        std::fs::write(dir.join(ir.model_path(load)), ron).unwrap();
    }
    std::fs::write(dir.join(ir.model_path("003_three")), "not a model").unwrap();
    std::fs::write(dir.join("textures/made_up.ktx2"), ktx2()).unwrap();
    std::fs::write(dir.join(ir.rig_path()), guac_ir::to_ron(&rig()).unwrap()).unwrap();
    let roster = Roster {
        identity: identity(),
        languages: vec![Language {
            id: guac_ir::roster::ENGLISH,
            name: "ENGLISH".into(),
        }],
        figures: vec![
            figure(1, "One", Category::Figure, "001_one"),
            figure(2, "Two", Category::Giant, "002_two"),
            figure(3, "Three", Category::Figure, "003_three"),
            figure(4, "Four", Category::Figure, "004_four"),
            figure(5, "Five", Category::Item, "005_five"),
        ],
    };
    let failed = [ModelFailure {
        load_file: "004_four".into(),
        reason: "GUAC-E-MODEL-UNREADABLE: made up".into(),
    }];
    Shelf::new(&roster, dir, ir, &failed)
}

pub(crate) fn placed(slot: u8, toy: u32, name: &str) -> PortalChange {
    let figure_id = FigureId::new(format!("{toy:06x}-0000"));
    let (toy_type, variant) = figure_id.subtype_parts().unwrap();
    PortalChange::Placed {
        slot: SlotIndex::new(slot).unwrap(),
        placement: Placement {
            figure_id,
            toy_type,
            variant,
            name: name.into(),
            element: Some(skylander_core::Element::Fire),
            portrait: None,
            owner: None,
        },
    }
}

/// The assets the turntable spawns into, on an app with `Time`.
pub(crate) fn add_turntable(app: &mut App) {
    app.init_asset::<Mesh>()
        .init_asset::<SkinnedMeshInverseBindposes>()
        .init_asset::<CharacterMaterial>()
        .init_resource::<ReducedMotion>()
        .add_plugins(TurntablePlugin);
}

pub(crate) fn app(shelf: Shelf) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::image::ImagePlugin::default(),
        bevy::transform::TransformPlugin,
    ));
    add_turntable(&mut app);
    let (tx, feed) = shelf_channel();
    tx.send(Arc::new(shelf)).unwrap();
    app.init_resource::<PortalCards>().insert_resource(feed);
    app
}

pub(crate) fn set(app: &mut App, changes: impl IntoIterator<Item = PortalChange>) {
    let cards = app
        .world()
        .resource::<PortalCards>()
        .clone()
        .reduce(changes);
    app.insert_resource(cards);
}

/// Frames until no slot is loading, up to 5 s.
pub(crate) fn settle(app: &mut App) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        let t = app.world().resource::<Turntables>();
        let loading = (0..8).any(|s| matches!(t.card_state(s as u8), Some(CardState::Loading)));
        if !loading {
            // One more, so what the last poll spawned is in the world.
            app.update();
            return;
        }
        assert!(Instant::now() < until, "still loading");
    }
}

pub(crate) fn states(app: &App) -> Vec<Option<&'static str>> {
    let t = app.world().resource::<Turntables>();
    (0..8)
        .map(|s| t.card_state(s).map(CardState::name))
        .collect()
}
