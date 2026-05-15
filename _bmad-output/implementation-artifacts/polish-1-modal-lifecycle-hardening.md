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

### Issue #134 — frozen-open on error (Pattern A bug, nuanced)

Separate concern. `static/js/modal.js`'s `htmx:afterRequest` listener filters error responses out (`if (!isConfirm || detail.failed || detail.successful === false) return;`). When the Confirm button's POST returns non-2xx, the listener early-returns and never calls `close()`. The modal stays open.

**But the "no visible feedback" framing in the original #134 report is more nuanced than first appears.** A reality-check of every error path showed three distinct behaviors currently shipping:

| Error path | Status | HX-Retarget emitted? | UX today |
|---|---|---|---|
| Trash 4xx (self-delete, last-admin, name-mismatch) | 403/400 | ❌ — handler returns `(StatusCode, Html(...))` directly | **Fully silent.** HTMX's default `responseHandling` for 4xx is `swap:false` → body dropped → user sees nothing. |
| Any `AppError::Conflict` (trash 409 version mismatch, ref-data in-use, return-loan optimistic-lock race) | 409 | ✅ — `AppError::Conflict::IntoResponse` (`src/error/mod.rs:140-154`) emits `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` (story 8-4 P18) | Feedback lands in `#feedback-list` BUT it sits behind the modal's translucent backdrop (`bg-black bg-opacity-50`). Partially readable; not obvious it's the failure for the action you just clicked. |

Issue #134's reporter most likely hit the second case (modal stays open, feedback fuzzy behind backdrop) and read it as "no visible feedback". The first case (fully silent) is the one the trash modal lives in today.

**UX decision (recorded — user-approved 2026-05-15):**
- Error feedback lands **inside the modal** in a dedicated `data-modal-error` region, NOT behind the backdrop nor in `#feedback-list`. Closer to the action that failed, no backdrop occlusion, modal stays open so the user can re-read context and retry or Cancel.
- **Server-side opt-out of `AppError::Conflict`'s `HX-Retarget` when the request originated from a modal Confirm.** A new `X-Modal-Confirm: true` request header (set by `modal.js` via `htmx:configRequest`) signals the server to omit the `HX-Retarget` / `HX-Reswap` headers in `AppError::Conflict::IntoResponse`. Result: the body lands in the response normally and `modal.js`'s listener injects it into `data-modal-error`. **Non-modal forms (ref-data list-level deletes, etc.) keep the existing retarget-to-feedback-list behavior** — story 8-4 P18's intent is preserved for that context.
- **A11y:** `role="alert"` alone on the error region. Per WAI-ARIA spec this implies `aria-live="assertive"` — screen readers announce immediately. Appropriate for action-failure context where the user is actively waiting for feedback. **NOT `aria-live="polite"`** (the spec rev 0 had `role="alert" aria-live="polite"` which is contradictory — pick one, `role="alert"` wins per the user-action-context).

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
   - **Two listeners** consume that custom event via **Variant A** of the HX-Trigger idiom (post-swap, addEventListener-native — see AC8 for variant taxonomy):
     - `static/js/modal.js` — for the migrated trash modal mounted in `#modal-slot`. Adds `document.body.addEventListener("modal-close", ...)` that calls `dialog.close()` + clears the slot (or equivalent — re-use the existing close path).
     - `static/js/inline-form.js` — for the 2 ref-data modals that stay in `#admin-modal-slot`. Adds the same listener at the same scope. Behavior: `slot.innerHTML = ""`, mirroring the existing `admin-modal-close` action handler.
   - **Broadcast semantics, intentional.** The event fires on `document.body`; both listeners receive it. Each acts on its own slot. `slot.innerHTML = ""` is idempotent when the slot is empty, so the listener whose slot is empty is a no-op. This matches the user-intent: when a destructive action succeeds, close anything modal-shaped that's still open. If a future surface needs scoped close (close only the modal that originated the request), HTMX supports a JSON payload (`HX-Trigger: {"modal-close":{"slot":"modal-slot"}}`) — extension possible without breaking the current contract.
   - **Future-proof:** any new admin handler that needs to close a modal on success uses the same trigger; the two listeners cover both slots.

