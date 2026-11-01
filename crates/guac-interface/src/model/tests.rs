//! The model on templates built in code: the clone, the tree, removal and
//! every member's write, read, clamp and reset (AT.5).

use guac_ir::interface::{Art, Content, Template};
use guac_script::{Obj, Value};
use proptest::prelude::*;

use super::*;
use crate::members::{MEMBERS, find};
use crate::sprite::{flags, kind, props};

fn template(name: &str, visual: u32) -> Template {
    Template {
        name: name.into(),
        object: 1,
        // Visible, blend 1, the visual type.
        properties: props::VISIBLE | (1 << 3) | visual,
        flags: 0,
        position: [0x140, -0x140],
        z_depth: 0.5,
        priority: -2.0,
        width: 237.0,
        height: 96.0,
        scale: [100.0, 90.0, 100.0],
        color: [0x80, 0x80, 0x80, 0xFF],
        replace_color: [0x80, 0x80, 0x80, 0],
        num_value: 7,
        depth: 100.0,
        content: Content::Container,
        string: None,
    }
}

fn art(size: Option<[u32; 2]>, desired: [i32; 2]) -> Template {
    let mut t = template("Art", kind::ART);
    t.content = Content::Image(Art {
        source: Some("a.png".into()),
        texture: None,
        size,
        stream_archive_hash: 0,
        desired,
    });
    t
}

fn member(namespace: &str, name: &str) -> &'static Member {
    find(namespace, name).unwrap_or_else(|| panic!("{namespace}.{name}"))
}

fn si(name: &str) -> &'static Member {
    member("spriteinfo", name)
}

fn smm(name: &str) -> &'static Member {
    member("spritematrixmeasurement", name)
}

fn read(m: &Model, id: SpriteId, mem: &Member) -> Value {
    match m.get(id, mem, &NoWorld) {
        Ok(Answer::Value(v)) => v,
        other => panic!("{}: {other:?}", mem.path()),
    }
}

fn elements(m: &Model, id: SpriteId, mem: &Member) -> Vec<Value> {
    match m.get(id, mem, &NoWorld) {
        Ok(Answer::Elements(v)) => v,
        other => panic!("{}: {other:?}", mem.path()),
    }
}

/// A model with a container template woken and one clone of it.
fn one() -> (Model, SpriteId, SpriteId) {
    let mut m = Model::default();
    let t = m.add_template(None, &template("Box", kind::CONTAINER), Value::Null);
    let c = m.spawn(t);
    (m, t, c)
}

#[test]
fn a_clone_copies_every_field_awake_last_in_active_and_its_sources_clones() {
    let mut m = Model::default();
    let obj = Obj { unit: 0, index: 4 };
    let t = m.add_template(Some(obj), &template("Box", kind::CONTAINER), Value::Null);
    assert_eq!(
        m.sprite_of(Value::Object(obj)),
        Some(t),
        "a script's reference"
    );
    assert_eq!(m.sprite(t).state, State::Asleep);
    assert!(m.active().is_empty());
    m.set(t, si("priority"), Value::Float(5.0)).unwrap();
    let a = m.spawn(t);
    let b = m.spawn(a);
    let (st, sa) = (m.sprite(t), m.sprite(a));
    assert_eq!(sa.state, State::Awake);
    assert_eq!(
        (
            sa.location,
            sa.properties,
            sa.size,
            sa.scale,
            sa.color,
            sa.replace
        ),
        (
            st.location,
            st.properties,
            st.size,
            st.scale,
            st.color,
            st.replace
        )
    );
    assert_eq!(sa.priority, 5.0, "the source's fields as they stand");
    assert_eq!(m.active(), &[a, b]);
    assert_eq!(elements(&m, t, smm("clones")), vec![m.value_of(a)]);
    assert_eq!(elements(&m, a, smm("clones")), vec![m.value_of(b)]);
    assert_eq!(read(&m, b, smm("parent")), m.value_of(a));
    assert_eq!(read(&m, t, smm("parent")), Value::Null);
    m.wake(t);
    assert_eq!(m.active(), &[a, b, t]);
    // A clone of a sprite holding children holds none and hangs on nothing.
    m.attach(b, Some(a));
    m.attach(a, Some(t));
    let c = m.spawn(a);
    assert!(m.sprite(c).children.is_empty());
    assert_eq!(m.sprite(c).parent, None);
    assert_eq!(m.sprite(c).placement & placement::ATTACHED, 0);
}

