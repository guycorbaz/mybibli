# Story 14.2: Navigation drawer on small screens

Status: ready-for-dev

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

- [ ] T1 — drawer markup from the single source; slim bar (AC1, AC2)
- [ ] T2 — drawer JS: open/close, focus trap / inert, scroll lock, reduced motion (AC3)
- [ ] T3 — scanner-guard + stacking (AC4)
- [ ] T4 — remove old panel; landmarks; comments (AC5)
- [ ] T5 — tests; gates; full suites once before the single push (AC6, AC7)

## Dev Notes

- Reuse, do not fork: the sidebar and the drawer may even be the same `<nav>` element restyled per breakpoint (sidebar ≥ `lg`, off-canvas drawer < `lg`) — evaluate this first; it removes the second landmark and keeps one DOM.
- CSP: no inline script/style; animations via classes. i18n: four locales, `locale_parity`.
- Rule #12: files < 2000 lines.
