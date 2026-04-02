use async_trait::async_trait;

use crate::models::media_type::MediaType;

use super::provider::{MetadataError, MetadataProvider, MetadataResult};

/// BnF (Bibliotheque nationale de France) metadata provider.
/// Uses the SRU (Search/Retrieve via URL) API with UNIMARC XML responses.
/// No API key required.
pub struct BnfProvider {
    client: reqwest::Client,
    base_url: String,
}

impl BnfProvider {
    pub fn new(client: reqwest::Client) -> Self {
        let base_url = std::env::var("BNF_API_BASE_URL")
            .unwrap_or_else(|_| "https://catalogue.bnf.fr/api/SRU".to_string());
        BnfProvider { client, base_url }
    }

    /// Create with a custom base URL (for testing with mock server).
    pub fn with_base_url(client: reqwest::Client, base_url: &str) -> Self {
        BnfProvider {
            client,
            base_url: base_url.to_string(),
        }
    }

    /// Parse UNIMARC XML response into MetadataResult.
    /// UNIMARC fields: 200$a = title, 200$e = subtitle, 200$f = first author statement,
    /// 700$a = author surname, 700$b = author forename,
    /// 210$c = publisher, 210$d = date, 101$a = language
    pub fn parse_sru_response(xml: &str) -> Option<MetadataResult> {
        // Check if we have any records
        if !xml.contains("<record>") && !xml.contains("<srw:record>") {
            return None;
        }

        // Extract author from 700 field (surname + forename)
        let surname = Self::extract_subfield(xml, "700", "a");
        let forename = Self::extract_subfield(xml, "700", "b");
        let mut authors = Vec::new();
        match (surname, forename) {
            (Some(s), Some(f)) => {
                let name = format!("{} {}", f.trim(), s.trim());
                let name = name.trim().to_string();
                if !name.is_empty() {
                    authors.push(name);
                }
            }
            (Some(s), None) => {
                let name = s.trim().to_string();
                if !name.is_empty() {
                    authors.push(name);
                }
            }
            _ => {
                // Fallback: try 200$f (statement of responsibility)
                if let Some(author_statement) = Self::extract_subfield(xml, "200", "f") {
                    let name = author_statement.trim().to_string();
                    if !name.is_empty() {
                        authors.push(name);
                    }
                }
            }
        }

        let title = Self::extract_subfield(xml, "200", "a");
        title.as_ref()?;

        // Extract page count from 215$a (physical description).
        // Common formats: "245 p.", "1 vol. (245 p.)", "XII-245 p.", "p. 245"
        // Strategy: find a number that appears right before "p" or extract digits near "p."
        let page_count = Self::extract_subfield(xml, "215", "a").and_then(|desc| {
            // Look for pattern: digits followed by optional space and "p" (e.g. "245 p.", "245p")
            let words: Vec<&str> = desc.split_whitespace().collect();
            for (i, word) in words.iter().enumerate() {
                if word.starts_with("p.") || *word == "p" {
                    // The number should be the previous word
                    if i > 0 {
                        // Handle "XII-245" by taking the part after the last hyphen
                        let prev = words[i - 1];
                        let num_part = prev.rsplit('-').next().unwrap_or(prev);
                        let cleaned: String = num_part.chars().filter(|c| c.is_ascii_digit()).collect();
                        if let Ok(n) = cleaned.parse::<i32>()
                            && n > 0
                        {
                            return Some(n);
                        }
                    }
                }
            }
            // Fallback: extract digits from within parentheses like "(245 p.)"
            if let Some(start) = desc.find('(')
                && let Some(end) = desc.find(')')
            {
                let inner = &desc[start + 1..end];
                let digits: String = inner.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(n) = digits.parse::<i32>()
                    && n > 0
                {
                    return Some(n);
                }
            }
            None
        });

        Some(MetadataResult {
            title,
            subtitle: Self::extract_subfield(xml, "200", "e"),
            publisher: Self::extract_subfield(xml, "210", "c"),
            publication_date: Self::extract_subfield(xml, "210", "d"),
            language: Self::extract_subfield(xml, "101", "a"),
            description: Self::extract_subfield(xml, "330", "a"),
            authors,
            page_count,
            ..MetadataResult::default()
        })
    }

