//! IGZ v6, Alchemy's object-graph file as Giants (BLUS30968) writes it:
//! big-endian with 32-bit pointers, read off the disc's 2,122 character IGZ
//! (openspec AC.5). A header and section table come first, then the fixup
//! section, then the data sections a [`Location`] indexes. Fixup records hold
//! the tables and the nibble-packed lists saying which words are pointers,
//! imports, handles, strings, null memory refs or named external refs ([`kind`]);
//! kinds this reader doesn't know (Giants writes kind 8 in every file) are
//! kept raw. Parsing checks every fixup against the file, and reads refuse an
//! unmarked non-zero word rather than guess.

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
pub mod layouts;

use std::collections::HashSet;

use crate::reader::Reader;
use crate::{ErrorKind, FormatError, fnv1a32};

const FORMAT: &str = "IGZ";
pub const MAGIC: &[u8; 4] = b"IGZ\x01";
pub const VERSION: u32 = 6;
/// Entries are (offset, size, alignment, memory pool), ended by a zero offset.
pub const SECTION_TABLE_AT: usize = 0x10;
pub const SECTION_ENTRY_LEN: usize = 0x10;
/// Section entries that fit before Giants' fixup section at 0x800.
pub const MAX_SECTIONS: usize = 0x7F;
/// The size word of a placeholder section, which has no bytes.
pub const PLACEHOLDER_SIZE: u32 = u32::MAX;
/// Magic, version, a word, first record offset, record count, two words.
pub const FIXUP_HEADER_LEN: usize = 0x1C;
/// Kind, two zero words, count, length, data start.
pub const RECORD_HEADER_LEN: usize = 0x18;
/// Bit 31 of a memory ref's size word: the ref has data.
pub const MEMORY_PRESENT: u32 = 0x8000_0000;
/// Bits 0-26 of a size word: the byte count. Bits 27-30 are the alignment.
pub const MEMORY_SIZE_MASK: u32 = 0x07FF_FFFF;
/// The flag on import and named external words; the bits below index EXID or EXNM.
pub const INDEX_FLAG: u32 = 0x8000_0000;
/// Locations address 24 bits of offset in each section.
pub const MAX_OFFSET: u32 = 0x00FF_FFFF;

const SUPPORTED: &str = "only IGZ v6, Giants', is read";

/// Fixup record kinds. Location lists are nibble varints, low nibble first,
/// 3 data bits and a continue bit, each value `v` moving on `v * 4 + 4`.
pub mod kind {
    /// Type names, NUL-terminated.
    pub const TMET: u32 = 0x0;
    /// Names EXNM indexes, NUL-terminated.
    pub const TSTR: u32 = 0x1;
    /// Imports by hash: (object, namespace) FNV-1a 32 pairs.
    pub const EXID: u32 = 0x2;
    /// Imports by name: (namespace, name) TSTR indexes.
    pub const EXNM: u32 = 0x3;
    /// The objects, a location list.
    pub const RVTB: u32 = 0x5;
    /// Pointer words, a location list.
    pub const POINTERS: u32 = 0x6;
    /// Import words (`0x80000000 | EXID index`), a location list.
    pub const IMPORTS: u32 = 0x7;
    /// Memory handle blocks: (size word, location) pairs.
    pub const TMHN: u32 = 0xA;
    /// Memory handle words (a TMHN index), a location list.
    pub const HANDLES: u32 = 0xB;
    /// Instance size per type, a u32 each.
    pub const MTSZ: u32 = 0xC;
    /// String pointer words, a location list.
    pub const RSTR: u32 = 0xE;
    /// Size words of null memory refs, a location list.
    pub const NULL_MEMORY_REFS: u32 = 0xF;
    /// Named external refs (`0x80000000 | EXNM index`), a location list:
    /// metaobjects (namespace `metaobject`) and named objects in level.bld.
    pub const META_OBJECTS: u32 = 0x10;

    /// Every kind this reader interprets.
    pub const KNOWN: [u32; 13] = [
        TMET,
        TSTR,
        EXID,
        EXNM,
        RVTB,
        POINTERS,
        IMPORTS,
        TMHN,
        HANDLES,
        MTSZ,
        RSTR,
        NULL_MEMORY_REFS,
        META_OBJECTS,
    ];
}

/// A position in the data sections: `(section << 24) | offset`, section 0
/// being the first after the fixups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location(pub u32);

impl Location {
    pub const fn new(section: u8, offset: u32) -> Self {
        Self(((section as u32) << 24) | (offset & MAX_OFFSET))
    }

    pub const fn section(self) -> usize {
        (self.0 >> 24) as usize
    }

    pub const fn offset(self) -> u32 {
        self.0 & MAX_OFFSET
    }

    /// `n` bytes on, or `None` when that leaves the section's 24 bits.
    pub fn offset_by(self, n: u32) -> Option<Self> {
        let offset = self.offset().checked_add(n)?;
        (offset <= MAX_OFFSET).then_some(Self((self.0 & !MAX_OFFSET) | offset))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub offset: u32,
    pub size: u32,
    pub alignment: u32,
    /// The runtime memory pool it loads into (opaque).
    pub pool: u32,
}

impl Section {
    /// Giants writes these with alignment 0, 4 bytes before the next section.
    pub fn is_placeholder(&self) -> bool {
        self.size == PLACEHOLDER_SIZE
    }

