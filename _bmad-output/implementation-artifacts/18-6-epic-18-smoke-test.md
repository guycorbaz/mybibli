# Story 18.6: Epic 18 smoke test (Foundation Rule #7)

Status: done

No GitHub issue. Covers: Foundation Rule #7. Epic 18; ships in v1.28.0. Sprint-status key: `18-6-epic-18-smoke-test`.

## Story

As the project,
I want one end-to-end journey from a blank browser over what the epic touched,
so that the epic is proven done.

## Acceptance Criteria

1. **AC1.** From a blank browser (no injected cookie, `loginAs`), the test chains: the dashboard with its chart (18-1) → a series page where an action fails and its message is shown (18-5) → the series created by the spec read back through `GET /api/v1/series` (18-4) → an MCP `initialize` answered (18-3).
2. **AC2.** No `waitForTimeout`; assertions only on the spec's own rows (Rule #23); green under `CI=true npx playwright test --workers=2`. If this test fails, the epic is not done.

## Decisions

- **D1 — a title first.** The dashboard donut renders only with data (`stats_by_genre.has_data()`); the spec scans its own ISBN (`specIsbn("E8", random)`) so it never depends on another spec having run.
- **D2 — Chart.js checked by version.** `window.Chart.version === "4.5.1"` (the vendored UMD sets the global) plus a drawn canvas of non-zero size: proves the upgraded file is the one served and that it renders.
- **D3 — two series.** One is deleted from a second tab to reproduce #526 on the stale page (server's not-found copy, no generic "Server error", focus on Delete); the other is read back through the API, and the deleted one is absent.
- **D4 — 18-2 has no journey.** Pinned build images are the image the E2E stack runs; nothing user-visible to assert.
- **D5 — file name.** It creates series, so `epic18-smoke` must sort after `classification-fieldset`, `dewey-code`, `empty-states` (#528) — it does.

## Tasks / Subtasks

- [x] T1 — `tests/e2e/specs/journeys/epic18-smoke.spec.ts` (AC1, AC2)
- [x] T2 — gates

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (`claude-opus-5-5`)

### Completion Notes List

- Developed on top of the 18-5 branch while its CI ran (exception to Rule #18 granted by the project lead on 2026-11-12; no push before 18-5 merged), then rebased on `main`.
- Targeted run against the 18-5 stack: 1 passed (3.4 s).
- **Full gates after the rebase on `main` (18-5 merged):** E2E on a fresh stack, `CI=true --workers=2` — **479 passed, 2 skipped, 1 flaky** (`connection-lost-overlay.spec.ts:72`, green on retry, untouched code); `tsc --noEmit` green. No Rust change → the Rust suite of 18-5 (2728 / 0) stands.
- Review inline (one spec file): selectors mirror `series-feedback-region` and `series-api`; Rule #23 holds (own title, own series, own keys revoked in `finally`); no `waitForTimeout`. No finding.
- Keys (read + propose) are revoked in `finally`; the MCP agent context is disposed.

### File List

- `tests/e2e/specs/journeys/epic18-smoke.spec.ts` (new)
- `_bmad-output/implementation-artifacts/18-6-epic-18-smoke-test.md`, `sprint-status.yaml`
