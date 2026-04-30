# Story 9.1: Dashboard — global stats card

Status: ready-for-dev

## Story

As any user (anonymous or authenticated),
I want to see global collection stats on the home page,
so that I get an immediate sense of the catalog's size at a glance.

## Acceptance Criteria

1. **AC1 — Card presence and structure.** Given the home page (`/`), when it renders for any role (Anonymous, Librarian, Admin), then a "Collection at a glance" card is visible and displays exactly three counts: total active titles, total active volumes, total active loans — each computed by excluding `deleted_at IS NOT NULL` rows. For loans, "active" additionally requires `returned_at IS NULL`.
2. **AC2 — Single round-trip query.** Given the counts, when computed, then they come from a single SQL round-trip (one query with three sub-counts) — not three separate `fetch_one` calls. No N+1, no per-entity lookup. The query lives in a service or model function with type signature `Result<CollectionGlance, AppError>` where `CollectionGlance` is a small struct with three `i64` (or `u64`) fields.
3. **AC3 — Empty DB renders the card.** Given a fresh DB, when the counts are zero, then the card still renders with "0 titles", "0 volumes", "0 loans" — no empty-state hiding here. The broader "empty catalog" StatusMessage UX is a separate concern handled by story 9-15; do NOT introduce it in this story.
4. **AC4 — Role-aware links on count lines.** Given the card, when rendered, then each count line is a clickable link as follows:
    - **Title count** → `/catalog` (route exists; no role gate). All roles get a real `<a href>`.
    - **Volume count** → `/catalog?view=volumes` if such a route exists; otherwise reuse `/catalog` (the dev agent must verify and document the choice in the Dev Agent Record). All roles get a real `<a href>`.
    - **Loan count** → `/loans` ONLY when `session.role >= Role::Librarian`. For Anonymous, the count is rendered as plain text inside a `<span>` with `aria-describedby` pointing to a hidden help message "Sign in to view loans" / "Connectez-vous pour voir les prêts" (the help text is added now; the visible tooltip component itself ships with story 9-19 — do NOT block on it).
5. **AC5 — Anonymous never receives the loan link in HTML.** Given the role-aware link generation, when tested, then the rendered HTML for an anonymous request does NOT contain `href="/loans"` anywhere (regression guard against accidental over-rendering). This is a byte-level assertion in the unit test for the handler.
6. **AC6 — CSP compliance.** The card uses Tailwind utility classes only — no `style="..."`, no `<style>` block, no inline event handlers. The audit test in `src/templates_audit.rs::no_inline_markup_in_templates` must continue to pass after this change.
7. **AC7 — i18n EN + FR.** All user-visible strings have keys in both `locales/en.yml` and `locales/fr.yml` under the existing `dashboard:` section: `dashboard.glance.heading`, `dashboard.glance.titles`, `dashboard.glance.volumes`, `dashboard.glance.active_loans`, `dashboard.glance.signin_to_view_loans`. Use `%{count}` plural-form params where appropriate. After editing locale files, run `touch src/lib.rs && cargo build` to force the i18n proc macro to re-read.
8. **AC8 — Unit tests.** Tests must cover: (a) count query returns correct values on empty DB (0/0/0); (b) count query returns correct values on a mixed dataset including soft-deleted titles, soft-deleted volumes, returned loans, and at least one of each active; (c) handler-level test that asserts the rendered HTML for an anonymous session contains the three counts AND does NOT contain `href="/loans"`; (d) handler-level test that asserts the rendered HTML for a Librarian session contains `href="/loans"` linked to the loan count.
9. **AC9 — E2E smoke.** A new spec or extension to `tests/e2e/specs/journeys/home.spec.ts` covers: anonymous → `/` → verify card with three counts visible, loan count is plain text (no link); login as librarian via `loginAs(page, "librarian")` → `/` → verify loan count is now an `<a>` element → click it → `/loans` opens. Use i18n-aware text matchers (`/Active loans|Prêts en cours/i`).

