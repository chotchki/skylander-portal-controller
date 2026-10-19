use super::builder::{dict, joined, level, pack};
use super::*;

#[test]
fn a_pack_reads_its_name_id_and_strings() {
    let bytes = pack("ENGLISH", ENGLISH, &["Eruptor", "", "Señor ✓ 𝄞"]);
    let p = LanguagePack::read(&Igz::parse(&bytes).unwrap()).unwrap();
    assert_eq!(p.name.as_deref(), Some("ENGLISH"));
    assert_eq!(p.lang_id, 6);
    assert_eq!(p.strings, ["Eruptor", "", "Señor ✓ 𝄞"]);
}

#[test]
fn a_pack_joins_to_its_level_in_address_order() {
    // Keys a and c sit in a later section than b and d, so address order
    // is b, d, a, c: the order the pack's strings follow.
    let (bytes, keys) = level(&["a", "b", "c", "d"]);
    let level = Igz::parse(&bytes).unwrap();
    let pack_bytes = pack("ENGLISH", ENGLISH, &["B", "D", "A", "C"]);
    let p = LanguagePack::read(&Igz::parse(&pack_bytes).unwrap()).unwrap();
    let joined = join(&level, &p).unwrap();
    let index = |at| level.object_at(at).unwrap().0;
    assert_eq!(
        joined,
        [
            (index(keys[1]), "B"),
            (index(keys[3]), "D"),
            (index(keys[0]), "A"),
            (index(keys[2]), "C"),
        ]
    );
    let texts = Texts::new(&level, vec![p]).unwrap();
    assert_eq!(texts.key_count(), 4);
    assert_eq!(texts.text(index(keys[0]), ENGLISH), Some("A"));
    assert_eq!(texts.text(index(keys[0]), 8), None);
}

#[test]
fn a_short_or_long_pack_is_refused() {
    let (bytes, _) = level(&["a", "b", "c"]);
    let level = Igz::parse(&bytes).unwrap();
    for strings in [&["A", "B"][..], &["A", "B", "C", "D"]] {
        let pack_bytes = pack("ENGLISH", ENGLISH, strings);
        let p = LanguagePack::read(&Igz::parse(&pack_bytes).unwrap()).unwrap();
        let e = join(&level, &p).unwrap_err();
        assert_eq!(
            e.kind,
            ErrorKind::CountMismatch {
                found: strings.len() as u64,
                expected: 3
            }
        );
        assert!(Texts::new(&level, vec![p]).is_err());
    }
}

#[test]
fn texts_carry_every_language_and_refuse_a_repeated_id() {
    let (bytes, keys) = level(&["k0", "k1"]);
    let level = Igz::parse(&bytes).unwrap();
    let read = |name, id, s: &[&str]| {
        LanguagePack::read(&Igz::parse(&pack(name, id, s)).unwrap()).unwrap()
    };
    let packs = vec![
        read("ENGLISH", 6, &["one", "zero"]),
        read("FRENCH", 8, &["un", "zéro"]),
    ];
    let texts = Texts::new(&level, packs).unwrap();
    let k0 = level.object_at(keys[0]).unwrap().0;
    let all: Vec<(i32, &str)> = texts
        .texts(k0)
        .unwrap()
        .into_iter()
        .map(|(l, t)| (l.id, t))
        .collect();
    assert_eq!(all, [(6, "zero"), (8, "zéro")]);
    assert_eq!(texts.languages()[1].name.as_deref(), Some("FRENCH"));

    let twice = vec![
        read("ENGLISH", 6, &["a", "b"]),
        read("ENGLISH", 6, &["a", "b"]),
    ];
    assert_eq!(
        Texts::new(&level, twice).unwrap_err().kind,
        ErrorKind::Duplicate
    );
}

#[test]
fn split_refuses_odd_unterminated_and_unpaired_text() {
    assert_eq!(split(&dict(&["ab", "c"])).unwrap(), ["ab", "c"]);
    assert_eq!(split(&[]).unwrap(), Vec::<String>::new());
    // An odd byte count.
    assert!(matches!(
        split(&[0, 0x41, 0]),
        Err((2, ErrorKind::Truncated { .. }))
    ));
    // Text after the last NUL.
    let mut d = dict(&["ab"]);
    d.extend_from_slice(&[0, 0x63]);
    assert!(matches!(split(&d), Err((6, ErrorKind::MissingNul { .. }))));
    // A lone high surrogate.
    assert_eq!(
        split(&[0xD8, 0x00, 0, 0]),
        Err((0, ErrorKind::InvalidUtf16))
    );
}

#[test]
fn a_file_without_or_with_two_packs_is_refused() {
    let (bytes, _) = level(&["a"]);
    let e = LanguagePack::read(&Igz::parse(&bytes).unwrap()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Missing);

    let mut b = crate::igz::builder::IgzBuilder::new();
    let blocks = b.section(0x10);
    for _ in 0..2 {
        let o = b.object(0, "LanguagePackInfo", 0x20);
        builder::pack_info(&mut b, o, blocks, "ENGLISH", 6, &dict(&["x"]));
    }
    let e = LanguagePack::read(&Igz::parse(&b.build()).unwrap()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Duplicate);
}

#[test]
fn the_seed_joins() {
    let bytes = joined(&["a", "b"], ENGLISH);
    let igz = Igz::parse(&bytes).unwrap();
    let p = LanguagePack::read(&igz).unwrap();
    let texts: Vec<&str> = join(&igz, &p)
        .unwrap()
        .into_iter()
        .map(|(_, t)| t)
        .collect();
    assert_eq!(texts, ["A", "B"]);
}
