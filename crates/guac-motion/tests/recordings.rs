//! The controller's laws against the oracle's own recordings (openspec AG.16
//! on AG.38, AG.10 and AG.39). `#[ignore]`d: the recordings are guest memory
//! and stay outside the tree, under `GUAC_CAPTURE_DIR` (process env, else
//! the workspace's `.env.dev`), as `BLUS30968/<run>/<set>/<replay>/
//! recording.json`. Run with `cargo test -p guac-motion --test recordings --
//! --ignored`.
//!
//! Each test replays one law a record at a time from the record before, as
//! AG.38 did, so it checks the law and not the geometry (`replays.rs` runs
//! the whole controller over level 213's collision, AG.18 both sides of the
//! seam). Records pair by `_curFrame`. Steps:
//! the push and the facing take the step three records back, the slowing and
//! gravity the step two back (the movement script's "time since last update"
//! where the watch holds it, each record's being the step two back).

mod oracle;

use guac_motion::ground;
use guac_motion::seam::{BodyId, ContactPlane, Vec3};
use guac_motion::stick::Stick;
use guac_motion::tuning::{FOOT, Tuning};
use guac_motion::walk::{Walk, heading, turn_facing, wrap};
use guac_oracle::recording::Frame;
use oracle::{consecutive, field, footing, recordings, stored};

const T: Tuning = Tuning::ERUPTOR;

/// AG.38's (2026-10-20) replay of the stored walk, Face and Move then Update
/// Movers from the record before: within 0.032 ft/s on every record but the
/// edge probe's holds and the landings (whose slide has no law yet, AG.47),
/// over all 38 replays of `ag38-switch-1792509065`; the facing to 4.01e-3°
/// on level ground (AG.8's bound for its turn law, which it measured there).
#[test]
#[ignore]
fn ag38_the_stored_walk_replays_to_0_032_ft_s() {
    let (mut checked, mut held, mut landings) = (0usize, 0usize, 0usize);
    let (mut worst, mut worst_at) = (0.0f32, String::new());
    let (mut worst_facing, mut facing_at, mut facings) = (0.0f32, String::new(), 0);
    let mut over_hundredth = 0;
    for (name, frames) in recordings("ag38-switch-1792509065") {
        for k in 3..frames.len() {
            let window = &frames[k - 3..=k];
            if !consecutive(window) {
                continue;
            }
            let (a2, a, b) = (&frames[k - 2], &frames[k - 1], &frames[k]);
            let (Some(ma), Some(mb)) = (&a.mover, &b.mover) else {
                continue;
            };
            let (Some(fa), Some(fa2)) = (footing(a), footing(a2)) else {
                continue;
            };
            let Some(view) = a.camera.map(|c| c.rotation[1]) else {
                continue;
            };
            if field(ma, "controller inactive") != 0.0 || field(mb, "controller inactive") != 0.0 {
                held += 1;
                continue;
            }
            if fa.supported && fa2.high() {
                landings += 1;
                continue;
            }
            let push = Stick::new(b.stick[0], b.stick[1]).push(view);
            let (push_step, slow_step) = (
                field(ma, "time since last update"),
                field(mb, "time since last update"),
            );
            let mut walk = Walk {
                velocity: stored(ma),
                landing: None,
            };
            walk.face_and_move(push, &fa, &T, push_step);
            walk.update_movers(push.held(), &fa, &T, slow_step);
            let miss = (walk.velocity - stored(mb)).length();
            checked += 1;
            if miss > 0.01 {
                over_hundredth += 1;
            }
            if miss > worst {
                worst = miss;
                worst_at = format!("{name} record {k} flip {}: {walk:?} vs {mb:?}", b.flip);
            }
            // His facing is the heading of his matrix only while he's
            // upright: leaned, the Euler heading isn't the turn's.
            let level = [a, b]
                .iter()
                .all(|f| f.hero.rotation[0].abs() < 0.01 && f.hero.rotation[2].abs() < 0.01);
            if level {
                let facing = turn_facing(a.hero.rotation[1], push, T.turn_rate, push_step);
                let off = wrap(facing - b.hero.rotation[1]).abs();
                facings += 1;
                if off > worst_facing {
                    worst_facing = off;
                    facing_at = format!("{name} record {k}");
                }
            }
        }
    }
    eprintln!(
        "checked {checked}, held {held}, landings {landings}, over 0.01 ft/s {over_hundredth}, \
         worst {worst} ft/s at {worst_at}; facing on level ground {facings}, worst {worst_facing}° at {facing_at}"
    );
    assert!(checked > 15_000, "{checked}");
    assert!(worst <= 0.032, "{worst} ft/s at {worst_at}");
    assert!(worst_facing <= 4.01e-3, "{worst_facing}° at {facing_at}");
}

