//! Library of Congress metadata provider (CR #263 — v1.5.0; SRU-only since
//! #495 — v1.20.1).
//!
//! The Library of Congress is the authoritative bibliographic source for
//! anything cataloged by the US national library — particularly useful for
//! older English-language titles, US government publications, and academic
//! works where Google Books / Open Library have gaps.
//!
//! Pairs naturally with [`super::bnf`] (national library for FR) — LoC fills
//! the same role for EN books in the provider chain.
//!
//! ## API: SRU / MARCXML only
//!
//! ```text
//! GET http://lx2.loc.gov:210/LCDB?version=1.1&operation=searchRetrieve
//!     &query=bath.isbn=<isbn>&recordSchema=marcxml&maximumRecords=1
//! ```
//!
//! No authentication. The base URL is overridable with `LOC_SRU_BASE_URL`
//! (the E2E stack points it at the mock server).
//!
//! Until v1.20.0 the primary call was the JSON search
//! `https://www.loc.gov/books/?q=<isbn>&fo=json&c=1`, with SRU adding the six
//! UNIMARC-aligned zones. Since 2026-10-26 `www.loc.gov` answers every
//! server-side request with a Cloudflare bot challenge (`403` +
//! `cf-mitigated: challenge`, #495), so the JSON path is gone: one SRU request
//! now supplies the whole description. No attempt is made to pass a
//! challenge — a `cf-mitigated: challenge` answer is logged as such and the
//! chain moves on.
//!
//! ## MARC 21 → mybibli
//!
//! | field | MARC 21 |
//! |---|---|
//! | title / subtitle | 245$a / 245$b |
//! | authors | 100$a, then every 700$a without a `$t` (inverted to "Forename Surname") |
//! | publisher | 264$b (publication, `ind2="1"`, preferred), else 260$b |
//! | publication date | 008/07–10, else the first four-digit run of 264$c / 260$c |
//! | language | 008/35–37 |
//! | page count | 300$a |
//! | description | 520$a |
//! | Dewey | 082$a (segmentation `/` removed) |
//! | six zones | 245$c, 250$a, 490$a, 490$v, 500$a, 240$a — see [`LibraryOfCongressProvider::parse_marcxml_response`] |
//!
//! Every value is XML-entity-decoded and NFC-normalised (the feed encodes
//! accents as decomposed numeric references) and, for the descriptive fields,
//! stripped of ISBD trailing punctuation. The record carries no cover URL;
//! the chain's cover-only fallbacks (`services::cover`) cover that.

use async_trait::async_trait;

use crate::models::media_type::MediaType;

use super::marc::{
    controlfield, datafields, decode_xml_text, extract_subfield, page_count_from_extent,
    trim_isbd_punctuation,
};
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
///
/// #495 — now that SRU is the primary lookup, the chain's per-provider budget
/// (5 s by default) bounds this loop: a second attempt fits only when the
/// first connection drops quickly, a third never does, and the chain then
/// records the provider as timed out rather than unreachable. Kept as is on
/// purpose: the #439 measurement is what sized the 2 s spacing, and an admin
/// can raise `provider_timeout.library_of_congress`.
const SRU_MAX_ATTEMPTS: u32 = 3;
const SRU_RETRY_BASE_DELAY_MS: u64 = 2000;

/// Production SRU endpoint; `LOC_SRU_BASE_URL` overrides it.
const SRU_BASE_URL_DEFAULT: &str = "http://lx2.loc.gov:210/LCDB";

/// #495 — identification sent on every LoC request: the running version plus
/// the project URL, as a good-citizen contact. Built at compile time so it can
/// never go stale again (it read `mybibli/1.5.0` and `mybibli/1.14.0` until
/// v1.20.0). Not a challenge workaround.
pub const USER_AGENT: &str = concat!(
    "mybibli/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/guycorbaz/mybibli)"
);

pub struct LibraryOfCongressProvider {
    client: reqwest::Client,
    sru_base_url: String,
}

/// Why an SRU request produced no body. Each variant has already been logged
/// by [`LibraryOfCongressProvider::fetch_sru`] when it is returned.
#[derive(Debug, PartialEq)]
enum SruFailure {
    /// `cf-mitigated: challenge` — Cloudflare served a bot challenge (#495).
    Challenge(u16),
    /// Any other non-success HTTP status.
    Status(u16),
    /// Connection-level failure after every retry.
    Transport(String),
    /// The body could not be read.
    Body(String),
}

