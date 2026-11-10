# Story 17.2: Add a volume from the title page

Status: done

GitHub issue: #652 (type:change-request). Covers: FR173, UX-DR43. Epic 17; ships in v1.27.0.

## Story

As a librarian on a title's page,
I want to add a volume by its V-code there,
so that I do not have to go back to `/catalog` and scan the ISBN again.

## Decisions

- **D1 — one V-code arm (DRY).** The part of the `/catalog/scan` V-code arm that does not depend on the catalog session moves to `src/routes/vcode_volume.rs`: reading the input (`VolumeService::normalize_vcode` + `validate_vcode`, refused with `feedback.vcode_invalid`), the existing-label answer (#585, `catalog_existing_label::existing_label_response`), the CR #300 confirmation modal, `VolumeService::create_volume` and the mapping of its refusals (label taken, label in the Trash with loans, race, title gone). The catalog arm and the new title-page handler both call it; each renders its own success.
- **D2 — the title page leaves the `/catalog` session alone.** The page names its title, so the handler never reads or writes `current_title_id`, and does not touch `last_volume_label`, the session counter, the active (batch) location or the scan-undo log. Why: those belong to a scanning run on `/catalog`; silently shelving a volume added from another page at the batch location of an earlier run is the kind of hidden attachment CR #300 exists to stop, and the title page has no Undo to offer. An existing label on the title page therefore always gets the #585 answer (same title → info; other title → warning + link to `/volume/{id}`), never the shelve path.
- **D3 — CR #300 is kept** as the spec asks: when the title already has volumes, a new label opens the same UX-DR8 confirmation modal; its Confirm posts back to the title page's endpoint with `confirmed=true`. The modal template takes its action URL and swap target instead of hard-coding `/catalog/scan`.
- **D4 — the field takes V-codes only.** Anything that is not `[vV]` + 4 digits (an ISBN, a word, `V0000`) gets `feedback.vcode_invalid` (Rule #9: it names the expected shape). Nothing else is rewritten (Rule #20).
- **D5 — request locale.** Every message of the shared arm is rendered in the request locale (`locale = loc`), including the service refusals (`VolumeService::create_volume_in(…, loc)`). This also closes the `vcode_invalid` half of #661 item 5 on `/catalog`; `code_unsupported` there gets the locale too (one line, same item).
- **D6 — scanner behaviour shared, not copied.** The field is a `data-mybibli-scan-field` input; `static/js/scan-field.js` reads its post URL, target and swap from data attributes (defaults = `/catalog/scan`, `#feedback-list`, `afterbegin`), so Enter handling, the V-code pre-check, the 1 s duplicate drop and the clearing of the field are the catalog's own. The answer lands in a live region under the field (`#title-volume-add-feedback`, replaced on each answer); the volumes list (`#title-volumes-list`) and its count are refreshed out of band. The field itself is never swapped, so focus stays in it (Rule #21); after the CR #300 modal closes, focus returns to the field that posted, only when nothing else holds focus.
- **D7 — the empty-state link to `/catalog` is kept unchanged**; ISBN entry on `/catalog` is unchanged. The issue's optional "open `/catalog` with the title active" is not done: the field covers the need.

## Acceptance Criteria

1. **AC1.** On `/title/{id}`, a Librarian or Admin sees a V-code field in the volumes section, with or without volumes; Anonymous sees none. A valid new V-code (any case) creates a volume on this title.
2. **AC2.** Same validation and messages as the catalog arm: invalid shape → `vcode_invalid`; existing label of this title → info; of another title → warning + link to `/volume/{id}`; title with volumes → CR #300 modal, Confirm creates; label in the Trash → #442 rules.
3. **AC3.** On success the volumes list and count update without reload; focus stays in the field; the `/catalog` session (current title, last label, counter, active location) is untouched.
4. **AC4.** CSP-clean (no inline script/style/handlers); messages in the request locale; new keys in en/fr/de/it.
5. **AC5.** Unit tests (input reading, modal action URL), DB integration (`tests/title_volume_add.rs`), E2E (`title-volume-add.spec.ts`: scanner burst on a title without volumes, CR #300 confirm, an existing label of another title, Anonymous sees no field, axe).

## Tasks

- [x] `src/routes/vcode_volume.rs`: shared arm; `catalog.rs` V-code arm calls it.
- [x] `VolumeService::create_volume_in` with a locale.
- [x] `VolumeConfirmAddModalTemplate`: `action_url`, `hx_target`, `hx_swap`.
- [x] Volumes list fragment (`templates/fragments/title_volumes_list.html`) shared by the page and the HTMX answer.
- [x] `POST /title/{id}/volumes` (`src/routes/title_volume_add.rs`), Librarian+.
- [x] `scan-field.js` data attributes + focus return after a modal.
- [x] Locale keys (field label, hint) in four locales.
- [x] Unit, DB integration, E2E.

## Dev Notes

- Catalog V-code arm: `src/routes/catalog.rs` `"vcode" =>` (shelve-existing at active location, title context, #441 deleted title, #585, CR #300, create, auto-shelve, OOBs).
- `routes/titles.rs` is at 1804 lines (Rule #12): new code goes to new files.
- Templates: `templates/pages/title_detail.html` `#title-volumes`; modal `templates/fragments/volume_confirm_add_modal.html`.

## Dev Agent Record

### File List

- `src/routes/vcode_volume.rs` — the shared V-code arm (`read_vcode`, `invalid_vcode_response`, `add_to_title`, CR #300 modal) + unit tests.
- `src/routes/catalog.rs` — the V-code arm calls the shared arm; `code_unsupported` in the request locale.
- `src/routes/title_volume_add.rs` — `POST /title/{id}/volumes`, `TitleVolumesView`, list template + unit tests.
- `src/routes/titles.rs`, `src/routes/mod.rs` — the page carries `volumes_view`; route.
- `src/routes/catalog_volumes.rs`, `templates/fragments/volume_confirm_add_modal.html` — modal action URL, target, swap.
- `src/services/volume.rs` — `create_volume_in` (refusals in the request locale).
- `templates/pages/title_detail.html`, `templates/fragments/title_volumes_list.html` — V-code field; list fragment shared with the HTMX answer.
- `static/js/scan-field.js` — post URL / target / swap from data attributes; focus returns to the posting field after a modal.
- `locales/{en,fr,de,it}.yml` — `title_detail.add_volume_field`, `title_detail.add_volume_hint`.
- `tests/title_volume_add.rs` — DB integration; `tests/scan_vcode_case.rs` — reads English explicitly (the scan now answers in the request locale).
- `tests/e2e/specs/journeys/title-volume-add.spec.ts` — E2E (spec id `TP`).

### Review

- **Round 1** (Blind Hunter, Edge Case Hunter, Acceptance Auditor). Medium, fixed: the focus return after a modal went to the last scan field after *any* modal (now: the element focused before the modal, tracked with `focusin`, Rule #21; E2E: a row's Delete modal returns focus to its button); a refusal answering the CR #300 Confirm left the dialog open (`vcode_volume::closing_modal`, both pages); the header's "N volumes" and the Delete-title button stayed stale after the first add (now swapped out of band); a title deleted meanwhile answered a generic 404 (now a 200 warning in the field's region, new key `feedback.title_page_title_gone`, Rule #9); after an HTTP error the typed code was restored only into `#scan-field` (now the field that posted); the E2E V-code ranges could collide between the spec's tests (disjoint ranges). Low, fixed: the field's client ISBN check named an ISBN in English (`data-isbn-error` = `vcode_invalid`); a failed volume count skipped the confirmation (now an error); the catalog arm's success copy in the request locale too; tests tightened (anonymous → 303, last label and undo log untouched, #442 reuse and a French service refusal from the title page, refusal after Confirm). Low, deferred to #663: same-label race wording, add vs delete-title race (pre-existing), catalog context not reset when the title vanishes at the modal step, no audio on the title page, catalog Confirm replacing the feedback list (pre-existing).
- **Round 2** (combined adversarial + edge pass). Medium, fixed: the HTMX error recovery refilled and focused the title page's V-code field after an error of *any* control on the page (now only when the failed request is that field's own post; otherwise `#scan-field` as before). Low, fixed: the two remaining `volume_no_title` messages of the catalog arm in the request locale. Low, accepted: `closing_modal` duplicates modal.js's close on any 2xx Confirm answer (explicit and harmless). Low, deferred to #663: the header's out-of-band ids are lost once the metadata block is re-rendered by an edit's save/cancel (`metadata_display_html` lacks them; partly pre-existing).

### Change Log
