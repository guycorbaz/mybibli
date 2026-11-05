//! Story 15-4 — a librarian edits or removes the lines of a pending
//! `series_update` proposal (the story 11-19 flow of
//! `services::proposal_edit`, which calls in here for this kind).
//!
//! - The **update line** is editable (name, type, total — the inputs of a
//!   series creation) and never removable. The edited values are checked
//!   with the filing rules against the series as filed (`current` stays
//!   as filed, so do `fingerprint` and the equivalence key — as for the
//!   other series kinds), then against the live series under its row
//!   lock (lock order proposal → items → series): another live series
//!   with the new name, the total rule on the live held positions and
//!   tomes minus the proposal's removals.
//! - A **removal line** is removable, not editable. Removing it re-runs
//!   the same checks on what remains: the freed tome may now sit above
//!   the proposed total, and a proposal left without any change is
//!   refused ("nothing to change" — reject it instead).

use serde_json::Value;
use sqlx::MySqlConnection;

use crate::models::expected_tome::ExpectedTomeModel;
use crate::models::proposal::{ProposalItemRow, ProposalModel};
use crate::models::series::{SeriesType, TitleSeriesModel};
use crate::services::proposal_edit::{EditError, EditForm, form_str, series_hint};
use crate::services::series_update_acceptance::{StoredSeriesUpdate, parse_items};
use crate::services::series_update_proposals::{
    PositionsInTheWay, SeriesUpdateError, check_total_rule, positions_list, parse_update_line, resolve_change, update_line,
};

/// The librarian's hint for a refused edit or removal (terse; the
/// positions are named).
pub fn update_hint(e: &SeriesUpdateError, loc: &str) -> String {
    use rust_i18n::t;
    match e {
        SeriesUpdateError::Series(e) => series_hint(e, loc),
        SeriesUpdateError::NoChange => {
            t!("proposals.edit_error_no_change", locale = loc).to_string()
        }
        SeriesUpdateError::TotalNeedsClosed => {
            t!("proposals.edit_error_invalid", locale = loc).to_string()
        }
        SeriesUpdateError::NameTaken { name, .. } => t!(
            "proposals.edit_error_series_exists",
            locale = loc,
            name = name.as_str()
        )
        .to_string(),
        SeriesUpdateError::TotalBelowPositions {
            total,
            positions: PositionsInTheWay { owned, expected },
        } => t!(
            "proposals.edit_error_below_positions",
            locale = loc,
            total = total,
            positions = positions_text(owned, expected, loc).as_str()
        )
        .to_string(),
        SeriesUpdateError::TomeOwned { .. }
        | SeriesUpdateError::TooManyRemovals { .. }
        | SeriesUpdateError::TomeTwice { .. }
        | SeriesUpdateError::TomeNotInSeries { .. } => {
            t!("proposals.edit_error_invalid", locale = loc).to_string()
        }
    }
}

/// "owned positions 7, 8; expected tomes at positions 12" — the
/// positions in the way, localized (shared with the stale message).
pub fn positions_text(owned: &[i32], expected: &[i32], loc: &str) -> String {
    use rust_i18n::t;
    let join = positions_list;
    let mut parts = Vec::new();
    if !owned.is_empty() {
        parts.push(
            t!("proposals.positions_owned", locale = loc, positions = join(owned).as_str())
                .to_string(),
        );
    }
    if !expected.is_empty() {
        parts.push(
            t!(
                "proposals.positions_expected",
                locale = loc,
                positions = join(expected).as_str()
            )
            .to_string(),
        );
    }
    parts.join("; ")
}

/// The new values of the update line from the edit form (pure — unit
/// tested): the change recomputed against the filed `current`; the
/// filed `fingerprint` kept.
pub fn edited_update_line(old: &Value, form: &EditForm, removed: &[u64]) -> Result<Value, EditError> {
    let (current, _) = parse_update_line(old).ok_or(EditError::NotEditable)?;
    let series_type = form_str(form, "series_type")
        .parse::<SeriesType>()
        .map_err(EditError::Invalid)?;
    let total = match series_type {
        SeriesType::Open => None,
        SeriesType::Closed => {
            let raw = form_str(form, "total").trim();
            // Empty or not a number reads as 0, which the range refuses.
            (!raw.is_empty()).then(|| raw.parse::<i64>().unwrap_or(0))
        }
    };
    // Type and total as filed are left alone (not re-validated):
    // `resolve_change` treats values equal to the current ones so.
    let change = resolve_change(
        &current,
        Some(form_str(form, "name")),
        Some(series_type),
        total,
    )?;
    let mut values = update_line(&current, &change, removed);
    values["fingerprint"] = old.get("fingerprint").cloned().unwrap_or(Value::Null);
    Ok(values)
}

