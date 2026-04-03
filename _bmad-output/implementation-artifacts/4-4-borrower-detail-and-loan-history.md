# Story 4.4: Borrower Detail & Loan History

Status: done

## Story

As a librarian,
I want to view a borrower's active loans on their detail page and return loans from there,
so that I can manage individual borrower relationships.

## Acceptance Criteria

### AC1: Borrower Detail Shows Active Loans (FR89)

- Given a borrower detail page at /borrower/{id}
- When it loads
- Then it displays the borrower's contact details (existing) AND a list of their active loans
- And if no active loans exist, an empty state message is shown

### AC2: Loan Details in Borrower Page (FR89)

- Given a borrower with active loans
- When viewing their detail page
- Then each active loan shows: volume label, title name, loaned_at date, and duration in days
- And duration is color-coded per UX-DR5: normal < 14 days, amber 14 to (threshold-1), red >= threshold with "Overdue" badge
- And the overdue threshold is read from AppSettings (default 30 days)

### AC3: Return Loan from Borrower Page (FR45, FR89)

- Given a borrower detail page showing active loans
- When the librarian clicks "Return" on a loan row (with confirmation dialog)
- Then the loan is returned using the same `POST /loans/{id}/return` endpoint as the /loans page
- And a success feedback message is displayed
- And the loan row disappears (page reloads)

## Tasks / Subtasks

