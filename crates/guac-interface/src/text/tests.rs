//! The measure, the fill and the glyphs on fonts built in code (AT.6).

use guac_ir::interface::{Font, Glyph};
use proptest::prelude::*;

use super::*;

fn glyph(c: char, step: f32) -> Glyph {
    Glyph {
        char: c as u16,
        step,
        offset: [0.0, 0.0],
        size: [step, 82.0],
        uv_min: [0.0, 0.0],
        uv_max: [0.01, 0.08],
        kern: [0; 4],
    }
}

/// A font 82 high, baseline 0, each letter its own step, sorted by char.
fn font(padding: f32, kern: f32) -> Font {
    let mut glyphs: Vec<Glyph> = " 0123456789abcdefghijklmnopqrstuvwxyz@-.,:"
        .chars()
        .map(|c| {
            glyph(
                c,
                if c == ' ' {
                    22.0
                } else {
                    30.0 + (c as u32 % 7) as f32
                },
            )
        })
        .collect();
    glyphs.sort_by_key(|g| g.char);
    Font {
        name: None,
        source: None,
        height: 82.0,
        base_line: 0.0,
        char_padding: padding,
        kern_amount: kern,
        color: [0.5, 0.5, 0.5, 1.0],
        atlas: "textures/x.ktx2".into(),
        atlas_size: [1024, 1024],
        glyphs,
    }
}

