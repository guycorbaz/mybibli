# Story 14.2: Navigation drawer on small screens

Status: done

GitHub issue: #547 (type:change-request). Covers: UX-DR39. Epic 14; ships in v1.24.0.

## Story

As a librarian on a phone or a tablet,
I want the same navigation in a drawer,
so that the screen stays for the content and every entry is one tap away.

## Reference

Mock-up `_bmad-output/planning-artifacts/ux-sidebar-mockup.html`, states "Mobile, fermée" / "Mobile, tiroir ouvert" (390 px): slim top bar (menu button + logo), drawer from the left (280 px, always expanded, close button in its head, scrim behind), foot with language, theme, logout. Decisions: drawer for every width < `lg` (1024 px), tablets included.

## Starting point (after Story 14-1, PR #613)

- One navigation source: `src/utils/nav.rs::nav_groups` → `BaseContextFields.nav_groups`; markup `templates/components/nav_entries.html`; shared pieces `templates/components/nav_macros.html` (icons, row classes, tooltip, `lang_select`, `theme_toggle`).
- ≥ `lg`: `templates/components/sidebar.html` (`nav#sidebar`, `hidden lg:flex`), state script `static/js/sidebar.js`, variant `sb-collapsed`.
- < `lg`: `templates/components/nav_bar.html` is the small-screen sticky header (`lg:hidden`) with `#mobile-menu-toggle` and the `#mobile-nav` dropdown panel (ids suffixed `-bar` / `-mobile`), driven by `static/js/nav.js`; 11-4 scroll margins (`scroll-mt-12 md:scroll-mt-14`) below `lg`.
- Read the 14-1 story file (`14-1-sidebar-wide-screens.md`), especially Completion Notes and Review Findings (dark mode now class-based and screen-only; storage guards; focus rules on /catalog).

## Acceptance Criteria

