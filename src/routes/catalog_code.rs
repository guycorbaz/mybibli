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
    /// Story 17-4 — a character other than a digit. Only a hand-typed code
    /// can carry one: the scan classifies digits only as `upc`.
    NotDigits,
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
        format!("{} {}", self.cause(loc), rust_i18n::t!("scan.irregular_action_scan", locale = loc))
    }

    /// Story 17-4 — the same warning under a form's UPC field, where the
    /// librarian corrects the code or ticks "keep as entered".
    pub fn form_message(self, loc: &str) -> String {
        format!("{} {}", self.cause(loc), rust_i18n::t!("scan.irregular_action_form", locale = loc))
    }

    /// What is wrong with the code, and why (Rule #9, first two parts).
    fn cause(self, loc: &str) -> String {
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
            CodeIrregularity::NotDigits => {
                rust_i18n::t!("scan.irregular_not_digits", locale = loc).to_string()
            }
        }
    }
}

/// The warning paragraph of an irregular code (`data-code-warning`), shared
/// by the scan's media-type choice and the UPC field of the title forms.
pub(crate) fn code_warning_html(warning: &str) -> String {
    format!(
        r#"<p data-code-warning class="mt-1 text-sm font-medium text-amber-800 dark:text-amber-300">{}</p>"#,
        crate::utils::html_escape(warning)
    )
}

/// Story 17-4 (#643 item 1) — what a UPC typed in a title form needs
/// before it is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedUpcVerdict {
    /// Store it as entered.
    Keep,
    /// Irregular and not confirmed yet: warn, store nothing.
    Confirm(CodeIrregularity),
    /// Refused: no confirmation can keep it.
    Refuse(UpcRefusal),
}

/// Story 17-4 — why a typed UPC is refused outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpcRefusal {
    /// A 12-digit UPC-A with a wrong check digit (#384), as the scan
    /// refuses it.
    CheckDigit,
    /// More than 13 characters: `titles.upc` cannot hold it.
    TooLong,
    /// A valid ISBN-10: it belongs in the ISBN field. The scan reads it as
    /// an ISBN, and one kept as a UPC would be moved to the ISBN at the next
    /// start (`convert_isbn10_upc_titles`, story 13-3) — so no "keep as
    /// entered" is offered (review round 2).
    Isbn10,
}

impl UpcRefusal {
    /// The field hint (Rule #9).
    pub fn message(self, loc: &str) -> String {
        match self {
            UpcRefusal::CheckDigit => {
                rust_i18n::t!("error.upc.invalid_checksum", locale = loc).to_string()
            }
            UpcRefusal::TooLong => rust_i18n::t!("error.upc.too_long", locale = loc).to_string(),
            UpcRefusal::Isbn10 => rust_i18n::t!("error.upc.isbn10", locale = loc).to_string(),
        }
    }
}

/// The longest UPC `titles.upc` (`VARCHAR(13)`) holds.
const UPC_MAX_CHARS: usize = 13;

/// Story 17-4 (Rule #20, pure) — the verdict on a typed UPC, separators
/// already removed. `confirmed` is the code the librarian ticked "keep as
/// entered" for; it keeps an irregular code only while it is that same
/// code. Only all-digit codes that `code_irregularity` passes are kept
/// silently.
pub fn typed_upc_verdict(upc: &str, confirmed: Option<&str>) -> TypedUpcVerdict {
    let irregularity = if upc.chars().count() > UPC_MAX_CHARS {
        return TypedUpcVerdict::Refuse(UpcRefusal::TooLong);
    } else if crate::utils::isbn10_to_isbn13(upc).is_some() {
        return TypedUpcVerdict::Refuse(UpcRefusal::Isbn10);
    } else if is_isbn10_shaped(upc) {
        // 9 digits and an `X` that fail the ISBN-10 check: the scan's
        // 10-digit warning, not "characters other than digits".
        Some(CodeIrregularity::TenDigits)
    } else if !upc.bytes().all(|b| b.is_ascii_digit()) {
        Some(CodeIrregularity::NotDigits)
    } else if TitleService::is_bad_upc_a(upc) {
        return TypedUpcVerdict::Refuse(UpcRefusal::CheckDigit);
    } else {
        code_irregularity(upc)
    };
    match irregularity {
        Some(i) if confirmed != Some(upc) => TypedUpcVerdict::Confirm(i),
        _ => TypedUpcVerdict::Keep,
    }
}

