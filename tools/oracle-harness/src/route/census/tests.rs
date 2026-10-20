//! The census on memory laid out in code ([`crate::actors::fixture`] and
//! [`crate::script::fixture`]), the disc's offsets taken from where the
//! fixture put each placement against a base of its heap.

use super::*;
use crate::actors::fixture::{Spec, World};
use crate::actors::{self};
use crate::script::fixture::{self as fx, Types as FxTypes};

const HEAP: u32 = 0x3000_0000;

fn actor(name: &str, offset: u32, script: Option<&str>) -> DiscActor {
    DiscActor {
        offset,
        name: name.to_owned(),
        script: script.map(|s| format!("C:/tfb/Content/Levels/Level_213/Scripts/{s}")),
        range: 0.0,
        position: [0.0; 3],
    }
}

/// A level: three actors on the game's list (they vote the section's base),
/// a trigger awake and fired, one asleep, a Chompy started on a behaviour
/// with variables of every kind, and its controller not started yet.
struct Level {
    w: World,
    disc: Disc,
    chompy: u32,
    ints: u32,
}

fn level() -> Level {
    let mut w = World::new();
    let t: FxTypes = fx::types(&mut w);
    let mut disc = Disc::default();
    for name in ["Sheep_1", "barrel_b_intact(1)", "pottery_2(5)"] {
        let a = w.actor(&Spec {
            name: name.into(),
            ..Spec::default()
        });
        disc.actors.push(actor(name, a.placement - HEAP, None));
    }
    // Fired: awake, its clock running. Asleep: inactive, never started.
    let fired = fx::placement(&mut w, "Setup_001_Trigger", None, 2.5, None);
    w.putf(fired + 0xD0, 20.0);
    disc.actors
        .push(actor("Setup_001_Trigger", fired - HEAP, None));
    let asleep = fx::placement(&mut w, "Activate_Norticus and Chompies", None, 0.0, None);
    w.put32(asleep + 0x54, 2);
    w.putf(asleep + 0xD0, 32.0);
    disc.actors.push(actor(
        "Activate_Norticus and Chompies",
        asleep - HEAP,
        Some("Activate_Norticus_Chompies.ai"),
    ));
    let hp = fx::int(&mut w, &t, "chompy hp", 3);
    let wait = fx::float(&mut w, &t, "windup time", 0.3);
    let target = fx::reference(&mut w, vtable::SCRIPT_REFERENCE, "target", 0);
    let inner = fx::int(&mut w, &t, "alerted", 0);
    let attention = fx::structure(&mut w, "attention parameters", &[inner]);
    let hit = fx::set(&mut w, "actors hit", &[]);
    let chompy = fx::placement(
        &mut w,
        "Enemy_Chompy(1)",
        Some("idle"),
        12.5,
        Some(&[hp, wait, target, attention, hit]),
    );
    disc.actors.push(actor(
        "Enemy_Chompy(1)",
        chompy - HEAP,
        Some("S2_Enemy_Chompy.ai"),
    ));
    let controller = fx::placement(&mut w, "Setup_001_Controller", None, 0.0, None);
    w.put32(controller + 0x54, 2);
    disc.actors.push(actor(
        "Setup_001_Controller",
        controller - HEAP,
        Some("Setup_001_Controller.ai"),
    ));
    Level {
        w,
        disc,
        chompy,
        ints: hp,
    }
}

fn err<T: std::fmt::Debug>(r: Result<T>) -> String {
    format!("{:#}", r.unwrap_err())
}

