use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::borrower::BorrowerModel;
use crate::models::loan::LoanModel;
use crate::models::volume::VolumeModel;
use crate::models::volume_state::VolumeStateModel;

pub struct LoanService;

/// Maximum total attempts for `LoanService::register_loan` when transient
/// MariaDB conflicts occur. A value of 3 means: 1 initial attempt + up to
/// 2 retries on deadlock / lock-wait-timeout.
///
/// Concurrent `FOR UPDATE` active-loan check + UPDATE volumes + INSERT loans
/// paths can acquire next-key locks in incompatible orders across workers.
/// MariaDB either detects a deadlock cycle and picks a victim (SQLSTATE
/// 40001 / MySQL 1213) or lets one transaction hit `innodb_lock_wait_timeout`
/// (SQLSTATE HY000 / MySQL 1205). Both are transient and both warrant a
/// retry of the full transaction on a fresh connection (standard InnoDB
/// concurrent-write pattern, story 5-1c).
const LOAN_CREATE_MAX_ATTEMPTS: usize = 3;

/// Classify a SQLx error as a transient MariaDB conflict worth retrying.
/// Matches SQLSTATE 40001 (deadlock detected, MySQL 1213) and MySQL error
/// 1205 (lock wait timeout exceeded, SQLSTATE HY000).
///
/// We explicitly avoid matching on the substring "deadlock" in the error
/// message — that was too broad and could match unrelated errors whose
/// message text happens to contain that word (e.g., user-named constraints
/// or translated messages).
fn is_transient_conflict(err: &sqlx::Error) -> bool {
    let sqlx::Error::Database(db_err) = err else {
        return false;
    };
    if db_err.code().as_deref() == Some("40001") {
        return true;
    }
    db_err
        .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
        .is_some_and(|mysql_err| mysql_err.number() == 1205)
}

impl LoanService {
    /// Register a new loan after validating all business rules:
    /// - Volume exists and is not soft-deleted
    /// - Volume's condition state allows lending (is_loanable)
    /// - Volume is not already on loan
    /// - Borrower exists and is not soft-deleted
    ///
    /// Then saves previous_location_id and sets volume.location_id = NULL.
    /// Uses a transaction to prevent race conditions (TOCTOU).
    ///
    /// **Transient-conflict retry:** transient MariaDB deadlocks (SQLSTATE
    /// 40001) and lock-wait timeouts (MySQL 1205) on the `FOR UPDATE` →
    /// UPDATE volumes → INSERT loans path are automatically retried up to
    /// [`LOAN_CREATE_MAX_ATTEMPTS`] total attempts. Each attempt re-runs the
    /// full pre-transaction validation (volume fetch, is_loanable, borrower
    /// fetch) so concurrent soft-deletes or relocates between attempts do
    /// not leave stale state (e.g., a stale `previous_location_id` cached
    /// from the first attempt).
    pub async fn register_loan(
        pool: &DbPool,
        volume_id: u64,
        borrower_id: u64,
    ) -> Result<LoanModel, AppError> {
        let mut attempt: usize = 0;
        loop {
            attempt += 1;
            match Self::register_loan_attempt(pool, volume_id, borrower_id).await {
                Ok(loan) => return Ok(loan),
                Err(err) => {
                    let is_transient = matches!(
                        &err,
                        AppError::Database(db_err) if is_transient_conflict(db_err)
                    );
                    if is_transient && attempt < LOAN_CREATE_MAX_ATTEMPTS {
                        tracing::warn!(
                            volume_id = volume_id,
                            borrower_id = borrower_id,
                            attempt = attempt,
                            "loan create hit MariaDB transient conflict, retrying"
                        );
                        continue;
                    }
                    if is_transient {
                        tracing::error!(
                            volume_id = volume_id,
                            borrower_id = borrower_id,
                            attempts = attempt,
                            "loan create exhausted retries on MariaDB transient conflict"
                        );
                    }
                    return Err(err);
                }
            }
        }
    }

    /// One full attempt at registering a loan: pre-transaction validation
    /// (re-run on each retry to catch concurrent state changes) followed by
    /// the transactional body. Split out so the outer loop can retry the
    /// entire sequence on transient conflicts.
    async fn register_loan_attempt(
        pool: &DbPool,
        volume_id: u64,
        borrower_id: u64,
    ) -> Result<LoanModel, AppError> {
        // 1. Validate volume exists
        let volume = VolumeModel::find_by_id(pool, volume_id)
            .await?
            .ok_or_else(|| {
                AppError::BadRequest(rust_i18n::t!("loan.volume_not_found").to_string())
            })?;

        // 2. Check loanable condition
        let is_loanable = VolumeStateModel::is_loanable_by_volume(pool, volume_id).await?;
        if !is_loanable {
            return Err(AppError::BadRequest(
                rust_i18n::t!("loan.not_loanable").to_string(),
            ));
        }

        // 3. Validate borrower exists
        BorrowerModel::find_by_id(pool, borrower_id)
            .await?
            .ok_or_else(|| AppError::BadRequest(rust_i18n::t!("error.not_found").to_string()))?;

        // 4. Transactional body
        Self::register_loan_txn(pool, volume_id, borrower_id, volume.location_id).await
    }

