//! Story 15-5 (#564, FR165) — accepting a `volume_value` proposal.
//!
//! The items (shapes: module doc of
//! [`crate::services::volume_value_proposals`]) propose a volume's current
//! value and, optionally, its list price as the purchase price. Acceptance
//! runs in **one transaction** (shared steps in
//! [`crate::services::acceptance`]):
//!
//! 1. lock the proposal row, check its `version`, share-lock its items;
//! 2. [`ProposalService::check_staleness_in`] — the volume row is locked
//!    (`FOR UPDATE`); a deleted volume marks the proposal `stale` (a moved
//!    `version` does not count, `version_counts`); a volume whose title was
//!    deleted counts as deleted too;
//! 3. per field (D7): the live current value differs from the one filed →
//!    stale ([`AcceptConflict::ValueChanged`]); a list-price line while a
//!    purchase price was set meanwhile → stale
//!    ([`AcceptConflict::PurchasePriceSet`]); the default currency changed
//!    since filing → stale ([`AcceptConflict::CurrencyChanged`], D3 as
//!    amended in review: amounts are only ever written in the library's
//!    currency);
//! 4. the decision is recorded and the amounts written
//!    ([`VolumeValueModel::apply_in`]: `current_value_updated_at = NOW()`,
//!    `version` bumped).
//!
//! Nothing is marked hand-set (D1): no automatic writer of volume amounts
//! exists. After the commit an `admin_audit` row `volume_value_update`
//! records old and new amounts, like the hand edit (best effort).
//!
//! Lock scope (epic 12 retro A1): every lock is taken by primary key — the
//! proposal, its items, one volume (its title is checked with a plain
//! read) — never a status range, so an
//! acceptance on one volume never blocks a decision on another.

use serde_json::{Value, json};
use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::models::proposal::{
    ProposalItemRow, ProposalKind, ProposalModel, ProposalStatus, ProposalTargetTable,
};
use crate::models::volume_value::{Amount, VolumeValueModel};
use crate::services::acceptance::{
    AcceptConflict, AcceptError, AcceptOutcome, StaleCause, in_retried_transaction,
    lock_pending_in, mark_stale_in,
};
use crate::services::proposals::{Decision, ProposalService, StaleReason, StaleTarget, Transition};
use crate::services::volume_value_proposals::{
    ValueLine, parse_default_currency, parse_list_price_line, parse_value_line, same_value,
};

/// A stored value proposal, parsed back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredVolumeValue {
    pub volume_id: u64,
    pub value: ValueLine,
    /// The list-price line, unless absent or removed by the librarian.
    pub list_price: Option<Amount>,
}

/// Parse the stored items of a value proposal (pure — unit tested): one
/// current-value line and at most one list-price line, all targeting the
/// same volume.
pub fn parse_items(items: &[ProposalItemRow]) -> Result<StoredVolumeValue, String> {
    let mut volume_id = None;
    let mut value = None;
    let mut list_price = None;
    for row in items {
        let target = row
            .target
            .filter(|t| t.table == ProposalTargetTable::Volumes)
            .ok_or_else(|| format!("item {}: a value line targets a volume", row.id))?;
        if *volume_id.get_or_insert(target.id) != target.id {
            return Err(format!("item {}: another volume", row.id));
        }
        let v: Value = serde_json::from_str(&row.proposed_values_json)
            .map_err(|e| format!("item {}: invalid JSON: {e}", row.id))?;
        match v.get("line").and_then(Value::as_str) {
            Some("current_value") if value.is_none() => {
                value = Some(
                    parse_value_line(&v)
                        .ok_or_else(|| format!("item {}: malformed current value", row.id))?,
                );
            }
            Some("list_price") if list_price.is_none() => {
                list_price = Some(
                    parse_list_price_line(&v)
                        .ok_or_else(|| format!("item {}: malformed list price", row.id))?,
                );
            }
            _ => return Err(format!("item {}: unexpected line", row.id)),
        }
    }
    let value = value.ok_or_else(|| "no current-value line".to_string())?;
    let good = |a: &Amount| a.cents > 0 && a.currency.is_some();
    if !good(&value.amount) || list_price.as_ref().is_some_and(|l| !good(l)) {
        return Err("an amount is not positive or has no currency".to_string());
    }
    Ok(StoredVolumeValue {
        volume_id: volume_id.ok_or_else(|| "no item".to_string())?,
        value,
        list_price,
    })
}