#[test]
fn the_disc_lists_names_once_and_picks_triggers_and_the_whitelist() {
    let mut disc = Disc::default();
    for (i, (name, script)) in [
        ("Setup_001_Trigger", None),
        ("Transition 7-1", None),
        (
            "Activate_Norticus and Chompies",
            Some("Activate_Norticus_Chompies.ai"),
        ),
        ("Enemy_Chompy(1)", Some("S2_Enemy_Chompy.ai")),
        ("Bird_template_Red(1)", Some("Bird_Ambient_new.ai")),
        ("Dup", Some("S2_Enemy_Chompy.ai")),
        ("Dup", Some("S2_Enemy_Chompy.ai")),
        ("", Some("S2_Enemy_Chompy.ai")),
    ]
    .into_iter()
    .enumerate()
    {
        disc.actors.push(actor(name, 0x100 * i as u32, script));
    }
    let once = disc.offsets();
    assert_eq!(once.len(), 5, "{once:?}");
    assert!(!once.contains_key("Dup") && !once.contains_key(""));
    assert_eq!(
        disc.triggers(),
        [
            "Activate_Norticus and Chompies",
            "Setup_001_Trigger",
            "Transition 7-1"
        ]
    );
    // The level's whitelist by script file name: the trigger's and the
    // Chompy's, not the bird's, and never a name listed twice.
    assert_eq!(
        disc.whitelisted(),
        ["Activate_Norticus and Chompies", "Enemy_Chompy(1)"]
    );
    assert_eq!(disc.script_file(0x300), Some("S2_Enemy_Chompy.ai"));
    assert_eq!(disc.script_file(0), None);
    // The route's whole reads: ROUTE first, then the whitelist's others, only
    // names the disc lists once.
    let names = whole_names(&disc);
    assert_eq!(
        names,
        [
            "Setup_001_Trigger",
            "Activate_Norticus and Chompies",
            "Enemy_Chompy(1)"
        ]
    );
    assert!(is_trigger("Pop_to_Camera_5") && is_trigger("trg_lvl_213_lush_peak"));
    assert!(!is_trigger("Enemy_Chompy(1)") && !is_trigger("Name_Zone(4)"));
    // WATCHED is ROUTE's less what doesn't ride the watch.
    assert!(WATCHED.iter().all(|n| ROUTE.contains(n)));
}

#[test]
fn briefs_read_every_placement_and_name_their_behaviours() {
    let mut l = level();
    let b = briefs(&mut l.w, HEAP, &l.disc).unwrap();
    assert_eq!(b.len(), l.disc.actors.len());
    let by = |n: &str| b.iter().find(|x| x.name == n).unwrap();
    let c = by("Enemy_Chompy(1)");
    assert_eq!(
        (
            c.activity_state,
            c.started,
            c.active_seconds,
            c.behaviour.as_deref(),
            c.script.as_deref()
        ),
        (1, true, 12.5, Some("idle"), Some("S2_Enemy_Chompy.ai"))
    );
    let t = by("Setup_001_Trigger");
    assert_eq!((t.activity_state, t.started, t.range), (1, false, 20.0));
    assert_eq!(t.behaviour, None);
    let s = by("Setup_001_Controller");
    assert_eq!(
        (s.activity_state, s.started, s.script.as_deref()),
        (2, false, Some("Setup_001_Controller.ai"))
    );
    // A behaviour pointer at anything but an OpUserBehavior is refused.
    let mut bad = level();
    let not = fx::named(&mut bad.w, vtable::VALUE_INFO, 0x24, "idle");
    bad.w.put32(bad.chompy + 0xAC, not);
    assert!(err(briefs(&mut bad.w, HEAP, &bad.disc)).contains("not an OpUserBehavior"));
    // A base that's off reads something that isn't a tfbActorInfo: refused,
    // naming the placement.
    let mut l = level();
    let e = err(briefs(&mut l.w, HEAP + 4, &l.disc));
    assert!(e.contains("Sheep_1") && e.contains("tfbActorInfo"), "{e}");
}

