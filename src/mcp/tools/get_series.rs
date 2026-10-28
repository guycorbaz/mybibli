//! Story 11-8 — `get_series`: one series with its positions, omnibus
//! flags and missing positions. Expected tomes arrive in story 11-10.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{args, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::series::TitleSeriesRow;
use crate::services::series::{AgentSeriesDetail, SeriesService};
use crate::services::title_detail::has_cover;

pub struct GetSeries;

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

#[derive(Debug, Serialize)]
struct Position {
    position: i32,
    title_id: u64,
    title: String,
    media_type: String,
    primary_contributor: Option<String>,
    is_omnibus: bool,
    has_cover: bool,
}

impl From<TitleSeriesRow> for Position {
    fn from(r: TitleSeriesRow) -> Self {
        Position {
            position: r.position_number,
            title_id: r.title_id,
            title: r.title_name,
            media_type: r.media_type,
            primary_contributor: r.primary_contributor,
            is_omnibus: r.is_omnibus,
            has_cover: has_cover(r.cover_image_url.as_deref()),
        }
    }
}

#[derive(Debug, Serialize)]
struct Document {
    id: u64,
    name: String,
    description: Option<String>,
    series_type: String,
    total_volume_count: Option<i32>,
    owned_positions: u64,
    gap_count: Option<u64>,
    missing_positions: Vec<i32>,
    missing_positions_truncated: bool,
    positions: Vec<Position>,
}

impl From<AgentSeriesDetail> for Document {
    fn from(d: AgentSeriesDetail) -> Self {
        Document {
            id: d.series.id,
            name: d.series.name,
            description: d.series.description,
            series_type: d.series.series_type.to_string(),
            total_volume_count: d.series.total_volume_count,
            owned_positions: d.gaps.owned_positions,
            gap_count: d.gaps.gap_count,
            missing_positions: d.gaps.missing_positions,
            missing_positions_truncated: d.gaps.missing_positions_truncated,
            positions: d.positions.into_iter().map(Into::into).collect(),
        }
    }
}

impl McpTool for GetSeries {
    fn name(&self) -> &'static str {
        "get_series"
    }

    fn title(&self) -> &'static str {
        "Get a series"
    }

    fn description(&self) -> &'static str {
        // MAX_LISTED_MISSING_POSITIONS is 1000 (pinned by a unit test).
        "Return one series of the library catalog by its numeric id: name, description, \
         type (`closed` with a known `total_volume_count`, or `open`), and `positions`: every \
         title the library holds in it, ordered by position, with title id, title, media \
         type, main contributor, whether a cover exists and `is_omnibus` (an omnibus volume \
         covering several positions appears once per covered position). For a closed series, \
         `missing_positions` lists the positions 1..total the library does not hold (at most \
         1000; `missing_positions_truncated` says when the list was cut) and `gap_count` \
         counts them as total minus `owned_positions`; an open series has no known gaps \
         (`gap_count` null, `missing_positions` empty). Find ids with list_series; read a \
         title with get_title. Deleted series are reported as not found. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Numeric id of the series."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let id = parse_args(args)?;
            match SeriesService::agent_series_detail(&ctx.state.pool, id).await? {
                Some(detail) => to_json(Document::from(detail), "get_series"),
                None => Err(ToolError::NotFound(format!(
                    "No series with id {id} exists in the catalog."
                ))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::series::MAX_LISTED_MISSING_POSITIONS;

    #[test]
    fn parse_args_accepts_a_positive_id_only() {
        assert_eq!(parse_args(json!({"id": 7})), Ok(7));
        for bad in [
            json!(null),
            json!({}),
            json!({"id": 0}),
            json!({"id": "7"}),
            json!({"id": 1, "extra": true}),
        ] {
            assert!(
                matches!(parse_args(bad.clone()), Err(ToolError::InvalidArguments(_))),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn description_states_the_missing_positions_cap() {
        assert_eq!(MAX_LISTED_MISSING_POSITIONS, 1000);
        assert!(GetSeries.description().contains("at most 1000"));
    }
}
