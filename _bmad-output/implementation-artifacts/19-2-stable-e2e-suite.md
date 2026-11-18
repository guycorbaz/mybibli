# Story 19.2: A stable E2E suite

Status: ready-for-dev

## Story

As the project,
I want the E2E suite to fail only when something is broken,
so that a red run is believed and a hidden defect is not waved through as "flaky".

## Context

Research (two read-only agents, 2026-11-18; last 40 CI runs on `main`, 2026-11-10 → 11-18) found that **none of the known intermittent failures is an application defect** — each is a test that depends on timing or on what other specs did:

- **#685** `form-refusals.spec.ts:80` (`#edit-isbn-error` "not found"; 2 CI occurrences + 1 local): the test fills and presses Enter on the title edit form right after HTMX swaps it in. htmx 2 wires a swapped node (`processNode`) in a settle task after `defaultSettleDelay` (20 ms, `setTimeout`) and marks it `htmx-added` until then; the form has only `hx-post` (no `method`/`action`), so an Enter inside that window is a native GET submit — the page reloads read-only and the error slot is gone.
- **#499** `shelving.spec.ts` "second shelve removes the earlier undo button" (failed on all retries in run 40478700849): its V-codes `V00{61+10r}` / `V00{62+10r}` collide with `loans.spec.ts:113,199` (V0061, V0062), `loan-returns.spec.ts:127` (V0072), `borrower-loans.spec.ts:102` (V0082); V0060 is also shared by `shelving:149` and `loans:34`. When loans ran first, the scan hits an existing label on another title (#585 warning, which contains the code, so the weak regex at :218 passes) and the L-code scan activates batch mode instead of shelving.
- **#554 item 1** (key revocation 500): a real deadlock, **already fixed** by story 13-14 (PR #593: `FORCE INDEX (idx_proposals_api_key)`, deadlock-only retry, `tests/api_key_revocation_lock_scope.rs`); no recurrence since.
- **#554 item 2** `highest-label-codes.spec.ts:36`: asserts the library-wide maximum V-code (`V9998`), which `home.spec.ts:243,309` (`Date.now() % 9000 + 1000`), `title-volume-add.spec.ts:33-35` (V9960–V9999) and the `uniqueVcode` / `uniqueLabel` generators can exceed — Rule #23.
- **#528**: `dewey-code.spec.ts:57` and `classification-fieldset.spec.ts:98` click `main button[type="submit"]` `.last()`, which becomes "Add to series" (`title_detail.html:279–314`) once any series exists; `empty-states.spec.ts:22,37,83` assert the library-wide empty `/series` list. Safe today only through file-name order (D-11-11-6), a convention repeated in eleven spec comments. Hidden series creators that sort earlier: `admin-permanent-delete.spec.ts:22–26`, `accessibility-full.spec.ts:317,341–346`.
- **#674**: V-code bands overlap widely — the random / time-based generators (`home.spec.ts`, `loans.spec.ts:243 uniqueVcode`, `home-search.spec.ts:19 uniqueLabel`) cover V0000–V9999; fixed bands collide (`csp-headers` V7000–7999 vs `volume-value-proposal` / `value-coverage`; `labels-page` V8000–8999 vs `epic15-smoke`, `volume-page-amounts`, `mobile-datatable-cards`; `entity-labels` V9000–9999 vs `epic16-smoke`, `vcode-any-case`, `title-volume-add`, `highest-label-codes`; `admin-health-label-watermarks` V6689/90 inside `title-detail-volumes` V6000–6999). `simulateTyping` types at 100 ms while `static/js/search.js:77` treats only `interKey > threshold` as typing and `templates/pages/home.html:37` hard-codes `data-scanner-threshold="100"` — no margin.
- **Also flaky in CI** (project lead, 2026-11-18: in scope): `volume-value-proposal.spec.ts:134` (`getAttribute` timeout at :62, twice), `title-detail-volumes.spec.ts:22` (`phantomConfirm` not visible), `admin-trash-restore.spec.ts:58` (`toHaveCount(0)` received 1).
- **Flakes are invisible**: `playwright.config.ts` sets `retries: process.env.CI ? 2 : 0`; a test that fails then passes leaves the job green, and the compose log (`_gates.yml:225–228`) is uploaded as an artifact only when the job fails (`:245–251`) — never printed.

## Decisions

