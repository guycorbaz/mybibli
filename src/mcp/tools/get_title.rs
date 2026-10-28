//! `get_title` — one title with its contributors, volumes and series.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::services::title_detail::find_title_detail;

pub struct GetTitle;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    id: u64,
}

/// Validate the raw arguments (pure — unit tested).
fn parse_args(args: Value) -> Result<u64, ToolError> {
    // serde would also read a struct from a sequence (`[1]`); only the
    // object form of the schema is accepted.
    if !args.is_object() {
        return Err(ToolError::InvalidArguments(
            "expected an object {\"id\": <positive integer>}".to_string(),
        ));
    }
    let args: Args = serde_json::from_value(args).map_err(|e| {
        ToolError::InvalidArguments(format!("expected {{\"id\": <positive integer>}} ({e})"))
    })?;
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
         roles, its physical volumes with their shelf location and condition, and its \
         series assignments with positions. Deleted titles are reported as not found. \
         Read-only."
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
            match find_title_detail(&ctx.state.pool, id).await? {
                Some(detail) => serde_json::to_value(detail).map_err(|e| {
                    tracing::error!(error = %e, "get_title: serialization failed");
                    ToolError::Internal
                }),
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
