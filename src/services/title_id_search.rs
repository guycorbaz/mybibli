//! Story 17-5 (#653) — find a title by its id from the search field.
//!
//! The MCP agent names titles by their database id ("990 is a duplicate of
//! 989"). The librarian types `#990` in the home search field to reach it.
//!
//! The shape is positive only (Foundation Rule #20): `#` followed by 1 to
//! 20 ASCII digits (a `u64`), the first one not `0`. A bare number stays a text
//! search (a year, a number in a title, an ISBN or a UPC); `#` is never
//! part of an ISBN, a UPC, a V-code or an L-code, so the shape cannot
//! collide with them. Kept apart from `services::search::detect_code` on
//! purpose: the id arm runs before code detection.

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::title::TitleModel;

/// Longest id accepted after the `#`: the digits of `u64::MAX` (a value
/// above it fails to parse and is text).
const MAX_ID_DIGITS: usize = 20;

/// The title id typed as `#<id>`, or `None` for any other input. The
/// input is expected trimmed; nothing is rewritten (`#0990`, `# 990`,
/// `#99a` are not ids).
pub fn parse_title_id(query: &str) -> Option<u64> {
    let digits = query.strip_prefix('#')?;
    if digits.is_empty()
        || digits.len() > MAX_ID_DIGITS
        || digits.starts_with('0')
        || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    digits.parse().ok()
}

/// A title id that answers no live title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleIdMiss {
    pub id: u64,
    /// The title is soft-deleted and not yet purged: restorable from the
    /// Trash. `false` = no such title (never existed, or purged).
    pub in_trash: bool,
}

/// What an id names: a live title, or a miss to explain.
#[derive(Debug)]
pub enum TitleIdLookup {
    Live(Box<TitleModel>),
    Miss(TitleIdMiss),
}

/// Look up the title an id names (live first, then the Trash).
pub async fn lookup(pool: &DbPool, id: u64) -> Result<TitleIdLookup, AppError> {
    if let Some(title) = TitleModel::find_by_id(pool, id).await? {
        return Ok(TitleIdLookup::Live(Box::new(title)));
    }
    // The Trash check only refines the message: a failure degrades to
    // "unknown" rather than failing the search.
    let in_trash = TitleModel::is_in_trash(pool, id).await.unwrap_or_else(|e| {
        tracing::warn!(title_id = id, error = %e, "is_in_trash failed; reporting the id as unknown");
        false
    });
    Ok(TitleIdLookup::Miss(TitleIdMiss { id, in_trash }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_digits_is_an_id() {
        assert_eq!(parse_title_id("#990"), Some(990));
        assert_eq!(parse_title_id("#1"), Some(1));
        assert_eq!(parse_title_id("#9999999999"), Some(9_999_999_999));
        assert_eq!(parse_title_id("#12345678901"), Some(12_345_678_901));
        assert_eq!(parse_title_id("#18446744073709551615"), Some(u64::MAX));
    }

    #[test]
    fn other_shapes_are_not_ids() {
        for input in [
            "", "#", "990", "1984", "#0", "#0990", "# 990", "#990 ", " #990", "#99a", "#-1",
            "#+1", "##990", "#９９０", "#18446744073709551616", "#123456789012345678901", "990#", "#V0042",
            "V0042", "L0001", "9782070360246",
        ] {
            assert_eq!(parse_title_id(input), None, "{input:?} must not be an id");
        }
    }

    #[test]
    fn an_id_is_never_a_code_the_search_detects() {
        use crate::services::search::{CodeType, detect_code};
        for input in ["#990", "#1", "#9999999999"] {
            assert_eq!(detect_code(input), CodeType::Text, "{input:?}");
        }
    }
}
