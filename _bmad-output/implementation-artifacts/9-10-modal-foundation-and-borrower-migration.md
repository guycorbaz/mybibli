# Story 9.10: Modal component foundation + migration #1 (delete borrower)

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As the project maintainer,
I want a CSP-clean Modal component with focus trap, scanner-guard integration, and 4 destructive variants, plus the first concrete migration (delete borrower) to prove it in production,
so that subsequent migrations (9.11–9.14) are mechanical and the UX-DR8 contract is exercised end-to-end before the rest of the `hx-confirm=` allowlist is emptied.

## ⚠️ Existing-code reality check

Before declaring "Modal foundation NEW", verify what's already shipped:

- **Three `<dialog open aria-modal="true">` modals already exist as ad-hoc fragments** for the admin UI:
  - `templates/fragments/admin_ref_delete_modal.html` (story 8-4 reference-data delete)
  - `templates/fragments/admin_ref_loanable_warning_modal.html` (story 8-4)
  - `templates/fragments/admin_trash_permanent_delete_modal.html` (story 8-7)
  
  Each is a self-contained fragment that the admin handlers render directly into `<div id="admin-modal-slot">` in `layouts/base.html`. No shared component, no focus trap, no centralized variant matrix. Story 9.10 introduces the **shared, parameterized macro** that these (and all future) destructive confirmations will eventually share — but **DOES NOT migrate the 3 existing admin fragments** in the same PR (refactor-during-feature anti-pattern). Migrations of the admin fragments are deferred work, tracked as a `type:code-review-finding` GH issue at story close.

- **`static/js/scanner-guard.js` (story 7-5)** already protects modals via `MutationObserver` watching `dialog[open]` + `[aria-modal="true"]`. The new component MUST use that same selector shape so it inherits the protection automatically — no new scanner-guard wiring needed.

- **No focus-trap helper exists today.** This story introduces the FIRST one. The trap MUST be reusable: story 9.16 (connection-lost overlay) and any future modal will reuse it.

- **`templates/pages/borrower_detail.html` line 27** (delete-borrower button) currently uses `hx-delete` + `hx-confirm="…"` (plain-browser-confirm dialog). The handler `DELETE /borrower/:id` is unchanged by this story — the migration only changes the trigger from a confirm dialog to the new Modal flow.

- **`ALLOWED_HX_CONFIRM_SITES` in `src/templates_audit.rs`** currently contains 5 entries (totaling 6 occurrences). After 9-10: borrower_detail.html drops from 2 → 1 (delete-borrower migrated; return-loan stays for 9.11). Total occurrences: 6 → 5.

## Acceptance Criteria

