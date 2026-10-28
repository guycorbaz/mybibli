# Story 11.13: Metadata proposal tool

Status: review

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR132 (with FR134, NFR44, NFR47, AR27, AR29 inherited from 11-5 / 11-6 / 11-9). Epic 11 — thirteenth story; the second kind of proposal an agent files. The agent never writes the catalog: each call files one `metadata` proposal through `ProposalService::file` (story 11-9). Acceptance is story 11-18; the proposals page is 11-16; editing before acceptance is 11-19.

## Story

As an AI agent,
I want to propose a title's missing information,
so that titles the providers never found become complete.

## Existing-code reality check (code map, 2026-10-28, `main` 63b2a8b)

| Place | Today | Consequence |
|---|---|---|
| `migrations/20261028120000_create_proposals.sql` | `proposals.kind` ENUM already has `metadata`; `ProposalKind::Metadata` exists in `src/models/proposal.rs` | no migration |
| `src/services/proposals.rs` (1311 lines) | `ProposalService::file(pool, &NewProposal)` (validates sources and items, share-locks a live `propose` key, reads each target's version, honours `expected_version` → `TargetChanged`, writes in one transaction, logs); item cap 64 KiB serialized | the metadata service builds a `NewProposal` and calls `file`; no duplicate rule needs the exclusive key lock of 11-12, so no own transaction |
| `src/services/series_proposals.rs` | `RefusalClass`, `FiledProposal` defined here; error → class → `ToolError` | move `RefusalClass` and `FiledProposal` to `services::proposals` (shared by every proposal kind), re-exported from `series_proposals` so 11-12 code does not move |
| `src/mcp/tools/series_proposal_args.rs` | `sources_property`, `filed_document`, `tool_error(SeriesProposalError, …)` | move the kind-independent helpers to a new `src/mcp/tools/proposal_args.rs`; `tool_error` becomes generic over a `Refusal` trait (`class()` + `Display` + `Debug`) |
| `src/routes/titles.rs::update_title` | title-edit validation: values trimmed, empty → NULL; publication date `YYYY-MM-DD` or `YYYY` (unparseable → silently NULL); no length check (column widths: `subtitle` VARCHAR(500), `publisher` VARCHAR(255), `dewey_code` VARCHAR(32), `description` TEXT) | the proposal validates explicitly: column widths, a year range, the Dewey rule below |
| `src/routes/api_v1.rs` (~436) | Dewey lite-validation inline: digits, `.` or `/` only | extract `utils::dewey_code_chars_valid` and use it from both (Rule 1) |
| `src/services/contributor.rs` | `find_or_create` / `add_to_title`: name trimmed, non-empty, `len() > 255` (bytes) refused; role by id, must exist (live) | the proposal applies the same name rule (shared constant) and resolves the role by **name** against live roles |
| `src/models/contributor.rs` | `TitleContributorModel::find_by_title(pool, id)` (name + role name, live rows); `ContributorRoleModel::find_all(pool)` (live `(id, name)`) | reused for the current contributors and the role lookup — no new SQL |
| `src/models/title.rs::TitleModel::find_by_id` | live title with `version`, `subtitle`, `description`, `publisher`, `publication_date`, `dewey_code` | reused for the current values |
| `get_title` MCP tool | returns those fields, contributors `{id, name, role}` and `version` | the agent reads, then pins `version` |
| `src/mcp/tools/mod.rs` | registry of 9 tools; `PROPOSE_TOOLS` (2) | 10 tools; `PROPOSE_TOOLS` gains `propose_title_metadata` |
| `tests/common/mcp.rs`, `tests/e2e/helpers/mcp.ts` | `ALL_TOOLS` / `PROPOSE_TOOLS` / `MCP_TOOLS` lists | add the tool |

## Acceptance Criteria

1. **AC1 — one tool, `propose` key only (FR132, NFR44).** `tools/list` adds `propose_title_metadata` (10 tools). Description precise enough to use without external docs (what is filed, that the catalog does not change until a librarian accepts, every refusal and bound); input schema closed (`additionalProperties: false`) at every level with a description on every property; annotations `readOnlyHint: false`, `destructiveHint: false`, `idempotentHint: false`. A `read` key is still refused by `/api/mcp`.
2. **AC2 — arguments.** `title_id` (required), `version?` (the title's `version` from `get_title`; a mismatch is refused, re-read and retry), `fields` (required object, at least one entry) with any of: `subtitle` (string), `description` (string), `publisher` (string), `publication_year` (integer), `dewey_code` (string), `contributors` (array of `{ name, role }`), and `sources` (required, http(s) URLs, 11-9 rules).
3. **AC3 — field validation, field by field.** Every refused field is named with the reason, all of them in one refusal (nothing is filed when any field is refused):
   - a field outside the list (e.g. `title`, `isbn`, `genre`) → "cannot be proposed", listing the fields that can;
   - a value of the wrong JSON type, `null`, or blank after trimming → refused (an agent completes information, it does not erase it);
   - `subtitle` > 500 characters, `publisher` > 255, `description` > 10 000, `dewey_code` > 32; a control character in a single-line field (all but `description`, which may hold line breaks and tabs);
   - `dewey_code` not made of digits, `.` and `/` with at least one digit (the API's rule, shared);
   - `publication_year` outside 1000..=next year;
   - `contributors`: empty list, more than 50 entries, a blank name or one longer than 255 bytes (the contributor service's rule), a control character in a name, a role that is not one of the library's live contributor roles (matched case-insensitively; the message lists the valid roles), the same name + role twice.
4. **AC4 — current next to proposed; unchanged fields dropped.** For each field the proposal stores the title's current value next to the proposed one. A field whose (normalised) proposed value equals the current value is dropped; a contributor whose name (case-insensitive, trimmed) and role are already on the title is dropped from the list, and the field is dropped when no contributor is left. When every field is dropped the call is refused ("nothing would change") and nothing is filed. The answer lists the dropped fields.
5. **AC5 — what is filed (FR134, AR29).** One proposal per call, kind `metadata`, attributed to the calling key, status `pending`, sources by 11-9's rules. One item per kept field, in the fixed order subtitle, description, publisher, publication_year, dewey_code, contributors, each targeting `titles/<id>` and recording the title's `version` (the pinned `version`, or the one read for the comparison — a title edited in between is refused with `TargetChanged`). Shapes (contract for 11-15, 11-16, 11-18, 11-19, 11-20, documented in the service module and D-11-13-2):
   - scalar field: `{"line": "field", "field": "<name>", "current": <value|null>, "proposed": <value>}` — strings for `subtitle`, `description`, `publisher`, `dewey_code`; integers for `publication_year` (current = the year of `publication_date`);
   - contributors: `{"line": "field", "field": "contributors", "current": [{"name", "role"}…], "proposed": [{"name", "role", "role_id"}…]}` — `proposed` lists contributors **to add**; acceptance never removes one. Whether a contributor already exists is settled at acceptance (11-18).
6. **AC6 — refusals.** Unknown or soft-deleted title → not found (points to `search_titles`). Key revoked / not `propose` → refused. `version` mismatch → refused (re-read). Every refusal is an `isError: true` result with an actionable English message; validation → `Invalid arguments: …`; a server error never leaks detail.
7. **AC7 — result.** `{ "proposal_id", "status": "pending", "kind": "metadata", "item_count", "unchanged_fields": [...] }`.
8. **AC8 — layering (AR27).** Thin tool `src/mcp/tools/propose_title_metadata.rs` over `services::metadata_proposals`; no SQL under `src/mcp/`; English agent-facing messages.
9. **AC9 — tests.** Unit: argument parsing, schema, every field rule, normalisation and comparison, item shapes, error mapping, registry (10 tools). Integration `tests/mcp_metadata_proposals.rs` over the real router: a nominal proposal (items, current values, versions, sources, key), an unchanged field dropped (and all-unchanged refused), invalid fields refused field by field (nothing written), a soft-deleted title, an unknown role, a `version` mismatch. E2E `mcp-metadata-proposal.spec.ts` (spec id `MD`): from a blank browser, admin scans a title and creates a `propose` key; the agent lists 10 tools, files a metadata proposal, sees an unchanged field reported, gets a field-by-field refusal; the title page is unchanged; key revoked.

## Tasks / Subtasks

- [x] T1 — Shared pieces: `RefusalClass` + `FiledProposal` moved to `services::proposals` (re-exported by `series_proposals`); `utils::dewey_code_chars_valid` used by `api_v1`; `MAX_CONTRIBUTOR_NAME_BYTES` constant in `services::contributor` used by its checks.
- [x] T2 — Service `src/services/metadata_proposals.rs`: request types, `MetadataProposalError` (English `Display`, `class()`), pure `validate_fields`, `resolve_roles`, `diff_against` (current vs proposed, items), `MetadataProposalService::propose` (read title + contributors + roles, pin version, `ProposalService::file`).
- [x] T3 — MCP: `src/mcp/tools/proposal_args.rs` (sources schema, filed document, generic `tool_error`), `propose_title_metadata.rs`; registry; registry test.
- [x] T4 — Integration tests `tests/mcp_metadata_proposals.rs`; tool counts in existing MCP tests / helpers.
- [x] T5 — E2E `tests/e2e/specs/journeys/mcp-metadata-proposal.spec.ts`; `MCP_TOOLS`.
- [x] T6 — `docs/route-role-matrix.md` MCP section; decisions D-11-13-*.

## Dev Notes

- **Item shapes are a contract** (D-11-13-2): `line` = `"field"`, `field` names the column-level field; 11-15's equivalence ("same set of fields") reads the `field` keys; 11-19 may remove an item or edit `proposed` (and must keep `current` as filed); 11-18 writes `proposed`, `publication_year` as `YYYY-01-01`, adds contributors by `role_id` (reuse a contributor with the same name and role, else create), marks written fields manually edited (FR144; `publication_year` is tracked as `publication_date`).
- **Staleness baseline (#521, decision belongs to 11-17/11-18):** every item records `titles.version` at filing (or the agent's pin). The scalar fields covered here are all written through statements that bump `titles.version` (edit form, API v1 PATCH, background metadata fetch). **Not** covered: `title_contributors` writes (manual add/remove, the fetch adding contributors) do not bump `titles.version`. The stored `current` values give 11-18 a field-level baseline it can compare to the live row whichever option #521 picks; for contributors, acceptance must skip a pair already present.
- **Normalisation and comparison:** proposed strings are trimmed (inner text kept as is); a scalar is unchanged when it equals the current value trimmed (`None` current never equals). `publication_year` compares with `publication_date`'s year. A contributor is unchanged when a current contributor has the same `role_id` and the same trimmed name compared with Rust `to_lowercase()` (accent-sensitive — at worst a near-duplicate reaches the librarian, and 11-18's reuse by the SQL collation settles it). Role names match the live roles with the same trimmed, `to_lowercase()` comparison; the stored `role` is the library's spelling.
- **No pending-duplicate rule** (D-11-13-5): a second metadata proposal for the same title is filed; accepting one bumps `titles.version`, so the other goes stale at its own acceptance.
- **No own transaction:** reads with the pool, then `ProposalService::file` with the pinned version — a concurrent edit between the read and the filing is refused by `TargetChanged`, so `current` is consistent with the recorded version for every scalar field.
- **Item size:** the item cap is 64 KiB serialized; a title whose current description is very long (> ~50 KB) could push the description item over it → mapped to a description-specific refusal.
- **Not here:** FR136 resubmission delay (11-15); applying anything (11-18); the web page (11-16).
- Heavy commands under `mem-guard`, cargo `-j 2`; `rustfmt --edition 2024` on touched leaf files only.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.13] — FR132; 11.15, 11.18, 11.19, 11.20 consumers.
- [Source: src/services/series_proposals.rs] — structure, refusal classes, item-shape documentation (D-11-12-2, -6, -7, -8).
- [Source: src/services/proposals.rs] — `ProposalService::file`, caps (D-11-9-5, -10, -11).
- [Source: src/routes/titles.rs#update_title], [Source: src/routes/api_v1.rs] — title-edit validation, Dewey rule.
- [Source: src/services/contributor.rs] — contributor name rule.

### Review Findings

Code review 2026-10-28 (bmad-code-review: Blind Hunter, Edge Case Hunter, Acceptance Auditor).

Round 1:
- [x] [Review][Patch] (Medium) Contributor names compared case-insensitively only, unlike the collation acceptance uses — `fold_name` for dedupe, "already on the title" and roles; NFC storage (D-11-13-9) [src/services/metadata_proposals.rs]
- [x] [Review][Patch] (Medium) A near-64 KiB current description / contributor list made the field unproposable, with misleading advice, classed as conflict, `index - 1` fallback untested — `fit_item` truncates `current` with `current_truncated`; `ItemTooLarge` removed
- [x] [Review][Patch] (Medium) `publication_year` hid the month and day acceptance erases — `current_date` in the item
- [x] [Review][Defer] (Medium, pre-existing) Re-adding a contributor whose junction row was soft-deleted fails on the unique key — #531; 11-18 contract notes it
- [x] [Review][Patch] (Low) Invisible / direction characters, unbounded echoed keys, role list repeated per error, "blank" for a missing key, huge-integer year, local-time bound, unpinned `TargetChanged` wording, Dewey schema pattern, description gaps, `tool_error` metadata test
- Accepted (Low): source errors after field errors; lite Dewey rule; no revoked-key test for this tool (transport guard); `description` escaping belongs to 11-16.

Round 2:
- [x] [Review][Patch] (Medium) Whitespace collapse ate no-break spaces and the unchanged check compared against the raw current value — `normalize_text` on both sides (D-11-13-10)
- [x] [Review][Patch] (Low) `fold_name` lower-cases first (Turkish İ); CRLF vs LF description is no change
- Accepted (Low): `fold_name` drops Devanagari-style vowel signs (documented).

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test` (unit + DB integration on the 3307 test DB): 1903 passed, 0 failed.
- `cargo sqlx prepare --check --workspace -- --all-targets`: up to date (no macro query added).
- `tsc --noEmit`: clean; `waitForTimeout` grep: clean.
- Playwright on a reset stack (`E2E_HOST_PORT=18080`, `CI=true --workers=2`): 369 passed, 2 skipped.

### Completion Notes List

- New service `services::metadata_proposals` (item-shape contract in its module doc, D-11-13-2) over `ProposalService::file`; typed `MetadataProposalError` implementing the new shared `Refusal` trait; pure `validate_fields` (all problems at once) and `diff_against` (current vs proposed, unchanged fields dropped).
- Shared pieces: `RefusalClass`, `FiledProposal`, `Refusal` in `services::proposals`; MCP `proposal_args` module; `utils::dewey_code_chars_valid` (API v1 reuses it); `MAX_CONTRIBUTOR_NAME_BYTES`.
- MCP tool `propose_title_metadata` (10 tools).
- Decisions D-11-13-1..8.

### File List

- `src/services/metadata_proposals.rs` (new), `src/services/mod.rs`, `src/services/proposals.rs`, `src/services/series_proposals.rs`, `src/services/contributor.rs`
- `src/mcp/tools/propose_title_metadata.rs` (new), `src/mcp/tools/proposal_args.rs` (new), `src/mcp/tools/series_proposal_args.rs`, `src/mcp/tools/propose_series_create.rs`, `src/mcp/tools/propose_series_attach.rs`, `src/mcp/tools/mod.rs`
- `src/utils.rs`, `src/routes/api_v1.rs`
- `tests/mcp_metadata_proposals.rs` (new), `tests/common/mcp.rs`, `tests/mcp_series_proposals.rs`
- `tests/e2e/specs/journeys/mcp-metadata-proposal.spec.ts` (new), `tests/e2e/helpers/mcp.ts`, `tests/e2e/specs/journeys/mcp-series-proposals.spec.ts`
- `docs/route-role-matrix.md`
- `_bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md`, `sprint-status.yaml`

## Change Log

- 2026-10-28 — story created (ready-for-dev).
- 2026-10-28 — validated against the create-story checklist: added the comparison / normalisation rules (dev notes); no AC change.
- 2026-10-28 — implemented (dev-story); status review.
- 2026-10-28 — code review round 1 (D-11-13-9); fixes applied.
- 2026-10-29 — code review round 2 (D-11-13-10); fixes applied.
