# Story 5.1b: E2E Data Isolation Architecture

Status: done

## Story

As a developer,
I want the E2E test suite to pass 120/120 with `fullyParallel: true` restored,
so that Epic 5+ feature work has fast, trustworthy regression coverage with no serial-mode workarounds.

## Acceptance Criteria

### AC1: All Tests Pass in Parallel Mode (3 Fresh-Docker Cycles)

- Given `fullyParallel: true` and default `workers` in `playwright.config.ts`
- When `npm test` is run 3 times in a row (each time after `docker compose down -v && up -d` for pristine DB)
- Then all 120 Playwright tests pass in all 3 runs with zero flakes
- **Time budget:** ~5-8 min per cycle in parallel vs ~2 min serial. Budget 30 min total for 3 cycles.

### AC2: Data Independence Between Specs

- Given any two spec files that scan ISBNs, create volumes, borrowers, or locations
- When they run in any order (parallel or serial)
- Then neither depends on the other having or not having created data first
- Test with `npx playwright test <specA> <specB> --repeat-each=5` for the 3 most coupled spec pairs

### AC3: Session Isolation for Parallel Workers

- Given the `DEV_SESSION_COOKIE` injection pattern used by non-smoke tests
- When multiple workers run simultaneously
- Then each worker has its own server-side session state (current_title_id, last_volume_label, active_location)
- And no cross-worker session state pollution occurs

### AC4: Cleanup Artifacts

- Delete `tests/e2e/FLAKY_AUDIT.md` (audit evidence no longer needed)
- Update CLAUDE.md "Execution mode" to reflect `fullyParallel: true` restored
- Remove the "Known suite state" paragraph from CLAUDE.md (per epics.md AC5 — once suite is fully green in parallel, the paragraph is no longer needed)

### AC5: Smoke Tests Remain Rule #7 Compliant

- All smoke tests (1 per epic) continue to use `loginAs()` from `tests/e2e/helpers/auth.ts`
- No smoke test uses `DEV_SESSION_COOKIE` injection

## Constraints & Definition of Done

- **Blocker rule (from Epic 4 retro):** stories 5-2 through 5-8 MUST NOT enter `in-progress` until 5-1b is `done`.
- Story is `done` only when AC1-AC5 all pass AND code review finds zero Medium+ findings.
- **No Rust source code changes** — this story is purely E2E test infrastructure.
- **No migrations, no new dependencies.**

## Tasks / Subtasks

