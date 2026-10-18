use super::builder::{IgzBuilder, METAIMAGES, pack_locations, sample};
use super::*;

fn be(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).unwrap()
}

/// A builder holding one 0x20-byte object at data section 0, offset 4.
fn one_object() -> (IgzBuilder, Location) {
    let mut b = IgzBuilder::new();
    let o = b.object(0, "igTest", 0x20);
    (b, o)
}

fn refused(bytes: &[u8]) -> FormatError {
    Igz::parse(bytes).expect_err("parse should refuse this file")
}

#[test]
fn the_sample_reads_back_every_known_fixup_kind() {
    let bytes = sample();
    let igz = Igz::parse(&bytes).unwrap();
    assert_eq!(igz.hash(), 0xCCEC_5BC4);
    let mut kinds: Vec<u32> = igz.records().iter().map(|r| r.kind).collect();
    kinds.sort();
    let mut known = kind::KNOWN.to_vec();
    known.push(8);
    known.sort();
    assert_eq!(kinds, known);

    // Fixups, section 0, a placeholder, pixels, vertexes, strings.
    assert_eq!(igz.sections().len(), 6);
    assert!(igz.data_sections()[1].is_placeholder());
    assert_eq!(igz.data_sections()[1].len(), 0);
    assert_eq!(
        igz.type_names(),
        ["igObjectList", "igImage2", "igPS3EdgeGeometrySegment"]
    );
    assert_eq!(igz.type_sizes(), [0x18, 0x60, 0xB0]);

    let [list, image, geom] = igz.objects() else {
        panic!("three objects")
    };
    assert_eq!(list.at, Location(4));
    assert_eq!(igz.type_name(image), "igImage2");
    assert_eq!(igz.objects_of_type("igPS3EdgeGeometrySegment").count(), 1);

    // An object list: count, then a memory ref to two object pointers.
    assert_eq!(igz.u32_at(at(list.at, 8), "_count").unwrap(), 2);
    let elements = igz.memory_ref(at(list.at, 0x10), "_data").unwrap().unwrap();
    assert_eq!((elements.data.len(), elements.alignment()), (8, 4));
    assert_eq!(
        igz.reference(at(list.at, 0x14), "_data").unwrap(),
        Ref::Data(elements.at)
    );
    assert_eq!(
        igz.reference(elements.at, "element").unwrap(),
        Ref::Object(1)
    );
    assert_eq!(
        igz.reference(at(elements.at, 4), "element").unwrap(),
        Ref::Object(2)
    );
    // Objects step by MTSZ, so the list's own markers stop at its extent.
    assert_eq!(igz.markers_of(list), [(at(list.at, 0x14), Marker::Pointer)]);

    assert_eq!(
        igz.string(at(image.at, 8), "_name").unwrap(),
        Some("eruptor_body")
    );
    let Ref::Import { index: 0, id } = igz.reference(at(image.at, 0x3C), "_format").unwrap() else {
        panic!("an import")
    };
    assert!(id.is(METAIMAGES, "dxt5_tile_big_ps3"));
    assert_eq!(id.namespace, 0x843C_D0C2);
    let Ref::Handle { index: 0, handle } = igz.reference(at(image.at, 0x48), "_data").unwrap()
    else {
        panic!("a handle")
    };
    assert_eq!((handle.data, handle.alignment()), (&[0xAB; 0x80][..], 128));
    assert_eq!(igz.memory_handles().len(), 1);
    // Unmarked zero words are null.
    assert_eq!(
        igz.reference(at(image.at, 0x4C), "_lockCount").unwrap(),
        Ref::Null
    );
    assert_eq!(igz.string(at(image.at, 0x4C), "_lockCount").unwrap(), None);

    let vertexes = igz
        .memory_ref(at(geom.at, 0x10), "_indexes")
        .unwrap()
        .unwrap();
    assert_eq!(
        (vertexes.data, vertexes.alignment()),
        (&[1, 2, 3, 4, 5, 6][..], 16)
    );
    // Null refs: size words 0, 0x10000000 and 0x7, all with bit 31 clear.
    for offset in [0x24, 0x50, 0x98] {
        assert_eq!(igz.memory_ref(at(geom.at, offset), "null").unwrap(), None);
        assert_eq!(
            igz.reference(at(geom.at, offset), "null").unwrap(),
            Ref::NullMemoryRef
        );
    }
    assert_eq!(
        igz.reference(at(geom.at, 0x8C), "meta").unwrap(),
        Ref::MetaObject {
            index: 0,
            name: ExternalName {
                namespace: "metaobject",
                name: "tfbActorInfo"
            }
        }
    );
    assert_eq!(
        igz.reference(at(geom.at, 0xA0), "meta").unwrap(),
        Ref::UnresolvedMeta(1)
    );
    let offsets: Vec<u32> = igz
        .refs_of(geom)
        .map(|(o, r)| r.map(|_| o).unwrap())
        .collect();
    assert_eq!(offsets, [0x14, 0x24, 0x50, 0x8C, 0x98, 0xA0]);

    // Giants' kind 8, kept raw.
    let unknown: Vec<_> = igz.unknown_records().collect();
    assert_eq!(unknown.len(), 1);
    assert_eq!((unknown[0].kind, unknown[0].count), (8, 1));
    assert_eq!(unknown[0].payload, list.at.0.to_be_bytes());
}

