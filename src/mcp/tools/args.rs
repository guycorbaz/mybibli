//! Story 11-7 — argument checks shared by the MCP tools (pure).

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::mcp::tool::ToolError;

/// Highest page a list tool serves. Pages hold `DEFAULT_PAGE_SIZE`
/// items, so this reaches far beyond a household library and keeps
/// `(page - 1) * page_size` inside `u32`.
pub const MAX_PAGE: u32 = 10_000;

/// Longest free-text query accepted, in characters.
pub const MAX_QUERY_CHARS: usize = 200;

/// Deserialize the arguments of a tool whose schema is an object.
/// `null` (no arguments) reads as `{}`. serde would also read a struct
/// from a sequence (`[1]`); only the object form is accepted.
pub fn parse_object<T: DeserializeOwned>(args: Value, expected: &str) -> Result<T, ToolError> {
    let args = match args {
        Value::Null => Value::Object(Default::default()),
        Value::Object(_) => args,
        _ => {
            return Err(ToolError::InvalidArguments(format!(
                "expected an object {expected}"
            )));
        }
    };
    serde_json::from_value(args)
        .map_err(|e| ToolError::InvalidArguments(format!("expected {expected} ({e})")))
}

/// Validate an optional 1-based page number; absent means 1.
pub fn page(page: Option<u32>) -> Result<u32, ToolError> {
    match page.unwrap_or(1) {
        p @ 1..=MAX_PAGE => Ok(p),
        p => Err(ToolError::InvalidArguments(format!(
            "`page` must be between 1 and {MAX_PAGE} (got {p})"
        ))),
    }
}

/// Validate an optional free-text query; absent means empty.
pub fn query(query: Option<String>) -> Result<String, ToolError> {
    let q = query.unwrap_or_default().trim().to_string();
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(ToolError::InvalidArguments(format!(
            "`query` must be at most {MAX_QUERY_CHARS} characters"
        )));
    }
    Ok(q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct A {
        page: Option<u32>,
    }

    #[test]
    fn parse_object_accepts_objects_and_null_only() {
        assert_eq!(
            parse_object::<A>(json!({"page": 2}), "{}"),
            Ok(A { page: Some(2) })
        );
        assert_eq!(parse_object::<A>(json!(null), "{}"), Ok(A { page: None }));
        for bad in [json!([2]), json!("x"), json!(3), json!({"limit": 50})] {
            assert!(
                matches!(
                    parse_object::<A>(bad.clone(), "{}"),
                    Err(ToolError::InvalidArguments(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn page_is_bounded() {
        assert_eq!(page(None), Ok(1));
        assert_eq!(page(Some(1)), Ok(1));
        assert_eq!(page(Some(MAX_PAGE)), Ok(MAX_PAGE));
        assert!(page(Some(0)).is_err());
        assert!(page(Some(MAX_PAGE + 1)).is_err());
        assert!(page(Some(u32::MAX)).is_err());
    }

    #[test]
    fn query_is_trimmed_and_bounded() {
        assert_eq!(query(None), Ok(String::new()));
        assert_eq!(query(Some("  Camus ".into())), Ok("Camus".into()));
        assert!(query(Some("é".repeat(MAX_QUERY_CHARS))).is_ok());
        assert!(query(Some("é".repeat(MAX_QUERY_CHARS + 1))).is_err());
    }
}
