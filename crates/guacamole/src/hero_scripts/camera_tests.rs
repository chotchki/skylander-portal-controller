//! The gameplay camera on its own scripts (openspec AG.28), on AG.28's
//! synthetic camera ([`guac_formats::script::builder::camera`]): the TV's
//! app with Eruptor joined, each fixed step's view as the TV drew it beside
//! the hero, the viewport and `camera control data` as the scripts left
//! them.

use std::path::Path;

use bevy::input::gamepad::{
    GamepadAxis, GamepadButton, RawGamepadAxisChangedEvent, RawGamepadEvent,
};
use bevy::prelude::*;
use guac_extract::intros::IrFormat;
use guac_formats::script::builder::camera::{
    CAMERA_ACTIVATOR, CAMERA_ACTIVATOR_DELAY, CAMERA_ACTIVATOR_RANGE, CAMERA_ACTIVATOR_SECONDS,
    CAMERA_BACK, CAMERA_EASE, CAMERA_LOOK_UP, CAMERA_PLANE_SIZE, CAMERA_ROCKS, CAMERA_SHAKE,
    CAMERA_SHAKE_SECONDS, CAMERA_START, CAMERA_VOLUME, CAMERA_VOLUME_NAME, CAMERA_VOLUME_SECONDS,
    CameraAt, CameraFault, SyntheticCam,
};
use guac_formats::script::builder::{WALKER_LOAD, WalkerFault};
use guac_ir::palette::Palette;
use guac_script::{PlacementId, Vm, eased};
use guacamole::frame::Frame;
use guacamole::script_engine::{METRES_PER_FOOT as FT, direction};
use guacamole::scripts::{ScriptRuntime, ScriptWorld};
use guacamole::viewport::{Source, Viewport};

use super::ScriptSource;
use crate::hero::{FollowCamera, HeroBody, NOT_YET, ViewHold};
use crate::play::{CameraReport, Mode};
use crate::test_support::hero_scripts::{write_camera, write_camera_with_walker};
use crate::test_support::level::{FLOOR, START, lights, write_level};
use crate::test_support::play::{
    ERUPTOR, WALKER_TOY, clear, join, level_state, mode, place, play_app, play_app_walkers, until,
    write_attack_parts,
};
use crate::test_support::test_pad;

/// Where the synthetic level's placements stand: the plane 2.5 m ahead of
/// the start across the walk to the wall, the volume 4 m to his +x side,
/// the activator 14 m off behind him (asleep) and its rock beyond it.
fn spots() -> CameraAt {
    CameraAt {
        plane: [START[0], FLOOR + 1.8, START[2] + 2.5],
        volume: [START[0] + 4.0, FLOOR + 1.8, START[2]],
        activator: [START[0] - 10.0, FLOOR, START[2] - 10.0],
        rock: [START[0] - 10.0, FLOOR, START[2] - 12.0],
    }
}

/// One fixed step as the TV left it: what each test's expectations are
/// checked against, step by step.
#[derive(Debug, Clone)]
struct Sample {
    /// The run's steps after it.
    step: u64,
    /// His feet and facing after his move.
    feet: Vec3,
    facing: f32,
    /// The TV's camera: its place, the way it looks, its far clip (m) and
    /// its picture's height (degrees).
    eye: Vec3,
    look: Vec3,
    far: f32,
    fov: f32,
    hold: ViewHold,
    report: CameraReport,
    /// The viewport as the scripts left it.
    view: Viewport,
    /// `camera control data`'s `pct` and `shake x`.
    pct: f32,
    shake: f32,
    /// The activator's behaviour.
    activator: Option<String>,
}

#[derive(Resource, Default)]
struct Samples(Vec<Sample>);

/// A field of `camera control data` as a number.
fn control(vm: &Vm, field: &str) -> f32 {
    let Some(g) = vm.program().global_named("camera control data") else {
        return f32::NAN;
    };
    vm.global_peek(g)
        .map(|v| vm.value(v))
        .and_then(|v| vm.field(v, field))
        .and_then(|v| vm.value(v).as_f32())
        .unwrap_or(f32::NAN)
}

/// The placement running [`CAMERA_ACTIVATOR`] in the run, found through
/// the part's files (its behaviour tells its wake).
fn activator(vm: &Vm, world: &ScriptWorld, source: &ScriptSource) -> Option<PlacementId> {
    let p = source
        .files
        .placed
        .iter()
        .find(|p| p.name == CAMERA_ACTIVATOR)?;
    vm.placement_of(*world.handles().get(&p.obj)?)
}