#[test]
fn a_record_of_an_unknown_kind_is_kept_not_read() {
    let (mut b, _) = one_object();
    b.unknown_record(0x11, 3, &[0xDE, 0xAD, 0xBE, 0xEF]);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let kinds: Vec<(u32, u32)> = igz.unknown_records().map(|r| (r.kind, r.count)).collect();
    assert_eq!(kinds, [(8, 1), (0x11, 3)]);
    assert_eq!(igz.record(0x11).unwrap().payload, [0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn location_lists_round_trip() {
    // 0x670 is 4 + 410 * 4 + 4, and 410 is 0b110_011_010: nibbles A, B, 6.
    assert_eq!(
        pack_locations(&[Location(4), Location(0x670)]),
        [0xA0, 0x6B]
    );
    assert_eq!(
        decode_locations(&[0xA0, 0x6B], 2).unwrap(),
        [Location(4), Location(0x670)]
    );
    let lists = [
        vec![Location(4)],
        vec![Location(4), Location(8), Location(12), Location(0x1000)],
        vec![
            Location::new(0, 0x10),
            Location::new(2, 0),
            Location::new(2, 0xFF_FFFC),
            Location::new(7, 0x40),
        ],
        vec![Location(u32::MAX - 3)],
    ];
    for list in lists {
        let packed = pack_locations(&list);
        assert_eq!(decode_locations(&packed, list.len()).unwrap(), list);
    }
}

#[test]
fn a_location_list_that_runs_out_or_overflows_is_refused() {
    // Two continue nibbles and nothing after them.
    let (nibble, err) = decode_locations(&[0x88], 1).unwrap_err();
    assert_eq!(nibble, 0);
    assert!(matches!(err, ErrorKind::Truncated { .. }), "{err:?}");
    // Twelve continue nibbles pass 32 bits.
    let err = decode_locations(&[0xFF; 6], 1).unwrap_err().1;
    assert!(matches!(err, ErrorKind::OutOfRange { .. }), "{err:?}");
    // The running location can't pass 32 bits either.
    let mut near_end = pack_locations(&[Location(u32::MAX - 3)]);
    near_end.push(0);
    let err = decode_locations(&near_end, 2).unwrap_err().1;
    assert!(matches!(err, ErrorKind::OutOfRange { .. }), "{err:?}");
}

#[test]
fn a_dangling_pointer_is_refused() {
    let (mut b, o) = one_object();
    b.pointer(at(o, 8), Location::new(0, 0x1_0000));
    let err = refused(&b.build());
    assert_eq!(err.field, "pointer");
    assert!(matches!(err.kind, ErrorKind::OutOfRange { .. }), "{err}");

    // One past the section's last byte dangles too.
    let (mut b, o) = one_object();
    b.pointer(at(o, 8), Location::new(0, 0x24));
    assert_eq!(refused(&b.build()).field, "pointer");
}

#[test]
fn a_pointer_into_a_missing_or_placeholder_section_is_refused() {
    let (mut b, o) = one_object();
    b.pointer(at(o, 8), Location::new(9, 0));
    let err = refused(&b.build());
    assert_eq!(
        (err.field, err.kind),
        ("pointer", ErrorKind::OutOfRange { value: 9, limit: 1 })
    );

    let (mut b, o) = one_object();
    let empty = b.placeholder_section();
    b.pointer(at(o, 8), Location::new(empty, 0));
    let err = refused(&b.build());
    assert_eq!(err.field, "pointer");
    assert!(
        matches!(err.kind, ErrorKind::OutOfRange { limit: 0, .. }),
        "{err}"
    );
}

#[test]
fn a_section_past_the_end_of_the_file_is_refused() {
    let (b, _) = one_object();
    let mut bytes = b.build();
    // Data section 0's size word.
    let entry = SECTION_TABLE_AT + SECTION_ENTRY_LEN;
    let len = bytes.len() as u32;
    put(&mut bytes, entry + 4, len);
    let err = refused(&bytes);
    assert_eq!(err.field, "section");
    assert!(matches!(err.kind, ErrorKind::Truncated { .. }), "{err}");
}

#[test]
fn bad_objects_are_refused() {
    // An unknown type index.
    let (mut b, o) = one_object();
    b.put_u32(o, 99);
    let err = refused(&b.build());
    assert_eq!(err.field, "object_type");

    // Two objects overlapping.
    let (mut b, o) = one_object();
    b.raw_object(at(o, 0x10));
    let err = refused(&b.build());
    assert_eq!(err.field, "object");

    // An object running past its section.
    let mut b = IgzBuilder::new();
    let o = b.object(0, "igTest", 0x20);
    let big = b.type_index("igBig", 0x100);
    b.put_u32(o, big);
    let err = refused(&b.build());
    assert_eq!(err.field, "object");

    // An RVTB entry outside every section.
    let (mut b, _) = one_object();
    b.raw_object(Location::new(5, 0));
    assert_eq!(refused(&b.build()).field, "object_type");
}

#[test]
fn import_handle_and_meta_indexes_must_be_in_range() {
    for (k, value) in [
        (kind::IMPORTS, INDEX_FLAG | 3),
        (kind::IMPORTS, 0),
        (kind::HANDLES, 1),
        (kind::META_OBJECTS, INDEX_FLAG),
    ] {
        let (mut b, o) = one_object();
        b.put_u32(at(o, 8), value);
        b.marker(k, at(o, 8));
        let err = refused(&b.build());
        assert!(
            matches!(err.kind, ErrorKind::OutOfRange { .. }),
            "kind {k:#x}: {err}"
        );
    }
}

#[test]
fn a_null_memory_ref_marker_on_a_present_ref_is_refused() {
    let (mut b, o) = one_object();
    b.put_u32(at(o, 8), MEMORY_PRESENT | 4);
    b.marker(kind::NULL_MEMORY_REFS, at(o, 8));
    assert_eq!(refused(&b.build()).field, "null_memory_ref");

    // Nor may its pointer word be marked.
    let (mut b, o) = one_object();
    b.null_memory_ref(at(o, 8), 0);
    b.marker(kind::POINTERS, at(o, 12));
    assert!(matches!(
        refused(&b.build()).kind,
        ErrorKind::OutOfRange { .. } | ErrorKind::Duplicate
    ));
}

#[test]
fn a_word_two_kinds_mark_is_refused() {
    let (mut b, o) = one_object();
    b.pointer(at(o, 8), o);
    b.marker(kind::META_OBJECTS, at(o, 8));
    let err = refused(&b.build());
    assert_eq!(err.kind, ErrorKind::Duplicate);
}

#[test]
fn a_duplicate_record_kind_is_refused() {
    let (mut b, _) = one_object();
    b.unknown_record(kind::TMET, 0, &[]);
    let err = refused(&b.build());
    assert_eq!((err.field, err.kind), ("record_kind", ErrorKind::Duplicate));
}

#[test]
fn a_string_without_its_nul_is_refused() {
    let (mut b, o) = one_object();
    let text = b.section(2);
    let target = b.block(text, b"abc", 1);
    b.put_u32(at(o, 8), target.0);
    b.marker(kind::RSTR, at(o, 8));
    let err = refused(&b.build());
    assert!(matches!(err.kind, ErrorKind::MissingNul { .. }), "{err}");
}

#[test]
fn reads_refuse_what_the_fixups_dont_explain() {
    let (mut b, o) = one_object();
    // A pointer-looking word with no fixup, and a present size word whose
    // pointer has none.
    b.put_u32(at(o, 8), 4);
    b.put_u32(at(o, 0x10), MEMORY_PRESENT | 4);
    b.put_u32(at(o, 0x14), 4);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let err = igz.reference(at(o, 8), "_ref").unwrap_err();
    assert_eq!(
        (err.field, err.kind),
        ("_ref", ErrorKind::Unsupported { value: 4 })
    );
    let err = igz.memory_ref(at(o, 0x10), "_data").unwrap_err();
    assert_eq!(err.kind, ErrorKind::Missing);
    // A null size word with a non-zero pointer.
    let err = igz.memory_ref(at(o, 0x0C), "_data").unwrap_err();
    assert!(matches!(err.kind, ErrorKind::Unsupported { .. }), "{err}");
    // Reads past a section and through a missing one.
    assert!(igz.u32_at(at(o, 0x20), "past").is_err());
    assert!(igz.u32_at(Location::new(3, 0), "nowhere").is_err());
    assert_eq!(o.offset_by(MAX_OFFSET), None);
}

#[test]
fn headers_other_than_giants_are_refused() {
    let (b, _) = one_object();
    let good = b.build();
    let fixups = be(&good, SECTION_TABLE_AT) as usize;

    let mut bytes = good.clone();
    bytes[3] = 2;
    assert!(matches!(refused(&bytes).kind, ErrorKind::BadMagic { .. }));

    let mut bytes = good.clone();
    put(&mut bytes, 4, 8);
    assert!(matches!(
        refused(&bytes).kind,
        ErrorKind::UnsupportedVersion { found: 8, .. }
    ));

    let mut bytes = good.clone();
    put(&mut bytes, 0xC, 1);
    assert_eq!(refused(&bytes).field, "header_word");

    let mut bytes = good.clone();
    put(&mut bytes, fixups + 4, 7);
    assert_eq!(refused(&bytes).field, "fixup_version");

    // A non-zero word in a record header.
    let mut bytes = good.clone();
    put(&mut bytes, fixups + FIXUP_HEADER_LEN + 4, 1);
    assert_eq!(refused(&bytes).field, "record_header");

    // No records at all: TMET is required.
    let mut bytes = good.clone();
    put(&mut bytes, fixups + 0x10, 0);
    let err = refused(&bytes);
    assert_eq!((err.field, err.kind), ("TMET", ErrorKind::Missing));

    // More records than the section holds.
    let mut bytes = good;
    put(&mut bytes, fixups + 0x10, 0x10_0000);
    assert_eq!(refused(&bytes).field, "record_count");
}

#[test]
fn object_containing_finds_the_extent() {
    let bytes = sample();
    let igz = Igz::parse(&bytes).unwrap();
    let image = igz.objects()[1];
    assert_eq!(
        igz.object_containing(at(image.at, 0x5C)).map(|o| o.0),
        Some(1)
    );
    assert_eq!(igz.object_at(image.at).map(|o| o.0), Some(1));
    assert_eq!(igz.object_at(at(image.at, 4)), None);
    assert_eq!(igz.object_containing(Location(0)), None);
    assert_eq!(image.field(0x60), None);
    assert_eq!(image.field(0x5C), Some(at(image.at, 0x5C)));
}

#[test]
fn a_memory_ref_needs_the_fixups_its_kind_has() {
    let (mut b, o) = one_object();
    // An import, then a pointer: looks present, but the size word is marked.
    b.import(at(o, 8), "metaobject", "igA");
    b.pointer(at(o, 0xC), o);
    // A handle word of 0, then a zero word: looks null, but isn't kind 0xF.
    let data = b.section(4);
    b.handle(at(o, 0x10), data, &[1, 2, 3, 4], 0);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let err = igz.memory_ref(at(o, 8), "_data").unwrap_err();
    assert_eq!(
        err.kind,
        ErrorKind::Unsupported {
            value: INDEX_FLAG.into()
        }
    );
    let err = igz.memory_ref(at(o, 0x10), "_data").unwrap_err();
    assert_eq!(err.kind, ErrorKind::Missing);
    // Two unmarked zero words aren't a null ref either.
    let err = igz.memory_ref(at(o, 0x18), "_data").unwrap_err();
    assert_eq!(err.kind, ErrorKind::Missing);
}

#[test]
fn a_marker_on_an_object_type_word_is_refused() {
    let mut b = IgzBuilder::new();
    b.object(0, "igA", 8);
    let o = b.object(0, "igObjectList", 0x10);
    b.marker(kind::POINTERS, o);
    let err = refused(&b.build());
    assert_eq!((err.field, err.kind), ("pointer", ErrorKind::Duplicate));
}

#[test]
fn many_strings_into_one_run_resolve_from_one_scan() {
    let mut b = IgzBuilder::new();
    let o = b.object(0, "igTest", 0x400);
    let text = b.section(1);
    let run = b.block(text, b"abcdefghij\0klm\0", 1);
    let other = b.section(1);
    let tail = b.block(other, b"xyz\0", 1);
    // Every byte of both runs, the NULs included, out of order.
    let mut targets: Vec<(Location, &[u8])> = Vec::new();
    for i in (0..15u32).rev() {
        let s: &[u8] = b"abcdefghij\0klm\0";
        let from = i as usize;
        let len = s[from..].iter().position(|c| *c == 0).unwrap();
        targets.push((at(run, i), &s[from..from + len]));
    }
    for i in 0..4u32 {
        targets.push((at(tail, i), &b"xyz"[i as usize..]));
    }
    for (j, (target, _)) in targets.iter().enumerate() {
        let word = at(o, 8 + 4 * j as u32);
        b.put_u32(word, target.0);
        b.marker(kind::RSTR, word);
    }
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    for (j, (_, want)) in targets.iter().enumerate() {
        let word = at(o, 8 + 4 * j as u32);
        assert_eq!(igz.reference(word, "s").unwrap(), Ref::String(want));
    }
}

#[test]
fn a_duplicate_type_name_is_refused() {
    let mut b = IgzBuilder::new();
    b.object(0, "igA", 8);
    b.object(0, "igB", 8);
    let mut bytes = b.build();
    let i = bytes.windows(4).position(|w| w == b"igB\0").unwrap();
    bytes[i + 2] = b'A';
    let err = refused(&bytes);
    assert_eq!((err.field, err.kind), ("TMET", ErrorKind::Duplicate));
}
