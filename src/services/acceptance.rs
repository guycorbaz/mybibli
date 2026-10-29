//! Stories 11-17 / 11-18 (FR138, FR140, FR146) — what accepting any kind
//! of proposal shares.
//!
//! Each kind applies its proposal in **one transaction** run by
//! [`in_retried_transaction`]: the proposal row is locked and its
//! `version` compared with the one the librarian saw
//! ([`lock_pending_in`]), the targets are re-checked (version **and**
//! the catalog state a version does not record, #521), then either the
//! proposal is marked `stale` (an `Ok` outcome, committed) or the
//! decision is recorded and the change applied. Any error rolls the
//! transaction back: nothing applied, the proposal still `pending`. A
//! deadlock is retried on a fresh transaction (#16 pattern).
//!
//! - series kinds: [`crate::services::series_acceptance`] (11-17);
//! - `metadata`: [`crate::services::metadata_acceptance`] (11-18);
//! - `cover`: [`crate::services::cover_acceptance`] (11-18) — its staged
//!   file is promoted **after** the commit (no file I/O inside a
//!   decision transaction, D-11-9-12).
//!
//! [`accept`] dispatches on the proposal's kind.

use std::fmt;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::proposal::{ProposalItemRow, ProposalKind, ProposalModel, ProposalStatus};
use crate::services::cover_acceptance::{self, AppliedCover};
use crate::services::expected_tomes::ExpectedTomeError;
use crate::services::metadata_acceptance::{self, AppliedMetadata};
use crate::services::proposals::{
    Decision, ProposalError, ProposalService, StaleTarget, Transition,
};
use crate::services::series_acceptance::{self, AppliedSeries};

/// Why the catalog no longer admits a proposal (the proposal goes
/// stale). Names come from the catalog, for the librarian's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptConflict {
    /// An active series with the proposed name exists now.
    SeriesExists { name: String },
    /// A title of the proposal is already in the series.
    TitleInSeries { title: String, position: i32 },
    /// A live title holds a proposed position.
    PositionHeld { position: i32, holder: String },
    /// A live expected tome sits at a proposed position.
    TomePositionTaken { position: i32, tome_title: String },
    /// A live expected tome already carries a proposed tome's ISBN.
    TomeIsbnTaken { isbn: String, tome_title: String },
    /// A proposed position is above the series' closed total.
    BeyondTotal { position: i32, total: i32 },
    /// Story 11-18 — a title field no longer holds the value recorded at
    /// filing (`field` is a proposable field name).
    FieldChanged { field: String },
    /// Story 11-18 — a proposed contributor's role was deleted (the role
    /// name recorded at filing).
    RoleGone { role: String },
    /// Story 11-18 — the title's cover changed since filing.
    CoverChanged,
}

/// Why an accept went stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleCause {
    /// Targets changed (`version`) or were deleted since filing.
    Targets(Vec<StaleTarget>),
    /// The catalog changed in a way no `version` records (#521).
    Conflict(AcceptConflict),
}

/// Outcome of an accept that did not fail; `A` is what was applied.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptOutcome<A = AppliedSeries> {
    Accepted(A),
    /// Nothing applied; the proposal is now `stale` (commit the mark).
    Stale(StaleCause),
}

/// Why an accept was refused or failed (roll back: nothing applied,
/// the proposal stays as it was).
#[derive(Debug)]
pub enum AcceptError {
    NotFound,
    NotPending {
        status: ProposalStatus,
    },
    /// The proposal's `version` moved since the page was opened.
    Changed,
    /// The kind is not the one the caller handles.
    WrongKind {
        kind: ProposalKind,
    },
    /// Every item was removed (story 11-19).
    NothingToApply,
    UnknownDecider,
    /// The stored items cannot be applied as they are (never produced by
    /// filing; a librarian rejects such a proposal).
    Invalid(String),
    /// Story 11-18 (#533) — a cover proposal whose staged image is gone
    /// (or the staging area is unavailable): nothing can be installed.
    CoverMissing,
    /// A failure while applying.
    Apply(AppError),
}

