# Story 1.9: Minimal Login/Logout

Status: done

## Story

As a librarian,
I want to log in with my username and password,
so that I can access the catalog and start cataloging books.

## Acceptance Criteria (BDD)

### AC1: Login Form

**Given** an anonymous user visits `/login`,
**When** the page loads,
**Then** a login form is displayed with username and password fields, a submit button, and a link back to home.

### AC2: Successful Authentication

**Given** a user submits valid credentials (username + password),
**When** the form is submitted,
**Then** a session is created (token in DB + HttpOnly cookie), and the user is redirected to `/catalog`.

### AC3: Failed Authentication

**Given** a user submits invalid credentials,
**When** the form is submitted,
**Then** an error message is displayed on the login form ("Invalid username or password") and no session is created.

### AC4: Logout

**Given** an authenticated user clicks "Log out",
**When** the logout is processed,
**Then** the session is soft-deleted in the DB, the cookie is cleared, and the user is redirected to `/`.

### AC5: Session Cookie Security

**Given** a session is created,
**When** the cookie is set,
**Then** it is HttpOnly, SameSite=Lax, path=/, no max-age (expires on browser close). Token is 256-bit cryptographically random (NFR10).

### AC6: E2E Smoke Test — Full User Journey

**Given** a blank browser (no cookies),
**When** a user navigates to `/`, clicks Login, enters credentials, submits, then scans an ISBN on `/catalog`,
**Then** the entire flow works end-to-end without any manual cookie injection.

### AC7: Dev Seed User Works

**Given** the dev seed migration has run,
**When** the dev_librarian user logs in with the seeded password,
**Then** authentication succeeds and the user can access `/catalog`.

## Explicit Scope Boundaries

**In scope:**
- `GET /login` — login form page
- `POST /login` — authenticate, create session, set cookie, redirect
- `POST /logout` — soft-delete session, clear cookie, redirect
- Login page template (`templates/pages/login.html`)
- Auth route module (`src/routes/auth.rs`)
- Password verification with Argon2 (already in Cargo.toml)
- Session token generation (256-bit random, base64 encoded)
- Fix dev seed: replace placeholder hash with real Argon2 hash for a known password
- E2E smoke test: blank browser → login → catalog → scan ISBN → title created
- i18n keys for login form labels and error messages

**NOT in scope:**
- User registration / account creation (Epic 7)
- Password reset (Epic 7)
- Admin user management (Epic 8)
- CSRF tokens (deferred — noted in deferred-work.md)
- "Remember me" / persistent sessions
- OAuth / SSO

## Tasks / Subtasks

- [x] Task 1: Fix dev seed user password hash (AC: 7)
  - [x] 1.1 Created migration `20260331000004_fix_dev_user_hash.sql` with real Argon2id hash for password "dev"
  - [x] 1.2 Original seed migration untouched
  - [x] 1.3 Hash generated via standalone Rust script using argon2 0.5

- [x] Task 2: Auth route module (AC: 1, 2, 3, 4, 5)
  - [x] 2.1 Created `src/routes/auth.rs` with `login_page()`, `login()`, `logout()`
  - [x] 2.2 login_page: redirects authenticated users to /catalog, renders form for anonymous
  - [x] 2.3 login: queries users table, verifies Argon2 hash, creates session + cookie, redirects to /catalog
  - [x] 2.4 logout: soft-deletes session, removes cookie, redirects to /
  - [x] 2.5 Token: `rand::random::<[u8; 32]>()` + base64 STANDARD encode → 44 chars. Added `base64 = "0.22"`
  - [x] 2.6 Cookie: HttpOnly, SameSite=Lax, path=/. No secure flag (HTTP on NAS).

- [x] Task 3: Login template (AC: 1, 3)
  - [x] 3.1 Created `templates/pages/login.html` — centered card, extends base.html
  - [x] 3.2 Error display: red border-l alert with role="alert"
  - [x] 3.3 Standard POST form, no HTMX
  - [x] 3.4 Accessibility: labels, autofocus, autocomplete attributes

- [x] Task 4: Register routes (AC: all)
  - [x] 4.1 Added `pub mod auth;` to routes/mod.rs
  - [x] 4.2 Registered GET+POST /login, GET+POST /logout
  - [x] 4.3 Changed Unauthorized redirect from `/` to `/login`

- [x] Task 5: i18n keys (AC: 1, 3)
  - [x] 5.1 Added login.title, login.username_label, login.password_label, login.submit, login.error_invalid, login.back_to_home
  - [x] 5.2 French translations added
  - [x] 5.3 Ran `touch src/lib.rs` before build

- [x] Task 6: Unit tests (AC: 2, 3, 5)
  - [x] 6.1 Password verification: valid, invalid, malformed hash (3 tests)
  - [x] 6.2 Token generation: length 44, valid base64, unique (3 tests)
  - [x] 6.3 Login template renders with and without error (2 tests)
  - [x] 6.4 Unauthorized redirect to /login verified (1 test)

