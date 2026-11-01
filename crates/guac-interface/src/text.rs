//! The text measure, the number fill and the glyph layout (the crate docs,
//! Text and the number fill; openspec AT.6). Single precision throughout,
//! each operation rounded as the original's are; `mul_add` where it fuses.

use guac_ir::interface::{Font, Glyph};

use crate::model::{Str, string_flags};
use crate::sprite::flags;

/// The four line ends and blanks the measure treats apart.
const LF: u16 = 0x0A;
const CR: u16 = 0x0D;
const TAB: u16 = 0x09;
const SPACE: u16 = 0x20;

/// A width within this of 0 doesn't wrap (0x3D4EE4's neighbour, 5e-7).
const NO_WRAP: f32 = 5e-7;

fn is_line_end(c: u16) -> bool {
    c == LF || c == CR
}

fn is_blank(c: u16) -> bool {
    c == SPACE || c == TAB
}

/// A font as the measure reads it.
#[derive(Debug, Clone, Copy)]
pub struct Metrics<'a> {
    pub font: &'a Font,
    /// The default font takes 4 for −baseline; nothing sets one today.
    pub default: bool,
}

impl<'a> Metrics<'a> {
    pub fn new(font: &'a Font) -> Self {
        Metrics {
            font,
            default: false,
        }
    }

    /// −baseline, the crate docs' *nb*.
    pub fn nb(&self) -> f32 {
        if self.default {
            4.0
        } else {
            -self.font.base_line
        }
    }

    /// The glyph for `c`: a binary search over the list as the original's
    /// (0x1F5238), so a list out of order misses what the search skips.
    pub fn glyph(&self, c: u16) -> Option<&'a Glyph> {
        let g = &self.font.glyphs;
        let at = g.partition_point(|x| x.char < c);
        g.get(at).filter(|x| x.char == c)
    }

    /// The kern before `g` after `prev` (0x3D7790).
    fn kern(&self, prev: Option<&Glyph>, g: &Glyph) -> f32 {
        match prev {
            Some(p) if self.font.kern_amount > 0.0 => {
                let top = u16::from(p.kern[1]) + u16::from(g.kern[0]);
                let bottom = u16::from(p.kern[3]) + u16::from(g.kern[2]);
                f32::from(top.min(bottom)) * self.font.kern_amount
            }
            _ => 0.0,
        }
    }

    /// A run's width (0x3D7790).
    pub fn run_width(&self, units: &[u16]) -> f32 {
        let mut w = 0.0f32;
        let mut prev = None;
        for &c in units {
            match self.glyph(c) {
                None => prev = None,
                Some(g) => {
                    let k = self.kern(prev, g);
                    w = w + g.step + self.font.char_padding - k;
                    prev = Some(g);
                }
            }
        }
        w
    }

    /// A string's width: its widest line (0x1F4BD0).
    pub fn string_width(&self, units: &[u16]) -> f32 {
        let mut widest = 0.0f32;
        let mut at = 0;
        while at < units.len() {
            let end = units[at..]
                .iter()
                .position(|&c| is_line_end(c))
                .map_or(units.len(), |n| at + n);
            let w = self.run_width(&units[at..end]);
            if widest <= w {
                widest = w;
            }
            // The next line starts one character on, whichever end it was.
            at = if end < units.len() { end + 1 } else { end };
        }
        widest
    }

    /// The height of `lines` lines: lines × (height + nb) − nb, fused.
    pub fn lines_height(&self, lines: i32) -> f32 {
        (lines as f32).mul_add(self.font.height + self.nb(), -self.nb())
    }

    /// How many lines fit `height` (0xB6E80).
    pub fn lines_that_fit(&self, height: f32) -> i32 {
        let pitch = self.font.height + self.nb();
        let k = ((height + self.nb()) / pitch) as i32;
        let over = ((k + 1) as f32).mul_add(pitch, -self.nb()) - height;
        if 0.001 < over { k } else { k + 1 }
    }

    /// A string wrapped at `width` (0x3D4EE8): the text written out and its
    /// line count.
    pub fn wrap(&self, width: f32, units: &[u16]) -> (Vec<u16>, i32) {
        let mut out = Vec::with_capacity(units.len() + 8);
        let mut count = 0;
        for line in self.lines(width, units) {
            if count > 0 && !units.get(line.start).is_some_and(|&c| is_line_end(c)) {
                out.push(LF);
            }
            out.extend_from_slice(&units[line.start..line.end]);
            count += 1;
        }
        (out, count)
    }

    /// The wrap's lines (0x3D5698, the drawing's; 0x3D4EE8 cuts the same):
    /// each its width (its words and the blanks after each, measured
    /// alone), its start and where it stops in `units`.
    pub fn lines(&self, width: f32, units: &[u16]) -> Vec<Line> {
        let mut lines = Vec::new();
        let no_wrap = width.abs() <= NO_WRAP;
        let mut s = 0;
        while s < units.len() && units[s] != 0 {
            let mut p = s;
            let mut words = 0;
            let mut lw = 0.0f32;
            while p < units.len() && !is_line_end(units[p]) && units[p] != 0 {
                let mut q = p;
                while q < units.len()
                    && units[q] != 0
                    && !is_blank(units[q])
                    && !is_line_end(units[q])
                {
                    q += 1;
                }
                let ww = self.run_width(&units[p..q]);
                if no_wrap || lw + ww <= width || words < 1 {
                    let mut r = q;
                    while r < units.len() && is_blank(units[r]) {
                        r += 1;
                    }
                    words += 1;
                    let sw = self.run_width(&units[q..r]);
                    lw += sw + ww;
                    p = r;
                } else {
                    break;
                }
            }
            lines.push(Line {
                width: lw,
                start: s,
                end: p,
            });
            s = if units.get(p).is_some_and(|&c| is_line_end(c)) {
                p + 1
            } else {
                p
            };
        }
        lines
    }
}

