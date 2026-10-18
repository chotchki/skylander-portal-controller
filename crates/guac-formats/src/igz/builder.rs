//! Writes IGZ v6 files in code, laid out the way Giants' own are, for tests
//! and fuzz seeds (`test-fixtures` feature). No real IGZ is ever committed;
//! the game-magic guard fails on one.

use std::collections::BTreeMap;

use super::{
    FIXUP_HEADER_LEN, INDEX_FLAG, Location, MAGIC, MEMORY_PRESENT, PLACEHOLDER_SIZE,
    RECORD_HEADER_LEN, SECTION_ENTRY_LEN, SECTION_TABLE_AT, VERSION, kind,
};
use crate::fnv1a32;

/// The namespace Giants' image formats are imported from.
pub const METAIMAGES: &str = "metaimages";

struct DataSection {
    bytes: Vec<u8>,
    alignment: u32,
    pool: u32,
    placeholder: bool,
}

/// Builds one IGZ. Data section 0 starts with a zero word, as on Giants, so
/// location 0 is never an object.
pub struct IgzBuilder {
    hash: u32,
    types: Vec<(String, u32)>,
    tstr: Vec<String>,
    exid: Vec<(u32, u32)>,
    exnm: Vec<(u32, u32)>,
    tmhn: Vec<(u32, Location)>,
    sections: Vec<DataSection>,
    strings_section: Option<u8>,
    objects: Vec<Location>,
    markers: BTreeMap<u32, Vec<Location>>,
    unknown: Vec<(u32, u32, Vec<u8>)>,
    giants_kind_8: bool,
}

impl Default for IgzBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl IgzBuilder {
    pub fn new() -> Self {
        Self {
            hash: 0xCCEC_5BC4,
            types: Vec::new(),
            tstr: Vec::new(),
            exid: Vec::new(),
            exnm: Vec::new(),
            tmhn: Vec::new(),
            sections: vec![DataSection {
                bytes: vec![0; 4],
                alignment: 0x80,
                pool: 8,
                placeholder: false,
            }],
            strings_section: None,
            objects: Vec::new(),
            markers: BTreeMap::new(),
            unknown: Vec::new(),
            giants_kind_8: true,
        }
    }

    pub fn section(&mut self, alignment: u32) -> u8 {
        self.sections.push(DataSection {
            bytes: Vec::new(),
            alignment,
            pool: 0x15,
            placeholder: false,
        });
        (self.sections.len() - 1) as u8
    }

    /// A placeholder section (size 0xFFFFFFFF, no bytes).
    pub fn placeholder_section(&mut self) -> u8 {
        self.sections.push(DataSection {
            bytes: Vec::new(),
            alignment: 0,
            pool: 0x36,
            placeholder: true,
        });
        (self.sections.len() - 1) as u8
    }

    /// The type's TMET index, adding it with this MTSZ size if new.
    pub fn type_index(&mut self, name: &str, size: u32) -> u32 {
        if let Some(i) = self.types.iter().position(|(n, _)| n == name) {
            assert_eq!(self.types[i].1, size, "{name} registered with two sizes");
            return i as u32;
        }
        self.types.push((name.into(), size));
        (self.types.len() - 1) as u32
    }

    /// A zeroed object of the type, aligned to 4 at the section's end, with
    /// its type index and a reference count of 1 written.
    pub fn object(&mut self, section: u8, type_name: &str, size: u32) -> Location {
        let type_index = self.type_index(type_name, size);
        let at = self.block(section, &vec![0; size as usize], 4);
        self.put_u32(at, type_index);
        self.put_u32(Self::plus(at, 4), 1);
        self.objects.push(at);
        at
    }

    pub fn block(&mut self, section: u8, data: &[u8], align: u32) -> Location {
        let s = &mut self.sections[section as usize];
        assert!(!s.placeholder, "placeholder sections hold no bytes");
        let at = s.bytes.len().next_multiple_of(align.max(1) as usize);
        s.bytes.resize(at, 0);
        s.bytes.extend_from_slice(data);
        Location::new(section, at as u32)
    }

