//! A hero's locomotion as his scripts pick it on the VM (openspec AH.49, AH
//! design decision 5's second case; stand-in ledger: `locomotion`):
//! `global.bld`'s locomotion script (`Macros_Animation.ai`'s every tick)
//! plays his idle, walk, run and fall at once and writes each one's
//! `animationinfo.blend percent` and `sequence.playback rate` every tick, as
//! AG.11's recordings hold them (AH.49's replays: every record's weights and
//! rates but AG.11's one known parting). This reads what it left on him for
//! his pose and the TV's gait reports; AG.30's picker is off for him while
//! it does (one side picks, never both).
//!
//! The weighted clips are the base clips the script set the layered bit on
//! ([`crate::script_engine::Sequence::weighted`]). They're drawn by AG.30's
//! reading of a record: his idle at its own weight and his fall at its own,
//! his moving gaits sharing what's left by their weights, the shares
//! summing to 1 (AG.30's blend over them, normalized linear, a hypothesis
//! AC.13's one capture supports for two clips). The script writes the gaits
//! either as the rest of the idle's weight or as their own split of 100, a
//! set-off's records alternating between the two (AG.11, AG.30); read
//! this way both forms draw alike, where weights normalized as written
//! would flicker between them a frame at a time. Which pose the original
//! draws of the two forms no recording shows; the reading is AG.30's, its
//! idle and share what AH.49's replays hold to the recordings. His scripts'
//! other clips are laid over these ([`crate::hands::playing`]). The reports
//! name four of them by their clips: his model's idle and his hero side's
//! walk, run and fall (AG.14), found in his palette by name.

use guac_ir::clip::Clip;
use guac_ir::palette::{Palette, SlotEntry};
use guac_motion::locomotion::{Handover, Play};
use guac_script::Handle;

use crate::scripts::ScriptWorld;

/// A clip the locomotion weights on him, as the step left it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weighted {
    /// Its slot in his palette.
    pub slot: u32,
    /// Seconds into it.
    pub time: f32,
    /// Percent.
    pub rate: f32,
    /// `blend percent`.
    pub weight: f32,
}

/// The palette slots of the four clips the gait reports name: idle, walk,
/// run and fall, each `None` where his palette holds no clip of that name.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Four(pub [Option<u32>; 4]);

impl Four {
    /// Each named clip's slot in `palette`: `names` is idle, walk, run and
    /// fall, a `None` naming none.
    pub fn of(palette: &Palette, names: [Option<&str>; 4]) -> Self {
        Four(names.map(|name| {
            let name = name?;
            palette
                .entries
                .iter()
                .find(|e| matches!(&e.entry, SlotEntry::Clip { clip, .. } if clip.name == name))
                .map(|e| u32::from(e.slot))
        }))
    }
}

/// What his locomotion on the VM left on him this step.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Picked {
    /// Each weighted clip playing on him, in the order started.
    pub clips: Vec<Weighted>,
    pub four: Four,
    /// His walk's speed, ft/s: what his step moved him by.
    pub speed: f32,
}

impl Picked {
    /// What the VM weights on `actor` now, `None` while it weights nothing
    /// on him (no locomotion of his scripts runs: AG.30's picker then).
    pub fn read(world: &ScriptWorld, actor: Handle, four: Four, speed: f32) -> Option<Picked> {
        let clips: Vec<Weighted> = world
            .sequences()
            .iter()
            .filter(|s| s.actor == actor && s.plays() && s.weighted())
            .filter_map(|s| {
                let (_, slot) = world.clip_of(s)?;
                Some(Weighted {
                    slot,
                    time: s.time,
                    rate: s.rate,
                    weight: s.blend,
                })
            })
            .collect();
        (!clips.is_empty()).then_some(Picked { clips, four, speed })
    }

    /// The weight of the clip in `slot`, percent; 0 for one not weighted.
    fn weight_of(&self, slot: Option<u32>) -> f32 {
        slot.and_then(|s| self.clips.iter().rev().find(|c| c.slot == s))
            .map_or(0.0, |c| c.weight)
    }

    fn clip_of(&self, slot: Option<u32>) -> Option<&Weighted> {
        slot.and_then(|s| self.clips.iter().rev().find(|c| c.slot == s))
    }

