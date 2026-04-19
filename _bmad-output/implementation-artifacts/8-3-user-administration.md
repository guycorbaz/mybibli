# Story 8.3: User administration

Status: ready-for-dev

Epic: 8 — Administration & Configuration
Requirements mapping: FR68 (admin user CRUD + role assignment), NFR13 (role isolation), NFR39 (25 items/page), AR9 (settings cache not applicable — user state is per-row, no cache), UX-DR7 (Users tab content), Foundation Rules #1–#7

---

> **TL;DR** — Fills the Users tab stub that story 8-1 left behind. Ships a paginated user list (25/page) with role + status filters, a non-modal **Create user** HTMX form, an inline **Edit user** form (pre-filled; blank password = keep hash), and two **destructive actions** (Deactivate / Reactivate) gated by soft-delete semantics (`users.deleted_at` — the existing schema column). Deactivation also soft-deletes every authenticated session row for that user so the kick-out is immediate (FR68 "deactivate"). **Self-deactivate guard** (409) and **last-active-admin guard** (409, on both deactivate AND role-demote) are enforced server-side in a single transaction per request. Password hashing reuses Argon2 from story 1-9 — zero new hashing code. Every mutation form carries the `_csrf_token` hidden input (enforced by `templates_audit::forms_include_csrf_token` from story 8-2). UX journey per `ux-design-specification.md` §Journey 7 with the terminology correction **"Delete" → "Deactivate"** (soft-delete, reversible via Reactivate) — the destructive-confirm on Deactivate uses `hx-confirm=` and adds **exactly one** entry to the 7-5 frozen allowlist (4 → 5) — explicit review signal, per the allowlist's design. Scope note: this story does NOT introduce a new Modal component (Epic 9 UX-DR8 owns that); the `hx-confirm=` browser native confirm is sufficient for v1 (see §Cross-cutting decisions for the 4→5 exemption rationale).

## Story

As an **admin**,
I want to create, edit, deactivate, and reactivate user accounts and assign roles (Librarian, Admin),
so that I control who can access mybibli and at what privilege level — without re-editing migrations or SQL by hand, and without ever being able to lock myself (or every admin) out of the system.

## Scope Reality & What This Story Ships

**The `users` table already exists** (`migrations/20260329000000_initial_schema.sql:208-220`) with every column this story needs:

```sql
CREATE TABLE users (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    username VARCHAR(255) NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    role ENUM('librarian', 'admin') NOT NULL DEFAULT 'librarian',
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    UNIQUE KEY uq_users_username (username),
    INDEX idx_users_deleted_at (deleted_at)
);
```

Plus `preferred_language ENUM('fr', 'en') NULL` added by `migrations/20260416000001_add_users_preferred_language.sql` (story 7-3). **No schema migration is needed** — this story is CRUD on the existing table.

**The `active` boolean is already load-bearing in the login guard** (`src/routes/auth.rs:153-154`): `SELECT ... FROM users WHERE username = ? AND active = TRUE AND deleted_at IS NULL`. That means setting `deleted_at = NOW()` alone is sufficient to block login — the compound AND already fails. Per the epic AC wording we use **`deleted_at` as the canonical deactivation signal** and leave `active` at its schema default (`TRUE`) for new users. Do NOT invent a second code path that toggles `active`: (a) it would create two equivalent states and drift; (b) the `deleted_at IS NULL` predicate is the project's universal soft-delete convention per CLAUDE.md. The `active` flag stays in the schema as belt-and-braces (future hard-admin-disable without losing history, if ever needed) — not this story's concern.

