//! Movement metrics (openspec AG.8, design decisions 5 and 7): the numbers a
//! walk's feel is measured by, read off a trace keyed on game time. The oracle's
//! side comes from an O5 [`Recording`] ([`Motion::from_recording`]); guacamole's
//! headless runner (AG.17) writes the same [`Motion`] at its fixed 1/60 s, so
//! one extractor reads both and AG.31 compares the numbers, never the frames.
//!
//! Game time is the only clock here: a long frame (a hitch) is one long step,
//! its distance counted at the speed that step moved at, so no metric depends
//! on how many flips or host seconds the stretch took.
//!
//! Within a step the hero moves in a straight line at one speed: the original
//! moves him by the velocity it has just updated times the step (AG.1), and a
//! fixed-step controller does the same. So a position between two samples is
//! interpolated linearly in time, and a speed is a step's distance over its
//! duration, never a derivative across samples. Ground means across the
//! ground, x and z; a heading is in degrees, θ facing (sin θ, 0, cos θ).
//!
//! The metrics, each a method on [`Motion`] over a stretch of game time the
//! caller names (from the script mapped onto the recording, [`script_times`]):
//! [`Motion::top_speed`], [`Motion::time_to_speed`], [`Motion::stop`],
//! [`Motion::reversal`], [`Motion::turn_rate`], [`Motion::wall_slide`],
//! [`Motion::trail`], [`Motion::settle`] and [`Motion::view_switch`].

use serde::{Deserialize, Serialize};

use crate::MetricError;
use crate::input::InputScript;
use crate::recording::{Alignment, Recording};

/// Where the camera is: its eye, the heading it looks along across the
/// ground and its pitch (degrees, negative looking down).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Eye {
    pub position: [f64; 3],
    pub heading: f64,
    pub pitch: f64,
}

/// The hero at one instant of game time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Game seconds from the trace's first sample.
    pub time: f64,
    /// His pivot, metres.
    pub position: [f64; 3],
    /// His facing, degrees.
    pub heading: f64,
    /// The stick that drives the step from this sample to the next, as the
    /// game holds it before its dead zone: x, -1 full left, and y, +1 full
    /// forward.
    pub stick: [f64; 2],
    pub camera: Option<Eye>,
}

/// A run of samples keyed on game time: times strictly increase and every
/// value is finite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Sample>", into = "Vec<Sample>")]
pub struct Motion {
    samples: Vec<Sample>,
}

/// A stretch of motion: how long it took and how far he went across the
/// ground meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    pub seconds: f64,
    pub distance: f64,
}

/// Distance over game time, both ways of measuring the distance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Speed {
    /// Across the ground, m/s.
    pub ground: f64,
    /// Along his path in 3D, m/s: the same on level ground, more on a slope.
    pub path: f64,
    pub seconds: f64,
}

/// Letting go: from the release to the step after which he no longer moves.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    /// The game time he came to rest at.
    pub at: f64,
    pub phase: Phase,
    /// The furthest he moved from where he came to rest, after it, metres.
    pub drift: f64,
}

/// A hard reversal, the stick swung to the other way: how he gets from one
/// way to the other. The original never stops for one (AG.8: from the next
/// frame he walks back at the speed he had while his facing comes round); a
/// walk that stops dead, turns where it stands and sets off again has a stop
/// and a turn as well.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Reversal {
    /// The least ground speed of a step in the window, m/s.
    pub least: f64,
    /// From the swing to the first sample whose facing is within the
    /// threshold of his facing at the window's end, game seconds.
    pub facing: f64,
    /// From the swing to the first step he doesn't move in; `None` when he
    /// never stops.
    pub stop: Option<Phase>,
    /// From there to the first step he moves in again; `None` when he never
    /// stops.
    pub turn: Option<Phase>,
    /// Degrees he turned while standing (signed, the way he turned); 0 when
    /// he never stops.
    pub turned: f64,
    /// Degrees of the whole change still to turn when he set off the new way
    /// (at the swing when he never stops).
    pub left: f64,
    /// From setting off the new way (the swing when he never stops) to the
    /// first step at the given share of top speed.
    pub go: Phase,
}

/// Turning, in degrees per game second, counted over the steps whose
/// heading changed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TurnRate {
    /// The whole turn, degrees, signed (positive toward higher headings).
    pub angle: f64,
    /// From the first turning step's start to the last's end.
    pub seconds: f64,
    /// The fastest single step, degrees per game second.
    pub peak: f64,
    /// The whole turn over its time.
    pub mean: f64,
    /// Steps he turned in.
    pub steps: usize,
}

/// A wall, as a plane: a point on it and its normal pointing to where the
/// hero is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub point: [f64; 3],
    pub normal: [f64; 3],
}

/// Against a wall: how fast he goes along it, how far from it he keeps and
/// whether it lifts him.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Slide {
    /// Along the wall across the ground, m/s.
    pub speed: f64,
    /// The pivot's least and greatest distance from the plane, metres.
    pub gap: [f64; 2],
    /// The most he rose above where the stretch began, metres.
    pub rise: f64,
}

/// The camera trailing the hero over a stretch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Trail {
    /// The trail angle furthest from 0 (signed), degrees: the hero's bearing
    /// from the camera across the ground less the camera's heading.
    pub angle: f64,
    /// When it was.
    pub at: f64,
    /// The eye's least and greatest distance from his pivot, metres.
    pub eye: [f64; 2],
}

/// The camera coming to rest after the hero stops.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Settle {
    /// Game seconds from the hero's stop to the camera's last move.
    pub seconds: f64,
    /// The trail angle and the eye's distance once it has.
    pub angle: f64,
    pub eye: f64,
}

/// A view switch: the stretch over which the camera's heading or pitch turns
/// by more than a threshold a step.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Switch {
    /// The game time its heading or pitch first moved.
    pub start: f64,
    /// From then to the end of the last step it moved in.
    pub length: f64,
    /// Heading and pitch before and after, degrees.
    pub before: [f64; 2],
    pub after: [f64; 2],
}

/// A script step's port-0 state change, placed on a recording.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScriptTime {
    /// The step's offset and the flip it acts in (`base` + offset).
    pub offset: u64,
    pub flip: u64,
    /// The game time of that flip's first record: the step it acts on starts
    /// there. `None` when the recording holds no record at that flip.
    pub time: Option<f64>,
    /// The game time of the first record from that flip on whose stick
    /// floats changed: the update the step drove starts at the record
    /// before it (`time`, when the two agree). `None` when no float changed.
    pub seen: Option<f64>,
}

