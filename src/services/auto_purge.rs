use std::collections::HashMap;

use serde_json::json;
use crate::db::DbPool;
use crate::error::AppError;
use crate::models::admin_audit::AdminAuditModel;

/// Maximum number of LIMIT-bounded DELETE batches to issue per table during a
/// single purge run. Each batch deletes up to 10 000 rows; with the cap at 100
/// a single family can drain up to 1 000 000 rows per run before the loop
/// breaks defensively. The cap exists to bound worst-case runtime if the
/// DELETE keeps returning a full batch (e.g., concurrent inserts of
/// soft-deleted rows). Stale rows beyond the cap will be picked up by the
/// next scheduled run.
const MAX_DRAIN_ITERATIONS: usize = 100;
/// Per-batch DELETE LIMIT — keep small enough that the implicit row-lock
/// window doesn't block concurrent writers for too long.
const DELETE_BATCH_SIZE: u64 = 10_000;

/// FK-safe deletion order for `run_purge` — children before their parents.
///
/// Distinct from `services::soft_delete::ALLOWED_TABLES` (which guards the
/// soft-delete API surface against user-supplied table names): the soft-delete
/// whitelist only contains entity-parent tables (the rows admins can soft-delete
/// from the UI), whereas auto-purge must also visit junction-table children so
/// the FK-dependent rows are gone before the parent DELETE runs (issue #60).
///
/// Every entry MUST exist in the schema and carry a `deleted_at` column —
/// `validate_schema` cross-checks this at startup.
pub(crate) const PURGE_DELETION_ORDER: &[&str] = &[
    "title_contributors", // FK → titles, contributors, contributor_roles
    "title_series",       // FK → titles, series
    "loans",              // FK → volumes, borrowers, storage_locations
    "volumes",            // FK → titles, volume_states, storage_locations
    "titles",             // FK → genres
    "series",
    "borrowers",
    "storage_locations", // self-FK (hierarchical)
    "contributors",
    "genres",
    // Issue #69: deactivated users hard-deleted after 30 days.
    // `services::users::deactivate` (story 8-3) wipes the user's
    // sessions rows, but that invariant only holds for users
    // deactivated through that handler — seed/migration state, direct
    // SQL, or pre-8-3 deactivations leave sessions rows that the
    // `sessions.user_id` RESTRICT FK turns into a permanent DELETE
    // blocker (issue #416). `run_purge` therefore deletes sessions
    // rows for doomed users in the same transaction, right before the
    // users batch. `admin_audit.user_id` is SET NULL (migration
    // 20260513000002 / issue #70), so audit history survives the hard
    // delete with `user_username` + `user_role` preserved in the JSON
    // details payload.
    "users",
];

#[derive(Clone, Debug, Default)]
pub struct PurgeStats {
    /// Total whitelisted tables visited (incremented once per iteration of
    /// the outer deletion-order loop). Renamed from `tables_processed`
    /// (R3-N2) so the success/error split is unambiguous.
    pub tables_attempted: usize,
    /// Tables that completed their drain without erroring (whether or not
    /// any rows were actually deleted). Forensic-grade: 0 vs N here is the
    /// signal that distinguishes "DB went down mid-purge" from "nothing to
    /// do."
    pub tables_succeeded: usize,
    /// Tables where any batch failed (transaction begin/commit, FK
    /// violation, lock timeout, …). Such a table may still have committed
    /// some rows (mid-drain failure) — see `errors` for the per-table
    /// detail string. (R3-N2 + R3-N11.)
    pub tables_errored: usize,
    /// Fix #77 — Tables whose drain reached `MAX_DRAIN_ITERATIONS` and
    /// stopped without erroring. These were previously double-counted
    /// (as `tables_succeeded` AND pushed into `errors`), which made
    /// `errors_count > 0` ambiguous: was it a real failure, or just a
    /// backlog deferred to the next run? Splitting them out keeps the
    /// alarm signal clean (`tables_errored` = real failures; `tables_capped`
    /// = "remaining rows will be cleared on the next scheduled run").
    pub tables_capped: usize,
    pub rows_deleted: u64,
    /// Per-table deletion counts, keyed by table name. Every whitelisted
    /// table that was attempted appears here (R3-N7) — value is `0` if
    /// nothing was deleted (or the first batch errored). Forensic
    /// reconstruction can then distinguish "processed but empty" from
    /// "skipped due to error" by cross-referencing the `errors` list.
    /// Recorded into `admin_audit.details` per Story 8-7 AC3 + Patch P10.
    pub per_table: HashMap<String, u64>,
    pub errors: Vec<String>,
}

