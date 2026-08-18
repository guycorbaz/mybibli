//! CR #443 — management labels applied to titles and volumes.
//!
//! Follows the story 8-4 reference-data shape (list / find / create / rename /
//! soft-delete / usage-count, transparent reactivation on name reuse) with one
//! structural difference that runs through every method here: a label is
//! many-to-many over TWO entity kinds from one shared vocabulary.
//!
//! The consequence worth stating up front, because getting it wrong is silent:
//! **usage is the sum of both join tables**. A label attached only to volumes
//! must still refuse deletion. A count that looked at `title_labels` alone
//! would happily delete a label a librarian is actively using on their shelves.

use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{CONFLICT_NAME_TAKEN, CreateOutcome, DeleteOutcome};
use crate::services::locking::check_update_result;

#[derive(Debug, Clone)]
pub struct LabelModel {
    pub id: u64,
    pub name: String,
    pub color: Option<String>,
    pub version: i32,
}

/// Usage of one label, split by entity kind.
///
/// Kept split rather than summed because the two numbers answer different
/// questions: the total gates deletion, while the breakdown is what the
/// `/labels` page shows ("À vérifier — 3 titres, 7 volumes"). Summing early
/// would force the page to query again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LabelUsage {
    pub titles: i64,
    pub volumes: i64,
}

impl LabelUsage {
    pub fn total(&self) -> i64 {
        self.titles + self.volumes
    }

    pub fn is_unused(&self) -> bool {
        self.total() == 0
    }
}

impl std::fmt::Display for LabelModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl LabelModel {
    fn from_row(r: &sqlx::mysql::MySqlRow) -> Result<LabelModel, AppError> {
        Ok(LabelModel {
            id: r.try_get("id")?,
            name: r.try_get("name")?,
            color: r.try_get("color")?,
            version: r.try_get("version")?,
        })
    }

    pub async fn list_all(pool: &DbPool) -> Result<Vec<LabelModel>, AppError> {
        let rows = sqlx::query(
            "SELECT id, name, color, version FROM labels WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(Self::from_row).collect()
    }

    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<LabelModel>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, color, version FROM labels WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        match row {
            Some(r) => Ok(Some(Self::from_row(&r)?)),
            None => Ok(None),
        }
    }