#[test]
fn children_keep_the_order_they_were_hung_in() {
    let (mut m, t, p) = one();
    let [a, b, c] = [m.spawn(t), m.spawn(t), m.spawn(t)];
    let pv = m.value_of(p);
    for x in [a, b, c] {
        m.set(x, smm("attach node"), pv).unwrap();
    }
    let kids = |m: &Model| elements(m, p, smm("attached objects"));
    let bases = m.bases();
    let v = |x: SpriteId| Value::Entity(bases.sprites + u64::from(x.0));
    assert_eq!(kids(&m), vec![v(a), v(b), v(c)]);
    // Hung again on the parent it has: last.
    m.set(a, smm("attach node"), pv).unwrap();
    assert_eq!(kids(&m), vec![v(b), v(c), v(a)]);
    // Off it: the rest in order.
    m.set(c, smm("attach node"), Value::Null).unwrap();
    assert_eq!(kids(&m), vec![v(b), v(a)]);
    assert_eq!(read(&m, c, smm("attach node")), Value::Null);
    assert_eq!(read(&m, b, smm("attach node")), pv);
    assert_eq!(m.sprite(b).placement & placement::ATTACHED, 1);
    assert_eq!(m.sprite(c).placement & placement::ATTACHED, 0);
    // A removed child leaves the list.
    m.remove(b);
    assert_eq!(kids(&m), vec![v(a)]);
}

#[test]
fn an_attach_onto_itself_a_descendant_a_sleeper_or_nothing_leaves_it_parentless() {
    let (mut m, t, p) = one();
    let child = m.spawn(t);
    let grandchild = m.spawn(t);
    let gone = m.spawn(t);
    m.remove(gone);
    m.attach(grandchild, Some(child));
    for onto in [
        m.value_of(child),
        m.value_of(grandchild),
        m.value_of(t),
        m.value_of(gone),
        Value::Entity(7),
        Value::Null,
    ] {
        m.attach(child, Some(p));
        assert_eq!(m.sprite(child).parent, Some(p));
        m.set(child, smm("attach node"), onto).unwrap();
        assert_eq!(m.sprite(child).parent, None, "onto {onto:?}");
        assert!(m.sprite(p).children.is_empty(), "off its old parent");
    }
    // A number doesn't convert to a reference: the write drops.
    m.attach(child, Some(p));
    assert_eq!(
        m.set(child, smm("attach node"), Value::Int(3)),
        Err(Refusal::Nothing)
    );
    assert_eq!(m.sprite(child).parent, Some(p));
    // A sleeper can't hang anywhere either.
    m.attach(t, Some(p));
    assert_eq!(m.sprite(t).parent, None);
}

#[test]
fn removal_takes_it_off_its_parent_and_its_children_off_it() {
    let (mut m, t, root) = one();
    let mid = m.spawn(t);
    let [k1, k2] = [m.spawn(t), m.spawn(t)];
    m.attach(mid, Some(root));
    m.attach(k1, Some(mid));
    m.attach(k2, Some(mid));
    m.sprite_mut(mid).activator = m.value_of(root);
    m.set(mid, si("priority"), Value::Float(9.0)).unwrap();
    m.remove(mid);
    let s = m.sprite(mid);
    assert_eq!(s.state, State::Removed);
    // What it hung on and what hung on it are gone; its fields stay.
    assert_eq!(read(&m, mid, smm("attach node")), Value::Null);
    assert!(elements(&m, mid, smm("attached objects")).is_empty());
    assert_eq!(read(&m, mid, smm("activator")), Value::Null);
    assert_eq!(read(&m, mid, si("priority")), Value::Float(9.0));
    assert!(elements(&m, root, smm("attached objects")).is_empty());
    // Detached, not removed: an attach of nothing each (0x6A620).
    for k in [k1, k2] {
        assert_eq!(m.sprite(k).state, State::Awake);
        assert_eq!(m.sprite(k).parent, None);
    }
    assert!(!m.active().contains(&mid));
    assert_eq!(m.removed(), &[mid]);
    assert_eq!(elements(&m, root, si("removed")), vec![m.value_of(mid)]);
    assert!(!m.sprite(t).clones.contains(&mid));
    // Removing twice changes nothing; a removed sprite can't be hung.
    m.remove(mid);
    assert_eq!(m.removed(), &[mid]);
    m.attach(mid, Some(root));
    assert_eq!(m.sprite(mid).parent, None);
}

