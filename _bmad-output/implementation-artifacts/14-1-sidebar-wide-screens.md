# Story 14.1: Sidebar on wide screens

Status: review

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

- [x] T1 — single navigation source + icons include (AC1, AC2)
- [x] T2 — sidebar layout in `base.html`, foot, collapsed state CSS (AC2–AC4, AC6)
- [x] T3 — pre-paint state script + toggle JS (AC5, AC8)
- [x] T4 — below-`lg` bar/panel switch to `lg` (AC7)
- [x] T5 — locales (AC9)
- [x] T6 — tests; adapt existing specs; gates; full suites once before the single push (AC10)

## Dev Notes

- Tailwind v4: after template/class changes rebuild `static/css/output.css` the way the repo does it (check `package.json` / CI / CLAUDE.md); a class only used in Rust strings or JS must be visible to the Tailwind scanner.
- Keep `HtmxResponse` OOB targets that live in the nav working (grep `data-proposals-badge`, any OOB that updates the badge).
- Session-timeout, connection monitor, shortcuts (`static/js/shortcuts.js`) may reference nav elements — grep before renaming ids.
- Foundation Rule #12: keep every touched file under 2000 lines.

## Completion Notes (2026-11-04)

**Single navigation source (AC1).** `src/utils/nav.rs` builds the entries once — `nav_groups(role, current_page, locale, pending_proposals) -> Vec<Vec<NavEntry>>` (key/icon, href, label, `current`, Proposals `badge` + `aria_label`, `tooltip()`), groups as in the mock-up, role gates in Rust (unknown role fails closed to anonymous), empty groups never emitted. `BaseContextFields.nav_groups` replaces the nine `nav_<entry>` / `nav_proposals_*` fields; new `nav_collapse`, `nav_expand`, `nav_theme_toggle`, `nav_language_label`. `templates/components/nav_entries.html` is the only markup for the entries; it is included by the sidebar and by the small-screen panel (`#mobile-nav`) — the unit test `sidebar_and_panel_render_the_same_entries` pins identical markup. Story 14-2's drawer can include `nav_entries.html` + reuse `nav_macros.html` as is. `templates/components/nav_macros.html`: `icon(name)` (Heroicons v2 outline paths from the mock-up, + `login`), `item_class`, `idle_class`, `current_class`, `tooltip(text)`.

**Sidebar (AC2–AC4, AC6).** `templates/components/sidebar.html`: `<nav aria-label="Main navigation" id="sidebar">`, `hidden lg:flex`, `sticky top-0 self-start h-screen`, `w-56` / `sb-collapsed:w-16`, `z-40`, `print:hidden`; logo link on top; entries in their own `overflow-y-auto` scroller; foot = language form (named, CSRF, `data-auto-submit`, noscript, `#lang-select`), theme toggle (`#theme-toggle`, localized text name), Log out POST form (CSRF) or Log in, collapse toggle (`data-sidebar-toggle`, `aria-controls="sidebar"`, `aria-expanded`, CSS-driven name "Collapse menu"/"Expand menu"). Current page: `aria-current="page"` + bold + 3 px left bar (`before:`). Collapsed: labels `sb-collapsed:sr-only` (accessible names kept), tooltip spans `aria-hidden`, shown on `group-hover` and `group-focus-visible`; they are `fixed left-18 z-50` with only `left` set, so they keep their static vertical position and escape the entries scroller's clipping (an `absolute` tooltip would be cut at 64 px). Badge becomes a pill on the icon. `layouts/base.html`: a `lg:flex` row (sidebar + `lg:flex-1 lg:min-w-0` column holding the header, `<main>` and the footer); `<main>` gains `lg:scroll-mt-0 lg:**:scroll-mt-0` (no bar above the content from `lg`; D-11-4-8 unchanged — still no root scroll-padding).

