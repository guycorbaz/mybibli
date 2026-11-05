//! Story 15-4 (#590, FR164) — accepting a `series_update` proposal.
//!
//! Order, in one transaction (retried on deadlock only):
//!
//! 1. lock the proposal, then its items (`acceptance::lock_pending_in`);
//! 2. parse and re-validate the stored lines (malformed → 400, still
//!    pending);
//! 3. lock the **series row** (`FOR UPDATE`), then
//!    `ProposalService::check_staleness_in` — the series' `version`
//!    decides, as for the other series kinds;
//! 4. lock each **removed tome row** by id (`FOR UPDATE`, after the
//!    series: the order every tome writer follows) and re-read it: gone,
//!    moved to another series or at another `version` →
//!    [`AcceptConflict::TomeChanged`];
//! 5. re-check against the live catalogue: a new name now held by
//!    another live series → [`AcceptConflict::SeriesExists`]; the total
//!    rule on the live held positions and tomes →
//!    [`AcceptConflict::BelowPosition`] (stale, positions named);
//! 6. decide (`transition_in`), then apply: the proposed fields of the
//!    series (bumps its `version`) and the soft-delete of each removed
//!    tome.
//!
//! Locking reads on proposals start from PK lists or the target index
//! (`check_staleness_in`), the series and removed tomes are locked by id:
//! no proposal row and no other series row is locked (epic 12 retro A1;
//! lock-scope test in `tests/proposal_series_update.rs`). The
//! live re-check reads the series' held positions and tomes with share
//! locks (as the series_attach acceptance): in REPEATABLE READ that range
//! also next-key locks the first tome entry of the next series in
//! `uq_expected_tomes_position`, so a tome write on that neighbour waits
//! for this short transaction to commit — a wait, never a cycle (#627). Nothing is marked hand-set
//! (D2, module doc of `series_update_proposals`).

use serde_json::Value;
use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::models::expected_tome::ExpectedTomeModel;
use crate::models::proposal::{
    ProposalItemRow, ProposalKind, ProposalModel, ProposalStatus, ProposalTargetTable,
};
use crate::models::series::{SeriesModel, TitleSeriesModel};
use crate::services::acceptance::{
    AcceptConflict, AcceptError, AcceptOutcome, StaleCause, in_retried_transaction,
    lock_pending_in, mark_stale_in,
};
use crate::services::proposals::{Decision, ProposalService, Transition};
use crate::services::series_update_proposals::{
    RemovalLine, SeriesChange, SeriesFields, SeriesUpdateError, check_total_rule,
    parse_removal_line, parse_update_line, resolve_change,
};

/// The stored lines of a `series_update` proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSeriesUpdate {
    pub series_id: u64,
    /// The series as the proposal was checked against at filing.
    pub filed: SeriesFields,
    pub change: SeriesChange,
    pub removals: Vec<RemovalLine>,
}

/// Parse the items (pure): the update line first, targeting a series,
/// then removal lines; each tome once.
pub fn parse_items(items: &[ProposalItemRow]) -> Result<StoredSeriesUpdate, String> {
    let Some((first, rest)) = items.split_first() else {
        return Err("no items".into());
    };
    let series_id = first
        .target
        .filter(|t| t.table == ProposalTargetTable::Series)
        .map(|t| t.id)
        .ok_or_else(|| format!("item {}: the update line targets a series", first.id))?;
    let v: Value = serde_json::from_str(&first.proposed_values_json)
        .map_err(|e| format!("item {}: invalid JSON: {e}", first.id))?;
    let (filed, change) = parse_update_line(&v)
        .ok_or_else(|| format!("item {}: not a series update line", first.id))?;
    let mut removals: Vec<RemovalLine> = Vec::with_capacity(rest.len());
    for row in rest {
        if row.target.is_some() {
            return Err(format!("item {}: a removal line has no target", row.id));
        }
        let v: Value = serde_json::from_str(&row.proposed_values_json)
            .map_err(|e| format!("item {}: invalid JSON: {e}", row.id))?;
        let line = parse_removal_line(&v)
            .ok_or_else(|| format!("item {}: not a tome removal line", row.id))?;
        if removals.iter().any(|r| r.tome_id == line.tome_id) {
            return Err(format!("item {}: tome {} listed twice", row.id, line.tome_id));
        }
        removals.push(line);
    }
    Ok(StoredSeriesUpdate {
        series_id,
        filed,
        change,
        removals,
    })
}

