use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{PaginatedList, DEFAULT_PAGE_SIZE};

/// Matches the `titles` table schema exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleModel {
    pub id: u64,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub language: String,
    pub media_type: String,
    pub publication_date: Option<NaiveDate>,
    pub publisher: Option<String>,
    pub isbn: Option<String>,
    pub issn: Option<String>,
    pub upc: Option<String>,
    pub cover_image_url: Option<String>,
    pub genre_id: u64,
    pub dewey_code: Option<String>,
    pub page_count: Option<i32>,
    pub track_count: Option<i32>,
    pub total_duration: Option<i32>,
    pub age_rating: Option<String>,
    pub issue_number: Option<i32>,
    pub manually_edited_fields: Option<String>,
    pub version: i32,
}

impl std::fmt::Display for TitleModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.title, self.media_type)
    }
}

/// Data required to create a new title.
#[derive(Debug, Deserialize)]
pub struct NewTitle {
    pub title: String,
    pub media_type: String,
    pub genre_id: u64,
    pub language: String,
    pub subtitle: Option<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<NaiveDate>,
    pub isbn: Option<String>,
    pub issn: Option<String>,
    pub upc: Option<String>,
    pub page_count: Option<i32>,
    pub track_count: Option<i32>,
    pub total_duration: Option<i32>,
    pub age_rating: Option<String>,
    pub issue_number: Option<i32>,
}

fn row_to_title(row: sqlx::mysql::MySqlRow) -> Result<TitleModel, sqlx::Error> {
    Ok(TitleModel {
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        subtitle: row.try_get("subtitle")?,
        description: row.try_get("description")?,
        language: row.try_get("language")?,
        media_type: row.try_get("media_type")?,
        publication_date: row.try_get("publication_date")?,
        publisher: row.try_get("publisher")?,
        isbn: row.try_get("isbn")?,
        issn: row.try_get("issn")?,
        upc: row.try_get("upc")?,
        cover_image_url: row.try_get("cover_image_url")?,
        genre_id: row.try_get("genre_id")?,
        dewey_code: row.try_get("dewey_code")?,
        page_count: row.try_get("page_count")?,
        track_count: row.try_get("track_count")?,
        total_duration: row.try_get("total_duration")?,
        age_rating: row.try_get("age_rating")?,
        issue_number: row.try_get("issue_number")?,
        manually_edited_fields: row.try_get("manually_edited_fields")?,
        version: row.try_get("version")?,
    })
}