fn record(
    runtime: Option<Res<ScriptRuntime>>,
    world: Option<Res<ScriptWorld>>,
    source: Option<Res<ScriptSource>>,
    heroes: Query<&HeroBody>,
    cameras: Query<(&FollowCamera, &Transform, &Projection)>,
    mut out: ResMut<Samples>,
) {
    let (Some(runtime), Some(world), Some(source)) = (runtime, world, source) else {
        return;
    };
    let (Ok(hero), Ok((follow, t, p))) = (heroes.single(), cameras.single()) else {
        return;
    };
    let Projection::Perspective(perspective) = p else {
        return;
    };
    let vm = &runtime.vm;
    out.0.push(Sample {
        step: runtime.steps,
        feet: hero.hero.pivot,
        facing: hero.hero.facing,
        eye: t.translation,
        look: *t.forward(),
        far: perspective.far,
        fov: perspective.fov.to_degrees(),
        hold: follow.hold.clone(),
        report: CameraReport::held(t, p, follow).unwrap(),
        view: *world.viewport(),
        pct: control(vm, "pct"),
        shake: control(vm, "shake x"),
        activator: activator(vm, &world, &source)
            .and_then(|id| vm.behaviour(id))
            .map(str::to_owned),
    });
}

/// The TV on AG.28's synthetic camera, its placements at [`spots`], its
/// update stopping as `fault` says: Eruptor placed and taken in, his
/// scripts joined, each fixed step recorded from then on. The app and the
/// pad that took him over.
fn camera_app(tmp: &Path, fault: CameraFault) -> (App, Entity) {
    let (mut app, tx) = play_app(tmp);
    app.init_resource::<Samples>()
        .add_systems(FixedUpdate, record.in_set(Frame::Record));
    write_attack_parts(tmp);
    write_camera(tmp, spots(), fault);
    tx.send(write_level(tmp, &lights())).unwrap();
    let pad = test_pad::connect(&mut app, None, None);
    until(&mut app, "the level and its scripts", |a| {
        level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
    });
    place(&mut app, 0, ERUPTOR, "Eruptor");
    until(&mut app, "the level", |a| mode(a) != Mode::Portal);
    test_pad::press(&mut app, pad, GamepadButton::East);
    until(&mut app, "his scripts", |a| {
        join(a).as_deref() == Some("joined")
    });
    (app, pad)
}

fn samples(app: &App) -> &[Sample] {
    &app.world().resource::<Samples>().0
}

fn last(app: &App) -> Sample {
    samples(app).last().cloned().expect("a step recorded")
}

