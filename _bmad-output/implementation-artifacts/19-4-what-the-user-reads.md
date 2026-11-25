# Story 19.4: What the user reads (closes #519, #538, #712; #689 items 1 and 3; #679 items 6–7)

Status: ready-for-dev

## Story

As a librarian,
I want the numbers, errors and focus on screen to be right,
so that I trust what the page tells me.

## Context

The story collects five small display faults and the error copy that the epic 17 and epic 18 retrospectives promised:

- **#519** — `/series` counts the owned and gap columns from **assignment rows** (`SeriesModel::active_count_titles`, one query per row, `src/routes/series.rs:85`). The dashboard, the MCP tools and `list_summaries` count **distinct positions within 1..=total** (`filled_positions_join!`, `src/models/series.rs:91`). Example: a closed series of 3 holding positions 1, 5 and 6 shows "3 owned, 0 gaps" on `/series`, but 2 gaps everywhere else.
- **#689 item 1** — the feedback regions sit at the top of the page. An error answered to a control low on a long page (for example, the expected-tome actions under the series grid) lands out of sight.
- **#689 item 3** — 42 OOB updates target `feedback-list` with the default `OobSwapMode::Replace` (`hx-swap-oob="true"`, i.e. outerHTML; admin routes, proposals, saved searches). They replace the region with a bare `<div id="feedback-list">`, which drops `aria-live`, `aria-relevant` and the classes. Later entries are then no longer announced.
- **#538** — after a successful rejection on `/proposals/{id}`, the answer swaps `#proposal-detail-body`. That removes the Reject button, and `modal.js` cannot restore focus to it, so focus falls to `<body>`. Accept and "remove line" have the same shape: their answer also swaps the body and drops the trigger.
- **#679 item 6** — `routes/titles.rs::update_title` still answers an empty title (`error.title.required`, :738) and an unknown media type (`error.media_type.invalid`, :885) with `AppError::BadRequest`. Since 18-5 that message is a feedback entry at the top of the page, not next to the field.
- **#679 item 7** — `error.isbn.already_used` ("… — refusing to create a duplicate") is worded for creation, but the edit form shows it too. It does not say which title holds the ISBN.
- **Retro epic 18 Q2** — `error.reference_data.name_taken` ("'X' already exists") and its siblings say neither why nor what to do (Rule #9).
- **#712** — the UX-DR8 modal, seven modal fragments and the connection-lost overlay use `bg-black bg-opacity-50`. `bg-opacity-*` no longer exists in Tailwind v4, so the backdrop is solid black and hides the page instead of dimming it.

Practice A3 (patch v1.28.1 retro): the copy and the two visible behaviours below (D2, D4, D5) are settled with the project lead **before** the first review pass.

## Decisions

- **D1 (#519) — one counting rule.** The `/series` list reads its rows with the owned count from `filled_positions_join!`: one query for the page, with no per-row query. The page keeps its sort (name), its paging and its columns. Reuse `SeriesModel::list_summaries(pool, "", false, page)`, which already returns `owned_count` from the shared join, or a sibling query that returns full `SeriesModel` rows if the template needs fields `SeriesSummaryRow` lacks. The gap column stays `total − owned` for closed series with a total, and 0 otherwise (`compute_gap`, now fed the right `owned`). The **series detail** header already counts the filled cells of its grid. Check that it agrees with `series_gaps` on the #519 example; if it does not, take `owned_positions` / `gap_count` from `services::series::series_gaps`.
  - Out of scope, unchanged: the `total_below_owned` guard of `SeriesService::update_series` (a hand edit keeps its own rule, see story 15-4 D5) and the guard of `delete_series` (rows, rightly: any assignment blocks a delete). Comment on #519 when closing it.
- **D2 (#689 item 1) — a late error is brought into view, focus untouched.** *(Approved by the project lead, 2026-11-25.)* When an `error` or `warning` entry lands in a `#feedback-list` region (main swap, OOB, or the `htmx:responseError` insert of 18-5) and the entry is not in the viewport, `static/js/mybibli.js` scrolls it into view (`scrollIntoView({ block: "nearest" })`; smooth only without `prefers-reduced-motion`). Focus stays where it was (Rule #21), and success and info entries never scroll. On `/catalog` the region is next to the scan field, so nothing moves while scanning. The alternative, a fixed region over the page, was rejected because it covers content and adds a second toast system.
- **D3 (#689 item 3) — OOB updates of `feedback-list` keep the region.** A `Replace` update whose target is `feedback-list` is emitted as `hx-swap-oob="innerHTML"`. The region element, with its `aria-live`, `aria-relevant` and per-page classes, stays in place. Do this in one place, `HtmxResponse::body`, or with a named mode (`OobSwapMode::Inner`) set by a helper `OobUpdate::feedback(content)` used at every call site. Add a test that no `feedback-list` OOB update renders `hx-swap-oob="true"`. `Append` (beforeend) is unchanged.
- **D4 (#538) — focus after a decision.** *(Approved by the project lead, 2026-11-25.)* Use the 19-1 mechanism (`data-modal-focus-fallback` on the trigger, read by `modal.js::focusFallbackOf`):
  - Accept and Reject → `#proposal-back-link`, the "← Back to the list" link above the heading. Give it this id; it is outside the swapped body, so it is always there.
  - "Remove line" → the first remaining line action of the list else `#proposal-back-link`. The back link comes first in document order, so one CSS selector list would always pick it: `focusFallbackOf` learns an ordered list, `data-modal-focus-fallback="#proposal-items [data-remove-line], #proposal-items [data-action='inline-form-add-toggle'] || #proposal-back-link"` (alternatives separated by ` || `, tried in turn; a value without ` || ` behaves as in 19-1).
  - `modal.js` may need the ordered fallback above; it must keep the 19-1 tests green.
- **D5 — copy (Rule #9, four locales).** *(Approved by the project lead, 2026-11-25; EN and FR shown, DE and IT follow them.)*
  - `error.reference_data.name_taken` (conflict; a trashed entry with that name is reactivated automatically by the model, so the colliding entry is always live and there is nothing to reactivate):
    - EN: "“%{name}” was not saved: this list already has an entry with that name. Choose another name, or use the existing entry."
    - FR: « « %{name} » n'a pas été enregistré : cette liste contient déjà une entrée de ce nom. Choisissez un autre nom, ou utilisez l'entrée existante. »
  - Siblings in `error.reference_data`:
    - `version_mismatch`:
      - EN: "Your change was not saved: another administrator changed this entry meanwhile. Reload the page, then make your change again."
      - FR: « Votre modification n'a pas été enregistrée : un autre administrateur a modifié cette entrée entre-temps. Rechargez la page, puis refaites votre modification. »
    - `invalid_version`:
      - EN: "Your change was not saved: the page is out of date. Reload it, then make your change again."
      - FR: « Votre modification n'a pas été enregistrée : la page n'est plus à jour. Rechargez-la, puis refaites votre modification. »
    - `node_type_name_too_long_for_cascade`, a field hint (terse), dropping the jargon `storage_locations`:
      - EN: "Name must be at most %{max} characters."
      - FR: « Le nom ne peut dépasser %{max} caractères. »
  - `error.user.username_taken`:
    - EN: "The account was not saved: the username “%{username}” is already taken. Choose another username."
    - FR: « Le compte n'a pas été enregistré : le nom d'utilisateur « %{username} » est déjà pris. Choisissez-en un autre. »
  - `error.user.version_mismatch`, worded like `reference_data.version_mismatch` with "account" / « compte ».
  - `error.isbn.already_used` (new arguments `title`; the title is a link to `/title/{id}`). It is one key, correct on both the creation and the edit form:
    - EN: "“%{title}” already has this ISBN. Check the number, or open that title."
    - FR: « « %{title} » porte déjà cet ISBN. Vérifiez le numéro, ou ouvrez ce titre. »
  - `error.title.required` and `error.media_type.invalid` keep their text as field hints. Only where they show changes (D6).
- **D6 (#679 items 6–7) — where the refusals show.**
  - **Edit form.** An empty title → `field_refusal(BAD_REQUEST, "edit-title-error", …)`, and an unknown media type → `field_refusal(BAD_REQUEST, "edit-media-type-error", …)`. Add the two slots (`<div id="…" data-field-slot aria-live="polite">`) under the fields in `templates/fragments/title_edit_form.html`. `edit-title` already has `aria-describedby="edit-title-tome-hint"` when a hint shows, so list both ids. `static/js/title-form.js` already empties every `[data-field-slot]` a field names in its `aria-describedby` on `input` (selects fire `input` too), so no JS change is needed; the `aria-describedby` is what wires it.
  - **Edit form, ISBN.** The `error.isbn.already_used` refusal stays in `edit-isbn-error`, now with the link. `field_refusal_html` escapes its message, so add a sibling (e.g. `field_refusal_link(status, slot, message, href, link_text)` / `field_refusal_html_with_link`). Everything is escaped except the `<a>` it builds itself; `href` is `/title/{id}`, built from an integer. The link text is the title name, so the message carries it once, as the link (no `%{title}` duplicated next to it).
  - **Manual creation form.** It keeps its feedback-list entry (#679 item 8 stays out of scope), now naming and linking the holder. `TitleService::create_manual` must hand back the holder (id + name) instead of a finished string, e.g. a dedicated error variant or a pre-check in the route. Build the entry with the existing link-in-feedback pattern (see `routes/catalog_existing_label.rs` for the #585 "other title" answer), escaping the name.
- **D7 (#712) — backdrops.** Replace `bg-black bg-opacity-50` with `bg-black/50` in the nine sites (`grep -rln "bg-opacity-" templates`: `components/modal.html`, `layouts/base.html` overlay, `fragments/admin_api_keys_{delete,created,revoke}_modal.html`, `admin_ref_{delete,loanable_warning}_modal.html`, `cover_upload_modal.html`, `saved_search_rename_modal.html`). Add a new `templates_audit` test refusing Tailwind v3-only utilities, which v4 dropped, in templates, Rust-built markup (production part, as the 19-1 audits read it) and `static/js` (vendored and `.min.js` excluded): `bg-opacity-`, `text-opacity-`, `border-opacity-`, `divide-opacity-`, `ring-opacity-`, `placeholder-opacity-`, `flex-shrink-`, `flex-grow-`, `overflow-ellipsis`, `decoration-slice`, `decoration-clone`. Match on a class-token boundary (so `shrink-0`, `grow` and `text-ellipsis` pass), with a self-test like `rust_markup_patterns_match_markup_not_code`.

## Acceptance Criteria

1. **AC1 (#519)** — On `/series`, a closed series of total 3 holding positions 1, 5 and 6 shows owned 1 and gaps 2. A position held twice counts once, and an omnibus counts each position it covers. The list issues one query per page, not one per row (integration test on the model, plus a route test on the rendered columns). The dashboard, `/series` and MCP `list_series` agree on the same data (integration test).
2. **AC2 (#689 item 1)** — An `error` or `warning` entry that lands in `#feedback-list` out of the viewport is scrolled into view, and `document.activeElement` is unchanged. A success entry does not scroll. E2E covers this: on a long series page, provoke a refusal from a control below the fold, assert the entry is in the viewport (`expectReallySeen(…, { inViewport: true })`) and that focus is still on the control.
3. **AC3 (#689 item 3)** — After an admin save answered with a `feedback-list` OOB update, the region still has `aria-live="polite"` and `aria-relevant="additions"`. A unit test covers `HtmxResponse::body`; an E2E case saves twice on an admin tab and asserts both attributes after each save.
4. **AC4 (#538)** — After a successful rejection, `document.activeElement` is `#proposal-back-link`, not `<body>`; the same holds after an acceptance. After a line removal, focus is on a line action, or on the back link when no line action is left. E2E covers rejection and removal; acceptance is covered where an existing accept spec runs.
5. **AC5 (copy)** — The keys of D5 read as approved, in all four locales (`locale_parity` green). Each was reviewed against `docs/error-message-style.md`: conflicts follow "what happened → why → what to do", field hints stay terse.
6. **AC6 (#679 item 6)** — On the edit form, submitting an empty title shows `error.title.required` in `#edit-title-error`, next to the field, and submitting an unknown media type shows `error.media_type.invalid` in `#edit-media-type-error`. In both cases the form stays, what was typed stays, focus does not move, and nothing is saved. Route tests cover the `HX-Retarget` / status / body. E2E (Rule #3) submits each value: the empty title by removing `required` in the page, the media type by posting a value outside the `<select>`. It asserts the message next to the field.
7. **AC7 (#679 item 7)** — Editing a title's ISBN to one another live title holds shows, in `#edit-isbn-error`, the D5 text with the holder's name as a link to `/title/{id}`. Creating a title manually with such an ISBN shows the same text and link in the feedback entry. The name is escaped (route test with `<b>`), and E2E covers both forms.
8. **AC8 (D5 others)** — The new name-taken refusal shows on the reference-data create and rename forms, and the new username-taken refusal on the user creation form. E2E: each existing spec that asserts the old text is updated, and at least one submits a taken name and asserts the new text where it shows today.
9. **AC9 (#712)** — The modal backdrop's computed `background-color` has an alpha of 0.5, not 1. E2E covers one UX-DR8 modal and the connection-lost overlay. No template, Rust-built markup or own JS file uses a Tailwind v3-only utility (new audit test, with a self-test that it catches `bg-opacity-50` and passes `shrink-0`).
10. **AC10** — Every fix has a unit or integration test and an E2E case. Unit tests, clippy (`-D warnings`), `sqlx prepare --check`, `npx tsc --noEmit` and the `waitForTimeout` and V-code literal gates are clean, and the full E2E suite runs `CI=true --workers=2` with no flaky test.

## Tasks / Subtasks

- [ ] T1 — `/series` list from the shared rule (D1, AC1): query, route, template unchanged or adapted, tests; check the detail header against `series_gaps`.
- [ ] T2 — feedback-list OOB innerHTML (D3, AC3): `HtmxResponse::body` or helper, every call site, unit test.
- [ ] T3 — scroll late error/warning entries into view (D2, AC2): `static/js/mybibli.js`, one function used by the swap, OOB and responseError paths.
- [ ] T4 — focus fallbacks on the proposal triggers + `#proposal-back-link` (D4, AC4).
- [ ] T5 — copy (D5, AC5): four locale files; `touch src/lib.rs`; update the unit and E2E assertions on the old text (`grep -rn "already exists\|existe déjà\|already taken\|refusing to create" src tests`).
- [ ] T6 — edit-form field refusals (D6, AC6): slots, `aria-describedby`, route.
- [ ] T7 — ISBN holder named and linked (D6, AC7): `field_refusal` link variant; creation path returns the holder; escaping test.
- [ ] T8 — backdrops `bg-black/50` + Tailwind v3 audit (D7, AC9).
- [ ] T9 — E2E (AC2–AC4, AC6–AC9): extend existing specs where they own the surface (`series-feedback-region.spec.ts`, `proposals-review.spec.ts`, `admin-reference-data.spec.ts`, `admin-user-validation.spec.ts`, a title-edit spec, `connection-lost-overlay.spec.ts`), with their own data (Rule #23) and `expectHtmxProcessed` before clicking a swapped Confirm (#723).
- [ ] T10 — gates (AC10); `CLAUDE.md`: one line under Key Patterns (feedback-list OOB is innerHTML; late errors scrolled into view; Tailwind v3 audit).

## Dev Notes

### Files

- `src/routes/series.rs` (list handler :69, `compute_gap` :18), `src/models/series.rs` (`filled_positions_join!` :91, `list_summaries` :414, `active_count_titles` :336), `src/services/series.rs` (`series_gaps` :529), `templates/pages/series_list.html`.
- `src/middleware/htmx.rs` (`OobUpdate` :73, `HtmxResponse::body` :121, `field_refusal` :206), and the 11 files with `target: "feedback-list"` (`grep -rln 'target: "feedback-list"' src`).
- `static/js/mybibli.js` (responseError insert :137, `serverFeedbackEntry` :194), `static/js/modal.js` (`focusFallbackOf` :215).
- `templates/pages/proposal_detail.html` (back link :14), `templates/fragments/proposal_detail_body.html` (Accept :37, Reject :45, remove line :286).
- `src/routes/titles.rs` (`update_title` :725; ISBN guard :797; `EDIT_ISBN_SLOT`), `src/services/title.rs` (`create_manual` ISBN check :318), `src/routes/catalog_title_form.rs` (error arm :188), `templates/fragments/title_edit_form.html`, `static/js/title-form.js`.
- `src/routes/admin_reference_data.rs` (`map_create_or_rename_conflict` :333), `src/routes/admin_users.rs` (:502).
- `src/templates_audit.rs` (1548 lines: room for one more test; stay under 2000, Rule #12).

### Guardrails

- Rule #21: no fixed focus target. D4 is a fallback used only when the trigger left the page; D2 never touches focus.
- CSP: no inline style or handlers; the link in a refusal is plain `<a href>`; escape every name (`html_escape`), never `|safe` on user data.
- Rule #20 does not apply (nothing rewrites data). Rule #22: no new `FOR UPDATE`.
- `t!()` with `locale = loc` everywhere (#661); all four locale files; DE `„…“`, IT `«…»`.
- E2E: own rows only (Rule #23); `specIsbn` / `specVcode` with a new spec id only if a new spec file is created (add it to `VCODE_BLOCKS`); no `waitForTimeout`.
- Heavy commands under `flock -w 10800 /tmp/station-heavy.lock mem-guard …`; never run `cargo fmt`.

### Gates

`cargo clippy --all-targets -- -D warnings`; `cargo test` (lib) and `cargo test --test integration` on the touched modules plus `locale_parity::`; `cargo sqlx prepare --check --workspace -- --all-targets` if a query changed; `cd tests/e2e && npx tsc --noEmit`; the full E2E suite on a reset stack with `CI=true --workers=2`.

### Previous story (19-3) intelligence

- A modal swapped into `#modal-slot` is wired by HTMX about 20 ms later: wait with `expectHtmxProcessed` on `[data-modal-confirm]` before clicking (`helpers/catalog.ts::trashVolumeFromTitlePage` is the model; #723 lists the specs that still race).
- `closing_modal` / `HX-Trigger-After-Swap: modal-close` closes a Pattern A modal after the swap; the proposal routes already use it (`proposals.rs:966`).
- Chromium applies `setOffline(false)` asynchronously (see `connection-lost-overlay.spec.ts`) — relevant to AC9's overlay check: assert on the overlay while it is shown, before restoring the network.

### References

- Epic: `_bmad-output/planning-artifacts/epics.md`, Story 19.4 (:3689) and the epic 19 decisions (:3629).
- Issues: #519, #689, #538, #679, #712. Rule #9 guide: `docs/error-message-style.md`.

## Validation (2026-11-25)

Checked against the code: the series list, `filled_positions_join!` and `list_summaries` lines; the 11 files with `feedback-list` OOB updates; the proposal triggers (the edit button is `data-action="inline-form-add-toggle"`, the list is `#proposal-items`) — D4 corrected to an ordered fallback, since the back link precedes the list; `title-form.js` already clears any slot named by `aria-describedby` — D6 and T6 corrected (no JS change); the nine `bg-opacity-50` sites; `templates_audit.rs` at 1548 lines. D2, D4 and D5 approved by the project lead before development (practice A3).

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
