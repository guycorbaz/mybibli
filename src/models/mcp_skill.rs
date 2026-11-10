//! Story 15-2 (#614, FR162) — MCP skills: Markdown instructions an
//! administrator writes once for the agent.
//!
//! Admin side (this story): `list_all` (inactive included), `find_by_id`,
//! `create`, `update`, `set_active`, `soft_delete`. Agent side (story
//! 15-3): `list_active`, `count_active` and `find_active_by_name` — the
//! only reads the MCP layer needs (through `services::mcp_skills`), all
//! limited to live **and** active rows.
//!
//! Story 17-6: `insert_default_in` writes the default skills mybibli ships
//! (`services::default_skills`), flagged `provided_by_mybibli` until an
//! admin changes them.
//!
//! Every write takes already-validated input (`services::mcp_skills`).
//! Name reuse: `name` is UNIQUE across all rows, trashed ones included. A
//! create or a rename onto a trashed name is refused (`NameInTrash`): the
//! trashed skill is never overwritten, and a Trash restore never collides.

use chrono::NaiveDateTime;
use sqlx::{MySqlConnection, Row};

use crate::db::DbPool;
use crate::error::AppError;
use crate::services::mcp_skills::ValidSkill;

/// One skill as the admin list and editor show it.
#[derive(Debug, Clone)]
pub struct McpSkill {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub body: String,
    pub active: bool,
    pub version: i32,
    /// UTC, as stored (`TIMESTAMP`); the web converts it to the instance
    /// zone for display (story 15-1).
    pub updated_at: NaiveDateTime,
    /// Username of the last change's author; `None` once that user was
    /// purged (`ON DELETE SET NULL`) or for a row written outside the UI.
    pub updated_by_name: Option<String>,
    /// Story 17-6 — a default skill no admin has changed since mybibli
    /// inserted it (cleared by `update` and `set_active`).
    pub provided_by_mybibli: bool,
}

/// What the agent reads (story 15-3): one live, active skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSkill {
    pub name: String,
    pub description: String,
    pub body: String,
    /// UTC, as stored (`TIMESTAMP`).
    pub updated_at: NaiveDateTime,
}

/// A live, active skill as listed to the agent (instructions, prompts):
/// no body, so a list never carries up to 64 KiB per skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSkillSummary {
    pub name: String,
    pub description: String,
}

/// Why a write was refused, beyond a database failure.
#[derive(Debug)]
pub enum SkillWriteError {
    /// Another live skill already has this name.
    NameTaken,
    /// A skill in the Trash has this name (create or rename).
    NameInTrash,
    /// The row's version moved since the editor was opened.
    Stale,
    /// The row is no longer live (deleted meanwhile, or never existed).
    Gone,
    Db(AppError),
}

impl From<AppError> for SkillWriteError {
    fn from(err: AppError) -> Self {
        SkillWriteError::Db(err)
    }
}

impl From<sqlx::Error> for SkillWriteError {
    fn from(err: sqlx::Error) -> Self {
        SkillWriteError::Db(AppError::from(err))
    }
}

fn is_duplicate_key(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23000"))
}

/// Columns of `McpSkill`, over `mcp_skills s LEFT JOIN users u`.
///
/// The author join deliberately has no `u.deleted_at IS NULL`: it names
/// who made a past change (like `proposals.decided_by_name`), and a
/// deactivated admin still made it. Once the user row is purged,
/// `updated_by` is SET NULL and the name reads as unknown.
const SELECT_SKILL: &str = "SELECT s.id, s.name, s.description, s.body, s.active, s.version, \
     CAST(s.updated_at AS DATETIME) AS updated_at, u.username AS updated_by_name, \
     s.provided_by_mybibli \
     FROM mcp_skills s LEFT JOIN users u ON u.id = s.updated_by";

pub struct McpSkillModel;

