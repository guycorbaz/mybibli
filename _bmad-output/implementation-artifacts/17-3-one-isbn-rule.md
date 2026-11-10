# Story 17.3: One ISBN rule for every form

Status: done

GitHub issue: #565 (type:code-review-finding, deferred from story 12-1). Covers: FR174. Epic 17; ships in v1.27.0.

## Story

As a librarian typing an ISBN by hand,
I want it stored the way a scan stores it,
so that a later scan finds the title instead of creating a duplicate.

## Where ISBN input is read today (inventory)

| Entry point | Code | Separators | `ISBN` prefix | ISBN-10 → 13 | Checksum |
|---|---|---|---|---|---|
| `/catalog/scan`, home scan `/scan` | `routes/catalog_code.rs::detect_code_type` (`utils::compact_isbn`, `isbn10_to_isbn13`) | yes | no | yes | downstream (`create_from_isbn`) |
| Web search, MCP `search_titles` | `services/search.rs` (`compact_isbn` → `detect_code`) | yes | no | yes | no (falls back to text) |
| Manual title creation | `services/title.rs::create_manual` (`non_empty_option`) | **no** | **no** | **no** | **no** — stored as typed; the field has `maxlength="13"` |
| Title edit (CR #272) | `routes/titles.rs` (`identifier_without_separators`) | yes | no | **no** ("must be 13 digits") | yes; `pattern="[0-9X]{0,13}" maxlength="13"` blocks a hyphenated value in the browser |
| Wish list "By ISBN" (preview + create) | `routes/wishlist.rs` (`validate_isbn13_checksum(trim)`) | **no** | **no** | **no** | yes; `pattern="[0-9]{13}"` |
| Expected tomes (form, proposals) | `services/expected_tomes.rs::normalize_isbn` | yes | no | yes | yes, 978/979 only |
| Expected-tome settling | `models/expected_tome.rs::live_titles_with_isbn_in` — SQL `UPPER(REPLACE(REPLACE(isbn,'-',''),' ',''))` + the ISBN-10 form | SQL copy | – | – | – |
| Volume move picker | `models/volume_move.rs::search_candidates` (`strip_isbn_separators`) | yes | no | no | – |
| Boot repair 13-3 | `services/title.rs::convert_isbn10_upc_titles` (`isbn10_to_isbn13`) | – | – | yes | yes |

## Production logs (2026-10-10 → 2026-11-10, read 2026-11-10)

- `Creating new title` logs the stored ISBN of every new title: 743 creations with an ISBN, **all 13 digits `978`/`979`**; 55 without ISBN. The 39 `Manual title created` lines all fall among those without ISBN.
- 782 `Processing scan` lines: none with a separator or an `ISBN` prefix (only `V,/+`, story 17-1).
- `Title metadata updated manually` (784) does not log the ISBN.
- **`titles.isbn` is `VARCHAR(13)`** (initial schema; strict SQL mode), and the manual field had `maxlength="13"`: a hyphenated ISBN-13 (17 characters) could never be stored. What the old manual form could leave is 13 characters at most — an ISBN-10 with hyphens or spaces (`2-205-00121-3`), a bare ISBN-10 (`2205001213`, the case of the existing test `manual_creation_with_an_isbn10_settles`), a truncated paste, or anything else typed. The repair therefore mostly concerns ISBN-10s; the issue's `978-2-205-00121-1` example is refused by the database, not stored.
- The first `Creating new title` is on 2026-10-26; titles up to ~id 280 predate the logs, so the stored values of older titles can only be read from a database export (`id, isbn, deleted`), which the project lead was asked for. Until it is read, the repair's effect on production is unknown (Rule #20).

## Decisions

- **D1 — one rule, positive shapes (Rule #20).** `utils::isbn::parse_isbn(input) -> Result<String, IsbnError>`: optional prefix `ISBN`, `ISBN-10`, `ISBN-13`, `ISBN10`, `ISBN13` (any case), optional `:`; the separators of `strip_isbn_separators` anywhere; what remains must be 13 digits starting `978`/`979` with a valid check digit (kept), or a valid ISBN-10 (converted to its ISBN-13). Anything else is refused (`Format` or `Checksum`), never rewritten. The `10`/`13` qualifier is read both as qualifier and as digits; the two readings differ by two characters, so at most one has a valid length — `ISBN-13 1312345678` keeps all its digits.
- **D2 — 978/979 only.** As the scan and the expected tomes already do: another EAN-13 is not an ISBN. The title edit accepted any 13 digits with a valid EAN check digit; a stored value that is left unchanged still round-trips (CR #272 behaviour kept).
- **D3 — forms.** `normalize_isbn_input(input) -> Result<Option<String>, IsbnError>`: blank (or separators only) → `None`. Manual creation, title edit, wish list (preview and create) and expected tomes use it. Two terse field hints (Rule #9): `error.isbn.invalid_format` (reworded: 10 or 13 digits, an example) and `error.isbn.invalid_checksum` (reworded: check digit, look for a typo); `wishlist.isbn_invalid` is dropped for them (DRY).
- **D4 — scan and search.** `isbn_lookup_form(input)`: the canonical ISBN-13 when `parse_isbn` accepts the input, otherwise the separator compaction as before — a mistyped check digit still reaches the scan's checksum refusal, and a search still falls back to text.
- **D5 — manual creation refuses an ISBN another live title holds** (`error.isbn.already_used`, the edit form's guard): the story's goal is that a typed ISBN never makes a duplicate.
- **D6 — the repair** (`services::isbn_repair`, `17-3-stored-isbn-one-rule`, `data_repairs::run_once`, at boot **before** the 13-3 conversion so its collision check sees canonical values). Each stored ISBN goes through `parse_isbn`; it is rewritten only when accepted and different (separators, prefix, bare ISBN-10 from the old manual form). **Titles in the Trash are covered too** (coordinator decision, 2026-11-10: a later restore must not bring back the old form) — a deliberate exception to the `deleted_at IS NULL` read rule, documented in `models/isbn_repair.rs`. Values grouped by their canonical form across live **and** trashed titles. Collisions (coordinator decision 2026-11-10, follow-up fix, #669): two or more **live** titles that would carry the same ISBN-13 are all left untouched; a trashed title never blocks the rewrite of a live one (the live title is the one the librarian kept; restoring the trashed twin is an explicit act); a trashed title is rewritten only when its new value collides with no other row, live or trashed. (Before the fix, any group of more than one title involving a rewrite was left untouched.) Write guarded by `id`, `version` and the old value; `version` and `updated_at` not moved (#521, 13-4). Pending metadata/cover items' `identity.isbn` follows the rewrite (as `carry_identity_title`), so the repair does not outdate them. The classification is pure (`plan_isbn_repair`); the ignored test `production_corpus` (`MYBIBLI_ISBN_CORPUS`) runs it over a CSV export, kept for later use, never committed data.
- **D10 — the first boot is auditable from the logs** (mitigation required by the coordinator for the missing export): one `info` line per rewritten title (`title_id`, `old`, `isbn`, `trashed`), one `warn` per title left untouched by a collision (`title_id`, `stored`, `isbn`, `trashed`, `colliding_title_ids`), one `info` summary with the counts.
- **D7 — the SQL copy goes.** After the repair every live title's ISBN is canonical or not an ISBN, so `live_titles_with_isbn_in` compares `isbn = ?` with the ISBN-13 only; the ISBN-10 form and the SQL `REPLACE` are removed.
- **D8 — fields.** The browser constraints that refused a hyphenated value go: manual ISBN `maxlength="13"` → 40, edit `pattern`/`maxlength="13"` → 40, wish list `pattern="[0-9]{13}"` removed.
- **D9 — volume move picker** puts an exact ISBN match first through `isbn_lookup_form`; its partial `LIKE` keeps the separator compaction.

## Acceptance Criteria

1. **AC1.** Manual creation, title edit, wish list "By ISBN", scan, search and expected tomes go through `utils::isbn` (separators, prefix, ISBN-10 → 13, checksum); an invalid ISBN in a form is refused with a terse hint, in all four locales.
2. **AC2.** The SQL separator rule of `models/expected_tome.rs` is removed (D7).
3. **AC3.** Stored ISBNs are repaired once per database on positive shapes, without bumping `titles.version`; colliding titles left untouched and logged.
4. **AC4.** The repair's classification is run over the production export and its changes recorded here before review. **Replaced by the project lead's decision (2026-11-10):** no export is available; the Rule #20 corpus step is replaced by the production-log analysis above (743 titles created with an ISBN since 2026-10-26, all canonical; the ~280 older titles unverified) and by the per-title audit lines of D10. The ignored `production_corpus` test stays for a later run.
5. **AC5.** Unit (parser shapes, plan), DB integration (manual creation, edit, wish list, repair: rewrite, collision, version, identity, run once) and E2E (a hyphenated ISBN typed by hand, then scanned bare, reaches the same title; an invalid ISBN gets the hint).

## Tasks

- [x] `src/utils/isbn.rs`: `parse_isbn`, `normalize_isbn_input`, `isbn_lookup_form`, `IsbnError::message` + unit tests (the older helpers stay in `utils.rs`, used by the new module).
- [x] Scan, search, manual creation (+ duplicate guard), edit, wish list, expected tomes, volume move: use it.
- [x] Templates: field constraints (D8).
- [x] Locales: reword the two hints, drop `wishlist.isbn_invalid` (en/fr/de/it).
- [x] `models/expected_tome.rs`: drop the SQL copy and the ISBN-10 form.
- [x] Repair service + model writes + `main.rs` call + DB integration + corpus test.
- [x] E2E spec.
- [x] Corpus run over the export (AC4) — replaced by the log analysis, lead's decision 2026-11-10; D10 audit logging added.
- [x] Trash covered by the repair (coordinator decision 2026-11-10); tests updated.

## Dev Notes

- Lock scope (Rule #22): the repair takes no `FOR UPDATE`; it runs at boot before the listener, in the `run_once` transaction, with guarded writes by primary key; the identity carry reads items from `idx_proposal_items_target` like `carry_identity_title`.
- `cargo fmt` is not run (repo-wide reformat); write in the neighbouring style.
- References: `_bmad-output/planning-artifacts/epics.md` § Story 17.3; issue #565; stories 11-1, 12-1, 13-3, 13-4, 17-1.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5

### Completion Notes List

- Implementation, unit, DB integration and E2E done; targeted gates green (clippy `--all-targets`, `isbn_one_rule` + the expected-tome / search / scan / conversion / proposal / locale integration suites, E2E: the new spec plus catalog, wish list, expected tomes, edit, search and scan specs at 2 workers).
- The existing test `a_tome_entered_for_a_title_already_in_place_or_stored_as_isbn10` relied on the SQL copy (D7): it now runs the boot repair first, which shows the copy unnecessary.
- **AC4:** no production export (2026-11-10); the project lead decided to continue on the log analysis. For a later run: `MYBIBLI_ISBN_CORPUS=<export.csv> SQLX_OFFLINE=true cargo test --lib production_corpus -- --ignored --nocapture`.
- Rebased on `main` after 17-5 / 17-6 (conflict in `services/search.rs`: the `#id` lookup of 17-5 kept before the ISBN arm).

- Final gates on the final code (2026-11-10): `cargo clippy --all-targets -D warnings` clean; full `cargo test` with the DB integration tests 2694 passed, 0 failed (locale parity included); `cargo sqlx prepare --check --workspace -- --all-targets` OK; `waitForTimeout` grep gate OK; full E2E `CI=true --workers=2`: 469 passed, 2 skipped, 0 failed.

### Review

- **Round 1** (Blind Hunter, Edge Case Hunter, Acceptance Auditor). Medium, fixed: a title the repair left alone after a collision could not be saved through the edit form with its ISBN untouched (the stored value normalised to the other title's ISBN-13 and hit the duplicate guard) — a stored value sent back unchanged now round-trips as stored, whether or not it parses (D2), test `a_title_left_by_a_repair_collision_stays_editable`; the volume-move picker only ordered by the exact ISBN, so an ISBN-10 or a prefixed ISBN listed nothing — it now also matches it (`OR t.isbn = ?`), test `the_volume_move_picker_finds_a_title_by_isbn10`; the E2E drew its ISBNs at random although manual creation now refuses a used ISBN — clock-based sequence with one slot per test. Low, fixed: wish-list field `inputmode="numeric"` removed (an `X` or a prefix), `isbn_lookup_form` comment corrected for prefixed inputs, the "changed meanwhile" repair line carries its values (D10), a misnamed test variable. Dismissed: a bare valid ISBN-10 scanned is converted (story 11-1 behaviour, unchanged); "other writers store non-canonical ISBNs" (only the forms of this story and the scan write `titles.isbn`). Low, deferred to #669: collision groups without a canonical member missed by exact lookups, trashed non-canonical twins, repair transaction granularity, non-atomic duplicate check, refusal copy without a link, prefixed bad check digit at scan, scan checksum locale, expected-tome hint, error-level logging, separator-only wish-list ISBN, corpus CSV parsing.

- **Round 2** (combined adversarial + edge pass on the round-1 fixes). No defect against the spec. Medium, kept as decided: with the collision check spanning the Trash (coordinator decision), a live title still holding its ISBN-10 whose duplicate — made by a later scan — was put in the Trash stays unrepaired and is only logged (`warn`); once the twin is purged nothing repairs it, and scans / tome settling miss it. Rewriting the live title would follow the bug's typical aftermath better; the choice is the coordinator's, recorded on #669 with the unit test `a_trashed_twin_blocks_the_rewrite_of_a_live_title` locking the current behaviour. Low, accepted: a stored non-canonical ISBN re-typed with other separators round-trips as stored (D2); the picker's exact match adds nothing for text queries.
- **Follow-up fix (2026-11-10, branch `fix/17-3-repair-trash-collision`)** — the round-2 Medium settled by the coordinator: take the alternative (D6 above). `plan_isbn_repair` takes each row's Trash flag; unit tests `a_trashed_twin_never_blocks_the_rewrite_of_a_live_title`, `a_trashed_title_is_rewritten_only_without_any_twin`; DB test `titles_that_would_collide_are_left_untouched` updated. The repair has not run in production yet (unreleased), so the `run_once` marker name is unchanged.

### File List

- `src/utils/isbn.rs` (new) — the one rule: `parse_isbn`, `normalize_isbn_input`, `isbn_lookup_form`, `IsbnError` + unit tests; `src/utils.rs` re-exports.
- `src/routes/catalog_code.rs` — scan classification through `isbn_lookup_form`; prefix tests.
- `src/services/search.rs` — web / MCP search through `isbn_lookup_form`; prefix tests.
- `src/services/title.rs`, `src/routes/catalog_title_form.rs` — manual creation: one rule, duplicate guard, request locale.
- `src/routes/titles.rs` — title edit: one rule, unchanged stored value round-trips.
- `src/routes/wishlist.rs` — preview and create: one rule.
- `src/services/expected_tomes.rs` — `normalize_isbn` delegates to the rule; `isbn13_to_isbn10` / `stored_forms` removed.
- `src/models/expected_tome.rs` — `live_titles_with_isbn` (plain `isbn = ?`), SQL separator copy removed.
- `src/models/volume_move.rs` — exact-first match through `isbn_lookup_form`.
- `src/models/isbn_repair.rs` (new), `src/services/isbn_repair.rs` (new), `src/models/proposal.rs` (`carry_identity_isbn`), `src/main.rs` — the run-once boot repair.
- `templates/components/title_form.html`, `templates/fragments/title_edit_form.html`, `templates/pages/wishlist_new.html` — field constraints (D8).
- `locales/{en,fr,de,it}.yml` — `error.isbn.invalid_format` / `invalid_checksum` reworded; `wishlist.isbn_invalid` removed.
- `tests/isbn_one_rule.rs` (new), `tests/expected_tomes.rs` — DB integration.
- `tests/e2e/specs/journeys/isbn-one-rule.spec.ts` (new, spec id `IR`).