impl fmt::Display for AcceptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AcceptError::NotFound => write!(f, "proposal not found"),
            AcceptError::NotPending { status } => write!(f, "proposal is {}", status.as_str()),
            AcceptError::Changed => write!(f, "proposal version moved"),
            AcceptError::WrongKind { kind } => {
                write!(f, "{} is not the kind being accepted", kind.as_str())
            }
            AcceptError::NothingToApply => write!(f, "nothing to apply"),
            AcceptError::UnknownDecider => write!(f, "unknown decider"),
            AcceptError::Invalid(why) => write!(f, "invalid proposal: {why}"),
            AcceptError::CoverMissing => write!(f, "the staged cover is missing"),
            AcceptError::Apply(e) => write!(f, "apply failed: {e}"),
        }
    }
}

impl From<AppError> for AcceptError {
    fn from(e: AppError) -> Self {
        AcceptError::Apply(e)
    }
}

impl From<sqlx::Error> for AcceptError {
    fn from(e: sqlx::Error) -> Self {
        AcceptError::Apply(AppError::from(e))
    }
}

impl From<ExpectedTomeError> for AcceptError {
    fn from(e: ExpectedTomeError) -> Self {
        AcceptError::Apply(AppError::from(e))
    }
}

impl From<ProposalError> for AcceptError {
    fn from(e: ProposalError) -> Self {
        match e {
            ProposalError::NotFound { .. } => AcceptError::NotFound,
            ProposalError::NotPending { status, .. } => AcceptError::NotPending { status },
            ProposalError::NothingToApply { .. } => AcceptError::NothingToApply,
            ProposalError::UnknownDecider { .. } => AcceptError::UnknownDecider,
            ProposalError::Database(inner) => AcceptError::Apply(inner),
            other => AcceptError::Apply(AppError::Internal(other.to_string())),
        }
    }
}

/// SQLSTATE 40001 — InnoDB picked this transaction as a deadlock victim.
pub(crate) fn is_deadlock(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some("40001"))
}

/// Attempts of an acceptance on a deadlock.
pub(crate) const ACCEPT_MAX_ATTEMPTS: usize = 3;

/// One attempt of an acceptance on a caller-held transaction.
pub(crate) type AcceptAttempt<'c, A> =
    Pin<Box<dyn Future<Output = Result<AcceptOutcome<A>, AcceptError>> + Send + 'c>>;

/// Run `attempt` in its own transaction: committed when the outcome is
/// `Accepted` or `Stale`, rolled back on any error. A deadlock
/// (concurrent scan-settle, hand edit, second acceptance) is retried on
/// a fresh transaction — the whole acceptance re-runs, checks included.
/// A lock-wait timeout is not retried: the blocker is likely still there
/// and each wait lasts `innodb_lock_wait_timeout` (D-11-17-11). `what`
/// names the kind and `user_id` the decider in the logs.
pub(crate) async fn in_retried_transaction<A, F>(
    pool: &DbPool,
    id: u64,
    user_id: u64,
    what: &'static str,
    mut attempt: F,
) -> Result<AcceptOutcome<A>, AcceptError>
where
    A: fmt::Debug,
    F: for<'c> FnMut(&'c mut MySqlConnection) -> AcceptAttempt<'c, A>,
{
    let mut n = 0;
    loop {
        n += 1;
        let may_retry = n < ACCEPT_MAX_ATTEMPTS;
        let mut tx = pool.begin().await?;
        match attempt(&mut tx).await {
            Ok(outcome) => {
                tx.commit().await?;
                match &outcome {
                    AcceptOutcome::Accepted(applied) => tracing::info!(
                        proposal_id = id,
                        user_id,
                        kind = what,
                        applied = ?applied,
                        "Proposal accepted and applied"
                    ),
                    AcceptOutcome::Stale(cause) => tracing::info!(
                        proposal_id = id,
                        user_id,
                        kind = what,
                        cause = ?cause,
                        "Proposal went stale at acceptance"
                    ),
                }
                return Ok(outcome);
            }
            Err(e) => {
                if let Err(rb) = tx.rollback().await {
                    tracing::warn!(error = %rb, "proposal acceptance rollback failed");
                }
                if let AcceptError::Apply(AppError::Database(d)) = &e
                    && is_deadlock(d)
                    && may_retry
                {
                    tracing::warn!(
                        proposal_id = id,
                        attempt = n,
                        error = %d,
                        "proposal acceptance hit a deadlock, retrying"
                    );
                    continue;
                }
                if matches!(e, AcceptError::Apply(_) | AcceptError::Invalid(_)) {
                    tracing::error!(proposal_id = id, kind = what, error = %e, "Proposal not applied");
                }
                return Err(e);
            }
        }
    }
}