/// What an accepted `series_update` changed, for the librarian's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedSeriesUpdate {
    pub series_id: u64,
    /// The series' name after the change.
    pub series_name: String,
    pub renamed: bool,
    /// The total after the change (`None` = open), when type or total
    /// changed.
    pub type_total: Option<Option<i32>>,
    pub removed: usize,
}

impl AppliedSeriesUpdate {
    pub fn href(&self) -> String {
        format!("/series/{}", self.series_id)
    }
}

pub async fn accept(
    pool: &DbPool,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome<AppliedSeriesUpdate>, AcceptError> {
    in_retried_transaction(pool, id, user_id, "series_update", |conn| {
        Box::pin(accept_in(conn, id, user_id, version))
    })
    .await
}

/// [`accept`] inside the caller's transaction (public for the lock-scope
/// regression test).
pub async fn accept_in(
    conn: &mut MySqlConnection,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome<AppliedSeriesUpdate>, AcceptError> {
    let (_, items) = lock_pending_in(conn, id, version, &[ProposalKind::SeriesUpdate]).await?;
    let stored = parse_items(&items).map_err(AcceptError::Invalid)?;
    // Re-validate the stored change against the filed series: a line
    // edited by hand went through the same rules, a corrupt one is
    // refused (still pending).
    let target = stored.change.apply(&stored.filed);
    let touches = stored.change.touches_total();
    let revalidated = resolve_change(
        &stored.filed,
        stored.change.name.as_deref(),
        touches.then_some(target.series_type),
        target.total.filter(|_| touches).map(i64::from),
    )
    .map_err(|e: SeriesUpdateError| AcceptError::Invalid(e.to_string()))?;
    if revalidated != stored.change {
        return Err(AcceptError::Invalid("inconsistent series change".into()));
    }
    if stored.change.is_empty() && stored.removals.is_empty() {
        return Err(AcceptError::Invalid("nothing to change".into()));
    }

    // Series row first, then the staleness check (the series' version).
    let shape = ExpectedTomeModel::lock_series_in(conn, stored.series_id).await?;
    let check = ProposalService::check_staleness_in(conn, id).await?;
    if check.status != ProposalStatus::Pending {
        return Ok(AcceptOutcome::Stale(StaleCause::Targets(check.stale_targets)));
    }
    let Some(shape) = shape else {
        // Unreachable while the update line targets the series: a deleted
        // series makes `check_staleness_in` mark the proposal stale above.
        return Err(AcceptError::Invalid("series not found".into()));
    };

    // Then the removed tomes, by id.
    for r in &stored.removals {
        let tome = ExpectedTomeModel::find_in(conn, r.tome_id, true).await?;
        let unchanged = tome
            .as_ref()
            .is_some_and(|t| t.series_id == stored.series_id && t.version == r.tome_version);
        if !unchanged {
            return mark_stale_in(
                conn,
                id,
                AcceptConflict::TomeChanged {
                    position: r.position,
                    tome_title: r.title.clone(),
                },
            )
            .await;
        }
    }

    // Live re-checks.
    let current = SeriesFields::from_shape(&shape);
    if stored.change.name.is_some()
        && let Some((_, other_name)) =
            ProposalModel::other_series_named(conn, &target.name, stored.series_id).await?
    {
        return mark_stale_in(conn, id, AcceptConflict::SeriesExists { name: other_name }).await;
    }
    let held: Vec<i32> = TitleSeriesModel::held_positions_in(conn, stored.series_id)
        .await?
        .iter()
        .map(|h| h.position)
        .collect();
    let tome_positions: Vec<(u64, i32)> = ExpectedTomeModel::live_slots_in(conn, stored.series_id)
        .await?
        .iter()
        .map(|t| (t.id, t.position))
        .collect();
    let removed: Vec<u64> = stored.removals.iter().map(|r| r.tome_id).collect();
    if let Err(SeriesUpdateError::TotalBelowPositions { total, positions }) = check_total_rule(
        &current,
        &stored.change,
        &held,
        &tome_positions,
        &removed,
    ) {
        return mark_stale_in(
            conn,
            id,
            AcceptConflict::BelowPosition {
                total,
                owned: positions.owned,
                expected: positions.expected,
            },
        )
        .await;
    }

    match ProposalService::transition_in(conn, id, &Decision::Accept { by: user_id }).await? {
        Transition::Decided(_) => {}
        Transition::WentStale(targets) => {
            return Ok(AcceptOutcome::Stale(StaleCause::Targets(targets)));
        }
    }
    if !stored.change.is_empty() {
        SeriesModel::apply_update_in(
            conn,
            stored.series_id,
            &target.name,
            target.series_type,
            target.total,
        )
        .await?;
    }
    for r in &stored.removals {
        // Locked and version-checked above: anything but one row is a bug,
        // rolled back rather than half applied.
        if ExpectedTomeModel::soft_delete_in(conn, r.tome_id, r.tome_version).await? != 1 {
            return Err(AcceptError::Invalid(format!(
                "expected tome {} could not be removed",
                r.tome_id
            )));
        }
    }
    Ok(AcceptOutcome::Accepted(AppliedSeriesUpdate {
        series_id: stored.series_id,
        series_name: target.name,
        renamed: stored.change.name.is_some(),
        type_total: stored.change.touches_total().then_some(target.total),
        removed: stored.removals.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::proposal::ProposalTarget;
    use crate::models::series::SeriesType;
    use serde_json::json;

    fn row(id: u64, target: Option<ProposalTargetTable>, v: Value) -> ProposalItemRow {
        ProposalItemRow {
            id,
            proposal_id: 1,
            target: target.map(|table| ProposalTarget { table, id: 4 }),
            target_version: target.map(|_| 1),
            proposed_values_json: v.to_string(),
            version: 1,
        }
    }

    fn update() -> Value {
        json!({"line": "series_update",
               "current": {"name": "Tintin", "series_type": "open", "total_volume_count": null},
               "series_type": "closed", "total_volume_count": 24, "fingerprint": "x"})
    }

    fn removal(tome_id: u64) -> Value {
        json!({"line": "tome_removal", "tome_id": tome_id, "tome_version": 2,
               "position": 25, "title": "Tome 25"})
    }

    #[test]
    fn parses_the_update_and_removal_lines() {
        let p = parse_items(&[
            row(1, Some(ProposalTargetTable::Series), update()),
            row(2, None, removal(9)),
        ])
        .unwrap();
        assert_eq!(p.series_id, 4);
        assert_eq!(p.change.total, Some(Some(24)));
        assert_eq!(p.change.series_type, Some(SeriesType::Closed));
        assert_eq!(p.removals.len(), 1);
        assert_eq!(p.removals[0].tome_version, 2);
    }

    #[test]
    fn refuses_bad_shapes() {
        let cases: Vec<Vec<ProposalItemRow>> = vec![
            vec![],
            vec![row(1, None, update())],
            vec![row(1, Some(ProposalTargetTable::Titles), update())],
            vec![row(1, Some(ProposalTargetTable::Series), removal(9))],
            vec![
                row(1, Some(ProposalTargetTable::Series), update()),
                row(2, Some(ProposalTargetTable::Series), removal(9)),
            ],
            vec![
                row(1, Some(ProposalTargetTable::Series), update()),
                row(2, None, removal(9)),
                row(3, None, removal(9)),
            ],
            vec![
                row(1, Some(ProposalTargetTable::Series), update()),
                row(2, None, json!({"line": "tome_removal"})),
            ],
        ];
        for items in cases {
            assert!(parse_items(&items).is_err(), "{items:?}");
        }
    }
}
