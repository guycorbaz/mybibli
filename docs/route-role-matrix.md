# Route role matrix

**What this is:** every route the router registers, with the role it
requires and whether it is subject to CSRF. Generated from
`src/routes/*.rs` and verified by hand.

**Last updated:** 2026-09-11 — **complete**: 172 method+path pairs over
148 paths. The previous revision stopped at story 8-5 (2026-04-28) and
documented 82 of them; everything added since — the setup wizard, the
HTTP API, the wishlist, saved searches, labels, API keys, the shelf
audit, every confirmation modal — was missing.

**How the roles below were established:** each row's role is the
`Session::require_role` / `require_role_with_return` call at the top of
its handler, or the `ApiKeyAuth` / `ApiKeyWrite` extractor in its
signature. A row reading **Anonymous** means the handler asks for
nothing — which is the intent for the public catalogue, and an accident
in two places (see *Anomalies* at the end).

**Keeping it honest:** a new route means a new row, in the PR that adds
it. The count above is the check — if `grep -c '\.route(' src/routes/*.rs`
disagrees with it, this file has drifted again.

## CSRF exemption

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

## Standing policy decisions

1. **Location mutations → Librarian**, except `DELETE /locations/{id}` which remains **Admin**. Rationale: daily cataloging (scan → shelve → occasionally create a new location) must not require Guy's intervention. Destructive removal of a location impacts taxonomy structure and loan/volume references, so deletion stays Admin.
2. **Borrower mutations → Librarian**, except `DELETE /borrower/{id}` which remains **Admin**. Rationale: consistent with `POST /borrowers` (already Librarian). Destructive removal of personal data (soft-delete but cascades loan deactivation) stays Admin.
3. **Anonymous reach** covers catalog browsing and detail pages only. All loan/borrower surfaces — including list pages — stay Librarian (Epic 7 scope: "anonymous visibility excludes loan-related data").
4. **`GET /catalog`** is Anonymous — the primary entry for visitors.
5. **`GET /locations`** (tree browser) is Anonymous, like the rest of the browse surface.

## Matrix

Columns: `method | path | role | csrf | note`. `csrf` is `—` for GET (the middleware only guards state-changing methods), `no` for every mutating route — meaning a token is required — and **yes** only for the single exempt route, `POST /login`.

