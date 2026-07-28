//! Library of Congress metadata provider (CR #263 — v1.5.0).
//!
//! The Library of Congress is the authoritative bibliographic source for
//! anything cataloged by the US national library — particularly useful for
//! older English-language titles, US government publications, and academic
//! works where Google Books / Open Library have gaps.
//!
//! Pairs naturally with [`super::bnf`] (national library for FR) — LoC fills
//! the same role for EN books in the provider chain.
//!
//! ## API
//!
//! Uses the public api.loc.gov JSON search endpoint:
//!
//! ```text
//! GET https://www.loc.gov/books/?q=<isbn>&fo=json&c=1
//! ```
//!
//! No authentication required. The "Loose" community guideline for rate
//! limiting is ~10 req/s per IP; household catalog usage is well below
//! that so v1 ships without a rate limiter.
//!
//! ## Response shape
//!
//! The endpoint returns a JSON object with a `results` array. Each entry
//! carries an inconsistent set of fields because LoC federates across
//! many catalogs (LCCN, MARC, Z39.50…). The fields we depend on:
//!
//! - `title` — single string (sometimes prefixed with the LCCN).
//! - `contributor` — array of strings; one per author / editor / translator.
//! - `date` — string (a year, sometimes a range like "1971-1972").
//! - `language` — array of strings (e.g. `["english"]`).
//! - `description` — array (HTML allowed) — used as the catalog
//!   description when present.
//! - `image_url` — array of cover URLs; LoC has high-quality scans for
//!   older titles.
//!
//! ## What's intentionally NOT extracted in v1
//!
//! - **Publisher** — not surfaced consistently by the search API; would
//!   require a per-record follow-up fetch via the LCCN.
//! - **Dewey code** — sometimes in `call_number` but mixed with LCC; too
//!   unreliable to extract without a structured parser.
//! - **Page count** — not in the search response.
//! - **LCSH subjects** — saved for the classification refactor ([#206]).
//!
//! Any of those become CRs of their own if the user surfaces a gap.

use async_trait::async_trait;
use serde::Deserialize;

use crate::models::media_type::MediaType;

use super::provider::{MetadataError, MetadataProvider, MetadataResult};

/// #439 — how many times to re-issue an SRU request that died at the transport
/// layer, and how long to wait between attempts.
///
/// Measured against the live endpoint on 2026-07-28: at ~0.35 s spacing, 79 of
/// 113 requests failed; at 2 s spacing, 6 of 26 still did. **Every failure was
/// a dropped connection, never an HTTP status** — the server hangs up instead
/// of answering 429, so `MetadataError::RateLimited` (and with it the #419
/// back-off, which keys on 429/503) never fires. Retrying the transport error
/// is the only way to see those records.
const SRU_MAX_ATTEMPTS: u32 = 3;
const SRU_RETRY_BASE_DELAY_MS: u64 = 2000;

pub struct LibraryOfCongressProvider {
    client: reqwest::Client,
    base_url: String,
    sru_base_url: String,
}

impl LibraryOfCongressProvider {
    pub fn new(client: reqwest::Client) -> Self {
        let base_url = std::env::var("LOC_API_BASE_URL")
            .unwrap_or_else(|_| "https://www.loc.gov/books/".to_string());
        let sru_base_url = std::env::var("LOC_SRU_BASE_URL")
            .unwrap_or_else(|_| "http://lx2.loc.gov:210/LCDB".to_string());
        LibraryOfCongressProvider {
            client,
            base_url,
            sru_base_url,
        }
    }

    /// Construct with a custom base URL — used by integration tests
    /// pointing at the e2e mock server.
    ///
    /// The SRU endpoint follows the same base by default so a mock server can
    /// serve both; override it with [`Self::with_sru_base_url`] when they
    /// differ.
    pub fn with_base_url(client: reqwest::Client, base_url: &str) -> Self {
        LibraryOfCongressProvider {
            client,
            base_url: base_url.to_string(),
            sru_base_url: base_url.to_string(),
        }
    }

