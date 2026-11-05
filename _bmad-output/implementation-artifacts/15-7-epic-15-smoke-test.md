# Story 15.7: Epic 15 smoke test (Foundation Rule #7)

Status: done

Covers: Foundation Rule #7. Epic 15; ships in v1.25.0.

## Story

As the project,
I want one end-to-end journey from a blank browser through the epic's core flow,
so that the epic is proven done.

## Acceptance Criteria

1. **AC1** — From a blank browser (no injected cookie), the test chains: admin login through `loginAs` → create a skill in Admin → Skills → an MCP client lists it as a prompt (`prompts/list`) and reads it with `get_skill` → the agent files a series modification (`propose_series_update`) and a volume value proposal (`propose_volume_value`) → the librarian (real `loginAs`) accepts both → the series shows the new total and `/stats/value` shows the coverage.
2. **AC2** — No `waitForTimeout`; green under `CI=true npx playwright test --workers=2`; if this test fails, epic 15 is not done.

## Design (dev, 2026-11-05)

- The series is created by an accepted agent proposal holding the scanned title and one expected tome; the modification closes it at 1 and removes that tome (the 15-4 total rule in action). The volume is found with `list_volumes_to_value` filtered on that series (15-6), valued in the library's `default_currency` (15-5); the coverage row of the series reads 1 / 1, 100 %.
- Shared-DB safety: unique skill (`0e5-…`, sorted first in the instructions' bounded list) and series names, assertions only on rows the test creates. Spec ID "E5"; V-code band V8000–V8499. Cleanup in `finally`: the skill is deactivated, the key revoked.

## Tasks

- [x] `tests/e2e/specs/journeys/epic15-smoke.spec.ts`.

## Dev Agent Record

### File List

- tests/e2e/specs/journeys/epic15-smoke.spec.ts (new)

### Completion Notes

Local targeted run (CI shape, 2 workers) green; CI green on PR #633 (all four gates).
