//! Search projection over `titles` (moved out of `title.rs`, #516):
//! the as-you-type / browse search (`active_search`), the recent-titles
//! list and, since story 11-8, the incomplete-metadata list for MCP agents.

use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::title::TitleModel;
use crate::models::{DEFAULT_PAGE_SIZE, PaginatedList};

/// The main contributor of title `t`: an "Auteur" first, else the
/// oldest live junction row (live contributor and role). Same choice as
/// the other title lists.
const PRIMARY_CONTRIBUTOR_SUBQUERY: &str = "(SELECT c.name FROM title_contributors tc \
     JOIN contributors c ON tc.contributor_id = c.id \
     JOIN contributor_roles cr ON tc.role_id = cr.id \
     WHERE tc.title_id = t.id AND tc.deleted_at IS NULL \
       AND c.deleted_at IS NULL AND cr.deleted_at IS NULL \
     ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC \
     LIMIT 1)";

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
    // CR #250 — Dewey code surfaced on the home list-view table so the
    // sortable column has a value to render. Already present on the
    // `titles` row; this just adds it to the search projection.
    pub dewey_code: Option<String>,
}

/// Allowed sort columns for search results (validated whitelist to prevent SQL injection).
/// CR #250 adds `primary_contributor` + `dewey_code` for the list-view
/// table's sortable column headers.
const VALID_SORT_COLUMNS: &[&str] = &[
    "title",
    "media_type",
    "genre_name",
    "volume_count",
    "primary_contributor",
    "dewey_code",
];
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
        // CR #250 — sort by the alias produced in the subquery (the
        // SELECT projection emits `primary_contributor` from a
        // correlated subquery; MariaDB resolves the alias in ORDER BY).
        "primary_contributor" => "primary_contributor",
        "dewey_code" => "t.dewey_code",
        _ => "t.title",
    }
}

