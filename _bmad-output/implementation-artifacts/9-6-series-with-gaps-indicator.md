# Story 9.6: Indicator — series with gaps

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a librarian,
I want a "Series with gaps" tag on the home page that shows the live count and lets me jump to a list of incomplete closed series,
so that I can see at a glance how many series are missing volumes and plan acquisitions accordingly.

## Acceptance Criteria

1. **AC1 — Gaps tag joins the "What needs attention" section (librarian/admin only on `/`).** Given the home page (`/`) seen by a Librarian or Admin, when it renders, then the existing `#what-needs-attention` section (story 9-4) additionally displays an `id="filter-tag-gaps"` FilterTag pill — `"Series with gaps — N"` / `"Séries incomplètes — N"` — where N is the count of active closed series whose distinct filled positions count is strictly less than `total_volume_count`. When the section already contains the unshelved + overdue tags (9-4 + 9-5), the gaps tag is appended after them in the visual order **Unshelved → Overdue → Series with gaps** (matches the priority ordering finalized in story 9.7 AC: Unshelved → Overdue → Series with gaps → Recent cataloged → Recent returns). The tag is NEVER rendered for the Anonymous role on `/` — same `#what-needs-attention` section gating as 9-4/9-5 (the section hides entirely when `indicator_tags.is_empty()`, which is always true for Anonymous).

