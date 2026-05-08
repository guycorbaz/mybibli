# Story 9.17: NavBar — hamburger menu + scanner auto-close

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a user on a tablet or mobile device,
I want the navigation bar to collapse into a hamburger menu, and any open menu to auto-close when a scanner burst arrives,
so that the menu does not interfere with cataloging on small screens.

## ⚠️ Existing-code reality check

Before writing a single line, walk the code that 9-17 touches and verify the assumptions below — they are LOCKED IN by current main as of 2026-05-08 (post 9-16 close):

- **Mobile nav markup ALREADY EXISTS** in `templates/components/nav_bar.html:50-83`. Verified:
  - `<button id="mobile-menu-toggle" class="md:hidden" aria-label="Open menu" aria-expanded="false" aria-controls="mobile-nav">` at line 50, with the ☰ icon SVG at line 51.
  - `<div id="mobile-nav" class="hidden md:hidden ...">` at line 57 contains the same nav links + language toggle. The `hidden` class is the JS-toggleable state; the `md:hidden` redundancy (which renders as `display: none` on md+ breakpoints) ensures the panel stays hidden on desktop even when the toggle script removes the first `hidden`.
  - All role-gated links are duplicated across the desktop list (lines 6-18) and the mobile panel (lines 57-83). Same `current_page` highlighting logic.
  - **9-17 SCOPE = enhancing the EXISTING toggle, not building from scratch.** No template restructuring; no new fragments.

