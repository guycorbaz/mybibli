//! K10plus metadata provider (CR #450 — v1.16.0).
//!
//! K10plus is the union catalogue of the German GBV/SWB library networks
//! (~260 million holdings, operated by BSZ + VZG). It holds a large share of
//! mainstream anglophone publishing — O'Reilly, Cambridge University Press,
//! Penguin — which is exactly the population the v1.14.1 backfill left
//! zone-less: English-prefix titles the Library of Congress does not carry
//! (measured in #450: LoC recovered 42 % of anglophone candidates; K10plus
//! answered 6 of the 10 sampled leftovers).
//!
//! ## Role in the chain: zones only
//!
//! This provider deliberately serves the **zone-completion pass only**
//! (`supplies_marc_zones` / `lookup_marc_zones`). Its `lookup_by_isbn`
//! returns `Ok(None)` without any network call, so scan behaviour is
//! unchanged: titles, authors and covers keep coming from the existing
//! chain members, and K10plus contributes the six UNIMARC-aligned zones
//! when the winner is missing them. Promoting it to a full lookup source
//! is a separate decision with its own data-quality questions (see the
//! e-book-aggregator noise below) and is NOT part of #450.
//!
//! ## API
//!
//! Anonymous SRU, no key, no registration:
//!
//! ```text
//! GET https://sru.k10plus.de/opac-de-627?version=1.1&operation=searchRetrieve
//!     &query=pica.isb%3D<isbn>&maximumRecords=5&recordSchema=marcxml
//! ```
//!
//! Records are MARC 21 slim XML — the same serialisation the LoC SRU feed
//! uses, so `metadata::marc` reads them unchanged. The catalogue data is
//! published under CC0 (K10plus Open Data statement), the cleanest licence
//! in the whole chain. No rate limit is documented; probes at 1 req/s drew
//! no throttling, and this provider ships a proactive 1 req/s limiter
//! anyway (the MusicBrainz pattern) — the #439 lesson is that SRU servers
//! throttle by dropping connections, not by answering 429, so politeness
//! beats back-off.
//!
//! ## Multi-record responses and e-book noise
//!
//! Unlike LoC (`maximumRecords=1`), K10plus routinely returns several
//! records per ISBN — print + e-book aggregator records side by side. Two
//! consequences, both measured live on 2026-08-14 (ISBN 9780596002701):
//!
//! 1. **Zones are read across ALL returned records** in document order
//!    (first non-empty wins per zone). A print record often carries the
//!    490/250 that the e-book record lacks, and vice versa. The shared
//!    string scanner in `metadata::marc` gives this union naturally.
//! 2. **E-book aggregator records (ProQuest/EBC) pollute `500$a`** with
//!    boilerplate ("Description based upon print version of record") and
//!    multi-kilobyte table-of-contents dumps. `general_note` therefore
//!    takes the first 500$a that is neither that boilerplate nor longer
//!    than [`MAX_GENERAL_NOTE_CHARS`] — a joined ToC blob reads terribly
//!    on the title-detail page, and the length cap kills it cheaply.

use std::sync::Arc;

use async_trait::async_trait;

use crate::models::media_type::MediaType;

use super::marc::{extract_subfield, extract_subfields};
use super::provider::{MetadataError, MetadataProvider, MetadataResult};
use super::rate_limiter::RateLimiter;

/// Longest `500$a` accepted as a general note. Real catalogue notes are one
/// or two sentences; e-book aggregator records dump entire tables of
/// contents into repeated 500 fields, thousands of characters long.
const MAX_GENERAL_NOTE_CHARS: usize = 500;

/// `500$a` boilerplate emitted by e-book aggregator records — a provenance
/// remark about the digitisation, not a bibliographic note.
const GENERAL_NOTE_BOILERPLATE: &str = "Description based upon print version";

/// ISBN prefixes K10plus is consulted for. Anything else returns `None`
/// instantly — no request, no limiter acquisition.
///
/// Rationale (rc.1 → rc.2 fix): the chain's zone-completion pass runs for
/// nearly every scanned book (490/240 are absent from most records, so
/// "all six zones filled" is the rare case). In rc.1 the 1 req/s limiter
/// was exposed through the trait, so the chain acquired it BEFORE every
/// completion call — and with K10plus consulted for every prefix, the
/// background metadata pipeline serialised behind the limiter under
/// parallel load. The release gate caught it: 82 K10plus calls in one E2E
/// run, +58 % suite wall-clock, two metadata-latency journeys timing out.
///
/// The gate keeps only prefixes where K10plus plausibly answers and #450
/// has a stake: 978-0/978-1 (the anglophone leftovers this provider
/// exists for), 978-3 (the German-language area — its home catalogue),
/// and 978-2 (French: the BnF already failed #450's 9782 leftovers, so a
/// free second opinion costs nothing). 979-x stays out pending a
/// measurement that justifies it.
const ISBN_PREFIXES: [&str; 4] = ["9780", "9781", "9782", "9783"];

