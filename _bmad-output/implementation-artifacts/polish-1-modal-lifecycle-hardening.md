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
Three fragments still use the pre-Modal-component ad-hoc shape. They are NOT equally buggy — a per-fragment audit (recorded here for the implementer) was the first deliverable of this spec's reality check:

| Property | `admin_trash_permanent_delete_modal.html` (story 8-7) | `admin_ref_delete_modal.html` (story 8-4) | `admin_ref_loanable_warning_modal.html` (story 8-4) |
|---|---|---|---|
| Emits `<dialog open>` declaratively (no `showModal()`) | ✅ | ✅ | ✅ |
| `#65` (no top-layer/backdrop/Esc) applies | ✅ | ✅ | ✅ |
| Trigger swap pattern | `hx-target="body" hx-swap="beforeend"` → stacks dialogs | `hx-target="#admin-modal-slot" hx-swap="innerHTML"` → safe | `hx-target="#admin-modal-slot" hx-swap="innerHTML"` → safe |
| `#67` (rapid-double-click duplicates) applies | ✅ | ❌ (innerHTML replaces) | ❌ (innerHTML replaces) |
| Cancel button shape | `hx-delete="{{ modal_close_target }}"` (CSS-selector-as-URL → 403/404/405) | `data-action="admin-modal-close"` → JS-delegated handler in `static/js/inline-form.js:145` | `data-action="admin-modal-close-revert-row"` → JS-delegated handler in `static/js/inline-form.js:152` (with row-revert side-effect) |
| `#61` (Cancel never closes) applies | ✅ | ❌ (delegated handler empties the slot) | ❌ (delegated handler reverts row + empties the slot) |
| Post-success cleanup | Handler renders into `#admin-trash-panel`; dialog (outside that container) is orphaned | Handler renders into `{{ list_target }}`; dialog (inside `#admin-modal-slot`) is orphaned | Handler renders into `{{ row_target }}`; dialog (inside `#admin-modal-slot`) is orphaned |
| `#64` (dialog stays in DOM post-success) applies | ✅ | ✅ | ✅ |

**The Trash modal is the only fragment that hits all four bugs.** The 2 ref-data modals already have a working close lifecycle via `static/js/inline-form.js:145-180` (delegated handlers `admin-modal-close` + `admin-modal-close-revert-row` + a document-level Escape listener at L189-199 that synthesises a click on the close button). The row-revert variant carries non-trivial UX (Cancel must GET `data-row-revert-endpoint` and swap into `data-row-revert-target` before closing — story 8-4 P14) that is NOT representable in the current `templates/components/modal.html` macro contract.

### Pattern A — UX-DR8 Modal component (story 9-10, shipped)
- `templates/components/modal.html` — 40-LOC Askama macro, 4 variants (`delete`, `delete-forever`, `remove`, `warning`).
- `static/js/modal.js` — 197-LOC focus-trap + lifecycle module, watches `#modal-slot` for `<dialog open>` mutations. Handles Tab cycling, Escape, backdrop click, Cancel, background tabindex sweep, focus restoration.
- Mounts via `hx-target="#modal-slot" hx-swap="innerHTML"` so successive opens replace any prior modal (single-modal invariant by construction).
- Used by 5 fragments today: `borrower_delete_modal.html`, `contributor_delete_modal.html`, `series_delete_modal.html`, `return_loan_modal.html`, `admin_user_deactivate_modal.html`.

**`modal.js` ALSO does NOT call `showModal()`** on the swapped-in dialog — it just observes the declarative `<dialog open>` attribute. So **issue #65 is project-wide**, not Pattern-B-only. Fixing it in `modal.js` (one-line addition: when a new `dialog[open]` lands in `#modal-slot`, call `dialog.showModal()`) covers all 5 Pattern A modals AND the trash modal once migrated. A symmetric fix in `inline-form.js`'s mutation observer / handler covers the 2 ref-data modals that stay on Pattern B.

