//! Story 13-9 (#581, FR156) — label proposals filed by an agent.
//!
//! The library's labels (CR #443, `models::label`) are librarian-defined
//! marks attached to titles and volumes ("Code douteux", "À vérifier"…).
//! An agent that finds a doubtful record proposes attaching one of them;
//! the MCP tool `propose_label` is a thin adapter over this module. The
//! catalog does not change until a librarian accepts the proposal
//! (`services::label_acceptance`).
//!
//! ## Item shape (contract for history, review and acceptance)
//!
//! Exactly one item, targeting `titles/<id>` or `volumes/<id>` (so it
//! records the target's `version` — not used for staleness, see
//! `proposals::version_counts`):
//!
//! ```json
//! {"line": "label", "label_id": 3, "label_name": "Code douteux",
//!  "note": "The ISBN printed on the copy belongs to another book." | null}
//! ```
//!
//! - `label_name` — the label's name at filing (shown if the label is
//!   deleted meanwhile; the live name wins while it exists).
//! - `note` — the agent's reason, at most [`MAX_NOTE_CHARS`] characters,
//!   shown to the librarian as plain text (NFR47).
//!
//! Equivalence (`proposal_history`): `label:<table>/<id>:<label_id>` — a
//! rejection blocks the same label on the same record for the
//! resubmission delay. Filing also refuses a label already on the record
//! and a pending proposal of the same label on the same record.
//!
//! Lock order: the filing key row (exclusive, like series proposals),
//! then plain reads; the duplicate read starts from the target index.

use std::fmt;

use serde_json::{Value, json};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::label::{LabelModel, LabelTarget};
use crate::models::proposal::{ProposalKind, ProposalModel, ProposalTarget, ProposalTargetTable};
use crate::services::proposals::{
    FiledProposal, NewProposal, NewProposalItem, ProposalError, ProposalService, Refusal,
    RefusalClass, log_filed, validate_sources,
};

/// Longest note the agent may attach, in characters.
pub const MAX_NOTE_CHARS: usize = 500;
/// At most this many label names are quoted when a label is unknown.
const MAX_LISTED_LABELS: usize = 50;

/// How the agent names the label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelChoice {
    Id(u64),
    /// Exact name (case and accents count).
    Name(String),
}

impl fmt::Display for LabelChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LabelChoice::Id(id) => write!(f, "with id {id}"),
            LabelChoice::Name(name) => write!(f, "named {}", Value::String(name.clone())),
        }
    }
}

/// A label proposal as the agent sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelProposalRequest {
    pub api_key_id: u64,
    pub target: LabelTarget,
    pub label: LabelChoice,
    pub note: Option<String>,
    pub sources: Vec<String>,
}

/// The proposal target of a label target.
pub fn proposal_target(target: LabelTarget) -> ProposalTarget {
    match target {
        LabelTarget::Title(id) => ProposalTarget {
            table: ProposalTargetTable::Titles,
            id,
        },
        LabelTarget::Volume(id) => ProposalTarget {
            table: ProposalTargetTable::Volumes,
            id,
        },
    }
}

/// The label target of a proposal target; `None` for a series.
pub fn label_target(target: ProposalTarget) -> Option<LabelTarget> {
    match target.table {
        ProposalTargetTable::Titles => Some(LabelTarget::Title(target.id)),
        ProposalTargetTable::Volumes => Some(LabelTarget::Volume(target.id)),
        ProposalTargetTable::Series => None,
    }
}

fn noun(target: LabelTarget) -> &'static str {
    proposal_target(target).table.noun()
}

/// A label proposal that could not be filed.
#[derive(Debug)]
pub enum LabelProposalError {
    NoteTooLong {
        chars: usize,
    },
    TargetNotFound {
        target: LabelTarget,
    },
    LabelNotFound {
        label: LabelChoice,
        /// The library's label names (at most [`MAX_LISTED_LABELS`]).
        known: Vec<String>,
    },
    /// The label is already on the record: nothing to propose.
    AlreadyAttached {
        target: LabelTarget,
        label_name: String,
    },
    /// A pending proposal already puts this label on this record.
    PendingDuplicate {
        proposal_id: u64,
    },
    Proposal(ProposalError),
}

impl Refusal for LabelProposalError {
    fn class(&self) -> RefusalClass {
        use LabelProposalError::*;
        match self {
            NoteTooLong { .. } => RefusalClass::InvalidInput,
            TargetNotFound { .. } | LabelNotFound { .. } => RefusalClass::NotFound,
            AlreadyAttached { .. } | PendingDuplicate { .. } => RefusalClass::Conflict,
            Proposal(e) if e.is_invalid_input() => RefusalClass::InvalidInput,
            Proposal(e) if e.is_conflict() => RefusalClass::Conflict,
            Proposal(_) => RefusalClass::Internal,
        }
    }
}