    /// Inner transaction body. On transient conflicts the caller
    /// (`register_loan`) retries the whole attempt including validations.
    async fn register_loan_txn(
        pool: &DbPool,
        volume_id: u64,
        borrower_id: u64,
        previous_location_id: Option<u64>,
    ) -> Result<LoanModel, AppError> {
        let mut tx = pool.begin().await.map_err(AppError::Database)?;

        // Re-check active loan inside transaction to prevent TOCTOU race
        let active_loan = sqlx::query(
            "SELECT id FROM loans WHERE volume_id = ? AND returned_at IS NULL AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(volume_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        if active_loan.is_some() {
            tx.rollback().await.map_err(AppError::Database)?;
            return Err(AppError::BadRequest(
                rust_i18n::t!("loan.already_on_loan").to_string(),
            ));
        }

        // Clear volume location
        sqlx::query("UPDATE volumes SET location_id = NULL WHERE id = ? AND deleted_at IS NULL")
            .bind(volume_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::Database)?;

        // Create loan
        let result = sqlx::query(
            "INSERT INTO loans (volume_id, borrower_id, previous_location_id) VALUES (?, ?, ?)",
        )
        .bind(volume_id)
        .bind(borrower_id)
        .bind(previous_location_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        let loan_id = result.last_insert_id();

        // Read back the created loan inside the transaction
        let row = sqlx::query(
            r#"SELECT id, volume_id, borrower_id,
                      CAST(loaned_at AS DATETIME) AS loaned_at,
                      CAST(returned_at AS DATETIME) AS returned_at,
                      previous_location_id, version
               FROM loans WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(loan_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        tx.commit().await.map_err(AppError::Database)?;

        match row {
            Some(r) => Ok(LoanModel {
                id: r.try_get("id")?,
                volume_id: r.try_get("volume_id")?,
                borrower_id: r.try_get("borrower_id")?,
                loaned_at: r.try_get("loaned_at")?,
                returned_at: r.try_get("returned_at")?,
                previous_location_id: r.try_get("previous_location_id")?,
                version: r.try_get("version")?,
            }),
            None => Err(AppError::Internal(
                "Failed to retrieve created loan".to_string(),
            )),
        }
    }
    /// Process a loan return: set returned_at, restore volume location.
    /// Returns (volume_label, Option<restored_path>) for success message.
    pub async fn return_loan(
        pool: &DbPool,
        loan_id: u64,
    ) -> Result<(String, Option<String>), AppError> {
        // 1. Fetch loan — must exist and be active
        let loan = LoanModel::find_by_id(pool, loan_id).await?.ok_or_else(|| {
            AppError::BadRequest(rust_i18n::t!("loan.not_found").to_string())
        })?;

        if loan.returned_at.is_some() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("loan.already_returned").to_string(),
            ));
        }

        // 2. Transaction: mark returned + restore location
        let mut tx = pool.begin().await.map_err(AppError::Database)?;

        // Set returned_at with optimistic locking via version
        let result = sqlx::query(
            "UPDATE loans SET returned_at = NOW(), version = version + 1, updated_at = NOW() \
             WHERE id = ? AND version = ? AND returned_at IS NULL AND deleted_at IS NULL",
        )
        .bind(loan_id)
        .bind(loan.version)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::Conflict(
                rust_i18n::t!("error.conflict").to_string(),
            ));
        }

        // Restore volume location (previous_location_id may be NULL — that's valid)
        sqlx::query("UPDATE volumes SET location_id = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(loan.previous_location_id)
            .bind(loan.volume_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await.map_err(AppError::Database)?;

        // 3. Build success message data (after commit)
        let volume_label = VolumeModel::find_by_id(pool, loan.volume_id)
            .await?
            .map(|v| v.label)
            .unwrap_or_default();
        let escaped_label = crate::utils::html_escape(&volume_label);

        let restored_path = if let Some(loc_id) = loan.previous_location_id {
            match crate::models::location::LocationModel::get_path(pool, loc_id).await {
                Ok(path) => Some(crate::utils::html_escape(&path)),
                Err(_) => None,
            }
        } else {
            None
        };

        Ok((escaped_label, restored_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loan_service_struct_exists() {
        let _service = LoanService;
    }

    // Regression guard: any non-Database sqlx error variant must be
    // classified as non-transient so `register_loan` does not retry
    // unrelated failure classes. If a future sqlx upgrade reshuffles the
    // `sqlx::Error` enum, this test must be updated alongside the
    // `let sqlx::Error::Database(_) = err` pattern in `is_transient_conflict`.
    #[test]
    fn is_transient_conflict_rejects_non_database_errors() {
        assert!(!is_transient_conflict(&sqlx::Error::RowNotFound));
        assert!(!is_transient_conflict(&sqlx::Error::PoolTimedOut));
        assert!(!is_transient_conflict(&sqlx::Error::PoolClosed));
        assert!(!is_transient_conflict(&sqlx::Error::WorkerCrashed));
        assert!(!is_transient_conflict(&sqlx::Error::ColumnNotFound(
            "foo".to_string()
        )));
        assert!(!is_transient_conflict(&sqlx::Error::Protocol(
            "stream error".to_string()
        )));
    }

    // Construction of a real `sqlx::Error::Database` with a controllable
    // SQLSTATE / MySQL error number requires either a live connection or a
    // custom `DatabaseError` impl. The retry-on-40001 and retry-on-1205
    // paths are therefore exercised by the parallel E2E suite (story 5-1c)
    // and by the `#[sqlx::test]` integration suite rather than this
    // unit-test module. See `tests/find_similar.rs` for the DB-backed
    // pattern if a dedicated integration test for the retry ever lands.
}
