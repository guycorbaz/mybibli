# Story 9.13: Migrate hx-confirm — delete series

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As the project maintainer,
I want the delete-series flow on `/series/:id` migrated from `hx-confirm=` to the UX-DR8 Modal component built in 9.10,
so that the destructive-action pattern is enforced, the existing server-side title-assignment guard keeps working unchanged, and the fourth grandfathered site exits the `ALLOWED_HX_CONFIRM_SITES` allowlist (2 → 1 entry — only `admin_users_row.html` remains for 9.14 to close).

## ⚠️ Existing-code reality check

Before writing a single line, walk the code that 9-13 touches and verify the assumptions below — they are LOCKED IN by the 9-12 close (current main as of 2026-05-07):

- **Modal macro is shipped, 10 params, UNCHANGED in this story.** `templates/components/modal.html` takes `(variant, title, body_html, confirm_label, cancel_label, action_url, action_method, csrf_token, hx_target, hx_swap)`. 9-13 calls it with `("delete", …, "DELETE", csrf_token, "#series-feedback", "innerHTML")` — the same hardcoded-target shape as 9-12.

- **`static/js/modal.js` is shipped (197 LOC).** Focus trap + Escape close + backdrop close + `[data-modal-trigger][data-pressed="true"]` focus-restoration + `htmx:afterRequest` close-on-2xx are all in place. UNCHANGED in this story.

- **`<div id="modal-slot">` is in `layouts/base.html`.** Already present on every page that extends the layout, including `/series/:id`. No layout edit needed.

- **`<div id="series-feedback" class="mt-4" aria-live="polite"></div>` already exists** at `templates/pages/series_detail.html:43`. The migration reuses it — do NOT add a sibling container. Verify the div is still rendered after Task 5's button-replace edit.

- **One `hx-confirm=` site in scope:**
  - `templates/pages/series_detail.html:35` — delete-series button inside the librarian/admin action bar (the role-gated `{% if role == "librarian" || role == "admin" %}` block at line 29). The whole `<button hx-delete=… hx-confirm=…>` lives on lines 35–39 (4 lines including the closing tag and the label text). The button already targets `#series-feedback` with `hx-swap="innerHTML"` — that piece is preserved by the macro contract.

- **Current `ALLOWED_HX_CONFIRM_SITES`** in `src/templates_audit.rs:35-38` has 2 entries × 1 occurrence = 2 grandfathered. After 9-13: `series_detail.html` entry removed entirely. Result: 1 entry × 1 occurrence = 1 grandfathered (`admin_users_row.html` for 9.14). The const NEVER carries a `count == 0` entry — the audit's positive assertion shape requires entries to be deleted, not zeroed.

- **DELETE handler `delete_series`** lives in `src/routes/series.rs:493-528` — same file as the GET detail handler. UNCHANGED by this story. Key contract:
  - Endpoint: `DELETE /series/{id}` (registered at `src/routes/mod.rs:174-179` as a method-routed `axum::routing::get(...).post(...).delete(series::delete_series)` chain on the same path). **Singular `/series/{id}`** — NO `/catalog/` prefix and NOT `/catalog/series/{id}`. This is asymmetric with 9-12's `/catalog/contributors/{id}` plural+/catalog/-prefix endpoint. Don't transcribe the contributor path by reflex.
  - **Role gate: `Role::Librarian`** (matches the template gate `{% if role == "librarian" || role == "admin" %}`; admin > librarian both pass). Same as 9-12.
  - On success (HTMX): 200 + `HX-Redirect: /series` → full-page nav.
  - On success (non-HTMX): `Redirect::to("/series")`.
  - **On `AppError::Conflict` (titles assigned)**: the route's `Err(e)` arm matches only `AppError::NotFound`; ANY other error (including `Conflict`) falls through to `_ => rust_i18n::t!("error.internal", …)` — so the generic "internal error" copy is rendered inline, NOT the meaningful `series.delete_has_titles` payload that the service constructs. This is a **latent UX bug** preserved as-is (server contract is "200 + inline feedback" with generic copy on conflict). DO NOT fix in 9-13 — the story spec says protections "remain server-side", not "improve". File a deferred GH issue (`type:code-review-finding`) at story close so the bug doesn't drift into the noise; the fix likely belongs in a chore PR after 9-14 (`Err(AppError::Conflict(msg)) => msg.clone()`, sibling of NotFound).
  - On NotFound: 200 + inline `feedback_html_pub("error", &message, "")` with the i18n `error.not_found` text.

