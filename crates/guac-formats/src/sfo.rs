//! PARAM.SFO, the key/value table at the root of a PS3 game's `PS3_GAME/`
//! that says which game and which version a dump is.
//!
//! Layout, little-endian throughout:
//! - a 0x14-byte header: magic `\0PSF`, format version, key-table start,
//!   data-table start, entry count;
//! - one 0x10-byte index entry per key: key offset (u16, into the key table),
//!   value format (u16), used length, max length, data offset (into the data
//!   table);
//! - the key table, NUL-terminated ASCII names;
//! - the data table, each value padded to its max length.
//!
//! Three value formats: `0x0204` UTF-8 with its NUL inside the used length,
//! `0x0404` a u32, and `0x0004` "UTF-8 special", raw bytes with no NUL (the
//! console stores account ids and save-data blobs that way, not always text).

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

use std::collections::BTreeSet;

use crate::reader::Reader;
use crate::{ErrorKind, FormatError};

const FORMAT: &str = "PARAM.SFO";
pub const MAGIC: &[u8; 4] = b"\0PSF";
pub const HEADER_LEN: usize = 0x14;
pub const INDEX_ENTRY_LEN: usize = 0x10;

pub const FMT_UTF8_SPECIAL: u16 = 0x0004;
pub const FMT_UTF8: u16 = 0x0204;
pub const FMT_U32: u16 = 0x0404;

/// The keys Guacamole reads. TITLE_ID and APP_VER make the dump identity
/// (design decision 4); VERSION and CATEGORY tell a disc from an update.
pub mod keys {
    pub const TITLE_ID: &str = "TITLE_ID";
    pub const APP_VER: &str = "APP_VER";
    pub const VERSION: &str = "VERSION";
    pub const CATEGORY: &str = "CATEGORY";
}

