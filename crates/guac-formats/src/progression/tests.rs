use super::builder::{ClampSpec, Disc, Odd, ProgressionSpec, RowSpec, disc, sample};
use super::*;

fn read(d: &Disc) -> Result<SaveTable, FormatError> {
    let init = Igz::parse(&d.init_setup).unwrap();
    let global = Igz::parse(&d.global).unwrap();
    let boot = Igz::parse(&d.bootstrap).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    SaveTable::read(&init, &global, &exports)
}

fn clamp(d: &Disc) -> Result<u32, FormatError> {
    gold_clamp(&Igz::parse(&d.global).unwrap())
}

/// Each row as (param, kind, [(word, bit, member)], width, per field, limit).
type Row<'s> = (
    &'s str,
    SaveKind,
    Vec<(u32, u32, Option<&'s str>)>,
    u32,
    u32,
    u32,
);

fn rows(t: &SaveTable) -> Vec<Row<'_>> {
    t.params
        .iter()
        .map(|p| {
            (
                p.param.as_str(),
                p.kind,
                p.fields
                    .iter()
                    .map(|f| (f.word, f.bit, f.member.as_deref()))
                    .collect(),
                p.width,
                p.bits_per_field,
                p.limit,
            )
        })
        .collect()
}

#[test]
fn each_call_reads_into_a_row_and_the_table_into_its_levels() {
    let t = read(&disc(&sample())).unwrap();
    assert_eq!(t.level_xp, [0, 400, 1100, 2500, 4500]);
    use SaveKind::*;
    assert_eq!(
        rows(&t),
        [
            ("pp_Exp", Plain, vec![(0, 0, None)], 12, 0, 3000),
            ("pp_Money", Plain, vec![(0, 12, None)], 14, 0, 9999),
            ("pp_CumulativeTime", Plain, vec![(1, 0, None)], 20, 0, 0),
            // A loop over the set, indexed and placed by the pass.
            (
                "pp_UpgradeBits",
                Plain,
                vec![
                    (2, 0, Some("pp_Path")),
                    (2, 1, Some("pp_Upgrade(1)")),
                    (2, 2, Some("pp_Upgrade(2)")),
                    (2, 3, Some("pp_Upgrade(3)")),
                ],
                1,
                0,
                0
            ),
            // The bit after them: the set's count.
            ("pp_ElementCount", Plain, vec![(2, 4, None)], 2, 0, 3),
            // The pass plus an offset.
            (
                "pp_Platforms",
                Plain,
                vec![
                    (2, 20, Some("pp_Platform(0)")),
                    (2, 21, Some("pp_Platform(1)")),
                    (2, 22, Some("pp_Platform(2)")),
                ],
                1,
                0,
                0
            ),
            // The body sizes an empty set to 6; two 16-bit fields a word.
            (
                "pp_Letters",
                Plain,
                vec![
                    (4, 0, None),
                    (4, 16, None),
                    (5, 0, None),
                    (5, 16, None),
                    (6, 0, None),
                    (6, 16, None),
                ],
                16,
                0,
                0
            ),
            // The table's last entry less pp_Exp's own limit: 4500 - 3000.
            ("pp_Exp", Spillover, vec![(7, 0, None)], 11, 0, 1500),
            ("pp_Hat", MirrorAndClear, vec![(7, 16, None)], 6, 0, 0),
            (
                "pp_Challenges",
                ExtendedBitfield,
                vec![(8, 0, None)],
                9,
                1,
                0
            ),
        ]
    );
    assert_eq!(clamp(&disc(&sample())), Ok(9000));
}

#[test]
fn rows_keep_their_call_positions_in_order() {
    let t = read(&disc(&sample())).unwrap();
    let positions: Vec<usize> = t.params.iter().map(|p| p.position).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
    assert_eq!(SaveKind::Spillover.macro_name(), SPILLOVER);
}

/// The roster builder's sample disc carries the same progression in its
/// `init_setup` and `global.bld`, beside the toy table.
#[test]
fn the_roster_sample_holds_the_progression() {
    use crate::roster::builder as roster;
    let d = roster::disc(&roster::sample());
    let t = read(&Disc {
        init_setup: d.init_setup.clone(),
        global: d.global.clone(),
        bootstrap: d.bootstrap.clone(),
    })
    .unwrap();
    assert_eq!(t, read(&disc(&sample())).unwrap());
    assert_eq!(gold_clamp(&Igz::parse(&d.global).unwrap()), Ok(9000));
}

fn odd(odd: Odd) -> FormatError {
    let mut spec = sample();
    spec.rows.push(RowSpec::Odd {
        param: "pp_Odd",
        odd,
    });
    read(&disc(&spec)).unwrap_err()
}

