//! Production watching (story 19-5, #708): four counters an Admin reads on
//! the Health tab instead of the logs — slow database statements, failed
//! connection waits, CSRF refusals and API-key refusals.
//!
//! Everything lives in memory (D1): no table, no write per request, reset on
//! restart. [`WatchStore`] is the plain logic with an injectable clock; the
//! process-wide [`WATCH`] only wraps it in a mutex, because the `tracing`
//! layer and the `From<sqlx::Error>` conversion cannot reach `AppState`.
//!
//! Where the counts are taken (D3):
//! - slow statements: [`SlowStatementLayer`], a `tracing` layer with its own
//!   per-layer filter, so the operator's log level never hides them;
//! - pool timeouts: `impl From<sqlx::Error> for AppError`;
//! - CSRF refusals: `middleware::csrf::reject`;
//! - API-key refusals: `middleware::api_key::{unauthorized, insufficient_scope}`.
//!
//! Recording is a mutex lock and a few integer updates (plus a new address
//! key at most): no I/O, no database query. The mutex is never held across
//! an `.await`, and a poisoned lock is recovered, never re-panicked.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

/// Hourly buckets kept per kind: the "last 24 hours" window.
pub const BUCKETS: usize = 24;
/// Distinct client addresses kept per hourly bucket (D2); refusals from
/// further addresses still count, in the bucket's `other` count.
pub const MAX_ADDRESSES_PER_BUCKET: usize = 64;
/// Addresses listed under a refusal row.
pub const TOP_ADDRESSES: usize = 3;

/// D5 thresholds, over the last 24 hours (v1 constants, not settings).
pub const SLOW_STATEMENT_THRESHOLD: u64 = 50;
pub const POOL_TIMEOUT_THRESHOLD: u64 = 0;
pub const CSRF_REFUSAL_THRESHOLD: u64 = 20;
pub const API_KEY_REFUSAL_THRESHOLD: u64 = 20;

/// `tracing` target of sqlx's statement log (`sqlx-core` `logger.rs`).
pub const SQLX_QUERY_TARGET: &str = "sqlx::query";
/// Start of the message sqlx logs for a statement over its threshold.
pub const SLOW_STATEMENT_PREFIX: &str = "slow statement";

const SECS_PER_HOUR: i64 = 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchKind {
    SlowStatement,
    PoolTimeout,
    CsrfRefusal,
    ApiKeyRefusal,
}

impl WatchKind {
    /// Display order of the panel.
    pub const ALL: [WatchKind; 4] = [
        WatchKind::SlowStatement,
        WatchKind::PoolTimeout,
        WatchKind::CsrfRefusal,
        WatchKind::ApiKeyRefusal,
    ];

    fn index(self) -> usize {
        match self {
            WatchKind::SlowStatement => 0,
            WatchKind::PoolTimeout => 1,
            WatchKind::CsrfRefusal => 2,
            WatchKind::ApiKeyRefusal => 3,
        }
    }

    /// The D5 threshold: a last-24-hours count strictly above it is shown
    /// as unusual.
    pub fn threshold(self) -> u64 {
        match self {
            WatchKind::SlowStatement => SLOW_STATEMENT_THRESHOLD,
            WatchKind::PoolTimeout => POOL_TIMEOUT_THRESHOLD,
            WatchKind::CsrfRefusal => CSRF_REFUSAL_THRESHOLD,
            WatchKind::ApiKeyRefusal => API_KEY_REFUSAL_THRESHOLD,
        }
    }

    /// The two refusal kinds keep per-address counts.
    pub fn tracks_addresses(self) -> bool {
        matches!(self, WatchKind::CsrfRefusal | WatchKind::ApiKeyRefusal)
    }
}

/// One hour of one kind. `hour` is the UTC instant truncated to the hour,
/// in hours since the epoch; a slot reused for a newer hour is reset.
#[derive(Debug, Clone)]
struct Bucket {
    hour: i64,
    count: u64,
    addresses: HashMap<String, u64>,
    other: u64,
}

