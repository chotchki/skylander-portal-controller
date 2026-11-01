//! The layout and the draw list on sprites built in code (AT.7).

use guac_ir::interface::{Art, Content, Template};
use guac_script::Value;
use proptest::prelude::*;

use super::*;
use crate::layout::Placed;
use crate::model::NoWorld;
use crate::sprite::{flags as sflags, kind, props};

fn template(visual: u32, w: f32, h: f32) -> Template {
    Template {
        name: "T".into(),
        object: 1,
        properties: props::VISIBLE | (1 << 3) | visual,
        flags: sflags::SIZED as i32,
        position: [0, 0],
        z_depth: 0.0,
        priority: 0.0,
        width: w,
        height: h,
        scale: [100.0; 3],
        color: [0x80, 0x80, 0x80, 0xFF],
        replace_color: [0x80, 0x80, 0x80, 0],
        num_value: 0,
        depth: 0.0,
        init_matrix: Default::default(),
        matrix: Default::default(),
        content: if visual == kind::ART {
            Content::Image(Art {
                source: Some("a.png".into()),
                texture: Some("textures/a.ktx2".into()),
                size: Some([16, 16]),
                stream_archive_hash: 0,
                desired: [16, 16],
            })
        } else {
            Content::Container
        },
        string: None,
    }
}

fn si(name: &str) -> &'static crate::Member {
    crate::find("spriteinfo", name).unwrap()
}

/// A model with one container and one art template, neither woken.
fn model() -> (Model, SpriteId, SpriteId) {
    let mut m = Model::default();
    let c = m.add_template(None, &template(kind::CONTAINER, 100.0, 60.0), Value::Null);
    let a = m.add_template(None, &template(kind::ART, 20.0, 10.0), Value::Null);
    (m, c, a)
}

fn at(m: &mut Model, id: SpriteId, x: f32, y: f32) {
    let half = |v: f32| u32::from((v * 16.0) as i16 as u16);
    m.set(id, si("location"), Value::Screen(half(x) << 16 | half(y)))
        .unwrap();
}

fn placed(f: &crate::layout::Frame, id: SpriteId) -> &Placed {
    f.get(id).unwrap()
}

#[test]
fn every_anchor_pair_hangs_its_point_on_its_parents() {
    let (mut m, ct, at_) = model();
    let parent = m.spawn(ct);
    at(&mut m, parent, 40.0, 30.0);
    for own in 0..9u32 {
        for on in 0..9u32 {
            let child = m.spawn(at_);
            m.set(child, si("alignment"), Value::Int(own as i32))
                .unwrap();
            m.set(child, si("align to parent"), Value::Int(on as i32))
                .unwrap();
            m.attach(child, Some(parent));
            let f = m.lay_out(&NoWorld);
            let r = placed(&f, child).rect();
            let x = 40.0 + (on % 3) as f32 * 50.0 - (own % 3) as f32 * 10.0;
            let y = 30.0 + (on / 3) as f32 * 30.0 - (own / 3) as f32 * 5.0;
            assert_eq!(r, [x, y, x + 20.0, y + 10.0], "{own} on {on}");
            m.remove(child);
        }
    }
}

#[test]
fn a_parents_move_and_scale_carry_its_children() {
    let (mut m, ct, art) = model();
    let parent = m.spawn(ct);
    let child = m.spawn(art);
    at(&mut m, child, 3.0, 4.0);
    m.attach(child, Some(parent));
    let before = placed(&m.lay_out(&NoWorld), child).rect();
    at(&mut m, parent, 10.0, -2.0);
    let after = placed(&m.lay_out(&NoWorld), child).rect();
    assert_eq!(
        after,
        [
            before[0] + 10.0,
            before[1] - 2.0,
            before[2] + 10.0,
            before[3] - 2.0
        ]
    );
    // At the parent's 150 % the child's place and size are 1.5 times.
    m.set_scale(parent, 0, 150.0);
    m.set_scale(parent, 1, 150.0);
    let f = m.lay_out(&NoWorld);
    assert_eq!(
        placed(&f, child).rect(),
        [10.0 + 4.5, -2.0 + 6.0, 10.0 + 4.5 + 30.0, -2.0 + 6.0 + 15.0]
    );
}

