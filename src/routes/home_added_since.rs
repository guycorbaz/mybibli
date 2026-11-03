//! Story 13-7 (#586) — the home browse "added since <date>" filter.
//!
//! The URL carries `added_since=YYYY-MM-DD`; the filter keeps the titles
//! added on or after midnight UTC that day — the title itself or one of
//! its live volumes (the shared rule of `models::date_added`). It
//! composes with `q`, `filter` and `sort` / `dir`; under the indicator
//! single-active-filter contract (stories 9-4..9-7) a parsed indicator
//! filter wins and the date is ignored, like `q` and `sort`.
//!
//! Saved searches deliberately do not store it: a saved absolute date
//! ("since 2026-10-31") goes stale the next week, and the saved-search
//! table has no column for it.
//!
//! Kept out of `home.rs` (Foundation Rule #12).

use chrono::{NaiveDate, NaiveDateTime};

use crate::utils::parse_iso_date;

/// The parsed `added_since` URL parameter: `None` when absent, empty,
/// malformed, or overridden by an indicator filter.
pub(crate) fn parse_param(raw: Option<&str>, indicator_active: bool) -> Option<NaiveDate> {
    if indicator_active {
        return None;
    }
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let parsed = parse_iso_date(raw);
    if parsed.is_none() {
        tracing::debug!(added_since = %raw, "ignoring a malformed added_since parameter");
    }
    parsed
}

/// The UTC lower bound of the filter: midnight UTC on `date`.
pub(crate) fn lower_bound(date: Option<NaiveDate>) -> Option<NaiveDateTime> {
    date.and_then(|d| d.and_hms_opt(0, 0, 0))
}

/// The `&added_since=…` suffix the browse links append so the filter
/// survives sorting and paging; empty without a filter. Not escaped:
/// the value is a validated `YYYY-MM-DD` (Askama escapes the `&` when the
/// template interpolates it into an attribute).
pub(crate) fn query_suffix(date: Option<NaiveDate>) -> String {
    date.map(|d| format!("&added_since={}", d.format("%Y-%m-%d")))
        .unwrap_or_default()
}

/// What the home template needs to render the filter form and the
/// "Added" column.
#[derive(Debug, Clone, Default)]
pub struct AddedSinceControl {
    /// The active date as `YYYY-MM-DD`, or empty.
    pub value: String,
    /// [`query_suffix`] of the active date.
    pub query_suffix: String,
    pub label: String,
    pub apply_label: String,
    pub clear_label: String,
    /// Header of the "date added" column / sort option.
    pub col_label: String,
}

impl AddedSinceControl {
    pub(crate) fn new(date: Option<NaiveDate>, loc: &str) -> Self {
        AddedSinceControl {
            value: date
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
            query_suffix: query_suffix(date),
            label: rust_i18n::t!("browse.added_since", locale = loc).to_string(),
            apply_label: rust_i18n::t!("browse.added_since_apply", locale = loc).to_string(),
            clear_label: rust_i18n::t!("browse.added_since_clear", locale = loc).to_string(),
            col_label: rust_i18n::t!("search.col.added", locale = loc).to_string(),
        }
    }

