//! Writes root lists in code, for tests and fuzz seeds (`test-fixtures`): a
//! bootstrap and a global whose ids [`super::Exports`] runs over.

use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;

use super::NAMESPACE;

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

/// A file whose root list holds one `StringInfo` per name, in order. With
/// `imports`, a `ScriptSet` after the roots lists those `global.bld` export
/// ids, so the file imports them.
pub fn rooted(names: &[String], imports: &[u32]) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let root = b.object(0, "igObjectList", 0x18);
    let roots: Vec<Option<Location>> = names
        .iter()
        .map(|name| {
            let o = b.object(0, "StringInfo", 0x28);
            b.string(at(o, 0x08), name);
            Some(o)
        })
        .collect();
    let blocks = b.section(0x10);
    list(&mut b, root, blocks, &roots);
    if !imports.is_empty() {
        let set = b.object(0, "ScriptSet", 0x1C);
        let members = b.object(0, "ScriptSetList", 0x24);
        b.string(at(set, 0x08), "imports");
        b.pointer(at(set, 0x18), members);
        let n = imports.len() as u32;
        b.put_u32(at(members, 0x08), n);
        b.put_u32(at(members, 0x0C), n);
        let data = b.memory_ref(at(members, 0x10), blocks, &vec![0; 4 * imports.len()], 0);
        let ns = crate::fnv1a32(NAMESPACE.as_bytes());
        for (i, id) in imports.iter().enumerate() {
            b.import_raw(at(data, 4 * i as u32), *id, ns);
        }
    }
    b.build()
}

/// Names that say which file and position an id landed on.
pub fn names(prefix: &str, n: usize) -> Vec<String> {
    (0..n).map(|i| format!("{prefix}{i}")).collect()
}

/// Giants' bootstrap shape: 19 roots.
pub fn bootstrap() -> Vec<u8> {
    rooted(&names("boot", 19), &[])
}