impl fmt::Display for LabelProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use LabelProposalError::*;
        match self {
            NoteTooLong { chars } => write!(
                f,
                "`note` is {chars} characters long; at most {MAX_NOTE_CHARS} are allowed. \
                 Nothing was filed."
            ),
            TargetNotFound { target } => write!(
                f,
                "No {} with id {} exists in the catalog (or it was deleted). Find title ids \
                 with search_titles and volume ids in get_title.",
                noun(*target),
                target.id()
            ),
            LabelNotFound { label, known } if known.is_empty() => write!(
                f,
                "No label {label} exists, and the library has no labels yet: a librarian \
                 creates them. Nothing was filed."
            ),
            LabelNotFound { label, known } => write!(
                f,
                "No label {label} exists (names must match exactly). The library's labels are: \
                 {}. Read their ids with list_labels.",
                known
                    .iter()
                    .map(|n| Value::String(n.clone()).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AlreadyAttached { target, label_name } => write!(
                f,
                "{} {} already carries the label {}: there is nothing to propose.",
                capitalized(noun(*target)),
                target.id(),
                Value::String(label_name.clone())
            ),
            PendingDuplicate { proposal_id } => write!(
                f,
                "Proposal {proposal_id} already proposes this label on this record and is \
                 waiting for a librarian. Nothing was filed; follow it with list_my_proposals."
            ),
            Proposal(e) => write!(f, "{e}"),
        }
    }
}

fn capitalized(word: &str) -> String {
    let mut c = word.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

impl From<AppError> for LabelProposalError {
    fn from(e: AppError) -> Self {
        LabelProposalError::Proposal(ProposalError::Database(e))
    }
}

impl From<sqlx::Error> for LabelProposalError {
    fn from(e: sqlx::Error) -> Self {
        LabelProposalError::Proposal(ProposalError::from(e))
    }
}

impl From<ProposalError> for LabelProposalError {
    fn from(e: ProposalError) -> Self {
        LabelProposalError::Proposal(e)
    }
}

/// The NFC form of a label name (pure): an agent may send decomposed
/// accents (NFD) for a name typed precomposed in the web form.
pub fn nfc(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.nfc().collect()
}

/// Trim the note; empty means none (pure).
pub fn normalize_note(note: Option<&str>) -> Result<Option<String>, LabelProposalError> {
    let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) else {
        return Ok(None);
    };
    let chars = n.chars().count();
    if chars > MAX_NOTE_CHARS {
        return Err(LabelProposalError::NoteTooLong { chars });
    }
    Ok(Some(n.to_string()))
}

/// The proposal item (see the module doc for the contract).
pub fn label_item(label: &LabelModel, note: Option<&str>) -> Value {
    json!({
        "line": "label",
        "label_id": label.id,
        "label_name": label.name,
        "note": note,
    })
}

/// Story 13-9 — the library's labels with their usage, by name (the MCP
/// `list_labels` tool).
pub async fn library_labels(
    pool: &DbPool,
) -> Result<Vec<(LabelModel, crate::models::label::LabelUsage)>, AppError> {
    LabelModel::list_all_with_usage(pool).await
}

pub struct LabelProposalService;

impl LabelProposalService {
    /// Check the request and file one `label_attach` proposal.
    pub async fn propose(
        pool: &DbPool,
        req: &LabelProposalRequest,
    ) -> Result<FiledProposal, LabelProposalError> {
        let note = normalize_note(req.note.as_deref())?;
        // Fast refusal before any lock; `file_in` checks them again.
        validate_sources(&req.sources)?;
        let target = proposal_target(req.target);

        let mut tx = pool.begin().await?;
        // Key row first (series order): filings of one key are serialised,
        // so the duplicate check below and the insert cannot interleave.
        if !ProposalModel::lock_filing_key_exclusive(&mut tx, req.api_key_id).await? {
            return Err(ProposalError::KeyNotAllowed {
                api_key_id: req.api_key_id,
            }
            .into());
        }
        if !target_is_live(&mut tx, req.target).await? {
            return Err(LabelProposalError::TargetNotFound { target: req.target });
        }
        let label = match &req.label {
            LabelChoice::Id(id) => LabelModel::find_by_id_in(&mut tx, *id, false).await?,
            LabelChoice::Name(name) => {
                LabelModel::find_by_exact_name_in(&mut tx, &nfc(name)).await?
            }
        };
        let Some(label) = label else {
            let known = LabelModel::list_all_in(&mut tx)
                .await?
                .into_iter()
                .take(MAX_LISTED_LABELS)
                .map(|l| l.name)
                .collect();
            return Err(LabelProposalError::LabelNotFound {
                label: req.label.clone(),
                known,
            });
        };
        if LabelModel::is_attached_in(&mut tx, req.target, label.id).await? {
            return Err(LabelProposalError::AlreadyAttached {
                target: req.target,
                label_name: label.name,
            });
        }
        if let Some(proposal_id) =
            ProposalModel::pending_label_attach(&mut tx, target.table, target.id, label.id).await?
        {
            return Err(LabelProposalError::PendingDuplicate { proposal_id });
        }
        let proposal = NewProposal {
            kind: ProposalKind::LabelAttach,
            api_key_id: req.api_key_id,
            sources: req.sources.clone(),
            items: vec![NewProposalItem {
                target: Some(target),
                expected_version: None,
                proposed_values: label_item(&label, note.as_deref()),
            }],
        };
        let proposal_id =
            ProposalService::file_in(&mut tx, &proposal)
                .await
                .map_err(|e| match e {
                    ProposalError::TargetNotFound { .. } => {
                        LabelProposalError::TargetNotFound { target: req.target }
                    }
                    other => other.into(),
                })?;
        tx.commit().await?;
        log_filed(proposal_id, &proposal);
        Ok(FiledProposal {
            proposal_id,
            kind: ProposalKind::LabelAttach,
            item_count: 1,
        })
    }
}

