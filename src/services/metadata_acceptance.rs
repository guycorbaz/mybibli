//! Story 11-18 (FR138, FR140, FR144, FR146) — accepting a `metadata`
//! proposal.
//!
//! The items (shapes: module doc of
//! [`crate::services::metadata_proposals`], D-11-13-2) are applied in
//! **one transaction** (the shared steps are in
//! [`crate::services::acceptance`]):
//!
//! 1. the proposal row is locked, its `version` compared, its items read
//!    (share lock) and parsed back; the proposed values are re-validated
//!    with the filing rules ([`validate_fields`]) — a malformed proposal
//!    is refused, not marked stale (D-11-17-6);
//! 2. [`ProposalService::check_staleness_in`] locks the title and marks
//!    the proposal `stale` when it was deleted — a moved `version` alone
//!    no longer outdates a metadata proposal (story 12-2, #542, FR149);
//! 3. under that lock, [`crate::services::proposals::title_moved`]
//!    (re-identified title; `version` for a `current_truncated` or
//!    pre-12-2 item), then the title's live values are compared with the
//!    `current` value each item recorded at filing: only a change of a
//!    field the proposal touches outdates it.
//!    Contributors only add (D-11-13-4): their `current` list is display
//!    only, but a proposed contributor whose role was deleted since is a
//!    conflict. A conflict marks the proposal `stale`;
//! 4. the decision is recorded and the fields written: scalar fields
//!    (`publication_year` as `YYYY-01-01`) plus `manually_edited_fields`
//!    (FR144, `publication_year` tracked as `publication_date`) and a
//!    `version` bump; each contributor is resolved by name with the SQL
//!    collation (an existing one is reused, else created), skipped when
//!    the title already credits that contributor id in that role (or the
//!    item resolved two entries to it), else added — a credit removed
//!    earlier is reactivated (#531).
//!
//! The title's junction rows are locked (`FOR UPDATE`, removed ones
//! included) before the "already credited" check, after the title row
//! (the order of `check_staleness_in`), so a concurrent manual add for
//! the title waits for the acceptance.

use std::collections::HashSet;

use chrono::{Datelike, NaiveDate};
use serde_json::{Map, Value, json};
use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::contributor::{ContributorRoleModel, TitleContributorModel};
use crate::models::proposal::{ProposalItemRow, ProposalKind, ProposalTargetTable};
use crate::models::title::{ProposedFieldWrites, TitleModel};
use crate::services::acceptance::{
    AcceptConflict, AcceptError, AcceptOutcome, StaleCause, in_retried_transaction,
    lock_pending_in, mark_stale_in, mark_title_moved_in,
};
use crate::services::contributor::ContributorService;
use crate::services::metadata_proposals::{
    PROPOSABLE_FIELDS, ProposedFields, normalize_text, validate_fields,
};
use crate::services::proposals::{Decision, ProposalService, Transition};

/// What an item recorded as the field's value at filing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Baseline {
    /// A text field (`None` = empty at filing).
    Text(Option<String>),
    /// `publication_year`: the full date at filing when recorded
    /// (`current_date`), else only its year.
    Date {
        year: Option<i64>,
        date: Option<Option<String>>,
    },
    /// `current_truncated`: the value was cut at filing; `version` only.
    Unchecked,
}

/// One scalar field item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredScalar {
    pub field: &'static str,
    pub baseline: Baseline,
    pub proposed: Value,
}

/// One contributor to add, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredContributor {
    pub name: String,
    pub role_id: u64,
    /// The role's name at filing (for messages).
    pub role: String,
}

/// A stored metadata proposal, parsed back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMetadata {
    pub title_id: u64,
    pub scalars: Vec<StoredScalar>,
    pub contributors: Option<Vec<StoredContributor>>,
}

/// Whether `field` is a multi-line text field.
fn multiline(field: &str) -> bool {
    field == "description"
}

