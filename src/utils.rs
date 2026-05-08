/// Return `/path` or `/path?query` from an `axum::http::Uri`, stripping the
/// scheme, host, and fragment. Used to populate the `current_url` hidden
/// field on the language-toggle form (story 7-3 AC 8) so clicking FR/EN
/// returns the user to the exact same path + query.
///
/// Pass `OriginalUri` (not the plain `Uri` extractor) — in nested routers
/// the plain `Uri` returns the post-nest sub-path, while `OriginalUri`
/// preserves the full request path.
pub fn current_url(uri: &axum::http::Uri) -> String {
    match uri.query() {
        Some(q) if !q.is_empty() => format!("{}?{}", uri.path(), q),
        _ => uri.path().to_string(),
    }
}

/// Percent-encode a string for use in URL query parameter values.
pub fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

/// Escape HTML special characters to prevent XSS.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Locale-aware percentage formatter (story 9-3).
///
/// EN: `"33.3%"` — period decimal, no space before `%`.
/// FR: `"33,3 %"` with a non-breaking space (`U+00A0`) before the `%` —
/// French typography requires NBSP between a number and any unit, not a
/// regular space (which would allow a line break between the number and
/// the unit). The `_uses_nbsp` test guards against that silent regression.
///
/// One decimal is always emitted (`100.0%`, never `100%`) for visual row
/// alignment; the dashboard rows scan more cleanly when the decimals line
/// up. Other locales fall back to the EN format until v2 broadens i18n.
pub fn format_percent(value: f64, locale: &str) -> String {
    let s = format!("{:.1}", value);
    match locale {
        "fr" => format!("{}\u{00A0}%", s.replace('.', ",")),
        _ => format!("{}%", s),
    }
}

/// Story 9-16 — base-layout connection-lost overlay i18n bundle.
///
/// Page templates that extend `layouts/base.html` carry the 4 strings
/// the overlay needs (heading, body, retry button, restored toast).
/// Bundled into a single struct field on each page-context struct so
/// per-page ctors gain ONE line (`connection_status:
/// ConnectionStatusContext::new(loc)`) instead of four — keeps the
/// blast radius across ~20 page structs minimal.
///
/// Read by `templates/layouts/base.html` via Askama's nested-field
/// access (`{{ connection_status.lost_heading }}` etc.). The
/// `restored_toast` string is also exposed as a `data-i18n-restored-
/// toast` attribute on the overlay div for `static/js/connection-
/// monitor.js` to read when it spawns the on-success toast.
pub struct ConnectionStatusContext {
    pub lost_heading: String,
    pub lost_body: String,
    pub lost_retry: String,
    pub restored_toast: String,
}

