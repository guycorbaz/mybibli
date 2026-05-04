# Story 9.8: Loan status role-aware on volume detail

Status: in-progress

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As an anonymous user,
I want to see whether a volume is on loan without seeing the borrower's name,
so that privacy is preserved while I can still tell whether the item is currently available.

## Acceptance Criteria

1. **AC1 — Volume-detail page (`/volume/:id`) gains a loan-status field — anonymous variant.** Given the volume-detail page seen by an Anonymous role, when it renders for a volume that has an active loan (`returned_at IS NULL AND deleted_at IS NULL`), then a loan-status badge displays **"On loan since {date}"** / **"En prêt depuis le {date}"** with NO borrower name, NO clickable borrower link, and NO numeric `borrower_id` reference. The `{date}` is the loan's `loaned_at` formatted via the existing locale-aware date helper (matching the format used elsewhere in the app — e.g. `loans.html`'s row date column). The badge is placed in a NEW row in the volume-detail page's left-side definition list, between the existing "Location" row (line 40-50 of `templates/pages/volume_detail.html`) and the librarian-only Edit button (line 53). Visual treatment: amber/yellow palette (`bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400`) — matches the existing "not shelved" badge palette so the eye reads "this volume is unavailable" consistently across both surfaces.

2. **AC2 — Volume-detail page — librarian/admin variant (with borrower).** Given the same view, when seen by a Librarian or Admin, then the loan-status badge displays **"On loan to {borrower name} since {date}"** / **"En prêt à {nom} depuis le {date}"** where `{borrower name}` is a clickable link to `/borrower/{borrower_id}` (existing route). The link uses the standard project link palette (`text-indigo-600 dark:text-indigo-400 hover:underline`). The `{date}` formatting matches the anonymous variant. The badge wrapper itself stays the same amber palette.

3. **AC3 — Not-on-loan: existing UX preserved (no new badge).** Given a volume that is NOT on loan (no active loan row in `loans`), when rendered for ANY role, then NO loan-status badge appears in the new row position. The "Location" badge (existing — `not_shelved_label` for `location_id IS NULL`, plain location path otherwise) stays unchanged in its existing position. Two-row layout when on loan; one-row layout (location only) when not on loan. The HTML structure does NOT include an empty wrapper or an empty `<span>` — the entire definition-list row is omitted via Askama `{% if let Some(loan) = loan_status %}` so the rendered HTML byte-stream is identical to the pre-9-8 baseline when there's no active loan.

4. **AC4 — Shared `loan_status_badge.html` partial parameterized by role.** Create a NEW Askama macro at `templates/components/loan_status_badge.html` parameterized by the role-aware shape:
   ```jinja
   {% macro badge(role, on_loan_since, borrower_id, borrower_name, label_anonymous, label_with_borrower) %}
   ```
   - `role: &str` — values `"anonymous"`, `"librarian"`, `"admin"` (matches the existing `role` field on every page-template struct, e.g. `volume_detail.html` line 53).
   - `on_loan_since: NaiveDate` (or pre-formatted `&str` — pick whichever matches the existing date-format helper convention; checking `templates/pages/loans.html` is the canonical reference).
   - `borrower_id: Option<u64>` — `Some` IFF role >= librarian AND on_loan; `None` otherwise.
   - `borrower_name: Option<&str>` — same as `borrower_id`.
   - `label_anonymous: &str` — pre-translated "On loan since {date}" template (interpolation done in the macro via Jinja `{{ label_anonymous }}` placeholder fill).
   - `label_with_borrower: &str` — pre-translated "On loan to {} since {}" template.
   The macro renders the anonymous-variant when `role == "anonymous"` OR `borrower_id.is_none()` (defense-in-depth: even if a future caller accidentally passes `role = "librarian"` without populating borrower fields, the macro falls back to the safe anonymous variant rather than panicking on a `None.unwrap()`). When called from page templates, callers use `{% call loan_status_badge::badge(...) %}{% endcall %}`. The macro file is < 30 LOC.

5. **AC5 — SQL projection narrowed for anonymous (no JOIN to `borrowers`).** Given the SQL that drives `volume_detail`, when fetching loan info, then for an Anonymous request the query SELECTs only `loaned_at` from `loans` WHERE `volume_id = ? AND returned_at IS NULL AND deleted_at IS NULL` — NO JOIN to `borrowers`, NO borrower_id projected. For Librarian/Admin requests the query JOINs `borrowers b ON l.borrower_id = b.id AND b.deleted_at IS NULL` and projects `b.name AS borrower_name + l.borrower_id`. Implementation: TWO new model methods on `LoanModel` (`src/models/loan.rs`):
   - `pub async fn active_loan_summary_for_volume(pool: &DbPool, volume_id: u64) -> Result<Option<NaiveDateTime>, AppError>` — returns just the `loaned_at` timestamp if an active loan exists, else `None`. NO borrower data fetched. Use `sqlx::query_scalar::<_, Option<NaiveDateTime>>` with the narrow SELECT.
   - `pub async fn active_loan_with_borrower_for_volume(pool: &DbPool, volume_id: u64) -> Result<Option<ActiveLoanWithBorrower>, AppError>` where `ActiveLoanWithBorrower` is a NEW small projection struct with `borrower_id: u64`, `borrower_name: String`, `loaned_at: NaiveDateTime`. Use `sqlx::query_as` with the JOIN. NEW struct (NOT `LoanWithDetails` reuse) because the dashboard's `LoanWithDetails` carries fields not needed here (`volume_label`, `title_name`, `duration_days`, `id`) and we want the narrow projection both for clarity and to reduce data fetched on every volume-detail page render.
   - Both methods use **dynamic `query` / `query_as`** (NOT the macro `sqlx::query!`) per project convention (matches 9-2/9-3/9-4/9-5/9-6/9-7).
   - Handler logic in `volume_detail` calls ONE of the two methods based on `session.role >= Role::Librarian` — the call site is the role gate. Two-layer defense: SQL projection narrowing AT THE QUERY LAYER (no borrower fields in anonymous SELECT) + role gate AT THE CALL SITE (handler picks which method to call). This pattern is intentionally stricter than 9-4/9-5/9-6/9-7's indicator surface, where we role-gated only at the call site (data was always fetched then conditionally rendered) — for FR59's privacy-sensitive case, the data MUST NOT travel through the application's memory at all when the user can't see it.

