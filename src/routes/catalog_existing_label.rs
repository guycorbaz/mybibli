//! A V-code scanned on `/catalog` that already labels a live volume, with a
//! title in context and no active location (#585).
//!
//! Before this module, such a scan always tried to CREATE a volume and was
//! refused with "Label V… is already assigned to …" — even when the label
//! belonged to the very title in context (the librarian had simply scanned
//! the ISBN + label pair again). Now:
//!
//! - same title → an **info** entry, nothing written;
//! - another title → a **warning** naming the owner (and its ISBN), with a
//!   link to the volume page, nothing written.
//!
//! The active-location shelve path, the new-label path (CR #300 modal +
//! `create_volume`) and the soft-deleted-label path (#442 reuse rules in
//! `VolumeService::resolve_duplicate_label`) are untouched.
//!
//! Kept out of `routes/catalog.rs` (Rule #12): the V-code arm makes one
//! call and returns the response.

use axum::response::{Html, IntoResponse, Response};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;
use crate::utils::{feedback_html, feedback_html_with_link};

/// Whose volume the scanned label is, relative to the title in context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingLabelCase {
    /// The label belongs to a volume of the title in context.
    SameTitle,
    /// The label belongs to a volume of another title.
    OtherTitle,
}

/// Pure decision (unit tested).
pub fn decide(volume_title_id: u64, context_title_id: u64) -> ExistingLabelCase {
    if volume_title_id == context_title_id {
        ExistingLabelCase::SameTitle
    } else {
        ExistingLabelCase::OtherTitle
    }
}

/// The ISBN clause after the quoted owner title: " (ISBN …)" when it has
/// one, empty otherwise. Before metadata arrives a title's text IS its ISBN
/// (the skeleton title), so the ISBN is not repeated then. The quotes go
/// around the title only (review 13-1 round 2).
pub fn isbn_suffix(title: &str, isbn: Option<&str>, loc: &str) -> String {
    match isbn.map(str::trim).filter(|i| !i.is_empty()) {
        Some(isbn) if isbn != title.trim() => rust_i18n::t!(
            "feedback.volume_owner_isbn_suffix",
            locale = loc,
            isbn = isbn
        )
        .to_string(),
        _ => String::new(),
    }
}

/// Info entry: the label is already recorded for the title in context.
/// `feedback_html` escapes the message.
pub fn same_title_feedback(label: &str, loc: &str) -> String {
    feedback_html(
        "info",
        &rust_i18n::t!(
            "feedback.volume_already_recorded",
            locale = loc,
            label = label
        ),
        "",
    )
}

/// Warning entry: the label is on another title. Names the owner — its
/// title and the [`isbn_suffix`] — (`None`
/// when the owner title cannot be read — the message then says "another
/// title"), says what to do, and links to the volume page.
/// `feedback_html_with_link` escapes the message, the hint, the href and the
/// link label (AC6) — nothing is pre-escaped here, which would double-escape.
pub fn other_title_feedback(
    label: &str,
    volume_id: u64,
    owner: Option<(&str, &str)>,
    loc: &str,
) -> String {
    let message = match owner {
        Some((title, isbn)) => rust_i18n::t!(
            "feedback.volume_on_other_title",
            locale = loc,
            label = label,
            title = title,
            isbn = isbn
        ),
        None => rust_i18n::t!(
            "feedback.volume_on_unknown_title",
            locale = loc,
            label = label
        ),
    };
    // Story 13.8 (#580): the hint says the volume can be moved to the right
    // title from its page (the "Move to another title" modal).
    let hint = rust_i18n::t!("feedback.volume_on_other_title_hint", locale = loc);
    let link_label = rust_i18n::t!("feedback.volume_open_link", locale = loc, label = label);
    feedback_html_with_link(
        "warning",
        &message,
        &hint,
        &format!("/volume/{volume_id}"),
        &link_label,
    )
}

/// Error entry: shelving an existing label at the active location failed
/// (the UPDATE errored). Never falls through to the "already recorded" info,
/// which would claim success for a volume that did not move.
pub fn shelve_failed_feedback(label: &str, loc: &str) -> String {
    feedback_html(
        "error",
        &rust_i18n::t!("feedback.volume_shelve_failed", locale = loc, label = label),
        "",
    )
}

/// Warning entry: the active location vanished between being chosen and
/// this scan; the volume was not shelved and the context was cleared.
pub fn active_location_gone_feedback(label: &str, loc: &str) -> String {
    feedback_html(
        "warning",
        &rust_i18n::t!("feedback.active_location_gone", locale = loc, label = label),
        "",
    )
}

