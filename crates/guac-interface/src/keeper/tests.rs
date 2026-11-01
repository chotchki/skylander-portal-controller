//! The keeper on a model built in code (AT.9): a clone of a template
//! answering as an instance, member access forwarded to the model, the
//! keeper's own measurement objects, strays, and a run on the VM stopping
//! where the registry refuses a member.

use std::sync::Arc;

use guac_formats::igz::Igz;
use guac_formats::igz::builder::IgzBuilder;
use guac_formats::script::builder::{El, Writer};
use guac_formats::script::{Class, Graph};
use guac_ir::interface::{Content, Template};
use guac_script::{
    Access, Answer as OpAnswer, Config, Host, HostFault, MemberId, Mode, Obj, OpCall, Program,
    Role, Unit, Value, Vm, members,
};

use super::*;
use crate::model::{NoWorld, Str, singleton};
use crate::sprite::{Source, kind, props};

const TEMPLATE: Obj = Obj { unit: 2, index: 40 };
const STRING: Obj = Obj { unit: 2, index: 41 };
const SPAWNER: Value = Value::Entity(0x51);

fn template(name: &str, visual: u32) -> Template {
    Template {
        name: name.into(),
        object: TEMPLATE.index,
        properties: props::VISIBLE | (1 << 3) | visual,
        flags: 0,
        position: [0, -320],
        z_depth: 0.0,
        priority: 0.0,
        width: 237.0,
        height: 96.0,
        scale: [100.0, 100.0, 100.0],
        color: [0x80, 0x80, 0x80, 0xFF],
        replace_color: [0x80, 0x80, 0x80, 0],
        num_value: 0,
        depth: 0.0,
        init_matrix: guac_ir::interface::Matrix {
            position: [1.0, 2.0, 3.0],
            rotation: [0.0, 90.0, 0.0],
        },
        matrix: guac_ir::interface::Matrix {
            position: [5.0, 0.0, 7.0],
            rotation: [0.0, 45.0, 0.0],
        },
        content: Content::Container,
        string: None,
    }
}

/// A keeper holding one container template at [`TEMPLATE`] and a string
/// at [`STRING`].
fn keeper() -> Keeper {
    let mut model = Model::new(BASES);
    model.add_template(
        Some(TEMPLATE),
        &template("Generic_Container", kind::CONTAINER),
        Value::Null,
    );
    model.add_file_string(STRING, "Level", Str::default());
    Keeper::new(model)
}

fn at(receiver: Option<Value>, namespace: &str, name: &str) -> Access {
    Access {
        placement: 0x51,
        receiver,
        member: MemberId::named(namespace, name),
        other_point: None,
        via: None,
        op: None,
    }
}

#[test]
fn a_clone_of_a_template_answers_as_an_instance() {
    let mut k = keeper();
    assert!(k.holds(Value::Object(TEMPLATE)));
    assert_eq!(k.spawn(Value::Entity(9), None, None, SPAWNER), None);
    let clone = k
        .spawn(Value::Object(TEMPLATE), None, None, SPAWNER)
        .expect("a sprite template clones");
    assert!(k.holds(clone));
    let id = k.model().sprite_of(clone).unwrap();
    let t = k.model().sprite_of(Value::Object(TEMPLATE)).unwrap();
    let s = k.model().sprite(id);
    assert_eq!(s.cloned_from, Source::Sprite(t));
    assert_eq!(s.activator, SPAWNER);
    assert_eq!(
        (s.init_matrix, s.matrix),
        (k.model().sprite(t).matrix, k.model().sprite(t).matrix),
        "its initial matrix its source's current"
    );
    assert_eq!(k.instances(), vec![id]);
    // A member reads it as the instance it is.
    let w = k
        .get(&at(Some(clone), "spriteinfo", "width"), &NoWorld)
        .unwrap();
    assert_eq!(w, Value::Float(237.0));
    let source = k.model().value_of(t);
    assert_eq!(
        k.get(
            &at(Some(clone), "spritematrixmeasurement", "parent"),
            &NoWorld
        ),
        Ok(source)
    );
    // Spawned at a sprite and facing 30: its place and its facing turn.
    let other = k
        .spawn(Value::Object(TEMPLATE), Some(clone), Some(30.0), SPAWNER)
        .unwrap();
    let o = k.model().sprite(k.model().sprite_of(other).unwrap());
    assert_eq!(o.init_matrix.position, [5.0, 0.0, 7.0]);
    assert_eq!(o.init_matrix.rotation, [0.0, 30.0, 0.0]);
    assert_eq!(o.matrix, o.init_matrix);
    // Removed: off `active`, read as taken off.
    assert!(k.remove(clone));
    assert!(k.removed(clone));
    assert!(!k.remove(Value::Entity(9)));
    assert_eq!(k.instances().len(), 1);
}