    /// Extract a subfield value from UNIMARC XML.
    /// Looks for `<datafield tag="TAG" ...><subfield code="CODE">VALUE</subfield></datafield>`
    fn extract_subfield(xml: &str, tag: &str, code: &str) -> Option<String> {
        // Find the datafield with matching tag
        let tag_pattern = format!(r#"tag="{}""#, tag);
        let mut search_from = 0;

        while let Some(df_start) = xml[search_from..].find(&tag_pattern) {
            let df_abs = search_from + df_start;

            // Find the end of this datafield
            let df_end = match xml[df_abs..].find("</datafield>") {
                Some(pos) => df_abs + pos,
                None => {
                    // Try namespace-prefixed variant
                    match xml[df_abs..].find("</mxc:datafield>") {
                        Some(pos) => df_abs + pos,
                        None => break,
                    }
                }
            };

            let datafield_content = &xml[df_abs..df_end];

            // Find subfield with matching code
            let code_pattern = format!(r#"code="{}""#, code);
            if let Some(sf_start) = datafield_content.find(&code_pattern) {
                // Find the > after the code attribute
                let after_code = &datafield_content[sf_start..];
                if let Some(gt_pos) = after_code.find('>') {
                    let value_start = sf_start + gt_pos + 1;
                    let value_content = &datafield_content[value_start..];
                    // Find closing </subfield>
                    let end_tag = if value_content.contains("</subfield>") {
                        "</subfield>"
                    } else {
                        "</mxc:subfield>"
                    };
                    if let Some(end_pos) = value_content.find(end_tag) {
                        let value = value_content[..end_pos].trim().to_string();
                        if !value.is_empty() {
                            return Some(value);
                        }
                    }
                }
            }

            search_from = df_end;
        }

        None
    }
}

#[async_trait]
impl MetadataProvider for BnfProvider {
    fn name(&self) -> &str {
        "BnF"
    }

    fn supports_media_type(&self, media_type: &MediaType) -> bool {
        matches!(media_type, MediaType::Book | MediaType::Bd | MediaType::Magazine)
    }

    async fn lookup_by_isbn(&self, isbn: &str) -> Result<Option<MetadataResult>, MetadataError> {
        // URL-encode ISBN to prevent query injection from malformed input
        let encoded_isbn: String = isbn
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        let url = format!(
            "{}?version=1.2&operation=searchRetrieve&query=bib.isbn%20adj%20%22{}%22&recordSchema=unimarcXchange&maximumRecords=1",
            self.base_url, encoded_isbn
        );

        tracing::debug!(isbn = %isbn, provider = "BnF", "Looking up ISBN");

        let response = self
            .client
            .get(&url)
            .header("User-Agent", "mybibli/0.1.0")
            .send()
            .await
            .map_err(|e| MetadataError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(MetadataError::Network(format!(
                "BnF API returned status {}",
                response.status()
            )));
        }

        let body = response
            .text()
            .await
            .map_err(|e| MetadataError::Parse(e.to_string()))?;

        Ok(Self::parse_sru_response(&body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BNF_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<srw:searchRetrieveResponse xmlns:srw="http://www.loc.gov/zing/srw/">
  <srw:numberOfRecords>1</srw:numberOfRecords>
  <srw:records>
    <srw:record>
      <srw:recordData>
        <mxc:record xmlns:mxc="info:lc/xmlns/marcxchange-v2">
          <mxc:datafield tag="101" ind1=" " ind2=" ">
            <mxc:subfield code="a">fre</mxc:subfield>
          </mxc:datafield>
          <mxc:datafield tag="200" ind1="1" ind2=" ">
            <mxc:subfield code="a">L'Écume des jours</mxc:subfield>
            <mxc:subfield code="e">roman</mxc:subfield>
            <mxc:subfield code="f">Boris Vian</mxc:subfield>
          </mxc:datafield>
          <mxc:datafield tag="210" ind1=" " ind2=" ">
            <mxc:subfield code="c">Gallimard</mxc:subfield>
            <mxc:subfield code="d">1947</mxc:subfield>
          </mxc:datafield>
          <mxc:datafield tag="700" ind1=" " ind2=" ">
            <mxc:subfield code="a">Vian</mxc:subfield>
            <mxc:subfield code="b">Boris</mxc:subfield>
          </mxc:datafield>
          <mxc:datafield tag="330" ind1=" " ind2=" ">
            <mxc:subfield code="a">A surrealist love story set in a dreamlike world.</mxc:subfield>
          </mxc:datafield>
        </mxc:record>
      </srw:recordData>
    </srw:record>
  </srw:records>
</srw:searchRetrieveResponse>"#;

    #[test]
    fn test_parse_sru_response_success() {
        let result = BnfProvider::parse_sru_response(SAMPLE_BNF_RESPONSE);
        assert!(result.is_some());
        let meta = result.unwrap();
        assert_eq!(meta.title.as_deref(), Some("L'\u{c9}cume des jours"));
        assert_eq!(meta.subtitle.as_deref(), Some("roman"));
        assert_eq!(meta.publisher.as_deref(), Some("Gallimard"));
        assert_eq!(meta.publication_date.as_deref(), Some("1947"));
        assert_eq!(meta.language.as_deref(), Some("fre"));
        assert_eq!(meta.authors.len(), 1);
        assert_eq!(meta.authors[0], "Boris Vian");
        assert_eq!(
            meta.description.as_deref(),
            Some("A surrealist love story set in a dreamlike world.")
        );
    }

    #[test]
    fn test_parse_sru_response_empty() {
        let xml = r#"<?xml version="1.0"?>
<srw:searchRetrieveResponse xmlns:srw="http://www.loc.gov/zing/srw/">
  <srw:numberOfRecords>0</srw:numberOfRecords>
  <srw:records/>
</srw:searchRetrieveResponse>"#;
        let result = BnfProvider::parse_sru_response(xml);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_sru_response_no_author_700_fallback_200f() {
        let xml = r#"<record>
          <datafield tag="200" ind1="1" ind2=" ">
            <subfield code="a">Test Title</subfield>
            <subfield code="f">Author Name</subfield>
          </datafield>
        </record>"#;
        let result = BnfProvider::parse_sru_response(xml);
        assert!(result.is_some());
        let meta = result.unwrap();
        assert_eq!(meta.title.as_deref(), Some("Test Title"));
        assert_eq!(meta.authors, vec!["Author Name"]);
    }

    #[test]
    fn test_bnf_provider_supports_media_types() {
        let provider = BnfProvider::new(reqwest::Client::new());
        assert!(provider.supports_media_type(&MediaType::Book));
        assert!(provider.supports_media_type(&MediaType::Bd));
        assert!(provider.supports_media_type(&MediaType::Magazine));
        assert!(!provider.supports_media_type(&MediaType::Cd));
        assert!(!provider.supports_media_type(&MediaType::Dvd));
    }

    #[test]
    fn test_bnf_provider_name() {
        let provider = BnfProvider::new(reqwest::Client::new());
        assert_eq!(provider.name(), "BnF");
    }

    #[test]
    fn test_parse_sru_response_minimal_title_only() {
        let xml = r#"<record>
          <datafield tag="200" ind1="1" ind2=" ">
            <subfield code="a">Minimal Title</subfield>
          </datafield>
        </record>"#;
        let result = BnfProvider::parse_sru_response(xml);
        assert!(result.is_some());
        let meta = result.unwrap();
        assert_eq!(meta.title.as_deref(), Some("Minimal Title"));
        assert!(meta.authors.is_empty());
        assert!(meta.publisher.is_none());
    }

    #[test]
    fn test_parse_page_count_simple() {
        let xml = r#"<record>
          <datafield tag="200" ind1="1" ind2=" ">
            <subfield code="a">Test</subfield>
          </datafield>
          <datafield tag="215" ind1=" " ind2=" ">
            <subfield code="a">245 p.</subfield>
          </datafield>
        </record>"#;
        let result = BnfProvider::parse_sru_response(xml).unwrap();
        assert_eq!(result.page_count, Some(245));
    }

    #[test]
    fn test_parse_page_count_roman_prefix() {
        let xml = r#"<record>
          <datafield tag="200" ind1="1" ind2=" ">
            <subfield code="a">Test</subfield>
          </datafield>
          <datafield tag="215" ind1=" " ind2=" ">
            <subfield code="a">XII-245 p.</subfield>
          </datafield>
        </record>"#;
        let result = BnfProvider::parse_sru_response(xml).unwrap();
        assert_eq!(result.page_count, Some(245));
    }

    #[test]
    fn test_parse_page_count_volume_with_parens() {
        let xml = r#"<record>
          <datafield tag="200" ind1="1" ind2=" ">
            <subfield code="a">Test</subfield>
          </datafield>
          <datafield tag="215" ind1=" " ind2=" ">
            <subfield code="a">1 vol. (320 p.)</subfield>
          </datafield>
        </record>"#;
        let result = BnfProvider::parse_sru_response(xml).unwrap();
        assert_eq!(result.page_count, Some(320));
    }

    #[test]
    fn test_parse_page_count_none_when_missing() {
        let xml = r#"<record>
          <datafield tag="200" ind1="1" ind2=" ">
            <subfield code="a">Test</subfield>
          </datafield>
        </record>"#;
        let result = BnfProvider::parse_sru_response(xml).unwrap();
        assert!(result.page_count.is_none());
    }
}
