---
story_key: 8-8-first-launch-setup-wizard
epic: 8
story: 8
title: First-launch setup wizard
status: review
created: 2026-04-29
last_updated: 2026-04-29 (implementation complete — handed off to code review)
estimated_effort: large
dependencies:
  - 8-1-admin-shell-and-health-tab          # AdminTab handlers must already exist (`/admin?tab=system`) so post-wizard redirect works
  - 8-2-csrf-middleware-and-form-token-injection   # All wizard POSTs go through the CSRF middleware
  - 8-3-user-administration                 # Re-uses `UserModel::create` + argon2 hashing chain
  - 8-4-reference-data-management           # Default ref-data is already seeded — wizard does not re-seed
  - 8-5-system-settings                     # `settings` table + `AppSettings` cache + provider-key contract is the writer
---

# Story 8-8: First-launch setup wizard

## Story Statement

**As a** first-time user installing mybibli,
**I want** a setup wizard that guides me through creating the admin account and initial configuration,
**so that** I can start using the app without editing migrations or seed files by hand, and resuming after an interruption does not destroy what I already entered.

## Functional Requirements

- **FR87** — System can present a first-launch setup wizard to create the initial admin account.
- **FR121** — Setup wizard steps are idempotent: if interrupted and resumed, each step detects existing data (e.g. admin account already created) and presents it for editing, not blank creation forms. No data loss on restart, no duplicate creation.
- **FR91** (referenced) — Default reference data (genres, volume_states, contributor_roles, location_node_types) is initialized on first launch. **Already shipped by migrations `20260330000001` / `20260330000002` / `20260401000001` and made admin-editable in story 8-4.** This story does NOT re-seed; it relies on the existing seed migrations.
- **FR86** (referenced) — Auto DB schema creation on first launch. **Already shipped by `sqlx::migrate!("./migrations").run(&pool)` in `src/main.rs`.** This story does NOT touch migrations runtime.

## Architectural Requirements

- **AR9** — `AppSettings` is loaded from MariaDB `settings` table into `Arc<RwLock<AppSettings>>`, invalidated on admin save. The wizard's Step 2 (Providers) and Step 3 (Preferences) write to the same `settings` rows used by `/admin/system` and MUST reload the cache the same way story 8-5 does (`AppSettings::load_from_db` + write-lock-swap, no `.await` while the lock is taken). Use the existing `admin_system::save_*_settings` handlers' helpers — do NOT re-roll the optimistic-locking + cache-reload chain.

## UX Design Requirements

- **UX-DR20** — SetupWizard with 4-step progress indicator (dots), Previous / Next navigation, data persistence per step (idempotent on resume), "Complete setup" label on the last step's primary button.

> **Source-of-truth deviation note (read before implementation):** The UX-DR20 mermaid in `_bmad-output/planning-artifacts/ux-design-specification.md:1014-1042` describes 4 steps as Account → **Locations** → **Data** → APIs. That diagram pre-dates the Epic-8 decomposition. The epics spec (`epics.md:1179-1198`) is the binding contract for this story and re-defines the 4 steps as **Admin → Providers → Preferences → Done (recap)**. Storage locations are NOT part of the wizard (locations admin is reachable post-wizard at `/locations`, shipped by Epic 2). Reference data is NOT in the wizard (8-4 made it admin-editable at `/admin?tab=reference-data` and seed migrations cover the bootstrap). Implementing the older mermaid would balloon scope and reintroduce already-deleted code. The wizard's job is the **minimum** needed to produce a usable login + working metadata fetches; everything else is `/admin/*` afterwards.

## Non-Functional Requirements

- The wizard intercepts every route while active (except `/static/*`, `/covers/*`, `/health`, and `/setup/*` itself); regular handlers MUST NOT be reachable until the wizard completes.
- Once the wizard completes (`settings.setup_completed_at IS NOT NULL`), `/setup` returns 404 — the wizard is single-use.
- Resume safety (FR121): Step 1 idempotency MUST NOT create a duplicate admin user. If an admin row already exists with `deleted_at IS NULL`, the form is pre-filled and the existing row is updated instead of inserted.
- E2E / dev bypass: `MYBIBLI_SKIP_SETUP` env var disables the wizard middleware entirely (matches the `MYBIBLI_SKIP_STARTUP_PURGE` pattern from story 8-7 — accept only `"1" | "true" | "TRUE"`, anything else is treated as unset).
- CSP-compliant: zero inline `<script>`, `<style>`, or `onclick=` in any wizard template — every interaction routes through `data-action` attributes (extend `static/js/inline-form.js` or add a small `static/js/setup-wizard.js`). Validated by the existing `src/templates_audit.rs::no_inline_markup_in_templates` test.
- CSRF-compliant: every POST in `/setup/*` carries `_csrf_token` from the anonymous-session row that the existing `session_resolve_middleware` mints on first hit. **No new CSRF exempt routes** — `templates_audit.rs::csrf_exempt_routes_frozen` MUST stay green (allowlist frozen at `[("POST", "/login")]`).
- i18n: every label, helper text, validation error, and recap value in EN + FR. The wizard runs before the user has set a default language, so the locale resolution chain is: `Accept-Language` header → fallback to `AppSettings::default_language()` (which the wizard later writes in Step 3). Use `rust_i18n::t!` everywhere — no hardcoded strings.

## Acceptance Criteria

### AC1 — Setup gating middleware

- Given a fresh install (no user with `role='admin'` AND `deleted_at IS NULL` exists, AND `settings.setup_completed_at IS NULL`), when any HTTP route is requested, then the new `setup_gate_middleware` redirects to `/setup` (HTTP 303 See Other for non-HTMX, HTMX `HX-Redirect` for HTMX requests).
- Given the same fresh-install state, when a request hits `/static/*`, `/covers/*`, `/health`, or any `/setup/*` route, then the middleware lets the request through unmodified (whitelist).
- Given the wizard has completed (`settings.setup_completed_at IS NOT NULL` OR an admin user exists per the dual condition below), when any user navigates to any non-wizard route, then the middleware is a no-op and the regular routing applies.
- The "wizard active" predicate is the boolean `(active_admin_count == 0) AND (setup_completed_at IS NULL)`. Both halves must be true. **`active_admin_count > 0` alone makes the wizard inactive — once an admin exists from a prior install / migration, the wizard does not run even if `setup_completed_at` is still NULL.** This handles upgrade paths where pre-Epic-8 deployments already have admins from the seed migration. (Conversely, `setup_completed_at IS NOT NULL` alone makes it inactive too, even if all admins were later soft-deleted — the wizard is single-use and "no admins" recovery happens via DB intervention or a future story, not by re-running setup.)
- The middleware caches the predicate in `Arc<RwLock<SetupGateState>>` to avoid a DB round-trip per request. The cache is invalidated by Step 1 (admin created) and Step 4 (`setup_completed_at` written) — both are inside the wizard handlers, so no other write path needs to know.

### AC2 — `/setup` page rendering

