//! Story 13-4 (#578) — provider descriptions as plain text.
//!
//! Google Books and Comic Vine return their `description` as HTML; stored
//! verbatim, the title page showed `<p>`, `<b>` and `&lt;` to the reader.
//! [`html_to_plain_text`] is the one conversion, applied where HTML comes
//! in — the Google Books and Comic Vine parsers, an agent's proposed
//! description at filing — and by the run-once startup repair of the
//! existing rows (`services::description_repair`);
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

/// Longest `&…;` looked at, in bytes (`&` included).
const MAX_ENTITY_LEN: usize = 32;

/// Windows-1252 characters of the C1 range, 0x80–0x9F (HTML5 numeric
/// character reference replacement table); `None` for the five unassigned
/// code points.
fn windows_1252(c: char) -> Option<char> {
    const TABLE: [Option<char>; 32] = [
        Some('\u{20AC}'),
        None,
        Some('\u{201A}'),
        Some('\u{0192}'),
        Some('\u{201E}'),
        Some('\u{2026}'),
        Some('\u{2020}'),
        Some('\u{2021}'),
        Some('\u{02C6}'),
        Some('\u{2030}'),
        Some('\u{0160}'),
        Some('\u{2039}'),
        Some('\u{0152}'),
        None,
        Some('\u{017D}'),
        None,
        None,
        Some('\u{2018}'),
        Some('\u{2019}'),
        Some('\u{201C}'),
        Some('\u{201D}'),
        Some('\u{2022}'),
        Some('\u{2013}'),
        Some('\u{2014}'),
        Some('\u{02DC}'),
        Some('\u{2122}'),
        Some('\u{0161}'),
        Some('\u{203A}'),
        Some('\u{0153}'),
        None,
        Some('\u{017E}'),
        Some('\u{0178}'),
    ];
    let code = c as u32;
    if (0x80..=0x9F).contains(&code) {
        TABLE[(code - 0x80) as usize]
    } else {
        None
    }
}

