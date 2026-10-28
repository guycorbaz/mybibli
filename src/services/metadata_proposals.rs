//! Story 11-13 (FR132) — metadata proposals filed by an agent.
//!
//! The MCP tool `propose_title_metadata` is a thin adapter over this
//! module. It validates the fields the agent proposes for one title
//! (field by field, every problem reported at once), compares each one
//! with the title as it is now, drops what would not change, and files
//! **one `metadata` proposal per call** through [`ProposalService::file`]
//! (story 11-9). Nothing in the catalogue changes: acceptance is story
//! 11-18.
//!
//! ## Item shapes (contract for 11-15, 11-16, 11-18, 11-19, 11-20)
//!
//! One item per proposed field, in the fixed order of
//! [`PROPOSABLE_FIELDS`], every item targeting `titles/<id>` (so each
//! records the title's `version`), discriminated by `"line": "field"`:
//!
//! - **scalar field** — `{"line": "field", "field": "<name>", "current":
//!   <value|null>, "proposed": <value>}`. Strings for `subtitle`,
//!   `description`, `publisher`, `dewey_code` (trimmed); integers for
//!   `publication_year` (`current` is the year of the title's
//!   `publication_date`; acceptance writes `YYYY-01-01`).
//! - **contributors** — `{"line": "field", "field": "contributors",
//!   "current": [{"name", "role"}…], "proposed": [{"name", "role",
//!   "role_id"}…]}`. `proposed` lists the contributors **to add**;
//!   acceptance never removes one. `role_id` is authoritative, `role` is
//!   the role's name at filing (for display). Whether a contributor with
//!   that name already exists is settled at acceptance (11-18: reuse,
//!   else create).
//!
//! `current` is the value at filing, kept as filed (11-19 edits
//! `proposed` only). It is also the field-level staleness baseline that
//! #521 may use: contributor writes (`title_contributors`) do not bump
//! `titles.version`, so acceptance must skip a contributor already on
//! the title.
//!
//! Errors are a typed [`MetadataProposalError`] with agent-facing
//! English messages (D-11-9-2 precedent).

use std::collections::HashSet;
use std::fmt;

use chrono::Datelike;
use serde_json::{Map, Value, json};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::contributor::TitleContributorModel;
use crate::models::contributor_role::ContributorRoleModel;
use crate::models::proposal::{ProposalKind, ProposalTarget, ProposalTargetTable};
use crate::models::title::TitleModel;
use crate::services::contributor::MAX_CONTRIBUTOR_NAME_BYTES;
use crate::services::proposals::{
    FiledProposal, MAX_ITEM_VALUES_BYTES, NewProposal, NewProposalItem, ProposalError,
    ProposalService, Refusal, RefusalClass, validate_sources,
};
use crate::utils::dewey_code_chars_valid;

/// The fields an agent may propose, in item order.
pub const PROPOSABLE_FIELDS: [&str; 6] = [
    "subtitle",
    "description",
    "publisher",
    "publication_year",
    "dewey_code",
    "contributors",
];
/// `titles.subtitle` width.
pub const MAX_SUBTITLE_CHARS: usize = 500;
/// `titles.publisher` width.
pub const MAX_PUBLISHER_CHARS: usize = 255;
/// A proposed description (the column holds more; a proposal item must
/// fit 64 KiB with the current value next to it — D-11-13-3).
pub const MAX_DESCRIPTION_CHARS: usize = 10_000;
/// `titles.dewey_code` width.
pub const MAX_DEWEY_CHARS: usize = 32;
/// Earliest publication year accepted.
pub const MIN_PUBLICATION_YEAR: i32 = 1000;
/// At most this many contributors per proposal.
pub const MAX_CONTRIBUTORS: usize = 50;

/// `propose_title_metadata`.
#[derive(Debug, Clone, PartialEq)]
pub struct MetadataProposalRequest {
    pub api_key_id: u64,
    pub title_id: u64,
    /// The title's `version` as the agent read it (optional pin).
    pub version: Option<i32>,
    /// Field name → proposed value, as the agent sent it.
    pub fields: Map<String, Value>,
    pub sources: Vec<String>,
}

/// A filed metadata proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiledMetadataProposal {
    pub filed: FiledProposal,
    /// Proposed fields dropped because the title already has that value.
    pub unchanged_fields: Vec<&'static str>,
}

