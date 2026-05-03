# Story 9.7: Indicators — recent cataloged + recent returns

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a librarian,
I want two indicator tags on the home page that surface what I just cataloged and what readers just returned in the last 7 days,
so that I can review the most recent activity in one click without scrolling through the full catalog or loans page.

## Acceptance Criteria

1. **AC1 — Two tags join the "What needs attention" section (librarian/admin only).** Given the home page (`/`) seen by a Librarian or Admin, when it renders, then the existing `#what-needs-attention` section additionally displays TWO `IndicatorTag` pills: `"Recent cataloged — N"` / `"Catalogués récemment — N"` AND `"Recent returns — N"` / `"Retours récents — N"`. Visual order across all 5 indicators is **Unshelved → Overdue → Series with gaps → Recent cataloged → Recent returns** (priority by actionability — "needs action" before "review"; matches the spec's section ordering finalized at `epics.md:1330`). Asserted by a new render test `home_renders_all_five_indicator_tags_in_priority_order`.

2. **AC2 — Both filters are Librarian-only (back to symmetric role gating; AC2 of 9-6 was the asymmetric exception).** Given an Anonymous user crafts `/?filter=recent-cataloged` or `/?filter=recent-returns`, when handled, then the filter is ignored AND no count query is issued AND no list query is issued (recent activity is Librarian-gated content; titles created today / loans returned today MAY contain Librarian-only context like draft titles or Borrower names). Implemented via the existing `routes::home_indicators::role_gated_indicator_filter` (story 9-6) which strips ALL variants for Anonymous — no per-variant work needed. The handler's `recent_cataloged_filter_active` and `recent_returns_filter_active` slot booleans are ALSO role-gated (mirroring 9-4 unshelved + 9-5 overdue, NOT the 9-6 Gaps asymmetry). The render tests `home_anonymous_does_not_render_recent_cataloged_tag` and `home_anonymous_does_not_render_recent_returns_tag` (with `?filter=` variants both ignored) lock the contract.

3. **AC3 — Zero-count rule + active-state escape hatch.** Given `count_recent_cataloged` returns 0 AND no recent-cataloged filter is active, the tag is hidden (UX-DR4). Given count = 0 AND `?filter=recent-cataloged` IS active (e.g., the librarian just refreshed past midnight when the 7-day window flipped), the tag IS still emitted in **active state** so the user has a visible escape hatch — same pattern as 9-4/9-5/9-6 escape-hatch contract. Same applies to `recent_returns`. The `#what-needs-attention` section hides only when `indicator_tags.is_empty()` after all 5 contributions.

4. **AC4 — `IndicatorFilter::RecentCataloged` + `IndicatorFilter::RecentReturns` enum variants + parser recognition.** The `IndicatorFilter` closed enum (introduced in 9-4, extended in 9-5/9-6) gains TWO new variants. `parse_indicator_filter` in `src/routes/home_indicators.rs` recognizes `Some("recent-cataloged")` → `Some(IndicatorFilter::RecentCataloged)` AND `Some("recent-returns")` → `Some(IndicatorFilter::RecentReturns)` (case-sensitive, like the others). The 9-6 test `parse_indicator_filter_unknown_bare_name_returns_none` (currently at `home_indicators.rs:240-250`) reserves `"recent-cataloged"` — this story **DELETES that line** (now recognized) AND removes the reservation entirely (no remaining indicator filters are reserved for future stories — 9-7 is the last indicator story in Epic 9). The fallback assertion `"nonsense"` stays to keep the warn-and-ignore branch covered. **EXTEND** the `parse_indicator_filter_case_sensitive` test with assertions for `"RECENT-CATALOGED"`, `"Recent-Cataloged"`, `"RECENT-RETURNS"`, `"Recent-Returns"` — all → `None`.

5. **AC5 — Single-active-filter precedence holds (no rewrite needed).** AC5 from 9-4 (single active indicator filter; legacy `parse_filter`/`?q=`/`?sort=` ignored when an indicator is parsed; HTMX search-fragment branch naturally short-circuits) applies unchanged. The handler logic at `home.rs:175-211` (post-9-6 P1 patch) already routes ANY `IndicatorFilter` variant through the same role-blind precedence path (`parsed_indicator.is_some()`) — no rewrite needed. **6-way mutual exclusion (NEW invariant):** at most ONE of `{unshelved_filter_active, overdue_filter_active, gaps_filter_active, recent_cataloged_filter_active, recent_returns_filter_active}` may be true at a time (the URL has one `?filter=` value; the parser returns one `IndicatorFilter` variant). Asserted by a new render test `home_librarian_recent_cataloged_filter_active_renders_only_recent_cataloged_list_section` (and counterpart for recent_returns).

6. **AC6 — Each filter swaps the recent-additions slot with its own list section, mutually exclusive with the other 4.** The existing 4-branch chain in `templates/pages/home.html` (post-9-6 — `#unshelved-list` / `#overdue-list` / `#gaps-list` / `#recent-additions`) becomes a **6-branch chain** by inserting two new `{% else if %}` branches between gaps and recent-additions:
   - `{% else if recent_cataloged_filter_active %}` → renders `<section id="recent-cataloged-list">` with the `recent_cataloged_titles: Vec<SearchResult>` list.
   - `{% else if recent_returns_filter_active %}` → renders `<section id="recent-returns-list">` with the `recent_returns: Vec<LoanWithDetails>` list.
   The recent-cataloged list reuses the `<article class="title-card">` markup verbatim from `#recent-additions` (lines 199-218 — same TitleCard shape, just bound to the new `recent_cataloged_titles` Vec instead of `recent_additions`). The recent-returns list reuses the `<a href="/borrower/{borrower_id}"><div>...<p>...</p></div></a>` row markup verbatim from `#overdue-list` (lines 165-181) — but row link target stays `/borrower/<id>` (intent: "who returned it, was it back when expected"; matches the loans page convention). Empty-state copy: "No recent additions in the last 7 days — nothing new to catalog yet" / "Aucun ajout récent ces 7 derniers jours — rien de nouveau à cataloguer encore" (recent-cataloged); "No recent returns in the last 7 days — quiet week!" / "Aucun retour récent ces 7 derniers jours — semaine calme !" (recent-returns).
   
   **NOTE on TitleCard duplication:** the spec's `home.html:186-189` comment already flags that the `<article>` block is "duplicated VERBATIM from the browse-results loop … known follow-up to extract into a partial". Story 9-7 inherits this debt by adding ONE MORE duplication (now 3 copies: browse-results loop, #recent-additions, #recent-cataloged-list). Extracting to a partial is OUT OF SCOPE for 9-7 (refactor-during-feature is anti-pattern); file as `type:code-review-finding` GH Issue at story close to lock the cleanup as its own focused PR.

7. **AC7 — Hardcoded 7-day window (NOT admin-configurable in v1).** The cutoff is the constant `RECENT_ACTIVITY_DAYS: i32 = 7`, declared in `src/routes/home_indicators.rs` (alongside `IndicatorFilter`). Spec text: "the cutoff is hardcoded in v1 (not admin-configurable per scope freeze) — documented in CLAUDE.md as a known constant; if the user later requests configurability, it becomes a settings story". The constant is referenced by all 4 new model methods (`count_recent_cataloged`, `list_recent_cataloged`, `count_recent_returns`, `list_recent_returns`) so a future change to e.g. 14 days lives in ONE place. **DOCUMENTATION REQUIREMENT:** add a one-line entry under "Architecture / Key Patterns" in `CLAUDE.md` mentioning the constant + its rationale ("hardcoded; configurable iff a future story extracts it to AppSettings"). Asserted by the unit test `recent_activity_window_constant_is_seven_days` in `home_indicators.rs::tests`.

8. **AC8 — Four NEW model methods (no existing reuse).** Add to `TitleModel` (`src/models/title.rs`) and `LoanModel` (`src/models/loan.rs`), patterned after 9-2 (`title::list_recent_active`) + 9-5 (`loan::count_overdue`/`loan::list_overdue`):
   - **`TitleModel::count_recent_cataloged(pool: &DbPool, days: i32) -> Result<i64, AppError>`** — SQL: `SELECT COUNT(*) FROM titles WHERE deleted_at IS NULL AND created_at >= NOW() - INTERVAL ? DAY`. Use `sqlx::query_as::<_, (i64,)>` (mirror `title::count_active`).
   - **`TitleModel::list_recent_cataloged(pool: &DbPool, days: i32, limit: u32) -> Result<Vec<SearchResult>, AppError>`** — same SQL projection as `list_recent_active` (lines 837-877) but with the additional WHERE clause `AND t.created_at >= NOW() - INTERVAL ? DAY`. Bind `(days, limit)`. ORDER BY `t.created_at DESC, t.id DESC` (same tiebreak as `list_recent_active`).
   - **`LoanModel::count_recent_returns(pool: &DbPool, days: i32) -> Result<i64, AppError>`** — SQL: `SELECT COUNT(*) FROM loans WHERE deleted_at IS NULL AND returned_at IS NOT NULL AND returned_at >= NOW() - INTERVAL ? DAY`. The `returned_at IS NOT NULL` guard is essential — without it, `returned_at >= NOW() - INTERVAL 7 DAY` would unexpectedly match NULL `returned_at` values per MariaDB's three-valued logic (`NULL >= X` evaluates to NULL, which is falsy, so it's actually safe — but the explicit guard makes intent crystal clear and locks the test contract).
   - **`LoanModel::list_recent_returns(pool: &DbPool, days: i32, limit: u32) -> Result<Vec<LoanWithDetails>, AppError>`** — same JOINs as `list_overdue` (loans → borrowers → volumes → titles, all with `deleted_at IS NULL`), WHERE clause `WHERE l.returned_at IS NOT NULL AND l.deleted_at IS NULL AND l.returned_at >= NOW() - INTERVAL ? DAY`, ORDER BY `l.returned_at DESC` (most-recently-returned first), LIMIT bound. Return `Vec<LoanWithDetails>` (existing struct — has all the fields the row template needs).
   - **Reuse pattern:** `SearchResult` (story 9-2) for recent_cataloged rows; `LoanWithDetails` (story 9-5) for recent_returns rows. NO new projection structs. Mirror of 9-5's `LoanWithDetails` REUSE decision (vs 9-4's `UnshelvedVolumeRow` NEW struct decision — 9-7 follows 9-5/9-6 since the existing structs already have every needed field).
   - Both pairs use **dynamic `query` / `query_as`** (NOT the macro `sqlx::query!`) — keeps `.sqlx/` cache untouched (project convention, mirrors 9-2/9-3/9-4/9-5/9-6).
   - **No composite index on `(created_at)` for titles or `(returned_at)` for loans.** For personal-library scale (< 10k titles, < 5k loans), the full-table scan with WHERE filter is acceptable v1. If a real deployment ever shows the count query taking > 50ms, file a `type:change-request` GH Issue. Same deferral rationale as 9-5's `idx_loans_overdue` and 9-6's `idx_series_gappy`.