fn u(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn st(text: &str, flags: u32, integer: i32, decimal: i32) -> Str {
    Str {
        units: u(text),
        flags,
        integer_digits: integer,
        decimal_digits: decimal,
    }
}

fn filled(s: &Str, v: f32) -> String {
    String::from_utf16(&fill(s, v)).unwrap()
}

const AT: u32 = string_flags::HAS_AT;
const TIME: u32 = string_flags::TIME;
const COMMAS: u32 = string_flags::COMMAS;

#[test]
fn a_number_fills_its_digits_padding_and_commas() {
    assert_eq!(filled(&st("@", AT, 0, 0), 290.0), "290");
    assert_eq!(filled(&st("@", AT, 0, 0), 0.0), "0");
    assert_eq!(filled(&st("@", AT, 0, 0), -7.0), "-7");
    assert_eq!(filled(&st("x @ y", AT, 0, 0), 42.0), "x 42 y");
    // Exactly that many digits: padded, the higher ones dropped.
    assert_eq!(filled(&st("@", AT, 4, 0), 42.0), "0042");
    assert_eq!(filled(&st("@", AT, 2, 0), 12345.0), "45");
    // A comma after every third digit when another follows.
    assert_eq!(filled(&st("@", AT | COMMAS, 0, 0), 1234567.0), "1,234,567");
    assert_eq!(filled(&st("@", AT | COMMAS, 0, 0), 123.0), "123");
    assert_eq!(filled(&st("@", AT | COMMAS, 0, 0), 1000.0), "1,000");
    assert_eq!(filled(&st("@", AT | COMMAS, 4, 0), 7.0), "0,007");
    // The first `@` takes it, later ones go, no flag keeps them.
    assert_eq!(filled(&st("@/@", AT, 0, 0), 5.0), "5/");
    assert_eq!(filled(&st("@", 0, 0, 0), 5.0), "@");
    // The value reads as the integer it stores without decimals.
    assert_eq!(filled(&st("@", AT, 0, 0), 9.9), "9");
}

#[test]
fn decimals_round_at_their_last_place() {
    // 0.049995 is a hair under half a place: 2.25 stays 2.2.
    assert_eq!(filled(&st("@", AT, 0, 1), 2.25), "2.2");
    assert_eq!(filled(&st("@", AT, 0, 1), 2.26), "2.3");
    assert_eq!(filled(&st("@", AT, 0, 1), 2.94), "2.9");
    assert_eq!(filled(&st("@", AT, 0, 2), 0.5), "0.50");
    assert_eq!(filled(&st("@", AT, 0, 2), 3.999), "4.00");
    assert_eq!(filled(&st("@", AT, 0, 1), -1.26), "-1.3");
    assert_eq!(filled(&st("@%", AT, 0, 1), 12.0), "12.0%");
    assert_eq!(filled(&st("@", AT, 3, 1), 5.0), "005.0");
}

#[test]
fn a_time_fills_its_fields_from_the_seconds() {
    assert_eq!(filled(&st("@", TIME, 4, 0), 125.0), "02:05");
    assert_eq!(filled(&st("@", TIME, 0, 0), 3725.0), "01:02:05");
    assert_eq!(filled(&st("@", TIME, 3, 0), 125.0), "2:05");
    // The last single digit taken mod 10 only when above 10.
    assert_eq!(filled(&st("@", TIME, 3, 0), 11.0 * 60.0), "1:00");
    assert_eq!(filled(&st("@", TIME, 3, 0), 10.0 * 60.0), "0:00");
    assert_eq!(filled(&st("@", TIME, -1, 0), 99.0), "");
    // A time fills without *has an `@`*.
    assert_eq!(filled(&st("T @", TIME, 4, 0), 61.0), "T 01:01");
}

#[test]
fn a_run_adds_steps_and_padding_less_each_kern_corner_pair() {
    let mut f = font(2.0, 3.0);
    let at = |f: &Font, c: char| f.glyphs.iter().position(|g| g.char == c as u16).unwrap();
    let (a, b) = (at(&f, 'a'), at(&f, 'b'));
    let (sa, sb) = (f.glyphs[a].step, f.glyphs[b].step);
    // No corners: steps and padding.
    assert_eq!(Metrics::new(&f).run_width(&u("ab")), sa + 2.0 + sb + 2.0);
    // Each pair alone: the top (a's top-right + b's top-left) against the
    // bottom (a's bottom-right + b's bottom-left), the smaller times 3.
    for (ak, bk, kern) in [
        ([0, 2, 0, 5], [1, 0, 3, 0], 3.0 * 3.0),
        ([0, 4, 0, 1], [2, 0, 1, 0], 2.0 * 3.0),
        ([0, 1, 0, 1], [0, 0, 0, 0], 3.0),
        ([9, 0, 9, 0], [0, 9, 0, 9], 0.0),
    ] {
        f.glyphs[a].kern = ak;
        f.glyphs[b].kern = bk;
        let m = Metrics::new(&f);
        assert_eq!(
            m.run_width(&u("ab")),
            sa + 2.0 + sb + 2.0 - kern,
            "{ak:?} {bk:?}"
        );
        // The first character kerns against nothing.
        assert_eq!(m.run_width(&u("b")), sb + 2.0);
    }
    // A kern amount of 0 kerns nothing.
    f.kern_amount = 0.0;
    assert_eq!(Metrics::new(&f).run_width(&u("ab")), sa + 2.0 + sb + 2.0);
}

#[test]
fn a_character_the_font_lacks_adds_nothing_and_breaks_the_kern() {
    let mut f = font(0.0, 1.0);
    let a = f.glyphs.iter().position(|g| g.char == 'a' as u16).unwrap();
    f.glyphs[a].kern = [4, 4, 4, 4];
    let m = Metrics::new(&f);
    let sa = f.glyphs[a].step;
    assert!(m.glyph('Q' as u16).is_none());
    assert_eq!(m.run_width(&u("aQa")), sa + sa, "no kern across the gap");
    assert_eq!(m.run_width(&u("aa")), sa + sa - 8.0);
    // Drawn: no quad; it moves x by the padding only when the line's last
    // character is a space.
    let p = font(5.0, 0.0);
    let m = Metrics::new(&p);
    let g = glyphs(&m, f32::MAX, Justify::Left, &u("aQa"));
    assert_eq!(g.len(), 2);
    assert_eq!(g[1].rect[0], sa + 5.0, "Q moved nothing");
    // The character it reads is the one past the line's end: past a line
    // feed, the next line's first.
    let g = glyphs(&m, f32::MAX, Justify::Left, &u("aQa\n b"));
    assert_eq!(g[1].rect[0], sa + 5.0 + 5.0, "Q moved the padding");
    let g = glyphs(&m, f32::MAX, Justify::Left, &u("aQa "));
    assert_eq!(g[1].rect[0], sa + 5.0, "a blank of its own line isn't read");
}

#[test]
fn a_strings_width_is_its_widest_line_and_its_lines_drop_trailing_ends() {
    let f = font(0.0, 0.0);
    let m = Metrics::new(&f);
    assert_eq!(m.string_width(&u("ab\nabcd\nc")), m.run_width(&u("abcd")));
    assert_eq!(line_count(&u("ab")), 1);
    assert_eq!(line_count(&u("ab\ncd")), 2);
    assert_eq!(line_count(&u("ab\r\ncd")), 3);
    assert_eq!(line_count(&u("ab\n\n")), 1);
    assert_eq!(line_count(&u("")), 1);
    assert_eq!(m.lines_height(3), 3.0 * 82.0);
}

#[test]
fn lines_fit_a_height_with_a_thousandth_of_slack() {
    let f = font(0.0, 0.0);
    let m = Metrics::new(&f);
    assert_eq!(m.lines_that_fit(164.0), 2);
    assert_eq!(m.lines_that_fit(163.9995), 2);
    assert_eq!(m.lines_that_fit(163.99), 1);
    assert_eq!(m.lines_that_fit(0.0), 0);
}

#[test]
fn a_wrap_breaks_at_the_width_keeping_blanks_and_one_long_word() {
    let f = font(0.0, 0.0);
    let m = Metrics::new(&f);
    let (aa, sp) = (m.run_width(&u("aa")), 22.0);
    // "aa aa aa" at the width of "aa aa": two lines, the first keeping
    // its trailing blank.
    let (out, n) = m.wrap(aa + sp + aa, &u("aa aa aa"));
    assert_eq!(
        (String::from_utf16(&out).unwrap().as_str(), n),
        ("aa aa \naa", 2)
    );
    // A word wider than the width stands alone on its line.
    let (out, n) = m.wrap(10.0, &u("aaaa b"));
    assert_eq!(
        (String::from_utf16(&out).unwrap().as_str(), n),
        ("aaaa \nb", 2)
    );
    // 0 never wraps; a source line feed always does.
    assert_eq!(m.wrap(0.0, &u("aa aa aa")).1, 1);
    assert_eq!(m.wrap(0.0, &u("aa\naa")).1, 2);
    // An empty source line is counted and written as nothing.
    let (out, n) = m.wrap(0.0, &u("a\n\nb"));
    assert_eq!((String::from_utf16(&out).unwrap().as_str(), n), ("a\nb", 3));
    // Each line's width is its words and blanks measured alone.
    let lines = m.lines(aa + sp + aa, &u("aa aa aa"));
    assert_eq!(lines[0].width, aa + sp + aa + sp);
    assert_eq!((lines[1].start, lines[1].end), (6, 8));
}

#[test]
fn an_unsized_text_measures_its_string_at_its_point_size() {
    let f = font(0.0, 0.0);
    let m = Metrics::new(&f);
    let t = TextIn {
        flags: flags::POINT_SIZE_SET,
        point_size: 41,
        width: 999.0,
        height: 999.0,
    };
    let s = 41.0 / 82.0;
    let w = m.string_width(&u("ab\nabc"));
    assert_eq!(
        size(&m, &t, &u("ab\nabc")),
        [w * s, 82.0 * s],
        "one line high"
    );
    let out = layout(&m, &t, &u("ab\nabc"));
    assert_eq!(out.size, [w * s, 2.0 * 82.0 * s]);
    assert_eq!(out.scale, [s, s]);
    assert_eq!(natural_size(&m, &t, &u("ab\nabc")), out.size);
    // Without a point size the scale is 1.
    let one = TextIn { flags: 0, ..t };
    assert_eq!(layout(&m, &one, &u("ab")).scale, [1.0, 1.0]);
}

#[test]
fn a_sized_text_stretches_or_fits() {
    let f = font(0.0, 0.0);
    let m = Metrics::new(&f);
    let w = m.string_width(&u("abc"));
    let s = 0.5;
    let stretch = TextIn {
        flags: flags::SIZED | flags::POINT_SIZE_SET,
        point_size: 41,
        width: 60.0,
        height: 30.0,
    };
    let out = layout(&m, &stretch, &u("abc"));
    assert_eq!(out.size, [60.0, 30.0]);
    assert_eq!(out.scale, [60.0 / w, 30.0 / 82.0]);
    // Fits: narrower than the string shrinks x alone, x never above y.
    let fits = TextIn {
        flags: stretch.flags | flags::FITS,
        width: w * s * 0.8,
        height: 200.0,
        ..stretch
    };
    let out = layout(&m, &fits, &u("abc"));
    assert_eq!(out.scale, [s * (fits.width / (w * s)), s]);
    assert_eq!(out.size, [fits.width, 82.0 * s]);
    // Shorter than a line: y shrinks, and x with it.
    let short = TextIn {
        height: 20.5,
        width: 999.0,
        ..fits
    };
    let out = layout(&m, &short, &u("abc"));
    assert_eq!(out.scale[1], s * (20.5 / 41.0));
    assert_eq!(out.scale[0], out.scale[1]);
    assert_eq!(out.size[1], 20.5);
}

#[test]
fn a_box_that_wraps_and_fits_shrinks_into_its_height() {
    let f = font(0.0, 0.0);
    let m = Metrics::new(&f);
    let text = u("aaa bbb ccc ddd eee fff");
    let boxed = TextIn {
        flags: flags::SIZED | flags::WRAPS | flags::FITS,
        point_size: 0,
        width: 300.0,
        height: 164.0,
    };
    // At 300 the string takes more than the 2 lines 164 holds.
    assert!(m.wrap(300.0, &text).1 > 2);
    let out = layout(&m, &boxed, &text);
    let lines = line_count(&out.units);
    assert!(lines >= 2, "{lines}");
    assert!(out.scale[0] <= out.scale[1] && out.scale[1] <= 1.0);
    assert!(out.size[1] <= 164.0 + 1e-3, "{:?}", out.size);
    assert_eq!(out.size[0], 300.0);
    // Room enough: one wrap, at full scale.
    let roomy = TextIn {
        height: 82.0 * 10.0,
        ..boxed
    };
    let out = layout(&m, &roomy, &text);
    assert_eq!(out.scale, [1.0, 1.0]);
    assert_eq!(out.units, m.wrap(300.0, &text).0);
    // One word wider than the box wraps at its own width and shrinks x.
    let word = u("aaaaaaaaaaaa");
    let narrow = TextIn {
        width: 100.0,
        ..roomy
    };
    let out = layout(&m, &narrow, &word);
    assert_eq!(out.scale[0], 100.0 / m.string_width(&word));
}

#[test]
fn natural_size_wraps_and_keeps_the_lines_that_fit() {
    let f = font(0.0, 0.0);
    let m = Metrics::new(&f);
    let (aa, sp) = (m.run_width(&u("aa")), 22.0);
    let t = TextIn {
        flags: flags::SIZED | flags::WRAPS,
        point_size: 0,
        width: aa + sp + aa,
        height: 164.0,
    };
    // Two lines kept of "aa aa aa": the last kept one measured on the
    // source at the wrapped text's offset (7, one past "aa aa "'s line
    // feed): "a"; the first, with its trailing blank, is wider than that
    // and than the width, so it is the width read.
    let [w, h] = natural_size(&m, &t, &u("aa aa aa"));
    assert_eq!(h, 2.0 * 82.0);
    assert_eq!(w, m.run_width(&u("aa aa ")));
    // Within the width, the last kept line's.
    let wide = TextIn {
        width: m.run_width(&u("aa aa ")),
        ..t
    };
    assert_eq!(
        natural_size(&m, &wide, &u("aa aa aa"))[0],
        m.run_width(&u("a"))
    );
    // A first line wider than the last and past the width is the width
    // read.
    let long = TextIn { width: 10.0, ..t };
    let [w, _] = natural_size(&m, &long, &u("aaaa b"));
    assert_eq!(w, m.run_width(&u("aaaa ")));
    // One line fits: the first, its source's lines high.
    let low = TextIn { height: 82.0, ..t };
    assert_eq!(
        natural_size(&m, &low, &u("aa aa aa")),
        [m.run_width(&u("aa aa aa")), 82.0]
    );
}

#[test]
fn glyphs_sit_on_their_lines_justified_within_the_break_width() {
    let f = font(1.0, 0.0);
    let m = Metrics::new(&f);
    let ab = m.run_width(&u("ab"));
    let left = glyphs(&m, 200.0, Justify::Left, &u("ab\nb"));
    assert_eq!(left[0].rect[0], 0.0);
    let sa = f.glyphs.iter().find(|g| g.char == 'a' as u16).unwrap().step;
    assert_eq!(left[1].rect[0], sa + 1.0);
    // The line feed is drawn with its line, as a space; the next line one
    // height lower.
    assert_eq!(left[2].char, ' ' as u16);
    assert_eq!(left[3].rect[1], 82.0);
    let right = glyphs(&m, 200.0, Justify::Right, &u("ab"));
    assert_eq!(right[0].rect[0], 200.0 - ab);
    let centre = glyphs(&m, 200.0, Justify::Center, &u("ab"));
    assert_eq!(centre[0].rect[0], ((200.0 - ab) * 0.5f32 + 0.5).floor());
    // Leading blanks count twice in the centring.
    let lead = glyphs(&m, 200.0, Justify::Center, &u(" ab"));
    let line = 23.0 + ab;
    assert_eq!(
        lead[0].rect[0],
        ((200.0 - line - 23.0) * 0.5f32 + 0.5).floor()
    );
    // Each glyph's quad is its offset over its size with its atlas box.
    assert_eq!(left[0].rect[3] - left[0].rect[1], 82.0);
    assert_eq!(left[0].uv, [0.0, 0.0, 0.01, 0.08]);
}

#[test]
fn a_sized_text_breaks_at_its_width_over_its_scale() {
    assert_eq!(
        break_width(true, 40.75, 0.268_092_1, 1),
        (152.0, Justify::Center)
    );
    assert_eq!(break_width(true, 10.0, 1.0, 2).1, Justify::Right);
    assert_eq!(break_width(true, 10.4, 1.0, 0), (10.0, Justify::Left));
    assert_eq!(break_width(true, 10.6, 1.0, 0).0, 11.0);
    assert_eq!(break_width(false, 10.0, 1.0, 2), (f32::MAX, Justify::Left));
}

/// Words the test font has every glyph for, so nothing measures as missing.
fn sentence() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z0-9]{1,9}", 0..12).prop_map(|w| w.join(" "))
}