### Root and infrastructure

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/health` | Anonymous | — | Liveness probe. Whitelisted by the setup gate so an orchestrator can reach it before the wizard runs. |

### Admin shell, Health tab and Trash (`src/routes/admin.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/admin` | Admin | — | Six-tab shell; Librarian → 403, Anonymous → 303 /login. |
| GET | `/admin/health` | Admin | — | Health panel fragment (HTMX + direct). |
| POST | `/admin/health/bulk-cover-refetch` | Admin | no | Long-running background action; paced and back-off'd (#419). |
| POST | `/admin/health/bulk-metadata-backfill` | Admin | no | Long-running background action over already-cataloged titles (#389). |
| GET | `/admin/trash` | Admin | — | List soft-deleted items; filter by type, search by name; paginated (25/page). |
| GET | `/admin/trash/{table}/{id}/permanent-delete` | Admin | — | Show confirmation modal with friction (type name to enable button). |
| POST | `/admin/trash/{table}/{id}/permanent-delete` | Admin | no | Hard-delete soft-deleted item; create audit entry; return feedback + OOB swap. |
| POST | `/admin/trash/{table}/{id}/restore` | Admin | no | Clear `deleted_at`, bump `version`. POST, not GET, so it rides the CSRF layer (#478). Returns the conflict modal first when restoring would clash with a newer relationship. |

### Admin → API keys (CR #241) (`src/routes/admin_api_keys.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/admin/api-keys` | Admin | — |  |
| POST | `/admin/api-keys` | Admin | no |  |
| DELETE | `/admin/api-keys/{id}` | Admin | no |  |
| GET | `/admin/api-keys/{id}/delete-modal` | Admin | — |  |
| POST | `/admin/api-keys/{id}/revoke` | Admin | no |  |
| GET | `/admin/api-keys/{id}/revoke-modal` | Admin | — |  |

### Admin → Reference data → Labels (#443) (`src/routes/admin_labels.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/admin/reference-data/labels` | Admin | — |  |
| POST | `/admin/reference-data/labels` | Admin | no |  |
| POST | `/admin/reference-data/labels/{id}/delete` | Admin | no |  |
| GET | `/admin/reference-data/labels/{id}/delete-modal` | Admin | — |  |
| POST | `/admin/reference-data/labels/{id}/rename` | Admin | no |  |

### Admin → Reference data (`src/routes/admin_reference_data.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/admin/reference-data` | Admin | — | Reference Data panel (4 sub-sections). |
| GET | `/admin/reference-data/contributor-roles` | Admin | — | Contributor roles section list fragment. |
| POST | `/admin/reference-data/contributor-roles` | Admin | no | Create role (reactivates on collision). |
| POST | `/admin/reference-data/contributor-roles/{id}/delete` | Admin | no | Soft-delete role with usage guard. |
| GET | `/admin/reference-data/contributor-roles/{id}/delete-modal` | Admin | — | Delete-confirm modal fragment. |
| POST | `/admin/reference-data/contributor-roles/{id}/rename` | Admin | no | Rename role with optimistic locking. |
| GET | `/admin/reference-data/genres` | Admin | — | Genres section list fragment. |
| POST | `/admin/reference-data/genres` | Admin | no | Create genre (reactivates soft-deleted on name match). |
| POST | `/admin/reference-data/genres/{id}/delete` | Admin | no | Soft-delete; refused with 409 if `count_usage > 0`. |
| GET | `/admin/reference-data/genres/{id}/delete-modal` | Admin | — | Delete-confirm modal fragment. |
| POST | `/admin/reference-data/genres/{id}/rename` | Admin | no | Rename with optimistic locking. |
| GET | `/admin/reference-data/node-types` | Admin | — | Location node types section list fragment. |
| POST | `/admin/reference-data/node-types` | Admin | no | Create node type. |
| POST | `/admin/reference-data/node-types/{id}/delete` | Admin | no | Soft-delete node type; usage guard matches by name. |
| GET | `/admin/reference-data/node-types/{id}/delete-modal` | Admin | — | Delete-confirm modal fragment. |
| POST | `/admin/reference-data/node-types/{id}/rename` | Admin | no | Transactional rename — cascades to `storage_locations.node_type` (loose VARCHAR FK). |
| GET | `/admin/reference-data/volume-states` | Admin | — | Volume states section list fragment. |
| POST | `/admin/reference-data/volume-states` | Admin | no | Create volume state with `is_loanable` flag. |
| POST | `/admin/reference-data/volume-states/{id}/delete` | Admin | no | Soft-delete with usage guard. |
| GET | `/admin/reference-data/volume-states/{id}/delete-modal` | Admin | — | Delete-confirm modal fragment. |
| POST | `/admin/reference-data/volume-states/{id}/loanable` | Admin | no | Toggle `is_loanable`; surfaces warning modal if active loans exist. |
| POST | `/admin/reference-data/volume-states/{id}/loanable/confirm` | Admin | no | Apply loanable toggle force=true (forward-only). |
| POST | `/admin/reference-data/volume-states/{id}/rename` | Admin | no | Rename with optimistic locking. |
| GET | `/admin/reference-data/volume-states/{id}/row` | Admin | — | Re-render row partial — used by Cancel-on-warning to revert checkbox visual state. |

### Admin → System settings (`src/routes/admin_system.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/admin/system` | Admin | — | System settings panel (3 forms: Loans, Providers, Language). |
| POST | `/admin/system/language` | Admin | no | Save default language (FR / EN); reloads cache; takes effect on next anonymous fresh-visitor request. |
| POST | `/admin/system/loans` | Admin | no | Save overdue threshold; reloads `AppSettings` cache. |
| POST | `/admin/system/log-level` | Admin | no |  |
| POST | `/admin/system/provider-timeouts` | Admin | no |  |
| POST | `/admin/system/providers` | Admin | no | Save 3 provider API keys (Google Books / OMDb / TMDb) in one transaction; NoChange/Clear/Set state machine; reloads cache. |
| POST | `/admin/system/sessions` | Admin | no |  |
| POST | `/admin/system/timeouts` | Admin | no |  |
| POST | `/admin/system/valuation` | Admin | no |  |

### Admin → Users (`src/routes/admin_users.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/admin/users` | Admin | — | List + form container (story 8-3 implements). |
| POST | `/admin/users` | Admin | no | Create user; validates; hashes password Argon2. |
| GET | `/admin/users/new` | Admin | — | Create user form fragment (HTMX). |
| POST | `/admin/users/{id}` | Admin | no | Update user (role, username, optional password). |
| POST | `/admin/users/{id}/deactivate` | Admin | no | Soft-delete user + invalidate sessions (atomic tx). |
| GET | `/admin/users/{id}/deactivate-modal` | Admin | — |  |
| GET | `/admin/users/{id}/edit` | Admin | — | Edit user form fragment (HTMX, pre-filled). |
| POST | `/admin/users/{id}/reactivate` | Admin | no | Clear `deleted_at`; user can log in again. |

### HTTP API v1 — bearer-token auth, no session (`src/routes/api_v1.rs`)

These routes authenticate with `Authorization: Bearer <key>` (or
`X-API-Key`), never with the session cookie, so `src/middleware/csrf.rs`
short-circuits the whole `/api/` prefix before its token check — the
`csrf` column below is therefore "not applicable" rather than "exempt".
Keys are minted and revoked in **Admin → API keys**; the scope
(read / write) is carried by the key, which is why the role column names
the key rather than a user.

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/api/v1/dewey/{prefix}` | API key *(read)* | n/a | Dewey suggestions for a prefix. |
| GET | `/api/v1/genres` | API key *(read)* | n/a | Genre vocabulary. |
| GET | `/api/v1/locations` | API key *(read)* | n/a | Storage-location tree. |
| GET | `/api/v1/series` | API key *(read)* | n/a | Series list. |
| GET | `/api/v1/titles` | API key *(read)* | n/a | Title list; paginated. |
| GET | `/api/v1/titles/{id}` | API key *(read)* | n/a | One title. |
| PATCH | `/api/v1/titles/{id}` | API key *(write)* | n/a | A read-only key gets 403 here. |

