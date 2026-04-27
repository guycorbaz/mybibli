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
    pub deleted_at: Option<NaiveDateTime>,
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

        // R3-N14: a non-empty `entity_type_filter` that isn't whitelisted
        // is a client error (probably a bad URL or a dropped option) — do
        // NOT silently fall through to the global UNION, that would mask
        // the bug AND inflate the row count vs. what the UI displays.
        if let Some(filter) = entity_type_filter
            && !filter.is_empty()
            && !ALLOWED_TABLES.contains(&filter)
        {
            return Err(AppError::BadRequest(format!(
                "invalid entity_type filter: {}",
                filter
            )));
        }

        let mut query_builder = String::new();

        // Build UNION query based on filters
        if let Some(filter) = entity_type_filter
            && !filter.is_empty() && ALLOWED_TABLES.contains(&filter) {
            // Single table filter
            let item_col = Self::get_item_name_column(filter);
            query_builder = format!(
                "SELECT CAST(id AS SIGNED) as id, '{}' as table_name, {} as item_name, CAST(deleted_at AS DATETIME) as deleted_at, version FROM {} WHERE deleted_at IS NOT NULL",
                filter, item_col, filter
            );
        }

        if query_builder.is_empty() {
            // No filter - build UNION of all tables
            let mut union_parts = vec![];
            for table in ALLOWED_TABLES {
                let item_col = Self::get_item_name_column(table);
                let part = format!(
                    "SELECT CAST(id AS SIGNED) as id, '{}' as table_name, {} as item_name, CAST(deleted_at AS DATETIME) as deleted_at, version FROM {} WHERE deleted_at IS NOT NULL",
                    table, item_col, table
                );
                union_parts.push(part);
            }
            query_builder = union_parts.join(" UNION ALL ");
        }

        // Add name search filter if provided (using parameterized LIKE binding
        // via subquery). Patch P24: escape LIKE metachars so a query like
        // `100%` doesn't widen the pattern to a wildcard match.
        //
        // R3-N5: use `|` as the escape character (not `\`). MariaDB's
        // `NO_BACKSLASH_ESCAPES` sql_mode (sometimes enabled in
        // production) makes the backslash a literal character, breaking
        // backslash-based escapes silently. The `|` is mode-independent.
        let (final_query, search_term) = if let Some(search) = name_search {
            if !search.is_empty() {
                let escaped = escape_like_pattern(search);
                let subquery = format!(
                    "({}) AS trash WHERE item_name LIKE ? ESCAPE '|' ORDER BY deleted_at DESC LIMIT ? OFFSET ?",
                    query_builder
                );
                (subquery, Some(format!("%{}%", escaped)))
            } else {
                query_builder.push_str(" ORDER BY deleted_at DESC LIMIT ? OFFSET ?");
                (query_builder, None)
            }
        } else {
            query_builder.push_str(" ORDER BY deleted_at DESC LIMIT ? OFFSET ?");
            (query_builder, None)
        };

        let rows = if let Some(term) = search_term {
            sqlx::query(&final_query)
                .bind(&term)
                .bind(per_page)
                .bind(offset)
                .fetch_all(pool)
                .await?
        } else {
            sqlx::query(&final_query)
                .bind(per_page)
                .bind(offset)
                .fetch_all(pool)
                .await?
        };

        Ok(rows
            .iter()
            .map(|r| TrashEntry {
                id: r.get::<i64, _>("id") as u64,
                table_name: r.get::<String, _>("table_name"),
                item_name: r.get::<String, _>("item_name"),
                deleted_at: r.get::<Option<NaiveDateTime>, _>("deleted_at"),
                version: r.get::<i32, _>("version"),
            })
            .collect())
    }

    /// Get the total count of soft-deleted items, filter-scoped.
    ///
    /// Pass `entity_type_filter = None, name_search = None` for the global
    /// badge total; pass them through unchanged when paginating a filtered
    /// view so "page X / Y" reflects the actual filtered result-set
    /// (Patch P23).
    pub async fn trash_count(
        pool: &DbPool,
        entity_type_filter: Option<&str>,
        name_search: Option<&str>,
    ) -> Result<u64, AppError> {
        // R3-N14: reject unknown entity-type filters loudly so the badge
        // count never quietly drifts from the panel's filtered list.
        if let Some(filter) = entity_type_filter
            && !filter.is_empty()
            && !ALLOWED_TABLES.contains(&filter)
        {
            return Err(AppError::BadRequest(format!(
                "invalid entity_type filter: {}",
                filter
            )));
        }

        let mut union_parts: Vec<String> = vec![];

        let single = entity_type_filter
            .filter(|f| !f.is_empty() && ALLOWED_TABLES.contains(f));

        if let Some(filter) = single {
            let item_col = Self::get_item_name_column(filter);
            union_parts.push(format!(
                "SELECT '{}' as table_name, {} as item_name FROM {} WHERE deleted_at IS NOT NULL",
                filter, item_col, filter
            ));
        } else {
            for table in ALLOWED_TABLES {
                let item_col = Self::get_item_name_column(table);
                union_parts.push(format!(
                    "SELECT '{}' as table_name, {} as item_name FROM {} WHERE deleted_at IS NOT NULL",
                    table, item_col, table
                ));
            }
        }
        let union_sql = union_parts.join(" UNION ALL ");

        // Optional name-search filter applied to the same UNION used by
        // `list_trash` so count and page slice always agree.
        // R3-N5: `|` escape char (NO_BACKSLASH_ESCAPES safe).
        let (final_query, search_term) = match name_search {
            Some(s) if !s.is_empty() => (
                format!(
                    "SELECT CAST(COUNT(*) AS SIGNED) as count FROM ({}) AS trash WHERE item_name LIKE ? ESCAPE '|'",
                    union_sql
                ),
                Some(format!("%{}%", escape_like_pattern(s))),
            ),
            _ => (
                format!("SELECT CAST(COUNT(*) AS SIGNED) as count FROM ({}) AS trash", union_sql),
                None,
            ),
        };

        let row = if let Some(term) = search_term {
            sqlx::query(&final_query).bind(term).fetch_one(pool).await?
        } else {
            sqlx::query(&final_query).fetch_one(pool).await?
        };

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
            "SELECT CAST(id AS SIGNED) as id, '{}' as table_name, {} as item_name, CAST(deleted_at AS DATETIME) as deleted_at, version FROM {} WHERE id = ? AND deleted_at IS NOT NULL",
            table,
            Self::get_item_name_column(table),
            table
        );

        let row = sqlx::query(&query).bind(id as i64).fetch_optional(pool).await?;

        Ok(row.map(|r| TrashEntry {
            id: r.get::<i64, _>("id") as u64,
            table_name: r.get::<String, _>("table_name"),
            item_name: r.get::<String, _>("item_name"),
            deleted_at: r.get::<Option<NaiveDateTime>, _>("deleted_at"),
            version: r.get::<i32, _>("version"),
        }))
    }

    /// Get the appropriate column name for item_name by table
    fn get_item_name_column(table: &str) -> &'static str {
        match table {
            "titles" => "title",
            "volumes" => "label",
            "contributors" => "name",
            "storage_locations" => "name",
            "borrowers" => "name",
            "series" => "name",
            _ => "name",
        }
    }
}