impl TitleModel {
    /// Full-text search across titles with pagination, sorting, and optional genre/state filters.
    #[allow(clippy::too_many_arguments)]
    pub async fn active_search(
        pool: &DbPool,
        query: &str,
        genre_id: Option<u64>,
        volume_state: Option<String>,
        sort: &Option<String>,
        dir: &Option<String>,
        page: u32,
        // CR #279 — when `true`, restrict to titles that have no
        // active volume row. Implemented as a `NOT EXISTS` guard
        // because LEFT JOIN + COUNT(...) = 0 would require GROUP BY,
        // which collides with the DISTINCT shape the data query
        // already needs.
        no_volumes_only: bool,
        // CR #355 — when `true`, restrict to titles with no cover image.
        // Same predicate the bulk cover-refetch uses (#214), minus the
        // identifier clause: titles WITHOUT an ISBN/ISSN/UPC can never be
        // auto-fetched, so they most need the manual-upload safety net.
        no_cover_only: bool,
        // Story 11-7 — exact media type (`book`, `bd`, …); bound, never
        // interpolated. Callers validate the value against `MediaType`.
        media_type: Option<&str>,
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

        if media_type.is_some() {
            conditions.push("t.media_type = ?".to_string());
        }

        if no_volumes_only {
            // CR #279 — title with zero active volumes.
            conditions.push(
                "NOT EXISTS (SELECT 1 FROM volumes v_no WHERE v_no.title_id = t.id \
                 AND v_no.deleted_at IS NULL)"
                    .to_string(),
            );
        }

        if no_cover_only {
            // CR #355 — title with no cover image (NULL or empty string).
            // No bind value: self-contained predicate on the titles table.
            conditions.push("(t.cover_image_url IS NULL OR t.cover_image_url = '')".to_string());
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
        // Fix #107: LEFT JOIN keeps orphan-genre titles in the count so it
        // matches the data-query row count. See the data-query comment for
        // the full rationale.
        let count_sql = format!(
            "SELECT COUNT(DISTINCT t.id) as cnt FROM titles t \
             LEFT JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL{} \
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
        if let Some(mt) = media_type {
            count_query = count_query.bind(mt);
        }
        if let Some(ref sv) = state_bind {
            count_query = count_query.bind(sv);
        }
        let count_row = count_query.fetch_one(pool).await?;
        let total_items: i64 = count_row.try_get("cnt")?;

        // Data query
        // Fix #107: LEFT JOIN + COALESCE on the genre relation so a title
        // whose genre row has been soft-deleted (orphan FK) still shows up
        // in the result set with an empty `genre_name` marker. The route
        // layer's `resolve_genre_placeholder` substitutes a locale-aware
        // "(Genre supprimé)" / "(Deleted genre)" before rendering, so the
        // title remains discoverable instead of silently disappearing from
        // the catalog. The story-8-4 deletion guard makes this unreachable
        // via the supported delete path; this is defense-in-depth against
        // a future migration or manual UPDATE that bypasses the guard.
        let data_sql = format!(
            "SELECT DISTINCT t.id, t.title, t.subtitle, t.media_type, t.cover_image_url, t.dewey_code, CAST(t.publication_date AS DATE) AS publication_date, \
                    COALESCE(g.name, '') AS genre_name, \
                    (SELECT c.name FROM title_contributors tc \
                     JOIN contributors c ON tc.contributor_id = c.id \
                     JOIN contributor_roles cr ON tc.role_id = cr.id \
                     WHERE tc.title_id = t.id AND tc.deleted_at IS NULL AND c.deleted_at IS NULL AND cr.deleted_at IS NULL \
                     ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC \
                     LIMIT 1) AS primary_contributor, \
                    (SELECT COUNT(*) FROM volumes v WHERE v.title_id = t.id AND v.deleted_at IS NULL) AS volume_count \
             FROM titles t \
             LEFT JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL{} \
             WHERE {} \
             ORDER BY {} {}, t.id ASC \
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
        if let Some(mt) = media_type {
            data_query = data_query.bind(mt);
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
                dewey_code: r.try_get("dewey_code").unwrap_or(None),
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

    /// List the N most recently created active titles, fully enriched for
    /// TitleCard rendering (story 9-2).
    ///
    /// Single SQL round-trip — mirrors the projection used by `active_search`
    /// (genre name, primary contributor subquery with "Auteur" preference,
    /// volume count subquery) but drops the FULLTEXT/LIKE/filter/sort layers
    /// in favor of a fixed `ORDER BY t.created_at DESC LIMIT ?`. Role-agnostic;
    /// the same query is issued for Anonymous, Librarian, and Admin requests.
    ///
    /// Returns an empty `Vec` on an empty DB (not an error).
    pub async fn list_recent_active(
        pool: &DbPool,
        limit: u32,
    ) -> Result<Vec<SearchResult>, AppError> {
        let rows = sqlx::query(
            // Fix #107: same LEFT JOIN + COALESCE pattern as
            // `list_recent_cataloged` and `active_search`. See those for
            // rationale.
            "SELECT t.id, t.title, t.subtitle, t.media_type, t.cover_image_url, t.dewey_code, \
                    CAST(t.publication_date AS DATE) AS publication_date, \
                    COALESCE(g.name, '') AS genre_name, \
                    (SELECT c.name FROM title_contributors tc \
                     JOIN contributors c ON tc.contributor_id = c.id \
                     JOIN contributor_roles cr ON tc.role_id = cr.id \
                     WHERE tc.title_id = t.id AND tc.deleted_at IS NULL \
                       AND c.deleted_at IS NULL AND cr.deleted_at IS NULL \
                     ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC \
                     LIMIT 1) AS primary_contributor, \
                    (SELECT COUNT(*) FROM volumes v WHERE v.title_id = t.id \
                       AND v.deleted_at IS NULL) AS volume_count \
             FROM titles t \
             LEFT JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL \
             WHERE t.deleted_at IS NULL \
             ORDER BY t.created_at DESC, t.id DESC \
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;

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
                dewey_code: r.try_get("dewey_code").unwrap_or(None),
            })
            .collect();

        Ok(items)
    }
}

/// Story 11-8 — a bibliographic field whose absence makes a title
/// "incomplete" for the MCP `list_titles_with_incomplete_metadata` tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataField {
    Description,
    Publisher,
    Contributor,
    PublicationYear,
}

impl MetadataField {
    /// Every field, in the order the tool reports them.
    pub const ALL: [MetadataField; 4] = [
        MetadataField::Description,
        MetadataField::Publisher,
        MetadataField::Contributor,
        MetadataField::PublicationYear,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MetadataField::Description => "description",
            MetadataField::Publisher => "publisher",
            MetadataField::Contributor => "contributor",
            MetadataField::PublicationYear => "publication_year",
        }
    }

    /// Parse the tool's argument value (exact, lower-case).
    pub fn parse(value: &str) -> Option<MetadataField> {
        Self::ALL.into_iter().find(|f| f.as_str() == value)
    }

    /// SQL predicate on `titles t` that holds when the field is missing.
    /// Fixed text chosen from this enum — never user input. A blank
    /// string counts as missing; a contributor counts only through a
    /// live junction row, live contributor and live role (the rule
    /// `get_title` lists contributors by).
    fn missing_predicate(self) -> &'static str {
        match self {
            MetadataField::Description => "(t.description IS NULL OR TRIM(t.description) = '')",
            MetadataField::Publisher => "(t.publisher IS NULL OR TRIM(t.publisher) = '')",
            MetadataField::Contributor => {
                "NOT EXISTS (SELECT 1 FROM title_contributors tc \
                   JOIN contributors c ON tc.contributor_id = c.id AND c.deleted_at IS NULL \
                   JOIN contributor_roles cr ON tc.role_id = cr.id AND cr.deleted_at IS NULL \
                   WHERE tc.title_id = t.id AND tc.deleted_at IS NULL)"
            }
            MetadataField::PublicationYear => "t.publication_date IS NULL",
        }
    }
}

/// Story 11-8 — one title missing at least one [`MetadataField`].
#[derive(Debug, Clone)]
pub struct IncompleteTitleRow {
    pub id: u64,
    pub title: String,
    pub subtitle: Option<String>,
    pub media_type: String,
    pub genre_name: String,
    pub isbn: Option<String>,
    pub primary_contributor: Option<String>,
    pub cover_image_url: Option<String>,
    pub publication_date: Option<chrono::NaiveDate>,
    /// The missing fields, in [`MetadataField::ALL`] order.
    pub missing: Vec<MetadataField>,
}

/// `WHERE` filter of [`TitleModel::list_incomplete_metadata`]: the
/// title misses `only`, or any field when `only` is `None`.
fn incomplete_filter(only: Option<MetadataField>) -> String {
    match only {
        Some(field) => field.missing_predicate().to_string(),
        None => format!(
            "({})",
            MetadataField::ALL
                .iter()
                .map(|f| f.missing_predicate())
                .collect::<Vec<_>>()
                .join(" OR ")
        ),
    }
}

impl TitleModel {
    /// Story 11-8 — one page of live titles missing `only` (or any
    /// [`MetadataField`] when `None`), optionally of one media type and
    /// genre, sorted by title then id. Page size `DEFAULT_PAGE_SIZE`.
    pub async fn list_incomplete_metadata(
        pool: &DbPool,
        only: Option<MetadataField>,
        media_type: Option<&str>,
        genre_id: Option<u64>,
        page: u32,
    ) -> Result<PaginatedList<IncompleteTitleRow>, AppError> {
        let offset = page.saturating_sub(1).saturating_mul(DEFAULT_PAGE_SIZE);
        let filter = format!(
            "t.deleted_at IS NULL AND {} \
             AND (? IS NULL OR t.media_type = ?) \
             AND (? IS NULL OR t.genre_id = ?)",
            incomplete_filter(only)
        );

        let count_sql = format!("SELECT COUNT(*) FROM titles t WHERE {filter}");
        let (total,): (i64,) = sqlx::query_as(&count_sql)
            .bind(media_type)
            .bind(media_type)
            .bind(genre_id)
            .bind(genre_id)
            .fetch_one(pool)
            .await?;

        let flags: String = MetadataField::ALL
            .iter()
            .map(|f| {
                format!(
                    "CASE WHEN {} THEN 1 ELSE 0 END AS missing_{}, ",
                    f.missing_predicate(),
                    f.as_str()
                )
            })
            .collect();
        let page_sql = format!(
            "SELECT t.id, t.title, t.subtitle, t.media_type, t.isbn, t.cover_image_url, \
                    CAST(t.publication_date AS DATE) AS publication_date, \
                    COALESCE(g.name, '') AS genre_name, {flags}\
                    {primary} AS primary_contributor \
             FROM titles t \
             LEFT JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL \
             WHERE {filter} \
             ORDER BY t.title ASC, t.id ASC \
             LIMIT ? OFFSET ?",
            primary = PRIMARY_CONTRIBUTOR_SUBQUERY,
        );
        let rows = sqlx::query(&page_sql)
            .bind(media_type)
            .bind(media_type)
            .bind(genre_id)
            .bind(genre_id)
            .bind(DEFAULT_PAGE_SIZE)
            .bind(offset)
            .fetch_all(pool)
            .await?;

        let items = rows
            .into_iter()
            .map(|r| {
                let mut missing = Vec::new();
                for field in MetadataField::ALL {
                    let flag: i64 = r.try_get(format!("missing_{}", field.as_str()).as_str())?;
                    if flag != 0 {
                        missing.push(field);
                    }
                }
                Ok(IncompleteTitleRow {
                    id: r.try_get("id")?,
                    title: r.try_get("title")?,
                    subtitle: r.try_get("subtitle")?,
                    media_type: r.try_get("media_type")?,
                    genre_name: r.try_get("genre_name")?,
                    isbn: r.try_get("isbn")?,
                    primary_contributor: r.try_get("primary_contributor")?,
                    cover_image_url: r.try_get("cover_image_url")?,
                    publication_date: r.try_get("publication_date")?,
                    missing,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;

        Ok(PaginatedList::new(
            items,
            page,
            u64::try_from(total).unwrap_or(0),
            None,
            None,
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
            dewey_code: None,
        };
        assert_eq!(result.id, 1);
        assert_eq!(result.title, "L'Étranger");
        assert_eq!(result.primary_contributor, Some("Albert Camus".to_string()));
    }

    #[test]
    fn test_validated_sort_valid() {
        assert_eq!(validated_sort(&Some("title".to_string())), "title");
        assert_eq!(
            validated_sort(&Some("media_type".to_string())),
            "media_type"
        );
        assert_eq!(
            validated_sort(&Some("genre_name".to_string())),
            "genre_name"
        );
        assert_eq!(
            validated_sort(&Some("volume_count".to_string())),
            "volume_count"
        );
    }

    // CR #250 expanded `VALID_SORT_COLUMNS` to include `dewey_code`
    // (and `primary_contributor`) so the home list-view table headers
    // can sort by them. The historical "rejects" test is superseded by
    // `test_validated_sort_accepts_cr250_columns` below.

    #[test]
    fn test_validated_sort_injection() {
        assert_eq!(validated_sort(&Some("DROP TABLE".to_string())), "title");
        assert_eq!(
            validated_sort(&Some("1; DROP TABLE--".to_string())),
            "title"
        );
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
        // CR #250 — sortable columns on the home list-view table.
        assert_eq!(
            map_sort_to_column("primary_contributor"),
            "primary_contributor"
        );
        assert_eq!(map_sort_to_column("dewey_code"), "t.dewey_code");
        assert_eq!(map_sort_to_column("unknown"), "t.title");
    }

    /// CR #250 — lock the SQL-injection guard for the two new sortable
    /// columns. Anything the table headers can send must be in the
    /// whitelist; anything else falls back to `title`.
    #[test]
    fn test_validated_sort_accepts_cr250_columns() {
        assert_eq!(
            validated_sort(&Some("primary_contributor".to_string())),
            "primary_contributor"
        );
        assert_eq!(
            validated_sort(&Some("dewey_code".to_string())),
            "dewey_code"
        );
        // Adjacent values that *look* close but aren't in the whitelist
        // must fall back to the default, not get partially accepted.
        assert_eq!(
            validated_sort(&Some("primary_contributor; --".to_string())),
            "title"
        );
        assert_eq!(
            validated_sort(&Some("dewey".to_string())),
            "title"
        );
    }
}
