//! Story 11-15 (FR135, FR136, AR30) — an agent's proposal history and
//! the resubmission delay for rejected proposals.
//!
//! ## Equivalent proposals (FR136)
//!
//! Two proposals are equivalent when they share the kind and the target
//! ([`ProposalSubject`]):
//!
//! - `series_create` — the proposed series name, folded like
//!   [`fold_name`] (case, accents, inner whitespace ignored);
//! - `series_attach` — the series id (the series line's target);
//! - `metadata` — the title id **and** the set of fields (the items'
//!   `field` keys, order ignored); since story 12-2 (#535) a rejection
//!   blocks every filing that **shares a field** with it
//!   ([`newest_overlapping_rejection`]), and a rejection records the key
//!   of its live lines (a line removed before the decision is not
//!   blocked);
//! - `cover` — the title id and the image's SHA-256 (`fingerprint`,
//!   story 12-2): a rejection blocks only the same image; a cover filed
//!   before has the legacy key `cover:titles/{id}` and blocks nothing new.
//!
//! The subject is read from the items as filed (the shapes documented in
//! `series_proposals`, `metadata_proposals`, `cover_proposals`) and
//! stored on the proposal as `equivalence_key`; later item edits (11-19)
//! do not change it. Equivalence is library-wide: a rejection of another
//! key's proposal blocks too, so a new key does not reset the delay.
//!
//! ## The delay
//!
//! [`ProposalService::file_in`](crate::services::proposals::ProposalService::file_in)
//! calls [`check_resubmission`] right after locking the filing key (the
//! lock order of story 11-12): an equivalent live proposal **rejected**
//! less than the delay ago refuses the filing with
//! [`ProposalError::RecentlyRejected`]. The delay is the `settings` row
//! `proposal_resubmission_delay_days` (Admin → System), read in the
//! filing transaction, so a change applies to the very next filing.
//! Instants come from the database clock (sessions run in UTC).

use std::collections::{BTreeSet, HashMap};

use chrono::{Duration, NaiveDateTime};
use serde_json::{Value, json};
use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::PaginatedList;
use crate::models::proposal::{
    LatestRejection,
    ProposalFilter, ProposalKind, ProposalModel, ProposalStatus, ProposalTarget,
    ProposalTargetTable, SubjectLineRow,
};
use crate::services::metadata_proposals::fold_name;
use crate::services::proposals::{NewProposal, ProposalError};

/// The `settings` key of the delay.
pub const KEY_RESUBMISSION_DELAY_DAYS: &str = "proposal_resubmission_delay_days";
/// Default delay (FR136).
pub const DEFAULT_RESUBMISSION_DELAY_DAYS: u32 = 90;
/// Longest delay an admin may set (ten years); 0 disables the rule.
pub const MAX_RESUBMISSION_DELAY_DAYS: u32 = 3650;
/// Width of `proposals.equivalence_key`.
pub const MAX_EQUIVALENCE_KEY_CHARS: usize = 700;

/// The stored delay; a missing, unreadable or out-of-range value reads
/// as the default (logged).
pub fn parse_resubmission_delay_days(raw: Option<&str>) -> u32 {
    let Some(raw) = raw else {
        return DEFAULT_RESUBMISSION_DELAY_DAYS;
    };
    match raw.trim().parse::<u32>() {
        Ok(v) if v <= MAX_RESUBMISSION_DELAY_DAYS => v,
        _ => {
            tracing::warn!(
                key = KEY_RESUBMISSION_DELAY_DAYS,
                value = %raw,
                "invalid resubmission delay (0..={MAX_RESUBMISSION_DELAY_DAYS}), using default"
            );
            DEFAULT_RESUBMISSION_DELAY_DAYS
        }
    }
}

/// The current delay, read from `settings` on the given connection.
pub async fn resubmission_delay_days(conn: &mut MySqlConnection) -> Result<u32, AppError> {
    let raw = ProposalModel::setting_value(conn, KEY_RESUBMISSION_DELAY_DAYS).await?;
    Ok(parse_resubmission_delay_days(raw.as_deref()))
}

/// The instant from which a proposal equivalent to one rejected at
/// `rejected_at` is accepted again.
pub fn resubmission_open_from(rejected_at: NaiveDateTime, delay_days: u32) -> NaiveDateTime {
    rejected_at + Duration::days(i64::from(delay_days))
}