6. **AC6 — `VolumeDetailTemplate` extended with `loan_status: Option<LoanStatusView>`.** NEW small view-model struct `LoanStatusView` (in `src/routes/catalog.rs` near `VolumeDetailTemplate`):
   ```rust
   pub struct LoanStatusView {
       pub loaned_at_label: String,    // pre-formatted date string, locale-aware
       pub borrower_id: Option<u64>,   // Some iff role >= Librarian
       pub borrower_name: Option<String>, // Some iff role >= Librarian
   }
   ```
   The handler builds `Option<LoanStatusView>` via the role-branched call to one of the two model methods. `VolumeDetailTemplate` gains 3 new fields: `loan_status: Option<LoanStatusView>`, `loan_status_label_anonymous: String` (pre-translated "On loan since %{date}" — locale-interpolated by the handler), `loan_status_label_with_borrower: String` (pre-translated "On loan to %{name} since %{date}"). The interpolation is done by `rust_i18n::t!()` call's standard `%{}` syntax (project convention; verified usable across the codebase). The macro then receives the already-interpolated strings — this is the same "pre-translate in the handler" pattern as 9-1..9-7 indicator headings.

7. **AC7 — i18n EN + FR (4 new keys).** Append to `locales/en.yml` + `locales/fr.yml` under the existing `volume:` block (`locales/{en,fr}.yml` around line ~250 — `volume.detail_title`, `volume.not_shelved`, etc. already live there):
   - `volume.on_loan_since` — EN: `"On loan since %{date}"`, FR: `"En prêt depuis le %{date}"`
   - `volume.on_loan_to_since` — EN: `"On loan to %{name} since %{date}"`, FR: `"En prêt à %{name} depuis le %{date}"`
   - **REUSED keys** (no new add): the borrower-link aria-label can reuse the existing `nav.borrowers` / `borrower.profile` family if such a key already exists; otherwise add a small `volume.view_borrower_aria` key (EN: `"View borrower profile"`, FR: `"Voir le profil de l'emprunteur"`) so the link's aria-label is properly localized rather than relying on the visible text alone.
   - **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper — keys start at root. After editing, run `touch src/lib.rs && cargo build`. The `i18n::audit::tests::all_t_keys_have_both_locales` test enforces EN/FR mirror.

8. **AC8 — HTML-name-leak regression guard (load-bearing security test).** Given the rendered `volume_detail.html` for an Anonymous request on a volume with an active loan to a borrower named (e.g.) `"Alice Tremblay"`, when the rendered HTML is byte-asserted, then the substring `"Alice"` MUST NOT appear ANYWHERE in the response body — not in a comment, not in a `data-*` attribute, not in a hidden field, not in an aria-label. The handler render test `volume_detail_anonymous_does_not_leak_borrower_name` is the primary regression guard. Test fixture: build a `VolumeDetailTemplate` with `role = "anonymous"`, `loan_status = Some(LoanStatusView { loaned_at_label: "2026-04-15", borrower_id: None, borrower_name: None })`, render, then `assert!(!html.contains("Alice"))`. To make the assertion meaningful, the test ALSO renders the SAME template with `role = "librarian"` + populated borrower fields, asserts `"Alice"` IS present in the librarian render — proves the test fixture would catch a leak (i.e., the absence in the anonymous render is not a tautology).

