//! Story 11-8 — `list_series`: the library's series with their held
//! positions and gaps, one page at a time.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ListPage, args, page_property, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::series::SeriesSummaryRow;
use crate::services::series::{SeriesService, gap_count};

pub struct ListSeries;

const EXPECTED: &str = "{\"query\"?: string, \"gaps_only\"?: boolean, \"page\"?: integer}";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    query: Option<String>,
    gaps_only: Option<bool>,
    page: Option<u32>,
}

/// Validated `(query, gaps_only, page)` (pure — unit tested).
fn parse_args(args: Value) -> Result<(String, bool, u32), ToolError> {
    let a: Args = args::parse_object(args, EXPECTED)?;
    Ok((
        args::query(a.query)?,
        a.gaps_only.unwrap_or(false),
        args::page(a.page)?,
    ))
}

#[derive(Debug, Serialize)]
struct Item {
    id: u64,
    name: String,
    series_type: String,
    total_volume_count: Option<i32>,
    owned_positions: u64,
    gap_count: Option<u64>,
}

impl From<SeriesSummaryRow> for Item {
    fn from(r: SeriesSummaryRow) -> Self {
        Item {
            gap_count: gap_count(r.series_type, r.total_volume_count, r.owned_count),
            id: r.id,
            name: r.name,
            series_type: r.series_type.to_string(),
            total_volume_count: r.total_volume_count,
            owned_positions: r.owned_count,
        }
    }
}

impl McpTool for ListSeries {
    fn name(&self) -> &'static str {
        "list_series"
    }

    fn title(&self) -> &'static str {
        "List series"
    }

    fn description(&self) -> &'static str {
        "List the series of the library catalog whose name contains `query` \
         (case-insensitive); omit `query` to list them all. A series is `closed` (it has a \
         known total number of tomes, `total_volume_count`) or `open` (ongoing, no total). \
         Each item gives the series id (use get_series for its positions and missing tomes), \
         name, type, total, `owned_positions` (distinct positions held by the library) and \
         `gap_count` (total minus owned positions; null for an open series, whose gaps are \
         unknown). Set `gaps_only` to list only the closed series with at least one gap. \
         Sorted by name, 25 per page; pass `page` (1-based) until `page` equals \
         `total_pages`. Deleted series and titles never appear or count. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "maxLength": args::MAX_QUERY_CHARS,
                    "description": "Part of the series name. Empty or omitted: all series."
                },
                "gaps_only": {
                    "type": "boolean",
                    "default": false,
                    "description": "true: only the closed series missing at least one tome."
                },
                "page": page_property("series")
            },
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let (query, gaps_only, page) = parse_args(args)?;
            let list =
                SeriesService::list_summaries(&ctx.state.pool, &query, gaps_only, page).await?;
            to_json(ListPage::<Item>::from(list), "list_series")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::series::SeriesType;

    #[test]
    fn parse_args_defaults_and_bounds() {
        assert_eq!(parse_args(json!({})), Ok((String::new(), false, 1)));
        assert_eq!(
            parse_args(json!({"query": " Astérix ", "gaps_only": true, "page": 2})),
            Ok(("Astérix".into(), true, 2))
        );
        for bad in [
            json!({"page": 0}),
            json!({"gaps_only": "yes"}),
            json!({"limit": 5}),
            json!({"query": "q".repeat(201)}),
        ] {
            assert!(
                matches!(parse_args(bad.clone()), Err(ToolError::InvalidArguments(_))),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn item_reports_gaps_for_closed_series_only() {
        let row = |series_type, total| SeriesSummaryRow {
            id: 1,
            name: "S".into(),
            series_type,
            total_volume_count: total,
            owned_count: 2,
        };
        let closed = serde_json::to_value(Item::from(row(SeriesType::Closed, Some(5)))).unwrap();
        assert_eq!(closed["series_type"], "closed");
        assert_eq!(closed["gap_count"], 3);
        let open = serde_json::to_value(Item::from(row(SeriesType::Open, None))).unwrap();
        assert_eq!(open["series_type"], "open");
        assert!(open["gap_count"].is_null());
        assert_eq!(open["owned_positions"], 2);
    }
}
