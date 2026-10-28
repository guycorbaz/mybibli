//! Story 11-7 — `search_titles`: find titles by text, ISBN or volume
//! label, filtered by media type and genre, one page at a time.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ListPage, args, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::media_type::MediaType;
use crate::models::title::SearchResult;
use crate::services::search::{CatalogSearch, GenreLookup, SearchService};
use crate::services::title_detail::has_cover;

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

/// The accepted `media_type` values, from [`MediaType::ALL`].
fn media_type_names() -> Vec<String> {
    MediaType::ALL.iter().map(ToString::to_string).collect()
}

fn parse_args(args: Value) -> Result<Parsed, ToolError> {
    let a: Args = args::parse_object(args, EXPECTED)?;
    let media_type = args::text("media_type", a.media_type)?;
    let media_type = if media_type.is_empty() {
        None
    } else {
        Some(
            MediaType::from_str(&media_type)
                .map_err(|_| {
                    ToolError::InvalidArguments(format!(
                        "unknown `media_type` \"{media_type}\"; use one of: {}",
                        media_type_names().join(", ")
                    ))
                })?
                .to_string(),
        )
    };
    let genre = Some(args::text("genre", a.genre)?).filter(|g| !g.is_empty());
    Ok(Parsed {
        query: args::query(a.query)?,
        media_type,
        genre,
        page: args::page(a.page)?,
    })
}

#[derive(Debug, Serialize)]
struct Item {
    id: u64,
    title: String,
    subtitle: Option<String>,
    media_type: String,
    genre_name: Option<String>,
    primary_contributor: Option<String>,
    volume_count: u64,
    has_cover: bool,
    publication_date: Option<String>,
    dewey_code: Option<String>,
}

impl From<SearchResult> for Item {
    fn from(r: SearchResult) -> Self {
        Item {
            id: r.id,
            title: r.title,
            subtitle: r.subtitle,
            media_type: r.media_type,
            genre_name: Some(r.genre_name).filter(|g| !g.is_empty()),
            primary_contributor: r.primary_contributor,
            volume_count: r.volume_count,
            has_cover: has_cover(r.cover_image_url.as_deref()),
            publication_date: r.publication_date.map(|d| d.format("%Y-%m-%d").to_string()),
            dewey_code: r.dewey_code,
        }
    }
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
         (exact match: the title that volume belongs to). Omit `query` to browse the whole catalog. Optional filters: `media_type` and `genre` \
         (a genre name as shown by get_title; case and accents are ignored). Results are sorted by title, \
         25 per page; pass `page` (1-based) to read further pages until `page` equals \
         `total_pages`. Each item gives the title id (use get_title for the full record), \
         title, subtitle, media type, genre, main contributor, number of volumes, whether a \
         cover exists, publication date and Dewey code. Deleted titles never appear. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "maxLength": args::MAX_QUERY_CHARS,
                    "description": "Free text, an ISBN-10/ISBN-13, or a volume label (V0042). Empty or omitted: all titles."
                },
                "media_type": {
                    "type": "string",
                    "enum": media_type_names(),
                    "description": "Only titles of this media type (bd = comics)."
                },
                "genre": {
                    "type": "string",
                    "maxLength": args::MAX_QUERY_CHARS,
                    "description": "Only titles of this genre, by name (case-insensitive). An unknown name returns the list of valid genres."
                },
                "page": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": args::MAX_PAGE,
                    "default": 1,
                    "description": "1-based page number; each page holds 25 titles."
                }
            },
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let parsed = parse_args(args)?;
            let pool = &ctx.state.pool;
            let genre_id = match parsed.genre.as_deref() {
                None => None,
                Some(name) => match SearchService::resolve_genre(pool, name).await? {
                    GenreLookup::Found(id) => Some(id),
                    GenreLookup::Unknown(known) if known.is_empty() => {
                        return Err(ToolError::InvalidArguments(
                            "the library has no genres; omit `genre`".to_string(),
                        ));
                    }
                    GenreLookup::Unknown(known) => {
                        return Err(ToolError::InvalidArguments(format!(
                            "unknown `genre` \"{name}\"; the library's genres are: {}",
                            known.join(", ")
                        )));
                    }
                },
            };
            let list = SearchService::search_catalog(
                pool,
                &CatalogSearch {
                    query: parsed.query,
                    media_type: parsed.media_type,
                    genre_id,
                    page: parsed.page,
                },
            )
            .await?;
            to_json(ListPage::<Item>::from(list), "search_titles")
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
    fn media_type_names_follow_the_enum() {
        assert_eq!(
            media_type_names(),
            ["book", "bd", "cd", "dvd", "magazine", "report"]
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
