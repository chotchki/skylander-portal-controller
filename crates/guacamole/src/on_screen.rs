//! The on-screen test's readers (openspec AH.107), what the host's
//! every-point-on-screen answer reaches (stand-in ledger: `on-screen`):
//! which scripts ask a point against the gameplay camera
//! (`positionmeasurement.collision` with `viewportinfo.default` on either
//! side), read off the linked op graphs. The host answers every such ask
//! "on screen" (AH.20), so before the whitelist grows each reader it would
//! run is listed with what its answer gates: one that gates play (a
//! trigger, a hint, a spawn) needs the gameplay camera's real answer first.
//!
//! A script reaches its own ops (every op outside a definition's block)
//! and, closed over, every definition an op it reaches names: a call, a
//! macro named as a value (an update macro put in a set), a parameter's
//! binding, a file variable's members (AH.12's rule, by the program's own
//! links). Spawned templates' scripts are roots of their own.
//!
//! Each ask a walker's scripts reach past the TV's own run is read for what
//! its answer gates (openspec AH.150, [`READ`]): one that gates play (a
//! target, a removal, a chase) is answered off the gameplay camera's view
//! while the camera's scripts drive it ([`crate::viewport::Viewport::shows`],
//! its hypotheses there), one that gates nothing in play (a shake) keeps
//! the every-point answer, and either way it holds no one. An ask not read
//! yet still holds the walker on his card.

use std::collections::{BTreeSet, HashSet};

use guac_script::{
    DefId, Elem, Global, Ins, MemberId, OpId, Operand, OperandId, Program, ScriptId,
};

/// One ask of a point against the gameplay camera: the op, where it sits
/// (its script's list and its position there) and the definition whose
/// block holds it, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ask {
    pub script: ScriptId,
    pub position: u32,
    pub op: OpId,
    pub def: Option<DefId>,
}

fn collision() -> MemberId {
    MemberId::named("positionmeasurement", "collision")
}

fn viewport() -> MemberId {
    MemberId::named("viewportinfo", "default")
}

/// The operands an op reads, its parameters' and checks' included.
fn operands(ins: &Ins) -> Vec<OperandId> {
    let all: Vec<Option<OperandId>> = match ins {
        Ins::Parameter(p) => vec![p.rhs, p.index],
        Ins::UseMacro { np, index } | Ins::SetBehavior { np, index } => vec![*np, *index],
        Ins::SetValue { lhs, rhs } => vec![*lhs, *rhs],
        Ins::SetReference { lhs, rhs, index } => vec![*lhs, *rhs, *index],
        Ins::Inc { lhs } | Ins::Dec { lhs } => vec![*lhs],
        Ins::Reset { np } => vec![*np],
        Ins::CheckValue { lhs, rhs, .. }
        | Ins::CheckMembership { lhs, rhs, .. }
        | Ins::ChangeMembership { lhs, rhs, .. }
        | Ins::ForEach { lhs, rhs, .. }
        | Ins::FindSubSet { lhs, rhs, .. } => vec![*lhs, *rhs],
        Ins::CheckReference {
            lhs, rhs, index, ..
        } => vec![*lhs, *rhs, *index],
        Ins::LoopValue { rhs } => vec![*rhs],
        Ins::FindVariable {
            owner, index, skip, ..
        } => vec![*owner, *index, *skip],
        Ins::World { operands, .. } => operands.iter().map(|(_, o)| *o).collect(),
        _ => Vec::new(),
    };
    all.into_iter().flatten().collect()
}

/// Whether a path's own elements name the gameplay camera, or an
/// expression's operands do.
fn names_viewport(program: &Program, o: OperandId) -> bool {
    match program.operands.get(o as usize) {
        Some(Operand::Path { elems, .. }) => elems.contains(&Elem::Member(viewport())),
        Some(Operand::Expr { a, b, .. }) => [a, b]
            .into_iter()
            .flatten()
            .any(|x| names_viewport(program, *x)),
        None => false,
    }
}

/// Whether operand `o`, or one under it, asks a point against the gameplay
/// camera; every element it names goes into `elems`.
fn visit(program: &Program, o: OperandId, elems: &mut Vec<Elem>, depth: u8) -> bool {
    if depth > 16 {
        return false;
    }
    match program.operands.get(o as usize) {
        Some(Operand::Path {
            elems: path, other, ..
        }) => {
            elems.extend(path.iter().copied());
            let asks = path.contains(&Elem::Member(collision()))
                && (path.contains(&Elem::Member(viewport()))
                    || other.is_some_and(|x| names_viewport(program, x)));
            let below = other.is_some_and(|x| visit(program, x, elems, depth + 1));
            asks || below
        }
        Some(Operand::Expr { a, b, .. }) => {
            let mut any = false;
            for x in [a, b].into_iter().flatten() {
                any |= visit(program, *x, elems, depth + 1);
            }
            any
        }
        None => false,
    }
}

