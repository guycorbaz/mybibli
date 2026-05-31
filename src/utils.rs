/// Generate a STANDARD-base64-encoded 32-byte session token (44 chars
/// w/ padding). Canonical implementation for `sessions.token`. Earlier
/// versions of the codebase duplicated this function across
/// `routes/auth.rs`, `services/auth.rs`, and `middleware/auth.rs`; the
/// three copies drifted with comments and contracts. Centralized here
/// (the `utils` layer depends on nothing else inside the crate, so no
/// module dependency cycle is introduced).
///
/// STANDARD (not URL-safe) base64 matches the historic on-disk format;
/// the `+`/`/`/`=` chars get percent-encoded inside `Set-Cookie` values
/// and decoded by the session resolver on the way back in. CSRF tokens
/// use URL_SAFE_NO_PAD — see `generate_csrf_token` below.
pub fn generate_session_token() -> String {
    use base64::Engine;
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Generate a URL_SAFE_NO_PAD-base64-encoded 32-byte CSRF token (43
/// chars). Canonical implementation for `sessions.csrf_token`. Previously
/// duplicated across `middleware/auth.rs` (canonical) and re-exported
/// from `middleware/csrf.rs`; centralizing in `utils` lets callers
/// (`services/auth.rs`, `services/setup.rs`, `routes/auth.rs`) import
/// directly instead of routing through middleware modules.
pub fn generate_csrf_token() -> String {
    use base64::Engine;
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

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

/// Story 9-19 — bundles the strings each contextual-help tooltip surface
/// needs. Two ctors: `with_icon` (the typical help-icon button + tooltip
/// span pair) and `placeholder_only` (sr-only span only, for surfaces like
/// the catalog scan field where the input's existing `placeholder` IS the
/// visible affordance and aria-describedby covers the screen-reader layer).
///
/// Read by `templates/components/tooltip.html` via Askama's `{% let
/// tooltip = self.<surface>_help %}{% include %}` pattern. When `summary`
/// is `None`, the fragment renders only the hidden `<span>` (no
/// help-icon button).
pub struct TooltipData {
    pub id: String,
    pub summary: Option<String>,
    pub text: String,
}

impl TooltipData {
    /// Help-icon surface: both `summary` (icon aria-label) and `text` are
    /// rendered. The fragment emits the `<button class="help-icon">` plus
    /// the hidden `<span role="tooltip">`.
    pub fn with_icon(id: &str, summary: &str, text: &str) -> Self {
        Self {
            id: id.to_string(),
            summary: Some(summary.to_string()),
            text: text.to_string(),
        }
    }

    /// Placeholder-only surface: the fragment renders only a hidden
    /// `<span class="sr-only">` linked via `aria-describedby` from the
    /// input. No help-icon button is generated. Sighted users rely on
    /// the input's existing `placeholder` text for the visible hint.
    pub fn placeholder_only(id: &str, text: &str) -> Self {
        Self {
            id: id.to_string(),
            summary: None,
            text: text.to_string(),
        }
    }
}

/// Story 9-20 — bundles all 16 strings the cheat-sheet `<dialog>` and the
/// "Press ? for shortcuts" footer link need. One field per page-route
/// struct (mirror of 9-16's `ConnectionStatusContext` rollout); each
/// ctor populates the bundle from the request locale.
///
/// Read by `templates/layouts/base.html` via `{{ shortcuts_cheat_sheet.* }}`.
pub struct ShortcutsCheatSheetContext {
    pub heading: String,
    pub category_navigation: String,
    pub category_catalog: String,
    pub category_modal: String,
    pub shortcut_help: String,
    pub shortcut_escape: String,
    pub shortcut_go_home: String,
    pub shortcut_go_catalog: String,
    pub shortcut_go_loans: String,
    pub shortcut_go_borrowers: String,
    pub shortcut_go_admin: String,
    pub shortcut_focus_scan: String,
    pub shortcut_new_title: String,
    pub then_label: String,
    pub close_label: String,
    pub footer_link: String,
}

impl ShortcutsCheatSheetContext {
    pub fn new(loc: &str) -> Self {
        Self {
            heading: rust_i18n::t!("shortcuts.cheat_sheet.heading", locale = loc).to_string(),
            category_navigation: rust_i18n::t!("shortcuts.cheat_sheet.category_navigation", locale = loc).to_string(),
            category_catalog: rust_i18n::t!("shortcuts.cheat_sheet.category_catalog", locale = loc).to_string(),
            category_modal: rust_i18n::t!("shortcuts.cheat_sheet.category_modal", locale = loc).to_string(),
            shortcut_help: rust_i18n::t!("shortcuts.cheat_sheet.shortcut_help", locale = loc).to_string(),
            shortcut_escape: rust_i18n::t!("shortcuts.cheat_sheet.shortcut_escape", locale = loc).to_string(),
            shortcut_go_home: rust_i18n::t!("shortcuts.cheat_sheet.shortcut_go_home", locale = loc).to_string(),
            shortcut_go_catalog: rust_i18n::t!("shortcuts.cheat_sheet.shortcut_go_catalog", locale = loc).to_string(),
            shortcut_go_loans: rust_i18n::t!("shortcuts.cheat_sheet.shortcut_go_loans", locale = loc).to_string(),
            shortcut_go_borrowers: rust_i18n::t!("shortcuts.cheat_sheet.shortcut_go_borrowers", locale = loc).to_string(),
            shortcut_go_admin: rust_i18n::t!("shortcuts.cheat_sheet.shortcut_go_admin", locale = loc).to_string(),
            shortcut_focus_scan: rust_i18n::t!("shortcuts.cheat_sheet.shortcut_focus_scan", locale = loc).to_string(),
            shortcut_new_title: rust_i18n::t!("shortcuts.cheat_sheet.shortcut_new_title", locale = loc).to_string(),
            then_label: rust_i18n::t!("shortcuts.cheat_sheet.then_label", locale = loc).to_string(),
            close_label: rust_i18n::t!("shortcuts.cheat_sheet.close_label", locale = loc).to_string(),
            footer_link: rust_i18n::t!("shortcuts.footer_link", locale = loc).to_string(),
        }
    }
}

/// Issue #35 (v1.7.11 slice) — shared fields carried by every full-page
/// template struct. Today each handler re-builds these inline; this
/// helper centralizes the i18n key lookups and field construction so a
/// future addition (e.g. a feature-flag field, a new nav entry) is a
/// single-site edit instead of a 17-handler rewrite.
///
/// Askama does not support struct-flattening or partial-struct macros,
/// so per-handler conversion still spells out each field — but each
/// line becomes a short `nav_catalog: base.nav_catalog,` instead of a
/// verbose `rust_i18n::t!("nav.catalog", locale = loc).to_string()`.
/// The DRY win is on i18n key strings + connection_status / shortcuts
/// constructor calls, not on raw line count.
///
/// V1.7.11 ships the helper + 2 representative handler conversions
/// (`loans_page`, `borrowers_page`). Remaining ~14 handlers tracked in
/// a follow-up issue — incremental migration.
pub struct BaseContextFields {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub connection_status: ConnectionStatusContext,
    pub shortcuts_cheat_sheet: ShortcutsCheatSheetContext,
    pub session_timeout_secs: u64,
    pub csrf_token: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_wishlist: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub nav_menu_open: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
}

/// Build the shared fields for a full-page render. Caller provides the
/// page-specific `current_page` slug + the original request URI (used
/// for the language-toggle hidden input and the lang-toggle aria label),
/// plus the `session_timeout_secs` scalar (handler reads it off
/// `AppState.session_timeout_secs()` and passes it in — keeping the
/// helper pool-free so it stays unit-testable without a live DbPool).
pub fn base_context(
    session: &crate::middleware::auth::Session,
    locale: &str,
    current_page: &'static str,
    uri: &axum::http::Uri,
    session_timeout_secs: u64,
) -> BaseContextFields {
    BaseContextFields {
        lang: locale.to_string(),
        role: session.role.to_string(),
        current_page,
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = locale).to_string(),
        connection_status: ConnectionStatusContext::new(locale),
        shortcuts_cheat_sheet: ShortcutsCheatSheetContext::new(locale),
        session_timeout_secs,
        csrf_token: session.csrf_token.clone(),
        nav_catalog: rust_i18n::t!("nav.catalog", locale = locale).to_string(),
        nav_loans: rust_i18n::t!("nav.loans", locale = locale).to_string(),
        nav_wishlist: rust_i18n::t!("nav.wishlist", locale = locale).to_string(),
        nav_locations: rust_i18n::t!("nav.locations", locale = locale).to_string(),
        nav_series: rust_i18n::t!("nav.series", locale = locale).to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers", locale = locale).to_string(),
        nav_admin: rust_i18n::t!("nav.admin", locale = locale).to_string(),
        nav_login: rust_i18n::t!("nav.login", locale = locale).to_string(),
        nav_logout: rust_i18n::t!("nav.logout", locale = locale).to_string(),
        nav_menu_open: rust_i18n::t!("nav.menu_open", locale = locale).to_string(),
        current_url: current_url(uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = locale).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_session_token_length_is_44() {
        // 32 bytes → STANDARD base64 with padding → 44 chars. Locks the
        // wire format expected by the `sessions.token` schema (VARCHAR(64))
        // and by `tasks/anonymous_session_purge.rs` (which assumes 44-char
        // tokens when crafting LIKE patterns).
        assert_eq!(generate_session_token().len(), 44);
    }

    #[test]
    fn generate_session_token_is_unique() {
        assert_ne!(generate_session_token(), generate_session_token());
    }

    #[test]
    fn generate_csrf_token_length_is_43() {
        // 32 bytes → URL_SAFE_NO_PAD base64 → 43 chars (no `=` padding).
        // Matches the existing CSRF wire format read by
        // `templates/layouts/base.html` (the `<meta name="csrf-token">`
        // value) and `static/js/csrf.js` (the `X-CSRF-Token` header).
        assert_eq!(generate_csrf_token().len(), 43);
    }

    #[test]
    fn generate_csrf_token_is_unique() {
        assert_ne!(generate_csrf_token(), generate_csrf_token());
    }

    #[test]
    fn generate_csrf_token_is_url_safe_charset() {
        // Sanity: URL_SAFE base64 uses `[A-Za-z0-9_-]` only — no `+`/`/`/`=`.
        // A regression that swapped to STANDARD here would silently break
        // the `X-CSRF-Token` header on Set-Cookie + Cookie round-trips.
        let tok = generate_csrf_token();
        assert!(
            tok.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "CSRF token must use URL-safe charset: {tok:?}"
        );
    }

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

    // ─── #35 slice — base_context helper coverage ──────────────────

    fn test_uri() -> axum::http::Uri {
        "/some/path?q=test".parse().expect("valid uri")
    }

    #[test]
    fn base_context_carries_session_role_and_csrf_token() {
        let session = crate::middleware::auth::Session::anonymous_with_token(
            "csrf-token-abc".to_string(),
        );
        let ctx = base_context(&session, "en", "loans", &test_uri(), 7200);
        assert_eq!(ctx.role, "anonymous");
        assert_eq!(ctx.csrf_token, "csrf-token-abc");
        assert_eq!(ctx.current_page, "loans");
        assert_eq!(ctx.session_timeout_secs, 7200);
    }

    #[test]
    fn base_context_current_url_reflects_uri() {
        let session = crate::middleware::auth::Session::anonymous_with_token(String::new());
        let ctx = base_context(&session, "en", "x", &test_uri(), 0);
        assert_eq!(ctx.current_url, "/some/path?q=test");
    }

    #[test]
    fn base_context_resolves_i18n_keys_per_locale() {
        let session = crate::middleware::auth::Session::anonymous_with_token(String::new());
        let en = base_context(&session, "en", "x", &test_uri(), 0);
        let fr = base_context(&session, "fr", "x", &test_uri(), 0);
        // lang field reflects the requested locale.
        assert_eq!(en.lang, "en");
        assert_eq!(fr.lang, "fr");
        // The nav.* keys exist in every locale; the EN + FR strings differ
        // for at least one of them (nav.borrowers is "Borrowers" vs
        // "Emprunteurs"). If both came back identical, the i18n key
        // lookup is failing silently.
        assert_ne!(
            en.nav_borrowers, fr.nav_borrowers,
            "EN and FR translations must differ — check that locale files \
             ship the nav.borrowers key in both"
        );
        // skip_to_content + language_toggle_aria similarly differ.
        assert_ne!(en.skip_label, fr.skip_label);
        assert_ne!(en.lang_toggle_aria, fr.lang_toggle_aria);
    }

    #[test]
    fn base_context_populates_all_nav_fields() {
        // Regression guard: if a future refactor drops a nav field from
        // base_context, this test fails because the field would be empty.
        // Every nav.* i18n key exists in en.yml — silent miss would
        // surface as empty string here.
        let session = crate::middleware::auth::Session::anonymous_with_token(String::new());
        let ctx = base_context(&session, "en", "x", &test_uri(), 0);
        for (name, value) in [
            ("nav_catalog", &ctx.nav_catalog),
            ("nav_loans", &ctx.nav_loans),
            ("nav_wishlist", &ctx.nav_wishlist),
            ("nav_locations", &ctx.nav_locations),
            ("nav_series", &ctx.nav_series),
            ("nav_borrowers", &ctx.nav_borrowers),
            ("nav_admin", &ctx.nav_admin),
            ("nav_login", &ctx.nav_login),
            ("nav_logout", &ctx.nav_logout),
            ("nav_menu_open", &ctx.nav_menu_open),
        ] {
            assert!(!value.is_empty(), "{name} must not be empty");
            assert!(
                !value.contains('.'),
                "{name} resolved to the raw i18n key {value} — missing en.yml entry?"
            );
        }
    }
}