    /// Bytes the section holds in the file.
    pub fn len(&self) -> usize {
        if self.is_placeholder() {
            0
        } else {
            self.size as usize
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The fixup header's other words, constant on Giants (0x60000, 0x1C, 0) and
/// not interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixupHeader {
    pub word_08: u32,
    pub first_record: u32,
    pub record_count: u32,
    pub word_14: u32,
    pub word_18: u32,
}

/// One fixup record as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixupRecord<'a> {
    pub kind: u32,
    pub count: u32,
    /// Absolute offset of the record header.
    pub at: usize,
    /// Absolute offset of the payload.
    pub payload_at: usize,
    pub payload: &'a [u8],
}

impl FixupRecord<'_> {
    pub fn is_known(&self) -> bool {
        kind::KNOWN.contains(&self.kind)
    }
}

/// What a location-list fixup says a word is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Marker {
    Pointer,
    Import,
    Handle,
    String,
    NullMemoryRef,
    MetaObject,
}

impl Marker {
    pub const ALL: [Marker; 6] = [
        Marker::Pointer,
        Marker::Import,
        Marker::Handle,
        Marker::String,
        Marker::NullMemoryRef,
        Marker::MetaObject,
    ];

    pub const fn kind(self) -> u32 {
        match self {
            Marker::Pointer => kind::POINTERS,
            Marker::Import => kind::IMPORTS,
            Marker::Handle => kind::HANDLES,
            Marker::String => kind::RSTR,
            Marker::NullMemoryRef => kind::NULL_MEMORY_REFS,
            Marker::MetaObject => kind::META_OBJECTS,
        }
    }

    pub const fn field(self) -> &'static str {
        match self {
            Marker::Pointer => "pointer",
            Marker::Import => "import",
            Marker::Handle => "memory_handle",
            Marker::String => "string",
            Marker::NullMemoryRef => "null_memory_ref",
            Marker::MetaObject => "meta_object",
        }
    }
}

/// An EXID entry: FNV-1a 32 of the object's name and of its namespace
/// (`metaimages` is 0x843CD0C2). Some hold a small number in place of the
/// object hash; what it indexes isn't known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalId {
    pub object: u32,
    pub namespace: u32,
}

impl ExternalId {
    pub fn is(&self, namespace: &str, object: &str) -> bool {
        self.namespace == fnv1a32(namespace.as_bytes()) && self.object == fnv1a32(object.as_bytes())
    }
}

/// An EXNM entry with its TSTR indexes resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalName<'a> {
    pub namespace: &'a str,
    pub name: &'a str,
}

/// A TMHN block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryHandle<'a> {
    pub size_word: u32,
    pub at: Location,
    pub data: &'a [u8],
}

impl MemoryHandle<'_> {
    pub fn alignment(&self) -> u32 {
        alignment(self.size_word)
    }
}

/// A present memory ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryRef<'a> {
    pub size_word: u32,
    pub at: Location,
    pub data: &'a [u8],
}

impl MemoryRef<'_> {
    pub fn alignment(&self) -> u32 {
        alignment(self.size_word)
    }
}

/// `4 << n` for n in bits 27-30 of a size word.
pub fn alignment(size_word: u32) -> u32 {
    4 << ((size_word >> 27) & 0xF)
}

/// An object from RVTB, spanning its type's MTSZ size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Object {
    pub at: Location,
    pub type_index: u32,
    pub size: u32,
}

impl Object {
    /// The field at `offset`, or `None` past the object.
    pub fn field(&self, offset: u32) -> Option<Location> {
        if offset < self.size {
            self.at.offset_by(offset)
        } else {
            None
        }
    }
}

/// A word resolved through the fixup that marks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ref<'a> {
    /// A zero word no fixup marks.
    Null,
    /// A pointer to the start of `objects()[index]`.
    Object(usize),
    /// A pointer anywhere else: into a memory block or inside an object.
    Data(Location),
    Import {
        index: u32,
        id: ExternalId,
    },
    Handle {
        index: u32,
        handle: MemoryHandle<'a>,
    },
    /// The string's bytes, NUL excluded.
    String(&'a [u8]),
    /// The size word of a memory ref the file marks null.
    NullMemoryRef,
    /// A kind-0x10 word's EXNM entry. A class only when the namespace is
    /// `metaobject`; level.bld also names objects (LightInfo, SpriteInfo, ...).
    MetaObject {
        index: u32,
        name: ExternalName<'a>,
    },
    /// A metaobject word without the index flag (script types only, on
    /// Giants). What it indexes isn't known, so it stays as stored.
    UnresolvedMeta(u32),
}

