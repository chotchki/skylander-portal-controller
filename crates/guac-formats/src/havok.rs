//! Havok 2011.2 packfiles (openspec AC.8): Giants' skeletons and spline clips.
//!
//! - Packfile: big-endian, 4-byte pointers, local, global and virtual fixups.
//! - Classes ([`Container`]) at Kovah's 2011.2 offsets, pinned by signature.
//! - Clips ([`spline`]): one block, mask 0x45, little-endian; others refused.

mod classes;
pub mod qs;
pub mod spline;

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
#[cfg(test)]
mod tests;

pub use classes::{
    AnimationContainer, Binding, BlendHint, Bone, Clip, Container, RootLevelContainer, Skeleton,
    SplineAnimation, Variant,
};
pub use qs::{QsTransform, Quat};

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::reader::Reader;
use crate::{ErrorKind, FormatError};

pub(crate) const FORMAT: &str = "Havok";

/// A packfile's first 8 bytes.
pub const MAGIC: [u8; 8] = [0x57, 0xE0, 0xE0, 0x57, 0x10, 0xC0, 0xC0, 0x10];
pub const FILE_VERSION: i32 = 9;
/// `contentsVersion`, NUL included; the 16th byte is fill.
pub const VERSION: &[u8; 15] = b"hk_2011.2.0-r1\0";
/// 4-byte pointers, big-endian, reuse padding, empty base.
pub const LAYOUT_RULES: [u8; 4] = [4, 0, 1, 1];
pub const HEADER_LEN: usize = 0x40;
pub const SECTION_HEADER_LEN: usize = 0x30;
pub const CLASSNAMES: &str = "__classnames__";
/// Class name entries: u32 signature, this byte, then the name.
pub const CLASS_SEPARATOR: u8 = 0x09;

/// The classes read here and their 2011.2 signatures (a layout hash).
pub const SIGNATURES: [(&str, u32); 5] = [
    (RootLevelContainer::CLASS, 0x2772_C11E),
    (AnimationContainer::CLASS, 0x8DC2_0333),
    (Skeleton::CLASS, 0x2F13_03FA),
    (SplineAnimation::CLASS, 0x1588_DB14),
    (Binding::CLASS, 0x66EA_C971),
];

pub(crate) fn err(field: &'static str, at: usize, kind: ErrorKind) -> FormatError {
    FormatError::new(FORMAT, field, at, kind)
}

pub(crate) fn unsupported(field: &'static str, at: usize, value: u64) -> FormatError {
    err(field, at, ErrorKind::Unsupported { value })
}

/// A place in a section: what fixups point at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ptr {
    pub section: usize,
    /// From the section's start.
    pub offset: usize,
}

impl Ptr {
    pub fn plus(self, n: usize) -> Self {
        Self {
            section: self.section,
            offset: self.offset.saturating_add(n),
        }
    }
}

/// A section header and its object data (the bytes before its fixups).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section<'a> {
    pub tag: &'a str,
    /// Absolute.
    pub start: usize,
    pub data: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FixupCounts {
    pub local: usize,
    pub global: usize,
    pub virtual_: usize,
}

/// A `__classnames__` entry: virtual fixups point at its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassName<'a> {
    pub name: &'a str,
    pub signature: u32,
}

/// Header, sections, class names and fixups, resolved.
#[derive(Debug, Clone)]
pub struct Packfile<'a> {
    bytes: &'a [u8],
    pub sections: Vec<Section<'a>>,
    /// Keyed by the name's offset in `__classnames__`.
    pub classes: BTreeMap<usize, ClassName<'a>>,
    pub fixups: FixupCounts,
    /// The hkRootLevelContainer.
    pub root: Ptr,
    pointers: HashMap<Ptr, Ptr>,
    objects: BTreeMap<Ptr, &'a str>,
}

fn i32_at(r: &Reader<'_>, at: usize, field: &'static str) -> Result<i32, FormatError> {
    r.u32_be(at, field).map(|v| v as i32)
}

/// A non-negative i32 below `limit`.
fn below(value: i32, limit: usize, field: &'static str, at: usize) -> Result<usize, FormatError> {
    usize::try_from(value)
        .ok()
        .filter(|v| *v < limit)
        .ok_or_else(|| {
            err(
                field,
                at,
                ErrorKind::OutOfRange {
                    value: u64::from(value as u32),
                    limit: limit as u64,
                },
            )
        })
}

