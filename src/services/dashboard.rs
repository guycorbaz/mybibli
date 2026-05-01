//! Dashboard data builders for the home page (story 9-1 onward).
//!
//! Keep business logic OUT of the route handler — these functions are small,
//! typed, and independently testable. Each function corresponds to one widget
//! on `/`. Story 9-1 ships the "Collection at a glance" card; subsequent Epic
//! 9 stories will add recent additions (9-2), stats by genre (9-3), and the
//! actionable indicators with FilterTag (9-4..9-7) on top of this module.

use crate::db::DbPool;
use crate::error::AppError;

/// Three-count summary shown by the home-page "Collection at a glance" card.
///
/// Counts exclude soft-deleted rows. `active_loans` additionally excludes
/// returned loans (`returned_at IS NULL`). All three fields are `i64`
/// because that's what MariaDB returns for `COUNT(*)`; the template only
/// needs to display them, no arithmetic downstream.
#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct CollectionGlance {
    pub titles: i64,
    pub volumes: i64,
    pub active_loans: i64,
}

/// Compute the three glance counts in a single SQL round-trip.
///
/// The query uses three correlated subqueries inside one SELECT — one
/// network round-trip, three independent COUNT(*) on indexed columns
/// (`deleted_at`, `returned_at`). Story 9-1 AC2 mandates this single-
/// round-trip shape; verified by inspection of the SQL below at code-
/// review time.
///
/// `active_loans` JOINs `volumes` and `borrowers` with `deleted_at IS NULL`
/// filters so an orphan loan whose volume or borrower has been soft-deleted
/// is NOT counted — matching `LoanModel::list_active` semantics so the
/// home-page count never diverges from what the user sees on `/loans`.
pub async fn collection_glance(pool: &DbPool) -> Result<CollectionGlance, AppError> {
    let glance: CollectionGlance = sqlx::query_as(
        "SELECT \
            (SELECT COUNT(*) FROM titles WHERE deleted_at IS NULL)  AS titles, \
            (SELECT COUNT(*) FROM volumes WHERE deleted_at IS NULL) AS volumes, \
            (SELECT COUNT(*) FROM loans l \
               JOIN volumes v ON l.volume_id = v.id AND v.deleted_at IS NULL \
               JOIN borrowers b ON l.borrower_id = b.id AND b.deleted_at IS NULL \
              WHERE l.returned_at IS NULL AND l.deleted_at IS NULL) AS active_loans",
    )
    .fetch_one(pool)
    .await?;
    Ok(glance)
}

/// One row of the home-page "By genre" section (story 9-3).
///
/// Carries only the SQL-emitted fields (id, name, count). The percentage
/// and locale-formatted labels are computed in the route handler — keeping
/// the model layer free of presentation concerns means a future caller
/// (e.g., an admin stats panel) can reuse the raw counts without ripping
/// out FR/EN formatting.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GenreStat {
    pub id: u64,
    pub name: String,
    pub title_count: i64,
}

/// Aggregate active title counts per genre, sorted for display.
///
/// Single SQL round-trip with `INNER JOIN`: a genre with zero active
/// titles is naturally excluded (no `LEFT JOIN`-induced 0-count rows),
/// so AC4's "no genres assigned → section hidden" falls out of the query
/// shape — an empty DB returns `Vec::new()`. Both halves of the soft-
/// delete invariant (AC5) are load-bearing: `t.deleted_at IS NULL`
/// excludes trashed titles, `g.deleted_at IS NULL` keeps an orphan FK
/// (titles still pointing at a soft-deleted genre) from leaking.
///
/// Ordering: `title_count DESC` puts the biggest genres on top
/// (presentation goal); the secondary `g.name ASC` tiebreak keeps the
/// row order deterministic across runs (and across deployments where
/// MariaDB's default ordering of equal `COUNT(*)` would otherwise be
/// implementation-defined).
pub async fn stats_by_genre(pool: &DbPool) -> Result<Vec<GenreStat>, AppError> {
    let rows: Vec<GenreStat> = sqlx::query_as(
        "SELECT g.id, g.name, COUNT(t.id) AS title_count \
           FROM titles t \
           JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL \
          WHERE t.deleted_at IS NULL \
          GROUP BY g.id, g.name \
          ORDER BY title_count DESC, g.name ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_glance_default_is_zeros() {
        let g = CollectionGlance::default();
        assert_eq!(g.titles, 0);
        assert_eq!(g.volumes, 0);
        assert_eq!(g.active_loans, 0);
    }
}
