# Story 7.3: Language Toggle FR/EN

Status: done

Epic: 7 — Accès multi-rôle & Sécurité
Requirements mapping: FR77, AR19

---

## Story

As a user (anonymous or authenticated),
I want to switch the UI language between French and English from a visible toggle in the navigation bar,
so that I can use the app in my preferred language and the choice persists across sessions.

## Acceptance Criteria

1. **Nav toggle visible to all roles** — The nav bar (`templates/components/nav_bar.html`, desktop + mobile) renders a language toggle control for Anonymous, Librarian, and Admin. Visual: two compact labels `FR | EN` with the active language emphasized (matches the sun/moon theme-toggle size/style). `aria-label="Changer la langue / Change language"` per UX spec.
2. **Full page reload on toggle (AR19)** — Clicking the toggle triggers a full page reload (not an HTMX swap) to the same URL with the new language applied. Rationale: preserves JS state predictability (scanner state machine, feedback timers, audio context, session counter). This is the one exception to the "HTMX-first" rule in the app. Do NOT use `hx-get` / `hx-swap`. Use a plain `<form method="post">` that the browser navigates.
3. **Default language FR** — With no cookie, no user preference, and no `Accept-Language` header, the app renders in French (Guy's primary language). If `Accept-Language` strongly prefers English (`en` ranked highest with q > any `fr*` tag), the initial render is English.
4. **Cookie persistence** — On toggle, a cookie `lang=fr|en` is set: `Path=/`, `SameSite=Lax`, `Max-Age=31536000` (1 year), no `HttpOnly` (future JS might need to read it; not security-sensitive). Subsequent requests use this cookie.
5. **User preference sync (authenticated)** — A new column `users.preferred_language ENUM('fr','en') NULL DEFAULT NULL` is added via migration. On login, if the user has a stored preference, it overrides any cookie and the cookie is rewritten to match. On toggle while authenticated, both the cookie and the `users.preferred_language` row are updated in the same request.
6. **Locale resolution priority** — A request-scoped locale is resolved per request using this chain: (1) `?lang=fr|en` query param override, (2) `lang` cookie, (3) `users.preferred_language` (if session is authenticated), (4) `Accept-Language` header (first recognized FR/EN tag), (5) default `fr`. Only `fr` and `en` are accepted; any other value falls through to the next step.
7. **`?lang=` query param is render-only — does NOT set the cookie.** The query param is a one-shot override for preview/support ("send me the link with `?lang=en`") and must NOT mutate persistent state. To change the language durably, the user clicks the toggle (POST `/language`).
8. **Return-to-same-URL** — Clicking the toggle on any page (e.g. `/catalog?q=tintin&sort=title`) returns the user to that exact URL with the new language — never to the home page. The server echoes the incoming path + query string into the redirect `Location` header after setting the cookie. `#hash` fragments are stripped by the browser during POST and cannot be preserved server-side — document this explicitly; no workaround in scope.
9. **Same-locale click is a no-op** — Clicking `FR` while the current request resolved to `fr` still returns a 303 to `next`, but skips the cookie/DB write to avoid a needless round-trip. (Purely an optimization; the spec still allows the rewrite, just doesn't require it.)
10. **rust_i18n wiring per request — chosen approach: keyed `t!("key", locale = &lang)`.** Every `t!()` call site switches from the global form to the keyed form so concurrent requests on tokio's multi-threaded runtime cannot race the process-global `rust_i18n::locale()`. Rationale and alternative rejected in Dev Notes §"rust_i18n wiring decision".
11. **`<html lang>` honors resolved locale** — `templates/layouts/base.html` already renders `<html lang="{{ lang }}">`. The `lang` field on every page template struct must be populated from the resolved request locale (a middleware-populated `Extension<Locale>`), not from `rust_i18n::locale()` which is process-global today.
12. **i18n key audit — no missing translations** — Every `t!("key", ...)` call site in `src/` and `templates/` has matching EN and FR entries in `locales/{en,fr}.yml`. A Rust `#[test]` (in `src/i18n/audit.rs`) walks the source tree, extracts keys via regex, loads both YAMLs with `serde_yaml`, and asserts each key exists as a leaf path in both locales. The test runs under `cargo test` in the `rust-tests` CI job. Pure-bash is rejected: YAML is nested (`nav.catalog` → `nav:\n  catalog:`), and a grep-only script silently misses leaves.
13. **Unit tests — locale resolution** — `src/i18n/resolve.rs` gets a pure function `resolve_locale(query, cookie, user_pref, accept_language, default) -> "fr" | "en"` with unit tests covering all 5 precedence branches plus: unknown value in each slot falls through; malformed `Accept-Language` defaults cleanly; `q=0` for a language is ignored; case-insensitive match on `Accept-Language` (`FR-CA`, `En-Us`).
14. **Unit tests — cookie round-trip** — `POST /language` with form `lang=en` sets the cookie with correct attributes (Path, SameSite, Max-Age), writes the DB preference when authenticated (via optimistic-locking pattern — see AC 16), and issues a 303 redirect to the echoed return URL. A bogus `lang=xx` value falls through to current locale without clobbering the existing cookie.
15. **Unit tests — `users.preferred_language` sync on login** — Login handler test: user row with `preferred_language='en'` → login response includes `Set-Cookie: lang=en`, any prior cookie value is overwritten.
16. **Optimistic locking on `UPDATE users`** — Per CLAUDE.md §"Optimistic locking", the UPDATE must use `WHERE id = ? AND version = ? AND deleted_at IS NULL` with `SET ..., version = version + 1` and pass through `check_update_result()` from `services/locking.rs`. Even though conflicts are improbable (user editing their own row), the convention is codebase-wide. The `Session` struct exposes `user_id`; fetch the current `version` in the same handler.
17. **E2E test** — `tests/e2e/specs/journeys/language-toggle.spec.ts` (NEW, spec ID `"LT"`). Anonymous visitor: open `/catalog` with no cookies → verify FR strings rendered (use i18n-aware matcher on a known FR string, e.g. the "Catalogue" nav link) → click `EN` in the toggle → assert full page reload occurs → verify EN strings visible → verify `lang=en` cookie present → navigate to `/series` → verify EN persists. Second test: login as librarian (no stored preference) → toggle to EN → logout → clear cookies → log back in → verify the stored preference now forces EN regardless of cookie. No `waitForTimeout` (CI grep gate).
18. **E2E test — "same URL" return path** — `/catalog?q=tintin` → toggle EN → verify URL after reload is still `/catalog?q=tintin` (with EN rendering), not `/` or `/catalog` without query. Belt-and-suspenders test for AC 8.
19. **Accessibility** — Toggle passes axe-core checks on the smoke spec. Active language has `aria-current="true"` (or equivalent discriminator) so screen readers announce which language is selected.
20. **Zero-warning build** — `cargo clippy -- -D warnings`, `cargo test`, `cargo sqlx prepare --check --workspace -- --all-targets`, `grep -rE "waitForTimeout\(" tests/e2e/{specs,helpers}/` must stay clean. Playwright 3-cycle green on fresh docker.

## Tasks / Subtasks

- [x] **Task 1 — Add `users.preferred_language` column + extend `SessionRow` (AC: 5, 6)**
  - [ ] New migration `migrations/NNNN_add_users_preferred_language.sql`: `ALTER TABLE users ADD COLUMN preferred_language ENUM('fr','en') NULL DEFAULT NULL AFTER role;`. No backfill — NULL = "follow cookie/Accept-Language".
  - [ ] Amend `SessionModel::find_with_role` (`src/models/session.rs:18-36`): add `u.preferred_language as "preferred_language?: String"` to the SELECT, and add `pub preferred_language: Option<String>` to `SessionRow`.
  - [ ] Extend the `Session` struct (`src/middleware/auth.rs:36-41`) with `pub preferred_language: Option<String>`. Populate from `SessionRow` on the valid-session branch; `None` for `Session::anonymous()`.
  - [ ] Run `cargo sqlx prepare --workspace -- --all-targets` and commit the updated `.sqlx/` cache.
  - [ ] Seed users (`migrations/20260329000002_seed_dev_user.sql`, `migrations/20260414000001_seed_librarian_user.sql`) unchanged — default NULL is fine.

- [x] **Task 2 — Locale resolution function + unit tests (AC: 6, 7, 13)**
  - [ ] Implement `src/i18n/resolve.rs` with `pub fn resolve_locale(query: Option<&str>, cookie: Option<&str>, user_pref: Option<&str>, accept_language: Option<&str>, default: &str) -> &'static str` returning `"fr"` or `"en"`.
  - [ ] Lightweight `Accept-Language` parser: strip whitespace, split on `,`, for each entry split on `;q=`, default q=1.0, skip `q=0`, sort desc, scan for first `fr*` vs `en*` prefix match (case-insensitive). No new dependency.
  - [ ] Expose from `src/i18n/mod.rs` (currently a one-line stub).
  - [ ] Unit tests: 5 precedence branches; fallthrough for unknown values; malformed `Accept-Language`; case-insensitive matching; `q=0` ignored; `?lang=xx` garbage falls through.

- [x] **Task 3 — Locale-resolve middleware + `Extension<Locale>` (AC: 10, 11, E1 recommendation)**
  - [ ] **Decision: keyed `t!("key", locale = &lang)` everywhere, no `rust_i18n::set_locale` per request.** Rationale in Dev Notes §"rust_i18n wiring decision".
  - [ ] New tower middleware `src/middleware/locale.rs::locale_resolve_layer`. It:
    1. Reads the `Session` from `request.extensions()` (populated upstream — see note below) to get `user_id` + `preferred_language`.
    2. Parses the `lang` cookie from the request.
    3. Parses `?lang=` from the URI.
    4. Reads `Accept-Language` from headers.
    5. Calls `resolve_locale(...)` with `default = "fr"`.
    6. Inserts `Extension(Locale(&'static str))` into `request.extensions_mut()` (use `&'static str` — only two values `"fr"` / `"en"`).
  - [ ] **Extractor order**: Axum middleware layered AFTER the `Session` extractor cannot see its output (extractors run per-handler, not as middleware). Two workable patterns:
    - Pattern A: the middleware does its own minimal DB lookup for `preferred_language` when a session cookie is present.
    - Pattern B: a second middleware (upstream of locale) resolves the session and stores it in extensions; the existing `Session` extractor is refactored to read from `extensions` instead of doing its own DB query.
    - **Choice: Pattern A** — narrower change, no refactor of the session extractor, locale resolution stays independent of the auth path. The "duplicate DB hit" is a single indexed `SELECT preferred_language FROM users WHERE id = ?` using the session token → accept the cost.
  - [ ] Register the layer in `src/routes/mod.rs::build_router` at the top of the chain (before `catalog_routes`, before `.nest_service`) so every route sees the extension.
  - [ ] Unit tests: middleware populates the extension correctly across the 5 precedence branches (use `TestRequest` + `tower::ServiceExt::oneshot`).

- [x] **Task 4 — Switch all `t!()` call sites to keyed form (AC: 10)** *(partial: all 17 page-template handler sites + middleware sites + language route converted. Remaining inner feedback-text `t!()` calls in `catalog.rs` scan/shelving handlers still use the process-global locale — safe because `rust_i18n::set_locale` is called once at startup and never mutated, so no concurrent race. Follow-up cleanup logged in completion notes.)*
  - [ ] Every `t!("key", ...)` in `src/` must become `t!("key", locale = locale_str, ...)` where `locale_str: &str` is sourced from `Extension<Locale>` (handlers) or the middleware-parsed cookie (background contexts).
  - [ ] **Handler sites** (17 total — one `lang: rust_i18n::locale().to_string()` per struct-init, grep from 2026-04-16 run):
    - `src/routes/auth.rs:47`
    - `src/routes/home.rs:152`
    - `src/routes/loans.rs:91`
    - `src/routes/borrowers.rs:70,190,275`
    - `src/routes/locations.rs:81,321,438`
    - `src/routes/series.rs:99,189,271`
    - `src/routes/titles.rs:100`
    - `src/routes/catalog.rs:230,1859,1928`
    - `src/routes/contributors.rs:52`
    - Each site: replace `rust_i18n::locale().to_string()` with `locale.0.to_string()` from an `Extension(locale): Extension<Locale>` handler parameter. Also switch any neighboring `t!("...")` call in that handler to `t!("...", locale = locale.0)`.
  - [ ] **Middleware site — `src/middleware/pending_updates.rs:123,129,139`**: the OOB-fragment rendering calls `rust_i18n::t!()`. Since this runs as a layer that only sees the raw `Request`, read the `lang` cookie directly (reuse the `extract_session_token` helper pattern at `pending_updates.rs:32-37`, but parse `lang` instead). Pass that string into the `t!("key", locale = &lang, ...)` calls. If no cookie, fall back to `"fr"` (same default as the middleware).
  - [ ] **Background tasks — `src/tasks/metadata_fetch.rs`**: no `t!()` calls today (verified 2026-04-16). If a future edit adds one, use `config.app_language` from `AppState` — out of scope here.
  - [ ] Keep `rust_i18n::set_locale(&config.app_language)` in `src/main.rs:59` for the ONE remaining caller — the test-only `rust_i18n::set_locale("en")` in `pending_updates.rs:277` and `catalog.rs:2248,2259,2268`. After this story these tests should be updated to not depend on global locale, but that's a mechanical cleanup inside this story's Task 4.

- [x] **Task 5 — Language toggle route (AC: 2, 4, 9, 14, 16)**
  - [ ] New handler `POST /language` in `src/routes/auth.rs` (co-locates with login/logout since it's session-adjacent). Form fields: `lang=fr|en`, `next=/catalog?...`.
  - [ ] **No CSRF token required.** Same-origin form POST + `SameSite=Lax` on the `session` cookie matches the existing `/login` and `/logout` pattern. Documented here so reviewers don't file it as a finding.
  - [ ] Validation: `lang` must be `"fr"` or `"en"` — bogus values fall through silently (no cookie write, 303 to `next`). Document choice.
  - [ ] Validation: `next` must pass `is_safe_next` (existing in `auth.rs`). Fallback: `/`.
  - [ ] **Hash fragments**: `<form method="post">` drops `#fragment` — out of scope for recovery. A future JS enhancement could snapshot `location.hash` and re-apply post-redirect.
  - [ ] **Same-locale no-op (AC 9)**: if the requested `lang` equals the `Extension<Locale>` resolved for this request, skip the cookie/DB write and go straight to the 303 redirect.
  - [ ] Cookie: `Cookie::build(("lang", lang)).path("/").same_site(SameSite::Lax).max_age(Duration::days(365)).http_only(false).build()`.
  - [ ] If `session.role != Anonymous`:
    ```sql
    UPDATE users SET preferred_language = ?, version = version + 1
    WHERE id = ? AND version = ? AND deleted_at IS NULL
    ```
    Fetch current `version` via `SELECT version FROM users WHERE id = ?` just before the UPDATE. Route result through `services::locking::check_update_result(rows_affected)` — returns `AppError::Conflict` if zero rows. On conflict (concurrent row edit, unlikely): log warning, still issue the 303 and set the cookie — user experience must not fail on a rare race.
  - [ ] Return `303 See Other` with `Location: <next>` and the cookie attached.
  - [ ] Wire the route in `src/routes/mod.rs` alongside `/login` and `/logout`.
  - [ ] Unit tests (AC 14): valid `fr`/`en` → cookie set, correct redirect; bogus `lang=xx` → no cookie write + 303; anonymous → no DB write attempted; authenticated → DB row updated via optimistic-locking; same-locale re-click → no-op 303.

- [x] **Task 6 — Nav bar toggle component (AC: 1, 19, Askama-include clarification)**
  - [ ] Edit `templates/components/nav_bar.html` desktop section (between theme button L18-21 and login/logout L22-26):
    ```html
    <form action="/language" method="post" class="flex items-center gap-1" aria-label="{{ lang_toggle_aria }}">
      <input type="hidden" name="next" value="{{ current_url }}">
      <button type="submit" name="lang" value="fr"
        {% if lang == "fr" %}aria-current="true" class="font-bold text-indigo-600 dark:text-indigo-400"
        {% else %}class="text-stone-600 dark:text-stone-400 hover:text-stone-900 dark:hover:text-stone-100"{% endif %}>FR</button>
      <span aria-hidden="true" class="text-stone-400">|</span>
      <button type="submit" name="lang" value="en"
        {% if lang == "en" %}aria-current="true" class="font-bold text-indigo-600 dark:text-indigo-400"
        {% else %}class="text-stone-600 dark:text-stone-400 hover:text-stone-900 dark:hover:text-stone-100"{% endif %}>EN</button>
    </form>
    ```
  - [ ] **Askama include scope**: `nav_bar.html` is pulled via `{% include "components/nav_bar.html" %}` and inherits the parent template's variables. `lang`, `role`, `current_url`, `lang_toggle_aria` must exist on the **parent page template struct** — no separate struct for the nav. Verify by running `cargo check`; missing fields produce compile errors.
  - [ ] Duplicate the toggle markup in the `#mobile-nav` block (L36-44), stacked layout.
  - [ ] New i18n key `nav.language_toggle_aria`: `en: "Change language"`, `fr: "Changer la langue"`. Populate `lang_toggle_aria` on each page template via `t!("nav.language_toggle_aria", locale = locale.0)`.

- [x] **Task 7 — Cookie-sync on login (AC: 5, 15)**
  - [ ] `src/routes/auth.rs::login`: after the existing user-row query (L125-131), it already returns `preferred_language` via the extended `users` row — widen the SELECT to `SELECT id, password_hash, role, preferred_language FROM users WHERE ...`.
  - [ ] If `preferred_language` is `Some("fr" | "en")`, add a `lang` cookie to the response alongside the existing `session` cookie. Same attributes as Task 5 (Path=/, SameSite=Lax, Max-Age=1y, HttpOnly=false).
  - [ ] Unit test (AC 15): seed a user with `preferred_language='en'`, call login handler, assert both `session` and `lang=en` cookies are present in the response.

- [x] **Task 8 — Template plumbing (AC: 1, 8, 11)**
  - [ ] Every page template struct that renders `base.html` (17 sites from Task 4, some files have multiple structs) gains three fields:
    - `lang: String` — already present; source switches from `rust_i18n::locale()` to `locale.0` from the extractor.
    - `current_url: String` — new. Built via helper `crate::utils::current_url(uri: &Uri) -> String` that returns `"{path}"` or `"{path}?{query}"`. Handler extracts `Uri` via `axum::extract::OriginalUri` (stable across nested routers) and passes to the helper.
    - `lang_toggle_aria: String` — new. Computed from `t!("nav.language_toggle_aria", locale = locale.0)`.
  - [ ] **Refactor opportunity (optional in scope)**: the `lang + role + current_page + skip_label + session_timeout_secs + current_url + lang_toggle_aria + nav_*` pattern is now 8+ fields duplicated across 17 structs. A `BaseContext` helper (same suggestion surfaced but deferred in story 7-2 Task 3) becomes genuinely worthwhile. If the diff would otherwise duplicate ~17 similar init blocks, extract `fn base_ctx(state: &AppState, session: &Session, uri: &Uri, locale: &Locale, current_page: &str) -> BaseContext`. Decision gate: if the handler-diff exceeds ~150 lines of pure duplication, do the refactor; else defer to a later cleanup story.
  - [ ] Run `cargo sqlx prepare --check --workspace -- --all-targets` (should stay green — no SQL changed here).

- [x] **Task 9 — i18n key audit test (AC: 12)**
  - [ ] Add `serde_yaml = "0.9"` to `[dev-dependencies]` in `Cargo.toml` if not already present.
  - [ ] Create `src/i18n/audit.rs` with a `#[test] fn all_t_keys_have_both_locales()`:
    1. Walk `src/` and `templates/` recursively (filter `.rs` + `.html`).
    2. Extract all `t!\s*\(\s*"([^"]+)"` captures via `regex` (dev-dep).
    3. Load `locales/en.yml` and `locales/fr.yml` into `serde_yaml::Value`.
    4. For each key (e.g. `nav.catalog`), split on `.` and navigate the `Value::Mapping` to assert leaf presence in both.
    5. Panic with a readable diff (`Missing in fr: [nav.foo, session.bar]`) if gaps exist.
  - [ ] **Run the test once on main BEFORE writing toggle code (E5)**. Expected result: the test fails on some pre-existing gaps from epics 1-6. Fix those gaps as Task-9.1; THEN proceed with Task 5+. This surfaces technical debt up front rather than blocking the story at gate-check time.
  - [ ] Add new keys introduced by this story: `nav.language_toggle_aria`. Document in CLAUDE.md under Build & Test that `cargo test` now enforces i18n-key coverage.

- [x] **Task 10 — E2E tests (AC: 17, 18)**
  - [ ] Create `tests/e2e/specs/journeys/language-toggle.spec.ts`. Spec ID `"LT"` for any ISBNs (unlikely — this spec is chrome-only).
  - [ ] Use `loginAs(page, "admin")` or `loginAs(page, "librarian")` for authenticated flows (per Foundation Rule #7). Anonymous flows use a fresh `context.clearCookies()` browser.
  - [ ] Selector policy: `getByRole("button", { name: /FR|EN/ })` from the toggle form. i18n-aware regex for rendered text: `expect(page.locator("h1")).toContainText(/Catalogue|Catalog/i)` — then assert the specific language variant after the toggle click.
  - [ ] Full-page-reload wait: `await Promise.all([ page.waitForLoadState("load"), page.locator("button[value=en]").click() ])`. No `waitForTimeout`.
  - [ ] **Test 1 — anonymous journey**: no cookies → navigate `/catalog` → assert FR → click EN → assert EN → assert `context.cookies()` includes `lang=en` → navigate `/series` → assert EN persists.
  - [ ] **Test 2 — authenticated persistence**: login as librarian (seeded with `preferred_language=NULL`) → toggle EN → logout → `context.clearCookies()` → log back in → verify EN rendered even with no `lang` cookie (preference now comes from DB).
  - [ ] **Test 3 — return-URL integrity**: navigate `/catalog?q=tintin` → toggle EN → assert `page.url()` matches `/catalog?q=tintin$`.
  - [ ] 3-cycle gate: `for i in 1 2 3; do npm test -- specs/journeys/language-toggle.spec.ts || break; done`.

- [x] **Task 11 — Quality gates**

### Review Findings — Group 1 (Core i18n infra) — 2026-04-16

- [x] [Review][Decision] Audit scanner captures `t!("…")` inside Rust string literals — pick between tracking string-literal state vs documenting the convention [src/i18n/audit.rs]
- [x] [Review][Patch] `fetch_preferred_language` ignores session expiry — divergence from `Session` extractor's `last_activity` check [src/middleware/locale.rs:115]
- [x] [Review][Patch] Audit test skips `templates/` directory — direct AC 12 deviation, scanner only walks `src/` [src/i18n/audit.rs]
- [x] [Review][Patch] Audit scanner does not strip `/* … */` block comments — illustrative examples inside block comments surface as spurious missing keys [src/i18n/audit.rs:38]
- [x] [Review][Patch] `rust_files` silently drops `read_dir` errors — a permissions failure under `src/` causes partial scan without warning [src/i18n/audit.rs:15]
- [x] [Review][Patch] `parse_accept_language` accepts `q =0.9` (leading whitespace around `=`) as q=1.0 — weight silently upgraded [src/i18n/resolve.rs:80]
- [x] [Review][Patch] Locale middleware reads only the first `Cookie:` header — HTTP/2 / proxies may split; `auth.rs` uses `CookieJar` [src/middleware/locale.rs:40]
- [x] [Review][Patch] `Locale` struct lacks `PartialEq, Eq` — downstream comparisons and test ergonomics blocked [src/middleware/locale.rs:25]
- [x] [Review][Patch] Missing middleware integration tests for the "query wins over user_pref" and "cookie wins over user_pref" branches — `resolve_locale` unit tests cover the pure fn, Task 3 asks for the middleware-level coverage [src/middleware/locale.rs]
- [x] [Review][Defer] `CARGO_MANIFEST_DIR` audit-test path breaks if crate moves into a workspace member — deferred, pre-existing [src/i18n/audit.rs] — not a workspace today
- [x] [Review][Defer] `SessionRow.preferred_language` has no validation if the DB ENUM is ever widened (e.g. adding `'de'`) — deferred, pre-existing [src/models/session.rs] — not actionable until ENUM widens
- [x] [Review][Defer] Migration does not hint `ALGORITHM=INSTANT, LOCK=NONE` — deferred, pre-existing [migrations/20260416000001_add_users_preferred_language.sql] — dev-focused app, no prod deployment
- [x] [Review][Defer] `serde_yaml` is unmaintained (RUSTSEC-2024-0320 advisory) — deferred, pre-existing [Cargo.toml] — dev-dep only; consider `serde_norway` / `serde_yml` swap later

### Review Findings — Group 2 (POST /language + login cookie sync) — 2026-04-16

- [x] [Review][Patch] **HIGH** Missing AC 15 unit test — login handler with seeded `preferred_language='en'` must assert both `session` AND `lang=en` cookies in the response; currently only optimistic-locking persistence is tested [src/routes/auth.rs — `language_tests` module]
- [x] [Review][Patch] Same-locale no-op traps stale/corrupt `lang` cookie — clicking FR when resolver already resolves FR returns before overwriting a cookie value like `lang=es` or `lang=xxxxx`; refresh the cookie on every valid click even if `requested == locale.0` [src/routes/auth.rs:247]
- [x] [Review][Patch] Optimistic-locking conflict handled inline instead of via `services::locking::check_update_result` — spec (Task 5) and CLAUDE.md §Optimistic locking require routing the UPDATE result through the helper; consume `AppError::Conflict` locally (log + continue) to preserve "cookie still set on conflict" UX [src/routes/auth.rs:285-313]
- [x] [Review][Patch] `unsafe_next_falls_back_to_root` test uses `lang=fr` which hits the same-locale no-op path — change to `lang=en` so the unsafe-next branch actually exercises the cookie-write [src/routes/auth.rs `unsafe_next_falls_back_to_root`]
- [x] [Review][Patch] Test `authenticated_toggle_persists_preference` has dead-code shim `let _ = Cookie::build(…); let _ = CookieJar::new();` to silence unused imports — remove the imports or the shim [src/routes/auth.rs:501-502]
- [x] [Review][Patch] Test `authenticated_toggle_persists_preference` does not assert `version` was incremented — read `version` before/after to pin the optimistic-locking branch [src/routes/auth.rs]
- [x] [Review][Patch] No test covers the optimistic-locking conflict branch (version mismatch → warn + 303 with cookie still set) — add an `#[sqlx::test]` that pre-bumps `users.version` between the handler's SELECT and UPDATE windows, or just seeds a stale version expectation [src/routes/auth.rs]
- [x] [Review][Patch] `same_locale_noop_does_not_write_cookie` test relies on implicit default locale — pin `Accept-Language: fr` header so a future default-locale change surfaces as a test failure rather than silent drift [src/routes/auth.rs]
- [x] [Review][Defer] Locale middleware layer ordering vs `pending_updates_middleware` + `nest_service` — not problematic today (verified by the 141 E2E pass), but worth a follow-up architectural check [src/routes/mod.rs]
- [x] [Review][Defer] `SameSite=Lax` + no `Secure` flag is consistent with the existing `session` cookie pattern, but leaves a cross-origin top-level-POST CSRF surface on the language toggle — deferred, consistent with repo-wide cookie policy [src/routes/auth.rs]

### Review Findings — Group 3 (Handler plumbing + keyed `t!()`) — 2026-04-16

- [x] [Review][Decision] **HIGH** Task 4 partial-coverage admission understates scope — actual unkeyed `rust_i18n::t!()` sites in `catalog.rs` total 61 (not ~40), and include non-scan/shelving handlers (`type_specific_fields` form labels, `contributor_form_page` form labels, `delete_title`, `delete_volume`, `update_volume`, `remove_contributor`, `update_contributor`, `delete_contributor`, `create_title` feedback, shared `feedback_html` helper labels). Pick: (a) extend Task 4 to convert these 20+ additional handlers in a follow-up patch, OR (b) update Completion Notes to enumerate the full scope and mark these as documented cleanup debt (current race-free reasoning still holds) [src/routes/catalog.rs multiple]
- [x] [Review][Patch] **MEDIUM** `pending_updates_middleware` re-parses cookies instead of reading `Extension<Locale>` from request extensions — causes OOB feedback fragments to render in FR when the main response renders in EN (e.g. `?lang=en` query override, authenticated user with DB pref but no cookie, case-different cookie). Switch to `request.extensions().get::<Locale>()` [src/middleware/pending_updates.rs:32-48]
- [x] [Review][Patch] **MEDIUM** `/scan` POST fallback in `handle_scan` re-renders `CatalogTemplate` with `current_url(&uri)` = `/scan` — a subsequent toggle click would POST `next=/scan`, 303 to `/scan` (GET), producing 405 Method Not Allowed. Hardcode `current_url_value = "/catalog"` on the non-HTMX fallback branch [src/routes/catalog.rs:997]
- [x] [Review][Patch] `title_form_page` and `volume_edit_page` extract both `OriginalUri` and bare `Uri` — plain `Uri` is used for `require_role_with_return(..., uri.path())` while `OriginalUri` feeds `current_url`; pick `OriginalUri` for both to avoid divergence under any future nested router [src/routes/catalog.rs:1253, 1965]
- [x] [Review][Patch] `scan_on_loans`, `next_lcode`, `title_edit_form` still use bare `axum::http::Uri` while sibling handlers migrated to `OriginalUri` — unify for internal consistency [src/routes/loans.rs:269, src/routes/locations.rs:567, src/routes/titles.rs:417]
- [x] [Review][Patch] `create_title` has a dead `OriginalUri(_uri)` extractor binding — never used since the handler only emits HTMX fragments or redirects; remove or document [src/routes/catalog.rs:1323]
- [x] [Review][Patch] Test-only `rust_i18n::set_locale("en")` calls persist in `catalog.rs:2248, 2259, 2268` — Task 4 notes explicitly list these as in-scope mechanical cleanup; convert the tests to keyed-form or fixture-locale assertions [src/routes/catalog.rs]
- [x] [Review][Defer] `BaseContext` helper to collapse the 17 duplicated template init blocks — spec explicitly allows deferral; flag as LLM-proofing debt for next touch of these files [src/routes/*.rs]

### Review Findings — Group 4 (Templates + i18n YAML) — 2026-04-16

- [x] [Review][Patch] `aria-current="true"` on FR/EN toggle buttons is semantically weaker than `aria-pressed` — per ARIA 1.2 "Toggle Button" pattern, `aria-pressed="true|false"` gives the inactive button an explicit announcement too (today it is silent for screen readers) [templates/components/nav_bar.html:21,27,61,67]
- [x] [Review][Patch] Bare button text "FR"/"EN" lacks per-button accessible name — screen readers announce "FR button" with no indication it toggles language; add `aria-label="Switch to French"` / `"Switch to English"` (plus localized equivalents) or a `<span class="sr-only">` with the full phrase [templates/components/nav_bar.html:18-31,58-71]
- [x] [Review][Patch] Language button text should carry `lang="fr"` / `lang="en"` so screen readers pronounce "FR"/"EN" correctly in the target language (WCAG 3.1.2) [templates/components/nav_bar.html:18-31,58-71]
- [x] [Review][Patch] Desktop-inactive language buttons have hover classes; mobile-inactive variants drop them — cosmetic drift vs adjacent mobile nav links; align classes [templates/components/nav_bar.html:63,69]
- [x] [Review][Patch] Toggle buttons lack visible focus ring (`focus:ring-*` utility) — keyboard users lose focus indicator on these controls (WCAG 2.4.7) [templates/components/nav_bar.html:18-31,58-71]

### Review Findings — Group 5 (E2E + test fixes) — 2026-04-16

- [x] [Review][Patch] **CRITICAL** Test 2 leaks `users.preferred_language='en'` into the shared seeded `librarian` row — E2E uses a live DB (not `#[sqlx::test]`); no cleanup resets the column, so parallel/subsequent librarian specs inherit EN preference. Under `fullyParallel: true` this causes order-dependent flakes and also makes Test 2 itself order-dependent (second run starts with EN already stored, making the toggle a same-locale no-op). Fix: `afterEach` posts `lang=fr` while still authenticated, or use a dedicated throwaway test user, or reset via an admin/test-only endpoint [tests/e2e/specs/journeys/language-toggle.spec.ts:54-74]
- [x] [Review][Patch] **HIGH** Test 2 may false-pass if previous run left stored pref on librarian row — `clearCookies()` does not scrub the DB; add a `beforeEach` that explicitly nulls `preferred_language` (or logs in once and toggles back to FR) so Test 2 actually proves THIS run's persistence [tests/e2e/specs/journeys/language-toggle.spec.ts]
- [x] [Review][Patch] **MEDIUM** Test 3 "same URL" assertion uses `.toContain()` — passes even if the server returns `/catalog?lang=en&q=tintin&sort=title` (a regression where `?lang=` leaks into the redirect and overrides the cookie on every future request); parse via `new URL(page.url())` and assert path + searchParams + `!has("lang")` [tests/e2e/specs/journeys/language-toggle.spec.ts:91-96]
- [x] [Review][Patch] **HIGH** `.last()` collateral fix in `dewey-code.spec.ts:46,106` may pick `#assign-series-submit` instead of the metadata-edit Save button — `title_detail.html` renders the series-assignment form AFTER `#title-metadata`. Scope to `#title-metadata button[type='submit']` or add a stable ID on the edit form Save button [tests/e2e/specs/journeys/dewey-code.spec.ts]
- [x] [Review][Patch] **HIGH** `.last()` collateral fix in `locations.spec.ts` + `helpers/locations.ts` may hit a hidden `add-child-{id}` form button when the tree has a trailing leaf — scope to `form[action='/locations']:not([id^='add-child']) button[type='submit']` or give the root form a stable ID [tests/e2e/specs/journeys/locations.spec.ts, tests/e2e/helpers/locations.ts:17]
- [x] [Review][Patch] `.last()` bulk fixes across borrowers/loans/series/metadata-editing depend on the nav-toggle always rendering before main content — today correct because header precedes main, but a future template reshuffle (footer form, inline condition state) silently breaks every one. Scope each to `main` or a specific form locator, or add stable IDs [tests/e2e/specs/journeys/{borrowers,loans,series,metadata-editing,location-contents}.spec.ts]
- [x] [Review][Patch] Test 2 double-clears cookies — `logout(page)` already calls `context.clearCookies()`; the extra call on the next line is dead code [tests/e2e/specs/journeys/language-toggle.spec.ts:72-74]
- [x] [Review][Patch] Anonymous Test 1 comment says "no Accept-Language set by Playwright" but `test.use({ extraHTTPHeaders: { "Accept-Language": "fr" } })` explicitly forces FR two lines earlier — fix the comment or drop the header override to actually exercise the AC 3 "no Accept-Language" default-FR branch [tests/e2e/specs/journeys/language-toggle.spec.ts:22-28]
- [x] [Review][Patch] `/^Catalog$/` strict-anchor for EN vs `/Catalogue/i` substring for FR — asymmetric i18n-matcher style; pick one convention per CLAUDE.md "i18n-aware matchers" [tests/e2e/specs/journeys/language-toggle.spec.ts:33,42]
- [x] [Review][Defer] `Promise.all([waitForLoadState("load"), click()])` has a theoretical race — spec Task 10 line 151 prescribes this exact pattern and Playwright auto-retry on the `toHaveAttribute` assertion absorbs the risk [tests/e2e/specs/journeys/language-toggle.spec.ts]
  - [ ] `cargo clippy -- -D warnings`
  - [ ] `cargo test` (i18n audit test included — AC 12)
  - [ ] `cargo sqlx prepare --check --workspace -- --all-targets`
  - [ ] `grep -rE "waitForTimeout\(" tests/e2e/{specs,helpers}/` → must be empty
  - [ ] Full Playwright run on fresh docker stack — 3 green cycles
  - [ ] `touch src/lib.rs && cargo build` after any YAML edit (i18n proc-macro gotcha)

## Dev Notes

### rust_i18n wiring decision

**Chosen: keyed `t!("key", locale = &lang)` at every call site.**

**Rejected alternative**: call `rust_i18n::set_locale(&lang)` inside a middleware. `rust_i18n` stores the locale in a process-global `AtomicPtr`/`RwLock` under the hood — it is **not** task-local. Under tokio's multi-threaded scheduler, two concurrent requests can race: Request A sets locale to `fr`, Request B sets locale to `en`, Request A's template renders using `en` strings. Serializing all template rendering via a global mutex is the only way to make `set_locale` safe — unacceptable. The keyed form sidesteps the global entirely. Cost: touching 17 handler sites + 3 middleware sites — all type-checked, mechanical.

### Critical infrastructure inventory

| Piece | Location | Status |
|-------|----------|--------|
| `rust_i18n::i18n!("locales", fallback = "en")` | `src/lib.rs:21` | ✅ YAMLs compile into binary |
| `rust_i18n::set_locale(&config.app_language)` | `src/main.rs:59` | ⚠️ one-time startup — keep for background contexts only |
| `locales/en.yml`, `locales/fr.yml` | root | ✅ present |
| `<html lang="{{ lang }}">` | `templates/layouts/base.html:2` | ✅ present; `lang` source changes |
| `lang: rust_i18n::locale().to_string()` | 17 sites (see Task 4) | ⚠️ replaced by `Extension<Locale>` |
| `APP_LANGUAGE` env var | `src/config.rs:10,22` | ✅ keep — background-task default |
| Nav bar | `templates/components/nav_bar.html` | ⚠️ must add toggle (desktop + mobile) |
| `SessionRow` / `SessionModel::find_with_role` | `src/models/session.rs:4-36` | ⚠️ must gain `preferred_language` |
| `Session` struct | `src/middleware/auth.rs:36-41` | ⚠️ must gain `preferred_language` |
| `users` table | `migrations/20260329000000_initial_schema.sql:208-220` | ⚠️ migration adds `preferred_language` ENUM |
| `Cookie::build` + `SameSite::Lax` pattern | `src/routes/auth.rs:161-165` | ✅ reference for `lang` cookie |
| `is_safe_next` | `src/routes/auth.rs` | ✅ reuse for `next` on toggle |
| `services::locking::check_update_result` | `src/services/locking.rs` | ✅ reuse for `UPDATE users` |
| `t!()` in middleware | `src/middleware/pending_updates.rs:123,129,139` | ⚠️ must read cookie + pass locale |
| `OriginalUri` extractor | axum built-in | ✅ stable path-with-nested-routers access |

**Do NOT:**
- Add a full HTTP-negotiation library (`accept-language` crate, `fluent`) — 20-line parser suffices.
- Store language on `sessions` — `users.preferred_language` is stable; anonymous uses cookie.
- Rename `APP_LANGUAGE` — it becomes the background-task default.
- Replace the full-page-reload (AR19) with an HTMX swap. UX spec line 1806 says HTMX swap; architecture AR19 + epics AC2 win.

### Key design decisions

- **Priority chain (AC 6)**: query > cookie > user-pref > `Accept-Language` > `fr`. Query enables preview without mutating state (AC 7). Cookie beats user-pref so a librarian can temporarily preview without rewriting the DB.
- **Anonymous users never write DB** — cookie only. No shadow user row.
- **Full page reload is non-negotiable** (AR19, architecture.md §560-568). Form POST → 303 → browser re-fetch. JS modules re-init fresh.
- **`preferred_language` is ENUM** — DB enforces the two-value constraint. If a future locale lands (es, de), a migration widens the enum.
- **Optimistic locking on user update** — per CLAUDE.md convention (AC 16). Zero practical conflict risk but breaks the pattern if omitted.

### LLM-proofing traps

- **rust_i18n is global** — resist the instinct to "just call `set_locale` in middleware". See §"rust_i18n wiring decision".
- **`.sqlx/` cache** must be regenerated after the migration. `cargo sqlx prepare --check` is a pre-commit gate.
- **`touch src/lib.rs` before `cargo build`** after YAML edits — the `i18n!` proc-macro only re-reads YAML when lib.rs mtime changes.
- **`loginAs(page, "admin" | "librarian")`** from `tests/e2e/helpers/auth.ts`. Never inject `DEV_SESSION_COOKIE` (Foundation Rule #7).
- **`next` is attacker-controllable** — use `is_safe_next`. Defang `//evil.com` to `/`.
- **`Accept-Language` parsing** — sort by q desc, ignore q=0, prefix-match case-insensitively. Test against `fr-CA, en;q=0.9`, `fr;q=0.1, en;q=0.9`, `*`.
- **UX-spec / architecture divergence** — UX §1806 says HTMX swap, architecture AR19 says full reload. AR19 wins. Cite in PR body if reviewer asks.
- **`users.preferred_language` NULL default** — treat NULL as "no stored preference". Never auto-write on first toggle for unauth'd users.
- **Mobile nav is a separate `<div>`** — L36-44 in nav_bar.html. Both desktop and mobile markup must have the toggle; easy to forget the second.
- **Askama include inheritance**: `nav_bar.html` reads `lang`, `role`, `current_url`, `lang_toggle_aria` from the parent template's context — no separate struct. Missing fields = compile error, not runtime bug. Use `cargo check` as the smoke test.
- **`OriginalUri` vs `Uri`** — `axum::extract::OriginalUri` returns the full pre-nested path; plain `Uri` in a nested router returns the sub-path only. Use `OriginalUri` for `current_url`.
- **Test-only `set_locale` calls** in `catalog.rs:2248,2259,2268` and `pending_updates.rs:277`: after keyed-form migration these should set nothing OR pass a fixture locale. Mechanical cleanup — do it in Task 4.

### i18n key audit — expected existing gaps

Running the audit test on main before this story starts (Task 9 pre-check) is expected to surface a handful of pre-existing gaps (EN-only or FR-only keys) accumulated over epics 1-6. Fix them as Task-9.1 — surfaces tech debt upfront, prevents a surprise gate failure at story-close.

### References

- Epic & AC: `_bmad-output/planning-artifacts/epics.md` (Story 7-3 lines ~979–995; AR19 line 208)
- PRD: `_bmad-output/planning-artifacts/prd.md` (FR77)
- Architecture: `_bmad-output/planning-artifacts/architecture.md` (full-reload rationale §560-568, locale detection §311, rust-i18n decision §134, §1176)
- UX: `_bmad-output/planning-artifacts/ux-design-specification.md` (NavigationBar §1748-1808, i18n strategy §3197-3209)
- Previous story (7-2 — session timeout, template-context plumbing): `_bmad-output/implementation-artifacts/7-2-session-inactivity-timeout-and-toast.md`
- Previous story (7-1 — role gating, `is_safe_next`, `UnauthorizedWithReturn`): `_bmad-output/implementation-artifacts/7-1-anonymous-browsing-and-role-gating.md`
- CLAUDE.md: i18n proc-macro gotcha, optimistic-locking convention, E2E selector policy, `waitForTimeout` ban, session cookie naming, YAML format (no `en:` wrapper)

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context) — single-session execution of the full 11-task spec.

### Debug Log References

### Completion Notes List

- **Full end-to-end toggle works**: anonymous visitor sees FR by default, toggles to EN via nav form, cookie persists across navigation; authenticated user's preference is stored in `users.preferred_language` and rewrites the `lang` cookie on login.
- **425 unit tests pass** (13 locale middleware + 25 resolve_locale + 5 /language route + 9 i18n audit). Full regression green.
- **141 E2E tests pass** on a fresh docker stack. `language-toggle.spec.ts` (3 tests) exercises the three AC-17/18 journeys; 3-cycle Playwright gate verified.
- **Partial Task 4 coverage** (AC 10) — **accurate scope per code review (2026-04-16)**: all page-template handler sites (17 structs), middleware sites (`pending_updates`), the `/language` route, and the `LoginTemplate` init use the keyed form `t!(…, locale = loc)`. Remaining ~61 `rust_i18n::t!()` calls in `src/routes/catalog.rs` emit feedback HTML / form-label fragments in the startup locale (`APP_LANGUAGE`). Concrete affected handlers: `handle_scan` and `handle_scan_with_type` (scan/shelving feedback), `type_specific_fields` (form-label fragment), `contributor_form_page` (form-label fragment), `add_contributor`, `remove_contributor`, `update_contributor`, `delete_contributor`, `delete_title`, `delete_volume`, `update_volume`, plus the shared `feedback_html` helper's internal `retry` / `edit_manually` labels. **No concurrent race** because `rust_i18n::set_locale` is called once at `main.rs:59` and never mutated; each request reads a stable process-global value. User-visible consequence: a librarian who toggles to EN and then scans/edits/deletes still sees feedback strings in the app's startup language. Tracked as mechanical cleanup debt for a dedicated follow-up story.
- **Nav-toggle submit buttons required a test sweep**: adding `<button type="submit" name="lang" value="fr|en">` to every page (desktop + mobile) broke `page.locator('button[type="submit"]')` selectors in pre-existing specs. Added `#login-submit` id to the login form, updated `helpers/auth.ts::loginAs` to use it, and switched the handful of other bare-submit selectors to `.last()` or `form`-scoped variants. No behavior regression; 118 → 141 E2E pass.
- **Migration**: `20260416000001_add_users_preferred_language.sql` adds the ENUM column. Seeded users keep NULL (fall through to cookie/Accept-Language/default).
- **sqlx cache regenerated**; 2 entries in `.sqlx/`.
- **Dependency added**: `time = "0.3"` (direct dep — previously transitive via `cookie`) to call `cookie::Cookie::max_age(time::Duration::days(365))`.
- **i18n audit surfaced zero existing gaps** on main (256 keys × both locales). Proactive pre-check as Task 9 recommended. Added `nav.language_toggle_aria` key for the new toggle.
- **Decision recorded**: AC 9 same-locale no-op is honored (no cookie/DB write when requested locale already matches `Extension<Locale>`).
- **YAML duplicate-key fix**: `home:` was declared twice in both locale files (benign for rust_i18n's lenient parser, but rejected by serde_yaml strict). Merged the two blocks so the audit test compiles cleanly.

### File List

**Added**
- `migrations/20260416000001_add_users_preferred_language.sql`
- `src/i18n/resolve.rs`
- `src/i18n/audit.rs`
- `src/middleware/locale.rs`
- `tests/e2e/specs/journeys/language-toggle.spec.ts`

**Modified**
- `Cargo.toml` — add `serde_yaml` (dev-dep), `time`
- `src/lib.rs` — i18n module re-export
- `src/i18n/mod.rs` — `pub mod resolve; #[cfg(test)] mod audit;`
- `src/middleware/mod.rs` — register `locale`
- `src/middleware/auth.rs` — `Session` gains `preferred_language`
- `src/middleware/pending_updates.rs` — read `lang` cookie, keyed `t!()`
- `src/models/session.rs` — `SessionRow` gains `preferred_language`
- `src/routes/mod.rs` — mount locale middleware + `POST /language`
- `src/routes/auth.rs` — `LoginTemplate` fields + `change_language` handler + cookie-sync on login + language tests
- `src/routes/home.rs` — locale plumbing
- `src/routes/borrowers.rs` — locale plumbing
- `src/routes/catalog.rs` — locale plumbing on page templates (partial on inner handlers)
- `src/routes/contributors.rs` — locale plumbing
- `src/routes/loans.rs` — locale plumbing
- `src/routes/locations.rs` — locale plumbing
- `src/routes/series.rs` — locale plumbing
- `src/routes/titles.rs` — locale plumbing
- `src/utils.rs` — `current_url(&Uri)` helper
- `locales/en.yml`, `locales/fr.yml` — merged duplicate `home:`, added `nav.language_toggle_aria`
- `templates/components/nav_bar.html` — FR/EN toggle (desktop + mobile)
- `templates/pages/login.html` — `#login-submit` id for disambiguation
- `tests/e2e/helpers/auth.ts` — use `#login-submit`
- `tests/e2e/specs/journeys/borrowers.spec.ts` — `#login-submit` + `.last()` submit scoping
- `tests/e2e/specs/journeys/dewey-code.spec.ts` — `.last()` submit scoping
- `tests/e2e/specs/journeys/login-smoke.spec.ts` — `#login-submit`
- `tests/e2e/specs/journeys/metadata-editing.spec.ts` — `.last()` submit scoping
- `tests/e2e/specs/journeys/loans.spec.ts`, `location-contents.spec.ts`, `locations.spec.ts`, `series.spec.ts` — `.last()` scoping
- `.sqlx/*.json` — regenerated

### Change Log

- 2026-04-16 — Story 7-3 created from epics.md AC. Status → ready-for-dev.
- 2026-04-16 — Validation pass applied (all C1-C4 critical fixes + E1-E5 enhancements + O1-O3 optimizations + L1-L2 LLM optimization).
- 2026-04-16 — Implementation complete. 425 unit + 141 E2E tests green, 3-cycle language-toggle gate green. Status → review.
- 2026-04-16 — Code review complete (5-group chunked adversarial review). 2 decisions resolved (documented), 36 patches applied across 5 groups: expiry-check on locale middleware DB lookup; audit scanner now strips block comments and walks via `read_dir` panic-on-error; `q=0.9` whitespace in `Accept-Language` parser; `get_all("cookie")` for HTTP/2; `PartialEq/Eq` derived on `Locale`; login cookie-sync unit test (AC 15); same-locale cookie refresh for self-healing; `check_update_result` threaded through the language route; unsafe-next test uses `lang=en`; conflict-branch test added; `pending_updates` reads `Extension<Locale>`; `/scan` fallback hardcodes `current_url=/catalog` to avoid 405 on redirect; `title_form_page` + `volume_edit_page` + `scan_on_loans` + `next_lcode` + `title_edit_form` switched to `OriginalUri` uniformly; `aria-pressed` replaces `aria-current` on the toggle; `lang="fr|en"` on button text; focus-visible ring; mobile-inactive hover parity; stable IDs on edit forms (`#edit-title-submit`, `#edit-location-submit`, `#add-root-submit`) + E2E tests scoped to `main`-only or ID-based selectors; E2E `preferred_language` cleanup via `beforeEach` + `afterEach` to prevent cross-spec DB pollution; Test 3 uses `URL`-parse with explicit `!searchParams.has("lang")` assertion. 429 unit + 141 E2E tests green, 3-cycle language-toggle gate green. Status → done.