    /// Idle, walk, run and fall as drawn (the module docs): summing to 1
    /// with only these four weighted; all 0 with nothing weighted.
    pub fn weights(&self) -> [f32; 4] {
        let shares = self.shares(&self.by_slot());
        self.four.0.map(|s| {
            s.and_then(|s| shares.iter().rev().find(|(x, _)| *x == s))
                .map_or(0.0, |(_, w)| *w)
        })
    }

    /// Each of `weights`' clips (by slot, percent as written) as drawn: the
    /// idle and the fall each its own weight over 100, the idle first, each
    /// other clip its share of what's left by its weight among them; all 0
    /// when nothing weighs anything.
    fn shares(&self, weights: &[(u32, f32)]) -> Vec<(u32, f32)> {
        let [idle, .., fall] = self.four.0;
        let total: f32 = weights.iter().map(|(_, w)| w.max(0.0)).sum();
        if total <= 0.0 {
            return weights.iter().map(|(s, _)| (*s, 0.0)).collect();
        }
        let own = |slot: Option<u32>| {
            slot.and_then(|s| weights.iter().rev().find(|(x, _)| *x == s))
                .map_or(0.0, |(_, w)| (w * 0.01).clamp(0.0, 1.0))
        };
        let i = own(idle);
        let f = own(fall).min(1.0 - i);
        let rest = (1.0 - i - f).max(0.0);
        let moving: f32 = weights
            .iter()
            .filter(|(s, _)| Some(*s) != idle && Some(*s) != fall)
            .map(|(_, w)| w.max(0.0))
            .sum();
        // Standing (no moving gait weighs anything), the idle takes the rest
        // and each gait draws 0, not 0 over 0.
        let (i, rest) = if moving > 0.0 {
            (i, rest)
        } else {
            (i + rest, 0.0)
        };
        weights
            .iter()
            .map(|&(s, w)| {
                let share = if Some(s) == idle {
                    i
                } else if Some(s) == fall {
                    f
                } else if moving > 0.0 {
                    rest * w.max(0.0) / moving
                } else {
                    0.0
                };
                (s, share)
            })
            .collect()
    }

    /// The walk's and the run's rates, 1 their authored speed.
    pub fn rates(&self) -> [f32; 2] {
        [1, 2].map(|i| self.clip_of(self.four.0[i]).map_or(1.0, |c| c.rate * 0.01))
    }

    /// The run's share of the walk and the run: of their weights as the
    /// script wrote them, a record's own split (AG.11).
    pub fn run_share(&self) -> f32 {
        let (walk, run) = (
            self.weight_of(self.four.0[1]),
            self.weight_of(self.four.0[2]),
        );
        if walk + run > 0.0 {
            run / (walk + run)
        } else {
            0.0
        }
    }