/// A wrapped line: its width (words and blanks, each measured alone), where
/// it starts in the text and where it stops.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub width: f32,
    pub start: usize,
    pub end: usize,
}

/// A string's lines (0x1F4CF0): one more than its line ends, less those at
/// its end.
pub fn line_count(units: &[u16]) -> i32 {
    let ends = units.iter().filter(|&&c| is_line_end(c)).count() as i32;
    let trailing = units.iter().rev().take_while(|&&c| is_line_end(c)).count() as i32;
    1 + ends - trailing
}

/// The rounding added at each count of decimal digits, and 10 to it: the
/// words at 0x723A30 on (index 0 is the `.` and `,` pair), the rounding the
/// same table 9 words on; past what was read, 0.
const TABLE: [f32; 21] = [
    0.0,
    10.0,
    100.0,
    1e3,
    1e4,
    1e5,
    1e6,
    1e7,
    1e8,
    1e9,
    0.049_995,
    0.004_999_5,
    4.9995e-4,
    4.9995e-5,
    4.9995e-6,
    4.9995e-7,
    4.9995e-8,
    4.9995e-9,
    4.9995e-10,
    0.0,
    0.0,
];

fn table(i: i64) -> f32 {
    usize::try_from(i)
        .ok()
        .and_then(|i| TABLE.get(i))
        .copied()
        .unwrap_or(0.0)
}

/// `n` written as `digits` digits from the lowest, or as many as it takes
/// when `digits` is 0 (at least one), into `out`, lowest first.
fn digits_low_first(mut n: i32, digits: i32, commas: bool, out: &mut Vec<u16>) {
    let digits = digits.min(199);
    let mut left = if digits == 0 { 1 } else { digits };
    let mut first = n == 0;
    let mut run = 0;
    while left > 0 && (first || digits != 0 || n > 0) {
        let next = n / 10;
        out.push((n - next * 10) as u16 + u16::from(b'0'));
        run += 1;
        if commas && run == 3 && (next > 0 || left > 1) {
            out.push(u16::from(b','));
            run = 0;
        }
        if digits != 0 {
            left -= 1;
        }
        first = false;
        n = next;
    }
}

