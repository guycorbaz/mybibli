//! Story 13-10 (#579) — the tome number.
//!
//! The number stays on `title_series.position_number` (the only
//! authoritative value) but behaves as the title's own: a tome marker
//! ("Tome 3 : …", "Titeuf T12 : …", "… n°12") never stays in a title a
//! metadata provider wrote, the number shows as "Tome N" everywhere, and
//! it is edited from the title page.
//!
//! * [`split_tome_marker`] — the pure, conservative marker parser.
//! * [`prepare_metadata_title`] + [`record_detected_tome`] — ingestion:
//!   the provider title is stripped; the number goes to the
//!   `titles.detected_tome` hint when the title is in no series, and is
//!   only logged when it already is (the position there was set by a
//!   person or an accepted proposal and is never changed silently).
//! * [`change_position`] — the position edit of one membership.
//! * Labels: [`tome_label`], [`positions_label`], [`detected_label`].

use std::collections::HashSet;

use crate::db::DbPool;
use crate::error::AppError;
use crate::metadata::provider::MetadataResult;
use crate::models::series::SeriesType;
use crate::models::title::TitleModel;
pub use crate::models::title_tome::{FIELD_SEP, MEMBERSHIP_SEP, TomeMembership};
use crate::models::title_tome::{TitleTomeModel, TwinRow};

/// Largest tome number the parser accepts.
pub const MAX_TOME: i32 = 10_000;

/// Characters that separate a marker from the rest of a title.
const SEPARATORS: [char; 7] = ['-', '\u{2013}', '\u{2014}', ':', ',', '.', ';'];

/// Which markers a media type recognises (code review M1): the short
/// ones ("T12", "No 7", "n° 9", "#3") are comic conventions and read as
/// false positives elsewhere ("Symphonie n° 9", "Aktion T4", "Chanel
/// N°5"); a book only knows the long words; music, films, magazines and
/// reports know none (a magazine's number is its `issue_number`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerSet {
    /// `bd`: every marker.
    Comic,
    /// `book`: Tome / Tomo / Volume / Vol. / Band.
    Book,
}

impl MarkerSet {
    /// The set of a media type; `None` — never parsed.
    pub fn for_media_type(media_type: &str) -> Option<Self> {
        match media_type {
            "bd" => Some(Self::Comic),
            "book" => Some(Self::Book),
            _ => None,
        }
    }
}

/// How the `.` after a marker word is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dot {
    /// No `.` ("Tome 3").
    Never,
    /// `.` optional ("Bd 5", "Bd. 5").
    Optional,
    /// `.` required: "Vol. 3" (a bare "Vol 93" is a flight).
    Required,
    /// `T`: "T12", "T.12", "T. 12" — but not "T 34" (a tank).
    CompactOrDotted,
}

/// Marker words, longest first where one is a prefix of another, with
/// their `.` rule and whether only a comic knows them.
const MARKER_WORDS: [(&str, Dot, bool); 9] = [
    ("tome", Dot::Never, false),
    ("tomo", Dot::Never, false),
    ("volume", Dot::Never, false),
    ("band", Dot::Never, false),
    ("vol", Dot::Required, false),
    ("bd", Dot::Optional, true),
    ("no", Dot::Optional, true),
    ("t", Dot::CompactOrDotted, true),
    ("#", Dot::Never, true),
];

/// Words that cannot stand for the series name before a marker ("Le
/// Tome 2 : la suite", "Ce tome 2 : …" are sentences, not volume 2 of
/// "Le" or "Ce").
const DETERMINERS: [&str; 43] = [
    "le", "la", "les", "l'", "un", "une", "des", "du", "de", "ce", "cet", "cette", "son", "sa",
    "ses", "mon", "ma", "mes", "ton", "ta", "tes", "notre", "votre", "leur", "leurs", "the", "a",
    "an", "this", "that", "my", "his", "her", "their", "der", "die", "das", "dem", "den", "ein",
    "eine", "il", "lo",
];

/// [`split_tome_marker`] for a title of `media_type` (unchanged for a
/// media type that knows no marker).
pub fn split_for_media_type(raw: &str, media_type: &str) -> (String, Option<i32>) {
    match MarkerSet::for_media_type(media_type) {
        Some(set) => split_tome_marker(raw, set),
        None => (raw.to_string(), None),
    }
}

/// Split a tome marker off a title (pure — unit tested over a table of
/// real titles). Recognised, case-insensitive, by every [`MarkerSet`]:
/// `Tome 12`, `Tomo 12`, `Volume 12`, `Band 12`, `Vol. 12`; by
/// [`MarkerSet::Comic`] only: `T12`, `T.12`, `T. 12`, `Bd. 12`, `Bd 12`,
/// `n° 12`, `No 12`, `No. 12`, `#12`; also between parentheses or
/// brackets; number 1..=[`MAX_TOME`].
///
/// Conservative — a false positive rewrites a title:
/// * the marker is a whole token: preceded by the start or a space;
/// * it is followed by the end of the title or by a separator (` - `,
///   ` : `, `, `, `. `, ` ; `, dashes) itself followed by a space or the
///   end — so "Vol 714 pour Sydney", "Tome 2-3", "T12.5" stay untouched;
/// * what is left must hold a letter or a digit, and the word before a
///   marker inside the title cannot be an article ("Le Tome 2 : …").
///
/// Only the marker and its adjacent separators are removed: "Thorgal -
/// Tome 29 - Le sacrifice" → "Thorgal - Le sacrifice"; "Tome 1 : La
/// Prophétie" → "La Prophétie"; "Les Tuniques bleues n°12" → "Les
/// Tuniques bleues". A bare leading "12." is never a marker ("2001. …").
/// Without a marker the title comes back unchanged and the number `None`.
pub fn split_tome_marker(raw: &str, set: MarkerSet) -> (String, Option<i32>) {
    let s = raw.trim();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    // Every marker of the set, as (start, end, number, matched word,
    // between brackets).
    let mut hits = Vec::new();
    for (k, &(i, c)) in chars.iter().enumerate() {
        if k > 0 && !chars[k - 1].1.is_whitespace() {
            continue;
        }
        if c == '(' || c == '[' {
            let close = if c == '(' { ')' } else { ']' };
            let inner = i + c.len_utf8();
            let Some((len, n, word)) = match_marker(&s[inner..], set) else {
                continue;
            };
            let after = s[inner + len..].trim_start();
            if after.starts_with(close) {
                let end = s.len() - after.len() + close.len_utf8();
                hits.push((i, end, n, word, true));
            }
        } else if let Some((len, n, word)) = match_marker(&s[i..], set) {
            hits.push((i, i + len, n, word, false));
        }
    }
    // Review round 2 (M1): two markers ("Intégrale T1 à T3", "T1, T2,
    // T3", "Volume 1 - Volume 2") name a range or a list, not this
    // title's number — the title is left alone.
    if let [(start, end, n, word, bracketed)] = hits.as_slice() {
        let marker = MarkerContext {
            needs_left_sep: LEFT_SEP_WORDS.contains(word),
            bare_t: *word == "t" && !bracketed,
        };
        if let Some(cleaned) = shape(&s[..*start], &s[*end..], marker) {
            return (cleaned, Some(*n));
        }
    }
    (raw.to_string(), None)
}

