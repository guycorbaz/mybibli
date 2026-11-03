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

/// How the owner title is named in the warning: its text, followed by its
/// ISBN when it has one. Before metadata arrives a title's text IS its ISBN
/// (the skeleton title), so the ISBN is not repeated then.
pub fn owner_label(title: &str, isbn: Option<&str>, loc: &str) -> String {
    match isbn.map(str::trim).filter(|i| !i.is_empty()) {
        Some(isbn) if isbn != title.trim() => rust_i18n::t!(
            "feedback.volume_owner_with_isbn",
            locale = loc,
            title = title,
            isbn = isbn
        )
        .to_string(),
        _ => title.to_string(),
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

/// Warning entry: the label is on another title. Names the owner, says what
/// to do, and links to the volume page. `feedback_html_with_link` escapes
/// the message, the hint, the href and the link label (AC6) — nothing is
/// pre-escaped here, which would double-escape.
pub fn other_title_feedback(label: &str, volume_id: u64, owner: &str, loc: &str) -> String {
    let message = rust_i18n::t!(
        "feedback.volume_on_other_title",
        locale = loc,
        label = label,
        owner = owner
    );
    // Story 13.8 (move a volume) will change this hint to "move it from its
    // page"; it is a key of its own so that change is a locale edit only.
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
            let owner = match &owner {
                Some(t) => owner_label(&t.title, t.isbn.as_deref(), loc),
                None => "?".to_string(),
            };
            tracing::info!(
                label = %existing.label,
                volume_id = existing.id,
                owner_title_id = existing.title_id,
                context_title_id,
                "Existing label scanned under another title; nothing written"
            );
            other_title_feedback(&existing.label, existing.id, &owner, loc)
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
    fn owner_label_appends_the_isbn() {
        let s = owner_label("Les Gorilles", Some("9782800100000"), "en");
        assert!(s.contains("Les Gorilles"), "{s}");
        assert!(s.contains("9782800100000"), "{s}");
    }

    #[test]
    fn owner_label_without_isbn_is_the_title() {
        assert_eq!(owner_label("Les Gorilles", None, "en"), "Les Gorilles");
        assert_eq!(
            owner_label("Les Gorilles", Some("  "), "en"),
            "Les Gorilles"
        );
    }

    #[test]
    fn owner_label_does_not_repeat_an_isbn_used_as_title() {
        assert_eq!(
            owner_label("9782800100000", Some("9782800100000"), "en"),
            "9782800100000"
        );
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
        let html = other_title_feedback("V1527", 42, "Les Gorilles (ISBN 978)", "en");
        assert!(html.contains("border-amber-500"), "warning variant: {html}");
        assert!(html.contains("V1527"));
        assert!(html.contains("Les Gorilles"));
        assert!(html.contains(r#"href="/volume/42""#), "{html}");
        assert!(!html.contains("onclick"));
    }

    #[test]
    fn other_title_feedback_escapes_the_owner() {
        let html = other_title_feedback("V0001", 1, "<script>alert(1)</script> & co", "en");
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
}