9. **AC9 — Defense-in-depth: SQL projection narrowing locked by integration test.** NEW DB-backed `#[sqlx::test]` in NEW `tests/volume_detail_loan_status.rs`:
    - `active_loan_summary_for_volume_returns_loaned_at_for_active_loan` — seed: 1 volume + 1 active loan; assert returned `Some(NaiveDateTime)`.
    - `active_loan_summary_for_volume_returns_none_for_returned_loan` — seed: 1 volume + 1 returned loan (`returned_at IS NOT NULL`); assert `None`.
    - `active_loan_summary_for_volume_returns_none_for_soft_deleted_loan` — soft-deleted active loan; assert `None`.
    - `active_loan_summary_for_volume_returns_none_when_no_loans` — empty fixture; assert `None`.
    - `active_loan_with_borrower_for_volume_returns_full_struct_for_active_loan` — seed: volume + active loan + borrower named "Test Borrower"; assert `Some(ActiveLoanWithBorrower { borrower_name: "Test Borrower", borrower_id: <id>, loaned_at: <ts> })`.
    - `active_loan_with_borrower_for_volume_returns_none_for_returned_loan` — returned loan; assert `None`.
    - `active_loan_with_borrower_for_volume_excludes_soft_deleted_borrower` — active loan whose borrower row is soft-deleted; assert `None` (the borrower JOIN's `b.deleted_at IS NULL` filters it out — locks the safety invariant).
   File-local helpers reuse the patterns from `tests/dashboard_overdue.rs` / `tests/dashboard_recent_activity.rs`: `first_genre_id`, `first_volume_state_id`, `insert_title`, `insert_volume`, `insert_borrower`, `insert_loan`, `mark_loan_returned`, `soft_delete_loan`, `soft_delete_borrower` (NEW helper for AC9c — `UPDATE borrowers SET deleted_at = NOW() WHERE id = ?`).

10. **AC10 — Macro tests (4-cell matrix) + full-page render tests.**
    - **(a) `loan_status_badge` macro tests** in `src/templates_audit.rs` OR a new `src/routes/catalog.rs::tests` block (whichever the project convention is for component-macro tests — check `templates/components/filter_tag.html`'s test placement from 9-4 as the canonical precedent). 4-cell matrix:
      - Anonymous + on-loan → renders "On loan since X" without borrower name OR `/borrower/` link.
      - Anonymous + not-on-loan → empty render (badge omitted entirely; the `{% if let Some %}` gate at the macro call site prevents emit).
      - Librarian + on-loan → renders "On loan to X since Y" with `/borrower/{id}` link.
      - Librarian + not-on-loan → empty render.
    - **(b) `volume_detail` handler render tests** in `src/routes/catalog.rs::tests`:
      - `volume_detail_anonymous_does_not_leak_borrower_name` (AC8 — primary security guard).
      - `volume_detail_librarian_renders_borrower_link` — assert `href="/borrower/{id}"` is present + the borrower name appears + the date appears.
      - `volume_detail_no_active_loan_renders_no_badge` — empty `loan_status: None`, assert no badge HTML present (no "On loan" text in either locale).
      - `volume_detail_anonymous_with_loan_renders_amber_palette` — assert the badge classes (`bg-amber-100`) are present, locking the "consistent unavailability cue" UX contract from AC1.
    - Existing `volume_detail` tests (if any) need their factory extended with the 3 new fields populated to sensible defaults (`loan_status: None`, two empty Strings) so they keep passing.

11. **AC11 — E2E (Foundation Rule #7).** Append a NEW `test.describe("Volume detail — loan status role-aware (FR59)", ...)` block to `tests/e2e/specs/journeys/loans-stack.spec.ts` (OR a new spec file if no logical home — `loans-stack.spec.ts` is the natural fit since it already exercises the loan lifecycle):
    - Smoke test (single test, librarian-driven): seed a volume + create a loan to "Alice Tremblay" via the existing loan-creation E2E helper (`tests/e2e/helpers/loans.ts::createLoan`). Then:
      1. Logout (or open a fresh anonymous context). Navigate to `/volume/<id>`. Assert the badge text matches `/On loan|En prêt/i`. Byte-assert `expect(await page.content()).not.toContain("Alice")`.
      2. `loginAs(page, "librarian")`. Reload `/volume/<id>`. Assert the badge contains "Alice" AND a clickable link to `/borrower/<id>` (use `getByRole("link", { name: /Alice/ })`). Click the link, assert URL becomes `/borrower/<id>`.
    - Use i18n-aware regex matchers for visible text. NO `waitForTimeout` (CI grep gate enforced).

12. **AC12 — CSP compliance.** No `style="..."`, no `<style>`, no `<script>`, no `onclick=`, no inline event handlers in any template touched by this story. The new `loan_status_badge.html` macro uses Tailwind utility classes only (matching the existing `not_shelved_label` palette). The `src/templates_audit.rs::no_inline_markup_in_templates` test (line 44) MUST stay green.

13. **AC13 — Foundation Rule #12 — `volume_detail.html` LOC.** The current file is **62 LOC**. Adding the loan-status row (~5 LOC) + the macro import (1 LOC) lands the file at ~68 LOC — far below any ceiling. No mitigation needed. `src/routes/catalog.rs` is 2070+ LOC pre-9-8 (already over 2000 in some metrics — verify with `wc -l` at task start). Adding the `LoanStatusView` struct + extending `VolumeDetailTemplate` + the 4 new render tests adds ~40-60 LOC. **AC13 verification step in Task 7**: `wc -l src/routes/catalog.rs` post-9-8 must NOT exceed any pre-existing project limit. If `catalog.rs` was already over 2000 pre-9-8, this story does NOT make it worse by more than the strict minimum needed; if a follow-up extraction is warranted, file as `type:code-review-finding` (out of scope for 9-8 itself per refactor-during-feature anti-pattern).

14. **AC14 — Shared partial is forward-compatible across surfaces.** The `loan_status_badge.html` macro is designed for reuse beyond `volume_detail.html`. Spec text mentions "any caller (volume row, title detail, search result)". As of story 9-8 close, ONLY `volume_detail.html` has a volume-rendering surface that displays loan status — `title_detail.html` (`templates/pages/title_detail.html`) does NOT list individual volumes today (it shows title metadata + contributors + series); search results / `#browse-results` show TitleCard, not volume rows. The spec's "future-proof for other callers" requirement is satisfied by the macro's parameterized API — no additional caller wiring in 9-8. If a future story (e.g. "title-detail page lists volumes" — not currently scoped) adds a volume-rendering surface to `title_detail.html`, it imports + reuses the macro with the same parameter shape.

15. **AC15 — No data leak via tracing logs.** Verify NO `tracing::info!` / `tracing::warn!` / `tracing::error!` call in the new code includes `borrower_name`, `borrower_id`, or any other PII. The handler's existing `tracing::error!(error = %e, "Failed to render volume detail template")` (catalog.rs:1947) does NOT include borrower data — verified out-of-scope. NEW model methods MUST log only non-PII fields (e.g., `volume_id`) on error paths. Asserted by code review (no automated test — log-content assertions require a tracing subscriber test setup that the project has deferred per 9-6 D5).

## Tasks / Subtasks

- [x] **Task 1 — Two NEW `LoanModel` methods + `ActiveLoanWithBorrower` projection + `tests/volume_detail_loan_status.rs` (AC: 5, 9)**
  - [ ] In `src/models/loan.rs`, add the `ActiveLoanWithBorrower` projection struct near `LoanWithDetails` (lines 21-30). Fields: `borrower_id: u64`, `borrower_name: String`, `loaned_at: NaiveDateTime`. Mark `pub` (the catalog handler + tests need it).
  - [ ] Add `pub async fn active_loan_summary_for_volume(pool: &DbPool, volume_id: u64) -> Result<Option<NaiveDateTime>, AppError>`. SQL: `SELECT CAST(loaned_at AS DATETIME) FROM loans WHERE volume_id = ? AND returned_at IS NULL AND deleted_at IS NULL LIMIT 1`. Use `sqlx::query_scalar::<_, NaiveDateTime>` with `.fetch_optional(pool).await?`. Return `Ok(opt)`. The `LIMIT 1` is defensive (only 1 active loan per volume per business rule, but the constraint isn't enforced at the schema level — verify by reading `migrations/20260329000000_initial_schema.sql:158-175`; if no UNIQUE constraint, the LIMIT 1 prevents a future data-integrity bug from blowing up this query). Place AFTER `count_recent_returns` (post-9-7 location).
  - [ ] Add `pub async fn active_loan_with_borrower_for_volume(pool: &DbPool, volume_id: u64) -> Result<Option<ActiveLoanWithBorrower>, AppError>`. SQL:
    ```sql
    SELECT l.borrower_id, b.name AS borrower_name, CAST(l.loaned_at AS DATETIME) AS loaned_at
    FROM loans l
    JOIN borrowers b ON l.borrower_id = b.id AND b.deleted_at IS NULL
    WHERE l.volume_id = ? AND l.returned_at IS NULL AND l.deleted_at IS NULL
    LIMIT 1
    ```
    Use `sqlx::query_as::<_, ActiveLoanWithBorrower>(...)` with `#[derive(sqlx::FromRow)]` on the struct (mirror `SeriesWithGap` from 9-6). `.fetch_optional(pool).await?`.
  - [ ] Both methods use **dynamic `query` / `query_as`** (NOT the macro `sqlx::query!`) per project convention.
  - [ ] Build `tests/volume_detail_loan_status.rs` (NEW, sibling of `tests/dashboard_recent_activity.rs`):
    - Helpers: copy `first_genre_id`, `first_volume_state_id`, `insert_title`, `insert_volume`, `insert_borrower`, `insert_loan`, `mark_loan_returned`, `soft_delete_loan` from `tests/dashboard_overdue.rs` (cross-file copy is project precedent).
    - NEW helper `soft_delete_borrower(pool, borrower_id)` — `UPDATE borrowers SET deleted_at = NOW() WHERE id = ?`.
    - 7 `#[sqlx::test(migrations = "./migrations")]` cases per AC9 (4 for `_summary_`, 3 for `_with_borrower_`).
  - [ ] Verify: `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test --test volume_detail_loan_status` — all 7 green; lock as Commit 1.

- [ ] **Task 2 — `LoanStatusView` view-model + extend `VolumeDetailTemplate` + handler wiring (AC: 1, 2, 3, 5, 6)**
  - [ ] In `src/routes/catalog.rs`, add `pub struct LoanStatusView { pub loaned_at_label: String, pub borrower_id: Option<u64>, pub borrower_name: Option<String> }` near `VolumeDetailTemplate` (line 1859).
  - [ ] Extend `VolumeDetailTemplate` (line 1859-1882) with 3 new fields:
    - `pub loan_status: Option<LoanStatusView>` — `Some` iff active loan exists (any role); `None` otherwise.
    - `pub loan_status_label_anonymous: String` — pre-translated `"On loan since %{date}"` template, with `%{date}` already substituted via `rust_i18n::t!("volume.on_loan_since", locale = loc, date = formatted_date)`.
    - `pub loan_status_label_with_borrower: String` — pre-translated, same pattern but with both `%{name}` and `%{date}` substituted (only built when role >= Librarian + active loan exists; else empty String).
  - [ ] In `volume_detail` handler (line 1884), branch on `session.role >= Role::Librarian` to call ONE of the two new `LoanModel` methods (NEVER both — strict no-double-fetch). Build `loan_status: Option<LoanStatusView>` from the result. Build the 2 pre-translated label Strings (the with-borrower label is only meaningfully populated for Librarian+on_loan; for anonymous OR no_loan, leave it as empty `String::new()` — the template only references it via the macro which won't be called in those cells).
  - [ ] Soft-degrade pattern: if EITHER model method errors, `tracing::warn!(error = %e, "active_loan_*_for_volume failed; rendering volume detail without loan status")` + `loan_status = None`. Mirrors the 9-7 `.unwrap_or_else` soft-degrade idiom.
  - [ ] Date formatting: use the existing project date helper (search `templates/pages/loans.html` for the format string — likely `format!("%Y-%m-%d")` or `chrono::NaiveDateTime::format`). If no shared helper exists, format inline as `loaned_at.date().to_string()` (ISO 8601 — locale-neutral; date-locale formatting is out of scope for this story per refactor-during-feature anti-pattern).

- [ ] **Task 3 — `loan_status_badge.html` macro + integration into `volume_detail.html` (AC: 1, 2, 3, 4)**
  - [ ] Create `templates/components/loan_status_badge.html` per AC4 macro signature. Body:
    ```jinja
    {% macro badge(role, loaned_at_label, borrower_id, borrower_name, label_anonymous, label_with_borrower) %}
    <div class="flex items-center gap-3">
        <span class="text-sm font-medium text-stone-500 dark:text-stone-400 w-32">{{ label_loan_status_field }}</span>
        <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400 text-xs font-medium">
            {% if role != "anonymous" %}{% if let Some(bid) = borrower_id %}{% if let Some(bname) = borrower_name %}
                {# Librarian/Admin variant — borrower name + link. The
                   `label_with_borrower` is pre-translated and pre-
                   interpolated by the handler with both name + date,
                   so the macro just splits on a sentinel placeholder
                   to insert the link. We use a 2-pass approach: the
                   handler builds the label as plain text "On loan to
                   Alice since 2026-04-15" and the macro renders the
                   borrower-name as a link by string-splitting on the
                   borrower_name. Alternative: build the label as 2
                   String fragments (before + after the name) in the
                   handler — cleaner but more handler logic. PICK
                   WHICHEVER pattern matches the existing project
                   convention for in-string interpolated links. The
                   simpler "2 String fragments" approach is recommended
                   for v1. #}
                <a href="/borrower/{{ bid }}" class="text-indigo-600 dark:text-indigo-400 hover:underline">{{ bname }}</a>
                <span>{{ label_with_borrower }}</span>
            {% endif %}{% endif %}
            {% else %}
                {# Anonymous variant — date only, NO borrower data. #}
                {{ label_anonymous }}
            {% endif %}
        </span>
    </div>
    {% endmacro %}
    ```
    **Implementation note:** the `label_with_borrower` interpolation pattern (whether to render the name as a link inside an interpolated string OR to split it into 2 String fragments) is the ONE judgment call in this macro. **Recommendation:** instead of `%{name}` interpolation, the handler builds 3 separate fields: `loan_status_label_prefix` ("On loan to "), borrower-name link rendered by macro, `loan_status_label_suffix` (" since 2026-04-15"). This avoids string-splitting in the template AND keeps the macro CSP-clean. Update AC6 to reflect this — the handler builds the prefix + suffix; the macro renders prefix + `<a>` + suffix.
  - [ ] Update `VolumeDetailTemplate` per the "3 fields" recommendation: replace `loan_status_label_with_borrower: String` with `loan_status_label_prefix: String` (e.g., "On loan to ") + `loan_status_label_suffix: String` (e.g., " since 2026-04-15"). The handler builds these from the locale + date + (NEVER includes the borrower name in the prefix/suffix — the name is rendered separately by the macro from `borrower_name`).
  - [ ] Update i18n keys per AC7 — `volume.on_loan_to_prefix` ("On loan to " / "En prêt à ") + `volume.on_loan_to_since_suffix` (" since %{date}" / " depuis le %{date}"). Drop the original `volume.on_loan_to_since` single-string key (replaced by the prefix + suffix pair).
  - [ ] In `templates/pages/volume_detail.html`, add `{% import "components/loan_status_badge.html" as loan_status %}` at the top (line 2).
  - [ ] Add the macro call between line 50 (Location row close) and line 52 (the `{% if role == "librarian" || role == "admin" %}` Edit button block):
    ```jinja
    {% if let Some(loan) = loan_status %}
    {% call loan_status::badge(role, loan.loaned_at_label, loan.borrower_id, loan.borrower_name, loan_status_label_anonymous, loan_status_label_prefix, loan_status_label_suffix) %}{% endcall %}
    {% endif %}
    ```
  - [ ] Add `label_loan_status_field: String` field to `VolumeDetailTemplate` and pre-translate via `rust_i18n::t!("volume.loan_status_field", locale = loc).to_string()` (e.g., "Loan status:" / "Statut du prêt :").

- [ ] **Task 4 — i18n EN + FR (5 new keys per Task 3 reshape) (AC: 7)**
  - [ ] In `locales/en.yml`, append to the existing `volume:` block:
    - `loan_status_field: "Loan status:"` (the left-side label in the definition list row)
    - `on_loan_since: "On loan since %{date}"` (anonymous variant)
    - `on_loan_to_prefix: "On loan to "` (text BEFORE the borrower-name link)
    - `on_loan_to_since_suffix: " since %{date}"` (text AFTER the borrower-name link)
    - `view_borrower_aria: "View borrower profile"` (aria-label for the link)
  - [ ] In `locales/fr.yml`, mirror at the same path:
    - `loan_status_field: "Statut du prêt :"`
    - `on_loan_since: "En prêt depuis le %{date}"`
    - `on_loan_to_prefix: "En prêt à "`
    - `on_loan_to_since_suffix: " depuis le %{date}"`
    - `view_borrower_aria: "Voir le profil de l'emprunteur"`
  - [ ] **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper. After editing, run `touch src/lib.rs && cargo build`. The `i18n::audit::tests::all_t_keys_have_both_locales` test enforces EN/FR mirror.

- [ ] **Task 5 — Macro tests (4-cell matrix) (AC: 10a)**
  - [ ] Add macro tests in the same location as 9-4's FilterTag macro tests — search `templates/components/filter_tag.html` usages in the codebase to find the canonical test placement. Likely candidates: `src/routes/home.rs::tests` (for `filter_tag` macro) OR a dedicated `src/templates_audit.rs` block. Use the SAME pattern to keep the precedent uniform.
  - [ ] 4 tests covering the AC10a matrix:
    - `loan_status_badge_macro_anonymous_on_loan_renders_date_only_no_borrower`
    - `loan_status_badge_macro_anonymous_not_on_loan_renders_nothing` (or: the `{% if let Some %}` gate at the call site means the macro is never invoked when not on loan; the test covers the gate behavior at the page-template level instead — pick the cleaner approach)
    - `loan_status_badge_macro_librarian_on_loan_renders_borrower_link`
    - `loan_status_badge_macro_librarian_not_on_loan_renders_nothing`
  - [ ] Each test instantiates a minimal Askama struct that calls the macro, renders it, and byte-asserts the expected output (presence/absence of `bname`, `href="/borrower/`, the amber palette classes, etc.).

- [ ] **Task 6 — `volume_detail` handler render tests (AC: 8, 10b)**
  - [ ] In `src/routes/catalog.rs::tests` (or wherever the existing `volume_detail` tests live — search first), add:
    - `volume_detail_anonymous_does_not_leak_borrower_name` — **AC8 LOAD-BEARING SECURITY GUARD.** Build a `VolumeDetailTemplate` with `role = "anonymous"` and `loan_status = Some(LoanStatusView { loaned_at_label: "2026-04-15", borrower_id: None, borrower_name: None })`. Render. Assert `!html.contains("Alice")` AND `!html.contains("Tremblay")` AND `!html.contains("/borrower/")`. Then build the SAME template with `role = "librarian"` + populated `borrower_id: Some(42)` + `borrower_name: Some("Alice Tremblay".to_string())` — render and assert `html.contains("Alice")` AND `html.contains("href=\"/borrower/42\"")`. The librarian-render assertion proves the test fixture WOULD catch a leak (i.e., the absence in the anonymous render is meaningful, not a tautology).
    - `volume_detail_librarian_renders_borrower_link` — assert `href="/borrower/{id}"` is present + the borrower name appears + the date appears.
    - `volume_detail_no_active_loan_renders_no_badge` — `loan_status: None`, assert no "On loan" text in either locale.
    - `volume_detail_anonymous_with_loan_renders_amber_palette` — assert `bg-amber-100` is present in the badge wrapper.
  - [ ] Existing `volume_detail` tests (if any — search first) need their factory extended with the 4 new fields populated to sensible defaults so they keep passing.

- [ ] **Task 7 — E2E spec block (AC: 11)**
  - [ ] In `tests/e2e/specs/journeys/loans-stack.spec.ts` (or whatever the existing loans E2E spec file is — verify by listing `tests/e2e/specs/journeys/` first), append `test.describe("Volume detail — loan status role-aware (FR59)", ...)` block with 1 smoke test per AC11.
  - [ ] Use the existing `tests/e2e/helpers/loans.ts::createLoan` helper to seed the fixture.
  - [ ] Use stable ID selectors / role queries; NO `waitForTimeout` (CI grep gate).
  - [ ] Use i18n-aware regex matchers for visible text: `/On loan|En prêt/i` etc.

- [ ] **Task 8 — Verify and document (AC: 1–15)**
  - [ ] `wc -l src/routes/catalog.rs` — verify the file did not grow significantly. AC13 is informational only (no hard ceiling set in CLAUDE.md for catalog.rs); document the final LOC in the Dev Agent Record.
  - [ ] `SQLX_OFFLINE=true cargo check && cargo clippy --all-targets -- -D warnings` — clean (zero warnings).
  - [ ] `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test` — full suite green. Expected: ~720 lib tests baseline + ~8 new (4 macro + 4 render) = ~728; +7 new integration tests in `tests/volume_detail_loan_status.rs`.
  - [ ] `cargo sqlx prepare --check --workspace` — expected no diff (Tasks 1 + 2 use dynamic `query` / `query_as`).
  - [ ] Tailwind rebuild: NOT required — every utility class used in the new macro (`bg-amber-100`, `text-amber-700`, `inline-flex`, etc.) is already present in compiled `output.css` (verified via the existing `not_shelved_label` badge in `volume_detail.html:46-49`).
  - [ ] Manual smoke from a running dev instance (`MYBIBLI_SKIP_SETUP=1 cargo run`):
    - Create a volume + a loan to a borrower named "Alice".
    - As anonymous: `curl http://localhost:8080/volume/<id>` and grep — `"Alice"` MUST NOT appear; `"On loan since"` MUST appear.
    - As librarian: `curl` with session cookie → grep — `"Alice"` MUST appear AND `"href=\"/borrower/<id>\""` MUST appear.
    - Click the borrower link in a browser → `/borrower/<id>` page loads.
  - [ ] **E2E** (Foundation Rule #13) — `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/loans-stack.spec.ts`.
  - [ ] Update Dev Agent Record at the bottom of this file: list of files touched, decisions on placement, anything surprising (drift discoveries — particularly that `volume_detail.html` did NOT have ANY loan-status display pre-9-8 despite the spec text implying "the existing field"; AND that `title_detail.html` does NOT list individual volumes today, so the macro's "shared across surfaces" intent is forward-looking only).
  - [ ] Update `_bmad-output/implementation-artifacts/sprint-status.yaml`: `9-8-loan-status-role-aware: ready-for-dev → in-progress` at start, `→ review` at end (only this line + `last_updated`, per CLAUDE.md rule 16).
  - [ ] Open draft PR at first commit (Foundation Rule #15). Title: `Story 9-8: Loan status role-aware on volume detail (#NN)`.

## Dev Notes

### Source tree references

| Concern | File / location | Notes |
|---|---|---|
| Volume-detail handler | `src/routes/catalog.rs:1884` (`volume_detail`) | extend with role-branched loan-status fetch + `LoanStatusView` build |
| Volume-detail template | `src/routes/catalog.rs:1859-1882` (`VolumeDetailTemplate`) | extend with 5 new fields (loan_status, label_loan_status_field, loan_status_label_anonymous, loan_status_label_prefix, loan_status_label_suffix) |
| Volume-detail page template | `templates/pages/volume_detail.html` (62 LOC pre-9-8) | add `{% import "components/loan_status_badge.html" as loan_status %}` + macro call between line 50 (Location row) and line 52 (Edit button block) |
| **NEW** loan-status partial | `templates/components/loan_status_badge.html` | NEW Askama macro per AC4 (parameterized by role + on-loan presence + borrower data) |
| Loan model | `src/models/loan.rs` (post-9-7 with `count_recent_returns` + `list_recent_returns`) | extend with `active_loan_summary_for_volume` (anonymous, no JOIN) + `active_loan_with_borrower_for_volume` (librarian, with JOIN) — see AC5 |
| Loan struct (return type) | `src/models/loan.rs:21-30` (`pub struct LoanWithDetails`) | NOT REUSED — too wide for the volume-detail surface. NEW narrower `ActiveLoanWithBorrower` struct for the librarian path (3 fields: borrower_id, borrower_name, loaned_at). |
| Borrower schema | `migrations/20260329000000_initial_schema.sql:147-156` | `borrowers (id, name, deleted_at, ...)`. The `b.deleted_at IS NULL` JOIN guard in the new librarian query is ESSENTIAL (locks AC9c). |
| Loan schema | `migrations/20260329000000_initial_schema.sql:158-175` | `loans (volume_id, borrower_id, loaned_at, returned_at, deleted_at, ...)`. The 3-condition active-loan filter (`returned_at IS NULL AND deleted_at IS NULL` + `volume_id = ?`) is reused from `count_active` / `count_overdue` (story 9-5). |
| Existing badge palette (location) | `templates/pages/volume_detail.html:46-49` (`not_shelved_label`) | the amber palette (`bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400`) is reused VERBATIM in the loan-status badge — locks the "consistent unavailability cue" UX contract. |
| Borrower detail route | `/borrower/:id` (existing route — verify in `src/routes/mod.rs`) | the link target for librarian's borrower-name in the badge. NO new route needed. |
| Role gating pattern | `session.role >= Role::Librarian` (project convention; `src/middleware/auth.rs:13-18` for the enum) | the role gate is at the HANDLER call site (handler picks which model method to call); the SQL projection is the second layer of defense (AC5). |
| Soft-degrade pattern | `src/routes/home.rs:267-339` (post-9-7 `.unwrap_or_else` uniformization) | apply the same pattern to the new loan-status fetch in `volume_detail` handler |
| Date formatting | check `templates/pages/loans.html` for the canonical date format string | use the same format for `loaned_at_label` to keep the UX consistent across surfaces |
| i18n locales | `locales/en.yml`, `locales/fr.yml` (search for the existing `volume:` block — likely around line 240-260 based on `volume.detail_title`, `volume.not_shelved`, etc.) | append 5 new keys per AC7 |
| i18n audit | `src/i18n/audit.rs::all_t_keys_have_both_locales` | enforces EN/FR mirror |
| Templates audit | `src/templates_audit.rs::no_inline_markup_in_templates` (line 44) | must stay green |
| Test pattern (DB-backed integration) | `tests/dashboard_overdue.rs` + `tests/dashboard_recent_activity.rs` (post-9-7) | sibling shape for `tests/volume_detail_loan_status.rs` |
| Test pattern (handler render) | `src/routes/home_indicator_tests.rs` (post-9-6 extraction) | mirror for `volume_detail` render tests; canonical "render template + byte-assert" pattern |
| Test pattern (component macro) | `templates/components/filter_tag.html` (story 9-4) tests — find their location and mirror | the FilterTag macro from 9-4 is the canonical precedent for component-macro testing |
| E2E spec | `tests/e2e/specs/journeys/loans-stack.spec.ts` (verify exists; if not, find the right file via `ls tests/e2e/specs/journeys/`) | append the new describe block here OR create a new spec if no logical home |
| E2E loginAs helper | `tests/e2e/helpers/auth.ts` | `loginAs(page, "librarian")` — typed union, do not pass other strings |
| E2E loan-creation helper | `tests/e2e/helpers/loans.ts::createLoan` | reuse for the AC11 fixture (creates volume + borrower + active loan in one call) |

### Anti-patterns to avoid

- **Reusing `LoanWithDetails` for the volume-detail surface.** That struct (story 9-5) carries `volume_label`, `title_name`, `id`, `duration_days` — fields the volume-detail page already has elsewhere on the page (via the `VolumeModel` + the title-name JOIN). A new narrow projection (`ActiveLoanWithBorrower`) keeps the SQL projection lean AND the contract focused on what this surface needs.
- **Fetching borrower data unconditionally and conditionally rendering it.** AC5 explicitly requires the SQL projection itself to be narrowed for Anonymous — the borrower's `name` MUST NOT travel through application memory if the user can't see it. This is stricter than the indicator subsystem's role-gate-at-call-site pattern (9-4..9-7) BECAUSE FR59 is a privacy-sensitive case. Two-layer defense: SQL projection narrowing + handler call-site branch.
- **String-splitting on `borrower_name` to insert the link inside an interpolated label.** The handler builds the label as 3 separate fields: prefix String + borrower-name (rendered as link by the macro) + suffix String. This avoids template-side string manipulation AND keeps the macro CSP-clean. Per the AC4 / AC6 / Task 3 implementation note.
- **`sqlx::query!` macro.** Forces `cargo sqlx prepare` regeneration in the PR. Use dynamic `query` / `query_as` (project convention; matches 9-2/9-3/9-4/9-5/9-6/9-7).
- **Calling `t!()` from inside the Askama macro.** Pre-translate in the handler, pass as `String` fields. Project convention; canonical example: `home.rs::home` post-9-7.
- **Inline `style="..."`, `<script>`, or `onclick=` in the new macro or `volume_detail.html` edits.** UX-DR24 mandates Tailwind utility classes resolving to `@theme` tokens. The amber palette (`bg-amber-100 dark:bg-amber-900/30`) is the model — copy verbatim from the existing `not_shelved_label` badge.
- **Logging `borrower_name` or `borrower_id` in `tracing::*` calls.** AC15 forbids it. The new model methods MUST log only non-PII fields (e.g., `volume_id`) on error paths.
- **Adding `loan_status` rendering to `title_detail.html` or `#browse-results` in 9-8.** Out of scope per AC14 — `title_detail.html` doesn't currently list individual volumes; search results show TitleCard not volume rows. The macro's API supports those future callers but no wiring is done in 9-8.
- **Refactoring `volume_detail.html` beyond the loan-status row.** Refactor-during-feature is anti-pattern. The Edit button block + the existing definition-list rows stay byte-identical.
- **Extracting a "VolumeBadge" component from the existing inline `not_shelved_label` span in `volume_detail.html:46-49`.** The spec text says "the existing VolumeBadge (UX-DR15) for shelved / unshelved" but no such template component exists today (the badge is inlined). Extracting it would be scope creep — flag as `type:code-review-finding` for a future "VolumeBadge component extraction" chore PR if a future story needs to reuse the location-status badge from another surface.

### Cross-story summary — surfaces touched + role-gating pattern recap

| Story | Surface | Role gating pattern |
|---|---|---|
| 9-4 | Home `#what-needs-attention` (Unshelved tag + list) | Symmetric (Librarian only) — call-site role gate; data fetched only when role >= Librarian. |
| 9-5 | Home `#overdue-list` | Symmetric — same as 9-4. |
| 9-6 | Home `#gaps-list` | **Asymmetric** — Anonymous-allowed via `gaps_filter_active` boolean; tag still Librarian-only (`role_gated_indicator_filter` + escape-hatch contract). |
| 9-7 | Home `#recent-cataloged-list` + `#recent-returns-list` | Symmetric — same as 9-4 (closes the indicator chapter). |
| **9-8** | **Volume-detail page (`/volume/:id`)** | **TWO-LAYER DEFENSE**: (1) SQL projection narrowed at the query layer for Anonymous (no JOIN to borrowers, no borrower fields projected); (2) Handler picks which method to call based on role. Stricter than the 9-4..9-7 pattern because FR59 is a privacy-sensitive case (borrower PII). |

### Architecture compliance

- **Error handling:** Any DB failure in the 2 new model methods returns `AppError::Database` via `?`; the handler soft-degrades with `tracing::warn!` + `loan_status = None` (badge omitted), matching the established 9-1..9-7 dashboard pattern. The volume-detail page MUST NOT 500 because the loan-status query had a hiccup.
- **Logging:** `tracing::warn!` at handler level on the soft-degrade paths; NEVER include `borrower_name` or `borrower_id` in any log (AC15). `tracing::debug!` only inside model functions if needed.
- **DB query discipline:** Every SELECT/JOIN of entity tables (`loans`, `borrowers`) MUST include `deleted_at IS NULL`. The 2 new queries inherit this pattern. The `b.deleted_at IS NULL` JOIN guard for the librarian query is ESSENTIAL (locks AC9c).
- **CSP middleware:** Already wraps the handler outermost. No work needed.
- **Pool access:** The handler already has `state.pool: DbPool`. No new connection.
- **One-branch-one-story (Foundation Rule #14):** Verify `git status` clean + on `main` + `git pull --ff-only` before starting; cut `story/9-8-loan-status-role-aware`. Open a draft PR (Rule #15) at the first commit.
- **Source-file-size limit (Foundation Rule #12):** `volume_detail.html` is at 62 LOC pre-9-8 → ~68 LOC post-9-8 (well below any ceiling). `src/routes/catalog.rs` LOC growth ≈ 40-60 LOC for the struct + handler + tests; verify with `wc -l` in Task 8. If `catalog.rs` was already over 2000 pre-9-8, this story does not make it worse by more than the strict minimum — file a follow-up extraction as `type:code-review-finding` if so.

### Library / framework requirements

- **Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + Tailwind CSS v4 + HTMX 2.0** — no version changes, no new dependencies.
- **rust_i18n** — already wired. Pre-translate the 5 new keys in the handler. Use `%{name}` / `%{date}` interpolation syntax (project convention; verified usable across the codebase via existing keys like `loans.html`'s `current_banner_with_volumes` etc.).
- **chrono** — already used (`NaiveDateTime` is the canonical type). Use the existing date format string from `templates/pages/loans.html` for consistency.
- **Askama macros** — existing pattern from `templates/components/filter_tag.html` (story 9-4) and `templates/components/cover.html`. The new `loan_status_badge.html` macro follows the same shape.

### File structure requirements

| File | Action | Rough size |
|---|---|---|
| `src/models/loan.rs` | **edit** | +60-80 LOC (`ActiveLoanWithBorrower` struct + `active_loan_summary_for_volume` + `active_loan_with_borrower_for_volume` async fns) |
| `src/routes/catalog.rs` | **edit** | +50-70 LOC (`LoanStatusView` struct + 5 new HomeTemplate fields + handler wiring + 4 render tests) |
| `templates/pages/volume_detail.html` | **edit** | +5 LOC (macro import + macro call wrapped in `{% if let Some(loan) = loan_status %}`) |
| `templates/components/loan_status_badge.html` | **create** | ~25 LOC (the macro per AC4 + Task 3) |
| `locales/en.yml` | **edit** | +5 lines under `volume:` |
| `locales/fr.yml` | **edit** | +5 lines under `volume:` |
| `tests/volume_detail_loan_status.rs` | **create** | ~200-250 LOC (7 `#[sqlx::test]` cases + helpers including `soft_delete_borrower`) |
| `tests/e2e/specs/journeys/loans-stack.spec.ts` (or new file) | **edit** | +30-50 LOC (1 new `test.describe` block, 1 smoke test) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **edit** | only the `9-8-...` line + `last_updated` (per CLAUDE.md rule 16) |
| `_bmad-output/implementation-artifacts/9-8-loan-status-role-aware.md` | **edit** | this file — Status, Tasks checked, Dev Agent Record on completion |

### Testing requirements

- **Coverage target:** every AC has at least one test (unit OR E2E). AC12 (CSP) is covered by `templates_audit.rs::no_inline_markup_in_templates` (existing — must stay green). AC13 (LOC) is informational verification in Task 8. AC15 (no-PII-in-logs) is covered by code review (no automated test).
- **AC8 HTML-name-leak regression guard** is THE LOAD-BEARING SECURITY TEST. The 2-render assertion shape (anonymous: `!html.contains("Alice")` + librarian: `html.contains("Alice")` to prove the test fixture would catch a leak) is mandatory.
- **AC9 SQL projection narrowing** is locked by 7 sqlx integration tests (4 for `_summary_`, 3 for `_with_borrower_`).
- **AC10 macro 4-cell matrix** locks the macro's parameterization contract.
- **E2E** keeps to 1 smoke test for parsimony — covers the full anonymous-vs-librarian role transition for a single volume.

### Project structure notes

This story exits the indicator-subsystem chapter (closed in 9-7) and moves to a new surface: the volume-detail page. Three intentional design decisions worth flagging:

1. **NEW projection struct (`ActiveLoanWithBorrower`) — NOT `LoanWithDetails` REUSE.** Story 9-5 introduced `LoanWithDetails` for the loans-page row template; story 9-7 reused it for `#recent-returns-list`. Here the surface is different: the volume-detail page already knows the volume and the title; the loan-status row only needs `borrower_id`, `borrower_name`, `loaned_at`. A new narrow struct keeps the projection lean AND the contract focused. Mirrors 9-6's `SeriesWithGap` decision (NEW narrow struct vs the wider existing model).

2. **TWO-LAYER role-gating defense — stricter than the indicator subsystem.** The indicator stories 9-4..9-7 role-gated only at the call site (data was always fetched, then conditionally rendered or hidden). Here the SQL projection itself is narrowed for Anonymous (no JOIN to `borrowers`, no `borrower_name` projected). This is BECAUSE FR59 is a privacy-sensitive case — the spec language mandates "no borrower data fetched for anonymous". Two-layer defense locks the contract AND minimizes the data-leak surface even in the face of a future templating bug.

3. **Macro `loan_status_badge.html` is forward-compatible across surfaces.** The spec text mentions "any caller (volume row, title detail, search result)". As of 9-8 close, only `volume_detail.html` has a volume-rendering surface that displays loan status. The macro's parameterized API supports future callers (e.g., a hypothetical "title-detail page lists volumes" story not currently scoped) without modification. NO additional caller wiring is done in 9-8 (refactor-during-feature anti-pattern).

4. **Drift discoveries to document at story close** (per the spec text claims):
   - `volume_detail.html` did NOT have any loan-status display pre-9-8 despite the spec text implying "the existing field". Story 9-8 ADDS the loan-status row from scratch.
   - `title_detail.html` does NOT list individual volumes today — only title metadata + contributors + series assignments. The macro's "shared across surfaces" intent is forward-looking only.
   - No `VolumeBadge` template component exists for the location/shelving status (the spec mentions UX-DR15 as if it were a template component). The location badge is INLINED in `volume_detail.html:46-49` (~4 LOC). Extracting it is OUT OF SCOPE for 9-8 — file as `type:code-review-finding` if a future story needs to reuse the location-status badge from another surface.

### Schema reality check (drift discoveries from spec text)

Drift discoveries this spec has factored in:

- **`volume_detail.html` has NO loan-status display today** — spec text "the volume row on `/title/:id` (or any volume-detail rendering)" implies an existing field; reality is the field is NEW.
- **`title_detail.html` does NOT list individual volumes** — spec text mentions volume rows on `/title/:id`; reality is the title-detail page shows metadata + contributors + series only. The macro's "shared across surfaces" intent is forward-looking; only `volume_detail.html` wires it in 9-8.
- **No `VolumeBadge` component exists** — spec mentions UX-DR15 as if it were extracted; the badge is inlined.
- **Loan/borrower schema verified** — `loans (volume_id, borrower_id, loaned_at, returned_at, deleted_at)` + `borrowers (id, name, deleted_at)`. The 3-condition active-loan filter is reused from `count_active` (9-5); the `b.deleted_at IS NULL` JOIN guard is the new safety invariant locked by AC9c.

If a fresh schema drift is discovered during dev (e.g., `loans` has a new column or constraint), document inline in the test helper AND in the Dev Agent Record's "drift discoveries" section.

## References

- [Story 9.8 spec — `_bmad-output/planning-artifacts/epics.md` lines 1336-1351](../planning-artifacts/epics.md)
- [PRD FR59 (loan status visibility — anonymous vs librarian/admin) — `_bmad-output/planning-artifacts/prd.md:693`](../planning-artifacts/prd.md)
- [UX-DR15 (VolumeBadge for shelved/unshelved — referenced but not extracted as a template component) — `_bmad-output/planning-artifacts/ux-design-specification.md`](../planning-artifacts/ux-design-specification.md)
- [Story 9-7 spec (canonical patterns: `.unwrap_or_else` soft-degrade, role-gated SQL fetch, REUSE-where-shape-fits, dynamic query/query_as) — `9-7-recent-activity-indicators.md`](./9-7-recent-activity-indicators.md)
- [Story 9-6 spec (canonical pattern: NEW narrow projection struct `SeriesWithGap` vs reuse-the-wider-model) — `9-6-series-with-gaps-indicator.md`](./9-6-series-with-gaps-indicator.md)
- [Story 9-5 spec (canonical pattern: `LoanWithDetails` REUSE, `count_active` 3-condition filter shape) — `9-5-overdue-loans-indicator.md`](./9-5-overdue-loans-indicator.md)
- [Story 9-4 spec (canonical pattern: FilterTag Askama macro — model for the new `loan_status_badge.html` macro) — `9-4-filtertag-and-unshelved-indicator.md`](./9-4-filtertag-and-unshelved-indicator.md)
- [`CLAUDE.md` — Foundation Rules (#1 DRY, #7 E2E smoke per epic, #11 GitHub Issues, #12 ≤ 2000 LOC, #13 local testing, #14 one-branch-one-story, #15 draft PR, #16 sprint-status ownership, #18 CI gating)](../../CLAUDE.md)
- [Loan schema — `migrations/20260329000000_initial_schema.sql:158-175` (`loans` table)](../../migrations/20260329000000_initial_schema.sql)
- [Borrower schema — `migrations/20260329000000_initial_schema.sql:147-156` (`borrowers` table)](../../migrations/20260329000000_initial_schema.sql)
- [Loan model — `src/models/loan.rs` (`LoanModel`, `LoanWithDetails`, `count_active`, `count_overdue`, `count_recent_returns`, etc.)](../../src/models/loan.rs)
- [Volume detail handler — `src/routes/catalog.rs:1859-1949` (`VolumeDetailTemplate` + `volume_detail` async fn)](../../src/routes/catalog.rs)
- [Volume detail template — `templates/pages/volume_detail.html` (62 LOC pre-9-8)](../../templates/pages/volume_detail.html)
- [Existing badge palette (location) — `templates/pages/volume_detail.html:46-49` (`not_shelved_label` amber span)](../../templates/pages/volume_detail.html)
- [FilterTag macro precedent — `templates/components/filter_tag.html` (story 9-4)](../../templates/components/filter_tag.html)
- [Cover macro precedent — `templates/components/cover.html`](../../templates/components/cover.html)
- [Dashboard integration test pattern — `tests/dashboard_recent_activity.rs` (story 9-7) + `tests/dashboard_overdue.rs` (story 9-5; sibling shape for `tests/volume_detail_loan_status.rs`)](../../tests/dashboard_overdue.rs)
- [Role enum — `src/middleware/auth.rs:13-18` (`Role::Anonymous < Role::Librarian < Role::Admin`)](../../src/middleware/auth.rs)
- [Borrower detail route (link target) — `src/routes/borrowers.rs` (existing `/borrower/:id`)](../../src/routes/borrowers.rs)
- [E2E loan helpers — `tests/e2e/helpers/loans.ts` (existing `createLoan`, `scanTitleAndVolume`, etc.)](../../tests/e2e/helpers/loans.ts)

## Dev Agent Record

### Agent Model Used

`claude-opus-4-7` (1M context).

### Debug Log References

_(to be filled by dev agent)_

### Completion Notes List

_(to be filled by dev agent)_

### File List

_(to be filled by dev agent)_

### Change Log

_(to be filled by dev agent)_
