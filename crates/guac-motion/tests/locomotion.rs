//! The locomotion stand-in on the controller (openspec AG.30): hero-movement's
//! Idle, walk and run, each scenario on parry and synthetic ground, the
//! locomotion fed each frame what the controller's frame left
//! ([`Frame::of`]).

mod common;

use common::*;
use guac_motion::hero::{Hero, Input};
use guac_motion::locomotion::{Air, Clip, Frame, Gaits, Locomotion, Loops};
use guac_motion::parry::ParryWorld;
use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_motion::tuning::FOOT;

/// Eruptor's loops (AG.14).
const LOOPS: Loops = Loops {
    idle: 4.0 / 3.0,
    walk: Some(1.0667),
    run: Some(0.6667),
    fall: Some(0.8),
};

struct Walk {
    world: ParryWorld,
    hero: Hero,
    loco: Locomotion,
}

impl Walk {
    fn on(soup: &Soup, at: Vec3) -> Walk {
        let world = world(soup, &[]);
        let hero = eruptor(&world, at, 0.0);
        Walk {
            world,
            hero,
            loco: Locomotion::new(Gaits::ERUPTOR, LOOPS),
        }
    }

    /// A frame with `stick` against a view along 0°: the weights after it,
    /// and the speed the locomotion read.
    fn frame(&mut self, stick: Stick) -> ([f32; 4], f32) {
        self.hero
            .step(&self.world, &Input::stick(stick, 0.0), DT, &[]);
        self.loco.step(DT, Frame::of(&self.hero));
        (self.loco.weights(), self.loco.speed())
    }

    fn frames(&mut self, n: usize, stick: Stick) -> Vec<([f32; 4], f32)> {
        (0..n).map(|_| self.frame(stick)).collect()
    }
}

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

/// Off at full stick: the idle gives way over 0.25 s, slow at first and
/// last; the walk starts a quarter of the way into its stride; the run joins
/// 0.065-0.069 s after he first moves and has it all 0.115-0.135 s after,
/// playing at 102.4 %.
#[test]
fn off_at_full_stick() {
    let mut w = Walk::on(&open_ground(), Vec3::new(0.0, 1.0, 0.0));
    w.frames(30, Stick::CENTRED);
    assert_eq!(w.loco.weights(), [1.0, 0.0, 0.0, 0.0], "standing");
    let frames = w.frames(40, stick(1.0, 0.0));
    let moved = frames.iter().position(|(_, s)| *s > 0.0).unwrap();
    assert_eq!(moved, 0, "he moves the first frame");
    let joins = frames.iter().position(|(w, _)| w[2] > 0.0).unwrap();
    let alone = frames
        .iter()
        .position(|(w, _)| w[2] > 0.0 && w[1] == 0.0)
        .unwrap();
    let t = |k: usize| (k - moved) as f32 * DT;
    eprintln!("the run joins {} s, alone {} s after", t(joins), t(alone));
    assert!((0.065..=0.069).contains(&t(joins)), "{}", t(joins));
    assert!((0.115..=0.135).contains(&t(alone)), "{}", t(alone));
    let idle_out = frames.iter().position(|(w, _)| w[0] == 0.0).unwrap();
    assert!(close(t(idle_out), 0.25, DT), "{}", t(idle_out));
    // Eased: the first frame's slide is a sliver of a linear one's.
    let first = 1.0 - frames[1].0[0];
    assert!(first < 0.5 * (DT / 0.25), "{first}");
    assert!(close(w.loco.play(Clip::Run).rate, 1.0237, 1e-3));
    assert_eq!(w.loco.top(), Clip::Run);
}

/// Half stick and creeping: he walks with no run, his walk at 89.6 % and
/// 1.9 %, and leaves his idle as at full stick.
#[test]
fn half_stick_and_creeping() {
    for (strength, rate) in [(0.5, 0.8957), (1.0 / 96.0, 0.0187)] {
        let mut w = Walk::on(&open_ground(), Vec3::new(0.0, 1.0, 0.0));
        let frames = w.frames(60, stick(strength, 0.0));
        assert!(
            frames.iter().all(|(w, _)| w[2] == 0.0),
            "no run at {strength}"
        );
        assert_eq!(w.loco.weights(), [0.0, 1.0, 0.0, 0.0]);
        let r = w.loco.play(Clip::Walk).rate;
        assert!(close(r, rate, 2e-4), "{strength}: {r}");
        let idle_out = frames.iter().position(|(w, _)| w[0] == 0.0).unwrap();
        assert_eq!(idle_out, 15, "{strength}: 0.25 s");
    }
}

