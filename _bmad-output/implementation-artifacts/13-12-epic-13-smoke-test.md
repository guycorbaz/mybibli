# Story 13.12: Epic 13 smoke test (Foundation Rule #7)

Status: ready-for-dev

Covers: Foundation Rule #7 (epic 13 core journey, FR153–FR160 end to end). Epic 13; ships in v1.23.0.

## Story

As the project,
I want one end-to-end journey from a blank browser through the epic's core flow,
so that the epic is proven done.

## Acceptance Criteria

1. **AC1 — blank browser.** The test starts from a blank context (`page.context().clearCookies()`), logs in through `loginAs(page, "librarian")` (or admin where an admin-only step needs it — an API key is created by an admin; use a second login in the same spec rather than cookie injection).
2. **AC2 — journey.** In one test, chained:
   1. On `/catalog`, scan an ISBN whose mock provider title carries a tome marker (the 13-10 mock case `specIsbn("TN", ≥1000)` or a new smoke-specific ISBN wired the same way), then its V-code label **twice in a row** within the duplicate window → exactly one volume, no error entry (13-1).
   2. Open the title page: the title shows **without** the marker and with "Tome N (detected)"; it shows "Added on" (13-7) and its description renders as plain paragraphs if the mock provides an HTML description (13-4, optional if no mock case — do not add one only for the smoke).
   3. Create a 2-level location tree, scan the location's L-code to shelve the volume; the title page shows the **full path** of the volume's location (13-6).
   4. On the home page, filter "added since" today → the title is listed (13-7).
   5. Scan a second ISBN (another title); open the first title's volume page and **move** the volume to the second title through the modal (13-8); the second title page lists the volume.
   6. As admin: create a label "Smoke doubtful <stamp>", create a `propose` API key; a scripted MCP client (`tests/e2e/helpers/mcp.ts`) calls `list_labels`, then `propose_label` on the volume with that label and a source; back as librarian (or admin), open the proposals list, accept the label proposal; the volume page shows the label and `/labels/{id}` lists the volume (13-9). Revoke the key at the end (helpers/api-keys.ts).
3. **AC3 — hygiene.** Unique data (`specIsbn` with a new specId, unique V/L codes in an unused range, unique names), no `waitForTimeout`, no cookie injection; green under `CI=true npx playwright test --workers=2` (Rule #13). If this test fails, the epic is not done.

## Tasks

- [ ] T1 — new spec `tests/e2e/specs/journeys/epic13-smoke.spec.ts` (choose an unused specId, e.g. `E3`)
- [ ] T2 — reuse helpers (auth, isbn, scanner, loans/scan helpers, locations helper `createChildLocation`, api-keys, mcp); add a mock case only if strictly needed
- [ ] T3 — gates: tsc, flake grep, run the spec alone and with the full suite at 2 workers
