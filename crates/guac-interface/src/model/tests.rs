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
        init_matrix: Default::default(),
        matrix: Default::default(),
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
    // No image: a bound texture's desired size × 0.5 (0xB6950).
    assert_eq!(read(&m, pending, si("width")), Value::Float(4.0));
    assert_eq!(read(&m, pending, si("height")), Value::Float(3.0));
    // No image and a desired size that isn't one (a movie's -1, -1): the
    // stored size × 0.5.
    let movie = m.add_template(None, &art(None, [-1, -1]), Value::Null);
    let movie = m.spawn(movie);
    assert_eq!(read(&m, movie, si("width")), Value::Float(118.5));
    // A text with no font: what it was handed, its stored size.
    assert_eq!(read(&m, text, si("width")), Value::Float(237.0));
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
                assert_eq!(m.reset(a, mem, &NoWorld), Ok(()), "{}", mem.path());
                assert_eq!(m.get(a, mem, &NoWorld), before, "{}", mem.path());
            }
            _ => {}
        }
    }
    // Every member with a reset handler resets (AT.25): none unmodelled.
    for mem in MEMBERS.iter().filter(|x| x.reset.is_some()) {
        let r = match mem.namespace {
            "spriteinfo" | "spritematrixmeasurement" => m.reset(a, mem, &NoWorld),
            _ => m.reset_global(mem),
        };
        assert_eq!(r, Ok(()), "{}", mem.path());
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
    m.set_global(member("nameextractor", "Object to name"), named, &NoWorld)
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
            m.set_global(mem, Value::Int(1), &NoWorld)
        };
        assert_eq!(r, Err(Refusal::ReadOnly), "{}", mem.path());
    }
    // No reset handler: nothing changes.
    m.set(a, si("priority"), Value::Float(3.0)).unwrap();
    m.reset(a, si("priority"), &NoWorld).unwrap();
    assert_eq!(read(&m, a, si("priority")), Value::Float(3.0));
    m.set(a, smm("ID"), Value::Int(12)).unwrap();
    assert_eq!(read(&m, a, smm("ID")), Value::Int(12));
}

