use super::builder::{bootstrap, names, rooted};
use super::*;
use crate::igz::layouts::StringInfo;

fn name_of(igz: &Igz<'_>, object: usize) -> String {
    StringInfo::read(igz, object)
        .unwrap()
        .name
        .unwrap()
        .to_owned()
}

#[test]
fn ids_run_over_bootstrap_then_global() {
    let boot_bytes = bootstrap();
    let global_bytes = rooted(&names("global", 5), &[]);
    let boot = Igz::parse(&boot_bytes).unwrap();
    let global = Igz::parse(&global_bytes).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    assert_eq!(exports.global_base(), GLOBAL_BASE);
    assert_eq!(exports.global_roots(), 5);

    // Under 0x14: bootstrap's roots, id 1 the first.
    let Export::Bootstrap(Root::Object(o)) = exports.resolve(1).unwrap() else {
        panic!()
    };
    assert_eq!(name_of(&boot, o), "boot0");
    let Export::Bootstrap(Root::Object(o)) = exports.resolve(3).unwrap() else {
        panic!()
    };
    assert_eq!(name_of(&boot, o), "boot2");
    let Export::Bootstrap(Root::Object(o)) = exports.resolve(0x13).unwrap() else {
        panic!()
    };
    assert_eq!(name_of(&boot, o), "boot18");
    // From 0x14: global's, position = id - 0x14.
    let Export::Global(Root::Object(o)) = exports.resolve(0x14).unwrap() else {
        panic!()
    };
    assert_eq!(name_of(&global, o), "global0");
    let Export::Global(Root::Object(o)) = exports.resolve(0x18).unwrap() else {
        panic!()
    };
    assert_eq!(name_of(&global, o), "global4");
}

#[test]
fn an_id_past_the_end_or_zero_is_refused() {
    let boot_bytes = bootstrap();
    let global_bytes = rooted(&names("global", 5), &[]);
    let exports = Exports::new(
        &Igz::parse(&boot_bytes).unwrap(),
        &Igz::parse(&global_bytes).unwrap(),
    )
    .unwrap();
    for id in [0, 0x19, u32::MAX] {
        let e = exports.resolve(id).unwrap_err();
        assert_eq!(
            e.kind,
            ErrorKind::OutOfRange {
                value: id.into(),
                limit: 0x19
            },
            "id {id:#x}"
        );
    }
}

#[test]
fn imports_from_global_resolve_and_others_pass() {
    let boot_bytes = bootstrap();
    let global_bytes = rooted(&names("global", 3), &[]);
    let user_bytes = rooted(&names("user", 1), &[0x15, 0x04]);
    let boot = Igz::parse(&boot_bytes).unwrap();
    let global = Igz::parse(&global_bytes).unwrap();
    let user = Igz::parse(&user_bytes).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    let resolved: Vec<Export> = user
        .external_ids()
        .iter()
        .map(|id| exports.resolve_import(id).unwrap().unwrap())
        .collect();
    let Export::Global(Root::Object(g)) = resolved[0] else {
        panic!()
    };
    assert_eq!(name_of(&global, g), "global1");
    let Export::Bootstrap(Root::Object(b)) = resolved[1] else {
        panic!()
    };
    assert_eq!(name_of(&boot, b), "boot3");
    let other = ExternalId {
        object: 0x15,
        namespace: fnv1a32(b"metaimages"),
    };
    assert!(exports.resolve_import(&other).is_none());
}

#[test]
fn a_file_without_a_root_list_is_refused() {
    let mut b = crate::igz::builder::IgzBuilder::new();
    b.object(0, "StringInfo", 0x28);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    assert!(matches!(
        Exports::new(&igz, &igz).unwrap_err().kind,
        ErrorKind::WrongType { .. }
    ));
}

#[test]
fn an_import_root_comes_back_unresolved_and_a_null_root_is_refused() {
    use crate::igz::builder::IgzBuilder;
    // Roots: a key, an import from another namespace, a null slot.
    let bytes = {
        let mut b = IgzBuilder::new();
        let root = b.object(0, "igObjectList", 0x18);
        let key = b.object(0, "StringInfo", 0x28);
        let blocks = b.section(0x10);
        b.put_u32(root.offset_by(0x08).unwrap(), 3);
        b.put_u32(root.offset_by(0x0C).unwrap(), 3);
        let data = b.memory_ref(root.offset_by(0x10).unwrap(), blocks, &[0; 12], 0);
        b.pointer(data, key);
        b.import_raw(data.offset_by(4).unwrap(), 1, 0xBA91_D879);
        b.build()
    };
    let boot_bytes = bootstrap();
    let boot = Igz::parse(&boot_bytes).unwrap();
    let global = Igz::parse(&bytes).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    assert_eq!(
        exports.resolve(0x14).unwrap().object(),
        Some(global.object_at(global.objects()[1].at).unwrap().0)
    );
    assert_eq!(
        exports.resolve(0x15).unwrap(),
        Export::Global(Root::Import(ExternalId {
            object: 1,
            namespace: 0xBA91_D879
        }))
    );
    assert_eq!(exports.resolve(0x15).unwrap().object(), None);
    assert_eq!(exports.resolve(0x16).unwrap_err().kind, ErrorKind::Missing);
}
