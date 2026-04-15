# Story 7.1: Anonymous browsing + role gating

Status: in-progress

## Story

**As a** visitor, **I want** to browse and search the catalog without logging in, **so that** I can explore Guy's library before deciding to request access;
**and as a** librarian/admin, **I want** cataloging, editing, loan, and admin operations strictly gated by role, **so that** unauthorized users cannot mutate state or access private data.

**FRs:** FR65, FR66, FR67 · **NFRs:** NFR13

## Acceptance Criteria

1. Anonymous visitor (no `session` cookie) navigating to `/catalog`, `/series`, title detail, volume detail, contributor page, or a location browse page: page renders read-only — no edit/delete/create buttons, no loan actions, no scan field.
2. Anonymous visitor hitting `/loans`, `/borrowers`, or any borrower/loan detail route → 303 redirect to `/login?next=<original-url>` (with `HX-Redirect` header for HTMX). No data leaks in the response body.
3. Anonymous visitor attempting any write (POST/PUT/DELETE) on titles, volumes, contributors, locations, series, loans, borrowers → 303 to `/login?next=...` (HX-Redirect for HTMX). Zero state change (assert via DB snapshot in unit/integration tests).
4. Librarian-role user hitting admin-only routes (user management, system configuration, settings) → **403 Forbidden** rendered via `AppError` → `FeedbackEntry` (NOT a redirect; Librarian is already authenticated).
5. Admin role → all operations permitted.
6. Nav bar (`templates/components/nav_bar.html`): Anonymous sees "Login" + read-only items (Catalog, Series, Contributors, Locations); cataloging/loan/admin items hidden. Librarian sees cataloging + loan items; admin items hidden. Admin sees everything. The existing `/admin` dead link is removed or gated (closes Epic 5/6 carry-over).
7. **Route audit table** committed at `docs/route-role-matrix.md`: every route in `src/routes/` listed with method, path, required role (Anonymous / Librarian / Admin), and a one-line justification. This is a hard deliverable — do not ship the story without it.
8. Unit tests: role-gating enforcement for each of the 3 roles × at least 2 representative routes per role (6 happy-path + 6 reject-path assertions minimum).
9. E2E smoke (Foundation Rule #7): blank browser → browse `/catalog` anonymously → open a title → verify read-only DOM (no edit/delete buttons) → attempt `/loans` → assert redirect to `/login` → `loginAs(page, "librarian")` → verify cataloging unlocked → attempt `/admin` (or whichever admin-only route exists) → assert 403 feedback entry. Smoke spec must NOT use `DEV_SESSION_COOKIE`.

## Tasks / Subtasks

- [ ] **Task 1 — Route role audit + matrix (AC #7) — MUST complete before Task 2**
  - [ ] Grep existing `require_role` / `Role::` callsites (~51 occurrences across 8 route files) and list current guards
  - [ ] Inventory every handler in `src/routes/{auth,borrowers,catalog,contributors,home,loans,locations,series,titles}.rs` (method + path)
  - [ ] Categorize each: public-read (Anonymous), mutation (Librarian), admin-only (Admin), loan/borrower data (Librarian)
  - [ ] **Resolve the `create_location` / edit-location question** (currently Admin-only per Epic 6 §4 finding). Decision options: (a) promote to Librarian with rationale, or (b) keep Admin-only and document why. Decision must land in the matrix before Task 2 touches routes.
  - [ ] Write `docs/route-role-matrix.md` — columns: `method | path | current_role | target_role | note`. The `current != target` rows are the Task 2 worklist.

- [ ] **Task 2 — `require_role` enforcement per the matrix (AC #2, #3, #4, #5)**
  - [ ] For each matrix row where `current_role != target_role`, update the handler; do NOT touch handlers already correct
  - [ ] Anonymous read routes: no guard; templates decide affordances based on `session.role`
  - [ ] Auth check at the route layer, not in services — preserve existing service-layer `check_update_result` patterns
  - [ ] Note: current `AppError::Unauthorized` always redirects to `/login` (`src/error/mod.rs:39-48`) — correct for Anonymous, wrong for authenticated-but-insufficient. Task 5 introduces `Forbidden` to fix AC #4.

- [ ] **Task 3 — `?next=` return URL round-trip (AC #2)**
  - [ ] Add `AppError::UnauthorizedWithReturn(String)` variant alongside `Unauthorized`; its `into_response` appends `?next=<url-encoded>` to the `/login` Location + `HX-Redirect`
  - [ ] `Session::require_role` (or a new `require_role_with_return(min, uri)`) emits the new variant for GET redirects; POST/PUT/DELETE keep the plain `Unauthorized` (no point returning user to a failed write)
  - [ ] `GET /login` accepts `?next=` and passes it to the login form as a hidden field
  - [ ] `POST /login` success: redirect to `next` if present AND same-origin (path-only, no scheme/host), else `/`
  - [ ] Unit test the same-origin guard: reject `next=https://evil.example.com/`, `next=//evil.example.com/`, `next=javascript:...`

- [ ] **Task 4 — Template affordance gating (AC #1, #6)**
  - [ ] Introduce a `BaseContext { role: Role, is_authenticated: bool, can_edit: bool, can_loan: bool, can_admin: bool }` struct in `src/templates/` (or extend the existing nav context feeder). Every page template embeds it via Askama composition — one place to populate, not N ad-hoc fields.
  - [ ] Replace current per-template role plumbing (grep for existing `role` / `lang` params in page templates) with the `BaseContext` field on each template struct
  - [ ] Wrap edit/delete/create/loan/scan affordances in `{% if ctx.can_edit %}` / `{% if ctx.can_loan %}` blocks
  - [ ] `templates/components/nav_bar.html`: three-way conditional on role; Login link visible only when Anonymous; **remove the `/admin` dead link** (closes Epic 5/6 carry-over — we do NOT create a stub `/admin` route in this story; see Task 7 below for the smoke-test target)
  - [ ] Hide `#scan-field` entirely for Anonymous (no cataloging path)

- [ ] **Task 5 — `AppError::Forbidden` variant (AC #4)**
  - [ ] Add `Forbidden(String)` variant to `AppError` in `src/error/mod.rs`
  - [ ] Returns `StatusCode::FORBIDDEN` with a `FeedbackEntry` ("error" variant) body for non-HTMX; HTMX gets the same fragment via `HxResponse`
  - [ ] Add i18n keys `error.forbidden.title` / `error.forbidden.body` (EN + FR) and run `touch src/lib.rs && cargo build`
  - [ ] `Session::require_role` returns `Unauthorized` when `role == Anonymous`, `Forbidden` when authenticated-but-insufficient

- [ ] **Task 6 — Unit tests (AC #8)**
  - [ ] `src/middleware/auth.rs` tests: 3 roles × 2 routes, assert the correct `Result<(), AppError>` variant (existing test file, extend)
  - [ ] Route-layer tests (or a focused integration test via `tests/` with `#[sqlx::test]`) that POSTs as each role and asserts status + DB snapshot unchanged for the reject cases
  - [ ] `?next` same-origin guard test

- [ ] **Task 7 — E2E smoke (AC #9) — Foundation Rule #7 MANDATORY**
  - [ ] New spec `tests/e2e/specs/journeys/epic7-role-gating-smoke.spec.ts`
  - [ ] Blank browser → `/catalog` anonymous → assert no `[data-action="edit"]` / no `#scan-field`
  - [ ] Click title → assert read-only detail page
  - [ ] Navigate to `/loans` → assert URL becomes `/login?next=%2Floans`
  - [ ] `loginAs(page, "librarian")` → navigate to `/catalog` → assert edit affordances present
  - [ ] **Admin-only smoke target:** use an **existing** currently-Admin-guarded route as the 403 target — recommended: `POST /locations` (or whichever route Task 1 confirms as Admin-target in the matrix). Submit a request as librarian and assert 403 via visible feedback entry (NOT a redirect). Do NOT create a stub `/admin` route — Epic 8 owns admin surfaces.
  - [ ] Non-smoke role-gating specs can use `loginAs(page, role)` in `beforeEach` per CLAUDE.md
  - [ ] Use unique spec ISBNs via `specIsbn("RG", n)`
  - [ ] Zero `waitForTimeout` — use DOM-state matchers (CI grep gate will fail otherwise)

- [ ] **Task 8 — i18n keys**
  - [ ] Login link, "Return to previous page" (or similar) if used, forbidden error text, any new nav item strings — EN + FR in `locales/en.yml` + `locales/fr.yml`
  - [ ] After edits: `touch src/lib.rs && cargo build`

- [ ] **Task 9 — SQLx offline cache + quality gates**
  - [ ] `cargo sqlx prepare` if queries change (unlikely in this story — mostly middleware + templates)
  - [ ] `cargo clippy -- -D warnings` clean
  - [ ] `cargo test` full unit suite green
  - [ ] DB integration tests green on port 3307
  - [ ] Full E2E green on 3 consecutive fresh-Docker cycles before moving to `review`

## Dev Notes

### Architecture compliance

- **Session extractor** (`src/middleware/auth.rs`) already exists with `Role` enum (`Anonymous < Librarian < Admin`) and `require_role(min_role)`. Build on this — do not introduce a parallel auth system.
- **`AppError` is the only error type** (`src/error/mod.rs`). Add `Forbidden` there; do not invent ad-hoc responses.
- **HTMX contract:** every route must handle both `HxRequest(true)` (fragment) and `HxRequest(false)` (full page). The role-gated redirect already does the right thing — `HX-Redirect` header + 303 `Location` coexist.
- **No `anyhow` / raw error strings.** All auth errors flow through `AppError`.
- **Soft delete + versioning rules** are untouched by this story — it's auth-layer only.

### Current state observations (important)

- `Session::from_request_parts` returns `Session::anonymous()` for missing / invalid cookies — so every handler receives a `Session`, no `Option`. Auth gating is consistent.
- `update_last_activity` is already fired from the extractor — Story 7.2 will lean on this.
- Nav bar contains an `/admin` dead link (Epic 6 retro §5). **Remove** it in Task 4 (AC #6); do NOT create a stub route — Epic 8 owns admin surfaces.
- ~51 `require_role` / `Role::` callsites exist across 8 route files today. Task 1 matrix reconciles current vs. target; Task 2 only touches mismatches.

### Route layout (informs Task 1)

- `src/routes/auth.rs` — login/logout (Anonymous must POST login; GET login always accessible)
- `src/routes/home.rs` — home page, probably Anonymous-visible
- `src/routes/catalog.rs`, `titles.rs`, `series.rs`, `contributors.rs`, `locations.rs` — mixed: GET list/detail = Anonymous, POST/PUT/DELETE = Librarian (except location mutations which are currently Admin-only per Epic 6 retro §4; decide and document)
- `src/routes/loans.rs`, `borrowers.rs` — Librarian only, entire surface (GET + mutations) per Epic 7 scope note "anonymous visibility excludes loan-related data"

### Previous-story intelligence (Epic 6 retro, 2026-04-14)

1. **`create_location` is currently Admin-only** — Guy's 6-2 finding. Task 1 must resolve this (promote to Librarian OR keep Admin-only with rationale) before Task 2 touches routes.
2. **CSRF is out of scope for this story** — it is not in Epic 7 AC and deserves its own test surface. Tracked as a separate Epic 7 follow-up story (to be created); do NOT add CSRF in this story.
3. **Commit-per-story-at-review discipline.** Epic 6 retro §4 item 3 + Action Item §7 "Habit". Commit & push when moving to `review`, not at `done`.
4. **Grep gate for `waitForTimeout`** is live in CI (`e2e` job). Any new occurrence fails the PR.
5. **`loginAs(page, role)` is typed** as `"admin" | "librarian"` — use it; `tsc --noEmit` will catch typos.

### Testing standards

- Foundation Rule #3 — unit tests alongside implementation (no shipped code without them).
- Foundation Rule #7 — E2E smoke per epic, this is Epic 7's smoke test. Blank browser, real login, real navigation.
- `#[sqlx::test]` for route-layer integration if you go that direction (see `tests/find_similar.rs` for the pattern).
- Assertion style: DOM-state matchers via `expect(locator).toBeVisible()` or `.toContainText(/EN|FR/i)` — i18n-aware regex.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic 7 lines 930-957]
- [Source: _bmad-output/implementation-artifacts/epic-6-retro-2026-04-14.md §6, §7 — Epic 7 preview & action items]
- [Source: src/middleware/auth.rs — Session extractor + Role enum]
- [Source: src/error/mod.rs:34-83 — AppError::IntoResponse, Unauthorized redirect]
- [Source: templates/components/nav_bar.html — nav bar with /admin dead link]
- [Source: CLAUDE.md — Session, HTMX, i18n, E2E patterns]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
