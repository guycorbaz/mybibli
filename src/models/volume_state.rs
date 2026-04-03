use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct VolumeStateModel {
    pub id: u64,
    pub name: String,
    pub is_loanable: bool,
}

impl std::fmt::Display for VolumeStateModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl VolumeStateModel {
    pub async fn list_active(pool: &DbPool) -> Result<Vec<VolumeStateModel>, AppError> {
        tracing::debug!("Listing active volume states");

        let rows = sqlx::query(
            "SELECT id, name, is_loanable FROM volume_states WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;

        let mut states = Vec::with_capacity(rows.len());
        for r in &rows {
            states.push(VolumeStateModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                is_loanable: r.try_get("is_loanable")?,
            });
        }
        Ok(states)
    }

    /// Check if a volume is loanable based on its condition state.
    /// Returns true if volume has no condition state (default loanable) or state.is_loanable is true.
    pub async fn is_loanable_by_volume(pool: &DbPool, volume_id: u64) -> Result<bool, AppError> {
        let row = sqlx::query(
            r#"SELECT vs.is_loanable
               FROM volume_states vs
               JOIN volumes v ON v.condition_state_id = vs.id
               WHERE v.id = ? AND v.deleted_at IS NULL AND vs.deleted_at IS NULL"#,
        )
        .bind(volume_id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(r.try_get("is_loanable")?),
            None => Ok(true), // No condition state assigned → loanable by default
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_state_display() {
        let state = VolumeStateModel {
            id: 1,
            name: "Neuf".to_string(),
            is_loanable: true,
        };
        assert_eq!(state.to_string(), "Neuf");
    }

    #[test]
    fn test_volume_state_clone() {
        let state = VolumeStateModel {
            id: 3,
            name: "Usé".to_string(),
            is_loanable: true,
        };
        let cloned = state.clone();
        assert_eq!(cloned.id, 3);
        assert_eq!(cloned.name, "Usé");
        assert!(cloned.is_loanable);
    }

    #[test]
    fn test_volume_state_not_loanable() {
        let state = VolumeStateModel {
            id: 5,
            name: "Détruit".to_string(),
            is_loanable: false,
        };
        assert!(!state.is_loanable);
    }
}
