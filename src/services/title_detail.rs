//! Story 11-6 — the title detail document.
//!
//! One JSON shape for a title with its contributors, volumes and series
//! assignments, served by `GET /api/v1/titles/{id}` (and the PATCH
//! response) and by the MCP `get_title` tool. Moved here from
//! `routes/api_v1.rs` so the MCP layer reaches it through `services/`
//! (AR27) instead of a route module.
//!
//! The DTOs are deliberately separate from the DB models so the
//! contract evolves independently of the schema. Soft-deleted rows never
//! appear: every model call below filters `deleted_at IS NULL`.

use serde::Serialize;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::contributor::TitleContributorModel;
use crate::models::genre::GenreModel;
use crate::models::series::TitleSeriesModel;
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;

#[derive(Debug, Serialize)]
pub struct TitleDetail {
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
    pub contributors: Vec<TitleDetailContributor>,
    pub volumes: Vec<TitleDetailVolume>,
    pub series: Vec<TitleDetailSeries>,
    pub version: i32,
}

#[derive(Debug, Serialize)]
pub struct TitleDetailContributor {
    pub id: u64,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct TitleDetailVolume {
    pub id: u64,
    pub label: String,
    pub location_name: Option<String>,
    pub location_label: Option<String>,
    pub condition_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TitleDetailSeries {
    pub series_id: u64,
    pub series_name: String,
    pub position: i32,
    pub is_omnibus: bool,
    pub end_position: Option<i32>,
}

/// Assemble the rich [`TitleDetail`] DTO from a `TitleModel`.
/// Shared by the `/api/v1` GET and PATCH handlers — PATCH calls it on the
/// post-update row so the response always mirrors the persisted state.
pub async fn build_title_detail(
    pool: &DbPool,
    title: &TitleModel,
) -> Result<TitleDetail, AppError> {
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

    let contributors: Vec<TitleDetailContributor> = contributors_raw
        .into_iter()
        .map(|tc| TitleDetailContributor {
            id: tc.contributor_id,
            name: tc.contributor_name,
            role: tc.role_name,
        })
        .collect();
    let volumes: Vec<TitleDetailVolume> = volumes_raw
        .into_iter()
        .map(|v| TitleDetailVolume {
            id: v.id,
            label: v.label,
            location_name: v.location_name,
            location_label: v.location_label,
            condition_name: v.condition_name,
        })
        .collect();
    let series: Vec<TitleDetailSeries> = series_raw
        .into_iter()
        .map(|sa| TitleDetailSeries {
            series_id: sa.series_id,
            series_name: sa.series_name,
            position: sa.position_start,
            is_omnibus: sa.is_omnibus,
            end_position: sa.position_end,
        })
        .collect();

    Ok(TitleDetail {
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
        publication_date: title
            .publication_date
            .map(|d| d.format("%Y-%m-%d").to_string()),
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

/// Load a live (not soft-deleted) title and assemble its detail
/// document; `None` when the id is unknown or soft-deleted.
pub async fn find_title_detail(pool: &DbPool, id: u64) -> Result<Option<TitleDetail>, AppError> {
    let Some(title) = TitleModel::find_by_id(pool, id).await? else {
        return Ok(None);
    };
    build_title_detail(pool, &title).await.map(Some)
}
