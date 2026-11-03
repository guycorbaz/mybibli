//! Story 13-9 (#581, FR156) — accepting a `label_attach` proposal.
//!
//! The item (shape: module doc of [`crate::services::label_proposals`])
//! names a label and the title or volume it marks. Acceptance runs in
//! **one transaction** (shared steps in [`crate::services::acceptance`]):
//!
//! 1. lock the proposal row, check its `version`, share-lock its items;
//! 2. [`ProposalService::check_staleness_in`] — the target row is locked
//!    (`FOR UPDATE`); a deleted title or volume marks the proposal
//!    `stale` (a moved `version` does not count, `version_counts`);
//!    A volume whose title was deleted counts as deleted too;
//! 3. the label row is share-locked — a deleted label marks the proposal
//!    `stale` ([`AcceptConflict::LabelGone`]);
//! 4. a label already on the record marks the proposal `stale`
//!    ([`AcceptConflict::LabelAttached`]), the convention of the other
//!    kinds for a change already made (a title already in the series,
//!    #521). A plain read: a hand attach committed in the instant between
//!    the transaction's snapshot and the target lock is not seen, and the
//!    upsert below absorbs it (the proposal is then accepted, nothing
//!    added). Conversely a hand *detach* committed in that window is not
//!    seen either: the proposal goes stale although the label is gone —
//!    accepted (single librarian; the agent may propose again);
//! 5. the decision is recorded and the label attached
//!    ([`LabelModel::attach_in`], an upsert).
//!
//! No `version` or identity staleness (`proposals::version_counts`): a
//! record corrected by hand after filing can still be labelled — the
//! librarian reads the agent's note and sources and judges.
//!
//! Lock scope (epic 12 retro A1): every lock above is taken by primary key
//! — the proposal, its items, one title or volume, one label — plus the
//! upsert's insert-intention / record lock on one link; never a status
//! range or a locking read on a range, so an acceptance on one record
//! never blocks a decision on another.

use serde_json::Value;
use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::models::label::{LabelModel, LabelTarget};
use crate::models::proposal::{ProposalItemRow, ProposalKind, ProposalModel, ProposalStatus};
use crate::services::acceptance::{
    AcceptConflict, AcceptError, AcceptOutcome, StaleCause, in_retried_transaction,
    lock_pending_in, mark_stale_in,
};
use crate::services::label_proposals::label_target;
use crate::services::proposals::{Decision, ProposalService, StaleReason, StaleTarget, Transition};

/// A stored label proposal, parsed back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLabel {
    pub target: LabelTarget,
    pub label_id: u64,
    /// The label's name at filing.
    pub label_name: String,
}

