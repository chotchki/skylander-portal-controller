//! The places on memory laid out in code ([`crate::actors::fixture`] and
//! [`crate::script::fixture`]), the disc's offsets taken from where the
//! fixture put each placement against a base of its heap, as the census's
//! tests do.

use super::*;
use crate::actors::fixture::{Spec, World};
use crate::actors::{self};
use crate::route::census::DiscActor;
use crate::script::fixture::{self as fx, Types as FxTypes};
use crate::watch::WatchList;

const HEAP: u32 = 0x3000_0000;

fn on_disc(disc: &mut Disc, name: &str, placement: u32) {
    disc.actors.push(DiscActor {
        offset: placement - HEAP,
        name: name.to_owned(),
        script: None,
        range: 0.0,
        position: [0.0; 3],
    });
}

/// Three actors on the game's list to vote the base (one a sheep with a
/// script of its own), a rock waiting with its `Timer` and `Open`, and an
/// activator whose script never started.
struct Level {
    w: World,
    t: FxTypes,
    disc: Disc,
    sheep: u32,
    rock: u32,
    timer: u32,
    open: u32,
    wait: u32,
}

fn level() -> Level {
    let mut w = World::new();
    let t = fx::types(&mut w);
    let mut disc = Disc::default();
    let mut sheep = 0;
    for name in ["Sheep_1", "barrel_b_intact(1)", "pottery_2(5)"] {
        let a = w.actor(&Spec {
            name: name.into(),
            position: [51.0, 48.0, -72.5],
            rotation: [0.0, 90.0, 0.0],
            ..Spec::default()
        });
        on_disc(&mut disc, name, a.placement);
        if name == "Sheep_1" {
            sheep = a.placement;
        }
    }
    let moving = fx::int(&mut w, &t, "Moving", 0);
    let list = w.list(vtable::OBJECT_LIST, &[moving], 1);
    w.put32(sheep + 0x1C, 0x8);
    w.put32(sheep + 0xB0, list);
    let roll = fx::float(&mut w, &t, "Roll", 12.0);
    let timer = fx::int(&mut w, &t, "Timer", 0);
    let open = fx::int(&mut w, &t, "Open", 0);
    let rock = fx::placement(
        &mut w,
        "Sliding_Rock",
        Some("Waiting"),
        7.25,
        Some(&[roll, timer, open]),
    );
    on_disc(&mut disc, "Sliding_Rock", rock);
    let wait = w.get32(rock + 0xAC);
    let activator = fx::placement(&mut w, "Sliding_Rock_Activator_1", None, 0.0, None);
    w.put32(activator + 0x54, 2);
    on_disc(&mut disc, "Sliding_Rock_Activator_1", activator);
    Level {
        w,
        t,
        disc,
        sheep,
        rock,
        timer,
        open,
        wait,
    }
}

fn spec(text: &str) -> PlaceSpec {
    PlaceSpec::parse(text).unwrap()
}

fn err<T: std::fmt::Debug>(r: Result<T>) -> String {
    format!("{:#}", r.unwrap_err())
}

/// What O5 copies at a pad read: each range's bytes back to back.
fn sample(w: &mut World, list: &WatchList, flip: u64) -> WatchRecord {
    let ranges: Vec<(u32, usize)> = list
        .ranges()
        .iter()
        .map(|r| (r.addr, r.len as usize))
        .collect();
    let payload: Vec<u8> = w
        .gather(&ranges)
        .unwrap()
        .into_iter()
        .flat_map(|b| b.unwrap())
        .collect();
    WatchRecord {
        flip,
        host_us: 0,
        missing: 0,
        read: 0,
        payload,
    }
}