/// Marker words that are also common words ("Rock Band 2", "Boy Band
/// 3"): accepted only at the start of the title or after a separator
/// ("Die Chroniken, Band 2").
const LEFT_SEP_WORDS: [&str; 2] = ["band", "bd"];

/// Words that, right before a marker, join it to another number ("Livre
/// 1 et Tome 2", "Intégrale à Tome 3"). The dash is a separator, not a
/// connector: "T1 - T2" is caught by the two-marker rule.
const CONNECTORS: [&str; 9] = ["&", "à", "a", "et", "and", "to", "und", "bis", "au"];

/// A marker at the start of `t`: bytes consumed and the number.
fn match_marker(t: &str, set: MarkerSet) -> Option<(usize, i32, &'static str)> {
    let comic = set == MarkerSet::Comic;
    let mut rest = None;
    let mut matched = "n\u{b0}";
    for (word, dot, comic_only) in MARKER_WORDS {
        if comic_only && !comic {
            continue;
        }
        if let Some(after) = strip_prefix_ignore_ascii_case(t, word) {
            let dotted = after.strip_prefix('.');
            let after = match (dot, dotted) {
                (Dot::Never, _) => after,
                (Dot::Optional, d) => d.unwrap_or(after),
                (Dot::Required, Some(d)) => d,
                (Dot::Required, None) => return None,
                (Dot::CompactOrDotted, Some(d)) => d,
                (Dot::CompactOrDotted, None) if after.starts_with(|c: char| c.is_ascii_digit()) => {
                    after
                }
                (Dot::CompactOrDotted, None) => return None,
            };
            rest = Some(after);
            matched = word;
            break;
        }
    }
    // "n°" / "nº" (any case of the n), comics only.
    if rest.is_none()
        && comic
        && let Some(after) = strip_prefix_ignore_ascii_case(t, "n")
        && let Some(after) = after
            .strip_prefix('\u{b0}')
            .or_else(|| after.strip_prefix('\u{ba}'))
    {
        rest = Some(after);
    }
    let after_word = rest?;
    let digits_start = after_word.trim_start();
    let digit_len = digits_start
        .bytes()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digit_len == 0 || digit_len > 5 {
        return None;
    }
    let tail = &digits_start[digit_len..];
    match tail.chars().next() {
        None => {}
        Some(ch) if ch.is_whitespace() || SEPARATORS.contains(&ch) || ch == ')' || ch == ']' => {}
        Some(_) => return None,
    }
    let n: i32 = digits_start[..digit_len].parse().ok()?;
    if !(1..=MAX_TOME).contains(&n) {
        return None;
    }
    Some((t.len() - tail.len(), n, matched))
}

/// `t` without the ASCII `prefix`, compared case-insensitively.
fn strip_prefix_ignore_ascii_case<'a>(t: &'a str, prefix: &str) -> Option<&'a str> {
    let head = t.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &t[prefix.len()..])
}

/// What [`shape`] needs to know of the marker it frames.
#[derive(Debug, Clone, Copy)]
struct MarkerContext {
    /// "Band" / "Bd.": only at the start or after a separator.
    needs_left_sep: bool,
    /// A bare "T12" (not bracketed): never at the end without a
    /// separator before it ("Opération T4", "Aktion T4").
    bare_t: bool,
}

/// The separators a shape accepts around a marker (review round 3).
const SHAPE_SEPARATORS: [char; 5] = [':', '-', '\u{2013}', '.', ','];

