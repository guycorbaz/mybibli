# Story 11.3: Rotate the "Recent additions" triangle

Status: review

GitHub issue: #494 (type:bug, Low). Covers: FR124. Epic 11 — third story (production fix, ships in v1.20.1 with 11-1 and 11-2).

## Story

As a visitor,
I want the triangle in front of "Recent additions" to show whether the section is open,
so that I can see its state at a glance.

## Existing-code reality check (code map, 2026-10-27)

| Place | Today | Problem |
|---|---|---|
| `templates/pages/home.html` l. 637 | `<details id="recent-additions-fold" data-home-fold-key="recent_additions">` — no class | nothing to hang a state variant on |
| `templates/pages/home.html` l. 639 | `<svg class="w-3 h-3 transition-transform" …><path d="M3 2l5 4-5 4z"/></svg>` (right-pointing triangle) | `transition-transform` but no rotation anywhere → always points right |
| `templates/pages/home.html` l. 638 | `<summary class="… list-none …">` | native marker hidden → the SVG is the only visual state cue |
| `static/js/home-recent-additions.js` | restores `open` from `localStorage` (`mybibli.home.fold.recent_additions` = `"open"`) on load; persists on `toggle` | no change needed — CSS reacts to the `open` attribute whichever way it is set |
| `templates/components/title_card.html` (rendered inside the fold) | `<article class="title-card group">` | an extra **unnamed** `group` ancestor would make any future `group-hover:` in the card fire whenever the pointer is anywhere in the fold → use a **named** group (see Dev Notes) |
| `Dockerfile` l. 22–27 | `output.css` built by `@tailwindcss/cli` (v4, unpinned; local build header says 4.2.2) from `static/css/input.css`; `output.css` is gitignored | new classes are emitted automatically by the Docker build (E2E and prod) |

No Rust handler, no locale key, no JS, no migration.

## Acceptance Criteria

1. **AC1 — open/close.** Given the section is closed, when I open it, then the triangle points down (rotated 90°), and points right again (no rotation) when I close it.
2. **AC2 — restored state.** Given an open state restored from `localStorage` on load, when the page renders, then the triangle already points down.
3. **AC3 — CSS only.** The change is CSS only: a Tailwind group on `<details id="recent-additions-fold">` and a `group-open` rotate-90 variant on the `<svg>`; CSP-clean (no inline style/script), no JavaScript change.
4. **AC4 — tests.** E2E (`tests/e2e/specs/journeys/home.spec.ts`) asserts the icon's computed rotation when closed, open, closed again, and after a reload that restores the open state. A Rust render test asserts the classes are present on the rendered fold.

## Tasks / Subtasks