/// Where index entry `i` starts.
pub const fn index_entry_offset(i: usize) -> usize {
    HEADER_LEN + i * INDEX_ENTRY_LEN
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SfoValue {
    Utf8(String),
    U32(u32),
    /// `0x0004` values, as stored.
    Utf8Special(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfoEntry {
    pub key: String,
    pub value: SfoValue,
}

/// A parsed PARAM.SFO, entries in file order. Keys are unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSfo {
    format_version: u32,
    entries: Vec<SfoEntry>,
}

impl ParamSfo {
    pub fn parse(bytes: &[u8]) -> Result<Self, FormatError> {
        let r = Reader::new(FORMAT, bytes);
        r.magic(0, MAGIC, "magic")?;
        let format_version = r.u32_le(4, "version")?;
        let key_table = r.u32_le(8, "key_table_start")? as usize;
        let data_table = r.u32_le(12, "data_table_start")? as usize;
        let count = r.u32_le(16, "tables_entries")? as usize;

        // The index must fit in the input before `count` sizes anything.
        let fits = (bytes.len().saturating_sub(HEADER_LEN)) / INDEX_ENTRY_LEN;
        if count > fits {
            return Err(r.err(
                "index entry",
                index_entry_offset(fits),
                ErrorKind::Truncated {
                    needed: INDEX_ENTRY_LEN,
                    available: bytes.len().saturating_sub(index_entry_offset(fits)),
                },
            ));
        }
        let index_end = index_entry_offset(count);
        if key_table < index_end || key_table > r.len() {
            return Err(r.err(
                "key_table_start",
                8,
                ErrorKind::OutOfRange {
                    value: key_table as u64,
                    limit: r.len() as u64,
                },
            ));
        }
        if data_table < key_table || data_table > r.len() {
            return Err(r.err(
                "data_table_start",
                12,
                ErrorKind::OutOfRange {
                    value: data_table as u64,
                    limit: r.len() as u64,
                },
            ));
        }

        let mut entries = Vec::with_capacity(count);
        let mut seen = BTreeSet::new();
        for i in 0..count {
            let at = index_entry_offset(i);
            let key_offset = r.u16_le(at, "key_offset")? as usize;
            let fmt = r.u16_le(at + 2, "data_fmt")?;
            let used = r.u32_le(at + 4, "data_len")? as usize;
            let max = r.u32_le(at + 8, "data_max_len")? as usize;
            let data_offset = r.u32_le(at + 12, "data_offset")? as usize;

            // Keys live between the two table starts.
            let key_at = key_table + key_offset;
            if key_at >= data_table {
                return Err(r.err(
                    "key_offset",
                    at,
                    ErrorKind::OutOfRange {
                        value: key_offset as u64,
                        limit: (data_table - key_table) as u64,
                    },
                ));
            }
            let key = r.cstr(key_at, data_table, "key")?;
            let key = r.utf8(key, key_at, "key")?;
            if !seen.insert(key) {
                return Err(r.err("key", key_at, ErrorKind::Duplicate));
            }

            if used > max {
                return Err(r.err(
                    "data_len",
                    at + 4,
                    ErrorKind::OutOfRange {
                        value: used as u64,
                        limit: max as u64,
                    },
                ));
            }
            // checked: on a 32-bit target the sum could wrap back into range.
            let Some(value_at) = data_table
                .checked_add(data_offset)
                .filter(|at| *at <= r.len())
            else {
                return Err(r.err(
                    "data_offset",
                    at + 12,
                    ErrorKind::OutOfRange {
                        value: data_offset as u64,
                        limit: (r.len() - data_table) as u64,
                    },
                ));
            };
            let raw = r.bytes(value_at, used, "value")?;

            let value = match fmt {
                FMT_UTF8 => match raw.split_last() {
                    Some((0, text)) => SfoValue::Utf8(r.utf8(text, value_at, "value")?.to_owned()),
                    _ => {
                        return Err(r.err(
                            "value",
                            value_at,
                            ErrorKind::MissingNul {
                                end: value_at + used,
                            },
                        ));
                    }
                },
                FMT_U32 => match raw {
                    &[a, b, c, d] => SfoValue::U32(u32::from_le_bytes([a, b, c, d])),
                    _ => {
                        return Err(r.err(
                            "data_len",
                            at + 4,
                            ErrorKind::OutOfRange {
                                value: used as u64,
                                limit: 4,
                            },
                        ));
                    }
                },
                FMT_UTF8_SPECIAL => SfoValue::Utf8Special(raw.to_vec()),
                other => {
                    return Err(r.err(
                        "data_fmt",
                        at + 2,
                        ErrorKind::Unsupported {
                            value: other.into(),
                        },
                    ));
                }
            };
            entries.push(SfoEntry {
                key: key.to_owned(),
                value,
            });
        }
        Ok(Self {
            format_version,
            entries,
        })
    }

    /// The header's format version (0x0101 on every PS3 disc), not the
    /// `VERSION` key.
    pub fn format_version(&self) -> u32 {
        self.format_version
    }

    pub fn entries(&self) -> &[SfoEntry] {
        &self.entries
    }

    pub fn get(&self, key: &str) -> Option<&SfoValue> {
        self.entries.iter().find(|e| e.key == key).map(|e| &e.value)
    }

    /// A `0x0204` value; `None` when the key is missing or holds another format.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            SfoValue::Utf8(s) => Some(s),
            _ => None,
        }
    }

    pub fn get_u32(&self, key: &str) -> Option<u32> {
        match self.get(key)? {
            SfoValue::U32(v) => Some(*v),
            _ => None,
        }
    }

    /// `BLUS30968` for Giants (US).
    pub fn title_id(&self) -> Option<&str> {
        self.get_str(keys::TITLE_ID)
    }

    /// The disc's application version, `01.00` for every Skylanders disc
    /// here; a title update raises it.
    pub fn app_ver(&self) -> Option<&str> {
        self.get_str(keys::APP_VER)
    }

    pub fn version(&self) -> Option<&str> {
        self.get_str(keys::VERSION)
    }

    /// `DG` for a disc game.
    pub fn category(&self) -> Option<&str> {
        self.get_str(keys::CATEGORY)
    }
}

