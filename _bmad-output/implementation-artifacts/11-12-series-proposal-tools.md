# Story 11.12: Series proposal tools

Status: review

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR130, FR131 (with FR134, NFR44, NFR47, AR27, AR29 inherited from 11-5 / 11-6 / 11-9). Epic 11 — twelfth story; the first MCP tools that *file* something. The agent never writes the catalog: each call files one proposal through `ProposalService::file` (story 11-9). Acceptance is story 11-17; the proposals page is 11-16.

## Story

As an AI agent,
I want to propose a new series, or attachments to an existing one,
so that the librarian can group volumes without searching by hand.

## Existing-code reality check (code map, 2026-10-28, `main` 4dc735e)

| Place | Today | Consequence |
|---|---|---|
| `src/services/proposals.rs` (1295 lines) | `ProposalService::file(pool, &NewProposal)` validates sources (D-11-9-5) and items (≤ 1000, object values ≤ 64 KiB, depth ≤ 20, total ≤ 1 MiB), share-locks a live `propose` key (`KeyNotAllowed`), reads each target's version (`TargetNotFound { index, table, id }`), honours an optional `expected_version` (`TargetChanged`), writes everything in one transaction; `ProposalError::is_invalid_input()` | the series service builds a `NewProposal` and calls `file`; it does not grow `proposals.rs` (Rule 12) — new `src/services/series_proposals.rs` |
| `src/models/proposal.rs` | `ProposalKind::{SeriesCreate, SeriesAttach}`, `ProposalTarget { table, id }`, `ProposalTargetTable::{Titles, Series}`; no query looks up pending proposals by target or by proposed name | two new model reads for the duplicate rule (AC5) |
| `src/mcp/tool.rs` | `ToolError::{InvalidArguments, NotFound, Internal}`; `read_only()` defaults to `true`; `ToolContext.api_key_id` carries the filing key | a refusal that is not an argument error (existing series, pending duplicate, revoked key) needs its own variant: `ToolError::Refused(String)` (message as is) |
| `src/mcp/transport.rs` | annotations: `read_only(tool.read_only())`, `idempotent(tool.read_only())`, `destructive(false)`, `open_world(false)` | proposal tools return `read_only() = false` → advertised `readOnlyHint: false`, `idempotentHint: false`, `destructiveHint: false` (they write only to the proposals queue) |
| `src/mcp/tools/mod.rs` | registry of 7 read tools; unit test asserts every tool is read-only and its description says "Read-only" | test split: read tools keep the assertion; proposal tools must say they never change the catalog |
| `src/mcp/tools/get_series.rs` | document has no `version`; `get_title` already returns `version` (`TitleDetail`) | add `version` to `get_series` so an agent can pin what it read (`expected_version`, D-11-9-11) |
| `SeriesService::agent_series_detail` | live series + live `title_series` rows (live titles) + live expected tomes | reused for the attach-time checks (AC4), no new SQL for them |
| `services::expected_tomes::{validate_title, normalize_isbn, check_position}` | tome rules of D-11-10-4 | reused for expected-tome lines so a proposal that passes filing does not fail acceptance on a malformed tome |
| `SeriesModel::active_find_by_name` | `name = ?`, column collation (server default charset in CI) | reused for "an active series with the same name" (AC3) |
| `tests/common/mcp.rs` | `Agent::{new, call, ok, err}`, `seed_key`, `add_title`, `soft_delete` | reused by the new integration test file |
| `tests/e2e/helpers/mcp.ts` | `MCP_TOOLS` (7 names), `mcpCallTool` | `MCP_TOOLS` gains the two tools |

## Acceptance Criteria