#[test]
fn a_member_write_reaches_the_model() {
    let mut k = keeper();
    let parent = k
        .spawn(Value::Object(TEMPLATE), None, None, SPAWNER)
        .unwrap();
    let child = k
        .spawn(Value::Object(TEMPLATE), None, None, SPAWNER)
        .unwrap();
    k.set(
        &at(Some(child), "spritematrixmeasurement", "attach node"),
        parent,
        &NoWorld,
    )
    .unwrap();
    k.set(
        &at(Some(child), "spriteinfo", "alignment"),
        Value::Int(6),
        &NoWorld,
    )
    .unwrap();
    k.set(
        &at(Some(child), "spriteinfo", "location"),
        Value::Screen(0xFEC0),
        &NoWorld,
    )
    .unwrap();
    let id = k.model().sprite_of(child).unwrap();
    let s = k.model().sprite(id);
    assert_eq!(s.parent, k.model().sprite_of(parent));
    assert_eq!(props::ALIGNMENT.get(s.properties), 6);
    assert_eq!(s.location, [0, -320]);
    assert_eq!(
        k.elements(
            &at(Some(parent), "spritematrixmeasurement", "attached objects"),
            &NoWorld
        ),
        Ok(vec![child])
    );
    // A member with no setter takes the write nowhere.
    k.set(
        &at(Some(child), "spriteinfo", "type"),
        Value::Int(1),
        &NoWorld,
    )
    .unwrap();
    assert_eq!(k.model().sprite(id).kind(), kind::CONTAINER);
    // A text written through the converter: its value set made a string.
    let converter = k
        .get(&at(None, "stringinfo", "string converter"), &NoWorld)
        .unwrap();
    assert_eq!(converter, k.model().singleton(singleton::STRING_CONVERTER));
    k.set_elements(
        &at(Some(converter), "stringconverter", "value set"),
        &[Value::Int(72), Value::Int(105)],
    )
    .unwrap();
    let text = k
        .get(&at(Some(converter), "stringconverter", "text"), &NoWorld)
        .unwrap();
    assert_eq!(k.model().string(text).unwrap().units, [72, 105]);
    k.set(
        &at(Some(converter), "stringconverter", "text"),
        Value::Object(STRING),
        &NoWorld,
    )
    .unwrap();
    assert_eq!(
        k.elements(
            &at(Some(converter), "stringconverter", "value set"),
            &NoWorld
        ),
        Ok("Level".bytes().map(|b| Value::Int(i32::from(b))).collect())
    );
    // A reset reaches it too: `ID` back to before its first write.
    let id_at = at(Some(child), "spritematrixmeasurement", "id");
    k.set(&id_at, Value::Int(5), &NoWorld).unwrap();
    k.reset(&id_at, &NoWorld).unwrap();
    assert_eq!(k.get(&id_at, &NoWorld), Ok(Value::Int(0)));
}