impl Bucket {
    fn empty(hour: i64) -> Self {
        Bucket {
            hour,
            count: 0,
            addresses: HashMap::new(),
            other: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct KindCounts {
    total: u64,
    buckets: Vec<Bucket>,
}

impl KindCounts {
    fn new() -> Self {
        KindCounts {
            total: 0,
            // `i64::MIN` never falls in a window: an unused slot.
            buckets: (0..BUCKETS).map(|_| Bucket::empty(i64::MIN)).collect(),
        }
    }
}

/// What the panel reads for one kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindSnapshot {
    pub kind: WatchKind,
    pub last_24h: u64,
    pub since_start: u64,
    /// Most refusals first, then address ascending; at most [`TOP_ADDRESSES`].
    pub top_addresses: Vec<(String, u64)>,
    /// Refusals over the last 24 hours whose address fell outside a full
    /// bucket's [`MAX_ADDRESSES_PER_BUCKET`].
    pub other_addresses: u64,
    /// Addresses of the last 24 hours not in [`Self::top_addresses`]: the
    /// distinct kept addresses beyond the top three, plus one per refusal
    /// whose address was not kept (an upper bound, reached only past the
    /// cap). What "and N more not listed" names (story 19-5 review).
    pub unlisted_addresses: u64,
}

impl KindSnapshot {
    pub fn above_threshold(&self) -> bool {
        self.last_24h > self.kind.threshold()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchSnapshot {
    pub started_at: DateTime<Utc>,
    /// In [`WatchKind::ALL`] order.
    pub kinds: Vec<KindSnapshot>,
}

impl WatchSnapshot {
    pub fn kind(&self, kind: WatchKind) -> &KindSnapshot {
        &self.kinds[kind.index()]
    }
}

/// The counting logic, clock injected (D1).
#[derive(Debug, Clone)]
pub struct WatchStore {
    started_at: DateTime<Utc>,
    kinds: Vec<KindCounts>,
}

fn hour_of(at: DateTime<Utc>) -> i64 {
    at.timestamp().div_euclid(SECS_PER_HOUR)
}

/// A bucket is in the window when its hour started less than 24 hours
/// before `now`: the current hour and the 23 before it.
fn in_window(bucket_hour: i64, now_hour: i64) -> bool {
    bucket_hour <= now_hour && bucket_hour > now_hour - BUCKETS as i64
}

impl WatchStore {
    pub fn new(started_at: DateTime<Utc>) -> Self {
        WatchStore {
            started_at,
            kinds: WatchKind::ALL.iter().map(|_| KindCounts::new()).collect(),
        }
    }

    pub fn started_at(&self) -> DateTime<Utc> {
        self.started_at
    }

    /// Count one `kind` event at `now`, from `client` for the refusal kinds
    /// (ignored for the others).
    pub fn record_at(&mut self, kind: WatchKind, client: Option<&str>, now: DateTime<Utc>) {
        let counts = &mut self.kinds[kind.index()];
        counts.total += 1;
        let hour = hour_of(now);
        let slot = hour.rem_euclid(BUCKETS as i64) as usize;
        let bucket = &mut counts.buckets[slot];
        if bucket.hour != hour {
            *bucket = Bucket::empty(hour);
        }
        bucket.count += 1;
        if !kind.tracks_addresses() {
            return;
        }
        let Some(client) = client else {
            return;
        };
        if let Some(n) = bucket.addresses.get_mut(client) {
            *n += 1;
        } else if bucket.addresses.len() < MAX_ADDRESSES_PER_BUCKET {
            bucket.addresses.insert(client.to_string(), 1);
        } else {
            bucket.other += 1;
        }
    }

    pub fn snapshot_at(&self, now: DateTime<Utc>) -> WatchSnapshot {
        let now_hour = hour_of(now);
        let kinds = WatchKind::ALL
            .iter()
            .map(|&kind| {
                let counts = &self.kinds[kind.index()];
                let mut last_24h = 0;
                let mut other_addresses = 0;
                let mut merged: HashMap<&str, u64> = HashMap::new();
                for bucket in counts.buckets.iter().filter(|b| in_window(b.hour, now_hour)) {
                    last_24h += bucket.count;
                    other_addresses += bucket.other;
                    for (addr, n) in &bucket.addresses {
                        *merged.entry(addr.as_str()).or_insert(0) += n;
                    }
                }
                let mut top: Vec<(String, u64)> =
                    merged.into_iter().map(|(a, n)| (a.to_string(), n)).collect();
                top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                let unlisted_addresses =
                    top.len().saturating_sub(TOP_ADDRESSES) as u64 + other_addresses;
                top.truncate(TOP_ADDRESSES);
                KindSnapshot {
                    kind,
                    last_24h,
                    since_start: counts.total,
                    top_addresses: top,
                    other_addresses,
                    unlisted_addresses,
                }
            })
            .collect();
        WatchSnapshot {
            started_at: self.started_at,
            kinds,
        }
    }
}

/// A [`WatchStore`] behind a mutex, on the real clock.
#[derive(Debug)]
pub struct Watch {
    store: Mutex<WatchStore>,
}

impl Default for Watch {
    fn default() -> Self {
        Self::new()
    }
}

impl Watch {
    pub fn new() -> Self {
        Watch {
            store: Mutex::new(WatchStore::new(Utc::now())),
        }
    }

    fn lock(&self) -> MutexGuard<'_, WatchStore> {
        self.store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn record(&self, kind: WatchKind, client: Option<&str>) {
        self.lock().record_at(kind, client, Utc::now());
    }

    pub fn snapshot(&self) -> WatchSnapshot {
        self.lock().snapshot_at(Utc::now())
    }
}

/// The process-wide store. Its start time is the first touch; `main` calls
/// [`start`] at boot so that is the process start.
pub static WATCH: LazyLock<Watch> = LazyLock::new(Watch::new);

/// Fix the start time of [`WATCH`] (called once from `main`).
pub fn start() {
    LazyLock::force(&WATCH);
}

/// Count one event in [`WATCH`].
pub fn record(kind: WatchKind, client: Option<&str>) {
    WATCH.record(kind, client);
}

/// Read [`WATCH`] now.
pub fn snapshot() -> WatchSnapshot {
    WATCH.snapshot()
}

/// Counts sqlx's slow-statement warnings into a [`Watch`]. Install it with
/// [`slow_statement_filter`] as its per-layer filter (see [`subscriber`]).
pub struct SlowStatementLayer {
    watch: &'static Watch,
}

impl SlowStatementLayer {
    pub fn new(watch: &'static Watch) -> Self {
        SlowStatementLayer { watch }
    }
}

/// Reads the `message` field of an event and tells whether it starts with
/// [`SLOW_STATEMENT_PREFIX`].
#[derive(Default)]
struct SlowMessageVisitor {
    slow: bool,
}

impl Visit for SlowMessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.slow = value.starts_with(SLOW_STATEMENT_PREFIX);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.slow = format!("{value:?}").starts_with(SLOW_STATEMENT_PREFIX);
        }
    }
}

impl<S: Subscriber> Layer<S> for SlowStatementLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        if meta.target() != SQLX_QUERY_TARGET || *meta.level() != Level::WARN {
            return;
        }
        let mut visitor = SlowMessageVisitor::default();
        event.record(&mut visitor);
        if visitor.slow {
            self.watch.record(WatchKind::SlowStatement, None);
        }
    }
}

