//! Arguments, schema fragments and error mapping shared by every
//! proposal tool (stories 11-12, 11-13, 11-14) (pure).

use serde_json::{Value, json};

use crate::mcp::tool::ToolError;
use crate::services::proposals::{
    FiledProposal, MAX_SOURCE_CHARS, MAX_SOURCES, Refusal, RefusalClass,
};

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
pub fn tool_error(e: impl Refusal, tool: &'static str) -> ToolError {
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
    use crate::services::series_proposals::{PendingFor, SeriesProposalError};

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
    fn metadata_refusals_map_through_the_same_helper() {
        use crate::services::metadata_proposals::MetadataProposalError as M;
        assert!(matches!(
            tool_error(M::NoFields, "t"),
            ToolError::InvalidArguments(m) if m.contains("`fields` is empty")
        ));
        assert!(matches!(
            tool_error(M::TitleNotFound { title_id: 3 }, "t"),
            ToolError::NotFound(m) if m.contains("No title with id 3")
        ));
        assert!(matches!(
            tool_error(M::NothingChanges { title_id: 3, fields: vec!["publisher"] }, "t"),
            ToolError::Refused(m) if m.contains("Nothing would change")
        ));
        assert_eq!(
            tool_error(
                M::Proposal(ProposalError::Database(AppError::Internal("pw".into()))),
                "t"
            ),
            ToolError::Internal
        );
    }
}