    /// Insert a label. On UNIQUE collision with a soft-deleted row, reactivate
    /// it and report `Reactivated` — same contract as the other four
    /// taxonomies, so an admin who re-creates a deleted label gets it back
    /// rather than an error they cannot act on.
    pub async fn create(
        pool: &DbPool,
        name: &str,
        color: Option<&str>,
    ) -> Result<CreateOutcome, AppError> {
        match sqlx::query("INSERT INTO labels (name, color) VALUES (?, ?)")
            .bind(name)
            .bind(color)
            .execute(pool)
            .await
        {
            Ok(res) => Ok(CreateOutcome::Created(res.last_insert_id())),
            Err(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("23000") => {
                let existing: Option<(u64, i32)> = sqlx::query_as(
                    "SELECT id, version FROM labels WHERE name = ? AND deleted_at IS NOT NULL LIMIT 1",
                )
                .bind(name)
                .fetch_optional(pool)
                .await?;

                match existing {
                    Some((id, version)) => {
                        // Restore the colour too: reactivating with the old
                        // colour would silently ignore what the admin just
                        // typed in the create form.
                        let res = sqlx::query(
                            "UPDATE labels SET deleted_at = NULL, color = ?, version = version + 1 \
                             WHERE id = ? AND version = ?",
                        )
                        .bind(color)
                        .bind(id)
                        .bind(version)
                        .execute(pool)
                        .await?;
                        if res.rows_affected() == 1 {
                            Ok(CreateOutcome::Reactivated(id))
                        } else {
                            Err(AppError::Conflict(CONFLICT_NAME_TAKEN.to_string()))
                        }
                    }
                    None => Err(AppError::Conflict(CONFLICT_NAME_TAKEN.to_string())),
                }
            }
            Err(other) => Err(AppError::from(other)),
        }
    }

    pub async fn rename(
        pool: &DbPool,
        id: u64,
        version: i32,
        new_name: &str,
        new_color: Option<&str>,
    ) -> Result<(), AppError> {
        let res = sqlx::query(
            "UPDATE labels SET name = ?, color = ?, version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(new_name)
        .bind(new_color)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await
        .map_err(|err| match &err {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23000") => {
                AppError::Conflict(CONFLICT_NAME_TAKEN.to_string())
            }
            _ => AppError::from(err),
        })?;
        check_update_result(res.rows_affected(), "label")
    }

    /// Usage across BOTH join tables. Soft-deleted links and soft-deleted
    /// parents do not count: a label attached only to trashed titles is free
    /// to delete, matching story 8-4 AC#4 for the other taxonomies.
    pub async fn count_usage(pool: &DbPool, id: u64) -> Result<LabelUsage, AppError> {
        let titles: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM title_labels tl \
             JOIN titles t ON t.id = tl.title_id AND t.deleted_at IS NULL \
             WHERE tl.label_id = ? AND tl.deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        let volumes: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM volume_labels vl \
             JOIN volumes v ON v.id = vl.volume_id AND v.deleted_at IS NULL \
             WHERE vl.label_id = ? AND vl.deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(LabelUsage {
            titles: titles.0,
            volumes: volumes.0,
        })
    }

    /// Atomic count-and-delete, mirroring `GenreModel::delete_if_unused`:
    /// `SELECT … FOR UPDATE` locks the label row so a concurrent attach blocks
    /// on the FK lookup until commit, closing the TOCTOU window a
    /// count-then-delete pair would leave open. Both join tables are counted
    /// inside the same transaction.
    pub async fn delete_if_unused(
        pool: &DbPool,
        id: u64,
        version: i32,
    ) -> Result<DeleteOutcome, AppError> {
        let mut tx = pool.begin().await?;

        let locked = sqlx::query(
            "SELECT id FROM labels WHERE id = ? AND version = ? AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(id)
        .bind(version)
        .fetch_optional(&mut *tx)
        .await?;
        if locked.is_none() {
            tx.rollback().await?;
            return Err(AppError::Conflict(
                rust_i18n::t!("error.conflict", entity = "label").to_string(),
            ));
        }

        let titles: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM title_labels tl \
             JOIN titles t ON t.id = tl.title_id AND t.deleted_at IS NULL \
             WHERE tl.label_id = ? AND tl.deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        let volumes: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM volume_labels vl \
             JOIN volumes v ON v.id = vl.volume_id AND v.deleted_at IS NULL \
             WHERE vl.label_id = ? AND vl.deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        let usage = LabelUsage {
            titles: titles.0,
            volumes: volumes.0,
        };
        if !usage.is_unused() {
            tx.rollback().await?;
            return Ok(DeleteOutcome::InUse(usage.total()));
        }

        let res = sqlx::query(
            "UPDATE labels SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(&mut *tx)
        .await?;
        check_update_result(res.rows_affected(), "label")?;

        tx.commit().await?;
        Ok(DeleteOutcome::Deleted)
    }
}

/// Which entity a label is being attached to.
///
/// The two join tables are structurally identical, so the alternative was
/// four near-duplicate methods. This keeps one implementation and makes the
/// asymmetry impossible: every attach/detach/list path handles both kinds or
/// none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelTarget {
    Title(u64),
    Volume(u64),
}

impl LabelTarget {
    fn table(self) -> &'static str {
        match self {
            LabelTarget::Title(_) => "title_labels",
            LabelTarget::Volume(_) => "volume_labels",
        }
    }

    fn fk_column(self) -> &'static str {
        match self {
            LabelTarget::Title(_) => "title_id",
            LabelTarget::Volume(_) => "volume_id",
        }
    }

    fn id(self) -> u64 {
        match self {
            LabelTarget::Title(id) | LabelTarget::Volume(id) => id,
        }
    }
}

