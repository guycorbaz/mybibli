# E2E Flaky Audit — Story 5-1 Task 1

**Date:** 2026-04-05
**Auditor:** Dev agent (Amelia via `/bmad-dev-story`)
**Context:** Story 5-1 E2E Stabilization
**Docker stack:** `tests/e2e/docker-compose.test.yml` fresh `up -d` for each run

## Baseline Measurements

| Run | Mode | Passed | Failed | Total | Duration |
|---|---|---|---|---|---|
| 1 | `fullyParallel: true`, workers=default | 73 | 47 | 120 | 54s |
| 2 | `fullyParallel: false`, workers=1 | 79 | 41 | 120 | 9m 30s |
| 3 | Serial + loginAs helper migrations (final story 5-1 state) | **84** | **36** | 120 | 6m 30s |

**Net improvement from story 5-1: +11 tests recovered** (73 → 84).
- +6 from serial mode (removes intra-file race conditions)
- +5 from loginAs() helper fixing credential drift in media-type-scanning.spec.ts `beforeEach` (tests used stale `admin123` password instead of seed `admin`)

**Net improvement from serial mode:** +6 tests passing. Retro's ~6 estimate matched this delta, not the total failure count.

## Root Cause Analysis

### Finding 1: Shared ISBN constant `9782070360246` across 11+ spec files

**Impact:** ~35 of 41 remaining failures
**Files affected:**
- `login-smoke.spec.ts`
- `cross-cutting.spec.ts`
- `catalog-title.spec.ts`
- `catalog-metadata.spec.ts`
- `catalog-volume.spec.ts`
- `epic2-smoke.spec.ts`
- `media-type-scanning.spec.ts` (line 62)
- `cover-image.spec.ts` (line 46)
- `borrower-loans.spec.ts`
- `shelving.spec.ts`
- `metadata-editing.spec.ts`

**Mechanism:** Tests scan ISBN and assert `data-feedback-variant="success"` (newly created). When any prior test in the same DB lifetime already scanned the same ISBN, the app correctly returns `variant="info"` (already exists), and the test fails.

**Representative error:**
```
Expected: "success"
Received: "info"
at catalog-title.spec.ts:34
```

**Serial mode does NOT fix this** because the app's DB persists across spec files; alphabetical spec ordering deterministically creates the title in the first spec that runs, so all others see "info".

### Finding 2: Smoke tests that use real login fail URL assertion

**Impact:** ~4 failures (`metadata-editing.spec.ts:104`, `media-type-scanning.spec.ts:92`, etc.)
**Error:** `expect(page).toHaveURL("/") — received "http://localhost:8080/login"`

**Mechanism:** Tests perform a real login via the form, then expect redirect to `/`. Either the login submit is failing silently (wrong selectors / wrong credentials) or the redirect target differs from `/`. Needs investigation — could be resolved by the new `loginAs()` helper in Task 5.

### Finding 3: Specific interaction flakes (remaining ~2)

- `shelving.spec.ts:168` — location page shows 0 volumes after shelving (timing or state)
- Accessibility tests timing out — likely secondary effects of the above

## Categorization (per story AC2 illustrative buckets)

| Category | Count | Status |
|---|---|---|
| Data isolation (shared seed ISBN) | ~35 | **Deferred to story 5-1b** — requires unique-ISBN-per-spec architecture or DB reset between specs |
| Smoke test login failures | ~4 | **In scope for story 5-1** — Task 5 `loginAs()` helper should resolve |
| HTMX timing / specific interaction | ~2 | **In scope for story 5-1** — Task 2 / Task 4 fixes |

## Decision for Story 5-1 Scope

Given the 47-vs-6 discrepancy with the retro estimate, Guy (project lead) agreed on **Option C: Hybrid**:

1. **Story 5-1 ships minimum viable stabilization:**
   - Apply `fullyParallel: false` (recovers 6 tests, immediate improvement)
   - Implement real-login helper (Task 5) → should resolve Finding 2
   - Document E2E patterns in CLAUDE.md (Task 6)
   - Verify `cargo sqlx prepare --check` (Task 7)
   - Final run with serial mode as new baseline (expect ~79 passing + whatever Task 5 recovers)

2. **New story 5-1b created** to own the deep fix:
   - Per-spec unique ISBN strategy (generator + mock server expansion)
   - OR DB reset between spec files (globalSetup hook)
   - Reintroduce `fullyParallel: true` once data isolation is architectural, not accidental
   - Will become the new blocker for Epic 5 feature stories 5-2..5-8 (replacing 5-1 once 5-1 ships)

## Outcome

This audit file is **kept in-tree** (not deleted per original Task 8 instruction) as evidence for story 5-1b's scope. It will be deleted when 5-1b is done and the suite reaches 100% green under parallel mode.