#[test]
fn a_place_is_its_name_and_the_numbers_after_the_equals_sign() {
    assert_eq!(
        spec("Sheep_1=Moving,Animation Timer"),
        PlaceSpec {
            name: "Sheep_1".into(),
            variables: vec!["Moving".into(), "Animation Timer".into()],
        }
    );
    assert_eq!(spec("Sliding_Rock(5)").variables, Vec::<String>::new());
    for bad in ["", "=Timer", "Sliding_Rock=", "Sliding_Rock=Timer,,Open"] {
        assert!(PlaceSpec::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn places_resolve_at_the_voted_base_and_go_on_the_list_whole() {
    let mut l = level();
    let frame = actors::read(&mut l.w).unwrap();
    let specs = [
        spec("Sliding_Rock=Timer,Open"),
        spec("Sheep_1=Moving"),
        spec("Sliding_Rock_Activator_1"),
    ];
    let p = PlaceWatch::resolve(&mut l.w, &frame, &l.disc, &specs, None).unwrap();
    let activator = HEAP + l.disc.offsets()["Sliding_Rock_Activator_1"];
    assert_eq!(
        p.places,
        vec![
            ("Sliding_Rock".to_owned(), l.rock),
            ("Sheep_1".to_owned(), l.sheep),
            ("Sliding_Rock_Activator_1".to_owned(), activator),
        ]
    );
    assert_eq!(p.cells.len(), 3);
    assert!(p.missing.is_empty());
    assert_eq!((p.cells[0].1.as_str(), p.cells[0].2), ("Timer", l.timer));
    assert_eq!((p.cells[1].1.as_str(), p.cells[1].2), ("Open", l.open));
    // The rock's Roll, which isn't named, is a float: its type is how the
    // named integers will read once they're floats.
    assert_eq!(p.float_type, l.t.float);
    let fields = p.list_fields();
    let place = fields
        .iter()
        .find(|f| f.name == "place.Sliding_Rock")
        .unwrap();
    assert_eq!((place.addr, place.len), (l.rock + 0x3C, 0xB8 - 0x3C));
    // The three cells sit within MAX_SPAN of each other: one block, from the
    // first one's type word to the last one's value.
    let cells: Vec<&Field> = fields
        .iter()
        .filter(|f| f.name.starts_with("cells."))
        .collect();
    let lo = l.timer.min(l.open).min(p.cells[2].2);
    let hi = l.timer.max(l.open).max(p.cells[2].2);
    assert_eq!(cells.len(), 1);
    assert_eq!(
        (cells[0].addr, cells[0].len),
        (lo + 0x1C, hi + 0x24 - (lo + 0x1C))
    );
    WatchList::new(fields).unwrap();
}

#[test]
fn cells_far_apart_go_in_blocks_of_their_own() {
    let mut l = level();
    // A second rock whose Timer the heap put well past the first's.
    l.w.alloc(2 * MAX_SPAN as usize);
    let timer = fx::float(&mut l.w, &l.t, "Timer", 9.5);
    let rock = fx::placement(
        &mut l.w,
        "Sliding_Rock(1)",
        Some("Delay"),
        7.25,
        Some(&[timer]),
    );
    on_disc(&mut l.disc, "Sliding_Rock(1)", rock);
    let frame = actors::read(&mut l.w).unwrap();
    let specs = [spec("Sliding_Rock=Timer"), spec("Sliding_Rock(1)=Timer")];
    let p = PlaceWatch::resolve(&mut l.w, &frame, &l.disc, &specs, None).unwrap();
    let blocks: Vec<Field> = p
        .list_fields()
        .into_iter()
        .filter(|f| f.name.starts_with("cells."))
        .collect();
    assert_eq!(blocks.len(), 2);
    assert!(blocks.iter().all(|b| b.len == 8), "{blocks:?}");
    let list = WatchList::new(p.list_fields()).unwrap();
    let at = p.read(&list, &sample(&mut l.w, &list, 1)).unwrap();
    assert_eq!(at[0].variables["Timer"], Number::Int(0));
    assert_eq!(at[1].variables["Timer"], Number::Float(9.5));
}

#[test]
fn a_record_reads_where_each_place_stood_its_wake_and_its_numbers() {
    let mut l = level();
    let frame = actors::read(&mut l.w).unwrap();
    // A variable by name whatever its case, as the VM finds one.
    let specs = [spec("Sliding_Rock=Timer,open"), spec("Sheep_1=Moving")];
    let p = PlaceWatch::resolve(&mut l.w, &frame, &l.disc, &specs, Some(l.t.float)).unwrap();
    let list = WatchList::new(p.list_fields()).unwrap();
    let before = p.read(&list, &sample(&mut l.w, &list, 10)).unwrap();
    assert_eq!(before[0].name, "Sliding_Rock");
    assert_eq!(
        (
            before[0].activity_state,
            before[0].behaviour,
            before[0].active_seconds
        ),
        (1, l.wait, 7.25)
    );
    assert_eq!(before[0].variables["Timer"], Number::Int(0));
    assert_eq!(before[0].variables["open"], Number::Int(0));
    assert_eq!(
        (before[1].position, before[1].rotation),
        ([51.0, 48.0, -72.5], [0.0, 90.0, 0.0])
    );
    assert_eq!(before[1].variables["Moving"], Number::Int(0));
    // The activator opens the rock and the rock draws its wait: Open an int
    // 1, Timer retyped to a float, the next behaviour and a later clock.
    l.w.put32(l.open + 0x20, 1);
    l.w.put32(l.timer + 0x1C, l.t.float);
    l.w.putf(l.timer + 0x20, 7.25 + 0.371_337);
    let delay = fx::named(&mut l.w, vtable::USER_BEHAVIOR, 0x28, "Delay");
    l.w.put32(l.rock + 0xAC, delay);
    l.w.putf(l.rock + 0xB4, 7.266_667);
    l.w.put_floats(l.sheep + 0x3C, &[52.0, 48.5, -70.0]);
    let after = p.read(&list, &sample(&mut l.w, &list, 11)).unwrap();
    assert_eq!(after[0].variables["open"], Number::Int(1));
    assert_eq!(after[0].variables["Timer"], Number::Float(7.25 + 0.371_337));
    assert_eq!(
        (after[0].behaviour, after[0].active_seconds),
        (delay, 7.266_667)
    );
    assert_eq!(after[1].position, [52.0, 48.5, -70.0]);
    // Record by record into the file, the behaviours named once.
    let records = [sample(&mut l.w, &list, 11)];
    let file = PlacesFile::of(&mut l.w, &p, &list, &records).unwrap();
    assert_eq!(file.records[0].0, 11);
    assert_eq!(file.behaviours[&delay], "Delay");
    // A record missing a range has no places.
    let mut cut = records[0].clone();
    cut.missing = 1;
    assert!(p.read(&list, &cut).is_none());
    assert!(PlacesFile::of(&mut l.w, &p, &list, &[cut]).is_err());
}

#[test]
fn what_cant_be_watched_is_refused_naming_it() {
    let mut l = level();
    let frame = actors::read(&mut l.w).unwrap();
    let mut resolve = |specs: &[PlaceSpec]| {
        err(PlaceWatch::resolve(
            &mut l.w,
            &frame,
            &l.disc,
            specs,
            Some(l.t.float),
        ))
    };
    assert!(resolve(&[]).contains("no places"));
    assert!(resolve(&[spec("Nobody")]).contains("Nobody"));
    assert!(resolve(&[spec("Sheep_1=Moving,moving")]).contains("twice"));
    assert!(resolve(&[spec("Sliding_Rock_Activator_1=Timer")]).contains("never started"));
    assert!(resolve(&[spec("Sliding_Rock"), spec("Sliding_Rock=Timer")]).contains("twice"));
    assert!(resolve(&[spec("Sliding_Rock=Timer,Timer")]).contains("twice"));
    // A variable that's a reference, not a number.
    let mut l2 = level();
    let target = fx::reference(&mut l2.w, vtable::SCRIPT_REFERENCE, "target", 0);
    let rock = fx::placement(&mut l2.w, "Sliding_Rock(5)", None, 0.0, Some(&[target]));
    on_disc(&mut l2.disc, "Sliding_Rock(5)", rock);
    let frame2 = actors::read(&mut l2.w).unwrap();
    let e = err(PlaceWatch::resolve(
        &mut l2.w,
        &frame2,
        &l2.disc,
        &[spec("Sliding_Rock(5)=target")],
        None,
    ));
    assert!(e.contains("isn't a number"), "{e}");
    // With no float among the place's numbers and none given, nothing says
    // how to read them.
    let e = err(PlaceWatch::resolve(
        &mut l2.w,
        &frame2,
        &l2.disc,
        &[spec("Sheep_1=Moving")],
        None,
    ));
    assert!(e.contains("FloatMeasurement"), "{e}");
    // A placement carrying another name at that offset.
    let mut l3 = level();
    l3.disc.actors.iter_mut().for_each(|a| {
        if a.name == "Sliding_Rock" {
            a.name = "Sliding_Rock(9)".into();
        }
    });
    let frame3 = actors::read(&mut l3.w).unwrap();
    let e = err(PlaceWatch::resolve(
        &mut l3.w,
        &frame3,
        &l3.disc,
        &[spec("Sliding_Rock(9)")],
        None,
    ));
    assert!(e.contains("not `Sliding_Rock(9)`"), "{e}");
}

#[test]
fn a_variable_the_place_doesnt_hold_is_listed_missing_and_the_rest_watched() {
    let mut l = level();
    let frame = actors::read(&mut l.w).unwrap();
    let specs = [spec("Sliding_Rock=Timer,Verticle,Open")];
    let p = PlaceWatch::resolve(&mut l.w, &frame, &l.disc, &specs, Some(l.t.float)).unwrap();
    assert_eq!(
        p.missing,
        vec![("Sliding_Rock".to_owned(), "Verticle".to_owned())]
    );
    let names: Vec<&str> = p.cells.iter().map(|c| c.1.as_str()).collect();
    assert_eq!(names, ["Timer", "Open"]);
    let list = WatchList::new(p.list_fields()).unwrap();
    let at = p.read(&list, &sample(&mut l.w, &list, 3)).unwrap();
    assert_eq!(at[0].variables.len(), 2);
}