/// `Some(open_from)` when a rejection at `rejected_at` still blocks an
/// equivalent filing at `now`; `None` once `now` reaches the reopening
/// instant (or the delay is 0).
pub fn blocked_until(
    rejected_at: NaiveDateTime,
    now: NaiveDateTime,
    delay_days: u32,
) -> Option<NaiveDateTime> {
    let open_from = resubmission_open_from(rejected_at, delay_days);
    (now < open_from).then_some(open_from)
}

/// An instant as RFC 3339 UTC (database sessions run in UTC).
pub fn rfc3339_utc(at: NaiveDateTime) -> String {
    at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// The parts of an item that say what a proposal is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SubjectLine<'a> {
    pub target: Option<ProposalTarget>,
    /// `proposed_values.line`.
    pub line: Option<&'a str>,
    /// `proposed_values.field` (metadata items).
    pub field: Option<&'a str>,
    /// `proposed_values.name` (the series line of a creation).
    pub name: Option<&'a str>,
    /// `proposed_values.fingerprint` (cover items, story 12-2).
    pub fingerprint: Option<&'a str>,
}

impl<'a> SubjectLine<'a> {
    /// The subject parts of an item read back by
    /// [`ProposalModel::subject_lines`].
    pub fn from_row(row: &'a SubjectLineRow) -> Self {
        SubjectLine {
            target: row.target,
            line: row.line.as_deref(),
            field: row.field.as_deref(),
            name: row.name.as_deref(),
            fingerprint: row.fingerprint.as_deref(),
        }
    }

    /// Read the subject parts of an item's `proposed_values`.
    pub fn of(target: Option<ProposalTarget>, values: &'a Value) -> Self {
        let s = |k: &str| values.get(k).and_then(Value::as_str);
        SubjectLine {
            target,
            line: s("line"),
            field: s("field"),
            name: s("name"),
            fingerprint: s("fingerprint"),
        }
    }
}

/// A series name folded for the equivalence key: compatibility forms
/// first (NFKC: fullwidth letters, ligatures), then [`fold_name`]
/// (case, accents, inner whitespace). Changing it changes stored keys:
/// the unit test `series_create_key_folds_the_name` pins it.
fn fold_key_name(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    fold_name(&name.nfkc().collect::<String>())
}

/// What a proposal is about: its kind and target (FR136).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalSubject {
    /// The name as proposed (the key folds it).
    SeriesCreate {
        name: String,
    },
    SeriesAttach {
        series_id: u64,
    },
    Metadata {
        title_id: u64,
        fields: BTreeSet<String>,
    },
    /// `fingerprint`: the proposed image's SHA-256 (story 12-2, #535 —
    /// a rejection blocks only the same image); `None` for a cover
    /// proposal filed before it.
    Cover {
        title_id: u64,
        fingerprint: Option<String>,
    },
}

fn target_of(line: &SubjectLine<'_>, table: ProposalTargetTable) -> Option<u64> {
    line.target.filter(|t| t.table == table).map(|t| t.id)
}

