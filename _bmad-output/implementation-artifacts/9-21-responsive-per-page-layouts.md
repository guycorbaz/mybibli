# Story 9.21: Responsive per-page layouts

Status: ready-for-dev

## Story

As a user on a tablet or mobile device,
I want each page to adapt its layout to the viewport,
so that the most important elements are reachable and usable without horizontal scrolling.

## ⚠️ Existing-code reality check

Status of `main` as of 2026-05-10 (post 9-20 close):

- **Tailwind CSS is mobile-first** — most surfaces ALREADY respond on small viewports because the developers used `sm:`, `md:`, `lg:` prefixes throughout. 9-21 fills the GAPS, not a from-scratch responsive pass.

- **Existing responsive coverage (per surface)**:
  - **`/catalog`** (`templates/pages/catalog.html`): uses `lg:order-1/2/3` for desktop-vs-mobile reordering of the scan-field/feedback-list. Works at `lg:` (1024px+); the spec says "tablet" should reorder. **Decision (frozen)**: keep the existing `lg:` breakpoint — switching to `md:` would put feedback-above-scan on tablets in landscape (768-1023px), which is the desktop-experience viewport and would feel odd. Document the spec deviation; UX-DR24 explicitly says desktop = `≥ 1024px`.
  - **`/loans`** (`templates/pages/loans.html:90`): `<table class="min-w-full text-sm">` — NO responsive prefix; overflows horizontally on viewports < ~700px. **9-21 GAP**.
  - **`/borrowers`** (`templates/pages/borrowers.html:63`): `<table class="w-full text-sm">` — same gap. **9-21 GAP**.
  - **`/title/:id`** (`templates/pages/title_detail.html`): contains a volumes table — verify in Task 1, likely same gap.
  - **`/`** (`templates/pages/home.html`): dashboard uses `grid-cols-1 sm:grid-cols-3` — already responsive.
  - **`/admin`** (`templates/components/admin_tabs.html:3`): `flex flex-wrap gap-2` — tabs already wrap on small viewports. The spec wants a "select dropdown on mobile" — that's an UX preference not a correctness issue; `flex-wrap` works. **Defer the dropdown variant to a follow-up**.

- **Pragmatic scope decision (frozen)**: 9-21 ships **minimal-viable responsiveness** for the table surfaces:
  1. Wrap each problematic table in `<div class="overflow-x-auto">` so the table scrolls HORIZONTALLY within its container instead of pushing the page off-screen. Standard Tailwind pattern.
  2. Apply `hidden md:table-cell` to the LESS-ESSENTIAL columns per the spec list (e.g., `created_at` on /loans).
  3. NO DataTable→cards transformation. The spec mentions "card list on mobile" but that requires dual markup or a JS rendering switch — both out of scope for v1. Defer as `type:change-request` follow-up.
  4. NO admin-tabs mobile dropdown. `flex-wrap` is good enough. Defer.

- **Existing tests for the affected surfaces**:
  - `tests/e2e/specs/journeys/loans.spec.ts` exercises /loans at default viewport (no responsive assertions today).
  - `tests/e2e/specs/journeys/borrowers.spec.ts` same.
  - **9-21 ADDS** a NEW spec `tests/e2e/specs/journeys/responsive-layouts.spec.ts` that runs each surface at 3 viewport widths (375 mobile, 768 tablet, 1280 desktop) and asserts no horizontal scroll AND that the role-gated tables remain readable.

- **i18n**: no new keys needed — the responsive transformations use Tailwind classes only, no new copy.

- **Foundation Rule #2**: no JS module added. Pure CSS / Tailwind transformations.

## Acceptance Criteria

1. **AC1 — `/loans` table responsive** (`templates/pages/loans.html`):
   - Wrap the `<table>` in `<div class="overflow-x-auto">` so the table scrolls horizontally within its container on viewports < `md:`.
   - Apply `hidden md:table-cell` to the columns that are NOT essential on mobile (per spec: keep Volume, Borrower, Duration, Action — hide "Created date" and "Borrowed_at").
   - Verify by inspection: at 375px viewport, the table renders with 4 columns visible; at 768px+, all columns visible.

