use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::models::media_type::MediaType;

use super::rate_limiter::RateLimiter;

/// Result of a metadata lookup from an external provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetadataResult {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
    pub cover_url: Option<String>,
    pub language: Option<String>,
    pub page_count: Option<i32>,
    pub track_count: Option<i32>,
    pub total_duration: Option<String>,
    pub age_rating: Option<String>,
    pub issue_number: Option<String>,
    pub dewey_code: Option<String>,
    // #389 Palier 1 — UNIMARC pragmatic-subset zones.
    pub statement_of_responsibility: Option<String>,
    pub edition_statement: Option<String>,
    pub collection_title: Option<String>,
    pub collection_number: Option<String>,
    pub general_note: Option<String>,
    pub original_title: Option<String>,
}

/// Trim a metadata string and treat the empty / whitespace-only result
/// as `None`. #23 sub-item 7: pre-fix providers occasionally produced
/// `Some("".to_string())` for missing-but-emitted fields (e.g. an empty
/// `<dc:subject>` in a SRU response), and the downstream merge then
/// COALESCEd that empty string into the DB column — visually identical
/// to NULL but distinct in queries and silently displacing
/// `manually_edited_fields`-protected values.
pub fn normalize_empty(s: Option<String>) -> Option<String> {
    s.and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else if trimmed.len() == v.len() {
            // Avoid the allocation when the input is already trimmed.
            Some(v)
        } else {
            Some(trimmed.to_string())
        }
    })
}

impl MetadataResult {
    /// Drop empty / whitespace-only strings on every `Option<String>`
    /// field and filter empty entries from `authors`. Providers should
    /// call this right before returning so the downstream merge never
    /// sees an empty string masquerading as a real value. #23 sub-item 7.
    pub fn normalize_empty_strings(mut self) -> Self {
        self.title = normalize_empty(self.title);
        self.subtitle = normalize_empty(self.subtitle);
        self.description = normalize_empty(self.description);
        self.publisher = normalize_empty(self.publisher);
        self.publication_date = normalize_empty(self.publication_date);
        self.cover_url = normalize_empty(self.cover_url);
        self.language = normalize_empty(self.language);
        self.total_duration = normalize_empty(self.total_duration);
        self.age_rating = normalize_empty(self.age_rating);
        self.issue_number = normalize_empty(self.issue_number);
        self.dewey_code = normalize_empty(self.dewey_code);
        self.statement_of_responsibility = normalize_empty(self.statement_of_responsibility);
        self.edition_statement = normalize_empty(self.edition_statement);
        self.collection_title = normalize_empty(self.collection_title);
        self.collection_number = normalize_empty(self.collection_number);
        self.general_note = normalize_empty(self.general_note);
        self.original_title = normalize_empty(self.original_title);
        self.authors = self
            .authors
            .into_iter()
            .filter_map(|a| normalize_empty(Some(a)))
            .collect();
        self
    }

    /// Do any of the six UNIMARC-aligned zones still have no value? (#439)
    ///
    /// Drives the chain's zone-completion pass: a result that already carries
    /// all six needs no second provider consulted.
    pub fn has_missing_unimarc_zones(&self) -> bool {
        self.statement_of_responsibility.is_none()
            || self.edition_statement.is_none()
            || self.collection_title.is_none()
            || self.collection_number.is_none()
            || self.general_note.is_none()
            || self.original_title.is_none()
    }

    /// How many of the six UNIMARC-aligned zones currently carry a value (#439).
    ///
    /// Used by the chain to report what a completion pass actually contributed.
    /// A boolean "are any missing" is useless for that: 490 (collection) and
    /// 240 (uniform title) are absent from most records, so a log gated on
    /// "all six now filled" would stay silent even on a pass that recovered
    /// two zones.
    pub fn filled_unimarc_zone_count(&self) -> usize {
        [
            &self.statement_of_responsibility,
            &self.edition_statement,
            &self.collection_title,
            &self.collection_number,
            &self.general_note,
            &self.original_title,
        ]
        .iter()
        .filter(|z| z.is_some())
        .count()
    }