#[test]
fn visible_reads_on_off_and_loading_and_a_change_sets_dirty() {
    let mut m = Model::default();
    let t = m.add_template(None, &art(Some([4, 4]), [0, 0]), Value::Null);
    let a = m.spawn(t);
    let vis = si("visible");
    assert_eq!(read(&m, a, vis), Value::Int(1));
    struct Loading;
    impl World for Loading {
        fn loading(&self, _: &Model, _: SpriteId) -> bool {
            true
        }
    }
    assert_eq!(m.get(a, vis, &Loading), Ok(Answer::Value(Value::Int(2))));
    m.sprite_mut(a).properties &= !props::DIRTY;
    m.set(a, vis, Value::Int(2)).unwrap();
    assert_eq!(m.sprite(a).properties & props::DIRTY, 0, "no change");
    m.set(a, vis, Value::Int(0)).unwrap();
    assert_eq!(read(&m, a, vis), Value::Int(0));
    assert_ne!(m.sprite(a).properties & props::DIRTY, 0);
    assert_eq!(m.get(a, vis, &Loading), Ok(Answer::Value(Value::Int(0))));
    // A clamp: 5 converts to 2, on.
    m.set(a, vis, Value::Int(5)).unwrap();
    assert_eq!(read(&m, a, vis), Value::Int(1));
}

#[test]
fn colours_and_the_replace_colour_read_back_the_colour() {
    let (mut m, _, a) = one();
    m.set(a, si("color"), Value::Color(0x1122_3344)).unwrap();
    assert_eq!(read(&m, a, si("color")), Value::Color(0x1122_3344));
    m.sprite_mut(a).replace[3] = 0x40;
    m.set(a, si("replace color"), Value::Color(0xAABB_CCDD))
        .unwrap();
    assert_eq!(m.sprite(a).replace, [0xAA, 0xBB, 0xCC, 0x40]);
    // The getter points at the colour (0xB3358).
    assert_eq!(read(&m, a, si("replace color")), Value::Color(0x1122_3344));
    assert_eq!(
        m.set(a, si("color"), Value::Int(3)),
        Err(Refusal::Nothing),
        "a number isn't a colour"
    );
}

#[test]
fn the_replace_percentage_and_the_bloom_intensity_clamp_and_round() {
    let (mut m, _, a) = one();
    let pct = si("replace color percentage");
    m.set(a, pct, Value::Int(50)).unwrap();
    assert_eq!(m.sprite(a).replace[3], 128);
    assert_eq!(read(&m, a, pct), Value::Int(50));
    m.set(a, pct, Value::Int(150)).unwrap();
    assert_eq!(
        (m.sprite(a).replace[3], read(&m, a, pct)),
        (255, Value::Int(100))
    );
    m.set(a, pct, Value::Int(-4)).unwrap();
    assert_eq!(read(&m, a, pct), Value::Int(0));
    // The read rounds half up (0xB33AC): a byte of 2 is 0.78 %, 254 99.6 %.
    for (byte, percent) in [(2, 1), (254, 100), (1, 0)] {
        m.sprite_mut(a).replace[3] = byte;
        assert_eq!(read(&m, a, pct), Value::Int(percent), "byte {byte}");
    }
    let bloom = si("bloom intensity");
    m.sprite_mut(a).properties &= !props::DIRTY;
    m.set(a, bloom, Value::Int(250)).unwrap();
    assert_eq!(read(&m, a, bloom), Value::Int(100));
    assert_ne!(m.sprite(a).properties & props::DIRTY, 0, "every write");
    // Bits past 100 an old word held still read 100.
    m.sprite_mut(a).placement = placement::BLOOM.set(0, 120);
    assert_eq!(read(&m, a, bloom), Value::Int(100));
}

