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
use crate::models::series::{SeriesModel, SeriesType};
use crate::models::title::TitleModel;
use crate::models::title_tome::{TitleTomeModel, TwinRow};

/// Largest tome number the parser accepts.
pub const MAX_TOME: i32 = 10_000;

/// Characters that separate a marker from the rest of a title.
const SEPARATORS: [char; 7] = ['-', '\u{2013}', '\u{2014}', ':', ',', '.', ';'];

/// Marker words, longest first where one is a prefix of another. The
/// flag says whether a `.` may follow the word ("Vol. 3", "T.12").
const MARKER_WORDS: [(&str, bool); 9] = [
    ("tome", false),
    ("tomo", false),
    ("volume", false),
    ("band", false),
    ("vol", true),
    ("bd", true),
    ("no", true),
    ("t", true),
    ("#", false),
];

/// Words that cannot stand for the series name before a marker ("Le
/// Tome 2 : la suite" is a sentence, not "Le" volume 2).
const DETERMINERS: [&str; 24] = [
    "le", "la", "les", "l'", "un", "une", "des", "du", "de", "the", "a", "an", "der", "die", "das",
    "ein", "eine", "il", "lo", "gli", "i", "el", "mon", "my",
];

/// Split a tome marker off a title (pure — unit tested over a table of
/// real comic titles). Recognised, case-insensitive: `Tome 12`, `Tomo
/// 12`, `T12`, `T 12`, `T. 12`, `T.12`, `Vol. 12`, `Vol 12`, `Volume
/// 12`, `Band 12`, `Bd. 12`, `n° 12`, `No 12`, `No. 12`, `#12`, also
/// between parentheses or brackets; number 1..=[`MAX_TOME`].
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
pub fn split_tome_marker(raw: &str) -> (String, Option<i32>) {
    let s = raw.trim();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        if k > 0 && !chars[k - 1].1.is_whitespace() {
            continue;
        }
        let span = if c == '(' || c == '[' {
            let close = if c == '(' { ')' } else { ']' };
            let inner = i + c.len_utf8();
            let Some((len, n)) = match_marker(&s[inner..]) else {
                continue;
            };
            let after = s[inner + len..].trim_start();
            if !after.starts_with(close) {
                continue;
            }
            (s.len() - after.len() + close.len_utf8(), n)
        } else {
            let Some((len, n)) = match_marker(&s[i..]) else {
                continue;
            };
            (i + len, n)
        };
        if let Some(cleaned) = rebuild(&s[..i], &s[span.0..]) {
            return (cleaned, Some(span.1));
        }
    }
    (raw.to_string(), None)
}

/// A marker at the start of `t`: bytes consumed and the number.
fn match_marker(t: &str) -> Option<(usize, i32)> {
    let mut rest = None;
    for (word, dot) in MARKER_WORDS {
        if let Some(after) = strip_prefix_ignore_ascii_case(t, word) {
            let after = if dot {
                after.strip_prefix('.').unwrap_or(after)
            } else {
                after
            };
            rest = Some(after);
            break;
        }
    }
    // "n°" / "nº" (any case of the n).
    if rest.is_none()
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
    Some((t.len() - tail.len(), n))
}

/// `t` without the ASCII `prefix`, compared case-insensitively.
fn strip_prefix_ignore_ascii_case<'a>(t: &'a str, prefix: &str) -> Option<&'a str> {
    let head = t.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &t[prefix.len()..])
}

/// Join what surrounds a marker; `None` when the context is not one of a
/// marker (see [`split_tome_marker`]).
fn rebuild(left_raw: &str, right_raw: &str) -> Option<String> {
    let right = right_raw.trim_start();
    let (right_sep, after) = match right.chars().next() {
        None => (None, ""),
        Some(c) if SEPARATORS.contains(&c) => {
            let rest = &right[c.len_utf8()..];
            if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
                return None;
            }
            (Some(c), rest.trim_start())
        }
        Some(_) => return None,
    };
    let left = left_raw.trim_end();
    let left_core = match left.chars().last() {
        Some(c) if SEPARATORS.contains(&c) => left[..left.len() - c.len_utf8()].trim_end(),
        _ => left,
    };
    let cleaned = match (left_core.is_empty(), after.is_empty()) {
        (true, true) => return None,
        (true, false) => after.to_string(),
        (false, true) => left_core.to_string(),
        (false, false) => {
            let sep = right_sep.unwrap_or('-');
            let sep = if sep == ',' || sep == '.' {
                format!("{sep} ")
            } else {
                format!(" {sep} ")
            };
            format!("{left_core}{sep}{after}")
        }
    };
    if !left_core.is_empty() {
        let last_word = left_core
            .rsplit(char::is_whitespace)
            .next()
            .unwrap_or("")
            .to_lowercase();
        if DETERMINERS.contains(&last_word.as_str()) {
            return None;
        }
    }
    cleaned
        .chars()
        .any(char::is_alphanumeric)
        .then_some(cleaned)
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

