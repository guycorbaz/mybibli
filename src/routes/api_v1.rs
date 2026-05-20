//! CR #241 — JSON HTTP API under `/api/v1/*`.
//!
//! All handlers require an API key via [`crate::middleware::api_key::ApiKeyAuth`]
//! (read endpoints) or [`crate::middleware::api_key::ApiKeyWrite`]
//! (mutation endpoints, added in Phase 4). Auth flows through
//! `Authorization: Bearer <plaintext>` or `X-API-Key: <plaintext>`.
//!
//! DTOs are intentionally **separate** from the DB models so the API
//! contract evolves independently of the schema. Adding a column to
//! `titles` won't break a client that pinned the API shape; renaming a
//! column won't propagate either.
//!
//! Soft-delete is respected end-to-end: deleted rows never appear,
//! deleted FKs (e.g. soft-deleted genre on an orphan title) get
//! `null`-ed in the DTO via the model's LEFT-JOIN + COALESCE pattern
//! shipped in v1.2.2.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::error::AppError;
use crate::middleware::api_key::{ApiKeyAuth, ApiKeyWrite};
use crate::models::contributor::TitleContributorModel;
use crate::models::genre::GenreModel;
use crate::models::location::LocationModel;
use crate::models::series::{SeriesModel, TitleSeriesModel};
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;

// ─── Common shapes ───────────────────────────────────────────────

/// Wrapper used by every paginated list endpoint so clients can rely
/// on a single shape (items + cursor metadata) regardless of which
/// resource they're walking.
#[derive(Debug, Serialize)]
pub struct ApiListResponse<T: Serialize> {
    pub items: Vec<T>,
    pub page: u32,
    pub total_pages: u32,
    pub total_items: i64,
}

// ─── /api/v1/titles ──────────────────────────────────────────────

/// DTO for the list endpoint. Carries the fields a classification AI
/// or a custom script most often needs at a glance. The detail
/// endpoint returns a richer object.
#[derive(Debug, Serialize)]
pub struct ApiTitleListItem {
    pub id: u64,
    pub title: String,
    pub subtitle: Option<String>,
    pub language: String,
    pub media_type: String,
    pub isbn: Option<String>,
    pub publisher: Option<String>,
    pub publication_year: Option<i32>,
    pub genre_id: u64,
    pub genre_name: Option<String>,
    pub dewey_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiTitleListQuery {
    pub page: Option<u32>,
    pub q: Option<String>,
    pub genre_id: Option<u64>,
    pub dewey_prefix: Option<String>,
}

/// `GET /api/v1/titles` — paginated list with light filtering.
pub async fn list_titles(
    State(state): State<AppState>,
    ApiKeyAuth(_ctx): ApiKeyAuth,
    Query(query): Query<ApiTitleListQuery>,
) -> Result<Json<ApiListResponse<ApiTitleListItem>>, AppError> {
    let pool = &state.pool;
    let page = query.page.unwrap_or(1).max(1);
    let q = query.q.unwrap_or_default();

    let outcome = crate::services::search::SearchService::search(
        pool,
        &q,
        query.genre_id,
        None, // volume_state — not exposed via API in v1
        &None,
        &None,
        page,
        false, // no_volumes_only — not exposed via API in v1
    )
    .await?;

    let results = match outcome {
        crate::services::search::SearchOutcome::Results(r) => r,
        crate::services::search::SearchOutcome::Redirect(_) => {
            // V-code / L-code shortcut paths in SearchService don't
            // apply to the JSON API surface; treat them as empty for
            // v1. A client wanting V/L lookups uses the dedicated
            // /api/v1/titles/:id or /api/v1/locations endpoints.
            return Ok(Json(ApiListResponse {
                items: vec![],
                page,
                total_pages: 0,
                total_items: 0,
            }));
        }
    };

    // Optional client-side filter by Dewey prefix. The search-service
    // path doesn't carry this filter; we apply it here so a small AI
    // classifier can ground its suggestions on the existing Dewey
    // distribution (per the CR's "AI helps with classification" use
    // case).
    let dewey_filter = query.dewey_prefix.unwrap_or_default();
    let items: Vec<ApiTitleListItem> = results
        .items
        .into_iter()
        .filter(|sr| {
            if dewey_filter.is_empty() {
                true
            } else {
                // The search result doesn't carry Dewey directly;
                // skip the filter in v1 (a follow-up adds dewey_code
                // to the search projection). The `dewey_prefix`
                // query param is intentionally a no-op until then.
                let _ = sr;
                true
            }
        })
        .map(|sr| ApiTitleListItem {
            id: sr.id,
            title: sr.title,
            subtitle: sr.subtitle,
            language: String::new(), // search result doesn't carry
            media_type: sr.media_type,
            isbn: None,              // search result doesn't carry
            publisher: None,
            publication_year: sr.publication_date.map(|d| d.format("%Y").to_string().parse::<i32>().unwrap_or(0)),
            genre_id: 0, // search result has genre_name, not id
            genre_name: Some(sr.genre_name),
            dewey_code: None,
        })
        .collect();

    Ok(Json(ApiListResponse {
        items,
        page: results.page,
        total_pages: results.total_pages,
        total_items: results.total_items as i64,
    }))
}

// ─── /api/v1/titles/:id ──────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ApiTitleDetail {
    pub id: u64,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub language: String,
    pub media_type: String,
    pub isbn: Option<String>,
    pub issn: Option<String>,
    pub upc: Option<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>, // ISO 8601 (YYYY-MM-DD)
    pub cover_image_url: Option<String>,
    pub genre_id: u64,
    pub genre_name: Option<String>,
    pub dewey_code: Option<String>,
    pub page_count: Option<i32>,
    pub track_count: Option<i32>,
    pub total_duration: Option<i32>,
    pub age_rating: Option<String>,
    pub issue_number: Option<i32>,
    pub contributors: Vec<ApiContributor>,
    pub volumes: Vec<ApiVolume>,
    pub series: Vec<ApiSeriesAssignment>,
    pub version: i32,
}

