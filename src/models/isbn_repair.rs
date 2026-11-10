//! Story 17-3 (#565) — the reads and writes of the boot repair of stored
//! ISBNs (`services::isbn_repair`).

use sqlx::MySqlConnection;

use crate::error::AppError;

/// A live title's stored ISBN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredIsbn {
    pub id: u64,
    pub version: i32,
    pub isbn: String,
}

/// Every live title with a non-blank ISBN, by id. A plain, non-locking
/// read: each write is then guarded by its row's `version` and old value.
pub async fn list_live_isbns(conn: &mut MySqlConnection) -> Result<Vec<StoredIsbn>, AppError> {
    let rows: Vec<(u64, i32, String)> = sqlx::query_as(
        "SELECT id, version, isbn FROM titles \
         WHERE deleted_at IS NULL AND isbn IS NOT NULL AND isbn <> '' \
         ORDER BY id ASC",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, version, isbn)| StoredIsbn { id, version, isbn })
        .collect())
}

/// Store `new` (the ISBN-13 of `old`), guarded by `version` and the old
/// value; `false` when the row changed meanwhile. A **non-edit write**
/// (#521, story 13-4): the same identifier re-encoded, so neither `version`
/// nor `updated_at` moves and `manually_edited_fields` is left alone.
pub async fn set_canonical_isbn(
    conn: &mut MySqlConnection,
    id: u64,
    version: i32,
    old: &str,
    new: &str,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE titles SET isbn = ?, updated_at = updated_at \
         WHERE id = ? AND version = ? AND isbn = ? AND deleted_at IS NULL",
    )
    .bind(new)
    .bind(id)
    .bind(version)
    .bind(old)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}
