//! The one error type every parser returns.

/// A parse failure: which format, which field, where in the input and why.
///
/// `offset` is absolute from the start of the slice the parser was handed, so
/// it can be checked against a hex dump of the same file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{format}: {field} at offset {offset:#x}: {kind}")]
pub struct FormatError {
    pub format: &'static str,
    pub field: &'static str,
    pub offset: usize,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ErrorKind {
    #[error("bad magic {found:02x?}, expected {expected:02x?}")]
    BadMagic {
        expected: &'static [u8],
        found: Vec<u8>,
    },
    #[error("needs {needed} bytes, {available} available")]
    Truncated { needed: usize, available: usize },
    #[error("{value:#x} is out of range (limit {limit:#x})")]
    OutOfRange { value: u64, limit: u64 },
    #[error("no NUL terminator before offset {end:#x}")]
    MissingNul { end: usize },
    #[error("not valid UTF-8")]
    InvalidUtf8,
    #[error("unsupported value {value:#x}")]
    Unsupported { value: u64 },
    #[error("duplicate entry")]
    Duplicate,
    /// A part the format requires isn't there.
    #[error("required but missing")]
    Missing,
    /// An object isn't the type a reader asked for, nor a subtype of it.
    #[error("a {found} where a {expected} belongs")]
    WrongType {
        expected: &'static str,
        found: String,
    },
    #[error("version {found:#x} isn't read here ({supported})")]
    UnsupportedVersion { found: u64, supported: &'static str },
    /// Compressed data that didn't come out at the size its container
    /// declared: the decoder failed partway, ran out of input or had bytes
    /// left past the declared end.
    #[error("unpacked {unpacked} of the {expected} bytes declared: {reason}")]
    Unpack {
        expected: u64,
        unpacked: u64,
        reason: String,
    },
}

impl FormatError {
    pub fn new(format: &'static str, field: &'static str, offset: usize, kind: ErrorKind) -> Self {
        Self {
            format,
            field,
            offset,
            kind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_format_field_and_hex_offset() {
        let err = FormatError::new(
            "PARAM.SFO",
            "key_offset",
            0x24,
            ErrorKind::OutOfRange {
                value: 0x300,
                limit: 0x40,
            },
        );
        assert_eq!(
            err.to_string(),
            "PARAM.SFO: key_offset at offset 0x24: 0x300 is out of range (limit 0x40)"
        );
    }
}