#[derive(Debug, Serialize)]
pub struct ApiContributor {
    pub id: u64,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct ApiVolume {
    pub id: u64,
    pub label: String,
    pub location_name: Option<String>,
    pub location_label: Option<String>,
    pub condition_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApiSeriesAssignment {
    pub series_id: u64,
    pub series_name: String,
    pub position: i32,
    pub is_omnibus: bool,
    pub end_position: Option<i32>,
}

/// `GET /api/v1/titles/:id` — full detail document.
pub async fn get_title(
    State(state): State<AppState>,
    ApiKeyAuth(_ctx): ApiKeyAuth,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    let pool = &state.pool;
    let title = match TitleModel::find_by_id(pool, id).await? {
        Some(t) => t,
        None => return Ok(not_found_response("title not found")),
    };
    let detail = build_title_detail(pool, &title).await?;
    Ok(Json(detail).into_response())
}

/// Assemble the rich [`ApiTitleDetail`] DTO from a `TitleModel`.
/// Shared by the GET and PATCH handlers — PATCH calls it on the
/// post-update row so the response always mirrors the persisted state.
async fn build_title_detail(
    pool: &crate::db::DbPool,
    title: &TitleModel,
) -> Result<ApiTitleDetail, AppError> {
    let contributors_raw = TitleContributorModel::find_by_title(pool, title.id).await?;
    let volumes_raw = VolumeModel::find_by_title(pool, title.id).await?;
    let series_raw = TitleSeriesModel::find_by_title(pool, title.id).await?;
    // Resolve genre name via the catalog's existing helper (returns
    // empty string for an orphan-FK soft-deleted genre — same shape
    // the home dashboard uses).
    let genre_name_str = GenreModel::find_name_by_id(pool, title.genre_id).await?;
    let genre_name = if genre_name_str.is_empty() {
        None
    } else {
        Some(genre_name_str)
    };

    let contributors: Vec<ApiContributor> = contributors_raw
        .into_iter()
        .map(|tc| ApiContributor {
            id: tc.contributor_id,
            name: tc.contributor_name,
            role: tc.role_name,
        })
        .collect();
    let volumes: Vec<ApiVolume> = volumes_raw
        .into_iter()
        .map(|v| ApiVolume {
            id: v.id,
            label: v.label,
            location_name: v.location_name,
            location_label: v.location_label,
            condition_name: v.condition_name,
        })
        .collect();
    let series: Vec<ApiSeriesAssignment> = series_raw
        .into_iter()
        .map(|sa| ApiSeriesAssignment {
            series_id: sa.series_id,
            series_name: sa.series_name,
            position: sa.position_start,
            is_omnibus: sa.is_omnibus,
            end_position: sa.position_end,
        })
        .collect();

    Ok(ApiTitleDetail {
        id: title.id,
        title: title.title.clone(),
        subtitle: title.subtitle.clone(),
        description: title.description.clone(),
        language: title.language.clone(),
        media_type: title.media_type.clone(),
        isbn: title.isbn.clone(),
        issn: title.issn.clone(),
        upc: title.upc.clone(),
        publisher: title.publisher.clone(),
        publication_date: title.publication_date.map(|d| d.format("%Y-%m-%d").to_string()),
        cover_image_url: title.cover_image_url.clone(),
        genre_id: title.genre_id,
        genre_name,
        dewey_code: title.dewey_code.clone(),
        page_count: title.page_count,
        track_count: title.track_count,
        total_duration: title.total_duration,
        age_rating: title.age_rating.clone(),
        issue_number: title.issue_number,
        contributors,
        volumes,
        series,
        version: title.version,
    })
}

// ─── /api/v1/genres ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ApiGenre {
    pub id: u64,
    pub name: String,
}

pub async fn list_genres(
    State(state): State<AppState>,
    ApiKeyAuth(_ctx): ApiKeyAuth,
) -> Result<Json<Vec<ApiGenre>>, AppError> {
    let items = GenreModel::list_active(&state.pool).await?;
    Ok(Json(
        items
            .into_iter()
            .map(|g| ApiGenre {
                id: g.id,
                name: g.name,
            })
            .collect(),
    ))
}

// ─── /api/v1/locations ───────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ApiLocation {
    pub id: u64,
    pub name: String,
    pub label: String,
    pub node_type: String,
    pub parent_id: Option<u64>,
}

