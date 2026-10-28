# Story 11.4: Sticky navigation bar

Status: review

GitHub issue: #496 (type:change-request). Covers: FR125. Epic 11 — fourth story (production fix bundled in the epic; ships in the release that closes epic 11, not in a patch).

## Story

As a user,
I want the navigation bar to stay visible while I scroll,
so that I never have to scroll back up to reach the menu.

## Existing-code reality check (code map, 2026-10-28)

| Place | Today | Consequence for this story |
|---|---|---|
| `templates/components/nav_bar.html` | `<header>` wraps `<nav aria-label="Main navigation" class="… h-12 md:h-14 … relative">` **and** the mobile panel `<div id="mobile-nav" class="hidden lg:hidden …">` | making the `<header>` sticky carries the mobile panel with it — no second sticky element needed |
| `templates/layouts/base.html` l. 2 | `<html lang=… class="h-full">`; `<body class="… min-h-full">`; header is a direct child of `<body>`; no ancestor has `overflow` | `position: sticky` sticks against the viewport for the whole page (the containing block is `<body>`, as tall as the document) |
| `templates/layouts/base.html` l. 33 | skip link `<a href="#main-content" class="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 …">` | the link itself already stacks above a `z-40` bar; its **target** `<main id="main-content">` would land under the bar → `scroll-padding-top` on `<html>` |
| Overlays | every modal `<dialog open aria-modal="true" class="fixed inset-0 … z-50">` (`components/modal.html` + 6 fragments), promoted to the top layer by `modal.js` / `inline-form.js` (`showModal()`); cheat-sheet `<dialog>` via `showModal()`; `#connection-lost-overlay` `fixed inset-0 z-50`; JS toasts (`session-timeout.js`, `connection-monitor.js`) `fixed … z-50` | a bar at `z-40` stays below every overlay; the top layer is above any `z-index` anyway |
| In-content popovers | saved-searches dropdown `z-30`, tooltip `z-20`, contributor autocomplete / loans `z-10` | all below `z-40` → they scroll **under** the bar, which is the expected behaviour for page content |
| `#feedback-list` (catalog, admin, title detail, home) | in-flow `<div aria-live="polite">` inside `<main>` — **not** an overlay | "renders above the bar" is read as: never covered by the bar when brought into view (scroll-padding), and no overlay is pushed under the bar (see D-11-4-5) |
| `#mobile-nav` | up to 8 links + language form + logout (admin) ≈ 430 px, and 11-21 will add a Proposals entry | once the header is sticky, a panel taller than the viewport (landscape phone, 375 px high) can no longer be scrolled into reach → bound it to the viewport and let it scroll internally |
| `templates/pages/wishlist_print.html` | stand-alone document (does **not** extend `base.html`) — no nav bar at all | the "bar not repeated" AC holds by construction; we add `print:static` on the header for any other page the user prints from the browser |
| In-page anchors | `href="#main-content"` (skip link), `href="#add-form"` (borrowers, intercepted by `borrowers.js`); no `scrollIntoView` / `hx-swap … show:` anywhere | `scroll-padding-top` on the root scroller covers fragment navigation, `focus()` scrolling and any future `scrollIntoView` |
| `Dockerfile` css stage | Tailwind v4 CLI scans `templates/` and regenerates `output.css` (gitignored) | new utility classes in templates are emitted automatically for E2E and prod |

No Rust handler change, no locale key, no JS, no migration.

## Acceptance Criteria

