//! Story 15-5 (UX-DR41, value part) — how the proposals page shows a
//! `volume_value` proposal: the current value as filed against the
//! proposed one, the list price labelled as a stand-in for the purchase
//! price, and — read live — the volume's condition, edition note,
//! purchase price and the date of its current value.
//!
//! [`value_view`] / [`list_price_view`] are the `parse_line` arms of
//! `services::proposal_review` (pure); [`with_volume_facts`] runs after
//! the detail is built, with the instance time zone.

use chrono_tz::Tz;
use serde_json::Value;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::volume_value::{Amount, VolumeValueModel};
use crate::services::proposal_review::{LineView, ReviewDetail};
use crate::services::volume_value_proposals::{parse_list_price_line, parse_value_line};
use crate::utils::local_time::format_local_date;

/// The live volume beside a value line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VolumeFacts {
    pub condition: Option<String>,
    pub edition_comment: Option<String>,
    /// The live purchase price ("24.90 CHF").
    pub purchase_price: Option<String>,
    /// The live current value and its date (instance zone) — shown only
    /// when they are no longer the ones filed (review round 1: the page
    /// never pairs the filed value with a live date).
    pub current_value: Option<String>,
    pub current_value_date: Option<String>,
    /// The live value differs from the one filed: accepting will only
    /// mark the proposal outdated.
    pub value_changed: bool,
}

/// An amount as the page shows it (pure): two decimals, a comma in
/// French (the `/stats/value` convention), then the currency.
pub fn amount_text(a: &Amount, loc: &str) -> String {
    let raw = crate::models::volume_value::cents_to_decimal(a.cents);
    let number = if loc == "fr" { raw.replace('.', ",") } else { raw };
    match &a.currency {
        Some(c) => format!("{number} {c}"),
        None => number,
    }
}

/// The display line of a current-value item (pure).
pub fn value_view(volume_id: u64, values: &Value, loc: &str) -> Option<LineView> {
    let line = parse_value_line(values)?;
    Some(LineView {
        line: "current_value",
        target_id: Some(volume_id),
        on_volume: true,
        proposed: amount_text(&line.amount, loc),
        current: line.current.as_ref().map(|a| amount_text(a, loc)),
        // The filed date (RFC 3339 UTC); `with_volume_facts` shows it in
        // the instance zone.
        current_date: line.current_updated_at.clone(),
        note: line.note.filter(|n| !n.trim().is_empty()),
        ..LineView::default()
    })
}

/// The display line of a list-price item (pure).
pub fn list_price_view(volume_id: u64, values: &Value, loc: &str) -> Option<LineView> {
    let line = parse_list_price_line(values)?;
    let purchase_problem = line.problem.map(|p| {
        let key = format!("proposals.purchase_problem_{}", p.as_str());
        rust_i18n::t!(&key, locale = loc).to_string()
    });
    Some(LineView {
        line: "list_price",
        target_id: Some(volume_id),
        on_volume: true,
        proposed: amount_text(&line.amount, loc),
        // Story 16-3 — the purchase price this line replaces, as filed.
        current: line.replaces.as_ref().map(|a| amount_text(a, loc)),
        purchase_problem,
        note: line.note.filter(|n| !n.trim().is_empty()),
        ..LineView::default()
    })
}

