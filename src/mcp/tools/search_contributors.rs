//! Story 11-7 — `search_contributors`: find contributors (authors,
//! illustrators, performers…) by name, one page at a time.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ListPage, args, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::contributor::ContributorSearchRow;
use crate::services::contributor::ContributorService;

pub struct SearchContributors;

const EXPECTED: &str = "{\"query\"?: string, \"page\"?: integer}";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    query: Option<String>,
    page: Option<u32>,
}

/// Validated `(query, page)` (pure — unit tested).
fn parse_args(args: Value) -> Result<(String, u32), ToolError> {
    let a: Args = args::parse_object(args, EXPECTED)?;
    Ok((args::query(a.query)?, args::page(a.page)?))
}

#[derive(Debug, Serialize)]
struct Item {
    id: u64,
    name: String,
    title_count: u64,
}

impl From<ContributorSearchRow> for Item {
    fn from(r: ContributorSearchRow) -> Self {
        Item {
            id: r.id,
            name: r.name,
            title_count: r.title_count,
        }
    }
}

impl McpTool for SearchContributors {
    fn name(&self) -> &'static str {
        "search_contributors"
    }

    fn title(&self) -> &'static str {
        "Search contributors"
    }

    fn description(&self) -> &'static str {
        "Search the contributors of the library catalog (authors, illustrators, translators, \
         performers…) whose name contains `query` (case-insensitive); omit `query` to list \
         them all. Results are sorted by name, 25 per page; pass `page` (1-based) to read \
         further pages until `page` equals `total_pages`. Each item gives the contributor id, \
         name and the number of titles they contribute to. To list those titles, call \
         search_titles with the contributor's name as `query`; get_title shows each \
         contributor's role. Deleted contributors never appear. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "maxLength": args::MAX_QUERY_CHARS,
                    "description": "Part of the contributor's name. Empty or omitted: all contributors."
                },
                "page": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": args::MAX_PAGE,
                    "default": 1,
                    "description": "1-based page number; each page holds 25 contributors."
                }
            },
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let (query, page) = parse_args(args)?;
            let list = ContributorService::search(&ctx.state.pool, &query, page).await?;
            to_json(ListPage::<Item>::from(list), "search_contributors")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_defaults_and_bounds() {
        assert_eq!(parse_args(json!({})), Ok((String::new(), 1)));
        assert_eq!(
            parse_args(json!({"query": " Hergé ", "page": 2})),
            Ok(("Hergé".into(), 2))
        );
        for bad in [
            json!([]),
            json!({"page": 0}),
            json!({"limit": 5}),
            json!({"query": ["a"]}),
        ] {
            assert!(
                matches!(parse_args(bad.clone()), Err(ToolError::InvalidArguments(_))),
                "{bad} should be rejected"
            );
        }
    }
}
