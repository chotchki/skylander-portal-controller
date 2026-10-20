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
    #[error("FLIP gave {count} values that aren't finite at {pixels_per_degree} pixels per degree")]
    FlipNotFinite {
        count: usize,
        pixels_per_degree: f32,
    },
    #[error("mean FLIP {index} must be finite and not negative, got {value}")]
    BadFlipMean { index: usize, value: f64 },
    #[error("this needs at least {needed} replays, got {found}")]
    TooFewReplays { needed: usize, found: usize },
    #[error("there's no replay {index} among {replays}")]
    NoSuchReplay { index: usize, replays: usize },
    #[error(
        "the record at flip {flip} is game frame {frame}, after {previous}: the recording lacks a frame and its step"
    )]
    FrameGap {
        flip: u64,
        previous: u32,
        frame: u32,
    },
    #[error(
        "the record at flip {flip} is game frame {frame} again: the port was read twice in one frame, so its step would count twice"
    )]
    FrameRepeat { flip: u64, frame: u32 },
    #[error(
        "the record at flip {flip} is the port's read {read} there, after flip {previous_flip}'s read {previous_read}: a record is missing between them"
    )]
    ReadOrder {
        previous_flip: u64,
        previous_read: u32,
        flip: u64,
        read: u32,
    },
    #[error("sample {index}'s game time {time} doesn't follow the one before it, {previous}")]
    TimeOrder {
        index: usize,
        previous: f64,
        time: f64,
    },
    #[error("game time {from} to {to} isn't a stretch inside the trace")]
    Window { from: f64, to: f64 },
    #[error("he doesn't stay still between game time {from} and {to}")]
    NotStill { from: f64, to: f64 },
    #[error("sample {index} has no camera")]
    NoCamera { index: usize },
    #[error("tick {index} is game frame {frame} after {previous}: a frame is missing")]
    TickGap {
        index: usize,
        previous: u32,
        frame: u32,
    },
    #[error("actor {id} is listed twice at frame {frame}")]
    DuplicateActor { frame: u32, id: u64 },
    #[error("actor {id} changes kind or placement at frame {frame}")]
    ActorChanged { frame: u32, id: u64 },
    #[error("actor {id} comes back at frame {frame} after leaving")]
    ActorReturned { frame: u32, id: u64 },
    #[error("chain record {index} doesn't ride the O5 record at its index")]
    ChainMismatch { index: usize },
    #[error("the item at {address:#010x} has no readable body at frame {frame}")]
    Unreadable { frame: u32, address: u32 },
    #[error("no {kind:?} behaviour is named for cell {cell:#x} (frame {frame})")]
    UnknownBehaviour { kind: String, cell: u32, frame: u32 },
    #[error("no clip is named for id {id} (frame {frame})")]
    UnknownClip { id: u32, frame: u32 },
    #[error("frame {frame} carries no generator words")]
    NoRng { frame: u32 },
    #[error("the generator at frame {frame} isn't within reach of F0's")]
    RngLost { frame: u32 },
    #[error("the generator at frame {frame} has drawn fewer than before")]
    RngBackwards { frame: u32 },
    #[error("{found} categories where {expected} were expected")]
    Categories { expected: usize, found: usize },
    #[error("probability {index} is {value}, not one a test can take")]
    BadProbability { index: usize, value: f64 },
    #[error("nothing to test: a side has no samples")]
    NoSamples,
}
