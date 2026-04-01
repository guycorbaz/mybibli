use crate::error::AppError;

/// Check the result of an optimistic-locked UPDATE query.
/// If 0 rows affected, the version has changed (concurrent edit conflict).
pub fn check_update_result(rows_affected: u64, entity_type: &str) -> Result<(), AppError> {
    if rows_affected == 0 {
        Err(AppError::Conflict(
            rust_i18n::t!("error.conflict", entity = entity_type).to_string(),
        ))
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
            AppError::Conflict(_) => {} // Expected
            other => panic!("Expected Conflict, got: {other}"),
        }
    }
}