impl ConnectionStatusContext {
    pub fn new(loc: &str) -> Self {
        Self {
            lost_heading: rust_i18n::t!("connection.lost_heading", locale = loc).to_string(),
            lost_body: rust_i18n::t!("connection.lost_body", locale = loc).to_string(),
            lost_retry: rust_i18n::t!("connection.lost_retry", locale = loc).to_string(),
            restored_toast: rust_i18n::t!("connection.restored_toast", locale = loc).to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_url_path_only() {
        let uri: axum::http::Uri = "/catalog".parse().unwrap();
        assert_eq!(current_url(&uri), "/catalog");
    }

    #[test]
    fn test_current_url_with_query() {
        let uri: axum::http::Uri = "/catalog?q=tintin&sort=title".parse().unwrap();
        assert_eq!(current_url(&uri), "/catalog?q=tintin&sort=title");
    }

    #[test]
    fn test_current_url_root() {
        let uri: axum::http::Uri = "/".parse().unwrap();
        assert_eq!(current_url(&uri), "/");
    }

    #[test]
    fn test_current_url_empty_query_drops_question_mark() {
        let uri: axum::http::Uri = "/foo".parse().unwrap();
        assert_eq!(current_url(&uri), "/foo");
    }

    #[test]
    fn test_url_encode_simple() {
        assert_eq!(url_encode("hello"), "hello");
    }

    #[test]
    fn test_url_encode_spaces() {
        assert_eq!(url_encode("hello world"), "hello%20world");
    }

    #[test]
    fn test_url_encode_ampersand() {
        assert_eq!(url_encode("rock&roll"), "rock%26roll");
    }

    #[test]
    fn test_url_encode_special() {
        assert_eq!(url_encode("a=b&c=d"), "a%3Db%26c%3Dd");
    }

    #[test]
    fn test_html_escape_special_chars() {
        assert_eq!(
            html_escape("<script>alert('xss')</script>"),
            "&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;"
        );
    }

    #[test]
    fn test_html_escape_ampersand() {
        assert_eq!(html_escape("Tom & Jerry"), "Tom &amp; Jerry");
    }

    #[test]
    fn test_html_escape_quotes() {
        assert_eq!(
            html_escape(r#"He said "hello""#),
            "He said &quot;hello&quot;"
        );
    }

    #[test]
    fn test_html_escape_clean_string() {
        assert_eq!(html_escape("Hello World"), "Hello World");
    }

    #[test]
    fn test_html_escape_empty() {
        assert_eq!(html_escape(""), "");
    }

    #[test]
    fn format_percent_en_basic() {
        assert_eq!(format_percent(33.3, "en"), "33.3%");
    }

    #[test]
    fn format_percent_fr_basic() {
        assert_eq!(format_percent(33.3, "fr"), "33,3\u{00A0}%");
    }

    /// AC9 NBSP invariant: French typography requires `U+00A0` between the
    /// digit run and the `%` sign (not a regular U+0020 space). A future
    /// "simplification" that swaps `\u{00A0}` for a regular space would
    /// allow visual line-wrap between the number and the unit — wrong.
    #[test]
    fn format_percent_fr_uses_nbsp() {
        let s = format_percent(50.0, "fr");
        let bytes = s.as_bytes();
        // The character before the trailing '%' must be NBSP (U+00A0,
        // encoded as 2 bytes 0xC2 0xA0 in UTF-8), NOT a regular space.
        let pct_pos = s.rfind('%').expect("percent sign present");
        // pct_pos is a byte index pointing at '%'; the two preceding bytes
        // are the UTF-8 encoding of NBSP.
        assert!(
            pct_pos >= 2,
            "string too short to carry NBSP before '%': {s:?}"
        );
        assert_eq!(
            &bytes[pct_pos - 2..pct_pos],
            &[0xC2, 0xA0],
            "expected NBSP (0xC2 0xA0) immediately before '%' in {s:?}"
        );
        // Negative assertion: the byte right before '%' must NOT be a
        // regular ASCII space (0x20) — proves the previous assertion is
        // not satisfied accidentally by some other 2-byte sequence.
        assert_ne!(bytes[pct_pos - 1], 0x20);
    }

    #[test]
    fn format_percent_one_decimal_kept_en() {
        assert_eq!(format_percent(100.0, "en"), "100.0%");
        assert_eq!(format_percent(0.0, "en"), "0.0%");
    }

    #[test]
    fn format_percent_one_decimal_kept_fr() {
        assert_eq!(format_percent(100.0, "fr"), "100,0\u{00A0}%");
    }

    #[test]
    fn format_percent_rounds_to_one_decimal() {
        // 1/3 → 33.333... → 33.3
        assert_eq!(format_percent((1.0 / 3.0) * 100.0, "en"), "33.3%");
        // 2/3 → 66.666... → 66.7
        assert_eq!(format_percent((2.0 / 3.0) * 100.0, "en"), "66.7%");
        assert_eq!(format_percent((2.0 / 3.0) * 100.0, "fr"), "66,7\u{00A0}%");
    }

    #[test]
    fn format_percent_unknown_locale_falls_back_to_en() {
        assert_eq!(format_percent(42.5, "de"), "42.5%");
        assert_eq!(format_percent(42.5, ""), "42.5%");
    }
}
