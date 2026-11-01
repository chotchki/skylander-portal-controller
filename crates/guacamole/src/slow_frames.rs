//! The slow-frame log (openspec AG.288): a playtest that stutters says why
//! in the log, every build. A frame over [`SLOW`] of the real clock is
//! logged at INFO with its number and length, the render pipelines compiled
//! in it ([`crate::warm::PipelineWatch`], AE.14's count), the scripts'
//! steps run in it and their time on the frame's thread (the step and the
//! engine's frame after it, [`ScriptLog`]) and the Mac's 1-minute load
//! average; once a minute a summary gives the frames drawn, their p99 and
//! the worst. Build-735d9d3's playtest logged no frame time at all, so its
//! stalls could only be lined up against the other lanes' load by the
//! clock: a slow frame that compiled nothing at a load of 20 reads as the
//! Mac's other work, one with a compile or a first spawn at a load of 5 as
//! the TV's own.

use std::time::Duration;

use bevy::prelude::*;
use tracing::info;

use crate::hero_scripts::ScriptLog;
use crate::warm::PipelineWatch;

/// A frame longer than this is logged.
pub(crate) const SLOW: Duration = Duration::from_millis(50);

/// A summary a minute: a playtest's notes and the lanes' load readings are
/// minutes apart, so a minute's p99 lines up with either.
pub(crate) const SUMMARY_EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SlowFrame {
    /// Frames since the TV started, from 1.
    pub frame: u64,
    pub took_ms: f64,
    /// Render pipelines compiled since the frame before.
    pub compiled: usize,
    /// The scripts' steps run in it, and their time on the frame's thread.
    pub script_steps: usize,
    pub script_ms: f64,
}

/// The last minute's frames, as logged.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Summary {
    pub frames: usize,
    pub p99_ms: f64,
    pub worst_ms: f64,
    pub slow: usize,
}

/// What the log counts across frames.
#[derive(Resource, Debug, Default)]
pub(crate) struct FrameWatch {
    frame: u64,
    /// The watch's running count as of the frame before, so a frame logs
    /// only its own compiles.
    compiled_before: Option<usize>,
    /// So a step is counted in one frame only.
    last_step: u64,
    /// The summary's minute: when it began and each frame's length, ms.
    since: Option<Duration>,
    lengths: Vec<f64>,
}

impl FrameWatch {
    /// One frame that took `took`, ending at `now` on the real clock, with
    /// the pipelines compiled so far (`None` with no watch) and the
    /// scripts' work since the last frame counted (steps and ms): the
    /// frame if it was slow, and the summary once a minute is up.
    pub(crate) fn frame(
        &mut self,
        now: Duration,
        took: Duration,
        compiled: Option<usize>,
        steps: (usize, f64),
    ) -> (Option<SlowFrame>, Option<Summary>) {
        self.frame += 1;
        let new = match (compiled, self.compiled_before) {
            (Some(now), Some(before)) => now.saturating_sub(before),
            _ => 0,
        };
        self.compiled_before = compiled.or(self.compiled_before);
        let took_ms = took.as_secs_f64() * 1e3;
        let since = *self.since.get_or_insert(now);
        self.lengths.push(took_ms);
        let slow = (took > SLOW).then_some(SlowFrame {
            frame: self.frame,
            took_ms,
            compiled: new,
            script_steps: steps.0,
            script_ms: steps.1,
        });
        let summary = (now.saturating_sub(since) >= SUMMARY_EVERY).then(|| {
            let mut sorted = std::mem::take(&mut self.lengths);
            sorted.sort_by(f64::total_cmp);
            self.since = Some(now);
            let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
            Summary {
                frames: sorted.len(),
                p99_ms: at(0.99),
                worst_ms: at(1.0),
                slow: sorted
                    .iter()
                    .filter(|ms| **ms > SLOW.as_secs_f64() * 1e3)
                    .count(),
            }
        });
        (slow, summary)
    }
}

/// The 1-minute load average, none where the OS gives none.
#[cfg(unix)]
pub(crate) fn load_1min() -> Option<f64> {
    let mut loads = [0.0f64; 3];
    // SAFETY: `getloadavg` writes at most `nelem` doubles into the buffer it
    // is handed; three fit.
    let n = unsafe { libc::getloadavg(loads.as_mut_ptr(), 3) };
    (n >= 1).then_some(loads[0])
}

