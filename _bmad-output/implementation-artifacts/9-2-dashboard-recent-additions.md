# Story 9.2: Dashboard — recent additions

Status: ready-for-dev

## Story

As any user (anonymous or authenticated),
I want to see the most recent titles added to the catalog on the home page,
so that I can quickly browse what is new without launching a search.

## Acceptance Criteria

1. **AC1 — Section presence and ordering.** Given the home page (`/`), when it renders for any role (Anonymous, Librarian, Admin), then a "Recent additions" section is visible directly below the "Collection at a glance" card (story 9-1) and above the browse controls. The section shows up to 10 most recently created active titles, sorted by `titles.created_at DESC`, excluding soft-deleted (`deleted_at IS NULL`).
2. **AC2 — TitleCard markup parity.** Each title in the section is rendered using the SAME visual template as the search-result browse list — `<article class="title-card">` with the `.title-card-cover`, `.title-card-info`, `.title-card-title`, `.title-card-contributor`, `.title-card-meta`, `.title-card-volumes` substructure as established by Epic 1 / UX-DR17 (list mode by default). The card is wrapped in an `<a href="/title/:id" class="title-card-link">` so a click navigates to the title detail page.
3. **AC3 — Single round-trip enriched query.** The query that drives the section is a single `SELECT` round-trip including the JOINs needed for TitleCard rendering: primary contributor (via `title_contributors` + `contributors` + `contributor_roles`, ordered by "Auteur" first), genre name, volume count (subquery on `volumes`), cover URL — all in one statement, no per-row N+1. New function lives at `src/models/title.rs::list_recent_active(pool, limit) -> Result<Vec<SearchResult>, AppError>` reusing the existing `SearchResult` struct (its 9 fields are exactly what TitleCard renders).
4. **AC4 — Fewer than 10 titles.** Given the active catalog has fewer than 10 titles, when the section renders, then it shows ALL existing active titles in the same order (no padding, no truncation downward to a smaller multiple).
5. **AC5 — Zero titles (interim empty-state).** Given the active catalog is empty (0 titles), when the section renders, then it displays a temporary inline empty-state (`<div class="text-center text-stone-500 dark:text-stone-400 py-8">` with the i18n text "No recent additions yet — start cataloging!" / "Pas encore d'ajouts récents — commencez à cataloguer !"). The full StatusMessage component arrives in story 9-15 and will be migrated then; for now, the inline fallback satisfies the spec's "instead of disappearing entirely" intent. Do NOT hide the section.
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

- [ ] **Task 1 — Add `list_recent_active` to `TitleModel` (AC: 1, 3, 6, 10a–c)**
  - [ ] In `src/models/title.rs`, add `pub async fn list_recent_active(pool: &DbPool, limit: u32) -> Result<Vec<SearchResult>, AppError>` after the existing `active_search` function (around line 810).
  - [ ] The SQL is a SINGLE SELECT shaped like `active_search`'s data query but simpler — no FULLTEXT, no LIKE, no genre/state filter, just `WHERE t.deleted_at IS NULL ORDER BY t.created_at DESC LIMIT ?`. Reuse the same JOINs (genres, primary contributor subquery, volume_count subquery) so the returned `SearchResult` shape matches. Use `sqlx::query` (dynamic) per project convention (see Story 9-1 anti-pattern: no `sqlx::query!` macro to avoid `.sqlx/` cache regeneration).
  - [ ] Note: `active_search`'s SQL fragment with the contributor + volume_count subqueries is large. Consider extracting the SELECT projection + JOINs into a shared `SQL_SEARCH_RESULT_PROJECTION: &str` constant if and only if duplication exceeds ~40 lines; otherwise duplicate inline (Foundation Rule #1 DRY allows three similar lines over a premature abstraction).
