//! CR #242 — wishlist items.
//!
//! Books the user wants to buy, kept separate from the owned catalog
//! `titles` table. Tolerates partial metadata: `title` is the only
//! required field, everything else can be NULL. The `isbn` column is
//! indexed so the catalog-scan auto-link can resolve a match in O(1).

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WishlistItem {
    pub id: u64,
    pub isbn: Option<String>,
    pub title: String,
    pub authors: Option<String>,
    pub publisher: Option<String>,
    pub publication_year: Option<u16>,
    pub cover_image_url: Option<String>,
    pub notes: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub version: i32,
}

/// Input fields for creating a wishlist item. Two valid shapes:
/// - free-form: just `title` (+ optional authors/publisher/notes)
/// - by-ISBN: title + isbn + everything the provider chain resolved
pub struct WishlistItemDraft<'a> {
    pub isbn: Option<&'a str>,
    pub title: &'a str,
    pub authors: Option<&'a str>,
    pub publisher: Option<&'a str>,
    pub publication_year: Option<u16>,
    pub cover_image_url: Option<&'a str>,
    pub notes: Option<&'a str>,
}

pub struct WishlistItemModel;

impl WishlistItemModel {
    /// Insert a new wishlist item. Returns the inserted row's id.
    pub async fn create(pool: &DbPool, draft: &WishlistItemDraft<'_>) -> Result<u64, AppError> {
        let id = sqlx::query(
            "INSERT INTO wishlist_items \
                (isbn, title, authors, publisher, publication_year, cover_image_url, notes) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(draft.isbn)
        .bind(draft.title)
        .bind(draft.authors)
        .bind(draft.publisher)
        .bind(draft.publication_year)
        .bind(draft.cover_image_url)
        .bind(draft.notes)
        .execute(pool)
        .await?
        .last_insert_id();
        Ok(id)
    }

    /// Look up an active wishlist item by id. Returns `None` when the row
    /// is missing OR soft-deleted (`deleted_at IS NOT NULL`).
    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<WishlistItem>, AppError> {
        let row = sqlx::query(
            "SELECT id, isbn, title, authors, publisher, \
                    CAST(publication_year AS UNSIGNED) AS publication_year, \
                    cover_image_url, notes, created_at, version \
             FROM wishlist_items \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(map_row))
    }

    /// Look up an active wishlist item by ISBN. Used by the catalog-scan
    /// auto-link path. Returns `None` if no active row matches; if more
    /// than one row carries the same ISBN (the user added the same book
    /// twice from different sources), the oldest is returned for
    /// deterministic test behavior.
    pub async fn find_by_isbn(
        pool: &DbPool,
        isbn: &str,
    ) -> Result<Option<WishlistItem>, AppError> {
        let row = sqlx::query(
            "SELECT id, isbn, title, authors, publisher, \
                    CAST(publication_year AS UNSIGNED) AS publication_year, \
                    cover_image_url, notes, created_at, version \
             FROM wishlist_items \
             WHERE isbn = ? AND deleted_at IS NULL \
             ORDER BY created_at ASC, id ASC \
             LIMIT 1",
        )
        .bind(isbn)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(map_row))
    }

    /// List every active (non-soft-deleted) wishlist item, newest-first.
    /// v1 has no pagination — the list size is bounded by user behavior
    /// (a household wishlist rarely exceeds ~100 entries). Promote to a
    /// `PaginatedList<>` shape if that assumption breaks.
    pub async fn list_active(pool: &DbPool) -> Result<Vec<WishlistItem>, AppError> {
        let rows = sqlx::query(
            "SELECT id, isbn, title, authors, publisher, \
                    CAST(publication_year AS UNSIGNED) AS publication_year, \
                    cover_image_url, notes, created_at, version \
             FROM wishlist_items \
             WHERE deleted_at IS NULL \
             ORDER BY created_at DESC, id DESC",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(map_row).collect())
    }

    /// Count active wishlist items. Drives the home-dashboard counter
    /// (UX-DR4 zero-count rule: tag hidden when 0).
    pub async fn count_active(pool: &DbPool) -> Result<i64, AppError> {
        let (n,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM wishlist_items WHERE deleted_at IS NULL")
                .fetch_one(pool)
                .await?;
        Ok(n)
    }

    /// Soft-delete a wishlist item with optimistic-locking on `version`.
    /// Returns the number of affected rows (0 = row missing OR version
    /// mismatch). Used by both "Mark as bought" and "Remove from wishlist"
    /// flows; the audit-trail entry is emitted by the route layer.
    pub async fn soft_delete(pool: &DbPool, id: u64, version: i32) -> Result<u64, AppError> {
        let result = sqlx::query(
            "UPDATE wishlist_items \
             SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }
}

fn map_row(r: sqlx::mysql::MySqlRow) -> WishlistItem {
    WishlistItem {
        id: r.try_get("id").unwrap_or(0),
        isbn: r.try_get("isbn").ok(),
        title: r.try_get("title").unwrap_or_default(),
        authors: r.try_get("authors").ok(),
        publisher: r.try_get("publisher").ok(),
        publication_year: r
            .try_get::<Option<i64>, _>("publication_year")
            .ok()
            .flatten()
            .map(|n| n as u16),
        cover_image_url: r.try_get("cover_image_url").ok(),
        notes: r.try_get("notes").ok(),
        created_at: r
            .try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")
            .unwrap_or_else(|_| chrono::Utc::now()),
        version: r.try_get("version").unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::MySqlPool;

    fn draft<'a>(title: &'a str, isbn: Option<&'a str>) -> WishlistItemDraft<'a> {
        WishlistItemDraft {
            isbn,
            title,
            authors: None,
            publisher: None,
            publication_year: None,
            cover_image_url: None,
            notes: None,
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn create_then_find_by_id_round_trips_required_fields(pool: MySqlPool) {
        let id = WishlistItemModel::create(&pool, &draft("Le Comte de Monte-Cristo", None))
            .await
            .unwrap();
        let got = WishlistItemModel::find_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.title, "Le Comte de Monte-Cristo");
        assert_eq!(got.isbn, None);
        assert_eq!(got.version, 0);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_by_id_returns_none_for_soft_deleted(pool: MySqlPool) {
        let id = WishlistItemModel::create(&pool, &draft("Soft-deleted", None))
            .await
            .unwrap();
        let n = WishlistItemModel::soft_delete(&pool, id, 0).await.unwrap();
        assert_eq!(n, 1);
        let got = WishlistItemModel::find_by_id(&pool, id).await.unwrap();
        assert!(got.is_none());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_by_isbn_returns_active_match(pool: MySqlPool) {
        let id = WishlistItemModel::create(&pool, &draft("Foundation", Some("9780553293357")))
            .await
            .unwrap();
        let got = WishlistItemModel::find_by_isbn(&pool, "9780553293357")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.id, id);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_by_isbn_skips_soft_deleted(pool: MySqlPool) {
        let id = WishlistItemModel::create(&pool, &draft("Foundation", Some("9780553293357")))
            .await
            .unwrap();
        WishlistItemModel::soft_delete(&pool, id, 0).await.unwrap();
        let got = WishlistItemModel::find_by_isbn(&pool, "9780553293357")
            .await
            .unwrap();
        assert!(got.is_none(), "soft-deleted rows MUST NOT match");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn list_active_orders_newest_first(pool: MySqlPool) {
        let a = WishlistItemModel::create(&pool, &draft("A", None)).await.unwrap();
        let b = WishlistItemModel::create(&pool, &draft("B", None)).await.unwrap();
        let c = WishlistItemModel::create(&pool, &draft("C", None)).await.unwrap();
        let got = WishlistItemModel::list_active(&pool).await.unwrap();
        assert_eq!(got.len(), 3);
        // Newest-first by created_at DESC, id DESC tiebreak.
        assert_eq!(got[0].id, c);
        assert_eq!(got[1].id, b);
        assert_eq!(got[2].id, a);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn list_active_excludes_soft_deleted(pool: MySqlPool) {
        let id = WishlistItemModel::create(&pool, &draft("Soft-deleted", None))
            .await
            .unwrap();
        WishlistItemModel::soft_delete(&pool, id, 0).await.unwrap();
        let got = WishlistItemModel::list_active(&pool).await.unwrap();
        assert!(got.is_empty());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn count_active_matches_list_active_len(pool: MySqlPool) {
        for t in ["A", "B", "C"] {
            WishlistItemModel::create(&pool, &draft(t, None)).await.unwrap();
        }
        let n = WishlistItemModel::count_active(&pool).await.unwrap();
        assert_eq!(n, 3);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn soft_delete_with_wrong_version_returns_zero(pool: MySqlPool) {
        let id = WishlistItemModel::create(&pool, &draft("Optimistic", None))
            .await
            .unwrap();
        let n = WishlistItemModel::soft_delete(&pool, id, 999).await.unwrap();
        assert_eq!(n, 0, "stale version MUST not soft-delete");
        // Row is still active.
        let got = WishlistItemModel::find_by_id(&pool, id).await.unwrap();
        assert!(got.is_some());
    }
}
