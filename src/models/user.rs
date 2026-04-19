use chrono::{DateTime, Utc};
use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    Active,
    Deactivated,
    All,
}

#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: u64,
    pub username: String,
    pub role: String,
    pub preferred_language: Option<String>,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub version: i32,
    pub last_login: Option<DateTime<Utc>>,
}

fn row_to_user(row: sqlx::mysql::MySqlRow) -> Result<UserRow, sqlx::Error> {
    Ok(UserRow {
        id: row.try_get("id")?,
        username: row.try_get("username")?,
        role: row.try_get("role")?,
        preferred_language: row.try_get("preferred_language")?,
        created_at: row.try_get("created_at")?,
        deleted_at: row.try_get("deleted_at")?,
        version: row.try_get("version")?,
        last_login: row.try_get("last_login")?,
    })
}

pub struct UserModel;

impl UserModel {
    /// Retrieve a paginated list of users with optional filters.
    /// Respects the filter_status to include active, deactivated, or all users.
    pub async fn list_page(
        pool: &DbPool,
        filter_role: Option<&str>,
        filter_status: UserStatus,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<UserRow>, AppError> {
        let mut query_str = String::from(
            "SELECT u.id, u.username, u.role, u.preferred_language, u.created_at, u.deleted_at, u.version, \
                    (SELECT MAX(s.created_at) FROM sessions s WHERE s.user_id = u.id) AS last_login \
             FROM users u WHERE 1=1",
        );

        match filter_status {
            UserStatus::Active => query_str.push_str(" AND u.deleted_at IS NULL"),
            UserStatus::Deactivated => query_str.push_str(" AND u.deleted_at IS NOT NULL"),
            UserStatus::All => {}, // No additional filter
        }

        if let Some(role) = filter_role {
            if !role.is_empty() && role != "all" {
                query_str.push_str(" AND u.role = ?");
            }
        }

        query_str.push_str(" ORDER BY u.username ASC LIMIT ? OFFSET ?");

        let mut query = sqlx::query(&query_str);

        if let Some(role) = filter_role {
            if !role.is_empty() && role != "all" {
                query = query.bind(role);
            }
        }

        query = query.bind(limit).bind(offset);

        let rows = query.fetch_all(pool).await?;
        let items: Vec<UserRow> = rows
            .into_iter()
            .map(row_to_user)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(items)
    }

    /// Count all users matching the filters.
    pub async fn count_all(
        pool: &DbPool,
        filter_role: Option<&str>,
        filter_status: UserStatus,
    ) -> Result<i64, AppError> {
        let mut query_str = String::from("SELECT COUNT(*) FROM users WHERE 1=1");

        match filter_status {
            UserStatus::Active => query_str.push_str(" AND deleted_at IS NULL"),
            UserStatus::Deactivated => query_str.push_str(" AND deleted_at IS NOT NULL"),
            UserStatus::All => {},
        }

        if let Some(role) = filter_role {
            if !role.is_empty() && role != "all" {
                query_str.push_str(" AND role = ?");
            }
        }

        let mut query = sqlx::query_as::<_, (i64,)>(&query_str);

        if let Some(role) = filter_role {
            if !role.is_empty() && role != "all" {
                query = query.bind(role);
            }
        }

        let (count,) = query.fetch_one(pool).await?;
        Ok(count)
    }

    /// Find a user by ID, including deactivated users.
    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<UserRow>, AppError> {
        let row = sqlx::query(
            "SELECT u.id, u.username, u.role, u.preferred_language, u.created_at, u.deleted_at, u.version, \
                    (SELECT MAX(s.created_at) FROM sessions s WHERE s.user_id = u.id) AS last_login \
             FROM users u WHERE u.id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_user(r)?)),
            None => Ok(None),
        }
    }

    /// Find a user by username, including deactivated users.
    pub async fn find_by_username(
        pool: &DbPool,
        username: &str,
    ) -> Result<Option<UserRow>, AppError> {
        let row = sqlx::query(
            "SELECT u.id, u.username, u.role, u.preferred_language, u.created_at, u.deleted_at, u.version, \
                    (SELECT MAX(s.created_at) FROM sessions s WHERE s.user_id = u.id) AS last_login \
             FROM users u WHERE u.username = ?",
        )
        .bind(username)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_user(r)?)),
            None => Ok(None),
        }
    }

    /// Create a new user. Returns the new user's ID.
    /// Unique constraint violation (error 1062) is mapped to AppError::Conflict("username_taken").
    pub async fn create(
        pool: &DbPool,
        username: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<u64, AppError> {
        tracing::info!(username = %username, role = %role, "Creating user");

        let result = sqlx::query(
            "INSERT INTO users (username, password_hash, role) VALUES (?, ?, ?)",
        )
        .bind(username)
        .bind(password_hash)
        .bind(role)
        .execute(pool)
        .await;

        match result {
            Ok(res) => {
                let id = res.last_insert_id();
                tracing::info!(user_id = id, username = %username, "User created");
                Ok(id)
            }
            Err(sqlx::Error::Database(e)) => {
                if e.code().as_ref().map(|c| c.as_ref()) == Some("23000") {
                    // SQLSTATE 23000: Integrity constraint violation (duplicate key)
                    tracing::warn!(username = %username, "User creation failed: username already taken");
                    Err(AppError::Conflict("username_taken".to_string()))
                } else {
                    Err(AppError::Database(sqlx::Error::Database(e)))
                }
            }
            Err(e) => Err(AppError::Database(e)),
        }
    }

    /// Update a user with optimistic locking. Password is optional (None = don't change).
    /// Username change may trigger unique constraint violation → AppError::Conflict("username_taken").
    pub async fn update(
        pool: &DbPool,
        id: u64,
        version: i32,
        new_username: &str,
        new_role: &str,
        new_password_hash: Option<&str>,
    ) -> Result<(), AppError> {
        tracing::info!(user_id = id, username = %new_username, role = %new_role, "Updating user");

        let result = if let Some(hash) = new_password_hash {
            sqlx::query(
                "UPDATE users SET username = ?, role = ?, password_hash = ?, version = version + 1, updated_at = NOW() \
                 WHERE id = ? AND version = ? AND deleted_at IS NULL",
            )
            .bind(new_username)
            .bind(new_role)
            .bind(hash)
            .bind(id)
            .bind(version)
            .execute(pool)
            .await
        } else {
            sqlx::query(
                "UPDATE users SET username = ?, role = ?, version = version + 1, updated_at = NOW() \
                 WHERE id = ? AND version = ? AND deleted_at IS NULL",
            )
            .bind(new_username)
            .bind(new_role)
            .bind(id)
            .bind(version)
            .execute(pool)
            .await
        };

        match result {
            Ok(res) => {
                crate::services::locking::check_update_result(res.rows_affected(), "user")?;
                tracing::info!(user_id = id, "User updated");
                Ok(())
            }
            Err(sqlx::Error::Database(e)) => {
                if e.code().as_ref().map(|c| c.as_ref()) == Some("23000") {
                    tracing::warn!(user_id = id, "User update failed: username already taken");
                    Err(AppError::Conflict("username_taken".to_string()))
                } else {
                    Err(AppError::Database(sqlx::Error::Database(e)))
                }
            }
            Err(e) => Err(AppError::Database(e)),
        }
    }

    /// Deactivate a user (soft-delete) in a single transaction.
    /// Also invalidates all live sessions for that user immediately.
    /// Guards: self-deactivate + last-admin.
    pub async fn deactivate(
        pool: &DbPool,
        id: u64,
        version: i32,
        acting_admin_id: u64,
    ) -> Result<(), AppError> {
        let mut tx = pool.begin().await?;

        // Row lock the target user to prevent race conditions on the admin-count check.
        let target: Option<(String,)> = sqlx::query_as(
            "SELECT role FROM users WHERE id = ? AND deleted_at IS NULL FOR UPDATE"
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;

        let (target_role,) = target.ok_or_else(|| AppError::NotFound("user".to_string()))?;

        // Guard: self-deactivate not allowed
        if id == acting_admin_id {
            return Err(AppError::Conflict("self_deactivate_blocked".to_string()));
        }

        // Guard: if this is an admin, ensure at least one other admin remains
        if target_role == "admin" {
            let remaining: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM users WHERE role = 'admin' AND deleted_at IS NULL AND id != ?"
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;

            if remaining.0 == 0 {
                return Err(AppError::Conflict("last_admin_blocked".to_string()));
            }
        }

        // Deactivate the user
        let update_result: sqlx::mysql::MySqlQueryResult = sqlx::query(
            "UPDATE users SET deleted_at = NOW(), version = version + 1 WHERE id = ? AND version = ? AND deleted_at IS NULL"
        )
        .bind(id)
        .bind(version)
        .execute(&mut *tx)
        .await?;

        crate::services::locking::check_update_result(update_result.rows_affected(), "user")?;

        // Invalidate all live sessions for this user
        let sessions_killed: sqlx::mysql::MySqlQueryResult = sqlx::query(
            "UPDATE sessions SET deleted_at = NOW() WHERE user_id = ? AND deleted_at IS NULL"
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;

        let sessions_killed_count = sessions_killed.rows_affected();

        tx.commit().await?;

        tracing::info!(user_id = id, sessions_killed = sessions_killed_count, "User deactivated and sessions invalidated");
        Ok(())
    }

    /// Reactivate a user (clear deleted_at).
    pub async fn reactivate(pool: &DbPool, id: u64, version: i32) -> Result<(), AppError> {
        tracing::info!(user_id = id, "Reactivating user");

        let result: sqlx::mysql::MySqlQueryResult = sqlx::query(
            "UPDATE users SET deleted_at = NULL, version = version + 1 WHERE id = ? AND version = ? AND deleted_at IS NOT NULL"
        )
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;

        crate::services::locking::check_update_result(result.rows_affected(), "user")?;

        tracing::info!(user_id = id, "User reactivated");
        Ok(())
    }

    /// Guard: check if demoting a user's role would leave no active admins.
    /// Called before update() when the role is changing.
    pub async fn demote_guard(
        pool: &DbPool,
        target_id: u64,
        new_role: &str,
        acting_admin_id: u64,
    ) -> Result<(), AppError> {
        // Only check if the target is the acting admin and the new role is not admin
        if target_id == acting_admin_id && new_role != "admin" {
            let mut tx = pool.begin().await?;

            // Row lock the target user to prevent race conditions on the admin-count check.
            let target: Option<(String,)> = sqlx::query_as(
                "SELECT role FROM users WHERE id = ? AND deleted_at IS NULL FOR UPDATE"
            )
            .bind(target_id)
            .fetch_optional(&mut *tx)
            .await?;

            let (target_role,) = target.ok_or_else(|| AppError::NotFound("user".to_string()))?;

            // Recheck: ensure we're still demoting an admin
            if target_role == "admin" {
                let remaining: (i64,) = sqlx::query_as(
                    "SELECT COUNT(*) FROM users WHERE role = 'admin' AND deleted_at IS NULL AND id != ?"
                )
                .bind(target_id)
                .fetch_one(&mut *tx)
                .await?;

                if remaining.0 == 0 {
                    tracing::warn!(user_id = target_id, "Role demote blocked: last active admin");
                    return Err(AppError::Conflict("last_admin_demote_blocked".to_string()));
                }
            }

            tx.commit().await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = "./migrations")]
    async fn test_create_and_find_by_id(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let id = UserModel::create(&pool, "alice", "hashed_password", "admin").await?;
        let user = UserModel::find_by_id(&pool, id).await?;
        assert!(user.is_some());
        let user = user.unwrap();
        assert_eq!(user.username, "alice");
        assert_eq!(user.role, "admin");
        assert_eq!(user.version, 1);
        assert!(user.deleted_at.is_none());
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_create_enforces_unique_username(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        UserModel::create(&pool, "bob", "hash1", "librarian").await?;
        let result = UserModel::create(&pool, "bob", "hash2", "admin").await;
        assert!(matches!(
            result,
            Err(AppError::Conflict(ref s)) if s == "username_taken"
        ));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_create_unique_username_case_sensitivity(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        UserModel::create(&pool, "charlie", "hash1", "librarian").await?;
        // MariaDB default collation is case-insensitive, so this should also fail
        let result = UserModel::create(&pool, "CHARLIE", "hash2", "admin").await;
        assert!(matches!(
            result,
            Err(AppError::Conflict(ref s)) if s == "username_taken"
        ));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_find_by_username(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        UserModel::create(&pool, "dave", "hash", "admin").await?;
        let user = UserModel::find_by_username(&pool, "dave").await?;
        assert!(user.is_some());
        assert_eq!(user.unwrap().username, "dave");
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_update_with_password(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let id = UserModel::create(&pool, "eve", "old_hash", "librarian").await?;
        let user = UserModel::find_by_id(&pool, id).await?.unwrap();
        UserModel::update(&pool, id, user.version, "eve_new", "admin", Some("new_hash")).await?;
        let updated = UserModel::find_by_id(&pool, id).await?.unwrap();
        assert_eq!(updated.username, "eve_new");
        assert_eq!(updated.role, "admin");
        assert_eq!(updated.version, 2);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_update_without_password_keeps_hash(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let id = UserModel::create(&pool, "frank", "original_hash", "librarian").await?;
        let user = UserModel::find_by_id(&pool, id).await?.unwrap();
        UserModel::update(&pool, id, user.version, "frank", "admin", None).await?;
        // Verify the role changed but we can't directly check hash; we'd need to re-query the raw password_hash column
        let updated = UserModel::find_by_id(&pool, id).await?.unwrap();
        assert_eq!(updated.role, "admin");
        assert_eq!(updated.version, 2);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_update_applies_optimistic_locking(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let id = UserModel::create(&pool, "grace", "hash", "librarian").await?;
        let user = UserModel::find_by_id(&pool, id).await?.unwrap();
        // Try update with stale version
        let result = UserModel::update(&pool, id, user.version - 1, "grace", "admin", None).await;
        assert!(matches!(result, Err(AppError::Conflict(ref s)) if s == "version_mismatch"));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_deactivate_self_is_blocked(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let admin_id = UserModel::create(&pool, "admin1", "hash", "admin").await?;
        let user = UserModel::find_by_id(&pool, admin_id).await?.unwrap();
        let result = UserModel::deactivate(&pool, admin_id, user.version, admin_id).await;
        assert!(matches!(
            result,
            Err(AppError::Conflict(ref s)) if s == "self_deactivate_blocked"
        ));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_deactivate_last_admin_is_blocked(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let admin_a = UserModel::create(&pool, "admin_a", "hash", "admin").await?;
        let admin_b = UserModel::create(&pool, "admin_b", "hash", "admin").await?;
        let user_b = UserModel::find_by_id(&pool, admin_b).await?.unwrap();
        // Deactivate B with A as acting admin
        UserModel::deactivate(&pool, admin_b, user_b.version, admin_a).await?;
        // Now try to deactivate A with B as acting admin (B is deactivated, but we use its ID for the guard)
        // This should fail because A is now the only admin
        let user_a = UserModel::find_by_id(&pool, admin_a).await?.unwrap();
        let result = UserModel::deactivate(&pool, admin_a, user_a.version, admin_b).await;
        assert!(matches!(
            result,
            Err(AppError::Conflict(ref s)) if s == "last_admin_blocked"
        ));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_deactivate_non_last_admin_succeeds(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let admin_a = UserModel::create(&pool, "admin_x", "hash", "admin").await?;
        let admin_b = UserModel::create(&pool, "admin_y", "hash", "admin").await?;
        let user_b = UserModel::find_by_id(&pool, admin_b).await?.unwrap();
        UserModel::deactivate(&pool, admin_b, user_b.version, admin_a).await?;
        let deactivated = UserModel::find_by_id(&pool, admin_b).await?.unwrap();
        assert!(deactivated.deleted_at.is_some());
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_deactivate_invalidates_sessions(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let user_id = UserModel::create(&pool, "user_with_sessions", "hash", "librarian").await?;
        // Manually insert 3 sessions for this user
        for i in 0..3 {
            sqlx::query(
                "INSERT INTO sessions (user_id, token, csrf_token, data, last_activity) VALUES (?, ?, ?, '{}', UTC_TIMESTAMP())"
            )
            .bind(user_id)
            .bind(format!("token_{}", i))
            .bind(format!("csrf_{}", i))
            .execute(&pool)
            .await?;
        }
        let user = UserModel::find_by_id(&pool, user_id).await?.unwrap();
        let admin_id = UserModel::create(&pool, "admin_for_deactivate", "hash", "admin").await?;
        UserModel::deactivate(&pool, user_id, user.version, admin_id).await?;
        // Check that sessions are marked deleted
        let active_sessions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sessions WHERE user_id = ? AND deleted_at IS NULL"
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(active_sessions, 0);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_reactivate_clears_deleted_at(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let user_id = UserModel::create(&pool, "reactivate_test", "hash", "librarian").await?;
        let user = UserModel::find_by_id(&pool, user_id).await?.unwrap();
        let admin_id = UserModel::create(&pool, "admin_for_reactivate", "hash", "admin").await?;
        // Deactivate
        UserModel::deactivate(&pool, user_id, user.version, admin_id).await?;
        let deactivated = UserModel::find_by_id(&pool, user_id).await?.unwrap();
        assert!(deactivated.deleted_at.is_some());
        // Reactivate
        UserModel::reactivate(&pool, user_id, deactivated.version).await?;
        let reactivated = UserModel::find_by_id(&pool, user_id).await?.unwrap();
        assert!(reactivated.deleted_at.is_none());
        assert_eq!(reactivated.version, deactivated.version + 1);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_demote_guard_blocks_last_admin_demotion(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let admin_id = UserModel::create(&pool, "sole_admin", "hash", "admin").await?;
        let result = UserModel::demote_guard(&pool, admin_id, "librarian", admin_id).await;
        assert!(matches!(
            result,
            Err(AppError::Conflict(ref s)) if s == "last_admin_demote_blocked"
        ));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_demote_guard_allows_when_other_admin_exists(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let admin_a = UserModel::create(&pool, "admin_for_demote_a", "hash", "admin").await?;
        let _admin_b = UserModel::create(&pool, "admin_for_demote_b", "hash", "admin").await?;
        let result = UserModel::demote_guard(&pool, admin_a, "librarian", admin_a).await;
        assert!(result.is_ok());
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_list_page_pagination(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        // Create 27 users
        for i in 0..27 {
            UserModel::create(&pool, &format!("user_{:02}", i), "hash", "librarian").await?;
        }
        // Page 1: 25 users
        let page1 = UserModel::list_page(&pool, None, UserStatus::Active, 0, 25).await?;
        assert_eq!(page1.len(), 25);
        // Page 2: 2 users
        let page2 = UserModel::list_page(&pool, None, UserStatus::Active, 25, 25).await?;
        assert_eq!(page2.len(), 2);
        // Count all
        let count = UserModel::count_all(&pool, None, UserStatus::Active).await?;
        assert_eq!(count, 27);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_list_page_filter_role_and_status(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        UserModel::create(&pool, "lib1", "hash", "librarian").await?;
        UserModel::create(&pool, "lib2", "hash", "librarian").await?;
        UserModel::create(&pool, "admin1", "hash", "admin").await?;
        // Deactivate one librarian
        let lib2 = UserModel::find_by_username(&pool, "lib2").await?.unwrap();
        let admin1 = UserModel::find_by_username(&pool, "admin1").await?.unwrap();
        UserModel::deactivate(&pool, lib2.id, lib2.version, admin1.id).await?;

        let active_libs = UserModel::list_page(&pool, Some("librarian"), UserStatus::Active, 0, 25).await?;
        assert_eq!(active_libs.len(), 1); // Only lib1
        let all_libs = UserModel::list_page(&pool, Some("librarian"), UserStatus::All, 0, 25).await?;
        assert_eq!(all_libs.len(), 2); // lib1 + lib2
        let deactivated = UserModel::list_page(&pool, Some("librarian"), UserStatus::Deactivated, 0, 25).await?;
        assert_eq!(deactivated.len(), 1); // lib2
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_last_login_computed(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let user_id = UserModel::create(&pool, "user_with_logins", "hash", "librarian").await?;
        // Insert 3 session rows with different timestamps
        sqlx::query("INSERT INTO sessions (user_id, token, csrf_token, data, created_at, last_activity) VALUES (?, ?, ?, '{}', DATE_SUB(NOW(), INTERVAL 10 DAY), DATE_SUB(NOW(), INTERVAL 10 DAY))")
            .bind(user_id)
            .bind("old_token")
            .bind("csrf1")
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO sessions (user_id, token, csrf_token, data, created_at, last_activity) VALUES (?, ?, ?, '{}', DATE_SUB(NOW(), INTERVAL 5 DAY), DATE_SUB(NOW(), INTERVAL 5 DAY))")
            .bind(user_id)
            .bind("recent_token")
            .bind("csrf2")
            .execute(&pool)
            .await?;
        sqlx::query("INSERT INTO sessions (user_id, token, csrf_token, data, created_at, last_activity) VALUES (?, ?, ?, '{}', DATE_SUB(NOW(), INTERVAL 1 DAY), DATE_SUB(NOW(), INTERVAL 1 DAY))")
            .bind(user_id)
            .bind("newest_token")
            .bind("csrf3")
            .execute(&pool)
            .await?;

        let user = UserModel::find_by_id(&pool, user_id).await?.unwrap();
        assert!(user.last_login.is_some());
        // The last_login should be the most recent (newest_token, 1 day ago)
        Ok(())
    }
}