/// The time fill (0x9C674), lowest field first.
fn time_low_first(value: i32, digits: i32, out: &mut Vec<u16>) {
    let mut left = match digits {
        0 => 6,
        d if d < 1 => return,
        d => d,
    };
    let mut v = value.wrapping_abs();
    loop {
        let mut width = 2;
        let mut part = v % 60;
        if left == 1 {
            width = 1;
            if part > 10 {
                part %= 10;
            }
        }
        let negative = part < 0;
        let mut part = part.wrapping_abs();
        for _ in 0..width {
            out.push((part % 10) as u16 + u16::from(b'0'));
            part /= 10;
        }
        if negative {
            out.push(u16::from(b'-'));
        }
        left -= width;
        if left < 1 {
            return;
        }
        out.push(u16::from(b':'));
        v /= 60;
    }
}

/// The number fill (0x9C858): `s`'s text with its first `@` replaced by
/// `value` as the string's settings write it. `value` is the sprite's
/// `numeric value`, which reads as an integer unless the string has decimal
/// digits.
pub fn fill(s: &Str, value: f32) -> Vec<u16> {
    let mut buf: Vec<u16> = Vec::new();
    let mut negative = false;
    let mut int = value as i32;
    if s.decimal_digits > 0 || s.flags & 0x1000 != 0 {
        negative = value < 0.0;
        let v = value.abs() + table(i64::from(s.decimal_digits) + 9);
        let mut frac = v;
        if frac >= 1.0 {
            frac = (f64::from(frac) - (frac as i64) as f64) as f32;
        }
        let n = (f64::from(table(i64::from(s.decimal_digits))) * f64::from(frac)) as i32;
        let n = n.wrapping_abs();
        let digits = s.decimal_digits.min(199);
        let mut n = n;
        for _ in 0..digits {
            buf.push((n % 10) as u16 + u16::from(b'0'));
            n /= 10;
        }
        buf.push(u16::from(b'.'));
        int = v as i32;
    }
    if s.flags & string_flags::TIME != 0 {
        time_low_first(int, s.integer_digits, &mut buf);
    } else {
        let minus = int < 0;
        let n = int.wrapping_abs();
        digits_low_first(
            n,
            s.integer_digits,
            s.flags & string_flags::COMMAS != 0,
            &mut buf,
        );
        if minus {
            buf.push(u16::from(b'-'));
        }
    }
    if negative {
        buf.push(u16::from(b'-'));
    }
    let fills = s.flags & (string_flags::HAS_AT | string_flags::TIME) != 0;
    let mut out = Vec::with_capacity(s.units.len() + buf.len());
    for &c in &s.units {
        if c == u16::from(b'@') && fills {
            out.extend(buf.drain(..).rev());
        } else {
            out.push(c);
        }
    }
    out
}

/// What the text layout reads of its sprite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextIn {
    /// The flags word: *sized*, *wraps*, *fits*, *point size set*.
    pub flags: u32,
    pub point_size: u32,
    /// The written (stored) width and height.
    pub width: f32,
    pub height: f32,
}

impl TextIn {
    /// The scale *s* against `m`'s height.
    pub fn scale(&self, m: &Metrics<'_>) -> f32 {
        if self.flags & flags::POINT_SIZE_SET != 0 {
            self.point_size as f32 / m.font.height
        } else {
            1.0
        }
    }
}

/// The text layout's answer (0xB6F70): its computed size, the glyphs' x
/// and y scales and the text as it draws (wrapped where it wraps).
#[derive(Debug, Clone, PartialEq)]
pub struct TextOut {
    pub size: [f32; 2],
    pub scale: [f32; 2],
    pub units: Vec<u16>,
}

/// The text's size, what `width` and `height` read on a sprite not *sized*
/// (0xB7F08).
pub fn size(m: &Metrics<'_>, t: &TextIn, units: &[u16]) -> [f32; 2] {
    let s = t.scale(m);
    [
        m.string_width(units) * s,
        (m.font.height + m.nb() - m.nb()) * s,
    ]
}

