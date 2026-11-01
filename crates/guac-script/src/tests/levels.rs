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