## Tasks / Subtasks

- [ ] **Task 1 — Add `count_active()` to the three models (AC: 2, 3, 8)**
  - [ ] In `src/models/title.rs`, add `pub async fn count_active(pool: &DbPool) -> Result<i64, AppError>` using `SELECT COUNT(*) FROM titles WHERE deleted_at IS NULL`. Follow the pattern in `src/models/volume_state.rs:177-184`.
  - [ ] In `src/models/volume.rs`, add `pub async fn count_active(pool: &DbPool) -> Result<i64, AppError>` using the same pattern on `volumes`.
  - [ ] In `src/models/loan.rs`, add `pub async fn count_active(pool: &DbPool) -> Result<i64, AppError>` filtering on `returned_at IS NULL AND deleted_at IS NULL`. Follow the pattern in `src/models/volume_state.rs::count_active_loans_for_state` (lines 257-273).
  - [ ] These per-model `count_active()` are intentionally added even though AC2 mandates a single-round-trip query, because they will be used by other Epic 9 stories (9-4, 9-5) and keep the per-table SQL co-located with each model.
- [ ] **Task 2 — Add `CollectionGlance` aggregate query in a service (AC: 1, 2)**
  - [ ] Create `src/services/dashboard.rs` (new module) — register it in `src/services/mod.rs` with `pub mod dashboard;`.
  - [ ] Define `pub struct CollectionGlance { pub titles: i64, pub volumes: i64, pub active_loans: i64 }` deriving `Debug, Clone, sqlx::FromRow`.
  - [ ] Implement `pub async fn collection_glance(pool: &DbPool) -> Result<CollectionGlance, AppError>` that runs a single SQL query of the form:
    ```sql
    SELECT
      (SELECT COUNT(*) FROM titles WHERE deleted_at IS NULL)  AS titles,
      (SELECT COUNT(*) FROM volumes WHERE deleted_at IS NULL) AS volumes,
      (SELECT COUNT(*) FROM loans WHERE returned_at IS NULL AND deleted_at IS NULL) AS active_loans
    ```
    Use `sqlx::query_as::<_, CollectionGlance>` + `.fetch_one(pool)`. This is a single round-trip per AC2.
  - [ ] DO NOT use `sqlx::query!` macro for this — it requires `.sqlx/` cache regeneration. Use the dynamic `query_as` form.
- [ ] **Task 3 — Add i18n keys (AC: 7)**
  - [ ] In `locales/en.yml` under `dashboard:`, add a `glance:` sub-section with: `heading: "Collection at a glance"`, `titles: "%{count} titles"`, `volumes: "%{count} volumes"`, `active_loans: "%{count} active loans"`, `signin_to_view_loans: "Sign in to view loans"`.
  - [ ] In `locales/fr.yml` under `dashboard:`, add the FR equivalents: `heading: "Aperçu de la collection"`, `titles: "%{count} titres"`, `volumes: "%{count} volumes"`, `active_loans: "%{count} prêts en cours"`, `signin_to_view_loans: "Connectez-vous pour voir les prêts"`.
  - [ ] **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper — keys start at root (per `CLAUDE.md` "Key Patterns / i18n").
  - [ ] After editing locale files, run `touch src/lib.rs && cargo build` to force proc-macro recompilation.
- [ ] **Task 4 — Wire the handler (AC: 1, 4, 5)**
  - [ ] In `src/routes/home.rs::home` (around line 76, before the existing template construction), call `services::dashboard::collection_glance(&state.pool).await?` and bind the result.
  - [ ] Extend `HomeTemplate` (the Askama struct returned by the handler) with three new fields: `glance_titles: i64`, `glance_volumes: i64`, `glance_active_loans: i64`. Pass them from the handler.
  - [ ] Use `session.role >= Role::Librarian` (the existing `Role` enum at `src/middleware/auth.rs` derives `PartialOrd`) to compute a boolean `loans_link_visible: bool` and pass it to the template.
  - [ ] DO NOT add a new route handler. The card is rendered inline by the existing `home` handler.
