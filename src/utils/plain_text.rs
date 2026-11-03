//! Story 13-4 (#578) — provider descriptions as plain text.
//!
//! Google Books and Comic Vine return their `description` as HTML; stored
//! verbatim, the title page showed `<p>`, `<b>` and `&lt;` to the reader.
//! [`html_to_plain_text`] is the one conversion, applied at ingestion
//! (`MetadataResult::normalize_empty_strings`), by the startup cleanup of
//! the existing rows and to an agent's proposed description;
//! [`description_paragraphs`] splits the stored plain text for display.
//!
//! [`decode_char_references`] is the entity decoder shared with the MARCXML
//! reader (`metadata::marc::decode_xml_text`).

use unicode_normalization::UnicodeNormalization;

/// HTML named entities of the Latin-1 block, U+00A0 to U+00FF in order.
const LATIN1_ENTITIES: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf",
    "laquo", "not", "shy", "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro",
    "para", "middot", "cedil", "sup1", "ordm", "raquo", "frac14", "frac12", "frac34", "iquest",
    "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig", "Ccedil", "Egrave", "Eacute",
    "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve", "Oacute",
    "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute",
    "THORN", "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil",
    "egrave", "eacute", "ecirc", "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde",
    "ograve", "oacute", "ocirc", "otilde", "ouml", "divide", "oslash", "ugrave", "uacute", "ucirc",
    "uuml", "yacute", "thorn", "yuml",
];

/// Common HTML named entities outside Latin-1: typographic quotes, dashes,
/// spaces, and the Latin Extended letters of French and Danish texts.
const EXTRA_ENTITIES: [(&str, char); 26] = [
    ("ndash", '\u{2013}'),
    ("mdash", '\u{2014}'),
    ("lsquo", '\u{2018}'),
    ("rsquo", '\u{2019}'),
    ("sbquo", '\u{201A}'),
    ("ldquo", '\u{201C}'),
    ("rdquo", '\u{201D}'),
    ("bdquo", '\u{201E}'),
    ("lsaquo", '\u{2039}'),
    ("rsaquo", '\u{203A}'),
    ("hellip", '\u{2026}'),
    ("bull", '\u{2022}'),
    ("dagger", '\u{2020}'),
    ("Dagger", '\u{2021}'),
    ("permil", '\u{2030}'),
    ("prime", '\u{2032}'),
    ("euro", '\u{20AC}'),
    ("trade", '\u{2122}'),
    ("ensp", '\u{2002}'),
    ("emsp", '\u{2003}'),
    ("thinsp", '\u{2009}'),
    ("OElig", '\u{0152}'),
    ("oelig", '\u{0153}'),
    ("Scaron", '\u{0160}'),
    ("scaron", '\u{0161}'),
    ("Yuml", '\u{0178}'),
];