### Migration scope (this iteration)

**Aggressive migration is NOT warranted for the 2 ref-data modals.** Their existing JS lifecycle in `inline-form.js` works; the per-modal UX glue (`admin-modal-close-revert-row`) would have to be re-implemented inside the UX-DR8 macro or in `modal.js`, which is scope creep against story 8-4's accepted UX. Selective migration is the right call:

- **MIGRATE** `admin_trash_permanent_delete_modal.html` to the UX-DR8 Modal component. The trash modal carries 4 of the 5 bugs end-to-end and the simplest UX (type-the-name confirmation — see Dev Notes for how to absorb it). Story 9-10's "deferred migration" note (`_bmad-output/implementation-artifacts/9-10-modal-foundation-and-borrower-migration.md`, §Existing-code reality check) explicitly anticipates this — quote: *"DOES NOT migrate the 3 existing admin fragments in the same PR (refactor-during-feature anti-pattern). Migrations of the admin fragments are deferred work, tracked as a `type:code-review-finding` GH issue at story close."*
- **LEAVE IN PLACE** the 2 ref-data modals on Pattern B. They get the `showModal()` fix via a symmetric patch to `inline-form.js`, which closes #65 for them without touching the working Cancel/Escape lifecycle. #64 is closed via the same `HX-Trigger: modal-close` pattern as the trash modal (AC2), but emitted by the ref-data handlers and listened to by both `modal.js` AND `inline-form.js`.

### Issue #134 — frozen-open on error (Pattern A bug)
Separate concern. `static/js/modal.js`'s `htmx:afterRequest` listener filters error responses out (`if (!isConfirm || detail.failed || detail.successful === false) return;`). When the Confirm button's POST returns non-2xx (409 Conflict from optimistic locking, 403 CSRF drift, etc.), the modal stays open with NO visual feedback. The user has to click Cancel manually. Affects every Pattern-A modal.

**UX decision required (recorded in this spec):**
- Error feedback lands **inside the modal** (inline error banner in the modal body, modal stays open so the user can re-read context and retry or Cancel)
- NOT in the page's `#feedback-list` (which would disappear under the modal's backdrop and feel disconnected from the action that failed)

This decision is binding for AC4 below. Re-open this spec if the UX choice changes.

## Acceptance Criteria

1. **AC1 — `admin_trash_permanent_delete_modal.html` migrated to the UX-DR8 Modal component.**
   - The fragment becomes a thin call site that includes `templates/components/modal.html` with `variant = "delete-forever"`, the right i18n title/body/confirm_label/cancel_label, and `action_method = "POST"` (or `"DELETE"` per the existing handler — verify in Task 1).
   - The fragment mounts into `#modal-slot` (NOT `#admin-modal-slot`) via `hx-target="#modal-slot" hx-swap="innerHTML"`. This brings it under `static/js/modal.js`'s lifecycle automatically — closes **#67** (duplication on rapid click via `innerHTML` semantics) and **#61** (Cancel button delegates to the modal-component handler, no CSS-selector-as-URL).
   - **Type-the-name confirmation is preserved.** The existing UX has the Confirm button disabled until the user types the item name verbatim (the legacy fragment carries `data-confirm-name="{{ item_name|escape }}" data-confirm-btn="confirm-delete-btn"` plus a JS handler somewhere — Task 1 must locate and verify it). The migration MUST keep this UX: either pass the type-to-confirm fields through the macro as new optional parameters OR keep the confirm-button-disabled logic outside the macro and let the call-site wire it. **Verify in Task 1** which path is cleaner — both are acceptable, but the choice MUST be recorded in Dev Agent Record because it affects AC4's error-region placement.
   - The trigger button in `templates/fragments/admin_trash_panel.html` line 76 updates its `hx-target` from `body` to `#modal-slot` and its `hx-swap` from `beforeend` to `innerHTML`.
   - The handler (`src/routes/admin.rs::permanent_delete_confirm_modal` — verify in Task 1) renders the new fragment shape.
   - **Server-side close on success** (AC2 below) covers **#64**.
   - **Existing handler contract MUST be unchanged** beyond the rendered HTML — preserve route, method, form fields (`_csrf_token`, version, entity_type, id), audit-log write, soft-vs-hard delete logic.

