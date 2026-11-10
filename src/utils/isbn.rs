//! Story 17-3 (#565, FR174) — the one ISBN rule shared by every entry
//! point: the scan, the web and MCP search, manual title creation, the
//! title edit form, the wish list, expected tomes and the boot repair of
//! stored ISBNs.
//!
//! Positive shapes only (Foundation Rule #20): an optional `ISBN` prefix,
//! the separators of [`strip_isbn_separators`], then 13 digits starting
//! `978` / `979` with a valid check digit, or a valid ISBN-10 (converted to
//! its ISBN-13). Anything else is refused, never rewritten.

use super::{compact_isbn, ean13_check_digit, isbn10_to_isbn13, strip_isbn_separators};

/// Why a typed value is not an ISBN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsbnError {
    /// Not 10 or 13 characters of an ISBN once the prefix and the
    /// separators are removed (or a 13-digit code outside 978 / 979).
    Format,
    /// ISBN-shaped, but its check digit does not match.
    Checksum,
}

impl IsbnError {
    /// The terse field hint (Rule #9) for this refusal.
    pub fn message(self, loc: &str) -> String {
        match self {
            Self::Format => rust_i18n::t!("error.isbn.invalid_format", locale = loc),
            Self::Checksum => rust_i18n::t!("error.isbn.invalid_checksum", locale = loc),
        }
        .to_string()
    }
}

/// `input` without a leading `ISBN` (any case), the readings of what
/// follows it: with an `-10` / `-13` / `10` / `13` qualifier taken as a
/// qualifier, and taken as digits. Each reading has its optional `:`
/// removed. Without the prefix, the input alone.
fn readings(input: &str) -> Vec<&str> {
    let trimmed = input.trim_start_matches(super::is_isbn_separator);
    let Some(rest) = trimmed
        .get(..4)
        .filter(|p| p.eq_ignore_ascii_case("isbn"))
        .map(|_| &trimmed[4..])
    else {
        return vec![input];
    };
    let mut out = vec![after_colon(rest)];
    let spaced = rest.trim_start_matches(super::is_isbn_separator);
    for qualifier in ["10", "13"] {
        if let Some(after) = spaced.strip_prefix(qualifier) {
            out.push(after_colon(after));
        }
    }
    out
}

/// `s` without separators and one `:` (full-width too) at its start.
fn after_colon(s: &str) -> &str {
    let s = s.trim_start_matches(super::is_isbn_separator);
    s.strip_prefix([':', '\u{FF1A}']).unwrap_or(s)
}

/// One reading, prefix already removed: its ISBN-13 or why it is not one.
fn parse_body(body: &str) -> Result<String, IsbnError> {
    let compact = strip_isbn_separators(body);
    let bytes = compact.as_bytes();
    if bytes.len() == 13 && bytes.iter().all(u8::is_ascii_digit) {
        if !(compact.starts_with("978") || compact.starts_with("979")) {
            return Err(IsbnError::Format);
        }
        return if ean13_check_digit(&compact) == compact[12..].parse::<u32>().ok() {
            Ok(compact)
        } else {
            Err(IsbnError::Checksum)
        };
    }
    if bytes.len() == 10
        && bytes[..9].iter().all(u8::is_ascii_digit)
        && matches!(bytes[9], b'0'..=b'9' | b'X' | b'x')
    {
        return isbn10_to_isbn13(&compact).ok_or(IsbnError::Checksum);
    }
    Err(IsbnError::Format)
}

/// The canonical ISBN-13 of `input`, or why it is not an ISBN.
///
/// A `10` / `13` after the prefix is read both as a qualifier and as
/// digits; the two readings differ by two characters, so at most one has
/// the length of an ISBN: `ISBN-13 1312345678` keeps all its digits.
pub fn parse_isbn(input: &str) -> Result<String, IsbnError> {
    let mut error = IsbnError::Format;
    for reading in readings(input) {
        match parse_body(reading) {
            Ok(isbn13) => return Ok(isbn13),
            Err(IsbnError::Checksum) => error = IsbnError::Checksum,
            Err(IsbnError::Format) => {}
        }
    }
    Err(error)
}

/// An ISBN typed in a form: `None` when blank (or separators only), the
/// canonical ISBN-13, or the refusal.
pub fn normalize_isbn_input(input: &str) -> Result<Option<String>, IsbnError> {
    if strip_isbn_separators(input).is_empty() {
        return Ok(None);
    }
    parse_isbn(input).map(Some)
}