    pub fn with_sru_base_url(mut self, sru_base_url: &str) -> Self {
        self.sru_base_url = sru_base_url.to_string();
        self
    }

    /// Fetch the MARC 21 record for an ISBN over SRU and read the six
    /// UNIMARC-aligned zones out of it (#439).
    ///
    /// **Supplementary, never fatal.** The flat `?fo=json` search remains the
    /// primary call — it is what supplies title, authors, date, language,
    /// description and, crucially, the cover URL, none of which the MARC record
    /// carries. This second request only adds the structured zones, so any
    /// failure here degrades to "no zones" rather than failing the lookup.
    ///
    /// MARC 21 → internal mapping (semantic equivalents of the UNIMARC zones in
    /// `docs/unimarc-mapping.md`):
    ///
    /// | internal | UNIMARC (BnF) | MARC 21 (LoC) |
    /// |---|---|---|
    /// | statement_of_responsibility | 200$f | 245$c |
    /// | edition_statement | 205$a | 250$a |
    /// | collection_title / number | 225$a / 225$v | 490$a / 490$v |
    /// | general_note | 300$a | 500$a |
    /// | original_title | 500$a | 240$a (uniform title) |
    ///
    /// Note the collision worth keeping in mind when reading both tables: `500`
    /// means "general note" in MARC 21 but "original title" in UNIMARC.
    async fn fetch_marc_zones(&self, safe_isbn: &str) -> Option<MetadataResult> {
        let url = format!(
            "{}?version=1.1&operation=searchRetrieve&query=bath.isbn={}&recordSchema=marcxml&maximumRecords=1",
            self.sru_base_url, safe_isbn
        );

        let mut attempt = 0;
        let body = loop {
            attempt += 1;
            match self
                .client
                .get(&url)
                .header("User-Agent", "mybibli/1.14.0")
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => match response.text().await {
                    Ok(text) => break text,
                    Err(e) => {
                        tracing::debug!(isbn = %safe_isbn, error = %e, "LoC SRU body read failed");
                        return None;
                    }
                },
                Ok(response) => {
                    // A real HTTP status — not the dropped-connection case, so
                    // retrying buys nothing.
                    tracing::debug!(
                        isbn = %safe_isbn,
                        status = %response.status(),
                        "LoC SRU returned a non-success status; skipping zones"
                    );
                    return None;
                }
                Err(e) if attempt < SRU_MAX_ATTEMPTS => {
                    // The dropped-connection case. Linear back-off: the server
                    // is throttling by connection count, so spacing matters
                    // more than exponential growth.
                    let delay = SRU_RETRY_BASE_DELAY_MS * attempt as u64;
                    tracing::debug!(
                        isbn = %safe_isbn,
                        attempt = attempt,
                        delay_ms = delay,
                        error = %e,
                        "LoC SRU transport failure, retrying"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
                Err(e) => {
                    tracing::info!(
                        isbn = %safe_isbn,
                        attempts = attempt,
                        error = %e,
                        "LoC SRU unreachable after retries; continuing without MARC zones"
                    );
                    return None;
                }
            }
        };

        Self::parse_marcxml_response(&body)
    }

    /// Read the six zones out of an SRU `searchRetrieve` MARCXML response.
    ///
    /// Returns `None` when the response carries no record at all, so the caller
    /// can distinguish "LoC does not hold this ISBN" from "it does, but the
    /// record has none of the zones we map".
    pub fn parse_marcxml_response(body: &str) -> Option<MetadataResult> {
        // `numberOfRecords` is namespace-prefixed in the live feed
        // (`<zs:numberOfRecords>`), so match on the local name.
        if !body.contains("numberOfRecords") || body.contains("numberOfRecords>0<") {
            return None;
        }
        if !body.contains("<record") {
            return None;
        }

        use super::marc::extract_subfield;
        Some(MetadataResult {
            statement_of_responsibility: extract_subfield(body, "245", "c"),
            edition_statement: extract_subfield(body, "250", "a"),
            collection_title: extract_subfield(body, "490", "a"),
            collection_number: extract_subfield(body, "490", "v"),
            general_note: extract_subfield(body, "500", "a"),
            original_title: extract_subfield(body, "240", "a"),
            ..MetadataResult::default()
        })
    }

    /// Parse a `?fo=json` response into a [`MetadataResult`]. Returns
    /// `None` when the `results` array is empty.
    ///
    /// Public for unit-test reuse from outside the trait surface.
    pub fn parse_json_response(body: &str) -> Option<MetadataResult> {
        let envelope: LocEnvelope = serde_json::from_str(body).ok()?;
        let first = envelope.results.into_iter().next()?;
        let title = first.title.as_ref().and_then(|t| {
            let trimmed = t.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })?;

        // Language array → first non-empty entry, normalized to a 2-letter
        // ISO code when the response uses the full English name.
        let language = first
            .language
            .iter()
            .flatten()
            .find_map(|l| {
                let lower = l.trim().to_ascii_lowercase();
                if lower.is_empty() {
                    None
                } else {
                    Some(normalize_language(&lower))
                }
            });

        // Description is an array of HTML strings — concatenate with
        // newlines and strip the most common tags. The catalog renders
        // descriptions as plain text so we don't want stray markup.
        let description = first.description.as_ref().and_then(|d| {
            let joined: String = d
                .iter()
                .map(|s| strip_html_tags(s))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            if joined.is_empty() { None } else { Some(joined) }
        });

        let cover_url = first
            .image_url
            .iter()
            .flatten()
            .find(|u| !u.trim().is_empty())
            .cloned();

        let authors: Vec<String> = first
            .contributor
            .into_iter()
            .flatten()
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();

        let publication_date = first.date.and_then(|d| {
            let trimmed = d.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

        Some(MetadataResult {
            title: Some(title),
            authors,
            publication_date,
            language,
            description,
            cover_url,
            ..MetadataResult::default()
        })
    }
}

/// Map LoC's English-language name onto the 2-letter ISO 639-1 code that
/// the rest of mybibli stores. Falls back to the raw value otherwise so
/// downstream code can still see it; an unrecognized value is harmless
/// since the title-save form lets the user override the language.
fn normalize_language(loc_value: &str) -> String {
    match loc_value {
        "english" | "eng" | "en" => "en".to_string(),
        "french" | "fre" | "fra" | "fr" => "fr".to_string(),
        "spanish" | "spa" | "esp" | "es" => "es".to_string(),
        "german" | "ger" | "deu" | "de" => "de".to_string(),
        "italian" | "ita" | "it" => "it".to_string(),
        other => other.to_string(),
    }
}

/// Strip a small set of common HTML tags from a description payload.
/// Not a full HTML parser — just enough to keep the rendered text clean
/// for the four tags LoC's `<p>`, `<br>`, `<i>`, `<b>` markup uses.
fn strip_html_tags(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for c in raw.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            ch if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.trim().to_string()
}

#[async_trait]
impl MetadataProvider for LibraryOfCongressProvider {
    fn name(&self) -> &str {
        "Library of Congress"
    }

    /// #439 — this provider serves structured MARC zones, so the chain's
    /// zone-completion pass may consult it.
    fn supplies_marc_zones(&self) -> bool {
        true
    }

    /// Zones straight from SRU — deliberately independent of the flat JSON
    /// search, which indexes different records and would otherwise veto
    /// perfectly good MARC data (#439).
    async fn lookup_marc_zones(&self, isbn: &str) -> Option<MetadataResult> {
        let safe_isbn: String = isbn.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        if safe_isbn.is_empty() {
            return None;
        }
        self.fetch_marc_zones(&safe_isbn).await
    }

    fn supports_media_type(&self, media_type: &MediaType) -> bool {
        // LoC catalogs books + periodicals. BD / CD / DVD belong to other
        // providers in the chain (BDGest / MusicBrainz / TMDb / OMDb).
        matches!(media_type, MediaType::Book | MediaType::Magazine)
    }

    async fn lookup_by_isbn(&self, isbn: &str) -> Result<Option<MetadataResult>, MetadataError> {
        // Drop everything but ASCII alphanumeric chars — defends against
        // accidental query-string injection from a malformed input.
        let safe_isbn: String = isbn.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        if safe_isbn.is_empty() {
            return Ok(None);
        }

        let url = format!("{}?q={}&fo=json&c=1", self.base_url, safe_isbn);

        tracing::debug!(isbn = %isbn, provider = "Library of Congress", "Looking up ISBN");

        let response = self
            .client
            .get(&url)
            .header("User-Agent", "mybibli/1.5.0")
            .send()
            .await
            .map_err(|e| MetadataError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(MetadataError::Network(format!(
                "Library of Congress API returned status {}",
                response.status()
            )));
        }

        let body = response
            .text()
            .await
            .map_err(|e| MetadataError::Parse(e.to_string()))?;

        // #439 — the JSON search stays primary (it alone carries the cover
        // URL); the SRU MARC record adds the six structured zones on top. Both
        // calls run for every ISBN rather than SRU-on-miss, because the target
        // case is exactly an anglophone title where the JSON answers perfectly
        // well and simply has no structured bibliographic data.
        let zones = self.fetch_marc_zones(&safe_isbn).await;

        Ok(match (Self::parse_json_response(&body), zones) {
            (Some(mut json), Some(marc)) => {
                json.fill_unimarc_zones_from(&marc);
                Some(json)
            }
            (Some(json), None) => Some(json),
            // The JSON search found nothing but the catalog holds a MARC
            // record. Zones alone are not a usable result — there is no title
            // to attach them to — so report the miss honestly.
            (None, _) => None,
        })
    }

    fn health_check_url(&self) -> Option<&str> {
        Some("https://www.loc.gov/")
    }
}

// ─── Wire format ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LocEnvelope {
    #[serde(default)]
    results: Vec<LocResult>,
}

#[derive(Debug, Deserialize)]
struct LocResult {
    // Most LoC fields are nullable / optional in practice. Default makes
    // the deserializer tolerant of missing keys; the parser then filters
    // empty strings + empty arrays on top.
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    contributor: Option<Vec<String>>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    language: Option<Vec<String>>,
    #[serde(default)]
    description: Option<Vec<String>>,
    #[serde(default)]
    image_url: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LOC_RESPONSE: &str = r#"{
      "results": [
        {
          "title": "The Naive and Sentimental Lover",
          "contributor": ["John le Carré"],
          "date": "1971",
          "language": ["english"],
          "description": ["<p>A novel by John le Carré.</p>"],
          "image_url": [
            "https://tile.loc.gov/storage-services/sample-cover.jpg"
          ]
        }
      ]
    }"#;

    #[test]
    fn parses_a_typical_loc_search_hit() {
        let result = LibraryOfCongressProvider::parse_json_response(SAMPLE_LOC_RESPONSE).unwrap();
        assert_eq!(
            result.title.as_deref(),
            Some("The Naive and Sentimental Lover")
        );
        assert_eq!(result.authors, vec!["John le Carré".to_string()]);
        assert_eq!(result.publication_date.as_deref(), Some("1971"));
        assert_eq!(result.language.as_deref(), Some("en"));
        assert_eq!(
            result.description.as_deref(),
            Some("A novel by John le Carré.")
        );
        assert!(result.cover_url.is_some());
    }

    #[test]
    fn empty_results_array_returns_none() {
        let body = r#"{"results": []}"#;
        assert!(LibraryOfCongressProvider::parse_json_response(body).is_none());
    }

    #[test]
    fn empty_title_returns_none() {
        let body = r#"{"results": [{"title": "   ", "contributor": ["X"]}]}"#;
        assert!(LibraryOfCongressProvider::parse_json_response(body).is_none());
    }

    #[test]
    fn missing_optional_fields_does_not_panic() {
        // Title alone — all the other fields are absent.
        let body = r#"{"results": [{"title": "Minimal Title"}]}"#;
        let result = LibraryOfCongressProvider::parse_json_response(body).unwrap();
        assert_eq!(result.title.as_deref(), Some("Minimal Title"));
        assert!(result.authors.is_empty());
        assert!(result.publication_date.is_none());
        assert!(result.language.is_none());
        assert!(result.description.is_none());
        assert!(result.cover_url.is_none());
    }

    #[test]
    fn multiple_contributors_are_preserved_in_order() {
        let body = r#"{"results": [{
          "title": "Co-authored",
          "contributor": ["Alice", "Bob", "Carol"]
        }]}"#;
        let result = LibraryOfCongressProvider::parse_json_response(body).unwrap();
        assert_eq!(result.authors, vec!["Alice", "Bob", "Carol"]);
    }

    #[test]
    fn html_tags_are_stripped_from_description() {
        let body = r#"{"results": [{
          "title": "T",
          "description": ["<p>First <i>italicized</i> paragraph.</p>", "<br>Second."]
        }]}"#;
        let result = LibraryOfCongressProvider::parse_json_response(body).unwrap();
        assert_eq!(
            result.description.as_deref(),
            Some("First italicized paragraph.\n\nSecond.")
        );
    }

    #[test]
    fn language_is_normalized_to_iso_code() {
        for (raw, expected) in [
            ("english", "en"),
            ("eng", "en"),
            ("french", "fr"),
            ("spanish", "es"),
            ("german", "de"),
            ("italian", "it"),
            ("klingon", "klingon"),
        ] {
            let body = format!(
                r#"{{"results":[{{"title":"T","language":["{}"]}}]}}"#,
                raw
            );
            let result = LibraryOfCongressProvider::parse_json_response(&body).unwrap();
            assert_eq!(result.language.as_deref(), Some(expected), "lang={raw}");
        }
    }

    #[test]
    fn supports_books_and_magazines_only() {
        let p = LibraryOfCongressProvider::new(reqwest::Client::new());
        assert!(p.supports_media_type(&MediaType::Book));
        assert!(p.supports_media_type(&MediaType::Magazine));
        assert!(!p.supports_media_type(&MediaType::Bd));
        assert!(!p.supports_media_type(&MediaType::Cd));
        assert!(!p.supports_media_type(&MediaType::Dvd));
    }

    #[test]
    fn provider_name_is_stable() {
        let p = LibraryOfCongressProvider::new(reqwest::Client::new());
        assert_eq!(p.name(), "Library of Congress");
    }

    #[test]
    fn malformed_json_returns_none_not_panic() {
        assert!(LibraryOfCongressProvider::parse_json_response("not json at all").is_none());
        assert!(LibraryOfCongressProvider::parse_json_response("{").is_none());
        assert!(LibraryOfCongressProvider::parse_json_response("").is_none());
    }

    // ─── #439 — SRU MARC 21 zones ─────────────────────────────────────

    /// Trimmed from the live `lx2.loc.gov:210/LCDB` response for ISBN
    /// 9780134685991, captured 2026-07-28. Keeps the `zs:` namespace prefix and
    /// the two repeated 500 fields exactly as the server sends them.
    const SRU_MARCXML: &str = r#"<?xml version="1.0"?>
