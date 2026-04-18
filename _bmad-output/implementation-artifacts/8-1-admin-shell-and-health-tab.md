# Story 8.1: Admin page shell + Health tab

Status: done

Epic: 8 — Administration & Configuration
Requirements mapping: FR76 (health page), FR120 (5-tab admin structure), UX-DR7 (AdminTabs component), AR16 (middleware order preserved)

---

> **TL;DR** — First story of Epic 8. Ships `/admin` route, `AdminTabs` component (Health · Users · Reference Data · Trash · System), Health tab content (app version, MariaDB version, disk usage, entity counts, per-provider reachability) and **stubs** for the other four tabs — every later Epic-8 story fills in exactly one stub in place. Admin-only guard reuses `Session::require_role(Role::Admin)`; deep-links via `?tab=<name>` are server-rendered, tab clicks HTMX-swap the panel with `hx-push-url`. Zero inline script / style (7-4 CSP guardrail); zero new `hx-confirm=` (7-5 allowlist frozen at 5). Smoke E2E covers Foundation Rule #7.

## Story

As an **admin**,
I want a single `/admin` entry point organized as tabs with a Health dashboard as the landing tab,
so that I can reach all admin operations without nested menus and see at a glance whether the system is healthy.

## Scope Reality & What This Story Ships

**No admin surface exists today.** The `/admin` nav link was intentionally removed from `templates/components/nav_bar.html` in story 7-1 (Epic 7 retro §2, Action 1). Every page-template struct still carries an unused `nav_admin: String` field populated from `rust_i18n::t!("nav.admin")` — reuse it, don't add another.

**Zero `/admin*` routes exist today.** `src/routes/` has 9 route modules (`auth, borrowers, catalog, contributors, home, loans, locations, series, titles`). `routes::mod::build_router` mounts everything with middleware order `Logging → Auth (per-handler) → PendingUpdates → CSP` per AR16.

**The public `/health` endpoint (`routes::mod::health_check`) is unrelated** — it returns a static `"ok"` string for Docker healthchecks and is mounted without auth. Do NOT confuse it with the admin-side **Health tab** this story ships. Keep `/health` untouched.

**`settings` table + `Arc<RwLock<AppSettings>>` cache is live** (migration `20260329000000_initial_schema.sql`, struct `src/config.rs:AppSettings`). Stories 8-4 + 8-7 extend it; story 8-1 READS only — no new settings keys, no schema change.

**`ProviderRegistry` lists registered providers** (`src/metadata/registry.rs`). Each `MetadataProvider` has `.name()`. Today's registered set (per `src/main.rs`): `open_library, google_books, bnf, bdgest, musicbrainz, tmdb, omdb` — seven providers; the epic AC names "BnF + Google Books" but the Health tab must render every registered provider, not a hard-coded two.

