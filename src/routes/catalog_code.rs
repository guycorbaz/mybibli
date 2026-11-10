//! Scanned code classification (`detect_code_type`).
//!
//! Split out of `routes/catalog.rs` by story 16-1 (#597, Foundation Rule #12).

use crate::services::title::TitleService;


/// Result of barcode prefix detection.
#[derive(Debug, PartialEq)]
pub struct CodeDetection {
    pub code_type: &'static str,
    pub inferred_media_type: Option<crate::models::media_type::MediaType>,
    pub inferred_code_type: Option<crate::models::media_type::CodeType>,
    /// Canonical form of the scanned code when it differs from the input:
    /// an ISBN-10 is rewritten to its ISBN-13 (story 11-1, #497). Callers
    /// use it for every downstream lookup, creation and provider call.
    pub normalized_code: Option<String>,
}

/// Validate an ISSN barcode as a valid EAN-13 with 977 prefix.
/// ISSN-13 format: 977 + 7-digit ISSN body + 2-digit issue + 1 EAN check digit.
/// Uses the standard EAN-13 checksum (same as ISBN-13: alternating weights 1,3).
fn validate_issn_from_ean(code: &str) -> bool {
    // EAN-13 check digit: same algorithm as ISBN-13
    code.starts_with("977") && TitleService::validate_isbn13_checksum(code)
}

/// Story 17-1 (#651, Rule #20) — a code that looks like a mistyped V-code:
/// `v`/`V` then 1 to 5 printable ASCII characters, no space, at least one
/// of them not a letter (`V12345`, `v12`, `V,/+`, `V12A4`). A word such as
/// `Valérian` or `Vol` is not one: the scan field also takes search text,
/// which gets the general message.
fn is_near_vcode(code: &str) -> bool {
    let Some(rest) = code.strip_prefix(['v', 'V']) else {
        return false;
    };
    (1..=5).contains(&rest.len())
        && rest.bytes().all(|b| b.is_ascii_graphic())
        && rest.bytes().any(|b| !b.is_ascii_alphabetic())
}

pub(crate) fn detect_code_type(code: &str) -> CodeDetection {
    use crate::models::media_type::{CodeType, MediaType};

    // Story 17-1 (#651): `v`/`V` + 4 ASCII digits is that V-code, carried as
    // `V` + digits (one rule, `VolumeService::normalize_vcode`). A near
    // V-code (`is_near_vcode`) stays `vcode` without rewrite, so the caller's
    // `validate_vcode` refuses it naming the expected shape.
    if let Some(canonical) = crate::services::volume::VolumeService::normalize_vcode(code) {
        return CodeDetection {
            code_type: "vcode",
            inferred_media_type: None,
            inferred_code_type: None,
            normalized_code: (canonical != code).then_some(canonical),
        };
    }
    if is_near_vcode(code) {
        return CodeDetection {
            code_type: "vcode",
            inferred_media_type: None,
            inferred_code_type: None,
            normalized_code: None,
        };
    }
    if code.starts_with('L') && code.len() == 5 {
        return CodeDetection {
            code_type: "lcode",
            inferred_media_type: None,
            inferred_code_type: None,
            normalized_code: None,
        };
    }
    // Story 12-1 (#502), 17-3 (#565): an ISBN typed with separators or an
    // `ISBN` prefix (`978-2-205-00121-1`, `ISBN 2 205 00121 3`) is an ISBN,
    // carried as its ISBN-13 (`utils::isbn_lookup_form`, the one rule). Only
    // an ISBN classification of that form is kept; anything else falls
    // through to the classification of the input as typed.
    let compact = crate::utils::isbn_lookup_form(code);
    if compact != code {
        let isbn13 = if crate::utils::is_isbn13_shaped(&compact) {
            Some(compact)
        } else {
            crate::utils::isbn10_to_isbn13(&compact)
        };
        if let Some(isbn13) = isbn13 {
            return CodeDetection {
                code_type: "isbn",
                inferred_media_type: Some(MediaType::Book),
                inferred_code_type: Some(CodeType::Isbn),
                normalized_code: Some(isbn13),
            };
        }
    }
    if crate::utils::is_isbn13_shaped(code) {
        return CodeDetection {
            code_type: "isbn",
            inferred_media_type: Some(MediaType::Book),
            inferred_code_type: Some(CodeType::Isbn),
            normalized_code: None,
        };
    }
    // Story 11-1 (#497): a valid ISBN-10 (incl. an `X` check digit) is an
    // ISBN, carried as its ISBN-13. A failed checksum falls through.
    if let Some(isbn13) = crate::utils::isbn10_to_isbn13(code) {
        return CodeDetection {
            code_type: "isbn",
            inferred_media_type: Some(MediaType::Book),
            inferred_code_type: Some(CodeType::Isbn),
            normalized_code: Some(isbn13),
        };
    }
    // 977 prefix: could be ISSN or UPC
    if code.starts_with("977") && code.len() == 13 && validate_issn_from_ean(code) {
        return CodeDetection {
            code_type: "issn",
            inferred_media_type: Some(MediaType::Magazine),
            inferred_code_type: Some(CodeType::Issn),
            normalized_code: None,
        };
    }
    if code.chars().all(|c| c.is_ascii_digit()) && code.len() >= 8 && code.len() <= 13 {
        return CodeDetection {
            code_type: "upc",
            inferred_media_type: None, // Needs disambiguation
            inferred_code_type: Some(CodeType::Upc),
            normalized_code: None,
        };
    }
    CodeDetection {
        code_type: "unknown",
        inferred_media_type: None,
        inferred_code_type: None,
        normalized_code: None,
    }
}