impl TitleModel {
    pub async fn find_by_isbn(pool: &DbPool, isbn: &str) -> Result<Option<TitleModel>, AppError> {
        tracing::debug!(isbn = %isbn, "Looking up title by ISBN");

        let row = sqlx::query(
            r#"SELECT id, title, subtitle, description, language,
                      media_type, publication_date, publisher, isbn, issn, upc,
                      cover_image_url, genre_id, dewey_code,
                      page_count, track_count, total_duration,
                      age_rating, issue_number,
                      CAST(manually_edited_fields AS CHAR) as manually_edited_fields, version
               FROM titles
               WHERE isbn = ? AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(isbn)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_title(r)?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_upc(pool: &DbPool, upc: &str) -> Result<Option<TitleModel>, AppError> {
        tracing::debug!(upc = %upc, "Looking up title by UPC");

        let row = sqlx::query(
            r#"SELECT id, title, subtitle, description, language,
                      media_type, publication_date, publisher, isbn, issn, upc,
                      cover_image_url, genre_id, dewey_code,
                      page_count, track_count, total_duration,
                      age_rating, issue_number,
                      CAST(manually_edited_fields AS CHAR) as manually_edited_fields, version
               FROM titles
               WHERE upc = ? AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(upc)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_title(r)?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_issn(pool: &DbPool, issn: &str) -> Result<Option<TitleModel>, AppError> {
        tracing::debug!(issn = %issn, "Looking up title by ISSN");

        let row = sqlx::query(
            r#"SELECT id, title, subtitle, description, language,
                      media_type, publication_date, publisher, isbn, issn, upc,
                      cover_image_url, genre_id, dewey_code,
                      page_count, track_count, total_duration,
                      age_rating, issue_number,
                      CAST(manually_edited_fields AS CHAR) as manually_edited_fields, version
               FROM titles
               WHERE issn = ? AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(issn)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_title(r)?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<TitleModel>, AppError> {
        tracing::debug!(id = id, "Looking up title by ID");

        let row = sqlx::query(
            r#"SELECT id, title, subtitle, description, language,
                      media_type, publication_date, publisher, isbn, issn, upc,
                      cover_image_url, genre_id, dewey_code,
                      page_count, track_count, total_duration,
                      age_rating, issue_number,
                      CAST(manually_edited_fields AS CHAR) as manually_edited_fields, version
               FROM titles
               WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_title(r)?)),
            None => Ok(None),
        }
    }

    pub async fn create(pool: &DbPool, new_title: &NewTitle) -> Result<TitleModel, AppError> {
        tracing::info!(
            title = %new_title.title,
            media_type = %new_title.media_type,
            isbn = ?new_title.isbn,
            "Creating new title"
        );

        let result = sqlx::query(
            r#"INSERT INTO titles (title, subtitle, language, media_type, publication_date,
                                   publisher, isbn, issn, upc, genre_id, page_count,
                                   track_count, total_duration, age_rating, issue_number)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&new_title.title)
        .bind(&new_title.subtitle)
        .bind(&new_title.language)
        .bind(&new_title.media_type)
        .bind(new_title.publication_date)
        .bind(&new_title.publisher)
        .bind(&new_title.isbn)
        .bind(&new_title.issn)
        .bind(&new_title.upc)
        .bind(new_title.genre_id)
        .bind(new_title.page_count)
        .bind(new_title.track_count)
        .bind(new_title.total_duration)
        .bind(&new_title.age_rating)
        .bind(new_title.issue_number)
        .execute(pool)
        .await?;

        let id = result.last_insert_id();
        TitleModel::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve created title".to_string()))
    }

    /// Update a title with optimistic locking (version check).
    /// Returns the updated title, or AppError::Conflict if the version has changed.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_with_locking(
        pool: &DbPool,
        id: u64,
        version: i32,
        title: &str,
        subtitle: Option<&str>,
        publisher: Option<&str>,
        language: &str,
        genre_id: u64,
        media_type: &str,
    ) -> Result<TitleModel, AppError> {
        let result = sqlx::query(
            "UPDATE titles SET title = ?, subtitle = ?, publisher = ?, \
             language = ?, genre_id = ?, media_type = ?, \
             version = version + 1, updated_at = NOW() \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(title)
        .bind(subtitle)
        .bind(publisher)
        .bind(language)
        .bind(genre_id)
        .bind(media_type)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;

        crate::services::locking::check_update_result(result.rows_affected(), "title")?;

        TitleModel::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve updated title".to_string()))
    }

    /// Update all metadata fields on a title with optimistic locking.
    /// Used by the metadata editing form and re-download confirmation.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_metadata(
        pool: &DbPool,
        id: u64,
        version: i32,
        title: &str,
        subtitle: Option<&str>,
        description: Option<&str>,
        publisher: Option<&str>,
        language: &str,
        genre_id: u64,
        publication_date: Option<chrono::NaiveDate>,
        dewey_code: Option<&str>,
        page_count: Option<i32>,
        track_count: Option<i32>,
        total_duration: Option<i32>,
        age_rating: Option<&str>,
        issue_number: Option<i32>,
        manually_edited_fields: Option<&str>,
    ) -> Result<TitleModel, AppError> {
        let result = sqlx::query(
            "UPDATE titles SET title = ?, subtitle = ?, description = ?, \
             publisher = ?, language = ?, genre_id = ?, \
             publication_date = ?, dewey_code = ?, \
             page_count = ?, track_count = ?, total_duration = ?, \
             age_rating = ?, issue_number = ?, \
             manually_edited_fields = ?, \
             version = version + 1, updated_at = NOW() \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(title)
        .bind(subtitle)
        .bind(description)
        .bind(publisher)
        .bind(language)
        .bind(genre_id)
        .bind(publication_date)
        .bind(dewey_code)
        .bind(page_count)
        .bind(track_count)
        .bind(total_duration)
        .bind(age_rating)
        .bind(issue_number)
        .bind(manually_edited_fields)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;

        crate::services::locking::check_update_result(result.rows_affected(), "title")?;

        TitleModel::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve updated title".to_string()))
    }

    /// Parse the manually_edited_fields JSON column into a Vec<String>.
    pub fn parsed_manually_edited_fields(&self) -> Vec<String> {
        self.manually_edited_fields
            .as_deref()
            .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
            .unwrap_or_default()
    }

    /// Find up to 8 related titles for a title detail page.
    /// Priority: same series (1) > same contributor (2) > same genre+decade (3).
    /// Deduplicated across criteria (keeps lowest priority number per title).
    /// Returns an empty Vec when no criterion yields candidates — the caller
    /// must render nothing in that case (FR114 / UX-DR30: section entirely absent).
    pub async fn find_similar(pool: &DbPool, title_id: u64) -> Result<Vec<SimilarTitle>, AppError> {
        // Load the anchor title to extract the match criteria
        let anchor = match TitleModel::find_by_id(pool, title_id).await? {
            Some(t) => t,
            None => return Ok(Vec::new()),
        };

        // Collect series_ids this title belongs to. LIMIT 20 caps worst-case
        // placeholder expansion on anthologies or omnibus collections.
        let series_ids: Vec<u64> = sqlx::query(
            "SELECT DISTINCT ts.series_id FROM title_series ts \
             JOIN series s ON ts.series_id = s.id AND s.deleted_at IS NULL \
             WHERE ts.title_id = ? AND ts.deleted_at IS NULL \
             LIMIT 20",
        )
        .bind(title_id)
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.try_get::<u64, _>("series_id"))
        .collect::<Result<Vec<_>, _>>()?;

        // Collect contributor_ids for this title. LIMIT 20 bounds the IN(...)
        // clause on titles with many contributors (translator + editor + many authors).
        let contributor_ids: Vec<u64> = sqlx::query(
            "SELECT DISTINCT tc.contributor_id FROM title_contributors tc \
             JOIN contributors c ON tc.contributor_id = c.id AND c.deleted_at IS NULL \
             WHERE tc.title_id = ? AND tc.deleted_at IS NULL \
             LIMIT 20",
        )
        .bind(title_id)
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.try_get::<u64, _>("contributor_id"))
        .collect::<Result<Vec<_>, _>>()?;

        // Decade bounds from publication year (inclusive)
        let decade_bounds: Option<(i32, i32)> = anchor.publication_date.map(decade_bounds_for_date);

        // Early return: no criteria → empty result, no query issued
        if series_ids.is_empty() && contributor_ids.is_empty() && decade_bounds.is_none() {
            return Ok(Vec::new());
        }

        // Build UNION ALL arms dynamically
        let mut arms: Vec<String> = Vec::new();
        let mut binds: Vec<BindVal> = Vec::new();

        // MariaDB requires parentheses around each UNION branch that contains
        // ORDER BY / LIMIT, so every arm is wrapped in (...).
        if !series_ids.is_empty() {
            let placeholders = vec!["?"; series_ids.len()].join(", ");
            arms.push(format!(
                "(SELECT DISTINCT t.id, t.title, t.media_type, t.cover_image_url, 1 AS priority \
                 FROM titles t \
                 JOIN title_series ts ON ts.title_id = t.id AND ts.deleted_at IS NULL \
                 JOIN series s ON ts.series_id = s.id AND s.deleted_at IS NULL \
                 WHERE ts.series_id IN ({placeholders}) AND t.id <> ? AND t.deleted_at IS NULL \
                 ORDER BY t.id ASC LIMIT 16)"
            ));
            for sid in &series_ids {
                binds.push(BindVal::U64(*sid));
            }
            binds.push(BindVal::U64(title_id));
        }

        if !contributor_ids.is_empty() {
            let placeholders = vec!["?"; contributor_ids.len()].join(", ");
            arms.push(format!(
                "(SELECT DISTINCT t.id, t.title, t.media_type, t.cover_image_url, 2 AS priority \
                 FROM titles t \
                 JOIN title_contributors tc ON tc.title_id = t.id AND tc.deleted_at IS NULL \
                 JOIN contributors c ON c.id = tc.contributor_id AND c.deleted_at IS NULL \
                 WHERE tc.contributor_id IN ({placeholders}) AND t.id <> ? AND t.deleted_at IS NULL \
                 ORDER BY t.id ASC LIMIT 16)"
            ));
            for cid in &contributor_ids {
                binds.push(BindVal::U64(*cid));
            }
            binds.push(BindVal::U64(title_id));
        }

        if let Some((start, end)) = decade_bounds {
            arms.push(
                "(SELECT t.id, t.title, t.media_type, t.cover_image_url, 3 AS priority \
                 FROM titles t \
                 WHERE t.genre_id = ? \
                   AND t.publication_date IS NOT NULL \
                   AND YEAR(t.publication_date) BETWEEN ? AND ? \
                   AND t.id <> ? \
                   AND t.deleted_at IS NULL \
                 ORDER BY t.id ASC LIMIT 16)"
                    .to_string(),
            );
            binds.push(BindVal::U64(anchor.genre_id));
            binds.push(BindVal::I32(start));
            binds.push(BindVal::I32(end));
            binds.push(BindVal::U64(title_id));
        }

        // Safety: arms is non-empty because at least one criterion is present (checked above)
        let union_sql = arms.join(" UNION ALL ");

        // Outer SELECT: dedup via GROUP BY + MIN(priority), attach primary contributor,
        // then order by priority ASC, id ASC and take top 8.
        //
        // CAST(u.id AS SIGNED) because MariaDB propagates BIGINT UNSIGNED from the inner
        // titles.id, and SQLx cannot decode BIGINT UNSIGNED into Rust integers (see
        // CLAUDE.md — MariaDB type gotchas). Read as i64, convert to u64 on the Rust side.
        let full_sql = format!(
            "SELECT CAST(u.id AS SIGNED) AS id, u.title, u.media_type, u.cover_image_url, \
             MIN(u.priority) AS priority, \
             (SELECT c.name FROM title_contributors tc \
              JOIN contributors c ON tc.contributor_id = c.id AND c.deleted_at IS NULL \
              JOIN contributor_roles cr ON tc.role_id = cr.id AND cr.deleted_at IS NULL \
              WHERE tc.title_id = u.id AND tc.deleted_at IS NULL \
              ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC \
              LIMIT 1) AS primary_contributor \
             FROM ({union_sql}) AS u \
             GROUP BY u.id, u.title, u.media_type, u.cover_image_url \
             ORDER BY priority ASC, id ASC \
             LIMIT 8"
        );

        let mut query = sqlx::query(&full_sql);
        for b in &binds {
            query = match b {
                BindVal::U64(v) => query.bind(v),
                BindVal::I32(v) => query.bind(v),
            };
        }

        let rows = query.fetch_all(pool).await?;

        let mut items: Vec<SimilarTitle> = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: i64 = row.try_get::<i64, _>("id")?;
            let priority_raw: i64 = row.try_get::<i64, _>("priority")?;
            // Explicit range check — silently clamping masks backend bugs
            // if a new arm ever yields an unexpected priority.
            let priority: u8 = match priority_raw {
                1..=3 => priority_raw as u8,
                other => {
                    return Err(AppError::Internal(format!(
                        "find_similar: unexpected priority value {other} (expected 1..=3)"
                    )))
                }
            };
            items.push(SimilarTitle {
                id: id as u64,
                title: row.try_get::<String, _>("title")?,
                media_type: row.try_get::<String, _>("media_type")?,
                cover_image_url: row.try_get::<Option<String>, _>("cover_image_url")?,
                primary_contributor: row.try_get::<Option<String>, _>("primary_contributor")?,
                priority,
            });
        }

        Ok(items)
    }
}