/// Free in the air: not supported, 0.4 ft or more up, and his manifold's
/// closest point (its first entry, the step's own; a step that finds nothing
/// leaves the last one standing) 2 cm or more off, as AG.10 chose them.
fn free(f: &Frame) -> bool {
    let Some(c) = &f.contacts else { return false };
    let up = footing(f).is_some_and(|ft| ft.high());
    up && c.planes.first().is_none_or(|p| p.distance >= 0.02)
}

/// AG.10's falls (2026-10-20 review): off the ground the body follows the
/// walk, so on every free record with two free ones before it the walk's
/// laws replayed from the record before (gravity 136.21 ft/s² over the step
/// two back, the steer 160 ft/s² x S over the step three back cut to the
/// larger of 21.5 x S and what it had, the let-go and the over-the-top
/// slowing at 50 ft/s²) give its velocity to 1.5e-3 m/s, over all 54 of
/// `ag39-falls-1792502813`.
#[test]
#[ignore]
fn ag39_free_falls_steer_and_slow_to_1_5e_3_m_s() {
    let (mut checked, mut worst, mut worst_at) = (0usize, 0.0f32, String::new());
    for (name, frames) in recordings("ag39-falls-1792502813") {
        for k in 3..frames.len() {
            if !consecutive(&frames[k - 3..=k]) || !frames[k - 2..=k].iter().all(free) {
                continue;
            }
            let (a, b) = (&frames[k - 1], &frames[k]);
            let Some(view) = a.camera.map(|c| c.rotation[1]) else {
                continue;
            };
            let fa = footing(a).unwrap();
            let push = Stick::new(b.stick[0], b.stick[1]).push(view);
            let (push_step, slow_step) = (frames[k - 3].clock.step(), frames[k - 2].clock.step());
            let mut walk = Walk {
                velocity: Vec3::from(a.hero.velocity) / FOOT,
                landing: None,
            };
            walk.face_and_move(push, &fa, &T, push_step);
            walk.update_movers(push.held(), &fa, &T, slow_step);
            let miss = (walk.velocity * FOOT - Vec3::from(b.hero.velocity)).length();
            checked += 1;
            if miss > worst {
                worst = miss;
                worst_at = format!("{name} record {k} flip {}", b.flip);
            }
        }
    }
    eprintln!("checked {checked}, worst {worst} m/s at {worst_at}");
    assert!(checked >= 240, "{checked}");
    assert!(worst <= 1.5e-3, "{worst} m/s at {worst_at}");
}