/// `scale` is an object of the keeper's whose parts it reads and writes; a
/// sprite is its own orientation; an initial matrix's turns read only.
#[test]
fn the_keepers_own_objects_answer_their_measurements() {
    let mut k = keeper();
    let s = k
        .spawn(Value::Object(TEMPLATE), None, None, SPAWNER)
        .unwrap();
    let scale = k
        .get(&at(Some(s), "spriteinfo", "scale"), &NoWorld)
        .unwrap();
    assert!(k.holds(scale));
    assert_ne!(scale, s);
    k.set(
        &at(Some(scale), "scalemeasurement", "x"),
        Value::Float(150.0),
        &NoWorld,
    )
    .unwrap();
    assert_eq!(
        k.get(&at(Some(scale), "scalemeasurement", "uniform"), &NoWorld),
        Err(HostFault::Nothing),
        "the three differ"
    );
    k.set(
        &at(Some(scale), "scalemeasurement", "uniform"),
        Value::Float(40.0),
        &NoWorld,
    )
    .unwrap();
    assert_eq!(
        k.get(&at(Some(scale), "scalemeasurement", "y"), &NoWorld),
        Ok(Value::Float(40.0))
    );
    let id = k.model().sprite_of(s).unwrap();
    assert_eq!(k.model().sprite(id).scale, [40.0; 3]);
    let turn = k
        .get(
            &at(Some(s), "spritematrixmeasurement", "orientation"),
            &NoWorld,
        )
        .unwrap();
    assert_eq!(turn, s, "the sprite is its own orientation");
    k.set(
        &at(Some(s), "orientationmeasurement", "roll"),
        Value::Float(180.0),
        &NoWorld,
    )
    .unwrap();
    k.set(
        &at(Some(s), "orientationmeasurement", "relative facing"),
        Value::Float(10.0),
        &NoWorld,
    )
    .unwrap();
    assert_eq!(k.model().sprite(id).matrix.rotation, [0.0, 55.0, 180.0]);
    let origin = k
        .get(&at(Some(s), "spritematrixmeasurement", "origin"), &NoWorld)
        .unwrap();
    let turns = k
        .get(
            &at(Some(origin), "matrixmeasurement", "orientation"),
            &NoWorld,
        )
        .unwrap();
    assert_eq!(
        k.get(
            &at(Some(turns), "orientationmeasurement", "facing"),
            &NoWorld
        ),
        Ok(Value::Float(45.0)),
        "the clone's initial matrix: its source's current"
    );
    k.set(
        &at(Some(turns), "orientationmeasurement", "facing"),
        Value::Float(1.0),
        &NoWorld,
    )
    .unwrap();
    assert_eq!(k.model().sprite(id).init_matrix.rotation[1], 45.0);
    // Spawned at an initial matrix: its place.
    let at_origin = k
        .spawn(Value::Object(TEMPLATE), Some(origin), None, SPAWNER)
        .unwrap();
    let placed = k.model().sprite(k.model().sprite_of(at_origin).unwrap());
    assert_eq!(placed.matrix.position, [5.0, 0.0, 7.0]);
}

/// A sprite member asked of no sprite, and a member the keeper doesn't
/// answer asked of its own object, read as nothing and are counted: never a
/// stop (decision 10).
#[test]
fn strays_and_unknown_members_are_nothing_and_counted() {
    let mut k = keeper();
    let s = k
        .spawn(Value::Object(TEMPLATE), None, None, SPAWNER)
        .unwrap();
    assert_eq!(
        k.get(&at(Some(Value::Entity(9)), "spriteinfo", "width"), &NoWorld),
        Err(HostFault::Nothing)
    );
    assert_eq!(
        k.set(
            &at(None, "spriteinfo", "height"),
            Value::Float(1.0),
            &NoWorld
        ),
        Err(HostFault::Nothing)
    );
    assert_eq!(
        k.strays()
            .iter()
            .map(|(n, c)| (n.as_str(), *c))
            .collect::<Vec<_>>(),
        [("spriteinfo.height", 1), ("spriteinfo.width", 1)]
    );
    let odd = at(Some(s), "abstractplacement", "attach node");
    assert_eq!(k.get(&odd, &NoWorld), Err(HostFault::Nothing));
    assert_eq!(k.unknown().get(&odd.member), Some(&1));
    // The class's own sets read on any receiver.
    assert_eq!(
        k.elements(&at(None, "spriteinfo", "active"), &NoWorld),
        Ok(vec![s])
    );
}

/// A run's host forwarding the interface to a keeper, as guacamole's does.
struct Forwarding {
    keeper: Keeper,
}

