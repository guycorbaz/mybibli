//! Admin → API keys (CR #241).
//!
//! Phase 5 of the v1.4.0 HTTP-API CR. Each row in `api_keys` is one
//! authentication token; this UI lets an admin mint, list, and revoke
//! them. Soft-revoke (`revoked_at = NOW()`) is preferred over hard
//! delete so the audit trail of past usage survives.
//!
//! The plaintext of a new key is shown EXACTLY ONCE — on the
//! `POST /admin/api-keys` response — in a UX-DR8 modal. After the
//! admin dismisses it, only the argon2 hash + 12-char prefix remain
//! in the DB. There is no recovery path; the admin can always revoke
//! and re-create.

use askama::Template;
use axum::Extension;
use axum::extract::{Form, OriginalUri, Path, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::api_key::{ApiKey, ApiKeyModel, ApiKeyScope, mint_plaintext_key};
use crate::routes::catalog::feedback_html_pub;
use crate::services::password::hash_password;

// ─── Forms ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateApiKeyForm {
    pub label: String,
    pub scope: String,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct RevokeApiKeyForm {
    pub version: i32,
    pub _csrf_token: String,
}

// ─── Template structs ────────────────────────────────────────────

#[derive(Template)]
#[template(path = "fragments/admin_api_keys_panel.html")]
struct AdminApiKeysPanel {
    panel_heading: String,
    intro: String,
    section_create: String,
    section_list: String,
    label_field: String,
    label_help: String,
    scope_field: String,
    scope_read: String,
    scope_read_help: String,
    scope_write: String,
    scope_write_help: String,
    btn_create: String,
    csrf_token: String,
    list_html: String,
    info_tooltip: crate::utils::TooltipData,
}

#[derive(Template)]
#[template(path = "fragments/admin_api_keys_list.html")]
struct AdminApiKeysList {
    keys: Vec<KeyRowDisplay>,
    empty_state: String,
    col_label: String,
    col_scope: String,
    col_prefix: String,
    col_created: String,
    col_last_used: String,
    col_status: String,
    col_actions: String,
    btn_revoke: String,
}

#[derive(Debug, Clone)]
struct KeyRowDisplay {
    id: u64,
    label: String,
    scope_label: String,
    scope_chip_class: &'static str,
    key_prefix: String,
    created_at: String,
    last_used_at: String,
    status_label: String,
    status_chip_class: &'static str,
    is_revoked: bool,
    revoke_aria: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_api_keys_created_modal.html")]
struct AdminApiKeyCreatedModal {
    heading: String,
    body: String,
    plaintext: String,
    copy_hint: String,
    btn_close: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_api_keys_revoke_modal.html")]
struct AdminApiKeyRevokeModal {
    modal_heading: String,
    modal_body: String,
    revoke_endpoint: String,
    list_target: String,
    csrf_token: String,
    version: i32,
    btn_revoke: String,
    btn_cancel: String,
}

// ─── Public render entrypoint ─────────────────────────────────────

/// Called from `admin::render_panel` when the active tab is `ApiKeys`.
pub async fn render_panel_html(
    state: &AppState,
    loc: &'static str,
    session: &Session,
) -> Result<String, AppError> {
    let keys = ApiKeyModel::list_for_admin(&state.pool).await?;
    let list_html = render_list_html(loc, &session.csrf_token, &keys)?;

    let panel = AdminApiKeysPanel {
        panel_heading: rust_i18n::t!("admin.api_keys.panel_heading", locale = loc).to_string(),
        intro: rust_i18n::t!("admin.api_keys.intro", locale = loc).to_string(),
        section_create: rust_i18n::t!("admin.api_keys.section_create", locale = loc).to_string(),
        section_list: rust_i18n::t!("admin.api_keys.section_list", locale = loc).to_string(),
        label_field: rust_i18n::t!("admin.api_keys.label_field", locale = loc).to_string(),
        label_help: rust_i18n::t!("admin.api_keys.label_help", locale = loc).to_string(),
        scope_field: rust_i18n::t!("admin.api_keys.scope_field", locale = loc).to_string(),
        scope_read: rust_i18n::t!("admin.api_keys.scope_read", locale = loc).to_string(),
        scope_read_help: rust_i18n::t!("admin.api_keys.scope_read_help", locale = loc)
            .to_string(),
        scope_write: rust_i18n::t!("admin.api_keys.scope_write", locale = loc).to_string(),
        scope_write_help: rust_i18n::t!("admin.api_keys.scope_write_help", locale = loc)
            .to_string(),
        btn_create: rust_i18n::t!("admin.api_keys.btn_create", locale = loc).to_string(),
        csrf_token: session.csrf_token.clone(),
        list_html,
        info_tooltip: crate::utils::TooltipData::with_icon(
            "admin-api-keys-info-tooltip",
            &rust_i18n::t!("admin.api_keys.tooltip_aria", locale = loc),
            &rust_i18n::t!("admin.api_keys.tooltip_text", locale = loc),
        ),
    };

    panel
        .render()
        .map_err(|_| AppError::Internal("admin api_keys panel render failed".to_string()))
}

fn render_list_html(loc: &'static str, _csrf_token: &str, keys: &[ApiKey]) -> Result<String, AppError> {
    let empty_state = if keys.is_empty() {
        rust_i18n::t!("admin.api_keys.empty_state", locale = loc).to_string()
    } else {
        String::new()
    };

    let never_label = rust_i18n::t!("admin.api_keys.last_used_never", locale = loc).to_string();
    let rows: Vec<KeyRowDisplay> = keys
        .iter()
        .map(|k| {
            let is_revoked = k.revoked_at.is_some();
            let (status_label, status_chip_class) = if is_revoked {
                (
                    rust_i18n::t!("admin.api_keys.status_revoked", locale = loc).to_string(),
                    "bg-stone-300 text-stone-700",
                )
            } else {
                (
                    rust_i18n::t!("admin.api_keys.status_active", locale = loc).to_string(),
                    "bg-emerald-100 text-emerald-800",
                )
            };
            let (scope_label, scope_chip_class) = match k.scope {
                ApiKeyScope::Read => (
                    rust_i18n::t!("admin.api_keys.scope_read", locale = loc).to_string(),
                    "bg-sky-100 text-sky-800",
                ),
                ApiKeyScope::Write => (
                    rust_i18n::t!("admin.api_keys.scope_write", locale = loc).to_string(),
                    "bg-amber-100 text-amber-800",
                ),
            };
            KeyRowDisplay {
                id: k.id,
                label: k.label.clone(),
                scope_label,
                scope_chip_class,
                key_prefix: k.key_prefix.clone(),
                created_at: k.created_at.format("%Y-%m-%d %H:%M").to_string(),
                last_used_at: k
                    .last_used_at
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| never_label.clone()),
                status_label,
                status_chip_class,
                is_revoked,
                revoke_aria: rust_i18n::t!(
                    "admin.api_keys.revoke_aria",
                    locale = loc,
                    label = &k.label
                )
                .to_string(),
            }
        })
        .collect();

    AdminApiKeysList {
        keys: rows,
        empty_state,
        col_label: rust_i18n::t!("admin.api_keys.col_label", locale = loc).to_string(),
        col_scope: rust_i18n::t!("admin.api_keys.col_scope", locale = loc).to_string(),
        col_prefix: rust_i18n::t!("admin.api_keys.col_prefix", locale = loc).to_string(),
        col_created: rust_i18n::t!("admin.api_keys.col_created", locale = loc).to_string(),
        col_last_used: rust_i18n::t!("admin.api_keys.col_last_used", locale = loc).to_string(),
        col_status: rust_i18n::t!("admin.api_keys.col_status", locale = loc).to_string(),
        col_actions: rust_i18n::t!("admin.api_keys.col_actions", locale = loc).to_string(),
        btn_revoke: rust_i18n::t!("admin.api_keys.btn_revoke", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("admin api_keys list render failed".to_string()))
}

// ─── Panel route ──────────────────────────────────────────────────

/// `GET /admin/api-keys` — tab swap. Renders the full shell (panel
/// inside the tab bar) for HTMX requests, full page for direct nav.
pub async fn admin_api_keys_panel(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
) -> Result<Response, AppError> {
    let return_path = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/admin?tab=api_keys".to_string());
    session.require_role_with_return(Role::Admin, &return_path, locale.0)?;

    crate::routes::admin::render_admin_for_api_keys(&state, &session, locale.0, &uri, is_htmx)
        .await
}

// ─── Create handler ───────────────────────────────────────────────

/// `POST /admin/api-keys` — mint a new key.
///
/// Returns the updated list as `main` plus two OOB swaps:
///   1. A success FeedbackEntry into `#feedback-list`
///   2. The one-time plaintext modal into `#admin-modal-slot`
pub async fn admin_api_keys_create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<CreateApiKeyForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=api_keys", locale.0)?;
    let loc = locale.0;

    let label_trimmed = form.label.trim();
    if label_trimmed.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("admin.api_keys.error_label_required", locale = loc).to_string(),
        ));
    }
    if label_trimmed.chars().count() > 120 {
        return Err(AppError::BadRequest(
            rust_i18n::t!("admin.api_keys.error_label_too_long", locale = loc).to_string(),
        ));
    }

    let scope = match form.scope.as_str() {
        "read" => ApiKeyScope::Read,
        "write" => ApiKeyScope::Write,
        _ => {
            return Err(AppError::BadRequest(
                rust_i18n::t!("admin.api_keys.error_invalid_scope", locale = loc).to_string(),
            ));
        }
    };

    let (plaintext, prefix) = mint_plaintext_key(scope);
    let hash = hash_password(&plaintext)
        .map_err(|e| AppError::Internal(format!("api key hash failed: {e}")))?;

    let _new_id = ApiKeyModel::create(
        &state.pool,
        label_trimmed,
        &hash,
        &prefix,
        scope,
        session.user_id,
    )
    .await?;

    tracing::info!(
        admin_id = session.user_id.unwrap_or(0),
        label = label_trimmed,
        scope = scope.as_str(),
        "API key minted"
    );

    // Refresh the list view and assemble OOB updates.
    let keys = ApiKeyModel::list_for_admin(&state.pool).await?;
    let list_html = render_list_html(loc, &session.csrf_token, &keys)?;

    let feedback = feedback_html_pub(
        "success",
        rust_i18n::t!("admin.api_keys.feedback_created", locale = loc).as_ref(),
        "",
    );

    let modal_html = AdminApiKeyCreatedModal {
        heading: rust_i18n::t!("admin.api_keys.created_modal_heading", locale = loc).to_string(),
        body: rust_i18n::t!("admin.api_keys.created_modal_body", locale = loc).to_string(),
        plaintext,
        copy_hint: rust_i18n::t!("admin.api_keys.created_copy_hint", locale = loc).to_string(),
        btn_close: rust_i18n::t!("admin.api_keys.created_btn_close", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("api key created-modal render failed".to_string()))?;

    Ok(HtmxResponse {
        main: list_html,
        oob: vec![
            OobUpdate {
                target: "feedback-list".to_string(),
                content: feedback,
            },
            OobUpdate {
                target: "admin-modal-slot".to_string(),
                content: modal_html,
            },
        ],
    })
}

