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
    datafields(xml, tag)
        .into_iter()
        .filter_map(|df| first_subfield_in(df, code))
        .collect()
}

/// Every `datafield[@tag=tag]` in document order, each as the raw slice from
/// its opening `<` through its closing tag (#495).
///
/// For callers that must read several subfields — or the indicators, see
/// [`indicator2`] — of the SAME datafield, e.g. skip a MARC 21 `700` that
/// carries a `$t` (a related work, not a contributor). [`extract_subfield`]
/// works on a slice unchanged, because the slice contains the `tag="…"`
/// pattern. Starting at `<` rather than at `tag=` keeps attributes written
/// before `tag` (legal XML) inside the slice.
pub fn datafields<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let tag_pattern = format!(r#"tag="{tag}""#);
    let mut out = Vec::new();
    let mut search_from = 0;

    while let Some(df_start) = xml[search_from..].find(&tag_pattern) {
        let df_abs = search_from + df_start;
        // Back up to the element's `<` when `tag=` is not its first attribute.
        let open = xml[search_from..df_abs]
            .rfind('<')
            .map(|p| search_from + p)
            .filter(|&p| !xml[p..df_abs].contains('>'))
            .unwrap_or(df_abs);

        // End of this datafield — plain or namespace-prefixed. The slice
        // keeps the closing tag so `extract_subfield` can run on it as is.
        let df_end = match xml[df_abs..].find("</datafield>") {
            Some(pos) => df_abs + pos + "</datafield>".len(),
            None => match xml[df_abs..].find("</mxc:datafield>") {
                Some(pos) => df_abs + pos + "</mxc:datafield>".len(),
                None => break,
            },
        };

        out.push(&xml[open..df_end]);
        search_from = df_end;
    }

    out
}

/// Second indicator of a datafield slice from [`datafields`], read from its
/// opening tag only (never from subfield text), with either quote style.
pub fn indicator2(datafield: &str) -> Option<char> {
    let opening = &datafield[..datafield.find('>')?];
    let after = &opening[opening.find("ind2=")? + "ind2=".len()..];
    let mut chars = after.chars();
    let quote = chars.next().filter(|q| *q == '"' || *q == '\'')?;
    let value = chars.next()?;
    (chars.next() == Some(quote)).then_some(value)
}