/// Frames until `done` holds of the last step recorded, up to `frames`.
fn run_until(app: &mut App, what: &str, frames: usize, done: impl Fn(&Sample) -> bool) {
    for _ in 0..frames {
        app.update();
        if samples(app).last().is_some_and(&done) {
            return;
        }
    }
    panic!(
        "{what}: not within {frames} frames, last {:?}",
        samples(app).last()
    );
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn hero_entity(app: &App) -> Entity {
    app.world()
        .resource::<crate::play::Play>()
        .hero()
        .expect("in the level")
}

/// Puts him at `p` (his pivot, his feet), as a test sets a hero down.
fn set_down(app: &mut App, p: Vec3) {
    let e = hero_entity(app);
    app.world_mut()
        .entity_mut(e)
        .get_mut::<HeroBody>()
        .unwrap()
        .hero
        .pivot = p;
}

/// A camera's numbers `k` of the way from `a`'s to `b`'s: distance,
/// pitch, heading and far clip.
fn between(a: SyntheticCam, b: SyntheticCam, k: f32) -> [f32; 4] {
    let l = |x: f32, y: f32| x + (y - x) * k;
    [
        l(a.distance, b.distance),
        l(a.pitch, b.pitch),
        l(a.heading, b.heading),
        l(a.far, b.far),
    ]
}

/// The view the synthetic update leaves for `numbers` on feet at `feet`:
/// the eye out along the heading and pitch by the distance, looking at the
/// point [`CAMERA_LOOK_UP`] ft over his feet; the eye, the way it looks and
/// the far clip (m).
fn view_of([distance, pitch, heading, far]: [f32; 4], feet: Vec3) -> (Vec3, Vec3, f32) {
    let eye = feet + direction(heading, pitch) * distance * FT;
    let aim = feet + Vec3::Y * CAMERA_LOOK_UP * FT;
    (eye, (aim - eye).normalize(), far * FT)
}

fn assert_view(s: &Sample, want: (Vec3, Vec3, f32), what: &str) {
    let (eye, look, far) = want;
    assert!(
        s.eye.distance(eye) < 2e-3,
        "{what}: step {} the eye {} drawn, {eye} the scripts'",
        s.step,
        s.eye
    );
    assert!(
        s.look.distance(look) < 1e-4,
        "{what}: step {} looking {} drawn, {look} the scripts'",
        s.step,
        s.look
    );
    assert!(
        (s.far - far).abs() < 1e-2,
        "{what}: step {} far {} drawn, {far} the scripts'",
        s.step,
        s.far
    );
}

/// Each step after the first with the one before it, where the run made
/// exactly one step between them.
fn pairs(samples: &[Sample]) -> impl Iterator<Item = (&Sample, &Sample)> {
    samples
        .windows(2)
        .map(|w| (&w[0], &w[1]))
        .filter(|(a, b)| b.step == a.step + 1)
}

/// AG.28: the level's definition defines its cameras and switches to the
/// first at once, and once Eruptor has joined the camera's scripts on the
/// VM the TV draws their viewport, one writer: the eye 30 ft out behind his
/// feet 25° up, looking at the point 6 ft over them, the picture 68°'s
/// height and the far clip 600 ft; `/api/_test/tv` says the scripts have
/// it. Standing still, the right stick held or not, it holds.
#[test]
fn a_definitions_camera_takes_the_view() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, pad) = camera_app(tmp.path(), CameraFault::None);
    run_until(&mut app, "the scripts' view", 30, |s| {
        s.hold == ViewHold::Scripts
    });
    frames(&mut app, 10);
    // The right stick held at a corner moves nothing (AH.41: the
    // original's camera reads none).
    for (axis, value) in [
        (GamepadAxis::RightStickX, 1.0),
        (GamepadAxis::RightStickY, 1.0),
    ] {
        app.world_mut()
            .write_message(RawGamepadEvent::from(RawGamepadAxisChangedEvent::new(
                pad, axis, value,
            )));
    }
    frames(&mut app, 20);
    let all = samples(&app);
    let on: Vec<&Sample> = all.iter().filter(|s| s.hold == ViewHold::Scripts).collect();
    assert!(on.len() >= 30, "{} steps on the scripts' view", on.len());
    for s in &on {
        assert_view(
            s,
            view_of(between(CAMERA_START, CAMERA_START, 0.0), s.feet),
            "camera 0",
        );
        assert!(
            (s.fov - guacamole::viewport::fov_y(68.0)).abs() < 1e-3,
            "{}",
            s.fov
        );
        assert_eq!(s.view.source, Source::Scripts);
        assert_eq!(s.report.scripts.as_deref(), Some("on"));
        assert_eq!(s.report.scripts_reason, None);
    }
    // The steps before the scripts drew said why the chain had it.
    if let Some(before) = all.iter().find(|s| s.hold != ViewHold::Scripts) {
        assert_eq!(before.hold, ViewHold::Chain(NOT_YET.to_owned()));
        assert_eq!(before.report.scripts.as_deref(), Some("none"));
    }
    let first = on.first().unwrap();
    assert!(
        on.iter()
            .all(|s| s.eye.distance(first.eye) < 1e-4 && s.feet.distance(first.feet) < 1e-4),
        "held still, and him with it"
    );
}

/// AG.28: across the plane and back. Walking at full stick through the
/// transition plane's box, the view blends its two cameras by where his
/// feet were the frame before (3G² - 2G³ on the camera behind it, G 0.5
/// plus his feet's distance ahead of the box's middle over its length, held
/// to [0, 1]), the far clip with it; let go part way, it holds where it is;
/// walked on out, it's the camera behind; walked back across, the first
/// again.
#[test]
fn a_plane_crossed_and_back_blends_by_his_feet_the_frame_before() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, pad) = camera_app(tmp.path(), CameraFault::None);
    run_until(&mut app, "the scripts' view", 30, |s| {
        s.hold == ViewHold::Scripts
    });
    let middle = spots().plane[2];
    let length = CAMERA_PLANE_SIZE[2];
    let weight = |feet: Vec3| {
        let ahead = (feet.z - middle) / FT;
        let g = (0.5 + ahead / length).clamp(0.0, 1.0);
        3.0 * g * g - 2.0 * g * g * g
    };
    let from = samples(&app).len();
    // Stopped with his feet at the box's middle: the hold.
    test_pad::stick(&mut app, pad, 0.0, 1.0);
    run_until(&mut app, "his feet at the plane's middle", 120, |s| {
        s.feet.z >= middle
    });
    test_pad::stick(&mut app, pad, 0.0, 0.0);
    frames(&mut app, 10);
    let stood = samples(&app).len();
    frames(&mut app, 30);
    let held = &samples(&app)[stood..];
    let w = weight(held[0].feet);
    assert!((0.2..0.8).contains(&w), "part way: {w}");
    for s in held {
        assert!(
            s.eye.distance(held[0].eye) < 1e-5,
            "held at step {}",
            s.step
        );
        assert!((s.pct - w).abs() < 1e-4, "{} against {w}", s.pct);
    }
    // Out the far side onto the camera behind it, then back across onto
    // the first.
    test_pad::stick(&mut app, pad, 0.0, 1.0);
    run_until(&mut app, "out ahead of it", 120, |s| {
        s.feet.z > middle + (length / 2.0 + 3.0) * FT
    });
    let [.., before, out] = samples(&app) else {
        unreachable!()
    };
    let want = view_of(between(CAMERA_START, CAMERA_BACK, 1.0), before.feet);
    assert_view(out, want, "behind it");
    test_pad::stick(&mut app, pad, 0.0, -1.0);
    run_until(&mut app, "back behind it", 180, |s| {
        s.feet.z < middle - (length / 2.0 + 3.0) * FT
    });
    test_pad::stick(&mut app, pad, 0.0, 0.0);
    frames(&mut app, 3);
    let [.., before, back] = samples(&app) else {
        unreachable!()
    };
    let want = view_of(between(CAMERA_START, CAMERA_BACK, 0.0), before.feet);
    assert_view(back, want, "in front");
    // Every step: the view by his feet the step before, which his move
    // then left behind.
    let walked = &samples(&app)[from..];
    let mut blended = 0;
    let mut moved = 0;
    for (before, s) in pairs(walked) {
        let w = weight(before.feet);
        blended += usize::from((0.05..0.95).contains(&w));
        moved += usize::from(s.feet.distance(before.feet) > 0.02);
        assert_view(
            s,
            view_of(between(CAMERA_START, CAMERA_BACK, w), before.feet),
            "the blend",
        );
    }
    assert!(blended > 20, "{blended} steps part way across");
    assert!(moved > 60, "{moved} steps he moved in");
}

