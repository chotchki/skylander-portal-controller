use std::collections::BTreeMap;

use guac_oracle::fight::draws::MtWords;
use guac_oracle::fight::extract::{BreakRule, ChainRule};
use guac_oracle::fight::oracle::ChainItem;
use guac_oracle::fight::ticks::{Cue, HERO};
use guac_oracle::input::{Button, Ds3Pad};
use guac_oracle::recording::{Body, Clip, Clock, Frame};
use guac_oracle::rng::Mt19937;

use super::*;
use crate::chains::{GiantsChains, Resolved};
use crate::rng::AnimLcg;
use crate::watch::Chains;

const THROW: u32 = 3;
const IDLE: u32 = 1;

/// One replay from anchor A as the lob script makes it: South pressed at
/// frame `press`, the throw layered over the idle for 20 frames, the ball
/// listed from `ball` to `burst`, the barrel gone at `burst` and its coin's
/// entry 2 frames later; the generator drawing 5 a frame up to frame
/// `part`, then `late` a frame.
struct Script {
    press: u32,
    ball: u32,
    burst: u32,
    late: u64,
    part: u32,
}

fn frame(k: u32, south: bool, throw: bool) -> Frame {
    let mut clips = vec![Clip {
        id: IDLE,
        weight: 100.0,
        rate: 100.0,
        period: 1.333,
        remaining: 0.5,
    }];
    if throw {
        clips.push(Clip {
            id: THROW,
            weight: 100.0,
            rate: 150.0,
            period: 0.5,
            remaining: 0.25,
        });
    }
    Frame {
        flip: 2 + u64::from(k),
        read: 0,
        host_us: 16_667 * u64::from(k),
        game_frame: 6_633 + k,
        clock: Clock {
            frame_delta: 1.0 / 60.0,
            presentation_rate: 1.0 / 60.0,
            logic_scale: 1.0,
            logic_stopped: false,
            object_scale: 1.0,
        },
        hero: Body {
            position: [31.09, 51.46, -72.98],
            rotation: [0.0, 253.7, 0.0],
            velocity: [0.0; 3],
        },
        camera: None,
        stick: [0.0, 0.0],
        pad: Some(Ds3Pad {
            digital2: if south { 0x40 } else { 0 },
            ..Ds3Pad::REST
        }),
        clips,
        contacts: None,
        mover: None,
    }
}

fn item(address: u32, kind: &str, placement: Option<&str>, at: [f32; 3]) -> ChainItem {
    ChainItem {
        address,
        kind: kind.into(),
        placement: placement.map(Into::into),
        serial: placement.is_none().then_some(0),
        body: Some(Body {
            position: at,
            rotation: [0.0; 3],
            velocity: [0.0; 3],
        }),
        behaviour: None,
        vars: BTreeMap::new(),
        missing: 0,
    }
}

/// The replay's files in a new dir under `root`.
fn write(root: &Path, label: &str, s: &Script) -> PathBuf {
    let dir = root.join(label);
    fs::create_dir_all(&dir).unwrap();
    let start = Mt19937::sgenrand(10_554);
    let mut g = start.clone();
    let barrel = [25.67, 53.28, -74.58];
    let mut frames = Vec::new();
    let mut records = Vec::new();
    for k in 0..40u32 {
        let f = frame(
            k,
            (s.press..s.press + 6).contains(&k),
            (s.press + 1..s.press + 21).contains(&k),
        );
        let mut items = vec![item(0x100, HERO, Some("009_Eruptor"), f.hero.position)];
        if k < s.burst {
            items.push(item(0x200, "breakable", Some("barrel_b_intact(1)"), barrel));
        }
        if (s.ball..s.burst).contains(&k) {
            items.push(item(0x300, "projectile", None, [30.5, 52.5, -73.2]));
        }
        if k >= s.burst + 2 {
            items.push(item(0x400, "coin", None, [25.9, 53.0, -74.4]));
        }
        records.push(ChainRecord {
            flip: f.flip,
            read: 0,
            game_frame: f.game_frame,
            items,
            mt: Some(MtWords {
                mti: g.mti(),
                word0: g.words()[0],
            }),
        });
        frames.push(f);
        let n = if k < s.part { 5 } else { s.late };
        for _ in 0..n {
            g.next_u32();
        }
    }
    let recording = Recording::new(frames).unwrap();
    fs::write(
        dir.join(RECORDING_FILE),
        serde_json::to_string(&recording).unwrap(),
    )
    .unwrap();
    let file = ChainFile {
        identity: "oracle sha256=test".into(),
        chains: GiantsChains {
            chains: Chains {
                slots: BTreeMap::new(),
            },
            actors: Vec::new(),
            hero_corpus: 0x100,
            hero_placement: 0,
            updater_index: 3,
            combat_control: 0,
            macros_ui: 0,
        },
        resolved: Resolved {
            clip_names: BTreeMap::from([
                (IDLE, "Eruptor_Idle.ma".to_owned()),
                (THROW, "Eruptor_RunAttackLeft.ma".to_owned()),
            ]),
            ..Resolved::default()
        },
        keys: Vec::new(),
        stride: 0,
    };
    fs::write(dir.join(CHAIN_FILE), serde_json::to_string(&file).unwrap()).unwrap();
    fs::write(
        dir.join(CHAIN_RECORDS),
        serde_json::to_string(&records).unwrap(),
    )
    .unwrap();
    let peek = Peek {
        flip: Some(2),
        meta: 0,
        instances: 1,
        instance: 0,
        refcount: 1,
        mt: start,
        anim: AnimLcg {
            seed: 0x12345,
            current: 0x12345,
        },
        fmod: None,
    };
    fs::write(
        dir.join("peek-f0.json"),
        serde_json::to_string(&peek).unwrap(),
    )
    .unwrap();
    dir
}

