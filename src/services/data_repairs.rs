//! Story 13-4 (#578), code review M1 — one-shot startup data repairs.
//!
//! A repair whose result cannot be told apart from healthy data (an HTML
//! description converted to plain text may legitimately still hold `<b>`
//! or `&lt;` as text) cannot be made idempotent by its selection predicate:
//! run again, it would rewrite healthy rows. [`run_once`] runs it at most
//! once per database, keyed by a name recorded in `data_repairs`.

use std::future::Future;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::data_repair::DataRepairModel;

/// Run the repair `name` unless it already ran on this database; `None`
/// when it is skipped. The marker is written after the repair returned
/// `Ok`, so a repair that fails as a whole runs again on the next boot. A
/// repair that logs and skips individual rows still counts as done: those
/// rows are not retried (they would need a hand fix, see the repair's log).
pub async fn run_once<T, F, Fut>(
    pool: &DbPool,
    name: &str,
    repair: F,
) -> Result<Option<T>, AppError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    if DataRepairModel::is_applied(pool, name).await? {
        tracing::debug!(repair = name, "Data repair already applied; skipped");
        return Ok(None);
    }
    let outcome = repair().await?;
    DataRepairModel::mark_applied(pool, name).await?;
    tracing::info!(repair = name, "Data repair applied");
    Ok(Some(outcome))
}
