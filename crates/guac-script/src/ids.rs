//! The hashing behind the VM's own maps (openspec AS.35): their keys are its
//! ids and the disc's member hashes, never an attacker's, so std's SipHash
//! buys nothing and cost about a fifth of a long level's run (level 214's
//! hurt checks, sampled). A multiply-rotate hash, rustc-hash 1's FxHasher.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// The word mixed in each step: 2^64 over pi, made odd (FxHasher's).
const K: u64 = 0x517c_c1b7_2722_0a95;

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct IdHasher(u64);

impl IdHasher {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(K);
    }
}

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }

    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
}

pub(crate) type Ids = BuildHasherDefault<IdHasher>;
pub(crate) type IdMap<K, V> = HashMap<K, V, Ids>;
pub(crate) type IdSet<K> = HashSet<K, Ids>;

#[cfg(test)]
mod tests {
    use std::hash::{BuildHasher, Hash};

    use super::*;

    fn hash(v: impl Hash) -> u64 {
        Ids::default().hash_one(v)
    }

    /// Equal keys hash equal, and the ids a map is keyed by spread: nearly
    /// every one of 1,024 consecutive ids lands on a pair of a hash's top 7
    /// bits and low 10 of its own (what hashbrown's tags and a 1,024-slot
    /// table take from it; a random hash would share about 4), and the top
    /// 7 alone take nearly all their values.
    #[test]
    fn equal_keys_hash_equal_and_ids_spread() {
        assert_eq!(hash((3u32, 7u64)), hash((3u32, 7u64)));
        assert_ne!(hash((3u32, 7u64)), hash((7u32, 3u64)));
        let mut seen = std::collections::HashSet::new();
        for id in 0..1024u32 {
            let h = hash(id);
            seen.insert(((h >> 57), h & 0x3ff));
        }
        assert!(seen.len() > 1000, "{} of 1024", seen.len());
        // The tags alone take nearly all of their 128 values.
        let tags: std::collections::HashSet<u64> = (0..1024u32).map(|id| hash(id) >> 57).collect();
        assert!(tags.len() > 120, "{} tags of 128", tags.len());
        // Bytes past a word's worth mix in too.
        assert_ne!(hash("level_214_tutotial_2"), hash("level_214_tutotial_3"));
    }
}