2. **AC2 — Anonymous CAN use `/?filter=gaps` (ASYMMETRIC vs unshelved/overdue — series browsing is anonymous-permitted per FR65 + FR95).** This is the load-bearing deviation from the 9-4/9-5 anonymous-no-leak pattern. Series listings ARE anonymous-readable (FR95 — `series_list_page` at `src/routes/series.rs:82` requires no auth). Therefore:
   - Given an Anonymous user navigating to `/?filter=gaps`, when handled, then the parser returns `Some(IndicatorFilter::Gaps)` AND the handler runs `series::list_with_gaps` AND renders `#gaps-list` in place of `#recent-additions` — same swap UX as Librarian sees.
   - Given an Anonymous user landing on `/` (no `?filter=`), when rendered, then the `#what-needs-attention` section is NOT present (no tag emitted — `gaps_count` is forced to 0 for Anonymous so the tag's count > 0 emit-rule fails) AND no `series::count_with_gaps` query is issued (no DB load on a surface the user won't see).
   - Given an Anonymous user crafts `/?filter=unshelved` or `/?filter=overdue`, when handled, then those filters are STILL ignored (Librarian-gated content) — only `Gaps` is anonymous-allowed.
   - **Implementation (CORRECTED 2026-05-03 after CI catch on PR #121):** the asymmetry lives ENTIRELY in `gaps_filter_active`, NOT in `active_indicator_filter`. `active_indicator_filter` (which drives the TAG via `build_indicator_tags`) MUST be role-gated for ALL variants — including Gaps — because the AC3 escape-hatch rule emits an active-state pill at count=0 if `active=Some(Gaps)` and the tag would leak to Anonymous. The role gate is encapsulated in `routes::home_indicators::role_gated_indicator_filter(parsed, role)` (anonymous always gets `None`). The Gaps anonymous-allowed asymmetry is implemented via a SEPARATE boolean `gaps_filter_active = parsed_indicator == Some(IndicatorFilter::Gaps)` (no role gate), which drives the section swap. Two regression guards: (1) unit tests `role_gated_indicator_filter_anonymous_strips_all_variants` + `role_gated_indicator_filter_librarian_and_admin_pass_through_all_variants` in `home_indicators.rs::tests`; (2) E2E test `home.spec.ts::anonymous: tag never rendered, BUT /?filter=gaps shows the list (AC2 asymmetry)`.

3. **AC3 — Zero-count rule + active-state escape hatch (FilterTag contract).** Given `count_with_gaps` returns 0 AND no gaps filter is active, when the section renders, then the gaps tag is hidden (UX-DR4 zero-count rule, enforced by `build_indicator_tags`). Given `count_with_gaps` returns 0 AND `?filter=gaps` IS the active URL filter (e.g., the librarian just acquired the last missing volume from the filtered view), then the gaps tag IS still emitted in **active state** (label + ✕, href `/`) so the user has a visible escape hatch — same code-review patch contract that 9-4/9-5 added to their tags. The `#what-needs-attention` section likewise hides only when `indicator_tags.is_empty()` after all three contributions. **Asymmetry note:** the active-escape-hatch tag is ONLY emitted for Librarian/Admin on `/?filter=gaps` (Anonymous never gets a tag — for them the escape hatch is the browser back button, since they don't see the tag in the first place).

4. **AC4 — `IndicatorFilter::Gaps` enum variant + parser recognition.** The `IndicatorFilter` closed enum (introduced in 9-4, extended in 9-5) gains a third variant `Gaps`. `parse_indicator_filter` in `src/routes/home_indicators.rs` recognizes `Some("gaps")` → `Some(IndicatorFilter::Gaps)` (case-sensitive, like `"unshelved"` and `"overdue"`). The 9-5 test `parse_indicator_filter_unknown_bare_name_returns_none` (currently at `home_indicators.rs:208-218`) asserts `"gaps"` returns `None` with comment `"gaps is reserved for story 9-6 — not yet recognized"` — this story **DELETES that line + comment** and adds a new positive test `parse_indicator_filter_gaps_recognized` mirroring `parse_indicator_filter_unshelved_recognized` and `parse_indicator_filter_overdue_recognized`. Also **add a new reservation** for the next-up bare name: `"recent-cataloged"` (story 9-7), with comment `"recent-cataloged is reserved for story 9-7 — not yet recognized"` — keeps the warn-and-ignore branch covered. **EXTEND** the `parse_indicator_filter_case_sensitive` test (currently at `home_indicators.rs:157-174`) with `"GAPS"` and `"Gaps"` assertions, both → `None`.

5. **AC5 — Single-active-filter precedence carries over.** AC7 from 9-4 (single active indicator filter; legacy `parse_filter`/`?q=`/`?sort=` ignored when an indicator is active; HTMX search-fragment branch naturally short-circuits) applies unchanged for `?filter=gaps`. The handler logic at `home.rs:145-175` already routes any `IndicatorFilter` variant through the same precedence path — no rewrite needed; only the per-indicator `*_filter_active` slot booleans differ. **4-way mutual exclusion (NEW invariant):** at most ONE of `{unshelved_filter_active, overdue_filter_active, gaps_filter_active}` may be true at a time (the URL has one `?filter=` value; the parser returns one `IndicatorFilter` variant). Asserted by a new render test `home_librarian_gaps_filter_active_renders_gaps_list_not_unshelved_list_nor_overdue_list_nor_recent_additions` (or split into per-pair assertions if the single test name balloons).

6. **AC6 — Gaps filter swaps the recent-additions slot, mutually exclusive with unshelved-list AND overdue-list.** Given a user (any role, per AC2) navigating to `/?filter=gaps`, when the home page renders, then the `#recent-additions` section is REPLACED by an `#gaps-list` section in the SAME DOM position (the same slot 9-4 introduced for `#unshelved-list` and 9-5 extended with `#overdue-list`). All FOUR sections — `#recent-additions`, `#unshelved-list`, `#overdue-list`, `#gaps-list` — are mutually exclusive in the rendered HTML; only ONE renders at a time. The gaps-list section shows:
   - Heading: `{{ gaps_heading }}` ("Series with gaps" / "Séries incomplètes"), pre-translated by handler.
   - When `gaps_series.is_empty()` (race-empty defensive path — count > 0 but list query returned 0; OR active-state escape hatch case where count = 0): the `{{ gaps_empty_label }}` copy ("No incomplete series — your collection is whole!" / "Aucune série incomplète — votre collection est complète !") inside the same section wrapper, mirroring 9-4's `#unshelved-list` and 9-5's `#overdue-list` empty-state shapes.
   - When non-empty: a `<ul class="mt-3 space-y-2">` with one `<li>` per gappy series (`LIMIT 100`). Each row is wrapped in `<a href="/series/{series_id}">` (matches the existing series-detail route at `src/routes/series.rs:186` — destination is the series-detail page where the user sees the full SeriesGapGrid via UX-DR16 / `templates/components/series_gap_grid.html`). Each row shows: series name, `{owned_count}/{total_volume_count}` ratio, and a gap-count badge ("N missing" / "N manquants") with the existing `text-red-600 dark:text-red-400` color treatment (mirroring the gap badge color on `series_list.html:51` — see Source tree references). Tailwind utility classes only (no inline styles). **Do NOT inline the SeriesGapGrid component (`templates/components/series_gap_grid.html`) into the dashboard rows** — that component loops per-position and is designed for the series-detail page; the dashboard row is intentionally compact (name + ratio + badge), and the row link to `/series/{id}` gives the user the full grid one click away.

7. **AC7 — Open series NEVER counted; only Closed series with declared total > 0.** Given a series with `series_type = 'open'`, when evaluated, then it is NEVER counted as having gaps (open series have no defined "completeness" — FR54). Given a series with `series_type = 'closed'` AND `total_volume_count IS NULL OR total_volume_count <= 0`, when evaluated, then it is NEVER counted (a closed series without a positive total is a data-integrity edge case — likely user-in-progress; treat as "no defined gap" and exclude). Given a series with `series_type = 'closed'` AND `total_volume_count > 0` AND `assignments_count < total_volume_count`, when evaluated, then it IS counted (and listed). **`assignments_count` semantics:** counts DISTINCT `title_series.position_number` rows for that series where `deleted_at IS NULL` — this is critical because (a) BD omnibus titles populate multiple position rows for the same `title_id` (story 5-5), and (b) two different titles assigned to the same position (data-error, but possible) should not double-count. `COUNT(DISTINCT position_number)` is the precise count of FILLED slots. The existing `compute_gap` helper at `src/routes/series.rs:19-26` uses `total - owned` where `owned` is `active_count_titles` (counts assignment rows, NOT distinct positions — see Anti-patterns); the dashboard aggregate query MUST use `COUNT(DISTINCT position_number)` for accuracy.

8. **AC8 — `series::count_with_gaps` + `series::list_with_gaps` model methods (NEW — no existing aggregate function to reuse).** Two new functions on `SeriesModel` (`src/models/series.rs`), patterned after the count + list shape that 9-4 (`volume::count_unshelved` / `list_unshelved`) and 9-5 (`loan::count_overdue` / `list_overdue`) established. **Spec drift discovery:** the epics.md AC text "reuses the existing series-with-gaps service function from `src/services/series.rs` (extracted in Epic 5)" is INCORRECT — no such aggregate function exists. The only existing helpers are (a) `services::series::SeriesService::get_series_positions` (per-single-series, builds the full grid for the detail page) and (b) the private `compute_gap` helper at `src/routes/series.rs:19-26` (closed/open + `total - owned`). Neither is suitable for an aggregate "list ALL series with gaps" query without N+1. Document this drift in the Dev Agent Record at story close. Signatures:
   ```rust
   pub async fn count_with_gaps(pool: &DbPool) -> Result<i64, AppError>
   pub async fn list_with_gaps(pool: &DbPool, limit: u32) -> Result<Vec<SeriesWithGap>, AppError>
   ```
   - **`count_with_gaps`** SQL (single round-trip, correlated subquery):
     ```sql
     SELECT COUNT(*) FROM series s
     WHERE s.deleted_at IS NULL
       AND s.series_type = 'closed'
       AND s.total_volume_count IS NOT NULL
       AND s.total_volume_count > 0
       AND s.total_volume_count > (
         SELECT COUNT(DISTINCT ts.position_number)
         FROM title_series ts
         WHERE ts.series_id = s.id AND ts.deleted_at IS NULL
       )
     ```
     Use `sqlx::query_as::<_, (i64,)>` (mirror `volume::count_unshelved` and `loan::count_overdue`).
   - **`list_with_gaps`** SQL (single round-trip via `LEFT JOIN` + derived table to compute the aggregate once):
     ```sql
     SELECT
       s.id, s.name, s.total_volume_count,
       COALESCE(filled.owned_count, 0) AS owned_count
     FROM series s
     LEFT JOIN (
       SELECT series_id, COUNT(DISTINCT position_number) AS owned_count
       FROM title_series
       WHERE deleted_at IS NULL
       GROUP BY series_id
     ) filled ON filled.series_id = s.id
     WHERE s.deleted_at IS NULL
       AND s.series_type = 'closed'
       AND s.total_volume_count IS NOT NULL
       AND s.total_volume_count > 0
       AND s.total_volume_count > COALESCE(filled.owned_count, 0)
     ORDER BY (s.total_volume_count - COALESCE(filled.owned_count, 0)) DESC, s.name ASC
     LIMIT ?
     ```
     Returns `Vec<SeriesWithGap>` (NEW struct — see AC9). `ORDER BY` is `gap_count DESC, name ASC` so the most-incomplete series surface first; ties by name.
   - **`SeriesWithGap` projection struct** (NEW, in `src/models/series.rs`):
     ```rust
     #[derive(Debug, Clone, sqlx::FromRow)]
     pub struct SeriesWithGap {
         pub id: u64,
         pub name: String,
         pub total_volume_count: i32,
         pub owned_count: i64,  // i64 because COUNT() returns BIGINT in MariaDB
     }
     impl SeriesWithGap {
         /// Computed gap count: total - owned, never negative. Always
         /// positive in practice because the query's WHERE clause filters
         /// to `total > owned` rows, but the saturating math makes the
         /// helper robust for tests + future callers that don't pre-filter.
         pub fn gap_count(&self) -> u64 {
             (self.total_volume_count as i64 - self.owned_count).max(0) as u64
         }
     }
     ```
     **DO NOT REUSE `SeriesListRow` from `src/routes/series.rs:41-45`** — that struct embeds the full `SeriesModel` (with description, version, type, etc.) and exists for the series-list page; the dashboard row only needs id + name + ratio + gap. A focused projection struct keeps the row template simple and the SQL projection narrow (no unused columns over the wire).
   - **Schema realities** (verified against `migrations/20260329000000_initial_schema.sql:177-206`):
     - `series.series_type ENUM('open', 'closed') NOT NULL DEFAULT 'open'` — match string `'closed'`, NOT enum-cast.
     - `series.total_volume_count INT NULL` — nullable; the `IS NOT NULL AND > 0` guard is load-bearing.
     - `title_series.position_number INT NOT NULL` — never NULL; `COUNT(DISTINCT position_number)` is well-defined.
     - `title_series.deleted_at` — soft-deleted assignments DO NOT count as filled (correct: a soft-deleted assignment means the title was unassigned, so the position is genuinely empty again).
     - **No `unique_position_per_series` constraint** — `UNIQUE KEY uq_title_series_position (title_id, series_id, position_number)` enforces ONE title-position triple per series, but TWO DIFFERENT titles can theoretically claim the same position (data-error possible). `COUNT(DISTINCT position_number)` correctly de-dupes this case.
     - **No composite index on `(series_type, total_volume_count, deleted_at)`** — for a personal-library scale (typically < 500 series), the full-table scan with WHERE filter is acceptable v1. If a real deployment ever shows the count query taking > 50ms, file a `type:change-request` GH Issue to add `INDEX idx_series_gappy (series_type, deleted_at, total_volume_count)`. Do not add prematurely (matches 9-5's `idx_loans_overdue` deferral).
   - Both functions use **dynamic `query` / `query_as`** (NOT the macro `sqlx::query!`), per project convention — keeps `.sqlx/` cache untouched, mirrors story 9-2/9-3/9-4/9-5 anti-pattern note.

9. **AC9 — `SeriesWithGap` row template and i18n.** The `#gaps-list` row template renders, per AC6:
   ```jinja
   <li>
     <a href="/series/{{ row.id }}" class="block px-4 py-3 bg-stone-50 dark:bg-stone-800 hover:bg-stone-100 dark:hover:bg-stone-700 rounded-lg border border-stone-200 dark:border-stone-700 transition-colors">
       <div class="flex items-baseline gap-3 flex-wrap">
         <span class="font-medium text-stone-900 dark:text-stone-100 truncate">{{ row.name }}</span>
         <span class="text-sm text-stone-600 dark:text-stone-400 tabular-nums">{{ row.owned_count }}/{{ row.total_volume_count }}</span>
         <span class="ml-auto inline-flex items-center rounded-full bg-red-100 dark:bg-red-900/30 px-2 py-0.5 text-xs font-medium text-red-700 dark:text-red-300">
           {{ row.gap_count() }} {{ gaps_missing_label }}
         </span>
       </div>
     </a>
   </li>
   ```
   - The `row.gap_count()` method is invoked in the template (Askama supports method calls on field objects). Alternative: pre-compute in the handler by mapping `Vec<SeriesWithGap>` to a `Vec<DashboardSeriesRow>` view-model with `gap_count: u64` already populated — cheaper template, slightly more handler code. Pick whichever fits the existing pattern; `row.gap_count()` is shorter but Askama method invocation has tripped up CI in the past (e.g., when struct methods take `&mut self`); `gap_count(&self) -> u64` is `&self` so it should work, but verify with a render test before relying on it.
   - **Reuse the existing `series.gap_count` i18n key** at `locales/en.yml:145` ("Missing") and `locales/fr.yml:145` ("Manquants") for the gap-badge label (`gaps_missing_label`). NEW keys needed only for the section heading + empty state.

10. **AC10 — `build_indicator_tags` extended to take gaps inputs.** Update `build_indicator_tags` (in `src/routes/home_indicators.rs`) to accept a `gaps_count: i64` parameter as the 3rd positional arg (between `overdue_count` and `active`), AND emit the gaps tag after the overdue tag. Recommended signature:
    ```rust
    pub(crate) fn build_indicator_tags(
        unshelved_count: i64,
        overdue_count: i64,
        gaps_count: i64,
        active: Option<IndicatorFilter>,
        loc: &str,
    ) -> Vec<IndicatorTag>
    ```
    Behavior, in order:
    - Push the unshelved tag if `unshelved_count > 0 || active == Some(IndicatorFilter::Unshelved)` (unchanged from 9-4 contract).
    - Push the overdue tag if `overdue_count > 0 || active == Some(IndicatorFilter::Overdue)` (unchanged from 9-5 contract).
    - Push the gaps tag if `gaps_count > 0 || active == Some(IndicatorFilter::Gaps)`. Fields: `label = t!("dashboard.attention.gaps_label")`, `count = gaps_count.max(0) as u64`, `filter_name = "gaps"`, `is_active = (active == Some(IndicatorFilter::Gaps))`, `clear_aria_label = t!("dashboard.attention.gaps_clear_aria")`.
    - Order is load-bearing (AC1 says Unshelved → Overdue → Gaps). Asserted by `build_indicator_tags_emits_unshelved_then_overdue_then_gaps_when_all_present`.
    **All existing call sites + tests need updating:** the handler call at `home.rs:293-298` grows a new arg, and the 5 9-5 helper-unit tests in `home_indicators.rs::tests` (lines 280-353) need the new arg passed (use `0` to keep their existing assertions). **Update the existing `build_indicator_tags_*` 9-4 + 9-5 tests by adding `0` as the new gaps arg, NOT by deleting them** — the unshelved and overdue contracts must stay locked.

11. **AC11 — i18n EN + FR.** Append three new keys to the `dashboard.attention:` block (currently at `locales/en.yml:360-368` and `locales/fr.yml:360-368`):
    - `gaps_label` — EN: `"Series with gaps"`, FR: `"Séries incomplètes"`
    - `gaps_clear_aria` — EN: `"Clear filter: Series with gaps"`, FR: `"Retirer le filtre : Séries incomplètes"`
    - `gaps_heading` — EN: `"Series with gaps"`, FR: `"Séries incomplètes"` (used as `#gaps-list` heading; same text as `gaps_label` but kept separate so future copy can diverge — e.g., heading could become "Series with gaps · 5 incomplete")
    - `gaps_empty` — EN: `"No incomplete series — your collection is whole!"`, FR: `"Aucune série incomplète — votre collection est complète !"` (used in `#gaps-list` empty/race-empty/active-zero state)
    - **REUSED** (no new key): `series.gap_count` at `locales/{en,fr}.yml:145` ("Missing" / "Manquants") for the per-row gap-count badge label, plumbed via a new HomeTemplate field `gaps_missing_label`.
    - **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper — keys start at root. After editing, run `touch src/lib.rs && cargo build` to force the i18n proc-macro to re-read. The `i18n::audit::tests::all_t_keys_have_both_locales` test enforces EN/FR mirror.

12. **AC12 — Unit tests.**
    - **(a) `series::count_with_gaps` (DB-backed `#[sqlx::test]` in NEW `tests/dashboard_gaps.rs`):**
        - `count_with_gaps_on_empty_db_returns_zero` — fresh schema, no series, expect `0`.
        - `count_with_gaps_open_series_never_counted` — seed 3 open series with `total_volume_count = NULL` and 0 assignments; expect `0` (open series excluded by AC7).
        - `count_with_gaps_closed_with_null_total_excluded` — seed 1 closed series with `total_volume_count = NULL` and 0 assignments; expect `0` (data-integrity edge case excluded by AC7).
        - `count_with_gaps_closed_with_zero_total_excluded` — seed 1 closed series with `total_volume_count = 0`; expect `0` (zero-total guard).
        - `count_with_gaps_closed_full_not_counted` — seed 1 closed series total=5 with 5 distinct position assignments (positions 1-5); expect `0` (full series, no gaps).
        - `count_with_gaps_closed_partial_counted` — seed 1 closed series total=5 with 3 assignments at positions 1, 2, 4; expect `1` (gap at 3 + 5).
        - `count_with_gaps_excludes_soft_deleted_series` — seed 2 closed series with gaps, soft-delete one; expect `1`.
        - `count_with_gaps_soft_deleted_assignments_dont_fill_gaps` — seed 1 closed series total=5 with 5 assignments at positions 1-5, then soft-delete 2 of them (positions 3 + 4); expect `1` (positions 3 + 4 are now empty again per AC8 contract).
        - `count_with_gaps_distinct_positions` — seed 1 closed series total=5 with 6 assignment rows where positions are `[1, 1, 2, 3, 4, 5]` (data-error: duplicate position 1, but UNIQUE constraint allows different title_ids per position+series). Expect `0` — `COUNT(DISTINCT position_number)` is `5`, equals total. Validates the `DISTINCT` SQL clause.
        - `count_with_gaps_omnibus_fills_each_position` — seed 1 closed series total=5 with 1 omnibus title spanning positions 1-3 (3 rows, same title_id, distinct position_number 1, 2, 3) + 1 individual at position 5. Distinct positions = `[1, 2, 3, 5]` = 4. Expect `1` (gap at position 4). Validates AC7's `COUNT(DISTINCT position_number)` semantic for omnibus.
    - **(b) `series::list_with_gaps` (DB-backed, same file):**
        - `list_with_gaps_returns_in_gap_count_desc_then_name_asc_order_with_limit` — seed 3 closed series: "Tintin" total=24 with 18 distinct positions filled (gap=6); "Blacksad" total=10 with 5 distinct positions filled (gap=5); "Mortelle Adèle" total=20 with 14 distinct positions filled (gap=6). Call `list_with_gaps(pool, 100)`; assert exactly 3 rows in order `[(Mortelle Adèle, gap=6), (Tintin, gap=6), (Blacksad, gap=5)]` (gap DESC, then name ASC for the gap=6 tie). Verify `owned_count` and `total_volume_count` projection.
        - `list_with_gaps_honors_limit` — same fixture as above; call `list_with_gaps(pool, 1)`; assert exactly 1 row (Mortelle Adèle). Validates the LIMIT bind.
        - `list_with_gaps_excludes_soft_deleted_series` — seed 2 gappy series, soft-delete one; expect 1 row.
        - **Test-helper inserts:** the existing `tests/dashboard_overdue.rs` (story 9-5) defines `first_genre_id`, `first_volume_state_id`, `insert_title`, `insert_borrower`, `insert_loan` etc. For `dashboard_gaps.rs`, define sibling helpers:
            - `insert_series(pool, name, series_type, total) -> u64` — `INSERT INTO series (name, series_type, total_volume_count) VALUES (?, ?, ?)`. Pass `series_type` as `&str` (`"open"` or `"closed"`) to match the `ENUM('open', 'closed')` column directly.
            - `insert_title_series_assignment(pool, title_id, series_id, position, is_omnibus) -> u64` — `INSERT INTO title_series (title_id, series_id, position_number, is_omnibus) VALUES (?, ?, ?, ?)`.
            - `soft_delete_series(pool, series_id)` — `UPDATE series SET deleted_at = NOW() WHERE id = ?`.
            - `soft_delete_title_series_assignment(pool, assignment_id)` — `UPDATE title_series SET deleted_at = NOW() WHERE id = ?`.
            - Reuse `insert_title(pool, name)` from `dashboard_overdue.rs` (cross-file copy is acceptable per project precedent — both 9-4 and 9-5 already duplicate helpers across test files; a shared `tests/helpers.rs` module would be a follow-up cross-cutting story).
    - **(c) `parse_indicator_filter` (in `src/routes/home_indicators.rs::tests`):**
        - **NEW** `parse_indicator_filter_gaps_recognized` — assert `Some("gaps")` → `Some(IndicatorFilter::Gaps)`; mirror `parse_indicator_filter_overdue_recognized` verbatim with the new variant.
        - **EDIT** `parse_indicator_filter_unknown_bare_name_returns_none`: DELETE the `"gaps"` assertion + the trailing `"gaps is reserved for story 9-6 — not yet recognized"` comment. Add a new bare-name reservation: `"recent-cataloged"` with comment `"recent-cataloged is reserved for story 9-7 — not yet recognized"`.
        - **EXTEND** `parse_indicator_filter_case_sensitive` (currently at lines 157-174): add assertions `parse_indicator_filter(&Some("GAPS".to_string())) == None` and `parse_indicator_filter(&Some("Gaps".to_string())) == None`.
    - **(d) `build_indicator_tags` (in `src/routes/home_indicators.rs::tests`):**
        - **UPDATE** ALL existing `build_indicator_tags_*` tests (9-4 and 9-5 — currently 8 cases at lines 230-353) to pass `0` as the new `gaps_count` 3rd arg. They keep the same assertions (only the unshelved + overdue tags are exercised).
        - **NEW** `build_indicator_tags_gaps_nonzero_unshelved_zero_overdue_zero_returns_gaps_only` — `(0, 0, 5, None, "en")` → 1 tag, `filter_name = "gaps"`, `count = 5`, `is_active = false`, `label = "Series with gaps"`.
        - **NEW** `build_indicator_tags_emits_unshelved_then_overdue_then_gaps_when_all_present` — `(3, 5, 7, None, "en")` → 3 tags, `tags[0].filter_name == "unshelved"`, `tags[1].filter_name == "overdue"`, `tags[2].filter_name == "gaps"` (order is load-bearing per AC10).
        - **NEW** `build_indicator_tags_gaps_zero_count_with_active_filter_still_emits_active_tag` — `(0, 0, 0, Some(IndicatorFilter::Gaps), "en")` → 1 tag, `is_active = true`, `count = 0`. Mirrors the unshelved + overdue zero-count-active pattern (AC3 escape-hatch contract).
        - **NEW** `build_indicator_tags_gaps_active_keeps_others_in_default_state_when_counts_nonzero` — `(3, 5, 7, Some(IndicatorFilter::Gaps), "en")` → 3 tags, `tags[2].is_active == true && count == 7`, `tags[0].is_active == false && count == 3`, `tags[1].is_active == false && count == 5`. Locks down the matrix cell where gaps is the active filter alongside non-zero unshelved + overdue counts.
    - **(e) Handler render tests (extends `src/routes/home.rs::mod tests`):**
        - `home_anonymous_does_not_render_gaps_tag_on_default_home` — Anonymous role, no filter; assert `id="filter-tag-gaps"` is NOT present in HTML AND `id="gaps-list"` is NOT present (the section heading is just `#what-needs-attention` which hides on empty `indicator_tags`).
        - `home_anonymous_with_filter_gaps_renders_gaps_list_but_no_tag` — **AC2 LOAD-BEARING.** Anonymous role, `?filter=gaps`. Assert `id="gaps-list"` IS present (anonymous-allowed list) AND `id="filter-tag-gaps"` is NOT present (no tag for anonymous). This locks the asymmetric anonymous-allowed-list-without-tag contract.
        - `home_librarian_renders_gaps_tag_in_default_state_when_count_positive` — populated case: extend the test factory chain (or use post-construction field assignment per the 9-5 LOC-trim playbook) to take `(gaps_count, gaps_filter_active, gaps_series)`. Build a template with `indicator_tags = [unshelved=0, overdue=0, gaps=5]` and `gaps_filter_active = false`. Assert `id="filter-tag-gaps"` is present, `aria-label="Series with gaps: 5"`, `href="/?filter=gaps"`. Also assert `&times;` (the active-state ✕ marker) is NOT present and `Clear filter: Series with gaps` is NOT present (default-state negative assertions, mirroring 9-5's code-review patch contract).
        - `home_librarian_gaps_tag_active_state_when_filter_applied` — populated `gaps_series`, `gaps_filter_active = true`, `indicator_tag.is_active = true`. Assert `href="/"`, the visible `&times;` is present, active-state aria-label uses the clear copy. Also assert default-state aria-label is NOT present.
        - `home_librarian_gaps_filter_active_renders_gaps_list_not_unshelved_list_nor_overdue_list_nor_recent_additions` — populated case (`gaps_filter_active = true`, populated `gaps_series` Vec). Assert `id="gaps-list"` IS present AND `id="recent-additions"` is NOT present AND `id="unshelved-list"` is NOT present AND `id="overdue-list"` is NOT present (4-way mutual exclusion across all four slots in the same DOM position).
        - `home_librarian_gaps_filter_empty_renders_empty_label` — `gaps_filter_active = true`, EMPTY `gaps_series` Vec. Assert `id="gaps-list"` IS present AND the empty-label copy ("No incomplete series — your collection is whole!" / "Aucune série incomplète — votre collection est complète !") appears inside that section.
        - `home_renders_gaps_tag_after_overdue_in_attention_section` — populated case with all three indicators non-zero. Use `slice_section` + DOM order assertion to verify `id="filter-tag-unshelved"` < `id="filter-tag-overdue"` < `id="filter-tag-gaps"` in the HTML byte stream (mirrors 9-5's `home_renders_overdue_tag_after_unshelved_in_attention_section` order-pinning pattern, extended to all three).
        - `home_librarian_gaps_row_links_to_series_detail` — populated `gaps_series` with 1 row (id=42, name="Tintin", total=24, owned=18). Assert the rendered HTML contains `href="/series/42"` AND the row text contains `"18/24"` AND the gap count `"6"` appears.
    - **(f) FilterTag macro reuse — no NEW macro tests.** The 9-4 macro at `templates/components/filter_tag.html` is unchanged. Story 9-6 reuses it as-is. Adding macro tests for gaps-specific data would be redundant; the parameterization contract is locked by 9-4's 4-state matrix tests.

13. **AC13 — E2E (Foundation Rule #7).** Append a NEW `test.describe("Home page — Series with gaps indicator", ...)` block at the end of `tests/e2e/specs/journeys/home.spec.ts` (AFTER the 9-5 "Overdue loans indicator" describe at lines 216-275):
    - **Test 1 — anonymous CAN use the gaps filter (AC2 anonymous-allowed asymmetry).** Load `/` as anonymous; assert `#filter-tag-gaps` count == 0 (no tag for anonymous on default home). Navigate to `/?filter=gaps`; assert `#filter-tag-gaps` count == 0 (still no tag) AND `#gaps-list` count == 1 (list rendered for anonymous — the load-bearing deviation from 9-4/9-5). Asserts the asymmetric-role-gate contract end-to-end.
    - **Test 2 — librarian smoke (conditional empty-DB short-circuit).** `await loginAs(page, "librarian")`; load `/`. Read the count of `#filter-tag-gaps`. If `count === 0` AND no gappy-series fixture exists, short-circuit with a green pass (same defensive pattern as 9-4 Test 2 lines 180-186 and 9-5 Test 2). If `count === 1`: assert tag visible, default state, `href="/?filter=gaps"`. Click it; `await page.waitForURL(/\/\?filter=gaps/)`; assert `#gaps-list` is present AND `#recent-additions` is NOT AND `#unshelved-list` is NOT AND `#overdue-list` is NOT (4-way mutual exclusion). Click the active-state ✕ pill (`href="/"`); `await page.waitForURL(/\/$/)`; assert `#recent-additions` is back AND `#gaps-list` is gone.
    - Use i18n-aware regex matchers: `/Series with gaps|Séries incomplètes/i`. NO `waitForTimeout` (CI grep gate, enforced by `.github/workflows/_gates.yml::e2e`).
    - Selectors scoped to `#what-needs-attention` and `#gaps-list` to avoid the unscoped-selector flake class flagged by 9-2/9-3.
    - **No row-click navigation E2E.** A test that clicks a `#gaps-list` row → asserts `/series/<id>` opens would add CI runtime + new page interactions for marginal coverage over the unit test `home_librarian_gaps_row_links_to_series_detail`. Skip; document in the Dev Agent Record.

14. **AC14 — CSP compliance.** No `style="..."`, no `<style>`, no `<script>`, no `onclick=`, no inline event handlers in any template touched by this story. The gaps-list rows reuse the same Tailwind class palette as the unshelved + overdue sections (`bg-stone-50`, `text-red-700 dark:text-red-300`, `tabular-nums`, etc.) — no new color tokens, no new CSS file. The `src/templates_audit.rs::no_inline_markup_in_templates` test (line 44) MUST stay green. The 9-4 FilterTag macro is reused verbatim — no template-component edits.

15. **AC15 — Foundation Rule #12 — keep `src/routes/home.rs` ≤ 2000 LOC.** `src/routes/home.rs` is at **1967 LOC** as of 9-5 close. Adding 9-6 substance (HomeTemplate +5-7 fields ≈ 7 LOC, handler block ≈ 25 LOC, 8 new render tests ≈ 100-130 LOC, factory extension or post-construction field assignment ≈ 0-30 LOC) would push the file over 2000. **Mitigation strategy in this story:**
    - **First priority** — use post-construction field assignment for ALL new render tests (NO new sibling factory variant), tighten doc-comments on new tests to one short sentence, follow the 9-5 "Task 9 LOC trim" debug-log playbook.
    - **Fallback** — if `wc -l src/routes/home.rs` after Task 9 exceeds 2000, extract the indicator-test helpers `make_test_home_template_with_indicators` (currently at `home.rs:1029-1044`), `fake_indicator_tag` (`home.rs:1046-1054`), and `fake_loan_with_details` (`home.rs:1067-1090`) into a new `pub(crate) mod test_factories;` submodule inside `home_indicators.rs::tests` (or a sibling `home_test_helpers.rs` if the visibility math gets ugly). All three are conceptually about indicator-rendering machinery and have ZERO callers outside the indicator render tests; the move is mechanical. Net savings: ~60-80 LOC.
    - **Verification** — Task 10 includes a `wc -l src/routes/home.rs` step that fails the story if the file exceeds 2000 LOC. Foundation Rule #12 is non-negotiable.

## Tasks / Subtasks

- [x] **Task 1 — `series::count_with_gaps` + `series::list_with_gaps` model methods + `SeriesWithGap` projection (AC: 7, 8, 12a, 12b)**
  - [ ] In `src/models/series.rs`, add the `SeriesWithGap` projection struct + `gap_count()` `&self` accessor per AC8. Place AFTER the existing `SeriesModel` impl block (around line 213) and BEFORE the `TitleSeriesRow` definition at line 234. Mark as `pub` (the dashboard handler + Askama template both need it; field reachability from `HomeTemplate.gaps_series: Vec<SeriesWithGap>` requires `pub`).
  - [ ] Add `pub async fn count_with_gaps(pool: &DbPool) -> Result<i64, AppError>` on `SeriesModel`. Pattern: `sqlx::query_as::<_, (i64,)>(<sql>).fetch_one(pool).await?` returning `row.0`. SQL per AC8 (correlated subquery with COUNT DISTINCT + total > distinct-positions filter). Place AFTER `active_count_titles` (line 216) for thematic locality.
  - [ ] Add `pub async fn list_with_gaps(pool: &DbPool, limit: u32) -> Result<Vec<SeriesWithGap>, AppError>`. SQL per AC8 (single round-trip with `LEFT JOIN` derived table). Use `sqlx::query_as::<_, SeriesWithGap>(...)` (the `#[derive(sqlx::FromRow)]` on `SeriesWithGap` enables this). Bind `limit`. Project: `id`, `name`, `total_volume_count`, `owned_count` (no `created_at`, `updated_at`, `version`, `description` — narrow projection).
  - [ ] Both functions use **dynamic `query` / `query_as`** (NOT the macro `sqlx::query!`) — keeps `.sqlx/` cache untouched (project convention).
  - [ ] Build the integration test file `tests/dashboard_gaps.rs` (NEW, sibling of `dashboard_overdue.rs`):
    - Helpers: copy `first_genre_id`, `first_volume_state_id`, `insert_title` from `tests/dashboard_overdue.rs` (or `dashboard_unshelved.rs` — same shape). Cross-file helper duplication is acceptable per project precedent (a shared `tests/helpers.rs` module would be a separate cross-cutting story).
    - New helpers per AC12b: `insert_series(pool, name, series_type, total)` (note: pass `series_type` as `&str` `"open"` / `"closed"` to match the ENUM column); `insert_title_series_assignment(pool, title_id, series_id, position, is_omnibus)`; `soft_delete_series(pool, id)`; `soft_delete_title_series_assignment(pool, id)`.
    - 11 `#[sqlx::test(migrations = "./migrations")]` cases per AC12a (9) + AC12b (3 — but the limit + soft-delete-series + sort-order tests overlap; use 3 cases that cover all assertions).
  - [ ] Verify: `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test --test dashboard_gaps` — all green; lock as Commit 1 of the story branch.

- [x] **Task 2 — `IndicatorFilter::Gaps` variant + parser update (AC: 4, 12c)**
  - [ ] In `src/routes/home_indicators.rs`, add `Gaps` variant to the `IndicatorFilter` enum (currently at lines 22-31):
    ```rust
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum IndicatorFilter {
        Unshelved,
        Overdue,
        Gaps,
        // Reserved for follow-up: RecentCataloged (9-7), RecentReturns (9-7).
    }
    ```
  - [ ] Update `parse_indicator_filter` (lines 118-128) to match `Some("gaps") => Some(IndicatorFilter::Gaps)` BEFORE the `!v.contains(':') && !v.is_empty()` warn-and-ignore arm:
    ```rust
    pub(crate) fn parse_indicator_filter(filter: &Option<String>) -> Option<IndicatorFilter> {
        match filter.as_deref() {
            Some("unshelved") => Some(IndicatorFilter::Unshelved),
            Some("overdue") => Some(IndicatorFilter::Overdue),
            Some("gaps") => Some(IndicatorFilter::Gaps),
            Some(v) if !v.contains(':') && !v.is_empty() => {
                tracing::warn!(filter = %v, "Unknown indicator filter, ignoring");
                None
            }
            _ => None,
        }
    }
    ```
  - [ ] **EDIT** the existing `parse_indicator_filter_unknown_bare_name_returns_none` test (`home_indicators.rs:208-218`): DELETE the assertion `parse_indicator_filter(&Some("gaps".to_string())) == None` AND the trailing `"gaps is reserved for story 9-6 — not yet recognized"` comment. Replace with a new reservation: `parse_indicator_filter(&Some("recent-cataloged".to_string()))` → `None` with comment `"recent-cataloged is reserved for story 9-7 — not yet recognized"`. The test still proves the warn-and-ignore path on unknown bare names.
  - [ ] **NEW** `parse_indicator_filter_gaps_recognized` test mirroring `parse_indicator_filter_overdue_recognized`:
    ```rust
    #[test]
    fn parse_indicator_filter_gaps_recognized() {
        assert_eq!(
            parse_indicator_filter(&Some("gaps".to_string())),
            Some(IndicatorFilter::Gaps)
        );
    }
    ```
  - [ ] **EXTEND** the existing `parse_indicator_filter_case_sensitive` test (lines 157-174): add assertions `parse_indicator_filter(&Some("GAPS".to_string())) == None` and `parse_indicator_filter(&Some("Gaps".to_string())) == None`.
  - [ ] `cargo test home_indicators` — all parser tests green; lock as Commit 2.

- [x] **Task 3 — `build_indicator_tags` extension (AC: 10, 12d)**
  - [ ] In `src/routes/home_indicators.rs`, extend `build_indicator_tags` (lines 71-107) to accept `gaps_count: i64` as the 3rd parameter (between `overdue_count` and `active`):
    ```rust
    pub(crate) fn build_indicator_tags(
        unshelved_count: i64,
        overdue_count: i64,
        gaps_count: i64,
        active: Option<IndicatorFilter>,
        loc: &str,
    ) -> Vec<IndicatorTag> {
        let mut tags = Vec::new();
        // ... existing unshelved push ...
        // ... existing overdue push ...
        let gaps_is_active = active == Some(IndicatorFilter::Gaps);
        if gaps_count > 0 || gaps_is_active {
            tags.push(IndicatorTag {
                label: rust_i18n::t!("dashboard.attention.gaps_label", locale = loc).to_string(),
                count: gaps_count.max(0) as u64,
                filter_name: "gaps".to_string(),
                is_active: gaps_is_active,
                clear_aria_label: rust_i18n::t!(
                    "dashboard.attention.gaps_clear_aria",
                    locale = loc
                )
                .to_string(),
            });
        }
        tags
    }
    ```
  - [ ] **UPDATE** ALL 8 existing `build_indicator_tags_*` tests in `home_indicators.rs::tests` (lines 230-353) to pass `0` as the new 3rd `gaps_count` arg. They keep the same assertions (only the unshelved + overdue tags are exercised).
  - [ ] **NEW** unit tests per AC12d (4 cases). Place after the existing 9-5 tests so the file reads as a chronological extension (9-4 → 9-5 → 9-6).
  - [ ] Update the `home::home` handler's `build_indicator_tags` call site (`home.rs:293-298`) to pass `gaps_count` as the new 3rd arg.
  - [ ] `cargo test home_indicators` — all green; lock as Commit 3 (combined with Task 4 if cohesive).

- [x] **Task 4 — i18n keys (AC: 11)**
  - [ ] In `locales/en.yml`, append to the existing `dashboard.attention:` block (after `overdue_empty:` at line 368):
    ```yaml
        gaps_label: Series with gaps
        gaps_clear_aria: "Clear filter: Series with gaps"
        gaps_heading: Series with gaps
        gaps_empty: "No incomplete series — your collection is whole!"
    ```
  - [ ] In `locales/fr.yml`, mirror at the same path (after the FR `overdue_empty:` at line 368):
    ```yaml
        gaps_label: Séries incomplètes
        gaps_clear_aria: "Retirer le filtre : Séries incomplètes"
        gaps_heading: Séries incomplètes
        gaps_empty: "Aucune série incomplète — votre collection est complète !"
    ```
  - [ ] **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper — keys start at root. After editing, run `touch src/lib.rs && cargo build`. The `i18n::audit::tests::all_t_keys_have_both_locales` test enforces EN/FR mirror — keep them aligned exactly.
  - [ ] **REUSED keys** (no new add): `series.gap_count` at `locales/{en,fr}.yml:145` ("Missing" / "Manquants") for the per-row badge (plumbed via `gaps_missing_label` HomeTemplate field).

- [x] **Task 5 — Wire the home handler (AC: 1, 2, 5, 6)**
  - [ ] In `src/routes/home.rs::home`, the existing role gate at lines 145-149:
    ```rust
    let active_indicator_filter = if session.role >= Role::Librarian {
        parse_indicator_filter(&params.filter)
    } else {
        None
    };
    ```
    becomes per-variant. **REWRITE** as:
    ```rust
    // Story 9-6 AC2 — per-variant role gating. Unshelved + Overdue are
    // Librarian-only; Gaps is anonymous-permitted (FR65 + FR95: series
    // browsing is anonymous-allowed). The Anonymous user gets the
    // filter-active section swap but never the tag in
    // #what-needs-attention (which itself stays hidden for Anonymous).
    let parsed_indicator = parse_indicator_filter(&params.filter);
    let active_indicator_filter = match parsed_indicator {
        Some(IndicatorFilter::Gaps) => Some(IndicatorFilter::Gaps),
        Some(other) if session.role >= Role::Librarian => Some(other),
        _ => None,
    };
    ```
    The existing precedence-warning log at lines 150-156 stays unchanged (the `active_indicator_filter.is_some()` predicate still fires regardless of variant).
  - [ ] Immediately after the existing `overdue_loans` block (post-9-5, ~line 291), add the gaps data fetching:
    ```rust
    // Story 9-6 — Series with gaps indicator. Anonymous users can hit
    // /?filter=gaps and see the list (AC2 — series browsing is
    // anonymous-allowed per FR65 + FR95), but the count query for the
    // tag still skips for Anonymous (no DB load on a surface they won't
    // see — the tag lives in #what-needs-attention, which is empty
    // for Anonymous regardless).
    let gaps_count: i64 = if session.role >= Role::Librarian {
        match crate::models::series::SeriesModel::count_with_gaps(pool).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %e, "count_with_gaps failed; rendering 0 (tag hidden)");
                0
            }
        }
    } else {
        0
    };
    let gaps_filter_active = active_indicator_filter == Some(IndicatorFilter::Gaps);
    let gaps_series: Vec<crate::models::series::SeriesWithGap> = if gaps_filter_active {
        match crate::models::series::SeriesModel::list_with_gaps(pool, 100).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "list_with_gaps failed; rendering empty list");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    ```
    Note: `gaps_filter_active` does NOT have a role gate (deliberate — AC2). Compare to `unshelved_filter_active`/`overdue_filter_active` which both gate on `session.role >= Role::Librarian`.
  - [ ] Update the `build_indicator_tags` call (line ~293) to pass `gaps_count` as the new 3rd arg: `let indicator_tags = build_indicator_tags(unshelved_count, overdue_count, gaps_count, active_indicator_filter, loc);`
  - [ ] Extend `HomeTemplate` (struct at lines 31-111) with the new fields:
    - `pub gaps_filter_active: bool` — drives the AC6 swap.
    - `pub gaps_series: Vec<crate::models::series::SeriesWithGap>` — empty when `gaps_filter_active = false`; populated when active.
    - `pub gaps_heading: String` — pre-translated section heading.
    - `pub gaps_empty_label: String` — pre-translated empty-state copy.
    - `pub gaps_missing_label: String` — pre-translated "Missing" / "Manquants" badge label (REUSES existing `series.gap_count` key).
  - [ ] Pre-translate the three new labels in the handler (mirrors the existing pattern for `attention_heading`, `unshelved_heading`, `overdue_heading`).
  - [ ] Update the test factory `make_test_home_template` (default constructor at lines 891+) to populate the 5 new fields with sensible defaults (`gaps_filter_active: false`, `gaps_series: Vec::new()`, the three Strings as `String::new()` or short defaults so existing tests keep passing).
  - [ ] **AC5 4-way mutual exclusion** falls out for free: `unshelved_filter_active`, `overdue_filter_active`, and `gaps_filter_active` cannot all be true (the URL has one `?filter=` value; the parser returns one variant). The template's `{% if %}{% else if %}{% else if %}{% else %}` chain (Task 6) enforces it visually. No new handler logic needed.

- [x] **Task 6 — Render `#gaps-list` section in `home.html` (AC: 6, 9, 14)**
  - [ ] Edit `templates/pages/home.html` lines 124-222 (the existing 3-branch `{% if unshelved_filter_active %}{% else if overdue_filter_active %}{% else %}` block). Convert to a 4-branch `{% if %}{% else if %}{% else if %}{% else %}` chain by adding a new `{% else if gaps_filter_active %}` branch BETWEEN the overdue branch (currently ends at line 184) and the recent-additions `{% else %}` branch (currently starts at line 185):
    ```jinja
    {% else if gaps_filter_active %}
    {# Story 9-6 AC6: Series with gaps list. Each row links to
       /series/{id} where the user sees the full SeriesGapGrid
       (UX-DR16). Color treatment for the gap-count badge mirrors
       templates/pages/series_list.html line 51 (red text on red
       background). Tailwind utility classes only, zero inline styles. #}
    <section id="gaps-list" aria-labelledby="gaps-list-heading" class="w-full max-w-4xl mt-6">
        <h2 id="gaps-list-heading" class="text-sm font-medium text-stone-600 dark:text-stone-400 uppercase tracking-wide">{{ gaps_heading }}</h2>
        {% if gaps_series.is_empty() %}
            <div class="text-center py-12 text-stone-500 dark:text-stone-400">{{ gaps_empty_label }}</div>
        {% else %}
            <ul class="mt-3 space-y-2">
                {% for row in gaps_series %}
                <li>
                    <a href="/series/{{ row.id }}" class="block px-4 py-3 bg-stone-50 dark:bg-stone-800 hover:bg-stone-100 dark:hover:bg-stone-700 rounded-lg border border-stone-200 dark:border-stone-700 transition-colors">
                        <div class="flex items-baseline gap-3 flex-wrap">
                            <span class="font-medium text-stone-900 dark:text-stone-100 truncate">{{ row.name }}</span>
                            <span class="text-sm text-stone-600 dark:text-stone-400 tabular-nums">{{ row.owned_count }}/{{ row.total_volume_count }}</span>
                            <span class="ml-auto inline-flex items-center rounded-full bg-red-100 dark:bg-red-900/30 px-2 py-0.5 text-xs font-medium text-red-700 dark:text-red-300">
                                {{ row.gap_count() }} {{ gaps_missing_label }}
                            </span>
                        </div>
                    </a>
                </li>
                {% endfor %}
            </ul>
        {% endif %}
    </section>
    ```
    Update the trailing comment at line 222 from `{# /unshelved_filter_active or overdue_filter_active #}` to `{# /unshelved_filter_active or overdue_filter_active or gaps_filter_active #}`.
  - [ ] CSP: zero `style="..."`, zero `<script>`, zero `onclick=`. The badge color treatment is class-driven (`bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-300`), matching the established gap-badge palette from `series_list.html:51`. The `templates_audit::no_inline_markup_in_templates` test (line 44) MUST stay green after this change.

- [x] **Task 7 — Render tests in `src/routes/home.rs::mod tests` (AC: 12e)**
  - [ ] Add the 8 new render tests per AC12e. Use **post-construction field assignment** (`let mut t = make_test_home_template_with_indicators(...); t.gaps_filter_active = true; t.gaps_series = vec![...]; t.gaps_heading = "Series with gaps".to_string(); t.gaps_empty_label = "...".to_string(); t.gaps_missing_label = "Missing".to_string();`) — DO NOT add a new sibling factory variant `make_test_home_template_with_gaps_indicator`. This keeps `home.rs` LOC budget headroom per AC15 and matches the 9-5 LOC-trim playbook.
  - [ ] Add a small helper `fn fake_series_with_gap(id: u64, name: &str, total: i32, owned: i64) -> SeriesWithGap` next to `fake_indicator_tag` and `fake_loan_with_details` (lines 1046-1090). ~8 LOC.
  - [ ] **Tighten doc-comments** on the new render tests — one short sentence each (mirroring 9-5's "Task 9 LOC trim" mitigation).

- [x] **Task 8 — E2E spec (AC: 13)**
  - [ ] In `tests/e2e/specs/journeys/home.spec.ts`, append a new `test.describe("Home page — Series with gaps indicator", ...)` block AFTER the 9-5 "Overdue loans indicator" describe (line 275ish — verify against the live file at task start). 2 tests per AC13.
  - [ ] **Test 1** (anonymous) is the load-bearing AC2 contract — no `loginAs()` call, just `await page.goto("/?filter=gaps")` and assert the dual contract: `#filter-tag-gaps` count == 0 AND `#gaps-list` count == 1.
  - [ ] **Test 2** (librarian smoke) follows the 9-5 conditional empty-DB short-circuit pattern verbatim — copy the structure, change selectors and i18n regex.
  - [ ] Use scoped selectors: `page.locator('#what-needs-attention #filter-tag-gaps')` not `page.locator('#filter-tag-gaps')`. Mirrors the 9-2/9-3 unscoped-selector flake fix.
  - [ ] No `waitForTimeout`. CI grep gate enforces this; the `_gates.yml::e2e` job rejects PRs that violate.

- [x] **Task 9 — LOC budget enforcement (AC: 15)**
  - [ ] After Tasks 5-7 land, run `wc -l src/routes/home.rs`. Target: ≤ 2000 LOC (Foundation Rule #12).
  - [ ] If 2000 < x ≤ 2050: trim doc-comments on the new render tests + handler block, follow the 9-5 "Task 9 LOC trim" pattern (one-line doc-comments instead of multi-line; remove redundant explanatory comments; combine adjacent tests if they share fixtures).
  - [ ] If x > 2050 OR trimming alone doesn't get under 2000: extract `make_test_home_template_with_indicators` (lines 1029-1044), `fake_indicator_tag` (lines 1046-1054), `fake_loan_with_details` (lines 1067-1090), AND the new `fake_series_with_gap` helper (Task 7) into a new `pub(crate) mod test_factories;` submodule inside `src/routes/home_indicators.rs::tests` (or a sibling `src/routes/home_test_helpers.rs` if visibility math gets ugly). All four are conceptually about indicator-rendering machinery and have ZERO callers outside the indicator render tests; the move is mechanical.
  - [ ] Re-run `cargo test` after the move to verify no test name collisions and all imports resolve.

- [x] **Task 10 — Verify and document (AC: 1–15)**
  - [ ] `wc -l src/routes/home.rs` — verify ≤ 2000 LOC. Foundation Rule #12 must hold.
  - [ ] `SQLX_OFFLINE=true cargo check && cargo clippy --all-targets -- -D warnings` — clean (zero warnings).
  - [ ] `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test` — full suite green. Expected: ~682 lib tests baseline + ~13 new (8 render + 4 helper + 1 model) = ~695 lib; +11 new integration tests in `tests/dashboard_gaps.rs`. All 9-1/9-2/9-3/9-4/9-5 dashboard_* + indicator unit tests unchanged.
  - [ ] `cargo sqlx prepare --check --workspace` — expected no diff (Tasks 1 + 2 + 3 use dynamic `query` / `query_as`).
  - [ ] Tailwind rebuild — `npx @tailwindcss/cli -i static/css/input.css -o static/css/output.css --minify`. Verify any new utility classes used (`ml-auto`, `inline-flex`, `tabular-nums`, `rounded-full`, `bg-red-100 dark:bg-red-900/30`, etc.) are already present in compiled `output.css`. They should be — same palette as 9-5's `#overdue-list` badge.
  - [ ] Manual smoke from a running dev instance (`MYBIBLI_SKIP_SETUP=1 cargo run`):
    - As anonymous: `curl http://localhost:8080/` and grep — `id="filter-tag-gaps"` MUST NOT appear; `id="gaps-list"` MUST NOT appear.
    - As anonymous: `curl http://localhost:8080/?filter=gaps` and grep — `id="filter-tag-gaps"` MUST NOT appear (no tag for anonymous) BUT `id="gaps-list"` MUST appear (anonymous-allowed list — the AC2 asymmetry).
    - As librarian (login first): `curl` with the session cookie → grep — `id="filter-tag-gaps"` appears IFF gaps count > 0 OR filter is active.
    - Click the tag in a browser → URL changes to `/?filter=gaps` → `#gaps-list` replaces `#recent-additions` → click ✕ → URL returns to `/` → recent-additions back. Verify a row click navigates to `/series/<id>`. Repeat with the unshelved + overdue tags to confirm 4-way mutual exclusion (only one list slot can be active at a time).
  - [ ] **E2E** (Foundation Rule #13) — `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/home.spec.ts`. Same `tests/e2e/test-results/` ownership-blocker caveat as 9-1/9-2/9-3/9-4/9-5 may apply locally; CI on the story branch is the source of truth.
  - [ ] Update Dev Agent Record at the bottom of this file: list of files touched, decisions on placement, anything surprising (drift discoveries — particularly the absence of an existing series-with-gaps service function despite the spec text claim).
  - [ ] Update `_bmad-output/implementation-artifacts/sprint-status.yaml`: `9-6-series-with-gaps-indicator: ready-for-dev → in-progress` at start, `→ review` at end (only this line + `last_updated`, per CLAUDE.md rule 16).
  - [ ] Open draft PR at first commit (Foundation Rule #15). Title: `Story 9-6: Series with gaps indicator (#NN)`.

### Review Findings

_Code review run on 2026-05-03 against PR #121 (`origin/main...HEAD`, 12 files / +1891 / −140 / 2364 LOC). Three parallel reviewers (Blind Hunter, Edge Case Hunter, Acceptance Auditor) raised findings. **Acceptance Auditor reported 15/15 ACs MET, zero anti-pattern violations, zero Foundation Rule violations.** Triage: 0 decision-needed, 4 patches, 5 deferred (to GitHub Issues per Foundation Rule #11), 12 dismissed._

#### Decision-needed

_None._

#### Patch (actionable in this story)

- [x] **[Review][Patch] Anonymous + `?filter=gaps&q=foo` renders BOTH `#gaps-list` AND search results** [`src/routes/home.rs:175-204`] — Blind Hunter M1 + Edge Case Hunter corroboration. The single-active-filter precedence-clearing block at lines 197-203 (`if active_indicator_filter.is_some() { query = String::new(); }`) and the `has_filter` derivation at line 204 both key on `active_indicator_filter`, which is `None` for Anonymous (role-stripped via `role_gated_indicator_filter`). For Anonymous + `?filter=gaps&q=foo`, `gaps_filter_active = true` swaps in `#gaps-list` AND `has_filter = true` triggers the search path simultaneously — both `#gaps-list` AND `#browse-results` render. Violates AC5 / AC6 single-active-filter intent for Anonymous. Fix: change the precedence clearing AND the `has_filter` derivation to also account for `parsed_indicator.is_some()` regardless of role (or equivalently, derive `gaps_filter_active`-aware `has_filter` to exclude the search path when any indicator filter is parsed). Add a render test `home_anonymous_with_filter_gaps_and_query_does_not_render_browse_results` to lock the contract.
- [x] **[Review][Patch] `count_with_gaps` + `list_with_gaps` don't filter `titles.deleted_at IS NULL`** [`src/models/series.rs:222-291`] — Edge Case Hunter. The dashboard queries join `series` + `title_series` only, never follow to `titles`. A series whose owned-volume rows reference soft-deleted titles is reported as "filled" by the dashboard, but the per-series `series::active_count_titles` (lines 216-227) DOES filter `t.deleted_at IS NULL` — same series shows different "owned" counts in `/series/<id>` vs the dashboard `#gaps-list`. Fix: add `AND ts.title_id IN (SELECT id FROM titles WHERE deleted_at IS NULL)` (or an explicit JOIN with the same predicate) to both queries. Add a `count_with_gaps_excludes_titles_with_soft_deleted_parent` integration test to lock the symmetry.
- [x] **[Review][Patch] Whitespace-fragile assertion in `home_librarian_gaps_row_links_to_series_detail`** [`src/routes/home_indicator_tests.rs:255`] — Blind Hunter M5 + Edge Case Hunter LOW. The assertion `html.contains(">\n                                6 Missing\n")` is hard-coded to 32-space indentation. Any reformat of `templates/pages/home.html` (Tailwind reflow, IDE auto-indent, attribute reorder) breaks the test for no semantic reason. Fix: replace with a content-only assertion such as `assert!(html.contains("6 Missing"))` AND a separate assertion that the badge class palette is present in the rendered HTML — both checks together still lock the AC9 contract without coupling to whitespace.
- [x] **[Review][Patch] `position_number = 0` (or negative) silently fills slots in count/list queries** [`src/models/series.rs:222-291`] — Edge Case Hunter LOW. Schema (`migrations/20260329000000_initial_schema.sql:194`) has `position_number INT NOT NULL` with no CHECK > 0. A bad row at position 0 contributes a distinct value to `COUNT(DISTINCT position_number)`, falsely inflating the filled-position count and masking a real gap at position N. Fix: add `AND ts.position_number > 0` to both queries to lock the slot-numbering convention (positions are 1..total). Add a `count_with_gaps_position_zero_does_not_fill_slot` test seeding a position=0 row alongside positions 1..(N-1), expecting the series to still register as gappy.

#### Defer — file as GitHub Issues per Foundation Rule #11

- [x] **[Review][Defer] Cross-method invariant test `count_with_gaps == list_with_gaps(.., very_high).len()`** [`src/models/series.rs`] — Blind Hunter L2. Two distinct SQL strategies (correlated subquery for count, LEFT JOIN derived table for list) are not pinned by an invariant test asserting they return the same set. A future SQL drift could silently decouple them. File as `type:code-review-finding` for cross-method symmetry test (broader scope than 9-6 — applies to all `count_*`/`list_*` pairs in models).
- [x] **[Review][Defer] E2E librarian smoke conditional empty-DB short-circuit** [`tests/e2e/specs/journeys/home.spec.ts:298-302`] — Blind Hunter L3 + Edge Case Hunter corroboration + cross-story (already filed in 9-5's review per Dev Agent Record). The `if (tagCount === 0) return;` pattern lets the test silently pass when the seed DB has no gappy series — Foundation Rule #7 wants smoke tests to perform the journey end-to-end. Same pattern in 9-4 unshelved + 9-5 overdue. File a coordinated `type:code-review-finding` covering all three indicator E2E smoke tests; fix is a deterministic seed migration in dev/test mode.
- [x] **[Review][Defer] `Role: PartialOrd` ordering assertion sanity test** [`src/middleware/auth.rs`] — Blind Hunter L4. The `*role >= Role::Librarian` comparison in `role_gated_indicator_filter` (and many other call sites across the codebase) silently relies on the derived `PartialOrd` ordering `Anonymous < Librarian < Admin`. If `Role` is later refactored or a new variant is inserted, the ordering breaks silently with no test failing. File as `type:code-review-finding` to add a single sanity test in `auth.rs::tests` asserting the variant ordering.
- [x] **[Review][Defer] Sprint-status YAML `last_updated` one-line bloat** [`_bmad-output/implementation-artifacts/sprint-status.yaml`] — Blind Hunter L7. Each story's `last_updated` field has accumulated multi-paragraph text (5KB+ on a single YAML line). Real bugs could hide in a trailing comment line and never get reviewed. Pre-existing pattern (every story does this). File as `type:chore` to refactor the format (multi-line scalar or external `CHANGELOG.md` per story).
- [x] **[Review][Defer] Tracing-log capture in `parse_indicator_filter_unknown_bare_name_returns_none`** [`src/routes/home_indicators.rs::tests`] — Blind Hunter L9. Test asserts the return value but doesn't capture the `tracing::warn!` to verify the log actually fires for genuine typos. The whole `!is_empty() && !contains(':')` guard exists to fire WARN on typos but not legacy patterns; without log capture, a refactor could silently drop the warn. File as `type:code-review-finding` for a testing-strategy improvement (covers the whole codebase, not just this site).

#### Dismissed (12)

- **`gap_count` cast defensive nit** (Blind H1) — saturating math `.max(0)` already handles negative inputs; SQL filter `total_volume_count > 0` excludes negatives upstream. No actual bug.
- **Anonymous escape hatch UX gap** (Blind M2) — spec explicitly acknowledges "for them the escape hatch is the browser back button". Accepted design.
- **`u32` LIMIT type concern** (Blind M3) — caller hardcodes `100`; `u32::MAX` fits in MariaDB `BIGINT UNSIGNED`. Defensive only.
- **Double `cfg(test)` gating** (Blind M4) — inner `cfg(test)` is redundant when outer is gated; cosmetic, no harm.
- **`gap_count` returns `i64` saturating to `u64`** (Blind L1) — SQL `COUNT(*)` is non-negative; defense-in-depth without an actual bug path.
- **INT vs BIGINT comparison in correlated subquery** (Blind L6) — MariaDB promotes INT to BIGINT safely; informational only.
- **`pub(crate)` exposure of test factories** (Blind L5) — items are inside `#[cfg(test)] mod tests`, only compile in test builds; `pub(in crate::routes)` would be tighter but doesn't change the security/contract surface.
- **`gaps_label` and `gaps_heading` byte-identical** (Blind L8) — spec acknowledges "kept separate so future copy can diverge". Accepted design.
- **MariaDB version-pin assertion in tests** (Blind L10) — version drift is a CI/infra concern (docker-compose pinned), not a per-test concern.
- **`LIMIT 100` truncation no UI signal** (Edge LOW) — spec explicitly says LIMIT 100 (AC6) with no truncation hint; matches 9-5's same accepted v1 trade-off.
- **Race between `count_with_gaps` and `list_with_gaps` under concurrent soft-delete** (Edge LOW) — cosmetic mismatch (tag shows N, list shows N-1), recoverable on next page load. Acceptable v1.
- **`count_with_gaps` swallows DB errors at WARN not ERROR** (Edge LOW) — established pattern across 9-1/9-2/9-3/9-4/9-5; per CLAUDE.md AppError convention, accepted soft-degrade pattern.

## Dev Notes

### Source tree references

| Concern | File / location | Notes |
|---|---|---|
| Home route registration | `src/routes/mod.rs:87` | `.route("/", axum::routing::get(home::home))` — no change |
| Home handler | `src/routes/home.rs:128-440` (post-9-5) | extend the existing handler; do NOT create a parallel route |
| Home template | `templates/pages/home.html` (~348 lines post-9-5) | extends `layouts/base.html`; sections: search, filter pills, metadata error, `#what-needs-attention` (lines 83-92), `#collection-glance` (96-122), the AC6 mutually-exclusive 4-branch slot at lines 124-222 (`#unshelved-list` / `#overdue-list` / `#gaps-list` / `#recent-additions`), `#stats-by-genre` (224-249), browse toggle, `#browse-results` |
| Insertion point for `#gaps-list` | `templates/pages/home.html` lines 124-222 — convert the 3-branch `{% if %}{% else if %}{% else %}` to a 4-branch chain by inserting a new `{% else if gaps_filter_active %}` branch between overdue (line 184) and recent-additions (line 185) | AC6 4-way mutual exclusion across all four sections |
| Series schema | `migrations/20260329000000_initial_schema.sql:177-206` | `series_type ENUM('open', 'closed') NOT NULL DEFAULT 'open'`; `total_volume_count INT NULL`; `title_series.position_number INT NOT NULL`; `UNIQUE KEY uq_title_series_position (title_id, series_id, position_number)` allows two different titles to share a position (data-error edge case — the AC8 `COUNT(DISTINCT position_number)` de-dupes); no composite index on `(series_type, total_volume_count, deleted_at)` — deferred per AC8 rationale |
| Series model | `src/models/series.rs` (554 LOC) | extend with `count_with_gaps` + `list_with_gaps` (mirror `volume::count_unshelved` + `volume::list_unshelved` shape from 9-4); add `SeriesWithGap` projection struct after `SeriesModel` impl block at line 213 |
| Existing per-series gap helper | `src/services/series.rs::SeriesService::get_series_positions` (line 244) | builds the full grid for a SINGLE series — used by the series-detail page; NOT suitable for an aggregate query (would be N+1 if looped). Story 9-6 writes a new aggregate query in `models/series.rs` instead. Document the drift in Dev Agent Record. |
| Existing private compute_gap | `src/routes/series.rs:19-26` | `total - owned, saturating_sub` — used by the series-list page (also N+1 because `active_count_titles` is called in a loop); NOT extracted/reused by 9-6, which writes a single-round-trip aggregate |
| Existing series-list aggregate (N+1) | `src/routes/series.rs:96-105` | runs `active_count_titles` per series in a loop — known performance debt; do NOT extend that pattern. The 9-6 dashboard query MUST be a single round-trip (correlated subquery for count, LEFT JOIN derived table for list). |
| `SeriesType` enum | `src/models/series.rs:12-23` | `Open` / `Closed`, `to_string()` returns `"open"` / `"closed"` matching the ENUM column literals |
| `SeriesModel` struct | `src/models/series.rs:39-46` | embeds `series_type: SeriesType`, `total_volume_count: Option<i32>`. The dashboard projection `SeriesWithGap` is NARROWER (id, name, total, owned) — see AC8 rationale. |
| `IndicatorFilter` enum + parser | `src/routes/home_indicators.rs` (post-9-5) | extend with `Gaps` variant + `Some("gaps")` parser arm |
| `IndicatorTag` view-model | `src/routes/home_indicators.rs:38-53` | unchanged |
| `build_indicator_tags` helper | `src/routes/home_indicators.rs:71-107` | extend to accept `gaps_count` 3rd param |
| Per-variant role gate (AC2 NEW pattern) | `src/routes/home.rs:145-149` (post-9-5) | rewrite from `if role >= Librarian { parse } else { None }` to a `match` that allows `Gaps` for any role and gates other variants on Librarian |
| Single-active-filter precedence (AC5) | `src/routes/home.rs:150-175` (post-9-5) | already routes ANY `IndicatorFilter` variant through the same precedence path — no rewrite |
| Soft-degrade pattern | `src/routes/home.rs:237-291` (unshelved + overdue with `tracing::warn!` + 0 / Vec::new() on error) | replicate verbatim for gaps |
| Series detail route | `src/routes/series.rs:186` (`series_detail_page`) | the dashboard row link target `/series/<id>` lands here; NO changes needed in this story |
| Series gap grid component | `templates/components/series_gap_grid.html` | renders the full per-position grid for the series-detail page; NOT reused inline in the dashboard rows (AC9 rationale — the dashboard row is intentionally compact) |
| Series list page (existing template) | `templates/pages/series_list.html` (line 51 has the gap-badge color reference) | the `bg-red-*` palette mirrored in `#gaps-list` rows comes from this template's gap-count cell |
| FilterTag macro | `templates/components/filter_tag.html` (post-9-4) | unchanged — reuse via `{% call filter_tag::tag(...) %}{% endcall %}` |
| HomeTemplate struct | `src/routes/home.rs:31-111` (post-9-5) | extend with 5 new fields (gaps_filter_active, gaps_series, gaps_heading, gaps_empty_label, gaps_missing_label) |
| `make_test_home_template` (default) | `src/routes/home.rs:891+` | populate the 5 new fields with sensible defaults so existing tests keep passing |
| `make_test_home_template_with_indicators` (factory) | `src/routes/home.rs:1029-1044` | DO NOT extend with a sibling `_with_gaps` variant (LOC-budget — AC15); use post-construction field assignment in the new render tests instead |
| Slice helpers | `src/routes/home.rs::mod tests::slice_section`, `attention_section_slice` | reuse for the new render assertions |
| i18n locales | `locales/en.yml:360-368`, `locales/fr.yml:360-368` (`dashboard.attention:` block) | append `gaps_label`, `gaps_clear_aria`, `gaps_heading`, `gaps_empty` |
| Existing series i18n keys (REUSED) | `locales/en.yml:145` (`series.gap_count` = "Missing"), `locales/fr.yml:145` (= "Manquants") | reused for the per-row gap-count badge label via `gaps_missing_label` template field |
| i18n audit | `src/i18n/audit.rs::all_t_keys_have_both_locales` | enforces EN/FR mirror |
| Templates audit | `src/templates_audit.rs::no_inline_markup_in_templates` (line 44) | must stay green |
| Test pattern (DB-backed integration) | `tests/dashboard_overdue.rs` (story 9-5) | sibling file model — `#[sqlx::test(migrations = "./migrations")]`, file-local helpers |
| Test pattern (handler render, no DB) | `src/routes/home.rs::mod tests` (post-9-5) | reuse the slice + factory pattern verbatim; use post-construction field assignment for new tests (LOC budget) |
| E2E spec for `/` | `tests/e2e/specs/journeys/home.spec.ts` (5 describes post-9-5) | extend with the new "Series with gaps indicator" describe AFTER the 9-5 "Overdue loans indicator" block |
| E2E loginAs helper | `tests/e2e/helpers/auth.ts` | `loginAs(page, "librarian")` — typed union, do not pass other strings |

### Anti-patterns to avoid

- **Reusing `services::series::SeriesService::get_series_positions` for the dashboard query.** That function loops over all assignments for a SINGLE series and builds a position grid — designed for the series-detail page. Calling it inside a loop over all series would be N+1 (one query per series). The 9-6 dashboard MUST use a single round-trip aggregate query (`count_with_gaps` correlated subquery + `list_with_gaps` LEFT JOIN derived table) — see AC8.
- **Reusing the private `compute_gap` helper at `routes/series.rs:19-26` via cross-module import.** That helper takes a `&SeriesModel` (full model) — the dashboard projection `SeriesWithGap` is narrower. The aggregate query computes `gap_count` in SQL (`total - owned`) and the `SeriesWithGap::gap_count(&self) -> u64` method exposes it to the template. Don't entangle the dashboard with the per-page helper.
- **Reusing `SeriesListRow` from `routes/series.rs:41-45` as the dashboard row type.** That struct embeds `series: SeriesModel` (with description, version, type, etc.) for the series-list table page. The dashboard needs `id`, `name`, `total`, `owned` only — narrower projection avoids loading unused columns and keeps the row template simple.
- **Inlining the SeriesGapGrid component (`templates/components/series_gap_grid.html`) in the dashboard rows.** That component loops per-position and is designed for the series-detail page (UX-DR16). The dashboard row is intentionally compact (name + ratio + badge) — the row link to `/series/{id}` gives the user the full grid one click away. Inline preview would be visually heavy AND template-coupling-heavy AND would slow the dashboard render for a marginal UX gain.
- **Applying the Anonymous-blanket gate to `IndicatorFilter::Gaps`.** The 9-4/9-5 pattern (`if role >= Librarian { parse } else { None }`) is wrong for Gaps — series browsing IS anonymous-allowed (FR65, FR95). The handler MUST allow `Some(IndicatorFilter::Gaps)` to flow through for Anonymous so the section swap happens. Anonymous never sees the TAG (because `gaps_count` is forced to 0 for them and `#what-needs-attention` hides on empty `indicator_tags`), but they DO see the `#gaps-list` when they navigate to `/?filter=gaps`. This is the load-bearing AC2 deviation.
- **Using `COUNT(*)` instead of `COUNT(DISTINCT position_number)` in the gap-detection query.** Two different titles can be assigned to the same position (the UNIQUE constraint is `(title_id, series_id, position_number)` — different `title_id` allows it); also BD omnibus titles have multiple rows for the same `title_id` at distinct positions. `COUNT(*)` would mis-count both edge cases. `COUNT(DISTINCT position_number)` is the precise count of FILLED slots — see AC7 + AC12a's `count_with_gaps_distinct_positions` and `count_with_gaps_omnibus_fills_each_position` tests for the regression guards.
- **Counting open series as having gaps.** Open series have `total_volume_count = NULL` (or unset) — there's no defined "completeness" (FR54). The query's `WHERE series_type = 'closed' AND total_volume_count IS NOT NULL AND total_volume_count > 0` filter excludes them by design. Locked by `count_with_gaps_open_series_never_counted` test (AC12a).
- **`sqlx::query!` macro.** Forces `cargo sqlx prepare` regeneration in the PR. Use dynamic `query` / `query_as` (project convention; see Story 9-2/9-3/9-4/9-5 anti-pattern note).
- **Calling `t!()` from inside the Askama template.** Pre-translate in the handler, pass as `String` fields. Project convention; canonical example: `src/routes/home.rs:303-320` (post-9-5).
- **Inline `style="..."` for the badge coloring.** UX-DR24 mandates Tailwind utility classes resolving to `@theme` tokens. The pattern `bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-300` from `series_list.html:51` and `loans.html` is the model — copy verbatim.
- **HTMX swap target invention.** AC8 of story 9-4 explicitly deferred HTMX boost; 9-5 + 9-6 inherit the plain-`<a href>` decision verbatim. Plain links work without JS; the FilterTag macro emits them; the handler renders the new state from the URL on the next request.
- **Running `list_with_gaps` when `gaps_filter_active = false`.** Handler must guard: `if gaps_filter_active { SeriesModel::list_with_gaps(...) } else { Vec::new() }`. Wasteful otherwise — the JOIN doesn't need to fire on every home page load.
- **Pushing past 2000 LOC in `src/routes/home.rs`.** Foundation Rule #12. AC15 is non-negotiable — Task 9 has the explicit verification step + the fallback extraction plan.
- **Adding a sibling factory `make_test_home_template_with_gaps_indicator` in `home.rs::tests`.** Would add ~15 LOC for marginal value over post-construction field assignment (the pattern 9-5 settled on after its "Task 9 LOC trim"). AC15 explicitly says NO new factory variant; use post-construction.

### Anonymous-allowed-filter asymmetry (load-bearing AC2 deviation)

The 9-4 and 9-5 indicator stories share a uniform anonymous-no-leak pattern: parse the filter only if `role >= Librarian`, force counts to 0 for Anonymous, never emit the tag, never run the list query. Story 9-6 BREAKS this uniformity for `IndicatorFilter::Gaps` because series browsing is deliberately anonymous-permitted (FR65 + FR95) — the entire `/series` route requires no auth, the series-detail page is anonymous-readable. A consistent dashboard UX therefore allows Anonymous to use `/?filter=gaps` to see the list of incomplete series.

The split:

| Surface | Anonymous | Librarian/Admin |
|---|---|---|
| `#filter-tag-gaps` in `#what-needs-attention` (on `/`) | Never rendered (section hides) | Rendered when count > 0 OR filter active |
| `count_with_gaps` query | NEVER ISSUED (force count = 0 — no DB load) | Issued unconditionally |
| `gaps_filter_active` slot boolean | Driven by raw parser result (anonymous CAN trigger) | Driven by raw parser result |
| `#gaps-list` section (on `/?filter=gaps`) | Rendered (anonymous-allowed) | Rendered |
| `list_with_gaps` query | Issued IFF `gaps_filter_active` | Issued IFF `gaps_filter_active` |
| `#recent-additions` swapped out by `#gaps-list` | Yes (mutual exclusion holds for anonymous too) | Yes |

**Implementation knob:** the handler computes `active_indicator_filter` (used by `build_indicator_tags`) WITH the role gate, and `gaps_filter_active` WITHOUT it. See Task 5's per-variant role-gate rewrite for the canonical shape. The render test `home_anonymous_with_filter_gaps_renders_gaps_list_but_no_tag` is the regression guard.

**Why no E2E for the threshold-change-style "what if Anonymous tries unshelved" cross-check?** Stories 9-4 and 9-5 already cover that — their anonymous E2E tests assert `#filter-tag-unshelved` / `#filter-tag-overdue` are not rendered AND `#unshelved-list` / `#overdue-list` are not rendered for `/?filter=unshelved` / `/?filter=overdue`. Story 9-6 inherits those guarantees via the per-variant role gate (Unshelved + Overdue stay Librarian-gated). No new E2E for the unchanged paths.

### Architecture compliance

- **Error handling:** Any DB failure in `count_with_gaps` / `list_with_gaps` returns `AppError::Database` via `?`; the handler soft-degrades with `tracing::warn!` + `0` (count) or `Vec::new()` (list), per the established 9-1/9-2/9-3/9-4/9-5 pattern. The home page MUST NOT 500 because the indicator query had a hiccup.
- **Logging:** `tracing::warn!` at handler level on the soft-degrade paths; `tracing::debug!` only inside model functions if needed. The single-active-filter `tracing::warn!` at `home.rs:151-156` (post-9-5) ALREADY covers the `?filter=gaps` + `?q=foo` collision case — no new log statement needed there. Indicator-related state changes are not interesting at info-level.
- **DB query discipline:** Every SELECT/JOIN of entity tables (`series`, `title_series`) MUST include `deleted_at IS NULL`. The `count_with_gaps` query filters `s.deleted_at IS NULL` on the outer query AND `ts.deleted_at IS NULL` inside the correlated subquery. The `list_with_gaps` query filters `s.deleted_at IS NULL` on the outer query AND `WHERE deleted_at IS NULL` inside the derived `filled` subquery. Soft-deleted assignments do not "fill" gaps (correct — see AC8 + the `count_with_gaps_soft_deleted_assignments_dont_fill_gaps` test). 
- **HTMX coexistence:** the `#gaps-list` (or `#unshelved-list` or `#overdue-list` or `#recent-additions`) sections sit OUTSIDE `#browse-results` (HTMX swap target) — same invariant as 9-1/9-2/9-3/9-4/9-5. Plain `<a href>` navigation does not interact with the existing HTMX search-fragment branch.
- **CSP middleware:** Already wraps the handler outermost. No work needed.
- **Pool access:** The handler already has `state.pool: DbPool`. Do not introduce a new connection.
- **One-branch-one-story (Foundation Rule #14):** Verify `git status` clean + on `main` + `git pull --ff-only` before starting; cut `story/9-6-series-with-gaps-indicator`. Open a draft PR (Rule #15) at the first commit (the model methods + integration tests).
- **Source-file-size limit (Foundation Rule #12):** `src/routes/home.rs` is at **1967 LOC** post-9-5 close. AC15 mandates trimming + post-construction-field-assignment in the new render tests; if that's not enough, extract the indicator-test factories into `home_indicators.rs::tests` per the Task 9 fallback. Net target: home.rs ≤ 2000 LOC at story close. The dashboard_gaps.rs integration test file is sized at ~250-300 LOC; well within bounds.

### Library / framework requirements

- **Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + Tailwind CSS v4 + HTMX 2.0** — no version changes, no new dependencies.
- **rust_i18n** — already wired. Pre-translate the four new keys (`gaps_label`, `gaps_clear_aria`, `gaps_heading`, `gaps_empty`) plus the reused `series.gap_count` in the handler via `rust_i18n::t!(…).to_string()`. No `count =` interpolation needed (the badge displays raw integer + label separately).
- **MariaDB `COUNT(DISTINCT col)` semantics:** `COUNT(DISTINCT position_number)` returns an integer count of unique non-null position values. NULL positions are excluded by default (and `position_number INT NOT NULL` means none exist anyway). Matches the per-AC8 contract.
- **MariaDB `LEFT JOIN` with derived table:** the `list_with_gaps` query uses `LEFT JOIN (SELECT … GROUP BY …) filled ON filled.series_id = s.id`. MariaDB optimizes this efficiently for the personal-library scale (< 500 series, < 5000 assignments). For larger collections, the derived table could be materialized via a subquery hint, but v1 doesn't need it.
- **Askama macros / method calls** — the FilterTag component is unchanged from 9-4; reuse via `{% call filter_tag::tag(...) %}{% endcall %}`. The `row.gap_count()` method invocation in the row template is supported by Askama 0.15 (verified via `templates_audit.rs` which doesn't restrict it). If Askama renders `row.gap_count` as a field-access by mistake, fall back to pre-computing `gap_count` in the handler and passing as a `Vec<DashboardSeriesRow>` view-model with `gap_count: u64` already populated.

### File structure requirements

| File | Action | Rough size |
|---|---|---|
| `src/routes/home.rs` | **edit** | +35-50 LOC (handler block + per-variant role gate rewrite + 8 render tests + small `fake_series_with_gap` helper); LOC TARGET ≤ 2000 (currently 1967) — AC15 trim + post-construction field assignment required; fallback extraction available |
| `src/routes/home_indicators.rs` | **edit** | +25-35 LOC (1 new enum variant + 1 new parser arm + 1 new helper if-block + 4 new helper unit tests; UPDATE 8 existing tests with new arg) |
| `src/models/series.rs` | **edit** | +60-80 LOC (`SeriesWithGap` struct + impl + `count_with_gaps` + `list_with_gaps` async fns) |
| `templates/pages/home.html` | **edit** | +30-35 LOC (the `{% else if gaps_filter_active %}` branch + the `<section id="gaps-list">` body) |
| `locales/en.yml` | **edit** | +4 lines under `dashboard.attention:` |
| `locales/fr.yml` | **edit** | +4 lines under `dashboard.attention:` |
| `tests/dashboard_gaps.rs` | **create** | ~250-300 LOC (11 `#[sqlx::test]` cases + helpers including `insert_series` + `insert_title_series_assignment` + `soft_delete_series` + `soft_delete_title_series_assignment`) |
| `tests/e2e/specs/journeys/home.spec.ts` | **edit** | +50-70 LOC (1 new `test.describe` block, 2 tests) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **edit** | only the `9-6-...` line + `last_updated` (per CLAUDE.md rule 16) |
| `_bmad-output/implementation-artifacts/9-6-series-with-gaps-indicator.md` | **edit** | this file — Status, Tasks checked, Dev Agent Record on completion |

### Testing requirements

- **Coverage target:** every AC has at least one test (unit OR E2E). AC14 (CSP) is covered by `templates_audit.rs::no_inline_markup_in_templates` (existing — must stay green). AC15 (LOC) is verified by the `wc -l` step in Task 9 + 10.
- **AC2 anonymous-allowed asymmetry** is the LOAD-BEARING new contract this story introduces. The render test `home_anonymous_with_filter_gaps_renders_gaps_list_but_no_tag` is the primary regression guard at the unit level; the E2E Test 1 is the secondary integration-level guard. Together they prove both halves: (a) Anonymous + `/?filter=gaps` → `#gaps-list` IS rendered, (b) Anonymous + `/?filter=gaps` → `#filter-tag-gaps` is NOT rendered. A future refactor that re-applies the blanket Anonymous gate would break BOTH guards loudly.
- **AC4 enum + parser update:** the `parse_indicator_filter_gaps_recognized` test is the primary positive guard. The `parse_indicator_filter_unknown_bare_name_returns_none` test (with the `"gaps"` line removed and `"recent-cataloged"` added) keeps the warn-and-ignore branch covered. Without the test edit, CI would flag a regression on the `"gaps is reserved for story 9-6"` assertion AS SOON AS Task 2 lands the parser change — order matters in the same commit.
- **AC6 4-way mutual exclusion** of `#recent-additions` vs `#unshelved-list` vs `#overdue-list` vs `#gaps-list` is the load-bearing layout invariant. The render test `home_librarian_gaps_filter_active_renders_gaps_list_not_unshelved_list_nor_overdue_list_nor_recent_additions` is the regression guard.
- **AC7 open-vs-closed + DISTINCT semantics** is locked by `count_with_gaps_open_series_never_counted`, `count_with_gaps_closed_with_null_total_excluded`, `count_with_gaps_distinct_positions`, and `count_with_gaps_omnibus_fills_each_position`. Without these, a future "simplify the SQL" refactor that drops the type filter or replaces DISTINCT with COUNT(*) would slip past CI silently.
- **AC8 soft-delete-of-assignment unfills the position** is locked by `count_with_gaps_soft_deleted_assignments_dont_fill_gaps`. Without it, a future query change that joins on `title_series` without the `deleted_at IS NULL` filter would mis-count.
- **AC10 emit order** — `build_indicator_tags_emits_unshelved_then_overdue_then_gaps_when_all_present` is the regression guard. Without it, a future "alphabetize the if-blocks" refactor would break the priority ordering with no test failing.
- **E2E** keeps to 1 anonymous + 1 librarian smoke test for parsimony. The anonymous test is the primary new contract (NO empty-DB short-circuit — the test explicitly asserts the dual contract and doesn't depend on seeded data). The librarian test follows the 9-5 conditional empty-DB short-circuit pattern.

### Project structure notes

This story extends the patterns 9-4 and 9-5 established. Three intentional design decisions worth flagging:

1. **`SeriesWithGap` is a new narrow projection (not a reuse of `SeriesListRow` or `SeriesModel`).** Story 9-5 reused `LoanWithDetails` because that struct already had every field the row template needed. Here the situation is reversed: `SeriesListRow` (in `routes/series.rs:41-45`) embeds the full `SeriesModel` (description, version, type, etc.) for the series-list TABLE page; `SeriesModel` has even more fields (the `description: Option<String>`, `version: i32`, etc.). The dashboard row needs only `id`, `name`, `total`, `owned` — a focused projection struct keeps the SQL projection narrow (no unused columns over the wire) AND keeps the row template simple (no field-access bloat). `gap_count()` is exposed as a `&self` method for the template.

2. **Per-variant role gate is a NEW shape (AC2 asymmetry).** This is the first time the indicator subsystem distinguishes role-gating per variant. The naive uniform pattern from 9-4/9-5 (gate the whole `parse_indicator_filter` result) doesn't work for series-browsing-allowed filters. Future indicator stories (9.7's recent-cataloged + recent-returns) revert to the Librarian-only pattern — but the per-variant gate is now a proven shape if a future indicator needs the Anonymous-allowed asymmetry too. The handler's per-variant `match` is more verbose than the original two-line `if`, but it makes the per-variant policy explicit; document the rationale in the inline comment so a future "simplify back to one gate" refactor is caught at code review.

3. **Aggregate gap-count query is NEW (despite the spec text claiming a reusable function).** The epics.md AC says "reuses the existing series-with-gaps service function from `src/services/series.rs` (extracted in Epic 5)". Audit reveals NO such aggregate function exists — only `services::series::SeriesService::get_series_positions` (per-single-series, used by the series-detail page). The dashboard needs an aggregate query to avoid N+1, so Task 1 writes a new model-layer pair (`count_with_gaps` + `list_with_gaps`). Document this drift discovery prominently in the Dev Agent Record at story close — future code-review readers should not be surprised by the absence of the "extracted" function. If at code-review time someone asks "shouldn't this reuse `get_series_positions`?", the answer is: that's a per-series helper for grid display; the dashboard needs an aggregate, and the existing series-list page itself has known N+1 debt that should be migrated to the new aggregate as a follow-up `type:change-request` GH Issue (NOT in scope for this story).

4. **HomeTemplate field-count reaches ~73-80 fields.** Pre-9-4 ~55, 9-4 ~61, 9-5 ~68, 9-6 +5 = ~73. By 9-7 close (5+ × 2 indicators) the count may hit 80+. Worth flagging again (9-5 also flagged this) — a future `DashboardSlots` substruct would tame the field count, but refactor-during-feature is anti-pattern; if 9-7 push starts to hurt, file `type:change-request` then.

5. **Series-list page N+1 cleanup is OUT OF SCOPE.** The existing `series_list_page` runs `active_count_titles` + `compute_gap` per series in a loop (`routes/series.rs:96-105`) — that's N+1. After 9-6 ships, a follow-up PR could migrate that page to use the new `list_with_gaps` aggregate (or a sibling `list_all_with_owned_count` if the filter doesn't fit). File as `type:change-request` GH Issue at story close; do NOT include in this story (focus + clean diff per the Foundation Rules).

The 9-4 FilterTag macro + 9-5 indicator-section-swap precedents stay the model for this story — no template-component edits, only data-side wiring + a new HTML branch in `home.html`.

### Schema reality check (drift discoveries from spec text)

Drift discoveries this spec has factored in:

- **"Reuses the existing series-with-gaps service function from `src/services/series.rs` (extracted in Epic 5)"** — INCORRECT. No such aggregate function exists. Only per-single-series helpers. AC8 writes a new aggregate query. Document in Dev Agent Record.
- **"Each row is a SeriesCard (existing component from Epic 5)"** — INCORRECT. No `SeriesCard` template component exists in `templates/components/`. The existing `series_list.html` table inlines its row markup. The 9-6 dashboard rows are inlined too (compact name + ratio + badge), matching the established 9-4/9-5 pattern of inlining row markup directly inside `#unshelved-list` / `#overdue-list`.
- **"showing the gap count and a SeriesGapGrid preview (UX-DR16)"** — the SeriesGapGrid component (`templates/components/series_gap_grid.html`) DOES exist but is per-position, designed for the series-detail page. Inlining it for each dashboard row would be heavy (template loops per-position) for marginal UX gain. The 9-6 dashboard row links to `/series/{id}` where the user gets the full grid one click away.
- **`series.series_type ENUM('open', 'closed')`** — the ENUM is stored as the string literal `"open"` or `"closed"` in MariaDB. The aggregate query matches `series_type = 'closed'` directly without enum-cast. The Rust `SeriesType` enum's `to_string()` returns the same lowercase literals.
- **`series.total_volume_count INT NULL`** — nullable; closed series can have NULL or 0 total (data-integrity edge case). The query's `IS NOT NULL AND > 0` guard excludes both — a closed series without a positive total doesn't have a defined "completeness".
- **`title_series.position_number INT NOT NULL`** — never NULL; `COUNT(DISTINCT position_number)` is well-defined.
- **`UNIQUE KEY uq_title_series_position (title_id, series_id, position_number)`** — ONE title can claim a position once per series, but TWO DIFFERENT titles can theoretically claim the same position (data-error possible). `COUNT(DISTINCT position_number)` correctly de-dupes this case (verified by `count_with_gaps_distinct_positions` test).

If a fresh schema drift is discovered during dev, document inline in the test helper AND in the Dev Agent Record's "drift discoveries" section.

## References

- [Story 9.6 spec — `_bmad-output/planning-artifacts/epics.md` lines 1298–1315](../planning-artifacts/epics.md)
- [Epic 9 scope note + indicator delivery split philosophy — `epics.md` lines 1200–1206](../planning-artifacts/epics.md)
- [Story 9.7 visual order definition (Unshelved → Overdue → Series with gaps → Recent cataloged → Recent returns) — `epics.md` line 1330](../planning-artifacts/epics.md)
- [PRD FR58 (actionable indicators), FR54 (open/closed series), FR65 (anonymous browsing), FR95 (anonymous series list with gap count) — `_bmad-output/planning-artifacts/prd.md`](../planning-artifacts/prd.md)
- [UX-DR4 (FilterTag dual state, zero-count rule) — `_bmad-output/planning-artifacts/ux-design-specification.md`](../planning-artifacts/ux-design-specification.md)
- [UX-DR16 (SeriesGapGrid for the series-detail page; NOT inlined in dashboard rows) — `ux-design-specification.md`](../planning-artifacts/ux-design-specification.md)
- [Story 9-5 spec (canonical patterns: per-indicator section swap, soft-degrade + warn pattern, post-construction-field-assignment for new render tests, LOC trim playbook, mutual-exclusion 3-branch chain → extends to 4-branch in 9-6) — `9-5-overdue-loans-indicator.md`](./9-5-overdue-loans-indicator.md)
- [Story 9-4 spec (canonical patterns: FilterTag macro, IndicatorFilter enum + parser, build_indicator_tags helper, code-review escape-hatch patch) — `9-4-filtertag-and-unshelved-indicator.md`](./9-4-filtertag-and-unshelved-indicator.md)
- [Story 9-3 spec (canonical patterns: view-model struct in `home.rs`, render test factory extension, slice helpers) — `9-3-dashboard-stats-by-genre.md`](./9-3-dashboard-stats-by-genre.md)
- [Story 9-1 spec (canonical patterns: handler-side i18n, single round-trip, soft-degrade on DB error) — `9-1-dashboard-global-stats-card.md`](./9-1-dashboard-global-stats-card.md)
- [Story 5-3/5-4 (series CRUD + gap detection foundation — the per-single-series `get_series_positions` and `compute_gap` helpers; NO aggregate gap-count function was extracted) — `5-3-series-crud-and-listing.md`, `5-4-title-series-assignment-and-gap-detection.md`](./5-3-series-crud-and-listing.md)
- [`CLAUDE.md` — Foundation Rules (#1 DRY, #7 E2E smoke per epic, #11 GitHub Issues, #12 ≤ 2000 LOC, #13 local testing, #14 one-branch-one-story, #15 draft PR, #16 sprint-status ownership, #18 CI gating)](../../CLAUDE.md)
- [Series schema — `migrations/20260329000000_initial_schema.sql:177-206` (`series` + `title_series` tables; `ENUM('open', 'closed')`; `total_volume_count INT NULL`; `position_number INT NOT NULL`; UNIQUE constraint allows same position with different titles)](../../migrations/20260329000000_initial_schema.sql)
- [Series model — `src/models/series.rs` (`SeriesModel`, `SeriesType` enum, `TitleSeriesModel` — extend with `count_with_gaps` + `list_with_gaps` + `SeriesWithGap` projection)](../../src/models/series.rs)
- [Series service (existing per-series helper) — `src/services/series.rs::SeriesService::get_series_positions` (DO NOT loop for the dashboard — write a new aggregate)](../../src/services/series.rs)
- [Series list page (existing N+1 pattern — DO NOT extend) — `src/routes/series.rs:96-105` (loops `active_count_titles` per series; gap-badge color reference at `series_list.html:51`)](../../src/routes/series.rs)
- [Series detail page (the dashboard row link target — `/series/{id}`, no changes in this story) — `src/routes/series.rs:186` (`series_detail_page`)](../../src/routes/series.rs)
- [Indicator subsystem — `src/routes/home_indicators.rs` (extend `IndicatorFilter` enum + `parse_indicator_filter` + `build_indicator_tags`)](../../src/routes/home_indicators.rs)
- [Home handler — `src/routes/home.rs:128-440` (post-9-5; extend with the per-variant role gate + gaps data fetching + HomeTemplate fields)](../../src/routes/home.rs)
- [Home template (existing 3-branch slot — convert to 4-branch) — `templates/pages/home.html` lines 124-222](../../templates/pages/home.html)
- [FilterTag macro precedent — `templates/components/filter_tag.html` (story 9-4, unchanged)](../../templates/components/filter_tag.html)
- [SeriesGapGrid component (per-position grid for series-detail page; NOT inlined in dashboard rows) — `templates/components/series_gap_grid.html`](../../templates/components/series_gap_grid.html)
- [Dashboard integration test pattern — `tests/dashboard_overdue.rs` (story 9-5; sibling shape for `tests/dashboard_gaps.rs`)](../../tests/dashboard_overdue.rs)

## Dev Agent Record

### Agent Model Used

`claude-opus-4-7` (1M context).

### Debug Log References

- **2026-05-03 — Task 5/6 LOC overshoot.** After Tasks 5 + 6 landed
  (handler wiring + 5 new HomeTemplate fields + the `#gaps-list`
  template branch), `wc -l src/routes/home.rs` returned **2029 LOC** —
  29 over the 2000 ceiling, *before* Task 7's 8 render tests added
  another ~140 LOC. Triggered AC15 fallback extraction earlier than
  planned.
- **2026-05-03 — Task 7/9 extraction.** After Task 7's 8 render tests +
  `fake_series_with_gap` helper landed, `wc -l src/routes/home.rs`
  returned **2172 LOC** (172 over). AC15's fallback path executed:
  created `src/routes/home_indicator_tests.rs` and moved the 6
  9-5 + 8 9-6 indicator render tests + helper into it. Cross-module
  test imports required `pub(crate) mod tests` on `home::tests` plus
  `pub(crate)` visibility on 4 fakes (`fake_indicator_tag`,
  `fake_unshelved_row`, `fake_loan_with_details`,
  `make_test_home_template_with_indicators`), 1 factory
  (`make_test_home_template_with_counts`), and 2 slice helpers
  (`slice_section`, `attention_section_slice`). Net: home.rs back to
  **1930 LOC** (70 below ceiling).
- **2026-05-03 — Askama Template trait import.** First compile of
  `home_indicator_tests.rs` failed with "no method named `render` found
  for struct `home::HomeTemplate`" — Askama's `Template` trait must be
  in scope via `use askama::Template;`. Added at the top of the new
  module. (In `home::tests`, `use super::*;` already brought the trait
  in via the parent's `use askama::Template;`.)

### Completion Notes List

- ✅ All 10 tasks complete with all 15 ACs satisfied.
- ✅ `home.rs` LOC: **1930** (under the 2000-LOC Foundation Rule #12
  ceiling). Net change vs pre-9-6: −37 LOC (Task 9 extraction freed
  243 LOC of room; 9-6 substance + tests added 206 LOC).
- ✅ `home_indicators.rs`: 429 LOC (was 388 pre-9-6; +41 LOC from the
  new variant + parser arm + helper if-block + 4 new helper unit tests).
- ✅ `home_indicator_tests.rs`: NEW file, 265 LOC (the 9-5 6 + 9-6 8
  render tests + `fake_series_with_gap` helper + Askama Template
  import). Pure code move for the 9-5 tests; new code for the 9-6 tests.
- ✅ Lib tests: **695 passing** (was 687 pre-9-6; +4 new helper tests
  in `home_indicators::tests` + +4 new test names due to the gaps
  variant matrix; the 8 9-6 render tests live in
  `home_indicator_tests::tests`).
- ✅ Integration tests: 109 passing across all `tests/*.rs` binaries —
  including the **13 new `dashboard_gaps.rs` cases** (10 count + 3 list,
  per AC12a/AC12b — the spec said 11; 13 reflects the actual coverage
  after writing tighter cases).
- ✅ E2E: 1 new `test.describe("Home page — Series with gaps
  indicator", ...)` block in `home.spec.ts` with 2 tests
  (anonymous-allowed asymmetry + librarian smoke with conditional
  empty-DB short-circuit).
- ✅ Clippy: clean with `-D warnings` across `--all-targets`.
- ✅ Templates audit (`no_inline_markup_in_templates`, CSP allowlist,
  CSRF coverage): all green.
- ✅ TypeScript check on `tests/e2e/`: clean.
- ✅ CI flake gate: no `waitForTimeout` calls added.

**Drift discoveries (recorded for future stories):**
- **NO existing aggregate gap-detection function** in `services/series.rs`,
  contrary to the epics.md AC text. Only per-single-series helpers
  (`get_series_positions` builds the grid for the detail page;
  `compute_gap` is private to `routes/series.rs` and computes per-series
  `total - owned`). Story 9-6 wrote new aggregate model fns
  (`count_with_gaps` + `list_with_gaps`) directly in `models/series.rs`
  with single-round-trip queries (correlated subquery for count, LEFT
  JOIN derived table for list). Documented prominently in spec.
- **`SeriesCard` template component does NOT exist** — spec text said
  "each row is a SeriesCard (existing component from Epic 5)" but
  `templates/components/` has no such file. Existing series-list page
  inlines its row markup. The 9-6 dashboard rows follow the same
  inline-markup pattern as 9-4/9-5.
- **No new `gap_count` clippy/wc warnings.** The `SeriesWithGap::
  gap_count(&self) -> u64` method is a `&self` accessor that Askama
  invokes successfully via `{{ row.gap_count() }}` — verified at
  template compile time.
- **Per-variant role gate works as designed.** The handler's `match`
  on `parsed_indicator` cleanly separates Gaps (anonymous-allowed)
  from Unshelved + Overdue (Librarian-only). Two derived booleans
  drive different surfaces: `active_indicator_filter` (role-gated,
  flows to `build_indicator_tags` so anonymous never sees a tag) and
  `gaps_filter_active` (raw, drives the section swap for any role).
  The render test
  `home_anonymous_with_filter_gaps_renders_gaps_list_but_no_tag` and
  the E2E "anonymous: tag never rendered, BUT /?filter=gaps shows the
  list" test together lock the asymmetric contract end-to-end.

**Key design decisions (mostly inherited from 9-4/9-5 + spec):**
1. `SeriesWithGap` NEW projection struct (NOT `SeriesListRow` reuse —
  that struct embeds the full `SeriesModel`; dashboard needs only
  id+name+total+owned). Mirror of the `UnshelvedVolumeRow` decision in
  9-4 (NEW struct vs `SearchResult` reuse).
2. Aggregate SQL with `COUNT(DISTINCT position_number)` to handle BD
  omnibus + same-position-different-titles edge cases. Pinned by 2
  load-bearing tests (`count_with_gaps_distinct_positions` +
  `count_with_gaps_omnibus_fills_each_position`).
3. Strict `>` boundary on `total > distinct_filled` — only series
  where filled-count is STRICTLY less than total are gappy. Boundary
  test `count_with_gaps_closed_full_not_counted` (5/5 = no gap) +
  `count_with_gaps_closed_partial_counted` (3/5 = gap) lock it.
4. Per-variant role gate (NEW pattern — first asymmetric role-gating
  in the indicator subsystem). Documented inline in `home.rs:145-149`
  comment + spec's "Anonymous-allowed-filter asymmetry" section.
5. Row link target is `/series/{id}` (NOT `/title/<id>`) — the
  destination is the series-detail page where the user sees the full
  SeriesGapGrid (UX-DR16) one click away. NOT inlining the grid in
  dashboard rows (heavy + couples templates).
6. 4-way mutual exclusion (`#recent-additions` / `#unshelved-list` /
  `#overdue-list` / `#gaps-list`) implemented as a single `{% if %}
  {% else if %}{% else if %}{% else %}` chain in `home.html`.
7. Test factory: NO new sibling `make_test_home_template_with_gaps`
  — used post-construction field assignment to keep LOC budget
  headroom (matches 9-5 LOC trim pattern).
8. AC15 LOC budget: extraction landed mid-flight as the proper fallback
  (vs trimming doc-comments) because Tasks 5+6 alone pushed home.rs
  over 2000. The extraction pattern (`pub(crate) mod tests` + sibling
  test file importing helpers) is now a precedent for stories 9-7+.

### File List

**Created:**
- `src/routes/home_indicator_tests.rs` (Task 9 LOC extraction — holds
  the 6 9-5 + 8 9-6 indicator render tests + `fake_series_with_gap`
  helper + Askama Template import; 265 LOC)
- `tests/dashboard_gaps.rs` (Task 1 — 13 `#[sqlx::test]` cases + 5
  helpers including `insert_series` + `insert_title_series_assignment`
  + soft-delete helpers; 326 LOC)

**Modified:**
- `src/models/series.rs` (Task 1: +`SeriesWithGap` projection struct +
  `count_with_gaps` + `list_with_gaps` async fns)
- `src/routes/home_indicators.rs` (Task 2 + Task 3: +`Gaps` enum
  variant + parser arm; +`gaps_count` 3rd param on
  `build_indicator_tags`; +4 new gaps helper unit tests; updated 8
  existing tests with new arg; updated parser tests for the
  `gaps`/`recent-cataloged` reservation rotation)
- `src/routes/home.rs` (Task 5: per-variant role gate rewrite at
  lines 145-167; +gaps fetching block; HomeTemplate +5 fields; test
  factory updated. Task 7: 8 new render tests added then moved out by
  Task 9. Task 9: extraction — `pub(crate) mod tests` + 7 helper
  visibility lifts; 14 indicator render tests deleted from this file)
- `src/routes/mod.rs` (Task 9: +`#[cfg(test)] mod home_indicator_tests;`)
- `templates/pages/home.html` (Task 6: 3-branch chain → 4-branch
  chain; +`{% else if gaps_filter_active %}` branch with `#gaps-list`
  section)
- `locales/en.yml` (Task 4: +4 keys under `dashboard.attention`)
- `locales/fr.yml` (Task 4: +4 keys under `dashboard.attention`)
- `tests/e2e/specs/journeys/home.spec.ts` (Task 8: +1 describe block,
  2 tests)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (Rule 16:
  9-6 line backlog → ready-for-dev → in-progress → review)
- `_bmad-output/implementation-artifacts/9-6-series-with-gaps-indicator.md`
  (Status, Tasks checked, Dev Agent Record)

### Change Log

- **2026-05-03** — Story 9-6 dev-story: 6 commits across the 10 tasks.
  - Commit 1 (Task 1): `count_with_gaps` + `list_with_gaps` model
    methods + `SeriesWithGap` projection + 13 `#[sqlx::test]` cases in
    `tests/dashboard_gaps.rs`. Spec drift discovery documented.
  - Commit 2 (Tasks 2 + 3 + 4): `IndicatorFilter::Gaps` variant +
    parser arm + extended `build_indicator_tags` (gaps_count param) +
    4 new helper unit tests + EN/FR i18n keys.
  - Commit 3 (Tasks 5 + 6): home handler per-variant role gate (AC2
    asymmetry) + gaps wiring + soft-degrade pattern; HomeTemplate +5
    fields; 4-branch mutual-exclusion chain in `home.html` with
    `#gaps-list` section.
  - Commit 4 (Tasks 7 + 9): 8 new render tests added then extracted to
    `src/routes/home_indicator_tests.rs` + 6 9-5 indicator render
    tests moved alongside (LOC budget — Foundation Rule #12). Cross-
    module test imports required `pub(crate)` lifts on factory + 4
    fakes + 2 slice helpers + `mod tests` itself.
  - Commit 5 (Task 8): E2E `test.describe("Home page — Series with
    gaps indicator")` block with 2 tests (anonymous AC2 asymmetry +
    librarian smoke).
  - Commit 6 (Task 10 — this commit): Dev Agent Record + Status
    `in-progress` → `review` + sprint-status flip.
