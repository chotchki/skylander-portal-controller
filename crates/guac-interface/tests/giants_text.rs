//! AT.6's Verify on the real Giants dump: AC.13's 13 text runs (43 glyphs,
//! the same in all three captures, AT.3) laid out again from their strings
//! in `global.bld`'s own font and templates, each sprite set as the HUD's
//! build macros set it (`UI - Create Player Health UI `, `Game - Create
//! Player XP UI`, `Game - Update Player Level Number`, `Money UI - Create
//! Player Money UI`, `Money UI - Update Money Number Size`), its glyphs
//! drawn at 1.5 frame pixels a unit and set where the run starts (each
//! run's place on screen is the layout's, AT.24's to check: one translation
//! a run, fitted), every glyph's corners then within 0.5 px of the frame's.
//! The frame's rectangles are AT.3's measurements off the captures
//! (`at-interface/` research, `out/at3.json`), numbers only.
//!
//! Ignored by default: it reads `GUAC_GIANTS_DUMP` from the environment or
//! the workspace's `.env.dev`:
//!
//! `cargo test -p guac-interface --release --test giants_text -- --ignored --nocapture`

use std::path::Path;

use guac_interface::model::{Str, string_flags};
use guac_interface::{Answer, MEMBERS, Model, NoWorld, SpriteId};
use guac_ir::interface::Interface;
use guac_ir::{DumpIdentity, Sha256};
use guac_script::{Obj, Value};

fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        return Some(value);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

fn load_average() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// AC.13's 13 runs, in draw order: each glyph's left, top, right and bottom
/// in the 1280x720 frame (AT.3), to the four places AT.3 printed.
#[allow(clippy::excessive_precision)]
const RUNS: [(&str, &[[f32; 4]]); 13] = [
    (
        "Eruptor",
        &[
            [199.7501, 551.9995, 214.4940, 590.9995],
            [214.4940, 551.9995, 226.3843, 590.9995],
            [226.3843, 551.9995, 242.0794, 590.9995],
            [242.0794, 551.9995, 258.2501, 590.9995],
            [258.2501, 551.9995, 270.6160, 590.9995],
            [270.6160, 551.9995, 288.2135, 590.9995],
            [288.2135, 551.9995, 300.1038, 590.9995],
        ],
    ),
    (
        "Eruptor",
        &[
            [196.7501, 548.9995, 211.4940, 587.9995],
            [211.4940, 548.9995, 223.3843, 587.9995],
            [223.3843, 548.9995, 239.0794, 587.9995],
            [239.0794, 548.9995, 255.2501, 587.9995],
            [255.2501, 548.9995, 267.6160, 587.9995],
            [267.6159, 548.9995, 285.2135, 587.9995],
            [285.2135, 548.9995, 297.1037, 587.9995],
        ],
    ),
    ("1", &[[225.6251, 656.9995, 238.9376, 698.9994]]),
    (
        "Level",
        &[
            [160.0001, 661.4994, 172.4664, 694.4994],
            [172.4664, 661.4994, 186.5412, 694.4994],
            [186.5412, 661.4994, 201.4203, 694.4994],
            [201.4203, 661.4994, 215.4952, 694.4994],
            [215.4952, 661.4994, 221.1251, 694.4994],
        ],
    ),
    ("0", &[[384.0626, 658.4995, 403.0001, 697.4995]]),
    ("0", &[[382.5626, 656.9995, 401.5001, 695.9994]]),
    (
        "290",
        &[
            [175.0001, 598.4995, 189.8904, 631.4995],
            [189.8904, 598.4995, 205.9879, 631.4995],
            [205.9879, 598.4995, 222.0855, 631.4995],
        ],
    ),
    (
        "290",
        &[
            [178.0001, 595.4995, 192.8904, 628.4995],
            [192.8904, 595.4995, 208.9879, 628.4995],
            [208.9879, 595.4995, 225.0855, 628.4995],
        ],
    ),
    (
        "290",
        &[
            [178.0001, 598.4995, 192.8904, 631.4995],
            [192.8904, 598.4995, 208.9879, 631.4995],
            [208.9879, 598.4995, 225.0855, 631.4995],
        ],
    ),
    (
        "290",
        &[
            [175.0001, 595.4995, 189.8904, 628.4995],
            [189.8904, 595.4995, 205.9879, 628.4995],
            [205.9879, 595.4995, 222.0855, 628.4995],
        ],
    ),
    ("1", &[[222.6251, 653.9995, 235.9376, 695.9994]]),
    (
        "Level",
        &[
            [157.0001, 658.4995, 169.4664, 691.4995],
            [169.4664, 658.4995, 183.5412, 691.4995],
            [183.5412, 658.4995, 198.4204, 691.4995],
            [198.4203, 658.4995, 212.4952, 691.4995],
            [212.4952, 658.4995, 218.1251, 691.4995],
        ],
    ),
    (
        "290",
        &[
            [176.5001, 596.9995, 191.3904, 629.9995],
            [191.3904, 596.9995, 207.4879, 629.9995],
            [207.4879, 596.9995, 223.5855, 629.9995],
        ],
    ),
];

