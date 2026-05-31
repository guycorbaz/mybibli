//! #29 sub-item 2 — defense-in-depth: locale values MUST NOT contain
//! raw HTML tags.
//!
//! Several handlers interpolate `rust_i18n::t!()` output directly into
//! `format!("…<h2>{}…", t!(...))` strings (catalog, titles, locations,
//! etc.). The values come from translator-controlled YAML files
//! committed to the repo, so the practical XSS surface today is zero —
//! a malicious translator would need a commit + review + merge.
//!
//! This audit is the belt for that braces: every entry in every locale
//! YAML is asserted to contain no HTML tag start. If a future
//! translator (or an unwitting copy-paste from an HTML mockup) lands
//! `<script>` or `<a href>` in a locale string, CI fails before the
//! string ever reaches a production format!() context.
//!
//! Rule:
//!   - `<` immediately followed by an ASCII letter, `/`, or `!` is a
//!     tag start and is REJECTED.
//!   - `&` followed by alpha/digit and a `;` later in the string is an
//!     HTML entity reference and is REJECTED.
//!   - Bare `<` / `>` / `&` are allowed (legitimate French apostrophes,
//!     punctuation, percent placeholders like `%{name}`).
//!
//! `rust_i18n::t!(...)` placeholders (`%{name}`) are interpolated AT
//! CALL TIME with caller-supplied values; this audit covers the static
//! template strings only.

use serde_yaml_ng::Value;
use std::path::Path;

const LOCALES: &[&str] = &["en", "fr", "de", "it"];

/// Dotted YAML paths whose values are KNOWN to contain HTML / pseudo-
/// HTML and have been audited as safe. The audit allows these specific
/// paths through; every other path is rejected.
///
/// Adding a new entry here is a deliberate "yes, this string is HTML
/// and the caller renders it raw" decision. Reviewers should require:
///   1. The string only uses an inline-emphasis whitelist
///      (`<em>`, `<strong>`, `<br>`, `<code>`, `<a>`), OR
///   2. The `<…>` is documentation prose (e.g. `<key>` as a placeholder
///      stand-in), AND
///   3. The render path is documented to bypass HTML escaping
///      intentionally.
const ALLOWED_HTML_KEYS: &[&str] = &[
    // feedback.volume_confirm.body — UX-DR8 modal body uses `<em>` for
    // title emphasis. Rendered through `Html()` in the catalog handler
    // so the `<em>` produces actual italic, not literal angle brackets.
    "feedback.volume_confirm.body",
    // admin.api_keys.tooltip_text — `<key>` is documentation prose
    // (placeholder for the user's actual API key value). Rendered
    // through Askama auto-escape so the `<` becomes `&lt;` and the
    // user sees literal `<key>`.
    "admin.api_keys.tooltip_text",
];

fn locales_root() -> &'static Path {
    Path::new("./locales")
}

fn looks_like_html_tag_start(s: &str) -> Option<String> {
    // `<` followed by ASCII letter / `/` / `!` — tag start, doctype,
    // closing tag, or comment open. Refuse all of them.
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'<' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next.is_ascii_alphabetic() || next == b'/' || next == b'!' {
                return Some(format!(
                    "raw HTML tag start `<{}` at byte offset {}",
                    next as char, i
                ));
            }
        }
    }
    None
}

fn looks_like_html_entity_ref(s: &str) -> Option<String> {
    // `&` followed by [a-zA-Z0-9#] and a `;` within the next 8 chars
    // matches `&amp;`, `&lt;`, `&#39;`, `&#x27;`, etc. Refuse so the
    // translator writes literal `&` instead and the rendering layer
    // (Askama auto-escape or `html_escape`) handles encoding.
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'&' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            let entity_lookahead = next.is_ascii_alphanumeric() || next == b'#';
            if entity_lookahead {
                let end = (i + 8).min(bytes.len());
                for j in i + 2..end {
                    if bytes[j] == b';' {
                        let entity = std::str::from_utf8(&bytes[i..=j])
                            .unwrap_or("<non-utf8>")
                            .to_string();
                        return Some(format!(
                            "HTML entity reference `{entity}` at byte offset {i}"
                        ));
                    }
                }
            }
        }
    }
    None
}

fn check_value(path: &str, value: &Value, violations: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            if ALLOWED_HTML_KEYS.contains(&path) {
                return;
            }
            if let Some(reason) = looks_like_html_tag_start(s) {
                violations.push(format!("{path}: {reason}: {s:?}"));
            } else if let Some(reason) = looks_like_html_entity_ref(s) {
                violations.push(format!("{path}: {reason}: {s:?}"));
            }
        }
        Value::Mapping(map) => {
            for (k, v) in map {
                let key_str = k.as_str().unwrap_or("?").to_string();
                let next_path = if path.is_empty() {
                    key_str
                } else {
                    format!("{path}.{key_str}")
                };
                check_value(&next_path, v, violations);
            }
        }
        Value::Sequence(seq) => {
            for (i, v) in seq.iter().enumerate() {
                let next_path = format!("{path}[{i}]");
                check_value(&next_path, v, violations);
            }
        }
        _ => {}
    }
}

#[test]
fn no_locale_value_contains_raw_html_tags_or_entities() {
    let mut all_violations: Vec<String> = Vec::new();

    for locale in LOCALES {
        let path = locales_root().join(format!("{locale}.yml"));
        let raw =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        let parsed: Value = serde_yaml_ng::from_str(&raw)
            .unwrap_or_else(|e| panic!("parse {path:?}: {e}"));
        let mut local: Vec<String> = Vec::new();
        check_value("", &parsed, &mut local);
        if !local.is_empty() {
            all_violations.push(format!("\n=== {locale}.yml ==="));
            all_violations.extend(local);
        }
    }

    assert!(
        all_violations.is_empty(),
        "locale strings must not contain raw HTML tags or entity references — \
         translators should write literal `<` / `&` / `>` and the rendering \
         layer (Askama auto-escape or `crate::utils::html_escape`) encodes \
         them. Violations:\n{}",
        all_violations.join("\n"),
    );
}

#[test]
fn audit_logic_recognizes_tag_starts() {
    // Self-tests for the audit predicates.
    assert!(looks_like_html_tag_start("<a>link</a>").is_some());
    assert!(looks_like_html_tag_start("<script>").is_some());
    assert!(looks_like_html_tag_start("</p>").is_some());
    assert!(looks_like_html_tag_start("<!DOCTYPE>").is_some());

    // Bare `<` / `>` are allowed (math, French quotes, etc.).
    assert!(looks_like_html_tag_start("a < b").is_none());
    assert!(looks_like_html_tag_start("«hello»").is_none());

    // Percent placeholders look like `%{name}` — must NOT be confused
    // with a tag start.
    assert!(looks_like_html_tag_start("Hello %{name}").is_none());
}

#[test]
fn audit_logic_recognizes_entity_refs() {
    assert!(looks_like_html_entity_ref("Tom &amp; Jerry").is_some());
    assert!(looks_like_html_entity_ref("&#39;").is_some());
    assert!(looks_like_html_entity_ref("&lt;").is_some());

    // Bare `&` is allowed (Tom & Jerry, etc.).
    assert!(looks_like_html_entity_ref("Tom & Jerry").is_none());
    assert!(looks_like_html_entity_ref("R&D").is_none());
}
