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
}
