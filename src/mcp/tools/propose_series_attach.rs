//! Story 11-12 — `propose_series_attach`: propose titles and expected
//! tomes for a series that already exists (FR131).

use serde::Deserialize;
use serde_json::{Value, json};

use super::args;
use super::series_proposal_args::{
    TitleArg, TomeArg, filed_document, lines, sources_property, titles_property, tomes_property,
    tool_error,
};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::services::series_proposals::{SeriesAttachRequest, SeriesProposalService};

pub struct ProposeSeriesAttach;

const TOOL: &str = "propose_series_attach";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    series_id: u64,
    series_version: Option<i32>,
    #[serde(default)]
    titles: Vec<TitleArg>,
    #[serde(default)]
    expected_tomes: Vec<TomeArg>,
    sources: Vec<String>,
}

/// Validate the raw arguments' shape (pure — unit tested).
fn parse_args(args: Value, api_key_id: u64) -> Result<SeriesAttachRequest, ToolError> {
    let a: Args = args::parse_object(
        args,
        "{\"series_id\": <positive integer>, \"titles\": [...], \"expected_tomes\": [...], \"sources\": [<url>]}",
    )?;
    if a.series_id == 0 {
        return Err(ToolError::InvalidArguments(
            "`series_id` must be a positive integer".to_string(),
        ));
    }
    let (titles, expected_tomes) = lines(a.titles, a.expected_tomes);
    Ok(SeriesAttachRequest {
        api_key_id,
        series_id: a.series_id,
        series_version: a.series_version,
        titles,
        expected_tomes,
        sources: a.sources,
    })
}

impl McpTool for ProposeSeriesAttach {
    fn name(&self) -> &'static str {
        TOOL
    }

    fn title(&self) -> &'static str {
        "Propose titles for an existing series"
    }

    fn description(&self) -> &'static str {
        "Propose to attach titles the library already holds to an existing series (each \
         with its position, and `is_omnibus` with `end_position` for an omnibus covering \
         several positions), and/or to record tomes of that series the library does not own \
         yet (`expected_tomes`: position, title, optional ISBN). Give at least one title or \
         expected tome, and `sources`: the http(s) URLs where you found the information. Read \
         the series with get_series first: proposed positions must be free. This files ONE \
         proposal for a librarian to accept, edit or reject; it does not change the catalog. \
         Returns `proposal_id` and `status` (`pending`). Refused with an explanation when: the \
         series id is unknown or deleted; a proposal for this series is still pending (the \
         message gives its id); a title id is unknown or deleted, is already in the series, \
         or is listed twice; a position is below 1, above 10000, above a closed series' total, \
         used twice in the proposal (omnibus ranges included), or already held by a title or \
         named by an expected tome of the series; `end_position` is given without \
         `is_omnibus`, or an omnibus lacks it or covers more than 100 positions; `titles` and \
         `expected_tomes` hold more than 999 entries together; an expected tome has no title, \
         an invalid ISBN or an ISBN the series (or this proposal) already expects; \
         `series_version` or a title's `version` no longer matches; a source is not an \
         http(s) URL. To create a series that does not exist, use propose_series_create."
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
                    "description": "Optional: the series' `version` as returned by get_series. When given and the series' own fields (name, description, type, total) changed since, the proposal is refused so you can re-read it. Without it, the version read while checking this call is recorded, and a series edited during the call is refused the same way (just retry)."
                },
                "titles": titles_property(),
                "expected_tomes": tomes_property(),
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
            let filed = SeriesProposalService::propose_attach(&ctx.state.pool, &req)
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
                "titles": [{"title_id": 4, "position": 1, "is_omnibus": true, "end_position": 2, "version": 5}],
                "sources": ["https://example.org/s"]
            }),
            9,
        )
        .unwrap();
        assert_eq!(
            (req.series_id, req.series_version, req.api_key_id),
            (3, Some(2), 9)
        );
        assert!(req.titles[0].is_omnibus);
        assert_eq!(req.titles[0].end_position, Some(2));
        assert_eq!(req.titles[0].version, Some(5));
        assert!(req.expected_tomes.is_empty());
    }

    #[test]
    fn parse_args_refuses_malformed_arguments() {
        for bad in [
            json!(null),
            json!({"series_id": 0, "sources": ["https://a.b"]}),
            json!({"series_id": "3", "sources": ["https://a.b"]}),
            json!({"series_id": 3}),
            json!({"series_id": 3, "sources": ["https://a.b"], "name": "x"}),
            json!({"series_id": 3, "sources": ["https://a.b"], "expected_tomes": [{"position": 1}]}),
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
