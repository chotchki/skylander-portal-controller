//! The locomotion on the controller (openspec AG.30, hero-movement's Idle,
//! walk and run; stand-in ledger: `locomotion`): each scenario on parry and
//! synthetic ground, the locomotion fed each frame what the controller's
//! frame left ([`Frame::of`]).

mod common;

use common::*;
use guac_motion::hero::{Hero, Input};
use guac_motion::locomotion::{Air, Clip, Frame, Gaits, Locomotion, Loops, eased};
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
/// he runs on the spot at 102.4 %, the run's stride running on through a
/// 4.5 s hold, never restarted (AG.58's `w58_hold`, Held against it).
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
    for k in 0..270 {
        let before = w.loco.play(Clip::Run);
        let (weights, speed) = w.frame(stick(1.0, 0.0));
        let run = w.loco.play(Clip::Run);
        assert_eq!(w.hero.pivot, still, "{k}: against the wall");
        assert_eq!(weights, [0.0, 0.0, 1.0, 0.0], "{k}");
        assert!(close(speed, 21.5, 1e-4), "{k}: {speed}");
        let ran = run.time - before.time;
        assert!(
            (ran - f64::from(DT) * f64::from(before.rate)).abs() < 1e-9,
            "{k}: the run's stride moved {ran}"
        );
    }
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

/// A drop under 20 ft/s: off a 0.3 m step onto level ground at full stick
/// he runs on over it, supported, at the step's height (within the snap's
/// reach a frame begun on the ground ends supported, and the snap waits for
/// his lean to turn: AG.70), with no fall; off a 0.6 m one he drops, and any
/// move down past 20 ft/s comes within 1.5 ft of the ground, so no fall
/// either.
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
        assert!(w.hero.pivot.z > 2.0, "{:?}", w.hero.pivot);
        if depth < 0.375 {
            // Still 5 mm over the step's top, supported, 0.3 m over the
            // ground he runs on.
            assert!(
                w.hero.footing.supported && (w.hero.pivot.y - 0.005).abs() < 2e-3,
                "{:?} {:?}",
                w.hero.pivot,
                w.hero.footing
            );
        } else {
            assert!(w.hero.pivot.y < 0.05 - depth, "{:?}", w.hero.pivot);
        }
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

/// AG.58 (hero-movement's Idle, walk and run, At an edge with nothing
/// below): held at an edge on ground falling 3° toward it with the stick
/// still pushed, as at the bridge's foot past the gate's door's end. The
/// probe stops him dead and the run's weight goes to the walk that frame,
/// the speed read the walk's vertical part; from the frame after the idle
/// comes back over 0.30 s, eased, and isn't turned back while the probe
/// flickers: each frame the slow point passes the push is zeroed again,
/// leaving only its slope's share (0.14 ft/s), which restarts the walk a
/// quarter into its stride and sets the run's rate without setting him off,
/// so the walk's stride holds at a quarter as it fades (AG.58's door runs:
/// 2.23-2.74 ft/s read at the stop, 9-10 restarts in the 0.30-0.34 s after
/// it, the walk at 0.28-0.30 s).
#[test]
fn held_at_a_sloped_edge_his_idle_comes_back_through_the_probes_flicker() {
    // A 3° ramp down toward +z to an open edge at z = 0, y = 0.
    let mut soup = Soup::default();
    let fall = 20.0 * 3.0f32.to_radians().tan();
    soup.ramp(Vec3::new(0.0, fall, -20.0), -3.0, [-20.0, 20.0], 20.0);
    let mut w = Walk::on(&soup, Vec3::new(0.0, 2.0, -12.0));
    w.frames(30, Stick::CENTRED);
    let mut log = Vec::new();
    for _ in 0..150 {
        let (weights, speed) = w.frame(stick(1.0, 0.0));
        let walk = w.loco.play(Clip::Walk);
        // What the TV's gait report and log hand out as `held`.
        assert_eq!(w.loco.held(), w.hero.probe_held);
        log.push((
            weights,
            speed,
            w.hero.probe_held,
            walk.time,
            w.loco.moving(),
        ));
    }
    let stop = log
        .iter()
        .position(|f| f.2)
        .expect("the probe stops him short of the edge");
    let (weights, read, ..) = log[stop];
    assert_eq!(
        weights,
        [0.0, 1.0, 0.0, 0.0],
        "the run's weight to the walk"
    );
    let slope = 21.5 * 3.0f32.to_radians().sin();
    assert!(read > 0.0 && read <= slope + 1e-3, "{read}");
    // The flicker: frames the push is zeroed again, a speed read on each.
    let flickers: Vec<usize> = (stop + 2..log.len())
        .filter(|&k| log[k].2 && log[k].1 > 0.0)
        .collect();
    eprintln!(
        "stopped {stop}, read {read} ft/s; {} flickers, the first {:?}",
        flickers.len(),
        flickers.first()
    );
    assert!(flickers.len() >= 9, "{flickers:?}");
    for &k in &flickers {
        let (_, speed, _, time, moving) = log[k];
        assert!(speed < 0.2, "{k}: {speed}");
        assert!(!moving, "{k}: a flicker doesn't set him off");
        let quarter = 0.25 * 1.0667 + f64::from(DT);
        assert!((time - quarter).abs() < 1e-6, "{k}: the walk at {time}");
    }
    // The idle's own slide from the frame after the stop, never turned back.
    for (k, f) in log.iter().enumerate().skip(stop + 1) {
        let x = (k - (stop + 1)) as f32 * DT / 0.30;
        let want = eased(x.min(1.0), 0.3);
        assert!(
            close(f.0[0], want, 1e-5),
            "{k}: the idle at {} not {want}",
            f.0[0]
        );
        assert!(close(f.0[1], 1.0 - want, 1e-5), "{k}: the walk under it");
        assert_eq!(f.0[2], 0.0, "{k}: no run");
    }
    assert_eq!(w.loco.weights(), [1.0, 0.0, 0.0, 0.0]);
}

