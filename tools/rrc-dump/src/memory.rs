//! Sparse RSX memory: only the bytes the capture's blocks wrote, per location.
//! Reads report how many requested bytes no block ever covered.

use std::collections::HashMap;

const PAGE: usize = 4096;

struct Page {
    bytes: Box<[u8; PAGE]>,
    written: Box<[bool; PAGE]>,
}

#[derive(Default)]
pub struct SparseMemory {
    pages: HashMap<(u32, u64), Page>,
    /// Length of the last block applied at each (location, offset), used to size textures.
    block_lengths: HashMap<(u32, u32), usize>,
}

impl SparseMemory {
    pub fn write(&mut self, location: u32, offset: u32, data: &[u8]) {
        self.block_lengths.insert((location, offset), data.len());
        let mut addr = u64::from(offset);
        let mut rest = data;
        while !rest.is_empty() {
            let page_no = addr / PAGE as u64;
            let in_page = (addr % PAGE as u64) as usize;
            let n = rest.len().min(PAGE - in_page);
            let page = self
                .pages
                .entry((location, page_no))
                .or_insert_with(|| Page {
                    bytes: Box::new([0; PAGE]),
                    written: Box::new([false; PAGE]),
                });
            page.bytes[in_page..in_page + n].copy_from_slice(&rest[..n]);
            page.written[in_page..in_page + n].fill(true);
            rest = &rest[n..];
            addr += n as u64;
        }
    }

    /// Returns `len` bytes at (location, offset), zero-filled where nothing was written, plus the unwritten count.
    pub fn read(&self, location: u32, offset: u64, len: usize) -> (Vec<u8>, usize) {
        let mut out = vec![0u8; len];
        let mut missing = 0;
        let mut addr = offset;
        let mut done = 0;
        while done < len {
            let page_no = addr / PAGE as u64;
            let in_page = (addr % PAGE as u64) as usize;
            let n = (len - done).min(PAGE - in_page);
            match self.pages.get(&(location, page_no)) {
                Some(page) => {
                    out[done..done + n].copy_from_slice(&page.bytes[in_page..in_page + n]);
                    missing += page.written[in_page..in_page + n]
                        .iter()
                        .filter(|w| !**w)
                        .count();
                }
                None => missing += n,
            }
            done += n;
            addr += n as u64;
        }
        (out, missing)
    }

    pub fn block_len_at(&self, location: u32, offset: u32) -> Option<usize> {
        self.block_lengths.get(&(location, offset)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_spans_pages_and_counts_gaps() {
        let mut m = SparseMemory::default();
        m.write(0, 4090, &[1, 2, 3, 4, 5, 6, 7, 8]);
        let (bytes, missing) = m.read(0, 4088, 12);
        assert_eq!(bytes, [0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0]);
        assert_eq!(missing, 4);
        let (_, missing) = m.read(1, 4090, 8);
        assert_eq!(missing, 8, "locations don't alias");
        assert_eq!(m.block_len_at(0, 4090), Some(8));
    }
}