**There is no `src/models/user.rs` yet** — user queries today live inline in `src/routes/auth.rs::login` (`SELECT ...`), `src/routes/auth.rs::change_language` (`UPDATE users.preferred_language`), and `src/routes/auth.rs::language_tests` (INSERTs). This story creates `src/models/user.rs` with the CRUD surface, moves/extends the existing queries there (pulling `login`'s SELECT over requires care — keep its exact shape or add a thin wrapper so the login flow stays unchanged), and wires `src/models/mod.rs`.

**The Users tab panel is already wired but stubbed** (`src/routes/admin.rs::admin_users_panel` at lines 230-239 + `templates/fragments/admin_users_panel.html` — 5-line placeholder). This story **replaces** the stub body with the real list/form content while keeping the admin-tab plumbing from 8-1 untouched (tab resolution, `require_role_with_return(Role::Admin, …)`, HTMX fragment vs full-page rendering). Also update the panel template's stale comment (`{# …Replaced by story 8-2 #}` → `{# …Replaced by story 8-3 #}`) — bookkeeping correction from the Epic 8 CSRF-insertion renumber.

**CSRF is in place** (story 8-2 shipped in-progress; middleware + `<meta name="csrf-token">` + template-audit enforcement are all live). Every `<form method="POST">` in this story MUST include the `<input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">` as the **first child** (enforced by `src/templates_audit.rs::forms_include_csrf_token`). Every new page-template struct that `{% extends "layouts/base.html" %}` MUST carry a `csrf_token: String` field populated from `session.csrf_token`. **The `base_context()` DRY helper from 8-2 review was deferred** (still on the open-findings list — a dedicated refactor story) — for this story, flatten the common fields manually, matching the existing pattern in 8-1's `admin.rs` (see `render_admin` / `render_panel` flow). Do NOT attempt the `base_context()` refactor here.

**Ships:**

1. **`src/models/user.rs`** (new). Public surface:
   - `pub struct UserRow { pub id: u64, pub username: String, pub role: String, pub preferred_language: Option<String>, pub created_at: chrono::DateTime<chrono::Utc>, pub deleted_at: Option<chrono::DateTime<chrono::Utc>>, pub version: i32, pub last_login: Option<chrono::DateTime<chrono::Utc>> }` — the list-row DTO. `last_login` is computed from the latest `sessions.created_at` for that `user_id` where `deleted_at IS NULL OR deleted_at IS NOT NULL` (we want the most recent login regardless of current session liveness) — see §Dev Notes "Last login query" for the exact SQL and why we do NOT add a `users.last_login` denormalized column.
   - `pub async fn list_page(pool, filter_role, filter_status, offset, limit) -> Result<Vec<UserRow>, AppError>` — paginated, sorted by `username ASC`. `filter_status: UserStatus` = `Active | Deactivated | All`. `filter_role: Option<Role>` = `Some(Librarian) | Some(Admin) | None`. Limit is always 25 (NFR39); caller passes `offset = (page - 1) * 25`.
   - `pub async fn count_all(pool, filter_role, filter_status) -> Result<i64, AppError>` — for pagination totals.
   - `pub async fn find_by_id(pool, id) -> Result<Option<UserRow>, AppError>` — for edit-form pre-fill. Returns including deactivated rows (the Edit form can edit a deactivated user's role/username too, though the UX only surfaces it via "Show deactivated" filter).
   - `pub async fn find_by_username(pool, username) -> Result<Option<UserRow>, AppError>` — for uniqueness check.
   - `pub async fn create(pool, username, password_hash, role) -> Result<u64, AppError>` — INSERT. Returns new id. **Unique-constraint violation (error code 1062 / SQLSTATE 23000) is mapped to `AppError::Conflict("username_taken")`** — caller's responsibility to render the localized message.
   - `pub async fn update(pool, id, version, new_username, new_role, new_password_hash: Option<String>) -> Result<(), AppError>` — optimistic-locking UPDATE. Password is `None` = don't change the `password_hash` column; `Some(hash)` = update it. Uses `services::locking::check_update_result` exactly like other entity updates. Username change re-checks uniqueness via the same 1062 mapping.
   - `pub async fn deactivate(pool, id, version, acting_admin_id) -> Result<(), AppError>` — runs in a **single DB transaction**:
     (a) SELECT the target user FOR UPDATE (row-lock so the last-admin guard is race-safe across concurrent admin sessions).
     (b) Guard: `if acting_admin_id == id → return AppError::Conflict("self_deactivate_blocked")`.
     (c) Guard: if target is admin AND `SELECT COUNT(*) FROM users WHERE role='admin' AND deleted_at IS NULL AND id != ?` (same transaction) == 0 → `AppError::Conflict("last_admin_blocked")`.
     (d) `UPDATE users SET deleted_at = NOW(), version = version + 1 WHERE id = ? AND version = ? AND deleted_at IS NULL`. `check_update_result` on affected rows → `Conflict("version_mismatch")` if stale.
     (e) `UPDATE sessions SET deleted_at = NOW() WHERE user_id = ? AND deleted_at IS NULL` — invalidates every live session for that user immediately. Return count for tracing.
     (f) Commit. On any error before COMMIT, the transaction rolls back and no partial state survives (in particular, we never end up with `users.deleted_at` set but sessions still alive, which would leave the attacker's session usable for `session_timeout_secs` minutes).
   - `pub async fn reactivate(pool, id, version) -> Result<(), AppError>` — `UPDATE users SET deleted_at = NULL, version = version + 1 WHERE id = ? AND version = ? AND deleted_at IS NOT NULL`. No session reinstatement (users just log back in afresh).
   - `pub async fn demote_guard(pool, target_id, new_role, acting_admin_id) -> Result<(), AppError>` — pre-check before `update()` when the role is changing. If `target_id == acting_admin_id AND new_role != Admin AND count_other_active_admins() == 0` → `Conflict("last_admin_demote_blocked")`. Encoded as a separate function rather than woven into `update()` so the unit-test surface is explicit and the error message is distinguishable from last-admin-deactivate.

2. **`src/services/password.rs`** (new, thin). Extract the Argon2 hashing used by `src/routes/auth.rs` tests (lines 494-509) into one reusable function:
   ```rust
   pub fn hash_password(plain: &str) -> Result<String, AppError> {
       use argon2::{Argon2, PasswordHasher};
       use argon2::password_hash::{SaltString, rand_core::OsRng};
       let salt = SaltString::generate(&mut OsRng);
       Argon2::default()
           .hash_password(plain.as_bytes(), &salt)
           .map(|h| h.to_string())
           .map_err(|e| AppError::Internal(format!("argon2 hash failed: {e}")))
   }
   ```
   Also move the existing `fn verify_password` from `src/routes/auth.rs:438-447` here (as `pub fn verify_password`) and update the single call site in `login`. **DRY per Foundation Rule #1** — hashing must live in exactly one place so future stories (setup wizard 8-8, password-reset features) reuse it. The tests in `routes/auth.rs` that currently inline-hash for fixtures should import this helper too (drop the `use argon2::...` imports from those test functions).

3. **`src/routes/admin.rs` — new handlers** (all are `async fn`, all start with `session.require_role_with_return(Role::Admin, &return_path)?`; Anonymous → 303, Librarian → 403; return type is `Result<Response, AppError>` consistent with the existing admin handlers):
   - `admin_users_panel(State, Session, HxRequest, OriginalUri, Query<UsersQuery>) -> Result<Response, AppError>` — **replaces the current 8-line stub** in admin.rs. Reads `UsersQuery { role: Option<String>, status: Option<String>, page: Option<u32> }`. Renders either the panel fragment (HTMX) or the full `/admin` page with the Users tab pre-selected (direct nav), matching 8-1's `render_panel` idiom.
   - `admin_users_create_form(State, Session) -> Result<Response, AppError>` — GET `/admin/users/new` — returns a fragment that HTMX swaps into the panel's "form slot". The form is always visible at the top of the panel when active; this handler lets the "New user" button toggle between "show form" and "hide form" for keyboard rhythm (a page-load with a query flag would also work — pick whichever matches the existing 8-1 HTMX idiom; prefer the latter if it's simpler).
   - `admin_users_create(State, Session, Form<CreateUserForm>) -> Result<Response, AppError>` — POST `/admin/users` — reads `username`, `password`, `role`, `_csrf_token` (CSRF handled by middleware, but the field is deserialized and ignored here). Validates:
     - `username` non-empty, ≤ 255 chars, trimmed (leading/trailing whitespace rejected so unique-index hits can't be worked around).
     - `password` ≥ 8 chars. **Upper bound 72 bytes** — Argon2 accepts longer, but we cap at 72 bytes to match bcrypt-family expectations in case we ever switch; error message is `error.user.password_too_long`. `8..=72` is the validated range.
     - `role` ∈ `{ "librarian", "admin" }`. Invalid → 400.
     - On validation failure: return 400 with a `feedback_html_pub("error", title, message)` body, rendered into `#feedback-list` via `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` (same coordination headers the CSRF middleware uses — reuse the pattern).
     - On success: hash password via `services::password::hash_password`, call `UserModel::create`, return an HTMX fragment that (a) re-renders the Users tab row list (the new user appears, sorted correctly) AND (b) emits a success FeedbackEntry via OOB swap into `#feedback-list` ("User %{username} created"). Use `HtmxResponse { main, oob }` — pattern already in `src/middleware/htmx.rs`.
   - `admin_users_edit_form(State, Session, Path<u64>) -> Result<Response, AppError>` — GET `/admin/users/{id}/edit` — returns the edit-row fragment with pre-filled `username` + `role` select. Password field is blank (placeholder `Leave blank to keep current` / FR: `Laisser vide pour conserver l'actuel`). 404 if user id does not exist (including deactivated). Includes `version` as a hidden input for optimistic locking.
   - `admin_users_update(State, Session, Path<u64>, Form<UpdateUserForm>) -> Result<Response, AppError>` — POST `/admin/users/{id}` (or PUT if the existing routing style uses PUT — check `src/routes/mod.rs`; POST-with-method-override is NOT used in this codebase, prefer plain POST for consistency with the other routes). Reads `username`, `role`, `password` (optional, empty = keep hash), `version`, `_csrf_token`. Calls `UserModel::demote_guard` if role is changing, then `UserModel::update`. Same validation as create for username/password when provided. Returns the updated row fragment + success FeedbackEntry.
   - `admin_users_deactivate(State, Session, Path<u64>, Form<DeactivateForm>) -> Result<Response, AppError>` — POST `/admin/users/{id}/deactivate`. `DeactivateForm { version: i32, _csrf_token: String }`. Calls `UserModel::deactivate(pool, id, version, session.user_id.unwrap_or(0))`. On `Conflict("self_deactivate_blocked")` / `Conflict("last_admin_blocked")` / `Conflict("version_mismatch")`: return the localized error via FeedbackEntry. On success: return an HTMX fragment that removes the row (or greys it out if the "Show deactivated" filter is active) and emits a success FeedbackEntry ("User %{username} deactivated"). Session invalidation is a silent background effect — the acting admin sees only their own success feedback; the deactivated user's next page load lands them on `/login`.
   - `admin_users_reactivate(State, Session, Path<u64>, Form<ReactivateForm>) -> Result<Response, AppError>` — POST `/admin/users/{id}/reactivate`. `ReactivateForm { version: i32, _csrf_token: String }`. Calls `UserModel::reactivate`. Success → restore row to the active list.

4. **Query struct and form structs** (in `src/routes/admin.rs`, next to the existing `AdminQuery`):
   ```rust
   #[derive(Deserialize, Default)]
   pub struct UsersQuery {
       pub role: Option<String>,     // "librarian" | "admin" | "" (all)
       pub status: Option<String>,   // "active" | "deactivated" | "all" (default "active")
       pub page: Option<u32>,        // 1-based, clamp to 1 if 0 or missing
   }

   #[derive(Deserialize)]
   pub struct CreateUserForm {
       pub username: String,
       pub password: String,
       pub role: String,              // "librarian" | "admin"
       pub _csrf_token: String,       // validated by middleware, field ignored here
   }

   #[derive(Deserialize)]
   pub struct UpdateUserForm {
       pub username: String,
       pub role: String,
       pub password: String,          // empty = keep hash
       pub version: i32,
       pub _csrf_token: String,
   }

   #[derive(Deserialize)]
   pub struct DeactivateForm { pub version: i32, pub _csrf_token: String }

   #[derive(Deserialize)]
   pub struct ReactivateForm { pub version: i32, pub _csrf_token: String }
   ```

5. **Route mounting** — `src/routes/mod.rs`. Add (inside the same block that mounts `/admin` and `/admin/{tab}` from story 8-1):
   ```rust
   .route("/admin/users", get(admin::admin_users_panel).post(admin::admin_users_create))
   .route("/admin/users/new", get(admin::admin_users_create_form))
   .route("/admin/users/{id}", post(admin::admin_users_update))
   .route("/admin/users/{id}/edit", get(admin::admin_users_edit_form))
   .route("/admin/users/{id}/deactivate", post(admin::admin_users_deactivate))
   .route("/admin/users/{id}/reactivate", post(admin::admin_users_reactivate))
   ```
   All four new POST routes are automatically CSRF-protected by the 8-2 middleware (the exempt allowlist is frozen at `POST /login` only; adding routes here does NOT require updating `CSRF_EXEMPT_ROUTES`). Confirm `docs/route-role-matrix.md` gets 6 new rows under Admin role after this story.

6. **Templates** — new and modified:
   - **`templates/fragments/admin_users_panel.html`** (REPLACE the existing stub). Structure:
     ```
     <section id="admin-users-panel" aria-labelledby="admin-users-heading">
       <h2 id="admin-users-heading" class="sr-only">{{ heading }}</h2>
       <div class="mb-4 flex items-center gap-4">
         <!-- Filter: role + status (GET form with hx-push-url=/admin?tab=users&role=…&status=…) -->
       </div>
       <div id="admin-users-form-slot" aria-live="polite">
         {# Empty by default; filled by /admin/users/new fragment or by validation errors #}
       </div>
       <div id="admin-users-list">
         {% include "fragments/admin_users_table.html" %}
       </div>
       <nav id="admin-users-pagination" aria-label="{{ pagination_aria }}">
         {# Prev / Page X of Y / Next — 25/page #}
       </nav>
     </section>
     ```
     Empty-state copy (when filtered list returns zero rows AND the filter is "active", "all roles"): render `admin.users.empty_state` i18n key — `"Only you here! Create a Librarian account for household members."` / FR `"Vous êtes seul·e ! Créez un compte Bibliothécaire pour les membres du foyer."` (from UX §Journey 7).
   - **`templates/fragments/admin_users_table.html`** (new). Renders the table + rows. Columns: username, role (localized), status badge (Active / Deactivated), created date (short-date i18n format), last login (short-datetime or "Never"), actions. Actions per row depend on status:
     - Active user, id != current admin: `[Edit] [Deactivate]` (the Deactivate button is a POST form with `_csrf_token` + `version`; `hx-post="/admin/users/{id}/deactivate"`, `hx-confirm="..."` **requires a new allowlist entry** — see §Cross-cutting decisions).
     - Active user, id == current admin: `[Edit]` only (no Deactivate button — belt to the server-side self-deactivate guard).
     - Deactivated user: `[Edit] [Reactivate]`.
   - **`templates/fragments/admin_users_row.html`** (new). Single row — used both in the initial render and as the HTMX OOB swap target when a row mutates. Includes `id="admin-users-row-{{ user.id }}"` for targeting.
   - **`templates/fragments/admin_users_form_create.html`** (new). Inline form — not a modal. Fields: username, password (single input — `type="password"`, `minlength="8"` for HTML-level pre-validation), role select (Librarian default), Submit / Cancel. The Cancel button is `hx-get="/admin/users" hx-target="#admin-users-form-slot" hx-swap="innerHTML"` to clear the slot.
   - **`templates/fragments/admin_users_form_edit.html`** (new). Pre-filled row-inline edit form — replaces the row with the form via `hx-target="#admin-users-row-{{ user.id }}" hx-swap="outerHTML"`. Fields: username (pre-filled), role (pre-selected), password (blank), hidden `version`, Submit / Cancel. Cancel HX-gets the read-only row fragment.
   - **No new modal template.** The deactivate confirmation uses `hx-confirm=` per the existing pattern; see §Cross-cutting decisions for the allowlist exemption requirement. The Reactivate action is non-destructive and skips `hx-confirm=`.

7. **`src/routes/admin.rs` — template structs** — define an `AdminUsersPanelTemplate` struct (extends `layouts/base.html` for direct-navigation path, uses `admin_users_panel.html` for HTMX path) with flattened common fields (no `base_context()` helper yet — match the existing 8-1 style):
   - `lang`, `role`, `current_page: "admin"`, `skip_label`, `nav_*` labels, `csrf_token`, `admin_tabs_*`, `trash_count` (re-use 8-1's Trash-badge-count helper — do NOT re-implement).
   - `heading`, `pagination_aria`, `empty_state` — new i18n-backed fields.
   - `users: Vec<UserRow>`, `filter_role`, `filter_status`, `page`, `total_pages`, `acting_admin_id: u64`.

8. **i18n keys — EN + FR** in `locales/en.yml` and `locales/fr.yml`, under the `admin.users.*` namespace (reuse the `admin:` top-level already established by 8-1):
   ```yaml
   admin:
     users:
       heading: User accounts                         # FR: "Comptes utilisateurs"
       empty_state: "Only you here! Create a Librarian account for household members."
                                                       # FR: "Vous êtes seul·e ! Créez un compte Bibliothécaire pour les membres du foyer."
       pagination_aria: User list pagination          # FR: "Pagination de la liste des utilisateurs"
       filter_role_label: Filter by role              # FR: "Filtrer par rôle"
       filter_status_label: Filter by status          # FR: "Filtrer par statut"
       filter_role_all: All roles                     # FR: "Tous les rôles"
       filter_status_active: Active                   # FR: "Actifs"
       filter_status_deactivated: Deactivated         # FR: "Désactivés"
       filter_status_all: All                         # FR: "Tous"
       col_username: Username                         # FR: "Nom d'utilisateur"
       col_role: Role                                 # FR: "Rôle"
       col_status: Status                             # FR: "Statut"
       col_created: Created                           # FR: "Créé le"
       col_last_login: Last login                     # FR: "Dernière connexion"
       col_actions: Actions                           # FR: "Actions"
       role_librarian: Librarian                      # FR: "Bibliothécaire"
       role_admin: Admin                              # FR: "Admin"
       status_active: Active                          # FR: "Actif"
       status_deactivated: Deactivated                # FR: "Désactivé"
       last_login_never: Never                        # FR: "Jamais"
       btn_new: New user                              # FR: "Nouvel utilisateur"
       btn_edit: Edit                                 # FR: "Modifier"
       btn_deactivate: Deactivate                     # FR: "Désactiver"
       btn_reactivate: Reactivate                     # FR: "Réactiver"
       btn_save: Save                                 # FR: "Enregistrer"
       btn_cancel: Cancel                             # FR: "Annuler"
       form_label_username: Username                  # FR: "Nom d'utilisateur"
       form_label_password: Password                  # FR: "Mot de passe"
       form_label_password_edit: "Password (leave blank to keep current)"
                                                       # FR: "Mot de passe (laisser vide pour conserver l'actuel)"
       form_label_role: Role                          # FR: "Rôle"
       confirm_deactivate: "Deactivate %{username}? They'll be signed out immediately and cannot log back in until reactivated."
                                                       # FR: "Désactiver %{username} ? La personne sera déconnectée immédiatement et ne pourra plus se connecter jusqu'à réactivation."
       success_created: "User %{username} created"    # FR: "Utilisateur %{username} créé"
       success_updated: "User %{username} updated"    # FR: "Utilisateur %{username} mis à jour"
       success_deactivated: "User %{username} deactivated (%{count} session(s) ended)"
                                                       # FR: "Utilisateur %{username} désactivé (%{count} session(s) terminée(s))"
       success_reactivated: "User %{username} reactivated"
                                                       # FR: "Utilisateur %{username} réactivé"
       error_username_taken: "Username '%{username}' is already taken"
                                                       # FR: "Le nom d'utilisateur « %{username} » est déjà pris"
       error_username_empty: Username is required     # FR: "Le nom d'utilisateur est requis"
       error_password_too_short: Password must be at least 8 characters
                                                       # FR: "Le mot de passe doit contenir au moins 8 caractères"
       error_password_too_long: Password must be at most 72 characters
                                                       # FR: "Le mot de passe ne peut dépasser 72 caractères"
       error_role_invalid: Role must be Librarian or Admin
                                                       # FR: "Le rôle doit être Bibliothécaire ou Admin"
       error_self_deactivate: You cannot deactivate your own account
                                                       # FR: "Vous ne pouvez pas désactiver votre propre compte"
       error_last_admin: At least one active admin must remain
                                                       # FR: "Au moins un administrateur actif doit rester"
       error_last_admin_demote: "You are the only active admin — create another before changing your role"
                                                       # FR: "Vous êtes le seul administrateur actif — créez-en un autre avant de changer votre rôle"
       error_not_found: User not found                # FR: "Utilisateur introuvable"
       error_version_mismatch: "User was modified by another admin — reload and retry"
                                                       # FR: "L'utilisateur a été modifié par un autre administrateur — rechargez et réessayez"
   ```
   Also ensure the `admin.users.coming_in_story` placeholder from 8-1 is **removed** from both YAML files (the stub no longer renders). Post-edit: `touch src/lib.rs && cargo build` (per CLAUDE.md i18n rule).

9. **Unit tests** (`#[cfg(test)]` module in `src/models/user.rs` + `src/routes/admin.rs` where relevant). Each test runs via `#[sqlx::test(migrations = "./migrations")]`:
   - `create_then_find_by_id`: INSERT + SELECT round-trip.
   - `create_enforces_unique_username`: two INSERTs with the same username → second returns `Conflict("username_taken")`.
   - `create_unique_is_case_sensitive_per_existing_schema`: INSERT `Alice` + INSERT `alice` — schema uses the default collation (`utf8mb4_unicode_ci` per migration default, which IS case-insensitive — **verify**; if it is case-insensitive, both inserts collide and the test asserts that; if case-sensitive, document and assert the opposite). This is a fact-finding test; the behavior is documented, not decided here. Reference: `migrations/20260329000000_initial_schema.sql` top (collation declaration).
   - `update_applies_optimistic_locking`: simulate a stale version → `Conflict("version_mismatch")`.
   - `update_empty_password_keeps_hash`: update without password param → `password_hash` column is unchanged on re-SELECT.
   - `update_role_demote_self_when_sole_admin_is_blocked`: seed the only admin, try to demote via `demote_guard` → `Conflict("last_admin_demote_blocked")`. Then seed a second admin, retry → OK.
   - `deactivate_self_is_blocked`: seed admin A, call `deactivate(A, version, A)` → `Conflict("self_deactivate_blocked")`.
   - `deactivate_last_admin_is_blocked`: seed admin A (only active admin), attempt to deactivate A via a **different** acting-admin id (only possible in tests — simulate via a seeded second admin who is then about-to-be-deactivated-first in the same test; OR use a fabricated `acting_admin_id` that exists but is the admin being acted upon) → `Conflict("last_admin_blocked")`.
   - `deactivate_non_last_admin_succeeds_and_invalidates_sessions`: seed two admins + 3 sessions for admin B (all `deleted_at IS NULL`), call `deactivate(B, v, A)` → success, `SELECT COUNT(*) FROM sessions WHERE user_id = B AND deleted_at IS NULL` == 0.
   - `reactivate_round_trip`: deactivate then reactivate → `deleted_at IS NULL` AND `version` bumped twice.
   - `list_page_pagination_and_sort`: seed 27 users, list `page=1` → 25 rows sorted username ASC; `page=2` → 2 rows; `count_all` → 27.
   - `list_page_filter_role_and_status`: seed mixed roles and active/deactivated → filter combinations return the expected subsets.
   - `last_login_computed_correctly`: seed user with 3 session rows, varying `created_at` → `UserRow.last_login` matches the most recent `created_at`.
   - `deactivate_is_transactional_rollback_on_session_delete_failure`: **synthetic failure path** — if implementable without brittle mocking, simulate a constraint failure during the sessions UPDATE and assert the outer UPDATE on users also rolled back. If not implementable cheaply, document the transaction shape in the test file's docstring as "behavior-not-directly-tested; covered by integration/manual review" — do NOT fake the test green.
   - `route_librarian_gets_403_on_users_routes`: wire a mini-router with seeded librarian session, hit each of the 6 new routes → 403 on all. Anonymous → 303 to `/login`.
   - `route_create_requires_min_8_password`: hit POST `/admin/users` with a 7-char password → 400 + `error_password_too_short` feedback.

10. **E2E test** — `tests/e2e/specs/admin/users.spec.ts`, spec ID **`"UA"`** (unique per `specIsbn()` convention; check no existing spec uses `UA`). Coverage mirrors the epic AC E2E plus the empty-state:
    - **Smoke path (Foundation Rule #7 for this story):** blank browser → `loginAs(page, "admin")` → navigate `/admin?tab=users` → assert 25/page list is visible (or empty-state if fresh) → click "New user" → fill form with a unique test username (`UA-librarian-${Date.now()}`) + password "test1234!" + role Librarian → submit → assert success FeedbackEntry + new row present → log out → `loginAs(page)` as the new user via `page.fill("#username", …)` + `page.fill("#password", "test1234!")` (loginAs helper accepts role-only; use raw `page.goto('/login')` + fill for the ad-hoc user) → assert librarian can reach `/catalog` but NOT `/admin` (403).
    - **Self-deactivate guard:** as admin `admin`, navigate `/admin?tab=users` → assert the admin row has NO Deactivate button (`await expect(page.locator('[data-user-id="admin"] [data-action="deactivate"]')).toHaveCount(0);`). As a server-side belt-and-braces check, `page.request.post("/admin/users/{admin_id}/deactivate", { form: { version, _csrf_token } })` → assert 409 response with the localized `error.user.self_deactivate` text.
    - **Last-admin guard (deactivate):** seed context: one admin only. Create a second admin via the UI. Deactivate the second admin (succeeds). Try to deactivate the first admin → 409 `error.last_admin`. Reactivate the second admin. Try again: still 409 on the first admin if still the only OTHER active admin after reactivation? Re-read the guard: the guard checks "count of active admins OTHER than the target == 0"; after reactivation there IS another admin, so the original deactivate should now succeed — but since we seeded only one `admin` and the test must leave the DB in a known state for parallel runs (spec ID scoping), prefer to roll back by NOT actually deactivating the seeded `admin` in the final assertion (hit the API path, assert 409 or 303, then just verify the 409-path).
    - **Last-admin demote guard:** as sole-admin, try to change own role to Librarian via the Edit form → assert 409 `error.last_admin_demote`. Then create a second admin, repeat → assert 303 / success.
    - **Deactivate invalidates sessions:** admin creates a librarian Z. Log in as Z in **the same browser context** (requires `browserContext.newPage()` to hold Z's session separate from admin's). As admin (page A), deactivate Z. In Z's page (page B), trigger any navigation or HTMX action → assert 303 to `/login` (or a 401/feedback that implies session was killed). This is the load-bearing "immediate kick-out" E2E.
    - **Reactivate path:** continuing from above, admin reactivates Z. Z navigates to `/login`, submits credentials → success → `/catalog`. Asserts "Reactivated users can log back in" (epic AC).
    - **CSRF coverage:** admin navigates to Users tab, tampers the meta CSRF token via `page.evaluate`, attempts to submit Create → assert 403 + the 8-2 "Session expired" FeedbackEntry text. (This is a belt to 8-2's smoke; if adding here is deemed duplicative with `csrf.spec.ts`, drop it — but including costs nothing.)
    - **Empty-state:** (may be skippable if the spec DB seeder doesn't cleanly produce zero-user state; optional assertion) — if the "active/all-roles" filter returns zero rows, the empty-state copy renders.
    - All assertions use i18n-aware regex `toContainText(/User .* created|Utilisateur .* créé/i)` per CLAUDE.md.
    - Parallel-safety: every user created uses a timestamp suffix; no shared usernames across test parallelism. The `UA` spec ID prevents ISBN collisions (there are no scans here, but keep the convention for consistency).

11. **Documentation updates:**
    - `CLAUDE.md` "Key Patterns" section: append a bullet under Session/auth covering the user-admin surface, explicitly noting: (a) the deactivation semantics (`users.deleted_at IS NOT NULL` = logged-out and login-blocked); (b) `users.active` is vestigial — new rows default `TRUE` and no code path toggles it; (c) the invariant that deactivation soft-deletes user sessions atomically in the same transaction. Short bullet — the Key Patterns section is not a reference manual.
    - `docs/route-role-matrix.md`: add six rows for the new routes, all at Admin role, none CSRF-exempt.
    - `_bmad-output/planning-artifacts/architecture.md` Authentication & Security subsection: add a short paragraph on the user-admin lifecycle (deactivation cascades to sessions in-transaction) so the architecture doc reflects this story's shape for future reference. Don't touch the CSP/SameSite paragraphs from 8-2.

**Does NOT ship:**

- **UX-DR8 Modal component.** Epic 9 owns that. This story uses the existing `hx-confirm=` pattern for the one destructive prompt (Deactivate). See §Cross-cutting decisions for the frozen-allowlist exemption.
- **Password-twice-to-confirm input on Create form.** UX §Journey 7 mermaid shows two password inputs; epic AC does not require it and the `error.user.password_too_short` flow plus "set and tell the user out-of-band" (per UX §1131) covers the mis-type case. Ship a single password field — revisit if Guy asks.
- **User display name / full name column.** Epic AC mentions "optional full name" on Create. Schema has NO `full_name` / `display_name` column today. Adding one is a schema migration AND would ripple into every template that shows who's logged in (login flow, nav-bar "Hello, X" if any, retrospective comments, etc.). Out of scope — if Guy confirms he wants it, land in a follow-up story that's scoped around the column addition + all templates, not bolted onto CRUD. **This is the biggest scope deviation from the epic AC; flag it at hand-off for the dev agent to confirm.**
- **"Has created loans" delete guard.** UX §Journey 7 shows this. Epic AC does not require it. Since this story soft-deletes (deactivates) rather than hard-deletes, loan FKs are preserved — no referential integrity issue. Skip.
- **Password-reset-link / email flow.** UX §Journey 7: "admin sets new password; user must be informed out-of-band". No email infra. Already the behavior built into Edit.
- **Bulk deactivate / import / CSV.** Not in epic.
- **Activity log (who deactivated whom when).** Epic 8-7 introduces `admin_audit` for permanent-delete; user-admin actions deliberately do NOT audit here. If flagged later, extend `admin_audit` in 8-7 or a follow-up.
- **Arrow-key accessibility on the Users tab table.** Epic 9 accessibility polish.

## Cross-cutting decisions this story depends on

**`hx-confirm=` allowlist — one new entry required.** Story 7-5 froze the allowlist at 4 grandfathered sites (`ALLOWED_HX_CONFIRM_SITES` in `src/templates_audit.rs`; confirmed by CLAUDE.md-vs-code reconciliation in 8-2). CLAUDE.md documents: *"New `hx-confirm=` attributes are BLOCKED by `src/templates_audit.rs::hx_confirm_matches_allowlist`; the allowlist is frozen at 4 grandfathered sites and only changes through explicit review."*

This story's Deactivate button is a legitimate destructive-confirm. Two options:

1. **Extend the allowlist.** Add `"admin/users/deactivate"` (or whatever site identifier the existing allowlist uses) — explicit constant edit + review signal. This IS what the "explicit review" door is for. Mark clearly in the commit message: *"allowlist: 4 → 5 for admin users deactivate per 8-3"*.
2. **Inline FeedbackEntry + "Confirm" second button pattern.** First click on Deactivate posts to a `/admin/users/{id}/confirm-deactivate` handler that returns an OOB FeedbackEntry containing a second "Confirm" button. Second click actually deactivates. Avoids `hx-confirm=` entirely but adds a round-trip and a new handler.

**Default: option 1 (extend allowlist to 5)**. Deactivate has the exact semantics `hx-confirm=` was designed for; inventing a double-tap pattern just to preserve a count invariant trades clarity for ceremony. The allowlist existence was designed for "gated additions," not "zero additions ever." The dev agent should (a) add the one entry, (b) update the audit assertion from `len == 4` → `len == 5`, (c) update CLAUDE.md's "frozen at 4" bullet to "frozen at 5", (d) note in commit that this is the intended review signal firing. If Guy raises a concern in review, fall back to option 2 — but do not pre-emptively over-engineer.

**`forms_include_csrf_token` audit — every new form covered.** The 8-2 audit walks `templates/` for `<form method="POST">` and asserts the next input is `_csrf_token`. Each of the 4 forms in this story (Create, Edit, Deactivate, Reactivate) is a POST form → each MUST carry the hidden input as its first child. The audit will fail `cargo test` if a single one is missed. This is the primary catch-all guarding against human error in template authoring.

**No `base_context()` helper refactor here.** Story 8-2 code review flagged it but deferred the refactor. Doing the refactor inside 8-3 would triple the blast radius (touch every full-page struct in the app). Stay aligned with 8-1's pattern: each template struct declares its own `csrf_token` field and each handler passes `session.csrf_token.clone()` in. If the refactor lands between 8-3 create-story and 8-3 dev-story, adapt; otherwise flatten manually.

**Last-admin race safety.** The `FOR UPDATE` row-lock in `UserModel::deactivate` is the only thing standing between two concurrent admin sessions each seeing "one other admin exists" and both deactivating the "other" simultaneously (ending up with zero admins). On MariaDB with the default `InnoDB` engine and transaction isolation (likely REPEATABLE READ per the Docker image), `SELECT ... FOR UPDATE` inside the same transaction that does the `COUNT(*)` and the `UPDATE` is the canonical safe pattern. If the codebase's connection pool ever moves to a DB without row-lock support or to a strict serializable mode, revisit — but for MariaDB today, this is the standard move. Same logic applies to `demote_guard` — wrap in a transaction with `FOR UPDATE` on the target row.

**Soft-delete whitelist (`services::soft_delete::ALLOWED_TABLES`) is NOT extended.** The whitelist enumerates tables whose soft-deletes surface in the future Trash view (story 8-6). `users` deliberately does NOT appear there — deactivated users are restored via a dedicated "Reactivate" button on the Users tab, not via the Trash (you don't want to see a list of your deactivated teammates next to deleted books). If 8-6 later decides to include users, that's 8-6's call. For 8-3, keep the whitelist untouched.

## Acceptance Criteria

1. **Paginated list (25/page, sortable, filterable) renders in the Users tab.**
   - Navigating to `/admin?tab=users` as an admin renders the full admin page with the Users tab pre-selected and the list server-side rendered (non-HTMX path). As librarian → 403. As anonymous → 303 → `/login?next=%2Fadmin%3Ftab%3Dusers`.
   - HTMX click on the Users tab (from another admin tab) GETs `/admin/users` and swaps only the panel fragment into `#admin-shell`, with `hx-push-url="/admin?tab=users"` keeping the URL canonical.
   - List columns: Username (bold, username-ASCII sort), Role (localized), Status (Active / Deactivated badge), Created (short-date i18n), Last login (short-datetime i18n or "Never"), Actions.
   - Sort: Username ascending by default. (Sortable column headers are an Epic 9 polish; this story ships the default only.)
   - Filters:
     - **Role**: All roles / Librarian / Admin — GET-form, updates the URL via `hx-push-url="/admin?tab=users&role=<value>"`.
     - **Status**: Active (default) / Deactivated / All — same pattern.
   - Pagination: 25 per page (NFR39). Navigation via Prev / Page %{n} of %{total} / Next. Each page click HTMX-swaps the list fragment and updates the URL with `?page=N`.
   - Empty state: when the filter returns zero users AND the filter is Active + All roles (i.e., the "no other users" case on a fresh install), show the UX §Journey 7 empty-state copy: EN `"Only you here! Create a Librarian account for household members."` / FR `"Vous êtes seul·e ! Créez un compte Bibliothécaire pour les membres du foyer."`.

2. **Create user — form-driven, non-modal, server-validated.**
   - "New user" button in the panel header. Clicking GETs `/admin/users/new` → HTMX-swaps the form fragment into `#admin-users-form-slot`.
   - Fields: Username (text, required, trimmed, ≤255 chars), Password (password, required, 8..=72 chars), Role (select: Librarian (default) / Admin).
   - The form POSTs to `/admin/users` (CSRF-protected by 8-2 middleware; hidden `_csrf_token` is the first child of the `<form>` element per the `forms_include_csrf_token` audit).
   - Server validation:
     - Empty / whitespace-only username → 400 + `error.user.username_empty` FeedbackEntry, form re-renders with the value preserved.
     - Password < 8 chars → 400 + `error.user.password_too_short`.
     - Password > 72 chars → 400 + `error.user.password_too_long`.
     - Role not in { librarian, admin } → 400 + `error.user.role_invalid`.
     - Username collision with an active OR deactivated user → 409 + `error.user.username_taken` (rationale per epic AC: prevent audit confusion by reusing a deactivated username on a new row — reactivating the deactivated user is the intended path).
   - On success: Argon2-hash the password, INSERT, return an HTMX fragment that (a) re-renders the rows list so the new user appears in sorted position, (b) clears the form slot, (c) emits a success FeedbackEntry OOB swap (`User %{username} created`).
   - **Password is never echoed back in any response body, log, or trace field. The Form struct's `password: String` is consumed by the hash function and never cloned into any other sink.**

3. **Edit user — inline row form, optimistic-locking, password optional.**
   - Clicking "Edit" on a row GETs `/admin/users/{id}/edit` → HTMX-swaps the row with the edit form (`hx-target="#admin-users-row-{{ id }}" hx-swap="outerHTML"`).
   - Fields pre-filled: Username, Role. Password field is **blank with placeholder** (EN: "Leave blank to keep current" / FR: "Laisser vide pour conserver l'actuel"). Hidden `version` input from the SELECTed row.
   - POST `/admin/users/{id}` with the form. Validation same as Create (empty password is allowed here and means "do not change the hash"); non-empty password is validated and re-hashed.
   - Optimistic locking: if `version` mismatch, return 409 + `error.user.version_mismatch`.
   - Username change to a value already taken → 409 + `error.user.username_taken`.
   - Role change that would leave the caller as the sole admin-turned-librarian → 409 + `error.user.last_admin_demote` (see AC 5).
   - On success: row re-rendered in read-only form via the same target swap + success FeedbackEntry.

4. **Deactivate — soft-delete + immediate session kick-out, atomic.**
   - Deactivate button is a POST form (per template, with hidden `version` + `_csrf_token`) with `hx-confirm="{{ confirm_deactivate }}"` and `hx-post="/admin/users/{id}/deactivate"`.
   - The acting admin's own row has **no Deactivate button** (UX affordance gate — belt to the server-side guard in AC 5).
   - On POST, `UserModel::deactivate` runs in a single DB transaction:
     (a) Row lock target user.
     (b) Self-deactivate guard → 409 `error.user.self_deactivate`.
     (c) Last-admin guard → 409 `error.user.last_admin` (applies when target is admin and no other active admin exists).
     (d) UPDATE users SET `deleted_at = NOW()`, `version = version + 1` where version matches; `check_update_result` → 409 `error.user.version_mismatch` on stale.
     (e) UPDATE sessions SET `deleted_at = NOW()` where `user_id = ? AND deleted_at IS NULL` — every live session for that user dies in the same transaction.
     (f) Commit.
   - Success response: HTMX fragment that removes the row from the active list (or greys it out if "Show deactivated" is on) + success FeedbackEntry "User %{username} deactivated (%{count} session(s) ended)".
   - The deactivated user's next request hits the auth middleware's LEFT JOIN on `users` → `deleted_at IS NOT NULL` → falls into the anonymous fallback → their HTMX request gets redirected to `/login` (or their navigation gets a fresh anonymous session + they land on a page that forces login for admin/librarian surfaces). The timing guarantee is "next request after the COMMIT", not "within N seconds".

5. **Last-active-admin guard — enforced on BOTH deactivate AND role-demote.**
   - `SELECT COUNT(*) FROM users WHERE role = 'admin' AND deleted_at IS NULL AND id != ?` (excluding the acting user) must remain ≥ 1 after the proposed action.
   - Deactivate: counted inside `UserModel::deactivate`'s transaction with the target row under `FOR UPDATE` row-lock.
   - Role demote: counted inside `UserModel::demote_guard`, called from `admin_users_update` BEFORE `UserModel::update`, also under a row-lock transaction.
   - Distinct error keys: `error.user.last_admin` (deactivate) vs `error.user.last_admin_demote` (role change). The FR copy for demote explicitly tells the admin to create another admin first, per UX §Journey 7.

6. **Reactivate — clears `deleted_at`, user can log in on next attempt.**
   - Reactivate button on a deactivated row → POST `/admin/users/{id}/reactivate` with hidden `version`.
   - UPDATE users SET `deleted_at = NULL`, `version = version + 1` where version matches (stale → 409).
   - No session reinstatement (user was logged out; they'll log back in fresh with a new session row).
   - Success response: row re-rendered in active state + success FeedbackEntry.

7. **Password hashing — Argon2 via `services::password::hash_password`, no new hashing code.**
   - The single source of truth for hashing is `src/services/password.rs::hash_password` (this story's extraction from `routes/auth.rs`). Every user create / password change routes through it.
   - The verify path in `login` keeps using the same crate (`argon2 = "0.5"`) via the moved `verify_password` helper.
   - Zero Cargo.toml changes — argon2 is already a dependency.

8. **Session extractor and role propagation unchanged.**
   - `Session` struct unchanged. `Session::require_role(Role::Admin)` / `require_role_with_return(Role::Admin, "/admin?tab=users")` gate every new route.
   - No changes to session-timeout semantics (story 7-2) or CSRF middleware (story 8-2).

9. **Every new `<form method="POST">` carries `_csrf_token` as the first child — enforced by the 8-2 audit test.**
   - Running `cargo test templates_audit` succeeds with the new templates in place.
   - Intentionally breaking one template (e.g., removing the hidden input from `admin_users_form_create.html`) makes the audit fail → acts as the regression gate.

10. **`hx-confirm` allowlist grows 4 → 5.**
    - Add `admin_users_deactivate` (or the existing-style site identifier — mimic the 4 grandfathered entries' format) to `ALLOWED_HX_CONFIRM_SITES` in `src/templates_audit.rs`.
    - Update the audit assertion upper bound (`len == 4` → `len == 5`).
    - Update CLAUDE.md's "Modal scanner-guard invariant" bullet: "frozen at 4 grandfathered sites" → "frozen at 5 grandfathered sites, the 5th added in story 8-3 for admin user deactivation".
    - Commit message mentions the exemption explicitly so reviewers see the count change.

11. **i18n keys — EN + FR.**
    - Every user-visible string in templates + every error message routed to FeedbackEntry uses `rust_i18n::t!("admin.users.*")` or `rust_i18n::t!("error.user.*")`.
    - Post-YAML-edit: `touch src/lib.rs && cargo build` (rust-i18n proc macro re-read).
    - FR translations present for every new key (no EN-only fallback).

12. **Unit tests pass** — see §Ships point 9 for the full list.

13. **E2E test passes** — see §Ships point 10 for the full spec. Includes the Foundation Rule #7 smoke path.

14. **Documentation complete** — CLAUDE.md bullet on deactivate-semantics, route-role-matrix updated, architecture Authentication & Security subsection notes the deactivate→session cascade.

## Tasks / Subtasks

- [ ] **Task 1: `src/models/user.rs` — new model module (AC: 1, 2, 3, 5, 6)**
  - [ ] 1.1 Create the file with `UserRow` struct + `UserStatus` enum (`Active | Deactivated | All`)
  - [ ] 1.2 Implement `list_page`, `count_all`, `find_by_id`, `find_by_username` with `deleted_at IS NULL` predicate honoring the `filter_status` param
  - [ ] 1.3 Implement `create` — INSERT + catch 1062 → `AppError::Conflict("username_taken")`
  - [ ] 1.4 Implement `update` — optimistic-locking UPDATE, `new_password_hash: Option<String>` branch
  - [ ] 1.5 Implement `deactivate` — transactional: row-lock → self-guard → last-admin-guard → UPDATE users → UPDATE sessions → commit
  - [ ] 1.6 Implement `reactivate` — optimistic-locking UPDATE, clears `deleted_at`
  - [ ] 1.7 Implement `demote_guard` — transactional: row-lock → last-admin-count check. Returns `Conflict("last_admin_demote_blocked")` on violation, `Ok(())` otherwise
  - [ ] 1.8 Implement `last_login` sub-query: `SELECT MAX(created_at) FROM sessions WHERE user_id = ?` — no `deleted_at` filter on the JOIN (we want most-recent login regardless of current session state; see §Dev Notes "Last login query")
  - [ ] 1.9 Wire `pub mod user;` into `src/models/mod.rs`
  - [ ] 1.10 `cargo sqlx prepare` to regenerate `.sqlx/` offline cache after new queries land

- [ ] **Task 2: `src/services/password.rs` — DRY hashing extraction (AC: 7)**
  - [ ] 2.1 Create `src/services/password.rs` with `pub fn hash_password(plain: &str) -> Result<String, AppError>` (Argon2 + random salt)
  - [ ] 2.2 Move `fn verify_password` from `src/routes/auth.rs:438-447` to `src/services/password.rs` as `pub fn verify_password(plain: &str, hash: &str) -> bool` — update the single call site in `login`
  - [ ] 2.3 Wire `pub mod password;` into `src/services/mod.rs`
  - [ ] 2.4 Remove the inline `use argon2::…; use argon2::password_hash::SaltString;` blocks from `routes/auth.rs` test functions (lines 492-505, 509+) — make them call `services::password::hash_password` instead

- [ ] **Task 3: Admin Users handlers in `src/routes/admin.rs` (AC: 1, 2, 3, 4, 5, 6)**
  - [ ] 3.1 Add `UsersQuery`, `CreateUserForm`, `UpdateUserForm`, `DeactivateForm`, `ReactivateForm` `#[derive(Deserialize)]` structs at the top of the module
  - [ ] 3.2 Implement `admin_users_panel` — replaces the current stub; reads filters + page; loads `UserRow` list + count; builds the template struct; returns HTMX fragment or full page
  - [ ] 3.3 Implement `admin_users_create_form` (GET fragment) and `admin_users_create` (POST) — validate → hash → insert → re-render list + success feedback OOB
  - [ ] 3.4 Implement `admin_users_edit_form` (GET fragment) and `admin_users_update` (POST) — pre-fill, optimistic-lock, demote-guard, update, row swap
  - [ ] 3.5 Implement `admin_users_deactivate` (POST) — `UserModel::deactivate`, map errors to localized FeedbackEntry, row remove or grey-out
  - [ ] 3.6 Implement `admin_users_reactivate` (POST) — `UserModel::reactivate`, map errors, row swap
  - [ ] 3.7 Every handler's first line: `let return_path = build_return_path(&uri); session.require_role_with_return(Role::Admin, &return_path)?;`

- [ ] **Task 4: Route wiring in `src/routes/mod.rs` (AC: 1)**
  - [ ] 4.1 Add the 6 new routes (GET/POST on `/admin/users`, GET `/admin/users/new`, POST `/admin/users/{id}`, GET `/admin/users/{id}/edit`, POST `/admin/users/{id}/deactivate`, POST `/admin/users/{id}/reactivate`)
  - [ ] 4.2 Confirm none are added to `CSRF_EXEMPT_ROUTES` in `src/middleware/csrf.rs` (they MUST be CSRF-protected)
  - [ ] 4.3 Update `docs/route-role-matrix.md` — 6 new rows, Admin role, `csrf_exempt = no`

- [ ] **Task 5: Templates (AC: 1, 2, 3, 4, 6, 9, 10)**
  - [ ] 5.1 **Replace** `templates/fragments/admin_users_panel.html` with the full panel (header, filters, form slot, list include, pagination); update comment to "Replaced by story 8-3"
  - [ ] 5.2 Create `templates/fragments/admin_users_table.html` (table shell + row loop via `{% include "fragments/admin_users_row.html" %}`)
  - [ ] 5.3 Create `templates/fragments/admin_users_row.html` (one `<tr id="admin-users-row-{{ user.id }}">` with status-dependent actions)
  - [ ] 5.4 Create `templates/fragments/admin_users_form_create.html` — inline form; hidden `_csrf_token` as the FIRST child of `<form>`
  - [ ] 5.5 Create `templates/fragments/admin_users_form_edit.html` — inline edit form; hidden `_csrf_token` + hidden `version` as FIRST children
  - [ ] 5.6 Deactivate button: `<form method="POST" action="/admin/users/{{ user.id }}/deactivate" class="inline">` with `<input type="hidden" name="_csrf_token" …>` + `<input type="hidden" name="version" …>` + a `<button type="submit" hx-confirm="{{ confirm_deactivate }}" hx-post="/admin/users/{{ user.id }}/deactivate">{{ btn_deactivate }}</button>`
  - [ ] 5.7 Reactivate button: same shape minus the `hx-confirm`
  - [ ] 5.8 "New user" button in the panel header: `hx-get="/admin/users/new" hx-target="#admin-users-form-slot" hx-swap="innerHTML"`
  - [ ] 5.9 Zero inline `style=""` / `onclick=` / inline `<script>`/`<style>` — all CSS classes only (CSP from 7-4)

- [ ] **Task 6: `hx-confirm=` allowlist extension (AC: 10)**
  - [ ] 6.1 Add one entry to `ALLOWED_HX_CONFIRM_SITES` in `src/templates_audit.rs` — identifier consistent with the 4 existing entries (use the template filename or a semantic key, matching the existing style)
  - [ ] 6.2 Update the audit test's length assertion (`len == 4` → `len == 5` if it checks length; full-slice equality if it pins the contents)
  - [ ] 6.3 Update CLAUDE.md: "frozen at 4 grandfathered sites" → "frozen at 5; 5th added story 8-3 for admin user deactivation"

- [ ] **Task 7: i18n keys + YAML (AC: 11)**
  - [ ] 7.1 Add `admin.users.*` and `error.user.*` keys to `locales/en.yml` (see §Ships 8)
  - [ ] 7.2 Mirror every key in `locales/fr.yml` with the French translations from §Ships 8
  - [ ] 7.3 Remove the `admin.users.coming_in_story` placeholder key from both YAMLs (or mark it unused — prefer deletion)
  - [ ] 7.4 `touch src/lib.rs && cargo build` to force rust-i18n proc-macro re-read

- [ ] **Task 8: Unit tests (AC: 12)**
  - [ ] 8.1 `#[sqlx::test]` block in `src/models/user.rs` covering the list in §Ships 9 (create, unique, update locking, update empty-password, demote guard, deactivate self/last-admin/normal, reactivate, list pagination + filters, last_login computation)
  - [ ] 8.2 Route-level tests in `src/routes/admin.rs::tests` (or a new `tests/admin_users.rs` integration file): librarian→403 + anonymous→303 on all 6 routes; min-8-password validation; CSRF-missing→403 via middleware (may be covered by 8-2 integration — not duplicate)
  - [ ] 8.3 Verify every test runs green locally: `cargo test user:: admin_users`
  - [ ] 8.4 `cargo sqlx prepare` re-run if any query changes during test authoring

- [ ] **Task 9: E2E spec (AC: 13)**
  - [ ] 9.1 Create `tests/e2e/specs/admin/users.spec.ts` — spec ID `"UA"` (confirm no collision via `grep -rn 'specIsbn("UA"' tests/e2e/`)
  - [ ] 9.2 Foundation Rule #7 smoke path (see §Ships 10 first bullet)
  - [ ] 9.3 Self-deactivate-guard (UI hidden + server 409)
  - [ ] 9.4 Last-admin-deactivate-guard + last-admin-demote-guard
  - [ ] 9.5 Deactivate-invalidates-sessions (two browser contexts)
  - [ ] 9.6 Reactivate round-trip
  - [ ] 9.7 Run `./scripts/e2e-reset.sh` then `cd tests/e2e && npm test` — 3 clean cycles
  - [ ] 9.8 Flake gate: `grep -rE "waitForTimeout\(" tests/e2e/specs/admin/users.spec.ts` → zero hits (use DOM-state waits per CLAUDE.md)

- [ ] **Task 10: Documentation (AC: 14)**
  - [ ] 10.1 CLAUDE.md Key Patterns bullet: user-admin deactivate semantics (`deleted_at`-based, atomic session cascade, `active` vestigial)
  - [ ] 10.2 `docs/route-role-matrix.md` — 6 new rows under Admin
  - [ ] 10.3 `_bmad-output/planning-artifacts/architecture.md` Authentication & Security: short paragraph on user-admin lifecycle (deactivate cascades sessions in-transaction)

- [ ] **Task 11: Regression gate (AC: 12, 13)**
  - [ ] 11.1 `cargo test` — all unit + integration green
  - [ ] 11.2 `cargo clippy -- -D warnings` — zero warnings (Foundation Rule)
  - [ ] 11.3 `cargo sqlx prepare --check --workspace -- --all-targets` — offline cache up to date
  - [ ] 11.4 `./scripts/e2e-reset.sh` + `cd tests/e2e && npm test` — 3 clean cycles
  - [ ] 11.5 `cargo test templates_audit` — CSRF-form + hx-confirm-allowlist + CSP audits green with the new templates

## Review Findings (Code Review Pass 1 — 2026-04-19)

### CRITICAL Issues (Block merge)

- [ ] [Review][Patch] Missing `demote_guard` call in `admin_users_update` [admin.rs:521]
  - The handler calls `UserModel::update()` directly when role changes, without first calling `UserModel::demote_guard()` to enforce the last-admin-demote guard. This allows the sole admin to demote themselves to Librarian, leaving zero active admins. **Fix:** Call `demote_guard` before `update` when role changes.

- [ ] [Review][Patch] `demote_guard` missing `FOR UPDATE` row-lock [user.rs:~340]
  - Unlike `deactivate`, the `demote_guard` function does not acquire a row-lock. Two concurrent admin sessions could each see "one other admin remains" and both proceed to demote themselves. **Fix:** Wrap `demote_guard` in a transaction with `SELECT ... FOR UPDATE` on the target user.

- [ ] [Review][Patch] `hx-confirm` allowlist NOT extended (4 → 5) [templates_audit.rs:35]
  - The deactivate button in `admin_users_panel.html:51` contains `hx-confirm=`, but `ALLOWED_HX_CONFIRM_SITES` has only 4 entries. The `cargo test templates_audit` audit will fail. **Fix:** Add the deactivate site identifier to the allowlist and update the assertion from `len == 4` to `len == 5`.

- [ ] [Review][Patch] Missing import `verify_password` in auth.rs tests [auth.rs:504-505]
  - Lines 504-505 call `verify_password()` without importing it; elsewhere the code uses the qualified path `crate::services::password::verify_password()`. This causes a compilation error. **Fix:** Add `use crate::services::password::verify_password;` at the top of the test module or use the qualified path consistently.

- [ ] [Review][Patch] Pagination `?page=` parameter completely ignored [admin.rs:732-755]
  - The `UsersQuery` struct defines a `page` field, and the template renders pagination UI, but the handler uses `Query(_query)` (underscore = ignored) and always renders page 1. The `page` parameter is never read or used for pagination offset. **Fix:** Extract `page` from query, validate (clamp to 1 if 0), convert to offset = (page-1)*25, and pass to `list_page()`.

### HIGH Priority Issues (Major gaps)

- [ ] [Review][Patch] Missing error handling for `demote_guard` conflicts [admin.rs:478]
  - When `demote_guard()` returns `Conflict("last_admin_demote_blocked")`, the handler has no error mapping to convert this to a user-visible FeedbackEntry. Currently the error bubbles up as a raw 409 conflict. **Fix:** Catch the conflict, map to the i18n key `error.user.last_admin_demote`, and return a FeedbackEntry with the localized message.

- [ ] [Review][Patch] No error mapping for `Conflict` in `admin_users_update` [admin.rs:521]
  - The `UserModel::update()` returns `Conflict("username_taken")` on duplicate username, but the handler doesn't catch it. The error bubbles up as a plain 409 with the raw identifier string, not a localized FeedbackEntry. **Fix:** Catch `Conflict("username_taken")` and return localized feedback like `admin_users_create` does.

- [ ] [Review][Patch] Deactivate/reactivate missing success feedback messages [admin.rs:548-594]
  - Both `admin_users_deactivate` and `admin_users_reactivate` return only the updated row HTML, but lack the success FeedbackEntry OOB swap that `admin_users_create` includes. Users see no explicit confirmation message. The i18n keys `success_deactivated` and `success_reactivated` are defined but never rendered. **Fix:** Return `HtmxResponse` with both the row and a feedback OOB swap (like create does).

- [ ] [Review][Patch] `confirm_deactivate` i18n placeholder is empty [admin.rs:797]
  - The panel renders `confirm_deactivate` with an empty `username=""` placeholder, so every Deactivate button shows a generic confirmation like "Deactivate ? They'll be signed out...". The username is only substituted in `render_user_row`, not in the panel. **Fix:** Pass the actual username in the i18n call, or compute the confirmation message dynamically per row.

### MEDIUM Priority Issues

- [ ] [Review][Patch] Edit form cancel button has broken HTMX target [templates/fragments/admin_users_form_edit.html:31]
  - The Cancel button fetches `/admin/users` (the full panel HTML) but targets `#admin-users-row-{{ user.id }}` (a single `<tr>`), causing DOM corruption. The entire panel gets swapped into a row cell. **Fix:** Either (a) fetch only the row fragment via a dedicated handler, or (b) change the target to `#admin-users-list` and swap the entire list, or (c) use a simpler cancel mechanism (e.g., close the form without a new fetch).

### MINOR / LOW Issues (Warnings)

- [x] [Review][Dismiss] Unused variable warnings (state, acting_admin_id) — compiler warnings, non-blocking
- [x] [Review][Dismiss] Case-insensitive username collation — documented behavior per CLAUDE.md, acceptable

---

**Summary:** 9 issues identified (5 CRITICAL, 4 HIGH, 0 MEDIUM at implementation level). All are actionable patches with clear fixes. Implementation is ~80% feature-complete but requires these corrections before merge.

## Dev Notes

### Deactivation semantics clarification (`deleted_at` vs `active`)

The schema has BOTH `users.deleted_at TIMESTAMP NULL` AND `users.active BOOLEAN NOT NULL DEFAULT TRUE`. The current login predicate (`src/routes/auth.rs:153-154`) enforces both: `WHERE username = ? AND active = TRUE AND deleted_at IS NULL`. This story uses **only `deleted_at`** for deactivation — the epic AC is explicit about this, and `deleted_at IS NULL` is the project's universal soft-delete convention (CLAUDE.md §Key Patterns). The `active` boolean stays at the schema default (`TRUE`) for all new users; no handler in this story writes to it. If a future story wants a "force-disable without surfacing in Trash/Reactivate UX" semantic, `active = FALSE` is free for that use. For now, treat `active` as vestigial — do not toggle it, do not expose it in the admin UI, do not document it to Guy as a knob.

### Last login query

The `UserRow.last_login` is the most recent `sessions.created_at` for that `user_id`, regardless of session deletion state. Rationale: the admin wants to see "when did this user last log in" — a user who logged in this morning and logged out at noon has a `last_login` of "this morning", not "never". The `sessions` row for that login exists (possibly soft-deleted via the logout cascade), and its `created_at` is the signal we want. SQL:

```sql
SELECT u.id, u.username, …,
       (SELECT MAX(s.created_at) FROM sessions s WHERE s.user_id = u.id) AS last_login
FROM users u
WHERE u.deleted_at IS NULL /* or IS NOT NULL depending on filter_status */
ORDER BY u.username ASC
LIMIT ? OFFSET ?
```

The subquery is cheap (sessions is indexed on `user_id`; MariaDB's correlated-subquery optimizer handles this well on small tables — mybibli is a single-user NAS with at most dozens of users). No denormalized `users.last_login` column is added — it would need invalidation on every login and risk drift.

### Last-admin race safety

The `FOR UPDATE` row-lock inside `UserModel::deactivate`'s transaction is the ONLY protection against two concurrent admin sessions both seeing "one other admin exists" and both deactivating → zero admins. Same logic for `demote_guard`. Implementation sketch:

```rust
pub async fn deactivate(pool: &DbPool, id: u64, version: i32, acting: u64) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    let target = sqlx::query!("SELECT role FROM users WHERE id = ? AND deleted_at IS NULL FOR UPDATE", id)
        .fetch_optional(&mut *tx).await?
        .ok_or_else(|| AppError::NotFound("user".into()))?;

    if id == acting { return Err(AppError::Conflict("self_deactivate_blocked".into())); }

    if target.role == "admin" {
        let remaining: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM users WHERE role = 'admin' AND deleted_at IS NULL AND id != ?",
            id
        ).fetch_one(&mut *tx).await?;
        if remaining == 0 { return Err(AppError::Conflict("last_admin_blocked".into())); }
    }

    let result = sqlx::query!("UPDATE users SET deleted_at = NOW(), version = version + 1 WHERE id = ? AND version = ? AND deleted_at IS NULL", id, version)
        .execute(&mut *tx).await?;
    check_update_result(result.rows_affected(), "user")?;  // -> Conflict("version_mismatch")

    let sessions_killed = sqlx::query!("UPDATE sessions SET deleted_at = NOW() WHERE user_id = ? AND deleted_at IS NULL", id)
        .execute(&mut *tx).await?.rows_affected();

    tx.commit().await?;
    tracing::info!(user_id = id, sessions_killed, "user deactivated");
    Ok(())
}
```

Rollback on any error before commit is automatic (sqlx's `Transaction` drops without commit → rollback). The happy path is one network round-trip to MariaDB (pool re-use); the guarded paths are the same number.

### Conflict error mapping — short, specific identifiers

The codebase's `AppError::Conflict(String)` renders via FeedbackEntry. Use short stable identifiers as the payload string (`"self_deactivate_blocked"`, `"last_admin_blocked"`, `"last_admin_demote_blocked"`, `"username_taken"`, `"version_mismatch"`) and let the handler map them to i18n keys for the user-visible text. Rationale: the payload string is an engineer-facing identifier surfaced in logs + test assertions; the i18n key is the user-facing one. Decoupling them lets FR / EN copy evolve without breaking tests.

### Handler error → FeedbackEntry recipe

```rust
match result {
    Err(AppError::Conflict(ref kind)) if kind == "self_deactivate_blocked" => {
        let body = feedback_html_pub("error", &t!("admin.users.btn_deactivate"), &t!("error.user.self_deactivate"));
        // Reuse the 4-header coordination pattern from CSRF middleware: HX-Retarget: #feedback-list, HX-Reswap: beforeend
        return Ok(Response::builder()
            .status(409)
            .header("HX-Retarget", "#feedback-list")
            .header("HX-Reswap", "beforeend")
            .header("Cache-Control", "no-store")
            .body(Body::from(body))?);
    },
    // …other branches
}
```

This mirrors the CSRF middleware's rejection envelope (story 8-2) so HTMX reliably swaps the error body into the feedback list without a full-page refresh. Keep the pattern consistent — future admin stories (8-4..8-7) will reuse it.

### Template authoring rules that bite if missed

1. `<input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">` MUST be the **first child** of every `<form method="POST">` — the `forms_include_csrf_token` audit regex from 8-2 scans for this. Not second-child, not after a wrapping `<div>`. First child.
2. `hx-confirm=` on any new element requires an allowlist entry in `src/templates_audit.rs::ALLOWED_HX_CONFIRM_SITES`. This story adds exactly one (the Deactivate button). Any OTHER new `hx-confirm=` fails `cargo test`.
3. Zero inline styles (`style="…"`), zero inline scripts (`<script>…</script>`), zero inline event handlers (`onclick=…`). CSP from 7-4 blocks them; the templates_audit test detects them. All CSS via classes, all JS behavior via `data-action="…"` + delegated handlers in `static/js/mybibli.js` (there's no JS behavior needed in this story — HTMX covers every interaction).
4. Every new template struct that `{% extends "layouts/base.html" %}` must populate the `csrf_token: String` field AND the existing common fields (`lang`, `role`, `current_page`, `skip_label`, `nav_*`, `admin_tabs_*`, `trash_count`). Copy the pattern from `AdminPageTemplate` in 8-1.
5. Tailwind classes only — no custom CSS. The 8-1 admin panel uses Tailwind conventions (`text-stone-600`, `dark:text-stone-400`, etc.) — follow them.

### MariaDB `RowNotFound` vs `Conflict("version_mismatch")` discrimination

`sqlx::query!(...).execute(...).await?.rows_affected()` returns 0 in TWO cases:
1. The row exists but `version` doesn't match → `version_mismatch`.
2. The row doesn't exist (already deleted, never existed) → `not_found`.

`check_update_result` in `services/locking.rs` collapses both into `Conflict`. For user-visible messaging we want to distinguish. Option: do a pre-SELECT inside the same transaction (already there for the row-lock in deactivate/demote) — if the SELECT returns `None`, return `AppError::NotFound("user")` → 404. If the SELECT returns `Some` but the UPDATE affects 0 rows, it's a version race → `Conflict("version_mismatch")`. The `update` handler (Task 3.4) should do this too for symmetry.

### ISO short-date + datetime formatting in templates

Existing pattern: `chrono` is already a transitive dep. Render dates in templates via an Askama filter or pre-format in Rust before passing. The project's existing pattern (check 8-1's `created_at` render in the Trash-badge code path) is to pre-format in Rust using `chrono::DateTime::format("%Y-%m-%d")`. Match it. Do NOT invent a new date-formatter Askama filter.

### Test-environment DB migrations

Per CLAUDE.md: `docker compose -f tests/docker-compose.rust-test.yml up -d` → DB at `mysql://root:root_test@localhost:3307/mybibli_rust_test`. Each `#[sqlx::test(migrations = "./migrations")]` test gets a fresh DB. This story adds NO new migration; unit tests just exercise the existing schema.

### Project Structure Notes

- **New files:** `src/models/user.rs`, `src/services/password.rs`, `templates/fragments/admin_users_table.html`, `templates/fragments/admin_users_row.html`, `templates/fragments/admin_users_form_create.html`, `templates/fragments/admin_users_form_edit.html`, `tests/e2e/specs/admin/users.spec.ts`.
- **Modified files:** `src/models/mod.rs` (register user module), `src/services/mod.rs` (register password module), `src/routes/admin.rs` (replace users-panel stub + 5 new handlers + form/query structs), `src/routes/auth.rs` (replace inline `fn verify_password` call with `services::password::verify_password`; remove inline Argon2 hashing from test helpers at lines 494-509), `src/routes/mod.rs` (6 new routes under `/admin/users`), `src/templates_audit.rs` (allowlist grows 4 → 5), `templates/fragments/admin_users_panel.html` (stub → real panel), `locales/en.yml` + `locales/fr.yml` (admin.users.* and error.user.* keys), `CLAUDE.md` (deactivate-semantics bullet + hx-confirm count), `docs/route-role-matrix.md` (6 new rows), `_bmad-output/planning-artifacts/architecture.md` (Auth & Security: user-admin deactivate cascade paragraph).
- **NOT modified (explicitly):** migrations (no schema change), `src/middleware/csrf.rs` (`CSRF_EXEMPT_ROUTES` stays frozen at `[("POST", "/login")]`), `src/services/soft_delete.rs::ALLOWED_TABLES` (users deliberately not added), `Cargo.toml` (argon2 already a dep, subtle already a dep via 8-2), `src/middleware/auth.rs::Session` (unchanged).
- **Detected conflicts / variances:**
  - Schema has `active` + `deleted_at` columns; this story uses only `deleted_at`. `active` stays untouched. Documented in §Dev Notes "Deactivation semantics clarification".
  - UX §Journey 7 shows a double-password-entry pattern; Epic AC does not require it. Ship single-password input. Flagged in §Does NOT ship.
  - UX §Journey 7 shows optional "Display name" field; no schema column exists. Drop from this story. Flagged in §Does NOT ship as the largest scope deviation.
  - UX §Journey 7 uses "Delete" wording; Epic AC uses "Deactivate" (soft-delete). Epic AC wins. i18n copy is "Deactivate" / "Désactiver".

### References

- [Source: CLAUDE.md — Foundation Rules #1/#2/#3/#4/#5/#6/#7/#10/#11]
- [Source: CLAUDE.md — Key Patterns — Session, HTMX OOB Swap Pattern, CSP & hardening headers, Modal scanner-guard invariant, CSRF synchronizer token (story 8-2)]
- [Source: CLAUDE.md — Admin page tab pattern (story 8-1)]
- [Source: _bmad-output/planning-artifacts/epics.md — Epic 8 Story 8.3 (User administration) acceptance criteria, lines 1088-1105]
- [Source: _bmad-output/planning-artifacts/prd.md — FR68 (create/edit/deactivate + role assignment), line 708]
- [Source: _bmad-output/planning-artifacts/architecture.md — Authentication & Security (lines 445-505), Database Schema Decisions (users + sessions), Session Lifecycle]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md — §Journey 7 User Management lines 1084-1133]
- [Source: _bmad-output/implementation-artifacts/8-1-admin-shell-and-health-tab.md — `AdminTab` enum, `render_admin`/`render_panel` idiom, `Session::require_role_with_return` pattern, `#admin-shell` HTMX target]
- [Source: _bmad-output/implementation-artifacts/8-2-csrf-middleware-and-form-token-injection.md — CSRF middleware, `_csrf_token` hidden-input contract, `forms_include_csrf_token` audit, HX-Trigger/Retarget/Reswap rejection envelope]
- [Source: _bmad-output/implementation-artifacts/7-5-scanner-guard-modal-interception.md — `ALLOWED_HX_CONFIRM_SITES` frozen allowlist pattern]
- [Source: _bmad-output/implementation-artifacts/1-9-minimal-login.md — Argon2 verification, session-creation flow]
- [Source: migrations/20260329000000_initial_schema.sql lines 208-220 (users table), 222-235 (sessions table)]
- [Source: migrations/20260416000001_add_users_preferred_language.sql — preferred_language column]
- [Source: src/routes/auth.rs lines 153-154 (login predicate `active = TRUE AND deleted_at IS NULL`), 438-447 (verify_password to be moved), 494-509 (test helper Argon2 usage to DRY)]
- [Source: src/routes/admin.rs lines 41-84 (AdminTab enum), 198-239 (admin_page + admin_users_panel stub), 276-427 (render_admin/render_panel/render_shell pattern)]
- [Source: src/middleware/auth.rs — `Session`, `require_role_with_return`, `Role::{Anonymous, Librarian, Admin}`]
- [Source: src/middleware/csrf.rs — `CSRF_EXEMPT_ROUTES` (frozen at `[("POST", "/login")]`), middleware layer wiring]
- [Source: src/services/locking.rs — `check_update_result(rows_affected, entity_type)`]
- [Source: src/services/soft_delete.rs — `ALLOWED_TABLES` (users deliberately not included)]
- [Source: src/templates_audit.rs — `forms_include_csrf_token`, `hx_confirm_matches_allowlist`, `csrf_exempt_routes_frozen`]
- [Source: src/utils.rs — `html_escape`; routes/catalog.rs — `feedback_html_pub` variant renderer]
- [Source: src/middleware/htmx.rs — `HxRequest`, `HtmxResponse`, `OobUpdate`]
- [Source: tests/e2e/helpers/auth.ts — `loginAs(page, role?)`, `logout(page)`]
- [Source: tests/e2e/helpers/isbn.ts — `specIsbn(specId, seq?)` for unique per-spec identifiers]
- [Source: argon2 crate 0.5 docs — https://docs.rs/argon2/0.5/argon2/]

### Previous Story Intelligence

**Story 8-1 (Admin shell + Health tab, done 2026-04-17) — carries forward:**
- The `/admin` tab plumbing, `AdminTab` enum, `?tab=` resolution, HTMX panel-fragment vs full-page render, `require_role_with_return(Role::Admin, …)` guard are all live and reused verbatim. Do NOT re-implement tab resolution.
- `render_admin`/`render_panel`/`render_shell` (`src/routes/admin.rs` lines 276-427) is the template rendering idiom; the Users panel follows the same flow. Copy the shape — don't invent a new one.
- `ALLOWED_TABLES` in `services/soft_delete.rs` was promoted to `pub` in 8-1; this story does NOT need to touch it (users deliberately NOT added to the whitelist — see §Cross-cutting decisions).
- 8-1's stub comment in `admin_users_panel.html` says "Replaced by story 8-2" — this is stale (Epic 8 was renumbered 2026-04-18 when CSRF was inserted as 8-2). Update to "Replaced by story 8-3" when replacing the file.

**Story 8-2 (CSRF middleware, in-progress with patches applied 2026-04-18) — foundation:**
- Every POST/PUT/PATCH/DELETE in this story is CSRF-protected by the middleware. `_csrf_token` hidden input is the first child of every `<form method="POST">` (audit-enforced). `<meta name="csrf-token">` is in `layouts/base.html`.
- The `HX-Trigger: csrf-rejected` + `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` + `Cache-Control: no-store` envelope is a NEW project-wide pattern — this story reuses the same coordination headers for Conflict/NotFound errors (see §Dev Notes "Handler error → FeedbackEntry recipe").
- The `base_context()` DRY helper is a known deferred item from 8-2 code review. Do NOT implement it here — flatten common template fields manually, 8-1-style.
- Anonymous visitors now get a session row on first GET (lazy anonymous session) with their own `csrf_token`. Not directly relevant to user-admin but worth knowing: session INSERTs happen on GET for non-cookie visitors.

**Story 7-5 (scanner-guard) — frozen-allowlist pattern:**
- `ALLOWED_HX_CONFIRM_SITES` currently has 4 entries (CLAUDE.md says "5" due to pre-existing doc drift that 8-2's code-review also touched — verify the CLAUDE.md number after 8-2 patches land). This story adds one → `len == 5` after this story lands.
- The modal-guard behavior is not reused here (we're not opening `<dialog>` modals; just `hx-confirm=` native browser confirms).

**Story 7-3 (language toggle) — argon2 hashing pattern reference:**
- Test fixtures at `src/routes/auth.rs:492-509` show the current inline Argon2 hashing pattern (SaltString + rand_core::OsRng + Argon2::default + hash_password). Task 2 extracts this into `services::password::hash_password` — the code should be literally the same API surface, just in a reusable function.

**Story 1-9 (minimal login) — Argon2 + session cookie baseline:**
- `fn verify_password` at `src/routes/auth.rs:438-447` is the canonical verify function; Task 2.2 moves it to `services::password`.
- The login INSERT at `src/routes/auth.rs:192-212` is the reference pattern for creating a session row.
- The `users` table schema has not changed since 1-9 (only `preferred_language` added by 7-3); all queries in this story fit the current schema.

### Git Intelligence Summary

Last 5 commits as of 2026-04-18:
- `92c9d7e` Story 8-2 (CSRF): code-review batch patches (16 of 38 findings) — 8-2 is green for the core flow; 22 findings deferred (none block 8-3).
- `0f80557` Story 8-2 (CSRF): dev-story implementation — middleware + form-token injection + lazy anonymous sessions — CSRF is LIVE; `_csrf_token` audit is active; layer order confirmed `CSP → Session-resolve → Locale → CSRF → handler`.
- `0fcbb34` Story 8-2 (CSRF): apply validate-story pass 2 patches + add Foundation Rule #11 — Foundation Rule #11 (GitHub Issues as single source of truth for change-requests, bugs, known-failures, code-review findings) applies to this story: any dev-agent-discovered follow-up gets a GH issue, not a markdown note.
- `9124d0d` Story 8-2 (CSRF): apply 7 critical + 5 enhancement patches from validate-story — establishes the `HX-Trigger: csrf-rejected` + `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` + `Cache-Control: no-store` envelope pattern this story reuses for Conflict errors.
- `23b1446` Story 8-2 (CSRF middleware + form-token injection): create-story output — reference template for the level of detail this story file aims for.
- `aff102e` Story 8-1: apply code-review patches (P1-P5) and mark done — admin shell is stabilized; 8-3 builds on stable foundation.

## Dev Agent Record

### Agent Model Used

claude-opus-4-7[1m] (2026-04-18 create-story run)

### Debug Log References

_Populated by dev-story run._

### Completion Notes List

_Populated by dev-story run._

### File List

_Populated by dev-story run._
