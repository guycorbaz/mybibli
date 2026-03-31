use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct VolumeStateModel {
    pub id: u64,
    pub name: String,
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
            "SELECT id, name FROM volume_states WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;

        let mut states = Vec::with_capacity(rows.len());
        for r in &rows {
            states.push(VolumeStateModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
            });
        }
        Ok(states)
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
        };
        assert_eq!(state.to_string(), "Neuf");
    }

    #[test]
    fn test_volume_state_clone() {
        let state = VolumeStateModel {
            id: 3,
            name: "Usé".to_string(),
        };
        let cloned = state.clone();
        assert_eq!(cloned.id, 3);
        assert_eq!(cloned.name, "Usé");
    }
}
