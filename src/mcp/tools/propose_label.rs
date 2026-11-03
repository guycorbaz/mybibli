//! Story 13-9 (#581, FR156) — `propose_label`: propose attaching one of
//! the library's labels to a title or a volume.

use serde::Deserialize;
use serde_json::{Value, json};

use super::args;
use super::proposal_args::{filed_document, sources_property, tool_error};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::label::LabelTarget;
use crate::services::label_proposals::{
    LabelChoice, LabelProposalRequest, LabelProposalService, MAX_NOTE_CHARS,
};

pub struct ProposeLabel;

const TOOL: &str = "propose_label";

const EXPECTED: &str = "{\"target\": {\"title_id\": <id>} | {\"volume_id\": <id>}, \
     \"label_id\": <id> | \"label_name\": <exact name>, \"sources\": [<url>], \"note\"?: <text>}";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    title_id: Option<u64>,
    volume_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    target: Target,
    label_id: Option<u64>,
    label_name: Option<String>,
    note: Option<String>,
    sources: Vec<String>,
}

fn invalid(msg: &str) -> ToolError {
    ToolError::InvalidArguments(msg.to_string())
}

/// Validate the raw arguments' shape (pure — unit tested); the note and
/// source rules live in `services::label_proposals`.
fn parse_args(args: Value, api_key_id: u64) -> Result<LabelProposalRequest, ToolError> {
    let a: Args = args::parse_object(args, EXPECTED)?;
    let target = match (a.target.title_id, a.target.volume_id) {
        (Some(0), None) | (None, Some(0)) => {
            return Err(invalid("`target` ids are positive integers"));
        }
        (Some(id), None) => LabelTarget::Title(id),
        (None, Some(id)) => LabelTarget::Volume(id),
        _ => {
            return Err(invalid(
                "`target` holds exactly one of `title_id` or `volume_id`",
            ));
        }
    };
    let label = match (a.label_id, a.label_name) {
        (Some(0), None) => return Err(invalid("`label_id` is a positive integer")),
        (Some(id), None) => LabelChoice::Id(id),
        (None, Some(name)) => {
            let name = args::text("label_name", Some(name))?;
            if name.is_empty() {
                return Err(invalid("`label_name` is empty"));
            }
            LabelChoice::Name(name)
        }
        _ => return Err(invalid("give exactly one of `label_id` or `label_name`")),
    };
    Ok(LabelProposalRequest {
        api_key_id,
        target,
        label,
        note: a.note,
        sources: a.sources,
    })
}

impl McpTool for ProposeLabel {
    fn name(&self) -> &'static str {
        TOOL
    }

    fn title(&self) -> &'static str {
        "Propose a label on a title or a volume"
    }

    fn description(&self) -> &'static str {
        "Propose marking a doubtful record with one of the library's labels: give the `target` \
         (`{\"title_id\": …}` or `{\"volume_id\": …}` — volume ids are in get_title) and the \
         label (`label_id`, or `label_name` matching a name exactly; list them with \
         list_labels). Use it when a record looks wrong and a librarian should check it (an \
         ISBN that belongs to another book, a volume catalogued under the wrong title…); say \
         why in `note` (at most 500 characters, shown to the librarian). Every proposal needs \
         `sources`: the http(s) URLs of the pages that show the problem. This files ONE \
         proposal for a librarian to accept (the label is then attached) or reject; it does \
         not change the catalog. Returns `proposal_id`, `status` (`pending`) and `item_count` \
         (1). Refused with an explanation, nothing filed, when: the title or volume is unknown \
         or deleted; no label has that id or exact name (the message lists the library's \
         labels); the record already carries the label; a pending proposal already proposes \
         this label on this record; the same label on the same record was rejected less than \
         the library's resubmission delay ago (90 days by default; the message gives the date \
         from which it may be proposed again); a source is not an http(s) URL; the key was \
         revoked or lacks the propose scope."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "object",
                    "description": "The record to mark: exactly one of `title_id` or `volume_id`.",
                    "properties": {
                        "title_id": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Numeric id of the title (from search_titles or get_title)."
                        },
                        "volume_id": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Numeric id of the volume (from get_title's `volumes[].id`)."
                        }
                    },
                    "minProperties": 1,
                    "maxProperties": 1,
                    "additionalProperties": false
                },
                "label_id": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Id of the label (from list_labels). Give this or `label_name`."
                },
                "label_name": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": args::MAX_QUERY_CHARS,
                    "description": "Exact name of the label, case and accents included (from list_labels). Give this or `label_id`."
                },
                "note": {
                    "type": "string",
                    "maxLength": MAX_NOTE_CHARS,
                    "description": "Why the record is doubtful, for the librarian (plain text)."
                },
                "sources": sources_property()
            },
            "required": ["target", "sources"],
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        false
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let req = parse_args(args, ctx.api_key_id)?;
            let filed = LabelProposalService::propose(&ctx.state.pool, &req)
                .await
                .map_err(|e| tool_error(e, TOOL))?;
            Ok(filed_document(&filed))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "https://example.org/notice";

    #[test]
    fn parse_args_reads_a_volume_target_and_a_label_id() {
        let req = parse_args(
            json!({"target": {"volume_id": 12}, "label_id": 3, "note": "wrong ISBN",
                   "sources": [SRC]}),
            9,
        )
        .unwrap();
        assert_eq!(
            req,
            LabelProposalRequest {
                api_key_id: 9,
                target: LabelTarget::Volume(12),
                label: LabelChoice::Id(3),
                note: Some("wrong ISBN".into()),
                sources: vec![SRC.into()],
            }
        );
    }

    #[test]
    fn parse_args_reads_a_title_target_and_a_label_name() {
        let req = parse_args(
            json!({"target": {"title_id": 4}, "label_name": " Code douteux ", "sources": [SRC]}),
            1,
        )
        .unwrap();
        assert_eq!(req.target, LabelTarget::Title(4));
        assert_eq!(req.label, LabelChoice::Name("Code douteux".into()));
        assert_eq!(req.note, None);
    }

    #[test]
    fn parse_args_refuses_malformed_arguments() {
        for bad in [
            json!(null),
            json!([]),
            json!({"target": {"title_id": 1}, "label_id": 1}),
            json!({"label_id": 1, "sources": [SRC]}),
            json!({"target": {}, "label_id": 1, "sources": [SRC]}),
            json!({"target": {"title_id": 1, "volume_id": 2}, "label_id": 1, "sources": [SRC]}),
            json!({"target": {"title_id": 0}, "label_id": 1, "sources": [SRC]}),
            json!({"target": {"series_id": 1}, "label_id": 1, "sources": [SRC]}),
            json!({"target": {"title_id": 1}, "sources": [SRC]}),
            json!({"target": {"title_id": 1}, "label_id": 1, "label_name": "x", "sources": [SRC]}),
            json!({"target": {"title_id": 1}, "label_id": 0, "sources": [SRC]}),
            json!({"target": {"title_id": 1}, "label_name": "  ", "sources": [SRC]}),
            json!({"target": {"title_id": 1}, "label_name": "x".repeat(201), "sources": [SRC]}),
            json!({"target": {"title_id": 1}, "label_id": 1, "sources": [SRC], "reason": "x"}),
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
        let s = ProposeLabel.input_schema();
        assert_eq!(s["properties"]["note"]["maxLength"], json!(MAX_NOTE_CHARS));
        assert_eq!(MAX_NOTE_CHARS, 500);
        assert!(
            ProposeLabel
                .description()
                .contains("at most 500 characters")
        );
    }
}