impl From<SruFailure> for MetadataError {
    fn from(f: SruFailure) -> Self {
        match f {
            SruFailure::Challenge(status) => MetadataError::Network(format!(
                "Library of Congress answered with a Cloudflare challenge (cf-mitigated: challenge, status {status})"
            )),
            SruFailure::Status(429) => MetadataError::RateLimited,
            SruFailure::Status(503) => MetadataError::Unavailable,
            SruFailure::Status(status) => {
                MetadataError::Network(format!("Library of Congress SRU returned status {status}"))
            }
            SruFailure::Transport(e) => MetadataError::Network(e),
            SruFailure::Body(e) => MetadataError::Parse(e),
        }
    }
}

/// Does this response carry Cloudflare's `cf-mitigated: challenge` marker?
///
/// Read from the headers only, whatever the status (Cloudflare uses 403, but
/// a challenge can ride on other codes). Case-insensitive on the value.
pub fn is_cloudflare_challenge(headers: &reqwest::header::HeaderMap) -> bool {
    headers.get_all("cf-mitigated").iter().any(|v| {
        v.to_str()
            .map(|s| s.trim().eq_ignore_ascii_case("challenge"))
            .unwrap_or(false)
    })
}

/// The first MARC `<record …>` element of an SRU response, if any.
///
/// `maximumRecords=1` makes this the only one in practice; slicing keeps a
/// misbehaving server from blending two records. Matches `<record ` / `<record>`
/// only, never `<zs:recordData>` and friends.
fn first_record(body: &str) -> Option<&str> {
    if !body.contains("numberOfRecords") || body.contains("numberOfRecords>0<") {
        return None;
    }
    let mut from = 0;
    let start = loop {
        let pos = from + body[from..].find("<record")?;
        match body[pos + "<record".len()..].chars().next() {
            Some(' ') | Some('>') | Some('\n') | Some('\t') | Some('\r') => break pos,
            _ => from = pos + 1,
        }
    };
    let rest = &body[start..];
    Some(match rest.find("</record>") {
        Some(end) => &rest[..end],
        None => rest,
    })
}

