//! Story 11-17 (FR138, FR140, FR146) — accepting a series proposal.
//!
//! A `series_create` or `series_attach` proposal (item shapes: module doc
//! of [`crate::services::series_proposals`], D-11-12-2) is applied in
//! **one transaction**:
//!
//! 1. the proposal row is locked and its `version` compared with the one
//!    the librarian saw (optimistic lock);
//! 2. its items are read (share lock) and parsed back into lines;
//! 3. series before titles — the order auto-settle follows (series →
//!    `title_series` insert, which share-locks the title through the
//!    FK): an attach locks its series row; a creation runs its same-name
//!    check as a locking read, which locks the scanned `series` rows and
//!    gaps until the end of the (short) transaction, so two creations of
//!    one name are serialised (#529-2);
//! 4. [`ProposalService::check_staleness_in`] locks the targets and marks
//!    the proposal `stale` when one was changed (version) or deleted;
//! 5. the lines are re-validated against the catalog **as it is now**
//!    (#521, D-11-17-2): `title_series` and expected-tome writes bump no
//!    `version`, so a position taken, a title attached or a tome entered
//!    since filing is found here; a conflict marks the proposal `stale`
//!    too (#529: a second create of the same name, a second attach on the
//!    same positions);
//! 6. [`ProposalService::transition_in`] records the decision (FR146) and
//!    the lines are applied — series, then titles, then expected tomes
//!    (origin `proposal`, auto-settle rules of D-11-10-*).
//!
//! A deadlock is retried (up to three attempts) by [`accept`]. The
//! kind-independent pieces live in [`crate::services::acceptance`].
//!
//! A stale outcome is an `Ok`: the caller commits the mark. Any error is
//! an `Err`: the caller rolls back, nothing is applied and the proposal
//! stays `pending`. No file or network I/O inside (D-11-9-12).
//!
//! Every read before the series lock is a locking read, so no snapshot
//! predates that lock (D-11-12-13 argument).

use serde_json::Value;
use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::expected_tome::{ExpectedTomeModel, ExpectedTomeOrigin, TomeSlot};
use crate::models::proposal::{
    ProposalItemRow, ProposalKind, ProposalModel, ProposalStatus, ProposalTargetTable,
};
use crate::models::series::{HeldPosition, SeriesModel, SeriesType, TitleSeriesModel};
// The shared acceptance types (story 11-18), re-exported for the callers
// of this module.
pub use crate::services::acceptance::{AcceptConflict, AcceptError, AcceptOutcome, StaleCause};
use crate::services::acceptance::{in_retried_transaction, lock_pending_in, mark_stale_in};
use crate::services::expected_tomes::{ExpectedTomeInput, ExpectedTomeService, closed_total_of};
use crate::services::proposals::{Decision, ProposalService, Transition};
use crate::services::series_proposals::{
    SeriesProposalError, TitleLine, TomeLine, ValidLines, check_against_series, validate_lines,
    validate_series_name, validate_total,
};

/// The series line of a stored proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredSeries {
    Create {
        name: String,
        series_type: SeriesType,
        total_volume_count: Option<i64>,
    },
    Attach {
        series_id: u64,
    },
}

/// A stored series proposal, parsed back into lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSeriesProposal {
    pub series: StoredSeries,
    pub titles: Vec<TitleLine>,
    pub tomes: Vec<TomeLine>,
}

fn int(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_i64)
}