/// The June savestate's live tree (AT.3's read of it): each sprite built
/// with its live size, scale, location and anchors (alignment, align to
/// parent), each laid out where the savestate's matrix has it. The screen
/// container at 150 % puts a unit at 1.5 px, the UI and gameplay containers
/// at (64, 36); a sprite with no parent hangs on the 1280 by 720 root by its
/// align to parent (the corner shades and the flash squares).
#[test]
fn the_june_savestates_sprites_stand_where_its_matrices_have_them() {
    let mut m = Model::default();
    let container = |m: &mut Model, w: f32, h: f32| {
        m.add_template(None, &template(kind::CONTAINER, w, h), Value::Null)
    };
    let art = |m: &mut Model, w: f32, h: f32| {
        m.add_template(None, &template(kind::ART, w, h), Value::Null)
    };
    let hang = |m: &mut Model, id: SpriteId, own: i32, on: i32, parent: Option<SpriteId>| {
        m.set(id, si("alignment"), Value::Int(own)).unwrap();
        m.set(id, si("align to parent"), Value::Int(on)).unwrap();
        m.attach(id, parent);
    };
    let screen = container(&mut m, 853.333_3, 480.0);
    m.wake(screen);
    m.set_scale(screen, 0, 150.0);
    m.set_scale(screen, 1, 150.0);
    hang(&mut m, screen, 4, 4, None);
    let ui = container(&mut m, 768.0, 432.0);
    m.wake(ui);
    hang(&mut m, ui, 4, 4, Some(screen));
    let gameplay = container(&mut m, 768.0, 432.0);
    m.wake(gameplay);
    hang(&mut m, gameplay, 4, 4, Some(ui));
    // `PortalUI_Container`: unsized, 0 by 0, at (0, 65).
    let mut t = template(kind::CONTAINER, 0.0, 0.0);
    t.flags = 0;
    let portal = m.add_template(None, &t, Value::Null);
    m.wake(portal);
    at(&mut m, portal, 0.0, 65.0);
    hang(&mut m, portal, 4, 4, Some(gameplay));
    let inner = container(&mut m, 768.0, 432.0);
    m.wake(inner);
    hang(&mut m, inner, 4, 4, Some(gameplay));
    // The HUD's main box: 237 by 96, bottom-left on bottom-left.
    let hud = container(&mut m, 237.0, 96.0);
    m.wake(hud);
    hang(&mut m, hud, 6, 6, Some(inner));
    // `GameplayOverlay_BL`, `_BR`, `_UL`, `_UR` and the four
    // `Generic_GraySquare`s, parentless.
    let corner = art(&mut m, 200.0, 200.0);
    let square = art(&mut m, 600.0, 600.0);
    let mut loose = Vec::new();
    for (t, own, on, want) in [
        (corner, 0, 6, [0.0, 720.0]),
        (corner, 0, 8, [1280.0, 720.0]),
        (corner, 0, 0, [0.0, 0.0]),
        (corner, 0, 2, [1280.0, 0.0]),
        (square, 7, 1, [340.0, -600.0]),
        (square, 3, 5, [1280.0, 60.0]),
        (square, 1, 7, [340.0, 720.0]),
        (square, 5, 3, [-600.0, 60.0]),
    ] {
        let a = m.spawn(t);
        hang(&mut m, a, own, on, None);
        loose.push((a, want));
    }
    let f = m.lay_out(&NoWorld);
    let near = |id: SpriteId, want: [f32; 2]| {
        let [x, depth, y] = placed(&f, id).point;
        assert!(
            (x - want[0]).abs() < 1e-3 && (y - want[1]).abs() < 1e-3,
            "{id:?} at ({x}, {y}), the savestate's ({}, {})",
            want[0],
            want[1]
        );
        assert_eq!(depth, 10_000.0);
    };
    near(screen, [0.0, 0.0]);
    near(ui, [64.0, 36.0]);
    near(gameplay, [64.0, 36.0]);
    near(portal, [640.0, 457.5]);
    near(inner, [64.0, 36.0]);
    near(hud, [64.0, 540.0]);
    for (a, want) in loose {
        near(a, want);
    }
    // 1.5 px a unit: the box is 355.5 by 144.
    let r = placed(&f, hud).rect();
    assert!((r[2] - r[0] - 355.5).abs() < 1e-3 && (r[3] - r[1] - 144.0).abs() < 1e-3);
}