/// One unit is 1.5 frame pixels (AT.3).
const PX: f32 = 1.5;

fn member(namespace: &str, name: &str) -> &'static guac_interface::Member {
    guac_interface::find(namespace, name).unwrap_or_else(|| panic!("{namespace}.{name}"))
}

fn si(name: &str) -> &'static guac_interface::Member {
    member("spriteinfo", name)
}

fn global_bld() -> Interface {
    let dump = env_or_dotenv("GUAC_GIANTS_DUMP").expect("GUAC_GIANTS_DUMP");
    let path = Path::new(&dump).join("PS3_GAME/USRDIR/permanent/global.bld");
    let bld = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let identity = DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([0; 32]),
    };
    guac_extract::interface::read("permanent/global.bld", &bld, None, identity)
        .expect("global.bld's interface")
        .ir
}

/// The HUD's text sprites, as its build macros leave each run's.
struct Hud {
    model: Model,
    text: SpriteId,
    text_box: SpriteId,
    number: Value,
    next: u32,
}

impl Hud {
    fn new(ir: &Interface) -> Self {
        let mut model = Model::default();
        let font = model.add_font(ir.fonts[0].clone());
        let template = |name: &str| {
            ir.templates
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("{name}"))
                .clone()
        };
        let text = model.add_template(None, &template("Generic_Text"), Value::Null);
        let text_box = model.add_template(None, &template("Generic_TextBox"), Value::Null);
        for t in [text, text_box] {
            model.set_font(t, Some(font));
        }
        // `Generic Number`: the one `@` it fills.
        let number = model.add_file_string(
            Obj { unit: 0, index: 1 },
            "@",
            Str {
                flags: string_flags::HAS_AT,
                ..Str::default()
            },
        );
        Hud {
            model,
            text,
            text_box,
            number,
            next: 2,
        }
    }

    fn string(&mut self, text: &str) -> Value {
        self.next += 1;
        self.model.add_file_string(
            Obj {
                unit: 0,
                index: self.next,
            },
            text,
            Str::default(),
        )
    }

    fn set(&mut self, id: SpriteId, name: &str, v: Value) {
        self.model.set(id, si(name), v).unwrap();
    }

    /// `natural size`'s x or y as the script reads it: a 12.4 half.
    fn natural(&self, id: SpriteId, y: bool) -> Value {
        let Ok(Answer::Value(Value::Screen(n))) = self.model.get(id, si("natural size"), &NoWorld)
        else {
            panic!("natural size")
        };
        let half = if y { n as u16 } else { (n >> 16) as u16 };
        Value::Float(f32::from(half as i16) / 16.0)
    }

    /// The player's name box: `Generic_TextBox`, 150 by 26, its point size
    /// its height, the name a run-time string.
    fn name(&mut self, name: &str) -> SpriteId {
        let a = self.model.spawn(self.text_box);
        let s = self.string(name);
        self.set(a, "justify", Value::Int(0));
        self.set(a, "width", Value::Float(150.0));
        self.set(a, "height", Value::Float(26.0));
        self.set(a, "point size", Value::Int(26));
        self.set(a, "text", s);
        a
    }

    /// The health number: `Generic_TextBox` at 22 points, 75 by 25.
    fn health(&mut self, value: i32) -> SpriteId {
        let a = self.model.spawn(self.text_box);
        self.set(a, "point size", Value::Int(22));
        self.set(a, "width", Value::Float(75.0));
        self.set(a, "height", Value::Float(25.0));
        self.set(a, "justify", Value::Int(0));
        self.set(a, "text", self.number);
        self.set(a, "numeric value", Value::Int(value));
        a
    }

    /// `Level`: `Generic_Text` at 22 points, sized to its natural size.
    fn level_label(&mut self) -> SpriteId {
        let a = self.model.spawn(self.text);
        let s = self.string("Level");
        self.set(a, "justify", Value::Int(0));
        self.set(a, "text", s);
        self.set(a, "point size", Value::Int(22));
        let (w, h) = (self.natural(a, false), self.natural(a, true));
        self.set(a, "width", w);
        self.set(a, "height", h);
        a
    }

    /// The level number: `Generic_Text` at 28 points, 99 measured first,
    /// then the level, sized to that's natural size.
    fn level_number(&mut self, level: i32) -> SpriteId {
        let a = self.model.spawn(self.text);
        self.set(a, "text", self.number);
        self.set(a, "justify", Value::Int(0));
        self.set(a, "point size", Value::Int(28));
        self.set(a, "numeric value", Value::Int(99));
        let (w, h) = (self.natural(a, false), self.natural(a, true));
        self.set(a, "width", w);
        self.set(a, "height", h);
        self.set(a, "numeric value", Value::Int(level));
        let (w, h) = (self.natural(a, false), self.natural(a, true));
        self.set(a, "width", w);
        self.set(a, "height", h);
        a
    }

    /// The gold: `Generic_Text` at 26 points, its height its point size,
    /// its width the natural size of the money it shows.
    fn gold(&mut self, money: i32) -> SpriteId {
        let a = self.model.spawn(self.text);
        self.set(a, "text", self.number);
        self.set(a, "justify", Value::Int(0));
        self.set(a, "point size", Value::Int(26));
        self.set(a, "numeric value", Value::Int(9999));
        self.set(a, "height", Value::Float(26.0));
        self.set(a, "numeric value", Value::Int(money));
        let w = self.natural(a, false);
        self.set(a, "width", w);
        a
    }

    /// The sprite's glyph quads in frame pixels from its top-left.
    fn quads(&self, id: SpriteId) -> Vec<(u16, [f32; 4])> {
        let (glyphs, out) = self.model.glyph_layout(id, &NoWorld).expect("glyphs");
        let [sx, sy] = out.scale;
        glyphs
            .iter()
            .map(|g| {
                let r = g.rect;
                (
                    g.char,
                    [
                        r[0] * sx * PX,
                        r[1] * sy * PX,
                        r[2] * sx * PX,
                        r[3] * sy * PX,
                    ],
                )
            })
            .collect()
    }
}

