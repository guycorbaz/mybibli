//! `get_title` — one title with its contributors, volumes and series.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{args, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::services::title_detail::find_agent_title_detail;

pub struct GetTitle;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    id: u64,
}

/// Validate the raw arguments (pure — unit tested).
fn parse_args(args: Value) -> Result<u64, ToolError> {
    let args: Args = args::parse_object(args, "{\"id\": <positive integer>}")?;
    if args.id == 0 {
        return Err(ToolError::InvalidArguments(
            "`id` must be a positive integer".to_string(),
        ));
    }
    Ok(args.id)
}

impl McpTool for GetTitle {
    fn name(&self) -> &'static str {
        "get_title"
    }

    fn title(&self) -> &'static str {
        "Get a title"
    }

    fn description(&self) -> &'static str {
        "Return one title of the library catalog by its numeric id: bibliographic fields \
         (title, subtitle, description, language, media type, ISBN/ISSN/UPC, publisher, \
         publication date, genre, Dewey code, page count…), its contributors with their \
         roles, its physical volumes with their shelf location, condition and `created_at` \
         (when the volume was entered), and its series assignments with positions \
         (`end_position` set for an omnibus covering several positions). Also `created_at` \
         (when the title was entered) and `updated_at` (its last change, including automatic \
         metadata refreshes), both ISO 8601 UTC like the volumes' dates; `has_cover` (whether \
         a cover image exists) and `manually_edited_fields` (fields the librarian edited by \
         hand; a metadata refresh never overwrites them). `detected_tome`: a tome number found in a \
         provider title (and removed from it) while the title was in no series — a hint, not \
         authoritative; the series positions are the title's tome numbers. `labels`: the \
         librarian's labels the title carries, and `volumes[].labels` those of each volume \
         (`{id, name}`; the whole list with list_labels; propose one with propose_label). \
         Each volume also gives `edition_comment`, `purchase_price` (`{amount, currency}` or \
         null) and `current_value` (`{amount, currency, updated_at}` or null), and \
         `default_currency` is the library's currency; propose a value with \
         propose_volume_value (list_volumes_to_value lists the volumes to value first). Find \
         ids with search_titles. Deleted titles are reported as not found. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Numeric id of the title."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let id = parse_args(args)?;
            match find_agent_title_detail(&ctx.state.pool, id).await? {
                Some(detail) => to_json(detail, "get_title"),
                None => Err(ToolError::NotFound(format!(
                    "No title with id {id} exists in the catalog."
                ))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_accepts_a_positive_id() {
        assert_eq!(parse_args(json!({"id": 42})), Ok(42));
    }

    #[test]
    fn parse_args_rejects_bad_shapes() {
        for bad in [
            json!(null),
            json!({}),
            json!({"id": 0}),
            json!({"id": -3}),
            json!({"id": "7"}),
            json!({"id": 1.5}),
            json!({"id": 1, "extra": true}),
            json!([1]),
        ] {
            assert!(
                matches!(parse_args(bad.clone()), Err(ToolError::InvalidArguments(_))),
                "{bad} should be rejected"
            );
        }
    }
}