impl LabelModel {
    /// Attach a label, or bring back a previously detached link.
    ///
    /// Detaching soft-deletes the row, and the composite UNIQUE covers
    /// soft-deleted rows too — so a plain INSERT on re-attach hits a
    /// constraint violation the user cannot act on ("already attached", when
    /// visibly it is not). Same reactivation contract as `create`.
    ///
    /// Idempotent on an already-live link: re-attaching what is already
    /// attached is a no-op rather than an error, because the UI can emit a
    /// duplicate request on a double click and the user's intent is satisfied
    /// either way.
    pub async fn attach(pool: &DbPool, target: LabelTarget, label_id: u64) -> Result<(), AppError> {
        let table = target.table();
        let fk = target.fk_column();

        // Select the nullity as a boolean rather than the TIMESTAMP itself:
        // per CLAUDE.md, a TIMESTAMP column read through a dynamic query needs
        // `CAST(col AS DATETIME)` or SQLx rejects it, and the date is not
        // wanted here — only whether the link is currently detached.
        let existing: Option<(u64, bool)> = sqlx::query_as(&format!(
            "SELECT id, (deleted_at IS NOT NULL) AS is_detached \
             FROM {table} WHERE {fk} = ? AND label_id = ? LIMIT 1"
        ))
        .bind(target.id())
        .bind(label_id)
        .fetch_optional(pool)
        .await?;

        match existing {
            Some((_, false)) => Ok(()), // already attached
            Some((link_id, true)) => {
                sqlx::query(&format!(
                    "UPDATE {table} SET deleted_at = NULL, version = version + 1 WHERE id = ?"
                ))
                .bind(link_id)
                .execute(pool)
                .await?;
                Ok(())
            }
            None => {
                sqlx::query(&format!(
                    "INSERT INTO {table} ({fk}, label_id) VALUES (?, ?)"
                ))
                .bind(target.id())
                .bind(label_id)
                .execute(pool)
                .await?;
                Ok(())
            }
        }
    }

    /// Detach by soft-deleting the link. Silent when nothing was attached:
    /// the caller asked for the label to be gone, and it is.
    pub async fn detach(pool: &DbPool, target: LabelTarget, label_id: u64) -> Result<(), AppError> {
        let table = target.table();
        let fk = target.fk_column();
        sqlx::query(&format!(
            "UPDATE {table} SET deleted_at = NOW(), version = version + 1 \
             WHERE {fk} = ? AND label_id = ? AND deleted_at IS NULL"
        ))
        .bind(target.id())
        .bind(label_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Labels currently on one entity, alphabetical.
    pub async fn list_for(pool: &DbPool, target: LabelTarget) -> Result<Vec<LabelModel>, AppError> {
        let table = target.table();
        let fk = target.fk_column();
        let rows = sqlx::query(&format!(
            "SELECT l.id, l.name, l.color, l.version FROM {table} j \
             JOIN labels l ON l.id = j.label_id AND l.deleted_at IS NULL \
             WHERE j.{fk} = ? AND j.deleted_at IS NULL ORDER BY l.name"
        ))
        .bind(target.id())
        .fetch_all(pool)
        .await?;
        rows.iter().map(Self::from_row).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_totals_both_entity_kinds() {
        let u = LabelUsage {
            titles: 3,
            volumes: 7,
        };
        assert_eq!(u.total(), 10);
        assert!(!u.is_unused());
    }

    #[test]
    fn a_label_used_only_on_volumes_is_not_unused() {
        // The mistake this whole type exists to prevent: counting titles
        // alone would report 0 and let the delete through, silently removing
        // a label from every volume carrying it.
        let u = LabelUsage {
            titles: 0,
            volumes: 4,
        };
        assert!(!u.is_unused(), "volumes-only usage must still block deletion");
        assert_eq!(u.total(), 4);
    }

    #[test]
    fn a_label_used_only_on_titles_is_not_unused() {
        let u = LabelUsage {
            titles: 2,
            volumes: 0,
        };
        assert!(!u.is_unused());
    }

    #[test]
    fn an_unused_label_is_deletable() {
        assert!(LabelUsage::default().is_unused());
        assert_eq!(LabelUsage::default().total(), 0);
    }

    #[test]
    fn label_displays_as_its_name() {
        let l = LabelModel {
            id: 1,
            name: "À vérifier".to_string(),
            color: Some("amber".to_string()),
            version: 1,
        };
        assert_eq!(l.to_string(), "À vérifier");
    }
}