**State (AC5, AC8).** `static/js/sidebar.js`, loaded in `<head>` after `theme.js`: reads `localStorage["mybibli.sidebar"]` ("collapsed"/"expanded", try/catch → expanded) and sets `sidebar-collapsed` on `<html>` before first paint; the custom variant `sb-collapsed` (`static/css/input.css`, `@media (width >= 1024px)` + `:where(.sidebar-collapsed *)`) does the rest. On DOMContentLoaded it syncs `aria-expanded` and wires the toggle. AC8 (as revised in review): after every toggle, mouse or keyboard, a page with `#scan-field` (/catalog) gets the focus back in the scan field, so the next scan lands there and a scanner's Enter can never re-toggle the sidebar; elsewhere focus stays on the toggle from the keyboard, and a mouse click does not take focus (`mousedown` default prevented). Feedback entries untouched.

**Below `lg` (AC7).** `components/nav_bar.html` is now the small-screen header only (`lg:hidden`): no link strip, no bar logout (it was `hidden xl:contents`), hamburger `lg:hidden`, panel `hidden lg:hidden`; the panel's entries come from `nav_entries.html` (they now carry icons and the Home entry). Ids suffixed `-bar` (`#lang-select-bar`, `#theme-toggle-bar`); the sidebar holds `#lang-select` / `#theme-toggle`. `theme.js` wires every `[data-theme-toggle]` and no longer overwrites the name with English "Switch to … mode".

**i18n (AC9).** `nav.home`, `nav.collapse_menu`, `nav.expand_menu`, `nav.language_label` in en/fr/de/it (mock-up wording); the theme toggle uses the existing `nav.theme_toggle`.

**Tests (AC10).** Unit: `src/utils/nav.rs` (7 tests: order per role, fail-closed, current page, localization, badge/aria/tooltip); `src/routes/nav_sticky_tests.rs` rewritten (20 render tests: sidebar shell classes, entries per role + separators, single source, current marker, foot, collapsed names/tooltips/pill, head script, small bar `lg` switch, logout/login placement, Proposals badge, language-form landmarks, localized theme name); `utils.rs` base-context tests adapted; `templates_audit.rs` updated (sidebar exempt from the inner-scroller rule like the panel; `lg:` scroll-margin resets allowed on `<main>`; base-context tell moved to `nav_logout`; z-50 doc mentions the sidebar). Integration: `tests/navbar_role_visibility.rs` (landmark test: one per breakpoint), `navbar_hamburger.rs` (`lg:hidden`), `proposals_access.rs` (badge texts helper). E2E: new `tests/e2e/specs/journeys/sidebar.spec.ts` (spec ID "SD", 11 tests: collapse/expand + persistence + first-frame width, blocked storage, tooltips on hover and keyboard focus, Tab reachability in both states, current marker, no overflow at 1024/1280/1440 × fr/en/de/it with a forced `99+` badge, axe expanded/collapsed × light/dark, /catalog focus + feedback, badge in both states via a real MCP proposal); adapted `responsive-layouts`, `navbar-hamburger`, `navbar-role-visibility`, `cross-cutting`, `login-smoke`, `proposals-access`, `series-epic11-smoke`, `helpers/proposals.ts` (`NAV = "nav#sidebar"`; landmark locators via `getByRole("navigation", { name: "Main navigation" })`, which only resolves the displayed one).

**Deviations / found on the way.**
- Two `<nav aria-label="Main navigation">` exist in the markup (sidebar ≥ lg, small bar < lg), never displayed together; 14-2 removes the bar.
- The mock-up's "Signed in: guy (Admin)" line in the foot is not implemented (not in the ACs; `BaseContext` carries no user name).
- Theme toggle name is the constant "Toggle theme" (localized) rather than the mock-up's state-dependent "Dark theme"/"Light theme".
- Pre-existing bug fixed: the Docker CSS stage only scanned `templates/`, so utilities used only in JS class strings (`bottom-4 right-4` of the session / connection toasts) were missing from the shipped `output.css`; the toasts sat at their static position. The sidebar makes the page ≥ 100vh, which pushed the session toast below the fold (`session-inactivity-timeout.spec.ts` failed). Fix: `Dockerfile` copies `static/js/` into the CSS stage and `input.css` declares `@source "../js"` (htmx.min.js excluded).
- `dark:` utilities follow `prefers-color-scheme` (Tailwind default), not the `.dark` class the theme toggle sets — pre-existing, unchanged; axe dark runs use `emulateMedia({ colorScheme: "dark" })`.