/// Compact row for the "Similar titles" section on /title/{id}.
/// Lean deliberately — no subtitle, no year, no volume count (UX-DR30 §24 anatomy).
#[derive(Debug, Clone)]
pub struct SimilarTitle {
    pub id: u64,
    pub title: String,
    pub media_type: String,
    pub cover_image_url: Option<String>,
    pub primary_contributor: Option<String>,
    /// 1 = same series, 2 = same contributor, 3 = same genre+decade.
    pub priority: u8,
}

/// Internal bind accumulator for the dynamic UNION in find_similar.
enum BindVal {
    U64(u64),
    I32(i32),
}

/// Compute the inclusive decade bounds for a publication date.
///
/// A "decade" is a 10-year span starting on a multiple of 10: e.g., 1957 → (1950, 1959),
/// 2020 → (2020, 2029), 2000 → (2000, 2009). Used by `find_similar` for the
/// genre+decade matching criterion (FR114).
pub fn decade_bounds_for_date(date: chrono::NaiveDate) -> (i32, i32) {
    use chrono::Datelike;
    let year = date.year();
    let start = year - year.rem_euclid(10);
    (start, start + 9)
}

/// Detect which metadata fields differ between an existing title and new form values.
/// Returns the names of fields that changed.
#[allow(clippy::too_many_arguments)]
pub fn detect_edited_fields(old: &TitleModel, new_title: &str, new_subtitle: Option<&str>,
    new_description: Option<&str>, new_publisher: Option<&str>, new_language: &str,
    new_genre_id: u64, new_publication_date: Option<chrono::NaiveDate>,
    new_dewey_code: Option<&str>, new_page_count: Option<i32>,
    new_track_count: Option<i32>, new_total_duration: Option<i32>,
    new_age_rating: Option<&str>, new_issue_number: Option<i32>,
) -> Vec<String> {
    let mut changed = Vec::new();

    if old.title != new_title { changed.push("title".to_string()); }
    if old.subtitle.as_deref() != new_subtitle { changed.push("subtitle".to_string()); }
    if old.description.as_deref() != new_description { changed.push("description".to_string()); }
    if old.publisher.as_deref() != new_publisher { changed.push("publisher".to_string()); }
    if old.language != new_language { changed.push("language".to_string()); }
    if old.genre_id != new_genre_id { changed.push("genre_id".to_string()); }
    if old.publication_date != new_publication_date { changed.push("publication_date".to_string()); }
    if old.dewey_code.as_deref() != new_dewey_code { changed.push("dewey_code".to_string()); }
    if old.page_count != new_page_count { changed.push("page_count".to_string()); }
    if old.track_count != new_track_count { changed.push("track_count".to_string()); }
    if old.total_duration != new_total_duration { changed.push("total_duration".to_string()); }
    if old.age_rating.as_deref() != new_age_rating { changed.push("age_rating".to_string()); }
    if old.issue_number != new_issue_number { changed.push("issue_number".to_string()); }

    changed
}