pub async fn list_locations(
    State(state): State<AppState>,
    ApiKeyAuth(_ctx): ApiKeyAuth,
) -> Result<Json<Vec<ApiLocation>>, AppError> {
    let items = LocationModel::find_all_tree(&state.pool).await?;
    Ok(Json(
        items
            .into_iter()
            .map(|l| ApiLocation {
                id: l.id,
                name: l.name,
                label: l.label,
                node_type: l.node_type,
                parent_id: l.parent_id,
            })
            .collect(),
    ))
}

// ─── /api/v1/series ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ApiSeries {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    pub series_type: String,
    pub total_volume_count: Option<i32>,
}

pub async fn list_series_endpoint(
    State(state): State<AppState>,
    ApiKeyAuth(_ctx): ApiKeyAuth,
) -> Result<Json<Vec<ApiSeries>>, AppError> {
    // v1: return up to one page (handled by SeriesModel::active_list).
    // Future patch: walk all pages or accept a page query param.
    let paginated = SeriesModel::active_list(&state.pool, 1).await?;
    Ok(Json(
        paginated
            .items
            .into_iter()
            .map(|s| ApiSeries {
                id: s.id,
                name: s.name,
                description: s.description,
                series_type: match s.series_type {
                    crate::models::series::SeriesType::Open => "open".to_string(),
                    crate::models::series::SeriesType::Closed => "closed".to_string(),
                },
                total_volume_count: s.total_volume_count,
            })
            .collect(),
    ))
}

// ─── /api/v1/dewey/:prefix ───────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ApiDeweyBucket {
    pub prefix: String,
    pub titles: Vec<ApiDeweyTitle>,
}

#[derive(Debug, Serialize)]
pub struct ApiDeweyTitle {
    pub id: u64,
    pub title: String,
    pub dewey_code: Option<String>,
    pub genre_name: Option<String>,
}