/// The common start of an acceptance: lock the proposal row, check it is
/// pending, at the `version` the page showed and of one of `kinds`, then
/// read its live items with a share lock (none → `NothingToApply`).
pub(crate) async fn lock_pending_in(
    conn: &mut MySqlConnection,
    id: u64,
    version: i32,
    kinds: &[ProposalKind],
) -> Result<(ProposalKind, Vec<ProposalItemRow>), AcceptError> {
    let (kind, status, current) = ProposalModel::lock_for_decision(conn, id)
        .await?
        .ok_or(AcceptError::NotFound)?;
    if status != ProposalStatus::Pending {
        return Err(AcceptError::NotPending { status });
    }
    if current != version {
        return Err(AcceptError::Changed);
    }
    if !kinds.contains(&kind) {
        return Err(AcceptError::WrongKind { kind });
    }
    let items = ProposalModel::items(conn, id, true).await?;
    if items.is_empty() {
        return Err(AcceptError::NothingToApply);
    }
    Ok((kind, items))
}

/// Mark the proposal `stale` for `conflict` (the caller commits).
pub(crate) async fn mark_stale_in<A>(
    conn: &mut MySqlConnection,
    id: u64,
    conflict: AcceptConflict,
) -> Result<AcceptOutcome<A>, AcceptError> {
    match ProposalService::transition_in(conn, id, &Decision::MarkStale).await? {
        Transition::Decided(_) | Transition::WentStale(_) => {}
    }
    Ok(AcceptOutcome::Stale(StaleCause::Conflict(conflict)))
}

/// What an acceptance applied, whatever the kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    Series(AppliedSeries),
    Metadata(AppliedMetadata),
    Cover(AppliedCover),
}

fn wrap<A>(outcome: AcceptOutcome<A>, f: impl FnOnce(A) -> Applied) -> AcceptOutcome<Applied> {
    match outcome {
        AcceptOutcome::Accepted(a) => AcceptOutcome::Accepted(f(a)),
        AcceptOutcome::Stale(cause) => AcceptOutcome::Stale(cause),
    }
}

/// Where cover files live (a cover acceptance promotes its staged file).
#[derive(Debug, Clone, Copy)]
pub struct CoverDirs<'a> {
    pub covers_dir: &'a Path,
    /// `None` when the staging area is unavailable.
    pub staging_dir: Option<&'a Path>,
}

/// Accept proposal `id` on behalf of `user_id`, whatever its kind
/// (read first; each kind re-checks it under the proposal lock). The
/// kind comes back with the outcome, for the librarian's message.
pub async fn accept(
    pool: &DbPool,
    dirs: CoverDirs<'_>,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<(ProposalKind, AcceptOutcome<Applied>), AcceptError> {
    let kind = {
        let mut conn = pool.acquire().await?;
        ProposalModel::find(&mut conn, id)
            .await?
            .ok_or(AcceptError::NotFound)?
            .kind
    };
    let outcome = match kind {
        ProposalKind::SeriesCreate | ProposalKind::SeriesAttach => wrap(
            series_acceptance::accept(pool, id, user_id, version).await?,
            Applied::Series,
        ),
        ProposalKind::Metadata => wrap(
            metadata_acceptance::accept(pool, id, user_id, version).await?,
            Applied::Metadata,
        ),
        ProposalKind::Cover => wrap(
            cover_acceptance::accept(pool, dirs, id, user_id, version).await?,
            Applied::Cover,
        ),
    };
    Ok((kind, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_database_deadlocks_are_retried() {
        assert!(!is_deadlock(&sqlx::Error::RowNotFound));
        assert!(!is_deadlock(&sqlx::Error::PoolTimedOut));
        assert!(!is_deadlock(&sqlx::Error::Protocol("x".into())));
    }

    #[test]
    fn errors_name_the_case_without_internals() {
        assert_eq!(
            AcceptError::CoverMissing.to_string(),
            "the staged cover is missing"
        );
        assert!(
            AcceptError::WrongKind {
                kind: ProposalKind::Cover
            }
            .to_string()
            .starts_with("cover")
        );
        assert!(matches!(
            AcceptError::from(ProposalError::NothingToApply { id: 1 }),
            AcceptError::NothingToApply
        ));
    }
}
