# Story 5.1c: Fix Epic 4 loan/borrower spec parallel-mode flakes

Status: done

## Story

As a developer working on mybibli,
I want the full E2E suite to pass 131/131 consistently in parallel mode,
so that Foundation Rule #5 is honoured and future stories can trust the gate signal instead of re-running the suite to chase phantom regressions.

## Context

Discovered during story 5-7 code review (2026-04-10). The full E2E suite runs 130/131 or 131/131 depending on the run, with a **different** Epic 4 loan/borrower spec failing each time. Three consecutive clean `down -v` + full runs produced these failures:

- **Run 1:** `loan-returns.spec.ts:117` — "overdue loan shows red styling and badge" (`V0074` not found in `body`)
- **Run 2:** `loan-returns.spec.ts:153` (scan V-code → return from scan result) + `loans.spec.ts:266` (active loan TIMESTAMP fix regression)
- **Run 3:** `borrower-loans.spec.ts:95` (return loan from borrower detail → loan disappears)

Each failing spec passes cleanly when re-run in isolation (`npx playwright test specs/journeys/loan-returns.spec.ts` → 6/6). The flakes are intermittent and random, affecting only loan/borrower specs — unit tests, integration tests, and non-loan E2E specs are all stable.

Story 5-7's Debug Log References noted: "Loan-spec flakes on repeated runs: loan-returns.spec.ts:117 and loans.spec.ts:140 fail intermittently... A single clean down -v + full run is green." But the 2026-04-10 observations show the flakes persist **even with a fresh `down -v` + full run**, contradicting that claim. The underlying race is real.

## Why this matters

Foundation Rule #5 — "No milestone transition until ALL tests (unit + E2E) are green" — is violated while these flakes exist. Every story that completes after this point either pays the cost of re-running the suite to get a green pass (wasted CI/dev time) or risks missing genuine regressions hidden in the noise floor. Guy's policy is zero test debt.

## Acceptance Criteria

1. **Zero flakes over 5 consecutive full-suite runs.** Each run must pass 131+/131+ on a fresh `down -v` + up stack, with `fullyParallel: true` and default worker count. No retries, no conditional skips.
2. **Root cause documented** in this story file's Dev Notes section after investigation — not a `waitForTimeout` workaround, not a global retry policy. An actual fix to the underlying race or state-isolation issue.
3. **Affected specs covered:** at minimum `loan-returns.spec.ts`, `loans.spec.ts`, `borrower-loans.spec.ts`. If Task 1 investigation surfaces other Epic 4 specs (`borrower-crud.spec.ts`, etc.) with the same root cause, include them in the fix.
4. **Regression guard:** the fix must not introduce `test.retries`, `test.describe.serial`, artificial delays via `waitForTimeout`, or any other mechanism that hides the underlying cause. The suite runtime must not increase by more than 10% (current baseline: ~16s).
5. **CLAUDE.md updated** with the lesson learned under the "E2E Test Patterns" → "Known app quirks" section (same format as the existing duplicate `#session-counter` note).
6. **No unit or integration regression:** `cargo clippy -- -D warnings`, `cargo test`, `cargo test --test find_similar`, and `cargo sqlx prepare --check --workspace -- --all-targets` must all remain green.

## Tasks / Subtasks

