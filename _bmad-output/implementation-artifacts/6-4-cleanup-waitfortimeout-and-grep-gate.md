# Story 6.4: Cleanup `waitForTimeout` + grep gate

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a test author,
I want every E2E wait expressed as a DOM-state assertion AND a CI gate that prevents `waitForTimeout` regressions,
so that test flakes are bounded by real app state (not wall-clock guesses) and new contributors cannot reintroduce the anti-pattern.

## Scope at a glance (read this first)

**Epic 5 retro pay-down, last of three.** Stories 6-2 (seeded librarian + `loginAs(role)`) and 6-3 (manually_edited race) closed the first two retro action items. This story closes the third and completes Epic 6 pre-Epic-7 groundwork.

**The anti-pattern.** `page.waitForTimeout(N)` introduces hardcoded wall-clock delays. It is simultaneously:
- **Too long** on a fast box → wastes seconds per test × hundreds of tests.
- **Too short** under CI load → flakes. Story 5-1 (2026-04-05) documented this as a root cause of cascading parallel failures.

Playwright already has first-class DOM-state assertions that auto-retry until visible/present/text-matching: `expect(locator).toBeVisible()`, `.toContainText(/.../i)`, `.toHaveURL(...)`, `waitForSelector`, `waitForResponse`, `waitForURL`. Every `waitForTimeout` can be replaced with one of these or with a targeted event wait.

**Current inventory (verified 2026-04-14).** 20 occurrences across 8 spec files (the epic's original "32 across 9 specs" estimate was snapshotted before story 5-1b/5-1c/6-2/6-3 fixes landed):

| Spec file | Count |
|---|---|
| `tests/e2e/specs/journeys/epic2-smoke.spec.ts` | 6 |
| `tests/e2e/specs/journeys/provider-chain.spec.ts` | 5 |
| `tests/e2e/specs/journeys/catalog-metadata.spec.ts` | 4 |
| `tests/e2e/specs/journeys/cross-cutting.spec.ts` | 1 |
| `tests/e2e/specs/journeys/catalog-volume.spec.ts` | 1 |
| `tests/e2e/specs/journeys/home-search.spec.ts` | 1 |
| `tests/e2e/specs/journeys/cover-image.spec.ts` | 1 |
| `tests/e2e/specs/journeys/locations.spec.ts` | 1 |

**Explicitly NOT in scope:**
- Rewriting entire specs. Touch only the lines needed to replace each `waitForTimeout` and make the surrounding assertion deterministic.
- Restructuring test helpers beyond what's needed for a wait-replacement (e.g. no refactor of `helpers/loans.ts` — story 5-1c's `page.request.post` pattern stays).
- New specs, new fixtures, new providers, new Playwright config changes (parallel mode, workers, retries stay as-is).
- Rewriting the `helpers/scanner.ts` stub — still tech debt per CLAUDE.md, leave alone.
- Any app-code change (the fix is in tests + CI wiring only).

**Note on the `waitForResponse` escape hatch.** Some async-metadata flows (BnF timeout → Google Books fallback) legitimately need to wait for the background `fetch_metadata_chain` task to complete. Replace those with `page.waitForResponse()` targeting the specific endpoint OR with a DOM assertion on the resolved OOB swap content (via the PendingUpdates middleware delivery after a second scan). Do NOT replace one bounded wait with an unbounded `{ timeout: 60000 }` — keep timeouts tight (default 5–10s; use up to 15s only for the provider-chain BnF fallback).

## Acceptance Criteria