#[test]
fn the_string_converter_turns_a_set_into_a_string_and_back() {
    let mut m = Model::default();
    let g = |n: &str| member("stringconverter", n);
    m.set_global(g("integer digits"), Value::Int(3), &NoWorld)
        .unwrap();
    m.set_global(g("decimal digits"), Value::Int(1), &NoWorld)
        .unwrap();
    m.set_global(g("use comma"), Value::Int(9), &NoWorld)
        .unwrap();
    m.set_global(g("'@' representation"), Value::Int(1), &NoWorld)
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
    m.set_global(g("text"), file, &NoWorld).unwrap();
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
    m.set_global(
        member("nameextractor", "Object to name"),
        Value::Entity(3),
        &NoWorld,
    )
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

/// The paired reads and writes on the sprites' current matrices (AT.25):
/// each place as its last layout left it, each setter moving the first's.
#[test]
fn the_paired_measures_read_and_move_the_current_matrices_places() {
    let (mut m, t, a) = one();
    let b = m.spawn(t);
    m.sprite_mut(a).matrix.position = [3.0, 10.0, 4.0];
    m.sprite_mut(b).matrix.position = [0.0, 4.0, 0.0];
    let p = |n: &str| member("spritepositionmeasurement", n);
    let f = |r| match r {
        Ok(Answer::Value(Value::Float(f))) => f,
        other => panic!("{other:?}"),
    };
    assert_eq!(f(m.get_paired(a, b, p("screen distance"))), 5.0);
    assert_eq!(f(m.get_paired(a, b, p("X offset"))), 3.0);
    assert_eq!(f(m.get_paired(a, b, p("Y offset"))), 4.0);
    assert_eq!(f(m.get_paired(a, b, p("priority offset"))), 6.0);
    let h = f(m.get_paired(a, b, p("heading")));
    assert!((h - 36.869_9).abs() < 1e-3, "{h}");
    assert_eq!(f(m.get_paired(a, a, p("heading"))), 0.0);

    // The offsets set the first's x, y or depth to the second's less the
    // value (so they read back negated), the rest kept.
    let at = |m: &Model| m.sprite(a).matrix.position;
    m.set_paired(a, b, p("X offset"), Value::Float(2.0))
        .unwrap();
    assert_eq!(at(&m), [-2.0, 10.0, 4.0]);
    m.set_paired(a, b, p("Y offset"), Value::Float(1.5))
        .unwrap();
    assert_eq!(at(&m), [-2.0, 10.0, -1.5]);
    m.set_paired(a, b, p("priority offset"), Value::Float(-3.0))
        .unwrap();
    assert_eq!(at(&m), [-2.0, 7.0, -1.5]);
    assert_eq!(f(m.get_paired(a, b, p("X offset"))), -2.0);

    // The heading round the second at the same distance in x and y, the
    // depth kept: 90 puts it on the second's x axis, 180 straight up.
    m.sprite_mut(a).matrix.position = [3.0, 10.0, 4.0];
    m.set_paired(a, b, p("heading"), Value::Float(90.0))
        .unwrap();
    let [x, d, y] = at(&m);
    assert!(
        (x - 5.0).abs() < 1e-5 && y.abs() < 1e-5 && d == 10.0,
        "{x} {d} {y}"
    );
    m.set_paired(a, b, p("heading"), Value::Float(180.0))
        .unwrap();
    let [x, _, y] = at(&m);
    assert!(x.abs() < 1e-4 && (y + 5.0).abs() < 1e-4, "{x} {y}");
    assert!((f(m.get_paired(a, b, p("heading"))).abs() - 180.0).abs() < 1e-3);

    // The distance along the line from the second, the depth the second's.
    m.sprite_mut(a).matrix.position = [3.0, 10.0, 4.0];
    m.set_paired(a, b, p("screen distance"), Value::Float(10.0))
        .unwrap();
    let [x, d, y] = at(&m);
    assert!((x - 6.0).abs() < 1e-5 && (y - 8.0).abs() < 1e-5, "{x} {y}");
    assert_eq!(d, 4.0, "the second's depth");
    // Standing on it in x and y: the length is never 0, so it stays on the
    // second, its depth the second's.
    m.sprite_mut(a).matrix.position = [0.0, 9.0, 0.0];
    m.set_paired(a, b, p("screen distance"), Value::Float(7.0))
        .unwrap();
    assert_eq!(at(&m), [0.0, 4.0, 0.0]);

    // A write doesn't reach `location`: the next layout puts the place back.
    m.sprite_mut(a).matrix.rotation = [0.0; 3];
    m.set(a, si("visible"), Value::Int(1)).unwrap();
    m.set_paired(a, b, p("X offset"), Value::Float(100.0))
        .unwrap();
    m.lay_out(&NoWorld);
    assert_ne!(at(&m)[0], -100.0, "laid out again");
    assert_eq!(read(&m, a, si("location")), Value::Screen(0x0140_FEC0));
    m.remove(b);
}

/// `screen location` is the sprite's anchor point off its last layout: its
/// alignment's offset off its stored size, at its computed scale, carried
/// by its current matrix (AT.25).
#[test]
fn screen_location_is_the_anchor_point_off_the_last_layout() {
    let mut m = Model::default();
    let mut root = template("Root", kind::CONTAINER);
    root.position = [0, 0];
    root.width = 640.0;
    root.height = 480.0;
    root.scale = [150.0; 3];
    let mut box_ = template("Box", kind::CONTAINER);
    // Bottom-right on its parent's bottom-right, 20 units in and up.
    box_.properties = props::VISIBLE | (8 << 8) | (8 << 12) | kind::CONTAINER;
    box_.position = [-20 * 16, -20 * 16];
    box_.width = 100.0;
    box_.height = 40.0;
    box_.scale = [100.0; 3];
    let r = m.add_template(None, &root, Value::Null);
    let b = m.add_template(None, &box_, Value::Null);
    m.wake(r);
    m.wake(b);
    m.attach(b, Some(r));
    // Before any layout: its current matrix's place (the template's) and
    // the offset unscaled, 100 by 40 at the bottom-right.
    m.sprite_mut(b).matrix.position = [1.0, 0.0, 2.0];
    let sl = |m: &Model| read(m, b, si("screen location"));
    assert_eq!(sl(&m), Value::Screen((101 * 16) << 16 | (42 * 16)));
    m.lay_out(&NoWorld);
    // The root 960 by 720 frame pixels at 1.5; the box hangs 30 px in from
    // its bottom-right, (930, 690), its top-left 150 by 60 back from there.
    assert_eq!(m.sprite(b).matrix.position[0], 930.0 - 150.0);
    assert_eq!(m.sprite(b).computed_scale, Some([1.5; 3]));
    assert_eq!(sl(&m), Value::Screen((930 * 16) << 16 | (690 * 16)));
    // Its anchor turned with it: turned 90 about depth, the offset (150, 60)
    // goes to (60, -150) from its top-left.
    m.sprite_mut(b).matrix.rotation = [0.0, 90.0, 0.0];
    let [x, _, y] = m.sprite(b).matrix.position;
    let Value::Screen(w) = sl(&m) else { panic!() };
    let (sx, sy) = ((w >> 16) as u16 as i16, w as u16 as i16);
    assert_eq!(
        (sx, sy),
        (((x + 60.0) * 16.0) as i16, ((y - 150.0) * 16.0) as i16)
    );
    // Truncated toward zero to 1/16, into 16 bits.
    m.sprite_mut(b).matrix.rotation = [0.0; 3];
    m.sprite_mut(b).matrix.position = [-0.09, 0.0, 0.0];
    m.sprite_mut(b).computed_scale = Some([0.0; 3]);
    let Value::Screen(w) = sl(&m) else { panic!() };
    assert_eq!(((w >> 16) as u16 as i16, w as u16 as i16), (-1, 0));
}

/// `origin` is the initial matrix; waking copies it into the current one,
/// and a clone starts from its source's current one (AT.25).
#[test]
fn origin_is_the_initial_matrix_and_waking_copies_it() {
    let mut m = Model::default();
    let mut t = template("Shade", kind::CONTAINER);
    t.init_matrix = Matrix {
        position: [0.914, 10_000.0, 480.0],
        rotation: [0.0, 90.0, 0.0],
    };
    t.matrix = Matrix {
        position: [5.0, 0.0, 6.0],
        rotation: [0.0, 45.0, 0.0],
    };
    let id = m.add_template(None, &t, Value::Null);
    assert_eq!(
        m.get(id, smm("origin"), &NoWorld),
        Ok(Answer::Matrix(t.init_matrix))
    );
    assert_eq!(m.sprite(id).matrix, t.matrix, "asleep, as the file has it");
    let c = m.spawn(id);
    assert_eq!(
        (m.sprite(c).init_matrix, m.sprite(c).matrix),
        (t.matrix, t.matrix),
        "a clone from its source's current matrix"
    );
    m.wake(id);
    assert_eq!(m.sprite(id).matrix, t.init_matrix);
}

/// `ID`'s reset puts back what it held before its first write, and does
/// nothing on one never written (AT.25, 0x6A830).
#[test]
fn an_ids_reset_takes_back_the_one_before_its_first_write() {
    let (mut m, _, a) = one();
    let id = smm("ID");
    m.reset(a, id, &NoWorld).unwrap();
    assert_eq!(read(&m, a, id), Value::Int(0), "never written");
    m.set(a, id, Value::Int(5)).unwrap();
    m.set(a, id, Value::Int(9)).unwrap();
    m.reset(a, id, &NoWorld).unwrap();
    assert_eq!(read(&m, a, id), Value::Int(0));
    m.set(a, id, Value::Int(3)).unwrap();
    m.reset(a, id, &NoWorld).unwrap();
    assert_eq!(read(&m, a, id), Value::Int(0), "the first snapshot kept");
}

/// A set's reset resets each sprite in it, last first, and writes nothing
/// back (AT.25, 0x7BB04 and 0x6A020): an awake clone taken off and woken
/// again, a template that isn't a clone put to sleep.
#[test]
fn a_sets_reset_restarts_each_sprite_it_holds() {
    let mut m = Model::default();
    let t = m.add_template(None, &template("Box", kind::CONTAINER), Value::Null);
    let other = m.add_template(None, &template("Other", kind::CONTAINER), Value::Null);
    m.wake(t);
    m.wake(other);
    let [a, b, g] = [m.spawn(t), m.spawn(t), m.spawn(t)];
    m.attach(a, Some(t));
    m.attach(other, Some(t));
    m.attach(b, Some(t));
    m.attach(g, Some(a));
    m.sprite_mut(a).activator = Value::Int(77);
    m.sprite_mut(a).init_matrix.position = [1.0, 2.0, 3.0];
    m.sprite_mut(a).matrix.position = [9.0, 9.0, 9.0];
    assert_eq!(m.active(), &[t, other, a, b, g]);
    m.reset(t, smm("attached objects"), &NoWorld).unwrap();
    // b first, then the template, then a: each off t; a's child off it.
    assert!(elements(&m, t, smm("attached objects")).is_empty());
    assert!(m.sprite(a).children.is_empty() && m.sprite(g).parent.is_none());
    assert_eq!(m.sprite(other).state, State::Asleep);
    assert_eq!(m.active(), &[t, g, b, a], "woken again, last first");
    assert_eq!(m.sprite(a).matrix.position, [1.0, 2.0, 3.0]);
    assert_eq!(m.sprite(a).activator, Value::Int(77), "its activator kept");
    assert_eq!(
        elements(&m, t, smm("clones")),
        [a, b, g].map(|x| m.value_of(x)).to_vec(),
        "its source kept, its clones as they were"
    );
    // `active`'s puts every awake template to sleep and wakes each clone.
    m.reset(t, si("active"), &NoWorld).unwrap();
    assert_eq!(m.sprite(t).state, State::Asleep);
    assert_eq!(m.active(), &[a, b, g]);
}

/// `Instantiation size` and `type count` are what the held object answers,
/// 0 with none (AT.25).
#[test]
fn the_name_extractors_class_numbers_come_from_the_object() {
    let mut m = Model::default();
    struct Class;
    impl World for Class {
        fn class_numbers(&self, v: Value) -> Option<[i32; 2]> {
            (v == Value::Entity(5)).then_some([0xE4, 3])
        }
    }
    let size = member("nameextractor", "Instantiation size");
    let count = member("nameextractor", "type count");
    let n = |m: &Model, mem| match m.get_global(mem, &Class) {
        Ok(Answer::Value(Value::Int(n))) => n,
        other => panic!("{other:?}"),
    };
    assert_eq!((n(&m, size), n(&m, count)), (0, 0), "no object");
    m.set_global(
        member("nameextractor", "Object to name"),
        Value::Entity(5),
        &Class,
    )
    .unwrap();
    assert_eq!((n(&m, size), n(&m, count)), (0xE4, 3));
}

/// The converters' set resets write an empty set back: no string, no
/// colours (AT.25, the Hypothesis in the crate docs).
#[test]
fn the_converters_set_resets_write_an_empty_set_back() {
    let mut m = Model::default();
    let vs = member("stringconverter", "value set");
    m.set_global_elements(vs, &[Value::Int(72), Value::Int(105)])
        .unwrap();
    let text = member("stringconverter", "text");
    assert_ne!(m.get_global(text, &NoWorld), Ok(Answer::Value(Value::Null)));
    m.reset_global(vs).unwrap();
    assert_eq!(m.get_global(text, &NoWorld), Ok(Answer::Value(Value::Null)));
    assert_eq!(m.get_global(vs, &NoWorld), Ok(Answer::Elements(Vec::new())));
    let cs = member("textcolorconverter", "color set");
    m.set_global_elements(cs, &[Value::Color(0xFF00_00FF)])
        .unwrap();
    m.reset_global(cs).unwrap();
    assert_eq!(m.get_global(cs, &NoWorld), Ok(Answer::Elements(Vec::new())));
}

/// Every member answers a read and a reset, and a paired one its pair
/// (AT.25): none left unmodelled.
#[test]
fn every_member_answers() {
    let (m, t, a) = one();
    let mut m = m;
    let b = m.spawn(t);
    for mem in &MEMBERS {
        let r = match mem.namespace {
            "spriteinfo" | "spritematrixmeasurement" => m.get(a, mem, &NoWorld).map(drop),
            "spritepositionmeasurement" => m.get_paired(a, b, mem).map(drop),
            _ => m.get_global(mem, &NoWorld).map(drop),
        };
        assert!(r.is_ok(), "{}: {r:?}", mem.path());
        if mem.set.is_none() {
            continue;
        }
        // Every write lands or is a value the member can't take, never
        // unmodelled.
        let w = match mem.namespace {
            "spriteinfo" | "spritematrixmeasurement" => m.set(a, mem, Value::Int(1)),
            "spritepositionmeasurement" => m.set_paired(a, b, mem, Value::Float(1.0)),
            _ => m.set_global(mem, Value::Int(1), &NoWorld),
        };
        assert_ne!(w, Err(Refusal::Unmodelled), "{}", mem.path());
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
                let _ = m.set_global(mem, v, &NoWorld);
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

/// A font 82 high with a glyph for each digit, `@` and a few letters, every
/// cell its step wide.
fn digits_font() -> guac_ir::interface::Font {
    let mut glyphs: Vec<guac_ir::interface::Glyph> = " 0123456789@Lelv"
        .chars()
        .map(|c| guac_ir::interface::Glyph {
            char: c as u16,
            step: 20.0 + (c as u32 % 5) as f32,
            offset: [0.0, 0.0],
            size: [20.0 + (c as u32 % 5) as f32, 82.0],
            uv_min: [0.0; 2],
            uv_max: [0.01; 2],
            kern: [0; 4],
        })
        .collect();
    glyphs.sort_by_key(|g| g.char);
    guac_ir::interface::Font {
        name: None,
        source: None,
        height: 82.0,
        base_line: 0.0,
        char_padding: 0.0,
        kern_amount: 0.0,
        color: [0.5, 0.5, 0.5, 1.0],
        atlas: "textures/f.ktx2".into(),
        atlas_size: [1024, 1024],
        glyphs,
    }
}

/// A text clone in the digits font showing a file string `@` with *has an
/// `@`*, at 41 points.
fn number_text() -> (Model, SpriteId, crate::text::Metrics<'static>) {
    let font: &'static _ = Box::leak(Box::new(digits_font()));
    let mut m = Model::default();
    let f = m.add_font(font.clone());
    let obj = Obj { unit: 1, index: 9 };
    let at = Str {
        flags: string_flags::HAS_AT,
        ..Str::default()
    };
    let s = m.add_file_string(obj, "@", at);
    let mut t = template("Number", kind::TEXT);
    t.flags = 0;
    let t = m.add_template(None, &t, s);
    m.set_font(t, Some(f));
    let a = m.spawn(t);
    m.set(a, si("point size"), Value::Int(41)).unwrap();
    m.set(a, si("numeric value"), Value::Int(290)).unwrap();
    (m, a, crate::text::Metrics::new(font))
}

#[test]
fn a_text_measures_its_filled_string_in_its_font() {
    let (mut m, a, f) = number_text();
    let u = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    assert_eq!(m.text_units(a, &NoWorld), Some(u("290")));
    let w = f.run_width(&u("290")) * 0.5;
    // Unwritten: the string's width by one line, at the point size.
    assert_eq!(read(&m, a, si("width")), Value::Float(w));
    assert_eq!(read(&m, a, si("height")), Value::Float(41.0));
    // natural size in 12.4, truncated to a sixteenth.
    let screen = |x: f32, y: f32| {
        Value::Screen(
            u32::from((x * 16.0) as i16 as u16) << 16 | u32::from((y * 16.0) as i16 as u16),
        )
    };
    assert_eq!(read(&m, a, si("natural size")), screen(w, 41.0));
    // Written back as the width, the glyphs stretch to the truncated size.
    let Value::Screen(n) = read(&m, a, si("natural size")) else {
        unreachable!()
    };
    let truncated = f32::from((n >> 16) as u16 as i16) / 16.0;
    m.set(a, si("width"), Value::Float(truncated)).unwrap();
    m.set(a, si("height"), Value::Float(41.0)).unwrap();
    let (glyphs, out) = m.glyph_layout(a, &NoWorld).unwrap();
    assert_eq!(glyphs.len(), 3);
    assert_eq!(out.scale, [truncated / f.run_width(&u("290")), 0.5]);
    // A string with no `@` flag keeps its `@`; no string, no glyphs.
    m.set(a, si("text"), Value::Null).unwrap();
    assert_eq!(m.glyph_layout(a, &NoWorld), None);
    assert_eq!(read(&m, a, si("natural size")), screen(0.0, 41.0));
}

#[test]
fn natural_size_reads_the_sixteenth_below() {
    // 65 font pixels at 26 points is 20.6098 units, 329.76 sixteenths: read
    // as 329, so a width written back from it draws a hair narrower (AC.13's
    // gold `0`, AT.6).
    let (mut m, a, _) = number_text();
    m.set(a, si("point size"), Value::Int(26)).unwrap();
    let Value::Screen(n) = read(&m, a, si("natural size")) else {
        unreachable!()
    };
    assert_eq!(n >> 16, 329);
}

#[test]
fn a_text_holding_an_object_shows_its_name() {
    let (mut m, a, f) = number_text();
    struct Named;
    impl World for Named {
        fn name_of(&self, v: Value) -> Option<String> {
            (v == Value::Entity(77)).then(|| "Lev".into())
        }
    }
    m.set(a, si("text"), Value::Entity(77)).unwrap();
    let u: Vec<u16> = "Lev".encode_utf16().collect();
    assert_eq!(m.text_units(a, &Named), Some(u.clone()));
    assert_eq!(
        read_with(&m, a, si("width"), &Named),
        Value::Float(f.run_width(&u) * 0.5)
    );
    // Without the name, nothing to measure.
    assert_eq!(read(&m, a, si("width")), Value::Float(0.0));
}

fn read_with(m: &Model, id: SpriteId, mem: &Member, w: &dyn World) -> Value {
    match m.get(id, mem, w) {
        Ok(Answer::Value(v)) => v,
        other => panic!("{}: {other:?}", mem.path()),
    }
}

#[test]
fn the_text_colour_converter_colours_letters_and_reads_them_back() {
    let (mut m, a, _) = number_text();
    let tc = |n: &str| member("textcolorconverter", n);
    let av = m.value_of(a);
    m.set_global(tc("text sprite"), av, &NoWorld).unwrap();
    // Read back: one colour per glyph drawn, each the font's 0x808080FF.
    let grey = Value::Color(0x8080_80FF);
    assert_eq!(
        m.get_global(tc("color set"), &NoWorld),
        Ok(Answer::Elements(vec![grey; 3]))
    );
    // Two colours colour the first two letters; the third keeps its own.
    let red = Value::Color(0xFF00_00FF);
    let blue = Value::Color(0x0000_FFFF);
    m.set_global_elements(tc("color set"), &[red, blue])
        .unwrap();
    assert_eq!(
        m.sprite(a).letters,
        vec![[0xFF, 0, 0, 0xFF], [0, 0, 0xFF, 0xFF]]
    );
    m.set_global(tc("text sprite"), av, &NoWorld).unwrap();
    assert_eq!(
        m.get_global(tc("color set"), &NoWorld),
        Ok(Answer::Elements(vec![red, blue, grey]))
    );
    // A new number rebuilds the letters in the font's colour.
    m.set(a, si("numeric value"), Value::Int(31)).unwrap();
    assert!(m.sprite(a).letters.is_empty());
    m.set_global(tc("text sprite"), av, &NoWorld).unwrap();
    assert_eq!(
        m.get_global(tc("color set"), &NoWorld),
        Ok(Answer::Elements(vec![grey; 2]))
    );
}
