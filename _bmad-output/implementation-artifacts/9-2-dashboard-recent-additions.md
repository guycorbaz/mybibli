# Story 9.2: Dashboard — recent additions

Status: done

## Story

As any user (anonymous or authenticated),
I want to see the most recent titles added to the catalog on the home page,
so that I can quickly browse what is new without launching a search.

## Acceptance Criteria

1. **AC1 — Section presence and ordering.** Given the home page (`/`), when it renders for any role (Anonymous, Librarian, Admin), then a "Recent additions" section is visible directly below the "Collection at a glance" card (story 9-1) and above the browse controls. The section shows up to 10 most recently created active titles, sorted by `titles.created_at DESC`, excluding soft-deleted (`deleted_at IS NULL`).
2. **AC2 — TitleCard markup parity.** Each title in the section is rendered using the SAME visual template as the search-result browse list — `<article class="title-card">` with the `.title-card-cover`, `.title-card-info`, `.title-card-title`, `.title-card-contributor`, `.title-card-meta`, `.title-card-volumes` substructure as established by Epic 1 / UX-DR17 (list mode by default). The card is wrapped in an `<a href="/title/:id" class="title-card-link">` so a click navigates to the title detail page.
3. **AC3 — Single round-trip enriched query.** The query that drives the section is a single `SELECT` round-trip including the JOINs needed for TitleCard rendering: primary contributor (via `title_contributors` + `contributors` + `contributor_roles`, ordered by "Auteur" first), genre name, volume count (subquery on `volumes`), cover URL — all in one statement, no per-row N+1. New function lives at `src/models/title.rs::list_recent_active(pool, limit) -> Result<Vec<SearchResult>, AppError>` reusing the existing `SearchResult` struct (its 9 fields are exactly what TitleCard renders).
4. **AC4 — Fewer than 10 titles.** Given the active catalog has fewer than 10 titles, when the section renders, then it shows ALL existing active titles in the same order (no padding, no truncation downward to a smaller multiple).
5. **AC5 — Zero titles (interim empty-state).** Given the active catalog is empty (0 titles), when the section renders, then it displays a temporary inline empty-state (`<div class="text-center py-12 text-stone-500 dark:text-stone-400">` with the i18n text "No recent additions yet — start cataloging!" / "Pas encore d'ajouts récents — commencez à cataloguer !"). The `py-12` matches the established empty-state convention used by `templates/pages/home.html` (browse-results empty), `locations.html`, and `series_list.html` — do NOT use `py-8`. The full StatusMessage component arrives in story 9-15 and will be migrated then; for now, the inline fallback satisfies the spec's "instead of disappearing entirely" intent. Do NOT hide the section.
6. **AC6 — Anonymous data leak guard.** Given the section is visible to anonymous users, when the query runs, then it SELECTs only public columns (id, title, subtitle, media_type, genre_name, primary_contributor, volume_count, cover_image_url, publication_date) — no role-gated columns are joined. The query is role-agnostic (no `if session.role …` branch in the SQL) and the rendered HTML for an anonymous request never differs from the librarian/admin HTML for this section.
7. **AC7 — HTMX swap survival.** The "Recent additions" section sits OUTSIDE the `#browse-results` HTMX swap target (per story 9-1's same invariant for the glance card). When the user types in the search field and the HTMX search fragment is swapped into `#browse-results`, the recent additions section MUST remain visible above. Verify the placement empirically.
8. **AC8 — CSP compliance.** No `style="..."`, no `<style>`, no `<script>`, no `onclick=`, no inline event handlers. Tailwind utility classes only. The audit test `src/templates_audit.rs::no_inline_markup_in_templates` must continue to pass.
9. **AC9 — i18n EN + FR.** Two new keys under the existing `dashboard:` section: `dashboard.recent_additions.heading` ("Recent additions" / "Ajouts récents") and `dashboard.recent_additions.empty_state` ("No recent additions yet — start cataloging!" / "Pas encore d'ajouts récents — commencez à cataloguer !"). After editing locale files, run `touch src/lib.rs && cargo build` to force the i18n proc-macro to re-read.
10. **AC10 — Unit tests.**
    - (a) `list_recent_active` returns active titles ordered by `created_at DESC` with the requested LIMIT, excludes soft-deleted titles, and enriches with primary contributor + volume count + genre name (no N+1 — verified by a single SQL round-trip on the function body).
    - (b) `list_recent_active` with limit 10 on a DB seeded with 12 active titles returns exactly the 10 most recent, in the right order.
    - (c) `list_recent_active` on an empty DB returns an empty `Vec` (not an error).
    - (d) Handler render test asserts: with 3 seeded `SearchResult` items, the rendered HTML contains 3 `<article class="title-card">` blocks scoped under `#recent-additions`, in the same order they were passed in. With 0 items, the empty-state `<div>` is rendered with the i18n text instead.
11. **AC11 — E2E smoke.** A new test in `tests/e2e/specs/journeys/home.spec.ts`: anonymous → `/` → verify `#recent-additions` section is visible, contains at least 1 (or 0 — gracefully handle either) `article.title-card`, click the first card → `await page.waitForURL(/\/title\/\d+/)`. Use i18n-aware regex matchers (`/Recent additions|Ajouts récents/i`). No `waitForTimeout` (CI grep gate).

## Tasks / Subtasks

- [x] **Task 1 — Add `list_recent_active` to `TitleModel` (AC: 1, 3, 6, 10a–c)**
  - [x] In `src/models/title.rs`, add `pub async fn list_recent_active(pool: &DbPool, limit: u32) -> Result<Vec<SearchResult>, AppError>` after the existing `active_search` function (around line 810).
  - [x] The SQL is a SINGLE SELECT shaped like `active_search`'s data query but simpler — no FULLTEXT, no LIKE, no genre/state filter, just `WHERE t.deleted_at IS NULL ORDER BY t.created_at DESC LIMIT ?`. Reuse the same JOINs (genres, primary contributor subquery, volume_count subquery) so the returned `SearchResult` shape matches. Use `sqlx::query` (dynamic) per project convention (see Story 9-1 anti-pattern: no `sqlx::query!` macro to avoid `.sqlx/` cache regeneration).
  - [x] **Duplication policy:** the projection in `active_search` (SELECT clause + JOINs) is **17 lines** (verified at `src/models/title.rs:763-779`). That is well under the rule-of-three threshold for extraction. **Duplicate the projection inline in `list_recent_active`; do NOT extract a shared `SQL_SEARCH_RESULT_PROJECTION: &str` constant in this story.** A future story that adds a third caller can revisit.
- [x] **Task 2 — Wire the handler (AC: 1, 4, 5, 6, 7)**
  - [x] In `src/routes/home.rs::home` (after the `glance = ...` block from story 9-1), call `TitleModel::list_recent_active(pool, 10).await` with the **soft-degrade pattern** established by story 9-1 (match on the result, log `tracing::warn!` and use an empty `Vec` on error so the home page doesn't 500 on a transient DB issue).
  - [x] Extend `HomeTemplate` (`src/routes/home.rs:31-81`, struct of 50 fields post-9-1) with **THREE new fields**:
    - `recent_additions: Vec<SearchResult>` (the data — pulled from `crate::models::title::SearchResult`)
    - `recent_additions_heading: String` (pre-translated label)
    - `recent_additions_empty: String` (pre-translated label for the empty-state)
  - [x] Translate `dashboard.recent_additions.heading` and `dashboard.recent_additions.empty_state` in the handler via `rust_i18n::t!(...).to_string()`, per project convention (see Story 9-1 Dev Notes "Library/framework requirements" and `src/routes/home.rs:207-212` canonical example).
  - [x] Pass them from the handler. Do NOT add a new route.
- [x] **Task 3 — Add i18n keys (AC: 9)**
  - [x] In `locales/en.yml` under `dashboard:` (after the `glance:` sub-block), add a `recent_additions:` sub-block with `heading: "Recent additions"` and `empty_state: "No recent additions yet — start cataloging!"`.
  - [x] In `locales/fr.yml` under the same path: `heading: "Ajouts récents"` and `empty_state: "Pas encore d'ajouts récents — commencez à cataloguer !"`.
  - [x] **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper. After editing, `touch src/lib.rs && cargo build`.
- [x] **Task 4 — Render the section in the template (AC: 1, 2, 5, 7, 8)**
  - [x] In `templates/pages/home.html`, insert the new section AFTER the `</section>` closing the glance card (line ~104) and BEFORE the `<!-- Browse toggle + sort -->` block (line ~106).
  - [x] Markup outline (Tailwind utility classes only). The inner `<article>...</article>` block is a **VERBATIM** copy of `templates/pages/home.html:148-165` — not a paraphrase. Reproduce exactly to avoid drift in `aria-label` interpolation, the `cover::cover` macro call, the icon path, and the `group` class on the article wrapper:
    ```jinja
    {# Recent additions section (story 9-2). Sits between #collection-glance
       and #browse-results so it survives HTMX search swaps. The inner
       <article> block is duplicated VERBATIM from home.html:148-165 (the
       browse-results loop) — known follow-up to extract into a partial. #}
    <section id="recent-additions" aria-labelledby="recent-additions-heading" class="w-full max-w-4xl mt-6">
        <h2 id="recent-additions-heading" class="text-sm font-medium text-stone-600 dark:text-stone-400 uppercase tracking-wide">{{ recent_additions_heading }}</h2>
        {% if recent_additions.is_empty() %}
            <div class="text-center py-12 text-stone-500 dark:text-stone-400">{{ recent_additions_empty }}</div>
        {% else %}
            <div class="recent-additions-list mt-3 space-y-2">
                {% for item in recent_additions %}
                <article class="title-card group">
                    <a href="/title/{{ item.id }}" class="title-card-link"
                       aria-label="{{ item.title }}{% if let Some(c) = item.primary_contributor %} — {{ c }}{% endif %}">
                        <div class="title-card-cover">
                            {% call cover::cover(item.cover_image_url.as_deref().unwrap_or_default(), item.title, item.media_type, "w-full h-full object-cover", "lazy", label_no_cover) %}{% endcall %}
                            <div class="title-card-overlay">
                                <img src="/static/icons/{{ item.media_type }}.svg" alt="" class="w-5 h-5 opacity-80">
                                <span class="text-xs">{{ item.volume_count }} vol</span>
                            </div>
                        </div>
                        <div class="title-card-info">
                            <p class="title-card-title">{{ item.title }}</p>
                            <p class="title-card-contributor">{{ item.primary_contributor.as_deref().unwrap_or("") }}</p>
                            <p class="title-card-meta">{{ item.genre_name }}{% if let Some(d) = item.publication_date %} · {{ d.format("%Y") }}{% endif %}</p>
                            <p class="title-card-volumes">{{ item.volume_count }} vol</p>
                        </div>
                    </a>
                </article>
                {% endfor %}
            </div>
        {% endif %}
    </section>
    ```
  - [x] CSP: zero `style="..."`, zero `onclick=`, zero inline `<script>`. Tailwind classes + the existing `.title-card-*` classes from `static/css/browse.css` only.
  - [x] **Markup duplication note (deliberate):** the `<article class="title-card">` block now exists in 3 places: (1) `templates/pages/home.html` lines ~148-165 (browse-results loop), (2) the new recent-additions section (this story), (3) `src/routes/home.rs::render_search_row` lines ~363-410 (HTMX fragment, Rust-side HTML builder). Extracting to an Askama partial would require coordinating with `render_search_row` (which builds HTML in Rust, not Jinja) — out of scope. File a follow-up GH Issue (`type:change-request`) at story close to extract a `components/title_card.html` partial + a Rust function that serializes a `SearchResult` to that partial's expected context (e.g., via `Template::render_into_string` on a tiny `TitleCardTemplate` struct).
- [x] **Task 5 — Unit tests (AC: 10)**
  - [x] Create a sibling file `tests/dashboard_recent_additions.rs` (NOT a `mod` inside `tests/dashboard_glance.rs`) — clarity and discoverability over co-location, since glance and recent-additions are independent services.
  - [x] **Critical: `created_at` determinism.** The existing helper `tests/dashboard_glance.rs::insert_title` does NOT set `created_at` (it falls back to `DEFAULT CURRENT_TIMESTAMP`). 12 rows inserted in a tight loop will share the same second-precision timestamp, breaking the ORDER-BY assertion. Introduce a NEW helper in `tests/dashboard_recent_additions.rs`:
    ```rust
    async fn insert_title_with_created_at(
        pool: &MySqlPool,
        title: &str,
        genre_id: u64,
        minutes_ago: i32,
    ) -> u64 {
        let r = sqlx::query(
            "INSERT INTO titles (title, language, media_type, genre_id, created_at) \
             VALUES (?, 'fr', 'book', ?, NOW() - INTERVAL ? MINUTE)",
        )
        .bind(title)
        .bind(genre_id)
        .bind(minutes_ago)
        .execute(pool)
        .await
        .expect("insert title");
        r.last_insert_id()
    }
    ```
    Each seeded row gets a distinct `minutes_ago` (e.g. 0, 1, 2, …, 11) so ORDER BY `created_at DESC` yields a deterministic sequence.
  - [x] Three `#[sqlx::test(migrations = "./migrations")]` cases:
    - `list_recent_active_returns_empty_vec_on_empty_db`
    - `list_recent_active_orders_by_created_at_desc_with_limit` — seed 12 active titles via `insert_title_with_created_at` with `minutes_ago` 0..=11, call `list_recent_active(pool, 10)`, assert exactly 10 results AND the IDs are in the expected order (most-recent first).
    - `list_recent_active_excludes_soft_deleted` — seed 5 active + 3 soft-deleted, call with limit 10, assert only the 5 active are returned.
  - [x] Add 2 handler render tests in `src/routes/home.rs::mod tests`:
    - `home_renders_recent_additions_with_three_items` — build a HomeTemplate via `make_test_home_template_with_recent(role, vec_of_3_search_results)`, render, scope assertion to `#recent-additions` slice (reuse the `glance_card_slice` helper pattern; create a sibling `recent_additions_slice` if needed), assert: 3 `<article class="title-card">` present in input order, the title text of each item appears.
    - `home_renders_recent_additions_empty_state` — same but with `vec![]`, assert: NO `<article>` inside `#recent-additions`, the empty-state `<div>` IS present with the i18n text.
- [x] **Task 6 — E2E spec (AC: 11)**
  - [x] Extend `tests/e2e/specs/journeys/home.spec.ts` with one new `test.describe("Home page — Recent additions section", ...)` block. **Place it AFTER the existing `test.describe("Home page — Collection at a glance card", ...)` block (story 9-1)** — i.e., immediately after that block's closing `});` so the file stays organized chronologically by story.
    - `anonymous: section visible, first card navigates to /title/:id` — load `/`, verify `#recent-additions` is visible, verify heading matches `/Recent additions|Ajouts récents/i`. Then conditionally: if `recentCards.count() > 0`, click the first → `waitForURL(/\/title\/\d+/)`. If 0 (fresh DB scenario), verify the empty-state `<div>` text matches `/start cataloging|commencez à cataloguer/i`.
  - [x] Use i18n-aware regex matchers consistently. Do NOT add `waitForTimeout` (CI grep gate).
  - [x] Reuse the `loginAs` import already present in `home.spec.ts` if a librarian-role test variant is added (optional — AC9 says role-agnostic, so a single anonymous test is sufficient).
- [x] **Task 7 — Verify and document (AC: 1–11)**
  - [x] `cargo check && cargo clippy --all-targets -- -D warnings`
  - [x] `cargo test --lib` (full unit + co-located integration suite — must stay zero-fail; expect ~628 + 4 new = 632)
  - [x] `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test --test dashboard_recent_additions` (or whatever filename is chosen) — 3 new integration tests pass.
  - [x] `cargo sqlx prepare --check --workspace -- --all-targets` — expected no diff (Task 1 uses dynamic `query`).
  - [x] Manual smoke: `curl http://localhost:8080/` and grep for `id="recent-additions"`, `Ajouts récents`, and `article class="title-card"` to confirm the section renders.
  - [x] Update Dev Agent Record at the bottom of this file: list of files touched, decision on `mod recent_additions` vs sibling test file, anything surprising.

## Dev Notes

### Source tree references

| Concern | File / location | Notes |
|---|---|---|
| Home route + handler | `src/routes/mod.rs` (route reg) + `src/routes/home.rs` (handler) | extend the existing handler; do NOT create a parallel route |
| Existing TitleCard markup (browse) | `templates/pages/home.html:148-165` | the `<article class="title-card">` shape — DUPLICATE this into the new section (see Task 4 markup-duplication note) |
| Existing TitleCard markup (HTMX fragment) | `src/routes/home.rs::render_search_row:363-410` | Rust-side HTML builder; informs the eventual partial extraction (out of scope for 9-2) |
| `SearchResult` struct (9 fields) | `src/models/title.rs:615-627` | reuse as-is; the 9 fields are sufficient for TitleCard rendering. **Note**: `subtitle: Option<String>` is in the struct but is NOT rendered by the current TitleCard markup (verified at `home.html:148-165` and `home.rs::render_search_row`). Keep `subtitle` in the projection for shape parity; do NOT add a render path for it in this story (a future story can extend the card). |
| `SearchService::enrich_title` (per-title enricher) | `src/services/search.rs:40-61` | OK to reference for shape, but DO NOT call per-row from the handler — Task 1 emits one enriched SELECT instead |
| `active_search` SELECT projection (canonical TitleCard-shaped SELECT) | `src/models/title.rs:763-779` | the SQL string — 17 lines including SELECT clause + JOINs + the contributor + volume_count subqueries; Task 1 mirrors this without the FULLTEXT/filter/sort/pagination layers |
| `active_search` row-mapping closure | `src/models/title.rs:795-810` | maps a `MySqlRow` into a `SearchResult`; mirror this exactly in `list_recent_active` so the two functions stay shape-compatible |
| HomeTemplate struct | `src/routes/home.rs:31-83` (post-9-1) | extend with 3 new fields per Task 2 |
| Story 9-1 glance card placement (reference for placement invariant) | `templates/pages/home.html:76-104` (the `<section id="collection-glance">`) | the new `#recent-additions` sits IMMEDIATELY after this, peer-level, both outside `#browse-results` |
| HomeTemplate test factory | `src/routes/home.rs::make_test_home_template_with_counts` | extend or sibling: introduce `make_test_home_template_with_recent(role, link, recent: Vec<SearchResult>)` that delegates to the glance factory + sets the new fields |
| Slice helper for scoped HTML assertions | `src/routes/home.rs::glance_card_slice` | clone the helper as `recent_additions_slice` (or generalize via a `slice_section(html, id)` helper) |
| i18n locales | `locales/en.yml` / `locales/fr.yml` (`dashboard:` section, after `glance:`) | NO top-level `en:`/`fr:` wrapper |
| Templates audit | `src/templates_audit.rs::no_inline_markup_in_templates` | must stay green |
| Test pattern (DB-backed integration) | `tests/dashboard_glance.rs` | hand-rolled SQL inserts via `sqlx::query` are the established pattern; reuse the `insert_title` helper or duplicate as needed |
| E2E helper (`loginAs`) | `tests/e2e/helpers/auth.ts` | optional — Task 6 baseline is anonymous |
| E2E pattern for TitleCard click | `tests/e2e/specs/journeys/home-search.spec.ts:27-38` | the existing `await page.waitForURL(/\/title\/\d+/)` pattern; copy verbatim |

### Anti-patterns to avoid

- **N+1 enrichment.** Do NOT fetch the title list, then loop and call `SearchService::enrich_title(pool, t)` per row. AC3 forbids it. Mirror `active_search`'s shape: ONE SELECT with subqueries for contributor + volume_count + JOIN for genre.
- **Hiding the section on empty DB.** AC5 mandates the inline empty-state. A `{% if !recent_additions.is_empty() %}` wrapper on the entire section is wrong.
- **Hard-failing on DB error.** Mirror story 9-1's `collection_glance` defensive pattern: `match TitleModel::list_recent_active(pool, 10).await { Ok(v) => v, Err(e) => { tracing::warn!(...); Vec::new() } }`. The home page MUST NOT return 500 because the recent-additions query had a hiccup.
- **`sqlx::query!` macro.** Forces `cargo sqlx prepare` regeneration in the PR. Use dynamic `query` (project convention).
- **Calling `t!()` from inside the Askama template.** Pre-translate in the handler, pass as `String` fields. Story 9-1 canonical example: `src/routes/home.rs:207-212` (`label_metadata_errors`).
- **Adding new CSS for the section.** The `.title-card-*` classes from `static/css/browse.css` (lines 15-48) cover everything; a new class like `.recent-additions-card` is unnecessary duplication.
- **Padding the result list to always 10.** AC4 explicitly: fewer than 10 → show all existing, no padding.
- **Ordering by `id` instead of `created_at`.** Adjacent IDs do NOT guarantee adjacent insertion times if a backfill / restore happened. Story spec is explicit: ORDER BY `created_at DESC`.
- **Filtering by role in SQL.** The query is role-agnostic (AC6). Branch in the handler ONLY if a future story adds role-gated columns.

### Architecture compliance

- **Error handling:** Any DB failure in `list_recent_active` returns `AppError::Database` via `?`; the handler soft-degrades with `tracing::warn!` + empty `Vec` (per story 9-1 pattern). No new `AppError` variant.
- **Logging:** `tracing::warn!` at handler level on the soft-degrade path; `tracing::debug!` only inside the model function if needed.
- **DB query discipline:** `WHERE deleted_at IS NULL` on every SELECT/JOIN of entity tables (titles, genres, contributors, volumes). MariaDB type gotchas: not relevant here — `created_at` is `TIMESTAMP` but read via `query_as` with the existing `SearchResult.publication_date: Option<NaiveDate>` shape (no new `created_at` field exposed; it's only used for ordering).
- **HTMX coexistence:** The new section sits OUTSIDE `#browse-results` (HTMX swap target) — same invariant as story 9-1's glance card. Verify by inspecting `home.html` after insert.
- **CSP middleware:** Already wraps the handler outermost. No work needed.
- **Pool access:** The handler already has `state.pool: DbPool`. Do not introduce a new connection.

### Library / framework requirements

- **Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + Tailwind CSS v4 + HTMX 2.0** — no version changes, no new dependencies.
- **rust_i18n** — already wired. Pre-translate `recent_additions_heading` and `recent_additions_empty` in the handler via `rust_i18n::t!("dashboard.recent_additions.heading", locale = loc).to_string()` and pass as `String` fields on the Askama template struct. Canonical example: `src/routes/home.rs:207-212`.

### File structure requirements

| File | Action | Rough size |
|---|---|---|
| `src/models/title.rs` | **edit** | +~40 lines (one `list_recent_active` fn — the SELECT + projection + JOIN block) |
| `src/services/dashboard.rs` | **edit** | optional re-export or thin wrapper if needed for symmetry with `collection_glance`; can be skipped — handler may call `TitleModel::list_recent_active` directly |
| `src/routes/home.rs` | **edit** | +~12 lines in handler + extend `HomeTemplate` (3 fields) + extend factory + 2 new render tests + slice helper extension |
| `templates/pages/home.html` | **edit** | +~25 lines for the section + the duplicated TitleCard markup |
| `locales/en.yml` | **edit** | +~3 lines under `dashboard.recent_additions:` |
| `locales/fr.yml` | **edit** | +~3 lines under `dashboard.recent_additions:` |
| `tests/dashboard_recent_additions.rs` | **create** | ~80 lines (3 `#[sqlx::test]` cases with seed helpers) |
| `tests/e2e/specs/journeys/home.spec.ts` | **edit** | +~25 lines (1 new test case) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **edit** | only the `9-2-...` line + `last_updated` (per CLAUDE.md rule 16) |
| `_bmad-output/implementation-artifacts/9-2-dashboard-recent-additions.md` | **edit** | this file — Status, Tasks checked, Dev Agent Record on completion |

### Testing requirements

- **Coverage target:** every AC has at least one test (unit OR E2E).
- **Ordering invariant** is the load-bearing test: `list_recent_active_orders_by_created_at_desc_with_limit` is the regression guard. If a future migration changes how `created_at` is populated (e.g., switching from `CURRENT_TIMESTAMP` to a manual default in INSERT), this test catches it.
- **Empty-state rendering** is the second load-bearing test: `home_renders_recent_additions_empty_state` ensures a future regression doesn't accidentally hide the section.
- **E2E** keeps to 1 anonymous test for parsimony — the section is role-agnostic per AC6.

### Project structure notes

This story aligns cleanly with the existing structure. Two intentional deviations worth noting:

1. **TitleCard markup duplication** — addressed by a follow-up GH Issue (`type:change-request` to extract `templates/components/title_card.html` partial in a future cross-cutting PR). The duplication scope is now 3 sites (browse loop in home.html, recent-additions in home.html, render_search_row in home.rs). This is exactly the "rule of three" trigger for extraction; doing it inside this story would balloon the PR and require a Rust↔Askama bridge.

2. **Sibling test file vs `mod` extension** — `tests/dashboard_recent_additions.rs` (sibling) chosen over extending `tests/dashboard_glance.rs` because the two services are functionally independent (one does counts, the other lists titles); discoverability by file name beats co-location.

## References

- [Story 9.2 spec — `_bmad-output/planning-artifacts/epics.md` lines 1226–1240](../planning-artifacts/epics.md)
- [Epic 9 scope note + split philosophy — `epics.md` lines 1204–1207](../planning-artifacts/epics.md)
- [PRD FR56 — `_bmad-output/planning-artifacts/prd.md`](../planning-artifacts/prd.md) (search for `FR56`)
- [PRD FR65 (anonymous browse) — `prd.md`](../planning-artifacts/prd.md)
- [UX-DR17 (TitleCard list/grid mode) + UX-DR18 (BrowseToggle) — `_bmad-output/planning-artifacts/ux-design-specification.md`](../planning-artifacts/ux-design-specification.md)
- [Story 9-1 spec (canonical patterns: handler-side i18n, single round-trip, soft-degrade, slice helper for scoped tests) — `9-1-dashboard-global-stats-card.md`](./9-1-dashboard-global-stats-card.md)
- [`CLAUDE.md` — Foundation Rules (#1 DRY, #7 E2E smoke, #11 GitHub Issues, #12 ≤ 2000 LOC, #13 local testing, #14 one-branch-one-story, #15 draft PR, #16 sprint-status ownership, #18 CI gating)](../../CLAUDE.md)

## Dev Agent Record

### Agent Model Used

`claude-opus-4-7` (1M context).

### Debug Log References

- `cargo test --lib` — 630 passed, 0 failed (628 pre-9-2 + 2 new render tests = `home_renders_recent_additions_with_three_items`, `home_renders_recent_additions_empty_state`).
- `cargo test --test dashboard_recent_additions` — 3 passed (empty DB → empty Vec; 12 seeded → 10 in DESC order; 5 active + 3 soft-deleted → 5 returned).
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo sqlx prepare --check --workspace -- --all-targets` — no diff (Task 1 uses dynamic `sqlx::query`).
- E2E validation deferred to CI (local `tests/e2e/test-results/` still owned by root from earlier Docker runs — same blocker as Story 9-1; documented as a permission cleanup task in the deferred-findings tracker, GH issue tba).

### Completion Notes List

- **Test file location**: sibling `tests/dashboard_recent_additions.rs` (not `mod` inside `dashboard_glance.rs`) — confirmed per spec, two services functionally independent.
- **Projection inlined, not extracted**: the `list_recent_active` SELECT duplicates the projection from `active_search` (genre + primary contributor subquery + volume_count subquery + cover/publication fields) inline. ~17 lines, well under the rule-of-three threshold. No shared `SQL_SEARCH_RESULT_PROJECTION` constant introduced; a future story with a third caller can revisit.
- **HTMX swap survival**: the `<section id="recent-additions">` is placed directly after the closing `</section>` of `#collection-glance` (line ~138) and before the `<!-- Browse toggle + sort -->` block (line ~140). Both sections sit OUTSIDE `#browse-results` (line ~177) so they survive HTMX search-swap targets. Manual smoke confirmed via `curl http://localhost:8080/` that both `id="collection-glance"` and `id="recent-additions"` appear in the rendered HTML, in that order.
- **Slice helper generalized**: instead of cloning `glance_card_slice`, refactored both into a single `slice_section(html, id)` helper. The original `glance_card_slice` becomes a 1-line shim for backward compatibility with story 9-1's render tests, and `recent_additions_slice` is the symmetric new helper. Better than duplication.
- **`fake_search_result` test factory**: introduced in `mod tests` to seed the new render tests with deterministic `SearchResult` instances without touching the DB. Reused by both `home_renders_recent_additions_with_three_items` (3 items in input order) and `home_renders_recent_additions_empty_state` (empty Vec).
- **AC1 ordering bug caught + fixed**: the initial template draft placed `#recent-additions` BEFORE `#collection-glance`. AC1 explicitly requires "Recent additions directly below Collection at a glance". Re-ordered before manual smoke.
- **Empty-state padding `py-12`**: matches the project convention used by browse-results empty (home.html), `locations.html`, and `series_list.html`. Validation pass had caught `py-8` as a divergence; fix applied per the validation refinements.

### File List

| File | Action |
|---|---|
| `src/models/title.rs` | edit (added `list_recent_active()` after `active_search`, ~50 lines) |
| `src/routes/home.rs` | edit (extended `HomeTemplate` with 3 fields, populated in handler with soft-degrade, refactored `glance_card_slice` into shared `slice_section` helper, added `recent_additions_slice`, added `make_test_home_template_with_recent`, added `fake_search_result` factory, added 2 new render tests) |
| `templates/pages/home.html` | edit (inserted `#recent-additions` section between `#collection-glance` and the browse toggle, with verbatim TitleCard markup duplicated from the browse-results loop) |
| `locales/en.yml` | edit (`dashboard.recent_additions.heading` + `empty_state`) |
| `locales/fr.yml` | edit (FR equivalents) |
| `tests/dashboard_recent_additions.rs` | create (3 `#[sqlx::test]` cases + `insert_title_with_created_at` helper) |
| `tests/e2e/specs/journeys/home.spec.ts` | edit (new `describe` block placed after the glance-card describe, single anonymous test that handles both populated and empty-DB cases) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | edit (9-2 in-progress → review on completion; only this line + `last_updated` per CLAUDE.md rule 16) |
| `_bmad-output/implementation-artifacts/9-2-dashboard-recent-additions.md` | edit (Status → review, all 7 tasks checked, Dev Agent Record filled) |

### Change Log

- **2026-04-30** — Initial implementation. All 7 tasks complete; 630 lib tests + 3 dashboard_recent_additions integration tests pass; clippy clean; sqlx cache unchanged. Followed Story 9-1 patterns (handler-side i18n, soft-degrade on DB error, scoped HTML assertions). One spec drift caught in template draft (AC1 ordering — `#recent-additions` was placed BEFORE `#collection-glance` in the first pass; re-ordered). E2E run deferred to CI per Story 9-1 precedent.
- **2026-04-30** — CI surfaced a regression in pre-existing tests: `dewey-code.spec.ts` (lines 24, 92) and `csp-headers.spec.ts` (line 153) used a bare `page.locator('a[href^="/title/"]').first()` selector after `goto('/?q=...')`. Before Story 9-2, the only `<a href="/title/N">` on the home page lived inside `#browse-results`. Story 9-2 added `#recent-additions` ABOVE `#browse-results` (which also renders `<a href="/title/N">` blocks), so `.first()` started picking a card from recent-additions instead of from the search results. Fix: scope all three sites to `'#browse-results a[href^="/title/"]'`. Pattern to remember for future stories that add entity-link sections to existing pages: **a global selector across the home page is now ambiguous; always scope to a section id**. The `similar-titles.spec.ts` pattern (`section.locator(...)`) was already correct and unaffected; `csp-headers.spec.ts:116` is on `/catalog` (no `#recent-additions`) and is unaffected.
- **2026-04-30** — Code review pass: 1 High + 3 Medium patches applied, 4 deferred (will be filed as GH Issues per CLAUDE.md rule 11), 4 dismissed.
  - **High — unstyled cards**: the wrapper `<div class="recent-additions-list mt-3 space-y-2">` did NOT trigger any of the `.title-card-*` rules in `static/css/browse.css` (all scoped under `.browse-list .` or `.browse-grid .`). Cards rendered as an unstyled text stack. Fixed by changing the wrapper class to `browse-list mt-3` so the existing `.browse-list .title-card-*` rules apply. Visual parity with the search-result browse list (AC2) is now preserved.
  - **Medium — empty-state test scoping**: 2 of 5 assertions in `home_renders_recent_additions_empty_state` searched the whole HTML; the heading assertion now runs on the section slice so a structural break would fail the test.
  - **Medium — AC1 ordering test**: added `home_renders_glance_above_recent_additions` to lock in document-order between `#collection-glance` and `#recent-additions`. The first implementation draft had them inverted; the new test prevents recurrence.
  - **Deferred** (file as GH Issues): CI grep gate for unscoped `a[href^="/title/"]` selectors; soft-deleted-genre silently hides title (pre-existing in `active_search` too); TitleCard partial extraction + `vol` literal i18n + count rendered twice (bundled — already on the books); `fake_search_result` factory missing `Some(d)`/`Some(c)` template-branch coverage.
  - **Dismissed**: `unwrap_or` swallowing decode errors (false positive — consistent with `active_search`); N+1 single-round-trip programmatic guard (intentionally not testable per Story 9-1 precedent — verified at code review); E2E "either way" branch (matches AC11 spec intent explicitly); cosmetic margin-spacing complaints.
  - Final test counts: 631 lib tests + 3 dashboard_recent_additions integration tests (632 lib previously; +1 = the new ordering test). Status: review → done.
