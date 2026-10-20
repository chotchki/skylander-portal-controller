//! The controller run free from each recording's start (openspec AG.16).

mod oracle;

use guac_motion::hero::{Hero, Input};
use guac_motion::parry::ParryWorld;
use guac_motion::seam::Vec3;
use guac_motion::solid::euler;
use guac_motion::stick::Stick;
use guac_motion::tuning::{FOOT, Tuning};
use guac_motion::walk::Walk;
use guac_oracle::recording::Frame;
use oracle::*;

#[derive(Debug, Clone)]
struct Step {
    k: usize,
    pivot: Vec3,
    velocity: Vec3,
    walk: Vec3,
    supported: bool,
    held_off: bool,
}

fn start(frames: &[Frame], s: usize) -> Hero {
    let f = &frames[s];
    let mut hero = Hero::new(
        eruptor_solid(),
        Tuning::ERUPTOR,
        Vec3::from(f.hero.position),
        f.hero.rotation[1],
    );
    hero.up = euler(f.hero.rotation) * Vec3::Y;
    hero.walk = Walk {
        velocity: f
            .mover
            .as_ref()
            .map_or(Vec3::from(f.hero.velocity) / FOOT, stored),
        landing: None,
    };
    hero.footing = footing(f).unwrap();
    hero.velocity = Vec3::from(f.hero.velocity);
    hero.held_off = f
        .mover
        .as_ref()
        .is_some_and(|m| field(m, "controller inactive") != 0.0);
    hero.set_steps(frames[s - 1].clock.step(), frames[s - 2].clock.step());
    hero
}

fn run(world: &ParryWorld, frames: &[Frame], s: usize) -> Vec<Step> {
    let mut hero = start(frames, s);
    let mut out = Vec::new();
    for k in s + 1..frames.len() {
        let (a, b) = (&frames[k - 1], &frames[k]);
        if b.game_frame != a.game_frame.wrapping_add(1) {
            break;
        }
        let view = a.camera.map_or(0.0, |c| c.rotation[1]);
        let input = Input::stick(Stick::new(b.stick[0], b.stick[1]), view);
        hero.step(world, &input, a.clock.step(), &[]);
        out.push(Step {
            k,
            pivot: hero.pivot,
            velocity: hero.velocity,
            walk: hero.walk.velocity,
            supported: hero.footing.supported,
            held_off: hero.held_off,
        });
    }
    out
}

fn describe(f: &Frame) -> String {
    let c = f.contacts.as_ref();
    let plane = c.and_then(|c| c.planes.first()).map(|p| {
        format!(
            "plane d {:.4} n ({:.3},{:.3},{:.3}) col {}",
            p.distance, p.normal[0], p.normal[1], p.normal[2], p.collider
        )
    });
    let ground = c.and_then(|c| c.ground.as_ref()).map(|g| {
        format!(
            "h {:.4} gn ({:.3},{:.3},{:.3})",
            f.hero.position[1] - g.location[1],
            g.normal[0],
            g.normal[1],
            g.normal[2]
        )
    });
    format!(
        "support {:?} {} {} stick {:?} inactive {:?}",
        c.map(|c| c.support),
        plane.unwrap_or_default(),
        ground.unwrap_or_default(),
        f.stick,
        f.mover.as_ref().map(|m| field(m, "controller inactive"))
    )
}

/// What theirs did over a record: its displacement over the step that
/// moved it there, m/s.
fn moved(frames: &[Frame], k: usize) -> Vec3 {
    let (a, b) = (&frames[k - 1], &frames[k]);
    (Vec3::from(b.hero.position) - Vec3::from(a.hero.position)) / a.clock.step()
}