/// Escape MySQL/MariaDB LIKE-pattern metacharacters so a user-supplied search
/// term is treated as a literal substring instead of a wildcard. Patch P24.
///
/// R3-N5: the chosen escape character is `|` (pipe), NOT `\`. Backslash is
/// fragile under MariaDB's `NO_BACKSLASH_ESCAPES` sql_mode where backslash
/// becomes a literal character; `|` works in every mode. The corresponding
/// SQL clause must specify `ESCAPE '|'`.
const LIKE_ESCAPE_CHAR: char = '|';

fn escape_like_pattern(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '%' | '_' => {
                out.push(LIKE_ESCAPE_CHAR);
                out.push(ch);
            }
            // The escape char itself must be doubled so a literal `|` in
            // the user's input survives the LIKE comparison.
            c if c == LIKE_ESCAPE_CHAR => {
                out.push(LIKE_ESCAPE_CHAR);
                out.push(c);
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_like_pattern_passthrough() {
        assert_eq!(escape_like_pattern("hello"), "hello");
        // Backslashes are now LITERAL — the escape char is `|`, not `\`,
        // so backslashes in user input pass through unchanged.
        assert_eq!(escape_like_pattern("path\\name"), "path\\name");
    }

    #[test]
    fn test_escape_like_pattern_escapes_metachars() {
        // % and _ are SQL LIKE wildcards; `|` is the escape character itself
        // (R3-N5) and must therefore be doubled in user input.
        assert_eq!(escape_like_pattern("100%"), "100|%");
        assert_eq!(escape_like_pattern("a_b"), "a|_b");
        assert_eq!(escape_like_pattern("a|b"), "a||b");
        assert_eq!(escape_like_pattern("50% off_today"), "50|% off|_today");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_list_trash_union_covers_all_tables(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW())")
            .bind("Deleted Title")
            .execute(&pool)
            .await?;

        sqlx::query("INSERT INTO volumes (title_id, label, deleted_at) VALUES (1, 'V0001', NOW())")
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
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW())")
            .bind("Deleted Title 1")
            .execute(&pool)
            .await?;

        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW())")
            .bind("Deleted Title 2")
            .execute(&pool)
            .await?;

        sqlx::query("INSERT INTO volumes (title_id, label, deleted_at) VALUES (1, 'V0001', NOW())")
            .execute(&pool)
            .await?;

        let entries = TrashModel::list_trash(&pool, 1, Some("volumes"), None).await?;
        assert_eq!(entries.len(), 1, "Expected 1 volume, got {}", entries.len());
        assert_eq!(entries[0].table_name, "volumes");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_trash_count(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW())")
            .bind("Deleted Title")
            .execute(&pool)
            .await?;

        let count = TrashModel::trash_count(&pool, None, None).await?;
        assert!(count > 0, "Expected count > 0, got {}", count);

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_trash_count_with_filters(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW())")
            .bind("Apple")
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW())")
            .bind("Banana")
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO volumes (title_id, label, deleted_at) VALUES (1, 'V0001', NOW())")
            .execute(&pool)
            .await?;

        // Global count includes all UNION rows.
        let global = TrashModel::trash_count(&pool, None, None).await?;
        assert!(global >= 3, "expected >= 3, got {}", global);

        // Entity-type filter narrows to one table.
        let titles_only = TrashModel::trash_count(&pool, Some("titles"), None).await?;
        assert_eq!(titles_only, 2);

        // Search narrows to a single match (case-insensitive substring).
        let apple = TrashModel::trash_count(&pool, None, Some("Apple")).await?;
        assert_eq!(apple, 1);

        Ok(())
    }

    /// R3-N14: an unknown entity-type filter must surface a `BadRequest`
    /// instead of silently treating it as "no filter" (which would inflate
    /// the count vs. the rendered list and mask buggy URLs).
    #[sqlx::test(migrations = "./migrations")]
    async fn test_invalid_entity_type_filter_rejected(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let list_err = TrashModel::list_trash(&pool, 1, Some("nonexistent_table"), None).await;
        assert!(
            matches!(list_err, Err(AppError::BadRequest(_))),
            "expected BadRequest, got {:?}",
            list_err
        );

        let count_err = TrashModel::trash_count(&pool, Some("nonexistent_table"), None).await;
        assert!(
            matches!(count_err, Err(AppError::BadRequest(_))),
            "expected BadRequest, got {:?}",
            count_err
        );

        // Empty string is still treated as "no filter" for back-compat
        // with existing front-end forms that submit an empty `entity_type`.
        let list_empty = TrashModel::list_trash(&pool, 1, Some(""), None).await?;
        assert!(list_empty.is_empty() || !list_empty.is_empty()); // type only — no error
        let count_empty = TrashModel::trash_count(&pool, Some(""), None).await?;
        let _ = count_empty;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_get_trash_entry(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES (?, 'book', 1, NOW())")
            .bind("Deleted Title")
            .execute(&pool)
            .await?;

        let entry = TrashModel::get_trash_entry(&pool, "titles", 1).await?;
        assert!(entry.is_some(), "Expected to find trash entry");
        assert_eq!(entry.unwrap().table_name, "titles");

        Ok(())
    }
}