- [ ] **Task 2 — Wire the handler (AC: 1, 4, 5, 6, 7)**
  - [ ] In `src/routes/home.rs::home` (after the `glance = ...` block from story 9-1), call `TitleModel::list_recent_active(pool, 10).await` with the **soft-degrade pattern** established by story 9-1 (match on the result, log `tracing::warn!` and use an empty `Vec` on error so the home page doesn't 500 on a transient DB issue).
  - [ ] Extend `HomeTemplate` (`src/routes/home.rs:32`) with TWO new fields: `recent_additions: Vec<SearchResult>` (the data) and `recent_additions_heading: String` + `recent_additions_empty: String` (pre-translated labels — handler-side translation per project convention, see story 9-1 Dev Notes "Library/framework requirements").
  - [ ] Pass them from the handler. Do NOT add a new route.
- [ ] **Task 3 — Add i18n keys (AC: 9)**
  - [ ] In `locales/en.yml` under `dashboard:` (after the `glance:` sub-block), add a `recent_additions:` sub-block with `heading: "Recent additions"` and `empty_state: "No recent additions yet — start cataloging!"`.
  - [ ] In `locales/fr.yml` under the same path: `heading: "Ajouts récents"` and `empty_state: "Pas encore d'ajouts récents — commencez à cataloguer !"`.
  - [ ] **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper. After editing, `touch src/lib.rs && cargo build`.
- [ ] **Task 4 — Render the section in the template (AC: 1, 2, 5, 7, 8)**
  - [ ] In `templates/pages/home.html`, insert the new section AFTER the `</section>` closing the glance card (line ~104) and BEFORE the `<!-- Browse toggle + sort -->` block (line ~106).
  - [ ] Markup outline (Tailwind utility classes only):
    ```jinja
    {# Recent additions section (story 9-2). Sits between #collection-glance
       and #browse-results so it survives HTMX search swaps. #}
    <section id="recent-additions" aria-labelledby="recent-additions-heading" class="w-full max-w-4xl mt-6">
        <h2 id="recent-additions-heading" class="text-sm font-medium text-stone-600 dark:text-stone-400 uppercase tracking-wide">{{ recent_additions_heading }}</h2>
        {% if recent_additions.is_empty() %}
            <div class="text-center text-stone-500 dark:text-stone-400 py-8">{{ recent_additions_empty }}</div>
        {% else %}
            <div class="recent-additions-list mt-3 space-y-2">
                {% for item in recent_additions %}
                    {# TitleCard markup — duplicated from the browse-results loop
                       (lines ~148-165). Out-of-scope for this story to extract
                       into an Askama partial; tracked for follow-up — see Dev Notes. #}
                    <article class="title-card">
                        <a href="/title/{{ item.id }}" class="title-card-link">
                            ... (mirror lines ~148-165 of home.html exactly,
                            same Tailwind classes, same icon/cover fallback) ...
                        </a>
                    </article>
                {% endfor %}
            </div>
        {% endif %}
    </section>
    ```
  - [ ] CSP: zero `style="..."`, zero `onclick=`, zero inline `<script>`. Tailwind classes + the existing `.title-card-*` classes from `static/css/browse.css` only.
  - [ ] **Markup duplication note (deliberate):** the `<article class="title-card">` block exists in 3 places after this story: (1) `templates/pages/home.html` lines ~148-165 (browse-results loop), (2) the new recent-additions section (this story), (3) `src/routes/home.rs::render_search_row` lines ~363-410 (HTMX fragment). Extracting to an Askama partial would require coordinating with `render_search_row` (which builds HTML in Rust, not Jinja) — out of scope. File a follow-up GH Issue (`type:change-request`) at story close to extract a `components/title_card.html` partial + a Rust function that serializes a `SearchResult` to that partial's expected context (e.g., via `Template::render_into_string` on a tiny `TitleCardTemplate` struct).
- [ ] **Task 5 — Unit tests (AC: 10)**
  - [ ] Extend `tests/dashboard_glance.rs` with a new test module `mod recent_additions { ... }` OR create a sibling file `tests/dashboard_recent_additions.rs`. Choice: **sibling file** for clarity — the two services (glance vs recent-additions) are independent and tests are easier to discover by name.
  - [ ] Three `#[sqlx::test(migrations = "./migrations")]` cases:
    - `list_recent_active_returns_empty_vec_on_empty_db`
    - `list_recent_active_orders_by_created_at_desc_with_limit` — seed 12 active titles with deterministic timestamps (use `INSERT ... VALUES (..., NOW() - INTERVAL N MINUTE)` so order is stable), call with limit 10, assert exactly 10 results in the expected order. Hand-rolled SQL inserts following the `tests/dashboard_glance.rs` pattern.
    - `list_recent_active_excludes_soft_deleted` — seed 5 active + 3 soft-deleted, call with limit 10, assert only the 5 active are returned.
  - [ ] Add 2 handler render tests in `src/routes/home.rs::mod tests`:
    - `home_renders_recent_additions_with_three_items` — build a HomeTemplate via `make_test_home_template_with_recent(role, vec_of_3_search_results)`, render, scope assertion to `#recent-additions` slice (reuse the `glance_card_slice` helper pattern; create a sibling `recent_additions_slice` if needed), assert: 3 `<article class="title-card">` present in input order, the title text of each item appears.
    - `home_renders_recent_additions_empty_state` — same but with `vec![]`, assert: NO `<article>` inside `#recent-additions`, the empty-state `<div>` IS present with the i18n text.
- [ ] **Task 6 — E2E spec (AC: 11)**
  - [ ] Extend `tests/e2e/specs/journeys/home.spec.ts` with one new `test.describe("Home page — Recent additions section", ...)` block:
    - `anonymous: section visible, first card navigates to /title/:id` — load `/`, verify `#recent-additions` is visible, verify heading matches `/Recent additions|Ajouts récents/i`. Then conditionally: if `recentCards.count() > 0`, click the first → `waitForURL(/\/title\/\d+/)`. If 0 (fresh DB scenario), verify the empty-state `<div>` text matches `/start cataloging|commencez à cataloguer/i`.
  - [ ] Use i18n-aware regex matchers consistently. Do NOT add `waitForTimeout` (CI grep gate).
  - [ ] Reuse the `loginAs` import already present in `home.spec.ts` if a librarian-role test variant is added (optional — AC9 says role-agnostic, so a single anonymous test is sufficient).
- [ ] **Task 7 — Verify and document (AC: 1–11)**
  - [ ] `cargo check && cargo clippy --all-targets -- -D warnings`
  - [ ] `cargo test --lib` (full unit + co-located integration suite — must stay zero-fail; expect ~628 + 4 new = 632)
  - [ ] `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test --test dashboard_recent_additions` (or whatever filename is chosen) — 3 new integration tests pass.
  - [ ] `cargo sqlx prepare --check --workspace -- --all-targets` — expected no diff (Task 1 uses dynamic `query`).
  - [ ] Manual smoke: `curl http://localhost:8080/` and grep for `id="recent-additions"`, `Ajouts récents`, and `article class="title-card"` to confirm the section renders.
  - [ ] Update Dev Agent Record at the bottom of this file: list of files touched, decision on `mod recent_additions` vs sibling test file, anything surprising.

## Dev Notes

### Source tree references

| Concern | File / location | Notes |
|---|---|---|
| Home route + handler | `src/routes/mod.rs` (route reg) + `src/routes/home.rs` (handler) | extend the existing handler; do NOT create a parallel route |
| Existing TitleCard markup (browse) | `templates/pages/home.html:148-165` | the `<article class="title-card">` shape — DUPLICATE this into the new section (see Task 4 markup-duplication note) |
| Existing TitleCard markup (HTMX fragment) | `src/routes/home.rs::render_search_row:363-410` | Rust-side HTML builder; informs the eventual partial extraction (out of scope for 9-2) |
| `SearchResult` struct (9 fields) | `src/models/title.rs:615-627` | reuse as-is; schema is complete for TitleCard rendering |
| `SearchService::enrich_title` (per-title enricher) | `src/services/search.rs:40-61` | OK to reference for shape, but DO NOT call per-row from the handler — Task 1 emits one enriched SELECT instead |
| `active_search` query template (FULLTEXT + filters + projection) | `src/models/title.rs:752-810` | the projection (lines 752-768) is the canonical TitleCard-shaped SELECT — Task 1 mirrors it without the FULLTEXT/filter/sort/pagination layers |
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

_To be filled by dev agent (e.g., `claude-opus-4-7`)._

### Debug Log References

_To be filled during implementation._

### Completion Notes List

_To be filled during implementation. Document at minimum:_
- _Decision on test file location (sibling `tests/dashboard_recent_additions.rs` vs `mod` inside `tests/dashboard_glance.rs`)._
- _Whether the SELECT projection was duplicated inline or extracted to a shared SQL constant (Task 1 sub-bullet)._
- _Confirmation that the section sits OUTSIDE `#browse-results` and survives an HTMX search swap._
- _Anything surprising encountered (existing helper found, schema quirk, i18n key collision, etc.)._

### File List

_To be filled by dev agent — exhaustive list of files touched, including the sprint-status.yaml line update._
