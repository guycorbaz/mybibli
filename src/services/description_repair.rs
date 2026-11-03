//! Story 13-4 (#578) — run-once startup repair of HTML descriptions.
//!
//! Descriptions stored before v1.23.0 may carry the providers' HTML (Google
//! Books, Comic Vine). [`repair_html_descriptions_once`], called from
//! `main.rs` after the migrations:
//!
//! 1. purges the `metadata_cache` rows whose description holds markup
//!    (outside the transaction; idempotent — a purged row is refetched);
//! 2. in **one transaction** with its `data_repairs` marker
//!    (`services::data_repairs::run_once`):
//!    - converts every live title description containing `<` or `&…;` to
//!      its [`html_to_plain_text`] form (cleared when nothing but markup),
//!      unless `description` is in the title's `manually_edited_fields`;
//!      a **non-edit write** — `version` is not bumped, so a pending
//!      series proposal recorded against the title stays acceptable
//!      (review round 2, M1; same principle as #521);
//!    - rewrites the pending metadata proposal items on `description`: a
//!      recorded `current` equal to the converted text becomes the new
//!      text (so the acceptance's `FieldChanged` does not fire), and an
//!      agent's `proposed` value filed before the deploy is converted like
//!      a new filing (a librarian-edited line is left as typed).
//!
//! The conversion is not idempotent on its own output (a converted text may
//! hold `<b>` or `&lt;` as text), hence the marker: it runs once per
//! database.

use std::collections::HashMap;

use serde_json::Value;
use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::metadata_cache::MetadataCacheModel;
use crate::models::proposal::ProposalModel;
use crate::models::title::TitleModel;
use crate::services::data_repairs;
use crate::services::metadata_proposals::normalize_text;
use crate::utils::html_to_plain_text;

/// Name of the story 13-4 repair in `data_repairs`.
pub const HTML_DESCRIPTIONS_REPAIR: &str = "13-4-html-descriptions-to-plain-text";

/// What the repair did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DescriptionCleanup {
    /// Description rewritten as plain text (or cleared when it was markup
    /// only).
    pub converted: usize,
    /// `description` is in `manually_edited_fields`: never rewritten.
    pub hand_edited: usize,
    /// Nothing to convert (`a < b`, `<<Le>>`, `R&D;`).
    pub unchanged: usize,
    /// The row changed between the read and the update (guarded write).
    pub superseded: usize,
    /// Pending proposal items whose recorded `current` was converted.
    pub items_current: usize,
    /// Pending proposal items whose agent-`proposed` value was converted.
    pub items_proposed: usize,
    /// `metadata_cache` rows purged (description with markup).
    pub cache_purged: u64,
}

/// Is `description` among a title's hand-edited fields? An unreadable
/// `manually_edited_fields` counts as edited: the repair never risks
/// rewriting a hand-typed text.
pub fn description_is_hand_edited(manually_edited_fields: Option<&str>) -> bool {
    match manually_edited_fields {
        None => false,
        Some(raw) => serde_json::from_str::<Vec<String>>(raw)
            .map(|fields| fields.iter().any(|f| f == "description"))
            .unwrap_or(true),
    }
}

/// A title description the repair converted: `(old, new)`.
type Converted = (String, Option<String>);

/// Rewrite one pending `description` item (pure — unit tested). `title`
/// is the conversion of its target title, if any; `edited` whether a
/// librarian edited the line. Returns `(current rewritten, proposed
/// rewritten)`.
pub fn repair_item(values: &mut Value, title: Option<&Converted>, edited: bool) -> (bool, bool) {
    let mut current_done = false;
    if let Some((old, new)) = title
        && let Some(current) = values.get("current").and_then(Value::as_str)
        && normalize_text(current, true) == normalize_text(old, true)
    {
        values["current"] = new.as_deref().map_or(Value::Null, Value::from);
        current_done = true;
    }
    let mut proposed_done = false;
    if !edited && let Some(proposed) = values.get("proposed").and_then(Value::as_str) {
        let plain = normalize_text(&html_to_plain_text(proposed), true);
        // A proposal of markup alone keeps its value: refusing it is the
        // librarian's call, not a data repair's.
        if !plain.is_empty() && plain != proposed {
            values["proposed"] = Value::from(plain);
            proposed_done = true;
        }
    }
    (current_done, proposed_done)
}

