//! What the comparisons refuse: inputs whose shapes don't line up, values that
//! aren't numbers and tolerances that can't be one.

/// Everything a trace, envelope, event, totals or frame comparison refuses.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MetricError {
    #[error("a trace needs at least one sample")]
    EmptyTrace,
    #[error("the sample at offset {offset} has no components")]
    NoComponents { offset: u64 },
    #[error("offsets must increase: {offset} follows {previous}")]
    OffsetOrder { previous: u64, offset: u64 },
    #[error("the sample at offset {offset} has {found} components where the first has {expected}")]
    ComponentCount {
        offset: u64,
        expected: usize,
        found: usize,
    },
    #[error("component {component} at offset {offset} isn't finite")]
    NotFinite { offset: u64, component: usize },
    #[error("nothing to build a reference from: no replays")]
    NoReplays,
    #[error(
        "replay {replay} isn't sampled at replay 0's checkpoints (they differ at index {index})"
    )]
    CheckpointMismatch { replay: usize, index: usize },
    #[error("the candidate has no sample at checkpoint {offset}")]
    MissingCheckpoint { offset: u64 },
    #[error("nothing to compare: the envelope has no checkpoints")]
    NoCheckpoints,
    #[error("{what} must be finite and not negative, got {value}")]
    BadTolerance { what: &'static str, value: f64 },
    #[error("event {index} at offset {offset} comes before the one ahead of it, at {previous}")]
    EventOrder {
        index: usize,
        previous: u64,
        offset: u64,
    },
    #[error(
        "replay {replay} doesn't have the totals replay 0 has ({name:?} is in one and not the other)"
    )]
    TotalsMismatch { replay: usize, name: String },
    #[error("a {width}x{height} frame can't be compared (empty, or too big for FLIP's u32 sizes)")]
    FrameSize { width: u32, height: u32 },
    #[error("a {width}x{height} frame of {channels} channels takes {expected} bytes, got {found}")]
    FrameData {
        width: u32,
        height: u32,
        channels: usize,
        expected: usize,
        found: usize,
    },
    #[error("the frames differ in size: {reference:?} against {test:?}")]
    FrameMismatch {
        reference: (u32, u32),
        test: (u32, u32),
    },
    #[error("the mask has {found} entries for a map of {expected} pixels")]
    MaskSize { expected: usize, found: usize },
    #[error("pixels per degree must be finite and above 0, got {0}")]
    BadPixelsPerDegree(f32),
    #[error("a frame band needs at least two replays, got {0}")]
    TooFewFrames(usize),
}