- [x] **Task 1 — Reproduce deterministically** (AC #1, #2)
  - [x] 1.1 Ran the full suite multiple times on fresh stacks (15 runs total across investigation + validation). Failures were always in loan/borrower specs or loan deadlock-adjacent paths. No failures in workers=1 mode — confirms the flakes are load/concurrency dependent.
  - [x] 1.2 Parallel mode with default workers reproduces the flake; sequential (workers=1) does not. Confirmed root cause is concurrency-related.
  - [x] 1.3 Running only the 3 suspect specs in parallel still reproduced the loan flakes, confirming the issue is internal to the loan flow and not caused by unrelated specs.
  - [x] 1.4 Captured the failure via `docker logs e2e-mybibli-1` which surfaced the actual server error (see Debug Log References below). No Playwright trace needed — the logs made the root cause obvious.

- [x] **Task 2 — Diagnose the race** (AC #2)
  - [x] 2.1 **Initial diagnosis (wrong):** suspected `waitForURL(/\/loans/)` no-op in `setupLoan` and `createLoanForBorrower`. This WAS a real bug (the URL already matched /loans so the wait was instant), but fixing it with an `#loan-feedback` assertion-as-wait uncovered the real issue.
  - [x] 2.2 **Actual root cause:** MariaDB deadlock (SQLSTATE 40001 / error 1213) inside `LoanService::register_loan`. Server logs showed: `"error returned from database: 1213 (40001): Deadlock found when trying to get lock; try restarting transaction"`. Parallel workers inserting into `loans` + `UPDATE volumes` + `SELECT ... FOR UPDATE` acquire InnoDB next-key locks in incompatible orders. InnoDB picks a victim transaction and kills it with 1213, producing a 500 to the test.
  - [x] 2.3 The `page.on("dialog", ...)` handler registration is indeed async. Replaced with `page.once("dialog", ...)` in helpers and test bodies, registered BEFORE the click.
  - [x] 2.4 Searched all 3 specs for `waitForTimeout` — 10 occurrences found across loan-returns (7) and borrower-loans (3). All removed and replaced with explicit DOM waits (`toContainText`, `not.toContainText`, `toBeVisible`).

- [x] **Task 3 — Fix the root cause** (AC #2, #3, #4)
  - [x] 3.1 **Server-side fix:** `LoanService::register_loan` now wraps its transaction in a deadlock-retry loop (`LOAN_CREATE_DEADLOCK_RETRIES = 3`). On SQLSTATE 40001, the full transaction is retried (not just the statement) with a fresh connection. This is the production-correct pattern for InnoDB concurrent writes — real librarians creating loans simultaneously would have hit this bug in production.
  - [x] 3.2 **Client-side helper:** created `tests/e2e/helpers/loans.ts` with canonical `scanTitleAndVolume`, `createBorrower`, `createLoan`, `returnLoanFromLoansPage` helpers. `createLoan` uses direct `page.request.post('/loans', {form: {...}})` instead of the HTMX form, giving deterministic commit-before-return semantics without any dependency on HTMX client-side interception.
  - [x] 3.3 All 3 specs (`loan-returns.spec.ts`, `loans.spec.ts`, `borrower-loans.spec.ts`) refactored to use the new helpers. No `waitForTimeout`, no `waitForURL` no-ops, no multiple `dialog` handlers, no fragile `.last()` selectors.

- [x] **Task 4 — Regression guard** (AC #1)
  - [x] 4.1 Ran the full suite 15 times across the investigation and validation cycles. After the final fix (deadlock retry + POST-based helper), achieved **5 consecutive green runs (runs 11–15)** plus 7+ total green runs in the session. See Debug Log References for the exact sequence.
  - [x] 4.2 5-run results documented below in Debug Log References.
  - [x] 4.3 No `test.retries`, no `test.describe.serial`, no `waitForTimeout` added. Suite runtime unchanged (~15–16s per run, same as pre-fix).

- [x] **Task 5 — Update CLAUDE.md** (AC #5)
  - [x] 5.1 Added a bullet under "E2E Test Patterns" → "Known app quirks" documenting the MariaDB deadlock retry on concurrent loan creation and the HTMX form-swap race on `/loans`.
  - [x] 5.2 Added `tests/e2e/helpers/loans.ts` to the "Helper files" list with a one-line rationale about why it uses direct POST instead of HTMX.

- [x] **Task 6 — Verification gate** (AC #6)
  - [x] 6.1 `cargo clippy --all-targets -- -D warnings` — clean
  - [x] 6.2 `cargo test` — 317 lib tests pass
  - [x] 6.3 `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test --test find_similar` — 12 integration tests pass in 2.3s
  - [x] 6.4 `DATABASE_URL=... cargo sqlx prepare --check --workspace -- --all-targets` — clean
  - [x] 6.5 5 consecutive E2E full-suite runs 131/131 green (runs 11–15)

### Review Findings

Code review run on 2026-04-11 by the bmad-code-review workflow (Blind Hunter + Edge Case Hunter + Acceptance Auditor). Story acceptance criteria ✅ all satisfied. 24 raw findings → 12 patch + 1 decision-needed + 4 deferred + 7 dismissed after triage and source verification.

**Decision needed (resolved 2026-04-11 → dismissed):**

- [x] [Review][Decision] Smoke tests use `createLoan` direct-POST instead of the HTMX form — possible Foundation Rule #7 tension — **Resolved:** accepted as documented. The HTMX form path remains covered by the double-loan test (`loans.spec.ts:103-129` uses `#loan-create-form` directly), so the form UI is not orphaned. Pragmatic tradeoff to preserve parallel stability.

**Patch (fixable, unambiguous) — all 12 fixes applied 2026-04-11:**

- [x] [Review][Patch] `is_deadlock_error` substring match is too broad and misses lock-wait-timeout (SQLSTATE HY000 / MySQL 1205) [src/services/loans.rs:26-34] — **Fixed:** renamed to `is_transient_conflict`, dropped the substring fallback, added a downcast to `sqlx::mysql::MySqlDatabaseError` to detect MySQL error 1205 explicitly.
- [x] [Review][Patch] Retry reuses stale `volume.location_id` captured before the loop [src/services/loans.rs:57, 82] — **Fixed:** extracted `register_loan_attempt` which re-fetches the volume inside each retry, so `previous_location_id` is always read fresh.
- [x] [Review][Patch] Pre-txn validations (volume exists / is_loanable / borrower exists) not re-run on retry [src/services/loans.rs:56-76, 80] — **Fixed:** validations 1–3 moved into `register_loan_attempt`, so each retry re-runs them against current state.
- [x] [Review][Patch] Retry budget off-by-one vs. documentation ("up to 3 times" = 2 retries + 1 initial = 3 total attempts) [src/services/loans.rs:22, 84] — **Fixed:** renamed constant to `LOAN_CREATE_MAX_ATTEMPTS`, doc-comment now says "max 3 total attempts = 1 initial + up to 2 retries". Also updated CLAUDE.md quirk #3 wording (see below).
- [x] [Review][Patch] No unit test for `is_deadlock_error` classifier — Foundation Rule #2 violation [src/services/loans.rs:239-247] — **Fixed:** added `is_transient_conflict_rejects_non_database_errors` test covering RowNotFound / PoolTimedOut / PoolClosed / WorkerCrashed / ColumnNotFound / Protocol variants. Database-variant coverage (40001, 1205) left to the parallel E2E suite — a unit-level test would require a custom `DatabaseError` impl.
- [x] [Review][Patch] `returnLoanFromBorrowerDetail` asserts `page.locator("body").not.toContainText(volumeLabel)` [tests/e2e/specs/journeys/borrower-loans.spec.ts:36-38, also :94/101/126/133] — **Fixed:** added `id="active-loans-section"` to the borrower detail template, scoped all four assertions (plus the Return button lookup inside the helper) to that section.
- [x] [Review][Patch] `scanTitleAndVolume` waits on `.feedback-entry.first()` — can lock onto a stale ISBN-scan entry [tests/e2e/helpers/loans.ts:44-47] — **Fixed:** now uses `.locator(".feedback-entry").filter({ hasText: new RegExp(\`\\b${volumeLabel}\\b\`, "i") })` with an explicit `toBeVisible` wait.
- [x] [Review][Patch] `getBorrowerIdByName` silently picks the wrong borrower when `.first()` falls back across collisions [tests/e2e/helpers/loans.ts:72-86] — **Fixed:** `await expect(link).toHaveCount(1)` asserted before reading the href — collisions now fail loud.
- [x] [Review][Patch] `createBorrower` waits on `page.locator("body").toContainText(name)` — matches stale DOM / form echo [tests/e2e/helpers/loans.ts:63] — **Fixed:** wait is now scoped to the `/borrower/:id` anchor matched by exact-name regex.
- [x] [Review][Patch] `returnLoanFromLoansPage` row filter uses substring match [tests/e2e/helpers/loans.ts:149-151] — **Fixed:** filter now uses `new RegExp(\`\\b${volumeLabel}\\b\`)`.
- [x] [Review][Patch] `createLoan` follows 303 redirect silently, diagnostic noise on /loans errors [tests/e2e/helpers/loans.ts:114-125] — **Fixed:** `page.request.post` now uses `maxRedirects: 0` and asserts `response.status() === 303` — the 303 is the direct signal from the handler, errors on the subsequent GET no longer masquerade as loan-create failures.
- [x] [Review][Patch] Volume edit success asserted via `not.toHaveURL(/\/edit$/)` — false-passes on 4xx error URLs with query strings [tests/e2e/specs/journeys/loans.spec.ts:71] — **Fixed:** positive assertion `toHaveURL(new RegExp(\`/volume/${volumeId}$\`))` matching the handler's `Redirect::to("/volume/{id}")` target.

**Post-fix verification (2026-04-11):**

- `cargo check` — clean
- `cargo clippy --all-targets -- -D warnings` — clean
- `cargo test --lib` — **318 passed** (317 previous + 1 new regression guard for `is_transient_conflict`)
- `DATABASE_URL=... cargo sqlx prepare --check --workspace -- --all-targets` — clean
- `npx playwright test --list specs/journeys/{borrower-loans,loan-returns,loans}.spec.ts` — 18 specs parse cleanly, no TS errors
- E2E full-suite re-run **not yet performed** — Foundation Rule #6 requires a clean second review pass before marking `done`.

### Review Findings — Pass 2 (Foundation Rule #6 re-review, 2026-04-11)

Re-review after Pass-1 fixes. **Status: CLEAN — zero new Medium+ findings.** Acceptance Auditor verified all 12 Pass-1 fixes present and correct, zero anti-patterns introduced, scope respected. 3 new Low patches identified for cheap robustness gains; 4 additional deferred items; 3 dismissals (duplicates of Pass-1 decisions or pre-existing).

**Patch (Low, cheap robustness) — all 3 fixes applied 2026-04-11:**

- [x] [Review][Patch] `volumeLabel` not regex-escaped before interpolation into `\b…\b` filters [tests/e2e/helpers/loans.ts:44-47, 149-151] — **Fixed:** factored a shared `escapeRegex()` helper in `tests/e2e/helpers/loans.ts` and applied it in `scanTitleAndVolume`, `returnLoanFromLoansPage`, `createBorrower`, and `getBorrowerIdByName`. All four helpers now share the same escape discipline.
- [x] [Review][Patch] `returnLoanFromLoansPage` regex is case-sensitive while `scanTitleAndVolume` is case-insensitive [tests/e2e/helpers/loans.ts:149-151] — **Fixed:** added the `"i"` flag to `returnLoanFromLoansPage`'s row filter regex. Helper parity restored.
- [x] [Review][Patch] Volume edit `toHaveURL(new RegExp(\`/volume/${volumeId}$\`))` false-fails on future `?query` or `#fragment` appended to the redirect URL [tests/e2e/specs/journeys/loans.spec.ts:71-73] — **Fixed:** widened to `toHaveURL(new RegExp(\`/volume/${volumeId}(?:$|[?#])\`))` so a future flash-message redirect does not trip the assertion.

**Post-fix verification (2026-04-11, Pass 2):**

- `npx playwright test --list specs/journeys/{borrower-loans,loan-returns,loans}.spec.ts` — 18 specs still parse cleanly, no TS errors.
- Rust gates untouched this pass (fixes are TS-only); Pass-1 `cargo clippy --all-targets -- -D warnings` + `cargo test --lib` (318 tests) + `cargo sqlx prepare --check` still apply.
- E2E full-suite 5-run gate (Foundation Rule #5) **re-run and green post-fixes** (2026-04-11):
  - Run 1 (rebuild): 131/131 passed in 15.8s
  - Run 2: 131/131 passed in 15.1s
  - Run 3: 131/131 passed in 15.2s
  - Run 4: 131/131 passed in 15.7s
  - Run 5: 131/131 passed in 15.3s

  Each run on a fresh `docker compose -f tests/e2e/docker-compose.test.yml down -v && up -d` stack. Total re-gate time ~7 minutes. Suite runtime unchanged vs. the pre-fix baseline (15.2s avg, well under the AC #4 +10% budget). Worth noting: the first attempt of run 1 failed before the image was rebuilt — a cached docker image masked the `templates/pages/borrower_detail.html` change and the `src/services/loans.rs` refactor. `docker compose build` must be invoked after touching Askama templates or Rust code; `up -d` alone reuses the cached image.

**Deferred (Pass 2):**

- [x] [Review][Defer] `getBorrowerIdByName` fails once active borrower count exceeds `/borrowers` default page size (25) [tests/e2e/helpers/loans.ts:72-93] — `/borrowers` renders page 1 only; `BorrowerModel::list_active` uses `DEFAULT_PAGE_SIZE = 25`. Currently ~12 borrowers are created per full E2E run and no `afterEach` cleanup exists (suite-wide accretion). The helper will break when accumulated borrowers pass the threshold. Pre-existing limitation — the old `.first().getAttribute("href")` had the same blindspot. Deferred because the fix requires either a dedicated borrower-search helper or walking pages, and is non-urgent.
- [x] [Review][Defer] Retry log confusion: `tracing::warn!("retrying")` followed by a `BadRequest` from re-validation on the retry attempt gives the impression the retry caused the validation error [src/services/loans.rs:101-114, 131-145] — Observability debt. A concurrent soft-delete between attempts produces a misleading log trail. Deferred: requires carrying `attempt` and `prior_transient` through the tracing span on final error.
- [x] [Review][Defer] `register_loan` retry loop does not retry `sqlx::Error::PoolTimedOut` even though the docs advertise robustness under parallel load [src/services/loans.rs:94-107] — `is_transient_conflict` correctly returns false for non-Database variants (per unit test), but under heavy contention the pool timeout can be the first symptom and the caller sees a 500 with no retry. Not strictly a bug — retrying on pool timeout could worsen contention — but the doc/CLAUDE.md "auto-retries" phrasing is broader than the actual behavior. Defer doc refinement.
- [x] [Review][Defer] `returnLoanFromBorrowerDetail` helper's `#active-loans-section` assertion depends on the return handler's OOB swap target [tests/e2e/specs/journeys/borrower-loans.spec.ts:29-44] — If the handler only swaps a flash region and leaves the section stale, the helper waits 10s then fails. Current specs follow the helper with `page.reload()` which masks the issue, but future callers that skip the reload could hit a false flake. Speculative; defer verification of the handler's OOB targets.

**Dismissed (Pass 2):**

- Blind Hunter: smoke tests go through `createLoan` direct POST (Foundation Rule #7 spirit) — already resolved as D1 in Pass 1, accepted as documented tradeoff.
- Blind Hunter: pre-txn validations still run outside the transaction, leaving a TOCTOU window for concurrent soft-deletes — pre-existing pattern, not introduced by Pass-1 fixes. Tracked separately if it ever becomes a live issue.
- Edge Case Hunter: `createLoan` throws instead of surfacing `BadRequest` body — duplicate of a Pass-1 deferred item (same concern, no new info).

**Deferred (real, out of story scope):**

- [x] [Review][Defer] `loans.rs` retry loop has no "exhausted retries" terminal log [src/services/loans.rs:93] — deferred, observability enhancement only (per-attempt `tracing::warn!` already exists)
- [x] [Review][Defer] `returnLoanFromLoansPage` `page.once("dialog")` fires silently if the template ever drops `confirm()` [tests/e2e/helpers/loans.ts:146-148] — deferred, implicit template-helper contract, no current regression
- [x] [Review][Defer] No deterministic E2E test injects a synthetic deadlock to exercise the retry path [src/services/loans.rs] — deferred, would require a concurrent-transaction harness; probabilistic coverage from parallel runs is acceptable for now
- [x] [Review][Defer] `createLoan` helper throws on non-2xx instead of exposing the feedback body — breaks negative-path callers if reused [tests/e2e/helpers/loans.ts:120-125] — deferred, no current caller uses `createLoan` for the failing path (the double-loan spec uses the HTMX form directly)

## Dev Notes

### Leading hypothesis — HTMX POST not awaited before navigation

After scanning the 3 failing specs, the recurring pattern is:

```ts
// In loan-returns.spec.ts:117
await setupLoan(page, "V0074", "LR-Overdue Borrower");
// ... possibly more setup ...
await expect(page.locator("body")).toContainText("V0074", { timeout: 5000 });
```

And in `loans.spec.ts:266`:

```ts
await scanField.fill("V0090");
await expect(page.locator(".feedback-entry").first()).toContainText(/V0090/i);
// ... create loan via HTMX ...
await expect(page.locator("#loan-feedback")).toContainText(/V0090|created|créé/i);
await page.goto("/loans");  // ← fresh GET, not HTMX
await expect(page.locator("#loans-table-body")).toContainText("V0090", { timeout: 5000 });
```

The `#loan-feedback` assertion confirms the HTMX POST **response** was received by the browser. But the assertion happens client-side — it does NOT wait for the server-side transaction to commit before Playwright proceeds. Under parallel load, multiple workers hammer the DB and the commit may lag the HTTP response acknowledgement. The subsequent `page.goto("/loans")` then reads `/loans` which queries `active_loans()` from the DB — and misses the just-created row.

**If this hypothesis is correct**, the fix is:
1. Use `page.waitForResponse(resp => resp.url().includes('/loan') && resp.request().method() === 'POST')` to wait for the POST response explicitly.
2. Or: server-side, ensure `SessionModel::set_current_title` and other side-effect writes are awaited BEFORE the HTMX response is returned.

Task 2 will confirm or disprove this by tracing a failing run.

### Alternative hypothesis — Playwright browser context race

`page.on("dialog", ...)` registration is asynchronous in Playwright. If the test clicks a Return button that triggers `confirm()` before the dialog handler is fully wired up, the dialog dismisses with the default action (usually cancel) and the loan is NOT returned. The assertion then fails because the loan is still there.

This specifically matches `borrower-loans.spec.ts:95` which uses `page.on("dialog", ...)` right before clicking.

**Fix pattern:** register the dialog handler BEFORE navigating to the page that may show it. Compare with loan-returns.spec.ts which probably does it correctly.

### E2E test infrastructure snapshot (from story 5-1 + 5-1b)

- **Execution mode:** `fullyParallel: true` with default worker count (typically 6 in CI, matches host cores locally).
- **Login strategy:** `loginAs(page)` in `beforeEach` — real browser login, per-test server-side session (no cookie injection).
- **Data isolation:** `specIsbn("XX", seq)` per-spec prefixes guarantee ISBN uniqueness across specs. V-codes are per-spec by convention (LN uses V0060+, LR uses V0070+, BL uses V0080+, etc.) but NOT enforced by any allocator — developers must manually pick non-overlapping ranges.
- **HTMX wait strategies:** CLAUDE.md hard-bans `waitForTimeout`. Use `expect(locator).toBeVisible()` / `.toContainText(regex)` for explicit waits. See CLAUDE.md → E2E Test Patterns for the canonical examples.
- **Mock metadata server:** `tests/e2e/mock-metadata-server/server.py` returns synthetic "Synthetic TestAuthor" for every scanned ISBN. Not relevant to loan specs (they don't rely on metadata content).

### Relevant files and line numbers

- `tests/e2e/specs/journeys/loan-returns.spec.ts:117` — overdue loan test (V0074)
- `tests/e2e/specs/journeys/loan-returns.spec.ts:153` — scan V-code → return test
- `tests/e2e/specs/journeys/loans.spec.ts:266` — active loan TIMESTAMP regression test (V0090)
- `tests/e2e/specs/journeys/borrower-loans.spec.ts:95` — return loan from borrower detail (V0081)
- `tests/e2e/helpers/auth.ts` — canonical `loginAs` pattern
- `tests/e2e/helpers/isbn.ts` — `specIsbn` generator
- `src/routes/loans.rs` — loan create/return handlers
- `src/models/loan.rs` — loan DB operations
- `src/middleware/htmx.rs` — HtmxResponse OOB pattern

### Anti-patterns to avoid

1. ❌ **`page.waitForTimeout(N)`** — hard-banned by CLAUDE.md. Never use this to work around a race.
2. ❌ **`test.retries(N)`** — masks the symptom, doesn't fix the cause. Violates AC #4.
3. ❌ **`test.describe.serial`** — serializes the spec, which contradicts `fullyParallel: true` and slows the suite. Only use as absolute last resort after Task 2 proves no other fix is possible, and document extensively.
4. ❌ **Silently skipping the failing tests** — obviously.
5. ❌ **Treating the flakes as "known issues"** — they ARE known, but the point of this story is to FIX them.

### Previous story intelligence — lessons from 5-1 and 5-1b

From `_bmad-output/implementation-artifacts/5-1-e2e-stabilization.md` and `5-1b-e2e-data-isolation-architecture.md`:

- **Story 5-1** (stabilized 83/120 → 116/120): established the `loginAs` per-test session pattern and fixed many non-deterministic selectors. It did NOT fix loan specs specifically because loan specs were already "mostly passing" at the time.
- **Story 5-1b** (added data isolation): introduced `specIsbn(specId, seq)` so each spec file uses unique ISBNs. It explicitly did NOT address V-code isolation because V-codes are user-facing labels (not API-generated), and the dev decided manual per-spec conventions were enough. This story 5-1c may need to revisit that decision.
- **Key learning from 5-1:** "never use `waitForTimeout`, always wait for DOM state explicitly" — apply this rigorously to the fixes in Task 3.
- **Key learning from 5-1b:** "parallel mode requires true per-test server-side session isolation" — verify the failing specs all use `loginAs(page)` in `beforeEach` (spot check confirms yes).

### Git intelligence — recent commits relevant to loans

Search commits touching `src/routes/loans.rs`, `src/models/loan.rs`, or `tests/e2e/specs/journeys/loan*`. Expected to find:

- Story 4-2 (loan-registration-and-validation): the TIMESTAMP fix that `loans.spec.ts:266` is a regression test for
- Story 4-3 (loan-return-and-location-restoration): the `setupLoan` helper + return flow
- Story 4-4 (borrower-detail-and-loan-history): introduced `borrower-loans.spec.ts`
- Story 5-1 (e2e-stabilization): migrated `loans.spec.ts` to `loginAs` pattern

Use `git log --oneline --follow tests/e2e/specs/journeys/loan-returns.spec.ts` and similar to see the history.

## References

- Story 5-7 code review validation gate (2026-04-10) — observed the 3 failing runs and confirmed isolation passes
- `_bmad-output/implementation-artifacts/5-1-e2e-stabilization.md` — baseline stabilization
- `_bmad-output/implementation-artifacts/5-1b-e2e-data-isolation-architecture.md` — per-spec data isolation
- `_bmad-output/implementation-artifacts/5-7-similar-titles-section.md` → Debug Log References → "Loan-spec flakes on repeated runs"
- `CLAUDE.md` → "E2E Test Patterns" → "HTMX wait strategies" and "Known app quirks"
- Foundation Rule #5 — ALL tests must be green before milestone transition
- `tests/e2e/playwright.config.ts` — `fullyParallel: true` configuration

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

**Initial diagnosis (incorrect, kept for posterity):**
The leading hypothesis in the story scoping was that the flakes came from `page.goto("/loans")` racing the HTMX POST commit, or from `#loan-feedback` assertions matching stale content. Fixing those issues uncovered a deeper bug.

**Actual root cause (confirmed by server logs):**
```
{"level":"ERROR","fields":{"message":"request error","status":"500 Internal Server Error",
"message":"error returned from database: 1213 (40001): Deadlock found when trying to get lock; try restarting transaction"},
"target":"mybibli::error"}
```
Concurrent workers hitting `LoanService::register_loan` race on three locks:
1. `SELECT id FROM loans WHERE volume_id = ? ... FOR UPDATE` — next-key lock on the loans `volume_id` index
2. `UPDATE volumes SET location_id = NULL WHERE id = ?` — X-lock on the volume row
3. `INSERT INTO loans (...)` — intention locks + auto-increment contention

Different transactions acquire these locks in different orders, InnoDB detects the deadlock cycle and aborts one transaction with SQLSTATE 40001 / error 1213. The standard fix is client-side retry of the transaction — which is what was added.

**Why the old helper didn't trigger this:**
The pre-fix helpers in loan-returns.spec.ts and borrower-loans.spec.ts ended with `await page.waitForURL(/\/loans/, { timeout: 10000 })`. This was a no-op because the loan form lives ON the `/loans` page — the URL already matched, so the wait resolved instantly before the POST even completed. Many tests raced the commit, failed to see the row, and flaked. Once the helper was rewritten to wait for the actual commit (via POST-based createLoan), the deadlock became visible because every call now genuinely synchronized on the DB.

**5-run gate results (runs 11–15, all on fresh `down -v` + up stacks):**
- Run 11: 131/131 passed in 15.2s
- Run 12: 131/131 passed in 15.3s
- Run 13: 131/131 passed in 15.3s
- Run 14: 131/131 passed in 15.2s
- Run 15: 131/131 passed in 14.9s

No retries used. No `test.describe.serial` added. No `waitForTimeout` added. Total investigation + validation: 15 full-suite runs across the session (8+ consecutive greens post-fix in the best streak).

**Drive-by discovery (not fixed in this story):**
Run 8 (during earlier validation sweeps) hit a transient flake in `media-type-scanning.spec.ts` → "UPC scan → select CD → MusicBrainz metadata loads". Three tests in that spec share the hardcoded UPC `0093624738626` in parallel, and one occasionally misses the `.feedback-skeleton` assertion. This is an Epic 3 flake, different code path from Epic 4 loans, and out of scope for story 5-1c. Not observed in the final 5-run gate (runs 11–15). Flagged for potential follow-up if it recurs.

### Completion Notes List

- **Root cause:** server-side MariaDB deadlock on concurrent loan creation (NOT a pure test bug). The previous tests masked the deadlock because their helpers returned before the server commit completed (`waitForURL` no-op).
- **Server fix:** added transactional deadlock retry in `LoanService::register_loan` with a new helper `register_loan_txn` that runs inside a retry loop. Detects SQLSTATE 40001 via `is_deadlock_error()` helper and retries up to 3 times. Standard InnoDB concurrent-write pattern.
- **E2E helper refactor:** created `tests/e2e/helpers/loans.ts` with canonical `scanTitleAndVolume`, `createBorrower`, `createLoan`, `returnLoanFromLoansPage`. `createLoan` uses `page.request.post('/loans', {form: ...})` for deterministic commit semantics. `getBorrowerIdByName` uses exact-match regex to avoid cross-test collisions.
- **Spec rewrites:** `loan-returns.spec.ts`, `loans.spec.ts`, `borrower-loans.spec.ts` rewritten to use the helpers. All `waitForTimeout` calls removed (10 in total across the 3 files). All `page.on("dialog")` converted to `page.once("dialog")` registered before the click. All `.last()` selectors replaced with specific IDs.
- **No scope creep:** did not touch media-type-scanning, borrower-crud, catalog-contributor, etc. Fix is contained to loan/borrower flow as story 5-1c scoped.
- **CLAUDE.md:** updated "Known app quirks" with the deadlock retry note and added `helpers/loans.ts` to the helpers list.
- **Verification:** clippy + 317 lib tests + 12 find_similar integration tests + sqlx prepare check all green. 5 consecutive E2E suite runs 131/131 (runs 11–15, ~15s each).

### File List

**Created:**
- `tests/e2e/helpers/loans.ts` — canonical loan-flow helpers (scanTitleAndVolume, createBorrower, createLoan, returnLoanFromLoansPage, getBorrowerIdByName)

**Modified:**
- `src/services/loans.rs` — added `LOAN_CREATE_DEADLOCK_RETRIES`, `is_deadlock_error()`, split `register_loan` into outer retry loop + `register_loan_txn` inner body
- `tests/e2e/specs/journeys/loan-returns.spec.ts` — rewritten to use `helpers/loans.ts`, removed 7 `waitForTimeout` calls, removed `waitForURL` no-op, fixed dialog handler registration
- `tests/e2e/specs/journeys/loans.spec.ts` — rewritten register/smoke/TIMESTAMP-regression tests to use helpers, removed fragile `.last()` selectors in favour of id-based and filter-based locators
- `tests/e2e/specs/journeys/borrower-loans.spec.ts` — rewritten to use helpers, removed 3 `waitForTimeout` calls, added `returnLoanFromBorrowerDetail` inline helper with proper dialog handling
- `CLAUDE.md` — added deadlock retry note under "Known app quirks", added `helpers/loans.ts` to "Helper files" list
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — 5-1c status progression
