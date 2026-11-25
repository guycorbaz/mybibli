//! Admin → Health: the "Production watching" section (story 19-5, #708).
//!
//! Data and labels for `templates/fragments/admin_health_watch.html`, read
//! from the in-memory [`crate::services::watch`] store on each Health tab
//! render — no polling, no database query.

use chrono_tz::Tz;

use crate::services::watch::{self, KindSnapshot, WatchKind, WatchSnapshot};
use crate::utils::local_time::format_local_date_time;

/// What the fragment prints. Built by [`build_watch_panel`].
pub struct WatchPanel {
    pub heading: String,
    pub col_last_24h: String,
    pub col_since_start: String,
    pub addresses_label: String,
    pub footnote: String,
    pub rows: Vec<WatchRow>,
}

pub struct WatchRow {
    /// Stable id prefix (`watch-csrf-refusals` → `…-24h`, `…-total`).
    pub id: &'static str,
    pub label: String,
    pub last_24h: u64,
    pub since_start: u64,
    /// Over its D5 threshold: warning colour and `marker`.
    pub above: bool,
    pub marker: String,
    /// The two refusal kinds list their most frequent addresses.
    pub tracks_addresses: bool,
    pub addresses: Vec<WatchAddress>,
    pub none_label: String,
    /// "and N more not listed", empty when no address fell outside the cap.
    pub other_label: String,
}

pub struct WatchAddress {
    pub address: String,
    pub count: u64,
}

fn row_id(kind: WatchKind) -> &'static str {
    match kind {
        WatchKind::SlowStatement => "watch-slow-statements",
        WatchKind::PoolTimeout => "watch-pool-timeouts",
        WatchKind::CsrfRefusal => "watch-csrf-refusals",
        WatchKind::ApiKeyRefusal => "watch-api-key-refusals",
    }
}

fn row_label(kind: WatchKind, loc: &str) -> String {
    match kind {
        WatchKind::SlowStatement => rust_i18n::t!("admin.health.watch.slow_statements", locale = loc),
        WatchKind::PoolTimeout => rust_i18n::t!("admin.health.watch.pool_timeouts", locale = loc),
        WatchKind::CsrfRefusal => rust_i18n::t!("admin.health.watch.csrf_refusals", locale = loc),
        WatchKind::ApiKeyRefusal => {
            rust_i18n::t!("admin.health.watch.api_key_refusals", locale = loc)
        }
    }
    .to_string()
}

fn build_row(k: &KindSnapshot, loc: &str) -> WatchRow {
    let other_label = if k.other_addresses > 0 {
        rust_i18n::t!(
            "admin.health.watch.addresses_other",
            locale = loc,
            count = k.other_addresses
        )
        .to_string()
    } else {
        String::new()
    };
    WatchRow {
        id: row_id(k.kind),
        label: row_label(k.kind, loc),
        last_24h: k.last_24h,
        since_start: k.since_start,
        above: k.above_threshold(),
        marker: rust_i18n::t!("admin.health.watch.above_marker", locale = loc).to_string(),
        tracks_addresses: k.kind.tracks_addresses(),
        addresses: k
            .top_addresses
            .iter()
            .map(|(address, count)| WatchAddress {
                address: address.clone(),
                count: *count,
            })
            .collect(),
        none_label: rust_i18n::t!("admin.health.watch.addresses_none", locale = loc).to_string(),
        other_label,
    }
}

/// The panel for `snapshot`, the start time shown in `tz`.
pub fn build_watch_panel(snapshot: &WatchSnapshot, tz: Tz, loc: &str) -> WatchPanel {
    let started = format_local_date_time(snapshot.started_at.naive_utc(), tz);
    WatchPanel {
        heading: rust_i18n::t!("admin.health.watch.heading", locale = loc).to_string(),
        col_last_24h: rust_i18n::t!("admin.health.watch.col_last_24h", locale = loc).to_string(),
        col_since_start: rust_i18n::t!(
            "admin.health.watch.col_since_start",
            locale = loc,
            time = started
        )
        .to_string(),
        addresses_label: rust_i18n::t!("admin.health.watch.addresses_label", locale = loc)
            .to_string(),
        footnote: rust_i18n::t!("admin.health.watch.footnote", locale = loc).to_string(),
        rows: snapshot.kinds.iter().map(|k| build_row(k, loc)).collect(),
    }
}