/// A section's offsets, relative to its start.
#[derive(Clone, Copy)]
struct Tables {
    start: usize,
    local: usize,
    global: usize,
    virtual_: usize,
    exports: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fixup {
    Local,
    Global,
    Virtual,
}

impl<'a> Packfile<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FormatError> {
        let r = Reader::new(FORMAT, bytes);
        r.magic(0, &MAGIC, "magic")?;
        let file_version = i32_at(&r, 0x0C, "fileVersion")?;
        if file_version != FILE_VERSION {
            return Err(err(
                "fileVersion",
                0x0C,
                ErrorKind::UnsupportedVersion {
                    found: u64::from(file_version as u32),
                    supported: "packfile version 9",
                },
            ));
        }
        let rules = r.bytes(0x10, 4, "layoutRules")?;
        if rules != LAYOUT_RULES {
            let value = u32::from_be_bytes([rules[0], rules[1], rules[2], rules[3]]);
            return Err(unsupported("layoutRules", 0x10, u64::from(value)));
        }
        r.magic(0x28, VERSION, "contentsVersion")?;
        let flags = r.u32_be(0x38, "flags")?;
        if flags != 0 {
            return Err(unsupported("flags", 0x38, u64::from(flags)));
        }
        let pred = r.bytes(0x3C, 2, "maxPredicate")?;
        if pred != [0xFF, 0xFF] {
            let value = u16::from_be_bytes([pred[0], pred[1]]);
            return Err(unsupported("maxPredicate", 0x3C, u64::from(value)));
        }

        let room = bytes.len().saturating_sub(HEADER_LEN) / SECTION_HEADER_LEN + 1;
        let count = below(i32_at(&r, 0x14, "numSections")?, room, "numSections", 0x14)?;
        let mut sections = Vec::with_capacity(count);
        let mut tables = Vec::with_capacity(count);
        let mut prev_end = HEADER_LEN + count * SECTION_HEADER_LEN;
        for i in 0..count {
            let o = HEADER_LEN + i * SECTION_HEADER_LEN;
            let tag = r.utf8(r.cstr(o, o + 19, "sectionTag")?, o, "sectionTag")?;
            let mut w = [0usize; 7];
            for (k, slot) in w.iter_mut().enumerate() {
                let at = o + 20 + 4 * k;
                let v = i32_at(&r, at, "section offsets")?;
                *slot = below(v, bytes.len() + 1, "section offsets", at)?;
            }
            let [start, local, global, virtual_, exports, imports, end] = w;
            if start < prev_end {
                let kind = ErrorKind::OutOfRange {
                    value: start as u64,
                    limit: prev_end as u64,
                };
                return Err(err("absoluteDataStart", o + 20, kind));
            }
            if !(local <= global && global <= virtual_ && virtual_ <= exports) {
                return Err(unsupported("fixup offsets", o + 24, virtual_ as u64));
            }
            if exports != end || imports != end {
                return Err(unsupported("exportsOffset", o + 36, exports as u64));
            }
            let section = r.bytes(start, end, "section")?;
            prev_end = start + end;
            sections.push(Section {
                tag,
                start,
                data: &section[..local],
            });
            tables.push(Tables {
                start,
                local,
                global,
                virtual_,
                exports,
            });
        }
        for (i, s) in sections.iter().enumerate() {
            if sections[..i].iter().any(|p| p.tag == s.tag) {
                let at = HEADER_LEN + i * SECTION_HEADER_LEN;
                return Err(err("sectionTag", at, ErrorKind::Duplicate));
            }
        }
        let names = sections
            .iter()
            .position(|s| s.tag == CLASSNAMES)
            .ok_or_else(|| err(CLASSNAMES, HEADER_LEN, ErrorKind::Missing))?;
        let classes = Self::class_names(&r, sections[names])?;

        let mut pf = Self {
            bytes,
            sections,
            classes,
            fixups: FixupCounts::default(),
            root: Ptr {
                section: 0,
                offset: 0,
            },
            pointers: HashMap::new(),
            objects: BTreeMap::new(),
        };
        for (si, t) in tables.iter().enumerate() {
            let s = t.start;
            pf.fixups_in(si, s + t.local, t.global - t.local, Fixup::Local, names)?;
            pf.fixups_in(
                si,
                s + t.global,
                t.virtual_ - t.global,
                Fixup::Global,
                names,
            )?;
            pf.fixups_in(
                si,
                s + t.virtual_,
                t.exports - t.virtual_,
                Fixup::Virtual,
                names,
            )?;
        }

