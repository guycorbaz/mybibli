use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::borrower::BorrowerModel;
use crate::models::loan::LoanModel;
use crate::models::volume::VolumeModel;
use crate::models::volume_state::VolumeStateModel;

pub struct LoanService;

impl LoanService {
    /// Register a new loan after validating all business rules:
    /// - Volume exists and is not soft-deleted
    /// - Volume's condition state allows lending (is_loanable)
    /// - Volume is not already on loan
    /// - Borrower exists and is not soft-deleted
    ///
    /// Then saves previous_location_id and sets volume.location_id = NULL.
    /// Uses a transaction to prevent race conditions (TOCTOU).
    pub async fn register_loan(
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
            .ok_or_else(|| {
                AppError::BadRequest(rust_i18n::t!("error.not_found").to_string())
            })?;

        // 4. Transaction: double-loan check + location update + loan insert
        let mut tx = pool.begin().await.map_err(AppError::Database)?;

        // Re-check active loan inside transaction to prevent TOCTOU race
        let active_loan = sqlx::query(
            "SELECT id FROM loans WHERE volume_id = ? AND returned_at IS NULL AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(volume_id)
        .fetch_optional(&mut *tx)
        .await?;

        if active_loan.is_some() {
            tx.rollback().await.map_err(AppError::Database)?;
            return Err(AppError::BadRequest(
                rust_i18n::t!("loan.already_on_loan").to_string(),
            ));
        }

        // Clear volume location
        let previous_location_id = volume.location_id;
        sqlx::query("UPDATE volumes SET location_id = NULL WHERE id = ? AND deleted_at IS NULL")
            .bind(volume_id)
            .execute(&mut *tx)
            .await?;

        // Create loan
        let result = sqlx::query(
            "INSERT INTO loans (volume_id, borrower_id, previous_location_id) VALUES (?, ?, ?)",
        )
        .bind(volume_id)
        .bind(borrower_id)
        .bind(previous_location_id)
        .execute(&mut *tx)
        .await?;

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
        .await?;

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
}