1. **AC1 — NEW `templates/components/modal.html` Askama macro** with the canonical UX-DR8 Modal contract:
   - Parameters: `variant` (one of `"delete"`, `"delete-forever"`, `"remove"`, `"warning"`), `title`, `body_html` (HTML-escaped at the call site by the handler), `confirm_label`, `cancel_label`, `action_url`, `action_method` (one of `"DELETE"`, `"POST"`).
   - Rendered as `<dialog open aria-modal="true">` so it inherits scanner-guard 7-5 protection automatically (verified by manual grep of `MODAL_SELECTOR` in `static/js/scanner-guard.js:38`).
   - The 4 variants share one macro body but differ in:
     - **Confirm-button color**: `delete` + `delete-forever` use red (`bg-red-600 hover:bg-red-700`), `remove` uses amber (`bg-amber-600 hover:bg-amber-700`), `warning` uses indigo (`bg-indigo-600 hover:bg-indigo-700`).
     - **Title icon** (small inline SVG via Tailwind sizing): red exclamation triangle for `delete`/`delete-forever`, amber circle for `remove`/`warning` (or omit entirely if SVG inlining is too heavy — decide in Task 1, document in Dev Agent Record). UX-DR24 forbids inline `style=` only — inline SVGs as element children are CSP-clean.
     - **Default focus target**: Cancel button always (UX-DR8 invariant — Cancel never destroys). The Confirm button is the SECOND tab stop.
   - Macro file ≤ 60 LOC (roomier than 9-8's 30-LOC ceiling because 4 variants share a body — verified at story close).

2. **AC2 — NEW `static/js/modal.js` focus-trap module** (separate from scanner-guard.js, which keeps its single-responsibility role):
   - On every `htmx:afterSwap` whose `e.detail.target.id === "modal-slot"`, scan the slot for a `<dialog open>` element. If found:
     - **Focus trap**: capture `Tab` and `Shift+Tab` keydowns inside the dialog; cycle within `dialog [tabindex]:not([tabindex="-1"]), dialog button, dialog [href], dialog input, dialog textarea, dialog select` (focusable elements). Tab from last → first; Shift+Tab from first → last.
     - **Initial focus**: move focus to the Cancel button (per UX-DR8). Cancel button MUST carry `data-modal-default-focus` so the JS finds it without coupling to button text.
     - **Background tabindex sweep**: set `tabindex="-1"` on every element in `body > *:not(#modal-slot)` that was previously focusable; restore on close. (Use a WeakSet to remember the original `tabindex` for restoration. AVOID `aria-hidden="true"` on `<body>` — Chrome's "blocked aria-hidden on focused element" warning.)
     - **Escape closes**: `keydown` Escape → call `dialog.close()` + clear `#modal-slot.innerHTML` + restore focus to the trigger (`document.querySelector('[data-modal-trigger][data-pressed="true"]')` — the trigger marks itself with `data-pressed` on click).
     - **Outside click closes**: Click on the `<dialog>` element itself (the backdrop, NOT its children) → close. Use `event.target === dialog` discrimination.
   - On dialog close (via Escape, outside click, Cancel button, or successful Confirm via HTMX `htmx:afterRequest` on the Confirm button), restore the background `tabindex`s + focus the trigger.
   - **Defense-in-depth fallback**: if the dialog doesn't carry `[data-modal-default-focus]`, focus falls to the first focusable element. Logged at `console.warn` for dev visibility.
   - File ≤ 200 LOC (well under any ceiling — single-responsibility focus trap).

3. **AC3 — `<div id="modal-slot">` added to `layouts/base.html`** (sibling of the existing `#admin-modal-slot`, NOT a rename — admin modals stay on their dedicated slot for the existing inline-form coordination logic in `static/js/inline-form.js`). Empty by default; HTMX swaps in modal HTML.
   - Position: directly after `#admin-modal-slot` for visual grouping (both are end-of-body slots so they overlay correctly).
   - Aria: no special attributes (the modal it wraps carries `aria-modal="true"`).

4. **AC4 — NEW route `GET /borrower/:id/delete-modal`** in `src/routes/borrowers.rs` (or wherever the existing borrower handlers live — verify in Task 1):
   - Returns the rendered modal fragment via the `modal::macro` shape from AC1.
   - Pre-translates 4 i18n keys: title (`borrower.delete_modal_title` — "Delete borrower {name}?"), body (`borrower.delete_modal_body` — "This will move the record to Trash."), confirm (`borrower.delete_modal_confirm` — "Delete"), cancel (`common.cancel` — "Cancel" / "Annuler").
   - HTML-escapes the borrower name via `crate::utils::html_escape` before embedding in `body_html`.
   - **Role gate**: requires Admin (mirrors the existing `DELETE /borrower/:id` handler — verify in Task 1).
   - Returns 404 if the borrower is soft-deleted or not found.
   - Direct browser navigation (non-HTMX request) returns 405 Method Not Allowed (the modal fragment is meaningless without the page context).

5. **AC5 — Migrate `templates/pages/borrower_detail.html` line 27** delete-borrower button:
   - Before: `<button hx-delete="/borrower/{{ borrower.id }}" hx-confirm="{{ confirm_delete }}" hx-target="body" …>`
   - After: `<button hx-get="/borrower/{{ borrower.id }}/delete-modal" hx-target="#modal-slot" hx-swap="innerHTML" data-modal-trigger …>`
   - The `data-modal-trigger` attribute is what `static/js/modal.js` uses to find the trigger for focus-restoration on close. The JS sets `data-pressed="true"` on click and clears it on close.
   - The handler `DELETE /borrower/:id` is unchanged — the Confirm button inside the rendered modal carries `hx-delete="/borrower/{{id}}"`. Successful response: redirect to `/borrowers` (HX-Redirect) and FeedbackEntry via OOB swap.

6. **AC6 — Update `src/templates_audit.rs::ALLOWED_HX_CONFIRM_SITES`**:
   - Before: `("templates/pages/borrower_detail.html", 2),`
   - After: `("templates/pages/borrower_detail.html", 1),`
   - Total grandfathered count: 6 → 5. The audit doc-comment at the top of the const stays as-is until 9-14 (final cleanup).
   - No new entries added (the new modal-fragment route uses `data-modal-trigger`, NOT `hx-confirm`).

7. **AC7 — i18n: 5 NEW keys per locale** (EN + FR), all under existing blocks:
   - `common.cancel: "Cancel" / "Annuler"` (NEW shared key — many future modals reuse it)
   - `borrower.delete_modal_title: "Delete borrower {name}?" / "Supprimer l'emprunteur {name} ?"` (NOTE: braces inside the value, since `rust_i18n::t!` doesn't support interpolation by default in this project — handler does `.replace("{name}", &escaped_name)` after translation; document in Dev Agent Record. ALTERNATIVE: use `%{name}` and `t!("..", name = &escaped_name)` if rust_i18n is configured for it — verify in Task 1.)
   - `borrower.delete_modal_body: "This will move the record to Trash. The borrower can be restored from the admin Trash panel within 30 days."`
   - `borrower.delete_modal_confirm: "Delete" / "Supprimer"`
   - `borrower.delete_modal_focus_lost: ""` — INTENTIONALLY EMPTY for now; reserved for future "focus restored" announcement (out of 9.10 scope). Skip if rust_i18n complains about empty values; track as deferred.

8. **AC8 — Macro unit tests** (NEW file `src/templates_audit.rs::tests` block OR co-located in a new `src/components/modal_tests.rs` if `templates_audit.rs` exceeds Rule #12):
   - 4 cases — one per variant — assert the rendered HTML contains:
     - `<dialog open aria-modal="true">` (scanner-guard contract)
     - The variant's confirm-button color class (red/red/amber/indigo)
     - The Cancel button as the FIRST `<button>` and carries `data-modal-default-focus`
     - The HTML-escaped title + body
     - The action-method attribute (`hx-delete` for `delete`, `hx-post` for `delete-forever`/`remove`/`warning`)
   - 1 negative case: invalid variant defaults to `warning` (defensive — never panics on bad input).

9. **AC9 — JS focus-trap unit tests** (DEFERRED — no JS test harness configured per AC15 of story 9-9). File a `type:code-review-finding` GH issue at story close.

10. **AC10 — Templates audit stays green**: `cargo test no_inline_markup_in_templates` and `cargo test hx_confirm_matches_allowlist` both pass after the migration. The new `modal.html` macro is CSP-clean (no `<script>`, `<style>`, `style=`, `onclick=`); the `data-modal-trigger` attribute is allowed (no event-handler form).

11. **AC11 — Integration tests** (NEW `tests/borrower_delete_modal.rs`, sibling of `tests/volume_detail_loan_status.rs`):
    - `get_delete_modal_returns_200_with_dialog_for_admin_request` — admin session, GET `/borrower/:id/delete-modal`, returns 200 + body contains `<dialog open aria-modal="true">` + the borrower name escaped.
    - `get_delete_modal_returns_403_for_librarian_request` — librarian session, returns 403 (or 303 → /login if the existing handler shape is "redirect anonymous"; verify in Task 4).
    - `get_delete_modal_returns_404_for_soft_deleted_borrower` — borrower with `deleted_at = NOW()`, returns 404.
    - `get_delete_modal_returns_404_for_nonexistent_borrower` — id 99999 with no row, returns 404.
    - `get_delete_modal_returns_405_for_non_htmx_request` — direct browser nav (no `HX-Request` header), returns 405.
    - `get_delete_modal_html_escapes_borrower_name` — borrower name is `<script>alert(1)</script>`, returned HTML contains `&lt;script&gt;` and NOT `<script>`.
    - `delete_borrower_via_existing_handler_still_works` — sanity check that the unchanged `DELETE /borrower/:id` handler responds 200 + soft-deletes the row + redirects to /borrowers (proves the migration didn't break the existing contract).

12. **AC12 — E2E test** (extend `tests/e2e/specs/journeys/borrower-loans.spec.ts` OR new `tests/e2e/specs/journeys/borrower-delete-modal.spec.ts` — pick the cleaner placement in Task 6):
    - Smoke test (admin login → `/borrower/:id` → click Delete button → assert modal opens via `await expect(page.locator("#modal-slot dialog[open]")).toBeVisible()` → assert Cancel button is focused via `await expect(page.locator("[data-modal-default-focus]")).toBeFocused()` → press Escape → assert modal closes → click Delete again → press Tab → assert focus moves to Confirm button → press Shift+Tab → assert focus returns to Cancel → click Confirm → assert HX-Redirect to /borrowers + borrower no longer in list).
    - Use `simulateScan` from `tests/e2e/helpers/scanner.ts` to confirm scanner-guard protection: while modal is open, `simulateScan(page, "body", "9782070360246")` should NOT populate any field outside the modal (the keystrokes are blocked by scanner-guard).
    - Per CLAUDE.md "Local Testing Before Push" Rule #13.

13. **AC13 — `templates/fragments/admin_ref_delete_modal.html`, `admin_ref_loanable_warning_modal.html`, `admin_trash_permanent_delete_modal.html` are NOT migrated in this PR.** They keep their ad-hoc shape. File a `type:code-review-finding` GH issue at story close: "Migrate the 3 admin-only modal fragments to the new `components/modal.html` macro for consistency. Out of scope for 9.10 because that would be a refactor-during-feature anti-pattern + the migration mechanics are unproven until 9.11–9.14 land."

14. **AC14 — Foundation Rule #12 LOC discipline**: `borrower_detail.html` was 90 LOC; the migration is +0/−0 net (one button line replaced). `routes/borrowers.rs` grows by ~50 LOC (new handler). `templates_audit.rs` grows by 5 LOC (the unit-test stubs delegate to the new component module). No file should approach 2000 LOC.

15. **AC15 — Coexistence with existing admin modals**: opening the new modal while an `#admin-modal-slot` modal is already open MUST not visually stack (admin modal closes first, OR the new modal opens on top with proper z-index). Document the chosen behavior in Dev Agent Record. The scanner-guard's stack-based `topModal()` already handles nested modals correctly per its source (story 7-5).

## Tasks / Subtasks

- [ ] **Task 1 — NEW `templates/components/modal.html` macro + i18n keys (AC: 1, 7)**
  - [ ] Read `templates/fragments/admin_ref_delete_modal.html` for the existing shape — copy the `<dialog open aria-modal="true">` + Tailwind utility classes; do NOT inherit the inline-form-specific HTMX attrs.
  - [ ] Write the macro at `templates/components/modal.html` with parameters: `variant`, `title`, `body_html` (raw HTML — escape at call site), `confirm_label`, `cancel_label`, `action_url`, `action_method`.
  - [ ] 4 variants share the body but pick distinct confirm-button colors via a `{% match variant %}` block.
  - [ ] Cancel button as FIRST `<button>`, carries `data-modal-default-focus` AND `data-modal-cancel`. Click handler: HTMX `hx-on:click="document.getElementById('modal-slot').innerHTML=''"` is FORBIDDEN by CSP — use a sibling JS handler in `static/js/modal.js` listening for click on `[data-modal-cancel]`.
  - [ ] Confirm button: `hx-{action_method}="{action_url}"`, `hx-target="body"` (or `#feedback-list` if the action returns a FeedbackEntry; verify in Task 4), `hx-on:close-modal="..."` is FORBIDDEN — use the post-success modal-close logic in JS via `htmx:afterRequest`.
  - [ ] Add 5 NEW i18n keys to `locales/en.yml` + `locales/fr.yml` per AC7. Verify the project's `rust_i18n::t!` interpolation syntax (likely `%{name}` per `format!` style — grep for `t!.*name = ` in the codebase to confirm).
  - [ ] Run `touch src/lib.rs && cargo build` to force i18n proc-macro rebuild.

- [ ] **Task 2 — NEW `static/js/modal.js` focus-trap module + base.html slot wiring (AC: 2, 3)**
  - [ ] Create `static/js/modal.js`. Module pattern: IIFE, uses `MutationObserver` on `#modal-slot` to detect dialog open/close.
  - [ ] On dialog open: scan focusable elements, set initial focus, install Tab/Shift+Tab/Escape/click-backdrop handlers.
  - [ ] On dialog close: remove handlers, restore background `tabindex`s, focus the trigger (`[data-modal-trigger][data-pressed="true"]`).
  - [ ] Add `<div id="modal-slot"></div>` to `layouts/base.html` directly after `<div id="admin-modal-slot"></div>` (verify exact location).
  - [ ] Add `<script src="/static/js/modal.js"></script>` to `base.html` after the existing `<script src="/static/js/inline-form.js">` (load order: scanner-guard → inline-form → modal).
  - [ ] CSP-clean: no inline event handlers, no `style=`. Pure JS module.

- [ ] **Task 3 — NEW handler `GET /borrower/:id/delete-modal` (AC: 4, 11)**
  - [ ] Locate the existing borrower handlers (likely `src/routes/borrowers.rs`); add the new GET handler.
  - [ ] Pre-translate 4 i18n keys; HTML-escape the borrower name; render the modal macro.
  - [ ] Role gate: Admin only (mirror `DELETE /borrower/:id` shape — verify the existing handler).
  - [ ] Soft-delete check: `WHERE deleted_at IS NULL` on the borrower lookup; return 404 if missing.
  - [ ] Non-HTMX request → 405. Mirror the pattern used by other modal-fragment handlers (verify in `routes/admin_reference_data.rs` how `admin_ref_delete_modal.html` is served).
  - [ ] Register the route in `src/routes/mod.rs` (likely as `.route("/borrower/:id/delete-modal", get(borrowers::delete_modal))`).
  - [ ] Build the integration test file `tests/borrower_delete_modal.rs` per AC11 (7 `#[sqlx::test]` cases).

- [ ] **Task 4 — Migrate `templates/pages/borrower_detail.html` line 27 (AC: 5, 6)**
  - [ ] Replace the `<button hx-delete=…>` with `<button hx-get="/borrower/{{ borrower.id }}/delete-modal" hx-target="#modal-slot" hx-swap="innerHTML" data-modal-trigger …>`.
  - [ ] Remove the `confirm_delete` field from `BorrowerDetailTemplate` if it has no other uses (grep first).
  - [ ] Update `src/templates_audit.rs::ALLOWED_HX_CONFIRM_SITES`: borrower_detail.html count 2 → 1.
  - [ ] Run `cargo test hx_confirm_matches_allowlist` to verify the audit passes.

- [ ] **Task 5 — Macro unit tests (AC: 8, 10)**
  - [ ] Add a new `mod modal_tests` (or extend `src/templates_audit.rs::tests`) with 5 `#[test]` cases — one per variant + 1 negative.
  - [ ] Each test instantiates a tiny Askama template wrapper that calls the macro with fixed args, renders, and asserts the AC8 invariants.
  - [ ] Run `cargo test no_inline_markup_in_templates` to verify the macro doesn't trip the CSP audit.

- [ ] **Task 6 — E2E spec (AC: 12)**
  - [ ] Decide placement: extend `tests/e2e/specs/journeys/borrower-loans.spec.ts` OR new file. Recommendation: NEW `tests/e2e/specs/journeys/borrower-delete-modal.spec.ts` because the modal is its own user surface and the test will be reused for 9.11–9.14 patterns.
  - [ ] Spec ID for `specIsbn`: `"BD"` (borrower-delete) — verify uniqueness vs other specs.
  - [ ] Use `loginAs(page, "admin")` for the seed phase + the modal interaction.
  - [ ] Use `simulateScan` to verify scanner-guard protection while modal is open.
  - [ ] No `waitForTimeout` (CI flake gate).

- [ ] **Task 7 — Verify and document (AC: 1–15)**
  - [ ] `wc -l templates/components/modal.html static/js/modal.js src/routes/borrowers.rs templates/pages/borrower_detail.html` — verify no surprise growth.
  - [ ] `SQLX_OFFLINE=true cargo build && cargo clippy --all-targets -- -D warnings` — clean.
  - [ ] `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test` — full suite green.
  - [ ] Templates audit + i18n EN/FR mirror + tsc on tests/e2e/ all green.
  - [ ] CI flake gate (`grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/`) clean.
  - [ ] Manual smoke (`MYBIBLI_SKIP_SETUP=1 cargo run`):
    - As admin, browse to `/borrower/:id`, click Delete → modal opens with focus on Cancel.
    - Press Escape → closes; press Tab inside the modal → focus cycles to Confirm and back; click Confirm → borrower soft-deleted, redirect to /borrowers.
    - Open the modal, then USB-scan a barcode — the field on the page behind the modal MUST NOT receive any keystrokes.
  - [ ] Update Dev Agent Record at the bottom of this file: Files touched, decisions (modal-slot vs. admin-modal-slot rename rejected; rust_i18n interpolation syntax used; per-variant icon decision), drift discoveries (3 admin modal fragments NOT migrated — deferred), JS unit tests deferred per AC9.
  - [ ] Update `_bmad-output/implementation-artifacts/sprint-status.yaml`: `9-10-modal-foundation-and-borrower-migration: ready-for-dev → in-progress` at start, `→ review` at end (only this line + `last_updated`, per CLAUDE.md rule 16).
  - [ ] Open draft PR at first commit (Foundation Rule #15). Title: `Story 9-10: Modal component foundation + delete-borrower migration (#NN)`.

## Dev Notes

### Source tree references

| Concern | File / location | Notes |
|---|---|---|
| **NEW** Modal macro | `templates/components/modal.html` | Sibling of `loan_status_badge.html`, `filter_tag.html`. Parameterized by 7 args, 4 variants. |
| **NEW** Focus-trap JS | `static/js/modal.js` | Single-responsibility: focus trap + Escape close + outside-click close + focus restoration. NOT scanner-guard (already separate). |
| **NEW** Modal slot | `layouts/base.html` (after `#admin-modal-slot`) | Empty `<div id="modal-slot">`. HTMX swaps modal HTML in. |
| **NEW** Handler | `src/routes/borrowers.rs::delete_modal` | Returns rendered modal fragment. Admin-only. |
| Existing admin modal slot | `layouts/base.html` (`<div id="admin-modal-slot">`) | UNCHANGED — admin reference-data modals stay coupled to `static/js/inline-form.js`. |
| Existing scanner-guard | `static/js/scanner-guard.js` | UNCHANGED — already protects `dialog[open]` + `[aria-modal="true"]`. New modal inherits automatically. |
| Existing borrower handlers | `src/routes/borrowers.rs` | DELETE handler unchanged; new GET handler co-located. |
| Existing borrower detail | `templates/pages/borrower_detail.html:27` | One-line migration: `hx-delete + hx-confirm` → `hx-get + hx-target + data-modal-trigger`. |
| Audit allowlist | `src/templates_audit.rs::ALLOWED_HX_CONFIRM_SITES:35-41` | Count drops 2 → 1 for `borrower_detail.html`. |
| Existing admin modal fragments (DEFERRED migration) | `templates/fragments/admin_ref_delete_modal.html`, `admin_ref_loanable_warning_modal.html`, `admin_trash_permanent_delete_modal.html` | NOT migrated in 9.10. File `type:code-review-finding` GH issue at story close. |
| i18n locales | `locales/en.yml`, `locales/fr.yml` | +5 keys (1 `common.cancel`, 4 `borrower.delete_modal_*`). |
| Existing E2E borrower tests | `tests/e2e/specs/journeys/borrower-loans.spec.ts` | Sibling for the new spec. |

### Anti-patterns to avoid

- **Migrating the 3 existing admin modal fragments in the same PR.** Refactor-during-feature anti-pattern. The new macro's mechanics are unproven until 9.11–9.14 land; migrating the admin fragments now would multiply the blast radius. File a deferred GH issue.
- **Renaming `#admin-modal-slot` to `#modal-slot`.** The admin slot is coupled to `static/js/inline-form.js` lifecycle logic. Renaming would entangle two surfaces with different contracts. Use a sibling `<div id="modal-slot">`.
- **Adding `aria-hidden="true"` to `<body>` when modal opens.** Chrome warns "blocked aria-hidden on focused element". Use `tabindex="-1"` sweep instead.
- **Reimplementing the scanner-guard in `modal.js`.** Story 7-5's `static/js/scanner-guard.js` already handles `dialog[open] + [aria-modal="true"]` via MutationObserver. The new component MUST use this exact selector shape so it inherits protection. Do NOT add a parallel keystroke interceptor.
- **Inline `style="display: none"` on closed modal slot.** CSP forbids inline styles. Use `class="hidden"` (Tailwind) or empty innerHTML.
- **`hx-on:click=` or `hx-on::after-request=` on the modal buttons.** CSP allows `hx-on::*` only with the `script-src 'self'` carve-out — verify in Task 1; project convention favors a sibling JS module listening for the event. Inline `onclick=` is forbidden.
- **Focus trap implemented as a wrapper that swallows ALL keys.** Forwards Tab/Shift+Tab/Escape only — printable keys (and Enter for forms) MUST pass through to the focused input.
- **Adding the new modal slot inside `<main>` instead of at end of `<body>`.** End-of-body slot is the documented pattern for overlays (the existing `#admin-modal-slot` follows this).

### Architecture compliance

- **Error handling:** Handler uses `AppError::NotFound` for missing borrower; `AppError::MethodNotAllowed` (or 405 status response) for non-HTMX requests; `AppError::Forbidden` (403 + i18n) for non-Admin.
- **Logging:** No PII logged on the modal-fragment endpoint. `tracing::debug!(borrower_id = %id, "delete modal requested")` for traceability.
- **DB query discipline:** Borrower lookup MUST include `WHERE deleted_at IS NULL`. Locked by `get_delete_modal_returns_404_for_soft_deleted_borrower` integration test.
- **CSP middleware:** Already wraps the handler. Modal HTML is server-rendered via Askama, no inline markup. The new `static/js/modal.js` is loaded via `<script src=>` in base.html (already CSP-allowed).
- **Pool access:** Handler takes `state.pool: DbPool`. No new connection.
- **Foundation Rule #14 one-branch-one-story:** Verify `git status` clean + on `main` + `git pull --ff-only` before starting; cut `story/9-10-modal-foundation-and-borrower-migration`. Open a draft PR (Rule #15) at the first commit.
- **Foundation Rule #12 LOC ceiling:** `routes/borrowers.rs` should stay under 2000 (verify pre-patch LOC in Task 3).
- **Foundation Rule #2 unit tests:** Co-located macro tests in `templates_audit.rs::tests` or sibling module. JS focus-trap tests deferred per AC9 (no JS test harness).
- **Foundation Rule #3 E2E tests:** New smoke test exercises the full delete-borrower flow with modal.
- **Foundation Rule #18 CI gating:** Wait for CI green before merging the PR.

### Library / framework requirements

- **Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + Tailwind CSS v4 + HTMX 2.0** — no version changes, no new dependencies.
- **rust_i18n** — already wired. The interpolation syntax for `{name}` placeholders may require `%{name}` + `t!("..", name = &val)`; verify pre-patch (grep for `t!.*=` in the codebase).
- **Native `<dialog>` element** — supported in all modern browsers (Safari 15.4+, Chrome 37+, Firefox 98+). No polyfill needed.
- **MutationObserver** — used by scanner-guard already. No new browser API.

### File structure requirements

| File | Action | Rough size |
|---|---|---|
| `templates/components/modal.html` | **create** | ≤ 60 LOC (4 variants share body) |
| `static/js/modal.js` | **create** | ~150 LOC (focus trap + Escape + outside-click + focus restoration) |
| `templates/layouts/base.html` | **edit** | +2 LOC (modal slot + script tag) |
| `src/routes/borrowers.rs` | **edit** | +~50 LOC (new GET handler) |
| `src/routes/mod.rs` | **edit** | +1 line route registration |
| `templates/pages/borrower_detail.html` | **edit** | 1 line replaced (button migration) |
| `src/templates_audit.rs` | **edit** | 1 line changed (count 2 → 1) + ~30 LOC for macro tests |
| `locales/en.yml` | **edit** | +5 keys |
| `locales/fr.yml` | **edit** | +5 keys mirror |
| `tests/borrower_delete_modal.rs` | **create** | ~250 LOC (7 `#[sqlx::test]` cases + helpers) |
| `tests/e2e/specs/journeys/borrower-delete-modal.spec.ts` | **create** | ~80 LOC (1 smoke test) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **edit** | only the `9-10-…` line + `last_updated` |
| `_bmad-output/implementation-artifacts/9-10-modal-foundation-and-borrower-migration.md` | **edit** | this file — Status, Tasks checked, Dev Agent Record |

### Testing requirements

- **Coverage target:** every AC has at least one test (unit OR integration OR E2E). AC9 (JS focus-trap unit tests) is explicitly deferred.
- **AC8 macro tests** (5 cases) lock the variant rendering contracts.
- **AC10 templates audit** (2 existing tests, no changes needed — they MUST stay green after the migration).
- **AC11 integration tests** (7 `#[sqlx::test]` cases) lock the handler end-to-end.
- **AC12 E2E test** (1 smoke) locks the full user journey including focus trap + scanner-guard.

### Project structure notes

This story is a **foundation + first-migration** story — the macro and JS module are NEW infrastructure that 9.11–9.14 will reuse mechanically. Three intentional design decisions:

1. **NEW `#modal-slot` sibling of `#admin-modal-slot`** (not a rename). The admin slot is coupled to `static/js/inline-form.js` (story 8-4 InlineForm pattern). Keeping them separate avoids entangling two surfaces with different contracts.

2. **Focus-trap JS in its OWN module** (`static/js/modal.js`), not bolted onto `scanner-guard.js`. Single-responsibility — scanner-guard handles keystroke routing, modal.js handles focus management. The two modules cooperate via the same `<dialog open aria-modal="true">` shape but never call each other.

3. **3 existing admin modal fragments NOT migrated** (refactor-during-feature anti-pattern). The new macro's mechanics need to be proven by stories 9.11–9.14 (4 more migrations) before risking the admin surface.

4. **JS unit tests deferred** per AC9. No JS test harness configured in the project (mirror of 9-9 AC15). E2E coverage in AC12 is the load-bearing test for the focus-trap behavior. File a `type:code-review-finding` GH issue at story close.

## References

- [Story 9.10 spec — `_bmad-output/planning-artifacts/epics.md` lines 1372-1390](../planning-artifacts/epics.md)
- [UX-DR8 Modal component — `_bmad-output/planning-artifacts/ux-design-specification.md`](../planning-artifacts/ux-design-specification.md)
- [`static/js/scanner-guard.js` — story 7-5 modal protection (REUSED, no changes)](../../static/js/scanner-guard.js)
- [`static/js/inline-form.js` — story 8-4 InlineForm slot lifecycle (PARALLEL, no changes)](../../static/js/inline-form.js)
- [`templates/fragments/admin_ref_delete_modal.html` — existing modal shape reference (NOT migrated in 9.10)](../../templates/fragments/admin_ref_delete_modal.html)
- [`templates/pages/borrower_detail.html` — migration target (line 27)](../../templates/pages/borrower_detail.html)
- [`src/templates_audit.rs::ALLOWED_HX_CONFIRM_SITES` — frozen allowlist (count drops 6 → 5)](../../src/templates_audit.rs)
- [`CLAUDE.md` — Foundation Rules + UX-DR24 (Tailwind only) + Modal scanner-guard invariant (story 7-5)](../../CLAUDE.md)
- [Story 9-9 spec (canonical patterns: NEW narrow handler, deferred JS unit tests, drift-discoveries section) — `9-9-home-scanner-state-machine.md`](./9-9-home-scanner-state-machine.md)
- [Story 9-8 spec (canonical patterns: NEW Askama macro, role-aware rendering, soft-degrade) — `9-8-loan-status-role-aware.md`](./9-8-loan-status-role-aware.md)

## Dev Agent Record

### Agent Model Used

`claude-opus-4-7` (1M context).

### Debug Log References

_(to be filled by dev agent)_

### Completion Notes List

_(to be filled by dev agent)_

### File List

_(to be filled by dev agent)_

### Change Log

_(to be filled by dev agent)_