fn survey(run_name: &str) {
    let world = ParryWorld::new(&level_213()).unwrap();
    for (name, frames) in recordings(run_name) {
        let steps = run(&world, &frames, 2);
        let (mut mm, mut cm, mut dv_ok, mut dw_ok, mut walks) = (0, 0, 0, 0, 0);
        let (mut first_mm, mut first_cm) = (None, None);
        let mut worst = 0.0f32;
        for s in &steps {
            let b = &frames[s.k];
            let dp = (s.pivot - Vec3::from(b.hero.position)).length();
            worst = worst.max(dp);
            if dp <= 1e-3 {
                mm += 1;
            } else if first_mm.is_none() {
                first_mm = Some(s.k);
            }
            if dp <= 1e-2 {
                cm += 1;
            } else if first_cm.is_none() {
                first_cm = Some(s.k);
            }
            if (s.velocity - moved(&frames, s.k)).length() <= 0.01 {
                dv_ok += 1;
            }
            if let Some(m) = &b.mover {
                walks += 1;
                if (s.walk - stored(m)).length() <= 0.032 {
                    dw_ok += 1;
                }
            }
        }
        eprintln!(
            "{name}: {} steps; pos 1 mm {mm} (first off {first_mm:?}), 1 cm {cm} (first off {first_cm:?}), worst {worst:.4} m; moved within 0.01 m/s {dv_ok}; walk within 0.032 ft/s {dw_ok} of {walks}",
            steps.len()
        );
        if std::env::var("REPLAY_VERBOSE").is_ok()
            && let Some(k) = first_cm
        {
            let s = steps.iter().find(|s| s.k == k).unwrap();
            let (a, b) = (&frames[k - 1], &frames[k]);
            eprintln!(
                "  at {k}: ours {:?} v {:?} walk {:?} sup {} held {}; theirs {:?} moved {:?} walk {:?}",
                s.pivot,
                s.velocity,
                s.walk,
                s.supported,
                s.held_off,
                b.hero.position,
                moved(&frames, k),
                b.mover.as_ref().map(stored)
            );
            eprintln!("  a: {}", describe(a));
            eprintln!("  b: {}", describe(b));
        }
    }
}

#[test]
#[ignore]
fn survey_ag38() {
    survey("ag38-switch-1792509065");
}

#[test]
#[ignore]
fn survey_ag39() {
    survey("ag39-falls-1792502813");
}

#[test]
#[ignore]
fn survey_ag9() {
    survey("ag9-contacts-1792471160");
    survey("ag9-contacts-1792472400");
}

#[test]
#[ignore]
fn survey_ag36() {
    survey("ag36-edges-1792474678");
}

/// REPLAY_DEBUG="run|set/replay|from|to" prints every step between.
#[test]
#[ignore]
fn debug_one() {
    let spec = std::env::var("REPLAY_DEBUG").unwrap();
    let parts: Vec<&str> = spec.split('|').collect();
    let (run_name, name, from, to) = (
        parts[0],
        parts[1],
        parts[2].parse::<usize>().unwrap(),
        parts[3].parse::<usize>().unwrap(),
    );
    let world = ParryWorld::new(&level_213()).unwrap();
    let frames = recordings(run_name).remove(name).unwrap();
    let mut hero = start(&frames, 2);
    for k in 3..=to.min(frames.len() - 1) {
        let (a, b) = (&frames[k - 1], &frames[k]);
        let view = a.camera.map_or(0.0, |c| c.rotation[1]);
        let input = Input::stick(Stick::new(b.stick[0], b.stick[1]), view);
        let before = hero.clone();
        hero.step(&world, &input, a.clock.step(), &[]);
        if k >= from {
            let planes = guac_motion::proxy::Proxy {
                seam: &world,
                shape: hero.solid().grown(0.0),
                centre: hero.solid().centre,
                rotation: hero.solid().rotation,
                exclude: &[],
            }
            .planes(hero.pivot, hero.up, 0.1);
            eprintln!(
                "{k}: ours p {:?} v {:?} walk {:?} foot {:?} up {:?} held {}",
                hero.pivot, hero.velocity, hero.walk, hero.footing, hero.up, hero.held_off
            );
            for p in planes.iter().take(4) {
                eprintln!(
                    "    plane d {:.5} n {:?} tri {:?}",
                    p.distance, p.normal, p.triangle
                );
            }
            eprintln!(
                "    theirs p {:?} v {:?} walk {:?} rot {:?}",
                b.hero.position,
                b.hero.velocity,
                b.mover.as_ref().map(stored),
                b.hero.rotation
            );
            eprintln!("    b: {}", describe(b));
            let _ = before;
        }
    }
}