/// Each degree difference wrapped to (-180, 180].
pub fn wrap(degrees: f64) -> f64 {
    let d = degrees.rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

/// The heading of a direction across the ground, degrees.
pub fn bearing(dx: f64, dz: f64) -> f64 {
    dx.atan2(dz).to_degrees()
}

fn ground(a: [f64; 3], b: [f64; 3]) -> f64 {
    (b[0] - a[0]).hypot(b[2] - a[2])
}

fn length(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt()
}

fn lerp(a: [f64; 3], b: [f64; 3], s: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * s,
        a[1] + (b[1] - a[1]) * s,
        a[2] + (b[2] - a[2]) * s,
    ]
}

impl Sample {
    fn floats(&self) -> impl Iterator<Item = f64> + '_ {
        let eye = self.camera.iter().flat_map(|e| {
            e.position
                .into_iter()
                .chain([e.heading, e.pitch])
                .collect::<Vec<_>>()
        });
        [self.time, self.heading]
            .into_iter()
            .chain(self.position)
            .chain(self.stick)
            .chain(eye)
    }
}

impl Motion {
    /// Refuses no samples, a time that doesn't increase and a value that
    /// isn't finite.
    pub fn new(samples: Vec<Sample>) -> Result<Self, MetricError> {
        if samples.is_empty() {
            return Err(MetricError::EmptyTrace);
        }
        for (index, s) in samples.iter().enumerate() {
            if let Some(component) = s.floats().position(|v| !v.is_finite()) {
                return Err(MetricError::NotFinite {
                    offset: index as u64,
                    component,
                });
            }
        }
        if let Some(index) = samples.windows(2).position(|w| w[1].time <= w[0].time) {
            return Err(MetricError::TimeOrder {
                index: index + 1,
                previous: samples[index].time,
                time: samples[index + 1].time,
            });
        }
        Ok(Self { samples })
    }