/// Parse the stored items of a label proposal (pure — unit tested):
/// exactly one `"line": "label"` item targeting a title or a volume, with
/// a label id.
pub fn parse_items(items: &[ProposalItemRow]) -> Result<StoredLabel, String> {
    let [row] = items else {
        return Err(format!("{} items, one expected", items.len()));
    };
    let target = row
        .target
        .and_then(label_target)
        .ok_or_else(|| format!("item {}: a label targets a title or a volume", row.id))?;
    let v: Value = serde_json::from_str(&row.proposed_values_json)
        .map_err(|e| format!("item {}: invalid JSON: {e}", row.id))?;
    if v.get("line").and_then(Value::as_str) != Some("label") {
        return Err(format!("item {}: not a label line", row.id));
    }
    let label_id = v
        .get("label_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("item {}: no label id", row.id))?;
    Ok(StoredLabel {
        target,
        label_id,
        label_name: v
            .get("label_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// What a label acceptance applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedLabel {
    pub target: LabelTarget,
    /// The label's live name.
    pub label_name: String,
    /// How the record is named: the title, or "V0042 — title".
    pub target_name: String,
}

impl AppliedLabel {
    /// The page of the labelled record.
    pub fn href(&self) -> String {
        match self.target {
            LabelTarget::Title(id) => format!("/title/{id}"),
            LabelTarget::Volume(id) => format!("/volume/{id}"),
        }
    }
}

/// Accept label proposal `id` on behalf of `user_id`.
pub async fn accept(
    pool: &DbPool,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome<AppliedLabel>, AcceptError> {
    in_retried_transaction(pool, id, user_id, "label_attach", |conn| {
        Box::pin(accept_in(conn, id, user_id, version))
    })
    .await
}

/// [`accept`] on the caller's transaction (the caller commits on `Ok`,
/// rolls back on `Err`). Public for the lock-scope regression test.
pub async fn accept_in(
    conn: &mut MySqlConnection,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome<AppliedLabel>, AcceptError> {
    let (_, items) = lock_pending_in(conn, id, version, &[ProposalKind::LabelAttach]).await?;
    let stored = parse_items(&items).map_err(AcceptError::Invalid)?;
    let check = ProposalService::check_staleness_in(conn, id).await?;
    if check.status != ProposalStatus::Pending {
        return Ok(AcceptOutcome::Stale(StaleCause::Targets(
            check.stale_targets,
        )));
    }
    // Review round 1 (L4): a live volume whose title was deleted is gone
    // too (it shows nowhere in the catalog).
    if let LabelTarget::Volume(vid) = stored.target
        && !ProposalModel::live_volume_briefs_in(conn, &[vid])
            .await?
            .contains_key(&vid)
    {
        match ProposalService::transition_in(conn, id, &Decision::MarkStale).await? {
            Transition::Decided(_) | Transition::WentStale(_) => {}
        }
        let targets = items
            .iter()
            .filter_map(|i| {
                i.target.map(|target| StaleTarget {
                    item_id: i.id,
                    target,
                    reason: StaleReason::Deleted,
                })
            })
            .collect();
        return Ok(AcceptOutcome::Stale(StaleCause::Targets(targets)));
    }
    let Some(label) = LabelModel::find_by_id_in(conn, stored.label_id, true).await? else {
        return mark_stale_in(
            conn,
            id,
            AcceptConflict::LabelGone {
                label: stored.label_name,
            },
        )
        .await;
    };
    if LabelModel::is_attached_in(conn, stored.target, label.id).await? {
        return mark_stale_in(
            conn,
            id,
            AcceptConflict::LabelAttached { label: label.name },
        )
        .await;
    }
    let target_name = target_name(conn, stored.target).await?;
    match ProposalService::transition_in(conn, id, &Decision::Accept { by: user_id }).await? {
        Transition::Decided(_) => {}
        // Unreachable: the same locks are held since the check above.
        Transition::WentStale(targets) => {
            return Ok(AcceptOutcome::Stale(StaleCause::Targets(targets)));
        }
    }
    LabelModel::attach_in(conn, stored.target, label.id).await?;
    Ok(AcceptOutcome::Accepted(AppliedLabel {
        target: stored.target,
        label_name: label.name,
        target_name,
    }))
}

/// How the librarian's message names the record (plain read; the row is
/// locked since the staleness check).
async fn target_name(
    conn: &mut MySqlConnection,
    target: LabelTarget,
) -> Result<String, AcceptError> {
    Ok(match target {
        LabelTarget::Title(id) => crate::models::title::TitleModel::find_by_id_in(conn, id, false)
            .await?
            .map(|t| t.title)
            .unwrap_or_default(),
        LabelTarget::Volume(id) => ProposalModel::live_volume_briefs_in(conn, &[id])
            .await?
            .remove(&id)
            .map(|v| format!("{} — {}", v.code, v.title))
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::proposal::{ProposalTarget, ProposalTargetTable};
    use serde_json::json;

    fn row(id: u64, table: ProposalTargetTable, v: Value) -> ProposalItemRow {
        ProposalItemRow {
            id,
            proposal_id: 1,
            target: Some(ProposalTarget { table, id: 4 }),
            target_version: Some(1),
            proposed_values_json: v.to_string(),
            version: 1,
        }
    }

    #[test]
    fn parses_the_label_item() {
        let ok =
            json!({"line": "label", "label_id": 3, "label_name": "Code douteux", "note": null});
        let p = parse_items(&[row(1, ProposalTargetTable::Volumes, ok.clone())]).unwrap();
        assert_eq!(
            p,
            StoredLabel {
                target: LabelTarget::Volume(4),
                label_id: 3,
                label_name: "Code douteux".into()
            }
        );
        let t = parse_items(&[row(1, ProposalTargetTable::Titles, ok)]).unwrap();
        assert_eq!(t.target, LabelTarget::Title(4));
    }

    #[test]
    fn refuses_bad_shapes() {
        let ok = json!({"line": "label", "label_id": 3});
        let cases: Vec<Vec<ProposalItemRow>> = vec![
            vec![],
            vec![
                row(1, ProposalTargetTable::Titles, ok.clone()),
                row(2, ProposalTargetTable::Titles, ok.clone()),
            ],
            vec![row(1, ProposalTargetTable::Series, ok.clone())],
            vec![row(
                1,
                ProposalTargetTable::Titles,
                json!({"line": "cover"}),
            )],
            vec![row(
                1,
                ProposalTargetTable::Titles,
                json!({"line": "label"}),
            )],
            vec![row(
                1,
                ProposalTargetTable::Titles,
                json!({"line": "label", "label_id": "3"}),
            )],
            vec![ProposalItemRow {
                target: None,
                ..row(1, ProposalTargetTable::Titles, ok.clone())
            }],
        ];
        for (i, items) in cases.iter().enumerate() {
            assert!(parse_items(items).is_err(), "case {i}");
        }
    }

    #[test]
    fn applied_links_to_the_record() {
        let a = AppliedLabel {
            target: LabelTarget::Volume(9),
            label_name: "x".into(),
            target_name: "V0009 — T".into(),
        };
        assert_eq!(a.href(), "/volume/9");
        let t = AppliedLabel {
            target: LabelTarget::Title(2),
            ..a
        };
        assert_eq!(t.href(), "/title/2");
    }
}
