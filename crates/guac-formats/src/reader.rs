//! Bounds-checked reads at absolute offsets, each failing with a
//! [`FormatError`] that names the field and where it was read.

use crate::{ErrorKind, FormatError};

pub(crate) struct Reader<'a> {
    data: &'a [u8],
    format: &'static str,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(format: &'static str, data: &'a [u8]) -> Self {
        Self { data, format }
    }

    pub(crate) fn len(&self) -> usize {
        self.data.len()
    }

    pub(crate) fn err(&self, field: &'static str, offset: usize, kind: ErrorKind) -> FormatError {
        FormatError::new(self.format, field, offset, kind)
    }

    pub(crate) fn bytes(
        &self,
        offset: usize,
        len: usize,
        field: &'static str,
    ) -> Result<&'a [u8], FormatError> {
        offset
            .checked_add(len)
            .and_then(|end| self.data.get(offset..end))
            .ok_or_else(|| {
                self.err(
                    field,
                    offset,
                    ErrorKind::Truncated {
                        needed: len,
                        available: self.data.len().saturating_sub(offset),
                    },
                )
            })
    }

    pub(crate) fn u16_le(&self, offset: usize, field: &'static str) -> Result<u16, FormatError> {
        let b = self.bytes(offset, 2, field)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(crate) fn u32_le(&self, offset: usize, field: &'static str) -> Result<u32, FormatError> {
        let b = self.bytes(offset, 4, field)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u32_be(&self, offset: usize, field: &'static str) -> Result<u32, FormatError> {
        let b = self.bytes(offset, 4, field)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn magic(
        &self,
        offset: usize,
        expected: &'static [u8],
        field: &'static str,
    ) -> Result<(), FormatError> {
        let found = self.data.get(offset..).unwrap_or_default();
        let found = &found[..found.len().min(expected.len())];
        if found == expected {
            Ok(())
        } else {
            Err(self.err(
                field,
                offset,
                ErrorKind::BadMagic {
                    expected,
                    found: found.to_vec(),
                },
            ))
        }
    }

    /// The bytes from `start` up to the first NUL before `end`, NUL excluded.
    pub(crate) fn cstr(
        &self,
        start: usize,
        end: usize,
        field: &'static str,
    ) -> Result<&'a [u8], FormatError> {
        let window = self
            .data
            .get(start..end.min(self.data.len()))
            .unwrap_or_default();
        window
            .iter()
            .position(|b| *b == 0)
            .map(|nul| &window[..nul])
            .ok_or_else(|| self.err(field, start, ErrorKind::MissingNul { end }))
    }

    pub(crate) fn utf8(
        &self,
        bytes: &'a [u8],
        offset: usize,
        field: &'static str,
    ) -> Result<&'a str, FormatError> {
        std::str::from_utf8(bytes).map_err(|_| self.err(field, offset, ErrorKind::InvalidUtf8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_past_the_end_fail_with_the_offset() {
        let r = Reader::new("T", &[1, 2, 3]);
        assert_eq!(r.u16_le(1, "f").unwrap(), 0x0302);
        assert_eq!(r.u32_be(0, "f").map_err(|e| e.offset), Err(0));
        assert!(r.u32_be(1, "f").is_err());
        let err = r.u32_le(1, "f").unwrap_err();
        assert_eq!(err.offset, 1);
        assert_eq!(
            err.kind,
            ErrorKind::Truncated {
                needed: 4,
                available: 2
            }
        );
        // An offset near usize::MAX can't wrap into range.
        assert!(r.bytes(usize::MAX, 2, "f").is_err());
        assert!(r.bytes(9, 0, "f").is_err());
    }

    #[test]
    fn cstr_stops_at_the_window() {
        let r = Reader::new("T", b"ab\0cd");
        assert_eq!(r.cstr(0, 5, "k").unwrap(), b"ab");
        let err = r.cstr(3, 5, "k").unwrap_err();
        assert_eq!(
            (err.offset, err.kind),
            (3, ErrorKind::MissingNul { end: 5 })
        );
        // The NUL at 2 is outside a window that ends at 2.
        assert!(r.cstr(0, 2, "k").is_err());
        assert!(r.cstr(7, 9, "k").is_err());
    }

    #[test]
    fn magic_reports_what_it_found() {
        let r = Reader::new("T", b"AB");
        let err = r.magic(0, b"ABC", "magic").unwrap_err();
        assert_eq!(
            err.kind,
            ErrorKind::BadMagic {
                expected: b"ABC",
                found: b"AB".to_vec()
            }
        );
        assert!(r.magic(0, b"AB", "magic").is_ok());
    }
}