impl Host for Forwarding {
    fn get(&mut self, at: &Access) -> Result<Value, HostFault> {
        self.keeper.get(at, &NoWorld)
    }
    fn set(&mut self, at: &Access, v: Value) -> Result<(), HostFault> {
        self.keeper.set(at, v, &NoWorld)
    }
    fn reset(&mut self, at: &Access) -> Result<(), HostFault> {
        self.keeper.reset(at, &NoWorld)
    }
    fn draw(&mut self) -> u32 {
        0
    }
    fn op(&mut self, call: &OpCall) -> Result<OpAnswer, HostFault> {
        let first = call.operands.first().and_then(|(_, v)| *v);
        match (call.class, first) {
            (Class::Spawn, Some(t)) => Ok(
                match self
                    .keeper
                    .spawn(t, None, None, Value::Entity(call.placement))
                {
                    Some(c) => OpAnswer::Run {
                        receiver: Some(c),
                        result: Some(c),
                    },
                    None => OpAnswer::Skip,
                },
            ),
            _ => Err(HostFault::Unmodelled),
        }
    }
    fn elements(&mut self, at: &Access) -> Result<Vec<Value>, HostFault> {
        self.keeper.elements(at, &NoWorld)
    }
    fn set_elements(&mut self, at: &Access, e: &[Value]) -> Result<(), HostFault> {
        self.keeper.set_elements(at, e)
    }
}

/// A script whose one behaviour writes `spriteinfo.<name>` heading the path,
/// with no sprite for the member to land on.
fn writing(name: &'static str) -> Arc<Program> {
    let mut b = IgzBuilder::new();
    {
        let w = &mut Writer::new(&mut b);
        let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
        for (c, label) in [
            (Class::StartUp, "STARTUP"),
            (Class::ShutDown, "SHUTDOWN"),
            (Class::MacroSection, "MACROS"),
        ] {
            let (p, _) = w.open(c, label);
            w.end_block(p);
        }
        w.end_block(pre);
        let bh = w.behaviour("idle");
        let one = w.int(1);
        w.set(&[El::Member("spriteinfo", name)], one);
        w.end_block(bh);
        w.end("C:/tfb/Synthetic/Ui.ai");
    }
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let graph = Graph::read(&igz).unwrap();
    let unit = Unit {
        name: "ui",
        role: Role::Other,
        igz: &igz,
        graph: &graph,
    };
    Arc::new(Program::link(&[unit]).unwrap())
}

/// On a run registering the interface, strict: a member of a sprite the
/// keeper answers with nothing to answer on is quiet, one the registry
/// doesn't hold stops the run naming it, as strict mode stops on any.
#[test]
fn a_member_the_model_doesnt_hold_is_refused_as_strict_mode_refuses_one() {
    for (name, stops) in [("width", false), ("lustre", true)] {
        let mut c = Config::new(Mode::Strict);
        members::register(&mut c.members, members::INTERFACE);
        let mut vm = Vm::new(writing(name), c);
        let id = vm.add(0x51, "UI", 0);
        let mut host = Forwarding { keeper: keeper() };
        let r = vm.tick(id, &mut host);
        match (stops, r) {
            (false, Ok(_)) => {}
            (true, Err(h)) => {
                let named = MemberId::named("spriteinfo", name);
                assert!(
                    h.to_string()
                        .contains(&format!("{:08x}.{:08x}", named.namespace, named.name)),
                    "{h}"
                );
            }
            (_, r) => panic!("spriteinfo.{name}: {r:?}"),
        }
    }
}

/// The keeper answers the 64 `guac-script` registers as the interface's,
/// each typed there as its handlers' table has it here: the same member,
/// class and range, one for one.
#[test]
fn the_registry_and_the_handlers_name_the_same_64() {
    assert_eq!(members::INTERFACE.len(), MEMBERS.len());
    for (r, m) in members::INTERFACE.iter().zip(&MEMBERS) {
        assert_eq!(r.path(), m.path(), "in the same order");
        assert_eq!(
            (r.class, r.min, r.max),
            (m.class, m.min, m.max),
            "{}",
            m.path()
        );
        assert_eq!(r.kind(), m.kind(), "{}", m.path());
    }
    let k = keeper();
    for r in members::INTERFACE {
        assert!(
            k.is_member(MemberId::named(r.namespace, r.name)),
            "{}",
            r.path()
        );
        assert!(Keeper::in_namespace(MemberId::named(r.namespace, r.name)));
    }
    assert!(!Keeper::in_namespace(MemberId::named("actorinfo", "width")));
}