#[test]
fn location_holds_both_twelve_four_halves() {
    let (mut m, _, a) = one();
    assert_eq!(read(&m, a, si("location")), Value::Screen(0x0140_FEC0));
    m.set(a, si("location"), Value::Screen(0xFFF0_0010))
        .unwrap();
    assert_eq!(m.sprite(a).location, [-16, 16]);
    assert_eq!(read(&m, a, si("location")), Value::Screen(0xFFF0_0010));
}

#[test]
fn depths_store_floats_clamped_to_ten_thousand() {
    let (mut m, _, a) = one();
    for name in ["priority", "Z Depth"] {
        m.set(a, si(name), Value::Int(-3)).unwrap();
        assert_eq!(read(&m, a, si(name)), Value::Float(-3.0));
        m.set(a, si(name), Value::Float(25_000.0)).unwrap();
        assert_eq!(read(&m, a, si(name)), Value::Float(10_000.0));
        m.set(a, si(name), Value::Float(-25_000.0)).unwrap();
        assert_eq!(read(&m, a, si(name)), Value::Float(-10_000.0));
    }
}

#[test]
fn a_size_written_reads_back_and_one_unwritten_is_the_kinds() {
    let mut m = Model::default();
    let image = m.add_template(None, &art(Some([24, 16]), [8, 8]), Value::Null);
    let pending = m.add_template(None, &art(None, [8, 6]), Value::Null);
    let text = m.add_template(None, &template("Text", kind::TEXT), Value::Null);
    let [image, pending, text] = [m.spawn(image), m.spawn(pending), m.spawn(text)];
    assert_eq!(read(&m, image, si("width")), Value::Float(24.0));
    assert_eq!(read(&m, image, si("height")), Value::Float(16.0));
    assert_eq!(read(&m, pending, si("width")), Value::Float(8.0));
    // No image and a desired size that isn't one (a movie's -1, -1): the
    // stored size (the crate docs' Hypothesis on 0xB6950's fallback).
    let movie = m.add_template(None, &art(None, [-1, -1]), Value::Null);
    let movie = m.spawn(movie);
    assert_eq!(read(&m, movie, si("width")), Value::Float(237.0));
    assert_eq!(m.get(text, si("width"), &NoWorld), Err(Refusal::Unmodelled));
    let (_, _, boxed) = one();
    let _ = boxed;
    m.sprite_mut(image).properties &= !props::DIRTY;
    m.set(image, si("width"), Value::Float(30.0)).unwrap();
    assert!(m.sprite(image).sized());
    assert_ne!(m.sprite(image).properties & props::DIRTY, 0);
    assert_eq!(read(&m, image, si("width")), Value::Float(30.0));
    // Now sized, the height reads its stored number, not the image's.
    assert_eq!(read(&m, image, si("height")), Value::Float(96.0));
    // The same value again on a sized sprite changes nothing.
    m.sprite_mut(image).properties &= !props::DIRTY;
    m.set(image, si("width"), Value::Float(30.0)).unwrap();
    assert_eq!(m.sprite(image).properties & props::DIRTY, 0);
    // natural size is the image's whatever was written.
    assert_eq!(
        read(&m, image, si("natural size")),
        Value::Screen((24 * 16) << 16 | (16 * 16))
    );
    m.set(text, si("depth"), Value::Float(4.0)).unwrap();
    assert_eq!(read(&m, text, si("depth")), Value::Float(4.0));
    // The number it already stores, written to a sprite not sized, sizes it.
    m.set(pending, si("height"), Value::Float(96.0)).unwrap();
    assert!(m.sprite(pending).sized());
    assert_eq!(read(&m, pending, si("width")), Value::Float(237.0));
}

#[test]
fn a_reset_with_no_handler_leaves_every_member_as_it_reads() {
    let (mut m, t, a) = one();
    let b = m.spawn(t);
    m.attach(b, Some(a));
    for mem in MEMBERS.iter().filter(|x| x.reset.is_none()) {
        match mem.namespace {
            "spriteinfo" | "spritematrixmeasurement" => {
                let before = m.get(a, mem, &NoWorld);
                assert_eq!(m.reset(a, mem), Ok(()), "{}", mem.path());
                assert_eq!(m.get(a, mem, &NoWorld), before, "{}", mem.path());
            }
            _ => {}
        }
    }
    for mem in MEMBERS.iter().filter(|x| x.reset.is_some()) {
        if matches!(mem.namespace, "spriteinfo" | "spritematrixmeasurement") {
            assert_eq!(m.reset(a, mem), Err(Refusal::Unmodelled), "{}", mem.path());
        }
    }
}

