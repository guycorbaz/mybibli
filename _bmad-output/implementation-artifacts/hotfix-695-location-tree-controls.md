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

- **D1 — source `src/` everywhere.** `input.css` gains `@source "../../src"`, the `css` stage gains `COPY src/ ./src/`. `tests/tailwind_sources.rs` checks that `src` is a source and that every positive `@source` directory is copied into the `css` stage, so the next source added cannot repeat the bug.
- **D2 — controls always shown, dimmed** (user's choice, 2026-11-18): `opacity-60`, full opacity on row hover (`group-hover:`), on focus inside the controls (`focus-within:`) and on a coarse pointer (`pointer-coarse:`). The class list is `locations::TREE_ROW_CONTROLS_CLASS`.
- **D3 — side effect accepted.** Building the CSS with `src/` adds 52 selectors to the image's `output.css` (measured by building the old and new stage contexts). Those that matter are classes Rust-built HTML already carried and never got in the image — feedback entry colours (`bg-green-50`, `border-green-500`, `bg-yellow-50`, …), the title page cover height (`h-72`), hover colours of the tree controls, `no-underline` on tree rows. The layout-looking ones (`md:fixed`, `md:overflow-y-auto`, `overflow-scroll`, `collapse`, `ml-100`) come from comments and test strings; no element carries them.

- **D4 — feedback suggestion contrast (found by the full E2E run).** With the colours of D3 in the image, a feedback entry's suggestion line (`text-stone-500` on `bg-blue-50`) fell to 4.4:1 (axe, contributor form). It moves to `text-stone-600` in both renderers (`utils::feedback_html_action`, `components/feedback_entry.html`).
- **D5 — `role="group"` on the tree's children container.** A nested `treeitem` sat in a plain `div` (axe `aria-required-parent`), visible only when the database holds nested locations at the time the accessibility spec runs. One attribute on the same component; the unit test pins it.

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

- Out of scope, noticed: the tree's delete button still carries an `hx-confirm=` and English `aria-label`s built in Rust (`templates_audit` only scans templates). Not touched here.
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
