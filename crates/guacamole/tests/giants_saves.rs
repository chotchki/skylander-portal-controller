//! AS.38's, AU.3's and AU.4's disc tests, headless with guacamole's host:
//! the original's save scripts asking for saves on the TV's run, each read
//! complete once a store stand-in has written it; chapter 1's end saved and
//! listed; and a fresh run loading that save going where the original's load
//! goes. Ignored by default; they read `GUAC_GIANTS_DUMP` (process env, else
//! `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_saves -- --ignored --nocapture --test-threads=1`
//!
//! (Stand-in ledger: `story-save`.)

use std::collections::BTreeMap;

use guac_motion::seam::Vec3;
use guac_script::{Mode, Value, Vm};
use guac_server::saves::SaveValues;
use guacamole::level_end;
use guacamole::story_save::{self, Asked, File, Layout, Resume, Session, label};

mod common;

use common::*;

/// The outro's player waypoint, facing `Temp_Level_Ender`: the dev entry's
/// end anchor (as `giants_level_end` sets him down).
const END: ([f32; 3], f32) = ([-10.282436, -153.01357, 318.01202], 17.6);
const CHAPTER_1: &str = "level_213_tutotial";
const LEVEL_214: &str = "level_214_tutotial_2";
/// A host profile's id, as the store keys it.
const HOST: &str = "01HOSTPROFILE";

/// The store's stand-in: each write handed over done `delay` steps later.
#[derive(Default)]
struct Store {
    delay: u32,
    pending: Option<(u32, String, SaveValues)>,
    rows: Vec<(String, SaveValues)>,
}

impl Store {
    fn pump(&mut self, session: &mut Session, run: &mut Run) {
        if self.pending.is_none()
            && let Some((host, layout, values)) = session.take_write()
        {
            assert_eq!(host, HOST);
            self.pending = Some((self.delay, layout, values));
        }
        if let Some((left, layout, values)) = self.pending.take() {
            if left == 0 {
                self.rows.push((layout, values));
                session.written(&mut run.world.save, Ok(()));
            } else {
                self.pending = Some((left - 1, layout, values));
            }
        }
    }
}