/// The definition an element names (a definition is always an op of its
/// file, as the linker reads one), a stub followed to the placed one.
fn def_named(program: &Program, e: Elem, out: &mut Vec<DefId>) {
    let def = match e {
        Elem::Op(op) => match program.ops.get(op as usize) {
            Some(Ins::Define(d)) => Some(*d),
            _ => None,
        },
        Elem::Global(g) => {
            match program.globals.get(g as usize).map(|(_, g)| g) {
                Some(Global::Set { members, .. }) => {
                    for m in members {
                        def_named(program, *m, out);
                    }
                }
                Some(Global::Reference(r)) => def_named(program, *r, out),
                _ => {}
            }
            None
        }
        _ => None,
    };
    if let Some(d) = def {
        out.push(program.resolve(d).unwrap_or(d));
    }
}

/// Each script's definitions' blocks, by position: (start, end) inclusive.
fn blocks(program: &Program, script: ScriptId) -> Vec<(u32, u32, DefId)> {
    program
        .defs
        .iter()
        .enumerate()
        .filter_map(|(d, def)| {
            let (s, at) = def.placed?;
            (s == script).then(|| {
                let op = program.scripts[s as usize].ops[at as usize];
                (at + 1, at + program.branch(op), d as DefId)
            })
        })
        .collect()
}

/// The on-screen asks at `positions` of `script`'s list into `asks`, the
/// definitions those ops name into `named`.
fn scan(
    program: &Program,
    script: ScriptId,
    positions: impl Iterator<Item = u32>,
    def: Option<DefId>,
    asks: &mut BTreeSet<Ask>,
    named: &mut Vec<DefId>,
) {
    let ops = &program.scripts[script as usize].ops;
    for position in positions {
        let Some(&op) = ops.get(position as usize) else {
            continue;
        };
        let ins = &program.ops[op as usize];
        let mut elems = Vec::new();
        let mut asked = false;
        for o in operands(ins) {
            asked |= visit(program, o, &mut elems, 0);
        }
        if let Ins::Parameter(p) = ins
            && let Some(d) = p.def
        {
            named.push(program.resolve(d).unwrap_or(d));
        }
        for e in elems {
            def_named(program, e, named);
        }
        if asked {
            asks.insert(Ask {
                script,
                position,
                op,
                def,
            });
        }
    }
}

/// The asks `script` reaches (its own ops' and every definition's it
/// reaches), and the definitions reached.
pub fn reached(program: &Program, script: ScriptId) -> (Vec<Ask>, BTreeSet<DefId>) {
    let own = blocks(program, script);
    let len = program.scripts[script as usize].ops.len() as u32;
    let mut asks = BTreeSet::new();
    let mut named = Vec::new();
    scan(
        program,
        script,
        (0..len).filter(|p| !own.iter().any(|(s, e, _)| (*s..=*e).contains(p))),
        None,
        &mut asks,
        &mut named,
    );
    let mut seen: HashSet<DefId> = HashSet::new();
    while let Some(d) = named.pop() {
        if !seen.insert(d) {
            continue;
        }
        let Some((s, at)) = program.defs[d as usize].placed else {
            continue;
        };
        let op = program.scripts[s as usize].ops[at as usize];
        let end = at + program.branch(op);
        scan(program, s, at + 1..=end, Some(d), &mut asks, &mut named);
    }
    (asks.into_iter().collect(), seen.into_iter().collect())
}

/// Every on-screen ask a program's lists hold, wherever it sits.
pub fn every_ask(program: &Program) -> Vec<Ask> {
    let mut asks = BTreeSet::new();
    for s in 0..program.scripts.len() as ScriptId {
        asks_in(program, s, &mut asks);
    }
    asks.into_iter().collect()
}

/// Every on-screen ask `script`'s list holds, wherever it sits.
fn asks_in(program: &Program, s: ScriptId, asks: &mut BTreeSet<Ask>) {
    let own = blocks(program, s);
    let len = program.scripts[s as usize].ops.len() as u32;
    for p in 0..len {
        let def = own
            .iter()
            .filter(|(a, e, _)| (*a..=*e).contains(&p))
            .max_by_key(|(a, _, _)| *a)
            .map(|(_, _, d)| *d);
        scan(program, s, std::iter::once(p), def, asks, &mut Vec::new());
    }
}

/// What an ask's answer gates, read off its script's own ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing in play: the host's every-point answer stands.
    Cosmetic,
    /// Play: answered off the gameplay camera's view while its scripts
    /// drive it.
    Gates,
}