- **Existing toggle JS** at `static/js/mybibli.js:174-184` (`initMobileMenuToggle`):
  ```js
  function initMobileMenuToggle() {
      var btn = document.getElementById("mobile-menu-toggle");
      var menu = document.getElementById("mobile-nav");
      if (!btn || !menu) return;
      btn.addEventListener("click", function () {
          var nowHidden = menu.classList.toggle("hidden");
          btn.setAttribute("aria-expanded", String(!nowHidden));
      });
  }
  ```
  This is a basic click-to-toggle. **9-17 EXTRACTS this into a NEW dedicated module `static/js/nav.js`** and adds: outside-click close, Escape close, focus trap, scanner-burst auto-close, route-change close. The mybibli.js bootstrap calls `initMobileMenuToggle()` somewhere — verify in Task 1 and remove the call (replaced by nav.js's own `init()`).

- **Breakpoint discrepancy with UX-DR24**: the existing markup uses Tailwind `md:hidden` (≥ 768px = desktop). UX-DR24 in the spec says "desktop breakpoint < 1024px" which is `lg:` in Tailwind. **9-17 DOES NOT CHANGE THE BREAKPOINT** — this is a separate UX decision that should be discussed in a follow-up if the team wants to align with UX-DR24's 1024px figure. Document the discrepancy in Dev Agent Record but ship the existing `md:` (768px) breakpoint as-is. Changing to `lg:` would also flip ~20 desktop-tablet users into mobile mode without coordination.

- **Scanner-guard module** at `static/js/scanner-guard.js`:
  - Public API at line 165: `window.mybibliScannerGuard = { getStackDepth, isActive }` — does NOT expose burst-detection events.
  - `MODAL_SELECTOR = 'dialog[open], [aria-modal="true"]'` at line 38 — burst-capture is gated to modal surfaces. The mobile nav panel is NEITHER a `<dialog open>` NOR `aria-modal="true"`.
  - **Scanner-burst detection** itself (the `< scannerThreshold` interKey logic) lives in `static/js/search.js:30-98`, NOT in scanner-guard. Search.js's state machine flips IDLE → DETECTING → SEARCH_MODE → SCAN_PENDING based on the inter-key delay. **9-17 needs a way to detect a scanner burst from nav.js without duplicating search.js's state machine.**
  - **DECISION** (frozen): rather than duplicate the burst detection, nav.js listens for the FIRST keydown burst on `document` while the panel is open. If two consecutive keys arrive within ~50ms (matches `scanner_burst_threshold_ms`), close the panel. Lightweight standalone detector — no coupling to search.js.

- **`scanner_burst_threshold_ms`** in `src/config.rs:157` (default 50ms) is the canonical config value. **CAVEAT — the `data-scanner-threshold` attribute lives ONLY on `#search-field` at `templates/pages/home.html:33` (value `"100"` there, not `"50"`); the canonical `#scan-field` template (`templates/components/scan_field.html`) does NOT carry the attribute.** Therefore nav.js's burst detector should: (a) on the home page, optionally read `document.getElementById("search-field")?.dataset.scannerThreshold`; (b) on every other page, fall back to a hardcoded constant `50` (matches the config default and is conservative — bursts can only get faster, not slower). Reading from `#scan-field`'s nonexistent `data-scanner-threshold` would always return `undefined` and is a dead branch — do NOT add it.

- **Focus-trap helper precedent** at `static/js/modal.js:28-33` (`focusableInside(dialog)`):
  ```js
  function focusableInside(dialog) {
      return Array.prototype.slice.call(dialog.querySelectorAll(
          "a[href], button:not([disabled]), input:not([disabled]), select, textarea, [tabindex]:not([tabindex='-1'])"
      ));
  }
  ```
  Keep modal.js's helper as-is. nav.js can copy or import the same selector. **DECISION**: copy-paste the selector logic (rule-of-three not yet hit; modal.js + nav.js = 2 callers). A future story can extract once a third surface (e.g., a sidebar) needs it.

- **Outside-click close** pattern: listen for `mousedown` on `document`, check if the event target is INSIDE `#mobile-nav` OR `#mobile-menu-toggle` (the trigger). If neither, close. Avoid `click` because it fires after `mousedown` + `mouseup` — a user dragging from inside the panel to outside (e.g., text selection) would close it on `click` but not on `mousedown`. Mirror of the modal.js approach (verify modal.js uses mousedown).

- **Escape close** + focus restore: standard pattern. Reuse `previousActiveElement` tracking (mirror of connection-monitor.js's pattern from 9-16).

- **Route change auto-close**: HTMX nav fires `htmx:afterRequest` with `e.detail.successful` — but full-page navigation via `<a href="/loans">` (the actual nav links) doesn't fire HTMX. The browser navigates away and the next page rebuilds the navbar from scratch (panel closed by default). **DECISION**: no explicit `popstate` / `hashchange` listener needed for full-page nav; the panel is closed by virtue of new page render. For HTMX-boosted nav (if any future story enables `hx-boost` on the nav links), the panel won't auto-close — flag as deferred. v1: no special handling.

- **Click-on-link close** (AC requires): the AC says "clicks a link → panel closes". For full-page navigation, the panel is irrelevant (page reloads). For users who click a link with modifier keys (Ctrl-click → new tab), the current page stays AND the panel should close. So add a click-listener on `#mobile-nav a[href]` that calls `close()`. Or simpler: any click inside the panel that isn't on the trigger button triggers close. **DECISION**: explicitly handle link-click for the modifier-key case; the simple "close on link click" is well-understood UX and inexpensive.

- **CSP compliance (story 7-4)**: nav.js loaded via `<script src="/static/js/nav.js">`; no inline handlers; uses `addEventListener` only.

- **Foundation Rule #2 (Unit Tests)**: same JS-harness gap as 9-16. AC for waiver mirrors AC18 of 9-16 — JS coverage delegated to E2E + integration tests on the rendered markup. File the same `type:change-request` GH issue at story close (or merge with 9-16's deferred ticket).

- **`<dialog>` vs disclosure pattern**: spec says either is acceptable. **DECISION**: stay with the existing `<div>` disclosure pattern (current markup at nav_bar.html:57-83). Migrating to `<dialog>` would require:
  - HTML5 `<dialog>` element (not `<div>`)
  - `dialog.showModal()` / `dialog.close()` API calls
  - Backdrop styling (browsers handle via `::backdrop` pseudo)
  - The `<dialog>` blocks page interaction (built-in modal behavior)
  
  These are all wins, but the migration is invasive and the disclosure pattern works fine with the manual focus-trap + Escape + outside-click implementation. v1: keep disclosure. A future story (e.g., 9-21 responsive layouts) can migrate to `<dialog>` if there's appetite.

## Acceptance Criteria

1. **AC1 — NEW JS module `static/js/nav.js`** (~120 LOC, IIFE shape):
   - Replaces and supersedes `initMobileMenuToggle()` in `mybibli.js:176-184`. **Remove that function + its bootstrap call** as part of this story.
   - Idempotent via `dataset.wired` guard on `#mobile-menu-toggle`.
   - State object: `{ open: bool, previousActiveElement: Element|null }`.
   - **`open()`** function: removes `hidden` class from `#mobile-nav`, sets `aria-expanded="true"` on the trigger, captures `previousActiveElement = document.activeElement`, focuses the first focusable element inside the panel (mirror of `modal.js`'s `focusableInside(dialog)[0]` pattern).
   - **`close({ restoreFocus = true })`** function: adds `hidden` class, sets `aria-expanded="false"`, restores focus to the trigger button (or `previousActiveElement` if it was the trigger; otherwise restore to the trigger to avoid focus jumping to the body).
   - **Trigger click**: toggles open/close state.
   - **Outside-click close**: `mousedown` listener on `document`. If `state.open` is true AND the event target is OUTSIDE `#mobile-nav` AND OUTSIDE `#mobile-menu-toggle`, call `close()`.
   - **Escape close**: `keydown` listener on `document`. If `state.open && e.key === "Escape"`, call `close()`.
   - **Focus trap**: `keydown` listener — if `e.key === "Tab"`, find focusable elements inside `#mobile-nav` and cycle (Shift+Tab from first → last; Tab from last → first). Mirror of `modal.js:75-95`'s focus-trap loop.
   - **Link click close**: delegated `click` listener on `#mobile-nav a[href]` → call `close({ restoreFocus: false })` (don't fight the navigation).
   - **Scanner-burst auto-close**: `keydown` listener on `document`. Track `lastKeydownAt = Date.now()` on every keystroke. If `state.open` AND `now - lastKeydownAt < threshold`, call `close({ restoreFocus: false })`. **Threshold source**: read `document.getElementById("search-field")?.dataset.scannerThreshold` (only present on the home page, value `"100"`); else hardcoded constant `50` (matches `scanner_burst_threshold_ms` config default). After close, forward the CURRENT keystroke (the one that confirmed the burst) to `#scan-field` if present via `scanField.value += e.key` + `dispatchEvent(new Event("input", { bubbles: true }))`; if `#scan-field` is absent, drop silently. Lightweight standalone burst detector — does NOT depend on scanner-guard.js's modal stack.
   - **`pagehide` listener**: noop (panel is hidden by default on next page load); state tracked in closure dies with the document.
   - CSP-clean: no `eval`, no inline handlers, all listeners via `addEventListener`.

2. **AC2 — Register `nav.js` in `templates/layouts/base.html`**:
   - Insert `<script src="/static/js/nav.js"></script>` immediately AFTER `<script src="/static/js/modal.js"></script>` (line 62) and BEFORE `<script src="/static/js/mybibli.js" defer></script>` (line 63). Rationale: nav.js is in the UI/keyboard-surface family with `modal.js` and `scanner-guard.js`; placing it after `modal.js` keeps load order intuitive (sync scripts in source order, `mybibli.js` deferred LAST). Putting nav.js after the deferred `mybibli.js` would technically work but is misleading because deferred scripts run AFTER all sync scripts regardless of source order.
   - **Remove `initMobileMenuToggle()` from `mybibli.js`** + its bootstrap call (`init()` at `mybibli.js:328` calls it). Verify no other module references the function.

3. **AC3 — Existing nav_bar.html markup UNCHANGED** (template-side):
   - The hamburger button + mobile panel markup at `nav_bar.html:50-83` is byte-identical post-9-17. The story only adds JS behavior; the markup is already correct (button has `aria-expanded`, `aria-controls`, panel has `id="mobile-nav"` + `hidden md:hidden` classes).
   - Exception: an `aria-label` translation key may be added if the AC4 i18n requires it. Default markup has `aria-label="Open menu"` — verify it's swapped to `t!()` if not already.

4. **AC4 — i18n: 1 new key per locale OR reuse existing**:
   - The hamburger button's `aria-label` should read "Open menu" / "Ouvrir le menu" per UX-DR6. Verify `nav_bar.html:50` — currently `aria-label="Open menu"` (hardcoded EN string).
   - If hardcoded, add `nav.menu_open: "Open menu"` / `"Ouvrir le menu"` to `locales/{en,fr}.yml` and replace the literal with `{{ nav_menu_open }}` in the template.
   - Page structs gain ONE field `nav_menu_open: String` populated via `t!("nav.menu_open", locale = loc)`. Mirror of the existing `nav_logout` field pattern (~19 page structs).
   - Run `cargo test all_t_keys_have_both_locales` after.
   - Run `touch src/lib.rs && cargo build`.

5. **AC5 — Desktop breakpoint UNCHANGED** (md: 768px):
   - The existing `md:hidden` class on the hamburger button (line 50) and the `hidden md:hidden` on `#mobile-nav` (line 57) keep the hamburger HIDDEN on `≥ 768px` and SHOW the inline desktop nav links.
   - **Discrepancy with UX-DR24** (which mentions 1024px): document in Dev Notes; ship as-is. A follow-up `type:change-request` GH issue can address the breakpoint alignment.

6. **AC6 — Role-based link visibility regression-free**:
   - Both the desktop list (lines 6-18) and the mobile panel (lines 57-83) currently apply `{% if role == "librarian" || role == "admin" %}` and `{% if role == "admin" %}` gates correctly. **9-17 does NOT touch the role logic.**
   - E2E asserts: anonymous user on tablet viewport opens hamburger → sees only `Catalog`/`Locations`/`Series`. Librarian sees those + `Borrowers`/`Loans`. Admin sees all.

7. **AC7 — Focus trap correctness**:
   - When the panel is open, Tab from the LAST focusable element inside the panel cycles to the FIRST. Shift+Tab from the FIRST cycles to the LAST.
   - Tab from outside the panel into it works normally (no trap until the user is inside).
   - Escape closes and restores focus to the trigger button (`#mobile-menu-toggle`).

8. **AC8 — Scanner-burst auto-close**:
   - Open panel + simulate a scanner burst (≥ 2 keys with inter-key < 50ms). Panel closes; the CURRENT keystroke (the one that confirmed the burst) is forwarded to `#scan-field` if present on the page. If no scan field exists on the page, the burst-confirming keystroke is dropped silently (panel just closes).
   - This requires nav.js to have a per-page-load timestamp comparison: `lastKeydownAt` updated on every keydown; if `now - lastKeydownAt < threshold`, treat as burst.
   - **Known v1 limitation (accepted)**: the FIRST keystroke of a burst cannot be classified as "burst" because there is no prior timestamp to compare against. It is consumed by the panel's normal focus target (typically a no-op since panel-internal links don't react to single printable chars; Enter on a focused link would activate it — the user re-scans). This single-character loss is acceptable for v1 because: (a) ISBN/V-code bursts terminate with Enter that fires AFTER nav.js has already closed the panel, so the Enter lands on `#scan-field`; (b) re-scanning is a low-friction recovery; (c) buffering recent keys to replay them adds complexity without proportional benefit. Document this in Dev Notes; revisit if user-testing surfaces friction.

9. **AC9 — CSP compliance**:
   - Run `cargo test no_inline_markup_in_templates` (no new inline `style=`, `<style>`, `onclick=`).
   - The new JS module is loaded via `<script src=...>`.

10. **AC10 — Unit tests (Rust side)** in `tests/navbar_hamburger.rs` (new file, ~120 LOC). 4 cases:
    1. `nav_js_is_registered_in_base_layout` — assert the rendered HTML contains `<script src="/static/js/nav.js">`.
    2. `mobile_menu_button_renders_with_correct_attributes` — GET `/login`, assert: `id="mobile-menu-toggle"`, `aria-label` (EN-locale) matches `nav_menu_open` t!() value, `aria-expanded="false"`, `aria-controls="mobile-nav"`, `class` contains `md:hidden`.
    3. `mobile_nav_panel_renders_role_gated_links` — for anonymous, librarian, admin sessions, assert the panel HTML contains the expected link set (anonymous: catalog/locations/series; librarian: + borrowers/loans; admin: + admin).
    4. `aria_label_renders_in_french_locale` — GET `/login` with `Cookie: lang=fr`, assert `aria-label="Ouvrir le menu"`.

11. **AC11 — E2E test** — NEW spec `tests/e2e/specs/journeys/navbar-hamburger.spec.ts` (~160 LOC). 5 scenarios using `page.setViewportSize({ width: 600, height: 800 })` for tablet sim:
    1. **Hamburger visible on tablet, hidden on desktop**: tablet viewport → `#mobile-menu-toggle` visible, desktop nav links hidden. Resize to `1280x720` → hamburger hidden, desktop links visible.
    2. **Open / link-click / Escape / outside-click**: tablet → click hamburger → panel visible + `aria-expanded="true"` → click a link inside → panel closes (verify `hidden` class re-added). Repeat with Escape: close. Repeat with click on `<body>` (outside both trigger and panel): close.
    3. **Focus trap**: open panel → first focusable element gains focus → Tab → second → … → last → Tab → wraps to first. Shift+Tab from first → wraps to last.
    4. **Scanner-burst auto-close on a page WITH `#scan-field`** (e.g., `/catalog`): open panel → call `simulateScan(page, "body", "AB")` from `tests/e2e/helpers/scanner.ts` (20 ms inter-key, trusted Playwright events) → panel closes within `expect(...).toBeHidden({ timeout: 1000 })` AND `#scan-field` value contains the burst-confirming character. **Mechanism**: pass `"body"` as the selector so `simulateScan` does NOT focus a different element — the keystrokes fire at `document` while the panel-internal element keeps focus. Do NOT use `dispatchEvent(new KeyboardEvent(...))` (untrusted events may be filtered).
    5. **Scanner-burst auto-close on a page WITHOUT `#scan-field`** (e.g., `/admin` after admin login, or any page where `#scan-field` is absent — verify in Task 1): open panel → `simulateScan(page, "body", "AB")` → panel closes. Assertion: `#mobile-nav.hidden` re-added; no error thrown by nav.js when `#scan-field` is null. Locks the "drop silently" branch of AC8.
    - Stable selectors: `#mobile-menu-toggle`, `#mobile-nav`, `#mobile-nav a[href]`.
    - Flake gate: no `waitForTimeout(N)`. Use `expect(...).toBeHidden({ timeout: ... })` for the post-burst close assertion. Use Playwright's native `keyboard.type(code, { delay: 20 })` via `simulateScan` — do NOT roll a manual `dispatchEvent` sequence.

12. **AC12 — Foundation Rule #12 LOC discipline**:
    - `static/js/nav.js`: NEW ~120 LOC.
    - `static/js/mybibli.js`: net −10 LOC (remove `initMobileMenuToggle` + its bootstrap call).
    - `templates/layouts/base.html`: net +1 LOC (`<script src="/static/js/nav.js">`).
    - `templates/components/nav_bar.html`: net +0/-0 LOC if `aria-label` was already templated; +1 LOC if it gets a new template variable.
    - `tests/navbar_hamburger.rs`: NEW ~120 LOC.
    - `tests/e2e/specs/journeys/navbar-hamburger.spec.ts`: NEW ~160 LOC (5 scenarios after AC11 expansion).
    - `locales/{en,fr}.yml`: +1 key per locale (verified missing as of 2026-05-08 — `grep -ni "menu" locales/en.yml` returns nothing).
    - `src/routes/*.rs` (the page structs): +1 field + 1 ctor line per page struct. Exact site count to be measured in Task 1 via `grep -nE 'nav_logout: rust_i18n' src/routes/*.rs` (currently 23 occurrences; expected ~19 page structs after excluding test fixtures, mirroring 9-16's final count). Net LOC well under 2000 per file.

13. **AC13 — Story-level grep audit** at story close:
    - `grep -rn 'initMobileMenuToggle' static/js/`: ZERO hits (function removed).
    - `grep -rn 'mobile-menu-toggle' static/js/`: only in `nav.js` (1+ hits).
    - `grep -rn 'mobile-nav' templates/ static/`: 1 in `nav_bar.html` (the panel id) + 1 in `nav_bar.html:50` (`aria-controls`) + N in `nav.js`.

14. **AC14 — Local Testing Before Push**:
    - `SQLX_OFFLINE=true cargo check` clean
    - `cargo clippy --all-targets -- -D warnings` clean
    - `cargo test --lib` green (≥769 lib tests + the new ~4 navbar_hamburger cases)
    - `cargo test --test navbar_hamburger` green
    - `cargo test no_inline_markup_in_templates` green
    - `cargo test all_t_keys_have_both_locales` green (if AC4 added a new key)
    - Full E2E green
    - Flake gate clean

15. **AC15 — Draft PR + CI gate**: Foundation Rule #15 + #18.

16. **AC16 — Foundation Rule #2 (Unit Tests) — explicit waiver for nav.js JS module**:
    - Same waiver as 9-16 AC18. JS coverage delegated to E2E (4 scenarios) + Rust integration tests (4 cases).
    - Document explicitly in Dev Agent Record. The deferred GH issue from 9-16 ("Add JS unit-testing harness Vitest") can subsume this.

## Tasks / Subtasks

- [ ] **Task 1 — Verify reality-check assumptions (AC: all)**
  - [ ] Re-read `templates/components/nav_bar.html:50-83` to confirm the markup is what the spec says (button + panel + role gates).
  - [ ] Read `static/js/mybibli.js:174-184` (`initMobileMenuToggle`) and find its bootstrap call (likely in mybibli.js's main `init()`). Capture for Task 6's removal.
  - [ ] Verify `nav.menu_open` does NOT exist in `locales/{en,fr}.yml` (grep). If it exists, skip AC4's add step.
  - [ ] Read `static/js/modal.js:28-33` (`focusableInside`) to confirm the focusable-selector pattern.
  - [ ] Verify `aria-label="Open menu"` at `nav_bar.html:50` is hardcoded EN (not `{{ nav_menu_open }}`). If templated, skip AC4's substitution.
  - [ ] Map all page structs that pass `nav_logout` (the closest pattern to `nav_menu_open`). Run `grep -nE 'nav_logout: rust_i18n' src/routes/*.rs | wc -l` for the EXACT occurrence count (currently 23 lines as of 2026-05-08; the 9-16 retrospective recorded 19 page-template structs after excluding test fixtures + `StepProviders` false-positive). Document the actual count + per-file breakdown in Dev Agent Record before editing.
  - [ ] **Confirm scanner-threshold attribute location**: `data-scanner-threshold` lives ONLY on `#search-field` (`templates/pages/home.html:33`, value `"100"`). The canonical `#scan-field` template (`templates/components/scan_field.html`) does NOT carry the attribute. nav.js's burst detector should read `document.getElementById("search-field")?.dataset.scannerThreshold` (home page only), else fall back to hardcoded `50`. Do NOT add a `data-scanner-threshold` to `#scan-field` — that's out of scope.
  - [ ] Confirm `#scan-field` is NOT rendered on every page (e.g., `/admin`, `/borrowers/{id}`). nav.js's "forward to `#scan-field`" branch must safely no-op when null. Pick a known-no-`#scan-field` page for AC11 Test 5.
  - [ ] Run baseline `cargo test no_inline_markup_in_templates` to confirm green BEFORE editing.

- [ ] **Task 2 — i18n key (AC: 4)** — CONDITIONAL on Task 1 outcome:
  - [ ] If `nav.menu_open` is missing in locales: add to `locales/en.yml` (`nav.menu_open: "Open menu"`) + `locales/fr.yml` (`"Ouvrir le menu"`). Insert next to `nav.logout`.
  - [ ] Otherwise: skip; the existing key is reused.
  - [ ] `touch src/lib.rs && cargo build`.

- [ ] **Task 3 — Update page structs to carry `nav_menu_open` (AC: 4)** — CONDITIONAL on Task 1:
  - [ ] If new key added: ~19 page structs gain `pub nav_menu_open: String` field + 1 ctor line `nav_menu_open: rust_i18n::t!("nav.menu_open", locale = loc).to_string(),`. Use `sed` mirror of 9-16's struct-edit pattern.
  - [ ] Update `nav_bar.html:50` to use `{{ nav_menu_open }}` (or whatever the i18n key resolves to) for the `aria-label`.
  - [ ] Run `cargo build` clean.

- [ ] **Task 4 — Create `static/js/nav.js` (AC: 1, 7, 8, 9)**
  - [ ] Implement IIFE per AC1 spec. State, `open()`, `close()`, focus trap, outside-click, Escape, link-click, scanner-burst, pagehide.
  - [ ] CSP-clean (no `eval`, no inline handlers).
  - [ ] Include comment block referencing AC mappings (AC1, AC7, AC8 etc.).

- [ ] **Task 5 — Register `nav.js` in `base.html` (AC: 2)**
  - [ ] Add `<script src="/static/js/nav.js"></script>` after `mybibli.js`.

- [ ] **Task 6 — Remove `initMobileMenuToggle` from `mybibli.js` (AC: 1, 13)**
  - [ ] Delete the function at `mybibli.js:174-184` and its bootstrap call (likely in the main `init()` or DOMContentLoaded handler).
  - [ ] Verify `grep -rn 'initMobileMenuToggle' static/js/` returns ZERO hits.

- [ ] **Task 7 — Integration tests (AC: 10)**
  - [ ] Create `tests/navbar_hamburger.rs` with 4 cases per AC10. Use a minimal `build_state` helper.
  - [ ] Run `SQLX_OFFLINE=true cargo test --test navbar_hamburger` and confirm 4/4 green.

- [ ] **Task 8 — E2E test (AC: 11)**
  - [ ] Create `tests/e2e/specs/journeys/navbar-hamburger.spec.ts` with 5 scenarios per AC11.
  - [ ] Use `page.setViewportSize` for tablet/desktop simulation.
  - [ ] Use `simulateScan(page, "body", "AB")` from `tests/e2e/helpers/scanner.ts` for the burst tests (Tests 4 + 5). Do NOT roll `dispatchEvent(new KeyboardEvent(...))` sequences — untrusted events can be filtered, and manual timing trips the flake gate.
  - [ ] Run `cd tests/e2e && npx tsc --noEmit` to verify the spec compiles.
  - [ ] Run `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/navbar-hamburger.spec.ts` (single-spec) and confirm green.
  - [ ] Run full E2E lane to confirm no regressions.

- [ ] **Task 9 — Local gate + push + draft PR (AC: 14, 15, 16)**
  - [ ] `SQLX_OFFLINE=true cargo check` clean
  - [ ] `cargo clippy --all-targets -- -D warnings` clean
  - [ ] `cargo test` (full lib + integration) green
  - [ ] CI flake gate: `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` returns nothing.
  - [ ] Run AC13 grep audit, document in Dev Agent Record.
  - [ ] Push branch + open draft PR.
  - [ ] WAIT for CI green per Foundation Rule #18.

## Dev Notes

### Why a NEW `static/js/nav.js` module vs extending `mybibli.js`

`mybibli.js` is the catch-all utility module. It already does feedback dismissal, scan-field restoration, mobile-menu basic toggle, and assorted bootstrap. Story 9-17 grows the mobile-menu surface from "click toggle" to "full disclosure with focus trap + outside-click + Escape + scanner-burst auto-close + link-click close" — that's ~120 LOC of focused logic. Putting it in mybibli.js would push that file over 300 LOC of unrelated concerns. A dedicated `nav.js` module is the right factoring.

### Why disclosure pattern, not `<dialog>`

The existing markup is a `<div>` with `hidden` class. Migrating to `<dialog>` requires:
- HTML5 `<dialog>` element with `showModal()` / `close()` API
- Backdrop styling via `::backdrop` pseudo-element
- Native focus-trap (browser handles it)
- Native Escape close

These are all wins, but the migration:
- Changes the DOM contract on every page that includes nav_bar.html (~all pages)
- Native `<dialog>` blocks page interaction (scroll, focus) — strong modal semantics that may be too heavy for a navigation menu
- Browser support is universal in 2026 but the visual polish (transitions, backdrop) needs CSS rework

Pragmatic v1: keep `<div>` disclosure. A future story can migrate if `<dialog>` gives ergonomic wins (e.g., 9-21 responsive layouts).

### Why standalone burst detector vs reusing scanner-guard.js

scanner-guard.js detects bursts via its event capture phase, but only WHILE a modal is open. Its API doesn't expose burst events for non-modal callers. Two options:
- **A**: Extend scanner-guard.js to dispatch a custom event on burst detection. Other modules (nav.js) listen.
- **B**: nav.js implements a tiny independent burst detector — last-keydown timestamp + threshold compare.

**Choice: B.** scanner-guard's burst detection is intrinsically tied to the modal-stack semantics (gate-then-forward). Lifting it out as a generic emitter risks confusing the existing contract. nav.js's needs are simpler: a 1-line "interKey < 50ms?" check. Standalone is cleaner, no cross-module coupling.

### Why no `<dialog>` does NOT trigger scanner-guard

Per 9-16's patch P2, scanner-guard's `MODAL_SELECTOR` matches `dialog[open], [aria-modal="true"]`. The mobile nav panel is neither. Therefore scanner-guard does NOT capture keystrokes while the panel is open — keystrokes flow normally. nav.js's standalone burst detector handles the auto-close.

If a future story migrates the mobile nav to `<dialog open>`, scanner-guard would auto-engage and the standalone detector becomes redundant. v1 keeps standalone.

### Breakpoint discrepancy (Tailwind `md:` vs UX-DR24's 1024px)

The existing markup uses `md:hidden` (Tailwind `md` = 768px). UX-DR24 says "below desktop breakpoint < 1024px". Tailwind `lg:` is 1024px. Two interpretations:
- The markup intent: tablets in landscape (≥768px) use desktop nav.
- The UX-DR24 intent: tablets in landscape (768-1023px) use mobile nav.

These conflict. **9-17 ships the existing `md:` breakpoint** because (a) changing the breakpoint affects all users with 768-1023px viewports, (b) the UX/PRD discussion needs to happen separately, (c) the current markup has worked since Epic 1. File a `type:change-request` GH issue at story close to align breakpoints with UX-DR24.

### Foundation Rule #2 waiver (JS module unit tests)

Same gap as 9-16. mybibli has no JS unit-testing harness. `nav.js`'s behavior is exercised by:
- **Integration tests (Rust)**: rendered markup + script registration verification.
- **E2E tests (Playwright)**: actual click/Escape/outside-click/burst flows via real browser.

These two layers cover the contract. The JS module's branches without DOM (e.g., `state.open` toggle logic) would benefit from Vitest tests but require infrastructure. File `type:change-request` GH issue: "Add JS unit-testing harness (Vitest) for browser modules" — same as 9-16. May be DRY-merged with 9-16's deferred ticket.

### NEW deferred items this story will file

- **Tailwind breakpoint alignment with UX-DR24** (`type:change-request`): mybibli currently uses `md:` (768px) for the mobile-nav cutoff; UX-DR24 mentions 1024px (`lg:`). Needs UX/PRD discussion.
- **Migrate mobile nav to `<dialog>`** (`type:change-request`): potential v2 polish to leverage native focus-trap, backdrop, and Escape semantics.
- **Extract `focusableInside` to a shared helper** (`type:code-review-finding`): rule-of-three not yet hit (modal.js + nav.js); revisit if a third surface needs it.

### Project Structure Notes

- `static/js/nav.js` — NEW module.
- `static/js/mybibli.js` — `initMobileMenuToggle` removed.
- `templates/components/nav_bar.html` — markup unchanged (or 1-line aria-label substitution if AC4).
- `templates/layouts/base.html` — script registration line added.
- `tests/navbar_hamburger.rs` — NEW integration test.
- `tests/e2e/specs/journeys/navbar-hamburger.spec.ts` — NEW E2E spec.
- ~19 page-route structs gain 1 field + 1 ctor line if AC4 adds the new i18n key.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.17] — story spec verbatim
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#UX-DR6] — partial scope: hamburger + scanner auto-close
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#UX-DR24] — desktop breakpoint < 1024px (discrepancy with current `md:` ≥ 768px)
- [Source: _bmad-output/implementation-artifacts/9-16-connection-lost-overlay.md] — recent precedent for new JS module + i18n bundle pattern
- [Source: CLAUDE.md#Foundation-Rules] — Rules #1, #11, #12, #13, #15, #18
- [Source: CLAUDE.md#Modal-scanner-guard-invariant-story-7-5] — scanner-guard's MODAL_SELECTOR contract
- [Source: templates/components/nav_bar.html:50-83] — existing hamburger button + mobile panel markup (unchanged in this story)
- [Source: static/js/mybibli.js:174-184] — existing `initMobileMenuToggle` (removed in this story)
- [Source: static/js/modal.js:28-33,75-95] — focus-trap precedent (`focusableInside` + Tab/Shift+Tab cycle)
- [Source: static/js/scanner-guard.js:38] — `MODAL_SELECTOR` (does NOT match the mobile nav panel)
- [Source: src/config.rs:157,204] — `scanner_burst_threshold_ms` default 50ms
- [Source: static/js/connection-monitor.js (story 9-16)] — IIFE precedent + `pagehide` cleanup pattern + `dataset.wired` idempotency

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
- `locales/en.yml` and `fr.yml` — +1 key per locale (`nav.menu_open`) IF AC4 adds it.
- `static/js/mybibli.js` — `initMobileMenuToggle` removed.
- `templates/layouts/base.html` — `<script src="/static/js/nav.js">` registered.
- `templates/components/nav_bar.html` — `aria-label="Open menu"` → `{{ nav_menu_open }}` IF AC4.
- `src/routes/*.rs` (~19 page structs) — +1 field + 1 ctor line IF AC4.

**New:**
- `static/js/nav.js` — IIFE module (~120 LOC).
- `tests/navbar_hamburger.rs` — 4 integration test cases.
- `tests/e2e/specs/journeys/navbar-hamburger.spec.ts` — 4 E2E scenarios.

**No change:**
- `static/js/scanner-guard.js`, `static/js/modal.js`, `templates/components/nav_bar.html` markup (per AC3 unless AC4).

### Change Log

| Date | Change |
| --- | --- |
| 2026-05-08 | Story created (backlog → ready-for-dev). Third polish-finalize story in Epic 9 (post 9-16 close). Scope: enhance the EXISTING mobile-menu toggle (`mybibli.js:174-184` `initMobileMenuToggle`) into a full disclosure pattern with outside-click close, Escape close, focus trap, scanner-burst auto-close, link-click close. NEW dedicated module `static/js/nav.js` (~120 LOC IIFE). Markup at `templates/components/nav_bar.html:50-83` is byte-identical (only the `aria-label` may be templated if AC4 adds the i18n key). Standalone burst detector (no scanner-guard.js coupling). Disclosure pattern (no `<dialog>` migration in v1). 4 integration tests + 4 E2E scenarios. Foundation Rule #2 waiver for JS unit tests (delegated to E2E + integration). Breakpoint discrepancy (Tailwind `md:` ≥ 768px vs UX-DR24's < 1024px) ACKNOWLEDGED but NOT addressed in this story — filed as deferred `type:change-request`. Migrate-to-`<dialog>` polish also deferred. |
| 2026-05-08 | Story validated; 7 improvements applied (3 critical + 4 enhancements). **Critical fixes**: (C1) **`data-scanner-threshold` location corrected** — the attribute lives ONLY on `#search-field` (`templates/pages/home.html:33`, value `"100"`); the canonical `#scan-field` template does NOT carry it. nav.js's burst detector now reads `document.getElementById("search-field")?.dataset.scannerThreshold` (home page only), else hardcoded fallback `50`. Reality-check + AC1 + Task 1 updated to reflect this. (C2) **First-keystroke loss in burst detection acknowledged** — the FIRST keystroke of a scanner burst cannot be classified as "burst" (no prior timestamp); v1 accepts the single-character loss because (a) ISBN/V-code bursts terminate with Enter that lands on `#scan-field` AFTER nav.js closes the panel, (b) re-scan is low-friction recovery. Documented in AC8 + Dev Notes. (C3) **E2E Test 4 mechanism specified** — use `simulateScan(page, "body", "AB")` from `tests/e2e/helpers/scanner.ts` (Playwright trusted events, 20 ms inter-key); do NOT use `dispatchEvent(new KeyboardEvent(...))` (untrusted events can be filtered). **Enhancements**: (E1) **Script placement corrected** — insert `<script src="/static/js/nav.js">` AFTER `modal.js` (line 62) and BEFORE the deferred `mybibli.js` (line 63). Putting it after deferred `mybibli.js` would technically work but is misleading (deferred scripts run after all sync scripts regardless of source order). (E2) **Page-struct count grep updated** — Task 1 now runs `grep -nE 'nav_logout: rust_i18n' src/routes/*.rs | wc -l` for the EXACT count (23 occurrences as of 2026-05-08; ~19 page structs expected after excluding test fixtures). (E3) **search.js state-machine line range corrected** to `30-98`. (E4) **NEW E2E Test 5 added** — burst auto-close on a page WITHOUT `#scan-field` (e.g., `/admin`); locks the "drop silently" branch of AC8. AC11 grew from 4 to 5 scenarios; spec LOC budget revised to ~160. **Final scope unchanged** at the file/struct level; clarifications only. |