// ─── Revoke handlers ──────────────────────────────────────────────

/// `GET /admin/api-keys/{id}/revoke-modal` — open the UX-DR8 confirm
/// dialog. Returns the modal fragment; the form posts to the revoke
/// endpoint.
pub async fn admin_api_keys_revoke_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=api_keys", locale.0)?;
    let loc = locale.0;

    let key = ApiKeyModel::find_by_id(&state.pool, id).await?.ok_or_else(|| {
        AppError::NotFound(rust_i18n::t!("admin.api_keys.error_not_found", locale = loc).to_string())
    })?;

    let modal = AdminApiKeyRevokeModal {
        modal_heading: rust_i18n::t!("admin.api_keys.revoke_modal_heading", locale = loc)
            .to_string(),
        modal_body: rust_i18n::t!(
            "admin.api_keys.revoke_modal_body",
            locale = loc,
            label = &key.label
        )
        .to_string(),
        revoke_endpoint: format!("/admin/api-keys/{}/revoke", key.id),
        list_target: "#admin-api-keys-list".to_string(),
        csrf_token: session.csrf_token.clone(),
        version: key.version,
        btn_revoke: rust_i18n::t!("admin.api_keys.btn_revoke_confirm", locale = loc).to_string(),
        btn_cancel: rust_i18n::t!("common.cancel", locale = loc).to_string(),
    };
    let html = modal
        .render()
        .map_err(|_| AppError::Internal("api key revoke-modal render failed".to_string()))?;
    Ok(axum::response::Html(html).into_response())
}