pub struct K10plusProvider {
    client: reqwest::Client,
    sru_base_url: String,
    limiter: Arc<RateLimiter>,
}

impl K10plusProvider {
    pub fn new(client: reqwest::Client) -> Self {
        let sru_base_url = std::env::var("K10PLUS_SRU_BASE_URL")
            .unwrap_or_else(|_| "https://sru.k10plus.de/opac-de-627".to_string());
        K10plusProvider {
            client,
            sru_base_url,
            limiter: Arc::new(RateLimiter::per_second(1.0)),
        }
    }

    /// Construct with a custom base URL — used by integration tests
    /// pointing at the e2e mock server.
    pub fn with_base_url(client: reqwest::Client, sru_base_url: &str) -> Self {
        K10plusProvider {
            client,
            sru_base_url: sru_base_url.to_string(),
            limiter: Arc::new(RateLimiter::per_second(1.0)),
        }
    }

    /// Read the six zones out of an SRU `searchRetrieve` MARCXML response.
    ///
    /// Returns `None` when the response carries no record, so the caller can
    /// distinguish "K10plus does not hold this ISBN" from "it does, but the
    /// records have none of the zones we map". The scan runs over the whole
    /// body, so with several records the first non-empty value per zone wins
    /// regardless of which record carries it.
    pub fn parse_marcxml_response(body: &str) -> Option<MetadataResult> {
        if !body.contains("numberOfRecords") || body.contains("numberOfRecords>0<") {
            return None;
        }
        if !body.contains("<record") {
            return None;
        }

        // 500$a needs the noise filter described in the module docs; the
        // other five zones are short by nature and take the plain first hit.
        let general_note = extract_subfields(body, "500", "a").into_iter().find(|note| {
            note.chars().count() <= MAX_GENERAL_NOTE_CHARS
                && !note.starts_with(GENERAL_NOTE_BOILERPLATE)
        });

        Some(MetadataResult {
            statement_of_responsibility: extract_subfield(body, "245", "c"),
            edition_statement: extract_subfield(body, "250", "a"),
            collection_title: extract_subfield(body, "490", "a"),
            collection_number: extract_subfield(body, "490", "v"),
            general_note,
            original_title: extract_subfield(body, "240", "a"),
            ..MetadataResult::default()
        })
    }

    async fn fetch_marc_zones(&self, safe_isbn: &str) -> Option<MetadataResult> {
        // Pace ONLY the requests that actually go out. The limiter lives
        // here, after the caller's prefix gate, and deliberately NOT behind
        // the trait's `rate_limiter()` — the chain acquires that one before
        // every completion call, gated or not, which is exactly the rc.1
        // serialisation bug described on ISBN_PREFIXES.
        self.limiter.acquire().await;

        let url = format!(
            "{}?version=1.1&operation=searchRetrieve&query=pica.isb%3D{}&maximumRecords=5&recordSchema=marcxml",
            self.sru_base_url, safe_isbn
        );

        // A single polite attempt: probes never saw the LoC-style dropped
        // connection here, and the 1 req/s limiter (acquired by the chain
        // before this call) keeps the pace civil. If production logs show
        // transport failures, lift the LoC retry loop — measured evidence
        // first, per the #450 method.
        let response = match self
            .client
            .get(&url)
            .header("User-Agent", "mybibli/1.16.0 (https://github.com/guycorbaz/mybibli)")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::info!(isbn = %safe_isbn, error = %e, "K10plus SRU unreachable; continuing without zones");
                return None;
            }
        };
        if !response.status().is_success() {
            tracing::debug!(
                isbn = %safe_isbn,
                status = %response.status(),
                "K10plus SRU returned a non-success status; skipping zones"
            );
            return None;
        }
        let body = match response.text().await {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!(isbn = %safe_isbn, error = %e, "K10plus SRU body read failed");
                return None;
            }
        };

        Self::parse_marcxml_response(&body)
    }
}

#[async_trait]
impl MetadataProvider for K10plusProvider {
    fn name(&self) -> &str {
        "K10plus"
    }

    fn supplies_marc_zones(&self) -> bool {
        true
    }

    async fn lookup_marc_zones(&self, isbn: &str) -> Option<MetadataResult> {
        let safe_isbn: String = isbn.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        if !ISBN_PREFIXES.iter().any(|p| safe_isbn.starts_with(p)) {
            return None;
        }
        self.fetch_marc_zones(&safe_isbn).await
    }