- **D1 — a flaky test turns CI red** (project lead, 2026-11-18): `failOnFlakyTests: !!process.env.CI` in `playwright.config.ts`; retries stay (they still report *which* attempt failed). A CI console reporter (`list`) next to `html`, so the failing assertion is in the job log.
- **D2 — the app log is printed** in the job log whenever Playwright does not pass (including a flaky-only failure, now red per D1): a `run`-only step (no new action, pinned-SHA rule untouched) in the `e2e` and `e2e-wizard` jobs of `.github/workflows/_gates.yml`, **before** the stack is stopped (`down -v` destroys the logs): `docker compose … logs --no-color --timestamps mybibli` inside a `::group::`.
- **D3 — `expectHtmxProcessed(locator)`** in `tests/e2e/helpers/htmx.ts`: waits until the top-level node of a swapped fragment no longer has the `htmx-added` class (removed in the same synchronous task as `processNode`). Used before typing into or submitting a form HTMX just swapped in (`form-refusals` `editNewTitle`, `classification-fieldset.spec.ts:40`, every other such site found). Documented in CLAUDE.md "HTMX wait strategies".
- **D4 — `specVcode(specId, seq)`**: one fixed 200-code block per spec file, declared in a typed `VCODE_BLOCKS` map (a typo fails `tsc`; a duplicate block throws at module load), in `tests/e2e/helpers/vcode.ts` next to `specIsbn` / `specLcode`. The offset includes `test.info().repeatEachIndex` and `test.info().retry` so a repetition or a CI retry never reuses its own **V-codes** (other data a spec creates — ISBNs, L-codes, borrower names — is not slot-aware unless the spec derives it from `vcodeSlot()`, as `shelving`'s "second shelve" and `highest-label-codes` do: a `--repeat-each` run of a spec with fixed data needs a stack reset per repetition, and with `failOnFlakyTests` a retry is red anyway); it throws when a block overflows; V0000 is never handed out. Every spec that creates volumes moves to it — hard-coded V-codes, `Date.now()` draws and the `uniqueVcode` / `uniqueLabel` generators go. The top block stays reserved for `highest-label-codes`. Codes become deterministic: a re-run on an un-reset stack needs `./scripts/e2e-reset.sh` (already the rule for a truth run). Literals that are only *message text* (no volume created) may stay. CLAUDE.md "Data isolation" points to `specVcode`.
- **D5 — #528 by stable ids and own scope, no ordering**: the two `.last()` clicks on the title page become `#edit-title-submit` (the id exists, `title_edit_form.html:176`); the library-wide empty `/series` cases leave E2E for a router-level `#[sqlx::test]` on a fresh database (anonymous without CTA, librarian with CTA → `/series/new`, French copy); `empty-states.spec.ts` keeps its own-query search case and gains the French check; every D-11-11-6 ordering comment is removed. Optional guard: a CI grep refusing `button[type="submit"]').last()` in a spec that visits `/title/`.
- **D6 — #554 item 2 asserts on its own codes**: the line's codes are parsed and compared (`>=`, fixed width) to what the spec created; the "never created" comments (`highest-label-codes`, `loans.spec.ts:175`) are rewritten. **#554 item 1** is closed with the 13-14 evidence.
- **D7 — #499**: shelving's codes from its `specVcode` block, the `+10r` arithmetic dropped, the assertions at :210/:218 tightened to the creation copy (`data-feedback-variant="success"` + the attached text), not just the label.
- **D8 — typing margin**: `simulateTyping` types at `TYPING_DELAY_MS = 150`, reads the field's (else `body`'s) `data-scanner-threshold` and throws when the delay is not at least threshold + 30 ms; its comments say the server default is 50 (`src/config.rs:433`) and the home field hard-codes 100.
- **D9 — "really seen" helper (retro v1.28.1 A1)**: `tests/e2e/helpers/visibility.ts` — `seenState(locator)` (effective opacity = product over the element and its ancestors, computed `visibility`, `display:none` ancestor, box size), `effectiveOpacity(locator)`, `expectReallySeen(locator, { minOpacity = 0.5, inViewport = false, timeout })` polling and printing the state on failure. Used in `locations.spec.ts` (#695 case), `shelving.spec.ts` (undo button), `helpers/drawer.ts` `expectDrawerOpen` (drawer + `#nav-scrim`). Documented in CLAUDE.md E2E patterns.
- **D10 — the three other flakes** are diagnosed and fixed at their cause like the others (a real defect found is fixed or filed, never retried away).

## Acceptance Criteria

1. **AC1.** #685, #499, #554 (item 2), #528, #674 and the three other CI flakes are fixed at their cause; each fix is described in the Dev Notes with its evidence. A deterministic reproduction is shown failing before / passing after where one exists (#685: `htmx.config.defaultSettleDelay = 1000`; #499: `loans` + `shelving` with `--workers=1`; #554: V9999 created first).
2. **AC2.** `specVcode` exists, is used by every spec that creates a volume, and a unit-style check (a `tsc`-typed map + a load-time duplicate check, or a grep gate in CI) makes two specs sharing a block or a hard-coded created V-code fail.
3. **AC3.** No spec depends on file order: the `.last()` title-page clicks are gone, the library-wide `/series` emptiness is asserted only in a fresh-database Rust test, the ordering comments are removed.
4. **AC4.** `expectHtmxProcessed` and `expectReallySeen` exist, are documented in CLAUDE.md, and are used at the sites listed in D3 / D9.
5. **AC5.** CI: a test that passes only on retry fails the job (D1), and the app log is printed in the job log when Playwright does not pass (D2), for both E2E jobs.
6. **AC6.** Three consecutive full runs under `CI=true npx playwright test --workers=2` on a fresh stack (`./scripts/e2e-reset.sh` before each) show no flaky and no failed test; plus `--repeat-each` runs of the specs fixed here (≥ 10).
7. **AC7.** #685, #499, #554, #528, #674 are closed by the PR (or #554 item 1 closed with the 13-14 evidence in a comment).

## Tasks / Subtasks

- [ ] T1 — CI: `failOnFlakyTests`, list reporter, app-log steps (AC5, D1, D2)
- [ ] T2 — `helpers/htmx.ts` `expectHtmxProcessed` + #685 fix + other swapped-form sites (AC1, AC4, D3)
- [ ] T3 — `helpers/vcode.ts` `specVcode` + migrate every volume-creating spec + guard (AC2, D4)
- [ ] T4 — #499 shelving (D7), #554 item 2 (D6), comments
- [ ] T5 — #528: stable ids, `empty-states` rework, Rust test for the empty `/series` list, remove ordering comments (AC3, D5)
- [ ] T6 — `simulateTyping` margin (D8)
- [ ] T7 — `helpers/visibility.ts` + uses (AC4, D9)
- [ ] T8 — the three other flakes (D10)
- [ ] T9 — CLAUDE.md (E2E patterns: HTMX wait, really seen, data isolation / `specVcode`, typing)
- [ ] T10 — gates: `npx tsc --noEmit`, flake grep gate, `--repeat-each` runs, three full runs (AC6)

## Dev Notes

- Research reports summarised in Context; file:line evidence there. No application code is expected to change except what a "real defect" finding requires (D10) and the Rust test of D5.
- `tests/e2e/helpers/isbn.ts` holds `specIsbn` / `specLcode` — follow their shape and the spec-id convention (2-character id per spec file).
- `--repeat-each` and CI retries reuse a test's data: D4's offset must include both.
- E2E stack: `./scripts/e2e-reset.sh` (port 8080 on this workstation today), `CI=true npx playwright test --workers=2`. Heavy commands under `mem-guard`, one at a time; up to 2–3 sub-agents (kesh's release is over).
- Couplings (Rule #24): this story touches shared E2E helpers and many specs — stories after it rebase before their E2E gate (named in the epic).

### `specVcode` and its guard (T3, D4, AC2)

- `tests/e2e/helpers/vcode.ts`: 50 blocks of 200 codes; block = `VCODE_BLOCKS[specId]` (a `const` map, so `VcodeSpecId` is the union of its keys and a typo fails `tsc --noEmit`). Block 0 (V0000–V0199) is never assigned — V0000 is refused by the server, and the rest of block 0 gives the specs a code no spec creates (`loans` not-found scan V0199, `catalog-volume` no-title warning V0099). Block 49 (V9800–V9999) is `HC` only.
- Slot = `repeatEachIndex * (project.retries + 1) + retry`, 20 codes per slot, 10 slots per block: in CI (2 retries) `--repeat-each` up to 3 fits; for `--repeat-each 10` (AC6) pass `--retries=0` — an overflow throws with that hint, never wraps.
- How a collision is caught: (1) two ids declaring the same block, a block out of range or the top block given to another id → `checkVcodeBlocks()` throws when the module loads, so every importing spec fails; (2) a hard-coded created V-code or one built by hand (`` `V${…}` ``) → `tests/e2e/vcode-literals.sh` lists every `V\d{4}` literal outside comments in `specs/` and `helpers/` and fails on one not allow-listed (`file:literal`, only message-text uses: V0000, V0099, V0199, V0042 as the example in the refusal, V0001 swallowed by the drawer). Proposed CI wiring next to the flake gate in the `e2e` job: `tests/e2e/vcode-literals.sh`.
- `specVcode` calls `test.info()`: call it inside a test or hook, never at module level.

### References

- Issues #685, #499, #554, #528, #674; epic 19 story 19.2; retro v1.28.1 A1.
- CLAUDE.md — "E2E Test Patterns", Rules #13, #23.

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

- Review pass 1 (two reviewers in parallel: helpers / CI / config, and specs): fixed H1 = M1 (`expectHtmxProcessed` only looked at the node itself; htmx marks only a fragment's top-level nodes, so the nested `#title-form-container form` passed at once — it now checks `closest(".htmx-added")`, callers may pass any node inside the swap; `form-refusals`' manual-form case converted; same wait added to the contributor form, proposal line edits and the contributor slot), M2 (non-V data is not slot-aware — `shelving`'s "second shelve" now derives its ISBNs / L-code from `vcodeSlot()`; D4 states the limit), L1 (`vcode-literals.sh` refuses one `specVcode` id in two files), L3 (`seenState` reports an ambiguous locator instead of "detached"), L4 (app log also printed when the job is cancelled / times out), L5 (`simulateTyping` parses the threshold and refuses an unreadable one), L6 (`seed_librarian` shared in `tests/common/web.rs`), indentation.
- Found by the first `--repeat-each` run (not a review finding): `createBorrower` and `getBorrowerIdByName` read the `/borrowers` list, sorted by name and paginated — once the shared database holds a page of borrowers sorting first, the new row is on page 2 (a latent Rule #23 flake that grows with the suite). Both now use `/borrowers/search` (exact name, fail loud on ambiguity, `borrowerIdByName`); `createBorrower` returns the id; five specs that clicked the list link go to `/borrower/{id}`.
- Review pass 2 (one reviewer): fixed M1 (four more specs still read the paginated `/borrowers` list — `borrower-loans`, `mobile-datatable-cards`, `csp-headers`, `borrowers`; now by id, the one list-surface check uses a name sorting first; `borrowers.spec`'s detail and edit cases create their own borrower instead of using "the first row", which edited another spec's borrower — Rule #23), L1 (`createBorrower` waits for the POST answer and the reload, not a no-op URL wait), L2 (`loan-returns`' swapped Return button waits for `expectHtmxProcessed`), L3 (the gate reads ids passed through `const SPEC… = "XX"` and names both files), L4 (`home-search`'s counter + clock L-code replaced by `specLcode(7, 500 + vcodeSlot())`), nit (`simulateTyping` falls back to `<body>` on an empty attribute).
- Review pass 3 (one reviewer, sweep of the whole suite): fixed M1 (`series.spec` read its renamed series in the paginated `/series` list — the detail page is its scope), M2 (`tome-number` edit form: the #685 wait), M3 (`session-inactivity-timeout` drops the instance's session timeout to 9 s while the other worker runs; `admin-system-settings` saves global settings — both now run in their own Playwright projects chained after all other tests), L1 (modal Confirm clicks in `helpers/loans.ts` wait for `expectHtmxProcessed`), L2 (`shelving` undo button), L5 (stale comments); found by the partial re-proof: `borrowers.spec`'s edit case matched its own heading ("BW-Edit Borrower" contains "Edit borrower" — renamed, link by role, false-green `if` removed) and `home-search`'s "/" case clicked the centre of `<body>`, a title link on a grown library (now blurs without clicking). Deferred Lows (proposal list reads, auto-proposed L-code race, false-green branches, gate quoting, non-V data slot-awareness) → #713.
- Review pass 4 (one reviewer, the pass-3 diff only): no defect in the code. Documented the two chained-project costs (run one alone with `--no-deps`; a `chromium` failure skips them — accepted, the job is red anyway); narrowed the CLAUDE.md rule to settings that disturb other workers (time zone, provider timeouts, bulk delay restore their value and stay); `borrowers.spec`'s save button targeted by its form action instead of "the last submit button".
- Partial proof before pass 2 (stopped to apply it): `--repeat-each 10 --retries=0` over the seven specs without fixed data, one database: 150 passed; four reset-then-run cycles of `loans` + `shelving` + `dewey-code`: 20/20 each.

### File List