/// The text's layout (0xB6F70).
pub fn layout(m: &Metrics<'_>, t: &TextIn, units: &[u16]) -> TextOut {
    let s = t.scale(m);
    let lines = line_count(units);
    if t.flags & flags::SIZED == 0 {
        return TextOut {
            size: [m.string_width(units) * s, m.lines_height(lines) * s],
            scale: [s, s],
            units: units.to_vec(),
        };
    }
    let (w, h) = (t.width, t.height);
    let wraps = t.flags & flags::WRAPS != 0;
    let fits = t.flags & flags::FITS != 0;
    if !wraps {
        let text_h = m.lines_height(lines);
        let str_w = m.string_width(units);
        if !fits {
            return TextOut {
                size: [w, h],
                scale: [w / str_w, h / text_h],
                units: units.to_vec(),
            };
        }
        let tall = text_h * s;
        let (height, sy) = if h < tall {
            (h, s * (h / tall))
        } else {
            (tall, s)
        };
        let wide = str_w * s;
        let mut sx = if w < wide { s * (w / wide) } else { s };
        if sx > sy {
            sx = sy;
        }
        return TextOut {
            size: [w, height],
            scale: [sx, sy],
            units: units.to_vec(),
        };
    }
    let (ws, hs) = (w / s, h / s);
    if !(fits && ws > 0.0 && hs > 0.0) {
        // The height counts a buffer this path never fills: taken as the
        // string's own lines (the crate docs' Hypothesis, AT.9).
        return TextOut {
            size: [w, m.lines_height(lines) * s],
            scale: [s, s],
            units: units.to_vec(),
        };
    }
    let pitch = m.font.height + m.nb();
    let mut ratio_w = 1.0f32;
    let mut ratio_h = 1.0f32;
    let (mut buf, n) = m.wrap(ws, units);
    let mut tall = n as f32 * pitch;
    let mut widest = ws;
    let mut k = m.lines_that_fit(hs);
    if k < n {
        k = k.max(lines);
        let mut tenths = k * 10;
        let mut last;
        loop {
            last = k;
            let mut hr = hs / (last as f32).mul_add(pitch, -m.nb());
            if hr > 1.0 {
                hr = 1.0;
            }
            let step = (m.string_width(units) / tenths as f32) * 10.0;
            let mut tw = step;
            let mut tries = 0;
            loop {
                let (b, c) = m.wrap(tw, units);
                buf = b;
                if c <= last {
                    break;
                }
                tries += 1;
                tw = step.mul_add(0.1, tw);
                if tries > 11 {
                    break;
                }
            }
            let prev = widest;
            widest = m.string_width(&buf);
            tenths += 10;
            k = last + 1;
            ratio_w = ws / widest;
            if widest == prev || ratio_w / hr >= 0.5 {
                break;
            }
        }
        tall = last as f32 * pitch;
    }
    let w2 = m.string_width(&buf);
    if widest < w2 {
        ratio_w = widest / w2;
        let (b, c) = m.wrap(w2, units);
        buf = b;
        tall = c as f32 * pitch;
    }
    let tall = tall - m.nb();
    if hs < tall {
        ratio_h = hs / tall;
    }
    let mut sx = ratio_w * s;
    let sy = s * ratio_h;
    if sx > sy {
        sx = sy;
    }
    let lines = line_count(&buf);
    TextOut {
        size: [w, m.lines_height(lines) * sy],
        scale: [sx, sy],
        units: buf,
    }
}

