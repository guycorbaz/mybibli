//! Shared MARC datafield/subfield reader (#439).
//!
//! UNIMARC (BnF, `unimarcXchange`) and MARC 21 (Library of Congress, `marcxml`)
//! disagree on which tag carries which meaning, but they share the same
//! serialisation:
//!
//! ```xml
//! <datafield tag="245" ind1="1" ind2="0">
//!   <subfield code="a">Effective Java /</subfield>
//!   <subfield code="c">Joshua Bloch.</subfield>
//! </datafield>
//! ```
//!
//! So the *reader* is common and only the tag table differs. This module was
//! lifted out of `bnf.rs` when the LoC provider needed the same traversal
//! (Foundation Rule #1) — behaviour is unchanged, including the two quirks the
//! BnF feed made necessary:
//!
//! 1. **Namespace-prefixed closing tags.** The BnF emits `</mxc:datafield>` and
//!    `</mxc:subfield>` in some responses, plain `</datafield>` in others.
//! 2. **Empty subfields are treated as absent**, so a present-but-blank
//!    `<subfield code="a"></subfield>` does not shadow a later datafield
//!    carrying a real value.
//!
//! The parser is deliberately a string scan rather than a real XML parse: it
//! only ever reads a handful of known tags out of a single record, and pulling
//! an XML dependency in for that was not worth it. It is tolerant of unknown
//! attributes and attribute order, but it does NOT decode entities — callers
//! that need that must handle it.

/// First non-empty `subfield[@code=code]` inside any `datafield[@tag=tag]`.
///
/// Datafields are scanned in document order and the first match wins. When a
/// tag legitimately repeats — MARC 21 `500` (general note) routinely does —
/// this returns the first occurrence only. That is the deliberate choice: a
/// joined blob reads badly in the title-detail UI, and the first note is the
/// principal one in practice.
pub fn extract_subfield(xml: &str, tag: &str, code: &str) -> Option<String> {
    extract_subfields(xml, tag, code).into_iter().next()
}

/// Every non-empty `subfield[@code=code]` across all `datafield[@tag=tag]`, in
/// document order.
///
/// Exposed for callers that genuinely want the repeats (e.g. collecting all
/// authors from repeated author datafields) without re-implementing the scan.
pub fn extract_subfields(xml: &str, tag: &str, code: &str) -> Vec<String> {
    let tag_pattern = format!(r#"tag="{tag}""#);
    let code_pattern = format!(r#"code="{code}""#);
    let mut out = Vec::new();
    let mut search_from = 0;

    while let Some(df_start) = xml[search_from..].find(&tag_pattern) {
        let df_abs = search_from + df_start;

        // End of this datafield — plain or namespace-prefixed.
        let df_end = match xml[df_abs..].find("</datafield>") {
            Some(pos) => df_abs + pos,
            None => match xml[df_abs..].find("</mxc:datafield>") {
                Some(pos) => df_abs + pos,
                None => break,
            },
        };

        let datafield = &xml[df_abs..df_end];
        if let Some(sf_start) = datafield.find(&code_pattern) {
            let after_code = &datafield[sf_start..];
            if let Some(gt_pos) = after_code.find('>') {
                let value_start = sf_start + gt_pos + 1;
                let value_content = &datafield[value_start..];
                let end_tag = if value_content.contains("</subfield>") {
                    "</subfield>"
                } else {
                    "</mxc:subfield>"
                };
                if let Some(end_pos) = value_content.find(end_tag) {
                    let value = value_content[..end_pos].trim();
                    if !value.is_empty() {
                        out.push(value.to_string());
                    }
                }
            }
        }

        search_from = df_end;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trimmed real MARC 21 record from lx2.loc.gov for ISBN 9780134685991,
    // captured 2026-07-28. Note the two repeated 500 fields — the shape that
    // motivated `extract_subfields`.
    const MARC21: &str = r#"<record xmlns="http://www.loc.gov/MARC21/slim">
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
</record>"#;

    // The BnF's namespace-prefixed variant, which the shared reader must keep
    // handling — this is the quirk that made a plain XML parser look expensive.
    const UNIMARC_PREFIXED: &str = r#"<mxc:record>
  <mxc:datafield tag="200" ind1="1" ind2=" ">
    <mxc:subfield code="a">Le petit prince</mxc:subfield>
    <mxc:subfield code="f">Antoine de Saint-Exupéry</mxc:subfield>
  </mxc:datafield>
</mxc:record>"#;

    #[test]
    fn reads_a_marc21_subfield() {
        assert_eq!(
            extract_subfield(MARC21, "245", "c"),
            Some("Joshua Bloch.".to_string())
        );
        assert_eq!(
            extract_subfield(MARC21, "250", "a"),
            Some("Third edition.".to_string())
        );
    }

    #[test]
    fn reads_a_namespace_prefixed_unimarc_subfield() {
        assert_eq!(
            extract_subfield(UNIMARC_PREFIXED, "200", "f"),
            Some("Antoine de Saint-Exupéry".to_string())
        );
    }

    #[test]
    fn repeated_tag_yields_the_first_for_extract_subfield() {
        assert_eq!(
            extract_subfield(MARC21, "500", "a"),
            Some(r#""Updated for Java 9"--Cover."#.to_string())
        );
    }

    #[test]
    fn repeated_tag_yields_all_for_extract_subfields() {
        let notes = extract_subfields(MARC21, "500", "a");
        assert_eq!(notes.len(), 2, "both 500 fields must be reachable");
        assert!(notes[1].contains("Best practices"));
    }

    #[test]
    fn absent_tag_and_absent_code_are_none() {
        assert_eq!(extract_subfield(MARC21, "490", "a"), None);
        assert_eq!(extract_subfield(MARC21, "245", "z"), None);
    }

    /// An empty subfield must not shadow a later datafield that has content —
    /// otherwise a blank leading record silently suppresses a good one.
    #[test]
    fn empty_subfield_is_skipped_in_favour_of_a_later_one() {
        let xml = r#"<record>
  <datafield tag="500"><subfield code="a">   </subfield></datafield>
  <datafield tag="500"><subfield code="a">Real note.</subfield></datafield>
</record>"#;
        assert_eq!(
            extract_subfield(xml, "500", "a"),
            Some("Real note.".to_string())
        );
    }

    #[test]
    fn malformed_input_does_not_panic_or_hang() {
        assert_eq!(extract_subfield(r#"<datafield tag="245">"#, "245", "a"), None);
        assert_eq!(extract_subfield("", "245", "a"), None);
        assert_eq!(extract_subfield(r#"tag="245" tag="245""#, "245", "a"), None);
    }
}
