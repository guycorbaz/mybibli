# Story 20.2: Send a missing series position to the wish list

Status: review

<!-- Closes #715. Source: _bmad-output/planning-artifacts/epics.md, Epic 20, Story 20.2; decisions D4, D5, D6 (approved by the project lead, 2026-11-26). Open questions Q1–Q4 (see Validation) approved by the project lead as recommended, 2026-11-26. -->

## Story

As a librarian looking at a series,
I want to add a missing position to the wish list from the series grid,
so that I do not retype the series and the tome number on `/wishlist/new`.

**Covers:** FR184, #715. **Branch:** `story/20-2-gap-to-wishlist`.

## Decisions this story implements (approved, epics.md "Decisions")

- **D4** — a gap sent to the wish list records an expected tome first, in one form and one transaction (tome + wish list entry).
- **D5** — after saving, the librarian is back on the series page, with the success entry and its link in `#series-feedback`.
- **D6** — the title is pre-filled "{series}, tome {position}", editable.

## Acceptance Criteria

1. **Given** a series page whose grid shows a plain gap (a missing position with no expected tome), as a Librarian or Admin, **then** the gap offers "Add to wish list" next to "Name this tome", with an accessible name that includes the position. Anonymous sees no action.
2. **When** it is used, **then** the expected-tome form opens in "wish list" mode (`/series/{id}/expected-tomes/new?position=N&wishlist=1`): the position is pre-filled, the title is pre-filled with "{series}, tome N" (editable), the ISBN is optional, and a help line says the tome is also recorded as expected.
3. **On save**, one transaction locks the series row by primary key (the existing lock order: series → tomes; no new `FOR UPDATE`, Rule #22), creates the expected tome (`ExpectedTomeService::create_in`) and the wish list entry (`WishlistItemModel::create_in`, notes = `wishlist_note(series, position)`), with the same dedup as the expected-tome path: ISBN, or title + note.
4. **Then** the browser returns to the series page (`/series/{id}?wishlist_item={item_id}`); `#series-feedback` shows "… was added to your wish list" with a link to the entry (the id is checked server-side: a live item, else nothing is shown), and the gap now shows the expected tome.
5. A position already on the wish list (same ISBN, or same title + note) creates nothing — neither entry nor tome — and answers with a link to the existing entry.
6. An ISBN a live title already holds answers the existing "already in your catalog" message, and nothing is added.
7. Every refusal is shown next to its field, with the values typed kept: blank title, invalid ISBN, a position held or out of range because the series changed meanwhile (`ExpectedTomeError::localized`).
8. A series deleted meanwhile answers the "not found" page.
9. The query parser accepts `wishlist=1` only (positive shape, Rule #20): any other value opens the plain form. The wish list dedup and its messages are shared by the two paths — the existing "Add to wishlist" button of an expected tome and the new combined form (DRY, Rule #1).
10. After the redirect, focus is where the page puts it on load. No HTMX swap is involved, so Rule #21 does not apply.
11. **Tests:** a DB integration test covers the combined create, both dedups, a failed wish list insert leaving no tome, and the lock scope (the combined create on series A does not wait on a tome write of series B); an E2E spec (`series-gap-wishlist.spec.ts`) adds a gap from a series it created, checks the feedback link, the named gap and the entry on `/wishlist`, the existing-entry answer, and the blank-title refusal next to the field.

## Proposed copy (taken from the epic plan, approved with the epic — D11)

Copy below is the plan's proposed copy (EN / FR), approved with the epic. DE and IT are translated in this story and reviewed like the others. Keys go in the `expected_tome:` block of `locales/{en,fr,de,it}.yml` only (coupling rule: each story its own block).

| Key | EN | FR |
|---|---|---|
| `expected_tome.gap_add_to_wishlist` | Add to wish list | Ajouter à la wish list |
| `expected_tome.gap_add_to_wishlist_named` (aria-label) | Add tome %{position} to the wish list | Ajouter le tome %{position} à la wish list |
| `expected_tome.form_title_wishlist` | Add tome %{position} to the wish list | Ajouter le tome %{position} à la wish list |
| `expected_tome.wishlist_form_help` | mybibli also records this tome as expected in the series, so the gap shows its title. | mybibli enregistre aussi ce tome comme attendu dans la série : la case manquante affiche alors son titre. |
| `expected_tome.wishlist_title_prefill` | %{series}, tome %{position} | %{series}, tome %{position} |
| `expected_tome.wishlist_save` | Add to wish list | Ajouter à la wish list |

Reused unchanged: `expected_tome.wishlist_added`, `wishlist_already`, `wishlist_open`, `wishlist_owned`, `wishlist_note`, `expected_tome.error.*` (the title, position and ISBN refusals of `ExpectedTomeError`). French says "wish list" (house term, epics.md).

## Tasks / Subtasks

- [x] **Model** — `src/models/wishlist.rs` (AC 3, 5)
  - [x] `WishlistItemModel::create_in(conn: &mut MySqlConnection, draft) -> Result<u64, AppError>`; `create(pool, …)` delegates to it (acquire a connection) so there is one INSERT.
  - [x] `find_by_isbn_in` / `find_without_isbn_in` on a connection; the pool variants delegate. Keep `deleted_at IS NULL` on every read.
  - [x] Do not touch `list_active`, `find_by_id`, `soft_delete`, `count_active` (story 20.1 works there in parallel).
- [x] **Shared dedup** — `src/routes/series_expected_tomes.rs` (AC 5, 9)
  - [x] Split `wish_tome` so the lookup "existing item for (isbn | title + note)" and the two feedback builders (added / already, with the "Open the wish list entry" link) are one function used by both paths.
- [x] **Grid** — `templates/components/series_gap_grid.html` (AC 1)
  - [x] In the plain-gap branch (no `pos.expected`), Librarian/Admin only, next to `data-name-gap`: `<a href="/series/{{ series.id }}/expected-tomes/new?position={{ pos.position }}&amp;wishlist=1" data-gap-wishlist="{{ pos.position }}" aria-label="…named…">`.
  - [x] The named aria-label takes the position: give `ExpectedTomeLabels` (`src/routes/series.rs`) the request locale and a method `gap_wishlist_named(&self, position: i32) -> String`, called from the template (Askama calls methods on fields).
- [x] **Form, GET** — `new_form` (AC 2, 9)
  - [x] `NewTomeQuery` gains `wishlist: Option<String>`; `wishlist_mode = query.wishlist.as_deref() == Some("1")`.
  - [x] Wish list mode: heading `form_title_wishlist` (position), title pre-filled with `wishlist_title_prefill` (series name, position) when the position is valid, help line, save label `wishlist_save`, hidden `<input name="wishlist" value="1">`.
- [x] **Form, POST** — `create` (AC 3–8)
  - [x] `TomeForm` gains `wishlist: Option<String>` (same positive shape).
  - [x] Plain mode: unchanged.
  - [x] Wish list mode, in this order on one transaction (see Validation V3 for why):
    1. pure checks first, outside the transaction: `validate_title`, `normalize_isbn`, `parse_position` (a refusal here re-renders at once, nothing locked);
    2. `ExpectedTomeModel::lock_series_in(&mut tx, id)` — the series row by primary key, the lock `create_in` takes anyway (re-taking it in the same transaction is a no-op); `None` → the existing 404;
    3. owned ISBN: `ExpectedTomeModel::live_titles_with_isbn` (plain read) non-empty → roll back, nothing written, the "already in your catalog" answer (AC 6; copy: open question Q2);
    4. wish list dedup (`find_by_isbn_in` / `find_without_isbn_in` with the note of the **submitted** position) hit → roll back, nothing written, the existing-entry answer with its link (AC 5; where it shows: open question Q1);
    5. `ExpectedTomeService::create_in(&mut tx, id, &input, ExpectedTomeOrigin::Manual)` — its refusals re-render the form (AC 7);
    6. `WishlistItemModel::create_in(&mut tx, draft)` with the tome's title, its normalised ISBN and `notes = wishlist_note(&series.name, position, loc)`;
    7. commit; then `audit_wishlist_add` (best effort, as today), `tracing::info!` with series, tome and item ids, redirect `303` to `/series/{id}?wishlist_item={item_id}`.
  - [x] Refusals: `refused()` re-renders the form in wish list mode (the hidden field, the heading, the help line and a `?wishlist=1` page URI kept), typed values kept, status 422; a database error propagates.
  - [x] Field-level errors (AC 7, Rule #3; scope: open question Q3): a pure `ExpectedTomeError::field() -> Option<TomeField>` (`Position` for `PositionOutOfRange` / `PositionHeld` / `PositionTaken`; `Title` for `TitleRequired` / `TitleTooLong`; `Isbn` for `InvalidIsbn` / `DuplicateIsbn`; `None` otherwise). The form shows the message under its field (`#expected-tome-position-error`, `#expected-tome-title-error`, `#expected-tome-isbn-error`, linked by `aria-describedby`, the input `aria-invalid="true"`); a message with no field stays in `#expected-tome-error` above the form.
  - [x] Give the submit button a stable id, `id="expected-tome-save"` (the 19-2 rule: target controls by id); the existing spec's `main button[type="submit"]').last()` keeps working.
- [x] **Series page** — `series_detail_page` + `templates/pages/series_detail.html` (AC 4)
  - [x] `SeriesDetailQuery` gains `wishlist_item: Option<String>`; parse a positive `u64` only; `WishlistItemModel::find_by_id` (live) → `expected_tome.wishlist_added` + link `/wishlist/{id}` via `crate::utils::feedback_html_with_link`, rendered inside `#series-feedback`; anything else renders nothing.
- [x] **Locales** — the six keys above in `en`, `fr`, `de`, `it`; `touch src/lib.rs` before building; `cargo test --test integration locale_parity::`.
- [x] **Tests** (see Testing)

## Dev Notes

### What exists (read before coding)

- `src/routes/series_expected_tomes.rs` (556 lines): `new_form`, `create`, `update`, `refused` (re-renders the form, 422, typed values kept, page URI rebuilt as a GET address), `delete_modal`, `delete`, `add_to_wishlist` → `wish_tome` (story 11-11, D-11-11-4: dedup by ISBN, else by title + note; answers into `#series-feedback`), `wishlist_note`.
- `src/services/expected_tomes.rs`: `ExpectedTomeService::create_in(conn, series_id, input, origin)` validates title and ISBN, **locks the series row** (`ExpectedTomeModel::lock_series_in`, `FOR UPDATE` by PK), runs `check_slot` (range, holder, position taken, duplicate ISBN in the series), inserts, then `settle_with_owned_title` — which **attaches a live title carrying the ISBN to the series and settles the tome** (attach = true). `ExpectedTomeError::localized(loc)` gives the librarian-facing copy.
- `src/models/wishlist.rs`: `create`, `find_by_id`, `find_by_isbn`, `find_without_isbn`, `list_active`, `count_active`, `soft_delete` — all on `&DbPool`. No connection variant yet.
- `src/routes/wishlist.rs::audit_wishlist_add(pool, session, item_id, title)` — the `wishlist_add` audit row, shared already.
- `templates/components/series_gap_grid.html`: plain gap branch at the `{% else %}` of `pos.expected` with the `data-name-gap` link; Librarian/Admin gated by `base.role`.
- `templates/pages/series_detail.html`: `<div id="series-feedback" class="mt-4" aria-live="polite"></div>`, empty today.
- `templates/pages/expected_tome_form.html`: full-page POST form, CSRF first child, one error box `#expected-tome-error` above the form.

### Guardrails

- **Lock scope (Rule #22):** no new `FOR UPDATE`. The series lock is the one `create_in` takes; wish list reads are plain reads on the same transaction.
- **Positive shapes (Rule #20):** `wishlist=1` only; `wishlist_item` a positive integer only.
- **CSP:** no inline script/style; the link is a plain `<a>`; `feedback_html_with_link` escapes the title (existing test `wishlist_feedback_escapes_the_title`).
- **i18n:** every string through `t!()`; keys in all four locales.
- **Errors:** `AppError` only; a database error propagates (logged, generic message).
- **File size (Rule #12):** `series_expected_tomes.rs` stays well under 2000 lines.
- **No formatter:** never run `cargo fmt` (it reformats the whole repository); write in the neighbouring style.
- **Heavy commands:** prefix with `flock -w 10800 /tmp/station-heavy.lock mem-guard …` (full `cargo test`, `cargo clippy --all-targets`, the first build of the worktree, `./scripts/e2e-reset.sh`, the Playwright suite).

### Couplings (Rule #24)

- 20.1 (in development in parallel) changes `src/routes/wishlist.rs` list / detail / delete-modal code and `WishlistItemModel` delete paths. 20.2 only adds connection variants in `models/wishlist.rs`. 20.1's E2E data must not match 20.2's dedup.
- 20.3 rebases on 20.2: it touches `update_series` / `edit_series_form` in `src/routes/series.rs`; 20.2 touches `series_detail_page`, `SeriesDetailQuery` and `ExpectedTomeLabels` only.
- Locale keys only in the `expected_tome:` block.
- `sprint-status.yaml`: this story's line plus `last_updated` only (Rule #16).

### Testing

- **Unit** (in `series_expected_tomes.rs`): the title prefill in each locale (series name and position present, no raw key); the query parser accepts `wishlist=1` only (`"1"` → mode, `"true"`, `"0"`, `""`, absent → plain).
- **DB integration** (`tests/series_gap_wishlist.rs`, declared in `tests/integration.rs`, using `crate::common`; fixtures as in `tests/expected_tomes_web.rs`: `add_series`, `isbn(n)`, `librarian(pool)`, `Browser::send`; the test instance renders French): the combined create writes one tome and one item (notes = the note, ISBN normalised) and one `wishlist_add` audit row, and redirects to `/series/{id}?wishlist_item={item}`; dedup by ISBN and by title + note writes nothing (neither tome nor item); an ISBN carried by a live title writes nothing and attaches nothing (no new `title_series` row); a refusal (blank title, invalid ISBN, position out of range, position held, position taken) answers 422 with the message under its field and writes nothing; `wishlist=0` / `wishlist=true` take the plain path (tome only); a deleted series answers 404; Anonymous is refused; the series page shows the success entry for a live `wishlist_item` and nothing for a removed, unknown or non-numeric one. **Failed wish list insert leaves no tome:** in the `#[sqlx::test]` database create a `BEFORE INSERT ON wishlist_items` trigger that `SIGNAL SQLSTATE '45000'`, post the combined form, assert the database error answer and no live tome. **Lock scope (Rule #22):** hold an open transaction on a second connection that locks series B (`lock_series_in`) and inserts a tome there; on the first connection `SET SESSION innodb_lock_wait_timeout = 2` and run the combined create on series A: it succeeds (pattern: `tests/volume_add_races.rs`, `tests/proposal_series_update.rs`).
- **E2E** (`tests/e2e/specs/journeys/series-gap-wishlist.spec.ts`, `specIsbn` id `"GW"` — not used by any spec today; series names carry a run stamp like `series-expected-tomes.spec.ts`): from a closed series of total 3 created by the spec (no title needed: a closed series with no title shows gaps 1–3), use "Add to wish list" on gap 2; check the pre-filled title, then save; check the series feedback and its link, the gap now named (`[data-expected-tome]` with the title), and the entry on `/wishlist`. **Existing-entry answer:** delete that expected tome through its modal (the wish list entry stays), use "Add to wish list" on gap 2 again and save the pre-filled title: the same title + note now match, nothing is created, the answer links to the first entry (see Validation V5 — a different gap never matches). **Refusal:** blank the title, submit, assert the message in `#expected-tome-title-error` and the other typed values kept. Axe on the wish-list-mode form. `loginAs`, no `waitForTimeout`, assert only on own rows (Rule #23), `npx tsc --noEmit`, truth gate `CI=true npx playwright test --workers=2`.

### References

- `_bmad-output/planning-artifacts/epics.md` — Epic 20, D4–D6, Story 20.2, couplings.
- Epic plan (copy table "20.2 Gap to the wish list").
- `CLAUDE.md` — "Expected tomes" Key Pattern ("Add to wish list" dedups on ISBN, or title + note without ISBN); Rules #3, #20, #21, #22, #23, #24.
- `docs/error-message-style.md`.

## Validation (2026-11-26, against `main` at a608f56)

Checked against the code: `templates/components/series_gap_grid.html`, `templates/pages/series_detail.html`, `templates/pages/expected_tome_form.html`, `templates/components/expected_tome_actions.html`, `src/routes/series_expected_tomes.rs`, `src/routes/series.rs` (`series_detail_page`, `SeriesDetailQuery`, `ExpectedTomeLabels`), `src/services/expected_tomes.rs`, `src/models/expected_tome.rs`, `src/models/wishlist.rs`, `src/routes/wishlist.rs`, `migrations/20260518214921_create_wishlist_items_table.sql`, `tests/expected_tomes_web.rs`, `tests/e2e/specs/journeys/series-expected-tomes.spec.ts`, `tests/e2e/helpers/isbn.ts`, `locales/en.yml`.

### Facts found

- **V1 — Story file name.** The sprint-status key is `20-2-series-gap-to-wishlist`, so the file is `20-2-series-gap-to-wishlist.md` (what `dev-story` looks up); the branch stays `story/20-2-gap-to-wishlist`.
- **V2 — Only closed series show plain gaps.** `SeriesService::position_grid` returns assigned positions only for an open series; a closed series builds `1..=total`. The new action therefore appears on closed series with a total, like "Name this tome" today. No change needed.
- **V3 — `create_in` attaches an owned title.** `ExpectedTomeService::create_in` ends with `settle_with_owned_title(…, attach = true)`: when a live title carries the typed ISBN and is not in the series, it is **attached at the gap** (`TitleSeriesModel::assign_in`) and the tome settled. AC 6 ("nothing is added") therefore needs the owner check **before** `create_in`. Fixed in Tasks: series lock first (the same `lock_series_in` by PK that `create_in` takes — no new kind of lock), then the owned-ISBN and dedup reads, then the inserts. Taking the series lock before the dedup read also serialises a double submit on the same series: the second one sees the first one's entry.
- **V4 — Refusals are not next to their field today.** `expected_tome_form.html` shows every refusal in one `#expected-tome-error` box above the form; the epic plan's reference ("as `render_form` already does") re-renders with the error but not under a field. AC 7 and Rule #3 need a per-field slot. Fixed in Tasks with a pure `ExpectedTomeError::field()` mapping. `#expected-tome-error` is asserted by `tests/expected_tomes_web.rs` (lines 289, 621) and `series-expected-tomes.spec.ts` (line 142); whether the plain form moves too is open question Q3.
- **V5 — The plan's E2E dedup step cannot match.** It adds gap 2, then expects "a second attempt on another gap with the same title" to hit the existing entry; but the note (`expected_tome.wishlist_note`) carries the position, so title + note never match on another gap. Fixed in Testing: delete the expected tome (the entry stays), then send gap 2 again.
- **V6 — The note is in the request locale.** `wishlist_note` and the title prefill are rendered in the librarian's language, so the title + note dedup misses across UI languages (an entry added in French is not found from an English session). Same limitation as the 11-11 path; out of scope, not a regression.
- **V7 — Widths agree.** `wishlist_items.title` is `VARCHAR(500)`, `MAX_TITLE_CHARS` is 500, `isbn` `VARCHAR(20)` holds the ISBN-13 from `normalize_isbn`. The tome's validated title fits the entry.
- **V8 — Lock scope.** The combined create adds no new `FOR UPDATE`. `create_in` already contains `position_holder_in` (`FOR UPDATE` on `title_series` joined to `titles`); it predates Rule #22 and is unchanged here; the lock-scope test covers the combined create as a whole.
- **V9 — 404 answer.** A series deleted meanwhile goes through `refused` → `into_app_error` → `AppError::NotFound`, a 404 with the feedback body — the same answer as the plain form (`a_deleted_series_hides_its_tome_routes`). "The not found page" in AC 8 means that answer.
- **V10 — Grid label with a parameter.** `ExpectedTomeLabels::new(loc)` is built once in `series_detail_page` (its only caller) and holds plain strings; the position-bearing aria-label needs the locale kept on the struct and a method called from the template (Tasks). The visible text "Add to wish list" also exists as `wishlist.add_to_wishlist` (same EN/FR); the plan's key `expected_tome.gap_add_to_wishlist` is kept for block ownership.
- **V11 — Success entry on load.** `#series-feedback` is `aria-live="polite"`; content present at page load is not announced as a change, but it is visible and read in page order. Focus is not moved (AC 10). No change needed.
- **V12 — Spec id.** `"GW"` is not used by any `specIsbn` call; character codes modulo 100 are unique per letter, so it cannot collide.
- **V13 — The title prefill names the pre-filled position.** If the librarian changes the position, the title still names the old one (editable, D6), while the note uses the submitted position. Accepted by D6; no change.
- **V14 — sprint-status.** Only the `20-2-…` line and the header comment changed (Rule #16); the YAML `last_updated:` key has not been maintained since story 11-15 and is left alone. `epic-20` stays `backlog` (20.1 is the first story and is on another branch).

### Open questions (product decisions)

**Approved by the project lead, 2026-11-26: Q1 (a), Q2 (a), Q3 both modes, Q4 fallback — all as recommended.**

- **Q1 — Where the "already on the wish list" answer shows (AC 5).** Not said by D4–D6 or the plan. Options: (a) re-render the form (200) with the info message `expected_tome.wishlist_already` and the "Open the wish list entry" link above the form, values kept; (b) redirect to the series page with the entry in `#series-feedback` (needs a second query value to tell "added" from "already"). *Recommendation: (a)* — nothing changed, the librarian may want to fix the title or ISBN, and no new URL parameter is needed.
- **Q2 — The copy for an ISBN a live title already holds (AC 6).** The plan names "the existing 'already in your catalog' message", i.e. `expected_tome.wishlist_owned`; but its text says "Its title holds this position in the series", which is false here (the position is a gap). Options: (a) `error.isbn.already_used` ("“%{title}” already has this ISBN. Check the number, or open that title.") under the ISBN field, the title as a link — existing copy, a field refusal (stories 17-3 / 19-4; `middleware::htmx::field_refusal_with_link` is the HTMX variant, this full-page form renders the same text with `utils::message_with_link_html` + `LINK_PLACEHOLDER` in the field slot); (b) a new key. *Recommendation: (a).*
- **Q3 — Field-level errors in the plain form too.** The template is shared; moving the messages under their fields in both modes changes the plain "New / Edit expected tome" form (and two Rust asserts and one E2E assert on `#expected-tome-error`). *Recommendation: both modes* — one template, Rule #3 applies to the plain form as well; `#expected-tome-error` stays for the messages with no field (version conflict, settled).
- **Q4 — Heading when the position is not a valid number on re-render.** `form_title_wishlist` needs a position. *Recommendation:* fall back to the existing `wishlist.add_to_wishlist` ("Add to wish list") as heading; no new copy.

## Review pass 1 (2026-11-26)

| # | Severity | Finding | Outcome |
|---|---|---|---|
| 1 | Medium | `?wishlist_item=` built the success entry for anyone: an anonymous visitor could read wish list titles by id. | Fixed: shown to Librarian+ only, and only when the entry's note is this series' wish list note (`is_series_wishlist_note`: exactly `wishlist_note(series, N, loc)` for a digit run N and one of the four languages, positive shape). Tests: `the_success_entry_is_for_a_librarian_and_this_series_only` (anonymous → empty, another series' entry → empty), unit `only_this_series_wishlist_note_matches`. |
| 2 | Medium | Field refusals after the full-page re-render were not announced. | Fixed: `autofocus` on the first `aria-invalid` input (position → title → ISBN). Tests: `the_first_invalid_field_only_takes_the_focus` (one `autofocus`, on the refused field, both modes; none before a refusal); E2E refusal case asserts the title is focused. |
| 3 | Low | `create_in` still attached an owned title (`attach = true`); "nothing attached" rested on the earlier read. | Fixed: `ExpectedTomeService::create_unattached_in` (shared `create_with(…, attach)`), used by `create_wished`. Test: `an_unattached_create_never_attaches_an_owned_title` (with the attaching variant as control). |
| 4 | Low | A double click on Save showed "already on your wish list". | Fixed: `data-submit-once` on the form; `mybibli.js::initSubmitOnce` blocks a second submit and disables the submit buttons, re-enabled on `pageshow` from the back/forward cache. No new copy. Tests: `the_form_is_sent_once` (hook present); E2E saves with a double click and expects the success entry. |
| 5 | Low | `tests/expected_tomes_web.rs` no longer asserted the escaped apostrophe. | Fixed: asserts `L&#x27;ISBN doit être` again. |
| M3 | Medium | The accessible name "Add tome N to the wish list" does not start with the visible "Add to wish list" (WCAG 2.5.3). | Open: needs a copy decision from the project lead; left as is. |
| L3, L5 | Low | — | Accepted by the reviewer; no change. |

Gates after the fixes: `cargo clippy --all-targets -- -D warnings` clean; integration `series_gap_wishlist::` `expected_tomes_web::` `expected_tomes::` `locale_parity::` `every_test_file_is_declared` `templates_audit` `series` `wishlist` — 158 passed; lib `series_expected_tomes` `expected_tomes` `routes::series` `templates_audit` — 63 passed; `tsc --noEmit` and the waitForTimeout grep pass. The two series E2E specs are not re-run yet (stack lent to story 19-8).

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

- Askama passes a method argument by reference: `ExpectedTomeLabels::gap_wishlist_named` takes `&i32`.
- Field slots are escaped with `utils::html_escape` (`'` → `&#x27;`), so the existing refusal test now matches the text after the apostrophe.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- `WishlistItemModel::{create_in, find_by_isbn_in, find_without_isbn_in}`; the pool variants acquire a connection and delegate (one INSERT, one query per lookup).
- `ExpectedTomeError::field()` → `TomeField` (pure, unit tested); `FormErrors::of` places a refusal under its field, else in `#expected-tome-error` (Q3: both modes). Field messages are escaped HTML so the owned-ISBN refusal (Q2) can carry its title link (`message_with_link_html` + `LINK_PLACEHOLDER`).
- Wish list mode (`wishlist=1` only, `is_wishlist_mode`): heading `form_title_wishlist` (Q4 fallback `wishlist.add_to_wishlist` without a valid position), title prefill (D6), help line, save label, hidden field; a re-render keeps the mode and uses `/series/{id}/expected-tomes/new?wishlist=1` as page URI.
- `create_wished`: pure checks → `lock_series_in` (PK, same lock as `create_in`) → owned ISBN (422 under the ISBN field, rollback) → shared dedup `existing_wish` (200, form re-rendered with `#expected-tome-notice`, Q1) → `create_in` → `WishlistItemModel::create_in` → commit → audit + `info!` → 303 `/series/{id}?wishlist_item={item}`. No new `FOR UPDATE`.
- `wish_tome` (the 11-11 button) now uses the same `existing_wish`, `wish_already_html`, `wish_added_html`.
- Series page: `parse_wishlist_item` (ASCII digits, > 0) → live item → `wish_added_html` inside `#series-feedback`.
- Lock-scope test bounds the HTTP send with a 5 s `tokio::time::timeout` instead of `SET SESSION innodb_lock_wait_timeout` (the request runs on a pool connection the test cannot configure); the server's wait timeout (50 s) is far above the bound.
- E2E spec written (`series-gap-wishlist.spec.ts`), `tsc --noEmit`, waitForTimeout grep and `vcode-literals.sh` pass. E2E run (image rebuilt `--no-cache`, binary checked for the new form markup): the two series specs 7/7; after a fresh reset, the full suite `CI=true --workers=2` 505 passed, 2 skipped, no flaky test, no fix needed. It uses no ISBN (every row hangs on a run-stamped series), so `specIsbn("GW")` is not called. The existing `series-expected-tomes.spec.ts` refusal now reads `#expected-tome-isbn-error` (Q3).
- Gates: `cargo clippy --all-targets -- -D warnings` clean; `cargo test --lib` 1957 passed; integration `series_gap_wishlist::` (12), `expected_tomes_web::`, `expected_tomes::`, `locale_parity::`, `every_test_file_is_declared`, `templates_audit`, `locale_html_safety::` — 61 passed; `series`, `wishlist`, `form_refusals` filters — 123 passed.

### File List

- `src/models/wishlist.rs`
- `src/services/expected_tomes.rs`
- `src/routes/series_expected_tomes.rs`
- `src/routes/series.rs`
- `templates/pages/expected_tome_form.html`
- `templates/components/series_gap_grid.html`
- `templates/pages/series_detail.html`
- `locales/en.yml`, `locales/fr.yml`, `locales/de.yml`, `locales/it.yml`
- `tests/series_gap_wishlist.rs` (new)
- `tests/integration.rs`
- `tests/expected_tomes_web.rs`
- `tests/e2e/specs/journeys/series-gap-wishlist.spec.ts` (new)
- `tests/e2e/specs/journeys/series-expected-tomes.spec.ts`
- `static/js/mybibli.js`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

### Change Log

- 2026-11-26 — code review pass 1: findings 1–5 fixed, M3 open (copy decision), L3/L5 accepted; E2E re-run pending.
- 2026-11-26 — E2E green (505 passed, 2 skipped); status review.
- 2026-11-26 — dev-story: gap → wish list form (tome + entry in one transaction), shared wish list dedup, field-level refusals in the expected-tome form, success entry on the series page. E2E pending.