/// Review round 3 (M1) — the parser accepts known shapes only; anything
/// else leaves the title untouched (a false negative costs a hand edit, a
/// false positive rewrites a title for good). With `MARKER N` the single
/// marker of the title:
///
/// * (A) leading — `MARKER N SEP REST`;
/// * (B) trailing — `LEFT [SEP] MARKER N` (a trailing `.` is dropped);
/// * (C) middle — `LEFT [SEP] MARKER N SEP REST`.
///
/// `SEP` is one of `:` `-` `–` `.` `,`; a dash stands between spaces,
/// the others are followed by a space. `REST` starts with a letter (after
/// optional opening quotes) — never a digit, so "Tome 1 - 2", "T1, 2 et
/// 3", "T. 3 - 1917" (a range or a list) stay untouched. `LEFT` holds a
/// letter, ends with a letter or a closing quote / bracket, and its last
/// word is neither an article nor a connector; a `.` before the marker
/// is a separator only after a word of three letters or more ("Dr. No 1",
/// "Mademoiselle J. T.3", "Le Spirou de... T4" stay untouched). The left
/// separator is optional in (C) so the issue's own "Titeuf T12 : …"
/// shape is kept.
fn shape(left_raw: &str, right_raw: &str, marker: MarkerContext) -> Option<String> {
    // Right side: end of title, or SEP + REST.
    let right = right_raw.trim_start();
    let (right_sep, rest) = match right.chars().next() {
        None => (None, ""),
        Some(c) if SHAPE_SEPARATORS.contains(&c) => {
            if (c == '-' || c == '\u{2013}') && !right_raw.starts_with(char::is_whitespace) {
                return None;
            }
            let after = &right[c.len_utf8()..];
            if !(after.is_empty() || after.starts_with(char::is_whitespace)) {
                return None;
            }
            (Some(c), after.trim_start())
        }
        Some(_) => return None,
    };
    if !rest.is_empty() {
        let first = rest
            .trim_start_matches(['"', '\'', '\u{ab}', '\u{201c}', '\u{2018}', ' '])
            .chars()
            .next()?;
        if !first.is_alphabetic() {
            return None;
        }
    }
    // Left side: start of title, or LEFT [SEP].
    let left = left_raw.trim_end();
    let (left_core, left_sep) = match left.chars().last() {
        Some(c) if SHAPE_SEPARATORS.contains(&c) => {
            let core = &left[..left.len() - c.len_utf8()];
            if (c == '-' || c == '\u{2013}') && !core.ends_with(char::is_whitespace) {
                return None;
            }
            (core.trim_end(), Some(c))
        }
        _ => (left, None),
    };
    if !left_core.is_empty() {
        let last = left_core.chars().last()?;
        let closing = ['"', '\'', '\u{bb}', '\u{201d}', '\u{2019}', ')', ']'];
        if !(last.is_alphabetic() || closing.contains(&last))
            || !left_core.chars().any(char::is_alphabetic)
        {
            return None;
        }
        let last_word = left_core
            .rsplit(char::is_whitespace)
            .next()
            .unwrap_or("")
            .to_lowercase();
        if DETERMINERS.contains(&last_word.as_str()) || CONNECTORS.contains(&last_word.as_str()) {
            return None;
        }
        // Review round 3 (L1): an abbreviation's dot is not a separator.
        if left_sep == Some('.') && last_word.chars().count() <= 2 {
            return None;
        }
        if left_sep.is_none() && (marker.needs_left_sep || (marker.bare_t && rest.is_empty())) {
            return None;
        }
    }
    let cleaned = match (left_core.is_empty(), rest.is_empty()) {
        (true, true) => return None,
        (true, false) => rest.to_string(),
        (false, true) => left_core.to_string(),
        (false, false) => {
            let sep = right_sep.unwrap_or('-');
            let sep = if sep == ',' || sep == '.' {
                format!("{sep} ")
            } else {
                format!(" {sep} ")
            };
            format!("{left_core}{sep}{rest}")
        }
    };
    Some(cleaned)
}

/// A provider's own tome number (pure — unit tested): the UNIMARC 225$v
/// / MARC 490$v `collection_number` of a comic (`bd`), when it is a plain
/// integer in range. Only for comics: for a book that zone is most often
/// a publisher's collection number ("Folio 1234"), not a tome.
/// `issue_number` is a magazine's own field and is never read here.
pub fn provider_tome_number(metadata: &MetadataResult, media_type: &str) -> Option<i32> {
    if media_type != "bd" {
        return None;
    }
    let raw = metadata.collection_number.as_deref()?.trim();
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) || raw.len() > 5 {
        return None;
    }
    raw.parse::<i32>()
        .ok()
        .filter(|n| (1..=MAX_TOME).contains(n))
}

/// Strip the marker from the provider title in `metadata` (pure — unit
/// tested) and return the tome number it carried. Choice (#579): a
/// marker in the title wins over a provider number; without a marker, a
/// comic's plain-integer `collection_number` is used
/// ([`provider_tome_number`]). Media types without a [`MarkerSet`] are
/// left alone.
pub fn strip_metadata_title(metadata: &mut MetadataResult, media_type: &str) -> Option<i32> {
    let parsed = match metadata.title.as_deref() {
        Some(t) => {
            let (cleaned, n) = split_for_media_type(t, media_type);
            if n.is_some() {
                metadata.title = Some(cleaned);
            }
            n
        }
        None => None,
    };
    parsed.or_else(|| provider_tome_number(metadata, media_type))
}

/// Ingestion (pure — unit tested): [`strip_metadata_title`], then the
/// number to record — `None` when the title is hand-edited (`title` in
/// `manually_edited_fields`: it is never rewritten, so its number is not
/// routed either). The stripped value still serves as the proposal of a
/// conflict form.
pub fn prepare_metadata_title(metadata: &mut MetadataResult, snapshot: &TitleModel) -> Option<i32> {
    let number = strip_metadata_title(metadata, &snapshot.media_type);
    let guarded: HashSet<String> = snapshot
        .parsed_manually_edited_fields()
        .into_iter()
        .collect();
    if guarded.contains("title") {
        return None;
    }
    number
}

/// Re-download confirmation (code review L3, pure — unit tested): the
/// number the preview found is recorded only when the title saved is the
/// cleaned provider title the form proposed.
pub fn confirmed_tome(final_title: &str, proposed_title: &str, number: Option<i32>) -> Option<i32> {
    let n = number.filter(|n| (1..=MAX_TOME).contains(n))?;
    (!proposed_title.trim().is_empty() && final_title == proposed_title).then_some(n)
}

/// Route a number found at ingestion (best effort — a failure is logged,
/// never returned): stored as the `detected_tome` hint when the title is
/// in no live series; when it is, nothing changes (the position there
/// was set by a person or an accepted proposal) and a differing number
/// is only logged at debug.
pub async fn record_detected_tome(pool: &DbPool, title_id: u64, number: i32) {
    let held = match TitleTomeModel::held_positions(pool, title_id).await {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!(error = %e, title_id, "Tome number routing: membership read failed");
            return;
        }
    };
    if !held.is_empty() {
        if !held.contains(&number) {
            tracing::debug!(
                title_id,
                detected = number,
                positions = ?held,
                "Tome number from metadata differs from the series position; position kept"
            );
        }
        return;
    }
    match TitleTomeModel::set_detected_tome(pool, title_id, number).await {
        Ok(()) => tracing::debug!(title_id, detected = number, "Detected tome number recorded"),
        Err(e) => tracing::warn!(error = %e, title_id, "Detected tome number write failed"),
    }
}

/// [`confirmed_tome`] then [`record_detected_tome`] (re-download, review
/// L3): called once the title is saved.
pub async fn record_confirmed_tome(
    pool: &DbPool,
    title_id: u64,
    final_title: &str,
    proposed_title: Option<&str>,
    number: Option<i32>,
) {
    if let Some(n) = confirmed_tome(final_title, proposed_title.unwrap_or(""), number) {
        record_detected_tome(pool, title_id, n).await;
    }
}

