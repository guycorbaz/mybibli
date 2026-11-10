//! Story 17-3 (#565) — the reads and writes of the boot repair of stored
//! ISBNs (`services::isbn_repair`).

use sqlx::MySqlConnection;

use crate::error::AppError;

/// A title's stored ISBN; `deleted` when the title is in the Trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredIsbn {
    pub id: u64,
    pub version: i32,
    pub isbn: String,
    pub deleted: bool,
}

/// Every title with a non-blank ISBN, by id — **trashed titles included on
/// purpose** (coordinator decision, 2026-11-10): a title restored from the
/// Trash must not bring back the old form, and the collision check must see
/// the trashed holders. A plain, non-locking read: each write is then
/// guarded by its row's `version` and old value.
pub async fn list_isbns(conn: &mut MySqlConnection) -> Result<Vec<StoredIsbn>, AppError> {
    let rows: Vec<(u64, i32, String, i64)> = sqlx::query_as(
        "SELECT id, version, isbn, CAST(deleted_at IS NOT NULL AS SIGNED) FROM titles \
         WHERE isbn IS NOT NULL AND isbn <> '' \
         ORDER BY id ASC",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, version, isbn, deleted)| StoredIsbn { id, version, isbn, deleted: deleted != 0 })
        .collect())
}

/// Store `new` (the ISBN-13 of `old`), guarded by `version` and the old
/// value; `false` when the row changed meanwhile. A **non-edit write**
/// (#521, story 13-4): the same identifier re-encoded, so neither `version`
/// nor `updated_at` moves and `manually_edited_fields` is left alone. A
/// trashed title is rewritten too (see [`list_isbns`]).
pub async fn set_canonical_isbn(
    conn: &mut MySqlConnection,
    id: u64,
    version: i32,
    old: &str,
    new: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE titles SET isbn = ?, updated_at = updated_at \
         WHERE id = ? AND version = ? AND isbn = ?",
    )
    .bind(new)
    .bind(id)
    .bind(version)
    .bind(old)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}