1. **AC1 — sticky.** Given a long page (home, catalog, series, admin), when I scroll to the bottom, then the navigation bar stays at the top of the viewport — at mobile (375), tablet (768) and desktop (1280) widths.
2. **AC2 — skip link.** Given the "Skip to content" link, when it is activated, then its target is not hidden under the bar (48 px below `md`, 56 px from `md`). The epic names `scroll-padding-top`; the shipped mechanism is `scroll-margin-top` on `<main>` and its descendants — same offsets, without the page jump a root padding causes when a bar control takes focus (D-11-4-8).
3. **AC3 — mobile menu.** Given the mobile menu is open while the bar is stuck, when I use it, then it stays usable and above the content; its last entry stays reachable on a short (landscape) viewport.
4. **AC4 — overlays.** Given an open modal, the connection-lost overlay or a toast, they still render above the bar (bar `z-40` < overlays `z-50` / top layer). The in-flow feedback list is never covered by the bar when scrolled into view (AC2 mechanism).
5. **AC5 — print.** Given a printable page, when it is printed, then the bar is not repeated (wishlist print has no bar; other pages: header `position: static` under `@media print`).
6. **AC6 — not compacted.** The navigation is not compacted: heights `h-12 md:h-14`, the `lg` strip/menu switch (CR #443) and every entry are unchanged.
7. **AC7 — tests.** E2E (`responsive-layouts.spec.ts`) asserts the bar is in the viewport after scrolling at the three widths, that the skip link's target is visible below the bar, the stuck mobile menu, an overlay above the bar, and print positioning. Rust render tests pin the classes on the shared layout and the overlay z-order invariant.

## Tasks / Subtasks

- [x] T1 — Layout (AC1, AC5, AC6): `templates/components/nav_bar.html` — `<header class="sticky top-0 z-40 print:static">`; comment citing #496 (z-order rationale). Do not touch the `<nav>` height or the `lg` breakpoint.
- [x] T2 — Anchors (AC2, AC4): `templates/layouts/base.html` — `<main id="main-content" class="scroll-mt-12 md:scroll-mt-14 **:scroll-mt-12 md:**:scroll-mt-14">` (initially `scroll-pt-*` on `<html>`, replaced after re-review — D-11-4-8); comment citing #496.
- [x] T3 — Mobile menu (AC3): `#mobile-nav` gets `max-h-[calc(100vh-48px)] md:max-h-[calc(100vh-56px)] supports-[height:100dvh]:max-h-[calc(100dvh-48px)] md:supports-[height:100dvh]:max-h-[calc(100dvh-56px)] overflow-y-auto overscroll-contain` (bar height subtracted per breakpoint; `vh` fallback, `dvh` where supported).
- [x] T4 — Rust render tests (AC7): new sibling module `src/routes/nav_sticky_tests.rs` (wired `#[cfg(test)]` in `routes/mod.rs`, same pattern as `home_fold_tests.rs`), rendering the home template through `home::tests::make_test_home_template_with_counts`:
  - `<header` opening tag carries `sticky`, `top-0`, `z-40`, `print:static`;
  - `<main id="main-content"` carries `scroll-mt-12`, `md:scroll-mt-14`, `**:scroll-mt-12`, `md:**:scroll-mt-14`; `<html>` carries no `scroll-p*` class;
  - `<div id="mobile-nav"` carries `overflow-y-auto` and both `max-h-[calc(100dvh-…)]` classes;
  - `<nav aria-label="Main navigation"` still carries `h-12` and `md:h-14` (AC6);
  - overlay invariant: walk `templates/`, every opening tag whose class list holds both `fixed` and `inset-0` carries `z-50` (so it stays above the `z-40` bar).
- [x] T5 — E2E (AC1–AC5, AC7): new describe "Story 11-4 — sticky navigation bar" in `tests/e2e/specs/journeys/responsive-layouts.spec.ts`:
  - per viewport (mobile / tablet / desktop), admin via `loginAs`, pages `/`, `/catalog`, `/series`, `/admin`: guarantee a scrollable document (CSSOM `min-height` on `#main-content`, not an inline `style` attribute), scroll to the bottom, assert `scrollY > 0` and the `nav[aria-label="Main navigation"]` top at 0 and `toBeInViewport()`;
  - skip link (mobile + desktop): scroll to bottom, focus the skip link, press Enter, assert `#main-content` top ≥ nav bottom − 1 and the nav still in viewport;
  - mobile menu (375×667 and landscape 667×375, admin): scroll to bottom, open the hamburger, `#mobile-nav` in viewport and its top equals the nav bottom; `elementFromPoint` at a menu link's centre is that link; the logout button scrolls into view inside the panel; a menu link navigates;
  - overlay: scroll to bottom, open the cheat sheet (`?`), `elementFromPoint` over the bar is not inside `<header>`;
  - print: `page.emulateMedia({ media: "print" })` → header computed `position` is `static`;
  - axe (`checkAccessibility`) on a stuck page with the mobile menu open.
- [x] T6 — Gates: clippy `-D warnings`, `cargo test`, sqlx check, flake grep, `tsc --noEmit`, full E2E (`CI=true --workers=2`).

### Review Findings

Code review 2026-10-28 (Blind Hunter, Edge Case Hunter, Acceptance Auditor): 0 decision-needed, 10 patch (5 Medium), 1 defer, 13 dismissed.

- [x] [Review][Patch] E2E nav check could pass vacuously: a late native `autofocus` (catalog scan field, under parallel load) scrolled the page back to 0 after the scroll poll, and at scrollY 0 a non-sticky bar also sits at y 0 (Medium, blind+edge) → scroll and measure in ONE evaluate, polled until the sample is genuinely scrolled (`scrollToBottomAndMeasure`); menu, dialog and overlay assertions read the same sample [tests/e2e/specs/journeys/responsive-layouts.spec.ts]
- [x] [Review][Patch] Landscape menu test did not prove the panel overflows (Medium) → assert `scrollHeight > clientHeight` at 667×375 [responsive-layouts.spec.ts]
- [x] [Review][Patch] Dialog test only exercised the top layer, not `z-40` (Medium, blind+auditor) → new E2E with the connection-lost overlay (plain `fixed inset-0 z-50`, not top layer) covering the stuck bar [responsive-layouts.spec.ts]
- [x] [Review][Patch] Overlay audit covered `fixed inset-0` in templates only while the header comment claimed toasts too (Medium, blind+edge+auditor) → `fixed_surfaces_stack_above_sticky_nav`: every `fixed` class list in `templates/` and in first-party `static/js/*.js` string literals must carry `z-50`; comment corrected (Rust-built HTML not scanned) [src/templates_audit.rs, templates/components/nav_bar.html]
- [x] [Review][Patch] No fallback when `dvh` is unsupported → the panel would be unbounded again (Medium, edge+blind) → `vh` base value, `dvh` under `supports-[height:100dvh]:` (cascade order checked with the CLI at both breakpoints) [templates/components/nav_bar.html]
- [x] [Review][Patch] Panel bound in `rem` while the bar is in `--spacing` px → drift with a non-16 px root font (Low) → `48px` / `56px` [templates/components/nav_bar.html]
- [x] [Review][Patch] Skip-link E2E skipped the `md` width; the focused link's own visibility was untested (Low) → tablet added; link asserted in viewport and topmost [responsive-layouts.spec.ts]
- [x] [Review][Patch] Landmark render test passed if the wrong form kept the name; desktop form "visible at every width" unchecked (Low) → asserts the named form is the one holding `#lang-select`, located in the always-visible right-hand group [src/routes/nav_sticky_tests.rs]
- [x] [Review][Patch] Render test matched an exact class string for the `lg` strip (Low) → class-set check [src/routes/nav_sticky_tests.rs]
- [x] [Review][Patch] Print test title claimed "not repeated" while asserting `position: static` (Low) → title says what it checks [responsive-layouts.spec.ts]
- [x] [Review][Defer] Sticky bar on very short viewports / high zoom (WCAG 1.4.10 reflow) [templates/components/nav_bar.html] — deferred to #509 (needs a product call on a height threshold)

Mutation proof after the patches (throwaway spec injecting CSS with `bypassCSP`): `header{position:static}` fails all 11 tests; `html{scroll-padding-top:0}` fails the 3 skip-link tests; `header{z-index:60}` fails the overlay test (and the skip-link tests); `#mobile-nav{max-height:none}` fails the landscape-menu test.

Re-review 2026-10-28 (Rule #6, 3 layers on the patched diff): 0 decision-needed, 4 patch (2 Medium), 0 defer, 8 dismissed.

- [x] [Review][Patch] Root `scroll-padding-top` made the page jump ~half a viewport whenever a control inside the stuck bar took focus — Escape returning focus to the hamburger, Shift+Tab into the nav (Medium, edge; reproduced in Chromium) → offset moved to `scroll-margin-top` on `<main>` and its descendants; two E2E tests (desktop focus + Shift+Tab in the bar, mobile Escape) assert `scrollY` is unchanged — D-11-4-8 [templates/layouts/base.html, responsive-layouts.spec.ts]
- [x] [Review][Patch] Story Dev Notes and decision records contradicted the shipped code (audit scope, panel units, gates not re-run after the patches) (Medium, auditor) → notes, T2/T3, D-11-4-2/3/5 updated; gates re-run on the final code (Completion Notes)
- [x] [Review][Patch] Audit floor could stay green with a broken JS scan; backtick literals not scanned (Low, edge+blind) → separate `js_fixed_surfaces >= 2`, backticks scanned, doc comment states what is not seen [src/templates_audit.rs]
- [x] [Review][Patch] `md` bound of the panel not exercised in a browser (Low) → menu test also at 768×1024 [responsive-layouts.spec.ts]

Re-review dismissed: px vs rem for the panel bound (false positive — this project's `--spacing` is `4px`, not `0.25rem`); focus hidden behind the open panel when tabbing out (nav.js traps Tab inside the panel); `md:fixed` / `z-[60]` / `classList.add` gaps in the audit (none exist; documented limits); dialog test re-scrolling with the modal open (would fail loudly, not pass vacuously); desktop language form visibility proven by byte order only (Low, the form sits in the always-visible group, D-11-4-7); skip-link test not starting from a stuck state (the skip link sits at the document top by design; the test still discriminates both halves); stale record of D-11-4-6 (accurate).

Mutation proof after the re-review patch: `html{scroll-padding-top:48px}` fails the two new focus tests; `main, main *{scroll-margin-top:0}` fails the three skip-link tests.

Dismissed (main ones, first pass): existing `sticky top-0` content elements hidden under the bar (none exist — grep); scroll padding ignoring the open mobile panel (the panel traps focus and closes on link click); `#[cfg(test)]` doubled (existing `home_fold_tests.rs` pattern); `body h-full` hypothetical; series detail vs list (D-11-4-6); `md` panel bound not E2E-tested (render test + same mechanism as below `md`); wishlist print page not E2E-tested (no bar by construction); a11y change and bounded panel outside the issue (D-11-4-7, D-11-4-3, recorded).

## Dev Notes

- **Why the `<header>`, not the `<nav>`.** The mobile panel is a sibling of `<nav>` inside `<header>`; sticking the header keeps the panel attached right below the bar with no positioning change. The header is a direct child of `<body>` and no ancestor sets `overflow`, so sticky works against the viewport for the full document height.
- **z-index choice: `z-40`.** Highest in-content layer today is the saved-searches dropdown (`z-30`); every overlay is `z-50` or in the top layer. `z-40` keeps the bar above content and below overlays. The skip link (`focus:z-50`, positioned at the document top) stays above the bar.
- **Backgrounds.** `<nav>` and `#mobile-nav` already carry `bg-white dark:bg-stone-800`; content scrolling underneath is hidden. No shadow, no hide-on-scroll, no size change (AC6 — the epic forbids compaction).
- **Offset for in-page targets: `scroll-margin-top` on `<main>` and descendants (was `scroll-padding-top` on `<html>`, see D-11-4-8).** `scroll-mt-12` → `scroll-margin-top: calc(var(--spacing) * 12)` = 48 px with this project's `--spacing: 4px` (`input.css`); `md:scroll-mt-14` = 56 px — exactly the bar heights. `**:` (Tailwind v4 all-descendants variant, emitted as `:is(& *)`) extends it to every element inside `<main>`, so fragment navigation, `focus()` and `scrollIntoView` land below the bar. A root `scroll-padding-top` would do the same for content but also applies to the controls inside the stuck bar: Chromium then scrolls the page up by the padding each time one of them takes focus. Modal slots and the footer sit outside `<main>` and need no offset.
- **Mobile panel height.** `100vh` fallback, `100dvh` under `@supports (height:100dvh)` (follows the mobile browser's dynamic toolbar); `48px` / `56px` = bar heights (`--spacing` is a px value here, so px matches the bar whatever the root font size). Cascade checked with the CLI: base → `supports` (dvh 48) → `md` (vh 56) → `md`+`supports` (dvh 56). `overscroll-contain` stops the page from scrolling behind the panel when its own scroll ends. `nav.js` focus trap and Escape handling are unchanged.
- **Print.** `print:` is a built-in Tailwind v4 variant (`@media print`). The wishlist print page does not use the base layout (no bar). For any other page printed with Ctrl-P, `position: static` guarantees the bar prints once at the top, never per page.
- **Reduced motion.** No transition or animation is added; the global `prefers-reduced-motion` rule in `input.css` already forces `scroll-behavior: auto`.
- **Arbitrary values** (`max-h-[calc(…)]`, `supports-[…]:`) are generated by the Tailwind CLI from the template scan (no `@source` change; the Dockerfile copies `templates/`).
- **Verified emission at story creation (2026-10-28, superseded for the panel bound and the anchor offset — see the two notes above; `@tailwindcss/cli@4.2.2`, project `input.css`):** `.sticky{position:sticky}`, `.z-40`, `.scroll-pt-12{scroll-padding-top:calc(var(--spacing)*12)}`, `md:scroll-pt-14` under `@media (width>=768px)`, `max-h-[calc(100dvh-3rem)]`, `md:max-h-[calc(100dvh-3.5rem)]`, `overscroll-contain`, and `print:static{@media print{position:static}}` emitted **after** `.sticky`, so it wins in print.
- **Fixed-surface invariant scope.** `templates_audit::fixed_surfaces_stack_above_sticky_nav` checks every `class="…"` in `templates/` and every multi-token string literal (`"…"`, `'…'`, backticks) in first-party `static/js/*.js` whose tokens include `fixed`: it must carry `z-50`. It counts the JS hits separately (the two toasts) so a silently broken JS scan fails. Not seen: `classList.add("fixed", …)` one token per argument, `md:fixed`, CSS files, HTML built in Rust — keep new fixed surfaces to one class string.
- **Hit testing under a top-layer dialog.** `document.elementFromPoint` over a `::backdrop` returns the `<dialog>` (pseudo-elements hit-test to their originating element) — hence the E2E asserts "not inside `<header>`", not a specific element.
- **Skip-link E2E discriminates both halves.** Without `sticky`, the jump leaves the bar off-screen (nav-in-viewport fails); with `sticky` but no scroll-padding, the target lands at viewport top 0 under the bar (`main.top ≥ nav.bottom` fails).
- **Playwright and sticky headers.** Playwright scrolls targets into view and retries with other alignments when another element would receive the click, so existing specs that click near the top of the viewport keep working; the full suite run is the proof.

### Regression map

- `tests/navbar_hamburger.rs`, `tests/navbar_role_visibility.rs`: assert ids, aria attributes and `lg:hidden` on the toggle/panel — the classes are added, none removed → unchanged.
- `src/templates_audit.rs` (no inline markup, CSRF forms, hx-confirm): classes only → green.
- `responsive-layouts.spec.ts` horizontal-overflow tests: no width change.
- `accessibility-full.spec.ts`: no new landmark, no colour change.
- Fail-before proof: on `main` the nav scrolls away (`toBeInViewport` fails after scrolling), the skip target sits at viewport top 0 under a stuck bar without scroll-padding, and the header computes `position: static` but not `sticky` — every E2E assertion and render test fails without T1–T3.

### Testing standards

- Render tests alongside code (`#[cfg(test)] mod tests`), no DB.
- E2E: `loginAs` (real browser login), no `waitForTimeout`, stable selectors (`nav[aria-label="Main navigation"]`, ids), no shared-DB writes (the spec only reads pages).

### References

- `_bmad-output/planning-artifacts/epics.md` § Epic 11 key decisions ("the navigation bar is not compacted") and § Story 11.4, FR125
- Issue #496; CR #443 (`lg` breakpoint); story 9-17 (`nav.js`); story 9-21 (`responsive-layouts.spec.ts`); polish-1 (modal top-layer promotion)
- `CLAUDE.md` Foundation Rules #2, #3, #12, #13; CSP rules (story 7-4)

## Dev Agent Record

### Agent Model Used

claude-opus-5-5

### Debug Log References

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created
- T1–T3: `<header class="sticky top-0 z-40 print:static">`; content offset `<main id="main-content" class="scroll-mt-12 md:scroll-mt-14 **:scroll-mt-12 md:**:scroll-mt-14">` (first shipped as `scroll-pt-*` on `<html>`, replaced in re-review — D-11-4-8); `#mobile-nav` bounded to the viewport below the bar (`vh` fallback, `dvh` under `supports-`), `overflow-y-auto overscroll-contain`. Comments cite #496. No JS, no inline style.
- T4: `src/routes/nav_sticky_tests.rs` (5 render tests: sticky header classes for the three roles, content scroll margin = bar heights and no root scroll padding, bounded mobile panel, navigation not compacted, only the desktop language form is a named landmark) + `templates_audit::fixed_surfaces_stack_above_sticky_nav` (every `fixed` class list in `templates/` and `static/js` carries `z-50`). The layout tests were red before T1–T3.
- Surfaced by the new axe check with the mobile menu open: the desktop and mobile language forms were two form landmarks with the same `aria-label` (axe `landmark-unique`, pre-existing, only visible with the menu open). Fixed in-story by dropping the `aria-label` on the mobile copy (its `<select>` keeps its sr-only label) — D-11-4-7.
- T5: `responsive-layouts.spec.ts` describe "Story 11-4 — sticky navigation bar", 14 tests after review: bar stuck after scrolling on `/`, `/catalog`, `/series`, `/admin` at 375/768/1280; skip-link target below the bar (375/768/1280); stuck mobile menu usable, above content, last entry reachable, axe-clean (375×667, 667×375, 768×1024); focusing a bar control / closing the menu with Escape does not move the page; top-layer dialog and plain `z-50` overlay above the bar; print → `position: static`. Scroll and measurement happen in one evaluate (a late native autofocus can reset the scroll). Spec stable over `--repeat-each=5` (85/85); mutation-checked (see Review Findings).
- T6 gates (2026-10-28, heavy commands under `mem-guard`): `cargo clippy --all-targets -- -D warnings` clean; `cargo test -j 2 --no-fail-fast` against the test DB — 61 binaries, 1,615 passed, 0 failed; `cargo sqlx prepare --check --workspace -- --all-targets` clean; flake grep clean; `tsc --noEmit` clean. Full E2E after `e2e-reset.sh`, `CI=true --workers=2`: **351 passed, 2 skipped, 1 flaky** (`shelving.spec.ts:193`, known #499, green on retry).
- Gates re-run on the final code after the re-review (2026-10-28): clippy `-D warnings` clean; `cargo test --no-fail-fast` (`-j 1` — a `-j 2` attempt was killed by mem-guard under station-wide memory pressure) — 61 binaries, 1,615 passed, 0 failed; sqlx check clean; flake grep clean; `tsc --noEmit` clean; full E2E after `e2e-reset.sh`, `CI=true --workers=2`: **357 passed, 2 skipped, 0 failed, 0 flaky**.

### File List

- `templates/components/nav_bar.html` — sticky header, bounded mobile panel, mobile language form without landmark name
- `templates/layouts/base.html` — content scroll margin on `<main>`
- `src/routes/nav_sticky_tests.rs` — new, 5 render tests
- `src/routes/mod.rs` — test module wiring
- `src/templates_audit.rs` — `fixed` surfaces z-order invariant (templates + static/js)
- `tests/e2e/specs/journeys/responsive-layouts.spec.ts` — 14 E2E tests
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — 11-4 → review
- `_bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md` — § 11-4

## Change Log

- 2026-10-28 — Validation pass (checklist.md): Tailwind emission of every new class verified with the CLI; overlay-invariant scope, `elementFromPoint`/top-layer semantics and the skip-link fail-before reasoning added to Dev Notes. No blocker found.
- 2026-10-28 — Implementation T1–T6; status → review.
- 2026-10-28 — Code review (3 layers): 10 patches applied (5 Medium), 1 deferred to #509, rest dismissed.
- 2026-10-28 — Re-review (Rule #6): 4 patches (2 Medium: root scroll-padding focus jump → `scroll-margin-top` on content, stale records), 8 dismissed.
