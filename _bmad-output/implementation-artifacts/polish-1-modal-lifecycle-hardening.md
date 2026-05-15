# Polish iteration 1: Modal lifecycle hardening

Status: backlog

**Bundle type:** post-Epic-10 polish iteration (per Epic 10 retro action A5 — production-driven polish bundles, no formal epic). 5 GH issues grouped because they share one fix surface.

**Closes:** #61, #64, #65, #67, #134

**Risk lens — mybibli is in production.** Foundation Rule #6 (code-review default) MUST be honored on this PR per Epic 10 retro action A1. Modal infrastructure is on every admin destructive-action and every borrower/loan modal — a regression here is user-visible immediately.

## Story

As an admin or librarian working with destructive-action modals (permanent delete, deactivate user, return loan, delete borrower / contributor / series, etc.),
I want every modal to **open as a real top-layer dialog, close reliably, and surface failures inline**,
so that I can cancel an action I changed my mind about, recover from a conflict without losing the page state, and never end up with a frozen or duplicated dialog stuck on the page.

## ⚠️ Existing-code reality check

Two distinct modal patterns coexist in the codebase today:

### Pattern A — UX-DR8 Modal component (story 9-10, shipped)
- `templates/components/modal.html` — 40-LOC Askama macro, 4 variants (`delete`, `delete-forever`, `remove`, `warning`).
- `static/js/modal.js` — 197-LOC focus-trap + lifecycle module, watches `#modal-slot` for `<dialog open>` swaps. Handles Tab cycling, Escape, backdrop click, Cancel, background tabindex sweep, focus restoration.
- Mounts via `hx-target="#modal-slot" hx-swap="innerHTML"` so successive opens replace any prior modal (single-modal invariant by construction).
- Used by 5 fragments today: `borrower_delete_modal.html`, `contributor_delete_modal.html`, `series_delete_modal.html`, `return_loan_modal.html`, `admin_user_deactivate_modal.html`.

### Pattern B — Legacy ad-hoc admin modals (stories 8-4 + 8-7, pre-9-10)
Three fragments still use the pre-Modal-component ad-hoc shape:
- `templates/fragments/admin_trash_permanent_delete_modal.html` (story 8-7, ~42 LOC)
- `templates/fragments/admin_ref_delete_modal.html` (story 8-4)
- `templates/fragments/admin_ref_loanable_warning_modal.html` (story 8-4)

