# Story 14.3: Epic 14 smoke test (Foundation Rule #7)

Status: ready-for-dev

GitHub issue: #547. Covers: Foundation Rule #7. Epic 14; ships in v1.24.0.

## Story

As the project,
I want one end-to-end journey from a blank browser through the new navigation,
so that the epic is proven done.

## Acceptance Criteria

1. **AC1 — journey.** A new spec `tests/e2e/specs/journeys/epic14-smoke.spec.ts` starts from a blank browser (no injected cookie) and chains, at 1280 px: login through `loginAs` → navigate from the sidebar to Catalog, Loans and Proposals (the Proposals badge is visible — create a pending proposal through the MCP helpers as other specs do, and clean it up) → collapse the sidebar → reload: still collapsed → navigate by an icon (accessible name) → expand; then at 390 px: open the drawer → navigate → the drawer is closed on the new page → log out from the drawer → the login page is shown.
2. **AC2 — hygiene.** Unique specId (check free ids), data isolated, no `waitForTimeout`, selectors by role / name (i18n-aware regexes).
3. **AC3 — gate.** Green under `CI=true npx playwright test --workers=2` in the full suite (Foundation Rule #13). If this test fails, the epic is not done.