#[test]
fn an_argument_shape_the_reader_doesnt_know_is_refused_by_name() {
    assert_eq!(odd(Odd::FloatBit).field, "_type");
    assert_eq!(odd(Odd::RandomBit).field, "_isRandom");
    // The body wrote that cell with a float: unknown when a row reads it.
    assert_eq!(odd(Odd::UnknownCell).field, "path");
    let e = odd(Odd::ValueAsParam);
    assert!(
        matches!(&e.kind, ErrorKind::WrongType { found, .. } if found == "ValueInfo"),
        "{e}"
    );
    assert_eq!(odd(Odd::UnknownParameter).field, "_varName");
}

#[test]
fn an_op_the_table_doesnt_hold_is_refused() {
    let mut spec = sample();
    spec.stray_op = true;
    let e = read(&disc(&spec)).unwrap_err();
    assert_eq!(
        e.kind,
        ErrorKind::WrongType {
            expected: "an op of the save-param table",
            found: "OpFindSubSet".into()
        }
    );
}

#[test]
fn the_level_table_must_fill_its_definition_and_rise() {
    // The definition sizes the table to fewer entries than the call fills.
    let mut spec = sample();
    spec.table_len = 4;
    let e = read(&disc(&spec)).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        (
            params::LEVEL_XP,
            ErrorKind::OutOfRange { value: 4, limit: 4 }
        )
    );
    // One left out reads 0, which doesn't rise.
    let mut spec = sample();
    spec.level_args = vec![1, 2, 4];
    assert_eq!(read(&disc(&spec)).unwrap_err().field, params::LEVEL_XP);
    // One passed twice.
    let mut spec = sample();
    spec.level_args = vec![1, 2, 2, 3, 4];
    assert_eq!(read(&disc(&spec)).unwrap_err().field, "_varName");
    // Falling.
    let mut spec = sample();
    spec.level_xp = vec![400, 300, 2500, 4500];
    assert_eq!(read(&disc(&spec)).unwrap_err().field, params::LEVEL_XP);
    // Too short to hold a level 2.
    let mut spec = sample();
    spec.table_len = 1;
    assert_eq!(read(&disc(&spec)).unwrap_err().field, DEFINE_LEVELUP);
}

#[test]
fn a_runaway_loop_is_refused_and_a_long_one_runs() {
    let mut spec = sample();
    spec.bare_loop = Some(MAX_PASSES as i32);
    assert!(read(&disc(&spec)).is_ok());
    spec.bare_loop = Some(MAX_PASSES as i32 + 1);
    let e = read(&disc(&spec)).unwrap_err();
    assert_eq!(e.field, "loop value");
    assert!(matches!(e.kind, ErrorKind::OutOfRange { .. }), "{e}");
}

#[test]
fn the_clamp_is_one_check_that_sets_its_own_value() {
    let with = |c| {
        clamp(&disc(&ProgressionSpec {
            clamp: c,
            ..sample()
        }))
    };
    assert_eq!(with(ClampSpec::One(65)), Ok(65));
    for (c, kind) in [
        (ClampSpec::None, ErrorKind::Missing),
        (ClampSpec::Mismatched(65), ErrorKind::Missing),
        (ClampSpec::Twice(65), ErrorKind::Duplicate),
    ] {
        let e = with(c).unwrap_err();
        assert_eq!((e.field, e.kind), (AWARD_MONEY, kind), "{c:?}");
    }
    let e = with(ClampSpec::One(0)).unwrap_err();
    assert_eq!(e.field, AWARD_MONEY);
}

#[test]
fn a_file_without_the_macros_is_refused_by_their_names() {
    let d = disc(&sample());
    // global.bld defines no save-param table, init_setup no Award Money.
    let global = Igz::parse(&d.global).unwrap();
    let boot = Igz::parse(&d.bootstrap).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    let e = SaveTable::read(&global, &global, &exports).unwrap_err();
    assert_eq!((e.field, e.kind), (SAVE_TABLE, ErrorKind::Missing));
    let e = gold_clamp(&Igz::parse(&d.init_setup).unwrap()).unwrap_err();
    assert_eq!((e.field, e.kind), (AWARD_MONEY, ErrorKind::Missing));
}

#[test]
fn relations_follow_the_disc_reading() {
    let expect = [
        (0, [true, true, false]),
        (1, [false, true, false]),
        (2, [false, true, true]),
        (3, [true, false, false]),
        (4, [false, false, true]),
        (5, [true, false, true]),
    ];
    for (code, want) in expect {
        let got = [1, 2, 3].map(|a| holds(code, a, 2).unwrap());
        assert_eq!(got, want, "{} ({code})", RELATIONS[code as usize]);
    }
    assert_eq!(holds(6, 1, 1).unwrap_err().field, "_relOperator");
    assert_eq!(level_xp_index("level xp (14)"), Some(14));
    assert_eq!(level_xp_index("level xp 14"), None);
}
