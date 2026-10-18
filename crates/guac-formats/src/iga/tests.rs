use super::builder::{IgaBuilder, fnv1a32};
use super::*;

fn text(len: usize) -> Vec<u8> {
    // Compressible but not trivial, so LZMA codes both matches and literals.
    (0..len)
        .map(|i| b"portal master "[i % 14] ^ (i / 997) as u8)
        .collect()
}

fn noise(len: usize) -> Vec<u8> {
    let mut x = 0x2545_F491_u32;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn names_sizes_and_hashes_read_back() {
    let small = text(100);
    let big = text(0x8000 * 2 + 1234);
    let bytes = IgaBuilder::new()
        .stored("a.txt", &small)
        .lzma("level.bld/level.igz", &big)
        .stored("empty", &[])
        .build();
    let archive = Archive::parse(&bytes).unwrap();
    let h = archive.header();
    assert_eq!((h.version, h.file_count, h.sector_size), (8, 3, 0x800));
    assert_eq!(
        h.name_table_offset as usize + h.name_table_size as usize,
        bytes.len()
    );
    assert_eq!(archive.entries().len(), 3);
    for (name, size) in [
        ("a.txt", 100),
        ("level.bld/level.igz", big.len()),
        ("empty", 0),
    ] {
        let e = archive.find(name).unwrap();
        assert_eq!(e.name(), name);
        assert_eq!(e.size() as usize, size, "{name}");
        assert_eq!(e.hash(), fnv1a32(name.as_bytes()), "{name}");
        assert_ne!(e.stamp(), 0);
    }
    // The TOC is in hash order, as Giants' is.
    let hashes: Vec<u32> = archive.entries().iter().map(Entry::hash).collect();
    assert!(hashes.is_sorted());
    assert_eq!(
        archive.find("level.bld/level.igz").unwrap().chunk_count(),
        3
    );
    assert!(archive.find("missing").is_none());
}

#[test]
fn stored_entry_is_a_view_into_the_archive() {
    let body = text(5000);
    let bytes = IgaBuilder::new().stored("s", &body).build();
    let archive = Archive::parse(&bytes).unwrap();
    let entry = &archive.entries()[0];
    assert_eq!(entry.compression(), Compression::Stored);
    let data = archive.read(entry).unwrap();
    assert!(matches!(data, Cow::Borrowed(_)));
    assert_eq!(&*data, &body[..]);
    let at = entry.offset() as usize;
    assert_eq!(data.as_ptr(), bytes[at..].as_ptr());
}

#[test]
fn lzma_entry_round_trips_across_chunks() {
    for len in [0, 1, 0x7FFF, 0x8000, 0x8001, 3 * 0x8000 + 77] {
        let body = text(len);
        let bytes = IgaBuilder::new().lzma("e", &body).build();
        let archive = Archive::parse(&bytes).unwrap();
        let entry = &archive.entries()[0];
        assert_eq!(entry.compression(), Compression::Lzma, "{len}");
        assert_eq!(entry.chunk_count(), len.div_ceil(CHUNK_LEN), "{len}");
        assert_eq!(&*archive.read(entry).unwrap(), &body[..], "{len}");
        let sizes: Vec<usize> = archive.chunks(entry).map(|c| c.unwrap().len()).collect();
        assert!(sizes.iter().all(|s| *s <= CHUNK_LEN), "{len}");
        assert_eq!(sizes.iter().sum::<usize>(), len);
    }
}

#[test]
fn incompressible_chunk_is_stored_raw_inside_an_lzma_entry() {
    // A chunk of noise (LZMA would grow it) between two of text.
    let body = [text(CHUNK_LEN), noise(CHUNK_LEN), text(500)].concat();
    let bytes = IgaBuilder::new().lzma("mixed", &body).build();
    let archive = Archive::parse(&bytes).unwrap();
    let entry = &archive.entries()[0];
    let chunks: Vec<_> = archive.chunks(entry).map(Result::unwrap).collect();
    assert!(matches!(chunks[0], Cow::Owned(_)));
    assert!(matches!(chunks[1], Cow::Borrowed(_)));
    assert!(matches!(chunks[2], Cow::Owned(_)));
    assert_eq!(&*archive.read(entry).unwrap(), &body[..]);
}

#[test]
fn every_block_table_width_reads() {
    // A 16-byte sector puts entries in all three tables at test sizes:
    // small up to 2032 bytes, medium up to 524,272, large past that.
    let sizes = [1000usize, 4000, 524_273];
    let mut b = IgaBuilder::new().sector_size(16);
    for (i, len) in sizes.iter().enumerate() {
        b = b.lzma(&format!("e{i}"), &text(*len));
    }
    let bytes = b.build();
    let archive = Archive::parse(&bytes).unwrap();
    let h = archive.header();
    assert!(h.small_blocks > 0 && h.medium_blocks > 0 && h.large_blocks > 0);
    for (i, len) in sizes.iter().enumerate() {
        let e = archive.find(&format!("e{i}")).unwrap();
        assert_eq!(&*archive.read(e).unwrap(), &text(*len)[..], "e{i}");
    }
}

#[test]
fn other_versions_are_refused_by_name() {
    let mut bytes = IgaBuilder::new().stored("x", b"x").build();
    put32(&mut bytes, 4, 0x0B);
    let err = Archive::parse(&bytes).unwrap_err();
    assert_eq!((err.field, err.offset), ("version", 4));
    assert!(matches!(
        err.kind,
        ErrorKind::UnsupportedVersion { found: 0x0B, .. }
    ));

    // Swap Force onward: the magic and version written big-endian.
    let mut be = b"\x1aAGI\0\0\0\x0a".to_vec();
    be.resize(HEADER_LEN, 0);
    let err = Archive::parse(&be).unwrap_err();
    assert_eq!(err.field, "magic");
    assert!(matches!(
        err.kind,
        ErrorKind::UnsupportedVersion { found: 0x0A, .. }
    ));
    assert!(err.to_string().contains("big-endian"), "{err}");

    let err = Archive::parse(b"PSF\0").unwrap_err();
    assert!(matches!(err.kind, ErrorKind::BadMagic { .. }));
}

#[test]
fn a_chunk_claiming_more_than_0x8000_bytes_errors() {
    let mut bytes = IgaBuilder::new().lzma("e", &text(4000)).build();
    let at = Archive::parse(&bytes).unwrap().entries()[0].offset() as usize;
    bytes[at..at + 2].copy_from_slice(&0x8001u16.to_le_bytes());
    let archive = Archive::parse(&bytes).unwrap();
    let err = archive.read(&archive.entries()[0]).unwrap_err();
    assert_eq!((err.field, err.offset), ("chunk_size", at));
    assert_eq!(
        err.kind,
        ErrorKind::OutOfRange {
            value: 0x8001,
            limit: 0x8000
        }
    );
}

#[test]
fn a_name_offset_past_the_table_errors() {
    let mut bytes = IgaBuilder::new()
        .stored("a", b"1")
        .stored("b", b"2")
        .build();
    let names_at = le32(&bytes, 0x1C) as usize;
    let names_len = le32(&bytes, 0x20);
    put32(&mut bytes, names_at + 4, names_len);
    let err = Archive::parse(&bytes).unwrap_err();
    assert_eq!((err.field, err.offset), ("name_offset", names_at + 4));
    assert_eq!(
        err.kind,
        ErrorKind::OutOfRange {
            value: names_len.into(),
            limit: names_len.into()
        }
    );
}

#[test]
fn a_size_mismatch_errors() {
    // Declared longer than the stream: the decoder runs out.
    let bytes = IgaBuilder::new()
        .lzma_declaring("long", &text(1000), 1200)
        .build();
    let archive = Archive::parse(&bytes).unwrap();
    let err = archive.read(&archive.entries()[0]).unwrap_err();
    assert_eq!(err.field, "chunk");
    assert!(
        matches!(err.kind, ErrorKind::Unpack { expected: 1200, unpacked, .. } if unpacked <= 1000),
        "{err}"
    );

    // Declared shorter: the stream has symbols left at the declared end.
    let bytes = IgaBuilder::new()
        .lzma_declaring("short", &text(1000), 800)
        .build();
    let archive = Archive::parse(&bytes).unwrap();
    let err = archive.read(&archive.entries()[0]).unwrap_err();
    assert!(
        matches!(err.kind, ErrorKind::Unpack { expected: 800, .. }),
        "{err}"
    );
}

#[test]
fn corrupt_tables_error_with_the_field() {
    let good = IgaBuilder::new()
        .stored("s", &text(300))
        .lzma("z", &text(9000))
        .build();
    let archive = Archive::parse(&good).unwrap();
    let count = archive.entries().len();
    let z = archive.find("z").unwrap();
    let s = archive.find("s").unwrap();
    let rec = |e: &Entry| HEADER_LEN + count * 4 + e.index() * RECORD_LEN;

    // A TOC size that disagrees with the counts.
    let mut b = good.clone();
    put32(&mut b, 0x08, le32(&good, 0x08) + 1);
    assert_eq!(Archive::parse(&b).unwrap_err().field, "toc_size");

    // A file count that would run the TOC off the end.
    let mut b = good.clone();
    put32(&mut b, 0x0C, u32::MAX);
    assert!(Archive::parse(&b).is_err());

    // A stored entry running past the input.
    let mut b = good.clone();
    put32(&mut b, rec(s) + 4, good.len() as u32);
    assert_eq!(Archive::parse(&b).unwrap_err().field, "data");

    // A first block slot past the table.
    let mut b = good.clone();
    put32(&mut b, rec(z) + 8, 0x2FFF_FFFF);
    assert_eq!(Archive::parse(&b).unwrap_err().field, "block_word");

    // A size past the cap.
    let mut b = good.clone();
    put32(&mut b, rec(z) + 4, MAX_UNPACKED_LEN + 1);
    assert_eq!(Archive::parse(&b).unwrap_err().field, "size");

    // A codec Giants doesn't use parses, then refuses to read.
    let mut b = good.clone();
    let word = le32(&good, rec(z) + 8);
    put32(&mut b, rec(z) + 8, (word & 0x0FFF_FFFF) | 0x1000_0000);
    let a = Archive::parse(&b).unwrap();
    let e = a.find("z").unwrap();
    assert_eq!(e.compression(), Compression::Other(1));
    let err = a.read(e).unwrap_err();
    assert_eq!(
        (err.field, err.kind),
        ("block_word", ErrorKind::Unsupported { value: 1 })
    );

    // A sector size that isn't a power of two.
    let mut b = good.clone();
    put32(&mut b, 0x10, 0x801);
    assert_eq!(Archive::parse(&b).unwrap_err().field, "sector_size");

    // LZMA properties that would size a 6 MB literal coder (lc 8, lp 4).
    let mut b = good.clone();
    let at = z.offset() as usize;
    b[at + 2] = 8 + 4 * 9;
    let a = Archive::parse(&b).unwrap();
    let err = a.read(a.find("z").unwrap()).unwrap_err();
    assert_eq!((err.field, err.offset), ("lzma_props", at + 2));

    // A chunk whose end slot isn't past its start.
    let mut b = good.clone();
    let small_at = HEADER_LEN
        + count * (4 + RECORD_LEN)
        + le32(&good, 0x24) as usize * 4
        + le32(&good, 0x28) as usize * 2;
    let first = (le32(&good, rec(z) + 8) & 0x0FFF_FFFF) as usize;
    b[small_at + first + 1] = b[small_at + first] & 0x7F;
    let a = Archive::parse(&b).unwrap();
    let err = a.read(a.find("z").unwrap()).unwrap_err();
    assert_eq!(
        (err.field, err.offset),
        ("block_slot", small_at + first + 1)
    );

    // The last name's stamp cut off by the table's end.
    let mut b = good.clone();
    put32(&mut b, 0x20, le32(&good, 0x20) - 1);
    b.pop();
    assert_eq!(Archive::parse(&b).unwrap_err().field, "stamp");
}

#[test]
fn reading_a_chunk_past_the_end_errors() {
    let bytes = IgaBuilder::new().lzma("z", &text(10)).build();
    let archive = Archive::parse(&bytes).unwrap();
    let e = &archive.entries()[0];
    assert_eq!(archive.read_chunk(e, 1).unwrap_err().field, "chunk");
}

#[test]
fn empty_archive_parses() {
    let bytes = IgaBuilder::new().build();
    assert!(Archive::parse(&bytes).unwrap().entries().is_empty());
}
