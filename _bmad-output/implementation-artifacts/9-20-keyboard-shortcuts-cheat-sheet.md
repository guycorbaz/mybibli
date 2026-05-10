# Story 9.20: Keyboard shortcuts complete + cheat-sheet dialog

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a keyboard-driven librarian,
I want consistent keyboard shortcuts during the scan workflow plus a discoverable "?" cheat sheet,
so that I can move at speed without reaching for the mouse.

## ⚠️ Existing-code reality check

Status of `main` as of 2026-05-10 (post 9-19 close):

- **Existing keyboard shortcuts in `static/js/mybibli.js:5-43`** (`initKeyboardShortcuts`):
  - `Ctrl+K` / `Cmd+K` → navigate to `/catalog` (gated to librarian/admin via `document.body.dataset.userRole`)
  - `Ctrl+Shift+B` → navigate to `/borrowers` (same gate)
  - `Ctrl+N` / `Cmd+N` → open title creation form (only on `/catalog`, librarian/admin)
  - **9-20 KEEPS THESE AS-IS.** They're orthogonal to the new shortcuts (modifier-key combos vs single-key `?` + `g`-chord). The cheat-sheet dialog will list them all (existing + new) so users discover both.

- **Multi-listener `Escape` architecture already in place** — verified:
  - `modal.js` listens on document and closes UX-DR8 modals when one is open
  - `nav.js` listens on document, closes the mobile hamburger panel (gated via `isModalOpen()` to defer to modal)
  - `tooltip.js` listens on document, closes tooltips (gated via `isModalOpen()` to defer to modal)
  - `mybibli.js` has 2 narrow Escape handlers: `initFormEscapeHandler` (closes the inline title-creation form on `/catalog`), `initTitleEditFormEscape` (clicks the Cancel button inside the inline title-edit form)
  - **9-20 ADDS the cheat-sheet `<dialog>` which uses NATIVE `<dialog>.showModal()` — Escape-to-close is handled by the browser (the `cancel` event fires naturally; we just add a listener on the `cancel` event to close).** No new Escape handler at the document level — `<dialog>` is its own surface.

- **Role-gating mechanism — `document.body.dataset.userRole`**: existing JS modules read `document.body.dataset.userRole` (set by `base.html:18` to `{{ role }}` — values `"anonymous"`, `"librarian"`, `"admin"`). 9-20's cheat-sheet uses the same dataset to FILTER which shortcuts to display. The g-chord navigation shortcuts also gate per-shortcut (e.g., `g-l` → /loans only fires for librarian/admin).

- **`<dialog>` native modal infrastructure**: `dialog.showModal()` natively handles:
  - Top-layer rendering (always above other content, no z-index gymnastics)
  - Native focus trap (Tab cycles within the dialog only)
  - Native `Escape` close (the `cancel` event fires)
  - Backdrop styling (via `::backdrop` pseudo-element)
  - Backdrop click does NOT close natively — we add a 1-line listener for that
  - **9-20 USES NATIVE `<dialog>.showModal()` for the cheat sheet, NOT modal.js's `#modal-slot` infra.** modal.js is for confirmation flows (story 9-10) where the dialog is HTMX-injected from a server fragment; the cheat sheet is purely client-side static markup, so native APIs are simpler and lighter. The spec's mention of "reusing Modal infrastructure" is interpreted as "use the same `<dialog>` element semantics" — which native `showModal()` provides without the manual focus-trap + Escape wiring modal.js carries.

- **Existing `current_page` template variable** is set per-page (e.g., `"catalog"`, `"loans"`, `"home"`) and exposed on `<body data-page="{{ current_page }}">`. The g-chord navigation can use this to no-op when the user is already on the destination page (small UX nicety).

- **Foundation Rule #2 waiver carry-forward**: shortcuts.js JS unit tests will be deferred (delegated to E2E + integration on rendered markup), per the same 9-16/9-17/9-19 pattern.