<zs:searchRetrieveResponse xmlns:zs="http://www.loc.gov/zing/srw/"><zs:version>1.1</zs:version><zs:numberOfRecords>1</zs:numberOfRecords><zs:records><zs:record><zs:recordSchema>marcxml</zs:recordSchema><zs:recordData><record xmlns="http://www.loc.gov/MARC21/slim">
  <datafield tag="245" ind1="1" ind2="0">
    <subfield code="a">Effective Java /</subfield>
    <subfield code="c">Joshua Bloch.</subfield>
  </datafield>
  <datafield tag="250" ind1=" " ind2=" ">
    <subfield code="a">Third edition.</subfield>
  </datafield>
  <datafield tag="500" ind1=" " ind2=" ">
    <subfield code="a">"Updated for Java 9"--Cover.</subfield>
  </datafield>
  <datafield tag="500" ind1=" " ind2=" ">
    <subfield code="a">"Best practices for the Java Platform" --Cover.</subfield>
  </datafield>
</record></zs:recordData></zs:record></zs:records></zs:searchRetrieveResponse>"#;

    /// The shape returned for an ISBN the catalog does not hold.
    const SRU_EMPTY: &str = r#"<?xml version="1.0"?>
<zs:searchRetrieveResponse xmlns:zs="http://www.loc.gov/zing/srw/"><zs:version>1.1</zs:version><zs:numberOfRecords>0</zs:numberOfRecords><zs:records></zs:records></zs:searchRetrieveResponse>"#;

    #[test]
    fn marcxml_maps_the_marc21_tags_onto_the_internal_zones() {
        let r = LibraryOfCongressProvider::parse_marcxml_response(SRU_MARCXML)
            .expect("a record is present");
        assert_eq!(
            r.statement_of_responsibility.as_deref(),
            Some("Joshua Bloch."),
            "245$c"
        );
        assert_eq!(r.edition_statement.as_deref(), Some("Third edition."), "250$a");
        assert_eq!(
            r.general_note.as_deref(),
            Some(r#""Updated for Java 9"--Cover."#),
            "500$a, first of two"
        );
        // Absent from this record — must be None, not an empty string.
        assert_eq!(r.collection_title, None, "490$a absent");
        assert_eq!(r.original_title, None, "240$a absent");
    }

    /// A MARC record carries no cover and no usable description, which is
    /// exactly why the SRU call complements the JSON search instead of
    /// replacing it.
    #[test]
    fn marcxml_never_supplies_a_cover_or_title() {
        let r = LibraryOfCongressProvider::parse_marcxml_response(SRU_MARCXML).unwrap();
        assert_eq!(r.cover_url, None);
        assert_eq!(r.title, None);
    }

    #[test]
    fn marcxml_zero_records_is_none() {
        assert!(LibraryOfCongressProvider::parse_marcxml_response(SRU_EMPTY).is_none());
    }

    #[test]
    fn marcxml_garbage_is_none_not_a_panic() {
        assert!(LibraryOfCongressProvider::parse_marcxml_response("").is_none());
        assert!(LibraryOfCongressProvider::parse_marcxml_response("<html>oops</html>").is_none());
    }

    /// The merge the provider performs internally: JSON supplies the record,
    /// MARC supplies the zones, and neither clobbers the other.
    #[test]
    fn json_result_keeps_its_fields_when_marc_zones_are_merged_in() {
        let mut json = LibraryOfCongressProvider::parse_json_response(SAMPLE_LOC_RESPONSE)
            .expect("sample parses");
        let cover_before = json.cover_url.clone();
        let title_before = json.title.clone();
        assert!(cover_before.is_some(), "fixture must carry a cover");

        let marc = LibraryOfCongressProvider::parse_marcxml_response(SRU_MARCXML).unwrap();
        json.fill_unimarc_zones_from(&marc);

        assert_eq!(json.cover_url, cover_before, "the cover must survive");
        assert_eq!(json.title, title_before, "the title must survive");
        assert_eq!(
            json.statement_of_responsibility.as_deref(),
            Some("Joshua Bloch."),
            "and the zones must land"
        );
    }
}