#[test]
fn priorities_and_depths_sum_down_the_tree_and_alpha_multiplies() {
    let (mut m, ct, art) = model();
    let parent = m.spawn(ct);
    let child = m.spawn(art);
    m.attach(child, Some(parent));
    m.set(parent, si("priority"), Value::Float(-10.0)).unwrap();
    m.set(child, si("priority"), Value::Float(5.0)).unwrap();
    m.set(parent, si("Z Depth"), Value::Float(2.0)).unwrap();
    m.set(child, si("Z Depth"), Value::Float(1.0)).unwrap();
    m.set(parent, si("color"), Value::Color(0x8080_8080))
        .unwrap();
    m.set(child, si("color"), Value::Color(0x8080_8080))
        .unwrap();
    let f = m.lay_out(&NoWorld);
    let p = placed(&f, child);
    assert_eq!((p.priority, p.depth), (-5.0, 3.0));
    // The root's 10,000, the parent's 2 × 25 and the child's 1 × 25.
    assert_eq!(placed(&f, parent).point[1], 10_000.0 + 25.0 * 2.0);
    assert_eq!(p.point[1], 10_000.0 + 25.0 * 2.0 + 25.0 * 1.0);
    assert_eq!(
        p.color,
        [0x80, 0x80, 0x80, (128.0f32 * (128.0 / 255.0)) as u8]
    );
}

#[test]
fn a_hidden_parent_hides_its_children_and_dirty_runs_down() {
    let (mut m, ct, art) = model();
    let parent = m.spawn(ct);
    let child = m.spawn(art);
    m.attach(child, Some(parent));
    let f = m.lay_out(&NoWorld);
    assert!(placed(&f, child).drawn);
    assert_eq!(
        m.sprite(child).properties & props::DIRTY,
        0,
        "drawn clears it"
    );
    assert_eq!(m.draw_list(&f).len(), 1, "the container draws nothing");
    assert_eq!(m.drawing(), &[child], "nor joins the frame's sprite list");
    m.set(parent, si("visible"), Value::Int(0)).unwrap();
    m.sprite_mut(child).properties |= props::DIRTY;
    let f = m.lay_out(&NoWorld);
    assert_eq!(
        m.sprite(child).properties & props::DIRTY,
        0,
        "not drawn clears it"
    );
    assert!(f.get(child).is_none_or(|p| !p.drawn));
    assert!(m.draw_list(&f).is_empty());
    assert!(m.drawing().is_empty());
    // A parent dirty makes its children dirty this frame.
    m.set(parent, si("visible"), Value::Int(1)).unwrap();
    let f = m.lay_out(&NoWorld);
    assert!(placed(&f, child).dirty);
    let f = m.lay_out(&NoWorld);
    assert!(f.get(child).is_some_and(|p| !p.dirty));
}

#[test]
fn a_child_before_its_parent_is_walked_after_it() {
    let (mut m, ct, art) = model();
    let child = m.spawn(art);
    let parent = m.spawn(ct);
    m.attach(child, Some(parent));
    let f = m.lay_out(&NoWorld);
    assert_eq!(f.walk, vec![parent, child]);
    assert_eq!(m.active(), &[parent, child], "the list drifts that way");
}

#[test]
fn an_unsized_sprite_stores_its_computed_size() {
    let (mut m, _, art) = model();
    let a = m.spawn(art);
    m.sprite_mut(a).flags &= !sflags::SIZED;
    let f = m.lay_out(&NoWorld);
    assert_eq!(placed(&f, a).size, [16.0, 16.0], "its image's");
    assert_eq!(m.sprite(a).size[..2], [16.0, 16.0]);
}

