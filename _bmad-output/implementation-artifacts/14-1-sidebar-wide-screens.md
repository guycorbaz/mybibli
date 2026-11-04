# Story 14.1: Sidebar on wide screens

Status: ready-for-dev

GitHub issue: #547 (type:change-request). Covers: UX-DR38. Epic 14; ships in v1.24.0.

## Story

As a librarian working on a computer,
I want the navigation in a left sidebar I can collapse,
so that every entry fits whatever the language and the wide tables get their width back.

## Reference

The project lead approved the interactive mock-up `_bmad-output/planning-artifacts/ux-sidebar-mockup.html` on 2026-11-04 (open it in a browser: language / state / width / theme switches). It is the visual spec: widths (224 px expanded, 64 px collapsed), groups and order, foot contents, badge placement, tooltips, current-page marker. Its icon paths are Heroicons v2 outline (MIT) and may be reused as-is. Decisions (see `epics.md` § Epic 14): state per browser in `localStorage`, starts expanded; new **Home** entry; drawer below `lg` is Story 14.2 — **not this story**.

## Existing-code reality check (code map, 2026-11-04)

| Place | Today |
|---|---|
| `templates/components/nav_bar.html` (157 l.) | sticky `<header>` with the horizontal bar (`hidden xl:flex` links), right group (language form, `#theme-toggle`, logout form), hamburger `#mobile-menu-toggle` (`xl:hidden`), and the `#mobile-nav` panel with a **hand-copied** second list of entries. Comments document 11-4 (sticky, z-40), 11-21 (`xl` switch, D-11-21-1, badge + aria-label), #150, #545. |
| `templates/layouts/base.html` | includes `nav_bar.html` before `<main id="main-content" class="scroll-mt-12 md:scroll-mt-14 **:scroll-mt-12 md:**:scroll-mt-14">` (11-4: scroll margin = sticky bar height, D-11-4-8 explains why not root scroll-padding). `theme.js` already loads in `<head>` (pattern for a pre-paint script). |
| `static/js/nav.js` (227 l.) | hamburger open/close, Escape, focus return for `#mobile-nav`. |
| `static/js/theme.js` | theme toggle bound to `#theme-toggle`; dark class applied pre-paint. |
| `static/js/tooltip.js`, `templates/components/tooltip.html` | existing tooltip component (CSP-clean) — check whether it fits collapsed-entry tooltips before writing a new one. |
| `src/utils.rs` ~l. 560–700 | `BaseContext` nav fields (`nav_*`, `nav_proposals_badge`, `nav_proposals_aria`, `current_page`, `role`, `lang`, `csrf_token`, `current_url`). |
| `src/routes/nav_sticky_tests.rs` (314 l.) | unit tests on the sticky header / scroll margins (11-4). |
| `src/templates_audit.rs::fixed_surfaces_stack_above_sticky_nav` (~l. 1080) | every `fixed` surface in templates/JS must carry `z-50` (above the `z-40` sticky nav). |
| E2E | `navbar-role-visibility`, `navbar-hamburger`, `responsive-layouts`, `proposals-access`, `login-smoke`, `cross-cutting`, `borrowers` specs and `helpers/proposals.ts` target the current bar / `#mobile-nav`. |

## Acceptance Criteria

