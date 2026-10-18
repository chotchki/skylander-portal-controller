//! The dumps guacamole extracts from (design decision 4): identities and
//! hashes only, never content. Anything else is an unknown dump.

use guac_ir::{DumpIdentity, Sha256};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct KnownDump {
    pub title_id: &'static str,
    pub app_ver: &'static str,
    /// Lowercase hex of the manifest hash (`dump::manifest_hash`).
    pub manifest_hash: &'static str,
    /// Whose dump produced it.
    pub source: &'static str,
}

impl KnownDump {
    pub fn matches(&self, id: &DumpIdentity) -> bool {
        self.title_id == id.title_id
            && self.app_ver == id.app_ver
            && Sha256::from_hex(self.manifest_hash) == Some(id.manifest_hash)
    }
}

/// One entry per hash chotchki's own BLUS30968 01.00 dumps produce. The HTPC's
/// joins at the Windows bring-up if it hashes differently.
pub const KNOWN_DUMPS: &[KnownDump] = &[KnownDump {
    title_id: "BLUS30968",
    app_ver: "01.00",
    manifest_hash: "5e933248f8cc7c54ac25bddf031e6cdc18f8bd44e7c8c8d4d539468f160d2d12",
    source: "chotchki's Giants disc, the Mac's dump (AB.4)",
}];

pub fn lookup<'a>(table: &'a [KnownDump], id: &DumpIdentity) -> Option<&'a KnownDump> {
    table.iter().find(|k| k.matches(id))
}

/// A title id or version safe to put in a folder name: what the identity dir
/// and the hash-cache lookup build paths from.
pub(crate) fn path_safe(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 16
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
        && s != "."
        && s != ".."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_well_formed() {
        for k in KNOWN_DUMPS {
            assert!(Sha256::from_hex(k.manifest_hash).is_some(), "{k:?}");
            assert!(path_safe(k.title_id) && path_safe(k.app_ver), "{k:?}");
        }
    }

    #[test]
    fn a_match_needs_all_three_fields() {
        let k = KNOWN_DUMPS[0];
        let id = DumpIdentity {
            title_id: k.title_id.into(),
            app_ver: k.app_ver.into(),
            manifest_hash: Sha256::from_hex(k.manifest_hash).unwrap(),
        };
        assert_eq!(lookup(KNOWN_DUMPS, &id), Some(&k));
        let other_ver = DumpIdentity {
            app_ver: "01.01".into(),
            ..id.clone()
        };
        assert_eq!(lookup(KNOWN_DUMPS, &other_ver), None);
        let other_hash = DumpIdentity {
            manifest_hash: Sha256([0xab; 32]),
            ..id
        };
        assert_eq!(lookup(KNOWN_DUMPS, &other_hash), None);
    }

    #[test]
    fn path_safety() {
        assert!(path_safe("BLUS30968") && path_safe("01.00"));
        for bad in ["", ".", "..", "a/b", "a\\b", "x y", "ü"] {
            assert!(!path_safe(bad), "{bad}");
        }
    }
}
