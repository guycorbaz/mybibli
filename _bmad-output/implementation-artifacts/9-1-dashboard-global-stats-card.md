# Story 9.1: Dashboard — global stats card

Status: review

## Story

As any user (anonymous or authenticated),
I want to see global collection stats on the home page,
so that I get an immediate sense of the catalog's size at a glance.

## Acceptance Criteria

1. **AC1 — Card presence and structure.** Given the home page (`/`), when it renders for any role (Anonymous, Librarian, Admin), then a "Collection at a glance" card is visible and displays exactly three counts: total active titles, total active volumes, total active loans — each computed by excluding `deleted_at IS NOT NULL` rows. For loans, "active" additionally requires `returned_at IS NULL`.
2. **AC2 — Single round-trip query.** Given the counts, when computed, then they come from a single SQL round-trip (one query with three sub-counts) — not three separate `fetch_one` calls. No N+1, no per-entity lookup. The query lives in a service or model function with type signature `Result<CollectionGlance, AppError>` where `CollectionGlance` is a small struct with three `i64` fields.
3. **AC3 — Empty DB renders the card.** Given a fresh DB, when the counts are zero, then the card still renders with "0 titles", "0 volumes", "0 active loans" — no empty-state hiding here. The broader "empty catalog" StatusMessage UX is a separate concern handled by story 9-15; do NOT introduce it in this story.
4. **AC4 — Role-aware links on count lines.** Given the card, when rendered, then each count line is a clickable link as follows:
    - **Title count** → `/catalog`. All roles get a real `<a href>`.
    - **Volume count** → `/catalog` (same target as the title count; v1 has no volume-only browse route — verified absent in `src/routes/mod.rs`). All roles get a real `<a href>`. If a future story adds a volume-specific browse, the link target updates trivially.
    - **Loan count** → `/loans` ONLY when `session.role >= Role::Librarian`. For Anonymous, the count is rendered as plain text inside a `<span aria-describedby="glance-loans-hint">` paired with a sibling `<span id="glance-loans-hint" class="sr-only">{{ glance_signin_hint }}</span>` carrying the i18n text "Sign in to view loans" / "Connectez-vous pour voir les prêts". The visible Tooltip component itself ships with story 9-19 — here we only need the screen-reader-accessible markup.
5. **AC5 — Anonymous never receives the loan link in HTML.** Given the role-aware link generation, when tested, then the rendered HTML for an anonymous request does NOT contain `href="/loans"` anywhere (regression guard against accidental over-rendering). This is a byte-level assertion in the unit test for the handler.
6. **AC6 — CSP compliance.** The card uses Tailwind utility classes only — no `style="..."`, no `<style>` block, no inline event handlers. The audit test in `src/templates_audit.rs::no_inline_markup_in_templates` must continue to pass after this change.
7. **AC7 — i18n EN + FR with proper pluralization.** Use rust_i18n's `one:`/`other:` branching pattern for the three count labels (titles, volumes, active_loans) — that is **12 YAML entries total** (3 keys × 2 forms × 2 languages) — following the existing project pattern in `locales/en.yml` (see `node_type_renamed_cascaded_one` / `_other` for reference). The `heading` and `signin_to_view_loans` keys do NOT branch (no count). Invocation: `t!("dashboard.glance.titles", count = n, locale = loc)`. After editing locale files, run `touch src/lib.rs && cargo build` to force the i18n proc macro to re-read.
8. **AC8 — Unit tests.** Tests must cover:
    - (a) count query returns correct values on empty DB (0/0/0);
    - (b) count query returns correct values on a mixed dataset including soft-deleted titles, soft-deleted volumes, returned loans, and at least one of each active;
    - (c) handler-level test for an anonymous session asserting the rendered HTML (i) contains the three counts, (ii) does NOT contain `href="/loans"`, (iii) contains BOTH `aria-describedby="glance-loans-hint"` AND a sibling `<span id="glance-loans-hint" class="sr-only">…</span>` whose text matches the EN or FR sign-in hint (linkage verification — both the reference and its target must coexist);
    - (d) handler-level test for a Librarian session asserting the rendered HTML contains `href="/loans"` linked to the loan count and does NOT contain the `aria-describedby="glance-loans-hint"` reference (no orphan markup).