1. **AC1 — top bar.** Below `lg`: a slim sticky top bar with the menu button (`aria-expanded`, `aria-controls`, localized name `nav.menu_open`) and the logo linking home. Nothing else is required in the bar (language / theme / login-logout move to the drawer foot); if an anonymous visitor's Log in stays visible in the bar for convenience, it must not overflow at 360 px in any locale.
2. **AC2 — drawer.** The menu button opens the navigation as a drawer from the left (≈ 280 px, max 85 vw) over a scrim, rendered from the same single source (`nav_entries.html` + macros) — no new hand-copied list. Always expanded (no collapse toggle, no tooltips). Head: logo + close button (localized name). Foot: language select, theme toggle, Log out (POST + CSRF) or Log in. Current page `aria-current="page"`, Proposals badge and `aria-label` as in the sidebar.
3. **AC3 — dismissal and focus.** Escape, a tap on the scrim, the close button, and following a link close it. On open, focus moves into the drawer (first entry or close button); Tab stays inside while it is open (focus trap or `inert` on the rest of the page); on close, focus returns to the menu button (except when a link navigates away). Background does not scroll while open. `prefers-reduced-motion` disables the slide animation.
4. **AC4 — scanner.** While the drawer is open, a scanner burst does not reach `#scan-field` and does not activate a drawer control: the drawer is a modal surface for `static/js/scanner-guard.js` (e.g. `role="dialog"` + `aria-modal="true"` with an accessible name, or the guard's documented selector). Drawer z-order: above page content and the sticky bar, below modals, toasts and the connection-lost overlay (keep `templates_audit::fixed_surfaces_stack_above_sticky_nav` rule consistent — update its documentation if the drawer is a `fixed` surface with a deliberate lower z-index, or give it `z-50` and verify the stacking against modals in E2E).
5. **AC5 — removal.** The old `#mobile-nav` dropdown panel, its duplicate language form and `nav.js` panel logic are removed (or `nav.js` rewritten for the drawer). Only one navigation landmark named "Main navigation" exists in the DOM per breakpoint; update the 14-1 unit test accordingly (no more "one per breakpoint" exception if the drawer and the sidebar can share one landmark — otherwise document why two exist and that only one is ever displayed). The `xl` stopgap notes of 11-21 disappear from templates.
6. **AC6 — no overflow.** No horizontal overflow at 360, 390, 768 and 1023 px in fr / en / de / it for anonymous, librarian and admin, drawer open and closed.
7. **AC7 — tests.** Unit: rendering (bar, drawer, roles, landmarks), audit tests. E2E: `navbar-hamburger.spec.ts` and `responsive-layouts.spec.ts` rewritten around the drawer; new cases for open/close by each means, focus into drawer and back to the button, Tab trapped, scanner burst while open on /catalog, navigation from the drawer closes it, logout from the drawer, axe with drawer open and closed (light and dark). The 14-1 sidebar spec stays green.

## Tasks / Subtasks

- [x] T1 — drawer markup from the single source; slim bar (AC1, AC2)
- [x] T2 — drawer JS: open/close, focus trap / inert, scroll lock, reduced motion (AC3)
- [x] T3 — scanner-guard + stacking (AC4)
- [x] T4 — remove old panel; landmarks; comments (AC5)
- [x] T5 — tests; gates; full suites once before the single push (AC6, AC7)

## Dev Notes

- Reuse, do not fork: the sidebar and the drawer may even be the same `<nav>` element restyled per breakpoint (sidebar ≥ `lg`, off-canvas drawer < `lg`) — evaluate this first; it removes the second landmark and keeps one DOM.
- CSP: no inline script/style; animations via classes. i18n: four locales, `locale_parity`.
- Rule #12: files < 2000 lines.

## Completion Notes (2026-11-04)

**Design choice — one `<nav>` for the sidebar and the drawer (Dev Notes idea adopted).** `components/sidebar.html`'s `<nav id="sidebar" aria-label="Main navigation">` is the sidebar at ≥ `lg` and the off-canvas drawer below. Classes are mobile-first: base utilities describe the drawer (`fixed inset-y-0 left-0 z-50 w-70 max-w-[85vw] invisible -translate-x-full shadow-xl`), the `lg:` ones restore the 14-1 sidebar (`lg:visible lg:translate-none lg:sticky lg:top-0 lg:bottom-auto lg:left-auto lg:self-start lg:h-screen lg:w-56 lg:z-40 …`; `lg:translate-none` matters: a transform would trap the collapsed tooltips, `fixed` boxes, inside the nav). A new custom variant `nav-open:` (`static/css/input.css`, below 1024 px while `<html>` carries `nav-drawer-open`) slides it in. Why this rather than a second drawer element: one DOM, one landmark, one language form, one theme toggle, unique ids (`#lang-select`, `#theme-toggle`), no "one per breakpoint" exception, nothing to keep in sync; the costs are modest — a wrapper `#nav-drawer` (`lg:contents`, so the nav stays the flex item of the 14-1 row) carrying the scrim and the dialog semantics, and a few `lg:` resets. Dialog semantics cannot sit on the `<nav>` itself (they would replace the landmark), and must not be in the server markup (at ≥ `lg` scanner-guard would treat the sidebar as an always-open modal), so `static/js/nav.js` adds `role="dialog"`, `aria-modal="true"` and `aria-label` (from `data-dialog-label`, `nav.menu_dialog`) to the wrapper while open only.

**AC1 — top bar.** `components/nav_bar.html` is now a slim sticky `<header class="sticky top-0 z-40 print:static lg:hidden h-12 md:h-14 …">` with `#mobile-menu-toggle` (Heroicons bars-3, `aria-label` = `nav.menu_open`, `aria-expanded`, `aria-controls="sidebar"`) then the logo link home. No `<nav>`, no form, no Log in in the bar (moved to the drawer foot), so 360 px cannot overflow in any locale. Bar heights unchanged (11-4 scroll margins below `lg` unchanged).

**AC2 — drawer.** Same single source (`nav_entries.html` included once, macros reused): head = logo + close button `#nav-drawer-close` (`lg:hidden`, x-mark icon, `nav.menu_close`); entries (own vertical scroller); foot = language form, theme toggle, Log out (POST + CSRF) or Log in. The collapse toggle is wrapped in `hidden lg:block` and `sb-collapsed:` only matches from `lg`, so the drawer is always expanded, with no tooltips. `aria-current`, Proposals badge + `aria-label` exactly as in the sidebar (same markup). 280 px, at most 85 vw; scrim `#nav-scrim` (`fixed inset-0 z-50 bg-black/50`, `aria-hidden`).

**AC3 — dismissal and focus.** `static/js/nav.js` rewritten for the drawer: closes on Escape, scrim click, close button, any link click in the drawer (no focus return), and when the viewport grows to `lg` (matchMedia). On open focus goes to the close button; `#page-column` (bar, `<main>`, footer — new id in `layouts/base.html`) gets `inert`, and Tab / Shift+Tab cycle inside the drawer; on close focus returns to the menu button. `html.nav-drawer-open { overflow: hidden }` below `lg` stops the background scroll. Slide and fade are `motion-safe:` only; `visibility` is transitioned on closing only (the open state's transition list omits it — otherwise a discrete visibility transition keeps the drawer hidden at the start and `focus()` misses; found by E2E).

**AC4 — scanner.** While open the wrapper matches scanner-guard's `[aria-modal="true"]`: a burst reaches neither `#scan-field` nor a drawer control (E2E: bursts with the close button and with an entry focused; URL, scan field and feedback list unchanged). Because the guard also blocks a *human* Enter on a link / Space on a button (WCAG 2.1.1 for keyboard users of a narrow window), nav.js adds a window-capture listener that activates the focused drawer link (Enter) or button (Enter / Space) itself when the key comes at least the burst threshold (`#search-field[data-scanner-threshold]`, else 50 ms, floor 10) after the previous keydown — a scanner's terminating Enter follows its last character within a few ms and stays blocked. Stacking: drawer and scrim are `fixed z-50` (audit rule kept, its doc updated): above the `z-40` bar and the content; earlier in the DOM than the connection-lost overlay and JS toasts (same z-50, so they paint above); modals are in the top layer. E2E pins drawer over bar, scrim over content, overlay over drawer.

**AC5 — removal.** `#mobile-nav`, its unnamed language form, the bar's language form / theme toggle / login link, the icon-only `theme_toggle` branch of the macro, and the 9-17 panel logic + stand-alone burst detector of nav.js are gone. Exactly one "Main navigation" landmark in the DOM at every width (14-1's two-landmark exception removed from `tests/navbar_role_visibility.rs` and documented in the templates). No `xl` stopgap note left in templates (also removed the `xl` mention on `PROPOSALS_BADGE_CAP`).

**AC6 — no overflow.** New `responsive-layouts.spec.ts` matrix: anonymous / librarian / admin × fr / en / de / it × 360 / 390 / 768 / 1023 px, drawer closed and open: no page overflow, bar content on screen, drawer at x 0 and 280 px wide, no row (entry, language, theme, Log in / Log out) overflowing it. The admin 360 / 1023 / 1024 / 1920 matrix kept (adapted).

**AC7 — tests.** Unit (`src/routes/nav_sticky_tests.rs`, 24 tests): sidebar ≥ lg classes, drawer classes + scrim + no dialog semantics at load + `nav-open` variant + scroll lock CSS, close button / collapse toggle wide-only, one landmark, entries rendered once, slim bar contents, one logout / login / language form / theme toggle, nav.js ↔ scanner-guard contract; `utils.rs` base-context test covers the two new keys; `templates_audit.rs` docs and the inner-scroller exemption updated. Integration: `navbar_hamburger.rs` (drawer slice, `aria-controls="sidebar"`, `lg:hidden` bar), `navbar_role_visibility.rs` (panel assertions removed, one landmark, one logout, one aria-current in the nav), `proposals_access.rs` (one badge). E2E: `navbar-hamburger.spec.ts` rewritten (10 tests: breakpoint + resize-to-lg closes, close/scrim/Escape with focus return and dialog role+name, link closes without focus return, focus into drawer + Tab trap + inert + no background scroll, scanner bursts on /catalog, human Enter follows a link + next page closed + aria-current, tap navigation, logout from the drawer, stacking, reduced motion, axe open/closed × light/dark); `responsive-layouts.spec.ts` (drawer instead of panel, AC6 matrix); `navbar-role-visibility`, `proposals-access`, `borrowers`, `login-smoke`, `language-toggle` (comments), `sidebar.spec.ts` (logo selector) adapted; new helper `tests/e2e/helpers/drawer.ts`. i18n: `nav.menu_close`, `nav.menu_dialog` in en/fr/de/it.

**Deviations / notes.**
- Focus on open goes to the close button (AC allows "first entry or close button").
- The human-paced Enter/Space activation in the drawer is an addition not named in the ACs (needed so the drawer stays keyboard-operable under scanner-guard). UX-DR8 modals keep the guard's stricter behaviour.
- The drawer does not forward a scanner burst to `#scan-field` (the 9-17 panel did); the burst is dropped, as for every modal (AC4).
- The reduced-motion E2E compares durations numerically: a global reduced-motion rule leaves `1e-05s`, not `0s`.

**Gates.** `cargo check` ✓; `cargo clippy --all-targets -- -D warnings` ✓; `cargo test --test locale_parity` ✓ (3); full Rust suite with DB: 2445 passed, 1 failed (`navbar_role_visibility::aria_current_renders_on_matching_page` — my over-strict whole-body count, the admin tabs carry their own `aria-current`; fixed, re-run 7/7); `tsc --noEmit` ✓; `waitForTimeout` gate ✓; full Playwright `CI=true --workers=2` on a freshly reset stack: 439 passed, 2 skipped, 0 flaky, 0 failed.

### Review Findings

Independent review of `a566f34`: no Medium+ finding. Lows applied on the same branch:

1. **+ 2. Synthetic activation in the open drawer** — `nav.js` called `preventDefault()` + `click()` on a human-paced Enter / Space, which left the language `<select>` unusable from the keyboard (scanner-guard blocked Space / Enter / type-ahead) and risked a double activation on Space (synthetic click + native keyup activation, e.g. Firefox). Fixed: the window-capture listener now only calls `stopPropagation()` (no `preventDefault`) for a human-paced key on a drawer link (Enter), button (Enter / Space) or the select (Enter / Space / printable type-ahead), so the guard never sees it and the browser's own default runs. Burst keys still reach the guard. Documented limit: the first key of a burst has no predecessor; on the select it could only match an option's first letter (F / E / D / I), which no ISBN / V- / L-code starts with. E2E: Space on the drawer theme toggle flips the theme exactly once; Enter on the close button closes and returns focus; type-ahead in the drawer language select changes the language.
3. **Burst threshold** — the admin setting `scanner_burst_threshold_ms` is now exposed once as `<body data-scanner-threshold>`: `BaseContextFields.scanner_burst_threshold_ms`, filled through a new `utils::PageSettings { session_timeout_secs, scanner_burst_threshold_ms }` read in one guard by `AppState::page_settings()`; `base_context` takes `PageSettings` instead of the bare timeout (all handlers switched; the login page, which has no `AppState`, passes timeout 0 and the default threshold). `nav.js` reads the body attribute (fallback 50, floor 10). Home's `#search-field[data-scanner-threshold="100"]` is left as is: it tunes `search.js`'s own search-vs-scan heuristic (default 100 there), a different contract — not changed in this story. Unit tests: `base_context_carries_session_role_and_csrf_token` checks the threshold; `nav_sticky_tests` checks the body attribute and that nav.js reads it.
4. **Toast / overlay behind the open drawer** — a `MutationObserver` in `nav.js` closes the drawer when `#session-timeout-toast` appears or `#connection-lost-overlay` loses `hidden`; focus returns to the menu button only if it was still inside the drawer. E2E: the overlay case (in the stacking test) and a session-toast case (a stand-in element; the real timer is minutes long).
5. **Android Back does not close the drawer** — accepted: by design the drawer pushes no history entry, so Back leaves the page, as it did with the 9-17 panel.
6. **Burst test could pass vacuously** — positive control added: after the bursts and Escape, a real scan of `specIsbn("NH", 1)` must produce exactly one new answer (skeleton or entry) and no navigation, so the earlier "no new answer" assertion is meaningful.
7. **Print** — the scroll lock is now `@media screen and (width < 1024px)` (unit test pins it).

Gates after the fix round: `cargo check` ✓; `cargo clippy --all-targets -- -D warnings` ✓; `cargo test --lib` (nav_sticky, utils, templates_audit: 124) ✓; full Rust suite with DB: 2446 passed, 0 failed (`locale_parity` included); `tsc --noEmit` ✓; `waitForTimeout` gate ✓; full Playwright `CI=true --workers=2` on a fresh stack: 442 passed, 2 skipped, 0 flaky, 0 failed.
