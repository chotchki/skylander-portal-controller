//! The sounds a level plays, off the dump (openspec AR.9-AR.12): the
//! sounds part read in memory with guac-extract's own reader (nothing
//! written), its music's loop seam decoded as the TV decodes it. Ignored by
//! default: reads `GUAC_GIANTS_DUMP` (process env, else `.env.dev`).

mod common;

use std::sync::Arc;

use common::{dump, identity};

/// Level 213's sounds part as the extract would write it: its IR and its
/// audio by path.
fn level_213() -> guac_extract::sounds::Read {
    let usr = dump().join("PS3_GAME/USRDIR");
    let exports = guac_extract::sounds::exports(&usr).expect("global's exports");
    let bld = std::fs::read(usr.join("level/level_213_tutotial.bld")).expect("the level's .bld");
    let arc = std::fs::read(usr.join("level/level_213_tutotial.arc")).expect("the level's .arc");
    guac_extract::sounds::read(
        "level/level_213_tutotial.bld",
        &bld,
        Some(&arc),
        &exports,
        identity(),
    )
    .expect("the level's sounds")
}

/// The remuxed bytes and the data of the sound named `name`.
fn track(read: &guac_extract::sounds::Read, name: &str) -> (Arc<[u8]>, guac_ir::sound::SoundData) {
    let sound = read
        .ir
        .sounds
        .iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no {name}"));
    let guac_ir::sound::DataRef::Here(home, object) = sound.data else {
        panic!("{name}'s data isn't the level's");
    };
    let data = read.ir.data_at(home, object).expect("its data").clone();
    let file = &data.audio.as_ref().expect("its audio").file;
    let bytes = &read
        .audio
        .iter()
        .find(|(p, _)| p == file)
        .expect("its bytes")
        .1;
    (Arc::from(bytes.as_slice()), data)
}

/// Every sample rodio's decoder makes of `bytes`, interleaved, and the
/// channel count.
fn decode(bytes: &Arc<[u8]>, gapless: bool) -> (Vec<f32>, u16) {
    use rodio::Source;
    let d = rodio::Decoder::builder()
        .with_byte_len(bytes.len() as u64)
        .with_data(std::io::Cursor::new(bytes.clone()))
        .with_gapless(gapless)
        .build()
        .expect("it decodes");
    let channels = d.channels().get();
    (d.collect(), channels)
}

/// The step across each looping track's seam against its music's own 99th
/// percentile step (openspec AR.11): the TV's loop (the frames decoded on
/// through the wrap, `guacamole::voice::Cycle`) under one, its second pass
/// the first's again once the decoder has settled, a fresh decode's codec
/// silence music on the second pass; a loop of the decoded samples (what a
/// plain decoder's repeat makes) printed beside it.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn a_looping_bed_loops_its_frames_on_through_the_seam() {
    use guacamole::voice::{Gains, VoiceDecoder};
    let read = level_213();
    let looping: Vec<&str> = read
        .ir
        .sounds
        .iter()
        .filter(|s| s.flags & 1 != 0 && s.properties >> 1 & 0x1F == guac_sound::MUSIC)
        .filter_map(|s| s.name.as_deref())
        .collect();
    assert_eq!(
        looping,
        [
            "mu_213_combat_shorter",
            "mu_213_explore",
            "mu_213_inside",
            "mu_213_inside_combat",
            "mu_213_intro"
        ],
        "chapter 1's looping tracks"
    );
    for name in looping {
        let (bytes, data) = track(&read, name);
        let n = data.audio.as_ref().unwrap().samples as usize;
        let (once, ch) = decode(&bytes, false);
        let ch = ch as usize;
        assert_eq!(once.len(), n * ch, "{name}: every frame decoded");
        // The TV's loop, two passes and a bit, left channel.
        let tv: Vec<f32> = VoiceDecoder::new(bytes.clone(), Gains::new(1.0, 1.0), true)
            .step_by(2)
            .take(2 * n + 8192)
            .collect();
        assert_eq!(tv.len(), 2 * n + 8192, "{name}: the loop plays on");
        let left: Vec<f32> = once.iter().step_by(ch).copied().collect();
        let mut steps: Vec<f32> = left.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        steps.sort_by(f32::total_cmp);
        let p99 = steps[steps.len() * 99 / 100];
        let join = (n - 2..n + 3)
            .map(|i| (tv[i] - tv[i - 1]).abs())
            .fold(0.0_f32, f32::max);
        let plain = (left[n - 1] - left[0]).abs();
        let silent = left.iter().take_while(|v| v.abs() <= 1e-4).count();
        // The second pass the first's from frame 4096 to its end, and the
        // third's start the first's again.
        let resettled =
            (4096..n).all(|i| tv[n + i] == tv[i]) && (4096..8192).all(|i| tv[2 * n + i] == tv[i]);
        println!(
            "{name}: {n} frames; the TV's loop steps {:.2} x p99 ({p99:.4}) across the seam; a loop of the samples steps {:.2} x and then holds {silent} frames under 1e-4 ({:.1} ms); second pass the first's from frame 4096 to its end and the third's start too: {resettled}",
            join / p99,
            plain / p99,
            silent as f32 * 1e3 / data.audio.as_ref().unwrap().rate as f32
        );
        assert!(join < p99, "{name}: the seam steps {join} against {p99}");
        assert!(resettled, "{name}: the second pass isn't the first's again");
        // A fresh decode's codec silence (under 1,000 frames: the intro's
        // 1.27 s is a rest in its music) is music on the second pass.
        assert!(
            silent > 1000 || tv[n..n + silent].iter().any(|v| v.abs() > 1e-4),
            "{name}: the second pass starts silent"
        );
    }
}

