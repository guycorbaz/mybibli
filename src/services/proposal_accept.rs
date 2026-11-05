//! Stories 11-17 / 11-18 (FR138, FR140) — the librarian-facing side of
//! accepting a proposal: the modal's summary and the localized copy of
//! every outcome (`docs/error-message-style.md`: what happened → why →
//! what to do; no internals). The acceptance itself is
//! [`crate::services::acceptance`].

use crate::error::AppError;
use crate::models::proposal::{ProposalKind, ProposalStatus};
use crate::services::acceptance::{AcceptConflict, AcceptError, Applied, StaleCause};
use crate::services::cover_acceptance::AppliedCover;
use crate::services::label_acceptance::AppliedLabel;
use crate::services::metadata_acceptance::AppliedMetadata;
use crate::services::proposal_review::{ReviewDetail, ReviewError, TargetLabel, field_label};
use crate::services::series_acceptance::AppliedSeries;
use crate::services::series_update_acceptance::AppliedSeriesUpdate;
use crate::utils::{feedback_html, feedback_html_with_link};

/// Whether the page offers "Accept" for a proposal: every pending one
/// (series kinds since 11-17, metadata and cover since 11-18).
pub fn can_accept(_kind: ProposalKind, status: ProposalStatus) -> bool {
    status == ProposalStatus::Pending
}

fn is_series(kind: ProposalKind) -> bool {
    matches!(
        kind,
        ProposalKind::SeriesCreate | ProposalKind::SeriesAttach | ProposalKind::SeriesUpdate
    )
}

/// The modal body of any kind (plain sentences; the caller escapes them).
pub fn modal_sentences(detail: &ReviewDetail, loc: &str) -> Vec<String> {
    use rust_i18n::t;
    let kind = detail.proposal.kind;
    if kind == ProposalKind::SeriesUpdate {
        return series_update_sentences(detail, loc);
    }
    if is_series(kind) {
        return AcceptSummary::of(detail, loc).sentences(loc);
    }
    let title = detail.target.text(loc);
    let mut out = Vec::new();
    if kind == ProposalKind::LabelAttach {
        // Story 13-9.
        for line in detail.lines.iter().filter(|l| l.line == "label") {
            out.push(
                t!(
                    "proposals.accept_modal_label",
                    locale = loc,
                    label = line.label_name.as_str(),
                    target = title.as_str()
                )
                .to_string(),
            );
        }
        out.push(t!("proposals.accept_modal_note_label", locale = loc).to_string());
        return out;
    }
    if kind == ProposalKind::Cover {
        out.push(
            t!(
                "proposals.accept_modal_cover",
                locale = loc,
                title = title.as_str()
            )
            .to_string(),
        );
        if detail
            .lines
            .iter()
            .any(|l| l.line == "cover" && l.replaces_existing)
        {
            out.push(t!("proposals.accept_modal_cover_replaces", locale = loc).to_string());
        }
    } else {
        let fields: Vec<String> = detail
            .lines
            .iter()
            .filter(|l| l.line == "field")
            .map(|l| l.field_label.clone())
            .collect();
        if !fields.is_empty() {
            out.push(
                t!(
                    "proposals.accept_modal_fields",
                    locale = loc,
                    title = title.as_str(),
                    fields = fields.join(", ").as_str()
                )
                .to_string(),
            );
        }
        let contributors: usize = detail
            .lines
            .iter()
            .filter(|l| l.line == "contributors")
            .map(|l| l.contributors_proposed.len())
            .sum();
        if contributors > 0 {
            out.push(
                t!(
                    "proposals.accept_modal_contributors",
                    locale = loc,
                    title = title.as_str(),
                    count = contributors
                )
                .to_string(),
            );
        }
    }
    out.push(t!("proposals.accept_modal_note_title", locale = loc).to_string());
    out
}

