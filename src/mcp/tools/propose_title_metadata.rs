//! Story 11-13 — `propose_title_metadata`: propose the missing
//! information of a title (FR132).

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::args;
use super::proposal_args::{filed_document, sources_property, tool_error};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::services::metadata_proposals::{
    MAX_CONTRIBUTORS, MAX_DESCRIPTION_CHARS, MAX_DEWEY_CHARS, MAX_PUBLISHER_CHARS,
    MAX_SUBTITLE_CHARS, MetadataProposalRequest, MetadataProposalService,
};

pub struct ProposeTitleMetadata;

const TOOL: &str = "propose_title_metadata";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    title_id: u64,
    version: Option<i32>,
    /// Kept as a map: the service names every refused field itself.
    fields: Map<String, Value>,
    sources: Vec<String>,
}

/// Validate the raw arguments' shape (pure — unit tested); the field
/// rules live in `services::metadata_proposals`.
fn parse_args(args: Value, api_key_id: u64) -> Result<MetadataProposalRequest, ToolError> {
    let a: Args = args::parse_object(
        args,
        "{\"title_id\": <id>, \"fields\": {…}, \"sources\": [<url>], \"version\"?: <int>}",
    )?;
    Ok(MetadataProposalRequest {
        api_key_id,
        title_id: a.title_id,
        version: a.version,
        fields: a.fields,
        sources: a.sources,
    })
}

impl McpTool for ProposeTitleMetadata {
    fn name(&self) -> &'static str {
        TOOL
    }

    fn title(&self) -> &'static str {
        "Propose a title's missing information"
    }

    fn description(&self) -> &'static str {
        "Propose values for a title's missing or wrong information: `subtitle`, \
         `description`, `publisher`, `publication_year`, `dewey_code`, and `contributors` \
         to add (each with its name and one of the library's contributor roles, as get_title \
         shows them, e.g. \"Auteur\"). Read the title with get_title first and pass its \
         `version`. Every proposal needs `sources`: the http(s) URLs where you found the \
         information. This files ONE proposal for a librarian to accept, edit or reject; it \
         does not change the catalog. Each field is stored next to the title's current \
         value; a field whose value would not change is dropped and listed in \
         `unchanged_fields` (a contributor already on the title with that role is dropped \
         the same way). Returns `proposal_id`, `status` (`pending`), `item_count` and \
         `unchanged_fields`. Refused with an explanation when: the title id is unknown or \
         deleted, or its `version` no longer matches (read it again); every proposed value \
         is already the title's; `fields` is empty. Refused field by field, every bad field \
         named at once and nothing filed, when: a field is not one of the six above; a value \
         is not of the expected type, or empty (a proposal completes a title, it never \
         erases a value); `subtitle` exceeds 500 characters, `publisher` 255, `description` \
         10000, `dewey_code` 32; a single-line value contains a control character (line \
         breaks and tabs are allowed in `description` only); `dewey_code` is not made of \
         digits, '.' and '/'; `publication_year` is before 1000 or after next year; \
         `contributors` is empty, holds more than 50 entries, repeats a name and role, has a \
         blank name or one over 255 bytes, or a role the library does not define (the \
         message lists the valid roles). A source that is not an http(s) URL is refused."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title_id": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Numeric id of the title (from search_titles or get_title)."
                },
                "version": {
                    "type": "integer",
                    "description": "The title's `version` as returned by get_title. When given and the title changed since, the proposal is refused so you can re-read it. Without it, a title edited while the call runs is refused too (retry)."
                },
                "fields": {
                    "type": "object",
                    "minProperties": 1,
                    "description": "The values you propose; give only the fields you found.",
                    "properties": {
                        "subtitle": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": MAX_SUBTITLE_CHARS,
                            "description": "Subtitle, one line."
                        },
                        "description": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": MAX_DESCRIPTION_CHARS,
                            "description": "Summary or back-cover text, plain text (line breaks allowed; never HTML)."
                        },
                        "publisher": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": MAX_PUBLISHER_CHARS,
                            "description": "Publisher's name, one line."
                        },
                        "publication_year": {
                            "type": "integer",
                            "minimum": 1000,
                            "description": "Year of publication of this edition (e.g. 1987), at most next year."
                        },
                        "dewey_code": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": MAX_DEWEY_CHARS,
                            "pattern": "^[0-9./]+$",
                            "description": "Dewey Decimal Classification class (e.g. \"843.914\")."
                        },
                        "contributors": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": MAX_CONTRIBUTORS,
                            "description": "Contributors to add to the title (none is removed). Whether the person already exists in the library is settled when the librarian accepts.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": {
                                        "type": "string",
                                        "minLength": 1,
                                        "description": "Name as the library writes it (see search_contributors), e.g. \"Hergé\"."
                                    },
                                    "role": {
                                        "type": "string",
                                        "minLength": 1,
                                        "description": "One of the library's contributor roles, as get_title shows them (e.g. \"Auteur\", \"Illustrateur\"); case is ignored."
                                    }
                                },
                                "required": ["name", "role"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "additionalProperties": false
                },
                "sources": sources_property()
            },
            "required": ["title_id", "fields", "sources"],
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        false
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let req = parse_args(args, ctx.api_key_id)?;
            let filed = MetadataProposalService::propose(&ctx.state.pool, &req)
                .await
                .map_err(|e| tool_error(e, TOOL))?;
            let mut doc = filed_document(&filed.filed);
            doc["unchanged_fields"] = json!(filed.unchanged_fields);
            Ok(doc)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_reads_the_whole_request() {
        let req = parse_args(
            json!({
                "title_id": 4,
                "version": 3,
                "fields": {"subtitle": "Roman", "isbn": "x"},
                "sources": ["https://example.org/t"]
            }),
            9,
        )
        .unwrap();
        assert_eq!((req.api_key_id, req.title_id, req.version), (9, 4, Some(3)));
        // Unknown field names reach the service, which names them.
        assert_eq!(req.fields.len(), 2);
    }

    #[test]
    fn parse_args_refuses_malformed_arguments() {
        for bad in [
            json!(null),
            json!({"title_id": 1, "sources": ["https://a.b"]}),
            json!({"title_id": 1, "fields": {}}),
            json!({"title_id": 1, "fields": [], "sources": ["https://a.b"]}),
            json!({"title_id": "1", "fields": {}, "sources": ["https://a.b"]}),
            json!({"title_id": 1, "fields": {}, "sources": ["https://a.b"], "extra": 1}),
        ] {
            assert!(
                matches!(
                    parse_args(bad.clone(), 1),
                    Err(ToolError::InvalidArguments(_))
                ),
                "{bad} should be refused"
            );
        }
    }

    #[test]
    fn schema_bounds_follow_the_service() {
        let s = ProposeTitleMetadata.input_schema();
        let f = &s["properties"]["fields"]["properties"];
        assert_eq!(f["subtitle"]["maxLength"], 500);
        assert_eq!(f["publisher"]["maxLength"], 255);
        assert_eq!(f["description"]["maxLength"], 10_000);
        assert_eq!(f["dewey_code"]["maxLength"], 32);
        assert_eq!(f["contributors"]["maxItems"], 50);
        let names: Vec<_> = f.as_object().unwrap().keys().cloned().collect();
        let mut expected: Vec<String> = crate::services::metadata_proposals::PROPOSABLE_FIELDS
            .iter()
            .map(|s| s.to_string())
            .collect();
        expected.sort();
        let mut names = names;
        names.sort();
        assert_eq!(names, expected);
    }
}
