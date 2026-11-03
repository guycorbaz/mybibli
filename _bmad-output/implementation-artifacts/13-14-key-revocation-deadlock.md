# Story 13.14: Concurrent key revocations no longer deadlock

Status: done

GitHub issue: #554, item 1 (type:known-failure). Covers: bug fix. Epic 13 — scheduled first (it reddens CI); ships in v1.23.0.

## Story

As an administrator,
I want revoking an API key always to succeed,
so that a revocation never answers "Server error" — and the CI suite stops failing on it.

## Evidence

CI run 41604087139 (PR #592, docs only), compose log: `request error 500 … 1213 (40001): Deadlock found when trying to get lock` on `POST /admin/api-keys/{id}/revoke` (2 ms), key 9 revoked 1 ms later by a parallel spec. Same symptom as run 40839328416 (2026-10-29), recorded in #554.

## Existing-code reality check (code map, 2026-11-03)

| Place | Today |
|---|---|
| `src/services/proposals.rs::revoke_key` (l. 1116) | one transaction: `ApiKeyModel::revoke_in` (UPDATE key row), `ProposalModel::lock_pending_of_key`, `update_status_from_pending` per id (PK), `staged_cover_files_of`; no retry |
| `src/models/proposal.rs::lock_pending_of_key` (l. 1064) | `SELECT id FROM proposals WHERE api_key_id = ? AND status = 'pending' AND deleted_at IS NULL ORDER BY id FOR UPDATE` — no index hint; with `idx_proposals_status (status, deleted_at)` available the optimizer may lock the library-wide pending range |
| `src/services/acceptance.rs` | `is_deadlock` (SQLSTATE 40001) + retried-transaction pattern (deadlock only, never lock-wait timeouts) |
| `tests/proposal_accept_metadata_cover.rs::an_acceptance_on_one_title_does_not_lock_proposals_of_another` | lock-scope regression test pattern (two transactions, `innodb_lock_wait_timeout = 2`) |
| CLAUDE.md, proposal lifecycle | "Lock scope (epic 12 retro A1): a locking read … must start from the target index … or a PK list — never from a status range" |

## Acceptance Criteria

1. **AC1 — lock scope.** `lock_pending_of_key` reads `FROM proposals FORCE INDEX (idx_proposals_api_key)`; every other locking read or write of `revoke_key` goes through PK lists (already the case — verify `staged_cover_files_of`).
2. **AC2 — retry.** `revoke_key` runs its transaction in a loop retried on deadlock (`acceptance::is_deadlock`) up to a small bound; lock-wait timeouts and other errors are not retried. The optimistic `version` check still makes a retry after a committed revocation a no-op (`Ok(None)` is not reachable for the retry of a rolled-back attempt).
3. **AC3 — regression test.** `a_revocation_does_not_lock_proposals_of_another_key` (DB integration): key A and key B each with one pending proposal; transaction 1 runs key A's revocation steps and stays open; transaction 2, with `innodb_lock_wait_timeout = 2`, runs `lock_pending_of_key` for key B and must not wait; then both commit and each key's proposal is stale.
4. **AC4 — unchanged behaviour.** Existing revocation tests (`tests/mcp_cover_proposals.rs`, `tests/admin_api_keys_delete.rs`, …) stay green; staged covers are still discarded after the commit.

## Tasks / Subtasks

- [x] T1 — `FORCE INDEX` in `lock_pending_of_key`; `.sqlx` if the query is a macro (it is `query_scalar` — dynamic, no cache)
- [x] T2 — retry loop in `revoke_key`
- [x] T3 — regression test (AC3)
- [x] T4 — gates (clippy, targeted DB tests, full suites once before the push)

### Review Findings

Code review 2026-11-03, round 1 (independent reviewer): 0 High, 1 Medium, 4 Low.

- [x] [Review][Patch] The retry — the real guarantee — was never exercised: the lock-scope test also passes without `FORCE INDEX` on a small table, and the constant test only restated the constant — Medium. Added `a_deadlocked_revocation_is_retried_and_succeeds`: a heavier blocker transaction holds the key's proposal, the revocation takes the key row and waits, the blocker asks for the key row, InnoDB rolls back the revocation, which is retried and succeeds. Verified to fail with `REVOKE_MAX_ATTEMPTS = 1`; six repeated runs green. Tautological test removed.
- [x] [Review][Patch] `innodb_lock_wait_timeout = 2` handed back to the pool — Low; reset to `DEFAULT` before the rollback.
- [x] [Review][Dismiss] `FOR UPDATE` through the key index also locks the key's decided proposals — Low; harmless for a revocation.
- [x] [Review][Dismiss] Retry loop duplicates the acceptance one (`in_retried_transaction`) — Low; the acceptance helper is typed for `AcceptOutcome`; a generic `retry_on_deadlock` is left for a third use (rule of three).
- [x] [Review][Dismiss] Immediate retry without delay — Low; the survivor keeps its locks and the retry simply waits for its commit.

Round 2: the Medium is fixed and verified; no Medium+ left.

## Completion Notes

- The lock-scope test does not reproduce the optimizer's choice of the status index (MariaDB already picks the key index on a small table); `FORCE INDEX` makes the plan explicit, and the deadlock retry is the guarantee, proven by the new test.
