//! Falls, landings, crests and let-gos on slopes in the controller (openspec
//! AG.16 on AG.10, AG.38 and AG.39): hero-movement's Falling and landing,
//! Slopes and Speeding up and slowing down's Let go on a slope, on parry and
//! synthetic ground.

mod common;

use common::*;
use guac_motion::hero::Input;
use guac_motion::scenarios;
use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_motion::tuning::FOOT;

/// Level ground to z = 0, a drop `depth` deep and ground under it, its face
/// shared with the upper floor's edge.
fn ledge(depth: f32, below: impl FnOnce(&mut Soup)) -> Soup {
    let mut soup = Soup::default();
    soup.floor(0.0, [-20.0, 20.0], [-20.0, 0.0], 1.0);
    soup.wall_z(0.0, [-20.0, 20.0], [-depth, 0.0], Vec3::Z);
    below(&mut soup);
    soup
}

/// Upward speed no landing reaches, m/s: f32 noise at 4 m down moves his
/// pivot by a few 1e-7 m a frame as the solve keeps him off the ground he
/// lands 5 mm over; a bounce would be metres a second.
const NO_RISE: f32 = 1e-4;

fn flat_below(depth: f32) -> Soup {
    ledge(depth, |s| {
        s.floor(-depth, [-20.0, 20.0], [0.0, 40.0], 1.0);
    })
}

fn walk(soup: &Soup, from: Vec3, frames: usize, stick_at: impl Fn(usize) -> Stick) -> Vec<State> {
    let world = world(soup, &[]);
    let mut hero = eruptor(&world, from, 0.0);
    (0..frames)
        .map(|k| {
            hero.step(&world, &Input::stick(stick_at(k), 0.0), DT, &[]);
            state(&hero)
        })
        .collect()
}

/// Where gravity, not the crest's band, acts (AG.38's 0.4 ft).
fn high(s: &State) -> bool {
    !s.supported && s.height.is_none_or(|h| h / FOOT >= 0.4)
}

/// Falling off the ledge route: off a 4 m drop at full stick, every frame up
/// in the air his downward speed grows by 136.21 ft/s² x the step (1/60 s
/// here), his walk across the ground at 21.5 ft/s; he lands without a bounce
/// and walks on.
#[test]
fn falling_off_a_ledge() {
    let states = walk(&flat_below(4.0), Vec3::new(0.0, 1.0, -6.0), 150, |_| {
        stick(1.0, 0.0)
    });
    let airborne: Vec<usize> = (1..states.len())
        .filter(|&k| high(&states[k - 1]))
        .collect();
    assert!(airborne.len() > 10, "{airborne:?}");
    let mut grew = 0;
    for &k in &airborne {
        let (a, b) = (states[k - 1], states[k]);
        if high(&b) {
            assert!(
                (a.walk.y - b.walk.y - 136.21 * DT).abs() < 1e-3,
                "{k}: {a:?} {b:?}"
            );
            assert!((ground_speed(b.velocity) - TOP).abs() < 1e-3, "{k}");
            grew += 1;
        }
    }
    assert!(grew > 10);
    let landed = states.iter().rposition(high).unwrap() + 1;
    assert!(
        states[landed..].iter().all(|s| s.velocity.y <= NO_RISE),
        "no bounce"
    );
    let end = states.last().unwrap();
    assert!(
        (end.pivot.y + 4.0 - 0.005).abs() < 6e-3 && end.supported,
        "{end:?}"
    );
    assert!((ground_speed(end.velocity) - TOP).abs() < 1e-3, "{end:?}");
}

/// A long drop: past 16.1 m of fall his downward speed stops at 120 ft/s
/// (36.58 m/s), and he lands without bouncing. The terminal speed is read,
/// not seen: no fall in reach is long enough (AG.47's).
#[test]
fn a_long_drop_levels_off_at_120_ft_s() {
    let states = walk(&flat_below(30.0), Vec3::new(0.0, 1.0, -3.0), 240, |k| {
        if k < 60 {
            stick(1.0, 0.0)
        } else {
            Stick::CENTRED
        }
    });
    let fastest = states.iter().map(|s| s.walk.y).fold(0.0f32, f32::min);
    assert_eq!(fastest, -120.0);
    let at_terminal = states.iter().filter(|s| s.walk.y == -120.0).count();
    assert!(at_terminal > 5, "{at_terminal}");
    let landed = states.iter().rposition(high).unwrap() + 1;
    assert!(states[landed..].iter().all(|s| s.velocity.y <= NO_RISE));
    assert!((states.last().unwrap().pivot.y + 30.0 - 0.005).abs() < 6e-3);
}

