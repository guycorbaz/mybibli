//! The MCP tools (stories 11-6, 11-7). Each one is a thin adapter over
//! `services/`, implementing [`super::tool::McpTool`]: it validates its
//! arguments, calls one service and maps the result to JSON.

mod args;
mod get_title;
mod search_contributors;
mod search_titles;

use serde::Serialize;
use serde_json::Value;

use super::tool::{ToolError, ToolRegistry};
use crate::models::{DEFAULT_PAGE_SIZE, PaginatedList};

/// Every tool the endpoint serves.
pub fn registry() -> ToolRegistry {
    ToolRegistry::default()
        .with(get_title::GetTitle)
        .with(search_titles::SearchTitles)
        .with(search_contributors::SearchContributors)
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
    fn registry_serves_exactly_the_catalog_read_tools() {
        let reg = registry();
        let names: Vec<_> = reg.iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            ["get_title", "search_contributors", "search_titles"],
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