    /// The heaviest of the four: `idle`, `walk`, `run` or `fall`, the first
    /// of equals.
    pub fn top(&self) -> &'static str {
        const NAMES: [&str; 4] = ["idle", "walk", "run", "fall"];
        let w = self.weights();
        let mut best = 0;
        for i in 1..4 {
            if w[i] > w[best] {
                best = i;
            }
        }
        NAMES[best]
    }

    /// Where this left him, for AG.30's locomotion to carry on from once his
    /// scripts stop picking (openspec AH.198): the four as drawn, each where
    /// it plays (its rate in AG.30's terms, 1 its authored speed),
    /// and the speed read.
    pub fn handover(&self) -> Handover {
        Handover {
            weights: self.weights(),
            plays: self.four.0.map(|slot| {
                self.clip_of(slot).map(|c| Play {
                    time: f64::from(c.time),
                    rate: c.rate * 0.01,
                })
            }),
            speed: self.speed,
        }
    }

    /// `falling` while his fall clip carries weight, else `ground`: the
    /// report's air reads the clip the script's fall check brings in, there
    /// being no state of AG.30's to read.
    pub fn air(&self) -> &'static str {
        if self.weight_of(self.four.0[3]) > 0.0 {
            "falling"
        } else {
            "ground"
        }
    }

    /// Whether his walk moves him: over the VM's 1e-4 float tolerance, which
    /// his walk's resting 3.6e-19 ft/s sits under (AG.38).
    pub fn moving(&self) -> bool {
        self.speed > 1e-4
    }

    /// Each weight by slot, for the next step's easing.
    pub fn by_slot(&self) -> Vec<(u32, f32)> {
        self.clips.iter().map(|c| (c.slot, c.weight)).collect()
    }

    /// The four as drawn `alpha` of the way from their last step to their
    /// next, a step `step` seconds long: each one's time this step less
    /// what's left of the step at its rate, none before 0, and its share
    /// (the module docs) of the weights eased from `before`'s (the step
    /// before's, by slot).
    pub fn drawn_four(&self, before: &[(u32, f32)], (alpha, step): (f32, f64)) -> [(f64, f32); 4] {
        let shares = self.shares(&self.eased(before, alpha));
        let back = f64::from(1.0 - alpha) * step;
        self.four.0.map(|slot| {
            let Some(c) = self.clip_of(slot) else {
                return (0.0, 0.0);
            };
            let w = shares
                .iter()
                .rev()
                .find(|(s, _)| Some(*s) == slot)
                .map_or(0.0, |(_, w)| *w);
            let at = (f64::from(c.time) - back * f64::from(c.rate) * 0.01).max(0.0);
            (at, w)
        })
    }

    /// Each weighted clip's weight `alpha` of the way from `before`'s (its
    /// slot's the step before; one not weighted then from 0) to this
    /// step's.
    fn eased(&self, before: &[(u32, f32)], alpha: f32) -> Vec<(u32, f32)> {
        self.clips
            .iter()
            .map(|c| {
                let from = before
                    .iter()
                    .rev()
                    .find(|(s, _)| *s == c.slot)
                    .map_or(0.0, |(_, w)| *w);
                (c.slot, from + (c.weight - from) * alpha)
            })
            .collect()
    }

    /// The clips to pose him by out of `palette`, each at its time and
    /// share as [`Picked::drawn_four`] eases them: every weighted clip with
    /// a share. With none (a weight of 0 on all of them, as on the frame his
    /// scripts' designer influence slides back from 0 after a whole clip),
    /// his idle at its time alone: nothing in the recordings shows that
    /// frame's pose, and the reference pose would flash (a hypothesis).
    pub fn posed<'p>(
        &self,
        before: &[(u32, f32)],
        palette: &'p Palette,
        (alpha, step): (f32, f64),
    ) -> Vec<(&'p Clip, f64, f64)> {
        let back = f64::from(1.0 - alpha) * step;
        let eased = self.shares(&self.eased(before, alpha));
        let mut out: Vec<(&'p Clip, f64, f64)> = self
            .clips
            .iter()
            .zip(&eased)
            .filter(|(_, (_, w))| *w > 0.0)
            .filter_map(|(c, (_, w))| {
                let clip = palette.clip(u16::try_from(c.slot).ok()?)?;
                let at = (f64::from(c.time) - back * f64::from(c.rate) * 0.01).max(0.0);
                Some((clip, at, f64::from(*w)))
            })
            .collect();
        if out.is_empty()
            && let Some(c) = self.clip_of(self.four.0[0])
            && let Some(clip) = palette.clip(u16::try_from(c.slot).unwrap_or(u16::MAX))
        {
            let at = (f64::from(c.time) - back * f64::from(c.rate) * 0.01).max(0.0);
            out.push((clip, at, 1.0));
        }
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use guac_ir::clip::{RotationTrack, Track, VecTrack};
    use guac_ir::palette::PaletteSlot;
    use guac_ir::{DumpIdentity, Sha256};

    use super::*;

    pub(crate) fn clip(name: &str, duration: f32) -> Clip {
        Clip {
            name: name.into(),
            duration,
            frame_duration: 1.0 / 30.0,
            frames: 2,
            tracks: vec![Track {
                joint: 0,
                translation: VecTrack {
                    constant: [0.0; 3],
                    spline: None,
                },
                rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
                scale: VecTrack {
                    constant: [1.0; 3],
                    spline: None,
                },
            }],
        }
    }

    /// Eruptor's palette's locomotion slots as his file holds them: idle 0,
    /// walk 1, run 3, a lean 12 (additive) and his fall 57.
    pub(crate) fn palette() -> Palette {
        let entry = |slot: u16, name: &str, length: f32, additive: bool| PaletteSlot {
            slot,
            name: None,
            label: None,
            entry: SlotEntry::Clip {
                additive,
                partial: false,
                layered: false,
                motion_extraction: 0,
                clip: clip(name, length),
            },
        };
        Palette {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: Sha256([0; 32]),
            },
            owner: "009_Eruptor".into(),
            slots: 64,
            entries: vec![
                entry(0, "Eruptor_Idle.ma", 1.3333, false),
                entry(1, "Eruptor_Walk.ma", 1.0667, false),
                entry(3, "Eruptor_Run.ma", 0.6667, false),
                entry(12, "Eruptor_LeanLeft.ma", 0.8, true),
                entry(57, "Eruptor_Falling.ma", 0.8, false),
            ],
            refused: Vec::new(),
        }
    }

    fn four() -> Four {
        Four::of(
            &palette(),
            [
                Some("Eruptor_Idle.ma"),
                Some("Eruptor_Walk.ma"),
                Some("Eruptor_Run.ma"),
                Some("Eruptor_Falling.ma"),
            ],
        )
    }

    fn picked(weights: [(u32, f32, f32); 4]) -> Picked {
        Picked {
            clips: weights
                .iter()
                .map(|&(slot, weight, rate)| Weighted {
                    slot,
                    time: 0.5,
                    rate,
                    weight,
                })
                .collect(),
            four: four(),
            speed: 14.0,
        }
    }

    /// The four found by name in his palette, a name it lacks naming none.
    #[test]
    fn the_four_are_found_by_their_clips_names() {
        assert_eq!(four(), Four([Some(0), Some(1), Some(3), Some(57)]));
        let none = Four::of(
            &palette(),
            [Some("Eruptor_Idle.ma"), None, Some("Nope.ma"), None],
        );
        assert_eq!(none, Four([Some(0), None, None, None]));
    }

    /// A record's two forms mid-set-off (AG.11): the idle at 40 % beside the
    /// gaits' own split of 100 (walk 60, run 40), or the gaits as the rest
    /// of the idle's (walk 36, run 24). Both draw alike: the idle 0.4, the
    /// gaits the other 0.6 by their weights, the run's share 0.4; the rates
    /// the script wrote, in hundredths. Normalized as written the first
    /// would draw the idle at 40/140, a flicker between the frames.
    #[test]
    fn a_records_two_forms_draw_alike_and_its_share_and_rates_read() {
        let p = picked([
            (0, 40.0, 100.0),
            (1, 60.0, 124.0),
            (3, 40.0, 117.0),
            (57, 0.0, 100.0),
        ]);
        let rest = picked([
            (0, 40.0, 100.0),
            (1, 36.0, 124.0),
            (3, 24.0, 117.0),
            (57, 0.0, 100.0),
        ]);
        let w = p.weights();
        for form in [&p, &rest] {
            for (got, want) in form.weights().iter().zip([0.4, 0.36, 0.24, 0.0]) {
                assert!((got - want).abs() < 1e-6, "{:?}", form.weights());
            }
            assert!((form.run_share() - 0.4).abs() < 1e-6);
        }
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!((p.run_share() - 0.4).abs() < 1e-6);
        assert_eq!(p.rates(), [1.24, 1.17]);
        assert_eq!(p.top(), "idle");
        assert_eq!(p.air(), "ground");
        assert!(p.moving());
        let falling = picked([
            (0, 0.0, 100.0),
            (1, 30.0, 100.0),
            (3, 0.0, 100.0),
            (57, 70.0, 100.0),
        ]);
        assert_eq!((falling.top(), falling.air()), ("fall", "falling"));
    }

    /// AH.198: what AG.30's locomotion carries on from is the four as drawn, each
    /// where it plays at AG.30's rate (1 its authored speed), the
    /// speed read; a slot not playing hands over nothing.
    #[test]
    fn the_handover_is_the_four_as_drawn_where_they_play() {
        use guac_motion::locomotion::Play;
        let p = picked([
            (0, 40.0, 100.0),
            (1, 60.0, 124.0),
            (3, 40.0, 117.0),
            (57, 0.0, 100.0),
        ]);
        let h = p.handover();
        assert_eq!(h.weights, p.weights());
        assert_eq!(
            h.plays,
            [100.0f32, 124.0, 117.0, 100.0].map(|r| Some(Play {
                time: 0.5,
                rate: r * 0.01
            }))
        );
        assert_eq!(h.speed, 14.0);
        let mut no_fall = p.clone();
        no_fall.clips.pop();
        assert_eq!(no_fall.handover().plays[3], None);
    }

    /// Eased between steps: each weight from the step before's toward this
    /// one's, a clip new this step from 0; each time back by the rest of
    /// the step at its rate.
    #[test]
    fn the_pose_eases_weights_and_times_between_steps() {
        let p = picked([
            (0, 50.0, 100.0),
            (1, 50.0, 50.0),
            (3, 0.0, 100.0),
            (57, 0.0, 100.0),
        ]);
        let before = vec![(0, 100.0), (3, 0.0)];
        let pal = palette();
        let posed = p.posed(&before, &pal, (0.5, 1.0 / 60.0));
        let names: Vec<&str> = posed.iter().map(|(c, ..)| c.name.as_str()).collect();
        assert_eq!(names, ["Eruptor_Idle.ma", "Eruptor_Walk.ma"]);
        assert!((posed[0].2 - 0.75).abs() < 1e-6, "idle eased from 100");
        assert!((posed[1].2 - 0.25).abs() < 1e-6, "walk eased from nothing");
        assert!((posed[0].1 - (0.5 - 0.5 / 60.0)).abs() < 1e-9);
        assert!((posed[1].1 - (0.5 - 0.5 / 60.0 * 0.5)).abs() < 1e-9);
        let four = p.drawn_four(&before, (0.5, 1.0 / 60.0));
        assert!((four[0].1 - 0.75).abs() < 1e-6 && (four[1].1 - 0.25).abs() < 1e-6);
    }

    /// A walker's locomotion on the VM (openspec AG.161), from a script
    /// built with AH.13's writer that does what `global.bld`'s does to a
    /// state's clip: its STARTUP takes his idle, walk and run out of his
    /// palette, sets each one's layered bit and looping and plays it, and
    /// his behaviour writes each one's `blend percent` and playback rate
    /// every tick. The pick reads the three as his locomotion's, at the
    /// weights and rates the script wrote, drawn by AG.30's reading; his
    /// fall, never played, weighs nothing.
    #[test]
    fn a_walkers_script_picks_his_clips_by_what_it_writes() {
        use std::sync::Arc;

        use guac_formats::igz::Igz;
        use guac_formats::igz::builder::IgzBuilder;
        use guac_formats::script::builder::{El, Writer};
        use guac_formats::script::{Class, Graph};
        use guac_script::{Generator, Mode, Obj, Program, Role, Unit, Whitelist};

        use crate::script_engine::NoEngine;
        use crate::scripts::{
            Files, Placed, WorldHost, headless_handles, headless_objects, headless_spawner,
            hero_config, init_chain,
        };

        const PATH: &str = "C:/tfb/Synthetic/Walker.ai";
        const MY: El<'static> = El::Member("optoplevelbehavior", "my");
        // Slot, weight and rate written each tick: idle, walk, run.
        const WRITTEN: [(i32, f32, f32); 3] =
            [(0, 20.0, 100.0), (1, 30.0, 150.0), (3, 50.0, 102.5)];
        let mut b = IgzBuilder::new();
        {
            let w = &mut Writer::new(&mut b);
            let clip = w.variable("clip", "ScriptReference", None);
            let seqs = WRITTEN
                .map(|(slot, ..)| w.variable(&format!("state {slot}"), "ScriptReference", None));
            w.script(
                PATH,
                |w| {
                    for ((slot, ..), seq) in WRITTEN.iter().zip(seqs) {
                        // setref clip = my.animation palette [slot]
                        let lhs = w.value(&[El::Local(clip)]);
                        let rhs = w.reference(
                            &[MY, El::Member("animationinfo", "animation palette")],
                            None,
                        );
                        let index = w.int(*slot);
                        let o = w.op(Class::SetReference, "setref||=");
                        w.refer(o, 0x1C, lhs);
                        w.refer(o, 0x20, rhs);
                        w.refer(o, 0x24, index);
                        w.push(o);
                        let one = w.int(1);
                        w.set(
                            &[El::Local(clip), El::Member("animationinfo", "is layered")],
                            one,
                        );
                        let one = w.int(1);
                        w.set(&[El::Local(clip), El::Member("sequence", "looping")], one);
                        // start sequence clip { setref state = <it> }
                        let started = w.group(&[El::Local(clip)], None);
                        let (p, o) = w.open(Class::StartSequence, "start sequence");
                        w.refer(o, 0x1C, started);
                        let l = w.value(&[El::Local(seq)]);
                        let r = w.reference(&[El::Local(o)], None);
                        let set = w.op(Class::SetReference, "set reference");
                        w.refer(set, 0x1C, l);
                        w.refer(set, 0x20, r);
                        w.push(set);
                        w.end_block(p);
                    }
                },
                |_| {},
                |w| {
                    let walk = w.behaviour("walk");
                    for ((_, weight, rate), seq) in WRITTEN.iter().zip(seqs) {
                        let weight = w.float(*weight);
                        w.set(
                            &[El::Local(seq), El::Member("animationinfo", "blend percent")],
                            weight,
                        );
                        let rate = w.float(*rate);
                        w.set(
                            &[El::Local(seq), El::Member("sequence", "playback rate")],
                            rate,
                        );
                    }
                    w.end_block(walk);
                },
            );
        }
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let program = Arc::new(
            Program::link(&[Unit {
                name: "level",
                role: Role::Other,
                igz: &igz,
                graph: &graph,
            }])
            .unwrap(),
        );
        let obj = Obj { unit: 0, index: 9 };
        // Running (activity state 4), from the level's start.
        let files = Files {
            placed: vec![Placed::named(obj, "walker", Some(PATH), 4)],
            ..Files::default()
        };
        let handles = headless_handles(&files);
        let mut c = hero_config(Mode::Strict);
        c.whitelist = Whitelist::All;
        let mut vm = init_chain(program, &files, &handles, &[0], c);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        world.set_palette_of(obj, &palette());
        let mut spawn = headless_spawner(1 << 40);
        for _ in 0..3 {
            let mut host = WorldHost {
                world: &mut world,
                engine: &mut NoEngine,
                generator: &mut Generator::seeded(1),
                spawn: &mut spawn,
            };
            vm.step(&mut host).unwrap_or_else(|h| panic!("{h}"));
        }
        let p = Picked::read(&world, handles[&obj], four(), 14.0).expect("his locomotion");
        let got: Vec<(u32, f32, f32)> =
            p.clips.iter().map(|c| (c.slot, c.weight, c.rate)).collect();
        assert_eq!(got, WRITTEN.map(|(s, w, r)| (s as u32, w, r)));
        let w = p.weights();
        for (got, want) in w.iter().zip([0.2, 0.3, 0.5, 0.0]) {
            assert!((got - want).abs() < 1e-6, "{w:?}");
        }
        assert_eq!(p.rates(), [1.5, 1.025]);
        assert!(
            world
                .sequences()
                .iter()
                .all(|s| s.looping && !s.laid_over())
        );
    }

    /// Nothing weighted with weight (the frame his scripts' influence
    /// slides back from 0): his idle alone at its time, never no clip.
    #[test]
    fn with_no_weight_on_any_clip_his_idle_stands() {
        let p = picked([
            (0, 0.0, 100.0),
            (1, 0.0, 100.0),
            (3, 0.0, 100.0),
            (57, 0.0, 100.0),
        ]);
        let pal = palette();
        let posed = p.posed(&[], &pal, (1.0, 1.0 / 60.0));
        assert_eq!(posed.len(), 1);
        assert_eq!(
            (posed[0].0.name.as_str(), posed[0].2),
            ("Eruptor_Idle.ma", 1.0)
        );
        assert_eq!(p.weights(), [0.0; 4]);
    }

    /// Standing at rest the script weights his idle 100 and his walk and run
    /// 0 (AG.11's records at rest): the idle draws whole and the gaits 0,
    /// never 0 over 0 (the review of AH.49: the TV's gait report read the
    /// walk and run as NaN on 154 of Tree Rex's 389 steps on the Mac).
    #[test]
    fn standing_his_gaits_draw_0() {
        let p = picked([
            (0, 100.0, 100.0),
            (1, 0.0, 100.0),
            (3, 0.0, 100.0),
            (57, 0.0, 100.0),
        ]);
        assert_eq!(p.weights(), [1.0, 0.0, 0.0, 0.0]);
        let four = p.drawn_four(&[(0, 100.0), (1, 0.0), (3, 0.0)], (0.5, 1.0 / 60.0));
        assert_eq!(four.map(|c| c.1), [1.0, 0.0, 0.0, 0.0]);
        let pal = palette();
        let posed = p.posed(&[], &pal, (1.0, 1.0 / 60.0));
        assert_eq!(posed.len(), 1);
        assert_eq!(
            (posed[0].0.name.as_str(), posed[0].2),
            ("Eruptor_Idle.ma", 1.0)
        );
    }
}