    fn supports_media_type(&self, media_type: &MediaType) -> bool {
        matches!(media_type, MediaType::Book | MediaType::Magazine)
    }

    /// Zones-only provider: the primary lookup answers nothing, with no
    /// network call, so the scan chain's behaviour is untouched (see the
    /// module docs for why this is deliberate).
    async fn lookup_by_isbn(&self, _isbn: &str) -> Result<Option<MetadataResult>, MetadataError> {
        Ok(None)
    }

    // NOTE: no `rate_limiter()` trait impl, on purpose. The chain acquires
    // a trait-exposed limiter before EVERY zone-completion call, including
    // the prefix-gated ones that return instantly — rc.1 shipped it that
    // way and serialised the whole background metadata pipeline under
    // parallel load. Pacing lives inside `fetch_marc_zones`, after the gate.

    fn health_check_url(&self) -> Option<&str> {
        Some("https://sru.k10plus.de/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the live `sru.k10plus.de/opac-de-627` response for ISBN
    /// 9780596002701, captured 2026-08-14. Two records: first an e-book
    /// aggregator record whose 500$a fields are boilerplate + a ToC dump,
    /// then a print record carrying the edition statement and a clean note.
    const SRU_MULTI_RECORD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<zs:searchRetrieveResponse xmlns:zs="http://www.loc.gov/zing/srw/"><zs:version>1.1</zs:version><zs:numberOfRecords>2</zs:numberOfRecords><zs:records><zs:record><zs:recordSchema>marcxml</zs:recordSchema><zs:recordData><record xmlns="http://www.loc.gov/MARC21/slim">
  <datafield tag="245" ind1="1" ind2="0">
    <subfield code="a">Network Security with OpenSSL</subfield>
    <subfield code="c">John Viega, Matt Messier, Pravir Chandra</subfield>
  </datafield>
  <datafield tag="500" ind1=" " ind2=" ">
    <subfield code="a">Description based upon print version of record</subfield>
  </datafield>
  <datafield tag="500" ind1=" " ind2=" ">
    <subfield code="a">Network Security with OpenSSL; Conventions Used in This Book; Comments and Questions; Acknowledgments; 1. Introduction; 1.1.2. Cryptographic Algorithms; 1.1.2.2. Public key encryption; 1.1.2.3. Cryptographic hash functions and Message Authentication Codes; 1.1.2.4. Digital signatures; 1.2. Overview of SSL; 1.3. Problems with SSL; 1.3.1.2. Load balancing; 1.3.2. Keys in the Clear; 1.3.3. Bad Server Credentials; 1.3.4. Certificate Validation; 1.3.5. Poor Entropy; 1.3.6. Insecure Cryptography; 1.4. What SSL Does Not Do Well; 1.4.2. Non-Repudiation; 1.4.3. Protection Against Software Flaws; 1.4.4. General-Purpose Data Security; 1.5. OpenSSL Basics; 1.6. Securing Third-Party Software; 1.6.2. Client-Side Proxies</subfield>
  </datafield>
</record></zs:recordData></zs:record><zs:record><zs:recordSchema>marcxml</zs:recordSchema><zs:recordData><record xmlns="http://www.loc.gov/MARC21/slim">
  <datafield tag="250" ind1=" " ind2=" ">
    <subfield code="a">1st edition.</subfield>
  </datafield>
  <datafield tag="490" ind1="0" ind2=" ">
    <subfield code="a">O'Reilly networking</subfield>
  </datafield>
  <datafield tag="500" ind1=" " ind2=" ">
    <subfield code="a">Includes index.</subfield>
  </datafield>
</record></zs:recordData></zs:record></zs:records></zs:searchRetrieveResponse>"#;

    const SRU_EMPTY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<zs:searchRetrieveResponse xmlns:zs="http://www.loc.gov/zing/srw/"><zs:version>1.1</zs:version><zs:numberOfRecords>0</zs:numberOfRecords><zs:records/></zs:searchRetrieveResponse>"#;

    #[test]
    fn zones_are_unioned_across_records_first_non_empty_wins() {
        let r = K10plusProvider::parse_marcxml_response(SRU_MULTI_RECORD)
            .expect("records are present");
        // From record 1 (e-book):
        assert_eq!(
            r.statement_of_responsibility.as_deref(),
            Some("John Viega, Matt Messier, Pravir Chandra"),
            "245$c"
        );
        // From record 2 (print) — record 1 has neither:
        assert_eq!(r.edition_statement.as_deref(), Some("1st edition."), "250$a");
        assert_eq!(
            r.collection_title.as_deref(),
            Some("O'Reilly networking"),
            "490$a"
        );
        // Absent everywhere — None, not an empty string:
        assert_eq!(r.collection_number, None, "490$v absent");
        assert_eq!(r.original_title, None, "240$a absent");
    }

    #[test]
    fn general_note_skips_ebook_boilerplate_and_toc_dumps() {
        let r = K10plusProvider::parse_marcxml_response(SRU_MULTI_RECORD).unwrap();
        // The first 500$a is provenance boilerplate, the second a >500-char
        // ToC dump; the first ACCEPTABLE note comes from the print record.
        assert_eq!(r.general_note.as_deref(), Some("Includes index."));
    }

    #[test]
    fn all_general_notes_noisy_yields_none_not_garbage() {
        let body = SRU_MULTI_RECORD.replace("Includes index.", "Description based upon print version of an e-book");
        let r = K10plusProvider::parse_marcxml_response(&body).unwrap();
        assert_eq!(r.general_note, None);
    }

    #[test]
    fn zero_records_is_none() {
        assert!(K10plusProvider::parse_marcxml_response(SRU_EMPTY).is_none());
    }

    #[test]
    fn garbage_is_none_not_a_panic() {
        assert!(K10plusProvider::parse_marcxml_response("").is_none());
        assert!(K10plusProvider::parse_marcxml_response("<html>oops</html>").is_none());
        assert!(K10plusProvider::parse_marcxml_response("not xml").is_none());
    }

    #[test]
    fn zones_never_carry_title_or_cover() {
        let r = K10plusProvider::parse_marcxml_response(SRU_MULTI_RECORD).unwrap();
        assert_eq!(r.title, None);
        assert_eq!(r.cover_url, None);
        assert!(r.authors.is_empty());
    }

    #[test]
    fn supports_books_and_magazines_only() {
        let p = K10plusProvider::new(reqwest::Client::new());
        assert!(p.supports_media_type(&MediaType::Book));
        assert!(p.supports_media_type(&MediaType::Magazine));
        assert!(!p.supports_media_type(&MediaType::Bd));
        assert!(!p.supports_media_type(&MediaType::Cd));
        assert!(!p.supports_media_type(&MediaType::Dvd));
    }

    #[test]
    fn provider_declares_zones_but_no_trait_level_limiter() {
        let p = K10plusProvider::new(reqwest::Client::new());
        assert!(p.supplies_marc_zones());
        // rc.2: the limiter is internal (post-prefix-gate). Exposing it via
        // the trait made the chain acquire it before every completion call
        // and serialised the metadata pipeline — this assertion locks the
        // fix against a well-meaning "add the trait impl back" regression.
        assert!(p.rate_limiter().is_none());
        assert_eq!(p.name(), "K10plus");
    }

    #[tokio::test]
    async fn non_covered_prefixes_are_gated_without_a_request() {
        // Unreachable base URL: if the gate leaks, the lookup would still
        // return None here — so assert on timing-free behaviour: covered
        // prefixes attempt (and fail on the dead endpoint), gated ones
        // return instantly. The generated E2E ISBNs (978-6x…978-9x) and
        // 979-x must all be gated.
        let p = K10plusProvider::with_base_url(reqwest::Client::new(), "http://127.0.0.1:1");
        for gated in ["9786684000012", "9788372000064", "9791032900116", "9798350000000"] {
            let start = std::time::Instant::now();
            assert!(p.lookup_marc_zones(gated).await.is_none());
            assert!(
                start.elapsed() < std::time::Duration::from_millis(900),
                "gated prefix {gated} must not pay the limiter or the network"
            );
        }
    }

    #[tokio::test]
    async fn primary_lookup_is_inert() {
        // Zones-only contract: no result and no panic, without any server.
        let p = K10plusProvider::with_base_url(reqwest::Client::new(), "http://127.0.0.1:1");
        let r = p.lookup_by_isbn("9780596002701").await.unwrap();
        assert!(r.is_none());
    }

    #[tokio::test]
    async fn unreachable_sru_degrades_to_no_zones() {
        let p = K10plusProvider::with_base_url(reqwest::Client::new(), "http://127.0.0.1:1");
        assert!(p.lookup_marc_zones("9780596002701").await.is_none());
    }

    #[tokio::test]
    async fn non_alphanumeric_isbn_is_rejected_without_a_request() {
        let p = K10plusProvider::with_base_url(reqwest::Client::new(), "http://127.0.0.1:1");
        assert!(p.lookup_marc_zones("///???").await.is_none());
    }
}
