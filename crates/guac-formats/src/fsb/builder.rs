//! Synthetic FSB5 banks for tests and fuzz seeds (`test-fixtures`): any
//! version, codec word, rate and channel codes, chunk chains and names,
//! written as [`super`]'s layout says. No real bank is ever committed; the
//! game-magic guard fails on one.

use super::{DATA_ALIGN, HEADER_LEN_V0, HEADER_LEN_V1, MAGIC, chunk};

/// A synthetic sample: its packed codes, chunks, name and raw bytes.
#[derive(Debug, Clone, Default)]
pub struct SampleSpec {
    /// Index into [`super::RATES`] and [`super::CHANNELS`].
    pub rate_code: u8,
    pub channel_code: u8,
    pub samples: u32,
    /// Chunks as (kind, bytes), in order.
    pub chunks: Vec<(u8, Vec<u8>)>,
    pub name: Option<String>,
    pub data: Vec<u8>,
}

impl SampleSpec {
    pub fn new(data: Vec<u8>, samples: u32) -> Self {
        Self {
            rate_code: 8,
            channel_code: 1,
            samples,
            data,
            ..Self::default()
        }
    }

    pub fn named(mut self, name: &str) -> Self {
        self.name = Some(name.to_owned());
        self
    }

    pub fn looped(mut self, start: u32, end: u32) -> Self {
        let mut b = start.to_le_bytes().to_vec();
        b.extend(end.to_le_bytes());
        self.chunks.push((chunk::LOOP, b));
        self
    }

    pub fn chunk(mut self, kind: u8, bytes: Vec<u8>) -> Self {
        self.chunks.push((kind, bytes));
        self
    }
}

/// A synthetic bank: header words, samples and any bytes past the data.
#[derive(Debug, Clone)]
pub struct BankSpec {
    pub version: u32,
    pub codec: u32,
    pub samples: Vec<SampleSpec>,
    /// Bytes after the data block, as an `.arc` entry can carry.
    pub trailer: Vec<u8>,
}

impl BankSpec {
    /// Version 1, MPEG, as Giants writes them.
    pub fn new(samples: Vec<SampleSpec>) -> Self {
        Self {
            version: 1,
            codec: 0x0B,
            samples,
            trailer: Vec::new(),
        }
    }
}

/// Where [`write`] put things, for tests that corrupt a field.
#[derive(Debug, Clone, Default)]
pub struct Written {
    pub bytes: Vec<u8>,
    /// Each sample's packed header.
    pub sample_headers: Vec<usize>,
    /// Each chunk's word, per sample.
    pub chunks: Vec<Vec<usize>>,
    pub names_at: usize,
    pub data_at: usize,
}

/// The bank's bytes: each sample's data aligned to 32 bytes in the data
/// block, a name table when any sample is named.
pub fn write(spec: &BankSpec) -> Written {
    let header_len = if spec.version == 0 {
        HEADER_LEN_V0
    } else {
        HEADER_LEN_V1
    };
    // The data block, each sample at a 32-byte boundary.
    let mut data = Vec::new();
    let mut offsets = Vec::new();
    for s in &spec.samples {
        data.resize(data.len().next_multiple_of(DATA_ALIGN), 0);
        offsets.push(data.len());
        data.extend(&s.data);
    }
    // The sample headers and their chunks.
    let mut headers = Vec::new();
    let mut sample_headers = Vec::new();
    let mut chunk_at = Vec::new();
    for (s, &offset) in spec.samples.iter().zip(&offsets) {
        let packed = u64::from(!s.chunks.is_empty())
            | u64::from(s.rate_code & 0xF) << 1
            | u64::from(s.channel_code & 3) << 5
            | ((offset / DATA_ALIGN) as u64 & 0x07FF_FFFF) << 7
            | u64::from(s.samples) << 34;
        sample_headers.push(header_len + headers.len());
        headers.extend(packed.to_le_bytes());
        let mut mine = Vec::new();
        for (i, (kind, bytes)) in s.chunks.iter().enumerate() {
            let more = u32::from(i + 1 < s.chunks.len());
            let word = more | ((bytes.len() as u32) & 0x00FF_FFFF) << 1 | u32::from(*kind) << 25;
            mine.push(header_len + headers.len());
            headers.extend(word.to_le_bytes());
            headers.extend(bytes);
        }
        chunk_at.push(mine);
    }
    // The name table.
    let mut names = Vec::new();
    if spec.samples.iter().any(|s| s.name.is_some()) {
        let n = spec.samples.len();
        let mut strings = Vec::new();
        for s in &spec.samples {
            names.extend(((n * 4 + strings.len()) as u32).to_le_bytes());
            strings.extend(s.name.as_deref().unwrap_or("").as_bytes());
            strings.push(0);
        }
        names.extend(strings);
        names.resize(names.len().next_multiple_of(4), 0);
    }
    let mut out = MAGIC.to_vec();
    for w in [
        spec.version,
        spec.samples.len() as u32,
        headers.len() as u32,
        names.len() as u32,
        data.len() as u32,
        spec.codec,
    ] {
        out.extend(w.to_le_bytes());
    }
    out.resize(header_len, 0);
    out.extend(&headers);
    let names_at = out.len();
    out.extend(&names);
    let data_at = out.len();
    out.extend(&data);
    out.extend(&spec.trailer);
    Written {
        bytes: out,
        sample_headers,
        chunks: chunk_at,
        names_at,
        data_at,
    }
}

/// The property test's and the fuzz targets' starting points: a Giants
/// bank (one named MPEG sample with padding between its frames), a version
/// 0 PCM bank of two samples with loop, rate and channel chunks, and a bank
/// with no name table and a trailer.
pub fn seeds() -> Vec<Vec<u8>> {
    use crate::mpeg::builder::stream;
    let giants = BankSpec::new(vec![
        SampleSpec::new(stream(6, 3), 6 * 1152).named("0x06838641.wav.hz"),
    ]);
    let pcm = BankSpec {
        version: 0,
        codec: 0x02,
        samples: vec![
            SampleSpec::new(vec![1; 64], 16)
                .looped(2, 15)
                .chunk(chunk::RATE, 22_050u32.to_le_bytes().to_vec())
                .named("a"),
            SampleSpec::new(vec![2; 40], 10)
                .chunk(chunk::CHANNELS, vec![6])
                .chunk(9, vec![7; 5])
                .named("b"),
        ],
        trailer: Vec::new(),
    };
    let mut bare = BankSpec::new(vec![SampleSpec::new(stream(2, 0), 2 * 1152)]);
    bare.trailer = vec![0xAA; 9];
    vec![write(&giants).bytes, write(&pcm).bytes, write(&bare).bytes]
}
