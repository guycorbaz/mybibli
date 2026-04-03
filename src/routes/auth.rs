use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Redirect};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::AppState;

// ─── Login form template ─────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/login.html")]
pub struct LoginTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub login_title: String,
    pub username_label: String,
    pub password_label: String,
    pub submit_label: String,
    pub back_to_home: String,
    pub error_message: String,
}

impl LoginTemplate {
    fn new(error_message: &str) -> Self {
        LoginTemplate {
            lang: rust_i18n::locale().to_string(),
            role: "anonymous".to_string(),
            current_page: "login",
            skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
            nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
            nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_locations: rust_i18n::t!("nav.locations").to_string(),
            nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
            nav_admin: rust_i18n::t!("nav.admin").to_string(),
            nav_login: rust_i18n::t!("nav.login").to_string(),
            nav_logout: rust_i18n::t!("nav.logout").to_string(),
            login_title: rust_i18n::t!("login.title").to_string(),
            username_label: rust_i18n::t!("login.username_label").to_string(),
            password_label: rust_i18n::t!("login.password_label").to_string(),
            submit_label: rust_i18n::t!("login.submit").to_string(),
            back_to_home: rust_i18n::t!("login.back_to_home").to_string(),
            error_message: error_message.to_string(),
        }
    }
}

// ─── Login form page ─────────────────────────────────────────────

pub async fn login_page(
    session: Session,
    HxRequest(_is_htmx): HxRequest,
) -> Result<impl IntoResponse, AppError> {
    // Already authenticated → redirect to catalog
    if session.role >= Role::Librarian {
        return Ok(Redirect::to("/catalog").into_response());
    }

    let template = LoginTemplate::new("");
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render login template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

// ─── Login handler ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Form(form): axum::Form<LoginRequest>,
) -> Result<(CookieJar, impl IntoResponse), AppError> {
    let pool = &state.pool;
    let username = form.username.trim();
    let password = form.password.as_str();

    // Look up user
    let user_row: Option<(u64, String, String)> = sqlx::query_as(
        "SELECT id, password_hash, role FROM users \
         WHERE username = ? AND active = TRUE AND deleted_at IS NULL",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;

    let Some((user_id, password_hash, role)) = user_row else {
        tracing::info!(username = %username, "Login failed: user not found");
        return render_login_error(jar);
    };

    // Verify password with Argon2
    if !verify_password(password, &password_hash) {
        tracing::info!(username = %username, "Login failed: invalid password");
        return render_login_error(jar);
    }

    // Generate session token
    let token = generate_session_token();

    // Insert session into database
    sqlx::query(
        "INSERT INTO sessions (token, user_id, data) VALUES (?, ?, '{}')",
    )
    .bind(&token)
    .bind(user_id)
    .execute(pool)
    .await?;

    tracing::info!(username = %username, role = %role, "Login successful");

    // Set session cookie
    let cookie = Cookie::build(("session", token))
        .http_only(true)
        .path("/")
        .same_site(SameSite::Lax)
        .build();

    Ok((jar.add(cookie), Redirect::to("/catalog").into_response()))
}

fn render_login_error(
    jar: CookieJar,
) -> Result<(CookieJar, axum::response::Response), AppError> {
    let error_msg = rust_i18n::t!("login.error_invalid").to_string();
    let template = LoginTemplate::new(&error_msg);
    match template.render() {
        Ok(html) => Ok((jar, Html(html).into_response())),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render login template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

// ─── Logout handler ──────────────────────────────────────────────

pub async fn logout(
    session: Session,
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, impl IntoResponse), AppError> {
    // Soft-delete session row
    if let Some(token) = &session.token {
        sqlx::query(
            "UPDATE sessions SET deleted_at = NOW() WHERE token = ? AND deleted_at IS NULL",
        )
        .bind(token)
        .execute(&state.pool)
        .await?;

        tracing::info!("User logged out");
    }

    // Clear cookie by removing it
    let cookie = Cookie::build(("session", ""))
        .path("/")
        .build();

    Ok((jar.remove(cookie), Redirect::to("/").into_response()))
}

// ─── Helpers ─────────────────────────────────────────────────────

fn verify_password(password: &str, hash: &str) -> bool {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    let Ok(parsed_hash) = PasswordHash::new(hash) else {
        tracing::warn!("Invalid password hash format in database");
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

pub fn generate_session_token() -> String {
    use base64::Engine;
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// ─── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_token_length() {
        let token = generate_session_token();
        assert_eq!(token.len(), 44, "Token should be 44 chars (32 bytes base64)");
    }

    #[test]
    fn test_generate_session_token_is_base64() {
        use base64::Engine;
        let token = generate_session_token();
        let decoded = base64::engine::general_purpose::STANDARD.decode(&token);
        assert!(decoded.is_ok(), "Token should be valid base64");
        assert_eq!(decoded.unwrap().len(), 32, "Decoded token should be 32 bytes");
    }

    #[test]
    fn test_generate_session_token_unique() {
        let t1 = generate_session_token();
        let t2 = generate_session_token();
        assert_ne!(t1, t2, "Tokens should be unique");
    }

    #[test]
    fn test_verify_password_valid() {
        // Generate a hash for "testpass" and verify it
        use argon2::{Argon2, PasswordHasher};
        use argon2::password_hash::SaltString;
        use rand::rngs::OsRng;

        let salt = SaltString::generate(OsRng);
        let hash = Argon2::default()
            .hash_password(b"testpass", &salt)
            .unwrap()
            .to_string();

        assert!(verify_password("testpass", &hash));
    }

    #[test]
    fn test_verify_password_invalid() {
        use argon2::{Argon2, PasswordHasher};
        use argon2::password_hash::SaltString;
        use rand::rngs::OsRng;

        let salt = SaltString::generate(OsRng);
        let hash = Argon2::default()
            .hash_password(b"testpass", &salt)
            .unwrap()
            .to_string();

        assert!(!verify_password("wrongpass", &hash));
    }

    #[test]
    fn test_verify_password_invalid_hash_format() {
        assert!(!verify_password("anything", "not-a-valid-hash"));
    }

    #[test]
    fn test_login_template_renders() {
        let template = LoginTemplate::new("");
        let result = template.render();
        assert!(result.is_ok());
        let html = result.unwrap();
        assert!(html.contains("username"));
        assert!(html.contains("password"));
        assert!(html.contains(r#"action="/login""#));
    }

    #[test]
    fn test_login_template_with_error() {
        let template = LoginTemplate::new("Invalid credentials");
        let result = template.render();
        assert!(result.is_ok());
        let html = result.unwrap();
        assert!(html.contains("Invalid credentials"));
    }
}