impl PurgeStats {
    /// Backward-compat alias retained for external callers (tests, logs)
    /// that still refer to the old name. Maps to `tables_succeeded`
    /// because that's the closest match for the "successfully processed"
    /// semantics the old field implied.
    pub fn tables_processed(&self) -> usize {
        self.tables_succeeded
    }
}

pub struct AutoPurgeService;

impl AutoPurgeService {
    /// Validate that every table in `PURGE_DELETION_ORDER` exists in the schema
    /// (called at startup from `main.rs`). On error main logs a warning and
    /// continues — this is a forensic guard, not a hard failure.
    pub async fn validate_schema(pool: &DbPool) -> Result<(), AppError> {
        for table in PURGE_DELETION_ORDER {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?)"
            )
            .bind(table)
            .fetch_one(pool)
            .await?;

            if !exists {
                return Err(AppError::Internal(format!("FK validation failed: table {} not found in schema", table)));
            }
        }

        tracing::info!("FK dependency validation passed");
        Ok(())
    }

    /// Run the 30-day auto-purge across all whitelisted tables.
    ///
    /// Per table:
    ///   - one transaction per LIMIT-bounded DELETE batch (commit between
    ///     batches so concurrent writers aren't blocked across the whole drain);
    ///   - drain loop continues until a batch returns < `DELETE_BATCH_SIZE`
    ///     rows OR the iteration cap is hit (defensive — see
    ///     `MAX_DRAIN_ITERATIONS`).
    ///
    /// On batch error (FK violation, lock timeout) the per-batch tx rolls back
    /// and the table is marked errored; the outer loop moves on so one bad
    /// family can't block the rest.
    ///
    /// After all tables are processed an `admin_audit` row is written with
    /// per-table counts (Story 8-7 P1 — moved from caller into the service so
    /// startup + scheduler share the same audit path).
    pub async fn run_purge(pool: &DbPool) -> Result<PurgeStats, AppError> {
        let mut stats = PurgeStats::default();

        for table in PURGE_DELETION_ORDER {
            let mut table_total: u64 = 0;
            let mut sessions_total: u64 = 0;
            let mut iterations: usize = 0;
            let mut errored = false;
            let mut drain_capped = false;

            // R3-N2 + R3-N11: every whitelisted table we visit counts as
            // attempted, even if the very first batch errors out.
            stats.tables_attempted += 1;

            loop {
                iterations += 1;

                let mut tx = match pool.begin().await {
                    Ok(tx) => tx,
                    Err(e) => {
                        let msg = format!("Failed to begin transaction for {} (after {} batch(es), {} rows committed): {}", table, iterations - 1, table_total, e);
                        tracing::error!("{}", msg);
                        stats.errors.push(msg);
                        errored = true;
                        break;
                    }
                };

                // Issue #416: sessions rows referencing a doomed user block
                // its DELETE via the RESTRICT FK forever — nothing else
                // purges stale authenticated sessions (anonymous_session_purge
                // only touches user_id IS NULL; story 7-2 expiry is
                // read-time only). Wipe them in the same transaction so the
                // users drain is FK-self-sufficient instead of relying on
                // the story 8-3 deactivation invariant.
                if *table == "users" {
                    match sqlx::query(
                        "DELETE FROM sessions WHERE user_id IN (\
                         SELECT id FROM users \
                         WHERE deleted_at IS NOT NULL \
                         AND deleted_at < NOW() - INTERVAL 30 DAY)",
                    )
                    .execute(&mut *tx)
                    .await
                    {
                        Ok(r) => {
                            let n = r.rows_affected();
                            if n > 0 {
                                tracing::info!(
                                    "Auto-purge users: deleted {} orphan session row(s) referencing doomed users",
                                    n
                                );
                                sessions_total += n;
                            }
                        }
                        Err(e) => {
                            let msg = format!(
                                "Failed to delete orphan sessions before users batch {} ({} rows already committed): {}",
                                iterations, table_total, e
                            );
                            tracing::error!("{}", msg);
                            stats.errors.push(msg);
                            if let Err(re) = tx.rollback().await {
                                tracing::error!("Failed to rollback transaction for {}: {}", table, re);
                            }
                            errored = true;
                            break;
                        }
                    }
                }

                // Hard-delete rows older than 30 days, bounded per batch so the
                // implicit row-locks don't block concurrent writers.
                let result = sqlx::query(&format!(
                    "DELETE FROM {} WHERE deleted_at IS NOT NULL AND deleted_at < NOW() - INTERVAL 30 DAY LIMIT {}",
                    table, DELETE_BATCH_SIZE
                ))
                .execute(&mut *tx)
                .await;

                let rows_affected = match result {
                    Ok(r) => r.rows_affected(),
                    Err(e) => {
                        let msg = format!("FK violation or error in {} (batch {}, {} rows already committed): {}", table, iterations, table_total, e);
                        tracing::error!("{}", msg);
                        stats.errors.push(msg);
                        if let Err(re) = tx.rollback().await {
                            tracing::error!("Failed to rollback transaction for {}: {}", table, re);
                        }
                        errored = true;
                        break;
                    }
                };

                if let Err(e) = tx.commit().await {
                    let msg = format!("Failed to commit transaction for {} (batch {}, {} rows already committed): {}", table, iterations, table_total, e);
                    tracing::error!("{}", msg);
                    stats.errors.push(msg);
                    errored = true;
                    break;
                }

                table_total += rows_affected;

                if rows_affected > 0 {
                    tracing::info!(
                        "Auto-purge {}: batch {} deleted {} rows",
                        table, iterations, rows_affected
                    );
                }

                // Drain done when last batch was partial.
                if rows_affected < DELETE_BATCH_SIZE {
                    break;
                }

                // Defensive cap to bound worst-case runtime; remaining stale
                // rows will be picked up by the next scheduled run.
                if iterations >= MAX_DRAIN_ITERATIONS {
                    tracing::warn!(
                        table = %table,
                        iterations = iterations,
                        deleted = table_total,
                        "Auto-purge drain iteration cap reached; remaining rows deferred to next run"
                    );
                    // R3-N12: surface the cap event in the stats so it
                    // shows up in admin_audit.details.errors_count rather
                    // than only in the log stream.
                    drain_capped = true;
                    break;
                }
            }

            stats.rows_deleted += table_total;
            if errored {
                stats.tables_errored += 1;
            } else {
                stats.tables_succeeded += 1;
            }
            // Fix #77 — drain-cap is NOT an error. The previous code
            // double-classified a capped run as both "succeeded" AND
            // pushed a string into `stats.errors` (which bumps
            // `errors_count` in the audit row), producing false alarms
            // for operators monitoring `errors_count > 0`. Now we
            // route the signal to a dedicated `tables_capped` counter,
            // log at warn (already done above), and leave `errors`
            // for actual transaction failures.
            if drain_capped {
                stats.tables_capped += 1;
                tracing::warn!(
                    table = %table,
                    rows_deleted = table_total,
                    "Auto-purge drain reached MAX_DRAIN_ITERATIONS; remaining rows deferred to next run"
                );
            }
            // R3-N7: every attempted table appears in `per_table`, even
            // with `0` when nothing was deleted or the first batch errored.
            stats.per_table.insert((*table).to_string(), table_total);
            // Issue #416: orphan session deletions ride along with the
            // users drain — surface them in the audit payload under their
            // own key (only when non-zero, so the forensic row stays free
            // of a permanent zero entry for a table auto-purge doesn't
            // otherwise visit).
            if sessions_total > 0 {
                stats.rows_deleted += sessions_total;
                stats.per_table.insert("sessions".to_string(), sessions_total);
            }
        }

        // Audit the run — startup and scheduler both use this path so the
        // audit trail is identical regardless of trigger (Story 8-7 P1).
        if let Err(e) = Self::record_purge_audit(pool, &stats).await {
            tracing::error!("Failed to record auto-purge in admin_audit: {}", e);
            stats.errors.push(format!("admin_audit insert failed: {}", e));
        }

        Ok(stats)
    }

    /// Record auto-purge in admin audit table (system action, no user_id).
    /// Includes per-table counts in the JSON `details` payload (Patch P10).
    pub async fn record_purge_audit(
        pool: &DbPool,
        stats: &PurgeStats,
    ) -> Result<(), AppError> {
        // Per-table map → JSON object so it round-trips as
        // `{"titles": 5, "volumes": 12, ...}` for forensic reconstruction.
        let per_table_json = serde_json::to_value(&stats.per_table)
            .unwrap_or(serde_json::Value::Null);

        let details = json!({
            // Issue #70: capture actor identity in the JSON payload so
            // the audit row survives an FK SET NULL when the SYSTEM
            // user row is itself deleted (admin_audit_user_fk via
            // migration 20260513000002). Hardcoded for SYSTEM since
            // those values are migration-stable.
            "user_username": "SYSTEM",
            "user_role": "system",
            // R3-N2 + R3-N11: split the conflated `tables_processed`
            // counter into attempted/succeeded/errored so forensic readers
            // can tell "everything ran clean" from "12 tables visited but
            // 3 of them errored mid-drain". `tables_processed` is kept as
            // an alias of `tables_succeeded` to preserve the field shape
            // for any downstream parser that still depends on it.
            "tables_attempted": stats.tables_attempted,
            "tables_succeeded": stats.tables_succeeded,
            "tables_errored": stats.tables_errored,
            // Fix #77 — surface drain-cap occurrences as their own
            // counter so operators can distinguish "real failure"
            // (`tables_errored > 0` or `errors_count > 0`) from
            // "deferred backlog" (`tables_capped > 0`, retries
            // automatically on the next scheduled run).
            "tables_capped": stats.tables_capped,
            "tables_processed": stats.tables_succeeded,
            "rows_deleted": stats.rows_deleted,
            "errors_count": stats.errors.len(),
            "per_table": per_table_json,
        });

        // Issue #68: attribute the row to the dedicated SYSTEM user
        // (migration 20260513000001) instead of hardcoding `user_id=1`.
        // A missing SYSTEM row points at a migration that did not run;
        // log loudly and fall back to id=1 so the audit insert does not
        // panic — the row will simply attribute to whatever admin owns
        // id=1, which is the pre-1.1.0 behaviour.
        let system_user_id =
            match crate::models::user::UserModel::find_system_user_id(pool).await {
                Ok(id) => id,
                Err(e) => {
                    tracing::error!(
                        error = ?e,
                        "SYSTEM user lookup failed — falling back to user_id=1. \
                         This points at migration 20260513000001 not having run; \
                         investigate and re-run migrations."
                    );
                    1
                }
            };

        AdminAuditModel::create(
            pool,
            system_user_id,
            "auto_purge",
            None,
            None,
            Some(details),
        )
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::soft_delete::ALLOWED_TABLES;
    use sqlx::Row;

    /// Fix #63 — `ALLOWED_TABLES` (the soft-delete API surface
    /// whitelist) and `PURGE_DELETION_ORDER` (the FK-safe deletion
    /// sequence used by auto-purge) used to drift silently: adding
    /// a new soft-deletable table to `ALLOWED_TABLES` without also
    /// listing it in `PURGE_DELETION_ORDER` meant that table was
    /// never auto-purged after the 30-day window. No compile-time
    /// enforcement caught it.
    ///
    /// The full refactor proposed in #63 (collapse both into a
    /// `PurgeableTable` enum) is a wider architectural change.
    /// This test ships the safety net: every `ALLOWED_TABLES`
    /// entry MUST also appear in `PURGE_DELETION_ORDER`. The
    /// reverse is NOT required (junction tables like
    /// `title_contributors` / `title_series` are auto-purged but
    /// not in the soft-delete API).
    ///
    /// Catches the drift on every CI run — converts the silent
    /// future-maintainer failure mode into a noisy CI red.
    #[test]
    fn allowed_tables_is_subset_of_purge_deletion_order() {
        for &t in ALLOWED_TABLES {
            assert!(
                PURGE_DELETION_ORDER.contains(&t),
                "table {t:?} is in ALLOWED_TABLES but missing from PURGE_DELETION_ORDER. \
                 Adding a soft-deletable table without auto-purge coverage means it \
                 accumulates soft-deleted rows forever. See #63 for the durable fix \
                 (collapse both lists into a single PurgeableTable enum)."
            );
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_purge_stats_empty_when_no_old_rows(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW())")
            .bind("Recent Delete")
            .execute(&pool)
            .await?;

        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert_eq!(stats.rows_deleted, 0, "No rows should be purged (less than 30 days old)");
        assert!(stats.errors.is_empty());

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_purge_deletes_31_day_old_rows(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW() - INTERVAL 31 DAY)")
            .bind("Old Delete")
            .execute(&pool)
            .await?;

        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert_eq!(stats.rows_deleted, 1, "Should purge 31-day-old row");
        assert!(stats.errors.is_empty());

        // Verify row is gone
        let check = sqlx::query("SELECT id FROM titles WHERE id = 1")
            .fetch_optional(&pool)
            .await?;
        assert!(check.is_none(), "Row should be hard-deleted");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_purge_respects_30_day_boundary(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW() - INTERVAL 29 DAY)")
            .bind("29 Day Delete")
            .execute(&pool)
            .await?;

        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW() - INTERVAL 31 DAY)")
            .bind("31 Day Delete")
            .execute(&pool)
            .await?;

        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert_eq!(stats.rows_deleted, 1, "Should purge only 31-day-old row");

        let check_29d = sqlx::query("SELECT id FROM titles WHERE title = '29 Day Delete'")
            .fetch_optional(&pool)
            .await?;
        assert!(check_29d.is_some(), "29-day-old row should still exist");

        Ok(())
    }

    /// Regression test for issue #60 — `ALLOWED_TABLES` filter was skipping
    /// junction-child tables so the FK-dependent rows survived and parent
    /// DELETEs hit FK violations, rolling back the transaction. After the
    /// fix, `PURGE_DELETION_ORDER` is canonical and children are visited
    /// before their parents.
    #[sqlx::test(migrations = "./migrations")]
    async fn test_purge_deletes_child_then_parent(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let title_id: u64 = sqlx::query(
            "INSERT INTO titles (title, media_type, genre_id, deleted_at) \
             VALUES (?, 'book', 1, NOW() - INTERVAL 31 DAY)",
        )
        .bind("Old Title with Series")
        .execute(&pool)
        .await?
        .last_insert_id();

        let series_id: u64 = sqlx::query(
            "INSERT INTO series (name, deleted_at) VALUES (?, NOW() - INTERVAL 31 DAY)",
        )
        .bind("Old Series")
        .execute(&pool)
        .await?
        .last_insert_id();

        sqlx::query(
            "INSERT INTO title_series (title_id, series_id, position_number, deleted_at) \
             VALUES (?, ?, 1, NOW() - INTERVAL 31 DAY)",
        )
        .bind(title_id)
        .bind(series_id)
        .execute(&pool)
        .await?;

        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert!(
            stats.errors.is_empty(),
            "purge should not error on FK-ordered children, got: {:?}",
            stats.errors
        );

        let title_left: Option<u64> =
            sqlx::query_scalar("SELECT id FROM titles WHERE id = ?")
                .bind(title_id)
                .fetch_optional(&pool)
                .await?;
        assert!(
            title_left.is_none(),
            "parent title should be hard-deleted once child title_series is purged first"
        );

        let series_left: Option<u64> =
            sqlx::query_scalar("SELECT id FROM series WHERE id = ?")
                .bind(series_id)
                .fetch_optional(&pool)
                .await?;
        assert!(
            series_left.is_none(),
            "parent series should be hard-deleted once child title_series is purged first"
        );

        let child_left: Option<u64> = sqlx::query_scalar(
            "SELECT title_id FROM title_series WHERE title_id = ? AND series_id = ?",
        )
        .bind(title_id)
        .bind(series_id)
        .fetch_optional(&pool)
        .await?;
        assert!(
            child_left.is_none(),
            "child title_series row should be hard-deleted"
        );

        Ok(())
    }

    /// Regression test for issue #416 — a soft-deleted user past the 30-day
    /// window whose `sessions` rows were NOT wiped by the story 8-3
    /// deactivation handler (seed state, direct SQL, pre-8-3 deactivation)
    /// blocked the users drain forever via the `fk_sessions_user` RESTRICT
    /// FK. The fix deletes the doomed users' sessions in the same
    /// transaction, right before the users batch.
    #[sqlx::test(migrations = "./migrations")]
    async fn test_purge_deletes_user_despite_orphan_sessions(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let user_id: u64 = sqlx::query(
            "INSERT INTO users (username, password_hash, role, deleted_at) \
             VALUES ('doomed_orphan_416', 'x', 'librarian', NOW() - INTERVAL 31 DAY)",
        )
        .execute(&pool)
        .await?
        .last_insert_id();

        sqlx::query("INSERT INTO sessions (token, user_id, data) VALUES ('orphan-session-416', ?, '{}')")
            .bind(user_id)
            .execute(&pool)
            .await?;

        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert!(
            stats.errors.is_empty(),
            "purge must not FK-error on a doomed user with live sessions, got: {:?}",
            stats.errors
        );
        assert_eq!(stats.tables_errored, 0);

        let user_left: Option<u64> = sqlx::query_scalar("SELECT id FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&pool)
            .await?;
        assert!(user_left.is_none(), "doomed user should be hard-deleted");

        let session_left: Option<String> =
            sqlx::query_scalar("SELECT token FROM sessions WHERE token = 'orphan-session-416'")
                .fetch_optional(&pool)
                .await?;
        assert!(session_left.is_none(), "orphan session row should be hard-deleted");

        assert!(
            stats.per_table.get("sessions").copied().unwrap_or(0) >= 1,
            "orphan session deletions should surface in per_table, got {:?}",
            stats.per_table
        );

        Ok(())
    }

    /// Issue #416 boundary companion — sessions of a user soft-deleted for
    /// LESS than 30 days are untouched: only users eligible for the hard
    /// purge get their sessions wiped.
    #[sqlx::test(migrations = "./migrations")]
    async fn test_purge_keeps_sessions_of_recently_deleted_user(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let user_id: u64 = sqlx::query(
            "INSERT INTO users (username, password_hash, role, deleted_at) \
             VALUES ('recent_delete_416', 'x', 'librarian', NOW() - INTERVAL 29 DAY)",
        )
        .execute(&pool)
        .await?
        .last_insert_id();

        sqlx::query("INSERT INTO sessions (token, user_id, data) VALUES ('recent-session-416', ?, '{}')")
            .bind(user_id)
            .execute(&pool)
            .await?;

        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert!(stats.errors.is_empty(), "got: {:?}", stats.errors);

        let user_left: Option<u64> = sqlx::query_scalar("SELECT id FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&pool)
            .await?;
        assert!(user_left.is_some(), "29-day user must survive the purge");

        let session_left: Option<String> =
            sqlx::query_scalar("SELECT token FROM sessions WHERE token = 'recent-session-416'")
                .fetch_optional(&pool)
                .await?;
        assert!(session_left.is_some(), "sessions of a not-yet-doomed user must survive");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_record_purge_audit(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut per_table = HashMap::new();
        per_table.insert("titles".to_string(), 5);
        per_table.insert("volumes".to_string(), 5);

        let stats = PurgeStats {
            tables_attempted: 5,
            tables_succeeded: 5,
            tables_errored: 0,
            tables_capped: 0,
            rows_deleted: 10,
            per_table,
            errors: vec![],
        };

        AutoPurgeService::record_purge_audit(&pool, &stats).await?;

        let check = sqlx::query("SELECT action FROM admin_audit WHERE action = 'auto_purge'")
            .fetch_one(&pool)
            .await?;
        let action: String = check.get("action");
        assert_eq!(action, "auto_purge");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_run_purge_writes_audit_with_per_table_counts(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Seed an old soft-deleted row that will be hard-purged.
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW() - INTERVAL 31 DAY)")
            .bind("Old Audited")
            .execute(&pool)
            .await?;

        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert!(stats.rows_deleted >= 1);
        assert_eq!(stats.per_table.get("titles").copied().unwrap_or(0), 1);

        // run_purge() writes the admin_audit row itself (P1).
        let row = sqlx::query("SELECT CAST(details AS CHAR) AS details FROM admin_audit WHERE action = 'auto_purge' ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool)
            .await?;
        let details_str: String = row.get("details");
        assert!(details_str.contains("\"per_table\""), "details should include per_table key, got {}", details_str);
        assert!(details_str.contains("\"titles\""), "details should mention titles, got {}", details_str);
        // R3-N2 + R3-N11: the audit row exposes the new split counters.
        assert!(details_str.contains("\"tables_attempted\""), "details should include tables_attempted, got {}", details_str);
        assert!(details_str.contains("\"tables_succeeded\""), "details should include tables_succeeded, got {}", details_str);
        assert!(details_str.contains("\"tables_errored\""), "details should include tables_errored, got {}", details_str);

        Ok(())
    }

    /// R3-N7: every whitelisted table that was visited shows up in
    /// `per_table`, even when zero rows were deleted. Forensic readers can
    /// then tell "table was processed but had nothing stale" from "table
    /// was skipped due to error".
    #[sqlx::test(migrations = "./migrations")]
    async fn test_per_table_includes_zero_count_entries(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Run with no stale rows anywhere — every whitelisted table should
        // still appear in per_table with a count of 0.
        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert_eq!(stats.rows_deleted, 0);
        for table in ALLOWED_TABLES {
            // Only tables that the runner visits via `deletion_order`
            // should show up. Cross-check: the deletion_order list above
            // contains every entity-data table; settings/sessions/users
            // are deliberately not in ALLOWED_TABLES at all.
            if matches!(*table, "titles" | "volumes" | "contributors" | "storage_locations"
                              | "borrowers" | "series" | "genres" | "loans"
                              | "title_contributors" | "series_title_assignments"
                              | "volume_locations") {
                assert!(
                    stats.per_table.contains_key(*table),
                    "per_table should include zero-count entry for {}, got keys {:?}",
                    table,
                    stats.per_table.keys().collect::<Vec<_>>()
                );
                assert_eq!(stats.per_table[*table], 0);
            }
        }
        // Stats counters: every visited table is "attempted" and (since
        // there are no errors) also "succeeded", with zero errored.
        assert!(stats.tables_attempted > 0);
        assert_eq!(stats.tables_attempted, stats.tables_succeeded);
        assert_eq!(stats.tables_errored, 0);
        Ok(())
    }

    /// R3-N2 + R3-N11: a clean run (no errors) gives `tables_attempted ==
    /// tables_succeeded` and `tables_errored == 0`. Counters are mutually
    /// exclusive per table.
    #[sqlx::test(migrations = "./migrations")]
    async fn test_stats_counters_clean_run(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let stats = AutoPurgeService::run_purge(&pool).await?;
        assert_eq!(stats.tables_succeeded + stats.tables_errored, stats.tables_attempted);
        assert_eq!(stats.tables_errored, 0);
        // Backward-compat alias still resolves.
        assert_eq!(stats.tables_processed(), stats.tables_succeeded);
        Ok(())
    }
}
