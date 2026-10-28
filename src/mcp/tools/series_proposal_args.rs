//! Story 11-12 — arguments, schema fragments and error mapping shared by
//! `propose_series_create` and `propose_series_attach` (pure).

use serde::Deserialize;
use serde_json::{Value, json};

use crate::mcp::tool::ToolError;
use crate::services::proposals::{MAX_ITEMS, MAX_SOURCE_CHARS, MAX_SOURCES};
use crate::services::series_proposals::{
    FiledProposal, RefusalClass, SeriesProposalError, TitleLine, TomeLine,
};

/// One entry of `titles`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TitleArg {
    title_id: u64,
    position: i64,
    #[serde(default)]
    is_omnibus: bool,
    end_position: Option<i64>,
    version: Option<i32>,
}

impl From<TitleArg> for TitleLine {
    fn from(a: TitleArg) -> Self {
        TitleLine {
            title_id: a.title_id,
            position: a.position,
            is_omnibus: a.is_omnibus,
            end_position: a.end_position,
            version: a.version,
        }
    }
}

/// One entry of `expected_tomes`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TomeArg {
    position: i64,
    title: String,
    isbn: Option<String>,
}

impl From<TomeArg> for TomeLine {
    fn from(a: TomeArg) -> Self {
        TomeLine {
            position: a.position,
            title: a.title,
            isbn: a.isbn,
        }
    }
}

/// Convert the argument lists.
pub fn lines(titles: Vec<TitleArg>, tomes: Vec<TomeArg>) -> (Vec<TitleLine>, Vec<TomeLine>) {
    (
        titles.into_iter().map(Into::into).collect(),
        tomes.into_iter().map(Into::into).collect(),
    )
}

/// Schema of `titles`.
pub fn titles_property() -> Value {
    json!({
        "type": "array",
        "maxItems": MAX_ITEMS - 1,
        "description": "Titles the library already holds, each with its position in the series. Find title ids with search_titles; a tome the library does not own goes in `expected_tomes` instead.",
        "items": {
            "type": "object",
            "properties": {
                "title_id": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Numeric id of the title (from search_titles or get_title)."
                },
                "position": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Position (tome number) of the title in the series, from 1. For an omnibus, the first position it covers."
                },
                "is_omnibus": {
                    "type": "boolean",
                    "default": false,
                    "description": "True when the title is an omnibus volume covering several positions; then `end_position` is required."
                },
                "end_position": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Omnibus only: the last position it covers (at least `position`)."
                },
                "version": {
                    "type": "integer",
                    "description": "Optional: the title's `version` as returned by get_title. When given and the title changed since, the proposal is refused so you can re-read it."
                }
            },
            "required": ["title_id", "position"],
            "additionalProperties": false
        }
    })
}

/// Schema of `expected_tomes`.
pub fn tomes_property() -> Value {
    json!({
        "type": "array",
        "maxItems": MAX_ITEMS - 1,
        "description": "Tomes of the series the library does not own yet (they show as expected on the series page once accepted).",
        "items": {
            "type": "object",
            "properties": {
                "position": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Position (tome number) of the tome in the series, from 1."
                },
                "title": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 500,
                    "description": "Title of the tome."
                },
                "isbn": {
                    "type": "string",
                    "maxLength": 40,
                    "description": "Optional ISBN-13 (978/979) or ISBN-10 of the tome; hyphens and spaces are allowed."
                }
            },
            "required": ["position", "title"],
            "additionalProperties": false
        }
    })
}

/// Schema of `sources`.
pub fn sources_property() -> Value {
    json!({
        "type": "array",
        "minItems": 1,
        "maxItems": MAX_SOURCES,
        "items": { "type": "string", "maxLength": MAX_SOURCE_CHARS },
        "description": "Required: the http(s) URLs of the pages where you found this information (publisher catalogue, library record, encyclopedia…). The librarian checks them before deciding."
    })
}

/// The answer to a successful filing.
pub fn filed_document(filed: &FiledProposal) -> Value {
    json!({
        "proposal_id": filed.proposal_id,
        "status": "pending",
        "kind": filed.kind.as_str(),
        "item_count": filed.item_count,
    })
}

/// Map a refusal to what the agent sees.
pub fn tool_error(e: SeriesProposalError, tool: &'static str) -> ToolError {
    match e.class() {
        RefusalClass::InvalidInput => ToolError::InvalidArguments(e.to_string()),
        RefusalClass::NotFound => ToolError::NotFound(e.to_string()),
        RefusalClass::Conflict => ToolError::Refused(e.to_string()),
        RefusalClass::Internal => {
            tracing::error!(error = ?e, tool, "MCP proposal tool failed");
            ToolError::Internal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::models::proposal::ProposalKind;
    use crate::services::proposals::ProposalError;
    use crate::services::series_proposals::PendingFor;

    #[test]
    fn refusals_map_to_actionable_tool_errors() {
        assert!(matches!(
            tool_error(SeriesProposalError::NothingToAttach, "t"),
            ToolError::InvalidArguments(_)
        ));
        assert!(matches!(
            tool_error(SeriesProposalError::TitleNotFound { title_id: 4 }, "t"),
            ToolError::NotFound(m) if m.contains("No title with id 4")
        ));
        assert!(matches!(
            tool_error(
                SeriesProposalError::PendingDuplicate {
                    proposal_id: 12,
                    series: PendingFor::SeriesName("X".into())
                },
                "t"
            ),
            ToolError::Refused(m) if m.contains("Proposal 12")
        ));
        assert_eq!(
            tool_error(
                SeriesProposalError::Proposal(ProposalError::Database(AppError::Internal(
                    "pw".into()
                ))),
                "t"
            ),
            ToolError::Internal
        );
    }

    #[test]
    fn filed_document_shape() {
        let doc = filed_document(&FiledProposal {
            proposal_id: 5,
            kind: ProposalKind::SeriesAttach,
            item_count: 3,
        });
        assert_eq!(
            doc,
            json!({"proposal_id": 5, "status": "pending", "kind": "series_attach", "item_count": 3})
        );
    }

    #[test]
    fn line_arguments_refuse_unknown_fields() {
        assert!(serde_json::from_value::<TitleArg>(json!({"title_id": 1, "position": 2})).is_ok());
        assert!(
            serde_json::from_value::<TitleArg>(json!({"title_id": 1, "position": 2, "pos": 3}))
                .is_err()
        );
        assert!(serde_json::from_value::<TomeArg>(json!({"position": 2, "title": "T"})).is_ok());
        assert!(
            serde_json::from_value::<TomeArg>(json!({"position": 2, "title": "T", "x": 1}))
                .is_err()
        );
    }
}
