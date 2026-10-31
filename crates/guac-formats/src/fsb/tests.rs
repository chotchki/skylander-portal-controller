use super::builder::{BankSpec, SampleSpec, write};
use super::*;
use crate::mpeg::builder::stream;

fn two_samples(version: u32) -> BankSpec {
    BankSpec {
        version,
        codec: 0x0B,
        samples: vec![
            SampleSpec::new(stream(3, 3), 3 * 1152).named("first"),
            SampleSpec {
                rate_code: 9,
                channel_code: 0,
                ..SampleSpec::new(vec![0x55; 70], 40).named("second")
            },
        ],
        trailer: vec![0xEE; 7],
    }
}

#[test]
fn a_bank_reads_back_field_for_field_in_both_versions() {
    for version in [0, 1] {
        let spec = two_samples(version);
        let w = write(&spec);
        let bank = Bank::parse(&w.bytes).unwrap();
        assert_eq!(bank.version, version);
        assert_eq!(bank.codec, Codec::Mpeg);
        assert_eq!(
            bank.header_len,
            if version == 0 { 0x40 } else { 0x3C },
            "the header's length is the version's"
        );
        assert_eq!(bank.data_at, w.data_at);
        assert_eq!(
            bank.len(),
            w.bytes.len() - 7,
            "the trailer isn't the bank's"
        );
        let [a, b] = &bank.samples[..] else {
            panic!("two samples")
        };
        assert_eq!(
            (a.name, a.rate, a.channels, a.samples),
            (Some("first"), 44_100, 2, 3 * 1152)
        );
        // The first runs to the second's aligned offset: its bytes, then the
        // alignment's zeros.
        let own = spec.samples[0].data.len();
        assert_eq!(&a.data[..own], &spec.samples[0].data[..]);
        assert_eq!(a.data.len(), own.next_multiple_of(DATA_ALIGN));
        assert!(a.data[own..].iter().all(|b| *b == 0));
        assert_eq!(a.data_at, w.data_at);
        assert_eq!(
            (b.name, b.rate, b.channels, b.samples),
            (Some("second"), 48_000, 1, 40)
        );
        // The second runs from its aligned offset to the block's end.
        assert_eq!(b.data_at % DATA_ALIGN, w.data_at % DATA_ALIGN);
        assert_eq!(b.data, &spec.samples[1].data[..]);
        assert_eq!((a.loop_points, b.loop_points), (None, None));
    }
}

#[test]
fn chunks_override_the_packed_header_and_are_all_kept() {
    let spec = BankSpec::new(vec![
        SampleSpec::new(vec![1; 32], 8)
            .chunk(chunk::CHANNELS, vec![6])
            .chunk(chunk::RATE, 22_050u32.to_le_bytes().to_vec())
            .looped(2, 7)
            .chunk(0x0D, vec![9; 5]),
    ]);
    let w = write(&spec);
    let bank = Bank::parse(&w.bytes).unwrap();
    let s = &bank.samples[0];
    assert_eq!(
        (s.channels, s.rate, s.loop_points),
        (6, 22_050, Some((2, 7)))
    );
    let kinds: Vec<_> = s.chunks.iter().map(|c| (c.kind, c.bytes.len())).collect();
    assert_eq!(kinds, [(1, 1), (2, 4), (3, 8), (0x0D, 5)]);
    assert_eq!(s.chunks[3].bytes, [9; 5]);
    assert_eq!(s.chunks[3].at, w.chunks[0][3] + CHUNK_HEADER_LEN);
}

#[test]
fn a_bank_without_names_has_none_and_an_unknown_codec_is_named() {
    let mut spec = BankSpec::new(vec![SampleSpec::new(vec![3; 10], 5)]);
    spec.codec = 0x42;
    let w = write(&spec);
    let bank = Bank::parse(&w.bytes).unwrap();
    assert_eq!(bank.codec, Codec::Other(0x42));
    assert_eq!(bank.codec.word(), 0x42);
    assert_eq!(bank.samples[0].name, None);
    for word in 0..0x12 {
        assert_eq!(Codec::from_word(word).word(), word);
        assert_ne!(
            Codec::from_word(word),
            Codec::Other(word),
            "{word:#x} is FMOD's"
        );
    }
    assert_eq!(Codec::from_word(0x08), Codec::Vag);
}

fn refusal(bytes: &[u8]) -> (&'static str, usize, ErrorKind) {
    let e = Bank::parse(bytes).unwrap_err();
    (e.field, e.offset, e.kind)
}

