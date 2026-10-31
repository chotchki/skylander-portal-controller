//! What AH.8's route needed of the VM (openspec AH.28), each rule on
//! scripts built in code: `undefined`'s set members reading as an empty
//! set's (the cannon's rubble plays `undefined.setvariant.next` as it
//! lands), and a placement's tags naming nothing or one object (the spike
//! gate's `close_set` names `null`, its `open_set` the one placement whose
//! removal opens it).

use super::init::{actor, program_and_objects, script};
use super::*;

/// `set reference <lhs> = <rhs>`.
fn setref(w: &mut Writer<'_>, lhs: &[El<'_>], rhs: &[El<'_>]) {
    let l = w.value(lhs);
    let r = w.reference(rhs, None);
    let o = w.op(Class::SetReference, "set reference");
    w.refer(o, 0x1C, l);
    w.refer(o, 0x20, r);
    w.push(o);
}

const UNDEFINED: El<'static> = El::Member("abstractscriptgroup", "undefined");

/// [`Bare`], answering a play by recording what it was asked to play.
#[derive(Default)]
struct Plays(Vec<Option<Value>>);

impl Host for Plays {
    fn get(&mut self, _: &Access) -> Result<Value, HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn set(&mut self, _: &Access, _: Value) -> Result<(), HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn reset(&mut self, _: &Access) -> Result<(), HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn draw(&mut self) -> u32 {
        0
    }
    fn op(&mut self, call: &OpCall) -> Result<Answer, HostFault> {
        let lhs = call
            .operands
            .iter()
            .find(|(n, _)| *n == "_LHS")
            .and_then(|(_, v)| *v);
        self.0.push(lhs);
        Ok(Answer::Done)
    }
}

