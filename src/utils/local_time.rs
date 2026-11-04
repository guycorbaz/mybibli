//! Story 15-1 (#602) — the instance time zone.
//!
//! Storage, the HTTP API and the MCP server stay UTC (every `TIMESTAMP`
//! column is read as a UTC `NaiveDateTime`: database sessions run in UTC,
//! `src/db.rs`). The web interface shows dates and cuts days in the
//! instance zone (Admin → System, default `Europe/Zurich`). This module
//! is the only place that converts between the two (Rule #1): display
//! conversion, the local calendar day of an instant, and the UTC bounds of
//! a local day. No call site adds or subtracts an offset itself.

use chrono::{Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

/// The zone of a fresh instance, and the fallback for an invalid row.
pub const DEFAULT_TIME_ZONE: Tz = chrono_tz::Europe::Zurich;

/// `YYYY-MM-DD HH:MM`, the date-time shape of the admin and review pages.
pub const DATE_TIME_FORMAT: &str = "%Y-%m-%d %H:%M";

/// An IANA zone name (`Europe/Zurich`, `America/New_York`, `UTC`), exact
/// spelling. `None` for anything else, the empty string included.
pub fn parse_time_zone(name: &str) -> Option<Tz> {
    name.trim().parse::<Tz>().ok()
}

/// Every zone name the instance accepts, for the Admin → System picker.
pub fn time_zone_names() -> impl Iterator<Item = &'static str> {
    chrono_tz::TZ_VARIANTS.iter().map(|tz| tz.name())
}

/// A stored UTC instant as wall-clock time in `tz`.
pub fn to_local(utc: NaiveDateTime, tz: Tz) -> NaiveDateTime {
    tz.from_utc_datetime(&utc).naive_local()
}

/// The calendar day, in `tz`, of a stored UTC instant.
pub fn local_date(utc: NaiveDateTime, tz: Tz) -> NaiveDate {
    to_local(utc, tz).date()
}

/// A stored UTC instant formatted in `tz` with a `chrono` pattern.
pub fn format_local(utc: NaiveDateTime, tz: Tz, pattern: &str) -> String {
    to_local(utc, tz).format(pattern).to_string()
}

/// The local calendar day of a stored UTC instant, as `YYYY-MM-DD`.
pub fn format_local_date(utc: NaiveDateTime, tz: Tz) -> String {
    local_date(utc, tz).to_string()
}

/// A stored UTC instant as `YYYY-MM-DD HH:MM` in `tz`.
pub fn format_local_date_time(utc: NaiveDateTime, tz: Tz) -> String {
    format_local(utc, tz, DATE_TIME_FORMAT)
}

/// The first UTC instant of the local day `day` in `tz`: local midnight,
/// or — in a zone whose clocks jump over midnight on that day — the first
/// local time after the gap. On a repeated midnight (clocks set back over
/// it) the earlier of the two instants.
pub fn day_start_utc(day: NaiveDate, tz: Tz) -> NaiveDateTime {
    let midnight = day.and_time(chrono::NaiveTime::MIN);
    let mut local = midnight;
    // A DST gap is at most a few hours; step by minutes until it ends.
    for _ in 0..(24 * 60) {
        match tz.from_local_datetime(&local) {
            LocalResult::Single(at) => return at.naive_utc(),
            LocalResult::Ambiguous(earlier, _) => return earlier.naive_utc(),
            LocalResult::None => local += Duration::minutes(1),
        }
    }
    // Unreachable for any real zone (no gap lasts a whole day).
    midnight
}

/// The UTC bounds `[start, end)` of the local day `day` in `tz`. 23 or 25
/// hours long on the days the clocks change.
pub fn day_bounds_utc(day: NaiveDate, tz: Tz) -> (NaiveDateTime, NaiveDateTime) {
    let next = day.succ_opt().unwrap_or(day);
    (day_start_utc(day, tz), day_start_utc(next, tz))
}

/// "Now" seen from the instance zone. Built once per request
/// ([`LocalClock::now`]) so every day count of a page uses the same
/// "today"; tests pin the instant with [`LocalClock::at`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalClock {
    tz: Tz,
    now_utc: NaiveDateTime,
}

