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

/// Render a FeedbackEntry HTML fragment for HTMX swap into `#feedback-list`.
///
/// Re-homed from `src/routes/catalog.rs` (#370 sub-item 2) — the prior
/// `pub fn feedback_html_pub` was a thin wrapper that crossed module
/// boundaries (the error layer depended on a `routes::catalog::` symbol
/// just to render its body). Centralizing in `utils` matches the layering
/// of `html_escape` above and removes the spurious dependency.
///
/// `variant` ∈ `{"success", "info", "warning", "error"}` (anything else
/// renders as `error`). `message` and `suggestion` are HTML-escaped here
/// — callers MUST NOT pre-escape. `suggestion = ""` suppresses the second
/// paragraph. `warning` / `error` carry a dismiss button bound to the
/// CSP-clean `data-action="dismiss-feedback"` delegated handler in
/// `static/js/mybibli.js`; `success` / `info` are auto-fade only.
pub fn feedback_html(variant: &str, message: &str, suggestion: &str) -> String {
    feedback_html_action(variant, message, suggestion, "")
}

/// Like [`feedback_html`] but renders an "Undo" affordance inside the entry
/// (polish-2 / #9). The button posts to `POST /catalog/undo` via HTMX —
/// CSRF is auto-injected by `static/js/csrf.js`; `static/js/mybibli.js`
/// disables it on click and removes it once the server-side undo window
/// elapses. CSP-clean: no inline JS, no `hx-confirm`. `undo_label` is
/// HTML-escaped here — callers MUST NOT pre-escape.
pub fn feedback_html_undoable(
    variant: &str,
    message: &str,
    suggestion: &str,
    undo_label: &str,
) -> String {
    let label = html_escape(undo_label);
    let button = format!(
        r##"<button type="button" data-action="undo-scan" hx-post="/catalog/undo" hx-target="#feedback-list" hx-swap="afterbegin" class="mt-2 inline-flex items-center gap-1 text-sm font-medium text-blue-600 dark:text-blue-400 hover:underline disabled:opacity-50 disabled:no-underline min-h-[44px] md:min-h-0" aria-label="{label}"><svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" d="M7.793 2.232a.75.75 0 01-.026 1.06L3.622 7.25h10.003a5.375 5.375 0 010 10.75H10.75a.75.75 0 010-1.5h2.875a3.875 3.875 0 000-7.75H3.622l4.145 3.957a.75.75 0 01-1.036 1.085l-5.5-5.25a.75.75 0 010-1.085l5.5-5.25a.75.75 0 011.062.026z" clip-rule="evenodd" /></svg>{label}</button>"##
    );
    feedback_html_action(variant, message, suggestion, &button)
}

