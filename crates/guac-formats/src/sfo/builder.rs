//! Writes PARAM.SFO files in code, for tests that need one (`test-fixtures`
//! feature: this crate's tests, AA.5's dump scan, AA.10's setup). No real SFO
//! is ever committed; the game-magic guard fails on one.

use super::{FMT_U32, FMT_UTF8, FMT_UTF8_SPECIAL, HEADER_LEN, INDEX_ENTRY_LEN, MAGIC, keys};

enum Value {
    Utf8(String),
    U32(u32),
    Special(Vec<u8>),
}

/// Entries are written in the order they're added (real files sort keys; the
/// parser doesn't care). Each value is padded to its max length, rounded up to
/// 4 bytes unless [`SfoBuilder::utf8_padded`] sets it.
#[derive(Default)]
pub struct SfoBuilder {
    entries: Vec<(String, Value, Option<u32>)>,
}

fn align4(n: usize) -> usize {
    n.next_multiple_of(4)
}

impl SfoBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The four keys Guacamole reads, sorted as a disc's PARAM.SFO sorts them,
    /// with CATEGORY `DG` (disc game).
    pub fn disc_game(title_id: &str, app_ver: &str, version: &str) -> Self {
        Self::new()
            .utf8(keys::APP_VER, app_ver)
            .utf8(keys::CATEGORY, "DG")
            .utf8(keys::TITLE_ID, title_id)
            .utf8(keys::VERSION, version)
    }

    pub fn utf8(mut self, key: &str, value: &str) -> Self {
        self.entries
            .push((key.into(), Value::Utf8(value.into()), None));
        self
    }

    /// A UTF-8 value in a slot of `max_len` bytes (at least its length + NUL).
    pub fn utf8_padded(mut self, key: &str, value: &str, max_len: u32) -> Self {
        assert!(max_len as usize > value.len(), "max_len must hold the NUL");
        self.entries
            .push((key.into(), Value::Utf8(value.into()), Some(max_len)));
        self
    }

    pub fn u32(mut self, key: &str, value: u32) -> Self {
        self.entries.push((key.into(), Value::U32(value), None));
        self
    }

    pub fn utf8_special(mut self, key: &str, value: &[u8]) -> Self {
        self.entries
            .push((key.into(), Value::Special(value.to_vec()), None));
        self
    }

    pub fn build(&self) -> Vec<u8> {
        let mut keys = Vec::new();
        let mut data = Vec::new();
        let mut index = Vec::new();
        for (key, value, max) in &self.entries {
            let (fmt, bytes) = match value {
                Value::Utf8(s) => (FMT_UTF8, [s.as_bytes(), &[0]].concat()),
                Value::U32(v) => (FMT_U32, v.to_le_bytes().to_vec()),
                Value::Special(b) => (FMT_UTF8_SPECIAL, b.clone()),
            };
            let max = max.map_or(align4(bytes.len()), |m| m as usize);
            index.extend_from_slice(&(keys.len() as u16).to_le_bytes());
            index.extend_from_slice(&fmt.to_le_bytes());
            index.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            index.extend_from_slice(&(max as u32).to_le_bytes());
            index.extend_from_slice(&(data.len() as u32).to_le_bytes());
            keys.extend_from_slice(key.as_bytes());
            keys.push(0);
            data.extend_from_slice(&bytes);
            data.resize(data.len() + max - bytes.len(), 0);
        }
        keys.resize(align4(keys.len()), 0);

        let key_table = HEADER_LEN + self.entries.len() * INDEX_ENTRY_LEN;
        let data_table = key_table + keys.len();
        let mut out = Vec::with_capacity(data_table + data.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&0x0101u32.to_le_bytes());
        out.extend_from_slice(&(key_table as u32).to_le_bytes());
        out.extend_from_slice(&(data_table as u32).to_le_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&index);
        out.extend_from_slice(&keys);
        out.extend_from_slice(&data);
        out
    }
}

/// A file whose index entries alias, which [`SfoBuilder`] never writes:
/// `entries` keys that are the suffixes of one run of `entries` `A`s, each with
/// a `0x0004` value spanning the whole `data_len`-byte data table. Every entry
/// fits the input on its own; together they'd copy about
/// `entries * (entries / 2 + data_len)` bytes, so past a few entries the parser
/// must refuse it.
pub fn aliasing(entries: usize, data_len: usize) -> Vec<u8> {
    let key_table = HEADER_LEN + entries * INDEX_ENTRY_LEN;
    let data_table = key_table + entries + 1;
    let mut out = Vec::with_capacity(data_table + data_len);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&0x0101u32.to_le_bytes());
    out.extend_from_slice(&(key_table as u32).to_le_bytes());
    out.extend_from_slice(&(data_table as u32).to_le_bytes());
    out.extend_from_slice(&(entries as u32).to_le_bytes());
    for i in 0..entries {
        out.extend_from_slice(&(i as u16).to_le_bytes()); // key i = entries - i `A`s
        out.extend_from_slice(&FMT_UTF8_SPECIAL.to_le_bytes());
        out.extend_from_slice(&(data_len as u32).to_le_bytes()); // used
        out.extend_from_slice(&(data_len as u32).to_le_bytes()); // max
        out.extend_from_slice(&0u32.to_le_bytes()); // data offset
    }
    out.resize(data_table - 1, b'A');
    out.push(0);
    out.resize(data_table + data_len, 0);
    out
}

/// The property test's seeds: well-formed files covering every value format,
/// padding, the empty table and two entries sharing one value (so mutation
/// starts from an aliasing table).
pub fn seeds() -> Vec<Vec<u8>> {
    vec![
        SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
        SfoBuilder::new()
            .u32("ATTRIBUTE", 0x20)
            .utf8_padded("TITLE", "A Title", 128)
            .utf8_special("BLOB", &[0, 1, 2, 3, 4])
            .utf8("EMPTY", "")
            .build(),
        SfoBuilder::new().build(),
        aliasing(2, 16),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sfo::ParamSfo;

    #[test]
    fn layout_matches_the_format() {
        let bytes = SfoBuilder::new().utf8("AB", "xyz").build();
        // Header, one entry, key table "AB\0" + 1 pad, value "xyz\0".
        assert_eq!(bytes.len(), 0x14 + 0x10 + 4 + 4);
        assert_eq!(&bytes[..4], b"\0PSF");
        assert_eq!(&bytes[0x24..0x28], b"AB\0\0");
        assert_eq!(&bytes[0x28..], b"xyz\0");
        assert_eq!(&bytes[0x16..0x18], &FMT_UTF8.to_le_bytes());
    }

    #[test]
    fn every_seed_parses() {
        for seed in seeds() {
            ParamSfo::parse(&seed).unwrap();
        }
    }
}