/// One step in play mode, then the save system's requests answered as the
/// TV's system answers them after the step, the store pumped.
fn step(run: &mut Run, session: &mut Session, store: &mut Store, level: &str) {
    let stops = run.step_play().unwrap();
    assert!(
        stops.is_empty(),
        "{:#?}",
        stops.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
    {
        let mut spawn = guacamole::scripts::headless_spawner(1 << 50);
        let mut engine = guacamole::script_engine::NoEngine;
        let mut host = guacamole::scripts::WorldHost {
            world: &mut run.world,
            engine: &mut engine,
            generator: &mut run.generator,
            spawn: &mut spawn,
        };
        story_save::serve(&mut run.vm, &mut host, session, level).unwrap_or_else(|h| panic!("{h}"));
    }
    store.pump(session, run);
    assert_eq!(run.world.save.monitor_stopped(), None);
}

fn control(vm: &Vm) -> Value {
    let g = vm.program().global_named("savegame control data").unwrap();
    vm.global_peek(g).unwrap()
}

fn int(vm: &Vm, v: Option<Value>) -> i64 {
    match v.map(|v| vm.value(v)) {
        Some(Value::Int(i)) => i64::from(i),
        Some(Value::Float(f)) => f as i64,
        other => panic!("{other:?}"),
    }
}

/// The save system idle: no request processing, none queued, no finalize
/// owed.
fn idle(vm: &Vm) -> bool {
    let c = control(vm);
    vm.field(c, "processing request").map(|v| vm.value(v)) == Some(Value::Null)
        && vm
            .field(c, "save requests")
            .and_then(|s| vm.set(s).map(<[_]>::len))
            == Some(0)
        && int(vm, vm.field(c, "need finalize")) == 0
}

/// AS.38: a fresh host's run on chapter 1 asks the level initializer's save
/// at its start; the scripts' requests (the slot, the header, the finalize
/// and the header again) each read active until the store stand-in has
/// written the save, 30 steps on, then done, and the save system goes idle. What the store holds is
/// every param named as the scripts name it, the slot marked in use as the
/// title's new game marks it, nothing done.
#[test]
#[ignore = "reads the Giants dump"]
fn a_save_asked_reads_complete_once_the_store_has_written() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, _found) = run_on_with(Mode::Play, Setup::default());
    let mut session = Session::new(Some(HOST.into()));
    let mut store = Store {
        delay: 30,
        ..Store::default()
    };
    // The first step runs the init: the start can't be made before it.
    step(&mut run, &mut session, &mut store, CHAPTER_1);
    assert_eq!(session.report.start.as_deref(), Some("fresh"));
    assert_eq!(int(&run.vm, run.vm.field(control(&run.vm), "activated")), 1);
    // The save asked: the slot's and the header's requests queued.
    let c = control(&run.vm);
    let queued = run
        .vm
        .field(c, "save requests")
        .and_then(|s| run.vm.set(s).map(<[_]>::len));
    assert_eq!(queued, Some(2), "the level initializer's save");
    let mut active_steps = 0;
    let mut states = Vec::new();
    while !(idle(&run.vm) && !session.writing() && store.pending.is_none()) {
        assert!(run.steps < 600, "the save system never went idle");
        step(&mut run, &mut session, &mut store, CHAPTER_1);
        let s = run.world.save.get();
        if s == label::ACTIVE {
            active_steps += 1;
        }
        if states.last() != Some(&s) {
            states.push(s);
        }
    }
    println!(
        "idle at step {}; states {states:?}; active {active_steps} steps; requests {:?}",
        run.steps, session.report.requests
    );
    // The slot and the header, then the finalize the save system queues
    // with the header again behind it (`Save - Add Finalize Request` asks a
    // global save whose first request finalizes).
    assert_eq!(
        session.report.requests,
        [
            (Asked::Save, label::INACTIVE),
            (Asked::Save, label::INACTIVE),
            (Asked::Finalize, label::INACTIVE),
            (Asked::Save, label::INACTIVE)
        ]
    );
    // Each save read active while the store had it (31 pumps each).
    assert!(active_steps >= 3 * 30, "{active_steps}");
    assert_eq!(store.rows.len(), 3, "one write per file saved");
    let (layout, values) = store.rows.last().unwrap().clone();
    let l = Layout::read(&run.vm).unwrap();
    assert_eq!(layout, l.id());
    assert_eq!(values.adventures, l.adventures);
    assert_eq!(
        values.adventures[..2],
        ["Level_213_Tutorial", "Level_214_Tutorial_2"]
    );
    assert_eq!(values.params.len(), l.params.len(), "every param kept");
    for p in &l.params {
        assert_eq!(
            values.params[&p.name].len(),
            p.fields as usize,
            "{}",
            p.name
        );
    }
    assert_eq!(values.params["flags - player"][0], 1, "the slot in use");
    assert!(values.params["level flags"].iter().all(|f| f & 1 == 0));
    assert_eq!(story_save::resume(Some(&values)), Resume::ChapterOne);
    println!(
        "{} params ({} slot words, {} header words), layout {layout}; {:.1} s; load at end: {}",
        l.params.len(),
        l.slot_words,
        l.header_words,
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// The params whose fields hold anything but 0, as `name: fields`.
fn listed(values: &SaveValues) -> BTreeMap<String, Vec<(usize, i64)>> {
    values
        .params
        .iter()
        .filter_map(|(n, f)| {
            let set: Vec<(usize, i64)> = f
                .iter()
                .enumerate()
                .filter(|(_, v)| **v != 0)
                .map(|(i, v)| (i, *v))
                .collect();
            (!set.is_empty()).then(|| (n.clone(), set))
        })
        .collect()
}

/// AS.38 and AU.4: chapter 1's end, as `giants_level_end` runs it, marks
/// its adventure done in the save's memory (the level end saves nothing
/// itself); the words carried into level 214's run, that run's start loads
/// them and saves, and the store holds chapter 1 done (every nonzero value
/// listed). A fresh run of chapter 1 for that profile goes where the
/// original's load goes, level 214, its start loading the stored save (the
/// adventure's `complete` filled 1 on the VM, the current adventure
/// chapter 1's).
#[test]
#[ignore = "reads the Giants dump"]
fn chapter_1s_end_saves_it_done_and_a_load_goes_on_to_214() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let setup = Setup {
        interface: true,
        ..Setup::default()
    };
    let (mut run, _found) = run_on_with(Mode::Play, setup);
    let mut session = Session::new(Some(HOST.into()));
    let mut store = Store::default();
    for _ in 0..2 {
        step(&mut run, &mut session, &mut store, CHAPTER_1);
    }
    // The current adventure is chapter 1's, as the switch would name it.
    let title = run.vm.global_peek(
        run.vm
            .program()
            .global_named("Current Adventure Title")
            .unwrap(),
    );
    let named = match title {
        Some(Value::Object(o)) => run.vm.program().name_of(o),
        _ => None,
    };
    assert_eq!(named, Some("Level_213_Tutorial"));
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..120 {
        step(&mut run, &mut session, &mut store, CHAPTER_1);
    }
    let first_saves = store.rows.len();
    assert!(first_saves >= 2, "the start's save");
    run.put(Vec3::from_array(END.0), END.1);
    let (ender, _) = run.placed("Temp_Level_Ender");
    assert!(run.world.wake(ender));
    let woken = run.steps;
    while run.world.level_change().is_none() {
        assert!(
            run.steps < woken + 2000,
            "no level change by step {}",
            run.steps
        );
        step(&mut run, &mut session, &mut store, CHAPTER_1);
    }
    assert!(level_end::recorded(&run.vm).is_some());
    assert_eq!(run.world.level_change().unwrap().name, LEVEL_214);
    let l = Layout::read(&run.vm).unwrap();
    let words = story_save::words_of(&run.vm, File::Slot).unwrap();
    let memory = l.decode(File::Slot, &words);
    assert_eq!(memory["level flags"][0] & 1, 1, "chapter 1 not marked done");
    assert_eq!(
        store.rows.len(),
        first_saves,
        "the level end saved: the original's doesn't"
    );
    // Its words are what the session carries into the next run, as the
    // original's memory carries them.
    assert_eq!(
        session.live.as_ref().map(|l| &l.slot),
        Some(&words),
        "the session carries other words"
    );
    let chapter_1_steps = run.steps;

    let (mut next, _found) = run_on_level(Mode::Play, LEVEL_214, setup);
    for _ in 0..2 {
        step(&mut next, &mut session, &mut store, LEVEL_214);
    }
    assert_eq!(session.report.start.as_deref(), Some("carried"));
    while !(idle(&next.vm) && !session.writing() && store.pending.is_none()) {
        assert!(next.steps < 600, "level 214's save never went idle");
        step(&mut next, &mut session, &mut store, LEVEL_214);
    }
    let (layout, saved) = store.rows.last().unwrap().clone();
    assert_eq!(layout, l.id(), "214's run read another layout");
    assert!(story_save::done(&saved, "Level_213_Tutorial"));
    assert!(!story_save::done(&saved, "Level_214_Tutorial_2"));
    println!("chapter 1's end, saved at level 214's start, nonzero values:");
    for (name, fields) in listed(&saved) {
        println!("  {name}: {fields:?}");
    }
    assert_eq!(
        story_save::resume(Some(&saved)),
        Resume::Level(LEVEL_214.into())
    );

    // A later session: the stored save, loaded at a fresh run's start.
    let mut later = Session::new(Some(HOST.into()));
    later.stored = Some((layout.clone(), saved.clone()));
    let (mut fresh, _found) = run_on_with(Mode::Play, setup);
    let mut store = Store::default();
    step(&mut fresh, &mut later, &mut store, CHAPTER_1);
    assert_eq!(later.report.start.as_deref(), Some("stored"));
    assert!(later.report.loaded);
    let complete = |vm: &Vm, title: &str| {
        let lpd = vm
            .global_peek(vm.program().global_named("level progression data").unwrap())
            .unwrap();
        let ads = vm.field(lpd, "adventure data").unwrap();
        vm.set(ads)
            .unwrap()
            .iter()
            .find(|a| match vm.field(**a, "title").map(|t| vm.value(t)) {
                Some(Value::Object(o)) => vm.program().name_of(o) == Some(title),
                _ => false,
            })
            .map(|a| int(vm, vm.field(*a, "complete")))
            .unwrap()
    };
    assert_eq!(complete(&fresh.vm, "Level_213_Tutorial"), 1);
    assert_eq!(complete(&fresh.vm, "Level_214_Tutorial_2"), 0);
    let loaded = l.decode(
        File::Slot,
        &story_save::words_of(&fresh.vm, File::Slot).unwrap(),
    );
    for (name, fields) in &loaded {
        assert_eq!(&saved.params[name], fields, "{name} didn't load");
    }
    println!(
        "chapter 1 {chapter_1_steps} steps, 214 {} steps; {:.1} s; load at end: {}",
        next.steps,
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// The host's packing against the scripts' own: every slot and header
/// param's fields written by `Set Save Param` on the VM read back alike off
/// the words by `Layout::decode`, and every field `Layout::encode` writes
/// read alike by `Save - Get Save Param`, over values reaching each field's
/// ends (signed and unsigned, the 32-bit ones, the fields crossing words).
#[test]
#[ignore = "reads the Giants dump"]
fn the_hosts_packing_reads_as_the_scripts_do() {
    let (mut run, _found) = run_on_with(Mode::Play, Setup::default());
    run.step_play().unwrap();
    let l = Layout::read(&run.vm).unwrap();
    let csp = run
        .vm
        .global_peek(
            run.vm
                .program()
                .global_named("console save params")
                .unwrap(),
        )
        .unwrap();
    let value_at = |p: &story_save::Param, i: u32| -> i64 {
        let n = i64::from(p.bits.min(31));
        let max = if p.signed {
            (1i64 << (n - 1)) - 1
        } else {
            (1i64 << n) - 1
        };
        let min = if p.signed { -(1i64 << (n - 1)) } else { 0 };
        match i % 4 {
            0 => max,
            1 => min,
            2 => (max + min) / 2 + 1,
            _ => i64::from(i % 2),
        }
    };
    let mut checked = 0;
    // The scripts write, the host reads.
    for p in &l.params {
        let param = run.vm.field(csp, &p.name).unwrap();
        for i in 0..p.fields {
            let v = value_at(p, i);
            run.with_host(|vm, host| {
                vm.call_with(
                    None,
                    "Set Save Param",
                    &[
                        ("val", Value::Int(v as i32)),
                        ("field index", Value::Int(i as i32)),
                        ("save param", param),
                    ],
                    &[],
                    host,
                )
            })
            .unwrap_or_else(|h| panic!("{h}"));
        }
    }
    for file in [File::Slot, File::Header] {
        let decoded = l.decode(file, &story_save::words_of(&run.vm, file).unwrap());
        for p in l
            .params
            .iter()
            .filter(|p| p.header == (file == File::Header))
        {
            for i in 0..p.fields {
                assert_eq!(
                    decoded[&p.name][i as usize],
                    value_at(p, i),
                    "{} [{i}]",
                    p.name
                );
                checked += 1;
            }
        }
    }
    // The host writes, the scripts read.
    for file in [File::Slot, File::Header] {
        let mut values = BTreeMap::new();
        for p in l
            .params
            .iter()
            .filter(|p| p.header == (file == File::Header))
        {
            values.insert(
                p.name.clone(),
                (0..p.fields)
                    .map(|i| value_at(p, i + 1))
                    .collect::<Vec<_>>(),
            );
        }
        let mut words = vec![0; l.words(file)];
        l.encode(file, &values, &mut words);
        let csp_field = match file {
            File::Slot => "save val set",
            File::Header => "header save val set",
        };
        let s = run.vm.field(csp, csp_field).unwrap();
        let elems = run.vm.set(s).unwrap().to_vec();
        assert_eq!(elems.len(), words.len());
        for (e, w) in elems.iter().zip(&words) {
            assert!(run.vm.put(*e, Value::Int(*w)));
        }
        for p in l
            .params
            .iter()
            .filter(|p| p.header == (file == File::Header))
        {
            let param = run.vm.field(csp, &p.name).unwrap();
            for i in 0..p.fields {
                let (_, back) = run
                    .with_host(|vm, host| {
                        vm.call_with(
                            None,
                            "Save - Get Save Param",
                            &[("field index", Value::Int(i as i32)), ("save param", param)],
                            &["val"],
                            host,
                        )
                    })
                    .unwrap_or_else(|h| panic!("{h}"));
                let got = int(&run.vm, back[0]);
                assert_eq!(
                    got,
                    value_at(p, i + 1),
                    "{} [{i}] read by the scripts",
                    p.name
                );
                checked += 1;
            }
        }
    }
    println!(
        "{checked} fields checked both ways over {} params",
        l.params.len()
    );
}