2. **AC2 — `/borrowers` table responsive** (`templates/pages/borrowers.html`):
   - Wrap the `<table>` in `<div class="overflow-x-auto">`.
   - Apply `hidden md:table-cell` to non-essential columns (verify in Task 1 — likely the email/phone columns since they're less critical than name).

3. **AC3 — `/title/:id` volumes table responsive** (`templates/pages/title_detail.html` or wherever the volumes table lives):
   - Same pattern: `overflow-x-auto` wrapper, `hidden md:table-cell` on selected columns.
   - Identify the volumes table location in Task 1.

4. **AC4 — `/admin` tabs already wrap (re-verified, no change)**:
   - `templates/components/admin_tabs.html:3` already has `flex flex-wrap gap-2`. **No change needed.** Defer the "select dropdown on mobile" preference to a follow-up GH issue if the user wants it later.

5. **AC5 — `/` dashboard already responsive (re-verified, no change)**:
   - `grid-cols-1 sm:grid-cols-3` is in place at `home.html:127`. **No change needed.** Verify by Task 1 grep.

6. **AC6 — `/catalog` `lg:` breakpoint preserved** (deviation from spec):
   - Existing `lg:order-1/2/3` (desktop reorder) is KEPT. Spec says "tablet should reorder feedback-above-scan" but UX-DR24 defines desktop as `≥1024px`, so `lg:` (1024px) is the correct breakpoint. Document the deviation in Dev Notes.

7. **AC7 — NO DataTable→cards transformation**:
   - Spec mentions a "mobile-cards variant" of the DataTable component. Out of scope for v1 — requires dual markup or JS rendering switch. **Defer as `type:change-request` follow-up GH issue at story close.**

8. **AC8 — CSP compliance**:
   - Pure Tailwind classes; no inline `style=`, no JS. `cargo test no_inline_markup_in_templates` green.

9. **AC9 — i18n**: no new keys.

10. **AC10 — E2E test** — NEW spec `tests/e2e/specs/journeys/responsive-layouts.spec.ts` (~150 LOC, 3 scenarios):
    1. **Mobile viewport (375x667)**: navigate to `/loans` (librarian), `/borrowers`, `/` and assert NO horizontal scroll on `<body>` AND that key elements are visible (e.g., `#search-field` on `/`, the loans table title-cell on `/loans`).
    2. **Tablet viewport (768x1024)**: same 3 surfaces, assert layout adapts (e.g., on `/loans` the hidden columns reappear).
    3. **Desktop viewport (1280x800)**: same surfaces, assert the desktop layout (all columns visible, dashboard at full width).
    - "No horizontal scroll" assertion: `await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)` (1px tolerance for sub-pixel).
    - Stable selectors only.
    - Flake gate: NO `waitForTimeout`.

11. **AC11 — Foundation Rule #12 LOC discipline**:
    - `templates/pages/loans.html`: ~5 LOC change (overflow wrapper + 2 column-hide).
    - `templates/pages/borrowers.html`: ~5 LOC.
    - `templates/pages/title_detail.html`: ~5 LOC.
    - `tests/e2e/specs/journeys/responsive-layouts.spec.ts`: NEW ~150 LOC.
    - **Total**: ~165 LOC.

12. **AC12 — Local testing**:
    - `cargo check` clean.
    - `cargo clippy --all-targets -- -D warnings` clean.
    - `cargo test no_inline_markup_in_templates` green.
    - Full E2E lane green (including the new responsive spec at 3 viewport widths).
    - Flake gate clean.

13. **AC13 — Draft PR + CI gate**: Foundation Rule #15 + #18.

14. **AC14 — Follow-up GH issues at story close**:
    - 1 `type:change-request` for "DataTable → mobile-cards transformation" (out of scope per AC7).
    - 1 `type:change-request` for "/admin tabs as mobile-only select dropdown" (out of scope per AC4).

## Tasks / Subtasks

- [ ] **Task 1 — Verify reality-check assumptions (AC: all)**
  - [ ] Read `templates/pages/loans.html` and identify the column structure (which `<th>` elements correspond to "Created date" / "Borrowed_at" — the spec wording).
  - [ ] Read `templates/pages/borrowers.html` and identify the columns to hide on mobile.
  - [ ] Find the volumes table on `/title/:id` (likely in `templates/pages/title_detail.html` or a fragment).
  - [ ] Confirm `templates/components/admin_tabs.html` uses `flex flex-wrap` (no change needed).
  - [ ] Confirm `templates/pages/home.html` dashboard uses `grid-cols-1 sm:grid-cols-3` (no change needed).
  - [ ] Run baseline `cargo test no_inline_markup_in_templates` → green.

- [ ] **Task 2 — Wrap `/loans` table in overflow-x-auto + column-hide (AC: 1)**
  - [ ] Edit `templates/pages/loans.html`: wrap the `<table>` in `<div class="overflow-x-auto">`.
  - [ ] Add `hidden md:table-cell` to the non-essential columns' `<th>` AND `<td>` elements.
  - [ ] Verify `cargo build` clean.

- [ ] **Task 3 — Wrap `/borrowers` table (AC: 2)**
  - [ ] Edit `templates/pages/borrowers.html`: same pattern.
  - [ ] Verify `cargo build` clean.

- [ ] **Task 4 — Wrap `/title/:id` volumes table (AC: 3)**
  - [ ] Edit the relevant template: same pattern.
  - [ ] Verify `cargo build` clean.

- [ ] **Task 5 — E2E test (AC: 10)**
  - [ ] Create `tests/e2e/specs/journeys/responsive-layouts.spec.ts` with 3 scenarios per AC10.
  - [ ] Run `npx tsc --noEmit` to verify the spec compiles.
  - [ ] Run `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/responsive-layouts.spec.ts` and confirm green.
  - [ ] Run full E2E lane.

- [ ] **Task 6 — Local gate + push + draft PR (AC: 12, 13)**
  - [ ] `cargo check` + `cargo clippy --all-targets -- -D warnings` clean.
  - [ ] `cargo test no_inline_markup_in_templates` green.
  - [ ] CI flake gate clean.
  - [ ] Push, open draft PR, wait CI green per Rule #18.

- [ ] **Task 7 — File 2 follow-up GH issues at story close (AC: 14)**
  - [ ] DataTable→mobile-cards transformation (`type:change-request`).
  - [ ] /admin tabs mobile-only select dropdown (`type:change-request`).

## Dev Notes

### Why `overflow-x-auto` over DataTable→cards transformation

The spec calls for a "mobile-cards variant" of the DataTable component. That requires:
- (a) Dual markup per surface (table for desktop, card list for mobile, both server-rendered, both kept in sync) — significant template churn.
- (b) Or a JS rendering switch — flash of wrong layout on initial render, plus JS complexity.

Both options have meaningful trade-offs. `overflow-x-auto` is the standard Tailwind pattern for "table doesn't fit on this viewport — let the user scroll": no dual markup, no JS, no flash, retains the same data structure for screen readers, prints correctly. Modern users on mobile expect to scroll horizontally on table content.

**Defer the card transformation** to a future story when there's clearer UX evidence that horizontal scroll is hurting users.

### Why keep the `lg:` breakpoint on /catalog

The story spec says "tablet — feedback list moves above the scan field". UX-DR24 defines tablet as 768-1023px and desktop as ≥1024px. The current `lg:order-...` puts the desktop-style layout at 1024px+, which means tablets get the mobile-style layout (feedback below). Switching to `md:order-...` would give tablets-in-landscape the desktop layout — which is fine if the spec intent is "use desktop layout when there's enough horizontal space", but the spec EXPLICITLY says "tablet — feedback above scan field" which is the MOBILE variant.

The current implementation matches "mobile + tablet" → mobile layout, "desktop" → desktop layout. That's coherent. The spec's prose contradicts UX-DR24 here. Stay with current behavior; document.

### Why no admin-tabs select dropdown

`flex flex-wrap gap-2` on `admin_tabs.html:3` already wraps tabs onto multiple rows on narrow viewports. A `<select>` dropdown is a UX preference; the wrapping behavior is already accessible. **Defer** the dropdown variant to a follow-up.

### Project Structure Notes

- `templates/pages/loans.html`, `borrowers.html`, `title_detail.html` (or volume fragment) — modified.
- `tests/e2e/specs/journeys/responsive-layouts.spec.ts` — NEW.
- No struct edits, no JS, no i18n keys.

### References

- [Source: epics.md#Story-9.21] — story spec verbatim
- [Source: ux-design-specification.md#UX-DR24] — breakpoint definitions
- [Source: ux-design-specification.md#UX-DR28] — responsive layouts UX-DR
- [Source: CLAUDE.md#Foundation-Rules] — Rules #11, #12, #13, #15, #18

## Dev Agent Record

### Agent Model Used

claude-opus-4-7[1m]

### Debug Log References

(populated by dev agent)

### Completion Notes List

(populated by dev agent)

### File List

**Modified:**
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — status transitions.
- `templates/pages/loans.html` — overflow wrapper + column-hide.
- `templates/pages/borrowers.html` — overflow wrapper + column-hide.
- `templates/pages/title_detail.html` (or volume fragment) — same.

**New:**
- `tests/e2e/specs/journeys/responsive-layouts.spec.ts` — 3 viewport scenarios.

### Change Log

| Date | Change |
| --- | --- |
| 2026-05-10 | Story created (backlog → ready-for-dev). PRAGMATIC SCOPE — minimal-viable responsiveness via Tailwind `overflow-x-auto` + `hidden md:table-cell` on the 3 problematic tables (/loans, /borrowers, /title/:id volumes). Other surfaces already mobile-first responsive (`/catalog` `lg:order`, `/` `grid-cols-1 sm:grid-cols-3`, `/admin` `flex-wrap`). DataTable→cards transformation explicitly DEFERRED to follow-up `type:change-request` per AC7. Admin tabs mobile-dropdown also deferred (flex-wrap is sufficient). 1 NEW E2E spec at 3 viewport widths verifying no horizontal scroll. Net ~165 LOC, no JS, no struct edits, no i18n. Targeted scope to enable epic-9 close today. |
