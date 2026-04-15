# Route Role Matrix

**Status:** authoritative reference for Story 7-1 (anonymous browsing + role gating).
**Last updated:** 2026-04-15.

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

Columns: `method | path | current_role | target_role | note`. Rows where `current ≠ target` are the Task 2 worklist.

### Root / infrastructure

| method | path | current | target | note |
|---|---|---|---|---|
| GET | `/` | Anonymous | Anonymous | Homepage with search. Librarian-only metadata-error badge is template-gated. |
| GET | `/health` | Anonymous | Anonymous | Liveness probe. |
| POST | `/session/keepalive` | Anonymous | Anonymous | Ping; updates `last_activity`. Anonymous sessions noop. |

### Auth (`src/routes/auth.rs`)

| method | path | current | target | note |
|---|---|---|---|---|
| GET | `/login` | Anonymous | Anonymous | Accepts `?next=` (Task 3). Redirects Librarian+ to `/catalog`. |
| POST | `/login` | Anonymous | Anonymous | On success: redirect to same-origin `next` if present, else `/`. |
| GET | `/logout` | Anonymous | Anonymous | Idempotent; soft-deletes session row. |
| POST | `/logout` | Anonymous | Anonymous | Same as GET. |

### Catalog (`src/routes/catalog.rs`)

| method | path | current | target | note |
|---|---|---|---|---|
| GET | `/catalog` | Librarian | **Anonymous** | **CHANGE** — AC #1. Scan-field + edit affordances template-gated. |
| POST | `/catalog/scan` | Librarian | Librarian | — |
| POST | `/catalog/scan-with-type` | Librarian | Librarian | — |
| GET | `/catalog/title/new` | Librarian | Librarian | — |
| POST | `/catalog/title` | Librarian | Librarian | — |
| GET | `/catalog/title/fields/{media_type}` | Librarian | Librarian | HTMX fragment. |
| GET | `/catalog/contributors/form` | Librarian | Librarian | — |
| GET | `/catalog/contributors/search` | Librarian | Librarian | — |
| POST | `/catalog/contributors/add` | Librarian | Librarian | — |
| POST | `/catalog/contributors/remove` | Librarian | Librarian | — |
| POST | `/catalog/contributors/update` | Librarian | Librarian | — |
| DELETE | `/catalog/contributors/{id}` | Librarian | Librarian | — |
| DELETE | `/catalog/title/{id}` | Librarian | Librarian | — |
| DELETE | `/catalog/volume/{id}` | Librarian | Librarian | — |

### Titles (`src/routes/titles.rs`)

| method | path | current | target | note |
|---|---|---|---|---|
| GET | `/title/{id}` | Anonymous | Anonymous | — |
| GET | `/title/{id}/metadata` | Anonymous | Anonymous | Embeds Librarian-only edit/redownload buttons; template-gated. |
| GET | `/title/{id}/edit` | Librarian | Librarian | — |
| POST | `/title/{id}` | Librarian | Librarian | — |
| POST | `/title/{id}/redownload` | Librarian | Librarian | — |
| POST | `/title/{id}/confirm-metadata` | Librarian | Librarian | — |
| POST | `/title/{id}/series` | Librarian | Librarian | — |
| POST | `/title/{id}/series/{assignment_id}/remove` | Librarian | Librarian | — |
| POST | `/title/{id}/series-remove` | Librarian | Librarian | — |

### Volumes (handled in `catalog.rs`)

| method | path | current | target | note |
|---|---|---|---|---|
| GET | `/volume/{id}` | Anonymous | Anonymous | — |
| GET | `/volume/{id}/edit` | Librarian | Librarian | — |
| POST | `/volume/{id}/update` | Librarian | Librarian | — |

### Contributors (`src/routes/contributors.rs`)

| method | path | current | target | note |
|---|---|---|---|---|
| GET | `/contributor/{id}` | Anonymous | Anonymous | — |

### Series (`src/routes/series.rs`)

| method | path | current | target | note |
|---|---|---|---|---|
| GET | `/series` | Anonymous | Anonymous | — |
| GET | `/series/{id}` | Anonymous | Anonymous | — |
| GET | `/series/new` | Librarian | Librarian | — |
| POST | `/series` | Librarian | Librarian | — |
| GET | `/series/{id}/edit` | Librarian | Librarian | — |
| POST | `/series/{id}` | Librarian | Librarian | — |
| DELETE | `/series/{id}` | Librarian | Librarian | — |

### Locations (`src/routes/locations.rs`)

| method | path | current | target | note |
|---|---|---|---|---|
| GET | `/location/{id}` | Anonymous | Anonymous | — |
| GET | `/locations` | Librarian | **Anonymous** | **CHANGE** — AC #1 "location browse page". |
| GET | `/locations/next-lcode` | Admin | **Librarian** | **CHANGE** — decision 1a. Used by create form. |
| GET | `/locations/{id}/edit` | Admin | **Librarian** | **CHANGE** — decision 1a. |
| POST | `/locations` | Admin | **Librarian** | **CHANGE** — decision 1a. |
| POST | `/locations/{id}` | Admin | **Librarian** | **CHANGE** — decision 1a. |
| DELETE | `/locations/{id}` | Admin | Admin | Destructive; stays Admin (decision 1a exception). Used as smoke-test 403 candidate. |

### Borrowers (`src/routes/borrowers.rs`)

| method | path | current | target | note |
|---|---|---|---|---|
| GET | `/borrowers` | Librarian | Librarian | — |
| POST | `/borrowers` | Librarian | Librarian | — |
| GET | `/borrowers/search` | Librarian | Librarian | — |
| GET | `/borrower/{id}` | Librarian | Librarian | — |
| GET | `/borrower/{id}/edit` | Admin | **Librarian** | **CHANGE** — decision 2a. |
| POST | `/borrower/{id}` | Admin | **Librarian** | **CHANGE** — decision 2a. |
| DELETE | `/borrower/{id}` | Admin | Admin | **Smoke-test 403 target (AC #9).** Destructive; stays Admin (decision 2a exception). |

### Loans (`src/routes/loans.rs`)

| method | path | current | target | note |
|---|---|---|---|---|
| GET | `/loans` | Librarian | Librarian | — |
| POST | `/loans` | Librarian | Librarian | — |
| GET | `/loans/scan` | Librarian | Librarian | — |
| POST | `/loans/{id}/return` | Librarian | Librarian | — |

### Static

| method | path | current | target | note |
|---|---|---|---|---|
| GET | `/static/*` | Anonymous | Anonymous | ServeDir; CSS/JS/favicon. |
| GET | `/covers/*` | Anonymous | Anonymous | ServeDir; cover images. |

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
