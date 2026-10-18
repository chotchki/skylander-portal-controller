//! Which dump an extract came from (design decision 4).

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// The identity key: `(TITLE_ID, disc APP_VER, manifest hash)`. No title-update
/// field: the parity target is the 1.00 disc, and a dump an update changed
/// misses the known table as an unknown dump.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DumpIdentity {
    pub title_id: String,
    pub app_ver: String,
    /// SHA-256 over every plaintext USRDIR file but the EBOOT (AB.4 defines
    /// the exact input).
    pub manifest_hash: Sha256,
}

/// A SHA-256 digest. Lowercase hex in RON so an extract diffs by eye, the raw
/// 32 bytes in postcard.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sha256(pub [u8; 32]);

impl fmt::Display for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256({self})")
    }
}

impl Sha256 {
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.as_bytes();
        if hex.len() != 64 {
            return None;
        }
        let nibble = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        let mut out = [0u8; 32];
        for (byte, [hi, lo]) in out.iter_mut().zip(hex.as_chunks::<2>().0) {
            *byte = nibble(*hi)? << 4 | nibble(*lo)?;
        }
        Some(Self(out))
    }
}

impl Serialize for Sha256 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(self)
        } else {
            self.0.serialize(s)
        }
    }
}

impl<'de> Deserialize<'de> for Sha256 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            struct Hex;
            impl Visitor<'_> for Hex {
                type Value = Sha256;
                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("64 lowercase hex digits")
                }
                fn visit_str<E: de::Error>(self, v: &str) -> Result<Sha256, E> {
                    Sha256::from_hex(v)
                        .ok_or_else(|| E::invalid_value(de::Unexpected::Str(v), &self))
                }
            }
            d.deserialize_str(Hex)
        } else {
            <[u8; 32]>::deserialize(d).map(Self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_rejects_bad_input() {
        let digest = Sha256(std::array::from_fn(|i| (i as u8).wrapping_mul(9)));
        assert_eq!(Sha256::from_hex(&digest.to_string()), Some(digest));
        assert_eq!(Sha256::from_hex("ab"), None);
        assert_eq!(Sha256::from_hex(&"G".repeat(64)), None);
        // Uppercase isn't what we write, so it isn't what we read.
        assert_eq!(Sha256::from_hex(&"AB".repeat(32)), None);
    }
}