/// Fill the live facts of the volume on a value proposal's current-value
/// line (no-op for other kinds or a deleted volume).
pub async fn with_volume_facts(
    pool: &DbPool,
    tz: Tz,
    mut detail: ReviewDetail,
    loc: &str,
) -> Result<ReviewDetail, AppError> {
    // Story 16-3 — a list price may be the proposal's only line.
    let Some(volume_id) = detail
        .lines
        .iter()
        .find(|l| matches!(l.line, "current_value" | "list_price"))
        .and_then(|l| l.target_id)
    else {
        return Ok(detail);
    };
    // Review pass 2 — independent of the live volume: the filed date in
    // the instance zone (L3), and no Remove button on a list price whose
    // removal would leave an unchanged value (refused, L2).
    let unchanged_value = detail
        .lines
        .iter()
        .any(|l| l.line == "current_value" && l.current.as_ref() == Some(&l.proposed));
    for line in detail.lines.iter_mut() {
        match line.line {
            "current_value" => {
                line.current_date = line
                    .current_date
                    .as_deref()
                    .and_then(|d| {
                        chrono::NaiveDateTime::parse_from_str(d, "%Y-%m-%dT%H:%M:%SZ").ok()
                    })
                    .map(|at| format_local_date(at, tz));
            }
            "list_price" if unchanged_value => line.removable = false,
            _ => {}
        }
    }
    let facts = {
        let mut conn = pool.acquire().await?;
        VolumeValueModel::review_facts_in(&mut conn, volume_id).await?
    };
    let Some((state, condition, edition_comment)) = facts else {
        return Ok(detail);
    };
    let live_value = state.current_value.as_ref().map(|a| amount_text(a, loc));
    let filed_value = detail
        .lines
        .iter()
        .find(|l| l.line == "current_value")
        .map_or_else(|| live_value.clone(), |l| l.current.clone());
    let live_purchase = state.purchase_price.as_ref().map(|a| amount_text(a, loc));
    for line in detail.lines.iter_mut().filter(|l| l.line == "list_price") {
        line.purchase_changed = line.current != live_purchase;
    }
    let facts = VolumeFacts {
        value_changed: live_value != filed_value,
        condition,
        edition_comment: edition_comment.filter(|c| !c.trim().is_empty()),
        purchase_price: state.purchase_price.as_ref().map(|a| amount_text(a, loc)),
        current_value: state.current_value.as_ref().map(|a| amount_text(a, loc)),
        current_value_date: state
            .current_value_updated_at
            .map(|at| format_local_date(at, tz)),
    };
    if let Some(first) = detail
        .lines
        .iter_mut()
        .find(|l| matches!(l.line, "current_value" | "list_price"))
    {
        first.volume_facts = Some(facts);
    }
    Ok(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn amounts_read_like_the_value_statistics() {
        let a = Amount {
            cents: 1850,
            currency: Some("CHF".into()),
        };
        assert_eq!(amount_text(&a, "en"), "18.50 CHF");
        assert_eq!(amount_text(&a, "fr"), "18,50 CHF");
    }

    #[test]
    fn value_lines_show_current_and_proposed() {
        let v = json!({"line": "current_value", "amount_cents": 1850, "currency": "CHF",
                       "current": {"amount_cents": 1200, "currency": "CHF", "updated_at": null},
                       "note": "  ", "fingerprint": "1850CHF"});
        let line = value_view(4, &v, "en").unwrap();
        assert_eq!(line.line, "current_value");
        assert_eq!(line.proposed, "18.50 CHF");
        assert_eq!(line.current.as_deref(), Some("12.00 CHF"));
        assert_eq!(line.note, None);
        let list = list_price_view(
            4,
            &json!({"line": "list_price", "amount_cents": 2490, "currency": "CHF"}),
            "en",
        )
        .unwrap();
        assert_eq!((list.line, list.proposed.as_str()), ("list_price", "24.90 CHF"));
        assert_eq!((list.current, list.purchase_problem), (None, None));
        let replacing = list_price_view(
            4,
            &json!({"line": "list_price", "amount_cents": 2490, "currency": "CHF",
                    "replaces": {"amount_cents": 249000, "currency": "CHF"},
                    "inconsistent": "scale", "note": "why"}),
            "en",
        )
        .unwrap();
        assert_eq!(replacing.current.as_deref(), Some("2490.00 CHF"));
        assert!(replacing.purchase_problem.is_some());
        assert_eq!(replacing.note.as_deref(), Some("why"));
        assert_eq!(value_view(4, &json!({"line": "current_value"}), "en"), None);
    }
}