/// Story 16-5 (#638) — why an all-digit code classified `upc` looks wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeIrregularity {
    /// 9 digits: an ISBN-10 that may have lost its last character (often `X`).
    NineDigits,
    /// 10 digits: the check digit fails as an ISBN-10.
    TenDigits,
    /// Another length no barcode has.
    Length(usize),
    /// An EAN-8 or EAN-13 length with a wrong check digit.
    CheckDigit,
}

/// EAN-8 check digit of the first 7 digits (pure; weights 3 and 1).
fn ean8_check_digit(code: &str) -> Option<u32> {
    let mut sum = 0;
    for (i, c) in code.get(..7)?.chars().enumerate() {
        let d = c.to_digit(10)?;
        sum += if i % 2 == 0 { d * 3 } else { d };
    }
    Some((10 - (sum % 10)) % 10)
}

/// Story 16-5 (#638, Rule #20) — the irregularity of a code classified `upc`
/// (pure): only EAN-8 and EAN-13 with a valid check digit and 12 digits pass
/// silently. A 12-digit UPC-A with a wrong check digit is refused at creation
/// (#384), so no warning is offered for it. The caller warns and lets the
/// librarian decide: the code is never rewritten.
pub fn code_irregularity(code: &str) -> Option<CodeIrregularity> {
    let check = |expected: Option<u32>| {
        let last = code.chars().last().and_then(|c| c.to_digit(10));
        (expected != last).then_some(CodeIrregularity::CheckDigit)
    };
    match code.len() {
        8 => check(ean8_check_digit(code)),
        13 => check(crate::utils::ean13_check_digit(code)),
        12 => None,
        9 => Some(CodeIrregularity::NineDigits),
        10 => Some(CodeIrregularity::TenDigits),
        len => Some(CodeIrregularity::Length(len)),
    }
}

