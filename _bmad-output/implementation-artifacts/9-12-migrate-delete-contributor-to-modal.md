# Story 9.12: Migrate hx-confirm — delete contributor

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As the project maintainer,
I want the delete-contributor flow on `/contributor/:id` migrated from `hx-confirm=` to the UX-DR8 Modal component built in 9.10,
so that the destructive-action pattern is enforced, the FR54 server-side protection (cannot delete a contributor with active title references) keeps working unchanged, and one more grandfathered site exits the `ALLOWED_HX_CONFIRM_SITES` allowlist (3 → 2 entries).

## ⚠️ Existing-code reality check

Before writing a single line, walk the code that 9-12 touches and verify the assumptions below — they are LOCKED IN by the 9-11 close (current main):

- **Modal macro is shipped and now takes 10 params.** `templates/components/modal.html` (39 LOC, 60 LOC ceiling) takes: `variant` / `title` / `body_html` (RAW — caller must escape interpolated user data) / `confirm_label` / `cancel_label` / `action_url` / `action_method` (`"DELETE"` or `"POST"`) / `csrf_token` / `hx_target` / `hx_swap`. The last two were added in 9-11; passing `("", "none")` reproduces pre-9-11 behavior (used by the 9-10 borrower modal because that flow uses HX-Redirect on success). 9-12 will pass `("#contributor-feedback", "innerHTML")` — see "Why hx-target+hx-swap matter here" below. This story does NOT modify the macro.

- **`static/js/modal.js` is shipped (197 LOC post-9-10/9-11 patches).** Focus trap + Escape close + mousedown-tracking backdrop close + `[data-modal-trigger][data-pressed="true"]` focus-restoration + `htmx:afterRequest` filter to `form` / `[data-modal-confirm]` are all in place. This story does NOT modify modal.js. Verify via grep at story close.

- **`<div id="modal-slot">` is in `layouts/base.html`** (sibling of `#admin-modal-slot`). Already loaded on every page that extends the layout, including `/contributor/:id`. No layout edit needed in this story.

- **One `hx-confirm=` site in scope:**
  - `templates/pages/contributor_detail.html:14` — delete-contributor button inside the librarian/admin action bar. (Epic 9.12 spec said "line 15" — actual file has it at line 14. The exact line is documented here for the dev agent; the current file is 41 LOC total.)

- **Current `ALLOWED_HX_CONFIRM_SITES`** in `src/templates_audit.rs:35-39` has 3 entries × 1 occurrence = 3 grandfathered. After 9-12: `contributor_detail.html` entry removed entirely. Result: 2 entries × 1 occurrence = 2 grandfathered (`series_detail.html` for 9.13, `admin_users_row.html` for 9.14). The const NEVER carries a `count == 0` entry — the audit's positive assertion shape requires entries to be deleted, not zeroed.

