//! Story 11-8 — `list_titles_without_cover`: the titles that have no
//! cover image, one page at a time.

use serde::Deserialize;
use serde_json::Value;

use super::{ListPage, TitleItem, args, resolve_genre, title_list_schema, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::services::search::{CatalogSearch, SearchService};

pub struct ListTitlesWithoutCover;

const EXPECTED: &str = "{\"media_type\"?: string, \"genre\"?: string, \"page\"?: integer}";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    media_type: Option<String>,
    genre: Option<String>,
    page: Option<u32>,
}

/// Validated `(media_type, genre, page)` (pure — unit tested).
type Parsed = (Option<String>, Option<String>, u32);

fn parse_args(args: Value) -> Result<Parsed, ToolError> {
    let a: Args = args::parse_object(args, EXPECTED)?;
    Ok((
        args::media_type(a.media_type)?,
        args::genre(a.genre)?,
        args::page(a.page)?,
    ))
}

impl McpTool for ListTitlesWithoutCover {
    fn name(&self) -> &'static str {
        "list_titles_without_cover"
    }

    fn title(&self) -> &'static str {
        "List titles without a cover"
    }

    fn description(&self) -> &'static str {
        "List the titles of the library catalog that have no cover image, optionally only \
         those of one `media_type` or `genre` (a genre name as shown by get_title; case and \
         accents are ignored). Sorted by title, 25 per page; pass `page` (1-based) until \
         `page` equals `total_pages`. Each item has the same shape as in search_titles: \
         title id (use get_title for the ISBN and the full record), title, subtitle, media \
         type, genre, main contributor, number of volumes, `has_cover` (always false here), \
         publication date and Dewey code. Deleted titles never appear. mybibli's cover \
         sources (named in the server instructions, with when they are asked) may already \
         have been asked for these titles without a cover: look elsewhere first — those sources may hold one now. Read-only."
    }

    fn input_schema(&self) -> Value {
        title_list_schema(serde_json::json!({}))
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let (media_type, genre, page) = parse_args(args)?;
            let pool = &ctx.state.pool;
            let genre_id = resolve_genre(ctx, genre.as_deref()).await?;
            let list = SearchService::search_catalog(
                pool,
                &CatalogSearch {
                    query: String::new(),
                    media_type,
                    genre_id,
                    page,
                    no_cover_only: true,
                },
            )
            .await?;
            to_json(
                ListPage::<TitleItem>::from(list),
                "list_titles_without_cover",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_args_defaults_and_bounds() {
        assert_eq!(parse_args(json!({})), Ok((None, None, 1)));
        assert_eq!(
            parse_args(json!({"media_type": "BD", "genre": " Roman ", "page": 4})),
            Ok((Some("bd".into()), Some("Roman".into()), 4))
        );
        for bad in [
            json!({"page": 0}),
            json!({"query": "x"}),
            json!({"media_type": "vinyl"}),
            json!({"genre": 3}),
        ] {
            assert!(
                matches!(parse_args(bad.clone()), Err(ToolError::InvalidArguments(_))),
                "{bad} should be rejected"
            );
        }
    }
}