/// Search result row for as-you-type search.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: u64,
    pub title: String,
    pub subtitle: Option<String>,
    pub media_type: String,
    pub genre_name: String,
    pub primary_contributor: Option<String>,
    pub volume_count: u64,
    pub cover_image_url: Option<String>,
    pub publication_date: Option<chrono::NaiveDate>,
}

/// Allowed sort columns for search results (validated whitelist to prevent SQL injection).
const VALID_SORT_COLUMNS: &[&str] = &["title", "media_type", "genre_name", "volume_count"];
const VALID_SORT_DIRS: &[&str] = &["asc", "desc"];

fn validated_sort(sort: &Option<String>) -> &str {
    match sort {
        Some(s) if VALID_SORT_COLUMNS.contains(&s.as_str()) => s.as_str(),
        _ => "title",
    }
}

fn validated_dir(dir: &Option<String>) -> &str {
    match dir {
        Some(d) if VALID_SORT_DIRS.contains(&d.as_str()) => d.as_str(),
        _ => "asc",
    }
}

fn map_sort_to_column(sort: &str) -> &str {
    match sort {
        "title" => "t.title",
        "media_type" => "t.media_type",
        "genre_name" => "g.name",
        "volume_count" => "volume_count",
        _ => "t.title",
    }
}

impl TitleModel {
    /// Full-text search across titles with pagination, sorting, and optional genre/state filters.
    pub async fn active_search(
        pool: &DbPool,
        query: &str,
        genre_id: Option<u64>,
        volume_state: Option<String>,
        sort: &Option<String>,
        dir: &Option<String>,
        page: u32,
    ) -> Result<PaginatedList<SearchResult>, AppError> {
        let sort_col = validated_sort(sort);
        let sort_dir = validated_dir(dir);
        let sql_order_col = map_sort_to_column(sort_col);
        let offset = (page.saturating_sub(1)) * DEFAULT_PAGE_SIZE;
        let trimmed = query.trim();

        // Escape LIKE wildcards and strip FULLTEXT operators from user input for LIKE queries
        let cleaned_for_like: String = trimmed
            .chars()
            .filter(|c| !matches!(c, '+' | '-' | '~' | '<' | '>' | '(' | ')' | '"' | '@' | '*'))
            .collect();
        let escaped_for_like = cleaned_for_like
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");

        // Strip FULLTEXT boolean mode operators from user input
        let escaped_for_ft: String = trimmed
            .chars()
            .filter(|c| !matches!(c, '+' | '-' | '~' | '<' | '>' | '(' | ')' | '"' | '@' | '*'))
            .collect();

        // Build WHERE clauses — use BindValue enum to handle mixed types
        let mut conditions = vec!["t.deleted_at IS NULL".to_string()];
        let mut string_binds: Vec<String> = Vec::new();
        let mut genre_bind: Option<u64> = None;
        let mut state_bind: Option<String> = None;
        let mut extra_join = String::new();

        if !trimmed.is_empty() && (!escaped_for_ft.is_empty() || !escaped_for_like.is_empty()) {
            if escaped_for_ft.len() >= 3 {
                // Use FULLTEXT for 3+ chars (after stripping operators)
                conditions.push(
                    "(MATCH(t.title, t.subtitle, t.description) AGAINST(? IN BOOLEAN MODE) \
                     OR t.id IN (SELECT tc.title_id FROM title_contributors tc \
                     JOIN contributors c ON tc.contributor_id = c.id \
                     WHERE c.name LIKE ? AND tc.deleted_at IS NULL AND c.deleted_at IS NULL))"
                        .to_string(),
                );
                string_binds.push(format!("{}*", escaped_for_ft));
                string_binds.push(format!("%{}%", escaped_for_like));
            } else if !escaped_for_like.is_empty() {
                // LIKE fallback for < 3 chars
                conditions.push(
                    "(t.title LIKE ? OR t.subtitle LIKE ? \
                     OR t.id IN (SELECT tc.title_id FROM title_contributors tc \
                     JOIN contributors c ON tc.contributor_id = c.id \
                     WHERE c.name LIKE ? AND tc.deleted_at IS NULL AND c.deleted_at IS NULL))"
                        .to_string(),
                );
                let like_pattern = format!("%{}%", escaped_for_like);
                string_binds.push(like_pattern.clone());
                string_binds.push(like_pattern.clone());
                string_binds.push(like_pattern);
            }
        }

        if let Some(gid) = genre_id {
            conditions.push("t.genre_id = ?".to_string());
            genre_bind = Some(gid);
        }

        if let Some(ref state_name) = volume_state {
            // Filter titles that have at least one volume in the given state
            extra_join = " JOIN volumes vol_f ON vol_f.title_id = t.id AND vol_f.deleted_at IS NULL \
                           JOIN volume_states vs_f ON vol_f.condition_state_id = vs_f.id AND vs_f.deleted_at IS NULL"
                .to_string();
            conditions.push("vs_f.name = ?".to_string());
            state_bind = Some(state_name.clone());
        }

        let where_clause = conditions.join(" AND ");

        // Count query
        let count_sql = format!(
            "SELECT COUNT(DISTINCT t.id) as cnt FROM titles t \
             JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL{} \
             WHERE {}",
            extra_join, where_clause
        );

        let mut count_query = sqlx::query(&count_sql);
        for val in &string_binds {
            count_query = count_query.bind(val);
        }
        if let Some(gid) = genre_bind {
            count_query = count_query.bind(gid);
        }
        if let Some(ref sv) = state_bind {
            count_query = count_query.bind(sv);
        }
        let count_row = count_query.fetch_one(pool).await?;
        let total_items: i64 = count_row.try_get("cnt")?;

        // Data query
        let data_sql = format!(
            "SELECT DISTINCT t.id, t.title, t.subtitle, t.media_type, t.cover_image_url, CAST(t.publication_date AS DATE) AS publication_date, \
                    g.name AS genre_name, \
                    (SELECT c.name FROM title_contributors tc \
                     JOIN contributors c ON tc.contributor_id = c.id \
                     JOIN contributor_roles cr ON tc.role_id = cr.id \
                     WHERE tc.title_id = t.id AND tc.deleted_at IS NULL AND c.deleted_at IS NULL AND cr.deleted_at IS NULL \
                     ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC \
                     LIMIT 1) AS primary_contributor, \
                    (SELECT COUNT(*) FROM volumes v WHERE v.title_id = t.id AND v.deleted_at IS NULL) AS volume_count \
             FROM titles t \
             JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL{} \
             WHERE {} \
             ORDER BY {} {} \
             LIMIT ? OFFSET ?",
            extra_join, where_clause, sql_order_col, sort_dir
        );

        let mut data_query = sqlx::query(&data_sql);
        for val in &string_binds {
            data_query = data_query.bind(val);
        }
        if let Some(gid) = genre_bind {
            data_query = data_query.bind(gid);
        }
        if let Some(ref sv) = state_bind {
            data_query = data_query.bind(sv);
        }
        data_query = data_query.bind(DEFAULT_PAGE_SIZE).bind(offset);

        let rows = data_query.fetch_all(pool).await?;

        let items: Vec<SearchResult> = rows
            .iter()
            .map(|r| SearchResult {
                id: r.try_get("id").unwrap_or(0),
                title: r.try_get("title").unwrap_or_default(),
                subtitle: r.try_get("subtitle").unwrap_or(None),
                media_type: r.try_get("media_type").unwrap_or_default(),
                genre_name: r.try_get("genre_name").unwrap_or_default(),
                primary_contributor: r.try_get("primary_contributor").unwrap_or(None),
                volume_count: r
                    .try_get::<i64, _>("volume_count")
                    .map(|v| v as u64)
                    .unwrap_or(0),
                cover_image_url: r.try_get("cover_image_url").unwrap_or(None),
                publication_date: r.try_get("publication_date").unwrap_or(None),
            })
            .collect();

        Ok(PaginatedList::new(
            items,
            page,
            total_items as u64,
            Some(sort_col.to_string()),
            Some(sort_dir.to_string()),
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_result_construction() {
        let result = SearchResult {
            id: 1,
            title: "L'Étranger".to_string(),
            subtitle: None,
            media_type: "book".to_string(),
            genre_name: "Roman".to_string(),
            primary_contributor: Some("Albert Camus".to_string()),
            volume_count: 2,
            cover_image_url: None,
            publication_date: None,
        };
        assert_eq!(result.id, 1);
        assert_eq!(result.title, "L'Étranger");
        assert_eq!(result.primary_contributor, Some("Albert Camus".to_string()));
    }

    #[test]
    fn test_validated_sort_valid() {
        assert_eq!(validated_sort(&Some("title".to_string())), "title");
        assert_eq!(validated_sort(&Some("media_type".to_string())), "media_type");
        assert_eq!(validated_sort(&Some("genre_name".to_string())), "genre_name");
        assert_eq!(validated_sort(&Some("volume_count".to_string())), "volume_count");
    }

    #[test]
    fn test_validated_sort_rejects_dewey_code_on_search() {
        assert_eq!(validated_sort(&Some("dewey_code".to_string())), "title");
    }

    #[test]
    fn test_validated_sort_injection() {
        assert_eq!(validated_sort(&Some("DROP TABLE".to_string())), "title");
        assert_eq!(validated_sort(&Some("1; DROP TABLE--".to_string())), "title");
        assert_eq!(validated_sort(&None), "title");
    }

    #[test]
    fn test_validated_dir_valid() {
        assert_eq!(validated_dir(&Some("asc".to_string())), "asc");
        assert_eq!(validated_dir(&Some("desc".to_string())), "desc");
    }

    #[test]
    fn test_validated_dir_injection() {
        assert_eq!(validated_dir(&Some("DROP".to_string())), "asc");
        assert_eq!(validated_dir(&None), "asc");
    }

    #[test]
    fn test_map_sort_to_column() {
        assert_eq!(map_sort_to_column("title"), "t.title");
        assert_eq!(map_sort_to_column("media_type"), "t.media_type");
        assert_eq!(map_sort_to_column("genre_name"), "g.name");
        assert_eq!(map_sort_to_column("volume_count"), "volume_count");
        assert_eq!(map_sort_to_column("unknown"), "t.title");
    }

    // ─── Similar titles — decade bounds (pure function) ────────────────
    // The decade computation is the only non-SQL-bound logic in find_similar.
    // DB-backed tests for the full query live in E2E (no Rust integration test
    // infrastructure in this project).

    #[test]
    fn test_decade_bounds_start_of_decade() {
        let d = chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        assert_eq!(decade_bounds_for_date(d), (2020, 2029));
    }

    #[test]
    fn test_decade_bounds_middle_of_decade() {
        let d = chrono::NaiveDate::from_ymd_opt(1957, 6, 19).unwrap();
        assert_eq!(decade_bounds_for_date(d), (1950, 1959));
    }

    #[test]
    fn test_decade_bounds_end_of_decade() {
        let d = chrono::NaiveDate::from_ymd_opt(1999, 12, 31).unwrap();
        assert_eq!(decade_bounds_for_date(d), (1990, 1999));
    }

    #[test]
    fn test_decade_bounds_year_2000() {
        let d = chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
        assert_eq!(decade_bounds_for_date(d), (2000, 2009));
    }

    #[test]
    fn test_decade_bounds_year_1900() {
        let d = chrono::NaiveDate::from_ymd_opt(1900, 7, 4).unwrap();
        assert_eq!(decade_bounds_for_date(d), (1900, 1909));
    }

    #[test]
    fn test_similar_title_struct_construction() {
        let st = SimilarTitle {
            id: 42,
            title: "La Peste".to_string(),
            media_type: "book".to_string(),
            cover_image_url: Some("https://example.com/cover.jpg".to_string()),
            primary_contributor: Some("Albert Camus".to_string()),
            priority: 2,
        };
        assert_eq!(st.id, 42);
        assert_eq!(st.priority, 2);
        assert_eq!(st.primary_contributor.as_deref(), Some("Albert Camus"));
    }

    #[test]
    fn test_title_model_display() {
        let title = TitleModel {
            id: 1,
            title: "L'Étranger".to_string(),
            subtitle: None,
            description: None,
            language: "fr".to_string(),
            media_type: "book".to_string(),
            publication_date: None,
            publisher: None,
            isbn: Some("9782070360246".to_string()),
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: None,
            page_count: Some(186),
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            manually_edited_fields: None,
            version: 1,
        };
        assert_eq!(title.to_string(), "L'Étranger (book)");
    }

    #[test]
    fn test_title_model_display_cd() {
        let title = TitleModel {
            id: 2,
            title: "Kind of Blue".to_string(),
            subtitle: None,
            description: None,
            language: "en".to_string(),
            media_type: "cd".to_string(),
            publication_date: None,
            publisher: None,
            isbn: None,
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 6,
            dewey_code: None,
            page_count: None,
            track_count: Some(5),
            total_duration: Some(2756),
            age_rating: None,
            issue_number: None,
            manually_edited_fields: None,
            version: 1,
        };
        assert_eq!(title.to_string(), "Kind of Blue (cd)");
    }

    fn make_test_title() -> TitleModel {
        TitleModel {
            id: 1,
            title: "Original Title".to_string(),
            subtitle: Some("Original Sub".to_string()),
            description: None,
            language: "fr".to_string(),
            media_type: "book".to_string(),
            publication_date: None,
            publisher: Some("Gallimard".to_string()),
            isbn: Some("9782070360246".to_string()),
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: None,
            page_count: Some(186),
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            manually_edited_fields: None,
            version: 1,
        }
    }

    #[test]
    fn test_detect_edited_fields_no_changes() {
        let old = make_test_title();
        let changed = detect_edited_fields(
            &old, "Original Title", Some("Original Sub"), None,
            Some("Gallimard"), "fr", 1, None, None,
            Some(186), None, None, None, None,
        );
        assert!(changed.is_empty());
    }

    #[test]
    fn test_detect_edited_fields_publisher_changed() {
        let old = make_test_title();
        let changed = detect_edited_fields(
            &old, "Original Title", Some("Original Sub"), None,
            Some("Flammarion"), "fr", 1, None, None,
            Some(186), None, None, None, None,
        );
        assert_eq!(changed, vec!["publisher"]);
    }

    #[test]
    fn test_detect_edited_fields_multiple_changes() {
        let old = make_test_title();
        let changed = detect_edited_fields(
            &old, "New Title", Some("Original Sub"), Some("A description"),
            Some("Gallimard"), "en", 1, None, None,
            Some(186), None, None, None, None,
        );
        assert!(changed.contains(&"title".to_string()));
        assert!(changed.contains(&"description".to_string()));
        assert!(changed.contains(&"language".to_string()));
        assert_eq!(changed.len(), 3);
    }

    #[test]
    fn test_parsed_manually_edited_fields_none() {
        let title = make_test_title();
        assert!(title.parsed_manually_edited_fields().is_empty());
    }

    #[test]
    fn test_parsed_manually_edited_fields_valid_json() {
        let mut title = make_test_title();
        title.manually_edited_fields = Some(r#"["publisher","description"]"#.to_string());
        let fields = title.parsed_manually_edited_fields();
        assert_eq!(fields, vec!["publisher", "description"]);
    }

    #[test]
    fn test_parsed_manually_edited_fields_invalid_json() {
        let mut title = make_test_title();
        title.manually_edited_fields = Some("not json".to_string());
        assert!(title.parsed_manually_edited_fields().is_empty());
    }
}