#[cfg(test)]
mod tests {
    use super::builder::SfoBuilder;
    use super::*;

    fn u32_at(bytes: &[u8], at: usize) -> usize {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
    }

    /// Absolute offset of entry `i`'s value in a built file.
    fn value_offset(bytes: &[u8], i: usize) -> usize {
        u32_at(bytes, 12) + u32_at(bytes, index_entry_offset(i) + 12)
    }

    fn sample() -> Vec<u8> {
        SfoBuilder::disc_game("BLUS30968", "01.00", "01.00")
            .u32("PARENTAL_LEVEL", 5)
            .utf8_special("SAVE_BLOB", &[0xff, 0x00, 0x80])
            .build()
    }

    #[test]
    fn all_four_keys_read_back() {
        let sfo = ParamSfo::parse(&sample()).unwrap();
        assert_eq!(sfo.title_id(), Some("BLUS30968"));
        assert_eq!(sfo.app_ver(), Some("01.00"));
        assert_eq!(sfo.version(), Some("01.00"));
        assert_eq!(sfo.category(), Some("DG"));
        assert_eq!(sfo.get_u32("PARENTAL_LEVEL"), Some(5));
        assert_eq!(
            sfo.get("SAVE_BLOB"),
            Some(&SfoValue::Utf8Special(vec![0xff, 0x00, 0x80]))
        );
        assert_eq!(sfo.format_version(), 0x0101);
        assert_eq!(sfo.entries().len(), 6);
        // Wrong-format and missing keys read as None, not a panic.
        assert_eq!(sfo.get_u32(keys::TITLE_ID), None);
        assert_eq!(sfo.get_str("PARENTAL_LEVEL"), None);
        assert_eq!(sfo.get("TITLE"), None);
    }

    #[test]
    fn padded_values_and_an_empty_table_parse() {
        let bytes = SfoBuilder::new()
            .utf8_padded(keys::TITLE_ID, "BLUS30968", 16)
            .utf8(keys::APP_VER, "")
            .build();
        let sfo = ParamSfo::parse(&bytes).unwrap();
        assert_eq!(sfo.title_id(), Some("BLUS30968"));
        assert_eq!(sfo.app_ver(), Some(""));
        assert!(
            ParamSfo::parse(&SfoBuilder::new().build())
                .unwrap()
                .entries()
                .is_empty()
        );
    }

    #[test]
    fn a_truncated_index_table_errors_at_the_first_missing_entry() {
        let mut bytes = sample();
        // Entries 0 and 1 whole, entry 2 half there.
        bytes.truncate(index_entry_offset(2) + 8);
        let err = ParamSfo::parse(&bytes).unwrap_err();
        assert_eq!((err.field, err.offset), ("index entry", 0x34));
        assert!(err.to_string().contains("offset 0x34"), "{err}");
    }

    #[test]
    fn a_truncated_data_table_errors_at_the_value() {
        let mut bytes = sample();
        let last = value_offset(&bytes, 5);
        bytes.truncate(last + 1);
        let err = ParamSfo::parse(&bytes).unwrap_err();
        assert_eq!((err.field, err.offset), ("value", last));
        assert!(matches!(err.kind, ErrorKind::Truncated { .. }), "{err}");
    }

    #[test]
    fn an_out_of_range_key_offset_errors_at_its_index_entry() {
        let mut bytes = sample();
        let at = index_entry_offset(1);
        bytes[at..at + 2].copy_from_slice(&0x7fffu16.to_le_bytes());
        let err = ParamSfo::parse(&bytes).unwrap_err();
        assert_eq!((err.field, err.offset), ("key_offset", 0x24));
        assert!(matches!(
            err.kind,
            ErrorKind::OutOfRange { value: 0x7fff, .. }
        ));
        assert!(err.to_string().contains("offset 0x24"), "{err}");
    }

