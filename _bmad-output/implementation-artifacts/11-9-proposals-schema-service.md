# Story 11.9: Proposals schema and service

Status: done

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

1. **AC1 — migration (AR29, AR24).** `proposals`: `id`, `kind ENUM('series_create','series_attach','metadata','cover')`, `status ENUM('pending','accepted','rejected','stale') DEFAULT 'pending'`, `api_key_id` (NOT NULL, FK `api_keys`), `sources JSON NOT NULL` (array of URL strings), `decided_by` (NULL, FK `users` ON DELETE SET NULL), `decided_at TIMESTAMP NULL`, `rejection_reason TEXT NULL`, `decided_by_name VARCHAR(255) NULL` (username copied at decision, D-11-9-11), AR24 quartet; indexes on `(status, deleted_at)`, `api_key_id`, `deleted_at`, `(deleted_at, created_at, id)` (listing). `proposal_items`: `id`, `proposal_id` (FK `proposals`), `target_table ENUM('titles','series') NULL`, `target_id BIGINT UNSIGNED NULL`, `target_version INT NULL`, `proposed_values JSON NOT NULL`, AR24 quartet; a CHECK constraint makes the three target columns all NULL (a line that creates something new: the series of a `series_create`, an expected tome) or all set; indexes on `proposal_id`, `(target_table, target_id)`, `deleted_at`. InnoDB, utf8mb4.
2. **AC2 — sources (FR134, NFR47).** Filing refuses, with an explicit message naming the offending source by its 1-based index: no source; more than 20; an empty one; one longer than 2048 characters; one that does not parse as an absolute URL; a scheme other than `http`/`https` (e.g. `javascript:`, `data:`, `ftp:`, `file:`); no host (reported by the parser as "not an absolute URL with a host name"); embedded credentials (`user:pass@`). Accepted sources are trimmed, stored in their parsed (normalised) form, duplicates dropped keeping first-seen order.
3. **AC3 — items.** Filing refuses: no item; more than 1000 items; `proposed_values` that is not a JSON object; an item's serialized values over 64 KiB; a target that does not exist or is soft-deleted (message names the item index, table and id). The service reads each target's current `version` itself at filing time (the caller never supplies it) and stores it in `target_version`. Proposal + items are written in one transaction; the proposal starts `pending`, `version = 1`, attributed to the filing key (FR134).
4. **AC4 — listing.** `list(filter { status?, kind?, api_key_id? }, page)` returns a `PaginatedList` (25 per page) of summaries `{ id, kind, status, api_key_id, api_key_label, item_count (live items), created_at, decided_at }` newest first (`created_at DESC, id DESC`); soft-deleted proposals and items never appear or count. The key label is shown even for a revoked key.
5. **AC5 — detail.** `detail(id)` returns the proposal (all columns, `sources` decoded, `decided_by` + the decider's username — the live one, or the copy taken at decision time once the user row is purged, D-11-9-11) and its live items in filing order (`id ASC`), `proposed_values` decoded; `None` for an unknown or soft-deleted id.
6. **AC6 — transitions.** Only `pending → accepted | rejected | stale`. Accept and reject require the deciding user id and set `decided_by`, `decided_at = NOW()`; reject takes an optional reason (trimmed, empty → NULL, at most 1000 characters, refused otherwise); stale sets `decided_at = NOW()` and leaves `decided_by` NULL (the system decided). Every transition bumps `version`. Any transition out of `accepted`, `rejected` or `stale` — and `pending → pending` — is refused (`NotPending`, carrying the current status); unknown/soft-deleted id → `NotFound`. The status guard is part of the `UPDATE … WHERE status = 'pending'`, so two concurrent deciders cannot both win. A transaction-scoped variant exists for story 11-17 (apply + accept in one transaction). **Refined in review (D-11-9-9..11):** accept and reject require an active user (`UnknownDecider`); accept refuses a proposal with no live item (`NothingToApply`) and re-checks staleness with locking reads of the proposal, its items and its targets — moved targets mark it `stale` (`transition_in` returns `Transition::WentStale`, the pool wrapper `ProposalError::Stale`); filing requires a live `propose` key (`KeyNotAllowed`) and may pin the agent's read with `expected_version` (`TargetChanged`).
7. **AC7 — staleness (FR140).** `stale_targets(id)` lists every live item whose target row is missing, soft-deleted, or whose `version` differs from `target_version` (`{ item_id, table, target_id, reason: deleted | changed }`); items without a target are never stale. `check_staleness(id)` locks the proposal row, and when it is `pending` with at least one stale target, marks it `stale` (AC6 rules); it returns the resulting status and the stale targets. On a non-pending proposal it changes nothing and returns its current status. A transaction-scoped variant exists for story 11-17.
8. **AC8 — cover staging directory.** `services::cover_staging::staging_dir_for(covers_dir)` = sibling directory `<covers dir name>-staging` (`./covers` → `./covers-staging`, `/app/covers` → `/app/covers-staging`; a path without a final name such as `.` is resolved on disk first; `None` for `/` — never inside the served directory, review round 1); `AppState::cover_staging_dir()` returns it (`None` when the directory does not exist); `main.rs` creates it at boot next to the covers directory and logs it; a creation failure is logged as a warning and does not stop the boot (nothing uses it yet). It is not served by any route and nothing writes to it yet (11-14).
9. **AC9 — JSON & sqlx.** JSON columns read with `CAST(… AS CHAR)`, TIMESTAMP columns with `CAST(… AS DATETIME)` in dynamic queries; `deleted_at IS NULL` on every read of `proposals`, `proposal_items`, `titles`, `series`; `cargo sqlx prepare` run (no macro query expected → cache unchanged) and `--check` green.
10. **AC10 — tests.** Unit: source validation (every refusal of AC2 + normalisation + dedupe), item validation, the transition table (all 16 from/to pairs), reason normalisation, the pure stale computation, the staging path derivation. Integration `tests/proposals_service.rs` (`#[sqlx::test(migrations = "./migrations")]`): filing nominal (version captured, items in order, sources stored), filing refusals (no source, bad scheme, unknown / soft-deleted target — nothing written), CHECK constraint on a half-set target, listing (filters, pagination, soft-deleted excluded, item count), detail (decoded JSON, unknown id), transitions (accept / reject with and without reason / stale, final-state refusals, concurrent double-decide), staleness (version bump, soft delete, hard delete of the target row, no-target items, non-pending proposal untouched), `decided_by` set NULL when the user row is purged.

## Tasks / Subtasks

- [x] T1 — Migration `20261028120000_create_proposals.sql` (AC1).
- [x] T2 — Model `src/models/proposal.rs`: `ProposalKind`, `ProposalStatus` (+ `can_transition_to`), `ProposalTargetTable`, row structs, insert / list / count / find / items / target versions / guarded status update, all taking `&mut MySqlConnection` where a transaction may need them (AC1, AC4–AC7, AC9).
- [x] T3 — Service `src/services/proposals.rs`: `ProposalError` (+ `From<ProposalError> for AppError`), pure validators (`validate_sources`, `validate_items`, `normalize_rejection_reason`, `stale_reasons`), `ProposalService::{file, list, detail, transition, transition_in, stale_targets, stale_targets_in, check_staleness, check_staleness_in}` (AC2–AC7).
- [x] T4 — `src/services/cover_staging.rs` + `AppState::cover_staging_dir()` + creation in `main.rs` (AC8).
- [x] T5 — Unit tests in both modules; integration tests `tests/proposals_service.rs` (AC10); add `proposals` and `proposal_items` to `tests/seed_drift.rs::ENTITY_TABLES_WITH_SOFT_DELETE` so the AR24 quartet is enforced on a fresh DB.
- [x] T6 — Docs: schema documented in the migration header and the module docs; CLAUDE.md / manual left to story 11-24 (no user-visible surface yet).
- [x] T7 — Gates: clippy `--all-targets -D warnings`, full `cargo test` (test DB 3307), `cargo sqlx prepare --check --workspace -- --all-targets`, flake grep, `tsc --noEmit`, full Playwright `CI=true --workers=2` on a reset stack (regression only — no E2E added, see D-11-9-6).

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

### Review Findings

Code review 2026-10-28 (bmad-code-review: Blind Hunter, Edge Case Hunter, Acceptance Auditor). Round 1 → fixes (D-11-9-8..10); round 2 → fixes (D-11-9-11); round 3 → see Senior Developer Review.

Round 1:
- [x] [Review][Patch] (Medium) Accept did not re-check staleness; the check read targets from the snapshot without locking them — accept now locks the proposal and its targets (`FOR UPDATE`) and marks it stale when a target moved [src/services/proposals.rs]
- [x] [Review][Patch] (Medium) The "already X" fallback was a non-locking read (could report `pending`) — now a locking read [src/services/proposals.rs]
- [x] [Review][Patch] (Medium) JSON nested deeper than MariaDB accepts surfaced as a server error — depth ≤ 20 refused as `ValuesTooDeep` [src/services/proposals.rs]
- [x] [Review][Patch] (Medium) Filing key not checked (unknown → 500, revoked/read-scope accepted) — share-locked live `propose` key required (`KeyNotAllowed`) [src/services/proposals.rs, src/models/proposal.rs]
- [x] [Review][Patch] (Medium) No total size bound per proposal — 1 MiB across items [src/services/proposals.rs]
- [x] [Review][Patch] (Low) Stored (normalised) source could exceed 2048 chars; `NoHost` dead branch; database text in `Display`; `is_invalid_input` by exclusion; decider not checked; detail on two connections; concurrency test not proving contention; no listing index; nullable unsigned ids read without `CAST(… AS SIGNED)`; `.`/`/` staging fallback inside the served dir; undocumented joins without `deleted_at` [several]
- [x] [Review][Defer] (Medium) Staleness misses writes that do not bump `version` (background cover fetch, `title_series` attach) → #521 (D-11-9-8)
- [x] [Review][Defer] (Medium) Staging directory outside the Docker covers volume; promotion by `rename` would hit EXDEV → #522 (for 11-14)
- Dismissed (round 1): purge FK conflicts (api_keys never hard-deleted; proposals not purged), internal-address sources (the librarian decides), items vs kind shapes (11-12..11-14), filing reads without lock (fails safe → stale), `MarkStale` public (revocation in 11-14), `check_staleness_in` outside a transaction (documented).

Round 2:
- [x] [Review][Patch] (Medium) A stale accept was an `Err`: `?` in a caller's transaction would roll back the stale mark — `transition_in` now returns `Transition::WentStale` (an `Ok`) [src/services/proposals.rs]
- [x] [Review][Patch] (Medium) Staleness baseline absorbed edits between the agent's read and its filing — optional `expected_version` per item, `TargetChanged` [src/services/proposals.rs]
- [x] [Review][Patch] (Medium) No test for accepting fresh targets, the target lock, or the current-read-vs-snapshot fix — three integration tests [tests/proposals_service.rs]
- [x] [Review][Patch] (Medium) Lock order for a future revocation could deadlock with filing — documented on `lock_filing_key` for 11-14 [src/models/proposal.rs]
- [x] [Review][Patch] (Low) Decider name lost after the user purge (`decided_by_name`); empty proposal acceptable (`NothingToApply`); active-user predicate; `UnknownDecider` classification; count/page snapshot; rollback error masking; total-size early exit; `cover_staging_dir()` on a missing directory; NotAUrl wording; AC8 text [several]
- Dismissed (round 2): api_keys purge FK (verified: `api_keys` is not in the auto-purge order), untargeted items never stale (a duplicate series is caught at acceptance, 11-17), gap lock on a purged top id (short transaction), per-item INSERT round trips (LAN, ≤ 1000), PROCESSLIST matching a running statement (the other transaction holds the lock, so running = waiting).

Round 3:
- [x] [Review][Patch] (Medium) Accept / staleness read the items and the decider from the caller's snapshot while the targets were locked — items (`LOCK IN SHARE MODE` when deciding) and the active-user check are now locking reads; regression test `accept_sees_items_and_deciders_changed_after_its_snapshot` [src/models/proposal.rs, src/services/proposals.rs, tests/proposals_service.rs]
- [x] [Review][Patch] (Low) AC1 / AC5 / AC6 text and Dev Agent Record counts brought in line with D-11-9-9..11 [this file]
- Dismissed (round 3): `fk_proposals_api_key` vs the purge (verified: `api_keys` is never hard-deleted — not in `auto_purge` order nor `ALLOWED_TABLES`; `api_keys.created_by` is `ON DELETE SET NULL`).

Round 4 (confirmation pass, all three lenses): no Medium+. Lows: accept's locking item read briefly blocks a concurrent filing when the accepted proposal is the newest (note for 11-17, D-11-9-12); early returns in `file` / `detail` roll back by drop (correct, style only) — both dismissed.

### Senior Developer Review (AI)

- **Outcome:** approved after three fix rounds; round 4 (confirmation pass) found no Medium+ issue. Deferred: #521, #522 (both Medium, `type:code-review-finding`).
- **Gates (2026-10-28, final code):** `cargo clippy --all-targets -- -D warnings` clean; `cargo test` with the test DB on 3307: 1786 passed / 0 failed across 66 binaries (`proposals_service` 26, lib unit tests for the story 30); `cargo sqlx prepare --check --workspace -- --all-targets` clean on a freshly migrated test DB; flake gate clean; `tsc --noEmit` clean; Playwright on a freshly reset stack, `CI=true --workers=2`: 362 passed, 2 skipped, 0 failed.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

- T1: `20261028120000_create_proposals.sql` — `proposals` / `proposal_items`, AR24 quartet, `decided_by` SET NULL, no FK on targets, `chk_proposal_items_target`.
- T2: `models/proposal.rs` — enums (`ProposalKind`, `ProposalStatus::can_transition_to`, `ProposalTargetTable`), grouped target-state read (one query per table), guarded `update_status_from_pending`, list/count, row lock; connection-scoped functions for 11-17.
- T3: `services/proposals.rs` — `ProposalError` (agent-facing, `is_invalid_input`, `From` into `AppError`), `validate_sources` / `validate_items` / `normalize_rejection_reason` / `stale_reasons` (pure), `ProposalService::{file, list, detail, transition(_in), stale_targets(_in), check_staleness(_in)}`.
- T4: `services/cover_staging.rs::staging_dir_for`, `AppState::cover_staging_dir()`, fail-open creation in `main.rs` (E2E stack log: `/tmp/test-covers-staging`).
- T5: 30 unit tests (models 5, services 20, staging 5); `tests/proposals_service.rs` 26 integration tests (17 at dev-story, +9 from the review rounds); `seed_drift` extended.
- T6: schema documented in the migration header and module docs.
- T7 gates (2026-10-28): `cargo clippy --all-targets -- -D warnings` clean; `cargo test` (test DB 3307) 1772 passed / 0 failed across 66 binaries; `cargo sqlx prepare --check --workspace -- --all-targets` clean (no macro query added); flake gate clean; `tsc --noEmit` clean; Playwright on a freshly reset stack, `CI=true --workers=2`: 362 passed, 2 skipped, 0 failed.

### File List

- `migrations/20261028120000_create_proposals.sql` (new)
- `src/models/proposal.rs` (new)
- `src/models/mod.rs`
- `src/services/proposals.rs` (new)
- `src/services/cover_staging.rs` (new)
- `src/services/mod.rs`
- `src/lib.rs`
- `src/main.rs`
- `tests/proposals_service.rs` (new)
- `tests/seed_drift.rs`
- `_bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

## Change Log

- 2026-10-28 — story created (create-story).
- 2026-10-28 — validated against the create-story checklist: AR24 regression net (`seed_drift`), purge interaction (`decided_by` SET NULL, no FK on targets), typed errors for the MCP layer, staging dir kept off `/covers`; no gap left open.
- 2026-10-28 — dev-story complete (T1–T7), gates green → review.
- 2026-10-28 — code review round 1: 5 Medium + 11 Low fixed, 2 Medium deferred (#521, #522).
- 2026-10-28 — code review round 2: 4 Medium + 10 Low fixed.
- 2026-10-28 — code review round 3: 1 Medium + 1 Low fixed; round 4 clean → done.