### Shelf audit (CR #237) (`src/routes/audit.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/audit` | Librarian | — |  |
| POST | `/audit/clear-all` | Librarian | no |  |
| POST | `/location/{id}/mark-audit` | Librarian | no |  |
| POST | `/volume/{id}/clear-audit` | Librarian | no |  |
| POST | `/volume/{id}/mark-audit` | Librarian | no |  |

### Auth and language (`src/routes/auth.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| POST | `/language` | Anonymous | no | Language toggle (story 7-3). Requires CSRF token (story 8-2 added hidden `_csrf_token` to the nav-bar form). Anonymous visitors carry a CSRF token via the lazy-anonymous session row. |
| GET | `/login` | Anonymous | — | Accepts `?next=` (Task 3). Redirects Librarian+ to `/catalog`. |
| POST | `/login` | Anonymous | **yes** | No authenticated session at request time; `SameSite=Lax` mitigates login-CSRF. Frozen in `CSRF_EXEMPT_ROUTES`. |
| POST | `/logout` | Anonymous | no | Requires CSRF token. The nav-bar logout is a POST form (story 8-2). |

### Borrowers (`src/routes/borrowers.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| DELETE | `/borrower/{id}` | Admin | no | **Smoke-test 403 target (AC #9).** Destructive; stays Admin (decision 2a exception). |
| GET | `/borrower/{id}` | Librarian | — | — |
| POST | `/borrower/{id}` | Librarian | no | decision 2a. |
| GET | `/borrower/{id}/delete-modal` | Admin | — |  |
| GET | `/borrower/{id}/edit` | Librarian | — | decision 2a. |
| GET | `/borrowers` | Librarian | — | — |
| POST | `/borrowers` | Librarian | no | — |
| GET | `/borrowers/search` | Librarian | — | — |
| GET | `/contributor/{id}/delete-modal` | Admin | — |  |
| GET | `/saved-searches/{id}/delete-modal` | Admin | — |  |
| GET | `/series/{id}/delete-modal` | Admin | — |  |