/// Story 15-4 — the modal body of a `series_update`: each changed field
/// (current → proposed), the removed tomes, then the note.
fn series_update_sentences(detail: &ReviewDetail, loc: &str) -> Vec<String> {
    use rust_i18n::t;
    let mut out = vec![
        t!(
            "proposals.accept_modal_series_update",
            locale = loc,
            name = detail.target.text(loc).as_str()
        )
        .to_string(),
    ];
    for c in detail
        .lines
        .iter()
        .filter(|l| l.line == "series_update")
        .flat_map(|l| &l.series_changes)
    {
        out.push(
            t!(
                "proposals.accept_modal_series_change",
                locale = loc,
                field = c.label.as_str(),
                current = c.current.as_str(),
                proposed = c.proposed.as_str()
            )
            .to_string(),
        );
    }
    let removed = detail
        .lines
        .iter()
        .filter(|l| l.line == "tome_removal")
        .count();
    if removed > 0 {
        out.push(
            t!(
                "proposals.accept_modal_tomes_removed",
                locale = loc,
                count = removed
            )
            .to_string(),
        );
    }
    out.push(t!("proposals.accept_modal_note_series_update", locale = loc).to_string());
    out
}

/// Story 15-4 — the success message of a `series_update` acceptance.
pub fn series_update_message(id: u64, applied: &AppliedSeriesUpdate, loc: &str) -> String {
    use rust_i18n::t;
    let mut parts = vec![
        t!("proposals.accepted_feedback", locale = loc, id = id).to_string(),
        t!(
            if applied.renamed {
                "proposals.applied_series_renamed"
            } else {
                "proposals.applied_series_update"
            },
            locale = loc,
            name = applied.series_name.as_str()
        )
        .to_string(),
    ];
    match applied.type_total {
        Some(Some(total)) => parts.push(
            t!("proposals.applied_series_closed", locale = loc, total = total).to_string(),
        ),
        Some(None) => parts.push(t!("proposals.applied_series_open", locale = loc).to_string()),
        None => {}
    }
    if applied.removed > 0 {
        parts.push(
            t!(
                "proposals.applied_tomes_removed",
                locale = loc,
                count = applied.removed
            )
            .to_string(),
        );
    }
    parts.join(" ")
}

/// What the accept modal announces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptSummary {
    pub creates: bool,
    pub series_name: String,
    pub titles: usize,
    pub tomes: usize,
}

impl AcceptSummary {
    /// Counted from the detail's display lines.
    pub fn of(detail: &ReviewDetail, loc: &str) -> Self {
        let count = |kind: &str| detail.lines.iter().filter(|l| l.line == kind).count();
        AcceptSummary {
            creates: matches!(detail.target, TargetLabel::NewSeries(_)),
            series_name: match &detail.target {
                TargetLabel::NewSeries(name) => name.clone(),
                other => other.text(loc),
            },
            titles: count("title"),
            tomes: count("tome"),
        }
    }

    /// The modal body text (plain; the caller escapes it).
    pub fn sentences(&self, loc: &str) -> Vec<String> {
        let head = if self.creates {
            rust_i18n::t!(
                "proposals.accept_modal_create",
                locale = loc,
                name = self.series_name.as_str()
            )
        } else {
            rust_i18n::t!(
                "proposals.accept_modal_attach",
                locale = loc,
                name = self.series_name.as_str()
            )
        };
        vec![
            head.to_string(),
            rust_i18n::t!(
                "proposals.accept_modal_counts",
                locale = loc,
                titles = self.titles,
                tomes = self.tomes
            )
            .to_string(),
            rust_i18n::t!("proposals.accept_modal_note", locale = loc).to_string(),
        ]
    }
}

