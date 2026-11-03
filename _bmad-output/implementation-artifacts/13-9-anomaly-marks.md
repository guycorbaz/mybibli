# Story 13.9: The agent proposes a label on a doubtful record

Status: ready-for-dev

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

- [ ] T1 — migration (ENUM kind) + model/service for the new kind (filing, equivalence key, acceptance, review labels)
- [ ] T2 — MCP tools `list_labels`, `propose_label`; `get_title` labels
- [ ] T3 — web review rendering + i18n (4 locales)
- [ ] T4 — tests (AC6), `cargo sqlx prepare` if macros change
- [ ] T5 — CLAUDE.md notes; gates; full suites once before the single push
