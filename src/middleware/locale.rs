//! Request-scoped locale middleware.
//!
//! Resolves the UI language for each incoming request via
//! [`crate::i18n::resolve_locale`] (query > cookie > user preference >
//! `Accept-Language` > default `"fr"`) and inserts `Extension(Locale(…))`
//! into the request so downstream handlers can render i18n strings via the
//! keyed form `t!("key", locale = locale.0, …)`.
//!
//! Why not `rust_i18n::set_locale` inside the middleware? `rust_i18n` stores
//! the locale in a process-global `AtomicPtr` — under tokio's multi-threaded
//! scheduler, concurrent requests race and can swap locales mid-template.
//! Keyed `t!(..., locale = …)` sidesteps the global. See story 7-3 Dev Notes
//! § "rust_i18n wiring decision".

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

use crate::AppState;
use crate::i18n::resolve_locale;

/// Request-scoped locale, inserted as an axum `Extension` by
/// [`locale_resolve_middleware`]. Only `"fr"` and `"en"` are possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locale(pub &'static str);

/// Axum middleware that populates `Extension<Locale>`.
///
/// Uses `from_fn_with_state` so the handler can reach `AppState.pool` without
/// relying on the request-extension `DbPool` that only the catalog sub-router
/// installs today.
pub async fn locale_resolve_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let query_lang: Option<String> = request.uri().query().and_then(parse_lang_query);

    // HTTP/2 and some proxies may split cookies across multiple `Cookie:`
    // headers. Concatenate all of them so our cookie parser sees every pair —
    // matches the behavior of `CookieJar` used by `src/routes/auth.rs`.
    let cookie_header: String = request
        .headers()
        .get_all("cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect::<Vec<&str>>()
        .join("; ");

    let cookie_lang: Option<String> = extract_named_cookie_value("lang")(&cookie_header);
    let session_token: Option<String> = extract_named_cookie_value("session")(&cookie_header);

    // Pattern A (per story 7-3 Task 3): narrow duplicate DB lookup for
    // preferred_language when a session cookie is present. Cheap (single
    // indexed SELECT on users by session token) and keeps the locale layer
    // decoupled from the Session extractor. Mirrors the Session extractor's
    // `last_activity` timeout gate so an expired session cannot keep feeding
    // its owner's stored locale after the auth layer has downgraded the user
    // to anonymous.
    let timeout_secs = state.session_timeout_secs();
    let user_pref: Option<String> = if let Some(token) = session_token.as_deref() {
        fetch_preferred_language(&state.pool, token, timeout_secs).await
    } else {
        None
    };

    let accept_language: Option<String> = request
        .headers()
        .get_all("accept-language")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .next()
        .map(str::to_string);

    let locale = resolve_locale(
        query_lang.as_deref(),
        cookie_lang.as_deref(),
        user_pref.as_deref(),
        accept_language.as_deref(),
        "fr",
    );

    request.extensions_mut().insert(Locale(locale));
    next.run(request).await
}

/// Parse `lang=fr` (or `lang=en`) out of a URI query string, returning only the
/// raw value — validation is delegated to [`resolve_locale`]'s fallthrough.
fn parse_lang_query(query: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("lang=")
            && !value.is_empty()
        {
            // Percent-decoding is not needed for `fr`/`en` ASCII values, and a
            // malformed value just falls through to the next slot anyway.
            return Some(value.to_string());
        }
    }
    None
}

/// Return a closure that extracts the value of a named cookie from a `Cookie`
/// header value. Handles multiple semicolon-separated pairs, trims whitespace,
/// and ignores empty values. Returns `None` if the cookie is absent.
fn extract_named_cookie_value(name: &'static str) -> impl Fn(&str) -> Option<String> {
    move |header: &str| {
        let prefix = format!("{name}=");
        for part in header.split(';') {
            let trimmed = part.trim();
            if let Some(value) = trimmed.strip_prefix(&prefix)
                && !value.is_empty()
            {
                return Some(value.to_string());
            }
        }
        None
    }
}