### Catalog and scanning (`src/routes/catalog.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/catalog` | Anonymous | — | AC #1. Scan-field + edit affordances template-gated. |
| POST | `/catalog/contributors/add` | Librarian | no | — |
| GET | `/catalog/contributors/form` | Librarian | — | — |
| POST | `/catalog/contributors/remove` | Librarian | no | — |
| GET | `/catalog/contributors/search` | Librarian | — | — |
| POST | `/catalog/contributors/update` | Librarian | no | — |
| DELETE | `/catalog/contributors/{id}` | Librarian | no | — |
| POST | `/catalog/scan` | Librarian | no | — |
| POST | `/catalog/scan-with-type` | Librarian | no | — |
| POST | `/catalog/title` | Librarian | no | — |
| GET | `/catalog/title/fields/{media_type}` | Librarian | — | HTMX fragment. |
| GET | `/catalog/title/new` | Librarian | — | — |
| DELETE | `/catalog/title/{id}` | Librarian | no | — |
| DELETE | `/catalog/volume/{id}` | Librarian | no | — |
| POST | `/debug/session-timeout` | Admin | no |  |
| POST | `/session/keepalive` | Admin | no | Ping; updates `last_activity`. Anonymous sessions noop. |
| DELETE | `/title/{id}` | Librarian | no |  |
| GET | `/title/{id}/contributor-form` | Librarian | — |  |
| DELETE | `/volume/{id}` | Librarian | no |  |
| GET | `/volume/{id}` | Anonymous | — | — |
| GET | `/volume/{id}/delete-modal` | Librarian | — |  |
| GET | `/volume/{id}/edit` | Librarian | — | — |
| POST | `/volume/{id}/update` | Librarian | no | — |

### Scan undo (#9) (`src/routes/catalog_undo.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| POST | `/catalog/undo` | Librarian | no |  |

### Contributors (`src/routes/contributors.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/contributor/{id}` | Anonymous | — | — |

### Applying labels to titles and volumes (#443) (`src/routes/entity_labels.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| POST | `/title/{id}/labels/attach` | Librarian | no |  |
| POST | `/title/{id}/labels/{label_id}/detach` | Librarian | no |  |
| POST | `/volume/{id}/labels/attach` | Librarian | no |  |
| POST | `/volume/{id}/labels/{label_id}/detach` | Librarian | no |  |

### Home and search (`src/routes/home.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/` | Anonymous | — | Homepage with search. Librarian-only metadata-error badge is template-gated. |

### Home scan field (`src/routes/home_scan.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/scan` | Anonymous | — |  |

### The /labels page (#443) (`src/routes/labels_page.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/labels` | Librarian | — |  |
| GET | `/labels/{id}` | Librarian | — |  |

### Loans (`src/routes/loans.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| POST | `/debug/seed-overdue-loan` | Admin | no |  |
| GET | `/loans` | Librarian | — | — |
| POST | `/loans` | Librarian | no | — |
| GET | `/loans/scan` | Librarian | — | — |
| POST | `/loans/{id}/return` | Librarian | no | — |
| GET | `/loans/{id}/return-modal` | Librarian | — |  |

### Storage locations (`src/routes/locations.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/location/{id}` | Anonymous | — | — |
| GET | `/locations` | Anonymous | — | AC #1 "location browse page". |
| POST | `/locations` | Librarian | no | decision 1a. |
| GET | `/locations/next-lcode` | Librarian | — | decision 1a. Used by create form. |
| DELETE | `/locations/{id}` | Admin | no | Destructive; stays Admin (decision 1a exception). Used as smoke-test 403 candidate. |
| POST | `/locations/{id}` | Librarian | no | decision 1a. |
| GET | `/locations/{id}/edit` | Librarian | — | decision 1a. |

