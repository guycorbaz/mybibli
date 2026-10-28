//! Story 11-12 — `propose_series_create`: propose a new series with the
//! titles it groups and the tomes the library does not own (FR130).

use serde::Deserialize;
use serde_json::{Value, json};

use super::args;
use super::series_proposal_args::{
    TitleArg, TomeArg, filed_document, lines, sources_property, titles_property, tomes_property,
    tool_error,
};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::series::SeriesType;
use crate::services::series_proposals::{SeriesCreateRequest, SeriesProposalService};

pub struct ProposeSeriesCreate;

const TOOL: &str = "propose_series_create";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    name: String,
    series_type: String,
    total_volume_count: Option<i64>,
    #[serde(default)]
    titles: Vec<TitleArg>,
    #[serde(default)]
    expected_tomes: Vec<TomeArg>,
    sources: Vec<String>,
}

/// Validate the raw arguments' shape (pure — unit tested); the rules
/// live in `services::series_proposals`.
fn parse_args(args: Value, api_key_id: u64) -> Result<SeriesCreateRequest, ToolError> {
    let a: Args = args::parse_object(
        args,
        "{\"name\": <string>, \"series_type\": \"open\"|\"closed\", \"sources\": [<url>], …}",
    )?;
    let series_type = match a.series_type.as_str() {
        "open" => SeriesType::Open,
        "closed" => SeriesType::Closed,
        _ => {
            return Err(ToolError::InvalidArguments(
                "`series_type` must be \"open\" or \"closed\"".to_string(),
            ));
        }
    };
    let (titles, expected_tomes) = lines(a.titles, a.expected_tomes);
    Ok(SeriesCreateRequest {
        api_key_id,
        name: a.name,
        series_type,
        total_volume_count: a.total_volume_count,
        titles,
        expected_tomes,
        sources: a.sources,
    })
}

impl McpTool for ProposeSeriesCreate {
    fn name(&self) -> &'static str {
        TOOL
    }

    fn title(&self) -> &'static str {
        "Propose a new series"
    }

    fn description(&self) -> &'static str {
        "Propose to create a series that does not exist in the library yet: its name, its \
         type (`closed` with the known `total_volume_count`, or `open` when the series is \
         ongoing or its total unknown), the titles the library already holds with their \
         position (and `is_omnibus` with `end_position` for an omnibus covering several \
         positions), and the tomes it does not own yet (`expected_tomes`: position, title, \
         optional ISBN). Every proposal needs `sources`: the http(s) URLs where you found the \
         information. This files ONE proposal for a librarian to accept, edit or reject; it \
         does not change the catalog. Returns `proposal_id` and `status` (`pending`). Refused \
         with an explanation when: a series with that name already exists, case and accents \
         ignored (the message gives its id: use propose_series_attach instead); a proposal \
         for the same series name is still pending (the message gives its id); the name is \
         empty or longer than 255 characters; a closed series has no total, or an open one \
         has one; a title id is unknown or deleted, or a title's `version` no longer matches; \
         a title is listed twice; a position is below 1, above 10000, above a closed series' \
         total, or used twice in the proposal (omnibus ranges included); `end_position` is \
         given without `is_omnibus`, or an omnibus lacks it or covers more than 100 \
         positions; `titles` and `expected_tomes` hold more than 999 entries together; an \
         expected tome has no title, an invalid ISBN or the ISBN of another tome; a source is \
         not an http(s) URL. Check existing series with list_series first."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 255,
                    "description": "Name of the series, as published (e.g. \"Blake et Mortimer\")."
                },
                "series_type": {
                    "type": "string",
                    "enum": ["open", "closed"],
                    "description": "`closed` when the series is finished and its number of tomes known (give `total_volume_count`); `open` otherwise."
                },
                "total_volume_count": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10000,
                    "description": "Number of tomes of a closed series; required for `closed`, refused for `open`."
                },
                "titles": titles_property(),
                "expected_tomes": tomes_property(),
                "sources": sources_property()
            },
            "required": ["name", "series_type", "sources"],
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        false
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let req = parse_args(args, ctx.api_key_id)?;
            let filed = SeriesProposalService::propose_create(&ctx.state.pool, &req)
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
                "name": "Blake",
                "series_type": "closed",
                "total_volume_count": 3,
                "titles": [{"title_id": 4, "position": 1}],
                "expected_tomes": [{"position": 2, "title": "T2", "isbn": "2205001213"}],
                "sources": ["https://example.org/blake"]
            }),
            9,
        )
        .unwrap();
        assert_eq!(req.api_key_id, 9);
        assert_eq!(req.series_type, SeriesType::Closed);
        assert_eq!(req.total_volume_count, Some(3));
        assert_eq!(req.titles.len(), 1);
        assert_eq!(req.expected_tomes[0].isbn.as_deref(), Some("2205001213"));
    }

    #[test]
    fn parse_args_refuses_malformed_arguments() {
        for bad in [
            json!(null),
            json!({"name": "S", "series_type": "closed"}),
            json!({"name": "S", "series_type": "finished", "sources": ["https://a.b"]}),
            json!({"name": "S", "series_type": "open", "sources": ["https://a.b"], "extra": 1}),
            json!({"name": "S", "series_type": "open", "sources": "https://a.b"}),
            json!({"name": "S", "series_type": "open", "sources": ["https://a.b"],
                   "titles": [{"title_id": 1}]}),
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
