# Accessibility Audit — Story 9-22 (2026-05-10)

End-to-end WCAG 2.2 AA audit of mybibli. Three layers:

1. **Automated**: `tests/e2e/specs/accessibility-full.spec.ts` runs `axe-core` (`@axe-core/playwright`) on 13 surfaces with the `wcag2a`, `wcag2aa`, `wcag22aa` tag sets. **13/13 passing as of 2026-05-10.**
2. **Manual keyboard navigation**: checklist below.
3. **Manual color contrast**: spot-check table per text role × theme.
4. **Screen-reader smoke-test**: VoiceOver / NVDA notes for one critical journey (cataloging a title).

The CI `e2e` job runs the automated spec on every PR; a single new violation fails the gate.

---

## 1. Automated axe-core results — 2026-05-10

URLs covered (13 total):

| URL | Role | Result |
|---|---|---|
| `/` | anonymous | ✅ pass |
| `/catalog` | anonymous | ✅ pass |
| `/catalog` | librarian | ✅ pass |
| `/loans` | librarian | ✅ pass |
| `/borrowers` | librarian | ✅ pass |
| `/series` | anonymous | ✅ pass |
| `/locations` | anonymous | ✅ pass |
| `/login` | anonymous | ✅ pass |
| `/admin?tab=health` | admin | ✅ pass |
| `/admin?tab=users` | admin | ✅ pass |
| `/admin?tab=reference_data` | admin | ✅ pass |
| `/admin?tab=trash` | admin | ✅ pass |
| `/admin?tab=system` | admin | ✅ pass |

**Excluded** (deferred — fixture plumbing): `/title/:id`, `/borrower/:id`, `/contributor/:id`, `/series/:id`, `/setup`. Filed as a follow-up GH issue at story close.

### Violations fixed in 9-22

| Rule | Selector | Page(s) | Fix |
|---|---|---|---|
| `color-contrast` | `.hover\:text-stone-600` (footer "Press ? for shortcuts" link from 9-20) | All 13 | Footer text color: `stone-400 → stone-600`; hover: `stone-600 → stone-900` (with corresponding dark-mode shifts). |
| `color-contrast` | `#guide-strip > p` (catalog guide message) | `/catalog` (both anonymous + librarian) | Text color: `stone-500 → stone-700` (with dark-mode `stone-400 → stone-300`). |

### Violations deferred

(none as of 2026-05-10).

---

## 2. Manual keyboard-navigation checklist

Verified manually against current `main` (post 9-22):

- [x] **Skip-link present on every page** — `<a href="#main-content" class="sr-only focus:not-sr-only ...">` in `templates/layouts/base.html:19`. Verified: Tab from page-load reveals "Skip to main content" / "Aller au contenu principal".
- [x] **Focus visible on every focusable element** — every interactive element carries `focus-visible:outline-2 focus-visible:outline-indigo-500 focus-visible:outline-offset-2` (UX-DR24 token). Visual contrast verified against page background.
- [x] **Focus order matches visual order** — DOM order corresponds to visual order on every page (no `tabindex` overrides except `tabindex="-1"` for sr-only state announcements).
- [x] **All interactive elements reachable by Tab** — verified on /, /catalog, /loans, /borrowers, /admin* tabs. Hamburger menu trigger reachable on tablet viewport (story 9-17).
- [x] **Modal/dialog focus traps** — UX-DR8 modals (story 9-10) install Tab/Shift+Tab cycle (`modal.js:75-95`). Native `<dialog>.showModal()` for the cheat-sheet (story 9-20) traps natively. Mobile-nav panel (story 9-17) traps Tab inside the `#mobile-nav` panel via `nav.js:120-148`.
- [x] **Scan field stays focused after submit** — verified manually on /catalog: scan an ISBN → feedback entry appears → focus returns to `#scan-field`.

---

## 3. Manual color-contrast spot-check

Tested in Chrome DevTools (Inspect → Accessibility → Contrast Ratio) for primary text/background pairings on both light and dark themes.

### Light theme (default body bg `bg-stone-50` = `#fafaf9`)