        let v = i32_at(&r, 0x18, "contentsSectionIndex")?;
        let section = below(v, count, "contentsSectionIndex", 0x18)?;
        let limit = pf.sections[section].data.len();
        let v = i32_at(&r, 0x1C, "contentsSectionOffset")?;
        let offset = below(v, limit, "contentsSectionOffset", 0x1C)?;
        pf.root = Ptr { section, offset };
        let cn_section = r.u32_be(0x20, "contentsClassNameSectionIndex")?;
        if cn_section as usize != names {
            let field = "contentsClassNameSectionIndex";
            return Err(unsupported(field, 0x20, u64::from(cn_section)));
        }
        let cn = r.u32_be(0x24, "contentsClassNameSectionOffset")? as usize;
        let named = pf.classes.get(&cn).map(|c| c.name);
        for (field, at, found) in [
            ("contentsClassNameSectionOffset", 0x24, named),
            ("contentsSectionOffset", 0x1C, pf.class_of(pf.root)),
        ] {
            if found != Some(RootLevelContainer::CLASS) {
                let kind = ErrorKind::WrongType {
                    expected: RootLevelContainer::CLASS,
                    found: found.unwrap_or("no object").to_owned(),
                };
                return Err(err(field, at, kind));
            }
        }
        Ok(pf)
    }

    /// `u32 signature, 0x09, name\0` up to 0xFF padding. Names are unique;
    /// the classes read here must carry their 2011.2 signatures.
    fn class_names(
        r: &Reader<'a>,
        s: Section<'a>,
    ) -> Result<BTreeMap<usize, ClassName<'a>>, FormatError> {
        let mut out = BTreeMap::new();
        let mut seen = HashSet::new();
        let mut pos = 0;
        while pos + 5 <= s.data.len() && s.data[pos..pos + 4] != [0xFF; 4] {
            let at = s.start + pos;
            let signature = r.u32_be(at, "class signature")?;
            if s.data[pos + 4] != CLASS_SEPARATOR {
                let found = u64::from(s.data[pos + 4]);
                return Err(unsupported("class separator", at + 4, found));
            }
            let raw = r.cstr(at + 5, s.start + s.data.len(), "class name")?;
            let name = r.utf8(raw, at + 5, "class name")?;
            if !seen.insert(name) {
                return Err(err("class name", at + 5, ErrorKind::Duplicate));
            }
            if let Some((_, want)) = SIGNATURES.iter().find(|(n, _)| *n == name)
                && *want != signature
            {
                return Err(unsupported("class signature", at, u64::from(signature)));
            }
            out.insert(pos + 5, ClassName { name, signature });
            pos += 5 + raw.len() + 1;
        }
        padding(&s.data[pos..], s.start + pos, "class name padding")?;
        Ok(out)
    }

    /// One fixup table. Local entries are `(src, dst)`, global `(src,
    /// section, dst)`, virtual `(src, section, class name)`; a `src` of -1
    /// ends the table and the rest is 0xFF.
    fn fixups_in(
        &mut self,
        si: usize,
        at: usize,
        len: usize,
        kind: Fixup,
        names: usize,
    ) -> Result<(), FormatError> {
        let words = if kind == Fixup::Local { 2 } else { 3 };
        let r = Reader::new(FORMAT, self.bytes);
        let data_len = self.sections[si].data.len();
        let mut pos = 0;
        while pos + 4 * words <= len {
            let e = at + pos;
            let w = |k: usize| i32_at(&r, e + 4 * k, "fixup");
            if w(0)? == -1 {
                break;
            }
            let src = below(w(0)?, data_len.saturating_sub(3), "fixup src", e)?;
            if src % 4 != 0 {
                return Err(unsupported("fixup src", e, src as u64));
            }
            let src = Ptr {
                section: si,
                offset: src,
            };
            let duplicate = match kind {
                Fixup::Local | Fixup::Global => {
                    let (section, dst_word) = if kind == Fixup::Local {
                        (si, 1)
                    } else {
                        let n = self.sections.len();
                        (below(w(1)?, n, "fixup section", e + 4)?, 2)
                    };
                    let limit = self.sections[section].data.len();
                    let dst_at = e + 4 * dst_word;
                    let offset = below(w(dst_word)?, limit, "fixup dst", dst_at)?;
                    if kind == Fixup::Local {
                        self.fixups.local += 1;
                    } else {
                        self.fixups.global += 1;
                    }
                    self.pointers.insert(src, Ptr { section, offset }).is_some()
                }
                Fixup::Virtual => {
                    let section = w(1)? as u32;
                    if section as usize != names {
                        let value = u64::from(section);
                        return Err(unsupported("virtual fixup section", e + 4, value));
                    }
                    let name = w(2)? as u32;
                    let class = self.classes.get(&(name as usize)).ok_or_else(|| {
                        unsupported("virtual fixup class", e + 8, u64::from(name))
                    })?;
                    self.fixups.virtual_ += 1;
                    self.objects.insert(src, class.name).is_some()
                }
            };
            if duplicate {
                return Err(err("fixup src", e, ErrorKind::Duplicate));
            }
            pos += 4 * words;
        }
        padding(&self.bytes[at + pos..at + len], at + pos, "fixup padding")
    }

    pub fn abs(&self, at: Ptr) -> usize {
        self.sections[at.section].start.saturating_add(at.offset)
    }

    /// Every object a virtual fixup names, with its class.
    pub fn objects(&self) -> impl Iterator<Item = (Ptr, &'a str)> + '_ {
        self.objects.iter().map(|(p, c)| (*p, *c))
    }

    pub fn class_of(&self, at: Ptr) -> Option<&'a str> {
        self.objects.get(&at).copied()
    }

    /// `len` bytes at `at + offset`, inside the section's data.
    pub fn bytes(
        &self,
        at: Ptr,
        offset: usize,
        len: usize,
        field: &'static str,
    ) -> Result<&'a [u8], FormatError> {
        let data = self.sections[at.section].data;
        let from = at.offset.saturating_add(offset);
        from.checked_add(len)
            .and_then(|end| data.get(from..end))
            .ok_or_else(|| {
                let kind = ErrorKind::Truncated {
                    needed: len,
                    available: data.len().saturating_sub(from),
                };
                err(field, self.abs(at.plus(offset)), kind)
            })
    }

    pub fn u8(&self, at: Ptr, offset: usize, field: &'static str) -> Result<u8, FormatError> {
        Ok(self.bytes(at, offset, 1, field)?[0])
    }

    pub fn i16(&self, at: Ptr, offset: usize, field: &'static str) -> Result<i16, FormatError> {
        let b = self.bytes(at, offset, 2, field)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    pub fn u32(&self, at: Ptr, offset: usize, field: &'static str) -> Result<u32, FormatError> {
        let b = self.bytes(at, offset, 4, field)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn i32(&self, at: Ptr, offset: usize, field: &'static str) -> Result<i32, FormatError> {
        self.u32(at, offset, field).map(|v| v as i32)
    }

    pub fn f32(&self, at: Ptr, offset: usize, field: &'static str) -> Result<f32, FormatError> {
        self.u32(at, offset, field).map(f32::from_bits)
    }

    /// A pointer field: the word is zero in the file and its fixup, if any,
    /// is the pointer.
    pub fn pointer(
        &self,
        at: Ptr,
        offset: usize,
        field: &'static str,
    ) -> Result<Option<Ptr>, FormatError> {
        let word = self.u32(at, offset, field)?;
        if word != 0 {
            let place = self.abs(at.plus(offset));
            return Err(unsupported(field, place, u64::from(word)));
        }
        Ok(self.pointers.get(&at.plus(offset)).copied())
    }

    /// An `hkStringPtr`: NUL-terminated inside the section it points into.
    pub fn string(
        &self,
        at: Ptr,
        offset: usize,
        field: &'static str,
    ) -> Result<Option<&'a str>, FormatError> {
        let Some(p) = self.pointer(at, offset, field)? else {
            return Ok(None);
        };
        let s = self.sections[p.section];
        let r = Reader::new(FORMAT, self.bytes);
        let start = s.start + p.offset;
        let raw = r.cstr(start, s.start + s.data.len(), field)?;
        r.utf8(raw, start, field).map(Some)
    }
}

fn padding(bytes: &[u8], at: usize, field: &'static str) -> Result<(), FormatError> {
    match bytes.iter().position(|b| *b != 0xFF) {
        Some(i) => Err(unsupported(field, at + i, u64::from(bytes[i]))),
        None => Ok(()),
    }
}