/// Shared renderer behind [`feedback_html`] and [`feedback_html_undoable`].
/// `extra_action_html` is injected verbatim inside the entry body after the
/// suggestion paragraph — callers are responsible for its safety (the only
/// caller passes a server-built, escaped button).
fn feedback_html_action(
    variant: &str,
    message: &str,
    suggestion: &str,
    extra_action_html: &str,
) -> String {
    let (border_color, bg_color, icon_color, icon_path) = match variant {
        "success" => (
            "border-green-500",
            "bg-green-50 dark:bg-green-900/20",
            "text-green-600 dark:text-green-400",
            r#"<path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.857-9.809a.75.75 0 00-1.214-.882l-3.483 4.79-1.88-1.88a.75.75 0 10-1.06 1.061l2.5 2.5a.75.75 0 001.137-.089l4-5.5z" clip-rule="evenodd" />"#,
        ),
        "info" => (
            "border-blue-500",
            "bg-blue-50 dark:bg-blue-900/20",
            "text-blue-600 dark:text-blue-400",
            r#"<path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a.75.75 0 000 1.5h.253a.25.25 0 01.244.304l-.459 2.066A1.75 1.75 0 0010.747 15H11a.75.75 0 000-1.5h-.253a.25.25 0 01-.244-.304l.459-2.066A1.75 1.75 0 009.253 9H9z" clip-rule="evenodd" />"#,
        ),
        "warning" => (
            "border-amber-500",
            "bg-amber-50 dark:bg-amber-900/20",
            "text-amber-600 dark:text-amber-400",
            r#"<path fill-rule="evenodd" d="M8.485 2.495c.673-1.167 2.357-1.167 3.03 0l6.28 10.875c.673 1.167-.17 2.625-1.516 2.625H3.72c-1.347 0-2.189-1.458-1.515-2.625L8.485 2.495zM10 5a.75.75 0 01.75.75v3.5a.75.75 0 01-1.5 0v-3.5A.75.75 0 0110 5zm0 9a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd" />"#,
        ),
        _ => (
            "border-red-500",
            "bg-red-50 dark:bg-red-900/20",
            "text-red-600 dark:text-red-400",
            r#"<path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.28 7.22a.75.75 0 00-1.06 1.06L8.94 10l-1.72 1.72a.75.75 0 101.06 1.06L10 11.06l1.72 1.72a.75.75 0 101.06-1.06L11.06 10l1.72-1.72a.75.75 0 00-1.06-1.06L10 8.94 8.28 7.22z" clip-rule="evenodd" />"#,
        ),
    };

    let suggestion_html = if suggestion.is_empty() {
        String::new()
    } else {
        format!(
            r#"<p class="text-sm text-stone-500 dark:text-stone-400 mt-1">{}</p>"#,
            html_escape(suggestion)
        )
    };

    let dismiss_html = if variant == "warning" || variant == "error" {
        r#"<button type="button" class="text-stone-500 dark:text-stone-400 hover:text-stone-600 dark:hover:text-stone-200 p-1 min-w-[44px] min-h-[44px] md:min-w-[36px] md:min-h-[36px] flex items-center justify-center" aria-label="Dismiss" data-action="dismiss-feedback"><svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z" /></svg></button>"#
    } else {
        ""
    };

    format!(
        r#"<div class="p-3 border-l-4 {} {} rounded-r feedback-entry" role="status" data-feedback-variant="{}">
  <div class="flex items-start gap-2">
    <svg class="{} w-5 h-5 flex-shrink-0 mt-0.5" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">{}</svg>
    <div class="flex-1">
      <p class="text-stone-700 dark:text-stone-300">{}</p>
      {}
      {}
    </div>
    {}
  </div>
</div>"#,
        border_color,
        bg_color,
        variant,
        icon_color,
        icon_path,
        html_escape(message),
        suggestion_html,
        extra_action_html,
        dismiss_html
    )
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

/// Issue #35 — shared fields carried by every full-page template struct,
/// centralizing the i18n key lookups + `connection_status` / `shortcuts`
/// constructor calls so a future global addition (a feature-flag field, a
/// new nav entry) is a single-site edit here instead of a per-handler
/// rewrite.
///
/// Issue #398 — page-template structs now **embed** this as a single
/// `base: BaseContextFields` field instead of flattening its ~20 fields,
/// and `layouts/base.html` + `components/nav_bar.html` read them via
/// `{{ base.lang }}` etc. (Askama nested-field access). Adding a
/// document-wide field is now one line in this struct + one in
/// `base_context()` — zero per-page churn. The handler still calls
/// `base_context()` and assigns the result as the page struct's `base`
/// field via field-init shorthand (`base,`).
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
    /// Issue #386 — JSON i18n bundle for client-side JS modules, emitted
    /// by `layouts/base.html` as a `<script type="application/json"
    /// id="i18n-bundle">` data island. See [`build_js_i18n_bundle`].
    pub js_i18n: String,
}

/// Issue #386 — build the per-request i18n bundle that
/// `layouts/base.html` emits as a `<script type="application/json"
/// id="i18n-bundle">` data island. Client JS reads it once via
/// `JSON.parse(document.getElementById("i18n-bundle").textContent)`
/// instead of carrying hand-synced `{en: …, fr: …}` objects (which
/// silently drifted from `locales/*.yml` and never covered de/it).
///
/// Strings are resolved server-side in the REQUEST locale, so JS picks
/// up de/it for free. `%{minutes}`-style placeholders are preserved
/// verbatim for client-side substitution.
///
/// `<`, `>`, and `&` in the serialized JSON are replaced with their
/// `\uXXXX` escapes so a translated value containing `</script>` (or
/// `<!--`) cannot break out of the data island — defense in depth on top
/// of `tests/locale_html_safety.rs` (which already refuses markup in
/// locale values).
pub fn build_js_i18n_bundle(locale: &str) -> String {
    let bundle = serde_json::json!({
        "session": {
            "expiry_soon": rust_i18n::t!("session.expiry_soon", locale = locale).to_string(),
            "expiry_in_minutes": rust_i18n::t!("session.expiry_in_minutes", locale = locale).to_string(),
            "stay_connected": rust_i18n::t!("session.stay_connected", locale = locale).to_string(),
            "dismiss_aria": rust_i18n::t!("session.dismiss_aria", locale = locale).to_string(),
        },
        // Issue #403 — the two modules that still carried hand-synced
        // {en, fr} objects after the #386 PoC. `%{status}` is preserved
        // verbatim for client-side substitution (same contract as
        // `%{minutes}` above).
        "inline_form": {
            "modal_busy": rust_i18n::t!("inline_form.modal_busy", locale = locale).to_string(),
        },
        "errors": {
            "server_error_retry": rust_i18n::t!("error.server_error_retry", locale = locale).to_string(),
        }
    });
    serde_json::to_string(&bundle)
        .unwrap_or_else(|_| "{}".to_string())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
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
        js_i18n: build_js_i18n_bundle(locale),
    }
}

