//! What AS.11's every-level run needed of the VM (openspec AS.27-AS.35),
//! each rule on scripts built in code: the level audio controllers' members,
//! `for each` over `undefined`, a tag holding a roll, a random colour, a
//! parameter outside a call, a write to `undefined`'s count and the slider
//! check's index.

use super::*;

const MY: El<'static> = El::Member("optoplevelbehavior", "my");

/// A host answering a template's `clonedsequence.clones` with the copies it
/// holds, and nothing else.
struct Clones(Vec<Value>);

impl Host for Clones {
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
    fn elements(&mut self, at: &Access) -> Result<Vec<Value>, HostFault> {
        if at.member == MemberId::named("clonedsequence", "clones") {
            Ok(self.0.clone())
        } else {
            Err(HostFault::Unmodelled)
        }
    }
}

/// AS.27, `MusicController.ai`'s shapes on a list built in code: the
/// sound system's reverb set heading the path (`Environmental Sound - Set
/// Reverb`) reads back as written, its amount clamped to 100, a sound's
/// global limit to 31; the ambience's check whether the one it faded out is
/// a copy of the one asked for holds only when the host's copies hold it.
/// Without the table, strict mode halts on the first of them.
#[test]
fn a_level_audio_controllers_reverb_limit_and_copies() {
    let reverb = [El::Member("soundinfo", "reverb setting 1")];
    let amount = [El::Member("soundinfo", "reverb amount 1")];
    let limit = [MY, El::Member("soundinfo", "global limit")];
    let p = program(|w| {
        let (setting, pct, cap, copy) = (
            var(w, "setting"),
            w.variable("pct", "ValueInfo", Some("FloatMeasurement")),
            var(w, "cap"),
            var(w, "copy"),
        );
        idle(
            w,
            |_| {},
            |w| {
                let three = w.int(3);
                w.set(&reverb, three);
                let v = w.rhs(INT, &reverb);
                w.set(&[El::Local(setting)], v);
                let high = w.int(250);
                w.set(&amount, high);
                let v = w.rhs("FloatMeasurement", &amount);
                w.set(&[El::Local(pct)], v);
                let over = w.int(40);
                w.set(&limit, over);
                let v = w.rhs(INT, &limit);
                w.set(&[El::Local(cap)], v);
                let c = w.check_in(&[MY, El::Member("clonedsequence", "clones")], 0, &[MY]);
                w.inc(&[El::Local(copy)]);
                w.end_block(c);
            },
        );
    });
    let config = || {
        let mut c = Config::new(Mode::Strict);
        members::register(&mut c.members, members::LEVELS);
        c
    };
    // Whether it is a copy is the host's list's to say.
    for (copies, held) in [
        (vec![], None),
        (vec![Value::Entity(7)], None),
        (
            vec![Value::Entity(7), Value::Entity(HANDLE)],
            Some(Value::Int(1)),
        ),
    ] {
        let mut vm = Vm::new(p.clone(), config());
        let id = vm.add(HANDLE, "ctl_lvl_021_level_audio", 0);
        vm.tick(id, &mut Clones(copies))
            .unwrap_or_else(|h| panic!("{h}"));
        assert_eq!(vm.local(id, "setting"), Some(Value::Int(3)));
        assert_eq!(vm.local(id, "pct"), Some(Value::Float(100.0)), "clamped");
        assert_eq!(vm.local(id, "cap"), Some(Value::Int(31)), "clamped");
        assert_eq!(vm.local(id, "copy"), held);
    }
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "ctl_lvl_021_level_audio", 0);
    let halt = vm.tick(id, &mut Clones(vec![])).unwrap_err();
    assert!(matches!(halt.reason, Reason::UnmodelledMember(_)), "{halt}");
    assert_eq!(halt.position, 9, "the first write, {halt}");
}