    /// Whether a date filter is active.
    pub fn is_active(&self) -> bool {
        !self.value.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn parse_param_accepts_a_date_only() {
        assert_eq!(
            parse_param(Some("2026-10-31"), false),
            Some(d(2026, 10, 31))
        );
        assert_eq!(
            parse_param(Some(" 2026-10-31 "), false),
            Some(d(2026, 10, 31))
        );
        for ignored in [
            None,
            Some(""),
            Some("31/10/2026"),
            Some("2026-10-31T08:00:00Z"),
            Some("x"),
        ] {
            assert_eq!(parse_param(ignored, false), None, "{ignored:?}");
        }
    }

    #[test]
    fn an_indicator_filter_overrides_the_date() {
        assert_eq!(parse_param(Some("2026-10-31"), true), None);
    }

    #[test]
    fn lower_bound_is_midnight_utc() {
        assert_eq!(
            lower_bound(Some(d(2026, 10, 31))),
            d(2026, 10, 31).and_hms_opt(0, 0, 0)
        );
        assert_eq!(lower_bound(None), None);
    }

    #[test]
    fn query_suffix_round_trips_the_date() {
        assert_eq!(query_suffix(Some(d(2026, 1, 5))), "&added_since=2026-01-05");
        assert_eq!(query_suffix(None), "");
    }

    #[test]
    fn control_is_active_only_with_a_date() {
        let on = AddedSinceControl::new(Some(d(2026, 10, 31)), "en");
        assert!(on.is_active());
        assert_eq!(on.value, "2026-10-31");
        assert_eq!(on.label, "Added since");
        assert_eq!(on.col_label, "Added");
        let off = AddedSinceControl::new(None, "fr");
        assert!(!off.is_active());
        assert_eq!(off.query_suffix, "");
        assert_eq!(off.label, "Ajoutés depuis le");
    }

    /// The home page renders the filter form (date input carrying the
    /// active date, a Clear link), the "date added" sort options, and keeps
    /// the date in the browse links.
    #[test]
    fn home_renders_the_filter_and_keeps_the_date_in_browse_links() {
        use askama::Template;
        let mut t = crate::routes::home::tests::make_test_home_template_with_counts(
            "librarian",
            true,
            3,
            3,
            0,
        );
        t.added_since = AddedSinceControl::new(Some(d(2026, 10, 31)), "en");
        let item = crate::models::title::SearchResult {
            id: 9,
            title: "Fondation".to_string(),
            subtitle: None,
            media_type: "book".to_string(),
            genre_name: "Roman".to_string(),
            primary_contributor: None,
            volume_count: 1,
            cover_image_url: None,
            publication_date: None,
            dewey_code: None,
            created_at: None,
            last_added_at: d(2026, 11, 1).and_hms_opt(10, 0, 0),
        };
        let page = |items| {
            Some(crate::models::PaginatedList::new(
                items,
                1,
                1,
                Some("date_added".to_string()),
                Some("desc".to_string()),
                None,
            ))
        };
        t.current_sort = "date_added".to_string();
        t.current_dir = "desc".to_string();
        t.results = page(vec![item]);
        let html = t.render().expect("render");
        assert!(html.contains(r#"id="browse-added-since-form" method="get" action="/""#));
        assert!(
            html.contains(r#"name="added_since" value="2026-10-31""#),
            "{html}"
        );
        assert!(html.contains(r#"id="browse-added-since-clear""#));
        assert!(html.contains(r#"<option value="date_added:desc" selected"#));
        // The column headers sort without losing the date; the "Added"
        // header flips to oldest first once active.
        // (Askama escapes the suffix's `&` as `&#38;`.)
        assert!(
            html.contains("sort=title&amp;dir=asc&#38;added_since=2026-10-31&amp;page=1"),
            "{html}"
        );
        assert!(
            html.contains("sort=date_added&amp;dir=asc&#38;added_since=2026-10-31"),
            "{html}"
        );
        assert!(html.contains(r#"data-col="added">2026-11-01</td>"#));
        // An empty result under the date filter reads as a filter result,
        // not as a failed search offering to create a title.
        t.results = page(vec![]);
        let html = t.render().expect("render");
        assert!(html.contains("Nothing matches this filter."), "{html}");

        let off = crate::routes::home::tests::make_test_home_template_with_counts(
            "anonymous",
            false,
            3,
            3,
            0,
        );
        let html = off.render().expect("render");
        assert!(html.contains(r#"name="added_since" value="""#));
        assert!(!html.contains(r#"id="browse-added-since-clear""#));
        assert!(!html.contains("added_since=2026"));
    }
}