- [x] T1 — Template (AC1–AC3): `templates/pages/home.html` — add `group/fold` to the `<details>`; add `group-open/fold:rotate-90` to the `<svg>` (keep `transition-transform`); update the CR #261 comment block with one line citing #494.
- [x] T2 — Rust render test (AC4): in a new sibling test module `src/routes/home_fold_tests.rs` (home.rs is already 2,562 lines — Rule #12, do not grow it; `home_indicator_tests.rs` is scoped to the indicators), render `make_test_home_template_with_counts("anonymous", false, 0, 0, 0)`, slice `#recent-additions` with `home::tests::slice_section`, assert the `<details id="recent-additions-fold"` tag carries `group/fold` and the `<svg` inside `<summary>` carries `group-open/fold:rotate-90` and `transition-transform`.
- [x] T3 — E2E (AC1, AC2, AC4): new test in the "Home page — Recent additions section" describe of `home.spec.ts`: anonymous blank context, `goto("/")`, icon = `#recent-additions-fold > summary svg`; `toHaveCSS("rotate", "none")`; click `summary` → `toHaveCSS("rotate", "90deg")`; click again → `"none"`; click to open, `page.reload()`, expect `details` to have the `open` attribute and the icon `toHaveCSS("rotate", "90deg")`.
- [x] T4 — Gates: clippy `-D warnings`, `cargo test`, sqlx check, flake grep, `tsc --noEmit`, full E2E (`CI=true --workers=2`).

## Dev Notes

- **Tailwind v4 emits the individual `rotate` property, not `transform`.** Verified with `@tailwindcss/cli@4.2.2` on 2026-10-27: `group-open/fold:rotate-90` → `&:is(:where(.group\/fold):is([open], :popover-open, :open) *) { rotate: 90deg; }`, and `transition-transform` → `transition-property: transform, translate, scale, rotate`. So the transition still animates, and the E2E must read the computed **`rotate`** (`"90deg"` / `"none"`); the computed `transform` stays `"none"` in both states. Playwright's `toHaveCSS` retries, which absorbs the 150 ms transition.
- **Named group (`group/fold`) instead of a bare `group`.** Tailwind's `group-*` variants match *any* ancestor `.group`. The fold contains title cards that are themselves `.group`; a bare `group` on `<details>` is harmless today (no `group-*` variant is used in any template) but would make a future `group-hover:` in a card fire across the whole fold. The named group scopes the variant to this `<details>` only. Same CSS-only, CSP-clean change the AC describes.
- **Rotation direction.** The path `M3 2l5 4-5 4z` points right; `rotate: 90deg` (clockwise) points it down. The default `transform-origin` (center of the 12×12 box) keeps it in place.
- **Restored state.** `home-recent-additions.js` sets the `open` attribute on `DOMContentLoaded`; the CSS selector matches `[open]` immediately. A brief transition from right to down may play on load when the state is restored — acceptable (same behaviour as any CSS-driven disclosure; not in the AC).
- Out of scope: other folds (locations tree #200 uses its own markup), `prefers-reduced-motion` handling for the 150 ms transition (pre-existing `transition-transform`), user-manual wording (epic documentation story 11-24).

### Regression map

- The fold's existing E2E (`home.spec.ts` ~l. 108) opens the fold via `el.open = true` and clicks a card — unaffected.
- `src/templates_audit.rs::no_inline_markup_in_templates` — classes only, no inline style → stays green.
- `home::tests::home_renders_recent_additions_with_three_items` counts `class="title-card group"` inside the section — the `<details>` class is `group/fold`, not that string → count unchanged.
- Fail-before proof: on `main` the open fold's icon computes `rotate: none`, so the new E2E fails without T1; the E2E stack (`tests/e2e/docker-compose.test.yml`) builds the app from the repo `Dockerfile`, whose `css` stage regenerates `output.css` from the templates — no committed CSS to update.
- No `toHaveCSS` precedent in the suite yet; it is Playwright's retrying web-first assertion (the existing "Tailwind CSS styles" test reads `getComputedStyle` once, which would race the 150 ms transition here).

### Testing standards

- Unit/render tests alongside code (`#[cfg(test)] mod tests`).
- E2E: blank anonymous context (the page is public; no `loginAs` needed — this *is* the real user journey), no `waitForTimeout`, stable id selectors.

### References

- `_bmad-output/planning-artifacts/epics.md` § Story 11.3, FR124
- Issue #494; CR #261 (introduced the fold)
- `CLAUDE.md` Foundation Rules #2, #3, #12, #13; CSP rules (story 7-4)

## Dev Agent Record

### Agent Model Used

claude-opus-5-5

### Debug Log References

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created
- T1: `group/fold` on `<details id="recent-additions-fold">`, `group-open/fold:rotate-90` on the triangle; CR #261 comment extended with the #494 rationale (named group). No JS, no inline style.
- T2: `src/routes/home_fold_tests.rs` (new sibling test module, wired `#[cfg(test)]` in `routes/mod.rs`) — 2 render tests: the fold carries `group/fold` and the icon `group-open/fold:rotate-90` + `transition-transform`; the fold carries no bare `group` (D-11-3-2). Both red before T1, green after.
- T3: `home.spec.ts` — "fold triangle points down while open, right while closed, and after reload" (blank anonymous context; `toHaveCSS("rotate", …)` closed → open → closed → open + reload).
- T4 gates (2026-10-27, heavy commands under `mem-guard`): `cargo clippy --all-targets -- -D warnings` clean; `cargo test -j 2 --no-fail-fast` against the test DB — 61 binaries, 1,609 passed, 0 failed; `cargo sqlx prepare --check --workspace -- --all-targets` clean; flake grep clean; `tsc --noEmit` clean. E2E: targeted describe 2/2; full suite after `e2e-reset.sh`, `CI=true --workers=2`: **343 passed, 2 skipped, 0 failed**.

### File List

- `templates/pages/home.html` — fold classes + comment
- `src/routes/home_fold_tests.rs` — new, 2 render tests
- `src/routes/mod.rs` — test module wiring
- `tests/e2e/specs/journeys/home.spec.ts` — 1 E2E test
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — 11-3 → review
- `_bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md` — § 11-3 (D-11-3-1, D-11-3-2)

## Change Log

- 2026-10-27 — Validation pass (checklist.md): added the fail-before proof, the E2E stack CSS build path and the `toHaveCSS` rationale to the regression map. No blocker found.
- 2026-10-27 — Implementation T1–T4; status → review.
