use chrono::NaiveDateTime;
use sqlx::Row;
use crate::db::DbPool;
use crate::error::AppError;
use crate::services::soft_delete::ALLOWED_TABLES;

#[derive(Clone, Debug)]
pub struct TrashEntry {
    pub id: u64,
    pub table_name: String,
    pub item_name: String,
    pub deleted_at: NaiveDateTime,
    pub version: i32,
}

pub struct TrashModel;

impl TrashModel {
    /// Fetch a paginated list of soft-deleted items across all whitelisted tables
    pub async fn list_trash(
        pool: &DbPool,
        page: u32,
        entity_type_filter: Option<&str>,
        name_search: Option<&str>,
    ) -> Result<Vec<TrashEntry>, AppError> {
        let per_page = 25i64;
        let offset = ((page as i64).saturating_sub(1)) * per_page;

        let mut query_builder = String::new();

        // Build UNION query based on filters
        if let Some(filter) = entity_type_filter {
            if !filter.is_empty() && ALLOWED_TABLES.contains(&filter) {
                // Single table filter
                let item_col = Self::get_item_name_column(filter);
                query_builder = format!(
                    "SELECT id, '{}' as table_name, {} as item_name, deleted_at, version FROM {} WHERE deleted_at IS NOT NULL",
                    filter, item_col, filter
                );
            }
        }

        if query_builder.is_empty() {
            // No filter - build UNION of all tables
            let mut union_parts = vec![];
            for table in ALLOWED_TABLES {
                let item_col = Self::get_item_name_column(table);
                let part = format!(
                    "SELECT id, '{}' as table_name, {} as item_name, deleted_at, version FROM {} WHERE deleted_at IS NOT NULL",
                    table, item_col, table
                );
                union_parts.push(part);
            }
            query_builder = union_parts.join(" UNION ALL ");
        }

        // Add name search filter if provided
        if let Some(search) = name_search && !search.is_empty() {
            query_builder = format!("({}) WHERE item_name LIKE '%{}%'", query_builder, search.replace("'", "''"));
        }

        // Add pagination
        query_builder.push_str(&format!(" ORDER BY deleted_at DESC LIMIT {} OFFSET {}", per_page, offset));

        let rows = sqlx::query(&query_builder).fetch_all(pool).await?;

        Ok(rows
            .iter()
            .map(|r| TrashEntry {
                id: r.get::<i64, _>("id") as u64,
                table_name: r.get::<String, _>("table_name"),
                item_name: r.get::<String, _>("item_name"),
                deleted_at: r.get::<NaiveDateTime, _>("deleted_at"),
                version: r.get::<i32, _>("version"),
            })
            .collect())
    }

    /// Get the total count of soft-deleted items (for badge)
    pub async fn trash_count(pool: &DbPool) -> Result<u64, AppError> {
        let query = Self::build_trash_count_query();
        let row = sqlx::query(&query).fetch_one(pool).await?;
        Ok(row.get::<i64, _>("count") as u64)
    }

    /// Fetch a single soft-deleted entry by table and id (for conflict detection)
    pub async fn get_trash_entry(
        pool: &DbPool,
        table: &str,
        id: u64,
    ) -> Result<Option<TrashEntry>, AppError> {
        if !ALLOWED_TABLES.contains(&table) {
            return Err(AppError::BadRequest(format!("Invalid table: {}", table)));
        }

        let query = format!(
            "SELECT id, '{}' as table_name, {} as item_name, deleted_at, version FROM {} WHERE id = ? AND deleted_at IS NOT NULL",
            table,
            Self::get_item_name_column(table),
            table
        );

        let row = sqlx::query(&query).bind(id as i64).fetch_optional(pool).await?;

        Ok(row.map(|r| TrashEntry {
            id: r.get::<i64, _>("id") as u64,
            table_name: r.get::<String, _>("table_name"),
            item_name: r.get::<String, _>("item_name"),
            deleted_at: r.get::<NaiveDateTime, _>("deleted_at"),
            version: r.get::<i32, _>("version"),
        }))
    }

    /// Build COUNT query for trash badge
    fn build_trash_count_query() -> String {
        let mut union_parts = vec![];

        for table in ALLOWED_TABLES {
            let part = format!(
                "SELECT COUNT(*) as count FROM {} WHERE deleted_at IS NOT NULL",
                table
            );
            union_parts.push(part);
        }

        format!("SELECT SUM(count) as count FROM ({}) as combined", union_parts.join(" UNION ALL "))
    }

    /// Get the appropriate column name for item_name by table
    fn get_item_name_column(table: &str) -> &'static str {
        match table {
            "titles" => "title",
            "volumes" => "volume_label",
            "contributors" => "contributor_name",
            "storage_locations" => "location_name",
            "borrowers" => "borrower_name",
            "series" => "series_name",
            _ => "name",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = "./migrations")]
    async fn test_list_trash_union_covers_all_tables(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, deleted_at) VALUES (?, NOW())")
            .bind("Deleted Title")
            .execute(&pool)
            .await?;

        sqlx::query("INSERT INTO volumes (title_id, volume_number, volume_label, deleted_at) VALUES (1, 1, 'Vol 1', NOW())")
            .execute(&pool)
            .await?;

        let entries = TrashModel::list_trash(&pool, 1, None, None).await?;
        assert!(entries.len() >= 2, "Expected at least 2 entries, got {}", entries.len());
        assert!(entries.iter().any(|e| e.table_name == "titles"), "Missing titles entry");
        assert!(entries.iter().any(|e| e.table_name == "volumes"), "Missing volumes entry");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_list_trash_with_entity_type_filter(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, deleted_at) VALUES (?, NOW())")
            .bind("Deleted Title 1")
            .execute(&pool)
            .await?;

        sqlx::query("INSERT INTO titles (title, deleted_at) VALUES (?, NOW())")
            .bind("Deleted Title 2")
            .execute(&pool)
            .await?;

        sqlx::query("INSERT INTO volumes (title_id, volume_number, volume_label, deleted_at) VALUES (1, 1, 'Vol 1', NOW())")
            .execute(&pool)
            .await?;

        let entries = TrashModel::list_trash(&pool, 1, Some("volumes"), None).await?;
        assert_eq!(entries.len(), 1, "Expected 1 volume, got {}", entries.len());
        assert_eq!(entries[0].table_name, "volumes");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_trash_count(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, deleted_at) VALUES (?, NOW())")
            .bind("Deleted Title")
            .execute(&pool)
            .await?;

        let count = TrashModel::trash_count(&pool).await?;
        assert!(count > 0, "Expected count > 0, got {}", count);

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_get_trash_entry(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, deleted_at) VALUES (?, NOW())")
            .bind("Deleted Title")
            .execute(&pool)
            .await?;

        let entry = TrashModel::get_trash_entry(&pool, "titles", 1).await?;
        assert!(entry.is_some(), "Expected to find trash entry");
        assert_eq!(entry.unwrap().table_name, "titles");

        Ok(())
    }
}