9. **AC9 — E2E smoke.** A new spec or extension to `tests/e2e/specs/journeys/home.spec.ts` covers:
    - anonymous → `/` → verify card heading matches `/Collection at a glance|Aperçu de la collection/i`, verify the three count rows are present, then `await expect(page.locator('a[href="/loans"]')).toHaveCount(0)`;
    - login as librarian via `loginAs(page, "librarian")` → `/` → `await expect(page.locator('a[href="/loans"]')).toHaveCount(1)`, click the link, `await page.waitForURL(/\/loans/)`.
    - Use i18n-aware regex matchers (`/Active loans|Prêts en cours/i`) for any user-visible text assertion.

## Tasks / Subtasks

- [x] **Task 1 — Add `count_active()` to the three models (AC: 2, 3, 8)**
  - [x] In `src/models/title.rs`, add `pub async fn count_active(pool: &DbPool) -> Result<i64, AppError>` using `SELECT COUNT(*) FROM titles WHERE deleted_at IS NULL`. Follow the pattern in `src/models/volume_state.rs:177-184`.
  - [x] In `src/models/volume.rs`, add `pub async fn count_active(pool: &DbPool) -> Result<i64, AppError>` using the same pattern on `volumes`.
  - [x] In `src/models/loan.rs`, add `pub async fn count_active(pool: &DbPool) -> Result<i64, AppError>` filtering on `returned_at IS NULL AND deleted_at IS NULL`. Follow the pattern in `src/models/volume_state.rs::count_active_loans_for_state` (lines 257-273).
  - [x] These per-model `count_active()` are intentionally added even though AC2 mandates a single-round-trip query, because they will be used by other Epic 9 stories (9-4, 9-5) and keep the per-table SQL co-located with each model.
- [x] **Task 2 — Add `CollectionGlance` aggregate query in a service (AC: 1, 2)**
  - [x] Create `src/services/dashboard.rs` (new module) — register it in `src/services/mod.rs` with `pub mod dashboard;`.
  - [x] Define `pub struct CollectionGlance { pub titles: i64, pub volumes: i64, pub active_loans: i64 }` deriving `Debug, Clone, sqlx::FromRow`.
  - [x] Implement `pub async fn collection_glance(pool: &DbPool) -> Result<CollectionGlance, AppError>` that runs a single SQL query of the form:
    ```sql
    SELECT
      (SELECT COUNT(*) FROM titles WHERE deleted_at IS NULL)  AS titles,
      (SELECT COUNT(*) FROM volumes WHERE deleted_at IS NULL) AS volumes,
      (SELECT COUNT(*) FROM loans WHERE returned_at IS NULL AND deleted_at IS NULL) AS active_loans
    ```
    Use `sqlx::query_as::<_, CollectionGlance>` + `.fetch_one(pool)`. This is a single round-trip per AC2 — verified by the SQL shape (1 SELECT, 0 join, 3 sub-counts) at code review time, no programmatic instrumentation needed.
  - [x] DO NOT use `sqlx::query!` macro for this — it requires `.sqlx/` cache regeneration. Use the dynamic `query_as` form.
