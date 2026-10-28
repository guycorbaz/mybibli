//! Banners appended to the feedback of a catalogue event carrying an
//! ISBN (scan or manual creation): the wishlist auto-link (CR #242) and
//! the expected-tome auto-settle outcomes (story 11-10, FR145).
//!
//! Kept out of `routes/catalog.rs` (Rule #12): the catalogue handlers
//! make one call and append the returned HTML.

use crate::db::DbPool;
use crate::services::expected_tomes::{ExpectedTomeService, SettleOutcome, SettleResult};
use crate::utils::feedback_html;

/// Wishlist auto-link banner + one feedback entry per expected tome the
/// ISBN matched. Empty when the title has no ISBN or nothing matches.
/// Never fails: a settle error is logged and shows nothing (the scan or
/// creation itself already succeeded).
pub async fn isbn_banners(pool: &DbPool, title_id: u64, isbn: Option<&str>, loc: &str) -> String {
    let Some(isbn) = isbn else {
        return String::new();
    };
    let mut html = crate::routes::wishlist::autolink_feedback_html(pool, isbn, loc).await;
    match ExpectedTomeService::settle_for_title(pool, title_id, isbn).await {
        Ok(outcomes) => html.push_str(&settle_feedback_html(&outcomes, loc)),
        Err(e) => {
            tracing::error!(error = %e, title_id, isbn = %isbn, "Expected tome auto-settle failed");
        }
    }
    html
}

/// One feedback entry per outcome (pure — unit tested). Messages go
/// through `feedback_html`, which escapes them.
pub fn settle_feedback_html(outcomes: &[SettleOutcome], loc: &str) -> String {
    let mut html = String::new();
    for o in outcomes {
        let series = o.series_name.as_str();
        let position = o.position.to_string();
        let entry = match &o.result {
            SettleResult::Settled => feedback_html(
                "success",
                &rust_i18n::t!(
                    "expected_tome.settled",
                    locale = loc,
                    position = &position,
                    series = series
                ),
                &rust_i18n::t!("expected_tome.settled_suggestion", locale = loc),
            ),
            conflict => {
                let why = match conflict {
                    SettleResult::PositionHeld { holder } => rust_i18n::t!(
                        "expected_tome.conflict_held",
                        locale = loc,
                        holder = holder.as_str()
                    ),
                    SettleResult::TitleElsewhere { position: other } => rust_i18n::t!(
                        "expected_tome.conflict_elsewhere",
                        locale = loc,
                        other = other.to_string(),
                        position = &position
                    ),
                    SettleResult::OutOfRange { total } => rust_i18n::t!(
                        "expected_tome.conflict_out_of_range",
                        locale = loc,
                        total = total.to_string()
                    ),
                    SettleResult::Settled => unreachable!("handled above"),
                };
                feedback_html(
                    "warning",
                    &rust_i18n::t!(
                        "expected_tome.conflict",
                        locale = loc,
                        series = series,
                        position = &position
                    ),
                    &why,
                )
            }
        };
        html.push_str(&entry);
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(result: SettleResult) -> SettleOutcome {
        SettleOutcome {
            tome_id: 1,
            series_id: 2,
            series_name: "Astérix <b>".into(),
            position: 3,
            result,
        }
    }

    #[test]
    fn no_outcome_no_html() {
        assert_eq!(settle_feedback_html(&[], "en"), "");
    }

    #[test]
    fn settled_is_a_success_entry_naming_series_and_position() {
        let html = settle_feedback_html(&[outcome(SettleResult::Settled)], "en");
        assert!(html.contains("data-feedback-variant=\"success\""));
        assert!(html.contains("position 3"));
        assert!(
            html.contains("Astérix &lt;b&gt;"),
            "series name escaped: {html}"
        );
        assert!(!html.contains("<b>"));
    }

    #[test]
    fn each_conflict_is_a_warning_with_its_reason() {
        let html = settle_feedback_html(
            &[
                outcome(SettleResult::PositionHeld {
                    holder: "Le <i>Gaulois</i>".into(),
                }),
                outcome(SettleResult::TitleElsewhere { position: 5 }),
                outcome(SettleResult::OutOfRange { total: 2 }),
            ],
            "en",
        );
        assert_eq!(html.matches("data-feedback-variant=\"warning\"").count(), 3);
        assert_eq!(html.matches("nothing was attached").count(), 3);
        assert!(html.contains("Le &lt;i&gt;Gaulois&lt;/i&gt;"));
        assert!(html.contains("position 5"));
        assert!(html.contains("only 2 tomes"));
    }

    #[test]
    fn copy_is_localized() {
        let html = settle_feedback_html(&[outcome(SettleResult::Settled)], "fr");
        assert!(html.contains("Tome attendu trouvé"));
    }
}