/// Decode character references once: `&#xHH;`, `&#NN;`, the five named XML
/// entities and, with `html_named`, the HTML named entities above. Anything
/// else (malformed or unknown) is kept verbatim. No Unicode normalisation:
/// callers apply NFC themselves.
///
/// C0 controls other than tab / LF / CR are kept as their reference (never
/// smuggled into the catalogue). A reference into 128–159 is the
/// Windows-1252 character in HTML mode; otherwise (XML mode — MARC 21
/// non-sort markers — and the five unassigned code points) it is dropped.
pub fn decode_char_references(raw: &str, html_named: bool) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let candidate = &rest[amp..];
        // An entity name is short (`&#x10FFFF;`, `&thinsp;`): look for its
        // `;` in the next few bytes only, so a text full of bare `&` is
        // scanned once, not once per `&` (review L1). A farther `;` would
        // make an unknown entity, kept verbatim anyway.
        let window = &candidate.as_bytes()[..candidate.len().min(MAX_ENTITY_LEN)];
        let decoded = window.iter().position(|&b| b == b';').and_then(|semi| {
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
                        // HTML: a reference into 128–159 means the Windows-
                        // 1252 character (`&#146;` is ’), as browsers read it.
                        Some(c) if html_named && windows_1252(c).is_some() => {
                            windows_1252(c).map(Some)
                        }
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

/// The HTML elements a provider description is made of; any other `<name`
/// is text.
const KNOWN_ELEMENTS: &[&str] = &[
    "a",
    "abbr",
    "article",
    "b",
    "big",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "dd",
    "del",
    "div",
    "dl",
    "dt",
    "em",
    "font",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "ins",
    "li",
    "ol",
    "p",
    "pre",
    "q",
    "s",
    "script",
    "section",
    "small",
    "span",
    "strike",
    "strong",
    "style",
    "sub",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "tt",
    "u",
    "ul",
];

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

/// What a `<` opens.
enum TagScan {
    /// A tag ending at this exclusive byte index.
    Tag(usize),
    /// Text: a `<` not followed by a known HTML element name (`/` allowed
    /// before it) then `>`, `/` or whitespace — `a < b`, `<3`, `<<Le>>`,
    /// `<?xml …?>`, `<![CDATA[…]]>` — or a would-be tag interrupted by
    /// another `<`. Comments and `<!DOCTYPE …>` are the only `<!` tags.
    NotATag,
    /// A tag (or comment) the input ends inside: it and everything after it
    /// are text. Stopping there keeps the scan linear — no later `<` is
    /// re-scanned to the end of the input (review L1).
    Unterminated,
}

/// Scan the tag starting at `s[0] == '<'`, honouring quoted attribute
/// values (a `>` inside quotes does not close the tag).
fn tag_end(s: &str) -> TagScan {
    if let Some(comment) = s.strip_prefix("<!--") {
        return match comment.find("-->") {
            Some(i) => TagScan::Tag(4 + i + 3),
            None => TagScan::Unterminated,
        };
    }
    let doctype = s
        .get(..9)
        .is_some_and(|head| head.eq_ignore_ascii_case("<!doctype"));
    if !doctype {
        // Review round 2 (L1): only a known HTML element name opens a tag,
        // so BnF's `<<Le>> Petit Prince` or `<Astérix>` stay text.
        let after_lt = s[1..].strip_prefix('/').unwrap_or(&s[1..]);
        let name_len = after_lt
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric())
            .count();
        let name = &after_lt[..name_len];
        let known = KNOWN_ELEMENTS.iter().any(|k| k.eq_ignore_ascii_case(name));
        let boundary = after_lt[name_len..]
            .chars()
            .next()
            .is_none_or(|c| c == '>' || c == '/' || c.is_ascii_whitespace());
        if !known || !boundary {
            return TagScan::NotATag;
        }
    }
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices().skip(1) {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return TagScan::Tag(i + 1),
            (None, '<') => return TagScan::NotATag,
            _ => {}
        }
    }
    TagScan::Unterminated
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
    let has_tag = has_tag(raw);
    let mut text = String::with_capacity(raw.len());
    let mut skip_until: Option<String> = None;
    let mut rest = raw;
    while let Some(lt) = rest.find('<') {
        if skip_until.is_none() {
            push_source_text(&mut text, &rest[..lt], has_tag);
        }
        let at = &rest[lt..];
        let end = match tag_end(at) {
            TagScan::Tag(end) => end,
            TagScan::NotATag => {
                if skip_until.is_none() {
                    text.push('<');
                }
                rest = &at[1..];
                continue;
            }
            TagScan::Unterminated => {
                rest = at;
                break;
            }
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
    // Review round 2 (L3): `&#127;` or a raw C0/C1 control must not reach
    // the catalogue (an agent's text is refused on one; a converted one
    // would bypass that check). Tab and line breaks are text.
    tidy_lines(&decoded)
        .nfc()
        .filter(|c| !c.is_control() || matches!(c, '\t' | '\n'))
        .collect()
}

/// Does `raw` carry at least one complete tag? Linear: stops at the first
/// tag, or at the first unterminated one (nothing after it is a tag).
fn has_tag(raw: &str) -> bool {
    for (i, _) in raw.match_indices('<') {
        match tag_end(&raw[i..]) {
            TagScan::Tag(_) => return true,
            TagScan::NotATag => {}
            TagScan::Unterminated => return false,
        }
    }
    false
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
    fn an_unterminated_tag_leaves_the_rest_as_text() {
        assert_eq!(
            html_to_plain_text("<b>x</b> <a \"y <i>z</i>"),
            "x <a \"y <i>z</i>"
        );
        assert_eq!(html_to_plain_text("a<!-- b <p>c"), "a<!-- b <p>c");
    }

    /// Review L1 — hostile inputs are scanned in linear time (a quadratic
    /// scan of 200k characters would take minutes, not milliseconds).
    #[test]
    fn hostile_inputs_are_scanned_linearly() {
        let n = 200_000;
        for unit in ["<!--", "&", "<a \"", "&#", "<a <b "] {
            let raw = unit.repeat(n / unit.len());
            let out = html_to_plain_text(&raw);
            assert!(!out.is_empty() || raw.is_empty(), "{unit}");
        }
        let amps = "&".repeat(n);
        assert_eq!(decode_char_references(&amps, false), amps);
    }

    /// Review L2 — HTML numeric references into 128–159 are Windows-1252;
    /// the XML (MARC) mode still drops them.
    #[test]
    fn c1_references_are_windows_1252_in_html_mode() {
        assert_eq!(
            html_to_plain_text("l&#146;eau &#150; &#147;x&#148; &#128;"),
            "l’eau – “x” €"
        );
        // Unassigned in Windows-1252: dropped.
        assert_eq!(html_to_plain_text("a&#129;b"), "ab");
        assert_eq!(decode_char_references("a&#146;b", false), "ab");
    }

    /// Review round 2 (L1) — only known HTML element names are tags.
    #[test]
    fn unknown_element_names_stay_text() {
        assert_eq!(
            html_to_plain_text("<<Le>> Petit Prince"),
            "<<Le>> Petit Prince"
        );
        assert_eq!(
            html_to_plain_text("<Astérix> &amp; <Obélix>"),
            "<Astérix> & <Obélix>"
        );
        assert_eq!(html_to_plain_text("<bible> and <br>x"), "<bible> and\nx");
        assert_eq!(html_to_plain_text("<b>c</b>"), "c");
        // `<b>` is a known element: a tag, even unbalanced.
        assert_eq!(html_to_plain_text("a <b> c"), "a c");
        assert_eq!(html_to_plain_text("<!DOCTYPE html><P CLASS=x>Up</P>"), "Up");
    }

    /// Review round 2 (L3) — no control character comes out.
    #[test]
    fn control_characters_are_dropped() {
        assert_eq!(html_to_plain_text("a&#127;b&#x81;c&#9;d"), "abc\td");
        assert_eq!(html_to_plain_text("<p>a\u{7}b</p>"), "ab");
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