impl LocalClock {
    pub fn now(tz: Tz) -> Self {
        Self::at(tz, Utc::now().naive_utc())
    }

    pub fn at(tz: Tz, now_utc: NaiveDateTime) -> Self {
        LocalClock { tz, now_utc }
    }

    pub fn tz(&self) -> Tz {
        self.tz
    }

    /// The instant, in UTC.
    pub fn now_utc(&self) -> NaiveDateTime {
        self.now_utc
    }

    /// Today's calendar day in the instance zone.
    pub fn today(&self) -> NaiveDate {
        local_date(self.now_utc, self.tz)
    }

    /// Whole local calendar days from the day of `utc` to today: 0 for
    /// an instant earlier today, 1 for one yesterday at 23:59 (the
    /// semantic of `DATEDIFF(NOW(), …)`, cut in the instance zone).
    pub fn days_since(&self, utc: NaiveDateTime) -> i64 {
        (self.today() - local_date(utc, self.tz)).num_days()
    }

    /// The first UTC instant of the local day `days` days before today.
    /// `days_since(x) > n` ⇔ `x < start_of_day_days_ago(n)` — the bound
    /// of a SQL filter that counts local days.
    pub fn start_of_day_days_ago(&self, days: i64) -> NaiveDateTime {
        let day = self
            .today()
            .checked_sub_signed(Duration::days(days))
            .unwrap_or(NaiveDate::MIN);
        day_start_utc(day, self.tz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    const ZURICH: Tz = chrono_tz::Europe::Zurich;

    #[test]
    fn default_zone_is_zurich() {
        assert_eq!(DEFAULT_TIME_ZONE.name(), "Europe/Zurich");
    }

    #[test]
    fn parse_accepts_iana_names_only() {
        assert_eq!(parse_time_zone("Europe/Zurich"), Some(ZURICH));
        assert_eq!(
            parse_time_zone(" America/New_York "),
            Some(chrono_tz::America::New_York)
        );
        assert_eq!(parse_time_zone("UTC"), Some(chrono_tz::UTC));
        assert_eq!(parse_time_zone(""), None);
        assert_eq!(parse_time_zone("Europe/Zürich"), None);
        assert_eq!(parse_time_zone("Mars/Olympus_Mons"), None);
        assert_eq!(parse_time_zone("+01:00"), None);
    }

    #[test]
    fn names_list_contains_the_default() {
        assert!(time_zone_names().any(|n| n == "Europe/Zurich"));
        assert!(time_zone_names().count() > 300);
    }

    #[test]
    fn half_past_midnight_in_zurich_is_the_next_day() {
        // 23:30 UTC on 4 Nov = 00:30 CET on 5 Nov (winter, UTC+1).
        let at = utc("2026-11-04 23:30:00");
        assert_eq!(local_date(at, ZURICH), day("2026-11-05"));
        assert_eq!(format_local_date_time(at, ZURICH), "2026-11-05 00:30");
        // 22:30 UTC in summer = 00:30 CEST (UTC+2).
        let at = utc("2026-07-01 22:30:00");
        assert_eq!(local_date(at, ZURICH), day("2026-07-02"));
        // In UTC itself nothing moves.
        assert_eq!(
            local_date(utc("2026-11-04 23:30:00"), chrono_tz::UTC),
            day("2026-11-04")
        );
    }

    #[test]
    fn day_bounds_in_winter_and_summer() {
        assert_eq!(
            day_bounds_utc(day("2026-11-05"), ZURICH),
            (utc("2026-11-04 23:00:00"), utc("2026-11-05 23:00:00"))
        );
        assert_eq!(
            day_bounds_utc(day("2026-07-02"), ZURICH),
            (utc("2026-07-01 22:00:00"), utc("2026-07-02 22:00:00"))
        );
    }

    #[test]
    fn spring_forward_day_is_23_hours_long() {
        // 29 Mar 2026: 02:00 CET → 03:00 CEST.
        let (start, end) = day_bounds_utc(day("2026-03-29"), ZURICH);
        assert_eq!(start, utc("2026-03-28 23:00:00"));
        assert_eq!(end, utc("2026-03-29 22:00:00"));
        assert_eq!((end - start).num_hours(), 23);
        // 00:59 UTC = 01:59 CET, 01:00 UTC = 03:00 CEST: same local day.
        assert_eq!(
            format_local_date_time(utc("2026-03-29 00:59:00"), ZURICH),
            "2026-03-29 01:59"
        );
        assert_eq!(
            format_local_date_time(utc("2026-03-29 01:00:00"), ZURICH),
            "2026-03-29 03:00"
        );
    }

    #[test]
    fn fall_back_day_is_25_hours_long() {
        // 25 Oct 2026: 03:00 CEST → 02:00 CET.
        let (start, end) = day_bounds_utc(day("2026-10-25"), ZURICH);
        assert_eq!(start, utc("2026-10-24 22:00:00"));
        assert_eq!(end, utc("2026-10-25 23:00:00"));
        assert_eq!((end - start).num_hours(), 25);
        // 00:30 and 01:30 UTC both read 02:30 local.
        assert_eq!(
            format_local_date_time(utc("2026-10-25 00:30:00"), ZURICH),
            "2026-10-25 02:30"
        );
        assert_eq!(
            format_local_date_time(utc("2026-10-25 01:30:00"), ZURICH),
            "2026-10-25 02:30"
        );
        // The last instant of the day is still that day.
        assert_eq!(
            local_date(utc("2026-10-25 22:59:59"), ZURICH),
            day("2026-10-25")
        );
        assert_eq!(
            local_date(utc("2026-10-25 23:00:00"), ZURICH),
            day("2026-10-26")
        );
    }

    #[test]
    fn a_day_without_midnight_starts_after_the_gap() {
        // America/Santiago, 7 Sep 2025: 00:00 -04 → 01:00 -03.
        let tz: Tz = chrono_tz::America::Santiago;
        let start = day_start_utc(day("2025-09-07"), tz);
        assert_eq!(start, utc("2025-09-07 04:00:00"));
        assert_eq!(format_local_date_time(start, tz), "2025-09-07 01:00");
    }

    #[test]
    fn days_since_counts_local_calendar_days() {
        // Now: 5 Nov 2026 00:10 in Zurich (23:10 UTC on 4 Nov).
        let clock = LocalClock::at(ZURICH, utc("2026-11-04 23:10:00"));
        assert_eq!(clock.today(), day("2026-11-05"));
        // A loan at 22:50 UTC on 4 Nov (23:50 local on 4 Nov) is a day old
        // locally, although DATEDIFF in UTC would say 0.
        assert_eq!(clock.days_since(utc("2026-11-04 22:50:00")), 1);
        // One at 23:05 UTC is 00:05 local today.
        assert_eq!(clock.days_since(utc("2026-11-04 23:05:00")), 0);
    }

    #[test]
    fn start_of_day_days_ago_is_the_overdue_bound() {
        let clock = LocalClock::at(ZURICH, utc("2026-11-04 23:10:00"));
        let bound = clock.start_of_day_days_ago(30);
        // Today is 5 Nov local; 30 days earlier is 6 Oct (CEST, UTC+2).
        assert_eq!(bound, utc("2026-10-05 22:00:00"));
        // days_since > 30 ⇔ instant < bound.
        let just_before = bound - Duration::seconds(1);
        assert_eq!(clock.days_since(just_before), 31);
        assert_eq!(clock.days_since(bound), 30);
    }

    #[test]
    fn to_local_follows_the_zone() {
        let at = utc("2026-01-15 12:00:00");
        assert_eq!(
            to_local(at, chrono_tz::America::New_York),
            utc("2026-01-15 07:00:00")
        );
        assert_eq!(
            to_local(at, chrono_tz::Asia::Tokyo),
            utc("2026-01-15 21:00:00")
        );
        assert_eq!(format_local(at, ZURICH, "%Y-%m-%d"), "2026-01-15");
    }
}
