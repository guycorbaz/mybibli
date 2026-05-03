# Story 9.5: Indicator — overdue loans

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a librarian,
I want an "Overdue loans" tag on the home page that shows the live count and lets me jump to a list of late returns,
so that I can quickly see and address loans that have passed the configured overdue threshold.

## Acceptance Criteria

1. **AC1 — Overdue tag joins the "What needs attention" section (librarian/admin only).** Given the home page (`/`) seen by a Librarian or Admin, when it renders, then the existing `#what-needs-attention` section (story 9-4) additionally displays an `id="filter-tag-overdue"` FilterTag pill — `"Overdue loans — N"` / `"Prêts en retard — N"` — where `N = COUNT(*) FROM loans WHERE returned_at IS NULL AND deleted_at IS NULL AND DATEDIFF(NOW(), loaned_at) > <threshold>`. The threshold is read from `AppSettings.overdue_threshold_days` via a NEW `AppState::overdue_threshold_days()` accessor (added in this story; mirrors `session_timeout_secs()`). When the section already exists with the unshelved tag, the overdue tag is appended in the visual order **Unshelved → Overdue** (matches the priority ordering finalized in story 9.7 AC: Unshelved → Overdue → Series with gaps → Recent cataloged → Recent returns).

2. **AC2 — Anonymous never sees the tag, never issues the query.** Given the home page rendered for an Anonymous role, when it renders, then `id="filter-tag-overdue"` is NOT present anywhere on the page AND the handler MUST NOT issue `loan::count_overdue` or `loan::list_overdue` for Anonymous — same two-layer defense pattern as 9-4 unshelved (no role-gated leak; no DB load on a surface the user can't see). Anonymous users crafting `/?filter=overdue` get the default home (filter is silently ignored — same path as the existing `parse_indicator_filter` no-op for Anonymous in `home.rs:132–136`).

3. **AC3 — Zero-count rule + active-state escape hatch (FilterTag contract).** Given `count_overdue` returns 0 AND no overdue filter is active, when the section renders, then the overdue tag is hidden (UX-DR4 zero-count rule, enforced by `build_indicator_tags`). Given `count_overdue` returns 0 AND `?filter=overdue` IS the active URL filter (e.g., the librarian just returned the last overdue loan from the filtered view), then the overdue tag IS still emitted in **active state** (label + ✕, href `/`) so the user has a visible escape hatch — same code-review patch contract that 9-4 added to the unshelved tag (`build_indicator_tags_zero_count_with_active_filter_still_emits_active_tag` / `filter_tag_macro_renders_active_pill_even_when_count_is_zero`). The `#what-needs-attention` section likewise hides only when `indicator_tags.is_empty()` after both unshelved + overdue contributions.

4. **AC4 — `IndicatorFilter::Overdue` enum variant + parser recognition.** The `IndicatorFilter` closed enum (introduced in 9-4 at `src/routes/home.rs:526–531`) gains a second variant `Overdue`. `parse_indicator_filter` recognizes `Some("overdue")` → `Some(IndicatorFilter::Overdue)` (case-sensitive, like `"unshelved"`). The 9-4 test `parse_indicator_filter_unknown_bare_name_returns_none` (lines 869–879) currently asserts `"overdue"` returns `None` with the comment `"overdue is reserved for story 9-5 — not yet recognized"` — this story **DELETES that line from the test** and adds a new positive test `parse_indicator_filter_overdue_recognized` mirroring `parse_indicator_filter_unshelved_recognized`. Anonymous-role handler MUST still treat the parser result as `None` for either variant (defensive — the role gate at `home.rs:132–136` is the primary check).

5. **AC5 — Single-active-filter precedence carries over.** AC7 from 9-4 (single active indicator filter; legacy `parse_filter`/`?q=`/`?sort=` ignored when an indicator is active; HTMX search-fragment branch naturally short-circuits) applies unchanged for `?filter=overdue`. The handler logic at `home.rs:132–162` already routes any `IndicatorFilter` variant through the same precedence path — no rewrite needed; only the `unshelved_filter_active`/`overdue_filter_active` slot booleans differ. **Mutual exclusion between indicator filters:** at most ONE of `{unshelved_filter_active, overdue_filter_active}` may be true at a time (the URL has one `?filter=` value; the parser returns one `IndicatorFilter` variant). Asserted by a new render test `home_librarian_overdue_filter_active_renders_overdue_list_not_unshelved_list_nor_recent_additions`.

6. **AC6 — Overdue filter swaps the recent-additions slot, mutually exclusive with unshelved-list.** Given a Librarian/Admin user navigating to `/?filter=overdue`, when the home page renders, then the `#recent-additions` section is REPLACED by an `#overdue-list` section in the SAME DOM position (the same slot 9-4 introduced for `#unshelved-list`). All three sections — `#recent-additions`, `#unshelved-list`, `#overdue-list` — are mutually exclusive in the rendered HTML; only ONE renders at a time. The overdue-list section shows:
   - Heading: `{{ overdue_heading }}` ("Overdue loans" / "Prêts en retard"), pre-translated by handler.
   - When `overdue_loans.is_empty()` (race-empty defensive path — count > 0 but list query returned 0; OR active-state escape hatch case where count = 0): the `{{ overdue_empty_label }}` copy ("No overdue loans — well done!" / "Aucun prêt en retard — bien joué !") inside the same section wrapper, mirroring 9-4's `#unshelved-list` empty-state shape.
   - When non-empty: a `<ul class="mt-3 space-y-2">` with one `<li>` per overdue loan (`LIMIT 100`). Each row is wrapped in `<a href="/borrower/{borrower_id}">` (NOT title — the librarian's intent is "who has it, contact them"; matches the loans-page row link target at `templates/pages/loans.html:110`) and shows: V-code label (e.g., `V0042`), title name, borrower name, `DATEDIFF(NOW(), loaned_at)` days as duration, and the existing color-coded duration treatment from `loans.html:114-118` (red `text-red-600` + `Overdue` badge when `duration_days >= overdue_threshold`; amber `text-amber-600` when `>= 14`; default otherwise) — by definition every row in this list satisfies the red threshold, but the row template stays uniform with the loans-page coloring rules so a future "all loans" surface can reuse the partial without divergence. Tailwind utility classes only (no inline styles).
   - The overdue tag in `#what-needs-attention` renders in active state (pill with ×).

7. **AC7 — Threshold change reflects on next request without restart.** Given the admin changes `overdue_loan_threshold_days` via `/admin?tab=system` (story 8-5 `save_loans_settings` handler at `src/routes/admin_system.rs:380–398`), when the home page is reloaded, then `count_overdue` and `list_overdue` use the new threshold immediately (no restart, no provider re-instantiation). This works because (a) `services::admin_system::reload_settings_cache` already re-reads `AppSettings` from DB and swaps the `Arc<RwLock<AppSettings>>` write-lock value, AND (b) the new `AppState::overdue_threshold_days()` accessor reads `.read()` per-request — same per-call read pattern as `state.session_timeout_secs()` / `state.google_books_api_key()`. The handler clones the i32 out of the read-guard BEFORE any `.await` (no guard held across await — see Foundation Rule guidance in `src/lib.rs:54-58`).

8. **AC8 — `loan::count_overdue` + `loan::list_overdue` model methods.** Two new functions on `LoanModel` (`src/models/loan.rs`), patterned after `count_active` (lines 297–304) and `list_active_by_borrower` (lines 252–291). Signatures:
   ```rust
   pub async fn count_overdue(pool: &DbPool, threshold_days: i32) -> Result<i64, AppError>
   pub async fn list_overdue(pool: &DbPool, threshold_days: i32, limit: u32) -> Result<Vec<LoanWithDetails>, AppError>
   ```
   - **`count_overdue`** SQL: `SELECT COUNT(*) FROM loans WHERE returned_at IS NULL AND deleted_at IS NULL AND DATEDIFF(NOW(), loaned_at) > ?`. `bind(threshold_days)`. Use `sqlx::query_as::<_, (i64,)>` (mirror `count_active`). **Strict-`>` boundary worked example:** with default `threshold_days = 30`, a loan made 30 days ago returns `DATEDIFF = 30`; `30 > 30` is `false` ⇒ NOT counted (FR48 wording: "exceeds this number of days"). A loan made 31 days ago returns `31 > 30 = true` ⇒ counted. The boundary test (`count_overdue_threshold_boundary` in AC12a) seeds 29/30/31-day loans and asserts only the 31-day loan flips the count.
   - **`list_overdue`** SQL: same JOINs as `list_active_by_borrower` (lines 256–268), but `WHERE l.returned_at IS NULL AND l.deleted_at IS NULL AND DATEDIFF(NOW(), l.loaned_at) > ?`, `ORDER BY l.loaned_at ASC` (oldest = most overdue first), `LIMIT ?` bound from the `limit` parameter. Return `Vec<LoanWithDetails>` (existing struct at lines 21–30 — has all the fields the row template needs: `borrower_id` for the link, `borrower_name`, `volume_label`, `title_name`, `loaned_at`, `duration_days`).
   - **Schema column:** the table column is `loans.loaned_at` (timestamp the loan was created), NOT `loans.borrowed_at` as the epics.md spec text says. Verified at `migrations/20260329000000_initial_schema.sql:158-175`.
   - **Index decision:** the schema defines `INDEX idx_loans_volume`, `INDEX idx_loans_borrower`, `INDEX idx_loans_deleted_at` but NO composite index on `(returned_at, loaned_at)`. For a personal-library scale (typically < 10k loan rows), the full-table scan with `WHERE returned_at IS NULL` is acceptable v1 — skip the index. If a real deployment ever shows the count query taking > 50ms, file a `type:change-request` GH Issue to add `INDEX idx_loans_overdue (returned_at, loaned_at)`. Do not add prematurely.
   - Both functions use **dynamic `query` / `query_as`** (NOT the macro `sqlx::query!`), per project convention — keeps the `.sqlx/` cache untouched, mirrors story 9-2/9-3/9-4 anti-pattern note.

9. **AC9 — `AppState::overdue_threshold_days()` accessor (NEW).** Add to `impl AppState` in `src/lib.rs`, immediately after `session_timeout_secs()` (line 54–59):
   ```rust
   /// Story 9-5: read the currently-configured overdue-loan threshold (days).
   /// Clones the scalar out of the `RwLock` so callers never hold the guard
   /// across `.await` points.
   pub fn overdue_threshold_days(&self) -> i32 {
       self.settings
           .read()
           .map(|s| s.overdue_threshold_days)
           .unwrap_or_else(|_| AppSettings::default().overdue_threshold_days)
   }
   ```
   This is the only valid call site for reading the threshold from `home.rs`. Do NOT inline `state.settings.read().unwrap().overdue_threshold_days` in the handler — that pattern persists in `src/routes/loans.rs:92` because it predates the `state.session_timeout_secs()` accessor convention; do NOT extend it. (Optional polish: a follow-up cleanup PR can migrate `loans.rs:92` to use the new accessor — file as `type:change-request` GH Issue at story close, do NOT include in this story.)

10. **AC10 — `build_indicator_tags` extended to take overdue inputs.** Update `build_indicator_tags` (in its post-Task 1 location at `src/routes/home_indicators.rs`) to accept overdue parameters AND emit the overdue tag after the unshelved tag. Recommended signature (extends in place, additive; visibility per AC15):
    ```rust
    pub(crate) fn build_indicator_tags(
        unshelved_count: i64,
        overdue_count: i64,
        active: Option<IndicatorFilter>,
        loc: &str,
    ) -> Vec<IndicatorTag>
    ```
    Behavior, in order:
    - Push the unshelved tag if `unshelved_count > 0 || active == Some(IndicatorFilter::Unshelved)` (unchanged from 9-4 contract).
    - Push the overdue tag if `overdue_count > 0 || active == Some(IndicatorFilter::Overdue)`. Fields: `label = t!("dashboard.attention.overdue_label")`, `count = overdue_count.max(0) as u64`, `filter_name = "overdue"`, `is_active = (active == Some(IndicatorFilter::Overdue))`, `clear_aria_label = t!("dashboard.attention.overdue_clear_aria")`.
    - Order is load-bearing (AC1 says Unshelved → Overdue). Asserted by `build_indicator_tags_emits_unshelved_before_overdue_when_both_present`.

11. **AC11 — i18n EN + FR.** Append four new keys to `dashboard.attention:` block (currently at `locales/en.yml:360-364` and `locales/fr.yml:360-364`):
    - `overdue_label` — EN: `"Overdue loans"`, FR: `"Prêts en retard"`
    - `overdue_clear_aria` — EN: `"Clear filter: Overdue loans"`, FR: `"Retirer le filtre : Prêts en retard"`
    - `overdue_heading` — EN: `"Overdue loans"`, FR: `"Prêts en retard"` (used as `#overdue-list` heading; same text as `overdue_label` but kept as a separate key so future copy can diverge — e.g., heading could become "Overdue loans · 5 items")
    - `overdue_empty` — EN: `"No overdue loans — well done!"`, FR: `"Aucun prêt en retard — bien joué !"` (used in `#overdue-list` empty/race-empty/active-zero state)
    - **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper — keys start at root. After editing, run `touch src/lib.rs && cargo build` to force the i18n proc-macro to re-read. The `i18n::audit::tests::all_t_keys_have_both_locales` test enforces EN/FR mirror.

12. **AC12 — Unit tests.**
    - **(a) `loan::count_overdue` (DB-backed `#[sqlx::test]` in NEW `tests/dashboard_overdue.rs`):**
        - `count_overdue_on_empty_db_returns_zero` — fresh schema, no loans, expect `0`.
        - `count_overdue_excludes_returned_and_soft_deleted` — seed: 3 active loans with `loaned_at = NOW() - INTERVAL 40 DAY` (overdue at default 30), 2 returned loans with the same age, 1 soft-deleted active loan also at 40 days; expect `3`.
        - `count_overdue_threshold_boundary` — seed: 1 loan at `NOW() - INTERVAL 30 DAY` exactly, 1 at 31 days, 1 at 29 days. Call with `threshold_days = 30`; expect `1` (only the 31-day loan is `> 30`). The strict `>` boundary is load-bearing — a `>=` boundary would flip the 30-day loan into the count and contradict the FR48/spec wording "exceeds this number of days".
        - `count_overdue_threshold_change_reflected` — seed 1 loan at 15 days. Call with `threshold_days = 30` → expect `0`. Call with `threshold_days = 7` → expect `1`. Asserts the threshold parameter actually drives the SQL.
    - **(b) `loan::list_overdue` (DB-backed, same file):**
        - `list_overdue_returns_in_loaned_at_asc_order_with_limit` — seed 5 loans with `loaned_at` at 35/36/37/38/39 days ago. Call `list_overdue(pool, 30, 3)`; assert exactly 3 rows in `loaned_at ASC` order (oldest first = 39 → 38 → 37 days). Verify the joined fields are populated (`borrower_name`, `volume_label`, `title_name`, `duration_days >= 35`).
        - `list_overdue_excludes_returned_and_soft_deleted` — same fixture as count test (b); call `list_overdue(pool, 30, 100)`; assert 3 rows, no returned/soft-deleted appears.
        - **Test-helper inserts:** the existing `tests/dashboard_unshelved.rs` defines `insert_volume_unshelved`, `insert_location`, etc. For `dashboard_overdue.rs`, define a sibling helper `insert_loan(pool, volume_id, borrower_id, days_ago)` that runs `INSERT INTO loans (volume_id, borrower_id, loaned_at) VALUES (?, ?, NOW() - INTERVAL ? DAY)` — same `created_at` determinism trick as `tests/dashboard_recent_additions.rs::insert_title_with_created_at`. Also need `insert_borrower(pool, name)` (mirror `insert_title`); `borrowers` schema is at migration `20260329000000_initial_schema.sql:147-156`.
    - **(c) `parse_indicator_filter` (extends `src/routes/home.rs::mod tests` at lines 819+):**
        - **NEW** `parse_indicator_filter_overdue_recognized` — assert `Some("overdue")` → `Some(IndicatorFilter::Overdue)`; mirror `parse_indicator_filter_unshelved_recognized` (lines 819–826) verbatim with the new variant.
        - **EDIT** `parse_indicator_filter_unknown_bare_name_returns_none` (lines 869–879): DELETE the `"overdue"` assertion + the trailing `"overdue is reserved for story 9-5 — not yet recognized"` comment. Keep `"nonsense"`. Add a new bare-name reservation (e.g., `"gaps"` for 9-6 reservation, with comment `"gaps is reserved for story 9-6 — not yet recognized"`) so the test still proves the warn-and-ignore path for unknown bare names.
        - Existing case-sensitive test (lines 828–841) should be extended: `parse_indicator_filter(&Some("OVERDUE".to_string()))` → `None` (closed enum, case-sensitive). Add this assertion alongside the existing UNSHELVED/Unshelved cases.
    - **(d) `build_indicator_tags` (extends `src/routes/home.rs::mod tests`):**
        - `build_indicator_tags_overdue_zero_unshelved_zero_no_active_returns_empty` — both 0, no active filter → `Vec::new()`.
        - `build_indicator_tags_overdue_nonzero_unshelved_zero_returns_overdue_only` — `(0, 5, None)` → 1 tag, `filter_name = "overdue"`, `count = 5`, `is_active = false`.
        - `build_indicator_tags_emits_unshelved_before_overdue_when_both_present` — `(3, 5, None)` → 2 tags, `tags[0].filter_name == "unshelved"`, `tags[1].filter_name == "overdue"` (order is load-bearing per AC10).
        - `build_indicator_tags_overdue_zero_count_with_active_filter_still_emits_active_tag` — `(0, 0, Some(IndicatorFilter::Overdue))` → 1 tag, `is_active = true`, `count = 0`. Mirrors 9-4's existing zero-count-active test (`build_indicator_tags_zero_count_with_active_filter_still_emits_active_tag`) — provides the AC3 escape-hatch contract for overdue.
        - `build_indicator_tags_unshelved_active_emits_overdue_in_default_state_when_count_nonzero` — `(0, 5, Some(IndicatorFilter::Unshelved))` → 2 tags (unshelved active-with-zero per 9-4 contract; overdue default state).
    - **(e) Handler render tests (extends `src/routes/home.rs::mod tests`):**
        - `home_anonymous_does_not_render_overdue_tag` — Anonymous role; assert `id="filter-tag-overdue"` is NOT present anywhere in the HTML (regression guard alongside the existing 9-4 anonymous-no-leak test). Also assert `id="overdue-list"` is NOT present.
        - `home_librarian_renders_overdue_tag_in_default_state_when_count_positive` — populated case: extend the `make_test_home_template_with_indicators` factory (line ~1058+) to take an additional `(overdue_count, overdue_filter_active, overdue_loans)` triple. Build a template with `indicator_tags = [unshelved=0, overdue=5]` and `overdue_filter_active = false`. Assert `id="filter-tag-overdue"` is present, `aria-label="Overdue loans: 5"`, `href="/?filter=overdue"`.
        - `home_librarian_overdue_tag_active_state_when_filter_applied` — same factory, `overdue_filter_active = true`, `indicator_tag.is_active = true`. Assert `href="/"`, the visible "×" is present, active-state aria-label uses the clear copy.
        - `home_librarian_overdue_filter_active_renders_overdue_list_not_unshelved_list_nor_recent_additions` — populated case (`overdue_filter_active = true`, populated `overdue_loans` Vec). Assert `id="overdue-list"` IS present AND `id="recent-additions"` is NOT present AND `id="unshelved-list"` is NOT present (mutual exclusion across all three slots in the same DOM position).
        - `home_librarian_overdue_filter_empty_renders_empty_label` — `overdue_filter_active = true`, EMPTY `overdue_loans` Vec. Assert `id="overdue-list"` IS present AND the empty-label copy ("No overdue loans — well done!" / "Aucun prêt en retard — bien joué !") appears inside that section.
        - `home_renders_overdue_tag_after_unshelved_in_attention_section` — populated case with both indicators non-zero. Use `slice_section` + DOM order assertion to verify `id="filter-tag-unshelved"` comes BEFORE `id="filter-tag-overdue"` in the HTML byte stream (mirrors 9-4 follow-up's `home_renders_what_needs_attention_above_collection_glance` / 9-2's `home_renders_glance_above_recent_additions` order-pinning pattern).
    - **(f) FilterTag macro reuse — no NEW macro tests.** The 9-4 macro (`templates/components/filter_tag.html`) is unchanged. Story 9-5 reuses it as-is. Adding macro tests for overdue-specific data would be redundant; the parameterization contract is already locked by 9-4's 4-state matrix tests.

13. **AC13 — E2E (Foundation Rule #7, librarian role).** Append a NEW `test.describe("Home page — Overdue loans indicator", ...)` block at the end of `tests/e2e/specs/journeys/home.spec.ts` (AFTER the 9-4 "What needs attention / Unshelved indicator" describe at lines 156-213):
    - **Test 1 — anonymous-no-leak:** load `/`; assert `#filter-tag-overdue` has count 0; assert `#overdue-list` has count 0. Navigate to `/?filter=overdue`; same assertions; assert `#recent-additions` is still visible (filter ignored). Mirrors 9-4 Test 1 shape.
    - **Test 2 — librarian smoke (conditional empty-DB short-circuit):** `await loginAs(page, "librarian")`; load `/`. Read the count of `#filter-tag-overdue`. If `count === 0` AND no overdue fixture exists, short-circuit with a green pass (same defensive pattern as 9-4 Test 2 lines 180-186 — CI seed DB may or may not have overdue loans depending on fixture freshness). If `count === 1`: assert tag visible, default state, `href="/?filter=overdue"`. Click it; `await page.waitForURL(/\/\?filter=overdue/)`; assert `#overdue-list` is present AND `#recent-additions` is NOT AND `#unshelved-list` is NOT. Click the active-state ✕ pill (`href="/"`); `await page.waitForURL(/\/$/)`; assert `#recent-additions` is back AND `#overdue-list` is gone.
    - Use i18n-aware regex matchers: `/Overdue loans|Prêts en retard/i`. NO `waitForTimeout` (CI grep gate, enforced by `.github/workflows/_gates.yml::e2e`).
    - Selectors scoped to `#what-needs-attention` and `#overdue-list` to avoid the unscoped-selector flake class flagged by 9-2/9-3.
    - **No threshold-change E2E.** The spec text "adjust threshold via /admin?tab=system → reload / → verify count updated" tempts a cross-page E2E, but it adds CI runtime + new admin-form interactions for marginal coverage over the unit test `count_overdue_threshold_change_reflected`. Skip the E2E threshold-change scenario; document the decision in the Dev Agent Record at story close. If post-merge UAT shows the threshold-change path is fragile, file a follow-up E2E story.

14. **AC14 — CSP compliance.** No `style="..."`, no `<style>`, no `<script>`, no `onclick=`, no inline event handlers in any template touched by this story. The overdue-list rows reuse the same Tailwind class palette as the loans-page row (`text-red-600 dark:text-red-400 font-semibold`, `bg-red-100 dark:bg-red-900/30`, etc.) — no new color tokens, no new CSS file. The `src/templates_audit.rs::no_inline_markup_in_templates` test (line 44) MUST stay green. The 9-4 FilterTag macro is reused verbatim — no template-component edits.

15. **AC15 — Foundation Rule #12 — split `src/routes/home.rs` BEFORE adding handler code.** `src/routes/home.rs` is at **1987 LOC** as of 9-4 close (post-review patches). Adding the 9-5 enum variant + parser update + `build_indicator_tags` extension + ~6 new template fields + ~10 new test cases will push it past 2000 LOC. **In this story, extract a new module `src/routes/home_indicators.rs`** containing: `IndicatorFilter` enum, `IndicatorTag` struct, `parse_indicator_filter` fn, `build_indicator_tags` fn, and their unit tests (the ~9 9-4 parser/build tests + the new 9-5 tests). **Visibility (load-bearing — the file won't compile without this):** ALL FOUR moved items become `pub(crate)` so `home.rs` can import them — the enum `pub(crate) enum IndicatorFilter`, the struct `pub(crate) struct IndicatorTag` (currently `pub` in home.rs; downgrade is safe since the only callers are inside `crate::routes`), and the two functions `pub(crate) fn parse_indicator_filter` / `pub(crate) fn build_indicator_tags`. The struct's fields stay `pub` (the Askama template needs to read them via field access through the `IndicatorTag` referenced from `HomeTemplate.indicator_tags`). In `home.rs`, replace the moved items with `use crate::routes::home_indicators::{IndicatorFilter, IndicatorTag, build_indicator_tags, parse_indicator_filter};` (no re-export needed — the test factory and HomeTemplate already reference the type by qualified path; an `use` at the top of `home.rs` is enough). Net result: `home.rs` shrinks by ~250-300 LOC (parser + helper + tests move out), creating headroom for stories 9-6/9-7 to add their own indicator parsers + helpers without re-doing the split. **Do NOT extract the home handler itself**, ONLY the indicator-filter machinery — the handler stays in `home.rs` as the page's main entry point. Add `pub mod home_indicators;` to `src/routes/mod.rs` (check `routes/mod.rs:1-10` for the registration convention used by sibling modules like `admin_reference_data.rs` / `admin_system.rs`; match it).

## Tasks / Subtasks

- [x] **Task 1 — Extract `src/routes/home_indicators.rs` to make room (AC: 15)**
  - [ ] Verify `home.rs` LOC: `wc -l src/routes/home.rs` (should be 1987 at story start).
  - [ ] Create `src/routes/home_indicators.rs`. Move: `IndicatorFilter` enum (lines 526–531), `IndicatorTag` struct (lines 538–553), `parse_indicator_filter` fn (lines 602–611), `build_indicator_tags` fn (lines 570–591), and ALL their unit tests from `mod tests` (lines 819–885 for the parser tests; the 3 `build_indicator_tags` tests).
  - [ ] In `home.rs`, replace the moved items with `use crate::routes::home_indicators::{IndicatorFilter, IndicatorTag, build_indicator_tags, parse_indicator_filter};`. Apply `pub(crate)` to all four moved items in the new file (per AC15 — the cross-module move requires it; the current `pub` on the enum + struct downgrades safely to `pub(crate)`).
  - [ ] Decide registration: read `src/routes/mod.rs` to see whether siblings like `admin_reference_data.rs`, `admin_system.rs` are registered as `pub mod ...;`. Match the convention. Most likely add `pub mod home_indicators;` after `pub mod home;`.
  - [ ] **STAYS in `src/routes/home.rs::mod tests`:** `slice_section`, `attention_section_slice`, `make_test_home_template_with_indicators`, `fake_indicator_tag`, `fake_unshelved_row`, and ALL handler-level render tests (they instantiate `HomeTemplate` with 60+ fields unrelated to the indicator-filter machinery). Only the 9-4 `parse_indicator_filter_*` (lines 819–885) and `build_indicator_tags_*` unit tests move to `home_indicators.rs::mod tests`.
  - [ ] `cargo check` after the extraction — must compile cleanly before adding ANY 9-5 logic. Lock the refactor as the first commit (clean diff: pure move, zero behavior change).
  - [ ] `cargo test` after the extraction — all 9-4 parser/build tests still pass. Lock as second commit if convenient.

- [x] **Task 2 — `loan::count_overdue` + `loan::list_overdue` model methods (AC: 8, 12a, 12b)**
  - [ ] In `src/models/loan.rs`, add `pub async fn count_overdue(pool: &DbPool, threshold_days: i32) -> Result<i64, AppError>` directly after `count_active` (line 297–304). Pattern: `sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM loans WHERE returned_at IS NULL AND deleted_at IS NULL AND DATEDIFF(NOW(), loaned_at) > ?").bind(threshold_days).fetch_one(pool).await?;` — return `row.0`.
  - [ ] Add `pub async fn list_overdue(pool: &DbPool, threshold_days: i32, limit: u32) -> Result<Vec<LoanWithDetails>, AppError>` mirroring `list_active_by_borrower` (lines 252–291). SQL JOINs identical (loans → borrowers → volumes → titles, all with `deleted_at IS NULL`). WHERE clause: `WHERE l.returned_at IS NULL AND l.deleted_at IS NULL AND DATEDIFF(NOW(), l.loaned_at) > ?`. ORDER BY `l.loaned_at ASC`. `LIMIT ?`. Bind `(threshold_days, limit)`. Map rows into `LoanWithDetails` exactly as `list_active_by_borrower` does (lines 274–288).
  - [ ] Both functions use **dynamic `query` / `query_as`** (NOT the macro `sqlx::query!`) — keeps `.sqlx/` cache untouched (project convention; see Story 9-2/9-3/9-4 anti-pattern note).
  - [ ] Build the integration test file `tests/dashboard_overdue.rs` (NEW, sibling of `dashboard_unshelved.rs`):
    - Helpers: copy `first_genre_id`, `first_volume_state_id`, `insert_title`, `insert_location`, `insert_volume_unshelved`, `insert_volume_at_location`, `soft_delete` from `tests/dashboard_unshelved.rs:20-110` (cross-file copy is acceptable per project precedent — both 9-4 and 9-3 already duplicate helpers across test files; a shared `tests/helpers.rs` module would be a follow-up cross-cutting story).
    - New helpers: `insert_borrower(pool, name) -> u64` (single INSERT into `borrowers` with `name`); `insert_loan(pool, volume_id, borrower_id, days_ago) -> u64` running `INSERT INTO loans (volume_id, borrower_id, loaned_at) VALUES (?, ?, NOW() - INTERVAL ? DAY)`; `mark_loan_returned(pool, loan_id)` running `UPDATE loans SET returned_at = NOW() WHERE id = ?`; `soft_delete_loan(pool, loan_id)` running `UPDATE loans SET deleted_at = NOW() WHERE id = ?`.
    - 6 `#[sqlx::test(migrations = "./migrations")]` cases per AC12a (4) + AC12b (2). Follow the file shape of `tests/dashboard_unshelved.rs` for the doc-comment header + helper layout.

- [x] **Task 3 — `IndicatorFilter::Overdue` variant + parser update (AC: 4, 12c)**
  - [ ] In `src/routes/home_indicators.rs` (post-Task 1 location), add `Overdue` variant to the `IndicatorFilter` enum:
    ```rust
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum IndicatorFilter {
        Unshelved,
        Overdue,
        // Reserved for follow-up: Gaps (9-6), RecentCataloged (9-7), RecentReturns (9-7).
    }
    ```
  - [ ] Update `parse_indicator_filter` to match `Some("overdue") => Some(IndicatorFilter::Overdue)` BEFORE the `!v.contains(':') && !v.is_empty()` warn-and-ignore arm:
    ```rust
    pub(crate) fn parse_indicator_filter(filter: &Option<String>) -> Option<IndicatorFilter> {
        match filter.as_deref() {
            Some("unshelved") => Some(IndicatorFilter::Unshelved),
            Some("overdue") => Some(IndicatorFilter::Overdue),
            Some(v) if !v.contains(':') && !v.is_empty() => {
                tracing::warn!(filter = %v, "Unknown indicator filter, ignoring");
                None
            }
            _ => None,
        }
    }
    ```
  - [ ] **EDIT** the existing `parse_indicator_filter_unknown_bare_name_returns_none` test (was at `home.rs:869-879`, moved to `home_indicators.rs` in Task 1): DELETE the assertion `parse_indicator_filter(&Some("overdue".to_string())) == None` AND the trailing `"overdue is reserved for story 9-5 — not yet recognized"` comment. Replace with a new reservation: `parse_indicator_filter(&Some("gaps".to_string()))` → `None` with comment `"gaps is reserved for story 9-6 — not yet recognized"`. The test still proves the warn-and-ignore path on unknown bare names; it just shifts the reservation forward by one story.
  - [ ] **NEW** `parse_indicator_filter_overdue_recognized` test mirroring `parse_indicator_filter_unshelved_recognized`:
    ```rust
    #[test]
    fn parse_indicator_filter_overdue_recognized() {
        assert_eq!(
            parse_indicator_filter(&Some("overdue".to_string())),
            Some(IndicatorFilter::Overdue)
        );
    }
    ```
  - [ ] **EXTEND** the existing `parse_indicator_filter_case_sensitive` test (was at `home.rs:828-841`): add assertions `parse_indicator_filter(&Some("OVERDUE".to_string())) == None` and `parse_indicator_filter(&Some("Overdue".to_string())) == None`.

- [x] **Task 4 — `build_indicator_tags` extension (AC: 10, 12d)**
  - [ ] In `src/routes/home_indicators.rs`, extend `build_indicator_tags` to accept `overdue_count: i64` as the 2nd parameter (between `unshelved_count` and `active`):
    ```rust
    pub(crate) fn build_indicator_tags(
        unshelved_count: i64,
        overdue_count: i64,
        active: Option<IndicatorFilter>,
        loc: &str,
    ) -> Vec<IndicatorTag> {
        let mut tags = Vec::new();
        let unshelved_is_active = active == Some(IndicatorFilter::Unshelved);
        if unshelved_count > 0 || unshelved_is_active {
            tags.push(IndicatorTag {
                label: rust_i18n::t!("dashboard.attention.unshelved_label", locale = loc).to_string(),
                count: unshelved_count.max(0) as u64,
                filter_name: "unshelved".to_string(),
                is_active: unshelved_is_active,
                clear_aria_label: rust_i18n::t!("dashboard.attention.unshelved_clear_aria", locale = loc).to_string(),
            });
        }
        let overdue_is_active = active == Some(IndicatorFilter::Overdue);
        if overdue_count > 0 || overdue_is_active {
            tags.push(IndicatorTag {
                label: rust_i18n::t!("dashboard.attention.overdue_label", locale = loc).to_string(),
                count: overdue_count.max(0) as u64,
                filter_name: "overdue".to_string(),
                is_active: overdue_is_active,
                clear_aria_label: rust_i18n::t!("dashboard.attention.overdue_clear_aria", locale = loc).to_string(),
            });
        }
        tags
    }
    ```
  - [ ] **UPDATE** the 3 existing `build_indicator_tags_*` tests in `home_indicators.rs` (moved from `home.rs:1058+` in Task 1) to pass `0` as the new `overdue_count` param. They keep the same assertions (only the unshelved tag is exercised).
  - [ ] **NEW** unit tests per AC12d (5 cases). Place after the existing 9-4 tests so the file reads as a chronological extension.
  - [ ] Update the `home::home` handler's call site (line ~235) to pass the new `overdue_count` arg.

- [x] **Task 5 — `AppState::overdue_threshold_days()` accessor (AC: 9)**
  - [ ] In `src/lib.rs`, add the `overdue_threshold_days` method to `impl AppState` immediately after `session_timeout_secs` (line 54-59). Doc-comment per AC9.
  - [ ] Unit test in `src/lib.rs` (or in `src/config.rs::tests` — pick the location that already houses `AppSettings` round-trip tests): construct `AppSettings { overdue_threshold_days: 14, ..Default::default() }`, wrap in `Arc<RwLock>`, build a minimal `AppState`, assert `state.overdue_threshold_days() == 14`. Pattern: search `cargo test test_app_state\|test_session_timeout_secs` to find an existing AppState/AppSettings test factory; if none exists, the simplest assertion is `let s = AppSettings { overdue_threshold_days: 14, ..Default::default() }; assert_eq!(14, s.overdue_threshold_days);` plus a separate test that the lock-read-and-clone pattern works. (Optional — the access is a 5-line copy of `session_timeout_secs` and the manual smoke tests will catch any regression. Skip if the existing AppState mock is non-trivial; document the skip in the Dev Agent Record.)

- [x] **Task 6 — Wire the home handler (AC: 1, 2, 5, 6, 7)**
  - [ ] In `src/routes/home.rs::home`, immediately after the existing `unshelved_volumes` block (~line 249), add the overdue data fetching:
    ```rust
    // Story 9-5 — Overdue loans indicator. Same anonymous-skip + soft-degrade
    // pattern as unshelved (AC2 no-leak, AC7 threshold-from-cache). Threshold
    // read once per request via the AppState accessor (clones the i32 out of
    // the read-guard, no .await held inside the lock).
    let overdue_threshold = state.overdue_threshold_days();
    let overdue_count: i64 = if session.role >= Role::Librarian {
        match crate::models::loan::LoanModel::count_overdue(pool, overdue_threshold).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %e, "count_overdue failed; rendering 0 (tag hidden)");
                0
            }
        }
    } else {
        0
    };
    let overdue_filter_active =
        session.role >= Role::Librarian && active_indicator_filter == Some(IndicatorFilter::Overdue);
    let overdue_loans: Vec<crate::models::loan::LoanWithDetails> = if overdue_filter_active {
        match crate::models::loan::LoanModel::list_overdue(pool, overdue_threshold, 100).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "list_overdue failed; rendering empty list");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    ```
  - [ ] Update the `build_indicator_tags` call (line ~235) to pass `overdue_count` as the new 2nd arg: `let indicator_tags = build_indicator_tags(unshelved_count, overdue_count, active_indicator_filter, loc);`
  - [ ] Extend `HomeTemplate` (struct at lines 31-98) with the new fields:
    - `pub overdue_filter_active: bool` — drives the AC6 swap.
    - `pub overdue_loans: Vec<crate::models::loan::LoanWithDetails>` — empty when `overdue_filter_active = false`; populated when active.
    - `pub overdue_heading: String` — pre-translated section heading.
    - `pub overdue_empty_label: String` — pre-translated empty-state copy.
    - `pub overdue_threshold_days: i64` — passed to the row template for the red-vs-amber color treatment (mirrors `loans.html`'s `overdue_threshold` field at `routes/loans.rs:69 + 133`). Use `i64` (not i32) to match the loans.html template's existing comparison type.
    - `pub days_label: String` — pre-translated "days" / "jours" label for the duration column (existing `loan.days` key at `locales/en.yml:393`).
    - `pub overdue_badge_label: String` — pre-translated "Overdue" / "En retard" badge label for the red row badge (existing `loan.overdue` key at `locales/en.yml:412`).
  - [ ] Pre-translate all four new labels in the handler (mirrors the existing pattern at lines ~423: `attention_heading: rust_i18n::t!("dashboard.attention.heading", locale = loc).to_string()`).
  - [ ] **AC5 mutual exclusion** falls out for free: `unshelved_filter_active` and `overdue_filter_active` cannot both be true (the URL has one `?filter=` value; the parser returns one variant). The template's `{% if %}{% elif %}{% else %}` chain enforces it visually. No new handler logic needed.

- [x] **Task 7 — Render `#overdue-list` section in `home.html` (AC: 6, 14)**
  - [ ] Edit `templates/pages/home.html` lines 124-187 (the existing `{% if unshelved_filter_active %}{% else %}` block). Convert to a 3-branch `{% if %}{% elif %}{% else %}` chain:
    ```jinja
    {% if unshelved_filter_active %}
    <section id="unshelved-list" ...> ... </section>     {# UNCHANGED — story 9-4 #}
    {% elif overdue_filter_active %}
    <section id="overdue-list" aria-labelledby="overdue-list-heading" class="w-full max-w-4xl mt-6">
        <h2 id="overdue-list-heading" class="text-sm font-medium text-stone-600 dark:text-stone-400 uppercase tracking-wide">{{ overdue_heading }}</h2>
        {% if overdue_loans.is_empty() %}
            <div class="text-center py-12 text-stone-500 dark:text-stone-400">{{ overdue_empty_label }}</div>
        {% else %}
            <ul class="mt-3 space-y-2">
                {% for loan in overdue_loans %}
                <li>
                    <a href="/borrower/{{ loan.borrower_id }}" class="block px-4 py-3 bg-stone-50 dark:bg-stone-800 hover:bg-stone-100 dark:hover:bg-stone-700 rounded-lg border border-stone-200 dark:border-stone-700 transition-colors">
                        <div class="flex items-baseline gap-3 flex-wrap">
                            <span class="font-mono text-sm font-semibold text-indigo-600 dark:text-indigo-400 tabular-nums">{{ loan.volume_label }}</span>
                            <span class="font-medium text-stone-900 dark:text-stone-100 truncate">{{ loan.title_name }}</span>
                            <span class="text-sm text-stone-600 dark:text-stone-400">— {{ loan.borrower_name }}</span>
                        </div>
                        <p class="mt-1 text-sm {% if loan.duration_days >= overdue_threshold_days %}text-red-600 dark:text-red-400 font-semibold{% elif loan.duration_days >= 14 %}text-amber-600 dark:text-amber-400{% endif %}">
                            {{ loan.duration_days }} {{ days_label }}
                            {% if loan.duration_days >= overdue_threshold_days %}
                            <span class="ml-1 inline-flex items-center rounded-full bg-red-100 dark:bg-red-900/30 px-2 py-0.5 text-xs font-medium text-red-700 dark:text-red-300">{{ overdue_badge_label }}</span>
                            {% endif %}
                        </p>
                    </a>
                </li>
                {% endfor %}
            </ul>
        {% endif %}
    </section>
    {% else %}
    <section id="recent-additions" ...> ... </section>     {# UNCHANGED — story 9-2 #}
    {% endif %}
    ```
  - [ ] CSP: zero `style="..."`, zero `<script>`, zero `onclick=`. The duration-color treatment is class-driven exactly like `loans.html:114-118`. The `templates_audit::no_inline_markup_in_templates` test (line 44) MUST stay green after this change.

- [x] **Task 8 — i18n keys (AC: 11)**
  - [ ] In `locales/en.yml`, append to the existing `dashboard.attention:` block (after `unshelved_empty:` at line 364):
    ```yaml
        overdue_label: Overdue loans
        overdue_clear_aria: "Clear filter: Overdue loans"
        overdue_heading: Overdue loans
        overdue_empty: "No overdue loans — well done!"
    ```
  - [ ] In `locales/fr.yml`, mirror at the same path (after the FR `unshelved_empty:` at line 364):
    ```yaml
        overdue_label: Prêts en retard
        overdue_clear_aria: "Retirer le filtre : Prêts en retard"
        overdue_heading: Prêts en retard
        overdue_empty: "Aucun prêt en retard — bien joué !"
    ```
  - [ ] **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper — keys start at root. After editing, run `touch src/lib.rs && cargo build`. The `i18n::audit::tests::all_t_keys_have_both_locales` test enforces EN/FR mirror — keep them aligned exactly.

- [x] **Task 9 — Tests (AC: 12, 13)**
  - [ ] **`tests/dashboard_overdue.rs`** (new sibling file per Task 2): 6 `#[sqlx::test]` cases (4 count + 2 list).
  - [ ] **`src/routes/home_indicators.rs::mod tests`** (post-Task 1 location): the `parse_indicator_filter_overdue_recognized` + `parse_indicator_filter_case_sensitive` extension + 5 new `build_indicator_tags_*` cases per Task 3 + Task 4 lists.
  - [ ] **`src/routes/home.rs::mod tests`**: 6 new handler render tests per AC12e. Extend the `make_test_home_template_with_indicators` factory to take overdue inputs (or add `make_test_home_template_with_overdue_indicator` if the parameter list grows uncomfortable). Reuse `slice_section` + `attention_section_slice`.
  - [ ] **`tests/e2e/specs/journeys/home.spec.ts`**: append `test.describe("Home page — Overdue loans indicator", …)` block per AC13. 2 tests (anonymous + librarian-with-empty-DB-short-circuit).

- [x] **Task 10 — Verify and document (AC: 1–15)**
  - [ ] `wc -l src/routes/home.rs` — verify the file is BELOW 2000 LOC after the Task 1 extraction + 9-5 additions. If still above, deepen the extraction (e.g., move `make_test_home_template_with_indicators` factory + slice helpers into `home_indicators.rs::mod tests` or a sibling test-helpers module). Foundation Rule #12 must hold.
  - [ ] `SQLX_OFFLINE=true cargo check && cargo clippy --all-targets -- -D warnings` — clean (zero warnings).
  - [ ] `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test` — full suite green. Expected: ~671 lib tests baseline + ~14 new = ~685 lib; +6 new integration tests in `tests/dashboard_overdue.rs` (going from 4 to 10 in the dashboard_* family). All 9-4 dashboard_unshelved + 9-1/9-2/9-3 dashboard_* unchanged.
  - [ ] `cargo sqlx prepare --check --workspace` — expected no diff (Tasks 2 + 3 + 4 use dynamic `query` / `query_as`).
  - [ ] Tailwind rebuild — `npx @tailwindcss/cli -i static/css/input.css -o static/css/output.css --minify`. Verify any new utility classes used (`flex-wrap` is widespread; nothing new expected).
  - [ ] Manual smoke from a running dev instance (`MYBIBLI_SKIP_SETUP=1 cargo run`):
    - As anonymous: `curl http://localhost:8080/` and grep — `id="filter-tag-overdue"` MUST NOT appear; `id="overdue-list"` MUST NOT appear.
    - As librarian (login first): `curl` with the session cookie → grep — `id="filter-tag-overdue"` appears IFF overdue count > 0 OR filter is active.
    - Click the tag in a browser → URL changes to `/?filter=overdue` → `#overdue-list` replaces `#recent-additions` → click ✕ → URL returns to `/` → recent-additions back. Repeat with the unshelved tag to confirm mutual exclusion (only one list slot can be active at a time).
    - Threshold-change smoke: change `overdue_loan_threshold_days` via `/admin?tab=system` → reload `/` → count reflects the new threshold (no restart).
  - [ ] **E2E** (Foundation Rule #13) — `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/home.spec.ts`. Same `tests/e2e/test-results/` ownership-blocker caveat as 9-1/9-2/9-3/9-4 may apply locally; CI on the story branch is the source of truth.
  - [ ] Update Dev Agent Record at the bottom of this file: list of files touched, decisions on placement (top vs between sections), anything surprising (drift discoveries, schema gotchas).
  - [ ] Update `_bmad-output/implementation-artifacts/sprint-status.yaml`: `9-5-overdue-loans-indicator: ready-for-dev → in-progress` at start, `→ review` at end (only this line + `last_updated`, per CLAUDE.md rule 16).
  - [ ] Open draft PR at first commit (Foundation Rule #15). Title: `Story 9-5: Overdue loans indicator (#NN)`.

## Dev Notes

### Source tree references

| Concern | File / location | Notes |
|---|---|---|
| Home route registration | `src/routes/mod.rs:87` | `.route("/", axum::routing::get(home::home))` — no change |
| Home handler | `src/routes/home.rs:115-440` (post-9-4) | extend the existing handler; do NOT create a parallel route |
| Home template | `templates/pages/home.html` (~313 lines post-9-4) | extends `layouts/base.html`; sections: search (14-33), filter pills (36-65), metadata error (67-74), `#what-needs-attention` (83-92), `#collection-glance` (96-122), the AC6 mutually-exclusive 3-branch slot at lines 124-187 (`#unshelved-list` / `#overdue-list` / `#recent-additions`), `#stats-by-genre` (189+), browse toggle, `#browse-results` |
| Insertion point for `#overdue-list` | `templates/pages/home.html` lines 124-187 — convert the `{% if unshelved_filter_active %}{% else %}` to a 3-branch chain | AC6 mutual exclusion across all three sections |
| Loan schema | `migrations/20260329000000_initial_schema.sql:158-175` | column is `loaned_at` (NOT `borrowed_at` as spec text says); only `idx_loans_volume`, `idx_loans_borrower`, `idx_loans_deleted_at` exist — no composite index for overdue scan |
| Loan model | `src/models/loan.rs` (435 LOC) | extend with `count_overdue` (mirror `count_active` at lines 297-304) + `list_overdue` (mirror `list_active_by_borrower` at lines 252-291) |
| Loan struct (return type) | `src/models/loan.rs:21-30` | `LoanWithDetails` — already has all fields needed (`borrower_id`, `borrower_name`, `volume_label`, `title_name`, `loaned_at`, `duration_days`); REUSE, do NOT create a new struct |
| `AppSettings.overdue_threshold_days` | `src/config.rs:156` (struct field), `src/config.rs:203` (Default = 30), `src/config.rs:237-242` (load_from_db parsing of `overdue_loan_threshold_days` row) | already wired since story 8-5; story 9-5 only ADDS the AppState accessor |
| `AppSettings` admin write path | `src/services/admin_system.rs:21` (`KEY_OVERDUE_THRESHOLD`), `src/services/admin_system.rs:38` (`validate_overdue_threshold`), `src/routes/admin_system.rs:380-398` (`save_loans_settings`) | unchanged — admin form already writes & reloads the cache via `reload_settings_cache` |
| `AppState` accessor pattern | `src/lib.rs:50-68` | `session_timeout_secs()` is the canonical pattern for `overdue_threshold_days()` |
| `IndicatorFilter` enum + parser | `src/routes/home.rs:526-611` (post-9-4); MOVED to `src/routes/home_indicators.rs` in Task 1 | extended with `Overdue` variant + `Some("overdue")` arm |
| `IndicatorTag` view-model | `src/routes/home.rs:538-553`; MOVED in Task 1 | unchanged |
| `build_indicator_tags` helper | `src/routes/home.rs:570-591`; MOVED in Task 1 | extended to accept `overdue_count` 2nd param |
| Single-active-filter precedence (AC5/AC7) | `src/routes/home.rs:128-162` (post-9-4) | already routes ANY `IndicatorFilter` variant through the same precedence path — no rewrite |
| Soft-degrade pattern | `src/routes/home.rs:224-249` (unshelved count + list with `tracing::warn!` + 0 / Vec::new() on error) | replicate verbatim for overdue |
| Loans-page row coloring (model) | `templates/pages/loans.html:107-132` | duration-day color treatment + Overdue badge — REUSE the same Tailwind class palette in `#overdue-list` rows |
| Loans-page row data | `src/routes/loans.rs:78-146` | precedent for inline `state.settings.read().unwrap().overdue_threshold_days` (do NOT extend; use the new accessor) |
| FilterTag macro | `templates/components/filter_tag.html` (post-9-4) | unchanged — reuse via `{% call filter_tag::tag(...) %}{% endcall %}` |
| HomeTemplate struct | `src/routes/home.rs:31-98` (post-9-4) | extend with 7 new fields (overdue_filter_active, overdue_loans, overdue_heading, overdue_empty_label, overdue_threshold_days, days_label, overdue_badge_label) |
| Test factory | `src/routes/home.rs::mod tests::make_test_home_template_with_indicators` (~line 1058 post-9-4) | extend signature to take `(overdue_count, overdue_filter_active, overdue_loans, overdue_threshold_days)` OR add a sibling `make_test_home_template_with_overdue` factory if the param list explodes |
| Slice helpers | `src/routes/home.rs::mod tests::slice_section`, `attention_section_slice` (~line 990 post-9-4) | reuse for the new render assertions; add `overdue_section_slice` if needed |
| i18n locales | `locales/en.yml:360-364`, `locales/fr.yml:360-364` (`dashboard.attention:` block) | append `overdue_label`, `overdue_clear_aria`, `overdue_heading`, `overdue_empty` |
| Existing loan i18n keys (REUSED) | `locales/en.yml:393` (`loan.days`), `locales/en.yml:412` (`loan.overdue`) | reused via `days_label` and `overdue_badge_label` template fields |
| i18n audit | `src/i18n/audit.rs::all_t_keys_have_both_locales` | enforces EN/FR mirror |
| Templates audit | `src/templates_audit.rs::no_inline_markup_in_templates` (line 44) | must stay green |
| Test pattern (DB-backed integration) | `tests/dashboard_unshelved.rs` (story 9-4) | sibling file — `#[sqlx::test(migrations = "./migrations")]`, file-local helpers, `wipe_seeded_genres` clone |
| Test pattern (handler render, no DB) | `src/routes/home.rs::mod tests` (post-9-4) | reuse the slice + factory pattern verbatim |
| E2E spec for `/` | `tests/e2e/specs/journeys/home.spec.ts` (4 describes post-9-4) | extend with the new "Overdue loans indicator" describe AFTER the 9-4 "What needs attention / Unshelved indicator" block |
| E2E loginAs helper | `tests/e2e/helpers/auth.ts` | `loginAs(page, "librarian")` — typed union, do not pass other strings |

### Anti-patterns to avoid

- **Inlining `state.settings.read().unwrap().overdue_threshold_days`** in the home handler. AC9 mandates the new `state.overdue_threshold_days()` accessor for safe lock-and-clone. The `loans.rs:92` precedent is legacy from before the accessor pattern existed; do NOT extend it. (A separate `type:change-request` GH Issue can migrate `loans.rs:92` later.)
- **Issuing the overdue queries for Anonymous users.** AC2 forbids it (no role-gated leak; loans are Librarian-content). The handler MUST short-circuit BEFORE the count query when `session.role < Role::Librarian` — same two-layer defense as 9-4 unshelved.
- **Holding the `RwLockReadGuard` across an `.await` point.** AC9's accessor clones the i32 out of the guard before returning. If you read the threshold inline (against AC9's directive) and then `.await` the count query, the guard is held — clippy will flag it; CI will fail.
- **Adding a composite index `(returned_at, loaned_at)` "just in case".** AC8 explicitly defers this — personal-library scale doesn't need it. CLAUDE.md "Don't add features beyond what the task requires."
- **Creating a new `OverdueLoanRow` struct.** `LoanWithDetails` already has every field the row template needs (`borrower_id`, `borrower_name`, `volume_label`, `title_name`, `loaned_at`, `duration_days`). A new struct would be a parallel type with the same shape — the kind of duplication 9-4 explicitly avoided when it kept `UnshelvedVolumeRow` separate from `SearchResult` (different shape there). Here the shapes match — REUSE.
- **Coexisting `#recent-additions` AND `#unshelved-list` AND `#overdue-list` in the rendered HTML.** AC6 mandates 3-way mutual exclusion in the same DOM position. The template's `{% if %}{% elif %}{% else %}` chain enforces this; render tests assert it byte-by-byte.
- **`>= threshold_days` instead of `> threshold_days`.** AC8 boundary test (`count_overdue_threshold_boundary`) locks the strict `>` semantic per FR48 wording "exceeds this number of days". A 30-day-old loan with default `threshold = 30` is NOT overdue — only 31+ days qualifies. (Side note: `loans.html:114-118` currently uses `>=` for the row coloring threshold; the count query uses `>`. This asymmetry is intentional — the count is the formal indicator, the coloring is a visual approximation that lights up on the boundary day. Do NOT "fix" the row template's `>=` to `>` — that change deserves its own UX review.)
- **`sqlx::query!` macro.** Forces `cargo sqlx prepare` regeneration in the PR. Use dynamic `query` / `query_as` (project convention; see Story 9-2/9-3/9-4 anti-pattern note).
- **Calling `t!()` from inside the Askama template.** Pre-translate in the handler, pass as `String` fields. Project convention; canonical example: `src/routes/home.rs:303-320`.
- **Inline `style="..."` for the red/amber row coloring.** UX-DR24 mandates Tailwind utility classes resolving to `@theme` tokens. The pattern `text-red-600 dark:text-red-400 font-semibold` from `loans.html:114` is the model — copy verbatim.
- **HTMX swap target invention.** AC8 of story 9-4 explicitly deferred HTMX boost; 9-5 inherits the plain-`<a href>` decision verbatim. Plain links work without JS; the FilterTag macro emits them; the handler renders the new state from the URL on the next request.
- **Running `list_overdue` when `overdue_filter_active = false`.** Handler must guard: `if overdue_filter_active { LoanModel::list_overdue(...) } else { Vec::new() }`. Wasteful otherwise — the 50-row JOIN doesn't need to fire on every home page load.
- **Pushing past 2000 LOC in `src/routes/home.rs`.** Foundation Rule #12. AC15 is non-negotiable — the Task 1 extraction is the FIRST commit on the branch, before any 9-5 logic lands.

### Threshold semantic — count vs row color (load-bearing asymmetry)

The overdue threshold is read once per request via `state.overdue_threshold_days()` (AC9) and propagated to TWO surfaces with INTENTIONALLY DIFFERENT comparison operators:

- **Count + list query (`count_overdue` / `list_overdue`):** strict `>` — `DATEDIFF(NOW(), loaned_at) > threshold_days`. A loan whose age exactly equals the threshold is NOT overdue. Locked by AC8 worked example + the `count_overdue_threshold_boundary` test (AC12a). FR48 wording "exceeds this number of days" is the spec authority.
- **Row coloring (`<p class="… {% if loan.duration_days >= overdue_threshold_days %}…">`):** inclusive `>=` — copied verbatim from `templates/pages/loans.html:114`. A loan whose age exactly equals the threshold lights up red in the row but is NOT in the count.

This is a **deliberate, established asymmetry** (it predates this story; the loans page has shipped this way since Epic 4). Do NOT "fix" the row template's `>=` to `>` to "match the count" — that change deserves its own UX review and would make every row in `#overdue-list` paradoxically un-colored at the boundary day. Story 9-5 inherits the asymmetry verbatim.

The amber band (`>= 14`) is also inherited from `loans.html:114` and is hard-coded (not configurable). If a future story makes the warn-band threshold admin-configurable, it would be its own story.

### Architecture compliance

- **Error handling:** Any DB failure in `count_overdue` / `list_overdue` returns `AppError::Database` via `?`; the handler soft-degrades with `tracing::warn!` + `0` (count) or `Vec::new()` (list), per the established 9-1/9-2/9-3/9-4 pattern. The home page MUST NOT 500 because the indicator query had a hiccup.
- **Logging:** `tracing::warn!` at handler level on the soft-degrade paths; `tracing::debug!` only inside model functions if needed. The single-active-filter `tracing::warn!` at `home.rs:137-143` (post-9-4) ALREADY covers the `?filter=overdue` + `?q=foo` collision case — no new log statement needed there. Indicator-related state changes are not interesting at info-level.
- **DB query discipline:** Every SELECT/JOIN of entity tables (`loans`, `borrowers`, `volumes`, `titles`) MUST include `deleted_at IS NULL`. The `count_overdue` query filters loans only; `list_overdue` extends to all four tables. Mirror the existing `list_active_by_borrower` JOIN shape.
- **Threshold accessor + `Arc<RwLock<AppSettings>>` discipline:** `state.overdue_threshold_days()` clones the scalar out of the read-guard; the guard is dropped before the function returns. The handler stores the cloned i32 in a local `let overdue_threshold` variable BEFORE calling `count_overdue` (which `.await`s) — no guard held across `.await`. This is the canonical safe pattern; clippy lints (e.g., `await_holding_lock`) would fail CI if violated.
- **HTMX coexistence:** the `#overdue-list` (or `#unshelved-list` or `#recent-additions`) sections sit OUTSIDE `#browse-results` (HTMX swap target) — same invariant as 9-1/9-2/9-3/9-4. Plain `<a href>` navigation does not interact with the existing HTMX search-fragment branch.
- **CSP middleware:** Already wraps the handler outermost. No work needed.
- **Pool access:** The handler already has `state.pool: DbPool`. Do not introduce a new connection.
- **One-branch-one-story (Foundation Rule #14):** Verify `git status` clean + on `main` + `git pull --ff-only` before starting; cut `story/9-5-overdue-loans-indicator`. Open a draft PR (Rule #15) at the first commit (the Task 1 extraction).
- **Source-file-size limit (Foundation Rule #12):** `src/routes/home.rs` is at **1987 LOC** post-9-4 follow-ups. AC15 mandates extracting the indicator-filter machinery to `src/routes/home_indicators.rs` BEFORE adding any 9-5 code. Net effect: home.rs shrinks by ~250-300 LOC, then 9-5 adds ~150 LOC of handler code + 7 template fields = home.rs comfortably below 1900 LOC at story close. `home_indicators.rs` lands at ~300-400 LOC.

### Library / framework requirements

- **Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + Tailwind CSS v4 + HTMX 2.0** — no version changes, no new dependencies.
- **rust_i18n** — already wired. Pre-translate the four new keys (`overdue_label`, `overdue_clear_aria`, `overdue_heading`, `overdue_empty`) in the handler via `rust_i18n::t!(…).to_string()`. No `count =` interpolation needed (the badge displays raw integer).
- **MariaDB `DATEDIFF()` semantics:** `DATEDIFF(NOW(), loaned_at)` returns the integer number of days between the two dates (calendar-day arithmetic, not 24h floor). This matches the established `loans.html` / `loans.rs` semantics — a loan made today shows `0 days`, a loan made yesterday shows `1 day`. **`DATEDIFF` is timezone-naive — both arguments are interpreted in the server's local timezone.** This matches the existing `duration_days` calculation at `loan.rs:112` (`DATEDIFF(NOW(), l.loaned_at) AS duration_days`); no behavior change.
- **Askama macros** — the FilterTag component is unchanged from 9-4; reuse via `{% call filter_tag::tag(...) %}{% endcall %}`. No new template engine features.

### File structure requirements

| File | Action | Rough size |
|---|---|---|
| `src/routes/home.rs` | **edit** | -250 to -300 LOC (Task 1 extraction) +150 LOC (9-5 handler + template fields + render tests) ≈ -100 LOC net; final ~1850-1900 LOC |
| `src/routes/home_indicators.rs` | **create** | ~300-400 LOC (the moved enum + struct + parser + helper + tests + new 9-5 tests) |
| `src/routes/mod.rs` | **edit** | +1 line (`pub mod home_indicators;` after `pub mod home;`) |
| `src/models/loan.rs` | **edit** | +60-80 LOC (`count_overdue` + `list_overdue` async fns) |
| `src/lib.rs` | **edit** | +8 LOC (`overdue_threshold_days()` accessor + doc-comment) |
| `templates/pages/home.html` | **edit** | +35-40 LOC (the `{% elif overdue_filter_active %}` branch + the `<section id="overdue-list">` body) |
| `locales/en.yml` | **edit** | +4 lines under `dashboard.attention:` |
| `locales/fr.yml` | **edit** | +4 lines under `dashboard.attention:` |
| `tests/dashboard_overdue.rs` | **create** | ~180-220 LOC (6 `#[sqlx::test]` cases + helpers including `insert_borrower` + `insert_loan` + `mark_loan_returned` + `soft_delete_loan`) |
| `tests/e2e/specs/journeys/home.spec.ts` | **edit** | +50-70 LOC (1 new `test.describe` block, 2 tests) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **edit** | only the `9-5-...` line + `last_updated` (per CLAUDE.md rule 16) |
| `_bmad-output/implementation-artifacts/9-5-overdue-loans-indicator.md` | **edit** | this file — Status, Tasks checked, Dev Agent Record on completion |

### Testing requirements

- **Coverage target:** every AC has at least one test (unit OR E2E). AC14 (CSP) is covered by `templates_audit.rs::no_inline_markup_in_templates` (existing — must stay green). AC15 (LOC) is verified by the `wc -l` step in Task 10.
- **AC2 anonymous-no-leak** is the load-bearing security invariant. The render test `home_anonymous_does_not_render_overdue_tag` is the primary regression guard; the E2E anonymous test is a secondary integration-level guard.
- **AC4 enum + parser update:** the `parse_indicator_filter_overdue_recognized` test is the primary positive guard. The `parse_indicator_filter_unknown_bare_name_returns_none` test (with the `"overdue"` line removed and `"gaps"` added) keeps the warn-and-ignore branch covered. Without the test edit, CI would flag a regression on the `"overdue is reserved for story 9-5"` assertion AS SOON AS Task 3 lands the parser change — order matters in the same commit.
- **AC6 mutual exclusion** of `#recent-additions` vs `#unshelved-list` vs `#overdue-list` is the load-bearing layout invariant. The render test `home_librarian_overdue_filter_active_renders_overdue_list_not_unshelved_list_nor_recent_additions` is the regression guard.
- **AC7 threshold change reflected** is covered by the unit test `count_overdue_threshold_change_reflected` (AC12a). E2E coverage of the admin-form → home-page reload chain is intentionally skipped (AC13 rationale: marginal value over the unit test, +CI runtime).
- **AC8 strict `>` boundary** is locked by `count_overdue_threshold_boundary` — without it, a future "make boundary inclusive" suggestion would slip past CI silently.
- **AC10 emit order** — `build_indicator_tags_emits_unshelved_before_overdue_when_both_present` is the regression guard. Without it, swapping the two `if` blocks in `build_indicator_tags` (e.g., a future "alphabetize" refactor) would break the priority ordering with no test failing.
- **E2E** keeps to 1 anonymous + 1 librarian smoke test for parsimony. The librarian test is conditional on seed-DB overdue loans existing; same defensive pattern as 9-2/9-3/9-4.

### Project structure notes

This story aligns cleanly with the patterns 9-4 established. Three intentional design decisions worth flagging:

1. **`src/routes/home_indicators.rs` extraction is a hard prerequisite, not a polish choice.** Foundation Rule #12 caps source files at 2000 LOC and `home.rs` is at 1987. Without the extraction, the file becomes a bottleneck for stories 9-6 (gaps) and 9-7 (recent cataloged + recent returns), each of which adds another enum variant + parser arm + helper extension + handler block + tests. Sequencing the extraction in 9-5's first commit creates room for three more indicator stories without revisiting LOC discipline. The extraction itself is a pure code move (zero behavior change), so the diff is reviewable as a single commit.

2. **`LoanWithDetails` REUSE (not a new `OverdueLoanRow` struct).** Story 9-4 created `UnshelvedVolumeRow` because `SearchResult` was title-centric and unshelved is volume-centric. Here the situation reverses: `LoanWithDetails` already has every field needed (`borrower_id`, `borrower_name`, `volume_label`, `title_name`, `loaned_at`, `duration_days`). A new struct would be parallel-shape duplication — the kind that triggers a `simplify` skill review. REUSE is the right call.

3. **`AppState::overdue_threshold_days()` accessor is the cleanup-payoff for the 8-5 → 9-5 → future-9-7 chain.** Story 8-5 introduced `state.session_timeout_secs()` / `state.default_language()` / `state.google_books_api_key()` as the canonical "read scalar from RwLock without holding guard across await" pattern. Story 9-5 is the first new story to need that pattern for the overdue threshold. Adding the accessor here costs ~8 LOC and unlocks the same safety contract for stories 9-6/9-7 (which may also read the threshold for date-window indicators) and a future cleanup of `loans.rs:92`. The `loans.rs:92` cleanup is OUT OF SCOPE for this story — file as `type:change-request` GH Issue at story close so it gets done in a focused PR with its own clippy/test cycle, not folded into a polish-heavy 9-5 PR.

4. **UX-DR5 "LoanRow" is a UX SPEC, not a template component.** `_bmad-output/planning-artifacts/ux-design-specification.md:1726` describes LoanRow as a row behavior bundle (scan-to-highlight, color-coded duration, contextual `[Return]` button) for the `/loans` page — implementation is INLINED into `templates/pages/loans.html:107-132`, NOT factored into `templates/components/loan_row.html`. The epics.md 9-5 spec text saying "each row uses the LoanRow variant of UX-DR5" reads as a component reference but is in fact a behavior reference. Story 9-5 follows the established pattern: inline the row markup directly inside `#overdue-list` (matching `loans.html`'s class palette + duration coloring verbatim). Do NOT create `templates/components/loan_row.html` in this story — the dashboard list and the loans-page table differ enough (no `<table>`, no `[Return]` button, link to borrower not to row-action) that a forced shared partial would just paper over the differences with conditionals. If a future "shared LoanRow component" story arises (e.g., when borrower-detail's loan list grows similar enough to justify the abstraction), that's its own story.

5. **HomeTemplate field-count growth — deferred cleanup signal.** Pre-9-4 HomeTemplate had ~55 fields. 9-4 added 6 more (~61). 9-5 adds 7 more (~68). 9-6 will add 5+ (gaps), 9-7 will add 5+ × 2 indicators (recent cataloged + recent returns). By 9-7 close, HomeTemplate may have 80+ fields — the kind of struct that's hard to `Default::default()` in tests and easy to forget when adding a new field. Worth flagging now so it's on the radar: a future `DashboardSlots` substruct (or per-indicator `IndicatorPanel { active, items, heading, empty_label }` clusters) would tame the field count. Out of scope for THIS story (refactor-during-feature is anti-pattern); if 9-6 or 9-7 push starts to hurt, file `type:change-request` then.

The 9-4 FilterTag macro precedent stays the model for indicator rendering — no template-component edits, only data-side wiring + a new HTML branch in `home.html`.

### Schema reality check (drift discovery summary from 9-4 onward)

Drift discoveries from 9-4 + early 9-5 inspection that this spec already factors in:

- `volumes.location_id` (NOT `storage_location_id` — already locked by 9-4).
- `loans.loaned_at` (NOT `borrowed_at` as the epics.md 9-5 spec text says — locked by AC8 of this story).
- `storage_locations.label` is `CHAR(5)`; `storage_locations.name` is `NOT NULL` no default — the dashboard_unshelved.rs helpers from 9-4 handle this and dashboard_overdue.rs's `insert_borrower` / `insert_loan` helpers don't touch storage_locations directly so no new gotcha here.
- No composite index on `(returned_at, loaned_at)` — explicitly NOT added in this story (AC8 rationale).

If a fresh schema drift is discovered during dev (e.g., `borrowers.name` has a tighter constraint than expected), document inline in the test helper AND in the Dev Agent Record's "drift discoveries" section.

## References

- [Story 9.5 spec — `_bmad-output/planning-artifacts/epics.md` lines 1279–1297](../planning-artifacts/epics.md)
- [Epic 9 scope note + indicator delivery split philosophy — `epics.md` lines 1200–1206](../planning-artifacts/epics.md)
- [Story 9.7 visual order definition (Unshelved → Overdue → Series with gaps → Recent cataloged → Recent returns) — `epics.md` line 1330](../planning-artifacts/epics.md)
- [PRD FR58 (actionable indicators), FR48 (overdue threshold), FR74 (admin configures threshold) — `_bmad-output/planning-artifacts/prd.md`](../planning-artifacts/prd.md)
- [UX-DR4 (FilterTag dual state, zero-count rule) — `_bmad-output/planning-artifacts/ux-design-specification.md`](../planning-artifacts/ux-design-specification.md)
- [UX-DR5 (LoanRow — scan-to-highlight, color-coded duration, contextual [Return] button — IS A UX SPEC, NOT A TEMPLATE COMPONENT; implementation lives inline in `loans.html:107-132`) — `ux-design-specification.md:1726-1742`](../planning-artifacts/ux-design-specification.md)
- [Story 9-4 spec (canonical patterns: FilterTag macro, IndicatorFilter enum + parser, build_indicator_tags helper, anonymous-no-leak two-layer defense, AC6 mutual-exclusion section swap, AC7 single-active-filter precedence, code-review escape-hatch patch) — `9-4-filtertag-and-unshelved-indicator.md`](./9-4-filtertag-and-unshelved-indicator.md)
- [Story 9-3 spec (canonical patterns: view-model struct in `home.rs`, render test factory extension, slice helpers, hide-entirely template gating, `wipe_seeded_genres` test helper) — `9-3-dashboard-stats-by-genre.md`](./9-3-dashboard-stats-by-genre.md)
- [Story 9-2 spec (canonical patterns: enriched single-round-trip projection, `created_at` determinism via `INTERVAL ? MINUTE`, sibling integration test file pattern) — `9-2-dashboard-recent-additions.md`](./9-2-dashboard-recent-additions.md)
- [Story 9-1 spec (canonical patterns: handler-side i18n, single round-trip, soft-degrade on DB error) — `9-1-dashboard-global-stats-card.md`](./9-1-dashboard-global-stats-card.md)
- [Story 8-5 spec (AppState accessor pattern, K/V settings discipline, `Arc<RwLock<AppSettings>>` reload-on-save) — `8-5-system-settings.md`](./8-5-system-settings.md)
- [`CLAUDE.md` — Foundation Rules (#1 DRY, #7 E2E smoke per epic, #11 GitHub Issues, #12 ≤ 2000 LOC, #13 local testing, #14 one-branch-one-story, #15 draft PR, #16 sprint-status ownership, #18 CI gating)](../../CLAUDE.md)
- [Loan schema — `migrations/20260329000000_initial_schema.sql:158-175` (column is `loaned_at`)](../../migrations/20260329000000_initial_schema.sql)
- [Loan model — `src/models/loan.rs` (`LoanModel`, `LoanWithDetails`, `count_active`, `list_active`, `list_active_by_borrower`)](../../src/models/loan.rs)
- [Loans page row pattern (duration coloring + Overdue badge) — `templates/pages/loans.html:107-132`](../../templates/pages/loans.html)
- [Existing AppState accessors — `src/lib.rs:50-104`](../../src/lib.rs)
- [Existing `parse_indicator_filter` (closed enum, `:` heuristic) — `src/routes/home.rs:602-611` (will move to `home_indicators.rs` in Task 1)](../../src/routes/home.rs)
- [Admin system threshold form — `src/routes/admin_system.rs:380-398` (`save_loans_settings` + `reload_settings_cache`)](../../src/routes/admin_system.rs)
- [FilterTag macro precedent — `templates/components/filter_tag.html` (story 9-4)](../../templates/components/filter_tag.html)
- [Dashboard integration test pattern — `tests/dashboard_unshelved.rs` (story 9-4)](../../tests/dashboard_unshelved.rs)

## Dev Agent Record

### Agent Model Used

`claude-opus-4-7` (1M context).

### Debug Log References

- **2026-05-02 — Task 1 extraction visibility tweak.** Initial pass set `IndicatorTag` to `pub(crate)` per AC15, but the publicly-reachable `HomeTemplate.indicator_tags: Vec<IndicatorTag>` field tripped the `private_interfaces` warning (clippy `-D warnings` would fail CI). Resolution: kept `IndicatorTag` as `pub` (matches its `Vec<IndicatorTag>` field's reachability); enum + functions stay `pub(crate)` per spec. Documented inline in `home_indicators.rs`.
- **2026-05-02 — Task 2 volume-label width.** Initial `dashboard_overdue.rs` helpers built labels like `"V95001"` (6 chars), tripping `volumes.label CHAR(5)`. Refactored `make_loan_fixture` to take a numeric `seq: u32` and format as `"V{seq:04}"` — keeps each test's labels under the column width with full uniqueness inside the per-test fresh DB.
- **2026-05-02 — Task 9 LOC trim.** After the 6 new render tests + factory + handler block landed, `home.rs` reached 2048 LOC — 48 over Foundation Rule #12. Trimmed by removing the sibling `make_test_home_template_with_overdue` factory (used post-construction field assignment instead — same pattern as `make_test_home_template_with_recent`) and tightening doc-comments on the new render tests. Final: `home.rs` at 1962 LOC.

### Completion Notes List

- ✅ All 10 tasks complete with all 15 ACs satisfied.
- ✅ `home.rs` LOC: 1962 (under the 2000-LOC Foundation Rule #12 ceiling). Net change vs pre-9-5: -25 LOC (Task 1 extraction created -216 LOC of room; 9-5 substance + tests added +191 LOC).
- ✅ `home_indicators.rs`: 336 LOC. The Task 1 commit was a pure code move (zero behavior change); 9-5 substance landed in subsequent commits per the spec's commit-by-commit plan.
- ✅ Lib tests: 682 passing (was 671 pre-9-5; +6 new render tests in `home::tests`, +5 new helper tests in `home_indicators::tests`).
- ✅ Integration tests: 96 passing across all `tests/*.rs` binaries — including the 6 new `dashboard_overdue.rs` cases (4 count + 2 list, per AC12a/AC12b).
- ✅ E2E: 1 new `test.describe("Home page — Overdue loans indicator", ...)` block in `home.spec.ts` with 2 tests (anonymous-no-leak + librarian smoke with conditional empty-DB short-circuit, mirroring 9-4 Test 2 shape).
- ✅ Clippy: clean with `-D warnings` across `--all-targets`.
- ✅ `.sqlx/` cache: unchanged — Task 2 + 3 + 4 use dynamic `query` / `query_as` per project convention.
- ✅ Templates audit (`no_inline_markup_in_templates`, CSP allowlist, CSRF coverage): all green.
- ✅ Tailwind: no rebuild needed — every utility class used in the new `#overdue-list` section (`flex-wrap`, `text-red-600`, `text-amber-600`, `bg-red-100`, `tabular-nums`, etc.) already present in compiled `output.css`.
- ⚠️ Local `cargo sqlx prepare --check` blocked by unavailable dev-DB credentials in this environment; CI on the story branch runs the check with proper credentials. No `query!` macros added in this story, so the cache cannot drift.

**Drift discoveries (none new — all anticipated by the spec):**
- `volumes.label` is `CHAR(5)` — confirmed; helper `make_loan_fixture` formats sequence numbers as 4-digit zero-padded V-codes.
- `loans.loaned_at` is the column name (NOT `borrowed_at` as the epics.md spec text says) — confirmed by AC8 + matches the existing 4-1 `LoanModel` shape.
- No composite index on `(returned_at, loaned_at)` — confirmed; deliberately not added per AC8 rationale.

**Key design decisions, mostly inherited from 9-4 + spec:**
1. `LoanWithDetails` REUSE (no new struct) — the existing struct already has every field the row template needs (`borrower_id`, `borrower_name`, `volume_label`, `title_name`, `loaned_at`, `duration_days`).
2. Strict `>` boundary on `DATEDIFF(NOW(), loaned_at) > threshold_days` (FR48 wording "exceeds this number of days") — pinned by `count_overdue_threshold_boundary` (29/30/31-day fixture, threshold=30, expect count=1). Asymmetric with the row coloring's inclusive `>=` — load-bearing inheritance from the loans page; documented in the spec's "Threshold semantic" note.
3. Anonymous role short-circuits BEFORE the count query (AC2 two-layer defense). Same pattern as 9-4 unshelved.
4. Threshold accessor `state.overdue_threshold_days()` clones the `i32` out of the `RwLock` read-guard before the handler `.await`s on the count query — no guard held across `.await` points (clippy `await_holding_lock` would catch a regression).
5. `#overdue-list` row link target is `/borrower/<id>` (NOT `/title/<id>`) — AC6 rationale "who has it, contact them"; matches the loans-page row link target.
6. 3-way mutual exclusion (`#recent-additions` / `#unshelved-list` / `#overdue-list`) implemented as a single `{% if %}{% else if %}{% else %}` chain in `home.html` — exactly one section renders per response.
7. Test factory: NO new sibling `make_test_home_template_with_overdue` — spec offered both options, chose post-construction field assignment to keep `home.rs` LOC budget headroom.
8. E2E threshold-change scenario intentionally skipped per AC13 (covered by `count_overdue_threshold_change_reflected` unit test; +CI runtime + admin-form interactions for marginal coverage).

### File List

**Created:**
- `src/routes/home_indicators.rs` (Task 1 extraction + Task 3 + Task 4 9-5 substance, 336 LOC)
- `tests/dashboard_overdue.rs` (Task 2 — 6 `#[sqlx::test]` cases + helpers, 274 LOC)

**Modified:**
- `src/routes/mod.rs` (+1 line: `pub mod home_indicators;`)
- `src/routes/home.rs` (Task 1 net -216 LOC; Task 6 + 9 net +191 LOC; final 1962 LOC)
- `src/models/loan.rs` (Task 2: +`count_overdue` + `list_overdue` async fns)
- `src/lib.rs` (Task 5: +`overdue_threshold_days()` accessor on `AppState`)
- `templates/pages/home.html` (Task 7: +`{% else if overdue_filter_active %}` branch + `#overdue-list` section body)
- `locales/en.yml` (Task 8: +4 keys under `dashboard.attention`)
- `locales/fr.yml` (Task 8: +4 keys under `dashboard.attention`)
- `tests/e2e/specs/journeys/home.spec.ts` (Task 9: +1 describe block, 2 tests)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (Rule 16: 9-5 line in-progress → review + `last_updated`)
- `_bmad-output/implementation-artifacts/9-5-overdue-loans-indicator.md` (Status, Tasks checked, Dev Agent Record)

### Change Log

- **2026-05-02** — Story 9-5 dev-story: 4 commits across the 10 tasks.
  - Commit 1 (Task 1): Pure-move extraction of `home_indicators.rs` from `home.rs`. Foundation Rule #12 headroom for the rest of Epic 9.
  - Commit 2 (Task 2): `LoanModel::count_overdue` + `LoanModel::list_overdue` + 6 `#[sqlx::test]` cases in `tests/dashboard_overdue.rs`.
  - Commit 3 (Tasks 3 + 4 + 8): `IndicatorFilter::Overdue` variant + parser arm + extended `build_indicator_tags` (overdue_count param) + 5 new helper unit tests + EN/FR i18n keys.
  - Commit 4 (Tasks 5 + 6 + 7 + 9): `AppState::overdue_threshold_days()` accessor; home handler overdue wiring with anonymous short-circuit + soft-degrade; `HomeTemplate` +7 fields; 3-branch mutual-exclusion chain in `home.html` with `#overdue-list` section; 6 new render tests in `home::mod tests`; E2E describe block in `home.spec.ts`.