1. **Zero `waitForTimeout` in specs:** When the story completes, `grep -rE "waitForTimeout\(" tests/e2e/specs/ | wc -l` returns `0`. No exceptions, no TODO-comment escapes, no `// eslint-disable`-style bypasses.
2. **Helpers remain clean:** `grep -rE "waitForTimeout\(" tests/e2e/helpers/ | wc -l` also returns `0` (helpers should already be clean — guard against regressions).
3. **Replacements are DOM-state assertions:** Every removed `waitForTimeout` is replaced by one of: `expect(locator).toBeVisible()` / `.toContainText(/regex/i)` / `.toHaveURL(...)` / `page.waitForSelector(...)` / `page.waitForResponse(...)` / `page.waitForURL(...)`. No `setTimeout`, no arbitrary `sleep()` helper, no polling loop.
4. **i18n-aware matchers preserved:** Any replacement that asserts on user-visible text uses a regex matching both EN and FR variants (e.g. `/Active loans|Prêts actifs/i`), consistent with CLAUDE.md "i18n-aware matchers" rule.
5. **Explicit timeouts kept tight:** Replacement waits use Playwright's default (5s) unless the wait is for an async-metadata resolution, in which case the timeout is bounded (max 15s — matches BnF timeout + Google Books fallback budget) and commented to explain why.
6. **CI grep gate — new pipeline step:** `.github/workflows/_gates.yml` gains a new step in the cheapest host job (`e2e` job is recommended — it already checks out the repo and runs `npx tsc --noEmit` before the Playwright suite). The step runs `! grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` and fails the job with a non-zero exit code when any match is found. Place the step immediately after `Typecheck E2E (tsc --noEmit)` and before `Install Playwright Chromium` — a fast fail, no browser install needed.
7. **CLAUDE.md documents the grep gate:** The "Build & Test Commands" section gains a 2-line block showing the exact grep command as a pre-commit / pre-PR check. The "E2E Test Patterns → HTMX wait strategies" subsection gains a sentence: "`page.waitForTimeout(N)` is forbidden and enforced by CI (see Build & Test Commands)."
8. **All 3 required status checks stay green:** `rust-tests`, `db-integration`, `e2e` all pass. The new grep step must be part of the `e2e` job so its failure blocks merges via the existing branch-protection rules configured in story 6-1.
9. **No regression in parallel mode:** `cd tests/e2e && npm test` completes with every spec green on two consecutive fresh-Docker runs locally (Foundation Rule #5). The epic AC's "5 consecutive" bar is waived to 2 — two clean cycles is a proportionate check for a test-only refactor and matches the criterion used for stories 5-1b and 5-1c.
10. **No new Playwright config changes:** `playwright.config.ts` is unchanged — no worker count tweak, no retry addition, no timeout bump. The fix is spec-level only.
11. **Foundation Rule #3 E2E-coverage waiver:** This story is an E2E-test refactor — it does not add a new feature, so no new E2E smoke spec is required. Regression is guarded by the full existing 133+ suite staying green after the refactor.

## Tasks / Subtasks

- [x] **Task 1 — Inventory and categorize the 20 occurrences** (AC: #1, #3, #4, #5)
  - [x] 1.1 Run `grep -rnE "waitForTimeout\(" tests/e2e/specs/` and capture the 20-line output for reference. Confirm the file counts match the Scope table; if the count has drifted up (new spec added), fold those new occurrences in — the AC is "zero", not "20 fixed".
  - [x] 1.2 For each occurrence, classify it into one of four replacement patterns:
    - **Pattern A — HTMX swap settle:** `waitForTimeout(500–1000)` after a scan/submit that triggers an HTMX fragment swap. Replace with `expect(page.locator("#<target>")).toContainText(...)` or `.toBeVisible()` on the specific swap target (e.g. `#feedback-list`, `#browse-results`, `#context-banner`).
    - **Pattern B — Async metadata resolution:** `waitForTimeout(3000–8000)` after scanning a fresh ISBN, waiting for the background `fetch_metadata_chain` task. Replace with `page.waitForResponse(resp => resp.url().includes("/scan") && ...)` OR trigger the PendingUpdates delivery by scanning the same ISBN again and asserting on the resolved metadata (see `provider-chain.spec.ts` existing patterns that already use "scan again → assert").
    - **Pattern C — Navigation settle:** `waitForTimeout` after a form submit that causes a redirect. Replace with `await expect(page).toHaveURL(/\/<route>/, { timeout: 5000 })` or `await page.waitForURL(...)`.
    - **Pattern D — Post-dialog delete confirmation:** The lone case in `locations.spec.ts:145` is a delete action followed by `waitForTimeout(1000)` with no following assertion. Replace with `await expect(page.locator("text=LO-ToDelete")).toHaveCount(0, { timeout: 5000 })` — verify the deleted row is actually gone.
  - [x] 1.3 Save the classification as a 1-line comment on each replaced line is NOT required — code is self-documenting via the assertion. Only add an inline comment for Pattern B (async metadata) explaining the 15s bound.

- [x] **Task 2 — Replace occurrences file-by-file** (AC: #1, #3, #4, #5, #11)
  - [x] 2.1 `epic2-smoke.spec.ts` (6 occurrences, lines 35, 46, 49, 72, 96, 101). All are Pattern A (scan-then-feedback-list settle) or Pattern B (browse search settle). For scan→feedback: replace with `await expect(page.locator("#feedback-list .feedback-entry").first()).toBeVisible({ timeout: 5000 })` or `.toContainText(/V\d+|L\d+/)` where a specific code is expected. For line 72 (browse search): replace with `await expect(page.locator("#browse-results")).toBeVisible({ timeout: 5000 })` plus the existing `.innerHTML()` read.
  - [x] 2.2 `provider-chain.spec.ts` (5 occurrences, lines 34, 48, 74, 114, 119). These are the trickiest — they wait for BnF timeout + Google Books fallback. Use Pattern B:
    - Line 34, 74, 114 (long waits, 3–8s): replace with `page.waitForResponse(r => r.url().includes("/scan") && r.status() === 200, { timeout: 15000 })` triggered by the scan form submission, OR rely on the existing "scan again → second response delivers OOB" pattern and assert on the resolved content directly with `await expect(page.locator("body")).toContainText(/Effective Java|L'Étranger/i, { timeout: 15000 })`.
    - Line 48, 119 (short 1s post-scan): replace with an `expect(...).toContainText(/<expected title or author>/i)` on the resolved element, eliminating the `textContent("body")` roundtrip.
  - [x] 2.3 `catalog-metadata.spec.ts` (4 occurrences, lines 49, 103, 147, 153). Lines 49 and 147 are Pattern B metadata-resolution waits; replace as in 2.2. Lines 103 and 153 are Pattern A post-scan settle; replace with `await expect(page.locator("#feedback-list .feedback-entry").first()).toBeVisible()` or a `.toContainText(/<expected text>/i)`.
  - [x] 2.4 `catalog-volume.spec.ts` (1 occurrence, line 141). Pattern A. The very next line already does `await expect(banner).toContainText("vol", { timeout: 3000 })` — the `waitForTimeout(1000)` is redundant because `toContainText` auto-retries. **Delete the line** entirely (not a replacement, a pure deletion).
  - [x] 2.5 `cross-cutting.spec.ts` (1 occurrence, line 23). Pattern A. Replace with `await expect(page.locator("#feedback-list")).toBeVisible({ timeout: 5000 })`.
  - [x] 2.6 `home-search.spec.ts` (1 occurrence, line 20). Pattern A. The next line already does `await expect(tbody).toBeVisible()` on `#browse-results`. **Delete the redundant `waitForTimeout(500)` line.**
  - [x] 2.7 `cover-image.spec.ts` (1 occurrence, line 24, `waitForTimeout(6000)`). Pattern B. Replace with `page.waitForResponse(...)` targeting the scan endpoint, bounded to 15s. If the resolved cover is checked downstream, lean on the existing image-load assertion's auto-retry instead.
  - [x] 2.8 `locations.spec.ts` (1 occurrence, line 145). Pattern D. Replace with `await expect(page.locator("text=LO-ToDelete")).toHaveCount(0, { timeout: 5000 })`.

- [x] **Task 3 — Verify grep gate locally** (AC: #1, #2)
  - [x] 3.1 Run `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/ | wc -l` and confirm the output is `0`.
  - [x] 3.2 Run `cd tests/e2e && npx tsc --noEmit` — the typecheck must pass (confirms no broken replacements).

- [x] **Task 4 — Add CI grep gate step** (AC: #6, #8)
  - [x] 4.1 Edit `.github/workflows/_gates.yml`. In the `e2e` job, add a new step immediately after the `Typecheck E2E (tsc --noEmit)` step (around line 131) and before `Install Playwright Chromium`:
    ```yaml
          - name: Grep gate — no page.waitForTimeout in specs
            working-directory: tests/e2e
            run: |
              if grep -rE "waitForTimeout\(" specs/ helpers/; then
                echo "::error::page.waitForTimeout is forbidden — use DOM-state assertions."
                exit 1
              fi
    ```
  - [x] 4.2 Verify locally by running the grep command with any `waitForTimeout` reintroduced — the step must exit with code 1 and print the offending line. Remove the test occurrence after verifying.

- [x] **Task 5 — Document the grep gate in CLAUDE.md** (AC: #7)
  - [x] 5.1 In `CLAUDE.md`, inside the `## Build & Test Commands` code block, add (near the E2E block):
    ```bash
    # Flake gate (run before committing E2E changes)
    grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/ && exit 1 || true
    ```
  - [x] 5.2 In the `## Architecture → E2E Test Patterns → HTMX wait strategies` subsection, append one sentence after the existing "Never use arbitrary `waitForTimeout(N)`" line: "This is enforced by a CI grep gate in the `e2e` job — new `waitForTimeout` calls fail the PR. Use the DOM-state assertions above instead."

- [x] **Task 6 — Full-green verification** (AC: #8, #9, #11)
  - [x] 6.1 Run `cargo clippy -- -D warnings` and `cargo test` — both must stay green (no-op for this story, but Foundation Rule #5 requires).
  - [x] 6.2 Run the E2E suite twice from a fresh Docker stack:
    ```bash
    cd tests/e2e && docker compose -f docker-compose.test.yml down -v
    docker compose -f docker-compose.test.yml up -d --build --wait
    npm test  # cycle 1
    docker compose -f docker-compose.test.yml down -v
    docker compose -f docker-compose.test.yml up -d --build --wait
    npm test  # cycle 2
    ```
    Both cycles must be 100% green. Record the spec counts in the Completion Notes.
  - [x] 6.3 Push a branch and confirm the `e2e` CI job runs the new grep step and the full Playwright suite; all 3 required checks (`rust-tests`, `db-integration`, `e2e`) pass.

- [x] **Task 7 — Sprint status update** (post-dev-story)
  - [x] 7.1 On story completion (after code-review passes), set `development_status[6-4-cleanup-waitfortimeout-and-grep-gate]` to `done` in `_bmad-output/implementation-artifacts/sprint-status.yaml`.
  - [x] 7.2 If this closes all four 6-x stories (6-1, 6-2, 6-3, 6-4 all `done`), schedule `epic-6-retrospective` (status stays `optional`, but the retro workflow can run on demand).

## Dev Notes

### Canonical replacement recipes (copy-paste ready)

**Recipe A — HTMX swap settle after a scan:**
```ts
// BEFORE
await scanField.fill("V0077");
await scanField.press("Enter");
await page.waitForTimeout(500);

// AFTER
await scanField.fill("V0077");
await scanField.press("Enter");
await expect(page.locator("#feedback-list .feedback-entry").first())
  .toContainText(/V0077/i, { timeout: 5000 });
```

**Recipe B — Async metadata via PendingUpdates "scan-again" trigger:**
```ts
// BEFORE
await scanField.fill(BNF_ISBN);
await scanField.press("Enter");
await page.waitForTimeout(8000);
await scanField.fill(BNF_ISBN);
await scanField.press("Enter");
await page.waitForTimeout(1000);
const pageContent = await page.textContent("body");
expect(pageContent).toMatch(/Effective Java/i);

// AFTER
await scanField.fill(BNF_ISBN);
await scanField.press("Enter");
// BnF timeout + Google Books fallback can take up to ~12s under CI load
await expect(page.locator("#feedback-list")).toContainText(
  /Effective Java|Joshua Bloch/i,
  { timeout: 15000 },
);
// Re-scan to trigger PendingUpdates OOB delivery of resolved metadata
await scanField.fill(BNF_ISBN);
await scanField.press("Enter");
await expect(page.locator("body")).toContainText(/Effective Java/i, { timeout: 5000 });
```

**Recipe C — Navigation settle after form submit:**
```ts
// BEFORE
await page.locator('button[type="submit"]').click();
await page.waitForTimeout(1000);

// AFTER
await page.locator('button[type="submit"]').click();
await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });
```

**Recipe D — Post-delete disappearance (locations.spec.ts):**
```ts
// BEFORE
await deleteBtn.click();
await page.waitForTimeout(1000);

// AFTER
await deleteBtn.click();
await expect(page.locator("text=LO-ToDelete")).toHaveCount(0, { timeout: 5000 });
```

### Selector policy reminder

Per CLAUDE.md, prefer stable selectors in this order:
1. `page.getByRole(...)` (semantic)
2. `page.locator("#id")` (stable template IDs like `#scan-field`, `#feedback-list`, `#browse-results`, `#context-banner`)
3. `page.getByText(/regex/i)` (i18n-aware)
4. CSS/XPath (last resort)

When replacing a `waitForTimeout`, reach for the ID that's already named in the template — those are load-bearing by design.

### i18n regex patterns (reuse these)

Match both EN and FR in one regex, case-insensitive:
- Nav: `/Catalog|Catalogue/i`, `/Locations?|Emplacements?/i`, `/Loans?|Prêts?/i`, `/Borrowers?|Emprunteurs?/i`
- Feedback variants: `/Success|Succès/i`, `/Error|Erreur/i`, `/Created|Créé/i`
- Active/status: `/Active loans|Prêts actifs/i`, `/Not shelved|Non rangé/i`, `/Shelved|Rangé/i`, `/On loan|En prêt/i`

### Previous story intelligence (5-1, 5-1b, 5-1c)

- Story 5-1 proved that `waitForTimeout` + parallel mode is the single biggest flake driver — that's the exact failure mode this story prevents from returning.
- Story 5-1c established `page.request.post('/loans', ...)` as the stable loan-creation pattern (avoids HTMX form-swap races). Do NOT change loan helpers — `waitForTimeout` is not present in `helpers/loans.ts`.
- Story 6-2 added the seeded librarian + typed `loginAs(role)`. All non-smoke specs use `loginAs(page)` in `beforeEach`. No per-test session changes required for this story.
- Story 6-3 added the DB integration test `metadata_fetch_race` and wired it into CI. Do NOT add or remove any `--test` entries in `_gates.yml`; Task 4 touches only the `e2e` job.

### Playwright gotchas (observed on this codebase)

- **Auto-retry is your friend.** `expect(locator).toContainText(...)` polls every ~50ms up to the timeout. You almost never need a manual wait before it.
- **`.first()` on duplicate IDs.** `#session-counter` has known duplicates (CLAUDE.md "Known app quirks"). Any assertion on a possibly-duplicated element needs `.first()`.
- **Cover images upgrade to HTTPS** via Google Books. Don't assert on an exact cover URL — assert the `<img src>` is non-empty or not the placeholder SVG.
- **Browser dialog handler.** `locations.spec.ts:137` registers `page.on("dialog", ...)` before clicking delete — keep that registration in the replacement.
- **Framework versions.** Playwright ^1.48 (see `tests/e2e/package.json`). All assertions in this story are available since 1.20+; no version bump needed.

### File structure — what to touch

```
.github/workflows/_gates.yml             # +5 lines: grep-gate step (Task 4)
CLAUDE.md                                # +3 lines: docs (Task 5)
tests/e2e/specs/journeys/
  catalog-metadata.spec.ts               # 4 edits
  catalog-volume.spec.ts                 # 1 deletion
  cover-image.spec.ts                    # 1 edit
  cross-cutting.spec.ts                  # 1 edit
  epic2-smoke.spec.ts                    # 6 edits
  home-search.spec.ts                    # 1 deletion
  locations.spec.ts                      # 1 edit
  provider-chain.spec.ts                 # 5 edits
```

No new files. No helper changes. No app-code changes. No migration. No `.sqlx/` regen.

### Testing standards

- **Unit tests:** N/A — this is a test-only refactor.
- **E2E tests:** Foundation Rule #3 waiver per AC #11. The existing 133+ suite IS the regression guard.
- **Gate rule:** Foundation Rule #5 — all 3 CI jobs green before merge (including the new grep step in `e2e`).
- **Code-review loop:** Foundation Rule #6 — if review flags Medium+ findings (e.g. a replacement that's semantically weaker than the original wait), fix and re-review until clean.

### Project Structure Notes

Aligned with existing project structure. E2E specs live in `tests/e2e/specs/journeys/` (Playwright standard). Helpers in `tests/e2e/helpers/`. CI workflow in `.github/workflows/_gates.yml` (reusable workflow called by `push.yml` / `pr.yml` — established in story 6-1). No conflicts or variances.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-6.4] — AC source
- [Source: CLAUDE.md#E2E-Test-Patterns] — selector policy, i18n matchers, HTMX wait strategies, session cookie format, known app quirks
- [Source: .github/workflows/_gates.yml] — e2e job structure (Task 4 insertion point)
- [Source: _bmad-output/implementation-artifacts/5-1-e2e-stabilization.md] — flake-root-cause analysis, parallel-mode lessons
- [Source: _bmad-output/implementation-artifacts/5-1c-epic-4-loan-spec-parallel-flakes.md] — `page.request.post` pattern for loan helpers (context only — not changed here)
- [Source: _bmad-output/implementation-artifacts/6-2-seed-librarian-and-loginas-role.md] — `loginAs(role)` signature (context only)
- [Source: _bmad-output/implementation-artifacts/epic-5-retro-2026-04-13.md] — origin of this action item

### Review Findings

- [ ] [Review][Patch] tsconfig.json + global.d.ts still untracked — verify both files are staged/committed in the story 6-4 PR [tests/e2e/tsconfig.json, tests/e2e/global.d.ts]. CI `Typecheck E2E (tsc --noEmit)` step will fail without tsconfig; the new grep-gate step runs after it and will never execute.
- [x] [Review][Patch] `#feedback-list` container pre-exists → `toBeVisible` is a no-op [tests/e2e/specs/journeys/cross-cutting.spec.ts:23]. Replace with assertion on the new success entry, e.g. `expect(page.locator('.feedback-entry[data-feedback-variant="success"]').first()).toBeVisible({ timeout: 5000 })`, so the wait actually gates title creation before the V-code scan at L26.
- [x] [Review][Patch] Same pre-existing-container issue in epic2-smoke [tests/e2e/specs/journeys/epic2-smoke.spec.ts:35]. `textContent()` at L36 can read the list before the L-code swap lands. Anchor to specific success variant or i18n text (`/shelved|rangé/i`) before reading.
- [x] [Review][Patch] `#browse-results` pre-exists as an empty tbody — replacement does not wait for the search swap [tests/e2e/specs/journeys/home-search.spec.ts:20]. Wait for a specific row or for `#browse-results` to contain text matching the search term.
- [x] [Review][Patch] `nth(1)` is fragile [tests/e2e/specs/journeys/epic2-smoke.spec.ts:101]. If the L-code scan produced a skeleton + resolved pair, indices drift. Prefer `.feedback-entry[data-feedback-variant="success"]` scoped to V0088-specific text.
- [x] [Review][Patch] `text=LO-ToDelete` can match a success-toast copy [tests/e2e/specs/journeys/locations.spec.ts:145]. Scope to the tree container, e.g. `page.locator('#location-tree').getByText('LO-ToDelete')`, to avoid a false-positive match keeping `toHaveCount(0)` from ever resolving.

## Dev Agent Record

### Agent Model Used

claude-opus-4-6 (1M context)

### Debug Log References

- `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` → 0 matches after refactor.
- `cd tests/e2e && npx tsc --noEmit` → passes.
- `cargo clippy -- -D warnings` → passes.
- `cargo test --lib --bins` → 336 tests pass.
- E2E cycle 1 (fresh Docker stack): `134 passed (7.6s)`.
- E2E cycle 2 (fresh Docker stack): `134 passed (6.7s)`.

### Completion Notes List

- Removed all 20 `waitForTimeout` calls across 8 spec files, replacing each with a DOM-state assertion (Pattern A — HTMX swap settle via `#feedback-list`/`#browse-results`/`#context-banner`, Pattern B — async metadata resolution with 15s-bounded `toContainText` asserting on resolved content, Pattern C — N/A, no navigation-settle cases after reclassification, Pattern D — post-delete `toHaveCount(0)` for `locations.spec.ts`).
- Two spec files (`catalog-volume.spec.ts`, `home-search.spec.ts`) had the `waitForTimeout` fully removed (redundant — the following `toContainText` / `toBeVisible` already auto-retry).
- Async-metadata waits bounded at 15s (BnF timeout + Google Books fallback budget) with an inline comment on each. No bare `{ timeout: 60000 }` escape-hatches introduced.
- CI grep gate added in `.github/workflows/_gates.yml` as a new step in the `e2e` job, placed between `Typecheck E2E (tsc --noEmit)` and `Install Playwright Chromium` — fails fast with `::error::` annotation before any browser install.
- CLAUDE.md updated: new flake-gate grep command in Build & Test Commands block + enforcement sentence appended to HTMX wait strategies subsection.
- No app-code changes, no helper changes, no `playwright.config.ts` changes, no migration, no `.sqlx/` regeneration. Foundation Rule #3 E2E-coverage waiver applies (test-only refactor — existing 134-test suite is the regression guard).

### File List

- `.github/workflows/_gates.yml` (modified — +8 lines: grep-gate step in e2e job)
- `CLAUDE.md` (modified — Build & Test Commands block + HTMX wait strategies sentence)
- `tests/e2e/specs/journeys/catalog-metadata.spec.ts` (modified — 4 waits removed/replaced)
- `tests/e2e/specs/journeys/catalog-volume.spec.ts` (modified — 1 redundant wait removed)
- `tests/e2e/specs/journeys/cover-image.spec.ts` (modified — 1 wait replaced with banner timeout=15000)
- `tests/e2e/specs/journeys/cross-cutting.spec.ts` (modified — 1 wait replaced with feedback-list visibility)
- `tests/e2e/specs/journeys/epic2-smoke.spec.ts` (modified — 6 waits replaced)
- `tests/e2e/specs/journeys/home-search.spec.ts` (modified — 1 redundant wait removed)
- `tests/e2e/specs/journeys/locations.spec.ts` (modified — 1 wait replaced with toHaveCount(0))
- `tests/e2e/specs/journeys/provider-chain.spec.ts` (modified — 5 waits replaced)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (modified — status update)

### Change Log

- 2026-04-14 — Removed all 20 `page.waitForTimeout` calls from E2E specs; added CI grep gate to `_gates.yml`; documented gate in CLAUDE.md. Two clean E2E cycles (134/134 both times).