/// What is wrong with one field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldProblem {
    NotProposable,
    WrongType { expected: &'static str },
    Blank,
    TooLongChars { max: usize },
    TooLongBytes { max: usize },
    ControlCharacter,
    DeweyShape,
    YearOutOfRange { min: i32, max: i32 },
    EmptyList,
    TooManyContributors { count: usize },
    UnknownRole { role: String, known: Vec<String> },
    ContributorTwice,
}

impl fmt::Display for FieldProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use FieldProblem::*;
        match self {
            NotProposable => write!(
                f,
                "cannot be proposed; the fields an agent may propose are {}",
                PROPOSABLE_FIELDS.join(", ")
            ),
            WrongType { expected } => write!(f, "must be {expected}"),
            Blank => write!(
                f,
                "is empty; propose a value, or leave the field out (a proposal completes a \
                 title, it does not erase a value)"
            ),
            TooLongChars { max } => write!(f, "is longer than {max} characters"),
            TooLongBytes { max } => write!(f, "is longer than {max} bytes (UTF-8)"),
            ControlCharacter => write!(f, "contains a control character (line break, tab…)"),
            DeweyShape => write!(
                f,
                "must be a Dewey class made of digits, '.' and '/' (e.g. \"843.914\")"
            ),
            YearOutOfRange { min, max } => write!(f, "must be a year between {min} and {max}"),
            EmptyList => write!(
                f,
                "is empty; give at least one contributor, or leave it out"
            ),
            TooManyContributors { count } => write!(
                f,
                "holds {count} contributors; at most {MAX_CONTRIBUTORS} per proposal"
            ),
            UnknownRole { role, known } if known.is_empty() => write!(
                f,
                "\"{role}\" is not a contributor role, and the library has none defined"
            ),
            UnknownRole { role, known } => write!(
                f,
                "\"{role}\" is not a contributor role of this library; use one of: {}",
                known.join(", ")
            ),
            ContributorTwice => write!(f, "repeats a contributor with the same name and role"),
        }
    }
}

