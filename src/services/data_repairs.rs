//! Story 13-4 (#578), code review M1 — one-shot startup data repairs.
//!
//! A repair whose result cannot be told apart from healthy data (an HTML
//! description converted to plain text may legitimately still hold `<b>`
//! or `&lt;` as text) cannot be made idempotent by its selection predicate:
//! run again, it would rewrite healthy rows. [`run_once`] runs it at most
//! once per database, keyed by a name recorded in `data_repairs`.

use std::future::Future;
use std::pin::Pin;

use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::data_repair::DataRepairModel;

/// The future a repair returns, borrowing the transaction's connection.
pub type RepairFuture<'c, T> = Pin<Box<dyn Future<Output = Result<T, AppError>> + Send + 'c>>;

/// Run the repair `name` unless it already ran on this database; `None`
/// when it is skipped. The marker check, every write of the repair and the
/// marker itself share **one transaction** (review round 2, L2): a repair
/// that fails rolls back entirely and runs again on the next boot, and a
/// committed repair always has its marker.
pub async fn run_once<T, F>(pool: &DbPool, name: &str, repair: F) -> Result<Option<T>, AppError>
where
    F: for<'c> FnOnce(&'c mut MySqlConnection) -> RepairFuture<'c, T>,
{
    let mut tx = pool.begin().await?;
    if DataRepairModel::is_applied_in(&mut tx, name).await? {
        tx.rollback().await?;
        tracing::debug!(repair = name, "Data repair already applied; skipped");
        return Ok(None);
    }
    // On an error the transaction is dropped, i.e. rolled back: no marker.
    let outcome = repair(&mut tx).await?;
    DataRepairModel::mark_applied_in(&mut tx, name).await?;
    tx.commit().await?;
    tracing::info!(repair = name, "Data repair applied");
    Ok(Some(outcome))
}

/// Whether the repair `name` already ran (for a step to run outside the
/// transaction, before it). A plain read.
pub async fn is_applied(pool: &DbPool, name: &str) -> Result<bool, AppError> {
    DataRepairModel::is_applied(pool, name).await
}