2. **AC2 — `HX-Trigger: modal-close` as the project-wide server-driven close idiom (closes #64 across ALL three fragments).**
   - The trash-purge handler (`src/routes/admin.rs::permanent_delete_confirm`) AND the 2 ref-data delete handlers (`src/routes/admin_reference_data.rs::*delete*` — verify exact symbols in Task 1) emit `HX-Trigger: modal-close` on success (HTTP 200 + panel/list re-render).
   - **Two listeners** consume that custom event:
     - `static/js/modal.js` — for the migrated trash modal mounted in `#modal-slot`. Adds a `document.body.addEventListener("modal-close", ...)` that calls `dialog.close()` + clears the slot (or equivalent — re-use the existing close path).
     - `static/js/inline-form.js` — for the 2 ref-data modals that stay in `#admin-modal-slot`. Adds the same listener at the same scope. Behavior: `slot.innerHTML = ""`, mirroring the existing `admin-modal-close` action handler.
   - This reuses the CLAUDE.md `HX-Trigger → JS-listener` idiom (story 8-2's `csrf-rejected` pattern) — a project-wide convention.
   - **Future-proof:** any new admin handler that needs to close a modal on success uses the same trigger; the two listeners cover both slots.

3. **AC3 — `showModal()` lifted to `modal.js` AND `inline-form.js` (closes #65 across ALL 8 modals — 5 Pattern A + 3 Pattern B).**
   - **`modal.js`**: when its existing mutation/swap observer detects a new `dialog[open]` lands in `#modal-slot`, it calls `dialog.showModal()` on it. The declarative `<dialog open>` attribute stays in the templates (it's the scanner-guard selector contract per story 7-5) — `showModal()` is idempotent against an already-open dialog OR we drop the `open` attribute server-side (Task 1 picks one — re-record the call in Dev Agent Record).
   - **`inline-form.js`**: symmetric addition watching `#admin-modal-slot`. The existing module already handles Escape + Cancel for that slot, so it's the natural owner of the `showModal()` call too.
   - **Cross-cutting impact:** this fixes #65 not only for the migrated trash modal but for ALL 5 already-shipped UX-DR8 modals (`borrower_delete_modal.html`, `contributor_delete_modal.html`, `series_delete_modal.html`, `return_loan_modal.html`, `admin_user_deactivate_modal.html`) AND the 2 ref-data modals. **Regression risk is highest here** — verify under code-review that the existing modal flows (delete-borrower, return-loan, etc.) still work end-to-end with the proper top-layer + native Escape + native backdrop behavior. Their custom JS Escape + backdrop-click handlers in `modal.js` may now be redundant or even fight the native ones — Task 1 must reconcile.

4. **AC4 — `static/js/modal.js` surfaces Confirm-error feedback INSIDE the open Pattern A modal (closes #134 for Pattern A only).**
   - Current behavior: `htmx:afterRequest` early-returns on failed responses, leaving the modal frozen open.
   - New behavior: on `detail.failed === true || detail.successful === false` for a request originating from the Confirm button:
     - Parse the response body as HTML (HTMX has already done the parse for us via `detail.xhr.responseText`).
     - Inject the response into a NEW DOM region inside the modal: `<div data-modal-error class="..."></div>`. The macro `templates/components/modal.html` adds this region (initially `hidden`; `modal.js` flips `hidden` off when injecting content).
     - Modal stays open. User can re-read context, hit Cancel, or fix the upstream condition and retry.
   - **Handler contract addendum:** when a Confirm handler returns non-2xx, the response body MUST be a renderable HTML fragment (FeedbackEntry shape from `feedback_html()` in `src/routes/catalog.rs:33`). Existing handlers already do this for 4xx (CSRF, 409 Conflict in optimistic-lock paths) — verify in Task 1 that no Confirm-handler returns plain-text or JSON on the error path; if any does, add the FeedbackEntry wrapper in the SAME PR (in-scope per #134's coordination clause).
   - **No re-opening on subsequent same-button click** — the existing focus-trap + lifecycle continues to run; only the error-region is new.
   - The macro change: bump `templates/components/modal.html` to include `<div data-modal-error role="alert" aria-live="polite" class="hidden ..."></div>` immediately after the body content. Tailwind sizing + colors: amber (variant `warning`/`remove`) / red (variant `delete`/`delete-forever`).
   - **Scope note:** this AC fixes #134 for Pattern A modals (which includes the migrated trash modal after AC1). The 2 ref-data modals stay on Pattern B and inherit their existing error-handling-via-list-re-render behavior. If a real user hits a frozen-open ref-data modal in production, file a follow-up issue and consider extending `inline-form.js` symmetrically; not in scope here.

5. **AC5 — `src/templates_audit.rs::hx_confirm_matches_allowlist` invariant still passes.** The allowlist is currently empty (per CLAUDE.md story 7-5 note: *"the allowlist is empty post Epic 9 — any new `hx-confirm=` is BLOCKED outright by the audit"*). This polish iteration introduces no new `hx-confirm=` attributes — it only migrates the trash modal to the UX-DR8 macro and patches JS modules. The audit MUST still report 0 occurrences.

6. **AC6 — Unit tests.**
   - `src/routes/admin.rs` — extend the existing trash-modal unit tests so the rendered HTML of the modal-fetch endpoint exercises the macro path (look for `data-modal-trigger`, `data-modal-default-focus`, the new `data-modal-error` div). Mirror the existing `admin_trash_modal_renders_with_csrf_token` test shape.
   - `src/routes/admin_reference_data.rs` — NEW tests `<ref_table>_delete_success_emits_hx_trigger_modal_close` for at least 2 of the 4 ref-data tables (parameterize if the pattern repeats cleanly). Same for loanable-warning confirm.
   - `src/routes/admin.rs` — NEW test `permanent_delete_success_emits_hx_trigger_modal_close`.
   - Migration target test count: +5 to +8 unit tests, focused on the HX-Trigger emission and the rendered macro shape.

7. **AC7 — E2E tests.**
   - NEW `tests/e2e/specs/journeys/admin-modal-lifecycle.spec.ts` covering:
     - **Smoke trash:** admin clicks Delete permanently → modal opens (assert `dialog[open]` count == 1 in `#modal-slot`) → Cancel → modal closed (assert count == 0 after a `waitFor`) → row still in trash.
     - **Success path trash:** admin clicks Delete permanently → modal opens → types name in the confirm input → Confirm → modal closes (via `HX-Trigger: modal-close`), panel updates, FeedbackEntry shows in `#feedback-list`.
     - **Error path trash (#134):** admin clicks Delete permanently on an entity whose version is stale (seed by bumping version in a parallel `page.request` call) → Confirm → modal STAYS open in `#modal-slot`, `data-modal-error` populated with the error FeedbackEntry, user can Cancel cleanly.
     - **Rapid-double-click guard (#67) trash:** click Delete permanently twice rapidly via two sequential clicks within 100ms → assert `dialog[open]` count is always 1 (`innerHTML` swap semantic guarantees this).
     - **Smoke ref-data:** admin clicks Delete on a genre/role/etc. → modal opens (assert `dialog[open]` in `#admin-modal-slot`, with `showModal()` proven by `dialog.matches(":modal")` per the spec invariant) → Cancel → closed.
     - **HX-Trigger ref-data:** ref-data delete success → modal in `#admin-modal-slot` is removed (via the new `inline-form.js` listener).
   - Reuse `tests/e2e/helpers/loans.ts` patterns (CSRF token from meta tag, `page.request.post` for parallel seed, etc.).
   - Per Foundation Rule #13 §4 (added by Epic 10 retro): the spec MUST be validated via `CI=true npx playwright test --workers=2` before push, not just the default-worker run.

8. **AC8 — Documentation refresh.**
   - `CLAUDE.md` "Key Patterns" — add a paragraph under the existing Modal section (story 7-5 / 9-10) documenting:
     - `HX-Trigger: modal-close` as the canonical server-driven modal-close idiom (listened in both `modal.js` AND `inline-form.js` — different slots, same event)
     - The error-feedback-INSIDE-modal UX decision and the `data-modal-error` region contract (Pattern A only)
     - The slot-ownership rule: **`#modal-slot` for Pattern A (UX-DR8 macro)**, **`#admin-modal-slot` for Pattern B (ref-data legacy)**. Both slots now call `showModal()` on `dialog[open]` arrival.
     - The `templates_audit.rs::hx_confirm_matches_allowlist` invariant remains empty.
   - **No new Foundation Rule** — this is a pattern extension, not a new discipline.

## Dev Notes (planning hints, NOT prescriptive)

- **Task 1 starts with reading**: `static/js/modal.js` end-to-end (197 LOC), `static/js/inline-form.js` (focus L145-199 for the existing admin-modal-close lifecycle + Escape handler), the trash modal template + handler + trigger button, and locate the type-the-name JS module (search for `data-confirm-name` and `data-confirm-btn` in `static/js/`). Don't write code before that pass — the type-to-confirm wiring (AC1 sub-bullet) and the `showModal()` reconciliation (AC3 — does it conflict with `modal.js`'s manual Escape + backdrop handlers?) are the two riskiest design decisions.
- **AC3 reconciliation question**: `modal.js` currently has its own Escape + backdrop-click + Tab-cycling handlers. `showModal()` provides native Escape + native backdrop close. Some of those manual handlers may become redundant or conflict (e.g., native Escape may close before the JS Tab-trap can react). Plan a Test 1 pass to walk the existing UX-DR8 modal specs (`borrower-delete-modal.spec.ts`, `contributor-delete-modal.spec.ts`, `series-delete-modal.spec.ts`, `return-loan-modal.spec.ts`, `admin_user_deactivate-modal.spec.ts`) and identify any behavior the JS provides that native `showModal()` doesn't (e.g., outside-click on `<dialog>` itself is NOT a standard native close — backdrop click via `event.target === dialog` is custom code that must stay).
- **The `HX-Trigger: modal-close` event listener** is best added directly to `modal.js` (`#modal-slot`) and `inline-form.js` (`#admin-modal-slot`) — same module that already owns each slot's lifecycle. ~10 LOC addition each.
- **The error-region (AC4)** needs a CSS-only conditional-display idiom — `[data-modal-error]:empty { display: none; }` won't work because of how Tailwind's hidden modifier composes; the simplest path is server-side `class="hidden"` initially + `modal.js` removes `hidden` on inject. Check `.feedback-entry` patterns elsewhere for prior art.
- **Trash modal call site is short** — once the type-to-confirm wiring is solved, the migrated fragment should be ~10 LOC (`{% import %}` + `{% call modal::modal(...) %}` + the type-to-confirm input + the hidden CSRF/version fields).
- **CSRF token plumbing**: the legacy trash fragment already includes `<input name="_csrf_token" value="{{ csrf_token|e }}">` — verify after migration that the macro emits this in the form. Story 8-2's `templates_audit.rs::forms_include_csrf_token` invariant must still pass.

## Out of scope (deliberately deferred)

- The 5 already-migrated UX-DR8 modals (`borrower_delete_modal.html`, `contributor_delete_modal.html`, `series_delete_modal.html`, `return_loan_modal.html`, `admin_user_deactivate_modal.html`) keep their existing templates unchanged. They inherit the AC3 `showModal()` fix and the AC4 error-region fix via `modal.js` without per-template edits.
- The 2 ref-data legacy modals (`admin_ref_delete_modal.html`, `admin_ref_loanable_warning_modal.html`) STAY on Pattern B in `#admin-modal-slot`. They get AC2 (`HX-Trigger: modal-close`) and AC3 (`showModal()` via the symmetric `inline-form.js` patch). They do NOT get AC4 (error feedback inside modal) — their existing error-handling-via-list-re-render behavior stays. If a real user hits a frozen-open ref-data modal in production, file a follow-up issue.
- The `admin-modal-close-revert-row` UX (loanable-warning Cancel revert) is preserved as-is. No re-implementation in `modal.js` or the UX-DR8 macro.
- No changes to scanner-guard 7-5 invariants. The MutationObserver-based `dialog[open]` interception keeps working — `showModal()` doesn't change the selector, just the open-state semantics.
- No changes to the cheat-sheet `<dialog>` in `layouts/base.html` (story 9-20) — it's informational, not destructive, and already uses `showModal()` via `static/js/shortcuts.js`.
- No changes to `templates_audit.rs` invariants (CSP, CSRF audit, `hx-confirm=` allowlist).
- No new `hx-confirm=` introductions (the allowlist is permanently empty by audit).

## Test plan (run order)

1. `SQLX_OFFLINE=true cargo clippy --bin mybibli --tests -- -D warnings` — clean.
2. `cargo test --lib` — 796 + new admin tests, all green.
3. `cargo test --test '*'` (integration suite) — green.
4. `./scripts/e2e-reset.sh && CI=true cd tests/e2e && npx playwright test --workers=2 specs/journeys/admin-modal-lifecycle.spec.ts` — new spec, all green.
5. Full CI-shape Playwright (`CI=true npx playwright test --workers=2`) — no regressions on existing modal specs.
6. Manual smoke in browser: open admin trash, click Delete permanently, verify backdrop appears + Esc closes + Cancel closes + Confirm closes-on-success + error stays-open-with-inline-banner.

## Code-review checkpoint (Epic 10 retro A1)

This iteration is the FIRST to reinstate the code-review-as-default discipline per Epic 10 retro action A1. Run `bmad-code-review` (3-layer adversarial) BEFORE merging. Expected Medium+ findings to probe:
- **`showModal()` regression risk on the 5 already-shipped UX-DR8 modals** (delete-borrower, return-loan, etc.). Native top-layer + Escape + backdrop semantics may collide with `modal.js`'s manual Tab-cycling, focus-trap, and Cancel-button delegation. Have the reviewer walk each existing modal E2E spec mentally and flag every assertion that relies on a non-native behavior.
- **Slot-move ripple effect on the trash trigger** — `templates/fragments/admin_trash_panel.html:76`'s `hx-target="body"` flips to `#modal-slot`. Anything else still firing on the old target (e.g., OOB swaps that landed in body)?
- **HX-Trigger event-name collision** — is `modal-close` already used anywhere? Grep before adding.
- **Type-to-confirm UX preservation** — the migrated trash modal MUST still disable Confirm until the user types the exact item name. Whatever path AC1 picks (macro extension vs call-site wiring), the E2E test in AC7 must lock the disabled-until-match behavior.
- **Error-region injection (AC4)** — is the response HTML always safe-by-construction? FeedbackEntry helper already escapes; verify it's used on every Confirm-handler error path. Plain-text/JSON returns would inject as raw text and look broken, NOT as an XSS vector (the body is inserted into a `<div>`, not via `eval`), but the UX bug would be ugly.
- **E2E race conditions on the rapid-double-click test (#67)** under 14-worker default-local Playwright (matches Epic 10 §4.1 pattern). The 100ms threshold may flake; consider `clickCount: 2` instead.

Re-run review after fixes if any Medium+ findings land. Story is clean only when a full review pass produces 0 Medium+ findings (Foundation Rule #6).