/// Parse the stored items of a metadata proposal (pure — unit tested).
/// Every item targets the same title, is a `"line": "field"` item for a
/// proposable field, at most once; anything else is refused with a short
/// English reason (filing never produces it).
pub fn parse_items(items: &[ProposalItemRow]) -> Result<StoredMetadata, String> {
    let mut title_id: Option<u64> = None;
    let mut seen: HashSet<&'static str> = HashSet::new();
    let mut out = StoredMetadata {
        title_id: 0,
        scalars: Vec::new(),
        contributors: None,
    };
    for row in items {
        let target = match row.target {
            Some(t) if t.table == ProposalTargetTable::Titles => t.id,
            _ => return Err(format!("item {}: a field targets a title", row.id)),
        };
        if *title_id.get_or_insert(target) != target {
            return Err(format!("item {}: another title", row.id));
        }
        let v: Value = serde_json::from_str(&row.proposed_values_json)
            .map_err(|e| format!("item {}: invalid JSON: {e}", row.id))?;
        if v.get("line").and_then(Value::as_str) != Some("field") {
            return Err(format!("item {}: not a field line", row.id));
        }
        let field = v
            .get("field")
            .and_then(Value::as_str)
            .and_then(|f| PROPOSABLE_FIELDS.iter().find(|p| **p == f).copied())
            .ok_or_else(|| format!("item {}: unknown field", row.id))?;
        if !seen.insert(field) {
            return Err(format!("item {}: {field} twice", row.id));
        }
        let proposed = v
            .get("proposed")
            .cloned()
            .ok_or_else(|| format!("item {}: nothing proposed", row.id))?;
        let truncated = v
            .get("current_truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let current = v.get("current").unwrap_or(&Value::Null);
        if field == "contributors" {
            let list = proposed
                .as_array()
                .ok_or_else(|| format!("item {}: contributors are not a list", row.id))?;
            let parsed = list
                .iter()
                .map(|c| {
                    let role_id = c.get("role_id")?.as_u64()?;
                    Some(StoredContributor {
                        name: c.get("name")?.as_str()?.to_string(),
                        role_id,
                        role: c
                            .get("role")
                            .and_then(Value::as_str)
                            .filter(|r| !r.trim().is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("#{role_id}")),
                    })
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| format!("item {}: a contributor lacks a name or role", row.id))?;
            out.contributors = Some(parsed);
            continue;
        }
        let baseline = if truncated {
            Baseline::Unchecked
        } else if field == "publication_year" {
            let year = match current {
                Value::Null => None,
                v => Some(
                    v.as_i64()
                        .ok_or_else(|| format!("item {}: current year", row.id))?,
                ),
            };
            let date = match v.get("current_date") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(Value::String(d)) => Some(Some(d.clone())),
                Some(_) => return Err(format!("item {}: current date", row.id)),
            };
            Baseline::Date { year, date }
        } else {
            match current {
                Value::Null => Baseline::Text(None),
                Value::String(s) => Baseline::Text(Some(s.clone())),
                _ => return Err(format!("item {}: current value", row.id)),
            }
        };
        out.scalars.push(StoredScalar {
            field,
            baseline,
            proposed,
        });
    }
    out.title_id = title_id.ok_or("no items")?;
    Ok(out)
}

/// Re-validate the stored values with the filing rules (pure). Roles are
/// matched by their stored id: each entry gets a placeholder role name
/// `#<id>` that only that id carries, so a role renamed since filing is
/// no problem (a deleted one is a state conflict, checked under lock).
pub fn revalidate(stored: &StoredMetadata, current_year: i32) -> Result<ProposedFields, String> {
    let mut map = Map::new();
    for s in &stored.scalars {
        map.insert(s.field.to_string(), s.proposed.clone());
    }
    let mut roles: Vec<(u64, String)> = Vec::new();
    if let Some(list) = &stored.contributors {
        let entries = list
            .iter()
            .map(|c| {
                if !roles.iter().any(|(id, _)| *id == c.role_id) {
                    roles.push((c.role_id, format!("#{}", c.role_id)));
                }
                json!({"name": c.name, "role": format!("#{}", c.role_id)})
            })
            .collect();
        map.insert("contributors".into(), Value::Array(entries));
    }
    validate_fields(&map, &roles, current_year).map_err(|e| e.to_string())
}

/// Validation keeps every contributor entry, in order, with its stored
/// role id, or fails as a whole; checked before anything is decided, so
/// the apply can pair the stored and validated lists (pure).
pub fn paired_contributors(stored: &StoredMetadata, valid: &ProposedFields) -> Result<(), String> {
    match (&stored.contributors, &valid.contributors) {
        (None, None) => Ok(()),
        (Some(s), Some(v))
            if s.len() == v.len() && s.iter().zip(v).all(|(a, b)| a.role_id == b.role_id) =>
        {
            Ok(())
        }
        _ => Err("contributors changed in validation".to_string()),
    }
}

/// A text value as compared: normal form, empty as absent.
fn compared(value: Option<&str>, multiline: bool) -> Option<String> {
    value
        .map(|v| normalize_text(v, multiline))
        .filter(|v| !v.is_empty())
}

/// Whether the title's live value differs from the item's baseline
/// (pure — unit tested). Values are compared in the normal form filing
/// stored them in, so spacing or NFD alone is no change.
pub fn field_changed(scalar: &StoredScalar, title: &TitleModel) -> bool {
    let live_text = |field: &str| -> Option<&str> {
        match field {
            "subtitle" => title.subtitle.as_deref(),
            "description" => title.description.as_deref(),
            "publisher" => title.publisher.as_deref(),
            "dewey_code" => title.dewey_code.as_deref(),
            _ => None,
        }
    };
    match &scalar.baseline {
        Baseline::Unchecked => false,
        Baseline::Text(stored) => {
            let ml = multiline(scalar.field);
            compared(stored.as_deref(), ml) != compared(live_text(scalar.field), ml)
        }
        Baseline::Date { year, date } => match date {
            Some(date) => {
                *date
                    != title
                        .publication_date
                        .map(|d| d.format("%Y-%m-%d").to_string())
            }
            None => *year != title.publication_date.map(|d| i64::from(d.year())),
        },
    }
}

/// What a metadata acceptance applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMetadata {
    pub title_id: u64,
    pub title: String,
    /// The scalar fields written, in item order (proposable names).
    pub fields: Vec<&'static str>,
    /// Contributors credited by this acceptance (`name`, role name).
    pub contributors_added: Vec<(String, String)>,
    /// Proposed contributors the title already credited in that role.
    pub contributors_present: Vec<(String, String)>,
}