    /// Fill this result's *empty* UNIMARC zones from `other`, leaving every
    /// value already present untouched (#439).
    ///
    /// This encodes the settled merge policy: field-by-field, **first source
    /// wins**. Because the chain consults BnF before the Library of Congress
    /// for books, that means BnF takes precedence where both answer, while a
    /// French title the BnF only partly described can still have its remaining
    /// zones completed from LoC. Provenance therefore becomes mixed within a
    /// single record, which is accepted.
    pub fn fill_unimarc_zones_from(&mut self, other: &MetadataResult) {
        fn fill(target: &mut Option<String>, source: &Option<String>) {
            if target.is_none()
                && let Some(v) = source
                && !v.trim().is_empty()
            {
                *target = Some(v.clone());
            }
        }
        fill(
            &mut self.statement_of_responsibility,
            &other.statement_of_responsibility,
        );
        fill(&mut self.edition_statement, &other.edition_statement);
        fill(&mut self.collection_title, &other.collection_title);
        fill(&mut self.collection_number, &other.collection_number);
        fill(&mut self.general_note, &other.general_note);
        fill(&mut self.original_title, &other.original_title);
    }
}

/// CR #396 — normalize a provider's display name into the slug used as
/// the K/V settings suffix (`provider_timeout.<slug>`) and the admin
/// form field suffix. Lowercases ASCII and collapses every
/// non-alphanumeric run into a single `_`, trimming leading/trailing
/// separators: `"BnF"` → `"bnf"`, `"Library of Congress"` →
/// `"library_of_congress"`. Must stay in sync with the row keys seeded
/// by migration 20260611000000.
pub fn provider_slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut last_was_sep = true; // suppress a leading separator
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('_');
            last_was_sep = true;
        }
    }
    if slug.ends_with('_') {
        slug.pop();
    }
    slug
}

/// Trait for external metadata providers (BnF, Google Books, Open Library, etc.).
#[async_trait]
pub trait MetadataProvider: Send + Sync {
    /// Human-readable name of the provider.
    fn name(&self) -> &str;

    /// Whether this provider supports the given media type.
    fn supports_media_type(&self, media_type: &MediaType) -> bool;

    /// Look up metadata by ISBN-13.
    async fn lookup_by_isbn(&self, isbn: &str) -> Result<Option<MetadataResult>, MetadataError>;

    /// Look up metadata by UPC barcode.
    async fn lookup_by_upc(&self, _upc: &str) -> Result<Option<MetadataResult>, MetadataError> {
        Ok(None) // Default: not supported
    }

    /// Search by title string.
    async fn search_by_title(&self, _title: &str) -> Result<Option<MetadataResult>, MetadataError> {
        Ok(None) // Default: not supported
    }

    /// Does this provider serve structured MARC bibliographic zones? (#439)
    ///
    /// Only the two national-library providers do — BnF over UNIMARC and the
    /// Library of Congress over MARC 21. Everything else (Google Books, Open
    /// Library, MusicBrainz, OMDb, TMDb, Comic Vine) returns flat metadata with
    /// no statement of responsibility, edition statement, collection or
    /// uniform title.
    ///
    /// The chain's zone-completion pass consults **only** providers that return
    /// `true` here. Without this gate the pass would sweep the entire chain on
    /// almost every lookup: `has_missing_unimarc_zones` is true whenever any of
    /// the six is empty, and 490 (collection) and 240 (uniform title) are
    /// legitimately absent from most records — so "all six present" is the rare
    /// case, not the common one.
    fn supplies_marc_zones(&self) -> bool {
        false
    }

    /// Fetch **only** the six MARC zones for an ISBN, for the chain's
    /// zone-completion pass (#439).
    ///
    /// Separate from [`Self::lookup_by_isbn`] on purpose. The LoC provider's
    /// normal lookup returns `None` when its flat JSON search finds nothing,
    /// because zones with no title are not a usable primary result — but for
    /// completion they are exactly what is wanted, and the JSON search and the
    /// SRU catalogue are different indexes that do not always agree. Routing
    /// completion through `lookup_by_isbn` would therefore silently discard
    /// recoverable zones whenever the two disagree.
    ///
    /// Only providers whose [`Self::supplies_marc_zones`] is true need
    /// implement this; the chain uses that as the cheap gate before calling.
    /// The returned result is read for its six zone fields and nothing else.
    async fn lookup_marc_zones(&self, _isbn: &str) -> Option<MetadataResult> {
        None
    }

    /// Return the rate limiter for this provider, if any.
    /// ChainExecutor calls `acquire()` before each lookup when present.
    fn rate_limiter(&self) -> Option<Arc<RateLimiter>> {
        None // Default: no rate limiting
    }