/// AG.28: a camera volume entered switches. Set down inside the volume, its
/// script (polling every 0.08 s) switches to the camera its id names over
/// the seconds its tilt holds: the share sliding 0 to 1 eased by 20 and 20,
/// the view that share of the way from where it stood to the volume's
/// camera, the far clip with it; then it stays on it.
#[test]
fn a_volume_entered_switches_over_its_time_and_ease() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = camera_app(tmp.path(), CameraFault::None);
    run_until(&mut app, "the scripts' view", 30, |s| {
        s.hold == ViewHold::Scripts
    });
    frames(&mut app, 20);
    assert!(
        samples(&app).iter().all(|s| s.pct == 1.0),
        "no switch outside it"
    );
    let inside = Vec3::from_array(spots().volume).with_y(START[1]);
    set_down(&mut app, inside);
    let from = samples(&app).len();
    let frames_long = (CAMERA_VOLUME_SECONDS * 60.0).round() as usize;
    frames(&mut app, frames_long + 30);
    let after = &samples(&app)[from..];
    let start = after
        .iter()
        .position(|s| s.pct < 1.0)
        .expect("a switch once he's inside");
    assert!(start <= 1, "the step he's inside: {start}");
    let ease = CAMERA_EASE as f32 * 0.02;
    for (j, s) in after[start..].iter().enumerate() {
        let want = eased(j as f32 / frames_long as f32, ease, ease);
        assert!(
            (s.pct - want).abs() < 1e-3,
            "{j} steps in: {} against {want}",
            s.pct
        );
        assert_view(
            s,
            view_of(between(CAMERA_START, CAMERA_VOLUME, s.pct), s.feet),
            "the switch",
        );
        assert_eq!(s.hold, ViewHold::Scripts);
    }
    assert_eq!(last(&app).pct, 1.0);
}