/// Accept metadata proposal `id` on behalf of `user_id`, in its own
/// transaction (deadlocks retried).
pub async fn accept(
    pool: &DbPool,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome<AppliedMetadata>, AcceptError> {
    in_retried_transaction(pool, id, user_id, "metadata", |conn| {
        Box::pin(accept_in(conn, id, user_id, version))
    })
    .await
}

async fn accept_in(
    conn: &mut MySqlConnection,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome<AppliedMetadata>, AcceptError> {
    let (_, items) = lock_pending_in(conn, id, version, &[ProposalKind::Metadata]).await?;
    let stored = parse_items(&items).map_err(AcceptError::Invalid)?;
    let valid = revalidate(&stored, chrono::Utc::now().year()).map_err(AcceptError::Invalid)?;
    paired_contributors(&stored, &valid).map_err(AcceptError::Invalid)?;

    let check = ProposalService::check_staleness_in(conn, id).await?;
    if check.status != crate::models::proposal::ProposalStatus::Pending {
        return Ok(AcceptOutcome::Stale(StaleCause::Targets(
            check.stale_targets,
        )));
    }
    // Locked by the staleness check; a locking read sees its latest
    // committed values.
    let title = TitleModel::find_by_id_in(conn, stored.title_id, true)
        .await?
        .ok_or_else(|| {
            AcceptError::Apply(AppError::Internal(
                "proposal title gone although not stale".to_string(),
            ))
        })?;
    if crate::services::proposals::title_moved(&items, &title) {
        return mark_title_moved_in(conn, id, &items).await;
    }
    if let Some(s) = stored.scalars.iter().find(|s| field_changed(s, &title)) {
        return mark_stale_in(
            conn,
            id,
            AcceptConflict::FieldChanged {
                field: s.field.to_string(),
            },
        )
        .await;
    }
    let mut credits: HashSet<(u64, u64)> = HashSet::new();
    let mut live_roles: Vec<(u64, String)> = Vec::new();
    if let Some(list) = &stored.contributors {
        credits = TitleContributorModel::lock_credits_in(conn, title.id)
            .await?
            .into_iter()
            .collect();
        let mut ids: Vec<u64> = list.iter().map(|c| c.role_id).collect();
        ids.sort_unstable();
        ids.dedup();
        live_roles = ContributorRoleModel::live_among_in(conn, &ids).await?;
        if let Some(gone) = list
            .iter()
            .find(|c| !live_roles.iter().any(|(rid, _)| *rid == c.role_id))
        {
            return mark_stale_in(
                conn,
                id,
                AcceptConflict::RoleGone {
                    role: gone.role.clone(),
                },
            )
            .await;
        }
    }

    // The decision (FR146), then the writes.
    match ProposalService::transition_in(conn, id, &Decision::Accept { by: user_id }).await? {
        Transition::Decided(_) => {}
        // Unreachable: the same locks are held since the check above.
        Transition::WentStale(targets) => {
            return Ok(AcceptOutcome::Stale(StaleCause::Targets(targets)));
        }
    }

    let publication_date = valid
        .publication_year
        .and_then(|y| NaiveDate::from_ymd_opt(y, 1, 1));
    let writes = ProposedFieldWrites {
        subtitle: valid.subtitle.as_deref(),
        description: valid.description.as_deref(),
        publisher: valid.publisher.as_deref(),
        publication_date,
        dewey_code: valid.dewey_code.as_deref(),
    };
    // Only the fields actually written are reported and marked.
    let fields: Vec<&'static str> = stored
        .scalars
        .iter()
        .map(|s| s.field)
        .filter(|f| match *f {
            "subtitle" => writes.subtitle.is_some(),
            "description" => writes.description.is_some(),
            "publisher" => writes.publisher.is_some(),
            "publication_year" => writes.publication_date.is_some(),
            "dewey_code" => writes.dewey_code.is_some(),
            _ => false,
        })
        .collect();
    if !fields.is_empty() {
        // `publication_year` is tracked as `publication_date`, the column
        // the re-download guard knows.
        let edited: Vec<&str> = fields
            .iter()
            .map(|f| match *f {
                "publication_year" => "publication_date",
                other => other,
            })
            .collect();
        TitleModel::apply_proposed_fields_in(
            conn,
            title.id,
            &writes,
            &title.manually_edited_with(&edited),
        )
        .await?;
    }

    let mut applied = AppliedMetadata {
        title_id: title.id,
        title: title.title.clone(),
        fields,
        contributors_added: Vec::new(),
        contributors_present: Vec::new(),
    };
    if let (Some(list), Some(valid_list)) = (&stored.contributors, &valid.contributors) {
        // Paired before the decision (`paired_contributors`).
        for (c, v) in list.iter().zip(valid_list) {
            let contributor = ContributorService::find_or_create_in(conn, &v.name).await?;
            let role = live_roles
                .iter()
                .find(|(rid, _)| *rid == c.role_id)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| c.role.clone());
            if !credits.insert((contributor.id, c.role_id)) {
                applied.contributors_present.push((contributor.name, role));
                continue;
            }
            TitleContributorModel::add_to_title_in(conn, title.id, contributor.id, c.role_id)
                .await?;
            applied.contributors_added.push((contributor.name, role));
        }
    }
    Ok(AcceptOutcome::Accepted(applied))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::proposal::ProposalTarget;

    fn row(id: u64, title: u64, v: Value) -> ProposalItemRow {
        ProposalItemRow {
            id,
            proposal_id: 1,
            target: Some(ProposalTarget {
                table: ProposalTargetTable::Titles,
                id: title,
            }),
            target_version: Some(1),
            proposed_values_json: v.to_string(),
            version: 1,
        }
    }

    // Story 12-2 (review): what the per-field checks cannot see.
    #[test]
    fn title_moved_detects_identity_and_legacy_cases() {
        use crate::models::proposal::{ProposalTarget, ProposalTargetTable};
        use crate::services::proposals::{title_identity, title_moved};
        let t = title();
        let item = |values: Value, version: i32| ProposalItemRow {
            id: 1,
            proposal_id: 1,
            target: Some(ProposalTarget {
                table: ProposalTargetTable::Titles,
                id: t.id,
            }),
            target_version: Some(version),
            proposed_values_json: values.to_string(),
            version: 1,
        };
        let same = title_identity(&t);
        let moved = t.version + 1;
        // Identity unchanged: a moved version alone is not enough.
        assert!(!title_moved(&[item(json!({"identity": same}), moved)], &t));
        // Identity changed (name, ISBN or media type): moved.
        let mut renamed = same.clone();
        renamed["title"] = json!("Autre œuvre");
        assert!(title_moved(&[item(json!({"identity": renamed}), t.version)], &t));
        let mut isbn = same.clone();
        isbn["isbn"] = json!("9782070360246");
        assert!(title_moved(&[item(json!({"identity": isbn}), t.version)], &t));
        // A cut baseline keeps the version rule.
        let cut = json!({"identity": same, "current_truncated": true});
        assert!(title_moved(&[item(cut.clone(), moved)], &t));
        assert!(!title_moved(&[item(cut, t.version)], &t));
        // Filed before 12-2 (no snapshot): the version rule.
        assert!(title_moved(&[item(json!({}), moved)], &t));
        assert!(!title_moved(&[item(json!({}), t.version)], &t));
    }

    fn title() -> TitleModel {
        serde_json::from_value(json!({
            "id": 7, "title": "T", "original_title": null, "subtitle": "Un  roman",
            "statement_of_responsibility": null, "description": "Ligne\r\nDeux",
            "general_note": null, "language": "fr", "media_type": "book",
            "publication_date": "1987-05-04", "publisher": null,
            "edition_statement": null, "collection_title": null, "collection_number": null,
            "isbn": null, "issn": null, "upc": null, "cover_image_url": null,
            "genre_id": 1, "dewey_code": "843", "page_count": null, "track_count": null,
            "total_duration": null, "age_rating": null, "issue_number": null,
            "manually_edited_fields": "[\"title\"]", "version": 3
        }))
        .unwrap()
    }

    #[test]
    fn parses_scalars_and_contributors() {
        let items = vec![
            row(
                1,
                7,
                json!({"line":"field","field":"subtitle","current":"Un roman","proposed":"Roman"}),
            ),
            row(
                2,
                7,
                json!({"line":"field","field":"publication_year","current":1987,
                             "current_date":"1987-05-04","proposed":1990}),
            ),
            row(
                3,
                7,
                json!({"line":"field","field":"description","current":"x",
                             "current_truncated":true,"proposed":"y"}),
            ),
            row(
                4,
                7,
                json!({"line":"field","field":"contributors","current":[],
                             "proposed":[{"name":"Hergé","role":"Auteur","role_id":2}]}),
            ),
        ];
        let p = parse_items(&items).unwrap();
        assert_eq!(p.title_id, 7);
        assert_eq!(p.scalars.len(), 3);
        assert_eq!(
            p.scalars[0].baseline,
            Baseline::Text(Some("Un roman".into()))
        );
        assert_eq!(
            p.scalars[1].baseline,
            Baseline::Date {
                year: Some(1987),
                date: Some(Some("1987-05-04".into()))
            }
        );
        assert_eq!(p.scalars[2].baseline, Baseline::Unchecked);
        assert_eq!(
            p.contributors,
            Some(vec![StoredContributor {
                name: "Hergé".into(),
                role_id: 2,
                role: "Auteur".into()
            }])
        );
    }

    #[test]
    fn refuses_bad_shapes() {
        let ok = json!({"line":"field","field":"subtitle","current":null,"proposed":"x"});
        let cases: Vec<Vec<ProposalItemRow>> = vec![
            vec![],
            vec![row(1, 7, json!({"line":"cover"}))],
            vec![row(
                1,
                7,
                json!({"line":"field","field":"title","proposed":"x"}),
            )],
            vec![row(1, 7, ok.clone()), row(2, 7, ok.clone())],
            vec![
                row(1, 7, ok.clone()),
                row(
                    2,
                    8,
                    json!({"line":"field","field":"publisher","current":null,"proposed":"x"}),
                ),
            ],
            vec![row(
                1,
                7,
                json!({"line":"field","field":"subtitle","current":3,"proposed":"x"}),
            )],
            vec![row(
                1,
                7,
                json!({"line":"field","field":"subtitle","current":null}),
            )],
            vec![row(
                1,
                7,
                json!({"line":"field","field":"contributors","proposed":[{"name":"A"}]}),
            )],
            vec![ProposalItemRow {
                target: None,
                ..row(1, 7, ok.clone())
            }],
            vec![ProposalItemRow {
                proposed_values_json: "{".into(),
                ..row(1, 7, ok.clone())
            }],
        ];
        for (i, items) in cases.iter().enumerate() {
            assert!(parse_items(items).is_err(), "case {i}");
        }
    }

    #[test]
    fn revalidates_with_the_filing_rules_and_stored_role_ids() {
        let items = vec![
            row(
                1,
                7,
                json!({"line":"field","field":"publisher","current":null,"proposed":"  Casterman "}),
            ),
            row(
                2,
                7,
                json!({"line":"field","field":"contributors","current":[],
                             "proposed":[{"name":"Hergé","role":"Auteur","role_id":2},
                                         {"name":"Hergé","role":"Dessin","role_id":5}]}),
            ),
        ];
        let v = revalidate(&parse_items(&items).unwrap(), 2026).unwrap();
        assert_eq!(v.publisher.as_deref(), Some("Casterman"));
        let list = v.contributors.unwrap();
        assert_eq!(
            list.iter().map(|c| c.role_id).collect::<Vec<_>>(),
            vec![2, 5]
        );
        // A value filing would refuse is refused here too.
        for bad in [
            json!({"line":"field","field":"publication_year","current":null,"proposed":99}),
            json!({"line":"field","field":"dewey_code","current":null,"proposed":"abc"}),
            json!({"line":"field","field":"subtitle","current":null,"proposed":"  "}),
            json!({"line":"field","field":"contributors","current":[],
                   "proposed":[{"name":"A","role":"x","role_id":1},{"name":"a","role":"x","role_id":1}]}),
        ] {
            let p = parse_items(&[row(1, 7, bad.clone())]).unwrap();
            assert!(revalidate(&p, 2026).is_err(), "{bad}");
        }
    }

    #[test]
    fn contributors_pair_by_stored_role_id() {
        let items = vec![row(
            1,
            7,
            json!({"line":"field","field":"contributors","current":[],
                   "proposed":[{"name":"A","role":"","role_id":2},{"name":"B","role":"X","role_id":5}]}),
        )];
        let stored = parse_items(&items).unwrap();
        assert_eq!(stored.contributors.as_ref().unwrap()[0].role, "#2");
        let valid = revalidate(&stored, 2026).unwrap();
        assert!(paired_contributors(&stored, &valid).is_ok());
        let mut other = valid.clone();
        other.contributors.as_mut().unwrap()[1].role_id = 9;
        assert!(paired_contributors(&stored, &other).is_err());
        other.contributors.as_mut().unwrap().pop();
        assert!(paired_contributors(&stored, &other).is_err());
        assert!(paired_contributors(&stored, &ProposedFields::default()).is_err());
    }

    #[test]
    fn compares_in_normal_form_and_by_date() {
        let t = title();
        let s = |field: &'static str, baseline: Baseline| StoredScalar {
            field,
            baseline,
            proposed: json!("x"),
        };
        // Spacing / line endings alone are no change.
        assert!(!field_changed(
            &s("subtitle", Baseline::Text(Some("Un roman".into()))),
            &t
        ));
        assert!(!field_changed(
            &s("description", Baseline::Text(Some("Ligne\nDeux".into()))),
            &t
        ));
        assert!(field_changed(&s("subtitle", Baseline::Text(None)), &t));
        assert!(!field_changed(&s("publisher", Baseline::Text(None)), &t));
        assert!(field_changed(
            &s("publisher", Baseline::Text(Some("X".into()))),
            &t
        ));
        assert!(!field_changed(
            &s("dewey_code", Baseline::Text(Some("843".into()))),
            &t
        ));
        assert!(!field_changed(&s("dewey_code", Baseline::Unchecked), &t));
        let date = |year: Option<i64>, date: Option<Option<String>>| {
            s("publication_year", Baseline::Date { year, date })
        };
        assert!(!field_changed(
            &date(Some(1987), Some(Some("1987-05-04".into()))),
            &t
        ));
        // Same year, other day: the date recorded at filing decides.
        assert!(field_changed(
            &date(Some(1987), Some(Some("1987-01-01".into()))),
            &t
        ));
        assert!(field_changed(&date(None, Some(None)), &t));
        assert!(!field_changed(&date(Some(1987), None), &t));
        assert!(field_changed(&date(Some(1986), None), &t));
    }
}
