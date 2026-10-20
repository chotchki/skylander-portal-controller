#![no_main]

//! The interpreter over graphs of the op-graph writer's shapes (AH.14): the
//! first byte picks a script built from the rest (checks, else skips, loops
//! and flows with block lengths from the input, calls, behaviour switches,
//! value cells, host members, a world op, casts, set growth) or the rest
//! read as an IGZ (seeded from the writer's `creature` and `every_class`).
//! The host's answers come from the input too. With the budget on, nothing
//! may panic and no tick may run past the budget.

use std::sync::Arc;

use guac_formats::igz::builder::IgzBuilder;
use guac_formats::igz::{Igz, Location};
use guac_formats::script::builder::{El, Writer};
use guac_formats::script::{Class, Graph};
use guac_script::{
    Access, Answer, Config, Host, HostFault, Kind, MemberSpec, Mode, OpCall, Program, Role, Unit,
    Value, Vm,
};
use libfuzzer_sys::fuzz_target;

const BUDGET: u32 = 4_000;
const TICKS: u64 = 3;
const PLACEMENTS: u64 = 2;
const INT: &str = "IntMeasurement";

struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn byte(&mut self) -> u8 {
        let b = if self.data.is_empty() {
            0
        } else {
            self.data[self.at % self.data.len()]
        };
        self.at += 1;
        b
    }

    fn done(&self) -> bool {
        self.at >= self.data.len()
    }
}

struct FuzzHost<'a>(Bytes<'a>);

impl Host for FuzzHost<'_> {
    fn get(&mut self, _: &Access) -> Result<Value, HostFault> {
        Ok(match self.0.byte() % 5 {
            0 => return Err(HostFault::Nothing),
            1 => return Err(HostFault::Unmodelled),
            2 => Value::Int(i32::from(self.0.byte()) - 128),
            3 => Value::Float(f32::from(self.0.byte()) / 7.0),
            _ => Value::Entity(u64::from(self.0.byte())),
        })
    }
    fn set(&mut self, _: &Access, _: Value) -> Result<(), HostFault> {
        match self.0.byte() % 3 {
            0 => Ok(()),
            1 => Err(HostFault::Nothing),
            _ => Err(HostFault::Unmodelled),
        }
    }
    fn reset(&mut self, a: &Access) -> Result<(), HostFault> {
        self.set(a, Value::Null)
    }
    fn draw(&mut self) -> u32 {
        u32::from_le_bytes([self.0.byte(), self.0.byte(), self.0.byte(), self.0.byte()])
    }
    fn op(&mut self, _: &OpCall) -> Result<Answer, HostFault> {
        Ok(match self.0.byte() % 5 {
            0 => Answer::Done,
            1 => Answer::Skip,
            2 => Answer::Run {
                receiver: Some(Value::Entity(9)),
                result: Some(Value::Int(1)),
            },
            3 => return Err(HostFault::Nothing),
            _ => return Err(HostFault::Unmodelled),
        })
    }
}

struct Env {
    vars: Vec<Location>,
    set: Location,
    macros: Vec<Location>,
    behaviours: Vec<Location>,
}

impl Env {
    fn var(&self, b: &mut Bytes<'_>) -> Location {
        self.vars[usize::from(b.byte()) % self.vars.len()]
    }
}