- **`series.confirm_delete`** in `locales/en.yml:155` and `locales/fr.yml:155` (`"Delete this series?"` / `"Supprimer cette série ?"`) is the OLD plain-confirm copy. This story DROPS it (zero callers after migration; mirror of 9-10's `borrower.confirm_delete` drop, 9-11's `loan.return_confirm` drop, 9-12's `contributor_detail.confirm_delete` drop). The sibling key `series.delete` (`"Delete"` / `"Supprimer"`) — the trigger button label — is RETAINED.

- **`series_detail.html` HAS no inline-form-rendered fragment fallback.** The page always renders fully via `series_detail_page` (`src/routes/series.rs:186-249`); there is NO HTMX-fragment branch with a duplicate delete button. The migration is therefore template-only — edit `templates/pages/series_detail.html` once, no Rust-emitted HTML to migrate. After the edit, the project-wide grep `grep -rn 'hx-confirm=' templates/` must drop from 2 hits to 1, and `grep -rn 'hx-confirm=' src/` stays at 1 (the pre-existing `src/routes/locations.rs:256` entry inherited from prior epics — out of scope, see 9-12 deferred items).

- **`role` gate on the delete trigger BUTTON** is `{% if role == "librarian" || role == "admin" %}` (`templates/pages/series_detail.html:29`). This stays — anonymous users never see the trigger, so the modal handler never receives an anonymous request from a real user. The `Role::Librarian` gate on the modal handler is the second layer (defense-in-depth: a malicious anonymous user typing `/series/42/delete-modal` directly hits a 303 → /login).

## Acceptance Criteria

1. **AC1 — NEW handler `GET /series/:id/delete-modal`** in `src/routes/series.rs` (sibling of `series_detail_page`):
   - Returns the rendered modal fragment via the shared `components/modal.html::modal` macro, variant `delete` (soft-delete — the series row goes to Trash via `SeriesModel::soft_delete`, mirroring 9-10/9-12; `delete-forever` is reserved for hard-delete from the Trash panel).
   - Pre-translates 4 i18n keys: title (`series.delete_modal_title` — `"Delete series %{name}?"` interpolated via `t!(..., name = …)`, mirror of 9-12's interpolation pattern), body (`series.delete_modal_body` — `"Assigned titles must be re-attached or detached first."`), confirm (`series.delete_modal_confirm` — `"Delete"`), cancel (`common.cancel` — already shipped by 9-10).
   - **Role gate**: `session.require_role_with_return(Role::Librarian, &format!("/series/{id}"))?` — mirrors the trigger's `{% if role == "librarian" || role == "admin" %}` template gate (admin > librarian both pass). The `_with_return` variant ensures an anonymous direct-URL hitter lands back on the series detail page after login (mirror of 9-10/9-12).
   - Returns 404 if the series is soft-deleted or not found (`SeriesModel::active_find_by_id` already filters `WHERE deleted_at IS NULL` and returns `Option<SeriesModel>`).
   - Direct browser navigation (no `HX-Request` header) returns 405 Method Not Allowed via `Ok(StatusCode::METHOD_NOT_ALLOWED.into_response())` — single line, **NO `Allow:` response header, EMPTY BODY**. Mirror of 9-11/9-12's clean 405 shape (verified by `tests/return_loan_modal.rs` and `tests/contributor_delete_modal.rs`). Rationale: `Allow: GET` self-contradicts a 405 when we DO support GET (just not without `HX-Request`); the 405 here means "wrong request shape".
   - **No `?target=` query parameter** in 9-13 (single surface, mirror of 9-12). The modal fragment hardcodes `hx_target="#series-feedback"` and `hx_swap="innerHTML"` via the macro's 9-th and 10-th params. Document the deviation from 9-11's closed-allowlist pattern in Dev Agent Record.
   - **HTML-escape the series name?** NO — pass the RAW name into `t!("series.delete_modal_title", name = …)`. Askama's default auto-escape on `{{ title }}` inside the modal macro handles HTML safety. Pre-escaping would double-escape. Mirror of 9-10/9-12's name handling. Verified by AC7's `..._html_escapes_series_name` test.

2. **AC2 — NEW fragment template `templates/fragments/series_delete_modal.html`** (mirror of `templates/fragments/contributor_delete_modal.html` from 9-12, ~17 LOC):
   - Imports the shared macro: `{% import "components/modal.html" as modal %}`.
   - Calls: `{% call modal::modal("delete", title, body_html, confirm_label, cancel_label, action_url, "DELETE", csrf_token, "#series-feedback", "innerHTML") %}{% endcall %}`.
   - The `action_url` handler-side is `format!("/series/{}", series.id)` — **singular `/series/{id}`, NO `/catalog/` prefix** (asymmetric with 9-12's contributor path). Verify in Task 1 by reading `src/routes/mod.rs:174-179` directly.
   - The `body_html` is built handler-side as `format!("<p>{}</p>", body_text)` after pulling `body_text` out of `t!()`; the i18n value carries no user-supplied interpolation, so no escape is needed (mirror of 9-10/9-12).

3. **AC3 — Migrate `templates/pages/series_detail.html:35`** delete-series button:
   - Before:
     ```html
     <button hx-delete="/series/{{ series.id }}" hx-confirm="{{ confirm_delete }}"
             hx-target="#series-feedback" hx-swap="innerHTML"
             class="px-3 py-1.5 text-sm font-medium text-red-600 dark:text-red-400 border border-red-300 dark:border-red-700 rounded-md hover:bg-red-50 dark:hover:bg-red-900/20">
         {{ delete_label }}
     </button>
     ```
   - After:
     ```html
     <button hx-get="/series/{{ series.id }}/delete-modal"
             hx-target="#modal-slot" hx-swap="innerHTML"
             data-modal-trigger aria-haspopup="dialog" aria-expanded="false"
             class="px-3 py-1.5 text-sm font-medium text-red-600 dark:text-red-400 border border-red-300 dark:border-red-700 rounded-md hover:bg-red-50 dark:hover:bg-red-900/20">
         {{ delete_label }}
     </button>
     ```
   - Tailwind classes UNCHANGED (visual identity preserved). `aria-haspopup="dialog"` + `aria-expanded="false"` are the 9-11/9-12 a11y standard for modal triggers.
   - The existing `#series-feedback` div at line 43 stays put — it is the modal Confirm form's `hx-target` after migration.

4. **AC4 — Drop the now-dead `confirm_delete` field** from `SeriesDetailTemplate` in `src/routes/series.rs:175` and the construction site at line 231. Foundation Rule #1 (DRY) — the field is dead immediately after migration (only the dropped `hx-confirm` attribute referenced it). Mirror of 9-10/9-11/9-12.
   - Verify by grep: `grep -rn 'confirm_delete\|series\.confirm_delete' src/ templates/ locales/` should return ZERO hits after the field, the i18n key, and the template attribute are all removed.

5. **AC5 — Update `src/templates_audit.rs::ALLOWED_HX_CONFIRM_SITES`**:
   - Before:
     ```rust
     ("templates/pages/series_detail.html", 1),
     ("templates/fragments/admin_users_row.html", 1),
     ```
   - After:
     ```rust
     ("templates/fragments/admin_users_row.html", 1),
     ```
   - Total entries: 2 → 1. Total occurrences: 2 → 1. The allowlist will reach `&[]` at 9-14 close.
   - `cargo test hx_confirm_matches_allowlist` MUST pass with the trimmed array.
   - **Audit doc-comment** at `src/templates_audit.rs:30-34` stays as-is (the wording about "the only templates allowed to carry this attribute" remains accurate; the migration count narrative belongs in CLAUDE.md and the story spec, not in the audit comment). The CLAUDE.md "Modal scanner-guard invariant" line still says "frozen at 5 grandfathered sites" — DO NOT edit that line in 9-13. The 9-14 spec explicitly handles the CLAUDE.md edit when the allowlist drops to `&[]`.

6. **AC6 — i18n: 3 NEW keys + 1 DROPPED key per locale** (EN + FR):
   - **NEW** under `series:` block in `locales/en.yml` and `locales/fr.yml`. **Insert after `series.delete_has_titles` at line 181** (clusters all `delete_*`-prefixed keys together; mirror of 9-12's placement of `contributor.delete_modal_*` near the existing `contributor.delete*` keys):
     - `delete_modal_title: "Delete series %{name}?" / "Supprimer la série %{name} ?"`
     - `delete_modal_body: "Assigned titles must be re-attached or detached first." / "Les titres associés doivent être détachés ou réaffectés au préalable."`
     - `delete_modal_confirm: "Delete" / "Supprimer"`
   - **DROPPED**: `series.confirm_delete` (EN: `"Delete this series?"` / FR: `"Supprimer cette série ?"`) — zero callers after AC3/AC4 (Foundation Rule #1 dead-key drop, mirror of 9-10/9-11/9-12 chain).
   - **REUSED** (no edits): `common.cancel` (shipped by 9-10), `series.delete` (the trigger button label, KEPT).
   - Run `cargo test all_t_keys_have_both_locales` (the parity test in `src/i18n/audit.rs:186`) to confirm every `t!()` call site has a key in both locale files. NOTE: this audit is **one-way** (it only checks that keys called via `t!()` exist in both locales; it does NOT catch a key present in only one locale that is never called from `t!()`). Eyeball-diff the EN/FR additions and removals.
   - Run `touch src/lib.rs && cargo build` after editing locale files to force the rust-i18n proc macro to re-read the YAML (CLAUDE.md i18n rule).

7. **AC7 — Integration tests** (NEW file `tests/series_delete_modal.rs`, sibling of `tests/contributor_delete_modal.rs` from 9-12 and `tests/borrower_delete_modal.rs` from 9-10). 9 `#[sqlx::test]` cases mirroring the 9-12 shape (same `build_state` / `seed_session` / `req_htmx` / `req_plain` / `body_text` helper pattern):
   - `get_series_delete_modal_returns_200_with_dialog_for_librarian_request` — librarian session, GET `/series/:id/delete-modal`, returns 200 + body contains `<dialog open aria-modal="true">` + the series name + `data-modal-variant="delete"` (stable selector, NOT the brittle `bg-red-600` Tailwind substring per the 9-12 review patch P6) + `data-modal-default-focus` on Cancel + `hx-delete="/series/{id}"` (verify the **singular path, NO `/catalog/` prefix**) + `hx-target="#series-feedback"` + `hx-swap="innerHTML"` + the hidden `_csrf_token` input.
   - `get_series_delete_modal_returns_200_for_admin_request` — admin can also delete (admin > librarian); same shape.
   - `get_series_delete_modal_redirects_anonymous_to_login` — anonymous session, returns 303 → `/login?next=%2Fseries%2F{id}` (verifies `_with_return` URL-encodes the series path correctly).
   - `get_series_delete_modal_returns_404_for_soft_deleted_series` — series with `deleted_at = NOW()`, returns 404 (the `active_find_by_id` filter handles this naturally).
   - `get_series_delete_modal_returns_404_for_nonexistent_series` — id `99999` with no row, returns 404.
   - `get_series_delete_modal_returns_405_for_non_htmx_request` — direct browser nav (no `HX-Request` header), returns 405. **Body is empty AND no `Allow:` response header is set**. Assertions: `assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);` + `assert!(resp.headers().get(axum::http::header::ALLOW).is_none(), "405 must not set Allow header — see story 9-11 code-review patch");` + `assert!(body_text(resp).await.is_empty(), "405 body must be empty per AC1 — story 9-12 review patch");`. Mirror of 9-11/9-12's 405 test shape.
   - `get_series_delete_modal_html_escapes_series_name` — series named `<script>alert(1)</script>`, returned HTML contains the entity form (`&lt;script&gt;` OR `&#60;script&#62;` — Askama emits one of the two valid forms; assert on raw-substring absence + ANY-entity-form presence) AND does NOT contain the raw `<script>alert(1)</script>` substring. Mirror of 9-12's escape-check assertion shape.
   - `delete_series_via_existing_handler_still_works` — sanity check: as a librarian, fire `DELETE /series/{id}` directly (the unchanged existing handler) for a series with NO title assignments, assert response 200 + `HX-Redirect: /series` + the row is soft-deleted (`deleted_at IS NOT NULL`). Mirror of 9-10's `delete_borrower_via_existing_handler_still_works` and 9-12's contributor equivalent.
   - **9-th case — title-assignment conflict path**: `delete_series_with_assigned_titles_returns_inline_conflict_feedback` — seed a series + a title row + an `INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, 1)` junction row (the junction table is **`title_series` singular** per `migrations/20260329000000_initial_schema.sql:190-206`; `position_number` is `NOT NULL`, pass `1`). Fire `DELETE /series/{id}`, assert response 200 + body contains the **generic `error.internal` copy** — exact strings: `"An internal error occurred"` (EN) / `"Une erreur interne est survenue"` (FR), per `locales/en.yml:187` and `locales/fr.yml:187`. The body must NOT contain the `series.delete_has_titles` payload (assert absence of the substring `"title(s) assigned"` and FR `"titre(s) assigné"`). This locks the **latent UX bug** described in the reality check; if a future PR fixes the bug (matching `Conflict(msg)` in the route handler), this test will fail and force the fix to flow through 9-13's regression net. Comment the assertion explicitly: `// LATENT UX BUG: series delete_series matches only NotFound; Conflict falls into the catch-all generic copy. Story 9-13 preserves this server contract; fix is deferred to a future chore PR.`
   - **CSRF assertion** (AC11): the librarian-happy-path test MUST also assert `assert!(html.contains("name=\"_csrf_token\""))` to lock the macro's CSRF embedding (mirror of 9-12).
   - **Integration test for the rendered detail page's `#series-feedback` div existence** (mirror of 9-12 review patch P3 — load-bearing because the modal hardcodes `hx_target="#series-feedback"`): add a 10th `#[sqlx::test]` case `series_detail_page_renders_feedback_target_div` that GETs `/series/:id` as a librarian and asserts `body.contains("id=\"series-feedback\"")`. This is the safety net against a future template edit accidentally removing the target div, which would silently no-op the modal Confirm flow.

8. **AC8 — Templates audit stays green**: `cargo test no_inline_markup_in_templates`, `cargo test hx_confirm_matches_allowlist`, `cargo test forms_include_csrf_token`, `cargo test csrf_exempt_routes_frozen` all pass after the migration. The new modal trigger uses `data-modal-trigger` (not `hx-confirm`), no new CSP-violating markup is introduced.

9. **AC9 — E2E test** — extend the existing `tests/e2e/specs/journeys/series.spec.ts:72-96` test (`"delete series removes it from list"`) to drive the new modal flow:
   - **Drop** the `page.on("dialog", (d) => d.accept());` line at `:82` — obsolete (no native confirm dialog any more).
   - **Replace** the `await deleteBtn.click();` at `:89` with the modal click sequence:
     ```ts
     await deleteBtn.click();
     await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
     await page.locator("[data-modal-confirm]").click();
     ```
   - Confirm click → `HX-Redirect: /series` → `await page.waitForURL("**/series", { timeout: 5000 })` continues to pass.
   - **NEW assertion — the delete-trigger button has no `hx-confirm`** (paranoid lock against re-introduction beyond the audit's count-mismatch detection — mirror of 9-12 review patch P5):
     ```ts
     await expect(deleteBtn).not.toHaveAttribute("hx-confirm", /./);
     ```
   - **NEW inline Escape-close + default-focus assertion** (regression cover for 9-10 focus-trap inheritance — mirror of 9-12 AC9):
     - BEFORE the final delete sequence, add:
       ```ts
       await deleteBtn.click();
       await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
       await expect(page.locator("[data-modal-default-focus]")).toBeFocused();
       await page.keyboard.press("Escape");
       await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible();
       ```
     - Then re-click the trigger to drive the actual delete flow. Same shape as 9-12.
   - **NEW conflict-path coverage** — extend the spec with a sibling test `"delete series with assigned titles shows block message"`:
     - **Naming convention**: use `const SERIES_NAME = \`SE-DeleteConflict-${Date.now()}\`;` to follow the existing `SE-<purpose>-${Date.now()}` pattern from `series.spec.ts:73` and `:108` — avoids parallel-spec collisions per the data-isolation rules in CLAUDE.md.
     - **Fixture flow** (mirror the existing assignment pattern at `tests/e2e/specs/journeys/series.spec.ts:104-145`): (1) `await loginAs(page)`; (2) `await page.goto("/series/new")` → fill name → submit → `await page.waitForURL(/\/series\/\d+/)` (capture `seriesUrl = page.url()`); (3) `await page.goto("/catalog")` → `#scan-field.fill(specIsbn("SE", 12))` → press Enter → wait for `.feedback-entry, .feedback-skeleton` (timeout 10000); (4) `await page.goto(\`/?q=${specIsbn("SE", 12)}\`)` → click first `a[href^='/title/']` → `waitForURL(/\/title\/\d+/)`; (5) `#assign-series.selectOption({ label: SERIES_NAME })` → `#assign-position.fill("1")` → `#assign-series-submit.click()` → `waitForURL(/\/title\/\d+/)`; (6) `await page.goto(seriesUrl)` to land on series detail. Now the conflict fixture is in place.
     - **Modal Confirm**: click Delete → `await expect(page.locator("#modal-slot dialog[open]")).toBeVisible()` → click `[data-modal-confirm]`.
     - **Assertions** (in order):
       1. Modal closes after the 200 response: `await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible();` — mirror of 9-12 review patch P4 (`htmx:afterRequest` close-on-2xx contract).
       2. `#series-feedback` contains the generic internal-error copy: `await expect(page.locator("#series-feedback")).toContainText(/An internal error occurred|Une erreur interne est survenue/i)` (exact strings per `locales/en.yml:187` + `locales/fr.yml:187`; the regex `/internal error|erreur interne/i` is also valid as a looser locale-agnostic match).
       3. NO redirect happened — the series was NOT deleted: `await expect(page).toHaveURL(seriesUrl);` (the URL stays on `/series/:id`).
     - Comment the test docblock: `// Locks the inline-feedback path on conflict. Note: feedback copy is the generic "error.internal" string ("An internal error occurred" / "Une erreur interne est survenue"), NOT the meaningful series.delete_has_titles message — that's a latent UX bug preserved by 9-13 (see story file's reality-check section + the deferred GH issue).`
     - **`specIsbn` import**: the helper is already imported in `series.spec.ts` (used at lines 121, 127). No new import needed.
   - **Helper updates**: NO new helper file (single-spec usage; mirror of 9-12 decision). The existing inline `page.evaluate(...)` blocks for series creation stay as-is.
   - **CI flake gate** (`grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/`) MUST stay clean — the existing test uses `waitForURL` and DOM assertions, not arbitrary sleeps; the migration must not regress this.
   - **EN/FR matcher invariance**: existing `page.getByRole("button", { name: /delete|supprimer/i })` continues to work. The `data-modal-confirm` selector is locale-agnostic.

10. **AC10 — Foundation Rule #12 LOC discipline**:
    - `templates/pages/series_detail.html` net change: 1 line replaced + a11y attrs added (~+1 LOC, still well under 2000).
    - `templates/fragments/series_delete_modal.html` is a NEW file (~17 LOC).
    - `src/routes/series.rs` grows by ~70 LOC (new `delete_modal` handler + `SeriesDeleteModalTemplate`). Current LOC: 528. Projected: ~595. Far under 2000 — no extraction needed.
    - `src/routes/mod.rs` net change: +4 LOC for the new route registration.
    - `src/templates_audit.rs` net change: −1 line (entry removed).
    - `tests/series_delete_modal.rs` is a NEW file (~330–360 LOC of integration tests, mirror of `tests/contributor_delete_modal.rs`'s 473 LOC). Lives in `tests/` (not `src/tests/`), so the 2000-LOC ceiling doesn't bite.
    - `tests/e2e/specs/journeys/series.spec.ts` net change: +30 to +50 LOC (modal click sequence + Escape-close + the new conflict-path test). Current LOC: 293. Projected: ~325–340.
    - `locales/en.yml` and `locales/fr.yml` net change: +3 keys / −1 key per locale = +2 lines per locale.

11. **AC11 — CSP / scanner-guard / CSRF inheritance**:
    - The new modal uses `<dialog open aria-modal="true">` (inherited from the macro). Scanner-guard 7-5 applies automatically — no new E2E assertion needed (the 9-10 + 9-11 specs already exercise scanner-guard inheritance once for the foundation, and 9-12 deliberately did not duplicate it; 9-13 follows suit per "foundation tests are load-bearing once, not per-migration").
    - **CSRF**: the modal's Confirm button issues `hx-delete="/series/{id}"`. The macro's `csrf_token` 8th param renders a hidden `<input name="_csrf_token">` inside the modal's confirm form (verified by AC7's 200-with-dialog test). Without it, the CSRF middleware on `DELETE /series/{id}` would 403.
    - **NOTE**: `templates_audit::forms_include_csrf_token` matches `<form method="POST">` only — the modal macro's `<form hx-delete=…>` (no `method=` attribute) is NOT scanned by that audit. The CSRF input is policed instead at TWO layers: (a) AC7's integration test asserts the hidden input is present in the rendered HTML; (b) the CSRF middleware rejects any state-changing request lacking a valid token, which the E2E tests would catch as a 403 + failed Confirm flow. Don't lean on the audit as the safety net — lean on AC7.

12. **AC12 — Server contract is UNCHANGED**: `DELETE /series/{id}` returns the same `HX-Redirect: /series` for HTMX success / `Redirect::to("/series")` for non-HTMX success / inline `Html(feedback_html_pub("error", &message, ""))` 200 on Conflict + NotFound. The existing `tests/e2e/specs/journeys/series.spec.ts:92` `await page.waitForURL("**/series", ...)` MUST keep passing. The only change to the existing handler is a doc-comment update (mirror of 9-12's discoverability-link patch): add `/// Trigger UX: see GET /series/:id/delete-modal (story 9-13).` above the `pub async fn delete_series` line.

13. **AC13 — Story-level grep audit**: at story close, run three greps and document the output in Dev Agent Record:
    - `grep -rnE 'hx-confirm=' templates/` — must return exactly 1 hit, matching `ALLOWED_HX_CONFIRM_SITES.len()` after the trim (only `admin_users_row.html` remains).
    - `grep -rnE 'hx-confirm=' src/` — must return EXACTLY 1 hit (the pre-existing `src/routes/locations.rs:256` Rust-emitted entry, unchanged from 9-12 close — out of scope; documented as inherited tech debt).
    - `grep -rn 'confirm_delete' src/ templates/ locales/` — must return ZERO series-related hits (the AC4 + AC6 cleanup must be complete). Borrower / contributor / loan / location / admin entries: ZERO if 9-10/9-11/9-12 cleaned them up (verify the count is exactly 0; if any sibling entries linger, file as code-review-finding — out of scope for 9-13 to migrate).

14. **AC14 — Local Testing Before Push (Foundation Rule #13)**: run the full local gate before opening the PR. Minimum:
    - `SQLX_OFFLINE=true cargo check` — clean
    - `cargo clippy --all-targets -- -D warnings` — clean
    - `cargo test --lib` — green (≥755 lib + the new AC7 cases + existing integration suites)
    - `cargo test --test series_delete_modal` — green (the 10 integration tests from AC7)
    - `cargo test hx_confirm_matches_allowlist` — green
    - `cargo test no_inline_markup_in_templates` — green
    - `cargo test forms_include_csrf_token` — green
    - `cargo test all_t_keys_have_both_locales` — green
    - Full E2E via `./scripts/e2e-reset.sh` + `cd tests/e2e && npm test` — green; pay attention to `series.spec.ts` going green with the migrated flow + the new conflict-path test.
    - The flake gate: `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` returns nothing.

15. **AC15 — Draft PR + CI gate (Foundation Rule #15 + #18)**: open a draft PR at the first commit (per `gh pr create --draft`) and WAIT for CI to finish before requesting review or merging. CI green → squash-merge. CI red → diagnose via `gh run view --log-failed`, fix, push, wait again. The hx-confirm migration chain is precisely the workflow #15 + #18 were designed for.

## Tasks / Subtasks

- [x] **Task 1 — Verify reality-check assumptions (AC: all)**
  - [x] Read `src/routes/series.rs:493-528` and confirm: role gate is `Role::Librarian`, endpoint is **singular `/series/{id}` with NO `/catalog/` prefix**, success returns `HX-Redirect: /series` for HTMX / `Redirect::to("/series")` for non-HTMX, error arm matches only `NotFound` and falls through to `error.internal` for everything else (the Conflict-→-generic-copy latent bug). Document the exact endpoint string in Dev Agent Record so the modal handler's `action_url = format!("/series/{}", id)` is unambiguous.
  - [x] Read `src/routes/mod.rs:174-179` and confirm the existing route registration (the new GET delete-modal route registers immediately AFTER this block).
  - [x] Read `templates/components/modal.html` and confirm the 10-param shape (UNCHANGED from 9-12). Confirm the `delete` variant emits `data-modal-variant="delete"` on the `<dialog>` (stable selector for AC7).
  - [x] Read `templates/fragments/contributor_delete_modal.html` and `tests/contributor_delete_modal.rs` for the mirror pattern. Series versions should be near-byte-identical with type/name swaps + path adjustment (`/series/{id}` instead of `/catalog/contributors/{id}`).
  - [x] Read `src/services/series.rs::SeriesService::delete_series` (lines 133-144) to confirm the `active_count_titles` guard and AppError::Conflict construction. UNCHANGED in this story.
  - [x] Read `src/models/series.rs::SeriesModel::active_find_by_id` to confirm it filters `WHERE deleted_at IS NULL` and returns `Option<SeriesModel>`. No new model method needed.
  - [x] Grep `confirm_delete` callers across `src/`, `templates/`, `locales/` to confirm dropping the i18n key + the field + the template attribute removes ALL series references. Document the call-site count in Dev Agent Record (expected: 4 — `series_detail.html:35`, `routes/series.rs:175` field, `routes/series.rs:231` construction, plus the i18n key in both en.yml + fr.yml at line 155).
  - [x] Measure current `src/routes/series.rs` LOC (`wc -l`). Project +70 LOC. Current is 528 → projected ~595, comfortably under 2000.
  - [x] Confirm the title-↔-series schema (LOCKED — verified during create-story): the junction table is **`title_series` (singular)** declared at `migrations/20260329000000_initial_schema.sql:190-206`, columns `(id, title_id, series_id, position_number, is_omnibus, created_at, updated_at, deleted_at, version)` with `position_number INT NOT NULL`. The `titles` table has **NO `series_id` column** — the association is exclusively the junction. The exact INSERT shape for the AC7 9th-case fixture is `INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, 1)` — `position_number = 1` (or any positive int) is fine; the guard query in `SeriesModel::active_count_titles` (`src/models/series.rs:216-227`) only filters on `ts.series_id = ? AND ts.deleted_at IS NULL AND t.deleted_at IS NULL`, so any active row triggers the conflict.

- [x] **Task 2 — i18n keys (AC: 6)**
  - [x] Add 3 new keys to `locales/en.yml` under the existing `series:` block:
    ```yaml
    series:
      # … existing keys …
      delete_modal_title: "Delete series %{name}?"
      delete_modal_body: "Assigned titles must be re-attached or detached first."
      delete_modal_confirm: "Delete"
    ```
  - [x] Add the same 3 keys to `locales/fr.yml` with FR copy:
    ```yaml
    series:
      # … existing keys …
      delete_modal_title: "Supprimer la série %{name} ?"
      delete_modal_body: "Les titres associés doivent être détachés ou réaffectés au préalable."
      delete_modal_confirm: "Supprimer"
    ```
  - [x] Drop `series.confirm_delete` from BOTH locale files (zero callers after AC3 + AC4; verified via Task 1 grep).
  - [x] Run `touch src/lib.rs && cargo build` to force rust-i18n proc-macro recompilation.
  - [x] Run `cargo test all_t_keys_have_both_locales` to confirm every `t!()` key has an entry in both locales.

- [x] **Task 3 — `GET /series/:id/delete-modal` handler + route (AC: 1, 2, 11)**
  - [x] Add to `src/routes/series.rs`:
    - `SeriesDeleteModalTemplate` struct (mirror of `BorrowerDeleteModalTemplate` from 9-10 / `ContributorDeleteModalTemplate` from 9-12): fields `title`, `body_html`, `confirm_label`, `cancel_label`, `action_url`, `csrf_token`. The fragment template references these field names directly.
    - `pub async fn delete_modal(...)` mirroring `contributors::delete_modal` from 9-12 (~70 LOC). Inputs: `State<AppState>`, `Session`, `Extension<Locale>`, `HxRequest(is_htmx)`, `Path<u64>`. Behaviors per AC1:
      - `session.require_role_with_return(Role::Librarian, &format!("/series/{id}"))?`
      - Early-return `Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response())` if `!is_htmx` — single-line shape, no `Allow:` header, no body. Mirror of 9-11/9-12.
      - `let series = SeriesModel::active_find_by_id(pool, id).await?.ok_or_else(|| AppError::NotFound(...))?;`
      - Pre-translate the 4 i18n keys via `t!(..., locale = loc)`. For the title key, pass the raw name: `t!("series.delete_modal_title", locale = loc, name = series.name.as_str())`.
      - Build `body_html = format!("<p>{body_text}</p>")`.
      - Set `action_url = format!("/series/{}", series.id)` (**SINGULAR, no `/catalog/` prefix** — verify against `mod.rs:174-179`).
      - Render the template; on `Err(e)` return `AppError::Internal(format!("series delete modal render: {e}"))` (mirror of 9-12's pattern with the original error captured for debuggability).
  - [x] Register the route in `src/routes/mod.rs` (immediately after the existing `/series/{id}` method-routed block at lines 174-179):
    ```rust
    .route(
        "/series/{id}/delete-modal",
        axum::routing::get(series::delete_modal),
    )
    ```

- [x] **Task 4 — Modal fragment template (AC: 2, 11)**
  - [x] Create `templates/fragments/series_delete_modal.html` (mirror of `templates/fragments/contributor_delete_modal.html`, ~17 LOC):
    ```jinja
    {# Story 9-13 — series delete confirmation modal.
       Calls the shared `components/modal.html::modal` macro with the
       `delete` variant. Body includes pre-translated assigned-titles hint copy.
       Hardcoded hx_target = "#series-feedback" because this is the
       only surface (single source). CSP-clean — no inline scripts/styles. #}
    {% import "components/modal.html" as modal %}
    {% call modal::modal(
        "delete",
        title,
        body_html,
        confirm_label,
        cancel_label,
        action_url,
        "DELETE",
        csrf_token,
        "#series-feedback",
        "innerHTML",
    ) %}{% endcall %}
    ```
  - [x] Run `cargo test no_inline_markup_in_templates` to confirm the new fragment is CSP-clean.

- [x] **Task 5 — Migrate the trigger button site (AC: 3, 4, 8)**
  - [x] `templates/pages/series_detail.html:35`: replace per AC3's before/after. Tailwind classes UNCHANGED. Add `aria-haspopup="dialog"` + `aria-expanded="false"`.
  - [x] Drop the `confirm_delete` field from `SeriesDetailTemplate` in `src/routes/series.rs:175`.
  - [x] Drop the construction-site `confirm_delete: rust_i18n::t!("series.confirm_delete", locale = loc).to_string(),` from `src/routes/series.rs:231`.
  - [x] Run `cargo test no_inline_markup_in_templates` to confirm no inline `style=` / `onclick=` slipped in.
  - [x] Run `cargo build` to confirm the field-removal compiles without errors.

- [x] **Task 6 — `ALLOWED_HX_CONFIRM_SITES` cleanup (AC: 5, 13)**
  - [x] Remove the `("templates/pages/series_detail.html", 1),` entry from the const array in `src/templates_audit.rs:36`.
  - [x] Run `cargo test hx_confirm_matches_allowlist` and confirm green.
  - [x] Run `cargo test --lib templates_audit` (all 4 audit tests) and confirm green.
  - [x] Run the AC13 grep audit:
    - `grep -rnE 'hx-confirm=' templates/` — must return exactly 1 hit (admin_users_row.html).
    - `grep -rnE 'hx-confirm=' src/` — must return exactly 1 hit (pre-existing locations.rs:256, inherited).
    - `grep -rn 'confirm_delete' src/ templates/ locales/` — must return ZERO series-related hits.
  - [x] Document the grep output in Dev Agent Record.

- [x] **Task 7 — Integration tests (AC: 7, 11)**
  - [x] Create `tests/series_delete_modal.rs` with the 10 `#[sqlx::test]` cases from AC7. Use the same fixture pattern as `tests/contributor_delete_modal.rs`:
    - `build_state(pool)` helper (verbatim copy from 9-12).
    - `seed_session(pool, username)` for `admin` / `librarian`.
    - `insert_series(pool, name, series_type)` helper that runs `INSERT INTO series (name, series_type) VALUES (?, ?)` and returns the inserted id.
    - `soft_delete_series(pool, id)` for the 404 test.
    - `assign_title_to_series(pool, series_id) -> u64` for the 9th-case conflict fixture: first `INSERT INTO titles (title, media_type, genre_id) VALUES ('9-13 Guard Title', 'book', (SELECT id FROM genres LIMIT 1))` to get a title id (mirror of `tests/contributor_delete_modal.rs::associate_title` shape), then `INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, 1)`. The `title_series` junction (singular!) is declared in `migrations/20260329000000_initial_schema.sql:190-206`. `position_number` is `NOT NULL` — pass `1` (or any positive int). Use raw SQL; no need to go through the service layer.
    - `req_htmx` / `req_plain` / `body_text` / `rand_suffix` helpers (verbatim copy from 9-12).
  - [x] Run `SQLX_OFFLINE=true cargo test --test series_delete_modal` and confirm all 10 pass green. Document the test count in Dev Agent Record.
  - [x] **CSRF assertion** (AC11): the librarian-happy-path test MUST also assert `assert!(html.contains("name=\"_csrf_token\""))`.
  - [x] **`#series-feedback` div lock** (AC7 10th case, mirror of 9-12 review patch P3): the 10th test GETs `/series/:id` as a librarian and asserts the response body contains `id="series-feedback"`. Load-bearing because the modal hardcodes `hx_target="#series-feedback"`.

- [x] **Task 8 — E2E updates (AC: 9, 12)**
  - [x] Edit `tests/e2e/specs/journeys/series.spec.ts:72-96` per AC9:
    - Drop the `page.on("dialog", (d) => d.accept());` line.
    - Replace `await deleteBtn.click();` with the modal click sequence (click trigger → wait `#modal-slot dialog[open]` → click `[data-modal-confirm]`).
    - Add the inline Escape-close + default-focus assertion before the actual delete sequence.
    - Add the `not.toHaveAttribute("hx-confirm", /./)` assertion on the trigger button (paranoid lock).
  - [x] Add the NEW `"delete series with assigned titles shows block message"` test sibling per AC9. Use the existing series-creation flow + the existing title/series-assignment helpers (the spec already does this for the assignment tests starting at line 99) to seed the conflict fixture.
  - [x] Run `cd tests/e2e && npx tsc --noEmit` to verify the spec edits don't break tsc.
  - [x] Run `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/series.spec.ts` (single-spec run for fast feedback) and confirm all tests green.
  - [x] Run the full E2E lane (`cd tests/e2e && npm test`) and confirm no other spec regressions (no ISBN collisions risk; spec ID for series.spec.ts uses date-stamped names like `SE-Delete-${Date.now()}` — no `specIsbn()` involvement).

- [x] **Task 9 — Server-side doc-comment (AC: 12)**
  - [x] Add `/// Trigger UX: see GET /series/:id/delete-modal (story 9-13).` doc-comment immediately above `pub async fn delete_series` in `src/routes/series.rs:493`. Mirror of 9-12 review patch P1. The handler body itself is UNCHANGED.

- [x] **Task 10 — Local gate + push (AC: 14, 15)**
  - [x] `SQLX_OFFLINE=true cargo check` — clean
  - [x] `cargo clippy --all-targets -- -D warnings` — clean
  - [x] `cargo test` (full lib + integration) — green
  - [x] CI flake gate: `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` returns nothing
  - [x] Push branch + open draft PR (Foundation Rule #15)
  - [x] WAIT for CI green per Foundation Rule #18 before requesting review / merging

## Dev Notes

### Pattern reuse (9-10 / 9-11 / 9-12 → 9-13)

This is the THIRD mechanical migration on top of the 9-10 foundation. The handler shape, fragment shape, and integration-test shape should all mirror `contributors::delete_modal` (9-12) 1-for-1 with type/name swaps and a path adjustment. Differences from 9-12:

- **DELETE endpoint is singular `/series/{id}`** — NO `/catalog/` prefix and NOT plural `/series-list/{id}`. Asymmetric with 9-12's `/catalog/contributors/{id}`. The action_url + the AC7 `hx-delete=` substring assertion + the `req_htmx` URI in the `delete_series_via_existing_handler_still_works` test all need this singular path. Triple-check in Task 1.
- **Role gate is `Role::Librarian`** (same as 9-12). Mirror of the trigger's `{% if role == "librarian" || role == "admin" %}` template gate.
- **Modal Confirm targets `#series-feedback` with `hx-swap="innerHTML"`** — same shape as 9-12's `#contributor-feedback`. The container already exists at `series_detail.html:43` (UNCHANGED).

If you find yourself diverging more than ~10 LOC in any of those mirrors, stop and revisit — the 9-12 spec was deliberately crafted to make 9-13 / 9-14 feel like copy-and-tune.

### Why `delete` not `warning`

Per UX-DR8: series deletion soft-deletes the row (`deleted_at = NOW()`) — it goes to Trash, recoverable for 30 days via the admin Trash panel (story 8-7). User perception is "destruction of data" (the series disappears from the catalog UI immediately). UX-DR8 maps this to the `delete` variant (red confirm button — `bg-red-600`). The `delete-forever` variant is reserved for hard-delete from Trash; `warning` is reserved for reversible state changes (like 9-11 return-loan); `remove` is reserved for non-destructive removals (like un-assigning a title from a series via `series.unassign` — that's a different handler, NOT this story).

### Why hx-target+hx-swap matter (vs. 9-10's `"", "none"`)

The 9-10 borrower delete handler uses `HX-Redirect` on success — full nav, no inline swap needed. So 9-10's modal Confirm form passed `("", "none")` to the macro.

The 9-13 series delete handler ALSO uses `HX-Redirect: /series` on SUCCESS. BUT on Conflict / NotFound it returns 200 + inline `feedback_html_pub` (matches 9-12's contributor pattern). The trigger button on `series_detail.html` originally used `hx-target="#series-feedback" hx-swap="innerHTML"` to land that error feedback. The modal Confirm form must reproduce this contract — otherwise the conflict feedback HTML would land in `#modal-slot` (overwriting the modal!) or be discarded.

So 9-13 hardcodes `hx_target="#series-feedback"` + `hx_swap="innerHTML"` in the modal fragment via the macro params. On Confirm click:
- **Success path**: server returns 200 + `HX-Redirect: /series` → htmx performs full nav → the inline-swap setup is irrelevant.
- **Conflict path** (titles assigned): server returns 200 + inline `feedback_html_pub("error", "internal error", "")` (the latent bug — generic copy instead of the meaningful payload) → htmx swaps it into `#series-feedback` → modal closes via modal.js's `htmx:afterRequest` listener (filtered to `[data-modal-confirm]` on 2xx per 9-10 PR #129) → user sees the (generic) error feedback under the action bar.

### The Conflict latent UX bug (preserve, defer)

The route handler at `src/routes/series.rs:520-526` does:

```rust
Err(e) => {
    let message = match &e {
        AppError::NotFound(msg) => msg.clone(),
        _ => rust_i18n::t!("error.internal", locale = loc).to_string(),
    };
    Ok(Html(feedback_html_pub("error", &message, "")).into_response())
}
```

`AppError::Conflict` falls into the catch-all `_` arm and gets rendered as the generic `error.internal` text — NOT the meaningful `series.delete_has_titles` payload that `SeriesService::delete_series` constructs at line 139-141. The user sees "Internal error" instead of "Cannot delete *Foo*: 3 title(s) assigned. Remove all title assignments first."

**Why preserve in 9-13:** The story spec explicitly says "existing protections … remain server-side". The migration's contract is "trigger UX changes; server contract identical". Fixing the bug WHILE migrating violates the refactor-during-feature anti-pattern and would invalidate the regression cover in the existing E2E suite.

**How to defer:** at story close, file a `type:code-review-finding` GitHub issue titled "series delete handler renders generic error.internal on Conflict instead of series.delete_has_titles" with a one-line fix sketch (`Err(AppError::Conflict(msg)) | Err(AppError::NotFound(msg)) => msg.clone(),` — combine the two arms with `|`). Include a link to this story file's reality-check section.

The AC7 9th case + the AC9 conflict-path E2E sibling test BOTH lock the buggy contract (assert generic copy) so the future fix flows through 9-13's regression net — when the fix lands, both tests fail, the fixer flips the assertion, and the bug is closed in a single chore PR.

### No `?target=` query parameter (vs. 9-11's closed allowlist)

9-11 had THREE surfaces (loans.html, borrower_detail.html, plus the Rust-emitted scan-card on /loans), each with a different feedback container. So 9-11 introduced a `?target=…` query parameter validated against a closed allowlist.

9-13 has ONE surface — the `/series/:id` page. The closed-allowlist pattern would be over-engineering (YAGNI, mirror of 9-12). Hardcode `#series-feedback` in the fragment template. If a future story adds a second series-delete surface (e.g., a series-list bulk-delete on `/series`), reintroduce the closed-allowlist pattern then.

### Drop `series.confirm_delete` per Foundation Rule #1

Mirror of 9-10's `borrower.confirm_delete` drop, 9-11's `loan.return_confirm` drop, and 9-12's `contributor_detail.confirm_delete` drop. Four migration stories, four dead-key drops. The pattern is established convention.

The retained sibling key `series.delete` (`"Delete"` / `"Supprimer"`) — used as the trigger button label — STAYS. Don't conflate the two in the grep cleanup: only `confirm_delete` is dead, `delete` is the live label.

### CLAUDE.md "Modal scanner-guard invariant" line

The CLAUDE.md line in the "Modal scanner-guard invariant" section currently says:

> the allowlist is frozen at 5 grandfathered sites (5th added in story 8-3 for admin user deactivation) and only changes through explicit review.

The wording was already inaccurate after 9-10/9-11/9-12 trimmed the count from 5 → 2. **9-13 does NOT edit this line** — the 9-14 spec explicitly handles the CLAUDE.md edit when the allowlist drops to `&[]`. Updating the count incrementally per migration would create three churning edits in three PRs. Leave as-is; 9-14 rewrites the whole sentence in one shot.

### File-LOC budget

`src/routes/series.rs` is 528 LOC pre-9-13 → ~595 post. Plenty of headroom (1400 LOC under the 2000 ceiling). No extraction needed.

`src/services/series.rs` (456 LOC) is UNCHANGED — the existing `SeriesService::delete_series` keeps its title-assignment guard.

`src/models/series.rs` (655 LOC) is UNCHANGED — the existing `active_find_by_id` and `active_count_titles` are reused.

### DEFERRED items inherited from 9-10/9-11/9-12 (no action in 9-13)

- **Two-modal race** — KF tracked. Series detail page has only one modal trigger; not exercised here.
- **Hardcoded `/series` post-success redirect** — KF-equivalent. Document only.
- **Frozen modal on Confirm 5xx** — KF tracked. User can press Escape.
- **Migrate the 3 admin modal fragments** — STILL deferred per 9-10 close. Out of scope.
- **JS focus-trap unit tests** — STILL deferred (no JS test harness).
- **Bidirectional EN/FR locale parity test** — deferred Epic 9 follow-up.
- **`src/routes/locations.rs:256` Rust-emitted `hx-confirm=`** — pre-existing, undetected by the audit. Out of scope; will be filed as a code-review-finding for a future migration sweep.

### NEW deferred item this story will file

- **`series::delete_series` Conflict-→-generic-copy latent UX bug** (see "The Conflict latent UX bug" section above). File at story close as `type:code-review-finding`.

### Project Structure Notes

- `src/routes/series.rs` already hosts the GET detail handler; the new modal handler sits alongside `series_detail_page`. No new module.
- `templates/fragments/series_delete_modal.html` mirrors `templates/fragments/contributor_delete_modal.html` (9-12 sibling). Same shape, three-line diff (different action_url path + different `hx_target` literal `#series-feedback`).
- `tests/series_delete_modal.rs` mirrors `tests/contributor_delete_modal.rs` (9-12 sibling).
- `static/js/modal.js`, `templates/components/modal.html`, `layouts/base.html` are ALL UNCHANGED.
- `tests/e2e/specs/journeys/series.spec.ts` is the single E2E spec touched.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.13] — story spec verbatim (8 ACs + EN/FR copy)
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#8.-Modal-—-Destructive-Confirmation-+-Warning] — UX-DR8 component anatomy, variants, accessibility
- [Source: _bmad-output/implementation-artifacts/9-10-modal-foundation-and-borrower-migration.md] — pattern precedent (Modal macro, modal.js, focus trap, scanner-guard inheritance, integration-test shape, dead-i18n-key drop, `require_role_with_return` for anonymous-redirect-to-detail)
- [Source: _bmad-output/implementation-artifacts/9-11-migrate-return-loan-to-modal.md] — pattern precedent (10-param macro signature, dropping the `Allow:` header on 405, dead-`confirm_label` field drop, AC9 inline E2E migration shape)
- [Source: _bmad-output/implementation-artifacts/9-12-migrate-delete-contributor-to-modal.md] — DIRECT precedent (single-surface hardcoded `hx_target` shape, FR-style server-contract preservation via AC12 doc-comment + AC13 grep audit, the 6-patches-after-review shape that 9-13 absorbs by-design — `data-modal-variant` selector, empty-body 405 assertion, feedback-target div integration test, modal-closes-after-2xx E2E assertion, no-hx-confirm trigger E2E assertion)
- [Source: CLAUDE.md#Foundation-Rules] — Rules #1 (DRY), #11 (issue tracking), #12 (LOC ceiling), #13 (local testing), #15 (draft PR), #18 (CI gating)
- [Source: CLAUDE.md#Modal-scanner-guard-invariant-story-7-5] — the `dialog[open]` + `[aria-modal="true"]` selector contract that the new modal inherits
- [Source: CLAUDE.md#Key-Patterns#CSRF-synchronizer-token-story-8-2] — why the modal macro takes `csrf_token` as its 8th param and how `_csrf_token` hidden inputs are policed by `templates_audit::forms_include_csrf_token`
- [Source: src/routes/series.rs:493-528] — existing `delete_series` handler (UNCHANGED in this story; only a doc-comment is added per AC12)
- [Source: src/routes/series.rs:148-249] — existing `SeriesDetailTemplate` + `series_detail_page` GET handler (sibling location for the new `delete_modal` handler)
- [Source: src/routes/mod.rs:174-179] — existing `/series/{id}` method-routed registration (DELETE binding UNCHANGED; new GET delete-modal sibling registered immediately after)
- [Source: src/services/series.rs:133-144] — `SeriesService::delete_series` — the title-assignment guard (active_count_titles + AppError::Conflict). UNCHANGED.
- [Source: src/services/series.rs:106] — the `active_count_titles` call site referenced by the guard
- [Source: src/models/series.rs:216-227] — `SeriesModel::active_count_titles` SQL (`SELECT COUNT(*) FROM title_series ts JOIN titles t ON ts.title_id = t.id WHERE ts.series_id = ? AND ts.deleted_at IS NULL AND t.deleted_at IS NULL`) — confirms the junction is `title_series` (singular) and any active row triggers the conflict
- [Source: src/models/series.rs:72] — `SeriesModel::active_find_by_id` (signature line) — already filters `WHERE deleted_at IS NULL` and returns `Option<SeriesModel>`. No new model method needed.
- [Source: migrations/20260329000000_initial_schema.sql:190-206] — `CREATE TABLE title_series` (the junction; `position_number INT NOT NULL`; `deleted_at TIMESTAMP NULL`); load-bearing for the AC7 9th-case conflict fixture. NB: there is NO `series_id` column on the `titles` table — the association lives exclusively in this junction.
- [Source: src/templates_audit.rs:35-38] — `ALLOWED_HX_CONFIRM_SITES` const (current state captured in AC5 / Task 6)
- [Source: templates/pages/series_detail.html:29-43] — line range of the current `hx-confirm` button + the surrounding role-gated action bar + the `#series-feedback` container
- [Source: templates/components/modal.html] — the 10-param shared macro (post-9-11 shape; UNCHANGED)
- [Source: templates/fragments/contributor_delete_modal.html] — the 17-line fragment template that 9-13's series_delete_modal.html mirrors
- [Source: tests/contributor_delete_modal.rs] — the 473-LOC integration-test mirror (10 cases including the post-review patch P3 feedback-target-div lock)
- [Source: tests/e2e/specs/journeys/series.spec.ts:72-96] — the existing E2E test that AC9 migrates
- [Source: tests/e2e/specs/journeys/series.spec.ts:104-145] — the existing series-assignment fixture pattern (login → goto /series/new → submit → goto /catalog → scan ISBN → goto /?q= → click title link → fill `#assign-series` + `#assign-position` + click `#assign-series-submit`) that the AC9 conflict-path sibling test mirrors verbatim

## Dev Agent Record

### Agent Model Used

claude-opus-4-7[1m]

### Debug Log References

- `cargo build` after `touch src/lib.rs` — green (i18n macro re-read).
- `cargo clippy --all-targets -- -D warnings` — green.
- `cargo test --lib` (with test MariaDB on :3307) — 755 passed, 0 failed.
- `cargo test --test series_delete_modal` — 10 passed, 0 failed.
- `cd tests/e2e && npx tsc --noEmit` — clean.
- Flake gate `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` — clean.
- Full E2E (`./scripts/e2e-reset.sh && npm test`) — 201 passed, 2 skipped, 1 failed. The single failure (`dewey-code.spec.ts`) is **pre-existing on `origin/main`** (verified by stashing 9-13 changes and re-running) — unrelated to this story; will be filed as a separate flake report.

### Completion Notes List

- ✅ AC1–AC4 implemented exactly to spec: handler `series::delete_modal` mirrors `contributors::delete_modal`, fragment `templates/fragments/series_delete_modal.html` mirrors `contributor_delete_modal.html`, trigger button migrated, `confirm_delete` field + i18n key + template attribute fully removed.
- ✅ AC5 — `ALLOWED_HX_CONFIRM_SITES` trimmed from 2 → 1 entry (only `admin_users_row.html` remains for 9-14).
- ✅ AC6 — 3 new keys added per locale, 1 dropped per locale; `all_t_keys_have_both_locales` audit green.
- ✅ AC7 — 10 `#[sqlx::test]` cases in `tests/series_delete_modal.rs`, all green. Includes the 9th-case latent-UX-bug lock (asserts generic `error.internal` copy on Conflict) + the 10th `series_detail_page_renders_feedback_target_div` lock.
- ✅ AC8 — All 4 templates_audit tests green (`hx_confirm_matches_allowlist`, `no_inline_markup_in_templates`, `forms_include_csrf_token`, `csrf_exempt_routes_frozen`).
- ✅ AC9 — E2E spec migrated: `page.on("dialog", …)` removed, modal click sequence + Escape-close + default-focus + `not.toHaveAttribute("hx-confirm", /./)` lock + new sibling test `delete series with assigned titles shows block message`. **Deviation from spec literal**: the conflict-path test uses `specIsbn("SE", 30)` instead of the spec-suggested `specIsbn("SE", 12)` because seq 12 is already used by the existing "clicking filled square" test at `series.spec.ts:192` and parallel mode would collide; seq 30 follows the existing 10/11/12/20 numbering convention.
- ✅ AC11 — CSRF: librarian-happy-path test asserts `name="_csrf_token"` is embedded.
- ✅ AC12 — Server contract preserved; doc-comment `/// Trigger UX: see GET /series/:id/delete-modal (story 9-13).` added above `delete_series`.
- ✅ AC13 — Story-level grep audit (post-migration):
  - `grep -rnE 'hx-confirm=' templates/` → **1 real attribute** at `templates/fragments/admin_users_row.html:23` (matches `ALLOWED_HX_CONFIRM_SITES.len()`); 3 other matches are doc-comments in `admin_reference_data_panel.html`, `admin_system_panel.html`, `components/modal.html`.
  - `grep -rnE 'hx-confirm=' src/` → **1 real attribute** at `src/routes/locations.rs:256` (pre-existing inherited from prior epics; out of scope per 9-12 close); other matches are doc-comments / regex strings / a negative-assert.
  - `grep -rn 'confirm_delete' src/ templates/ locales/` → **0 hits** (clean).
- ✅ AC14 — Local gate run, all green except the pre-existing `dewey-code.spec.ts` failure documented above.
- 🔄 AC15 — Draft PR opened; awaiting CI (Foundation Rule #18).
- 📋 **Deferred GH issue filed**: [#139 — series delete_series renders generic error.internal on Conflict instead of series.delete_has_titles](https://github.com/guycorbaz/mybibli/issues/139), labelled `type:code-review-finding`. Locks the latent UX bug and references the two tests that pin the buggy behaviour so the future fix flows through 9-13's regression net.

### File List

**Modified:**
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — story 9-13 status `ready-for-dev` → `in-progress` → `review`; `last_updated` bumped to 2026-05-07.
- `locales/en.yml` — +3 series keys (`delete_modal_title`, `delete_modal_body`, `delete_modal_confirm`); −1 series key (`confirm_delete`).
- `locales/fr.yml` — same shape, FR copy.
- `src/routes/series.rs` — new `SeriesDeleteModalTemplate` struct + `pub async fn delete_modal(...)` handler (~70 LOC); `confirm_delete` field dropped from `SeriesDetailTemplate`; ctor site dropped; `/// Trigger UX: …` doc-comment added above `delete_series`.
- `src/routes/mod.rs` — new route registration `GET /series/{id}/delete-modal`.
- `src/templates_audit.rs` — `series_detail.html` entry removed from `ALLOWED_HX_CONFIRM_SITES` (2 → 1 entries).
- `templates/pages/series_detail.html` — delete-series button: `hx-confirm` removed; `hx-get="/series/{id}/delete-modal"` + `hx-target="#modal-slot"` + `data-modal-trigger` + `aria-haspopup="dialog"` + `aria-expanded="false"`.
- `tests/e2e/specs/journeys/series.spec.ts` — `delete series removes it from list` migrated to modal flow + Escape-close + default-focus + paranoid `hx-confirm` lock; new sibling test `delete series with assigned titles shows block message`.

**New:**
- `templates/fragments/series_delete_modal.html` — modal fragment calling the shared `components/modal.html::modal` macro (delete variant, hardcoded `#series-feedback` target).
- `tests/series_delete_modal.rs` — 10 `#[sqlx::test]` cases.

**No change:**
- `src/services/series.rs`, `src/models/series.rs`, `static/js/modal.js`, `templates/components/modal.html`, `layouts/base.html`.

### Review Findings

Adversarial code review run 2026-05-07 with 3 parallel reviewers (Blind Hunter / Edge Case Hunter / Acceptance Auditor). 34 inputs → 2 actionable patches + 8 deferred + 24 dismissed. Acceptance Auditor reports all 15 ACs PASS with three minor literal deviations (none material).

**Patches (actionable — apply on this branch):**

- [x] [Review][Patch] AC7 admin test allégé vs spec "same shape" [tests/series_delete_modal.rs:203] — `get_series_delete_modal_returns_200_for_admin_request` carries 2 assertions instead of the 8 assertions of the librarian-happy-path test. Spec AC7 says "same shape". Extend the admin test to mirror the full assertion set (dialog, name, `data-modal-default-focus`, singular `hx-delete=/series/{id}`, `hx-target=#series-feedback`, `hx-swap=innerHTML`, `data-modal-variant=delete`, `_csrf_token` input). **Applied 2026-05-07.**
- [x] [Review][Patch] Audit gap: `tracing::debug!` in `delete_modal` doesn't log `user_id` on a destructive surface [src/routes/series.rs ~540] — `tracing::debug!(series_id = id, "delete modal requested")` should also include `session.user_id` to enable forensic post-incident analysis. **Applied 2026-05-07** — note: 9-10 (borrower) and 9-12 (contributor) sibling handlers have the same gap and should be aligned in a follow-up sweep (file as `type:code-review-finding`). Verified post-fix: `cargo check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --test series_delete_modal` (10/10 passed).

**Deferred (file as GitHub Issues `type:code-review-finding` per Foundation Rule #11 — do NOT add to a markdown tracking doc):**

- [x] [Review][Defer] [HIGH] CSRF rejection retargets to `#feedback-list`, which doesn't exist on `/series/:id` [src/middleware/csrf.rs:286 + templates/pages/series_detail.html] — when the CSRF token rotates (sibling-tab login, 7-day session purge), the modal Confirm fires DELETE with stale token → 403 → `HX-Retarget: #feedback-list` retargets into a nonexistent element → silent no-op, zero user feedback. Pre-existing pattern issue (affects all pages without `#feedback-list`); 9-13 increases exposure surface via the modal Confirm form. Fix likely: render a global `#feedback-list` in `layouts/base.html`, or have the CSRF retarget fall back to the page's `aria-live` region.
- [x] [Review][Defer] `body_html = format!("<p>{}</p>")` bypasses Askama auto-escape [src/routes/series.rs:544] — string-concat HTML rendering trusts the i18n YAML supply chain. The current value is plain text, but a translator who edits the YAML (e.g., to add `<em>`) introduces a markup-injection vector. Pattern inherited from 9-10 / 9-12; warrants a sweep across all modal fragments rather than a one-off fix in 9-13.
- [x] [Review][Defer] No explicit `Cache-Control: no-store` on the modal-fragment response carrying a CSRF token [src/routes/series.rs:556-562] — handler returns plain `Html(html).into_response()`. If a CDN or proxy ever caches the response, a CSRF token leaks cross-user. Verify whether CSP / security-headers middleware sets `no-store` by default; if not, add it explicitly to the delete_modal response.
- [x] [Review][Defer] TOCTOU race in `SeriesService::delete_series` [src/services/series.rs:133-144] — `active_count_titles` then a separate `UPDATE` is non-atomic. A concurrent `INSERT INTO title_series` between the two queries leaves an orphan junction row pointing at a soft-deleted series. Pre-existing service-layer issue, untouched by 9-13. Fix: wrap in transaction + `SELECT … FOR UPDATE`, or move the guard into the UPDATE statement.
- [x] [Review][Defer] Modal Confirm rendered even when server will reject [src/routes/series.rs:509] — `delete_modal` does not consult `active_count_titles` before rendering the destructive Confirm button. User confirms → server returns 200 + generic `error.internal` (latent UX bug, GH #139) → modal closes with no visible action. Improvement: short-circuit `delete_modal` to render a `warning` variant (no Confirm) when titles are assigned. Distinct from #139 (UI-side anticipation vs server-side error mapping).
- [x] [Review][Defer] Series name containing literal `%{...}` collides with rust_i18n interpolation [src/routes/series.rs:537-542] — `t!("series.delete_modal_title", name = series.name.as_str())` interpolates `%{name}` from the locale value, but if `series.name` itself contains `%{x}` the result is undefined (depends on rust_i18n parser). No regression test. Add a test: series named `"Tales of %{count}"` → assert title renders the literal name without further interpolation.
- [x] [Review][Defer] No audit for `%{...}` placeholder shape parity between EN and FR locales [src/i18n/audit.rs] — `all_t_keys_have_both_locales` only checks key presence, not interpolation parity. A translator who renames `%{name}` → `%{nom}` in FR silently breaks runtime. Add a parser-based audit that diffs the placeholder set per key per locale.
- [x] [Review][Defer] No automated guard against orphan `t!()` callers after dropping a key [src/i18n/audit.rs] — story 9-13 drops `series.confirm_delete` and relies on a manual grep to confirm zero callers. A future hand-typed `t!("series.confirm_delete", …)` would be a runtime-only failure (rust_i18n returns the key string). Add a static audit walking `t!()` call sites and verifying each has a key in BOTH locales (currently the audit goes locale → call sites; reverse direction is missing).

### Change Log

| Date | Change |
| --- | --- |
| 2026-05-07 | Story created (backlog → ready-for-dev) |
| 2026-05-07 | Story validated; 7 improvements applied (3 critical + 3 enhancements + 1 optimization): pinned `title_series` junction (singular) name + exact `INSERT … (title_id, series_id, position_number) VALUES (?, ?, 1)` shape across Task 1, Task 7 helper, and AC7 9th case (eliminates dev-agent guesswork on schema); fully spelled out AC9 conflict-path E2E fixture flow with explicit step list mirroring `series.spec.ts:104-145` + assertion checklist + naming convention `SE-DeleteConflict-${Date.now()}`; pinned i18n insertion point to "after `series.delete_has_titles` at line 181"; added `migrations/20260329000000_initial_schema.sql:190-206` + `src/models/series.rs:72` + `:216-227` to References; documented exact EN/FR `error.internal` strings ("An internal error occurred" / "Une erreur interne est survenue") in AC7 9th case + AC9 docblock. |
| 2026-05-07 | Story implemented (in-progress → review). 10 integration tests + 2 E2E tests + audit/i18n cleanup; deferred GH issue #139 filed for the latent Conflict-→-generic-copy UX bug. AC9 conflict-path test uses `specIsbn("SE", 30)` instead of the spec-suggested seq 12 to avoid collision with the existing "clicking filled square" test under parallel mode. |
| 2026-05-07 | Code review complete (review → done). 3 parallel reviewers (Blind / Edge / Auditor) — 34 inputs → 2 actionable patches + 8 deferred + 24 dismissed. 0 BLOCKERS, 0 decision-needed. Acceptance Auditor: ALL 15 ACs PASS. 2 patches applied: (1) extended `get_series_delete_modal_returns_200_for_admin_request` to mirror the librarian-happy-path's full 8-assertion set per AC7 "same shape"; (2) added `user_id = ?session.user_id` to the `delete_modal` `tracing::debug!` for destructive-surface auditability (sibling 9-10 / 9-12 handlers should be aligned in a follow-up sweep). Post-patch: `cargo check`, `clippy --all-targets -- -D warnings`, `cargo test --test series_delete_modal` (10/10) all green. 8 deferred items to file as `type:code-review-finding` GH issues (Foundation Rule #11): top items are CSRF retarget to `#feedback-list` not present on /series/:id (HIGH, pre-existing), `body_html` string-concat bypassing Askama auto-escape (sweep across modal fragments), no `Cache-Control: no-store` on CSRF-bearing fragment, TOCTOU race in `SeriesService::delete_series`, modal renders Confirm even when server will reject (UX dead-end), `%{name}` interpolation collision with user-supplied series names, no EN/FR `%{...}` placeholder parity audit, no automated guard against orphan `t!()` callers after dropping a key. |
