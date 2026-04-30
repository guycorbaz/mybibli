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
