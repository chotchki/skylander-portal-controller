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
        let own = blocks(program, s);
        let len = program.scripts[s as usize].ops.len() as u32;
        for p in 0..len {
            let def = own
                .iter()
                .filter(|(a, e, _)| (*a..=*e).contains(&p))
                .max_by_key(|(a, _, _)| *a)
                .map(|(_, _, d)| *d);
            scan(
                program,
                s,
                std::iter::once(p),
                def,
                &mut asks,
                &mut Vec::new(),
            );
        }
    }
    asks.into_iter().collect()
}

#[cfg(test)]
mod tests;