- **i18n key naming**: `shortcuts.cheat_sheet.<field>` for the dialog, `shortcuts.footer_link` for the discoverability footer. Mirror of the `help.<surface>.<field>` pattern from 9-19.

## Acceptance Criteria

1. **AC1 — NEW JS module `static/js/shortcuts.js`** (~150 LOC, IIFE shape, mirror of `nav.js`):
   - **`?` shortcut**: keydown listener on document. When the active key is `?` (i.e., `e.key === "?"`) AND the focus is NOT in a text input, textarea, or contenteditable element, calls `dialog.showModal()` on `#shortcuts-cheat-sheet`. CSP-clean.
   - **`g`-chord shortcuts** with 800ms timeout: when `g` is pressed (and focus is not in a text input), start a chord-window timer (`setTimeout(800)`). The next keystroke within the window:
     - `c` → `window.location = "/catalog"` (anonymous-readable; always allowed)
     - `l` → `/loans` if `body.dataset.userRole !== "anonymous"` else no-op
     - `h` → `/` (anonymous-readable; always allowed)
     - `b` → `/borrowers` if `body.dataset.userRole !== "anonymous"` else no-op
     - `a` → `/admin` if `body.dataset.userRole === "admin"` else no-op
   - Any OTHER keystroke during the chord window cancels it. The timer expiring also cancels.
   - **Input-skip gate**: the helper `isTextInput(activeElement)` returns `true` for `<input>` (text/search/email/tel/password/number), `<textarea>`, or `[contenteditable=true]`. Both `?` and `g`-chord skip when this returns true.
   - **Backdrop-click close** for the cheat-sheet dialog: a single listener on the dialog itself — when the click target equals the dialog (the backdrop area), call `dialog.close()`.
   - **`cancel` event listener** on the dialog: native Escape fires `cancel`; our listener just lets the default close happen. Optional: `e.preventDefault()` then `dialog.close()` for explicit control. Keep default behavior (less code).
   - Idempotent via `dataset.wired` guard on the dialog itself.
   - CSP-clean (no `eval`, no inline handlers).

2. **AC2 — Register `shortcuts.js` in `templates/layouts/base.html`** AFTER `tooltip.js` and BEFORE deferred `mybibli.js`. Same script-ordering family as the other UI-surface modules.

3. **AC3 — NEW cheat-sheet `<dialog>` element in `templates/layouts/base.html`** (sibling slot to `#modal-slot`, `#admin-modal-slot`, `#connection-lost-overlay`):
   - `<dialog id="shortcuts-cheat-sheet" class="...">` — closed by default (no `open` attribute).
   - Inside: a `<form method="dialog">` (so a Close button works without JS), a heading (`<h2>{{ cheat_sheet_heading }}</h2>`), and a list of shortcut groups.
   - **Role-gated rendering** done at the SERVER side (Askama if-blocks reading `role`):
     - **Anonymous**: shows only `?`, `Esc`, `g h` (home), `g c` (catalog).
     - **Librarian**: anonymous set + `Ctrl+K`, `Ctrl+Shift+B`, `Ctrl+N`, `g l` (loans), `g b` (borrowers).
     - **Admin**: librarian set + `g a` (admin).
   - Each row uses `<kbd>` elements for the keystroke representation (e.g., `<kbd>?</kbd>`, `<kbd>g</kbd> then <kbd>c</kbd>`).
   - Heading is `<h2 id="shortcuts-cheat-sheet-title">`; the dialog has `aria-labelledby="shortcuts-cheat-sheet-title"`.
   - Close button (inside `<form method="dialog">`) carries the i18n label `cheat_sheet_close_label`.