/// Up to 12 ops from the input; every block opened gets a length from the
/// input that stays inside the region, a flow's sometimes 0xFFFF.
fn region(w: &mut Writer<'_>, b: &mut Bytes<'_>, env: &Env) {
    let mut openers: Vec<(usize, Location, u8, bool)> = Vec::new();
    let n = usize::from(b.byte() % 12) + 1;
    for _ in 0..n {
        if b.done() {
            break;
        }
        match b.byte() % 16 {
            0 => {
                let lit = w.int(i32::from(b.byte()) - 64);
                let v = env.var(b);
                w.set(&[El::Local(v)], lit);
            }
            1 => {
                let (x, y) = (env.var(b), env.var(b));
                let (x, y) = (w.rhs(INT, &[El::Local(x)]), w.rhs(INT, &[El::Local(y)]));
                let e = w.expr(x, Some((u32::from(b.byte() % 5), y)));
                let v = env.var(b);
                w.set(&[El::Local(v)], e);
            }
            2 => {
                let v = env.var(b);
                w.inc(&[El::Local(v)]);
            }
            3 => {
                let v = env.var(b);
                let lhs = w.value(&[El::Local(v)]);
                let rhs = w.int(i32::from(b.byte() % 8));
                let (p, o) = w.open(Class::CheckValue, "if");
                w.refer(o, 0x1C, lhs);
                w.word(o, 0x20, u32::from(b.byte() % 7));
                w.refer(o, 0x24, rhs);
                openers.push((p, o, b.byte(), false));
            }
            4 => {
                let (p, o) = w.open(Class::Skip, "else");
                openers.push((p, o, b.byte(), true));
            }
            5 => {
                let count = w.int(i32::from(b.byte() % 6) - 1);
                let (p, o) = w.open(Class::LoopValue, "loop value|||loop index");
                w.refer(o, 0x20, count);
                openers.push((p, o, b.byte(), false));
            }
            6 => {
                let f = w.flow(Class::FlowLoop);
                w.push(f);
            }
            7 => {
                let f = w.flow(Class::FlowContinue);
                w.push(f);
            }
            8 if b.byte() % 4 == 0 => {
                let f = w.flow(Class::FlowEnd);
                w.push(f);
            }
            9 => {
                let m = env.macros[usize::from(b.byte()) % env.macros.len()];
                let arg = w.int(i32::from(b.byte()));
                w.call(m, &[("v0", arg)]);
            }
            10 => {
                let beh = env.behaviours[usize::from(b.byte()) % env.behaviours.len()];
                w.set_behavior(beh);
            }
            11 => {
                let v = w.int(i32::from(b.byte()));
                w.set(&[El::Member("test", "cell")], v);
            }
            12 => {
                let v = w.int(1);
                w.set(
                    &[El::Member("optoplevelbehavior", "my"), El::Member("actorinfo", "health")],
                    v,
                );
            }
            13 => {
                let f = w.float(f32::from(b.byte()) / 3.0);
                w.set(&[El::Member("system", "cast to integer")], f);
            }
            14 => {
                let n = w.int(i32::from(b.byte() % 5));
                w.set(&[El::Local(env.set), El::Member("setvariant", "count")], n);
            }
            _ => {
                let seq = w.group(&[El::Named("ActorInfo", "Chompy_Bite")], None);
                let (p, o) = w.open(Class::StartSequence, "start sequence");
                w.refer(o, 0x1C, seq);
                openers.push((p, o, b.byte(), false));
            }
        }
    }
    let end = w.len();
    for (p, o, want, flow) in openers {
        let max = end - 1 - p;
        let n = if flow && want == 255 {
            0xFFFF
        } else {
            usize::from(want) % (max + 1)
        };
        w.half(o, 0x1A, n as u16);
    }
}

fn structured(data: &[u8]) -> Vec<u8> {
    let mut builder = IgzBuilder::new();
    let w = &mut Writer::new(&mut builder);
    let b = &mut Bytes { data, at: 0 };
    let env = Env {
        vars: (0..4)
            .map(|i| w.variable(&format!("v{i}"), "ValueInfo", Some(INT)))
            .collect(),
        set: w.variable("s", "ScriptSet", Some("ValueInfo")),
        macros: (0..2)
            .map(|i| w.op(Class::DefineMacro, &format!("M{i}")))
            .collect(),
        behaviours: ["idle", "battle"]
            .iter()
            .map(|n| w.op(Class::UserBehavior, n))
            .collect(),
    };
    let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
    let (start, _) = w.open(Class::StartUp, "STARTUP");
    region(w, b, &env);
    w.end_block(start);
    let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
    w.end_block(shut);
    let (section, _) = w.open(Class::MacroSection, "MACROS");
    for &m in &env.macros {
        let at = w.push(m);
        let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
        let enter = w.flow(Class::FlowMacro);
        w.push(enter);
        w.close(iface);
        region(w, b, &env);
        w.end_block(at);
    }
    w.end_block(section);
    region(w, b, &env);
    w.end_block(pre);
    for &beh in &env.behaviours {
        let at = w.push(beh);
        region(w, b, &env);
        w.end_block(at);
    }
    w.end("C:/tfb/Content/Levels/Synthetic/Fuzz.ai");
    builder.build()
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let bytes = if mode % 2 == 0 {
        structured(rest)
    } else {
        rest.to_vec()
    };
    let Ok(igz) = Igz::parse(&bytes) else {
        return;
    };
    let Ok(graph) = Graph::read(&igz) else {
        return;
    };
    let unit = Unit {
        name: "fuzz",
        role: Role::Other,
        igz: &igz,
        graph: &graph,
    };
    let Ok(program) = Program::link(&[unit]) else {
        return;
    };
    let program = Arc::new(program);
    for mode in [Mode::Strict, Mode::Play] {
        let mut config = Config::new(mode);
        config.budget = BUDGET;
        config
            .members
            .value_cell("test", "cell", MemberSpec::new(Kind::Int, 0.0, 255.0))
            .host("actorinfo", "health", MemberSpec::new(Kind::Float, 0.0, 0.0));
        let mut vm = Vm::new(program.clone(), config);
        let mut host = FuzzHost(Bytes { data, at: 7 });
        if !program.scripts.is_empty() {
            for h in 0..PLACEMENTS {
                vm.add(h + 1, "Fuzzed", 0);
            }
        }
        for _ in 0..TICKS {
            let _ = vm.step(&mut host);
        }
        // No tick runs past the budget: the op that crosses it is counted,
        // then the script stops.
        assert!(vm.executed() <= (u64::from(BUDGET) + 1) * TICKS * PLACEMENTS);
        let before = vm.executed();
        let _ = vm.call(None, "M0", &mut host);
        assert!(vm.executed() - before <= u64::from(BUDGET) + 1);
    }
});