/// Steering in the air: swung 90° 0.1 s into a fall at full stick, his walk
/// turns toward it 6.0-7.7° a frame over the first three frames, at 6.553
/// m/s; reversed, he slows 0.7-0.9 m/s a frame; let go, 0.23-0.28 m/s a
/// frame, his fall unchanged (AG.10's air recordings).
#[test]
fn steering_and_letting_go_in_the_air() {
    // The swing starts 6 frames after he's first up in the air.
    let world = world(&flat_below(4.0), &[]);
    for (degrees, check) in [
        (Some(90.0), "swing"),
        (Some(180.0), "reverse"),
        (None, "let go"),
    ] {
        let mut hero = eruptor(&world, Vec3::new(0.0, 1.0, -6.0), 0.0);
        let mut up_for = 0;
        let mut states = Vec::new();
        for _ in 0..200 {
            let st = state(&hero);
            if high(&st) {
                up_for += 1;
            }
            let stick = if up_for < 6 {
                stick(1.0, 0.0)
            } else {
                match degrees {
                    Some(d) => stick(1.0, d),
                    None => Stick::CENTRED,
                }
            };
            hero.step(&world, &Input::stick(stick, 0.0), DT, &[]);
            if up_for >= 6 && high(&st) {
                states.push((st, state(&hero)));
            }
            if up_for >= 6 && states.len() >= 3 {
                break;
            }
        }
        for (a, b) in &states {
            let turn = angle_between(bearing(a.velocity), bearing(b.velocity));
            let (sa, sb) = (ground_speed(a.velocity), ground_speed(b.velocity));
            match check {
                "swing" => {
                    assert!((6.0..=7.7).contains(&turn) || turn < 6.0, "{turn}");
                    assert!((sb - TOP).abs() < 1e-3, "{sb}");
                }
                "reverse" => assert!((0.7..=0.9).contains(&(sa - sb)), "{}", sa - sb),
                _ => {
                    assert!((0.23..=0.28).contains(&(sa - sb)), "{}", sa - sb);
                    assert!(turn < 1e-3);
                }
            }
        }
        if check == "swing" {
            let first = angle_between(bearing(states[0].0.velocity), bearing(states[0].1.velocity));
            assert!((6.0..=7.7).contains(&first), "{first}");
        }
    }
}

/// Landing on the corridor's slope: from a fall begun well up, he never
/// rises; the frame after he lands he slides straight down the slope, and
/// the frame after that walks the stick's way from that speed.
#[test]
fn landing_on_a_slope_slides_down_its_fall_line() {
    // Under the ledge, ground falling away at 17° along +x, across his way.
    let soup = ledge(4.0, |s| {
        let a = 17.0f32.to_radians();
        let (lo, hi) = (
            Vec3::new(-20.0, -4.0 + 20.0 * a.tan(), 0.0),
            Vec3::new(20.0, -4.0 - 20.0 * a.tan(), 0.0),
        );
        s.quad(
            [lo, lo + Vec3::Z * 40.0, hi + Vec3::Z * 40.0, hi],
            Vec3::new(a.sin(), a.cos(), 0.0),
        );
    });
    let states = walk(&soup, Vec3::new(0.0, 1.0, -6.0), 150, |_| stick(1.0, 0.0));
    let last_up = states.iter().rposition(high).unwrap();
    let landed = (last_up + 1..states.len())
        .find(|&k| states[k].supported)
        .unwrap();
    assert!(
        states[landed..].iter().all(|s| s.velocity.y <= NO_RISE),
        "no rise"
    );
    let slide = states[landed + 1];
    let down = Vec3::new(1.0, 0.0, 0.0);
    assert!(
        angle_between(bearing(slide.velocity), bearing(down)) < 0.5,
        "{slide:?}"
    );
    let on = states[landed + 2];
    assert!(angle_between(bearing(on.velocity), 0.0) < 1.0, "{on:?}");
    assert!(ground_speed(on.velocity) > ground_speed(slide.velocity) * 0.9);
}

/// The dip's edge ahead of the start: level ground falling away 1.4° past
/// z = 0 (AG.38's `a` crossed it).
fn dip() -> Soup {
    let mut soup = Soup::default();
    soup.floor(0.0, [-20.0, 20.0], [-20.0, 0.0], 2.0);
    soup.ramp(Vec3::ZERO, -1.4, [-20.0, 20.0], 20.0);
    soup
}