/// 9 digits then a digit or `X`: the shape of an ISBN-10, valid or not.
fn is_isbn10_shaped(code: &str) -> bool {
    code.len() == 10
        && code.as_bytes()[..9].iter().all(u8::is_ascii_digit)
        && code.ends_with(|c: char| c.is_ascii_digit() || c == 'X' || c == 'x')
}

/// Story 17-4 — what a title form does with its UPC field.
pub(crate) enum TypedUpcOutcome {
    /// The value to write (`None` clears the column).
    Store(Option<String>),
    /// Store nothing: send this response, which puts the warning and its
    /// confirmation in the form's UPC slot.
    Warn(axum::response::Response),
}

/// Story 17-4 (#643 item 1) — the check of a title form's UPC field, shared
/// by manual creation and the title edit. Separators are removed (CR #272).
/// A value equal to `stored` (the title's UPC, edit form) is written back as
/// stored without a check (as 17-3's unchanged ISBN). An irregular code
/// answers with the scan's warning swapped into `#<slot_id>` (the response
/// retargets there, so focus is not moved, Rule #21); a 12-digit code with a
/// wrong check digit is refused with the #384 message, a code longer than
/// the column with its own hint.
pub(crate) fn check_typed_upc(
    typed: Option<&str>,
    stored: Option<&str>,
    confirmed: Option<&str>,
    slot_id: &str,
    loc: &str,
) -> Result<TypedUpcOutcome, crate::error::AppError> {
    use axum::response::IntoResponse;
    let without_separators = |v: Option<&str>| v.and_then(crate::utils::identifier_without_separators);
    let Some(upc) = without_separators(typed) else {
        return Ok(TypedUpcOutcome::Store(None));
    };
    if without_separators(stored).as_deref() == Some(upc.as_str()) {
        return Ok(TypedUpcOutcome::Store(stored.map(str::to_string)));
    }
    match typed_upc_verdict(&upc, without_separators(confirmed).as_deref()) {
        TypedUpcVerdict::Keep => Ok(TypedUpcOutcome::Store(Some(upc))),
        TypedUpcVerdict::Refuse(refusal) => Err(crate::error::AppError::BadRequest(refusal.message(loc))),
        TypedUpcVerdict::Confirm(irregularity) => Ok(TypedUpcOutcome::Warn(
            (
                [("HX-Retarget", format!("#{slot_id}")), ("HX-Reswap", "innerHTML".to_string())],
                axum::response::Html(typed_upc_warning_html(irregularity, &upc, loc)),
            )
                .into_response(),
        )),
    }
}

