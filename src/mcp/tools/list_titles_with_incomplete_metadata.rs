//! Story 11-8 — `list_titles_with_incomplete_metadata`: the titles
//! missing a description, a publisher, a contributor or a publication
//! year, one page at a time.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ListPage, args, iso_date, resolve_genre, title_list_schema, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::title_search::{IncompleteTitleRow, MetadataField};
use crate::services::search::SearchService;
use crate::services::title_detail::has_cover;

pub struct ListTitlesWithIncompleteMetadata;

const EXPECTED: &str =
    "{\"missing\"?: string, \"media_type\"?: string, \"genre\"?: string, \"page\"?: integer}";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    missing: Option<String>,
    media_type: Option<String>,
    genre: Option<String>,
    page: Option<u32>,
}

#[derive(Debug, PartialEq, Eq)]
struct Parsed {
    missing: Option<MetadataField>,
    media_type: Option<String>,
    genre: Option<String>,
    page: u32,
}

fn field_names() -> Vec<&'static str> {
    MetadataField::ALL.iter().map(|f| f.as_str()).collect()
}

/// Validate the raw arguments (pure — unit tested).
fn parse_args(args: Value) -> Result<Parsed, ToolError> {
    let a: Args = args::parse_object(args, EXPECTED)?;
    let missing = args::text("missing", a.missing)?;
    let missing = if missing.is_empty() {
        None
    } else {
        Some(MetadataField::parse(&missing).ok_or_else(|| {
            ToolError::InvalidArguments(format!(
                "unknown `missing` field \"{missing}\"; use one of: {}",
                field_names().join(", ")
            ))
        })?)
    };
    Ok(Parsed {
        missing,
        media_type: args::media_type(a.media_type)?,
        genre: args::genre(a.genre)?,
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
    isbn: Option<String>,
    primary_contributor: Option<String>,
    publication_date: Option<String>,
    has_cover: bool,
    missing: Vec<&'static str>,
}

impl From<IncompleteTitleRow> for Item {
    fn from(r: IncompleteTitleRow) -> Self {
        Item {
            id: r.id,
            title: r.title,
            subtitle: r.subtitle,
            media_type: r.media_type,
            genre_name: Some(r.genre_name).filter(|g| !g.is_empty()),
            isbn: r.isbn,
            primary_contributor: r.primary_contributor,
            publication_date: iso_date(r.publication_date),
            has_cover: has_cover(r.cover_image_url.as_deref()),
            missing: r.missing.into_iter().map(MetadataField::as_str).collect(),
        }
    }
}

impl McpTool for ListTitlesWithIncompleteMetadata {
    fn name(&self) -> &'static str {
        "list_titles_with_incomplete_metadata"
    }

    fn title(&self) -> &'static str {
        "List titles with incomplete metadata"
    }

    fn description(&self) -> &'static str {
        "List the titles of the library catalog whose metadata is incomplete: no description, \
         no publisher (empty text counts as missing), no contributor (author, illustrator…) \
         or no publication year. Pass `missing` to keep only the titles lacking that one \
         field; optional `media_type` and `genre` filters (a genre name as shown by \
         get_title; case and accents are ignored). Sorted by title, 25 per page; pass `page` \
         (1-based) until `page` equals `total_pages`. Each item gives the title id (use \
         get_title for the full record), title, subtitle, media type, genre, ISBN, main \
         contributor, publication date, whether a cover exists, and `missing`: every field \
         the title lacks. Deleted titles and contributors never count. Read-only."
    }

    fn input_schema(&self) -> Value {
        title_list_schema(json!({
            "missing": {
                "type": "string",
                "enum": field_names(),
                "description": "Only titles lacking this field. Omitted: titles lacking any of them."
            }
        }))
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let parsed = parse_args(args)?;
            let pool = &ctx.state.pool;
            let genre_id = resolve_genre(ctx, parsed.genre.as_deref()).await?;
            let list = SearchService::list_incomplete_metadata(
                pool,
                parsed.missing,
                parsed.media_type.as_deref(),
                genre_id,
                parsed.page,
            )
            .await?;
            to_json(
                ListPage::<Item>::from(list),
                "list_titles_with_incomplete_metadata",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_reads_the_missing_field() {
        assert_eq!(
            parse_args(json!({})),
            Ok(Parsed {
                missing: None,
                media_type: None,
                genre: None,
                page: 1
            })
        );
        for f in MetadataField::ALL {
            assert_eq!(
                parse_args(json!({ "missing": f.as_str() }))
                    .unwrap()
                    .missing,
                Some(f)
            );
        }
        // Case-insensitive, like `media_type` (story 11-8 review).
        assert_eq!(
            parse_args(json!({"missing": "Publication_Year"}))
                .unwrap()
                .missing,
            Some(MetadataField::PublicationYear)
        );
        let err = parse_args(json!({"missing": "isbn"})).unwrap_err();
        assert!(
            err.message()
                .contains("description, publisher, contributor, publication_year")
        );
        for bad in [
            json!({"missing": 1}),
            json!({"page": 0}),
            json!({"media_type": "vinyl"}),
            json!({"query": "x"}),
        ] {
            assert!(
                matches!(parse_args(bad.clone()), Err(ToolError::InvalidArguments(_))),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn item_lists_missing_fields_by_name() {
        let row = IncompleteTitleRow {
            id: 3,
            title: "T".into(),
            subtitle: None,
            media_type: "book".into(),
            genre_name: String::new(),
            isbn: None,
            primary_contributor: None,
            cover_image_url: Some(String::new()),
            publication_date: None,
            missing: vec![MetadataField::Contributor, MetadataField::PublicationYear],
        };
        let v = serde_json::to_value(Item::from(row)).unwrap();
        assert_eq!(v["missing"], json!(["contributor", "publication_year"]));
        assert_eq!(v["has_cover"], false);
        assert!(v["genre_name"].is_null());
    }
}