/// Whether the target is live: a live title, or a live volume of a live
/// title (plain read).
async fn target_is_live(
    conn: &mut sqlx::MySqlConnection,
    target: LabelTarget,
) -> Result<bool, AppError> {
    Ok(match target {
        LabelTarget::Title(id) => {
            ProposalModel::target_states(conn, ProposalTargetTable::Titles, &[id], true, false)
                .await?
                .contains_key(&id)
        }
        LabelTarget::Volume(id) => ProposalModel::live_volume_briefs_in(conn, &[id])
            .await?
            .contains_key(&id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_names_are_compared_in_nfc() {
        assert_eq!(nfc("A\u{300} ve\u{301}rifier"), "À vérifier");
        assert_eq!(nfc("Code douteux"), "Code douteux");
    }

    #[test]
    fn notes_are_trimmed_and_bounded() {
        assert_eq!(normalize_note(None).unwrap(), None);
        assert_eq!(normalize_note(Some("   ")).unwrap(), None);
        assert_eq!(
            normalize_note(Some("  ISBN of another book \n")).unwrap(),
            Some("ISBN of another book".to_string())
        );
        let max = "é".repeat(MAX_NOTE_CHARS);
        assert_eq!(normalize_note(Some(&max)).unwrap(), Some(max.clone()));
        assert!(matches!(
            normalize_note(Some(&format!("{max}x"))),
            Err(LabelProposalError::NoteTooLong { chars: 501 })
        ));
    }

    #[test]
    fn the_item_follows_the_contract() {
        let label = LabelModel {
            id: 3,
            name: "Code douteux".to_string(),
            color: None,
            version: 1,
        };
        assert_eq!(
            label_item(&label, Some("why")),
            json!({"line": "label", "label_id": 3, "label_name": "Code douteux", "note": "why"})
        );
        assert_eq!(label_item(&label, None)["note"], Value::Null);
    }

    #[test]
    fn targets_round_trip() {
        for t in [LabelTarget::Title(4), LabelTarget::Volume(9)] {
            assert_eq!(label_target(proposal_target(t)), Some(t));
        }
        assert_eq!(
            label_target(ProposalTarget {
                table: ProposalTargetTable::Series,
                id: 1
            }),
            None
        );
    }

    #[test]
    fn refusals_are_classed_and_actionable() {
        use LabelProposalError::*;
        let not_found = TargetNotFound {
            target: LabelTarget::Volume(12),
        };
        assert_eq!(not_found.class(), RefusalClass::NotFound);
        assert!(not_found.to_string().starts_with("No volume with id 12"));
        let unknown = LabelNotFound {
            label: LabelChoice::Name("code douteux".into()),
            known: vec!["Code douteux".into()],
        };
        assert_eq!(unknown.class(), RefusalClass::NotFound);
        let m = unknown.to_string();
        assert!(
            m.contains("named \"code douteux\"") && m.contains("\"Code douteux\""),
            "{m}"
        );
        assert!(m.contains("list_labels"));
        let none = LabelNotFound {
            label: LabelChoice::Id(5),
            known: vec![],
        };
        assert!(none.to_string().contains("no labels yet"));
        let attached = AlreadyAttached {
            target: LabelTarget::Title(4),
            label_name: "Code douteux".into(),
        };
        assert_eq!(attached.class(), RefusalClass::Conflict);
        assert!(attached.to_string().starts_with("Title 4 already carries"));
        assert_eq!(
            PendingDuplicate { proposal_id: 8 }.class(),
            RefusalClass::Conflict
        );
        assert_eq!(
            NoteTooLong { chars: 600 }.class(),
            RefusalClass::InvalidInput
        );
        assert_eq!(
            Proposal(ProposalError::NoSources).class(),
            RefusalClass::InvalidInput
        );
        assert_eq!(
            Proposal(ProposalError::KeyNotAllowed { api_key_id: 1 }).class(),
            RefusalClass::Conflict
        );
        assert_eq!(
            Proposal(ProposalError::Database(AppError::Internal("pw".into()))).class(),
            RefusalClass::Internal
        );
    }
}
