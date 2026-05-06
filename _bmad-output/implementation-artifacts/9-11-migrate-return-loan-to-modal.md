# Story 9.11: Migrate hx-confirm — return loan (loans.html + borrower_detail.html)

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As the project maintainer,
I want the two "return loan" confirmation flows (on `/loans` and on borrower detail) migrated from `hx-confirm=` to the UX-DR8 Modal component built in 9.10,
so that the return-loan UX is consistent with the destructive-action pattern, two grandfathered sites are removed in lockstep, and `borrower_detail.html` exits the `ALLOWED_HX_CONFIRM_SITES` allowlist completely.

## ⚠️ Existing-code reality check

Before writing a single line, walk the code that 9-11 touches and verify the assumptions below — they are LOCKED IN by the 9-10 close and the current main:

- **Modal macro is already shipped.** `templates/components/modal.html` (39 LOC at 9-10 close, 60 LOC ceiling) takes 8 parameters: `variant` / `title` / `body_html` (RAW — caller must escape) / `confirm_label` / `cancel_label` / `action_url` / `action_method` / `csrf_token`. The `warning` variant is already implemented; this story does NOT modify the macro. Verify the macro file is unchanged at story close.

- **`static/js/modal.js` is shipped.** The focus trap, Escape close, mousedown-tracking backdrop close, `[data-modal-trigger][data-pressed="true"]` focus-restoration, and `htmx:afterRequest` filter to `[data-modal-confirm]` (so nested fetches don't close the modal) are all in place from 9-10. This story does NOT modify modal.js. Confirm via grep at story close.

- **`<div id="modal-slot">` is in `layouts/base.html`** (sibling of `#admin-modal-slot`). Already loaded on every page that extends the layout, including `/loans` and `/borrower/:id`. No layout edit needed in this story.

- **Two `hx-confirm=` sites left in scope:**
  - `templates/pages/loans.html:123` — return-loan button inside the loans table row.
  - `templates/pages/borrower_detail.html:79` — return-loan button inside the active-loans table on the borrower detail page.
  
  Both buttons are IDENTICAL except for `hx-target`: `#loan-feedback` on `/loans`, `#borrower-feedback` on `/borrower/:id`. They both `hx-post` to the same server endpoint `POST /loans/{id}/return`.

- **Current `ALLOWED_HX_CONFIRM_SITES`** in `src/templates_audit.rs` has 5 entries × 1 occurrence = 5 grandfathered. After 9-11: BOTH `loans.html` AND `borrower_detail.html` entries are removed entirely (NOT decremented to 0 — entries with `count == 0` would never match the audit's positive assertion shape; the entry must be deleted from the array). Result: 3 entries × 1 occurrence = 3 grandfathered. Total grandfathered drops from 5 → 3 in this single PR.

- **Server handler `POST /loans/{id}/return`** in `src/routes/loans.rs:225` is **unchanged** by this story. Returns `HtmxResponse { main: feedback_html, oob: vec![] }` for HTMX requests, `Redirect::to("/loans")` for non-HTMX. Role gate is `session.require_role(Role::Librarian)?` (NOT admin — librarian can return loans, mirrors the role gate on the existing buttons). The handler's `IntoResponse` shape is the contract that the modal's Confirm button must rely on: a successful response inserts inline feedback HTML into the modal-confirm's `hx-target`.

- **`loan.return_confirm`** in `locales/en.yml:436` and `locales/fr.yml:436` ("Return this volume?" / "Retourner ce volume ?") is the OLD plain-confirm copy. This story DROPS it (zero callers after migration, dead key per Foundation Rule #1) and adds 3 new keys (modal title / body / confirm). Same dead-key drop pattern that 9-10 used for `borrower.confirm_delete`.

- **The two surfaces share IDENTICAL modal copy** per the epics spec — the variant (`warning`), the title, the body, and both i18n labels are the same across `loans.html` and `borrower_detail.html`. This story therefore introduces ONE shared modal route + ONE shared fragment template + ONE set of i18n keys reused on both surfaces.

## Acceptance Criteria

1. **AC1 — NEW shared route `GET /loans/:id/return-modal`** in `src/routes/loans.rs` (sibling of the existing `return_loan_handler`):
   - Returns the rendered modal fragment via the `templates/components/modal.html` macro from 9-10, variant `warning` (return is REVERSIBLE — the volume can be re-loaned — so this is `warning`, not `delete`).
   - Pre-translates 4 i18n keys: title (`loan.return_modal_title` — "Mark loan as returned?"), body (`loan.return_modal_body` — "The volume will be available again."), confirm (`loan.return_modal_confirm` — "Mark as returned"), cancel (`common.cancel` — already shipped by 9-10).
   - **Role gate**: `session.require_role(Role::Librarian)?` (mirrors the existing `POST /loans/:id/return` handler — verified via Task 1 grep).
   - Returns 404 if the loan is not found OR if `returned_at IS NOT NULL` (already returned — the row exists but the action would be a no-op; better to refuse the modal than render a confirmation that will fail with `loan.already_returned`).
   - Direct browser navigation (no `HX-Request` header) returns 405 Method Not Allowed (mirrors 9-10's modal route shape — the modal fragment is meaningless without page context).
   - **Feedback target query parameter** (`?target=...`): the route accepts an optional `target` query parameter, validates it against the closed allowlist `["loan-feedback", "borrower-feedback"]`, and embeds it as the Confirm button's `hx-target` (rendered as `hx-target="#loan-feedback"` or `hx-target="#borrower-feedback"`). If `target` is missing OR not in the allowlist, default to `loan-feedback`. The allowlist is a `const` in the handler module so a future surface (e.g., a dashboard widget) is one entry away. **Validation IS load-bearing**: without the allowlist, an attacker could pass `?target=evil-injected-element` and steer feedback HTML into a chosen DOM node. The handler MUST reject anything outside the closed set.

2. **AC2 — Migrate `templates/pages/loans.html:123`** return-loan button:
   - Before: `<button hx-post="/loans/{{ loan.id }}/return" hx-confirm="{{ confirm_label }}" hx-target="#loan-feedback" hx-swap="innerHTML" hx-disabled-elt="this" …>{{ return_label }}</button>`
   - After: `<button hx-get="/loans/{{ loan.id }}/return-modal?target=loan-feedback" hx-target="#modal-slot" hx-swap="innerHTML" data-modal-trigger aria-haspopup="dialog" aria-expanded="false" …>{{ return_label }}</button>`
   - Tailwind classes unchanged (visual identity preserved). `hx-disabled-elt="this"` dropped (the modal flow doesn't need double-click protection on the trigger — modal.js's `data-pressed` mechanism + the modal opening already prevents reuse).
   - The `confirm_label` template field can stay in the handler's view-model UNTIL all migrations are done (it's still used by the audit allowlist's other entries during 9.12–9.14); the field becomes dead at 9.14 close, dropped then.

3. **AC3 — Migrate `templates/pages/borrower_detail.html:79`** return-loan button:
   - Identical migration shape to AC2 but with `?target=borrower-feedback` query parameter.
   - Before: `<button hx-post="/loans/{{ loan.id }}/return" hx-confirm="{{ confirm_label }}" hx-target="#borrower-feedback" hx-swap="innerHTML" hx-disabled-elt="this" …>{{ return_label }}</button>`
   - After: `<button hx-get="/loans/{{ loan.id }}/return-modal?target=borrower-feedback" hx-target="#modal-slot" hx-swap="innerHTML" data-modal-trigger aria-haspopup="dialog" aria-expanded="false" …>{{ return_label }}</button>`
   - **DRY check at story close**: both button lines (across the two files) MUST be byte-identical except for the `target=...` query parameter value. If the snippets diverge in any other respect (Tailwind classes, ARIA attributes, hx-swap value), refactor before commit — copy-paste drift is the precise pattern this story exists to prevent.

4. **AC4 — Update `src/templates_audit.rs::ALLOWED_HX_CONFIRM_SITES`**:
   - Before:
     ```rust
     ("templates/pages/loans.html", 1),
     ("templates/pages/borrower_detail.html", 1),
     ("templates/pages/contributor_detail.html", 1),
     ("templates/pages/series_detail.html", 1),
     ("templates/fragments/admin_users_row.html", 1),
     ```
   - After:
     ```rust
     ("templates/pages/contributor_detail.html", 1),
     ("templates/pages/series_detail.html", 1),
     ("templates/fragments/admin_users_row.html", 1),
     ```
   - Total entries: 5 → 3. Total occurrences: 5 → 3. The allowlist will reach `&[]` at 9.14 close per the Epic 9 migration chain.
   - `cargo test hx_confirm_matches_allowlist` MUST pass with the trimmed array.

5. **AC5 — i18n: 3 NEW keys + 1 DROPPED key per locale** (EN + FR):
   - **NEW** under `loan:` block:
     - `return_modal_title: "Mark loan as returned?" / "Marquer le prêt comme retourné ?"`
     - `return_modal_body: "The volume will be available again." / "Le volume redevient disponible."`
     - `return_modal_confirm: "Mark as returned" / "Marquer comme retourné"`
   - **DROPPED**: `loan.return_confirm: "Return this volume?" / "Retourner ce volume ?"` — zero callers after AC2/AC3 migration (Foundation Rule #1 dead-key drop, mirrors 9-10's `borrower.confirm_delete` removal).
   - **REUSED** (no edits): `common.cancel` (shipped by 9-10), `loan.return` (the table button label and the modal trigger label — kept).
   - Run the EN/FR mirror parity test (`cargo test locale_keys_match`) to confirm no key is added in one locale without the other.
   - Run `touch src/lib.rs && cargo build` after editing locale files to force the rust-i18n proc macro to re-read the YAML (CLAUDE.md i18n rule).

6. **AC6 — Templates audit stays green**: `cargo test no_inline_markup_in_templates`, `cargo test hx_confirm_matches_allowlist`, `cargo test forms_include_csrf_token`, `cargo test csrf_exempt_routes_frozen` all pass after the migration. The new modal trigger uses `data-modal-trigger` (allowed — story 9-10 audit pattern), no new CSP-violating markup is introduced.

7. **AC7 — Integration tests** (NEW file `tests/return_loan_modal.rs`, sibling of `tests/borrower_delete_modal.rs` that 9-10 created):
   - `get_return_modal_returns_200_with_dialog_for_librarian_request` — librarian session, GET `/loans/:id/return-modal?target=loan-feedback`, returns 200 + body contains `<dialog open aria-modal="true">` + the warning-variant indigo confirm button.
   - `get_return_modal_returns_200_for_admin_request` — admin can also return loans (admin > librarian); same shape as above.
   - `get_return_modal_returns_303_for_anonymous_request` — anonymous session, returns 303 → `/login?next=%2Floans` (or whatever the `require_role` shape is — VERIFY in Task 1).
   - `get_return_modal_returns_404_for_nonexistent_loan` — id 99999 with no row, returns 404.
   - `get_return_modal_returns_404_for_already_returned_loan` — loan with `returned_at = NOW()`, returns 404 (refuse to render a no-op confirmation; covered by AC1 spec).
   - `get_return_modal_returns_405_for_non_htmx_request` — direct browser nav (no `HX-Request` header), returns 405.
   - `get_return_modal_target_loan_feedback_renders_correct_hx_target` — query param `?target=loan-feedback` → response body contains `hx-target="#loan-feedback"`.
   - `get_return_modal_target_borrower_feedback_renders_correct_hx_target` — query param `?target=borrower-feedback` → response body contains `hx-target="#borrower-feedback"`.
   - `get_return_modal_target_invalid_falls_back_to_loan_feedback` — query param `?target=evil-injected` → response body contains `hx-target="#loan-feedback"` (the safe default), and **NOT** `hx-target="#evil-injected"` (the security-load-bearing assertion that locks the validation).
   - `get_return_modal_target_missing_falls_back_to_loan_feedback` — no `?target=` → default applies.
   - `post_return_loan_via_existing_handler_still_works` — sanity check that the unchanged `POST /loans/:id/return` responds 200 + transitions the loan to returned + returns inline feedback (proves the migration didn't break the existing contract). Mirrors 9-10's `delete_borrower_via_existing_handler_still_works` test.

8. **AC8 — Macro-render co-location** (LOC discipline): a single render test in `src/routes/modal_tests.rs` verifies the `warning` variant rendering with the return-loan copy is byte-identical regardless of which `target` the handler passes. This proves AC3's DRY claim at the unit-test layer — if a future contributor adds a divergent class to one of the two button sites, the unit test won't catch that, but the templates_audit + the E2E suite below will. Don't duplicate effort across layers.

9. **AC9 — E2E test** (extend `tests/e2e/specs/journeys/loan-returns.spec.ts` AND `tests/e2e/specs/journeys/borrower-loans.spec.ts` — both surfaces need a smoke):
   - **`/loans` smoke** (extending `loan-returns.spec.ts`): librarian login → setup a loan → on `/loans` click "Return" → assert modal opens (`#modal-slot dialog[open]` visible) → assert Cancel is focused (`[data-modal-default-focus]`) → assert Cancel button comes BEFORE Confirm in tab order → press Escape → assert modal closes → click "Return" again → click Confirm → assert loan disappears from `#loans-table-body` (existing assertion, kept).
   - **Borrower-detail smoke** (extending `borrower-loans.spec.ts`): librarian login → setup a loan to a borrower → navigate to `/borrower/:id` → click "Return" inside the active-loans table → assert modal opens → click Confirm → assert loan disappears from `#active-loans-section`.
   - **Scanner-guard inheritance check** (1 of the 2 specs only — pick `loan-returns.spec.ts` for visibility): while the modal is open and Cancel is focused, send `simulateScan(page, "body", "9782070360246")` → assert dialog is still visible AND Cancel is still focused (Cancel-focused proves Enter terminator was suppressed; if scanner-guard were broken, the Enter would activate Cancel and close the modal). Mirrors the load-bearing-assertion pattern fixed in 9-10 PR #129.
   - **Helper updates** in `tests/e2e/helpers/loans.ts`:
     - `returnLoanFromLoansPage(page, volumeLabel)` and `returnLoanFromBorrowerDetail(page, volumeLabel)` (the latter currently lives in `borrower-loans.spec.ts` — relocate to the shared helper module if Task 6 deems it shared enough). Both helpers MUST drop the `page.once("dialog", ...)` browser-confirm interception (now obsolete) and replace it with a Modal click sequence: click Return → wait for `#modal-slot dialog[open]` → click `[data-modal-confirm]`.
     - The helpers must use the existing CSS-based selectors for the Return button (e.g., `button:has-text("Return"), button:has-text("Retourner")`) to support EN+FR — DO NOT introduce English-only assertions.
   - CI flake gate (`grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/`) MUST stay clean — use DOM-state assertions, not arbitrary sleeps.

10. **AC10 — Foundation Rule #12 LOC discipline**:
    - `loans.html` net change: 1 line replaced (still well under 2000).
    - `borrower_detail.html` net change: 1 line replaced (still well under 2000).
    - `src/routes/loans.rs` grows by ~50 LOC (new `return_modal_handler` + the closed allowlist const + a small `Deserialize` query struct). Verify it stays under 2000 at story close (current LOC: TBD — measure in Task 1, abort if already over).
    - `src/templates_audit.rs` net change: −2 lines (entries removed). Loses LOC, doesn't gain.
    - `tests/return_loan_modal.rs` is a NEW file (~250–300 LOC of integration tests). Lives in `tests/` (not `src/tests/`), so the 2000 LOC ceiling on individual source files doesn't bite.

11. **AC11 — CSP / scanner-guard / CSRF inheritance**:
    - The new modal uses `<dialog open aria-modal="true">` (inherited from the macro). Scanner-guard 7-5 applies automatically — verified by AC9's load-bearing E2E assertion.
    - CSRF: the modal's Confirm button POSTs to `/loans/:id/return` via `hx-post` + `hx-target` + `hx-swap`. The macro's 8th param (`csrf_token`) renders a hidden `<input name="_csrf_token">` inside the modal's confirm form (the macro already does this — verify in Task 1 by reading the macro source). Without the hidden input, the CSRF middleware on `POST /loans/:id/return` would 403 (`HX-Trigger: csrf-rejected` header), since HTMX `hx-post` from a fresh fragment swap doesn't automatically include the meta-tag CSRF header unless `csrf.js` has re-attached. **Verify in AC7 integration tests**: after rendering the modal fragment, the response body MUST contain `<input type="hidden" name="_csrf_token"`.

12. **AC12 — Coexistence with admin modals**: clicking a return-loan button while an `#admin-modal-slot` modal is somehow already open (shouldn't be possible from the same screen, but document the behavior anyway) MUST not visually stack with z-index issues. The scanner-guard's stack-based `topModal()` already handles nested modals correctly (from 7-5). Mark this as "by-construction OK; no new code" in Dev Agent Record.

13. **AC13 — Server contract is UNCHANGED**: `POST /loans/:id/return` returns the same `HtmxResponse { main: feedback_html, oob: vec![] }` for HTMX, `Redirect::to("/loans")` for non-HTMX. The same E2E assertions about the row disappearing from `#loans-table-body` MUST keep passing. The only change to the existing handler is a doc-comment update to mention the modal route as a discoverability link.

14. **AC14 — Story-level grep audit**: at story close, run `grep -rE 'hx-confirm=' templates/` and assert the count matches `ALLOWED_HX_CONFIRM_SITES.len()` exactly (3 after this story). A grep mismatch means a new `hx-confirm` slipped in or an entry wasn't fully migrated. Document the grep output in Dev Agent Record.

15. **AC15 — Local Testing Before Push (Foundation Rule #13)**: run the full local gate before opening the PR. Minimum:
    - `SQLX_OFFLINE=true cargo check` — clean
    - `cargo clippy --all-targets -- -D warnings` — clean
    - `cargo test --lib` — green (≥ ~903 tests + the AC8 render test)
    - `cargo test --test return_loan_modal` — green (the 11 integration tests from AC7)
    - Full E2E via `./scripts/e2e-reset.sh` + `cd tests/e2e && npm test` — green; surface the loan-returns and borrower-loans specs for verification.
    - The flake gate: `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` returns nothing.
    - i18n: EN and FR locale files have the same key set (parity test green).

## Tasks / Subtasks

- [x] **Task 1 — Verify reality-check assumptions and inventory current state (AC: all)**
  - [x] Read `src/routes/loans.rs::return_loan_handler` (line 225 area) and confirm: role gate is `Role::Librarian`, returns `HtmxResponse` for HTMX / `Redirect::to("/loans")` for non-HTMX, takes `Path<u64>` for the loan id, no `Path<u64>` validation issues with already-returned loans (the model layer raises `loan.already_returned`).
  - [x] Read `src/routes/mod.rs` and verify the existing route declaration: `.route("/loans/{id}/return", axum::routing::post(loans::return_loan_handler))`. The new route will follow the same plural-`loans` convention.
  - [x] Read `templates/components/modal.html` (39 LOC) — confirm it accepts the 8 params per the 9-10 contract and that the `warning` variant uses `bg-indigo-600`. Confirm `csrf_token` is the 8th param and is rendered as `<input type="hidden" name="_csrf_token" value="…">` inside the form.
  - [x] Read `static/js/modal.js` for the `[data-modal-trigger]` / `[data-modal-default-focus]` / `[data-modal-confirm]` contracts.
  - [x] Grep `loan.return_confirm` callers across `src/`, `templates/`, `tests/` to confirm dropping the key only removes the two `hx-confirm` references. Document the call-sites count in Dev Agent Record.
  - [x] Measure current `src/routes/loans.rs` LOC (`wc -l src/routes/loans.rs`) and project +50 LOC. If projected total ≥ 1900, plan an extraction (e.g., new sibling `src/routes/loans_modal.rs`) BEFORE Task 3.

- [x] **Task 2 — i18n keys (AC: 5)**
  - [x] Add 3 new keys to `locales/en.yml` under the existing `loan:` block: `return_modal_title`, `return_modal_body`, `return_modal_confirm`. Add the same 3 to `locales/fr.yml`.
  - [x] Drop `loan.return_confirm` from BOTH locale files (zero callers after the migration; verified via Task 1 grep).
  - [x] Run `touch src/lib.rs && cargo build` to force rust-i18n proc-macro recompilation.
  - [x] Run `cargo test locale_keys_match` (or whatever the EN/FR parity test is named — verify) to confirm no key drift.

- [x] **Task 3 — `GET /loans/:id/return-modal` handler + route (AC: 1, 2, 3, 11)**
  - [x] Add a `ReturnModalQuery` struct in `src/routes/loans.rs`: `#[derive(Deserialize)] struct ReturnModalQuery { target: Option<String> }`.
  - [x] Add the closed allowlist as a module-level `const FEEDBACK_TARGETS: &[&str] = &["loan-feedback", "borrower-feedback"];`.
  - [x] Implement `pub async fn return_modal_handler(...)` mirroring `borrowers::delete_modal_handler` from 9-10 (the precedent set in `tests/borrower_delete_modal.rs` is the integration-test contract). Inputs: `State<AppState>`, `Session`, `Extension<Locale>`, `HxRequest(is_htmx)`, `Path<u64>`, `Query<ReturnModalQuery>`. Behaviors:
    - Require `Role::Librarian`.
    - Return 405 if `!is_htmx`.
    - Look up the loan via the existing `LoanModel::find_by_id_active` (verify the method name in `src/models/loan.rs`); 404 if not found OR if `returned_at IS NOT NULL`.
    - Validate `query.target.as_deref()` against `FEEDBACK_TARGETS`; default to `"loan-feedback"` on missing/invalid.
    - Pre-translate the 4 i18n keys via `t!(…, locale = locale.0)`.
    - Render `templates/fragments/return_loan_modal.html` (see Task 4 — a thin fragment that calls the shared modal macro with the per-loan-resolved fields).
  - [x] Register the route in `src/routes/mod.rs` immediately above the existing `POST /loans/{id}/return` line: `.route("/loans/{id}/return-modal", axum::routing::get(loans::return_modal_handler))`.

- [x] **Task 4 — Modal fragment template (AC: 1, 8, 11)**
  - [x] Create `templates/fragments/return_loan_modal.html` (mirror of `templates/fragments/borrower_delete_modal.html` from 9-10, ~16 LOC). Uses the shared `{% import "components/modal.html" as modal %}` and emits `{% call modal::modal(variant="warning", title=title, body_html=body_html, confirm_label=confirm_label, cancel_label=cancel_label, action_url=action_url, action_method="POST", csrf_token=csrf_token) %}`.
  - [x] The `action_url` field must be `format!("/loans/{}/return", loan_id)`. The Confirm button inherits this via the macro.
  - [x] **CRITICAL nuance — feedback target injection**: the macro currently emits `hx-swap="none"` on the Confirm button (per 9-10). For 9-11 we want `hx-swap="innerHTML"` and `hx-target="#{validated_target}"` so the feedback renders inline. **Decision in Task 4**: either (a) add 2 new optional macro params (`hx_target`, `hx_swap`) defaulting to `none` so 9-10 callers don't change, OR (b) wrap the macro call in our fragment and override the relevant attributes via `hx-target` + `hx-swap` on the Confirm-form `<form>` (HTMX picks up the closest enclosing form's hx-target if not on the button itself). Pick (a) if the macro signature change stays under the 60-LOC ceiling — it's the cleaner forward-compatibility path for 9.12–9.14. Document the choice in Dev Agent Record.

- [x] **Task 5 — Migrate the two button sites (AC: 2, 3)**
  - [x] `templates/pages/loans.html:123`: replace per AC2's before/after.
  - [x] `templates/pages/borrower_detail.html:79`: replace per AC3's before/after.
  - [x] Run `cargo test no_inline_markup_in_templates` to confirm no inline `style=` / `onclick=` slipped in.
  - [x] Verify the byte-identity rule from AC3 with a `diff` of the two lines (only the `target=...` value should differ).

- [x] **Task 6 — `ALLOWED_HX_CONFIRM_SITES` cleanup (AC: 4, 14)**
  - [x] Remove the two entries (`loans.html`, `borrower_detail.html`) from the const array in `src/templates_audit.rs`.
  - [x] Run `cargo test hx_confirm_matches_allowlist` and `cargo test --lib templates_audit` to confirm.
  - [x] Run `grep -rnE 'hx-confirm=' templates/` and document the count in Dev Agent Record (must equal `ALLOWED_HX_CONFIRM_SITES.len()` = 3).

- [x] **Task 7 — Integration tests (AC: 7, 11)**
  - [x] Create `tests/return_loan_modal.rs` with the 11 `#[sqlx::test]` cases from AC7.
  - [x] Use the same fixture pattern as `tests/borrower_delete_modal.rs`: setup an admin user + a librarian user + an anonymous baseline; create a loan via `LoanModel::create_loan` for the librarian/admin happy paths; mark a loan as returned (`returned_at = NOW()`) for the 404-on-already-returned test.
  - [x] Assert response bodies contain `<dialog open aria-modal="true">`, the indigo `bg-indigo-600` button color (warning variant), the validated `hx-target="#…"`, and the CSRF hidden input.
  - [x] Run `SQLX_OFFLINE=true cargo test --test return_loan_modal` and confirm all 11 pass green.

- [x] **Task 8 — E2E updates (AC: 9)**
  - [x] Update `tests/e2e/helpers/loans.ts::returnLoanFromLoansPage`: drop the `page.once("dialog", ...)` browser-confirm interception; replace with the modal click sequence (click Return → wait for `#modal-slot dialog[open]` → click `[data-modal-confirm]` → wait for row removal from `#loans-table-body`).
  - [x] Move `returnLoanFromBorrowerDetail` from the inline `borrower-loans.spec.ts` helper into `tests/e2e/helpers/loans.ts` as a sibling (Foundation Rule #1 DRY — the two helpers share 90% of their body modulo target selector).
  - [x] Add the scanner-guard inheritance assertion in `loan-returns.spec.ts` (mirror the 9-10 pattern: `simulateScan(page, "body", "9782070360246")` while modal open + Cancel focused → assert `dialog` still visible AND Cancel still focused).
  - [x] Run `cd tests/e2e && npx tsc --noEmit` to verify the helper-signature changes don't break tsc.
  - [x] Run `./scripts/e2e-reset.sh && cd tests/e2e && npm test` (full E2E lane) and confirm both `loan-returns.spec.ts` and `borrower-loans.spec.ts` go green; no other spec regressions.

- [x] **Task 9 — Local gate + push (AC: 15)**
  - [x] `SQLX_OFFLINE=true cargo check` — clean
  - [x] `cargo clippy --all-targets -- -D warnings` — clean
  - [x] `cargo test` (full lib + integration) — green
  - [x] CI flake gate clean
  - [x] Push branch + open draft PR (Foundation Rule #15)

## Dev Notes

### Pattern reuse (9-10 → 9-11)

This is the FIRST mechanical migration on top of the 9-10 foundation. The handler shape, fragment shape, and integration-test shape should all mirror `tests/borrower_delete_modal.rs` and `borrowers::delete_modal_handler` 1-for-1, with the obvious type swaps. If you find yourself diverging more than ~10 LOC in any of those mirrors, stop and revisit — the 9.10 spec was deliberately crafted to make 9.11–9.14 feel like copy-and-tune.

### Why `warning` not `delete`

A loan return is REVERSIBLE — the volume can be re-loaned to the same borrower (or another) at any time. The user perception is "completion of an action," not "destruction of data." UX-DR8 maps this to the `warning` variant (indigo confirm button, neutral title icon if any). The `delete` and `delete-forever` variants are reserved for actual data destruction (delete borrower, hard-delete from trash).

### Why a closed allowlist for `feedback_target`

The two surfaces send different `hx-target` values (`#loan-feedback` vs `#borrower-feedback`) because they have different feedback containers in their layouts. A naïve implementation would let the trigger button pass any string into the modal's `hx-target`. That's a HTML-injection foothold: a malicious link could open the modal with `?target=evil` and steer the server's feedback HTML into a pre-planted DOM node. The closed allowlist (`["loan-feedback", "borrower-feedback"]`) eliminates the foothold by construction. AC7's `get_return_modal_target_invalid_falls_back_to_loan_feedback` test locks the validation as a regression guard.

### Server contract is unchanged

This is a UX migration, not a backend rework. `POST /loans/:id/return` returns inline feedback HTML for HTMX requests today, and it continues to do so after this story. The modal's Confirm button is configured to render that feedback into the validated target. If the feedback shape ever changes (e.g., to switch to OOB swaps for `#loans-table-body` row removal in a future story), the modal's Confirm button can be updated then — out of scope here.

### Drop `loan.return_confirm` per Foundation Rule #1

The dead-key drop pattern was established by 9-10 (which dropped `borrower.confirm_delete` for the same reason). Don't keep dead i18n keys around "in case someone needs them" — their existence is a liability, and the audit doesn't catch unused keys. Grep first, drop second.

### File-LOC budget

If `src/routes/loans.rs` is already close to the 2000-line limit (verify in Task 1), extract `return_modal_handler` + `ReturnModalQuery` + `FEEDBACK_TARGETS` into a new sibling `src/routes/loans_modal.rs` BEFORE adding the handler. The 9-10 precedent for "extract sibling on growth" is `src/routes/admin_reference_data.rs` (story 8-4). This is cheaper than a refactor mid-story.

### DEFERRED items from 9-10 that 9-11 should be aware of

- **Two-modal race** (modal.js slot-clearing on second trigger before B's dialog renders): not exercised by 9-11 because the user has only ONE return-loan modal open at a time on either surface. If a future story (e.g., 9.20 keyboard shortcuts) exposes a way to open multiple modals concurrently, the JS will need to be revisited.
- **Modal closes on nested HTMX**: the modal.js patch from 9-10 PR #129 filters `htmx:afterRequest` to `[data-modal-confirm]` only. Our return-loan modal has NO autocomplete or inline validation, so this isn't exercised. If a future variant adds form fields with HTMX, re-verify.
- **AC9 of 9-10: JS focus-trap unit tests** — STILL deferred (no JS test harness in project). The E2E load-bearing assertion in AC9 of THIS story is the regression guard, mirroring 9-10.
- **Migrate the 3 admin modal fragments** — STILL deferred per the 9-10 close. Out of scope here too. Revisit after 9.14 closes the migration chain (the new macro will have proven its mechanics across 4 surfaces by then).

### Project Structure Notes

- `src/routes/loans.rs` already hosts the loan handlers; the new modal handler sits alongside `return_loan_handler`. No new module unless LOC pushes us close to 2000 (see budget note above).
- `templates/fragments/return_loan_modal.html` mirrors `templates/fragments/borrower_delete_modal.html` (9-10 sibling).
- `tests/return_loan_modal.rs` mirrors `tests/borrower_delete_modal.rs` (9-10 sibling).
- `static/js/modal.js` is unchanged. `templates/components/modal.html` is unchanged UNLESS the Task 4 decision (a) adds `hx_target` / `hx_swap` macro params for forward compatibility — measure-twice-cut-once before extending the signature.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.11] — story spec verbatim (the 8 ACs + the EN/FR copy)
- [Source: _bmad-output/implementation-artifacts/9-10-modal-foundation-and-borrower-migration.md] — pattern precedent (Modal macro, modal.js, focus trap, scanner-guard inheritance, integration-test shape, dead-i18n-key drop)
- [Source: CLAUDE.md#Foundation-Rules] — Rules #1 (DRY), #11 (issue tracking), #12 (LOC ceiling), #13 (local testing), #15 (draft PR), #18 (CI gating)
- [Source: CLAUDE.md#Modal-scanner-guard-invariant-story-7-5] — the `dialog[open]` + `[aria-modal="true"]` selector contract that the new modal inherits
- [Source: CLAUDE.md#Key-Patterns#CSRF-synchronizer-token-story-8-2] — why the modal macro takes `csrf_token` as its 8th param and how `_csrf_token` hidden inputs are policed by `templates_audit::forms_include_csrf_token`
- [Source: src/routes/loans.rs:225] — existing `return_loan_handler` (verbatim signature reproduced in AC1 / Task 1)
- [Source: src/templates_audit.rs:35] — `ALLOWED_HX_CONFIRM_SITES` const (current state captured in AC4 / Task 6)
- [Source: templates/pages/loans.html:122-129] — line range of the current `hx-confirm` button
- [Source: templates/pages/borrower_detail.html:78-86] — line range of the current `hx-confirm` button (epics doc said 72; actual is 79 — the spec's line number was off-by-7, fixed here)
- [Source: tests/e2e/specs/journeys/loan-returns.spec.ts] — existing E2E suite (the row-disappearance assertion is the load-bearing contract; AC9 keeps it)
- [Source: tests/e2e/specs/journeys/borrower-loans.spec.ts] — second existing E2E suite (`returnLoanFromBorrowerDetail` helper to relocate per Task 8)

## Dev Agent Record

### Agent Model Used

claude-opus-4-7[1m]

### Debug Log References

### Completion Notes List

**SPEC DRIFT — `loan.return_confirm` had 3 callers, not 2.** The 3rd caller is `loan_row_html` in `src/routes/loans.rs:330` (now line ~426) — a Rust `format!` HTML fragment for the V-code scan-card on `/loans`. The `templates_audit::hx_confirm_matches_allowlist` walks `templates/` only, so this Rust-emitted `hx-confirm=` was invisible to the audit. The story's "Existing-code reality check" missed it.

Decision: extend scope to also migrate the scan-card. Otherwise dropping the i18n key per AC5 would either break the scan-card UX (rust_i18n returns the literal key string when missing) or violate AC5 ("zero callers after migration"). The cleanest reconciliation is full migration. As a consequence:

- `FEEDBACK_TARGETS` allowlist is **3 entries**, not the 2 specified in AC1: `["loan-feedback", "borrower-feedback", "scan-result"]`. The 3rd entry (`scan-result`) is the existing scan-card container `<div id="scan-result">` in `templates/pages/loans.html:31`.
- The validated allowlist still locks the security-load-bearing property — `?target=evil-injected` still 404... err, falls back to `loan-feedback`. AC7 covers this with a dedicated regression test.
- The grep audit at story close reports exactly 3 active `hx-confirm=` sites in `templates/`, matching `ALLOWED_HX_CONFIRM_SITES.len()`.

**Task 4 macro decision — option (a) chosen.** The pre-9-11 `templates/components/modal.html` macro hardcoded `hx-swap="none"` on the form. For 9-11 we need `hx-swap="innerHTML"` + `hx-target="#…"`. Two options were on the table: (a) extend macro signature with optional `hx_target`/`hx_swap` params, (b) wrap-and-override at the fragment level. Askama 0.15 macros do NOT support default args, so option (b) is impossible without duplicating the macro body (DRY violation). Option (a): added two new positional params to `modal()`, updated existing 9-10 callers (`borrower_delete_modal.html` + `modal_test_wrapper.html`) to pass `"", "none"` literals for byte-identical pre-9-10 behavior. Macro grew from 39 LOC to 39 LOC (signature line +1 column, form line +1 conditional — net 0). Two new render tests (`warning_variant_with_hx_target_renders_innerhtml_swap`, `warning_variant_default_omits_hx_target_attribute`) + one byte-identity guard (`warning_variant_byte_identical_across_feedback_targets`) lock the contract.

**`confirm_label` field removal** — the spec said keep it through 9.14 close. Reality check showed both `LoansTemplate.confirm_label` and `BorrowerDetailTemplate.confirm_label` become **dead immediately after the migration** (no template caller, only the dropped `t!("loan.return_confirm")` invocation). Foundation Rule #1 (DRY) prohibits dead plumbing. Dropped both fields + their construction-site `t!()` calls. Documenting deviation here. (The `confirm_label` field on the new `ReturnLoanModalTemplate` and the existing `BorrowerDeleteModalTemplate` are LIVE — they feed the modal macro's confirm-button label.)

**Raw-string delimiter bump in `loan_row_html`** — the migrated scan-card emits `hx-target="#modal-slot"` and `hx-target="#scan-result"` patterns that contain the `"#` sequence, which terminates a `r#"..."#` raw string. Bumped to `r##"..."##`.

**E2E regression-guard test added** — `loan-returns.spec.ts::scanner-guard suppresses scanner burst while modal open` exercises the load-bearing scanner-guard (story 7-5) inheritance: open the return modal, fire `simulateScan(page, "body", "9782070360246")`, assert dialog still visible AND Cancel still focused. Mirror of the 9-10 PR #129 fix pattern.

**Test coverage added:**
- 12 sqlx integration tests in `tests/return_loan_modal.rs` (AC7 happy paths × roles + 404/405 contracts + target validation + invalid-target security guard + post-handler sanity).
- 3 macro render tests in `src/routes/modal_tests.rs` (AC8 byte-identity + new param wiring + backward compat).
- 1 unit test updated in `loans::tests::test_loan_row_html_highlighted` (asserts new `hx-get="…/return-modal"` + `data-modal-trigger`, asserts `hx-confirm=` is gone).
- 2 E2E specs extended: `loan-returns.spec.ts` (scan-card modal flow + new scanner-guard burst test) and `borrower-loans.spec.ts` (relocated helper).
- 1 helper relocated: `returnLoanFromBorrowerDetail` from inline `borrower-loans.spec.ts` into `tests/e2e/helpers/loans.ts` (DRY).

**Final test counts (lib + integration): 918 passing — clippy clean (`-D warnings`), tsc clean, flake gate clean, AC14 grep count == 3.**

### File List

**New files (3):**
- `templates/fragments/return_loan_modal.html` (16 LOC) — Askama fragment that calls the shared modal macro with the warning variant + per-loan target.
- `tests/return_loan_modal.rs` (~430 LOC) — 12 `#[sqlx::test]` integration cases (AC7 + AC11 + AC1 security guard).

**Modified Rust (5):**
- `src/routes/loans.rs` — added `ReturnModalQuery`, `FEEDBACK_TARGETS` const (3 entries), `ReturnLoanModalTemplate`, `return_modal_handler`. Migrated `loan_row_html` (Rust scan-card) to emit modal-trigger markup. Removed `confirm_label` field + `t!("loan.return_confirm")` from `LoansTemplate`. 451 LOC → 559 LOC (well under 2000 ceiling).
- `src/routes/borrowers.rs` — removed dead `confirm_label` field + `t!("loan.return_confirm")` from `BorrowerDetailTemplate`.
- `src/routes/mod.rs` — registered `GET /loans/{id}/return-modal` route.
- `src/routes/modal_tests.rs` — extended `ModalTestWrapper` struct + `render` helper with new `hx_target`/`hx_swap` fields. 3 new tests for the new params + byte-identity guard.
- `src/templates_audit.rs` — `ALLOWED_HX_CONFIRM_SITES` trimmed from 5 to 3 entries (removed `loans.html` + `borrower_detail.html`).

**Modified templates (4):**
- `templates/components/modal.html` — macro signature gained `hx_target` + `hx_swap` params; form line conditionally renders `hx-target` and uses dynamic `hx-swap`. Net LOC unchanged (39).
- `templates/fragments/borrower_delete_modal.html` — pass `"", "none"` to preserve pre-9-11 behavior.
- `templates/fragments/modal_test_wrapper.html` — pass new params through from struct.
- `templates/pages/loans.html` — return button: `hx-confirm=` → `hx-get=…/return-modal?target=loan-feedback` + `data-modal-trigger` + `aria-haspopup="dialog"`.
- `templates/pages/borrower_detail.html` — same migration with `?target=borrower-feedback`.

**Modified i18n (2):**
- `locales/en.yml` — added 3 keys (`loan.return_modal_title`, `_body`, `_confirm`); dropped `loan.return_confirm`.
- `locales/fr.yml` — same shape, FR copy.

**Modified E2E (3):**
- `tests/e2e/helpers/loans.ts` — `returnLoanFromLoansPage` rewritten for modal flow (no more `page.once("dialog")`); NEW `returnLoanFromBorrowerDetail` exported.
- `tests/e2e/specs/journeys/loan-returns.spec.ts` — scan-card AC6 test rewritten for modal flow; new scanner-guard burst regression test (AC9).
- `tests/e2e/specs/journeys/borrower-loans.spec.ts` — dropped inline helper, imports shared one from `helpers/loans.ts`.

**Modified docs (1):**
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — story 9-11 status flip ready-for-dev → in-progress → review (this PR's responsibility per Foundation Rule #16).