/// One ask read (openspec AH.150): its script by file name, its position
/// in that list, the definition whose block holds it (`None` for the
/// script's own ops), the verdict and what the answer gates, in our words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Read {
    pub file: &'static str,
    pub position: u32,
    pub def: Option<&'static str>,
    pub verdict: Verdict,
    pub gates: &'static str,
}

/// The asks the fourteen walkers' scripts reach past the TV's own run on
/// the 1.00 disc (openspec AH.150; AH.107's reader, `giants_walkers`' probe
/// of the asks), each read off its own ops. AH.107's five, which the TV's
/// own run reaches, keep the every-point answer and aren't listed. The host
/// knows an ask by its op, so two places holding one op (`101_BugmanGiant.ai`
/// #2059 and #2103) must read alike.
pub const READ: &[Read] = &[
    Read {
        file: "Minion.ai",
        position: 2920,
        def: Some("World - Shake Objects Now"),
        verdict: Verdict::Cosmetic,
        gates: "which shake objects (the breakables, `destructibles.ai`'s) bounce or twist at a \
                giant's footstep or slam, each with its shake sound and a draw or two; nothing reads \
                the shake's flags back",
    },
    Read {
        file: "002_SkyTurtle.ai",
        position: 1421,
        def: Some("002 - Manage Tornados"),
        verdict: Verdict::Gates,
        gates: "a tornado's chase target: the foes within 60 ft of it on screen",
    },
    Read {
        file: "002_SkyTurtle.ai",
        position: 1432,
        def: Some("002 - Manage Tornados"),
        verdict: Verdict::Gates,
        gates: "a tornado's chase target with no foe found: foes and breakables within 60 ft of it \
                on screen",
    },
    Read {
        file: "021_DrillBot.ai",
        position: 706,
        def: None,
        verdict: Verdict::Gates,
        gates: "the upgrade-4 gun's target: let go once off screen, the gun turning back and his \
                third attack's shots aiming anew",
    },
    Read {
        file: "025_BamBazooker.ai",
        position: 3243,
        def: None,
        verdict: Verdict::Gates,
        gates: "the mortar's target: the autoaim's only while on screen, else the ground 35 ft \
                ahead",
    },
    Read {
        file: "029_ShadowMaid.ai",
        position: 1704,
        def: Some("029 - Manage Orbiting Skulls"),
        verdict: Verdict::Gates,
        gates: "a launched skull leaving the screen: shut down 0.15 s later, the first one off \
                starting the starfall",
    },
    Read {
        file: "029_ShadowMaid.ai",
        position: 2496,
        def: None,
        verdict: Verdict::Gates,
        gates: "a launched orbiting skull leaving the screen: taken off 0.15 s later, the first \
                one off starting the starfall",
    },
    Read {
        file: "029_ShadowMaid.ai",
        position: 2569,
        def: None,
        verdict: Verdict::Gates,
        gates: "the starfall's targets: foes and breakables within 80 ft on screen",
    },
    Read {
        file: "101_BugmanGiant.ai",
        position: 2059,
        def: None,
        verdict: Verdict::Gates,
        gates: "his first attack's lock-on: the autoaim's target taken, and him turned to it, \
                only while it's on screen",
    },
    Read {
        file: "101_BugmanGiant.ai",
        position: 2103,
        def: None,
        verdict: Verdict::Gates,
        gates: "his first attack's lock-on from the throwing hand: the autoaim's target taken \
                only while it's on screen",
    },
];

/// The read of `ask` in `reads`: its script's file name (any case), its
/// position and the definition holding it all as read.
pub fn read_of<'r>(program: &Program, ask: &Ask, reads: &'r [Read]) -> Option<&'r Read> {
    let file = crate::scripts::file_name(&program.scripts.get(ask.script as usize)?.name);
    let def = ask.def.map(|d| program.defs[d as usize].display.as_str());
    reads.iter().find(|r| {
        r.position == ask.position
            && r.file.eq_ignore_ascii_case(file)
            && match (r.def, def) {
                (None, None) => true,
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                _ => false,
            }
    })
}

/// The ops of every ask in `program` that `reads` reads as gating play:
/// what the host answers off the gameplay camera's view
/// ([`crate::scripts::ScriptWorld::answer_from_view`]).
pub fn answered(program: &Program, reads: &[Read]) -> HashSet<OpId> {
    let mut asks = BTreeSet::new();
    for (s, script) in program.scripts.iter().enumerate() {
        let file = crate::scripts::file_name(&script.name);
        if reads
            .iter()
            .any(|r| r.verdict == Verdict::Gates && r.file.eq_ignore_ascii_case(file))
        {
            asks_in(program, s as ScriptId, &mut asks);
        }
    }
    asks.iter()
        .filter(|a| read_of(program, a, reads).is_some_and(|r| r.verdict == Verdict::Gates))
        .map(|a| a.op)
        .collect()
}

#[cfg(test)]
mod tests;