/// The success entry: one sentence per fact, the first naming the
/// proposal and the series.
pub fn applied_messages(id: u64, applied: &AppliedSeries, loc: &str) -> (String, String) {
    let key = if applied.created {
        "proposals.applied_created"
    } else {
        "proposals.applied_attached"
    };
    let message = format!(
        "{} {}",
        rust_i18n::t!("proposals.accepted_feedback", locale = loc, id = id),
        rust_i18n::t!(
            key,
            locale = loc,
            name = applied.series_name.as_str(),
            titles = applied.titles_attached,
            tomes = applied.tomes_created,
            settled = applied.tomes_settled
        )
    );
    // Story 13-11 — expected tomes settled by a placed title.
    let filled = (applied.tomes_filled > 0).then(|| {
        rust_i18n::t!(
            "proposals.applied_filled",
            locale = loc,
            count = applied.tomes_filled
        )
        .to_string()
    });
    let suggestion = filled
        .into_iter()
        .chain(applied.tomes_skipped.iter().map(|s| {
            rust_i18n::t!(
                "proposals.applied_skipped",
                locale = loc,
                title = s.title.as_str(),
                position = s.position,
                owner = s.owner_title.as_str(),
                owner_position = s.owner_position
            )
            .to_string()
        }))
        .collect::<Vec<_>>()
        .join(" ");
    (message, suggestion)
}

