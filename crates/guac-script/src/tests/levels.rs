//! What AS.11's every-level run needed of the VM (openspec AS.27-AS.35),
//! each rule on scripts built in code: the level audio controllers' members,
//! `for each` over `undefined`, a tag holding a roll, a random colour, a
//! parameter outside a call, a write to `undefined`'s count, a suspended
//! placement and the walk passing it, and the order variables found before
//! a placement runs join it in (AS.35's maps' order being their hasher's).

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

/// AS.34, 003's definition on a list built in code: a random colour
/// literal (the file's word, alpha high: the eruption's dark colour's
/// bytes) rolls each byte, alpha's draw first, then blue's, green's and
/// red's, each up to its own value.
#[test]
fn a_random_colour_literal_rolls_each_byte() {
    let file = 0xFF14_1422u32;
    let p = program(|w| {
        let c = w.variable("c", "ValueInfo", Some("ColorMeasurement"));
        idle(
            w,
            |_| {},
            |w| {
                let roll = w.literal("ColorMeasurement", file, true);
                w.set(&[El::Local(c)], roll);
            },
        );
    });
    for seed in [5489, 9, 31_337] {
        let (vm, id) = {
            let mut vm = Vm::new(p.clone(), Config::new(Mode::Strict));
            let id = vm.add(HANDLE, "003_Definition", 0);
            vm.tick(id, &mut Rolling(Generator::seeded(seed)))
                .unwrap_or_else(|h| panic!("{h}"));
            (vm, id)
        };
        let mut g = Generator::seeded(seed);
        let d: [u32; 4] = std::array::from_fn(|_| g.next_u32() & 0x7FFF_FFFF);
        // In the VM's order, red high: red 0x22, green and blue 0x14,
        // alpha 0xFF.
        let want =
            ((d[3] % 0x23) << 24) | ((d[2] % 0x15) << 16) | ((d[1] % 0x15) << 8) | (d[0] % 0x100);
        assert_eq!(vm.local(id, "c"), Some(Value::Color(want)), "seed {seed}");
    }
}

/// AS.34, level 212's definition on a list built in code: a parameter op
/// outside any call, binding into no definition, binds nothing and the
/// tick goes on; one naming a definition still stops.
#[test]
fn a_parameter_outside_a_call_binds_nothing() {
    let build = |named: bool| {
        program(|w| {
            let after = var(w, "after");
            let def = w.op(Class::DefineMacro, "Camera - Define Camera");
            idle(
                w,
                |_| {},
                |w| {
                    let zero = w.int(0);
                    let o = w.op(Class::MacroParameter, "parameter");
                    w.text(o, 0x1C, "look from heading offset");
                    w.class_word(o, 0x20, "ValueInfo");
                    w.word(o, 0x28, u32::MAX);
                    w.refer(o, 0x30, zero);
                    if named {
                        w.refer(o, 0x38, def);
                    }
                    w.push(o);
                    w.inc(&[El::Local(after)]);
                },
            );
        })
    };
    let mut vm = Vm::new(build(false), Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "_Level_Def", 0);
    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "after"), Some(Value::Int(1)));
    let mut vm = Vm::new(build(true), Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "_Level_Def", 0);
    let halt = vm.tick(id, &mut Bare).unwrap_err();
    assert_eq!(
        halt.reason,
        Reason::Unsupported("a parameter outside a call".into()),
        "{halt}"
    );
}