/// AS.28, the spike floors' controller's STARTUP on a list built in code: a
/// `for each` over `undefined` (offset 3, as the controller's first) skips
/// its block as one over an empty set does, and the tick goes on.
#[test]
fn a_for_each_over_undefined_skips_as_an_empty_sets_does() {
    let p = program(|w| {
        let (inside, after) = (var(w, "inside"), var(w, "after"));
        let empty = w.variable("empty", "ScriptSet", Some("ValueInfo"));
        idle(
            w,
            |_| {},
            |w| {
                let undefined = w.stack(
                    StackClass::Set,
                    &[El::Member("abstractscriptgroup", "undefined")],
                );
                let none = w.stack(StackClass::Set, &[El::Local(empty)]);
                for set in [undefined, none] {
                    let three = w.int(3);
                    let (at, fe) = w.open(Class::ForEach, "for each");
                    w.refer(fe, 0x20, set);
                    w.refer(fe, 0x28, three);
                    w.inc(&[El::Local(inside)]);
                    w.end_loop(at);
                }
                w.inc(&[El::Local(after)]);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "SpikeFloorController", 0);
    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "inside"), None, "neither block ran");
    assert_eq!(
        vm.local(id, "after"),
        Some(Value::Int(1)),
        "the tick went on"
    );
}

/// AS.30, `Setup_003_Controller`'s profile on a list built in code: its
/// script load reads 0, so a peak kept only when it's over the last one
/// stays as it was, and an average's total stays 0; without the table
/// strict mode halts on it.
#[test]
fn a_placements_script_load_reads_0() {
    let load = [MY, El::Member("actorinfo", "script load")];
    let p = program(|w| {
        let float = Some("FloatMeasurement");
        let (peak, total, after) = (
            w.variable("peak", "ValueInfo", float),
            w.variable("total", "ValueInfo", float),
            var(w, "after"),
        );
        idle(
            w,
            |_| {},
            |w| {
                let pk = w.rhs("FloatMeasurement", &[El::Local(peak)]);
                let c = w.check(&load, 4, pk);
                let l = w.rhs("FloatMeasurement", &load);
                w.set(&[El::Local(peak)], l);
                w.end_block(c);
                let t = w.rhs("FloatMeasurement", &[El::Local(total)]);
                let l = w.rhs("FloatMeasurement", &load);
                let sum = w.expr(t, Some((0, l)));
                w.set(&[El::Local(total)], sum);
                w.inc(&[El::Local(after)]);
            },
        );
    });
    let mut config = Config::new(Mode::Strict);
    members::register(&mut config.members, members::LEVELS);
    let mut vm = Vm::new(p.clone(), config);
    let id = vm.add(HANDLE, "Setup_003_Controller", 0);
    for _ in 0..3 {
        vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    }
    // Read, the local holds its default; set, it would hold the load's
    // float.
    assert_eq!(vm.local(id, "peak"), Some(Value::Int(0)), "never over");
    assert_eq!(vm.local(id, "total"), Some(Value::Float(0.0)));
    assert_eq!(vm.local(id, "after"), Some(Value::Int(3)));
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Setup_003_Controller", 0);
    let halt = vm.tick(id, &mut Bare).unwrap_err();
    assert!(matches!(halt.reason, Reason::UnmodelledMember(_)), "{halt}");
}

/// A clock placement whose tags are `time` = `value`, found every tick
/// (filter 1, as challenge 49's clocks find theirs) into `got`.
fn tagged_clock(value: impl FnOnce(&mut Writer<'_>) -> Location) -> (Arc<Program>, Obj) {
    let mut clock = None;
    let (p, obj) = super::init::program_and_objects(|w| {
        let a = super::init::actor(w);
        clock = Some(a);
        let v = value(w);
        w.tags(a, &[("time", "ValueInfo", INT, v)]);
        let got = var(w, "got");
        super::init::script(
            w,
            "C:/tfb/Synthetic/Challenge_Clock.ai",
            |_| {},
            |_| {},
            |w| {
                super::init::behaviour(w, |w| {
                    let (f, fop) = super::init::find(w, "time", "ValueInfo", INT, 1, &[MY]);
                    let t = w.rhs(INT, &[El::Local(fop)]);
                    w.set(&[El::Local(got)], t);
                    w.close(f);
                })
            },
        );
    });
    (p, obj.at(clock.unwrap()))
}

/// AS.33, challenge 49's clocks on lists built in code: a tag holding a
/// roll (`random(30)`, under an expression as the disc writes it) reads a
/// draw of the session's generator taken as the tag is found, as the roll
/// reads on a right side; one holding an expression of literals its value.
/// One reading a path stops as unsupported.
#[test]
fn a_tag_holding_a_roll_reads_a_draw_as_it_is_found() {
    let (p, at) = tagged_clock(|w| {
        let roll = w.literal(INT, 30, true);
        w.expr(roll, None)
    });
    for seed in [5489, 77, 2026] {
        let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
        let id = vm.add_at(HANDLE, "Challenge_Clock(2)", 0, at);
        let mut host = Rolling(Generator::seeded(seed));
        vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
        let first = Generator::seeded(seed).next_u32();
        assert_eq!(
            vm.local(id, "got"),
            random_literal(Value::Int(30), first),
            "seed {seed}"
        );
        // Drawn once, as it's made: a second find reads the same.
        vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
        assert_eq!(vm.local(id, "got"), random_literal(Value::Int(30), first));
    }
    let (p, at) = tagged_clock(|w| {
        let (two, three) = (w.int(2), w.int(3));
        w.expr(two, Some((0, three)))
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add_at(HANDLE, "Challenge_Clock(2)", 0, at);
    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "got"), Some(Value::Int(5)));
    let (p, at) = tagged_clock(|w| w.rhs(INT, &[MY, El::Member("system", "script timer")]));
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add_at(HANDLE, "Challenge_Clock(2)", 0, at);
    let halt = vm.tick(id, &mut Bare).unwrap_err();
    assert_eq!(
        halt.reason,
        Reason::Unsupported("a tag whose value reads a path".into()),
        "{halt}"
    );
}

/// A host recording each world op's `_RHS` operand as read, finding no
/// target.
#[derive(Default)]
struct Spawns(Vec<Option<Value>>);

impl Host for Spawns {
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
        let rhs = call
            .operands
            .iter()
            .find(|(n, _)| *n == "_RHS")
            .and_then(|(_, v)| *v);
        self.0.push(rhs);
        Ok(Answer::Skip)
    }
}

/// AS.32, chapter 1's cutscene manager and the title's text keyframer on a
/// list built in code: a spawn at a sprite's origin is asked at nothing, a
/// write through the text colour converter goes nowhere, and the tick goes
/// on. Without the table strict mode halts on the first.
#[test]
fn a_sprites_origin_and_text_converter_read_nothing() {
    let p = program(|w| {
        let after = var(w, "after");
        idle(
            w,
            |_| {},
            |w| {
                let zero = w.int(0);
                let spawn = w.spawn(
                    &[MY],
                    &[MY, El::Member("spritematrixmeasurement", "origin")],
                    zero,
                );
                w.end_block(spawn);
                let l = w.value(&[
                    El::Member("spriteinfo", "text color converter"),
                    El::Member("textcolorconverter", "text sprite"),
                ]);
                let r = w.reference(&[El::Member("abstractscriptgroup", "null")], None);
                let o = w.op(Class::SetReference, "set reference");
                w.refer(o, 0x1C, l);
                w.refer(o, 0x20, r);
                w.push(o);
                w.inc(&[El::Local(after)]);
            },
        );
    });
    let mut config = Config::new(Mode::Strict);
    members::register(&mut config.members, members::LEVELS);
    let mut vm = Vm::new(p.clone(), config);
    let id = vm.add(HANDLE, "CutsceneHandler", 0);
    let mut host = Spawns::default();
    vm.tick(id, &mut host).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(host.0, vec![Some(Value::Null)], "spawned at nothing");
    assert_eq!(vm.local(id, "after"), Some(Value::Int(1)));
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "CutsceneHandler", 0);
    let halt = vm.tick(id, &mut Spawns::default()).unwrap_err();
    assert!(matches!(halt.reason, Reason::UnmodelledMember(_)), "{halt}");
}
