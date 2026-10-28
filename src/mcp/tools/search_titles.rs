//! Story 11-7 — `search_titles`: find titles by text, ISBN or volume
//! label, filtered by media type and genre, one page at a time.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{ListPage, TitleItem, args, resolve_genre, title_list_schema, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::services::search::{CatalogSearch, SearchService};

pub struct SearchTitles;

const EXPECTED: &str =
    "{\"query\"?: string, \"media_type\"?: string, \"genre\"?: string, \"page\"?: integer}";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    query: Option<String>,
    media_type: Option<String>,
    genre: Option<String>,
    page: Option<u32>,
}

/// Arguments after validation (pure — unit tested). The genre name is
/// resolved against the database afterwards.
#[derive(Debug, PartialEq, Eq)]
struct Parsed {
    query: String,
    media_type: Option<String>,
    genre: Option<String>,
    page: u32,
}

fn parse_args(args: Value) -> Result<Parsed, ToolError> {
    let a: Args = args::parse_object(args, EXPECTED)?;
    Ok(Parsed {
        query: args::query(a.query)?,
        media_type: args::media_type(a.media_type)?,
        genre: args::genre(a.genre)?,
        page: args::page(a.page)?,
    })
}

impl McpTool for SearchTitles {
    fn name(&self) -> &'static str {
        "search_titles"
    }

    fn title(&self) -> &'static str {
        "Search titles"
    }

    fn description(&self) -> &'static str {
        "Search the titles of the library catalog. `query` may be free text (matched against \
         title, subtitle, description and contributor names), an ISBN-10 or ISBN-13, hyphens \
         allowed (exact match: only the titles carrying it), or a volume label such as V0042 \
         (exact match: the title that volume belongs to). Omit `query` to browse the whole \
         catalog. Optional filters: `media_type` and `genre` (a genre name as shown by \
         get_title; case and accents are ignored). Results are sorted by title (titles sharing \
         one ISBN: oldest first), 25 per page; pass `page` (1-based) to read further pages \
         until `page` equals `total_pages`. Each item gives the title id (use get_title for \
         the full record), title, subtitle, media type, genre, main contributor, number of \
         volumes, whether a cover exists, publication date and Dewey code. Deleted titles \
         never appear. Read-only."
    }

    fn input_schema(&self) -> Value {
        title_list_schema(json!({
            "query": {
                "type": "string",
                "maxLength": args::MAX_QUERY_CHARS,
                "description": "Free text, an ISBN-10/ISBN-13, or a volume label (V0042). Empty or omitted: all titles."
            }
        }))
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let parsed = parse_args(args)?;
            let pool = &ctx.state.pool;
            let genre_id = resolve_genre(pool, parsed.genre.as_deref()).await?;
            let list = SearchService::search_catalog(
                pool,
                &CatalogSearch {
                    query: parsed.query,
                    media_type: parsed.media_type,
                    genre_id,
                    page: parsed.page,
                    no_cover_only: false,
                },
            )
            .await?;
            to_json(ListPage::<TitleItem>::from(list), "search_titles")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_defaults_and_normalises() {
        assert_eq!(
            parse_args(json!({})),
            Ok(Parsed {
                query: String::new(),
                media_type: None,
                genre: None,
                page: 1
            })
        );
        assert_eq!(
            parse_args(
                json!({"query": " Tintin ", "media_type": "BD", "genre": " Roman ", "page": 3})
            ),
            Ok(Parsed {
                query: "Tintin".into(),
                media_type: Some("bd".into()),
                genre: Some("Roman".into()),
                page: 3
            })
        );
        // Empty filters read as absent.
        assert_eq!(
            parse_args(json!({"media_type": "", "genre": "  "}))
                .unwrap()
                .media_type,
            None
        );
    }

    #[test]
    fn parse_args_rejects_bad_shapes() {
        for bad in [
            json!([1]),
            json!({"page": 0}),
            json!({"page": 10_001}),
            json!({"page": "2"}),
            json!({"page_size": 100}),
            json!({"limit": 100}),
            json!({"media_type": "vinyl"}),
            json!({"query": 42}),
            json!({"query": "x".repeat(201)}),
            json!({"genre": "g".repeat(201)}),
            json!({"genre": 3}),
            json!({"media_type": "m".repeat(201)}),
        ] {
            assert!(
                matches!(parse_args(bad.clone()), Err(ToolError::InvalidArguments(_))),
                "{bad} should be rejected"
            );
        }
    }
}