impl McpSkillModel {
    fn from_row(r: &sqlx::mysql::MySqlRow) -> Result<McpSkill, AppError> {
        Ok(McpSkill {
            id: r.try_get("id")?,
            name: r.try_get("name")?,
            description: r.try_get("description")?,
            body: r.try_get("body")?,
            active: r.try_get("active")?,
            version: r.try_get("version")?,
            updated_at: r.try_get("updated_at")?,
            updated_by_name: r.try_get("updated_by_name")?,
            provided_by_mybibli: r.try_get("provided_by_mybibli")?,
        })
    }

    /// Every live skill, inactive ones included, by name (admin list).
    pub async fn list_all(pool: &DbPool) -> Result<Vec<McpSkill>, AppError> {
        let rows = sqlx::query(&format!(
            "{SELECT_SKILL} WHERE s.deleted_at IS NULL ORDER BY s.name"
        ))
        .fetch_all(pool)
        .await?;
        rows.iter().map(Self::from_row).collect()
    }

    /// A live skill by id (active or not).
    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<McpSkill>, AppError> {
        let row = sqlx::query(&format!(
            "{SELECT_SKILL} WHERE s.id = ? AND s.deleted_at IS NULL"
        ))
        .bind(id)
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(Self::from_row).transpose()
    }