- [x] Task 7: E2E smoke test — FULL user journey (AC: 6, 7)
  - [x] 7.1 CRITICAL: blank browser → / → Login → credentials → /catalog → scan ISBN → feedback
  - [x] 7.2 Invalid credentials → error alert on /login
  - [x] 7.3 Logout → redirect to /, /catalog redirects to /login
  - [x] 7.4 Anonymous /catalog → redirect to /login

## Dev Notes

### Architecture Compliance

- **Service layer:** Auth logic can live directly in route handlers — login/logout are thin HTTP operations, not business logic requiring a service
- **Error handling:** `AppError` enum — `Unauthorized` redirects to `/login`
- **Logging:** `tracing::info!` for login success/failure (audit trail)
- **i18n:** `t!("key")` for all user-facing text. **Run `touch src/lib.rs` after locale changes!**
- **DB queries:** `WHERE deleted_at IS NULL AND active = TRUE` on users table
- **Session cookie:** Name is `"session"` (verified in auth middleware). HttpOnly, SameSite=Lax, path=/, no max-age.

### What Already Exists (DO NOT recreate)

- `src/middleware/auth.rs` — Session extractor, Role enum, `require_role()`. Already reads cookie `"session"` and queries `SessionModel::find_with_role()`.
- `src/models/session.rs` — `find_with_role()`, `update_last_activity()`, `set_current_title()`, `increment_session_counter()`. All use `deleted_at IS NULL`.
- `templates/components/nav_bar.html` — Already has `{% if role == "anonymous" %}<a href="/login">{{ nav_login }}</a>{% else %}<a href="/logout">{{ nav_logout }}</a>{% endif %}`. No changes needed.
- `argon2 = "0.5"` in Cargo.toml — already available.
- `rand = "0.8"` in Cargo.toml — already available.
- `axum-extra` with `cookie` feature — already available.

### Dev Seed User

- Username: `dev_librarian`
- Password: `dev` (new migration will set correct Argon2 hash)
- Role: `librarian`
- Pre-seeded session token: `ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2` (still works for existing E2E tests)

### Base64 Encoding for Token

Add `base64 = "0.22"` to Cargo.toml. Usage: `base64::engine::general_purpose::STANDARD.encode(&random_bytes)`. 32 random bytes → 44-char base64 string → fits `sessions.token VARCHAR(44)` exactly.

### Logout Pattern

The nav bar currently has `<a href="/logout">` which is a GET link. But logout should be POST (not GET) to prevent CSRF. Options:
1. Change to `<form method="POST" action="/logout"><button type="submit">{{ nav_logout }}</button></form>` (safest)
2. Keep GET but also support POST (pragmatic for MVP — single-user NAS)
3. Use HTMX: `hx-post="/logout"` with redirect

**Recommended for MVP:** Support both GET and POST on `/logout` route. The nav bar can use a simple link (GET) for now.

### References

- [Source: _bmad-output/implementation-artifacts/epic-1-retro-2026-03-31.md#Action-Items]
- [Source: _bmad-output/planning-artifacts/prd.md#FR66, #FR67, #FR69, #NFR9, #NFR10]
- [Source: _bmad-output/planning-artifacts/architecture.md#Session-Lifecycle]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- `rand::thread_rng().gen()` is a reserved keyword in Rust 2024 edition — used `rand::random()` instead
- `time::Duration::ZERO` not available — used `CookieJar::remove()` instead of max-age=0
- `touch src/lib.rs` required after adding i18n keys (proc macro rebuild)

### Completion Notes List

- **Task 1:** New migration fixes dev_librarian password hash for password "dev"
- **Task 2:** Full auth module: login_page, login (Argon2 verify + session create + cookie), logout (soft-delete + cookie remove)
- **Task 3:** Centered login form with error display, accessibility attributes, dark mode support
- **Task 4:** Routes registered, Unauthorized now redirects to /login
- **Task 5:** i18n keys EN+FR for login form
- **Task 6:** 8 new unit tests (password verify, token gen, template render, redirect)
- **Task 7:** E2E smoke test: full journey without cookie injection

### Change Log

- 2026-03-31: Implemented story 1-9: Minimal Login/Logout — all 7 tasks complete

### File List

**New files:**
- `migrations/20260331000004_fix_dev_user_hash.sql`
- `src/routes/auth.rs`
- `templates/pages/login.html`
- `tests/e2e/specs/journeys/login-smoke.spec.ts`

**Modified files:**
- `Cargo.toml` — added `base64 = "0.22"`
- `src/routes/mod.rs` — added `pub mod auth`, registered /login and /logout routes
- `src/error/mod.rs` — Unauthorized redirect changed from `/` to `/login`
- `locales/en.yml` — added `login.*` keys
- `locales/fr.yml` — added French `login.*` translations
