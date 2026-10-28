# Story 11.9: Proposals schema and service

Status: in-progress

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR134, FR140, NFR47, AR29. Epic 11 — ninth story; blocks every proposal story (11-12 series tools, 11-13 metadata, 11-14 cover, 11-15 history, 11-16..11-19 proposals page). No MCP tool, no route, no template, no locale key in this story: it lays the schema and the service the next stories call.

## Story

As a librarian,
I want every agent proposal stored with its sources and the state of what it targets,
so that I can review it safely later.

## Existing-code reality check (code map, 2026-10-28, `main` 387ba9f)

| Place | Today | Consequence |
|---|---|---|
| `migrations/` | last file `20261028000000_api_keys_scope_propose.sql`; entity tables follow AR24 (`id BIGINT UNSIGNED`, `created_at`/`updated_at` TIMESTAMP, `deleted_at` TIMESTAMP NULL, `version INT NOT NULL DEFAULT 1`); test DB is `mariadb:10.11` (CHECK constraints enforced, `JSON` = `LONGTEXT` + `json_valid` check) | new `20261028120000_create_proposals.sql` with `proposals` + `proposal_items` |
| `api_keys` | `id BIGINT UNSIGNED`; never hard-deleted (soft `deleted_at`, `revoked_at`); not in `ALLOWED_TABLES` nor in the auto-purge order | `proposals.api_key_id NOT NULL` FK → `api_keys(id)` (RESTRICT is safe: no code path hard-deletes a key) |
| `users` | hard-purged 30 days after deactivation by `services::auto_purge` (story 8-7, issue #69); `admin_audit.user_id` is `ON DELETE SET NULL` for that reason | `proposals.decided_by` FK → `users(id)` **`ON DELETE SET NULL`**, otherwise a decided proposal would block the purge of its decider |
| `titles`, `series` | hard-purged 30 days after soft delete | `proposal_items.target_id` carries **no FK** (polymorphic target + must not block the purge); a purged target reads as "deleted" → stale |
| `src/models/*.rs` | dynamic `sqlx::query` + `Row::try_get` is the norm (6 macro calls repo-wide); `PaginatedList::new`, `DEFAULT_PAGE_SIZE = 25` in `models/mod.rs` | same style; TIMESTAMP columns read with `CAST(col AS DATETIME)` (dynamic-query gotcha), JSON with `CAST(col AS CHAR)` |
| `src/mcp/tool.rs` | `From<AppError> for ToolError` maps **every** `AppError` to `Internal` | filing errors must stay typed (`ProposalError`) so 11-12..11-14 can turn a validation refusal into an `InvalidArguments` the agent can act on |
| `AppState.covers_dir` (`src/lib.rs`, built in `main.rs` from `COVERS_DIR`, default `./covers`, served by `ServeDir` at `/covers`) | 36 files construct `AppState` literally (tests) | the staging directory is **derived** from `covers_dir` (sibling `<name>-staging`), exposed by `AppState::cover_staging_dir()`, created at boot — no new `AppState` field, never served |
| `tests/seed_drift.rs` | `ENTITY_TABLES_WITH_SOFT_DELETE` asserts the AR24 quartet on a fresh DB | extend it with the two new tables (AR24 regression net) |
| `src/services/admin_health.rs` | entity counts over a fixed table list (`information_schema` used only to probe `deleted_at`) | untouched — proposals are not catalogue entities; the dashboard indicator is 11-21 |
| `reqwest` 0.12 | re-exports `url::Url` as `reqwest::Url` | source URLs parsed with `reqwest::Url` — no new dependency |

## Acceptance Criteria

1. **AC1 — migration (AR29, AR24).** `proposals`: `id`, `kind ENUM('series_create','series_attach','metadata','cover')`, `status ENUM('pending','accepted','rejected','stale') DEFAULT 'pending'`, `api_key_id` (NOT NULL, FK `api_keys`), `sources JSON NOT NULL` (array of URL strings), `decided_by` (NULL, FK `users` ON DELETE SET NULL), `decided_at TIMESTAMP NULL`, `rejection_reason TEXT NULL`, AR24 quartet; indexes on `(status, deleted_at)`, `api_key_id`, `deleted_at`. `proposal_items`: `id`, `proposal_id` (FK `proposals`), `target_table ENUM('titles','series') NULL`, `target_id BIGINT UNSIGNED NULL`, `target_version INT NULL`, `proposed_values JSON NOT NULL`, AR24 quartet; a CHECK constraint makes the three target columns all NULL (a line that creates something new: the series of a `series_create`, an expected tome) or all set; indexes on `proposal_id`, `(target_table, target_id)`, `deleted_at`. InnoDB, utf8mb4.
2. **AC2 — sources (FR134, NFR47).** Filing refuses, with an explicit message naming the offending source by its 1-based index: no source; more than 20; an empty one; one longer than 2048 characters; one that does not parse as an absolute URL; a scheme other than `http`/`https` (e.g. `javascript:`, `data:`, `ftp:`, `file:`); no host; embedded credentials (`user:pass@`). Accepted sources are trimmed, stored in their parsed (normalised) form, duplicates dropped keeping first-seen order.
3. **AC3 — items.** Filing refuses: no item; more than 1000 items; `proposed_values` that is not a JSON object; an item's serialized values over 64 KiB; a target that does not exist or is soft-deleted (message names the item index, table and id). The service reads each target's current `version` itself at filing time (the caller never supplies it) and stores it in `target_version`. Proposal + items are written in one transaction; the proposal starts `pending`, `version = 1`, attributed to the filing key (FR134).
4. **AC4 — listing.** `list(filter { status?, kind?, api_key_id? }, page)` returns a `PaginatedList` (25 per page) of summaries `{ id, kind, status, api_key_id, api_key_label, item_count (live items), created_at, decided_at }` newest first (`created_at DESC, id DESC`); soft-deleted proposals and items never appear or count. The key label is shown even for a revoked key.
5. **AC5 — detail.** `detail(id)` returns the proposal (all columns, `sources` decoded, `decided_by` + the decider's username when the row still exists) and its live items in filing order (`id ASC`), `proposed_values` decoded; `None` for an unknown or soft-deleted id.
6. **AC6 — transitions.** Only `pending → accepted | rejected | stale`. Accept and reject require the deciding user id and set `decided_by`, `decided_at = NOW()`; reject takes an optional reason (trimmed, empty → NULL, at most 1000 characters, refused otherwise); stale sets `decided_at = NOW()` and leaves `decided_by` NULL (the system decided). Every transition bumps `version`. Any transition out of `accepted`, `rejected` or `stale` — and `pending → pending` — is refused (`NotPending`, carrying the current status); unknown/soft-deleted id → `NotFound`. The status guard is part of the `UPDATE … WHERE status = 'pending'`, so two concurrent deciders cannot both win. A transaction-scoped variant exists for story 11-17 (apply + accept in one transaction).
7. **AC7 — staleness (FR140).** `stale_targets(id)` lists every live item whose target row is missing, soft-deleted, or whose `version` differs from `target_version` (`{ item_id, table, target_id, reason: deleted | changed }`); items without a target are never stale. `check_staleness(id)` locks the proposal row, and when it is `pending` with at least one stale target, marks it `stale` (AC6 rules); it returns the resulting status and the stale targets. On a non-pending proposal it changes nothing and returns its current status. A transaction-scoped variant exists for story 11-17.
8. **AC8 — cover staging directory.** `services::cover_staging::staging_dir_for(covers_dir)` = sibling directory `<covers dir name>-staging` (`./covers` → `./covers-staging`, `/app/covers` → `/app/covers-staging`; a path without a final name such as `.` falls back to `<path>/covers-staging`); `AppState::cover_staging_dir()` returns it; `main.rs` creates it at boot next to the covers directory and logs it; a creation failure is logged as a warning and does not stop the boot (nothing uses it yet). It is not served by any route and nothing writes to it yet (11-14).
9. **AC9 — JSON & sqlx.** JSON columns read with `CAST(… AS CHAR)`, TIMESTAMP columns with `CAST(… AS DATETIME)` in dynamic queries; `deleted_at IS NULL` on every read of `proposals`, `proposal_items`, `titles`, `series`; `cargo sqlx prepare` run (no macro query expected → cache unchanged) and `--check` green.
10. **AC10 — tests.** Unit: source validation (every refusal of AC2 + normalisation + dedupe), item validation, the transition table (all 16 from/to pairs), reason normalisation, the pure stale computation, the staging path derivation. Integration `tests/proposals_service.rs` (`#[sqlx::test(migrations = "./migrations")]`): filing nominal (version captured, items in order, sources stored), filing refusals (no source, bad scheme, unknown / soft-deleted target — nothing written), CHECK constraint on a half-set target, listing (filters, pagination, soft-deleted excluded, item count), detail (decoded JSON, unknown id), transitions (accept / reject with and without reason / stale, final-state refusals, concurrent double-decide), staleness (version bump, soft delete, hard delete of the target row, no-target items, non-pending proposal untouched), `decided_by` set NULL when the user row is purged.

## Tasks / Subtasks

- [ ] T1 — Migration `20261028120000_create_proposals.sql` (AC1).
- [ ] T2 — Model `src/models/proposal.rs`: `ProposalKind`, `ProposalStatus` (+ `can_transition_to`), `ProposalTargetTable`, row structs, insert / list / count / find / items / target versions / guarded status update, all taking `&mut MySqlConnection` where a transaction may need them (AC1, AC4–AC7, AC9).
- [ ] T3 — Service `src/services/proposals.rs`: `ProposalError` (+ `From<ProposalError> for AppError`), pure validators (`validate_sources`, `validate_items`, `normalize_rejection_reason`, `stale_reasons`), `ProposalService::{file, list, detail, transition, transition_in, stale_targets, stale_targets_in, check_staleness, check_staleness_in}` (AC2–AC7).
- [ ] T4 — `src/services/cover_staging.rs` + `AppState::cover_staging_dir()` + creation in `main.rs` (AC8).
- [ ] T5 — Unit tests in both modules; integration tests `tests/proposals_service.rs` (AC10); add `proposals` and `proposal_items` to `tests/seed_drift.rs::ENTITY_TABLES_WITH_SOFT_DELETE` so the AR24 quartet is enforced on a fresh DB.
- [ ] T6 — Docs: schema documented in the migration header and the module docs; CLAUDE.md / manual left to story 11-24 (no user-visible surface yet).
- [ ] T7 — Gates: clippy `--all-targets -D warnings`, full `cargo test` (test DB 3307), `cargo sqlx prepare --check --workspace -- --all-targets`, flake grep, `tsc --noEmit`, full Playwright `CI=true --workers=2` on a reset stack (regression only — no E2E added, see D-11-9-6).

## Dev Notes

- **Layering (AR27 for later):** 11-12..11-14 MCP tools call `ProposalService::file` and map `ProposalError::is_invalid_input()` variants to `ToolError::InvalidArguments(e.to_string())`; this story does not touch `src/mcp/`.
- **Error copy:** `ProposalError` messages are agent-facing English (D-11-7 precedent: MCP text is not localized). `NotPending` / `NotFound` will reach librarians through 11-16..11-19; those stories map them to localized `t!()` copy. `From<ProposalError> for AppError`: validation → `BadRequest`, `NotFound` → `NotFound`, `NotPending` → `Conflict`, `Database` → the inner error.
- **Target version read** in the filing transaction: `SELECT id, version FROM {titles|series} WHERE id IN (…) AND deleted_at IS NULL` — table names come only from the `ProposalTargetTable` enum (never from input). Same grouped query (without the `deleted_at` filter, projecting `deleted_at IS NOT NULL`) for staleness, so 1000 items cost two queries, not 1000.
- **Locking:** `check_staleness` uses `SELECT … FOR UPDATE` on the proposal row inside a transaction before computing staleness, so a concurrent accept (11-17) serialises behind it.
- **Why no FK on `target_id`:** polymorphic, and the 30-day auto-purge hard-deletes titles/series; a dangling target is exactly the "deleted" stale case.
- **Not in `ALLOWED_TABLES`, not in auto-purge:** proposals are never soft-deleted by this story; a future purge of decided proposals is out of scope.
- **Staging persistence (D-11-9-4):** in the Docker image `COVERS_DIR=./covers` → `/app/covers-staging`, which is not on the `mybibli_covers` volume; story 11-14 decides whether to add a volume / document it before anything is written there.
- Format only touched files (`rustfmt --edition 2024 <file>`), never repo-wide `cargo fmt`. Heavy commands under `mem-guard`, cargo `-j 2`.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.9] — FR134, FR140, NFR47, AR29; stories 11.12–11.19 for the consumers.
- [Source: migrations/20260519141801_create_api_keys_table.sql], [Source: migrations/20260513000002_admin_audit_fk_set_null.sql] — FK conventions.
- [Source: src/services/auto_purge.rs] — purge order (titles, series, users hard-deleted).
- [Source: src/mcp/tool.rs#From<AppError> for ToolError] — why filing errors stay typed.
- [Source: _bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md] — D-11-5..D-11-8.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-10-28 — story created (create-story).
- 2026-10-28 — validated against the create-story checklist: AR24 regression net (`seed_drift`), purge interaction (`decided_by` SET NULL, no FK on targets), typed errors for the MCP layer, staging dir kept off `/covers`; no gap left open.