/// AG.39's cliff under the ledge route: a face of it holds him up to 44.92°
/// and none from 45.04°. Over every record whose manifold's own (fresh)
/// closest point is a face of 40°-50° within 2 cm, alone in it (AG.39's
/// count, 16 held and 37 not, was of those; fresh entries alone give 13 and
/// 44), the support check given
/// that plane, touching, holds him on every face the original held him on
/// and on none of 45.04° or more. (The original also let go of some under 45°
/// with the plane further off, the support's reach, a hypothesis.)
#[test]
#[ignore]
fn ag39_cliff_faces_hold_him_to_44_92_and_not_from_45_04() {
    let (mut held, mut steep, mut wrong) = (0, 0, Vec::new());
    for (name, frames) in recordings("ag39-falls-1792502813") {
        for k in 1..frames.len() {
            let (Some(c0), Some(c)) = (&frames[k - 1].contacts, &frames[k].contacts) else {
                continue;
            };
            let Some(first) = c.planes.first() else {
                continue;
            };
            if c0.planes.first() == Some(first) || first.distance >= 0.02 {
                continue;
            }
            let n = Vec3::from(first.normal);
            // That face alone: the cast's entry the same plane.
            let alone = c
                .planes
                .iter()
                .all(|p| ground::angle(Vec3::from(p.normal), n) < 0.5);
            if !alone {
                continue;
            }
            let slope = ground::angle(n, Vec3::Y);
            if !(40.0..50.0).contains(&slope) {
                continue;
            }
            let plane = ContactPlane {
                body: BodyId(1),
                triangle: None,
                material: None,
                normal: n,
                point: Vec3::from(first.position),
                distance: 0.0,
            };
            let ours = ground::support(&[plane]).supported;
            let theirs = c.support == 2;
            if theirs {
                held += 1;
                if !ours {
                    wrong.push(format!("{name} record {k}: held at {slope}°"));
                }
            }
            if slope >= 45.04 {
                steep += 1;
                if ours || theirs {
                    wrong.push(format!(
                        "{name} record {k}: {slope}° ours {ours} theirs {theirs}"
                    ));
                }
            }
        }
    }
    eprintln!("held {held}, at 45.04° or more {steep}");
    // 13 held and 44 at 45.04° or more by this selection (fresh entries only).
    assert!(held >= 13 && steep >= 37, "{held} {steep}");
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// AG.39's landings: with the stick held, the record after the proxy's
/// landing record runs straight down the slope's fall line, and the one
/// after it the stick's way. Which landings slide is AG.47's question: the
/// walk's law here slides him when his last frame off the ground began with
/// the walk falling past 30 ft/s (the decompile's), and this test counts how
/// many of AG.39's landings it calls as recorded, beside the band rule
/// AG.10's text gives (that frame began 0.4 ft or more up), over every
/// landing with the stick held but those into the cliff. The walk is
/// rebuilt from the body's velocity at the last free record, frame by frame
/// through the walk's laws to the landing. Where it slides him, the slide
/// runs along the recorded one within 1°; how fast is the decompile's 1.2 x
/// the fall's speed x the slope/90°, a hypothesis, so it's reported.
#[test]
#[ignore]
fn ag39_landings_slide_down_the_fall_line() {
    let (mut landings, mut slides, mut by_speed, mut by_band, mut cliff) = (0, 0, 0, 0, 0);
    let mut lines = Vec::new();
    for (name, frames) in recordings("ag39-falls-1792502813") {
        let Some(last_free) = (1..frames.len()).rev().find(|&k| free(&frames[k])) else {
            continue;
        };
        let Some(land) = (last_free + 1..frames.len() - 2)
            .find(|&k| footing(&frames[k]).is_some_and(|f| f.supported))
        else {
            continue;
        };
        if frames[land + 1].stick == [0.0, 0.0] || !consecutive(&frames[last_free - 2..=land + 1]) {
            continue;
        }
        // AG.10's landings are "not into the cliff": none with a face of it
        // within 2 cm or the watch's contacts lost about the landing.
        let into_the_cliff = frames[land - 2..=land + 1].iter().any(|f| {
            f.contacts.as_ref().is_none_or(|c| {
                c.planes.is_empty()
                    || c.planes
                        .first()
                        .is_some_and(|p| p.distance < 0.02 && p.normal[1] < 0.75)
            })
        });
        if into_the_cliff {
            cliff += 1;
            continue;
        }
        landings += 1;
        let mut walk = Walk {
            velocity: Vec3::from(frames[last_free].hero.velocity) / FOOT,
            landing: None,
        };
        for k in last_free + 1..=land + 1 {
            let (a, b) = (&frames[k - 1], &frames[k]);
            let view = a.camera.map_or(0.0, |c| c.rotation[1]);
            let push = Stick::new(b.stick[0], b.stick[1]).push(view);
            let fa = footing(a).unwrap();
            walk.face_and_move(push, &fa, &T, frames[k - 3].clock.step());
            walk.update_movers(push.held(), &fa, &T, frames[k - 2].clock.step());
            if k == land {
                // The walk as his last frame off the ground left it, before
                // the landing's own frame reads him supported.
                assert!(walk.landing.is_some() || walk.velocity.y >= -30.0 - 10.0);
            }
        }
        let ours = walk.velocity.length() > 0.0 && frames[land + 1].hero.velocity != [0.0; 3] && {
            let fall_line = footing(&frames[land]).unwrap().ground.unwrap();
            let downhill = heading(Vec3::new(fall_line.x, 0.0, fall_line.z)).unwrap();
            heading(walk.velocity).is_some_and(|t| wrap(downhill - t).abs() < 1.0)
        };
        let band = footing(&frames[land - 2]).is_some_and(|f| f.high());
        let fall_line = footing(&frames[land]).unwrap().ground.unwrap();
        let downhill = heading(Vec3::new(fall_line.x, 0.0, fall_line.z)).unwrap();
        let slide = Vec3::from(frames[land + 1].hero.velocity);
        let slid = heading(slide).is_some_and(|t| wrap(downhill - t).abs() < 1.0);
        by_speed += usize::from(ours == slid);
        by_band += usize::from(band == slid);
        slides += usize::from(slid);
        let mut line = format!("  {name}: land {land}, slid {slid}, ours {ours}, band {band}");
        if ours && slid {
            line += &format!(
                ", slide {:.2} m/s against {:.2}",
                walk.velocity.length() * FOOT,
                slide.length()
            );
        }
        lines.push(line);
    }
    eprintln!(
        "landings {landings} ({cliff} into the cliff left out), slid {slides}; \
         the 30 ft/s rule calls {by_speed}, the band {by_band}"
    );
    for line in &lines {
        eprintln!("{line}");
    }
    assert!(landings >= 26, "{landings}");
    assert_eq!(by_speed, landings, "the 30 ft/s rule");
}
