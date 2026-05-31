use crate::error::AppError;

/// Check the result of an optimistic-locked UPDATE query.
///
/// If 0 rows affected, the version has changed (concurrent edit conflict)
/// OR the row was soft-deleted between read and write — the DB symptom
/// is identical so we report `VersionMismatch` (the overwhelmingly common
/// case under the single-tenant household load this project targets).
/// Callers that CAN distinguish (e.g. by re-querying the row's
/// `deleted_at` after a failed UPDATE) should prefer `AppError::SoftDeleted`
/// directly. See #370 for the previous shared-message gripe.
///
/// `entity_type` is `&'static str` so the resulting variant carries a
/// compile-time name (matches `AppError::VersionMismatch { entity }`).
pub fn check_update_result(
    rows_affected: u64,
    entity_type: &'static str,
) -> Result<(), AppError> {
    if rows_affected == 0 {
        Err(AppError::VersionMismatch {
            entity: entity_type,
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_update_result_success() {
        assert!(check_update_result(1, "title").is_ok());
    }

    #[test]
    fn test_check_update_result_multiple_rows() {
        assert!(check_update_result(3, "volume").is_ok());
    }

    #[test]
    fn test_check_update_result_conflict() {
        let result = check_update_result(0, "title");
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::VersionMismatch { entity } => assert_eq!(entity, "title"),
            other => panic!("Expected VersionMismatch, got: {other}"),
        }
    }
}