/// Decoded, NFC-normalised value; `None` when empty.
fn decoded(raw: Option<String>) -> Option<String> {
    raw.map(|v| decode_xml_text(&v).trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Decoded value with ISBD trailing punctuation removed; `None` when empty.
fn descriptive(raw: Option<String>) -> Option<String> {
    decoded(raw)
        .map(|v| trim_isbd_punctuation(&v))
        .filter(|v| !v.is_empty())
}

/// `"Bloch, Joshua"` → `"Joshua Bloch"` (the BnF provider's convention). Only
/// a name with exactly one `", "` is inverted; anything else is kept as is.
fn invert_name(name: &str) -> String {
    let mut parts = name.split(", ");
    match (parts.next(), parts.next(), parts.next()) {
        (Some(surname), Some(forename), None) if !surname.is_empty() && !forename.is_empty() => {
            format!("{forename} {surname}")
        }
        _ => name.to_string(),
    }
}

/// First run of four ASCII digits — `"[2018]"`, `"c2010."`, `"©2018"` → year.
fn first_year(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    bytes
        .windows(4)
        .position(|w| w.iter().all(u8::is_ascii_digit))
        .map(|i| s[i..i + 4].to_string())
}

impl LibraryOfCongressProvider {
    pub fn new(client: reqwest::Client) -> Self {
        let sru_base_url =
            std::env::var("LOC_SRU_BASE_URL").unwrap_or_else(|_| SRU_BASE_URL_DEFAULT.to_string());
        LibraryOfCongressProvider {
            client,
            sru_base_url,
        }
    }

    /// Construct with a custom SRU base URL — used by tests pointing at a mock
    /// server (the same endpoint `LOC_SRU_BASE_URL` sets in production code).
    pub fn with_base_url(client: reqwest::Client, sru_base_url: &str) -> Self {
        LibraryOfCongressProvider {
            client,
            sru_base_url: sru_base_url.to_string(),
        }
    }

    /// One SRU `searchRetrieve` for an ISBN, shared by the primary lookup and
    /// the zone-completion entry point.
    ///
    /// Logging contract (#495): a Cloudflare challenge and any other
    /// non-success status are logged at `warn`, with distinct messages, and
    /// are never retried; transport failures keep the #439 retry loop.
    async fn fetch_sru(&self, safe_isbn: &str) -> Result<String, SruFailure> {
        let url = format!(
            "{}?version=1.1&operation=searchRetrieve&query=bath.isbn={}&recordSchema=marcxml&maximumRecords=1",
            self.sru_base_url, safe_isbn
        );

        let mut attempt = 0;
        loop {
            attempt += 1;
            match self
                .client
                .get(&url)
                .header(reqwest::header::USER_AGENT, USER_AGENT)
                .send()
                .await
            {
                Ok(response) if is_cloudflare_challenge(response.headers()) => {
                    let status = response.status().as_u16();
                    tracing::warn!(
                        isbn = %safe_isbn,
                        status = status,
                        "Library of Congress answered with a Cloudflare challenge (cf-mitigated: challenge); not retried, moving on"
                    );
                    return Err(SruFailure::Challenge(status));
                }
                Ok(response) if response.status().is_success() => {
                    return response.text().await.map_err(|e| {
                        tracing::warn!(isbn = %safe_isbn, error = %e, "Library of Congress SRU body read failed");
                        SruFailure::Body(e.to_string())
                    });
                }
                Ok(response) => {
                    // A real HTTP status — not the dropped-connection case, so
                    // retrying buys nothing.
                    let status = response.status().as_u16();
                    tracing::warn!(
                        isbn = %safe_isbn,
                        status = status,
                        "Library of Congress SRU returned a non-success status"
                    );
                    return Err(SruFailure::Status(status));
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
                        "LoC SRU unreachable after retries"
                    );
                    return Err(SruFailure::Transport(e.to_string()));
                }
            }
        }
    }

    /// The six UNIMARC-aligned zones of one MARC 21 record (#439).
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
    fn zones_of(record: &str) -> MetadataResult {
        MetadataResult {
            statement_of_responsibility: decoded(extract_subfield(record, "245", "c")),
            edition_statement: decoded(extract_subfield(record, "250", "a")),
            collection_title: decoded(extract_subfield(record, "490", "a")),
            collection_number: decoded(extract_subfield(record, "490", "v")),
            general_note: decoded(extract_subfield(record, "500", "a")),
            original_title: decoded(extract_subfield(record, "240", "a")),
            ..MetadataResult::default()
        }
    }

    /// Read the six zones out of an SRU `searchRetrieve` MARCXML response —
    /// the zone-completion contract (#439), unchanged by #495.
    ///
    /// Returns `None` when the response carries no record at all, so the caller
    /// can distinguish "LoC does not hold this ISBN" from "it does, but the
    /// record has none of the zones we map".
    pub fn parse_marcxml_response(body: &str) -> Option<MetadataResult> {
        first_record(body).map(Self::zones_of)
    }

    /// Read a full description out of an SRU `searchRetrieve` MARCXML response
    /// (#495). `None` when there is no record or the record has no title
    /// (245$a) — a description without a title is not a usable result.
    pub fn parse_marcxml_record(body: &str) -> Option<MetadataResult> {
        let record = first_record(body)?;
        let title = descriptive(extract_subfield(record, "245", "a"))?;

        let mut authors: Vec<String> = Vec::new();
        for tag in ["100", "700"] {
            for df in datafields(record, tag) {
                // A 700 with $t is a name-title added entry: a related work
                // (e.g. "Shakespeare, William. Hamlet"), not a contributor.
                if tag == "700" && extract_subfield(df, tag, "t").is_some() {
                    continue;
                }
                if let Some(name) = descriptive(extract_subfield(df, tag, "a")) {
                    let name = invert_name(&name);
                    if !authors.contains(&name) {
                        authors.push(name);
                    }
                }
            }
        }

        // Publication statement: 264 with ind2="1" is the publication proper
        // (ind2 0/2/3/4 are production, distribution, manufacture, copyright);
        // 260 is the pre-RDA field.
        let field_264: Vec<&str> = datafields(record, "264");
        // Without an ind2="1" field, never fall back on a copyright notice
        // (ind2="4"): its $c is a ©-date and it has no $b.
        let publication = field_264
            .iter()
            .find(|df| df.contains(r#"ind2="1""#))
            .or_else(|| field_264.iter().find(|df| !df.contains(r#"ind2="4""#)))
            .copied();
        let publisher = publication
            .and_then(|df| descriptive(extract_subfield(df, "264", "b")))
            .or_else(|| descriptive(extract_subfield(record, "260", "b")));

        let fixed = controlfield(record, "008");
        let publication_date = fixed
            .as_deref()
            .and_then(|f| f.get(7..11))
            .filter(|y| y.bytes().all(|b| b.is_ascii_digit()))
            .map(str::to_string)
            .or_else(|| {
                publication
                    .and_then(|df| decoded(extract_subfield(df, "264", "c")))
                    .or_else(|| decoded(extract_subfield(record, "260", "c")))
                    .and_then(|c| first_year(&c))
            });

        let language = fixed
            .as_deref()
            .and_then(|f| f.get(35..38))
            .map(|l| l.trim().to_ascii_lowercase())
            .filter(|l| {
                l.len() == 3
                    && l.bytes().all(|b| b.is_ascii_alphabetic())
                    && !matches!(l.as_str(), "und" | "zxx" | "mul")
            })
            .map(|l| normalize_language(&l));

        let page_count = decoded(extract_subfield(record, "300", "a"))
            .and_then(|extent| page_count_from_extent(&extent));

        let dewey_code = decoded(extract_subfield(record, "082", "a"))
            .map(|d| d.replace('/', "").trim().to_string())
            .filter(|d| !d.is_empty());

        Some(MetadataResult {
            title: Some(title),
            subtitle: descriptive(extract_subfield(record, "245", "b")),
            authors,
            publisher,
            publication_date,
            language,
            page_count,
            description: decoded(extract_subfield(record, "520", "a")),
            dewey_code,
            ..Self::zones_of(record)
        })
    }
}

/// Map LoC's language value onto the 2-letter ISO 639-1 code that the rest of
/// mybibli stores. Falls back to the raw value otherwise so downstream code can
/// still see it; an unrecognized value is harmless since the title-save form
/// lets the user override the language.
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

    /// #495 — its zones ride on the same request as its normal lookup, so a
    /// primary miss in this run settles the zone question too.
    fn zones_come_with_lookup(&self) -> bool {
        true
    }

    /// Zones only, for the chain's completion pass when another provider won.
    /// Any failure (already logged by `fetch_sru`) degrades to "no zones".
    async fn lookup_marc_zones(&self, isbn: &str) -> Option<MetadataResult> {
        let safe_isbn: String = isbn.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        if safe_isbn.is_empty() {
            return None;
        }
        let body = self.fetch_sru(&safe_isbn).await.ok()?;
        Self::parse_marcxml_response(&body)
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

        tracing::debug!(isbn = %isbn, provider = "Library of Congress", "Looking up ISBN");

        // #495 — one SRU request supplies the description AND the six zones;
        // the challenged `www.loc.gov/books/?fo=json` search is gone.
        let body = self.fetch_sru(&safe_isbn).await?;
        // A 200 that is not an SRU answer (an HTML page from a proxy or a
        // challenge without the marker header, an SRU diagnostic) is a
        // failure of the source, not "LoC does not hold this ISBN".
        if !body.contains("numberOfRecords") {
            tracing::warn!(
                isbn = %safe_isbn,
                "Library of Congress SRU answered 200 with a body that is not a searchRetrieve response"
            );
            return Err(MetadataError::Parse(
                "Library of Congress SRU response carries no numberOfRecords".to_string(),
            ));
        }
        Ok(Self::parse_marcxml_record(&body))
    }

    /// #495 — probe the endpoint the provider actually queries (including a
    /// `LOC_SRU_BASE_URL` override, so the E2E stack never probes the real
    /// host). A `HEAD` on the production SRU URL answers 400, which the #285
    /// any-status rule counts as reachable.
    fn health_check_url(&self) -> Option<&str> {
        Some(&self.sru_base_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            r.edition_statement.as_deref(),
            Some("Third edition."),
            "250$a"
        );
        assert_eq!(
            r.general_note.as_deref(),
            Some(r#""Updated for Java 9"--Cover."#),
            "500$a, first of two"
        );
        // Absent from this record — must be None, not an empty string.
        assert_eq!(r.collection_title, None, "490$a absent");
        assert_eq!(r.original_title, None, "240$a absent");
    }

    /// The zones-only parser (the completion contract) never supplies a title
    /// or a cover — those come from `parse_marcxml_record` on a primary lookup.
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

    /// #495 — the live `lx2.loc.gov` record for 9780134685991 (captured
    /// 2026-10-27), trimmed to the fields the provider reads, plus a second
    /// contributor and a name-title 700 (`$t`) that must NOT become an author.
    const SRU_FULL: &str = r#"<?xml version="1.0"?>
<zs:searchRetrieveResponse xmlns:zs="http://www.loc.gov/zing/srw/"><zs:version>1.1</zs:version><zs:numberOfRecords>1</zs:numberOfRecords><zs:records><zs:record><zs:recordSchema>marcxml</zs:recordSchema><zs:recordPacking>xml</zs:recordPacking><zs:recordData><record xmlns="http://www.loc.gov/MARC21/slim">
  <leader>02298cam a22004217i 4500</leader>
  <controlfield tag="001">20033591</controlfield>
  <controlfield tag="008">170925t20182018mau      b    001 0 eng d</controlfield>
  <datafield tag="020" ind1=" " ind2=" ">
    <subfield code="a">9780134685991</subfield>
    <subfield code="q">(paperback)</subfield>
  </datafield>
  <datafield tag="082" ind1="0" ind2="4">
    <subfield code="a">005.13/3</subfield>
    <subfield code="2">23</subfield>
  </datafield>
  <datafield tag="100" ind1="1" ind2=" ">
    <subfield code="a">Bloch, Joshua,</subfield>
    <subfield code="e">author.</subfield>
  </datafield>
  <datafield tag="245" ind1="1" ind2="0">
    <subfield code="a">Effective Java /</subfield>
    <subfield code="c">Joshua Bloch.</subfield>
  </datafield>
  <datafield tag="250" ind1=" " ind2=" ">
    <subfield code="a">Third edition.</subfield>
  </datafield>
  <datafield tag="264" ind1=" " ind2="4">
    <subfield code="c">&#xA9;2017</subfield>
  </datafield>
  <datafield tag="264" ind1=" " ind2="1">
    <subfield code="a">Boston :</subfield>
    <subfield code="b">Addison-Wesley,</subfield>
    <subfield code="c">[2018]</subfield>
  </datafield>
  <datafield tag="300" ind1=" " ind2=" ">
    <subfield code="a">xx, 392 pages ;</subfield>
    <subfield code="c">24 cm</subfield>
  </datafield>
  <datafield tag="500" ind1=" " ind2=" ">
    <subfield code="a">"Updated for Java 9"--Cover.</subfield>
  </datafield>
  <datafield tag="520" ind1=" " ind2=" ">
    <subfield code="a">"Each chapter of Effective Java consists of several items." --Publisher.</subfield>
  </datafield>
  <datafield tag="700" ind1="1" ind2=" ">
    <subfield code="a">Gafter, Neal,</subfield>
    <subfield code="e">contributor.</subfield>
  </datafield>
  <datafield tag="700" ind1="1" ind2="2">
    <subfield code="a">Gosling, James.</subfield>
    <subfield code="t">Java language specification.</subfield>
  </datafield>
  <datafield tag="700" ind1="1" ind2=" ">
    <subfield code="a">Bloch, Joshua.</subfield>
  </datafield>
</record></zs:recordData><zs:recordPosition>1</zs:recordPosition></zs:record></zs:records><zs:echoedSearchRetrieveRequest><zs:version>1.1</zs:version><zs:query>bath.isbn=9780134685991</zs:query></zs:echoedSearchRetrieveRequest></zs:searchRetrieveResponse>"#;

    /// #495 — the pre-RDA shape (260 instead of 264, "p." extent) with the
    /// decomposed numeric references the live feed uses for accents, from the
    /// record for 9782070360246 (captured 2026-10-27). Its 008 date1 is
    /// unknown (`19uu`) so the date must come from 260$c.
    const SRU_260_ACCENTS: &str = r#"<zs:searchRetrieveResponse xmlns:zs="http://www.loc.gov/zing/srw/"><zs:numberOfRecords>1</zs:numberOfRecords><zs:records><zs:record><zs:recordData><record xmlns="http://www.loc.gov/MARC21/slim">
  <controlfield tag="008">100421s19uu    fr            000 0 fre  </controlfield>
  <datafield tag="100" ind1="1" ind2=" ">
    <subfield code="a">Bomsel, Olivier.</subfield>
  </datafield>
  <datafield tag="245" ind1="1" ind2="2">
    <subfield code="a">L'e&#x301;conomie immate&#x301;rielle :</subfield>
    <subfield code="b">industries et marche&#x301;s d'expe&#x301;riences /</subfield>
    <subfield code="c">Olivier Bomsel.</subfield>
  </datafield>
  <datafield tag="260" ind1=" " ind2=" ">
    <subfield code="a">Paris :</subfield>
    <subfield code="b">Gallimard,</subfield>
    <subfield code="c">c2010.</subfield>
  </datafield>
  <datafield tag="300" ind1=" " ind2=" ">
    <subfield code="a">282 p. ;</subfield>
  </datafield>
</record></zs:recordData></zs:record></zs:records></zs:searchRetrieveResponse>"#;

    #[test]
    fn full_record_maps_the_descriptive_fields() {
        let r = LibraryOfCongressProvider::parse_marcxml_record(SRU_FULL).expect("record");
        assert_eq!(r.title.as_deref(), Some("Effective Java"));
        assert_eq!(r.subtitle, None);
        assert_eq!(
            r.authors,
            vec!["Joshua Bloch".to_string(), "Neal Gafter".to_string()],
            "100 first, 700 next, duplicates and name-title 700s skipped"
        );
        assert_eq!(
            r.publisher.as_deref(),
            Some("Addison-Wesley"),
            "264 ind2=1 wins over ind2=4"
        );
        assert_eq!(r.publication_date.as_deref(), Some("2018"), "008/07-10");
        assert_eq!(r.language.as_deref(), Some("en"), "008/35-37");
        assert_eq!(r.page_count, Some(392));
        assert_eq!(r.dewey_code.as_deref(), Some("005.133"));
        assert_eq!(
            r.description.as_deref(),
            Some(r#""Each chapter of Effective Java consists of several items." --Publisher."#)
        );
        assert_eq!(r.cover_url, None, "MARC 21 carries no cover");
    }

    /// The single SRU request also carries the zones, so a LoC win needs no
    /// second call to complete them.
    #[test]
    fn full_record_also_carries_the_six_zones() {
        let r = LibraryOfCongressProvider::parse_marcxml_record(SRU_FULL).unwrap();
        assert_eq!(
            r.statement_of_responsibility.as_deref(),
            Some("Joshua Bloch.")
        );
        assert_eq!(r.edition_statement.as_deref(), Some("Third edition."));
        assert_eq!(
            r.general_note.as_deref(),
            Some(r#""Updated for Java 9"--Cover."#)
        );
        assert_eq!(r.collection_title, None);
    }

    #[test]
    fn pre_rda_record_with_accents_is_decoded_and_composed() {
        let r = LibraryOfCongressProvider::parse_marcxml_record(SRU_260_ACCENTS).unwrap();
        assert_eq!(
            r.title.as_deref(),
            Some("L'\u{e9}conomie immat\u{e9}rielle")
        );
        assert_eq!(
            r.subtitle.as_deref(),
            Some("industries et march\u{e9}s d'exp\u{e9}riences")
        );
        assert_eq!(r.authors, vec!["Olivier Bomsel".to_string()]);
        assert_eq!(r.publisher.as_deref(), Some("Gallimard"));
        assert_eq!(
            r.publication_date.as_deref(),
            Some("2010"),
            "008 19uu → 260$c"
        );
        assert_eq!(r.language.as_deref(), Some("fr"));
        assert_eq!(r.page_count, Some(282));
    }

    /// Without a publication 264 (ind2="1"), a copyright 264 (ind2="4") must
    /// not stand in: its $c is a ©-date and it has no publisher.
    #[test]
    fn copyright_264_is_never_the_publication_statement() {
        let body = r#"<zs:numberOfRecords>1</zs:numberOfRecords><record xmlns="x">
  <controlfield tag="008">200101s19uu    nyu           000 0 eng d</controlfield>
  <datafield tag="245" ind1="1" ind2="0"><subfield code="a">T.</subfield></datafield>
  <datafield tag="264" ind1=" " ind2="4"><subfield code="c">&#xA9;1999</subfield></datafield>
  <datafield tag="260" ind1=" " ind2=" "><subfield code="b">Old Press,</subfield><subfield code="c">2001.</subfield></datafield>
</record>"#;
        let r = LibraryOfCongressProvider::parse_marcxml_record(body).unwrap();
        assert_eq!(r.publisher.as_deref(), Some("Old Press"));
        assert_eq!(r.publication_date.as_deref(), Some("2001"));
    }

    #[test]
    fn record_without_245a_is_no_result() {
        let body = r#"<zs:numberOfRecords>1</zs:numberOfRecords><record xmlns="x">
  <datafield tag="100" ind1="1" ind2=" "><subfield code="a">Someone,</subfield></datafield>
</record>"#;
        assert!(LibraryOfCongressProvider::parse_marcxml_record(body).is_none());
        // The zone parser still sees the record (it has no zones, but exists).
        assert!(LibraryOfCongressProvider::parse_marcxml_response(body).is_some());
    }

    #[test]
    fn empty_or_garbage_response_is_no_result() {
        assert!(LibraryOfCongressProvider::parse_marcxml_record(SRU_EMPTY).is_none());
        assert!(LibraryOfCongressProvider::parse_marcxml_record("").is_none());
        assert!(
            LibraryOfCongressProvider::parse_marcxml_record("<html>challenge</html>").is_none()
        );
        // `<zs:recordData>` alone is not a MARC record.
        let no_record = r#"<zs:numberOfRecords>1</zs:numberOfRecords><zs:records><zs:record><zs:recordData></zs:recordData></zs:record></zs:records>"#;
        assert!(LibraryOfCongressProvider::parse_marcxml_record(no_record).is_none());
    }

    #[test]
    fn invert_name_only_touches_surname_comma_forename() {
        assert_eq!(invert_name("Bloch, Joshua"), "Joshua Bloch");
        assert_eq!(invert_name("Tolkien, J. R. R."), "J. R. R. Tolkien");
        assert_eq!(invert_name("Madonna"), "Madonna");
        assert_eq!(invert_name("Smith, John, Jr."), "Smith, John, Jr.");
    }

    #[test]
    fn first_year_reads_bracketed_and_prefixed_years() {
        assert_eq!(first_year("[2018]").as_deref(), Some("2018"));
        assert_eq!(first_year("c2010.").as_deref(), Some("2010"));
        assert_eq!(first_year("\u{a9}2018").as_deref(), Some("2018"));
        assert_eq!(first_year("[19--?]"), None);
    }

    #[test]
    fn user_agent_carries_the_running_version_and_project_url() {
        assert!(USER_AGENT.starts_with(&format!("mybibli/{} ", env!("CARGO_PKG_VERSION"))));
        assert!(USER_AGENT.contains("https://github.com/guycorbaz/mybibli"));
    }

    #[test]
    fn cloudflare_challenge_is_read_from_the_header_case_insensitively() {
        use reqwest::header::{HeaderMap, HeaderValue};
        let mut h = HeaderMap::new();
        assert!(!is_cloudflare_challenge(&h));
        h.insert("cf-mitigated", HeaderValue::from_static("Challenge"));
        assert!(is_cloudflare_challenge(&h));
        let mut other = HeaderMap::new();
        other.insert("cf-mitigated", HeaderValue::from_static("block"));
        assert!(!is_cloudflare_challenge(&other));
    }

    #[test]
    fn sru_failures_map_onto_typed_metadata_errors() {
        assert!(matches!(
            MetadataError::from(SruFailure::Status(429)),
            MetadataError::RateLimited
        ));
        assert!(matches!(
            MetadataError::from(SruFailure::Status(503)),
            MetadataError::Unavailable
        ));
        let challenge = MetadataError::from(SruFailure::Challenge(403)).to_string();
        let plain = MetadataError::from(SruFailure::Status(403)).to_string();
        assert!(challenge.contains("Cloudflare challenge"), "{challenge}");
        assert!(!plain.contains("Cloudflare"), "{plain}");
    }

    #[test]
    fn health_probe_targets_the_queried_sru_endpoint() {
        let p = LibraryOfCongressProvider::with_base_url(reqwest::Client::new(), "http://x/LCDB");
        assert_eq!(p.health_check_url(), Some("http://x/LCDB"));
        assert_eq!(SRU_BASE_URL_DEFAULT, "http://lx2.loc.gov:210/LCDB");
    }

    // ─── #495 — provider against a local mock SRU server ──────────────

    mod mock {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use axum::extract::{Query, State};
        use axum::http::{HeaderMap, StatusCode};
        use axum::response::IntoResponse;

        pub const NOMINAL: &str = "9780134685991";
        pub const EMPTY: &str = "9780000000000";
        pub const CHALLENGE: &str = "9780000000001";
        pub const SERVER_ERROR: &str = "9780000000002";
        pub const NOT_SRU: &str = "9780000000003";

        #[derive(serde::Deserialize)]
        struct Params {
            query: String,
        }

        async fn sru(
            State(hits): State<Arc<AtomicUsize>>,
            headers: HeaderMap,
            Query(p): Query<Params>,
        ) -> axum::response::Response {
            hits.fetch_add(1, Ordering::SeqCst);
            // Every request must identify itself. A mismatch answers 418 (no
            // retry, immediate test failure) instead of panicking the server
            // task, which would look like a dropped connection and trigger
            // the 2 s + 4 s transport back-off.
            if headers.get("user-agent").and_then(|v| v.to_str().ok()) != Some(super::USER_AGENT) {
                return (StatusCode::IM_A_TEAPOT, "bad user-agent").into_response();
            }
            match p.query.trim_start_matches("bath.isbn=") {
                NOMINAL => (StatusCode::OK, super::SRU_FULL).into_response(),
                CHALLENGE => (
                    StatusCode::FORBIDDEN,
                    [("cf-mitigated", "challenge"), ("content-type", "text/html")],
                    "<html><title>Just a moment...</title></html>",
                )
                    .into_response(),
                SERVER_ERROR => (StatusCode::INTERNAL_SERVER_ERROR, "boom").into_response(),
                NOT_SRU => (StatusCode::OK, "<html><body>Portal</body></html>").into_response(),
                _ => (StatusCode::OK, super::SRU_EMPTY).into_response(),
            }
        }

        /// Serve `/LCDB` on a random local port; returns the base URL and the
        /// request counter.
        pub async fn start() -> (String, Arc<AtomicUsize>) {
            let hits = Arc::new(AtomicUsize::new(0));
            let app = axum::Router::new()
                .route("/LCDB", axum::routing::get(sru))
                .with_state(hits.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            (format!("http://{addr}/LCDB"), hits)
        }
    }

    async fn mock_provider() -> (
        LibraryOfCongressProvider,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let (base, hits) = mock::start().await;
        (
            LibraryOfCongressProvider::with_base_url(reqwest::Client::new(), &base),
            hits,
        )
    }

    #[tokio::test]
    async fn lookup_reads_the_record_from_sru() {
        let (p, hits) = mock_provider().await;
        let r = p
            .lookup_by_isbn(mock::NOMINAL)
            .await
            .unwrap()
            .expect("a record");
        assert_eq!(r.title.as_deref(), Some("Effective Java"));
        assert_eq!(r.publisher.as_deref(), Some("Addison-Wesley"));
        assert_eq!(r.edition_statement.as_deref(), Some("Third edition."));
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "one request only"
        );
    }

    #[tokio::test]
    async fn lookup_of_an_unknown_isbn_is_a_clean_miss() {
        let (p, _) = mock_provider().await;
        assert!(p.lookup_by_isbn(mock::EMPTY).await.unwrap().is_none());
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn challenge_is_logged_as_such_not_retried_and_fails_the_lookup() {
        let (p, hits) = mock_provider().await;
        let err = p.lookup_by_isbn(mock::CHALLENGE).await.unwrap_err();
        assert!(err.to_string().contains("Cloudflare challenge"), "{err}");
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "never retried"
        );
        assert!(logs_contain("WARN"));
        assert!(logs_contain("Cloudflare challenge"));
        assert!(!logs_contain("non-success status"));
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn plain_http_error_is_logged_at_warn_distinctly() {
        let (p, hits) = mock_provider().await;
        let err = p.lookup_by_isbn(mock::SERVER_ERROR).await.unwrap_err();
        assert!(err.to_string().contains("status 500"), "{err}");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(logs_contain("WARN"));
        assert!(logs_contain("non-success status"));
        assert!(!logs_contain("Cloudflare"));
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn a_200_that_is_not_an_sru_answer_is_a_failure_not_a_miss() {
        let (p, _) = mock_provider().await;
        let err = p.lookup_by_isbn(mock::NOT_SRU).await.unwrap_err();
        assert!(matches!(err, MetadataError::Parse(_)), "{err}");
        assert!(logs_contain("not a searchRetrieve response"));
    }

    #[tokio::test]
    async fn zone_entry_point_degrades_to_none_on_challenge_and_error() {
        let (p, _) = mock_provider().await;
        assert!(p.lookup_marc_zones(mock::CHALLENGE).await.is_none());
        assert!(p.lookup_marc_zones(mock::SERVER_ERROR).await.is_none());
        assert!(p.lookup_marc_zones(mock::EMPTY).await.is_none());
        let zones = p.lookup_marc_zones(mock::NOMINAL).await.expect("zones");
        assert_eq!(zones.edition_statement.as_deref(), Some("Third edition."));
        assert_eq!(zones.title, None, "the zone path never supplies a title");
    }

    #[tokio::test]
    async fn non_alphanumeric_isbn_makes_no_request() {
        let p =
            LibraryOfCongressProvider::with_base_url(reqwest::Client::new(), "http://127.0.0.1:1");
        assert!(p.lookup_by_isbn("///???").await.unwrap().is_none());
        assert!(p.lookup_marc_zones("///").await.is_none());
    }
}
