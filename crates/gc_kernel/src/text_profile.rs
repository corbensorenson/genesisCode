use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::error::KernelError;
use crate::fallible_alloc::{checked_add, string_with_capacity};

pub const UNICODE_STANDARD_VERSION: (u8, u8, u8) = (17, 0, 0);
pub const NORMALIZATION_IMPLEMENTATION_VERSION: &str = "unicode-normalization/0.1.25";
pub const SEGMENTATION_IMPLEMENTATION_VERSION: &str = "unicode-segmentation/1.13.3";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphemeRangeError {
    IndexOverflow,
    OutOfRange,
}

pub fn scalar_len(input: &str) -> usize {
    input.chars().count()
}

pub fn grapheme_len(input: &str) -> usize {
    input.graphemes(true).count()
}

pub fn normalize_nfc(input: &str) -> Result<String, KernelError> {
    let output_len = input.nfc().try_fold(0usize, |len, scalar| {
        checked_add(len, scalar.len_utf8(), "str/nfc")
    })?;
    let mut output = string_with_capacity(output_len, "str/nfc")?;
    output.extend(input.nfc());
    Ok(output)
}

pub fn grapheme_slice_bounds(
    input: &str,
    start: usize,
    len: usize,
) -> Result<(usize, usize), GraphemeRangeError> {
    let end = start
        .checked_add(len)
        .ok_or(GraphemeRangeError::IndexOverflow)?;
    let mut start_byte = None;
    let mut end_byte = None;
    let mut count = 0usize;

    for (byte, _) in input.grapheme_indices(true) {
        if count == start {
            start_byte = Some(byte);
        }
        if count == end {
            end_byte = Some(byte);
        }
        count = count
            .checked_add(1)
            .ok_or(GraphemeRangeError::IndexOverflow)?;
    }
    if count == start {
        start_byte = Some(input.len());
    }
    if count == end {
        end_byte = Some(input.len());
    }

    match (start_byte, end_byte) {
        (Some(start_byte), Some(end_byte)) => Ok((start_byte, end_byte)),
        _ => Err(GraphemeRangeError::OutOfRange),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implementation_tables_are_unicode_17() {
        assert_eq!(unicode_normalization::UNICODE_VERSION, (17, 0, 0));
        assert_eq!(UNICODE_STANDARD_VERSION, (17, 0, 0));
    }

    #[test]
    fn normalization_and_extended_graphemes_are_distinct() {
        assert_eq!(scalar_len("e\u{301}"), 2);
        assert_eq!(grapheme_len("e\u{301}"), 1);
        assert_eq!(normalize_nfc("e\u{301}").expect("normalize"), "é");
        assert_eq!(grapheme_len("👩‍👩‍👧‍👦"), 1);
    }

    #[test]
    fn grapheme_slice_accepts_empty_end_and_rejects_overflow() {
        assert_eq!(grapheme_slice_bounds("aé", 1, 1), Ok((1, 3)));
        assert_eq!(grapheme_slice_bounds("aé", 2, 0), Ok((3, 3)));
        assert_eq!(
            grapheme_slice_bounds("a", usize::MAX, 1),
            Err(GraphemeRangeError::IndexOverflow)
        );
        assert_eq!(
            grapheme_slice_bounds("a", 2, 0),
            Err(GraphemeRangeError::OutOfRange)
        );
    }
}