/// "Tome N" / "Tomes N–M" for one membership (pure — unit tested).
pub fn tome_label(start: i32, end: Option<i32>, loc: &str) -> String {
    match end {
        Some(e) if e != start => rust_i18n::t!(
            "tome.range",
            locale = loc,
            start = start.to_string(),
            end = e.to_string()
        )
        .to_string(),
        _ => rust_i18n::t!("tome.single", locale = loc, n = start.to_string()).to_string(),
    }
}

/// The label of a run of positions in ONE series (pure): "Tome 3",
/// "Tomes 3–5" (a run), "Tomes 3, 12" (otherwise); `None` when empty.
pub fn series_positions_label(positions: &[i32], loc: &str) -> Option<String> {
    let mut p: Vec<i32> = positions.to_vec();
    p.sort_unstable();
    p.dedup();
    let (first, last) = (*p.first()?, *p.last()?);
    if p.len() == 1 {
        return Some(tome_label(first, None, loc));
    }
    if (last - first) as usize + 1 == p.len() {
        return Some(tome_label(first, Some(last), loc));
    }
    let list = p
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Some(rust_i18n::t!("tome.several", locale = loc, list = list).to_string())
}

/// The label of every membership of a title (pure — unit tested), for
/// list rows and the volume page. Positions are grouped per series
/// (review L5): one series → "Tome 3" / "Tomes 3–5"; several → "Tome 3 ·
/// A, Tome 4 · B" (series in the given order); `None` in no series.
pub fn positions_label(memberships: &[TomeMembership], loc: &str) -> Option<String> {
    let mut groups: Vec<(u64, &str, Vec<i32>)> = Vec::new();
    for m in memberships {
        match groups.iter_mut().find(|g| g.0 == m.series_id) {
            Some(g) => g.2.push(m.position),
            None => groups.push((m.series_id, m.series_name.as_str(), vec![m.position])),
        }
    }
    match groups.as_slice() {
        [] => None,
        [(_, _, p)] => series_positions_label(p, loc),
        many => Some(
            many.iter()
                .filter_map(|(_, name, p)| {
                    series_positions_label(p, loc).map(|l| format!("{l} \u{b7} {name}"))
                })
                .collect::<Vec<_>>()
                .join(", "),
        ),
    }
}

/// "Tome N (detected)" for the hint (pure).
pub fn detected_label(n: i32, loc: &str) -> String {
    rust_i18n::t!("tome.detected", locale = loc, n = n.to_string()).to_string()
}

/// Parse the memberships a list query selects (pure — unit tested):
/// [`MEMBERSHIP_SEP`]-separated `series_id` [`FIELD_SEP`] `position`
/// [`FIELD_SEP`] `series name` records; malformed records are skipped.
pub fn parse_memberships(raw: Option<&str>) -> Vec<TomeMembership> {
    raw.unwrap_or("")
        .split(MEMBERSHIP_SEP)
        .filter_map(|rec| {
            let mut f = rec.splitn(3, FIELD_SEP);
            Some(TomeMembership {
                series_id: f.next()?.parse().ok()?,
                position: f.next()?.parse().ok()?,
                series_name: f.next()?.to_string(),
            })
        })
        .collect()
}

/// The positions label of a title (volume page).
pub async fn title_positions_label(
    pool: &DbPool,
    title_id: u64,
    loc: &str,
) -> Result<Option<String>, AppError> {
    let held = TitleTomeModel::memberships(pool, title_id).await?;
    Ok(positions_label(&held, loc))
}

/// AC6 — a hand-typed title is never rewritten; when it holds a marker,
/// the librarian is told how to move the number (pure — unit tested).
pub fn manual_title_hint(title: &str, media_type: &str, loc: &str) -> Option<String> {
    let (_, n) = split_for_media_type(title, media_type);
    n.map(|n| {
        rust_i18n::t!(
            "tome.manual_hint",
            locale = loc,
            label = tome_label(n, None, loc)
        )
        .to_string()
    })
}

/// [`manual_title_hint`] as an info feedback entry (empty without one).
pub fn manual_title_hint_html(title: &str, media_type: &str, loc: &str) -> String {
    manual_title_hint(title, media_type, loc)
        .map(|m| crate::utils::feedback_html("info", &m, ""))
        .unwrap_or_default()
}

/// What the title page shows of the tome number besides each series
/// membership (whose label comes from `TitleSeriesAssignment`).
#[derive(Debug, Clone, Default)]
pub struct TitleTomeView {
    /// "Tome N (detected)" — only when the title is in no series.
    pub detected_label: Option<String>,
    /// Pre-fill of `#assign-position` (the hint), only in no series.
    pub prefill: Option<i32>,
    pub label_change: String,
    pub label_new_position: String,
    pub label_save: String,
}

impl TitleTomeView {
    /// Build the view (pure — unit tested).
    pub fn new(detected: Option<i32>, in_series: bool, loc: &str) -> Self {
        let detected = detected.filter(|_| !in_series);
        Self {
            detected_label: detected.map(|n| detected_label(n, loc)),
            prefill: detected,
            label_change: rust_i18n::t!("tome.change_position", locale = loc).to_string(),
            label_new_position: rust_i18n::t!("tome.new_position", locale = loc).to_string(),
            label_save: rust_i18n::t!("tome.save_position", locale = loc).to_string(),
        }
    }

    /// Load the hint and build the view.
    pub async fn load(
        pool: &DbPool,
        title_id: u64,
        in_series: bool,
        loc: &str,
    ) -> Result<Self, AppError> {
        let detected = TitleTomeModel::detected_tome(pool, title_id).await?;
        Ok(Self::new(detected, in_series, loc))
    }
}