    pub fn put_u32(&mut self, at: Location, value: u32) {
        self.put_bytes(at, &value.to_be_bytes());
    }

    pub fn put_bytes(&mut self, at: Location, bytes: &[u8]) {
        let s = &mut self.sections[at.section()];
        let o = at.offset() as usize;
        s.bytes[o..o + bytes.len()].copy_from_slice(bytes);
    }

    /// A fixup marking `at` without writing the word, for broken files.
    pub fn marker(&mut self, kind: u32, at: Location) {
        let list = self.markers.entry(kind).or_default();
        assert!(!list.contains(&at), "{at:?} marked twice as kind {kind:#x}");
        list.push(at);
    }

    /// An extra RVTB entry without an object behind it, for broken files.
    pub fn raw_object(&mut self, at: Location) {
        self.objects.push(at);
    }

    pub fn pointer(&mut self, at: Location, target: Location) {
        self.put_u32(at, target.0);
        self.marker(kind::POINTERS, at);
    }

    /// The string in a strings section (made on first use, as Giants keeps
    /// its strings apart) and a string pointer to it at `at`.
    pub fn string(&mut self, at: Location, text: &str) {
        let section = match self.strings_section {
            Some(s) => s,
            None => {
                let s = self.section(2);
                self.strings_section = Some(s);
                s
            }
        };
        let target = self.block(section, &[text.as_bytes(), &[0]].concat(), 1);
        self.put_u32(at, target.0);
        self.marker(kind::RSTR, at);
    }

    /// An EXID import of `object` in `namespace`, by FNV-1a 32.
    pub fn import(&mut self, at: Location, namespace: &str, object: &str) -> u32 {
        self.import_raw(
            at,
            fnv1a32(object.as_bytes()),
            fnv1a32(namespace.as_bytes()),
        )
    }

    pub fn import_raw(&mut self, at: Location, object: u32, namespace: u32) -> u32 {
        let index = self.exid.len() as u32;
        self.exid.push((object, namespace));
        self.put_u32(at, INDEX_FLAG | index);
        self.marker(kind::IMPORTS, at);
        index
    }

    /// A present memory ref at `at` (size word, pointer) to `data`, appended
    /// to `section` at `4 << align_log2` bytes.
    pub fn memory_ref(
        &mut self,
        at: Location,
        section: u8,
        data: &[u8],
        align_log2: u32,
    ) -> Location {
        let target = self.block(section, data, 4 << align_log2);
        self.put_u32(at, MEMORY_PRESENT | (align_log2 << 27) | data.len() as u32);
        self.pointer(Self::plus(at, 4), target);
        target
    }

    /// A null memory ref with this size word, marked by kind 0xF.
    pub fn null_memory_ref(&mut self, at: Location, size_word: u32) {
        self.put_u32(at, size_word);
        self.put_u32(Self::plus(at, 4), 0);
        self.marker(kind::NULL_MEMORY_REFS, at);
    }

    /// A TMHN block holding `data` and a handle word for it at `at`.
    pub fn handle(&mut self, at: Location, section: u8, data: &[u8], align_log2: u32) -> u32 {
        let target = self.block(section, data, 4 << align_log2);
        let index = self.tmhn.len() as u32;
        self.tmhn
            .push(((align_log2 << 27) | data.len() as u32, target));
        self.put_u32(at, index);
        self.marker(kind::HANDLES, at);
        index
    }

    /// An EXNM metaobject reference, its names added to TSTR.
    pub fn meta_object(&mut self, at: Location, namespace: &str, name: &str) -> u32 {
        let namespace = self.tstr_index(namespace);
        let name = self.tstr_index(name);
        let index = self.exnm.len() as u32;
        self.exnm.push((namespace, name));
        self.put_u32(at, INDEX_FLAG | index);
        self.marker(kind::META_OBJECTS, at);
        index
    }

    /// A metaobject word without the index flag, as Giants' script types hold.
    pub fn unflagged_meta(&mut self, at: Location, value: u32) {
        self.put_u32(at, value);
        self.marker(kind::META_OBJECTS, at);
    }

