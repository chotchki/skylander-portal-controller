//! Decoding the serialized `frame_capture_data` stream into owned Rust values.
//! Layout and encoding are written up in the crate docs.

use std::collections::HashMap;
use std::io::Read;

use flate2::read::MultiGzDecoder;
use thiserror::Error;

pub const MAGIC: [u8; 4] = *b"RRC\0";
pub const SUPPORTED_VERSION: u32 = 6;

pub const TILE_STATE_BYTES: usize = 15 * 16 + 8 * 24;
pub const DISPLAY_BUFFERS_BYTES: usize = 8 * 16 + 4;
pub const PROGRAM_WORDS: usize = 544 * 4;
pub const CONSTANT_SLOTS: usize = 512;
pub const REGISTER_WORDS: usize = 0x10000 / 4;

const PROGRAM_BYTES: usize = PROGRAM_WORDS * 4;
const CONSTANT_BYTES: usize = CONSTANT_SLOTS * 16;
const REGISTER_BYTES: usize = REGISTER_WORDS * 4;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("not an RRC capture: magic is {0:02x?}, expected \"RRC\\0\"")]
    BadMagic([u8; 4]),
    #[error(
        "RRC version {found} is not supported: rrc-dump reads version {SUPPORTED_VERSION} only (re-capture on the pinned RPCS3)"
    )]
    UnsupportedVersion { found: u32 },
    #[error(
        "capture was written by a big-endian host (LE_format = {0}); only little-endian captures are read"
    )]
    NotLittleEndian(u32),
    #[error("capture truncated reading {what} at stream offset {offset:#x}")]
    Truncated { what: &'static str, offset: usize },
    #[error("varint for {what} at stream offset {offset:#x} overflows")]
    BadVarint { what: &'static str, offset: usize },
    #[error(
        "{left} bytes follow the command list; expected {without} (no starting constants) or {with} (savestate-booted capture)"
    )]
    BadRegisterState {
        left: usize,
        without: usize,
        with: usize,
    },
    #[error("gzip stream is corrupt: {0}")]
    Gzip(#[source] std::io::Error),
}

/// One `memory_map` entry: where a blob of bytes lands in RSX memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryBlock {
    pub offset: u32,
    pub location: u32,
    pub data_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayCommand {
    pub header: u32,
    pub value: u32,
    /// `memory_map` ids to apply before this command runs (unordered in the file).
    pub memory_state: Vec<u64>,
    pub tile_state: u64,
    pub display_buffer_state: u64,
}

#[derive(Debug, Clone)]
pub struct RegState {
    pub transform_program: Vec<u32>,
    /// Present only when the writing session was booted from a savestate.
    pub transform_constants: Option<Vec<[u32; 4]>>,
    pub registers: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Capture {
    pub version: u32,
    pub tile_states: usize,
    pub display_buffer_states: usize,
    /// Block id (what a command's `memory_state` names) to its placement.
    pub blocks: HashMap<u64, MemoryBlock>,
    /// A block's `data_id` to its bytes, shared by every block with identical contents.
    pub data: HashMap<u64, Vec<u8>>,
    pub commands: Vec<ReplayCommand>,
    pub reg_state: RegState,
}

/// Returns the serialized stream, gunzipping when the bytes start with the gzip magic.
pub fn decompress(file: &[u8]) -> Result<Vec<u8>, CaptureError> {
    if file.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        MultiGzDecoder::new(file)
            .read_to_end(&mut out)
            .map_err(CaptureError::Gzip)?;
        Ok(out)
    } else {
        Ok(file.to_vec())
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], CaptureError> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.bytes.len());
        let end = end.ok_or(CaptureError::Truncated {
            what,
            offset: self.pos,
        })?;
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u32(&mut self, what: &'static str) -> Result<u32, CaptureError> {
        Ok(u32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }

    fn u64(&mut self, what: &'static str) -> Result<u64, CaptureError> {
        Ok(u64::from_le_bytes(self.take(8, what)?.try_into().unwrap()))
    }

    fn varint(&mut self, what: &'static str) -> Result<usize, CaptureError> {
        let start = self.pos;
        let mut value: u64 = 0;
        for shift in (0..64).step_by(7) {
            let byte = self.take(1, what)?[0];
            let payload = u64::from(byte & 0x7f);
            if shift == 63 && payload > 1 {
                break;
            }
            value |= payload << shift;
            if byte & 0x80 == 0 {
                // A count bigger than the remaining stream can't be honest; reject before allocating.
                return usize::try_from(value)
                    .ok()
                    .filter(|&v| v <= self.bytes.len())
                    .ok_or(CaptureError::BadVarint {
                        what,
                        offset: start,
                    });
            }
        }
        Err(CaptureError::BadVarint {
            what,
            offset: start,
        })
    }

    fn left(&self) -> usize {
        self.bytes.len() - self.pos
    }
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect()
}