#[test]
#[ignore = "reads the Giants dump (GUAC_GIANTS_DUMP)"]
fn ac13s_43_glyphs_lay_out_again_from_their_strings() {
    println!("load at the start: {}", load_average());
    let started = std::time::Instant::now();
    let ir = global_bld();
    assert_eq!(MEMBERS.len(), 64);
    let font = &ir.fonts[0];
    println!(
        "font {:?}: {} glyphs, height {}, padding {}, kern {}",
        font.source,
        font.glyphs.len(),
        font.height,
        font.char_padding,
        font.kern_amount
    );
    // The disc's wrap flag never stands without the fit flag (the crate
    // docs' Hypothesis on that path).
    let wraps_alone = ir
        .templates
        .iter()
        .filter(|t| t.flags & 0x80 != 0 && t.flags & 0x100 == 0)
        .count();
    println!("global.bld templates wrapping without fitting: {wraps_alone}");
    let mut hud = Hud::new(&ir);
    let mut worst = (0.0f32, String::new());
    let mut glyphs = 0;
    let mut within = 0;
    for (i, (label, frame)) in RUNS.iter().enumerate() {
        let sprite = match *label {
            "Eruptor" => hud.name("Eruptor"),
            "1" => hud.level_number(1),
            "Level" => hud.level_label(),
            "0" => hud.gold(0),
            "290" => hud.health(290),
            other => panic!("{other}"),
        };
        let quads = hud.quads(sprite);
        let drawn: String = quads
            .iter()
            .map(|(c, _)| char::from_u32(u32::from(*c)).unwrap())
            .collect();
        assert_eq!(&drawn, label, "run {i}'s characters");
        // The run's place: the mean offset of every corner.
        let n = (frame.len() * 2) as f32;
        let dx = frame
            .iter()
            .zip(&quads)
            .map(|(f, (_, q))| (f[0] - q[0]) + (f[2] - q[2]))
            .sum::<f32>()
            / n;
        let dy = frame
            .iter()
            .zip(&quads)
            .map(|(f, (_, q))| (f[1] - q[1]) + (f[3] - q[3]))
            .sum::<f32>()
            / n;
        for (j, (f, (c, q))) in frame.iter().zip(&quads).enumerate() {
            let off = [
                (f[0] - q[0] - dx).abs(),
                (f[1] - q[1] - dy).abs(),
                (f[2] - q[2] - dx).abs(),
                (f[3] - q[3] - dy).abs(),
            ]
            .into_iter()
            .fold(0.0f32, f32::max);
            glyphs += 1;
            if off <= 0.5 {
                within += 1;
            }
            if off > worst.0 {
                worst = (
                    off,
                    format!("run {i} ({label}) glyph {j} ({})", *c as u8 as char),
                );
            }
            println!(
                "run {i:2} {label:8} glyph {j} {}: frame {:?} model {:?} off {off:.4} px",
                *c as u8 as char,
                f,
                [q[0] + dx, q[1] + dy, q[2] + dx, q[3] + dy]
            );
        }
        let out = hud.model.glyph_layout(sprite, &NoWorld).unwrap().1;
        println!(
            "run {i:2} {label:8} scale {:?} of point size's {:.6}: squeeze {:.4}; size {:?}; placed at ({dx:.3}, {dy:.3})",
            out.scale,
            out.scale[1],
            out.scale[0] / out.scale[1],
            out.size
        );
    }
    println!(
        "{within} of {glyphs} glyphs within 0.5 px; worst {:.4} px at {}",
        worst.0, worst.1
    );
    println!(
        "in {:.2} s; load at the end: {}",
        started.elapsed().as_secs_f32(),
        load_average()
    );
    assert_eq!(glyphs, 43);
    assert_eq!(within, 43, "worst {:.4} px at {}", worst.0, worst.1);
    assert_eq!(wraps_alone, 0);
}