3. **AC3 — `showModal()` lifted to `modal.js` AND `inline-form.js` (closes #65 across ALL 8 modals — 5 Pattern A + 3 Pattern B).**
   - **`modal.js`**: when its existing mutation/swap observer detects a new `dialog[open]` lands in `#modal-slot`, it calls `dialog.showModal()` on it. The declarative `<dialog open>` attribute stays in the templates (it's the scanner-guard selector contract per story 7-5) — `showModal()` is idempotent against an already-open dialog OR we drop the `open` attribute server-side (Task 1 picks one — re-record the call in Dev Agent Record).
   - **`inline-form.js`**: symmetric addition watching `#admin-modal-slot`. The existing module already handles Escape + Cancel for that slot, so it's the natural owner of the `showModal()` call too.
   - **Cross-cutting impact:** this fixes #65 not only for the migrated trash modal but for ALL 5 already-shipped UX-DR8 modals (`borrower_delete_modal.html`, `contributor_delete_modal.html`, `series_delete_modal.html`, `return_loan_modal.html`, `admin_user_deactivate_modal.html`) AND the 2 ref-data modals. **Regression risk is highest here** — verify under code-review that the existing modal flows (delete-borrower, return-loan, etc.) still work end-to-end with the proper top-layer + native Escape + native backdrop behavior. Their custom JS Escape + backdrop-click handlers in `modal.js` may now be redundant or even fight the native ones — Task 1 must reconcile.

4. **AC4 — `static/js/modal.js` surfaces Confirm-error feedback INSIDE the open Pattern A modal (closes #134 for Pattern A only).**

   Four coordinated changes (client tag + server suppression + client inject + macro region):

   **4.a — `modal.js` tags every Confirm request with `X-Modal-Confirm: true`.**
   - Add an `htmx:configRequest` listener that, when the originating element matches the existing `isConfirm` shape (FORM tag inside `state.dialog`, `[data-modal-confirm]` attr, or `closest("[data-modal-confirm]")`), sets `evt.detail.headers["X-Modal-Confirm"] = "true"`.
   - Plain non-modal forms (ref-data list-level deletes, page-level forms) get no header — no change for them.

   **4.b — `AppError::Conflict::IntoResponse` (`src/error/mod.rs:140-154`) suppresses `HX-Retarget` / `HX-Reswap` when `X-Modal-Confirm: true` is present.**
   - The `IntoResponse` impl doesn't take Request — see Dev Notes for the implementation path (an extractor that stashes the header into a thread-local OR a response middleware that strips the headers when X-Modal-Confirm was on the way in OR carry the flag through the AppError variant's structure). **Task 1 picks the cleanest plumbing path — non-trivial decision, record in Dev Agent Record.**
   - When `X-Modal-Confirm` is set on the request: response is 409 with HTML body but **NO** `HX-Retarget`, **NO** `HX-Reswap`. HTMX's `responseHandling` for 4xx is still `swap:false` by default, so the body would normally be dropped — but step 4.c catches it.
   - When the header is absent (non-modal context): existing retarget-to-`#feedback-list` behavior is preserved verbatim — story 8-4 P18 contract intact.

   **4.c — `modal.js`'s existing `htmx:afterRequest` listener injects the body into `data-modal-error`.**
   - On `detail.failed === true || detail.successful === false` for a Confirm-originating request:
     - **Race guard:** if `state` is null or `state.dialog` is no longer in the DOM (user closed the modal between request and response), early-return.
     - **Retarget guard:** if `evt.detail.xhr.getResponseHeader("HX-Retarget")` is non-empty, HTMX has already swapped the body elsewhere (defensive: 4.b should prevent this for modal Confirms, but if a future handler ships an explicit retarget the guard avoids double-display).
     - Read `evt.detail.xhr.responseText` (raw HTML string).
     - Find `state.dialog.querySelector("[data-modal-error]")`. Set `innerHTML = responseText`. Remove the `hidden` class.
     - Modal stays open. Focus stays where it was (the existing focus-trap lifecycle is undisturbed).
   - **Clear on retry:** add an `htmx:beforeRequest` listener for the SAME Confirm-button detection. When fired, set `data-modal-error.innerHTML = ""` and re-add the `hidden` class. A retry starts with a clean slate, no stale errors.

   **4.d — Macro `templates/components/modal.html` adds the error region.**
   - Insert `<div data-modal-error role="alert" class="hidden ..."></div>` immediately after the modal body content.
   - **A11y:** `role="alert"` alone — per WAI-ARIA spec this implies `aria-live="assertive"`. NO explicit `aria-live` attribute (the rev 0 contradiction `role="alert" aria-live="polite"` is resolved in favor of `role="alert"` for the action-failure context — user just clicked, is actively waiting for feedback, assertive announce is appropriate).
   - Tailwind sizing + variant-tinted colors: amber (variant `warning`/`remove`) / red (variant `delete`/`delete-forever`). Match the existing `feedback_html()` palette for visual consistency with `#feedback-list`.
   - The `hidden` class is the CSP-clean conditional-display mechanism (Tailwind `[data-modal-error]:empty { display: none }` doesn't compose cleanly per Dev Notes).

   **4.e — Handler contract addendum: uniformize `AppError::IntoResponse` (closes a latent coverage gap that affects the whole app).**

   A point-5 audit of the rev 3 spec revealed that the FeedbackEntry-HTML-on-error contract only holds today for **two** of the six `AppError` variants:
   - ✅ `Forbidden` (`error/mod.rs:113-133`) — emits HTML + `HX-Retarget: #feedback-container` — but **`#feedback-container` doesn't exist anywhere in templates** (latent dead-retarget bug; body still dropped by HTMX).
   - ✅ `Conflict` (`error/mod.rs:140-154`) — emits HTML + `HX-Retarget: #feedback-list` — works (target exists on admin.html / catalog.html).
   - ❌ `NotFound`, `BadRequest`, `Internal`, `Database` — fall through to `(status, client_message).into_response()` at line 181 — **plain-text bodies**, dropped by HTMX's default `responseHandling` for 4xx/5xx. Used massively in services (`BorrowerService::delete_borrower` has-loans, `SeriesService::delete_series` has-assignments, etc.). Every handler that propagates these via `?` produces a silent failure on the wire.

   **Universal fix in `src/error/mod.rs::IntoResponse`** (user-approved Option B 2026-05-15):
   - Wrap `NotFound`, `BadRequest`, `Internal`, `Database` final-arm responses in `feedback_html_pub("error", &client_message, "")` HTML + `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` + `Content-Type: text/html; charset=utf-8`. ~10 LOC change, single point of truth.
   - **Fix the latent dead-retarget bug** in `Forbidden` by switching its target from `#feedback-container` to `#feedback-list` (the only feedback region that actually exists across app pages). Document the switch in the commit message — it's a behavior change (Forbidden errors become VISIBLE for the first time) but consistent with the spec's overall direction.
   - Keep the variant-specific status codes unchanged (400, 404, 409, 403, 500).
   - The middleware from AC4.b (`ModalConfirmRetargetGuard`) strips `HX-Retarget` / `HX-Reswap` for modal-Confirm requests across ALL these variants, so modal.js's inject path (AC4.c) gets a clean HTML body to drop into `data-modal-error` regardless of which `AppError` fired.

   **Test impact** (audited in Task 1): existing Rust tests (`tests/admin_system_integration.rs:196,257`, `tests/setup_wizard.rs:239`, the 4 `*_delete_modal.rs` tests at L218-321) assert **only status codes**, NOT body content, for these variants. Universal fix is safe — no test updates needed beyond the new positive assertions in AC6.

   **Side-effect bonus:** `AppError::Forbidden` / `BadRequest` / `NotFound` / etc. errors that today vanish silently on HTMX 4xx will now surface as visible FeedbackEntry banners in `#feedback-list` for every HTMX-driven page. This benefits the whole production app, not just the modal lifecycle. Story 8-4 P18's original intent (no silent admin failures) is generalized.

   **Scope note:** AC4 fixes #134 for Pattern A modals (5 already-shipped + the migrated trash). The 2 ref-data modals stay on Pattern B and inherit their existing UX:
   - On `AppError::Conflict` (in-use guard) → no X-Modal-Confirm header → 4.b's suppression doesn't trigger → retarget-to-`#feedback-list` is preserved → feedback partially visible behind backdrop (pre-existing, unchanged).
   - On non-Conflict 4xx — **and this is new in rev 5** — the AC4.e universal `AppError::IntoResponse` fix means these errors also retarget to `#feedback-list` now (instead of vanishing silently). Behind the backdrop, partially visible — same imperfect UX as Conflict has today. Better than the pre-rev-5 silent failure.
   - If a real user finds the behind-backdrop visibility insufficient, file a follow-up issue to extend `inline-form.js` symmetrically with an `X-Admin-Modal-Confirm` header + admin-modal data-modal-error region. Not in scope here.

5. **AC5 — `src/templates_audit.rs::hx_confirm_matches_allowlist` invariant still passes.** The allowlist is currently empty (per CLAUDE.md story 7-5 note: *"the allowlist is empty post Epic 9 — any new `hx-confirm=` is BLOCKED outright by the audit"*). This polish iteration introduces no new `hx-confirm=` attributes — it only migrates the trash modal to the UX-DR8 macro and patches JS modules. The audit MUST still report 0 occurrences.

6. **AC6 — Unit tests.**
   - `src/routes/admin.rs` — extend the existing trash-modal unit tests so the rendered HTML of the modal-fetch endpoint exercises the macro path (look for `data-modal-trigger`, `data-modal-default-focus`, the new `data-modal-error` div). Mirror the existing `admin_trash_modal_renders_with_csrf_token` test shape.
   - `src/routes/admin_reference_data.rs` — NEW tests `<ref_table>_delete_success_emits_hx_trigger_modal_close` for at least 2 of the 4 ref-data tables (parameterize if the pattern repeats cleanly). Same for loanable-warning confirm.
   - `src/routes/admin.rs` — NEW test `permanent_delete_success_emits_hx_trigger_modal_close`.
   - Migration target test count: +5 to +8 unit tests, focused on the HX-Trigger emission and the rendered macro shape.

7. **AC7 — E2E tests.**
   - NEW `tests/e2e/specs/journeys/admin-modal-lifecycle.spec.ts` covering:
     - **Smoke trash:** admin clicks Delete permanently → modal opens (assert `dialog[open]` count == 1 in `#modal-slot`) → Cancel → modal closed (assert count == 0 after a `waitFor`) → row still in trash. **Note:** the existing `tests/e2e/specs/journeys/admin-permanent-delete.spec.ts:48` clicks Cancel but does NOT assert post-click closure — so the test currently passes despite #61 being broken (Cancel sends a 403 DELETE silently). This new AC7 assertion locks the fixed behavior so #61 cannot regress unnoticed again.
     - **Success path trash:** admin clicks Delete permanently → modal opens → types name in the confirm input → Confirm → modal closes (via `HX-Trigger: modal-close`), panel updates, FeedbackEntry shows in `#feedback-list`.
     - **Error path trash (#134):** admin clicks Delete permanently on an entity whose version is stale (seed by bumping version in a parallel `page.request` call) → Confirm → modal STAYS open in `#modal-slot`, `data-modal-error` populated with the error FeedbackEntry, user can Cancel cleanly.
     - **Rapid-double-click guard (#67) trash:** click Delete permanently twice rapidly via two sequential clicks within 100ms → assert `dialog[open]` count is always 1 (`innerHTML` swap semantic guarantees this).
     - **Smoke ref-data:** admin clicks Delete on a genre/role/etc. → modal opens (assert `dialog[open]` in `#admin-modal-slot`, with `showModal()` proven by `dialog.matches(":modal")` per the spec invariant) → Cancel → closed.
     - **HX-Trigger ref-data:** ref-data delete success → modal in `#admin-modal-slot` is removed (via the new `inline-form.js` listener).
   - Reuse `tests/e2e/helpers/loans.ts` patterns (CSRF token from meta tag, `page.request.post` for parallel seed, etc.).
   - Per Foundation Rule #13 §4 (added by Epic 10 retro): the spec MUST be validated via `CI=true npx playwright test --workers=2` before push, not just the default-worker run.

8. **AC8 — Documentation refresh.**
   - `CLAUDE.md` "Key Patterns" — add a paragraph under the existing Modal section (story 7-5 / 9-10) documenting:
     - **HX-Trigger idiom — two legitimate variants** (a new clarification the project hasn't had so far):
       - **Variant A — post-swap effect** (`document.body.addEventListener("event-name", handler)`). HTMX dispatches a DOM custom event keyed on the header value AFTER the swap completes. Use when the side-effect doesn't influence the swap itself — close a modal, refresh a widget, toast a message, etc. **New canonical example:** `HX-Trigger: modal-close` consumed by `modal.js` + `inline-form.js` (this polish iteration's AC2).
       - **Variant B — pre-swap decision** (`document.body.addEventListener("htmx:beforeSwap", evt => parseHeader(evt.detail.xhr.getResponseHeader("HX-Trigger")))`). Listener attaches to the synchronous HTMX swap-decision event and parses the header itself. Use when the side-effect MUST influence the swap (force-swap on 403, retarget, suppress isError). **Existing canonical example:** `HX-Trigger: csrf-rejected` consumed by `csrf.js` to flip `evt.detail.shouldSwap = true` on a 403 (story 8-2). Variant B is also robust to comma-separated lists (`csrf-rejected, session-warn`) — a future composability concern Variant A doesn't have.
     - The error-feedback-INSIDE-modal UX decision and the `data-modal-error` region contract (Pattern A only). Coordinates the `X-Modal-Confirm: true` request-header set by `modal.js` on every Confirm AND the `AppError::Conflict::IntoResponse` server-side suppression of `HX-Retarget`/`HX-Reswap` when the header is present. Ref-data forms (no header) keep the story 8-4 P18 retarget-to-`#feedback-list` contract.
     - The `X-Modal-Confirm: true` request header pattern itself, as a new project-wide idiom for context-aware error routing — server-side error responses adapt their target based on whether the request came from a modal Confirm or from a page-level form. Future modal-like surfaces can opt-in by setting the same header.
     - The **uniformized `AppError::IntoResponse` contract** (AC4.e): every variant now emits FeedbackEntry HTML + HX-Retarget `#feedback-list`. New handlers no longer need to manually wrap errors in `feedback_html_pub()` — propagating any `AppError` via `?` is enough. The latent `#feedback-container` dead-retarget for `Forbidden` is fixed in the same change.
     - The slot-ownership rule: **`#modal-slot` for Pattern A (UX-DR8 macro)**, **`#admin-modal-slot` for Pattern B (ref-data legacy)**. Both slots now call `showModal()` on `dialog[open]` arrival.
     - The `templates_audit.rs::hx_confirm_matches_allowlist` invariant remains empty.
   - **No new Foundation Rule** — this is a pattern extension, not a new discipline.

## Dev Notes (planning hints, NOT prescriptive)

- **Task 1 starts with reading**: `static/js/modal.js` end-to-end (197 LOC), `static/js/inline-form.js` (focus L145-199 for the existing admin-modal-close lifecycle + Escape handler), the trash modal template + handler + trigger button, and locate the type-the-name JS module (search for `data-confirm-name` and `data-confirm-btn` in `static/js/`). Don't write code before that pass — the type-to-confirm wiring (AC1 sub-bullet) and the `showModal()` reconciliation (AC3 — does it conflict with `modal.js`'s manual Escape + backdrop handlers?) are the two riskiest design decisions.
- **AC3 reconciliation question**: `modal.js` currently has its own Escape + backdrop-click + Tab-cycling handlers. `showModal()` provides native Escape + native backdrop close. Some of those manual handlers may become redundant or conflict (e.g., native Escape may close before the JS Tab-trap can react). Plan a Test 1 pass to walk the existing UX-DR8 modal specs (`borrower-delete-modal.spec.ts`, `contributor-delete-modal.spec.ts`, `series-delete-modal.spec.ts`, `return-loan-modal.spec.ts`, `admin_user_deactivate-modal.spec.ts`) and identify any behavior the JS provides that native `showModal()` doesn't (e.g., outside-click on `<dialog>` itself is NOT a standard native close — backdrop click via `event.target === dialog` is custom code that must stay).
- **The `HX-Trigger: modal-close` event listener** is best added directly to `modal.js` (`#modal-slot`) and `inline-form.js` (`#admin-modal-slot`) — same module that already owns each slot's lifecycle. ~10 LOC addition each.
- **The error-region (AC4)** needs a CSS-only conditional-display idiom — `[data-modal-error]:empty { display: none; }` won't work because of how Tailwind's hidden modifier composes; the simplest path is server-side `class="hidden"` initially + `modal.js` removes `hidden` on inject. Check `.feedback-entry` patterns elsewhere for prior art.
- **AC4.b — `AppError::Conflict::IntoResponse` reading the request header** is the trickiest plumbing piece. Three candidate approaches, ordered by cleanliness:
  - **(a)** A response middleware (`tower::Layer`) that runs AFTER the handler. It inspects the request headers (captured pre-handler via a shared state or a `task_local`) and, if `X-Modal-Confirm` was on the way in AND the response carries `HX-Retarget: #feedback-list`, strips those HTMX headers. Pure orthogonal — `AppError::Conflict::IntoResponse` stays untouched.
  - **(b)** A request extractor that stashes the header into the Axum request extensions, plus an `AppError::Conflict` variant change to `Conflict { msg: String, from_modal: bool }`. Every site that constructs the variant has to pass the flag.
  - **(c)** Carry a `tokio::task_local` flag set by an extractor and read by `IntoResponse`. Less invasive than (b) but uses a thread-local — easy to mis-use.
  - Recommend (a) — it's the lowest-coupling. Lives next to the existing `csp.rs` / `csrf.rs` middleware layer order documented in CLAUDE.md (`Logging → Auth → [Handler] → PendingUpdates → CSP`). Insert a new `ModalConfirmRetargetGuard` layer that reads `X-Modal-Confirm` on the way in and rewrites HX-* headers on the way out. Task 1 confirms after reading `src/middleware/`.
- **Trash modal call site is short** — once the type-to-confirm wiring is solved, the migrated fragment should be ~10 LOC (`{% import %}` + `{% call modal::modal(...) %}` + the type-to-confirm input + the hidden CSRF/version fields).
- **CSRF token plumbing**: the legacy trash fragment already includes `<input name="_csrf_token" value="{{ csrf_token|e }}">` — verify after migration that the macro emits this in the form. Story 8-2's `templates_audit.rs::forms_include_csrf_token` invariant must still pass.

## Out of scope (deliberately deferred)

- The 5 already-migrated UX-DR8 modals (`borrower_delete_modal.html`, `contributor_delete_modal.html`, `series_delete_modal.html`, `return_loan_modal.html`, `admin_user_deactivate_modal.html`) keep their existing templates unchanged. They inherit the AC3 `showModal()` fix and the AC4 error-region fix via `modal.js` without per-template edits.
- The 2 ref-data legacy modals (`admin_ref_delete_modal.html`, `admin_ref_loanable_warning_modal.html`) STAY on Pattern B in `#admin-modal-slot`. They get AC2 (`HX-Trigger: modal-close`) and AC3 (`showModal()` via the symmetric `inline-form.js` patch). They do NOT get AC4 (error feedback inside modal) — their errors continue to land in `#feedback-list` via the AC4.e universal retarget, partially visible behind the modal's translucent backdrop. If a real user hits a frozen-open ref-data modal in production with this UX still being insufficient, file a follow-up issue and consider extending `inline-form.js` symmetrically with an `X-Admin-Modal-Confirm` header + per-modal `data-modal-error` region.
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

This iteration is the **first** to reinstate the code-review-as-default discipline per Epic 10 retro action A1. Production-context cost of a regression is real — mybibli is live at v1.1.2 and every modal Confirm path is on a critical workflow.

### When + how

1. Spec PR (this one, #199) merges first after review feedback is incorporated.
2. Implementation branch opens: `polish-1/modal-lifecycle-hardening` from main.
3. Implementer completes ACs 1–8, runs the full Test plan above locally (incl. the `CI=true --workers=2` truth-gate from Foundation Rule #13 §4).
4. **Before marking the implementation PR ready**: invoke `/bmad-code-review` (3-layer adversarial: Blind Hunter, Edge Case Hunter, Acceptance Auditor — same shape Epic 9 used on stories 9-17 → 9-19 where it caught Medium findings the dev had missed).
5. Triage findings per the severity table below.
6. On any blocking finding (Critical or High, plus any Medium that is action-relevant for this PR): fix in the same branch, re-run `/bmad-code-review` from scratch. Foundation Rule #6: story is clean **only** when a full pass surfaces 0 Medium+ findings.
7. After 0-Medium+ pass: PR ready → CI green → squash-merge.

### Severity triage

| Severity | Disposition | Tag in PR |
|---|---|---|
| **Critical** | Absolute merge blocker. Production correctness, data integrity, security boundary. Fix before any merge. | `severity:critical` |
| **High** | Merge blocker for this PR. User-visible regression, accessibility regression, code-quality red flag. Fix in this branch. | `severity:high` |
| **Medium** | Merge blocker per Foundation Rule #6 — but if action-irrelevant to this PR's narrative (e.g. a pre-existing issue surfaced by the review but not introduced here), can be split into a GH issue with `type:code-review-finding` and deferred. Document the split in the PR body. | `severity:medium` |
| **Low** | Not a merge blocker. File as GH issue with `type:code-review-finding, severity:low, status:deferred` and move on. | `severity:low` |

### Probes the reviewer must walk explicitly

Grouped by AC for orientation. **Each bullet is a concrete probe**, not a vague "check carefully" — the reviewer should leave a `[Verified]` or `[Finding]` line on each.

#### Probes for AC1 (trash modal migration)

- **Slot-move ripple effect.** `templates/fragments/admin_trash_panel.html:76`'s `hx-target="body"` flips to `#modal-slot`. Grep the repo for `hx-target="body"` outside this file and verify no other code path relied on the trash modal landing at body-end as a sibling. Also verify scanner-guard's global selector still finds the dialog post-slot-move (already analysed in spec Point 2, but re-verify).
- **Type-to-confirm UX preservation.** The migrated trash modal MUST still disable the Confirm button until the user types the exact item name. Whatever path AC1 picks (macro extension via new params vs call-site wiring), the E2E test in AC7 must lock disabled-until-match. Also verify the JS module currently providing `data-confirm-name` / `data-confirm-btn` behavior (Task 1 finding) is wired correctly to the migrated DOM.
- **`modal_close_target` field removal.** Confirm `pub modal_close_target: String,` on `admin.rs:348` and its construction site at `:882` (CSS-selector-as-URL — issue #61's root cause) are both deleted. Grep for any other usage to be safe.

#### Probes for AC2 (`HX-Trigger: modal-close` broadcast)

- **Event-name collision.** Is `modal-close` already used anywhere as an HX-Trigger value, a DOM event listener, an htmx hyperscript trigger, or a JS `dispatchEvent`? Grep `modal-close` across the repo before merge.
- **Dual-listener safety re-verify.** modal.js (for `#modal-slot`) and inline-form.js (for `#admin-modal-slot`) both listen for `modal-close`. Each acts on its own slot — `slot.innerHTML = ""` idempotent. Confirm both handlers early-return cleanly if their slot is empty.
- **Future-extension scope.** The spec mentions JSON payload (`HX-Trigger: {"modal-close":{"slot":"modal-slot"}}`) as a future scoping mechanism. Confirm the rev-5 broadcast-by-default implementation doesn't paint us into a corner if scoped close becomes needed.

#### Probes for AC3 (`showModal()` lift)

- **5 already-shipped UX-DR8 modals — regression risk.** Walk each existing modal E2E spec (`borrower-delete-modal.spec.ts`, `contributor-delete-modal.spec.ts`, `series-delete-modal.spec.ts`, `return-loan-modal.spec.ts`, `admin-user-deactivate-modal.spec.ts` — verify exact paths in Task 1) and flag every assertion that relies on a non-native behavior. Native top-layer + Escape + backdrop semantics may collide with modal.js's existing manual Tab-cycling, focus-trap, and Cancel-button delegation. Decision recorded in Dev Agent Record about which manual handlers stay vs are removed.
- **2 ref-data legacy modals — regression risk.** The `admin-modal-close-revert-row` UX (Cancel on loanable-warning triggers a row-revert HTMX GET) must still work after the symmetric `showModal()` in inline-form.js. Walk story 8-4 P14's contract explicitly.
- **`<dialog open>` attribute reconciliation.** Task 1's decision (idempotent `showModal()` against an already-`open` dialog vs. drop the `open` attribute server-side) is recorded in Dev Agent Record. Reviewer confirms the chosen path doesn't break the scanner-guard MODAL_SELECTOR contract (`dialog[open], [aria-modal="true"]` — story 7-5).

#### Probes for AC4 (#134 error feedback inside modal)

- **AC4.b — middleware vs CSRF rejection interaction (CRITICAL probe).** The CSRF middleware (`src/middleware/csrf.rs:352`) emits `HX-Trigger: csrf-rejected` + `HX-Retarget: #feedback-list` on a token-drift 403. If a request comes from a modal Confirm (`X-Modal-Confirm: true`) AND hits CSRF rejection, the new `ModalConfirmRetargetGuard` middleware would strip the retarget — leaving the user with a frozen modal AND no CSRF feedback anywhere. **The middleware MUST whitelist `HX-Trigger: csrf-rejected` from stripping** (or equivalently, only strip when no `HX-Trigger: csrf-rejected` is present in the response). Verify this in code-review and in a new E2E test: open a modal, wait for session timeout / induce CSRF drift, click Confirm, assert the modal closes AND a session-expired FeedbackEntry appears in `#feedback-list`.
- **AC4.b — request-scoped header detection.** The middleware reads `X-Modal-Confirm` from the request and rewrites response headers based on that. Verify the implementation is request-scoped — no thread-local pitfalls (Tokio task-local would be acceptable; raw `thread_local!` is not because Axum handlers run on a worker pool).
- **AC4.b — layer order.** The new layer sits in `src/middleware/`. Per CLAUDE.md the existing chain is `Logging → Auth → [Handler] → PendingUpdates → CSP` (story 7-4). Confirm the new layer's position in `src/routes/mod.rs::build_router` doesn't break that order or any inter-layer header dependency.
- **AC4.c — error-region injection safety.** Response HTML is injected as `innerHTML` into `data-modal-error`. Verify that `feedback_html_pub()` (catalog.rs:33) HTML-escapes every dynamic input. Any code path that bypasses `feedback_html_pub` and returns raw user-supplied text would be an XSS vector. The AppError variants (now all wrapped per AC4.e) carry user-derived `msg` strings — confirm they're all escaped server-side before reaching the inject path.
- **AC4.c — race guard correctness.** If `state.dialog` is gone when the failed response arrives (user clicked Cancel), modal.js must early-return cleanly without throwing. Verify the guard order: state-null check FIRST, then dialog-still-in-DOM check.
- **AC4.c — clear-on-retry semantics.** Verify the `htmx:beforeRequest` clearing listener doesn't fire on NON-Confirm requests within the modal (e.g., an autocomplete dropdown inside the modal body would fire htmx:beforeRequest too — the listener must use the same `isConfirm` discrimination as the inject listener).
- **AC4.d — ARIA `role="alert"` only.** Verify no axe-core rule trips on the absence of explicit `aria-live`. The WCAG 2.2 AA gate from story 10-5 should accept `role="alert"` alone (implies `aria-live="assertive"`), but run the new `admin-modal-lifecycle.spec.ts` AC7 cases through axe to confirm. Test on Firefox AND Chromium — historically the two read alerts differently.
- **AC4.e — universal IntoResponse fix.**
  - The 4 variant arms (BadRequest, NotFound, Internal, Database) all get the same HTML+retarget shape as Conflict, not just one by accident.
  - The `#feedback-container` → `#feedback-list` switch in Forbidden doesn't break a use case. Grep `#feedback-container` outside `error/mod.rs` to confirm zero references (already verified pre-spec — re-verify in case a parallel branch added one).
  - Existing E2E Playwright specs that DO assert on `#feedback-list` content for OTHER reasons aren't accidentally broken by new error feedback landing there (e.g. a test that asserts `#feedback-list` is empty after a navigation — would now see a stale Forbidden FeedbackEntry from a 403 elsewhere).
  - **Defense-in-depth check (sensitive info leakage).** The new universal retarget makes `AppError::Forbidden` / `Internal` / `Database` errors visible for the first time on the wire. Verify no error path leaked sensitive info via plain text that we now expose as user-visible HTML — focus on `Database` (whose `err.to_string()` could include SQL fragments or column names) and `Internal` (whose `msg.clone()` could include path or session details). Spec says client-message is sanitized to "An internal error occurred" for both Internal and Database — confirm post-implementation that this stays true.

#### Probes for AC7 (E2E)

- **E2E race conditions on the rapid-double-click test (#67).** Under 14-worker default-local Playwright the 100ms threshold may flake (Epic 10 §4.1 pattern). Use `clickCount: 2` or assert on `dialog[open]` count == 1 throughout the click sequence, NOT just at the end.
- **CI-shape verification.** Foundation Rule #13 §4 mandates `CI=true npx playwright test --workers=2` for shared-DB-state specs. The new modal-lifecycle spec touches shared DB state (trash entries, ref-data rows). Implementer MUST have run this; reviewer confirms by reading the PR description / Dev Agent Record.

### Manual smoke checklist (Task 1 author + reviewer)

The implementer runs this checklist in a real browser before marking PR ready. Reviewer also walks it during review (manual cross-check is cheap and catches what specs don't).

```
□ /admin?tab=trash, click Delete permanently → modal opens
  □ backdrop appears (native top-layer post-showModal)
  □ Escape closes
  □ Backdrop click closes
  □ Cancel closes
  □ Type wrong name → Confirm stays disabled
  □ Type correct name → Confirm enables
  □ Confirm with success → modal closes (HX-Trigger), panel updates,
    FeedbackEntry in #feedback-list
  □ Trigger 409 conflict (open second tab, race) → error region
    visible INSIDE modal with role=alert, modal stays open
  □ Retry after error → error region clears at htmx:beforeRequest
□ /admin?tab=reference_data, delete a genre → ref-data modal opens
  □ Escape closes (existing inline-form.js handler)
  □ Cancel closes (existing data-action handler)
  □ Confirm with success → modal closes (HX-Trigger: modal-close
    via new inline-form.js listener)
  □ Confirm with in-use conflict → error in #feedback-list, modal
    stays open (intentional — see AC4 scope note)
□ /borrowers, delete a borrower with active loans → BadRequest
  → user sees FeedbackEntry in #feedback-list (AC4.e bonus)
□ Force CSRF drift (delete session row via DB), click Confirm in
  any modal → modal closes, "Session expired" FeedbackEntry visible
  (validates AC4.b CSRF whitelist probe)
□ axe-core via DevTools on each modal in open state → 0 WCAG 2.2
  AA violations
□ Mobile viewport (375px) → trash modal still usable
```

### Rollback if blocked

If `/bmad-code-review` surfaces blockers that can't be fixed within reasonable scope (e.g. AC4.b middleware design proves architecturally incompatible with story 8-2's CSRF flow), the spec authorizes splitting:

- **Partial merge:** ACs 1, 2, 3, 6, 7, 8 (modal migration + showModal + tests + docs) ship as polish-1a. AC4 (error feedback inside modal) defers to polish-1b with its own spec PR after the design issue is resolved. AC4.e universal IntoResponse fix CAN ship with polish-1a (no dependency on AC4.b middleware) — it's the polish-1a's bonus.
- **Defer entirely:** if even ACs 1–3 surface deep regression risks, polish-1 defers to a re-spec'd polish-2 that takes the lessons. Polish iterations are advisory, not contractual — the framework that introduced them (Epic 10 retro A5) explicitly allows.
- **Block merge:** in all rollback cases, file a `type:change-request` GH issue tagged `polish-1-blocked-by-review` linking the spec PR + the review findings, and merge nothing until the user explicitly approves the new path.