/// A parsed IGZ, every fixup already checked against the file.
#[derive(Debug, Clone)]
pub struct Igz<'a> {
    bytes: &'a [u8],
    hash: u32,
    sections: Vec<Section>,
    fixup_header: FixupHeader,
    records: Vec<FixupRecord<'a>>,
    type_names: Vec<&'a str>,
    type_sizes: Vec<u32>,
    strings: Vec<&'a str>,
    external_ids: Vec<ExternalId>,
    external_names: Vec<ExternalName<'a>>,
    handles: Vec<MemoryHandle<'a>>,
    objects: Vec<Object>,
    /// Every marked word, sorted, one marker each.
    markers: Vec<(Location, Marker)>,
    /// Each kind-0xE target with its length, sorted.
    string_lens: Vec<(Location, u32)>,
}

impl<'a> Igz<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FormatError> {
        let r = Reader::new(FORMAT, bytes);
        r.magic(0, MAGIC, "magic")?;
        let version = r.u32_be(4, "version")?;
        if version != VERSION {
            return Err(r.err(
                "version",
                4,
                ErrorKind::UnsupportedVersion {
                    found: version.into(),
                    supported: SUPPORTED,
                },
            ));
        }
        let hash = r.u32_be(8, "hash")?;
        // Zero in every Giants file and of unknown meaning.
        let word_0c = r.u32_be(0xC, "header_word")?;
        if word_0c != 0 {
            return Err(r.err(
                "header_word",
                0xC,
                ErrorKind::Unsupported {
                    value: word_0c.into(),
                },
            ));
        }

        let sections = read_sections(&r)?;
        let (fixup_header, records) = read_records(&r, sections[0])?;
        let mut igz = Self {
            bytes,
            hash,
            sections,
            fixup_header,
            records,
            type_names: Vec::new(),
            type_sizes: Vec::new(),
            strings: Vec::new(),
            external_ids: Vec::new(),
            external_names: Vec::new(),
            handles: Vec::new(),
            objects: Vec::new(),
            markers: Vec::new(),
            string_lens: Vec::new(),
        };
        igz.read_tables()?;
        igz.read_objects()?;
        igz.read_markers()?;
        Ok(igz)
    }

    /// The header's u32 at 0x8, a hash of something unknown.
    pub fn hash(&self) -> u32 {
        self.hash
    }

    /// Every section, the fixup section first.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// The sections a [`Location`] indexes.
    pub fn data_sections(&self) -> &[Section] {
        &self.sections[1..]
    }

    pub fn fixup_header(&self) -> &FixupHeader {
        &self.fixup_header
    }

    /// Every fixup record in file order.
    pub fn records(&self) -> &[FixupRecord<'a>] {
        &self.records
    }

    pub fn record(&self, kind: u32) -> Option<&FixupRecord<'a>> {
        self.records.iter().find(|rec| rec.kind == kind)
    }

    /// The records this reader doesn't interpret.
    pub fn unknown_records(&self) -> impl Iterator<Item = &FixupRecord<'a>> {
        self.records.iter().filter(|rec| !rec.is_known())
    }

    /// TMET.
    pub fn type_names(&self) -> &[&'a str] {
        &self.type_names
    }

    /// MTSZ, one per type name.
    pub fn type_sizes(&self) -> &[u32] {
        &self.type_sizes
    }

    pub fn type_index(&self, name: &str) -> Option<u32> {
        self.type_names
            .iter()
            .position(|t| *t == name)
            .map(|i| i as u32)
    }

    pub fn type_name(&self, object: &Object) -> &'a str {
        self.type_names
            .get(object.type_index as usize)
            .copied()
            .unwrap_or_default()
    }

    /// TSTR.
    pub fn strings(&self) -> &[&'a str] {
        &self.strings
    }

    /// EXID.
    pub fn external_ids(&self) -> &[ExternalId] {
        &self.external_ids
    }

    /// EXNM, resolved.
    pub fn external_names(&self) -> &[ExternalName<'a>] {
        &self.external_names
    }

    /// TMHN.
    pub fn memory_handles(&self) -> &[MemoryHandle<'a>] {
        &self.handles
    }

    /// RVTB's objects, sorted by location.
    pub fn objects(&self) -> &[Object] {
        &self.objects
    }

    pub fn objects_of_type<'s>(&'s self, name: &str) -> impl Iterator<Item = &'s Object> + 's {
        let index = self.type_index(name);
        self.objects
            .iter()
            .filter(move |o| Some(o.type_index) == index)
    }

    /// The object starting exactly at `at`, with its index.
    pub fn object_at(&self, at: Location) -> Option<(usize, &Object)> {
        let i = self.objects.binary_search_by_key(&at, |o| o.at).ok()?;
        Some((i, &self.objects[i]))
    }

    /// The object whose extent holds `at`.
    pub fn object_containing(&self, at: Location) -> Option<(usize, &Object)> {
        let i = self
            .objects
            .partition_point(|o| o.at <= at)
            .checked_sub(1)?;
        let o = &self.objects[i];
        (o.at.section() == at.section() && at.offset() - o.at.offset() < o.size).then_some((i, o))
    }

    /// Every marked word, sorted by location.
    pub fn markers(&self) -> &[(Location, Marker)] {
        &self.markers
    }

    pub fn marker_at(&self, at: Location) -> Option<Marker> {
        let i = self.markers.binary_search_by_key(&at, |(l, _)| *l).ok()?;
        Some(self.markers[i].1)
    }

    /// The marked words in `[start, start + len)`.
    pub fn markers_in(&self, start: Location, len: u32) -> &[(Location, Marker)] {
        let end = Location(start.0.saturating_add(len));
        let from = self.markers.partition_point(|(l, _)| *l < start);
        let to = self.markers.partition_point(|(l, _)| *l < end);
        &self.markers[from..to.max(from)]
    }

    /// The marked words inside an object's extent.
    pub fn markers_of(&self, object: &Object) -> &[(Location, Marker)] {
        self.markers_in(object.at, object.size)
    }

    /// `len` bytes at `at`, inside one data section.
    pub fn bytes(
        &self,
        at: Location,
        len: usize,
        field: &'static str,
    ) -> Result<&'a [u8], FormatError> {
        let file = self.file_offset(at, len, field)?;
        Ok(&self.bytes[file..file + len])
    }

    pub fn u32_at(&self, at: Location, field: &'static str) -> Result<u32, FormatError> {
        let b = self.bytes(at, 4, field)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn i32_at(&self, at: Location, field: &'static str) -> Result<i32, FormatError> {
        self.u32_at(at, field).map(|v| v as i32)
    }

    pub fn u16_at(&self, at: Location, field: &'static str) -> Result<u16, FormatError> {
        let b = self.bytes(at, 2, field)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn f32_at(&self, at: Location, field: &'static str) -> Result<f32, FormatError> {
        self.u32_at(at, field).map(f32::from_bits)
    }

    /// The word at `at` through the fixup marking it. An unmarked word must
    /// be zero ([`Ref::Null`]).
    pub fn reference(&self, at: Location, field: &'static str) -> Result<Ref<'a>, FormatError> {
        let value = self.u32_at(at, field)?;
        match self.marker_at(at) {
            Some(marker) => self.resolve(at, marker, value, field),
            None if value == 0 => Ok(Ref::Null),
            None => Err(self.err_at(
                at,
                field,
                ErrorKind::Unsupported {
                    value: value.into(),
                },
            )),
        }
    }

    /// The memory ref (size word, pointer) at `at`, `None` when bit 31 is
    /// clear. Present: unmarked size word, pointer-marked pointer; null:
    /// kind 0xF on the size word, an unmarked zero pointer.
    pub fn memory_ref(
        &self,
        at: Location,
        field: &'static str,
    ) -> Result<Option<MemoryRef<'a>>, FormatError> {
        let size_word = self.u32_at(at, field)?;
        let ptr_at = self.next_word(at, field)?;
        let ptr = self.u32_at(ptr_at, field)?;
        let marked = self.marker_at(ptr_at);
        let size_marker = self.marker_at(at);
        if size_word & MEMORY_PRESENT == 0 {
            return if ptr != 0 || marked.is_some() {
                Err(self.err_at(ptr_at, field, ErrorKind::Unsupported { value: ptr.into() }))
            } else if size_marker != Some(Marker::NullMemoryRef) {
                Err(self.err_at(at, field, ErrorKind::Missing))
            } else {
                Ok(None)
            };
        }
        if size_marker.is_some() {
            return Err(self.err_at(
                at,
                field,
                ErrorKind::Unsupported {
                    value: size_word.into(),
                },
            ));
        }
        if marked != Some(Marker::Pointer) {
            return Err(self.err_at(ptr_at, field, ErrorKind::Missing));
        }
        let target = Location(ptr);
        let data = self.bytes(target, (size_word & MEMORY_SIZE_MASK) as usize, field)?;
        Ok(Some(MemoryRef {
            size_word,
            at: target,
            data,
        }))
    }

    /// The string a marked word points at, `None` for an unmarked zero.
    pub fn string(
        &self,
        at: Location,
        field: &'static str,
    ) -> Result<Option<&'a str>, FormatError> {
        match self.reference(at, field)? {
            Ref::Null => Ok(None),
            Ref::String(bytes) => std::str::from_utf8(bytes)
                .map(Some)
                .map_err(|_| self.err_at(at, field, ErrorKind::InvalidUtf8)),
            _ => Err(self.err_at(
                at,
                field,
                ErrorKind::Unsupported {
                    value: self.u32_at(at, field)?.into(),
                },
            )),
        }
    }

    /// Every marked word inside an object, resolved, with its offset.
    pub fn refs_of<'s>(
        &'s self,
        object: &'s Object,
    ) -> impl Iterator<Item = (u32, Result<Ref<'a>, FormatError>)> + 's {
        self.markers_of(object).iter().map(move |&(at, marker)| {
            let resolved = self
                .u32_at(at, marker.field())
                .and_then(|value| self.resolve(at, marker, value, marker.field()));
            (at.offset() - object.at.offset(), resolved)
        })
    }

    fn resolve(
        &self,
        at: Location,
        marker: Marker,
        value: u32,
        field: &'static str,
    ) -> Result<Ref<'a>, FormatError> {
        let out_of_range = |limit: usize| {
            self.err_at(
                at,
                field,
                ErrorKind::OutOfRange {
                    value: value.into(),
                    limit: limit as u64,
                },
            )
        };
        match marker {
            Marker::Pointer => {
                let target = Location(value);
                self.file_offset(target, 1, field)?;
                Ok(match self.object_at(target) {
                    Some((i, _)) => Ref::Object(i),
                    None => Ref::Data(target),
                })
            }
            Marker::Import => {
                let i = flagged(value, self.external_ids.len())
                    .ok_or_else(|| out_of_range(self.external_ids.len()))?;
                Ok(Ref::Import {
                    index: i as u32,
                    id: self.external_ids[i],
                })
            }
            Marker::Handle => {
                let handle = self
                    .handles
                    .get(value as usize)
                    .ok_or_else(|| out_of_range(self.handles.len()))?;
                Ok(Ref::Handle {
                    index: value,
                    handle: *handle,
                })
            }
            Marker::String => {
                let target = Location(value);
                let start = self.file_offset(target, 1, field)?;
                let len = match self.string_lens.binary_search_by_key(&target, |&(l, _)| l) {
                    Ok(i) => self.string_lens[i].1 as usize,
                    Err(_) => self.cstr_at(target, start, field)?.len(),
                };
                Ok(Ref::String(&self.bytes[start..start + len]))
            }
            Marker::NullMemoryRef => Ok(Ref::NullMemoryRef),
            Marker::MetaObject if value & INDEX_FLAG == 0 => Ok(Ref::UnresolvedMeta(value)),
            Marker::MetaObject => {
                let i = flagged(value, self.external_names.len())
                    .ok_or_else(|| out_of_range(self.external_names.len()))?;
                Ok(Ref::MetaObject {
                    index: i as u32,
                    name: self.external_names[i],
                })
            }
        }
    }

    fn next_word(&self, at: Location, field: &'static str) -> Result<Location, FormatError> {
        at.offset_by(4).ok_or_else(|| {
            self.err_at(
                at,
                field,
                ErrorKind::OutOfRange {
                    value: u64::from(at.offset()) + 4,
                    limit: MAX_OFFSET.into(),
                },
            )
        })
    }

    /// Where `len` bytes at `at` start in the file, if they sit inside one
    /// data section. Zero bytes may sit at the section's end.
    fn file_offset(
        &self,
        at: Location,
        len: usize,
        field: &'static str,
    ) -> Result<usize, FormatError> {
        let Some(section) = self.sections.get(1 + at.section()) else {
            return Err(self.err_at(
                at,
                field,
                ErrorKind::OutOfRange {
                    value: at.section() as u64,
                    limit: (self.sections.len() - 1) as u64,
                },
            ));
        };
        let offset = at.offset() as usize;
        let end = offset.saturating_add(len);
        if end > section.len() {
            return Err(self.err_at(
                at,
                field,
                ErrorKind::OutOfRange {
                    value: end as u64,
                    limit: section.len() as u64,
                },
            ));
        }
        Ok(section.offset as usize + offset)
    }

    /// An error at a location's file offset (0 when its section is missing).
    pub(crate) fn err_at(&self, at: Location, field: &'static str, kind: ErrorKind) -> FormatError {
        let offset = self
            .sections
            .get(1 + at.section())
            .map_or(0, |s| s.offset as usize + at.offset() as usize);
        FormatError::new(FORMAT, field, offset, kind)
    }

    fn required(&self, kind: u32, field: &'static str) -> Result<FixupRecord<'a>, FormatError> {
        self.record(kind).copied().ok_or_else(|| {
            FormatError::new(
                FORMAT,
                field,
                self.sections[0].offset as usize,
                ErrorKind::Missing,
            )
        })
    }

    fn read_tables(&mut self) -> Result<(), FormatError> {
        let r = Reader::new(FORMAT, self.bytes);
        let tmet = self.required(kind::TMET, "TMET")?;
        self.type_names = read_strings(&r, &tmet, "TMET")?;
        // Lookups by name must be unambiguous.
        let mut seen = HashSet::with_capacity(self.type_names.len());
        if !self.type_names.iter().all(|name| seen.insert(*name)) {
            return Err(r.err("TMET", tmet.payload_at, ErrorKind::Duplicate));
        }
        let mtsz = self.required(kind::MTSZ, "MTSZ")?;
        if mtsz.count as usize != self.type_names.len() {
            return Err(r.err(
                "MTSZ",
                mtsz.at + 12,
                ErrorKind::OutOfRange {
                    value: mtsz.count.into(),
                    limit: self.type_names.len() as u64,
                },
            ));
        }
        self.type_sizes = fixed_entries(&r, &mtsz, 4, "MTSZ")?
            .map(|e| be32(e, 0))
            .collect();
        if let Some(tstr) = self.record(kind::TSTR).copied() {
            self.strings = read_strings(&r, &tstr, "TSTR")?;
        }
        if let Some(exid) = self.record(kind::EXID).copied() {
            self.external_ids = fixed_entries(&r, &exid, 8, "EXID")?
                .map(|e| ExternalId {
                    object: be32(e, 0),
                    namespace: be32(e, 4),
                })
                .collect();
        }
        if let Some(exnm) = self.record(kind::EXNM).copied() {
            let entries = fixed_entries(&r, &exnm, 8, "EXNM")?;
            let mut names = Vec::with_capacity(entries.len());
            for (j, e) in entries.enumerate() {
                let at = exnm.payload_at + j * 8;
                names.push(ExternalName {
                    namespace: self.tstr(&r, be32(e, 0), at)?,
                    name: self.tstr(&r, be32(e, 4), at + 4)?,
                });
            }
            self.external_names = names;
        }
        if let Some(tmhn) = self.record(kind::TMHN).copied() {
            let entries = fixed_entries(&r, &tmhn, 8, "TMHN")?;
            let mut handles = Vec::with_capacity(entries.len());
            for e in entries {
                let size_word = be32(e, 0);
                let at = Location(be32(e, 4));
                let data = self.bytes(at, (size_word & MEMORY_SIZE_MASK) as usize, "TMHN")?;
                handles.push(MemoryHandle {
                    size_word,
                    at,
                    data,
                });
            }
            self.handles = handles;
        }
        Ok(())
    }

    fn tstr(&self, r: &Reader<'a>, index: u32, at: usize) -> Result<&'a str, FormatError> {
        self.strings.get(index as usize).copied().ok_or_else(|| {
            r.err(
                "EXNM",
                at,
                ErrorKind::OutOfRange {
                    value: index.into(),
                    limit: self.strings.len() as u64,
                },
            )
        })
    }

    fn read_objects(&mut self) -> Result<(), FormatError> {
        let r = Reader::new(FORMAT, self.bytes);
        let rvtb = self.required(kind::RVTB, "RVTB")?;
        let locations = unpack_locations(&r, &rvtb, "RVTB")?;
        let mut objects: Vec<Object> = Vec::with_capacity(locations.len());
        for at in locations {
            let type_index = self.u32_at(at, "object_type")?;
            let Some(&size) = self.type_sizes.get(type_index as usize) else {
                return Err(self.err_at(
                    at,
                    "object_type",
                    ErrorKind::OutOfRange {
                        value: type_index.into(),
                        limit: self.type_sizes.len() as u64,
                    },
                ));
            };
            // At least the type index and the reference count, and every
            // field addressable by a location.
            let fits = size >= 8 && at.offset_by(size).is_some();
            if !fits {
                return Err(self.err_at(
                    at,
                    "MTSZ",
                    ErrorKind::OutOfRange {
                        value: size.into(),
                        limit: u64::from(MAX_OFFSET - at.offset()),
                    },
                ));
            }
            self.bytes(at, size as usize, "object")?;
            if let Some(prev) = objects.last()
                && prev.at.section() == at.section()
                && prev.at.offset() + prev.size > at.offset()
            {
                return Err(self.err_at(
                    at,
                    "object",
                    ErrorKind::OutOfRange {
                        value: at.offset().into(),
                        limit: (prev.at.offset() + prev.size).into(),
                    },
                ));
            }
            objects.push(Object {
                at,
                type_index,
                size,
            });
        }
        self.objects = objects;
        Ok(())
    }

    fn read_markers(&mut self) -> Result<(), FormatError> {
        let r = Reader::new(FORMAT, self.bytes);
        let present: Vec<(Marker, FixupRecord<'a>)> = Marker::ALL
            .iter()
            .filter_map(|m| self.record(m.kind()).map(|rec| (*m, *rec)))
            .collect();
        // Counts are capped by their payloads, so this is bounded by the input.
        let mut total = 0usize;
        for (_, rec) in &present {
            total += (rec.count as usize).min(rec.payload.len() * 2);
        }
        let mut markers = Vec::with_capacity(total);
        for (marker, rec) in present {
            let words = unpack_locations(&r, &rec, marker.field())?;
            markers.extend(words.into_iter().map(|at| (at, marker)));
        }
        self.string_lens = self.index_strings(&markers)?;
        for &(at, marker) in &markers {
            let value = self.u32_at(at, marker.field())?;
            if marker == Marker::NullMemoryRef {
                let ptr = self.u32_at(self.next_word(at, marker.field())?, marker.field())?;
                if value & MEMORY_PRESENT != 0 || ptr != 0 {
                    return Err(self.err_at(
                        at,
                        marker.field(),
                        ErrorKind::Unsupported {
                            value: value.into(),
                        },
                    ));
                }
            } else {
                self.resolve(at, marker, value, marker.field())?;
            }
        }
        markers.sort_unstable();
        if let Some(w) = markers.windows(2).find(|w| w[0].0 == w[1].0) {
            return Err(self.err_at(w[1].0, w[1].1.field(), ErrorKind::Duplicate));
        }
        self.markers = markers;
        // RVTB claims each object's type word, so no marker may sit on it.
        for o in &self.objects {
            if let Some(m) = self.marker_at(o.at) {
                return Err(self.err_at(o.at, m.field(), ErrorKind::Duplicate));
            }
        }
        // A null memory ref's pointer word can't be marked too.
        for &(at, marker) in &self.markers {
            if marker == Marker::NullMemoryRef
                && let Some(ptr_at) = at.offset_by(4)
                && self.marker_at(ptr_at).is_some()
            {
                return Err(self.err_at(ptr_at, marker.field(), ErrorKind::Duplicate));
            }
        }
        Ok(())
    }

    /// Each string target's length. Targets go in order and a NUL found
    /// ahead is reused, so a section is scanned once however many point in.
    fn index_strings(
        &self,
        markers: &[(Location, Marker)],
    ) -> Result<Vec<(Location, u32)>, FormatError> {
        let field = Marker::String.field();
        let mut targets = Vec::new();
        for &(at, marker) in markers {
            if marker == Marker::String {
                targets.push(Location(self.u32_at(at, field)?));
            }
        }
        targets.sort_unstable();
        targets.dedup();
        let mut lens = Vec::with_capacity(targets.len());
        // The section and file offset of the last NUL found.
        let mut nul: Option<(usize, usize)> = None;
        for target in targets {
            let start = self.file_offset(target, 1, field)?;
            let end = match nul {
                Some((section, n)) if section == target.section() && n >= start => n,
                _ => start + self.cstr_at(target, start, field)?.len(),
            };
            nul = Some((target.section(), end));
            lens.push((target, (end - start) as u32));
        }
        Ok(lens)
    }

    /// The NUL-terminated bytes at `target` (file offset `start`), inside
    /// its section.
    fn cstr_at(
        &self,
        target: Location,
        start: usize,
        field: &'static str,
    ) -> Result<&'a [u8], FormatError> {
        let section = &self.sections[1 + target.section()];
        let end = section.offset as usize + section.len();
        Reader::new(FORMAT, self.bytes).cstr(start, end, field)
    }
}

/// The index under [`INDEX_FLAG`], when the flag is set and it's below `limit`.
fn flagged(value: u32, limit: usize) -> Option<usize> {
    let i = (value & !INDEX_FLAG) as usize;
    (value & INDEX_FLAG != 0 && i < limit).then_some(i)
}

fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_sections(r: &Reader<'_>) -> Result<Vec<Section>, FormatError> {
    let mut sections = Vec::new();
    for i in 0..=MAX_SECTIONS {
        let at = SECTION_TABLE_AT + i * SECTION_ENTRY_LEN;
        let offset = r.u32_be(at, "section_offset")?;
        if offset == 0 {
            break;
        }
        if i == MAX_SECTIONS {
            return Err(r.err(
                "section_table",
                at,
                ErrorKind::OutOfRange {
                    value: (i + 1) as u64,
                    limit: MAX_SECTIONS as u64,
                },
            ));
        }
        let section = Section {
            offset,
            size: r.u32_be(at + 4, "section_size")?,
            alignment: r.u32_be(at + 8, "section_alignment")?,
            pool: r.u32_be(at + 12, "section_pool")?,
        };
        r.bytes(offset as usize, section.len(), "section")?;
        sections.push(section);
    }
    if sections.is_empty() {
        return Err(r.err("section_table", SECTION_TABLE_AT, ErrorKind::Missing));
    }
    Ok(sections)
}

fn read_records<'a>(
    r: &Reader<'a>,
    fixups: Section,
) -> Result<(FixupHeader, Vec<FixupRecord<'a>>), FormatError> {
    let base = fixups.offset as usize;
    if fixups.len() < FIXUP_HEADER_LEN {
        return Err(r.err(
            "fixup_header",
            base,
            ErrorKind::Truncated {
                needed: FIXUP_HEADER_LEN,
                available: fixups.len(),
            },
        ));
    }
    let end = base + fixups.len();
    r.magic(base, MAGIC, "fixup_magic")?;
    let version = r.u32_be(base + 4, "fixup_version")?;
    if version != VERSION {
        return Err(r.err(
            "fixup_version",
            base + 4,
            ErrorKind::UnsupportedVersion {
                found: version.into(),
                supported: SUPPORTED,
            },
        ));
    }
    let header = FixupHeader {
        word_08: r.u32_be(base + 8, "fixup_header")?,
        first_record: r.u32_be(base + 0xC, "first_record")?,
        record_count: r.u32_be(base + 0x10, "record_count")?,
        word_14: r.u32_be(base + 0x14, "fixup_header")?,
        word_18: r.u32_be(base + 0x18, "fixup_header")?,
    };
    let first = header.first_record as usize;
    if !(FIXUP_HEADER_LEN..=fixups.len()).contains(&first) {
        return Err(r.err(
            "first_record",
            base + 0xC,
            ErrorKind::OutOfRange {
                value: first as u64,
                limit: fixups.len() as u64,
            },
        ));
    }
    let room = (fixups.len() - first) / RECORD_HEADER_LEN;
    let count = header.record_count as usize;
    if count > room {
        return Err(r.err(
            "record_count",
            base + 0x10,
            ErrorKind::OutOfRange {
                value: count as u64,
                limit: room as u64,
            },
        ));
    }
    let mut records: Vec<FixupRecord<'a>> = Vec::with_capacity(count);
    let mut kinds = HashSet::with_capacity(count);
    let mut at = base + first;
    for _ in 0..count {
        if end - at < RECORD_HEADER_LEN {
            return Err(r.err(
                "record",
                at,
                ErrorKind::Truncated {
                    needed: RECORD_HEADER_LEN,
                    available: end - at,
                },
            ));
        }
        let kind = r.u32_be(at, "record_kind")?;
        // Zero in every Giants record; unknown, so anything else is refused.
        for word in [4, 8] {
            let value = r.u32_be(at + word, "record_header")?;
            if value != 0 {
                return Err(r.err(
                    "record_header",
                    at + word,
                    ErrorKind::Unsupported {
                        value: value.into(),
                    },
                ));
            }
        }
        let count = r.u32_be(at + 12, "record_count")?;
        let length = r.u32_be(at + 16, "record_length")? as usize;
        let start = r.u32_be(at + 20, "record_data_start")? as usize;
        if length > end - at {
            return Err(r.err(
                "record_length",
                at + 16,
                ErrorKind::OutOfRange {
                    value: length as u64,
                    limit: (end - at) as u64,
                },
            ));
        }
        if !(RECORD_HEADER_LEN..=length).contains(&start) {
            return Err(r.err(
                "record_data_start",
                at + 20,
                ErrorKind::OutOfRange {
                    value: start as u64,
                    limit: length as u64,
                },
            ));
        }
        if !kinds.insert(kind) {
            return Err(r.err("record_kind", at, ErrorKind::Duplicate));
        }
        records.push(FixupRecord {
            kind,
            count,
            at,
            payload_at: at + start,
            payload: r.bytes(at + start, length - start, "record_payload")?,
        });
        at += length;
    }
    Ok((header, records))
}