/// REPLAY_TRIS="a,b,..." prints those triangles and who shares their edges.
#[test]
#[ignore]
fn debug_triangles() {
    use guac_motion::seam::Collider;
    let tris: Vec<usize> = std::env::var("REPLAY_TRIS")
        .unwrap()
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    let bodies = level_213();
    let Collider::Mesh(mesh) = &bodies[0].collider else {
        panic!()
    };
    let v = mesh.vertices();
    let t = mesh.triangles();
    for &i in &tris {
        let [a, b, c] = t[i];
        let (pa, pb, pc) = (v[a as usize], v[b as usize], v[c as usize]);
        let n = (pb - pa).cross(pc - pa).normalize();
        eprintln!(
            "tri {i}: {a} {b} {c} n {n:?} welding? mat {}",
            mesh.materials()[i]
        );
        eprintln!("   {pa:?} {pb:?} {pc:?}");
        for (e0, e1) in [(a, b), (b, c), (c, a)] {
            let shared: Vec<usize> = t
                .iter()
                .enumerate()
                .filter(|(j, tj)| *j != i && tj.contains(&e0) && tj.contains(&e1))
                .map(|(j, _)| j)
                .collect();
            // Same position, other index.
            let dup = |x: u32| {
                v.iter()
                    .enumerate()
                    .filter(|(j, p)| *j as u32 != x && (**p - v[x as usize]).length() < 1e-4)
                    .map(|(j, _)| j)
                    .collect::<Vec<_>>()
            };
            eprintln!(
                "   edge {e0}-{e1}: shared by {shared:?}; dups {:?} {:?}",
                dup(e0),
                dup(e1)
            );
        }
    }
}

/// Every record of AG.38's run stepped from the recorded record before:
/// where the controller's own frame parts from the original's.
#[test]
#[ignore]
fn one_step_ag38() {
    use std::collections::BTreeMap;
    let world = ParryWorld::new(&level_213()).unwrap();
    let mut table: BTreeMap<String, usize> = BTreeMap::new();
    let mut samples: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut pos_err = Vec::new();
    for (name, frames) in recordings("ag38-switch-1792509065") {
        for k in 4..frames.len() {
            if !consecutive(&frames[k - 4..=k]) {
                continue;
            }
            let (a, b) = (&frames[k - 1], &frames[k]);
            if a.mover.is_none() || b.mover.is_none() || a.contacts.is_none() {
                continue;
            }
            let mut hero = start(&frames, k - 1);
            let view = a.camera.map_or(0.0, |c| c.rotation[1]);
            let input = Input::stick(Stick::new(b.stick[0], b.stick[1]), view);
            hero.step(&world, &input, a.clock.step(), &[]);
            let theirs = footing(b).unwrap();
            let key = format!(
                "sup ours {} theirs {}",
                hero.footing.supported, theirs.supported
            );
            *table.entry(key.clone()).or_default() += 1;
            let dp = (hero.pivot - Vec3::from(b.hero.position)).length();
            pos_err.push(dp);
            if hero.footing.supported != theirs.supported {
                let list = samples.entry(key).or_default();
                if list.len() < 15 {
                    list.push(format!(
                        "{name} {k}: a h {:?} sup {} | b h {:?} | ours h {:?} dp {dp:.4} walk {:?} theirs {:?}\n      b {}",
                        footing(a).unwrap().height,
                        footing(a).unwrap().supported,
                        theirs.height,
                        hero.footing.height,
                        hero.walk.velocity,
                        stored(b.mover.as_ref().unwrap()),
                        describe(b)
                    ));
                }
            }
        }
    }
    pos_err.sort_by(f32::total_cmp);
    let q = |f: f64| pos_err[((pos_err.len() - 1) as f64 * f) as usize];
    eprintln!("{table:?}");
    eprintln!(
        "position after one step: n {} median {:.2e} p90 {:.2e} p99 {:.2e} max {:.2e}; within 1e-4 {}",
        pos_err.len(),
        q(0.5),
        q(0.9),
        q(0.99),
        q(1.0),
        pos_err.iter().filter(|&&d| d <= 1e-4).count()
    );
    for (k, v) in samples {
        eprintln!("{k}:");
        for s in v {
            eprintln!("  {s}");
        }
    }
}

