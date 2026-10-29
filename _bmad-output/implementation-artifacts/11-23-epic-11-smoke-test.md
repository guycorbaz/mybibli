# Story 11.23: Epic 11 smoke test (Foundation Rule #7)

Status: done

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: Foundation Rule #7 (FR126–FR147 end to end). Epic 11 — twenty-third story. Test-only story: no production code changes expected.

## Story

As a maintainer,
I want one test that walks the epic's core journey from a blank browser,
so that I know the pieces work together.

## Acceptance Criteria

1. **AC1 — blank browser, real logins.** The test starts from a browser context with no injected cookie; every login goes through `loginAs` (`tests/e2e/helpers/auth.ts`), never `DEV_SESSION_COOKIE`.
2. **AC2 — a `propose` key from the admin UI.** An admin creates a `propose` key in Admin → API keys (`helpers/api-keys.ts::createApiKey`, through `helpers/proposals.ts::newProposeKey`); the key is revoked in `afterEach` from a fresh browser context so the run leaves no active key, even on failure.
3. **AC3 — a scripted MCP client, both protocol revisions (NFR42).** A fresh Playwright request context (no browser cookies) plays the agent over plain JSON-RPC (`helpers/mcp.ts`): `initialize` on revision `2025-11-25` then `tools/list` (exactly `MCP_TOOLS`); `search_titles` finds each of the two scanned titles by ISBN — one call per revision (`2025-11-25` header-only, `2026-07-28` session-less with `_meta`); `propose_series_create` files a closed series with the two titles (positions 1 and 2) and one expected tome (position 3, with an ISBN).
4. **AC4 — the librarian sees it waiting (FR143, UX-DR32/33).** On the next navigation the nav entry `nav a[href="/proposals"]` carries the badge (`[data-proposals-badge]` text matching the exact count in its `aria-label`) and the home page shows the `#filter-tag-pending-proposals` pill with the same count. The pending count is library-wide and parallel specs file proposals, so the assertion is relative (≥ 1, badge = aria = pill) instead of the epic's literal "shows 1"; the nav entry leads to `/proposals`, which lists this proposal.
5. **AC5 — edit and accept (FR139, FR140).** The librarian opens the proposal, moves the second title from position 2 to position 4 with the line's inline form, accepts through the UX-DR8 modal; the proposal is `accepted`.
6. **AC6 — the catalog shows the result (FR142, FR147).** The series page shows the first title at position 1, the second at position 4 and the expected tome at position 3 (cross-checked with `get_series`). The librarian sends the expected tome to the wish list, scans its ISBN on `/catalog` (the "expected tome found" feedback and the wish-list auto-link appear), and the series page then shows position 3 owned and no expected tome left.
7. **AC7 — isolation and determinism.** Fresh spec id `EK` with a per-run stamp in ISBN sequences, a stamped series name (a re-run on a non-reset stack meets none of an earlier run's titles, tomes or wish-list items, and nothing is rejected, so the 90-day resubmission delay of 11-15 never applies); no `waitForTimeout`; i18n-aware regexes; the file name (`series-…`) sorts well after `classification-fieldset`, `dewey-code`, `empty-states` (D-11-11-6, #528).
8. **AC8 — green under CI shape.** The spec passes under `CI=true npx playwright test --workers=2` in the full suite (Foundation Rule #13). If it fails, epic 11 is not done.

## Tasks / Subtasks

- [x] T1 — DRY: move the badge/pill reader `shownCount` of `proposals-access.spec.ts` into `helpers/proposals.ts` (`shownPendingCount`) and use it from both specs.
- [x] T2 — `tests/e2e/specs/journeys/series-epic11-smoke.spec.ts` (AC1–AC7).
- [x] T3 — Gates: waitForTimeout grep, `tsc --noEmit`, the new spec + `proposals-access.spec.ts` targeted, then the full suite once on a reset stack before the push.
- [x] T4 — Decisions log, sprint-status.

## Dev Notes

- Reuse: `helpers/proposals.ts` (`newProposeKey`, `revokeProposeKey`, `scanSettledTitle`, `acceptFromDetail`), `helpers/mcp.ts` (`mcpPost`, `mcpCallTool`, `MCP_TOOLS`, `MCP_HEADERS`). The inline line-edit pattern comes from `series-proposal-edit.spec.ts` (`EDIT` / `SAVE` / `UPDATED` regexes, `[data-edit-slot] form`, label `Position`). Expected-tome wish/scan from `series-expected-tomes.spec.ts` (`#series-feedback`, `[data-wishlist-autolink]`, `a[role="gridcell"]` aria-label `" N — "`).
- `scanSettledTitle` waits for the background metadata fetch (it bumps `titles.version`) so the proposal is not filed on a version that is about to change (stale at acceptance otherwise).
- The 2026-07-28 call shape is the one of `mcp-endpoint.spec.ts` (headers `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name`; `_meta` with `io.modelcontextprotocol/protocolVersion`, `clientInfo`, `clientCapabilities`).
- The `/proposals` list is newest first; the just-filed proposal is on the first page.
- E2E instance renders French; no language switch is needed (if one were, `?lang=xx`, not the toggle).
- Known flake elsewhere in the suite: `shelving.spec.ts` "second shelve removes the earlier undo button" (passes on retry).

### References

- `_bmad-output/planning-artifacts/epics.md` § Story 11.23, NFR42.
- Stories 11-6 (endpoint), 11-11 (expected tomes), 11-12 (series proposals), 11-17 (accept), 11-19 (edit), 11-21 (badge, pill).

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

- Targeted run on the 18080 stack (`--workers=2`, with `proposals-access.spec.ts`): smoke green; a re-run on the same non-reset stack green (per-run stamp). `proposals-access` timed out in `scanSettledTitle` on that non-reset stack — its fixed `specIsbn("NB", 1)` already existed from an earlier run (pre-existing, fixed ISBN; green on a reset stack).

### Completion Notes List

- D-11-23-1..4 (test design), D-11-23-5 **[PRODUCT]** (CI trigger, decided by the project lead, separate commit).
- No production code changed.
- Code review round 1 (combined layer): no Medium+; six Lows fixed, one deferred as #550 (D-11-23-6).

### File List

- `tests/e2e/specs/journeys/series-epic11-smoke.spec.ts` (new, spec id `EK`).
- `tests/e2e/helpers/proposals.ts` — `NAV`, `shownPendingCount` (moved from the 11-21 spec).
- `tests/e2e/specs/journeys/proposals-access.spec.ts` — uses the helper.
- `.github/workflows/ci.yml` — push trigger limited to `main` (separate commit, D-11-23-5).
- `_bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md`, `sprint-status.yaml`.