/// The character an HTML named entity (without `&` and `;`) stands for,
/// beyond the five XML ones.
fn html_named_entity(name: &str) -> Option<char> {
    if let Some(i) = LATIN1_ENTITIES.iter().position(|n| *n == name) {
        return char::from_u32(0xA0 + i as u32);
    }
    EXTRA_ENTITIES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

/// Decode character references once: `&#xHH;`, `&#NN;`, the five named XML
/// entities and, with `html_named`, the HTML named entities above. Anything
/// else (malformed or unknown) is kept verbatim. No Unicode normalisation:
/// callers apply NFC themselves.
///
/// C0 controls other than tab / LF / CR are kept as their reference (never
/// smuggled into the catalogue); C1 controls (MARC 21 non-sort markers) are
/// dropped.
pub fn decode_char_references(raw: &str, html_named: bool) -> String {
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
                _ if entity.starts_with('#') => {
                    let code = if let Some(hex) = entity
                        .strip_prefix("#x")
                        .or_else(|| entity.strip_prefix("#X"))
                    {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        entity[1..].parse::<u32>().ok()
                    };
                    match code.and_then(char::from_u32) {
                        Some(c) if c < ' ' && !matches!(c, '\t' | '\n' | '\r') => None,
                        Some(c) if ('\u{80}'..='\u{9f}').contains(&c) => Some(None),
                        other => other.map(Some),
                    }
                }
                _ if html_named => html_named_entity(entity).map(Some),
                _ => None,
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
    out
}

/// What a tag turns into in the plain text.
enum TagBreak {
    None,
    /// `<br>`: always a line break (two in a row make a blank line).
    Line,
    /// A list item or table row boundary: a line break unless the text
    /// already ends one (`</li><li>` is a single break).
    Item,
    Paragraph,
}

/// Elements whose content is never text (dropped with their content).
const SKIPPED_ELEMENTS: [&str; 2] = ["script", "style"];

fn tag_break(name: &str) -> TagBreak {
    match name {
        "br" => TagBreak::Line,
        "li" | "tr" | "dt" | "dd" => TagBreak::Item,
        "p" | "div" | "ul" | "ol" | "dl" | "table" | "blockquote" | "pre" | "h1" | "h2" | "h3"
        | "h4" | "h5" | "h6" | "section" | "article" => TagBreak::Paragraph,
        _ => TagBreak::None,
    }
}

/// End (exclusive byte index into `s`) of the tag starting at `s[0] == '<'`,
/// honouring quoted attribute values (a `>` inside quotes does not close
/// the tag). `None` when `s` does not start a tag: `<` followed by anything
/// but a letter, `/` + letter, `!` or `?` is prose (`a < b`, `<3`), and an
/// unterminated tag is kept as text.
fn tag_end(s: &str) -> Option<usize> {
    let mut chars = s[1..].chars();
    let first = chars.next()?;
    let starts_tag = match first {
        c if c.is_ascii_alphabetic() => true,
        '/' => chars.next().is_some_and(|c| c.is_ascii_alphabetic()),
        '!' | '?' => true,
        _ => false,
    };
    if !starts_tag {
        return None;
    }
    if let Some(comment) = s.strip_prefix("<!--") {
        return comment.find("-->").map(|i| 4 + i + 3);
    }
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices().skip(1) {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i + 1),
            (None, '<') => return None,
            _ => {}
        }
    }
    None
}

/// Lower-cased element name of a tag (`<P class=x>` → `p`, `</li>` → `li`)
/// and whether it is a closing tag.
fn tag_name(tag: &str) -> (String, bool) {
    let inner = &tag[1..];
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let name: String = inner
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    (name, closing)
}

/// Story 13-4 (#578) — turn a provider's HTML description into plain text.
///
/// - tags are removed; `<br>` becomes a line break, a block (`<p>`, `<div>`,
///   headings, lists…) a paragraph break, a list item a line break; the
///   content of `<script>` / `<style>` and comments are dropped;
/// - when the text carries tags, its own line breaks and runs of spaces are
///   layout and collapse to one space (as a browser renders them);
/// - character references are decoded **once**, after the tags are removed
///   (`&lt;b&gt;` stays the text `<b>`, `&amp;lt;` becomes `&lt;`): XML,
///   numeric and the common HTML named ones ([`decode_char_references`]);
/// - each line is trimmed, at most one blank line separates paragraphs, the
///   whole is trimmed and normalised to NFC.
///
/// A `<` that opens no tag (`a < b`, `<3`) is prose and stays. Text without
/// `<` or `&` is returned unchanged, byte for byte.
pub fn html_to_plain_text(raw: &str) -> String {
    if !raw.contains(['<', '&']) {
        return raw.to_string();
    }
    // Whether the text carries markup decides how its own whitespace reads.
    let has_tag = raw
        .match_indices('<')
        .any(|(i, _)| tag_end(&raw[i..]).is_some());
    let mut text = String::with_capacity(raw.len());
    let mut skip_until: Option<String> = None;
    let mut rest = raw;
    while let Some(lt) = rest.find('<') {
        if skip_until.is_none() {
            push_source_text(&mut text, &rest[..lt], has_tag);
        }
        let at = &rest[lt..];
        let Some(end) = tag_end(at) else {
            if skip_until.is_none() {
                text.push('<');
            }
            rest = &at[1..];
            continue;
        };
        let tag = &at[..end];
        rest = &at[end..];
        if tag.starts_with("<!") || tag.starts_with("<?") {
            continue;
        }
        let (name, closing) = tag_name(tag);
        if let Some(open) = &skip_until {
            if closing && name == *open {
                skip_until = None;
            }
            continue;
        }
        if !closing && SKIPPED_ELEMENTS.contains(&name.as_str()) && !tag.ends_with("/>") {
            skip_until = Some(name);
            continue;
        }
        match tag_break(&name) {
            TagBreak::None => {}
            TagBreak::Line => text.push('\n'),
            TagBreak::Item if text.trim_end_matches(' ').ends_with('\n') => {}
            TagBreak::Item => text.push('\n'),
            TagBreak::Paragraph => text.push_str("\n\n"),
        }
    }
    if skip_until.is_none() {
        push_source_text(&mut text, rest, has_tag);
    }

    let decoded = decode_char_references(&text, true);
    tidy_lines(&decoded).nfc().collect()
}

