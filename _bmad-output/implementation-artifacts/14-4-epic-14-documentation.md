# Story 14.4: Documentation

Status: ready-for-dev

GitHub issue: #547. Covers: UX-DR38, UX-DR39 (documentation). Epic 14; ships in v1.24.0.

## Story

As an operator,
I want the manual to describe the new navigation,
so that it matches what users see.

## Code map (2026-11-04)

Passages that describe the old top bar / hamburger (EN and FR mirror each other):
- `docs/manual/{en,fr}/03-usage.tex` § "The navigation bar" / « La barre de navigation » (`sec:navbar`, l. 9–30) + the screenshot `01-home-desktop.png` (l. 24); l. 565 / 601 mention the navigation bar.
- `docs/manual/{en,fr}/02-configuration.tex` l. 357 / 390 (language choice "in the navigation bar"; theme toggle).
- `docs/manual/{en,fr}/05-metadata-providers.tex` l. 175 / 194 ("Open the hamburger menu (top-left…)").
- `docs/manual/en/11-api-integrations.tex` l. 43 (+ FR twin) ("Click Administration in the navigation bar").
- `docs/manual/en/13-agents-proposals.tex` l. 412 (+ FR twin) (Proposals entry in the navigation bar).
- `docs/manual/{en,fr}/08-upgrade-migration.tex`: historical "What's new" entries (#496, Labels) stay as they are — they describe past releases.
- Screenshots: find how existing ones were produced (grep the repo for the screenshot script / Playwright capture, e.g. `scripts/` or `docs/manual/*/images`).

## Acceptance Criteria

1. **AC1 — usage chapter.** `sec:navbar` becomes "The sidebar" / « La barre latérale » (keep the label `sec:navbar` so cross-references hold, or update every `\ref`): from 1024 px a left sidebar, expanded (icon + label) or collapsed (icons, tooltip), the toggle at its foot, the state remembered by the browser; groups; language, theme, log out at the foot; the Proposals badge; below 1024 px the menu button opens the same navigation as a drawer (Escape, scrim, close button). The theme toggle now really switches light/dark and is remembered by the browser; printing is always light.
2. **AC2 — other passages.** Every other passage listed in the code map says "sidebar" (or "menu" on small screens) instead of the navigation bar / hamburger; historical What's-new entries untouched.
3. **AC3 — screenshots.** Screenshots that show the old top bar in the chapters above are retaken (expanded sidebar at 1280 px; one of the drawer at 390 px if the chapter shows mobile), same tooling as before; the EN and FR manuals use their own language.
4. **AC4 — PDFs.** Both PDFs rebuilt (`mem-guard` for LaTeX — see the repo's manual build command) and committed; no LaTeX errors, no undefined references.
5. **AC5 — CLAUDE.md.** A "Navigation shell (epic 14)" pattern: single source (`utils/nav.rs::nav_groups` → `nav_entries.html` + `nav_macros.html`), one `<nav id="sidebar">` = sidebar ≥ lg / drawer < lg, `sidebar.js` pre-paint state (`localStorage` key, `sb-collapsed` variant), `nav.js` drawer (inert `#page-column`, dialog role only while open, human-paced keys bypass scanner-guard, burst threshold from `<body data-scanner-threshold>` / `PageSettings`), class-based screen-only dark mode (`@custom-variant dark`), `@source` scanning `static/js` (vendor excluded), z-index rule. Remove/adjust stale notes about the top bar, `xl` stopgap, `#mobile-nav`. Keep it concise.
6. **AC6 — release docs** (Foundation Rule #19, v1.24.0) are NOT part of this story.
