use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

// #516 — the search projection lives in `title_search.rs`; re-exported
// so `models::title::SearchResult` keeps working for every caller.
pub use super::title_search::SearchResult;

/// Story 11-18 — the columns an accepted metadata proposal writes
/// (`None` = not proposed, left as it is).
#[derive(Debug, Clone, Copy, Default)]
pub struct ProposedFieldWrites<'a> {
    /// Story 12-5 (#563) — never empty (`titles.title` is NOT NULL).
    pub title: Option<&'a str>,
    pub subtitle: Option<&'a str>,
    pub description: Option<&'a str>,
    pub publisher: Option<&'a str>,
    pub publication_date: Option<NaiveDate>,
    pub dewey_code: Option<&'a str>,
}

/// Matches the `titles` table schema exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleModel {
    pub id: u64,
    pub title: String,
    /// UNIMARC 454/500 — title of the original work (for translations).
    pub original_title: Option<String>,
    pub subtitle: Option<String>,
    /// UNIMARC 200$f — statement of responsibility.
    pub statement_of_responsibility: Option<String>,
    pub description: Option<String>,
    /// UNIMARC 300$a — general note.
    pub general_note: Option<String>,
    pub language: String,
    pub media_type: String,
    pub publication_date: Option<NaiveDate>,
    pub publisher: Option<String>,
    /// UNIMARC 205$a — edition statement.
    pub edition_statement: Option<String>,
    /// UNIMARC 225$a — collection (series) title.
    pub collection_title: Option<String>,
    /// UNIMARC 225$v — collection (series) number.
    pub collection_number: Option<String>,
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

/// Fix #214: projection used by the bulk cover-refetch admin action.
/// Carries only the fields `fetch_metadata_chain` needs.
#[derive(Debug, Clone)]
pub struct MissingCoverTitle {
    pub id: u64,
    pub code: String,
    pub code_type: crate::models::media_type::CodeType,
    pub media_type: String,
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
    // UNIMARC Palier 1 (#389) zones — populated by the BnF backfill.
    pub statement_of_responsibility: Option<String>,
    pub edition_statement: Option<String>,
    pub collection_title: Option<String>,
    pub collection_number: Option<String>,
    pub general_note: Option<String>,
    pub original_title: Option<String>,
}