fn operand_path(program: &guac_script::Program, o: Option<guac_script::OperandId>) -> String {
    use guac_script::{Elem, Ins, Operand};
    let Some(o) = o else { return String::new() };
    match &program.operands[o as usize] {
        Operand::Path {
            elems,
            literal,
            other,
            ..
        } => {
            let names: Vec<String> = elems
                .iter()
                .map(|e| match e {
                    Elem::Op(op) => match &program.ops[*op as usize] {
                        Ins::Parameter(p) => program.sym(p.name).to_owned(),
                        Ins::Variable { name, .. } => format!("${}", program.sym(*name)),
                        _ => format!("{:?}", program.op_info[*op as usize].class),
                    },
                    Elem::Named(a, b) => format!("{}.{}", program.sym(*a), program.sym(*b)),
                    other => format!("{other:?}"),
                })
                .collect();
            let other = other.map_or(String::new(), |o| {
                format!(" @{}", operand_path(program, Some(o)))
            });
            format!(
                "[{}{}{other}]",
                names.join("/"),
                literal.map_or(String::new(), |l| format!(" ={l:?}"))
            )
        }
        Operand::Expr { a, op, b } => format!(
            "({} {op:?} {})",
            operand_path(program, *a),
            operand_path(program, *b)
        ),
    }
}

/// A development probe for AR.12: the ops of every definition whose name
/// holds one of `AR12_DEFS` (comma-separated, case ignored), as the linked
/// program holds them, from its definition to the next.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_definitions() {
    use guac_script::Ins;
    let (run, _) = common::run_on_with(guac_script::Mode::Play, common::Setup::default());
    let program = run.vm.program_arc();
    let wanted: Vec<String> = std::env::var("AR12_DEFS")
        .unwrap_or_default()
        .split(',')
        .map(str::to_ascii_lowercase)
        .collect();
    for def in &program.defs {
        let name = def.display.to_ascii_lowercase();
        if !wanted.iter().any(|w| !w.is_empty() && name.contains(w)) {
            continue;
        }
        let Some((script, at)) = program
            .scripts
            .iter()
            .find_map(|s| s.ops.iter().position(|&o| o == def.op).map(|i| (s, i)))
        else {
            println!("== {:?} {}: not in any script's ops", def.kind, def.display);
            continue;
        };
        println!(
            "== {:?} {} in {} at #{at}",
            def.kind, def.display, script.name
        );
        for (k, &op) in script.ops.iter().enumerate().skip(at).take(400) {
            if k > at && program.defs.iter().any(|d| d.op == op) {
                break;
            }
            let ins = &program.ops[op as usize];
            let text = match ins {
                Ins::World { operands, words } => format!(
                    "World {} {:?}",
                    operands
                        .iter()
                        .map(|(n, o)| format!("{n}={}", operand_path(&program, *o)))
                        .collect::<Vec<_>>()
                        .join(" "),
                    words
                ),
                Ins::SetValue { lhs, rhs } => format!(
                    "SetValue {} = {}",
                    operand_path(&program, *lhs),
                    operand_path(&program, *rhs)
                ),
                other => {
                    let t = format!("{other:?}");
                    t.chars().take(160).collect()
                }
            };
            println!("  #{k} {:?} {text}", program.op_info[op as usize].class);
        }
    }
}

