//! Story 20-3 (D8, #524) — what a title restore from the Trash says about
//! the expected tomes of its series: one `success` entry per tome settled
//! with the restored title (Q4 (a)), one `warning` entry with a link to the
//! series per tome left at one of its positions (Q2 (b)). Kept out of
//! `admin.rs` (Rule #12).

use rust_i18n::t;

use crate::db::DbPool;
use crate::services::expected_tomes::{ExpectedTomeService, RestoreTomeNotes};
use crate::utils::{feedback_html, feedback_html_with_link};

/// The entries to append after "Restored: …" once title `title_id` (named
/// `title`) is back: settles the tomes it matches first (best effort, see
/// [`ExpectedTomeService::after_title_restore`]). Empty when the restore
/// touches no expected tome.
pub async fn restore_tome_feedback(pool: &DbPool, title_id: u64, title: &str, loc: &str) -> String {
    let notes = ExpectedTomeService::after_title_restore(pool, title_id).await;
    restore_tome_lines(&notes, title, loc)
}

/// The entries for `notes` (pure). Names are escaped by the feedback
/// helpers; the link is built from the integer series id.
pub fn restore_tome_lines(notes: &RestoreTomeNotes, title: &str, loc: &str) -> String {
    let mut html = String::new();
    for o in &notes.settled {
        let message = t!(
            "admin.trash.restore_tome_settled",
            locale = loc,
            position = o.position,
            series = &o.series_name
        );
        html.push_str(&feedback_html("success", &message, ""));
    }
    if notes.to_review.is_empty() {
        return html;
    }
    let open_series = t!("proposals.open_series", locale = loc);
    for tome in &notes.to_review {
        let message = t!(
            "admin.trash.restore_tome_to_review",
            locale = loc,
            title = title,
            position = tome.position,
            series = &tome.series_name
        );
        html.push_str(&feedback_html_with_link(
            "warning",
            &message,
            "",
            &format!("/series/{}", tome.series_id),
            &open_series,
        ));
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::expected_tomes::{SettleOutcome, SettleResult, TomeOnRestoredPosition};

    fn notes() -> RestoreTomeNotes {
        RestoreTomeNotes {
            settled: vec![SettleOutcome {
                tome_id: 1,
                series_id: 7,
                series_name: "<b>Astérix</b>".into(),
                position: 3,
                result: SettleResult::Settled,
            }],
            to_review: vec![TomeOnRestoredPosition {
                series_id: 42,
                series_name: "Tintin & Co".into(),
                position: 5,
            }],
        }
    }

    #[test]
    fn nothing_to_say_without_tomes() {
        assert_eq!(restore_tome_lines(&RestoreTomeNotes::default(), "T", "en"), "");
    }

    #[test]
    fn settled_line_is_a_success_entry_with_escaped_names() {
        let mut n = notes();
        n.to_review.clear();
        let html = restore_tome_lines(&n, "T", "en");
        assert!(html.contains("&lt;b&gt;Astérix&lt;/b&gt;"), "{html}");
        assert!(!html.contains("<b>Astérix"), "{html}");
        assert!(html.contains("position 3"), "{html}");
        assert!(html.contains("feedback-entry"), "{html}");
        assert!(!html.contains("href="), "{html}");
    }

    #[test]
    fn to_review_line_is_a_warning_with_the_series_link() {
        let mut n = notes();
        n.settled.clear();
        let html = restore_tome_lines(&n, "<i>Le Lotus</i>", "en");
        assert!(html.contains(r#"href="/series/42""#), "{html}");
        assert!(html.contains("Open the series"), "{html}");
        assert!(html.contains("&lt;i&gt;Le Lotus&lt;/i&gt;"), "{html}");
        assert!(html.contains("Tintin &amp; Co"), "{html}");
        assert!(!html.contains("<i>Le Lotus"), "{html}");
        assert!(html.contains("dismiss-feedback"), "a warning stays until dismissed: {html}");
    }

    #[test]
    fn lines_are_worded_in_every_locale() {
        for loc in ["en", "fr", "de", "it"] {
            let html = restore_tome_lines(&notes(), "T", loc);
            assert!(!html.contains("admin.trash."), "{loc}: {html}");
            assert!(!html.contains("proposals.open_series"), "{loc}: {html}");
            assert!(html.contains('5') && html.contains('3'), "{loc}: {html}");
        }
    }
}
