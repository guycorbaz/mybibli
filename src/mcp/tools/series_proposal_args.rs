//! Story 11-12 — arguments, schema fragments and error mapping shared by
//! `propose_series_create` and `propose_series_attach` (pure). The
//! kind-independent pieces live in `proposal_args`.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::services::series_proposals::{MAX_LINES, TitleLine, TomeLine};

/// One entry of `titles`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TitleArg {
    title_id: u64,
    position: i64,
    #[serde(default)]
    is_omnibus: bool,
    end_position: Option<i64>,
    version: Option<i32>,
}

impl From<TitleArg> for TitleLine {
    fn from(a: TitleArg) -> Self {
        TitleLine {
            title_id: a.title_id,
            position: a.position,
            is_omnibus: a.is_omnibus,
            end_position: a.end_position,
            version: a.version,
        }
    }
}

/// One entry of `expected_tomes`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TomeArg {
    position: i64,
    title: String,
    isbn: Option<String>,
}

impl From<TomeArg> for TomeLine {
    fn from(a: TomeArg) -> Self {
        TomeLine {
            position: a.position,
            title: a.title,
            isbn: a.isbn,
        }
    }
}

/// Convert the argument lists.
pub fn lines(titles: Vec<TitleArg>, tomes: Vec<TomeArg>) -> (Vec<TitleLine>, Vec<TomeLine>) {
    (
        titles.into_iter().map(Into::into).collect(),
        tomes.into_iter().map(Into::into).collect(),
    )
}

/// Schema of `titles`.
pub fn titles_property() -> Value {
    json!({
        "type": "array",
        "maxItems": MAX_LINES,
        "description": "Titles the library already holds, each with its position in the series (at most 999 entries in `titles` and `expected_tomes` together). Find title ids with search_titles; a tome the library does not own goes in `expected_tomes` instead.",
        "items": {
            "type": "object",
            "properties": {
                "title_id": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Numeric id of the title (from search_titles or get_title)."
                },
                "position": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10000,
                    "description": "Position (tome number) of the title in the series, from 1. For an omnibus, the first position it covers."
                },
                "is_omnibus": {
                    "type": "boolean",
                    "default": false,
                    "description": "True when the title is an omnibus volume covering several positions; then `end_position` is required."
                },
                "end_position": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10000,
                    "description": "Omnibus only: the last position it covers (at least `position`, at most 100 positions in all)."
                },
                "version": {
                    "type": "integer",
                    "description": "Optional: the title's `version` as returned by get_title. When given and the title changed since, the proposal is refused so you can re-read it."
                }
            },
            "required": ["title_id", "position"],
            "additionalProperties": false
        }
    })
}

/// Schema of `expected_tomes`.
pub fn tomes_property() -> Value {
    json!({
        "type": "array",
        "maxItems": MAX_LINES,
        "description": "Tomes of the series the library does not own yet (at most 999 entries in `titles` and `expected_tomes` together) (they show as expected on the series page once accepted).",
        "items": {
            "type": "object",
            "properties": {
                "position": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10000,
                    "description": "Position (tome number) of the tome in the series, from 1."
                },
                "title": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 500,
                    "description": "Title of the tome."
                },
                "isbn": {
                    "type": "string",
                    "maxLength": 40,
                    "description": "Optional ISBN-13 (978/979) or ISBN-10 of the tome; hyphens and spaces are allowed."
                }
            },
            "required": ["position", "title"],
            "additionalProperties": false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The schema bounds follow the service constants (the descriptions
    /// quote them as numbers).
    #[test]
    fn schema_bounds_follow_the_service() {
        use crate::services::series_proposals::{MAX_OMNIBUS_SPAN, MAX_POSITION};
        assert_eq!(
            (MAX_LINES, MAX_POSITION, MAX_OMNIBUS_SPAN),
            (999, 10_000, 100)
        );
        let t = titles_property();
        assert_eq!(t["maxItems"], 999);
        assert_eq!(t["items"]["properties"]["position"]["maximum"], 10_000);
        assert_eq!(
            tomes_property()["items"]["properties"]["position"]["maximum"],
            10_000
        );
    }

    #[test]
    fn line_arguments_refuse_unknown_fields() {
        assert!(serde_json::from_value::<TitleArg>(json!({"title_id": 1, "position": 2})).is_ok());
        assert!(
            serde_json::from_value::<TitleArg>(json!({"title_id": 1, "position": 2, "pos": 3}))
                .is_err()
        );
        assert!(serde_json::from_value::<TomeArg>(json!({"position": 2, "title": "T"})).is_ok());
        assert!(
            serde_json::from_value::<TomeArg>(json!({"position": 2, "title": "T", "x": 1}))
                .is_err()
        );
    }
}