/// Coming to rest: let go at top speed, the run gives way to the walk as he
/// slows (all walk 0.10-0.14 s later), he's at rest 0.25-0.27 s later, and
/// from the frame after the idle comes back over 0.30 s, whole 0.57-0.60 s
/// after the let-go, the walk keeping its last rate as it fades.
#[test]
fn coming_to_rest() {
    let mut w = Walk::on(&open_ground(), Vec3::new(0.0, 1.0, 0.0));
    w.frames(60, stick(1.0, 0.0));
    assert_eq!(w.loco.weights(), [0.0, 0.0, 1.0, 0.0]);
    let mut frames = Vec::new();
    for _ in 0..60 {
        let f = w.frame(Stick::CENTRED);
        frames.push((f.0, f.1, w.hero.velocity, w.loco.play(Clip::Walk).rate));
    }
    // Timed as AG.11 timed them: game time from the first record he slows,
    // the let-go's own frame here.
    assert!(
        ground_speed(frames[0].2) < TOP - 1e-3,
        "slowing from the let-go"
    );
    let t = |k: usize| k as f32 * DT;
    let all_walk = frames.iter().position(|f| f.0[2] == 0.0).unwrap();
    let rest = frames.iter().position(|f| f.2 == Vec3::ZERO).unwrap();
    let whole = frames.iter().position(|f| f.0[0] == 1.0).unwrap();
    eprintln!(
        "all walk {} s, at rest {} s, the idle whole {} s after the let-go",
        t(all_walk),
        t(rest),
        t(whole)
    );
    assert!((0.10..=0.14).contains(&t(all_walk)));
    assert!((0.25..=0.27).contains(&t(rest)), "{}", t(rest));
    assert!((0.57..=0.60).contains(&t(whole)), "{}", t(whole));
    let kept = frames[rest].3;
    assert!(frames[rest..].iter().all(|f| f.3 == kept), "its last rate");
}

/// Into a wall: at full stick straight into it his walk holds 21.5 ft/s and
/// he runs on the spot at 102.4 %.
#[test]
fn into_a_wall_he_runs_on_the_spot() {
    let mut w = Walk::on(&walled(), Vec3::new(0.0, 1.0, 3.0));
    let frames = w.frames(120, stick(1.0, 0.0));
    let still = w.hero.pivot;
    w.frames(30, stick(1.0, 0.0));
    assert_eq!(w.hero.pivot, still, "against the wall");
    assert!(close(frames[119].1, 21.5, 1e-4), "{}", frames[119].1);
    assert_eq!(w.loco.weights(), [0.0, 0.0, 1.0, 0.0]);
    assert!(close(w.loco.play(Clip::Run).rate * 100.0, 102.37, 0.01));
}

/// Level ground to z = 0, a drop `depth` deep and ground under it.
fn ledge(depth: f32) -> Soup {
    let mut soup = Soup::default();
    soup.floor(0.0, [-20.0, 20.0], [-20.0, 0.0], 1.0);
    soup.wall_z(0.0, [-20.0, 20.0], [-depth, 0.0], Vec3::Z);
    soup.floor(-depth, [-20.0, 20.0], [0.0, 40.0], 1.0);
    soup
}