#[test]
fn screen_width_is_the_width_through_every_scale_up_the_tree() {
    let (mut m, t, p) = one();
    let c = m.spawn(t);
    m.attach(c, Some(p));
    m.sprite_mut(p).scale = [150.0, 150.0, 100.0];
    m.sprite_mut(c).scale = [50.0, 100.0, 100.0];
    // 237 x 0.5 x 1.5, 96 x 1.0 x 1.5.
    assert_eq!(
        read(&m, c, si("screen width")),
        Value::Float(237.0 * 0.5 * 1.5)
    );
    assert_eq!(read(&m, c, si("screen height")), Value::Float(96.0 * 1.5));
}

#[test]
fn the_enums_hold_their_bits_and_clamp_to_their_ranges() {
    let (mut m, _, a) = one();
    for (name, range, bits) in [
        ("blend mode", 7, props::BLEND),
        ("alignment", 8, props::ALIGNMENT),
        ("align to parent", 8, props::ALIGN_TO_PARENT),
        ("justify", 2, props::JUSTIFY),
    ] {
        m.set(a, si(name), Value::Int(range)).unwrap();
        assert_eq!(read(&m, a, si(name)), Value::Int(range));
        assert_eq!(bits.get(m.sprite(a).properties), range as u32);
        m.set(a, si(name), Value::Int(range + 5)).unwrap();
        assert_eq!(read(&m, a, si(name)), Value::Int(range), "{name} clamped");
        m.set(a, si(name), Value::Float(1.9)).unwrap();
        assert_eq!(read(&m, a, si(name)), Value::Int(1), "{name} truncated");
    }
    m.set(a, si("texture flip"), Value::Int(3)).unwrap();
    assert_eq!(flags::FLIP.get(m.sprite(a).flags), 3);
    assert_eq!(read(&m, a, si("texture flip")), Value::Int(3));
}

#[test]
fn justify_sets_dirty_only_with_text() {
    let (mut m, _, a) = one();
    m.sprite_mut(a).properties &= !props::DIRTY;
    m.set(a, si("justify"), Value::Int(1)).unwrap();
    assert_eq!(m.sprite(a).properties & props::DIRTY, 0);
    m.sprite_mut(a).text = Value::Entity(7);
    m.set(a, si("justify"), Value::Int(2)).unwrap();
    assert_ne!(m.sprite(a).properties & props::DIRTY, 0);
}

#[test]
fn clipping_once_on_stays_on_and_its_writes_drop_the_written_size() {
    let (mut m, _, a) = one();
    let clip = si("clipping");
    m.set(a, si("width"), Value::Float(40.0)).unwrap();
    m.set(a, clip, Value::Int(1)).unwrap();
    assert_eq!(read(&m, a, clip), Value::Int(1));
    assert!(!m.sprite(a).sized(), "bit 0 cleared, not 25");
    m.set(a, si("width"), Value::Float(40.0)).unwrap();
    m.set(a, clip, Value::Int(0)).unwrap();
    assert_eq!(read(&m, a, clip), Value::Int(1), "still on");
    assert!(!m.sprite(a).sized());
    // Writing what it reads changes nothing.
    m.set(a, si("width"), Value::Float(40.0)).unwrap();
    m.set(a, clip, Value::Int(1)).unwrap();
    assert!(m.sprite(a).sized());
}

#[test]
fn take_a_picture_is_flags_bit_nine() {
    let (mut m, _, a) = one();
    m.set(a, si("Take a Picture"), Value::Int(1)).unwrap();
    assert_eq!(m.sprite(a).flags & flags::PICTURE, flags::PICTURE);
    assert_eq!(read(&m, a, si("Take a Picture")), Value::Int(1));
    m.set(a, si("Take a Picture"), Value::Int(0)).unwrap();
    assert_eq!(m.sprite(a).flags & flags::PICTURE, 0);
}