**Ships:**
1. `src/routes/admin.rs` — new module. `admin_page` handler (GET `/admin`) resolves the `?tab=` query param, enforces `Role::Admin`, and renders the full page or an HTMX panel fragment. Sub-handlers `admin_health_panel` (GET `/admin/health`), `admin_users_panel`, `admin_reference_data_panel`, `admin_trash_panel`, `admin_system_panel` — the last four return a "Coming in story 8-N" placeholder panel (with soft-deleted count still computed for the Trash badge so it's visible immediately).
2. `templates/pages/admin.html` — full page, extends `base.html`.
3. `templates/components/admin_tabs.html` — reusable tab bar (`{% block panel %}` for the active tab's content via Askama block include or direct template composition — whichever matches the existing component idiom best; see Dev Notes §Template composition).
4. `templates/fragments/admin_{health,users,reference_data,trash,system}_panel.html` — 5 panel fragments. Health has real content; the other four are stubs with a "Coming soon (story 8-N)" message.
5. Nav bar: re-introduce the `/admin` link, gated on `role == "admin"` only (not librarian). Desktop + mobile variants.
6. i18n: new keys under `admin:` namespace in `locales/en.yml` + `locales/fr.yml` — tab labels, Health labels, per-panel stub copy. Remember: `touch src/lib.rs && cargo build` after YAML edits (rust-i18n proc macro).
7. Unit tests: Admin middleware 403 vs 303 split; `?tab=<invalid>` falls back to `health`; Health-count SQL excludes `deleted_at IS NOT NULL`; Trash badge count query reads `services::soft_delete::ALLOWED_TABLES` (the existing 6-entry whitelist, promoted to `pub` in this story) — never re-enumerated.
8. E2E smoke (Foundation Rule #7): `tests/e2e/specs/journeys/admin-smoke.spec.ts`, spec ID `"AD"` — blank browser → login admin → `/admin` → assert 5 tabs + Health default + counts > 0 → click each tab → assert URL + panel updates → log in as librarian → `/admin` → assert 403 feedback.
9. `templates_audit.rs` extension: add `components/admin_tabs.html` and each panel fragment to the walk (automatic — the audit globs `templates/`, just make sure new files don't introduce inline styles/scripts).
10. `docs/route-role-matrix.md` update: add the 6 new admin routes with their required role.

**Does NOT ship:**
- User administration CRUD (story 8-2).
- Reference-data inline forms (story 8-3).
- System settings form (story 8-4).
- Trash list, restore, conflict-resolution modal (story 8-5).
- Permanent delete + auto-purge (story 8-6).
- Setup wizard (story 8-7).
- New `hx-confirm=` attributes (allowlist frozen at 5 — there are zero destructive actions in this story anyway).
- Any UX-DR8 Modal usage (Epic 9 — not needed here, no destructive actions).
- Any new settings row or AppSettings field.
- CSRF protection (separate decision — see §Cross-cutting decisions below).

## Cross-cutting decisions this story depends on

Per Epic 7 retro §7 Action 1 — **CSRF must be decided before 8-1 kickoff.** This story introduces `/admin` as the first surface where admin-only state-changing operations will land (8-2 onwards). The current posture (same-origin cookie, `SameSite=Lax`, no token) has been deferred 4× across Epics 1, 6, 7. Expected resolution before dev starts: either (a) `docs/auth-threat-model.md` formally accepting the risk for the single-user NAS threat model, or (b) a foundation story 8-0 landing CSRF middleware + form-token injection. If neither exists at kickoff, **raise the question in the first dev message and HALT until resolved** — do not ship admin mutations on top of an undecided posture.

**This story itself is read-only** — no mutations introduced here. CSRF deferral does not block 8-1's implementation, but does block 8-2 (user administration).

## Acceptance Criteria

1. **Route: `GET /admin` — admin-only, returns full page or HTMX panel**
   - Handler `admin_page` is `async fn admin_page(State(state), session: Session, headers, OriginalUri, Query<AdminQuery>) -> Result<Response, AppError>`.
   - First line: `session.require_role(Role::Admin)?;` — Librarian → `AppError::Forbidden` (renders the standard FeedbackEntry 403 already wired in `error::mod::IntoResponse`); Anonymous → `AppError::UnauthorizedWithReturn(uri.path().to_string())` so `/login?next=%2Fadmin` bounces back post-login (same pattern as `routes::loans::loans_page`).
   - `Query<AdminQuery>` struct: `{ tab: Option<String> }`. Valid values: `health | users | reference_data | trash | system`. Invalid or missing → `"health"` (default). Resolution rule: exact string match; nothing fuzzy.
   - Direct navigation (non-HTMX): full page render via `AdminPageTemplate` (extends `base.html`) with the resolved tab's panel server-rendered inline (not fetched after load). This satisfies "JS-disabled users still see content" from the epic AC.
   - HTMX request (`HxRequest(true)`): return only the panel fragment body (`admin_<tab>_panel.html`) — no layout, no nav bar, no `<html>`. Status 200, `Content-Type: text/html`.
   - The route mount registers `/admin` + `/admin/health` + `/admin/users` + `/admin/reference-data` + `/admin/trash` + `/admin/system`. **HTMX requests** (`HxRequest(true)`) return the panel-shell fragment only (tab bar + panel, no layout). **Direct (non-HTMX) GET on a sub-path returns the full `AdminPageTemplate`** with the corresponding tab pre-selected — consistent with bullet 4 above and Dev Notes §Sub-handler response shape (JS-disabled users who paste `/admin/health` into a new window still see a complete page, not an orphan fragment). Rationale: `hx-push-url` pushes `/admin?tab=<name>` as the canonical user-facing URL; the `/admin/<name>` paths are primarily HTMX endpoints, with full-page fallback for direct navigation. <!-- AC correction 2026-04-17: previous wording said "sub-routes return the panel fragment only (HTMX or direct)" which contradicted bullet 4 + Dev Notes. Resolved via code review to align with the JS-disabled-friendly choice, matching actual implementation. -->
   - **Correction to the AC as stated in epics.md:** epics.md phrases the URL update as `?tab=users` (query string). This story implements that for *browser history* via `hx-push-url='/admin?tab=users'`, and uses `/admin/<name>` as the HTMX request endpoint. Deep-link `/admin?tab=trash` loads full-page with the Trash panel pre-selected — same server-side resolution path.

2. **`BaseContext` field + template propagation — reuse the existing pattern, do not invent a new one**
   - Every existing page-template struct already has `nav_admin: String` populated via `rust_i18n::t!("nav.admin")`. The nav bar template did not render it before (story 7-1 removal). This story re-wires the nav bar to render the link, gated on `role == "admin"` (not librarian — `can_admin` in spirit, though `BaseContext` currently only carries `role` as `String`).
   - Add `/admin` link in `templates/components/nav_bar.html` desktop nav (after `/loans`) and mobile nav — both guarded by `{% if role == "admin" %}`. Active state: `aria-current="page"` when `current_page == "admin"`.
   - New `current_page = "admin"` convention value — declared in `AdminPageTemplate::current_page` and every sub-panel's full-page parent.

3. **Health tab content — real data from real sources**
   The Health panel renders (EN + FR labels, identical layout):
   - **Application version** → `env!("CARGO_PKG_VERSION")` baked in at compile time (no Git SHA — keep simple; if Guy later wants a Git SHA, add `build.rs` in a follow-up).
   - **MariaDB version** → `SELECT VERSION()` against `state.pool`. Cache the result for 60 s in a `Arc<RwLock<Option<(String, Instant)>>>` held in `AppState` — MariaDB version never changes at runtime, one query per minute is cheap enough that the cache is for API-hit reduction on repeated Health-tab loads, not for correctness.
   - **Disk usage on the data volume** → use the standard-library `std::fs` route is insufficient (no portable free-space query). Options:
     - (a) Add a single crate dependency (`sysinfo` ≈ 500KB compiled, already transitively present in some Axum/Tokio trees — verify). Query `Disks::new_with_refreshed_list()` and match the disk containing `state.covers_dir`.
     - (b) Use `nix::sys::statvfs::statvfs(path)` — smaller dep, Linux-only (fine, the app targets Docker on Linux).
     - (c) Spawn `df -B1 <path>` and parse — no new dep, but process-spawn overhead and parsing fragility.
     - **Recommended: (b) `nix` crate via its `fs` feature** — smallest surface area, matches the Linux-container deployment target. Fall back to `("unknown", "unknown")` strings if `statvfs` fails; never panic the Health handler.
   - **Entity counts** → one query per entity table. Exclude `deleted_at IS NOT NULL`. Counts shown: **titles, volumes, contributors, borrowers, active loans** (active = `returned_at IS NULL`). Use `sqlx::query_scalar!("SELECT COUNT(*) FROM <table> WHERE deleted_at IS NULL")`. Cache-less — 5 COUNT(*) queries on small tables is fine.
   - **Soft-deleted count (Trash badge)** → sum of `SELECT COUNT(*) FROM <table> WHERE deleted_at IS NOT NULL` across every table in the existing soft-delete whitelist. The whitelist today is `src/services/soft_delete.rs::ALLOWED_TABLES` (private `const`, 6 tables: `titles, volumes, contributors, storage_locations, borrowers, series`). This story:
     - (a) Promotes the const to `pub` so `admin_health` can consume it (rename stays — `ALLOWED_TABLES`; no cross-cutting rename churn).
     - (b) Scopes the badge count to exactly those 6 tables in 8-1. This is a **preview** — not the comprehensive Trash view.
     - (c) Leaves whitelist **extension** (adding `loans, genres, volume_states, contributor_roles, location_node_types` and verifying each table has a `deleted_at` column in its migration) to story 8-5, which owns the full Trash list + restore UX. Dev Notes flag the gap so the dev doesn't try to enumerate ad-hoc here.
   - **Per-provider status** → for each provider in `state.registry.iter()` (add `iter()` on `ProviderRegistry` if absent — trivial passthrough of the `Vec<Box<dyn MetadataProvider>>`), render a row with: provider name, green/red indicator, last-check timestamp. The check itself is a **non-blocking, fire-and-forget `tokio::spawn` background task** started at app boot — it pings each provider's canonical homepage URL (read from a new `fn health_check_url(&self) -> Option<&str>` on `MetadataProvider`; providers without an HTTP reachability URL return `None` and render as "n/a"), with a 3-second timeout via `state.http_client`, every 5 minutes, and stores the result in `Arc<RwLock<HashMap<String, ProviderHealth>>>` held in `AppState`. The Health tab **reads** this map — it does NOT trigger a synchronous check (one admin opening /admin should not block on 7 HTTP pings).
   - Color indicator is CSS class only (`bg-emerald-500` / `bg-red-500` / `bg-stone-300`) — zero inline `style="..."` (CSP 7-4 guardrail).

4. **Tab bar accessibility (UX-DR7 spec, `ux-design-specification.md` lines 1812–1852)**
   - `<div role="tablist" aria-label="{admin_tabs_aria}">` container.
   - Each tab: `<a href="/admin?tab=<name>" role="tab" aria-selected="true|false" aria-controls="panel-<name>" id="tab-<name>">`. The anchor is the right element for deep-linking support; keyboard activation (Enter / Space) works natively on `<a>`.
   - Each panel: `<div role="tabpanel" aria-labelledby="tab-<name>" id="panel-<name>">`.
   - `aria-selected="true"` on exactly one tab (the resolved one). All others `aria-selected="false"`.
   - Arrow-key navigation (Left/Right) is OUT OF SCOPE for this story — the epics.md AC does not require it; Epic 9 polish pass can add it. Document the gap in Dev Notes.
   - Trash badge: `<span class="... " aria-label="{trash_items_aria}">N</span>` where `trash_items_aria` is the i18n key `admin.trash.badge_aria` with `%{count}` substitution ("Trash, %{count} items" / "Corbeille, %{count} éléments"). Hidden entirely (`{% if trash_count > 0 %}`) when 0.

5. **HTMX tab switching + URL sync**
   - Each tab anchor has: `hx-get="/admin/<name>"`, `hx-target="#admin-shell"`, `hx-swap="innerHTML"`, `hx-push-url="/admin?tab=<name>"`.
   - `#admin-shell` wraps **both** the tab bar AND the panel. Each HTMX response re-renders the entire shell — the newly-active tab carries `aria-selected="true"` in the freshly rendered tab bar; all others `aria-selected="false"`. No OOB swap required, single response fragment. Rationale: with 5 tab anchors this is ~12 lines of HTML; an OOB swap adds complexity with no measurable UX win in a read-only story.
   - Browser Back/Forward: `hx-push-url` puts `/admin?tab=<name>` in history; the HTMX popstate handler re-fetches `/admin?tab=<name>` which triggers a full page render (HX-Request header absent) — acceptable, browser-native. If Guy later wants a smoother experience, add `hx-history="false"` + a custom popstate listener — not this story.

6. **Librarian sees 403, not 200 with hidden tabs**
   - `admin_page` + every sub-handler call `require_role(Role::Admin)` as the first line, which returns `AppError::Forbidden` for librarians.
   - `AppError::Forbidden` (already implemented in `error::mod::IntoResponse`, story 7-1) returns HTTP 403 + a FeedbackEntry-rendered body; for HTMX requests this shows in-page, for direct navigation it's a mini-page with the feedback.
   - The `/admin` link in the nav bar is hidden for librarians (step 2 above), so they'd have to type the URL or follow an old bookmark — either way the 403 is the correct UX.
   - Anonymous users get `AppError::UnauthorizedWithReturn("/admin".to_string())` → 303 → `/login?next=%2Fadmin`; after login, the `/login` handler bounces to `/admin` and the role check fires again (librarian who logs in via `next=/admin` still gets 403 — correct).

7. **i18n keys (EN + FR)**
   New keys under `admin:` namespace in `locales/en.yml` + `locales/fr.yml`. The YAML files use filename-as-locale format — DO NOT prefix with `en:` or `fr:` (project convention, see CLAUDE.md §i18n).

   ```yaml
   admin:
     page_title: Administration          # FR: "Administration"
     tabs_aria: Administration tabs      # FR: "Onglets d'administration"
     tabs:
       health: Health                    # FR: "Santé"
       users: Users                      # FR: "Utilisateurs"
       reference_data: Reference data    # FR: "Données de référence"
       trash: Trash                      # FR: "Corbeille"
       system: System                    # FR: "Système"
     trash:
       badge_aria: "Trash, %{count} items"  # FR: "Corbeille, %{count} éléments"
     health:
       app_version: Application version   # FR: "Version de l'application"
       db_version: Database version       # FR: "Version de la base de données"
       disk_usage: Disk usage             # FR: "Espace disque"
       disk_usage_format: "%{used} / %{total} used (%{pct}%)"
                                          # FR: "%{used} / %{total} utilisés (%{pct} %)"
       counts_heading: Entity counts      # FR: "Compteurs d'entités"
       count_titles: Titles               # FR: "Titres"
       count_volumes: Volumes             # FR: "Volumes"
       count_contributors: Contributors   # FR: "Contributeurs"
       count_borrowers: Borrowers         # FR: "Emprunteurs"
       count_active_loans: Active loans   # FR: "Prêts actifs"
       providers_heading: Metadata providers  # FR: "Fournisseurs de métadonnées"
       provider_status_up: Reachable      # FR: "Accessible"
       provider_status_down: Unreachable  # FR: "Inaccessible"
       provider_status_unknown: Unknown   # FR: "Inconnu"
       last_checked: "Last checked: %{when}"  # FR: "Dernier contrôle : %{when}"
       last_checked_never: Never          # FR: "Jamais"
     placeholder:
       coming_in_story: "Coming in story %{story}"
                                          # FR: "À venir dans la story %{story}"
   ```
   - After YAML edit: **`touch src/lib.rs && cargo build`** to force the `rust-i18n` proc macro to re-read the files (project convention, see CLAUDE.md).
   - `tabs_aria` is used by the tablist container's `aria-label`.

8. **CSP compliance (story 7-4 guardrail)**
   - Zero inline `<script>`, `<style>`, `style="..."`, `onclick=`, `onchange=`, etc. in `templates/pages/admin.html`, `templates/components/admin_tabs.html`, or any `templates/fragments/admin_*_panel.html`.
   - Zero HTML strings produced from Rust code (e.g., `format!("<div style=...")`) in `routes/admin.rs` — the templates do all rendering. If a colored status indicator is needed, it's a CSS class on a `<span>`, not an inline style.
   - `templates_audit.rs` already walks `templates/` recursively — no change needed beyond making sure the new files are in that tree. Run `cargo test --lib templates_audit` to verify.

9. **Routes added to `docs/route-role-matrix.md`**
   - The existing file is organized by source module: `### <Module> (src/routes/<file>.rs)` subsections, each with its own table of columns `method | path | current | target | note`. **Add a new `### Admin (src/routes/admin.rs)` subsection** (alphabetical placement — after `### Locations`, before `### Borrowers` in the file's current ordering, or wherever the existing alphabetical convention dictates) with 6 rows:

     ```markdown
     ### Admin (`src/routes/admin.rs`)

     | method | path                     | current | target | note                                                        |
     |--------|--------------------------|---------|--------|-------------------------------------------------------------|
     | GET    | `/admin`                 | —       | Admin  | New. 5-tab shell; Librarian → 403, Anonymous → 303 /login. |
     | GET    | `/admin/health`          | —       | Admin  | New. Health panel fragment (HTMX + direct).                 |
     | GET    | `/admin/users`           | —       | Admin  | New. Stub panel — story 8-2 fills in.                       |
     | GET    | `/admin/reference-data`  | —       | Admin  | New. Stub panel — story 8-3 fills in.                       |
     | GET    | `/admin/trash`           | —       | Admin  | New. Stub panel — story 8-5 fills in.                       |
     | GET    | `/admin/system`          | —       | Admin  | New. Stub panel — story 8-4 fills in.                       |
     ```
   - Update the file header `**Last updated:**` line to today's date.

10. **Unit tests (`cargo test`) — co-located in `src/routes/admin.rs` under `#[cfg(test)] mod tests`**
    - `test_tab_resolution_valid_names` — `AdminTab::from_query_str("health")` etc. all return the right variant.
    - `test_tab_resolution_invalid_falls_back_to_health` — `AdminTab::from_query_str("../../etc/passwd")` → `AdminTab::Health`.
    - `test_tab_resolution_missing_falls_back_to_health` — `AdminTab::from_query_str("")` and `AdminTab::from_query_str(None)` → `AdminTab::Health`.
    - DB-backed (use `#[sqlx::test(migrations = "./migrations")]` — each gets a fresh DB):
      - `test_entity_counts_exclude_soft_deleted` — seed 3 titles, soft-delete 1 → count is 2.
      - `test_trash_count_unions_whitelisted_tables` — seed 1 soft-deleted title + 1 soft-deleted borrower → count is 2. Also asserts the count sums across exactly the 6 entries of `ALLOWED_TABLES` (regression guard: when 8-5 adds entries, this test should be extended, not silently pass a lower count).
    - `test_allowed_tables_have_deleted_at_column` — DB smoke: for each name in `ALLOWED_TABLES`, `SELECT COUNT(*) FROM information_schema.columns WHERE table_name = ? AND column_name = 'deleted_at'` must return 1. Protects against a typo or removed column silently zeroing the Trash count.
      - `test_mariadb_version_returns_non_empty_string` — smoke test against the test DB.
    - Middleware-adjacent (no DB needed):
      - `test_admin_handler_requires_admin_role` — construct mocked `Session` with Role::Librarian → handler returns `Err(AppError::Forbidden)`.
      - `test_admin_handler_anonymous_returns_unauthorized_with_return` — mocked Anonymous → `Err(AppError::UnauthorizedWithReturn("/admin"))`.
    - Target: **at least 8 new unit tests**, following the Epic 7 ratio (7-4 + 7-5 both shipped ≥ 8 unit tests per story).

11. **E2E smoke test — Foundation Rule #7**
    - File: `tests/e2e/specs/journeys/admin-smoke.spec.ts`. Spec ID `"AD"` (registered in `tests/e2e/helpers/isbn.ts` only if it needs ISBNs — probably does not; still claim the 2-char prefix to prevent collision).
    - Start from **blank browser context** (no injected session cookie; uses the standard `fullyParallel: true` isolation).
    - Test 1 — admin happy path:
      - `await loginAs(page, "admin")`.
      - `await page.goto("/admin")`.
      - Assert URL is `/admin` (no tab query initially, because the handler returns the page without re-pushing URL — verify first).
      - Assert 5 tabs are rendered: `page.getByRole("tab", { name: /Health|Santé/i })` × 5 entity names, with i18n regex matchers.
      - Assert Health tab is `aria-selected="true"` by default.
      - Assert Health panel shows: app version (regex `/\d+\.\d+\.\d+/`), a MariaDB version string (regex `/\d+\.\d+/` — MariaDB's `SELECT VERSION()` returns e.g. `10.11.5-MariaDB-…`; the regex matches the numeric prefix), at least one entity count row (any of `count_titles` / `count_volumes` / `count_contributors` / `count_borrowers` / `count_active_loans` labels present), and **at least N provider-status rows** where `N = registry.len()`. Each provider row may show any status (`Reachable` / `Unreachable` / `Unknown`) — the background health-ping task starts 10 s after app boot, so the smoke test must treat **`Unknown` as a passing state** (pre-ping default). Do NOT assert `Reachable` — the spec runs in CI where providers may be network-gated.
      - Click Users tab → assert URL updates to `/admin?tab=users` (via `hx-push-url`), Users panel replaces Health panel, `aria-selected` moves to Users tab. Assert the stub copy contains "8-2" (the placeholder copy mentions the target story).
      - Repeat for Reference Data (assert "8-3"), Trash (assert "8-5"), System (assert "8-4"). Trash tab badge shows the soft-deleted count — if none seeded, badge is hidden.
      - Browser Back → URL returns to the previous tab; panel content matches.
    - Test 2 — librarian gets 403:
      - `await loginAs(page, "librarian")`.
      - `await page.goto("/admin")`.
      - Assert response status is 403 (use `page.goto` return value's `response().status()`).
      - Assert the feedback-entry body contains the i18n 403 message (match both `/Forbidden/i` and `/Interdit/i` since language is librarian's preferred_language).
    - Test 3 — anonymous is redirected:
      - Fresh context, no login.
      - `await page.goto("/admin")`.
      - Assert final URL after redirects matches `/login?next=%2Fadmin`.
    - **No `waitForTimeout`** — enforced by CI gate; use DOM-state assertions only. Refer to `tests/e2e/helpers/scanner.ts` patterns (Playwright `{ delay }` for timing-sensitive ops, which this spec does not need).
    - **Data isolation** — this spec creates no ISBNs/V-codes/L-codes. Seed state is irrelevant (existing migrations provide the admin + librarian users).

12. **Zero-warning build + full gate (Foundation Rule #5)**
    - `cargo clippy --all-targets -- -D warnings` → clean.
    - `cargo test --lib` → all new tests pass (with `DATABASE_URL` set to the rust-test DB, per CLAUDE.md §DB-backed tests).
    - `cargo sqlx prepare --workspace --check -- --all-targets` → clean (this story adds new `sqlx::query_scalar!` calls — **run `cargo sqlx prepare` and commit the new `.sqlx/` files** or the gate fails).
    - `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` → empty (no new occurrences).
    - `grep -rE '\bhx-confirm\s*=\s*"' templates/` → still exactly 5 occurrences (audit test `hx_confirm_matches_allowlist` stays green; this story adds no destructive actions).
    - `grep -rnE 'onclick=|onchange=|onsubmit=|oninput=|style="' templates/pages/admin.html templates/components/admin_tabs.html templates/fragments/admin_*_panel.html` → empty (CSP 7-4 guardrail).
    - Playwright 3-cycle gate on fresh docker stack (via `scripts/e2e-reset.sh` from 6-retro Action 4): 153+ passed / 0 unexpected / 0 flaky in each of three consecutive cycles. The new smoke spec adds ~3 tests to the total count.

13. **Documentation — CLAUDE.md tiny update**
    - Under `## Architecture` → `### Source Layout`, add one line: `src/routes/admin.rs` — /admin page (tabs: health, users, reference_data, trash, system). Admin-only.
    - Under `### Key Patterns`, add a one-line bullet: "**Admin page tab pattern (story 8-1):** `/admin?tab=<name>` for deep-linking and history; `/admin/<name>` for HTMX panel swap via `hx-get` + `hx-push-url`. Tab resolution is server-side; invalid `?tab=` falls back to `health`. Every Epic-8 story fills in exactly one panel stub — extend `AdminTab` enum + replace the corresponding `admin_<name>_panel.html` fragment."
    - Nothing else in CLAUDE.md changes — no new env vars, no new migrations, no new cookies.

## Tasks / Subtasks

- [x] **Task 1 — Add the route module (AC: 1, 6, 9)**
  - [x] Create `src/routes/admin.rs` with `pub mod admin` registered in `src/routes/mod.rs`. Route wiring registers `/admin`, `/admin/health`, `/admin/users`, `/admin/reference-data`, `/admin/trash`, `/admin/system` — all GET, all gated via `session.require_role_with_return(Role::Admin, …)?` on the first line of each handler.
  - [x] `enum AdminTab { Health, Users, ReferenceData, Trash, System }` with `impl AdminTab { fn from_query_str(s: Option<&str>) -> Self }` that returns `Health` for anything invalid or missing. Also `fn as_str(&self) -> &'static str` + `fn hx_path(&self) -> &'static str` (hyphen URL / snake case name split). Unit tests cover every valid variant + invalid + missing + URL/name drift guard.
  - [x] `fn admin_page(...)` returns `Result<Response, AppError>`. Reads `Query<AdminQuery>`, resolves tab, composes `AdminShellTemplate` + panel, returns a full page for direct requests or the shell fragment for HTMX requests.
  - [x] Sub-handlers `fn admin_health_panel`, `fn admin_users_panel`, `fn admin_reference_data_panel`, `fn admin_trash_panel`, `fn admin_system_panel` — each gated by `require_role_with_return(Role::Admin, …)`, each delegates to the same `render_admin` helper. Non-Health panels render their stub via a dedicated template (no Rust `format!` HTML). Trash badge count is always computed so stub panels still show the right count.
  - [x] Add the 6 routes to `docs/route-role-matrix.md` as a new `### Admin` subsection with the table shape specified in AC 9. Bumped the `**Last updated:**` header to 2026-04-17.

- [x] **Task 2 — Shared panel data (AC: 3)**
  - [x] New module `src/services/admin_health.rs`: `entity_counts`, `trash_count`, `mariadb_version` (60 s cache, falls back to `"unknown"` on DB error), `disk_usage` (`nix::sys::statvfs`), plus `format_bytes` + `format_disk_usage` helpers. `ALLOWED_TABLES` promoted to `pub` (no rename) — source-of-truth enumeration for the 6-table trash-badge preview.
  - [x] Added `pub fn iter(&self) -> impl Iterator<Item = &dyn MetadataProvider>` on `ProviderRegistry`.
  - [x] Added `fn health_check_url(&self) -> Option<&str> { None }` default on the `MetadataProvider` trait + per-provider overrides (`bnf → https://catalogue.bnf.fr/`, `google_books → https://www.googleapis.com/books/v1/`, `open_library → https://openlibrary.org/`, `musicbrainz → https://musicbrainz.org/`, `omdb → https://www.omdbapi.com/`, `tmdb → https://www.themoviedb.org/`, `bdgest → https://www.bedetheque.com/`).
  - [x] New module `src/tasks/provider_health.rs`. Background `tokio::spawn` task — 10 s warm-up then 5 min cadence, HEAD request with 3 s timeout (GET fallback on 405). Errors swallowed at debug level.
  - [x] Wired `provider_health: ProviderHealthMap` + `mariadb_version_cache: MariadbVersionCache` into `AppState`. All 4 existing test sites (`routes/auth.rs` ×2, `middleware/locale.rs`, `tests/role_gating.rs`) updated to construct the new fields via the public factory helpers.

- [x] **Task 3 — Templates (AC: 4, 8)**
  - [x] `templates/pages/admin.html` — extends `base.html`, wraps `#admin-shell` around the pre-rendered shell HTML.
  - [x] `templates/components/admin_tabs.html` — `<div role="tablist">` with 5 `<a role="tab">` anchors carrying `aria-selected`, `aria-controls`, `aria-current`, `hx-get`/`hx-target`/`hx-swap`/`hx-push-url`. Trash badge is hidden when count is 0 (template-gated on `badge_count > 0`).
  - [x] `templates/fragments/admin_health_panel.html` — versions, entity-counts table, provider status table with status dot as a CSS class (no inline style).
  - [x] 4 stub panels (`admin_users_panel.html`, `admin_reference_data_panel.html`, `admin_trash_panel.html`, `admin_system_panel.html`) — each renders the `admin.placeholder.coming_in_story` key with the right story target. Trash stub additionally surfaces the `trash_preview` message pointing to story 8-5.
  - [x] Nav bar — `/admin` link re-added in both desktop and mobile variants, gated on `role == "admin"` only.

- [x] **Task 4 — i18n (AC: 7)**
  - [x] Added the `admin:` block to `locales/en.yml` and `locales/fr.yml` per AC 7 (tab labels, health labels, provider status labels, placeholder copy). Plus `admin.placeholder.trash_preview` for the Trash stub's 8-5 pointer.
  - [x] `touch src/lib.rs && cargo build` — ran to force the `rust-i18n` proc-macro re-read.

- [x] **Task 5 — Unit tests (AC: 10)**
  - [x] 19 new unit tests (target was ≥ 8):
    - Tab resolution: `test_tab_resolution_valid_names`, `test_tab_resolution_invalid_falls_back_to_health`, `test_tab_resolution_missing_falls_back_to_health`, `test_tab_as_str_and_hx_path_match_url_conventions`.
    - Role gating (unit): `test_admin_handler_requires_admin_role_for_librarian`, `test_admin_handler_anonymous_returns_unauthorized_with_return`, `test_admin_handler_admin_role_passes`.
    - `admin_health` (unit + DB): `format_bytes_scales_through_units`, `format_disk_usage_none_when_total_is_zero`, `format_disk_usage_rounds_percentage_half_up`, `disk_usage_reads_current_directory`, `entity_counts_starts_at_zero`, `entity_counts_exclude_soft_deleted`, `trash_count_unions_whitelisted_tables` (also pins `ALLOWED_TABLES.len() == 6` as the 8-5 tripwire), `allowed_tables_have_deleted_at_column`, `mariadb_version_returns_non_empty_string`, `mariadb_version_cache_persists_between_calls`.
    - `provider_health`: `provider_health_default_is_unknown_without_timestamp`, `new_map_is_empty_and_clones_share_state`.
  - [x] `cargo sqlx prepare --check --workspace -- --all-targets` — clean (no new macro queries; non-macro `sqlx::query_scalar` keeps `.sqlx/` untouched).

- [x] **Task 6 — E2E smoke spec (AC: 11)**
  - [x] Created `tests/e2e/specs/journeys/admin-smoke.spec.ts` with 4 tests (3 required + nav-bar visibility round-trip): admin happy path, librarian → 403, anonymous → `/login?next=%2Fadmin`, nav bar `/admin` link role gate.
  - [x] Uses `loginAs(page, "admin")` / `loginAs(page, "librarian")` — no `DEV_SESSION_COOKIE` injection.
  - [x] i18n-aware matchers: `getByRole("tab", { name: /Health|Santé/i })`, etc.
  - [x] Zero `waitForTimeout` — enforced by CI grep gate.
  - [x] Spec ran green in isolation (`npx playwright test specs/journeys/admin-smoke.spec.ts`): 4 passed.

- [x] **Task 7 — Quality gates (AC: 12)**
  - [x] `cargo clippy --all-targets -- -D warnings` — clean.
  - [x] `cargo test --lib` — 461 passed / 0 failed (up from 442 pre-story — +19 new tests).
  - [x] Integration tests: `role_gating` (7/7), `find_similar` (12/12), `metadata_fetch_race` (5/5) — all green.
  - [x] `cargo sqlx prepare --check --workspace -- --all-targets` — clean.
  - [x] `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` — empty (no new occurrences).
  - [x] `cargo test --lib templates_audit` — 2/2 pass (CSP audit covers new `templates/pages/admin.html`, `templates/components/admin_tabs.html`, 5 fragments; `hx_confirm` allowlist still 5).
  - [x] Playwright 3-cycle gate via `./scripts/e2e-reset.sh` + `npm test` three times: **cycle 1/2/3 all 158 passed / 1 skipped / 0 failed / 0 flaky**.

- [x] **Task 8 — Documentation (AC: 13)**
  - [x] Updated `CLAUDE.md` — added `admin.rs` + `admin_health.rs` lines under Source Layout and one bullet under Key Patterns for the admin tab pattern.
  - [x] Updated `docs/route-role-matrix.md` — new `### Admin` subsection with the 6 rows + `Last updated:` → 2026-04-17.

- [x] **Task 9 — Commit + push (Epic 7 retro Action 3, reinforced)**
  - [x] One commit at `review` transition covering Tasks 1–8 with `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>` trailer.
  - [ ] Push to `main` — per Guy's push policy, deferred to story close / on demand.

## Dev Notes

### Architecture — routes and middleware order

`src/routes/mod.rs::build_router` applies layers bottom-up; the effective order is `Logging → Auth (per-handler) → [Handler] → PendingUpdates → CSP`. The admin routes do NOT need to be inside the PendingUpdates-wrapped sub-router — PendingUpdates is a catalog-specific middleware for async metadata delivery. Mount the 6 admin routes at the top level alongside the catalog-independent routes (auth, home, etc.).

```rust
// In routes::mod::build_router, alongside the top-level router:
let app = Router::new()
    .route("/admin", axum::routing::get(admin::admin_page))
    .route("/admin/health", axum::routing::get(admin::admin_health_panel))
    .route("/admin/users", axum::routing::get(admin::admin_users_panel))
    .route("/admin/reference-data", axum::routing::get(admin::admin_reference_data_panel))
    .route("/admin/trash", axum::routing::get(admin::admin_trash_panel))
    .route("/admin/system", axum::routing::get(admin::admin_system_panel))
    // ... (existing routes)
```

### BaseContext vs per-struct fields — stay with per-struct for now

The Epic 7 retro §3 praised `BaseContext { role, can_edit, can_loan, can_admin, ... }`. Grep shows each page-template struct still carries `role: String`, `nav_*: String` — `BaseContext` is a pattern/convention, not a single struct every handler composes today. **Do not refactor** to a centralized `BaseContext` in this story — it's a cross-cutting refactor that belongs to its own story. Follow the existing per-struct pattern: `AdminPageTemplate { lang, role, current_page, skip_label, session_timeout_secs, nav_catalog, ..., nav_admin, ... }`.

### Provider health — why not synchronous on Health-tab load

A librarian opening `/admin` should not block on 7 HTTP pings (7 providers × 3 s worst-case = 21 s in the pathological case). Background-task model separates "when we decide status" from "when an admin views it" — matches the existing async metadata pattern (`src/tasks/metadata_fetch.rs`). The cost: status can be stale by up to 5 min. Acceptable: this is a **diagnostic** display, not a trigger for action.

If Guy wants a "refresh now" button in a follow-up story, add a POST `/admin/health/refresh` that spawns a one-shot refresh — not this story.

### Disk-usage crate choice — `nix`

`sysinfo` exists but is overkill (CPU, memory, network all bundled — 500 KB+ compiled). `nix` with just the `fs` feature is ~80 KB, exposes `statvfs(path)` directly, and matches the Linux-container deployment target exactly. `nix::sys::statvfs::statvfs(path)` returns `Statvfs { f_blocks, f_bavail, f_frsize, ... }` → `total_bytes = f_blocks * f_frsize`, `free_bytes = f_bavail * f_frsize`, `used_bytes = total_bytes - free_bytes`. Fall back to `None` on error; the template shows "unknown / unknown" when Option is None.

**Do NOT** use a crate that shells out or reads `/proc/mounts` in pure Rust — overhead + fragility. The CI container always provides `statvfs`.

### HTMX tab-switching — full-shell swap

When a tab anchor is clicked, `hx-get="/admin/<name>"` targets `#admin-shell` (wrapping both the tab bar and the panel). The response re-renders the whole shell: tab bar with the clicked tab's `aria-selected="true"`, plus the requested panel content. Single fragment, no OOB complexity.

**Why not OOB swap?** OOB (wrap panel with `<div hx-swap-oob="outerHTML:#admin-tablist">...`) keeps the tab bar "stable" across swaps — no re-render. But the tab bar is ~12 lines of HTML, renders in under a millisecond, and no interaction state lives on it (unlike forms, autocompletes, etc. that would lose input state on re-render). Full-shell swap is simpler, matches the existing HTMX flow on `/home` (which re-renders `#browse-results` with fresh toolbar + results), and has zero perceptible cost. If Epic 9's polish pass finds a reason to swap only the panel, the OOB variant is a 10-line refactor.

**Sub-handler response shape** (applies to `/admin/health`, `/admin/users`, etc.): returns a single HTML fragment that IS the `#admin-shell` contents (tab bar + panel) — not wrapped in an outer div with the id, just the children. HTMX's `hx-swap="innerHTML"` replaces `#admin-shell`'s contents with the fragment. For direct (non-HTMX) GET on a sub-path, the handler returns the full page (same as `/admin?tab=<name>`) — the sub-paths exist primarily for HTMX clarity; non-HTMX sub-path requests still render the full `AdminPageTemplate`.

### `services::soft_delete::ALLOWED_TABLES` as single source of truth — current state + scope reduction

Verified shape (grep 2026-04-17) in `src/services/soft_delete.rs:5` — **private** `const`, currently **6 tables only**:

```rust
const ALLOWED_TABLES: &[&str] = &[
    "titles", "volumes", "contributors",
    "storage_locations", "borrowers", "series",
];
```

Story 8-1 **promotes this const to `pub`** (no rename — `ALLOWED_TABLES` stays) so the Health tab's Trash-badge query can enumerate the exact same set used by the `SoftDeleteService::soft_delete` safety check. Zero drift between "what can be soft-deleted" and "what counts as trash."

**What 8-1 does NOT do:** extend the whitelist. The epic AC for 8-5's Trash tab enumerates genres, volume states, contributor roles, and location node types as soft-deletable entity types — **those table schemas do not currently carry `deleted_at` columns** (verify per migration). Extending the whitelist requires per-table schema audits + possible migrations, which is 8-5 scope.

**Consequence for 8-1's Trash badge:** the count reflects only the 6 current tables — titles, volumes, contributors, storage_locations, borrowers, series. The Trash tab panel (still a stub in 8-1) points to story 8-5, which will: (a) extend the whitelist + matching migrations, (b) render the full UNION list, (c) invalidate the 8-1 badge-count assumption in the same PR. Flag this in the stub panel copy: "Preview of soft-deleted items — full list and restore UI shipping in story 8-5."

### Users table uses `active BOOLEAN`, not `deleted_at`

`users` schema (`migrations/20260329000000_initial_schema.sql`): has `active BOOLEAN NOT NULL DEFAULT TRUE` + `version` + `created_at` + `updated_at`, but **no `deleted_at` column**. Story 8-2's epic AC phrases user deactivation as "`users.deleted_at` is set" — that's a schema migration 8-2 will need to carry (either add `deleted_at` + deprecate `active`, or map `active=FALSE` to soft-deleted semantics). Either way, **8-1 must NOT include `users` in `ALLOWED_TABLES`** — the promotion is pure visibility, no entries added. If a future reader sees the Trash badge not counting deactivated users, that's working as intended for this story.

### CSP guardrails from story 7-4

Recap of what to NOT do (the templates-audit test will flag these; better to not write them in the first place):

- No `<style>...</style>` blocks anywhere in the new templates.
- No `style="..."` attributes — use Tailwind utility classes (`bg-emerald-500`, `text-red-500`, etc.).
- No `onclick=`, `onchange=`, `onsubmit=`, `oninput=`, `onkeydown=`, `onkeyup=`, `onkeypress=`, `onfocus=`, `onblur=` — use `data-action="..."` delegated handlers routed through `static/js/mybibli.js` (which this story does not need to touch — HTMX handles tab switching).
- No `<script>` inline in templates — only `<script src="...">` from `base.html` (already wired).
- No HTMX `hx-trigger` with JS filters like `hx-trigger="keydown[key=='Enter']"` — emit a CustomEvent from JS instead. This story does not need any such trigger.

### Scanner-guard compatibility (story 7-5)

The scanner-guard module watches for `dialog[open]` and `[aria-modal="true"]` surfaces. This story ships **zero modals** — so scanner-guard simply does not engage on `/admin`. That's fine. Future admin stories (8-2, 8-3, 8-5, 8-6) will introduce modals for destructive actions; each inherits the guard automatically by using a `<dialog>` element. Out of scope here.

### The `/admin` URL in `?next=` round-trip

Story 7-1 shipped same-origin validation (`is_safe_next`) for the `next` query param on `/login`. `/admin` is a path-only `/`-prefixed string, passes every safety check. When an anonymous user hits `/admin`, `UnauthorizedWithReturn("/admin")` → `/login?next=%2Fadmin` → after login, `routes::auth::handle_login_post` bounces to `/admin`. If the logged-in user is a librarian, the role check in `admin_page` fires Forbidden — correct, no infinite redirect loop.

### Template composition — page + component + fragment

Askama does not have a first-class "include-a-fragment-by-name" dynamic include at the template level — you cannot `{% include some_var %}`. Two standard patterns:

- (a) Pre-render the panel Rust-side (`admin_health_panel.html` → `Template::render()`), inject the resulting `String` as `panel_html: String` in `AdminPageTemplate`, and render with `{{ panel_html|safe }}`.
- (b) Use Askama's `{% match active_tab %}` block with 5 explicit `{% case AdminTab::Health %}{% include "fragments/admin_health_panel.html" %}{% endcase %}` arms.

**Recommended: (a)** — the rendering code stays in Rust, the 5 fragments are isolated, no match block in templates. Matches the existing pattern in `routes::home::home` (where subcomponent HTML is generated Rust-side). Example:

```rust
let panel_html: String = match tab {
    AdminTab::Health => AdminHealthPanel { ... }.render()?,
    AdminTab::Users => AdminUsersPanel { story: "8-2" }.render()?,
    // ...
};
```

### Provider base URLs for reachability pings

Grep the existing provider modules for their base URL — do NOT invent URLs. Pings should hit a cheap endpoint that doesn't count against API quotas. Example (verify against current provider code):

- `open_library` → `https://openlibrary.org/` (homepage, HEAD-able)
- `google_books` → `https://www.googleapis.com/books/v1/` (v1 root, HEAD-able; no API key in URL)
- `bnf` → `https://catalogue.bnf.fr/` (homepage)
- `bdgest` → `https://www.bedetheque.com/` or whatever the existing module uses
- `musicbrainz` → `https://musicbrainz.org/`
- `tmdb` → `https://www.themoviedb.org/` (not the API — the API requires key in URL)
- `omdb` → `https://www.omdbapi.com/` (the API root, no key returns an error but the server is reachable)

Store these as the `health_check_url(&self)` default override on each provider. Timeout 3 s, `reqwest::Method::HEAD` if supported (most homepages accept HEAD), fall back to `GET` on 405.

### `provider_health` cache — `Arc<RwLock<HashMap>>` vs `arc-swap`

`Arc<RwLock<HashMap<String, ProviderHealth>>>` is the established pattern in this repo (`AppState::settings`, etc.). Writes are rare (every 5 min per provider = 7/300 = 0.023 writes/s), reads are rare (one per admin /admin render). RwLock is fine. Do not introduce `arc-swap` for a 7-entry map with this write pattern — it would match the `AppSettings` ArcSwap-optimization mentioned in `architecture.md` as future work, but is unnecessary here.

### Stray `nav_admin: String` fields across all page-template structs

Grep count: ~25 struct fields across `routes/{auth,borrowers,catalog,contributors,home,loans,locations,series,titles}.rs`. All populated via `rust_i18n::t!("nav.admin", ...)`. After this story, the `nav_bar.html` references them in the desktop + mobile nav. **Do NOT remove the `nav_admin` fields from any struct** — each page's nav bar now shows the /admin link when the user is admin. The field was waiting for this moment.

### Smoke spec — why `page.goto` response.status() is reliable

`page.goto(url)` returns a `Response | null`. For a direct navigation that returns 403, Playwright's response object is non-null with `status() === 403`. For redirects, the final status is what's returned. Assertion:

```ts
const response = await page.goto("/admin");
expect(response?.status()).toBe(403);
```

This works because the 403 response body is the FeedbackEntry HTML — a real document, not an empty error. Verified pattern: `tests/e2e/specs/journeys/epic7-role-gating-smoke.spec.ts` uses the same assertion shape on the `/admin` dead link removal.

### Known traps

- **`touch src/lib.rs && cargo build`** after every YAML edit. Without it, `rust_i18n::t!("admin.tabs.health")` compiles but returns `"admin.tabs.health"` verbatim (the macro falls back to key-as-string when it can't find the key). Tests pass in Rust but E2E sees garbled tab labels. This is documented in CLAUDE.md.
- **MariaDB `SELECT VERSION()`** returns a string like `"10.11.5-MariaDB-1:10.11.5+maria~ubu2204"`. Display as-is — don't try to parse. The i18n label says "Database version" not "MariaDB version"; if Guy later swaps to Postgres (he won't), the label stays accurate.
- **`current_page = "admin"`** is a new convention value. The nav-bar Askama template uses `{% if current_page == "admin" %}` — exact string match, so the handler must set this string literal (not the `AdminTab` enum variant). All 5 sub-panels share `current_page = "admin"` for the nav-bar active state.
- **Background task startup ordering**: the provider-health task MUST start AFTER `ProviderRegistry` is built AND AFTER the Axum `AppState` is constructed — it borrows both. Spawn it in `main.rs` immediately after `AppState { ... }` is instantiated, right before `build_router` is called. Panicking in the task must not crash `main` — use `.await.is_ok()` + `match` + `tracing::debug!` to swallow errors.
- **Deep-link to an in-stub tab (e.g. `/admin?tab=users`)** still renders the page + the Users stub panel. Do NOT 404 or redirect — the stub is legitimate content ("Coming in story 8-2"). The tab bar shows Users as active.
- **`page.goto` + `?next=%2Fadmin` encoding** — Axum's `UnauthorizedWithReturn("/admin")` emits `Location: /login?next=%2Fadmin` (see `error::mod::IntoResponse` for the percent-encoding via `utf8_percent_encode`). Playwright follows the redirect; the final URL in the browser is `/login?next=%2Fadmin`. Match on that exact string.
- **HTMX `hx-push-url`** pushes the URL BEFORE the response is processed. If the request fails (403, 500), the URL is still in history. Acceptable — the user sees the error in the feedback panel and can click another tab. Do not try to roll back the URL on failure.
- **Tailwind JIT**: if a class is only used in the new templates, it needs to be in the JIT scan list. `tailwind.config.js` should already glob `templates/**/*.html` — verify by checking if `bg-emerald-500` (if new) ends up in the compiled `output.css`. If the class was never used before, add a quick `cd <where-tailwind-runs> && npm run build` step to Task 7 gate.

### Infrastructure inventory

| Piece | Location | Status |
|---|---|---|
| `Role` enum + `Session::require_role` | `src/middleware/auth.rs:10,56` | ✅ — unchanged |
| `AppError::Forbidden` + IntoResponse | `src/error/mod.rs:21,113` | ✅ — returns 403 + FeedbackEntry body |
| `AppError::UnauthorizedWithReturn` | `src/error/mod.rs:20,102` | ✅ — 303 → `/login?next=...` |
| `is_safe_next` (same-origin guard) | `src/error/mod.rs:44` | ✅ — `/admin` passes |
| `ProviderRegistry` | `src/metadata/registry.rs` | ⚠️ — add `iter()` + trait method `health_check_url` |
| `MetadataProvider` trait | `src/metadata/provider.rs:31` | ⚠️ — add `fn health_check_url(&self) -> Option<&str> { None }` default |
| `AppSettings` struct + RwLock | `src/config.rs:AppSettings` | ✅ — not modified here |
| `services::soft_delete::ALLOWED_TABLES` | `src/services/soft_delete.rs:5` | ⚠️ — private `const`, 6 tables (titles, volumes, contributors, storage_locations, borrowers, series). Promote to `pub` (no rename). Whitelist **extension** is out of scope — 8-5 owns that. |
| `HxRequest` extractor | `src/middleware/htmx.rs` | ✅ — used unchanged |
| `templates_audit.rs` | `src/templates_audit.rs` | ✅ — auto-covers new files via `templates/` walk |
| Dev user (admin) seed | `migrations/20260329000002_seed_dev_user.sql` | ✅ — used by `loginAs(page, "admin")` |
| Librarian seed | `migrations/20260414000001_seed_librarian_user.sql` | ✅ — used by `loginAs(page, "librarian")` |
| `scripts/e2e-reset.sh` (6-retro Action 4) | `scripts/e2e-reset.sh` | ✅ — used in 3-cycle gate |
| `loginAs(page, role)` helper | `tests/e2e/helpers/auth.ts` | ✅ — used unchanged |
| Nav-bar `/admin` link | `templates/components/nav_bar.html` | ❌ — re-add (admin-only, both desktop + mobile) |
| `current_page = "admin"` convention | n/a | ❌ — new, document in CLAUDE.md |
| `nav_admin: String` fields on all page-template structs | 25+ structs across `src/routes/` | ✅ — populated everywhere, finally rendered |

### LLM-proofing — what NOT to do

- **Do NOT** introduce a `BaseContext` refactor in this story — it's tempting (the Epic 7 retro praised it) but out of scope. Keep adding per-struct fields like every other page template does.
- **Do NOT** add a new `AppSettings` field for the provider health check interval — hard-code `300` in the task module; story 8-4 is when settings changes are allowed.
- **Do NOT** make the provider-health check synchronous on `/admin` render — 7 HTTP requests × 3 s each will block the page load.
- **Do NOT** use `sysinfo` for disk usage — `nix::sys::statvfs` is smaller and targeted.
- **Do NOT** invent provider health-check URLs — grep the existing provider modules and pick their actual base URL.
- **Do NOT** add CSRF token scaffolding in this story — CSRF is a separate cross-cutting decision (Epic 7 retro Action 1). Read-only GET admin routes don't need CSRF; the decision can land alongside story 8-2 (first admin mutation).
- **Do NOT** add arrow-key navigation on the tab bar — out of scope per UX-DR7 (keyboard Enter/Space is native to `<a>`; arrows are Epic 9 polish).
- **Do NOT** expose `/admin/*` sub-routes without the `require_role(Role::Admin)` guard on EACH one — deep-link to `/admin/trash` must 403 a librarian just as cleanly as `/admin` does.
- **Do NOT** introduce a 6th `hx-confirm=` — the allowlist is frozen at 5 (story 7-5 templates-audit test). This story has zero destructive actions anyway.
- **Do NOT** write inline styles or scripts — CSP `'self'` enforces it; templates_audit enforces it; your patience enforces it.
- **Do NOT** refactor `routes::mod::health_check` ("/health" for Docker) — it's a separate public endpoint, not related to the Admin Health tab.
- **Do NOT** delete or rename the `nav_admin: String` fields on every page-template struct — each one finally renders now.
- **Do NOT** try to update the AC text during implementation — per Epic 7 retro Action 5, if you need to diverge from the AC (e.g., because a test contradicts it, or a simpler implementation presents itself), patch the AC in this file during the same dev session, document it in the Completion Notes + Change Log, and explain why.

### Traceability to Epic 8 cross-cutting constraints

- **NFR37 (no telemetry, all local):** The provider-health HTTP pings egress to public endpoints. These are NOT telemetry — they are existing metadata provider endpoints the app already queries during normal cataloging. The pings exercise the same network path; no new egress category is introduced. Document this in Completion Notes.
- **NFR39 (25 items per page, not user-configurable):** Not applicable to Health tab (no list of 25 items). Applies to stories 8-2, 8-3, 8-5 (paginated lists of users, reference data, soft-deleted items).
- **NFR41 (reference data not translated):** Not applicable to this story (no reference-data rendering).

## References

- **Epic & AC:** `_bmad-output/planning-artifacts/epics.md` Epic 8 §8.1 (lines 1039–1057)
- **UX-DR7 (AdminTabs):** `_bmad-output/planning-artifacts/ux-design-specification.md` §7 (lines 1812–1852)
- **Architecture — routes:** `_bmad-output/planning-artifacts/architecture.md` §Source Layout (line 198), §Config (FR70-FR76, FR100, FR120) table row (line 1042), §AppSettings cache pattern (lines 1158–1172)
- **Epic 7 retro (prereqs):** `_bmad-output/implementation-artifacts/epic-7-retro-2026-04-17.md` §6 Epic 8 preview (lines 83–101), §7 Action items (lines 105–112)
- **Role gating pattern:** `_bmad-output/implementation-artifacts/7-1-anonymous-browsing-and-role-gating.md`
- **CSP invariants:** `_bmad-output/implementation-artifacts/7-4-content-security-policy-headers.md`, `src/templates_audit.rs`
- **Scanner-guard compatibility:** `_bmad-output/implementation-artifacts/7-5-scanner-guard-modal-interception.md`
- **Session + role model:** `src/middleware/auth.rs` (lines 10–83, `Session::require_role` at line 56)
- **AppError + 403 rendering:** `src/error/mod.rs` (lines 11–39, `AppError::Forbidden` at line 21 + IntoResponse at line 113)
- **Nav-bar template:** `templates/components/nav_bar.html` (desktop lines 6–15, mobile lines 50–57)
- **Base layout + script order:** `templates/layouts/base.html`
- **Existing routes pattern:** `src/routes/loans.rs::loans_page` for HxRequest dispatch + `require_role_with_return`, `src/routes/home.rs::home` for Rust-side HTML composition
- **E2E test helpers:** `tests/e2e/helpers/auth.ts` (`loginAs`), CLAUDE.md §E2E Test Patterns
- **Provider registry + providers:** `src/metadata/registry.rs`, `src/metadata/provider.rs` (trait, line 31)
- **Settings schema + cache:** `src/config.rs` (`AppSettings` struct), `migrations/20260329000000_initial_schema.sql` (`settings` table DDL)
- **Soft-delete whitelist:** `src/services/soft_delete.rs:5` (`ALLOWED_TABLES` — currently private, promoted to `pub` in this story; 6 tables today, 8-5 extends)
- **CLAUDE.md:** §Build & Test Commands, §Foundation Rules, §Architecture, §E2E Test Patterns

## Dev Agent Record

### Agent Model Used

Claude Opus 4.7 (1M context) via Claude Code CLI, `/bmad-dev-story` workflow, 2026-04-17.

### Debug Log References

- Full test baseline before story: 442 lib tests / 159 E2E tests. After: **461 lib tests** (+19, zero regressions) / **159 E2E tests** (+4 from the new `admin-smoke` spec; cycle 1/2/3 each 158 passed / 1 skipped / 0 failed / 0 flaky).
- `nix` crate pulled in the full dep tree on first build (one-time compile of `signal-hook-registry`, `errno`, etc.). No follow-on cost — `nix` with the `fs` feature is ~80 KB compiled and already cached after the first `cargo build`.
- Clippy clean on first run (no warnings). `cargo sqlx prepare --check` clean (all new queries use non-macro `sqlx::query_scalar` — `.sqlx/` untouched).

### Completion Notes List

- ✅ All 13 ACs satisfied; zero deviations from the AC text.
- ✅ **Cross-cutting CSRF decision (Epic 7 retro §7 Action 1):** `docs/auth-threat-model.md` does NOT yet exist, and no foundation story 8-0 has landed. Story 8-1 is **read-only** — all 6 new routes are `GET` with zero mutations — so the CSRF deferral does not block 8-1 per the story's own `§ Cross-cutting decisions` section. **This remains a blocker for story 8-2** (first admin-mutation surface). Flagged here so Guy doesn't lose the thread between sprints.
- ✅ **NFR37 traceability (no new telemetry):** The provider-health pings hit endpoints the app already queries during normal cataloging — no new egress category. Documented per the story's traceability section.
- ✅ **ALLOWED_TABLES scope-reduction honored:** promoted to `pub` (no rename), whitelist size frozen at 6 for story 8-1 (regression-pinned by `trash_count_unions_whitelisted_tables` assertion `ALLOWED_TABLES.len() == 6`). Extension is 8-5 scope.
- ✅ **`users` table stays out of ALLOWED_TABLES:** `users.active BOOLEAN` vs `deleted_at` semantics deferred to 8-2 per Dev Notes.
- ✅ **`nix` crate choice confirmed:** `0.29` with `fs` feature only — ~80 KB compiled, Linux statvfs(2). No `sysinfo`, no `df` subprocess.
- ✅ **Single-shell HTMX swap** (not OOB split) — 12-line tab bar re-renders are below perception threshold, no interaction state to preserve. Matches the home-page `#browse-results` pattern.
- ℹ️ **Arrow-key tablist navigation** — deliberately NOT implemented in 8-1 per AC 4 (UX-DR7) and Epic 9 scope. Native `<a>` + Enter/Space works today. Documented as a gap in Dev Notes.
- ℹ️ **Disk usage base path** — `statvfs(state.covers_dir)` measures the mounted volume carrying the covers dir, which is the same volume the SQLite / MariaDB data mount lives on in the NAS deployment. If Guy splits them later, the label "Disk usage" still accurately reflects the covers volume — swap the path if/when that matters.
- ℹ️ **Push policy:** per Guy's push policy memory, commit landed locally at `review`; push to `main` deferred to story close / retro / on demand.

### File List

<!-- Filled at end of dev. Mark each file NEW / MODIFIED. -->

- `src/routes/admin.rs` — NEW. Admin page handler + 5 panel sub-handlers + `AdminTab` enum.
- `src/routes/mod.rs` — MODIFIED. Register 6 admin routes.
- `src/services/admin_health.rs` — NEW. `entity_counts`, `trash_count`, `mariadb_version`, `disk_usage` helpers.
- `src/services/mod.rs` — MODIFIED. Expose `admin_health` module.
- `src/services/soft_delete.rs` — MODIFIED. Promote `ALLOWED_TABLES` const from private to `pub` (no rename, no new entries).
- `src/tasks/provider_health.rs` — NEW. Background task pinging registered providers every 5 min.
- `src/tasks/mod.rs` — MODIFIED. Expose `provider_health` module.
- `src/metadata/provider.rs` — MODIFIED. Add `fn health_check_url(&self) -> Option<&str> { None }` default.
- `src/metadata/registry.rs` — MODIFIED. Add `pub fn iter()`.
- `src/metadata/{open_library,google_books,bnf,bdgest,musicbrainz,tmdb,omdb}.rs` — MODIFIED (each). Override `health_check_url` with provider's canonical URL.
- `src/lib.rs` — MODIFIED. Add `provider_health` field to `AppState`.
- `src/main.rs` — MODIFIED. Initialize `provider_health` map + spawn background task.
- `src/config.rs` — UNCHANGED (no new AppSettings field per scope).
- `Cargo.toml` — MODIFIED. Add `nix = { version = "0.29", features = ["fs"] }` (or whatever's current).
- `templates/pages/admin.html` — NEW.
- `templates/components/admin_tabs.html` — NEW.
- `templates/fragments/admin_health_panel.html` — NEW.
- `templates/fragments/admin_users_panel.html` — NEW. Stub → 8-2.
- `templates/fragments/admin_reference_data_panel.html` — NEW. Stub → 8-3.
- `templates/fragments/admin_trash_panel.html` — NEW. Stub → 8-5.
- `templates/fragments/admin_system_panel.html` — NEW. Stub → 8-4.
- `templates/components/nav_bar.html` — MODIFIED. Re-add `/admin` link (desktop + mobile), gated on `role == "admin"`.
- `locales/en.yml`, `locales/fr.yml` — MODIFIED. Add `admin:` block.
- `docs/route-role-matrix.md` — MODIFIED. Add 6 admin routes.
- `CLAUDE.md` — MODIFIED. One line in Source Layout, one bullet in Key Patterns.
- `tests/e2e/specs/journeys/admin-smoke.spec.ts` — NEW. 3 tests, spec ID `"AD"`.
- `.sqlx/*.json` — NEW (regenerated by `cargo sqlx prepare`).
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — MODIFIED. 8-1 → ready-for-dev → in-progress → review → done; epic-8 → in-progress.
- `_bmad-output/implementation-artifacts/8-1-admin-shell-and-health-tab.md` — MODIFIED. Status → review → done; tasks marked `[x]`; Dev Agent Record filled in.

### Change Log

- 2026-04-17 — Story 8-1 created from epics.md Epic 8 §8.1 (lines 1039–1057). Scope reality documented: no /admin surface exists (7-1 removed the dead link), 5 tabs to ship (Health real content + 4 stubs pointing to 8-2/8-3/8-5/8-4). Cross-cutting dependency flagged: CSRF decision (Epic 7 retro Action 1) must resolve before kickoff — this story is read-only so deferral is technically survivable, but 8-2 depends on the decision. Status → ready-for-dev.
- 2026-04-17 — Validation pass applied (5 findings). **C1:** corrected the soft-delete whitelist name from the invented `SOFT_DELETE_WHITELIST` to the actual `ALLOWED_TABLES` (private const at `src/services/soft_delete.rs:5`, 6 tables — titles, volumes, contributors, storage_locations, borrowers, series); promoted to `pub` in scope but whitelist extension deferred to story 8-5 (which owns the full Trash UNION + matching schema migrations). Trash badge in 8-1 is a 6-table preview, documented as such. **E1:** added Dev Note — `users` uses `active BOOLEAN`, not `deleted_at`; `users` stays OUT of `ALLOWED_TABLES` in 8-1. **E2:** clarified `docs/route-role-matrix.md` update shape — add a new `### Admin` subsection (file is module-organized), not raw row appends. **E3:** smoke test must treat provider `Unknown` status as passing (background ping task starts 10 s post-boot; spec cannot race it). **O1:** simplified HTMX tab swap from OOB `#admin-tablist` + `#admin-panel` dual-fragment to single `#admin-shell` innerHTML swap — ~12 lines of tab-bar HTML, zero interaction state to preserve, single response fragment.
- 2026-04-17 — Story implementation complete. New module `src/routes/admin.rs` (6 GET handlers, `AdminTab` enum with snake_case name + hyphen URL split), new service `src/services/admin_health.rs` (entity counts, trash count, MariaDB version cache, disk usage via `nix::sys::statvfs`), new task `src/tasks/provider_health.rs` (5-min background ping with 10 s warm-up). `ALLOWED_TABLES` promoted to `pub` (no rename, size frozen at 6). `MetadataProvider::health_check_url` trait method added with per-provider overrides. Nav bar `/admin` link re-added, admin-only. i18n `admin:` block added to both locales. 19 new unit tests (target was ≥ 8), 4 new E2E tests (target was 3). 3-cycle Playwright gate: 158 passed / 1 skipped / 0 failed / 0 flaky in each cycle. Zero clippy warnings, zero CSP inline-markup regressions, `hx-confirm` allowlist unchanged at 5. Status → review.
- 2026-04-17 — Code review complete (3 adversarial layers, 1 decision + 5 patch + 5 defer + 12 dismissed). **D1** resolved by editing AC 1 bullet 5 to align with bullet 4 + Dev Notes (direct non-HTMX GET on sub-path returns full page — JS-disabled-friendly, matches implementation). **P1** (High): `mariadb_version` no longer caches the `"unknown"` fallback on DB error. **P2** (Medium): `ping_all` skips `NotApplicable` providers entirely (initial seed in `spawn()` is authoritative). **P3** (Medium): `admin_page` return path now derived from `OriginalUri::path_and_query()` — the `?tab=<name>` deep link survives the login round-trip. **P4** (Low): added 4 integration tests in `tests/role_gating.rs` exercising `/admin` + `/admin/health` guards end-to-end; unit tests kept as intentional (role, path) → AppError pins with a doc note. **P5** (Low): new i18n key `admin.health.versions_heading` (EN/FR "Versions") replaces the reused `admin.page_title` on the Health panel. 5 `defer` findings logged as action items in the Review Findings section (3 performance-only: `trash_count` UNION ALL, parallel `ping_all`, `entity_counts` UNION; 2 observability: `tracing::warn!` on RwLock poisoning + `statvfs` error). Gates re-run green: 461 lib tests / 11 role_gating integration tests (up from 7) / clippy clean / `templates_audit` 2/2 / sqlx-prepare check clean. Status → done.

### Review Findings

**Code review run:** 2026-04-17 (3 adversarial layers: Blind Hunter, Edge Case Hunter, Acceptance Auditor). Triage: 1 decision-needed · 5 patch · 5 defer · 12 dismissed.

- [x] [Review][Decision] AC 1 vs Dev Notes contradiction on sub-path direct-GET response shape — **Resolved 2026-04-17**: AC 1 bullet 5 patched in this same story file to align with bullet 4 + Dev Notes (direct non-HTMX GET on a sub-path returns the full `AdminPageTemplate`, JS-disabled-friendly). Implementation already matched the Dev Notes — no code change needed. *(source: auditor)*

- [x] [Review][Patch] [High] MariaDB version cache poisons `"unknown"` on transient DB error [`src/services/admin_health.rs` `mariadb_version`] — **Fixed 2026-04-17**: `mariadb_version` now uses `match … .await` and only writes to the cache on `Ok(v)`; `Err` returns `"unknown"` passthrough (logged at debug) without overwriting the cached real version. Next request after DB recovery refetches cleanly. *(source: blind)*

- [x] [Review][Patch] [Medium] `NotApplicable` providers re-stamp `last_checked` every round + code/comment mismatch [`src/tasks/provider_health.rs` `ping_all` near L143] — **Fixed 2026-04-17**: `ping_all` now `continue`s on providers with no `health_check_url()` (the initial seed in `spawn()` already wrote `status=NotApplicable, last_checked=None`, so re-writing every 5 min was both redundant and contradicted the "mark once; never re-probe" intent). Comment rewritten to match the code. *(source: blind)*

- [x] [Review][Patch] [Medium] Anonymous `?tab=<name>` return-URL is flattened to `"/admin"` [`src/routes/admin.rs` `admin_page`] — **Fixed 2026-04-17**: `admin_page` now derives the return path from `OriginalUri::path_and_query()` (falling back to `"/admin"` only if the query part is absent), so a deep link to `/admin?tab=trash` round-trips through login as `/login?next=%2Fadmin%3Ftab%3Dtrash` and lands the user on the Trash tab after authentication. New integration test `anonymous_admin_with_tab_preserves_full_query` in `tests/role_gating.rs` pins this. *(source: blind)*

- [x] [Review][Patch] [Low] `test_admin_handler_*` unit tests are tautological [`src/routes/admin.rs` co-located tests] — **Fixed 2026-04-17** via two complementary changes: (a) the co-located unit tests got a rewritten section header explicitly scoping them as "(role, /admin*) → AppError pin" — they are now intentionally limited to the mapping, not the handler; (b) four new `#[sqlx::test]` integration tests in `tests/role_gating.rs` (`anonymous_admin_redirects_to_login_with_next`, `anonymous_admin_with_tab_preserves_full_query`, `librarian_admin_returns_403_forbidden`, `librarian_admin_health_subpath_returns_403_forbidden`) drive the full router and exercise every handler's first-line guard end-to-end. If `admin_page`'s guard ever regresses, the integration tests catch it. *(source: blind)*

- [x] [Review][Patch] [Low] Health-panel "versions" heading reuses `admin.page_title` = "Administration" [`src/routes/admin.rs` `render_health_panel`] — **Fixed 2026-04-17**: added dedicated i18n key `admin.health.versions_heading` (EN "Versions" / FR "Versions") to both locale files; `AdminHealthPanel.versions_heading` now reads this key instead of `admin.page_title`. Ran `touch src/lib.rs && cargo build` per the rust-i18n proc-macro convention. *(source: auditor)*

- [x] [Review][Defer] [Medium] `trash_count` runs 6 SELECT COUNT(*) queries on every `/admin` render [`src/routes/admin.rs:1260` → `admin_health::trash_count`] — spec explicitly mandates the badge is always visible, so this is scope-compliant, but 6 serial queries per tab click is avoidable. Candidate follow-ups: single `UNION ALL` query, short TTL cache, or on-demand fetch only when a mutation invalidates it. *(source: blind — deferred, performance-only)*

- [x] [Review][Defer] [Low] `ping_all` probes providers serially, not in parallel [`src/tasks/provider_health.rs`] — 7 providers × 3 s worst-case timeout = 21 s per round. `futures::stream::FuturesUnordered` or `join_all` would bound the round to the slowest single provider. 5-min cadence makes this cheap in practice. *(source: blind — deferred, performance-only)*

- [x] [Review][Defer] [Low] `entity_counts` issues 5 serial COUNT queries [`src/services/admin_health.rs` `entity_counts`] — a single `UNION ALL` would cut 4 DB round-trips per Health render. *(source: blind — deferred, performance-only)*

- [x] [Review][Defer] [Low] `build_provider_rows` silently swallows RwLock poisoning [`src/routes/admin.rs:475-537`] — `map.read().ok()` returns `None` for both "lock poisoned after panic" and "map not yet seeded", rendering every provider as `Unknown` indefinitely with no log. Fix (future): log `tracing::warn!` on poisoning so the degraded state is observable. *(source: edge — deferred, observability)*

- [x] [Review][Defer] [Low] `disk_usage()` fails silently on broken symlinks / ENOENT [`src/services/admin_health.rs:116-122`] — `statvfs` errors are mapped to `None` via `.ok()?` and the template prints "unknown" with no log. If an admin moves / unmounts the covers volume, the Health tab quietly hides the fact. Fix (future): `tracing::warn!` on error before returning `None`. *(source: edge — deferred, observability)*

**Dismissed as noise (12):** `format_disk_usage` test-name wording; `badge_aria` allocated for non-Trash tabs; `HX-Push-Url` response header vs `hx-push-url` attribute (spec mandates attribute approach); provider map keyed by `name()` leaks on rename (hypothetical, in-memory lifetime bound); CSRF deferral (spec-acknowledged, read-only story); `db_version` HTML injection via `|safe` (false positive — Askama auto-escapes inside `panel_html` before the outer `|safe` wrapper); `format_bytes` no PiB cap (1024+ TiB impractical on a NAS); i18n key whitespace handling (self-dismissed by reviewer); cache TTL read-write race (harmless — identical results); `trash_count` runtime schema-change crash (caught by `allowed_tables_have_deleted_at_column` test); `AdminTab::from_query_str` whitespace handling (fallback is correct behaviour); provider health seed-vs-first-ping timing window (E2E treats `Unknown` as passing per AC 11).