/// Look up `users.preferred_language` for the user that owns `session_token`.
/// Returns `None` on any failure (no session, expired, DB error, NULL column).
/// Applies the same `last_activity`-based expiry gate as the `Session`
/// extractor so an expired session no longer lifts its owner's stored locale.
async fn fetch_preferred_language(
    pool: &crate::db::DbPool,
    session_token: &str,
    timeout_secs: u64,
) -> Option<String> {
    type PrefRow = (Option<String>, chrono::DateTime<chrono::Utc>);
    let row: Result<Option<PrefRow>, sqlx::Error> = sqlx::query_as(
        "SELECT u.preferred_language, s.last_activity FROM sessions s \
         JOIN users u ON s.user_id = u.id \
         WHERE s.token = ? AND s.deleted_at IS NULL AND u.deleted_at IS NULL",
    )
    .bind(session_token)
    .fetch_optional(pool)
    .await;
    match row {
        Ok(Some((lang, last_activity))) => {
            let now = chrono::Utc::now();
            if crate::models::session::SessionModel::is_expired(last_activity, now, timeout_secs) {
                None
            } else {
                lang
            }
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, "locale middleware: preferred_language lookup failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lang_query_finds_lang_only() {
        assert_eq!(parse_lang_query("lang=en"), Some("en".to_string()));
    }

    #[test]
    fn parse_lang_query_finds_lang_among_others() {
        assert_eq!(
            parse_lang_query("q=tintin&lang=fr&sort=title"),
            Some("fr".to_string())
        );
    }

    #[test]
    fn parse_lang_query_returns_none_when_absent() {
        assert_eq!(parse_lang_query("q=tintin&sort=title"), None);
        assert_eq!(parse_lang_query(""), None);
    }

    #[test]
    fn parse_lang_query_returns_raw_garbage_for_fallthrough() {
        // `resolve_locale` normalizes; the parser just extracts the raw value.
        assert_eq!(parse_lang_query("lang=xx"), Some("xx".to_string()));
        assert_eq!(parse_lang_query("lang="), None);
    }

    #[test]
    fn extract_named_cookie_reads_lang() {
        let f = extract_named_cookie_value("lang");
        assert_eq!(f("lang=en"), Some("en".to_string()));
        assert_eq!(f("session=abc; lang=fr"), Some("fr".to_string()));
        assert_eq!(f("session=abc; lang=fr; theme=dark"), Some("fr".to_string()));
    }

    #[test]
    fn extract_named_cookie_reads_session_token() {
        let f = extract_named_cookie_value("session");
        assert_eq!(f("session=abc123"), Some("abc123".to_string()));
        assert_eq!(f("lang=fr; session=xyz"), Some("xyz".to_string()));
    }

    #[test]
    fn extract_named_cookie_returns_none_when_missing_or_empty() {
        let f = extract_named_cookie_value("lang");
        assert_eq!(f(""), None);
        assert_eq!(f("session=abc"), None);
        assert_eq!(f("lang="), None);
    }

    #[test]
    fn extract_named_cookie_does_not_match_prefix_collision() {
        let f = extract_named_cookie_value("lang");
        // `language_pref=foo` must not match as `lang=uage_pref=foo`.
        assert_eq!(f("language_pref=foo; other=bar"), None);
    }
}

// ─── Integration-style middleware tests ────────────────────────
//
// These drive the middleware via tower::oneshot against a tiny router so we
// cover the full 5-slot precedence chain end-to-end without spinning a real
// HTTP server. Requires a DB pool — uses #[sqlx::test] to get a fresh DB.

#[cfg(test)]
mod middleware_integration_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::{Extension, Router};
    use tower::ServiceExt;

    // Bare helper: returns the resolved locale as the response body so the
    // test can assert on it.
    async fn handler(Extension(locale): Extension<Locale>) -> impl IntoResponse {
        (StatusCode::OK, locale.0)
    }

    fn router(state: AppState) -> Router {
        Router::new()
            .route("/probe", get(handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                locale_resolve_middleware,
            ))
            .with_state(state)
    }

    fn test_state(pool: crate::db::DbPool) -> AppState {
        use std::sync::{Arc, RwLock};
        AppState {
            pool,
            settings: Arc::new(RwLock::new(crate::config::AppSettings::default())),
            http_client: reqwest::Client::new(),
            registry: Arc::new(crate::metadata::registry::ProviderRegistry::new()),
            covers_dir: std::path::PathBuf::from("/tmp"),
            provider_health: crate::tasks::provider_health::new_provider_health_map(),
            mariadb_version_cache: crate::services::admin_health::new_mariadb_version_cache(),
        }
    }

    async fn resolved_locale(app: Router, req: HttpRequest<Body>) -> String {
        let res = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        String::from_utf8(body.to_vec()).unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn query_param_wins_over_cookie_and_accept_language(pool: crate::db::DbPool) {
        let app = router(test_state(pool));
        let req = HttpRequest::builder()
            .uri("/probe?lang=en")
            .header("cookie", "lang=fr")
            .header("accept-language", "fr-CH")
            .body(Body::empty())
            .unwrap();
        assert_eq!(resolved_locale(app, req).await, "en");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn cookie_wins_over_accept_language(pool: crate::db::DbPool) {
        let app = router(test_state(pool));
        let req = HttpRequest::builder()
            .uri("/probe")
            .header("cookie", "lang=en")
            .header("accept-language", "fr-CH")
            .body(Body::empty())
            .unwrap();
        assert_eq!(resolved_locale(app, req).await, "en");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn accept_language_wins_when_no_cookie(pool: crate::db::DbPool) {
        let app = router(test_state(pool));
        let req = HttpRequest::builder()
            .uri("/probe")
            .header("accept-language", "en-US,fr;q=0.5")
            .body(Body::empty())
            .unwrap();
        assert_eq!(resolved_locale(app, req).await, "en");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn defaults_to_fr_when_no_signal(pool: crate::db::DbPool) {
        let app = router(test_state(pool));
        let req = HttpRequest::builder()
            .uri("/probe")
            .body(Body::empty())
            .unwrap();
        assert_eq!(resolved_locale(app, req).await, "fr");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn stored_user_pref_wins_over_accept_language(pool: crate::db::DbPool) {
        // Seed a user with preferred_language='en' + a valid session token
        // and expect the middleware to lift that preference over an FR
        // Accept-Language header.
        // VARCHAR(44) — fixture token tuned to the seed schema.
        let token = "LOCLETESTTOKENWINSOVERACCEPTLANG123456789012";
        let username = "locale_mw_user";
        sqlx::query(
            "INSERT INTO users (username, password_hash, role, preferred_language) \
             VALUES (?, '$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU', 'librarian', 'en')",
        )
        .bind(username)
        .execute(&pool)
        .await
        .unwrap();
        let user_id: u64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
            .bind(username)
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO sessions (token, user_id, data, last_activity) \
             VALUES (?, ?, '{}', UTC_TIMESTAMP())",
        )
        .bind(token)
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

        let app = router(test_state(pool));
        let req = HttpRequest::builder()
            .uri("/probe")
            .header("cookie", format!("session={token}"))
            .header("accept-language", "fr")
            .body(Body::empty())
            .unwrap();
        assert_eq!(resolved_locale(app, req).await, "en");
    }
}