/// A development probe for AR.12: every `start sequence` op in the linked
/// program whose operands name one of `AR12_WORDS` (comma-separated, case
/// ignored), with its script, its place and the definition it sits in.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_starts() {
    use guac_formats::script::Class;
    use guac_script::Ins;
    let (run, _) = common::run_on_with(guac_script::Mode::Play, common::Setup::default());
    let program = run.vm.program_arc();
    let words: Vec<String> = std::env::var("AR12_WORDS")
        .unwrap_or_default()
        .split(',')
        .map(str::to_ascii_lowercase)
        .collect();
    for s in &program.scripts {
        let mut def = String::new();
        for (k, &op) in s.ops.iter().enumerate() {
            if let Some(d) = program.defs.iter().find(|d| d.op == op) {
                def = d.display.clone();
            }
            if program.op_info[op as usize].class != Class::StartSequence {
                continue;
            }
            let Ins::World { operands, .. } = &program.ops[op as usize] else {
                continue;
            };
            let text = operands
                .iter()
                .map(|(n, o)| format!("{n}={}", operand_path(&program, *o)))
                .collect::<Vec<_>>()
                .join(" ");
            let low = text.to_ascii_lowercase();
            if words.iter().any(|w| !w.is_empty() && low.contains(w)) {
                println!("{} #{k} in '{def}': {text}", s.name);
            }
        }
    }
}

/// A development probe for AR.12: the placements whose script names one of
/// `AR12_WORDS`, with their file, activity and whether the TV's listing
/// runs them.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_placements() {
    let (run, _) = common::run_on_with(guac_script::Mode::Play, common::Setup::default());
    let c = guacamole::scripts::fight_config(guac_script::Mode::Play);
    let words: Vec<String> = std::env::var("AR12_WORDS")
        .unwrap_or_default()
        .split(',')
        .map(str::to_ascii_lowercase)
        .collect();
    for p in &run.files.placed {
        let Some(script) = &p.script else { continue };
        let low = script.to_ascii_lowercase();
        if words.iter().any(|w| !w.is_empty() && low.contains(w)) {
            println!(
                "{} unit {} activity {} {script}: listed {:?}",
                p.name,
                p.obj.unit,
                p.activity_state,
                c.whitelist.listing(&p.name, script)
            );
        }
    }
}

/// A development probe for AR.11: the names the TV's run knows for the
/// member ids `AR11_IDS` lists (`<namespace>:<name>` decimal, comma-
/// separated), and the live table's (`AH18_LIVE_TABLE`) for the rest.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_member_names() {
    let c = guacamole::scripts::fight_config(guac_script::Mode::Play);
    let live = std::env::var("AH18_LIVE_TABLE")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    for spec in std::env::var("AR11_IDS").unwrap_or_default().split(',') {
        let Some((a, b)) = spec.split_once(':') else {
            continue;
        };
        let id = guac_script::MemberId {
            namespace: a.parse().unwrap(),
            name: b.parse().unwrap(),
        };
        let live_name = live
            .lines()
            .filter(|l| !l.starts_with('#'))
            .find_map(|row| {
                let c: Vec<&str> = row.split('\t').collect();
                let ns = c[0].to_ascii_lowercase();
                let ns = ns.strip_prefix("tfb").unwrap_or(&ns).to_owned();
                let name = c[2].to_ascii_lowercase();
                let name = name.trim_start_matches('-').to_owned();
                (guac_script::MemberId::named(&ns, &name) == id).then(|| format!("{ns}.{name}"))
            });
        println!("{spec}: {} (live: {live_name:?})", c.members.name(id));
    }
}