impl CodeIrregularity {
    /// The warning shown with the media-type choice (Rule #9).
    pub fn message(self, loc: &str) -> String {
        match self {
            CodeIrregularity::NineDigits => {
                rust_i18n::t!("scan.irregular_nine_digits", locale = loc).to_string()
            }
            CodeIrregularity::TenDigits => {
                rust_i18n::t!("scan.irregular_ten_digits", locale = loc).to_string()
            }
            CodeIrregularity::Length(len) => {
                rust_i18n::t!("scan.irregular_length", locale = loc, len = len).to_string()
            }
            CodeIrregularity::CheckDigit => {
                rust_i18n::t!("scan.irregular_check_digit", locale = loc).to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_code_type_isbn_978() {
        let d = detect_code_type("9782070360246");
        assert_eq!(d.code_type, "isbn");
        assert_eq!(
            d.inferred_media_type,
            Some(crate::models::media_type::MediaType::Book)
        );
        assert_eq!(
            d.inferred_code_type,
            Some(crate::models::media_type::CodeType::Isbn)
        );
    }

    #[test]
    fn test_detect_code_type_isbn_979() {
        assert_eq!(detect_code_type("9791032305560").code_type, "isbn");
    }

    #[test]
    fn test_detect_code_type_vcode() {
        assert_eq!(detect_code_type("V0042").code_type, "vcode");
    }

    // Story 17-1 (#651) — a lowercase `v` is the same V-code, carried canonical.
    #[test]
    fn test_detect_code_type_lowercase_vcode_is_normalized() {
        let d = detect_code_type("v0042");
        assert_eq!(d.code_type, "vcode");
        assert_eq!(d.normalized_code.as_deref(), Some("V0042"));
        assert_eq!(detect_code_type("V0042").normalized_code, None);
    }

    // Story 17-1 (#651) — any other code starting with a `v` is a malformed
    // volume label (refused naming the shape), never rewritten.
    #[test]
    fn test_detect_code_type_near_vcode_stays_vcode_unrewritten() {
        for code in ["V12345", "v12", "V,/+", "V12A4", "V0-42"] {
            let d = detect_code_type(code);
            assert_eq!(d.code_type, "vcode", "{code}");
            assert_eq!(d.normalized_code, None, "{code}");
            assert!(!crate::services::volume::VolumeService::validate_vcode(code), "{code}");
        }
    }

    // Story 17-1 (#651) — words and spaced or long inputs starting with a V
    // are search text or other codes, not near V-codes.
    #[test]
    fn test_detect_code_type_text_starting_with_v_is_not_a_vcode() {
        for code in ["Valérian", "Vagabond", "Vol", "VABCD", "V for Vendetta", "V 0042", "V", "V123456", "V٠١٢٣"] {
            assert_eq!(detect_code_type(code).code_type, "unknown", "{code}");
        }
    }

    // Story 17-1 (#651) — the reported input: five digits, no V. Not guessed.
    #[test]
    fn test_detect_code_type_five_digits_is_unknown() {
        let d = detect_code_type("11338");
        assert_eq!(d.code_type, "unknown");
        assert_eq!(d.normalized_code, None);
    }

    #[test]
    fn test_detect_code_type_lcode() {
        assert_eq!(detect_code_type("L0001").code_type, "lcode");
    }

    #[test]
    fn test_detect_code_type_upc() {
        let d = detect_code_type("12345678");
        assert_eq!(d.code_type, "upc");
        assert!(d.inferred_media_type.is_none()); // UPC needs disambiguation
        assert_eq!(
            d.inferred_code_type,
            Some(crate::models::media_type::CodeType::Upc)
        );
    }

    #[test]
    fn test_detect_code_type_unknown() {
        assert_eq!(detect_code_type("ABCDEF").code_type, "unknown");
    }

    #[test]
    fn test_detect_code_type_v0000() {
        // V0000 matches vcode format — validation happens in VolumeService
        assert_eq!(detect_code_type("V0000").code_type, "vcode");
    }

    #[test]
    fn test_detect_code_type_977_valid_issn() {
        // 13-digit 977-prefixed code with valid EAN-13 check digit → ISSN
        let d = detect_code_type("9770000000003");
        assert_eq!(d.code_type, "issn");
        assert_eq!(
            d.inferred_media_type,
            Some(crate::models::media_type::MediaType::Magazine)
        );
    }

    #[test]
    fn test_detect_code_type_977_invalid_ean_checksum() {
        // 13-digit 977-prefixed code with INVALID EAN-13 check digit → UPC
        let d = detect_code_type("9770000000000");
        assert_eq!(d.code_type, "upc");
    }

    #[test]
    fn test_detect_code_type_977_invalid_issn_as_upc() {
        // 977-prefix but shorter than 13 digits → falls through to UPC
        let d = detect_code_type("9771234567");
        assert_eq!(d.code_type, "upc");
    }

    // Story 12-1 (#502) — ISBN typed with separators.
    #[test]
    fn test_detect_code_type_isbn13_with_separators_is_carried_bare() {
        for code in [
            "978-2-205-00121-1",
            "978 2 205 00121 1",
            "978\u{2010}2\u{2010}205\u{2010}00121\u{2010}1",
            "978\u{a0}2\u{a0}205\u{a0}00121\u{a0}1",
        ] {
            let d = detect_code_type(code);
            assert_eq!(d.code_type, "isbn", "{code}");
            assert_eq!(d.inferred_media_type, Some(crate::models::media_type::MediaType::Book));
            assert_eq!(d.normalized_code.as_deref(), Some("9782205001211"), "{code}");
        }
    }

    #[test]
    fn test_detect_code_type_isbn10_with_separators_becomes_isbn13() {
        for (code, isbn13) in [
            ("2-205-00121-3", "9782205001211"),
            ("2 205 00121 3", "9782205001211"),
            ("0-8044-2957-X", "9780804429573"),
            ("0-8044-2957-x", "9780804429573"),
        ] {
            let d = detect_code_type(code);
            assert_eq!(d.code_type, "isbn", "{code}");
            assert_eq!(d.normalized_code.as_deref(), Some(isbn13), "{code}");
        }
    }

    #[test]
    fn test_detect_code_type_non_isbn_with_separators_keeps_raw_classification() {
        // Not 978/979, bad mod-11 checksum, ISSN-EAN, text: the compact form
        // is not an ISBN, so the input as typed is classified (as before).
        for code in ["400-1234-567890", "2-205-00121-4", "977-1234-567003", "Spider-Man 3"] {
            let d = detect_code_type(code);
            assert_eq!(d.code_type, "unknown", "{code}");
            assert_eq!(d.normalized_code, None, "{code}");
        }
        // A bad EAN check digit is still an `isbn` shape, like its bare form;
        // `create_from_isbn` refuses it downstream.
        let d = detect_code_type("978-2-205-00121-9");
        assert_eq!(d.code_type, "isbn");
        assert_eq!(d.normalized_code.as_deref(), Some("9782205001219"));
    }

    // Story 17-3 (#565) — a pasted `ISBN` prefix.
    #[test]
    fn test_detect_code_type_isbn_prefix_is_removed() {
        for (code, isbn13) in [
            ("ISBN 978-2-07-036024-6", "9782070360246"),
            ("ISBN-13: 9782070360246", "9782070360246"),
            ("isbn 2-205-00121-3", "9782205001211"),
            ("ISBN-10 2205001213", "9782205001211"),
        ] {
            let d = detect_code_type(code);
            assert_eq!(d.code_type, "isbn", "{code}");
            assert_eq!(d.normalized_code.as_deref(), Some(isbn13), "{code}");
        }
        assert_eq!(detect_code_type("ISBN 9772070360246").code_type, "unknown");
    }

    #[test]
    fn test_detect_code_type_bare_codes_take_the_unchanged_path() {
        assert_eq!(detect_code_type("9782205001211").normalized_code, None);
        assert_eq!(detect_code_type("V0042").code_type, "vcode");
        assert_eq!(detect_code_type("12345678").code_type, "upc");
    }

    // Story 11-1 (#497) — ISBN-10 at scan.
    #[test]
    fn test_detect_code_type_valid_isbn10_is_isbn_with_isbn13() {
        let d = detect_code_type("2205001213");
        assert_eq!(d.code_type, "isbn");
        assert_eq!(d.inferred_media_type, Some(crate::models::media_type::MediaType::Book));
        assert_eq!(d.inferred_code_type, Some(crate::models::media_type::CodeType::Isbn));
        assert_eq!(d.normalized_code.as_deref(), Some("9782205001211"));
    }

    #[test]
    fn test_detect_code_type_isbn10_x_check_digit() {
        for code in ["080442957X", "080442957x"] {
            let d = detect_code_type(code);
            assert_eq!(d.code_type, "isbn", "{code}");
            assert_eq!(d.normalized_code.as_deref(), Some("9780804429573"));
        }
    }

    #[test]
    fn test_detect_code_type_isbn10_bad_checksum_keeps_current_class() {
        let d = detect_code_type("2205001214");
        assert_eq!(d.code_type, "upc");
        assert_eq!(d.normalized_code, None);
        let d = detect_code_type("220500121X");
        assert_eq!(d.code_type, "unknown");
        assert_eq!(d.normalized_code, None);
    }

    #[test]
    fn test_detect_code_type_isbn13_is_not_rewritten() {
        assert_eq!(detect_code_type("9782205001211").normalized_code, None);
        assert_eq!(detect_code_type("12345678").normalized_code, None);
    }

    // Story 16-5 (#638).
    #[test]
    fn valid_barcodes_are_regular() {
        for code in ["96385074", "0093624738626", "5051889004578", "5410983227007", "036000291452"] {
            assert_eq!(code_irregularity(code), None, "{code}");
        }
    }

    #[test]
    fn irregular_codes_say_why() {
        use CodeIrregularity::*;
        assert_eq!(code_irregularity("220500218"), Some(NineDigits));
        assert_eq!(code_irregularity("2205025062"), Some(TenDigits));
        assert_eq!(code_irregularity("12345678901"), Some(Length(11)));
        assert_eq!(code_irregularity("12345678"), Some(CheckDigit));
        assert_eq!(code_irregularity("0093624738627"), Some(CheckDigit));
        // A 12-digit UPC-A is left to the #384 refusal at creation.
        assert_eq!(code_irregularity("036000291453"), None);
    }

    #[test]
    fn what_reaches_the_upc_arm_is_covered() {
        // The two production codes that started #638 reach the arm.
        for code in ["220500218", "2205025062"] {
            assert_eq!(detect_code_type(code).code_type, "upc", "{code}");
            assert!(code_irregularity(code).is_some(), "{code}");
        }
        // A valid ISBN-10 never does.
        assert_eq!(detect_code_type("2205011502").code_type, "isbn");
    }

    #[test]
    fn each_irregularity_has_a_message() {
        for i in [
            CodeIrregularity::NineDigits,
            CodeIrregularity::TenDigits,
            CodeIrregularity::Length(11),
            CodeIrregularity::CheckDigit,
        ] {
            for loc in ["en", "fr", "de", "it"] {
                let m = i.message(loc);
                assert!(!m.is_empty() && !m.contains("scan."), "{loc}: {m}");
            }
        }
        assert!(CodeIrregularity::Length(11).message("en").contains("11"));
    }
}