/// `GET /api/v1/dewey/{prefix}` — titles whose dewey_code starts with
/// `<prefix>`. Helps a classification AI ground its suggestions on
/// the existing distribution. Single-tenant catalog → no pagination
/// in v1 (the bucket is bounded by the catalog size).
pub async fn list_titles_by_dewey_prefix(
    State(state): State<AppState>,
    ApiKeyAuth(_ctx): ApiKeyAuth,
    Path(prefix): Path<String>,
) -> Result<Response, AppError> {
    let trimmed = prefix.trim();
    // Defensive: reject pathological inputs early. Dewey codes are
    // ASCII digits + optional `.` + a few more digits. Anything past
    // 20 chars is a sign of a misuse / probe.
    if trimmed.is_empty() || trimmed.len() > 20 {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_dewey_prefix"})),
        )
            .into_response());
    }

    // The titles model doesn't ship a `find_by_dewey_prefix` yet;
    // walking via raw SQL keeps Phase 3 self-contained. A future
    // patch can promote this into a TitleModel method if the call
    // site grows.
    let pattern = format!("{}%", trimmed);
    let rows = sqlx::query(
        "SELECT t.id, t.title, t.dewey_code, COALESCE(g.name, '') AS genre_name \
         FROM titles t \
         LEFT JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL \
         WHERE t.deleted_at IS NULL \
           AND t.dewey_code LIKE ? \
         ORDER BY t.dewey_code ASC, t.title ASC \
         LIMIT 500",
    )
    .bind(&pattern)
    .fetch_all(&state.pool)
    .await?;

    use sqlx::Row;
    let titles: Vec<ApiDeweyTitle> = rows
        .into_iter()
        .map(|r| {
            let genre_name: String = r.try_get("genre_name").unwrap_or_default();
            ApiDeweyTitle {
                id: r.try_get("id").unwrap_or(0),
                title: r.try_get("title").unwrap_or_default(),
                dewey_code: r.try_get("dewey_code").ok(),
                genre_name: if genre_name.is_empty() {
                    None
                } else {
                    Some(genre_name)
                },
            }
        })
        .collect();

    Ok(Json(ApiDeweyBucket {
        prefix: trimmed.to_string(),
        titles,
    })
    .into_response())
}

// ─── PATCH /api/v1/titles/{id} ───────────────────────────────────

/// Distinguish "field omitted" from "field set to null" — the standard
/// serde double-Option trick. `None` = omitted, `Some(None)` = explicit
/// `null`, `Some(Some(v))` = a value to set. Drives partial updates.
fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// JSON body for `PATCH /api/v1/titles/{id}`.
///
/// Allow-list of mutable fields is intentionally narrow — these are
/// the four attributes a classifier or labelling script needs. Other
/// fields (title, isbn, publication_date…) require the full UI flow
/// so cover refetch, search-index updates, and contributor invariants
/// stay consistent.
#[derive(Debug, Deserialize)]
pub struct ApiTitlePatchBody {
    /// Optimistic-locking version (`titles.version`). Required so two
    /// concurrent classifiers can't silently clobber each other.
    pub version: i32,

    #[serde(default, deserialize_with = "double_option")]
    pub subtitle: Option<Option<String>>,

    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,

    /// Dewey decimal code, e.g. `"813.54"`. Nullable.
    #[serde(default, deserialize_with = "double_option")]
    pub dewey_code: Option<Option<String>>,

    /// Genre FK. Non-nullable in the schema, so we accept only a
    /// concrete `u64`; a JSON `null` here returns 400.
    pub genre_id: Option<u64>,
}