/// AS.34, challenge 49's trees on a list built in code: raising
/// `undefined`'s count goes nowhere, as `undefined`'s writes do; its count
/// still reads 0 and its last nothing, and the tick goes on.
#[test]
fn raising_undefineds_count_goes_nowhere() {
    let undefined = El::Member("abstractscriptgroup", "undefined");
    let p = program(|w| {
        let (count, after) = (var(w, "count"), var(w, "after"));
        let last = w.variable("last", "ScriptReference", None);
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[undefined, El::Member("setvariant", "count")]);
                w.inc(&[undefined, El::Member("setvariant", "count")]);
                let n = w.rhs(INT, &[undefined, El::Member("setvariant", "count")]);
                w.set(&[El::Local(count)], n);
                let l = w.value(&[El::Local(last)]);
                let r = w.reference(&[undefined, El::Member("setvariant", "last")], None);
                let o = w.op(Class::SetReference, "set reference");
                w.refer(o, 0x1C, l);
                w.refer(o, 0x20, r);
                w.push(o);
                w.inc(&[El::Local(after)]);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "Trunk_Template(1)", 0);
    vm.tick(id, &mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(vm.local(id, "count"), Some(Value::Int(0)));
    assert_eq!(vm.local(id, "last"), Some(Value::Null));
    assert_eq!(vm.local(id, "after"), Some(Value::Int(1)));
}

/// A host answering `animationinfo.walk` and `run` heading a path with the
/// clip it holds for the placement asking, and recording each world op's
/// clip as read (`_LHS` of a play, `_NP` of a move).
#[derive(Default)]
struct Clips {
    asked: Vec<(u64, Option<Value>, &'static str)>,
    played: Vec<(Class, Option<Value>)>,
}

impl Host for Clips {
    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
        for (name, clip) in [("walk", 0x77), ("run", 0x78)] {
            if at.member == MemberId::named("animationinfo", name) {
                self.asked.push((at.placement, at.receiver, name));
                return Ok(Value::Entity(clip + at.placement));
            }
        }
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
        let field = if call.class == Class::StartSequence {
            "_LHS"
        } else {
            "_NP"
        };
        let clip = call
            .operands
            .iter()
            .find(|(n, _)| *n == field)
            .and_then(|(_, v)| *v);
        self.played.push((call.class, clip));
        Ok(Answer::Done)
    }
}

/// AS.31, the maggot's walk and 008's boats' run on lists built in code:
/// each heads its path, asked of the host on the running placement's
/// behalf with no receiver, and the play or the move gets the clip the
/// host answers for that placement.
#[test]
fn a_creatures_walk_and_run_are_asked_for_the_placement_running() {
    let p = program(|w| {
        idle(
            w,
            |_| {},
            |w| {
                let play = w.start_sequence(&[El::Member("animationinfo", "walk")]);
                w.end_block(play);
                let np = w.group(&[El::Member("animationinfo", "run")], None);
                let o = w.op(Class::MoveTo, "move to");
                w.refer(o, 0x2C, np);
                w.push(o);
            },
        );
    });
    let mut config = Config::new(Mode::Strict);
    members::register(&mut config.members, members::LEVELS);
    let mut vm = Vm::new(p, config);
    let (maggot, boat) = (0x100, 0x200);
    let a = vm.add(maggot, "Magot", 0);
    let b = vm.add(boat, "Ship_Pirate1", 0);
    let mut host = Clips::default();
    vm.tick(a, &mut host).unwrap_or_else(|h| panic!("{h}"));
    vm.tick(b, &mut host).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(
        host.asked,
        vec![
            (maggot, None, "walk"),
            (maggot, None, "run"),
            (boat, None, "walk"),
            (boat, None, "run"),
        ]
    );
    let walk = |h: u64| Some(Value::Entity(0x77 + h));
    let run = |h: u64| Some(Value::Entity(0x78 + h));
    assert_eq!(
        host.played,
        vec![
            (Class::StartSequence, walk(maggot)),
            (Class::MoveTo, run(maggot)),
            (Class::StartSequence, walk(boat)),
            (Class::MoveTo, run(boat)),
        ]
    );
}

/// AS.29, PvP's bricks on a list built in code: a brick suspending itself
/// finishes that tick, then the step's walk passes it (its ticks, its clock
/// and its place on the list kept) while another reads its flag as 1; once
/// the other clears it, it ticks again from the next step, at its place. A
/// write of 7 holds as 1, a reset clears it, and on something the VM
/// doesn't run the flag goes through nothing.
#[test]
fn a_suspended_placement_is_passed_until_its_flag_clears() {
    let suspended = |of: El<'static>| [of, El::Member("placement", "script suspended")];
    let p = program(|w| {
        let (ticks, after, go, seen, far) = (
            var(w, "ticks"),
            var(w, "after"),
            var(w, "go"),
            var(w, "seen"),
            var(w, "far"),
        );
        let other = w.variable("other", "ScriptReference", None);
        let stranger = w.variable("stranger", "ScriptReference", None);
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Local(ticks)]);
                for (n, value) in [(1, 1), (3, 7)] {
                    let k = w.int(n);
                    let c = check(w, go, 1, k).0;
                    let v = w.int(value);
                    w.set(&suspended(MY), v);
                    w.inc(&[El::Local(after)]);
                    w.end_block(c);
                }
                let s = w.rhs(INT, &suspended(El::Local(other)));
                w.set(&[El::Local(seen)], s);
                let two = w.int(2);
                let c = check(w, go, 1, two).0;
                let zero = w.int(0);
                w.set(&suspended(El::Local(other)), zero);
                w.end_block(c);
                let four = w.int(4);
                let c = check(w, go, 1, four).0;
                let np = w.group(&suspended(El::Local(other)), None);
                let o = w.op(Class::Reset, "reset");
                w.refer(o, 0x1C, np);
                w.push(o);
                w.end_block(c);
                let one = w.int(1);
                w.set(&suspended(El::Local(stranger)), one);
                let s = w.rhs(INT, &suspended(El::Local(stranger)));
                w.set(&[El::Local(far)], s);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let (a, b, c) = (
        vm.add(0x10, "brick1PvP", 0),
        vm.add(0x11, "brick2PvP", 0),
        vm.add(0x12, "brick3PvP", 0),
    );
    let step = |vm: &mut Vm| {
        vm.step(&mut Bare).unwrap_or_else(|h| panic!("{h}"));
    };
    let ticks = |vm: &Vm, id| vm.local(id, "ticks");
    step(&mut vm);
    assert!(vm.set_local(b, "other", Value::Entity(0x10)));
    assert!(vm.set_local(a, "stranger", Value::Entity(0x999)));
    vm.set_local(a, "go", Value::Int(1));
    step(&mut vm);
    assert!(vm.is_suspended(a));
    assert_eq!(vm.local(a, "after"), Some(Value::Int(1)), "its tick ran on");
    let clock = vm.active_seconds(a);
    for _ in 0..3 {
        step(&mut vm);
    }
    assert_eq!(ticks(&vm, a), Some(Value::Int(2)), "passed");
    assert_eq!(ticks(&vm, b), Some(Value::Int(5)));
    assert_eq!(vm.active_seconds(a), clock, "its clock kept");
    assert_eq!(vm.local(b, "seen"), Some(Value::Int(1)));
    assert_eq!(vm.local(a, "far"), Some(Value::Int(0)), "a stranger's none");
    // B clears it after the walk passed A: A ticks from the next step, first.
    vm.set_local(a, "go", Value::Int(0));
    vm.set_local(b, "go", Value::Int(2));
    step(&mut vm);
    assert!(!vm.is_suspended(a));
    assert_eq!(ticks(&vm, a), Some(Value::Int(2)), "not this step");
    vm.set_local(b, "go", Value::Int(0));
    step(&mut vm);
    assert_eq!(ticks(&vm, a), Some(Value::Int(3)), "the next");
    let order: Vec<String> = vm.tick_order().into_iter().map(|(_, n)| n).collect();
    assert_eq!(
        order,
        ["brick1PvP", "brick2PvP", "brick3PvP"],
        "its place kept"
    );
    // 7 holds as 1; B's reset clears it.
    vm.set_local(a, "go", Value::Int(3));
    step(&mut vm);
    assert!(vm.is_suspended(a));
    assert_eq!(vm.local(b, "seen"), Some(Value::Int(1)));
    vm.set_local(b, "go", Value::Int(4));
    step(&mut vm);
    assert!(!vm.is_suspended(a));
    assert_eq!(ticks(&vm, c), Some(Value::Int(9)));
}

/// AS.29: the walk reads the flag as it reaches a placement (the original's
/// updater checks it per placement, just before that placement's tick), so
/// one a placement ahead of it on the list clears ticks in that same step;
/// one it suspends ahead of it is passed that step.
#[test]
fn a_flag_cleared_ahead_of_it_in_the_walk_ticks_it_that_step() {
    let p = program(|w| {
        let (ticks, go, seen) = (var(w, "ticks"), var(w, "go"), var(w, "seen"));
        let other = w.variable("other", "ScriptReference", None);
        let suspended = [
            El::Local(other),
            El::Member("placement", "script suspended"),
        ];
        idle(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Local(ticks)]);
                let s = w.rhs(INT, &suspended);
                w.set(&[El::Local(seen)], s);
                for (n, value) in [(1, 1), (2, 0)] {
                    let k = w.int(n);
                    let c = check(w, go, 1, k).0;
                    let v = w.int(value);
                    w.set(&suspended, v);
                    w.end_block(c);
                }
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let (a, b) = (vm.add(0x10, "brick1PvP", 0), vm.add(0x11, "brick2PvP", 0));
    let step = |vm: &mut Vm| vm.step(&mut Bare).unwrap_or_else(|h| panic!("{h}"));
    let ticks = |vm: &Vm, id| vm.local(id, "ticks");
    step(&mut vm);
    assert!(vm.set_local(a, "other", Value::Entity(0x11)));
    vm.set_local(a, "go", Value::Int(1));
    step(&mut vm);
    assert!(vm.is_suspended(b));
    assert_eq!(
        ticks(&vm, b),
        Some(Value::Int(1)),
        "passed the step A set it"
    );
    vm.set_local(a, "go", Value::Int(0));
    step(&mut vm);
    assert_eq!(ticks(&vm, b), Some(Value::Int(1)));
    vm.set_local(a, "go", Value::Int(2));
    step(&mut vm);
    assert!(!vm.is_suspended(b));
    assert_eq!(
        ticks(&vm, b),
        Some(Value::Int(2)),
        "ticked the step A cleared it"
    );
    assert_eq!(ticks(&vm, a), Some(Value::Int(4)));
}

/// AS.29's reading on the way out: a placement suspended and then taken off
/// still runs its SHUTDOWN once, after the step's walk, as one not
/// suspended does (whether the original's shutdown queue passes a suspended
/// placement isn't read; the crate docs, The tick, say so).
#[test]
fn a_suspended_placement_taken_off_still_runs_its_shutdown() {
    let p = program(|w| {
        let (ticks, shut) = (var(w, "ticks"), var(w, "shut"));
        frame(
            w,
            |_| {},
            |w| {
                w.inc(&[El::Local(shut)]);
            },
            |_| {},
            |_| {},
            |w| {
                let b = w.behaviour("idle");
                w.inc(&[El::Local(ticks)]);
                let one = w.int(1);
                w.set(&[MY, El::Member("placement", "script suspended")], one);
                w.end_block(b);
            },
        );
    });
    let mut vm = Vm::new(p, Config::new(Mode::Strict));
    let id = vm.add(HANDLE, "brick1PvP", 0);
    for _ in 0..3 {
        vm.step(&mut Bare).unwrap_or_else(|h| panic!("{h}"));
    }
    assert!(vm.is_suspended(id));
    assert_eq!(vm.local(id, "ticks"), Some(Value::Int(1)), "passed");
    assert_eq!(vm.local(id, "shut"), None);
    assert!(vm.remove(HANDLE));
    vm.step(&mut Bare).unwrap_or_else(|h| panic!("{h}"));
    assert_eq!(
        vm.local(id, "shut"),
        Some(Value::Int(1)),
        "its SHUTDOWN ran"
    );
    assert_eq!(vm.local(id, "ticks"), Some(Value::Int(1)));
}

/// AS.35's maps hash in an order of their own, so a rule that walks one
/// can't lean on it: two variables `find variable` made by one name (an int
/// and a set) on a handle no placement runs under yet both join that
/// placement's variables when it's added, each under its own container
/// (AU.53), the one made first keyed by the name, either way round.
#[test]
fn of_two_variables_found_by_one_name_before_it_runs_both_are_kept_in_order() {
    for int_first in [true, false] {
        let p = program(|w| {
            let other = w.variable("other", "ScriptReference", None);
            idle(
                w,
                |_| {},
                |w| {
                    let int = |w: &mut Writer<'_>| {
                        let (f, fo) =
                            super::init::find(w, "x", "ValueInfo", INT, 2, &[El::Local(other)]);
                        let five = w.int(5);
                        w.set(&[El::Local(fo)], five);
                        w.end_block(f);
                    };
                    let set = |w: &mut Writer<'_>| {
                        let (f, _) =
                            super::init::find(w, "x", "ScriptSet", INT, 2, &[El::Local(other)]);
                        w.end_block(f);
                    };
                    if int_first {
                        int(w);
                        set(w);
                    } else {
                        set(w);
                        int(w);
                    }
                },
            );
        });
        let mut vm = Vm::new(p, Config::new(Mode::Strict));
        let finder = vm.add(0x10, "Finder", 0);
        vm.step(&mut Bare).unwrap_or_else(|h| panic!("{h}"));
        assert!(vm.set_local(finder, "other", Value::Entity(0x99)));
        vm.step(&mut Bare).unwrap_or_else(|h| panic!("{h}"));
        let owner = vm.add(0x99, "Owner", 0);
        let x = vm.locals_named(owner, "x");
        let kinds: Vec<bool> = x.iter().map(|v| matches!(v, Value::Set(_))).collect();
        assert_eq!(kinds, [!int_first, int_first], "made first, keyed: {x:?}");
        assert!(x.contains(&Value::Int(5)), "the int kept: {x:?}");
    }
}
