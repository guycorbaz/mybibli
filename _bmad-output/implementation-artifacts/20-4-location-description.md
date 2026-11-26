# Story 20.4: A description for a storage location (closes #716)

Status: in-progress

## Story

As a librarian,
I want to describe where a storage location is,
so that anyone can find "the left bookcase, top shelf, behind the door".

## Context

Usage feedback of 2026-11-25 (#716): a location has a name, a type and an L-code, but nothing that says where it physically is. Covers FR185 ("A librarian can give a storage location an optional description (where to find it), shown on the location page and returned by the API").

What exists:
- **Table.** `storage_locations` (`migrations/20260329000000_initial_schema.sql:109`): `id`, `parent_id`, `name`, `node_type`, `label`, the soft-delete quartet, plus `is_organizational` (`migrations/20260520150000_storage_locations_organizational.sql`). No description.
- **Model.** `src/models/location.rs` — `LocationModel { id, parent_id, name, node_type, label, is_organizational }`, built by hand in five readers (`find_by_id`, `find_by_label_any_state`, `find_by_label`, `find_all_tree`, `find_children`) and three unit-test literals. Every query is a dynamic `sqlx::query()`: **no `.sqlx/` change**. `update_with_locking` (l. 319) writes `name`, `node_type`, `parent_id`, `is_organizational`, guarded by `id + version + deleted_at IS NULL`, then `check_update_result`.
- **Service.** `src/services/locations.rs::update_location` (l. 161) trims the name, checks the type, the parent (cycle) and the organizational flip, then calls the model. Its refusals are `AppError::BadRequest` built with `t!()` **without locale** (instance language) — left as they are (see Validation).
- **Routes.** `src/routes/locations.rs`: `GET /locations/{id}/edit` → `edit_location_page` (l. 559, `LocationEditTemplate` l. 542); `POST /locations/{id}` → `update_location` (l. 620, `UpdateLocationForm` l. 607), which answers `Redirect::to("/locations")` on success; `GET /location/{id}` → `location_detail` (l. 58, `LocationDetailTemplate` l. 37). Test helper `loc()` l. 805, `test_location_detail_template_renders` l. 1128.
- **Templates.** `templates/pages/location_edit.html` — a plain `method="POST"` form (no HTMX) with name, type, parent, the organizational checkbox, submit `#edit-location-submit` and a hard-coded English `← Back` link (l. 66). `templates/pages/location_detail.html` — breadcrumb (`aria-label="Location path"`, hard-coded English, though `location.breadcrumb_label` exists in the four locales), then `<h1>{{ location.name }}</h1>` and `<p>{{ location.label }} · {{ location.node_type }}</p>`.
- **A refusal today** comes back as `AppError::BadRequest`: through `IntoResponse` a bare feedback fragment, not the form (epic 20 "What the issues did not say"). The pattern to copy is `src/routes/series_expected_tomes.rs::render_form` + `refused` (l. 61 / l. 208): re-render the page with the typed values and the message, status **422**, base context built on the page's own GET address.
- **API.** `src/routes/api_v1.rs::list_locations` (l. 216) maps `find_all_tree` into `ApiLocation { id, name, label, node_type, parent_id }` (l. 208). No test covers `/api/v1/locations` yet; `tests/api_v1_series.rs` is the model (`common::mcp::{router, seed_key}`).
- **MCP.** No tool reads `storage_locations` rows (`get_title` shows a volume's location path only). Unchanged (D9).
- **Backups.** The 19-6 dump takes every non-generated column of every table, so the new column is backed up with no change to `src/services/backup/`. `tests/backup_round_trip.rs` seeds one location (l. 43, `'RT room'`, `L9871`) without a description.

## Decisions

All approved by the project lead with the epic (2026-11-26):
- **D9** — the description is edited on the edit form only and shown on `/location/{id}` only (not in the tree, not searched); 500 characters, plain text; `GET /api/v1/locations` returns it; MCP unchanged.
- **D11** — copy (EN / FR from the epic; DE / IT proposed below, reviewed like the others):

| Key | EN | FR | DE | IT |
|---|---|---|---|---|
| `location.description_label` | Description | Description | Beschreibung | Descrizione |
| `location.description_help` | Optional. Where to find it, for example “left bookcase, top shelf, behind the door”. | Facultatif. Où le trouver, par exemple « bibliothèque de gauche, étagère du haut, derrière la porte ». | Optional. Wo er zu finden ist, zum Beispiel „linkes Bücherregal, oberstes Fach, hinter der Tür“. | Facoltativa. Dove trovarla, per esempio «libreria di sinistra, ripiano in alto, dietro la porta». |
| `location.description_too_long` (field) | Description must be at most 500 characters. | La description doit compter au plus 500 caractères. | Die Beschreibung darf höchstens 500 Zeichen lang sein. | La descrizione può contenere al massimo 500 caratteri. |
| `location.back_to_tree` (replaces "← Back") | Back to locations | Retour aux emplacements | Zurück zu den Standorten | Torna alle ubicazioni |

(DE "er" = der Standort; IT "Facoltativa" agrees with "descrizione", "trovarla" with "ubicazione".)

Technical choices made in this story (no visible behaviour beyond D9):
- **T-a — length is counted as the user sees it.** Browsers submit a textarea's line breaks as `\r\n`. The limit counts Unicode scalar values (`chars()`) of the trimmed text **with each `\r\n` counted as one character**, so 500 characters typed with line breaks are not refused. The text is stored as received (no line-ending rewrite, Rule #20).
- **T-b — no `maxlength` on the textarea.** The server is the authority (no textarea in the project carries one), and the E2E refusal case can type 501 characters.
- **T-c — one writer.** Only the web edit form writes the column (the API is read-only for locations, MCP has no location tool, creation does not take it — D9). The create path writes `NULL` by omission; `LocationModel::create`'s signature is unchanged.

## Acceptance Criteria

1. **AC1 — migration.** A new migration adds `storage_locations.description TEXT NULL` (additive, no default text, no index), timestamped after every migration on `main` **and** after 19-7's `20261126100000_settings_backup_retention.sql` (e.g. `20261126120000_storage_locations_description.sql`).
2. **AC2 — edit form.** `/locations/{id}/edit` shows, after the parent select and before the organizational checkbox, an optional multi-line field: `<label for="edit-description">` (`location.description_label`), `<textarea id="edit-description" name="description" rows="4">` pre-filled with the stored text (Askama-escaped), a help line `#edit-description-help` (`location.description_help`) referenced by `aria-describedby`. The create forms (root form and the tree's inline "add child" form) are unchanged.
3. **AC3 — saving.** On save the text is trimmed; blank (empty or whitespace only) stores `NULL`; anything else is stored exactly as trimmed (no other rewriting, Rule #20). Saving bumps `version` like any edit.
4. **AC4 — refusal next to the field.** More than 500 characters (T-a counting) is refused: nothing is written; the edit page is rendered again with status 422, every typed value kept (name, type, parent, organizational, description) and the version the form carried; the message `location.description_too_long`, in the **request** locale, is in `#edit-description-error` right under the textarea, which gets `aria-invalid="true"` and `aria-describedby="edit-description-help edit-description-error"`. Exactly 500 characters is accepted. (Rule #3)
5. **AC5 — location page.** `/location/{id}` shows the description in `<p id="location-description">` under the heading block (after the "L-code · type" line), escaped by Askama (never `|safe`, never HTML), line breaks kept by `whitespace-pre-line`; nothing (no element) when it is `NULL`. The tree (`/locations`) and the search do not show it.
6. **AC6 — API.** `GET /api/v1/locations` returns `description` for each location: the string, or `null` when unset. MCP is unchanged. The API chapter of the manual is updated by story 20.7 (its AC lists it; see Validation V3).
7. **AC7 — localized back link.** The edit page's "← Back" link reads `location.back_to_tree` (arrow kept, `aria-hidden="true"`); the location page's breadcrumb `aria-label` reads `location.breadcrumb_label` (existing key).
8. **AC8 — backups.** `tests/backup_round_trip.rs` seeds a location description (multi-line, with quotes and non-ASCII characters); the round trip stays green, so a backup written by 19-6 and loaded back (what 19-8 restores) keeps it.
9. **AC9 — tests.**
   - Unit: the description check (500 accepted, 501 refused, blank / whitespace → `None`, surrounding whitespace trimmed and inner text kept, 499 characters + one `\r\n` accepted); the location page escapes `<b>` and renders no `#location-description` when `None`; the edit template shows the error, `aria-invalid` and the typed text when refused; `UpdateLocationForm` parses with and without `description`.
   - DB integration (`tests/location_description.rs`, declared in `tests/integration.rs`): save, clear (`NULL`), a stale version refused with the row unchanged; the web POST of 501 characters answers 422 with the message and the typed text, row unchanged; `GET /api/v1/locations` returns the text and `null`.
   - E2E (`tests/e2e/specs/journeys/location-description.spec.ts`, new): on a location the spec created, set a two-line description, read it on `/location/{id}` (both lines), clear it and see it gone; type 501 characters, assert the message next to the field and the typed text kept; axe (`checkAccessibility`) on the edit page, in its normal and its refused state.

## Tasks / Subtasks

- [x] T1 — Migration (AC1).
- [x] T2 — Model (AC3, AC6): `LocationModel.description: Option<String>`; add `description` to the five SELECTs and their mappings (`try_get::<Option<String>, _>("description")`); `update_with_locking` gains `description: Option<&str>` and writes it; update the three unit-test literals in `models/location.rs` and `loc()` in `routes/locations.rs`.
- [x] T3 — Service (AC3, AC4): a pure `LocationService::normalize_description(raw: &str) -> Result<Option<String>, DescriptionTooLong>` (const `DESCRIPTION_MAX_CHARS: usize = 500`, T-a counting); `update_location` takes `description: Option<&str>` (already normalized) and passes it to the model. Unit tests in the service's test module.
- [x] T4 — Route (AC2, AC4, AC7): `UpdateLocationForm.description: String` (`#[serde(default)]`); `update_location` calls `normalize_description` first and on refusal re-renders the edit page (422) — one shared `render_edit_page(state, session, loc, id, EditFormState, status)` used by `edit_location_page` and the refusal path (DRY), base context on `/locations/{id}/edit`; `LocationEditTemplate` gains `description_value`, `description_error: Option<String>`, `description_label`, `description_help`, `back_label`, and takes the typed name / type / parent / organizational from the form on refusal (the L-code from the DB row). Built with `locale = loc` (request locale).
- [x] T5 — Templates (AC2, AC4, AC5, AC7): `location_edit.html` (textarea, help, error slot rendered only when `description_error` is `Some`, `aria-describedby` naming the error id only then; back link); `location_detail.html` (`#location-description`, `whitespace-pre-line`, breadcrumb label). `LocationDetailTemplate` gains `breadcrumb_label`. No inline style, no `|safe`.
- [x] T6 — API (AC6): `ApiLocation.description: Option<String>` mapped from the model.
- [x] T7 — Locales (D11): the four keys in `locales/{en,fr,de,it}.yml`, `location:` block, placed after `organizational_blocked_has_volumes` — away from the `tree:` sub-block that 20.5 edits. `touch src/lib.rs` before building; `cargo test --test integration locale_parity::`.
- [x] T8 — Backup fixture (AC8): give `'RT room'` a description in `SEED` (or an `UPDATE` with a bound multi-line value, like `TRICKY` for borrowers).
- [x] T9 — DB integration tests (AC9): `tests/location_description.rs` + `mod location_description;` in `tests/integration.rs`.
- [x] T10 — E2E spec (AC9).
- [ ] T11 — Gates: `cargo clippy -- -D warnings`, targeted tests, full `cargo test` and Playwright `CI=true npx playwright test --workers=2` once on the final code (under `flock -w 10800 /tmp/station-heavy.lock mem-guard …`); `npx tsc --noEmit`; flake gate.

## Dev Notes

- **Do not touch** (20.5 owns them): `render_tree_html`, `render_node_at_depth`, `delete_success_response` in `src/routes/locations.rs`; `templates/pages/locations.html`; `static/js/locations-tree-toggle.js`; `tests/e2e/specs/journeys/locations.spec.ts`; `tests/e2e/specs/accessibility-full.spec.ts`; the `location.tree:` locale sub-block. The tree code reads other fields of `LocationModel`; adding one is the only shared change. Docs under `docs/manual/` are 20.7's (and 19.11's) — not edited here.
- **Form re-render** — copy `series_expected_tomes.rs::render_form` / `refused`: 422 status, page URI = the GET address so the layout's language toggle and login `next` never point at the POST route. On refusal, read the location (`find_by_id`, 404 if gone) for its L-code and the lists (node types, `find_all_tree` for the parent select) again.
- **Order in `update_location`:** role check → `normalize_description` → on refusal re-render; otherwise `LocationService::update_location(…, description.as_deref())` → redirect to `/locations` as today. The other refusals of the service (blank name, invalid type, missing parent, cycle, organizational flip, version conflict) keep today's behaviour (see Validation, open question Q1).
- **Escaping.** Askama escapes `{{ }}` by default; the textarea content is `{{ description_value }}` with no whitespace between the tags (a leading newline inside `<textarea>` is eaten by the HTML parser, so `<textarea …>{{ description_value }}</textarea>` on one line). Never `|safe`; never through `body_html` or `feedback_html`.
- **CSP / audit** — `src/templates_audit.rs` checks templates (no inline style, CSRF first child of a POST form — already there) and the production Rust code; nothing new is built in Rust here.
- **MariaDB** — `TEXT` decodes as `String` through `sqlx::query()` (it is `BLOB` only for `JSON`); keep `deleted_at IS NULL` on every read touched.
- **Rule #20** — "stored as typed": trim only. No collapsing of spaces, no line-ending conversion, no HTML stripping (`html_to_plain_text` is for providers only).
- **E2E data** — no ISBN, no V-code needed. L-codes: bucket 9 is unused by `specLcode` (only `L9998` / `L9999` are taken, by `highest-label-codes.spec.ts` and `shelving.spec.ts`): use `specLcode(9, 100 + vcodeSlot() * 10 + n)` (`specLcode` from `helpers/isbn.ts`, `vcodeSlot` from `helpers/vcode.ts`; `n` < 10 per test, unique across the spec's tests) and names prefixed `LDS-` with the same slot, so a CI retry on the same stack never meets its own earlier location (V2). Reach the location without the tree markup that 20.5 changes: after `createLocation(page, name, code)` (`helpers/locations.ts`), `page.goto("/scan?code=" + code)` lands on `/location/{id}` (story 9-9); read the id from the URL and go to `/locations/{id}/edit`. Selectors: `#edit-description`, `#edit-description-error`, `#edit-location-submit`, `#location-description`. Assert only on the spec's own location (Rule #23); no `waitForTimeout`. Use `loginAs(page)` in `beforeEach`. The textarea fill of 501 characters: `"x".repeat(501)`; the refused page keeps the 501 characters (`toHaveValue`).
- **Migration ordering** — 19-7 (in progress, PR #730) adds `20261126100000_…`; this story's timestamp must sort after it, whichever merges first, so production applies both in order in v1.29.0.
- `mem-guard` + the station lock for every heavy command; pass the rule to any sub-agent that compiles or tests. Never run `cargo fmt`.

### References

- Issue #716; `_bmad-output/planning-artifacts/epics.md` — Epic 20 (decisions D9, D11, couplings), Story 20.4, FR185.
- `src/routes/series_expected_tomes.rs` l. 61–100, 208–262 (form re-render pattern).
- `src/routes/locations.rs`, `src/models/location.rs`, `src/services/locations.rs`, `src/routes/api_v1.rs` l. 205–233.
- `tests/backup_round_trip.rs` l. 21–80, `tests/api_v1_series.rs`, `tests/form_refusals.rs` (#676 field-refusal tests).
- Story 19-1 (`19-1-location-tree-delete-modal.md`) — last story on these files.
- CLAUDE.md — Foundation Rules #3, #20, #21, #23, #24; "i18n", "DB queries", "CSP & hardening headers", "Backups".

## Validation (2026-11-26)

Checked against the code of `main` at `a608f56` (`src/routes/locations.rs`, `src/models/location.rs`, `src/services/locations.rs`, `src/routes/api_v1.rs`, `src/routes/home_scan.rs`, `src/mcp/`, the two location templates, `tests/backup_round_trip.rs`, `tests/integration.rs`, `tests/e2e/helpers/{locations,isbn,vcode}.ts`, `tests/e2e/playwright.config.ts`, the E2E specs that use L-codes).

Facts confirmed:
- `LocationModel` is built only in `src/models/location.rs` (five readers, three test literals) and in `loc()` of `src/routes/locations.rs`; no other file constructs it. `update_with_locking` has one caller (`LocationService::update_location`), which has one caller (the route). `LocationEditTemplate` / `LocationDetailTemplate` are built once each, plus the detail render test.
- No typed `sqlx::query!` reads `storage_locations` (`.sqlx/` has no entry for it): no offline-cache change.
- No MCP tool returns a `storage_locations` row; `get_title` gives a volume's location path only. D9 "MCP unchanged" needs no code.
- No `hx-boost` anywhere: the edit form is a native POST, so a 422 page renders as a page (the `series_expected_tomes` pattern works as is).
- No textarea in `templates/` carries `maxlength` (series description, title description): T-b matches the project.
- `location.breadcrumb_label` exists in the four locales and is unused, while `location_detail.html:9` hard-codes `aria-label="Location path"` — fixed in AC7 (existing copy, no decision needed).
- `/scan?code=<L-code>` redirects to `/location/{id}` (`routes/home_scan.rs:127`), which lets the E2E spec avoid the tree markup that 20.5 rewrites.
- `tests/backup_round_trip.rs` asserts every table is non-empty and compares `CHECKSUM TABLE`, so a seeded description is compared byte for byte after the load.
- Story 19-8 (restore) is still `backlog`: AC8 is met through the 19-6 round trip (load of `database.sql` into a fresh schema), which is what 19-8 will run.

Gaps fixed in this file (no product decision involved):
- **V1 — line breaks and the 500 limit.** A textarea submits line breaks as `\r\n`; a plain `chars().count()` would refuse a 495-character text with five line breaks. T-a counts `\r\n` as one character and stores the text as received. (Counting is per Unicode scalar value: a composed emoji counts as several characters — acceptable for a 500-character note.)
- **V2 — E2E data on retries.** CI runs `retries: 2` on the same stack; a fixed L-code would make a retry fail on "This L-code is already in use". The Dev Notes now derive the codes from `vcodeSlot()` in the unused `specLcode` bucket 9 (only `L9998` / `L9999` are taken there).
- **V3 — the API chapter.** Story 20.4's AC says the API field is "documented in the API chapter", and story 20.7's AC says `11-api-integrations.tex` documents it; the couplings give every `docs/manual/` edit to 19.11 / 20.7 so the PDFs are rebuilt once. AC6 hands the manual to 20.7; this story does not edit `docs/manual/`.
- **V4 — migration order.** 19-7 (open, PR #730) adds `20261126100000_settings_backup_retention.sql`; AC1 requires a later timestamp so the two apply in order whichever merges first.
- **V5 — locale placement.** The keys go in the `location:` block away from its `tree:` sub-block, which 20.5 edits (Rule #24 coupling).
- **V6 — `aria-describedby`.** The error id is named only when the error is rendered, so the textarea never points at a missing id.
- **V7 — sprint status.** `epic-20` stays `backlog` in `sprint-status.yaml`: Rule #16 lets this PR change only its own line and the header; the epic line is for a chore PR (or the first story merged, per the project's practice).

Pre-existing, left as is (outside the story):
- `LocationService::create_location` / `update_location` / `validate_parent_chain` build their refusals with `t!()` without locale (instance language), and they still come back as a bare fragment from the plain form — see Q1. Same class as #710's "`t!()` without locale near the touched code".
- A version conflict on the edit form answers a bare 409 fragment.

### Open questions (product decisions — not decided here)

- **Q1 — the edit form's other refusals.** The epic notes that the location form shows refusals as a bare 400 page and that 20.4 "re-renders its form with the error"; the approved 20.4 AC names only the description refusal (20.3's AC, by contrast, lists the series form's existing refusals). Once this story builds the re-render, the other refusals (blank name, invalid type, cycle, organizational location that still holds volumes) could be shown next to their fields with their current copy, in the request locale. Under Rule #3 each would then need its E2E case.
  *Recommendation:* keep 20.4 to the description refusal as approved, and open a `type:change-request` issue for the four others (and the 409), to be done by a later story with their E2E cases.
  **Settled (2026-11-26):** only the description refusal is re-rendered next to its field in this story; the other refusals (and the 409) are tracked in issue #732.
- **Q2 — where a save lands.** A save redirects to `/locations` (today's behaviour), where the description is not shown (D9), so a librarian does not see what was just saved without opening the location page.
  *Recommendation:* keep the redirect as it is (no behaviour change in this story); revisit with Q1's issue if usage asks for it.
  **Settled (2026-11-26):** the redirect to `/locations` after save is kept.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

- Askama 0.15 escapes `<` / `>` as `&#60;` / `&#62;` (not `&lt;`): the template tests assert that form.
- `users.preferred_language` is `ENUM('fr','en')`: the request-locale integration test signs in as `en` while the instance language is the seeded `fr`.
- The edit page never shows its own L-code (only the parent options): the refusal test checks the form's `action` instead.
- Build artefacts live in `CARGO_TARGET_DIR=/home/gcorbaz/devel/.wt-targets/20-4` (on disk): a `target/` inside the worktree filled the user quota of the `/tmp` tmpfs.

### Completion Notes List

- Migration `20261126120000_storage_locations_description.sql` (`TEXT NULL`, after 19-7's `20261126100000`).
- `LocationModel.description` read by the five readers; `update_with_locking` writes it. No typed query touched: no `.sqlx/` change.
- `LocationService::normalize_description` + `DESCRIPTION_MAX_CHARS` (500) + `DescriptionTooLong` (T-a: `\r\n` counts as one character; trim only, Rule #20).
- Route: one `render_edit_page` shared by the GET and the refusal (422, typed name / type / parent / organizational / description and the form's version kept, base context on `/locations/{id}/edit`, request locale). The service's other refusals unchanged (#732); redirect to `/locations` kept (Q2).
- Templates: textarea + help + error slot (`aria-invalid`, `aria-describedby` naming the error id only when shown); localized back link; `#location-description` (`whitespace-pre-line`, escaped) on the location page; breadcrumb `aria-label` localized.
- API: `ApiLocation.description` (`null` when unset). MCP unchanged.
- Locales: four keys in `en/fr/de/it` after `organizational_blocked_has_volumes` (`description_too_long` takes `%{max}`, rendered as in D11).
- Backup round trip seeds a multi-line description with quotes and non-ASCII characters.
- Gates run: `cargo test --lib`, `cargo clippy --all-targets -- -D warnings`, integration `location_description:: locale_parity:: backup_round_trip:: every_test_file_is_declared location_delete_modal:: api_v1 locale_html_safety::` (37 passed), `npx tsc --noEmit`, waitForTimeout grep, `vcode-literals.sh`.
- **Pending (T11):** the Playwright run of `specs/journeys/location-description.spec.ts` (and the full `CI=true --workers=2` suite) — not run here, the E2E stack is in use by another agent. Status stays `in-progress` until it is green.

### File List

- migrations/20261126120000_storage_locations_description.sql (new)
- src/models/location.rs
- src/services/locations.rs
- src/routes/locations.rs
- src/routes/api_v1.rs
- templates/pages/location_edit.html
- templates/pages/location_detail.html
- locales/en.yml, locales/fr.yml, locales/de.yml, locales/it.yml
- tests/location_description.rs (new)
- tests/integration.rs
- tests/backup_round_trip.rs
- tests/e2e/specs/journeys/location-description.spec.ts (new)
- _bmad-output/implementation-artifacts/sprint-status.yaml
- _bmad-output/implementation-artifacts/20-4-location-description.md

### Change Log

- 2026-11-26 — dev-story: description column, edit field with refusal next to the field, location page, API field, localized back link and breadcrumb label; unit, DB-integration and E2E tests (E2E run pending).