#[test]
fn equal_priorities_draw_in_the_order_they_started_drawing() {
    let (mut m, _, art) = model();
    let [a, b, c] = [m.spawn(art), m.spawn(art), m.spawn(art)];
    let f = m.lay_out(&NoWorld);
    let order = |m: &Model, f| m.draw_list(f).iter().map(Entry::sprite).collect::<Vec<_>>();
    assert_eq!(order(&m, &f), vec![a, b, c]);
    // a hidden a frame comes back last among its equals.
    m.set(a, si("visible"), Value::Int(0)).unwrap();
    m.lay_out(&NoWorld);
    m.set(a, si("visible"), Value::Int(1)).unwrap();
    let f = m.lay_out(&NoWorld);
    assert_eq!(order(&m, &f), vec![b, c, a]);
    // Higher priority draws first, behind.
    m.set(c, si("priority"), Value::Float(10.0)).unwrap();
    m.set(b, si("priority"), Value::Float(-2.0)).unwrap();
    let f = m.lay_out(&NoWorld);
    assert_eq!(order(&m, &f), vec![c, a, b]);
}

#[test]
fn art_draws_its_texture_over_its_rectangle_with_its_paint() {
    let (mut m, _, art) = model();
    let a = m.spawn(art);
    at(&mut m, a, 5.0, 6.0);
    m.set(a, si("blend mode"), Value::Int(4)).unwrap();
    m.set(a, si("texture flip"), Value::Int(1)).unwrap();
    m.set(a, si("replace color percentage"), Value::Int(100))
        .unwrap();
    let f = m.lay_out(&NoWorld);
    let list = m.draw_list(&f);
    assert_eq!(
        list,
        vec![Entry::Art {
            sprite: a,
            texture: Some("textures/a.ktx2".into()),
            rect: [5.0, 6.0, 25.0, 16.0],
            uv: [1.0, 0.0, 0.0, 1.0],
            paint: Paint {
                color: [0x80, 0x80, 0x80, 0xFF],
                replace: [0x80, 0x80, 0x80, 0xFF],
                blend: blend::GETS_CUT_OUT,
                invert: false,
            },
        }]
    );
    assert_eq!(flipped([0.0, 0.0, 1.0, 1.0], 3), [1.0, 1.0, 0.0, 0.0]);
    assert_eq!(flipped([0.0, 0.0, 1.0, 1.0], 2), [0.0, 1.0, 1.0, 0.0]);
}

#[test]
fn models_emitters_and_movies_are_entries_over_their_rectangles() {
    let mut m = Model::default();
    let mut t = template(kind::MODEL, 8.0, 8.0);
    t.content = Content::Model(Some("models/x".into()));
    let model = m.add_template(None, &t, Value::Null);
    let mut t = template(kind::PARTICLES, 4.0, 4.0);
    t.content = Content::Emitter(Some("Burst".into()));
    let emitter = m.add_template(None, &t, Value::Null);
    let mut t = template(kind::ART, 6.0, 6.0);
    t.content = Content::Movie(Art {
        source: Some("intro.mov".into()),
        texture: None,
        size: None,
        stream_archive_hash: 0,
        desired: [-1, -1],
    });
    let movie = m.add_template(None, &t, Value::Null);
    let [a, b, c] = [m.spawn(model), m.spawn(emitter), m.spawn(movie)];
    let f = m.lay_out(&NoWorld);
    assert_eq!(
        m.draw_list(&f),
        vec![
            Entry::Model {
                sprite: a,
                path: Some("models/x".into()),
                rect: [0.0, 0.0, 8.0, 8.0]
            },
            Entry::Emitter {
                sprite: b,
                name: Some("Burst".into()),
                rect: [0.0, 0.0, 4.0, 4.0]
            },
            Entry::Movie {
                sprite: c,
                source: Some("intro.mov".into()),
                rect: [0.0, 0.0, 6.0, 6.0]
            },
        ]
    );
}