/// AG.28: the sliding rocks. Set down within the activator's range, it
/// wakes that step and opens the rock, whose shake moves the eye across the
/// view by `shake x` (2.5 times its magnitude, dying down over 0.5 s), the
/// view's turn untouched; two seconds after the wake the view switches to
/// the rocks' camera over two seconds eased by 20 and 20.
#[test]
fn the_sliding_rocks_switch_after_their_wake_and_shake_the_eye() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = camera_app(tmp.path(), CameraFault::None);
    run_until(&mut app, "the scripts' view", 30, |s| {
        s.hold == ViewHold::Scripts
    });
    frames(&mut app, 20);
    assert!(
        samples(&app)
            .iter()
            .all(|s| s.shake == 0.0 && s.activator.is_none()),
        "asleep 14 m off, not yet ticked"
    );
    let near = Vec3::from_array(spots().activator) + Vec3::new(0.0, START[1] - FLOOR, 5.0);
    assert!(near.distance(Vec3::from_array(spots().activator)) < CAMERA_ACTIVATOR_RANGE * FT);
    set_down(&mut app, near);
    let from = samples(&app).len();
    let switch = ((CAMERA_ACTIVATOR_DELAY + CAMERA_ACTIVATOR_SECONDS) * 60.0) as usize;
    frames(&mut app, switch + 30);
    let after = &samples(&app)[from..];
    let wake = after
        .iter()
        .position(|s| s.activator.as_deref() == Some("delay"))
        .expect("woken");
    assert_eq!(wake, 0, "woken the step he's in range");
    // The shake: across the view, its turn the unshaken view's.
    let shaking: Vec<(usize, &Sample)> = after
        .iter()
        .enumerate()
        .filter(|(_, s)| s.shake != 0.0)
        .collect();
    let (first, _) = shaking.first().expect("a shake");
    assert!(*first <= wake + 2, "the shake {first} steps after the wake");
    let peak = 2.5 * CAMERA_SHAKE;
    let shake_frames = (CAMERA_SHAKE_SECONDS * 60.0).round() as usize;
    assert!(shaking.len() + 1 >= shake_frames && shaking.len() <= shake_frames + 1);
    for (i, s) in &shaking {
        let want = peak * (1.0 - (i - first) as f32 / shake_frames as f32);
        assert!(
            (s.shake - want).abs() < 1e-3,
            "{i}: {} against {want}",
            s.shake
        );
        // The rock ticks after the camera's update in the walk: the step it
        // shakes, the update has already placed the eye; from the next, the
        // eye goes across by the step's `shake x`.
        let moved = if i == first { 0.0 } else { s.shake };
        let (eye, look, far) = view_of(between(CAMERA_START, CAMERA_START, 0.0), s.feet);
        let facing = look.x.atan2(look.z).to_degrees();
        let across = direction(facing - 90.0, 0.0) * moved * FT;
        assert_view(s, (eye + across, look, far), "the shake");
    }
    // The switch two seconds after the wake, over two.
    let start = after.iter().position(|s| s.pct < 1.0).expect("the switch");
    let delay = (CAMERA_ACTIVATOR_DELAY * 60.0) as usize;
    assert!(
        (delay..=delay + 1).contains(&(start - wake)),
        "the switch {} steps after the wake",
        start - wake
    );
    let long = (CAMERA_ACTIVATOR_SECONDS * 60.0) as usize;
    let ease = CAMERA_EASE as f32 * 0.02;
    for (j, s) in after[start..].iter().enumerate() {
        let want = eased(j as f32 / long as f32, ease, ease);
        assert!(
            (s.pct - want).abs() < 1e-3,
            "{j} steps in: {} against {want}",
            s.pct
        );
        assert_view(
            s,
            view_of(between(CAMERA_START, CAMERA_ROCKS, s.pct), s.feet),
            "to the rocks",
        );
    }
    assert_eq!(last(&app).pct, 1.0);
}

/// AG.28: the stick held through a switch. Held to his left the whole way,
/// he walks into the volume and the view moves to its camera; each step he
/// walks the stick's way against the view the step before showed (his
/// facing the viewport's then, plus the stick's 90°), so his walk turns
/// with the view.
#[test]
fn the_stick_held_through_a_switch_walks_against_the_view_the_step_before() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, pad) = camera_app(tmp.path(), CameraFault::None);
    run_until(&mut app, "the scripts' view", 30, |s| {
        s.hold == ViewHold::Scripts
    });
    let from = samples(&app).len();
    test_pad::stick(&mut app, pad, -1.0, 0.0);
    run_until(&mut app, "the switch", 120, |s| s.pct < 1.0);
    frames(&mut app, (CAMERA_VOLUME_SECONDS * 60.0) as usize + 10);
    test_pad::stick(&mut app, pad, 0.0, 0.0);
    let walked = &samples(&app)[from..];
    let switched = walked.iter().position(|s| s.pct < 1.0).unwrap();
    let facing_then = walked[switched].facing;
    assert!(
        (facing_then - 90.0).abs() < 1e-2,
        "his facing {facing_then} as it began"
    );
    let mut turning = 0;
    for (before, s) in pairs(walked).skip(2) {
        let want = (before.view.angles.facing + 90.0).rem_euclid(360.0);
        let got = s.facing.rem_euclid(360.0);
        let off = (got - want + 540.0).rem_euclid(360.0) - 180.0;
        assert!(
            off.abs() < 1e-2,
            "step {}: facing {got}, the view the step before {} + 90",
            s.step,
            before.view.angles.facing
        );
        // The view the step before, as the TV drew it.
        let drawn = before.look.x.atan2(before.look.z).to_degrees();
        let gap = (drawn - before.view.angles.facing + 540.0).rem_euclid(360.0) - 180.0;
        assert!(
            gap.abs() < 1e-2,
            "drawn {drawn}, the viewport's {}",
            before.view.angles.facing
        );
        turning += usize::from((0.0..1.0).contains(&s.pct) && s.pct > 0.0);
    }
    assert!(turning > 60, "{turning} steps walked while it switched");
    let end = walked.last().unwrap().facing.rem_euclid(360.0);
    assert!(
        (end - 180.0).abs() < 1e-2,
        "his facing {end} once it's done"
    );
}

