//! Story 13-8 (#580) — the SQL behind "move a volume to another title":
//! the title picker of the move modal, the locked reads + optimistic
//! UPDATE of the move itself, and the locked reads of the title delete
//! that must not race a move. The decisions live in
//! `services::volume_move` and `TitleService::delete_without_volumes`.

use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

/// Most candidates the picker lists (AC1: "at most ~10 results").
pub const MOVE_CANDIDATE_LIMIT: u32 = 10;

/// A title the volume can be moved to, as the picker shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveCandidate {
    pub id: u64,
    pub title: String,
    pub primary_contributor: Option<String>,
    pub isbn: Option<String>,
}

/// The volume row as the move reads it, under its row lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedVolume {
    pub title_id: u64,
    pub label: String,
    pub version: i32,
}

/// The columns of a [`MoveCandidate`] from the live title aliased `t`.
/// The primary contributor follows the title lists' rule (an "Auteur"
/// first, else the oldest live credit).
const CANDIDATE_COLUMNS: &str = "t.id, t.title, t.isbn, \
    (SELECT c.name FROM title_contributors tc \
       JOIN contributors c ON tc.contributor_id = c.id \
       JOIN contributor_roles cr ON tc.role_id = cr.id \
      WHERE tc.title_id = t.id AND tc.deleted_at IS NULL \
        AND c.deleted_at IS NULL AND cr.deleted_at IS NULL \
      ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC \
      LIMIT 1) AS primary_contributor";

fn candidate_from_row(r: &sqlx::mysql::MySqlRow) -> Result<MoveCandidate, AppError> {
    Ok(MoveCandidate {
        id: r.try_get("id")?,
        title: r.try_get("title")?,
        primary_contributor: r.try_get("primary_contributor")?,
        isbn: r.try_get("isbn")?,
    })
}

/// Live titles matching `query` by title text or ISBN, `exclude_title_id`
/// (the volume's current title) left out. An exact ISBN match comes
/// first, then alphabetical order. An empty query lists nothing.
pub async fn search_candidates(
    pool: &DbPool,
    query: &str,
    exclude_title_id: u64,
) -> Result<Vec<MoveCandidate>, AppError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let compact = crate::utils::strip_isbn_separators(trimmed);
    // Story 17-3 (#565): the one ISBN rule's form also matches exactly (an
    // ISBN-10 or a pasted `ISBN` prefix lists its title, first).
    let exact = crate::utils::isbn_lookup_form(trimmed);
    let sql = format!(
        "SELECT {CANDIDATE_COLUMNS} FROM titles t \
         WHERE t.deleted_at IS NULL AND t.id <> ? \
           AND (t.title LIKE ? OR t.isbn LIKE ? OR t.isbn = ?) \
         ORDER BY CASE WHEN t.isbn = ? THEN 0 ELSE 1 END, t.title ASC, t.id ASC \
         LIMIT {MOVE_CANDIDATE_LIMIT}"
    );
    let rows = sqlx::query(&sql)
        .bind(exclude_title_id)
        .bind(crate::utils::like_contains_pattern(trimmed))
        .bind(crate::utils::like_contains_pattern(&compact))
        .bind(&exact)
        .bind(&exact)
        .fetch_all(pool)
        .await?;
    rows.iter().map(candidate_from_row).collect()
}

/// One live title as a [`MoveCandidate`] (the picked target), `None`
/// when it does not exist or is deleted.
pub async fn find_candidate(
    pool: &DbPool,
    title_id: u64,
) -> Result<Option<MoveCandidate>, AppError> {
    let sql =
        format!("SELECT {CANDIDATE_COLUMNS} FROM titles t WHERE t.id = ? AND t.deleted_at IS NULL");
    let row = sqlx::query(&sql)
        .bind(title_id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(candidate_from_row).transpose()
}

/// The live volume `id` under a row lock (`FOR UPDATE`), `None` when it
/// does not exist or is deleted.
pub async fn lock_volume_in(
    conn: &mut sqlx::MySqlConnection,
    id: u64,
) -> Result<Option<LockedVolume>, AppError> {
    let row = sqlx::query(
        "SELECT title_id, label, version FROM volumes \
         WHERE id = ? AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    row.map(|r| {
        Ok(LockedVolume {
            title_id: r.try_get("title_id")?,
            label: r.try_get("label")?,
            version: r.try_get("version")?,
        })
    })
    .transpose()
}

/// Whether title `id` is live, read under a shared lock so the title
/// cannot be deleted before the move commits.
pub async fn title_is_live_in(conn: &mut sqlx::MySqlConnection, id: u64) -> Result<bool, AppError> {
    let row: Option<(u64,)> = sqlx::query_as(
        "SELECT id FROM titles WHERE id = ? AND deleted_at IS NULL LOCK IN SHARE MODE",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.is_some())
}

/// Attach volume `id` (at `version`) to `title_id`; bumps `version`.
/// Returns the number of rows changed (0 = stale version or deleted).
pub async fn set_title_in(
    conn: &mut sqlx::MySqlConnection,
    id: u64,
    version: i32,
    title_id: u64,
) -> Result<u64, AppError> {
    let done = sqlx::query(
        "UPDATE volumes SET title_id = ?, version = version + 1, updated_at = NOW() \
         WHERE id = ? AND version = ? AND deleted_at IS NULL",
    )
    .bind(title_id)
    .bind(id)
    .bind(version)
    .execute(&mut *conn)
    .await?;
    Ok(done.rows_affected())
}

/// Title `id` locked for a delete (`FOR UPDATE`), `true` when it is live.
/// A move share-locks its target ([`title_is_live_in`]), so the delete
/// waits for an in-flight move to commit — and a move arriving after the
/// lock waits for the delete, then sees the title gone.
pub async fn lock_title_for_delete_in(
    conn: &mut sqlx::MySqlConnection,
    id: u64,
) -> Result<bool, AppError> {
    let row: Option<(u64,)> =
        sqlx::query_as("SELECT id FROM titles WHERE id = ? AND deleted_at IS NULL FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(row.is_some())
}

/// Live volumes of title `id` (a plain read: taken after
/// [`lock_title_for_delete_in`], its snapshot already includes any move
/// committed while the delete waited).
pub async fn count_live_volumes_in(
    conn: &mut sqlx::MySqlConnection,
    title_id: u64,
) -> Result<i64, AppError> {
    let (n,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM volumes WHERE title_id = ? AND deleted_at IS NULL")
            .bind(title_id)
            .fetch_one(&mut *conn)
            .await?;
    Ok(n)
}