/// `POST /admin/api-keys/{id}/revoke` — soft-revoke the key.
///
/// Optimistic-lock via `version`; on mismatch (concurrent revoke from
/// another admin window) returns 409. On success, swaps the list and
/// closes the modal via `HX-Trigger: modal-close`.
pub async fn admin_api_keys_revoke(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<RevokeApiKeyForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=api_keys", locale.0)?;
    let loc = locale.0;

    let key = ApiKeyModel::find_by_id(&state.pool, id).await?.ok_or_else(|| {
        AppError::NotFound(rust_i18n::t!("admin.api_keys.error_not_found", locale = loc).to_string())
    })?;
    if key.revoked_at.is_some() {
        return Err(AppError::Conflict(
            rust_i18n::t!("admin.api_keys.error_already_revoked", locale = loc).to_string(),
        ));
    }

    let n = ApiKeyModel::revoke(&state.pool, id, form.version).await?;
    if n == 0 {
        return Err(AppError::Conflict(
            rust_i18n::t!("admin.api_keys.error_version_conflict", locale = loc).to_string(),
        ));
    }

    // Audit row for the revoke. Pattern mirrors the api_v1 PATCH —
    // user_id = the acting admin (always set here, since this handler
    // runs behind require_role).
    let details = serde_json::json!({
        "key_id": id,
        "label": key.label,
        "scope": key.scope.as_str(),
        "prefix": key.key_prefix,
    });
    if let Err(e) = crate::models::admin_audit::AdminAuditModel::create(
        &state.pool,
        session.user_id.unwrap_or(0),
        "api_key_revoke",
        Some("api_keys"),
        Some(id),
        Some(details),
    )
    .await
    {
        tracing::warn!(
            key_id = id,
            error = %e,
            "api_key revoke audit insert failed — revoke has already committed"
        );
    }

    let keys = ApiKeyModel::list_for_admin(&state.pool).await?;
    let list_html = render_list_html(loc, &session.csrf_token, &keys)?;

    let feedback = feedback_html_pub(
        "success",
        rust_i18n::t!("admin.api_keys.feedback_revoked", locale = loc).as_ref(),
        "",
    );

    let resp = HtmxResponse {
        main: list_html,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    };
    Ok(resp.into_response_with_hx_trigger("modal-close"))
}