impl Capture {
    /// Reads a capture file's bytes, gzipped or not.
    pub fn from_file_bytes(file: &[u8]) -> Result<Self, CaptureError> {
        Self::parse(&decompress(file)?)
    }

    /// Reads an already-decompressed serialized stream.
    pub fn parse(stream: &[u8]) -> Result<Self, CaptureError> {
        let mut c = Cursor {
            bytes: stream,
            pos: 0,
        };
        let magic: [u8; 4] = c.take(4, "magic")?.try_into().unwrap();
        if magic != MAGIC {
            return Err(CaptureError::BadMagic(magic));
        }
        let version = c.u32("version")?;
        if version != SUPPORTED_VERSION {
            return Err(CaptureError::UnsupportedVersion { found: version });
        }
        let le = c.u32("LE_format")?;
        if le != 1 {
            return Err(CaptureError::NotLittleEndian(le));
        }

        let tile_states = c.varint("tile map count")?;
        for _ in 0..tile_states {
            c.take(TILE_STATE_BYTES, "tile state")?;
            c.u64("tile state id")?;
        }

        // Capacity hints are capped by the smallest entry that fits in what's left: a block is a
        // 16-byte memory_block plus its u64 id, a data entry at least a 1-byte length plus its u64 id.
        let block_count = c.varint("memory map count")?;
        let mut blocks = HashMap::with_capacity(block_count.min(c.left() / 24));
        for _ in 0..block_count {
            let block = MemoryBlock {
                offset: c.u32("memory block offset")?,
                location: c.u32("memory block location")?,
                data_id: c.u64("memory block data id")?,
            };
            blocks.insert(c.u64("memory block id")?, block);
        }

        let data_count = c.varint("memory data map count")?;
        let mut data = HashMap::with_capacity(data_count.min(c.left() / 9));
        for _ in 0..data_count {
            let len = c.varint("memory data length")?;
            let bytes = c.take(len, "memory data bytes")?.to_vec();
            data.insert(c.u64("memory data id")?, bytes);
        }

        let display_buffer_states = c.varint("display buffer map count")?;
        for _ in 0..display_buffer_states {
            c.take(DISPLAY_BUFFERS_BYTES, "display buffer state")?;
            c.u64("display buffer state id")?;
        }

        let command_count = c.varint("replay command count")?;
        let mut commands = Vec::with_capacity(command_count.min(c.left() / 20));
        for _ in 0..command_count {
            let header = c.u32("command header")?;
            let value = c.u32("command value")?;
            let n = c.varint("command memory state count")?;
            let memory_state = (0..n)
                .map(|_| c.u64("command memory state id"))
                .collect::<Result<_, _>>()?;
            commands.push(ReplayCommand {
                header,
                value,
                memory_state,
                tile_state: c.u64("command tile state")?,
                display_buffer_state: c.u64("command display buffer state")?,
            });
        }

        let without = PROGRAM_BYTES + REGISTER_BYTES;
        let with = without + CONSTANT_BYTES;
        let has_constants = match c.left() {
            n if n == without => false,
            n if n == with => true,
            left => {
                return Err(CaptureError::BadRegisterState {
                    left,
                    without,
                    with,
                });
            }
        };
        let transform_program = words(c.take(PROGRAM_BYTES, "transform program")?);
        let transform_constants = if has_constants {
            let raw = words(c.take(CONSTANT_BYTES, "transform constants")?);
            Some(raw.as_chunks::<4>().0.to_vec())
        } else {
            None
        };
        let registers = words(c.take(REGISTER_BYTES, "method registers")?);

        Ok(Capture {
            version,
            tile_states,
            display_buffer_states,
            blocks,
            data,
            commands,
            reg_state: RegState {
                transform_program,
                transform_constants,
                registers,
            },
        })
    }
}
