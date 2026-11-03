# Story 13.12: Epic 13 smoke test (Foundation Rule #7)

Status: review

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

- [x] T1 — new spec `tests/e2e/specs/journeys/epic13-smoke.spec.ts` (choose an unused specId, e.g. `E3`)
- [x] T2 — reuse helpers (auth, isbn, scanner, loans/scan helpers, locations helper `createChildLocation`, api-keys, mcp); add a mock case only if strictly needed
- [ ] T3 — gates: tsc, flake grep, run the spec alone and with the full suite at 2 workers (tsc, flake grep and the spec alone done; the full-suite run at 2 workers is left to the orchestrator)

## Completion Notes

- **Spec:** `tests/e2e/specs/journeys/epic13-smoke.spec.ts` (spec ID `E3`), one test chaining 13-6 (two-level tree, full path on the title page), 13-1 (V-code submitted twice back to back → one "attached" + one "shelved" entry, no error, banner "1 vol"), 13-10 (title without the marker, "Tome 3 (detected)"), 13-7 ("Added on"; home "added since today" + sort by date added lists the title), 13-8 (move through the modal, target ISBN as a scanner burst; second title lists the volume), 13-9 (admin creates a label and a `propose` key; MCP `list_labels` then `propose_label`; librarian opens the proposal from `/proposals?status=pending` and accepts; volume page shows the label chip, `/labels/{id}` lists the volume). Key revoked in `afterEach` from a fresh context.
- **Blank browser / logins:** `clearCookies()` + `loginAs` only (librarian → admin → librarian); no cookie injection.
- **Unique data, re-runnable on a non-reset stack:** per-run seconds stamp. Marker ISBN = `specIsbn("TN", 91000–99999)` — the existing 13-10 mock case (seq ≥ 1000) in a band `tome-number.spec.ts` (1000–90999) never draws; second ISBN `specIsbn("E3", RUN)`; V-code band V0360–V0439 (no other spec hardcodes/derives it, except the suite-wide random generators of loans/home specs that every spec already lives with); L-codes in bucket 8 (`specLcode(8, …)`, unused).
- **Reorder vs. AC2:** the location tree is created **before** the scans so the L-code is scanned right after the V-code (an L-code shelves the session's last volume label, which a later ISBN scan clears); all steps are otherwise as specified.
- **Not covered:** 13-4 HTML description (optional per AC2.2): its only mock case is a fixed Google Books ISBN owned by `html-description.spec.ts`; no mock case added.
- **DRY:** the label-creation step of `mcp-label-proposal.spec.ts` moved to a new helper `tests/e2e/helpers/labels.ts::createLabel`, used by both specs.
- **Gates:** `npx tsc --noEmit` clean; flake grep clean; on a fresh stack (`E2E_HOST_PORT=18080 ./scripts/e2e-reset.sh`) `CI=true npx playwright test epic13-smoke mcp-label-proposal --workers=2` → 2 passed; the smoke re-run twice more on the non-reset stack → passed both times. The full suite at 2 workers was left to the orchestrator's final gate (per the dev brief).
- No product code changed; no bug found.

### Review Findings

Round 1 (2026-11-03) — 1 Medium, 5 Low, all patched:

- [x] [Review][Patch] M1 — the "added since" step did not prove filtering (the date sort put the title first anyway). Now: full-text search on the title, today → the title is listed, tomorrow → it is not. Decision (coordinator): the filter is not exercised on an exact-ISBN query, because on the web an exact ISBN/V-code hit means "show me this book" and bypasses the browse filters by design (13-7 review, L5, dismissed; the MCP applies `added_since`). The smoke had surfaced it: `/?q=<isbn>&added_since=<tomorrow>` still lists the title. The title text ("Le Mystère des tomes", the fixed mock title) is shared across runs, so the search is sorted by date added, newest first, to keep the title on page 1. The tomorrow check proves the filter, not the sort.
- [x] [Review][Patch] L2 — the double read is now proved directly: a `page.on("request")` listener checks that exactly one `POST /catalog/scan` carries the V-code.
- [x] [Review][Patch] L3 — after the move, the old title page no longer lists the V-code.
- [x] [Review][Patch] L4 — the key label is fixed before `createApiKey` (new optional `{ exactLabel }` in `helpers/api-keys.ts`, backward compatible); `afterEach` revokes it only when its row exists.
- [x] [Review][Patch] L5 — wider draws and an accurate comment: V-code V2000–V3999 from the clock in seconds (`% 2000`, no repeat within 33 min; only the suite-wide random generators of loans/home-search/home can reach it); the L-code pair is drawn in bucket 8 among codes `/locations` does not list yet; second ISBN on seconds `% 100000`; location names carry the full seconds stamp.
- [x] [Review][Patch] L6 — `utcDay()` documents that the server cuts days in UTC (#602) and that a run straddling midnight UTC is an accepted risk.

Gates after round 1: tsc and flake grep clean; fresh stack, smoke alone twice → passed; smoke + home + date-added + volume-move + mcp-label-proposal at `CI=true --workers=2 --retries=0` → 21 passed.