/// Run the repair once per database; `None` when it already ran.
pub async fn repair_html_descriptions_once(
    pool: &DbPool,
) -> Result<Option<DescriptionCleanup>, AppError> {
    if data_repairs::is_applied(pool, HTML_DESCRIPTIONS_REPAIR).await? {
        return Ok(None);
    }
    let cache_purged = MetadataCacheModel::purge_markup_descriptions(pool).await?;
    data_repairs::run_once(pool, HTML_DESCRIPTIONS_REPAIR, move |conn| {
        Box::pin(async move {
            let mut report = convert_in(conn).await?;
            report.cache_purged = cache_purged;
            tracing::info!(
                converted = report.converted,
                skipped_hand_edited = report.hand_edited,
                skipped_unchanged = report.unchanged,
                skipped_superseded = report.superseded,
                items_current = report.items_current,
                items_proposed = report.items_proposed,
                cache_purged = report.cache_purged,
                "HTML description: startup plain-text conversion done (#578)"
            );
            Ok(report)
        })
    })
    .await
}

/// The transactional part: titles, then pending proposal items.
async fn convert_in(conn: &mut MySqlConnection) -> Result<DescriptionCleanup, AppError> {
    let mut report = DescriptionCleanup::default();
    let mut converted: HashMap<u64, Converted> = HashMap::new();
    for candidate in TitleModel::list_descriptions_with_markup(conn).await? {
        if description_is_hand_edited(candidate.manually_edited_fields.as_deref()) {
            report.hand_edited += 1;
            continue;
        }
        let plain = html_to_plain_text(&candidate.description);
        if plain == candidate.description {
            report.unchanged += 1;
            continue;
        }
        let new = (!plain.is_empty()).then_some(plain);
        if TitleModel::set_plain_description(
            conn,
            candidate.id,
            candidate.version,
            &candidate.description,
            new.as_deref(),
        )
        .await?
        {
            report.converted += 1;
            converted.insert(candidate.id, (candidate.description, new));
        } else {
            report.superseded += 1;
            tracing::info!(
                title_id = candidate.id,
                "HTML description: title changed meanwhile; left unchanged"
            );
        }
    }
    for (item_id, title_id, raw, edited) in
        ProposalModel::pending_description_items_in(conn).await?
    {
        let Ok(mut values) = serde_json::from_str::<Value>(&raw) else {
            tracing::warn!(
                item_id,
                "HTML description: unreadable proposal item; left as is"
            );
            continue;
        };
        let title = title_id.and_then(|id| converted.get(&id));
        let (current, proposed) = repair_item(&mut values, title, edited);
        if current || proposed {
            ProposalModel::set_item_values_in(conn, item_id, &values).await?;
            report.items_current += usize::from(current);
            report.items_proposed += usize::from(proposed);
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn description_hand_edit_guard() {
        assert!(!description_is_hand_edited(None));
        assert!(!description_is_hand_edited(Some("[]")));
        assert!(!description_is_hand_edited(Some(
            r#"["title","publisher"]"#
        )));
        assert!(description_is_hand_edited(Some(
            r#"["title","description"]"#
        )));
        // Unreadable: treated as edited, never rewritten.
        assert!(description_is_hand_edited(Some("not json")));
    }

    fn item(current: Value, proposed: &str) -> Value {
        json!({"line": "field", "field": "description", "current": current, "proposed": proposed})
    }

    #[test]
    fn an_item_follows_its_converted_title_and_its_proposal_is_converted() {
        let conv: Converted = ("<p>Old</p>".into(), Some("Old".into()));
        let mut v = item(json!("<p>Old</p>"), "<b>New</b> &amp; more");
        assert_eq!(repair_item(&mut v, Some(&conv), false), (true, true));
        assert_eq!(v["current"], "Old");
        assert_eq!(v["proposed"], "New & more");
    }

    #[test]
    fn a_current_that_no_longer_matches_the_title_is_left_alone() {
        let conv: Converted = ("<p>Old</p>".into(), Some("Old".into()));
        let mut v = item(json!("Something else"), "Plain");
        assert_eq!(repair_item(&mut v, Some(&conv), false), (false, false));
        assert_eq!(v["current"], "Something else");
    }

    #[test]
    fn a_markup_only_title_clears_the_recorded_current() {
        let conv: Converted = ("<p><br></p>".into(), None);
        let mut v = item(json!("<p><br></p>"), "Text");
        assert_eq!(repair_item(&mut v, Some(&conv), false), (true, false));
        assert!(v["current"].is_null());
    }

    #[test]
    fn a_librarian_edited_line_keeps_its_proposed_text() {
        let mut v = item(Value::Null, "<b>typed by hand</b>");
        assert_eq!(repair_item(&mut v, None, true), (false, false));
        assert_eq!(v["proposed"], "<b>typed by hand</b>");
    }

    #[test]
    fn a_markup_only_proposal_keeps_its_value() {
        let mut v = item(Value::Null, "<p></p>");
        assert_eq!(repair_item(&mut v, None, false), (false, false));
    }
}