/// Answer the scan of `existing` (a live volume) with `context_title_id` in
/// context. Writes nothing.
pub async fn existing_label_response(
    pool: &DbPool,
    existing: &VolumeModel,
    context_title_id: u64,
    loc: &str,
) -> Result<Response, AppError> {
    let html = match decide(existing.title_id, context_title_id) {
        ExistingLabelCase::SameTitle => {
            tracing::info!(
                label = %existing.label,
                title_id = context_title_id,
                "Existing label re-scanned under its own title; nothing to do"
            );
            same_title_feedback(&existing.label, loc)
        }
        ExistingLabelCase::OtherTitle => {
            let owner = TitleModel::find_by_id(pool, existing.title_id).await?;
            let owner = owner.as_ref().map(|t| {
                (
                    t.title.as_str(),
                    isbn_suffix(&t.title, t.isbn.as_deref(), loc),
                )
            });
            tracing::info!(
                label = %existing.label,
                volume_id = existing.id,
                owner_title_id = existing.title_id,
                context_title_id,
                "Existing label scanned under another title; nothing written"
            );
            other_title_feedback(
                &existing.label,
                existing.id,
                owner.as_ref().map(|(t, i)| (*t, i.as_str())),
                loc,
            )
        }
    };
    Ok(Html(html).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_same_title() {
        assert_eq!(decide(7, 7), ExistingLabelCase::SameTitle);
    }

    #[test]
    fn decide_other_title() {
        assert_eq!(decide(403, 404), ExistingLabelCase::OtherTitle);
    }

    #[test]
    fn isbn_suffix_names_the_isbn() {
        let s = isbn_suffix("Les Gorilles", Some("9782800100000"), "en");
        assert_eq!(s, " (ISBN 9782800100000)");
    }

    #[test]
    fn isbn_suffix_is_empty_without_isbn() {
        assert_eq!(isbn_suffix("Les Gorilles", None, "en"), "");
        assert_eq!(isbn_suffix("Les Gorilles", Some("  "), "en"), "");
    }

    #[test]
    fn isbn_suffix_does_not_repeat_an_isbn_used_as_title() {
        assert_eq!(
            isbn_suffix("9782800100000", Some("9782800100000"), "en"),
            ""
        );
    }

    #[test]
    fn quotes_enclose_the_title_only_with_the_isbn_outside() {
        for loc in ["en", "fr", "de", "it"] {
            let html = other_title_feedback(
                "V1527",
                42,
                Some(("Les Gorilles", " (ISBN 9782800100000)")),
                loc,
            );
            let title_at = html.find("Les Gorilles").unwrap();
            let isbn_at = html.find("(ISBN 9782800100000)").unwrap();
            let closing = ["”", "»", "“"]
                .iter()
                .filter_map(|q| html[title_at..].find(q).map(|i| title_at + i))
                .min()
                .unwrap_or_else(|| panic!("{loc}: no closing quote in {html}"));
            assert!(closing < isbn_at, "{loc}: ISBN inside the quotes: {html}");
        }
    }

    #[test]
    fn same_title_feedback_is_info_and_names_the_label() {
        let html = same_title_feedback("V1403", "en");
        assert!(html.contains("border-blue-500"), "info variant: {html}");
        assert!(html.contains("V1403"));
        assert!(html.contains("already recorded"));
    }

    #[test]
    fn other_title_feedback_is_warning_with_volume_link() {
        let html = other_title_feedback("V1527", 42, Some(("Les Gorilles", " (ISBN 978)")), "en");
        assert!(html.contains("border-amber-500"), "warning variant: {html}");
        assert!(html.contains("V1527"));
        assert!(html.contains("Les Gorilles"));
        assert!(html.contains(r#"href="/volume/42""#), "{html}");
        assert!(!html.contains("onclick"));
    }

    #[test]
    fn other_title_feedback_escapes_the_owner() {
        let html = other_title_feedback(
            "V0001",
            1,
            Some(("<script>alert(1)</script> & co", "")),
            "en",
        );
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&amp; co"));
    }

    #[test]
    fn messages_are_localized() {
        let html = same_title_feedback("V0001", "fr");
        assert!(html.contains("V0001"));
        assert!(
            !html.contains("already recorded"),
            "fr copy expected: {html}"
        );
    }

    #[test]
    fn other_title_feedback_without_owner_says_another_title() {
        let html = other_title_feedback("V0002", 3, None, "en");
        assert!(html.contains("another title"), "{html}");
        assert!(!html.contains("“?”") && !html.contains("\"?\""), "{html}");
        assert!(html.contains(r#"href="/volume/3""#));
    }

    #[test]
    fn shelve_failed_is_an_error_and_never_the_recorded_info() {
        let html = shelve_failed_feedback("V0003", "en");
        assert!(html.contains("border-red-500"), "{html}");
        assert!(html.contains("V0003"));
        assert!(!html.contains("already recorded"));
    }

    #[test]
    fn active_location_gone_is_a_warning() {
        let html = active_location_gone_feedback("V0004", "en");
        assert!(html.contains("border-amber-500"), "{html}");
        assert!(html.contains("no longer exists"));
    }
}