#[test]
fn a_numeric_value_reads_whole_unless_its_string_has_decimals() {
    let mut m = Model::default();
    let t = m.add_template(None, &template("Text", kind::TEXT), Value::Null);
    let a = m.spawn(t);
    let nv = si("numeric value");
    assert_eq!(read(&m, a, nv), Value::Int(7));
    m.set(a, nv, Value::Float(-2.7)).unwrap();
    assert_eq!(m.sprite(a).numeric_value, -2.0, "truncated toward 0");
    assert_eq!(read(&m, a, nv), Value::Int(-2));
    let obj = Obj { unit: 1, index: 2 };
    let two = Str {
        decimal_digits: 2,
        ..Str::default()
    };
    let s = m.add_file_string(obj, "@", two);
    m.set(a, si("text"), s).unwrap();
    m.sprite_mut(a).properties |= props::WIDTH_VALID;
    m.sprite_mut(a).properties &= !props::DIRTY;
    m.set(a, nv, Value::Float(1.25)).unwrap();
    assert_eq!(read(&m, a, nv), Value::Float(1.25));
    let p = m.sprite(a).properties;
    assert_eq!((p & props::DIRTY != 0, p & props::WIDTH_VALID), (true, 0));
    // One decimal digit is enough to read as a float (0xB3E40: below 1 reads
    // whole).
    let one = Str {
        decimal_digits: 1,
        ..Str::default()
    };
    let s = m.add_file_string(Obj { unit: 1, index: 3 }, "@", one);
    m.set(a, si("text"), s).unwrap();
    m.set(a, nv, Value::Float(2.5)).unwrap();
    assert_eq!(read(&m, a, nv), Value::Float(2.5));
}

#[test]
fn point_size_keeps_twelve_bits_and_the_set_flag_follows_its_sign() {
    let (mut m, _, a) = one();
    let ps = si("point size");
    m.set(a, ps, Value::Int(22)).unwrap();
    assert_eq!(read(&m, a, ps), Value::Int(22));
    assert_ne!(m.sprite(a).flags & flags::POINT_SIZE_SET, 0);
    m.set(a, ps, Value::Int(4096)).unwrap();
    assert_eq!(read(&m, a, ps), Value::Int(0), "the low twelve bits");
    assert_ne!(m.sprite(a).flags & flags::POINT_SIZE_SET, 0);
    // 0 against the 0 the bits hold is no change: the flag stays (the
    // handler compares the twelve bits with the whole value, 0xB4768).
    m.set(a, ps, Value::Int(0)).unwrap();
    assert_ne!(m.sprite(a).flags & flags::POINT_SIZE_SET, 0);
    m.set(a, ps, Value::Int(5)).unwrap();
    m.set(a, ps, Value::Int(-5)).unwrap();
    assert_eq!(read(&m, a, ps), Value::Int(0));
    assert_eq!(m.sprite(a).flags & flags::POINT_SIZE_SET, 0, "clamped to 0");
}

#[test]
fn a_text_write_clears_the_string_flags_and_a_name_extractor_shows_its_object() {
    let (mut m, t, a) = one();
    m.sprite_mut(a).flags |= flags::STRING;
    m.sprite_mut(a).properties |= props::WIDTH_VALID;
    m.set(a, si("text"), Value::Entity(5)).unwrap();
    assert_eq!(m.sprite(a).flags & flags::STRING, 0);
    assert_eq!(m.sprite(a).properties & props::WIDTH_VALID, 0);
    assert_eq!(read(&m, a, si("text")), Value::Entity(5));
    let named = m.value_of(t);
    m.set_global(member("nameextractor", "Object to name"), named)
        .unwrap();
    let extractor = m.singleton(singleton::NAME_EXTRACTOR);
    m.set(a, si("text"), extractor).unwrap();
    assert_eq!(read(&m, a, si("text")), named);
}

