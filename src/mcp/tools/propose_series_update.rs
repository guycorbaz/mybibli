//! Story 15-4 — `propose_series_update`: propose to change an existing
//! series' name, type or total, and to remove expected tomes (FR164).

use serde::Deserialize;
use serde_json::{Value, json};

use super::args;
use super::proposal_args::{filed_document, sources_property, tool_error};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::series::SeriesType;
use crate::services::series_update_proposals::{
    SeriesUpdateProposalService, SeriesUpdateRequest,
};

pub struct ProposeSeriesUpdate;

const TOOL: &str = "propose_series_update";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    series_id: u64,
    series_version: Option<i32>,
    name: Option<String>,
    series_type: Option<String>,
    total_volume_count: Option<i64>,
    #[serde(default)]
    remove_expected_tomes: Vec<u64>,
    sources: Vec<String>,
}

/// Validate the raw arguments' shape (pure — unit tested).
fn parse_args(args: Value, api_key_id: u64) -> Result<SeriesUpdateRequest, ToolError> {
    let a: Args = args::parse_object(
        args,
        "{\"series_id\": <positive integer>, \"series_type\": \"closed\", \"total_volume_count\": 24, \"remove_expected_tomes\": [<tome id>], \"sources\": [<url>]}",
    )?;
    if a.series_id == 0 {
        return Err(ToolError::InvalidArguments(
            "`series_id` must be a positive integer".to_string(),
        ));
    }
    let series_type = match a.series_type.as_deref() {
        None => None,
        Some("open") => Some(SeriesType::Open),
        Some("closed") => Some(SeriesType::Closed),
        Some(other) => {
            return Err(ToolError::InvalidArguments(format!(
                "`series_type` must be \"open\" or \"closed\" (got \"{other}\")"
            )));
        }
    };
    Ok(SeriesUpdateRequest {
        api_key_id,
        series_id: a.series_id,
        series_version: a.series_version,
        name: a.name,
        series_type,
        total_volume_count: a.total_volume_count,
        remove_expected_tomes: a.remove_expected_tomes,
        sources: a.sources,
    })
}

impl McpTool for ProposeSeriesUpdate {
    fn name(&self) -> &'static str {
        TOOL
    }

    fn title(&self) -> &'static str {
        "Propose changes to an existing series"
    }

    fn description(&self) -> &'static str {
        "Propose to correct an existing series: its `name`, its `series_type` (\"open\" or \
         \"closed\") and `total_volume_count` (the number of tomes of a closed series), and/or \
         to remove expected tomes that do not exist (`remove_expected_tomes`: their ids from \
         get_series). Give only what changes, and `sources`: the http(s) URLs where you found \
         the information. To close an open series, give `series_type` \"closed\" with the \
         total; a closed series takes a new total alone. Read the series with get_series \
         first: a closed total may not be lower than a position the library owns, nor than \
         the position of an expected tome — propose removing those expected tomes in the same \
         call. This files ONE proposal for a librarian to accept, edit or reject; it does not \
         change the catalog. Returns `proposal_id` and `status` (`pending`). Refused with an \
         explanation when: the series id is unknown or deleted; nothing would change; the \
         name is empty, longer than 255 characters or another series' name; a total is given \
         for an open series without `series_type` \"closed\", or is below 1 or above 10000; \
         the total would leave owned positions or expected tomes outside the series (the \
         message names them); an expected tome id is not one of this series or is listed \
         twice; a proposal for this series is still pending (the message gives its id); the \
         same change was rejected recently; `series_version` no longer matches; a source is \
         not an http(s) URL. To add titles or expected tomes, use propose_series_attach."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "series_id": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Numeric id of the existing series (from list_series or get_series)."
                },
                "series_version": {
                    "type": "integer",
                    "description": "Optional: the series' `version` as returned by get_series. When given and the series' own fields changed since, the proposal is refused so you can re-read it."
                },
                "name": {
                    "type": "string",
                    "maxLength": 255,
                    "description": "The corrected series name; omit to keep the current one."
                },
                "series_type": {
                    "type": "string",
                    "enum": ["open", "closed"],
                    "description": "\"closed\" for a finished series with a known number of tomes, \"open\" otherwise; omit to keep the current type."
                },
                "total_volume_count": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10000,
                    "description": "The number of tomes of the closed series; omit to keep the current total."
                },
                "remove_expected_tomes": {
                    "type": "array",
                    "items": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Id of an expected tome of this series (from get_series)."
                    },
                    "description": "Expected tomes of this series to remove (tomes that do not exist, or above the corrected total)."
                },
                "sources": sources_property()
            },
            "required": ["series_id", "sources"],
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        false
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let req = parse_args(args, ctx.api_key_id)?;
            let filed = SeriesUpdateProposalService::propose_update(&ctx.state.pool, &req)
                .await
                .map_err(|e| tool_error(e, TOOL))?;
            Ok(filed_document(&filed))
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
                "series_id": 3,
                "series_version": 2,
                "name": "Tintin",
                "series_type": "closed",
                "total_volume_count": 24,
                "remove_expected_tomes": [7, 8],
                "sources": ["https://example.org/s"]
            }),
            9,
        )
        .unwrap();
        assert_eq!(
            (req.series_id, req.series_version, req.api_key_id),
            (3, Some(2), 9)
        );
        assert_eq!(req.series_type, Some(SeriesType::Closed));
        assert_eq!(req.total_volume_count, Some(24));
        assert_eq!(req.remove_expected_tomes, vec![7, 8]);
        let bare = parse_args(json!({"series_id": 3, "sources": ["https://a.b"]}), 1).unwrap();
        assert_eq!(bare.series_type, None);
        assert!(bare.remove_expected_tomes.is_empty());
    }

    #[test]
    fn parse_args_refuses_malformed_arguments() {
        for bad in [
            json!(null),
            json!({"series_id": 0, "sources": ["https://a.b"]}),
            json!({"series_id": 3}),
            json!({"series_id": 3, "sources": ["https://a.b"], "series_type": "finished"}),
            json!({"series_id": 3, "sources": ["https://a.b"], "titles": []}),
            json!({"series_id": 3, "sources": ["https://a.b"], "remove_expected_tomes": ["7"]}),
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
}