/// `PATCH /api/v1/titles/{id}` — write-scope-only.
///
/// Returns the refreshed [`ApiTitleDetail`] on success, 401/403 on
/// auth issues, 404 on missing/deleted title, 409 on version mismatch,
/// 400 on validation errors.
pub async fn patch_title(
    State(state): State<AppState>,
    ApiKeyWrite(ctx): ApiKeyWrite,
    Path(id): Path<u64>,
    Json(body): Json<ApiTitlePatchBody>,
) -> Result<Response, AppError> {
    let pool = &state.pool;

    let Some(existing) = TitleModel::find_by_id(pool, id).await? else {
        return Ok(not_found_response("title_not_found"));
    };

    if existing.version != body.version {
        return Ok((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "version_mismatch",
                "expected": existing.version,
                "supplied": body.version,
                "reason": "Title was updated since you read it. Re-fetch and retry.",
            })),
        )
            .into_response());
    }

    // Validate genre_id if provided — bail early so the audit row only
    // records actual successful changes.
    if let Some(genre_id) = body.genre_id
        && GenreModel::find_by_id(pool, genre_id).await?.is_none()
    {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_genre_id",
                "reason": format!("genre {} does not exist or is deleted", genre_id),
            })),
        )
            .into_response());
    }

    // Dewey lite-validation: must be empty (->NULL) or numeric-ish.
    // Match the same regex used by the metadata-edit form.
    if let Some(Some(dewey)) = body.dewey_code.as_ref() {
        let trimmed = dewey.trim();
        if !trimmed.is_empty()
            && !trimmed
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '/')
        {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_dewey_code",
                    "reason": "Dewey code must contain digits, '.' or '/' only.",
                })),
            )
                .into_response());
        }
    }

    // Collect the diff for the audit trail BEFORE writing — we want
    // both the old + new value of every field that actually changed.
    let mut changes = serde_json::Map::new();
    let mut had_change = false;

    let mut setters: Vec<&str> = Vec::new();
    let mut bind_subtitle: Option<Option<String>> = None;
    let mut bind_description: Option<Option<String>> = None;
    let mut bind_dewey: Option<Option<String>> = None;
    let mut bind_genre: Option<u64> = None;

    if let Some(opt_v) = body.subtitle.clone() {
        let new_v = opt_v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if new_v != existing.subtitle {
            setters.push("subtitle = ?");
            bind_subtitle = Some(new_v.clone());
            changes.insert(
                "subtitle".to_string(),
                serde_json::json!({"old": existing.subtitle, "new": new_v}),
            );
            had_change = true;
        }
    }
    if let Some(opt_v) = body.description.clone() {
        let new_v = opt_v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if new_v != existing.description {
            setters.push("description = ?");
            bind_description = Some(new_v.clone());
            changes.insert(
                "description".to_string(),
                serde_json::json!({"old": existing.description, "new": new_v}),
            );
            had_change = true;
        }
    }
    if let Some(opt_v) = body.dewey_code.clone() {
        let new_v = opt_v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if new_v != existing.dewey_code {
            setters.push("dewey_code = ?");
            bind_dewey = Some(new_v.clone());
            changes.insert(
                "dewey_code".to_string(),
                serde_json::json!({"old": existing.dewey_code, "new": new_v}),
            );
            had_change = true;
        }
    }
    if let Some(g) = body.genre_id
        && g != existing.genre_id
    {
        setters.push("genre_id = ?");
        bind_genre = Some(g);
        changes.insert(
            "genre_id".to_string(),
            serde_json::json!({"old": existing.genre_id, "new": g}),
        );
        had_change = true;
    }

    if !had_change {
        // Idempotent no-op — still return 200 with the current state,
        // no audit row, no version bump. Matches typical PATCH RFC
        // wording: 204/200 are both valid; we pick 200 + body so the
        // client can confirm.
        let detail = build_title_detail(pool, &existing).await?;
        return Ok((StatusCode::OK, Json(detail)).into_response());
    }

    // Build & execute the UPDATE. Order of binds MUST match the order
    // of pushes into `setters` above.
    let mut sql = String::from("UPDATE titles SET ");
    sql.push_str(&setters.join(", "));
    sql.push_str(", version = version + 1, updated_at = NOW() WHERE id = ? AND version = ? AND deleted_at IS NULL");

    let mut q = sqlx::query(&sql);
    if let Some(ref opt_v) = bind_subtitle {
        q = q.bind(opt_v.as_deref());
    }
    if let Some(ref opt_v) = bind_description {
        q = q.bind(opt_v.as_deref());
    }
    if let Some(ref opt_v) = bind_dewey {
        q = q.bind(opt_v.as_deref());
    }
    if let Some(g) = bind_genre {
        q = q.bind(g);
    }
    q = q.bind(id).bind(body.version);

    let result = q.execute(pool).await?;
    crate::services::locking::check_update_result(result.rows_affected(), "title")?;

    // Audit row — write directly so we can pass NULL user_id when the
    // API key's issuing admin has been purged. Attribution always
    // travels in `details` (key id + label). Don't fail the request if
    // audit insertion fails: the change has already landed.
    let key_label = fetch_api_key_label(pool, ctx.key_id).await;
    let issuer_user_id = fetch_api_key_created_by(pool, ctx.key_id).await;
    let details = serde_json::json!({
        "via_api_key_id": ctx.key_id,
        "via_api_key_label": key_label,
        "scope": ctx.scope.as_str(),
        "changes": changes,
    });
    if let Err(e) = sqlx::query(
        "INSERT INTO admin_audit (user_id, action, entity_type, entity_id, details) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(issuer_user_id.map(|v| v as i64))
    .bind("api_patch_title")
    .bind("titles")
    .bind(id as i64)
    .bind(&details)
    .execute(pool)
    .await
    {
        tracing::warn!(
            title_id = id,
            key_id = ctx.key_id,
            error = %e,
            "api_patch_title audit insert failed — the title update has already committed"
        );
    }

    let refreshed = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::Internal("title vanished after patch".to_string()))?;
    let detail = build_title_detail(pool, &refreshed).await?;
    Ok((StatusCode::OK, Json(detail)).into_response())
}