- [x] **Task 3 — Add i18n keys (AC: 7)**
  - [x] In `locales/en.yml` under `dashboard:`, add a `glance:` sub-section with these 8 keys:
    - `heading: "Collection at a glance"`
    - `titles_one: "%{count} title"` and `titles_other: "%{count} titles"`
    - `volumes_one: "%{count} volume"` and `volumes_other: "%{count} volumes"`
    - `active_loans_one: "%{count} active loan"` and `active_loans_other: "%{count} active loans"`
    - `signin_to_view_loans: "Sign in to view loans"`
  - [x] In `locales/fr.yml` under `dashboard:`, add the FR equivalents (8 keys):
    - `heading: "Aperçu de la collection"`
    - `titles_one: "%{count} titre"` and `titles_other: "%{count} titres"`
    - `volumes_one: "%{count} volume"` and `volumes_other: "%{count} volumes"`
    - `active_loans_one: "%{count} prêt en cours"` and `active_loans_other: "%{count} prêts en cours"`
    - `signin_to_view_loans: "Connectez-vous pour voir les prêts"`
  - [x] **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper — keys start at root (per `CLAUDE.md` "Key Patterns / i18n").
  - [x] After editing locale files, run `touch src/lib.rs && cargo build` to force proc-macro recompilation.
- [x] **Task 4 — Wire the handler (AC: 1, 4, 5)**
  - [x] In `src/routes/home.rs::home` (around line 76, before the existing template construction), call `services::dashboard::collection_glance(&state.pool).await?` and bind the result.
  - [x] Extend `HomeTemplate` (the Askama struct returned by the handler) with new fields: `glance_heading: String`, `glance_titles_label: String`, `glance_volumes_label: String`, `glance_active_loans_label: String`, `glance_signin_hint: String`, `glance_titles_count: i64`, `glance_volumes_count: i64`, `glance_active_loans_count: i64`, `loans_link_visible: bool`. Pre-translate the 4 label strings + hint in the handler via `rust_i18n::t!(...)` calls.
  - [x] Use `session.role >= Role::Librarian` (the existing `Role` enum at `src/middleware/auth.rs` derives `PartialOrd`) to compute `loans_link_visible`.
  - [x] DO NOT add a new route handler. The card is rendered inline by the existing `home` handler.
- [x] **Task 5 — Render the card in the template (AC: 1, 4, 6)**
  - [x] In `templates/pages/home.html`, insert the card **between the filter tags (closing tag at line ~65) and the `#browse-results` div (opening at line ~106)**. Rationale: this placement keeps the search field above the fold (no CLS on tablet/mobile), sits OUTSIDE the HTMX swap target `#browse-results` so the card stays visible during search interactions, and is consistent with the search-as-homepage UX (UX spec §"Home page"). The dev agent may deviate only with explicit justification documented in the Dev Agent Record.
  - [x] Markup outline (Tailwind utility classes; all i18n strings come pre-translated as `String` fields on `HomeTemplate`):
    - `<section aria-labelledby="glance-heading">` with `<h2 id="glance-heading">{{ glance_heading }}</h2>`
    - Three `<dl>`-style or list-item rows, each rendering its pre-translated label (which already includes the count via `%{count}` interpolation)
    - Title count: always `<a href="/catalog">{{ glance_titles_label }}</a>`
    - Volume count: always `<a href="/catalog">{{ glance_volumes_label }}</a>`
    - Loan count: `{% if loans_link_visible %}<a href="/loans">{{ glance_active_loans_label }}</a>{% else %}<span aria-describedby="glance-loans-hint">{{ glance_active_loans_label }}</span><span id="glance-loans-hint" class="sr-only">{{ glance_signin_hint }}</span>{% endif %}`
  - [x] CSP: zero `style="..."`, zero `onclick=`, zero inline `<script>`. Tailwind classes only.
- [x] **Task 6 — Unit tests (AC: 2, 3, 5, 8)**
  - [x] Add `tests/dashboard_glance.rs` (new file) with `#[sqlx::test(migrations = "./migrations")]` tests:
    - `glance_on_empty_db_returns_zeros` — fresh schema, no fixtures, expect `(0, 0, 0)`.
    - `glance_excludes_soft_deleted_and_returned` — seed: 3 active titles + 1 soft-deleted; 5 active volumes + 2 soft-deleted; 4 loans of which 1 returned + 1 soft-deleted; expect `(3, 5, 2)`.
  - [x] Add handler-level rendering tests in `src/routes/home.rs` `mod tests` (or a sibling test module) — follow the existing pattern at `src/routes/home.rs:411-559`. Tests:
    - `home_anonymous_renders_glance_no_loans_link` — invoke the handler with a `Session { role: Role::Anonymous, .. }`, render the template to a string, then assert: contains the three i18n labels (EN locale, plural form); does NOT contain `href="/loans"`; contains BOTH `aria-describedby="glance-loans-hint"` AND `id="glance-loans-hint"` (linkage check); the span carrying `id="glance-loans-hint"` carries class `sr-only` and contains the EN sign-in hint text.
    - `home_librarian_renders_glance_with_loans_link` — same but with `Role::Librarian`; assert `href="/loans"` IS present and does NOT contain `aria-describedby="glance-loans-hint"` (no orphan).
