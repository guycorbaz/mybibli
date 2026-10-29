//! Story 11-17 (FR138, FR140) — the librarian-facing side of accepting a
//! series proposal: the modal's summary and the localized copy of every
//! outcome (`docs/error-message-style.md`: what happened → why → what to
//! do; no internals). The acceptance itself is
//! [`crate::services::series_acceptance`].

use crate::error::AppError;
use crate::models::proposal::{ProposalKind, ProposalStatus};
use crate::services::proposal_review::{ReviewDetail, ReviewError, TargetLabel};
use crate::services::series_acceptance::{AcceptConflict, AcceptError, AppliedSeries, StaleCause};

/// Whether the page offers "Accept" for a proposal (pending series
/// proposals; metadata and cover acceptance is story 11-18).
pub fn can_accept(kind: ProposalKind, status: ProposalStatus) -> bool {
    status == ProposalStatus::Pending
        && matches!(
            kind,
            ProposalKind::SeriesCreate | ProposalKind::SeriesAttach
        )
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
    let suggestion = applied
        .tomes_skipped
        .iter()
        .map(|s| {
            rust_i18n::t!(
                "proposals.applied_skipped",
                locale = loc,
                title = s.title.as_str(),
                position = s.position,
                owner = s.owner_title.as_str(),
                owner_position = s.owner_position
            )
            .to_string()
        })
        .collect::<Vec<_>>()
        .join(" ");
    (message, suggestion)
}

/// Why the proposal went stale, as a clause.
pub fn stale_why(cause: &StaleCause, loc: &str) -> String {
    use rust_i18n::t;
    match cause {
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
            AcceptConflict::BeyondTotal { position, total } => t!(
                "proposals.stale_why_beyond_total",
                locale = loc,
                position = position,
                total = total
            ),
        },
    }
    .to_string()
}

/// The warning entry of a stale outcome: what happened and why, then
/// what to do.
pub fn stale_messages(id: u64, cause: &StaleCause, loc: &str) -> (String, String) {
    (
        rust_i18n::t!(
            "proposals.stale_feedback",
            locale = loc,
            id = id,
            why = stale_why(cause, loc).as_str()
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
        | AcceptError::Apply(_) => AppError::Conflict(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::proposals::{StaleReason, StaleTarget};
    use crate::services::series_acceptance::SkippedTome;

    const LOCALES: [&str; 4] = ["en", "fr", "de", "it"];

    fn no_raw_key(s: &str) {
        let raw = s
            .match_indices("proposals.")
            .any(|(i, m)| s[i + m.len()..].starts_with(|c: char| c.is_ascii_lowercase()));
        assert!(!raw, "raw key leaked: {s}");
        assert!(!s.contains("%{"), "unfilled placeholder: {s}");
    }

    #[test]
    fn only_pending_series_proposals_can_be_accepted() {
        assert!(can_accept(
            ProposalKind::SeriesCreate,
            ProposalStatus::Pending
        ));
        assert!(can_accept(
            ProposalKind::SeriesAttach,
            ProposalStatus::Pending
        ));
        assert!(!can_accept(ProposalKind::Metadata, ProposalStatus::Pending));
        assert!(!can_accept(ProposalKind::Cover, ProposalStatus::Pending));
        for s in [
            ProposalStatus::Accepted,
            ProposalStatus::Rejected,
            ProposalStatus::Stale,
        ] {
            assert!(!can_accept(ProposalKind::SeriesCreate, s));
        }
    }

    #[test]
    fn every_outcome_is_localized() {
        let applied = AppliedSeries {
            series_id: 3,
            series_name: "Astérix".into(),
            created: true,
            titles_attached: 2,
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
                tomes_skipped: vec![],
                ..applied.clone()
            };
            let (m, s) = applied_messages(7, &attached, loc);
            no_raw_key(&m);
            assert!(s.is_empty());
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