/// What a value acceptance applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedVolumeValue {
    pub volume_id: u64,
    /// "V0042 — title".
    pub volume_name: String,
    pub value: Amount,
    /// The list price written as the purchase price.
    pub purchase_price: Option<Amount>,
    /// The current value before (for the audit row).
    pub previous_value: Option<Amount>,
}

impl AppliedVolumeValue {
    /// The volume's page.
    pub fn href(&self) -> String {
        format!("/volume/{}", self.volume_id)
    }
}

/// Accept value proposal `id` on behalf of `user_id`.
pub async fn accept(
    pool: &DbPool,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome<AppliedVolumeValue>, AcceptError> {
    let outcome = in_retried_transaction(pool, id, user_id, "volume_value", |conn| {
        Box::pin(accept_in(conn, id, user_id, version))
    })
    .await?;
    if let AcceptOutcome::Accepted(applied) = &outcome {
        audit(pool, user_id, applied).await;
    }
    Ok(outcome)
}

/// The amounts as the audit row records them (the hand edit's shape).
fn audit_amount(a: Option<&Amount>) -> (Option<String>, Option<String>) {
    (
        a.map(|a| crate::models::volume_value::cents_to_decimal(a.cents)),
        a.and_then(|a| a.currency.clone()),
    )
}

/// The `admin_audit` row of an accepted value (best effort: the value is
/// already committed).
async fn audit(pool: &DbPool, user_id: u64, applied: &AppliedVolumeValue) {
    let (old_cv, old_cv_cur) = audit_amount(applied.previous_value.as_ref());
    let (new_cv, new_cv_cur) = audit_amount(Some(&applied.value));
    let mut details = json!({
        "source": "proposal",
        "old": {"current_value": old_cv, "current_value_currency": old_cv_cur},
        "new": {"current_value": new_cv, "current_value_currency": new_cv_cur},
    });
    if let Some(p) = &applied.purchase_price {
        let (pp, pp_cur) = audit_amount(Some(p));
        details["old"]["purchase_price"] = Value::Null;
        details["new"]["purchase_price"] = json!(pp);
        details["new"]["purchase_currency"] = json!(pp_cur);
    }
    if let Err(e) = crate::models::admin_audit::AdminAuditModel::create(
        pool,
        user_id,
        "volume_value_update",
        Some("volumes"),
        Some(applied.volume_id),
        Some(details),
    )
    .await
    {
        tracing::warn!(
            volume_id = applied.volume_id,
            error = %e,
            "volume_value_update audit insert failed — the accepted value has already committed"
        );
    }
}

/// [`accept`] on the caller's transaction (the caller commits on `Ok`,
/// rolls back on `Err`). Public for the lock-scope regression test.
pub async fn accept_in(
    conn: &mut MySqlConnection,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome<AppliedVolumeValue>, AcceptError> {
    let (_, items) = lock_pending_in(conn, id, version, &[ProposalKind::VolumeValue]).await?;
    let stored = parse_items(&items).map_err(AcceptError::Invalid)?;
    let check = ProposalService::check_staleness_in(conn, id).await?;
    if check.status != ProposalStatus::Pending {
        return Ok(AcceptOutcome::Stale(StaleCause::Targets(
            check.stale_targets,
        )));
    }
    // Locked since the staleness check; `None` when its title was deleted.
    let Some(live) = VolumeValueModel::find_in(conn, stored.volume_id, true).await? else {
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
    };
    if !same_value(stored.value.current.as_ref(), live.current_value.as_ref()) {
        return mark_stale_in(conn, id, AcceptConflict::ValueChanged).await;
    }
    if stored.list_price.is_some() && live.purchase_price.is_some() {
        return mark_stale_in(conn, id, AcceptConflict::PurchasePriceSet).await;
    }
    let default = parse_default_currency(
        ProposalModel::setting_value(conn, "default_currency")
            .await?
            .as_deref(),
    );
    if stored.value.amount.currency.as_deref() != Some(default.as_str()) {
        return mark_stale_in(conn, id, AcceptConflict::CurrencyChanged { currency: default })
            .await;
    }
    let volume_name = ProposalModel::live_volume_briefs_in(conn, &[live.id])
        .await?
        .remove(&live.id)
        .map(|v| format!("{} — {}", v.code, v.title))
        .unwrap_or_else(|| live.label.clone());
    match ProposalService::transition_in(conn, id, &Decision::Accept { by: user_id }).await? {
        Transition::Decided(_) => {}
        // Unreachable: the same locks are held since the check above.
        Transition::WentStale(targets) => {
            return Ok(AcceptOutcome::Stale(StaleCause::Targets(targets)));
        }
    }
    let currency = |a: &Amount| a.currency.clone().unwrap_or_default();
    let value_currency = currency(&stored.value.amount);
    let purchase_currency = stored.list_price.as_ref().map(currency);
    let written = VolumeValueModel::apply_in(
        conn,
        live.id,
        stored.value.amount.cents,
        &value_currency,
        stored
            .list_price
            .as_ref()
            .zip(purchase_currency.as_deref())
            .map(|(l, c)| (l.cents, c)),
    )
    .await?;
    if written != 1 {
        return Err(AcceptError::Apply(crate::error::AppError::Internal(format!(
            "volume {} not written ({written} rows)",
            live.id
        ))));
    }
    Ok(AcceptOutcome::Accepted(AppliedVolumeValue {
        volume_id: live.id,
        volume_name,
        value: stored.value.amount,
        purchase_price: stored.list_price,
        previous_value: live.current_value,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::proposal::ProposalTarget;
    use crate::models::volume_value::VolumeValueState;
    use crate::services::volume_value_proposals::{list_price_line, value_line};

    fn row(id: u64, table: ProposalTargetTable, volume: u64, v: Value) -> ProposalItemRow {
        ProposalItemRow {
            id,
            proposal_id: 1,
            target: Some(ProposalTarget { table, id: volume }),
            target_version: Some(1),
            proposed_values_json: v.to_string(),
            version: 1,
        }
    }

    fn state() -> VolumeValueState {
        VolumeValueState {
            id: 4,
            title_id: 2,
            label: "V0004".into(),
            version: 1,
            purchase_price: None,
            current_value: None,
            current_value_updated_at: None,
        }
    }

    #[test]
    fn a_value_and_its_list_price_parse_back() {
        let v = ProposalTargetTable::Volumes;
        let stored = parse_items(&[
            row(1, v, 4, value_line(&state(), 1850, "CHF", Some(2490), None)),
            row(2, v, 4, list_price_line(2490, "CHF")),
        ])
        .unwrap();
        assert_eq!(stored.volume_id, 4);
        assert_eq!(stored.value.amount.cents, 1850);
        assert_eq!(stored.list_price.unwrap().cents, 2490);
        let alone = parse_items(&[row(1, v, 4, value_line(&state(), 1850, "CHF", None, None))])
            .unwrap();
        assert_eq!(alone.list_price, None);
    }

    #[test]
    fn malformed_items_are_refused() {
        let v = ProposalTargetTable::Volumes;
        let value = || value_line(&state(), 1850, "CHF", None, None);
        assert!(parse_items(&[]).is_err());
        assert!(parse_items(&[row(1, v, 4, list_price_line(2490, "CHF"))]).is_err());
        assert!(parse_items(&[row(1, ProposalTargetTable::Titles, 4, value())]).is_err());
        assert!(parse_items(&[row(1, v, 4, value()), row(2, v, 5, list_price_line(1, "CHF"))])
            .is_err());
        assert!(parse_items(&[row(1, v, 4, value()), row(2, v, 4, value())]).is_err());
        assert!(parse_items(&[row(1, v, 4, json!({"line": "label"}))]).is_err());
        let zero = value_line(&state(), 0, "CHF", None, None);
        assert!(parse_items(&[row(1, v, 4, zero)]).is_err());
    }
}