/// Append a run of source text. In HTML (`collapse`), the source's line
/// breaks, tabs and runs of spaces are layout and read as one space, as a
/// browser renders them; only the tags produce line breaks. Without tags
/// (text with entities only), the text keeps its own line breaks.
fn push_source_text(out: &mut String, segment: &str, collapse: bool) {
    if !collapse {
        out.push_str(segment);
        return;
    }
    let mut last_space = out.ends_with(' ');
    for c in segment.chars() {
        if matches!(c, ' ' | '\n' | '\r' | '\t') {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
}

/// Trim every line, drop leading / trailing blank lines and keep at most
/// one blank line in a row.
fn tidy_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_blank = false;
    for line in text.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            pending_blank = !out.is_empty();
            continue;
        }
        if !out.is_empty() {
            out.push_str(if pending_blank { "\n\n" } else { "\n" });
        }
        out.push_str(line);
        pending_blank = false;
    }
    out
}

/// Story 13-4 (#578) — the paragraphs of a stored description for display:
/// one per non-blank line, trimmed. Each is rendered as its own escaped
/// `<p>` (no `|safe`).
pub fn description_paragraphs(description: &str) -> Vec<String> {
    description
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_returned_byte_identical() {
        for s in [
            "Plain text.",
            "  leading and trailing  ",
            "Line one\n\n\n\nLine two",
            "e\u{301}cole (decomposed, no NFC on plain text)",
            "",
        ] {
            assert_eq!(html_to_plain_text(s), s);
        }
    }

    #[test]
    fn the_titeuf_description_reads_as_text() {
        let raw = "<p><b>Grandis un peu, Titeuf !</b></p><p>La vie de Titeuf &eacute;volue.</p>";
        assert_eq!(
            html_to_plain_text(raw),
            "Grandis un peu, Titeuf !\n\nLa vie de Titeuf évolue."
        );
    }

    #[test]
    fn br_is_a_line_break_and_blocks_are_paragraphs() {
        assert_eq!(html_to_plain_text("a<br>b<br/>c<BR />d"), "a\nb\nc\nd");
        assert_eq!(html_to_plain_text("<div>a</div><div>b</div>"), "a\n\nb");
        assert_eq!(
            html_to_plain_text("<ul><li>one</li><li>two</li></ul>after"),
            "one\ntwo\n\nafter"
        );
    }

    #[test]
    fn at_most_one_blank_line_in_a_row() {
        assert_eq!(
            html_to_plain_text("<p>a</p><p></p><br><br><br><p>b</p>"),
            "a\n\nb"
        );
    }

    #[test]
    fn nested_and_malformed_tags() {
        assert_eq!(html_to_plain_text("<p>a <b>b</p>"), "a b");
        assert_eq!(html_to_plain_text("<p><i><b>x</b></i></p>"), "x");
        // An unterminated tag is kept as text.
        assert_eq!(html_to_plain_text("a <b"), "a <b");
    }

    #[test]
    fn a_greater_than_inside_a_quoted_attribute_does_not_close_the_tag() {
        assert_eq!(
            html_to_plain_text(
                r#"<a href="x" title="a > b">link</a> and <span data-x='>'>more</span>"#
            ),
            "link and more"
        );
    }

    #[test]
    fn source_whitespace_in_html_is_layout() {
        assert_eq!(
            html_to_plain_text("<p>A long\n   wrapped\tline</p>\n\n\n<p>Next</p>"),
            "A long wrapped line\n\nNext"
        );
    }

    #[test]
    fn named_numeric_and_xml_entities_are_decoded() {
        assert_eq!(
            html_to_plain_text(
                "&laquo;&nbsp;Caf&eacute;&nbsp;&raquo; &ndash; &#233;t&#xE9; &amp; &quot;x&quot; &hellip;"
            ),
            "«\u{a0}Café\u{a0}» – été & \"x\" …"
        );
        assert_eq!(
            html_to_plain_text("&OElig;uvre &oelig;uf &euro;"),
            "Œuvre œuf €"
        );
    }

    #[test]
    fn entities_are_decoded_once() {
        assert_eq!(html_to_plain_text("a &amp;lt; b"), "a &lt; b");
        // An escaped tag is text, not markup.
        assert_eq!(html_to_plain_text("&lt;b&gt;bold&lt;/b&gt;"), "<b>bold</b>");
    }

    #[test]
    fn unknown_entities_and_bare_ampersands_are_kept() {
        assert_eq!(
            html_to_plain_text("Tom & Jerry &bogus; R&D"),
            "Tom & Jerry &bogus; R&D"
        );
    }

    #[test]
    fn a_less_than_in_prose_is_kept() {
        assert_eq!(html_to_plain_text("a < b and c <3 d"), "a < b and c <3 d");
        assert_eq!(html_to_plain_text("<p>1 < 2</p>"), "1 < 2");
    }

    #[test]
    fn comments_and_script_content_are_dropped() {
        assert_eq!(
            html_to_plain_text(
                "a<!-- hidden <b> -->b<script>var x = '<p>';</script>c<style>p{}</style>d"
            ),
            "abcd"
        );
    }

    #[test]
    fn the_result_is_trimmed_and_nfc() {
        assert_eq!(html_to_plain_text("<p>  e&#x301;cole  </p>"), "école");
    }

    #[test]
    fn markup_only_becomes_empty() {
        assert_eq!(html_to_plain_text("<p><br></p>"), "");
    }

    #[test]
    fn latin1_table_is_complete_and_aligned() {
        assert_eq!(html_named_entity("nbsp"), Some('\u{a0}'));
        assert_eq!(html_named_entity("times"), Some('×'));
        assert_eq!(html_named_entity("szlig"), Some('ß'));
        assert_eq!(html_named_entity("eacute"), Some('é'));
        assert_eq!(html_named_entity("divide"), Some('÷'));
        assert_eq!(html_named_entity("yuml"), Some('ÿ'));
        assert_eq!(html_named_entity("aring"), Some('å'));
        assert_eq!(html_named_entity("oslash"), Some('ø'));
        assert_eq!(html_named_entity("AElig"), Some('Æ'));
        assert_eq!(html_named_entity("nope"), None);
    }

    #[test]
    fn xml_mode_ignores_html_named_entities() {
        assert_eq!(
            decode_char_references("&eacute; &amp;", false),
            "&eacute; &"
        );
        assert_eq!(decode_char_references("&eacute; &amp;", true), "é &");
    }

    #[test]
    fn the_conversion_is_stable_on_its_own_output() {
        let once = html_to_plain_text("<p>Un <b>a</b> &amp; b</p><p>c</p>");
        assert_eq!(html_to_plain_text(&once), once);
    }

    #[test]
    fn paragraphs_split_on_line_breaks() {
        assert_eq!(
            description_paragraphs("First\n\nSecond line\nThird\n"),
            vec!["First", "Second line", "Third"]
        );
        assert!(description_paragraphs("  \n ").is_empty());
        assert_eq!(description_paragraphs("Single"), vec!["Single"]);
    }
}
