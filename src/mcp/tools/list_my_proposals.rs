//! Story 11-15 — `list_my_proposals`: the calling key's own proposals,
//! their status, decision date and rejection reason (FR135).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ListPage, args, page_property, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::proposal::{ProposalKind, ProposalStatus};
use crate::services::proposal_history::{
    HistoryEntry, MAX_RESUBMISSION_DELAY_DAYS, ProposalHistoryService, rfc3339_utc,
};

pub struct ListMyProposals;

const TOOL: &str = "list_my_proposals";
const EXPECTED: &str = "{\"status\"?: string, \"kind\"?: string, \"page\"?: integer}";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    status: Option<String>,
    kind: Option<String>,
    page: Option<u32>,
}

fn names<T>(all: &[T], name: fn(&T) -> &'static str) -> Vec<&'static str> {
    all.iter().map(name).collect()
}

/// Validated `(status, kind, page)` (pure — unit tested).
fn parse_args(
    args: Value,
) -> Result<(Option<ProposalStatus>, Option<ProposalKind>, u32), ToolError> {
    let a: Args = args::parse_object(args, EXPECTED)?;
    let status = match a.status.as_deref() {
        None => None,
        Some(s) => Some(ProposalStatus::parse(s).ok_or_else(|| {
            ToolError::InvalidArguments(format!(
                "unknown `status`; use one of: {}",
                names(&ProposalStatus::ALL, ProposalStatus::as_str).join(", ")
            ))
        })?),
    };
    let kind = match a.kind.as_deref() {
        None => None,
        Some(k) => Some(ProposalKind::parse(k).ok_or_else(|| {
            ToolError::InvalidArguments(format!(
                "unknown `kind`; use one of: {}",
                names(&ProposalKind::ALL, ProposalKind::as_str).join(", ")
            ))
        })?),
    };
    Ok((status, kind, args::page(a.page)?))
}

#[derive(Debug, Serialize)]
struct Item {
    proposal_id: u64,
    kind: &'static str,
    status: &'static str,
    filed_at: String,
    decided_at: Option<String>,
    rejection_reason: Option<String>,
    item_count: i64,
    subject: Option<Value>,
    resubmittable_from: Option<String>,
}

impl From<HistoryEntry> for Item {
    fn from(e: HistoryEntry) -> Self {
        Item {
            proposal_id: e.proposal_id,
            kind: e.kind.as_str(),
            status: e.status.as_str(),
            filed_at: rfc3339_utc(e.filed_at),
            decided_at: e.decided_at.map(rfc3339_utc),
            rejection_reason: e.rejection_reason,
            item_count: e.item_count,
            subject: e.subject.map(|s| s.to_json()),
            resubmittable_from: e.resubmittable_from.map(rfc3339_utc),
        }
    }
}

impl McpTool for ListMyProposals {
    fn name(&self) -> &'static str {
        TOOL
    }

    fn title(&self) -> &'static str {
        "List my proposals"
    }

    fn description(&self) -> &'static str {
        "List the proposals filed with YOUR API key (never another key's), newest first, 25 \
         per page; pass `page` (1-based) until `page` equals `total_pages`. Filter with \
         `status` (`pending`, `accepted`, `rejected`, `stale`) and `kind` (`series_create`, \
         `series_attach`, `metadata`, `cover`). Each item gives `proposal_id`, `kind`, \
         `status`, `filed_at`, `decided_at` (null while pending; set when a librarian \
         accepted or rejected it, or when it went stale because its target changed or the \
         key was revoked), `rejection_reason` (the librarian's optional explanation, null \
         otherwise), `item_count`, and `subject`: what it was about — `{series_name}` for a \
         series creation, `{series_id}` for additions to a series, `{title_id, fields}` for \
         metadata, `{title_id}` for a cover (as the proposal now stands; a librarian may have \
         edited it). For a rejected proposal, `resubmittable_from` is the instant from which \
         an equivalent proposal is accepted again: the newest rejection of an equivalent \
         proposal (by any key) plus the library's resubmission delay, at most 3650 days; it \
         may be in the past, and it is null when the proposal blocks nothing. Equivalent means the same kind and target: the same series name (case and accents \
         ignored) for a creation, the same series for additions, the same title and the same \
         set of fields for metadata, the same title for a cover. Before that instant an \
         equivalent proposal is refused, whichever key files it — read the reason and propose \
         something else. Dates are RFC 3339 UTC. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": names(&ProposalStatus::ALL, ProposalStatus::as_str),
                    "description": "Only proposals in this status. Omitted: every status."
                },
                "kind": {
                    "type": "string",
                    "enum": names(&ProposalKind::ALL, ProposalKind::as_str),
                    "description": "Only proposals of this kind. Omitted: every kind."
                },
                "page": page_property("proposals")
            },
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let (status, kind, page) = parse_args(args)?;
            let list = ProposalHistoryService::list_mine(
                &ctx.state.pool,
                ctx.api_key_id,
                status,
                kind,
                page,
            )
            .await?;
            to_json(ListPage::<Item>::from(list), TOOL)
        })
    }
}

// The description quotes the delay bound.
const _: () = assert!(MAX_RESUBMISSION_DELAY_DAYS == 3650);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::proposal_history::ProposalSubject;
    use chrono::NaiveDate;

    #[test]
    fn parse_args_defaults_filters_and_errors() {
        assert_eq!(parse_args(json!({})), Ok((None, None, 1)));
        assert_eq!(
            parse_args(json!({"status": "rejected", "kind": "cover", "page": 3})),
            Ok((Some(ProposalStatus::Rejected), Some(ProposalKind::Cover), 3))
        );
        let err = parse_args(json!({"status": "refused"})).unwrap_err();
        assert!(
            matches!(&err, ToolError::InvalidArguments(m) if m.contains("pending, accepted, rejected, stale")),
            "{err:?}"
        );
        let err = parse_args(json!({"kind": "series"})).unwrap_err();
        assert!(
            matches!(&err, ToolError::InvalidArguments(m) if m.contains("series_create, series_attach, metadata, cover")),
            "{err:?}"
        );
        for bad in [json!({"page": 0}), json!({"api_key_id": 2}), json!([1])] {
            assert!(
                matches!(parse_args(bad.clone()), Err(ToolError::InvalidArguments(_))),
                "{bad} should be refused"
            );
        }
    }

    #[test]
    fn item_document_shape() {
        let at = NaiveDate::from_ymd_opt(2026, 10, 29)
            .unwrap()
            .and_hms_opt(8, 30, 0)
            .unwrap();
        let item = Item::from(HistoryEntry {
            proposal_id: 7,
            kind: ProposalKind::Cover,
            status: ProposalStatus::Rejected,
            filed_at: at,
            decided_at: Some(at),
            rejection_reason: Some("Blurry".into()),
            item_count: 1,
            subject: Some(ProposalSubject::Cover { title_id: 4 }),
            resubmittable_from: Some(at),
        });
        assert_eq!(
            serde_json::to_value(item).unwrap(),
            json!({
                "proposal_id": 7, "kind": "cover", "status": "rejected",
                "filed_at": "2026-10-29T08:30:00Z", "decided_at": "2026-10-29T08:30:00Z",
                "rejection_reason": "Blurry", "item_count": 1,
                "subject": {"title_id": 4},
                "resubmittable_from": "2026-10-29T08:30:00Z"
            })
        );
    }
}