fn names(list: &[(String, String)]) -> String {
    list.iter()
        .map(|(name, role)| format!("{name} ({role})"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The success entry of a metadata acceptance: `(message, suggestion)`.
pub fn metadata_messages(id: u64, applied: &AppliedMetadata, loc: &str) -> (String, String) {
    use rust_i18n::t;
    let mut parts = vec![t!("proposals.accepted_feedback", locale = loc, id = id).to_string()];
    let title = applied.title.as_str();
    if !applied.fields.is_empty() {
        let fields = applied
            .fields
            .iter()
            .map(|f| field_label(f, loc))
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(
            t!(
                "proposals.applied_fields",
                locale = loc,
                title = title,
                fields = fields.as_str()
            )
            .to_string(),
        );
    }
    if !applied.contributors_added.is_empty() {
        parts.push(
            t!(
                "proposals.applied_contributors_added",
                locale = loc,
                title = title,
                names = names(&applied.contributors_added).as_str()
            )
            .to_string(),
        );
    }
    if applied.fields.is_empty() && applied.contributors_added.is_empty() {
        parts.push(t!("proposals.applied_nothing_new", locale = loc, title = title).to_string());
    }
    let suggestion = if applied.contributors_present.is_empty() {
        String::new()
    } else {
        t!(
            "proposals.applied_contributors_present",
            locale = loc,
            names = names(&applied.contributors_present).as_str()
        )
        .to_string()
    };
    (parts.join(" "), suggestion)
}

/// The entry of a cover acceptance: `(installed, message, suggestion)` —
/// a success when the image is installed, else a warning (the proposal
/// is accepted but the cover did not change).
pub fn cover_messages(id: u64, applied: &AppliedCover, loc: &str) -> (bool, String, String) {
    use rust_i18n::t;
    let head = t!("proposals.accepted_feedback", locale = loc, id = id);
    let title = applied.title.as_str();
    if applied.installed {
        let key = if applied.replaced {
            "proposals.applied_cover_replaced"
        } else {
            "proposals.applied_cover"
        };
        (
            true,
            format!("{head} {}", t!(key, locale = loc, title = title)),
            String::new(),
        )
    } else {
        (
            false,
            format!(
                "{head} {}",
                t!(
                    "proposals.applied_cover_failed",
                    locale = loc,
                    title = title
                )
            ),
            t!("proposals.applied_cover_failed_hint", locale = loc).to_string(),
        )
    }
}

/// The feedback entry (HTML) of an accepted proposal, with a link to
/// what changed.
pub fn applied_entry(id: u64, applied: &Applied, loc: &str) -> String {
    use rust_i18n::t;
    match applied {
        Applied::Series(a) => {
            let (message, suggestion) = applied_messages(id, a, loc);
            feedback_html_with_link(
                "success",
                &message,
                &suggestion,
                &format!("/series/{}", a.series_id),
                &t!("proposals.open_series", locale = loc),
            )
        }
        Applied::Metadata(a) => {
            let (message, suggestion) = metadata_messages(id, a, loc);
            feedback_html_with_link(
                "success",
                &message,
                &suggestion,
                &format!("/title/{}", a.title_id),
                &t!("proposals.open_title", locale = loc),
            )
        }
        Applied::Cover(a) => {
            let (installed, message, suggestion) = cover_messages(id, a, loc);
            feedback_html_with_link(
                if installed { "success" } else { "warning" },
                &message,
                &suggestion,
                &format!("/title/{}", a.title_id),
                &t!("proposals.open_title", locale = loc),
            )
        }
        Applied::Label(a) => feedback_html_with_link(
            "success",
            &label_message(id, a, loc),
            "",
            &a.href(),
            &t!(
                match a.target {
                    crate::models::label::LabelTarget::Volume(_) => "proposals.open_volume",
                    crate::models::label::LabelTarget::Title(_) => "proposals.open_title",
                },
                locale = loc
            ),
        ),
        Applied::SeriesUpdate(a) => feedback_html_with_link(
            "success",
            &series_update_message(id, a, loc),
            "",
            &a.href(),
            &t!("proposals.open_series", locale = loc),
        ),
    }
}

/// Story 13-9 — the success message of a label acceptance.
pub fn label_message(id: u64, applied: &AppliedLabel, loc: &str) -> String {
    use rust_i18n::t;
    format!(
        "{} {}",
        t!("proposals.accepted_feedback", locale = loc, id = id),
        t!(
            "proposals.applied_label",
            locale = loc,
            label = applied.label_name.as_str(),
            target = applied.target_name.as_str()
        )
    )
}

/// The warning entry (HTML) of an acceptance that went stale.
pub fn stale_entry(id: u64, kind: ProposalKind, cause: &StaleCause, loc: &str) -> String {
    let (message, hint) = stale_messages_of(id, kind, cause, loc);
    feedback_html("warning", &message, &hint)
}

/// Why the proposal went stale, as a clause.
pub fn stale_why(cause: &StaleCause, loc: &str) -> String {
    stale_why_of(ProposalKind::SeriesAttach, cause, loc)
}

/// [`stale_why`] for a proposal of `kind` (a title kind names the title
/// rather than "the series or a title").
pub fn stale_why_of(kind: ProposalKind, cause: &StaleCause, loc: &str) -> String {
    use rust_i18n::t;
    match cause {
        StaleCause::Targets(_) if kind == ProposalKind::SeriesUpdate => {
            t!("proposals.stale_why_series_changed", locale = loc)
        }
        StaleCause::Targets(_) if kind == ProposalKind::LabelAttach => {
            t!("proposals.stale_why_label_target_gone", locale = loc)
        }
        StaleCause::Targets(_) if !is_series(kind) => {
            t!("proposals.stale_why_title_changed", locale = loc)
        }
        StaleCause::Targets(_) => t!("proposals.stale_why_targets", locale = loc),
        StaleCause::Conflict(c) => match c {
            AcceptConflict::SeriesExists { name } => t!(
                "proposals.stale_why_series_exists",
                locale = loc,
                name = name.as_str()
            ),
            AcceptConflict::TitleInSeries { title, position } => t!(
                "proposals.stale_why_title_in_series",
                locale = loc,
                title = title.as_str(),
                position = position
            ),
            AcceptConflict::PositionHeld { position, holder } => t!(
                "proposals.stale_why_position_held",
                locale = loc,
                position = position,
                holder = holder.as_str()
            ),
            AcceptConflict::TomePositionTaken {
                position,
                tome_title,
            } => t!(
                "proposals.stale_why_tome_position",
                locale = loc,
                position = position,
                tome = tome_title.as_str()
            ),
            AcceptConflict::TomeIsbnTaken { isbn, tome_title } => t!(
                "proposals.stale_why_tome_isbn",
                locale = loc,
                isbn = isbn.as_str(),
                tome = tome_title.as_str()
            ),
            AcceptConflict::TomeIsbnMismatch {
                position,
                tome_title,
                tome_isbn,
            } => t!(
                "proposals.stale_why_tome_isbn_mismatch",
                locale = loc,
                position = position,
                tome = tome_title.as_str(),
                isbn = tome_isbn.as_str()
            ),
            AcceptConflict::BeyondTotal { position, total } => t!(
                "proposals.stale_why_beyond_total",
                locale = loc,
                position = position,
                total = total
            ),
            AcceptConflict::FieldChanged { field } => t!(
                "proposals.stale_why_field_changed",
                locale = loc,
                field = field_label(field, loc).as_str()
            ),
            AcceptConflict::RoleGone { role } => t!(
                "proposals.stale_why_role_gone",
                locale = loc,
                role = role.as_str()
            ),
            AcceptConflict::CoverChanged => t!("proposals.stale_why_cover_changed", locale = loc),
            AcceptConflict::LabelGone { label } => t!(
                "proposals.stale_why_label_gone",
                locale = loc,
                label = label.as_str()
            ),
            AcceptConflict::LabelAttached { label } => t!(
                "proposals.stale_why_label_attached",
                locale = loc,
                label = label.as_str()
            ),
            AcceptConflict::TomeChanged {
                position,
                tome_title,
            } => t!(
                "proposals.stale_why_tome_changed",
                locale = loc,
                position = position,
                tome = tome_title.as_str()
            ),
            AcceptConflict::BelowPosition {
                total,
                owned,
                expected,
            } => t!(
                "proposals.stale_why_below_positions",
                locale = loc,
                total = total,
                positions = crate::services::series_update_edit::positions_text(
                    owned, expected, loc
                )
                .as_str()
            ),
        },
    }
    .to_string()
}

/// The warning entry of a stale outcome: what happened and why, then
/// what to do.
pub fn stale_messages(id: u64, cause: &StaleCause, loc: &str) -> (String, String) {
    stale_messages_of(id, ProposalKind::SeriesAttach, cause, loc)
}

/// [`stale_messages`] for a proposal of `kind`.
pub fn stale_messages_of(
    id: u64,
    kind: ProposalKind,
    cause: &StaleCause,
    loc: &str,
) -> (String, String) {
    // A series update changes rather than adds (story 15-4).
    let key = if is_series(kind) && kind != ProposalKind::SeriesUpdate {
        "proposals.stale_feedback"
    } else {
        "proposals.stale_feedback_title"
    };
    (
        rust_i18n::t!(
            key,
            locale = loc,
            id = id,
            why = stale_why_of(kind, cause, loc).as_str()
        )
        .to_string(),
        rust_i18n::t!("proposals.stale_feedback_hint", locale = loc).to_string(),
    )
}

/// The refusal a librarian reads.
pub fn error_localized(e: &AcceptError, loc: &str) -> String {
    let review = |r: ReviewError| r.localized(loc);
    match e {
        AcceptError::NotFound => review(ReviewError::NotFound),
        AcceptError::NotPending {
            status: ProposalStatus::Stale,
        } => review(ReviewError::Stale),
        AcceptError::NotPending { status } => review(ReviewError::NotPending { status: *status }),
        AcceptError::Changed => review(ReviewError::Changed),
        AcceptError::UnknownDecider => review(ReviewError::UnknownDecider),
        AcceptError::WrongKind { .. } => {
            rust_i18n::t!("proposals.error_accept_kind", locale = loc).to_string()
        }
        AcceptError::NothingToApply => {
            rust_i18n::t!("proposals.error_nothing_to_apply", locale = loc).to_string()
        }
        AcceptError::Invalid(_) => {
            rust_i18n::t!("proposals.error_accept_invalid", locale = loc).to_string()
        }
        AcceptError::CoverMissing => {
            rust_i18n::t!("proposals.error_cover_missing", locale = loc).to_string()
        }
        AcceptError::Apply(_) => {
            rust_i18n::t!("proposals.error_accept_failed", locale = loc).to_string()
        }
    }
}

/// The refusal as the web layer answers it. A failed apply is answered
/// with the localized "nothing changed, still pending" message (409), not
/// a bare server error: the librarian must know the catalog is intact.
pub fn into_app_error(e: AcceptError, loc: &str) -> AppError {
    let message = error_localized(&e, loc);
    match e {
        AcceptError::NotFound => AppError::NotFound(message),
        AcceptError::Invalid(_) | AcceptError::WrongKind { .. } => AppError::BadRequest(message),
        AcceptError::NotPending { .. }
        | AcceptError::Changed
        | AcceptError::NothingToApply
        | AcceptError::UnknownDecider
        | AcceptError::CoverMissing
        | AcceptError::Apply(_) => AppError::Conflict(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::proposals::{StaleReason, StaleTarget};
    use crate::services::series_acceptance::SkippedTome;

    const LOCALES: [&str; 4] = ["en", "fr", "de", "it"];

    #[test]
    fn series_update_messages_name_what_changed() {
        use crate::services::series_update_acceptance::AppliedSeriesUpdate;
        let applied = AppliedSeriesUpdate {
            series_id: 4,
            series_name: "Cubitus (Dupa)".into(),
            renamed: true,
            type_total: Some(Some(39)),
            removed: 2,
        };
        assert_eq!(
            series_update_message(7, &applied, "en"),
            "Proposal #7 accepted. Series renamed to “Cubitus (Dupa)”. It is now closed at 39 \
             tomes. Expected tomes removed: 2."
        );
        let opened = AppliedSeriesUpdate {
            renamed: false,
            type_total: Some(None),
            removed: 0,
            ..applied
        };
        assert_eq!(
            series_update_message(7, &opened, "en"),
            "Proposal #7 accepted. Series “Cubitus (Dupa)” updated. It is now open."
        );
        for loc in LOCALES {
            no_raw_key(&series_update_message(7, &opened, loc));
        }
    }

    #[test]
    fn series_update_stale_reasons_name_the_positions() {
        let below = StaleCause::Conflict(AcceptConflict::BelowPosition {
            total: 18,
            owned: vec![19],
            expected: vec![20, 21],
        });
        assert_eq!(
            stale_why_of(ProposalKind::SeriesUpdate, &below, "en"),
            "the series now has owned positions 19; expected tomes at positions 20, 21, above \
             the proposed total of 18."
        );
        let tome = StaleCause::Conflict(AcceptConflict::TomeChanged {
            position: 15,
            tome_title: "WS 15".into(),
        });
        assert!(stale_why_of(ProposalKind::SeriesUpdate, &tome, "en").contains("“WS 15”"));
        let targets = StaleCause::Targets(Vec::new());
        assert_eq!(
            stale_why_of(ProposalKind::SeriesUpdate, &targets, "en"),
            "the series was changed or deleted after the proposal was filed."
        );
        // "nothing was changed", not "nothing was added".
        let (message, _) = stale_messages_of(3, ProposalKind::SeriesUpdate, &targets, "en");
        assert!(message.contains("nothing was changed"), "{message}");
        for loc in LOCALES {
            for cause in [&below, &tome, &targets] {
                no_raw_key(&stale_why_of(ProposalKind::SeriesUpdate, cause, loc));
            }
        }
    }

    fn no_raw_key(s: &str) {
        let raw = s
            .match_indices("proposals.")
            .any(|(i, m)| s[i + m.len()..].starts_with(|c: char| c.is_ascii_lowercase()));
        assert!(!raw, "raw key leaked: {s}");
        assert!(!s.contains("%{"), "unfilled placeholder: {s}");
    }

    #[test]
    fn every_pending_proposal_can_be_accepted() {
        for kind in ProposalKind::ALL {
            assert!(can_accept(kind, ProposalStatus::Pending), "{kind:?}");
            for s in [
                ProposalStatus::Accepted,
                ProposalStatus::Rejected,
                ProposalStatus::Stale,
            ] {
                assert!(!can_accept(kind, s));
            }
        }
    }

    #[test]
    fn metadata_and_cover_outcomes_are_localized() {
        let applied = AppliedMetadata {
            title_id: 4,
            title: "Tintin".into(),
            fields: vec!["subtitle", "publication_year"],
            contributors_added: vec![("Hergé".into(), "Auteur".into())],
            contributors_present: vec![("Jacobs".into(), "Dessin".into())],
        };
        let nothing = AppliedMetadata {
            fields: vec![],
            contributors_added: vec![],
            ..applied.clone()
        };
        let conflicts = [
            AcceptConflict::FieldChanged {
                field: "publisher".into(),
            },
            AcceptConflict::RoleGone {
                role: "Dessin".into(),
            },
            AcceptConflict::CoverChanged,
        ];
        for loc in LOCALES {
            let (m, s) = metadata_messages(7, &applied, loc);
            no_raw_key(&m);
            no_raw_key(&s);
            assert!(
                m.contains("Tintin") && m.contains("Hergé") && m.contains('7'),
                "{loc}: {m}"
            );
            assert!(s.contains("Jacobs"), "{loc}: {s}");
            let (m, s) = metadata_messages(7, &nothing, loc);
            no_raw_key(&m);
            assert!(m.contains("Tintin"), "{loc}: {m}");
            assert!(!s.is_empty());
            for c in &conflicts {
                for kind in [ProposalKind::Metadata, ProposalKind::Cover] {
                    let (m, h) = stale_messages_of(7, kind, &StaleCause::Conflict(c.clone()), loc);
                    no_raw_key(&m);
                    no_raw_key(&h);
                }
            }
            let (m, _) =
                stale_messages_of(7, ProposalKind::Cover, &StaleCause::Targets(vec![]), loc);
            no_raw_key(&m);
            let field = stale_why_of(
                ProposalKind::Metadata,
                &StaleCause::Conflict(conflicts[0].clone()),
                loc,
            );
            assert!(
                field.contains(&field_label("publisher", loc)),
                "{loc}: {field}"
            );
            let m = error_localized(&AcceptError::CoverMissing, loc);
            no_raw_key(&m);
        }
    }

    // Story 13-9 — every label outcome is localized and links the record.
    #[test]
    fn label_outcomes_are_localized() {
        use crate::models::label::LabelTarget;
        let applied = AppliedLabel {
            target: LabelTarget::Volume(9),
            label_name: "Code douteux".into(),
            target_name: "V0009 — Tintin".into(),
        };
        for loc in LOCALES {
            let m = label_message(7, &applied, loc);
            no_raw_key(&m);
            assert!(
                m.contains("Code douteux") && m.contains("V0009"),
                "{loc}: {m}"
            );
            let html = applied_entry(7, &Applied::Label(applied.clone()), loc);
            assert!(html.contains("/volume/9"), "{html}");
            for cause in [
                StaleCause::Targets(vec![]),
                StaleCause::Conflict(AcceptConflict::LabelGone {
                    label: "Code douteux".into(),
                }),
                StaleCause::Conflict(AcceptConflict::LabelAttached {
                    label: "Code douteux".into(),
                }),
            ] {
                let (m, h) = stale_messages_of(7, ProposalKind::LabelAttach, &cause, loc);
                no_raw_key(&m);
                no_raw_key(&h);
            }
        }
        let gone = stale_why_of(
            ProposalKind::LabelAttach,
            &StaleCause::Conflict(AcceptConflict::LabelGone {
                label: "Code douteux".into(),
            }),
            "en",
        );
        assert!(gone.contains("Code douteux"), "{gone}");
    }

    #[test]
    fn a_cover_not_installed_is_a_warning() {
        let mut a = AppliedCover::for_test(4, "Tintin", true, true);
        let (ok, m, s) = cover_messages(7, &a, "en");
        assert!(ok && s.is_empty() && m.contains("replaces"), "{m}");
        a = AppliedCover::for_test(4, "Tintin", false, false);
        let (ok, m, s) = cover_messages(7, &a, "en");
        assert!(
            !ok && m.contains("keeps its previous cover") && !s.is_empty(),
            "{m}"
        );
        for loc in LOCALES {
            for (replaced, installed) in [(true, true), (false, true), (false, false)] {
                let (_, m, s) =
                    cover_messages(7, &AppliedCover::for_test(4, "T", replaced, installed), loc);
                no_raw_key(&m);
                no_raw_key(&s);
            }
        }
        let html = applied_entry(7, &Applied::Cover(a), "en");
        assert!(html.contains("/title/4"), "{html}");
        assert!(matches!(
            into_app_error(AcceptError::CoverMissing, "en"),
            AppError::Conflict(m) if m.contains("still pending")
        ));
    }

    #[test]
    fn every_outcome_is_localized() {
        let applied = AppliedSeries {
            series_id: 3,
            series_name: "Astérix".into(),
            created: true,
            titles_attached: 2,
            tomes_filled: 1,
            tomes_created: 3,
            tomes_settled: 1,
            tomes_skipped: vec![SkippedTome {
                position: 4,
                title: "T4".into(),
                owner_title: "Owned".into(),
                owner_position: 2,
            }],
        };
        let causes = [
            StaleCause::Targets(vec![StaleTarget {
                item_id: 1,
                target: crate::models::proposal::ProposalTarget {
                    table: crate::models::proposal::ProposalTargetTable::Titles,
                    id: 1,
                },
                reason: StaleReason::Changed,
            }]),
            StaleCause::Conflict(AcceptConflict::SeriesExists { name: "S".into() }),
            StaleCause::Conflict(AcceptConflict::TitleInSeries {
                title: "T".into(),
                position: 1,
            }),
            StaleCause::Conflict(AcceptConflict::PositionHeld {
                position: 2,
                holder: "H".into(),
            }),
            StaleCause::Conflict(AcceptConflict::TomePositionTaken {
                position: 3,
                tome_title: "X".into(),
            }),
            StaleCause::Conflict(AcceptConflict::TomeIsbnTaken {
                isbn: "9782070360246".into(),
                tome_title: "X".into(),
            }),
            StaleCause::Conflict(AcceptConflict::TomeIsbnMismatch {
                position: 4,
                tome_title: "X".into(),
                tome_isbn: "9782070360246".into(),
            }),
            StaleCause::Conflict(AcceptConflict::BeyondTotal {
                position: 9,
                total: 5,
            }),
        ];
        let errors = [
            AcceptError::NotFound,
            AcceptError::NotPending {
                status: ProposalStatus::Accepted,
            },
            AcceptError::NotPending {
                status: ProposalStatus::Stale,
            },
            AcceptError::Changed,
            AcceptError::WrongKind {
                kind: ProposalKind::Cover,
            },
            AcceptError::NothingToApply,
            AcceptError::UnknownDecider,
            AcceptError::Invalid("x".into()),
            AcceptError::Apply(AppError::Internal("secret db text".into())),
        ];
        for loc in LOCALES {
            let (m, s) = applied_messages(7, &applied, loc);
            no_raw_key(&m);
            no_raw_key(&s);
            assert!(m.contains("Astérix") && m.contains('7'), "{loc}: {m}");
            assert!(s.contains("Owned") && s.contains("T4"), "{loc}: {s}");
            let attached = AppliedSeries {
                created: false,
                tomes_filled: 0,
                tomes_skipped: vec![],
                ..applied.clone()
            };
            let (m, s) = applied_messages(7, &attached, loc);
            no_raw_key(&m);
            assert!(s.is_empty());
            // Story 13-11 — the filled tomes are counted.
            let filled = AppliedSeries {
                tomes_filled: 2,
                ..attached.clone()
            };
            let (_, s) = applied_messages(7, &filled, loc);
            no_raw_key(&s);
            assert!(s.contains('2'), "{loc}: {s}");
            for c in &causes {
                let (m, s) = stale_messages(7, c, loc);
                no_raw_key(&m);
                no_raw_key(&s);
            }
            for e in &errors {
                let m = error_localized(e, loc);
                no_raw_key(&m);
                assert!(!m.contains("secret"), "{loc}: {m}");
            }
            let summary = AcceptSummary {
                creates: true,
                series_name: "S".into(),
                titles: 2,
                tomes: 1,
            };
            for s in summary.sentences(loc) {
                no_raw_key(&s);
            }
            for s in (AcceptSummary {
                creates: false,
                ..summary.clone()
            })
            .sentences(loc)
            {
                no_raw_key(&s);
            }
        }
    }

    #[test]
    fn a_failed_apply_reads_as_nothing_changed() {
        let e = into_app_error(AcceptError::Apply(AppError::Internal("boom".into())), "en");
        match e {
            AppError::Conflict(m) => {
                assert!(m.contains("still pending"), "{m}");
                assert!(!m.contains("boom"));
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
    }
}