/// AG.28: the camera's update stopped in play (a member nothing models)
/// hands the view to the TV's own camera from where it stood, for good:
/// the step it stops the eye and the way it looks unmoved, `/api/_test/tv`
/// saying `off` and why; walked on, the TV's camera follows him from there
/// at the distance the scripts left, and nothing the scripts write reaches
/// the viewport again.
#[test]
fn a_stopped_camera_script_hands_the_view_to_the_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, pad) = camera_app(tmp.path(), CameraFault::Frame);
    run_until(&mut app, "the scripts' view", 30, |s| {
        s.hold == ViewHold::Scripts
    });
    run_until(&mut app, "the stop", 200, |s| {
        matches!(s.hold, ViewHold::HandedBack(_))
    });
    let all = samples(&app);
    let at = all.len() - 1;
    let (before, stop) = (all[at - 1].clone(), all[at].clone());
    assert_eq!(before.hold, ViewHold::Scripts);
    assert!(
        stop.eye.distance(before.eye) < 1e-3,
        "the eye {} then {}",
        before.eye,
        stop.eye
    );
    assert!(stop.look.distance(before.look) < 1e-3);
    assert!((stop.far - before.far).abs() < 1e-2);
    assert_eq!(stop.report.scripts.as_deref(), Some("off"));
    let why = stop.report.scripts_reason.clone().unwrap_or_default();
    assert!(
        why.contains("the camera's frame stopped") && why.contains("unmodelled member"),
        "{why}"
    );
    assert_eq!(stop.view.source, Source::HandedBack);
    let kept = stop.view;
    // Walked on: the TV's camera follows him from there.
    test_pad::stick(&mut app, pad, 0.0, 1.0);
    frames(&mut app, 60);
    test_pad::stick(&mut app, pad, 0.0, 0.0);
    frames(&mut app, 120);
    let end = last(&app);
    assert!(matches!(end.hold, ViewHold::HandedBack(_)));
    assert!(end.feet.distance(stop.feet) > 1.0, "he walked");
    assert!(end.eye.distance(stop.eye) > 1.0, "it followed him");
    let gap = |s: &Sample| s.eye.distance(s.feet);
    assert!(
        (gap(&end) - gap(&stop)).abs() < 0.3,
        "{} m from his feet, {} when it stopped",
        gap(&end),
        gap(&stop)
    );
    assert_eq!(end.view.eye, kept.eye, "the scripts write nowhere");
}

/// AG.28: a camera volume's script stopped in play (a member nothing models,
/// the tick he's inside it, its switch never made) hands the view to the
/// TV's own camera from where it stood, as the update's stop does: the step
/// it stops the view unmoved, `/api/_test/tv` saying `off` with the volume's
/// stop for why, and nothing the scripts write reaching the viewport again,
/// though their update runs on.
#[test]
fn a_stopped_volume_script_hands_the_view_to_the_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _pad) = camera_app(tmp.path(), CameraFault::Volume);
    run_until(&mut app, "the scripts' view", 30, |s| {
        s.hold == ViewHold::Scripts
    });
    frames(&mut app, 10);
    let inside = Vec3::from_array(spots().volume).with_y(START[1]);
    set_down(&mut app, inside);
    run_until(&mut app, "the volume's stop", 60, |s| {
        matches!(s.hold, ViewHold::HandedBack(_))
    });
    let all = samples(&app);
    let at = all.len() - 1;
    let (before, stop) = (all[at - 1].clone(), all[at].clone());
    assert_eq!(before.hold, ViewHold::Scripts);
    // Set down 4 m off, he isn't under the view's line: the TV's camera
    // takes it where it stood and its first step closes on him by one
    // follow step, no jump.
    assert!(
        stop.eye.distance(before.eye) < 0.25,
        "the eye {} then {}",
        before.eye,
        stop.eye
    );
    assert!(stop.look.distance(before.look) < 0.02);
    assert_eq!(stop.report.scripts.as_deref(), Some("off"));
    let why = stop.report.scripts_reason.clone().unwrap_or_default();
    assert!(
        why.contains("a camera script stopped")
            && why.contains(CAMERA_VOLUME_NAME)
            && why.contains("unmodelled member"),
        "{why}"
    );
    assert_eq!(stop.view.source, Source::HandedBack);
    assert!(
        all.iter().all(|s| s.pct == 1.0),
        "the volume's switch never made"
    );
    let kept = stop.view;
    frames(&mut app, 120);
    let end = last(&app);
    assert!(matches!(end.hold, ViewHold::HandedBack(_)));
    assert_eq!(end.report.scripts.as_deref(), Some("off"));
    assert!(end.step > stop.step + 100, "the run went on");
    assert_eq!(end.view.eye, kept.eye, "the scripts write nowhere");
    assert!(
        end.eye.distance(end.feet) < stop.eye.distance(stop.feet),
        "the TV's camera closes on him where he stands"
    );
}