/// First non-empty `subfield[@code=code]` value inside one datafield slice.
fn first_subfield_in(datafield: &str, code: &str) -> Option<String> {
    let code_pattern = format!(r#"code="{code}""#);
    let sf_start = datafield.find(&code_pattern)?;
    let after_code = &datafield[sf_start..];
    let gt_pos = after_code.find('>')?;
    let value_start = sf_start + gt_pos + 1;
    let value_content = &datafield[value_start..];
    let end_tag = if value_content.contains("</subfield>") {
        "</subfield>"
    } else {
        "</mxc:subfield>"
    };
    let end_pos = value_content.find(end_tag)?;
    let value = value_content[..end_pos].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Raw text of `controlfield[@tag=tag]` (e.g. the MARC 21 `008` fixed-length
/// data elements), untrimmed: positions inside it are significant (#495).
pub fn controlfield(xml: &str, tag: &str) -> Option<String> {
    let pattern = format!(r#"<controlfield tag="{tag}">"#);
    let start = xml.find(&pattern)? + pattern.len();
    let len = xml[start..].find("</controlfield>")?;
    Some(xml[start..start + len].to_string())
}

/// Decode the XML entities a MARCXML text node may carry and normalise the
/// result to Unicode NFC (#495).
///
/// The Library of Congress encodes every non-ASCII character as a numeric
/// reference, frequently in decomposed form (`e&#x301;` for `é`), so a raw
/// value would show the references and, once decoded, would still not match a
/// precomposed search query. Handles `&#xHH;`, `&#NN;` and the five named XML
/// entities; anything else (malformed or unknown) is kept verbatim.
pub fn decode_xml_text(raw: &str) -> String {
    use unicode_normalization::UnicodeNormalization;

    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let candidate = &rest[amp..];
        let decoded = candidate.find(';').and_then(|semi| {
            let entity = &candidate[1..semi];
            // Some(Some(c)): push c; Some(None): drop the reference; None:
            // keep it verbatim.
            let ch: Option<Option<char>> = match entity {
                "amp" => Some(Some('&')),
                "lt" => Some(Some('<')),
                "gt" => Some(Some('>')),
                "quot" => Some(Some('"')),
                "apos" => Some(Some('\'')),
                _ => {
                    let code = if let Some(hex) =
                        entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X"))
                    {
                        u32::from_str_radix(hex, 16).ok()
                    } else if let Some(dec) = entity.strip_prefix('#') {
                        dec.parse::<u32>().ok()
                    } else {
                        None
                    };
                    match code.and_then(char::from_u32) {
                        // C0 controls other than tab / LF / CR are forbidden
                        // in XML 1.0: keep the reference verbatim rather than
                        // smuggle a NUL or ESC into the catalogue.
                        Some(c) if c < ' ' && !matches!(c, '\t' | '\n' | '\r') => None,
                        // C1 controls are legal but invisible; MARC 21 uses
                        // them as non-sort markers (U+0088/89, U+0098/9C
                        // around a leading article). Drop them.
                        Some(c) if ('\u{80}'..='\u{9f}').contains(&c) => Some(None),
                        other => other.map(Some),
                    }
                }
            };
            ch.map(|c| (c, semi + 1))
        });
        match decoded {
            Some((c, consumed)) => {
                if let Some(c) = c {
                    out.push(c);
                }
                rest = &candidate[consumed..];
            }
            None => {
                out.push('&');
                rest = &candidate[1..];
            }
        }
    }
    out.push_str(rest);
    out.nfc().collect()
}

/// Abbreviations whose final period is part of the word, not MARC's closing
/// punctuation (compared case-insensitively, without the period).
const KEEP_PERIOD_ABBREVIATIONS: [&str; 19] = [
    "inc", "co", "corp", "ltd", "jr", "sr", "etc", "bros", "st", "dept", "univ", "pub", "publ",
    "dist", "assoc", "mfg", "intl", "vol", "ed",
];

/// Strip the ISBD / MARC trailing punctuation from a descriptive value (#495).
///
/// - `/`, `:`, `;`, `=` — ISBD separators, stripped only when preceded by
///   whitespace (always the case in ISBD); a bare trailing `/` (`"AC/DC/"`)
///   is left alone.
/// - `,` — the comma MARC appends before the next subfield (`"Addison-Wesley,"`).
/// - one final `.` — MARC's closing period — unless it belongs to the value: an
///   initial (`"Tolkien, J. R. R."`), a dotted abbreviation (`"U.S.A."`,
///   `"Ph.D."`), a known abbreviation (`"Co."`, `"Jr."`, `"Univ."`) or an
///   ellipsis (`"And then..."`). A period right after `)` or `]` is always
///   MARC's (`"Title (2nd ed.)."`).
///
/// Separators and commas are stripped repeatedly, before and after the
/// period (`"Title / ;"`, `"Gallimard, ."` → `"Gallimard"`); the period at
/// most once.
pub fn trim_isbd_punctuation(raw: &str) -> String {
    let mut s = raw.trim();
    let mut period_done = false;
    loop {
        let before = s.len();
        s = s.trim_end_matches(',').trim_end();
        let mut chars = s.chars().rev();
        if let (Some(last), Some(prev)) = (chars.next(), chars.next())
            && matches!(last, '/' | ':' | ';' | '=')
            && prev.is_whitespace()
        {
            s = s[..s.len() - last.len_utf8()].trim_end();
        }
        if !period_done
            && let Some(stripped) = s.strip_suffix('.')
        {
            period_done = true;
            if !period_belongs_to_value(stripped) {
                s = stripped.trim_end();
            }
        }
        if s.len() == before {
            break;
        }
    }
    s.to_string()
}

/// Is the period after `before` part of the value rather than MARC's closing
/// punctuation? See [`trim_isbd_punctuation`].
fn period_belongs_to_value(before: &str) -> bool {
    if before.ends_with('.') {
        return true; // ellipsis
    }
    if before.ends_with(')') || before.ends_with(']') {
        return false;
    }
    let last_token = before.rsplit([' ', ',', '(']).next().unwrap_or(before);
    let is_initial =
        last_token.chars().count() == 1 && last_token.chars().all(char::is_alphabetic);
    // "U.S.A", "Ph.D" — letters and dots only; "2.0" is a number, not an
    // abbreviation, and keeps no closing period.
    let is_dotted = last_token.contains('.')
        && last_token.chars().all(|c| c == '.' || c.is_alphabetic());
    let is_known = KEEP_PERIOD_ABBREVIATIONS
        .iter()
        .any(|a| last_token.eq_ignore_ascii_case(a));
    is_initial || is_dotted || is_known
}

/// Page count from a physical-description extent: UNIMARC 215$a (`"245 p."`,
/// `"XII-245 p."`, `"1 vol. (320 p.)"`) or MARC 21 300$a (`"xx, 392 pages ;"`,
/// `"282 p. ;"`). Lifted from the BnF provider when the Library of Congress
/// needed the same parse (#495, Foundation Rule #1); `page` / `pages` were
/// added for the RDA wording.
pub fn page_count_from_extent(desc: &str) -> Option<i32> {
    // Pattern: a number right before "p" / "p." / "page(s)".
    let words: Vec<&str> = desc.split_whitespace().collect();
    // AACR2 records unnumbered leaves and plates in brackets — "xii, 245, [2]
    // p.", "[16] p. of plates, 245 p." — so an unbracketed sequence anywhere
    // in the extent wins and a bracketed count is only the fallback.
    let mut bracketed = None;
    for (i, word) in words.iter().enumerate() {
        let w = word.trim_end_matches([',', ';', ':', ')']);
        if (w.starts_with("p.") || w == "p" || w == "pages" || w == "page") && i > 0 {
            for prev in words[..i].iter().rev().take(3) {
                let Some(n) = leading_page_number(prev) else {
                    break;
                };
                if prev.starts_with('[') {
                    bracketed.get_or_insert(n);
                } else {
                    return Some(n);
                }
            }
        }
    }
    if bracketed.is_some() {
        return bracketed;
    }
    // Fallback: digits at the start of a parenthesised group, "(245 p.)".
    if let Some(start) = desc.find('(')
        && let Some(end) = desc.find(')')
        && start < end
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
}

/// The page number a single extent token carries: `"245"`, `"245,"`,
/// `"XII-245"` (part after the last hyphen), `"[2]"`. `None` for a token with
/// no digits (`"xii,"`), which ends the backwards scan.
fn leading_page_number(token: &str) -> Option<i32> {
    let num_part = token.rsplit('-').next().unwrap_or(token);
    let cleaned: String = num_part.chars().filter(|c| c.is_ascii_digit()).collect();
    cleaned.parse::<i32>().ok().filter(|n| *n > 0)
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

    // ─── #495 helpers ─────────────────────────────────────────────────

    #[test]
    fn datafields_returns_one_slice_per_field_readable_by_extract_subfield() {
        let fields = datafields(MARC21, "500");
        assert_eq!(fields.len(), 2);
        assert_eq!(
            extract_subfield(fields[1], "500", "a").as_deref(),
            Some(r#""Best practices for the Java Platform" --Cover."#)
        );
        assert!(datafields(MARC21, "700").is_empty());
    }

    #[test]
    fn indicator2_reads_the_opening_tag_whatever_the_attribute_order() {
        let xml = r#"<record>
  <datafield ind1=" " ind2="4" tag="264"><subfield code="c">ind2="1" in text</subfield></datafield>
  <datafield tag="264" ind1=' ' ind2='1'><subfield code="b">Press,</subfield></datafield>
</record>"#;
        let fields = datafields(xml, "264");
        assert_eq!(fields.len(), 2);
        assert_eq!(indicator2(fields[0]), Some('4'), "ind2 before tag= is still seen");
        assert_eq!(indicator2(fields[1]), Some('1'), "single quotes");
        assert_eq!(extract_subfield(fields[1], "264", "b").as_deref(), Some("Press,"));
        assert_eq!(indicator2(r#"<datafield tag="1">"#), None);
    }

    #[test]
    fn controlfield_keeps_positions() {
        let xml = r#"<record><controlfield tag="008">170925t20182018mau      b    001 0 eng d</controlfield></record>"#;
        let f = controlfield(xml, "008").unwrap();
        assert_eq!(&f[7..11], "2018");
        assert_eq!(&f[35..38], "eng");
        assert_eq!(controlfield(xml, "001"), None);
    }

    #[test]
    fn decode_xml_text_handles_numeric_and_named_entities_and_composes() {
        let decoded = decode_xml_text("L'e&#x301;conomie immate&#x301;rielle");
        assert_eq!(decoded, "L'\u{e9}conomie immat\u{e9}rielle");
        assert_eq!(decoded.chars().count(), "L'économie immatérielle".chars().count());
        assert_eq!(decode_xml_text("&#xA9;2018"), "©2018");
        assert_eq!(decode_xml_text("&#233;t&#233;"), "été");
        assert_eq!(decode_xml_text("A &amp; B &lt;C&gt; &quot;D&quot; &apos;E&apos;"), r#"A & B <C> "D" 'E'"#);
    }

    #[test]
    fn decode_xml_text_keeps_malformed_entities_verbatim() {
        assert_eq!(decode_xml_text("Tom & Jerry"), "Tom & Jerry");
        assert_eq!(decode_xml_text("&bogus; &#xZZ; &#;"), "&bogus; &#xZZ; &#;");
        assert_eq!(decode_xml_text("&#x110000;"), "&#x110000;");
        assert_eq!(decode_xml_text("a&#0;b&#x1B;c"), "a&#0;b&#x1B;c", "C0 controls stay verbatim");
        assert_eq!(decode_xml_text("a&#x9;b"), "a\tb");
        // MARC 21 non-sort markers (C1) around a leading article are dropped.
        assert_eq!(decode_xml_text("&#x98;The &#x9C;Hobbit"), "The Hobbit");
        assert_eq!(decode_xml_text("trailing &"), "trailing &");
        assert_eq!(decode_xml_text(""), "");
    }

    #[test]
    fn trim_isbd_punctuation_strips_trailing_marks_but_keeps_initials() {
        assert_eq!(trim_isbd_punctuation("Effective Java /"), "Effective Java");
        assert_eq!(trim_isbd_punctuation("Boston :"), "Boston");
        assert_eq!(trim_isbd_punctuation("Addison-Wesley,"), "Addison-Wesley");
        assert_eq!(trim_isbd_punctuation("Bomsel, Olivier."), "Bomsel, Olivier");
        assert_eq!(trim_isbd_punctuation("Tolkien, J. R. R."), "Tolkien, J. R. R.");
        assert_eq!(trim_isbd_punctuation("Title / ;"), "Title");
        assert_eq!(trim_isbd_punctuation("Equal ="), "Equal");
        assert_eq!(trim_isbd_punctuation("  "), "");
        assert_eq!(trim_isbd_punctuation("."), "");
    }

    #[test]
    fn trim_isbd_punctuation_keeps_periods_and_slashes_that_belong_to_the_value() {
        assert_eq!(trim_isbd_punctuation("U.S.A."), "U.S.A.");
        assert_eq!(trim_isbd_punctuation("Smith, John, Ph.D."), "Smith, John, Ph.D.");
        assert_eq!(trim_isbd_punctuation("Acme Co."), "Acme Co.");
        assert_eq!(trim_isbd_punctuation("Smith, John, Jr.,"), "Smith, John, Jr.");
        assert_eq!(trim_isbd_punctuation("Cats, dogs, etc."), "Cats, dogs, etc.");
        assert_eq!(trim_isbd_punctuation("And then..."), "And then...");
        assert_eq!(trim_isbd_punctuation("AC/DC/"), "AC/DC/");
        assert_eq!(trim_isbd_punctuation("Title :"), "Title");
        assert_eq!(trim_isbd_punctuation("Title."), "Title", "one closing period only");
        assert_eq!(trim_isbd_punctuation("Gallimard, ."), "Gallimard");
        assert_eq!(trim_isbd_punctuation("Title / ."), "Title");
        assert_eq!(trim_isbd_punctuation("Title (2nd ed.)."), "Title (2nd ed.)");
        assert_eq!(trim_isbd_punctuation("Version 2.0."), "Version 2.0");
        assert_eq!(trim_isbd_punctuation("Harvard Univ."), "Harvard Univ.");
        assert_eq!(trim_isbd_punctuation("Title\u{a0}/"), "Title");
    }

    #[test]
    fn page_count_reads_unimarc_and_marc21_extents() {
        assert_eq!(page_count_from_extent("245 p."), Some(245));
        assert_eq!(page_count_from_extent("XII-245 p."), Some(245));
        assert_eq!(page_count_from_extent("1 vol. (320 p.)"), Some(320));
        assert_eq!(page_count_from_extent("xx, 392 pages ;"), Some(392));
        assert_eq!(page_count_from_extent("282 p. ;"), Some(282));
        assert_eq!(page_count_from_extent("1 page ;"), Some(1));
        assert_eq!(page_count_from_extent("1 volume (unpaged) ;"), None);
        assert_eq!(page_count_from_extent(""), None);
        assert_eq!(page_count_from_extent(") weird ("), None);
        // AACR2 unnumbered leaves in brackets after the main sequence.
        assert_eq!(page_count_from_extent("xii, 245, [2] p. :"), Some(245));
        assert_eq!(page_count_from_extent("245, [3] p. ;"), Some(245));
        assert_eq!(page_count_from_extent("[48] p. :"), Some(48));
        assert_eq!(page_count_from_extent("[16] p. of plates, 245 p."), Some(245));
    }
}