/// Test-only constructor for [`BaseContextFields`]. Issue #398 collapsed
/// the per-test flat base-field blocks (lang / role / nav_* / …) into this
/// single call: callers pass the role, current_page slug, and
/// session-timeout that matter to their assertions; the nav labels +
/// skip_label + lang_toggle_aria resolve from the real `en` locale (so a
/// label tweak doesn't silently desync a hand-copied test literal),
/// csrf_token is the conventional `"tok"` stub, and current_url is `"/"`
/// (no test asserts on it — it only feeds the nav language-toggle form).
#[cfg(test)]
pub(crate) fn test_base_context(
    role: &str,
    current_page: &'static str,
    session_timeout_secs: u64,
) -> BaseContextFields {
    let locale = "en";
    BaseContextFields {
        lang: locale.to_string(),
        role: role.to_string(),
        current_page,
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = locale).to_string(),
        connection_status: ConnectionStatusContext::new(locale),
        shortcuts_cheat_sheet: ShortcutsCheatSheetContext::new(locale),
        session_timeout_secs,
        csrf_token: "tok".to_string(),
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
        current_url: "/".to_string(),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = locale).to_string(),
        js_i18n: build_js_i18n_bundle(locale),
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

    // ─── #386 — JS i18n bundle ─────────────────────────────────────

    #[test]
    fn js_i18n_bundle_carries_session_keys_as_valid_json() {
        let json = build_js_i18n_bundle("en");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("bundle must be valid JSON");
        let session = &parsed["session"];
        for key in [
            "expiry_soon",
            "expiry_in_minutes",
            "stay_connected",
            "dismiss_aria",
        ] {
            assert!(
                session[key].as_str().is_some_and(|s| !s.is_empty()),
                "session.{key} must be a non-empty string in the bundle"
            );
        }
        // The parameterized string keeps its client-substituted placeholder.
        assert!(
            session["expiry_in_minutes"]
                .as_str()
                .unwrap()
                .contains("%{minutes}"),
            "expiry_in_minutes must preserve the %{{minutes}} placeholder for client-side substitution"
        );
    }

    #[test]
    fn js_i18n_bundle_resolves_per_locale() {
        let en = build_js_i18n_bundle("en");
        let de = build_js_i18n_bundle("de");
        // de/it were the gap the hand-synced JS objects never covered —
        // prove the server-side bundle resolves them and differs from en.
        let en_v: serde_json::Value = serde_json::from_str(&en).unwrap();
        let de_v: serde_json::Value = serde_json::from_str(&de).unwrap();
        assert_ne!(
            en_v["session"]["stay_connected"], de_v["session"]["stay_connected"],
            "de bundle must carry the German string, not the English fallback"
        );
    }

    // ─── #403 — i18n-bundle sweep (inline-form.js + mybibli.js) ───

    #[test]
    fn js_i18n_bundle_carries_403_sweep_keys() {
        let json = build_js_i18n_bundle("en");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("bundle must be valid JSON");
        assert!(
            parsed["inline_form"]["modal_busy"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            "inline_form.modal_busy must be a non-empty string in the bundle"
        );
        let retry = parsed["errors"]["server_error_retry"]
            .as_str()
            .expect("errors.server_error_retry must be a string");
        assert!(!retry.is_empty());
        // The parameterized string keeps its client-substituted placeholder.
        assert!(
            retry.contains("%{status}"),
            "server_error_retry must preserve the %{{status}} placeholder for client-side substitution"
        );
    }

    #[test]
    fn js_i18n_bundle_403_keys_resolve_in_de_and_it() {
        // de/it copy is the net-new content of #403 — prove both resolve
        // to a translated string, not the English fallback or the raw key.
        let en: serde_json::Value =
            serde_json::from_str(&build_js_i18n_bundle("en")).unwrap();
        for loc in ["de", "it"] {
            let v: serde_json::Value =
                serde_json::from_str(&build_js_i18n_bundle(loc)).unwrap();
            for (a, b) in [
                (&v["inline_form"]["modal_busy"], &en["inline_form"]["modal_busy"]),
                (
                    &v["errors"]["server_error_retry"],
                    &en["errors"]["server_error_retry"],
                ),
            ] {
                assert_ne!(a, b, "{loc} bundle must differ from en");
                assert!(
                    !a.as_str().unwrap().contains("inline_form.")
                        && !a.as_str().unwrap().contains("error."),
                    "{loc} value must not be a raw i18n key: {a}"
                );
            }
        }
    }

    #[test]
    fn js_i18n_bundle_escapes_angle_brackets_for_safe_embedding() {
        // The serialized bundle must never contain a literal `<` or `>` —
        // they're \uXXXX-escaped so a future translated value containing
        // `</script>` can't break out of the data island.
        let json = build_js_i18n_bundle("en");
        assert!(
            !json.contains('<') && !json.contains('>'),
            "bundle must escape < and > to \\u003c / \\u003e: {json}"
        );
    }
}