/// What the scan and the search look an input up by: its canonical
/// ISBN-13 when it is an ISBN, otherwise the separator compaction of
/// [`compact_isbn`] (an unprefixed ISBN-shaped value with a wrong check
/// digit still reaches the scan's checksum refusal; a prefixed one stays as
/// typed, like any text).
pub fn isbn_lookup_form(input: &str) -> String {
    parse_isbn(input).unwrap_or_else(|_| compact_isbn(input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_isbn13_is_kept() {
        assert_eq!(parse_isbn("9782070360246").as_deref(), Ok("9782070360246"));
        assert_eq!(parse_isbn("9791032305560").as_deref(), Ok("9791032305560"));
    }

    #[test]
    fn separators_are_removed() {
        for input in [
            "978-2-205-00121-1",
            "978 2 205 00121 1",
            " 978\u{2011}2\u{2013}205\u{a0}00121\t1 ",
            "978\u{00ad}2205\u{200b}001211",
        ] {
            assert_eq!(parse_isbn(input).as_deref(), Ok("9782205001211"), "{input:?}");
        }
    }

    #[test]
    fn an_isbn10_becomes_its_isbn13() {
        assert_eq!(parse_isbn("2205001213").as_deref(), Ok("9782205001211"));
        assert_eq!(parse_isbn("2-205-00121-3").as_deref(), Ok("9782205001211"));
        assert_eq!(parse_isbn("0-8044-2957-x").as_deref(), Ok("9780804429573"));
        assert_eq!(parse_isbn("080442957X").as_deref(), Ok("9780804429573"));
    }

    #[test]
    fn a_prefix_is_removed() {
        for input in [
            "ISBN 978-2-07-036024-6",
            "isbn 9782070360246",
            "ISBN:9782070360246",
            "ISBN : 978 2 07 036024 6",
            "ISBN-13: 978-2-07-036024-6",
            "ISBN-13 9782070360246",
            "ISBN13 9782070360246",
            "ISBN 13 978-2-07-036024-6",
            "ISBN-139782070360246",
            "Isbn\u{a0}978-2-07-036024-6",
        ] {
            assert_eq!(parse_isbn(input).as_deref(), Ok("9782070360246"), "{input:?}");
        }
        for input in ["ISBN-10: 2-205-00121-3", "ISBN10 2205001213", "ISBN 2205001213"] {
            assert_eq!(parse_isbn(input).as_deref(), Ok("9782205001211"), "{input:?}");
        }
    }

    #[test]
    fn a_qualifier_before_an_isbn10_starting_with_its_digits_keeps_them() {
        // Valid ISBN-10s starting with 13 and with 10.
        let ten13 = (0..10)
            .map(|d| format!("131234567{d}"))
            .chain(std::iter::once("131234567X".to_string()))
            .find(|c| isbn10_to_isbn13(c).is_some())
            .unwrap();
        let ten10 = (0..10)
            .map(|d| format!("102345678{d}"))
            .chain(std::iter::once("102345678X".to_string()))
            .find(|c| isbn10_to_isbn13(c).is_some())
            .unwrap();
        let want13 = isbn10_to_isbn13(&ten13).unwrap();
        let want10 = isbn10_to_isbn13(&ten10).unwrap();
        assert_eq!(parse_isbn(&format!("ISBN-13 {ten13}")), Ok(want13.clone()));
        assert_eq!(parse_isbn(&format!("ISBN {ten13}")), Ok(want13.clone()));
        assert_eq!(parse_isbn(&format!("ISBN-13: {ten13}")), Ok(want13));
        assert_eq!(parse_isbn(&format!("ISBN 10-{}", &ten10[2..])), Ok(want10.clone()));
        assert_eq!(parse_isbn(&format!("ISBN {ten10}")), Ok(want10));
    }

    #[test]
    fn a_wrong_check_digit_is_a_checksum_error() {
        assert_eq!(parse_isbn("9782070360247"), Err(IsbnError::Checksum));
        assert_eq!(parse_isbn("978-2-07-036024-7"), Err(IsbnError::Checksum));
        assert_eq!(parse_isbn("2205001214"), Err(IsbnError::Checksum));
        assert_eq!(parse_isbn("ISBN 2-205-00121-4"), Err(IsbnError::Checksum));
    }

    #[test]
    fn anything_else_is_a_format_error() {
        for input in [
            "",
            "abc",
            "Spider-Man",
            "978207036024",       // 12 digits
            "97820703602461",     // 14 digits
            "9772070360246",      // another EAN-13 (977)
            "0123456789012",      // another EAN-13
            "978.2.07.036024.6",  // dots are no separators
            "978/2070360246",
            "X205001213",
            "22050O1213",
            "ISBN",
            "ISBN-13",
            "ISBNX 9782070360246",
            "ISBN-12 9782070360246",
            "EAN 9782070360246",
            "9782070360246 ISBN",
            "é-é-é-é-é",
        ] {
            assert_eq!(parse_isbn(input), Err(IsbnError::Format), "{input:?}");
        }
    }

    #[test]
    fn normalize_isbn_input_clears_a_blank_value() {
        for blank in ["", "  ", "-", "\u{2014}", "\u{00ad}"] {
            assert_eq!(normalize_isbn_input(blank), Ok(None), "{blank:?}");
        }
        assert_eq!(
            normalize_isbn_input(" 978-2-205-00121-1 "),
            Ok(Some("9782205001211".to_string()))
        );
        assert_eq!(normalize_isbn_input("12-34"), Err(IsbnError::Format));
    }

    #[test]
    fn isbn_lookup_form_falls_back_to_the_separator_compaction() {
        assert_eq!(isbn_lookup_form("ISBN 2-205-00121-3"), "9782205001211");
        assert_eq!(isbn_lookup_form("978-2-07-036024-7"), "9782070360247");
        assert_eq!(isbn_lookup_form("Spider-Man"), "Spider-Man");
        assert_eq!(isbn_lookup_form("ISBN 978-2-07-036024-7"), "ISBN 978-2-07-036024-7");
        assert_eq!(isbn_lookup_form("V0042"), "V0042");
    }

    #[test]
    fn every_error_has_a_message_in_every_locale() {
        for loc in ["en", "fr", "de", "it"] {
            for e in [IsbnError::Format, IsbnError::Checksum] {
                let m = e.message(loc);
                assert!(!m.is_empty() && !m.contains("error.isbn"), "{loc}: {m}");
            }
        }
    }
}
