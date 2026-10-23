//! AH.59's tick order on scripts built in code (crate docs, The tick): the
//! activators' group ahead of the rest, a requeued placement at its group's
//! end, a native in its held placement's place, and the walk skipping
//! nothing when a placement leaves a place it has passed. Each placement
//! reads a file's counter into its `seen`, then counts it on, so `seen` is
//! its place in the step's walk.

use super::init::Engine;
use super::*;

/// Every placement runs this: `seen` records its place in the step's walk.
fn counting() -> Arc<Program> {
    program(|w| {
        let f = w.b.object(0, "ValueInfo", 0x24);
        w.text(f, 0x08, "Counter");
        w.meta_word(f, 0x1C, INT);
        w.word(f, 0x20, 0);
        let seen = var(w, "seen");
        idle(
            w,
            |_| {},
            |w| {
                let v = w.rhs(INT, &[El::Local(f)]);
                w.set(&[El::Local(seen)], v);
                w.inc(&[El::Local(f)]);
            },
        )
    })
}

fn seen(vm: &Vm, ids: &[PlacementId]) -> Vec<Option<Value>> {
    ids.iter().map(|&id| vm.local(id, "seen")).collect()
}

fn ints(v: &[i32]) -> Vec<Option<Value>> {
    v.iter().map(|&n| Some(Value::Int(n))).collect()
}

/// The activators tick ahead of every other placement, whenever they were
/// added (the hero's minion, added last, ticks first: AH.59's recordings
/// have him alone in the scheduler's first group at every frame); one no
/// longer an activator joins the rest's end.
#[test]
fn activators_tick_first_then_the_rest_in_their_order() {
    let mut vm = Vm::new(counting(), Config::new(Mode::Strict));
    let ids = [
        vm.add(0x10, "A", 0),
        vm.add(0x20, "B", 0),
        vm.add(0x30, "Hero", 0),
    ];
    let mut host = Engine::default();
    assert!(vm.set_activator(0x30, true));
    assert!(!vm.set_activator(0x99, true), "no such placement");
    assert!(vm.is_activator(ids[2]) && !vm.is_activator(ids[0]));
    vm.step(&mut host).unwrap();
    assert_eq!(seen(&vm, &ids), ints(&[1, 2, 0]), "the activator first");
    assert_eq!(
        vm.tick_order(),
        [(true, "Hero"), (false, "A"), (false, "B")].map(|(g, n)| (g, n.to_owned()))
    );
    vm.set_activator(0x30, false);
    vm.step(&mut host).unwrap();
    assert_eq!(seen(&vm, &ids), ints(&[3, 4, 5]), "then at the rest's end");
}

/// A placement requeued (the original's `reset` of a placement) leaves its
/// place for its group's end: before a step, it ticks after every other;
/// during one, from the native of a placement the walk reaches first, it
/// still ticks that step, last (AH.59: the music controller resets the mover
/// update every frame, which then ticks after every other awake placement).
#[test]
fn a_requeued_placement_ticks_after_every_other_that_step() {
    let mut config = Config::new(Mode::Strict);
    config.whitelist = Whitelist::with_first_ticks(&["Mover", "X"], &[], &["Test.ai"]);
    let mut vm = Vm::new(counting(), config);
    let music = vm.add(0x10, "Music", 0);
    let ids = [vm.add(0x20, "Mover", 0), vm.add(0x30, "X", 0)];
    let mut host = Engine::default();
    // The first step runs every first tick; Music is held from the next.
    vm.step(&mut host).unwrap();
    assert!(vm.is_held(music));
    assert_eq!(seen(&vm, &ids), ints(&[1, 2]));
    let mut natives = 0;
    let mut requeue = |vm: &mut Vm, id: PlacementId, _: &mut dyn Host| {
        if id == music {
            natives += 1;
            assert!(vm.requeue(0x20));
        }
        Ok(())
    };
    vm.step_with(&mut host, &mut requeue).unwrap();
    assert_eq!(seen(&vm, &ids), ints(&[4, 3]), "the mover last, this step");
    vm.step_with(&mut host, &mut requeue).unwrap();
    assert_eq!(seen(&vm, &ids), ints(&[6, 5]));
    assert_eq!(natives, 2);
    assert!(!vm.requeue(0x99));
    // Requeued between steps, X goes last.
    assert!(vm.requeue(0x30));
    vm.step(&mut host).unwrap();
    assert_eq!(seen(&vm, &ids), ints(&[7, 8]));
}