/// The TV on AG.28's synthetic camera with AG.27's synthetic walker beside
/// the fighter (openspec AG.190), the walker stopping as `fault` says: him
/// placed and taken in, his scripts joined. The app and the pad.
fn camera_walker_app(tmp: &Path, fault: WalkerFault) -> (App, Entity) {
    let (mut app, tx) = play_app_walkers(tmp, &[(WALKER_TOY, WALKER_LOAD)]);
    app.init_resource::<Samples>()
        .add_systems(FixedUpdate, record.in_set(Frame::Record));
    write_attack_parts(tmp);
    // His palette, Eruptor's under his name: what the join sets on his
    // minion's template.
    let rel = IrFormat::Ron.palette_path("009_Eruptor");
    let mut palette: Palette =
        guac_ir::from_ron(&std::fs::read_to_string(tmp.join(rel)).unwrap()).unwrap();
    palette.owner = WALKER_LOAD.into();
    std::fs::write(
        tmp.join(IrFormat::Ron.palette_path(WALKER_LOAD)),
        guac_ir::to_ron(&palette).unwrap(),
    )
    .unwrap();
    write_camera_with_walker(tmp, spots(), CameraFault::None, fault);
    tx.send(write_level(tmp, &lights())).unwrap();
    let pad = test_pad::connect(&mut app, None, None);
    until(&mut app, "the level and its scripts", |a| {
        level_state(a) == "ready" && a.world().contains_resource::<ScriptRuntime>()
    });
    place(&mut app, 0, WALKER_TOY, "Synthetic");
    until(&mut app, "the level", |a| mode(a) != Mode::Portal);
    test_pad::press(&mut app, pad, GamepadButton::East);
    until(&mut app, "his scripts", |a| {
        join(a).as_deref() == Some("joined")
    });
    (app, pad)
}

/// The step the view went to the TV's camera, and the one before: where it
/// stood when it went.
fn handed_back(app: &App) -> (Sample, Sample) {
    let all = samples(app);
    let at = all
        .iter()
        .position(|s| matches!(s.hold, ViewHold::HandedBack(_)))
        .expect("handed back");
    assert!(at > 0, "the scripts had the view first");
    (all[at - 1].clone(), all[at].clone())
}

/// Walked on with the stick held a second, then let go a second: the TV's
/// camera follows him from where the view went, at the distance the
/// scripts left, and nothing the scripts write reaches the viewport.
fn follows_him(app: &mut App, pad: Entity, stop: &Sample) {
    let kept = last(app).view;
    test_pad::stick(app, pad, 0.0, 1.0);
    frames(app, 60);
    test_pad::stick(app, pad, 0.0, 0.0);
    frames(app, 120);
    let end = last(app);
    assert!(matches!(end.hold, ViewHold::HandedBack(_)));
    assert_eq!(end.report.scripts.as_deref(), Some("off"));
    assert!(end.feet.distance(stop.feet) > 1.0, "he walked");
    assert!(end.eye.distance(stop.eye) > 1.0, "it followed him");
    let gap = |s: &Sample| s.eye.distance(s.feet);
    assert!(
        (gap(&end) - gap(stop)).abs() < 0.3,
        "{} m from his feet, {} when it went",
        gap(&end),
        gap(stop)
    );
    assert_eq!(end.view.eye, kept.eye, "the scripts write nowhere");
}

/// AG.190: a walker whose minion stops in play after the scripts have the
/// view falls back on AG.16's controller, and his fall-back hands the view
/// to the TV's camera as a camera script's stop does: nothing steps his
/// camera target off the player loop's joined run, so the chain takes the
/// view from where it stood, `/api/_test/tv` saying `off` with his reason,
/// and follows him as he walks on.
#[test]
fn a_walker_falling_back_hands_the_view_to_the_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, pad) = camera_walker_app(tmp.path(), WalkerFault::InPlay);
    run_until(&mut app, "his fall-back", 60, |s| {
        matches!(s.hold, ViewHold::HandedBack(_))
    });
    let (before, stop) = handed_back(&app);
    assert_eq!(before.hold, ViewHold::Scripts);
    assert_eq!(before.report.scripts.as_deref(), Some("on"));
    assert_eq!(join(&app).as_deref(), Some("off"));
    // He walks his scripts' 18.5 ft/s up to the step he stops: the chain's
    // first step closes on him by one follow step, no jump.
    assert!(
        stop.eye.distance(before.eye) < 0.25,
        "the eye {} then {}",
        before.eye,
        stop.eye
    );
    assert!(stop.look.distance(before.look) < 0.02);
    assert_eq!(stop.report.scripts.as_deref(), Some("off"));
    let why = stop.report.scripts_reason.clone().unwrap_or_default();
    assert!(
        why.contains("his scripts stopped") && why.contains("unmodelled member"),
        "{why}"
    );
    assert_eq!(stop.view.source, Source::HandedBack);
    follows_him(&mut app, pad, &stop);
}

