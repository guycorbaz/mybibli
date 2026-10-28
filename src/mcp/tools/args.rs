//! Story 11-7 — argument checks shared by the MCP tools (pure).

use std::str::FromStr;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::mcp::tool::ToolError;
use crate::models::media_type::MediaType;

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

/// Validate an optional string argument named `name`: at most
/// [`MAX_QUERY_CHARS`] characters as sent (the schema's `maxLength`),
/// then trimmed; absent means empty. The length error does not echo
/// the value; callers may quote the (bounded) value in their own errors.
pub fn text(name: &str, value: Option<String>) -> Result<String, ToolError> {
    let value = value.unwrap_or_default();
    if value.chars().count() > MAX_QUERY_CHARS {
        return Err(ToolError::InvalidArguments(format!(
            "`{name}` must be at most {MAX_QUERY_CHARS} characters"
        )));
    }
    Ok(value.trim().to_string())
}

/// [`text`] for the `query` argument.
pub fn query(query: Option<String>) -> Result<String, ToolError> {
    text("query", query)
}

/// The accepted `media_type` values, from [`MediaType::ALL`].
pub fn media_type_names() -> Vec<String> {
    MediaType::ALL.iter().map(ToString::to_string).collect()
}

/// Validate an optional `media_type` filter (case-insensitive); empty
/// or absent means no filter. Returns the canonical name.
pub fn media_type(value: Option<String>) -> Result<Option<String>, ToolError> {
    let value = text("media_type", value)?;
    if value.is_empty() {
        return Ok(None);
    }
    MediaType::from_str(&value)
        .map(|m| Some(m.to_string()))
        .map_err(|_| {
            ToolError::InvalidArguments(format!(
                "unknown `media_type` \"{value}\"; use one of: {}",
                media_type_names().join(", ")
            ))
        })
}

/// Validate an optional `genre` name filter; empty or absent means no
/// filter. The name is resolved against the database by
/// [`super::resolve_genre`].
pub fn genre(value: Option<String>) -> Result<Option<String>, ToolError> {
    Ok(Some(text("genre", value)?).filter(|g| !g.is_empty()))
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
        // The bound applies to the value as sent, like `maxLength`.
        assert!(query(Some(format!(" {} ", "a".repeat(MAX_QUERY_CHARS)))).is_err());
        assert_eq!(text("genre", Some(" BD ".into())), Ok("BD".into()));
        let long = text("genre", Some("g".repeat(MAX_QUERY_CHARS + 1))).unwrap_err();
        assert!(long.message().contains("`genre`") && !long.message().contains("ggg"));
    }

    #[test]
    fn media_type_names_follow_the_enum() {
        assert_eq!(
            media_type_names(),
            ["book", "bd", "cd", "dvd", "magazine", "report"]
        );
    }

    #[test]
    fn media_type_and_genre_filters() {
        assert_eq!(media_type(None), Ok(None));
        assert_eq!(media_type(Some(" ".into())), Ok(None));
        assert_eq!(media_type(Some("BD".into())), Ok(Some("bd".into())));
        let err = media_type(Some("vinyl".into())).unwrap_err();
        assert!(
            err.message()
                .contains("book, bd, cd, dvd, magazine, report")
        );
        assert!(media_type(Some("m".repeat(MAX_QUERY_CHARS + 1))).is_err());
        assert_eq!(genre(Some("  ".into())), Ok(None));
        assert_eq!(genre(Some(" Roman ".into())), Ok(Some("Roman".into())));
        assert!(genre(Some("g".repeat(MAX_QUERY_CHARS + 1))).is_err());
    }
}