1. **AC1 — two tools, `propose` key only (FR130, FR131, NFR44).** `tools/list` adds `propose_series_create` and `propose_series_attach` (9 tools). Each has a description precise enough to use without external docs (what is filed, that nothing in the catalog changes until a librarian accepts, the refusals), an object input schema with `additionalProperties: false` at every level and a description on every property, and annotations `readOnlyHint: false`, `destructiveHint: false`, `idempotentHint: false`. A `read` key is still refused by `/api/mcp` (11-6 guard, unchanged).
2. **AC2 — arguments.**
   - `propose_series_create`: `name` (string, required), `series_type` (`open` | `closed`, required), `total_volume_count` (integer ≥ 1, required for `closed`, refused for `open`), `titles` (array of `{ title_id, position, is_omnibus?, end_position?, version? }`), `expected_tomes` (array of `{ position, title, isbn? }`), `sources` (array of http(s) URLs, required).
   - `propose_series_attach`: `series_id` (required), `series_version?`, `titles`, `expected_tomes`, `sources` (same item shapes).
   - At least one title or expected tome for attach; create may carry none (a bare series).
   - `is_omnibus: true` requires `end_position ≥ position` (the omnibus covers `position..=end_position`, as `assign_omnibus`); `end_position` is refused without `is_omnibus: true`.
   - Optional `version` / `series_version` pin the version the agent read (`get_title.version`, `get_series.version`); a mismatch is refused (`TargetChanged`, re-read and retry).