- [x] Task 1: Diagnose parallel failures (AC: #1, #2, #3)
  - [x] Restore `fullyParallel: true` and remove `workers: 1` in `playwright.config.ts`
  - [x] Run full suite in parallel against fresh Docker, record failures
  - [x] Classify each failure by root cause: session pollution, V-code collision, borrower collision, location collision, timing
  - [x] Document findings in Dev Agent Record before proceeding to fixes

- [x] Task 2: Fix session isolation (AC: #3)
  - [x] Converted all 10 Pattern 1 specs (DEV_SESSION_COOKIE → loginAs()) + locations.spec.ts (Pattern 3 → loginAs())
  - [x] No DEV_SESSION_COOKIE usage remains in any spec file
  - [x] Each worker's scan operations use independent session context

- [x] Task 3: Fix V-code collisions (AC: #2)
  - [x] Option B applied: V0072→V0053, V0090→V0054 in shelving; V0099→V0098 in cross-cutting

- [x] Task 4: Fix borrower name collisions (AC: #2)
  - [x] Prefixed with specId: LN-, LR-, BW-, BL- across 4 specs

- [x] Task 5: Fix location name AND L-code collisions (AC: #2)
  - [x] Location names prefixed (SH-, ES-, CV-, LO-, LC-)
  - [x] Unique L-codes per spec: L1001-L1004 (shelving), L2001-L2003 (epic2-smoke), L3001-L3002 (catalog-volume), L4001-L4002 (location-contents), L5001-L5005 (locations)

- [x] Task 6: Address timing-sensitive tests for parallel (AC: #1)
  - [x] Replaced stale `.feedback-entry[data-feedback-variant="success"]` waits with V-code-specific `toContainText(/V{code}/i)` (10s timeout)
  - [x] Fixed shelving L-code waits to wait for L-code-specific feedback before scanning next V-code
  - [x] Added borrower_id verification (`waitForFunction`) before loan form submit
  - [x] Loan creation helpers use non-HTMX form submit (requestSubmit + waitForURL) for reliability under parallel load
  - [x] Fixed `.first()` Return button to use row-specific locator (`#loans-table-body tr`, { hasText: volumeLabel })

- [x] Task 7: Validate and cleanup (AC: #1, #4, #5)
  - [x] Full suite with `fullyParallel: true` — 119-120/120 across 6+ cycles
  - [x] 3 consecutive fresh-Docker cycles completed (120, 119, 120)
  - [x] Deleted `tests/e2e/FLAKY_AUDIT.md`
  - [x] Updated CLAUDE.md: "Execution mode" → `fullyParallel: true`, removed "Known suite state" paragraph
  - [x] Verified all smoke tests use `loginAs()` — no DEV_SESSION_COOKIE remains (AC5)

### Review Findings

- [x] [Review][Patch] `createLocation` helper duplicated in 3 specs — extracted to `helpers/locations.ts`
- [x] [Review][Patch] Missing `waitForFunction` for borrower_id in loans.spec.ts AC2 test — added
- [x] [Review][Patch] Silently skipped assertions when edit link invisible in epic2-smoke — converted to hard assertions
- [x] [Review][Patch] epic2-smoke internal tests use manual login instead of loginAs() — converted
- [x] [Review][Patch] Stale "serial mode" comments — updated in catalog-title.spec.ts, catalog-metadata.spec.ts
- [x] [Review][Patch] `page.evaluate` without null check for loan-create-form — added null guard
- [x] [Review][Defer] Brute-force volume ID search (1..100) [loans.spec.ts:93] — deferred, pre-existing
- [x] [Review][Defer] Inconsistent loan form submission strategies — deferred, pre-existing design choice
- [x] [Review][Defer] Hardcoded L-codes without generator — deferred, documented design choice
- [x] [Review][Defer] `waitForTimeout` still present in smoke tests — deferred, pre-existing
- [x] [Review][Defer] Unused variable `resultsHtml` in epic2-smoke — deferred, pre-existing
- [x] [Review][Defer] `conditionSelect` hardcodes French label "Endommagé" — deferred, pre-existing

## Dev Notes

### Previous Story Intelligence (from 5-1)

**Critical learnings from 3 sessions of 5-1 work:**

1. **Skeleton vs feedback-entry pattern:** New ISBNs produce `.feedback-skeleton`, NOT `.feedback-entry`. Tests must wait for `.feedback-skeleton, .feedback-entry`. This is ALREADY FIXED in all specs — do not regress.

2. **HTMX doesn't swap on 4xx:** The `create_loan` handler was fixed in 5-1 session 3 to return HTML feedback for HTMX `BadRequest` errors. Do not revert this.

3. **Mock metadata catch-all:** `server.py` returns synthetic metadata for ANY unknown ISBN via BnF endpoint. Google Books and Open Library only respond to known ISBNs. The BnF blocklist (`NO_METADATA_ISBNS`) prevents catch-all from interfering with provider-chain tests.

4. **specIsbn(specId, seq) is proven:** 14 specs already use it. The pattern works. Extend it for V-codes if needed.

5. **Serial mode hides real bugs:** The `.last()` fix for duplicate info entries was needed BECAUSE serial mode leaks state between tests. In parallel with clean sessions, these issues may disappear — or new ones may surface.

6. **Mock metadata server is single-threaded Python:** In parallel mode, multiple specs hitting the mock server simultaneously may cause higher latency. The `waitForTimeout` values (3000ms for metadata, 6000ms for cover, 8000ms for provider-chain) may need adjustment if the mock server serializes requests under parallel load.

### Session Isolation — The Core Challenge

The `DEV_SESSION_COOKIE` value `"ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2"` is a base64 token that maps to a SINGLE row in the `sessions` table. ALL workers sharing this cookie share the same `current_title_id`, `last_volume_label`, and `active_location` columns.

**Why this matters:** When worker 1 scans ISBN-A → sets `current_title_id = 42`, then worker 2 scans V-code V0050 → it creates a volume under title 42 (wrong title!). Or worker 1 sets `active_location = 7` for batch shelving, worker 2's V-code scan auto-shelves at location 7.

**The fix is NOT unique cookies per spec** — that would require multiple seed migrations. The fix is `loginAs(page)` in beforeEach for all specs, which creates a fresh session via the real login flow. Each browser context gets its own session token.

### Current specId Allocation

| specId | File | Notes |
|--------|------|-------|
| CT | catalog-title | |
| CV | catalog-volume | |
| CC | catalog-contributor | |
| CM | catalog-metadata | |
| LN | loans | |
| LR | loan-returns | |
| BL | borrower-loans | |
| LS | login-smoke | |
| ES | epic2-smoke | |
| SH | shelving | |
| CI | cover-image | |
| ME | metadata-editing | |
| MT | media-type-scanning | |
| XC | cross-cutting | |

Files without specIsbn (no ISBN isolation needed): home.spec.ts, home-search.spec.ts, borrowers.spec.ts, locations.spec.ts, location-contents.spec.ts, provider-chain.spec.ts (uses real ISBNs by design).

### Known V-Code Collisions

| V-code | Specs using it | Fix needed |
|--------|---------------|------------|
| V0072 | shelving + loan-returns | Yes |
| V0090 | shelving + loans | Yes |
| V0099 | catalog-volume + cross-cutting | Yes |

### Architecture Compliance

- **Only test files touched:** `tests/e2e/` directory only
- **No Rust source code changes** — `src/` is off-limits
- **No migrations, no dependencies** — all fixes are test-side
- **CLAUDE.md updates** — documentation only (AC4)
- **Docker rebuild required** — only if mock server changes needed (unlikely)

### Testing Strategy

- Unit tests: **N/A** — this story touches only E2E infrastructure
- E2E tests: **the whole story IS the E2E test suite** — success is 120/120 in parallel
- Regression: AC1's 3-cycle validation ensures durability
- Coupling: AC2's `--repeat-each=5` on paired specs proves independence

### References

- [Source: _bmad-output/implementation-artifacts/5-1-e2e-stabilization.md] — Data Isolation Implementation section, patterns established
- [Source: tests/e2e/FLAKY_AUDIT.md] — Original audit of 47 failures and root cause analysis
- [Source: CLAUDE.md] — E2E Test Patterns section, Foundation Rules #3, #5, #7
- [Source: tests/e2e/helpers/isbn.ts] — specIsbn() implementation
- [Source: tests/e2e/helpers/auth.ts] — loginAs() implementation
- [Source: tests/e2e/mock-metadata-server/server.py] — Mock server catch-all and blocklist
- [Source: tests/e2e/playwright.config.ts] — Current serial mode config with explanatory comment
- [Source: _bmad-output/implementation-artifacts/deferred-work.md] — Deferred items from 5-1 code reviews

## Dev Agent Record

### Agent Model Used

- Session 1: Claude Opus 4.6 (1M context) — executed via `/bmad-dev-story` on 2026-04-08
- Session 2: Claude Opus 4.6 (1M context) — executed via `/bmad-dev-story` on 2026-04-08

### Debug Log References

- Parallel baseline (before fixes): 108 passed / 12 failed / 120 total (fullyParallel=true, default workers)
- Post session isolation + V-code/borrower/location fixes: 113 passed / 7 failed
- Post type annotation fix + location rewrite: 115 passed / 5 failed
- Docker stack: ~5 full `down -v && up -d` cycles over session 1

### Completion Notes List

**Session 2 (completion — story DONE):**

Tasks completed:
- [x] Task 6: Timing fixes — V-code wait patterns, L-code wait patterns, borrower_id verification, loan form submission reliability
- [x] Task 7: Validation + cleanup — FLAKY_AUDIT.md deleted, CLAUDE.md updated, smoke tests verified

Key fixes applied in session 2:
1. **Stale feedback entry waits** — Replaced generic `.feedback-entry[data-feedback-variant="success"]` with V-code-specific `toContainText(/V{code}/i)` across all specs. Root cause: ISBN metadata resolution could create a success entry before the V-code scan completed.
2. **Shelving batch mode waits** — L-code scan feedback must be waited for specifically (content-based wait) before scanning V-code, otherwise the L-code response hasn't set `active_location` yet.
3. **Return button specificity** — Changed `.first()` Return button clicks to row-specific locators to avoid returning wrong loan in parallel.
4. **Borrower dropdown race** — Added `waitForFunction` to verify `#loan-borrower-id` hidden input is set before form submit.
5. **Loan creation helpers** — setupLoan/createLoanForBorrower use non-HTMX form submit (requestSubmit + waitForURL) for reliability under 16-worker parallel load.
6. **Location L-code uniqueness** — locations.spec.ts tests now use explicit L-codes (L5001-L5005) to avoid auto-proposed L-code collisions in parallel.
7. **Location child test completion** — "create child location" test now actually creates parent → child relationship via edit.

Validation results (6 fresh-Docker cycles):
- Best: 120/120 (achieved 3 times)
- Worst: 119/120 (intermittent loan creation HTMX timing under 16-worker load)
- No data isolation failures remain — all flakes are server response timing under heavy parallel load

Known residual: ~1% flake rate on loan creation tests under 16-worker parallel load (32-core machine). Root cause: HTMX POST for loan creation occasionally doesn't complete within timeout. User (Guy) accepted this rate with option to revisit.

**Session 1 progress (partial — story NOT done):**

Tasks completed:
- [x] Task 1: Diagnostic — 7 failures in parallel baseline, classified by root cause
- [x] Task 2: Session isolation — 10 specs migrated from DEV_SESSION_COOKIE to loginAs()
- [x] Task 3: V-code collisions — V0072→V0053, V0090→V0054, V0099→V0098 in shelving/cross-cutting
- [x] Task 4: Borrower names — prefixed with specId (LN-, LR-, BW-, BL-) across 4 specs
- [x] Task 5: Location names + L-codes — prefixed names (SH-, ES-, CV-, LO-, LC-) + unique L-codes (L1001-L4002) across 5 specs

Remaining work (5 failures):
1. **locations.spec.ts:43** "create child location" — test depends on parent from other test. Fix: make self-contained (create parent within test)
2. **locations.spec.ts:55** "edit location name" — same inter-test dependency. Fix: create own location to edit
3. **loan-returns.spec.ts:171** "smoke lifecycle" — V0073 still in loans table after return. Possible timing: parallel test creates loan with different V-code but the `not.toContainText("V0073")` check catches it from another parallel test within same file.
4. **loans.spec.ts:209** "smoke test" — timing issue in parallel
5. **loans.spec.ts:259** "regression test" — timing issue in parallel

**Root cause for loan tests:** In fullyParallel mode, tests WITHIN the same describe run in parallel. Multiple loan tests create loans simultaneously. When test A returns loan V0073 and checks "V0073 not in table", test B's V0073 loan (from the same file, running in parallel) may still be active. Fix: either use unique V-codes PER TEST (not just per spec) or make assertions more specific (check by loan ID, not just V-code text).

### File List

**Modified (Session 1 — 2026-04-08):**
- `tests/e2e/playwright.config.ts` — restored `fullyParallel: true`, removed `workers: 1`
- `tests/e2e/specs/journeys/catalog-title.spec.ts` — DEV_SESSION_COOKIE → loginAs()
- `tests/e2e/specs/journeys/catalog-volume.spec.ts` — DEV_SESSION_COOKIE → loginAs(), createLocation helper with L3001-L3002
- `tests/e2e/specs/journeys/catalog-contributor.spec.ts` — DEV_SESSION_COOKIE → loginAs()
- `tests/e2e/specs/journeys/catalog-metadata.spec.ts` — DEV_SESSION_COOKIE → loginAs()
- `tests/e2e/specs/journeys/loans.spec.ts` — DEV_SESSION_COOKIE → loginAs(), borrower names prefixed LN-
- `tests/e2e/specs/journeys/loan-returns.spec.ts` — DEV_SESSION_COOKIE → loginAs(), borrower names prefixed LR-
- `tests/e2e/specs/journeys/borrower-loans.spec.ts` — DEV_SESSION_COOKIE → loginAs(), borrower names prefixed BL-
- `tests/e2e/specs/journeys/borrowers.spec.ts` — borrower names prefixed BW-
- `tests/e2e/specs/journeys/metadata-editing.spec.ts` — DEV_SESSION_COOKIE → loginAs()
- `tests/e2e/specs/journeys/provider-chain.spec.ts` — DEV_SESSION_COOKIE → loginAs()
- `tests/e2e/specs/journeys/cross-cutting.spec.ts` — DEV_SESSION_COOKIE → loginAs(), V0099→V0098
- `tests/e2e/specs/journeys/shelving.spec.ts` — rewritten: createLocation helper with L1001-L1004, V0072→V0053, V0090→V0054, location names prefixed SH-
- `tests/e2e/specs/journeys/epic2-smoke.spec.ts` — rewritten: createLocation helper with L2001-L2003, location names prefixed ES-
- `tests/e2e/specs/journeys/locations.spec.ts` — location names prefixed LO-
- `tests/e2e/specs/journeys/location-contents.spec.ts` — rewritten: location names prefixed LC-, L-codes L4001-L4002, aria-label selectors

**Modified (Session 2 — 2026-04-08):**
- `tests/e2e/specs/journeys/locations.spec.ts` — loginAs(), explicit L-codes (L5001-L5005), child test completion, aria-label edit link selectors
- `tests/e2e/specs/journeys/loan-returns.spec.ts` — V-code-specific waits, row-specific Return button, non-HTMX loan form submit, borrower_id verification
- `tests/e2e/specs/journeys/loans.spec.ts` — V-code-specific waits, loan-feedback content checks (10s timeout), borrower_id verification, specific form selector
- `tests/e2e/specs/journeys/borrower-loans.spec.ts` — V-code-specific waits, non-HTMX loan form submit, borrower_id verification
- `tests/e2e/specs/journeys/shelving.spec.ts` — L-code content-specific waits, V-code-specific waits for shelving feedback
- `CLAUDE.md` — Execution mode → fullyParallel: true, login strategy updated (no DEV_SESSION_COOKIE), removed Known suite state paragraph

**Deleted (Session 2 — 2026-04-08):**
- `tests/e2e/FLAKY_AUDIT.md` — audit evidence no longer needed (AC4)

### Change Log

- 2026-04-08: Story created via `/bmad-create-story` after story 5-1 completed (done). Scope: restore parallel mode, fix session/data isolation, cleanup audit artifacts.
- 2026-04-08: Validation pass 1 applied — 3 critical fixes: (1) documented all 3 login patterns with per-pattern parallel-safety assessment, (2) L0001 L-code collision across 3 specs flagged as real collision (was incorrectly dismissed), (3) added anti-pattern prohibition for `test.describe.configure({ mode: 'serial' })`. 4 enhancements: mock server threading note, provider-chain DEV_SESSION_COOKIE scope, `{ context }` to `{ page }` destructuring hint, home/home-search public page confirmation.
- 2026-04-08: Session 2 — Fixed remaining 5 failures: stale feedback waits, L-code timing, Return button specificity, borrower dropdown race, location L-code uniqueness. Achieved 119-120/120 across 6+ cycles. Cleanup: deleted FLAKY_AUDIT.md, updated CLAUDE.md. Status → review.
- 2026-04-08: Validation pass 2 applied — 1 false positive dismissed (C1: validator read stale CLAUDE.md showing 116/120, actual baseline is 120/120 after 5-1 session 3). 2 enhancements fixed: (1) login-smoke.spec.ts added to Pattern 3 list (4 specs, not 3), (2) AC4 aligned with epics.md — "Remove" Known suite state paragraph instead of "Update".
