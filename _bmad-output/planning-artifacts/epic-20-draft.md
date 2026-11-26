# Epic 20 — draft plan (for review by the project lead)

Status: draft, 2026-11-26. Read-only research on `story/19-7-scheduled-backups` (= `main` + 19-7 planning commits). Nothing in the repository was changed.

---

## Findings that change the issues as written

1. **#714 — the trigger already exists.** `templates/pages/wishlist.html` (table row and mobile card) and `templates/pages/wishlist_detail.html` both carry a "✓ Bought" button (`hx-get="/wishlist/{id}/delete-modal"`), and `tests/e2e/specs/journeys/wishlist.spec.ts` (steps 6–7) clicks it and checks that the entry is gone. What is missing is a **"Remove" action that is not "Bought"**: the modal says "Mark … as bought?", its Confirm button reads "Bought", and the audit row is `wishlist_bought`. Only the scan banner says "Remove from wish list" (`autolink_feedback_html`), and it opens the same "bought" modal.
2. **#714 — the modal text is wrong.** `wishlist.delete_modal_body` says the entry can be restored from the Trash within 30 days. `wishlist_items` is in neither `services/soft_delete.rs::ALLOWED_TABLES` (the Trash) nor `services/auto_purge.rs::PURGE_DELETION_ORDER`. A removed entry cannot be restored, and it is never purged.
3. **#524 — half of it is done.** Story 11-11 already shows these tomes under "Expected tomes to review" (`services/expected_tomes.rs::place_on_grid`, `ReviewReason::{AboveTotal, Held}`, `templates/pages/series_detail.html`). What is left is **preventing** the two states: the hand edit of a series total (`SeriesService::update_series` compares the total with the owned *count* only) and the title restore (`TrashService::detect_restore_conflicts` has no `titles` arm). The `series_update` proposal already refuses a total below a live expected tome (story 15-4), so only the hand edit lacks the rule.
4. **The series and location forms show no refusal next to a field.** `templates/pages/series_form.html` and `templates/pages/location_edit.html` are plain `method="POST"` forms. A refusal from `update_series` / `update_location` comes back as `AppError::BadRequest`: a bare 400 fragment with no page around it. Rule #3 asks for an E2E case that asserts the message next to the field. So 20.3 and 20.4 must also re-render their form with the error, as `routes/series_expected_tomes.rs::render_form` already does.
5. `templates/pages/location_edit.html` has a hard-coded English "← Back". 20.4 fixes it (all user-facing text goes through `t!()`).
6. French copy uses "wish list" everywhere (`locales/fr.yml`, e.g. "Ajouter à la wish list"). The copy below keeps that term.

---

## Epic header (format of `epics.md`)

### Epic 20: Daily-use fixes for the wish list, series and locations