impl ProposalSubject {
    /// The subject of a proposal of `kind` with these items; `None` when
    /// the items do not have the kind's shape (no equivalence then).
    pub fn from_lines(kind: ProposalKind, lines: &[SubjectLine<'_>]) -> Option<Self> {
        match kind {
            ProposalKind::SeriesCreate => lines
                .iter()
                .find(|l| l.line == Some("series") && l.target.is_none())
                .and_then(|l| l.name)
                .map(str::trim)
                .filter(|n| !fold_key_name(n).is_empty())
                .map(|n| ProposalSubject::SeriesCreate {
                    name: n.to_string(),
                }),
            ProposalKind::SeriesAttach => lines
                .iter()
                .find(|l| l.line == Some("series"))
                .and_then(|l| target_of(l, ProposalTargetTable::Series))
                .map(|series_id| ProposalSubject::SeriesAttach { series_id }),
            ProposalKind::Metadata => {
                let mut title_id = None;
                let mut fields = BTreeSet::new();
                for l in lines.iter().filter(|l| l.line == Some("field")) {
                    let id = target_of(l, ProposalTargetTable::Titles)?;
                    if *title_id.get_or_insert(id) != id {
                        return None;
                    }
                    fields.insert(l.field?.to_string());
                }
                Some(ProposalSubject::Metadata {
                    title_id: title_id?,
                    fields,
                })
            }
            ProposalKind::Cover => lines
                .iter()
                .find(|l| l.line == Some("cover"))
                .and_then(|l| {
                    target_of(l, ProposalTargetTable::Titles).map(|title_id| {
                        ProposalSubject::Cover {
                            title_id,
                            fingerprint: l.fingerprint.map(str::to_string),
                        }
                    })
                }),
        }
    }

    /// The subject of a proposal about to be filed.
    pub fn of_new(proposal: &NewProposal) -> Option<Self> {
        let lines: Vec<SubjectLine<'_>> = proposal
            .items
            .iter()
            .map(|i| SubjectLine::of(i.target, &i.proposed_values))
            .collect();
        Self::from_lines(proposal.kind, &lines)
    }

    /// The stored equivalence key (see the migration
    /// `20261029120000_proposals_equivalence_key.sql`), at most
    /// [`MAX_EQUIVALENCE_KEY_CHARS`] characters.
    pub fn equivalence_key(&self) -> String {
        let key = match self {
            ProposalSubject::SeriesCreate { name } => {
                format!("series_create:{}", fold_key_name(name))
            }
            ProposalSubject::SeriesAttach { series_id } => {
                format!("series_attach:series/{series_id}")
            }
            ProposalSubject::Metadata { title_id, fields } => format!(
                "metadata:titles/{title_id}:{}",
                fields
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            ProposalSubject::Cover {
                title_id,
                fingerprint: Some(fp),
            } => format!("cover:titles/{title_id}:{fp}"),
            // Filed before story 12-2: blocks only legacy-keyed filings,
            // i.e. nothing filed since (intended — #535).
            ProposalSubject::Cover {
                title_id,
                fingerprint: None,
            } => format!("cover:titles/{title_id}"),
        };
        key.chars().take(MAX_EQUIVALENCE_KEY_CHARS).collect()
    }

    /// The subject as the history shows it to the agent.
    pub fn to_json(&self) -> Value {
        match self {
            ProposalSubject::SeriesCreate { name } => json!({ "series_name": name }),
            ProposalSubject::SeriesAttach { series_id } => json!({ "series_id": series_id }),
            ProposalSubject::Metadata { title_id, fields } => {
                json!({ "title_id": title_id, "fields": fields })
            }
            ProposalSubject::Cover { title_id, .. } => json!({ "title_id": title_id }),
        }
    }

    /// An English phrase naming the subject (agent-facing messages).
    pub fn describe(&self) -> String {
        match self {
            ProposalSubject::SeriesCreate { name } => {
                format!("creation of the series {}", Value::String(name.clone()))
            }
            ProposalSubject::SeriesAttach { series_id } => {
                format!("additions to series {series_id}")
            }
            ProposalSubject::Metadata { title_id, fields } => format!(
                "{} of title {title_id}",
                fields
                    .iter()
                    .map(|f| format!("`{f}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ProposalSubject::Cover { title_id, .. } => {
                format!("this cover image for title {title_id}")
            }
        }
    }
}

/// `metadata:titles/{id}:` — the key prefix of every metadata proposal on
/// a title.
fn metadata_key_prefix(title_id: u64) -> String {
    format!("metadata:titles/{title_id}:")
}

/// The field set recorded in a metadata equivalence key (the part after
/// [`metadata_key_prefix`]); empty when the key has another shape.
fn metadata_key_fields<'k>(key: &'k str, prefix: &str) -> BTreeSet<&'k str> {
    key.strip_prefix(prefix)
        .map(|rest| rest.split(',').filter(|f| !f.is_empty()).collect())
        .unwrap_or_default()
}

/// The title and the field set of a metadata equivalence key; `None` for
/// any other key.
fn parse_metadata_key(key: &str) -> Option<(u64, BTreeSet<String>)> {
    let rest = key.strip_prefix("metadata:titles/")?;
    let (id, _) = rest.split_once(':')?;
    let title_id: u64 = id.parse().ok()?;
    let fields = metadata_key_fields(key, &metadata_key_prefix(title_id))
        .into_iter()
        .map(str::to_string)
        .collect();
    Some((title_id, fields))
}

/// Story 12-2 (#535, FR136): a rejected metadata proposal blocks each of
/// its fields. The newest live rejection on `title_id` whose fields meet
/// `fields`, with the fields they share (empty set: nothing overlaps).
async fn newest_overlapping_rejection(
    conn: &mut MySqlConnection,
    title_id: u64,
    fields: &BTreeSet<String>,
) -> Result<Option<(LatestRejection, BTreeSet<String>)>, AppError> {
    let prefix = metadata_key_prefix(title_id);
    let rows = ProposalModel::rejections_with_key_prefix(conn, &prefix).await?;
    Ok(newest_overlap(rows, &prefix, fields))
}

/// The first of `rows` (newest first) whose key shares a field with
/// `fields`, with the shared fields (pure — unit tested).
fn newest_overlap<R>(
    rows: Vec<(String, R)>,
    prefix: &str,
    fields: &BTreeSet<String>,
) -> Option<(R, BTreeSet<String>)> {
    rows.into_iter().find_map(|(key, rejection)| {
        let overlap: BTreeSet<String> = metadata_key_fields(&key, prefix)
            .into_iter()
            .filter(|f| fields.contains(*f))
            .map(str::to_string)
            .collect();
        (!overlap.is_empty()).then_some((rejection, overlap))
    })
}

/// The rejection that blocks a filing (FR136).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockingRejection {
    /// What the refused proposal is about (described in English).
    pub subject: String,
    pub rejected_at: NaiveDateTime,
    pub open_from: NaiveDateTime,
    pub delay_days: u32,
    /// The rejected proposal's id and reason when the same key filed it
    /// (another key's proposal is not shown to this agent).
    pub own: Option<(u64, Option<String>)>,
}

/// Refuse the filing of `proposal` when an equivalent live proposal was
/// rejected less than the delay ago. Call on the filing transaction,
/// after the filing key is locked. Returns the key to store (`None`
/// when the items match no subject shape).
pub async fn check_resubmission(
    conn: &mut MySqlConnection,
    proposal: &NewProposal,
) -> Result<Option<String>, ProposalError> {
    let Some(subject) = ProposalSubject::of_new(proposal) else {
        return Ok(None);
    };
    let key = subject.equivalence_key();
    let delay_days = resubmission_delay_days(conn).await?;
    if delay_days == 0 {
        return Ok(Some(key));
    }
    // Metadata: any rejection sharing a field blocks (story 12-2, #535);
    // the other kinds: the same key.
    let (last, blocked_subject) = match &subject {
        ProposalSubject::Metadata { title_id, fields } => {
            match newest_overlapping_rejection(conn, *title_id, fields).await? {
                Some((last, overlap)) => (
                    last,
                    ProposalSubject::Metadata {
                        title_id: *title_id,
                        fields: overlap,
                    },
                ),
                None => return Ok(Some(key)),
            }
        }
        _ => match ProposalModel::latest_rejection(conn, &key).await? {
            Some(last) => (last, subject.clone()),
            None => return Ok(Some(key)),
        },
    };
    match blocked_until(last.decided_at, last.now, delay_days) {
        None => Ok(Some(key)),
        Some(open_from) => Err(ProposalError::RecentlyRejected(Box::new(
            BlockingRejection {
                subject: blocked_subject.describe(),
                rejected_at: last.decided_at,
                open_from,
                delay_days,
                own: (last.api_key_id == proposal.api_key_id)
                    .then_some((last.proposal_id, last.rejection_reason)),
            },
        ))),
    }
}

/// One proposal of the agent's history (FR135).
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub proposal_id: u64,
    pub kind: ProposalKind,
    pub status: ProposalStatus,
    pub filed_at: NaiveDateTime,
    pub decided_at: Option<NaiveDateTime>,
    pub rejection_reason: Option<String>,
    pub item_count: i64,
    /// `None` when the live items match no subject shape.
    pub subject: Option<ProposalSubject>,
    /// For a rejected proposal: from when an equivalent one is accepted
    /// again — the newest live equivalent rejection (any key) plus the
    /// current delay (may be in the past); `None` when the proposal has
    /// no equivalence key (it never blocks).
    pub resubmittable_from: Option<NaiveDateTime>,
}