3. **AC3 — create refusals.** Refused with an actionable message: blank name or longer than 255 characters; `closed` without a total; a total for `open`; an **active series with the same name** (case- and accent-insensitive, `utf8mb4_unicode_ci` — D-11-12-11) → refused with a message naming the series id and telling the agent to call `propose_series_attach` with that `series_id`.
4. **AC4 — line refusals (both tools).** Refused, naming the offending title id / tome: an unknown or soft-deleted title; a position ≤ 0 (or beyond `i32`); a **position used twice in the proposal** (title, omnibus range or expected tome — ranges compared, never expanded); the same title twice; a position above the closed total (proposed total for create, the series' total for attach); an expected tome with a blank / > 500-character title or an invalid ISBN (expected-tome rules, D-11-10-4); two tomes with the same ISBN. Attach only, against the series as it is now: unknown or soft-deleted series (not found, points to `list_series`); a title already in the series; a position held by another live title; a position or ISBN of a live expected tome of the series.
5. **AC5 — pending duplicate.** A pending (live) proposal already targeting the same series refuses the new one and the message returns the existing proposal's id: for attach, a pending proposal with an item targeting `series/<id>`; for create, a pending `series_create` proposal whose series line has the same name (case- and accent-insensitive, `utf8mb4_unicode_ci`). A decided (accepted / rejected / stale) or soft-deleted proposal never blocks.
6. **AC6 — what is filed (FR134, AR29).** One proposal per call, kind `series_create` / `series_attach`, attributed to the calling key, sources stored by 11-9's rules, status `pending`. One item per line, in this order:
   1. the series line — create: `{"line":"series","name","series_type","total_volume_count"}` without target; attach: `{"line":"series"}` targeting `series/<id>`;
   2. one per title: `{"line":"title","position","is_omnibus","end_position"?}` targeting `titles/<id>`;
   3. one per expected tome: `{"line":"expected_tome","position","title","isbn"}` without target (ISBN normalised to ISBN-13 or null).
   Every targeted item records its target's `version` (staleness); expected-tome lines become `series_expected_tomes` rows with origin `proposal` when accepted (11-17 — the origin is fixed by the line type, not stored in the values).
7. **AC7 — result.** Success returns structured content `{ "proposal_id", "status": "pending", "kind", "item_count" }`. Every refusal is an `isError: true` result whose text says what was wrong and what to do; validation → `Invalid arguments: …`; not found → plain message; existing series / pending duplicate / key not allowed → plain message (`ToolError::Refused`); a server error never leaks detail.
8. **AC8 — layering (AR27).** Tools are thin adapters (`src/mcp/tools/propose_series_*.rs`): parse arguments, call `services::series_proposals`, map the typed `SeriesProposalError`. No SQL under `src/mcp/` (`no_sql_under_src_mcp` stays green). Agent-facing messages are English (D-11-9-2).
9. **AC9 — tests.** Unit: argument parsing and schemas, pure line validation (positions, overlaps, omnibus, totals, names, tomes), error → `ToolError` mapping, registry (9 tools, read/propose split). Integration `tests/mcp_series_proposals.rs` over the real router: nominal create (items, targets, versions, sources, key) and attach; each refusal of AC3–AC5 (nothing written); the pending duplicate returning its id for both tools, and a decided proposal not blocking; `version` pin mismatch; `get_series.version`. E2E `mcp-series-proposals.spec.ts`: from a blank browser, admin creates a `propose` key and a title, the agent lists 9 tools, files a create proposal, gets the duplicate refusal with the id, gets the "use attach" refusal for an existing series, files an attach proposal; key revoked.

## Tasks / Subtasks

- [x] T1 — `get_series` returns `version` (+ description, unit test) (AC2).
- [x] T2 — Model: `ProposalModel::pending_targeting(pool, table, id)` and `pending_series_create_named(pool, name)` (AC5).
- [x] T3 — Service `src/services/series_proposals.rs`: input types, `SeriesProposalError` (English `Display`), pure `validate_*` functions, `SeriesProposalService::{propose_create, propose_attach}` building the items of AC6 and mapping `ProposalError` target errors to title/series ids (AC2–AC6).
- [x] T4 — `ToolError::Refused`; tools `propose_series_create.rs`, `propose_series_attach.rs` with a shared argument module; registry + annotations; registry tests split (AC1, AC7, AC8).
- [x] T5 — Integration tests `tests/mcp_series_proposals.rs`; existing MCP test files updated for 9 tools (AC9).
- [x] T6 — E2E spec + `MCP_TOOLS` helper (AC9).
- [x] T7 — `docs/route-role-matrix.md` MCP section lists the proposal tools; decisions D-11-12-* logged.

## Dev Notes

- **Item shapes are a contract** for 11-16 (display), 11-17 (accept) and 11-19 (edit): keep `line` as the discriminant and the series line first. Document them on the service module.
- **Error mapping:** `SeriesProposalError::Proposal(ProposalError)` keeps 11-9's typed errors; `is_invalid_input()` → `InvalidArguments`; `TargetNotFound` / `TargetChanged` are rewritten by the series service to name the title / series id (the agent never sees 11-9's internal item index); `KeyNotAllowed` → `Refused`; `Database` and decision-only variants → `Internal` (logged).
- **Duplicate checks** run inside the filing transaction, after an exclusive lock on the filing key row, as plain reads (D-11-12-11, D-11-12-13).
- **Collation:** the pending-name lookup compares `CONVERT(JSON_UNQUOTE(JSON_EXTRACT(...)) USING utf8mb4) COLLATE utf8mb4_unicode_ci` with the bound name converted the same way — CI's server default charset must not change the result.
- **For 11-19 (edit before accept):** the series line is not a removable line (removing it leaves titles without a series); 11-19 must keep it.
- **Caps:** positions and totals ≤ 10 000, an omnibus ≤ 100 positions, ≤ 999 lines (D-11-12-9, review round 1).
- **Not here:** FR136 resubmission delay (11-15); applying anything (11-17); the web page (11-16).
- Heavy commands under `mem-guard`, cargo `-j 2`; `rustfmt --edition 2024` on touched leaf files only.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.12] — FR130, FR131.
- [Source: src/services/proposals.rs] — `ProposalService::file`, `ProposalError` (D-11-9-2, -5, -10, -11).
- [Source: src/services/expected_tomes.rs] — tome validation (D-11-10-4, D-11-10-11).
- [Source: src/mcp/tools/get_series.rs], [Source: src/mcp/tool.rs] — tool conventions (D-11-7-*, D-11-8-*).
- [Source: _bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md] — D-11-9-*, D-11-10-*, D-11-11-*.

### Review Findings

Code review 2026-10-28 (bmad-code-review: Blind Hunter, Edge Case Hunter, Acceptance Auditor).