Usage feedback of 2026-11-25 (`mybibli-bugs.md`) asked for three small things: removing a wish list entry that is no longer wanted, sending a missing series position to the wish list, and describing where a storage location is. Two older findings go with them: expected tomes that a lowered total or a Trash restore leaves in states that creation refuses (#524), and a location tree that announces a keyboard pattern it does not implement (#700). No new dependency, one migration.

**FRs:** FR183–FR185 (proposed: FR183 wish list removal, FR184 gap to wish list, FR185 location description)
**NFRs:** none new (NFR3 CSP and NFR12 a11y unchanged)
**UX-DRs:** UX-DR47 (proposed: location tree as a nested list with disclosure buttons, if D10 = B)
**Issues:** #714, #715, #716, #524, #700 (the visible part: the tree pattern and the stale parent after a deletion)

**Release.** v1.29.0, shipped together with Epic 19 after both retrospectives (memory "une release par epic": epics 19 + 20 together).

**Key decisions (project lead, date TBD).** See the "Decisions" section below: D1–D12.

**Sequencing.** 20.1 and 20.4 can start at once. 20.2 → 20.3 (shared `src/routes/series.rs`, see couplings). 20.5 after 20.4 (shared `src/routes/locations.rs`). Then 20.6 (epic smoke, Rule #7) → 20.7 (documentation, merged with or after 19.11) → retrospective → release v1.29.0.

**Couplings (Rule #24).**
- **Locale files** `locales/{en,fr,de,it}.yml`: every story adds keys. Each story adds its keys only in its own block (`wishlist:`, `expected_tome:`, `series:`, `location:`), so rebases stay trivial. Run `cargo test --test integration locale_parity::` after each rebase.
- **`src/routes/series.rs`**: 20.2 touches `series_detail_page` (success entry after the combined form, if D5 = A); 20.3 touches `update_series` / `edit_series_form`. Different functions, same file: 20.3 rebases on 20.2.
- **Series page**: 20.2 changes `templates/components/series_gap_grid.html`. 20.3 does not touch the grid or `series_detail.html`; it changes `series_form.html`. The "Expected tomes to review" section stays as it is.
- **`src/routes/series_expected_tomes.rs`**: only 20.2 touches it. 20.3 adds its rule in `services/series.rs` and `models/expected_tome.rs`, not in the routes.
- **`src/routes/locations.rs`**: 20.4 touches `LocationEditTemplate`, `LocationDetailTemplate`, the create/update forms and their handlers. 20.5 touches `render_tree_html` / `render_node_at_depth` / `delete_success_response`. If D9 includes the inline "add child" form, 20.4 also edits `render_node_at_depth`, and the two stories collide: 20.5 then goes first. `LocationModel` gains a field in 20.4; the tree code reads other fields only.
- **`templates/pages/locations.html`**: 20.5 changes the tree container. 20.4 changes it only if D9 includes the root create form.
- **E2E specs**: `tests/e2e/specs/journeys/locations.spec.ts` and `tests/e2e/specs/accessibility-full.spec.ts` select `[role="treeitem"]` (about 20 places). 20.5 owns those edits. 20.4 puts its cases in a **new** spec (`location-description.spec.ts`), and 20.1, 20.2 and 20.3 each use their own new spec (each with its own `specIsbn` id), so no shared spec is edited by two stories.
- **`wishlist.rs` / `WishlistItemModel`**: 20.1 changes the list, detail and delete-modal code. 20.2 (if D4 = A) adds `WishlistItemModel::create_in` (connection variant) in `models/wishlist.rs`. Different functions; 20.1's E2E data must not use a title or ISBN that 20.2's dedup (ISBN, or title + note) would match.
- **Manual and PDFs**: 19.11 and 20.7 both edit `docs/manual/{en,fr}/*.tex` and rebuild binary PDFs. 20.7 goes after 19.11's merge, or the two are done as one story, so the PDFs are rebuilt once.
- **`sprint-status.yaml`**: adding the `epic-20` block is a chore PR (Rule #16), before 20.1. Each story PR then changes its own line plus `last_updated` (header line 2 and the YAML key): that header conflicts on every rebase, so take `main`'s and rewrite it.
- **Migration and backups**: 20.4's migration is additive (`description` NULL). A v1.28 backup restored by 19.8 goes through the migrations and gets the column. The 19-6 dump takes every column, so no change is needed there. `tests/backup_round_trip.rs` covers it if one location in its fixture gets a description.

---

## Stories

### Story 20.1: Remove an entry from the wish list (closes #714)

As a librarian,
I want to remove a wish list entry I no longer want, without saying I bought it,
So that the list holds only books I still want, and its history says what really happened.

**Covers:** FR183, #714

**Acceptance criteria (if D1 = A, D2 = A, D3 = A):**

**Given** the `/wishlist` list (table row and mobile card) and an entry page `/wishlist/{id}`, as a Librarian or Admin
**Then** each entry shows two actions: "Bought" (unchanged) and "Remove", each with an accessible name that includes the entry's title
**When** "Remove" is used
**Then** the UX-DR8 modal opens in `#modal-slot` with the "remove" copy (D11) and a "Remove" confirm button; scanner-guard protects it (no `hx-confirm=`)
**And** on Confirm the entry is soft-deleted (`WishlistItemModel::soft_delete`, version-guarded); an `admin_audit` row `wishlist_removed` is written (the "Bought" path keeps `wishlist_bought`); the browser goes to `/wishlist` and the entry is gone
**And** neither modal claims a Trash restore any more (D2)
**And** an entry already removed in another tab answers, inside the modal, a message that says so and what to do (D11). This is the refusal case: an E2E test asserts the message
**And** Anonymous sees neither action; `DELETE /wishlist/{id}` stays Librarian-gated
**And** the scan banner's "Remove from wish list" opens the "bought" modal as it does today (D1 note)

**Files:**
- `src/routes/wishlist.rs`: `wishlist_delete_modal` takes `?action=remove|bought` (default `bought`) and picks title, body and confirm label; `wishlist_delete` takes the same value (hidden field, or a query string on `action_url`) for the audit action; new labels on `WishlistPageTemplate` and `WishlistDetailTemplate`; the "already removed" answer
- `templates/pages/wishlist.html`, `templates/pages/wishlist_detail.html`: the "Remove" button (`data-modal-trigger`, `aria-haspopup="dialog"`, stable ids such as `wishlist-remove-{id}`)
- `templates/fragments/wishlist_delete_modal.html`: unchanged, or one comment line
- `locales/{en,fr,de,it}.yml` (`wishlist:` block)
- `tests/e2e/specs/journeys/wishlist-remove.spec.ts` (new)

**Tests:**
- Unit (in `wishlist.rs`): the modal template renders the "remove" copy for `action=remove` and the "bought" copy otherwise; an unknown `action` falls back to "bought" (positive shape); no modal body mentions the Trash (string check in the four locales).
- DB integration (`tests/wishlist_remove.rs`, declared in `tests/integration.rs`): a remove writes `wishlist_removed`, a bought writes `wishlist_bought`; a second delete with the old version answers the conflict and writes nothing.
- E2E (`wishlist-remove.spec.ts`): from the list, remove an entry the spec created and check it is gone and its sibling stays (Rule #23); from the entry page, the same; the refusal case: open the modal, delete the entry through `page.request` (another tab), then Confirm, and assert the message in the modal; a Librarian sees both buttons, Anonymous neither; axe on `/wishlist` with both buttons.

**Before approval:** D1 changes behaviour (one action or two), so the story waits for it. The D2 copy fix (the false Trash claim) is copy only and can be done at once.

---

### Story 20.2: Send a missing series position to the wish list (closes #715)

As a librarian looking at a series,
I want to add a missing position to the wish list from the series grid,
So that I do not retype the series and the tome number on `/wishlist/new`.

**Covers:** FR184, #715

**Acceptance criteria (if D4 = A, D5 = A, D6 = A):**

**Given** a series page whose grid shows a plain gap (a missing position with no expected tome), as a Librarian or Admin
**Then** the gap offers "Add to wish list" next to "Name this tome", with an accessible name that includes the position
**When** it is used
**Then** the expected-tome form opens in "wish list" mode (`/series/{id}/expected-tomes/new?position=N&wishlist=1`): the position is pre-filled, the title is pre-filled with "{series}, tome N" (editable), the ISBN is optional, and a help line says the tome is also recorded as expected
**And** on save, one transaction locks the series row by primary key (the existing lock order: series → tomes; no new `FOR UPDATE`, Rule #22), creates the expected tome (`ExpectedTomeService::create_in`) and the wish list item (`WishlistItemModel::create_in`, note = `wishlist_note`), with the same dedup as the expected-tome path: ISBN, or title + note
**And** the browser returns to the series page; `#series-feedback` shows "… was added to your wish list" with a link to the entry, and the gap now shows the expected tome
**And** a position already on the wish list (same ISBN, or same title + note) creates nothing and answers with a link to the existing entry, and the expected tome is not created either
**And** an ISBN a live title already holds answers the existing "already in your catalog" message, and nothing is added
**And** every refusal is shown next to its field, with the values typed kept: blank title, invalid ISBN, a position held or out of range because the series changed meanwhile (`ExpectedTomeError::localized`)
**And** a series deleted meanwhile answers the "not found" page; Anonymous sees no action
**And** after the redirect, focus is where the page puts it on load. No HTMX swap is involved, so Rule #21 does not apply

**Files:**
- `templates/components/series_gap_grid.html`: the "Add to wish list" link in the plain-gap branch (`data-gap-wishlist="{{ pos.position }}"`)
- `src/routes/series_expected_tomes.rs`: `NewTomeQuery` / `TomeForm` gain `wishlist`; `new_form` (prefill, heading, help); `create` (combined path); `wish_tome` split so its dedup and messages are shared by the two paths (DRY)
- `templates/pages/expected_tome_form.html`: hidden `wishlist` field, help line, heading and button label for the mode
- `src/services/expected_tomes.rs`: a `create_with_wishlist` (or the route composes `create_in` + `WishlistItemModel::create_in` in one transaction)
- `src/models/wishlist.rs`: `create_in(conn, draft)`, and `find_by_isbn` / `find_without_isbn` on a connection
- `src/routes/series.rs` (`series_detail_page`) + `templates/pages/series_detail.html`: read `?wishlist_item={id}`, check that the item is live, and render the success entry in `#series-feedback` (only if D5 = A)
- `locales/*.yml` (`expected_tome:` block)
- `tests/e2e/specs/journeys/series-gap-wishlist.spec.ts` (new)

**Tests:**
- Unit: `wishlist_note` and the title prefill in each locale; the query parser accepts `wishlist=1` only (positive shape).
- DB integration (`tests/series_gap_wishlist.rs`): the combined create writes one tome and one item; dedup by ISBN and by title + note writes nothing; a failure of the wish list insert leaves no tome (one transaction); the lock-scope test: the combined create on series A does not wait on a tome write of series B (Rule #22).
- E2E: from a closed series with total 3 and a title at position 1 (all created by the spec), add gap 2 to the wish list; check the series feedback and link, the gap now named, and the entry on `/wishlist`; a second attempt on another gap with the same title answers the existing-entry link; the refusal case: blank the title, submit, assert the message next to the title field.

**Before approval:** D4 is the open point of the issue and changes behaviour, so the story waits.

---

### Story 20.3: Expected tomes stay consistent with the series total and Trash restores (closes #524)

As a librarian,
I want the series edit and the Trash restore to stop leaving expected tomes in states their creation refuses,
So that the series page and the agent see a consistent series.

**Covers:** fix (#524)

**Acceptance criteria (if D7 = A, D8 = A):**

**Given** a closed series with a live, unsettled expected tome at position P
**When** a librarian saves the series form with a total below P (or switches it to closed with such a total)
**Then** the save is refused; the form is shown again with the values typed and the message next to the total field (D11), naming the highest such position
**And** the check runs in one transaction that locks the series row by primary key first, then reads the tomes without a locking read. Tome creation also locks the series row first, so the two serialise (Rule #22); a lock-scope test is written with the query
**And** the existing refusals of the form (name required, duplicate name, total required, total below owned count) are shown the same way, next to their fields
**Given** a title in the Trash whose `title_series` row is still live, and an expected tome created meanwhile at that position
**When** an Admin restores the title from the Trash
**Then** the restore succeeds; `ExpectedTomeService::settle_for_title_id` then records the settlement when the tome carries the title's ISBN (positive shape, Rule #20); otherwise the restore answer adds an information line that names the series, the position and the tome, with a link to the series page, where the tome is listed under "Expected tomes to review"
**And** a restore that touches no expected tome answers exactly as today

**Files:**
- `src/services/series.rs` (`update_series`: transaction, series lock, tome check, a typed refusal that keeps the field)
- `src/models/expected_tome.rs` (`max_unsettled_position_in(conn, series_id)`: live, unsettled, by `series_id` index, no locking read) and `src/models/series.rs` (`lock_by_id_in`, if not already there)
- `src/routes/series.rs` (`update_series` and `create_series` re-render the form with the error; `edit_series_form` shares the renderer)
- `templates/pages/series_form.html` (error slot under each field, `aria-describedby`)
- `src/services/trash.rs` and the restore handler in `src/routes/admin.rs` (titles: settle after commit, then compute the information line)
- `locales/*.yml` (`series:` and `admin.trash` blocks)
- `tests/e2e/specs/journeys/series-total-tomes.spec.ts` (new)

**Tests:**
- Unit: the refusal message in each locale; the pure check "total below the highest unsettled tome position" (`None` total, open series, settled tome ignored).
- DB integration (`tests/series_total_tomes.rs`): lowering below a tome is refused and the row is unchanged; lowering to exactly P passes; a settled tome does not block; the race test (two connections: tome creation at position 5 against a total lowered to 4; one wins and the end state is consistent); the lock-scope test (an edit of series A does not wait on a tome insert in series B); the restore test: matching ISBN → settled; different ISBN → information line, tome unchanged.
- E2E: the refusal case (lower the total under a tome the spec created; assert the message next to the total field and the typed value kept); the restore case (delete a title of the spec's series, name its position as an expected tome, restore the title from the Trash, read the information line and follow its link).

**Before approval:** D7 and D8 change behaviour (refuse or warn; settle or only inform), so the story waits. The form re-render with field errors (finding 4) can start at once.

---

### Story 20.4: A description for a storage location (closes #716)

As a librarian,
I want to describe where a storage location is,
So that anyone can find "the left bookcase, top shelf, behind the door".

**Covers:** FR185, #716

**Acceptance criteria (if D9 = A):**

**Given** a migration adding `storage_locations.description TEXT NULL` (additive, no default text)
**Then** the edit form `/locations/{id}/edit` offers an optional multi-line "Description" field with a help line; blank saves `NULL`; the text is trimmed and stored as typed (no rewriting, Rule #20)
**And** more than 500 characters is refused next to the field, with the values typed kept (Rule #3)
**And** the location page `/location/{id}` shows the description under its heading, escaped by Askama, line breaks kept (`whitespace-pre-line`), never as HTML; nothing is shown when it is empty
**And** `GET /api/v1/locations` returns `description` (null when unset), documented in the API chapter (D9)
**And** the edit page's "← Back" goes through `t!()`
**And** a backup written by 19-6 and restored by 19-8 keeps the description (the round-trip fixture has one)

**Files:**
- `migrations/2026112xxxxxxx_storage_locations_description.sql` (new)
- `src/models/location.rs` (`LocationModel.description`, every `SELECT` and `map`, `update_with_locking`; `create` too if D9 = B)
- `src/services/locations.rs` (`update_location`: trim, length check, typed refusal)
- `src/routes/locations.rs` (`UpdateLocationForm`, `update_location` re-rendering `location_edit.html` with the error, `LocationEditTemplate` and `LocationDetailTemplate` labels; the test helper `loc()` near line 805)
- `templates/pages/location_edit.html`, `templates/pages/location_detail.html`
- `src/routes/api_v1.rs` (`list_locations`)
- `tests/backup_round_trip.rs` (fixture), `locales/*.yml` (`location:` block)
- `tests/e2e/specs/journeys/location-description.spec.ts` (new)
- `.sqlx/` only if a typed macro query changes (the location model uses `sqlx::query()`, so probably none)

**Tests:**
- Unit: the length check (500 passes, 501 refused, blank → `None`, a trimmed value is stored as typed); the detail template escapes `<b>`.
- DB integration (`tests/location_description.rs`): save, clear, version conflict; `deleted_at IS NULL` kept on every read.
- E2E: set a description on a location the spec created, read it on `/location/{id}`, clear it; the refusal case: 501 characters, assert the message next to the field; axe on the edit page.

**Before approval:** D9 decides which forms carry the field (and whether 20.4 collides with 20.5). The migration, the model and the location page display do not depend on it and can start.

---

### Story 20.5: The location tree says what it does (closes the visible part of #700)

As a keyboard or screen-reader user on `/locations`,
I want the tree to behave the way it is announced,
So that the keys I press do what the page told me they would.

**Covers:** fix (#700), UX-DR47 (proposed)

**Acceptance criteria (if D10 = B):**

**Given** `/locations`
**Then** the locations are a nested list (`<ul>` / `<li>`, one landmark-free list labelled by the page heading); no `role="tree"`, `treeitem` or `group` remains
**And** each location with children has a disclosure button before its link: `aria-expanded`, `aria-controls` = its child list, and an accessible name that includes the location name (D11); the collapsed state still persists in `localStorage` (`locations-tree-toggle.js`)
**And** Tab reaches, in order, each row's disclosure button, link, add, edit and delete controls; nothing needs arrow keys
**And** deleting a location's last child updates its parent at once: the disclosure button and the empty child list go, the icon becomes the leaf icon (server OOB in `delete_success_response`); a deletion that leaves other children changes nothing else
**And** after a deletion, focus goes to the parent's link (or "Add a root location" for a root), as 19-1 does, with the `data-modal-focus-fallback` selector updated to the new markup (Rule #21)
**And** axe passes on `/locations` with nested, collapsed and expanded rows; the templates audit still passes

**Files:**
- `src/routes/locations.rs` (`render_tree_html`, `render_node_at_depth`, `delete_success_response` + a parent-row OOB, the focus fallback selector)
- `templates/pages/locations.html` (the container)
- `static/js/locations-tree-toggle.js` (`aria-controls`; no key handling)
- `static/css/browse.css` only if the indent classes move from `div` to `li`
- `locales/*.yml` (`location.tree` block)
- `tests/e2e/specs/journeys/locations.spec.ts`, `tests/e2e/specs/accessibility-full.spec.ts` (selectors: `[role="treeitem"]` becomes `#tree-node-{id}` / `li[data-tree-node-id]`)

**Tests:**
- Unit (in `locations.rs`): the rendered tree has no `role="tree"`, `treeitem` or `group`; a parent with children has a button with `aria-expanded` and `aria-controls` pointing at an existing id; a leaf has none; the delete answer for a last child carries the parent OOB, and the one for a child with siblings does not.
- E2E: keyboard (Tab order through one row; Enter and Space on the disclosure button toggle it, and `aria-expanded` follows); deleting the last child of a parent the spec created removes the parent's toggle without a reload; collapse persists across a reload; axe.

**Before approval:** D10 changes behaviour, so the story waits. The fix for the stale parent after a deletion is needed with either option and can start at once.

---

### Story 20.6: Epic 20 smoke test (Foundation Rule #7)

As the project,
I want one end-to-end journey from a blank browser over what the epic built,
So that the epic is proven done.

**Covers:** Foundation Rule #7

**Acceptance criteria:**

**Given** a blank browser with no injected cookie
**When** the test runs
**Then** it chains: `loginAs` → a closed series of total 4 with one title, created by the spec → a gap sent to the wish list from the grid → the entry found on `/wishlist`, then removed through "Remove" → the series total lowered below that expected tome and refused next to the field → a location created by the spec, given a description, read on its page → the location tree reached by keyboard, a parent collapsed and expanded with Enter
**And** it asserts only on its own rows (Rule #23); no `waitForTimeout`; green under `CI=true npx playwright test --workers=2`. If this test fails, the epic is not done

**Files:** `tests/e2e/specs/journeys/epic20-smoke.spec.ts` (new; its own `specIsbn` id).

**Before approval:** waits for 20.1–20.5.

---

### Story 20.7: Documentation

As a librarian reading the manual,
I want it to describe the new actions,
So that I find them without asking.

**Covers:** FR183–FR185 (documentation)

**Acceptance criteria:**

**Given** the user manual (English and French)
**Then** `03-usage.tex` describes "Remove" versus "Bought" on the wish list, sending a gap to the wish list, the location description, and the total rule for expected tomes; `11-api-integrations.tex` documents `description` in `GET /api/v1/locations`; `06-roles-permissions.tex` is checked for the new actions (Librarian+)
**And** `CLAUDE.md` Key Patterns: "Expected tomes" gains the hand-edit total rule and the restore settlement; a short note describes the location tree as a disclosure list (no ARIA tree)
**And** the PDFs are rebuilt **once**, after 19.11 (or 19.11 and 20.7 are one story)
**And** release documentation (Rule #19, v1.29.0) is done at release time, not in this story

**Files:** `docs/manual/{en,fr}/03-usage.tex`, `11-api-integrations.tex`, possibly `06-roles-permissions.tex`, the two `main.pdf`, `CLAUDE.md`.

**Before approval:** copy only, but it follows the decisions; it is written last.

---

## Decisions for the project lead (practice A3)

**Approved by the project lead on 2026-11-26:** the story cut (20.1–20.7); D1 = A (a separate "Remove"), D2 = A (fix the false Trash text), D3 = A (keep the reload); D4 = A, D5 = A, D6 = A; D7 = A (refuse a total below an expected tome); D8 = A (restore; settle on an equal ISBN, otherwise an information line with a link to the series); D9 as recommended; D10 = B (nested list with disclosure buttons); D11, the copy below; D12, two short retrospectives on the same day, before the v1.29.0 cut. Every decision below is therefore settled as recommended.

**D1 — #714: what "remove" means.**
- A. Add a separate "Remove" next to "Bought". Each has its own modal copy and audit action (`wishlist_removed`, `wishlist_bought`). The scan banner keeps the "bought" modal (a scanned ISBN means the book is in hand).
- B. Keep one action, renamed "Bought or no longer wanted".
- C. Keep as is, and only make "Bought" easier to see.
- *Recommendation: A.* It answers the request, keeps the audit truthful, and costs one button and one query parameter.

**D2 — #714: the false Trash claim.**
- A. Fix the text: the entry cannot be restored.
- B. Put wish list items in the Trash (`ALLOWED_TABLES`, the Trash list reading `title` as the name, `PURGE_DELETION_ORDER`, restore).
- *Recommendation: A in 20.1.* B is a change request of its own if wanted. Note: removed rows are never purged today; adding `wishlist_items` to `PURGE_DELETION_ORDER` alone would fix that, and could be filed with B.

**D3 — #714: after Confirm on the list.**
- A. Keep the full reload to `/wishlist` (`HX-Redirect`, as today, already tested).
- B. Remove the row and the card in place, with focus on the next entry's link.
- *Recommendation: A.* Lists are short; B adds OOB work for two layouts for little gain.

**D4 — #715 open point: create an expected tome first, or a wish list item directly.**
- A. Expected tome first, in one form and one transaction (tome + item). The gap is then named on the series page; scanning the book later settles the tome and fires the wish list banner; the dedup and messages are the existing ones. Cost: a second record, and a title is required (pre-filled, D6).
- B. Item only: open `/wishlist/new` in free-form mode, pre-filled with the title and the note. Less code, but the series page never shows that the position is wanted, and a wish list item cannot be edited afterwards (there is no edit page).
- C. One click: create the item "{series}, tome N" with no form. The fastest, but the title can never be fixed and there is no ISBN.
- *Recommendation: A.* The series grid stays the single picture of what is missing and what is wanted, and nothing new needs to be invented for dedup or settlement.

**D5 — #715: where the librarian lands after saving (if D4 = A).**
- A. Back on the series page, with the success entry and its link in `#series-feedback` (`?wishlist_item={id}`, checked server-side).
- B. On the new wish list entry `/wishlist/{id}`.
- *Recommendation: A.* The librarian is working through a series and may add several gaps.

**D6 — #715: title pre-fill.**
- A. Pre-filled "{series}, tome {position}", editable.
- B. Blank (the field is required).
- *Recommendation: A.*

**D7 — #524: lowering a total below an expected tome.**
- A. Refuse, naming the highest position, next to the total field. This is the rule the `series_update` proposal already applies.
- B. Accept, and warn that tomes are now listed for review.
- C. Accept, and delete the tomes above the new total.
- *Recommendation: A.* B keeps producing the state #524 reports. C deletes data without asking.

**D8 — #524: restoring a title onto a position an expected tome took meanwhile.**
- A. Restore. Settle the tome when it carries the title's ISBN (positive shape). Otherwise add an information line with a link to the series.
- B. Refuse the restore until the tome is moved or deleted.
- C. Restore silently (the review section already shows it).
- *Recommendation: A.* A restore should not fail over a note, and the librarian is told where to look.

**D9 — #716: scope of the description.**
- Where it can be edited: A. the edit form only (recommended: creation stays quick, and 20.4 stays apart from 20.5); B. also the root create form and the inline "add child" form of the tree.
- Where it is shown: on `/location/{id}` only (recommended), not in the tree, not searched.
- Length: 500 characters, plain text (recommended).
- API: add `description` to `GET /api/v1/locations` (additive; recommended). MCP: no change.

**D10 — #700: the tree pattern.**
- A. Implement the ARIA tree: roving `tabindex` on treeitems, Up/Down/Left/Right/Home/End, `aria-expanded` on the treeitem, focus management after deletions. Effort: high (new key-handling JS, keeping focus right through HTMX swaps and deletions, about 20 E2E selectors). Accessibility: good only if done fully. The rows hold links, buttons and an inline form, and interactive content inside treeitems is badly supported by screen readers. Risk: high; the #695 attempt was reverted.
- B. Drop `role="tree"` for a nested list with disclosure buttons. Effort: low (markup, `aria-controls`, selectors). Accessibility: matches what the page does; native Tab order; screen readers announce list levels and "expanded/collapsed" on the buttons; no arrow keys, and none are expected in a list. Risk: low.
- *Recommendation: B.* Either way: fix the stale parent after deleting its last child, and test with axe and the keyboard.

**D11 — copy.** The proposed copy below (EN and FR; DE and IT are translated in the story and reviewed like the others).

**D12 — epics 19 and 20 share v1.29.0.** Do they get one retrospective or two? Rule #8 says one per epic. Proposal: two short retrospectives on the same day, before the release cut.

---

## Proposed copy (EN / FR)

Field refusals are short; operation errors follow "what happened → why → what you can do" (`docs/error-message-style.md`).

### 20.1 Wish list

| Key (proposed) | EN | FR |
|---|---|---|
| `wishlist.action_remove` | Remove | Retirer |
| `wishlist.action_remove_named` (aria-label) | Remove “%{title}” from the wish list | Retirer « %{title} » de la wish list |
| `wishlist.action_bought_named` (aria-label) | Mark “%{title}” as bought | Marquer « %{title} » comme acheté |
| `wishlist.remove_modal_title` | Remove “%{title}” from the wish list? | Retirer « %{title} » de la wish list ? |
| `wishlist.remove_modal_body` | The entry leaves the wish list. It cannot be restored: add it again if you change your mind. | L’entrée quitte la wish list. Elle ne peut pas être restaurée : ajoutez-la à nouveau si vous changez d’avis. |
| `wishlist.delete_modal_body` (fixed, "bought") | The entry leaves the wish list. It cannot be restored afterwards. | L’entrée quitte la wish list. Elle ne pourra pas être restaurée ensuite. |
| `wishlist.already_removed` | This entry is no longer on the wish list. It was probably removed in another tab or by another librarian. Reload the list to see it as it is now. | Cette entrée n’est plus dans la wish list. Elle a sans doute été retirée dans un autre onglet ou par un autre bibliothécaire. Rechargez la liste pour la voir à jour. |

### 20.2 Gap to the wish list

| Key (proposed) | EN | FR |
|---|---|---|
| `expected_tome.gap_add_to_wishlist` | Add to wish list | Ajouter à la wish list |
| `expected_tome.gap_add_to_wishlist_named` (aria-label) | Add tome %{position} to the wish list | Ajouter le tome %{position} à la wish list |
| `expected_tome.form_title_wishlist` | Add tome %{position} to the wish list | Ajouter le tome %{position} à la wish list |
| `expected_tome.wishlist_form_help` | mybibli also records this tome as expected in the series, so the gap shows its title. | mybibli enregistre aussi ce tome comme attendu dans la série : la case manquante affiche alors son titre. |
| `expected_tome.wishlist_title_prefill` | %{series}, tome %{position} | %{series}, tome %{position} |
| `expected_tome.wishlist_save` | Add to wish list | Ajouter à la wish list |

Reused unchanged: `wishlist_added`, `wishlist_already`, `wishlist_open`, `wishlist_owned`, `wishlist_note`, `error.title_required`, the position and ISBN refusals of `ExpectedTomeError`.

### 20.3 Series total and restore

| Key (proposed) | EN | FR |
|---|---|---|
| `series.total_below_expected` (field) | Total must be at least %{position}: an expected tome is recorded at position %{position}. | Le total doit être d’au moins %{position} : un tome attendu est enregistré à la position %{position}. |
| `admin.trash.restore_tome_to_review` | “%{title}” is back at position %{position} of “%{series}”, where an expected tome was recorded meanwhile. Check it under “Expected tomes to review” on the series page. | « %{title} » retrouve la position %{position} de « %{series} », où un tome attendu a été enregistré entre-temps. Vérifiez-le sous « Tomes attendus à revoir » sur la page de la série. |
| `admin.trash.restore_tome_settled` | The expected tome at position %{position} of “%{series}” now counts as owned. | Le tome attendu à la position %{position} de « %{series} » compte désormais comme possédé. |
| link label | Open the series | Ouvrir la série |

Existing refusals shown next to their fields from now on (copy unchanged): `series.name_required`, `series.name_duplicate`, `series.total_required_for_closed`, `series.total_below_owned`.

### 20.4 Location description

| Key (proposed) | EN | FR |
|---|---|---|
| `location.description_label` | Description | Description |
| `location.description_help` | Optional. Where to find it, for example “left bookcase, top shelf, behind the door”. | Facultatif. Où le trouver, par exemple « bibliothèque de gauche, étagère du haut, derrière la porte ». |
| `location.description_too_long` (field) | Description must be at most 500 characters. | La description doit compter au plus 500 caractères. |
| `location.back_to_tree` (replaces "← Back") | Back to locations | Retour aux emplacements |

### 20.5 Location tree (option B)

| Key (proposed) | EN | FR |
|---|---|---|
| `location.tree.toggle_named` (aria-label; state from `aria-expanded`) | Locations inside %{name} | Emplacements dans %{name} |

`location.tree.toggle` ("Toggle subtree" / "Afficher / masquer la sous-arborescence") is removed once unused.

---

## Can a story start before the lead approves?

| Story | Before approval? | What can start now |
|---|---|---|
| 20.1 | Wait for D1 (behaviour) | The D2 copy fix |
| 20.2 | Wait for D4 / D5 (behaviour) | `WishlistItemModel::create_in` and the connection-level dedup lookups |
| 20.3 | Wait for D7 / D8 (behaviour) | Series form re-render with field errors (needed in any case) |
| 20.4 | Wait for D9 (form scope, API) | Migration, model field, display on `/location/{id}` |
| 20.5 | Wait for D10 (behaviour) | Parent update after deleting the last child (needed with either option) |
| 20.6 | After 20.1–20.5 | — |
| 20.7 | After the decisions and 19.11 | — |

Only the D11 copy can change after development starts without rework.