/// `natural size` for text (0xB81E8).
pub fn natural_size(m: &Metrics<'_>, t: &TextIn, units: &[u16]) -> [f32; 2] {
    let s = t.scale(m);
    if t.flags & flags::WRAPS == 0 {
        return [
            m.string_width(units) * s,
            m.lines_height(line_count(units)) * s,
        ];
    }
    let (buf, n) = m.wrap(t.width / s, units);
    if n == 0 {
        return [0.0, 0.0];
    }
    let kept = m.lines_that_fit(t.height / s).min(n);
    if kept == 0 {
        return [0.0, 0.0];
    }
    // The start of the last kept line in the wrapped text.
    let mut at = 0;
    for _ in 0..kept - 1 {
        while at < buf.len() && buf[at] != LF && buf[at] != 0 {
            at += 1;
        }
        at += 1;
    }
    let rest = units.get(at.min(units.len())..).unwrap_or(&[]);
    let last = m.string_width(rest) * s;
    let height = m.lines_height(kept - 1 + line_count(rest)) * s;
    let mut width = last;
    if at > 0 {
        let earlier = m.string_width(&buf[..(at - 1).min(buf.len())]) * s;
        if last < earlier && earlier > t.width {
            width = earlier;
        }
    }
    [width, height]
}

/// One glyph as the text draws it, in font pixels from the text's top-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphQuad {
    /// Left, top, right, bottom.
    pub rect: [f32; 4],
    /// The atlas box: u and v at its minimum, then at its maximum.
    pub uv: [f32; 4],
    /// The character it draws.
    pub char: u16,
}

/// The justification the font's layout takes: the sprite's 1 (center) is
/// the font's 2, its 2 (right) the font's 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Justify {
    Left,
    Center,
    Right,
}

impl Justify {
    /// From the sprite's `justify` (0 left, 1 center, 2 right).
    pub fn from_sprite(j: u32) -> Self {
        match j {
            1 => Justify::Center,
            2 => Justify::Right,
            _ => Justify::Left,
        }
    }
}

/// The width a text's glyphs break at and the justification they take
/// (0xB79A8): a *sized* text's computed width over its x scale plus 0.4999,
/// truncated, with its justification; one not *sized* never breaks and sets
/// left.
pub fn break_width(sized: bool, computed_width: f32, scale_x: f32, justify: u32) -> (f32, Justify) {
    if sized {
        (
            ((computed_width / scale_x + 0.4999) as i64) as f32,
            Justify::from_sprite(justify),
        )
    } else {
        (f32::MAX, Justify::Left)
    }
}

/// The glyphs of `units` (0x3D6EB0): broken at `width`, justified, from the
/// text's top-left in font pixels.
pub fn glyphs(m: &Metrics<'_>, width: f32, justify: Justify, units: &[u16]) -> Vec<GlyphQuad> {
    let mut out = Vec::new();
    let pad = m.font.char_padding;
    let mut y = 0.0f32;
    for line in m.lines(width, units) {
        let mut end = line.end;
        if units.get(end) == Some(&LF) {
            end += 1;
        }
        let mut x = match justify {
            Justify::Left => 0.0,
            Justify::Right => 0.0 + (width - line.width),
            Justify::Center => {
                let lead = units[line.start..]
                    .iter()
                    .take_while(|&&c| is_blank(c))
                    .count();
                let lead = m.run_width(&units[line.start..line.start + lead]);
                0.0 + ((width - line.width) - lead).mul_add(0.5, 0.5).floor()
            }
        };
        let last_is_space = units.get(end) == Some(&SPACE);
        let mut prev: Option<&Glyph> = None;
        for &c in &units[line.start..end.min(units.len())] {
            let c = if c == LF { SPACE } else { c };
            match m.glyph(c) {
                None => {
                    prev = None;
                    if last_is_space {
                        x += pad;
                    }
                }
                Some(g) => {
                    x -= m.kern(prev, g);
                    let (x0, y0) = (x + g.offset[0], y + g.offset[1]);
                    out.push(GlyphQuad {
                        rect: [x0, y0, x0 + g.size[0], y0 + g.size[1]],
                        uv: [g.uv_min[0], g.uv_min[1], g.uv_max[0], g.uv_max[1]],
                        char: c,
                    });
                    x += g.step + pad;
                    prev = Some(g);
                }
            }
        }
        y += m.font.height - m.font.base_line;
    }
    out
}

#[cfg(test)]
mod tests;
