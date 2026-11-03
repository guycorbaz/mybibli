# Story 13.9: The agent proposes a label on a doubtful record

Status: review

GitHub issue: #581 (type:change-request). Covers: FR155, FR156 (as revised), UX-DR37 (as revised). Epic 13; ships in v1.23.0.

## Story

As a librarian working with an agent,
I want the agent to propose attaching one of my labels (e.g. "Code douteux") to a title or a volume,
so that doubtful records found by the agent are marked for my review without the agent changing the catalog.

## Decision (project lead, 2026-11-03 — revises the epic plan, recorded in #581)

Reuse the existing **entity labels** (CR #443: librarian-defined labels attached to titles and volumes, Librarian+ only, `/labels/{id}` lists what carries each) instead of a distinct anomaly notion. No per-record reason. The epic text of Story 13.9 (anomaly table, reason, anomalies list) is superseded: FR155 is met by labels as they exist (mark by hand, list, clear by hand); this story delivers FR156 — the agent proposes a label.

## Existing-code reality check (to verify at story start)

| Place | Today |
|---|---|
| `src/models/label.rs` | `LabelModel` (`list_all`, `find_by_id`, …), `LabelTarget` (title / volume, l. 269), `attach` (l. 308, idempotent?), `detach`, `list_for`, `list_titles_for`, `list_volumes_for` |
| `src/routes/entity_labels.rs` | attach/detach handlers on title and volume pages (Librarian+) |
| `proposals.kind` ENUM (`series_create`, `series_attach`, `metadata`, `cover`) | a new kind needs a migration (`ALTER TABLE … MODIFY kind ENUM(…, 'label_attach')`) |
| `src/services/proposals.rs` (`ProposalService::file_in`, statuses, `equivalence_key`, `log_filed`), `src/services/acceptance.rs` (one route `POST /proposals/{id}/accept`, kinds dispatched to `*_acceptance.rs`), `proposal_review.rs` / `routes/proposals.rs` (`ProposalLabels::t`), `proposal_history.rs` | the proposal lifecycle — follow the cover/metadata kinds as templates |
| `src/mcp/tools/mod.rs` (`registry()`, `PROPOSE_TOOLS`), `src/mcp/tools/proposal_args.rs`, `tests/common/mcp.rs`, `MCP_TOOLS` in `tests/e2e/helpers/mcp.ts` | adding a propose tool (closed schema, description containing "does not change the catalog" and "`sources`") |
| `src/mcp/tools/get_title.rs` | title + volumes (13-6 added `location_path`, 13-7 will add dates) — no labels today |
| CLAUDE.md, Proposal lifecycle | lock scope rule (target index / PK list), staleness, equivalence/resubmission delay |

## Acceptance Criteria

1. **AC1 — read side.** A new read tool `list_labels` (read_only) returns the library's labels (id, name); `get_title` returns the labels carried by the title and by each of its volumes (names + ids). Librarian-only data is fine for an API key (keys are created by an admin).
2. **AC2 — propose.** A new tool `propose_label` takes `{target: {title_id} | {volume_id}, label_id (or label name, exact match), sources[] (required, D-11-9-5 rules), note? (≤ 500 chars, shown to the librarian)}`; it files a `label_attach` proposal (one item: target kind + id, label id + name snapshot) and does not change the catalog. Refusals (mapped through `services::proposals::Refusal` → `ToolError`): unknown/deleted target, unknown label, label already attached (nothing to propose), an equivalent proposal already pending (same target + label), rejection block per the resubmission delay (equivalence key `label:<target>:<label_id>`). Volume ids are accepted only for live volumes.
3. **AC3 — review.** The proposals list and detail show the kind (localized label "Label"), the target (title or volume label + title), the label name, the note and the sources; accept/reject via the existing modals.
4. **AC4 — accept.** Accepting attaches the label (idempotent if attached meanwhile → accepted, nothing to change, or stale per the existing rule — pick the existing convention for "already applied"); a deleted target or a deleted label → stale; follows the acceptance transaction and lock-scope rules.
5. **AC5 — key revocation and history.** Revoking the key stales its pending label proposals (no staged files); history/resubmission behave like the other kinds.
6. **AC6 — tests.** Unit (argument parsing, refusal mapping, equivalence key). DB integration (`tests/mcp_label_proposals.rs`): file, refusals, accept attaches, reject, stale on deleted target/label, revocation, lock-scope regression test (accepting a label proposal on one title does not lock proposals of another — like `an_acceptance_on_one_title_does_not_lock_proposals_of_another`). E2E (specId `LB`): an agent (scripted MCP client, see `mcp-*.spec.ts`) lists labels, proposes "Code douteux" on a volume; the librarian accepts; the volume page shows the label and `/labels/{id}` lists it. Registry test, `tests/common/mcp.rs` and `MCP_TOOLS` updated.
7. **AC7 — docs.** CLAUDE.md MCP/proposal-lifecycle notes mention the new kind; the MCP `initialize` instructions are unaffected unless they enumerate tools (check `src/mcp/instructions.rs`).

## Tasks / Subtasks

- [x] T1 — migration (ENUM kind) + model/service for the new kind (filing, equivalence key, acceptance, review labels)
- [x] T2 — MCP tools `list_labels`, `propose_label`; `get_title` labels
- [x] T3 — web review rendering + i18n (4 locales)
- [x] T4 — tests (AC6), `cargo sqlx prepare` if macros change
- [x] T5 — CLAUDE.md notes; gates; full suites once before the single push

## Completion Notes

**What was done**
- Migration `20261104090000_proposals_label_attach.sql`: `proposals.kind` gains `label_attach` (appended); `proposal_items.target_table` gains `volumes` (a label may target a volume).
- Model: `ProposalKind::LabelAttach`, `ProposalTargetTable::Volumes` (+ `noun()`), `SubjectLineRow.label_id` (and `name` falls back to `label_name`), `ProposalModel::{live_volume_briefs[_in], pending_label_attach}` (duplicate read from the target index), `VolumeBrief`. `LabelModel::{attach_in (upsert), find_by_id_in, find_by_exact_name_in, is_attached_in, list_for_volumes}`; `attach` now delegates to `attach_in`.
- Services: new `label_proposals.rs` (filing, refusals, item shape documented, `library_labels`) and `label_acceptance.rs` (acceptance). `ProposalSubject::LabelAttach` with key `label:<table>/<id>:<label_id>`; `version_counts` false; acceptance dispatch, `AcceptConflict::{LabelGone, LabelAttached}`, `Applied::Label`; review (`TargetLabel::Volume`, `label` line with note and label link, list row "Label: …"); edit refuses edit/removal of the single label line; localized modal/outcome/stale copy (4 locales).
- MCP: `list_labels` (read-only), `propose_label`; `get_title` returns `labels` and `volumes[].labels` (the `/api/v1` document is unchanged — `labels` is skipped there); `list_my_proposals` docs; registry + `PROPOSE_TOOLS`, `tests/common/mcp.rs`, `MCP_TOOLS` updated. `initialize` instructions do not enumerate tools: unchanged.
- CLAUDE.md "Proposal lifecycle": label proposals paragraph.

**Decisions / deviations**
- "Already attached meanwhile" → `stale` (convention of the series kinds, #521). A hand attach committed in the instant between the transaction snapshot and the target lock is absorbed by the upsert (accepted, nothing added) — no locking read on the join table, so two acceptances on different records never contend (a `FOR UPDATE` on a missing link would take gap locks and deadlock on insert).
- Label by name: byte-exact match as specified; the refusal lists the library's labels.
- Revocation needed no change (`revoke_key` stales every pending proposal; no staged file).
- No `sqlx::query!` macro added: no `.sqlx` change.

**Tests**
- Unit: label_proposals, label_acceptance, list_labels, propose_label, proposal_history (key), proposal_review (line + target), proposal_accept (outcomes, 4 locales), proposal_edit (keep/editable), routes row_extras.
- DB integration `tests/mcp_label_proposals.rs` (11 tests, incl. lock-scope `an_acceptance_on_one_record_does_not_lock_proposals_of_another`).
- E2E `tests/e2e/specs/journeys/mcp-label-proposal.spec.ts` (specId `LB`, V-codes V4500–V4999).
- Full suites not run here (the orchestrator runs them once before the push).

No file above 2000 lines (largest touched: `proposal_edit.rs` 1723).