/// The proposal as stored once item `edited` carries `values` (or,
/// with `values` `None`, without it).
fn stored_with(
    rows: &[ProposalItemRow],
    item_id: u64,
    values: Option<&Value>,
) -> Result<StoredSeriesUpdate, EditError> {
    let rows: Vec<ProposalItemRow> = rows
        .iter()
        .filter(|r| values.is_some() || r.id != item_id)
        .cloned()
        .map(|mut r| {
            if let Some(v) = values
                && r.id == item_id
            {
                r.proposed_values_json = v.to_string();
            }
            r
        })
        .collect();
    parse_items(&rows).map_err(EditError::Invalid)
}

/// Check the proposal as it would be stored against the live series,
/// under its row lock.
async fn check_live(conn: &mut MySqlConnection, stored: &StoredSeriesUpdate) -> Result<(), EditError> {
    if stored.change.is_empty() && stored.removals.is_empty() {
        return Err(SeriesUpdateError::NoChange.into());
    }
    if ExpectedTomeModel::lock_series_in(conn, stored.series_id)
        .await?
        .is_none()
    {
        return Err(EditError::SeriesGone);
    }
    if let Some(name) = &stored.change.name
        && let Some((other_id, other_name)) =
            ProposalModel::other_series_named(conn, name, stored.series_id).await?
    {
        return Err(SeriesUpdateError::NameTaken {
            series_id: other_id,
            name: other_name,
        }
        .into());
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
    check_total_rule(
        &stored.filed,
        &stored.change,
        &held,
        &tome_positions,
        &removed,
    )?;
    Ok(())
}

/// The new values of edited line `item_id` (the update line only).
pub async fn edit_in(
    conn: &mut MySqlConnection,
    rows: &[ProposalItemRow],
    item_id: u64,
    old: &Value,
    form: &EditForm,
) -> Result<Value, EditError> {
    let current = parse_items(rows).map_err(EditError::Invalid)?;
    let removed: Vec<u64> = current.removals.iter().map(|r| r.tome_id).collect();
    let values = edited_update_line(old, form, &removed)?;
    check_live(conn, &stored_with(rows, item_id, Some(&values))?).await?;
    Ok(values)
}

/// Check that the proposal without line `item_id` still holds.
pub async fn check_removal_in(
    conn: &mut MySqlConnection,
    rows: &[ProposalItemRow],
    item_id: u64,
) -> Result<(), EditError> {
    check_live(conn, &stored_with(rows, item_id, None)?).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn form(pairs: &[(&str, &str)]) -> EditForm {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn old() -> Value {
        json!({"line": "series_update",
               "current": {"name": "Tintin", "series_type": "open", "total_volume_count": null},
               "series_type": "closed", "total_volume_count": 24, "fingerprint": "filed"})
    }

    #[test]
    fn an_edit_recomputes_the_change_and_keeps_the_fingerprint() {
        let v = edited_update_line(
            &old(),
            &form(&[("name", "Tintin"), ("series_type", "closed"), ("total", "23")]),
            &[],
        )
        .unwrap();
        assert_eq!(v["total_volume_count"], json!(23));
        assert_eq!(v["series_type"], json!("closed"));
        assert!(v.get("name").is_none(), "{v}");
        assert_eq!(v["fingerprint"], json!("filed"));
        assert_eq!(v["current"], old()["current"]);
    }

    #[test]
    fn a_rename_keeps_a_hand_set_total_outside_the_range() {
        let old = json!({"line": "series_update",
               "current": {"name": "Big", "series_type": "closed", "total_volume_count": 20000},
               "name": "Big!", "fingerprint": "filed"});
        let v = edited_update_line(
            &old,
            &form(&[("name", "Big?"), ("series_type", "closed"), ("total", "20000")]),
            &[],
        )
        .unwrap();
        assert_eq!(v["name"], json!("Big?"));
        assert!(v.get("total_volume_count").is_none(), "{v}");
    }

    #[test]
    fn a_closed_series_without_total_can_be_renamed() {
        let old = json!({"line": "series_update",
               "current": {"name": "Legacy", "series_type": "closed", "total_volume_count": null},
               "name": "Legacy!", "fingerprint": "filed"});
        let v = edited_update_line(
            &old,
            &form(&[("name", "Legacy?"), ("series_type", "closed"), ("total", "")]),
            &[],
        )
        .unwrap();
        assert_eq!(v["name"], json!("Legacy?"));
    }

    #[test]
    fn an_edit_passes_the_filing_rules() {
        let bad = |pairs: &[(&str, &str)]| edited_update_line(&old(), &form(pairs), &[]).is_err();
        assert!(bad(&[("name", "Tintin"), ("series_type", "closed"), ("total", "")]));
        assert!(bad(&[("name", "Tintin"), ("series_type", "closed"), ("total", "x")]));
        assert!(bad(&[("name", " "), ("series_type", "open")]));
        assert!(bad(&[("name", "Tintin"), ("series_type", "neither")]));
        // Back to the series as it is: a valid line with no change (the
        // whole-proposal check refuses it when nothing else remains).
        let v = edited_update_line(&old(), &form(&[("name", "Tintin"), ("series_type", "open")]), &[])
            .unwrap();
        assert!(parse_update_line(&v).unwrap().1.is_empty());
    }
}