#[test]
fn a_text_draws_its_glyphs_at_its_scales_from_its_top_left() {
    use guac_ir::interface::{Font, Glyph};
    let glyph = |c: char| Glyph {
        char: c as u16,
        step: 40.0,
        offset: [0.0, 0.0],
        size: [40.0, 82.0],
        uv_min: [0.1, 0.2],
        uv_max: [0.3, 0.4],
        kern: [0; 4],
    };
    let mut m = Model::default();
    let font = m.add_font(Font {
        name: None,
        source: None,
        height: 82.0,
        base_line: 0.0,
        char_padding: 0.0,
        kern_amount: 0.0,
        color: [0.5, 0.5, 0.5, 1.0],
        atlas: "textures/font.ktx2".into(),
        atlas_size: [1024, 1024],
        glyphs: vec![glyph('a'), glyph('b')],
    });
    let s = m.add_file_string(
        guac_script::Obj { unit: 0, index: 3 },
        "ab",
        crate::Str::default(),
    );
    let mut t = template(kind::TEXT, 0.0, 0.0);
    t.flags = 0;
    let t = m.add_template(None, &t, s);
    m.set_font(t, Some(font));
    let a = m.spawn(t);
    m.set(a, si("point size"), Value::Int(41)).unwrap();
    m.set_scale(a, 0, 150.0);
    m.set_scale(a, 1, 200.0);
    at(&mut m, a, 10.0, 20.0);
    // The first letter coloured by the text colour converter.
    let tc = |n: &str| crate::find("textcolorconverter", n).unwrap();
    let av = m.value_of(a);
    m.set_global(tc("text sprite"), av, &NoWorld).unwrap();
    m.set_global_elements(tc("color set"), &[Value::Color(0xFF00_00FF)])
        .unwrap();
    let f = m.lay_out(&NoWorld);
    let list = m.draw_list(&f);
    assert_eq!(list.len(), 2);
    // 40 font pixels at 41/82, 150 % across and 200 % down: 30 by 82 px;
    // parentless, 10 and 20 px.
    let Entry::Glyph {
        rect,
        uv,
        letter,
        atlas,
        ..
    } = &list[1]
    else {
        panic!("{list:?}")
    };
    assert_eq!(*rect, [40.0, 20.0, 70.0, 20.0 + 82.0 * 0.5 * 2.0]);
    assert_eq!(*uv, [0.1, 0.2, 0.3, 0.4]);
    assert_eq!(*letter, LETTER);
    assert_eq!(atlas, "textures/font.ktx2");
    let Entry::Glyph { letter, .. } = &list[0] else {
        panic!("{list:?}")
    };
    assert_eq!(*letter, [0xFF, 0, 0, 0xFF]);
}

#[test]
fn art_takes_its_x_scale_across_and_its_y_scale_down() {
    let (mut m, _, art) = model();
    let a = m.spawn(art);
    m.set_scale(a, 0, 150.0);
    m.set_scale(a, 1, 50.0);
    let f = m.lay_out(&NoWorld);
    assert_eq!(placed(&f, a).rect(), [0.0, 0.0, 30.0, 5.0]);
}

#[test]
fn a_model_and_an_emitter_keep_their_anchors_off() {
    let mut m = Model::default();
    let mut t = template(kind::MODEL, 40.0, 20.0);
    t.content = Content::Model(None);
    let model = m.add_template(None, &t, Value::Null);
    let mut t = template(kind::PARTICLES, 40.0, 20.0);
    t.content = Content::Emitter(None);
    let emitter = m.add_template(None, &t, Value::Null);
    let art = m.add_template(None, &template(kind::ART, 10.0, 10.0), Value::Null);
    for source in [model, emitter] {
        // Its own alignment doesn't move it, and a child's align to parent
        // on it doesn't move the child.
        let a = m.spawn(source);
        at(&mut m, a, 100.0, 50.0);
        m.set(a, si("alignment"), Value::Int(8)).unwrap();
        let c = m.spawn(art);
        m.set(c, si("align to parent"), Value::Int(8)).unwrap();
        m.attach(c, Some(a));
        let f = m.lay_out(&NoWorld);
        assert_eq!(placed(&f, a).point, [100.0, 10_000.0, 50.0]);
        assert_eq!(placed(&f, c).rect(), [100.0, 50.0, 110.0, 60.0]);
        m.remove(c);
        m.remove(a);
    }
}

#[test]
fn a_removed_sprite_leaves_the_frames_sprite_list() {
    let (mut m, _, art) = model();
    let [a, b] = [m.spawn(art), m.spawn(art)];
    m.lay_out(&NoWorld);
    assert_eq!(m.drawing(), &[a, b]);
    m.remove(a);
    m.lay_out(&NoWorld);
    assert_eq!(m.drawing(), &[b]);
}