/// Parse the stored items of a series proposal (pure — unit tested).
/// The series line must come first and match the kind; title lines
/// target a title; anything else is refused with a short English reason
/// (filing never produces it; 11-19's edits must keep the shapes).
pub fn parse_items(
    kind: ProposalKind,
    items: &[ProposalItemRow],
) -> Result<StoredSeriesProposal, String> {
    let mut iter = items.iter();
    let first = iter.next().ok_or("no items")?;
    let values = |row: &ProposalItemRow| -> Result<Value, String> {
        serde_json::from_str(&row.proposed_values_json)
            .map_err(|e| format!("item {}: invalid JSON: {e}", row.id))
    };
    let head = values(first)?;
    if head.get("line").and_then(Value::as_str) != Some("series") {
        return Err(format!(
            "item {}: the first line is not the series",
            first.id
        ));
    }
    let series = match kind {
        ProposalKind::SeriesCreate => {
            if first.target.is_some() {
                return Err(format!("item {}: a creation targets nothing", first.id));
            }
            let name = head
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("item {}: no series name", first.id))?;
            let series_type = head
                .get("series_type")
                .and_then(Value::as_str)
                .and_then(|t| t.parse::<SeriesType>().ok())
                .ok_or_else(|| format!("item {}: no valid series type", first.id))?;
            let total_volume_count = match head.get("total_volume_count") {
                None | Some(Value::Null) => None,
                Some(v) => Some(
                    v.as_i64()
                        .ok_or_else(|| format!("item {}: total is not an integer", first.id))?,
                ),
            };
            StoredSeries::Create {
                name: name.to_string(),
                series_type,
                total_volume_count,
            }
        }
        ProposalKind::SeriesAttach => match first.target {
            Some(t) if t.table == ProposalTargetTable::Series => {
                StoredSeries::Attach { series_id: t.id }
            }
            _ => return Err(format!("item {}: an attach targets its series", first.id)),
        },
        other => {
            return Err(format!(
                "a {} proposal is not a series proposal",
                other.as_str()
            ));
        }
    };

    let mut titles = Vec::new();
    let mut tomes = Vec::new();
    for row in iter {
        let v = values(row)?;
        match v.get("line").and_then(Value::as_str) {
            Some("title") => {
                let title_id = match row.target {
                    Some(t) if t.table == ProposalTargetTable::Titles => t.id,
                    _ => return Err(format!("item {}: a title line targets a title", row.id)),
                };
                let position =
                    int(&v, "position").ok_or_else(|| format!("item {}: no position", row.id))?;
                let is_omnibus = v
                    .get("is_omnibus")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                titles.push(TitleLine {
                    title_id,
                    position,
                    is_omnibus,
                    end_position: int(&v, "end_position"),
                    version: None,
                });
            }
            Some("expected_tome") => {
                let position =
                    int(&v, "position").ok_or_else(|| format!("item {}: no position", row.id))?;
                let title = v
                    .get("title")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("item {}: no tome title", row.id))?;
                let isbn = match v.get("isbn") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => Some(s.clone()),
                    Some(_) => return Err(format!("item {}: the ISBN is not a string", row.id)),
                };
                tomes.push(TomeLine {
                    position,
                    title: title.to_string(),
                    isbn,
                });
            }
            _ => return Err(format!("item {}: unknown line", row.id)),
        }
    }
    Ok(StoredSeriesProposal {
        series,
        titles,
        tomes,
    })
}

impl AcceptConflict {
    /// The conflict of a re-validation error, named from `held` (the
    /// series' current holders). `None` for an error that is not a state
    /// conflict.
    pub fn from_check(e: &SeriesProposalError, held: &[HeldPosition]) -> Option<Self> {
        use SeriesProposalError as E;
        Some(match e {
            E::TitleAlreadyInSeries { title_id, position } => AcceptConflict::TitleInSeries {
                title: held
                    .iter()
                    .find(|h| h.title_id == *title_id)
                    .map(|h| h.title.clone())
                    .unwrap_or_else(|| format!("#{title_id}")),
                position: *position,
            },
            E::PositionHeld {
                position, holder, ..
            } => AcceptConflict::PositionHeld {
                position: *position,
                holder: holder.clone(),
            },
            E::TomePositionTaken {
                position,
                tome_title,
            } => AcceptConflict::TomePositionTaken {
                position: *position,
                tome_title: tome_title.clone(),
            },
            E::TomeIsbnTaken { isbn, tome_title } => AcceptConflict::TomeIsbnTaken {
                isbn: isbn.clone(),
                tome_title: tome_title.clone(),
            },
            E::SeriesExists { name, .. } => AcceptConflict::SeriesExists { name: name.clone() },
            _ => return None,
        })
    }
}

/// A tome line not created because its book is already in the series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedTome {
    pub position: i32,
    pub title: String,
    pub owner_title: String,
    pub owner_position: i32,
}

/// What an acceptance applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedSeries {
    pub series_id: u64,
    pub series_name: String,
    /// `true` for a `series_create`.
    pub created: bool,
    pub titles_attached: usize,
    pub tomes_created: usize,
    /// Of the created tomes, those settled at once by a title carrying
    /// their ISBN.
    pub tomes_settled: usize,
    pub tomes_skipped: Vec<SkippedTome>,
}

/// A `series_create` once its series line is validated.
struct ValidCreate {
    name: String,
    series_type: SeriesType,
    total: Option<i32>,
    /// The stored name of an active series that already has this name.
    existing: Option<String>,
}

/// The positions of validated lines above `total` (pure): the first
/// one, as a conflict.
fn beyond_total(lines: &ValidLines, total: Option<i32>) -> Option<AcceptConflict> {
    let total = total?;
    lines
        .titles
        .iter()
        .map(|t| t.end())
        .chain(lines.tomes.iter().map(|t| t.position))
        .find(|p| *p > total)
        .map(|position| AcceptConflict::BeyondTotal { position, total })
}