proptest! {
    /// Any string wrapped at any width: every line, its trailing blanks
    /// off, measures within the width unless it holds one word.
    #[test]
    fn a_wrap_never_runs_past_its_width_but_for_one_word(
        text in sentence(),
        width in 1.0f32..2000.0,
        kern in 0.0f32..3.0,
        padding in 0.0f32..4.0,
    ) {
        let f = font(padding, kern);
        let m = Metrics::new(&f);
        let units = u(&text);
        let (out, n) = m.wrap(width, &units);
        if !units.is_empty() {
            prop_assert_eq!(n as usize, out.split(|&c| c == LF).count());
        }
        for line in out.split(|&c| c == LF) {
            let blanks = line.iter().rev().take_while(|&&c| c == SPACE).count();
            let trimmed = &line[..line.len() - blanks];
            let words = trimmed.split(|&c| c == SPACE).filter(|w| !w.is_empty()).count();
            prop_assert!(
                words <= 1 || m.run_width(trimmed) <= width * (1.0 + 1e-5) + 1e-3,
                "{:?} at {}: {}", String::from_utf16_lossy(line), width, m.run_width(trimmed)
            );
        }
    }
}

/// A font whose letters are all 30 wide and its space 22, so a fit is
/// worked by hand.
fn even_font() -> Font {
    let mut f = font(0.0, 0.0);
    for g in &mut f.glyphs {
        g.step = if g.char == SPACE { 22.0 } else { 30.0 };
        g.size[0] = g.step;
    }
    f
}