/// AC5 — move one membership of a title to `new_position`, with the
/// validations of a hand assignment (`SeriesService::assign_title`):
/// ≥ 1, ≤ the total of a closed series, not a position the title already
/// holds in the series. Optimistic lock on `title_series.version`. An
/// omnibus range is not edited here (remove and re-assign it). After the
/// commit, expected tomes are re-read with the series rules
/// (`ExpectedTomeService::settle_for_title_id`: a tome of the new position
/// carrying the title's ISBN is recorded as settled; a tome settled at the
/// old position reads as a gap again by definition).
pub async fn change_position(
    pool: &DbPool,
    title_id: u64,
    assignment_id: u64,
    version: i32,
    new_position: i32,
    loc: &str,
) -> Result<(), AppError> {
    if new_position < 1 {
        return Err(AppError::BadRequest(
            rust_i18n::t!("series.position_invalid", locale = loc).to_string(),
        ));
    }
    let not_found =
        || AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string());
    let mut tx = pool.begin().await?;
    // Lock order of every series writer (review L4): the series row, then
    // its `title_series` rows, then (in the settle that follows) its tomes.
    let series_id = TitleTomeModel::membership_series_in(&mut tx, title_id, assignment_id)
        .await?
        .ok_or_else(not_found)?;
    let series =
        crate::models::expected_tome::ExpectedTomeModel::lock_series_in(&mut tx, series_id)
            .await?
            .ok_or_else(not_found)?;
    let current = TitleTomeModel::lock_membership_in(&mut tx, title_id, assignment_id)
        .await?
        .filter(|m| m.series_id == series_id)
        .ok_or_else(not_found)?;
    if current.version != version {
        return Err(AppError::VersionMismatch {
            entity: "title_series",
        });
    }
    if current.is_omnibus {
        return Err(AppError::BadRequest(
            rust_i18n::t!("tome.omnibus_not_editable", locale = loc).to_string(),
        ));
    }
    if current.position == new_position {
        return Ok(());
    }
    if series.series_type == SeriesType::Closed
        && let Some(total) = series.total_volume_count
        && new_position > total
    {
        return Err(AppError::BadRequest(
            rust_i18n::t!(
                "series.position_exceeds_total",
                locale = loc,
                position = new_position,
                total = total
            )
            .to_string(),
        ));
    }
    let rows =
        match TitleTomeModel::lock_twin_in(&mut tx, title_id, current.series_id, new_position)
            .await?
        {
            TwinRow::Live => {
                return Err(AppError::BadRequest(
                    rust_i18n::t!(
                        "series.position_taken",
                        locale = loc,
                        position = new_position
                    )
                    .to_string(),
                ));
            }
            TwinRow::Deleted(twin) => {
                TitleTomeModel::move_onto_twin_in(&mut tx, assignment_id, version, twin).await?
            }
            TwinRow::None => {
                TitleTomeModel::move_in(&mut tx, assignment_id, version, new_position).await?
            }
        };
    crate::services::locking::check_update_result(rows, "title_series")?;
    tx.commit().await?;
    tracing::info!(
        title_id,
        assignment_id,
        from = current.position,
        to = new_position,
        "Series position changed from the title page"
    );
    crate::services::expected_tomes::ExpectedTomeService::settle_for_title_id(pool, title_id).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (input, expected title, expected number) — real comic titles.
    const CASES: &[(&str, &str, Option<i32>)] = &[
        // Markers at the start.
        ("Tome 1 : La Prophétie", "La Prophétie", Some(1)),
        (
            "Tome 12 - Le Cadeau de César",
            "Le Cadeau de César",
            Some(12),
        ),
        ("T2 - Le retour", "Le retour", Some(2)),
        (
            "T.3 : La Nuit des cosmonautes",
            "La Nuit des cosmonautes",
            Some(3),
        ),
        ("Vol. 4 : Les Larmes", "Les Larmes", Some(4)),
        ("Band 7: Die Wüste", "Die Wüste", Some(7)),
        ("#1 : Le commencement", "Le commencement", Some(1)),
        ("TOME 3 : X", "X", Some(3)),
        // Markers in the middle.
        (
            "Thorgal - Tome 29 - Le sacrifice",
            "Thorgal - Le sacrifice",
            Some(29),
        ),
        (
            "Titeuf T12 : Bienvenue en adolescence !",
            "Titeuf : Bienvenue en adolescence !",
            Some(12),
        ),
        (
            "Blake et Mortimer, tome 5 : Le Piège diabolique",
            "Blake et Mortimer : Le Piège diabolique",
            Some(5),
        ),
        (
            "Astérix - T. 12 - Astérix aux Jeux olympiques",
            "Astérix - Astérix aux Jeux olympiques",
            Some(12),
        ),
        (
            "Lucky Luke T.7, L'Évasion des Dalton",
            "Lucky Luke, L'Évasion des Dalton",
            Some(7),
        ),
        // Round 3: a REST starting with a digit is the range shape ("T1 -
        // 3"), indistinguishable from this year — untouched.
        (
            "L'Homme de l'année T. 3 - 1917",
            "L'Homme de l'année T. 3 - 1917",
            None,
        ),
        // Markers at the end.
        ("Les Tuniques bleues n°12", "Les Tuniques bleues", Some(12)),
        (
            "Les Tuniques bleues - N° 12",
            "Les Tuniques bleues",
            Some(12),
        ),
        ("Les Schtroumpfs nº 5", "Les Schtroumpfs", Some(5)),
        ("One Piece Vol. 100", "One Piece", Some(100)),
        ("Naruto, Volume 3", "Naruto", Some(3)),
        ("Die Schlümpfe, Bd. 5", "Die Schlümpfe", Some(5)),
        ("Die Schlümpfe Bd. 5", "Die Schlümpfe Bd. 5", None),
        ("Tex Willer, tomo 3", "Tex Willer", Some(3)),
        ("Batman #12", "Batman", Some(12)),
        ("Spirou et Fantasio - T05", "Spirou et Fantasio", Some(5)),
        // Round 3: a bare trailing "T19" without a separator before it is
        // refused, so "Opération T4" / "Aktion T4" stay untouched; the
        // separated form still splits.
        ("Gaston T19.", "Gaston T19.", None),
        ("Gaston - T19.", "Gaston", Some(19)),
        ("Opération T4", "Opération T4", None),
        ("Calvin and Hobbes No. 2", "Calvin and Hobbes", Some(2)),
        ("Astérix (Tome 12)", "Astérix", Some(12)),
        ("Yoko Tsuno [T. 4]", "Yoko Tsuno", Some(4)),
        (
            "XIII - Tome 13 : The XIII Mystery",
            "XIII : The XIII Mystery",
            Some(13),
        ),
        ("  Tome 3 : Le Mystère  ", "Le Mystère", Some(3)),
        // Numbers that are part of the name, or no marker: untouched.
        ("1984", "1984", None),
        ("Apollo 13", "Apollo 13", None),
        ("Les 4 As", "Les 4 As", None),
        ("Spirou et Fantasio 12", "Spirou et Fantasio 12", None),
        ("Les 7 boules de cristal", "Les 7 boules de cristal", None),
        ("XIII", "XIII", None),
        ("Tome et Janry", "Tome et Janry", None),
        ("Le Tome", "Le Tome", None),
        ("Tome", "Tome", None),
        ("Tomes et tomettes", "Tomes et tomettes", None),
        ("Astérix T", "Astérix T", None),
        ("Vol 714 pour Sydney", "Vol 714 pour Sydney", None),
        ("Le Tome 2 de ma vie", "Le Tome 2 de ma vie", None),
        ("Le Tome 2 : la suite", "Le Tome 2 : la suite", None),
        ("T2 Trainspotting", "T2 Trainspotting", None),
        ("Terminator 2", "Terminator 2", None),
        ("Fahrenheit 451", "Fahrenheit 451", None),
        ("Tome 2-3", "Tome 2-3", None),
        ("Vol. 2.1 : Préquelle", "Vol. 2.1 : Préquelle", None),
        ("Tome 12 bis", "Tome 12 bis", None),
        ("Tome 0 : Prologue", "Tome 0 : Prologue", None),
        ("Tome 3", "Tome 3", None),
        ("#1", "#1", None),
        ("T-34 : Le char", "T-34 : Le char", None),
        ("Tome 123456 : Trop", "Tome 123456 : Trop", None),
        (
            "2001. L'Odyssée de l'espace",
            "2001. L'Odyssée de l'espace",
            None,
        ),
        ("Lot 12 : Inventaire", "Lot 12 : Inventaire", None),
        ("Catch-22", "Catch-22", None),
        // Review L1 — a determiner before a mid-title marker.
        ("Ce tome 2 : x", "Ce tome 2 : x", None),
        ("Cette Tome 2 : x", "Cette Tome 2 : x", None),
        ("Son Tome 3 - Le retour", "Son Tome 3 - Le retour", None),
        ("Their Volume 2: Rise", "Their Volume 2: Rise", None),
        ("Dem Band 4 : Ende", "Dem Band 4 : Ende", None),
        (
            "T 34 : un char de légende",
            "T 34 : un char de légende",
            None,
        ),
        ("Vol 93 : le récit", "Vol 93 : le récit", None),
        // Review round 2 (M1) — two markers or a connector: untouched.
        ("Intégrale T1 à T3", "Intégrale T1 à T3", None),
        ("T1 & T2", "T1 & T2", None),
        ("T1, T2, T3", "T1, T2, T3", None),
        (
            "Thorgal - Tome 1 - Tome 2",
            "Thorgal - Tome 1 - Tome 2",
            None,
        ),
        ("Le Scorpion T1 - T2", "Le Scorpion T1 - T2", None),
        ("Intégrale et T3 : fin", "Intégrale et T3 : fin", None),
        ("Livre 1 & Tome 2 : x", "Livre 1 & Tome 2 : x", None),
        ("Tome 1 à 3 : Intégrale", "Tome 1 à 3 : Intégrale", None),
        // Review round 3 (M1) — a range or a list with bare numbers.
        ("L'Intégrale T1 - 3", "L'Intégrale T1 - 3", None),
        (
            "Thorgal Intégrale Tome 1 - 3",
            "Thorgal Intégrale Tome 1 - 3",
            None,
        ),
        ("Coffret Tome 1, 2, 3", "Coffret Tome 1, 2, 3", None),
        ("Spirou T1, 2 et 3", "Spirou T1, 2 et 3", None),
        ("XIII T1 – 2", "XIII T1 – 2", None),
        ("Astérix T1 - 2 - 3", "Astérix T1 - 2 - 3", None),
        ("Tome 1 - 2", "Tome 1 - 2", None),
        ("Tome 3 : ...", "Tome 3 : ...", None),
        ("Tome 3 - « Le Retour »", "« Le Retour »", Some(3)),
        // Review round 3 (L1) — an abbreviation's or an ellipsis' dot.
        (
            "Le Spirou de... T4 : Le Journal d'un ingénu",
            "Le Spirou de... T4 : Le Journal d'un ingénu",
            None,
        ),
        (
            "Mademoiselle J. T.3 - Le Monde",
            "Mademoiselle J. T.3 - Le Monde",
            None,
        ),
        ("Dr. No 1", "Dr. No 1", None),
        (
            "Thorgal. Tome 3 : Le Pays Qâ",
            "Thorgal : Le Pays Qâ",
            Some(3),
        ),
        ("Thorgal-Tome 3 : x", "Thorgal-Tome 3 : x", None),
    ];

    /// Review M1 — (media type, input, expected title, expected number):
    /// the short markers are comic conventions; music, films, magazines
    /// and reports are never parsed.
    const MEDIA_CASES: &[(&str, &str, &str, Option<i32>)] = &[
        ("cd", "Symphonie n° 9", "Symphonie n° 9", None),
        ("book", "Symphonie n° 9", "Symphonie n° 9", None),
        ("cd", "Sonate Op. 27 No. 2", "Sonate Op. 27 No. 2", None),
        ("book", "Sonate Op. 27 No. 2", "Sonate Op. 27 No. 2", None),
        (
            "cd",
            "Concerto pour piano no 21, K. 467",
            "Concerto pour piano no 21, K. 467",
            None,
        ),
        (
            "book",
            "Concerto pour piano no 21, K. 467",
            "Concerto pour piano no 21, K. 467",
            None,
        ),
        ("book", "Chanel N°5", "Chanel N°5", None),
        ("dvd", "Chanel N°5", "Chanel N°5", None),
        ("dvd", "Kill Bill Vol. 1", "Kill Bill Vol. 1", None),
        ("book", "Aktion T4", "Aktion T4", None),
        ("book", "Vol 93 : le récit", "Vol 93 : le récit", None),
        ("bd", "Vol 93 : le récit", "Vol 93 : le récit", None),
        (
            "bd",
            "T 34 : un char de légende",
            "T 34 : un char de légende",
            None,
        ),
        ("book", "Agent No 7", "Agent No 7", None),
        ("book", "Paris, No 5", "Paris, No 5", None),
        (
            "book",
            "Titeuf T12 : Bienvenue",
            "Titeuf T12 : Bienvenue",
            None,
        ),
        ("book", "Batman #12", "Batman #12", None),
        ("magazine", "Spirou n° 4123", "Spirou n° 4123", None),
        (
            "magazine",
            "Tome 3 : Le Mystère",
            "Tome 3 : Le Mystère",
            None,
        ),
        ("report", "Volume 2 : Annexes", "Volume 2 : Annexes", None),
        ("cd", "Tome 3 : Le Mystère", "Tome 3 : Le Mystère", None),
        (
            "bd",
            "Titeuf T12 : Bienvenue",
            "Titeuf : Bienvenue",
            Some(12),
        ),
        (
            "bd",
            "Les Tuniques bleues n°12",
            "Les Tuniques bleues",
            Some(12),
        ),
        (
            "book",
            "Harry Potter, tome 1 : à l'école des sorciers",
            "Harry Potter : à l'école des sorciers",
            Some(1),
        ),
        (
            "book",
            "Le Seigneur des anneaux, Volume 2 : Les Deux Tours",
            "Le Seigneur des anneaux : Les Deux Tours",
            Some(2),
        ),
        (
            "book",
            "À la recherche du temps perdu Vol. 4 : Sodome et Gomorrhe",
            "À la recherche du temps perdu : Sodome et Gomorrhe",
            Some(4),
        ),
        ("book", "Die Chroniken, Band 2", "Die Chroniken", Some(2)),
        ("book", "Band 3 : Die Rückkehr", "Die Rückkehr", Some(3)),
        // Review round 2 (L1) — "Band" inside a plain phrase is a word.
        ("book", "Rock Band 2", "Rock Band 2", None),
        ("book", "Boy Band 3 : x", "Boy Band 3 : x", None),
        ("book", "Jazz Band 1920 : x", "Jazz Band 1920 : x", None),
        ("bd", "Rock Band 2", "Rock Band 2", None),
        // Review round 2 (M1) — two markers in a book.
        ("book", "Volume 1 - Volume 2", "Volume 1 - Volume 2", None),
        ("book", "Tome 1, Tome 2 : x", "Tome 1, Tome 2 : x", None),
        ("bd", "Le Scorpion T1 - T2", "Le Scorpion T1 - T2", None),
        // Review round 3 (M1).
        ("book", "Volume 1 - 2 : x", "Volume 1 - 2 : x", None),
        ("bd", "L'Intégrale T1 - 3", "L'Intégrale T1 - 3", None),
        ("bd", "Coffret Tome 1, 2, 3", "Coffret Tome 1, 2, 3", None),
        ("bd", "Spirou T1, 2 et 3", "Spirou T1, 2 et 3", None),
        ("bd", "XIII T1 – 2", "XIII T1 – 2", None),
        ("bd", "Astérix T1 - 2 - 3", "Astérix T1 - 2 - 3", None),
        ("bd", "Tome 1 - 2", "Tome 1 - 2", None),
        ("bd", "Aktion T4", "Aktion T4", None),
        // Review round 3 (L1).
        (
            "bd",
            "Le Spirou de... T4 : Le Journal d'un ingénu",
            "Le Spirou de... T4 : Le Journal d'un ingénu",
            None,
        ),
        ("bd", "Dr. No 1", "Dr. No 1", None),
        (
            "bd",
            "Mademoiselle J. T.3 - Le Monde",
            "Mademoiselle J. T.3 - Le Monde",
            None,
        ),
    ];

    #[test]
    fn split_for_media_type_table() {
        for (media, input, title, number) in MEDIA_CASES {
            let (got_title, got_number) = split_for_media_type(input, media);
            assert_eq!(
                (got_title.as_str(), got_number),
                (*title, *number),
                "{media}: {input:?}"
            );
        }
    }

    #[test]
    fn split_tome_marker_table() {
        assert!(CASES.len() >= 30);
        for (input, title, number) in CASES {
            let (got_title, got_number) = split_tome_marker(input, MarkerSet::Comic);
            assert_eq!(
                (got_title.as_str(), got_number),
                (*title, *number),
                "input: {input:?}"
            );
        }
    }

    #[test]
    fn untouched_titles_come_back_byte_for_byte() {
        let raw = "  Apollo 13 ";
        assert_eq!(
            split_tome_marker(raw, MarkerSet::Comic),
            (raw.to_string(), None)
        );
    }

    #[test]
    fn upper_bound_is_accepted_and_beyond_refused() {
        assert_eq!(
            split_tome_marker("Tome 10000 : Fin", MarkerSet::Book).1,
            Some(10_000)
        );
        assert_eq!(
            split_tome_marker("Tome 10001 : Fin", MarkerSet::Book).1,
            None
        );
    }

    fn snapshot(media_type: &str, edited: Option<&str>) -> TitleModel {
        TitleModel {
            id: 1,
            title: "x".to_string(),
            original_title: None,
            subtitle: None,
            statement_of_responsibility: None,
            description: None,
            general_note: None,
            language: "fr".to_string(),
            media_type: media_type.to_string(),
            publication_date: None,
            publisher: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            isbn: None,
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: None,
            page_count: None,
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            manually_edited_fields: edited.map(str::to_string),
            version: 1,
        }
    }

    fn metadata(title: &str, collection_number: Option<&str>) -> MetadataResult {
        MetadataResult {
            title: Some(title.to_string()),
            collection_number: collection_number.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn prepare_strips_the_marker_and_returns_its_number() {
        let mut md = metadata("Tome 3 : Le Mystère", None);
        assert_eq!(
            prepare_metadata_title(&mut md, &snapshot("bd", None)),
            Some(3)
        );
        assert_eq!(md.title.as_deref(), Some("Le Mystère"));
    }

    #[test]
    fn prepare_prefers_the_marker_over_the_provider_number() {
        let mut md = metadata("Tome 3 : Le Mystère", Some("7"));
        assert_eq!(
            prepare_metadata_title(&mut md, &snapshot("bd", None)),
            Some(3)
        );
    }

    #[test]
    fn prepare_falls_back_to_a_comic_collection_number() {
        let mut md = metadata("Le Mystère", Some("7"));
        assert_eq!(
            prepare_metadata_title(&mut md, &snapshot("bd", None)),
            Some(7)
        );
        assert_eq!(md.title.as_deref(), Some("Le Mystère"));
        // A book's collection number is a publisher's (Folio 1234).
        let mut md = metadata("Le Mystère", Some("1234"));
        assert_eq!(
            prepare_metadata_title(&mut md, &snapshot("book", None)),
            None
        );
        // Not a plain integer.
        let mut md = metadata("Le Mystère", Some("12 bis"));
        assert_eq!(prepare_metadata_title(&mut md, &snapshot("bd", None)), None);
    }

    #[test]
    fn prepare_routes_nothing_for_a_hand_edited_title() {
        let mut md = metadata("Tome 3 : Le Mystère", Some("3"));
        let snap = snapshot("bd", Some(r#"["title"]"#));
        assert_eq!(prepare_metadata_title(&mut md, &snap), None);
        // The proposed value is still clean (it is never written anyway).
        assert_eq!(md.title.as_deref(), Some("Le Mystère"));
    }

    #[test]
    fn labels_are_localized() {
        assert_eq!(tome_label(3, None, "fr"), "Tome 3");
        assert_eq!(tome_label(3, None, "en"), "Volume 3");
        assert_eq!(tome_label(3, None, "de"), "Band 3");
        assert_eq!(tome_label(3, None, "it"), "Tomo 3");
        assert_eq!(tome_label(3, Some(5), "fr"), "Tomes 3–5");
        assert_eq!(tome_label(3, Some(3), "fr"), "Tome 3");
        assert_eq!(detected_label(3, "fr"), "Tome 3 (détecté)");
        assert_eq!(detected_label(3, "en"), "Volume 3 (detected)");
    }

    fn m(series_id: u64, name: &str, position: i32) -> TomeMembership {
        TomeMembership {
            series_id,
            series_name: name.to_string(),
            position,
        }
    }

    #[test]
    fn positions_label_covers_single_run_and_scattered() {
        assert_eq!(positions_label(&[], "fr"), None);
        assert_eq!(
            positions_label(&[m(1, "A", 4)], "fr").as_deref(),
            Some("Tome 4")
        );
        assert_eq!(
            positions_label(&[m(1, "A", 5), m(1, "A", 3), m(1, "A", 4)], "fr").as_deref(),
            Some("Tomes 3–5")
        );
        assert_eq!(
            positions_label(&[m(1, "A", 12), m(1, "A", 3), m(1, "A", 3)], "fr").as_deref(),
            Some("Tomes 3, 12")
        );
    }

    #[test]
    fn positions_label_never_merges_two_series_into_a_range() {
        // Review L5: tome 3 of A and tome 4 of B are not "Tomes 3–4".
        assert_eq!(
            positions_label(&[m(1, "A", 3), m(2, "B", 4)], "fr").as_deref(),
            Some("Tome 3 \u{b7} A, Tome 4 \u{b7} B")
        );
        assert_eq!(
            positions_label(&[m(1, "A", 3), m(1, "A", 4), m(2, "B", 9)], "en").as_deref(),
            Some("Volumes 3–4 \u{b7} A, Volume 9 \u{b7} B")
        );
    }

    #[test]
    fn parse_memberships_reads_the_list_column() {
        assert!(parse_memberships(None).is_empty());
        assert!(parse_memberships(Some("")).is_empty());
        let raw = format!("7{FIELD_SEP}3{FIELD_SEP}A: B{MEMBERSHIP_SEP}9{FIELD_SEP}4{FIELD_SEP}C");
        assert_eq!(
            parse_memberships(Some(&raw)),
            vec![m(7, "A: B", 3), m(9, "C", 4)]
        );
        assert!(parse_memberships(Some("x\u{1f}3\u{1f}A")).is_empty());
    }

    #[test]
    fn confirmed_tome_requires_the_proposed_title_to_be_saved() {
        assert_eq!(confirmed_tome("Le Mystère", "Le Mystère", Some(3)), Some(3));
        // The librarian kept a hand-edited title: nothing recorded.
        assert_eq!(confirmed_tome("Mon titre", "Le Mystère", Some(3)), None);
        assert_eq!(confirmed_tome("Le Mystère", "Le Mystère", None), None);
        assert_eq!(confirmed_tome("", "", Some(3)), None);
        assert_eq!(confirmed_tome("x", "x", Some(0)), None);
    }

    #[test]
    fn manual_hint_only_for_a_marker() {
        assert!(manual_title_hint("Apollo 13", "bd", "fr").is_none());
        let hint = manual_title_hint("Thorgal - Tome 29 - Le sacrifice", "bd", "fr").unwrap();
        assert!(hint.contains("Tome 29"), "{hint}");
        assert!(manual_title_hint_html("Apollo 13", "bd", "fr").is_empty());
        assert!(manual_title_hint_html("Batman #12", "bd", "en").contains("feedback-entry"));
        // A short marker in a book or a CD is not a tome.
        assert!(manual_title_hint("Symphonie n° 9", "cd", "fr").is_none());
        assert!(manual_title_hint("Batman #12", "book", "fr").is_none());
    }

    #[test]
    fn view_shows_the_hint_only_outside_series() {
        let v = TitleTomeView::new(Some(3), false, "fr");
        assert_eq!(v.detected_label.as_deref(), Some("Tome 3 (détecté)"));
        assert_eq!(v.prefill, Some(3));
        let v = TitleTomeView::new(Some(3), true, "fr");
        assert!(v.detected_label.is_none());
        assert!(v.prefill.is_none());
        assert!(
            TitleTomeView::new(None, false, "fr")
                .detected_label
                .is_none()
        );
    }
}