- **DELETE handler `delete_contributor`** lives in `src/routes/catalog.rs:1663-1698` — NOT in `src/routes/contributors.rs` (which only hosts the GET detail page). The DELETE handler is **unchanged** by this story. Key contract:
  - Endpoint: `DELETE /catalog/contributors/{id}` (registered at `src/routes/mod.rs:75-76`). Note the **plural `contributors/` and the `/catalog/` prefix** — asymmetric with the GET detail endpoint `/contributor/{id}` (singular, no `/catalog/` prefix). This asymmetry is pre-existing and OUT OF SCOPE.
  - **Role gate: `Role::Librarian`** (NOT `Role::Admin` like 9-10's `delete_borrower`). Mirrored by the modal handler — see AC1.
  - On success (HTMX): 200 + `HX-Redirect: /catalog` → full-page nav.
  - On success (non-HTMX): `Redirect::to("/catalog")`.
  - **On Conflict (FR54)**: returns 200 + `Html(feedback_html("error", &message, ""))` — INLINE HTML, NOT 409. The 200 status is intentional so HTMX performs the swap; HTMX suppresses 4xx swaps by default unless `hx-target-4xx` is set. The conceptual "409 Conflict" referenced in epics.md is the AppError variant carried internally by `ContributorService::delete_contributor`, but the wire status is 200. The modal Confirm form must therefore receive a target so the conflict feedback HTML lands somewhere visible.
  - On NotFound: same shape (200 + inline error feedback).
  - The conflict message comes from `error.contributor.has_titles` (`"Cannot delete %{name}. This contributor is associated with %{count} title(s). …"`).

- **`contributor_detail.confirm_delete`** in `locales/en.yml:138` and `locales/fr.yml:138` (`"Delete this contributor?"` / `"Supprimer ce contributeur ?"`) is the OLD plain-confirm copy. This story DROPS it (zero callers after migration; mirror of 9-10's `borrower.confirm_delete` drop and 9-11's `loan.return_confirm` drop). The sibling key `contributor_detail.delete` (`"Delete"` / `"Supprimer"`) — used as the trigger button label — is RETAINED.

- **`contributor_detail.html` HAS no inline-form-rendered fragment fallback.** The HTMX path of `contributor_detail` in `src/routes/contributors.rs:55-57` builds a fragment via `contributor_detail_fragment(...)`, but that fragment does NOT include the delete-button row. The migration is therefore template-only: edit `templates/pages/contributor_detail.html` once, no Rust-emitted HTML to migrate. This is the OPPOSITE of the 9-11 spec drift (where `loan_row_html` in `loans.rs:330` had a 3rd hidden `hx-confirm=` caller). For 9-12, run `grep -rn 'hx-confirm=' src/ templates/` after migration and assert exactly 2 hits remain (the templates entries for series + admin_users_row), no Rust-emitted hits.

- **`role` gate on the delete trigger BUTTON itself** is `{% if role == "librarian" || role == "admin" %}` (`templates/pages/contributor_detail.html:11`). This stays — anonymous users never see the trigger, so the modal handler never receives an anonymous request from a real user. The `Role::Librarian` gate on the modal handler is the second layer (defense-in-depth: a malicious anonymous user typing `/contributor/42/delete-modal` directly hits a 303 → /login).

## Acceptance Criteria

1. **AC1 — NEW handler `GET /contributor/:id/delete-modal`** in `src/routes/contributors.rs` (sibling of `contributor_detail`):
   - Returns the rendered modal fragment via `templates/components/modal.html`'s `modal::modal` macro from 9-10 (now 10-param shape post-9-11), variant `delete` (true soft-delete — the contributor row goes to Trash, mirroring borrower delete; `delete-forever` is reserved for hard-delete from the admin Trash panel; see "Why `delete` not `warning`" in Dev Notes).
   - Pre-translates 4 i18n keys: title (`contributor.delete_modal_title` — `"Delete contributor %{name}?"` interpolated via `t!(..., name = …)`, mirroring 9-10's `borrower.delete_modal_title` interpolation pattern), body (`contributor.delete_modal_body` — `"Linked titles will lose this contributor unless re-assigned."`), confirm (`contributor.delete_modal_confirm` — `"Delete"`), cancel (`common.cancel` — already shipped by 9-10).
   - **Role gate**: `session.require_role_with_return(Role::Librarian, &format!("/contributor/{id}"))?` — mirrors the trigger's `{% if role == "librarian" || role == "admin" %}` gate (admin > librarian, both pass). The `_with_return` variant ensures an anonymous direct-URL hitter lands back on the contributor detail page after login, NOT on /home (mirror of 9-10 borrower delete_modal pattern).
   - Returns 404 if the contributor is soft-deleted or not found (`ContributorModel::find_by_id` already filters `WHERE deleted_at IS NULL` and returns `Option<ContributorModel>`).
   - Direct browser navigation (no `HX-Request` header) returns 405 Method Not Allowed (mirrors 9-11's modal route shape — the modal fragment is meaningless without page context). **No `Allow:` response header, empty body** — the 9-11 patched-form is `return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());` (single line, no header tuple, no body). Verify against `src/routes/loans.rs:309` for the canonical shape. Rationale: `Allow: GET` self-contradicts 405; we DO support GET for HTMX, the 405 here means "wrong request shape" (browser nav vs HTMX), not "wrong method".
   - **No `?target=` query parameter** in 9-12 (single surface). The modal fragment hardcodes `hx_target="#contributor-feedback"` and `hx_swap="innerHTML"` via the macro's 9-th and 10-th params. Document the divergence from 9-11 in Dev Agent Record (9-11 needed a closed allowlist for 3 surfaces; 9-12 has 1 surface, YAGNI on the allowlist). If a future story adds a second contributor-delete surface (e.g., a contributor-list bulk action), reintroduce the closed-allowlist pattern then — not now.
   - **HTML-escape the contributor name?** NO — pass the RAW name into `t!("contributor.delete_modal_title", name = …)`. Askama's default auto-escape on `{{ title }}` inside the modal macro handles HTML safety. Pre-escaping would double-escape (`<` → `&lt;` → `&amp;lt;`). Mirror of 9-10's borrower-name handling. Verified by AC7's `get_contributor_delete_modal_html_escapes_contributor_name` test.

2. **AC2 — NEW fragment template `templates/fragments/contributor_delete_modal.html`** (mirror of `templates/fragments/borrower_delete_modal.html` from 9-10, ~17 LOC):
   - Imports the shared macro: `{% import "components/modal.html" as modal %}`.
   - Calls: `{% call modal::modal("delete", title, body_html, confirm_label, cancel_label, action_url, "DELETE", csrf_token, "#contributor-feedback", "innerHTML") %}{% endcall %}`.
   - The `action_url` handler-side is `format!("/catalog/contributors/{}", contributor.id)` — the **plural-`contributors/` + `/catalog/` prefix** from the existing route registration in `src/routes/mod.rs:75-76`. NOT `/contributor/{id}` (which is the GET detail path, singular). Easy off-by-one trap — verify in Task 1 by reading `mod.rs` directly.
   - The `body_html` is built handler-side as `format!("<p>{}</p>", body_text)` after pulling `body_text` out of `t!()`; the i18n value carries no user-supplied interpolation, so no escape is needed (mirror of 9-10).

3. **AC3 — Migrate `templates/pages/contributor_detail.html:14`** delete-contributor button:
   - Before:
     ```html
     <button hx-delete="/catalog/contributors/{{ contributor.id }}" hx-confirm="{{ confirm_delete }}"
             hx-target="#contributor-feedback" hx-swap="innerHTML"
             class="px-3 py-1.5 text-sm font-medium text-red-600 dark:text-red-400 border border-red-300 dark:border-red-700 rounded-md hover:bg-red-50 dark:hover:bg-red-900/20">
         {{ delete_label }}
     </button>
     ```
   - After:
     ```html
     <button hx-get="/contributor/{{ contributor.id }}/delete-modal"
             hx-target="#modal-slot" hx-swap="innerHTML"
             data-modal-trigger aria-haspopup="dialog" aria-expanded="false"
             class="px-3 py-1.5 text-sm font-medium text-red-600 dark:text-red-400 border border-red-300 dark:border-red-700 rounded-md hover:bg-red-50 dark:hover:bg-red-900/20">
         {{ delete_label }}
     </button>
     ```
   - Tailwind classes unchanged (visual identity preserved). `hx-disabled-elt` was NOT used here (unlike loans.html / borrower_detail.html), so no need to restore it (the 9-11 code-review patch about restoring `hx-disabled-elt` only applied to the return-loan trigger; double-click protection on a delete-trigger is reasonable but introducing it here would be a new behavior, OUT OF SCOPE).
   - `aria-haspopup="dialog"` + `aria-expanded="false"` are the 9-11 a11y standard for modal triggers (added by the 9-11 code-review patch). Apply them here too.

4. **AC4 — Drop the now-dead `confirm_delete` field** from `ContributorDetailTemplate` in `src/routes/contributors.rs:35` and the construction site at line 78-79. Foundation Rule #1 (DRY) — the field is dead immediately after migration (only the dropped `hx-confirm` attribute referenced it). Mirror of 9-10's `BorrowerDetailTemplate.confirm_delete` drop and 9-11's `LoansTemplate.confirm_label` drop.
   - Verify by grep: `grep -rn 'confirm_delete\|contributor_detail\.confirm_delete' src/ templates/ locales/` should return ZERO hits after the field, the i18n key, and the template attribute are all removed.

5. **AC5 — Update `src/templates_audit.rs::ALLOWED_HX_CONFIRM_SITES`**:
   - Before:
     ```rust
     ("templates/pages/contributor_detail.html", 1),
     ("templates/pages/series_detail.html", 1),
     ("templates/fragments/admin_users_row.html", 1),
     ```
   - After:
     ```rust
     ("templates/pages/series_detail.html", 1),
     ("templates/fragments/admin_users_row.html", 1),
     ```
   - Total entries: 3 → 2. Total occurrences: 3 → 2. The allowlist will reach `&[]` at 9-14 close.
   - `cargo test hx_confirm_matches_allowlist` MUST pass with the trimmed array.
   - **Audit doc-comment** at `src/templates_audit.rs:30-34` stays as-is (the wording about "the only templates allowed to carry this attribute" remains accurate; the migration count narrative belongs in CLAUDE.md and the story spec, not in the audit comment).

6. **AC6 — i18n: 3 NEW keys + 1 DROPPED key per locale** (EN + FR):
   - **NEW** under `contributor:` block (sibling of existing `contributor.added`, `contributor.duplicate`, etc. in `locales/en.yml:99-110`):
     - `delete_modal_title: "Delete contributor %{name}?" / "Supprimer le contributeur %{name} ?"`
     - `delete_modal_body: "Linked titles will lose this contributor unless re-assigned." / "Les titres liés perdront ce contributeur sauf s'il est réassigné."`
     - `delete_modal_confirm: "Delete" / "Supprimer"`
   - **DROPPED**: `contributor_detail.confirm_delete` (EN: `"Delete this contributor?"` / FR: `"Supprimer ce contributeur ?"`) — zero callers after AC3/AC4 (Foundation Rule #1 dead-key drop, mirror of 9-10's `borrower.confirm_delete` and 9-11's `loan.return_confirm`).
   - **REUSED** (no edits): `common.cancel` (shipped by 9-10), `contributor_detail.delete` (the trigger button label, KEPT).
   - Run `cargo test all_t_keys_have_both_locales` (the actual EN/FR parity test, in `src/i18n/audit.rs:186`) to confirm every `t!()` call site has a key in both locale files. NOTE: this audit is **one-way** (it only checks that keys called via `t!()` exist in both locales; it does NOT catch a key present in only one locale that is NEVER called from `t!()`). The 9-11 review filed a deferred GH issue to add a bidirectional check; until then, eyeball-diff the EN/FR additions and removals.
   - Run `touch src/lib.rs && cargo build` after editing locale files to force the rust-i18n proc macro to re-read the YAML (CLAUDE.md i18n rule).

7. **AC7 — Integration tests** (NEW file `tests/contributor_delete_modal.rs`, sibling of `tests/borrower_delete_modal.rs` from 9-10 and `tests/return_loan_modal.rs` from 9-11). 8 `#[sqlx::test]` cases, mirror of `borrower_delete_modal.rs`'s 8 cases:
   - `get_contributor_delete_modal_returns_200_with_dialog_for_librarian_request` — librarian session, GET `/contributor/:id/delete-modal`, returns 200 + body contains `<dialog open aria-modal="true">` + the contributor name + the `delete` variant's red Confirm button (`bg-red-600`) + `data-modal-default-focus` on Cancel + `hx-delete="/catalog/contributors/{id}"` (verify the **plural + /catalog/ prefix**) + `hx-target="#contributor-feedback"` + `hx-swap="innerHTML"` + the hidden `_csrf_token` input.
   - `get_contributor_delete_modal_returns_200_for_admin_request` — admin can also delete (admin > librarian); same shape.
   - `get_contributor_delete_modal_redirects_anonymous_to_login` — anonymous session, returns 303 → `/login?next=%2Fcontributor%2F{id}` (mirror of 9-10's `get_delete_modal_redirects_anonymous_to_login` which uses `require_role_with_return`).
   - `get_contributor_delete_modal_returns_404_for_soft_deleted_contributor` — contributor with `deleted_at = NOW()`, returns 404 (the `find_by_id` filter handles this naturally).
   - `get_contributor_delete_modal_returns_404_for_nonexistent_contributor` — id `99999` with no row, returns 404.
   - `get_contributor_delete_modal_returns_405_for_non_htmx_request` — direct browser nav (no `HX-Request` header), returns 405. **Body is empty AND no `Allow:` response header is set**. Assertions: `assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);` + `assert!(resp.headers().get(axum::http::header::ALLOW).is_none(), "405 must not set Allow header — see story 9-11 code-review patch");`. Mirror of the 9-11 `get_return_modal_returns_405_for_non_htmx_request` test in `tests/return_loan_modal.rs`.
   - `get_contributor_delete_modal_html_escapes_contributor_name` — contributor named `<script>alert(1)</script>`, returned HTML contains `&#60;script&#62;` (Askama numeric entity form, per 9-10's behavior verified test) AND does NOT contain the raw `<script>alert(1)</script>` substring. Both the named (`&lt;`) and numeric (`&#60;`) forms are valid; assert on raw-substring absence + entity-substring presence (mirror of 9-10's assertion shape).
   - `delete_contributor_via_existing_handler_still_works` — sanity check: as a librarian, fire `DELETE /catalog/contributors/{id}` directly (the unchanged existing handler) for a contributor with NO title associations, assert response 200 + `HX-Redirect: /catalog` + the row is soft-deleted (`deleted_at IS NOT NULL`). Mirrors 9-10's `delete_borrower_via_existing_handler_still_works`.
   - **OPTIONAL but RECOMMENDED — 9-th case for the FR54 conflict path**: `delete_contributor_with_active_titles_returns_inline_conflict_feedback` — seed a contributor + a title-contributor junction, fire `DELETE /catalog/contributors/{id}`, assert response 200 + body contains the conflict message (`"Cannot delete"` / `"Impossible de supprimer"`) + the row is NOT soft-deleted. Locks the FR54 contract that this story explicitly preserves (epics AC1: "the existing FR54 protection ... remains server-side and still returns 409 Conflict on attempt"). This is the only AC7 test that exercises the modal-Confirm → conflict-feedback round-trip path; without it, the conflict branch is only covered by E2E (AC9), which is more brittle.

8. **AC8 — Templates audit stays green**: `cargo test no_inline_markup_in_templates`, `cargo test hx_confirm_matches_allowlist`, `cargo test forms_include_csrf_token`, `cargo test csrf_exempt_routes_frozen` all pass after the migration. The new modal trigger uses `data-modal-trigger` (not `hx-confirm`), no new CSP-violating markup is introduced.

9. **AC9 — E2E test** — extend the existing `tests/e2e/specs/journeys/catalog-contributor.spec.ts:134-260` test (`"delete contributor with associations shows block message, unassign then delete succeeds"`) to drive the new modal flow:
   - **Drop** the `page.on("dialog", (d) => d.accept());` line at `:217` — obsolete (no native confirm dialog any more).
   - **Replace** both `await deleteBtn.click();` paths (lines `:224` and `:256`) with the modal click sequence:
     ```ts
     await deleteBtn.click();
     await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
     await page.locator("[data-modal-confirm]").click();
     ```
   - The first click (with associations) opens the modal, click Confirm → conflict feedback lands in `#contributor-feedback` → modal closes (htmx 2xx). Existing assertion `await expect(feedback).toContainText(/Cannot delete|Impossible de supprimer/i, ...)` continues to pass.
   - The second click (after un-association) opens the modal, click Confirm → `HX-Redirect: /catalog` → `await page.waitForURL("**/catalog", ...)` continues to pass.
   - **Smoke flow assertion (NEW)**: between the two delete-attempts, add a quick modal-open / Escape-close assertion to lock the focus-trap inheritance from 9-10:
     ```ts
     await deleteBtn.click();
     await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
     await expect(page.locator("[data-modal-default-focus]")).toBeFocused();
     await page.keyboard.press("Escape");
     await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible();
     ```
     Place this BEFORE the un-association step so it doesn't perturb the conflict-feedback assertion. (If injecting fits the existing flow, do it; if it bloats the spec past clarity, inline a separate `test("delete-contributor modal — Escape closes and restores focus")` instead. Prefer in-line for brevity.)
   - **NO new scanner-guard E2E test** in 9-12. The 9-10 + 9-11 specs already exercise scanner-guard inheritance on the new modal selector shape; 9-12 inherits the same protection by-construction (same `<dialog open aria-modal="true">` macro shape). Adding a third scanner-guard burst test would be redundant per the 9-11 code-review's "AC9 missing E2E assertions" decision (accepted gap — Modal foundation tests are load-bearing once, not per-migration).
   - **Helper updates** in `tests/e2e/helpers/`:
     - **No new helper file** for contributor delete (the existing test is single-call and inline; do not create `tests/e2e/helpers/contributor.ts` for a one-shot flow). If a second contributor-delete spec emerges in 9-13/9-14 work, extract then.
     - The existing inline `page.evaluate(...)` blocks for `htmx.ajax(...)` to populate the contributor list stay as-is.
   - **CI flake gate** (`grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/`) MUST stay clean — use DOM-state assertions, not arbitrary sleeps. The existing test does this correctly; the migration must not regress it.
   - **EN/FR matcher invariance**: existing `page.getByRole("button", { name: /delete|supprimer/i })` and `expect(feedback).toContainText(/Cannot delete|Impossible de supprimer/i)` patterns continue to work for both locales. The `data-modal-confirm` selector is locale-agnostic.

10. **AC10 — Foundation Rule #12 LOC discipline**:
    - `templates/pages/contributor_detail.html` net change: 1 line replaced (still 41 LOC, well under 2000).
    - `templates/fragments/contributor_delete_modal.html` is a NEW file (~17 LOC).
    - `src/routes/contributors.rs` grows by ~70 LOC (new `delete_modal` handler + `ContributorDeleteModalTemplate`). Current LOC: 157. Projected: ~227. Far under 2000 — no extraction needed.
    - `src/routes/catalog.rs` is UNCHANGED (the existing `delete_contributor` handler stays put). No LOC growth in catalog.rs (which is over the 2000-LOC ceiling pre-9-12 per story 9-9 retro — keeping new code OUT of catalog.rs is the discipline).
    - `src/routes/mod.rs` net change: +1 line for the new route registration. Current LOC: TBD — measure in Task 1, won't push it close to 2000.
    - `src/templates_audit.rs` net change: −1 line (entry removed). Loses LOC, doesn't gain.
    - `tests/contributor_delete_modal.rs` is a NEW file (~280–330 LOC of integration tests, mirror of `tests/borrower_delete_modal.rs`'s 327 LOC). Lives in `tests/` (not `src/tests/`), so the 2000-LOC ceiling doesn't bite.
    - `tests/e2e/specs/journeys/catalog-contributor.spec.ts` net change: +5 to +10 LOC (modal click sequence replaces single-click; optional inline Escape-close assertion). Current LOC: 356. Projected: 361–366.
    - `locales/en.yml` and `locales/fr.yml` net change: +3 keys / −1 key per locale = +2 lines per locale.

11. **AC11 — CSP / scanner-guard / CSRF inheritance**:
    - The new modal uses `<dialog open aria-modal="true">` (inherited from the macro). Scanner-guard 7-5 applies automatically — no new E2E assertion needed (see AC9 rationale).
    - **CSRF**: the modal's Confirm button issues `hx-delete="/catalog/contributors/{id}"`. The macro's `csrf_token` 8th param renders a hidden `<input name="_csrf_token">` inside the modal's confirm form (verified by AC7's `…_with_dialog_for_librarian_request` test asserting the hidden input is present). Without it, the CSRF middleware on `DELETE /catalog/contributors/{id}` would 403.
    - **NOTE**: `templates_audit::forms_include_csrf_token` matches `<form method="POST">` only — the modal macro's `<form hx-delete=…>` (no `method=` attribute) is NOT scanned by that audit. The CSRF input is policed instead at TWO layers: (a) AC7's integration tests assert the hidden input is present in the rendered HTML; (b) the CSRF middleware rejects any state-changing request lacking a valid token, which the E2E tests would catch as a 403 + failed Confirm flow. Don't lean on the audit as the safety net for this macro — lean on AC7.

12. **AC12 — Server contract is UNCHANGED**: `DELETE /catalog/contributors/{id}` returns the same `HX-Redirect: /catalog` for HTMX success / `Redirect::to("/catalog")` for non-HTMX success / inline `Html(feedback_html("error", …))` 200 for Conflict + NotFound. The existing `tests/e2e/specs/journeys/catalog-contributor.spec.ts:259` `await page.waitForURL("**/catalog", ...)` MUST keep passing. The only change to the existing handler is a doc-comment update to mention the modal route as a discoverability link (e.g., `/// Trigger UX: see GET /contributor/:id/delete-modal (story 9-12).`).

13. **AC13 — Story-level grep audit**: at story close, run two greps and document the output in Dev Agent Record:
    - `grep -rnE 'hx-confirm=' templates/` — must return exactly 2 hits, matching `ALLOWED_HX_CONFIRM_SITES.len()` after the trim (series_detail.html + admin_users_row.html).
    - `grep -rnE 'hx-confirm=' src/` — must return ZERO hits. Unlike 9-11 (where `loan_row_html` had a Rust-emitted `hx-confirm=`), 9-12 has no Rust-emitted callers — confirm by grep.
    - `grep -rn 'confirm_delete' src/ templates/ locales/` — must return ZERO hits (the AC4 + AC6 cleanup must be complete).

14. **AC14 — Local Testing Before Push (Foundation Rule #13)**: run the full local gate before opening the PR. Minimum:
    - `SQLX_OFFLINE=true cargo check` — clean
    - `cargo clippy --all-targets -- -D warnings` — clean
    - `cargo test --lib` — green (≥918 lib + integration tests + new AC7 cases)
    - `cargo test --test contributor_delete_modal` — green (the 8 or 9 integration tests from AC7)
    - `cargo test hx_confirm_matches_allowlist` — green
    - `cargo test no_inline_markup_in_templates` — green
    - `cargo test forms_include_csrf_token` — green
    - Full E2E via `./scripts/e2e-reset.sh` + `cd tests/e2e && npm test` — green; pay attention to `catalog-contributor.spec.ts` going green with the migrated flow.
    - The flake gate: `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` returns nothing.
    - i18n EN/FR mirror parity test green.

15. **AC15 — Draft PR + CI gate (Foundation Rule #15 + #18)**: open a draft PR at the first commit (per `gh pr create --draft`) and WAIT for CI to finish before requesting review or merging. CI green → squash-merge. CI red → diagnose via `gh run view --log-failed`, fix, push, wait again. The hx-confirm migration chain is precisely the workflow #15 + #18 were designed for — many small mechanical PRs with tight CI feedback loops.

## Tasks / Subtasks

- [x] **Task 1 — Verify reality-check assumptions and inventory current state (AC: all)**
  - [x] Read `src/routes/catalog.rs:1663-1698` and confirm: role gate is `Role::Librarian` (NOT Admin), endpoint is `/catalog/contributors/{id}` (plural + `/catalog/`), success returns `HX-Redirect: /catalog` for HTMX / `Redirect::to("/catalog")` for non-HTMX, Conflict returns 200 + `Html(feedback_html(...))`. Document the exact endpoint string in Dev Agent Record so the modal handler's `action_url = format!("/catalog/contributors/{}", id)` is unambiguous.
  - [x] Read `src/routes/mod.rs:75-76` and confirm the existing route registration. The new GET route will be registered AFTER `"/contributor/{id}"` (line 149) for visual grouping with the GET detail handler.
  - [x] Read `templates/components/modal.html` and confirm it accepts the 10 params per the 9-10/9-11 contract. Confirm the `delete` variant emits `bg-red-600` on the Confirm button.
  - [x] Read `templates/fragments/borrower_delete_modal.html` and `tests/borrower_delete_modal.rs` for the mirror pattern. The contributor versions should be near-byte-identical with type/name swaps.
  - [x] Grep `confirm_delete` callers across `src/`, `templates/`, `locales/` to confirm dropping the i18n key + the field + the template attribute removes ALL references. Document the call-site count in Dev Agent Record (expected: 4 — `contributor_detail.html:14`, `routes/contributors.rs:35` field, `routes/contributors.rs:78-79` construction, plus the i18n key in both en.yml + fr.yml).
  - [x] Measure current `src/routes/contributors.rs` LOC (`wc -l`). Project +70 LOC. If projected total ≥1900, plan an extraction (e.g., new sibling `src/routes/contributors_modal.rs`) BEFORE Task 3. Current is 157 → projected ~227, comfortably under.
  - [x] Verify `ContributorModel::find_by_id` already filters `WHERE deleted_at IS NULL` (it does — `src/models/contributor.rs:24-43`). No new model method needed.

- [x] **Task 2 — i18n keys (AC: 6)**
  - [x] Add 3 new keys to `locales/en.yml` under the existing `contributor:` block (after the existing `contributor.deleted: Contributor deleted.` line at `:104` is a clean place — keep alphabetical-ish order or follow existing pattern):
    ```yaml
    contributor:
      # … existing keys …
      delete_modal_title: "Delete contributor %{name}?"
      delete_modal_body: "Linked titles will lose this contributor unless re-assigned."
      delete_modal_confirm: "Delete"
    ```
  - [x] Add the same 3 keys to `locales/fr.yml` with FR copy:
    ```yaml
    contributor:
      # … existing keys …
      delete_modal_title: "Supprimer le contributeur %{name} ?"
      delete_modal_body: "Les titres liés perdront ce contributeur sauf s'il est réassigné."
      delete_modal_confirm: "Supprimer"
    ```
  - [x] Drop `contributor_detail.confirm_delete` from BOTH locale files (zero callers after AC3 + AC4; verified via Task 1 grep).
  - [x] Run `touch src/lib.rs && cargo build` to force rust-i18n proc-macro recompilation.
  - [x] Run `cargo test all_t_keys_have_both_locales` (the parity test in `src/i18n/audit.rs:186`) to confirm every `t!()` key has an entry in both locales.

- [x] **Task 3 — `GET /contributor/:id/delete-modal` handler + route (AC: 1, 2, 11)**
  - [x] Add to `src/routes/contributors.rs`:
    - `ContributorDeleteModalTemplate` struct (mirror of `BorrowerDeleteModalTemplate` from 9-10): fields `title`, `body_html`, `confirm_label`, `cancel_label`, `action_url`, `csrf_token`. The fragment template references these field names directly.
    - `pub async fn delete_modal(...)` mirroring `borrowers::delete_modal` from 9-10 (~70 LOC). Inputs: `State<AppState>`, `Session`, `Extension<Locale>`, `HxRequest(is_htmx)`, `Path<u64>`. Behaviors per AC1:
      - `session.require_role_with_return(Role::Librarian, &format!("/contributor/{id}"))?`
      - Early-return `Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response())` if `!is_htmx` — single-line shape, no `Allow:` header tuple, no body. Mirror of `src/routes/loans.rs:309`.
      - `let contributor = ContributorModel::find_by_id(pool, id).await?.ok_or_else(|| AppError::NotFound(...))?;`
      - Pre-translate the 4 i18n keys via `t!(..., locale = loc)`. For the title key, pass the raw name: `t!("contributor.delete_modal_title", locale = loc, name = contributor.name.as_str())`.
      - Build `body_html = format!("<p>{body_text}</p>")`.
      - Set `action_url = format!("/catalog/contributors/{}", contributor.id)` (PLURAL + `/catalog/`).
      - Render the template; on `Err(e)` return `AppError::Internal(format!("contributor delete modal render: {e}"))` (mirror of 9-10's pattern with the original error captured for production debuggability — patched in 9-11 code review).
  - [x] Register the route in `src/routes/mod.rs` (immediately after the existing `"/contributor/{id}"` GET registration at line 149-150):
    ```rust
    .route(
        "/contributor/{id}/delete-modal",
        axum::routing::get(contributors::delete_modal),
    )
    ```

- [x] **Task 4 — Modal fragment template (AC: 2, 11)**
  - [x] Create `templates/fragments/contributor_delete_modal.html` (mirror of `templates/fragments/borrower_delete_modal.html`, ~17 LOC):
    ```jinja
    {# Story 9-12 — contributor delete confirmation modal.
       Calls the shared `components/modal.html::modal` macro with the
       `delete` variant. Body includes pre-translated FR54 hint copy.
       Hardcoded hx_target = "#contributor-feedback" because this is the
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
        "#contributor-feedback",
        "innerHTML",
    ) %}{% endcall %}
    ```
  - [x] Run `cargo test no_inline_markup_in_templates` to confirm the new fragment is CSP-clean (it is by construction — just an Askama macro call — but the audit should pass on the touched file set).

- [x] **Task 5 — Migrate the trigger button site (AC: 3, 4, 8)**
  - [x] `templates/pages/contributor_detail.html:14`: replace per AC3's before/after. Tailwind classes UNCHANGED.
  - [x] Drop the `confirm_delete` field from `ContributorDetailTemplate` in `src/routes/contributors.rs:35`.
  - [x] Drop the construction-site `confirm_delete: rust_i18n::t!("contributor_detail.confirm_delete", ...).to_string(),` from `src/routes/contributors.rs:78-79`.
  - [x] Run `cargo test no_inline_markup_in_templates` to confirm no inline `style=` / `onclick=` slipped in.
  - [x] Run `cargo build` to confirm the field-removal compiles without errors.

- [x] **Task 6 — `ALLOWED_HX_CONFIRM_SITES` cleanup (AC: 5, 13)**
  - [x] Remove the `("templates/pages/contributor_detail.html", 1),` entry from the const array in `src/templates_audit.rs:36`.
  - [x] Run `cargo test hx_confirm_matches_allowlist` and confirm green.
  - [x] Run `cargo test --lib templates_audit` (all 4 audit tests) and confirm green.
  - [x] Run the AC13 grep audit:
    - `grep -rnE 'hx-confirm=' templates/` — must return exactly 2 hits.
    - `grep -rnE 'hx-confirm=' src/` — must return ZERO hits.
    - `grep -rn 'confirm_delete' src/ templates/ locales/` — must return ZERO hits.
  - [x] Document the grep output in Dev Agent Record.

- [x] **Task 7 — Integration tests (AC: 7, 11)**
  - [x] Create `tests/contributor_delete_modal.rs` with the 8 (+1 optional FR54) `#[sqlx::test]` cases from AC7. Use the same fixture pattern as `tests/borrower_delete_modal.rs`:
    - `build_state(pool)` helper.
    - `seed_session(pool, username)` to create a session for `admin` / `librarian` (both seeded by `migrations/20260329000002_seed_dev_user.sql` + `migrations/20260414000001_seed_librarian_user.sql`).
    - `insert_contributor(pool, name)` helper that runs `INSERT INTO contributors (name) VALUES (?)` and returns the inserted id.
    - `soft_delete_contributor(pool, id)` for the 404 test.
    - `req_htmx(method, uri, session_cookie)` and `req_plain(method, uri, session_cookie)` request builders.
    - `body_text(resp)` to extract response body.
  - [x] For the FR54 conflict test (AC7 9th case), insert a title row + a `title_contributors` junction row before firing the DELETE. Use raw `INSERT` SQL — there's no need to go through the service layer for fixture setup.
  - [x] Run `SQLX_OFFLINE=true cargo test --test contributor_delete_modal` and confirm all pass green. Document the test count in Dev Agent Record.
  - [x] **CSRF assertion** (AC11): one of the librarian/admin happy-path tests MUST also assert `assert!(html.contains("name=\"_csrf_token\""))` to lock the macro's CSRF embedding.

- [x] **Task 8 — E2E updates (AC: 9, 12)**
  - [x] Edit `tests/e2e/specs/journeys/catalog-contributor.spec.ts:134-260` per AC9:
    - Drop the `page.on("dialog", (d) => d.accept());` line.
    - Replace both `await deleteBtn.click();` paths with the modal click sequence (click trigger → wait `#modal-slot dialog[open]` → click `[data-modal-confirm]`).
    - Add the inline Escape-close assertion between the two delete-attempts (or as a separate sibling test, dev's choice).
  - [x] Run `cd tests/e2e && npx tsc --noEmit` to verify the spec edits don't break tsc.
  - [x] Run `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/catalog-contributor.spec.ts` (single-spec run for fast feedback) and confirm green.
  - [x] Run the full E2E lane (`cd tests/e2e && npm test`) and confirm no other spec regressions (spec ID `"CC"` in `catalog-contributor.spec.ts` is unchanged; no ISBN collisions risk).

- [x] **Task 9 — Local gate + push (AC: 14, 15)**
  - [x] `SQLX_OFFLINE=true cargo check` — clean
  - [x] `cargo clippy --all-targets -- -D warnings` — clean
  - [x] `cargo test` (full lib + integration) — green
  - [x] CI flake gate: `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` returns nothing
  - [x] Push branch + open draft PR (Foundation Rule #15)
  - [x] WAIT for CI green per Foundation Rule #18 before requesting review / merging

## Dev Notes

### Pattern reuse (9-10 / 9-11 → 9-12)

This is the SECOND mechanical migration on top of the 9-10 foundation (9-11 was the first). The handler shape, fragment shape, and integration-test shape should all mirror `borrowers::delete_modal` 1-for-1 with type/name swaps. Differences from 9-10:
- **Role gate is `Role::Librarian`** (NOT Admin). Mirror of the trigger's `{% if role == "librarian" || role == "admin" %}` template gate.
- **DELETE endpoint has the `/catalog/contributors/{id}` plural-+-`/catalog/` prefix**, NOT `/contributor/{id}` (which is the GET detail path). Off-by-one trap — verify in Task 1.
- **Modal Confirm targets `#contributor-feedback` with `hx-swap="innerHTML"`** (not `hx-target=""` + `hx-swap="none"` like 9-10). The contributor handler returns inline feedback for FR54 conflict, so the macro's 9-th and 10-th params are wired to a real target.

If you find yourself diverging more than ~10 LOC in any of those mirrors, stop and revisit — the 9-10 + 9-11 specs were deliberately crafted to make 9-12/9-13/9-14 feel like copy-and-tune.

### Why `delete` not `warning`

Per UX-DR8: contributor deletion soft-deletes the row (`deleted_at = NOW()`) — it goes to Trash, recoverable for 30 days via the admin Trash panel (story 8-7). The user perception is "destruction of data" (the row disappears from the catalog UI immediately), even though the auto-purge is reversible until the 30-day window closes. UX-DR8 maps this to the `delete` variant (red confirm button, conveys severity). The `delete-forever` variant is reserved for hard-delete from Trash; `warning` is reserved for reversible state changes (like the 9-11 return-loan flow); `remove` is reserved for non-destructive removals (like un-assigning a contributor from a title without deleting the contributor row — that's covered by `POST /catalog/contributors/remove`, NOT this handler).

### Why hx-target+hx-swap matter here (vs. 9-10's `"", "none"`)

The 9-10 borrower delete handler uses `HX-Redirect` on success — full nav, no inline swap needed. So 9-10's modal Confirm form passes `("", "none")` to the macro and the form omits `hx-target` / `hx-swap`. Confirm click → htmx fires DELETE → 200 + HX-Redirect → full nav. Modal disappears with the page change.

The 9-12 contributor delete handler uses `HX-Redirect: /catalog` on SUCCESS — same as 9-10. BUT on FR54 Conflict it returns 200 + inline `feedback_html` (because returning 4xx would suppress the swap by default). The trigger button on `contributor_detail.html` originally used `hx-target="#contributor-feedback" hx-swap="innerHTML"` to land that conflict feedback. The modal Confirm form must reproduce this contract — otherwise the conflict feedback HTML would land in `#modal-slot` (overwriting the modal!) or be discarded.

So 9-12 hardcodes `hx_target="#contributor-feedback"` + `hx_swap="innerHTML"` in the modal fragment via the macro params added in 9-11. On Confirm click:
- **Success path**: server returns 200 + `HX-Redirect: /catalog` → htmx performs full nav → the inline-swap setup is irrelevant.
- **FR54 Conflict path**: server returns 200 + inline `feedback_html` → htmx swaps it into `#contributor-feedback` → modal closes via modal.js's `htmx:afterRequest` listener (filtered to `[data-modal-confirm]` on 2xx per 9-10 PR #129) → user sees the conflict feedback under the action bar.

### No `?target=` query parameter (vs. 9-11's closed allowlist)

9-11 had THREE surfaces (loans.html, borrower_detail.html, plus the Rust-emitted scan-card on /loans), each with a different feedback container. So 9-11 introduced a `?target=…` query parameter validated against a closed allowlist (`["loan-feedback", "borrower-feedback", "scan-result"]`) to reuse one route across all three.

9-12 has ONE surface — the `/contributor/:id` page. The closed-allowlist pattern would be over-engineering (YAGNI). Hardcode `#contributor-feedback` in the fragment template. If a future story adds a second surface (e.g., a contributor-list bulk-delete on `/catalog`), reintroduce the closed-allowlist pattern then. Document this deviation from 9-11 explicitly in Dev Agent Record so reviewers don't ask.

### Why drop the `Allow: GET` header on 405

The 9-11 code review patched the equivalent header on the return-modal handler (was `Allow: GET`, became no header). Reasoning: returning `Allow: GET` on a 405 self-contradicts — we DO support GET (just not without the `HX-Request` header). The 405 here means "wrong request shape" (browser nav vs HTMX request), not "wrong method". Standards-compliance-wise, `Allow:` should advertise the methods the resource accepts; "GET-but-only-via-HTMX" is not a method.

The 9-10 borrower delete-modal handler still has `Allow: GET` on its 405 — that's pre-9-11-patch state, and bringing it into line is OUT OF SCOPE for 9-12 (would be a refactor-during-feature anti-pattern). 9-12 starts clean (no Allow header) per the 9-11 fix; the borrower handler can be aligned in a chore PR (file as `type:code-review-finding` if the 9-12 reviewer flags it).

### FR54 conflict feedback path is the load-bearing test in this story

The "delete with active titles" path is the entire reason FR54 exists. The 9-12 migration must keep this path working IDENTICALLY to today. Two tests cover it:
- `delete_contributor_with_active_titles_returns_inline_conflict_feedback` (AC7 9th case, integration) — locks the server contract: 200 + conflict message in body + row not soft-deleted.
- `catalog-contributor.spec.ts` "delete contributor with associations shows block message…" (AC9, E2E) — locks the user-facing flow: open modal → Confirm → conflict feedback in `#contributor-feedback` → modal closes → un-assign → re-open modal → Confirm → redirect to /catalog.

If either test goes red, the migration broke FR54. Don't ship until both are green.

### Drop `contributor_detail.confirm_delete` per Foundation Rule #1

Mirror of 9-10's `borrower.confirm_delete` drop and 9-11's `loan.return_confirm` drop. Three migration stories, three dead-key drops. The pattern is now established convention for this migration chain.

The retained sibling key `contributor_detail.delete` (`"Delete"` / `"Supprimer"`) — used as the trigger button label — STAYS. Don't conflate the two in the grep cleanup: only `confirm_delete` is dead, `delete` is the live label.

### File-LOC budget

`src/routes/contributors.rs` is 157 LOC pre-9-12 → ~227 post. Plenty of headroom. No extraction needed.

`src/routes/catalog.rs` is over 2000 LOC pre-9-12 (a known condition documented as a deferred GH issue per the 9-9 code-review). The DELETE handler `delete_contributor` lives there but is UNCHANGED. Adding the modal handler to `catalog.rs` would worsen the violation; placing it in `contributors.rs` (where the GET detail handler already lives) keeps catalog.rs static and preserves the per-resource modular shape.

### DEFERRED items from 9-10/9-11 that 9-12 should be aware of (no action in 9-12)

- **Two-modal race** (modal.js slot-clearing on second trigger before B's dialog renders) — KF #134 (or similar — verify via `gh issue list --label type:known-failure`). Not exercised by 9-12: the contributor detail page has only one modal trigger, and the user can only have one modal open at a time. Don't add a regression test for the race here; it's out of scope.
- **Hardcoded `/catalog` post-success redirect** — KF #133-equivalent (the 9-11 review filed a similar issue for the `/loans` redirect). Document only — out of scope.
- **Frozen modal on Confirm error** (e.g., 500 from server) — KF #134-equivalent. The modal stays open if `htmx:afterRequest` fires with `xhr.status >= 400` (modal.js only closes on 2xx). The user is stuck without a clear escape-route. Documented quirk; the user can press Escape to close. Out of scope here.
- **Migrate the 3 admin modal fragments** (`admin_ref_delete_modal.html`, etc.) — STILL deferred per the 9-10 close. Out of scope here. Revisit after 9-14 closes the migration chain.
- **JS focus-trap unit tests** — STILL deferred (no JS test harness in project). Mirror of 9-10 / 9-11 deferral.
- **Bidirectional EN/FR locale parity test** — deferred Epic 9 follow-up per 9-11 review.

### Project Structure Notes

- `src/routes/contributors.rs` already hosts the GET detail handler; the new modal handler sits alongside `contributor_detail`. No new module.
- `src/routes/catalog.rs` is UNCHANGED (the existing DELETE handler stays put).
- `templates/fragments/contributor_delete_modal.html` mirrors `templates/fragments/borrower_delete_modal.html` (9-10 sibling). Same shape, three-line diff (different action_url + non-empty `hx_target`/`hx_swap` literals).
- `tests/contributor_delete_modal.rs` mirrors `tests/borrower_delete_modal.rs` (9-10 sibling).
- `static/js/modal.js`, `templates/components/modal.html`, `layouts/base.html` are ALL UNCHANGED.
- `tests/e2e/specs/journeys/catalog-contributor.spec.ts` is the single E2E spec touched.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.12] — story spec verbatim (8 ACs + EN/FR copy)
- [Source: _bmad-output/implementation-artifacts/9-10-modal-foundation-and-borrower-migration.md] — pattern precedent (Modal macro, modal.js, focus trap, scanner-guard inheritance, integration-test shape, dead-i18n-key drop, `require_role_with_return` for anonymous-redirect-to-detail)
- [Source: _bmad-output/implementation-artifacts/9-11-migrate-return-loan-to-modal.md] — pattern precedent (10-param macro signature, dropping the `Allow:` header on 405, dead-`confirm_label` field drop, raw-string delimiter bumps if any, AC9 inline E2E migration shape, FR54-equivalent server-contract preservation via AC13)
- [Source: CLAUDE.md#Foundation-Rules] — Rules #1 (DRY), #11 (issue tracking), #12 (LOC ceiling), #13 (local testing), #15 (draft PR), #18 (CI gating)
- [Source: CLAUDE.md#Modal-scanner-guard-invariant-story-7-5] — the `dialog[open]` + `[aria-modal="true"]` selector contract that the new modal inherits
- [Source: CLAUDE.md#Key-Patterns#CSRF-synchronizer-token-story-8-2] — why the modal macro takes `csrf_token` as its 8th param and how `_csrf_token` hidden inputs are policed by `templates_audit::forms_include_csrf_token`
- [Source: src/routes/catalog.rs:1663-1698] — existing `delete_contributor` (verbatim signature reproduced in AC1 / Task 1; UNCHANGED in this story)
- [Source: src/routes/contributors.rs:40-88] — existing `contributor_detail` GET handler (sibling location for the new `delete_modal` handler)
- [Source: src/routes/mod.rs:75-76] — existing `DELETE /catalog/contributors/{id}` route registration (UNCHANGED)
- [Source: src/routes/mod.rs:149-150] — existing `GET /contributor/{id}` route registration (new GET delete-modal sibling registered here)
- [Source: src/services/contributor.rs:129-143] — `ContributorService::delete_contributor` — the FR54 protection (count_title_associations + AppError::Conflict). UNCHANGED in this story.
- [Source: src/models/contributor.rs:24-43] — `ContributorModel::find_by_id` — already filters `WHERE deleted_at IS NULL`. No new model method needed.
- [Source: src/templates_audit.rs:35-39] — `ALLOWED_HX_CONFIRM_SITES` const (current state captured in AC5 / Task 6)
- [Source: templates/pages/contributor_detail.html:11-21] — line range of the current `hx-confirm` button + the surrounding role-gated action bar
- [Source: templates/components/modal.html] — the 10-param shared macro (post-9-11 shape)
- [Source: templates/fragments/borrower_delete_modal.html] — the 17-line fragment template that 9-12's contributor_delete_modal.html mirrors
- [Source: tests/borrower_delete_modal.rs] — the 327-LOC integration-test mirror
- [Source: tests/e2e/specs/journeys/catalog-contributor.spec.ts:134-260] — the existing E2E test that AC9 migrates

## Dev Agent Record

### Agent Model Used

claude-opus-4-7[1m]

### Debug Log References

- `cargo test --test contributor_delete_modal` — 9/9 green (8 AC7 cases + 1 FR54 conflict)
- `cargo test --lib templates_audit` — 4/4 green
- `cargo test` (full lib + integration) — 755 lib + 9 contributor_delete_modal + ~170 other integration = all green
- `cargo clippy --all-targets -- -D warnings` — clean
- `cd tests/e2e && npm test` (clean stack) — 200 passed, 2 skipped, 1 unrelated flake (`home-search.spec.ts:222`, passes in isolation; documented parallel-flake from prior stories, not introduced by 9-12)
- `cd tests/e2e && npx playwright test specs/journeys/catalog-contributor.spec.ts` — 7/7 green (incl. the migrated 1.3s modal flow)
- `cd tests/e2e && npx tsc --noEmit` — clean
- CI flake gate `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` — empty

### Completion Notes List

- **Mirror of 9-10/9-11.** Handler shape, fragment, integration tests are byte-near-identical to `borrowers::delete_modal` + `borrower_delete_modal.html` + `tests/borrower_delete_modal.rs` with:
  - Role gate `Role::Librarian` (not Admin) — verified by `_for_admin_request` test (admin > librarian both pass).
  - `_with_return("/contributor/{id}")` so anonymous direct-URL hitter lands back on the contributor page after login (locked by `redirects_anonymous_to_login` test asserting `/login?next=%2Fcontributor%2F{id}`).
  - Endpoint asymmetry retained: trigger button hits singular `/contributor/{id}/delete-modal` (sibling of GET detail), but the Confirm form `hx-delete`s the plural `/catalog/contributors/{id}` per the existing route registration at `mod.rs:75-76` (UNCHANGED). Locked by `..._with_dialog_for_librarian_request` asserting `hx-delete="/catalog/contributors/{id}"`.
  - 405 returns NO `Allow:` header per the 9-11 code-review patch — clean shape `Ok(StatusCode::METHOD_NOT_ALLOWED.into_response())`. Locked by `returns_405_for_non_htmx_request` asserting `headers().get(header::ALLOW).is_none()`. Out-of-scope for 9-12: aligning the borrower handler's 9-10 `Allow: GET` to this clean form (refactor-during-feature anti-pattern).

- **No `?target=` query parameter.** Single surface (the `/contributor/:id` page), so the modal fragment hardcodes `hx_target="#contributor-feedback"` + `hx_swap="innerHTML"` via the macro's 9-th and 10-th params. YAGNI on the 9-11 closed-allowlist pattern. Locked by the librarian-happy-path test asserting both attributes appear in the rendered HTML.

- **FR54 conflict path is the load-bearing contract.** `delete_contributor_with_active_titles_returns_inline_conflict_feedback` (AC7's 9th case) exercises the exact path that hangs the migration's correctness — server returns 200 + inline `feedback_html` with the conflict copy + the contributor row stays alive (no `deleted_at` written). The wire status is 200 (NOT 4xx — HTMX would suppress the swap by default), and there is NO `HX-Redirect` header on conflict. Both invariants asserted explicitly so a future regression flips visible.

- **Variant `delete` (not `warning`).** Soft-delete is "destruction of data" perception even if reversible from Trash within 30 days (mirror of 9-10). Modal Confirm button gets the red `bg-red-600` palette — locked by the librarian test asserting `bg-red-600` in the rendered HTML.

- **Drop `contributor_detail.confirm_delete` per Rule #1.** Dead key + dead struct field + dead construction site, all removed in the same PR (mirror of 9-10's `borrower.confirm_delete` + 9-11's `loan.return_confirm`). Three migration stories, three dead-key drops.

- **Spec line-number drift (minor, non-blocking).** Spec said the trigger button was at `contributor_detail.html:14` and the role gate at `:11`; actual file has the role gate at `:13` and the button at `:15`. Edit was unambiguous (used surrounding context). The `confirm_delete` field was at `routes/contributors.rs:35`, construction at `:78-79` exactly as the spec said.

- **`ALLOWED_HX_CONFIRM_SITES` 3 → 2.** `templates_audit::hx_confirm_matches_allowlist` green on the trimmed array. AC13 grep audit (`grep -rnE 'hx-confirm=' templates/`) returns 2 attribute hits matching `ALLOWED_HX_CONFIRM_SITES.len() = 2` — `series_detail.html` (next migration 9-13) + `admin_users_row.html` (last migration 9-14). The chain is 2 PRs from `&[]`.

- **`grep -rnE 'hx-confirm=' src/` not zero — 1 pre-existing.** `src/routes/locations.rs:256` Rust-emits an `hx-confirm` for the delete-location tree-row button (pre-existing for many epics; not in the migration chain 9-10 → 9-14). 9-12 introduces no new Rust-emitted `hx-confirm` (verified: zero hits inside `src/routes/contributors.rs`). The `templates_audit` regex walks `templates/` only — the location.rs string is undetected by the audit (latent gap, pre-existing). Out-of-scope for 9-12 to migrate; document as an inherited condition.

- **`grep -rn 'confirm_delete' src/ templates/ locales/`** returns only `series_detail.html` + `routes/series.rs` + `series.confirm_delete` i18n key — all reserved for 9-13. Zero contributor-related matches.

- **CSRF token coverage.** AC11 `_with_dialog_for_librarian_request` asserts `name="_csrf_token"` is in the rendered HTML. The macro's 8th param wires the hidden input automatically; without it, the CSRF middleware on `DELETE /catalog/contributors/{id}` would 403.

- **E2E migration.** Replaced `page.on("dialog", (d) => d.accept())` + 2× `deleteBtn.click()` with the modal click sequence (click trigger → wait `#modal-slot dialog[open]` → click `[data-modal-confirm]`). Inserted a between-attempt Escape-close assertion that verifies `[data-modal-default-focus]` is the focused element AND Escape closes the modal — locks 9-10 focus-trap inheritance into the contributor flow, mirror of 9-11's `scanner-guard suppresses scanner burst while modal open` regression-cover idea but lighter (no scanner-burst in 9-12 per AC9 rationale: foundation tests are load-bearing once at 9-10/9-11, not per-migration).

- **No new helper file.** `tests/e2e/helpers/contributor.ts` not created — single-call inline flow per AC9's "if a second contributor-delete spec emerges in 9-13/9-14, extract then" guidance.

- **No `.sqlx/` cache update needed.** All new SQL goes through runtime `sqlx::query`/`query_as`/`query_scalar` (the test fixture inserts) — no compile-time `query!` macros added.

- **LOC budget respected.** `src/routes/contributors.rs` 157 → 235 (well under 2000). `tests/contributor_delete_modal.rs` 365 LOC NEW (lives under `tests/`, not bound by Rule #12). `templates/pages/contributor_detail.html` 41 → 41 (1 line replaced). `src/templates_audit.rs` −1 line. `templates/fragments/contributor_delete_modal.html` 17 LOC NEW.

### File List

NEW:
- `templates/fragments/contributor_delete_modal.html` (17 LOC)
- `tests/contributor_delete_modal.rs` (365 LOC, 9 `#[sqlx::test]` cases)

MODIFIED:
- `src/routes/contributors.rs` (+78 LOC: `ContributorDeleteModalTemplate` + `delete_modal` handler; -2 LOC: dropped `confirm_delete` field + construction site; +1 use stmt: `Role`)
- `src/routes/mod.rs` (+4 LOC: registered `GET /contributor/{id}/delete-modal`)
- `src/templates_audit.rs` (-1 LOC: removed `contributor_detail.html` entry from `ALLOWED_HX_CONFIRM_SITES`)
- `templates/pages/contributor_detail.html` (-1/+3 LOC: replaced `hx-delete`+`hx-confirm` button with `hx-get`+`data-modal-trigger`+a11y attrs; net +2 LOC)
- `locales/en.yml` (+3 keys under `contributor:` — `delete_modal_title`/`delete_modal_body`/`delete_modal_confirm`; -1 key dropped `contributor_detail.confirm_delete`)
- `locales/fr.yml` (mirror EN — +3 keys, -1 key)
- `tests/e2e/specs/journeys/catalog-contributor.spec.ts` (~+12 LOC net: replaced 2× `deleteBtn.click()` with modal click sequence + inline Escape-close + default-focus assertions; dropped `page.on("dialog", ...)`)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (Story 9-12 ready-for-dev → in-progress → review; `last_updated` narrative)
- `_bmad-output/implementation-artifacts/9-12-migrate-delete-contributor-to-modal.md` (Status header → review; Tasks/Subtasks all `[x]`; Dev Agent Record populated)

### Change Log

| Date | Change |
| --- | --- |
| 2026-05-06 | Story created (backlog → ready-for-dev) |
| 2026-05-06 | Dev-story complete (in-progress → review). 3rd PR in the hx-confirm migration chain (9.10 → 9.14, 2 PRs left). Migrated delete-contributor to UX-DR8 Modal (variant `delete`, soft-delete to Trash). Server contract `DELETE /catalog/contributors/{id}` UNCHANGED — handler in `catalog.rs:1663` not touched, only the trigger UX is. ALLOWED_HX_CONFIRM_SITES: 3 → 2 entries. Dropped dead `contributor.confirm_delete` i18n key + struct field + construction site (Rule #1). 9 integration tests, 1 E2E spec migration with focus-trap regression assertion. 755 lib + 9 contributor_delete_modal + integration tests all green; clippy clean -D warnings; templates audit + i18n parity + tsc + CI flake gate all green; full E2E suite 200/200 passed (1 unrelated flake on `home-search.spec.ts:222`). |
| 2026-05-06 | Code review complete (review → done). 3 parallel reviewers (Blind/Edge/Auditor) — 31 findings consolidated → 9 actionable + 18 dismissed. 0 BLOCKERS (Acceptance Auditor: 14/15 ACs MET + 1 PARTIAL on AC12, all addressed). 6 patches applied: (1) AC12 doc-comment on `delete_contributor` discoverability link; (2) 405 test asserts empty body per AC1 spec; (3) NEW integration test `contributor_detail_page_renders_feedback_target_div` locks `<div id="contributor-feedback">` existence on the rendered page (load-bearing for the modal's hardcoded `hx_target="#contributor-feedback"`); (4) E2E asserts modal closes after FR54 conflict 200 (regression cover for modal.js `htmx:afterRequest` close-on-2xx contract); (5) E2E asserts trigger button has no `hx-confirm=` attribute (paranoid lock against accidental re-introduction beyond the templates_audit's count-mismatch detection); (6) replaced brittle Tailwind `bg-red-600` substring with stable `data-modal-variant="delete"` selector. 3 items deferred to GH Issues per Foundation Rule #11 (404 silent UI feedback cross-cutting on all 3 modal migrations; i18n body_html defense-in-depth escape for community translations; pre-existing `locations.rs:256` Rust-emitted hx-confirm). 755 lib + 10 contributor_delete_modal (was 9, +1 new feedback-target integration test) + integration tests all green; clippy clean -D warnings; tsc clean; full E2E suite 200/200 passed on clean stack with the same unrelated `home-search.spec.ts:222` flake (passes in isolation). |

### Review Findings

Adversarial code review run on 2026-05-06 (3 parallel layers: Blind Hunter / Edge Case Hunter / Acceptance Auditor). 0 decision-needed, 6 patches proposed, 3 deferred, ~18 dismissed as signal-to-noise / mirror-of-borrower / pre-existing / theoretical.

**Auditor BLOCKER count: 0.** All 15 ACs MET (1 PARTIAL on AC12's optional doc-comment, addressed by patch P1).

#### Patches

- [x] [Review][Patch] AC12 doc-comment on `delete_contributor` [src/routes/catalog.rs:1663] — add `/// Trigger UX: see GET /contributor/:id/delete-modal (story 9-12).` discoverability link per AC12 wording.
- [x] [Review][Patch] 405 test asserts empty body [tests/contributor_delete_modal.rs::get_contributor_delete_modal_returns_405_for_non_htmx_request] — AC1 specifies "empty body"; lock with `assert!(body_text(resp).await.is_empty())`.
- [x] [Review][Patch] Verify `#contributor-feedback` element exists on rendered detail page [tests/contributor_delete_modal.rs] — Medium. The modal fragment hardcodes `hx_target="#contributor-feedback"`. If a future template change removed the target div, the FR54 conflict feedback would silently no-op. Add an integration test that GETs `/contributor/:id` as a librarian and asserts the response body contains `id="contributor-feedback"`.
- [x] [Review][Patch] E2E asserts modal closes after FR54 conflict 200 [tests/e2e/specs/journeys/catalog-contributor.spec.ts:232-241] — strengthens regression cover for modal.js's `htmx:afterRequest` close-on-2xx contract. Add `await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible()` after the conflict-feedback assertion.
- [x] [Review][Patch] E2E asserts trigger button no longer carries `hx-confirm=` [tests/e2e/specs/journeys/catalog-contributor.spec.ts] — paranoid lock against accidental re-introduction; the templates_audit catches *count mismatches* but doesn't reject re-adding both file and entry. Add `await expect(deleteBtn).not.toHaveAttribute("hx-confirm")`.
- [x] [Review][Patch] Replace brittle `bg-red-600` substring with `data-modal-variant="delete"` [tests/contributor_delete_modal.rs::get_contributor_delete_modal_returns_200_with_dialog_for_librarian_request] — more stable than Tailwind class substrings; the macro emits `data-modal-variant="{{ variant }}"` on the `<dialog>` already.

#### Deferred (to be filed as GH Issues per Foundation Rule #11)

- [x] [Review][Defer] 404 silent UI feedback (TOCTOU concurrent deletion) [Edge Hunter] — cross-cutting on all 3 modal migrations (9-10 borrower, 9-11 return-loan, 9-12 contributor). Librarian A loads contributor page; Librarian B deletes the same contributor in another tab; A clicks Delete → handler returns 404 → HTMX swap no-ops → `#modal-slot` stays empty → no UI feedback. Cross-cutting code-review-finding; not a 9-12 regression.
- [x] [Review][Defer] i18n `body_html` defense-in-depth escape [src/routes/contributors.rs:90] — `let body_html = format!("<p>{body_text}</p>");` ships the i18n value through the macro's RAW `|safe` channel. Static literals today; latent risk if community translations or YAML-import workflows are added later. Mirror of 9-10's borrower handler — file as a pattern-wide code-review-finding to consider html-escaping the i18n body across all 3 modals at once.
- [x] [Review][Defer] Pre-existing `src/routes/locations.rs:256` Rust-emitted `hx-confirm=` [src/routes/locations.rs:256] — inherited from prior epics; `templates_audit::hx_confirm_matches_allowlist` walks `templates/` only and misses Rust string-emitted markup. Out of scope for 9-12 (delete-contributor migration); should be filed as a code-review-finding for a future migration sweep.