#[test]
fn a_census_votes_the_base_reads_the_triggers_and_the_placements_whole() {
    let mut l = level();
    let mut frame = actors::read(&mut l.w).unwrap();
    frame.flip = Some(77);
    let names = vec![
        "Enemy_Chompy(1)".to_owned(),
        "Setup_001_Controller".to_owned(),
    ];
    let c = read(&mut l.w, &frame, &l.disc, &names).unwrap();
    assert_eq!((c.flip, c.base.base, c.base.votes), (77, HEAP, 3));
    // Triggers: the fired one awake and clocked, the asleep one not fired.
    assert_eq!(c.fired("Setup_001_Trigger"), Some(true));
    assert_eq!(c.fired("Activate_Norticus and Chompies"), Some(false));
    assert_eq!(c.fired("Enemy_Chompy(1)"), None);
    // The Chompy whole, every variable's value; the controller not started.
    let w = &c.whole[0];
    assert_eq!(
        (w.name.as_str(), w.placement),
        ("Enemy_Chompy(1)", l.chompy)
    );
    let vars = w.variables.as_ref().unwrap();
    let v = |n: &str| vars.iter().find(|x| x.name == n).unwrap().value.clone();
    assert_eq!(v("chompy hp"), Some(Value::Number(script::Number::Int(3))));
    assert_eq!(
        v("windup time"),
        Some(Value::Number(script::Number::Float(0.3)))
    );
    assert_eq!(
        v("target"),
        Some(Value::Reference {
            to: None,
            name: None
        })
    );
    assert_eq!(
        v("attention parameters"),
        Some(Value::Structure { fields: 1 })
    );
    assert_eq!(v("actors hit"), Some(Value::Set { count: 0 }));
    assert_eq!(c.whole[1].variables, None);
    assert_eq!(
        c.brief("Enemy_Chompy(1)").unwrap().behaviour.as_deref(),
        Some("idle")
    );
    // A value that won't read keeps its reason and the rest still read.
    let mut l2 = level();
    l2.w.put32(l2.ints + 0x1C, 0x3000_0010);
    let frame2 = actors::read(&mut l2.w).unwrap();
    let c2 = read(&mut l2.w, &frame2, &l2.disc, &names[..1]).unwrap();
    let hp = c2.whole[0].variables.as_ref().unwrap();
    let hp = hp.iter().find(|x| x.name == "chompy hp").unwrap();
    assert!(hp.value.is_none() && hp.error.is_some(), "{hp:?}");
    // A name the disc doesn't list once, or one at another's placement, is
    // refused.
    assert!(err(read(&mut l.w, &frame, &l.disc, &["Nobody".to_owned()])).contains("Nobody"));
    let e = err(whole(
        &mut l.w,
        &mut Types::default(),
        "Enemy_Chompy(2)",
        l.chompy,
    ));
    assert!(e.contains("Enemy_Chompy(1)"), "{e}");
}

#[test]
fn what_changed_between_two_reads_and_the_scripts_that_started() {
    let mut l = level();
    let frame = actors::read(&mut l.w).unwrap();
    let before = read(&mut l.w, &frame, &l.disc, &[]).unwrap();
    // The trigger wakes the controller, which starts on a behaviour; the
    // Chompy moves on to another.
    let controller = HEAP + l.disc.offsets()["Setup_001_Controller"];
    l.w.put32(controller + 0x54, 1);
    l.w.put32(controller + 0x1C, 0x8);
    l.w.putf(controller + 0xB4, 0.1);
    let wait = fx::named(&mut l.w, vtable::USER_BEHAVIOR, 0x28, "Wait For Minion");
    l.w.put32(controller + 0xAC, wait);
    let alerted = fx::named(&mut l.w, vtable::USER_BEHAVIOR, 0x28, "alerted");
    l.w.put32(l.chompy + 0xAC, alerted);
    let after = read(&mut l.w, &frame, &l.disc, &[]).unwrap();
    let changes = after.changes_since(&before);
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert_eq!(changes[0].name, "Enemy_Chompy(1)");
    assert_eq!(changes[0].from.2.as_deref(), Some("idle"));
    assert_eq!(changes[0].to.2.as_deref(), Some("alerted"));
    assert_eq!(changes[1].name, "Setup_001_Controller");
    assert_eq!(changes[1].from, (2, false, None));
    assert_eq!(changes[1].to, (1, true, Some("Wait For Minion".to_owned())));
    // Of the scripts, only the controller's started between them.
    let scripts = RouteRecord::scripts_between(&before, &after);
    assert_eq!(
        scripts,
        [(
            "Setup_001_Controller.ai".to_owned(),
            vec!["Setup_001_Controller".to_owned()]
        )]
        .into_iter()
        .collect()
    );
    assert!(RouteRecord::scripts_between(&after, &after).is_empty());
}

fn wake(state: u32, behaviour: u32, secs: f32) -> Wake {
    Wake {
        activity_state: state,
        behaviour,
        active_seconds: secs,
    }
}

