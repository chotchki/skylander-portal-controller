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

/// AT.3's frame: `_Screen_Container` 853.333 by 480 at 150 % on the frame,
/// the UI container 768 by 432 centred in it, so a unit is 1.5 px from
/// (64, 36).
#[test]
fn the_screen_and_ui_containers_map_a_unit_to_one_and_a_half_pixels() {
    let mut m = Model::default();
    let screen = m.add_template(
        None,
        &template(kind::CONTAINER, 853.333_3, 480.0),
        Value::Null,
    );
    let ui = m.add_template(None, &template(kind::CONTAINER, 768.0, 432.0), Value::Null);
    let art = m.add_template(None, &template(kind::ART, 237.0, 96.0), Value::Null);
    m.wake(screen);
    m.set_scale(screen, 0, 150.0);
    m.set_scale(screen, 1, 150.0);
    m.wake(ui);
    m.set(ui, si("alignment"), Value::Int(4)).unwrap();
    m.set(ui, si("align to parent"), Value::Int(4)).unwrap();
    m.attach(ui, Some(screen));
    let a = m.spawn(art);
    m.attach(a, Some(ui));
    // The HUD's box: bottom-left on bottom-left, 20 up.
    m.set(a, si("alignment"), Value::Int(6)).unwrap();
    m.set(a, si("align to parent"), Value::Int(6)).unwrap();
    at(&mut m, a, 0.0, -20.0);
    let f = m.lay_out(&NoWorld);
    let r = placed(&f, ui).rect();
    assert!(
        (r[0] - 64.0).abs() < 1e-3 && (r[1] - 36.0).abs() < 1e-3,
        "{r:?}"
    );
    let b = placed(&f, a).rect();
    let want = [
        64.0,
        36.0 + (432.0 - 20.0 - 96.0) * 1.5,
        64.0 + 237.0 * 1.5,
        36.0 + (432.0 - 20.0) * 1.5,
    ];
    for (got, want) in b.iter().zip(want) {
        assert!((got - want).abs() < 1e-3, "{b:?} against {want:?}");
    }
    // Parentless, a sprite stands in frame pixels at depth 10,000.
    assert_eq!(placed(&f, screen).point, [0.0, 10_000.0, 0.0]);
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
    assert_eq!(p.point[1], 10_000.0 + 25.0 * 1.0);
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
    m.set(parent, si("visible"), Value::Int(0)).unwrap();
    let f = m.lay_out(&NoWorld);
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
    m.set_scale(a, 1, 150.0);
    at(&mut m, a, 10.0, 20.0);
    let f = m.lay_out(&NoWorld);
    let list = m.draw_list(&f);
    assert_eq!(list.len(), 2);
    // 40 font pixels at 41/82 and 150 %: 30 px; parentless, 10 and 20 px.
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
    assert_eq!(*rect, [40.0, 20.0, 70.0, 20.0 + 82.0 * 0.5 * 1.5]);
    assert_eq!(*uv, [0.1, 0.2, 0.3, 0.4]);
    assert_eq!(*letter, LETTER);
    assert_eq!(atlas, "textures/font.ktx2");
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