#[test]
fn the_fit_takes_the_pass_that_wraps_into_the_lines_that_fit() {
    let f = even_font();
    let m = Metrics::new(&f);
    // Four 90-wide words in a 200 by 164 box: four lines at 200, two fit.
    // The pass for 2 lines tries 426 / 2 = 213 first, which wraps into
    // two lines 224 wide ("aaa aaa " with its blank): x at 200/224, y 1,
    // and it stops there (200/224 against 1 isn't under 0.5).
    let text = u("aaa aaa aaa aaa");
    let t = TextIn {
        flags: flags::SIZED | flags::WRAPS | flags::FITS,
        point_size: 0,
        width: 200.0,
        height: 164.0,
    };
    assert_eq!(m.wrap(200.0, &text).1, 4);
    let out = layout(&m, &t, &text);
    assert_eq!(out.scale, [200.0 / 224.0, 1.0]);
    assert_eq!(out.size, [200.0, 164.0]);
    assert_eq!(out.units, u("aaa aaa \naaa aaa"));
    // At half the point size the same box holds them on two lines at 0.5.
    let half = TextIn {
        flags: t.flags | flags::POINT_SIZE_SET,
        point_size: 41,
        ..t
    };
    let out = layout(&m, &half, &text);
    assert_eq!(out.scale, [0.5, 0.5]);
    assert_eq!(out.units, m.wrap(400.0, &text).0);
}