Round 1:
- [x] [Review][Patch] (High) Omnibus range / total unbounded (`1..i32::MAX` → runaway acceptance, unbounded grid) — positions and totals ≤ 10 000, omnibus ≤ 100 positions (D-11-12-9) [src/services/series_proposals.rs]
- [x] [Review][Patch] (Medium) Item cap checked after an O(n²) validation — line cap (999) first, hash sets (D-11-12-9) [src/services/series_proposals.rs]
- [x] [Review][Patch] (Medium) Duplicate rule raced by parallel calls — checks in the filing transaction behind an exclusive key-row lock, `ProposalService::file_in` (D-11-12-11) [src/services/series_proposals.rs, src/services/proposals.rs, src/models/proposal.rs]
- [x] [Review][Patch] (Medium) Attach title line on an expected tome's position accepted (description said refused) — refused (D-11-12-10)
- [x] [Review][Patch] (Medium) AC9: several refusals only unit-tested — integration cases added; unblocking by accept / soft delete; concurrent filing test [tests/mcp_series_proposals.rs]
- [x] [Review][Patch] (Low) Existing-series check used the column collation — same `utf8mb4_unicode_ci` comparison as the pending check; normalised sources filed; descriptions list every refusal and bound; E2E checks the revoked key
- [x] [Review][Defer] (Low) E2E specs creating a series depend on file order — #528 (pre-existing)
- [x] [Review][Defer] (Low) Owned ISBN on a tome line, cross-key race, cross-proposal title conflicts — re-check at acceptance, #529
- Dismissed (round 1): existing omnibus covering a proposed position (one `title_series` row per covered position, so the equality checks are complete — verified by two layers); `Tome` message names the position rather than the title (identifies the line); KeyNotAllowed through the router (the guard refuses first; unit-tested mapping).

Round 2:
- [x] [Review][Patch] (Medium) Share-locking duplicate reads on unindexed predicates locked every series row / whole index ranges — deadlock-prone against an acceptance (11-16/17) and between keys; now plain reads after the key lock (D-11-12-13) [src/models/proposal.rs]
- [x] [Review][Patch] (Medium) Attach without `series_version` recorded a baseline newer than the state it was checked against — the version read for the checks is now pinned (D-11-12-13) [src/services/series_proposals.rs]
- [x] [Review][Patch] (Low) "Proposal filed" logged before the commit — `log_filed` after commit; line-cap test now proves the cap runs first; `version` wording narrowed to the series' own fields (#521); stale spec / module notes refreshed
- Accepted (Low, round 2): `utf8mb4_unicode_ci` weighs all supplementary-plane characters equally (emoji-only differences read as the same name — a false "already exists", never a wrong write); the concurrency test is scheduling-dependent; attach refusals that share `validate_lines` are exercised through create; attach checks against titles/tomes read before the transaction (re-checked at acceptance, #529); a pending create for a name does not block an attach on a hand-created series of that name.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test` (unit + DB integration on the 3307 test DB): 1880 passed, 0 failed.
- `cargo sqlx prepare --check --workspace -- --all-targets`: up to date (no macro query added).
- `tsc --noEmit`: clean; `waitForTimeout` grep: clean.
- Playwright on a reset stack (`E2E_HOST_PORT=18080`, `CI=true --workers=2`): 368 passed, 2 skipped.

### Completion Notes List

- New service `services::series_proposals` (contract of the item shapes in its module doc) over `ProposalService::file_in` (new transaction-scoped body of `file`); typed `SeriesProposalError` with `class()`.
- Two model reads for the pending-duplicate rule (`ProposalModel::pending_targeting`, `pending_series_create_named`).
- MCP: `propose_series_create`, `propose_series_attach`, shared `series_proposal_args`; `ToolError::Refused`; `get_series.version`.
- Decisions D-11-12-1..13 (9..13 from code review).

### File List

- `src/services/series_proposals.rs` (new), `src/services/mod.rs`
- `src/models/proposal.rs`
- `src/mcp/tool.rs`, `src/mcp/tools/mod.rs`, `src/mcp/tools/get_series.rs`
- `src/mcp/tools/propose_series_create.rs`, `src/mcp/tools/propose_series_attach.rs`, `src/mcp/tools/series_proposal_args.rs` (new)
- `tests/mcp_series_proposals.rs` (new), `tests/common/mcp.rs`, `tests/mcp_catalog_read_tools.rs`, `tests/mcp_series_gaps_tools.rs`
- `tests/e2e/specs/journeys/mcp-series-proposals.spec.ts` (new), `tests/e2e/helpers/mcp.ts`
- `docs/route-role-matrix.md`
- `_bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md`, `sprint-status.yaml`

## Change Log

- 2026-10-28 — story created (ready-for-dev).
- 2026-10-28 — validated against the create-story checklist: added the 11-19 note on the series line and the `i32` bound; no AC change.
- 2026-10-28 — implemented (dev-story); status review.