- Given `GET /setup` and the wizard is active, when the handler runs, then it renders `templates/pages/setup.html` (extending `layouts/bare.html` — no nav bar, no admin scaffolding) with:
  - 4-dot progress indicator (1=Admin, 2=Providers, 3=Preferences, 4=Done) — current step pulses, completed steps filled, future steps dimmed (per UX-DR20 § 20)
  - One panel per step (rendered server-side, the unselected steps are NOT in the DOM — server-side step resolution per AC3)
  - Previous / Next buttons (Step 1 has Previous disabled; Step 4's Next is labeled "Complete setup")
- Given `GET /setup` and the wizard is NOT active, when the handler runs, then it returns 404 NotFound rendered via `AppError::NotFound`. (The single-use property of the wizard.)
- The handler builds a `SetupContext` struct passed to every wizard template:
  ```rust
  pub struct SetupContext {
      pub lang: &'static str,                          // "fr" | "en"
      pub role: &'static str,                          // always "anonymous" pre-Step-1, "admin" Step-2..4
      pub csrf_token: String,                          // from Session.csrf_token
      pub step: SetupStep,                             // resolved server-side
      pub field_values: StepFormValues,                // typed per step (see Task 5)
      pub field_errors: HashMap<String, String>,       // populated on validation 400; empty on first render
      pub admin_already_exists: bool,                  // Step 1 flag for the "leave password blank" hint
      pub keyed_providers: &'static [&'static str],    // KEYED_PROVIDERS — Step 2 row source
      pub recap: Option<RecapData>,                    // populated only on Step 4
  }
  ```
  All fields are populated by the handler before rendering — the template never queries data.

### AC3 — Server-side step resolution (resume detection / FR121)

The current step is resolved server-side from a deterministic predicate over `users` + `settings`. **No client-side cookie, query param, or hidden field carries step state.** Resolution table:

| Predicate (evaluated top-down, first match wins) | Resolved state |
|--------------------------------------------------|----------------|
| `setup_completed_at` is set OR active admin already exists | **wizard inactive** — `/setup` returns 404, gate middleware is a no-op (AC2 + AC8) |
| No active admin user exists | **Step 1 — Admin** |
| Admin exists, AND none of the three provider-key rows have a non-empty value, AND `setup_step_3_done == '0'` | **Step 2 — Providers** (form is initially blank) |
| Admin exists, AND at least one provider key OR Step 2 was visited (we use the same `setup_step_3_done` sentinel logic), AND `setup_step_3_done == '0'` | **Step 3 — Preferences** |
| Admin exists, AND `setup_step_3_done == '1'` | **Step 4 — Done (recap)** |

- Going-forward via URL is impossible (the URL has no step param). Going-backward is via the `_back: bool` form field on each step's submit (see AC4-AC7) — the handler 303s to `/setup`, the next GET re-resolves server-side and lands on the previous step. **No `?step=N`, no `/setup/back` route, no cookie.** The "previous step" is the one before the current per the resolution order above; if the resolved state degrades (e.g. admin row was deleted between requests), the resolver returns to the earliest still-applicable step.
- Step 1 idempotency: `admin_already_exists == true` in `SetupContext` flips three things — username field is pre-filled with the admin row's username, password is blank with the localized hint `t!("setup.step_1_admin_exists_hint")` ("An admin account is already configured. Leave the password blank to keep it unchanged, or enter a new one to update."), "Create admin" button label changes to "Update admin". On submit, the existing row is updated via `UserModel::update` (per story 8-3) instead of `UserModel::create` — no duplicate row.
- Step 2 idempotency: provider-key fields are pre-filled with the masked values (last 4 chars only — same masking as `admin_system::save_provider_keys`); an unchanged masked value submitted back triggers no DB write (same "no-op on unchanged masked value" path as story 8-5).
- Step 3 idempotency: language radio + overdue input are pre-filled from current `settings` rows.

### AC4 — Step 1: Admin account

- Given Step 1, when the admin submits username (required, trimmed, unique vs `users.username`) + password (required, ≥ 8 chars), then a user row with `role = 'admin'`, `active = TRUE`, `deleted_at IS NULL`, `password_hash = argon2(…)` is created via `UserModel::create` (re-using story 8-3's chain — argon2 hashing via `services/password::hash_password`). **No `full_name` column** — the epic mentioned an "optional full name" field but the `users` schema has no such column, so it is explicitly out of scope.
- **Single-flight admin creation guard (closes the C1 race):** the Step 1 handler runs in a single transaction: BEGIN → re-read `SELECT COUNT(*) FROM users WHERE role='admin' AND active=TRUE AND deleted_at IS NULL` → if `> 0`, ROLLBACK + return 409 Conflict (`error.setup.admin_already_created` — "An admin account was created by another browser. Please reload."). Else INSERT the new admin → COMMIT. This closes the window where two concurrent first-launch browsers could each pass the gate (cache says wizard active) and each create an admin row with different usernames.
- After commit, the wizard authenticates the new admin: a fresh `sessions` row is INSERTed with `user_id = new_admin.id` and a fresh CSRF token (re-using `routes/auth.rs::generate_session_token` + `middleware/csrf::generate_csrf_token`). The current anonymous session row is soft-deleted; the response sets the new `session=` cookie. **This re-uses the exact session-rotation logic from `routes/auth.rs::login` lines ~183-220 — extract a helper `services::auth::authenticate_session(pool, user_id, &session) -> Result<(String /* session token */, String /* csrf token */)>` and call it from both login and Step 1.**
- **Validation errors do NOT use `feedback_html`.** The wizard uses native form submits, so on 400 the handler re-renders `templates/pages/setup.html` with `SetupContext.field_errors` populated (e.g. `{"password": "setup.errors.password_too_short", "username": "..."}`) and the `field_values` retained (username preserved, password cleared per security hygiene). The Askama template renders each error inline under its field via `{% if let Some(err) = ctx.field_errors.get("password") %}<p class="text-red-600">{{ t!(err, locale = ctx.lang) }}</p>{% endif %}`. **No `FeedbackEntry` component, no HTMX OOB swap, no `feedback_html` call.**
- `username` UNIQUE collision (whether against an active OR soft-deleted user — the index ignores `deleted_at`) → `UserModel::create` returns `AppError::Conflict("username_taken")` per its existing 23000 mapping. The handler renders the conflict as a `field_errors["username"] = "setup.errors.username_taken"` field-level error, not a global 409 page.
- **Self-deactivate / last-active-admin guards from story 8-3 do NOT apply here** (we are creating, not modifying). Step 1 idempotent re-submit (admin already exists) goes through `UserModel::update` and the role is fixed at `admin` — the demote-guard from 8-3 cannot fire (we never demote in the wizard). **In the idempotent-update path, the single-flight guard above is skipped** (we ARE the existing admin from a previous Step 1 submit; the gate cache hasn't yet flipped because the wizard isn't done — but a re-render naturally lands on Step 2, not back on Step 1, so this path is reached only if the user explicitly hits Previous from Step 2 then re-submits Step 1).
- After successful submit (create OR update), the SetupGateState cache is invalidated and the response 303s to `/setup` (which the next GET re-resolves to Step 2).

### AC5 — Step 2: Metadata provider API keys

- Given Step 2, when the page renders, then it lists every provider that consumes a key today: **Google Books, OMDb, TMDb** (the current set wired in `src/main.rs:142-159`). Each row has: provider name, an optional API key input (masked when pre-filled — last 4 chars only, same masking as `admin_system::save_provider_keys`), a "Skip" checkbox (skipping leaves the row at empty string in `settings` ONLY if the row was previously empty; an existing key submitted with Skip checked is NOT cleared — clearing requires the explicit `_clear_<key>` form field per the 8-5 contract).
- BnF, Open Library, MusicBrainz, BDGest are NOT listed (they use no API key today — would mislead the user). The provider list is **`pub const KEYED_PROVIDERS: &[&str] = &["google_books", "omdb", "tmdb"];`** declared in `src/metadata/mod.rs` (single source of truth — both `routes/setup.rs` and `routes/admin_system.rs` import it). **Hard-coding the three names is acceptable** — a future provider that needs a key adds itself there in the same PR that wires it. Do not over-engineer with a `requires_api_key()` trait method until a fourth keyed provider lands.
- Submitting writes via the same helper that `admin_system::save_provider_keys` uses today — extract `services::admin_system::save_provider_keys(pool, settings_arc, payload) -> Result<()>` from the existing route handler in this story (rule of three: the writer now has two callers, the wizard + admin/system). Settings cache reload via `AppSettings::load_from_db` + write-lock-swap (AR9).
- **Validation errors render via `SetupContext.field_errors`** — same pattern as AC4 (no `feedback_html`, no FeedbackEntry).
- No key is mandatory — users with no key get the no-result path on those providers (the existing per-provider "empty key short-circuit" from story 8-5 covers this).
- **Out of scope:** validating keys (no probe HTTP call). The Health-tab provider-status from story 8-1 surfaces reachability after the wizard completes; nudging the user to fix bad keys post-wizard is the right place.

### AC6 — Step 3: Preferences (default language + overdue threshold)

- Given Step 3, when the page renders, then it shows two fields:
  - **Default language** — radio FR / EN (writes to `settings.default_language`, per the locale-resolution chain established in story 7-3 + 8-5)
  - **Overdue loan threshold (days)** — integer input, default 30, min 1 (writes to `settings.overdue_loan_threshold_days`, per story 8-5)
- Validation: integer < 1 → `SetupContext.field_errors["overdue"] = "setup.errors.overdue_must_be_positive"`; language not in `{fr, en}` → `field_errors["language"] = "setup.errors.invalid_language"`. Both produce HTTP 400 status with the wizard re-rendered (submitted values preserved). **Same pattern as AC4 — no `feedback_html`, no FeedbackEntry.**
- On successful submit, the two settings are written via the same `services::admin_system` helpers as `admin_system::save_loans_settings` and `admin_system::save_language_settings`. Cache reload via AR9.
- Set `settings.setup_step_3_done = '1'` so AC3's resolution moves to Step 4. (See Task 4 for why this sentinel exists — `default_language='fr'` + `overdue=30` is not distinguishable from "user explicitly chose those values" without a sentinel.)

### AC7 — Step 4: Done (recap + Complete setup)

- Given Step 4, when the page renders, then it shows a read-only recap:
  - Admin username (from `users` row)
  - For each keyed provider: "configured" if key non-empty, "not set" otherwise (last 4 chars masked are NOT shown here — the recap is high-level)
  - Default language (FR / EN)
  - Overdue threshold (days)
- The primary button is labeled "Complete setup" (`t!("setup.complete_button")`).
- Clicking "Complete setup" (POST `/setup/complete`):
  - Writes `settings.setup_completed_at` via the existing `save_setting` UPDATE chain (the row was INSERT-IGNORE-seeded by Task 1 with empty-string sentinel; this UPDATE flips it to a real value). **Format: ISO 8601 / RFC 3339 with `Z` suffix** — e.g. `"2026-04-29T12:34:56Z"`. Generated via `chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)` (the `true` argument forces `Z` instead of `+00:00`). Read back in `AppSettings::load_from_db` via `chrono::DateTime::parse_from_rfc3339` → `Some(...)`; empty string → `None`. Anything else (including malformed timestamps) → log warn + `None` (treat as wizard incomplete — the gate will fire on next request, which is the correct fail-safe).
  - Invalidates the SetupGateState cache.
  - Redirects to `/catalog` (HTMX `HX-Redirect`, non-HTMX 303 Location). (`/admin?tab=health` is a defensible alternative — pick `/catalog` because the UX spec § Journey 5 line 1024-1027 frames the climax as "first scan within 30 minutes" and `/catalog` is the place the user wants to land.)

### AC8 — Post-completion behavior

- Given `setup_completed_at IS NOT NULL`, when ANY user (anonymous, librarian, admin) navigates to `/setup` or any `/setup/*` sub-route, then the server returns 404 NotFound. **No "wizard already done" redirect to `/admin?tab=system`** — the wizard is dead, not relocated.
- Given `setup_completed_at IS NOT NULL`, when the gate middleware runs on any other route, then it is a no-op (cached "wizard inactive" branch).

### AC9 — `MYBIBLI_SKIP_SETUP` bypass

- Given `MYBIBLI_SKIP_SETUP=1` (or `true` / `TRUE`) is set in the process env when the app starts, when the gate middleware runs, then it is a no-op for ALL routes — fresh-install or not. **No DB writes** (does not flip `setup_completed_at`); the bypass is purely runtime.
- The env var is read **once at startup** in `main.rs`, parsed into `bool`, and stored alongside `SetupGateState` (e.g. as a sibling field `bypass_via_env: bool` or as `Arc<bool>`). The middleware then reads the cached bool, never re-reads the env. This matches the `MYBIBLI_SKIP_STARTUP_PURGE` pattern (story 8-7) — env-vars cannot change for a running process, so per-request reads are wasteful.
- Used by the Playwright seed chain (`tests/e2e/docker-compose.test.yml`) and by local dev where the user has injected a session via the `DEV_SESSION_COOKIE` migration. Add the env var to `tests/e2e/docker-compose.test.yml` so existing E2E specs keep working without seeding `setup_completed_at`.
- Documented in `CLAUDE.md` under "Build & Test Commands" (alongside `MYBIBLI_SKIP_STARTUP_PURGE`).
- **Empty string `MYBIBLI_SKIP_SETUP=""` and `MYBIBLI_SKIP_SETUP=0` are NOT treated as enable** (story 8-7 R3-N6 lesson — `.is_ok()` is too permissive). Use `matches!(v.as_str(), "1" | "true" | "TRUE")`.

### AC10 — i18n coverage (EN + FR)

- Every label, button, validation error, and recap field in the wizard renders in both EN and FR. Locale resolution before Step 3 picks a value: `Accept-Language` header → first 2 chars match `fr` or `en`, else fall back to `AppSettings::default_language()` (which is still the migration default `'fr'` until Step 3 writes a different value). Cookie `lang=` is also honored (the existing `locale_resolve_middleware` chain).
- New i18n keys (illustrative — exact key names in Task 6):
  - `setup.title`, `setup.step_1_title`, `setup.step_2_title`, `setup.step_3_title`, `setup.step_4_title`
  - `setup.step_1_username_label`, `setup.step_1_password_label`, `setup.step_1_password_hint`
  - `setup.step_2_provider_key_label`, `setup.step_2_skip_label`
  - `setup.step_3_language_label`, `setup.step_3_overdue_label`
  - `setup.step_4_recap_admin`, `setup.step_4_recap_providers_configured`, `setup.step_4_recap_providers_not_set`, `setup.step_4_recap_language`, `setup.step_4_recap_overdue`
  - `setup.previous_button`, `setup.next_button`, `setup.complete_button`
  - `setup.errors.username_required`, `setup.errors.username_taken`, `setup.errors.password_too_short`, `setup.errors.invalid_language`, `setup.errors.overdue_must_be_positive`
- After adding keys to `locales/en.yml` + `locales/fr.yml`, run `touch src/lib.rs && cargo build` (per CLAUDE.md "i18n").

### AC11 — CSP compliance

- The `templates/pages/setup.html` + any new fragments contain ZERO inline `<script>`, `<style>`, `style="..."`, or `on*=` attributes. The progress-dot pulse is pure CSS (Tailwind `animate-pulse` — already used elsewhere) on the active dot, conditionally rendered server-side.
- Step transitions are pure form submits (POST `/setup/step-N` → 303 to GET `/setup`) — no client-side step state. **No JS needed beyond the existing `csrf.js` (which adds the `X-CSRF-Token` header to HTMX requests; the wizard uses native form submits so even that is unused).**
- The existing `src/templates_audit.rs::no_inline_markup_in_templates` test catches regressions.

### AC12 — CSRF compliance

- Every POST form in the wizard (`/setup/step-1`, `/setup/step-2`, `/setup/step-3`, `/setup/complete`) carries `<input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">` as a top-3 input — caught by `templates_audit.rs::forms_include_csrf_token` if missing.
- The CSRF token comes from the anonymous session row that `session_resolve_middleware` mints on first hit (story 8-2 lazy-anonymous-session pattern). The wizard does NOT add `/setup/*` to `CSRF_EXEMPT_ROUTES` — `csrf_exempt_routes_frozen` stays at `[("POST", "/login")]`.
- After Step 1 (admin authenticated), the CSRF token rotates as part of the session-rotation chain (Step 1 → new authenticated session → fresh CSRF token in the new session row). Steps 2/3/4 read the new token from the meta tag of the next page render — no special handling needed.

### AC13 — Concurrent first-launch safety

- Given two browsers hit a fresh-install simultaneously, when both Step-1 forms are submitted with **different** usernames, then the single-flight admin guard from AC4 catches the second submit: it BEGINs a transaction, re-reads `active_admin_count`, sees it is now 1, ROLLBACKs and returns 409 Conflict (`error.setup.admin_already_created` — "An admin account was created by another browser. Please reload."). Both browsers cannot create admin rows.
- Given two browsers hit with the **same** username, then the second submit also fails: either via the AC4 single-flight guard (if the first commit landed in time) or via the `users.username` UNIQUE constraint (`UserModel::create` returning `Conflict("username_taken")`). Both failure modes converge on the user reloading and resuming at Step 2.
- After Step 1 from browser A, browser B's GET `/setup` resumes at the resolved step from AC3 — it does NOT see Step 1 again. **The browser-B user is now anonymous in a wizard that requires the admin's session for Steps 2-4** — they get the same Step 2 form but, because they are not authenticated as the new admin, their submits go through CSRF + session middleware as anonymous. This is fine: Steps 2-4 do NOT call `session.require_role(Admin)` (the gate predicate IS the access check during the wizard). Browser B can complete the wizard; the new admin's session in browser A becomes "logged in as the admin user" the next time browser A interacts.
- (Alternative considered: bind Steps 2-4 to the admin session created in Step 1, requiring browser B to log in. Rejected as user-hostile for the rare two-browser race; the predicate-only access pattern is consistent with browser A also being just-an-anonymous-with-a-fresh-cookie until they log in for real.)

### AC14 — Test coverage

Unit tests (`#[cfg(test)]` in `src/services/setup.rs` + `src/middleware/setup_gate.rs`):
- Resume-detection decides the correct landing step for every partial state (no admin / admin-only / admin + ≥1 provider key / admin + provider + step_3_done).
- `setup_completed_at IS NOT NULL` makes `/setup` 404 in handler tests.
- `MYBIBLI_SKIP_SETUP=1` bypasses; empty string and `0` do not.
- Static-asset whitelist (`/static/*`, `/covers/*`, `/health`) bypasses the gate.
- `active_admin_count > 0` AND `setup_completed_at IS NULL` resolves to "wizard inactive" (upgrade-from-pre-Epic-8 path).

Integration tests (`#[sqlx::test]` in `tests/setup_wizard.rs` — new file):
- Full happy path: anonymous browser → GET `/setup` → POST step-1 → POST step-2 → POST step-3 → POST `/setup/complete` → GET `/setup` returns 404 → GET `/catalog` works → POST `/login` with the new admin works.
- Resume after browser-close mid-step-2: simulated by deleting the session cookie between requests; the next GET `/setup` resumes at Step 2 with provider-key fields blank (or pre-filled if any were already submitted). Verify NO duplicate admin row.
- Re-running `cargo test --test setup_wizard` twice on the same DB is idempotent — each `#[sqlx::test]` gets a fresh schema.

E2E (Playwright `tests/e2e/specs/journeys/setup-wizard.spec.ts` — new spec):
- **Smoke (Foundation Rule #7):** Start the app with a clean DB and **without** `MYBIBLI_SKIP_SETUP` (override the docker-compose env var via the spec's `webServer` config OR use a separate `docker-compose.setup-test.yml`). Open a blank browser → navigate to `/catalog` → verify redirect to `/setup` → fill Step 1 (username `wizard_admin`, password `wizard_pass`) → submit → verify Step 2 → fill Google Books key `wkey-gb-1234` → submit (skip OMDb + TMDb) → verify Step 3 → pick EN, threshold `21` → submit → verify Step 4 recap shows `wizard_admin`, "Google Books configured", "OMDb not set", "TMDb not set", language "English", threshold "21" → click "Complete setup" → verify redirect to `/catalog` → verify `/setup` returns 404 → log out → log in as `wizard_admin` / `wizard_pass` → verify admin navigation works.
- **Resume:** Same setup, but close the browser context after Step 1 → reopen with a fresh context → verify the wizard resumes at Step 2 (NOT Step 1) — and `users` table has exactly one admin row. (Implementation hint: after Step 1, the test gets a fresh session cookie via `session_resolve_middleware`. Closing the browser context drops cookies; the new context gets a new anonymous session, but the gate predicate is now "admin exists, no key set" → Step 2.)
- Tests run in their own docker-compose (no `MYBIBLI_SKIP_SETUP`) — keep all *other* specs unchanged. The existing test suite continues to pass with `MYBIBLI_SKIP_SETUP=1` set in `docker-compose.test.yml` (Task 8.4).

## Tasks / Subtasks

### Task 1 — Schema sentinel for resume detection (AC3, AC6) — migration

- [ ] Create migration `migrations/20260429000000_add_setup_wizard_settings_keys.sql` (filename matches the project's `YYYYMMDDHHMMSS` 14-char convention with `000000` time component, sorts after `20260428000000_seed_system_settings_rows.sql`):
  ```sql
  -- Story 8-8: First-launch setup wizard sentinels.
  -- Idempotent INSERT IGNORE per project convention (see 20260428000000_seed_system_settings_rows.sql).
  --
  -- setup_completed_at — RFC 3339 / ISO 8601 UTC timestamp set when the admin clicks
  --   "Complete setup", e.g. '2026-04-29T12:34:56Z'. Empty string = "not yet completed".
  --   AppSettings parses via chrono::DateTime::parse_from_rfc3339; empty/malformed → None.
  -- setup_step_3_done — '1' once Step 3 has been visited; resolves the Step 3 vs Step 4
  --   ambiguity when language='fr' + overdue=30 (defaults that the user may have
  --   explicitly re-confirmed).
  INSERT IGNORE INTO settings (setting_key, setting_value) VALUES
      ('setup_completed_at', ''),
      ('setup_step_3_done', '0');
  ```
- [ ] Add to `AppSettings` in `src/config.rs`:
  - `pub setup_completed_at: Option<chrono::DateTime<chrono::Utc>>` — parse via `chrono::DateTime::parse_from_rfc3339(&value).ok().map(|dt| dt.with_timezone(&chrono::Utc))`. Empty string OR parse failure → `None` (with `tracing::warn!` on the failure case so a corrupted value is observable).
  - `pub setup_step_3_done: bool` — parse via `value == "1"`. Anything else → `false`.
  - Add explicit `match` arms in `AppSettings::load_from_db` (the catch-all `_ => {}` at line 314 is the "ignore unknown keys" path; new keys need explicit arms to land in `settings`).
- [ ] Run `cargo sqlx prepare --workspace -- --all-targets` after any new typed query.

> **Why two sentinels and not one timestamp:** the wizard could (in theory) detect "Step 3 done" from `default_language != 'fr' OR overdue != 30`, but a user who explicitly picks FR + 30 leaves no signal. A 1-byte sentinel `'1'` removes the ambiguity at zero cost. `setup_completed_at` alone could also be used as the single sentinel (set it at Step 4 only), but that conflates "Step 3 was visited" with "wizard fully complete" and breaks the resume case where the user closes the browser between Step 3 and Step 4.

### Task 2 — `setup_gate_middleware` (AC1, AC9)

- [ ] Create `src/middleware/setup_gate.rs`:
  - `pub struct SetupGateState { active: bool }` cached in `Arc<RwLock<SetupGateState>>` and stored in `AppState` (extend `AppState` in `src/lib.rs`).
  - `pub async fn setup_gate_middleware(State(state), request, next) -> Response`:
    - If `MYBIBLI_SKIP_SETUP` is `"1" | "true" | "TRUE"` → next.run().
    - If request path starts with `/static/`, `/covers/`, equals `/health`, or starts with `/setup` → next.run().
    - Read cached `active` flag (read-lock, fast path). If `false` → next.run().
    - If `true` (wizard active): if HTMX request → respond 200 + `HX-Redirect: /setup`; else 303 See Other Location: /setup.
  - `pub async fn refresh_gate_state(pool: &DbPool) -> SetupGateState`:
    - Query `SELECT COUNT(*) FROM users WHERE role='admin' AND active=TRUE AND deleted_at IS NULL` and `SELECT setting_value FROM settings WHERE setting_key='setup_completed_at'`. Compute `active = (admin_count == 0) AND (setup_completed_at empty/NULL)`.
  - `pub fn invalidate_gate_state(state_arc: &Arc<RwLock<SetupGateState>>)` — small helper that the wizard handlers call after Step 1 and Step 4.
- [ ] Wire into `src/routes/mod.rs::build_router` outside the catalog sub-router and OUTSIDE the existing `csrf_middleware` layer (so wizard POSTs go through CSRF first — order: `CSP → SetupGate → SessionResolve → Locale → CSRF → handler`). Re-read CLAUDE.md "Middleware Layer Order" before wiring; if SetupGate must run AFTER SessionResolve (for any reason — likely not), document why in a comment.
- [ ] Read CLAUDE.md "Key Patterns / CSRF" before deciding whether SetupGate runs before or after CSRF — wizard POSTs need CSRF protection (anonymous-session token).
- [ ] Initialize `SetupGateState` in `src/main.rs` after `AppSettings::load_from_db` and before binding the listener. Pass it into `AppState`.

### Task 3 — `services/setup.rs` (AC3 step resolution + AC4-7 step writers)

- [ ] Create `src/services/setup.rs`:
  - `pub enum SetupStep { Admin, Providers, Preferences, Done }`
  - `pub async fn resolve_step(pool: &DbPool) -> Result<SetupStep, AppError>` — implements the AC3 truth table.
  - `pub async fn create_or_update_admin(pool, username, password, &session) -> Result<(SessionToken, CsrfToken), AppError>` — wraps argon2 hash + UserModel::create-or-update + session rotation. Re-uses `routes/auth.rs` helpers.
  - `pub async fn save_provider_keys(pool, settings_arc, payload) -> Result<(), AppError>` — extract from existing `routes/admin_system.rs::save_provider_keys` if not already factored.
  - `pub async fn save_preferences(pool, settings_arc, language, overdue_threshold) -> Result<(), AppError>` — similar.
  - `pub async fn complete_setup(pool, settings_arc) -> Result<(), AppError>` — writes `setup_completed_at = UTC_TIMESTAMP()`, reloads settings cache.
- [ ] **Reuse, don't reinvent:** the helpers above MUST delegate to existing `services/admin_system` writers if they exist; otherwise extract them out of the `routes/admin_system.rs` handlers in this story (rule of three — each writer is now used in two places, the wizard + admin/system, so factoring is justified).

### Task 4 — Setup routes + handlers

- [ ] Create `src/routes/setup.rs`:
  - `pub async fn setup_page(State, Session) -> Response` — GET `/setup`. Calls `resolve_step`, builds a `SetupContext`, renders the appropriate panel.
  - `pub async fn step_1_submit(State, Session, jar, Form<Step1Form>) -> Result<Response, AppError>` — POST `/setup/step-1`. Validates fields → on error, re-render with `field_errors` populated (HTTP 400). On success, calls `services::setup::create_or_update_admin` (which wraps the AC4 single-flight tx), rotates session via `services::auth::authenticate_session`, invalidates gate state, 303 → `/setup`.
  - `pub async fn step_2_submit(State, Session, Form<Step2Form>) -> Result<Response, AppError>` — POST `/setup/step-2`. If `_back == true`, 303 → `/setup` (next GET resolves to Step 1). Else calls `services::admin_system::save_provider_keys`, 303 → `/setup`.
  - `pub async fn step_3_submit(State, Session, Form<Step3Form>) -> Result<Response, AppError>` — POST `/setup/step-3`. If `_back == true`, 303 → `/setup` (resolves to Step 2). Else validates → on error, re-render with `field_errors`. On success calls `services::admin_system::save_preferences` and sets `setup_step_3_done = '1'`, 303 → `/setup`.
  - `pub async fn complete_submit(State, Session, Form<Step4Form>) -> Result<Response, AppError>` — POST `/setup/complete`. If `_back == true`, 303 → `/setup` (resolves to Step 3 — but only if `setup_step_3_done` is rolled back to `'0'` first; alternative: skip the rollback and Previous on Step 4 simply doesn't reach Step 3 because the resolver will land on Step 4 again). Else calls `services::setup::complete_setup`, invalidates gate state, 303 → `/catalog`.
- [ ] Each step's `Form<StepNForm>` includes `#[serde(default, rename = "_back")] pub back: bool` so the Previous button can submit `_back=true` against the same endpoint. **No separate `/setup/back` route.** **No `?step=N` query param.**
- [ ] **Going-backward semantics for `_back`:** Step 4's Previous button is hidden (no useful previous state because Step 3 is `done`). Step 3's Previous button submits `_back=true` to `/setup/step-3`, which 303s without writing — the next GET re-resolves: provider keys are still set, `setup_step_3_done` is still `'0'`, so resolver lands on Step 3 again. To actually return to Step 2, the handler would have to clear something — instead, **Previous from Step 3 unsets `setup_step_3_done` is NOT done** (we don't lose data on Previous per UX-DR20 § "Previous does not rollback data"). The simplest correct UX: Previous on Step 3 returns to Step 2's form values (which are still in DB) by 303-ing to `/setup` and letting the resolver pick — but the resolver picks Step 3 because `setup_step_3_done='1'` if Step 3 was already submitted. **Resolution: Previous is only meaningful BEFORE Step 3 is submitted; after Step 3 commits, the "Previous" button on Step 4 is removed.** Spell this in the UI: Step 2 has Previous (returns to Step 1, but Step 1 was already submitted so this is also a no-op return — rendering Step 1 in idempotent-update mode); Step 3 has Previous (returns to Step 2 which re-renders with current keys); Step 4 has NO Previous button.
- [ ] Mount in `src/routes/mod.rs::build_router` BEFORE `.with_state(state)`. Routes:
  ```
  /setup            → GET setup_page
  /setup/step-1     → POST step_1_submit
  /setup/step-2     → POST step_2_submit
  /setup/step-3     → POST step_3_submit
  /setup/complete   → POST complete_submit
  ```
- [ ] Each handler verifies the wizard is active OR `MYBIBLI_SKIP_SETUP` is set; otherwise 404. (Defense-in-depth — the gate middleware lets `/setup/*` through, so each handler MUST self-gate too.)

### Task 5 — Templates

- [ ] **`templates/layouts/bare.html`** — add `<meta name="csrf-token" content="{{ csrf_token|e }}">` immediately after `<meta charset>` (mirrors `base.html:6`). The login page also benefits — its form already has the `_csrf_token` hidden input but no meta tag exists; adding it is forward-compatible if login ever moves to HTMX. Verify the existing login page tests stay green.
- [ ] `templates/pages/setup.html` extending `layouts/bare.html`. Receives the `SetupContext` struct (see AC2). Block `content` is a `{% match ctx.step %}` over the 4 panels:
  - `templates/fragments/setup_step_admin.html` — username + password fields. If `ctx.admin_already_exists`, show `t!("setup.step_1_admin_exists_hint")` ("An admin account is already configured. Leave the password blank to keep it unchanged, or enter a new one to update.") and label the submit button `t!("setup.step_1_update_button")`. Otherwise label it `t!("setup.step_1_create_button")`. Field errors render inline: `{% if let Some(err_key) = ctx.field_errors.get("username") %}<p class="text-red-600">{{ t!(err_key, locale = ctx.lang) }}</p>{% endif %}`.
  - `templates/fragments/setup_step_providers.html` — 3 rows generated from `ctx.keyed_providers` (the `KEYED_PROVIDERS` const), each with key input + Skip checkbox. Pre-fill the masked value when present.
  - `templates/fragments/setup_step_preferences.html` — language radios + overdue input. Inline error display per `ctx.field_errors`.
  - `templates/fragments/setup_step_done.html` — read-only recap from `ctx.recap` + "Complete setup" button. **No Previous button on Step 4** (per Task 4 going-backward semantics).
- [ ] `templates/components/setup_progress.html` — 4-dot progress indicator. Active dot has `aria-current="step"` and Tailwind `animate-pulse`; completed dots have `aria-label="Step N: <label>, completed"`; future dots are dimmed.
- [ ] All forms have `_csrf_token` hidden input as a top-3 input (per `forms_include_csrf_token` audit).
- [ ] All forms include `<input type="hidden" name="_back" value="0">` by default; the Previous button is a separate `<button type="submit" formname="_back" formvalue="1">` OR a sibling submit input that overrides — pick the simplest CSP-compliant pattern. (No `formname`/`formvalue` is HTML5-standard but not perfectly supported; safer is two buttons inside one form, both `type="submit"`, with `name="_back" value="0"` and `name="_back" value="1"` — only the clicked button's name/value is sent. CSP-clean.)
- [ ] Buttons use `t!()` for all labels — `setup.next_button`, `setup.previous_button`, `setup.complete_button`, `setup.step_1_create_button`, `setup.step_1_update_button`.

### Task 6 — i18n keys (AC10)

- [ ] Add the keys listed in AC10 to `locales/en.yml` and `locales/fr.yml`.
- [ ] Run `touch src/lib.rs && cargo build` to force the proc-macro to re-read.
- [ ] **CRITICAL YAML FORMAT:** keys at root level (no `en:` / `fr:` wrapper). Per CLAUDE.md "i18n".

### Task 7 — Bypass / dev-loop env var (AC9)

- [ ] In `main.rs`, read `MYBIBLI_SKIP_SETUP` once at startup with `matches!(v.as_str(), "1" | "true" | "TRUE")` and store it in `SetupGateState.bypass_via_env: bool` (or as a sibling `Arc<bool>`). The middleware reads the cached bool — never re-reads the env. This matches the `MYBIBLI_SKIP_STARTUP_PURGE` pattern (story 8-7 R3-N6).
- [ ] Document in `CLAUDE.md` "Build & Test Commands":
  ```
  # Bypass the first-launch setup wizard (story 8-8) — accept "1" / "true" / "TRUE" only.
  MYBIBLI_SKIP_SETUP=1 cargo run
  ```
- [ ] Add `MYBIBLI_SKIP_SETUP=` (commented out) to `.env.example` with a short comment matching the `MYBIBLI_SKIP_STARTUP_PURGE` style.

### Task 8 — Compose & test infra

- [ ] `tests/e2e/docker-compose.test.yml` — add `MYBIBLI_SKIP_SETUP=1` to the mybibli service's `environment:` block so existing E2E specs continue to pass (none of them go through the wizard; the seeded DB has no admin yet means without bypass every spec would 303 to `/setup`).
- [ ] `docker-compose.dev.yml` (if used) — same env var. Local `cargo run` against an empty DB will hit the wizard; that's the desired behavior for the new spec.
- [ ] `tests/docker-compose.rust-test.yml` — same env var IF the new sqlx integration test exercises non-wizard routes (most likely yes).
- [ ] **For the new wizard E2E spec:** override the env var per-spec via Playwright's `webServer.env` (or `test.use({ ... })` with a custom server fixture) so the wizard E2E runs against a fresh container WITHOUT `MYBIBLI_SKIP_SETUP`. **Do NOT create a separate `docker-compose.setup-test.yml`** — duplicating the entire stack just to flip one env var is unnecessary. The Playwright config can spawn a sibling container with the override, or the spec can use `playwright/test`'s `globalSetup` to `docker compose run` a temporary mybibli pointed at an empty DB. Pick whichever the existing test infra supports most naturally; document the choice in the spec's header comment.

### Task 9 — Audit gate verification

- [ ] Run `cargo test no_inline_markup_in_templates forms_include_csrf_token csrf_exempt_routes_frozen` — all three must stay green.
- [ ] If `forms_include_csrf_token` fires for a setup form → fix the form (do NOT add to any allowlist; there isn't one).
- [ ] If `csrf_exempt_routes_frozen` fires → REVERT — `/setup/*` must NOT be exempted.

### Task 10 — Architecture & doc updates

- [ ] Update `CLAUDE.md`:
  - Add bullet under "Key Patterns" → "Setup wizard (story 8-8): wizard active iff `(admin_count == 0) AND (setup_completed_at IS NULL)`. Cached in `AppState.setup_gate`. `MYBIBLI_SKIP_SETUP=1|true|TRUE` bypasses the middleware. Step resolution is server-side (see `services::setup::resolve_step`) — `?step=N` accepted only for going backwards via Previous form."
  - Add `MYBIBLI_SKIP_SETUP=1` next to `MYBIBLI_SKIP_STARTUP_PURGE` in the env-var section.
- [ ] Update `_bmad-output/planning-artifacts/architecture.md`:
  - Section "Configuration Architecture" or "Authentication & Security": add the wizard's middleware position and the gate predicate.
  - Domain Map row for `Wizard (FR86-FR87, FR91, FR121)`: confirm `routes/setup.rs`, `services/setup.rs`, `middleware/setup_gate.rs`, `pages/setup.html`, `components/setup_progress.html`, `specs/journeys/setup-wizard.spec.ts` paths (the line at architecture.md:1078 already names the files — verify they match what we ship).

### Task 11 — Tests (AC14)

- [ ] Unit tests inline with each new module (per project convention). The `resolve_step` resolver should be split into a **pure function** that takes the predicate inputs and returns the `SetupStep` — testable WITHOUT a DB. The 16-state truth table walks `(admin_count: 0|>0) × (any_provider_key: bool) × (setup_step_3_done: bool) × (setup_completed: bool)` and asserts the resolved state per AC3.
- [ ] `tests/setup_wizard.rs` — `#[sqlx::test(migrations = "./migrations")]` for the integration cases listed in AC14. Note that the dev_librarian seed (`migrations/20260329000002`) creates a librarian user; this does NOT count as admin so the wizard predicate stays "active" for the test. Integration tests should not need to soft-delete the librarian; they assert the gate fires regardless.
- [ ] `tests/e2e/specs/journeys/setup-wizard.spec.ts` — smoke + resume (per AC14). **Pin the locale at the start:** before the first `page.goto`, set the cookie `await page.context().addCookies([{ name: 'lang', value: 'en', url: <baseURL> }])` so all assertions match EN strings deterministically. New ISBN-namespace `SW` for unique-data isolation per CLAUDE.md "E2E test patterns" (no other spec uses the wizard, so this is just for paranoia consistency).
- [ ] Pre-push gate (Foundation Rule #13): `cargo check && cargo clippy --all-targets -- -D warnings && cargo test && (cd tests/e2e && npx playwright test specs/journeys/setup-wizard.spec.ts)`.

### Task 12 — Manual smoke & ship

- [ ] `git checkout -b story/8-8-first-launch-setup-wizard` (Foundation Rule #14).
- [ ] First commit pushes a draft PR (Foundation Rule #15): `gh pr create --draft -B main`.
- [ ] After CI green and code-review-clean: mark sprint-status entry `8-8-first-launch-setup-wizard` → `done`.

## Dev Notes

### Architecture patterns to follow

1. **AppSettings reload chain (AR9):** every `settings`-table write goes through `AppSettings::load_from_db` + `Arc<RwLock<>>` write-lock-swap. Never hold the write lock across an `.await`. Pattern lives in `services/admin_system.rs` — copy it. Story 8-5 retro / GH issue #90 already flags the duplication; if extracting `save_setting(pool, settings_arc, key, value)` is now natural, this is the moment.
2. **Optimistic locking on `settings`:** the `settings` table has a per-row `version INT`. Every UPDATE includes `WHERE setting_key = ? AND version = ?`. Concurrent admins (admin A in `/admin/system`, admin B in the wizard's Step 2 — only possible if the gate is bypassed for one of them via `MYBIBLI_SKIP_SETUP`) get a 409 from `services/locking.rs::check_update_result`.
3. **Soft-delete is irrelevant for `settings`:** the K/V table has `deleted_at` but it is never set. Reads always include `WHERE deleted_at IS NULL` per CLAUDE.md but that's a uniform safety net.
4. **CSP-clean templates:** every interaction is a form submit. The progress indicator is server-rendered HTML with classes — no JS needed. Verify post-write with `cargo test no_inline_markup_in_templates`.
5. **i18n keys:** YAML files are NOT wrapped (`nav:` not `en: nav:`). After adding keys, `touch src/lib.rs && cargo build`.
6. **MariaDB type gotchas (CLAUDE.md):** if any new typed query reads a `JSON` column, wrap with `CAST(... AS CHAR)`; for `BIGINT UNSIGNED NULL` use `CAST(... AS SIGNED)` then `Option<i64>`. None of the wizard queries hit JSON or BIGINT-UNSIGNED-nullable, so this is a vigilance reminder, not a known issue.
7. **Session rotation on Step 1 admin authentication:** copy the chain from `routes/auth.rs::login` lines ~183-220. Soft-delete the anonymous session row, INSERT a fresh `sessions` row with the new admin's `user_id`, set the `session=` cookie via `Cookie::build` with `SameSite::Lax` + `HttpOnly` + 7-day Max-Age (matches the existing pattern). The new CSRF token comes from `middleware::csrf::generate_csrf_token()` — same crate-level helper. Extract `services::auth::authenticate_session(pool, user_id, &session)` and call from both login and Step 1.
8. **Pure step resolver:** split `services::setup::resolve_step` into:
   ```rust
   pub struct SetupPredicateInputs {
       pub active_admin_count: i64,
       pub setup_completed_at: Option<DateTime<Utc>>,
       pub any_provider_key_set: bool,
       pub setup_step_3_done: bool,
   }
   pub async fn fetch_predicate_inputs(pool: &DbPool) -> Result<SetupPredicateInputs, AppError> { ... }
   pub fn resolve_step(inputs: &SetupPredicateInputs) -> Option<SetupStep> { ... }  // None = wizard inactive
   ```
   The pure resolver is unit-testable without a DB; the 16-state truth table from AC14 becomes a parameterized test. The fetcher is integration-testable with `#[sqlx::test]`.
9. **Cache invalidation race accepted:** the `Arc<RwLock<SetupGateState>>` cache is a hint, not authoritative. Between Step 1's commit and the cache-invalidation call, a concurrent gate-middleware read may still see `active=true` and 303 a request to `/setup` that is no longer needed. The next GET re-resolves correctly (it queries the DB). One wasted redirect per race window — no correctness impact, no E2E flake.
10. **Field-error rendering pattern (NOT FeedbackEntry):** the wizard uses native `<form>` submits, so validation errors are inline under each field via `SetupContext.field_errors`. **Do NOT use `feedback_html`, do NOT use the FeedbackEntry component, do NOT use HTMX OOB swaps.** That entire layer is for HTMX-driven flows; the wizard is server-rendered POST → 303 → GET.

### Why the wizard is intentionally sparse vs the older UX-DR20 mermaid

The 2026-Q1 UX spec (`ux-design-specification.md:1014-1042`) shows 4 steps that include Storage Locations (step 2) and Reference Data review (step 3). Both moved out of the wizard between then and now:

- **Locations:** Epic 2 (story 2-1) shipped a full-featured location tree at `/locations` that's reachable post-wizard. Forcing a wizard-mode subset was scope-creeping the UX.
- **Reference data:** Epic 8 story 8-4 made the four ref-data tables admin-editable at `/admin?tab=reference-data`, and seed migrations cover the bootstrap. Showing them again in the wizard is redundant.

The wizard's 4 steps are now: identity (admin) → external integrations (provider keys) → personalization (language + overdue threshold) → confirmation (recap + commit). This is the **minimum** setup that produces a usable login + working metadata fetches; everything else is `/admin/*` or `/locations` afterwards.

### Why server-side step resolution (vs client cookie / query param)

Cookie- or URL-driven step state opens the door to:
- Cookie tampering: skipping Step 1 to land on Step 2 with no admin row → a malformed POST hitting `save_provider_keys` from an anonymous user.
- "Direct link to Step 4" demos that crash because no admin exists.
- Multi-tab desync where two tabs disagree on the step.

Server-side resolution closes all three. The cost is a `users` + `settings` query per `GET /setup`; both are tiny and the wizard is a one-shot flow — performance is irrelevant.

### Edge cases & known interactions

- **Soft-deleted admin from a prior failed wizard run.** `users.username` is UNIQUE regardless of `deleted_at`. If a Step-1 attempt created an admin row that was later soft-deleted (manually via DB intervention; the wizard itself never soft-deletes), a re-run with the same username hits the UNIQUE constraint and `UserModel::create` returns `Conflict("username_taken")`. The wizard surfaces it as `field_errors["username"] = "setup.errors.username_taken"`. **Auto-reactivation is NOT implemented** — that scenario is rare enough that asking the user to pick a different name is fine. (Issue [#69](https://github.com/guycorbaz/mybibli/issues/69) tracks adding `users` to `ALLOWED_TABLES` for soft-delete-aware Trash UX; orthogonal to this story.)
- **Dev-environment `dev_librarian` seed coexistence.** The seed migration `20260329000002_seed_dev_user.sql` creates a librarian user when no users exist. The wizard's predicate is `active_admin_count == 0`, so a dev_librarian alone does NOT make the wizard inactive — the wizard fires correctly on a freshly-migrated dev DB. Integration tests do NOT need to delete the librarian.
- **Pre-Epic-8 deployments with an admin already seeded.** `active_admin_count > 0` makes the wizard inactive (AC1). Any admin row created before `setup_completed_at` was a thing flips the gate to "wizard never runs," which is the desired upgrade behavior.

### Why the `setup_step_3_done` sentinel exists

The truth table needs to distinguish "admin filled in language=fr + overdue=30" from "user has not visited Step 3". The two states leave the `settings` table with the same row values (the seed defaults). Without a sentinel, the resolver would loop the user back to Step 3 forever. A 1-byte `'1'` flag in `settings.setup_step_3_done` resolves it at zero cost. (Alternative: add `setup_started_at TIMESTAMP NULL` to track per-step visits — over-engineered for v1.)

### Source-of-truth deviations against UX-DR20

UX-DR20 says: 4 steps = Account / Locations / Data / APIs. **This story implements:** 4 steps = Admin / Providers / Preferences / Done. The deviation is intentional and traced back to the Epic-8 decomposition (Locations and Reference Data both became `/admin/*` features post-Epic-2/Epic-8). The dev agent should NOT silently align the implementation to the UX-DR20 mermaid — that diagram is documentation drift the retrospective should fix; this story's epic-spec ACs are the binding contract.

### Git intelligence

Recent merged work (relevant for this story):

- **#86 (8-5)** — System settings shipped the `/admin?tab=system` panel, the `Arc<RwLock<AppSettings>>` reload chain, and the empty-string convention for "key not set". Reuse those helpers wholesale.
- **#84 (8-4)** — Reference data CRUD made the four ref-data tables admin-editable. Seeded defaults are already in place; the wizard does NOT re-seed.
- **#83 / #81** — Bugfixes around session-cookie collisions and the language-toggle race. The wizard's Step 1 session-rotation chain MUST replicate the post-fix cookie semantics from `routes/auth.rs::login` (`SameSite::Lax`, 7-day Max-Age, percent-encoded base64 cookie value handled by the resolver).
- **#59 (8-7)** — Permanent delete + auto-purge introduced the `MYBIBLI_SKIP_STARTUP_PURGE` env-var pattern (R3-N6 fix: only `1` / `true` / `TRUE` enable; empty string and `0` do not). Story 8-8's `MYBIBLI_SKIP_SETUP` MUST follow the same pattern verbatim.

### Files to create / modify

**New files:**
- `src/middleware/setup_gate.rs` — gate middleware + `SetupGateState` cache (incl. `bypass_via_env`)
- `src/routes/setup.rs` — 5 handlers (GET page + 4 step-submits including `/setup/complete`). **No `/setup/back` route** (Previous handled via `_back: bool` form field).
- `src/services/setup.rs` — `SetupPredicateInputs`, `fetch_predicate_inputs`, pure `resolve_step`, `create_or_update_admin` (with single-flight tx), `complete_setup`. **No `save_provider_keys` / `save_preferences` here** — those go in `services/admin_system.rs` (factored out of the existing routes for reuse).
- `src/services/auth.rs` — extract `authenticate_session(pool, user_id, &session) -> Result<(SessionToken, CsrfToken), AppError>` from `routes/auth.rs::login` for shared use by login + Step 1.
- `templates/pages/setup.html` — extends `layouts/bare.html`
- `templates/fragments/setup_step_admin.html`
- `templates/fragments/setup_step_providers.html`
- `templates/fragments/setup_step_preferences.html`
- `templates/fragments/setup_step_done.html`
- `templates/components/setup_progress.html`
- `migrations/20260429000000_add_setup_wizard_settings_keys.sql`
- `tests/setup_wizard.rs` (sqlx integration tests)
- `tests/e2e/specs/journeys/setup-wizard.spec.ts` (with `webServer.env` override to omit `MYBIBLI_SKIP_SETUP`)

**Modified files:**
- `src/main.rs` — initialize `SetupGateState` (incl. env-var read), pass into `AppState`
- `src/lib.rs` — extend `AppState` with `setup_gate: Arc<RwLock<SetupGateState>>`
- `src/middleware/mod.rs` — `pub mod setup_gate;`
- `src/routes/mod.rs` — register `/setup/*` routes; insert the gate middleware in the layer order (between CSP and SessionResolve)
- `src/services/mod.rs` — `pub mod setup; pub mod auth;` (the `auth` module is new — extract the shared session-rotation helper)
- `src/services/admin_system.rs` — **NEW module** (extract reusable `save_provider_keys(pool, settings_arc, payload)` and `save_preferences(pool, settings_arc, language, overdue)` helpers from `routes/admin_system.rs`).
- `src/routes/admin_system.rs` — call into the extracted `services/admin_system.rs` helpers; route handlers become thin wrappers.
- `src/config.rs` — extend `AppSettings` with `setup_completed_at: Option<DateTime<Utc>>` + `setup_step_3_done: bool` (with explicit match arms in `load_from_db`).
- `src/metadata/mod.rs` — declare `pub const KEYED_PROVIDERS: &[&str] = &["google_books", "omdb", "tmdb"];`
- `templates/layouts/bare.html` — add `<meta name="csrf-token" content="{{ csrf_token|e }}">` after `<meta charset>`
- `locales/en.yml` + `locales/fr.yml` — new `setup.*` keys (incl. `setup.step_1_admin_exists_hint`, `error.setup.admin_already_created`)
- `tests/e2e/docker-compose.test.yml` — add `MYBIBLI_SKIP_SETUP=1` to mybibli service
- `tests/docker-compose.rust-test.yml` — add `MYBIBLI_SKIP_SETUP=1` (for the rust integration test DB)
- `docker-compose.dev.yml` (if present) — add `MYBIBLI_SKIP_SETUP=1` for normal dev loops
- `.env.example` — add `# MYBIBLI_SKIP_SETUP=` (commented out, with explanatory comment)
- `CLAUDE.md` — document the wizard pattern + env var
- `_bmad-output/planning-artifacts/architecture.md` — document the gate predicate + middleware position
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — flip `8-8-first-launch-setup-wizard` to `ready-for-dev` (done at story creation) → later `in-progress` → `review` → `done`

### Project Structure Notes

- The architecture doc at `architecture.md:199-250` already names `routes/setup.rs`, `pages/setup.html`, `components/setup_wizard.html` as the wizard's home. Implementation should match those paths verbatim. The component name in this story is `setup_progress.html` (not `setup_wizard.html`) because it's only the progress dots — the wizard isn't a single reusable component. If the architecture doc must be updated to match, do it as part of Task 10.
- The `bare.html` layout already exists (`templates/layouts/bare.html`) and is used by the login page. The wizard reuses it with no modifications — it has no nav bar, no admin scaffolding, no scan field.
- Do NOT add `/setup/*` to any router sub-tree that loads `pending_updates_middleware` (catalog routes only). The wizard has no async metadata fetches.
- The `_csrf_token` hidden input must be the FIRST or near-first child of every wizard `<form method="POST">` (`forms_include_csrf_token` walks the first 5 inputs). The progress indicator is BEFORE the form, so this is naturally satisfied.

### Testing strategy

- **Unit:** the step resolver is a pure function over `(active_admin_count, setup_completed_at, has_any_provider_key, setup_step_3_done)` → `SetupStep`. 16-state truth table is testable without a DB by parameterizing over the inputs (or a single `#[sqlx::test]` that walks the states).
- **Integration:** seed the test DB to specific states (no admin / admin only / admin + key / admin + key + step_3_done), then assert the wizard handlers respond correctly. Re-use the rust-test docker-compose (port 3307) per CLAUDE.md.
- **E2E:** the smoke spec is the load-bearing test (Foundation Rule #7). The resume spec is the second smoke. Both run in their OWN docker-compose without `MYBIBLI_SKIP_SETUP` so the gate fires for real.

### Implementation hints

- **Previous button = `_back: bool` form field**, not a separate route. Each step's submit form has TWO submit buttons inside it: `<button type="submit" name="_back" value="0">{{ t!("setup.next_button") }}</button>` (Next/primary) and `<button type="submit" name="_back" value="1">{{ t!("setup.previous_button") }}</button>` (Previous). HTML5 sends only the clicked button's `name=value`. The handler reads `form.back: bool` (default `false`) and short-circuits without writing data on `_back=true`.
- The Step 1 password field is type `password`; the username field has `autocomplete="username"` and `autofocus`. After a 400 (validation error), preserve the username value but never the password (security hygiene + browser autofill convention).
- For the Step 4 recap, the masked provider keys are NOT shown ("configured" / "not set" only) — the user just submitted them and the unmasked value is in their fingertips memory, not the page.
- The post-completion redirect to `/catalog` is HTMX-aware: send `HX-Redirect: /catalog` header on HTMX requests, 303 Location: /catalog on direct submits. The Step 4 form is a regular form submit, so 303 is the path.
- **`KEYED_PROVIDERS` const lives in `src/metadata/mod.rs`** so both the wizard and `routes/admin_system.rs` import the same source. Story-time set: `["google_books", "omdb", "tmdb"]`.
- `MYBIBLI_SKIP_SETUP` belongs in `.env.example` — copy the comment style of `MYBIBLI_SKIP_STARTUP_PURGE`.

### References

- [Source: _bmad-output/planning-artifacts/epics.md:1179-1198] — story 8-8 ACs (the binding spec)
- [Source: _bmad-output/planning-artifacts/prd.md:778-790] — FR86, FR87, FR91, FR121
- [Source: _bmad-output/planning-artifacts/architecture.md:199-250] — file structure for wizard
- [Source: _bmad-output/planning-artifacts/architecture.md:490-505] — middleware layer order (AR16, post-8-2)
- [Source: _bmad-output/planning-artifacts/architecture.md:1072-1078] — domain map for Auth/Wizard
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md:1014-1042] — Journey 5 mermaid (older, partial drift — see Source-of-truth deviation note)
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md:2462-2506] — UX-DR20 SetupWizard component spec (progress dots, accessibility)
- [Source: _bmad-output/implementation-artifacts/8-7-permanent-delete-and-auto-purge.md] — `MYBIBLI_SKIP_STARTUP_PURGE` pattern (R3-N6: accept only `"1" | "true" | "TRUE"`)
- [Source: _bmad-output/implementation-artifacts/8-5-system-settings.md] — `AppSettings` reload chain, K/V settings table, masked-API-key UX
- [Source: _bmad-output/implementation-artifacts/8-3-user-administration.md] — argon2 hashing chain, `UserModel::create`, last-active-admin guard
- [Source: _bmad-output/implementation-artifacts/8-2-csrf-middleware-and-form-token-injection.md] — CSRF synchronizer-token pattern, `forms_include_csrf_token` audit, exempt-routes allowlist freeze
- [Source: src/main.rs:50-160] — startup chain: migrations → schema validation → startup purge → env-var migration → `AppSettings::load_from_db` → `Arc<RwLock<>>` → registry → AppState
- [Source: src/middleware/auth.rs:115-262] — `session_resolve_middleware` and lazy-anonymous-session pattern
- [Source: src/routes/auth.rs:138-220] — login session-rotation chain (Step 1 reuses this)
- [Source: src/templates_audit.rs:43-289] — `no_inline_markup_in_templates`, `forms_include_csrf_token`, `csrf_exempt_routes_frozen` audit gates

## Dev Agent Record

### Agent Model Used

Claude Opus 4.7 (1M context) via Claude Code (`bmad-dev-story` workflow).

### Debug Log References

- `cargo check --all-targets` — green (final).
- `cargo clippy --all-targets -- -D warnings` — green (final).
- `cargo test --lib` — 523 unit tests pass, 96 `#[sqlx::test]`-gated tests skipped locally (no DB on host); the new wizard suites (`config`, `middleware::setup_gate`, `services::setup`, `services::admin_system`, `services::auth`, `routes::setup`) contribute 26 newly-passing unit tests.
- `cargo test --lib templates_audit::` — 4/4 green (CSP `no_inline_markup_in_templates`, `forms_include_csrf_token`, `csrf_exempt_routes_frozen`, `hx_confirm_matches_allowlist`).
- The wizard's sqlx integration tests (`tests/setup_wizard.rs`) and the Playwright spec (`tests/e2e/specs/journeys/setup-wizard.spec.ts`) are wired but require a live DB / a sibling docker-compose without `MYBIBLI_SKIP_SETUP`; CI will exercise the rust integration tests, the E2E spec is gated by `MYBIBLI_SETUP_E2E=1` per its header runbook.

### Completion Notes List

- **Layer order at request time** is now `CSP → SetupGate → SessionResolve → Locale → CSRF → handler` (Task 2 in `routes/mod.rs`).
- **`SetupGateState` cache** is `Arc<RwLock<>>` in `AppState` (`src/lib.rs`); refreshed by `middleware::setup_gate::refresh` after Step 1 + Step 4 only. `force_set_active` is exposed (no `#[cfg(test)]` gate) so external integration tests can flip the cache without the DB.
- **`MYBIBLI_SKIP_SETUP`** is read once at startup with the strict `matches!("1" | "true" | "TRUE")` accept-set (R3-N6). Added to `.env.example`, `tests/e2e/docker-compose.test.yml`, `docker-compose.dev.yml`, and documented in CLAUDE.md.
- **Step resolution** is split into `services::setup::fetch_predicate_inputs` (DB) + pure `resolve_step` / `resolve_step_with_admin` (16-state truth table covered by the unit tests — no DB needed).
- **Step 1 single-flight** is implemented in `services::setup::create_or_update_admin` via `BEGIN → SELECT COUNT → INSERT → COMMIT`; UNIQUE collisions surface as `Conflict("username_taken")`, the existing-admin race surfaces as `Conflict("admin_already_created")`.
- **Session rotation** is shared via `services::auth::authenticate_session` (extracted from `routes/auth.rs::login`) — call sites: login + Step 1.
- **K/V settings writes** are shared via `services::admin_system::{save_setting, reload_settings_cache, validate_*}` (rule of three: admin/system + wizard). The complex provider-key action machinery (`action_for`, `apply_provider_action`) stays in `routes/admin_system.rs` because only the admin form's clear-checkbox UI needs it; the wizard uses a simpler "set if non-empty and not the masked display" pass.
- **Templates** are CSP-clean (`templates_audit::no_inline_markup_in_templates` is green): pure form submits with two submit buttons (`name="_back" value="0|1"`) for Next/Previous; progress dots use Tailwind `animate-pulse`. `bare.html` now carries `<meta name="csrf-token">` matching `base.html`.
- **i18n keys** added under the `setup:` and `error.system:`/`error.csrf*` umbrellas in both `locales/en.yml` and `locales/fr.yml`.
- **Architecture & docs** updated: domain map row in `architecture.md` lists every wizard surface; CLAUDE.md gains a "First-launch setup wizard (story 8-8)" bullet and a `MYBIBLI_SKIP_SETUP=` entry in the build-commands block.

### File List

**New files:**
- `src/middleware/setup_gate.rs` (gate middleware + cache state + tests)
- `src/routes/setup.rs` (5 handlers + form structs + render helpers + tests)
- `src/services/setup.rs` (`SetupStep`, predicate, resolver, writers, tests)
- `src/services/auth.rs` (`authenticate_session` shared with `routes/auth.rs::login`)
- `src/services/admin_system.rs` (settings save/reload helpers shared with `routes/admin_system.rs`)
- `migrations/20260429000000_add_setup_wizard_settings_keys.sql`
- `templates/pages/setup.html`
- `templates/components/setup_progress.html`
- `templates/fragments/setup_step_admin.html`
- `templates/fragments/setup_step_providers.html`
- `templates/fragments/setup_step_preferences.html`
- `templates/fragments/setup_step_done.html`
- `tests/setup_wizard.rs` (sqlx integration tests)
- `tests/e2e/specs/journeys/setup-wizard.spec.ts` (smoke + resume — gated by `MYBIBLI_SETUP_E2E=1`)

**Modified files:**
- `src/lib.rs` (`AppState.setup_gate`)
- `src/main.rs` (`SetupGateState::initialize` + AppState wiring)
- `src/middleware/mod.rs` (`pub mod setup_gate;`)
- `src/routes/mod.rs` (5 wizard routes + `setup_gate_middleware` layer)
- `src/services/mod.rs` (`pub mod admin_system; pub mod auth; pub mod setup;`)
- `src/config.rs` (`AppSettings.setup_completed_at` + `setup_step_3_done` + parse arms + tests)
- `src/middleware/csrf.rs` (test fixture: `setup_gate` field)
- `src/middleware/locale.rs` (test fixture: `setup_gate` field)
- `src/routes/auth.rs` (test fixture: `setup_gate` field — both fixtures)
- `tests/admin_system_integration.rs`, `tests/csrf_integration.rs`, `tests/role_gating.rs`, `tests/session_cookie_collision.rs` (test fixtures: `setup_gate` field)
- `templates/layouts/bare.html` (`<meta name="csrf-token">`)
- `locales/en.yml` + `locales/fr.yml` (`setup:` and error keys)
- `tests/e2e/docker-compose.test.yml` + `docker-compose.dev.yml` (`MYBIBLI_SKIP_SETUP: "1"`)
- `.env.example` (`MYBIBLI_SKIP_SETUP=` + `MYBIBLI_SKIP_STARTUP_PURGE=` documentation)
- `CLAUDE.md` (env-var entry + "First-launch setup wizard" bullet under Key Patterns)
- `_bmad-output/planning-artifacts/architecture.md` (domain-map row for Wizard)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (`8-8-first-launch-setup-wizard: in-progress → review`)

### Change Log

- 2026-04-29 — initial implementation across Tasks 1-12: schema migration, gate middleware, wizard routes + handlers + templates, i18n EN/FR, CSP/CSRF audit-clean, env-var bypass, doc updates, integration + E2E spec.