#[test]
fn colour_0x80_draws_the_art_as_stored() {
    let paint = Paint {
        color: [0x80, 0x80, 0x80, 0xFF],
        replace: [0x80, 0x80, 0x80, 0],
        blend: blend::CROSSFADE,
        invert: false,
    };
    for texel in [[0.2, 0.5, 0.9, 0.7], [1.0, 0.0, 0.25, 1.0]] {
        let out = shade(texel, &paint);
        for c in 0..3 {
            assert!(
                (out[c] - texel[c]).abs() <= texel[c] / 255.0 + 1e-6,
                "{out:?}"
            );
        }
        assert_eq!(out[3], texel[3]);
    }
    // The replace colour at full takes the rgb over; inverting flips alpha.
    let full = Paint {
        replace: [0xFF, 0, 0, 0xFF],
        invert: true,
        ..paint
    };
    assert_eq!(shade([0.2, 0.5, 0.9, 0.25], &full), [1.0, 0.0, 0.0, 0.75]);
    assert!(inverts(blend::CUTS_OUT_INVERT) && inverts(blend::CLEARS_CUT_OUT));
    assert!(!inverts(blend::CUTS_OUT) && !inverts(blend::CROSSFADE));
}

#[test]
fn each_blend_draws_its_equation() {
    let src = [0.8, 0.4, 0.2, 0.25];
    let dst = [0.1, 0.2, 0.3, 0.6];
    let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
    let cases = [
        (blend::OPAQUE, [0.8, 0.4, 0.2, 0.6]),
        (
            blend::CROSSFADE,
            [
                0.8 * 0.25 + 0.1 * 0.75,
                0.4 * 0.25 + 0.2 * 0.75,
                0.2 * 0.25 + 0.3 * 0.75,
                0.6,
            ],
        ),
        (blend::ADDITIVE, [0.1 + 0.2, 0.2 + 0.1, 0.3 + 0.05, 0.6]),
        (blend::CUTS_OUT, [0.1, 0.2, 0.3, 0.85]),
        (
            blend::GETS_CUT_OUT,
            [
                0.8 * 0.4 + 0.1 * 0.6,
                0.4 * 0.4 + 0.2 * 0.6,
                0.2 * 0.4 + 0.3 * 0.6,
                0.6,
            ],
        ),
        (blend::CLEARS_CUT_OUT, [0.1, 0.2, 0.3, 0.35]),
        (
            blend::GETS_CUT_OUT_ADD,
            [0.1 + 0.32, 0.2 + 0.16, 0.3 + 0.08, 0.6],
        ),
        (blend::CUTS_OUT_INVERT, [0.1, 0.2, 0.3, 0.85]),
    ];
    for (mode, want) in cases {
        assert!(
            near(blend(mode, src, dst), want),
            "{mode}: {:?}",
            blend(mode, src, dst)
        );
    }
    // The frame stores 0-1.
    assert_eq!(blend(blend::CUTS_OUT, src, [0.0, 0.0, 0.0, 0.9])[3], 1.0);
    assert_eq!(
        blend(blend::CLEARS_CUT_OUT, src, [0.0, 0.0, 0.0, 0.1])[3],
        0.0
    );
}

proptest! {
    /// Any tree hung by any attaches: every awake sprite is walked once,
    /// each after its parent.
    #[test]
    fn any_tree_is_walked_parents_first_once_each(
        hangs in prop::collection::vec((0usize..12, 0usize..12), 0..30)
    ) {
        let (mut m, ct, _) = model();
        let ids: Vec<SpriteId> = (0..12).map(|_| m.spawn(ct)).collect();
        for (a, b) in hangs {
            m.attach(ids[a], Some(ids[b]));
        }
        let f = m.lay_out(&NoWorld);
        prop_assert_eq!(f.walk.len(), 12);
        for (i, &id) in f.walk.iter().enumerate() {
            if let Some(p) = m.sprite(id).parent {
                let pi = f.walk.iter().position(|&w| w == p).unwrap();
                prop_assert!(pi < i);
            }
        }
    }
}