    pub fn tstr_index(&mut self, name: &str) -> u32 {
        if let Some(i) = self.tstr.iter().position(|n| n == name) {
            return i as u32;
        }
        self.tstr.push(name.into());
        (self.tstr.len() - 1) as u32
    }

    /// A record of a kind this reader doesn't interpret. Giants' kind 8 is
    /// written by default; adding kind 8 here replaces it.
    pub fn unknown_record(&mut self, kind: u32, count: u32, payload: &[u8]) {
        if kind == 8 {
            self.giants_kind_8 = false;
        }
        self.unknown.push((kind, count, payload.to_vec()));
    }

    pub fn without_kind_8(mut self) -> Self {
        self.giants_kind_8 = false;
        self
    }

    fn plus(at: Location, n: u32) -> Location {
        at.offset_by(n).expect("inside the section")
    }

    pub fn build(&self) -> Vec<u8> {
        let mut objects = self.objects.clone();
        objects.sort();
        let mut records: Vec<(u32, u32, Vec<u8>)> = Vec::new();
        if !self.tstr.is_empty() {
            records.push((kind::TSTR, self.tstr.len() as u32, strings(&self.tstr)));
        }
        let names: Vec<String> = self.types.iter().map(|(n, _)| n.clone()).collect();
        records.push((kind::TMET, names.len() as u32, strings(&names)));
        let sizes: Vec<u8> = self
            .types
            .iter()
            .flat_map(|(_, s)| s.to_be_bytes())
            .collect();
        records.push((kind::MTSZ, self.types.len() as u32, sizes));
        for (k, pairs) in [(kind::EXID, &self.exid), (kind::EXNM, &self.exnm)] {
            if !pairs.is_empty() {
                records.push((k, pairs.len() as u32, pairs_bytes(pairs.iter().copied())));
            }
        }
        if !self.tmhn.is_empty() {
            let pairs = self.tmhn.iter().map(|(w, l)| (*w, l.0));
            records.push((kind::TMHN, self.tmhn.len() as u32, pairs_bytes(pairs)));
        }
        records.push((kind::RVTB, objects.len() as u32, pack_locations(&objects)));
        // Giants' order for the location lists.
        for k in [
            kind::RSTR,
            kind::POINTERS,
            kind::NULL_MEMORY_REFS,
            kind::IMPORTS,
            kind::META_OBJECTS,
            kind::HANDLES,
        ] {
            if let Some(list) = self.markers.get(&k) {
                let mut list = list.clone();
                list.sort();
                records.push((k, list.len() as u32, pack_locations(&list)));
            }
        }
        // Giants' kind 8: one big-endian u32, 4 on the disc (the first
        // object's location, or a constant).
        if self.giants_kind_8 && !objects.is_empty() {
            records.push((8, 1, objects[0].0.to_be_bytes().to_vec()));
        }
        records.extend(self.unknown.iter().cloned());

        let mut fixups = Vec::new();
        for word in [
            u32::from_be_bytes(*MAGIC),
            VERSION,
            0x0006_0000,
            FIXUP_HEADER_LEN as u32,
            records.len() as u32,
            FIXUP_HEADER_LEN as u32,
            0,
        ] {
            fixups.extend_from_slice(&word.to_be_bytes());
        }
        for (k, count, payload) in &records {
            let padded = payload.len().next_multiple_of(4);
            let length = (RECORD_HEADER_LEN + padded) as u32;
            for word in [*k, 0, 0, *count, length, RECORD_HEADER_LEN as u32] {
                fixups.extend_from_slice(&word.to_be_bytes());
            }
            fixups.extend_from_slice(payload);
            fixups.resize(fixups.len() + padded - payload.len(), 0);
        }

        // Header, the section table and its zero terminator, then sections.
        let table_len = (1 + self.sections.len() + 1) * SECTION_ENTRY_LEN;
        let mut out = vec![0u8; (SECTION_TABLE_AT + table_len).next_multiple_of(16)];
        out[..4].copy_from_slice(MAGIC);
        out[4..8].copy_from_slice(&VERSION.to_be_bytes());
        out[8..12].copy_from_slice(&self.hash.to_be_bytes());
        let mut entries = vec![(out.len() as u32, fixups.len() as u32, 0x800, 0)];
        out.extend_from_slice(&fixups);
        for s in &self.sections {
            out.resize(out.len().next_multiple_of(4), 0);
            if s.placeholder {
                entries.push((out.len() as u32, PLACEHOLDER_SIZE, 0, s.pool));
                out.extend_from_slice(&[0; 4]);
            } else {
                entries.push((out.len() as u32, s.bytes.len() as u32, s.alignment, s.pool));
                out.extend_from_slice(&s.bytes);
            }
        }
        for (i, (offset, size, alignment, pool)) in entries.into_iter().enumerate() {
            let at = SECTION_TABLE_AT + i * SECTION_ENTRY_LEN;
            for (j, word) in [offset, size, alignment, pool].into_iter().enumerate() {
                out[at + j * 4..at + j * 4 + 4].copy_from_slice(&word.to_be_bytes());
            }
        }
        out
    }
}