fn read_strings<'a>(
    r: &Reader<'a>,
    rec: &FixupRecord<'a>,
    field: &'static str,
) -> Result<Vec<&'a str>, FormatError> {
    let count = rec.count as usize;
    // Each string takes at least its NUL.
    if count > rec.payload.len() {
        return Err(r.err(
            field,
            rec.at + 12,
            ErrorKind::OutOfRange {
                value: count as u64,
                limit: rec.payload.len() as u64,
            },
        ));
    }
    let end = rec.payload_at + rec.payload.len();
    let mut out = Vec::with_capacity(count);
    let mut p = rec.payload_at;
    for _ in 0..count {
        let s = r.cstr(p, end, field)?;
        out.push(r.utf8(s, p, field)?);
        p += s.len() + 1;
    }
    Ok(out)
}

/// A record's first `count` entries of `width` bytes.
fn fixed_entries<'a>(
    r: &Reader<'a>,
    rec: &FixupRecord<'a>,
    width: usize,
    field: &'static str,
) -> Result<std::slice::ChunksExact<'a, u8>, FormatError> {
    let needed = (rec.count as usize).saturating_mul(width);
    rec.payload
        .get(..needed)
        .map(|entries| entries.chunks_exact(width))
        .ok_or_else(|| {
            r.err(
                field,
                rec.at + 12,
                ErrorKind::Truncated {
                    needed,
                    available: rec.payload.len(),
                },
            )
        })
}