/// Ingestion (pure — unit tested): strip the marker from the provider
/// title in `metadata` (so the stored title and any conflict form show
/// the clean value), and return the tome number to record — `None` when
/// the title is hand-edited (`title` in `manually_edited_fields`: it is
/// never rewritten, so its number is not routed either).
///
/// Choice (#579): a marker in the title wins over a provider number;
/// without a marker, a comic's plain-integer `collection_number` is used
/// ([`provider_tome_number`]).
pub fn prepare_metadata_title(metadata: &mut MetadataResult, snapshot: &TitleModel) -> Option<i32> {
    let parsed = match metadata.title.as_deref() {
        Some(t) => {
            let (cleaned, n) = split_tome_marker(t);
            if n.is_some() {
                metadata.title = Some(cleaned);
            }
            n
        }
        None => None,
    };
    let guarded: HashSet<String> = snapshot
        .parsed_manually_edited_fields()
        .into_iter()
        .collect();
    if guarded.contains("title") {
        return None;
    }
    parsed.or_else(|| provider_tome_number(metadata, &snapshot.media_type))
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
    match TitleTomeModel::set_detected_tome_if_unattached(pool, title_id, number).await {
        Ok(_) => tracing::debug!(title_id, detected = number, "Detected tome number recorded"),
        Err(e) => tracing::warn!(error = %e, title_id, "Detected tome number write failed"),
    }
}

/// Clear the hint after a hand attach (best effort, logged).
pub async fn clear_detected_tome(pool: &DbPool, title_id: u64) {
    if let Err(e) = TitleTomeModel::clear_detected_tome(pool, title_id).await {
        tracing::warn!(error = %e, title_id, "Detected tome number clear failed");
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

/// The label of every position a title holds (pure — unit tested), for
/// list rows: "Tome 3", "Tomes 3–5" (a run), "Tomes 3, 12" (otherwise);
/// `None` when it holds none.
pub fn positions_label(positions: &[i32], loc: &str) -> Option<String> {
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

/// "Tome N (detected)" for the hint (pure).
pub fn detected_label(n: i32, loc: &str) -> String {
    rust_i18n::t!("tome.detected", locale = loc, n = n.to_string()).to_string()
}

/// Parse the comma-separated positions a list query selects (pure).
pub fn parse_positions(csv: Option<&str>) -> Vec<i32> {
    csv.unwrap_or("")
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect()
}

/// The positions label of a title (volume page).
pub async fn title_positions_label(
    pool: &DbPool,
    title_id: u64,
    loc: &str,
) -> Result<Option<String>, AppError> {
    let held = TitleTomeModel::held_positions(pool, title_id).await?;
    Ok(positions_label(&held, loc))
}

/// AC6 — a hand-typed title is never rewritten; when it holds a marker,
/// the librarian is told how to move the number (pure — unit tested).
pub fn manual_title_hint(title: &str, loc: &str) -> Option<String> {
    let (_, n) = split_tome_marker(title);
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
pub fn manual_title_hint_html(title: &str, loc: &str) -> String {
    manual_title_hint(title, loc)
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
    let mut tx = pool.begin().await?;
    let current = TitleTomeModel::lock_membership_in(&mut tx, title_id, assignment_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
        })?;
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
    let series = SeriesModel::active_find_by_id(pool, current.series_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
        })?;
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
        (
            "L'Homme de l'année T. 3 - 1917",
            "L'Homme de l'année - 1917",
            Some(3),
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
        ("Die Schlümpfe Bd. 5", "Die Schlümpfe", Some(5)),
        ("Tex Willer, tomo 3", "Tex Willer", Some(3)),
        ("Batman #12", "Batman", Some(12)),
        ("Spirou et Fantasio - T05", "Spirou et Fantasio", Some(5)),
        ("Gaston T19.", "Gaston", Some(19)),
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
    ];

    #[test]
    fn split_tome_marker_table() {
        assert!(CASES.len() >= 30);
        for (input, title, number) in CASES {
            let (got_title, got_number) = split_tome_marker(input);
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
        assert_eq!(split_tome_marker(raw), (raw.to_string(), None));
    }

    #[test]
    fn upper_bound_is_accepted_and_beyond_refused() {
        assert_eq!(split_tome_marker("Tome 10000 : Fin").1, Some(10_000));
        assert_eq!(split_tome_marker("Tome 10001 : Fin").1, None);
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

    #[test]
    fn positions_label_covers_single_run_and_scattered() {
        assert_eq!(positions_label(&[], "fr"), None);
        assert_eq!(positions_label(&[4], "fr").as_deref(), Some("Tome 4"));
        assert_eq!(
            positions_label(&[5, 3, 4], "fr").as_deref(),
            Some("Tomes 3–5")
        );
        assert_eq!(
            positions_label(&[12, 3, 3], "fr").as_deref(),
            Some("Tomes 3, 12")
        );
    }

    #[test]
    fn parse_positions_reads_the_list_column() {
        assert_eq!(parse_positions(None), Vec::<i32>::new());
        assert_eq!(parse_positions(Some("3,12")), vec![3, 12]);
        assert_eq!(parse_positions(Some("")), Vec::<i32>::new());
    }

    #[test]
    fn manual_hint_only_for_a_marker() {
        assert!(manual_title_hint("Apollo 13", "fr").is_none());
        let hint = manual_title_hint("Thorgal - Tome 29 - Le sacrifice", "fr").unwrap();
        assert!(hint.contains("Tome 29"), "{hint}");
        assert!(manual_title_hint_html("Apollo 13", "fr").is_empty());
        assert!(manual_title_hint_html("Batman #12", "en").contains("feedback-entry"));
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