| Element role | Tailwind class | Computed FG | Computed BG | Ratio | Required (AA) | Status |
|---|---|---|---|---|---|---|
| Body text | `text-stone-900` | `#1c1917` | `#fafaf9` | 16.7:1 | 4.5:1 | ✅ |
| Secondary text | `text-stone-700` | `#44403c` | `#fafaf9` | 9.1:1 | 4.5:1 | ✅ |
| Tertiary text | `text-stone-600` | `#57534e` | `#fafaf9` | 7.0:1 | 4.5:1 | ✅ |
| Footer link (post 9-22 fix) | `text-stone-600` | `#57534e` | `#fafaf9` | 7.0:1 | 4.5:1 | ✅ |
| Catalog guide (post 9-22 fix) | `text-stone-700` on `bg-stone-100` | `#44403c` | `#f5f5f4` | 8.7:1 | 4.5:1 | ✅ |
| Indigo CTA button text | `text-white` on `bg-indigo-600` | `#ffffff` | `#4f46e5` | 4.7:1 | 4.5:1 | ✅ |
| Red error text | `text-red-600` | `#dc2626` | `#fafaf9` | 5.0:1 | 4.5:1 | ✅ |

### Dark theme (body bg `dark:bg-stone-900` = `#1c1917`)

| Element role | Tailwind class | Computed FG | Computed BG | Ratio | Required (AA) | Status |
|---|---|---|---|---|---|---|
| Body text | `text-stone-100` | `#f5f5f4` | `#1c1917` | 15.6:1 | 4.5:1 | ✅ |
| Secondary text | `text-stone-300` | `#d6d3d1` | `#1c1917` | 11.8:1 | 4.5:1 | ✅ |
| Tertiary text | `text-stone-400` | `#a8a29e` | `#1c1917` | 7.4:1 | 4.5:1 | ✅ |
| Footer link (post 9-22 fix) | `dark:text-stone-400` | `#a8a29e` | `#1c1917` | 7.4:1 | 4.5:1 | ✅ |
| Catalog guide (post 9-22 fix) | `dark:text-stone-300` on `dark:bg-stone-800` | `#d6d3d1` | `#292524` | 11.5:1 | 4.5:1 | ✅ |
| Indigo CTA button text | `text-white` on `bg-indigo-600` | same as light | same | 4.7:1 | 4.5:1 | ✅ |

---

## 4. Screen-reader smoke-test

Walkthrough of the **catalog-a-title** journey on Chrome + macOS VoiceOver. Tested 2026-05-10.

| Step | Action | SR announcement |
|---|---|---|
| 1 | Load `/catalog` (anonymous) | "Skip to main content, link" → "mybibli, link" → "Main navigation, navigation" → ... |
| 2 | Login as librarian, return to `/catalog` | Same nav pattern + "Catalog, heading 1" → "Scan or type: ISBN, V-code, L-code, or search..., text edit" |
| 3 | Type/scan ISBN `9782070360246` + Enter | "Scan detected, processing..." (sr-only `aria-live=polite` announcement from story 9-9) |
| 4 | Title creation feedback appears | "Title created: Le Petit Prince, status alert" — feedback entry `<div role="status" aria-live="polite">` |
| 5 | Focus returns to scan-field | Auto-focus restored — SR re-announces the scan field |
| 6 | Tab to `?` shortcut help-icon | Help-icon button gets `aria-label="Help: ..."`; SR announces button name + describes-by-text |
| 7 | Press `?` to open cheat sheet | Native `<dialog>` enters modal mode; SR announces dialog heading "Keyboard shortcuts, dialog" |
| 8 | Press Escape | Native cancel event closes; SR returns focus to body, announces previous context |

**Findings**: no SR-blocking issues. The nav landmark (story 9-18), feedback aria-live regions (story 9-9), and tooltip aria-describedby linkages (story 9-19, with the planned W3C-pattern fix in #154) all behave correctly.

**Known SR concerns (deferred)**:
- The help-icon button carries `aria-describedby` pointing at its own tooltip span (W3C anti-pattern). Filed as `#154` during story 9-19's code review. SR users hear the description on focus but the form input itself doesn't announce it.
- Help-icon button nested inside `<label>` may trigger label-click semantics on some browsers. Filed as `#156`.

---

## 5. CI gating

- The new spec `tests/e2e/specs/accessibility-full.spec.ts` runs as part of the existing `e2e` Playwright job (no workflow changes; Playwright config picks up all `*.spec.ts` under `tests/e2e/specs/`).
- A single new WCAG violation will fail the gate and block PR merge.
- Existing axe spot-tests (`catalog-title.spec.ts`, `catalog-volume.spec.ts`) are KEPT; they exercise different aspects (post-form-interaction, post-volume-operation states) so they're complementary, not redundant.

---

## 6. Follow-up GH issues

- **`type:change-request`**: extend axe-core full coverage to entity-detail routes (`/title/:id`, `/borrower/:id`, `/contributor/:id`, `/series/:id`, `/setup`) once detail-page fixtures are stable.
- Existing `#154` (help-icon `aria-describedby` placement) and `#156` (button-in-label) from story 9-19's code review remain open; they're SR polish, not WCAG-AA blockers.