/// Story 17-4 — the content of a form's UPC warning slot: the scan's
/// warning and a "keep as entered" box bound to this code.
pub(crate) fn typed_upc_warning_html(irregularity: CodeIrregularity, upc: &str, loc: &str) -> String {
    let code = crate::utils::html_escape(upc);
    let keep = crate::utils::html_escape(&rust_i18n::t!("title.form.upc_keep", locale = loc, code = upc));
    format!(
        r#"{}<label class="mt-1 flex items-center gap-2 text-sm text-stone-700 dark:text-stone-300"><input type="checkbox" name="upc_confirmed" value="{code}" class="rounded border-stone-300 dark:border-stone-600"> {keep}</label>"#,
        code_warning_html(&irregularity.form_message(loc))
    )
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
            CodeIrregularity::NotDigits,
        ] {
            for loc in ["en", "fr", "de", "it"] {
                for m in [i.message(loc), i.form_message(loc)] {
                    assert!(!m.is_empty() && !m.contains("scan."), "{loc}: {m}");
                }
                assert_ne!(i.message(loc), i.form_message(loc), "{loc}");
            }
        }
        assert!(CodeIrregularity::Length(11).message("en").contains("11"));
    }

    // Story 17-4 (#643 item 1).
    #[test]
    fn a_regular_typed_upc_is_kept() {
        for code in ["96385074", "0093624738626", "036000291452"] {
            assert_eq!(typed_upc_verdict(code, None), TypedUpcVerdict::Keep, "{code}");
        }
    }

    #[test]
    fn an_irregular_typed_upc_asks_for_confirmation() {
        use CodeIrregularity::*;
        assert_eq!(typed_upc_verdict("220500218", None), TypedUpcVerdict::Confirm(NineDigits));
        assert_eq!(typed_upc_verdict("2205025062", None), TypedUpcVerdict::Confirm(TenDigits));
        assert_eq!(typed_upc_verdict("12345", None), TypedUpcVerdict::Confirm(Length(5)));
        assert_eq!(typed_upc_verdict("0093624738627", None), TypedUpcVerdict::Confirm(CheckDigit));
        assert_eq!(typed_upc_verdict("1234567A", None), TypedUpcVerdict::Confirm(NotDigits));
        assert_eq!(typed_upc_verdict("ABC", None), TypedUpcVerdict::Confirm(NotDigits));
    }

    #[test]
    fn a_confirmation_keeps_only_the_code_it_was_given_for() {
        assert_eq!(typed_upc_verdict("220500218", Some("220500218")), TypedUpcVerdict::Keep);
        // Corrected after the warning: checked again.
        assert_eq!(
            typed_upc_verdict("220500217", Some("220500218")),
            TypedUpcVerdict::Confirm(CodeIrregularity::NineDigits)
        );
    }

    #[test]
    fn a_wrong_upc_a_check_digit_is_refused_even_confirmed() {
        assert_eq!(typed_upc_verdict("036000291453", None), TypedUpcVerdict::Refuse(UpcRefusal::CheckDigit));
        assert_eq!(typed_upc_verdict("036000291453", Some("036000291453")), TypedUpcVerdict::Refuse(UpcRefusal::CheckDigit));
    }

    #[test]
    fn a_typed_upc_longer_than_the_column_is_refused() {
        assert_eq!(typed_upc_verdict("12345678901234", None), TypedUpcVerdict::Refuse(UpcRefusal::TooLong));
        assert_eq!(
            typed_upc_verdict("12345678901234", Some("12345678901234")),
            TypedUpcVerdict::Refuse(UpcRefusal::TooLong)
        );
        // 13 characters fit, even irregular ones.
        assert_eq!(typed_upc_verdict("ééééééééééééé", None), TypedUpcVerdict::Confirm(CodeIrregularity::NotDigits));
        for loc in ["en", "fr", "de", "it"] {
            for r in [UpcRefusal::CheckDigit, UpcRefusal::TooLong, UpcRefusal::Isbn10] {
                assert!(!r.message(loc).contains("error."), "{loc}: {r:?}");
            }
        }
    }

    #[test]
    fn a_valid_isbn10_typed_as_upc_is_refused_towards_the_isbn_field() {
        for code in ["2070360245", "080442957X", "080442957x"] {
            assert_eq!(typed_upc_verdict(code, None), TypedUpcVerdict::Refuse(UpcRefusal::Isbn10), "{code}");
            assert_eq!(typed_upc_verdict(code, Some(code)), TypedUpcVerdict::Refuse(UpcRefusal::Isbn10), "{code}");
        }
        // A failed ISBN-10, with or without an `X`, keeps the scan's
        // 10-digit warning.
        for code in ["2205025062", "220500121X"] {
            assert_eq!(typed_upc_verdict(code, None), TypedUpcVerdict::Confirm(CodeIrregularity::TenDigits), "{code}");
        }
    }

    #[test]
    fn the_upc_checksum_refusal_is_for_12_digit_codes_only() {
        for loc in ["en", "fr", "de", "it"] {
            let m = TitleService::upc_checksum_refusal("036000291453", loc).expect(loc);
            assert!(!m.contains("error."), "{loc}: {m}");
        }
        assert_eq!(TitleService::upc_checksum_refusal("036000291452", "en"), None);
        for code in ["0093624738627", "12345678", "220500218"] {
            assert_eq!(TitleService::upc_checksum_refusal(code, "en"), None, "{code}");
        }
    }

    #[test]
    fn the_form_warning_carries_the_scan_warning_and_a_bound_confirmation() {
        let html = typed_upc_warning_html(CodeIrregularity::NotDigits, "12<b>", "en");
        assert!(html.contains("data-code-warning"), "{html}");
        assert!(html.contains(r#"name="upc_confirmed" value="12&lt;b&gt;""#), "{html}");
        assert!(html.contains("Keep 12&lt;b&gt; as entered"), "{html}");
        assert!(!html.contains("<b>"), "{html}");
    }
}