    /// Story 15-3 — the skills the agent may read: live and active, by
    /// name, at most `limit` of them (name and description only).
    pub async fn list_active(
        pool: &DbPool,
        limit: u32,
    ) -> Result<Vec<ActiveSkillSummary>, AppError> {
        let rows = sqlx::query(
            "SELECT name, description FROM mcp_skills \
             WHERE deleted_at IS NULL AND active = TRUE ORDER BY name LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(ActiveSkillSummary {
                    name: r.try_get("name")?,
                    description: r.try_get("description")?,
                })
            })
            .collect()
    }

    /// Story 15-3 — how many live, active skills there are (the
    /// instructions say how many a capped list left out).
    pub async fn count_active(pool: &DbPool) -> Result<u64, AppError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM mcp_skills WHERE deleted_at IS NULL AND active = TRUE",
        )
        .fetch_one(pool)
        .await?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Story 15-3 — one live, active skill by its exact name; `None` for an
    /// unknown, inactive or trashed name.
    pub async fn find_active_by_name(
        pool: &DbPool,
        name: &str,
    ) -> Result<Option<ActiveSkill>, AppError> {
        let row = sqlx::query(
            "SELECT name, description, body, CAST(updated_at AS DATETIME) AS updated_at \
             FROM mcp_skills WHERE name = ? AND deleted_at IS NULL AND active = TRUE",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;
        row.map(|r| {
            Ok(ActiveSkill {
                name: r.try_get("name")?,
                description: r.try_get("description")?,
                body: r.try_get("body")?,
                updated_at: r.try_get("updated_at")?,
            })
        })
        .transpose()
    }

    /// Insert a skill and return its id. A name held by a live row is
    /// `NameTaken`; a name held by a trashed row is `NameInTrash` — the
    /// trashed skill is never overwritten: the admin restores it from the
    /// Trash or picks another name (story 15-2 review M1).
    pub async fn create(
        pool: &DbPool,
        skill: &ValidSkill,
        user_id: Option<u64>,
    ) -> Result<u64, SkillWriteError> {
        let inserted = sqlx::query(
            "INSERT INTO mcp_skills (name, description, body, active, updated_by) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&skill.name)
        .bind(&skill.description)
        .bind(&skill.body)
        .bind(skill.active)
        .bind(user_id)
        .execute(pool)
        .await;
        match inserted {
            Ok(res) => Ok(res.last_insert_id()),
            Err(err) if is_duplicate_key(&err) => Err(Self::name_holder(pool, &skill.name).await?),
            Err(err) => Err(err.into()),
        }
    }

    /// Story 17-6 — insert a default skill shipped with mybibli, inside the
    /// caller's transaction: no author, marked `provided_by_mybibli`.
    /// `Ok(false)` when any row, live or in the Trash, already holds the
    /// name: nothing is written and that row is left untouched. A plain
    /// insert whose duplicate-key error is the signal — no locking read on
    /// `mcp_skills` (Rule #22); in MariaDB a failed statement rolls back
    /// alone, the transaction goes on.
    pub async fn insert_default_in(
        conn: &mut MySqlConnection,
        skill: &ValidSkill,
    ) -> Result<bool, AppError> {
        let inserted = sqlx::query(
            "INSERT INTO mcp_skills \
             (name, description, body, active, updated_by, provided_by_mybibli) \
             VALUES (?, ?, ?, ?, NULL, TRUE)",
        )
        .bind(&skill.name)
        .bind(&skill.description)
        .bind(&skill.body)
        .bind(skill.active)
        .execute(&mut *conn)
        .await;
        match inserted {
            Ok(_) => Ok(true),
            Err(err) if is_duplicate_key(&err) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// After a duplicate-name error: which kind of row holds the name.
    async fn name_holder(pool: &DbPool, name: &str) -> Result<SkillWriteError, AppError> {
        let in_trash: Option<(u64,)> =
            sqlx::query_as("SELECT id FROM mcp_skills WHERE name = ? AND deleted_at IS NOT NULL")
                .bind(name)
                .fetch_optional(pool)
                .await?;
        Ok(if in_trash.is_some() {
            SkillWriteError::NameInTrash
        } else {
            SkillWriteError::NameTaken
        })
    }

    /// Replace a live skill's fields, guarded by `version`.
    pub async fn update(
        pool: &DbPool,
        id: u64,
        version: i32,
        skill: &ValidSkill,
        user_id: Option<u64>,
    ) -> Result<(), SkillWriteError> {
        let res = sqlx::query(
            "UPDATE mcp_skills SET name = ?, description = ?, body = ?, active = ?, \
             updated_by = ?, provided_by_mybibli = FALSE, version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(&skill.name)
        .bind(&skill.description)
        .bind(&skill.body)
        .bind(skill.active)
        .bind(user_id)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await;
        match res {
            Ok(r) if r.rows_affected() == 1 => Ok(()),
            Ok(_) => Err(Self::why_not_written(pool, id).await),
            Err(err) if is_duplicate_key(&err) => Err(Self::name_holder(pool, &skill.name).await?),
            Err(err) => Err(err.into()),
        }
    }

    /// Activate or deactivate a live skill, guarded by `version`. Counts
    /// as a change: author and date move.
    pub async fn set_active(
        pool: &DbPool,
        id: u64,
        version: i32,
        active: bool,
        user_id: Option<u64>,
    ) -> Result<(), SkillWriteError> {
        let res = sqlx::query(
            "UPDATE mcp_skills SET active = ?, updated_by = ?, provided_by_mybibli = FALSE, \
             version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(active)
        .bind(user_id)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
        if res.rows_affected() == 1 {
            Ok(())
        } else {
            Err(Self::why_not_written(pool, id).await)
        }
    }

    /// Move a live skill to the Trash, guarded by `version`. The version is
    /// left as is, like `SoftDeleteService`: the Trash's restore and
    /// permanent delete lock on the version the Trash panel shows.
    pub async fn soft_delete(pool: &DbPool, id: u64, version: i32) -> Result<(), SkillWriteError> {
        let res = sqlx::query(
            "UPDATE mcp_skills SET deleted_at = NOW() \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
        if res.rows_affected() == 1 {
            Ok(())
        } else {
            Err(Self::why_not_written(pool, id).await)
        }
    }

    /// After a guarded write touched no row: stale version or gone row.
    async fn why_not_written(pool: &DbPool, id: u64) -> SkillWriteError {
        match sqlx::query_scalar::<_, u64>(
            "SELECT id FROM mcp_skills WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        {
            Ok(Some(_)) => SkillWriteError::Stale,
            Ok(None) => SkillWriteError::Gone,
            Err(err) => SkillWriteError::Db(err.into()),
        }
    }
}
