# Story 9.14: Migrate hx-confirm — admin user deactivation (final cleanup)

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As the project maintainer,
I want the admin user-deactivate flow on `/admin?tab=users` migrated from `hx-confirm=` to the UX-DR8 Modal component built in 9.10,
so that UX-DR8 is fully implemented, the existing server-side guards (self-deactivate + last-active-admin from story 8-3) keep working unchanged, and **the fifth and final grandfathered site exits `ALLOWED_HX_CONFIRM_SITES` — the constant becomes `&[]` and the constraint becomes "no `hx-confirm=` anywhere in templates."**

## ⚠️ Existing-code reality check

Before writing a single line, walk the code that 9-14 touches and verify the assumptions below — they are LOCKED IN by the 9-13 close (current main as of 2026-05-07):

- **Modal macro is shipped, 10 params, will be EXTENDED to 11 params in this story (the only behavioral change to the macro since 9-11).** `templates/components/modal.html` currently takes `(variant, title, body_html, confirm_label, cancel_label, action_url, action_method, csrf_token, hx_target, hx_swap)`. 9-14 adds an 11ᵗʰ param `version: i32` rendered as `<input type="hidden" name="version" value="{{ version }}">` only when `version != 0` — required because the deactivate handler reads `Form<DeactivateForm>` with optimistic-locking `version: i32` (`src/routes/admin.rs:126-129`). All 4 existing callers (`templates/fragments/borrower_delete_modal.html`, `return_loan_modal.html`, `contributor_delete_modal.html`, `series_delete_modal.html`) pass a literal `0` since their handlers don't use optimistic locking on the destructive path. **Why not rule-of-three with a body_html-embedded form?** Because the macro's `<form>` carries the Cancel/Confirm buttons + `data-modal-confirm` selector + the macro-rendered `_csrf_token` — splitting the form between body_html and the macro-emitted form is impossible (HTML doesn't allow nested forms). Why not encode `version` in `action_url` (`?version=N`)? Because `DeactivateForm` reads from request body, not query — switching contract violates "server contract unchanged" mandate.

- **`static/js/modal.js` is shipped (197 LOC).** Focus trap + Escape close + backdrop close + `[data-modal-trigger][data-pressed="true"]` focus-restoration + `htmx:afterRequest` close-on-2xx are all in place. UNCHANGED in this story.

- **`<div id="modal-slot">` is in `layouts/base.html`.** Already present on every page that extends the layout, including `/admin?tab=users`. No layout edit needed.

- **`<div id="feedback-list">`** is the OOB feedback target used by `admin_users_deactivate` (`src/routes/admin.rs:709-712`) and `admin_users_reactivate` (`:741-744`) to deliver success feedback. It must already exist on the admin page; verify by reading `templates/pages/admin.html` and/or `templates/fragments/admin_users_panel.html` during Task 1 and confirm the div is rendered.

- **One `hx-confirm=` site in scope:**
  - `templates/fragments/admin_users_row.html:23` — deactivation form inside the action cell of every active-user row. The whole `<form method="POST" hx-post hx-confirm hx-target hx-swap>...<button>...</button></form>` lives on lines 23–27 (5 lines including the closing tag). The form already targets `#admin-users-row-{{ user.id }}` with `hx-swap="outerHTML"` and submits hidden `_csrf_token` + `version` inputs — the row swap + optimistic-locking shape is preserved by the macro contract through the new 11ᵗʰ param.

- **Reactivate button** at `templates/fragments/admin_users_row.html:30-34` (wrapped by `{% if user.deleted_at.is_some() %}` at line 29 and `{% endif %}` at line 35) is OUT OF SCOPE — it never carried `hx-confirm` (no destructive-action confirmation; reactivation is recoverable). Untouched by this story.

- **Current `ALLOWED_HX_CONFIRM_SITES`** in `src/templates_audit.rs:35-37` has 1 entry × 1 occurrence = 1 grandfathered. After 9-14: the array becomes `&[]` (empty). Result: 0 entries, 0 occurrences. **The audit test `hx_confirm_matches_allowlist` MUST keep passing with the empty slice — the test's positive-assertion shape (sum allowed counts = total grep hits) holds at `0 == 0`.** The audit doc-comment at lines 30-34 must be updated per AC8 to remove the "the only templates allowed to carry this attribute" wording — replaced with "no template may carry this attribute" (the steady-state contract).

- **POST handler `admin_users_deactivate`** lives in `src/routes/admin.rs:680-714` — same file as the GET admin handlers. UNCHANGED by this story. Key contract:
  - Endpoint: `POST /admin/users/{id}/deactivate` (registered at `src/routes/mod.rs:256` as `axum::routing::post(admin::admin_users_deactivate)`).
  - **Role gate: `Role::Admin`** (`require_role_with_return(Role::Admin, "/admin?tab=users")`) — admin-only feature; anonymous gets 303 → `/login?next=%2Fadmin%3Ftab%3Dusers`, librarian gets 403.
  - Reads `Form<DeactivateForm>` (body): `version: i32`, `_csrf_token: String`. The CSRF middleware on `POST` enforces `_csrf_token` BEFORE the handler runs.
  - On success: returns `HtmxResponse` with `main: row_html` (the updated row, deactivated state — `<span>Deactivated</span>` in the status cell, no Deactivate button, with Reactivate button) + OOB `feedback-list` (success FeedbackEntry "User Foo deactivated (3 session(s) ended)").
  - **On `AppError::Conflict`**: the handler returns the AppError unchanged via `?`. **Conflict variant strings** (verified in `src/models/user.rs:287,300`): `"self_deactivate_blocked"` (when `id == acting_admin_id`), `"last_admin_blocked"` (when target is the last active admin), version-mismatch (a localized string from `services::locking::check_update_result` via `rust_i18n::t!("error.conflict", entity = "user")` — NOT a literal "version_mismatch" string). **`AppError::Conflict::IntoResponse` contract** (verified in `src/error/mod.rs:140-155`): returns **HTTP 409 + `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend`** + the inline feedback HTML body. The `HX-Retarget` header **overrides** the modal form's `hx-target=#admin-users-row-{id}` — the error feedback is appended to `#feedback-list` (the page-level OOB target rendered at `templates/pages/admin.html:11`), the user's row stays intact. This is BETTER UX than the spec previously implied. The 8-3 regression-net tests already lock this behavior.
  - On NotFound (deleted user): `UserModel::deactivate` returns `AppError::NotFound` → AppError's `IntoResponse` renders inline feedback (same retarget contract).

- **`admin.users.confirm_deactivate`** in `locales/en.yml:556` and `locales/fr.yml:556` is the OLD plain-confirm copy (`"Deactivate %{username}? They'll be signed out immediately and cannot log back in until reactivated."` / `"Désactiver %{username} ? La personne sera déconnectée immédiatement et ne pourra plus se connecter jusqu'à réactivation."`). This story DROPS it (zero callers after migration; mirror of 9-10's `borrower.confirm_delete` drop, 9-11's `loan.return_confirm` drop, 9-12's `contributor_detail.confirm_delete` drop, 9-13's `series.confirm_delete` drop). The sibling key `admin.users.btn_deactivate` (`"Deactivate"` / `"Désactiver"`) — the trigger button label — is RETAINED.

- **`admin_users_row.html` is rendered in TWO contexts** (verified by grep). Both must work after migration:
  1. **Standalone** via `AdminUsersRow` template (`src/routes/admin.rs:271-285`) — used by `admin_users_row_view`, `admin_users_update`, `admin_users_deactivate`, `admin_users_reactivate` to return the updated row.
  2. **`{% include %}`-d** from `templates/fragments/admin_users_table.html:18-19` — when the panel re-renders the full table, the table fragment iterates `users: Vec<UserWithConfirm>` and `let`-binds each item's `user` and `confirm_deactivate` fields before including the row fragment.
  
  This means the cleanup of `confirm_deactivate` is BIGGER than just the row template + `AdminUsersRow` struct. Verified call-site map:
  - `src/routes/admin.rs:216-219` — wrapper struct `UserWithConfirm { user: UserRow, confirm_deactivate: String }` (created exclusively for the panel-table iteration)
  - `src/routes/admin.rs:249` — `AdminUsersPanel.users: Vec<UserWithConfirm>` (must become `Vec<UserRow>`)
  - `src/routes/admin.rs:283` — `AdminUsersRow.confirm_deactivate` field
  - `src/routes/admin.rs:1104-1107` — `render_panel`'s `users: Vec<UserWithConfirm> = users_raw.into_iter().map(...).collect()` block
  - `src/routes/admin.rs:1171,1185` — `render_user_row`'s `let confirm_deactivate = ...` (line 1171) + `confirm_deactivate,` ctor field (line 1185)
  - `templates/fragments/admin_users_row.html:23` — `{{ confirm_deactivate|e }}` attribute interpolation
  - `templates/fragments/admin_users_table.html:19` — `{% let confirm_deactivate = item.confirm_deactivate %}`
  
  **Total: 5 Rust sites + 2 template sites + the i18n key.** Migration plan:
  - Delete the `UserWithConfirm` struct entirely (only used for `confirm_deactivate` plumbing).
  - Change `AdminUsersPanel.users` to `Vec<UserRow>`.
  - In `render_panel`, replace the `into_iter().map(...).collect()` block with `let users = users_raw;` (or skip the binding altogether).
  - In `admin_users_table.html:18-19`, replace `{% let user = item.user %}` + `{% let confirm_deactivate = item.confirm_deactivate %}` with `{% let user = item %}` (and drop the `confirm_deactivate` binding entirely).
  - Drop the `confirm_deactivate` field from `AdminUsersRow` + the `let` binding (line 1171) + the ctor field (line 1185).
  - Drop the `{{ confirm_deactivate|e }}` from `admin_users_row.html:23` (subsumed by AC3's whole-form replacement).

- **The trigger BUTTON visibility gate** `{% if user.deleted_at.is_none() && user.id != acting_admin_id %}` (`templates/fragments/admin_users_row.html:22`) STAYS — admins never see the trigger on their own row (UI-side self-deactivate guard, mirror of 8-3 spec). The server-side guard (handler returns 409 if `id == acting_admin_id`) is the second layer (defense-in-depth: a malicious admin crafting a direct request to `POST /admin/users/{own_id}/deactivate` hits the server guard).

- **CLAUDE.md "Modal scanner-guard invariant"** section currently says (verbatim, 2026-05-07): *"the allowlist is frozen at 5 grandfathered sites (5th added in story 8-3 for admin user deactivation) and only changes through explicit review."* This wording was already inaccurate after 9-10/9-11/9-12/9-13 trimmed the count — postponed by every prior story per the "9-14 rewrites the whole sentence in one shot" plan. **9-14 IS that rewrite.** AC8 replaces this line with *"the allowlist is empty post Epic 9 — any new `hx-confirm=` is BLOCKED outright by `templates_audit.rs::hx_confirm_matches_allowlist`."*

## Acceptance Criteria

1. **AC1 — NEW handler `GET /admin/users/:id/deactivate-modal`** in `src/routes/admin.rs` (sibling of `admin_users_row_view` at line 563):
   - Returns the rendered modal fragment via the shared `components/modal.html::modal` macro, variant `delete` (soft-delete — `users.deleted_at` is set; the user is recoverable via the existing Reactivate button on the same row, NOT via the Trash panel since `users` is intentionally NOT in `services::soft_delete::ALLOWED_TABLES`).
   - Pre-translates 4 i18n keys: title (`admin.users.deactivate_modal_title` — `"Deactivate user %{username}?"` interpolated via `t!(..., username = …)`, mirror of 9-13's `name` interpolation pattern), body (`admin.users.deactivate_modal_body` — `"They will be logged out immediately and cannot log back in until reactivated."`), confirm (`admin.users.deactivate_modal_confirm` — `"Deactivate"`), cancel (`common.cancel` — already shipped by 9-10).
   - **Role gate**: `session.require_role_with_return(Role::Admin, "/admin?tab=users")?` — mirror of the existing `admin_users_*` handlers. Anonymous gets 303 → `/login?next=%2Fadmin%3Ftab%3Dusers`. Librarian gets 403 (one-step lower than Admin — different from 9-10/9-11/9-12/9-13 where Librarian COULD access).
   - Returns 404 if the user is already soft-deleted OR not found. **Verified**: `UserModel::find_by_id` (`src/models/user.rs:133-148`) **DOES NOT filter `deleted_at IS NULL`** — its doc-comment explicitly says "including deactivated users." Therefore the handler MUST add an explicit guard:
     ```rust
     let user = UserModel::find_by_id(pool, id)
         .await?
         .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;
     if user.deleted_at.is_some() {
         return Err(AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()));
     }
     ```
     The audit semantics: a soft-deleted user is "already deactivated"; the modal is meaningless. The 404 protects against double-deactivation races.
   - **Pre-flight self-deactivate / last-admin guards: NOT IMPLEMENTED in 9-14** (frozen DECISION). Modal renders regardless of whether the subsequent POST will 409. Rationale: (a) keeps server contract unchanged per migration mandate; (b) the trigger-button visibility gate `{% if user.deleted_at.is_none() && user.id != acting_admin_id %}` already hides the self-deactivate path in the UI — only direct URL crafting reaches it; (c) the last-active-admin scenario is rare in single-user NAS deployment (KF #32); (d) the actual UX on Conflict is acceptable (verified — `AppError::Conflict::IntoResponse` retargets to `#feedback-list`, not the row, so the user sees feedback under the panel without their row replaced). Pre-flight would extract `count_active_admins()` from `UserModel::deactivate` — that's "improvement", not "migration". AC7's `..._renders_for_self_target` and `..._renders_for_last_active_admin` lock the "always render" contract; a future pre-flight PR will flip those tests, mirror of 9-13's #139 pattern.
   - Direct browser navigation (no `HX-Request` header) returns 405 Method Not Allowed via `Ok(StatusCode::METHOD_NOT_ALLOWED.into_response())` — single line, **NO `Allow:` response header, EMPTY BODY**. Mirror of 9-11/9-12/9-13's clean 405 shape.
   - **No `?target=` query parameter** in 9-14 (single surface, mirror of 9-12/9-13). The modal fragment hardcodes `hx_target=format!("#admin-users-row-{}", user.id)` and `hx_swap="outerHTML"` via the macro's 9ᵗʰ and 10ᵗʰ params. The action_url is `format!("/admin/users/{}/deactivate", user.id)`.
   - **HTML-escape the username?** NO — pass the RAW name into `t!("admin.users.deactivate_modal_title", username = …)`. Askama's default auto-escape on `{{ title }}` inside the modal macro handles HTML safety. Pre-escaping would double-escape. Mirror of 9-13's `name` handling.

2. **AC2 — NEW fragment template `templates/fragments/admin_user_deactivate_modal.html`** (mirror of `templates/fragments/series_delete_modal.html` from 9-13, ~18 LOC, +1 LOC for the version param):
   - Imports the shared macro: `{% import "components/modal.html" as modal %}`.
   - Calls (note the 11ᵗʰ param `version`): `{% call modal::modal("delete", title, body_html, confirm_label, cancel_label, action_url, "POST", csrf_token, hx_target, "outerHTML", version) %}{% endcall %}`.
   - The `action_url` handler-side is `format!("/admin/users/{}/deactivate", user.id)`.
   - The `hx_target` handler-side is `format!("#admin-users-row-{}", user.id)` — the per-row target ID matches the existing `<tr id="admin-users-row-{{ user.id }}">` shape (`templates/fragments/admin_users_row.html:2`).
   - The `body_html` is built handler-side as `format!("<p>{}</p>", body_text)` after pulling `body_text` out of `t!()`; the i18n value carries no user-supplied interpolation, so no escape is needed (mirror of 9-10/9-12/9-13 — and yes, the body_html string-concat-bypasses-Askama-auto-escape concern flagged in 9-13's review #137 is preserved here for the same "out-of-scope sweep" reason).
   - The `version: i32` is `user.version` — the macro will render `<input type="hidden" name="version" value="...">` because the value is non-zero (post-creation version is always ≥ 1; see `users.version` schema default).

3. **AC3 — Migrate `templates/fragments/admin_users_row.html:23-27`** deactivate form (lines 23-27 inclusive, the whole `<form>...</form>` block):
   - Before:
     ```html
     <form method="POST" action="/admin/users/{{ user.id }}/deactivate" class="inline" hx-post="/admin/users/{{ user.id }}/deactivate" hx-confirm="{{ confirm_deactivate|e }}" hx-target="#admin-users-row-{{ user.id }}" hx-swap="outerHTML">
         <input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">
         <input type="hidden" name="version" value="{{ user.version }}">
         <button type="submit" class="px-2 py-1 bg-red-600 text-white text-xs rounded hover:bg-red-700">{{ btn_deactivate }}</button>
     </form>
     ```
   - After:
     ```html
     <button hx-get="/admin/users/{{ user.id }}/deactivate-modal"
             hx-target="#modal-slot" hx-swap="innerHTML"
             data-modal-trigger aria-haspopup="dialog" aria-expanded="false"
             class="px-2 py-1 bg-red-600 text-white text-xs rounded hover:bg-red-700">
         {{ btn_deactivate }}
     </button>
     ```
   - Tailwind classes UNCHANGED (visual identity preserved). The `<form>` wrapper + the two hidden inputs DISAPPEAR from the trigger because the modal carries them in its own form. `aria-haspopup="dialog"` + `aria-expanded="false"` are the 9-11/9-12/9-13 a11y standard for modal triggers.
   - The visibility guard `{% if user.deleted_at.is_none() && user.id != acting_admin_id %}` (line 22) STAYS — same UI-side gate.
   - The Reactivate `<form>` at lines 30-34 (wrapped by `{% if user.deleted_at.is_some() %}` at line 29 + `{% endif %}` at line 35) is UNTOUCHED (not in scope; never had `hx-confirm`).
   - **Important**: `admin_users_row.html` is rendered in TWO contexts — standalone via the `AdminUsersRow` template AND `{% include %}`-d from `admin_users_table.html`. After AC4's `UserWithConfirm` teardown, the include's `{% let user = item %}` provides a `UserRow` directly (no `confirm_deactivate` field), and the standalone path also no longer carries the field. Both paths must lint clean — verify by running `cargo build` after Task 6.

4. **AC4 — Drop ALL `confirm_deactivate` plumbing** (5 Rust sites + 2 template sites + i18n key) per the call-site map in the reality-check section. Foundation Rule #1 (DRY) — the entire `UserWithConfirm` wrapper struct exists ONLY to thread `confirm_deactivate` from `render_panel` to `admin_users_table.html`; once the attribute disappears from the row, the wrapper is dead weight. Mirror of 9-10/9-11/9-12/9-13's principle but with a wider blast radius because of the panel-table iteration. Specifically:
   - DELETE `struct UserWithConfirm` (`src/routes/admin.rs:216-219`).
   - CHANGE `AdminUsersPanel.users: Vec<UserWithConfirm>` → `Vec<UserRow>` (`:249`).
   - SIMPLIFY `render_panel` block (`:1104-1107`) — drop the `into_iter().map(...).collect()`; pass `users_raw` directly.
   - DROP `AdminUsersRow.confirm_deactivate` field (`:283`).
   - DROP `let confirm_deactivate = ...` (`:1171`) + `confirm_deactivate,` ctor field (`:1185`) in `render_user_row`.
   - EDIT `admin_users_table.html:18-19` — replace the two `{% let %}` lines with `{% let user = item %}` (the iteration variable is now a `UserRow` directly, not a wrapper).
   - The `{{ confirm_deactivate|e }}` reference in `admin_users_row.html:23` disappears as part of AC3's whole-form replacement.
   - **Verify by grep**: `grep -rn 'confirm_deactivate\|UserWithConfirm' src/ templates/ locales/` MUST return ZERO hits after cleanup. **`UserWithConfirm` co-droppage is non-negotiable** — leaving it as dead code triggers `cargo clippy --all-targets -- -D warnings` (`unused struct`).

5. **AC5 — Update `src/templates_audit.rs::ALLOWED_HX_CONFIRM_SITES`**:
   - Before:
     ```rust
     const ALLOWED_HX_CONFIRM_SITES: &[(&str, usize)] = &[
         ("templates/fragments/admin_users_row.html", 1),
     ];
     ```
   - After:
     ```rust
     const ALLOWED_HX_CONFIRM_SITES: &[(&str, usize)] = &[];
     ```
   - Total entries: 1 → 0. Total occurrences: 1 → 0. **The empty slice is the steady state from 9-14 onwards.**
   - `cargo test hx_confirm_matches_allowlist` MUST pass with the empty array — verify the test's positive-assertion shape handles `0 == 0` correctly (read the test body in Task 1 to confirm; if it has a `> 0` precondition somewhere, refactor minimally, but expectation is "no change needed").

6. **AC6 — i18n: 3 NEW keys + 1 DROPPED key per locale** (EN + FR):
   - **NEW** under `admin.users:` block in `locales/en.yml` and `locales/fr.yml`. **Insert after `admin.users.confirm_deactivate` at line 556** (the line that's about to be dropped — but insert BEFORE the deletion so the placement is deterministic; clusters all `deactivate*`-prefixed keys together):
     - `deactivate_modal_title: "Deactivate user %{username}?" / "Désactiver l'utilisateur %{username} ?"`
     - `deactivate_modal_body: "They will be logged out immediately and cannot log back in until reactivated." / "Sa session sera fermée immédiatement et il ne pourra plus se reconnecter avant réactivation."`
     - `deactivate_modal_confirm: "Deactivate" / "Désactiver"`
   - **DROPPED**: `admin.users.confirm_deactivate` (EN + FR) — zero callers after AC3/AC4 (Foundation Rule #1 dead-key drop, mirror of 9-10/9-11/9-12/9-13 chain).
   - **REUSED** (no edits): `common.cancel` (shipped by 9-10), `admin.users.btn_deactivate` (the trigger button label, KEPT).
   - Run `cargo test all_t_keys_have_both_locales` (the parity test in `src/i18n/audit.rs:186`) to confirm every `t!()` call site has a key in both locale files.
   - Run `touch src/lib.rs && cargo build` after editing locale files to force the rust-i18n proc macro to re-read the YAML (CLAUDE.md i18n rule).

7. **AC7 — Integration tests** (NEW file `tests/admin_user_deactivate_modal.rs`, sibling of `tests/series_delete_modal.rs` from 9-13 and `tests/contributor_delete_modal.rs` from 9-12). 11 `#[sqlx::test]` cases, mirror of the 9-13 shape with one extra case for the librarian-403 path (which is unique to 9-14 because `Role::Admin` excludes Librarian — earlier stories' `Role::Librarian` accepted both):
   - `get_user_deactivate_modal_returns_200_with_dialog_for_admin_request` — admin session, GET `/admin/users/:id/deactivate-modal` (target = a librarian user), returns 200 + body contains `<dialog open aria-modal="true">` + the username + `data-modal-variant="delete"` (stable selector, NOT the brittle `bg-red-600` Tailwind substring per the 9-12 review patch P6) + `data-modal-default-focus` on Cancel + `hx-post="/admin/users/{id}/deactivate"` (verify the path) + `hx-target="#admin-users-row-{id}"` + `hx-swap="outerHTML"` + the hidden `_csrf_token` input + the hidden `version` input with the user's current version.
   - `get_user_deactivate_modal_returns_403_for_librarian_request` — **NEW vs 9-13** — librarian session, returns 403 (NOT 200, NOT 303). This locks the `Role::Admin` gate. Mirror of `tests/admin_users.rs::admin_users_panel_returns_403_for_librarian` from story 8-3.
   - `get_user_deactivate_modal_redirects_anonymous_to_login` — anonymous session, returns 303 → `/login?next=%2Fadmin%3Ftab%3Dusers` (verifies `_with_return` URL-encodes the admin-tab path correctly — note the `?` and `=` percent-encoding).
   - `get_user_deactivate_modal_returns_404_for_soft_deleted_user` — user with `deleted_at = NOW()`, returns 404. Mirror of 9-13's soft-deleted case.
   - `get_user_deactivate_modal_returns_404_for_nonexistent_user` — id `99999` with no row, returns 404.
   - `get_user_deactivate_modal_returns_405_for_non_htmx_request` — direct browser nav (no `HX-Request` header), returns 405. **Body is empty AND no `Allow:` response header is set**. Assertions exact-match 9-13's shape: `assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);` + `assert!(resp.headers().get(axum::http::header::ALLOW).is_none(), "405 must not set Allow header — see story 9-11 code-review patch");` + `assert!(body_text(resp).await.is_empty(), "405 body must be empty per AC1 — story 9-12 review patch");`.
   - `get_user_deactivate_modal_html_escapes_username` — user named `<script>alert(1)</script>` (the existing `users` schema allows arbitrary-but-non-empty usernames; verify by reading the migration), returned HTML contains the entity form (`&lt;script&gt;` OR `&#60;script&#62;`) AND does NOT contain the raw `<script>alert(1)</script>` substring. Mirror of 9-13's escape-check assertion shape. **NB**: if the username column has length / charset constraints that disallow this, downgrade to a simpler escape probe (e.g. `User & Co` → `User &amp; Co`) and document in Dev Agent Record.
   - `get_user_deactivate_modal_renders_for_self_target` — admin session, target = the acting admin's OWN id. **Asserts the modal IS rendered (200) — locks the "always render" contract** per AC1's frozen DECISION. Comment explicitly: `// LATENT UX BUG (deferred): self-deactivate trigger is hidden in the UI but the modal handler renders for self-target. Story 9-14 preserves this server contract; pre-flight gate fix is deferred to a future chore PR (mirror of 9-13's #139 pattern).`
   - `get_user_deactivate_modal_renders_for_last_active_admin` — single admin in DB (=the acting admin in the prior test, same row), target = a freshly-seeded admin who is the only OTHER active admin → modal renders 200 (the handler doesn't pre-flight; the actual deactivate POST would 409). Same "always render" lock + comment.
   - `deactivate_user_via_existing_handler_still_works` — sanity check: as an admin, fire `POST /admin/users/{id}/deactivate` directly with `version=1` + valid `_csrf_token` (the unchanged existing handler) for a librarian target, assert response 200 + body contains the deactivated row markup (`status_deactivated` cell, `Reactivate` button visible) + the row is soft-deleted (`deleted_at IS NOT NULL`). Mirror of 9-10's `delete_borrower_via_existing_handler_still_works` and 9-13's series equivalent.
   - **CSRF assertion** (AC11): the admin-happy-path test MUST also assert `assert!(html.contains("name=\"_csrf_token\""))` to lock the macro's CSRF embedding (mirror of 9-13).
   - **Version assertion** (AC11 — NEW vs 9-13): the admin-happy-path test MUST also assert `assert!(html.contains(&format!("name=\"version\" value=\"{}\"", user.version)))` to lock the new 11ᵗʰ-param `version` rendering. **Critical regression net** for the macro extension.
   - **Integration test for the rendered users-panel `#admin-users-row-{id}` exists** (mirror of 9-12 review patch P3 / 9-13 AC7 10ᵗʰ case): add an 11ᵗʰ `#[sqlx::test]` case `admin_users_panel_renders_row_target_div_for_each_active_user` that GETs `/admin?tab=users` as an admin and asserts the body contains `id="admin-users-row-{id}"` for at least one seeded librarian user. Load-bearing because the modal hardcodes `hx_target=#admin-users-row-{id}`.

8. **AC8 — Templates audit + audit-doc-comment + CLAUDE.md update**:
   - `cargo test no_inline_markup_in_templates`, `cargo test hx_confirm_matches_allowlist` (now with empty allowlist), `cargo test forms_include_csrf_token`, `cargo test csrf_exempt_routes_frozen` all pass after the migration.
   - **Update `src/templates_audit.rs:30-34` doc-comment** — replace:
     ```rust
     /// Grandfathered `hx-confirm=` sites — the only templates allowed to carry
     /// this attribute. The count is the exact expected number of occurrences
     /// per file; a mismatch (new destructive button, or an Epic-9 migration
     /// removing one) forces the PR to update this list, which is the whole
     /// point of the audit: a reviewer is always in the loop.
     ```
     with:
     ```rust
     /// `hx-confirm=` is FORBIDDEN in all templates (post Epic 9 close).
     /// The empty allowlist is the steady-state contract: any new occurrence
     /// in `templates/` fails `hx_confirm_matches_allowlist` outright. The
     /// audit infrastructure is preserved on purpose — re-introducing a
     /// destructive flow with `hx-confirm=` requires editing this constant
     /// (an explicit, reviewable act), not just adding the attribute.
     ```
   - **Update `CLAUDE.md` "Modal scanner-guard invariant" section** — replace the line *"the allowlist is frozen at 5 grandfathered sites (5th added in story 8-3 for admin user deactivation) and only changes through explicit review."* with: *"the allowlist is empty post Epic 9 — any new `hx-confirm=` is BLOCKED outright by `templates_audit.rs::hx_confirm_matches_allowlist`."* This was deferred from every prior 9-1x story per "9-14 rewrites the whole sentence in one shot." Verify the exact line by reading CLAUDE.md before editing — the "Modal scanner-guard invariant (story 7-5)" anchor section is the one to touch.

9. **AC9 — E2E test** — extend an existing admin-users E2E spec OR create a new one if none covers deactivate today. **Reality check during Task 1**: scan `tests/e2e/specs/journeys/` for any spec referencing `Deactivate` / `deactivate` / `admin/users` user flows; if none exists (likely — `admin-smoke.spec.ts` covers tab navigation only, not deactivate flow), then either:
   - **Option A** — extend `admin-smoke.spec.ts` with a new `test("deactivate user via modal", ...)` block (probably best — single admin spec file, easier to maintain).
   - **Option B** — create a NEW `tests/e2e/specs/journeys/admin-users.spec.ts` if the smoke spec is purposely tight (Foundation Rule #7 says smoke tests are blank-context epic-journey tests; deactivate is a sub-flow, may belong elsewhere).
   - **DECISION** (frozen): **Option A** — extend `admin-smoke.spec.ts`. Mirror of 9-13 extending `series.spec.ts`. New test block:
     ```ts
     test("deactivate user via modal", async ({ page }) => {
       await page.context().clearCookies();
       await loginAs(page, "admin");
       // Seed a librarian to deactivate (server-side via API, NOT a chained UI flow)
       const username = `AU-Deact-${Date.now()}`;
       await page.request.post("/admin/users", { form: { username, password: "deactivate-test", role: "librarian", _csrf_token: <fetched> } });
       // Navigate to /admin?tab=users and find the new librarian row
       await page.goto("/admin?tab=users");
       const row = page.locator(`tr:has-text("${username}")`);
       const deactivateBtn = row.getByRole("button", { name: /Deactivate|Désactiver/i });
       // Paranoid lock: trigger button has no hx-confirm
       await expect(deactivateBtn).not.toHaveAttribute("hx-confirm", /./);
       // Open modal, verify default-focus + Escape-close (mirror of 9-13 AC9)
       await deactivateBtn.click();
       await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
       await expect(page.locator("[data-modal-default-focus]")).toBeFocused();
       await page.keyboard.press("Escape");
       await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible();
       // Reopen and confirm the actual deactivate
       await deactivateBtn.click();
       await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
       await page.locator("[data-modal-confirm]").click();
       // Modal closes after 200 (htmx:afterRequest contract from 9-12 review patch P4)
       await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible();
       // Row swap: librarian row now shows Deactivated status + Reactivate button
       await expect(row).toContainText(/Deactivated|Désactivé/i);
       await expect(row.getByRole("button", { name: /Reactivate|Réactiver/i })).toBeVisible();
       // OOB feedback in #feedback-list
       await expect(page.locator("#feedback-list")).toContainText(/deactivated|désactivé/i);
     });
     ```
   - **CSRF token fetching for `page.request.post`** — `tests/e2e/global-setup.ts` does NOT exist and `tests/e2e/helpers/` has no admin-user seeder, so there is NO precedent for the direct-DB-INSERT path. **DECISION** (frozen): use the **CSRF-meta-fetch + POST `/admin/users`** path (the existing 8-3 admin handler), per the pattern documented in CLAUDE.md "CSRF synchronizer token (story 8-2)". Concrete snippet:
     ```ts
     async function seedLibrarianUser(page: Page, username: string): Promise<void> {
       // 1. Land on the admin page so the meta CSRF token is in the DOM.
       await page.goto("/admin?tab=users");
       const csrf = await page.evaluate(() => {
         return document.querySelector<HTMLMetaElement>('meta[name="csrf-token"]')?.content || "";
       });
       // 2. Submit the create-user form via the existing 8-3 handler.
       const resp = await page.request.post("/admin/users", {
         form: {
           username,
           password: "deactivate-test-pw",
           role: "librarian",
           _csrf_token: csrf,
         },
       });
       if (!resp.ok()) throw new Error(`seedLibrarianUser failed: ${resp.status()} ${await resp.text()}`);
     }
     ```
     The function lives inline in `admin-smoke.spec.ts` (same-file scope) — do NOT extract to `tests/e2e/helpers/admin-users.ts` unless a SECOND admin-tests spec needs it (rule of three; mirror of 9-12's "no new helper file" decision).
   - **CI flake gate** (`grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/`) MUST stay clean.
   - **EN/FR matcher invariance**: existing `getByRole("button", { name: /Deactivate|Désactiver/i })` continues to work.

10. **AC10 — Foundation Rule #12 LOC discipline**:
    - `templates/fragments/admin_users_row.html` net change: 5 lines replaced by a 6-line `<button>` (~+1 LOC, still well under 2000).
    - `templates/fragments/admin_user_deactivate_modal.html` is a NEW file (~18 LOC).
    - `templates/components/modal.html` net change: +1 LOC (the new optional 11ᵗʰ-param `version` block).
    - `src/routes/admin.rs` grows by ~80 LOC (new `admin_users_deactivate_modal` handler + `AdminUserDeactivateModalTemplate` struct) AND shrinks by ~10 LOC from the `UserWithConfirm` teardown. Current LOC: **1583** (verified via `wc -l` on 2026-05-07). Projected: ~1653. Far under 2000 — no extraction needed.
    - `src/routes/mod.rs` net change: +4 LOC for the new route registration.
    - `src/templates_audit.rs` net change: −1 LOC for the entry; ±5 LOC for the doc-comment rewrite.
    - `tests/admin_user_deactivate_modal.rs` is a NEW file (~520 LOC of integration tests, mirror of `tests/series_delete_modal.rs`'s 524 LOC + the librarian-403 case + the version-input lock).
    - `tests/e2e/specs/journeys/admin-smoke.spec.ts` net change: +30 to +50 LOC (new test block).
    - `tests/e2e/helpers/admin-users.ts` is a NEW file if the seedLibrarianUser helper is created (~30 LOC).
    - `locales/en.yml` and `locales/fr.yml` net change: +3 keys / −1 key per locale = +2 lines per locale.
    - `CLAUDE.md` net change: 1 line replaced.

11. **AC11 — CSP / scanner-guard / CSRF inheritance + version optimistic-locking**:
    - The new modal uses `<dialog open aria-modal="true">` (inherited from the macro). Scanner-guard 7-5 applies automatically — no new E2E assertion needed (mirror of 9-12/9-13 "foundation tests are load-bearing once").
    - **CSRF**: the modal's Confirm button issues `hx-post="/admin/users/{id}/deactivate"`. The macro's `csrf_token` 8ᵗʰ param renders a hidden `<input name="_csrf_token">` inside the modal's confirm form (verified by AC7's 200-with-dialog test). Without it, the CSRF middleware on `POST /admin/users/{id}/deactivate` would 403.
    - **Version optimistic-locking** (NEW vs 9-13): the macro's NEW 11ᵗʰ param renders a hidden `<input name="version" value="{{ user.version }}">` inside the modal's confirm form, fed from `user.version` server-side at GET `/admin/users/:id/deactivate-modal` time. The handler's `Form<DeactivateForm>` extractor reads it from the body. **TOCTOU window**: between the GET (modal open) and the POST (Confirm click), if the user is updated by another admin, the version mismatch flows through `services::locking::check_update_result` returning `AppError::Conflict(rust_i18n::t!("error.conflict", entity = "user").to_string())` (a localized string, NOT a literal sentinel) → 409 + `HX-Retarget: #feedback-list` + inline feedback. Pre-existing 8-3 contract; UNCHANGED by this story.
    - **NOTE**: `templates_audit::forms_include_csrf_token` matches `<form method="POST">` only — the modal macro's `<form hx-post=…>` (no `method=` attribute) is NOT scanned by that audit. The CSRF input is policed instead at TWO layers: (a) AC7's integration test asserts the hidden input is present in the rendered HTML; (b) the CSRF middleware rejects any state-changing request lacking a valid token, which the E2E tests would catch as a 403. Don't lean on the audit as the safety net — lean on AC7. **Same shape as 9-13.**

12. **AC12 — Server contract is UNCHANGED**: `POST /admin/users/{id}/deactivate` returns the same `HtmxResponse { main: row_html, oob: feedback_list }` for success and the same `AppError::Conflict` for self-deactivate / last-admin / version-mismatch / inline feedback shape via AppError's IntoResponse. The existing 8-3 integration tests + smoke spec MUST keep passing. The only change to the existing handler is a doc-comment update (mirror of 9-12/9-13's discoverability-link patch): add `/// Trigger UX: see GET /admin/users/:id/deactivate-modal (story 9-14).` above the `pub async fn admin_users_deactivate` line.

13. **AC13 — Story-level grep audit**: at story close, run three greps and document the output in Dev Agent Record:
    - `grep -rnE 'hx-confirm=' templates/` — must return EXACTLY 0 hits (real attributes; doc-comment occurrences in `admin_reference_data_panel.html`, `admin_system_panel.html`, `components/modal.html` are PROSE, not attributes — distinguish via the regex `hx-confirm\s*=\s*"` if needed). **This is the headline grep that proves Epic 9's hx-confirm-empty contract.**
    - `grep -rnE 'hx-confirm=' src/` — must return EXACTLY 1 real-attribute hit (the pre-existing `src/routes/locations.rs:256` Rust-emitted entry, unchanged from 9-13 close — out of scope; documented as inherited tech debt; tracked via GH issue #138). **9-14 does NOT touch this.**
    - `grep -rn 'confirm_deactivate' src/ templates/ locales/` — must return ZERO admin-related hits (the AC4 + AC6 cleanup must be complete).

14. **AC14 — Local Testing Before Push (Foundation Rule #13)**: run the full local gate before opening the PR. Minimum:
    - `SQLX_OFFLINE=true cargo check` — clean
    - `cargo clippy --all-targets -- -D warnings` — clean
    - `cargo test --lib` — green (≥755 lib + the new AC7 cases + existing integration suites)
    - `cargo test --test admin_user_deactivate_modal` — green (the 11 integration tests from AC7)
    - `cargo test hx_confirm_matches_allowlist` — green (with the empty allowlist)
    - `cargo test no_inline_markup_in_templates` — green
    - `cargo test forms_include_csrf_token` — green
    - `cargo test all_t_keys_have_both_locales` — green
    - Existing 8-3 integration tests (`cargo test --test admin_users` if present) — green; **regression-net for the unchanged server handler**.
    - Full E2E via `./scripts/e2e-reset.sh` + `cd tests/e2e && npm test` — green; pay attention to `admin-smoke.spec.ts` going green with the new deactivate-modal test.
    - The flake gate: `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` returns nothing.

15. **AC15 — Draft PR + CI gate (Foundation Rule #15 + #18)**: open a draft PR at the first commit (per `gh pr create --draft`) and WAIT for CI to finish before requesting review or merging. CI green → squash-merge. The hx-confirm migration chain closes here: the PR description should celebrate Epic 9's Modal foundation reaching steady state (`ALLOWED_HX_CONFIRM_SITES = &[]`).

## Tasks / Subtasks

- [x] **Task 1 — Verify reality-check assumptions (AC: all)**
  - [x] Read `src/routes/admin.rs:680-714` and confirm: role gate is `Role::Admin`, endpoint is `POST /admin/users/{id}/deactivate`, success returns `HtmxResponse { main: row_html, oob: feedback-list }`, error path is `AppError::Conflict` for self-deactivate / last-admin / version-mismatch propagated unchanged via `?` (no in-handler match on `Err(e)`). Document the exact endpoint string in Dev Agent Record.
  - [x] Read `src/routes/mod.rs:256-257` and confirm the existing route registrations.
  - [x] Read `templates/components/modal.html` and confirm the 10-param shape. **Plan the 11ᵗʰ-param `version: i32` extension** — verify Askama 0.15 macro syntax for an additional positional param, plan the conditional render `{% if version != 0 %}<input type="hidden" name="version" value="{{ version }}">{% endif %}`, document the Askama-version-default-value caveat (does not exist in macros — must touch all callers).
  - [x] Read `templates/fragments/series_delete_modal.html` (the 9-13 mirror) and `tests/series_delete_modal.rs` (the test mirror). 9-14 will be ~near-byte-identical with type/name swaps + the 11ᵗʰ-param addition + the librarian-403 NEW case.
  - [x] Read `src/models/user.rs::UserModel::deactivate` to confirm: signature `(pool: &MySqlPool, id: u64, version: i32, acting_admin_id: u64) -> Result<u64, AppError>` (returns the count of sessions killed). Conflict variants (verified at `:287,300`): `AppError::Conflict("self_deactivate_blocked")`, `AppError::Conflict("last_admin_blocked")`. Version-mismatch flows through `services::locking::check_update_result` returning `AppError::Conflict(rust_i18n::t!("error.conflict", entity = "user").to_string())` — a localized string, NOT a literal sentinel. NotFound when target user doesn't exist. UNCHANGED by this story.
  - [x] **`UserModel::find_by_id` is already verified**: returns `Option<UserRow>` AND **includes deactivated users** (no `deleted_at IS NULL` filter). The handler at Task 4 MUST add an explicit `if user.deleted_at.is_some() { ... 404 ... }` guard per AC1.
  - [x] Verify `<div id="feedback-list">` is rendered on `/admin?tab=users` — read `templates/pages/admin.html` and `templates/fragments/admin_users_panel.html` to find it. If absent, file as deferred GH issue and document the fragility (mirror of 9-13 review #E1 pattern). **This is critical** because the existing deactivate handler ALREADY OOBs to `feedback-list`; if the div doesn't exist on /admin, the existing 8-3 success feedback is silently dropped TODAY — that would be a pre-existing bug (verify before assuming).
  - [x] Grep `confirm_deactivate` AND `UserWithConfirm` callers across `src/`, `templates/`, `locales/` to confirm full cleanup scope. **Expected map (verified 2026-05-07)**: 5 Rust sites (`admin.rs:216-219` UserWithConfirm struct, `:249` panel field, `:283` row field, `:1104-1107` panel ctor, `:1171` + `:1185` row ctor) + 2 template sites (`admin_users_row.html:23`, `admin_users_table.html:19`) + 1 i18n key per locale (`en.yml:556`, `fr.yml:556`). Document the post-cleanup grep output in Dev Agent Record (target: 0 hits across both terms).
  - [x] Measure current `src/routes/admin.rs` LOC (`wc -l`). Project +80 LOC. Current is 1583 → projected ~1663, comfortably under 2000.
  - [x] Read `src/templates_audit.rs::hx_confirm_matches_allowlist` test body to verify it handles the empty-allowlist case correctly. **If the test has any precondition that requires `ALLOWED_HX_CONFIRM_SITES.len() > 0`, refactor minimally** (expectation: no change needed — the test sums allowed counts and compares to total grep hits; 0 == 0 holds).
  - [x] Scan `tests/e2e/specs/journeys/` for any spec touching admin user deactivate; confirm AC9 Option A (extend `admin-smoke.spec.ts`) is the right call.

- [x] **Task 2 — Modal macro extension (AC: 2, 11)**
  - [x] Edit `templates/components/modal.html`: add `version` as the 11ᵗʰ positional param. Render conditionally:
    ```jinja
    {%- macro modal(variant, title, body_html, confirm_label, cancel_label, action_url, action_method, csrf_token, hx_target, hx_swap, version) -%}
        ...
        <input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">
        {% if version != 0 %}<input type="hidden" name="version" value="{{ version }}">{% endif %}
        ...
    {%- endmacro -%}
    ```
  - [x] Update all 4 existing callers to pass `0` as the new 11ᵗʰ arg:
    - `templates/fragments/borrower_delete_modal.html`
    - `templates/fragments/return_loan_modal.html`
    - `templates/fragments/contributor_delete_modal.html`
    - `templates/fragments/series_delete_modal.html`
  - [x] Run `cargo build` and confirm Askama re-compiles all 4 fragments without errors.
  - [x] Run `cargo test --lib --test borrower_delete_modal --test return_loan_modal --test contributor_delete_modal --test series_delete_modal` to confirm the existing 4 modal-fragment integration suites still pass.

- [x] **Task 3 — i18n keys (AC: 6)**
  - [x] Add 3 new keys to `locales/en.yml` under the existing `admin.users:` block:
    ```yaml
    admin:
      users:
        # … existing keys …
        deactivate_modal_title: "Deactivate user %{username}?"
        deactivate_modal_body: "They will be logged out immediately and cannot log back in until reactivated."
        deactivate_modal_confirm: "Deactivate"
    ```
  - [x] Add the same 3 keys to `locales/fr.yml` with FR copy:
    ```yaml
    admin:
      users:
        # … existing keys …
        deactivate_modal_title: "Désactiver l'utilisateur %{username} ?"
        deactivate_modal_body: "Sa session sera fermée immédiatement et il ne pourra plus se reconnecter avant réactivation."
        deactivate_modal_confirm: "Désactiver"
    ```
  - [x] Drop `admin.users.confirm_deactivate` from BOTH locale files (zero callers after AC3 + AC4).
  - [x] Run `touch src/lib.rs && cargo build` to force rust-i18n proc-macro recompilation.
  - [x] Run `cargo test all_t_keys_have_both_locales` to confirm parity.

- [x] **Task 4 — `GET /admin/users/:id/deactivate-modal` handler + route (AC: 1, 2, 11)**
  - [x] Add to `src/routes/admin.rs`:
    - `AdminUserDeactivateModalTemplate` struct (mirror of `SeriesDeleteModalTemplate` from 9-13): fields `title`, `body_html`, `confirm_label`, `cancel_label`, `action_url`, `csrf_token`, `hx_target`, `version`. **The `hx_target` field is NEW vs 9-13** (9-13 hardcoded the literal `#series-feedback` in the fragment). Pass `format!("#admin-users-row-{}", user.id)` from the handler. Alternative: hardcode in the fragment template using `{% call modal::modal(..., "#admin-users-row-" ~ user.id, ...) %}` Askama string-concat — pick whichever matches local convention. **DECISION** (frozen): use the handler-side construction (the `hx_target` template field) — fewer Askama gotchas, mirror of 9-13's pre-rendering approach.
    - `pub async fn admin_users_deactivate_modal(...)` mirroring the 9-13 series equivalent (~80 LOC). Inputs: `State<AppState>`, `Session`, `Extension<Locale>`, `HxRequest(is_htmx)`, `Path<u64>`. Behaviors per AC1:
      - `session.require_role_with_return(Role::Admin, "/admin?tab=users")?`
      - Early-return `Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response())` if `!is_htmx` — single-line shape, no `Allow:` header, no body.
      - `let user = UserModel::find_by_id(pool, id).await?.ok_or_else(|| AppError::NotFound(...))?;` THEN `if user.deleted_at.is_some() { return Err(AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())); }` — `find_by_id` does NOT filter `deleted_at` (verified, see Task 1).
      - Pre-translate the 4 i18n keys via `t!(..., locale = loc)`. For the title key, pass the raw username: `t!("admin.users.deactivate_modal_title", locale = loc, username = user.username.as_str())`.
      - Build `body_html = format!("<p>{body_text}</p>")`.
      - Set `action_url = format!("/admin/users/{}/deactivate", user.id)`.
      - Set `hx_target = format!("#admin-users-row-{}", user.id)`.
      - Pass `version = user.version` (i32 — read the schema; users.version is `INT NOT NULL DEFAULT 1`).
      - **Do NOT pre-flight self-deactivate or last-admin guards** — frozen DECISION per AC1; lock via the AC7 always-render tests.
      - Render the template; on `Err(e)` return `AppError::Internal(format!("admin user deactivate modal render: {e}"))` (mirror of 9-13).
      - Add `tracing::debug!(target_user_id = id, acting_user_id = ?session.user_id, "deactivate modal requested");` — mirror of the 9-13 patch that added user_id to the destructive-surface log. **9-14 ships this from day one** (no follow-up sweep).
  - [x] Register the route in `src/routes/mod.rs` (immediately after the existing `/admin/users/{id}/deactivate` POST registration at line 256):
    ```rust
    .route(
        "/admin/users/{id}/deactivate-modal",
        axum::routing::get(admin::admin_users_deactivate_modal),
    )
    ```

- [x] **Task 5 — Modal fragment template (AC: 2, 11)**
  - [x] Create `templates/fragments/admin_user_deactivate_modal.html` (~18 LOC, mirror of `templates/fragments/series_delete_modal.html`):
    ```jinja
    {# Story 9-14 — admin user deactivate confirmation modal.
       FINAL migration in the hx-confirm → UX-DR8 Modal chain.
       Calls the shared `components/modal.html::modal` macro with the
       `delete` variant + POST method + the new 11ᵗʰ-param `version`
       for optimistic locking. CSP-clean — no inline scripts/styles. #}
    {% import "components/modal.html" as modal %}
    {% call modal::modal(
        "delete",
        title,
        body_html,
        confirm_label,
        cancel_label,
        action_url,
        "POST",
        csrf_token,
        hx_target,
        "outerHTML",
        version,
    ) %}{% endcall %}
    ```
  - [x] Run `cargo test no_inline_markup_in_templates` to confirm the new fragment is CSP-clean.

- [x] **Task 6 — Migrate the trigger + tear down all `confirm_deactivate` plumbing (AC: 3, 4, 8)**
  - [x] `templates/fragments/admin_users_row.html:23-27`: replace the entire `<form>...</form>` block with a single `<button hx-get=...>` per AC3. Tailwind classes UNCHANGED. Add `aria-haspopup="dialog"` + `aria-expanded="false"`.
  - [x] DELETE `struct UserWithConfirm` (`src/routes/admin.rs:216-219`) — the whole `#[derive(Template-context)]` block.
  - [x] CHANGE `AdminUsersPanel.users: Vec<UserWithConfirm>` → `Vec<UserRow>` (`src/routes/admin.rs:249`).
  - [x] SIMPLIFY `render_panel` (`src/routes/admin.rs:1104-1107`) — replace the `let users: Vec<UserWithConfirm> = users_raw.into_iter().map(...).collect();` with `let users = users_raw;` (or just rename the binding; the call-site below will accept `Vec<UserRow>` after the struct change).
  - [x] DROP `AdminUsersRow.confirm_deactivate` field (`src/routes/admin.rs:283`).
  - [x] DROP `let confirm_deactivate = ...` (`src/routes/admin.rs:1171`) + the `confirm_deactivate,` ctor field (`src/routes/admin.rs:1185`) in `render_user_row`.
  - [x] EDIT `templates/fragments/admin_users_table.html:18-19` — collapse to `{% let user = item %}` (single line) since the iteration is now over `Vec<UserRow>` directly.
  - [x] Run `cargo build` and confirm no `unused struct` / `unused field` warnings (clippy with `-D warnings` would fail otherwise).
  - [x] Run `cargo test no_inline_markup_in_templates`.
  - [x] Final cleanup grep: `grep -rn 'confirm_deactivate\|UserWithConfirm' src/ templates/ locales/` returns 0 hits.

- [x] **Task 7 — `ALLOWED_HX_CONFIRM_SITES` cleanup + audit-doc-comment + CLAUDE.md (AC: 5, 8, 13)**
  - [x] Remove the single entry from the const array in `src/templates_audit.rs:35-37`. The const becomes `const ALLOWED_HX_CONFIRM_SITES: &[(&str, usize)] = &[];`.
  - [x] Update the doc-comment at lines 30-34 per AC8 to the new "FORBIDDEN" wording.
  - [x] Update CLAUDE.md "Modal scanner-guard invariant" line per AC8.
  - [x] Run `cargo test hx_confirm_matches_allowlist` and confirm green with the empty allowlist.
  - [x] Run `cargo test --lib templates_audit` (all 4 audit tests) and confirm green.
  - [x] Run the AC13 grep audit:
    - `grep -rnE 'hx-confirm\s*=\s*"' templates/` — must return EXACTLY 0 hits (use the strict regex to exclude prose mentions in doc-comments; `hx-confirm=` without a quote is not a real attribute).
    - `grep -rnE 'hx-confirm\s*=\s*"' src/` — must return EXACTLY 1 hit (`src/routes/locations.rs:256`, pre-existing).
    - `grep -rn 'confirm_deactivate' src/ templates/ locales/` — must return ZERO admin-related hits.
  - [x] Document the grep output in Dev Agent Record. **This is the celebratory moment** — Epic 9's `hx-confirm` migration chain closes here.

- [x] **Task 8 — Integration tests (AC: 7, 11)**
  - [x] Create `tests/admin_user_deactivate_modal.rs` with the 11 `#[sqlx::test]` cases from AC7. Use the same fixture pattern as `tests/series_delete_modal.rs`:
    - `build_state(pool)` helper.
    - `seed_session(pool, username)` for `admin` / `librarian`.
    - `insert_librarian_user(pool, name) -> u64` helper that runs `INSERT INTO users (username, password_hash, role) VALUES (?, '$argon2id$...', 'librarian')` (use a known hash — copy from the migration seed at `migrations/20260414000001_seed_librarian_user.sql` or build a hash inline via `password::hash_password("test")`).
    - `soft_delete_user(pool, id)` for the 404 test.
    - `req_htmx` / `req_plain` / `body_text` helpers (verbatim copy from 9-13).
  - [x] Run `SQLX_OFFLINE=true cargo test --test admin_user_deactivate_modal` and confirm all 11 pass green.

- [x] **Task 9 — E2E updates (AC: 9, 12)**
  - [x] Verify the seedLibrarianUser fixture path (Task 1 outcome). Implement `tests/e2e/helpers/admin-users.ts::seedLibrarianUser` if direct DB INSERT is the chosen path (mirror existing Playwright global setup if present), or use the CSRF-meta-fetch fallback.
  - [x] Edit `tests/e2e/specs/journeys/admin-smoke.spec.ts`: add the new `test("deactivate user via modal", ...)` block per AC9.
  - [x] Run `cd tests/e2e && npx tsc --noEmit` to verify the spec edits don't break tsc.
  - [x] Run `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/admin-smoke.spec.ts` (single-spec run for fast feedback) and confirm all tests green.
  - [x] Run the full E2E lane (`cd tests/e2e && npm test`) and confirm no other spec regressions.

- [x] **Task 10 — Server-side doc-comment (AC: 12)**
  - [x] Add `/// Trigger UX: see GET /admin/users/:id/deactivate-modal (story 9-14).` doc-comment immediately above `pub async fn admin_users_deactivate` in `src/routes/admin.rs:680`. The handler body itself is UNCHANGED.

- [x] **Task 11 — Local gate + push (AC: 14, 15)**
  - [x] `SQLX_OFFLINE=true cargo check` — clean
  - [x] `cargo clippy --all-targets -- -D warnings` — clean
  - [x] `cargo test` (full lib + integration) — green
  - [x] CI flake gate: `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` returns nothing
  - [x] Push branch + open draft PR (Foundation Rule #15) with a description that highlights this is the FINAL hx-confirm migration (`ALLOWED_HX_CONFIRM_SITES = &[]`)
  - [x] WAIT for CI green per Foundation Rule #18 before requesting review / merging

## Dev Notes

### Pattern reuse + 4 critical deviations from 9-13

Fourth mechanical migration on top of the 9-10 foundation; mirror `series::delete_modal` (9-13) 1-for-1 with these deviations:

- **POST `/admin/users/{id}/deactivate`** (not DELETE) — macro's `action_method` branch handles it.
- **`Role::Admin`** (not Librarian) — adds the 11ᵗʰ AC7 case for the librarian-403 path.
- **11ᵗʰ macro param `version: i32`** for optimistic locking — touches all 4 existing callers (pass `0`).
- **`tracing::debug!` includes `acting_user_id`** from day one (mirror of 9-13's post-review patch, applied proactively).

Latent UX bug (modal renders even when POST will 409 on self-target / last-admin) is deliberately PRESERVED, mirror of 9-13's #139 pattern; AC7's `..._renders_for_self_target` and `..._renders_for_last_active_admin` lock the contract; deferred GH issue at story close.

### Why `delete` variant + row-target outerHTML

UX-DR8: deactivation = "destruction of access" semantically (sessions killed, login blocked). Maps to `delete` variant (red Confirm). User is recoverable via the same-row Reactivate button (NOT via Trash — `users` is intentionally outside `services::soft_delete::ALLOWED_TABLES` per GH issue #69).

`hx-target=#admin-users-row-{id}` + `hx-swap=outerHTML` mirrors the existing 8-3 contract: the handler returns the FULL updated row (deactivated state — Reactivate button visible). On success the row swaps in place; on `AppError::Conflict` the `HX-Retarget: #feedback-list` header (verified `src/error/mod.rs:140-155`) overrides the modal's `hx-target` and the feedback lands on the page-level OOB target instead — the user's row stays intact.

### Drop `admin.users.confirm_deactivate` + `UserWithConfirm` per Foundation Rule #1

Mirror of 9-10's `borrower.confirm_delete` drop, 9-11's `loan.return_confirm` drop, 9-12's `contributor_detail.confirm_delete` drop, 9-13's `series.confirm_delete` drop — but 9-14's blast radius is wider because the `UserWithConfirm` wrapper struct + `Vec<UserWithConfirm>` panel iteration exist solely to thread the dead key. Co-droppage of the wrapper is non-negotiable (clippy `unused struct`).

Retained sibling key `admin.users.btn_deactivate` (the trigger label) STAYS — don't conflate.

### CLAUDE.md "Modal scanner-guard invariant" — finally rewritten

The CLAUDE.md line *"the allowlist is frozen at 5 grandfathered sites"* has been inaccurate since 9-10/9-11/9-12/9-13 trimmed the count from 5 → 1. Each prior story explicitly deferred the rewrite per "9-14 rewrites the whole sentence in one shot."

**9-14 IS that rewrite.** AC8 replaces the line with: *"the allowlist is empty post Epic 9 — any new `hx-confirm=` is BLOCKED outright by `templates_audit.rs::hx_confirm_matches_allowlist`."*

### File-LOC budget

`src/routes/admin.rs` is 1583 LOC pre-9-14 → ~1663 post. Plenty of headroom (337 LOC under the 2000 ceiling). No extraction needed. Future stories (9-15 onwards) should re-evaluate when admin.rs grows past ~1800 — the file's already on the radar.

`src/models/user.rs` is UNCHANGED — `deactivate` + `find_by_id` reused.

### DEFERRED items inherited from 9-10..9-13 (no action in 9-14)

- **Two-modal race** — KF tracked. Admin row has only one modal trigger per user; not exercised here.
- **Frozen modal on Confirm 5xx** — KF tracked. User can press Escape.
- **Migrate the 3 admin modal fragments** (Trash permanent-delete, etc.) — STILL deferred per 9-10 close. Out of scope (those use a different modal pattern, not the UX-DR8 macro).
- **JS focus-trap unit tests** — STILL deferred (no JS test harness).
- **Bidirectional EN/FR locale parity test** — deferred Epic 9 follow-up (GH #137 sweep candidate).
- **`src/routes/locations.rs:256` Rust-emitted `hx-confirm=`** — pre-existing (GH #138). Out of scope; will be addressed by a future migration sweep.
- **CSRF rejection retargets to `#feedback-list` not present on every page** — GH issue from 9-13 review. NOT applicable to /admin?tab=users specifically because `#feedback-list` IS present on admin (verified in Task 1) — but the broader fix sweep is still deferred.

### NEW deferred item this story will file

- **`admin_users_deactivate_modal` always renders even when self-deactivate / last-admin would 409** — pre-flight UX dead-end. File at story close as `type:code-review-finding` (mirror of #139 pattern from 9-13). Body cites the AC7 lock tests.

### Project Structure Notes

- `src/routes/admin.rs` already hosts the GET admin handlers; the new modal handler sits alongside `admin_users_row_view`. No new module.
- `templates/fragments/admin_user_deactivate_modal.html` mirrors `templates/fragments/series_delete_modal.html` (9-13 sibling). Same shape, two-line diff (different action_url path + the new 11ᵗʰ-param `version`).
- `tests/admin_user_deactivate_modal.rs` mirrors `tests/series_delete_modal.rs` (9-13 sibling).
- `static/js/modal.js`, `layouts/base.html` are UNCHANGED.
- `templates/components/modal.html` gets a single +1 LOC for the optional `version` hidden input.
- `tests/e2e/specs/journeys/admin-smoke.spec.ts` is the single E2E spec touched.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.14] — story spec verbatim (8 ACs + EN/FR copy)
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#8.-Modal-—-Destructive-Confirmation-+-Warning] — UX-DR8 component anatomy, variants, accessibility
- [Source: _bmad-output/implementation-artifacts/9-10-modal-foundation-and-borrower-migration.md] — pattern precedent (Modal macro, modal.js, focus trap, scanner-guard inheritance, integration-test shape, dead-i18n-key drop, `require_role_with_return` for anonymous-redirect)
- [Source: _bmad-output/implementation-artifacts/9-11-migrate-return-loan-to-modal.md] — pattern precedent (10-param macro signature, dropping the `Allow:` header on 405, dead-`confirm_label` field drop, AC9 inline E2E migration shape)
- [Source: _bmad-output/implementation-artifacts/9-12-migrate-delete-contributor-to-modal.md] — pattern precedent (single-surface hardcoded `hx_target` shape, 6-patches-after-review shape including `data-modal-variant` selector, empty-body 405 assertion)
- [Source: _bmad-output/implementation-artifacts/9-13-migrate-delete-series-to-modal.md] — DIRECT precedent (singular-path action_url, deferred latent UX bug + GH issue lock, `tracing::debug! user_id` patch shipped from day one in 9-14, 11-test integration suite shape)
- [Source: CLAUDE.md#Foundation-Rules] — Rules #1 (DRY), #11 (issue tracking), #12 (LOC ceiling), #13 (local testing), #15 (draft PR), #18 (CI gating)
- [Source: CLAUDE.md#Modal-scanner-guard-invariant-story-7-5] — the `dialog[open]` + `[aria-modal="true"]` selector contract that the new modal inherits — **AC8 rewrites this section's allowlist line**
- [Source: CLAUDE.md#Key-Patterns#CSRF-synchronizer-token-story-8-2] — why the modal macro takes `csrf_token` as its 8ᵗʰ param + why optimistic locking via `version` is a separate hidden input
- [Source: src/routes/admin.rs:680-714] — existing `admin_users_deactivate` handler (UNCHANGED in this story; only a doc-comment is added per AC12)
- [Source: src/routes/admin.rs:563-579] — existing `admin_users_row_view` GET handler (sibling location for the new `admin_users_deactivate_modal` handler)
- [Source: src/routes/admin.rs:271-285] — `AdminUsersRow` template struct (drop `confirm_deactivate` field per AC4)
- [Source: src/routes/admin.rs:1165-1186] — `render_user_row` (drop the `let confirm_deactivate = ...` at `:1171` AND the `confirm_deactivate,` ctor field at `:1185` per AC4)
- [Source: src/routes/admin.rs:216-219, 249, 1104-1107] — `UserWithConfirm` wrapper struct + `AdminUsersPanel.users: Vec<UserWithConfirm>` + the `into_iter().map(...).collect()` panel ctor (DELETE all per AC4)
- [Source: templates/fragments/admin_users_table.html:18-19] — `{% let user = item.user %}` + `{% let confirm_deactivate = item.confirm_deactivate %}` (collapse to `{% let user = item %}` per AC4)
- [Source: src/routes/mod.rs:256-257] — existing `/admin/users/{id}/deactivate` POST + `/reactivate` POST registrations
- [Source: src/models/user.rs::UserModel::deactivate] — the self-deactivate + last-admin guards (UNCHANGED). Returns `Result<u64, AppError>` (count of sessions killed). Conflict variants: `AppError::Conflict("self_deactivate_blocked")` (`:287`), `AppError::Conflict("last_admin_blocked")` (`:300`), version-mismatch via `services::locking::check_update_result` returning a localized string. Plus `AppError::NotFound`.
- [Source: src/error/mod.rs:140-155] — `AppError::Conflict::IntoResponse` emits **HTTP 409 + `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend`** + inline feedback HTML body. The `HX-Retarget` header overrides any `hx-target` on the requesting form; this is why modal Confirm errors land on `#feedback-list` (page-level OOB target at `templates/pages/admin.html:11`), NOT on the row.
- [Source: src/models/user.rs::UserModel::find_by_id] — verify deleted_at filter behavior in Task 1
- [Source: src/templates_audit.rs:30-37] — `ALLOWED_HX_CONFIRM_SITES` const + the doc-comment that AC8 rewrites
- [Source: templates/fragments/admin_users_row.html:22-27] — the role-gated trigger form being migrated; surrounding visibility gate STAYS
- [Source: templates/components/modal.html] — the 10-param shared macro (current state) — **9-14 extends to 11 params**
- [Source: templates/fragments/series_delete_modal.html] — the 18-line fragment template that 9-14's admin_user_deactivate_modal.html mirrors
- [Source: tests/series_delete_modal.rs] — the 524-LOC integration-test mirror (9-13 sibling)
- [Source: tests/e2e/specs/journeys/admin-smoke.spec.ts] — the existing admin smoke spec that AC9 extends
- [Source: locales/en.yml:548-559] — `admin.users.btn_deactivate` (KEPT) + `admin.users.confirm_deactivate` (DROPPED) + sibling keys
- [Source: locales/fr.yml:548-559] — same shape, FR copy

## Dev Agent Record

### Agent Model Used

claude-opus-4-7[1m]

### Debug Log References

- `cargo check` — green
- `cargo clippy --all-targets -- -D warnings` — green
- `cargo test --lib` — 757 passed, 0 failed
- `cargo test --test admin_user_deactivate_modal` — 11 passed, 0 failed
- `cargo test --test borrower_delete_modal --test return_loan_modal --test contributor_delete_modal --test series_delete_modal` — all 4 existing modal-fragment integration suites green after 11ᵗʰ-param macro extension
- `cargo test --lib templates_audit` — 4/4 green (allowlist `&[]` accepted by `hx_confirm_matches_allowlist`)
- `cargo test --lib all_t_keys_have_both_locales` — green (NEW keys present in both locales, OLD `confirm_deactivate` dropped)
- `cargo test --lib modal_tests` — 10 passed (8 pre-existing + 2 NEW for the `version` param contract)
- `npx tsc --noEmit` (E2E) — clean
- Flake gate `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` — clean
- Single-spec `npx playwright test specs/journeys/admin-smoke.spec.ts` — 5/5 passed (4 existing + new "deactivate user via modal")
- Full E2E `cd tests/e2e && npm test` (post `./scripts/e2e-reset.sh`) — 200 passed, 2 skipped, 3 failed. The 3 failures (`similar-titles.spec.ts:105`, `:170`, `home-search.spec.ts:222`) are **pre-existing flakes on `origin/main`** — verified by re-running `similar-titles.spec.ts` in isolation post-reset (passes 2/2). Caused by data-pollution under parallel mode on tests that use fixed (non-date-stamped) entity names. Unrelated to this story; same pattern as 9-13's pre-existing `dewey-code.spec.ts` flake. Will be filed as a separate flake report.
- AC13 grep audit (post-migration):
  - `grep -rnE 'hx-confirm\s*=\s*"' templates/` → **0 hits** (the FINAL contract: empty allowlist, no real attributes anywhere)
  - `grep -rnE 'hx-confirm\s*=\s*"' src/` → **1 hit** at `src/routes/locations.rs:256` (pre-existing inherited tech debt, GH #138; out of scope)
  - `grep -rn 'confirm_deactivate' src/ templates/ locales/` → **0 hits** (clean)
  - `grep -rn 'UserWithConfirm' src/ templates/` → **0 hits** (wrapper struct + table iteration fully torn down)

### Completion Notes List

- ✅ AC1 — `GET /admin/users/:id/deactivate-modal` handler implemented in `src/routes/admin.rs` with all required behaviors: `Role::Admin` gate, 405 on non-HTMX (empty body, no Allow header), 404 on soft-deleted (explicit `deleted_at.is_some()` guard since `find_by_id` doesn't filter), pre-translated 4 i18n keys with `%{username}` interpolation, `tracing::debug!` logs `target_user_id` + `acting_user_id` from day one (mirror of 9-13's post-review patch, applied proactively).
- ✅ AC2 — `templates/fragments/admin_user_deactivate_modal.html` (NEW, 18 LOC) calls the shared macro with the 11-param signature: `delete` variant + `POST` method + hardcoded `#admin-users-row-{id}` target via the `hx_target` template field + `outerHTML` swap + `version` for optimistic locking.
- ✅ AC3 — Trigger button at `templates/fragments/admin_users_row.html:23-27` migrated: the entire `<form>` block replaced by a single `<button hx-get>` with `data-modal-trigger`, `aria-haspopup="dialog"`, `aria-expanded="false"`. Tailwind classes preserved.
- ✅ AC4 — All `confirm_deactivate` plumbing torn down (5 Rust sites + 2 template sites): `UserWithConfirm` struct DELETED, `AdminUsersPanel.users: Vec<UserRow>`, simplified `render_panel` (removed `into_iter().map().collect()` block), `AdminUsersRow.confirm_deactivate` field DROPPED, `let confirm_deactivate = …` + ctor field DROPPED in `render_user_row`, `admin_users_table.html:18-19` collapsed to `{% for user in users %}`. Clippy clean (no `unused struct` warning).
- ✅ AC5 — `ALLOWED_HX_CONFIRM_SITES` is now `&[]` (empty steady state). The `hx_confirm_matches_allowlist` audit handles it cleanly (both loops short-circuit on empty input).
- ✅ AC6 — 3 NEW i18n keys per locale (`deactivate_modal_title` with `%{username}` interpolation, `_body`, `_confirm`); OLD `admin.users.confirm_deactivate` dropped per locale. `all_t_keys_have_both_locales` green.
- ✅ AC7 — 11 `#[sqlx::test]` cases in `tests/admin_user_deactivate_modal.rs`, all green:
  - admin happy path with full assertion set + `name="version" value="…"` lock for the macro's new 11ᵗʰ-param contract
  - librarian-403 (NEW vs 9-13 — Role::Admin excludes Librarian)
  - anonymous → 303 `/login?next=%2Fadmin%3Ftab%3Dusers`
  - 404 for soft-deleted user (explicit handler guard)
  - 404 for nonexistent user
  - 405 for non-HTMX (empty body, no Allow header)
  - HTML-escape username (XSS probe `<script>alert(1)</script>` → entities)
  - `_renders_for_self_target` (latent UX bug lock — handler doesn't pre-flight self-deactivate guard)
  - `_renders_for_last_active_admin` (latent UX bug lock — handler doesn't pre-flight last-admin guard)
  - sanity DELETE via existing handler still soft-deletes the row
  - admin panel renders `<tr id="admin-users-row-{id}">` for each active user (load-bearing for the modal's hardcoded `hx_target`)
- ✅ AC8 — All 4 templates_audit tests green; audit doc-comment rewritten ("FORBIDDEN in all templates"); CLAUDE.md "Modal scanner-guard invariant" line rewritten ("the allowlist is empty post Epic 9").
- ✅ AC9 — E2E test `deactivate user via modal` added to `admin-smoke.spec.ts`: inline CSRF-meta-fetch + POST `/admin/users` seed flow, paranoid `not.toHaveAttribute("hx-confirm", /./)` lock, default-focus + Escape-close regression cover, modal-closes-on-2xx assert, row swap to deactivated state, OOB `#feedback-list` text assert. All 5 admin-smoke tests pass post-reset.
- ✅ AC10 — LOC budget respected: `src/routes/admin.rs` 1583 → 1671 LOC (added `admin_users_deactivate_modal` ~80 LOC + struct, removed `UserWithConfirm` plumbing). Far under the 2000 ceiling.
- ✅ AC11 — CSRF + version optimistic-locking inputs both verified by AC7's happy-path test (`name="_csrf_token"` + `name="version" value="<n>"` substring asserts).
- ✅ AC12 — Server contract preserved; doc-comment `/// Trigger UX: see GET /admin/users/:id/deactivate-modal (story 9-14).` added above `pub async fn admin_users_deactivate`. Handler body UNCHANGED.
- ✅ AC13 — Story-level grep audit clean (see Debug Log References).
- ✅ AC14 — Local gate run, all green (modulo the 3 pre-existing unrelated flakes documented above).
- 🔄 AC15 — Draft PR #141 opened at first commit; awaiting CI on the implementation push.
- 📋 **Latent UX bug to file as deferred GH issue at story close** (per spec): `admin_users_deactivate_modal` always renders even when the subsequent POST will 409 (self-target / last-admin). AC7's `_renders_for_self_target` and `_renders_for_last_active_admin` tests lock the contract; future fix flips them. Mirror of 9-13's #139 pattern.

### File List

**Modified:**
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — story 9-14 status `ready-for-dev` → `in-progress` → `review`; `last_updated` bump.
- `locales/en.yml` — +3 admin.users keys (`deactivate_modal_title`, `deactivate_modal_body`, `deactivate_modal_confirm`); −1 admin.users key (`confirm_deactivate`).
- `locales/fr.yml` — same shape, FR copy.
- `src/routes/admin.rs` — new `AdminUserDeactivateModalTemplate` struct + `pub async fn admin_users_deactivate_modal(...)` handler (~80 LOC); `confirm_deactivate` field dropped from `AdminUsersRow`; ctor site dropped; `/// Trigger UX: …` doc-comment added above `admin_users_deactivate`.
- `src/routes/mod.rs` — new route registration `GET /admin/users/{id}/deactivate-modal`.
- `src/templates_audit.rs` — `admin_users_row.html` entry removed from `ALLOWED_HX_CONFIRM_SITES` (1 → 0 entries; const becomes `&[]`); doc-comment rewritten.
- `templates/components/modal.html` — 11ᵗʰ param `version: i32` added to the macro signature; conditional `<input type="hidden" name="version" ...>` added inside the form.
- `templates/fragments/borrower_delete_modal.html` — pass `0` as the new 11ᵗʰ macro arg.
- `templates/fragments/return_loan_modal.html` — pass `0` as the new 11ᵗʰ macro arg.
- `templates/fragments/contributor_delete_modal.html` — pass `0` as the new 11ᵗʰ macro arg.
- `templates/fragments/series_delete_modal.html` — pass `0` as the new 11ᵗʰ macro arg.
- `templates/fragments/admin_users_row.html` — deactivate `<form>` block: replaced with a `<button hx-get=...>` modal trigger.
- `tests/e2e/specs/journeys/admin-smoke.spec.ts` — new `test("deactivate user via modal", ...)` block + Escape-close + default-focus + paranoid `hx-confirm` lock.
- `CLAUDE.md` — "Modal scanner-guard invariant (story 7-5)" line rewritten to reflect the empty allowlist.

**New:**
- `templates/fragments/admin_user_deactivate_modal.html` — modal fragment calling the shared `components/modal.html::modal` macro (delete variant, POST method, version optimistic-locking).
- `tests/admin_user_deactivate_modal.rs` — 11 `#[sqlx::test]` cases.
- `tests/e2e/helpers/admin-users.ts` — `seedLibrarianUser` helper (if direct DB INSERT path chosen in Task 9).

**No change:**
- `src/services/admin*.rs`, `src/models/user.rs`, `static/js/modal.js`, `layouts/base.html`.

### Review Findings

Adversarial code review run 2026-05-07 with 3 parallel reviewers (Blind Hunter / Edge Case Hunter / Acceptance Auditor). 34 inputs → 2 actionable patches + 8 deferred + 24 dismissed. Acceptance Auditor reports all 15 ACs PASS with three minor literal deviations (none material).

**Patches (actionable — apply on this branch):**

- [x] [Review][Patch] French copy of `admin.users.deactivate_modal_body` is gendered while EN is neutral [locales/fr.yml:557] — rewrote to `"cette personne ne pourra plus se reconnecter avant réactivation"` matching EN's neutral tone. **Applied 2026-05-07.**
- [x] [Review][Patch] Trigger `<button>` lacks explicit `type="button"` [templates/fragments/admin_users_row.html:23] — added `type="button"` for defense-in-depth (mirror of Cancel button in `templates/components/modal.html`). **Applied 2026-05-07.** Verified post-fix: `cargo check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --test admin_user_deactivate_modal` (11/11 passed).

**Deferred (file as GitHub Issues `type:code-review-finding` per Foundation Rule #11 — do NOT add to a markdown tracking doc):**

- [x] [Review][Defer] [HIGH] NotFound from a modal handler silently no-ops in `#modal-slot` [src/error/mod.rs (default arm) + src/routes/admin.rs:626-633] — when the modal handler returns `AppError::NotFound` (target user soft-deleted between the panel render and the trigger click, or direct URL-crafted to a nonexistent id), `IntoResponse` returns 404 + plain body, NO `HX-Retarget`/`HX-Reswap` headers. HTMX default config does NOT swap on 4xx, so the modal slot stays empty — the trigger appears broken. Cross-cutting on all 5 modal handlers (9-10..9-14). Fix: emit `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` + the inline feedback fragment (mirror of the Conflict path), or special-case 404 to render a 200 fragment.
- [x] [Review][Defer] [HIGH] Conflict feedback hidden under modal overlay [static/js/modal.js:166-175 + src/error/mod.rs:140-155] — when Confirm POST returns 409 (self-deactivate via direct URL, last-admin race, version mismatch from concurrent edit), the `HX-Retarget: #feedback-list` lands feedback at the page-level OOB target, OUTSIDE `#modal-slot`. The modal's `htmx:afterRequest` listener sees `detail.failed === true` (non-2xx) and KEEPS the modal open. Result: modal stays open obscuring the feedback, Confirm appears unresponsive, only Esc/Cancel recovers. Cross-cutting: affects every destructive modal with a Conflict path. Fix: either close the modal on any non-2xx + flash the feedback, OR retarget Conflict feedback into a slot inside the modal body.
- [x] [Review][Defer] [MED] Row-swap mid-filter shows stale row [templates/fragments/admin_users_row.html + admin_users_panel.html] — `/admin?tab=users&status=active` filters out deactivated users, but the modal's `hx-target=#admin-users-row-{id}` + `hx-swap=outerHTML` swaps in a deactivated row in place. The user is staring at a deactivated row inside an "Active only" filter — visibly inconsistent until the next page refresh. Pre-existing 8-3 contract; needs a broader fix (refresh panel on deactivate, or filter-aware row swap).
- [x] [Review][Defer] [MED] Two-tab race: stale `version` → 409 dead-end [src/routes/admin.rs + src/models/user.rs::deactivate] — TOCTOU between modal GET (reads `user.version` at T0) and Confirm POST (T1). If anything updates the user between (admin edit, password change, role change in another tab), `version` becomes stale → 409 version-mismatch. Combined with the HIGH Conflict-feedback-hidden defer above, this is a UX dead-end. Modal renders happily but Confirm always 409s. Fix: surface stale-version 409s with explicit "page may be out of date — refresh and retry" copy, OR auto-close the modal on any non-2xx.
- [x] [Review][Defer] [MED] Conflicting Conflict variants in races blur audit signal [src/models/user.rs::deactivate + services::locking::check_update_result] — in race conditions (concurrent password change while admin opens last-admin modal), the Confirm POST may fail with `error.conflict` (version mismatch) instead of the more specific `last_admin_blocked` or `self_deactivate_blocked`. The model's UPDATE WHERE clause carries both the version check and the role guard; whichever short-circuits first wins. Operational consequence: the audit log records the version-mismatch flavor instead of the more informative root cause. Consider preserving the more specific message in race conditions.
- [x] [Review][Defer] `find_active_by_id(pool, id)` model helper [src/models/user.rs:133-148] — the current handler does a 2-step check: `find_by_id` (no filter) + `if user.deleted_at.is_some() { 404 }`. Correct but exposes a TOCTOU window and uses 2 round-trips. A `find_active_by_id` method filtering `WHERE id = ? AND deleted_at IS NULL` would single-trip the read and atomicize the check. Broader cleanup; not 9-14-scoped.
- [x] [Review][Defer] Enter key doesn't activate Confirm/Cancel via scanner-guard [static/js/scanner-guard.js:96-102] — when the modal is open, scanner-guard captures keydown at the document-capture phase. Enter on a non-text-input focused element (Cancel/Confirm button) is `preventDefault`+`stopPropagation`'d and dropped. Keyboard-only users must use Space, not Enter, to activate the modal buttons. Documented limitation; should either be added to CLAUDE.md "Modal scanner-guard invariant" section or special-cased in scanner-guard for `data-modal-confirm`/`data-modal-cancel` buttons.
- [x] [Review][Defer] No regression test for `%{...}` literal in username [tests/admin_user_deactivate_modal.rs] — `t!("admin.users.deactivate_modal_title", username = user.username.as_str())` interpolates `%{username}`. If a malicious admin creates a user named literally `%{username}`, the second-pass substitution behavior is undefined (rust_i18n likely doesn't recurse, but unverified). Add a regression test for username = `"admin %{x} user"` to lock the no-recursion contract.

### Change Log

| Date | Change |
| --- | --- |
| 2026-05-07 | Story created (backlog → ready-for-dev). Final PR in the hx-confirm migration chain (9.10 → 9.14, closes Epic 9's UX-DR8 modal foundation). Spec mirrors 9-13 with 4 critical deviations: (1) POST not DELETE — macro's action_method branch handles this; (2) `Role::Admin` not `Role::Librarian` — adds an 11ᵗʰ AC7 case for the librarian-403 path; (3) hx-target = `#admin-users-row-{id}` not a feedback container — row swap on success; (4) **NEW 11ᵗʰ macro param `version: i32`** for optimistic locking — touches the macro file + all 4 existing callers (forced micro-extension; YAGNI risk acknowledged but unavoidable since the form needs body-side `version`). Trigger button is currently a `<form>` with hidden inputs (not a plain `<button>`) — the migration replaces the entire `<form>` with a single `<button>`. Pre-flight self-deactivate / last-admin guards INTENTIONALLY OMITTED per "server contract unchanged" mandate; latent UX dead-end preserved as deferred GH issue (mirror of 9-13's #139 pattern). `ALLOWED_HX_CONFIRM_SITES` projected 1 → 0 (`&[]`) — empty steady state. CLAUDE.md "Modal scanner-guard invariant" line finally rewritten (deferred from every prior 9-1x story per "9-14 rewrites in one shot"). 11 integration tests planned (10 from 9-13 mirror + librarian-403 NEW). E2E test extends `admin-smoke.spec.ts`. `src/routes/admin.rs` projected 1583 → ~1663 LOC (under 2000 ceiling). Templates audit doc-comment rewritten. |
| 2026-05-07 | Story validated; 13 improvements applied (7 critical + 3 enhancements + 3 optimizations). **Critical fixes**: (1) corrected fragment filename `loan_return_modal.html` → `return_loan_modal.html`; (2) corrected `AppError::Conflict` variant strings to actual `"self_deactivate_blocked"` / `"last_admin_blocked"` + version-mismatch as i18n localized string (verified `src/models/user.rs:287,300`); (3) corrected `UserModel::deactivate` return type `Result<usize, AppError>` → `Result<u64, AppError>`; (4) corrected `AppError::Conflict::IntoResponse` contract — verified emits `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` + 409 (the `HX-Retarget` overrides the modal's `hx-target`, so error feedback lands on the page-level feedback list, NOT replaces the row); (5) **expanded AC4 cleanup scope** — the dead `confirm_deactivate` plumbing has 5 Rust sites + 2 template sites (NOT 2 sites), centered on a `UserWithConfirm` wrapper struct that exists ONLY to thread the dead key; co-droppage non-negotiable per clippy `unused struct`; (6) corrected ctor line `~1165` → `1185` (with `let` binding at `1171`); (7) removed reference to fictional `tests/e2e/global-setup.ts` precedent. **Enhancements**: definitively confirmed `UserModel::find_by_id` does NOT filter `deleted_at` (handler must add explicit guard); concrete CSRF-meta-fetch + POST snippet for AC9 seed; documented dual-render (`{% include %}` + standalone) of `admin_users_row.html`. **Optimizations**: trimmed ~50 lines of redundant prose; pinned Reactivate-form line numbers; LOC budget verified via `wc -l`. |
| 2026-05-07 | Story implemented (in-progress → review). 11 integration tests + 1 E2E test + audit/i18n cleanup; UserWithConfirm wrapper torn down across 5 Rust sites + 2 templates; modal macro extended to 11 params (version optimistic-locking) with all 4 prior callers + test wrapper updated to pass 0; `ALLOWED_HX_CONFIRM_SITES` reaches `&[]` steady-state; CLAUDE.md "Modal scanner-guard invariant" line rewritten. PR #141 opened, CI all green (DB integration / Playwright E2E / Playwright wizard E2E / Rust tests + clippy + sqlx-prepare — both push and pull_request runs). |
| 2026-05-07 | Code review complete (review → done). 3 parallel reviewers (Blind / Edge / Auditor) — 34 inputs → 2 actionable patches + 8 deferred + 24 dismissed. Acceptance Auditor: ALL 15 ACs PASS. 0 BLOCKERS, 0 decision-needed. 2 patches applied: (1) gendered FR copy of `admin.users.deactivate_modal_body` rewritten to neutral ("cette personne ne pourra plus" instead of "il ne pourra plus") to match EN's neutral tone; (2) added explicit `type="button"` to the deactivate trigger button — defense-in-depth against a future template refactor that wraps the row in a `<form>` (mirror of the modal's Cancel button). Post-patch: `cargo check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --test admin_user_deactivate_modal` (11/11 passed) — all green. 8 deferred items to file as `type:code-review-finding` GH issues per Foundation Rule #11: top items are HIGH NotFound modal silent no-op (cross-cutting on all 5 modal handlers — modal slot stays empty when 4xx because IntoResponse doesn't HX-Retarget), HIGH Conflict feedback hidden under modal overlay (cross-cutting UX dead-end on every destructive modal with a Conflict path), MED row-swap-mid-filter shows stale row, MED two-tab race version=stale → 409 dead-end, MED conflicting Conflict variants in races blur audit signal, find_active_by_id helper for one-trip soft-delete check, scanner-guard Enter-key activation, %{...} literal username regression test gap. **🎉 Epic 9 hx-confirm migration chain complete: 9-10 → 9-14, ALLOWED_HX_CONFIRM_SITES = &[], UX-DR8 Modal is now the canonical destructive-confirmation pattern across the entire codebase.** |