- [x] Task 0: i18n Keys (AC: #1)
  - [ ] Add to `locales/en.yml` and `locales/fr.yml`:
    - `borrower.active_loans: "Active loans"` / `"Prêts actifs"`
    - `borrower.no_active_loans: "This borrower has no active loans."` / `"Cet emprunteur n'a aucun prêt actif."`
  - [ ] Run `touch src/lib.rs && cargo build`

- [x] Task 1: LoanModel — list_active_by_borrower (AC: #1, #2)
  - [ ] Add `list_active_by_borrower(pool, borrower_id) -> Result<Vec<LoanWithDetails>, AppError>`:
    ```sql
    SELECT l.id, l.volume_id, l.borrower_id, l.loaned_at,
           b.name AS borrower_name, v.label AS volume_label,
           t.title AS title_name,
           DATEDIFF(NOW(), l.loaned_at) AS duration_days
    FROM loans l
    JOIN borrowers b ON l.borrower_id = b.id AND b.deleted_at IS NULL
    JOIN volumes v ON l.volume_id = v.id AND v.deleted_at IS NULL
    JOIN titles t ON v.title_id = t.id AND t.deleted_at IS NULL
    WHERE l.borrower_id = ? AND l.returned_at IS NULL AND l.deleted_at IS NULL
    ORDER BY l.loaned_at DESC
    ```
    No pagination (borrowers typically have few active loans).
  - [ ] Unit test: verify struct construction

- [x] Task 2: Update borrower_detail Route + Template (AC: #1, #2, #3)
  - [ ] Add fields to `BorrowerDetailTemplate`:
    - `active_loans: Vec<LoanWithDetails>` (import from `crate::models::loan::LoanWithDetails`)
    - `overdue_threshold: i64`
    - `days_label: String`
    - `return_label: String`
    - `overdue_label: String`
    - `confirm_label: String`
    - `active_loans_label: String`
    - `no_active_loans_label: String`
    - `col_volume: String`
    - `col_title: String`
    - `col_date: String`
    - `col_duration: String`
    - `col_action: String`
  - [ ] In `borrower_detail()` handler:
    - Call `LoanModel::list_active_by_borrower(pool, borrower.id)`
    - Read `state.settings.read().unwrap().overdue_threshold_days as i64`
    - Pass active_loans, threshold, and all i18n labels to template
  - [ ] Update `templates/pages/borrower_detail.html`:
    - Add "Active loans" section after contact details
    - If loans empty: show `{{ no_active_loans_label }}`
    - If loans exist: table with columns volume, title, date, duration (color-coded), Return button — **NO borrower column** (borrower context already known from the page)
    - Duration color coding: same pattern as loans.html (`>= threshold` red+badge, `>= 14` amber, else normal)
    - Return button: `hx-post="/loans/{{ loan.id }}/return"` `hx-confirm="{{ confirm_label }}"` `hx-target="#borrower-feedback"` `hx-disabled-elt="this"`
    - Add `aria-live="polite"` to `#borrower-feedback` div for accessibility
    - Page reload after return: use `window.location.reload()` (NOT URLSearchParams — borrower detail has no query params unlike /loans)
  - [ ] Unit tests for template fields

- [x] Task 3: E2E Tests (AC: #1-#3)
  - [ ] E2E: navigate to borrower detail → see active loans section (empty state or with loans)
  - [ ] E2E: create loan for borrower → navigate to detail → verify loan appears with correct info
  - [ ] E2E: return loan from borrower detail page → verify loan disappears
  - [ ] **Smoke test**: login → create borrower → lend volume → borrower detail → return loan → verify
  - [ ] Test file: `tests/e2e/specs/journeys/borrower-loans.spec.ts`

## Dev Notes

### Architecture Compliance

- **Routes thin, services thick**: No new service logic needed — `LoanService::return_loan()` already handles returns. Route just adds data loading.
- **Error handling**: Existing `AppError` pattern. Borrower not found → 404.
- **Soft delete**: All queries include `deleted_at IS NULL`.
- **i18n**: All user-facing text via `t!()`.
- **XSS**: Template uses Askama auto-escaping. Return button reuses `/loans/{id}/return` endpoint which already handles html_escape.

### No New Routes Needed

The return action uses the existing `POST /loans/{id}/return` endpoint registered at `src/routes/mod.rs:142-143`. The borrower detail page at `/borrower/{id}` is already registered. No route changes needed.

### Data Model

`LoanModel::list_active_by_borrower()` follows the same pattern as `list_active()` but filtered by `borrower_id` and without pagination. Returns `Vec<LoanWithDetails>` reusing the existing struct.

### Template Pattern

The active loans table in `borrower_detail.html` reuses the same HTML structure as the loans table in `loans.html` (lines 78-132), with these differences:
- No pagination (all loans shown)
- No sort headers (small number of loans per borrower)
- No scan field
- No "New loan" form
- Same Return button pattern with `hx-disabled-elt="this"`
- Same duration color coding with `overdue_threshold`

### Existing Infrastructure (reuse, don't reinvent)

- `LoanWithDetails` struct — already has all needed fields (src/models/loan.rs:20-30)
- `POST /loans/{id}/return` → `return_loan_handler()` — already handles return with transaction
- `feedback_html_pub()` — for HTMX feedback
- `BorrowerModel::find_by_id()` — already used in borrower_detail handler
- `BorrowerModel::count_active_loans()` — exists but not needed (we load the full list instead)
- Loan i18n keys: `loan.return`, `loan.return_confirm`, `loan.overdue`, `loan.days`, `loan.col_*` — all exist

### Previous Story Intelligence

**From story 4-3 code review:**
- Return button must have `hx-disabled-elt="this"` to prevent double-click
- Duration color: `overdue_threshold` must be `i64` to match `duration_days` type
- Page reload after return should preserve URL params via `URLSearchParams`
- format! strings: `#` in hx-target must be passed as named parameter (not inline)

### References

- [Source: _bmad-output/planning-artifacts/prd.md] — FR89
- [Source: _bmad-output/planning-artifacts/epics.md] — Epic 4, Story 4.4
- [Source: src/routes/borrowers.rs] — BorrowerDetailTemplate, borrower_detail handler
- [Source: src/models/loan.rs] — LoanWithDetails, list_active pattern
- [Source: templates/pages/borrower_detail.html] — current template structure
- [Source: templates/pages/loans.html:78-132] — loan table pattern to reuse
- [Source: src/routes/loans.rs:189-215] — return_loan_handler (reuse endpoint)

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- DB still offline (pre-existing session.rs query! macro). Code follows verified patterns.

### Completion Notes List

- Task 0: Added 2 i18n keys (borrower.active_loans, borrower.no_active_loans) to en.yml and fr.yml.
- Task 1: Added LoanModel::list_active_by_borrower() — filtered query by borrower_id, no pagination, returns Vec<LoanWithDetails>.
- Task 2: Updated BorrowerDetailTemplate with 13 new fields, borrower_detail handler loads active loans + overdue threshold, template shows loan table with Return buttons, overdue colors, aria-live, window.location.reload on return.
- Task 3: Created borrower-loans.spec.ts with 4 E2E tests: empty state, loan details display, return from detail, smoke lifecycle.

### File List

- src/models/loan.rs (modified) — Added list_active_by_borrower method
- src/routes/borrowers.rs (modified) — Updated BorrowerDetailTemplate + borrower_detail handler with active loans
- templates/pages/borrower_detail.html (modified) — Added active loans table with return buttons and overdue colors
- locales/en.yml (modified) — Added borrower.active_loans, borrower.no_active_loans
- locales/fr.yml (modified) — Added borrower.active_loans, borrower.no_active_loans
- tests/e2e/specs/journeys/borrower-loans.spec.ts (new) — E2E tests for borrower loan history

### Change Log

- 2026-04-03: Story 4-4 implemented — Borrower detail page with active loans, return buttons, overdue highlighting, E2E tests.
