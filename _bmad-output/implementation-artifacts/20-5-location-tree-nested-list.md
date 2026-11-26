# Story 20.5: The location tree says what it does (closes the visible part of #700)

Status: ready-for-dev

## Story

As a keyboard or screen-reader user on `/locations`,
I want the tree to behave the way it is announced,
so that the keys I press do what the page told me they would.

**Covers:** fix (#700), UX-DR47. **Epic:** 20 (v1.29.0, shipped with Epic 19).

## Context

`/locations` declares `role="tree"` / `role="treeitem"` / `role="group"` (`templates/pages/locations.html:15`, `src/routes/locations.rs::render_node_at_depth`) but implements none of the ARIA tree keyboard pattern: treeitems are not focusable, there are no arrow keys, and the expansion state is on the toggle button (a disclosure). A screen reader switches to widget mode on `role="tree"` and expects arrow keys; nothing happens (#700). A first attempt in #695 to put `aria-expanded` on treeitems was reverted in review.

Second visible defect (#700, also noted as kept finding 10 of 19-1's review): deleting a location's **last child** removes the child row (OOB `delete` of `#tree-node-{id}`) but leaves the parent's toggle button, its empty child container and its folder icon in place until a reload.

**Decision D10 (project lead, 2026-11-26, approved):** the tree becomes a nested list with disclosure buttons; no ARIA tree. **D11:** the copy below. Nothing else is decided by this story.

## Acceptance Criteria

From `_bmad-output/planning-artifacts/epics.md` → Story 20.5 (verbatim intent, numbered):

1. **Nested list.** On `/locations` the locations are a nested list (`<ul>` / `<li>`): one root list labelled by the page heading (`aria-labelledby` → the `<h1>`), each location an `<li>`, each location's children an `<ul>` inside its `<li>`. No `role="tree"`, `role="treeitem"` or `role="group"` remains anywhere on the page.
2. **Disclosure button.** Each location with children has a disclosure button before its link, carrying `aria-expanded` (`true` / `false`), `aria-controls` = the id of its child list, and an accessible name that includes the location name (`location.tree.toggle_named`, D11). A leaf has no button (the alignment spacer stays). The collapsed state still persists in `localStorage` (`static/js/locations-tree-toggle.js`, key `mybibli-locations-collapsed`, unchanged).
3. **Native Tab order.** Tab reaches, in order, each row's disclosure button, link, add, edit and delete controls (the ones the role may see); nothing needs arrow keys; no key handling is added in JS. Enter and Space on the disclosure button toggle it (native `<button>`), and `aria-expanded` follows.
4. **Parent updated after its last child is deleted.** Deleting a location's last live child updates its parent at once, without a reload: the parent's disclosure button and its (now empty) child list go, the folder icon becomes the leaf icon (📁 → 📍) — server OOB elements in the answer built by `delete_success_response`. A deletion that leaves the parent other live children changes nothing else (no parent OOB).
5. **Focus (Rule #21).** After a deletion, focus goes to the parent's link (or `#add-root-summary` for a root location), as 19-1 does; the `data-modal-focus-fallback` selector matches the new markup.
6. **Accessibility.** axe (WCAG 2.2 AA tags) passes on `/locations` with nested rows, with a parent collapsed and with it expanded; `src/templates_audit.rs` still passes.
7. **Tests.** Unit tests check the rendered tree (no tree roles; a parent's button `aria-controls` points at an id that exists in the output; a leaf has no button; the parent OOB is emitted only for a last child). E2E cases cover the Tab order through one row, Enter and Space on the disclosure button with `aria-expanded` following, the last-child deletion without a reload, and the collapse kept across a reload. `locations.spec.ts` and `accessibility-full.spec.ts` select rows by `#tree-node-{id}` / `li[data-tree-node-id]` — no `[role="treeitem"]` left.

### Approved copy (D11)

| Key | EN | FR | DE (proposed) | IT (proposed) |
|---|---|---|---|---|
| `location.tree.toggle_named` (aria-label; state from `aria-expanded`) | `Locations inside %{name}` | `Emplacements dans %{name}` | `Standorte in %{name}` | `Ubicazioni in %{name}` |

`location.tree.toggle` ("Toggle subtree" / "Afficher / masquer la sous-arborescence" / DE / IT) is **removed** from all four locale files once unused. DE / IT wording follows the existing `location.tree.*` lines (`Standort`, `ubicazione`); reviewed like the other translations.

## Tasks / Subtasks

- [ ] **T0 — Rebase on 20-4 first** (AC all). Story 20-4 (location description) is finished on `story/20-4-location-description`, not yet merged. Start this story only after it merges, from `main` (Rule #14), or rebase onto it before any code. 20-4 touched `LocationEditTemplate`, `LocationDetailTemplate`, the create/update handlers, `LocationModel` (new `description` field), `location_edit.html`, `location_detail.html`, a migration and `location-description.spec.ts`. 20-5 must not touch those; after the rebase, the test helper `loc()` in `locations.rs`'s test module already builds `description` (20-4 added it) — reuse it.
- [ ] **T1 — Markup of the tree** (AC 1, 2, 5) in `src/routes/locations.rs::render_node_at_depth`:
  - [ ] Row element `<div role="treeitem" id="tree-node-{id}" class="{indent_class}" data-tree-node-id="{id}">` → `<li id="tree-node-{id}" class="{indent_class}" data-tree-node-id="{id}">`. Keep `id`, `data-tree-node-id`, the indent class and the inner row `<div class="flex … group">` (the Tailwind `group` **class** stays: it drives `group-hover` / `group-focus-within` on the row controls, #695).
  - [ ] Children container `<div class="tree-children" role="group" id="tree-children-{id}">` → `<ul class="tree-children" id="tree-children-{id}">` (close with `</ul>`); the node closes with `</li>`.
  - [ ] Disclosure button: add `id="tree-toggle-{id}"` and `aria-controls="tree-children-{id}"`; aria-label from `location.tree.toggle_named` with the **raw** name through `t!()`, then the whole label escaped once (the pattern of `add_child_named` l. 276–281). Keep `class="tree-toggle …"`, `aria-expanded="true"`, the `aria-hidden` ▼ icon span. `data-tree-target` may stay or be dropped in favour of `aria-controls` (see T3) — one source, not both read.
  - [ ] Icon span inside the link: add `id="tree-icon-{id}"` (target of the parent OOB, T4).
  - [ ] Focus fallback (l. 289–292): keep `#tree-node-{parent} > div > a[href="/location/{parent}"]` valid by keeping the `li > div > a` structure — then the selector and its unit test `child_delete_falls_back_to_the_parent_link` stay unchanged. If the structure changes, change both.
  - [ ] Update the comments that describe the ARIA tree (l. 333–345, 386–387) and the `TREE_ROW_CONTROLS_CLASS` doc where it mentions the tree role.
- [ ] **T2 — Page container** (AC 1) in `templates/pages/locations.html`: `<h1 id="locations-heading" …>`; replace `<div role="tree" aria-label="{{ tree_title }}" class="space-y-1">` with `<ul id="locations-tree" role="list" aria-labelledby="locations-heading" class="space-y-1">`. Empty-state branch unchanged.
- [ ] **T3 — JS** (AC 2, 3) `static/js/locations-tree-toggle.js`: read the target from `aria-controls` (keep a fallback on `data-tree-target` only if it is kept in the markup); update the header comment (`<ul class="tree-children">`, no tree role). No key handling (native button). Persisted-state restore selector moves to `.tree-toggle[aria-controls="tree-children-<id>"]` if `data-tree-target` is dropped.
- [ ] **T4 — Parent update after the last child's deletion** (AC 4, 5) in `src/routes/locations.rs`:
  - [ ] `delete_location` handler: read the location first (`LocationModel::find_by_id`; `None` → the existing `already_gone_response(id, loc, AlreadyGoneAt::Confirm)`), keep its `parent_id`; call `LocationService::delete_location` as today; on success, if `parent_id` is `Some(p)` and `p` has no live child left (`LocationModel::find_children(pool, p)` empty, or a `COUNT(*) … deleted_at IS NULL`), add the parent OOB. A failure of that read after a successful delete must not turn the answer into an error: log `warn` and answer without the parent OOB (the delete happened; a reload fixes the row).
  - [ ] `delete_success_response(id, emptied_parent: Option<u64>, message)` (signature change; split-out helper `parent_emptied_oob(parent_id) -> String`). The OOB elements (htmx 2, top level of the answer):
    - `<ul id="tree-children-{p}" hx-swap-oob="delete"></ul>` — the now-empty child list;
    - `<span id="tree-toggle-{p}" class="w-6" aria-hidden="true" hx-swap-oob="outerHTML"></span>` — the disclosure button becomes the leaf spacer (same classes as the leaf spacer in `render_node_at_depth`; share one const so they cannot drift);
    - `<span id="tree-icon-{p}" class="text-stone-500 dark:text-stone-400" aria-hidden="true" hx-swap-oob="outerHTML">📍</span>` — the leaf icon (same classes as the rendered icon span; share them).
    Emit the child's `row_removal_oob` first, then these three (if the list went first, the row OOB would only miss its target).
    Prefer these three targeted OOBs over re-rendering the parent `<li>`: a full re-render needs the depth (indent class), the recursive volume count, node types, next L-code, rights and CSRF token, and would wipe an open inline "add child" form.
  - [ ] `row_removal_oob(id)` stays `<div id="tree-node-{id}" hx-swap-oob="delete"></div>` (the tag of a `delete` OOB is irrelevant) — `tests/location_delete_modal.rs` and the unit tests assert that exact string; no churn needed.
  - [ ] The OOB elements must be in the answer **before** `modal-close` runs: they are (same response, `HX-Trigger-After-Swap`); the focus fallback then finds the parent's link, which the targeted OOBs do not replace.
- [ ] **T5 — Locales** (D11) `locales/{en,fr,de,it}.yml`, `location.tree:` block only: add `toggle_named`, remove `toggle`. Run `cargo test --test integration locale_parity::` and `touch src/lib.rs` before building.
- [ ] **T6 — Unit tests** (AC 7) in `src/routes/locations.rs` `mod tests` (render helpers `render_salon` / `render_salon_in`, `loc()`):
  - [ ] Rewrite `render_tree_html_emits_toggle_and_children_container_for_parents` for the new markup: a parent has `<button … id="tree-toggle-1" … aria-controls="tree-children-1" … aria-expanded="true"` and the output contains `id="tree-children-1"` (the id `aria-controls` names exists); a leaf (id 2) has no `tree-toggle-2`, no `tree-children-2`.
  - [ ] New: `render_tree_html` output (and the rendered `/locations` template, via `LocationsTemplate::render` with a non-empty `tree_html` and `base: crate::utils::test_base_context("admin", "locations", …)` as `test_location_detail_template_renders` does) contains no `role="tree"`, `role="treeitem"`, `role="group"`. Match the **attribute** `role="group"`, not the word `group` (the row's Tailwind `group` class stays).
  - [ ] New: every `<li` opened is closed; children lists are `<ul class="tree-children"` inside the parent `<li>` (a 3-level fixture: root → child → grandchild).
  - [ ] New: the toggle's name is `Locations inside Salon` in `en`, `Emplacements dans Salon` in `fr`, and a name with `<b>"` is escaped once (pattern of `control_labels_are_localized_and_escaped`).
  - [ ] Update `delete_success_answer_removes_the_row_and_closes_the_modal` for the new signature; new: with `Some(1)` the body carries the three parent OOBs (`tree-children-1` delete, `tree-toggle-1` and `tree-icon-1` outerHTML); with `None` it carries none of them.
- [ ] **T7 — DB integration tests** (AC 4) in `tests/location_delete_modal.rs` (already a module of `tests/integration.rs`): deleting the only child of a parent → the answer carries the parent OOBs for that parent id; deleting one of two children → no parent OOB; deleting a root → no parent OOB. Existing assertions stay green.
- [ ] **T8 — E2E** (AC 3, 4, 5, 6, 7) in `tests/e2e/specs/journeys/locations.spec.ts`:
  - [ ] Replace every `[role="treeitem"]` (16 places) with row selectors that cannot match ancestors: `li[data-tree-node-id] > div` filtered by text for a row, `#tree-node-{id}` when the id is known (read it from the row's edit link `href`). Note: `filter({ hasText })` on an `<li>` also matches every **ancestor** `<li>` (it contains the child's text) and `.first()` then returns the outermost one — the old `'[role="treeitem"]'.filter({ hasText: "LO-698-Child" }).first()` (l. 225) actually selected the parent. Use the row `div` or the id.
  - [ ] New test — keyboard and disclosure: create a parent + child (unique names, L-codes from the free `L5705`–`L5715` range of this spec's bucket 5; `createLocation` / `createChildLocation` from `helpers/locations.ts`); focus the parent's disclosure button; Tab → the parent link → add → edit → delete (admin); back on the button, Enter → `aria-expanded="false"`, `#tree-children-{id}` hidden; Space → `aria-expanded="true"`, visible. Assert the button's accessible name `Locations inside <name>`.
  - [ ] New test — collapse persists: collapse a parent, `page.reload()`, the button reads `aria-expanded="false"` and the child list is hidden.
  - [ ] New test — last child deleted without a reload: parent + one child; set a marker (`await page.evaluate(() => (window as any).__noReload = true)`); delete the child through the modal; assert the parent row has no `.tree-toggle`, `#tree-children-{pid}` has count 0, the parent's link shows 📍; focus is on the parent's link (Rule #21); the marker is still set (no reload). Then a second parent with two children: delete one → the toggle and the list remain, the other child still listed.
  - [ ] New test — axe on `/locations` with nested rows: parent + child + grandchild created by the spec, axe with `["wcag2a", "wcag2aa", "wcag22aa"]` (as `accessibility-full.spec.ts`), once expanded, once with the parent collapsed. Assert `[role="tree"], [role="treeitem"], [role="group"]` count 0 on the page.
  - [ ] Assert only on the spec's own rows (Rule #23); no `waitForTimeout`; `expectHtmxProcessed` is not needed for these full-page forms.
- [ ] **T9 — `tests/e2e/specs/accessibility-full.spec.ts`** (AC 7): the 3 `[role="treeitem"]` uses (l. 159, 216 and the comments l. 94, 144) become `li[data-tree-node-id]` (the "populated" guard and the seed lookup). The seed lookup by text keeps `.first()` — it seeds a root only.
- [ ] **T10 — Gates.** `cargo check`, `cargo clippy -- -D warnings`, `cargo test` for `routes::locations` + `--test integration location_delete_modal:: locale_parity::`, `templates_audit`; `cd tests/e2e && npx tsc --noEmit`; the `waitForTimeout` grep; E2E on the reset stack with `CI=true npx playwright test --workers=2` (Rule #13 point 4: the specs create locations on the shared DB). Heavy commands under `flock -w 10800 /tmp/station-heavy.lock mem-guard …`.

## Dev Notes

### What exists (read before coding)

- `src/routes/locations.rs` (1152 lines on `main`, ≈ 1370 after 20-4; this story adds ≈ 150 → stays under Rule #12's 2000):
  - `render_tree_html` (l. 156) → `render_node_at_depth` (l. 202–425), the only producer of tree markup. Called only from `locations_page` (l. 431).
  - Row controls: `TREE_ROW_CONTROLS_CLASS` (l. 199), add (`data-locations-toggle="add-child-{id}"`), edit link, delete (Admin only, `data-modal-trigger`, `data-modal-focus-fallback`).
  - Leaf spacer `<span class="w-6" aria-hidden="true"></span>` (l. 365); icon 📁 / 📍 (l. 216–220).
  - Inline add-child form (`<form id="add-child-{id}" … class="hidden {child_margin_class} …">`) sits between the row `div` and the children container — inside the `<li>`, which is valid HTML.
  - `delete_location` (l. 716), `already_gone_response` (l. 749), `row_removal_oob` (l. 772), `delete_success_response` (l. 777).
- `LocationService::delete_location` (`src/services/locations.rs:229`): refuses with children / volumes (request locale), soft-deletes, `NotFound` when gone. Its signature stays; the handler reads the parent.
- `static/js/modal.js::focusFallbackOf` (l. 218) queries the fallback selector **at close time**, after the swap (`HX-Trigger-After-Swap: modal-close`), so it finds the parent link after the OOBs ran. ` || ` alternatives are supported (19-4).
- `static/js/locations-tree-toggle.js`: click delegation on `.tree-toggle`, `hidden` class on the target, ▼ / ▶ icon, `aria-expanded` mirrored, collapsed ids persisted.
- Indentation: `static/css/browse.css` `tree-indent-{0..8,cap}` (padding) on the row element and `tree-margin-*` on the inline form. Tailwind v4 preflight sets `ul { list-style: none; padding: 0; margin: 0 }`, so moving the classes from `div` to `li` keeps the look; `browse.css` needs no change.

### Guardrails

- **`role="list"` on the `<ul>`s.** With `list-style: none`, Safari / VoiceOver drops list semantics. Put `role="list"` on the root `<ul>` and on every `ul.tree-children` (not a tree role; axe accepts it). The unit test forbidding tree roles must not forbid `role="list"`.
- **CSP / audit (story 7-4, 19-1).** No inline `style=` / `on…=`; every `aria-label` in Rust-built markup through `t!()` in the request locale (`templates_audit` reads `src/**/*.rs`). `aria-controls` and ids are not names — fine as literals.
- **Escape once.** Raw name through `t!()`, then `crate::utils::html_escape` on the whole label (never escape the name before `t!()` — double escaping).
- **No arrow keys, no roving tabindex** — D10 decided against the ARIA tree pattern. Do not add `tabindex` to rows.
- **Rule #21.** Do not move focus anywhere new; only keep the 19-1 fallback valid.
- **Rule #20 / data:** nothing is rewritten; the story touches markup and one read after delete.
- **Rule #22:** no new `FOR UPDATE` (the parent read is a plain read after the delete).
- The `delete` OOB of `#tree-children-{p}` also removes the deleted child's own `<li>` if the row OOB has not yet run; both orders give the same DOM. A missing OOB target (two siblings deleted from two tabs) only fires htmx's `oobErrorNoTarget` event; nothing in `static/js` listens to it.
- `localStorage` may still list a parent id that lost its children; harmless (the restore loop skips missing ids). Do not add cleanup.

### Testing standards

- Unit tests next to the code (`mod tests` in `locations.rs`); DB tests are modules of `tests/integration.rs` (`use crate::common`, never `mod common;`).
- E2E: `loginAs(page)` in `beforeEach` (already); i18n regex matchers (`/Locations inside|Emplacements dans/`) when the locale is not pinned; unique names prefixed `LO-700-…`; L-codes `L5705`–`L5715` (free: the spec uses L5001–L5005, L5099, L5185–L5186, L5695, L5698–L5704); a re-run needs `./scripts/e2e-reset.sh` (fixed codes).
- `CI=true npx playwright test --workers=2` is the truth gate (Rule #13).

### Project Structure Notes

- Files touched: `src/routes/locations.rs`, `templates/pages/locations.html`, `static/js/locations-tree-toggle.js`, `locales/{en,fr,de,it}.yml` (`location.tree:` block), `tests/location_delete_modal.rs`, `tests/e2e/specs/journeys/locations.spec.ts`, `tests/e2e/specs/accessibility-full.spec.ts`.
- Not touched: `src/services/locations.rs`, `src/models/location.rs` (20-4's), `location_edit.html`, `location_detail.html` (20-4's), `static/css/browse.css`, `.sqlx/` (no typed query changes).
- Coupling (Rule #24): 20-6 (epic smoke) reaches the tree by keyboard and collapses / expands a parent with Enter — it relies on the button's accessible name and `aria-expanded` defined here. 20-7 adds the CLAUDE.md note "location tree = disclosure list, no ARIA tree".

### References

- `_bmad-output/planning-artifacts/epics.md` → Epic 20 (decisions D10, D11; couplings), Story 20.5; UX-DR47 (l. 353).
- Plan `epic-20-plan.md` (scratchpad) → Story 20.5, D10 option B, copy table "20.5 Location tree".
- GitHub #700 (body: the three defects and the proposed options).
- `_bmad-output/implementation-artifacts/19-1-location-tree-delete-modal.md` → D2 (OOB row removal, `tree-node-{id}`), review pass 1 finding 3 (focus fallback) and kept finding 10 (stale parent — fixed here).
- `_bmad-output/implementation-artifacts/19-4-what-the-user-reads.md` → `data-modal-focus-fallback` alternatives.
- CLAUDE.md → CSP & hardening headers, Modal lifecycle hardening, E2E Test Patterns, Rules #3, #13, #21, #23, #24.

## Validation

Checked on 2026-11-26 against `main` at `be26c23` and the unmerged `story/20-4-location-description` (`5088e8d`).

**Confirmed in the code.**
- Tree markup has one producer, `render_node_at_depth`, reached only through `render_tree_html` ← `locations_page`; the container is `templates/pages/locations.html:15`. No other template, JS module or Rust file emits `role="tree"`, `treeitem` or `group` (repo-wide grep; `src/templates_audit.rs` has no rule about these roles, so the audit is unaffected).
- `location.tree.toggle` is used only at `locations.rs:358`; no JS bundle reads it — removing it from the four locale files is safe.
- `delete_location` does not know the parent today; `LocationService::delete_location` has a single caller (the handler), so reading the parent in the handler (T4) is enough and the service keeps its signature.
- `modal.js` resolves `data-modal-focus-fallback` at close time, after `HX-Trigger-After-Swap` — i.e. after htmx 2 has applied the OOB swaps; the targeted OOBs of T4 never replace the parent's link, so the 19-1 selector `#tree-node-{p} > div > a[href=…]` keeps working with `li > div > a`.
- 20-4's diff touches `LocationModel` (+ `description` in every SELECT, `find_children` included), the edit / detail templates and handlers, and the test helper `loc()`; it does not touch `render_node_at_depth`, `render_tree_html`, `delete_location`, `delete_success_response`, `locations.html` or `locations.spec.ts`. The rebase (T0) should be conflict-free apart from `sprint-status.yaml`'s header.
- No E2E today exercises the disclosure toggle or `mybibli-locations-collapsed` (grep of `tests/e2e`) — the keyboard and persistence cases are new coverage, not rewrites.
- No document-level key handler on `/locations` takes Enter or Space (`mybibli.js` shortcuts use Ctrl/Cmd; `shortcuts.js` takes `?` and `g`; scanner-guard acts only while a modal is open).
- `locations.spec.ts` has 16 `[role="treeitem"]` uses, `accessibility-full.spec.ts` 2 (+ 2 comments). The free L-code range `L5705`–`L5715` was checked against every literal under `tests/e2e/` (bucket 9 is 20-4's `specLcode(9, …)`).

**Gaps fixed in this file during validation.**
- T4: order of the OOB elements (row removal first).
- T6: the `/locations` template render test needs `crate::utils::test_base_context` (existing helper).
- `tests/location_delete_modal.rs` added to the touched files (T7): the plan's file list omitted it, but the parent OOB is only observable end-to-end through the handler.
- T8: the ancestor-matching trap of `filter({ hasText })` on nested `<li>` rows (the existing l. 225 locator already selects the parent instead of the child).
- Guardrails: `role="list"` on the `<ul>`s (VoiceOver drops list semantics under `list-style: none`); the unit test must match `role="group"` as an attribute, since the row keeps the Tailwind `group` class.

**Open questions (visible behaviour not covered by D10 / D11 or the plan's copy).** Settled 2026-11-26, project lead informed and agreed: (1) the "already deleted" answer applies the same parent check and OOB updates; (2) the untranslated "{n} vol" is out of scope, tracked in #737; (3) the last root location deleted keeps today's behaviour.
1. **A child deleted in another tab.** When this tab's modal finds the child already gone (`already_gone_response`, at open or at Confirm), the stale row is removed but its parent may be left with a toggle over an empty list — the same defect as AC 4, by another path. *Recommendation:* apply the same parent check and OOB there (same helper, one read), and add a unit test; it is cheap and makes AC 4 hold whatever the path.
2. **Untranslated volume count in tree rows.** `render_node_at_depth` writes `"{n} vol"` as a literal (l. 222), not through `t!()`: French, German and Italian users read "vol". Not part of #700 and no approved copy exists. *Recommendation:* open a separate `type:bug` issue (copy to approve, four locales) rather than widen 20-5.
3. **Last root location deleted.** The root `<ul>` stays, empty, instead of the "no locations yet" empty state until a reload (pre-existing; the OOB only removes rows). *Recommendation:* leave as is (rare, once per library, reload fixes it); mention it in the 20-5 review notes, no issue unless the lead wants one.

DE / IT wording of `location.tree.toggle_named` (`Standorte in %{name}`, `Ubicazioni in %{name}`) is a proposal, to be reviewed like the other translations (D11 leaves DE / IT to the stories).

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.

### File List