**Gates.** `cargo check` ✓; `cargo clippy --all-targets -- -D warnings` ✓; `cargo test --test locale_parity` ✓ (3); full Rust suite with DB: 2441 passed, 0 failed; `tsc --noEmit` ✓; `waitForTimeout` gate ✓; full Playwright `CI=true --workers=2` on a reset stack: 421 passed, 2 skipped, 0 flaky, 0 failed.

### Review Findings

Independent review of `ad591a2`: no Medium+ finding. Fix round applied:

1. **Blocked-storage E2E filter could never match** (`/sidebar/i` vs the DOMException message "blocked") — fixed: the test now asserts no page error at all. It exposed a pre-existing bug: `theme.js` read/wrote `localStorage` unguarded, so with blocked storage it threw before wiring its toggle. Fixed: `readStored` / `writeStored` in try/catch (blocked → OS preference, toggle works for the page); the E2E toggles the theme under blocked storage.
2. **Theme toggle did not switch dark mode** (pre-existing, also on `main`): `dark:` compiled to `prefers-color-scheme`, only `browse.css` read the `.dark` class. Fixed: `@custom-variant dark (&:where(.dark, .dark *));` in `input.css` (theme.js applies the OS preference when nothing is stored, so OS-dark users keep dark). Media-based leftovers aligned: `browse.css` media twin removed (it kept the cards dark under a user-chosen light theme), the dark placeholder rule now `.dark ::placeholder`, `home-stats-donut.js` follows the `<html>` class (MutationObserver) instead of the OS query. `wishlist_print.html` (no theme.js, one `dark:` class) now always prints light — intended. axe dark runs still get dark through theme.js's OS fallback (`emulateMedia`). Regression E2E: "the theme toggle switches the page to dark and back, remembered across a reload" (light OS preference, body and sidebar background change, persists).
3. **DRY** — the language `<select>` (label, options, noscript) and the theme toggle are macros in `nav_macros.html` (`lang_select(id, lang, label, class)`, `theme_toggle(id, name, row)`), used by the sidebar, the small bar and the panel; unit test `language_select_and_theme_toggle_come_from_the_macros`. The bar's toggle now uses the same Heroicons sun/moon.
4. **Collapse toggle name flipped** — fixed: one fixed accessible name "Collapse menu" (visible when expanded, sr-only when collapsed); `aria-expanded` carries the state; "Expand menu" stays only in the aria-hidden collapsed tooltip. Unit + E2E updated.
5. **Keyboard toggle on /catalog left focus on the toggle** — fixed as specified: after any toggle, focus returns to `#scan-field` when the page has one; AC8 note above updated; E2E covers mouse and keyboard (Enter does not produce a scan).
6. `@source not "../js/vendor";` added to `input.css`.
7. `tests/proposals_access.rs` — the `entry_tag` doc paragraph moved back above `fn entry_tag`.
8. **Accepted / dismissed:**
   - Collapsed tooltips are not hoverable/dismissable per WCAG 1.4.13 — dismissed: they are `aria-hidden` duplicates of each row's accessible name (no information only in the tooltip), shown on hover/focus of the row and gone when it leaves; `pointer-events: none`, nothing to hover.
   - No cross-tab sync of the collapsed state — accepted: the state is per browser and applies on the next page load, as decided by the project lead.

9. **Found while gating item 1** — `audio.js` and `browse-mode.js` also read/wrote `localStorage` unguarded (page errors under blocked storage, caught by the stricter E2E). Guarded the same way (blocked → default, choice kept in memory for the page).

Gates after the fix round: `cargo check` ✓; `cargo clippy --all-targets -- -D warnings` ✓; `cargo test --lib` 1663 passed (incl. `templates_audit`, `nav_sticky_tests` 21, `utils::nav` 7); `locale_parity` 3, `proposals_access` 7, `navbar_role_visibility` 6, `navbar_hamburger` 6, `csrf_integration` 17 — all passed; `tsc --noEmit` ✓; full Playwright `CI=true --workers=2` on a fresh stack: 422 passed, 2 skipped, 0 flaky, 0 failed. (A previous full run of the same code had the known #499 test "second shelve removes the earlier undo button" fail its three attempts under load; it passed alone on a fresh stack and in the confirming full run.)