#[test]
fn read_only_members_refuse_writes_and_resets_follow_their_handlers() {
    let (mut m, _, a) = one();
    for mem in MEMBERS.iter().filter(|x| x.set.is_none()) {
        let r = if mem.namespace == "spriteinfo" || mem.namespace == "spritematrixmeasurement" {
            m.set(a, mem, Value::Int(1))
        } else {
            m.set_global(mem, Value::Int(1))
        };
        assert_eq!(r, Err(Refusal::ReadOnly), "{}", mem.path());
    }
    // No reset handler: nothing changes.
    m.set(a, si("priority"), Value::Float(3.0)).unwrap();
    m.reset(a, si("priority")).unwrap();
    assert_eq!(read(&m, a, si("priority")), Value::Float(3.0));
    assert_eq!(
        m.reset(a, smm("attached objects")),
        Err(Refusal::Unmodelled)
    );
    m.set(a, smm("ID"), Value::Int(12)).unwrap();
    assert_eq!(read(&m, a, smm("ID")), Value::Int(12));
}

#[test]
fn the_string_converter_turns_a_set_into_a_string_and_back() {
    let mut m = Model::default();
    let g = |n: &str| member("stringconverter", n);
    m.set_global(g("integer digits"), Value::Int(3)).unwrap();
    m.set_global(g("decimal digits"), Value::Int(1)).unwrap();
    m.set_global(g("use comma"), Value::Int(9)).unwrap();
    m.set_global(g("'@' representation"), Value::Int(1))
        .unwrap();
    assert_eq!(
        m.get_global(g("use comma"), &NoWorld),
        Ok(Answer::Value(Value::Int(1)))
    );
    let set = [
        Value::Int(0x47),
        Value::Float(0x40 as f32 - 0.2),
        Value::Int(0x10021),
    ];
    m.set_global_elements(g("value set"), &set).unwrap();
    let Ok(Answer::Value(text)) = m.get_global(g("text"), &NoWorld) else {
        panic!("no text");
    };
    let s = m.string(text).unwrap();
    // 63.8 rounds to 64, an `@`; a code's low sixteen bits.
    assert_eq!(s.units, vec![0x47, 0x40, 0x21]);
    assert_eq!(
        s.flags,
        string_flags::TIME | string_flags::COMMAS | string_flags::HAS_AT
    );
    assert_eq!((s.integer_digits, s.decimal_digits), (3, 1));
    assert_eq!(
        m.get_global(g("value set"), &NoWorld),
        Ok(Answer::Elements(set.to_vec()))
    );
    // An empty set makes no string.
    m.set_global_elements(g("value set"), &[]).unwrap();
    assert_eq!(
        m.get_global(g("text"), &NoWorld),
        Ok(Answer::Value(Value::Null))
    );
    // A file's string taken apart into its characters.
    let obj = Obj { unit: 2, index: 9 };
    let file = m.add_file_string(obj, "Hi", Str::default());
    m.set_global(g("text"), file).unwrap();
    assert_eq!(
        m.get_global(g("value set"), &NoWorld),
        Ok(Answer::Elements(vec![Value::Int(0x48), Value::Int(0x69)]))
    );
    assert_eq!(
        m.get_global(member("stringinfo", "String Converter"), &NoWorld),
        Ok(Answer::Value(m.singleton(singleton::STRING_CONVERTER)))
    );
}

#[test]
fn the_name_checksum_is_the_names_fnv1a_and_the_margins_read_zero() {
    let mut m = Model::default();
    struct Names;
    impl World for Names {
        fn name_of(&self, _: Value) -> Option<String> {
            Some("a".into())
        }
    }
    let sum = member("nameextractor", "Name checksum");
    assert_eq!(
        m.get_global(sum, &Names),
        Ok(Answer::Value(Value::Int(0))),
        "no object"
    );
    m.set_global(member("nameextractor", "Object to name"), Value::Entity(3))
        .unwrap();
    assert_eq!(
        m.get_global(sum, &Names),
        Ok(Answer::Value(Value::Int(0xE40C_292Cu32 as i32)))
    );
    assert_eq!(fnv1a(b""), 0x811C_9DC5);
    let left = member("viewporttitlesafeinfo", "left margin");
    assert_eq!(
        m.get_global(left, &NoWorld),
        Ok(Answer::Value(Value::Int(0)))
    );
}

