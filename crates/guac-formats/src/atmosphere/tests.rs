use super::builder::{SCRIPT, Shape, ZoneCall, level, sample, zone_call};
use super::*;
use crate::igz::builder::IgzBuilder;
use crate::script::builder::{El, Writer};

fn read(bytes: &[u8]) -> Result<Atmosphere<'_>, FormatError> {
    Atmosphere::read(&Igz::parse(bytes).unwrap())
}

fn packed(bits: u32) -> Colour {
    bits.to_le_bytes()
}

/// A call's arguments as the reader gives them back.
fn expected(z: &ZoneCall) -> (i32, Colour, [Light; 3], Fog) {
    let light = |(d, s, f, t): (u32, u32, i32, i32)| Light {
        diffuse: packed(d),
        specular: packed(s),
        facing: f,
        tilt: t,
    };
    (
        z.id,
        packed(z.ambient),
        z.lights.map(light),
        Fog {
            on: z.fog_on,
            start: z.start,
            range: z.range,
            colour: packed(z.colour),
            sky_dome_percent: z.sky_dome_percent,
            sky_dome_colour: packed(z.sky_dome_colour),
            yon_colour: if z.yon_from_fog > 0 {
                packed(z.colour)
            } else {
                packed(z.yon)
            },
        },
    )
}

fn got(z: &Zone) -> (i32, Colour, [Light; 3], Fog) {
    (z.id, z.ambient, z.lights, z.fog)
}

/// The PS3's branch runs and the Wii's doesn't: zone 0 is the else's,
/// set up at the start, every argument as the call wrote it, colours low
/// byte red; the zones after the check are kept beside it.
#[test]
fn the_ps3s_zones_are_read_and_zone_0_is_set_up() {
    let bytes = sample();
    let a = read(&bytes).unwrap();
    assert_eq!(a.script, SCRIPT);
    let ids: Vec<i32> = a.zones.iter().map(|z| z.id).collect();
    assert_eq!(ids, [0, 6, 1, 2]);
    let start = a.start_zone().unwrap();
    assert_eq!(got(start), expected(&ZoneCall::sample(0, 1)));
    assert_eq!(got(&a.zones[1]), expected(&ZoneCall::sample(6, 2)));
    assert_eq!(got(&a.zones[3]), expected(&ZoneCall::sample(2, 4)));
    // Low byte red: 0x00315373 is (115, 83, 49).
    assert_eq!(packed(0x0031_5373), [115, 83, 49, 0]);
    assert!(a.zones.windows(2).all(|w| w[0].position < w[1].position));
}

/// A check the PS3 passes runs its block and skips the else; every one of
/// the six relations is evaluated with the PS3 as 1.
#[test]
fn a_platform_check_runs_as_a_ps3() {
    for (relation, rhs, ps3_side) in [
        (1, 1, true),  // == 1
        (1, 3, false), // == 3
        (5, 3, true),  // != 3
        (5, 1, false), // != 1
        (0, 1, true),  // <= 1
        (3, 1, false), // < 1
        (2, 1, true),  // >= 1
        (4, 0, true),  // > 0
    ] {
        let mut b = IgzBuilder::new();
        let mut w = Writer::new(&mut b);
        let add = w.op(Class::DefineMacro, ADD_ZONE);
        let then_zone = ZoneCall::sample(0, 5);
        let else_zone = ZoneCall::sample(0, 6);
        w.script(
            SCRIPT,
            |w| {
                let r = w.int(rhs);
                let check = w.check(&[El::Member(PLATFORM.0, PLATFORM.1)], relation, r);
                write_call(w, add, &then_zone);
                let skip = w.otherwise(check);
                write_call(w, add, &else_zone);
                w.close(skip);
            },
            |_| {},
            |_| {},
        );
        let bytes = b.build();
        let a = read(&bytes).unwrap();
        let want = if ps3_side { then_zone } else { else_zone };
        assert_eq!(a.zones.len(), 1, "{relation} {rhs}");
        assert_eq!(
            got(a.start_zone().unwrap()),
            expected(&want),
            "{relation} {rhs}"
        );
    }
}

fn write_call(w: &mut Writer<'_>, add: crate::igz::Location, z: &ZoneCall) {
    zone_call(w, add, z, &Shape::default());
}

/// A later call for an id rewrites that zone where it stands; the zone set
/// up is the last call for 0.
#[test]
fn a_second_call_for_an_id_rewrites_it() {
    let bytes = level(
        &[],
        &[ZoneCall::sample(0, 1), ZoneCall::sample(3, 2)],
        &[ZoneCall::sample(0, 7)],
        &Shape::default(),
    );
    let a = read(&bytes).unwrap();
    assert_eq!(a.zones.iter().map(|z| z.id).collect::<Vec<_>>(), [0, 3]);
    assert_eq!(a.start, Some(0));
    assert_eq!(got(&a.zones[0]), expected(&ZoneCall::sample(0, 7)));
    // No zone 0, nothing set up.
    let bytes = level(&[], &[ZoneCall::sample(4, 1)], &[], &Shape::default());
    let a = read(&bytes).unwrap();
    assert_eq!((a.zones.len(), a.start), (1, None));
}

