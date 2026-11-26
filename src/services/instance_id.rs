//! Story 19-6 (#657, D4) — the instance id.
//!
//! Sixteen lowercase hex characters, generated **once per database** (the
//! `data_repairs` marker `19-6-instance-id`) into the K/V setting
//! `instance_id`. It names the backups of this library
//! (`mybibli-<instance_id>-…`), is carried by their manifest and is shown
//! on Admin → Health, under Versions. Not editable: no form writes it.

use crate::db::DbPool;
use crate::error::AppError;
use crate::services::data_repairs::run_once;

/// The `data_repairs` marker of the one-time generation.
pub const REPAIR_NAME: &str = "19-6-instance-id";
/// The `settings` key that holds the id.
pub const SETTING_KEY: &str = "instance_id";
/// Length of an instance id (8 random bytes, hex).
pub const INSTANCE_ID_LEN: usize = 16;

/// Positive shape (Rule #20): exactly 16 lowercase hex characters.
pub fn is_instance_id(s: &str) -> bool {
    s.len() == INSTANCE_ID_LEN && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A fresh random id.
pub fn generate() -> String {
    let bytes: [u8; INSTANCE_ID_LEN / 2] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Create the id unless this database already ran the generation, then
/// return it. Called at startup (non-fatal) and by every backup, so a
/// failed boot-time generation is retried. `INSERT IGNORE`: a row already
/// there (a database loaded from a backup) is kept.
pub async fn ensure(pool: &DbPool) -> Result<String, AppError> {
    run_once(pool, REPAIR_NAME, |conn| {
        Box::pin(async move {
            sqlx::query(
                "INSERT IGNORE INTO settings (setting_key, setting_value) VALUES (?, ?)",
            )
            .bind(SETTING_KEY)
            .bind(generate())
            .execute(&mut *conn)
            .await?;
            Ok(())
        })
    })
    .await?;
    read(pool)
        .await?
        .ok_or_else(|| AppError::Internal("instance_id setting missing or malformed".to_string()))
}

/// The stored id, if there is a well-formed one.
pub async fn read(pool: &DbPool) -> Result<Option<String>, AppError> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT setting_value FROM settings WHERE setting_key = ? AND deleted_at IS NULL",
    )
    .bind(SETTING_KEY)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(v,)| v).filter(|v| is_instance_id(v)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_id_has_the_instance_id_shape() {
        for _ in 0..50 {
            let id = generate();
            assert!(is_instance_id(&id), "{id}");
        }
    }

    #[test]
    fn the_shape_is_sixteen_lowercase_hex_characters_only() {
        assert!(is_instance_id("0123456789abcdef"));
        assert!(!is_instance_id("0123456789ABCDEF"));
        assert!(!is_instance_id("0123456789abcde"));
        assert!(!is_instance_id("0123456789abcdef0"));
        assert!(!is_instance_id("0123456789abcdeg"));
        assert!(!is_instance_id(""));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn the_id_is_created_once_and_kept(pool: DbPool) {
        let first = ensure(&pool).await.unwrap();
        assert!(is_instance_id(&first));
        let again = ensure(&pool).await.unwrap();
        assert_eq!(first, again, "a second call (a restart) keeps the id");
        assert_eq!(read(&pool).await.unwrap().as_deref(), Some(first.as_str()));
    }
}
