# Route Role Matrix

**Status:** authoritative reference for Story 7-1 (anonymous browsing + role gating).
**Last updated:** 2026-04-27 (story 8-4 — 19 reference-data CRUD routes added under `/admin/reference-data/*`).

## CSRF exemption (story 8-2)

Every state-changing method (POST / PUT / PATCH / DELETE) on every route in
this matrix requires a matching `X-CSRF-Token` header or `_csrf_token` form
field — see `src/middleware/csrf.rs`. The sole exempt route is:

| method | path | rationale |
|---|---|---|
| POST | `/login` | No authenticated session exists at request time. `SameSite=Lax` on the session cookie is the login-CSRF mitigation (a cross-site top-level POST does not carry the cookie). |

Frozen by `src/templates_audit.rs::csrf_exempt_routes_frozen` — adding a new
exempt route requires a visible edit to `CSRF_EXEMPT_ROUTES` in
`src/middleware/csrf.rs` AND an update to the audit assertion in the same PR.

## Role model

- `Anonymous` — no `session` cookie (or invalid/expired).
- `Librarian` — authenticated user with role `librarian`.
- `Admin` — authenticated user with role `admin` (Guy).

Ordering: `Anonymous < Librarian < Admin`. A route requiring `Librarian` is also accessible to `Admin` (enforced by `Session::require_role`, `src/middleware/auth.rs`).

## Policy decisions (Task 1)

