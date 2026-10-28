//! Story 11-6 — the MCP tools. Each one is a thin adapter over
//! `services/`, implementing [`super::tool::McpTool`].

mod get_title;

use super::tool::ToolRegistry;

/// Every tool the endpoint serves.
pub fn registry() -> ToolRegistry {
    ToolRegistry::default().with(get_title::GetTitle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_tools_are_described_with_object_schemas() {
        let reg = registry();
        assert!(reg.get("get_title").is_some());
        for tool in reg.iter() {
            let name = tool.name();
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{name} is not snake_case"
            );
            assert!(!tool.title().is_empty(), "{name} has no title");
            assert!(
                tool.description().len() > 40,
                "{name} description too short"
            );
            let schema = tool.input_schema();
            assert_eq!(schema["type"], "object", "{name} schema is not an object");
            assert!(
                tool.read_only(),
                "{name}: story 11-6 serves read-only tools only"
            );
        }
    }
}