/// Accept proposal `id` on behalf of `user_id`, in its own transaction:
/// committed when the outcome is `Accepted` or `Stale`, rolled back on
/// any error. A deadlock (concurrent scan-settle or hand assignment, or
/// two creations of one name on an empty `series` table) is retried
/// (`acceptance::in_retried_transaction`).
pub async fn accept(
    pool: &DbPool,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome, AcceptError> {
    in_retried_transaction(pool, id, user_id, "series", |conn| {
        Box::pin(accept_in(conn, id, user_id, version))
    })
    .await
}

/// [`accept`] on a caller-held transaction (see the module doc for the
/// order). On `Ok` the caller commits; on `Err` it rolls back.
async fn accept_in(
    conn: &mut MySqlConnection,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome, AcceptError> {
    let (kind, items) = lock_pending_in(
        conn,
        id,
        version,
        &[ProposalKind::SeriesCreate, ProposalKind::SeriesAttach],
    )
    .await?;
    let stored = parse_items(kind, &items).map_err(AcceptError::Invalid)?;

    // Series before titles (module doc, step 3). An attach locks its
    // series row; a creation runs its same-name check as a locking read
    // (no index serves the comparison, so it locks the scanned `series`
    // rows and gaps): two creations of one name are serialised, and the
    // second sees the first's series once it commits (#529-2).
    let invalid = |e: SeriesProposalError| AcceptError::Invalid(e.to_string());
    let (shape, create) = match &stored.series {
        StoredSeries::Attach { series_id } => (
            ExpectedTomeModel::lock_series_in(conn, *series_id).await?,
            None,
        ),
        StoredSeries::Create {
            name,
            series_type,
            total_volume_count,
        } => {
            let name = validate_series_name(name).map_err(invalid)?;
            let total = validate_total(*series_type, *total_volume_count).map_err(invalid)?;
            let existing = ProposalModel::lock_series_named(conn, &name).await?;
            (
                None,
                Some(ValidCreate {
                    name,
                    series_type: *series_type,
                    total,
                    existing,
                }),
            )
        }
    };
    let check = ProposalService::check_staleness_in(conn, id).await?;
    if check.status == ProposalStatus::Stale {
        return Ok(AcceptOutcome::Stale(StaleCause::Targets(
            check.stale_targets,
        )));
    }

    // Re-validation against the catalog as it is now (step 5).
    let (lines, conflict) = match (&stored.series, &create, &shape) {
        (_, Some(c), _) => {
            let lines = validate_lines(&stored.titles, &stored.tomes, c.total).map_err(invalid)?;
            let conflict = c
                .existing
                .clone()
                .map(|name| AcceptConflict::SeriesExists { name });
            (lines, conflict)
        }
        (StoredSeries::Attach { series_id }, None, Some(shape)) => {
            let lines = validate_lines(&stored.titles, &stored.tomes, None).map_err(invalid)?;
            let total = closed_total_of(shape.series_type, shape.total_volume_count);
            let conflict = match beyond_total(&lines, total) {
                Some(c) => Some(c),
                None => {
                    let held = TitleSeriesModel::held_positions_in(conn, *series_id).await?;
                    let slots: Vec<TomeSlot> =
                        ExpectedTomeModel::live_slots_in(conn, *series_id).await?;
                    match check_against_series(&lines, &held, &slots) {
                        Ok(()) => None,
                        Err(e) => Some(
                            AcceptConflict::from_check(&e, &held)
                                .ok_or_else(|| AcceptError::Invalid(e.to_string()))?,
                        ),
                    }
                }
            };
            (lines, conflict)
        }
        // Not stale, so an attach's series is live and was locked above.
        _ => {
            return Err(AcceptError::Apply(AppError::Internal(
                "attach series not locked although not stale".to_string(),
            )));
        }
    };
    if let Some(conflict) = conflict {
        return mark_stale_in(conn, id, conflict).await;
    }

    // The decision (FR146), then the apply (step 6).
    match ProposalService::transition_in(conn, id, &Decision::Accept { by: user_id }).await? {
        Transition::Decided(_) => {}
        // Unreachable: the same locks are held since the check above.
        Transition::WentStale(targets) => {
            return Ok(AcceptOutcome::Stale(StaleCause::Targets(targets)));
        }
    }
    let (series_id, series_name, created) = match (create, &shape) {
        // The validated name and total, as checked above.
        (Some(c), _) => {
            let sid = SeriesModel::create_in(conn, &c.name, None, c.series_type, c.total).await?;
            ProposalModel::set_applied_series_in(conn, id, sid).await?;
            (sid, c.name, true)
        }
        (None, Some(shape)) => (shape.id, shape.name.clone(), false),
        (None, None) => {
            return Err(AcceptError::Apply(AppError::Internal(
                "attach series not locked although not stale".to_string(),
            )));
        }
    };

    for t in &lines.titles {
        match t.end_position {
            Some(end) => {
                TitleSeriesModel::assign_omnibus_in(conn, t.title_id, series_id, t.position, end)
                    .await?
            }
            None => {
                TitleSeriesModel::assign_in(conn, t.title_id, series_id, t.position).await?;
            }
        }
    }

    let mut applied = AppliedSeries {
        series_id,
        series_name,
        created,
        titles_attached: lines.titles.len(),
        tomes_created: 0,
        tomes_settled: 0,
        tomes_skipped: Vec::new(),
    };
    for t in &lines.tomes {
        if let Some(isbn) = &t.isbn
            && let Some((owner, owner_position)) =
                ExpectedTomeService::owner_elsewhere_in(conn, series_id, t.position, isbn).await?
        {
            applied.tomes_skipped.push(SkippedTome {
                position: t.position,
                title: t.title.clone(),
                owner_title: ExpectedTomeModel::live_title_name_in(conn, owner)
                    .await?
                    .unwrap_or_else(|| format!("#{owner}")),
                owner_position,
            });
            continue;
        }
        let input = ExpectedTomeInput {
            position: t.position,
            title: &t.title,
            isbn: t.isbn.as_deref(),
        };
        let tome_id =
            ExpectedTomeService::create_in(conn, series_id, &input, ExpectedTomeOrigin::Proposal)
                .await?;
        applied.tomes_created += 1;
        if ExpectedTomeModel::find_in(conn, tome_id, false)
            .await?
            .is_some_and(|r| r.is_settled())
        {
            applied.tomes_settled += 1;
        }
    }
    Ok(AcceptOutcome::Accepted(applied))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::proposal::ProposalTarget;
    use crate::services::series_proposals::{ValidTitle, ValidTome};
    use serde_json::json;

    fn row(id: u64, target: Option<(ProposalTargetTable, u64)>, v: Value) -> ProposalItemRow {
        ProposalItemRow {
            id,
            proposal_id: 1,
            target: target.map(|(table, id)| ProposalTarget { table, id }),
            target_version: target.map(|_| 1),
            proposed_values_json: v.to_string(),
            version: 1,
        }
    }

    #[test]
    fn parses_a_create_with_titles_and_tomes() {
        let items = vec![
            row(
                1,
                None,
                json!({"line":"series","name":"Astérix","series_type":"closed","total_volume_count":5}),
            ),
            row(
                2,
                Some((ProposalTargetTable::Titles, 7)),
                json!({"line":"title","position":1,"is_omnibus":false}),
            ),
            row(
                3,
                Some((ProposalTargetTable::Titles, 8)),
                json!({"line":"title","position":2,"is_omnibus":true,"end_position":3}),
            ),
            row(
                4,
                None,
                json!({"line":"expected_tome","position":4,"title":"T4","isbn":"9782070360246"}),
            ),
            row(
                5,
                None,
                json!({"line":"expected_tome","position":5,"title":"T5","isbn":null}),
            ),
        ];
        let p = parse_items(ProposalKind::SeriesCreate, &items).unwrap();
        assert_eq!(
            p.series,
            StoredSeries::Create {
                name: "Astérix".into(),
                series_type: SeriesType::Closed,
                total_volume_count: Some(5)
            }
        );
        assert_eq!(p.titles.len(), 2);
        assert_eq!(p.titles[1].end_position, Some(3));
        assert!(p.titles[1].is_omnibus);
        assert_eq!(p.tomes[0].isbn.as_deref(), Some("9782070360246"));
        assert_eq!(p.tomes[1].isbn, None);
    }

    #[test]
    fn parses_an_attach_open_create_and_refuses_bad_shapes() {
        let attach = vec![row(
            1,
            Some((ProposalTargetTable::Series, 9)),
            json!({"line":"series"}),
        )];
        assert_eq!(
            parse_items(ProposalKind::SeriesAttach, &attach)
                .unwrap()
                .series,
            StoredSeries::Attach { series_id: 9 }
        );
        let open = vec![row(
            1,
            None,
            json!({"line":"series","name":"O","series_type":"open","total_volume_count":null}),
        )];
        assert!(matches!(
            parse_items(ProposalKind::SeriesCreate, &open)
                .unwrap()
                .series,
            StoredSeries::Create {
                total_volume_count: None,
                ..
            }
        ));
        // Attach without a series target, create with one, wrong kind,
        // title line without target, unknown line, empty, bad JSON.
        let cases: Vec<(ProposalKind, Vec<ProposalItemRow>)> = vec![
            (
                ProposalKind::SeriesAttach,
                vec![row(1, None, json!({"line":"series"}))],
            ),
            (ProposalKind::SeriesCreate, attach.clone()),
            (ProposalKind::Metadata, attach.clone()),
            (
                ProposalKind::SeriesAttach,
                vec![
                    attach[0].clone(),
                    row(2, None, json!({"line":"title","position":1})),
                ],
            ),
            (
                ProposalKind::SeriesAttach,
                vec![attach[0].clone(), row(2, None, json!({"line":"cover"}))],
            ),
            (ProposalKind::SeriesAttach, vec![]),
            (
                ProposalKind::SeriesAttach,
                vec![ProposalItemRow {
                    proposed_values_json: "{".into(),
                    ..attach[0].clone()
                }],
            ),
            (
                ProposalKind::SeriesAttach,
                vec![
                    row(
                        2,
                        None,
                        json!({"line":"expected_tome","position":1,"title":"x"}),
                    ),
                    attach[0].clone(),
                ],
            ),
        ];
        for (i, (kind, items)) in cases.iter().enumerate() {
            assert!(parse_items(*kind, items).is_err(), "case {i}");
        }
    }

    fn lines(titles: &[(u64, i32, Option<i32>)], tomes: &[i32]) -> ValidLines {
        ValidLines {
            titles: titles
                .iter()
                .map(|&(title_id, position, end_position)| ValidTitle {
                    title_id,
                    position,
                    end_position,
                    version: None,
                })
                .collect(),
            tomes: tomes
                .iter()
                .map(|&position| ValidTome {
                    position,
                    title: format!("T{position}"),
                    isbn: None,
                })
                .collect(),
        }
    }

    #[test]
    fn beyond_total_checks_omnibus_ends_and_tomes() {
        assert_eq!(beyond_total(&lines(&[(1, 1, Some(4))], &[2]), None), None);
        assert_eq!(
            beyond_total(&lines(&[(1, 1, Some(3))], &[2]), Some(3)),
            None
        );
        assert_eq!(
            beyond_total(&lines(&[(1, 1, Some(4))], &[2]), Some(3)),
            Some(AcceptConflict::BeyondTotal {
                position: 4,
                total: 3
            })
        );
        assert_eq!(
            beyond_total(&lines(&[], &[5]), Some(3)),
            Some(AcceptConflict::BeyondTotal {
                position: 5,
                total: 3
            })
        );
    }

    #[test]
    fn conflicts_are_named_from_the_catalog() {
        let held = vec![HeldPosition {
            title_id: 7,
            position: 2,
            title: "Le Lotus bleu".into(),
        }];
        let l = lines(&[(7, 4, None)], &[]);
        let e = check_against_series(&l, &held, &[]).unwrap_err();
        assert_eq!(
            AcceptConflict::from_check(&e, &held),
            Some(AcceptConflict::TitleInSeries {
                title: "Le Lotus bleu".into(),
                position: 2
            })
        );
        let l = lines(&[(8, 1, Some(3))], &[]);
        let e = check_against_series(&l, &held, &[]).unwrap_err();
        assert_eq!(
            AcceptConflict::from_check(&e, &held),
            Some(AcceptConflict::PositionHeld {
                position: 2,
                holder: "Le Lotus bleu".into()
            })
        );
        let slots = vec![TomeSlot {
            position: 5,
            title: "Tome 5".into(),
            isbn: Some("9782070360246".into()),
        }];
        let l = lines(&[], &[5]);
        let e = check_against_series(&l, &[], &slots).unwrap_err();
        assert_eq!(
            AcceptConflict::from_check(&e, &[]),
            Some(AcceptConflict::TomePositionTaken {
                position: 5,
                tome_title: "Tome 5".into()
            })
        );
        let mut l = lines(&[], &[6]);
        l.tomes[0].isbn = Some("9782070360246".into());
        let e = check_against_series(&l, &[], &slots).unwrap_err();
        assert_eq!(
            AcceptConflict::from_check(&e, &[]),
            Some(AcceptConflict::TomeIsbnTaken {
                isbn: "9782070360246".into(),
                tome_title: "Tome 5".into()
            })
        );
        assert!(check_against_series(&lines(&[(9, 3, None)], &[4]), &held, &slots).is_ok());
        assert_eq!(
            AcceptConflict::from_check(&SeriesProposalError::NameRequired, &[]),
            None
        );
    }
}