fn rules() -> ReportRules {
    let breaks = BreakRule {
        kinds: vec!["breakable".into()],
        broken: Cue::Gone,
        coin: "coin".into(),
    };
    ReportRules {
        spawns: vec![
            SpawnRule {
                kind: "projectile".into(),
                parents: vec![HERO.into()],
                radius: 3.0,
                within: 1,
            },
            SpawnRule {
                kind: "coin".into(),
                parents: vec!["breakable".into()],
                radius: 2.0,
                within: 3,
            },
        ],
        rules: Rules {
            chain: Some(ChainRule {
                attack: Button::South,
                throw: Cue::Playing(vec![
                    "Eruptor_RunAttackLeft.ma".into(),
                    "Eruptor_RunAttackRight.ma".into(),
                ]),
                ball: "projectile".into(),
                burst: Cue::Gone,
                breaks: breaks.clone(),
                reach: 6.0,
                window: 0.5,
            }),
            breaks: Some(breaks),
            ..Rules::default()
        },
        ordered: BTreeSet::new(),
        quantum: 0.005,
        max_twists: 100,
        stamps_past_horizon: false,
    }
}

#[test]
fn a_groups_floors_pass_its_own_replays_and_fail_a_press_moved() {
    let tmp = tempfile::tempdir().unwrap();
    let s = |press, ball, burst, late| Script {
        press,
        ball,
        burst,
        late,
        part: 30,
    };
    // Four replays: one ball a frame late, one burst a frame late, the
    // generator parting after frame 30 in two of them.
    let dirs = [
        write(tmp.path(), "lob-00", &s(3, 8, 20, 9)),
        write(tmp.path(), "lob-01", &s(3, 9, 20, 9)),
        write(tmp.path(), "lob-02", &s(3, 8, 21, 7)),
        write(tmp.path(), "lob-03", &s(3, 8, 20, 9)),
    ];
    // The press 3 frames late, everything after it with it.
    let late = write(tmp.path(), "lob_shift3", &s(6, 11, 23, 9));
    let rules = rules();
    let group: Vec<Replay> = dirs.iter().map(|d| load(d, &rules).unwrap()).collect();
    let r = GroupReport::build(&group, &[load(&late, &rules).unwrap()], &rules).unwrap();
    // The draws agree through frame 30 (a record holds the draws before its
    // frame's) and part at 31, frame 30's own draws apart.
    assert_eq!(
        (r.horizon.last_agreeing, r.horizon.parted_at),
        (Some(30), Some(31))
    );
    let first = &r.replays[0];
    assert_eq!(first.per_frame[..3], [5, 5, 5]);
    // The chain: the press at 3, the throw from 4, the ball from 8, its
    // burst and the barrel's break at 20, the coin named by the barrel.
    let link = &first.links[0];
    assert_eq!(link.press.map(|p| p.frame), Some(3));
    assert_eq!(link.throw.frame, 4);
    assert_eq!(link.ball.as_ref().map(|b| b.at.frame), Some(8));
    assert_eq!(link.burst.map(|b| b.frame), Some(20));
    assert_eq!(
        link.broke.as_ref().map(|b| b.name.as_str()),
        Some("barrel_b_intact(1)")
    );
    assert_eq!(
        link.coins
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["barrel_b_intact(1)>coin#1"]
    );
    assert_eq!(first.totals["drops"], 1);
    // The floors: a frame for the ball's spawn and the barrel's break (and
    // the ball's leaving and the coin's spawn with it), rounded up to 0.02.
    let f = 1.0 / 60.0;
    assert!((r.needed["spawn"] - f).abs() < 1e-9, "{:?}", r.needed);
    assert!((r.needed["break"] - f).abs() < 1e-9, "{:?}", r.needed);
    assert_eq!(r.floors["spawn"], 0.02);
    assert_eq!(r.needed.get("press South"), Some(&0.0));
    assert!(r.leave_one_out.iter().all(|j| j.passed), "{}", r.text());
    // The press 3 frames late fails, in the hero's log first.
    let (c, j) = &r.candidates[0];
    assert_eq!(c.label, "lob_shift3");
    assert!(!j.passed);
    assert!(j.failed.contains(&HERO.to_owned()), "{:?}", j.failed);
    // A floor at 0 fails the replays a frame off.
    let open = Judge {
        floors: BTreeMap::new(),
        ..r.judge.clone()
    };
    let runs: Vec<ActorLogs> = r.replays.iter().map(|x| x.logs.clone()).collect();
    assert!(
        leave_one_out(&runs, &open)
            .unwrap()
            .iter()
            .any(|v| !v.passed())
    );
    let text = r.text();
    assert!(text.contains("candidate lob_shift3: fails"), "{text}");
    // With the generator parting at frame 3 instead (anchor A's parts at 1
    // to 4), every event lies past the horizon: nothing is stamped, every floor reads 0
    // and the late press passes on order alone; judged past the horizon too,
    // the floors come back and it fails again.
    let early: Vec<PathBuf> = [(8, 20, 9), (9, 20, 7), (8, 21, 9), (8, 20, 7)]
        .iter()
        .enumerate()
        .map(|(k, &(ball, burst, late))| {
            let script = Script {
                part: 2,
                ..s(3, ball, burst, late)
            };
            write(tmp.path(), &format!("early-{k:02}"), &script)
        })
        .collect();
    let build = |rules: &ReportRules| {
        let g: Vec<Replay> = early.iter().map(|d| load(d, rules).unwrap()).collect();
        GroupReport::build(&g, &[load(&late, rules).unwrap()], rules).unwrap()
    };
    let gated = build(&rules);
    assert_eq!(
        (gated.horizon.last_agreeing, gated.judge.horizon),
        (Some(2), Some(2))
    );
    assert!(
        gated.floors.values().all(|&v| v == 0.0),
        "{:?}",
        gated.floors
    );
    assert!(gated.candidates[0].1.passed);
    let open = build(&ReportRules {
        stamps_past_horizon: true,
        ..rules.clone()
    });
    assert_eq!(open.judge.horizon, None);
    assert_eq!(open.floors["spawn"], 0.02);
    assert!(open.leave_one_out.iter().all(|j| j.passed));
    assert!(!open.candidates[0].1.passed);
    assert!(open.text().contains("stamps judged everywhere"));
}