    #[test]
    fn a_value_missing_its_nul_errors_at_the_value() {
        let mut bytes = sample();
        // Entry 2 is TITLE_ID; its used length ends on the NUL.
        let at = value_offset(&bytes, 2);
        let used = u32_at(&bytes, index_entry_offset(2) + 4);
        bytes[at + used - 1] = b'X';
        let err = ParamSfo::parse(&bytes).unwrap_err();
        assert_eq!((err.field, err.offset), ("value", at));
        assert_eq!(err.kind, ErrorKind::MissingNul { end: at + used });
        assert!(err.to_string().contains(&format!("{at:#x}")), "{err}");
    }

    #[test]
    fn a_key_missing_its_nul_errors_at_the_key() {
        let mut bytes = sample();
        let key_table = u32_at(&bytes, 8);
        let data_table = u32_at(&bytes, 12);
        // The last key runs to the data table with no NUL anywhere after it.
        let last_key = key_table
            + usize::from(u16::from_le_bytes(
                bytes[index_entry_offset(5)..index_entry_offset(5) + 2]
                    .try_into()
                    .unwrap(),
            ));
        bytes[last_key..data_table].fill(b'K');
        let err = ParamSfo::parse(&bytes).unwrap_err();
        assert_eq!((err.field, err.offset), ("key", last_key));
        assert_eq!(err.kind, ErrorKind::MissingNul { end: data_table });
    }

    #[test]
    fn malformed_headers_and_entries_are_refused() {
        let good = sample();
        let patched = |at: usize, with: &[u8]| {
            let mut b = good.clone();
            b[at..at + with.len()].copy_from_slice(with);
            ParamSfo::parse(&b).unwrap_err()
        };
        let e0 = index_entry_offset(0);

        let err = patched(0, b"\0PSG");
        assert_eq!((err.field, err.offset), ("magic", 0));
        // Key table starting inside the index.
        let err = patched(8, &4u32.to_le_bytes());
        assert_eq!((err.field, err.offset), ("key_table_start", 8));
        let err = patched(12, &u32::MAX.to_le_bytes());
        assert_eq!((err.field, err.offset), ("data_table_start", 12));
        // An entry count no input could hold.
        let err = patched(16, &u32::MAX.to_le_bytes());
        assert_eq!(err.field, "index entry");
        // Unknown value format.
        let err = patched(e0 + 2, &0x0104u16.to_le_bytes());
        assert_eq!((err.field, err.offset), ("data_fmt", e0 + 2));
        assert_eq!(err.kind, ErrorKind::Unsupported { value: 0x104 });
        let err = patched(e0 + 4, &0x1000u32.to_le_bytes());
        assert_eq!((err.field, err.offset), ("data_len", e0 + 4));
        let err = patched(e0 + 12, &0x10_0000u32.to_le_bytes());
        assert_eq!((err.field, err.offset), ("data_offset", e0 + 12));
        // A u32 value that isn't 4 bytes (entry 4 is PARENTAL_LEVEL).
        let e4 = index_entry_offset(4);
        let err = patched(e4 + 4, &2u32.to_le_bytes());
        assert_eq!((err.field, err.offset), ("data_len", e4 + 4));
        // Two entries naming the same key.
        let key0 = &good[index_entry_offset(0)..index_entry_offset(0) + 2];
        let err = patched(index_entry_offset(1), key0);
        assert_eq!(err.kind, ErrorKind::Duplicate);
    }

    #[test]
    fn every_prefix_of_a_good_file_errors_cleanly() {
        let bytes = sample();
        for len in 0..bytes.len() {
            // Some prefixes still parse (the padding at the end is optional
            // to the parser); none may panic.
            let _ = ParamSfo::parse(&bytes[..len]);
        }
    }
}