/// A held placement's native runs where the walk reaches it: after the
/// placements ahead of it have ticked this step and before those after it.
#[test]
fn a_native_runs_in_its_held_placements_place() {
    let mut config = Config::new(Mode::Strict);
    config.whitelist = Whitelist::with_first_ticks(&["A", "B"], &[], &["Test.ai"]);
    let mut vm = Vm::new(counting(), config);
    let a = vm.add(0x10, "A", 0);
    let held = vm.add(0x20, "Held", 0);
    let b = vm.add(0x30, "B", 0);
    let mut host = Engine::default();
    vm.step(&mut host).unwrap();
    let mut saw = Vec::new();
    vm.step_with(&mut host, &mut |vm, id, _| {
        assert_eq!(id, held, "only a held placement runs one");
        saw.push((vm.local(a, "seen"), vm.local(b, "seen")));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        saw,
        [(Some(Value::Int(3)), Some(Value::Int(2)))],
        "A ticked this step, B not yet"
    );
    assert_eq!(seen(&vm, &[a, b]), ints(&[3, 4]));
    // A native's halt ends the step there: B doesn't tick.
    let halt = vm
        .step_with(&mut host, &mut |_, _, _| {
            Err(Halt {
                placement: None,
                placement_name: "native".to_owned(),
                script: String::new(),
                position: 0,
                op: None,
                reason: Reason::Unsupported("stop".to_owned()),
            })
        })
        .unwrap_err();
    assert!(halt.to_string().contains("stop"));
    assert_eq!(seen(&vm, &[a, b]), ints(&[5, 4]));
    // The next step walks from the head again.
    vm.step(&mut host).unwrap();
    assert_eq!(seen(&vm, &[a, b]), ints(&[6, 7]));
}

/// A placement leaving a place the walk has passed (here made an activator
/// mid-walk, from a native) doesn't make the walk skip the next one: the
/// original's scheduler steps its index back the same way.
#[test]
fn a_placement_leaving_a_passed_place_skips_nothing() {
    let mut config = Config::new(Mode::Strict);
    config.whitelist = Whitelist::with_first_ticks(&["A", "B", "C"], &[], &["Test.ai"]);
    let mut vm = Vm::new(counting(), config);
    let a = vm.add(0x10, "A", 0);
    let held = vm.add(0x20, "Held", 0);
    let b = vm.add(0x30, "B", 0);
    let c = vm.add(0x40, "C", 0);
    let mut host = Engine::default();
    vm.step(&mut host).unwrap();
    assert_eq!(
        seen(&vm, &[a, b, c]),
        ints(&[0, 2, 3]),
        "Held's first tick at 1"
    );
    vm.step_with(&mut host, &mut |vm, id, _| {
        if id == held {
            vm.set_activator(0x10, true);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(seen(&vm, &[a, b, c]), ints(&[4, 5, 6]), "B and C each once");
    vm.step(&mut host).unwrap();
    assert_eq!(seen(&vm, &[a, b, c]), ints(&[7, 8, 9]), "A first now");
}

/// What a held placement's native hands over through [`Host::adopt`]: a
/// spawn the walk reaches after the reset (AH.54's lava, spawned by the
/// eruption's tick after the music controller's).
#[derive(Default)]
struct Late {
    inner: Engine,
    pending: std::rc::Rc<std::cell::RefCell<Vec<(Handle, String, crate::ScriptId)>>>,
}

impl Host for Late {
    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
        self.inner.get(at)
    }
    fn set(&mut self, at: &Access, value: Value) -> Result<(), HostFault> {
        self.inner.set(at, value)
    }
    fn reset(&mut self, at: &Access) -> Result<(), HostFault> {
        self.inner.reset(at)
    }
    fn draw(&mut self) -> u32 {
        0
    }
    fn adopt(&mut self, _: &Program) -> Vec<(Handle, String, crate::ScriptId)> {
        std::mem::take(&mut *self.pending.borrow_mut())
    }
}

/// A placement sent round its states ticks after what joins the list behind
/// it during the walk, its script timer from 0 (AH.59: the eruption's lava,
/// spawned after the music controller has reset the mover update, lists
/// ahead of it; its timer one frame's at every record). Music resets the
/// mover where the walk reaches it; Spawner, after it, spawns Child once.
#[test]
fn a_reset_placement_ticks_after_what_joins_behind_it() {
    let mut config = Config::new(Mode::Strict);
    config.whitelist = Whitelist::with_first_ticks(&["Mover", "X", "Child"], &[], &["Test.ai"]);
    let mut vm = Vm::new(counting(), config);
    let music = vm.add(0x10, "Music", 0);
    let mover = vm.add(0x20, "Mover", 0);
    let spawner = vm.add(0x30, "Spawner", 0);
    let x = vm.add(0x40, "X", 0);
    let mut host = Late::default();
    vm.step(&mut host).unwrap();
    assert!(vm.is_held(music) && vm.is_held(spawner));
    assert_eq!(seen(&vm, &[mover, x]), ints(&[1, 3]));
    let pending = host.pending.clone();
    let mut spawned = false;
    let mut natives = |vm: &mut Vm, id: PlacementId, _: &mut dyn Host| {
        if id == music {
            assert!(vm.requeue(0x20));
        } else if id == spawner && !spawned {
            spawned = true;
            pending.borrow_mut().push((0x50, "Child".to_owned(), 0));
        }
        Ok(())
    };
    vm.step_with(&mut host, &mut natives).unwrap();
    let child = vm.placement_of(0x50).expect("adopted");
    assert_eq!(
        seen(&vm, &[x, child, mover]),
        ints(&[4, 5, 6]),
        "the child spawned behind the reset ticks ahead of the mover"
    );
    let order: Vec<String> = vm.tick_order().into_iter().map(|(_, n)| n).collect();
    assert_eq!(order, ["Music", "Spawner", "X", "Child", "Mover"]);
    assert_eq!(
        vm.active_seconds(mover),
        Some(vm.config().tick_seconds),
        "its timer restarted, one tick on"
    );
    assert_eq!(vm.active_seconds(x), Some(2.0 * vm.config().tick_seconds));
    vm.step_with(&mut host, &mut natives).unwrap();
    assert_eq!(seen(&vm, &[x, child, mover]), ints(&[7, 8, 9]));
    assert_eq!(vm.active_seconds(mover), Some(vm.config().tick_seconds));
}