/// AG.190 and AH.151 on Eruptor: his minion stopping in play, he falls back
/// on his measured table, the view goes to the TV's camera with his reason,
/// and the stick takes him to 21.5 ft/s though the part's mover update
/// writes him still each tick (his minion kept his walk for it, and keeps
/// it no more): its writes to his walk are dropped.
#[test]
fn eruptor_falling_back_walks_at_his_top_speed_and_hands_the_view_back() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, pad) = camera_app(tmp.path(), CameraFault::Hero);
    run_until(&mut app, "his fall-back", 200, |s| {
        matches!(s.hold, ViewHold::HandedBack(_))
    });
    let (before, stop) = handed_back(&app);
    assert_eq!(before.hold, ViewHold::Scripts);
    let why = stop.report.scripts_reason.clone().unwrap_or_default();
    assert!(
        why.contains("his scripts stopped") && why.contains("unmodelled member"),
        "{why}"
    );
    assert_eq!(join(&app).as_deref(), Some("off"));
    let e = hero_entity(&app);
    test_pad::stick(&mut app, pad, 0.0, 1.0);
    frames(&mut app, 30);
    let body = &app.world().entity(e).get::<HeroBody>().unwrap().hero;
    let walk = body.walk.velocity.length();
    assert!((walk - 21.5).abs() < 1e-3, "{walk} ft/s");
    assert_eq!(*body.tuning(), guac_motion::tuning::Tuning::ERUPTOR);
    let world = app.world().resource::<ScriptWorld>();
    assert!(world.walks_alone(e.to_bits()));
    let walk_writes: u32 = world
        .hero_writes()
        .iter()
        .filter(|((_, what), _)| *what == guacamole::scripts::HeroWrite::Walk)
        .map(|(_, n)| n)
        .sum();
    assert!(
        walk_writes >= 30,
        "the update's writes dropped: {walk_writes}"
    );
    test_pad::stick(&mut app, pad, 0.0, 0.0);
    follows_him(&mut app, pad, &stop);
}

/// AG.190: a swap mid-level isn't a fall-back. Eruptor taken off the portal
/// and put back leaves the scripts the view: no step hands it to the TV's
/// camera, and once he's joined again `/api/_test/tv` reads `on`.
#[test]
fn a_swap_mid_level_leaves_the_scripts_the_view() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, pad) = camera_app(tmp.path(), CameraFault::None);
    run_until(&mut app, "the scripts' view", 30, |s| {
        s.hold == ViewHold::Scripts
    });
    frames(&mut app, 30);
    clear(&mut app, 0);
    frames(&mut app, 30);
    place(&mut app, 1, ERUPTOR, "Eruptor");
    until(&mut app, "him back in", |a| {
        a.world().resource::<crate::play::Play>().hero().is_some()
    });
    test_pad::press(&mut app, pad, GamepadButton::East);
    until(&mut app, "his scripts again", |a| {
        join(a).as_deref() == Some("joined")
    });
    frames(&mut app, 30);
    assert!(
        samples(&app)
            .iter()
            .all(|s| !matches!(s.hold, ViewHold::HandedBack(_))),
        "never handed back"
    );
    let end = last(&app);
    assert_eq!(end.hold, ViewHold::Scripts);
    assert_eq!(end.report.scripts.as_deref(), Some("on"));
    assert_eq!(
        *app.world().resource::<crate::hero::CameraScripts>(),
        crate::hero::CameraScripts::On
    );
}

/// AG.191: the portal manager stopped by a stretch of its own that isn't
/// the camera's (its projectile manager, here) stops its tick for good, the
/// camera's frame in it: the view goes to the TV's camera from where it
/// stood, `/api/_test/tv` saying `off` with the portal manager's stop, and
/// the TV's camera follows him as he walks on (his own scripts running on).
#[test]
fn a_portal_manager_stopped_in_play_hands_the_view_to_the_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, pad) = camera_app(tmp.path(), CameraFault::PortalManager);
    run_until(&mut app, "the scripts' view", 30, |s| {
        s.hold == ViewHold::Scripts
    });
    run_until(&mut app, "the portal manager's stop", 200, |s| {
        matches!(s.hold, ViewHold::HandedBack(_))
    });
    let (before, stop) = handed_back(&app);
    assert_eq!(before.hold, ViewHold::Scripts);
    assert!(stop.eye.distance(before.eye) < 1e-3);
    let why = stop.report.scripts_reason.clone().unwrap_or_default();
    assert!(
        why.contains("the portal manager stopped") && why.contains("unmodelled member"),
        "{why}"
    );
    assert_eq!(stop.view.source, Source::HandedBack);
    let w = app.world();
    let vm = &w.resource::<ScriptRuntime>().vm;
    let pm = guacamole::scripts::portal_manager_placement(vm, w.resource::<ScriptWorld>())
        .expect("the portal manager");
    assert!(vm.stopped(pm).is_some(), "the portal manager stopped");
    assert_eq!(join(&app).as_deref(), Some("joined"), "his own run on");
    follows_him(&mut app, pad, &stop);
}