Each:
- Emits `<dialog open aria-modal="true">` directly (NO `showModal()` call → no top-layer, no backdrop, no Esc — issue **#65**)
- Mounts into `#admin-modal-slot` via `hx-target="body" hx-swap="beforeend"` (NOT `#admin-modal-slot innerHTML` — duplication on rapid double-click — issue **#67**)
- Uses `hx-delete="<CSS selector string>"` for the Cancel button (literally sends DELETE to a path containing `dialog:has(form[hx-post*='...']`) — 405/404/403, **Cancel never closes** — issue **#61**)
- After successful Confirm: the handler re-renders the panel into `#admin-trash-panel`. The dialog itself is outside that container, so it stays in the DOM (issue **#64**)

**This polish iteration migrates these 3 fragments to the UX-DR8 Modal component**, picking up #61, #64, #65, #67 by construction. Story 9-10's "deferred migration" note (`_bmad-output/implementation-artifacts/9-10-modal-foundation-and-borrower-migration.md`, §Existing-code reality check) explicitly anticipates this — quote: *"DOES NOT migrate the 3 existing admin fragments in the same PR (refactor-during-feature anti-pattern). Migrations of the admin fragments are deferred work, tracked as a `type:code-review-finding` GH issue at story close."*

### Issue #134 — frozen-open on error (Pattern A bug)
Separate concern. `static/js/modal.js`'s `htmx:afterRequest` listener filters error responses out (`if (!isConfirm || detail.failed || detail.successful === false) return;`). When the Confirm button's POST returns non-2xx (409 Conflict from optimistic locking, 403 CSRF drift, etc.), the modal stays open with NO visual feedback. The user has to click Cancel manually. Affects every Pattern-A modal.

**UX decision required (recorded in this spec):**
- Error feedback lands **inside the modal** (inline error banner in the modal body, modal stays open so the user can re-read context and retry or Cancel)
- NOT in the page's `#feedback-list` (which would disappear under the modal's backdrop and feel disconnected from the action that failed)

This decision is binding for AC4 below. Re-open this spec if the UX choice changes.

## Acceptance Criteria

1. **AC1 — `admin_trash_permanent_delete_modal.html` migrated to the UX-DR8 Modal component.**
   - The fragment becomes a thin call site that includes `templates/components/modal.html` with `variant = "delete-forever"`, the right i18n title/body/confirm_label/cancel_label, and `action_method = "POST"` (or `"DELETE"` per the existing handler — verify in Task 1).
   - The fragment mounts into `#modal-slot` (NOT `#admin-modal-slot`) via `hx-target="#modal-slot" hx-swap="innerHTML"`. This brings it under `static/js/modal.js`'s lifecycle automatically — closes #65 (showModal/backdrop/Esc), #67 (duplication on rapid click via `innerHTML` semantics), #61 (Cancel button delegates to the modal-component handler, no CSS-selector-as-URL).
   - The trigger button in `templates/fragments/admin_trash_panel.html` updates its `hx-target` accordingly.
   - The handler (`src/routes/admin.rs::permanent_delete_confirm_modal` — verify in Task 1) renders the new fragment shape.
   - **Server-side close on success** (AC2 below) covers #64.
   - **Existing handler contract MUST be unchanged** beyond the rendered HTML — preserve route, method, form fields (`_csrf_token`, version, entity_type, id), audit-log write, soft-vs-hard delete logic.

2. **AC2 — Successful Confirm closes the modal server-side via `HX-Trigger: modal-close`.**
   - When the trash-purge handler returns success (HTTP 200/204 + panel re-render), it adds the header `HX-Trigger: modal-close`.
   - A new delegated listener in `static/js/modal.js` (or a small dedicated handler — author's choice, document in Dev Agent Record) responds to that custom event by calling `dialog.close()` + `dialog.remove()` (or equivalent — depends on Pattern A's existing close path).
   - This reuses the CLAUDE.md `HX-Trigger → JS-listener` idiom (story 8-2's `csrf-rejected` pattern) — a project-wide convention.
   - **Closes #64 by construction** — the dialog is explicitly removed on success, not orphaned by a panel re-render.
   - **Future-proof:** any new admin handler that needs to close a modal on success uses the same trigger.

3. **AC3 — `admin_ref_delete_modal.html` and `admin_ref_loanable_warning_modal.html` migrated to the UX-DR8 Modal component (same shape as AC1).**
   - Same migration pattern, same `#modal-slot` mount, same `HX-Trigger: modal-close` on success.
   - **Coordinate with `static/js/inline-form.js`** — the reference-data CRUD pattern (CLAUDE.md story 8-4) uses inline-form for in-place editing AND the legacy modal for delete confirmation. The inline-form coordination MUST remain unbroken: `#admin-modal-slot` keeps its existing role for any future inline-form-coordinated overlay; the 2 delete-confirmation modals leave `#admin-modal-slot` and move to `#modal-slot`. **Verify in Task 1** by reading `inline-form.js` end-to-end — if it has a hard-coded reference to `#admin-modal-slot` AS the mount point for delete confirmations, the slot move needs an inline-form patch in the same PR.
   - Same handler-contract preservation as AC1: route/method/CSRF/audit/reactivation logic unchanged.

4. **AC4 — `static/js/modal.js` surfaces Confirm-error feedback INSIDE the open modal (closes #134).**
   - Current behavior: `htmx:afterRequest` early-returns on failed responses, leaving the modal frozen open.
   - New behavior: on `detail.failed === true || detail.successful === false` for a request originating from the Confirm button:
     - Parse the response body as HTML (HTMX has already done the parse for us via `detail.xhr.responseText`).
     - Inject the response into a NEW DOM region inside the modal: `<div data-modal-error class="..."></div>`. The macro `templates/components/modal.html` adds this region (empty by default, hidden via `[data-modal-error:empty]` Tailwind class or equivalent).
     - Modal stays open. User can re-read context, hit Cancel, or fix the upstream condition and retry.
   - **Handler contract addendum:** when a Confirm handler returns non-2xx, the response body MUST be a renderable HTML fragment (FeedbackEntry shape from `feedback_html()` in `src/routes/catalog.rs:33`). Existing handlers already do this for 4xx (CSRF, 409 Conflict in optimistic-lock paths) — verify in Task 1 that no Confirm-handler returns plain-text or JSON on the error path; if any does, add the FeedbackEntry wrapper in the SAME PR (in-scope per #134's coordination clause).
   - **No re-opening on subsequent same-button click** — the existing focus-trap + lifecycle continues to run; only the error-region is new.
   - The macro change: bump `templates/components/modal.html` to include `<div data-modal-error role="alert" aria-live="polite" class="..."></div>` immediately after the body content. Tailwind sizing + colors: amber (variant `warning`/`remove`) / red (variant `delete`/`delete-forever`).

5. **AC5 — `src/templates_audit.rs::hx_confirm_matches_allowlist` invariant still passes.** The allowlist is currently empty (per CLAUDE.md story 7-5 note: *"the allowlist is empty post Epic 9 — any new `hx-confirm=` is BLOCKED outright by the audit"*). This polish iteration introduces no new `hx-confirm=` attributes — it only migrates ad-hoc modals to the UX-DR8 macro. The audit MUST still report 0 occurrences.

6. **AC6 — Unit tests.**
   - `src/routes/admin.rs` — extend `admin_trash_*` unit tests so the rendered HTML of the modal-fetch endpoint exercises the macro path (look for `data-modal-trigger`, `data-modal-default-focus`, the new `data-modal-error` div). Mirror the existing `admin_trash_modal_renders_with_csrf_token` test shape.
   - `src/routes/admin_reference_data.rs` — same for the 2 ref-data delete handlers.
   - New unit test: `permanent_delete_success_sends_hx_trigger_modal_close` (and equivalents for ref-data) — verifies the response header.
   - Migration target test count: +6 to +10 unit tests across the 3 fragments, depending on how many corner cases (CSRF rejection, validation failure, success) get explicit coverage.

7. **AC7 — E2E tests.**
   - NEW `tests/e2e/specs/journeys/admin-modal-lifecycle.spec.ts` (or extend `admin-trash.spec.ts` if it exists) covering:
     - **Smoke:** admin clicks Delete permanently → modal opens (assert `dialog[open]` count == 1) → Cancel → modal closed (assert count == 0 after a `waitFor`) → row still in trash.
     - **Success path:** admin clicks Delete permanently → modal opens → fills name confirmation if required → Confirm → modal closes (HX-Trigger), panel updates, FeedbackEntry shows in `#feedback-list`.
     - **Error path (#134):** admin clicks Delete permanently on an entity that 409s (concurrent deletion — seed by deleting via DB in parallel, or by manipulating version) → Confirm → modal STAYS open, `data-modal-error` populated with the error FeedbackEntry, user can Cancel cleanly.
     - **Rapid-double-click guard (#67):** click Delete permanently twice rapidly via `page.locator(...).click({ clickCount: 2 })` or two sequential clicks within 100ms → assert `dialog[open]` count is always 1 (`innerHTML` swap semantic guarantees this).
   - Reuse `tests/e2e/helpers/loans.ts` patterns (CSRF token from meta tag, `page.request.post` for parallel seed, etc.).
   - Per Foundation Rule #13 §4 (added by Epic 10 retro): the spec MUST be validated via `CI=true npx playwright test --workers=2` before push, not just the default-worker run.

8. **AC8 — Documentation refresh.**
   - `CLAUDE.md` "Key Patterns" — add a paragraph under the existing Modal section (story 7-5 / 9-10) documenting:
     - `HX-Trigger: modal-close` as the canonical server-driven modal-close idiom
     - The error-feedback-INSIDE-modal UX decision and the `data-modal-error` region contract
     - The "all destructive admin modals mount into `#modal-slot`, NOT `#admin-modal-slot`" rule (single source of mounting; `#admin-modal-slot` is reserved for inline-form coordination)
   - **No new Foundation Rule** — this is a pattern extension, not a new discipline.

## Dev Notes (planning hints, NOT prescriptive)

- **Task 1 starts with reading**: `static/js/modal.js` end-to-end (197 LOC), `inline-form.js` for the slot-coupling check (AC3), and the 3 legacy fragments + their handlers. Don't write code before that pass — the slot move (AC3) is the riskiest decision in the spec and depends on what `inline-form.js` actually does.
- **The `HX-Trigger: modal-close` event listener** is best added directly to `modal.js` (same module that already owns the modal lifecycle) rather than a new file. ~10 LOC addition.
- **The error-region (AC4)** needs a CSS rule — `[data-modal-error:empty] { display: none; }` won't work because of how Tailwind's hidden modifier composes; check `.feedback-entry` patterns elsewhere for the CSP-clean conditional-display idiom. Likely solution: server side, conditionally output `<div data-modal-error class="hidden ..."></div>` and have `modal.js` flip `hidden` off when injecting content.
- **The 3 legacy fragments share a similar shape** — write a small Askama include partial first if duplication exceeds 3 sites (the rule-of-three), but the macro itself IS the partial. So the call sites should be ~10 LOC each.
- **CSRF token plumbing**: the legacy fragments already include `<input name="_csrf_token" value="{{ csrf_token|e }}">` — verify after migration that the macro emits this in the form. Story 8-2's `templates_audit.rs::forms_include_csrf_token` invariant must still pass.

## Out of scope (deliberately deferred)

- The 5 already-migrated UX-DR8 modals (`borrower_delete_modal.html` etc.) are NOT touched by this iteration. They already use Pattern A; the #134 fix on `modal.js` (AC4) covers them automatically.
- No changes to scanner-guard 7-5 invariants. The MutationObserver-based `dialog[open]` interception keeps working.
- No changes to `#admin-modal-slot` semantics for non-modal admin UI (inline-form etc.). Only the 3 delete-confirmation modals move out of it.
- No new `hx-confirm=` introductions (the allowlist is permanently empty by audit).

## Test plan (run order)

1. `SQLX_OFFLINE=true cargo clippy --bin mybibli --tests -- -D warnings` — clean.
2. `cargo test --lib` — 796 + new admin tests, all green.
3. `cargo test --test '*'` (integration suite) — green.
4. `./scripts/e2e-reset.sh && CI=true cd tests/e2e && npx playwright test --workers=2 specs/journeys/admin-modal-lifecycle.spec.ts` — new spec, all green.
5. Full CI-shape Playwright (`CI=true npx playwright test --workers=2`) — no regressions on existing modal specs.
6. Manual smoke in browser: open admin trash, click Delete permanently, verify backdrop appears + Esc closes + Cancel closes + Confirm closes-on-success + error stays-open-with-inline-banner.

## Code-review checkpoint (Epic 10 retro A1)

This iteration is the FIRST to reinstate the code-review-as-default discipline per Epic 10 retro action A1. Run `bmad-code-review` (3-layer adversarial) BEFORE merging. Expected Medium+ findings:
- Slot-move ripple effects (anything still listening on `#admin-modal-slot` mutations?)
- HX-Trigger event-name collision check (any existing listener for `modal-close`?)
- Error-region injection — is the response HTML always safe-by-construction? (FeedbackEntry helper already escapes — verify it's used on every error path)
- E2E race conditions on the rapid-double-click test (#67) under 14-worker default-local Playwright (matches Epic 10 §4.1 pattern).

Re-run review after fixes if any Medium+ findings land. Story is clean only when a full review pass produces 0 Medium+ findings (Foundation Rule #6).