/// A development probe for AR.11: the run from the level's start with its
/// sounds, joined and standing; every `AR11_EVERY` steps (30) to
/// `AR11_STEPS` (900), the music playing (time, rate, volume as the play
/// reads), and with `AR11_TRACE=<from>-<to>` the ops `MusicController.ai`
/// and the level's music scripts ran over those steps.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_the_music_from_the_start() {
    let (mut run, _) = common::joined_with(
        guac_script::Mode::Strict,
        common::Setup {
            props: true,
            sounds: true,
            no_music: std::env::var_os("AR11_NO_MUSIC").is_some(),
            ..common::Setup::default()
        },
    );
    let every: u64 = std::env::var("AR11_EVERY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let steps: u64 = std::env::var("AR11_STEPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(900);
    let trace = std::env::var("AR11_TRACE").ok().and_then(|s| {
        let (a, b) = s.split_once('-')?;
        Some((a.parse::<u64>().ok()?, b.parse::<u64>().ok()?))
    });
    let program = run.vm.program_arc();
    for k in 0..steps {
        if trace.is_some_and(|(a, _)| a == k) {
            run.vm.trace(Some(200_000));
        }
        run.step().unwrap_or_else(|h| panic!("{h}"));
        if let Some((_, b)) = trace
            && b == k
        {
            let mut last = None;
            for (sid, pos) in run.vm.traced() {
                let s = &program.scripts[sid as usize];
                let low = s.name.to_ascii_lowercase();
                if !(low.contains("music")
                    || low.contains("snd_")
                    || low.contains("soundcontroller"))
                {
                    continue;
                }
                if last.is_some_and(|(l, p)| l == sid && p + 1 == pos) {
                    last = Some((sid, pos));
                    continue;
                }
                let op = s.ops[pos as usize];
                println!(
                    "  trace {} #{pos} {:?}",
                    s.name, program.op_info[op as usize].class
                );
                last = Some((sid, pos));
            }
        }
        if k % every == 0 {
            let w = &run.world;
            let music: Vec<String> = w
                .plays()
                .iter()
                .filter(|p| w.play_params(p).category == guac_sound::MUSIC)
                .map(|p| {
                    let s = &w.sequences()[p.sequence as usize];
                    format!(
                        "{} t {:.2} rate {} vol {:.0} gain {:.2}",
                        p.name.as_deref().unwrap_or("?"),
                        s.time,
                        s.rate,
                        w.play_params(p).volume_percent,
                        w.master_gain(p)
                    )
                })
                .collect();
            println!(
                "step {k} clock {} drawn {}: {music:?}",
                w.sound_clock(),
                run.generator.drawn()
            );
        }
    }
}

/// A development probe for AR.11: every macro in the game systems'
/// `update while unpaused macros` set 30 steps after the join, by name, and
/// each statement of `MusicController.ai`'s tick that calls a macro.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_the_update_set() {
    let (mut run, _) = common::joined_with(
        guac_script::Mode::Play,
        common::Setup {
            props: true,
            sounds: true,
            no_music: std::env::var_os("AR11_NO_MUSIC").is_some(),
            ..common::Setup::default()
        },
    );
    for _ in 0..30 {
        run.step_play().unwrap_or_else(|h| panic!("{h}"));
    }
    let vm = &mut run.vm;
    let g = vm
        .program()
        .global_named("System Update Controls")
        .expect("the controls");
    let controls = vm.global(g).map(|v| vm.value(v)).expect("set");
    for field in [
        "update while unpaused macros",
        "update while paused macros",
        "update always macros",
    ] {
        let Some(set) = vm.field(controls, field) else {
            println!("{field}: none");
            continue;
        };
        let elems = vm
            .set(set)
            .map(<[guac_script::Value]>::to_vec)
            .unwrap_or_default();
        let program = vm.program_arc();
        let names: Vec<String> = elems
            .into_iter()
            .filter_map(|e| match vm.value(e) {
                guac_script::Value::Object(o) => program.def_of(o),
                _ => None,
            })
            .filter_map(|d| {
                let d = program.resolve(d).unwrap_or(d);
                program.defs.get(d as usize).map(|d| d.display.clone())
            })
            .collect();
        println!("{field}: {names:?}");
    }
}