fn strings(names: &[String]) -> Vec<u8> {
    names.iter().flat_map(|n| n.bytes().chain([0])).collect()
}

fn pairs_bytes(pairs: impl Iterator<Item = (u32, u32)>) -> Vec<u8> {
    pairs
        .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
        .collect()
}

/// A sorted location list as nibble varints, low nibble first.
pub fn pack_locations(locations: &[Location]) -> Vec<u8> {
    let mut nibbles = Vec::new();
    let mut prev = 0u32;
    for at in locations {
        let step = at.0.checked_sub(prev).filter(|s| *s >= 4 && s % 4 == 0);
        let step = step.unwrap_or_else(|| panic!("{at:?} after {prev:#x}: not ascending words"));
        let mut v = (step - 4) / 4;
        loop {
            let n = (v & 7) as u8;
            v >>= 3;
            if v == 0 {
                nibbles.push(n);
                break;
            }
            nibbles.push(n | 8);
        }
        prev = at.0;
    }
    nibbles
        .chunks(2)
        .map(|pair| pair[0] | pair.get(1).map_or(0, |hi| hi << 4))
        .collect()
}

/// A small file touching every fixup kind this reader knows, plus Giants'
/// kind 8 and a placeholder section.
pub fn sample() -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let list = b.object(0, "igObjectList", 0x18);
    let image = b.object(0, "igImage2", 0x60);
    let geom = b.object(0, "igPS3EdgeGeometrySegment", 0xB0);
    b.placeholder_section();
    let pixels = b.section(0x80);
    let vertexes = b.section(0x10);

    // The list's data: pointers to the other two objects.
    let mut elements = Vec::new();
    elements.extend_from_slice(&image.0.to_be_bytes());
    elements.extend_from_slice(&geom.0.to_be_bytes());
    b.put_u32(Location(list.0 + 8), 2);
    b.put_u32(Location(list.0 + 0xC), 2);
    let block = b.memory_ref(Location(list.0 + 0x10), 0, &elements, 0);
    b.pointer(block, image);
    b.pointer(Location(block.0 + 4), geom);

    b.string(Location(image.0 + 8), "eruptor_body");
    b.import(Location(image.0 + 0x3C), METAIMAGES, "dxt5_tile_big_ps3");
    b.handle(Location(image.0 + 0x48), pixels, &[0xAB; 0x80], 5);

    b.memory_ref(Location(geom.0 + 0x10), vertexes, &[1, 2, 3, 4, 5, 6], 2);
    b.null_memory_ref(Location(geom.0 + 0x24), 0);
    b.null_memory_ref(Location(geom.0 + 0x50), 0x1000_0000);
    b.null_memory_ref(Location(geom.0 + 0x98), 0x7);
    b.meta_object(Location(geom.0 + 0x8C), "metaobject", "tfbActorInfo");
    b.unflagged_meta(Location(geom.0 + 0xA0), 1);
    b.build()
}

/// Seeds for the property test and the fuzz corpus.
pub fn seeds() -> Vec<Vec<u8>> {
    let mut minimal = IgzBuilder::new();
    minimal.object(0, "igObject", 8);
    vec![sample(), minimal.build(), minimal.without_kind_8().build()]
}