9. **AC9 — `build_indicator_tags` extended to take 2 new count params + emit 2 new tag blocks.** Update `build_indicator_tags` (`src/routes/home_indicators.rs:71-122`) to grow from 5 args to 7 args:
    ```rust
    pub(crate) fn build_indicator_tags(
        unshelved_count: i64,
        overdue_count: i64,
        gaps_count: i64,
        recent_cataloged_count: i64,
        recent_returns_count: i64,
        active: Option<IndicatorFilter>,
        loc: &str,
    ) -> Vec<IndicatorTag>
    ```
    Behavior, in order (AC1 priority): unshelved → overdue → gaps → recent_cataloged → recent_returns. Each push block follows the existing pattern (count > 0 || is_active → push). Asserted by the new test `build_indicator_tags_emits_all_five_tags_in_priority_order_when_all_present`. **All existing call sites + tests need updating:** the handler call site at `home.rs:354-360` grows 2 new args; ALL existing helper-unit tests in `home_indicators.rs::tests` (currently ~15 cases after the 9-6 batch) need `0, 0` inserted as the 4th + 5th args. Update them by adding the args, NOT by deleting them — the existing contracts must stay locked.

10. **AC10 — i18n EN + FR (8 new keys).** Append to the existing `dashboard.attention:` block (currently at `locales/{en,fr}.yml:360-372`):
    - `recent_cataloged_label` — EN: `"Recent cataloged"`, FR: `"Catalogués récemment"`
    - `recent_cataloged_clear_aria` — EN: `"Clear filter: Recent cataloged"`, FR: `"Retirer le filtre : Catalogués récemment"`
    - `recent_cataloged_heading` — EN: `"Recently cataloged (last 7 days)"`, FR: `"Catalogués récemment (7 derniers jours)"`
    - `recent_cataloged_empty` — EN: `"No recent additions in the last 7 days — nothing new to catalog yet"`, FR: `"Aucun ajout récent ces 7 derniers jours — rien de nouveau à cataloguer encore"`
    - `recent_returns_label` — EN: `"Recent returns"`, FR: `"Retours récents"`
    - `recent_returns_clear_aria` — EN: `"Clear filter: Recent returns"`, FR: `"Retirer le filtre : Retours récents"`
    - `recent_returns_heading` — EN: `"Recently returned (last 7 days)"`, FR: `"Retours récents (7 derniers jours)"`
    - `recent_returns_empty` — EN: `"No recent returns in the last 7 days — quiet week!"`, FR: `"Aucun retour récent ces 7 derniers jours — semaine calme !"`
    - **REUSED keys** (no new add): `loan.days` (the days-since-return label inside row text); `loan.overdue` is NOT reused (recent returns are by definition NOT overdue — the row template skips the overdue badge for #recent-returns-list).
    - **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper — keys start at root. After editing, run `touch src/lib.rs && cargo build`. The `i18n::audit::tests::all_t_keys_have_both_locales` test enforces EN/FR mirror.

11. **AC11 — Unit tests.**
    - **(a) `title::count_recent_cataloged` + `title::list_recent_cataloged` (DB-backed `#[sqlx::test]` in NEW `tests/dashboard_recent_activity.rs`):**
        - `count_recent_cataloged_on_empty_db_returns_zero` — fresh schema, no titles, expect `0`.
        - `count_recent_cataloged_excludes_soft_deleted` — seed 3 titles with `created_at = NOW() - INTERVAL 1 DAY`, soft-delete one; expect `2`.
        - `count_recent_cataloged_window_boundary` — seed: 1 title `created_at = NOW() - INTERVAL 6 DAY`, 1 at 7 DAY, 1 at 8 DAY. Call with `days = 7`; expect `2` (the 6-day and 7-day titles match `>= NOW() - INTERVAL 7 DAY`; the 8-day title falls outside). The `>=` boundary is locked here — symmetric with the loans returned_at boundary.
        - `count_recent_cataloged_zero_days_returns_zero_or_today_only` — call with `days = 0`; expect titles with `created_at >= NOW()` only (essentially "today's titles with sub-second freshness"). Edge case verifies the parameter actually drives the SQL (and locks the AC8 worked example for `INTERVAL 0 DAY`).
        - `list_recent_cataloged_returns_in_created_at_desc_order_with_limit` — seed 5 titles with `created_at` at 1/2/3/4/5 days ago. Call `list_recent_cataloged(pool, 7, 3)`; assert exactly 3 rows in `created_at DESC` order (newest first = 1 → 2 → 3 days ago). Verify the joined fields are populated (`primary_contributor`, `genre_name`, `volume_count`).
    - **(b) `loan::count_recent_returns` + `loan::list_recent_returns` (DB-backed, same file):**
        - `count_recent_returns_on_empty_db_returns_zero`.
        - `count_recent_returns_excludes_active_loans` — seed: 2 active loans (`returned_at IS NULL`) created today + 2 returned loans (`returned_at = NOW()`). Expect `2` (only returned loans count).
        - `count_recent_returns_excludes_soft_deleted_loans`.
        - `count_recent_returns_window_boundary` — symmetric with `count_recent_cataloged_window_boundary` but on `returned_at`. Seed loans with `returned_at` at 6/7/8 days ago; call with `days = 7`; expect `2`.
        - `list_recent_returns_returns_in_returned_at_desc_order_with_limit` — symmetric with `list_recent_cataloged_…`. Newest-returned first.
    - **(c) `parse_indicator_filter` (extends `home_indicators.rs::tests`):**
        - **NEW** `parse_indicator_filter_recent_cataloged_recognized` — assert `Some("recent-cataloged")` → `Some(IndicatorFilter::RecentCataloged)`.
        - **NEW** `parse_indicator_filter_recent_returns_recognized` — symmetric.
        - **EDIT** `parse_indicator_filter_unknown_bare_name_returns_none` (`home_indicators.rs:240-250`): DELETE the `"recent-cataloged"` reservation + comment (now recognized). Keep `"nonsense"` only; no new reservation needed (9-7 is the LAST indicator story in Epic 9 per the spec text at `epics.md:1206`).
        - **EXTEND** `parse_indicator_filter_case_sensitive` (currently at `home_indicators.rs:160-180`): add assertions for `"RECENT-CATALOGED"`, `"Recent-Cataloged"`, `"RECENT-RETURNS"`, `"Recent-Returns"` — all → `None`.
    - **(d) `build_indicator_tags` (extends `home_indicators.rs::tests`):**
        - **UPDATE** ALL existing `build_indicator_tags_*` tests (currently ~12 cases after 9-6 batch) to pass `0, 0` as the new 4th + 5th args. They keep the same assertions (only the unshelved/overdue/gaps tags are exercised).
        - **NEW** `build_indicator_tags_recent_cataloged_only_returns_recent_cataloged_tag_in_default_state` — `(0, 0, 0, 5, 0, None, "en")` → 1 tag, `filter_name = "recent-cataloged"`, `count = 5`, EN label `"Recent cataloged"`.
        - **NEW** `build_indicator_tags_recent_returns_only_returns_recent_returns_tag_in_default_state` — symmetric.
        - **NEW** `build_indicator_tags_emits_all_five_tags_in_priority_order_when_all_present` — `(3, 5, 7, 9, 11, None, "en")` → 5 tags in order `[unshelved, overdue, gaps, recent-cataloged, recent-returns]`. Locks the AC1 visual order at the helper level.
        - **NEW** `build_indicator_tags_recent_cataloged_zero_count_with_active_filter_still_emits_active_tag` — `(0, 0, 0, 0, 0, Some(IndicatorFilter::RecentCataloged), "en")` → 1 tag, `is_active = true`, `count = 0` (escape hatch — same contract as 9-4/9-5/9-6).
        - **NEW** `build_indicator_tags_recent_returns_zero_count_with_active_filter_still_emits_active_tag` — symmetric.
    - **(e) Handler render tests (extends `src/routes/home_indicator_tests.rs::tests`):**
        - `home_anonymous_does_not_render_recent_cataloged_tag` — Anonymous role; assert `id="filter-tag-recent-cataloged"` is NOT present in HTML AND `id="recent-cataloged-list"` is NOT present (mirrors the 9-5 anonymous-no-leak overdue test). Repeat with `?filter=recent-cataloged` URL → still no tag, no list (Librarian-gated; `role_gated_indicator_filter` strips the variant).
        - `home_anonymous_does_not_render_recent_returns_tag` — symmetric.
        - `home_librarian_renders_recent_cataloged_tag_in_default_state_when_count_positive` — populated case via post-construction field assignment (NO new factory variant per AC15 LOC budget). Build a template with `indicator_tags = [recent-cataloged=5]` and `recent_cataloged_filter_active = false`. Assert `id="filter-tag-recent-cataloged"` is present, `aria-label="Recent cataloged: 5"`, `href="/?filter=recent-cataloged"`. Negative assertions for active-state markers (`!contains("&times;")`, `!contains("Clear filter: Recent cataloged")`) per the 9-5 code-review patch contract.
        - `home_librarian_renders_recent_returns_tag_in_default_state_when_count_positive` — symmetric.
        - `home_librarian_recent_cataloged_tag_active_state_when_filter_applied` — populated `recent_cataloged_titles`, `recent_cataloged_filter_active = true`. Assert `href="/"`, the `&times;` marker is present, active-state aria-label uses the clear copy. Negative assertions for default-state aria-label.
        - `home_librarian_recent_returns_tag_active_state_when_filter_applied` — symmetric.
        - `home_librarian_recent_cataloged_filter_active_renders_only_recent_cataloged_list_section` — populated case (`recent_cataloged_filter_active = true`, populated `recent_cataloged_titles`). Assert `id="recent-cataloged-list"` IS present AND `id="recent-additions"` is NOT AND `id="unshelved-list"` is NOT AND `id="overdue-list"` is NOT AND `id="gaps-list"` is NOT AND `id="recent-returns-list"` is NOT (6-way mutual exclusion across all six slots in the same DOM position).
        - `home_librarian_recent_returns_filter_active_renders_only_recent_returns_list_section` — symmetric.
        - `home_librarian_recent_cataloged_filter_empty_renders_empty_label` — `recent_cataloged_filter_active = true`, EMPTY `recent_cataloged_titles` Vec. Assert `id="recent-cataloged-list"` IS present AND the empty-label copy ("No recent additions in the last 7 days …") appears inside.
        - `home_librarian_recent_returns_filter_empty_renders_empty_label` — symmetric.
        - `home_renders_all_five_indicator_tags_in_priority_order` — populated case with all 5 indicators non-zero. Use `slice_section` + `attention_section_slice` + DOM order assertion to verify document-byte-stream order: unshelved < overdue < gaps < recent-cataloged < recent-returns. Locks the AC1 priority ordering at the rendered-HTML level.
        - `home_librarian_recent_cataloged_row_links_to_title_detail` — populated `recent_cataloged_titles` with 1 item (id=42, title="Tintin"). Assert the rendered HTML contains `href="/title/42"` (TitleCard convention, matching #recent-additions row-link target).
        - `home_librarian_recent_returns_row_links_to_borrower` — populated `recent_returns` with 1 LoanWithDetails (`borrower_id=10`). Assert `href="/borrower/10"` (mirrors #overdue-list row-link target — same intent).
    - **(f) `RECENT_ACTIVITY_DAYS` constant (extends `home_indicators.rs::tests`):**
        - **NEW** `recent_activity_window_constant_is_seven_days` — assert `RECENT_ACTIVITY_DAYS == 7`. One-line test that locks the v1 spec freeze; if a future story makes the window admin-configurable, this test fails loudly and the migration path is obvious (delete the constant, replace with `state.recent_activity_days()`).
    - **(g) FilterTag macro reuse — no NEW macro tests.** The 9-4 macro at `templates/components/filter_tag.html` is unchanged. Story 9-7 reuses it as-is for both new tags.

12. **AC12 — E2E (Foundation Rule #7).** Append a NEW `test.describe("Home page — Recent activity indicators", ...)` block at the end of `tests/e2e/specs/journeys/home.spec.ts` (AFTER the 9-6 "Series with gaps indicator" describe at lines 277-340ish):
    - **Test 1 — anonymous-no-leak (recent-cataloged + recent-returns).** Load `/` as anonymous; assert `#filter-tag-recent-cataloged` count == 0 AND `#filter-tag-recent-returns` count == 0 AND `#recent-cataloged-list` count == 0 AND `#recent-returns-list` count == 0. Navigate to `/?filter=recent-cataloged`; same assertions; assert `#recent-additions` is still visible (filter ignored). Repeat with `/?filter=recent-returns`. Mirrors 9-5 Test 1 shape, extended to both URLs.
    - **Test 2 — librarian smoke (conditional empty-DB short-circuit, BOTH tags in one test).** `await loginAs(page, "librarian")`; load `/`. Read counts of both tags. Conditional short-circuit: if BOTH counts are 0, return green pass (no recent activity in the seed DB). Otherwise:
      - For each non-zero tag, click it → assert URL change → assert the matching list section renders AND the other 5 sections do NOT (6-way mutual exclusion) → click the active-state ✕ → assert URL returns to `/` AND `#recent-additions` is back.
      - If both tags are non-zero, exercise both clicks back-to-back (the second click clears the first filter via the URL change, exactly like 9-5 → 9-6 transitions).
    - Use i18n-aware selectors via stable IDs (`#filter-tag-recent-cataloged`, `#filter-tag-recent-returns`, `#recent-cataloged-list`, `#recent-returns-list`). NO `waitForTimeout` (CI grep gate, enforced by `.github/workflows/_gates.yml::e2e`).
    - **No deterministic-fixture E2E.** Same rationale as 9-4/9-5/9-6: the conditional empty-DB short-circuit is acceptable for v1; the deterministic seed migration for indicator E2Es is already deferred as a cross-story `type:code-review-finding` (story 9-6 review D2). Adding deterministic seed for 9-7 is OUT OF SCOPE.

13. **AC13 — CSP compliance.** No `style="..."`, no `<style>`, no `<script>`, no `onclick=`, no inline event handlers in any template touched by this story. The recent-cataloged-list reuses TitleCard markup verbatim (already CSP-clean from Epic 1). The recent-returns-list reuses LoanRow markup verbatim from `#overdue-list` (already CSP-clean from 9-5). The 9-4 FilterTag macro is reused for both new tags — no template-component edits. The `src/templates_audit.rs::no_inline_markup_in_templates` test (line 44) MUST stay green.

14. **AC14 — Foundation Rule #12 — keep `src/routes/home.rs` ≤ 2000 LOC.** `src/routes/home.rs` is at **1948 LOC** as of 9-6 close. Adding 9-7 substance (HomeTemplate +6-8 fields ≈ 8 LOC, handler 2 new fetch blocks + 2 new active booleans ≈ 50 LOC, test factory updates to populate the new fields ≈ 12 LOC) ≈ ~70 LOC growth in `home.rs`. Final estimated LOC: ~2018 — JUST over the ceiling.
    - **First priority** — extend `make_test_home_template_with_counts` factory to populate the new fields with sensible defaults (~12 LOC unavoidable); use post-construction field assignment for ALL new render tests (NO new sibling factory variant); tighten doc-comments on the handler block; reuse the 9-5/9-6 LOC-trim playbook.
    - **All NEW render tests** live in `src/routes/home_indicator_tests.rs` (the precedent set in 9-6 — all indicator render tests live there; NOT in `home.rs::tests`). This continues to pay dividends: the ~12 new render tests for 9-7 add ZERO LOC to home.rs.
    - **Fallback** — if `wc -l src/routes/home.rs` after Task 8 exceeds 2000, extract the handler's per-indicator data-fetching blocks (currently 4 blocks × ~25 LOC each = ~100 LOC after 9-7 lands) into a sibling `src/routes/home_data.rs` module exposing `pub(crate) async fn fetch_indicator_counts_and_lists(state, session, parsed_indicator) -> IndicatorData`. Net savings: ~80 LOC. Mark this as a Task 9 verification step; do NOT pre-extract speculatively (refactor-during-feature anti-pattern).
    - **Verification** — Task 9 includes a `wc -l src/routes/home.rs` step that fails the story if the file exceeds 2000 LOC. Foundation Rule #12 is non-negotiable.

15. **AC15 — Closes the indicator-subsystem chapter (5/5 indicators delivered).** With this story, the indicator subsystem is FEATURE-COMPLETE per the Epic 9 scope freeze: 5 indicators (Unshelved, Overdue, Gaps, Recent cataloged, Recent returns) are delivered, the visual ordering is locked (priority by actionability), and the `IndicatorFilter` enum is closed (no more reservations in `parse_indicator_filter_unknown_bare_name_returns_none`). FR58 is fully satisfied. Subsequent Epic 9 stories (9-8 onward) move to different surfaces (loan status role-aware, scanner state machine, modal migrations, etc.). Task 10 documentation must update the Dev Agent Record's "Architecture compliance" / "Project structure notes" section to mark the indicator chapter closed and call out the test counts for the whole chapter (per-story totals + cumulative).

## Tasks / Subtasks

- [ ] **Task 1 — Four NEW model methods + `tests/dashboard_recent_activity.rs` (AC: 7, 8, 11a, 11b)**
  - [ ] In `src/models/title.rs`, add `pub async fn count_recent_cataloged(pool: &DbPool, days: i32) -> Result<i64, AppError>` directly after `count_active` (line 186-189). Pattern: `sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM titles WHERE deleted_at IS NULL AND created_at >= NOW() - INTERVAL ? DAY").bind(days).fetch_one(pool).await?` — return `row.0`.
  - [ ] In `src/models/title.rs`, add `pub async fn list_recent_cataloged(pool: &DbPool, days: i32, limit: u32) -> Result<Vec<SearchResult>, AppError>` mirroring `list_recent_active` (lines 833-879). SQL projection identical (genres JOIN + primary_contributor subquery + volume_count subquery). WHERE clause: `WHERE t.deleted_at IS NULL AND t.created_at >= NOW() - INTERVAL ? DAY`. ORDER BY `t.created_at DESC, t.id DESC`. LIMIT bound. Bind `(days, limit)`.
  - [ ] In `src/models/loan.rs`, add `pub async fn count_recent_returns(pool: &DbPool, days: i32) -> Result<i64, AppError>` directly after `count_overdue` (lines 297-318 post-9-5). SQL: `SELECT COUNT(*) FROM loans WHERE deleted_at IS NULL AND returned_at IS NOT NULL AND returned_at >= NOW() - INTERVAL ? DAY`. Bind `days`.
  - [ ] In `src/models/loan.rs`, add `pub async fn list_recent_returns(pool: &DbPool, days: i32, limit: u32) -> Result<Vec<LoanWithDetails>, AppError>` mirroring `list_overdue`. SQL JOINs identical (loans → borrowers → volumes → titles, all with `deleted_at IS NULL`). WHERE: `WHERE l.returned_at IS NOT NULL AND l.deleted_at IS NULL AND l.returned_at >= NOW() - INTERVAL ? DAY`. ORDER BY `l.returned_at DESC`. LIMIT bound. Bind `(days, limit)`.
  - [ ] All four functions use **dynamic `query` / `query_as`** (NOT the macro `sqlx::query!`) — keeps `.sqlx/` cache untouched.
  - [ ] Build the integration test file `tests/dashboard_recent_activity.rs` (NEW, sibling of `dashboard_overdue.rs` + `dashboard_gaps.rs`):
    - Helpers: copy `first_genre_id`, `first_volume_state_id`, `insert_title`, `insert_volume`, `insert_borrower`, `insert_loan`, `mark_loan_returned`, `soft_delete_loan` from `tests/dashboard_overdue.rs` (cross-file copy is acceptable — same project precedent as 9-4/9-5/9-6).
    - NEW helper: `insert_title_with_created_at(pool, name, days_ago)` → `INSERT INTO titles (...) VALUES (...) ; UPDATE titles SET created_at = NOW() - INTERVAL ? DAY WHERE id = LAST_INSERT_ID()` — same backdating pattern as `dashboard_overdue.rs::insert_loan` for `loaned_at`. (Two-statement: insert with default `created_at = NOW()`, then UPDATE to backdate. The UPDATE is necessary because `created_at` has `DEFAULT CURRENT_TIMESTAMP` in the schema, and direct INSERT with explicit `created_at` works but is more verbose.)
    - NEW helper: `mark_loan_returned_at(pool, loan_id, days_ago)` → `UPDATE loans SET returned_at = NOW() - INTERVAL ? DAY WHERE id = ?`. Used to deterministically set the `returned_at` window.
    - 5 `#[sqlx::test(migrations = "./migrations")]` cases per AC11a (5).
    - 5 `#[sqlx::test]` cases per AC11b (5).
  - [ ] Verify: `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test --test dashboard_recent_activity` — all 10 green; lock as Commit 1 of the story branch.

- [ ] **Task 2 — `RECENT_ACTIVITY_DAYS` constant + 2 new `IndicatorFilter` variants + parser update (AC: 4, 7, 11c, 11f)**
  - [ ] In `src/routes/home_indicators.rs`, add a `pub(crate) const RECENT_ACTIVITY_DAYS: i32 = 7;` near the top of the file (above the `IndicatorFilter` enum). Add a doc comment per AC7: hardcoded v1 cutoff; configurable iff a future story extracts it to AppSettings.
  - [ ] Add `RecentCataloged` and `RecentReturns` variants to the `IndicatorFilter` enum (currently 3 variants post-9-6). Remove the `// Reserved for follow-up Epic 9 story: RecentCataloged (9-7), RecentReturns (9-7)` comment — they're now landed.
  - [ ] Update `parse_indicator_filter`:
    ```rust
    pub(crate) fn parse_indicator_filter(filter: &Option<String>) -> Option<IndicatorFilter> {
        match filter.as_deref() {
            Some("unshelved") => Some(IndicatorFilter::Unshelved),
            Some("overdue") => Some(IndicatorFilter::Overdue),
            Some("gaps") => Some(IndicatorFilter::Gaps),
            Some("recent-cataloged") => Some(IndicatorFilter::RecentCataloged),
            Some("recent-returns") => Some(IndicatorFilter::RecentReturns),
            Some(v) if !v.contains(':') && !v.is_empty() => {
                tracing::warn!(filter = %v, "Unknown indicator filter, ignoring");
                None
            }
            _ => None,
        }
    }
    ```
  - [ ] **EDIT** `parse_indicator_filter_unknown_bare_name_returns_none`: DELETE the `"recent-cataloged"` reservation + comment (now recognized). Keep only `"nonsense"` — no new reservation needed (9-7 is the LAST indicator story).
  - [ ] **NEW** `parse_indicator_filter_recent_cataloged_recognized` and `parse_indicator_filter_recent_returns_recognized` tests (mirror the existing positive tests for unshelved/overdue/gaps).
  - [ ] **EXTEND** `parse_indicator_filter_case_sensitive` test: add `"RECENT-CATALOGED"`, `"Recent-Cataloged"`, `"RECENT-RETURNS"`, `"Recent-Returns"` assertions, all → `None`.
  - [ ] **NEW** `recent_activity_window_constant_is_seven_days` test in `home_indicators.rs::tests`: `assert_eq!(RECENT_ACTIVITY_DAYS, 7);`.
  - [ ] `cargo test routes::home_indicators` — all parser + constant tests green; lock as Commit 2.

- [ ] **Task 3 — `build_indicator_tags` extension + 5 new helper unit tests (AC: 9, 11d)**
  - [ ] In `src/routes/home_indicators.rs`, extend `build_indicator_tags` to take `recent_cataloged_count: i64` as the 4th parameter and `recent_returns_count: i64` as the 5th parameter (between `gaps_count` and `active`). Add 2 new push blocks at the END of the function, after the existing gaps push block, in the order recent-cataloged → recent-returns (per AC1 priority ordering).
  - [ ] **UPDATE** ALL existing `build_indicator_tags_*` tests in `home_indicators.rs::tests` (currently ~12 cases after 9-6 batch) to pass `0, 0` as the new 4th + 5th args. They keep the same assertions.
  - [ ] **NEW** unit tests per AC11d (5 cases): `build_indicator_tags_recent_cataloged_only_…`, `build_indicator_tags_recent_returns_only_…`, `build_indicator_tags_emits_all_five_tags_in_priority_order_when_all_present`, `build_indicator_tags_recent_cataloged_zero_count_with_active_filter_still_emits_active_tag`, `build_indicator_tags_recent_returns_zero_count_with_active_filter_still_emits_active_tag`.
  - [ ] Update the `home::home` handler's `build_indicator_tags` call site (post-9-6 location, ~`home.rs:354-360`) to pass the new args. Use placeholders `0, 0` initially; Task 5 wires the real counts.
  - [ ] `cargo test routes::home_indicators` — all green; lock as Commit 3.

- [ ] **Task 4 — i18n keys (AC: 10)**
  - [ ] In `locales/en.yml`, append 8 new keys to the existing `dashboard.attention:` block (after `gaps_empty:` at line 372 post-9-6). See AC10 for exact text.
  - [ ] In `locales/fr.yml`, mirror the 8 keys at the same path.
  - [ ] **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper. After editing, run `touch src/lib.rs && cargo build`. The `i18n::audit::tests::all_t_keys_have_both_locales` test enforces EN/FR mirror.

- [ ] **Task 5 — Wire the home handler (AC: 1, 2, 5, 6)**
  - [ ] In `src/routes/home.rs::home`, immediately after the existing `gaps_series` block (post-9-6, ~line 351), add the recent-cataloged data fetching:
    ```rust
    // Story 9-7 — Recent cataloged indicator. Same anonymous-skip + soft-degrade
    // pattern as unshelved/overdue (AC2 — back to symmetric role gating; recent
    // activity is Librarian-gated). Window cutoff is the hardcoded
    // `RECENT_ACTIVITY_DAYS` constant per AC7 spec freeze.
    let recent_cataloged_count: i64 = if session.role >= Role::Librarian {
        match crate::models::title::TitleModel::count_recent_cataloged(
            pool,
            crate::routes::home_indicators::RECENT_ACTIVITY_DAYS,
        )
        .await
        {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %e, "count_recent_cataloged failed; rendering 0 (tag hidden)");
                0
            }
        }
    } else {
        0
    };
    let recent_cataloged_filter_active = session.role >= Role::Librarian
        && active_indicator_filter == Some(IndicatorFilter::RecentCataloged);
    let recent_cataloged_titles: Vec<crate::models::title::SearchResult> =
        if recent_cataloged_filter_active {
            match crate::models::title::TitleModel::list_recent_cataloged(
                pool,
                crate::routes::home_indicators::RECENT_ACTIVITY_DAYS,
                100,
            )
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "list_recent_cataloged failed; rendering empty list");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
    ```
  - [ ] Add the analogous block for recent-returns immediately after, pattern-matching the recent-cataloged block. Use `LoanModel::count_recent_returns` and `LoanModel::list_recent_returns`. The `recent_returns_filter_active` boolean uses `IndicatorFilter::RecentReturns`. The list type is `Vec<crate::models::loan::LoanWithDetails>`.
  - [ ] Update the `build_indicator_tags` call (line ~354) to pass `recent_cataloged_count` as 4th arg and `recent_returns_count` as 5th arg.
  - [ ] Extend `HomeTemplate` (struct at `home.rs:32-122` post-9-6) with the new fields:
    - `pub recent_cataloged_filter_active: bool`
    - `pub recent_cataloged_titles: Vec<crate::models::title::SearchResult>`
    - `pub recent_cataloged_heading: String`
    - `pub recent_cataloged_empty_label: String`
    - `pub recent_returns_filter_active: bool`
    - `pub recent_returns: Vec<crate::models::loan::LoanWithDetails>`
    - `pub recent_returns_heading: String`
    - `pub recent_returns_empty_label: String`
  - [ ] Pre-translate the 4 new heading + empty labels in the handler (mirror the existing pattern for `attention_heading`, `unshelved_heading`, etc.).
  - [ ] Update the `make_test_home_template_with_counts` factory (`home.rs::tests` post-9-6) to populate the 8 new fields with sensible defaults so existing tests keep passing.
  - [ ] **AC5 6-way mutual exclusion** falls out for free: `unshelved_filter_active`, `overdue_filter_active`, `gaps_filter_active`, `recent_cataloged_filter_active`, `recent_returns_filter_active` cannot ALL be true at once (the URL has one `?filter=` value; the parser returns one variant). Template's `{% if %}{% elif %}{% elif %}{% elif %}{% elif %}{% else %}` chain enforces it visually.

- [ ] **Task 6 — Render `#recent-cataloged-list` + `#recent-returns-list` sections in `home.html` (AC: 6, 13)**
  - [ ] Edit `templates/pages/home.html` lines 124-220ish (the existing 4-branch `{% if %}{% else if %}{% else if %}{% else %}` chain post-9-6). Convert to a 6-branch chain by inserting two new `{% else if %}` branches BETWEEN the gaps branch (currently ends ~line 219) and the recent-additions `{% else %}` branch:
    - `{% else if recent_cataloged_filter_active %}` → `<section id="recent-cataloged-list">` with TitleCard markup verbatim from `#recent-additions` (lines 200-218 post-9-6) bound to `recent_cataloged_titles`. Heading: `{{ recent_cataloged_heading }}`. Empty-state: `{{ recent_cataloged_empty_label }}`.
    - `{% else if recent_returns_filter_active %}` → `<section id="recent-returns-list">` with LoanRow markup verbatim from `#overdue-list` (lines 165-181 post-9-6) bound to `recent_returns`. Row link target: `/borrower/{{ loan.borrower_id }}` (matches #overdue-list intent — "who returned it"). NO red overdue badge in the row template (recent returns are by definition NOT overdue). Heading: `{{ recent_returns_heading }}`. Empty-state: `{{ recent_returns_empty_label }}`.
  - [ ] Update the trailing comment at the end of the chain to list all 6 branches: `{# /unshelved_filter_active or overdue_filter_active or gaps_filter_active or recent_cataloged_filter_active or recent_returns_filter_active #}`.
  - [ ] CSP: zero `style="..."`, zero `<script>`, zero `onclick=`. Both new sections reuse the established Tailwind class palettes from #recent-additions (TitleCard) and #overdue-list (LoanRow). The `templates_audit::no_inline_markup_in_templates` test (line 44) MUST stay green.

- [ ] **Task 7 — Render tests in `src/routes/home_indicator_tests.rs::tests` (AC: 11e)**
  - [ ] Add the 12 new render tests per AC11e + the 2 row-link target tests (14 total). Use **post-construction field assignment** on the existing `make_test_home_template_with_indicators` factory — DO NOT add a new sibling factory variant. This keeps `home.rs` LOC budget headroom per AC14 and matches the 9-5/9-6 LOC-trim playbook.
  - [ ] Add small helper functions: `fake_search_result_for_recent_cataloged(id, title)` (returns a minimal `SearchResult` populated with the 9 required fields) — OR reuse the existing `fake_search_result` from `home.rs::tests` (currently `pub(crate)` since 9-6 — verify it's accessible). The 14 new render tests ALL live in `home_indicator_tests.rs` — ZERO LOC added to `home.rs` for this batch.

- [ ] **Task 8 — E2E spec block (AC: 12)**
  - [ ] In `tests/e2e/specs/journeys/home.spec.ts`, append a new `test.describe("Home page — Recent activity indicators", ...)` block AFTER the 9-6 "Series with gaps indicator" describe (~line 277 post-9-6 — verify against the live file at task start). 2 tests per AC12.
  - [ ] **Test 1** (anonymous) covers BOTH `?filter=recent-cataloged` and `?filter=recent-returns` URLs in one `test()` — single anonymous session, navigate to both URLs in sequence, assert no leak on either.
  - [ ] **Test 2** (librarian smoke) follows the 9-4/9-5/9-6 conditional empty-DB short-circuit pattern. If BOTH tags are zero, return green pass. Otherwise click each non-zero tag in turn and verify the corresponding list section + 6-way mutual exclusion.
  - [ ] Use scoped selectors: `page.locator('#what-needs-attention #filter-tag-recent-cataloged')` not `page.locator('#filter-tag-recent-cataloged')`. Mirrors the 9-2/9-3 unscoped-selector flake fix.
  - [ ] No `waitForTimeout`. CI grep gate enforces this.

- [ ] **Task 9 — LOC budget enforcement (AC: 14)**
  - [ ] After Tasks 5-7 land, run `wc -l src/routes/home.rs`. Target: ≤ 2000 LOC (Foundation Rule #12).
  - [ ] If 2000 < x ≤ 2050: trim doc-comments on the new render tests + handler block; reuse the 9-5/9-6 "trim then re-check" iteration pattern.
  - [ ] If x > 2050 OR trimming alone doesn't get under 2000: extract the handler's per-indicator data-fetching blocks into a new sibling `src/routes/home_data.rs` module. The function signature should be `pub(crate) async fn fetch_indicator_data(state: &AppState, session: &Session, parsed_indicator: Option<IndicatorFilter>, role_gated: Option<IndicatorFilter>) -> IndicatorData` returning a struct with all 5 (count, list, filter_active) triples. Net savings: ~80 LOC. Mark this as the AC14 fallback extraction; document the decision in the Dev Agent Record.

- [ ] **Task 10 — Verify and document (AC: 1–15)**
  - [ ] `wc -l src/routes/home.rs` — verify ≤ 2000 LOC. Foundation Rule #12 must hold.
  - [ ] `SQLX_OFFLINE=true cargo check && cargo clippy --all-targets -- -D warnings` — clean (zero warnings).
  - [ ] `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test` — full suite green. Expected: ~698 lib tests baseline + ~20 new (1 constant + 5 helper + 14 render) ≈ ~718 lib; +10 new integration tests in `tests/dashboard_recent_activity.rs` (going from 16 to 26 in the dashboard_* family).
  - [ ] `cargo sqlx prepare --check --workspace` — expected no diff (Tasks 1-3 use dynamic `query` / `query_as`).
  - [ ] Tailwind rebuild not required — every utility class used in the new sections (TitleCard + LoanRow palettes) already present.
  - [ ] Manual smoke from a running dev instance (`MYBIBLI_SKIP_SETUP=1 cargo run`):
    - As anonymous: `curl http://localhost:8080/?filter=recent-cataloged` and grep — `id="filter-tag-recent-cataloged"` MUST NOT appear; `id="recent-cataloged-list"` MUST NOT appear; `#recent-additions` IS still rendered.
    - Repeat with `?filter=recent-returns`.
    - As librarian: `curl` with session cookie → grep — both tags appear iff their respective counts > 0.
    - Click each tag in a browser → URL changes → respective list replaces #recent-additions → click ✕ → URL returns → recent-additions back. Repeat with the other 3 indicators (unshelved, overdue, gaps) to confirm 6-way mutual exclusion.
  - [ ] **E2E** (Foundation Rule #13) — `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/home.spec.ts`. Same `tests/e2e/test-results/` ownership-blocker caveat as 9-1/9-2/9-3/9-4/9-5/9-6 may apply locally; CI on the story branch is the source of truth.
  - [ ] Update Dev Agent Record at the bottom of this file: list of files touched, decisions on placement, anything surprising. **Special: mark the indicator subsystem chapter closed** — call out the cumulative test counts (per-story totals + grand total across 9-4/9-5/9-6/9-7).
  - [ ] Update `_bmad-output/implementation-artifacts/sprint-status.yaml`: `9-7-recent-activity-indicators: ready-for-dev → in-progress` at start, `→ review` at end (only this line + `last_updated`, per CLAUDE.md rule 16).
  - [ ] Open draft PR at first commit (Foundation Rule #15). Title: `Story 9-7: Recent activity indicators (#NN)`.

## Dev Notes

### Source tree references

| Concern | File / location | Notes |
|---|---|---|
| Home route registration | `src/routes/mod.rs:87` | `.route("/", axum::routing::get(home::home))` — no change |
| Home handler | `src/routes/home.rs:128-562` (post-9-6) | extend the existing handler; do NOT create a parallel route |
| Home template | `templates/pages/home.html` (~370 lines post-9-6) | extends `layouts/base.html`; sections: search, filter pills, metadata error, `#what-needs-attention` (lines 83-92), `#collection-glance` (96-122), the 4-branch slot at lines 124-220 (becomes 6-branch in this story), `#stats-by-genre` (~225-250), browse toggle, `#browse-results` |
| Insertion point for `#recent-cataloged-list` + `#recent-returns-list` | `templates/pages/home.html` lines 124-220 — convert the 4-branch `{% if %}{% else if %}{% else if %}{% else %}` to a 6-branch chain by inserting two new `{% else if %}` branches between gaps and recent-additions | AC6 6-way mutual exclusion across all six sections |
| Title model | `src/models/title.rs` | extend with `count_recent_cataloged` (mirror `count_active` lines 186-189) + `list_recent_cataloged` (mirror `list_recent_active` lines 833-879) |
| Title struct (return type) | `src/models/title.rs:617` (`pub struct SearchResult`) | REUSE — has all the fields TitleCard needs (`id`, `title`, `subtitle`, `media_type`, `genre_name`, `primary_contributor`, `volume_count`, `cover_image_url`, `publication_date`) |
| Loan model | `src/models/loan.rs` (post-9-5 with count_overdue + list_overdue) | extend with `count_recent_returns` + `list_recent_returns` (mirror count_overdue + list_overdue at lines 297-413) |
| Loan struct (return type) | `src/models/loan.rs:21-30` (`pub struct LoanWithDetails`) | REUSE — same fields the row template needs (`borrower_id`, `borrower_name`, `volume_label`, `title_name`, `loaned_at`, `returned_at`, `duration_days`) |
| Loan schema | `migrations/20260329000000_initial_schema.sql:158-175` | column is `loaned_at` + `returned_at`; no composite index on `(returned_at)` for the recent-returns scan — same deferral as 9-5's `idx_loans_overdue` |
| `IndicatorFilter` enum + parser | `src/routes/home_indicators.rs:22-45` post-9-6 (with `Unshelved`, `Overdue`, `Gaps` variants) | extended with `RecentCataloged` + `RecentReturns` variants + 2 new parser arms |
| `IndicatorTag` view-model | `src/routes/home_indicators.rs:53-67` | unchanged |
| `build_indicator_tags` helper | `src/routes/home_indicators.rs:71-122` | extended to accept 2 new count params (4th + 5th args) + 2 new push blocks |
| `role_gated_indicator_filter` helper | `src/routes/home_indicators.rs:142-150` (post-9-6 P1 fix) | unchanged — already role-gates ALL variants for Anonymous |
| `RECENT_ACTIVITY_DAYS` constant (NEW) | `src/routes/home_indicators.rs` (top-of-file, near `IndicatorFilter`) | the v1 hardcoded 7-day window cutoff; docstring per AC7 |
| Single-active-filter precedence | `src/routes/home.rs:175-211` (post-9-6 P1 fix — role-blind via `parsed_indicator.is_some()`) | already routes ANY `IndicatorFilter` variant through the same precedence path — no rewrite |
| Soft-degrade pattern | `src/routes/home.rs:237-351` (unshelved + overdue + gaps with `tracing::warn!` + 0 / Vec::new() on error) | replicate verbatim for recent_cataloged + recent_returns |
| FilterTag macro | `templates/components/filter_tag.html` (post-9-4) | unchanged — reuse via `{% call filter_tag::tag(...) %}{% endcall %}` for both new tags |
| HomeTemplate struct | `src/routes/home.rs:32-122` (post-9-6) | extend with 8 new fields (recent_cataloged_filter_active, recent_cataloged_titles, recent_cataloged_heading, recent_cataloged_empty_label, recent_returns_filter_active, recent_returns, recent_returns_heading, recent_returns_empty_label) |
| Test factory (default) | `src/routes/home.rs::tests::make_test_home_template_with_counts` (lines ~947-1029 post-9-6) | extend with the 8 new fields populated to sensible defaults |
| Test factory (indicator) | `src/routes/home.rs::tests::make_test_home_template_with_indicators` (lines ~1100-1112 post-9-6, `pub(crate)`) | unchanged — render tests use post-construction field assignment per AC14 |
| Slice helpers | `src/routes/home.rs::tests::slice_section`, `attention_section_slice` (post-9-6, `pub(crate)`) | reuse for the new render assertions |
| i18n locales | `locales/en.yml:360-372`, `locales/fr.yml:360-372` (`dashboard.attention:` block post-9-6) | append `recent_cataloged_*` (4 keys) + `recent_returns_*` (4 keys) |
| Existing reused i18n | `locales/en.yml:393` (`loan.days`) | reused via `days_label` HomeTemplate field for the recent-returns row text. `loan.overdue` is NOT reused (recent returns are NOT overdue by definition). |
| i18n audit | `src/i18n/audit.rs::all_t_keys_have_both_locales` | enforces EN/FR mirror |
| Templates audit | `src/templates_audit.rs::no_inline_markup_in_templates` (line 44) | must stay green |
| Test pattern (DB-backed integration) | `tests/dashboard_overdue.rs` + `tests/dashboard_gaps.rs` | sibling shape for `tests/dashboard_recent_activity.rs` — `#[sqlx::test(migrations = "./migrations")]`, file-local helpers |
| Test pattern (handler render, no DB) | `src/routes/home_indicator_tests.rs` (post-9-6) | ALL new render tests live here per AC14 — keeps home.rs LOC budget headroom |
| E2E spec for `/` | `tests/e2e/specs/journeys/home.spec.ts` (5 describes post-9-6) | extend with the new "Recent activity indicators" describe AFTER the 9-6 "Series with gaps indicator" block |
| E2E loginAs helper | `tests/e2e/helpers/auth.ts` | `loginAs(page, "librarian")` — typed union, do not pass other strings |

### Anti-patterns to avoid

- **Creating a new projection struct for recent_cataloged or recent_returns rows.** Both indicators reuse existing structs (`SearchResult` for titles, `LoanWithDetails` for loans). The TitleCard and LoanRow markups reuse the same field shapes verbatim. A new struct would be parallel-shape duplication — the kind that triggers a `simplify` skill review (mirrors 9-5's `LoanWithDetails` REUSE decision and 9-6's `SeriesWithGap` NEW-but-narrow decision; the right call here is REUSE because the existing structs already have every field needed).
- **Adding a per-variant role-gating bypass for recent_cataloged or recent_returns.** Story 9-6's Gaps was the EXCEPTION — series browsing is anonymous-permitted. Recent activity is Librarian-gated content (titles created today may be drafts, loans returned today expose borrower names). Use `role_gated_indicator_filter` unchanged for both new variants — they get stripped to None for Anonymous, exactly like Unshelved + Overdue.
- **Hardcoding `7` inline in the SQL or handler.** The `RECENT_ACTIVITY_DAYS` constant exists for a reason — a future change to e.g. 14 days lives in ONE place. Inline literals scatter the change.
- **`sqlx::query!` macro.** Forces `cargo sqlx prepare` regeneration in the PR. Use dynamic `query` / `query_as`.
- **Calling `t!()` from inside the Askama template.** Pre-translate in the handler, pass as `String` fields. Project convention.
- **Inline `style="..."` for the row coloring.** UX-DR24 mandates Tailwind utility classes. Reuse existing class palettes from #recent-additions (TitleCard) and #overdue-list (LoanRow).
- **Adding a red "Returned overdue" badge to the recent-returns rows.** This story does NOT distinguish "returned on time" vs "returned late". The recent-returns surface is purely chronological (most-recently-returned first); whether the return was overdue at the time of return is OUT OF SCOPE for v1 (a future "loan history with overdue/on-time stats" story could extend it). The row template uses ONLY the borrower name + V-code + title name + days-since-return — no badge logic.
- **Running `list_recent_cataloged` or `list_recent_returns` when their respective `*_filter_active = false`.** Handler must guard. Wasteful otherwise.
- **Pushing past 2000 LOC in `src/routes/home.rs`.** Foundation Rule #12. AC14 is non-negotiable. The fallback extraction (`home_data.rs`) is documented if the trim alone doesn't suffice.
- **Adding a sibling factory `make_test_home_template_with_recent_cataloged_indicator` in `home_indicator_tests.rs::tests`.** Use post-construction field assignment (the pattern locked by 9-5/9-6's LOC-budget mitigation).
- **Putting NEW render tests in `home.rs::tests`.** ALL indicator render tests live in `home_indicator_tests.rs` per the AC14 / 9-6 precedent. ZERO render tests should be added to `home.rs` for 9-7.
- **Reserving a new bare-name in `parse_indicator_filter_unknown_bare_name_returns_none` after deleting `"recent-cataloged"`.** Story 9-7 is the LAST indicator story in Epic 9 (per the Epic 9 scope freeze at `epics.md:1206`). The reservation chain ENDS here. Future indicator additions are out-of-scope and would be a new story.

### Symmetric vs asymmetric role gating recap (cross-story summary)

| Story | Indicator | Role gating | Reason |
|---|---|---|---|
| 9-4 | Unshelved | **Symmetric** (Librarian/Admin only) | Volume management is Librarian-gated. Anonymous never sees volume IDs / locations. |
| 9-5 | Overdue | **Symmetric** (Librarian/Admin only) | Loans expose borrower names — privacy. |
| 9-6 | Gaps | **Asymmetric** (anonymous-allowed via `gaps_filter_active`; tag still Librarian-only) | Series browsing is anonymous-permitted (FR65 + FR95). |
| 9-7 | Recent cataloged | **Symmetric** (Librarian/Admin only) | Titles created in the last 7 days may include drafts or per-Librarian work-in-progress. |
| 9-7 | Recent returns | **Symmetric** (Librarian/Admin only) | Loans expose borrower names — privacy. |

The 9-6 asymmetry is the SOLE exception in the indicator subsystem. Stories 9-7 onward revert to the symmetric pattern via `role_gated_indicator_filter`.

### Architecture compliance

- **Error handling:** Any DB failure in the 4 new model methods returns `AppError::Database` via `?`; the handler soft-degrades with `tracing::warn!` + `0` (count) or `Vec::new()` (list), per the established 9-1/9-2/9-3/9-4/9-5/9-6 pattern.
- **Logging:** `tracing::warn!` at handler level on the soft-degrade paths; `tracing::debug!` only inside model functions if needed. The single-active-filter `tracing::warn!` at `home.rs:179-185` (post-9-6) ALREADY covers the conflict case for the new variants — no new log statement needed.
- **DB query discipline:** Every SELECT/JOIN of entity tables (`titles`, `loans`, `borrowers`, `volumes`, `genres`, `contributors`, `contributor_roles`) MUST include `deleted_at IS NULL`. The 4 new queries inherit this pattern from their 9-2/9-5 templates.
- **HTMX coexistence:** the 6 list-section slots all sit OUTSIDE `#browse-results` (HTMX swap target) — same invariant as 9-1 through 9-6. Plain `<a href>` navigation does not interact with the HTMX search-fragment branch.
- **CSP middleware:** Already wraps the handler outermost. No work needed.
- **Pool access:** The handler already has `state.pool: DbPool`. No new connection.
- **One-branch-one-story (Foundation Rule #14):** Verify `git status` clean + on `main` + `git pull --ff-only` before starting; cut `story/9-7-recent-activity-indicators`. Open a draft PR (Rule #15) at the first commit (the Task 1 model methods).
- **Source-file-size limit (Foundation Rule #12):** `home.rs` is at **1948 LOC** post-9-6 close. AC14 mandates trimming + post-construction-field-assignment + ALL render tests in `home_indicator_tests.rs`. Estimated final LOC: ~2000-2020 — at the boundary; the AC14 fallback (`home_data.rs` extraction) is the documented escape hatch.

### Library / framework requirements

- **Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + Tailwind CSS v4 + HTMX 2.0** — no version changes, no new dependencies.
- **rust_i18n** — already wired. Pre-translate the 8 new keys in the handler. No `count =` interpolation needed.
- **MariaDB `INTERVAL` semantics:** `NOW() - INTERVAL ? DAY` accepts an integer parameter via SQLx bind. The `?` is replaced with the bound `i32` value. Both 9-5 (`count_overdue` with `DATEDIFF(NOW(), loaned_at) > ?`) and 9-7 use parameter binding for the day window — consistent semantics.
- **MariaDB `>=` boundary on TIMESTAMP comparison:** `created_at >= NOW() - INTERVAL 7 DAY` is INCLUSIVE — a title created exactly at the 7-day boundary instant matches. Locked by `count_recent_cataloged_window_boundary` test. This is the OPPOSITE of the 9-5 strict-`>` boundary on `DATEDIFF` — the difference is intentional and reflects the FR semantics ("last 7 days" includes the 7th day; "exceeds N days" is strict). Document this asymmetry in the model fn doc-comment to prevent a future drive-by "harmonize the boundary" refactor.
- **Askama macros** — the FilterTag component is unchanged from 9-4; reuse via `{% call filter_tag::tag(...) %}{% endcall %}` for both new tags.

### File structure requirements

| File | Action | Rough size |
|---|---|---|
| `src/routes/home.rs` | **edit** | +60-70 LOC (handler 2 new fetch blocks + 8 new HomeTemplate fields + factory updates); LOC TARGET ≤ 2000 (currently 1948) — AC14 trim + post-construction field assignment required; fallback extraction available |
| `src/routes/home_indicators.rs` | **edit** | +20 LOC (RECENT_ACTIVITY_DAYS const + 2 new enum variants + 2 new parser arms + 2 new helper if-blocks; UPDATE 12 existing tests with 0,0; 5 new helper unit tests + 1 constant test) |
| `src/routes/home_indicator_tests.rs` | **edit** | +200 LOC (14 new render tests using post-construction field assignment + small helper; ALL render tests live here per AC14) |
| `src/models/title.rs` | **edit** | +60 LOC (`count_recent_cataloged` + `list_recent_cataloged` async fns) |
| `src/models/loan.rs` | **edit** | +60 LOC (`count_recent_returns` + `list_recent_returns` async fns) |
| `templates/pages/home.html` | **edit** | +50-60 LOC (the 2 `{% else if %}` branches + the 2 `<section>` bodies) |
| `locales/en.yml` | **edit** | +8 lines under `dashboard.attention:` |
| `locales/fr.yml` | **edit** | +8 lines under `dashboard.attention:` |
| `tests/dashboard_recent_activity.rs` | **create** | ~250-300 LOC (10 `#[sqlx::test]` cases + helpers including `insert_title_with_created_at` + `mark_loan_returned_at`) |
| `tests/e2e/specs/journeys/home.spec.ts` | **edit** | +60-80 LOC (1 new `test.describe` block, 2 tests covering both indicators) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **edit** | only the `9-7-...` line + `last_updated` (per CLAUDE.md rule 16) |
| `_bmad-output/implementation-artifacts/9-7-recent-activity-indicators.md` | **edit** | this file — Status, Tasks checked, Dev Agent Record on completion |
| `CLAUDE.md` | **edit** | +1 line under "Architecture / Key Patterns" — note the `RECENT_ACTIVITY_DAYS` hardcoded constant per AC7 |

### Testing requirements

- **Coverage target:** every AC has at least one test (unit OR E2E). AC13 (CSP) is covered by `templates_audit.rs::no_inline_markup_in_templates` (existing — must stay green). AC14 (LOC) is verified by the `wc -l` step in Task 9 + 10. AC15 (chapter close) is documented in the Dev Agent Record.
- **AC2 anonymous-no-leak** is the load-bearing security invariant. The 2 render tests `home_anonymous_does_not_render_recent_cataloged_tag` + `home_anonymous_does_not_render_recent_returns_tag` are the primary regression guards; the E2E anonymous test is a secondary integration-level guard.
- **AC7 hardcoded constant** is locked by the `recent_activity_window_constant_is_seven_days` unit test. Without it, a future drive-by refactor could silently change the value.
- **AC4 enum + parser updates:** the 2 positive recognition tests + the case-sensitive extension lock the parser contract.
- **AC6 6-way mutual exclusion** of all 6 list-section slots is the load-bearing layout invariant. The 2 active-state render tests (`home_librarian_recent_cataloged_filter_active_renders_only_recent_cataloged_list_section` + symmetric for recent_returns) are the regression guards — each asserts ALL OTHER 5 sections are NOT present.
- **AC9 emit order** — `build_indicator_tags_emits_all_five_tags_in_priority_order_when_all_present` is the regression guard at the helper level; `home_renders_all_five_indicator_tags_in_priority_order` is the regression guard at the rendered-HTML level.
- **AC11 boundary** — `count_recent_cataloged_window_boundary` and `count_recent_returns_window_boundary` lock the inclusive `>=` semantic per FR wording "last 7 days". A future "harmonize the boundary" refactor that switches to strict `>` would break these tests loudly.
- **E2E** keeps to 1 anonymous + 1 librarian smoke test (the librarian one covers BOTH indicators in one test for parsimony).

### Project structure notes

This story closes the indicator subsystem chapter (5/5 indicators delivered). Three intentional design decisions worth flagging:

1. **Both new indicators reuse existing structs (`SearchResult` for titles, `LoanWithDetails` for loans).** Story 9-6 created `SeriesWithGap` because no existing struct had the right shape (the dashboard needed only id+name+total+owned, not the full `SeriesModel`). Here the situation is different: `SearchResult` already has every field TitleCard needs (it's used by `#recent-additions` since 9-2), and `LoanWithDetails` already has every field LoanRow needs (it's used by `#overdue-list` since 9-5). NEW projections would be parallel-shape duplication — DON'T do it.

2. **Symmetric role gating returns to be the default.** The 9-6 Gaps asymmetry was the EXCEPTION (anonymous-allowed series browsing per FR65 + FR95). Both new indicators in this story are Librarian-only — the established pattern from 9-4/9-5 returns. The `role_gated_indicator_filter` helper handles both new variants without modification, and the 2 `*_filter_active` slot booleans use the role-gated `active_indicator_filter` (NOT the raw `parsed_indicator`) — exactly mirroring the 9-4/9-5 pattern. NO per-variant role-gating bypasses needed.

3. **`RECENT_ACTIVITY_DAYS` is a deliberately hardcoded constant in v1.** The spec freeze (epics.md:1325) explicitly defers admin-configurability ("if the user later requests configurability, it becomes a settings story"). The constant lives in `home_indicators.rs` (not `config.rs`) precisely to signal "v1 hardcoded — extract to AppSettings if a user requests"; the 4 model methods take `days: i32` as a parameter (NOT a hardcoded inline literal) so the future migration path is to (a) extract the constant to `AppSettings.recent_activity_days`, (b) add `state.recent_activity_days()` accessor, (c) replace the constant references at the 4 call sites — a focused 4-line diff.

4. **HomeTemplate field-count growth — deferred cleanup signal escalates.** Pre-9-4 HomeTemplate had ~55 fields. 9-4 added 6 (~61). 9-5 added 7 (~68). 9-6 added 5 (~73). 9-7 adds 8 (~81). The struct is now visibly cluttered and the test factories are getting unwieldy. After 9-7 ships, Epic 9's indicator chapter is closed and a refactor PR to introduce a `DashboardSlots` substruct (or per-indicator `IndicatorPanel { active, items, heading, empty_label }` clusters) is a natural next move — file as `type:code-review-finding` GH Issue at story close. OUT OF SCOPE for 9-7 itself (refactor-during-feature is anti-pattern).

5. **`home_indicator_tests.rs` continues to absorb ALL new render tests — ZERO LOC added to home.rs for Task 7.** This keeps `home.rs` lean and prevents the LOC ceiling from being hit by the test surface alone. The 9-6 extraction precedent pays off again here.

The 9-4 FilterTag macro + 9-5/9-6 indicator-section-swap precedents stay the model — no template-component edits, only data-side wiring + 2 new HTML branches in `home.html`.

### Schema reality check (drift discoveries from spec text)

Drift discoveries this spec has factored in:

- **`titles.created_at`** — `TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP` per `migrations/20260329000000_initial_schema.sql`. Inserts default to NOW(). The `insert_title_with_created_at` helper uses a 2-statement pattern (INSERT then UPDATE) to backdate for tests — the alternative (explicit `created_at = ?` in INSERT) works but is more verbose and would require updating every existing test helper.
- **`loans.returned_at`** — `TIMESTAMP NULL DEFAULT NULL` (returned_at is NULL while loan is active, set on return). Schema explicit; AC8 `returned_at IS NOT NULL` guard is essential.
- **`loans.loaned_at` (NOT `borrowed_at`)** — locked by 9-5 spec drift discovery. Story 9-7's `list_recent_returns` reuses the existing `LoanWithDetails` struct which uses the correct column name.
- **`titles.created_at` is `TIMESTAMP` not `DATETIME`** — SQLx may need `CAST(t.created_at AS DATETIME) AS created_at` in dynamic queries, but `list_recent_active` (9-2) already handles this implicitly via the existing projection. Mirror the 9-2 query shape exactly.
- **No composite index on `titles(created_at)` or `loans(returned_at)`** — explicitly NOT added in this story (AC8 rationale matches 9-5/9-6 deferral).

If a fresh schema drift is discovered during dev (e.g., `titles.created_at` has a different type than expected), document inline in the test helper AND in the Dev Agent Record's "drift discoveries" section.

## References

- [Story 9.7 spec — `_bmad-output/planning-artifacts/epics.md` lines 1317–1334](../planning-artifacts/epics.md)
- [Epic 9 scope note + indicator delivery split philosophy + final ordering — `epics.md` lines 1200–1206 + 1330](../planning-artifacts/epics.md)
- [PRD FR58 (actionable indicators), FR65 (anonymous browsing), FR69 (session timeout) — `_bmad-output/planning-artifacts/prd.md`](../planning-artifacts/prd.md)
- [UX-DR4 (FilterTag dual state, zero-count rule) — `_bmad-output/planning-artifacts/ux-design-specification.md`](../planning-artifacts/ux-design-specification.md)
- [Story 9-6 spec (canonical patterns: per-variant role-gating asymmetry, role_gated_indicator_filter, aggregate model fns, 4-way mutual exclusion, AC15 LOC extraction precedent — `home_indicator_tests.rs`) — `9-6-series-with-gaps-indicator.md`](./9-6-series-with-gaps-indicator.md)
- [Story 9-5 spec (canonical patterns: per-indicator section swap, soft-degrade + warn pattern, post-construction-field-assignment for new render tests, LOC trim playbook, mutual-exclusion 3-branch chain → extends to 6-branch in 9-7) — `9-5-overdue-loans-indicator.md`](./9-5-overdue-loans-indicator.md)
- [Story 9-4 spec (canonical patterns: FilterTag macro, IndicatorFilter enum + parser, build_indicator_tags helper) — `9-4-filtertag-and-unshelved-indicator.md`](./9-4-filtertag-and-unshelved-indicator.md)
- [Story 9-2 spec (canonical pattern for `list_recent_active` — single round-trip enriched projection that 9-7's `list_recent_cataloged` mirrors) — `9-2-dashboard-recent-additions.md`](./9-2-dashboard-recent-additions.md)
- [`CLAUDE.md` — Foundation Rules (#1 DRY, #7 E2E smoke per epic, #11 GitHub Issues, #12 ≤ 2000 LOC, #13 local testing, #14 one-branch-one-story, #15 draft PR, #16 sprint-status ownership, #18 CI gating)](../../CLAUDE.md)
- [Title schema — `migrations/20260329000000_initial_schema.sql:88-113` (`titles` table with `created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP`)](../../migrations/20260329000000_initial_schema.sql)
- [Title model — `src/models/title.rs` (`TitleModel`, `SearchResult`, `count_active`, `list_recent_active`)](../../src/models/title.rs)
- [Loan model — `src/models/loan.rs` (`LoanModel`, `LoanWithDetails`, `count_active`, `count_overdue`, `list_overdue`)](../../src/models/loan.rs)
- [Indicator subsystem — `src/routes/home_indicators.rs` (extend with 2 enum variants + parser arms + RECENT_ACTIVITY_DAYS constant)](../../src/routes/home_indicators.rs)
- [Home handler — `src/routes/home.rs` (post-9-6 with role_gated_indicator_filter usage; extend with 2 new fetch blocks)](../../src/routes/home.rs)
- [Home template (existing 4-branch slot — convert to 6-branch) — `templates/pages/home.html`](../../templates/pages/home.html)
- [FilterTag macro precedent — `templates/components/filter_tag.html` (story 9-4, unchanged)](../../templates/components/filter_tag.html)
- [Indicator render tests precedent — `src/routes/home_indicator_tests.rs` (story 9-6 extraction; ALL new render tests for 9-7 land here)](../../src/routes/home_indicator_tests.rs)
- [Dashboard integration test pattern — `tests/dashboard_overdue.rs` + `tests/dashboard_gaps.rs` (sibling shape for `tests/dashboard_recent_activity.rs`)](../../tests/dashboard_overdue.rs)

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
