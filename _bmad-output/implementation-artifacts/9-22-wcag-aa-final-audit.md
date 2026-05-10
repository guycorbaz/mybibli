# Story 9.22: WCAG 2.2 AA — final audit + axe-core full coverage

Status: ready-for-dev

## Story

As the project maintainer,
I want every page to pass WCAG 2.2 AA via automated axe-core checks in CI plus verified manual contrast/keyboard audits,
so that the accessibility commitment from the project brief is closed end-to-end and regressions are caught on every PR.

## ⚠️ Existing-code reality check

Status of `main` as of 2026-05-10 (post 9-21 close):

- **`@axe-core/playwright` already installed** (`tests/e2e/package.json:dep`).
- **Helper `tests/e2e/helpers/accessibility.ts` exists** with a basic `checkAccessibility(page)` that runs default axe-core rules and asserts zero violations.
- **Existing spot tests**: `catalog-title.spec.ts` and `catalog-volume.spec.ts` already invoke axe on `/catalog` (defaults, not WCAG-2.2-AA-tagged). These spot tests are KEPT.
- **Spec calls for a NEW comprehensive spec** at `tests/e2e/specs/accessibility-full.spec.ts` (top-level `specs/`, not `specs/journeys/`) that iterates ~14 URLs with `runOnly: ['wcag2a', 'wcag2aa', 'wcag22aa']`.
- **Manual audit doc** at `docs/accessibility-audit.md` — keyboard nav checklist + color contrast table + screen-reader smoke-test notes. NEW file.
- **Risk**: axe-core may surface real violations across 14 URLs. The pragmatic plan is:
  - If 0 violations: ship as-is.
  - If 1-3 violations: triage and either fix inline OR file as `type:bug` deferred.
  - If many violations: ship the SPEC as a "discovery report" with violations filed as separate `type:bug` issues; the comprehensive-spec-passes goal becomes a follow-up story.

## Acceptance Criteria