/// A record's location list.
fn unpack_locations(
    r: &Reader<'_>,
    rec: &FixupRecord<'_>,
    field: &'static str,
) -> Result<Vec<Location>, FormatError> {
    let count = rec.count as usize;
    // Every value takes at least one nibble.
    if count > rec.payload.len() * 2 {
        return Err(r.err(
            field,
            rec.at + 12,
            ErrorKind::OutOfRange {
                value: count as u64,
                limit: rec.payload.len() as u64 * 2,
            },
        ));
    }
    decode_locations(rec.payload, count)
        .map_err(|(nibble, kind)| r.err(field, rec.payload_at + nibble / 2, kind))
}

/// `count` locations from a nibble list, or the nibble it failed at.
pub(crate) fn decode_locations(
    payload: &[u8],
    count: usize,
) -> Result<Vec<Location>, (usize, ErrorKind)> {
    let mut out = Vec::with_capacity(count);
    let mut nibble = 0usize;
    let mut running = 0u64;
    for _ in 0..count {
        let start = nibble;
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let Some(byte) = payload.get(nibble / 2) else {
                return Err((
                    start,
                    ErrorKind::Truncated {
                        needed: nibble / 2 + 1,
                        available: payload.len(),
                    },
                ));
            };
            let n = if nibble.is_multiple_of(2) {
                byte & 0xF
            } else {
                byte >> 4
            };
            nibble += 1;
            value |= u64::from(n & 7) << shift;
            shift += 3;
            if n & 8 == 0 {
                break;
            }
            // Eleven nibbles already pass any 32-bit location.
            if shift >= 33 {
                return Err((
                    start,
                    ErrorKind::OutOfRange {
                        value,
                        limit: u32::MAX.into(),
                    },
                ));
            }
        }
        running += value * 4 + 4;
        if running > u64::from(u32::MAX) {
            return Err((
                start,
                ErrorKind::OutOfRange {
                    value: running,
                    limit: u32::MAX.into(),
                },
            ));
        }
        out.push(Location(running as u32));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