#[test]
fn what_fired_record_by_record_in_the_order_it_happened() {
    let names = vec!["Setup_001_Trigger".to_owned(), "Enemy_Chompy(1)".to_owned()];
    let named: BTreeMap<u32, String> = [(0x10, "idle".to_owned()), (0x20, "alerted".to_owned())]
        .into_iter()
        .collect();
    let records = vec![
        // The baseline: the Chompy already running, nothing reported.
        (2, 100, vec![wake(2, 0, 0.0), wake(1, 0x10, 5.0)]),
        (3, 101, vec![wake(2, 0, 0.0), wake(1, 0x10, 5.02)]),
        // The trigger wakes, and in the same record the Chompy's behaviour
        // moves: both at flip 4, the trigger first by the names' order.
        (4, 102, vec![wake(1, 0, 0.016), wake(1, 0x20, 5.03)]),
        // A pointer nobody named reads as itself; the trigger sleeps again.
        (6, 103, vec![wake(2, 0, 0.033), wake(1, 0x30, 5.05)]),
    ];
    let f = fired(&names, &records, &named).unwrap();
    let got: Vec<(u64, u32, &str, &str)> = f
        .iter()
        .map(|e| (e.flip, e.game_frame, e.name.as_str(), e.what.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (4, 102, "Setup_001_Trigger", "active"),
            (4, 102, "Setup_001_Trigger", "state 2 -> 1"),
            (4, 102, "Enemy_Chompy(1)", "behaviour idle -> alerted"),
            (6, 103, "Setup_001_Trigger", "state 1 -> 2"),
            (6, 103, "Enemy_Chompy(1)", "behaviour alerted -> 0x00000030"),
        ]
    );
    assert!(fired(&names, &[], &named).unwrap().is_empty());
    let short = vec![(2, 100, vec![wake(2, 0, 0.0)])];
    assert!(err(fired(&names, &short, &named)).contains("1 wake states for 2 names"));
}

#[test]
fn a_route_record_is_written_and_read_back() {
    use super::super::{Look, Seen, Step};
    let seen = |flip: u64, x: f64| Seen {
        flip,
        game_frame: 5_960 + flip as u32,
        pivot: [x, 43.4, -93.4],
        facing: 90.0,
        velocity: [0.0; 3],
        view: 15.0,
        eye: [0.0; 3],
    };
    let plan = super::super::RoutePlan {
        steps: vec![
            Step::Via {
                to: super::super::Point { x: 20.0, z: -93.4 },
                radius: 1.0,
            },
            Step::Read {
                label: "anchor".into(),
            },
        ],
        save: true,
    };
    let looks = vec![
        Look {
            seen: seen(2, 10.0),
            step: 0,
            did: "walk".into(),
        },
        Look {
            seen: seen(10, 12.0),
            step: 0,
            did: "walk".into(),
        },
        Look {
            seen: seen(18, 19.5),
            step: 1,
            did: "read anchor".into(),
        },
        Look {
            seen: seen(18, 19.5),
            step: 2,
            did: "done".into(),
        },
    ];
    let steps = RouteRecord::steps(&looks, &plan);
    assert_eq!(steps.len(), 3);
    assert_eq!(
        (
            steps[0].index,
            steps[0].first_flip,
            steps[0].last_flip,
            steps[0].looks
        ),
        (0, 2, 18, 2)
    );
    assert_eq!((steps[0].from[0], steps[0].to[0]), (10.0, 19.5));
    assert!(steps[0].step.starts_with("Via"));
    assert_eq!(steps[2].step, "done");
    let record = RouteRecord {
        identity: "oracle sha256=fe45b32f…".into(),
        route: plan.to_ron(),
        steps,
        fired: vec![Fired {
            name: "Setup_001_Trigger".into(),
            flip: 120,
            game_frame: 11_447,
            what: "state 2 -> 1".into(),
        }],
        changes: vec![("f0".into(), "anchor".into(), Vec::new())],
        scripts: [(
            "Setup_001_Controller.ai".to_owned(),
            vec!["Setup_001_Controller".to_owned()],
        )]
        .into_iter()
        .collect(),
        anchor: Some(AnchorReads {
            label: "anchor".into(),
            flip: 18,
            game_frame: 5_978,
            pivot: [19.5, 43.4, -93.4],
            facing: 90.0,
            files: vec!["actors-anchor.json".into(), "census-anchor.json".into()],
            seed: Some(10_554),
            drawn: Some(133_619),
            fired: vec!["Setup_001_Trigger".into()],
        }),
    };
    let dir = tempfile::tempdir().unwrap();
    record.write(dir.path()).unwrap();
    assert_eq!(RouteRecord::read(dir.path()).unwrap(), record);
    assert!(RouteRecord::read(&dir.path().join("nowhere")).is_err());
}
