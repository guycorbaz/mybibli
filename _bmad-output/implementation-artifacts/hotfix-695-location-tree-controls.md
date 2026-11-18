# Story hotfix-695: Location tree controls visible

Status: review

GitHub issue: #695 (closes). Covers: fix. Outside any epic; ships in patch v1.28.1 (decided 2026-11-18). Sprint-status key: `hotfix-695-location-tree-controls`.

## Story

As a librarian on `/locations`,
I want the add / edit / delete controls of each tree row to be visible,
so that I do not have to guess where they are — with a mouse, a finger or the keyboard.

## Context

The controls are built in Rust (`src/routes/locations.rs::render_node_at_depth`) inside a `<span class="… opacity-0 group-hover:opacity-100 …">`. `static/css/input.css` sources only `static/js` explicitly; a local Tailwind run from the repo root finds `src/` anyway through automatic source detection, but the Dockerfile `css` stage copies only `templates/` and `static/js/`, so the shipped `output.css` lacks every class used only in Rust-built HTML. `group-hover:opacity-100` is one of them: in the image, the controls stay at opacity 0. Even with the class generated, a hover-only control is unreachable on a touch screen and invisible while keyboard-focused.

## Decisions

- **D1 — declared sources only, the same everywhere.** `input.css` imports Tailwind with `source(none)` and declares `templates`, `static/js` and `src` (`@source "../../src"` new); the `css` stage gains `COPY src/ ./src/`. A local build and the image now read the same set (measured: identical class lists). `tests/tailwind_sources.rs` checks `source(none)`, the three sources, and that every positive `@source` path is inside a path the `css` stage copies **to the same place** (source = destination: Tailwind reads the stage's tree) — the next source added cannot repeat the bug.
- **D2 — controls always shown, dimmed** (user's choice, 2026-11-18): `opacity-75` (review: 60 % put `text-stone-600` under 3:1, WCAG 1.4.11), full opacity on row hover (`group-hover:`), on keyboard focus anywhere in the row (`group-focus-within:`) and on a coarse pointer (`pointer-coarse:`). The class list is `locations::TREE_ROW_CONTROLS_CLASS`.
- **D3 — side effect accepted.** Building the CSS with `src/` adds 52 selectors to the image's `output.css` (measured by building the old and new stage contexts). Those that matter are classes Rust-built HTML already carried and never got in the image — feedback entry colours (`bg-green-50`, `border-green-500`, `bg-yellow-50`, …), the title page cover height (`h-72`), hover colours of the tree controls, `no-underline` on tree rows. The layout-looking ones (`md:fixed`, `md:overflow-y-auto`, `overflow-scroll`, `collapse`, `ml-100`) come from comments and test strings; no element carries them.

- **D4 — feedback suggestion contrast (found by the full E2E run).** With the colours of D3 in the image, a feedback entry's suggestion line (`text-stone-500` on `bg-blue-50`) fell to 4.4:1 (axe, contributor form). It moves to `text-stone-600` in both renderers (`utils::feedback_html_action`, `components/feedback_entry.html`).
- **D5 — `role="group"` on the tree's children container.** A nested `treeitem` sat in a plain `div` (axe `aria-required-parent`), visible only when the database holds nested locations at the time the accessibility spec runs. One attribute on the same component; the unit test pins it.

- **D6 — expansion state on the toggle only (review passes 1–2).** Pass 1 asked for `aria-expanded` on parent treeitems; pass 2 showed that without roving focus it is announced twice and goes stale when the last child is deleted. Kept as a disclosure: `aria-expanded` on the toggle button (one `setExpanded` in `locations-tree-toggle.js`); the full tree pattern → #700.
- **D7 — Rust test files are not Tailwind sources.** `@source not` excludes `src/templates_audit.rs` and `src/**/*_tests.rs` (class names quoted in assertions, e.g. `md:fixed`).

## Acceptance Criteria

1. **AC1.** In the Docker image, a tree row's controls are visible at rest (opacity strictly between 0 and 1), at full opacity on row hover and when one of them holds keyboard focus.
2. **AC2.** `tests/tailwind_sources.rs` fails when `input.css` stops sourcing `src/` or when a source directory is not copied into the `css` stage.
3. **AC3.** A unit test pins D2's classes and refuses `opacity-0`.

## Tasks / Subtasks

- [x] T1 — `input.css` + Dockerfile (AC1, D1)
- [x] T2 — `TREE_ROW_CONTROLS_CLASS` (D2) + unit test (AC3)
- [x] T3 — `tests/tailwind_sources.rs` (AC2); `read()` shared through `tests/common/repo.rs` with `toolchain_pins`
- [x] T4 — E2E case in `locations.spec.ts`: computed opacity (Playwright's `toBeVisible` counts opacity 0 as visible)
- [x] T5 — CLAUDE.md (navigation shell paragraph: Tailwind sources)
- [x] T6 — gates (full E2E on a fresh stack, 2 workers)

## Dev Notes

- Out of scope: the tree's delete button still carries an `hx-confirm=` and English `aria-label`s built in Rust (`templates_audit` only scans templates) → #698.
- Review (high, 10 candidates): fixed 1 (`source(none)`), 2 (75 %), 4 (`aria-expanded`), 5 (`group-focus-within`), 6 (`#[path]` include of `repo.rs` only), 7 (option flags, prefix matching), 9 (wait for the row, not the URL), 10 (no comment line in sprint-status); 3 → #698; kept 8 (utilities generated from Rust comments match no element; the `css` layer rebuilding on a Rust edit costs seconds).
- Review pass 2 (high, 8 candidates): fixed 1 (COPY destination checked), 3–4 (D6, → #700), 5 (`toolchain_pins` includes `repo.rs` only), 7 (D7); 6 (feedback markup in four copies) → #701; 2 (the delete control, now visible, still uses `hx-confirm` and English labels) → #698, decision for the release; kept 8 (JSON-form `COPY` / renamed stage fail loudly, never pass silently).
- DRY: the location test fixtures of `locations.rs` go through `loc(…)` / `render_salon(…)`.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (`claude-opus-5-5`)

### File List

- `static/css/input.css`, `Dockerfile`
- `src/routes/locations.rs`
- `tests/tailwind_sources.rs` (new), `tests/common/repo.rs` (new), `tests/common/mod.rs`, `tests/toolchain_pins.rs`
- `tests/e2e/specs/journeys/locations.spec.ts`
- `CLAUDE.md`, `_bmad-output/implementation-artifacts/sprint-status.yaml`