#[test]
fn a_fit_takes_at_least_the_strings_own_lines() {
    // Three source lines in a box two high: the first pass is for 3 lines,
    // not the 2 that fit, and the height shrinks the three into the box.
    let f = even_font();
    let m = Metrics::new(&f);
    let t = TextIn {
        flags: flags::SIZED | flags::WRAPS | flags::FITS,
        point_size: 0,
        width: 300.0,
        height: 164.0,
    };
    let out = layout(&m, &t, &u("aaa\naaa\naaa"));
    let r = 164.0f32 / 246.0;
    assert_eq!(out.scale, [r, r]);
    assert_eq!(out.size, [300.0, 246.0 * r]);
}

#[test]
fn a_wrapped_natural_height_counts_the_source_from_the_last_kept_line() {
    // One line fits, but the source runs on past the kept line's line feed:
    // two lines high.
    let f = font(0.0, 0.0);
    let m = Metrics::new(&f);
    let t = TextIn {
        flags: flags::SIZED | flags::WRAPS,
        point_size: 0,
        width: 1000.0,
        height: 82.0,
    };
    let [w, h] = natural_size(&m, &t, &u("ab\ncd"));
    assert_eq!(h, 2.0 * 82.0);
    assert_eq!(w, m.string_width(&u("ab\ncd")));
}

#[test]
fn a_baseline_spaces_the_lines() {
    // −baseline 4: each line 86 apart, the last one's 4 not counted.
    let mut f = font(0.0, 0.0);
    f.base_line = -4.0;
    let m = Metrics::new(&f);
    assert_eq!(m.nb(), 4.0);
    assert_eq!(m.lines_height(2), 2.0 * 86.0 - 4.0);
    assert_eq!(m.lines_that_fit(168.0), 2);
    assert_eq!(m.lines_that_fit(167.0), 1);
    let g = glyphs(&m, f32::MAX, Justify::Left, &u("a\nb"));
    assert_eq!(g[2].char, 'b' as u16);
    assert_eq!(g[2].rect[1], 86.0);
}
