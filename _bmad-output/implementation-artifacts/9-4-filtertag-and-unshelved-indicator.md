# Story 9.4: FilterTag component + first actionable indicator (unshelved volumes)

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a librarian,
I want to see the count of unshelved volumes as a clickable tag on the home page,
so that I can immediately jump to the list of volumes that need shelving.

## Acceptance Criteria

1. **AC1 — "What needs attention" section presence (librarian/admin only).** Given the home page (`/`), when it renders for a Librarian or Admin role, then a "What needs attention" / "À traiter" section is visible BETWEEN `#collection-glance` (story 9-1) and `#recent-additions` (story 9-2) — i.e., directly under the search field + filter pills, above the glance card. **Wait — placement note:** the section sits ABOVE `#collection-glance` (top of the dashboard), because actionable indicators are the highest-priority surface for a librarian and should not be scrolled past. The dev may deviate ONLY with explicit justification documented in the Dev Agent Record. The section is OUTSIDE the `#browse-results` HTMX swap target.
2. **AC2 — Anonymous never sees the section.** Given the home page rendered for an Anonymous role, when it renders, then `id="what-needs-attention"` is NOT present in the HTML, and `id="filter-tag-unshelved"` is NOT present anywhere on the page. The handler MUST NOT issue the unshelved count query for Anonymous (no role-gated leak; no DB load for surfaces the user can't see). Anonymous users crafting `/?filter=unshelved` get the default home (filter is silently ignored — no 400, no leak).
3. **AC3 — FilterTag component shape.** A new Askama macro at `templates/components/filter_tag.html` named `tag(label, count, filter_name, is_active)` is created. Behavior:
    - **`count == 0`:** the macro emits NOTHING (UX-DR4 zero-count rule). The `#what-needs-attention` section likewise hides when ALL tags are zero (handler filters zero-count tags before passing to the template).
    - **`is_active == false` (default state):** renders as a pill `<a href="/?filter={{ filter_name }}" id="filter-tag-{{ filter_name }}" class="...stone-100..." aria-label="{{ label }}: {{ count }}"><span>{{ label }}</span><span class="font-semibold tabular-nums">{{ count }}</span></a>`. Tailwind classes only — no inline styles.
    - **`is_active == true` (active state):** renders as `<a href="/" id="filter-tag-{{ filter_name }}" class="...indigo-600 text-white..." aria-label="Clear filter: {{ label }}"><span>{{ label }}</span><span aria-hidden="true">×</span></a>`. The href clears the filter (returns to root). The visible "×" is decorative; aria-label communicates the clear action.
    - The macro is **parameterized for reuse** — story 9-5 (overdue), 9-6 (gaps), 9-7 (recent activity) call it with their own `(label, count, filter_name, is_active)` tuples without modifying the macro.
4. **AC4 — Single SQL round-trip for the count.** The handler computes the unshelved count via a single `SELECT COUNT(*) FROM volumes WHERE location_id IS NULL AND deleted_at IS NULL` round-trip. **Schema note:** the column is `volumes.location_id`, NOT `volumes.storage_location_id` as the epics.md spec text says — verified at `migrations/20260329000000_initial_schema.sql` lines for the `CREATE TABLE volumes` block (column + `INDEX idx_volumes_location`). The literal spec text is a minor naming drift; the implementation uses the actual column name. New function lives at `src/models/volume.rs::count_unshelved(pool: &DbPool) -> Result<i64, AppError>`.
5. **AC5 — URL filter enum (`IndicatorFilter`).** A new closed enum at `src/routes/home.rs` (or a new module `src/routes/home_filters.rs` if `home.rs` grows uncomfortable):
    ```rust
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum IndicatorFilter {
        Unshelved,
        // Reserved for stories 9-5/9-6/9-7:
        // Overdue, Gaps, RecentCataloged, RecentReturns
    }
    ```
    Plus a parser `fn parse_indicator_filter(filter: &Option<String>) -> Option<IndicatorFilter>` that:
    - Returns `Some(Unshelved)` for `Some("unshelved")` (case-sensitive — closed enum).
    - Returns `None` for `Some("UNSHELVED")` and any other unknown bare-name value, AND emits `tracing::warn!(filter = %v, "Unknown indicator filter, ignoring")`.
    - Returns `None` for `Some("genre:N")` / `Some("state:foo")` patterns (those route through the existing `parse_filter` for the `#browse-results` swap; the indicator-filter parser stays out of their lane).
    - Returns `None` for `None`.
    - Anonymous-role handler MUST treat `parse_indicator_filter` result as `None` regardless (defensive — the role gate is the primary check, but a second guard prevents accidental leak if the order of checks is ever reordered).
6. **AC6 — Indicator filter result swaps the recent-additions slot.** Given a Librarian/Admin user navigating to `/?filter=unshelved`, when the home page renders, then:
    - The `#recent-additions` section is REPLACED by an `#unshelved-list` section in the SAME DOM position. Both sections cannot coexist in the rendered HTML.
    - The unshelved-list section shows the heading "Unshelved volumes" / "Volumes à ranger", followed by a flat `<ul>` with one `<li>` per unshelved volume (LIMIT 100 per render — large libraries don't need infinite scroll for v1; the count badge tells the user the total).
    - Each row shows: V-code label (e.g., `V0042`), the title, and the primary contributor. Each row is wrapped in `<a href="/title/{title_id}">` so a click navigates to the title detail page.
    - The unshelved tag in `#what-needs-attention` renders in active state (pill with ×).
    - When the unshelved-list is empty (count > 0 returned a Vec, but a race deleted them all between count and list — defensive), render the inline empty-state copy "No unshelved volumes" / "Aucun volume à ranger" inside the `#unshelved-list` section.
7. **AC7 — Single-active-filter constraint.** The unshelved indicator filter is mutually exclusive with the search query (`?q=...`) AND with the existing genre/state filters (`?filter=genre:N` / `?filter=state:foo`). When both an indicator filter AND a search/genre/state filter are present in the URL, the indicator filter takes precedence and the search query is ignored (with a `tracing::warn!`). The existing `?filter=` parser branches first on the indicator-filter enum; only if that returns `None` does the legacy `parse_filter` (genre/state) run.
8. **AC8 — Plain `<a href>` navigation, no HTMX boost in v1.** The FilterTag macro emits plain `<a href>` links. Clicking the tag triggers a FULL-page navigation (server renders the right state based on the URL). **Deviation from spec:** the spec text says "HTMX swap only, no full page reload" for the ✕ click and "HTMX boost-style for partial swap" for forward. **v1 ships full-page navigation for both directions** because: (a) keeps story scope focused on the component + URL enum infrastructure; (b) avoids inventing a new HTMX swap region (would require wrapping multiple dashboard sections in a single `<div id="home-dashboard">` swap target, a cross-cutting refactor); (c) plain `<a>` is the JS-disabled fallback the spec requires anyway. HTMX partial-swap polish belongs in a follow-up story (file as `type:change-request` GH Issue at story close if it becomes a UX pain point).
9. **AC9 — CSP compliance.** No `style="..."`, no `<style>`, no `<script>`, no `onclick=`, no inline event handlers. The FilterTag macro uses Tailwind utility classes only (UX-DR24 tokens via the existing `@theme` block in `static/css/input.css`). The `src/templates_audit.rs::no_inline_markup_in_templates` test must continue to pass.
10. **AC10 — i18n EN + FR.**
    - `dashboard.attention.heading` — EN: "What needs attention", FR: "À traiter"
    - `dashboard.attention.unshelved_label` — EN: "Unshelved volumes", FR: "Volumes à ranger"
    - `dashboard.attention.unshelved_clear_aria` — EN: "Clear filter: Unshelved volumes", FR: "Retirer le filtre : Volumes à ranger" (for the active-state aria-label)
    - `dashboard.attention.unshelved_empty` — EN: "No unshelved volumes", FR: "Aucun volume à ranger" (for the AC6 race-empty defensive copy)
    - The count badge displays the raw integer (no plural inflection — UX-DR4 keeps the badge compact and number-first; the label carries the noun).
    - After editing locale files, run `touch src/lib.rs && cargo build` to force the i18n proc-macro to re-read.
11. **AC11 — Unit tests.**
    - (a) `volume::count_unshelved` returns 0 on empty DB; returns N for N volumes with `location_id IS NULL`; excludes soft-deleted; excludes shelved (volumes with non-NULL `location_id`). DB-backed `#[sqlx::test]` in a new sibling file `tests/dashboard_unshelved.rs`.
    - (b) `volume::list_unshelved(pool, limit)` returns rows in `created_at DESC, id DESC` order (stable tiebreak); excludes soft-deleted titles AND volumes; honors `limit`; joins title + primary contributor in a single round-trip (mirror the projection style of `models::title::list_recent_active` from story 9-2). DB-backed `#[sqlx::test]` in same file.
    - (c) `parse_indicator_filter` unit tests covering: `Some("unshelved")` → `Some(Unshelved)`, `Some("UNSHELVED")` → `None`, `Some("genre:5")` → `None`, `Some("state:foo")` → `None`, `Some("nonsense")` → `None`, `None` → `None`. Plus an `assert_logs` test (or an inspection comment if the project has no log-capture helper) that the unknown-bare-name path logs a warning.
    - (d) Handler render tests in `src/routes/home.rs::mod tests`:
        - `home_anonymous_does_not_render_attention_section` — render with `role="anonymous"`; assert `id="what-needs-attention"` is NOT present AND `id="filter-tag-unshelved"` is NOT present.
        - `home_librarian_renders_attention_section_with_unshelved_tag` — populated case (count=5); assert section + tag both present, tag in default state with `aria-label="Unshelved volumes: 5"`, href `/?filter=unshelved`.
        - `home_librarian_renders_unshelved_tag_in_active_state_when_filter_active` — same as above but `is_active=true`; assert href is `/`, the visible "×" is present, and the active-state aria-label uses the clear copy.
        - `home_librarian_zero_count_hides_attention_section` — when `indicator_tags` is empty (all zero counts filtered out), assert the section is NOT in the HTML (mirrors story 9-3's `home_renders_stats_by_genre_empty_section_hidden` pattern).
        - `home_librarian_unshelved_filter_active_renders_unshelved_list_not_recent_additions` — populated case; assert `id="unshelved-list"` IS present AND `id="recent-additions"` is NOT present, in the same DOM position.
    - (e) FilterTag macro render test (no DB) — render the macro directly via a tiny test template that calls `{% call filter_tag::tag(...) %}{% endcall %}`. Assert the four-state matrix: count=0 default → empty; count=0 active → empty (defensive); count=N default → pill with count; count=N active → pill with ×.
12. **AC12 — E2E smoke (Foundation Rule #7, librarian role).** A new `test.describe("Home page — What needs attention / Unshelved indicator", ...)` block in `tests/e2e/specs/journeys/home.spec.ts`, placed AFTER the existing 9-3 stats-by-genre describe block:
    - Test 1 — anonymous: load `/`; assert `#what-needs-attention` has count 0; assert no `#filter-tag-unshelved`. Then navigate to `/?filter=unshelved`; assert section is still hidden + recent-additions still visible (filter ignored).
    - Test 2 — librarian smoke: `await loginAs(page, "librarian")` (real browser login per Foundation Rule #7); load `/`. If the seed DB has at least one unshelved volume (most CI fixtures do; the `dashboard_glance.rs` test pattern uses seed reference data), assert the unshelved tag is visible with a non-zero count. Click it; `await page.waitForURL(/\/\?filter=unshelved/)`; assert `#unshelved-list` is present and `#recent-additions` is NOT. Click the active-state ✕ pill; `await page.waitForURL("/")` (or `/$`); assert `#recent-additions` is back. If the seed DB has zero unshelved volumes (some CI fixtures), the test conditionally short-circuits with a green pass — same defensive pattern as 9-2's E2E.
    - Use i18n-aware regex matchers: `/What needs attention|À traiter/i`, `/Unshelved volumes|Volumes à ranger/i`. No `waitForTimeout` (CI grep gate).
    - Selectors scoped to `#what-needs-attention` and `#unshelved-list` to sidestep the unscoped-selector flake class flagged by 9-2 / 9-3.

## Tasks / Subtasks

- [x] **Task 1 — Schema-aware `volume::count_unshelved` + `list_unshelved` (AC: 4, 6, 11a, 11b)**
  - [x] In `src/models/volume.rs`, add `pub async fn count_unshelved(pool: &DbPool) -> Result<i64, AppError>` using `SELECT COUNT(*) FROM volumes WHERE location_id IS NULL AND deleted_at IS NULL`. Pattern: mirror `count_active` (story 9-1) — `sqlx::query_scalar::<_, i64>` + `.fetch_one(pool)`. Dynamic `query`, NOT the macro form.
  - [x] Add `pub async fn list_unshelved(pool: &DbPool, limit: u32) -> Result<Vec<UnshelvedVolumeRow>, AppError>` returning a new struct:
    ```rust
    pub struct UnshelvedVolumeRow {
        pub id: u64,                          // volume id
        pub label: String,                    // "V0042"
        pub title_id: u64,
        pub title: String,
        pub primary_contributor: Option<String>,
        pub media_type: String,
    }
    ```
    Single SQL round-trip JOINing `volumes` → `titles` → primary `title_contributors` (filtered to "Auteur" first via `ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC LIMIT 1` subquery, mirroring `title.rs::list_recent_active` lines 837-855). All JOINed entity tables filtered with `deleted_at IS NULL`. Sort: `ORDER BY v.created_at DESC, v.id DESC LIMIT ?`. Dynamic `query` (consistent with project convention; see Story 9-2 anti-pattern note).
  - [x] Place the struct definition near the existing `Volume` struct in `src/models/volume.rs`. Make it `pub struct` because the route handler / `HomeTemplate` needs it.
- [x] **Task 2 — `IndicatorFilter` enum + `parse_indicator_filter` (AC: 5, 7, 11c)**
  - [x] In `src/routes/home.rs`, add the enum + parser at module scope (above the existing `parse_filter`):
    ```rust
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum IndicatorFilter {
        Unshelved,
    }

    fn parse_indicator_filter(filter: &Option<String>) -> Option<IndicatorFilter> {
        match filter.as_deref() {
            Some("unshelved") => Some(IndicatorFilter::Unshelved),
            Some(v) if !v.contains(':') => {
                tracing::warn!(filter = %v, "Unknown indicator filter, ignoring");
                None
            }
            _ => None, // genre:/state:/None — legacy parse_filter handles those
        }
    }
    ```
    Reasoning for the `!v.contains(':')` guard: the existing `?filter=genre:5` and `?filter=state:foo` patterns must NOT trigger the unknown-bare-name warning. The presence of `:` is the heuristic that disambiguates "namespaced legacy filter" from "indicator filter".
  - [x] **6 unit tests** in `mod tests` covering the AC11c matrix (5 input → expected output cases + 1 log-emission case). For the log-emission case: if no log-capture helper exists in the project (likely — none observed in stories 9-1/9-2/9-3), assert via `tracing_subscriber::fmt::TestWriter` OR document the manual verification path in a comment and skip the assertion. Pragmatic option: add an assertion comment + ship without the runtime log capture; the warning is observable in `cargo run` smoke tests.
- [x] **Task 3 — `IndicatorTag` view-model + `build_indicator_tags` helper (AC: 1, 3, 6, 7, 11d)**
  - [x] In `src/routes/home.rs`, add a small view-model struct (mirroring 9-3's `StatsByGenreRow` pattern):
    ```rust
    pub struct IndicatorTag {
        pub label: String,        // pre-translated, e.g., "Unshelved volumes"
        pub count: u64,           // non-zero (caller filters)
        pub filter_name: String,  // "unshelved" — matches AC5 enum string
        pub is_active: bool,      // true when this filter is the active URL filter
        pub clear_aria_label: String, // pre-translated active-state aria-label
    }
    ```
  - [x] Add `fn build_indicator_tags(unshelved_count: i64, active: Option<IndicatorFilter>, loc: &str) -> Vec<IndicatorTag>` that:
    - Returns `Vec::new()` when `unshelved_count == 0` (zero-count rule for the only tag in v1).
    - Returns `vec![IndicatorTag { label: t!("dashboard.attention.unshelved_label"), count: unshelved_count as u64, filter_name: "unshelved".to_string(), is_active: active == Some(IndicatorFilter::Unshelved), clear_aria_label: t!("dashboard.attention.unshelved_clear_aria") }]` when count > 0.
    - For stories 9-5/9-6/9-7, this helper extends with additional `IndicatorTag` entries — the function shape is forward-compatible.
  - [x] Unit tests in `mod tests`:
    - `build_indicator_tags_zero_returns_empty_vec`
    - `build_indicator_tags_nonzero_returns_unshelved_tag_in_default_state`
    - `build_indicator_tags_nonzero_with_active_filter_marks_unshelved_active`
- [x] **Task 4 — Wire the home handler (AC: 1, 2, 4, 6, 7, 8)**
  - [x] In `src/routes/home.rs::home`, before the existing `let glance = …` block (around line 167):
    - **Anon role** → `let unshelved_count = 0i64; let unshelved_volumes: Option<Vec<UnshelvedVolumeRow>> = None; let active_indicator_filter = None;`. Skip the DB queries entirely.
    - **Librarian/Admin** → call `parse_indicator_filter(&params.filter)` → `active_indicator_filter`. Issue `volume::count_unshelved` with the soft-degrade pattern (warn + 0 on error, mirroring 9-1/9-2/9-3). If `active_indicator_filter == Some(Unshelved)`, ALSO issue `volume::list_unshelved(pool, 100)` with the same soft-degrade.
  - [x] Build `indicator_tags = build_indicator_tags(unshelved_count, active_indicator_filter, loc)`.
  - [x] Extend `HomeTemplate` (currently 55+ fields post-9-3) with FOUR new fields:
    - `attention_heading: String` (pre-translated)
    - `indicator_tags: Vec<IndicatorTag>` (empty Vec → section hidden; non-empty → section shown)
    - `unshelved_filter_active: bool` (drives the AC6 swap between `#recent-additions` and `#unshelved-list`)
    - `unshelved_volumes: Vec<UnshelvedVolumeRow>` (empty Vec when `unshelved_filter_active=false`; populated when active)
    - `unshelved_heading: String` (pre-translated, e.g., "Unshelved volumes" — used as the section heading when active)
    - `unshelved_empty_label: String` (pre-translated, for the AC6 race-empty defensive copy)
  - [x] Pre-translate all labels via `rust_i18n::t!(…).to_string()` per project convention (canonical example: `src/routes/home.rs:303-320`).
  - [x] **AC7 single-active-filter:** if `active_indicator_filter.is_some()`, the existing `parse_filter` result should be ignored AND the `query` field treated as empty (with a `tracing::warn!` if either was non-default). Easiest: short-circuit by setting `let query = String::new(); let (genre_id, volume_state) = (None, None);` when an indicator filter is active. The `#browse-results` section will render its empty state.
  - [x] **HTMX coexistence:** the existing HTMX search-fragment branch (`if is_htmx && (!query.trim().is_empty() || has_filter)`) must NOT fire when `active_indicator_filter.is_some()` — same short-circuit logic (no query, no `has_filter` for the legacy filter parser → fragment branch naturally doesn't fire). Verify by inspection.
- [x] **Task 5 — FilterTag macro template (AC: 3, 8, 9)**
  - [x] Create `templates/components/filter_tag.html`. Pattern: mirror `templates/components/cover.html`'s macro shape. Single macro, no extends:
    ```jinja
    {%- macro tag(label, count, filter_name, is_active, clear_aria_label) -%}
    {%- if count > 0 -%}
    {%- if is_active -%}
    <a href="/" id="filter-tag-{{ filter_name }}" class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full bg-indigo-600 text-white dark:bg-indigo-500 text-sm hover:bg-indigo-700 dark:hover:bg-indigo-600 transition-colors" aria-label="{{ clear_aria_label }}">
        <span>{{ label }}</span>
        <span aria-hidden="true">&times;</span>
    </a>
    {%- else -%}
    <a href="/?filter={{ filter_name }}" id="filter-tag-{{ filter_name }}" class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full bg-stone-100 dark:bg-stone-800 text-stone-700 dark:text-stone-300 text-sm hover:bg-stone-200 dark:hover:bg-stone-700 transition-colors" aria-label="{{ label }}: {{ count }}">
        <span>{{ label }}</span>
        <span class="font-semibold tabular-nums">{{ count }}</span>
    </a>
    {%- endif -%}
    {%- endif -%}
    {%- endmacro -%}
    ```
  - [x] In `templates/pages/home.html`, import the macro at the top (next to the existing `{% import "components/cover.html" as cover %}`) as `{% import "components/filter_tag.html" as filter_tag %}`.
- [x] **Task 6 — Render `#what-needs-attention` + `#unshelved-list` sections in home.html (AC: 1, 2, 6, 9)**
  - [x] Insert `#what-needs-attention` AT THE TOP of the dashboard sections — BEFORE `#collection-glance` (which currently sits at template line ~78). The section is wrapped in `{% if !indicator_tags.is_empty() %}` (AC2 + AC3 zero-count rule). Inside: heading + a flex-wrap container of `{% call filter_tag::tag(...) %}{% endcall %}` calls, one per item in `indicator_tags`.
  - [x] In the `#recent-additions` block (currently at template lines ~106-141), wrap the entire section in `{% if !unshelved_filter_active %}…{% else %}<section id="unshelved-list">…</section>{% endif %}`. The two sections occupy the same DOM position; only one renders at a time (AC6).
  - [x] The `#unshelved-list` section: heading (`{{ unshelved_heading }}`), then either (a) the empty-state inline div with `{{ unshelved_empty_label }}` if `unshelved_volumes.is_empty()`, OR (b) `<ul class="mt-3 space-y-2">` with one `<li>` per row. Each `<li>` is a full-row `<a href="/title/{{ row.title_id }}">` showing V-code + title + author. Tailwind utility classes only — no new CSS file needed (text + flex layout suffices).
  - [x] CSP: zero `style="..."`, zero `<script>`, zero `onclick=`. The `src/templates_audit.rs::no_inline_markup_in_templates` test must continue to pass after this change.
- [x] **Task 7 — i18n keys (AC: 10)**
  - [x] In `locales/en.yml`, under the existing `dashboard:` block (after `stats_by_genre:`), add:
    ```yaml
      attention:
        heading: What needs attention
        unshelved_label: Unshelved volumes
        unshelved_clear_aria: "Clear filter: Unshelved volumes"
        unshelved_empty: No unshelved volumes
    ```
  - [x] In `locales/fr.yml`, mirror under the same path:
    ```yaml
      attention:
        heading: À traiter
        unshelved_label: Volumes à ranger
        unshelved_clear_aria: "Retirer le filtre : Volumes à ranger"
        unshelved_empty: Aucun volume à ranger
    ```
  - [x] **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper — keys start at root. After editing, `touch src/lib.rs && cargo build`. The `i18n::audit::tests::all_t_keys_have_both_locales` test enforces EN/FR mirror — keep them aligned exactly.
- [x] **Task 8 — Tests (AC: 11, 12)**
  - [x] **`tests/dashboard_unshelved.rs`** (new sibling file, mirror of `tests/dashboard_stats_by_genre.rs` from 9-3):
    - `count_unshelved_on_empty_db_returns_zero` — fresh schema, no fixtures, expect `0`.
    - `count_unshelved_excludes_shelved_and_soft_deleted` — seed: 3 unshelved active, 2 shelved active (with `location_id`), 1 unshelved soft-deleted, 1 shelved soft-deleted; expect `3`.
    - `list_unshelved_returns_in_created_at_desc_order_with_limit` — seed 5 unshelved with distinct `created_at`; call `list_unshelved(pool, 3)`; assert exactly 3 rows in newest-first order.
    - Use `wipe_seeded_genres` + new helpers `insert_volume_unshelved` and `insert_volume_at_location` patterned after the existing `dashboard_stats_by_genre.rs` test helpers. **Critical:** `created_at` determinism — use `INSERT INTO volumes (..., created_at) VALUES (..., NOW() - INTERVAL ? MINUTE)` per the same pattern as `tests/dashboard_recent_additions.rs::insert_title_with_created_at`.
  - [x] **`src/routes/home.rs::mod tests`** — extend the existing test module (currently at lines 612+ post-9-3 follow-ups):
    - 5 handler render tests (AC11d) — extend `make_test_home_template_with_counts` factory to accept `indicator_tags` + `unshelved_filter_active` + `unshelved_volumes` parameters, OR create a new factory `make_test_home_template_with_indicators(role, indicator_tags, unshelved_filter_active, unshelved_volumes)`. Reuse `slice_section`. Add a new `attention_section_slice` helper.
    - `parse_indicator_filter` 6 unit tests (AC11c).
    - `build_indicator_tags` 3 unit tests (Task 3).
    - FilterTag macro test (AC11e) — render a tiny fixture template that imports + calls the macro four times (count=0×default, count=0×active, count=N×default, count=N×active); assert HTML shape per case.
  - [x] **`tests/e2e/specs/journeys/home.spec.ts`** — append a new `test.describe("Home page — What needs attention / Unshelved indicator", …)` block AFTER the 9-3 stats-by-genre describe block. Two tests (anonymous + librarian) per AC12 with conditional empty-DB short-circuit on the librarian path.
- [x] **Task 9 — Verify and document (AC: 1–12)**
  - [x] `SQLX_OFFLINE=true cargo check && cargo clippy --all-targets -- -D warnings` — clean (zero warnings).
  - [x] `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test` — full suite green. Expected count: ~660 lib (post-follow-ups was 652) + ~14 new tests = ~666 lib; +3 new integration tests in `tests/dashboard_unshelved.rs`.
  - [x] `cargo sqlx prepare --check --workspace` — expected no diff (Tasks 1+2 use dynamic `query` / `query_scalar`).
  - [x] Tailwind rebuild — `npx @tailwindcss/cli -i static/css/input.css -o static/css/output.css --minify`. Verify any new utility classes used (`tabular-nums` was added in 9-3, `inline-flex` is widespread; nothing new expected).
  - [x] Manual smoke from a running dev instance (`MYBIBLI_SKIP_SETUP=1 cargo run`):
    - As anonymous: `curl http://localhost:8080/` and grep — `id="what-needs-attention"` MUST NOT appear; `id="filter-tag-unshelved"` MUST NOT appear.
    - As librarian (login first): `curl` with the session cookie → grep — `id="what-needs-attention"` appears IFF unshelved count > 0; `id="filter-tag-unshelved"` appears.
    - Click the tag in a browser → URL changes to `/?filter=unshelved` → `#unshelved-list` replaces `#recent-additions` → click ✕ → URL returns to `/` → recent-additions back.
  - [x] **E2E** (Foundation Rule #13) — `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/home.spec.ts`. If local `tests/e2e/test-results/` ownership blocker persists from 9-1/9-2/9-3, document the skip in Dev Agent Record and rely on CI.
  - [x] Update Dev Agent Record at the bottom of this file: list of files touched, decisions on placement (top vs between sections), anything surprising.
  - [x] Update `_bmad-output/implementation-artifacts/sprint-status.yaml`: `9-4-filtertag-and-unshelved-indicator: ready-for-dev → in-progress` at start, `→ review` at end (only this line + `last_updated`, per CLAUDE.md rule 16).

## Dev Notes

### Source tree references

| Concern | File / location | Notes |
|---|---|---|
| Home route registration | `src/routes/mod.rs:87` | `.route("/", axum::routing::get(home::home))` — no change needed |
| Home handler | `src/routes/home.rs:87-326` | extend the existing handler; do NOT create a parallel route |
| Home template | `templates/pages/home.html` (~265 lines post-9-3) | extends `layouts/base.html`; section blocks: search (14-33), filter pills (36-65), metadata error (67-74), `#collection-glance` (78-104), `#recent-additions` (110-141), `#stats-by-genre` (143-167), browse toggle (169+), `#browse-results` (197+) |
| Insertion point for `#what-needs-attention` | `templates/pages/home.html` immediately AFTER the metadata-error badge block (~line 75) and BEFORE `#collection-glance` (line ~78) | top of dashboard, librarian/admin only |
| Insertion point for `#unshelved-list` | `templates/pages/home.html` — wrap the entire `#recent-additions` block (~lines 110-141) in `{% if !unshelved_filter_active %}…{% else %}<section id="unshelved-list">…</section>{% endif %}` | same DOM position as recent-additions; mutually exclusive (AC6) |
| Volume schema | `migrations/20260329000000_initial_schema.sql` `CREATE TABLE volumes` | column is `location_id` (NOT `storage_location_id` as spec text says); `INDEX idx_volumes_location` already exists |
| Volume model | `src/models/volume.rs` | extend with `count_unshelved` + `list_unshelved` + `UnshelvedVolumeRow` struct; pattern from `count_active` (story 9-1) |
| Existing `parse_filter` (genre/state) | `src/routes/home.rs:426-438` | DO NOT modify — the new `parse_indicator_filter` is a SIBLING, runs FIRST in the handler chain (AC7 single-active-filter) |
| Existing `is_singular` helper | `src/routes/home.rs:333-338` | NOT used by 9-4 — the indicator badge displays raw count, no plural inflection |
| Existing `format_percent` helper | `src/utils.rs::format_percent` | NOT used by 9-4 — no percentage in the unshelved indicator |
| Soft-degrade pattern | `src/routes/home.rs:167-186` (glance + recent_additions); `src/routes/home.rs:188-198` (stats_by_genre) | replicate for unshelved count + list |
| HomeTemplate struct | `src/routes/home.rs:31-110` (struct + `StatsByGenreRow`) | extend with 6 new fields per Task 4 |
| Test factory + slice helpers | `src/routes/home.rs::mod tests` (`slice_section`, `make_test_home_template_with_*`) | reuse + extend (no rewrite) |
| i18n locales | `locales/en.yml:342-358`, `locales/fr.yml:342-358` (`dashboard:` block) | append `attention:` sub-block after `stats_by_genre:` |
| i18n audit | `src/i18n/audit.rs::all_t_keys_have_both_locales` | enforces EN/FR mirror |
| Templates audit | `src/templates_audit.rs::no_inline_markup_in_templates` | must stay green |
| FilterTag macro precedent | `templates/components/cover.html` | macro shape (no `{% extends %}`, single `{% macro %}…{% endmacro %}` block, called via `{% call cover::cover(…) %}{% endcall %}`) |
| `list_recent_active` projection | `src/models/title.rs:837-855` | the SELECT + JOIN + primary contributor subquery shape — mirror in `list_unshelved` |
| Test pattern (DB-backed integration) | `tests/dashboard_glance.rs`, `tests/dashboard_recent_additions.rs`, `tests/dashboard_stats_by_genre.rs` (story 9-3) | `#[sqlx::test(migrations = "./migrations")]`; file-local helpers; `wipe_seeded_genres` from 9-3 — clone if needed |
| Test pattern (handler render, no DB) | `src/routes/home.rs::mod tests` lines 670-1284 (post-9-3 follow-ups) | reuse the slice + factory pattern verbatim |
| E2E spec for `/` | `tests/e2e/specs/journeys/home.spec.ts` (post-9-3 has 3 describe blocks: Home page, Collection at a glance, Recent additions, Stats by genre) | extend with the new "What needs attention" describe AFTER 9-3's |
| E2E loginAs helper | `tests/e2e/helpers/auth.ts` | `loginAs(page, "librarian")` — typed union, do not pass other strings |

### Anti-patterns to avoid

- **Issuing the unshelved-count query for Anonymous users.** AC2 forbids it (no role-gated leak; no DB load for surfaces the user can't see). The handler MUST short-circuit BEFORE the count query when `session.role < Role::Librarian`.
- **Rendering the unshelved tag at zero count.** AC3 + UX-DR4 zero-count rule. The `#what-needs-attention` section also disappears when ALL tags are zero (handler filters zero-count tags out of `indicator_tags` before passing to the template; template checks `!indicator_tags.is_empty()`).
- **Coexisting `#recent-additions` AND `#unshelved-list` in the rendered HTML.** AC6 mandates mutual exclusion in the same DOM position. A template that renders both simultaneously would break the visual layout AND duplicate data fetches.
- **Letting indicator filter coexist with `?q=` or `?filter=genre:N`.** AC7 single-active-filter — the indicator filter takes precedence; the legacy parser receives empty inputs when an indicator filter is active.
- **Hardcoded color hex in CSS.** UX-DR24 mandates `var(--color-*)` tokens. The FilterTag macro uses Tailwind `bg-indigo-600` / `bg-stone-100` etc. — these RESOLVE to the `@theme` tokens defined in `static/css/input.css`. NO new CSS file needed; NO hex values.
- **`sqlx::query!` macro.** Forces `cargo sqlx prepare` regeneration in the PR. Use dynamic `query` / `query_scalar` (project convention; see Story 9-2/9-3 anti-pattern note).
- **Calling `t!()` from inside the Askama template.** Pre-translate in the handler, pass as `String` fields. Project convention; canonical example: `src/routes/home.rs:303-320`.
- **Inline `style="..."` or `<style>` blocks** for active-state highlighting. The active vs default-state styling lives in the macro's class list (`bg-indigo-600 ...` vs `bg-stone-100 ...`), driven by the `is_active` boolean. CSP-clean.
- **HTMX swap target invention.** AC8 explicitly defers HTMX boost to a future polish story. v1 ships plain `<a href>` for both forward and backward navigation. A `<div id="home-dashboard">` swap-region wrapper + per-link `hx-target` is a cross-cutting refactor that this story does not own.
- **Running the unshelved-LIST query when the filter is NOT active.** Handler must guard: `if active_indicator_filter == Some(Unshelved) { volume::list_unshelved(...) } else { Vec::new() }`. Wasteful otherwise.
- **Using `INDEX idx_volumes_storage_location_id` or any reference to the wrong column name.** The schema column is `location_id` (FK to `storage_locations`). Documented in AC4.
- **Anonymous handler crafting `/?filter=unshelved` and getting the unshelved list.** AC2 explicitly forbids — the handler's role check happens BEFORE the count query AND the parse_indicator_filter result is discarded for Anonymous.

### Architecture compliance

- **Error handling:** Any DB failure in `count_unshelved` / `list_unshelved` returns `AppError::Database` via `?`; the handler soft-degrades with `tracing::warn!` + `0` (count) or `Vec::new()` (list), per the established 9-1/9-2/9-3 pattern. The home page MUST NOT 500 because the indicator query had a hiccup.
- **Logging:** `tracing::warn!` at handler level on the soft-degrade paths AND on the unknown-bare-name `parse_indicator_filter` branch; `tracing::debug!` only inside model functions if needed. Indicator-related state changes are not interesting at info-level.
- **DB query discipline:** Every SELECT/JOIN of entity tables (`volumes`, `titles`, `contributors`, `contributor_roles`) MUST include `deleted_at IS NULL`. The `count_unshelved` query uses ONLY `volumes.deleted_at IS NULL`; the `list_unshelved` query also includes it on titles + contributors + contributor_roles via the joined subquery (mirror `list_recent_active` from story 9-2).
- **HTMX coexistence:** the `#what-needs-attention` and `#unshelved-list` (or `#recent-additions`) sections sit OUTSIDE `#browse-results` (HTMX swap target) — same invariant as 9-1/9-2/9-3. Plain `<a href>` navigation does not interact with the existing HTMX search-fragment branch.
- **CSP middleware:** Already wraps the handler outermost. No work needed.
- **Pool access:** The handler already has `state.pool: DbPool`. Do not introduce a new connection.
- **One-branch-one-story (Foundation Rule #14):** Verify `git status` clean + on `main` + `git pull --ff-only` before starting; cut `story/9-4-filtertag-and-unshelved-indicator`. Open a draft PR (Rule #15) at the first commit.
- **Source-file-size limit (Foundation Rule #12):** `src/routes/home.rs` is ~1300 lines post-9-3 follow-ups. This story adds ~80 LOC handler + 1 enum + 1 parser + 1 view-model + 1 helper + ~10 new tests = ~250 LOC. Total ~1550, comfortable headroom to 2000. If clearly approaching the limit, consider extracting the indicator-filter machinery into `src/routes/home_indicators.rs` as a follow-up (NOT in this story).

### Library / framework requirements

- **Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + Tailwind CSS v4 + HTMX 2.0** — no version changes, no new dependencies.
- **rust_i18n** — already wired. Pre-translate the four new keys (heading + unshelved_label + unshelved_clear_aria + unshelved_empty) in the handler via `rust_i18n::t!(…).to_string()`. No `count =` interpolation needed (the badge displays raw integer; no plural inflection).
- **Askama macros** — the FilterTag component uses Askama's `{% macro %}` / `{% call %}` shape, mirroring `templates/components/cover.html`. No new template engine features.

### File structure requirements

| File | Action | Rough size |
|---|---|---|
| `src/models/volume.rs` | **edit** | +~70 lines (`count_unshelved` + `list_unshelved` + `UnshelvedVolumeRow` struct) |
| `src/routes/home.rs` | **edit** | +~80 lines handler block + `IndicatorFilter` enum + `parse_indicator_filter` + `IndicatorTag` view-model + `build_indicator_tags` helper + 6 template fields + ~14 new test cases + factory extension |
| `templates/components/filter_tag.html` | **create** | ~25 lines (single macro, two branches: active vs default) |
| `templates/pages/home.html` | **edit** | +~50 lines (`#what-needs-attention` section + `#unshelved-list` section + the `{% import %}` for the FilterTag macro + the `{% if !unshelved_filter_active %}` wrap around `#recent-additions`) |
| `locales/en.yml` | **edit** | +~5 lines under `dashboard.attention:` |
| `locales/fr.yml` | **edit** | +~5 lines under `dashboard.attention:` |
| `tests/dashboard_unshelved.rs` | **create** | ~120 lines (3 `#[sqlx::test]` cases + helpers) |
| `tests/e2e/specs/journeys/home.spec.ts` | **edit** | +~50 lines (1 new `test.describe` block, 2 tests) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **edit** | only the `9-4-...` line + `last_updated` (per CLAUDE.md rule 16) |
| `_bmad-output/implementation-artifacts/9-4-filtertag-and-unshelved-indicator.md` | **edit** | this file — Status, Tasks checked, Dev Agent Record on completion |

### Testing requirements

- **Coverage target:** every AC has at least one test (unit OR E2E). AC9 (CSP) is covered by `templates_audit.rs::no_inline_markup_in_templates` (existing — must stay green).
- **AC2 anonymous-no-leak** is the load-bearing security invariant. The render test `home_anonymous_does_not_render_attention_section` is the primary regression guard. The E2E anonymous test is a secondary integration-level guard.
- **AC4 schema column name** (`location_id` vs `storage_location_id`) is the load-bearing schema invariant. The unit test `count_unshelved_excludes_shelved_and_soft_deleted` will fail at the SQL parse step if the wrong column name is used — sqlx returns a clear error, the test fails noisily, the column-name regression is impossible to ship.
- **AC6 mutual exclusion** of `#recent-additions` vs `#unshelved-list` is the load-bearing layout invariant. The render test `home_librarian_unshelved_filter_active_renders_unshelved_list_not_recent_additions` is the regression guard.
- **AC11e FilterTag macro 4-state matrix** — without this test, a future regression that swaps the active/default class lists, or the macro that emits a tag at count=0, would slip past the handler-level tests (which only exercise specific scenarios).
- **E2E** keeps to 1 anonymous + 1 librarian smoke test for parsimony. The librarian test is conditional on seed-DB unshelved volumes existing; same defensive pattern as 9-2/9-3.

### Project structure notes

This story aligns cleanly with the existing structure. Three intentional design decisions worth flagging:

1. **`parse_indicator_filter` is a sibling of `parse_filter`, not a replacement.** The existing `parse_filter` returns `(Option<u64>, Option<String>)` for `genre:N` / `state:foo` and feeds the `#browse-results` swap. The new parser handles bare-name closed enum values for indicator filters (`unshelved`, future `overdue`/`gaps`/etc.) and feeds the dashboard swap. Both run in sequence — indicator first, fall through to legacy parser if not matched. The `:` heuristic in the unknown-bare-name guard prevents false-positive warnings on legacy patterns. Future cleanup (3+ stories down the line, when the URL filter grammar settles) can unify the two into a single parsing layer; not in this story's scope.

2. **`UnshelvedVolumeRow` is a NEW model struct, NOT a reuse of `SearchResult`.** `SearchResult` is title-centric (id = title id, fields like `volume_count`, `genre_name`); the unshelved list is volume-centric (id = volume id, fields like `label` / V-code, `title_id` for navigation). Forcing `SearchResult` would require padding fields with sentinel values and renaming `id`'s meaning in the rendering — confusing and error-prone. A focused struct is cleaner. Stories 9-7 (recent cataloged + recent returns) may extend or sibling this struct; cross-cutting "VolumeRow" generalization is out of scope.

3. **Plain `<a href>` over HTMX boost in v1 (AC8 deviation).** Documented thoroughly in AC8 with rationale. The spec text says "HTMX swap only, no full page reload" for the ✕ click; v1 ships full-page navigation for both directions. The trade-off: a single-tab user clicking the indicator filter sees the entire page reload (slower than partial swap, but: no JS dependency, no swap-target wrapper refactor, plain links work without JS, browser history works naturally). HTMX boost is a scoped polish layer that can be added in a future story without rewriting the FilterTag component or its callers.

The `cover.html` macro precedent is the model for `filter_tag.html` — a single component file imported into `home.html` and called via `{% call ... %}`. No deviation, no new template architecture.

## References

- [Story 9.4 spec — `_bmad-output/planning-artifacts/epics.md` lines 1259–1277](../planning-artifacts/epics.md)
- [Epic 9 scope note + split philosophy — `epics.md` lines 1200–1206](../planning-artifacts/epics.md)
- [PRD FR58 (actionable indicators) — `_bmad-output/planning-artifacts/prd.md`](../planning-artifacts/prd.md) (search for `FR58`)
- [UX-DR4 (FilterTag dual state) — `_bmad-output/planning-artifacts/ux-design-specification.md`](../planning-artifacts/ux-design-specification.md)
- [Story 9-1 spec (canonical patterns: handler-side i18n, single round-trip, soft-degrade, slice helper for scoped tests) — `9-1-dashboard-global-stats-card.md`](./9-1-dashboard-global-stats-card.md)
- [Story 9-2 spec (canonical patterns: `list_recent_active` projection with primary contributor subquery, `created_at` determinism in tests, sibling integration test file) — `9-2-dashboard-recent-additions.md`](./9-2-dashboard-recent-additions.md)
- [Story 9-3 spec (canonical patterns: view-model struct in `home.rs`, render test factory extension, byte-identical anonymous parity test, hide-entirely template gating, `wipe_seeded_genres` test helper) — `9-3-dashboard-stats-by-genre.md`](./9-3-dashboard-stats-by-genre.md)
- [`CLAUDE.md` — Foundation Rules (#1 DRY, #7 E2E smoke per epic, #11 GitHub Issues, #12 ≤ 2000 LOC, #13 local testing, #14 one-branch-one-story, #15 draft PR, #16 sprint-status ownership, #18 CI gating)](../../CLAUDE.md)
- [Volume schema — `migrations/20260329000000_initial_schema.sql` `CREATE TABLE volumes` block (column is `location_id`, NOT `storage_location_id` as spec text says)](../../migrations/20260329000000_initial_schema.sql)
- [Cover macro precedent — `templates/components/cover.html`](../../templates/components/cover.html)
- [Existing `parse_filter` (genre/state) — `src/routes/home.rs:426-438`](../../src/routes/home.rs)

## Dev Agent Record

### Agent Model Used

`claude-opus-4-7` (1M context).

### Debug Log References

- `SQLX_OFFLINE=true cargo check --all-targets` — clean.
- `SQLX_OFFLINE=true cargo clippy --all-targets -- -D warnings` — clean (zero warnings).
- `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test` — full suite green:
  - **668 lib tests** (was 652 post-9-3 follow-ups, +16 new: 6 `parse_indicator_filter` + 3 `build_indicator_tags` + 6 handler render + 1 FilterTag macro guard).
  - `tests/dashboard_unshelved.rs` — 3/3 integration tests passing (empty DB, count excludes shelved + soft-deleted, list ordering + LIMIT).
  - All other integration suites (dashboard_glance 9-1, dashboard_recent_additions 9-2, dashboard_stats_by_genre 9-3) unchanged.
- `cargo sqlx prepare --check` — `.sqlx/` cache shows zero diff (Tasks 1+2 use dynamic `query_as` / `query_scalar`, no macro form).
- Tailwind rebuild not needed — all utility classes used (`tabular-nums`, `inline-flex`, `font-mono`, `transition-colors`, etc.) already present in `output.css` from prior stories. Verified by grep.
- `templates_audit::no_inline_markup_in_templates` — green. The `&times;` in the FilterTag macro is an HTML entity (decimal-encoded), not an inline event handler or style attribute.
- Manual `grep -rE 'waitForTimeout\(' tests/e2e/specs/ tests/e2e/helpers/` — clean (no new violations of the CI grep gate).
- E2E run not executed locally (same `tests/e2e/test-results/` root-ownership blocker as 9-1/9-2/9-3 documented). CI on the story branch validates `home.spec.ts`'s new "What needs attention" describe block.

### Completion Notes List

- **Schema column was `volumes.location_id`** (not `storage_location_id` as the spec text said). The integration test `count_unshelved_excludes_shelved_and_soft_deleted` would have failed at SQL parse time if the wrong column name was used — built-in regression guard.
- **`storage_locations.label` is `CHAR(5)`** — the integration test helper for inserting a location had to use a 5-char label (e.g., `L9401`), not a longer descriptive string. Caught at first test run; documented inline in `insert_location` helper.
- **`storage_locations.name` is `NOT NULL` with no default** — needed to be explicitly bound in the helper. Same first-run discovery; helper now binds `label` to both `label` and `name` columns for simplicity.
- **`parse_indicator_filter` adds a `!v.is_empty()` guard** beyond the spec's `!v.contains(':')` heuristic — an empty `?filter=` query string would otherwise emit a `tracing::warn!` for nothing on every legitimate request that just clears the filter. Covered by the `parse_indicator_filter_none_and_empty_return_none` test.
- **Single-active-filter precedence (AC7)** is enforced in the handler by reordering: indicator parser runs FIRST, and if it returns `Some(...)`, the handler clears `query` to empty AND skips legacy `parse_filter` AND sets `has_filter = false`. This makes the existing search-fragment HTMX branch naturally not fire (its predicate becomes false). Logged at `tracing::warn!` if both an indicator filter and a non-default `?q=`/`?sort=` were provided.
- **Anonymous role short-circuits BEFORE both queries** (`count_unshelved` and `list_unshelved`) — handler-side guard at the role check, defensive guard at `unshelved_filter_active = session.role >= Librarian && active_indicator_filter == Some(Unshelved)`. Two-layer defense; covered by `home_anonymous_does_not_render_attention_section`.
- **FilterTag macro defensive guard** — beyond the section-level `{% if !indicator_tags.is_empty() %}` and the helper-side filter, the macro itself emits nothing when `count == 0`. Covered by `filter_tag_macro_hides_zero_count_pill_even_when_section_renders` — exercises a forced-non-empty Vec containing a zero-count tag, which would slip past the section guard but still hit the macro guard.
- **Initial template-placement bug** caught in dev: first `#what-needs-attention` insertion landed BEFORE the metadata-error badge block (line ~67) instead of AFTER it (between metadata-error and `#collection-glance`). Re-positioned to correct location. The `home_renders_glance_above_recent_additions` test (story 9-2) keeps glance above recent-additions; no equivalent invariant exists for "metadata-error before what-needs-attention" yet — manual smoke verified.
- **Plain `<a href>` over HTMX boost decision** stood up well — clicking the tag and the ✕ both produce predictable URL changes (`/?filter=unshelved` and `/`) that the handler renders correctly. The full-page reload is fast on the dev machine; deferred HTMX polish remains a future-story option without architectural debt.
- **No new GH Issues filed.** No deferred findings; the cross-cutting genre-filter UX issue (#112) from 9-3 is the closest-related deferred item — the unshelved filter follows the same `?filter=` pattern but doesn't suffer from stale-link issues since "unshelved" is not an entity id that can be deleted.

### File List

| File | Action |
|---|---|
| `src/models/volume.rs` | edit — added `count_unshelved` + `list_unshelved` async fns + `UnshelvedVolumeRow` pub struct |
| `src/routes/home.rs` | edit — added `IndicatorFilter` enum + `parse_indicator_filter` + `IndicatorTag` view-model + `build_indicator_tags` helper + handler block (anonymous short-circuit + soft-degrade + role-gated list query) + `unshelved_filter_active`/`unshelved_volumes` short-circuit logic for AC7 + 6 new HomeTemplate fields + `attention_section_slice` helper + `make_test_home_template_with_indicators`/`fake_indicator_tag`/`fake_unshelved_row` factories + 6 parser tests + 3 build_indicator_tags tests + 6 handler render tests + 1 FilterTag macro guard test |
| `templates/components/filter_tag.html` | create — single `tag(...)` macro, default + active state branches, CSP-clean Tailwind classes only |
| `templates/pages/home.html` | edit — added `{% import filter_tag %}`, inserted `#what-needs-attention` section between metadata-error block and `#collection-glance`, wrapped `#recent-additions` in `{% if unshelved_filter_active %}<section id="unshelved-list">…</section>{% else %}<section id="recent-additions">…</section>{% endif %}` for AC6 mutual exclusion |
| `locales/en.yml` | edit — `dashboard.attention.{heading, unshelved_label, unshelved_clear_aria, unshelved_empty}` |
| `locales/fr.yml` | edit — same path, FR variants ("À traiter" / "Volumes à ranger" / "Retirer le filtre : Volumes à ranger" / "Aucun volume à ranger") |
| `tests/dashboard_unshelved.rs` | create — 3 `#[sqlx::test]` cases + helpers (`first_genre_id`, `first_volume_state_id`, `insert_title`, `insert_location`, `insert_volume_unshelved`, `insert_volume_at_location`, `insert_volume_unshelved_with_age`, `soft_delete`) |
| `tests/e2e/specs/journeys/home.spec.ts` | edit — appended `test.describe("Home page — What needs attention / Unshelved indicator", …)` block with 2 tests (anonymous AC2 + librarian smoke with empty-DB short-circuit) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | edit — `9-4-filtertag-and-unshelved-indicator: ready-for-dev → in-progress → review` (per CLAUDE.md rule 16) |
| `_bmad-output/implementation-artifacts/9-4-filtertag-and-unshelved-indicator.md` | edit — Status `ready-for-dev → review`, all Tasks checked, Dev Agent Record filled |

### Change Log

- **2026-05-02** — Initial implementation. All 9 tasks complete; 668 lib tests + 3 dashboard_unshelved integration tests pass; clippy clean; sqlx cache unchanged; templates audit green. Followed Story 9-1/9-2/9-3 patterns (handler-side i18n, soft-degrade on DB error, single round-trip queries, scoped HTML assertions, sibling integration test file). Three drift discoveries during dev: `storage_locations.label` is `CHAR(5)` (test helper used a 5-char label `L9401`); `storage_locations.name` is `NOT NULL` no default (helper binds the label to both columns); template placement of `#what-needs-attention` corrected to land between metadata-error block and `#collection-glance`. Plain `<a href>` decision (deviation from spec's "HTMX swap only") held up cleanly — full-page reload is fast and predictable; HTMX polish remains a future option. E2E run deferred to CI per 9-1/9-2/9-3 precedent. PR #114 squash-merged into `main` as commit 3204d4d.

- **2026-05-02** — Code review pass (3 parallel reviewers: Blind Hunter, Edge Case Hunter, Acceptance Auditor). 0 decision-needed, **4 patches applied** (1 Medium UX + 3 Low coverage gaps), 0 deferred, 16+ dismissed. Final test counts: 671 lib tests + 4 integration tests, all green. Patches landed via PR `chore/9-4-code-review-followups`.

### Review Findings

**Code review pass — 2026-05-02** (post-merge of PR #114). 3 parallel reviewers raised ~27 findings; triaged into 4 patch + 0 defer + 16+ dismiss.

**Patch (Medium):**

- [x] [Review][Patch] **Active filter at zero count strands the user — no visible escape hatch** [`src/routes/home.rs::build_indicator_tags` + `templates/components/filter_tag.html`] — when a librarian was on `/?filter=unshelved` and the count dropped to 0 (e.g., last unshelved volume just got shelved), `build_indicator_tags` returned an empty Vec → `#what-needs-attention` section hidden → no visible ✕ to clear the filter; user had to edit the URL or use Back. Fix: (a) helper now emits the unshelved tag in active state regardless of count when its filter is the active URL filter, (b) FilterTag macro flipped to render the active-state pill unconditionally and the default-state pill only when count > 0. New tests `filter_tag_macro_renders_active_pill_even_when_count_is_zero` and `build_indicator_tags_zero_count_with_active_filter_still_emits_active_tag` lock the contract.

**Patch (Low):**

- [x] [Review][Patch] **AC1 placement regression guard absent** [`src/routes/home.rs::mod tests`] — no test pinned `#what-needs-attention` ABOVE `#collection-glance` in document order. A future template edit re-ordering the dashboard sections would slip past CI. Fix: added `home_renders_what_needs_attention_above_collection_glance` mirroring 9-2's review-fix `home_renders_glance_above_recent_additions` pattern.
- [x] [Review][Patch] **`id DESC` tiebreak unverified** [`tests/dashboard_unshelved.rs::list_unshelved_returns_in_created_at_desc_order_with_limit`] — the SQL ORDER BY clause includes `created_at DESC, id DESC` but no test seeded two volumes with an identical `created_at`. Fix: new `list_unshelved_id_desc_tiebreak_when_created_at_matches` inserts two volumes with `INTERVAL 0 MINUTE` (identical timestamp via single-query NOW()) and asserts the higher id comes first.
- [x] [Review][Patch] **FilterTag 4-state matrix coverage 3/4** [`src/routes/home.rs::filter_tag_macro_*`] — count=0×is_active=true was the missing matrix corner. Fix folded into the P1 patch above (`filter_tag_macro_renders_active_pill_even_when_count_is_zero` covers exactly this corner with the new behavior).

**Defer:** none this round.

**Dismissed (high-level rationale):**

- **Blind: `list_unshelved` `unwrap_or_default` swallows decode errors** — pattern is consistent with `models::title::list_recent_active` from story 9-2 (already shipped). Dismissed in 9-2's review for the same reason ("consistent with `active_search`"). Cross-cutting refactor would be its own story.
- **Blind: anonymous WARN spam** — reviewer misread the predicate; anonymous role short-circuits BEFORE the parser, never logs.
- **Blind/Edge: `bind(limit)` u32 type concern** — consistent with 9-2's `list_recent_active`; 668 tests pass.
- **Blind/Edge: `count as u64` cast unguarded** — `COUNT(*)` always returns ≥ 0; CLAUDE.md "Don't add error handling, fallbacks, or validation for scenarios that can't happen". (Note: P1 patch separately added `.max(0)` defensively while restructuring the helper, but the original concern is dismissed as a hypothetical.)
- **Blind: E2E `#recent-additions` toBeVisible** — 9-2's invariant: section is always present (with inline empty-state on zero titles). Test assertion is correct.
- **Blind: `>7<` whitespace fragility in render test** — passes today; failure would be loud.
- **Blind: `media_type` field unused** — kept for forward-compat with 9-7 (recent activity indicators); cost negligible.
- **Blind/Auditor: log emission test absent** — spec's "or an inspection comment" allowance honored; comment present in the tests.
- **Blind: `LIMIT 100` magic number** — cosmetic; would extract to constant only if a per_page param appears.
- **Blind: WARN logs `query=""` when only sort set** — cosmetic noise.
- **Auditor: AC7 sort-param scope creep** — defensive tightening, documented in completion notes.
- **Auditor: AC5 `!is_empty()` guard broader than spec** — documented in completion notes; suppresses spurious WARN on empty filter clear.
- **Edge: `slice_section` panics on hidden section** — informative panic message, not a runtime bug.
- **Edge: whitespace-only / NUL byte filter triggers WARN** — probe-traffic noise; closed enum protects from injection.
- **Edge: E2E session-sharing race** — Playwright's auto-wait on `toBeVisible` handles the post-navigation DOM update.