/// Slopes, the dip's edge: he leaves the ground for a frame over it and walks
/// on at √(v² + 10²) ft/s from v, held while the stick is: from half stick's
/// 3.277 m/s to 4.486 (√ gives 4.475, the push tilted to the slope that frame
/// the rest; AG.8's `g1` at 0.500 of the stick 4.4862-4.4878, AG.38's at 0.49
/// 3.179 to 4.415); at full stick the cut keeps 6.553 m/s.
#[test]
fn the_dips_edge_at_half_and_full_stick() {
    for (s, want) in [(0.5, 4.484..=4.489), (1.0, TOP - 1e-3..=TOP + 1e-3)] {
        let states = walk(&dip(), Vec3::new(0.0, 1.0, -6.0), 120, |_| stick(s, 0.0));
        let off = states.iter().filter(|st| !st.supported).count();
        assert_eq!(off, 1, "{s}: {off} frames off");
        let end = states.last().unwrap();
        let speed = end.velocity.length();
        assert!(want.contains(&speed), "{s}: {speed}");
    }
}

/// A 17.3° plane through the origin rising toward `uphill` (a heading).
fn slope(uphill: f32) -> Soup {
    let a = 17.3f32.to_radians();
    let (s, c) = uphill.to_radians().sin_cos();
    let up_dir = Vec3::new(s, 0.0, c);
    let side = Vec3::new(c, 0.0, -s);
    let n = (Vec3::Y * a.cos() - up_dir * a.sin()).normalize();
    let mut soup = Soup::default();
    for i in -10..10 {
        for j in -10..10 {
            let corner = |di: i32, dj: i32| {
                let p = side * (2.0 * (i + di) as f32) + up_dir * (2.0 * (j + dj) as f32);
                p + Vec3::Y * (p.dot(up_dir) * a.tan())
            };
            soup.quad([corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)], n);
        }
    }
    soup
}

/// Let go on a slope: walking down, across and up a 17.3° slope at full
/// stick and let go, he slides 0.81-1.05 m in 0.23-0.27 s and stops dead
/// from what his walk's vertical part leaves: walking down, all of it (1.32-
/// 2.54 m/s); across, next to nothing (0.01-0.52); up, the first frame let go
/// leaves the ground and its 10 ft/s turns the part down, so he stops from
/// 10 ft/s less it: 1.10 m/s for a walk pitched the slope's 17.3° (AG.38's
/// `g38_t2979_up`, 29.7° off the fall line, its walk pitched 16.3°, 1.21
/// m/s, that recording's 1.21-2.73 the lower end).
#[test]
fn let_go_on_a_slope() {
    let world = world(&slope(0.0), &[]);
    for way in [180.0, 90.0, 0.0] {
        let mut hero = eruptor(&world, Vec3::new(0.0, 3.0, 0.0), 0.0);
        let input = |s| Input::stick(s, way);
        for _ in 0..45 {
            hero.step(&world, &input(stick(1.0, 0.0)), DT, &[]);
        }
        let released = hero.pivot;
        let vertical = hero.walk.velocity.y;
        let mut moving = Vec::new();
        let mut off = 0;
        for _ in 0..60 {
            hero.step(&world, &input(Stick::CENTRED), DT, &[]);
            if hero.velocity != Vec3::ZERO {
                moving.push(hero.walk.velocity.length() * FOOT);
            }
            off += usize::from(!hero.footing.supported);
        }
        let slid = (hero.pivot - released).length();
        let seconds = moving.len() as f32 * DT;
        let last = *moving.last().unwrap();
        assert!((0.81..=1.05).contains(&slid), "{way}: {slid} m");
        assert!((0.23..=0.27).contains(&seconds), "{way}: {seconds} s");
        let (from, hop) = match way {
            0.0 => ((10.0 - vertical) * FOOT, 1),
            _ => (vertical.abs() * FOOT, 0),
        };
        assert_eq!(off, hop, "{way}");
        if way == 90.0 {
            assert!((0.01..=0.52).contains(&last), "{last}");
        } else {
            // What's left of the across part adds under 0.05 m/s.
            let near = (last - from).abs() < 5e-2;
            assert!(near, "{way}: from {last} m/s, not {from}");
        }
        if way == 180.0 {
            assert!((1.32..=2.54).contains(&last), "{last}");
        }
    }
}

/// Falling and landing's open edge (AG.48), on parry: dropped 0.8 m out
/// from an overhang's open top edge he slides round it and lands below,
/// moving every frame; 0.45 m out it holds him up.
#[test]
fn round_an_open_edge_he_falls_on() {
    on_parry(scenarios::round_an_open_edge_he_falls_on());
}

#[test]
fn an_open_edge_within_45_holds_him_up() {
    on_parry(scenarios::an_open_edge_within_45_holds_him_up());
}