- [x] **Task 7 — E2E spec (AC: 9)**
  - [x] Extend `tests/e2e/specs/journeys/home.spec.ts` with two new test cases:
    - `glance card visible to anonymous, loan count is not a link` — load `/`, verify the card heading matches `/Collection at a glance|Aperçu de la collection/i`, verify the three count rows are present, then `await expect(page.locator('a[href="/loans"]')).toHaveCount(0)`.
    - `glance card visible to librarian, loan count navigates to /loans` — `await loginAs(page, "librarian")`, load `/`, `await expect(page.locator('a[href="/loans"]')).toHaveCount(1)`, click the link, `await page.waitForURL(/\/loans/)`.
  - [x] Use `loginAs(page, "librarian")` from `tests/e2e/helpers/auth.ts`. Do NOT inject `DEV_SESSION_COOKIE` (per `CLAUDE.md` Foundation Rule #7 + the parallel-safety hard rule).
  - [x] Use i18n-aware regex matchers — both EN + FR strings.
  - [x] No `waitForTimeout` calls — the CI grep gate enforced by the `e2e` job will fail the PR.
- [x] **Task 8 — Verify and document (AC: 1–9)**
  - [x] Run locally before push (per `CLAUDE.md` Foundation Rule #13):
    - `cargo check && cargo clippy -- -D warnings`
    - `cargo test` (unit + the new `tests/dashboard_glance.rs`)
    - `./scripts/e2e-reset.sh` then `cd tests/e2e && npx playwright test specs/journeys/home.spec.ts` (or the equivalent if running Docker stack)
  - [x] Run `cargo sqlx prepare` if any new query macro was added (Task 2 uses dynamic queries, so likely a no-op — verify no `.sqlx/` diff).
  - [x] Update Dev Agent Record at the bottom of this file: list of files touched, card placement decision in the home template (or justification for deviating from the recommended placement), anything surprising encountered.

## Dev Notes

### Source tree references

The dev agent should not need to reinvent any of these. All paths are relative to repo root (`/home/gcorbaz/Synology/devel/mybibli/`).

| Concern | File / location | Notes |
|---|---|---|
| Home route registration | `src/routes/mod.rs:87` | `.route("/", axum::routing::get(home::home))` — no change needed |
| Home handler | `src/routes/home.rs:76-224` | extend the existing handler; do NOT create a parallel route |
| Home template | `templates/pages/home.html` (174 lines) | extends `layouts/base.html`; section blocks: hero (8-11), search (14-33), filter tags (36-65), metadata error badge (67-74, role-gated), browse results (105-139), pagination (142-169) |
| HTMX swap target on `/` | `templates/pages/home.html:25` (`hx-target="#browse-results"`) + `#browse-results` div at line ~106 | the card MUST sit outside `#browse-results` so it survives search swaps — the recommended insertion point (lines ~65–106) satisfies this |
| Session extractor & Role enum | `src/middleware/auth.rs` | `Role: Anonymous < Librarian < Admin` (derives `PartialOrd`) — use `session.role >= Role::Librarian` |
| Existing role-aware template gating | `templates/pages/home.html:112` | `{% if role == "librarian" || role == "admin" %}` — reuse this pattern, NOT a fresh approach |
| Existing role-aware handler gating | `src/routes/home.rs:154-163` | the `metadata_error_count` block — pattern: handler computes the value or skips, template trusts the value |
| Canonical i18n pre-translation example | `src/routes/home.rs:207-212` (`label_metadata_errors: rust_i18n::t!("dashboard.metadata_errors", locale = loc, count = metadata_error_count).to_string()`) → rendered at `templates/pages/home.html:71` as plain `{{ label_metadata_errors }}` | **replicate this pattern for the four glance labels (heading + 3 counts) plus the sign-in hint** |
| Pluralization YAML reference | `locales/en.yml` keys ending in `_one` / `_other` (e.g., `node_type_renamed_cascaded_one` / `_other`) | when the `t!` call passes `count = n`, rust_i18n auto-selects `_one` for n=1, `_other` otherwise |
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
- **Linking the volume count to a `?view=volumes` route.** That route does NOT exist in v1. Both title and volume counts link to `/catalog`. If a future story adds a volume-only browse, the link target updates trivially.
- **Using `sqlx::query!` macro for the new query.** That requires `cargo sqlx prepare` and a `.sqlx/` diff in the PR. The codebase already uses dynamic `query_as` for ad-hoc counts (see `volume_state.rs`); stay consistent.
- **Inline styles for the card.** `style="text-align: center"` will trip `templates_audit.rs::no_inline_markup_in_templates` and fail `cargo test`. Use Tailwind utilities exclusively.
- **Adding a new route like `GET /dashboard` or `GET /home/glance`.** The card is part of `/`. Adding a fragment-fetch route for it would over-engineer this story (the counts are cheap; HTMX async refresh is not in scope).
- **Empty-state-hiding the card.** AC3 requires the card to render even on a fresh DB. Story 9-15 will handle the broader empty-catalog UX — do not pre-empt it here.
- **Tooltip implementation for the anonymous "Sign in to view loans" hint.** UX-DR19 / Story 9-19 ships the actual Tooltip component; here you only need an `aria-describedby` link to a `class="sr-only"` text node so screen readers get the explanation. Sighted users will pick up the visible tooltip when 9-19 ships.
- **Calling `t!` from inside the Askama template.** The project pattern is "translate in handler, pass to template" (canonical example: `src/routes/home.rs:207-212`). Stay consistent.
- **Singular-only or "(s)" hack labels.** AC7 mandates `_one`/`_other` branching. Do NOT write `"%{count} title(s)"` — it ships poor i18n grammar in both EN and FR (especially FR where `prêt`/`prêts` differ).
- **Re-translating reference data values or anything from `locales/*.yml`.** Locale files are i18n-only for UI labels (NFR41 — reference data not translated). The card's text labels are UI labels, so they ARE translated; the counts themselves are numbers (locale-formatted via Rust's `format!` if FR uses non-breaking spaces — keep it simple and use plain `i64` formatting, no thousands separator in v1).

### Architecture compliance

- **Error handling:** Any DB failure in `collection_glance` returns `AppError::Database` via `?` — do not introduce a new error variant.
- **Logging:** Use `tracing::debug!` at most, only inside the service function if needed. Counts are not interesting at info-level.
- **DB query discipline:** All three subqueries already include `deleted_at IS NULL` (per `CLAUDE.md` "Key Patterns / DB queries"). The loans subquery additionally needs `returned_at IS NULL` for "active" semantics (per FR48 + AR23).
- **HTMX:** Not applicable to the card itself — it renders server-side on full page load. No `hx-*` attributes, no OOB swaps. Important coexistence note: the existing search field on `/` swaps `#browse-results` (`hx-target="#browse-results"` at `templates/pages/home.html:25`); the card sits OUTSIDE that swap target so it stays visible during search and filter interactions — verify this placement invariant when the card is inserted (Task 5 already targets the safe zone between filter tags and `#browse-results`).
- **CSP middleware:** Already wraps the handler outermost. No work needed.
- **Optimistic locking:** Not applicable — this is read-only.
- **Pool access:** The handler already has access to `state.pool: DbPool`. Do not introduce a new connection.

### Library / framework requirements

- **Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + Tailwind CSS v4 + HTMX 2.0** — no version changes. No new dependencies. If you find yourself reaching for a new crate, stop and re-read this section.
- **rust_i18n** — already wired. The `t!` macro is invoked from Rust (handler) — pre-translate strings in the handler, pass them as `String` fields on the Askama template struct. Askama itself has limited support for runtime-locale macro calls; the project pattern is "translate in handler, pass to template". Canonical example: `src/routes/home.rs:207-212` pre-translates `dashboard.metadata_errors` with `count` interpolation and passes `label_metadata_errors: String` to the template (rendered at `templates/pages/home.html:71` as plain `{{ label_metadata_errors }}`). Replicate this pattern for the four glance labels (heading + 3 counts) plus the sign-in hint. For the count labels, the `count =` parameter triggers rust_i18n's `_one`/`_other` auto-selection — that is why the YAML keys are paired (Task 3).

### File structure requirements

The full set of files this story creates or modifies (no others — if you find yourself touching unrelated files, stop and reconsider):

| File | Action | Rough size |
|---|---|---|
| `src/services/dashboard.rs` | **create** | ~40 lines (struct + 1 fn + tests) |
| `src/services/mod.rs` | **edit** | +1 line (`pub mod dashboard;`) |
| `src/models/title.rs` | **edit** | +~10 lines (one `count_active` fn) |
| `src/models/volume.rs` | **edit** | +~10 lines |
| `src/models/loan.rs` | **edit** | +~10 lines |
| `src/routes/home.rs` | **edit** | +~25 lines in handler + extend `HomeTemplate` struct + `mod tests` additions |
| `templates/pages/home.html` | **edit** | +~25 lines for the card section |
| `locales/en.yml` | **edit** | +~10 lines under `dashboard:` (8 keys) |
| `locales/fr.yml` | **edit** | +~10 lines under `dashboard:` (8 keys) |
| `tests/dashboard_glance.rs` | **create** | ~70 lines (2 `#[sqlx::test]` cases) |
| `tests/e2e/specs/journeys/home.spec.ts` | **edit** | +~30 lines (2 new test cases) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **edit** | only the `9-1-...` line + `last_updated` (per CLAUDE.md rule 16) |

### Testing requirements

- **Coverage target:** every AC has at least one test (unit OR E2E).
- **Soft-delete exclusion is the load-bearing invariant** for this story. The mixed-dataset test in `tests/dashboard_glance.rs::glance_excludes_soft_deleted_and_returned` is the regression guard. If a future migration changes a soft-delete column name or semantics, this test catches it.
- **Role-aware HTML assertion** is the second load-bearing test (AC5 + AC8c). It must use the actual `Session` extractor type, not a mock — the extractor's behavior is the contract. The aria-describedby linkage check (AC8c iii) prevents an orphan-reference accessibility regression.
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

`claude-opus-4-7` (1M context).

### Debug Log References

- `cargo test --lib` — 624 passed, 0 failed (after `dashboard.glance.titles` initially failed `i18n::audit::tests::all_t_keys_have_both_locales` because the audit expects literal scalar leaves, not the rust_i18n `_one`/`_other` auto-resolved prefix; resolution: branch on `count == 1` in the handler with two literal `t!()` calls per count).
- `cargo test --test dashboard_glance` — 2 passed (empty DB → zeros; mixed dataset → 3/5/2 with soft-deleted titles + soft-deleted volumes + returned loan + soft-deleted loan correctly excluded).
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo sqlx prepare --check --workspace -- --all-targets` — no diff (Task 2 uses dynamic `query_as`, Task 1 uses dynamic `query_as` — no `.sqlx/` regeneration needed).
- Manual smoke via `curl http://localhost:8080/`: FR locale (default) renders "Aperçu de la collection / 0 titres / 0 prêts en cours" with `aria-describedby="glance-loans-hint"` paired to `id="glance-loans-hint"` carrying the `sr-only` "Connectez-vous pour voir les prêts" hint and zero `href="/loans"` occurrences. `?lang=en` flips to "Collection at a glance / 0 titles / 0 active loans / Sign in to view loans".
- E2E locally blocked by `EACCES` on `tests/e2e/test-results/` (artefacts owned by root from a prior Docker run); CI run on push will validate `home.spec.ts` end-to-end.

### Completion Notes List

- **Card placement**: implemented at the recommended location — between the metadata-error badge (line ~74) and the browse toggle (line ~78), inside the safe zone `(filter tags ~65) ↔ (#browse-results ~106)`. The card is OUTSIDE the HTMX swap target `#browse-results`, so it survives in-place during search/filter swaps. No deviation from the story spec.
- **i18n pluralization**: rust_i18n auto-resolution of `key` → `key.one`/`key.other` collides with the project's i18n audit (`src/i18n/audit.rs::all_t_keys_have_both_locales`), which expects every `t!()` first argument to be a literal scalar leaf. Resolved by branching on `count == 1` in the handler and calling two literal `t!()` keys per count (`titles_one` / `titles_other`, etc.). This preserves correct EN/FR plural grammar (notably FR `prêt` vs `prêts`) without weakening the audit.
- **Pattern reuse**: the new `src/services/dashboard.rs` deliberately mirrors `src/services/admin_health.rs::entity_counts` (also a counts builder), but uses a single SQL round-trip with three correlated subqueries instead of five separate `query_scalar` calls. Both are valid for their context: admin Health refreshes per page render and counts five tables; the home glance card targets a hot path (`/`) where the latency saving matters more.
- **Per-model `count_active()`**: added to all three of `title.rs`, `volume.rs`, `loan.rs` per Task 1 — they're not used by the glance query itself (which lives in the service) but are positioned for reuse by stories 9-4 / 9-5 (FilterTag indicators).
- **`HomeTemplate` test factory**: the existing `test_home_template_renders` test was extracted into a `make_test_home_template(role, loans_link_visible)` factory inside `mod tests` to avoid duplicating ~50 fields across the three glance-card render tests.
- **No surprises with schema or query semantics**. The single-round-trip query worked first try; the soft-delete + `returned_at` filtering matches the convention from `volume_state.rs::count_active_loans_for_state`.

### File List

| File | Action |
|---|---|
| `src/models/title.rs` | edit (added `count_active()`) |
| `src/models/volume.rs` | edit (added `count_active()`) |
| `src/models/loan.rs` | edit (added `count_active()`) |
| `src/services/dashboard.rs` | create (`CollectionGlance` + `collection_glance()` + smoke test) |
| `src/services/mod.rs` | edit (registered `pub mod dashboard;`) |
| `src/routes/home.rs` | edit (extended `HomeTemplate` with 9 fields, computed glance + plural-aware labels in handler, added 2 render tests + factory) |
| `templates/pages/home.html` | edit (inserted `#collection-glance` section between metadata badge and browse toggle) |
| `locales/en.yml` | edit (8 keys under `dashboard.glance.*`) |
| `locales/fr.yml` | edit (8 keys under `dashboard.glance.*`) |
| `tests/dashboard_glance.rs` | create (2 `#[sqlx::test]` cases — empty DB + mixed soft-deleted/returned) |
| `tests/e2e/specs/journeys/home.spec.ts` | edit (added 2 test cases — anonymous card without loan link, librarian card navigates to /loans) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | edit (9-1 → review on completion; epic-9 already in-progress from spec creation) |
| `_bmad-output/implementation-artifacts/9-1-dashboard-global-stats-card.md` | edit (Status → review, Tasks all checked, Dev Agent Record filled) |

### Change Log

- **2026-04-30** — Initial implementation. All 8 tasks complete; 624 lib tests + 2 dashboard_glance integration tests pass; clippy clean; sqlx cache unchanged. E2E validation deferred to CI (local `tests/e2e/test-results/` directory owned by root from earlier Docker runs blocks Playwright reporter writes).
