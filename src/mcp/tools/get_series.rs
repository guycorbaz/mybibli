//! Story 11-8 — `get_series`: one series with its positions, omnibus
//! flags and missing positions; story 11-10 adds its expected tomes.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{args, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::expected_tome::ExpectedTomeRow;
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

/// Story 11-10 — a tome the library does not own yet.
#[derive(Debug, Serialize)]
struct ExpectedTome {
    id: u64,
    position: i32,
    title: String,
    isbn: Option<String>,
    origin: &'static str,
    settled_title_id: Option<u64>,
}

impl From<ExpectedTomeRow> for ExpectedTome {
    fn from(r: ExpectedTomeRow) -> Self {
        ExpectedTome {
            id: r.id,
            position: r.position,
            title: r.title,
            isbn: r.isbn,
            origin: r.origin.as_str(),
            settled_title_id: r.settled_title_id,
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
    expected_tomes: Vec<ExpectedTome>,
    /// Story 11-12 — pin it in `propose_series_attach.series_version`.
    version: i32,
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
            expected_tomes: d.expected_tomes.into_iter().map(Into::into).collect(),
            version: d.series.version,
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
         counts them as total minus `owned_positions` (the distinct positions held within \
         1..total; a title listed above the total fills no slot); an open series has no \
         known gaps (`gap_count` null, `missing_positions` empty). `expected_tomes` lists the \
         tomes the librarian expects but may not own yet, ordered by position: id, position, \
         title, ISBN-13 (or null), `origin` (`manual` or `proposal`) and `settled_title_id` \
         (the catalogued title holding its position, null while not settled — a position held by \
         another title also leaves it null). `version` changes whenever the series is \
         edited; pass it as `series_version` to propose_series_attach. Find ids with \
         list_series; read a title with get_title. Deleted series are reported as not found. Read-only."
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

    #[test]
    fn document_carries_the_expected_tomes() {
        use crate::models::expected_tome::ExpectedTomeOrigin;
        use crate::models::series::{SeriesModel, SeriesType};
        use crate::services::series::series_gaps;
        let detail = AgentSeriesDetail {
            series: SeriesModel {
                id: 4,
                name: "S".into(),
                description: None,
                series_type: SeriesType::Closed,
                total_volume_count: Some(3),
                version: 6,
            },
            positions: Vec::new(),
            gaps: series_gaps(SeriesType::Closed, Some(3), &[]),
            expected_tomes: vec![ExpectedTomeRow {
                id: 9,
                series_id: 4,
                position: 2,
                title: "Tome 2".into(),
                isbn: Some("9782070360246".into()),
                origin: ExpectedTomeOrigin::Proposal,
                settled_title_id: None,
                version: 1,
            }],
        };
        let v = serde_json::to_value(Document::from(detail)).unwrap();
        assert_eq!(
            v["expected_tomes"],
            json!([{
                "id": 9,
                "position": 2,
                "title": "Tome 2",
                "isbn": "9782070360246",
                "origin": "proposal",
                "settled_title_id": null
            }])
        );
        assert!(GetSeries.description().contains("expected_tomes"));
        // Story 11-12: the series version, for `series_version`.
        assert_eq!(v["version"], 6);
        assert!(GetSeries.description().contains("series_version"));
    }
}