#[test]
fn the_paired_measures_read_off_the_layouts_places() {
    let (mut m, t, a) = one();
    let b = m.spawn(t);
    struct Places;
    impl World for Places {
        fn place(&self, _: &Model, s: SpriteId) -> Option<[f32; 3]> {
            Some(if s.0 == 1 {
                [3.0, 10.0, 4.0]
            } else {
                [0.0, 4.0, 0.0]
            })
        }
    }
    let p = |n: &str| member("spritepositionmeasurement", n);
    let f = |r| match r {
        Ok(Answer::Value(Value::Float(f))) => f,
        other => panic!("{other:?}"),
    };
    assert_eq!(f(m.get_paired(a, b, p("screen distance"), &Places)), 5.0);
    assert_eq!(f(m.get_paired(a, b, p("X offset"), &Places)), 3.0);
    assert_eq!(f(m.get_paired(a, b, p("Y offset"), &Places)), 4.0);
    assert_eq!(f(m.get_paired(a, b, p("priority offset"), &Places)), 6.0);
    let h = f(m.get_paired(a, b, p("heading"), &Places));
    assert!((h - 36.869_9).abs() < 1e-3, "{h}");
    assert_eq!(f(m.get_paired(a, a, p("heading"), &Places)), 0.0);
    assert_eq!(
        m.get_paired(a, b, p("heading"), &NoWorld),
        Err(Refusal::Unmodelled)
    );
    m.remove(b);
}

/// Every member answers a read, but the computed ones the layout and the
/// text measure give (AT.6, AT.7) and the ones no interface closure reads.
#[test]
fn every_member_but_the_layouts_and_the_texts_answers() {
    let (m, _, a) = one();
    let unmodelled = [
        "spriteinfo.screen location",
        "spritematrixmeasurement.origin",
        "nameextractor.instantiation size",
        "nameextractor.type count",
    ];
    for mem in &MEMBERS {
        let r = match mem.namespace {
            "spriteinfo" | "spritematrixmeasurement" => m.get(a, mem, &NoWorld),
            "spritepositionmeasurement" => continue,
            _ => m.get_global(mem, &NoWorld),
        };
        if unmodelled.contains(&mem.path().as_str()) {
            assert_eq!(r, Err(Refusal::Unmodelled), "{}", mem.path());
        } else {
            assert!(r.is_ok(), "{}: {r:?}", mem.path());
        }
    }
}

/// Members whose read is what was written, or its clamp.
fn plain() -> Vec<&'static Member> {
    [
        "priority",
        "Z Depth",
        "width",
        "height",
        "depth",
        "color",
        "location",
        "blend mode",
        "alignment",
        "align to parent",
        "texture flip",
        "justify",
        "replace color percentage",
        "bloom intensity",
    ]
    .iter()
    .map(|n| si(n))
    .chain([smm("ID")])
    .collect()
}

fn any_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<i32>().prop_map(Value::Int),
        (-1e6f32..1e6).prop_map(Value::Float),
        any::<u32>().prop_map(Value::Color),
        any::<u32>().prop_map(Value::Screen),
        any::<u64>().prop_map(Value::Entity),
    ]
}

proptest! {
    /// Any sequence of writes to any writable sprite member never panics,
    /// and a plain member reads back what it took or its clamp.
    #[test]
    fn any_writes_never_panic_and_plain_members_read_their_clamp(
        writes in prop::collection::vec((0usize..MEMBERS.len(), any_value()), 0..40)
    ) {
        let (mut m, t, a) = one();
        let b = m.spawn(t);
        let plain = plain();
        for (i, v) in writes {
            let mem = &MEMBERS[i];
            if !matches!(mem.namespace, "spriteinfo" | "spritematrixmeasurement") {
                let _ = m.set_global(mem, v);
                continue;
            }
            let target = if i % 2 == 0 { a } else { b };
            let took = m.set(target, mem, v);
            let _ = m.get(target, mem, &NoWorld);
            if took.is_err() || !plain.contains(&mem) {
                continue;
            }
            let expected = guac_script::convert(&mem.spec(), v).unwrap();
            let got = match m.get(target, mem, &NoWorld) {
                Ok(Answer::Value(g)) => g,
                other => panic!("{other:?}"),
            };
            let expected = match (mem.name, expected) {
                ("bloom intensity", Value::Int(n)) => Value::Int(n.min(100)),
                (_, e) => e,
            };
            prop_assert_eq!(got, expected, "{}", mem.path());
        }
    }
}