    /// Canonical homepage URL used by the background provider-health task
    /// (story 8-1). Pings hit a cheap HEAD-able endpoint that does NOT count
    /// against API quotas. Providers without a reachable public URL return
    /// `None` and render as "n/a" in the Admin → Health tab.
    fn health_check_url(&self) -> Option<&str> {
        None
    }
}

/// Errors that can occur during metadata lookup.
#[derive(Debug)]
pub enum MetadataError {
    Network(String),
    Parse(String),
    Timeout,
    /// #23 — typed rate-limit signal. Providers that detect HTTP 429
    /// `TOO_MANY_REQUESTS` MUST return this variant rather than wrapping
    /// the 429 status into a `Network(String)` message containing the
    /// literal "429". The chain in `metadata/chain.rs` matches on this
    /// variant for cleaner log triage (`provider_rate_limited` field on
    /// the tracing span) and so a future "back off and retry the next
    /// provider in the chain" policy has a structured signal to gate on.
    RateLimited,
    /// #419 — typed transient-unavailability signal (HTTP 503). Google
    /// Books answers a request burst with 503 Service Unavailable
    /// storms rather than 429; providers that detect 503 MUST return
    /// this variant so the bulk cover-refetch loop can back off and
    /// retry instead of burning the title's only chance for the run.
    Unavailable,
}

impl MetadataError {
    /// #419 — true for the transient throttle signals (429 / 503) that
    /// the bulk cover-refetch loop retries with backoff. Timeouts and
    /// generic network errors are NOT throttles: retrying them
    /// back-to-back against a struggling provider makes things worse.
    pub fn is_throttle(&self) -> bool {
        matches!(
            self,
            MetadataError::RateLimited | MetadataError::Unavailable
        )
    }
}

impl std::fmt::Display for MetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetadataError::Network(msg) => write!(f, "Network error: {msg}"),
            MetadataError::Parse(msg) => write!(f, "Parse error: {msg}"),
            MetadataError::Timeout => write!(f, "Request timed out"),
            MetadataError::RateLimited => {
                write!(f, "Rate limited by provider (HTTP 429)")
            }
            MetadataError::Unavailable => {
                write!(f, "Provider temporarily unavailable (HTTP 503)")
            }
        }
    }
}