fn row_to_title(row: sqlx::mysql::MySqlRow) -> Result<TitleModel, sqlx::Error> {
    Ok(TitleModel {
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        original_title: row.try_get("original_title")?,
        subtitle: row.try_get("subtitle")?,
        statement_of_responsibility: row.try_get("statement_of_responsibility")?,
        description: row.try_get("description")?,
        general_note: row.try_get("general_note")?,
        language: row.try_get("language")?,
        media_type: row.try_get("media_type")?,
        publication_date: row.try_get("publication_date")?,
        publisher: row.try_get("publisher")?,
        edition_statement: row.try_get("edition_statement")?,
        collection_title: row.try_get("collection_title")?,
        collection_number: row.try_get("collection_number")?,
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

/// Full `TitleModel` projection of the live titles carrying one ISBN
/// (bind: the ISBN). Shared by `find_by_isbn` and `list_by_isbn`.
const SELECT_LIVE_BY_ISBN: &str = r#"SELECT id, title, original_title, subtitle, statement_of_responsibility,
                      description, general_note, language,
                      media_type, publication_date, publisher,
                      edition_statement, collection_title, collection_number,
                      isbn, issn, upc,
                      cover_image_url, genre_id, dewey_code,
                      page_count, track_count, total_duration,
                      age_rating, issue_number,
                      CAST(manually_edited_fields AS CHAR) as manually_edited_fields, version
               FROM titles
               WHERE isbn = ? AND deleted_at IS NULL"#;

impl TitleModel {
    pub async fn find_by_isbn(pool: &DbPool, isbn: &str) -> Result<Option<TitleModel>, AppError> {
        tracing::debug!(isbn = %isbn, "Looking up title by ISBN");

        let row = sqlx::query(&format!("{SELECT_LIVE_BY_ISBN} LIMIT 1"))
            .bind(isbn)
            .fetch_optional(pool)
            .await?;

        match row {
            Some(r) => Ok(Some(row_to_title(r)?)),
            None => Ok(None),
        }
    }

    /// Story 11-7 — every live title carrying `isbn`, oldest first.
    /// `titles.isbn` has no UNIQUE key (the scan flow prevents duplicates,
    /// a manual edit does not), so an agent's exact lookup must see them
    /// all.
    pub async fn list_by_isbn(pool: &DbPool, isbn: &str) -> Result<Vec<TitleModel>, AppError> {
        let rows = sqlx::query(&format!("{SELECT_LIVE_BY_ISBN} ORDER BY id ASC"))
            .bind(isbn)
            .fetch_all(pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(row_to_title)
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// Story 9-9 — narrow lookup returning ONLY the title id for a
    /// given ISBN. Used by the home-page scan-to-navigate handler
    /// (`/scan?code=…`) which only needs to redirect to `/title/:id`.
    /// Sibling of `find_by_isbn` (which fetches the full TitleModel).
    pub async fn find_id_by_isbn(
        pool: &DbPool,
        isbn: &str,
    ) -> Result<Option<u64>, AppError> {
        let id = sqlx::query_scalar::<_, u64>(
            "SELECT id FROM titles WHERE isbn = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(isbn)
        .fetch_optional(pool)
        .await?;
        Ok(id)
    }

    pub async fn find_by_upc(pool: &DbPool, upc: &str) -> Result<Option<TitleModel>, AppError> {
        tracing::debug!(upc = %upc, "Looking up title by UPC");

        let row = sqlx::query(
            r#"SELECT id, title, original_title, subtitle, statement_of_responsibility,
                      description, general_note, language,
                      media_type, publication_date, publisher,
                      edition_statement, collection_title, collection_number,
                      isbn, issn, upc,
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
            r#"SELECT id, title, original_title, subtitle, statement_of_responsibility,
                      description, general_note, language,
                      media_type, publication_date, publisher,
                      edition_statement, collection_title, collection_number,
                      isbn, issn, upc,
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
        let mut conn = pool.acquire().await?;
        Self::find_by_id_in(&mut conn, id, false).await
    }

    /// [`Self::find_by_id`] on the given connection. With `lock` the read
    /// is `FOR UPDATE` (story 11-18: a proposal acceptance reads the live
    /// values it compares and overwrites under the title row lock).
    pub async fn find_by_id_in(
        conn: &mut sqlx::MySqlConnection,
        id: u64,
        lock: bool,
    ) -> Result<Option<TitleModel>, AppError> {
        tracing::debug!(id = id, "Looking up title by ID");

        let sql = format!(
            r#"SELECT id, title, original_title, subtitle, statement_of_responsibility,
                      description, general_note, language,
                      media_type, publication_date, publisher,
                      edition_statement, collection_title, collection_number,
                      isbn, issn, upc,
                      cover_image_url, genre_id, dewey_code,
                      page_count, track_count, total_duration,
                      age_rating, issue_number,
                      CAST(manually_edited_fields AS CHAR) as manually_edited_fields, version
               FROM titles
               WHERE id = ? AND deleted_at IS NULL{}"#,
            if lock { " FOR UPDATE" } else { "" }
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;

        match row {
            Some(r) => Ok(Some(row_to_title(r)?)),
            None => Ok(None),
        }
    }

    /// Story 11-18 — write the fields of an accepted metadata proposal
    /// (each `Some` column is set; `None` leaves it as it is), the new
    /// `manually_edited_fields` set, and bump `version`. The caller holds
    /// the title row lock (`find_by_id_in(…, true)`).
    pub async fn apply_proposed_fields_in(
        conn: &mut sqlx::MySqlConnection,
        id: u64,
        fields: &ProposedFieldWrites<'_>,
        manually_edited_json: &str,
    ) -> Result<(), AppError> {
        let mut sets: Vec<&str> = Vec::new();
        if let Some(t) = fields.title {
            debug_assert!(!t.trim().is_empty(), "a proposal never empties the title");
            sets.push("title = ?");
        }
        if fields.subtitle.is_some() {
            sets.push("subtitle = ?");
        }
        if fields.description.is_some() {
            sets.push("description = ?");
        }
        if fields.publisher.is_some() {
            sets.push("publisher = ?");
        }
        if fields.publication_date.is_some() {
            sets.push("publication_date = ?");
        }
        if fields.dewey_code.is_some() {
            sets.push("dewey_code = ?");
        }
        let sql = format!(
            "UPDATE titles SET {}manually_edited_fields = ?, version = version + 1, \
             updated_at = NOW() WHERE id = ? AND deleted_at IS NULL",
            sets.iter().map(|s| format!("{s}, ")).collect::<String>()
        );
        let mut q = sqlx::query(&sql);
        for v in [fields.title, fields.subtitle, fields.description, fields.publisher]
            .into_iter()
            .flatten()
        {
            q = q.bind(v);
        }
        if let Some(d) = fields.publication_date {
            q = q.bind(d);
        }
        if let Some(v) = fields.dewey_code {
            q = q.bind(v);
        }
        let done = q
            .bind(manually_edited_json)
            .bind(id)
            .execute(&mut *conn)
            .await?;
        if done.rows_affected() != 1 {
            return Err(AppError::Internal(format!(
                "title {id} not updated by an accepted proposal"
            )));
        }
        Ok(())
    }

    /// Story 11-18 — the decision half of an accepted cover proposal:
    /// `cover_image_url` joins `manually_edited_fields` (so a background
    /// cover fetch landing before the promotion leaves the title alone)
    /// and `version` is bumped (the other pending proposals for the title
    /// go stale at their own acceptance). The caller holds the row lock.
    pub async fn mark_cover_accepted_in(
        conn: &mut sqlx::MySqlConnection,
        id: u64,
        manually_edited_json: &str,
    ) -> Result<(), AppError> {
        let done = sqlx::query(
            "UPDATE titles SET manually_edited_fields = ?, version = version + 1, \
             updated_at = NOW() WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(manually_edited_json)
        .bind(id)
        .execute(&mut *conn)
        .await?;
        if done.rows_affected() != 1 {
            return Err(AppError::Internal(format!(
                "title {id} not updated by an accepted cover"
            )));
        }
        Ok(())
    }

    /// Story 11-18 — take `field` out of `manually_edited_fields` (an
    /// accepted cover whose file could not be installed), in a short
    /// transaction under the title row lock.
    pub async fn unmark_manually_edited(
        pool: &DbPool,
        id: u64,
        field: &str,
    ) -> Result<(), AppError> {
        let mut tx = pool.begin().await?;
        let Some(title) = Self::find_by_id_in(&mut tx, id, true).await? else {
            return Ok(());
        };
        let kept: Vec<String> = title
            .parsed_manually_edited_fields()
            .into_iter()
            .filter(|f| f != field)
            .collect();
        let json = (!kept.is_empty())
            .then(|| serde_json::to_string(&kept).unwrap_or_else(|_| "[]".into()));
        sqlx::query(
            "UPDATE titles SET manually_edited_fields = ?, updated_at = NOW() \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(json)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Story 11-18 — record a promoted cover's URL; `false` when the
    /// title is gone (soft-deleted since the decision). No `version`
    /// bump: the decision already bumped it.
    pub async fn set_cover_url(pool: &DbPool, id: u64, cover_url: &str) -> Result<bool, AppError> {
        let done = sqlx::query(
            "UPDATE titles SET cover_image_url = ?, updated_at = NOW() \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(cover_url)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    /// #202 — which provider resolved this title's metadata, or `None` when
    /// no source has ever been recorded (every row predating the column, and
    /// any title whose only successful fetch came from the cache).
    ///
    /// A dedicated one-column query rather than a new `TitleModel` field on
    /// purpose. `TitleModel` is built by hand from 31 explicit SELECT lists
    /// across this file; widening the struct would touch every one of them for
    /// a value that exactly one page renders. The cost of the extra round-trip
    /// is a single indexed primary-key lookup on a page that already issues
    /// several.
    pub async fn find_metadata_source(
        pool: &DbPool,
        id: u64,
    ) -> Result<Option<String>, AppError> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT metadata_source FROM titles WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        // Flatten "no such title" and "title with no source" — both mean the
        // badge has nothing to show, and the caller has already established
        // the title exists.
        Ok(row.and_then(|(source,)| source).filter(|s| !s.is_empty()))
    }

    /// Fix #214: count active titles eligible for a bulk cover-refetch.
    ///
    /// Eligibility: `cover_image_url IS NULL OR ''` AND at least one
    /// lookup code (ISBN / ISSN / UPC) — the bulk-fetch reuses the
    /// per-title metadata chain, which only operates on codes. Titles
    /// without any code can't be re-fetched here; they'd be a manual
    /// catalog edit.
    pub async fn count_missing_covers(pool: &DbPool) -> Result<i64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM titles \
             WHERE deleted_at IS NULL \
               AND (cover_image_url IS NULL OR cover_image_url = '') \
               AND (isbn IS NOT NULL OR issn IS NOT NULL OR upc IS NOT NULL)",
        )
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Fix #214: list the titles eligible for a bulk cover-refetch.
    ///
    /// Returns the minimal projection the bulk task needs to call
    /// [`crate::tasks::metadata_fetch::fetch_metadata_chain`]: title
    /// id, the lookup code, its type, and the media type. ISBN is
    /// preferred when several codes are set (it has the broadest
    /// provider coverage); UPC second, ISSN last.
    ///
    /// Ordered by `created_at ASC` so the user sees the oldest
    /// cover-less titles fill in first.
    pub async fn list_missing_covers(
        pool: &DbPool,
    ) -> Result<Vec<MissingCoverTitle>, AppError> {
        let rows = sqlx::query(
            "SELECT id, isbn, issn, upc, media_type FROM titles \
             WHERE deleted_at IS NULL \
               AND (cover_image_url IS NULL OR cover_image_url = '') \
               AND (isbn IS NOT NULL OR issn IS NOT NULL OR upc IS NOT NULL) \
             ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;

        let titles = rows
            .into_iter()
            .filter_map(|r| {
                let id: u64 = r.try_get("id").ok()?;
                let isbn: Option<String> = r.try_get("isbn").ok().flatten();
                let issn: Option<String> = r.try_get("issn").ok().flatten();
                let upc: Option<String> = r.try_get("upc").ok().flatten();
                let media_type: String = r.try_get("media_type").ok()?;
                // Priority order: ISBN first (best provider coverage),
                // then UPC, then ISSN.
                let (code, code_type) = isbn
                    .map(|c| (c, crate::models::media_type::CodeType::Isbn))
                    .or_else(|| upc.map(|c| (c, crate::models::media_type::CodeType::Upc)))
                    .or_else(|| issn.map(|c| (c, crate::models::media_type::CodeType::Issn)))?;
                Some(MissingCoverTitle {
                    id,
                    code,
                    code_type,
                    media_type,
                })
            })
            .collect();
        Ok(titles)
    }

    /// #389 Palier 1 — count of active titles that carry any lookup code
    /// (ISBN / ISSN / UPC). Drives the "Backfill metadata from BnF" admin
    /// action, which re-fetches metadata for the whole coded-title set so
    /// the newly-added UNIMARC zones populate on already-cataloged titles.
    /// Unlike [`count_missing_covers`], there is NO `cover_image_url` filter
    /// — every coded title is a candidate regardless of cover state.
    pub async fn count_all_with_code(pool: &DbPool) -> Result<i64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM titles \
             WHERE deleted_at IS NULL \
               AND ((isbn IS NOT NULL AND isbn <> '') \
                 OR (issn IS NOT NULL AND issn <> '') \
                 OR (upc IS NOT NULL AND upc <> ''))",
        )
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// #389 Palier 1 — list every active title that carries a lookup code,
    /// for the bulk metadata backfill. Reuses the [`MissingCoverTitle`]
    /// projection and the same ISBN → UPC → ISSN priority as
    /// [`list_missing_covers`], but WITHOUT the `cover_image_url IS NULL`
    /// filter: the goal here is to re-run the metadata chain over the whole
    /// coded catalog so new UNIMARC zones backfill on existing titles.
    ///
    /// Ordered by `created_at ASC` so the oldest titles fill in first.
    pub async fn list_all_with_code(
        pool: &DbPool,
    ) -> Result<Vec<MissingCoverTitle>, AppError> {
        let rows = sqlx::query(
            "SELECT id, isbn, issn, upc, media_type FROM titles \
             WHERE deleted_at IS NULL \
               AND ((isbn IS NOT NULL AND isbn <> '') \
                 OR (issn IS NOT NULL AND issn <> '') \
                 OR (upc IS NOT NULL AND upc <> '')) \
             ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;

        let titles = rows
            .into_iter()
            .filter_map(|r| {
                let id: u64 = r.try_get("id").ok()?;
                let isbn: Option<String> = r.try_get("isbn").ok().flatten();
                let issn: Option<String> = r.try_get("issn").ok().flatten();
                let upc: Option<String> = r.try_get("upc").ok().flatten();
                let media_type: String = r.try_get("media_type").ok()?;
                // Priority order: ISBN first (best provider coverage),
                // then UPC, then ISSN. Empty strings are treated as absent
                // to match the SQL `<> ''` filter.
                let (code, code_type) = isbn
                    .filter(|c| !c.is_empty())
                    .map(|c| (c, crate::models::media_type::CodeType::Isbn))
                    .or_else(|| {
                        upc.filter(|c| !c.is_empty())
                            .map(|c| (c, crate::models::media_type::CodeType::Upc))
                    })
                    .or_else(|| {
                        issn.filter(|c| !c.is_empty())
                            .map(|c| (c, crate::models::media_type::CodeType::Issn))
                    })?;
                Some(MissingCoverTitle {
                    id,
                    code,
                    code_type,
                    media_type,
                })
            })
            .collect();
        Ok(titles)
    }

    /// Count of active (non-soft-deleted) titles in the catalog.
    /// Used by `services::dashboard::collection_glance` for the home-page
    /// "Collection at a glance" card and reusable by other dashboard surfaces.
    pub async fn count_active(pool: &DbPool) -> Result<i64, AppError> {
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM titles WHERE deleted_at IS NULL")
                .fetch_one(pool)
                .await?;
        Ok(row.0)
    }

    /// Story 9-7 — count active titles created in the last `days` days.
    /// Drives the "Recent cataloged" dashboard indicator.
    ///
    /// Boundary semantics: **inclusive** `>= NOW() - INTERVAL ? DAY` per
    /// FR wording "last 7 days" (the 7-day boundary IS counted). This
    /// is INTENTIONALLY ASYMMETRIC with `loan::count_overdue`'s strict
    /// `> threshold` — the spec's "exceeds N days" wording dictates the
    /// strict boundary there. Do NOT "harmonize" the boundary in a
    /// drive-by refactor; the asymmetry is locked by the
    /// `count_recent_cataloged_window_boundary` integration test.
    pub async fn count_recent_cataloged(
        pool: &DbPool,
        days: i32,
    ) -> Result<i64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM titles \
             WHERE deleted_at IS NULL AND created_at >= NOW() - INTERVAL ? DAY",
        )
        .bind(days)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Story 9-7 — list active titles created in the last `days` days,
    /// ordered by `created_at DESC` (newest first), bounded by `limit`.
    ///
    /// Mirrors `list_recent_active`'s projection (genre + primary
    /// contributor subquery + volume_count subquery) verbatim — the
    /// ONLY difference is the additional WHERE clause `AND
    /// t.created_at >= NOW() - INTERVAL ? DAY`. Returns
    /// `Vec<SearchResult>` (existing struct from story 9-2 — no new
    /// projection).
    pub async fn list_recent_cataloged(
        pool: &DbPool,
        days: i32,
        limit: u32,
    ) -> Result<Vec<SearchResult>, AppError> {
        let rows = sqlx::query(
            // Fix #107: LEFT JOIN + COALESCE so a title whose genre was
            // soft-deleted (orphan FK — story 8-4's deletion guard prevents
            // this via the supported path, but defense-in-depth against a
            // future migration / manual UPDATE) still appears in the result
            // set instead of silently disappearing. The empty `genre_name`
            // marker is replaced with a locale-aware placeholder at the
            // route layer via `routes::home::resolve_genre_placeholder`.
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
               AND t.created_at >= NOW() - INTERVAL ? DAY \
             ORDER BY t.created_at DESC, t.id DESC \
             LIMIT ?",
        )
        .bind(days)
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
                                   track_count, total_duration, age_rating, issue_number,
                                   statement_of_responsibility, edition_statement,
                                   collection_title, collection_number, general_note,
                                   original_title)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
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
        .bind(&new_title.statement_of_responsibility)
        .bind(&new_title.edition_statement)
        .bind(&new_title.collection_title)
        .bind(&new_title.collection_number)
        .bind(&new_title.general_note)
        .bind(&new_title.original_title)
        .execute(pool)
        .await?;

        let id = result.last_insert_id();
        TitleModel::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve created title".to_string()))
    }

    /// Backfill the six UNIMARC Palier 1 (#389) zones on an existing title.
    ///
    /// Best-effort, gap-filling update: each column is wrapped in
    /// `COALESCE(?, col)` so passing `None` for an argument leaves the
    /// existing value untouched (backfill only fills gaps — it never
    /// wipes data). Intentionally does NOT do optimistic-lock version
    /// checking; the BnF backfill path is best-effort and must not fail
    /// on a concurrent edit.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_unimarc_zones(
        pool: &DbPool,
        id: u64,
        statement_of_responsibility: Option<&str>,
        edition_statement: Option<&str>,
        collection_title: Option<&str>,
        collection_number: Option<&str>,
        general_note: Option<&str>,
        original_title: Option<&str>,
    ) -> Result<(), AppError> {
        tracing::debug!(id = id, "Backfilling UNIMARC zones on title");

        sqlx::query(
            "UPDATE titles SET \
             statement_of_responsibility = COALESCE(?, statement_of_responsibility), \
             edition_statement = COALESCE(?, edition_statement), \
             collection_title = COALESCE(?, collection_title), \
             collection_number = COALESCE(?, collection_number), \
             general_note = COALESCE(?, general_note), \
             original_title = COALESCE(?, original_title), \
             version = version + 1, updated_at = NOW() \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(statement_of_responsibility)
        .bind(edition_statement)
        .bind(collection_title)
        .bind(collection_number)
        .bind(general_note)
        .bind(original_title)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
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

    /// CR #272 — extension of [`Self::update_metadata`] that ALSO updates
    /// the three identifier columns (`isbn`, `issn`, `upc`). Used by the
    /// metadata editing form when the user corrects a wrong-scan ISBN.
    /// The re-download / conflict-confirm flows keep calling
    /// [`Self::update_metadata`] because the provider response always
    /// carries back the same identifier that was looked up.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_full(
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
        isbn: Option<&str>,
        issn: Option<&str>,
        upc: Option<&str>,
        media_type: &str,
    ) -> Result<TitleModel, AppError> {
        let result = sqlx::query(
            "UPDATE titles SET title = ?, subtitle = ?, description = ?, \
             publisher = ?, language = ?, genre_id = ?, \
             publication_date = ?, dewey_code = ?, \
             page_count = ?, track_count = ?, total_duration = ?, \
             age_rating = ?, issue_number = ?, \
             manually_edited_fields = ?, \
             isbn = ?, issn = ?, upc = ?, \
             media_type = ?, \
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
        .bind(isbn)
        .bind(issn)
        .bind(upc)
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
    ///
    /// Returns an empty Vec when the column is NULL. If the JSON is present but
    /// malformed (not an array of strings), logs a warning and returns empty —
    /// a fail-open that weakens the metadata-fetch guard for a single corrupt
    /// row, but surfaces the corruption in logs instead of silently masking it.
    pub fn parsed_manually_edited_fields(&self) -> Vec<String> {
        let Some(raw) = self.manually_edited_fields.as_deref() else {
            return Vec::new();
        };
        match serde_json::from_str::<Vec<String>>(raw) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    title_id = self.id,
                    error = %e,
                    raw = %raw,
                    "manually_edited_fields is not a valid JSON array of strings; treating as empty"
                );
                Vec::new()
            }
        }
    }

    /// Story 11-18 — the `manually_edited_fields` JSON once `fields` are
    /// added to the current set (sorted, duplicates dropped), so a later
    /// metadata re-download leaves them alone (FR144, #232).
    pub fn manually_edited_with(&self, fields: &[&str]) -> String {
        let mut set: std::collections::BTreeSet<String> =
            self.parsed_manually_edited_fields().into_iter().collect();
        set.extend(fields.iter().map(|f| (*f).to_string()));
        serde_json::to_string(&set.into_iter().collect::<Vec<_>>()).unwrap_or_else(|_| "[]".into())
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
                    )));
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
pub fn detect_edited_fields(
    old: &TitleModel,
    new_title: &str,
    new_subtitle: Option<&str>,
    new_description: Option<&str>,
    new_publisher: Option<&str>,
    new_language: &str,
    new_publication_date: Option<chrono::NaiveDate>,
    new_dewey_code: Option<&str>,
    new_page_count: Option<i32>,
    new_track_count: Option<i32>,
    new_total_duration: Option<i32>,
    new_age_rating: Option<&str>,
    new_issue_number: Option<i32>,
) -> Vec<String> {
    let mut changed = Vec::new();

    if old.title != new_title {
        changed.push("title".to_string());
    }
    if old.subtitle.as_deref() != new_subtitle {
        changed.push("subtitle".to_string());
    }
    if old.description.as_deref() != new_description {
        changed.push("description".to_string());
    }
    if old.publisher.as_deref() != new_publisher {
        changed.push("publisher".to_string());
    }
    if old.language != new_language {
        changed.push("language".to_string());
    }
    // NOTE: `genre_id` is intentionally NOT tracked here. It is a
    // mybibli-only classification, no provider in the metadata chain
    // ever returns it, so the conflict-confirm flow has nothing to
    // protect. Tracking it used to send every re-fetch through the
    // confirmation form and silently swallow the Open Library cover
    // fallback (fix #232).
    if old.publication_date != new_publication_date {
        changed.push("publication_date".to_string());
    }
    if old.dewey_code.as_deref() != new_dewey_code {
        changed.push("dewey_code".to_string());
    }
    if old.page_count != new_page_count {
        changed.push("page_count".to_string());
    }
    if old.track_count != new_track_count {
        changed.push("track_count".to_string());
    }
    if old.total_duration != new_total_duration {
        changed.push("total_duration".to_string());
    }
    if old.age_rating.as_deref() != new_age_rating {
        changed.push("age_rating".to_string());
    }
    if old.issue_number != new_issue_number {
        changed.push("issue_number".to_string());
    }

    changed
}


#[cfg(test)]
mod tests {
    use super::*;

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
            statement_of_responsibility: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            general_note: None,
            original_title: None,
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
            statement_of_responsibility: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            general_note: None,
            original_title: None,
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
            statement_of_responsibility: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            general_note: None,
            original_title: None,
            version: 1,
        }
    }

    #[test]
    fn test_detect_edited_fields_no_changes() {
        let old = make_test_title();
        let changed = detect_edited_fields(
            &old,
            "Original Title",
            Some("Original Sub"),
            None,
            Some("Gallimard"),
            "fr",
            None,
            None,
            Some(186),
            None,
            None,
            None,
            None,
        );
        assert!(changed.is_empty());
    }

    #[test]
    fn test_detect_edited_fields_publisher_changed() {
        let old = make_test_title();
        let changed = detect_edited_fields(
            &old,
            "Original Title",
            Some("Original Sub"),
            None,
            Some("Flammarion"),
            "fr",
            None,
            None,
            Some(186),
            None,
            None,
            None,
            None,
        );
        assert_eq!(changed, vec!["publisher"]);
    }

    #[test]
    fn test_detect_edited_fields_multiple_changes() {
        let old = make_test_title();
        let changed = detect_edited_fields(
            &old,
            "New Title",
            Some("Original Sub"),
            Some("A description"),
            Some("Gallimard"),
            "en",
            None,
            None,
            Some(186),
            None,
            None,
            None,
            None,
        );
        assert!(changed.contains(&"title".to_string()));
        assert!(changed.contains(&"description".to_string()));
        assert!(changed.contains(&"language".to_string()));
        assert_eq!(changed.len(), 3);
    }

    /// Regression guard for fix #232.
    ///
    /// The signature deliberately omits `new_genre_id` so a future
    /// refactor cannot quietly reintroduce `"genre_id"` tracking. If
    /// someone ever puts `new_genre_id: u64` back into the parameter
    /// list and pushes it onto `changed`, this test stops compiling
    /// because the call won't match the signature any more. Belt-and-
    /// braces against the conflict-confirm-flow regression that
    /// silently swallowed Open Library cover fallbacks for every title
    /// whose genre had ever been edited.
    #[test]
    fn test_detect_edited_fields_does_not_track_genre() {
        let old = make_test_title();
        let changed = detect_edited_fields(
            &old,
            "Original Title",
            Some("Original Sub"),
            None,
            Some("Gallimard"),
            "fr",
            None,
            None,
            Some(186),
            None,
            None,
            None,
            None,
        );
        // The model never produces "genre_id" in the changed list — the
        // function doesn't even take a genre input. The empty result
        // here is the explicit assertion that nothing genre-related
        // leaks through.
        assert!(
            !changed.iter().any(|f| f == "genre_id"),
            "genre_id must never appear in manually_edited_fields"
        );
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

    /// Story 9-9 review fix (AC11b + Foundation Rule #2) — DB-backed unit
    /// tests co-located with `find_id_by_isbn`. Pattern mirrors the
    /// `#[sqlx::test]` precedent in `src/middleware/locale.rs` /
    /// `src/services/admin_health.rs`. The integration tests in
    /// `tests/home_scan_redirect.rs` exercise these methods end-to-end via
    /// the handler; these tests lock the model contract narrowly so a
    /// regression in the SQL is caught at the model layer first.
    #[sqlx::test(migrations = "./migrations")]
    async fn find_id_by_isbn_returns_some_for_active_match(pool: sqlx::MySqlPool) {
        let g: u64 = sqlx::query_scalar(
            "SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let r = sqlx::query(
            "INSERT INTO titles (title, isbn, language, media_type, genre_id) \
             VALUES ('Test', '9782070360246', 'fr', 'book', ?)",
        )
        .bind(g)
        .execute(&pool)
        .await
        .unwrap();
        let expected_id = r.last_insert_id();

        let got = TitleModel::find_id_by_isbn(&pool, "9782070360246")
            .await
            .unwrap();
        assert_eq!(got, Some(expected_id));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_id_by_isbn_returns_none_for_soft_deleted(pool: sqlx::MySqlPool) {
        let g: u64 = sqlx::query_scalar(
            "SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let r = sqlx::query(
            "INSERT INTO titles (title, isbn, language, media_type, genre_id) \
             VALUES ('Soft-deleted', '9782070360246', 'fr', 'book', ?)",
        )
        .bind(g)
        .execute(&pool)
        .await
        .unwrap();
        let id = r.last_insert_id();
        sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();

        let got = TitleModel::find_id_by_isbn(&pool, "9782070360246")
            .await
            .unwrap();
        assert_eq!(got, None, "soft-deleted titles MUST NOT match");
    }

    // ─── #202 — metadata_source provenance ─────────────────────────

    /// Helper: insert a title with an optional recorded source, return its id.
    #[cfg(test)]
    async fn insert_title_with_source(pool: &DbPool, source: Option<&str>) -> u64 {
        let g: u64 = sqlx::query_scalar(
            "SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let r = sqlx::query(
            "INSERT INTO titles (title, language, media_type, genre_id, metadata_source) \
             VALUES ('Provenance probe', 'fr', 'book', ?, ?)",
        )
        .bind(g)
        .bind(source)
        .execute(pool)
        .await
        .unwrap();
        r.last_insert_id()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_metadata_source_round_trips_the_stored_provider(pool: sqlx::MySqlPool) {
        let id = insert_title_with_source(&pool, Some("google_books")).await;
        let got = TitleModel::find_metadata_source(&pool, id).await.unwrap();
        assert_eq!(got, Some("google_books".to_string()));
    }

    /// A pre-#202 row carries NULL and must read as "no provenance", which is
    /// what suppresses the badge rather than rendering an empty one.
    #[sqlx::test(migrations = "./migrations")]
    async fn find_metadata_source_returns_none_when_never_recorded(pool: sqlx::MySqlPool) {
        let id = insert_title_with_source(&pool, None).await;
        let got = TitleModel::find_metadata_source(&pool, id).await.unwrap();
        assert_eq!(got, None);
    }

    /// An empty string is not a provider. It cannot be written by the current
    /// code path, but a hand-edited row or a future import could produce one,
    /// and `Some("")` would render a badge reading just "Source:".
    #[sqlx::test(migrations = "./migrations")]
    async fn find_metadata_source_treats_empty_string_as_absent(pool: sqlx::MySqlPool) {
        let id = insert_title_with_source(&pool, Some("")).await;
        let got = TitleModel::find_metadata_source(&pool, id).await.unwrap();
        assert_eq!(got, None, "an empty source must not render a badge");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_metadata_source_returns_none_for_soft_deleted(pool: sqlx::MySqlPool) {
        let id = insert_title_with_source(&pool, Some("BnF")).await;
        sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        let got = TitleModel::find_metadata_source(&pool, id).await.unwrap();
        assert_eq!(got, None, "soft-deleted titles MUST NOT report a source");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_metadata_source_returns_none_for_nonexistent(pool: sqlx::MySqlPool) {
        let got = TitleModel::find_metadata_source(&pool, 9_999_999)
            .await
            .unwrap();
        assert_eq!(got, None);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_id_by_isbn_returns_none_for_nonexistent(pool: sqlx::MySqlPool) {
        let got = TitleModel::find_id_by_isbn(&pool, "9780000000002")
            .await
            .unwrap();
        assert_eq!(got, None);
    }
}