/// `undefined` holds no set, so its `setvariant` members read as an empty
/// set's: a count of 0, no element. A play of its `next` plays nothing and
/// the tick goes on, as the debris of the cannon's rubble wall does as it
/// lands.
#[test]
fn undefineds_set_members_read_as_an_empty_sets() {
    let p = program(|w| {
        let (count, after, some) = (var(w, "count"), var(w, "after"), var(w, "some"));
        let next = w.variable("next", "ScriptReference", None);
        idle(
            w,
            |_| {},
            |w| {
                let seven = w.int(7);
                w.set(&[El::Local(count)], seven);
                let n = w.rhs(INT, &[UNDEFINED, El::Member("setvariant", "count")]);
                w.set(&[El::Local(count)], n);
                setref(
                    w,
                    &[El::Local(next)],
                    &[UNDEFINED, El::Member("setvariant", "next")],
                );
                let c = w.check_ref(
                    &[El::Local(next)],
                    5,
                    &[El::Member("abstractscriptgroup", "null")],
                );
                w.inc(&[El::Local(some)]);
                w.end_block(c);
                let play = w.start_sequence(&[UNDEFINED, El::Member("setvariant", "next")]);
                w.end_block(play);
                w.inc(&[El::Local(after)]);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Debris_Gate", 0);
    let mut host = Plays::default();
    vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "count"), Some(Value::Int(0)), "an empty set's");
    assert_eq!(vm.local(id, "some"), None, "no element");
    assert_eq!(host.0, vec![Some(Value::Null)], "a play of nothing");
    assert_eq!(
        vm.local(id, "after"),
        Some(Value::Int(1)),
        "the tick went on"
    );
}

/// A set reference tag naming `null` holds nothing: the gate's `close_set`
/// is found, its count reads through nothing and its block is skipped. One
/// naming a single placement holds a set of it alone: the gate's
/// `open_set` counts 1 and its first is that placement, so a check of its
/// members' removal reads it.
#[test]
fn a_set_reference_tag_naming_null_or_one_object() {
    let (mut gate, mut target) = (None, None);
    let (p, obj) = program_and_objects(|w| {
        let a = actor(w);
        let t = actor(w);
        (gate, target) = (Some(a), Some(t));
        let nothing = w.reference(&[El::Member("abstractscriptgroup", "null")], None);
        let one = w.reference(&[El::Local(t)], None);
        w.tags(
            a,
            &[
                ("close_set", "ScriptSetReference", "tfbActorInfo", nothing),
                ("open_set", "ScriptSetReference", "tfbActorInfo", one),
            ],
        );
        let (closed, open) = (var(w, "closed"), var(w, "open"));
        let first = w.variable("first", "ScriptReference", None);
        script(
            w,
            "C:/tfb/Synthetic/Gate_Spikes.ai",
            |w| {
                let my = [El::Member("optoplevelbehavior", "my")];
                let (f, fop) =
                    super::init::find(w, "close_set", "ScriptSetReference", "tfbActorInfo", 1, &my);
                let zero = w.int(0);
                let c = w.check(
                    &[El::Local(fop), El::Member("setvariant", "count")],
                    4,
                    zero,
                );
                w.inc(&[El::Local(closed)]);
                w.end_block(c);
                w.close(f);
                let (f, fop) =
                    super::init::find(w, "open_set", "ScriptSetReference", "tfbActorInfo", 1, &my);
                let n = w.rhs(INT, &[El::Local(fop), El::Member("setvariant", "count")]);
                w.set(&[El::Local(open)], n);
                setref(
                    w,
                    &[El::Local(first)],
                    &[El::Local(fop), El::Member("setvariant", "first")],
                );
                w.close(f);
            },
            |_| {},
            |w| super::init::behaviour(w, |_| {}),
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    vm.name_object(obj.at(target.unwrap()), 0x47);
    let id = vm.add_at(
        HANDLE,
        "Spike_Gate_Wooden_Template(1)",
        0,
        obj.at(gate.unwrap()),
    );
    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "closed"), None, "nothing in close_set");
    assert_eq!(vm.local(id, "open"), Some(Value::Int(1)), "a set of one");
    assert_eq!(vm.local(id, "first"), Some(Value::Entity(0x47)));
}

/// The music controller's check the push blocks' tortoises stopped on
/// before AG.28 (openspec AG.165: `MusicController.ai` #1227, `Sound -
/// Ultra Sound`): a sound's `attenuation distance silent` read and written
/// on an op list built in code. With the camera's table (AG.28) it's a value
/// cell: read, its registered default, 0; written below its range, clamped
/// to 0; the tick going on. Without it, strict mode halts on it.
#[test]
fn a_sounds_silent_distance_is_a_value_cell() {
    let silent = [
        El::Member("optoplevelbehavior", "my"),
        El::Member("soundinfo", "attenuation distance silent"),
    ];
    let p = program(|w| {
        let (far, back, after) = (
            w.variable("far", "ValueInfo", Some("FloatMeasurement")),
            w.variable("back", "ValueInfo", Some("FloatMeasurement")),
            var(w, "after"),
        );
        idle(
            w,
            |_| {},
            |w| {
                let v = w.rhs("FloatMeasurement", &silent);
                w.set(&[El::Local(far)], v);
                let below = w.int(-3);
                w.set(&silent, below);
                let v = w.rhs("FloatMeasurement", &silent);
                w.set(&[El::Local(back)], v);
                w.inc(&[El::Local(after)]);
            },
        );
    });
    let mut config = Config::new(Mode::Strict);
    members::register(&mut config.members, members::CAMERA);
    let mut vm = Vm::new(p.clone(), config);
    let id = vm.add(HANDLE, "2_Push_Block_Art_Sheep", 0);
    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "far"), Some(Value::Float(0.0)));
    assert_eq!(vm.local(id, "back"), Some(Value::Float(0.0)), "clamped");
    assert_eq!(vm.local(id, "after"), Some(Value::Int(1)));
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "2_Push_Block_Art_Sheep", 0);
    let halt = vm.tick(id, &mut Bare).unwrap_err();
    assert!(matches!(halt.reason, Reason::UnmodelledMember(_)), "{halt}");
}
