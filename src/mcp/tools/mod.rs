//! The MCP tools (stories 11-6, 11-7). Each one is a thin adapter over
//! `services/`, implementing [`super::tool::McpTool`]: it validates its
//! arguments, calls one service and maps the result to JSON.

mod args;
mod get_series;
mod get_title;
mod list_series;
mod list_titles_with_incomplete_metadata;
mod list_titles_without_cover;
mod search_contributors;
mod search_titles;

use serde::Serialize;
use serde_json::{Value, json};

use super::tool::{ToolContext, ToolError, ToolRegistry};
use crate::models::title::SearchResult;
use crate::models::{DEFAULT_PAGE_SIZE, PaginatedList};
use crate::services::search::{GenreLookup, SearchService};
use crate::services::title_detail::has_cover;

/// Every tool the endpoint serves.
pub fn registry() -> ToolRegistry {
    ToolRegistry::default()
        .with(get_title::GetTitle)
        .with(search_titles::SearchTitles)
        .with(search_contributors::SearchContributors)
        .with(list_series::ListSeries)
        .with(get_series::GetSeries)
        .with(list_titles_without_cover::ListTitlesWithoutCover)
        .with(list_titles_with_incomplete_metadata::ListTitlesWithIncompleteMetadata)
}

/// One page of a list tool's answer — the same shape for every list.
#[derive(Debug, Serialize)]
struct ListPage<T: Serialize> {
    items: Vec<T>,
    page: u32,
    page_size: u32,
    total_items: u64,
    total_pages: u32,
}

impl<T: Serialize, R: Into<T>> From<PaginatedList<R>> for ListPage<T> {
    fn from(list: PaginatedList<R>) -> Self {
        ListPage {
            page: list.page,
            page_size: DEFAULT_PAGE_SIZE,
            total_items: list.total_items,
            total_pages: list.total_pages,
            items: list.items.into_iter().map(Into::into).collect(),
        }
    }
}

/// A title in a title list (`search_titles`, `list_titles_without_cover`).
#[derive(Debug, Serialize)]
struct TitleItem {
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

impl From<SearchResult> for TitleItem {
    fn from(r: SearchResult) -> Self {
        TitleItem {
            id: r.id,
            title: r.title,
            subtitle: r.subtitle,
            media_type: r.media_type,
            genre_name: Some(r.genre_name).filter(|g| !g.is_empty()),
            primary_contributor: r.primary_contributor,
            volume_count: r.volume_count,
            has_cover: has_cover(r.cover_image_url.as_deref()),
            publication_date: iso_date(r.publication_date),
            dewey_code: r.dewey_code,
        }
    }
}

/// A date as `YYYY-MM-DD`.
fn iso_date(date: Option<chrono::NaiveDate>) -> Option<String> {
    date.map(|d| d.format("%Y-%m-%d").to_string())
}

/// The `page` property of a list tool's schema; `what` names the items.
fn page_property(what: &str) -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": args::MAX_PAGE,
        "default": 1,
        "description": format!("1-based page number; each page holds 25 {what}.")
    })
}

/// Input schema of a title list: `extra` properties plus the shared
/// `media_type`, `genre` and `page` properties.
fn title_list_schema(extra: Value) -> Value {
    let mut properties = extra;
    let map = properties.as_object_mut().expect("extra is an object");
    map.insert(
        "media_type".into(),
        json!({
            "type": "string",
            "enum": args::media_type_names(),
            "description": "Only titles of this media type (bd = comics)."
        }),
    );
    map.insert(
        "genre".into(),
        json!({
            "type": "string",
            "maxLength": args::MAX_QUERY_CHARS,
            "description": "Only titles of this genre, by name (case and accents ignored). An unknown name returns the list of valid genres."
        }),
    );
    map.insert("page".into(), page_property("titles"));
    json!({ "type": "object", "properties": properties, "additionalProperties": false })
}

/// Resolve an optional genre name to its id; an unknown name is an
/// argument error listing the library's genres.
async fn resolve_genre(ctx: &ToolContext, name: Option<&str>) -> Result<Option<u64>, ToolError> {
    let Some(name) = name else {
        return Ok(None);
    };
    match SearchService::resolve_genre(&ctx.state.pool, name).await? {
        GenreLookup::Found(id) => Ok(Some(id)),
        GenreLookup::Unknown(known) if known.is_empty() => Err(ToolError::InvalidArguments(
            "the library has no genres; omit `genre`".to_string(),
        )),
        GenreLookup::Unknown(known) => Err(ToolError::InvalidArguments(format!(
            "unknown `genre` \"{name}\"; the library's genres are: {}",
            known.join(", ")
        ))),
    }
}

/// Serialize a tool's answer; a failure is logged and hidden.
fn to_json(value: impl Serialize, tool: &'static str) -> Result<Value, ToolError> {
    serde_json::to_value(value).map_err(|e| {
        tracing::error!(error = %e, tool, "MCP tool: serialization failed");
        ToolError::Internal
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_serves_exactly_the_read_tools() {
        let reg = registry();
        let names: Vec<_> = reg.iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            [
                "get_series",
                "get_title",
                "list_series",
                "list_titles_with_incomplete_metadata",
                "list_titles_without_cover",
                "search_contributors",
                "search_titles"
            ],
            "tool set changed: update this test and the docs"
        );
    }

    #[test]
    fn registry_tools_are_described_with_object_schemas() {
        let reg = registry();
        for tool in reg.iter() {
            let name = tool.name();
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{name} is not snake_case"
            );
            assert!(!tool.title().is_empty(), "{name} has no title");
            let description = tool.description();
            assert!(description.len() > 200, "{name} description too short");
            assert!(
                description.contains("Read-only"),
                "{name}: say it is read-only"
            );
            let schema = tool.input_schema();
            assert_eq!(schema["type"], "object", "{name} schema is not an object");
            assert_eq!(
                schema["additionalProperties"], false,
                "{name}: unknown arguments must be refused"
            );
            for (prop, def) in schema["properties"].as_object().expect("properties") {
                assert!(
                    def["description"].as_str().is_some_and(|d| !d.is_empty()),
                    "{name}.{prop} has no description"
                );
            }
            assert!(tool.read_only(), "{name}: catalog tools are read-only");
        }
    }

    #[test]
    fn list_page_carries_the_fixed_page_size() {
        let list = PaginatedList::new(vec![1u8, 2], 2, 27, None, None, None);
        let page: ListPage<u8> = list.into();
        let v = serde_json::to_value(page).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "items": [1, 2], "page": 2, "page_size": 25,
                "total_items": 27, "total_pages": 2
            })
        );
    }
}