/// The panel for the process-wide store, now.
pub fn current_watch_panel(tz: Tz, loc: &str) -> WatchPanel {
    build_watch_panel(&watch::snapshot(), tz, loc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;
    use chrono::{TimeZone, Utc};

    use crate::services::watch::WatchStore;

    /// Renders the fragment alone, as the Health panel includes it.
    #[derive(Template)]
    #[template(path = "fragments/admin_health_watch.html")]
    struct WatchFragment {
        watch: WatchPanel,
    }

    fn render(store: &WatchStore, loc: &str) -> String {
        let now = Utc.with_ymd_and_hms(2026, 11, 25, 12, 30, 0).unwrap();
        let panel = build_watch_panel(&store.snapshot_at(now), chrono_tz::Europe::Zurich, loc);
        WatchFragment { watch: panel }.render().unwrap()
    }

    fn store() -> WatchStore {
        WatchStore::new(Utc.with_ymd_and_hms(2026, 11, 25, 7, 5, 0).unwrap())
    }

    fn record(store: &mut WatchStore, kind: WatchKind, client: Option<&str>, n: usize) {
        let now = Utc.with_ymd_and_hms(2026, 11, 25, 12, 0, 0).unwrap();
        for _ in 0..n {
            store.record_at(kind, client, now);
        }
    }

    /// The `<td>` with `id` and its content up to `</td>`.
    fn cell<'a>(html: &'a str, id: &str) -> &'a str {
        let start = html.find(&format!("id=\"{id}\"")).unwrap_or_else(|| panic!("{id} in {html}"));
        let rest = &html[start..];
        &rest[..rest.find("</td>").unwrap()]
    }

    #[test]
    fn a_quiet_instance_shows_no_marker() {
        let html = render(&store(), "en");
        assert!(html.contains("Production watching"));
        assert!(html.contains("Since start (2026-11-25 08:05)"), "{html}");
        assert!(html.contains("Counted in memory since the last start; a restart resets them."));
        assert!(!html.contains("above the usual level"));
        assert!(!html.contains("text-amber-800"));
        assert_eq!(html.matches("none").count(), 2, "{html}");
    }

    #[test]
    fn a_count_at_its_threshold_shows_no_marker_one_above_does() {
        let mut s = store();
        record(&mut s, WatchKind::CsrfRefusal, Some("10.0.0.1"), 20);
        record(&mut s, WatchKind::SlowStatement, None, 51);
        record(&mut s, WatchKind::PoolTimeout, None, 1);
        record(&mut s, WatchKind::ApiKeyRefusal, Some("10.0.0.2"), 21);
        let html = render(&s, "en");

        let csrf = cell(&html, "watch-csrf-refusals-24h");
        assert!(csrf.contains("20"));
        assert!(!csrf.contains("above the usual level"));
        assert!(!csrf.contains("text-amber-800"));

        for id in ["watch-slow-statements-24h", "watch-pool-timeouts-24h", "watch-api-key-refusals-24h"] {
            let c = cell(&html, id);
            assert!(c.contains("above the usual level"), "{id}: {c}");
            assert!(c.contains("text-amber-800 dark:text-amber-300 font-semibold"), "{id}: {c}");
        }
        assert!(cell(&html, "watch-slow-statements-total").contains("51"));
        assert!(!cell(&html, "watch-slow-statements-total").contains("above"));
    }

    #[test]
    fn refusal_rows_list_their_top_addresses_escaped() {
        let mut s = store();
        record(&mut s, WatchKind::CsrfRefusal, Some("10.0.0.1"), 3);
        record(&mut s, WatchKind::CsrfRefusal, Some("<b>evil</b>"), 1);
        let html = render(&s, "fr");
        assert!(html.contains("Adresses les plus fréquentes :"));
        assert!(html.contains("10.0.0.1</span> (3)"), "{html}");
        assert!(html.contains("&#60;b&#62;evil&#60;/b&#62;") || html.contains("&lt;b&gt;evil"), "{html}");
        assert!(!html.contains("<b>evil"));
        // The API-key row has none.
        assert!(html.contains("aucune"));
    }

    #[test]
    fn addresses_beyond_the_cap_are_announced() {
        let mut s = store();
        for i in 0..(watch::MAX_ADDRESSES_PER_BUCKET + 2) {
            record(&mut s, WatchKind::ApiKeyRefusal, Some(&format!("10.1.{i}.1")), 1);
        }
        let html = render(&s, "en");
        assert!(html.contains("and 2 more not listed"), "{html}");
    }

    #[test]
    fn every_locale_has_the_copy() {
        for loc in ["en", "fr", "de", "it"] {
            let html = render(&store(), loc);
            assert!(!html.contains("admin.health.watch"), "{loc}: {html}");
        }
    }
}