1. **Location mutations → Librarian**, except `DELETE /locations/{id}` which remains **Admin**. Rationale: daily cataloging (scan → shelve → occasionally create a new location) must not require Guy's intervention. Destructive removal of a location impacts taxonomy structure and loan/volume references, so deletion stays Admin.
2. **Borrower mutations → Librarian**, except `DELETE /borrower/{id}` which remains **Admin**. Rationale: consistent with `POST /borrowers` (already Librarian). Destructive removal of personal data (soft-delete but cascades loan deactivation) stays Admin.
3. **Anonymous reach** covers catalog browsing and detail pages only. All loan/borrower surfaces — including list pages — stay Librarian (Epic 7 scope: "anonymous visibility excludes loan-related data").
4. **`GET /catalog`** becomes Anonymous (AC #1). It's the primary entry for visitors.
5. **`GET /locations`** (tree browser) becomes Anonymous (AC #1 "location browse page").

## Matrix

Columns: `method | path | current_role | target_role | csrf_exempt | note`. Rows where `current ≠ target` are the Task 2 worklist. `csrf_exempt` is `yes` only for `POST /login` (per top section) and `—` for non-mutating methods (GET / HEAD / OPTIONS bypass CSRF at the middleware level); all other mutating rows are implicitly `no`.

### Root / infrastructure

| method | path | current | target | csrf_exempt | note |
|---|---|---|---|---|---|
| GET | `/` | Anonymous | Anonymous | — | Homepage with search. Librarian-only metadata-error badge is template-gated. |
| GET | `/health` | Anonymous | Anonymous | — | Liveness probe. |
| POST | `/session/keepalive` | Anonymous | Anonymous | no | Ping; updates `last_activity`. Anonymous sessions noop. |

### Auth (`src/routes/auth.rs`)

| method | path | current | target | csrf_exempt | note |
|---|---|---|---|---|---|
| GET | `/login` | Anonymous | Anonymous | — | Accepts `?next=` (Task 3). Redirects Librarian+ to `/catalog`. |
| POST | `/login` | Anonymous | Anonymous | **yes** | No authenticated session at request time; `SameSite=Lax` mitigates login-CSRF. Frozen in `CSRF_EXEMPT_ROUTES`. |
| GET | `/logout` | — | **405** | — | **CHANGE (story 8-2)** — GET `/logout` removed; the router only exposes POST so a cross-origin `<img src="/logout">` or mistyped anchor cannot end a session. |
| POST | `/logout` | Anonymous | Anonymous | no | Requires CSRF token. The nav-bar logout is a POST form (story 8-2). |
| POST | `/language` | Anonymous | Anonymous | no | Language toggle (story 7-3). Requires CSRF token (story 8-2 added hidden `_csrf_token` to the nav-bar form). Anonymous visitors carry a CSRF token via the lazy-anonymous session row. |

### Catalog (`src/routes/catalog.rs`)

| method | path | current | target | csrf_exempt | note |
|---|---|---|---|---|---|
| GET | `/catalog` | Librarian | **Anonymous** | — | **CHANGE** — AC #1. Scan-field + edit affordances template-gated. |
| POST | `/catalog/scan` | Librarian | Librarian | no | — |
| POST | `/catalog/scan-with-type` | Librarian | Librarian | no | — |
| GET | `/catalog/title/new` | Librarian | Librarian | — | — |
| POST | `/catalog/title` | Librarian | Librarian | no | — |
| GET | `/catalog/title/fields/{media_type}` | Librarian | Librarian | — | HTMX fragment. |
| GET | `/catalog/contributors/form` | Librarian | Librarian | — | — |
| GET | `/catalog/contributors/search` | Librarian | Librarian | — | — |
| POST | `/catalog/contributors/add` | Librarian | Librarian | no | — |
| POST | `/catalog/contributors/remove` | Librarian | Librarian | no | — |
| POST | `/catalog/contributors/update` | Librarian | Librarian | no | — |
| DELETE | `/catalog/contributors/{id}` | Librarian | Librarian | no | — |
| DELETE | `/catalog/title/{id}` | Librarian | Librarian | no | — |
| DELETE | `/catalog/volume/{id}` | Librarian | Librarian | no | — |

### Titles (`src/routes/titles.rs`)

| method | path | current | target | csrf_exempt | note |
|---|---|---|---|---|---|
| GET | `/title/{id}` | Anonymous | Anonymous | — | — |
| GET | `/title/{id}/metadata` | Anonymous | Anonymous | — | Embeds Librarian-only edit/redownload buttons; template-gated. |
| GET | `/title/{id}/edit` | Librarian | Librarian | — | — |
| POST | `/title/{id}` | Librarian | Librarian | no | — |
| POST | `/title/{id}/redownload` | Librarian | Librarian | no | — |
| POST | `/title/{id}/confirm-metadata` | Librarian | Librarian | no | — |
| POST | `/title/{id}/series` | Librarian | Librarian | no | — |
| POST | `/title/{id}/series/{assignment_id}/remove` | Librarian | Librarian | no | — |
| POST | `/title/{id}/series-remove` | Librarian | Librarian | no | — |

### Volumes (handled in `catalog.rs`)

| method | path | current | target | csrf_exempt | note |
|---|---|---|---|---|---|
| GET | `/volume/{id}` | Anonymous | Anonymous | — | — |
| GET | `/volume/{id}/edit` | Librarian | Librarian | — | — |
| POST | `/volume/{id}/update` | Librarian | Librarian | no | — |

### Contributors (`src/routes/contributors.rs`)

| method | path | current | target | csrf_exempt | note |
|---|---|---|---|---|---|
| GET | `/contributor/{id}` | Anonymous | Anonymous | — | — |

### Series (`src/routes/series.rs`)

| method | path | current | target | csrf_exempt | note |
|---|---|---|---|---|---|
| GET | `/series` | Anonymous | Anonymous | — | — |
| GET | `/series/{id}` | Anonymous | Anonymous | — | — |
| GET | `/series/new` | Librarian | Librarian | — | — |
| POST | `/series` | Librarian | Librarian | no | — |
| GET | `/series/{id}/edit` | Librarian | Librarian | — | — |
| POST | `/series/{id}` | Librarian | Librarian | no | — |
| DELETE | `/series/{id}` | Librarian | Librarian | no | — |

### Locations (`src/routes/locations.rs`)

| method | path | current | target | csrf_exempt | note |
|---|---|---|---|---|---|
| GET | `/location/{id}` | Anonymous | Anonymous | — | — |
| GET | `/locations` | Librarian | **Anonymous** | — | **CHANGE** — AC #1 "location browse page". |
| GET | `/locations/next-lcode` | Admin | **Librarian** | — | **CHANGE** — decision 1a. Used by create form. |
| GET | `/locations/{id}/edit` | Admin | **Librarian** | — | **CHANGE** — decision 1a. |
| POST | `/locations` | Admin | **Librarian** | no | **CHANGE** — decision 1a. |
| POST | `/locations/{id}` | Admin | **Librarian** | no | **CHANGE** — decision 1a. |
| DELETE | `/locations/{id}` | Admin | Admin | no | Destructive; stays Admin (decision 1a exception). Used as smoke-test 403 candidate. |

### Borrowers (`src/routes/borrowers.rs`)

| method | path | current | target | csrf_exempt | note |
|---|---|---|---|---|---|
| GET | `/borrowers` | Librarian | Librarian | — | — |
| POST | `/borrowers` | Librarian | Librarian | no | — |
| GET | `/borrowers/search` | Librarian | Librarian | — | — |
| GET | `/borrower/{id}` | Librarian | Librarian | — | — |
| GET | `/borrower/{id}/edit` | Admin | **Librarian** | — | **CHANGE** — decision 2a. |
| POST | `/borrower/{id}` | Admin | **Librarian** | no | **CHANGE** — decision 2a. |
| DELETE | `/borrower/{id}` | Admin | Admin | no | **Smoke-test 403 target (AC #9).** Destructive; stays Admin (decision 2a exception). |

### Loans (`src/routes/loans.rs`)

| method | path | current | target | csrf_exempt | note |
|---|---|---|---|---|---|
| GET | `/loans` | Librarian | Librarian | — | — |
| POST | `/loans` | Librarian | Librarian | no | — |
| GET | `/loans/scan` | Librarian | Librarian | — | — |
| POST | `/loans/{id}/return` | Librarian | Librarian | no | — |

### Admin (`src/routes/admin.rs`)

Story 8-1 introduced the `/admin` surface. Every handler's first line is `session.require_role_with_return(Role::Admin, …)?` — Librarian → 403 FeedbackEntry body; Anonymous → 303 → `/login?next=%2Fadmin`. All read-only in 8-1; mutations land in 8-2+.

| method | path                     | current | target | csrf_exempt | note                                                        |
|--------|--------------------------|---------|--------|-------------|-------------------------------------------------------------|
| GET    | `/admin`                 | —       | Admin  | —           | New (8-1). 5-tab shell; Librarian → 403, Anonymous → 303 /login. |
| GET    | `/admin/health`          | —       | Admin  | —           | New (8-1). Health panel fragment (HTMX + direct).                 |
| GET    | `/admin/users`           | —       | Admin  | —           | New (8-1). List + form container (story 8-3 implements).           |
| GET    | `/admin/users/new`       | —       | Admin  | —           | New (8-3). Create user form fragment (HTMX).                       |
| POST   | `/admin/users`           | —       | Admin  | no          | New (8-3). Create user; validates; hashes password Argon2.        |
| GET    | `/admin/users/{id}/edit` | —       | Admin  | —           | New (8-3). Edit user form fragment (HTMX, pre-filled).             |
| POST   | `/admin/users/{id}`      | —       | Admin  | no          | New (8-3). Update user (role, username, optional password).        |
| POST   | `/admin/users/{id}/deactivate` | — | Admin | no | New (8-3). Soft-delete user + invalidate sessions (atomic tx). |
| POST   | `/admin/users/{id}/reactivate` | — | Admin | no | New (8-3). Clear `deleted_at`; user can log in again.              |
| GET    | `/admin/reference-data`  | —       | Admin  | —           | New (8-1, filled by 8-4). Reference Data panel (4 sub-sections).  |
| GET    | `/admin/reference-data/genres` | — | Admin | — | New (8-4). Genres section list fragment. |
| POST   | `/admin/reference-data/genres` | — | Admin | no | New (8-4). Create genre (reactivates soft-deleted on name match). |
| POST   | `/admin/reference-data/genres/{id}/rename` | — | Admin | no | New (8-4). Rename with optimistic locking. |
| GET    | `/admin/reference-data/genres/{id}/delete-modal` | — | Admin | — | New (8-4). Delete-confirm modal fragment. |
| POST   | `/admin/reference-data/genres/{id}/delete` | — | Admin | no | New (8-4). Soft-delete; refused with 409 if `count_usage > 0`. |
| GET    | `/admin/reference-data/volume-states` | — | Admin | — | New (8-4). Volume states section list fragment. |
| POST   | `/admin/reference-data/volume-states` | — | Admin | no | New (8-4). Create volume state with `is_loanable` flag. |
| POST   | `/admin/reference-data/volume-states/{id}/rename` | — | Admin | no | New (8-4). Rename with optimistic locking. |
| GET    | `/admin/reference-data/volume-states/{id}/delete-modal` | — | Admin | — | New (8-4). Delete-confirm modal fragment. |
| POST   | `/admin/reference-data/volume-states/{id}/delete` | — | Admin | no | New (8-4). Soft-delete with usage guard. |
| POST   | `/admin/reference-data/volume-states/{id}/loanable` | — | Admin | no | New (8-4). Toggle `is_loanable`; surfaces warning modal if active loans exist. |
| POST   | `/admin/reference-data/volume-states/{id}/loanable/confirm` | — | Admin | no | New (8-4). Apply loanable toggle force=true (forward-only). |
| GET    | `/admin/reference-data/volume-states/{id}/row` | — | Admin | — | New (8-4). Re-render row partial — used by Cancel-on-warning to revert checkbox visual state. |
| GET    | `/admin/reference-data/contributor-roles` | — | Admin | — | New (8-4). Contributor roles section list fragment. |
| POST   | `/admin/reference-data/contributor-roles` | — | Admin | no | New (8-4). Create role (reactivates on collision). |
| POST   | `/admin/reference-data/contributor-roles/{id}/rename` | — | Admin | no | New (8-4). Rename role with optimistic locking. |
| GET    | `/admin/reference-data/contributor-roles/{id}/delete-modal` | — | Admin | — | New (8-4). Delete-confirm modal fragment. |
| POST   | `/admin/reference-data/contributor-roles/{id}/delete` | — | Admin | no | New (8-4). Soft-delete role with usage guard. |
| GET    | `/admin/reference-data/node-types` | — | Admin | — | New (8-4). Location node types section list fragment. |
| POST   | `/admin/reference-data/node-types` | — | Admin | no | New (8-4). Create node type. |
| POST   | `/admin/reference-data/node-types/{id}/rename` | — | Admin | no | New (8-4). Transactional rename — cascades to `storage_locations.node_type` (loose VARCHAR FK). |
| GET    | `/admin/reference-data/node-types/{id}/delete-modal` | — | Admin | — | New (8-4). Delete-confirm modal fragment. |
| POST   | `/admin/reference-data/node-types/{id}/delete` | — | Admin | no | New (8-4). Soft-delete node type; usage guard matches by name. |
| GET    | `/admin/trash`           | —       | Admin  | —           | New (8-1, filled by 8-6 & 8-7). List soft-deleted items; filter by type, search by name; paginated (25/page). |
| GET    | `/admin/trash/{table}/{id}/permanent-delete` | — | Admin | — | New (8-7). Show confirmation modal with friction (type name to enable button). |
| POST   | `/admin/trash/{table}/{id}/permanent-delete` | — | Admin | no | New (8-7). Hard-delete soft-deleted item; create audit entry; return feedback + OOB swap. |
| GET    | `/admin/system`          | —       | Admin  | —           | New (8-1). Stub panel — story 8-4 fills in.                       |

### Static

| method | path | current | target | csrf_exempt | note |
|---|---|---|---|---|---|
| GET | `/static/*` | Anonymous | Anonymous | — | ServeDir; CSS/JS/favicon. |
| GET | `/covers/*` | Anonymous | Anonymous | — | ServeDir; cover images. |

## Task 2 worklist (`current ≠ target`)

1. `GET /catalog` → drop `require_role`.
2. `GET /locations` → drop `require_role`.
3. `GET /locations/next-lcode` → `Admin` → `Librarian`.
4. `GET /locations/{id}/edit` → `Admin` → `Librarian`.
5. `POST /locations` → `Admin` → `Librarian`.
6. `POST /locations/{id}` → `Admin` → `Librarian`.
7. `GET /borrower/{id}/edit` → `Admin` → `Librarian`.
8. `POST /borrower/{id}` → `Admin` → `Librarian`.

9 changes to route guards; 58 handlers audited; **2 routes remain Admin**: `DELETE /borrower/{id}` and `DELETE /locations/{id}`.

## Smoke-test target (AC #9)

`DELETE /borrower/{id}` is the 403 assertion target. Flow: librarian creates a borrower → attempts DELETE → server returns 403 Forbidden (not a redirect) with a `FeedbackEntry` body.