/// His fall: off a 6 m drop at full stick, the frame after his move first
/// goes down faster than 20 ft/s his fall comes in over 0.2 s, the run
/// fading under it; once it's whole, the frame after he's within 1.5 ft of
/// the ground (the move before slower or lower) the run comes back over
/// 0.12 s, with no landing animation.
#[test]
fn his_fall() {
    let mut w = Walk::on(&ledge(6.0), Vec3::new(0.0, 1.0, -6.0));
    let mut log = Vec::new();
    for _ in 0..180 {
        let before = (w.hero.velocity, w.hero.footing.height);
        let (weights, _) = w.frame(stick(1.0, 0.0));
        log.push((before, weights, *w.loco.air()));
    }
    let fast = log
        .iter()
        .position(|((v, _), _, _)| -v.y / FOOT > 20.0)
        .expect("a fall past 20 ft/s");
    let began = log
        .iter()
        .position(|(_, _, a)| matches!(a, Air::Falling(_)))
        .unwrap();
    assert_eq!(began, fast, "the frame after the move that passed 20 ft/s");
    assert_eq!(log[began].1[3], 0.0, "nothing shows the frame it begins");
    let whole = log.iter().position(|(_, w, _)| w[3] == 1.0).unwrap();
    assert_eq!(whole - began, 12, "0.2 s");
    for (k, (_, w, _)) in log.iter().enumerate().take(whole + 1).skip(began) {
        let want = (k - began) as f32 * DT / 0.2;
        assert!(close(w[3], want.min(1.0), 1e-5), "{k}");
        assert!(close(w[2], 1.0 - want.min(1.0), 1e-5), "the run under it");
    }
    let back = log
        .iter()
        .position(|(_, _, a)| matches!(a, Air::Landing(_)))
        .unwrap();
    let (_, low) = log[back].0;
    assert!(low.is_some_and(|h| h < 1.5 * FOOT), "{low:?}");
    let ground = log[back..]
        .iter()
        .position(|(_, _, a)| *a == Air::Ground)
        .unwrap();
    assert_eq!(ground, 8, "back over 0.12 s");
    assert_eq!(*w.loco.air(), Air::Ground);
    assert_eq!(
        w.loco.weights(),
        [0.0, 0.0, 1.0, 0.0],
        "running on, no landing"
    );
}

/// A drop under 20 ft/s: off a 0.3 m step at full stick the snap takes him
/// down and he runs on with no fall; off a 0.6 m one he drops, and any move
/// down past 20 ft/s comes within 1.5 ft of the ground, so no fall either.
#[test]
fn a_drop_under_20_ft_s() {
    for depth in [0.3, 0.6] {
        let mut w = Walk::on(&ledge(depth), Vec3::new(0.0, 1.0, -3.0));
        let mut fastest = 0.0f32;
        for _ in 0..90 {
            let (weights, _) = w.frame(stick(1.0, 0.0));
            fastest = fastest.max(-w.hero.velocity.y / FOOT);
            assert_eq!(weights[3], 0.0, "{depth} m");
        }
        eprintln!("the {depth} m drop's fastest move down: {fastest} ft/s");
        assert!(
            w.hero.pivot.z > 2.0 && w.hero.pivot.y < 0.05 - depth,
            "{:?}",
            w.hero.pivot
        );
        assert_eq!(w.loco.weights(), [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(*w.loco.air(), Air::Ground);
    }
}

/// Standing: at the start for 8.3 s (AG.11's longest stand) he plays his
/// idle loop throughout and nothing else.
#[test]
fn standing() {
    let mut w = Walk::on(&open_ground(), Vec3::new(0.0, 1.0, 0.0));
    for (weights, speed) in w.frames(500, Stick::CENTRED) {
        assert_eq!(weights, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(speed, 0.0);
    }
    assert!(close(w.loco.play(Clip::Idle).time as f32, 500.0 * DT, 1e-3));
}

/// At the level's edge the edge probe zeroes his walk before it's read, so
/// held there with the stick still pushed he comes to rest in his idle (the
/// walk's 0.30 s fade, a hypothesis at AG.11's 37 such stops).
#[test]
fn held_at_an_edge_he_comes_to_rest() {
    // A 30 m drop the probe refuses (225 ft is its reach, but nothing is
    // below the floor's edge within 4 ft of his walk's point).
    let mut soup = Soup::default();
    soup.floor(0.0, [-20.0, 20.0], [-20.0, 0.0], 1.0);
    let mut w = Walk::on(&soup, Vec3::new(0.0, 1.0, -6.0));
    let mut held = None;
    for k in 0..240 {
        w.frame(stick(1.0, 0.0));
        if held.is_none() && w.hero.held_off {
            held = Some(k);
        }
    }
    assert!(held.is_some(), "the probe never held him");
    assert_eq!(w.loco.speed(), 0.0);
    assert_eq!(w.loco.weights(), [1.0, 0.0, 0.0, 0.0]);
}
