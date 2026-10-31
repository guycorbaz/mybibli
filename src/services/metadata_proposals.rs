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
//!   `description`, `publisher`, `dewey_code` (trimmed, NFC; one-line
//!   values with inner whitespace collapsed); integers for
//!   `publication_year`, whose item also carries `"current_date"`
//!   (`YYYY-MM-DD` or null) — `current` is its year, and acceptance
//!   writes `YYYY-01-01`, so the reviewer sees the month and day it
//!   replaces.
//! - **contributors** — `{"line": "field", "field": "contributors",
//!   "current": [{"name", "role"}…], "proposed": [{"name", "role",
//!   "role_id"}…]}`. `proposed` lists the contributors **to add**;
//!   acceptance never removes one. `role_id` is authoritative, `role` is
//!   the role's name at filing (for display). Names are compared folded
//!   ([`fold_name`]: case, accents, inner whitespace — close to the
//!   `utf8mb4_unicode_ci` collation; it also drops vowel signs of
//!   scripts such as Devanagari, which the collation weighs — accepted,
//!   11-18 re-checks by contributor id): duplicates within the list are
//!   refused, and a name + role already on the title is dropped.
//! - **`"current_truncated": true`** is added when the title's current
//!   value was too long to fit the 64 KiB item cap next to the proposed
//!   one ([`fit_item`]): the string lost its tail, or the list its last
//!   entries. The field-level baseline is then incomplete; staleness
//!   relies on `version` for that item (story 12-2,
//!   `proposals::title_moved`).
//! - **`"identity"`** (story 12-2) — the title's name, ISBN and media
//!   type at filing (`proposals::title_identity`): a re-identified title
//!   outdates the proposal.
//!
//! `current` is the value at filing, kept as filed (11-19 edits
//! `proposed` only; to re-validate an edit it calls [`validate_fields`]
//! with `{name, role}` contributor entries). It is also the field-level
//! staleness baseline that #521 may use: contributor writes
//! (`title_contributors`) do not bump `titles.version`.
//!
//! **For 11-18 (acceptance):** resolve each proposed contributor with
//! the SQL collation (`find_or_create`), then skip it when the title
//! already has that contributor id + role (re-check, it may have been
//! added since filing) and when the item resolves two entries to the
//! same contributor; a junction row soft-deleted earlier must be
//! reactivated rather than inserted (the unique key ignores
//! `deleted_at`, #531).
//!
//! Errors are a typed [`MetadataProposalError`] with agent-facing
//! English messages (D-11-9-2 precedent).

use std::collections::HashSet;
use std::fmt;

use chrono::{Datelike, NaiveDate};
use serde_json::{Map, Value, json};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

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
pub const PROPOSABLE_FIELDS: [&str; 7] = [
    "title",
    "subtitle",
    "description",
    "publisher",
    "publication_year",
    "dewey_code",
    "contributors",
];
/// `titles.title` width (story 12-5, #563).
pub const MAX_TITLE_CHARS: usize = 500;
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
/// At most this many field errors spelled out in one refusal.
pub const MAX_REPORTED_ERRORS: usize = 50;

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
    WrongType {
        expected: &'static str,
    },
    Missing,
    Blank,
    TooLongChars {
        max: usize,
    },
    TooLongBytes {
        max: usize,
    },
    ControlCharacter,
    DeweyShape,
    YearOutOfRange {
        min: i32,
        max: i32,
    },
    EmptyList,
    TooManyContributors {
        count: usize,
    },
    /// The valid roles are listed once, after every field error.
    UnknownRole {
        role: String,
    },
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
            Missing => write!(f, "is missing"),
            Blank => write!(
                f,
                "is empty; propose a value, or leave the field out (a proposal completes a \
                 title, it does not erase a value)"
            ),
            TooLongChars { max } => write!(f, "is longer than {max} characters"),
            TooLongBytes { max } => write!(f, "is longer than {max} bytes (UTF-8)"),
            ControlCharacter => write!(
                f,
                "contains a control or invisible character (line break, tab, zero-width or \
                 direction mark…)"
            ),
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
            UnknownRole { role } => {
                write!(f, "\"{role}\" is not a contributor role of this library")
            }
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
    /// One or more fields refused; nothing filed. `roles` are the
    /// library's contributor roles, listed once when a role was unknown.
    Fields {
        errors: Vec<FieldError>,
        roles: Vec<String>,
    },
    TitleNotFound {
        title_id: u64,
    },
    TargetChanged {
        title_id: u64,
        expected: i32,
        current: i32,
        /// The agent gave `version` (else the title moved during the call).
        pinned: bool,
    },
    /// Every proposed value is already the title's.
    NothingChanges {
        title_id: u64,
        fields: Vec<&'static str>,
    },
    Proposal(ProposalError),
}