1. **AC1 — NEW spec `tests/e2e/specs/accessibility-full.spec.ts`** (~150 LOC):
   - Iterates over the URL list: `/`, `/catalog`, `/loans`, `/borrowers`, `/series`, `/locations`, `/login`, `/admin?tab=health`, `/admin?tab=users`, `/admin?tab=reference_data`, `/admin?tab=trash`, `/admin?tab=system`.
   - For each URL: navigate, run `new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze()`, assert zero violations.
   - Login as appropriate role per URL (anonymous for `/`, `/catalog`, `/login`; librarian for `/loans`, `/borrowers`, `/series`, `/locations`; admin for `/admin*`).
   - On violation: fail with rule-id + target selector + element name (axe's default error format does this).
   - **DEVIATION FROM ORIGINAL SPEC**: skipping `/title/:id`, `/borrower/:id`, `/contributor/:id`, `/series/:id`, `/setup` from the spec's URL list — these need fixture data to render meaningfully (a borrower id, a setup-active state). Defer detailed-page audits to a follow-up issue.

2. **AC2 — Manual audit doc** `docs/accessibility-audit.md` (NEW):
   - Section 1: Keyboard navigation checklist (skip-link, focus visible, focus order, modal/dialog focus traps, scan-field focus retention).
   - Section 2: Color contrast table — light + dark themes, foreground/background hex per text role, ratio ≥ 4.5:1 (normal) / 3:1 (large) verified manually using browser dev tools.
   - Section 3: Screen-reader smoke-test notes (one critical journey: cataloging a title) — what landmarks announce as, what feedback entries sound like.
   - Section 4: Discovered axe violations — listed with rule-id, page, severity. Either filed as separate `type:bug` issues or in-scope-fixed.

3. **AC3 — Discovered violations triage**:
   - If 0 violations: ship the spec; close the story; no GH issues.
   - If violations found: file each as `type:bug` GH issue with the rule-id and failing-target. Don't block the story on fixes — the spec-runs-clean is the new contract.

4. **AC4 — CI integration**: the new spec is part of the existing `e2e` job (Playwright config picks up `tests/e2e/specs/**/*.spec.ts`). No CI workflow changes needed.

5. **AC5 — Foundation Rule #12 LOC**:
   - `tests/e2e/specs/accessibility-full.spec.ts`: NEW ~150 LOC.
   - `docs/accessibility-audit.md`: NEW ~150 LOC.

6. **AC6 — Local Testing Before Push**:
   - `cargo check` clean.
   - `cargo clippy --all-targets -- -D warnings` clean.
   - The new accessibility-full spec passes (0 violations) OR violations are documented in audit doc + filed as deferred issues.
   - Full E2E lane green (existing tests not regressed).

7. **AC7 — Draft PR + CI gate**: Foundation Rule #15 + #18.

## Tasks / Subtasks

- [ ] **Task 1 — Create `tests/e2e/specs/accessibility-full.spec.ts`**
  - [ ] Iterate the 12-URL list; per-URL login + axe scan with WCAG 2.2 AA tags.
  - [ ] Assert zero violations OR collect them into a structured list for triage.

- [ ] **Task 2 — Create `docs/accessibility-audit.md`**
  - [ ] Skeleton with 4 sections (keyboard / contrast / SR / discovered violations).
  - [ ] Manual checklist filled in with results from axe + browser-dev-tool measurements.

- [ ] **Task 3 — Run + triage**
  - [ ] `npx playwright test specs/accessibility-full.spec.ts`.
  - [ ] If violations: classify each as fixable-now vs deferred GH issue.
  - [ ] Fix simple ones inline (missing alt-text, color contrast tweaks).
  - [ ] File deferred ones as `type:bug`.

- [ ] **Task 4 — Local gate + push + draft PR**
  - [ ] cargo check + clippy clean.
  - [ ] Full E2E lane green.
  - [ ] Push, open draft PR, wait CI green per Rule #18.

## Dev Notes

### Why a spec-discovery approach over a fix-everything story

mybibli has 21 prior Epic-9 stories that emphasized accessibility (CSP, aria-labels, focus management, role-aware nav, tooltips with aria-describedby). The codebase is likely close to WCAG 2.2 AA already. This story EXPOSES whatever residual gaps exist via axe-core full coverage AND establishes the regression gate for future PRs.

If the audit reveals 0 violations: great, ship the gate.
If the audit reveals N violations: each one becomes its own focused story, prioritized via Severity. The current story ships the discovery + audit doc; the fixes are tracked as separate issues.

### Why skip /title/:id, /borrower/:id, /contributor/:id, /series/:id from the URL list

Those routes need fixture data (an id) to render. Setting up DB seeds + IDs across 4 routes adds a layer of test plumbing that's orthogonal to the WCAG audit. Defer to a follow-up issue: "Extend axe-core full coverage to entity-detail routes (/title/:id, /borrower/:id, /contributor/:id, /series/:id) once detail-page fixtures are stable."

### Project Structure Notes

- `tests/e2e/specs/accessibility-full.spec.ts` — NEW comprehensive spec.
- `docs/accessibility-audit.md` — NEW manual audit doc.
- No code changes unless axe surfaces small fixable issues (alt-text, contrast).

### References

- [Source: epics.md#Story-9.22] — story spec verbatim
- [Source: tests/e2e/helpers/accessibility.ts] — existing helper
- [Source: tests/e2e/specs/journeys/catalog-title.spec.ts] — existing spot axe usage

## Dev Agent Record

### Agent Model Used

claude-opus-4-7[1m]

### Debug Log References

(populated by dev agent)

### Completion Notes List

(populated by dev agent)

### File List

**New:**
- `tests/e2e/specs/accessibility-full.spec.ts`
- `docs/accessibility-audit.md`

### Change Log

| Date | Change |
| --- | --- |
| 2026-05-10 | Story created (backlog → ready-for-dev). PRAGMATIC SCOPE: discovery + gate, not a fix-everything story. Iterates 12 URLs (top-level routes only — entity-detail routes deferred for fixture-plumbing reasons), runs axe-core with WCAG 2.2 AA tags, asserts zero violations. If violations found: file as `type:bug`. Manual audit doc covers keyboard nav + contrast + SR smoke-test notes. Net ~300 LOC (spec + doc). Shipping path: 0 violations → close; N violations → ship spec + doc + N deferred issues. |