/// Their support against the leave test and our planes at the end pose.
#[test]
#[ignore]
fn support_table_ag38() {
    use guac_motion::proxy::{LEAVE_NORMAL, LEAVE_SPEED, Proxy};
    use std::collections::BTreeMap;
    let world = ParryWorld::new(&level_213()).unwrap();
    let mut table: BTreeMap<(bool, bool, i32), [usize; 2]> = BTreeMap::new();
    for (_, frames) in recordings("ag38-switch-1792509065") {
        for k in 4..frames.len() {
            if !consecutive(&frames[k - 4..=k]) {
                continue;
            }
            let (a, b) = (&frames[k - 1], &frames[k]);
            let (Some(_), Some(mb), Some(fa)) = (&a.mover, &b.mover, footing(a)) else {
                continue;
            };
            let hero = start(&frames, k - 1);
            let v = stored(mb) * FOOT;
            let n = fa.ground.unwrap_or(Vec3::Y);
            let walking = fa.supported && n.y >= LEAVE_NORMAL && n.dot(v) <= LEAVE_SPEED;
            let left = fa.supported && !walking;
            let proxy = Proxy {
                seam: &world,
                shape: hero.solid().grown(0.0),
                centre: hero.solid().centre,
                rotation: hero.solid().rotation,
                exclude: &[],
            };
            let up = euler(b.hero.rotation) * Vec3::Y;
            let planes = proxy.planes(Vec3::from(b.hero.position), up, 0.05);
            let d = planes
                .iter()
                .filter(|p| p.normal.y > 0.7)
                .map(|p| p.distance)
                .fold(f32::INFINITY, f32::min);
            let bin = if d.is_finite() {
                (d * 1000.0).floor() as i32
            } else {
                99
            };
            let theirs = b.contacts.as_ref().unwrap().support == 2;
            table.entry((fa.supported, left, bin)).or_default()[usize::from(theirs)] += 1;
        }
    }
    eprintln!("(a supported, left, mm past skin at b's pose): [unsupported, supported]");
    for (k, v) in table {
        eprintln!("  {k:?}: {v:?}");
    }
}

/// REPLAY_RAYS="x,y,z" prints what rays straight down and up from there,
/// and down from 100 m over it, meet.
#[test]
#[ignore]
fn debug_rays() {
    use guac_motion::seam::PhysicsSeam;
    let p: Vec<f32> = std::env::var("REPLAY_RAYS")
        .unwrap()
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    let at = Vec3::new(p[0], p[1], p[2]);
    let world = ParryWorld::new(&level_213()).unwrap();
    eprintln!("down: {:?}", world.ray(at, Vec3::NEG_Y, 500.0, &[]));
    eprintln!("up: {:?}", world.ray(at, Vec3::Y, 500.0, &[]));
    eprintln!(
        "from 100 m up: {:?}",
        world.ray(at + Vec3::Y * 100.0, Vec3::NEG_Y, 500.0, &[])
    );
    for dz in [-0.6f32, -0.3, 0.0, 0.3, 0.6, 0.9] {
        let q = at + Vec3::new(0.0, 0.0, dz);
        eprintln!(
            "  z {:+.1}: down {:?} up {:?}",
            dz,
            world
                .ray(q, Vec3::NEG_Y, 500.0, &[])
                .map(|h| (h.point.y, h.triangle, h.normal)),
            world
                .ray(q, Vec3::Y, 500.0, &[])
                .map(|h| (h.point.y, h.triangle, h.normal))
        );
    }
}