/// The body's rule for the colour past the far clip: the fog's when the
/// call says so, else its own.
#[test]
fn the_yon_colour_follows_the_calls_choice() {
    let mut own = ZoneCall::sample(0, 1);
    own.yon_from_fog = 0;
    own.yon = 0x0012_3456;
    let bytes = level(&[], &[own], &[], &Shape::default());
    let a = read(&bytes).unwrap();
    assert_eq!(a.start_zone().unwrap().fog.yon_colour, packed(0x0012_3456));
    let bytes = level(&[], &[ZoneCall::sample(0, 1)], &[], &Shape::default());
    let a = read(&bytes).unwrap();
    let z = a.start_zone().unwrap();
    assert_eq!(z.fog.yon_colour, z.fog.colour);
}

/// What the reader doesn't model it refuses, by name.
#[test]
fn what_it_doesnt_model_is_refused() {
    let zones = [ZoneCall::sample(0, 1)];
    let refused = |shape: Shape| {
        let bytes = level(&[], &zones, &[], &shape);
        read(&bytes).unwrap_err()
    };
    // A parameter it reads, left out.
    let e = refused(Shape {
        omit: vec![params::FOG_RANGE],
        ..Shape::default()
    });
    assert_eq!((e.field, e.kind), (params::FOG_RANGE, ErrorKind::Missing));
    // One the macro doesn't have.
    let e = refused(Shape {
        stranger: Some("fog density"),
        ..Shape::default()
    });
    assert_eq!(e.field, "_varName");
    // A colour as an integer.
    let e = refused(Shape {
        colours_as_ints: true,
        ..Shape::default()
    });
    assert!(matches!(e.kind, ErrorKind::WrongType { .. }), "{e}");
    // Lights copied from another zone.
    let e = refused(Shape {
        lights_from: Some(1),
        ..Shape::default()
    });
    assert_eq!(e.field, params::LIGHTS_FROM);
    assert!(read(&level(&[], &zones, &[], &Shape::default())).is_ok());
}

/// A zone call under a condition it can't evaluate, or another
/// atmosphere macro in STARTUP, is refused; so is a level with no
/// definition, or two.
#[test]
fn a_zone_it_cant_place_is_refused() {
    // Under a check on something else.
    let mut b = IgzBuilder::new();
    let mut w = Writer::new(&mut b);
    let add = w.op(Class::DefineMacro, ADD_ZONE);
    w.script(
        SCRIPT,
        |w| {
            let one = w.int(1);
            let check = w.check(&[El::Member("system", "online")], 1, one);
            write_call(w, add, &ZoneCall::sample(0, 1));
            w.end_block(check);
        },
        |_| {},
        |_| {},
    );
    let bytes = b.build();
    let e = read(&bytes).unwrap_err();
    assert_eq!(e.field, "an atmosphere macro inside a block");

    // In the else of a check on something else.
    let mut b = IgzBuilder::new();
    let mut w = Writer::new(&mut b);
    let add = w.op(Class::DefineMacro, ADD_ZONE);
    w.script(
        SCRIPT,
        |w| {
            let one = w.int(1);
            let check = w.check(&[El::Member("system", "online")], 1, one);
            let skip = w.otherwise(check);
            write_call(w, add, &ZoneCall::sample(0, 1));
            w.close(skip);
        },
        |_| {},
        |_| {},
    );
    let bytes = b.build();
    assert_eq!(
        read(&bytes).unwrap_err().field,
        "an atmosphere macro inside a block"
    );

    // A switch in STARTUP.
    let mut b = IgzBuilder::new();
    let mut w = Writer::new(&mut b);
    let switch = w.op(Class::DefineMacro, OTHER_MACROS[1]);
    w.script(
        SCRIPT,
        |w| {
            let zero = w.int(0);
            w.call(switch, &[("to zone ID", zero)]);
        },
        |_| {},
        |_| {},
    );
    let bytes = b.build();
    assert_eq!(
        read(&bytes).unwrap_err().field,
        "an atmosphere macro in STARTUP"
    );

    // No definition script; two.
    let mut b = IgzBuilder::new();
    Writer::new(&mut b).script("C:/tfb/Other.ai", |_| {}, |_| {}, |_| {});
    let bytes = b.build();
    assert_eq!(read(&bytes).unwrap_err().kind, ErrorKind::Missing);
    let mut b = IgzBuilder::new();
    let mut w = Writer::new(&mut b);
    w.script(SCRIPT, |_| {}, |_| {}, |_| {});
    w.script(
        "C:/tfb/Content/Levels/Level_901/Scripts/Level_Definition_901.ai",
        |_| {},
        |_| {},
        |_| {},
    );
    let bytes = b.build();
    assert_eq!(read(&bytes).unwrap_err().kind, ErrorKind::Duplicate);
}

/// A definition whose STARTUP adds nothing reads as no zones.
#[test]
fn no_zones_is_no_zones() {
    let mut b = IgzBuilder::new();
    Writer::new(&mut b).script(SCRIPT, |_| {}, |_| {}, |_| {});
    let bytes = b.build();
    let a = read(&bytes).unwrap();
    assert!(a.zones.is_empty() && a.start.is_none());
}