fn put_u32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn counts_and_sizes_past_the_input_are_refused_by_name() {
    let w = write(&two_samples(1));
    // A sample count the header block can't hold.
    let mut b = w.bytes.clone();
    put_u32(&mut b, 8, 1_000);
    assert_eq!(refusal(&b).0, "sample_count");
    // A data block past the input.
    let mut b = w.bytes.clone();
    put_u32(&mut b, 0x14, 1 << 30);
    assert_eq!(refusal(&b).0, "data");
    // A name table past the input.
    let mut b = w.bytes.clone();
    put_u32(&mut b, 0x10, 1 << 30);
    assert_eq!(refusal(&b).0, "name_table");
    // A version nobody wrote.
    let mut b = w.bytes.clone();
    put_u32(&mut b, 4, 2);
    assert!(matches!(
        refusal(&b),
        ("version", 4, ErrorKind::UnsupportedVersion { found: 2, .. })
    ));
    // Not a bank.
    assert_eq!(refusal(b"FSB4 and more bytes here").0, "magic");
}

#[test]
fn a_data_offset_past_the_next_or_the_block_is_refused() {
    let w = write(&two_samples(1));
    // The second sample's offset (bits 7-33) pushed past the block.
    let mut b = w.bytes.clone();
    let at = w.sample_headers[1];
    let packed = u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
    let moved = (packed & !(0x07FF_FFFF << 7)) | (1_000u64 << 7);
    b[at..at + 8].copy_from_slice(&moved.to_le_bytes());
    let (field, offset, kind) = refusal(&b);
    assert_eq!((field, offset), ("data_offset", w.sample_headers[0]));
    assert!(matches!(kind, ErrorKind::OutOfRange { .. }));
    // A rate code past the table.
    let mut b = w.bytes.clone();
    let packed = u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
    let bad = (packed & !(0xF << 1)) | (14 << 1);
    b[at..at + 8].copy_from_slice(&bad.to_le_bytes());
    assert_eq!(refusal(&b).0, "rate_code");
}

#[test]
fn offsets_out_of_order_are_refused() {
    // Three samples, the second's offset moved before the first's, all
    // inside the block.
    let spec = BankSpec::new(vec![
        SampleSpec::new(vec![1; 64], 8),
        SampleSpec::new(vec![2; 64], 8),
        SampleSpec::new(vec![3; 64], 8),
    ]);
    let w = write(&spec);
    let mut b = w.bytes.clone();
    let at = w.sample_headers[0];
    let packed = u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
    // The first now starts at 96, past the second's 64.
    let moved = (packed & !(0x07FF_FFFF << 7)) | (3u64 << 7);
    b[at..at + 8].copy_from_slice(&moved.to_le_bytes());
    let (field, offset, kind) = refusal(&b);
    assert_eq!((field, offset), ("data_offset", at));
    assert_eq!(
        kind,
        ErrorKind::OutOfRange {
            value: 96,
            limit: 64
        }
    );
}

#[test]
fn a_chunk_longer_than_its_block_or_shorter_than_its_kind_is_refused() {
    let spec = BankSpec::new(vec![SampleSpec::new(vec![1; 8], 4).looped(1, 3)]);
    let w = write(&spec);
    let at = w.chunks[0][0];
    // Longer than what's left of the header block.
    let mut b = w.bytes.clone();
    put_u32(&mut b, at, (200 << 1) | (u32::from(chunk::LOOP) << 25));
    assert_eq!(refusal(&b).0, "chunk_body");
    // A loop chunk of four bytes.
    let short = BankSpec::new(vec![
        SampleSpec::new(vec![1; 8], 4).chunk(chunk::LOOP, vec![0; 4]),
    ]);
    assert!(matches!(
        refusal(&write(&short).bytes),
        (
            "loop_chunk",
            _,
            ErrorKind::Truncated {
                needed: 8,
                available: 4
            }
        )
    ));
    // A channels chunk of none.
    let empty = BankSpec::new(vec![
        SampleSpec::new(vec![1; 8], 4).chunk(chunk::CHANNELS, vec![]),
    ]);
    assert_eq!(refusal(&write(&empty).bytes).0, "channels_chunk");
    // A rate chunk of two bytes.
    let rate = BankSpec::new(vec![
        SampleSpec::new(vec![1; 8], 4).chunk(chunk::RATE, vec![1, 2]),
    ]);
    assert_eq!(refusal(&write(&rate).bytes).0, "rate_chunk");
}

#[test]
fn a_name_offset_past_the_table_is_refused() {
    let w = write(&two_samples(1));
    let mut b = w.bytes.clone();
    put_u32(&mut b, w.names_at + 4, 0x4000);
    let (field, offset, _) = refusal(&b);
    assert_eq!((field, offset), ("name_offset", w.names_at + 4));
    // A name with no NUL before the table ends.
    let one = BankSpec::new(vec![SampleSpec::new(vec![1; 8], 4).named("abc")]);
    let w = write(&one);
    let mut b = w.bytes.clone();
    b[w.names_at + 4..w.data_at].fill(b'x');
    assert_eq!(refusal(&b).0, "name");
}

#[test]
fn every_seed_parses() {
    for (i, seed) in builder::seeds().iter().enumerate() {
        assert!(Bank::parse(seed).is_ok(), "seed {i}");
    }
}