#[test]
fn a_report_writes_beside_its_rules_and_refuses_a_dir_without_a_peek() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Script {
        press: 3,
        ball: 8,
        burst: 20,
        late: 9,
        part: 30,
    };
    let dirs: Vec<PathBuf> = (0..3)
        .map(|k| write(tmp.path(), &format!("lob-{k:02}"), &s))
        .collect();
    let at = tmp.path().join("rules.ron");
    fs::write(&at, ron::to_string(&rules()).unwrap()).unwrap();
    let out = tmp.path().join("report");
    let r = report(&at, &dirs, &[], &out).unwrap();
    assert!(out.join("report.json").exists() && out.join("report.txt").exists());
    // Three alike: every floor 0, the horizon never parting.
    assert!(r.floors.values().all(|&v| v == 0.0), "{:?}", r.floors);
    assert_eq!(r.horizon.parted_at, None);
    fs::remove_file(dirs[1].join("peek-f0.json")).unwrap();
    let err = report(&at, &dirs, &[], &tmp.path().join("again")).unwrap_err();
    assert!(format!("{err:#}").contains("no F0 peek"), "{err:#}");
    // A quantum that isn't positive is refused.
    let mut bad = rules();
    bad.quantum = 0.0;
    assert!(ReportRules::from_ron(&ron::to_string(&bad).unwrap()).is_err());
    assert_eq!(round_up(1.0 / 60.0, 0.005), 0.02);
    assert_eq!(round_up(0.0, 0.005), 0.0);
    assert_eq!(round_up(0.01, 0.005), 0.01);
}
