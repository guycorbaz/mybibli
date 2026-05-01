# Story 9.3: Dashboard — stats by genre

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As any user (anonymous or authenticated),
I want to see the catalog distribution by genre on the home page,
so that I can understand the composition of the library at a glance.

## Acceptance Criteria

1. **AC1 — Section presence and ordering.** Given the home page (`/`), when it renders for any role (Anonymous, Librarian, Admin), then a "By genre" section is visible directly below the "Recent additions" section (story 9-2) and above the browse controls (`<!-- Browse toggle + sort -->`). The section sits OUTSIDE the `#browse-results` HTMX swap target so it survives search/filter swaps — same placement invariant as `#collection-glance` (9-1) and `#recent-additions` (9-2).
2. **AC2 — Row content.** Given the section is non-empty, when each row renders, then it displays exactly four pieces of information in this order: (a) genre name, (b) title count for that genre, (c) percentage of total active titles in that genre (rounded to 1 decimal), (d) a horizontal bar visually proportional to the percentage. Rows are sorted by `title_count DESC`, then `genres.name ASC` (locale-insensitive tiebreak — keeps order deterministic across runs).
3. **AC3 — Single round-trip aggregated query.** The query that drives the section is a single `SELECT … GROUP BY genres.id` round-trip joining `titles` to `genres` with `WHERE titles.deleted_at IS NULL AND genres.deleted_at IS NULL`. The total denominator (used for percentage) is computed Rust-side from the rows (`rows.iter().map(|r| r.title_count).sum()`) — NOT via a second SQL round-trip. The query lives at `src/services/dashboard.rs::stats_by_genre(pool) -> Result<Vec<GenreStat>, AppError>` where `GenreStat` is a small struct with the SQL-emitted fields (id, name, title_count). Percentage is computed in the route handler, not in SQL.
4. **AC4 — Empty state (no genres assigned).** Given the active catalog has zero titles (so zero genre assignments), when the section renders, then it is **hidden entirely** — `<section id="stats-by-genre">` is not emitted. AC1 ordering is preserved when present, but the section may be absent. This deviates from 9-1/9-2 (which always render their card with a 0-state) on purpose: the spec is explicit ("when no genres are assigned anywhere, the section is hidden entirely; the broader empty-catalog UX is handled by StatusMessage in 9.15"). When the StatusMessage component arrives in 9-15, that component will own the empty-catalog story for the page as a whole.
5. **AC5 — Soft-deleted exclusion (titles AND genres).** Given a genre `g` exists with `deleted_at IS NOT NULL`, when the section renders, then no row for `g` appears even if some titles still point at it (orphan FK left over from soft-deleted reference data). Given a title `t` has `deleted_at IS NOT NULL`, when its genre's count is computed, then `t` is NOT counted. The query JOINs with `genres.deleted_at IS NULL` AND filters `titles.deleted_at IS NULL` — both halves are load-bearing.
6. **AC6 — Genre row clickability + filter target.** Given a row, when clicked, then it navigates to `/?filter=genre:<id>` (the existing genre-filter URL pattern established in `src/routes/home.rs::parse_filter` at line ~340). **Spec note:** the literal text in `epics.md:1250` says `/catalog?genre=<id>`, but `/catalog` is the scan page (it does NOT support a `genre=` query param — verified in `src/routes/catalog.rs::CatalogTemplate`). The actual genre-filter route in v1 is `/?filter=genre:<id>` (home page with active filter). This is the same convention story 9-1 used when its "volume count → `/catalog?view=volumes`" deviation pointed to plain `/catalog`. Use a real `<a href>` (not HTMX `hx-get`) so the link is bookmarkable AND works without JS; HTMX boost on `<body>` (if/when added) will progressively enhance it.
7. **AC7 — Anonymous data leak guard.** Given the section is visible to anonymous users, when the query runs, then it SELECTs only public columns (id, name, count) — no role-gated columns are joined. The query is role-agnostic (no `if session.role …` SQL branch) and the rendered HTML for an anonymous request is byte-identical to the librarian/admin HTML for this section. **No** anonymous-leak regression guard is needed in the route test (unlike 9-1's `href="/loans"` check) because there is no role-gated link in this section — but the byte-identical invariant is asserted in the unit test by rendering once with `Anonymous` and once with `Librarian` and comparing the slice.
8. **AC8 — CSP compliance.** No `style="..."`, no `<style>`, no `<script>`, no `onclick=`, no inline event handlers. The horizontal bar uses semantic `<progress value="X" max="Y">` (HTML attributes, NOT inline CSS — CSP-clean and accessible by default). Custom styling comes from a class-driven rule in `static/css/browse.css` (or a new `static/css/dashboard.css` if browse.css is the wrong home) that uses `var(--color-indigo-500)` / `var(--color-stone-200)` tokens defined in `static/css/input.css` `@theme` block. The `src/templates_audit.rs::no_inline_markup_in_templates` test must continue to pass.
9. **AC9 — i18n EN + FR with locale-aware percentage formatting.**
   - Three new keys under the existing `dashboard:` section (after `recent_additions:`):
     - `dashboard.stats_by_genre.heading` — EN: "By genre", FR: "Par genre"
     - `dashboard.stats_by_genre.titles_one` — EN: "%{count} title", FR: "%{count} titre"
     - `dashboard.stats_by_genre.titles_other` — EN: "%{count} titles", FR: "%{count} titres"
   - The percentage is formatted **locale-aware** — EN: `"12.5%"` (period decimal separator, no space), FR: `"12,5 %"` (comma decimal separator, **non-breaking space** `U+00A0` before the `%` per French typographic convention). Implement as a small helper in `src/routes/home.rs` or `src/utils.rs::format_percent(value: f64, locale: &str) -> String`. **Do NOT** add a new dependency for locale-aware number formatting — a 4-line `if locale == "fr" { … }` branch is sufficient for the v1 EN/FR scope.
   - Pluralization uses the same `_one`/`_other` pattern as 9-1's glance card (literal `t!` keys, branched on `is_singular(loc, count)` — see Task 2 sub-bullet). After editing locale files, run `touch src/lib.rs && cargo build` to force the proc macro to re-read.
10. **AC10 — Unit tests.**
    - (a) `stats_by_genre` returns rows ordered by `title_count DESC, name ASC`, excluding both soft-deleted titles and soft-deleted genres — `tests/dashboard_stats_by_genre.rs::stats_by_genre_orders_and_excludes_soft_deleted` (DB-backed `#[sqlx::test]`).
    - (b) `stats_by_genre` on an empty DB returns `Ok(vec![])` (not an error) — `tests/dashboard_stats_by_genre.rs::stats_by_genre_on_empty_db_returns_empty_vec`.
    - (c) `stats_by_genre` with all titles in one genre returns one row whose count equals total active titles — same test file, scenario `stats_by_genre_single_genre_full_share`.
    - (d) Handler render test — populated case: build a HomeTemplate via `make_test_home_template_with_stats(role, vec_of_3_genre_stats)`, render, scope assertion to `#stats-by-genre` slice (`stats_by_genre_slice` — sibling of `recent_additions_slice` using the existing `slice_section` helper), assert: 3 rows present in input order, each row contains the genre name and the formatted percentage; the EN test asserts `"12.5%"` and the FR test asserts `"12,5\u{00A0}%"` (literal NBSP).
    - (e) Handler render test — empty case: with `vec![]`, assert: `id="stats-by-genre"` is **NOT** present in the rendered HTML (AC4 hide-entirely).
    - (f) Document-order regression test: `home_renders_recent_additions_above_stats_by_genre` — locks `#recent-additions` strictly before `#stats-by-genre` in the rendered output. Mirrors the 9-2 review-fix test `home_renders_glance_above_recent_additions`. Without this, a future template edit could swap the order silently.
    - (g) Anonymous parity test: render the populated case once with `role="anonymous"` and once with `role="librarian"`, slice both to `stats-by-genre`, `assert_eq!(slice_anon, slice_librarian)` — locks AC7.
    - (h) Percentage rounding edge cases: a unit test on `format_percent` covers `(1.0/3.0) * 100.0 = 33.333…` → EN `"33.3%"`, FR `"33,3\u{00A0}%"`; `100.0` → `"100.0%"` (we keep the trailing `.0` for visual alignment per 1-decimal mandate); `0.0` → `"0.0%"` (defensive — should never appear in rendered output because zero-count genres are excluded from the SQL result by the INNER JOIN, but the helper itself must not panic).
11. **AC11 — E2E smoke.** Extend `tests/e2e/specs/journeys/home.spec.ts` with one new `test.describe("Home page — Stats by genre section", ...)` block placed AFTER the existing `test.describe("Home page — Recent additions section", ...)` block. The single test (anonymous role, since AC7 is role-agnostic) does:
    - Navigate to `/`.
    - If `page.locator("#stats-by-genre").count() > 0`: assert the heading matches `/By genre|Par genre/i`; assert the first row contains both a number AND a percentage matching `/\d+(\.\d+)?\s?%/` (EN) or `/\d+(,\d+)?\s?%/` (FR — combined regex `/\d+([.,]\d+)?\s*%/` accepts both); click the first row and `await page.waitForURL(/\/\?filter=genre:\d+/)` (note: home page URL with filter query, NOT `/catalog?genre=`).
    - If 0 rows (empty DB): assert `await expect(page.locator("#stats-by-genre")).toHaveCount(0)` (AC4 hide-entirely). Use the same conditional pattern as `home.spec.ts:103-115` (Story 9-2's E2E).
    - Use i18n-aware regex matchers; no `waitForTimeout` (CI grep gate enforced by the `e2e` job). The new genre-row link selector `'#stats-by-genre a[href^="/?filter=genre:"]'` is **scoped to the section** — explicitly avoiding the deferred unscoped-selector follow-up filed during 9-2 (per `9-2-dashboard-recent-additions.md` Change Log 2026-04-30).

## Tasks / Subtasks

- [x] **Task 1 — Add `stats_by_genre()` to `services::dashboard` (AC: 1, 2, 3, 5, 7, 10a–c)**
  - [x] In `src/services/dashboard.rs`, add `pub struct GenreStat { pub id: u64, pub name: String, pub title_count: i64 }` deriving `Debug, Clone, sqlx::FromRow`.
  - [x] Add `pub async fn stats_by_genre(pool: &DbPool) -> Result<Vec<GenreStat>, AppError>`. SQL shape (single round-trip):
    ```sql
    SELECT g.id, g.name, COUNT(t.id) AS title_count
      FROM titles t
      JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL
     WHERE t.deleted_at IS NULL
     GROUP BY g.id, g.name
     ORDER BY title_count DESC, g.name ASC
    ```
    Use `sqlx::query_as::<_, GenreStat>` + `.fetch_all(pool)`. The INNER JOIN form (rather than `FROM genres g LEFT JOIN titles t`) means a genre with zero active titles is automatically excluded — AC4's "no genres assigned → section hidden" is therefore observed naturally by the query: an empty DB returns `Vec::new()`.
  - [x] **Do NOT use `sqlx::query!` macro** (forces `.sqlx/` cache regeneration in the PR — project convention is dynamic `query_as`, see `services::dashboard::collection_glance` and `models::title::list_recent_active`).
  - [x] Co-locate three `#[sqlx::test]` cases at the bottom of `src/services/dashboard.rs` OR in a new file `tests/dashboard_stats_by_genre.rs` (sibling). **Choose the sibling file** for parity with `tests/dashboard_glance.rs` (9-1) and `tests/dashboard_recent_additions.rs` (9-2) — discoverability by file name beats co-location. The three cases are AC10a/b/c.
  - [x] **Test data isolation** — the migration seed adds genres like "Roman", "BD", … so each test must use unique genre names (e.g. prefix with `Z-9-3-`) or rely on pre-seeded ones. The `tests/dashboard_glance.rs::first_genre_id` helper is reusable; clone or import its pattern (helpers can stay file-local — no shared `tests/common.rs` exists yet).
- [x] **Task 2 — Wire the handler (AC: 1, 4, 6, 9)**
  - [x] In `src/routes/home.rs::home`, after the `recent_additions = …` block (around line ~186), call `crate::services::dashboard::stats_by_genre(pool).await` with the **soft-degrade pattern** established by 9-1/9-2: `match … { Ok(v) => v, Err(e) => { tracing::warn!(error = %e, "stats_by_genre failed; rendering empty section"); Vec::new() } }`. The home page MUST NOT 500 because the dashboard query had a hiccup.
  - [x] Compute the total denominator and per-row percentage **in the handler**:
    ```rust
    let total: i64 = stats_rows.iter().map(|r| r.title_count).sum();
    let stats: Vec<StatsByGenreRow> = stats_rows.into_iter().map(|r| {
        let pct = if total > 0 {
            ((r.title_count as f64 / total as f64) * 1000.0).round() / 10.0
        } else { 0.0 };
        StatsByGenreRow {
            id: r.id,
            name: r.name,
            title_count: r.title_count,
            count_label: /* t!("dashboard.stats_by_genre.titles_{one|other}", count = …) */,
            percent_label: format_percent(pct, loc),
            // For the <progress> element:
            value: r.title_count,
            max: total,
        }
    }).collect();
    ```
    Define `StatsByGenreRow` as a small inner struct in `src/routes/home.rs` (or a sibling module if you prefer — `src/routes/home_dashboard.rs` is overkill for one struct; keep it in `home.rs`).
  - [x] Reuse the existing `is_singular(locale, count)` helper at `src/routes/home.rs:333-338` for `_one`/`_other` selection on `count_label`. Do NOT duplicate it.
  - [x] Implement `format_percent(value: f64, locale: &str) -> String` — preferred location is `src/utils.rs` (since it's a pure formatting helper with no DB / no AppState dependency, and `utils.rs` already houses `html_escape`, `url_encode`, `current_url` — same shape). Specification:
    ```rust
    pub fn format_percent(value: f64, locale: &str) -> String {
        let s = format!("{:.1}", value); // e.g., "33.3"
        match locale {
            "fr" => format!("{}\u{00A0}%", s.replace('.', ",")), // "33,3 %" (NBSP)
            _    => format!("{}%", s),                           // "33.3%"
        }
    }
    ```
    Add three unit tests in the same file covering: EN basic, FR with NBSP, the rounding edge cases listed in AC10h.
  - [x] Extend `HomeTemplate` (currently 53 fields post-9-2 at `src/routes/home.rs:31-85`) with **TWO** new fields:
    - `stats_by_genre: Vec<StatsByGenreRow>` — the rows; empty when section should hide
    - `stats_by_genre_heading: String` — pre-translated `dashboard.stats_by_genre.heading`
  - [x] Translate the heading via `rust_i18n::t!("dashboard.stats_by_genre.heading", locale = loc).to_string()`. Pre-translate `count_label` per row in the handler (NOT in the template) — same pattern as 9-1/9-2.
  - [x] Pass the new fields from the handler. Do NOT add a new route.
- [x] **Task 3 — Add i18n keys (AC: 9)**
  - [x] In `locales/en.yml`, under `dashboard:` after the `recent_additions:` block, add:
    ```yaml
      stats_by_genre:
        heading: By genre
        titles_one: "%{count} title"
        titles_other: "%{count} titles"
    ```
  - [x] In `locales/fr.yml`, mirror under the same path:
    ```yaml
      stats_by_genre:
        heading: Par genre
        titles_one: "%{count} titre"
        titles_other: "%{count} titres"
    ```
  - [x] **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper. Keys start at root level. The proc-macro reads filename → locale.
  - [x] After editing, run `touch src/lib.rs && cargo build` to force the i18n proc-macro to re-read YAML files. The `i18n::audit::tests::all_t_keys_have_both_locales` test (mentioned in 9-1 Debug Log) will fail if a key exists in only one of EN/FR — keep them perfectly mirrored.
- [x] **Task 4 — Render the section in the template (AC: 1, 2, 4, 6, 8)**
  - [x] In `templates/pages/home.html`, insert the new section AFTER the closing `</section>` of `#recent-additions` (currently line ~141) and BEFORE the `<!-- Browse toggle + sort -->` block (line ~143).
  - [x] Markup outline (Tailwind utility classes only; the bar uses `<progress>` with custom CSS for theming):
    ```jinja
    {# Stats by genre section (story 9-3). Sits between #recent-additions and
       #browse-results so it survives HTMX search swaps. Hidden entirely when
       the catalog has zero genre assignments (AC4) — empty-state UX moves to
       StatusMessage in story 9-15 for the page as a whole. #}
    {% if !stats_by_genre.is_empty() %}
    <section id="stats-by-genre" aria-labelledby="stats-by-genre-heading" class="w-full max-w-4xl mt-6">
        <h2 id="stats-by-genre-heading" class="text-sm font-medium text-stone-600 dark:text-stone-400 uppercase tracking-wide">{{ stats_by_genre_heading }}</h2>
        <ul class="mt-3 space-y-2">
            {% for row in stats_by_genre %}
            <li>
                <a href="/?filter=genre:{{ row.id }}" class="block px-4 py-3 bg-stone-50 dark:bg-stone-800 hover:bg-stone-100 dark:hover:bg-stone-700 rounded-lg border border-stone-200 dark:border-stone-700 transition-colors">
                    <div class="flex items-center justify-between gap-3">
                        <span class="font-medium text-stone-900 dark:text-stone-100 truncate">{{ row.name }}</span>
                        <span class="text-sm text-stone-600 dark:text-stone-400 whitespace-nowrap"><span class="tabular-nums">{{ row.count_label }}</span> · <span class="tabular-nums">{{ row.percent_label }}</span></span>
                    </div>
                    <progress class="genre-bar mt-2 w-full" value="{{ row.value }}" max="{{ row.max }}" aria-label="{{ row.percent_label }}">{{ row.percent_label }}</progress>
                </a>
            </li>
            {% endfor %}
        </ul>
    </section>
    {% endif %}
    ```
    Notes:
    - The `<progress>` element is the semantic, CSP-clean choice for a percentage visualization (`value`/`max` are HTML attributes, NOT inline `style`). The textual fallback (`>{{ row.percent_label }}</progress>`) is read by older browsers that don't render `<progress>` natively.
    - The `<a>` wraps the entire list-item so the click target is the full row (UX-DR8 / Fitts's-law). `<a>` + `<progress>` is valid HTML5.
    - `tabular-nums` keeps the count and percentage right-aligned visually as the rows scan.
  - [x] CSP discipline: zero `style="..."`, zero `onclick=`, zero inline `<script>`. Tailwind classes + the new `.genre-bar` class only.
  - [x] **Add styling for `<progress class="genre-bar">` in `static/css/browse.css`** (extending the file rather than creating a new one — keeps dashboard-related CSS co-located with the title-card rules that 9-2 also targets). Specification:
    ```css
    /* Story 9-3: Stats-by-genre horizontal bar. <progress> uses cross-browser
       pseudo-elements; values come from the @theme tokens defined in
       static/css/input.css (UX-DR24). */
    progress.genre-bar {
        appearance: none;
        -webkit-appearance: none;
        height: 6px;
        border: none;
        border-radius: 3px;
        background-color: var(--color-stone-200);
    }
    progress.genre-bar::-webkit-progress-bar {
        background-color: var(--color-stone-200);
        border-radius: 3px;
    }
    progress.genre-bar::-webkit-progress-value {
        background-color: var(--color-indigo-500);
        border-radius: 3px;
    }
    progress.genre-bar::-moz-progress-bar {
        background-color: var(--color-indigo-500);
        border-radius: 3px;
    }
    .dark progress.genre-bar,
    .dark progress.genre-bar::-webkit-progress-bar {
        background-color: var(--color-stone-700);
    }
    .dark progress.genre-bar::-webkit-progress-value,
    .dark progress.genre-bar::-moz-progress-bar {
        background-color: var(--color-indigo-400);
    }
    ```
    These use the `@theme` tokens already defined in `static/css/input.css:5-26` — no hardcoded hex values, satisfying AC8's "no hardcoded colors" clause.
  - [x] **Tailwind v4 build awareness** — the project uses `@tailwindcss/cli` (see `package.json`); the `output.css` is regenerated when CSS changes. Run `npx @tailwindcss/cli -i static/css/input.css -o static/css/output.css` (or whatever the project's npm script is) after editing `browse.css`. Verify `static/css/output.css` actually includes the new `.genre-bar` rules — Tailwind v4 should pass through unrecognized custom CSS verbatim, but confirm.
- [x] **Task 5 — Unit tests (AC: 7, 10)**
  - [x] Create `tests/dashboard_stats_by_genre.rs` (sibling, not `mod` inside `dashboard_glance.rs` or `dashboard_recent_additions.rs`). Three `#[sqlx::test(migrations = "./migrations")]` cases (AC10a/b/c). Pattern from `tests/dashboard_recent_additions.rs:1-30` for the imports + `MySqlPool` plumbing.
  - [x] Helpers needed (file-local — same approach as 9-1/9-2 — no shared `tests/common.rs` yet):
    ```rust
    async fn insert_genre(pool: &MySqlPool, name: &str) -> u64 { … }
    async fn insert_title_in_genre(pool: &MySqlPool, title: &str, genre_id: u64) -> u64 { … }
    async fn soft_delete(pool: &MySqlPool, table: &str, id: u64) { … } // dup from dashboard_glance.rs is fine
    ```
    Genre names must be unique within each test (e.g. `Z-9-3-Foo`, `Z-9-3-Bar`) to avoid colliding with seed migrations or with sibling tests running in parallel via the dedicated test DB.
  - [x] Test scenarios:
    - `stats_by_genre_on_empty_db_returns_empty_vec` — fresh schema, no fixtures → `Vec::new()`.
    - `stats_by_genre_orders_and_excludes_soft_deleted` — seed: 3 active titles in `Z-9-3-A`, 2 active in `Z-9-3-B`, 1 active + 2 soft-deleted in `Z-9-3-C`, 5 active titles in **soft-deleted** genre `Z-9-3-D` (orphans on FK) → expect rows for A (3), B (2), C (1) only, in that order; D is excluded. The orphan-on-soft-deleted-genre case is the critical AC5 invariant.
    - `stats_by_genre_single_genre_full_share` — seed: 4 active titles all in `Z-9-3-Single` → expect a single row with `title_count = 4`. Handler-side computation in 10d covers the corresponding 100.0% percentage.
  - [x] Add **6 handler render tests** in `src/routes/home.rs::mod tests` (AC10d/e/f/g — paired):
    - `home_renders_stats_by_genre_with_three_rows` — populated case (3 rows), EN locale; assert section present, 3 `<li>` rows, each contains genre name + percentage. Asserts the EN format `"33.3%"`.
    - `home_renders_stats_by_genre_empty_section_hidden` — empty case (`vec![]`); assert `id="stats-by-genre"` is NOT in the rendered HTML.
    - `home_renders_recent_additions_above_stats_by_genre` — populated case; locks document order between `#recent-additions` and `#stats-by-genre`. Mirror the 9-2 `home_renders_glance_above_recent_additions` test.
    - `home_stats_by_genre_byte_identical_for_anonymous_and_librarian` — render twice with same `stats_by_genre` payload, role differs; slice both to `stats-by-genre`, `assert_eq!(slice_anon, slice_librarian)`. AC7.
    - `home_renders_stats_by_genre_french_uses_nbsp_and_comma` — same data as the EN test but with `lang = "fr"` and `count_label`/`percent_label` pre-formatted via the FR variant; assert the slice contains `"33,3\u{00A0}%"` and does NOT contain `"33.3%"`. AC9 NBSP invariant.
  - [x] Add **factories** to `mod tests`:
    - `make_test_home_template_with_stats(role: &str, stats: Vec<StatsByGenreRow>) -> HomeTemplate` — sibling of `make_test_home_template_with_recent`, delegates to `make_test_home_template_with_counts`.
    - `fake_genre_stat_row(id: u64, name: &str, count_label: &str, percent_label: &str, value: i64, max: i64) -> StatsByGenreRow` — deterministic row for assertion-friendly tests.
  - [x] Add **slice helper** sibling: `fn stats_by_genre_slice(html: &str) -> &str { slice_section(html, "stats-by-genre") }` next to `recent_additions_slice` at line ~649. Reuse the existing `slice_section` helper unchanged.
  - [x] Add **`format_percent` unit tests** in `src/utils.rs` (or wherever the helper lands):
    - `format_percent_en_basic` — `format_percent(33.3, "en") == "33.3%"`
    - `format_percent_fr_basic` — `format_percent(33.3, "fr") == "33,3\u{00A0}%"`
    - `format_percent_fr_uses_nbsp` — assert the byte at `s.len() - 2` is `\u{00A0}` (NBSP), not a regular space `\u{0020}`. NBSP is critical for French typography and would silently degrade if a future refactor changed it.
    - `format_percent_one_decimal_kept` — `format_percent(100.0, "en") == "100.0%"` (we keep `.0` for visual alignment).
    - `format_percent_zero` — defensive (zero-count genres are excluded by SQL but the helper must not panic).
- [x] **Task 6 — E2E spec (AC: 11)**
  - [x] Extend `tests/e2e/specs/journeys/home.spec.ts`. Place a new `test.describe("Home page — Stats by genre section", ...)` block AFTER the existing `test.describe("Home page — Recent additions section", ...)` block (after line 117 in the current file). One anonymous test:
    ```ts
    test.describe("Home page — Stats by genre section", () => {
      test("anonymous: section visible, first row navigates to /?filter=genre:<id> (or section hidden)", async ({
        page,
      }) => {
        await page.goto("/");
        const section = page.locator("#stats-by-genre");
        const sectionCount = await section.count();
        if (sectionCount === 0) {
          // AC4 — fresh catalog, section is hidden entirely. Done.
          return;
        }
        await expect(section).toBeVisible();
        await expect(section.getByRole("heading", { level: 2 })).toContainText(
          /By genre|Par genre/i,
        );
        const rows = section.locator("li");
        await expect(rows.first()).toContainText(/\d+([.,]\d+)?\s*%/);
        // Scope the link selector to the section to avoid the deferred unscoped-
        // selector flake (per 9-2 Change Log 2026-04-30).
        const firstLink = section.locator('a[href^="/?filter=genre:"]').first();
        await firstLink.click();
        await page.waitForURL(/\/\?filter=genre:\d+/);
      });
    });
    ```
  - [x] Use i18n-aware regex matchers consistently. Do NOT add `waitForTimeout` (CI grep gate).
  - [x] **No login required** — AC7 says role-agnostic. A single anonymous test is sufficient.
- [x] **Task 7 — Verify and document (AC: 1–11)**
  - [x] `cargo check && cargo clippy --all-targets -- -D warnings` (zero warnings policy, Foundation Rule).
  - [x] `cargo test --lib` — full unit + co-located integration suite. Expected count: ~631 (post-9-2) + 5 new render tests in `home.rs` + 5 new `format_percent` tests = ~641 lib tests. Plus the new `tests/dashboard_stats_by_genre.rs` integration tests run separately.
  - [x] `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test --test dashboard_stats_by_genre` — 3 new integration tests pass.
  - [x] `cargo sqlx prepare --check --workspace -- --all-targets` — expected no diff (Task 1 uses dynamic `query_as`).
  - [x] **Tailwind build:** verify `static/css/output.css` regenerates and contains the new `.genre-bar` rules. The exact npm command is in `package.json`'s scripts (or run `npx @tailwindcss/cli -i static/css/input.css -o static/css/output.css`).
  - [x] **Manual smoke** (from a running dev instance — `MYBIBLI_SKIP_SETUP=1 cargo run`):
    - `curl http://localhost:8080/` and grep for `id="stats-by-genre"`, `Par genre`, `<progress class="genre-bar"`.
    - Verify in a browser: dark-mode toggle keeps the bar legible; clicking a row navigates to `/?filter=genre:<id>` and the search results show only that genre.
  - [x] **E2E** (Foundation Rule #13 — local before push): `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/home.spec.ts`. If the local `tests/e2e/test-results/` is owned by root (the recurring 9-1 / 9-2 blocker), document the skip in Dev Agent Record and rely on CI.
  - [x] Update Dev Agent Record at the bottom of this file: list of files touched, decisions on `<progress>` styling location, anything surprising.
  - [x] Update `_bmad-output/implementation-artifacts/sprint-status.yaml`: flip `9-3-dashboard-stats-by-genre: backlog → ready-for-dev` (already done by the create-story step) → `in-progress` at start of dev-story → `review` at end. Update only this line + `last_updated` (CLAUDE.md rule 16).

## Dev Notes

### Source tree references

The dev agent should not need to reinvent any of these. All paths relative to repo root.

| Concern | File / location | Notes |
|---|---|---|
| Home route registration | `src/routes/mod.rs:87` | `.route("/", axum::routing::get(home::home))` — no change needed |
| Home handler | `src/routes/home.rs:87-326` | extend the existing handler; do NOT create a parallel route |
| Home template | `templates/pages/home.html` (240 lines post-9-2) | extends `layouts/base.html`; section blocks: search (14-33), filter tags (36-65), metadata error badge (67-74), `#collection-glance` (76-104), `#recent-additions` (106-141), browse toggle (143-169), `#browse-results` (171-237) |
| Insertion point for the new section | `templates/pages/home.html` between line ~141 (closing `</section>` of `#recent-additions`) and line ~143 (`<!-- Browse toggle + sort -->`) | preserves AC1 ordering invariant: glance → recent → stats → browse |
| Service module | `src/services/dashboard.rs` (created in 9-1) | extend with `GenreStat` + `stats_by_genre()`; `pub mod` already registered in `src/services/mod.rs` |
| Existing service pattern | `src/services/dashboard.rs::collection_glance` (lines 37-50) | single round-trip query with `sqlx::query_as` — mirror exactly |
| Existing genre model | `src/models/genre.rs` | `GenreModel::list_active`, `find_name_by_id`, `count_usage` — useful background but NOT called by Task 1 (the new query JOINs directly) |
| Title schema reminder | `migrations/*.sql` `CREATE TABLE titles` | `genre_id BIGINT UNSIGNED NOT NULL` + FK to `genres(id)` — every active title has exactly one genre. The spec's "title with no genre — NOT counted" clause is therefore a forward-compat hedge; the current schema makes it impossible. Keep the JOIN form as specified — if a future migration relaxes the FK, the query stays correct. |
| Soft-delete invariant | `src/services/dashboard.rs:37-49`, `src/models/title.rs:837-855` | every entity SELECT/JOIN must include `deleted_at IS NULL` — both halves of AC5 |
| `is_singular` helper for plural i18n | `src/routes/home.rs:333-338` | reuse for `count_label`; do NOT duplicate |
| Pre-translation pattern (handler-side `t!`) | `src/routes/home.rs:303-320` (glance heading + recent_additions heading) | replicate for `stats_by_genre_heading` |
| Soft-degrade on DB error pattern | `src/routes/home.rs:167-186` (glance + recent_additions) | replicate for stats_by_genre — same `match … Ok(v) / Err(e) → tracing::warn! + Vec::new()` shape |
| HomeTemplate struct | `src/routes/home.rs:31-85` (53 fields post-9-2) | extend with `stats_by_genre: Vec<StatsByGenreRow>`, `stats_by_genre_heading: String` |
| Test factory & slice helpers | `src/routes/home.rs:628-651` (`slice_section`, `glance_card_slice`, `recent_additions_slice`); `src/routes/home.rs:653-745` (`make_test_home_template_with_counts`, `make_test_home_template_with_recent`, `fake_search_result`) | reuse + extend (no rewrite) |
| i18n locales | `locales/en.yml:342-355`, `locales/fr.yml:342-355` (`dashboard:` block) | append `stats_by_genre:` sub-block after `recent_additions:` |
| i18n audit (forces both locales to mirror) | `src/i18n/audit.rs::all_t_keys_have_both_locales` | `cargo test` enforces — keep EN + FR keys aligned exactly |
| Templates audit (CSP enforcement) | `src/templates_audit.rs::no_inline_markup_in_templates` | must stay green |
| Filter parsing (existing genre filter) | `src/routes/home.rs:340-352` (`parse_filter`) | the destination of the row link — `?filter=genre:<id>` |
| `SearchService::search` (handles the filtered request when user clicks a row) | `src/services/search.rs::search` (lines 75-187) | already handles `genre_id` filter — no change needed; the row link drives the existing pipeline |
| Test pattern (DB-backed integration) | `tests/dashboard_glance.rs` (story 9-1) and `tests/dashboard_recent_additions.rs` (story 9-2) | `#[sqlx::test(migrations = "./migrations")]` — file-local helpers; no shared `tests/common.rs` |
| Test pattern (handler render, no DB) | `src/routes/home.rs::mod tests` (lines 612-...; the post-9-2 file has factories `make_test_home_template_with_*`) | reuse the slice + factory pattern verbatim |
| E2E spec for `/` | `tests/e2e/specs/journeys/home.spec.ts:1-117` | extend with the new describe block AFTER 9-2's block |
| E2E i18n-aware regex pattern | `tests/e2e/specs/journeys/home.spec.ts:36, 99` | matches both EN and FR — replicate with `/By genre|Par genre/i` |
| CSS file | `static/css/browse.css` (existing) + `static/css/input.css` (`@theme` tokens) | extend `browse.css` with `.genre-bar` rules; use `var(--color-*)` from the `@theme` block — NO hardcoded hex values |
| Tailwind build | `package.json`'s scripts + `static/css/output.css` | regenerate after CSS changes |

### Anti-patterns to avoid

- **Two SQL round-trips for count + total.** AC3 explicitly mandates one round-trip. Sum the rows Rust-side; do NOT issue a separate `SELECT COUNT(*) FROM titles …` query. (The spec's "active titles with at least one genre" denominator simplifies to "active titles" given the current `genre_id NOT NULL` FK — keep the Rust-side sum form for forward-compat with a hypothetical NULL-able future.)
- **Inline `style="width: ...%"` on the bar.** This is the obvious-but-wrong tactic for variable-width visualizations. CSP blocks it (`src/middleware/csp.rs` strict directive — no `unsafe-inline`). Use `<progress value max>` (HTML attributes are NOT inline CSS) or pre-bucketed Tailwind classes. The story specifies `<progress>`.
- **Linking the row to `/catalog?genre=<id>`.** The spec text says this; the actual route doesn't exist — `/catalog` is the scan page in v1. The home page `/?filter=genre:<id>` is the canonical genre-filter URL. Same convention as 9-1's "volume count → `/catalog`" deviation. Documented in AC6.
- **Computing percentage in SQL.** The handler is the right layer for the rounding + locale formatting. Keeping SQL strictly aggregational (count, group, order) makes the function reusable for future stories that might want raw counts (e.g., a /admin/health stats panel). Single-responsibility for `stats_by_genre`: return rows, sorted, with counts.
- **Empty-state hiding via JS / CSS.** AC4 says hide via SERVER-side `{% if %}`. JS-driven hide would briefly flash the empty section and re-trigger HTMX swap recalculation. Server-side `{% if %}` is also CSP-trivial.
- **Calling `t!()` from inside the Askama template** for the count or heading labels. Project convention is "translate in handler, pass to template" (canonical example: `src/routes/home.rs:303-320`).
- **Role-branched SQL.** AC7 mandates byte-identical HTML for anonymous and librarian. The query is role-agnostic. If a future story needs role-gated columns (e.g., "private genres for admin only"), branch in the handler, not in SQL — keep the model layer pure.
- **Hardcoded colors in `browse.css` (or anywhere CSS).** AC8 mandates token-based — `var(--color-indigo-500)` not `#6366f1`. UX-DR24 compliance.
- **Regular space `\u{0020}` instead of NBSP `\u{00A0}` in the FR percentage format.** French typography requires NBSP between number and `%` (and similarly for `:`, `;`, `?`, `!`). A regular space allows a line break between the number and the unit, which is wrong typographically and visually awful when a row wraps. The unit test `format_percent_fr_uses_nbsp` is the regression guard.
- **Adding an unscoped `a[href^="/?filter="]` selector in the E2E.** Genre-filter pills already exist in `templates/pages/home.html:36-65` and use the same href pattern. An unscoped selector would match both, and `.first()` would be ambiguous. Always scope to `#stats-by-genre`. (This is the same flake class flagged in 9-2 Change Log 2026-04-30 for `a[href^="/title/"]`.)
- **Zero-count rows in the rendered output.** The INNER JOIN form excludes them automatically. If you accidentally write `LEFT JOIN`, `genres` with no titles will appear as `0 titles, 0%` rows — wrong per AC2 (sorted by count desc, so they'd cluster at the bottom polluting the visualization).

### Architecture compliance

- **Error handling:** Any DB failure in `stats_by_genre` returns `AppError::Database` via `?`; the handler soft-degrades with `tracing::warn!` + empty `Vec` (per 9-1 / 9-2 patterns). No new `AppError` variant.
- **Logging:** `tracing::warn!` at handler level on the soft-degrade path; `tracing::debug!` only inside the service function if needed. Counts are not interesting at info-level.
- **DB query discipline:** `WHERE deleted_at IS NULL` on every SELECT/JOIN of entity tables (titles, genres). MariaDB type gotchas not relevant — `COUNT(t.id)` returns `BIGINT` mapped to Rust `i64` automatically by sqlx.
- **HTMX coexistence:** The new section sits OUTSIDE `#browse-results` (HTMX swap target) — same invariant as 9-1 and 9-2. Verify by inspecting `home.html` after insert.
- **CSP middleware:** Already wraps the handler outermost. No work needed in middleware. The `<progress>` element + class-driven CSS is fully CSP-clean.
- **Pool access:** The handler already has `state.pool: DbPool`. Do not introduce a new connection.
- **One-branch-one-story (Foundation Rule #14):** Verify `git status` clean + on `main` + `git pull --ff-only` before starting; cut `story/9-3-dashboard-stats-by-genre`. Open a draft PR (Rule #15) at the first commit.
- **Source-file-size limit (Foundation Rule #12):** `src/routes/home.rs` is ~870 lines post-9-2. This story adds ~30-50 lines (handler block + 2 template fields + 5 render tests). Comfortable headroom to 2000.

### Library / framework requirements

- **Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + Tailwind CSS v4 + HTMX 2.0** — no version changes, no new dependencies. If you reach for a number-formatting crate (`num-format`, `unicode-segmentation`, etc.) — stop. The 4-line `if locale == "fr" { … }` branch is sufficient for v1 EN/FR scope.
- **rust_i18n** — already wired. Pre-translate `stats_by_genre_heading` and `count_label` per row in the handler via `rust_i18n::t!(…).to_string()` + the existing `is_singular` helper. Pass as `String` fields. Canonical example: `src/routes/home.rs:303-320`.
- **`<progress>` element semantics** — supported in all evergreen browsers. Cross-browser styling uses `appearance: none` + `::-webkit-progress-bar` / `::-webkit-progress-value` / `::-moz-progress-bar` pseudo-elements (already specified in Task 4 CSS). No JS, no polyfill.

### File structure requirements

| File | Action | Rough size |
|---|---|---|
| `src/services/dashboard.rs` | **edit** | +~25 lines (`GenreStat` struct + `stats_by_genre` fn) |
| `src/utils.rs` | **edit** | +~15 lines (`format_percent` helper + 5 unit tests) |
| `src/routes/home.rs` | **edit** | +~40 lines in handler + 2 new `HomeTemplate` fields + 1 inner struct (`StatsByGenreRow`) + 6 new render tests + factories |
| `templates/pages/home.html` | **edit** | +~25 lines for the section (wrapped in `{% if !empty %}`) |
| `static/css/browse.css` | **edit** | +~30 lines (`.genre-bar` rules, light + dark, cross-browser) |
| `static/css/output.css` | **regenerate** | (build artefact — regenerate via Tailwind CLI after browse.css edit) |
| `locales/en.yml` | **edit** | +~4 lines under `dashboard.stats_by_genre:` |
| `locales/fr.yml` | **edit** | +~4 lines under `dashboard.stats_by_genre:` |
| `tests/dashboard_stats_by_genre.rs` | **create** | ~80 lines (3 `#[sqlx::test]` cases + helpers) |
| `tests/e2e/specs/journeys/home.spec.ts` | **edit** | +~25 lines (1 new test case in a new `describe` block) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **edit** | only the `9-3-...` line + `last_updated` (CLAUDE.md rule 16) |
| `_bmad-output/implementation-artifacts/9-3-dashboard-stats-by-genre.md` | **edit** | this file — Status, Tasks checked, Dev Agent Record on completion |

### Testing requirements

- **Coverage target:** every AC has at least one test (unit OR E2E). AC8 (CSP) is covered by `templates_audit.rs::no_inline_markup_in_templates` (existing — must stay green).
- **AC4 hide-entirely** is the load-bearing visual invariant: `home_renders_stats_by_genre_empty_section_hidden` is the regression guard. A future template edit that wraps `#stats-by-genre` in `{% if true %}` would slip past CI without this test.
- **AC5 soft-deleted exclusion (both halves)** is the load-bearing data invariant: `stats_by_genre_orders_and_excludes_soft_deleted` covers it. Without the orphan-on-soft-deleted-genre fixture, a future migration that drops the `JOIN ... AND g.deleted_at IS NULL` half would silently leak orphans.
- **AC6 link target** is verified by the E2E (`waitForURL(/\/\?filter=genre:\d+/)`) — the URL pattern lock is what guarantees the route compatibility with `parse_filter`.
- **AC9 NBSP** is the load-bearing typography invariant: `format_percent_fr_uses_nbsp` (byte-level assertion on `\u{00A0}`) prevents accidental degradation to a regular space.
- **AC10f document-order** is the load-bearing layout invariant: `home_renders_recent_additions_above_stats_by_genre` prevents the same kind of cross-section ordering regression that 9-2's review caught (`home_renders_glance_above_recent_additions`).
- **E2E** keeps to 1 anonymous test for parsimony — the section is role-agnostic per AC7; a librarian variant would only re-test what the unit tests already cover.

### Project structure notes

This story aligns cleanly with the existing structure. Three intentional decisions worth flagging:

1. **`<progress>` over a custom div+span bar.** `<progress>` is the semantic, accessible, CSP-clean primitive for "X out of Y" visualizations. Browsers announce it correctly to screen readers (UX-DR23 / accessibility). Custom CSS via pseudo-elements is fully CSP-compatible. The alternative (discretized Tailwind width buckets) is more code for less semantics.

2. **`format_percent` lives in `src/utils.rs`, not in `src/routes/home.rs`.** The utility shape (pure function, no state, no async) matches the existing `html_escape` / `url_encode` / `current_url` neighbors. If a future story (e.g., 9-7 indicator percentages, 9-22 a11y audit metrics) needs locale-aware percentage formatting, it gets the helper for free without an import dance.

3. **`StatsByGenreRow` lives in `src/routes/home.rs`, not in a new module.** It is a presentation-layer struct that pairs SQL output with pre-translated labels — coupled to `HomeTemplate`. Splitting it out would add an indirection without callers. If a future story extracts the dashboard rendering into its own module (`src/routes/dashboard.rs`?), this struct moves with the rest.

The deferred TitleCard partial extraction (filed by 9-2) is **NOT** revisited here. The genre-row markup is structurally different (a full-width row with a bar, no cover) so it does not duplicate the TitleCard. No new follow-up issues planned for this story unless review uncovers something.

## References

- [Story 9.3 spec — `_bmad-output/planning-artifacts/epics.md` lines 1242–1257](../planning-artifacts/epics.md)
- [Epic 9 scope note + split philosophy — `epics.md` lines 1200–1206](../planning-artifacts/epics.md)
- [PRD FR57 — `_bmad-output/planning-artifacts/prd.md`](../planning-artifacts/prd.md) (search for `FR57`)
- [UX-DR24 design tokens — `_bmad-output/planning-artifacts/epics.md:242` + `_bmad-output/planning-artifacts/ux-design-specification.md:403, 2654`](../planning-artifacts/ux-design-specification.md)
- [Story 9-1 spec (canonical patterns: handler-side i18n, single round-trip, soft-degrade, slice helper for scoped tests, `is_singular` locale-aware plural helper) — `9-1-dashboard-global-stats-card.md`](./9-1-dashboard-global-stats-card.md)
- [Story 9-2 spec (canonical patterns: empty-state inline rendering, `slice_section` helper, document-order test, `fake_search_result` factory, sibling test file, deferred unscoped-selector flake class) — `9-2-dashboard-recent-additions.md`](./9-2-dashboard-recent-additions.md)
- [`CLAUDE.md` — Foundation Rules (#1 DRY, #7 E2E smoke per epic, #11 GitHub Issues, #12 ≤ 2000 LOC, #13 local testing, #14 one-branch-one-story, #15 draft PR, #16 sprint-status ownership, #18 CI gating)](../../CLAUDE.md)
- [Tailwind v4 `@theme` tokens — `static/css/input.css:1-76`](../../static/css/input.css)
- [Existing genre filter URL pattern — `src/routes/home.rs:340-352` (`parse_filter`)](../../src/routes/home.rs)

## Dev Agent Record

### Agent Model Used

`claude-opus-4-7` (1M context).

### Debug Log References

- `SQLX_OFFLINE=true cargo check --all-targets` — clean.
- `SQLX_OFFLINE=true cargo clippy --all-targets -- -D warnings` — clean (zero warnings).
- `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test` — full suite green:
  - **643 lib tests** (was 631 pre-9-3, +12: 7 `format_percent` + 5 `stats_by_genre` handler render tests).
  - `tests/dashboard_stats_by_genre.rs` — 3/3 integration tests passing (empty DB, ordering + soft-delete exclusion both halves, single-genre full-share).
  - All other integration suites (dashboard_glance 9-1, dashboard_recent_additions 9-2, etc.) unchanged.
- `cargo sqlx prepare --check` not run cleanly locally (DB-credential mismatch on pre-existing `query!` macros in `models/session.rs` — same blocker 9-1/9-2 hit). However: `git status .sqlx/` shows zero diff, confirming Task 1's dynamic `query_as` adds no new compile-time-checked queries.
- Tailwind rebuild via `npx @tailwindcss/cli -i static/css/input.css -o static/css/output.css --minify` — no diff in `output.css` (all utility classes I used were already present from other templates: `tabular-nums`, `whitespace-nowrap`, `truncate`, `mt-3`, `space-y-2`, `gap-3`, etc., verified by grep).
- `templates_audit::no_inline_markup_in_templates` — green. Zero inline `style=`, zero `<style>`, zero inline scripts in the new section.
- Manual `grep -rE 'waitForTimeout\(' tests/e2e/specs/ tests/e2e/helpers/` — clean (no new violations of the CI grep gate).
- E2E run not executed locally (same `tests/e2e/test-results/` root-ownership blocker 9-1/9-2 documented). CI on the story branch validates `home.spec.ts`'s new "Stats by genre section" describe block.

### Completion Notes List

- **Single round-trip query (AC3)** — `services::dashboard::stats_by_genre` returns `Vec<GenreStat>` from one `SELECT … GROUP BY` with INNER JOIN. Total denominator computed Rust-side via `rows.iter().map(|r| r.title_count).sum()`. No second query.
- **INNER JOIN naturally satisfies AC4 + AC5** — empty / soft-deleted genres can't appear in the result. `wipe_seeded_genres` helper in the integration test gives each `#[sqlx::test]` a hermetic baseline (the seed migration's "Roman" / "BD" rows would otherwise muddy assertions about exact row counts).
- **Row link target is `/?filter=genre:<id>`** (not the spec literal `/catalog?genre=<id>` — `/catalog` is the scan page in v1, doesn't accept that param). Documented in AC6 + Anti-Patterns; same convention as 9-1's `/catalog?view=volumes` → `/catalog` deviation. The link drives the existing `parse_filter` → `SearchService::search` pipeline unchanged.
- **`<progress value max>` for the variable-width bar** — semantic, accessible, CSP-clean (HTML attributes, not inline `style`). Custom CSS via `::-webkit-progress-bar`/`::-webkit-progress-value`/`::-moz-progress-bar` pseudo-elements in `static/css/browse.css`, all colors via `var(--color-*)` tokens from `static/css/input.css` `@theme` block (UX-DR24 — no hardcoded hex).
- **`format_percent` lives in `src/utils.rs`** — pure function, no state, no async. 7 unit tests including a byte-level NBSP regression guard (`format_percent_fr_uses_nbsp`) that asserts the bytes `0xC2 0xA0` immediately before `%` and rejects a regular `0x20` space — load-bearing for FR typography.
- **`build_stats_by_genre_rows` is a pure helper at module scope** in `home.rs`, not inlined in the handler. Keeps `home::home` readable and gives the test factory a clean injection seam (the render tests pre-construct `StatsByGenreRow` values directly without going through the SQL → `GenreStat` → row pipeline).
- **`StatsByGenreRow` is module-public** (in `routes/home.rs`) — needed by the test module for the `make_test_home_template_with_stats` factory. Adjacent precedent: `HomeTemplate` itself is `pub struct`. Visibility kept minimal: `pub` on the struct + fields, no derives beyond what Askama needs implicitly.
- **5 handler render tests + 7 helper tests = 12 new lib tests.** Coverage:
  - `home_renders_stats_by_genre_with_three_rows` — populated case, 3 rows, in input order, with name/count/percent/link assertions scoped to the section slice.
  - `home_renders_stats_by_genre_empty_section_hidden` — AC4 lock-in: empty Vec → no `id="stats-by-genre"` in the HTML.
  - `home_renders_recent_additions_above_stats_by_genre` — AC10f document-order lock; mirrors 9-2's review-fix `home_renders_glance_above_recent_additions`.
  - `home_stats_by_genre_byte_identical_for_anonymous_and_librarian` — AC7 anonymous parity; `assert_eq!` between the two slice strings.
  - `home_renders_stats_by_genre_french_uses_nbsp_and_comma` — FR percent labels survive the template render verbatim, EN form does not leak.
- **Initial template-placement bug caught + fixed in dev** — first edit inserted `#stats-by-genre` BEFORE `#recent-additions` (violates AC1: section must be directly below recent-additions). Re-positioned to between recent-additions and the browse toggle. The new `home_renders_recent_additions_above_stats_by_genre` test would have caught this — exactly the regression class it was designed to lock in.
- **No new GH Issues filed.** No deferred findings from this story; the previously deferred TitleCard partial extraction (filed by 9-2 review) is unaffected — the genre-row markup is structurally different (no cover, full-width row, `<progress>` bar) and does NOT duplicate TitleCard.

### File List

| File | Action |
|---|---|
| `src/services/dashboard.rs` | edit — added `GenreStat` struct + `stats_by_genre()` async fn (single GROUP BY round-trip) |
| `src/utils.rs` | edit — added `format_percent(value, locale)` + 7 unit tests including byte-level NBSP guard |
| `src/routes/home.rs` | edit — extended `HomeTemplate` with `stats_by_genre` + `stats_by_genre_heading` fields, added `StatsByGenreRow` struct, added `build_stats_by_genre_rows` helper, added handler block (single round-trip + soft-degrade + label computation), added `stats_by_genre_slice` helper, added `make_test_home_template_with_stats` + `fake_genre_stat_row` factories, added 5 new render tests |
| `templates/pages/home.html` | edit — inserted `#stats-by-genre` section between `#recent-additions` and the browse toggle (AC1 placement); rendered as `<ul>` with `<li><a><progress></a></li>` rows; wrapped in `{% if !stats_by_genre.is_empty() %}` for AC4 hide-entirely |
| `static/css/browse.css` | edit — appended `.genre-bar` rules for `<progress>` (cross-browser pseudo-elements, light + dark, all `@theme` token-based, zero hardcoded hex) |
| `locales/en.yml` | edit — appended `dashboard.stats_by_genre.{heading, titles_one, titles_other}` |
| `locales/fr.yml` | edit — same path, FR variants |
| `tests/dashboard_stats_by_genre.rs` | create — 3 `#[sqlx::test]` cases + file-local helpers (`insert_genre`, `insert_title_in_genre`, `soft_delete`, `wipe_seeded_genres`) |
| `tests/e2e/specs/journeys/home.spec.ts` | edit — appended `test.describe("Home page — Stats by genre section", …)` block with one anonymous test handling both populated and hidden-section branches; selector scoped to `#stats-by-genre` to sidestep the deferred unscoped-selector flake class flagged by 9-2 |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | edit — `9-3-dashboard-stats-by-genre`: `ready-for-dev → in-progress → review` (per CLAUDE.md rule 16; only this line + `last_updated`) |
| `_bmad-output/implementation-artifacts/9-3-dashboard-stats-by-genre.md` | edit — Status `ready-for-dev → review`, all Tasks checked, Dev Agent Record filled |

### Change Log

- **2026-05-01** — Initial implementation. All 7 tasks complete; 643 lib tests + 3 dashboard_stats_by_genre integration tests pass; clippy clean; sqlx cache unchanged. Followed Story 9-1 / 9-2 patterns (handler-side i18n, soft-degrade on DB error, single round-trip query, scoped HTML assertions, sibling integration test file). One spec drift caught in template draft (AC1 ordering — `#stats-by-genre` was placed BEFORE `#recent-additions` in the first pass; re-ordered to satisfy AC1's "directly below recent-additions" placement). E2E run deferred to CI per story 9-1 / 9-2 precedent (`tests/e2e/test-results/` ownership blocker).