1. **AC1 — one navigation template.** The entries (href, icon, label, role gate, current-page key, badge) are rendered from ONE place (an Askama include and/or a list built in Rust from `BaseContext`) — no second hand-copied list for desktop vs small screens (Rule #1). Story 14.2 will render the same navigation in the drawer.
2. **AC2 — sidebar ≥ `lg` (1024 px).** A full-height left `<nav aria-label="Main navigation">` (keep the existing accessible name unless a test proves a better one) with the mock-up's groups and order for each role: Home · Catalog, Locations, Series · (Librarian+) Borrowers, Loans, Wish list, Labels, Proposals · (Admin) Admin. Icon + label; current page `aria-current="page"` with a non-colour marker (left bar + weight). Proposals keeps its `aria-label` with the exact count and its text badge (`utils::proposals_badge_text`). Separators are presentational (no empty landmark). The logo + "mybibli" link home at the top.
3. **AC3 — foot.** Language select (one form, CSRF hidden input, `data-auto-submit`, noscript OK), theme toggle (keeps `theme.js` working; accessible name), Log out (POST form, CSRF) — or Log in for anonymous — and the collapse toggle (`aria-expanded`, `aria-controls`, localized name "Collapse menu" / "Expand menu").
4. **AC4 — collapsed state.** 64 px; icons only; every entry keeps its accessible name; a tooltip on hover AND keyboard focus (CSP-clean, no inline style); the Proposals badge becomes a pill on the icon (still text). Language select stays usable (compact). Expanded is 224 px; no label overflows at 1024/1280/1440 px in fr/en/de/it with an admin's full set.
5. **AC5 — persistence without flash.** State in `localStorage` (key e.g. `mybibli.sidebar`), every access in try/catch (blocked storage → expanded). Applied before first paint by an external script in `<head>` (like `theme.js`) that sets a class on `<html>`; CSS does the rest. Starts expanded. No inline script / style (CSP, `templates_audit`).
6. **AC6 — layout.** At ≥ `lg` there is no top bar: page content sits right of the sidebar and uses the remaining width; the sidebar stays in view while scrolling (sticky/fixed full height, own vertical scroll if taller than the viewport). Scroll margins of 11-4 become 0 at ≥ `lg` (no bar to clear) and stay as today below `lg`; keep `nav_sticky_tests` and the D-11-4-8 reasoning consistent. Modals, toasts, connection-lost overlay stay above (`z-50` rule kept/updated). `print:hidden` for the sidebar.
7. **AC7 — below `lg` unchanged in behaviour.** The current top bar + hamburger panel keep working below 1024 px; their switch moves from `xl` to `lg` (the bar never shows at ≥ `lg`). It may already render from the AC1 single source. Story 14.2 replaces it.
8. **AC8 — `/catalog`.** Toggling the sidebar does not steal or drop focus from the scan workflow in a harmful way (after a toggle, focus stays on the toggle; the next scan still lands in `#scan-field` per existing focus rules); feedback entries are untouched; scanner-guard invariant unchanged.
9. **AC9 — i18n.** New keys (`nav.home`, collapse / expand names, theme toggle name if not already localized — today it is a hard-coded English `aria-label="Toggle theme"`; `nav.theme_toggle` exists) in the four locale files; `cargo test --test locale_parity`.
10. **AC10 — tests.** Unit: rendering per role (entries present/absent, order, aria-current, badge/aria-label), audit tests green. E2E (new spec, unique specId — check `tests/e2e/specs` for free ids): collapse/expand, persistence across reload, tooltip on keyboard focus, badge in both states, every entry reachable by Tab, no overflow at 1024/1280/1440 in four locales (admin), axe in both states light/dark. Existing specs that target the old bar at ≥ `lg` adapted (selectors by role/name, not by old ids where possible).

## Tasks / Subtasks

- [ ] T1 — single navigation source + icons include (AC1, AC2)
- [ ] T2 — sidebar layout in `base.html`, foot, collapsed state CSS (AC2–AC4, AC6)
- [ ] T3 — pre-paint state script + toggle JS (AC5, AC8)
- [ ] T4 — below-`lg` bar/panel switch to `lg` (AC7)
- [ ] T5 — locales (AC9)
- [ ] T6 — tests; adapt existing specs; gates; full suites once before the single push (AC10)

## Dev Notes

- Tailwind v4: after template/class changes rebuild `static/css/output.css` the way the repo does it (check `package.json` / CI / CLAUDE.md); a class only used in Rust strings or JS must be visible to the Tailwind scanner.
- Keep `HtmxResponse` OOB targets that live in the nav working (grep `data-proposals-badge`, any OOB that updates the badge).
- Session-timeout, connection monitor, shortcuts (`static/js/shortcuts.js`) may reference nav elements — grep before renaming ids.
- Foundation Rule #12: keep every touched file under 2000 lines.