### Saved searches (#367) (`src/routes/saved_searches.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| POST | `/saved-searches` | Librarian | no |  |
| POST | `/saved-searches/{id}/delete` | Librarian | no |  |
| POST | `/saved-searches/{id}/rename` | Librarian | no |  |
| GET | `/saved-searches/{id}/rename-modal` | Librarian | — |  |

### Series (`src/routes/series.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/series` | Anonymous | — | — |
| POST | `/series` | Librarian | no | — |
| GET | `/series/new` | Librarian | — | — |
| DELETE | `/series/{id}` | Librarian | no | — |
| GET | `/series/{id}` | Anonymous | — | — |
| POST | `/series/{id}` | Librarian | no | — |
| GET | `/series/{id}/edit` | Librarian | — | — |

### First-launch setup wizard (story 8-8) (`src/routes/setup.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/setup` | Anonymous | — | 404 once the wizard has completed — single-use. Reachable only while the setup gate is active (`active_admin_count == 0 AND setup_completed_at IS NULL`); the gate, not a role, is what protects it. |
| POST | `/setup/complete` | Anonymous | no | Writes `setup_completed_at`; the gate becomes a no-op forever after. |
| POST | `/setup/step-1` | Anonymous | no | Creates the first admin and authenticates the session. Protected by the setup gate, not by a role — there is no user to have one yet. |
| POST | `/setup/step-2` | Anonymous | no | Provider API keys. Setup gate. |
| POST | `/setup/step-3` | Anonymous | no | Preferences. Setup gate. |

### Valuation and statistics (`src/routes/stats.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/stats/value` | Librarian | — |  |

### Title lifecycle (`src/routes/title_lifecycle.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/title/{id}/delete-modal` | Librarian | — |  |

### Titles (`src/routes/titles.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/title/{id}` | Anonymous | — | — |
| POST | `/title/{id}` | Librarian | no | — |
| POST | `/title/{id}/confirm-metadata` | Librarian | no | — |
| POST | `/title/{id}/cover` | Librarian | no |  |
| GET | `/title/{id}/cover/upload-modal` | Librarian | — |  |
| GET | `/title/{id}/edit` | Librarian | — | — |
| GET | `/title/{id}/metadata` | Anonymous | — | Embeds Librarian-only edit/redownload buttons; template-gated. |
| POST | `/title/{id}/redownload` | Librarian | no | — |
| POST | `/title/{id}/series` | Librarian | no | — |
| POST | `/title/{id}/series-remove` | Librarian | no | — |
| POST | `/title/{id}/series/{assignment_id}/remove` | Librarian | no | — |

### Wishlist (`src/routes/wishlist.rs`)

| method | path | role | csrf | note |
|---|---|---|---|---|
| GET | `/wishlist` | Librarian | — |  |
| POST | `/wishlist` | Librarian | no |  |
| GET | `/wishlist/export.pdf` | Anonymous | — | **Anonymous by omission** — same as `/wishlist/print`. See the anomalies section. |
| GET | `/wishlist/new` | Librarian | — |  |
| POST | `/wishlist/preview-isbn` | Librarian | no |  |
| GET | `/wishlist/print` | Anonymous | — | **Anonymous by omission** — the handler takes no `Session`, so the printable wishlist is served to anyone who knows the URL, while `/wishlist` itself is Librarian. Not a deliberate policy; see the anomalies section. |
| DELETE | `/wishlist/{id}` | Librarian | no |  |
| GET | `/wishlist/{id}` | Librarian | — |  |
| GET | `/wishlist/{id}/delete-modal` | Librarian | — |  |


## Anomalies

Two rows above document behaviour that reads as an oversight rather than
a decision. They are recorded here rather than quietly fixed, because
changing them changes behaviour and that belongs in its own change:

- `GET /wishlist/print` and `GET /wishlist/export.pdf` take no `Session`
  and are therefore readable by anyone who knows the URL, while
  `GET /wishlist` requires Librarian. Standing policy 3 above says
  anonymous reach covers "catalog browsing and detail pages only"; a
  printable wish list is neither.

Anything else in this file that looks wrong is more likely to be the
file than the code — check the handler before trusting the row.