    /// An O5 recording as a motion: game time under AG.1's alignment (a
    /// record's own step moves the hero from its pose to the next record's),
    /// the hero's pivot and facing, the camera, and as each sample's stick
    /// the floats the NEXT record holds, since a record is taken as the pad
    /// read starts, before the game reads this frame's stick into them (the
    /// stick that moved him from record k to k + 1 is record k + 1's). The
    /// last sample keeps its own floats. Refused as the recording's game time
    /// is (a lost or repeated game frame).
    pub fn from_recording(recording: &Recording) -> Result<Self, MetricError> {
        let times = recording.game_time(Alignment::Earlier)?;
        let frames = recording.frames();
        let samples = frames
            .iter()
            .zip(&times)
            .enumerate()
            .map(|(k, (f, &time))| {
                let stick = frames.get(k + 1).unwrap_or(f).stick;
                Sample {
                    time,
                    position: f.hero.position.map(f64::from),
                    heading: f64::from(f.hero.rotation[1]),
                    stick: stick.map(f64::from),
                    camera: f.camera.map(|v| Eye {
                        position: v.position.map(f64::from),
                        heading: f64::from(v.rotation[1]),
                        pitch: f64::from(v.rotation[0]),
                    }),
                }
            })
            .collect();
        Motion::new(samples)
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// The game time the trace covers: every metric's window lies inside it.
    pub fn span(&self) -> (f64, f64) {
        (
            self.samples[0].time,
            self.samples[self.samples.len() - 1].time,
        )
    }

    /// The sample whose step holds `t`: the last one at or before it (the
    /// first before the trace starts, the last after it ends).
    pub fn index_at(&self, t: f64) -> usize {
        self.samples
            .partition_point(|s| s.time <= t)
            .saturating_sub(1)
    }

    /// Where he was at `t`, moving in a straight line through each step;
    /// held at the ends outside the trace.
    pub fn position_at(&self, t: f64) -> [f64; 3] {
        let k = self.index_at(t);
        let a = &self.samples[k];
        match self.samples.get(k + 1) {
            Some(b) if t > a.time => lerp(a.position, b.position, (t - a.time) / (b.time - a.time)),
            _ => a.position,
        }
    }

    /// His ground speed through step k (from sample k to k + 1), m/s.
    pub fn ground_speed(&self, k: usize) -> f64 {
        let (a, b) = (&self.samples[k], &self.samples[k + 1]);
        ground(a.position, b.position) / (b.time - a.time)
    }

    /// How far he went across the ground (or along his path in 3D) between
    /// two game times.
    pub fn distance(&self, from: f64, to: f64) -> f64 {
        self.path(from, to, ground)
    }

    fn path(&self, from: f64, to: f64, metric: fn([f64; 3], [f64; 3]) -> f64) -> f64 {
        if to <= from {
            return 0.0;
        }
        let mut sum = 0.0;
        let mut at = self.position_at(from);
        for s in &self.samples[self.index_at(from) + 1..] {
            if s.time >= to {
                break;
            }
            sum += metric(at, s.position);
            at = s.position;
        }
        sum + metric(at, self.position_at(to))
    }

    fn window(&self, from: f64, to: f64) -> Result<(usize, usize), MetricError> {
        let (first, last) = self.span();
        if !(from.is_finite() && to.is_finite()) || to <= from || from < first || to > last {
            return Err(MetricError::Window { from, to });
        }
        Ok((self.index_at(from), self.index_at(to)))
    }

    /// Top speed: distance per game second over a stretch he walks steadily
    /// (`from` once he's up to speed, `to` before he lets go).
    pub fn top_speed(&self, from: f64, to: f64) -> Result<Speed, MetricError> {
        self.window(from, to)?;
        let seconds = to - from;
        Ok(Speed {
            ground: self.path(from, to, ground) / seconds,
            path: self.path(from, to, length) / seconds,
            seconds,
        })
    }

    /// From a push at `push` (a sample's time: the start of the step the
    /// pushed stick drives), the game time until the first step he moves at
    /// `share` of `top` or faster, and the distance he covered by its start;
    /// `None` when no step before the trace's end does. A step moves at one
    /// speed, so the time is a step's start, never interpolated.
    pub fn time_to_speed(&self, push: f64, top: f64, share: f64) -> Option<Phase> {
        let k0 = self.samples.partition_point(|s| s.time < push);
        (k0..self.samples.len().saturating_sub(1))
            .find(|&k| self.ground_speed(k) >= share * top)
            .map(|k| {
                let at = self.samples[k].time;
                Phase {
                    seconds: at - push,
                    distance: self.distance(push, at),
                }
            })
    }

    /// Letting go at `release`: the first step from which every step up to
    /// `until` moves him `still` metres or less across the ground, the
    /// distance and time to it, and how far he drifts afterwards.
    pub fn stop(&self, release: f64, still: f64, until: f64) -> Result<Stop, MetricError> {
        let (k0, k1) = self.window(release, until)?;
        let moving = (k0..k1)
            .rev()
            .find(|&k| ground(self.samples[k].position, self.samples[k + 1].position) > still);
        let rest = moving.map_or(k0, |k| k + 1);
        if rest >= k1 {
            return Err(MetricError::NotStill {
                from: release,
                to: until,
            });
        }
        let at = self.samples[rest].time.max(release);
        let p = self.position_at(at);
        let drift = self.samples[rest..=k1]
            .iter()
            .map(|s| ground(p, s.position))
            .fold(0.0, f64::max);
        Ok(Stop {
            at,
            phase: Phase {
                seconds: at - release,
                distance: self.distance(release, at),
            },
            drift,
        })
    }

    /// A reversal swung at `swing`, over the window to `until`: the least
    /// speed of a step in it; the game time until his facing comes within
    /// `still_deg` of where it is at `until`; when he stops (a step he moves
    /// `still` or less in), the stop, the turn where he stands (to the first
    /// step he moves further in again, with the angle turned) and what's
    /// still to turn of the change to his heading at `until` when he sets off;
    /// and the go, from setting off the new way (the swing itself when he
    /// never stops, as the original's hero never does) to the first step at
    /// `share` of `top`.
    pub fn reversal(
        &self,
        swing: f64,
        top: f64,
        share: f64,
        still: f64,
        still_deg: f64,
        until: f64,
    ) -> Result<Reversal, MetricError> {
        let (k0, k1) = self.window(swing, until)?;
        let moved = |k: usize| ground(self.samples[k].position, self.samples[k + 1].position);
        let end = self.samples[k1].heading;
        let least = (k0..k1)
            .map(|k| self.ground_speed(k))
            .fold(f64::INFINITY, f64::min);
        // Sample k1 is within any threshold of itself, so one is found.
        let faced = (k0..=k1)
            .find(|&k| wrap(end - self.samples[k].heading).abs() <= still_deg)
            .unwrap_or(k1);
        let facing = (self.samples[faced].time - swing).max(0.0);
        let (stop, turn, turned, set_off, t_set) = match (k0..k1).find(|&k| moved(k) <= still) {
            None => (None, None, 0.0, k0, swing),
            Some(stopped) => {
                let off =
                    (stopped..k1)
                        .find(|&k| moved(k) > still)
                        .ok_or(MetricError::NotStill {
                            from: swing,
                            to: until,
                        })?;
                let (t_stop, t_off) = (self.samples[stopped].time, self.samples[off].time);
                let turned = (stopped..off)
                    .map(|k| wrap(self.samples[k + 1].heading - self.samples[k].heading))
                    .sum();
                let stop = Phase {
                    seconds: t_stop - swing,
                    distance: self.distance(swing, t_stop),
                };
                let turn = Phase {
                    seconds: t_off - t_stop,
                    distance: self.distance(t_stop, t_off),
                };
                (Some(stop), Some(turn), turned, off, t_off)
            }
        };
        let go = self
            .time_to_speed(t_set, top, share)
            .filter(|g| t_set + g.seconds <= until)
            .ok_or(MetricError::NotStill {
                from: t_set,
                to: until,
            })?;
        Ok(Reversal {
            least,
            facing,
            stop,
            turn,
            turned,
            left: wrap(end - self.samples[set_off].heading),
            go,
        })
    }

    /// His turning between two game times: every step whose heading changes
    /// by more than `still_deg` counts (a recorded heading wobbles by an f32
    /// ulp while he walks straight), its rate the change over the step's game
    /// time.
    pub fn turn_rate(&self, from: f64, to: f64, still_deg: f64) -> Result<TurnRate, MetricError> {
        let (k0, k1) = self.window(from, to)?;
        let mut out = TurnRate {
            angle: 0.0,
            seconds: 0.0,
            peak: 0.0,
            mean: 0.0,
            steps: 0,
        };
        let (mut first, mut last) = (None, None);
        for k in k0..k1 {
            let (a, b) = (&self.samples[k], &self.samples[k + 1]);
            let d = wrap(b.heading - a.heading);
            if d.abs() <= still_deg {
                continue;
            }
            out.angle += d;
            out.steps += 1;
            out.peak = out.peak.max(d.abs() / (b.time - a.time));
            first.get_or_insert(a.time);
            last = Some(b.time);
        }
        if let (Some(first), Some(last)) = (first, last) {
            out.seconds = last - first;
            out.mean = out.angle.abs() / out.seconds;
        }
        Ok(out)
    }

    /// Sliding along a wall between two game times: his ground speed along
    /// it (his ground path with its run toward the wall taken out), how close
    /// to and far from the plane his pivot kept, and how much he rose.
    pub fn wall_slide(&self, from: f64, to: f64, wall: Plane) -> Result<Slide, MetricError> {
        let (k0, k1) = self.window(from, to)?;
        let n = wall.normal;
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if !(len.is_finite() && len > 0.0) {
            return Err(MetricError::Window { from, to });
        }
        let n = n.map(|c| c / len);
        let (gn, gl) = ([n[0], n[2]], n[0].hypot(n[2]));
        let along = |a: [f64; 3], b: [f64; 3]| {
            let d = [b[0] - a[0], b[2] - a[2]];
            if gl == 0.0 {
                return d[0].hypot(d[1]);
            }
            let into = (d[0] * gn[0] + d[1] * gn[1]) / (gl * gl);
            (d[0] - into * gn[0]).hypot(d[1] - into * gn[1])
        };
        let gap = |p: [f64; 3]| {
            (p[0] - wall.point[0]) * n[0]
                + (p[1] - wall.point[1]) * n[1]
                + (p[2] - wall.point[2]) * n[2]
        };
        let start = self.position_at(from);
        let mut points = vec![start];
        points.extend(
            self.samples[k0 + 1..=k1]
                .iter()
                .take_while(|s| s.time < to)
                .map(|s| s.position),
        );
        points.push(self.position_at(to));
        let slid: f64 = points.windows(2).map(|w| along(w[0], w[1])).sum();
        let gaps: Vec<f64> = points.iter().map(|&p| gap(p)).collect();
        Ok(Slide {
            speed: slid / (to - from),
            gap: [
                gaps.iter().copied().fold(f64::INFINITY, f64::min),
                gaps.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ],
            rise: points.iter().map(|p| p[1] - start[1]).fold(0.0, f64::max),
        })
    }

    fn eye(&self, k: usize) -> Result<Eye, MetricError> {
        self.samples[k]
            .camera
            .ok_or(MetricError::NoCamera { index: k })
    }

    /// The trail angle at sample k: the hero's bearing from the camera across
    /// the ground less the camera's heading, degrees.
    pub fn trail_angle(&self, k: usize) -> Result<f64, MetricError> {
        let (s, e) = (&self.samples[k], self.eye(k)?);
        Ok(wrap(
            bearing(s.position[0] - e.position[0], s.position[2] - e.position[2]) - e.heading,
        ))
    }

    /// The eye's distance from his pivot at sample k, metres.
    pub fn eye_distance(&self, k: usize) -> Result<f64, MetricError> {
        Ok(length(self.eye(k)?.position, self.samples[k].position))
    }

    /// The camera trailing him over the samples between two game times.
    pub fn trail(&self, from: f64, to: f64) -> Result<Trail, MetricError> {
        let (k0, k1) = self.window(from, to)?;
        let first = if self.samples[k0].time < from {
            k0 + 1
        } else {
            k0
        };
        if first > k1 {
            return Err(MetricError::Window { from, to });
        }
        let mut out = Trail {
            angle: 0.0,
            at: self.samples[first].time,
            eye: [f64::INFINITY, f64::NEG_INFINITY],
        };
        for k in first..=k1 {
            let (angle, eye) = (self.trail_angle(k)?, self.eye_distance(k)?);
            if angle.abs() > out.angle.abs() {
                out.angle = angle;
                out.at = self.samples[k].time;
            }
            out.eye = [out.eye[0].min(eye), out.eye[1].max(eye)];
        }
        Ok(out)
    }

    /// From the hero's stop at `stop`, the game time until the camera's last
    /// step that moved its eye more than `still` metres or turned it more than
    /// `still_deg` before `until`, and where it rests.
    pub fn settle(
        &self,
        stop: f64,
        still: f64,
        still_deg: f64,
        until: f64,
    ) -> Result<Settle, MetricError> {
        let (k0, k1) = self.window(stop, until)?;
        let mut last = None;
        for k in k0..k1 {
            let (a, b) = (self.eye(k)?, self.eye(k + 1)?);
            if length(a.position, b.position) > still
                || wrap(b.heading - a.heading).abs() > still_deg
                || wrap(b.pitch - a.pitch).abs() > still_deg
            {
                last = Some(k + 1);
            }
        }
        let rest = last.unwrap_or(k0);
        if rest >= k1 && last.is_some() {
            return Err(MetricError::NotStill {
                from: stop,
                to: until,
            });
        }
        Ok(Settle {
            seconds: (self.samples[rest].time - stop).max(0.0),
            angle: self.trail_angle(k1)?,
            eye: self.eye_distance(k1)?,
        })
    }

    /// The camera's view changing between two game times: the first and last
    /// steps its heading or pitch turned more than `still_deg` in. `None`
    /// when it held still throughout.
    pub fn view_switch(
        &self,
        from: f64,
        to: f64,
        still_deg: f64,
    ) -> Result<Option<Switch>, MetricError> {
        let (k0, k1) = self.window(from, to)?;
        let mut moved = Vec::new();
        for k in k0..k1 {
            let (a, b) = (self.eye(k)?, self.eye(k + 1)?);
            if wrap(b.heading - a.heading).abs() > still_deg
                || (b.pitch - a.pitch).abs() > still_deg
            {
                moved.push(k);
            }
        }
        let (Some(&first), Some(&last)) = (moved.first(), moved.last()) else {
            return Ok(None);
        };
        let (before, after) = (self.eye(first)?, self.eye(last + 1)?);
        let start = self.samples[first].time;
        Ok(Some(Switch {
            start,
            length: self.samples[last + 1].time - start,
            before: [before.heading, before.pitch],
            after: [after.heading, after.pitch],
        }))
    }
}

/// Where each of a script's port-0 steps falls in a recording whose script
/// offset 0 is flip `base`: its flip's first record's game time, and the
/// first record from there on whose stick floats differ from the ones before
/// that flip (the floats show a step one record late, `Motion`'s doc says
/// why). The floats, not the flips, are what a metric reads the stick from;
/// this only places the script's steps, and a step whose `seen` isn't the
/// record after `time`'s says the flips and the floats disagree there.
pub fn script_times(
    recording: &Recording,
    script: &InputScript,
    base: u64,
) -> Result<Vec<ScriptTime>, MetricError> {
    let times = recording.game_time(Alignment::Earlier)?;
    let frames = recording.frames();
    let [port0, _] = script.entries();
    Ok(port0
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let flip = base + entry.at;
            let next = port0.get(i + 1).map_or(u64::MAX, |e| base + e.at);
            let at = frames.iter().position(|f| f.flip == flip && f.read == 0);
            let before = at.map(|k| frames[k].stick);
            let seen = at.and_then(|k| {
                (k + 1..frames.len())
                    .take_while(|&j| frames[j].flip <= next)
                    .find(|&j| Some(frames[j].stick) != before)
                    .map(|j| times[j])
            });
            ScriptTime {
                offset: entry.at,
                flip,
                time: at.map(|k| times[k]),
                seen,
            }
        })
        .collect())
}