/// One refused field; `field` is a path such as `contributors[2].role`
/// (1-based index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: String,
    pub problem: FieldProblem,
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` {}", self.field, self.problem)
    }
}

/// A metadata proposal that could not be filed.
#[derive(Debug)]
pub enum MetadataProposalError {
    /// `fields` is empty.
    NoFields,
    /// One or more fields refused; nothing filed.
    Fields(Vec<FieldError>),
    TitleNotFound {
        title_id: u64,
    },
    TargetChanged {
        title_id: u64,
        expected: i32,
        current: i32,
    },
    /// Every proposed value is already the title's.
    NothingChanges {
        title_id: u64,
        fields: Vec<&'static str>,
    },
    /// One item, with the title's current value next to the proposed
    /// one, exceeds the item cap.
    ItemTooLarge {
        field: &'static str,
    },
    Proposal(ProposalError),
}

impl Refusal for MetadataProposalError {
    fn class(&self) -> RefusalClass {
        use MetadataProposalError::*;
        match self {
            NoFields | Fields(_) => RefusalClass::InvalidInput,
            TitleNotFound { .. } => RefusalClass::NotFound,
            TargetChanged { .. } | NothingChanges { .. } | ItemTooLarge { .. } => {
                RefusalClass::Conflict
            }
            Proposal(e) if e.is_invalid_input() => RefusalClass::InvalidInput,
            Proposal(ProposalError::KeyNotAllowed { .. }) => RefusalClass::Conflict,
            Proposal(_) => RefusalClass::Internal,
        }
    }
}

impl fmt::Display for MetadataProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use MetadataProposalError::*;
        match self {
            NoFields => write!(
                f,
                "`fields` is empty: give at least one of {}.",
                PROPOSABLE_FIELDS.join(", ")
            ),
            Fields(errors) => {
                write!(
                    f,
                    "{} field{} refused, nothing was filed:",
                    errors.len(),
                    if errors.len() == 1 { " was" } else { "s were" }
                )?;
                for e in errors {
                    write!(f, "\n- {e}.")?;
                }
                write!(f, "\nCorrect or remove these fields and call again.")
            }
            TitleNotFound { title_id } => write!(
                f,
                "No title with id {title_id} exists in the catalog (or it was deleted). Find \
                 title ids with search_titles."
            ),
            TargetChanged {
                title_id,
                expected,
                current,
            } => write!(
                f,
                "Title {title_id} changed since you read it (version {expected}, now {current}): \
                 read it again with get_title and file the proposal on its current state."
            ),
            NothingChanges { title_id, fields } => write!(
                f,
                "Nothing would change: title {title_id} already has the proposed value of {}. \
                 Nothing was filed.",
                fields.join(", ")
            ),
            ItemTooLarge { field } => write!(
                f,
                "The `{field}` entry, with the title's current value next to it, exceeds the \
                 {} KiB a proposal entry may hold: propose a shorter value, or leave `{field}` \
                 out.",
                MAX_ITEM_VALUES_BYTES / 1024
            ),
            Proposal(e) => write!(f, "{e}"),
        }
    }
}

impl From<AppError> for MetadataProposalError {
    fn from(e: AppError) -> Self {
        MetadataProposalError::Proposal(ProposalError::Database(e))
    }
}

impl From<ProposalError> for MetadataProposalError {
    fn from(e: ProposalError) -> Self {
        MetadataProposalError::Proposal(e)
    }
}

impl From<MetadataProposalError> for AppError {
    fn from(e: MetadataProposalError) -> Self {
        match e.class() {
            RefusalClass::InvalidInput => AppError::BadRequest(e.to_string()),
            RefusalClass::NotFound => AppError::NotFound(e.to_string()),
            RefusalClass::Conflict => AppError::Conflict(e.to_string()),
            RefusalClass::Internal => match e {
                MetadataProposalError::Proposal(p) => p.into(),
                other => AppError::Internal(other.to_string()),
            },
        }
    }
}

/// A contributor to add, once validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedContributor {
    pub name: String,
    pub role_id: u64,
    /// The library's spelling of the role.
    pub role: String,
}

/// The proposed fields, validated (`None` = not proposed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProposedFields {
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub publication_year: Option<i32>,
    pub dewey_code: Option<String>,
    pub contributors: Option<Vec<ProposedContributor>>,
}

/// The title's current values the proposal is compared with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CurrentValues {
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub publication_year: Option<i32>,
    pub dewey_code: Option<String>,
    /// `(name, role_id, role name)` of each live contributor.
    pub contributors: Vec<(String, u64, String)>,
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// A trimmed, non-blank string within `max` characters; single-line
/// unless `multiline` (then only line breaks and tabs are allowed).
fn text(value: &Value, max: usize, multiline: bool) -> Result<String, FieldProblem> {
    let Some(raw) = value.as_str() else {
        return Err(FieldProblem::WrongType {
            expected: "a string",
        });
    };
    let v = raw.trim();
    if v.is_empty() {
        return Err(FieldProblem::Blank);
    }
    if v.chars().count() > max {
        return Err(FieldProblem::TooLongChars { max });
    }
    let allowed = |c: char| multiline && matches!(c, '\n' | '\r' | '\t');
    if v.chars().any(|c| c.is_control() && !allowed(c)) {
        return Err(FieldProblem::ControlCharacter);
    }
    Ok(v.to_string())
}

fn dewey(value: &Value) -> Result<String, FieldProblem> {
    let v = text(value, MAX_DEWEY_CHARS, false)?;
    if !dewey_code_chars_valid(&v) || !v.chars().any(|c| c.is_ascii_digit()) {
        return Err(FieldProblem::DeweyShape);
    }
    Ok(v)
}

fn year(value: &Value, current_year: i32) -> Result<i32, FieldProblem> {
    let max = current_year + 1;
    let Some(y) = value.as_i64() else {
        return Err(FieldProblem::WrongType {
            expected: "an integer year (e.g. 1987)",
        });
    };
    i32::try_from(y)
        .ok()
        .filter(|y| (MIN_PUBLICATION_YEAR..=max).contains(y))
        .ok_or(FieldProblem::YearOutOfRange {
            min: MIN_PUBLICATION_YEAR,
            max,
        })
}

/// Validate `contributors`, pushing each problem with its path.
fn contributors(
    value: &Value,
    roles: &[(u64, String)],
    errors: &mut Vec<FieldError>,
) -> Option<Vec<ProposedContributor>> {
    let err = |field: String, problem| FieldError { field, problem };
    let Some(list) = value.as_array() else {
        errors.push(err(
            "contributors".into(),
            FieldProblem::WrongType {
                expected: "an array of {\"name\", \"role\"} objects",
            },
        ));
        return None;
    };
    if list.is_empty() {
        errors.push(err("contributors".into(), FieldProblem::EmptyList));
        return None;
    }
    if list.len() > MAX_CONTRIBUTORS {
        errors.push(err(
            "contributors".into(),
            FieldProblem::TooManyContributors { count: list.len() },
        ));
        return None;
    }
    let before = errors.len();
    let mut out = Vec::with_capacity(list.len());
    let mut seen: HashSet<(String, u64)> = HashSet::new();
    for (i, entry) in list.iter().enumerate() {
        let path = format!("contributors[{}]", i + 1);
        let Some(obj) = entry.as_object() else {
            errors.push(err(
                path,
                FieldProblem::WrongType {
                    expected: "an object {\"name\", \"role\"}",
                },
            ));
            continue;
        };
        for key in obj.keys().filter(|k| *k != "name" && *k != "role") {
            errors.push(err(
                format!("{path}.{key}"),
                FieldProblem::WrongType {
                    expected: "absent (a contributor has only `name` and `role`)",
                },
            ));
        }
        let name = match obj.get("name") {
            None => Err(FieldProblem::Blank),
            Some(v) => text(v, usize::MAX, false).and_then(|n| {
                if n.len() > MAX_CONTRIBUTOR_NAME_BYTES {
                    Err(FieldProblem::TooLongBytes {
                        max: MAX_CONTRIBUTOR_NAME_BYTES,
                    })
                } else {
                    Ok(n)
                }
            }),
        };
        let role = match obj.get("role") {
            None => Err(FieldProblem::Blank),
            Some(v) => text(v, usize::MAX, false).and_then(|r| {
                roles
                    .iter()
                    .find(|(_, known)| same_name(known, &r))
                    .cloned()
                    .ok_or_else(|| FieldProblem::UnknownRole {
                        role: r.chars().take(80).collect(),
                        known: roles.iter().map(|(_, n)| n.clone()).collect(),
                    })
            }),
        };
        match (name, role) {
            (Ok(name), Ok((role_id, role))) => {
                if !seen.insert((name.to_lowercase(), role_id)) {
                    errors.push(err(path, FieldProblem::ContributorTwice));
                    continue;
                }
                out.push(ProposedContributor {
                    name,
                    role_id,
                    role,
                });
            }
            (name, role) => {
                if let Err(p) = name {
                    errors.push(err(format!("{path}.name"), p));
                }
                if let Err(p) = role {
                    errors.push(err(format!("{path}.role"), p));
                }
            }
        }
    }
    (errors.len() == before).then_some(out)
}

/// Validate the proposed fields (pure — unit tested): every problem of
/// every field is collected, so the agent corrects them in one go.
/// `roles` are the library's live contributor roles `(id, name)`.
pub fn validate_fields(
    fields: &Map<String, Value>,
    roles: &[(u64, String)],
    current_year: i32,
) -> Result<ProposedFields, MetadataProposalError> {
    if fields.is_empty() {
        return Err(MetadataProposalError::NoFields);
    }
    let mut errors: Vec<FieldError> = Vec::new();
    let mut out = ProposedFields::default();
    for (key, value) in fields {
        let mut single = |r: Result<(), FieldProblem>| {
            if let Err(problem) = r {
                errors.push(FieldError {
                    field: key.clone(),
                    problem,
                });
            }
        };
        match key.as_str() {
            "subtitle" => {
                single(text(value, MAX_SUBTITLE_CHARS, false).map(|v| out.subtitle = Some(v)))
            }
            "description" => {
                single(text(value, MAX_DESCRIPTION_CHARS, true).map(|v| out.description = Some(v)))
            }
            "publisher" => {
                single(text(value, MAX_PUBLISHER_CHARS, false).map(|v| out.publisher = Some(v)))
            }
            "publication_year" => {
                single(year(value, current_year).map(|v| out.publication_year = Some(v)))
            }
            "dewey_code" => single(dewey(value).map(|v| out.dewey_code = Some(v))),
            "contributors" => out.contributors = contributors(value, roles, &mut errors),
            _ => single(Err(FieldProblem::NotProposable)),
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(MetadataProposalError::Fields(errors))
    }
}

/// The proposal items and the fields dropped as unchanged (pure — unit
/// tested), in [`PROPOSABLE_FIELDS`] order.
pub fn diff_against(
    proposed: &ProposedFields,
    current: &CurrentValues,
) -> (Vec<(&'static str, Value)>, Vec<&'static str>) {
    let mut items = Vec::new();
    let mut unchanged = Vec::new();
    let mut scalar = |field: &'static str, cur: Value, new: Option<Value>, same: bool| {
        let Some(new) = new else { return };
        if same {
            unchanged.push(field);
        } else {
            items.push((
                field,
                json!({"line": "field", "field": field, "current": cur, "proposed": new}),
            ));
        }
    };
    let text_same = |new: &Option<String>, cur: &Option<String>| matches!((new, cur), (Some(n), Some(c)) if n == c.trim());
    scalar(
        "subtitle",
        json!(current.subtitle),
        proposed.subtitle.as_ref().map(|v| json!(v)),
        text_same(&proposed.subtitle, &current.subtitle),
    );
    scalar(
        "description",
        json!(current.description),
        proposed.description.as_ref().map(|v| json!(v)),
        text_same(&proposed.description, &current.description),
    );
    scalar(
        "publisher",
        json!(current.publisher),
        proposed.publisher.as_ref().map(|v| json!(v)),
        text_same(&proposed.publisher, &current.publisher),
    );
    scalar(
        "publication_year",
        json!(current.publication_year),
        proposed.publication_year.map(|v| json!(v)),
        proposed.publication_year.is_some()
            && proposed.publication_year == current.publication_year,
    );
    scalar(
        "dewey_code",
        json!(current.dewey_code),
        proposed.dewey_code.as_ref().map(|v| json!(v)),
        text_same(&proposed.dewey_code, &current.dewey_code),
    );
    if let Some(list) = &proposed.contributors {
        let added: Vec<Value> = list
            .iter()
            .filter(|p| {
                !current
                    .contributors
                    .iter()
                    .any(|(name, role_id, _)| *role_id == p.role_id && same_name(name, &p.name))
            })
            .map(|p| json!({"name": p.name, "role": p.role, "role_id": p.role_id}))
            .collect();
        if added.is_empty() {
            unchanged.push("contributors");
        } else {
            let cur: Vec<Value> = current
                .contributors
                .iter()
                .map(|(name, _, role)| json!({"name": name, "role": role}))
                .collect();
            items.push((
                "contributors",
                json!({"line": "field", "field": "contributors", "current": cur, "proposed": added}),
            ));
        }
    }
    (items, unchanged)
}

pub struct MetadataProposalService;

impl MetadataProposalService {
    /// File a `metadata` proposal for one title (FR132).
    pub async fn propose(
        pool: &DbPool,
        req: &MetadataProposalRequest,
    ) -> Result<FiledMetadataProposal, MetadataProposalError> {
        let wants_contributors = req.fields.contains_key("contributors");
        let roles = if wants_contributors {
            ContributorRoleModel::find_all(pool).await?
        } else {
            Vec::new()
        };
        let current_year = chrono::Local::now().year();
        let proposed = validate_fields(&req.fields, &roles, current_year)?;
        let sources = validate_sources(&req.sources)?;

        let Some(title) = TitleModel::find_by_id(pool, req.title_id).await? else {
            return Err(MetadataProposalError::TitleNotFound {
                title_id: req.title_id,
            });
        };
        if let Some(expected) = req.version
            && expected != title.version
        {
            return Err(MetadataProposalError::TargetChanged {
                title_id: title.id,
                expected,
                current: title.version,
            });
        }
        let current_contributors = if proposed.contributors.is_some() {
            TitleContributorModel::find_by_title(pool, title.id)
                .await?
                .into_iter()
                .map(|c| (c.contributor_name, c.role_id, c.role_name))
                .collect()
        } else {
            Vec::new()
        };
        let current = CurrentValues {
            subtitle: title.subtitle,
            description: title.description,
            publisher: title.publisher,
            publication_year: title.publication_date.map(|d| d.year()),
            dewey_code: title.dewey_code,
            contributors: current_contributors,
        };

        let (items, unchanged_fields) = diff_against(&proposed, &current);
        if items.is_empty() {
            return Err(MetadataProposalError::NothingChanges {
                title_id: title.id,
                fields: unchanged_fields,
            });
        }
        let target = ProposalTarget {
            table: ProposalTargetTable::Titles,
            id: title.id,
        };
        // The version the comparison ran against: a title edited between
        // that read and the filing is refused rather than recorded as the
        // baseline (D-11-12-13 precedent), so `current` matches it.
        let version = req.version.unwrap_or(title.version);
        let field_names: Vec<&'static str> = items.iter().map(|(f, _)| *f).collect();
        let proposal = NewProposal {
            kind: ProposalKind::Metadata,
            api_key_id: req.api_key_id,
            sources,
            items: items
                .into_iter()
                .map(|(_, values)| NewProposalItem {
                    target: Some(target),
                    expected_version: Some(version),
                    proposed_values: values,
                })
                .collect(),
        };
        let proposal_id = ProposalService::file(pool, &proposal)
            .await
            .map_err(|e| match e {
                ProposalError::TargetNotFound { .. } => MetadataProposalError::TitleNotFound {
                    title_id: target.id,
                },
                ProposalError::TargetChanged {
                    expected, current, ..
                } => MetadataProposalError::TargetChanged {
                    title_id: target.id,
                    expected,
                    current,
                },
                ProposalError::ValuesTooLarge { index, .. } => {
                    MetadataProposalError::ItemTooLarge {
                        field: field_names.get(index - 1).copied().unwrap_or("description"),
                    }
                }
                other => MetadataProposalError::Proposal(other),
            })?;
        Ok(FiledMetadataProposal {
            filed: FiledProposal {
                proposal_id,
                kind: ProposalKind::Metadata,
                item_count: proposal.items.len(),
            },
            unchanged_fields,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YEAR: i32 = 2026;

    fn roles() -> Vec<(u64, String)> {
        vec![(1, "Auteur".into()), (2, "Illustrateur".into())]
    }

    fn fields(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn field_errors(v: Value) -> Vec<FieldError> {
        match validate_fields(&fields(v), &roles(), YEAR) {
            Err(MetadataProposalError::Fields(e)) => e,
            other => panic!("expected field errors, got {other:?}"),
        }
    }

    fn problem_of(errors: &[FieldError], field: &str) -> FieldProblem {
        errors
            .iter()
            .find(|e| e.field == field)
            .unwrap_or_else(|| panic!("no error for {field}: {errors:?}"))
            .problem
            .clone()
    }

    #[test]
    fn valid_fields_are_trimmed_and_resolved() {
        let p = validate_fields(
            &fields(json!({
                "subtitle": "  Roman ",
                "description": "Line one\nLine\ttwo",
                "publisher": "Gallimard",
                "publication_year": 1987,
                "dewey_code": "843.914",
                "contributors": [{"name": " Jean Dupont ", "role": "illustrateur"}]
            })),
            &roles(),
            YEAR,
        )
        .unwrap();
        assert_eq!(p.subtitle.as_deref(), Some("Roman"));
        assert_eq!(p.description.as_deref(), Some("Line one\nLine\ttwo"));
        assert_eq!(p.publication_year, Some(1987));
        assert_eq!(
            p.contributors.unwrap(),
            vec![ProposedContributor {
                name: "Jean Dupont".into(),
                role_id: 2,
                role: "Illustrateur".into()
            }]
        );
    }

    #[test]
    fn empty_fields_are_refused() {
        assert!(matches!(
            validate_fields(&Map::new(), &roles(), YEAR),
            Err(MetadataProposalError::NoFields)
        ));
    }

    #[test]
    fn every_bad_field_is_reported_at_once() {
        let errors = field_errors(json!({
            "isbn": "9782070360246",
            "title": "Other",
            "subtitle": "   ",
            "publisher": 12,
            "publication_year": 999,
            "dewey_code": "abc",
            "description": null
        }));
        assert_eq!(errors.len(), 7, "{errors:?}");
        assert_eq!(problem_of(&errors, "isbn"), FieldProblem::NotProposable);
        assert_eq!(problem_of(&errors, "title"), FieldProblem::NotProposable);
        assert_eq!(problem_of(&errors, "subtitle"), FieldProblem::Blank);
        assert!(matches!(
            problem_of(&errors, "publisher"),
            FieldProblem::WrongType { .. }
        ));
        assert_eq!(
            problem_of(&errors, "publication_year"),
            FieldProblem::YearOutOfRange {
                min: 1000,
                max: YEAR + 1
            }
        );
        assert_eq!(problem_of(&errors, "dewey_code"), FieldProblem::DeweyShape);
        assert!(matches!(
            problem_of(&errors, "description"),
            FieldProblem::WrongType { .. }
        ));
        let msg = MetadataProposalError::Fields(errors).to_string();
        assert!(msg.starts_with("7 fields were refused, nothing was filed:"));
        assert!(msg.contains("`isbn` cannot be proposed; the fields an agent may propose are subtitle, description, publisher, publication_year, dewey_code, contributors"));
    }

    #[test]
    fn lengths_follow_the_columns() {
        let ok = validate_fields(
            &fields(json!({
                "subtitle": "é".repeat(MAX_SUBTITLE_CHARS),
                "publisher": "é".repeat(MAX_PUBLISHER_CHARS),
                "description": "é".repeat(MAX_DESCRIPTION_CHARS),
                "dewey_code": "1".repeat(MAX_DEWEY_CHARS),
            })),
            &roles(),
            YEAR,
        );
        assert!(ok.is_ok(), "{ok:?}");
        let errors = field_errors(json!({
            "subtitle": "é".repeat(MAX_SUBTITLE_CHARS + 1),
            "publisher": "é".repeat(MAX_PUBLISHER_CHARS + 1),
            "description": "é".repeat(MAX_DESCRIPTION_CHARS + 1),
            "dewey_code": "1".repeat(MAX_DEWEY_CHARS + 1),
        }));
        assert_eq!(errors.len(), 4);
        assert_eq!(
            problem_of(&errors, "subtitle"),
            FieldProblem::TooLongChars { max: 500 }
        );
    }

    #[test]
    fn control_characters_only_in_the_description() {
        let errors = field_errors(json!({"subtitle": "a\nb", "publisher": "a\u{0}b"}));
        assert_eq!(
            problem_of(&errors, "subtitle"),
            FieldProblem::ControlCharacter
        );
        assert_eq!(
            problem_of(&errors, "publisher"),
            FieldProblem::ControlCharacter
        );
        let errors = field_errors(json!({"description": "a\u{7}b"}));
        assert_eq!(
            problem_of(&errors, "description"),
            FieldProblem::ControlCharacter
        );
    }

    #[test]
    fn dewey_and_year_rules() {
        for good in ["843", "843.914", "973/.926"] {
            assert!(dewey(&json!(good)).is_ok(), "{good}");
        }
        for bad in ["...", "/", "84a", "843 914"] {
            assert_eq!(dewey(&json!(bad)), Err(FieldProblem::DeweyShape), "{bad}");
        }
        assert_eq!(year(&json!(1000), YEAR), Ok(1000));
        assert_eq!(year(&json!(YEAR + 1), YEAR), Ok(YEAR + 1));
        assert!(year(&json!(YEAR + 2), YEAR).is_err());
        assert!(matches!(
            year(&json!("1987"), YEAR),
            Err(FieldProblem::WrongType { .. })
        ));
        assert!(matches!(
            year(&json!(1987.5), YEAR),
            Err(FieldProblem::WrongType { .. })
        ));
        assert!(year(&json!(i64::MAX), YEAR).is_err());
    }

    #[test]
    fn contributor_rules() {
        let errors = field_errors(json!({"contributors": []}));
        assert_eq!(problem_of(&errors, "contributors"), FieldProblem::EmptyList);
        let many: Vec<Value> = (0..=MAX_CONTRIBUTORS)
            .map(|i| json!({"name": format!("N{i}"), "role": "Auteur"}))
            .collect();
        let errors = field_errors(json!({ "contributors": many }));
        assert!(matches!(
            problem_of(&errors, "contributors"),
            FieldProblem::TooManyContributors { .. }
        ));
        let errors = field_errors(json!({"contributors": [
            {"name": "A", "role": "Autor"},
            {"name": " ", "role": "Auteur"},
            {"name": "x".repeat(256), "role": "Auteur"},
            {"name": "B", "role": "Auteur", "id": 3},
            "C",
            {"name": "D", "role": "auteur"},
            {"name": "d", "role": "Auteur"},
            {"name": "E\tF", "role": "Auteur"},
            {"role": "Auteur"}
        ]}));
        assert!(matches!(
            problem_of(&errors, "contributors[1].role"),
            FieldProblem::UnknownRole { ref known, .. } if known == &["Auteur", "Illustrateur"]
        ));
        assert_eq!(
            problem_of(&errors, "contributors[2].name"),
            FieldProblem::Blank
        );
        assert_eq!(
            problem_of(&errors, "contributors[3].name"),
            FieldProblem::TooLongBytes { max: 255 }
        );
        assert!(matches!(
            problem_of(&errors, "contributors[4].id"),
            FieldProblem::WrongType { .. }
        ));
        assert!(matches!(
            problem_of(&errors, "contributors[5]"),
            FieldProblem::WrongType { .. }
        ));
        assert_eq!(
            problem_of(&errors, "contributors[7]"),
            FieldProblem::ContributorTwice
        );
        assert_eq!(
            problem_of(&errors, "contributors[8].name"),
            FieldProblem::ControlCharacter
        );
        assert_eq!(
            problem_of(&errors, "contributors[9].name"),
            FieldProblem::Blank
        );
        // 255 bytes pass even when that is fewer characters.
        let ok = validate_fields(
            &fields(json!({"contributors": [{"name": "é".repeat(127), "role": "Auteur"}]})),
            &roles(),
            YEAR,
        );
        assert!(ok.is_ok());
        let no_roles = validate_fields(
            &fields(json!({"contributors": [{"name": "A", "role": "Auteur"}]})),
            &[],
            YEAR,
        );
        assert!(
            no_roles
                .unwrap_err()
                .to_string()
                .contains("has none defined")
        );
    }

    fn current() -> CurrentValues {
        CurrentValues {
            subtitle: Some("Roman".into()),
            description: None,
            publisher: Some("Gallimard ".into()),
            publication_year: Some(1987),
            dewey_code: None,
            contributors: vec![("Jean Dupont".into(), 1, "Auteur".into())],
        }
    }

    #[test]
    fn unchanged_fields_are_dropped_and_items_follow_the_documented_shapes() {
        let proposed = ProposedFields {
            subtitle: Some("Roman".into()),
            description: Some("Desc".into()),
            publisher: Some("Gallimard".into()),
            publication_year: Some(1988),
            dewey_code: Some("843".into()),
            contributors: Some(vec![
                ProposedContributor {
                    name: "jean dupont".into(),
                    role_id: 1,
                    role: "Auteur".into(),
                },
                ProposedContributor {
                    name: "Jean Dupont".into(),
                    role_id: 2,
                    role: "Illustrateur".into(),
                },
            ]),
        };
        let (items, unchanged) = diff_against(&proposed, &current());
        assert_eq!(unchanged, ["subtitle", "publisher"]);
        let names: Vec<_> = items.iter().map(|(f, _)| *f).collect();
        assert_eq!(
            names,
            [
                "description",
                "publication_year",
                "dewey_code",
                "contributors"
            ]
        );
        assert_eq!(
            items[0].1,
            json!({"line": "field", "field": "description", "current": null, "proposed": "Desc"})
        );
        assert_eq!(
            items[1].1,
            json!({"line": "field", "field": "publication_year", "current": 1987, "proposed": 1988})
        );
        assert_eq!(
            items[3].1,
            json!({
                "line": "field", "field": "contributors",
                "current": [{"name": "Jean Dupont", "role": "Auteur"}],
                "proposed": [{"name": "Jean Dupont", "role": "Illustrateur", "role_id": 2}]
            })
        );
    }

    #[test]
    fn contributors_already_on_the_title_drop_the_field() {
        let proposed = ProposedFields {
            contributors: Some(vec![ProposedContributor {
                name: "JEAN DUPONT".into(),
                role_id: 1,
                role: "Auteur".into(),
            }]),
            ..Default::default()
        };
        let (items, unchanged) = diff_against(&proposed, &current());
        assert!(items.is_empty());
        assert_eq!(unchanged, ["contributors"]);
    }

    #[test]
    fn refusals_are_classified() {
        use MetadataProposalError as E;
        assert_eq!(E::NoFields.class(), RefusalClass::InvalidInput);
        assert_eq!(E::Fields(vec![]).class(), RefusalClass::InvalidInput);
        assert_eq!(
            E::TitleNotFound { title_id: 1 }.class(),
            RefusalClass::NotFound
        );
        let nothing = E::NothingChanges {
            title_id: 4,
            fields: vec!["subtitle"],
        };
        assert_eq!(nothing.class(), RefusalClass::Conflict);
        assert!(
            nothing
                .to_string()
                .contains("title 4 already has the proposed value of subtitle")
        );
        assert_eq!(
            E::TargetChanged {
                title_id: 1,
                expected: 1,
                current: 2
            }
            .class(),
            RefusalClass::Conflict
        );
        assert_eq!(
            E::Proposal(ProposalError::NoSources).class(),
            RefusalClass::InvalidInput
        );
        assert_eq!(
            E::Proposal(ProposalError::KeyNotAllowed { api_key_id: 1 }).class(),
            RefusalClass::Conflict
        );
        assert_eq!(
            E::Proposal(ProposalError::Database(AppError::Internal("x".into()))).class(),
            RefusalClass::Internal
        );
        assert!(
            E::ItemTooLarge {
                field: "description"
            }
            .to_string()
            .contains("64 KiB")
        );
    }
}
