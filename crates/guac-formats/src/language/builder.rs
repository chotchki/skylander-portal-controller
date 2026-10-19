//! Writes language packs and their levels in code, for tests and fuzz seeds
//! (`test-fixtures`). The text is ours; nothing comes off a disc.

use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

/// `strings` as a `_stringDict`: UTF-16BE, each ending in a NUL.
pub fn dict(strings: &[&str]) -> Vec<u8> {
    strings
        .iter()
        .flat_map(|s| s.encode_utf16().chain([0]))
        .flat_map(u16::to_be_bytes)
        .collect()
}

/// A `LanguagePackInfo` at `o`: name, null directory, resolve state 1, the
/// id and a dict in `blocks`.
pub fn pack_info(
    b: &mut IgzBuilder,
    o: Location,
    blocks: u8,
    name: &str,
    lang_id: i32,
    dict: &[u8],
) {
    b.string(at(o, 0x08), name);
    b.put_bytes(at(o, 0x10), &[1]);
    b.put_u32(at(o, 0x14), lang_id as u32);
    b.memory_ref(at(o, 0x18), blocks, dict, 0);
}

/// A `.pak`: a root `igObjectList` holding one `LanguagePackInfo`.
pub fn pack(name: &str, lang_id: i32, strings: &[&str]) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let root = b.object(0, "igObjectList", 0x18);
    let info = b.object(0, "LanguagePackInfo", 0x20);
    let blocks = b.section(0x10);
    list(&mut b, root, blocks, &[Some(info)]);
    pack_info(&mut b, info, blocks, name, lang_id, &dict(strings));
    b.build()
}

/// A level whose root list holds one `StringInfo` per name. Every other
/// key goes into a later section, so address order isn't creation order:
/// keys 1, 3, 5 ... come first. Returns the bytes and each key's location
/// in `names` order.
pub fn level(names: &[&str]) -> (Vec<u8>, Vec<Location>) {
    let mut b = IgzBuilder::new();
    let root = b.object(0, "igObjectList", 0x18);
    let late = b.section(4);
    let blocks = b.section(0x10);
    let keys: Vec<Location> = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let o = b.object(if i % 2 == 0 { late } else { 0 }, "StringInfo", 0x28);
            b.string(at(o, 0x08), name);
            o
        })
        .collect();
    let elements: Vec<_> = keys.iter().copied().map(Some).collect();
    list(&mut b, root, blocks, &elements);
    (b.build(), keys)
}

/// One file holding a level's keys and a pack for them, in address order,
/// so the pack joins to it: the fuzz seed.
pub fn joined(names: &[&str], lang_id: i32) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let root = b.object(0, "igObjectList", 0x18);
    let keys: Vec<Location> = names
        .iter()
        .map(|name| {
            let o = b.object(0, "StringInfo", 0x28);
            b.string(at(o, 0x08), name);
            o
        })
        .collect();
    let info = b.object(0, "LanguagePackInfo", 0x20);
    let blocks = b.section(0x10);
    let mut elements: Vec<_> = keys.iter().copied().map(Some).collect();
    elements.push(Some(info));
    list(&mut b, root, blocks, &elements);
    let texts: Vec<String> = names.iter().map(|n| n.to_uppercase()).collect();
    let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
    pack_info(&mut b, info, blocks, "ENGLISH", lang_id, &dict(&texts));
    b.build()
}