/// AG.68 (hero-movement's Idle, walk and run, Let go from a long run): under
/// a pure run the walk plays on at the last mix's rate (Eruptor's 144-147 %
/// as he passes 12-21 ft/s), out of step with the run, so the first frame of
/// the mix after a let-go (the frame after it, its own reading the walk
/// before its first slowing) the run takes the walk's place in its stride plus
/// its own advance, at about 90 % of the weight, as the original's does
/// (AG.68's recordings, each record advanced by the step of the one before:
/// the run on the walk's place to 1e-7 s at 15 of 16 let-gos, the jump
/// 0.02-0.42 of its cycle). The longer the run, the further apart the two
/// drift, so the jump is the original's and stays: after the same time at top
/// speed it lands within 0.01 of the original's two replays (`r68_short`
/// −0.168 and −0.190, `r68_long` +0.400 and +0.415).
#[test]
fn let_go_from_a_long_run_the_run_takes_the_walks_place() {
    // The original's jumps after the same time at top speed (AG.68).
    for (seconds, theirs) in [(1.25f32, -0.190..=-0.168), (9.0, 0.400..=0.415)] {
        let mut w = Walk::on(&open_ground(), Vec3::new(0.0, 1.0, -36.0));
        w.frames(30, Stick::CENTRED);
        let run_steps = (seconds / DT).round() as usize;
        w.frames(run_steps, stick(1.0, 0.0));
        assert_eq!(
            w.loco.weights(),
            [0.0, 0.0, 1.0, 0.0],
            "{seconds} s: the run alone"
        );
        let kept = w.loco.play(Clip::Walk).rate;
        assert!(
            (1.44..=1.47).contains(&kept),
            "{seconds} s: the walk kept {kept}"
        );
        // The let-go's own frame reads the walk before its first slowing.
        let (weights, speed) = w.frame(Stick::CENTRED);
        assert_eq!((weights[2], speed), (1.0, 21.5), "{seconds} s");
        let walk_before = w.loco.play(Clip::Walk);
        let run_before = w.loco.play(Clip::Run);
        let (weights, speed) = w.frame(Stick::CENTRED);
        let share = weights[2] / (weights[1] + weights[2]);
        assert!(
            speed < 21.5 && (0.85..0.95).contains(&share),
            "{seconds} s: {speed} {share}"
        );
        let run = w.loco.play(Clip::Run);
        let place = walk_before.phase(1.0667) * 0.6667 + f64::from(DT * run.rate);
        assert!(
            (run.time - place).abs() < 1e-5,
            "{seconds} s: the run at {} not the walk's place {place}",
            run.time
        );
        let own = run_before.time + f64::from(DT * run_before.rate);
        let jump = ((run.time - own) / 0.6667 + 0.5).rem_euclid(1.0) - 0.5;
        eprintln!("{seconds} s at top speed: the run jumps {jump:+.3} of its cycle at {share:.3}");
        assert!(
            jump >= theirs.start() - 0.01 && jump <= theirs.end() + 0.01,
            "{seconds} s: a jump of {jump:+.3} against the original's {theirs:?}"
        );
    }
}
