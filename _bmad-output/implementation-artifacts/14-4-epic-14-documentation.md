# Story 14.4: Documentation

Status: review

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

## Completion Notes (2026-11-04)

**AC1 — chapter 3.** `sec:navbar` (label kept, so the 08 cross-reference holds) becomes "The sidebar" / « La barre latérale »: from 1024 px a left sidebar, groups (Home · Catalog, Locations, Series · Borrowers, Loans, Wish list, Labels, Proposals for Librarian+ · Admin), current page in bold with its bar, Proposals badge (`99+`); foot = language, theme, Log out / Log in, then the Collapse menu button (icons only, name in a tooltip on hover or Tab, badge as a pill), state remembered per browser, not per account; below 1024 px the slim bar with the menu button and the drawer (same entries and foot, never collapsed; closed by Escape, scrim, close button or a link; page behind neither scrolls nor takes the keyboard; a scanner read is ignored). Paragraph on the theme: light/dark for the whole interface, remembered by the browser, system setting until a choice is made, printing always light. FR in the manual's own terms (« barre latérale », « tiroir », « douchette », « Réduire le menu »).

**AC2 — other passages.** ch. 2 (language selector at the foot of the sidebar, in the menu on a small screen), ch. 3 Wish list (entry of the sidebar), ch. 11 (click **Admin** — the real entry label, the old text said "Administration" — in the sidebar), ch. 13 (Proposals entry in the sidebar). ch. 8 What's-new entries untouched.

**AC3 — screenshots.** No capture tooling exists in the repo: the old images were production screenshots from the NAS (commit c4a2577), in French only, shared with README and the website. Retaken on the E2E stack (port 18080) with a throw-away Playwright script (not committed): admin, 1280 × 860, device scale 2, light, sidebar expanded; seeded with locations and six titles from the mock provider, shelved. New files, one per language: `docs/screenshots/01-home-desktop-{en,fr}.png` (ch. 3) and `04-locations-desktop-{en,fr}.png` (ch. 9 — it also showed the old top bar; FR shot taken on a separately seeded stack with French location names). Captions rewritten (no longer "live install"). The old `01-home-desktop.png` / `04-locations-desktop.png` are left in place: README and `website/` still use them (release-doc territory, Rule #19). No drawer shot: neither chapter showed a mobile view. Stack taken down with `down -v`.

**AC4 — PDFs.** `mem-guard docs/manual/build.sh en` / `fr`: exit 0, no undefined reference (only the pre-existing `LatinModernMono/b/n` font-shape warning), no overfull box in the edited passages (all remaining ones are in untouched paragraphs). Both PDFs committed.

**AC5 — CLAUDE.md.** New "Navigation shell (epic 14, #547)" Key Pattern; the Templates note (nav in `sidebar.html`, slim bar in `nav_bar.html`, `nav_groups` in the base context) and the CSRF note (logout form at the foot of the sidebar) corrected. No `xl` / `#mobile-nav` / top-bar note was left elsewhere.

**Remaining intentional grep hits:** `en/05-metadata-providers.tex:175` and `fr/05-metadata-providers.tex:194` — the hamburger menu of the **Google Cloud console** (steps to create a Books API key), not mybibli's navigation.

**Gates.** grep gate as above; `cargo test --lib templates_audit`: 20 passed.

## File List

- `CLAUDE.md`
- `docs/manual/{en,fr}/02-configuration.tex`, `03-usage.tex`, `09-best-practices.tex`, `11-api-integrations.tex`, `13-agents-proposals.tex`
- `docs/manual/mybibli-manual-{en,fr}.pdf`
- `docs/screenshots/01-home-desktop-{en,fr}.png`, `docs/screenshots/04-locations-desktop-{en,fr}.png` (new)
- `_bmad-output/implementation-artifacts/14-4-epic-14-documentation.md`