impl std::error::Error for MetadataError {}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── #23 — typed RateLimited + normalize_empty_strings ──────────

    #[test]
    fn metadata_error_rate_limited_display() {
        let e = MetadataError::RateLimited;
        let s = e.to_string();
        assert!(s.contains("Rate limited"), "got {s:?}");
        // The pre-#23 chain matched `err_str.contains("429")`. The new
        // Display still includes 429 so logs stay grep-friendly.
        assert!(s.contains("429"), "Display should still mention HTTP 429 for log grep: got {s:?}");
    }

    /// #419 — Unavailable (503) mirrors the RateLimited (429) contract:
    /// grep-friendly Display, and both classify as transient throttles;
    /// timeouts and generic network errors do not.
    #[test]
    fn metadata_error_unavailable_display_and_throttle_classification() {
        let s = MetadataError::Unavailable.to_string();
        assert!(s.contains("503"), "Display should mention HTTP 503 for log grep: got {s:?}");

        assert!(MetadataError::RateLimited.is_throttle());
        assert!(MetadataError::Unavailable.is_throttle());
        assert!(!MetadataError::Timeout.is_throttle());
        assert!(!MetadataError::Network("connection refused".into()).is_throttle());
        assert!(!MetadataError::Parse("bad json".into()).is_throttle());
    }

    #[test]
    fn normalize_empty_string_helper_strips_empties() {
        assert_eq!(normalize_empty(None), None);
        assert_eq!(normalize_empty(Some(String::new())), None);
        assert_eq!(normalize_empty(Some("   ".to_string())), None);
        assert_eq!(normalize_empty(Some("\t\n".to_string())), None);
        assert_eq!(
            normalize_empty(Some("  Gallimard  ".to_string())),
            Some("Gallimard".to_string()),
        );
        assert_eq!(
            normalize_empty(Some("Gallimard".to_string())),
            Some("Gallimard".to_string()),
        );
    }

    #[test]
    fn metadata_result_normalize_drops_empty_optional_strings() {
        // Mix of empty / whitespace / real values across every
        // Option<String> field. The normalized result must keep the real
        // values and turn every empty/whitespace one into None.
        let result = MetadataResult {
            title: Some("Real title".to_string()),
            subtitle: Some(String::new()),
            description: Some("   ".to_string()),
            authors: vec![
                "Real author".to_string(),
                String::new(),
                "  ".to_string(),
                "  Trimmed  ".to_string(),
            ],
            publisher: Some(String::new()),
            publication_date: Some("\t".to_string()),
            cover_url: Some("https://example.com/cover.jpg".to_string()),
            language: Some(String::new()),
            page_count: Some(123), // u32 fields are untouched
            total_duration: Some(String::new()),
            age_rating: Some(String::new()),
            issue_number: Some(String::new()),
            dewey_code: Some(String::new()),
            ..MetadataResult::default()
        }
        .normalize_empty_strings();

        assert_eq!(result.title.as_deref(), Some("Real title"));
        assert!(result.subtitle.is_none());
        assert!(result.description.is_none());
        assert_eq!(
            result.authors,
            vec!["Real author".to_string(), "Trimmed".to_string()],
            "empty authors stripped, whitespace trimmed",
        );
        assert!(result.publisher.is_none());
        assert!(result.publication_date.is_none());
        assert_eq!(
            result.cover_url.as_deref(),
            Some("https://example.com/cover.jpg"),
            "real URL must survive",
        );
        assert!(result.language.is_none());
        assert_eq!(result.page_count, Some(123), "numeric fields untouched");
        assert!(result.total_duration.is_none());
        assert!(result.age_rating.is_none());
        assert!(result.issue_number.is_none());
        assert!(result.dewey_code.is_none());
    }

    #[test]
    fn test_metadata_result_default() {
        let result = MetadataResult::default();
        assert!(result.title.is_none());
        assert!(result.authors.is_empty());
        assert!(result.publisher.is_none());
    }

    #[test]
    fn test_metadata_result_construction() {
        let result = MetadataResult {
            title: Some("L'Ecume des jours".to_string()),
            subtitle: None,
            description: Some("A novel by Boris Vian".to_string()),
            authors: vec!["Boris Vian".to_string()],
            publisher: Some("Gallimard".to_string()),
            publication_date: Some("1947".to_string()),
            cover_url: None,
            language: Some("fr".to_string()),
            page_count: Some(235),
            ..MetadataResult::default()
        };
        assert_eq!(result.title.as_deref(), Some("L'Ecume des jours"));
        assert_eq!(result.authors.len(), 1);
        assert_eq!(result.authors[0], "Boris Vian");
        assert_eq!(result.publisher.as_deref(), Some("Gallimard"));
        assert_eq!(result.page_count, Some(235));
    }

    #[test]
    fn test_metadata_result_multiple_authors() {
        let result = MetadataResult {
            authors: vec!["Author A".to_string(), "Author B".to_string()],
            ..MetadataResult::default()
        };
        assert_eq!(result.authors.len(), 2);
    }

    #[test]
    fn test_metadata_error_display() {
        assert_eq!(
            MetadataError::Network("connection refused".to_string()).to_string(),
            "Network error: connection refused"
        );
        assert_eq!(
            MetadataError::Parse("invalid JSON".to_string()).to_string(),
            "Parse error: invalid JSON"
        );
        assert_eq!(MetadataError::Timeout.to_string(), "Request timed out");
    }

    // ─── CR #396 — provider_slug ─────────────────────────────────

    #[test]
    fn provider_slug_matches_seeded_row_keys() {
        // One assertion per provider registered in main.rs — must stay
        // in sync with migration 20260611000000's row keys.
        assert_eq!(provider_slug("bdgest"), "bdgest");
        assert_eq!(provider_slug("BnF"), "bnf");
        assert_eq!(provider_slug("google_books"), "google_books");
        assert_eq!(provider_slug("Library of Congress"), "library_of_congress");
        assert_eq!(provider_slug("open_library"), "open_library");
        assert_eq!(provider_slug("musicbrainz"), "musicbrainz");
        assert_eq!(provider_slug("omdb"), "omdb");
        assert_eq!(provider_slug("tmdb"), "tmdb");
    }

    #[test]
    fn provider_slug_collapses_separator_runs_and_trims() {
        assert_eq!(provider_slug("  A -- B  "), "a_b");
        assert_eq!(provider_slug("Comic Vine!"), "comic_vine");
        assert_eq!(provider_slug(""), "");
        assert_eq!(provider_slug("---"), "");
    }
}