- [ ] **Task 5 — Render the card in the template (AC: 1, 4, 6)**
  - [ ] In `templates/pages/home.html`, insert a new section between the hero (line ~11) and the search input (line ~14), or adjacent to the existing metadata error badge — pick the placement that respects the responsive layout and is consistent with the search-as-homepage UX (UX spec §"Home page"). Document the chosen placement in the Dev Agent Record.
  - [ ] Markup outline (Tailwind classes + i18n via `t!` macro emitted from the handler-side context, NOT inline `{{ t!(...) }}` if Askama doesn't support it — pre-translate strings in the handler and pass as fields):
    - A `<section aria-labelledby="glance-heading">` with `<h2 id="glance-heading">{{ glance_heading }}</h2>`
    - Three `<dl>`-style or list-item rows, each with the count + label
    - Title count: always an `<a href="/catalog">{{ glance_titles_label }}</a>`
    - Volume count: always an `<a href="/catalog?view=volumes">{{ glance_volumes_label }}</a>` (or `/catalog` if the `view=volumes` route doesn't exist — verify and document)
    - Loan count: `{% if loans_link_visible %}<a href="/loans">…</a>{% else %}<span aria-describedby="glance-loans-hint">…</span><span id="glance-loans-hint" class="sr-only">{{ glance_signin_hint }}</span>{% endif %}`
  - [ ] CSP: zero `style="..."`, zero `onclick=`, zero inline `<script>`. Tailwind classes only.
- [ ] **Task 6 — Unit tests (AC: 2, 3, 5, 8)**
  - [ ] Add `tests/dashboard_glance.rs` (new file) with `#[sqlx::test(migrations = "./migrations")]` tests:
    - `glance_on_empty_db_returns_zeros` — fresh schema, no fixtures, expect `(0, 0, 0)`.
    - `glance_excludes_soft_deleted_and_returned` — seed: 3 active titles + 1 soft-deleted; 5 active volumes + 2 soft-deleted; 4 loans of which 1 returned + 1 soft-deleted; expect `(3, 5, 2)`.
    - `glance_is_single_round_trip` — instrument via `sqlx::Executor` query log if feasible; otherwise document the SQL text via `.fetch_one` source. (Lightweight check; if instrumentation is non-trivial, skip and rely on code review of the SQL.)
  - [ ] Add handler-level rendering tests in `src/routes/home.rs` `mod tests` (or a sibling test module) — follow the existing pattern at `src/routes/home.rs:411-559`. Tests:
    - `home_anonymous_renders_glance_no_loans_link` — invoke the handler with a `Session { role: Role::Anonymous, .. }`, render the template to a string, assert it contains the three i18n labels AND does NOT contain `href="/loans"`.
    - `home_librarian_renders_glance_with_loans_link` — same but with `Role::Librarian`, assert `href="/loans"` IS present.
- [ ] **Task 7 — E2E spec (AC: 9)**
  - [ ] Extend `tests/e2e/specs/journeys/home.spec.ts` with two new test cases:
    - `glance card visible to anonymous, loan count is not a link` — load `/`, verify the card heading text matches `/Collection at a glance|Aperçu de la collection/i`, verify three count entries, assert the loan-count parent is NOT an `<a>` (use `page.locator(...).evaluateAll(els => els.every(e => e.tagName !== 'A'))` or a similar assertion).
    - `glance card visible to librarian, loan count navigates to /loans` — `await loginAs(page, "librarian")`, load `/`, click the loan-count link, `await page.waitForURL(/\/loans/)`.
  - [ ] Use `loginAs(page, "librarian")` from `tests/e2e/helpers/auth.ts`. Do NOT inject `DEV_SESSION_COOKIE` (per `CLAUDE.md` Foundation Rule #7 + the parallel-safety hard rule).
  - [ ] Use i18n-aware regex matchers — both EN + FR strings.
  - [ ] No `waitForTimeout` calls — the CI grep gate enforced by the `e2e` job will fail the PR.
- [ ] **Task 8 — Verify and document (AC: 1–9)**
  - [ ] Run locally before push (per `CLAUDE.md` Foundation Rule #13):
    - `cargo check && cargo clippy -- -D warnings`
    - `cargo test` (unit + the new `tests/dashboard_glance.rs`)
    - `./scripts/e2e-reset.sh` then `cd tests/e2e && npx playwright test specs/journeys/home.spec.ts` (or the equivalent if running Docker stack)
  - [ ] Run `cargo sqlx prepare` if any new query macro was added (Task 2 uses dynamic queries, so likely a no-op — verify no `.sqlx/` diff).
  - [ ] Update Dev Agent Record at the bottom of this file: list of files touched, choice for `volume count` link target (with rationale), card placement decision in the home template, anything surprising encountered.

## Dev Notes

### Source tree references

The dev agent should not need to reinvent any of these. All paths are relative to repo root (`/home/gcorbaz/Synology/devel/mybibli/`).

| Concern | File / location | Notes |
|---|---|---|
| Home route registration | `src/routes/mod.rs:87` | `.route("/", axum::routing::get(home::home))` — no change needed |
| Home handler | `src/routes/home.rs:76-224` | extend the existing handler; do NOT create a parallel route |
| Home template | `templates/pages/home.html` (174 lines) | extends `layouts/base.html`; section blocks: hero (8-11), search (14-33), filter tags (36-65), metadata error badge (67-74, role-gated), browse results (105-139), pagination (142-169) |
| Session extractor & Role enum | `src/middleware/auth.rs` | `Role: Anonymous < Librarian < Admin` (derives `PartialOrd`) — use `session.role >= Role::Librarian` |
| Existing role-aware template gating | `templates/pages/home.html:112` | `{% if role == "librarian" || role == "admin" %}` — reuse this pattern, NOT a fresh approach |
| Existing role-aware handler gating | `src/routes/home.rs:154-163` | the `metadata_error_count` block — pattern: handler computes the value or skips, template trusts the value |
| Count query reference patterns | `src/models/volume_state.rs:177-184` (count_usage); `src/models/volume_state.rs:257-273` (count_active_loans_for_state) | use the `sqlx::query_as::<_, (i64,)>` form for ad-hoc counts; `query_scalar::<_, i64>` is also fine |
| Per-model files where count_active goes | `src/models/title.rs`, `src/models/volume.rs`, `src/models/loan.rs` | none currently has `count_active`; add it (see Task 1) |
| Existing service organization | `src/services/` (admin_health.rs, locking.rs, soft_delete.rs, etc.) | new `dashboard.rs` belongs here; register in `src/services/mod.rs` |
| i18n locales | `locales/en.yml` / `locales/fr.yml` | top-level sections include `dashboard:` (already used for `dashboard.metadata_errors`); extend with `dashboard.glance.*` sub-keys. **NO `en:`/`fr:` top-level wrapper.** |
| Templates audit (CSP enforcement) | `src/templates_audit.rs::no_inline_markup_in_templates` + `hx_confirm_matches_allowlist` (post-9-14: empty) | `cargo test` runs both — must stay green |
| Test (DB-backed integration) pattern | `tests/search_filter_browse.rs:57-87` | `#[sqlx::test(migrations = "./migrations")]` — the dominant pattern in this repo |
| Test (handler render, no DB) pattern | `src/routes/home.rs:411-559` | `#[test]` + `#[cfg(test)]` modules co-located with the handler — reuse for the role-aware HTML assertions |
| E2E helpers | `tests/e2e/helpers/auth.ts` (`loginAs`), `tests/e2e/helpers/isbn.ts`, `tests/e2e/helpers/scanner.ts` | `loginAs(page, "librarian")` — typed union, do not pass other strings |
| E2E spec for `/` | `tests/e2e/specs/journeys/home.spec.ts` (basic h1 + title + CSS) and `home-search.spec.ts` (search interactions) | extend `home.spec.ts` for the glance card tests |

### Anti-patterns to avoid

- **Three independent `fetch_one` calls.** AC2 explicitly mandates a single round-trip. Use the SELECT-with-three-subqueries pattern shown in Task 2.
- **Adding the loan count to anonymous-visible HTML as `<a href="/loans">`.** AC5 enforces this with a byte-level assertion. Even if the link would 401-redirect, leaking it leaks a route surface to anonymous users — unacceptable per the project's role-aware visibility discipline (FR59 / Story 9-8 will tighten the same principle further).
- **Using `sqlx::query!` macro for the new query.** That requires `cargo sqlx prepare` and a `.sqlx/` diff in the PR. The codebase already uses dynamic `query_as` for ad-hoc counts (see `volume_state.rs`); stay consistent.
- **Inline styles for the card.** `style="text-align: center"` will trip `templates_audit.rs::no_inline_markup_in_templates` and fail `cargo test`. Use Tailwind utilities exclusively.
- **Adding a new route like `GET /dashboard` or `GET /home/glance`.** The card is part of `/`. Adding a fragment-fetch route for it would over-engineer this story (the counts are cheap; HTMX async refresh is not in scope).
- **Empty-state-hiding the card.** AC3 requires the card to render even on a fresh DB. Story 9-15 will handle the broader empty-catalog UX — do not pre-empt it here.
- **Tooltip implementation for the anonymous "Sign in to view loans" hint.** UX-DR19 / Story 9-19 ships the actual Tooltip component; here you only need an `aria-describedby` link to a `class="sr-only"` text node so screen readers get the explanation. Sighted users will pick up the tooltip when 9-19 ships.
- **Re-translating reference data values or anything from `locales/*.yml`.** Locale files are i18n-only for UI labels (NFR41 — reference data not translated). The card's text labels are UI labels, so they ARE translated; the counts themselves are numbers (locale-formatted via Rust's `format!` if FR uses non-breaking spaces — keep it simple and use plain `i64` formatting, no thousands separator in v1).

### Architecture compliance

- **Error handling:** Any DB failure in `collection_glance` returns `AppError::Database` via `?` — do not introduce a new error variant.
- **Logging:** Use `tracing::debug!` at most, only inside the service function if needed. Counts are not interesting at info-level.
- **DB query discipline:** All three subqueries already include `deleted_at IS NULL` (per `CLAUDE.md` "Key Patterns / DB queries"). The loans subquery additionally needs `returned_at IS NULL` for "active" semantics (per FR48 + AR23).
- **HTMX:** Not applicable — the card is rendered server-side on full page load. No `hx-*` attributes, no OOB swaps.
- **CSP middleware:** Already wraps the handler outermost. No work needed.
- **Optimistic locking:** Not applicable — this is read-only.
- **Pool access:** The handler already has access to `state.pool: DbPool`. Do not introduce a new connection.

### Library / framework requirements

- **Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + Tailwind CSS v4 + HTMX 2.0** — no version changes. No new dependencies. If you find yourself reaching for a new crate, stop and re-read this section.
- **rust_i18n** — already wired. The `t!` macro is invoked from Rust (handler) — pre-translate strings in the handler, pass them as `&str` fields on the Askama template struct. Askama itself has limited support for runtime-locale macro calls; the project pattern is "translate in handler, pass to template".

### File structure requirements

The full set of files this story creates or modifies (no others — if you find yourself touching unrelated files, stop and reconsider):

| File | Action | Rough size |
|---|---|---|
| `src/services/dashboard.rs` | **create** | ~40 lines (struct + 1 fn + tests) |
| `src/services/mod.rs` | **edit** | +1 line (`pub mod dashboard;`) |
| `src/models/title.rs` | **edit** | +~10 lines (one `count_active` fn) |
| `src/models/volume.rs` | **edit** | +~10 lines |
| `src/models/loan.rs` | **edit** | +~10 lines |
| `src/routes/home.rs` | **edit** | +~15 lines in handler + extend `HomeTemplate` struct + `mod tests` additions |
| `templates/pages/home.html` | **edit** | +~25 lines for the card section |
| `locales/en.yml` | **edit** | +~6 lines under `dashboard:` |
| `locales/fr.yml` | **edit** | +~6 lines under `dashboard:` |
| `tests/dashboard_glance.rs` | **create** | ~80 lines (3 `#[sqlx::test]` cases) |
| `tests/e2e/specs/journeys/home.spec.ts` | **edit** | +~30 lines (2 new test cases) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **edit** | only the `9-1-...` line + `last_updated` (per CLAUDE.md rule 16) |

### Testing requirements

- **Coverage target:** every AC has at least one test (unit OR E2E).
- **Soft-delete exclusion is the load-bearing invariant** for this story. The mixed-dataset test in `tests/dashboard_glance.rs::glance_excludes_soft_deleted_and_returned` is the regression guard. If a future migration changes a soft-delete column name or semantics, this test catches it.
- **Role-aware HTML assertion** is the second load-bearing test (AC5). It must use the actual `Session` extractor type, not a mock — the extractor's behavior is the contract.
- **E2E parallelism** — keep the new tests independent (no shared session). `loginAs` per test (in `beforeEach` if a block is reused).

### Project structure notes

This story aligns cleanly with the existing structure — no variances. The new `src/services/dashboard.rs` follows the established `src/services/` pattern (compare to `admin_health.rs` which also assembles per-table counts for the Health admin tab). The 3 new model methods (`count_active`) keep the per-table SQL co-located with each model, consistent with `volume_state.rs::count_usage` and `count_active_loans_for_state`.

## References

- [Story 9.1 spec — `_bmad-output/planning-artifacts/epics.md` lines 1210–1224](../planning-artifacts/epics.md)
- [Epic 9 scope note + split philosophy — `epics.md` lines 1204–1207](../planning-artifacts/epics.md)
- [PRD FR55 — `_bmad-output/planning-artifacts/prd.md:689`](../planning-artifacts/prd.md)
- [PRD FR65 (anonymous browse) and FR59 (loan-status role gating) for the role-aware visibility principle — `prd.md`](../planning-artifacts/prd.md)
- [Architecture AR8, AR15, AR16, AR17 — `_bmad-output/planning-artifacts/architecture.md`](../planning-artifacts/architecture.md) (handler/template wiring, error pipeline, middleware order, soft-delete query naming)
- [UX spec §"Home page" + UX-DR24 design tokens — `_bmad-output/planning-artifacts/ux-design-specification.md`](../planning-artifacts/ux-design-specification.md)
- [`CLAUDE.md` — Key Patterns / Foundation Rules (#7 E2E smoke, #11 GitHub Issues, #12 ≤ 2000 LOC, #13 local testing, #14 one-branch-one-story, #15 draft PR, #16 sprint-status ownership, #18 CI gating)](../../CLAUDE.md)

## Dev Agent Record

### Agent Model Used

_To be filled by dev agent (e.g., `claude-opus-4-7`)._

### Debug Log References

_To be filled during implementation._

### Completion Notes List

_To be filled during implementation. Document at minimum:_
- _Volume count link target chosen (`/catalog?view=volumes` or fallback) and why._
- _Card placement in `templates/pages/home.html` (above search? below hero? adjacent to metadata badge?) and why._
- _Anything surprising encountered (existing helper found, schema quirk, i18n key collision, etc.)._

### File List

_To be filled by dev agent — exhaustive list of files touched, including the sprint-status.yaml line update._