async fn fetch_api_key_label(pool: &crate::db::DbPool, key_id: u64) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT label FROM api_keys WHERE id = ?")
        .bind(key_id as i64)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

async fn fetch_api_key_created_by(pool: &crate::db::DbPool, key_id: u64) -> Option<u64> {
    // fetch_optional on a query_scalar<Option<i64>> yields
    // Result<Option<Option<i64>>>: outer = row found?, inner =
    // `created_by` non-NULL? We flatten both to a single Option<u64>.
    let triple: Option<Option<Option<i64>>> = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT CAST(created_by AS SIGNED) FROM api_keys WHERE id = ?",
    )
    .bind(key_id as i64)
    .fetch_optional(pool)
    .await
    .ok();
    triple.flatten().flatten().map(|v| v as u64)
}

// ─── Helpers ─────────────────────────────────────────────────────

fn not_found_response(reason: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": "not_found",
            "reason": reason,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_body_omitted_field_is_none() {
        let body: ApiTitlePatchBody = serde_json::from_str(r#"{"version": 7}"#).unwrap();
        assert_eq!(body.version, 7);
        assert!(body.subtitle.is_none(), "omitted = None (no change)");
        assert!(body.description.is_none());
        assert!(body.dewey_code.is_none());
        assert!(body.genre_id.is_none());
    }

    #[test]
    fn patch_body_explicit_null_is_some_none() {
        // The double_option dance: explicit JSON null = clear-to-NULL.
        let body: ApiTitlePatchBody =
            serde_json::from_str(r#"{"version": 7, "subtitle": null, "dewey_code": null}"#)
                .unwrap();
        assert_eq!(body.subtitle, Some(None));
        assert_eq!(body.dewey_code, Some(None));
        // Untouched fields remain None (= omitted, no change).
        assert!(body.description.is_none());
    }

    #[test]
    fn patch_body_value_present_is_some_some() {
        let body: ApiTitlePatchBody = serde_json::from_str(
            r#"{"version": 7, "subtitle": "A long-awaited subtitle", "genre_id": 42}"#,
        )
        .unwrap();
        assert_eq!(
            body.subtitle,
            Some(Some("A long-awaited subtitle".to_string()))
        );
        assert_eq!(body.genre_id, Some(42));
    }

    #[test]
    fn dewey_code_validation_accepts_classic_shapes() {
        // We unit-test the regex shape directly — the handler logic
        // around it is exercised by the integration tests.
        let valid_cases = ["813", "813.54", "973/.926", "100.0"];
        for c in valid_cases {
            assert!(
                c.chars()
                    .all(|ch| ch.is_ascii_digit() || ch == '.' || ch == '/'),
                "{c} should be valid"
            );
        }
        let invalid_cases = ["abc", "813;DROP TABLE titles", "ml-100"];
        for c in invalid_cases {
            assert!(
                !c.chars()
                    .all(|ch| ch.is_ascii_digit() || ch == '.' || ch == '/'),
                "{c} should be invalid"
            );
        }
    }
}