#[cfg(not(unix))]
pub(crate) fn load_1min() -> Option<f64> {
    None
}

/// This frame against [`SLOW`], and the minute's summary when it's up.
fn watch_frames(
    time: Res<Time<Real>>,
    pipelines: Option<Res<PipelineWatch>>,
    log: Option<Res<ScriptLog>>,
    mut watch: ResMut<FrameWatch>,
) {
    let steps = match log.as_deref() {
        Some(log) => {
            let (last, n, ms) = log.work_since(watch.last_step);
            watch.last_step = last;
            (n, ms)
        }
        None => (0, 0.0),
    };
    let compiled = pipelines.map(|p| p.ready());
    let (slow, summary) = watch.frame(time.elapsed(), time.delta(), compiled, steps);
    if let Some(f) = slow {
        info!(
            frame = f.frame,
            took_ms = format_args!("{:.1}", f.took_ms),
            compiled = f.compiled,
            script_steps = f.script_steps,
            script_ms = format_args!("{:.1}", f.script_ms),
            load_1min = load_1min()
                .map(|l| format!("{l:.1}"))
                .as_deref()
                .unwrap_or("-"),
            "slow frame"
        );
    }
    if let Some(s) = summary {
        info!(
            frames = s.frames,
            p99_ms = format_args!("{:.1}", s.p99_ms),
            worst_ms = format_args!("{:.1}", s.worst_ms),
            slow = s.slow,
            load_1min = load_1min()
                .map(|l| format!("{l:.1}"))
                .as_deref()
                .unwrap_or("-"),
            "frames over the last minute"
        );
    }
}

/// The slow-frame log, in every build.
pub(crate) struct SlowFramesPlugin;

impl Plugin for SlowFramesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameWatch>()
            .add_systems(Last, watch_frames);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// AG.288: an 80 ms frame with two pipelines compiled in it and a
    /// script step is logged with every number; a 16 ms one isn't, nor a
    /// frame right at the limit.
    #[test]
    fn a_slow_frame_is_logged_with_its_compiles_and_steps() {
        let mut watch = FrameWatch::default();
        assert_eq!(
            watch.frame(16 * MS, 16 * MS, Some(10), (1, 2.0)),
            (None, None)
        );
        let (slow, summary) = watch.frame(96 * MS, 80 * MS, Some(12), (2, 7.5));
        assert_eq!(
            slow,
            Some(SlowFrame {
                frame: 2,
                took_ms: 80.0,
                compiled: 2,
                script_steps: 2,
                script_ms: 7.5,
            })
        );
        assert_eq!(summary, None);
        assert_eq!(watch.frame(146 * MS, SLOW, Some(12), (0, 0.0)).0, None);
        // A frame with no watch counts no compiles, nor takes the count's
        // place.
        let slow = watch.frame(250 * MS, 104 * MS, None, (0, 0.0)).0.unwrap();
        assert_eq!((slow.frame, slow.compiled), (4, 0));
        let slow = watch
            .frame(350 * MS, 100 * MS, Some(13), (0, 0.0))
            .0
            .unwrap();
        assert_eq!(slow.compiled, 1);
    }

    /// Once a minute: the frames drawn in it, their p99 and the worst, then
    /// a new minute.
    #[test]
    fn a_minutes_frames_are_summed_up() {
        let mut watch = FrameWatch::default();
        let mut now = Duration::ZERO;
        let mut summaries = Vec::new();
        for i in 0..4000u32 {
            let took = if i == 1000 { 120 * MS } else { 16 * MS };
            now += took;
            if let (_, Some(s)) = watch.frame(now, took, None, (0, 0.0)) {
                summaries.push((i, s));
            }
        }
        assert_eq!(summaries.len(), 1, "{summaries:?}");
        let (i, s) = &summaries[0];
        assert_eq!(s.worst_ms, 120.0);
        assert_eq!(s.p99_ms, 16.0);
        assert_eq!(s.slow, 1);
        assert_eq!(s.frames, *i as usize + 1);
        // A minute and a frame on from the first: 16 ms each but the one.
        assert!((3700..3800).contains(i), "{i}");
    }

    #[cfg(unix)]
    #[test]
    fn the_load_average_reads() {
        let load = load_1min().expect("the OS gives one");
        assert!(load.is_finite() && load >= 0.0, "{load}");
    }
}
