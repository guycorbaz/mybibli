//! Story 15-6 (#564 part 2, FR166) — `list_volumes_to_value`: the volumes
//! an agent should value first — never valued, then the oldest estimates.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ListPage, args, page_property, resolve_genre, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::value_coverage::{VolumeToValue, VolumesToValueFilter};
use crate::services::title_detail::AgentVolumeValue;
use crate::services::volume_value_proposals::volumes_to_value;

pub struct ListVolumesToValue;

const TOOL: &str = "list_volumes_to_value";

const EXPECTED: &str = "{\"series_id\"?: integer, \"genre\"?: string, \"page\"?: integer}";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    series_id: Option<u64>,
    genre: Option<String>,
    page: Option<u32>,
}

/// Validated `(series_id, genre, page)` (pure — unit tested).
type Parsed = (Option<u64>, Option<String>, u32);

fn parse_args(args: Value) -> Result<Parsed, ToolError> {
    let a: Args = args::parse_object(args, EXPECTED)?;
    if a.series_id == Some(0) {
        return Err(ToolError::InvalidArguments(
            "`series_id` must be a positive integer".to_string(),
        ));
    }
    Ok((a.series_id, args::genre(a.genre)?, args::page(a.page)?))
}

/// A volume to value, as the agent reads it.
#[derive(Debug, Serialize)]
struct Item {
    volume_id: u64,
    label: String,
    title_id: u64,
    title: String,
    condition_name: Option<String>,
    #[serde(flatten)]
    value: AgentVolumeValue,
}

impl From<VolumeToValue> for Item {
    fn from(v: VolumeToValue) -> Self {
        Item {
            value: AgentVolumeValue::of(&v.state, v.edition_comment),
            volume_id: v.state.id,
            label: v.state.label,
            title_id: v.state.title_id,
            title: v.title,
            condition_name: v.condition_name,
        }
    }
}

impl McpTool for ListVolumesToValue {
    fn name(&self) -> &'static str {
        TOOL
    }

    fn title(&self) -> &'static str {
        "List the volumes to value first"
    }

    fn description(&self) -> &'static str {
        "List the library's physical volumes in the order to value them: first those never \
         valued, then those whose estimate has no date, then the oldest estimates first (ties \
         by title, then V-code), optionally only those of one series (`series_id`, from \
         list_series) or one `genre` (a genre name as shown by get_title; case and accents \
         are ignored). 25 per page; pass `page` (1-based) until `page` equals `total_pages`. \
         Each item gives `volume_id`, `label` (V-code), `title_id`, `title`, \
         `condition_name`, `edition_comment`, `purchase_price` (`{amount, currency}` or \
         null) and `current_value` (`{amount, currency, updated_at}` or null, ISO 8601 \
         UTC). Volumes with a value proposal still waiting for a librarian are left out. \
         Deleted volumes and titles never appear. Propose a value with \
         propose_volume_value (get_title gives the library's `default_currency`). Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "series_id": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Only volumes of titles in this series (from list_series)."
                },
                "genre": {
                    "type": "string",
                    "description": "Only volumes of titles of this genre (case and accents are ignored)."
                },
                "page": page_property("volumes")
            },
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let (series_id, genre, page) = parse_args(args)?;
            let genre_id = resolve_genre(ctx, genre.as_deref()).await?;
            let list = volumes_to_value(
                &ctx.state.pool,
                VolumesToValueFilter {
                    series_id,
                    genre_id,
                },
                page,
            )
            .await?;
            to_json(ListPage::<Item>::from(list), TOOL)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_defaults_and_bounds() {
        assert_eq!(parse_args(json!({})), Ok((None, None, 1)));
        assert_eq!(
            parse_args(json!({"series_id": 4, "genre": " BD ", "page": 2})),
            Ok((Some(4), Some("BD".into()), 2))
        );
        for bad in [
            json!({"series_id": 0}),
            json!({"page": 0}),
            json!({"volume_id": 3}),
            json!({"genre": 3}),
        ] {
            assert!(
                matches!(parse_args(bad.clone()), Err(ToolError::InvalidArguments(_))),
                "{bad} should be rejected"
            );
        }
    }
}