/// The per-layer filter of [`SlowStatementLayer`]: `sqlx::query` at `WARN`
/// and above, whatever the operator's log level.
pub fn slow_statement_filter() -> tracing_subscriber::filter::Targets {
    tracing_subscriber::filter::Targets::new().with_target(SQLX_QUERY_TARGET, Level::WARN)
}

/// The reload handle of the operator's log filter (admin "Log level" form).
pub type LogFilterHandle = tracing_subscriber::reload::Handle<
    tracing_subscriber::EnvFilter,
    tracing_subscriber::Registry,
>;

/// The application's `tracing` subscriber: the log output layers (`output`,
/// stdout and the optional file) under the operator's `filter` as a
/// **per-layer** filter behind one reload handle, and the slow-statement
/// counter under its own filter — so an `error` log level hides the
/// warnings from the log but not from the count. The log output is the
/// same as with a global filter: same filter, same layers.
pub fn subscriber<L>(
    output: L,
    filter: tracing_subscriber::EnvFilter,
    watch: &'static Watch,
) -> (impl Subscriber + Send + Sync + 'static, LogFilterHandle)
where
    L: Layer<tracing_subscriber::Registry> + Send + Sync + 'static,
{
    let (filter, handle) = tracing_subscriber::reload::Layer::new(filter);
    let subscriber = tracing_subscriber::registry()
        .with(output.with_filter(filter))
        .with(SlowStatementLayer::new(watch).with_filter(slow_statement_filter()));
    (subscriber, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::io::Write;
    use std::sync::Arc;

    fn at(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 25, h, m, s).unwrap()
    }

    fn store() -> WatchStore {
        WatchStore::new(at(0, 0, 0))
    }

    #[test]
    fn thresholds_are_the_d5_values() {
        assert_eq!(SLOW_STATEMENT_THRESHOLD, 50);
        assert_eq!(POOL_TIMEOUT_THRESHOLD, 0);
        assert_eq!(CSRF_REFUSAL_THRESHOLD, 20);
        assert_eq!(API_KEY_REFUSAL_THRESHOLD, 20);
        assert_eq!(WatchKind::SlowStatement.threshold(), 50);
        assert_eq!(WatchKind::PoolTimeout.threshold(), 0);
        assert_eq!(WatchKind::CsrfRefusal.threshold(), 20);
        assert_eq!(WatchKind::ApiKeyRefusal.threshold(), 20);
    }

    #[test]
    fn caps_are_the_d2_values() {
        assert_eq!(MAX_ADDRESSES_PER_BUCKET, 64);
        assert_eq!(TOP_ADDRESSES, 3);
        assert_eq!(BUCKETS, 24);
    }

    #[test]
    fn above_threshold_is_strictly_above() {
        let mut s = store();
        let now = at(12, 0, 0);
        for _ in 0..20 {
            s.record_at(WatchKind::CsrfRefusal, Some("10.0.0.1"), now);
        }
        s.record_at(WatchKind::PoolTimeout, None, now);
        let snap = s.snapshot_at(now);
        assert!(!snap.kind(WatchKind::CsrfRefusal).above_threshold());
        assert!(snap.kind(WatchKind::PoolTimeout).above_threshold());
        assert!(!snap.kind(WatchKind::SlowStatement).above_threshold());
        s.record_at(WatchKind::CsrfRefusal, Some("10.0.0.1"), now);
        assert!(s.snapshot_at(now).kind(WatchKind::CsrfRefusal).above_threshold());
    }

    /// Buckets are hourly: the window holds the current hour and the 23
    /// before it. Read at hh:59:30, an event 23 h 59 min earlier falls in
    /// the oldest bucket still in the window, one 24 h earlier just before.
    #[test]
    fn the_window_is_the_last_24_hours() {
        let now = at(10, 59, 30);
        let mut s = store();
        s.record_at(WatchKind::SlowStatement, None, now - chrono::Duration::minutes(23 * 60 + 59));
        assert_eq!(s.snapshot_at(now).kind(WatchKind::SlowStatement).last_24h, 1);

        let mut s = store();
        s.record_at(WatchKind::SlowStatement, None, now - chrono::Duration::hours(24));
        let snap = s.snapshot_at(now);
        assert_eq!(snap.kind(WatchKind::SlowStatement).last_24h, 0);
        assert_eq!(snap.kind(WatchKind::SlowStatement).since_start, 1);
    }

    #[test]
    fn a_reused_slot_is_reset() {
        let mut s = store();
        let yesterday = at(9, 10, 0) - chrono::Duration::hours(24);
        for _ in 0..5 {
            s.record_at(WatchKind::CsrfRefusal, Some("10.0.0.9"), yesterday);
        }
        // Same slot (09:00), one day later.
        let today = at(9, 20, 0);
        s.record_at(WatchKind::CsrfRefusal, Some("10.0.0.1"), today);
        let snap = s.snapshot_at(today);
        let csrf = snap.kind(WatchKind::CsrfRefusal);
        assert_eq!(csrf.last_24h, 1);
        assert_eq!(csrf.since_start, 6);
        assert_eq!(csrf.top_addresses, vec![("10.0.0.1".to_string(), 1)]);
    }

    #[test]
    fn since_start_counts_every_event() {
        let mut s = store();
        for h in 0..30 {
            s.record_at(WatchKind::ApiKeyRefusal, Some("a"), at(0, 0, 0) + chrono::Duration::hours(h));
        }
        let snap = s.snapshot_at(at(0, 0, 0) + chrono::Duration::hours(29));
        assert_eq!(snap.kind(WatchKind::ApiKeyRefusal).since_start, 30);
        assert_eq!(snap.kind(WatchKind::ApiKeyRefusal).last_24h, 24);
        assert_eq!(snap.started_at, at(0, 0, 0));
        assert_eq!(snap.kind(WatchKind::SlowStatement).since_start, 0);
    }

    #[test]
    fn kinds_are_counted_apart() {
        let mut s = store();
        let now = at(8, 0, 0);
        s.record_at(WatchKind::SlowStatement, None, now);
        s.record_at(WatchKind::PoolTimeout, None, now);
        s.record_at(WatchKind::PoolTimeout, None, now);
        let snap = s.snapshot_at(now);
        assert_eq!(snap.kind(WatchKind::SlowStatement).last_24h, 1);
        assert_eq!(snap.kind(WatchKind::PoolTimeout).last_24h, 2);
        assert_eq!(snap.kind(WatchKind::CsrfRefusal).last_24h, 0);
        assert!(snap.kind(WatchKind::PoolTimeout).top_addresses.is_empty());
    }

    #[test]
    fn top_addresses_by_count_then_address() {
        let mut s = store();
        let now = at(14, 30, 0);
        let earlier = now - chrono::Duration::hours(3);
        for (addr, n, when) in [
            ("10.0.0.5", 2, now),
            ("10.0.0.3", 1, earlier),
            ("10.0.0.3", 1, now),
            ("10.0.0.9", 4, earlier),
            ("10.0.0.1", 2, earlier),
            ("10.0.0.7", 1, now),
        ] {
            for _ in 0..n {
                s.record_at(WatchKind::CsrfRefusal, Some(addr), when);
            }
        }
        let snap = s.snapshot_at(now);
        assert_eq!(
            snap.kind(WatchKind::CsrfRefusal).top_addresses,
            vec![
                ("10.0.0.9".to_string(), 4),
                ("10.0.0.1".to_string(), 2),
                ("10.0.0.3".to_string(), 2),
            ]
        );
    }

    #[test]
    fn addresses_beyond_the_top_three_are_counted_as_unlisted() {
        let mut s = store();
        let now = at(15, 0, 0);
        for i in 0..10 {
            s.record_at(WatchKind::CsrfRefusal, Some(&format!("10.1.0.{i}")), now);
        }
        let snap = s.snapshot_at(now);
        let csrf = snap.kind(WatchKind::CsrfRefusal);
        assert_eq!(csrf.top_addresses.len(), 3);
        assert_eq!(csrf.other_addresses, 0);
        assert_eq!(csrf.unlisted_addresses, 7);
        // Three addresses or fewer: nothing unlisted.
        let mut s = store();
        s.record_at(WatchKind::CsrfRefusal, Some("10.1.0.1"), now);
        assert_eq!(s.snapshot_at(now).kind(WatchKind::CsrfRefusal).unlisted_addresses, 0);
    }

    #[test]
    fn addresses_beyond_the_cap_count_as_other() {
        let mut s = store();
        let now = at(16, 0, 0);
        for i in 0..MAX_ADDRESSES_PER_BUCKET {
            s.record_at(WatchKind::ApiKeyRefusal, Some(&format!("10.0.{i}.1")), now);
        }
        // A known address still counts in its own entry.
        s.record_at(WatchKind::ApiKeyRefusal, Some("10.0.0.1"), now);
        for _ in 0..3 {
            s.record_at(WatchKind::ApiKeyRefusal, Some("192.168.1.1"), now);
        }
        let snap = s.snapshot_at(now);
        let api = snap.kind(WatchKind::ApiKeyRefusal);
        assert_eq!(api.last_24h, MAX_ADDRESSES_PER_BUCKET as u64 + 4);
        assert_eq!(api.other_addresses, 3);
        // 64 kept addresses, 3 listed, plus the 3 refusals not kept.
        assert_eq!(api.unlisted_addresses, MAX_ADDRESSES_PER_BUCKET as u64 - 3 + 3);
        assert_eq!(api.top_addresses[0], ("10.0.0.1".to_string(), 2));
        assert!(!api.top_addresses.iter().any(|(a, _)| a == "192.168.1.1"));
        let bucket = &s.kinds[WatchKind::ApiKeyRefusal.index()].buckets
            [hour_of(now).rem_euclid(BUCKETS as i64) as usize];
        assert_eq!(bucket.addresses.len(), MAX_ADDRESSES_PER_BUCKET);
    }

    #[test]
    fn non_refusal_kinds_keep_no_address() {
        let mut s = store();
        let now = at(1, 0, 0);
        s.record_at(WatchKind::SlowStatement, Some("10.0.0.1"), now);
        assert!(s.snapshot_at(now).kind(WatchKind::SlowStatement).top_addresses.is_empty());
    }

    #[test]
    fn a_poisoned_watch_still_counts() {
        let watch = Arc::new(Watch::new());
        let w = Arc::clone(&watch);
        let _ = std::thread::spawn(move || {
            let _guard = w.store.lock().unwrap();
            panic!("poison the lock");
        })
        .join();
        assert!(watch.store.is_poisoned());
        watch.record(WatchKind::PoolTimeout, None);
        assert_eq!(watch.snapshot().kind(WatchKind::PoolTimeout).since_start, 1);
    }

    // ── subscriber: slow-statement layer + per-layer log filter ──────────

    /// A `MakeWriter` collecting the log output in memory.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
        type Writer = Captured;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Built like `main.rs`: the JSON output layer under the reloadable
    /// operator filter, the slow-statement counter under its own.
    fn test_subscriber(
        directive: &str,
    ) -> (impl Subscriber + Send + Sync, LogFilterHandle, Captured, &'static Watch) {
        let captured = Captured::default();
        let output = tracing_subscriber::fmt::layer()
            .json()
            .with_target(true)
            .with_writer(captured.clone());
        let watch: &'static Watch = Box::leak(Box::new(Watch::new()));
        let filter = tracing_subscriber::EnvFilter::new(directive);
        let (subscriber, handle) = subscriber(output, filter, watch);
        (subscriber, handle, captured, watch)
    }

    fn emit_slow_statement() {
        tracing::warn!(
            target: "sqlx::query",
            summary = "select 1",
            elapsed_secs = 1.5,
            "slow statement: execution time exceeded alert threshold"
        );
    }

    #[test]
    fn a_slow_statement_is_counted_under_an_error_log_level() {
        let (subscriber, _handle, captured, watch) = test_subscriber("error");
        tracing::subscriber::with_default(subscriber, || {
            assert!(tracing::enabled!(target: "sqlx::query", Level::WARN));
            emit_slow_statement();
        });
        assert_eq!(watch.snapshot().kind(WatchKind::SlowStatement).since_start, 1);
        // The log level still hides it from the output.
        assert!(!captured.text().contains("slow statement"));
    }

    #[test]
    fn other_sqlx_query_events_are_not_counted() {
        let (subscriber, _handle, _captured, watch) = test_subscriber("trace");
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "sqlx::query", summary = "select 1", "select 1");
            tracing::warn!(target: "sqlx::query", "statement failed");
            tracing::error!(target: "sqlx::query", "slow statement: but an error");
            tracing::warn!(target: "mybibli", "slow statement: elsewhere");
        });
        assert_eq!(watch.snapshot().kind(WatchKind::SlowStatement).since_start, 0);
    }

    #[test]
    fn a_slow_statement_is_logged_and_counted_under_info() {
        let (subscriber, _handle, captured, watch) = test_subscriber("info");
        tracing::subscriber::with_default(subscriber, emit_slow_statement);
        assert_eq!(watch.snapshot().kind(WatchKind::SlowStatement).since_start, 1);
        let text = captured.text();
        assert!(text.contains("slow statement"), "{text}");
        assert!(text.contains("\"target\":\"sqlx::query\""), "{text}");
    }

    /// AC5 — the operator filter still decides what is written, and a
    /// reload through the handle (the admin "Log level" form) applies.
    #[test]
    fn a_reload_to_warn_drops_info_lines_from_the_output() {
        let (subscriber, handle, captured, _watch) = test_subscriber("info");
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "mybibli", "first info line");
            handle
                .modify(|f| *f = tracing_subscriber::EnvFilter::new("warn"))
                .unwrap();
            tracing::info!(target: "mybibli", "second info line");
            tracing::warn!(target: "mybibli", "a warn line");
        });
        let text = captured.text();
        assert!(text.contains("first info line"), "{text}");
        assert!(!text.contains("second info line"), "{text}");
        assert!(text.contains("a warn line"), "{text}");
        assert!(text.contains("\"level\":\"WARN\""), "{text}");
    }

    #[test]
    fn the_operator_filter_directives_still_apply_per_target() {
        let (subscriber, _handle, captured, _watch) = test_subscriber("warn,mybibli=debug");
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(target: "mybibli", "mybibli debug line");
            tracing::info!(target: "hyper", "hyper info line");
        });
        let text = captured.text();
        assert!(text.contains("mybibli debug line"), "{text}");
        assert!(!text.contains("hyper info line"), "{text}");
    }
}