4. **AC4 — NEW footer link** in `templates/layouts/base.html` (small unobtrusive link at the page bottom):
   - `<a href="#" data-shortcuts-help-link class="text-xs text-stone-500 ...">Press ? for shortcuts</a>` (i18n key `shortcuts.footer_link`).
   - The link is purely a discoverability hint. Clicking it ALSO opens the cheat sheet (the `data-shortcuts-help-link` attribute is a delegated trigger handled by shortcuts.js's `click` listener — same code path as the `?` keystroke).
   - Renders for ALL roles including anonymous (the cheat sheet itself filters per-role; the footer link is universal).
   - Position: in a `<footer>` element after `<main>`, before the script tags.

5. **AC5 — i18n: NEW top-level `shortcuts:` block** in `locales/en.yml` + `locales/fr.yml` (~16 keys per locale):
   - `shortcuts.cheat_sheet.heading: "Keyboard shortcuts"` / `"Raccourcis clavier"`
   - `shortcuts.cheat_sheet.category_navigation: "Navigation"`
   - `shortcuts.cheat_sheet.category_catalog: "Catalog"` / `"Catalogue"`
   - `shortcuts.cheat_sheet.category_modal: "Modal"` (same in FR)
   - `shortcuts.cheat_sheet.shortcut_help: "Open this cheat sheet"` / `"Ouvrir cet aide-mémoire"`
   - `shortcuts.cheat_sheet.shortcut_escape: "Close any open dialog or menu"` / `"Fermer tout dialogue ou menu ouvert"`
   - `shortcuts.cheat_sheet.shortcut_go_home: "Go to home"` / `"Aller à l'accueil"`
   - `shortcuts.cheat_sheet.shortcut_go_catalog: "Go to catalog"` / `"Aller au catalogue"`
   - `shortcuts.cheat_sheet.shortcut_go_loans: "Go to loans"` / `"Aller aux prêts"`
   - `shortcuts.cheat_sheet.shortcut_go_borrowers: "Go to borrowers"` / `"Aller aux emprunteurs"`
   - `shortcuts.cheat_sheet.shortcut_go_admin: "Go to admin"` / `"Aller à l'administration"`
   - `shortcuts.cheat_sheet.shortcut_focus_scan: "Focus the scan field"` / `"Focuser le champ de scan"`
   - `shortcuts.cheat_sheet.shortcut_new_title: "Add a new title"` / `"Ajouter un nouveau titre"`
   - `shortcuts.cheat_sheet.then_label: "then"` / `"puis"` (used between `<kbd>g</kbd> then <kbd>c</kbd>`)
   - `shortcuts.cheat_sheet.close_label: "Close"` / `"Fermer"`
   - `shortcuts.footer_link: "Press ? for shortcuts"` / `"Appuyez sur ? pour les raccourcis"`
   - Run `cargo test all_t_keys_have_both_locales` after.
   - Run `touch src/lib.rs && cargo build`.

6. **AC6 — Page-route struct extensions**: every page-route struct that extends `base.html` already passes `role`. The cheat-sheet renders directly from `role` — no new struct field needed for the dialog content (everything lives in `base.html` + i18n keys). HOWEVER, the cheat-sheet's i18n strings need to be passed in. Two options:
   - (a) Each page struct gains 16 String fields (one per i18n key) — high churn.
   - (b) Bundle the i18n keys into a `ShortcutsCheatSheetContext` helper struct (mirror of 9-16's `ConnectionStatusContext`) with one `new(loc, role)` ctor that pre-populates all strings. **Each page struct gains ONE field `shortcuts_cheat_sheet: ShortcutsCheatSheetContext`.**
   - **DECISION (frozen): option (b).** ~25 page structs gain ONE field + ONE ctor line each (mirror of 9-16's rollout pattern). Mass mechanical edits via Python script.

7. **AC7 — `cancel` event closes the dialog (native Escape behavior)**:
   - `<dialog>.showModal()` natively fires `cancel` on Escape. Default behavior closes the dialog. shortcuts.js does NOT add a custom Escape listener at the document level — the dialog's own `cancel` event covers it.
   - This avoids adding yet another document-level keydown handler that might conflict with modal.js / nav.js / tooltip.js.

8. **AC8 — `prefers-reduced-motion` honored**:
   - `<dialog>` has `class="..."` with no `motion-safe:transition-*` classes (instant open/close in both modes). The native `<dialog>` open is instant by default; we don't add CSS transitions.

9. **AC9 — CSP compliance**:
   - `cargo test no_inline_markup_in_templates` green. No new `style=`, `<style>`, `onclick=`.
   - shortcuts.js loaded via `<script src=...>`; all listeners via `addEventListener`.

10. **AC10 — Unit tests (Rust integration)** — NEW file `tests/keyboard_shortcuts_cheat_sheet.rs` (~250 LOC, ~8 cases):
    1. `shortcuts_js_is_registered_in_base_layout` — assert `<script src="/static/js/shortcuts.js">` in rendered HTML.
    2. `cheat_sheet_dialog_renders_with_correct_id_and_aria` — GET `/login`, assert `<dialog id="shortcuts-cheat-sheet" aria-labelledby="shortcuts-cheat-sheet-title">` is in the body.
    3. `cheat_sheet_anonymous_shows_minimal_set` — GET `/login`, assert dialog contains `?`, `Esc`, `g h`, `g c` rows but NOT `Ctrl+K`, NOT `g l`, NOT `g b`, NOT `g a`.
    4. `cheat_sheet_librarian_shows_extended_set` — GET `/loans` with librarian session, assert dialog contains `Ctrl+K`, `Ctrl+Shift+B`, `Ctrl+N`, `g l`, `g b` BUT NOT `g a`.
    5. `cheat_sheet_admin_shows_full_set` — GET `/admin?tab=health` with admin session, assert dialog contains `g a`.
    6. `cheat_sheet_french_locale` — GET with `Cookie: lang=fr`, assert FR copy ("Raccourcis clavier", "puis").
    7. `footer_link_renders_with_data_attribute` — assert footer contains `<a ... data-shortcuts-help-link>Press ? for shortcuts</a>`.
    8. `footer_link_french_locale` — assert FR variant.

11. **AC11 — E2E test** — NEW spec `tests/e2e/specs/journeys/keyboard-shortcuts-cheat-sheet.spec.ts` (~200 LOC, 5 scenarios):
    1. **Anonymous opens cheat sheet via `?` + verifies minimal content** — navigate to `/`, press `?`, assert `dialog#shortcuts-cheat-sheet[open]` is visible, contains `<kbd>g</kbd>` `<kbd>c</kbd>` row but NOT the librarian-only rows.
    2. **Librarian extended cheat sheet** — login as librarian, navigate to `/loans`, press `?`, assert dialog contains `Ctrl+K`, `Ctrl+Shift+B`, `Ctrl+N`, `g l`, `g b`.
    3. **`g`-chord navigation** — login as librarian, navigate to `/`, press `g` then within 800ms press `c`, assert `await page.waitForURL(/\/catalog/)`.
    4. **Cheat sheet does NOT open when typing in search input** — navigate to `/`, focus `#search-field`, press `?`, assert dialog stays closed (the `?` is consumed by the input as text).
    5. **Escape closes the dialog (native `<dialog>` cancel)** — open the dialog via `?`, press `Escape`, assert `await expect(dialog).not.toHaveAttribute("open")`.
    - Stable selectors: `dialog#shortcuts-cheat-sheet`, `[data-shortcuts-help-link]`.
    - i18n-aware regex: `await expect(dialog).toContainText(/Keyboard shortcuts|Raccourcis clavier/i)`.
    - Flake gate: NO `waitForTimeout`. The 800ms chord window is exercised within Playwright's normal step timing (no explicit sleep needed — `page.keyboard.press('g')` followed by `page.keyboard.press('c')` finishes in tens of ms).

12. **AC12 — Foundation Rule #12 LOC discipline**:
    - `static/js/shortcuts.js`: NEW ~150 LOC.
    - `templates/layouts/base.html`: +~80 LOC (the `<dialog>` markup + footer link).
    - `src/utils.rs`: +~40 LOC for `ShortcutsCheatSheetContext` struct + ctor.
    - `locales/{en,fr}.yml`: +16 keys per locale (~25 LOC each).
    - `~25 page-route structs`: +1 field + 1 ctor line per struct (~50 LOC total).
    - `tests/keyboard_shortcuts_cheat_sheet.rs`: NEW ~250 LOC.
    - `tests/e2e/specs/journeys/keyboard-shortcuts-cheat-sheet.spec.ts`: NEW ~200 LOC.

13. **AC13 — Story-level grep audit**:
    - `grep -rn 'data-shortcuts-help-link' templates/` returns exactly 1 (in `base.html`).
    - `grep -rn 'id="shortcuts-cheat-sheet"' templates/` returns exactly 1 (in `base.html`).
    - `grep -rE 'shortcuts\.cheat_sheet|shortcuts\.footer_link' locales/` returns matching keys.

14. **AC14 — Local Testing Before Push**:
    - `SQLX_OFFLINE=true cargo check` clean
    - `cargo clippy --all-targets -- -D warnings` clean
    - `cargo test --lib` green
    - `cargo test --test keyboard_shortcuts_cheat_sheet` green (8 cases)
    - `cargo test no_inline_markup_in_templates` + `all_t_keys_have_both_locales` green
    - Full E2E green
    - Flake gate clean

15. **AC15 — Draft PR + CI gate**: Foundation Rule #15 + #18.

16. **AC16 — Foundation Rule #2 (Unit Tests) waiver for `shortcuts.js`**: same as 9-16/9-17/9-19. JS coverage delegated to E2E + Rust integration. Inherits 9-16's deferred Vitest harness ticket.

## Tasks / Subtasks

- [ ] **Task 1 — Verify reality-check assumptions (AC: all)**
  - [ ] Confirm `mybibli.js:5-43` keyboard shortcuts are present and unchanged.
  - [ ] Confirm `body.dataset.userRole` is set in `base.html:18`.
  - [ ] Confirm `<dialog>` element is supported in target browsers (Chromium / Playwright — yes since 2022; Safari since 15.4 in 2022).
  - [ ] Confirm no existing `shortcuts.*` keys in `locales/`.
  - [ ] Confirm `current_page` template variable is set on `<body data-page="{{ current_page }}">`.
  - [ ] Run baseline `cargo test --lib all_t_keys_have_both_locales no_inline_markup_in_templates` → both green.

- [ ] **Task 2 — Create `ShortcutsCheatSheetContext` helper (AC: 6)**
  - [ ] Add to `src/utils.rs` after `TooltipData`. Mirror of 9-16's `ConnectionStatusContext::new(loc)` pattern.
  - [ ] All 16 i18n strings populated at construction time.
  - [ ] `cargo build` clean.

- [ ] **Task 3 — i18n keys (AC: 5)**
  - [ ] Add `shortcuts:` block (~16 keys) to `locales/en.yml` + `locales/fr.yml`.
  - [ ] `touch src/lib.rs && cargo build`.
  - [ ] `cargo test --lib all_t_keys_have_both_locales` green.

- [ ] **Task 4 — Add cheat-sheet dialog + footer link to `base.html` (AC: 3, 4)**
  - [ ] Insert `<dialog>` markup (sibling to `#modal-slot` / `#connection-lost-overlay`).
  - [ ] Inside the `<dialog>`: heading + role-gated `<kbd>` rows + close button.
  - [ ] Add footer link in a `<footer>` element after `<main>`.
  - [ ] Use `{% if role != "anonymous" %}` and `{% if role == "admin" %}` for the role gating.
  - [ ] `cargo test no_inline_markup_in_templates` green.

- [ ] **Task 5 — Wire `ShortcutsCheatSheetContext` into all page structs (AC: 6)**
  - [ ] Map all page structs (similar to 9-16's `ConnectionStatusContext` rollout — `grep -rn 'connection_status: crate::utils' src/routes/` to find them).
  - [ ] Add ONE field per struct + ONE ctor line. Use Python script for mechanical edits.
  - [ ] Update test fixtures (volume_detail_tests.rs, home.rs:813, titles.rs:1346/1449, locations.rs:677) with hardcoded fixtures.
  - [ ] `cargo build --all-targets` clean.

- [ ] **Task 6 — Create `static/js/shortcuts.js` (AC: 1)**
  - [ ] Implement IIFE per AC1.
  - [ ] `?` shortcut, `g`-chord with 800ms timer, input-skip gate, backdrop-click close, footer-link click handler.
  - [ ] CSP-clean.
  - [ ] Idempotent via `dataset.wired`.

- [ ] **Task 7 — Register `shortcuts.js` in `base.html` (AC: 2)**
  - [ ] Add `<script src="/static/js/shortcuts.js"></script>` AFTER tooltip.js and BEFORE deferred mybibli.js.

- [ ] **Task 8 — Integration tests (AC: 10)**
  - [ ] Create `tests/keyboard_shortcuts_cheat_sheet.rs` with 8 cases.
  - [ ] Run with `SQLX_OFFLINE=true DATABASE_URL='...rust_test' cargo test --test keyboard_shortcuts_cheat_sheet`.

- [ ] **Task 9 — E2E test (AC: 11)**
  - [ ] Create `tests/e2e/specs/journeys/keyboard-shortcuts-cheat-sheet.spec.ts`.
  - [ ] 5 scenarios per AC11.
  - [ ] No `waitForTimeout`. Stable selectors.

- [ ] **Task 10 — Local gate + push + draft PR (AC: 14, 15)**
  - [ ] cargo check + clippy + tests + full E2E + flake gate + AC13 grep audit.
  - [ ] Push, open draft PR, wait CI green per Rule #18.

## Dev Notes

### Why native `<dialog>` over modal.js's infrastructure

modal.js (story 9-10) was designed for HTMX-injected destructive-confirm modals: it watches `#modal-slot` for `<dialog open>` insertions, installs manual focus-trap, Escape, backdrop-click. It's stateful and ties into the HTMX swap lifecycle.

The cheat sheet is **purely client-side static markup** — it lives in `base.html` from page render onwards, content never changes per-session. Native `<dialog>.showModal()` provides:
- Top-layer rendering (free, no z-index)
- Native focus trap (free, no manual Tab/Shift+Tab cycle)
- Native `cancel` event on Escape (free)

The only thing native doesn't do is backdrop click. We add 1 line for that. Net: ~5 LOC of dialog-specific JS in shortcuts.js vs ~50 LOC if we reused modal.js's manual infra. The Foundation-Rule-#1 DRY principle prefers the native API when it does the job.

### Why server-side role-gating for cheat-sheet content

The cheat sheet shows different shortcuts per role. We could:
- (a) Render ALL shortcuts in HTML and hide some via `data-role` + JS filtering. CSP-clean but bloats the HTML for anonymous users.
- (b) Render only the role's subset server-side via Askama `{% if role == "..." %}` blocks.

We pick (b). The per-render markup is slightly different per role but Askama caches templates so there's no compile penalty. SR users hear only the relevant subset.

### Why no document-level `Escape` handler in shortcuts.js

modal.js, nav.js, tooltip.js already handle Escape on their respective surfaces. Adding another document-level Escape handler in shortcuts.js would create yet another listener that needs an `isModalOpen()` gate (and a "is-cheat-sheet-open" gate, and so on).

The native `<dialog>.showModal()` fires `cancel` on Escape — we don't need to listen at the document level for the cheat sheet specifically. The dialog's own surface handles its own close. Cleaner architecture: each surface owns its own Escape; the document-level listeners only fire when no surface owns the keystroke.

### Why g-chord uses `setTimeout(800)` not `keyup`-driven timing

The 800ms timeout is a UX choice: it must be long enough for typists with tremor or hesitation, short enough that an accidental `g` keypress doesn't catch a follow-up keystroke 5 seconds later. `setTimeout(800)` started on `g`-keydown and cleared on any other keydown is the simplest implementation.

Alternative: keep state across multiple keystrokes via a small state machine. Overkill for 5 chord shortcuts.

### Foundation Rule #2 waiver

shortcuts.js JS unit tests deferred. Coverage:
- Integration tests (Rust): rendered cheat-sheet markup + role-aware filtering.
- E2E tests (Playwright): the actual `?` keystroke, `g`-chord navigation, input-skip behavior, Escape close.

The deferred GH issue from 9-16 ("Add JS unit-testing harness Vitest") subsumes shortcuts.js too.

### Project Structure Notes

- `static/js/shortcuts.js` — NEW.
- `src/utils.rs` — `ShortcutsCheatSheetContext` added.
- `templates/layouts/base.html` — cheat-sheet `<dialog>` + footer link + `<script>`.
- `locales/en.yml` + `fr.yml` — 16 keys each under `shortcuts:` block.
- ~25 page-route structs — one field + one ctor line each.
- `tests/keyboard_shortcuts_cheat_sheet.rs` — NEW.
- `tests/e2e/specs/journeys/keyboard-shortcuts-cheat-sheet.spec.ts` — NEW.

### References

- [Source: epics.md#Story-9.20] — story spec verbatim
- [Source: prd.md#FR84] — keyboard-shortcut requirement
- [Source: static/js/mybibli.js:5-43] — existing `Ctrl+K` / `Ctrl+Shift+B` / `Ctrl+N` shortcuts (UNCHANGED)
- [Source: static/js/modal.js] — UX-DR8 modal infra (NOT reused for cheat sheet — see Dev Notes)
- [Source: 9-16-connection-lost-overlay.md] — `ConnectionStatusContext` precedent for the i18n-bundle helper
- [Source: 9-19-contextual-help-tooltips.md] — recent precedent for top-level i18n block + role-aware rendering
- [Source: CLAUDE.md#Foundation-Rules] — Rules #2, #11, #12, #15, #18

## Dev Agent Record

### Agent Model Used

claude-opus-4-7[1m]

### Debug Log References

(populated by dev agent)

### Completion Notes List

(populated by dev agent)

### File List

**Modified:**
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — status transitions.
- `locales/en.yml` + `fr.yml` — `shortcuts:` block (~16 keys per locale).
- `src/utils.rs` — `ShortcutsCheatSheetContext` helper.
- `templates/layouts/base.html` — `<dialog>` + footer link + `<script>`.
- ~25 page-route structs — one field + one ctor line.

**New:**
- `static/js/shortcuts.js` — IIFE module (~150 LOC).
- `tests/keyboard_shortcuts_cheat_sheet.rs` — 8 integration cases (~250 LOC).
- `tests/e2e/specs/journeys/keyboard-shortcuts-cheat-sheet.spec.ts` — 5 E2E scenarios (~200 LOC).

### Change Log

| Date | Change |
| --- | --- |
| 2026-05-10 | Story created (backlog → ready-for-dev). Adds `?`, `g`-chord (`g h/c/l/b/a`), and a cheat-sheet `<dialog>` with role-aware filtering. Native `<dialog>.showModal()` over modal.js's manual focus-trap infra (the cheat sheet is purely client-side static markup; native API does the job in ~5 LOC vs ~50). Existing `Ctrl+K` / `Ctrl+Shift+B` / `Ctrl+N` in mybibli.js kept as-is — listed in the cheat sheet so users discover both. Server-side role-gating for cheat-sheet content (Askama if-blocks). 16 i18n keys per locale bundled in a `ShortcutsCheatSheetContext` helper struct (mirror of 9-16's pattern); ~25 page structs gain one field + one ctor line each. 8 integration tests + 5 E2E scenarios. Foundation Rule #2 waiver inherits 9-16's deferred Vitest harness. |
