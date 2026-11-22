//! A level's lightning emitters as the scripts reach them (openspec AS.700):
//! level 208's reflect beams are a `Reflect_Beam` clone whose one generator
//! is a lightning strip, and the emitter's script sets its end
//! (`lightningfields.target`) and width (`segmentgenerator.segment width`,
//! feet) through `particleinfo.generators`' first, then
//! `generatorinfo.lightning`.
//!
//! The host answers those paths on a level's own emitter whose generators
//! hold a lightning, template or clone, and nothing anywhere else: a
//! generator reads as the emitter itself (the reflect beams' templates hold
//! one each), its `lightning` the same, and the generator's other members
//! (its records, its ribbon) answer nothing as every path through
//! `generators` did before. A spawn archive's lightning (the level-up's)
//! stays unanswered, as the extract keeps it unread. Stand-in ledger:
//! `lightning`.

use std::collections::HashMap;

use guac_ir::particle::{Kind as GeneratorKind, LevelEmitters};
use guac_script::{Handle, Obj, Value};
use guac_script::{Kind, MemberSpec, Members};

use crate::script_engine::FEET_PER_METRE;

/// What a level's lightning emitter holds as its file stores it: its first
/// lightning generator's width (metres) and tint (four codes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Template {
    pub width: f32,
    pub tint: [u8; 4],
}

impl Template {
    /// Its width as the scripts read it: they keep feet, the file metres.
    pub fn width_feet(&self) -> f32 {
        self.width * FEET_PER_METRE
    }
}

/// What the scripts wrote on a lightning clone: its target, its width in
/// feet and its tint, each where written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beam {
    pub target: Value,
    pub width_feet: Option<f32>,
    pub tint: Option<u32>,
}

impl Default for Beam {
    fn default() -> Self {
        Self {
            target: Value::Null,
            width_feet: None,
            tint: None,
        }
    }
}

/// Every emitter a level places (its particles part, linked as `unit`)
/// whose generators hold a lightning, by object, with its first
/// lightning's values: what the TV draws its clones with.
pub fn templates(unit: u16, part: &LevelEmitters) -> HashMap<Obj, Template> {
    part.emitters
        .iter()
        .filter_map(|p| {
            let l = p.emitter.generators.iter().find_map(|g| match &g.kind {
                GeneratorKind::Lightning(l) => Some(l),
                _ => None,
            })?;
            Some((
                Obj {
                    unit,
                    index: p.emitter.object,
                },
                Template {
                    width: l.width,
                    tint: l.colour,
                },
            ))
        })
        .collect()
}

/// The members the host answers for a lightning (the module docs), each
/// as the live member table registers it.
pub const MEMBERS: [(&str, &str, Kind, f32, f32); 4] = [
    ("particleinfo", "generators", Kind::Set, 0.0, 0.0),
    ("generatorinfo", "lightning", Kind::Reference, 0.0, 0.0),
    (
        "segmentgenerator",
        "segment width",
        Kind::Float,
        0.0,
        2_147_483_647.0,
    ),
    ("lightningfields", "target", Kind::Reference, 0.0, 0.0),
];

/// The generator's members the built levels' scripts reach past
/// `generators` that a lightning answers nothing for, as before (openspec
/// AS.700's read of 201, 205, 208, 213, 214 and the hub's first stage).
pub const UNANSWERED: [(&str, &str, Kind); 15] = [
    ("generatorinfo", "ribbon", Kind::Reference),
    ("generatorinfo", "appearance", Kind::Reference),
    ("generatorinfo", "movement", Kind::Reference),
    ("generatorinfo", "generation", Kind::Reference),
    ("generatorinfo", "affectors", Kind::Reference),
    ("generatorinfo", "bloom intensity", Kind::Float),
    ("generatorinfo", "sort overlay layer", Kind::Int),
    ("generatorinfo", "start delay", Kind::Float),
    ("segmentgenerator", "taper", Kind::Int),
    ("segmentgenerator", "uv scroll", Kind::Float),
    ("segmentgenerator", "uv scale", Kind::Float),
    ("segmentgenerator", "segment count", Kind::Int),
    ("lightningfields", "chaos", Kind::Float),
    ("lightningfields", "constrained", Kind::Int),
    ("lightningfields", "freeze animation", Kind::Int),
];

/// `segmentgenerator.tint color`, kept where a script writes it.
pub const TINT: (&str, &str) = ("segmentgenerator", "tint color");

/// Registers [`MEMBERS`], [`TINT`] and [`UNANSWERED`] on the host, over
/// the value cell the hero's table keeps `generators` as.
pub fn register(members: &mut Members) {
    for (ns, name, kind, min, max) in MEMBERS {
        members.host(ns, name, MemberSpec::new(kind, min, max));
    }
    members.host(TINT.0, TINT.1, MemberSpec::new(Kind::Color, 0.0, 0.0));
    for (ns, name, kind) in UNANSWERED {
        members.host(ns, name, MemberSpec::new(kind, 0.0, 0.0));
    }
}

/// A lightning clone's beam as the TV draws it: where it ends and its
/// width in metres where the scripts set one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drawn {
    pub clone: Handle,
    pub end: [f32; 3],
    pub width: Option<f32>,
}
