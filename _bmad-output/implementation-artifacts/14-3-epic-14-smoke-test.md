# Story 14.3: Epic 14 smoke test (Foundation Rule #7)

Status: review

GitHub issue: #547. Covers: Foundation Rule #7. Epic 14; ships in v1.24.0.

## Story

As the project,
I want one end-to-end journey from a blank browser through the new navigation,
so that the epic is proven done.

## Acceptance Criteria

1. **AC1 — journey.** A new spec `tests/e2e/specs/journeys/epic14-smoke.spec.ts` starts from a blank browser (no injected cookie) and chains, at 1280 px: login through `loginAs` → navigate from the sidebar to Catalog, Loans and Proposals (the Proposals badge is visible — create a pending proposal through the MCP helpers as other specs do, and clean it up) → collapse the sidebar → reload: still collapsed → navigate by an icon (accessible name) → expand; then at 390 px: open the drawer → navigate → the drawer is closed on the new page → log out from the drawer → the login page is shown.
2. **AC2 — hygiene.** Unique specId (check free ids), data isolated, no `waitForTimeout`, selectors by role / name (i18n-aware regexes).
3. **AC3 — gate.** Green under `CI=true npx playwright test --workers=2` in the full suite (Foundation Rule #13). If this test fails, the epic is not done.

## Completion Notes (2026-11-04)

**Spec.** `tests/e2e/specs/journeys/epic14-smoke.spec.ts`, spec ID `E4` (free: no `specIsbn("E4"` nor `SPEC = "E4"` anywhere in `tests/e2e/`). One test.

**Setup** runs in its own browser context so the journey's page stays blank: `newProposeKey` (admin), `scanSettledTitle` (librarian, `specIsbn("E4", seconds % 100000)` — a re-run on a non-reset stack scans a new title), then a scripted MCP client files one `propose_title_metadata` on it. **Cleanup** (`finally`): `revokeProposeKey` — revoking the key turns its pending proposal stale, so nothing stays pending and no equivalence block is left.

**Journey** (blank context, asserted with `cookies() == []`): at 1280 px `loginAs(librarian)` → sidebar Catalog (`aria-current`) → Loans → Proposals, reached by its accessible name with the pending count (`Proposals, N pending`, four locales), badge text `\d+|99+` → collapse (`aria-expanded="false"`, width 64 px) → reload: still collapsed, badge still visible → Series by its icon (accessible name, label visually hidden) → expand (224 px); at 390 px the drawer is closed → open (`openDrawer`) → Locations from the `Menu` dialog → new page shows it closed (`expectDrawerClosed`) → open, Log out → `/` → the anonymous drawer has no Log out and offers Log in → `/login`, username field and submit shown. Note: logout redirects to `/` (unchanged behaviour), so the login page is reached through the anonymous drawer's Log in entry.

**Hygiene.** Selectors by role and accessible name with en/fr/de/it regexes (the "Main navigation" landmark name is not localized); `helpers/drawer.ts`, `helpers/proposals.ts`, `helpers/mcp.ts`, `helpers/auth.ts` reused, nothing duplicated; no `waitForTimeout`.

**Gates.**
- `npx tsc --noEmit`: clean.
- `waitForTimeout` grep gate: clean.
- New spec alone on a freshly reset stack (`CI=true`, `--workers=2`): 1 passed; `--repeat-each=2` on the same stack: 2 passed.
- Full suite on a freshly reset stack, `CI=true npx playwright test --workers=2`: 443 passed, 2 skipped (pre-existing skips), 0 failed — 445 tests in 105 files (3.1 min).

## File List

- `tests/e2e/specs/journeys/epic14-smoke.spec.ts` (new)
- `_bmad-output/implementation-artifacts/14-3-epic-14-smoke-test.md`