/// Chapter 1's music by its own scripts (openspec AR.11), from the level's
/// start with its sounds, joined and standing, strict: the level's music
/// controller starts the intro track looping as the level starts; the
/// explore track it asks for next waits held (its rate 0, `Sound - Play With
/// Delay`) until the music controller's sound system update starts it on
/// the intro's bar (`Music - Play Music`'s beat sync), then the intro is
/// stopped; the step of each, and nothing stopped on the way.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_music_plays_the_intro_then_explore_on_its_bar() {
    let (mut run, _) = common::joined_with(
        guac_script::Mode::Strict,
        common::Setup {
            props: true,
            sounds: true,
            ..Default::default()
        },
    );
    println!("load before: {}", common::load());
    let track = |run: &common::Run, name: &str| {
        run.world
            .plays()
            .iter()
            .find(|p| p.name.as_deref() == Some(name))
            .map(|p| (p.started, run.world.sequences()[p.sequence as usize].rate))
    };
    let intro = track(&run, "mu_213_intro").expect("the intro plays from the start");
    let (mut asked, mut played, mut intro_off) = (None, None, None);
    for _ in 0..1200 {
        run.step().unwrap_or_else(|h| panic!("{h}"));
        let clock = run.world.sound_clock();
        match track(&run, "mu_213_explore") {
            Some((_, rate)) if rate == 0.0 && asked.is_none() => asked = Some(clock),
            Some((_, rate)) if rate > 0.0 && played.is_none() => played = Some(clock),
            _ => {}
        }
        if intro_off.is_none() && track(&run, "mu_213_intro").is_none() {
            intro_off = Some(clock);
        }
    }
    let ended: Vec<String> = run
        .world
        .sounds_ended()
        .filter(|e| e.name.as_deref().is_some_and(|n| n.starts_with("mu_")))
        .map(|e| format!("{:?} {} {}-{}", e.name, e.how.name(), e.started, e.ended))
        .collect();
    println!(
        "the intro from frame {} (rate {}); explore asked at {asked:?}, playing from {played:?}; the intro off at {intro_off:?}; music ended {ended:?}; load after: {}",
        intro.0,
        intro.1,
        common::load()
    );
    assert_eq!(intro.1, 100.0);
    let (asked, played, off) = (asked.unwrap(), played.unwrap(), intro_off.unwrap());
    assert!(asked < played, "held, then played");
    // On the intro's last bar: 18.84 s from its start, less a bar at 145 BPM.
    let intro_end = intro.0 as f32 + 18.84 * 60.0;
    let bar = 4.0 * 60.0 / 145.0 * 60.0;
    assert!(
        (played as f32) <= intro_end && (played as f32) >= intro_end - 2.0 * bar,
        "played at {played}, the intro ending at {intro_end}"
    );
    assert!(off >= played, "the intro stopped as explore starts: {off}");
    // The music controller's own clock, which its update times delayed
    // starts and fades by, ran with the steps (its tick counted, as its own
    // would be): 1,200 steps are 20 s.
    let places = guacamole::player_loop::Places::of(&run.vm, &run.world);
    let music = places.music.expect("the music controller runs");
    let data = run
        .vm
        .local(music, "music data")
        .map(|v| run.vm.value(v))
        .expect("its music data");
    let time = run
        .vm
        .field(data, "time")
        .and_then(|v| run.vm.value(v).as_f32())
        .expect("its time");
    println!("the music controller's time {time:.3} s");
    assert!(time > 19.9, "the music controller's time {time}");
}
