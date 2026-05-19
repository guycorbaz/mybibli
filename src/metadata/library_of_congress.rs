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

pub struct LibraryOfCongressProvider {
    client: reqwest::Client,
    base_url: String,
}

impl LibraryOfCongressProvider {
    pub fn new(client: reqwest::Client) -> Self {
        let base_url = std::env::var("LOC_API_BASE_URL")
            .unwrap_or_else(|_| "https://www.loc.gov/books/".to_string());
        LibraryOfCongressProvider { client, base_url }
    }

    /// Construct with a custom base URL — used by integration tests
    /// pointing at the e2e mock server.
    pub fn with_base_url(client: reqwest::Client, base_url: &str) -> Self {
        LibraryOfCongressProvider {
            client,
            base_url: base_url.to_string(),
        }
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

        Ok(Self::parse_json_response(&body))
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
}