impl Refusal for MetadataProposalError {
    fn class(&self) -> RefusalClass {
        use MetadataProposalError::*;
        match self {
            NoFields | Fields { .. } => RefusalClass::InvalidInput,
            TitleNotFound { .. } => RefusalClass::NotFound,
            TargetChanged { .. } | NothingChanges { .. } => RefusalClass::Conflict,
            Proposal(e) if e.is_invalid_input() => RefusalClass::InvalidInput,
            Proposal(e) if e.is_conflict() => RefusalClass::Conflict,
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
            Fields { errors, roles } => {
                write!(
                    f,
                    "{} field{} refused, nothing was filed:",
                    errors.len(),
                    if errors.len() == 1 { " was" } else { "s were" }
                )?;
                for e in errors.iter().take(MAX_REPORTED_ERRORS) {
                    write!(f, "\n- {e}.")?;
                }
                if errors.len() > MAX_REPORTED_ERRORS {
                    write!(f, "\n- … and {} more.", errors.len() - MAX_REPORTED_ERRORS)?;
                }
                if errors
                    .iter()
                    .any(|e| matches!(e.problem, FieldProblem::UnknownRole { .. }))
                {
                    if roles.is_empty() {
                        write!(f, "\nThe library defines no contributor role.")?;
                    } else {
                        write!(
                            f,
                            "\nThe library's contributor roles are: {}.",
                            roles.join(", ")
                        )?;
                    }
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
                pinned: true,
            } => write!(
                f,
                "Title {title_id} changed since you read it (version {expected}, now {current}): \
                 read it again with get_title and file the proposal on its current state."
            ),
            TargetChanged {
                title_id,
                pinned: false,
                ..
            } => write!(
                f,
                "Title {title_id} was edited while your proposal was being filed: nothing was \
                 filed; call again (read it with get_title first to see the new values)."
            ),
            NothingChanges { title_id, fields } => write!(
                f,
                "Nothing would change: title {title_id} already has the proposed value of {}. \
                 Nothing was filed.",
                fields.join(", ")
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
    /// Story 12-5 (#563) — a correction of the main title.
    pub title: Option<String>,
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
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<NaiveDate>,
    pub dewey_code: Option<String>,
    /// `(name, role_id, role name)` of each live contributor.
    pub contributors: Vec<(String, u64, String)>,
}

/// A name folded for comparison, close to the `utf8mb4_unicode_ci`
/// collation acceptance will match contributors with (D-11-13-9):
/// decomposed, accents dropped, lower case, inner whitespace collapsed.
pub fn fold_name(name: &str) -> String {
    name.to_lowercase()
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn same_name(a: &str, b: &str) -> bool {
    fold_name(a) == fold_name(b)
}

/// Characters refused in agent text: control characters (line breaks
/// and tabs allowed when `multiline`) and invisible format characters
/// (zero-width, direction marks and isolates, line / paragraph
/// separators, BOM) that could hide text or spoof a name (NFR47).
fn forbidden_char(c: char, multiline: bool) -> bool {
    (c.is_control() && !(multiline && matches!(c, '\n' | '\r' | '\t')))
        || matches!(
            c,
            '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{2060}'..='\u{2069}'
                | '\u{FEFF}'
        )
}

/// A field name as echoed in a message: at most 80 characters, without
/// control characters or backticks (agent input, D-11-13-9).
fn echo_key(key: &str) -> String {
    key.chars()
        .take(80)
        .map(|c| if c.is_control() || c == '`' { '?' } else { c })
        .collect()
}

/// The normal form proposed text is stored in, and current values are
/// compared in (round 2): trimmed, NFC, CRLF / CR as LF; one-line values
/// get runs of ordinary whitespace (space, tab, line break) collapsed to
/// one space — no-break spaces (French typography) are kept as they are.
pub fn normalize_text(raw: &str, multiline: bool) -> String {
    let nfc: String = raw.trim().nfc().collect();
    if multiline {
        return nfc.replace("\r\n", "\n").replace('\r', "\n");
    }
    let mut out = String::with_capacity(nfc.len());
    let mut in_run = false;
    for c in nfc.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r') {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out
}

/// A non-blank string in [`normalize_text`] form, within `max`
/// characters (counted after normalisation); single-line unless
/// `multiline` (then line breaks and tabs are allowed).
fn text(value: &Value, max: usize, multiline: bool) -> Result<String, FieldProblem> {
    let Some(raw) = value.as_str() else {
        return Err(FieldProblem::WrongType {
            expected: "a string",
        });
    };
    if raw.chars().any(|c| forbidden_char(c, multiline)) {
        return Err(FieldProblem::ControlCharacter);
    }
    let v = normalize_text(raw, multiline);
    if v.is_empty() {
        return Err(FieldProblem::Blank);
    }
    if v.chars().count() > max {
        return Err(FieldProblem::TooLongChars { max });
    }
    Ok(v)
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
    let out_of_range = FieldProblem::YearOutOfRange {
        min: MIN_PUBLICATION_YEAR,
        max,
    };
    match value.as_i64() {
        Some(y) => i32::try_from(y)
            .ok()
            .filter(|y| (MIN_PUBLICATION_YEAR..=max).contains(y))
            .ok_or(out_of_range),
        // An integer above i64::MAX is still an integer.
        None if value.is_u64() => Err(out_of_range),
        None => Err(FieldProblem::WrongType {
            expected: "an integer year (e.g. 1987)",
        }),
    }
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
                format!("{path}.{}", echo_key(key)),
                FieldProblem::WrongType {
                    expected: "absent (a contributor has only `name` and `role`)",
                },
            ));
        }
        let name = match obj.get("name") {
            None => Err(FieldProblem::Missing),
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
            None => Err(FieldProblem::Missing),
            Some(v) => text(v, usize::MAX, false).and_then(|r| {
                roles
                    .iter()
                    .find(|(_, known)| same_name(known, &r))
                    .cloned()
                    .ok_or_else(|| FieldProblem::UnknownRole { role: echo_key(&r) })
            }),
        };
        match (name, role) {
            (Ok(name), Ok((role_id, role))) => {
                if !seen.insert((fold_name(&name), role_id)) {
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
                    field: echo_key(key),
                    problem,
                });
            }
        };
        match key.as_str() {
            "title" => single(text(value, MAX_TITLE_CHARS, false).map(|v| out.title = Some(v))),
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
        Err(MetadataProposalError::Fields {
            errors,
            roles: roles.iter().map(|(_, n)| n.clone()).collect(),
        })
    }
}

/// The proposal items and the fields dropped as unchanged (pure — unit
/// tested), in [`PROPOSABLE_FIELDS`] order.
pub fn diff_against(
    proposed: &ProposedFields,
    current: &CurrentValues,
) -> (Vec<(&'static str, Value)>, Vec<&'static str>) {
    let mut items: Vec<(&'static str, Value)> = Vec::new();
    let mut unchanged = Vec::new();
    let mut text_field =
        |field: &'static str, new: &Option<String>, cur: &Option<String>, multiline: bool| {
            let Some(n) = new else { return };
            // A difference in normal form alone (spacing, NFD) is no change.
            if cur
                .as_deref()
                .map(|c| normalize_text(c, multiline))
                .as_deref()
                == Some(n.as_str())
            {
                unchanged.push(field);
            } else {
                items.push((
                    field,
                    json!({"line": "field", "field": field, "current": cur, "proposed": n}),
                ));
            }
        };
    text_field("title", &proposed.title, &current.title, false);
    text_field("subtitle", &proposed.subtitle, &current.subtitle, false);
    text_field(
        "description",
        &proposed.description,
        &current.description,
        true,
    );
    text_field("publisher", &proposed.publisher, &current.publisher, false);
    let current_year = current.publication_date.map(|d| d.year());
    let year_item = proposed.publication_year.map(|year| {
        (
            year,
            // `current_date` shows the reviewer the month and day that
            // acceptance (`YYYY-01-01`) replaces.
            json!({
                "line": "field", "field": "publication_year",
                "current": current_year,
                "current_date": current.publication_date.map(|d| d.format("%Y-%m-%d").to_string()),
                "proposed": year,
            }),
        )
    });
    text_field(
        "dewey_code",
        &proposed.dewey_code,
        &current.dewey_code,
        false,
    );
    if let Some((year, item)) = year_item {
        // Keep the documented order: publication_year before dewey_code.
        let at = items
            .iter()
            .position(|(f, _)| *f == "dewey_code")
            .unwrap_or(items.len());
        if Some(year) == current_year {
            let at_u = unchanged
                .iter()
                .position(|f| *f == "dewey_code")
                .unwrap_or(unchanged.len());
            unchanged.insert(at_u, "publication_year");
        } else {
            items.insert(at, ("publication_year", item));
        }
    }
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

/// Make an item fit [`MAX_ITEM_VALUES_BYTES`] by shortening its
/// `current` value (pure — unit tested): a string loses its tail, a list
/// its last entries, and `"current_truncated": true` is added. The
/// proposed value alone always fits (its bounds keep it far below the
/// cap), so a title whose current description or contributor list is
/// very long can still receive a proposal (D-11-13-9).
pub fn fit_item(mut item: Value) -> Value {
    let size = |v: &Value| v.to_string().len();
    if size(&item) <= MAX_ITEM_VALUES_BYTES {
        return item;
    }
    item["current_truncated"] = json!(true);
    loop {
        let excess = size(&item).saturating_sub(MAX_ITEM_VALUES_BYTES);
        if excess == 0 {
            return item;
        }
        match &mut item["current"] {
            Value::String(s) => {
                // Drop characters from the tail until their serialized
                // (escaped) size covers the excess.
                let mut freed = 0;
                while freed < excess {
                    let Some(c) = s.pop() else { return item };
                    freed += Value::String(c.to_string()).to_string().len() - 2;
                }
            }
            Value::Array(list) if !list.is_empty() => {
                list.pop();
            }
            _ => return item,
        }
    }
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
        let current_year = chrono::Utc::now().year();
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
                pinned: true,
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
        let identity = crate::services::proposals::title_identity(&title);
        let current = CurrentValues {
            title: Some(title.title),
            subtitle: title.subtitle,
            description: title.description,
            publisher: title.publisher,
            publication_date: title.publication_date,
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
        let proposal = NewProposal {
            kind: ProposalKind::Metadata,
            api_key_id: req.api_key_id,
            sources,
            items: items
                .into_iter()
                .map(|(_, mut values)| {
                    // Story 12-2 (review): the title's identity at filing.
                    values["identity"] = identity.clone();
                    NewProposalItem {
                        target: Some(target),
                        expected_version: Some(version),
                        proposed_values: fit_item(values),
                    }
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
                    pinned: req.version.is_some(),
                },
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
            Err(MetadataProposalError::Fields { errors, .. }) => errors,
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
            "genre": "Other",
            "subtitle": "   ",
            "publisher": 12,
            "publication_year": 999,
            "dewey_code": "abc",
            "description": null
        }));
        assert_eq!(errors.len(), 7, "{errors:?}");
        assert_eq!(problem_of(&errors, "isbn"), FieldProblem::NotProposable);
        assert_eq!(problem_of(&errors, "genre"), FieldProblem::NotProposable);
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
        let msg = MetadataProposalError::Fields {
            errors,
            roles: vec![],
        }
        .to_string();
        assert!(msg.starts_with("7 fields were refused, nothing was filed:"));
        assert!(msg.contains("`isbn` cannot be proposed; the fields an agent may propose are title, subtitle, description, publisher, publication_year, dewey_code, contributors"));
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
            FieldProblem::UnknownRole { ref role } if role == "Autor"
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
            FieldProblem::Missing
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
                .contains("The library defines no contributor role.")
        );
    }

    fn current() -> CurrentValues {
        CurrentValues {
            title: Some("Le Petit Prince".into()),
            subtitle: Some("Roman".into()),
            description: None,
            publisher: Some("Gallimard ".into()),
            publication_date: NaiveDate::from_ymd_opt(1987, 6, 15),
            dewey_code: None,
            contributors: vec![("Jean Dupont".into(), 1, "Auteur".into())],
        }
    }

    // Story 12-5 (#563): the title is proposable, never blank, ≤ 500 chars.
    #[test]
    fn title_is_validated_like_a_single_line_text() {
        let ok = validate_fields(&fields(json!({"title": "  Le  Petit   Prince "})), &[], 2026)
            .unwrap();
        assert_eq!(ok.title.as_deref(), Some("Le Petit Prince"));
        for bad in [json!(""), json!("   "), json!("a".repeat(MAX_TITLE_CHARS + 1)), json!(3)] {
            assert!(
                validate_fields(&fields(json!({ "title": bad })), &[], 2026).is_err(),
                "{bad}"
            );
        }
        assert_eq!(PROPOSABLE_FIELDS[0], "title");
    }

    #[test]
    fn a_title_correction_is_an_item_and_an_unchanged_title_is_dropped() {
        let fix = ProposedFields {
            title: Some("Le Petit Prince (édition illustrée)".into()),
            ..Default::default()
        };
        let (items, unchanged) = diff_against(&fix, &current());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].0, "title");
        assert_eq!(items[0].1["current"], json!("Le Petit Prince"));
        assert!(unchanged.is_empty());
        let same = ProposedFields {
            title: Some("Le Petit Prince".into()),
            ..Default::default()
        };
        let (items, unchanged) = diff_against(&same, &current());
        assert!(items.is_empty());
        assert_eq!(unchanged, vec!["title"]);
    }

    #[test]
    fn unchanged_fields_are_dropped_and_items_follow_the_documented_shapes() {
        let proposed = ProposedFields {
            title: None,
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
            json!({"line": "field", "field": "publication_year", "current": 1987, "current_date": "1987-06-15", "proposed": 1988})
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
        assert_eq!(
            E::Fields {
                errors: vec![],
                roles: vec![]
            }
            .class(),
            RefusalClass::InvalidInput
        );
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
                current: 2,
                pinned: true
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
        let pinned = E::TargetChanged {
            title_id: 7,
            expected: 1,
            current: 2,
            pinned: true,
        };
        assert!(
            pinned
                .to_string()
                .contains("changed since you read it (version 1, now 2)")
        );
        let unpinned = E::TargetChanged {
            title_id: 7,
            expected: 1,
            current: 2,
            pinned: false,
        };
        assert!(
            unpinned
                .to_string()
                .contains("was edited while your proposal was being filed")
        );
    }

    #[test]
    fn names_fold_like_the_collation() {
        assert_eq!(fold_name("  José   SARAMAGO "), "jose saramago");
        assert!(same_name("Hergé", "Herge\u{301}"));
        assert!(same_name("Jean  Dupont", "jean dupont"));
        assert!(!same_name("Jean Dupont", "Jean Dupond"));
        // Accent-folded duplicates in one proposal are refused.
        let errors = field_errors(json!({"contributors": [
            {"name": "José", "role": "Auteur"},
            {"name": "Jose", "role": "auteur"}
        ]}));
        assert_eq!(
            problem_of(&errors, "contributors[2]"),
            FieldProblem::ContributorTwice
        );
        // A role written without its accent is found.
        let roles = vec![(9, "Éditeur".to_string())];
        let ok = validate_fields(
            &fields(json!({"contributors": [{"name": "A", "role": "editeur"}]})),
            &roles,
            YEAR,
        )
        .unwrap();
        assert_eq!(ok.contributors.unwrap()[0].role, "Éditeur");
    }

    #[test]
    fn text_is_normalised_and_invisible_characters_are_refused() {
        let p = validate_fields(
            &fields(json!({"subtitle": "  Un   conte\u{a0}poétique ", "publisher": "Gallimard"})),
            &roles(),
            YEAR,
        )
        .unwrap();
        // Ordinary spaces collapse; the no-break space is kept.
        assert_eq!(p.subtitle.as_deref(), Some("Un conte\u{a0}poétique"));
        // NFD input is stored as NFC.
        let p = validate_fields(
            &fields(json!({"publisher": "Herge\u{301}"})),
            &roles(),
            YEAR,
        )
        .unwrap();
        assert_eq!(p.publisher.as_deref(), Some("Hergé"));
        for bad in [
            "\u{200B}",
            "a\u{202E}b",
            "a\u{2028}b",
            "\u{FEFF}x",
            "a\u{2066}b",
        ] {
            let errors = field_errors(json!({ "subtitle": bad }));
            assert_eq!(
                problem_of(&errors, "subtitle"),
                FieldProblem::ControlCharacter,
                "{bad:?}"
            );
            let errors = field_errors(json!({ "description": bad }));
            assert_eq!(
                problem_of(&errors, "description"),
                FieldProblem::ControlCharacter,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn echoed_keys_are_bounded_and_roles_listed_once() {
        let long = format!("x`{}\n", "k".repeat(200));
        let errors = field_errors(json!({ long.clone(): 1 }));
        assert_eq!(errors[0].field.chars().count(), 80);
        assert!(!errors[0].field.contains('`') && !errors[0].field.contains('\n'));

        let msg = validate_fields(
            &fields(json!({"contributors": [
                {"name": "A", "role": "Autor"}, {"name": "B", "role": "Painter"}
            ]})),
            &roles(),
            YEAR,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(msg.matches("Auteur, Illustrateur").count(), 1, "{msg}");
        assert!(msg.contains("The library's contributor roles are: Auteur, Illustrateur."));

        let many: Map<String, Value> = (0..60).map(|i| (format!("f{i}"), json!(1))).collect();
        let msg = validate_fields(&many, &roles(), YEAR)
            .unwrap_err()
            .to_string();
        assert!(msg.starts_with("60 fields were refused"), "{msg}");
        assert!(msg.contains("… and 10 more."), "{msg}");
    }

    #[test]
    fn a_huge_integer_year_is_out_of_range() {
        assert!(matches!(
            year(&json!(u64::MAX), YEAR),
            Err(FieldProblem::YearOutOfRange { .. })
        ));
    }

    #[test]
    fn publication_year_shows_the_full_current_date() {
        let proposed = ProposedFields {
            publication_year: Some(1988),
            dewey_code: Some("843".into()),
            ..Default::default()
        };
        let (items, _) = diff_against(&proposed, &current());
        assert_eq!(
            items[0].1,
            json!({"line": "field", "field": "publication_year", "current": 1987,
                   "current_date": "1987-06-15", "proposed": 1988})
        );
        assert_eq!(items[1].0, "dewey_code");
        let same = ProposedFields {
            publication_year: Some(1987),
            dewey_code: Some("843".into()),
            ..Default::default()
        };
        let (items, unchanged) = diff_against(&same, &current());
        assert_eq!(unchanged, ["publication_year"]);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn oversized_current_values_are_truncated_to_fit() {
        let small =
            json!({"line": "field", "field": "description", "current": "a", "proposed": "b"});
        assert_eq!(fit_item(small.clone()), small);

        // A current description of 70 000 quotes (140 000 serialized bytes).
        let item = json!({"line": "field", "field": "description",
                          "current": "\"".repeat(70_000), "proposed": "x".repeat(10_000)});
        let fitted = fit_item(item);
        assert!(fitted.to_string().len() <= MAX_ITEM_VALUES_BYTES);
        assert_eq!(fitted["current_truncated"], true);
        assert_eq!(fitted["proposed"], "x".repeat(10_000));
        assert!(fitted["current"].as_str().unwrap().len() > 1000);

        let current: Vec<Value> = (0..2000)
            .map(|i| json!({"name": format!("Contributor number {i}"), "role": "Auteur"}))
            .collect();
        let item = json!({"line": "field", "field": "contributors", "current": current,
                          "proposed": [{"name": "N", "role": "Auteur", "role_id": 1}]});
        let fitted = fit_item(item);
        assert!(fitted.to_string().len() <= MAX_ITEM_VALUES_BYTES);
        assert_eq!(fitted["current_truncated"], true);
        assert!(fitted["current"].as_array().unwrap().len() > 100);
    }

    #[test]
    fn a_difference_in_normal_form_alone_is_no_change() {
        let current = CurrentValues {
            subtitle: Some("Tome 1\u{a0}:  L'aube ".into()),
            publisher: Some("Herge\u{301}".into()),
            description: Some("a\r\nb".into()),
            dewey_code: Some("843\t".into()),
            ..Default::default()
        };
        let p = validate_fields(
            &fields(json!({
                "subtitle": "Tome 1\u{a0}: L'aube",
                "publisher": "Hergé",
                "description": "a\nb",
                "dewey_code": "843"
            })),
            &roles(),
            YEAR,
        )
        .unwrap();
        let (items, unchanged) = diff_against(&p, &current);
        assert!(items.is_empty(), "{items:?}");
        assert_eq!(
            unchanged,
            ["subtitle", "description", "publisher", "dewey_code"]
        );
        assert_eq!(fold_name("İlhan"), fold_name("ilhan"));
    }
}