pub struct ProposalHistoryService;

impl ProposalHistoryService {
    /// One page (25) of the live proposals filed by `api_key_id`, newest
    /// first, optionally filtered by status and kind.
    pub async fn list_mine(
        pool: &DbPool,
        api_key_id: u64,
        status: Option<ProposalStatus>,
        kind: Option<ProposalKind>,
        page: u32,
    ) -> Result<PaginatedList<HistoryEntry>, AppError> {
        let filter = ProposalFilter {
            status,
            kind,
            api_key_id: Some(api_key_id),
        };
        let page = page.max(1);
        let (rows, total) = ProposalModel::list(pool, &filter, page).await?;
        let ids: Vec<u64> = rows.iter().map(|r| r.id).collect();
        let mut conn = pool.acquire().await?;
        let lines = ProposalModel::subject_lines(&mut conn, &ids).await?;
        let delay_days = resubmission_delay_days(&mut conn).await?;
        let keys: Vec<&str> = rows
            .iter()
            .filter(|r| r.status == ProposalStatus::Rejected)
            .filter_map(|r| r.equivalence_key.as_deref())
            .collect();
        let latest = ProposalModel::latest_rejection_dates(&mut conn, &keys).await?;
        // Story 12-2 (#535): a rejected metadata proposal reopens when no
        // live rejection sharing one of its fields blocks any more.
        // One prefix query per title on the page, not per row.
        let mut overlapping: HashMap<u64, NaiveDateTime> = HashMap::new();
        let mut by_title: HashMap<u64, Vec<(String, NaiveDateTime)>> = HashMap::new();
        for r in rows.iter().filter(|r| r.status == ProposalStatus::Rejected) {
            let Some((title_id, fields)) =
                r.equivalence_key.as_deref().and_then(parse_metadata_key)
            else {
                continue;
            };
            if let std::collections::hash_map::Entry::Vacant(slot) = by_title.entry(title_id) {
                let prefix = metadata_key_prefix(title_id);
                let found = ProposalModel::rejections_with_key_prefix(&mut conn, &prefix)
                    .await?
                    .into_iter()
                    .map(|(k, last)| (k, last.decided_at))
                    .collect();
                slot.insert(found);
            }
            let rejections = by_title[&title_id].clone();
            if let Some((at, _)) =
                newest_overlap(rejections, &metadata_key_prefix(title_id), &fields)
            {
                overlapping.insert(r.id, at);
            }
        }
        drop(conn);

        let items = rows
            .into_iter()
            .map(|r| {
                let own: Vec<SubjectLine<'_>> = lines
                    .iter()
                    .filter(|l| l.proposal_id == r.id)
                    .map(SubjectLine::from_row)
                    .collect();
                HistoryEntry {
                    subject: ProposalSubject::from_lines(r.kind, &own),
                    // What the filing check enforces: the newest live
                    // equivalent rejection (any key), not this row's own.
                    resubmittable_from: (r.status == ProposalStatus::Rejected)
                        .then_some(r.equivalence_key.as_ref())
                        .flatten()
                        .and_then(|k| overlapping.get(&r.id).or_else(|| latest.get(k)))
                        .map(|at| resubmission_open_from(*at, delay_days)),
                    proposal_id: r.id,
                    kind: r.kind,
                    status: r.status,
                    filed_at: r.created_at,
                    decided_at: r.decided_at,
                    rejection_reason: r.rejection_reason,
                    item_count: r.item_count,
                }
            })
            .collect();
        Ok(PaginatedList::new(items, page, total, None, None, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::proposals::NewProposalItem;
    use chrono::NaiveDate;

    fn at(d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
    }

    fn title(id: u64) -> Option<ProposalTarget> {
        Some(ProposalTarget {
            table: ProposalTargetTable::Titles,
            id,
        })
    }

    fn series(id: u64) -> Option<ProposalTarget> {
        Some(ProposalTarget {
            table: ProposalTargetTable::Series,
            id,
        })
    }

    fn item(target: Option<ProposalTarget>, values: Value) -> NewProposalItem {
        NewProposalItem {
            target,
            expected_version: None,
            proposed_values: values,
        }
    }

    fn key_of(kind: ProposalKind, items: Vec<NewProposalItem>) -> Option<String> {
        ProposalSubject::of_new(&NewProposal {
            kind,
            api_key_id: 1,
            sources: vec![],
            items,
        })
        .map(|s| s.equivalence_key())
    }

    #[test]
    fn newest_overlap_takes_the_newest_rejection_sharing_a_field() {
        let prefix = "metadata:titles/7:";
        let rows = vec![
            ("metadata:titles/7:dewey_code".to_string(), 3),
            ("metadata:titles/7:description,publisher".to_string(), 2),
            ("metadata:titles/7:publisher".to_string(), 1),
        ];
        let set = |f: &[&str]| f.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>();
        assert_eq!(
            newest_overlap(rows.clone(), prefix, &set(&["publisher", "subtitle"])),
            Some((2, set(&["publisher"])))
        );
        assert_eq!(
            newest_overlap(rows.clone(), prefix, &set(&["dewey_code", "description"])),
            Some((3, set(&["dewey_code"])))
        );
        assert_eq!(newest_overlap(rows, prefix, &set(&["subtitle"])), None);
        // A key of another title's prefix never matches.
        assert_eq!(
            newest_overlap(
                vec![("metadata:titles/70:publisher".to_string(), 9)],
                prefix,
                &set(&["publisher"])
            ),
            None
        );
    }

    #[test]
    fn metadata_keys_parse_back_to_title_and_fields() {
        let key = key_of(
            ProposalKind::Metadata,
            ["publisher", "description"]
                .iter()
                .map(|f| item(title(7), json!({"line": "field", "field": f, "proposed": "x"})))
                .collect(),
        )
        .unwrap();
        let (id, fields) = parse_metadata_key(&key).unwrap();
        assert_eq!(id, 7);
        assert_eq!(
            fields,
            ["description", "publisher"].iter().map(|s| s.to_string()).collect()
        );
        assert_eq!(parse_metadata_key("cover:titles/7:ab"), None);
        assert_eq!(parse_metadata_key("metadata:titles/x:a"), None);
        assert!(metadata_key_fields("metadata:titles/7:", "metadata:titles/7:").is_empty());
    }

    #[test]
    fn delay_parsing_falls_back_to_the_default() {
        assert_eq!(parse_resubmission_delay_days(None), 90);
        assert_eq!(parse_resubmission_delay_days(Some("30")), 30);
        assert_eq!(parse_resubmission_delay_days(Some(" 0 ")), 0);
        assert_eq!(parse_resubmission_delay_days(Some("3650")), 3650);
        for bad in ["3651", "-1", "", "ninety", "1.5"] {
            assert_eq!(parse_resubmission_delay_days(Some(bad)), 90, "{bad}");
        }
    }

    #[test]
    fn delay_with_a_simulated_clock() {
        let rejected = at(1, 10);
        let open = resubmission_open_from(rejected, 30);
        assert_eq!(open, at(31, 10));
        // Inside the delay: blocked until the reopening instant.
        assert_eq!(blocked_until(rejected, at(1, 11), 30), Some(open));
        assert_eq!(
            blocked_until(rejected, open - Duration::seconds(1), 30),
            Some(open)
        );
        // At and after the reopening instant: accepted.
        assert_eq!(blocked_until(rejected, open, 30), None);
        assert_eq!(blocked_until(rejected, at(31, 11), 30), None);
        // A delay of 0 never blocks.
        assert_eq!(blocked_until(rejected, rejected, 0), None);
        assert_eq!(rfc3339_utc(open), "2026-10-31T10:00:00Z");
    }

    #[test]
    fn series_create_key_folds_the_name() {
        let create = |name: &str| {
            key_of(
                ProposalKind::SeriesCreate,
                vec![
                    item(
                        None,
                        json!({"line": "series", "name": name, "series_type": "open"}),
                    ),
                    item(None, json!({"line": "expected_tome", "position": 1})),
                ],
            )
        };
        assert_eq!(
            create("Les Schtroumpfs").as_deref(),
            Some("series_create:les schtroumpfs")
        );
        assert_eq!(create("  LES   SCHTRÖUMPFS "), create("Les Schtroumpfs"));
        // Compatibility forms (fullwidth letters, ligatures) fold too.
        assert_eq!(create("Ｌes Schtroumpfs"), create("Les Schtroumpfs"));
        assert_eq!(create("Les Schtroumpﬁ"), create("les schtroumpfi"));
        assert_ne!(create("Gaston"), create("Les Schtroumpfs"));
        assert_eq!(create("   "), None);
        // A long name is cut to the column width.
        let long = create(&"a".repeat(2000)).unwrap();
        assert_eq!(long.chars().count(), MAX_EQUIVALENCE_KEY_CHARS);
    }

    #[test]
    fn series_attach_key_is_the_series() {
        let attach = |id, title_lines: u64| {
            let mut items = vec![item(series(id), json!({"line": "series"}))];
            for t in 0..title_lines {
                items.push(item(
                    title(t + 1),
                    json!({"line": "title", "position": t + 1}),
                ));
            }
            key_of(ProposalKind::SeriesAttach, items)
        };
        assert_eq!(attach(12, 0).as_deref(), Some("series_attach:series/12"));
        // Whatever the lines, the same series is the same subject.
        assert_eq!(attach(12, 3), attach(12, 0));
        assert_ne!(attach(13, 0), attach(12, 0));
        // No series line with a series target: no subject.
        assert_eq!(
            key_of(
                ProposalKind::SeriesAttach,
                vec![item(title(1), json!({"line": "series"}))]
            ),
            None
        );
    }

    #[test]
    fn metadata_key_is_the_title_and_the_field_set() {
        let meta = |id, fields: &[&str]| {
            key_of(
                ProposalKind::Metadata,
                fields
                    .iter()
                    .map(|f| {
                        item(
                            title(id),
                            json!({"line": "field", "field": f, "proposed": "x"}),
                        )
                    })
                    .collect(),
            )
        };
        assert_eq!(
            meta(4, &["subtitle", "description"]).as_deref(),
            Some("metadata:titles/4:description,subtitle")
        );
        // Order does not matter; the set does.
        assert_eq!(
            meta(4, &["description", "subtitle"]),
            meta(4, &["subtitle", "description"])
        );
        assert_ne!(
            meta(4, &["description"]),
            meta(4, &["description", "subtitle"])
        );
        assert_ne!(meta(5, &["description"]), meta(4, &["description"]));
        // Two titles in one proposal, or no field line: no subject.
        assert_eq!(
            key_of(
                ProposalKind::Metadata,
                vec![
                    item(title(1), json!({"line": "field", "field": "subtitle"})),
                    item(title(2), json!({"line": "field", "field": "publisher"})),
                ]
            ),
            None
        );
        assert_eq!(meta(4, &[]), None);
    }

    // Story 12-2 (#535): a fingerprinted cover is keyed by title AND image.
    #[test]
    fn cover_key_is_the_title_and_the_image() {
        let cover = |id, fp: &str| {
            key_of(
                ProposalKind::Cover,
                vec![item(
                    title(id),
                    json!({"line": "cover", "staged_file": "a.jpg", "fingerprint": fp}),
                )],
            )
        };
        assert_eq!(cover(7, "ab12").as_deref(), Some("cover:titles/7:ab12"));
        assert_ne!(cover(7, "ab12"), cover(7, "cd34"), "another image");
        assert_ne!(cover(8, "ab12"), cover(7, "ab12"), "another title");
    }

    // A cover filed before story 12-2 has no fingerprint: legacy key.
    #[test]
    fn cover_key_is_the_title() {
        let cover = |id| {
            key_of(
                ProposalKind::Cover,
                vec![item(
                    title(id),
                    json!({"line": "cover", "staged_file": "a.jpg"}),
                )],
            )
        };
        assert_eq!(cover(7).as_deref(), Some("cover:titles/7"));
        assert_ne!(cover(8), cover(7));
        // The kind is part of the key: a cover is never a metadata proposal.
        assert_ne!(
            cover(7),
            key_of(
                ProposalKind::Metadata,
                vec![item(title(7), json!({"line": "field", "field": "cover"}))]
            )
        );
    }

    #[test]
    fn subject_documents_and_descriptions() {
        let m = ProposalSubject::Metadata {
            title_id: 3,
            fields: ["subtitle".to_string(), "description".to_string()]
                .into_iter()
                .collect(),
        };
        assert_eq!(
            m.to_json(),
            json!({"title_id": 3, "fields": ["description", "subtitle"]})
        );
        assert_eq!(m.describe(), "`description`, `subtitle` of title 3");
        assert_eq!(
            ProposalSubject::SeriesCreate {
                name: "Yakari".into()
            }
            .to_json(),
            json!({"series_name": "Yakari"})
        );
        assert_eq!(
            ProposalSubject::SeriesAttach { series_id: 2 }.to_json(),
            json!({"series_id": 2})
        );
        assert_eq!(
            ProposalSubject::Cover { title_id: 9, fingerprint: None }.describe(),
            "this cover image for title 9"
        );
    }
}