impl TryFrom<Vec<Sample>> for Motion {
    type Error = MetricError;

    fn try_from(samples: Vec<Sample>) -> Result<Self, Self::Error> {
        Motion::new(samples)
    }
}

impl From<Motion> for Vec<Sample> {
    fn from(m: Motion) -> Self {
        m.samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Ds3Pad;
    use crate::recording::{Body, Clock, Frame};

    const STEP: f64 = 1.0 / 60.0;
    /// A long frame, clamped to three nominal frames.
    const HITCH: f64 = 0.05;

    /// A walk along `heading` from (10, 43, -93): step k lasts `dts[k]` and
    /// moves him at `speeds[k]`, turning him to `headings[k + 1]`.
    fn walk(heading: f64, dts: &[f64], speeds: &[f64]) -> Motion {
        let headings = vec![heading; dts.len() + 1];
        walk_turning(&headings, dts, speeds)
    }

    fn walk_turning(headings: &[f64], dts: &[f64], speeds: &[f64]) -> Motion {
        assert_eq!(
            (dts.len(), speeds.len() + 1),
            (speeds.len(), headings.len())
        );
        let mut p = [10.0, 43.0, -93.0];
        let mut t = 0.0;
        let mut samples = vec![sample(t, p, headings[0])];
        for k in 0..dts.len() {
            let h = headings[k + 1].to_radians();
            p = [
                p[0] + speeds[k] * dts[k] * h.sin(),
                p[1],
                p[2] + speeds[k] * dts[k] * h.cos(),
            ];
            t += dts[k];
            samples.push(sample(t, p, headings[k + 1]));
        }
        Motion::new(samples).unwrap()
    }

    fn sample(time: f64, position: [f64; 3], heading: f64) -> Sample {
        Sample {
            time,
            position,
            heading,
            stick: [0.0, 1.0],
            camera: None,
        }
    }

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} against {b} (tolerance {tol})");
    }

    #[test]
    fn top_speed_is_distance_over_game_time_a_hitch_included() {
        // 6 m/s throughout; the fourth step is a hitch, so its distance is
        // 0.3 m where a nominal step's is 0.1.
        let dts = [STEP, STEP, STEP, HITCH, STEP, STEP, STEP, STEP];
        let m = walk(15.0, &dts, &[6.0; 8]);
        let (t0, t1) = m.span();
        let s = m.top_speed(t0, t1).unwrap();
        close(s.ground, 6.0, 1e-12);
        close(s.path, 6.0, 1e-12);
        close(s.seconds, 7.0 / 60.0 + HITCH, 1e-15);
        // A window cutting steps in two: still 6 m/s, and 6 m a game second.
        let s = m.top_speed(0.025, 0.11).unwrap();
        close(s.ground, 6.0, 1e-12);
        close(m.distance(0.025, 0.11), 6.0 * 0.085, 1e-12);
        // Counted in flips the hitch would read 6 x (8 x 0.1 + 0.2) / (8 / 60) = 7.5 m/s.
        assert!(m.top_speed(t1, t1 + 1.0).is_err());
        assert!(m.top_speed(0.1, 0.05).is_err());
        assert!(m.top_speed(-1.0, 0.1).is_err());
    }

    #[test]
    fn on_a_slope_the_path_runs_further_than_the_ground() {
        let mut m = walk(0.0, &[STEP; 4], &[6.0; 4]);
        // Climbing 0.05 m a step: 3 in 4 across the ground, 5 in 4 along it.
        for (k, s) in m.samples.iter_mut().enumerate() {
            s.position[1] += 0.075 * k as f64;
        }
        let (t0, t1) = m.span();
        let s = m.top_speed(t0, t1).unwrap();
        close(s.ground, 6.0, 1e-12);
        close(s.path, 6.0 * 1.25, 1e-12);
    }

    #[test]
    fn time_to_speed_counts_from_the_push_to_the_first_fast_enough_step() {
        // At rest for two steps, pushed at the third sample, speeding up 1 m/s
        // a step with a hitch in the ramp, then at the 6 m/s top.
        let dts = [STEP, STEP, STEP, STEP, HITCH, STEP, STEP, STEP, STEP, STEP];
        let speeds = [0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 6.0, 6.0];
        let m = walk(15.0, &dts, &speeds);
        let push = m.samples()[2].time;
        // 90 % of 6 is 5.4: the step at 6 m/s, the eighth, starts at sample 7.
        let ninety = m.time_to_speed(push, 6.0, 0.9).unwrap();
        close(ninety.seconds, 4.0 * STEP + HITCH, 1e-12);
        close(
            ninety.distance,
            (1.0 + 2.0 + 4.0 + 5.0) * STEP + 3.0 * HITCH,
            1e-12,
        );
        // 45 % of top: the step at 3 m/s, which is the hitch.
        let half = m.time_to_speed(push, 6.0, 0.45).unwrap();
        close(half.seconds, 2.0 * STEP, 1e-12);
        // Never as fast as 7.
        assert!(m.time_to_speed(push, 7.0, 1.0).is_none());
        // A step exactly at the share counts (speeds exact in binary).
        let exact = walk(0.0, &[0.25; 3], &[2.0, 4.0, 4.0]);
        let at = exact.time_to_speed(0.0, 4.0, 1.0).unwrap();
        assert_eq!((at.seconds, at.distance), (0.25, 0.5));
        // The first 0.15 s of game time from the push: the ramp, then the top
        // speed for what's left of it.
        let ramp = 4.0 * STEP + HITCH;
        let ramp_m = (1.0 + 2.0 + 4.0 + 5.0) * STEP + 3.0 * HITCH;
        assert!(ramp < 0.15 && push + 0.15 <= m.span().1);
        close(
            m.distance(push, push + 0.15),
            ramp_m + 6.0 * (0.15 - ramp),
            1e-12,
        );
    }

    #[test]
    fn a_stop_is_the_distance_to_the_first_step_he_stays_still_from() {
        // Top speed, let go at sample 3: 4, then 2 m/s through a hitch, then
        // still bar a creep under the threshold.
        let dts = [STEP, STEP, STEP, STEP, HITCH, STEP, STEP, STEP, STEP];
        let speeds = [6.0, 6.0, 6.0, 4.0, 2.0, 0.0, 0.0, 0.0006 * 60.0, 0.0];
        let m = walk(105.0, &dts, &speeds);
        let release = m.samples()[3].time;
        let (_, end) = m.span();
        let s = m.stop(release, 1e-3, end).unwrap();
        close(s.at, m.samples()[5].time, 1e-15);
        close(s.phase.seconds, STEP + HITCH, 1e-12);
        close(s.phase.distance, 4.0 * STEP + 2.0 * HITCH, 1e-12);
        close(s.drift, 0.0006, 1e-9);
        // With a tighter threshold the creep counts as moving, and he comes to
        // rest after it.
        let tight = m.stop(release, 1e-4, end).unwrap();
        close(tight.at, m.samples()[8].time, 1e-15);
        // Still moving at the end: refused.
        let moving = walk(0.0, &[STEP; 4], &[6.0; 4]);
        let (t0, t1) = moving.span();
        assert_eq!(
            moving.stop(t0, 1e-3, t1),
            Err(MetricError::NotStill { from: t0, to: t1 })
        );
    }

    #[test]
    fn a_reversal_that_stops_dead_splits_into_its_stop_turn_and_go() {
        // Walking at 90 deg; swung at sample 3 to 270. He stops dead in the
        // swing's own step, turns 24 deg a step where he stands (a hitch turns
        // 72) until within 15 deg, creeping 0.05 mm in one of those steps (under
        // the 0.1 mm a step that counts as moving), then walks off speeding up
        // 2 m/s a step.
        let mut headings = vec![90.0, 90.0, 90.0, 90.0];
        let mut speeds = vec![6.0, 6.0, 6.0];
        let mut dts = vec![STEP, STEP, STEP];
        for (h, dt, v) in [
            (114.0, STEP, 0.0),
            (186.0, HITCH, 0.0),
            (210.0, STEP, 0.0),
            (234.0, STEP, 5e-5 / STEP),
            (258.0, STEP, 0.0),
        ] {
            headings.push(h);
            speeds.push(v);
            dts.push(dt);
        }
        for v in [2.0, 4.0, 6.0, 6.0, 6.0] {
            headings.push(270.0);
            speeds.push(v);
            dts.push(STEP);
        }
        let m = walk_turning(&headings, &dts, &speeds);
        let swing = m.samples()[3].time;
        let (_, end) = m.span();
        let r = m.reversal(swing, 6.0, 0.9, 1e-4, 1e-3, end).unwrap();
        assert_eq!(
            r.stop,
            Some(Phase {
                seconds: 0.0,
                distance: 0.0
            })
        );
        let turn = r.turn.unwrap();
        close(turn.seconds, 4.0 * STEP + HITCH, 1e-12);
        close(turn.distance, 5e-5, 1e-12);
        // 90 to 258 in place, 12 left once he sets off.
        close(r.turned, 168.0, 1e-9);
        close(r.left, 12.0, 1e-9);
        // Off at sample 8: 2 then 4 m/s, then 6 at sample 10.
        close(r.go.seconds, 2.0 * STEP, 1e-12);
        close(r.go.distance, 6.0 * STEP, 1e-12);
        // He stood: the least speed is 0, and he faces 270 from sample 9.
        close(r.least, 0.0, 1e-12);
        close(r.facing, 5.0 * STEP + HITCH, 1e-12);
    }

    #[test]
    fn a_reversal_at_speed_never_stops_while_his_facing_comes_round() {
        // The original's (AG.8): walking at 90 deg at 6 m/s (a hitch among the
        // steps), swung at sample 3 to 270. From the swing's own step he walks
        // along 270, at 5.5 m/s in that step and 6 after, while his facing
        // turns 24 deg a step the short way round, through 0, until it's on 270.
        let mut p = [10.0, 43.0, -93.0];
        let mut t = 0.0;
        let mut samples = vec![sample(t, p, 90.0)];
        let mut steps: Vec<(f64, f64, f64, f64)> = [STEP, HITCH, STEP]
            .into_iter()
            .map(|dt| (dt, 6.0, 90.0, 90.0))
            .collect();
        let turning = [
            66.0, 42.0, 18.0, -6.0, -30.0, -54.0, -78.0, -90.0, -90.0, -90.0,
        ];
        for (i, h) in turning.into_iter().enumerate() {
            steps.push((STEP, if i == 0 { 5.5 } else { 6.0 }, 270.0, h));
        }
        for (dt, v, walk, facing) in steps {
            let w = walk.to_radians();
            p = [p[0] + v * dt * w.sin(), p[1], p[2] + v * dt * w.cos()];
            t += dt;
            samples.push(sample(t, p, facing));
        }
        let m = Motion::new(samples).unwrap();
        let swing = m.samples()[3].time;
        let (_, end) = m.span();
        let r = m.reversal(swing, 6.0, 0.9, 1e-4, 1e-3, end).unwrap();
        assert_eq!((r.stop, r.turn, r.turned), (None, None, 0.0));
        close(r.least, 5.5, 1e-9);
        // On 270 (-90) at sample 11, eight steps after the swing.
        close(r.facing, 8.0 * STEP, 1e-12);
        // The whole half turn still to go at the swing.
        close(r.left.abs(), 180.0, 1e-9);
        // Already at 90 % of top in the swing's own step; 99 % a step on.
        assert_eq!(
            r.go,
            Phase {
                seconds: 0.0,
                distance: 0.0
            }
        );
        let near = m.reversal(swing, 6.0, 0.99, 1e-4, 1e-3, end).unwrap();
        close(near.go.seconds, STEP, 1e-12);
        close(near.go.distance, 5.5 * STEP, 1e-12);
        // A window that ends before he's up to that share: refused.
        let early = m.samples()[4].time - STEP / 2.0;
        assert!(m.reversal(swing, 6.0, 0.99, 1e-4, 1e-3, early).is_err());
    }

    #[test]
    fn turn_rate_takes_each_steps_change_over_its_game_time() {
        // 24 deg in each nominal step (1440 deg/s), 72 in a hitch (1440 too),
        // then 9 deg in a nominal step (540), then straight.
        let headings = [15.0, 39.0, 63.0, 135.0, 144.0, 144.0, 144.0];
        let dts = [STEP, STEP, HITCH, STEP, STEP, STEP];
        let m = walk_turning(&headings, &dts, &[6.0; 6]);
        let (t0, t1) = m.span();
        let r = m.turn_rate(t0, t1, 1e-3).unwrap();
        close(r.angle, 129.0, 1e-9);
        close(r.peak, 1440.0, 1e-6);
        close(r.seconds, 3.0 * STEP + HITCH, 1e-12);
        close(r.mean, 129.0 / (3.0 * STEP + HITCH), 1e-9);
        assert_eq!(r.steps, 4);
        // Across north: 350 to 10 is +20, not -340.
        let m = walk_turning(&[350.0, 10.0, 10.0], &[STEP, STEP], &[6.0; 2]);
        let (t0, t1) = m.span();
        close(m.turn_rate(t0, t1, 1e-3).unwrap().angle, 20.0, 1e-9);
        // Straight: no turn, no rate; an ulp's wobble on a heading of 105
        // (7.6e-6 deg) isn't a turn, though with no threshold it is one.
        let m = walk_turning(&[105.0, 105.0, 105.000_007_6, 105.0], &[STEP; 3], &[6.0; 3]);
        let (t0, t1) = m.span();
        let r = m.turn_rate(t0, t1, 1e-3).unwrap();
        assert_eq!((r.angle, r.steps, r.peak), (0.0, 0, 0.0));
        assert_eq!(m.turn_rate(t0, t1, 0.0).unwrap().steps, 2);
    }

    #[test]
    fn a_wall_slide_measures_speed_along_it_the_gap_and_any_rise() {
        // A wall facing -x at x = 12; he runs in at 45 deg, then slides north
        // along it at 4 m/s with his pivot 0.8644 m off it, a hitch among the
        // slide's steps, and is bumped 2 mm up once.
        let wall = Plane {
            point: [12.0, 43.0, 0.0],
            normal: [-1.0, 0.0, 0.0],
        };
        let mut samples = vec![sample(0.0, [11.0, 44.5, -93.0], 45.0)];
        let mut t = 0.0;
        let mut p = [12.0 - 0.8644, 44.5, -92.9];
        for (k, dt) in [STEP, STEP, HITCH, STEP, STEP].into_iter().enumerate() {
            t += dt;
            if k > 0 {
                p[2] += 4.0 * dt;
            }
            let mut q = p;
            if k == 3 {
                q[1] += 0.002;
            }
            samples.push(sample(t, q, 45.0));
        }
        let m = Motion::new(samples).unwrap();
        let from = m.samples()[1].time;
        let (_, to) = m.span();
        let s = m.wall_slide(from, to, wall).unwrap();
        close(s.speed, 4.0, 1e-9);
        close(s.gap[0], 0.8644, 1e-9);
        close(s.gap[1], 0.8644, 1e-9);
        close(s.rise, 0.002, 1e-9);
        // From the run in: the gap closes from 1 m and the speed along the
        // wall counts only the run along it.
        let (t0, _) = m.span();
        let s = m.wall_slide(t0, to, wall).unwrap();
        close(s.gap[1], 1.0, 1e-9);
        close(s.speed, (0.1 + 4.0 * (HITCH + 3.0 * STEP)) / to, 1e-9);
        assert!(
            m.wall_slide(
                t0,
                to,
                Plane {
                    point: wall.point,
                    normal: [0.0; 3]
                }
            )
            .is_err()
        );
    }

    /// The camera 10 m behind and 8 m above a point that lags the hero by
    /// `lag` of the way, looking along 15 deg.
    fn with_camera(m: &Motion, lag: &[f64]) -> Motion {
        let h = 15f64.to_radians();
        let mut aim = m.samples()[0].position;
        let samples = m
            .samples()
            .iter()
            .zip(lag)
            .map(|(s, &l)| {
                aim = lerp(aim, s.position, 1.0 - l);
                Sample {
                    camera: Some(Eye {
                        position: [
                            aim[0] - 10.0 * h.sin(),
                            aim[1] + 8.0,
                            aim[2] - 10.0 * h.cos(),
                        ],
                        heading: 15.0,
                        pitch: -38.66,
                    }),
                    ..*s
                }
            })
            .collect();
        Motion::new(samples).unwrap()
    }

    #[test]
    fn the_trail_is_the_heros_bearing_off_the_cameras_heading_and_it_settles() {
        // He walks at 105 deg (to the camera's left) for 6 steps, a hitch
        // among them, then stops; the aim follows half way each step while he
        // walks, then closes all but 0.1005 m, then 0.1 and 0.0005 m and holds.
        let dts = [
            STEP, STEP, HITCH, STEP, STEP, STEP, STEP, STEP, STEP, STEP, STEP,
        ];
        let speeds = [6.0, 6.0, 6.0, 6.0, 6.0, 6.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let m = walk(105.0, &dts, &speeds);
        let mut lag = vec![0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
        let mut gaps = Vec::new();
        let mut aim = m.samples()[0].position;
        for (s, &l) in m.samples().iter().zip(&lag) {
            aim = lerp(aim, s.position, 1.0 - l);
        }
        let stop = m.samples()[6].position;
        let mut left = ground(aim, stop);
        for close_by in [left - 0.1005, 0.1, 0.0005] {
            gaps.push(close_by);
            lag.push((left - close_by) / left);
            left -= close_by;
        }
        lag.extend([1.0, 1.0]);
        let m = with_camera(&m, &lag);
        // At rest at the start: dead ahead, 12.8 m off.
        close(m.trail_angle(0).unwrap(), 0.0, 1e-9);
        close(m.eye_distance(0).unwrap(), 164f64.sqrt(), 1e-9);
        let (t0, _) = m.span();
        let walking = m.trail(t0, m.samples()[6].time).unwrap();
        // He's left of the camera's heading: a positive trail, and the eye
        // further off than at rest while it lags.
        assert!(walking.angle > 1.0, "{walking:?}");
        let k = m.index_at(walking.at);
        let s = &m.samples()[k];
        let e = s.camera.unwrap();
        close(
            walking.angle,
            bearing(s.position[0] - e.position[0], s.position[2] - e.position[2]) - 15.0,
            1e-9,
        );
        assert!(walking.eye[1] > 164f64.sqrt() && walking.eye[0] <= walking.eye[1]);
        // Settled: the last step the eye moved more than 1 mm is the 0.1 m
        // one, two steps after he stopped; then back dead ahead (the 0.5 mm
        // left over is under the threshold).
        let (_, end) = m.span();
        let settle = m.settle(m.samples()[6].time, 1e-3, 1e-3, end).unwrap();
        close(settle.seconds, 2.0 * STEP, 1e-12);
        assert!(settle.angle.abs() < 0.01, "{settle:?}");
        close(settle.eye, 164f64.sqrt(), 1e-3);
        // No camera: refused.
        let bare = walk(0.0, &[STEP; 2], &[6.0; 2]);
        assert_eq!(bare.trail_angle(1), Err(MetricError::NoCamera { index: 1 }));
    }

    #[test]
    fn a_view_switch_runs_from_the_first_to_the_last_step_the_view_turns_in() {
        // The camera holds 15 deg, then turns to 60 over six steps (a hitch
        // among them) and drops its pitch, then holds.
        let dts = [
            STEP, STEP, STEP, STEP, STEP, HITCH, STEP, STEP, STEP, STEP, STEP,
        ];
        let m = walk(0.0, &dts, &[0.0; 11]);
        let headings = [
            15.0, 15.0, 15.0, 15.0, 20.0, 30.0, 40.0, 50.0, 55.0, 60.0, 60.0, 60.0,
        ];
        let samples = m
            .samples()
            .iter()
            .zip(headings)
            .map(|(s, h)| Sample {
                camera: Some(Eye {
                    position: [0.0, 52.0, -107.0],
                    heading: h,
                    pitch: if h > 15.0 { -30.0 } else { -23.36 },
                }),
                ..*s
            })
            .collect();
        let m = Motion::new(samples).unwrap();
        let (t0, t1) = m.span();
        let s = m.view_switch(t0, t1, 1e-3).unwrap().unwrap();
        close(s.start, 3.0 * STEP, 1e-12);
        close(s.length, 5.0 * STEP + HITCH, 1e-12);
        assert_eq!((s.before, s.after), ([15.0, -23.36], [60.0, -30.0]));
        // Held still: no switch.
        assert_eq!(m.view_switch(m.samples()[9].time, t1, 1e-3), Ok(None));
        // The trail reads each sample's own heading: at 40 deg, the hero at
        // (10, -93) from the eye at (0, -107) bears 35.54 deg, 4.46 left of it.
        let k = 6;
        assert_eq!(m.samples()[k].camera.unwrap().heading, 40.0);
        close(m.trail_angle(k).unwrap(), bearing(10.0, 14.0) - 40.0, 1e-9);
    }

    fn frame(flip: u64, game_frame: u32, frame_delta: f32, z: f32, stick: [f32; 2]) -> Frame {
        Frame {
            flip,
            read: 0,
            host_us: flip * 16_667,
            game_frame,
            clock: Clock {
                frame_delta,
                presentation_rate: 1.0 / 60.0,
                logic_scale: 1.0,
                logic_stopped: false,
                object_scale: 1.0,
            },
            hero: Body {
                position: [10.0, 43.0, z],
                rotation: [0.0, 15.0, 0.0],
                velocity: [0.0; 3],
            },
            camera: None,
            stick,
            pad: Some(Ds3Pad::REST),
            clips: Vec::new(),
        }
    }

    #[test]
    fn a_recording_keys_on_game_time_and_takes_each_steps_stick_from_the_next_record() {
        // Pushed at flip 12's read: flip 12's record still holds the old
        // floats, flip 13's the new ones and the first step's move; a 0.2 s
        // frame at flip 13 is a 0.05 s step.
        let frames = vec![
            frame(10, 100, 0.0167, -93.0, [0.0, 0.0]),
            frame(11, 101, 0.0158, -93.0, [0.0, 0.0]),
            frame(12, 102, 0.0170, -93.0, [0.0, 0.0]),
            frame(13, 103, 0.2, -92.98, [0.0, 1.0]),
            frame(14, 104, 0.0165, -92.93, [0.0, 1.0]),
            frame(15, 105, 0.0166, -92.90, [0.0, 0.0]),
        ];
        let rec = Recording::new(frames).unwrap();
        let m = Motion::from_recording(&rec).unwrap();
        let t: Vec<f64> = m.samples().iter().map(|s| s.time).collect();
        let want = [0.0, 0.0167, 0.0325, 0.0495, 0.0995, 0.116];
        for (got, want) in t.iter().zip(want) {
            close(*got, want, 1e-6);
        }
        let sticks: Vec<[f64; 2]> = m.samples().iter().map(|s| s.stick).collect();
        assert_eq!(
            sticks,
            [
                [0.0, 0.0],
                [0.0, 0.0],
                [0.0, 1.0],
                [0.0, 1.0],
                [0.0, 0.0],
                [0.0, 0.0]
            ]
        );
        // The script: pushed at offset 2 (flip 12), let go at 4 (flip 14),
        // a step at offset 9 past the recording.
        let script = InputScript::from_ron(
            "(steps: [(at: 0, pads: {0: ()}), (at: 2, pads: {0: (left: (x: 128, y: 0))}),
              (at: 4, pads: {0: ()}), (at: 9, pads: {0: ()})])",
        )
        .unwrap();
        let placed = script_times(&rec, &script, 10).unwrap();
        assert_eq!(placed.len(), 4);
        assert_eq!((placed[1].offset, placed[1].flip), (2, 12));
        close(placed[1].time.unwrap(), t[2], 1e-12);
        close(placed[1].seen.unwrap(), t[3], 1e-12);
        close(placed[2].time.unwrap(), t[4], 1e-12);
        close(placed[2].seen.unwrap(), t[5], 1e-12);
        // At rest from the start: nothing changes before the push.
        assert_eq!(placed[0].seen, None);
        assert_eq!((placed[3].time, placed[3].seen), (None, None));
        // A ring that wrapped can start on a flip's second read: a step at
        // that flip has no first read to act at, so it isn't placed.
        let mut wrapped = rec.frames()[2..].to_vec();
        wrapped[0].read = 1;
        let wrapped = Recording::new(wrapped).unwrap();
        let placed = script_times(&wrapped, &script, 10).unwrap();
        assert_eq!((placed[1].flip, placed[1].time), (12, None));
        assert!(placed[2].time.is_some());
        // A lost game frame can't be keyed.
        let mut lost = rec.frames().to_vec();
        lost[4].game_frame += 1;
        lost[5].game_frame += 1;
        let lost = Recording::new(lost).unwrap();
        assert!(matches!(
            Motion::from_recording(&lost),
            Err(MetricError::FrameGap { .. })
        ));
    }

    #[test]
    fn a_motion_refuses_disordered_times_and_values_that_arent_numbers() {
        assert_eq!(Motion::new(Vec::new()), Err(MetricError::EmptyTrace));
        let m = walk(0.0, &[STEP; 3], &[6.0; 3]);
        let mut back: Vec<Sample> = m.clone().into();
        back[2].time = back[1].time;
        assert!(matches!(
            Motion::new(back),
            Err(MetricError::TimeOrder { index: 2, .. })
        ));
        let mut nan: Vec<Sample> = m.clone().into();
        nan[1].stick[0] = f64::NAN;
        assert!(matches!(
            Motion::new(nan),
            Err(MetricError::NotFinite { offset: 1, .. })
        ));
        let mut eye: Vec<Sample> = m.clone().into();
        eye[3].camera = Some(Eye {
            position: [0.0, f64::INFINITY, 0.0],
            heading: 15.0,
            pitch: -20.0,
        });
        assert!(Motion::new(eye).is_err());
        // JSON both ways (serde_json's float parse is within an ulp), and the
        // same checks on the way in.
        let text = serde_json::to_string(&m).unwrap();
        let back: Motion = serde_json::from_str(&text).unwrap();
        for (a, b) in back.samples().iter().zip(m.samples()) {
            close(a.time, b.time, 1e-12);
            close(a.position[2], b.position[2], 1e-12);
        }
        let mut swapped: Vec<Sample> = m.clone().into();
        swapped.swap(0, 1);
        let text = serde_json::to_string(&swapped).unwrap();
        assert!(serde_json::from_str::<Motion>(&text).is_err());
        // Interpolation and the helpers at the ends.
        close(m.position_at(-1.0)[2], -93.0, 0.0);
        close(
            m.position_at(STEP * 1.5)[2],
            -93.0 + 6.0 * STEP * 1.5,
            1e-12,
        );
        assert_eq!(m.index_at(10.0), 3);
        close(wrap(190.0), -170.0, 1e-12);
        close(wrap(-180.0), 180.0, 1e-12);
        close(bearing(1.0, 0.0), 90.0, 1e-12);
    }
}
