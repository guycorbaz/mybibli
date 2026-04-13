# Story 5.8: Dewey Code Management

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a librarian,
I want to assign a Dewey Decimal Classification code to a title and sort my location contents by it,
so that I can shelve my physical books by classification and verify the catalog matches the physical shelf order.

## Scope at a glance (read this first)

**Most of the plumbing already exists.** The `titles.dewey_code VARCHAR(15) NULL` column, model CRUD (`TitleModel::create` / `update_metadata` already bind dewey_code at `src/models/title.rs:94,118,142,166,287`), the edit form input (`templates/fragments/title_edit_form.html:54-58`), and the i18n label key `metadata.field.dewey_code` are all in place from stories 1-3 and 3-5. This story adds the remaining four pieces:

1. **BnF pre-fill** — parse UNIMARC field `676$a` in `src/metadata/bnf.rs` into a new `dewey_code` field on `MetadataResult`, and include it in the async metadata-fetch `UPDATE` in `src/tasks/metadata_fetch.rs` so scanned titles inherit the Dewey code when BnF publishes one.
2. **Location-view sort with NULL last** — add `dewey_code` to the whitelist + SQL in `VolumeModel::find_by_location`, and to the column list on `templates/pages/location_detail.html`. MariaDB idiom: `ORDER BY t.dewey_code IS NULL, t.dewey_code <dir>`.
3. **Re-download conflict flow parity** — add `dewey_code` to `TitleService::build_field_conflicts` / `build_auto_updates` / `all_fields` / `field_label` / `get_title_field_value` / `get_metadata_field_value` so re-download respects manual edits of Dewey the same way it does for publisher and description. Add `new_dewey_code` + `accept_dewey_code` to `MetadataConfirmForm` and `MetadataConfirmTemplate`, and wire through `confirm_metadata`.
4. **i18n cleanup of hardcoded `"Dewey:"`** — replace the two hardcoded occurrences at `templates/pages/title_detail.html:42-44` and `src/routes/titles.rs:286-287` with the existing `metadata.field.dewey_code` i18n label.

**Explicitly NOT in scope:** making Dewey searchable or filterable (FR118 hard rule — "physical sort order only"), adding Dewey sort to the main browse/search page, Dewey validation/format checks (free text), adding Dewey to `Google Books` or other secondary providers (BnF is the only authoritative source for French-market Dewey codes in v1), UI-side natural/numeric sort (string sort is sufficient — see Dev Notes).

## Acceptance Criteria

1. **Edit & persist (already wired — verify it still works):** Given the title detail page `/title/{id}`, when a librarian clicks "Edit metadata", enters a free-text value in the Dewey code input (e.g. `843.914`), and clicks Save, then the value is persisted on the title row and visible on the read-only detail page after the HTMX swap. A blank value stores `NULL`. The form submission goes through the existing `POST /title/{id}` handler and is subject to the existing optimistic-lock check.
2. **BnF pre-fill on scan:** Given a librarian scans an ISBN and BnF returns a UNIMARC record containing `<datafield tag="676"><subfield code="a">843.914</subfield></datafield>`, when the async metadata-fetch task resolves, then `titles.dewey_code` for that title is updated to `843.914` (via `COALESCE(?, dewey_code)` — the existing value is not clobbered if BnF omits the tag). If BnF returns no `676$a`, the column stays `NULL`.
3. **Location-view sort — NULL last, both directions:** Given a storage-location detail page `/location/{id}` with volumes whose linked titles have a mix of Dewey codes and NULLs, when the user clicks the "Dewey" column header, then the volumes are re-fetched sorted `ORDER BY t.dewey_code IS NULL, t.dewey_code ASC` (NULLs last), paginated 25/page per NFR39. Clicking the header a second time toggles to DESC but NULLs still appear last (`ORDER BY t.dewey_code IS NULL, t.dewey_code DESC`). The active sort arrow (▲ / ▼) renders on the Dewey column header when it's the current sort.
4. **Not searchable, not filterable (FR118 hard rule):** The home search page (`GET /`) MUST NOT accept `sort=dewey_code` — the whitelist `VALID_SORT_COLUMNS` in `src/models/title.rs:568` stays unchanged. There is no Dewey filter chip, no Dewey facet, no Dewey option in the browse sort dropdown at `templates/pages/home.html:82-86`. A verification unit test asserts `validated_sort(&Some("dewey_code".to_string()))` falls back to `"title"`.
5. **Location-view Dewey column render:** Given a volume row, when its linked title has a non-null `dewey_code`, then the Dewey cell shows the value in a monospace font; when `dewey_code IS NULL`, the cell shows `—` (em dash, consistent with the existing `primary_contributor` empty pattern at `templates/pages/location_detail.html:69-70`). The column label comes from a new i18n key `location.col_dewey`.
6. **Re-download respects manual edits:** Given a librarian manually edited the Dewey code to a custom value (e.g. `800 [librarian-custom]`), when they re-download metadata and BnF returns a different Dewey code (`843.914`), then the confirmation screen lists `dewey_code` under conflicts with current + new values. If the librarian unchecks "accept new value", the manual value is preserved; if they check it, the new BnF value is applied and the field is removed from `manually_edited_fields`. Mirrors the existing `publisher` / `description` flow at `src/routes/titles.rs:694-775`.
7. **Auto-update on re-download when not manually edited:** Given the Dewey code was auto-populated by BnF on creation (never manually edited), when the librarian re-downloads metadata and BnF returns a new value, then the new value is applied without user confirmation and included in `auto_updates` diff output.
8. **i18n parity — no hardcoded labels:** The string `Dewey:` must not appear as a hardcoded literal anywhere in `templates/` or `src/routes/`. The two existing occurrences (`templates/pages/title_detail.html:42-44` and `src/routes/titles.rs:286-287`) are replaced with the i18n label from `metadata.field.dewey_code`. A grep gate asserts zero hardcoded `"Dewey:"` in `templates/` and `src/routes/`.
9. **i18n keys — both locales:** `location.col_dewey` exists in `locales/en.yml` (`"Dewey"`) and `locales/fr.yml` (`"Dewey"`). `metadata.field.dewey_code` already exists — reuse, don't duplicate. After adding new keys, `touch src/lib.rs && cargo build` is run so the `rust_i18n` proc macro picks them up.
10. **Unit tests (4 new):**
    - BnF provider: `parse_sru_response` extracts `676$a` into `MetadataResult.dewey_code` when present; returns `None` when absent.
    - Sort whitelist: `validated_location_sort(&Some("dewey_code".to_string()))` returns `"dewey_code"`; `validated_sort(&Some("dewey_code".to_string()))` (home search) still returns `"title"` (AC #4 guard).
    - `TitleService::build_auto_updates` / `build_field_conflicts` include `dewey_code` — parametrized with a title whose `dewey_code` differs from `metadata.dewey_code`, for both the "manually edited" and "not edited" branches.
    - `metadata_fetch::update_title_from_metadata` propagates `dewey_code` into the `UPDATE` statement — DB integration test following the `tests/find_similar.rs` pattern (fresh `#[sqlx::test]` pool, insert title, call handler, assert post-update value).
11. **DB integration test — NULL-last ordering (`tests/find_by_location_dewey.rs` or added to the existing integration-test crate):** Seed one location + 4 volumes whose linked titles have dewey codes `["200", "843.914", NULL, "843.2"]`. Call `VolumeModel::find_by_location(pool, loc_id, &Some("dewey_code".to_string()), &Some("asc".to_string()), 1)` and assert the returned order is `["200", "843.2", "843.914", NULL]`. Flip to `desc` and assert `["843.914", "843.2", "200", NULL]` (NULLs still last). Use `#[sqlx::test(migrations = "./migrations")]` — the dedicated MariaDB test DB + `cargo test --test <crate>` run are already wired (see CLAUDE.md → "DB-backed integration tests" section).
12. **E2E test — `tests/e2e/specs/journeys/dewey-code.spec.ts` (2 tests):**
    - **Test 1 — edit → persist → sort location view:** Using `loginAs(page)` in `beforeEach`. Scan a unique ISBN (`specIsbn("DC", 1)`), open title detail, click Edit, fill the Dewey field with `"843.914"`, save. Navigate back to detail page via HTMX swap and assert the Dewey value is visible. Then assign a location (reuse `shelving.spec.ts` pattern) and navigate to `/location/{loc_id}?sort=dewey_code&dir=asc`. Assert the row for the title appears in the list with the Dewey value rendered.
    - **Test 2 — NULL-last ordering in live stack:** Create 2 titles in the same location — one with Dewey `"200"` (`specIsbn("DC", 2)`), one without Dewey (`specIsbn("DC", 3)`). Navigate to the location page with `sort=dewey_code&dir=asc`. Assert the title with Dewey appears before the one without. Flip to `dir=desc` and assert the same title (with Dewey) still appears before the one without — NULL always sinks. Use the i18n-aware regex matcher `/Dewey|Dewey/i` (label is identical in EN/FR) for the column header visibility check.
    - Spec prefix `"DC"` (Dewey Code) — confirmed unused in grep against `tests/e2e/specs/journeys/`. Generate unique ISBNs with `specIsbn("DC", seq)`. No `DEV_SESSION_COOKIE` injection (Foundation Rule #7 hard rule).
13. **Verification gate (Foundation Rule #5):** `cargo clippy -- -D warnings`, `cargo test`, `cargo sqlx prepare --check --workspace -- --all-targets`, `cargo test --test find_similar` (unchanged), `cargo test --test find_by_location_dewey` (new, if a new crate is used), and `cd tests/e2e && npm test` — full suite green (131+ tests passing, no regressions in parallel mode). Update story status to `review`.

## Tasks / Subtasks

- [x] **Task 0 — Verify AC #1 is already functional** (AC: #1)
  - [x] 0.1 The edit-form path is fully wired: `src/routes/titles.rs:437` reads `dewey_code` from form, `detect_edited_fields` at `src/models/title.rs:543` already pushes `"dewey_code"` into the changed set, and `manually_edited_fields` is merged cumulatively at `src/routes/titles.rs:457-471`. No code changes required for AC #1 — only the E2E test in Task 7.2 verifies it end-to-end. **If E2E Test 1 fails, that indicates a regression elsewhere, not a missing feature.**

- [x] **Task 1 — BnF provider extracts Dewey** (AC: #2, #10 BnF test)
  - [x] 1.1 In `src/metadata/provider.rs`, add `pub dewey_code: Option<String>` to `MetadataResult`. Default to `None` in `#[derive(Default)]`. Update existing tests in `src/metadata/provider.rs:81-108` only if they construct `MetadataResult` literally — with `..MetadataResult::default()` the addition is backward-compatible. Do NOT touch `bdgest.rs`, `google_books.rs`, `open_library.rs`, etc. — they return `..MetadataResult::default()` spread and will pick up `None` automatically.
  - [x] 1.2 In `src/metadata/bnf.rs::parse_sru_response` (line 109), add `dewey_code: Self::extract_subfield(xml, "676", "a")` to the returned struct literal. UNIMARC field `676` is "Dewey Decimal Classification", subfield `$a` is the code itself. The existing `extract_subfield` helper handles namespace prefix (`<mxc:datafield>` variant) and whitespace trimming — no new parsing needed.
  - [x] 1.3 Add unit test `test_parse_sru_response_with_dewey_676a` that feeds an XML fixture containing `<mxc:datafield tag="676" ind1=" " ind2=" "><mxc:subfield code="a">843.914</mxc:subfield></mxc:datafield>` and asserts `result.dewey_code.as_deref() == Some("843.914")`.
  - [x] 1.4 Add unit test `test_parse_sru_response_without_dewey_returns_none` that reuses `SAMPLE_BNF_RESPONSE` (which has no 676 field) and asserts `result.dewey_code.is_none()`.
  - [x] 1.5 **Optional — follow-up for retros, not this story:** adding `676$a` parsing to Google Books / Open Library. Both providers expose their own classification fields (Google Books has `industryIdentifiers`, LC rarely Dewey; Open Library may have `dewey_decimal_class` in the `works` endpoint). They are not in scope for v1 — BnF is the only authoritative French Dewey source we care about.

- [x] **Task 2 — Async metadata-fetch task propagates Dewey** (AC: #2, #10 integration test)
  - [x] 2.1 In `src/tasks/metadata_fetch.rs::update_title_from_metadata` (line 71), add `dewey_code = COALESCE(?, dewey_code),` to the UPDATE statement (between `publication_date` and `track_count` for readability — order doesn't matter functionally). Add a matching `.bind(&metadata.dewey_code)` in the bind sequence. This mirrors the existing pattern for `publisher`, `language`, etc. The `COALESCE` ensures that if BnF omits the tag but some other provider earlier in the chain populated something, the earlier value is preserved.
  - [x] 2.2 Add a DB-backed integration test to `tests/` (new file `tests/metadata_fetch_dewey.rs` or inline in `tests/find_similar.rs` — prefer a new file for clarity) using `#[sqlx::test(migrations = "./migrations")]`. Three test cases:
    - **Pre-fill null → value:** Seed title with `dewey_code = NULL`. Call `update_title_from_metadata` with a `MetadataResult { title: Some("x".into()), dewey_code: Some("843.914".into()), ..Default::default() }`. Re-fetch the title and assert `dewey_code.as_deref() == Some("843.914")`.
    - **COALESCE preserves existing:** Seed title with `dewey_code = Some("800")`. Call with `MetadataResult { title: Some("x".into()), dewey_code: None, ..Default::default() }`. Assert title still has `dewey_code.as_deref() == Some("800")` (verifies the `COALESCE(?, dewey_code)` semantics).
    - **Realistic-length Dewey roundtrip:** Seed a title and update with `dewey_code: Some("843.914094".into())` (10 chars — typical BnF extended notation). Re-fetch and assert it roundtripped exactly, no truncation. This exercises the `VARCHAR(15)` column at a realistic width (see Dev Notes → "VARCHAR(15) length ceiling").
  - [x] 2.3 **No need to touch** the synchronous create path at `src/routes/titles.rs:437-477` (`create_title_from_scan`). Scans create the title immediately with empty metadata and the async task fills fields in via the UPDATE path above. Manual title creation via the catalog-side create form already accepts a `dewey_code` input (verified at `src/routes/titles.rs:397,437,453,477`).

- [x] **Task 3 — Re-download conflict flow treats Dewey as first-class field** (AC: #6, #7, #10 service test)
  - [x] 3.1 In `src/services/title.rs`:
    - In `build_auto_updates::all_fields` at line 356-357, add `"dewey_code"` to the array.
    - In `field_label` at line 371-386, add `"dewey_code" => rust_i18n::t!("metadata.field.dewey_code").to_string(),`.
    - In `get_title_field_value` at line 388-403, add `"dewey_code" => title.dewey_code.clone().unwrap_or_default(),`.
    - In `get_metadata_field_value` at line 405-420, add `"dewey_code" => metadata.dewey_code.clone().unwrap_or_default(),`.
    - `build_field_conflicts` iterates over `manually_edited` (external input), so it picks up `dewey_code` automatically once the four helpers above know the field.
  - [x] 3.2 In `src/routes/titles.rs`:
    - Add `pub new_dewey_code: String` + `pub accept_dewey_code: Option<String>` to `MetadataConfirmForm` (lines 613-664). Mirror the existing publisher wiring exactly.
    - Add `pub new_dewey_code: String` to the `MetadataConfirmTemplate` struct at **line 897** (the struct sits below the handlers in the same file). Populate it in the render call at lines 578-603 with `new_dewey_code: metadata.dewey_code.clone().unwrap_or_default(),`.
    - Add a `final_dewey_code` computation in `confirm_metadata` at lines 694-775 following the same `if use_new(...) { ... } else { kept_count += 1; title.dewey_code.clone() }` pattern as `final_publisher`.
    - Update the `TitleModel::update_metadata` call at lines 786-792 to pass `final_dewey_code.as_deref()` in the existing `dewey_code` position (currently passes `title.dewey_code.as_deref()` — change to the new computed value).
    - Also update `apply_metadata_to_title` (definition at **line 833**, called from the no-conflict branch at line 554) — change the `dewey_code` argument from `title.dewey_code.as_deref()` at line 865 to `metadata.dewey_code.as_deref().or(title.dewey_code.as_deref())`. This keeps the existing value when BnF omits the tag (AC #7 semantics) while applying fresh BnF values on change.
  - [x] 3.3 In `templates/fragments/metadata_confirm.html`, add ONE line to the hidden-fields block (after line 18 `<input type="hidden" name="new_cover_url" ...>`):
    ```html
    <input type="hidden" name="new_dewey_code" value="{{ new_dewey_code }}">
    ```
    **Do NOT add a new conflict-row or accept-checkbox markup** — the conflict table at lines 31-42 already iterates `{% for c in conflicts %}` and renders `<input name="accept_{{ c.field_name }}">` dynamically. When `FieldConflict.field_name == "dewey_code"` (generated by the Task 3.1 helpers), the checkbox is auto-named `accept_dewey_code`. The only template delta is the single hidden input.
  - [x] 3.4 Add a unit test in `src/services/title.rs` tests module: construct a `TitleModel` with `dewey_code: Some("800".into())` and `manually_edited_fields: Some(r#"["dewey_code"]"#.into())`, pass a `MetadataResult { dewey_code: Some("843.914".into()), ..Default::default() }`, and assert `build_field_conflicts` returns exactly one conflict with `field_name == "dewey_code"`, `current_value == "800"`, `new_value == "843.914"`. Second test: with an empty `manually_edited_fields` vec, assert `build_auto_updates` contains a string like `"Dewey code: 800 -> 843.914"`.

- [x] **Task 4 — Location-view sort whitelist + SQL + UI** (AC: #3, #5, #10 sort test, #11 DB integration test)
  - [x] 4.1 In `src/models/volume.rs::LOCATION_SORT_COLUMNS` at line 228, add `"dewey_code"` to the whitelist. In `map_location_sort_column` at line 245-252, add `"dewey_code" => "t.dewey_code",`. Note: the struct `VolumeWithTitle` at lines 213-225 needs a new `pub dewey_code: Option<String>` field.
  - [x] 4.2 In `VolumeModel::find_by_location`, modify the `data_sql` format string at lines 279-300:
    - Add `t.dewey_code` to the SELECT list (right after `t.media_type` or grouped with title fields).
    - Replace `ORDER BY {} {}` with a conditional that, when `sort_col == "dewey_code"`, emits `ORDER BY t.dewey_code IS NULL, t.dewey_code {sort_dir}` (NULL last, both directions). For all other sort columns, keep the existing `ORDER BY {} {}`. The cleanest implementation is a helper: `fn order_by_clause(sql_col: &str, sort_dir: &str) -> String { if sql_col == "t.dewey_code" { format!("{} IS NULL, {} {}", sql_col, sql_col, sort_dir) } else { format!("{} {}", sql_col, sort_dir) } }`.
    - In the row-mapping closure at lines 311-321, add `dewey_code: r.try_get("dewey_code").unwrap_or(None),`. Use explicit `try_get::<Option<String>, _>("dewey_code")` if the inferred type proves ambiguous (see story 5-7 review patch for precedent).
  - [x] 4.3 Add unit test `test_validated_location_sort_accepts_dewey_code` in `src/models/volume.rs` tests module: `assert_eq!(validated_location_sort(&Some("dewey_code".to_string())), "dewey_code")` and `assert_eq!(map_location_sort_column("dewey_code"), "t.dewey_code")`.
  - [x] 4.4 Add unit test `test_validated_sort_rejects_dewey_code_on_search` in `src/models/title.rs` tests module (companion module to the one at line 785): `assert_eq!(validated_sort(&Some("dewey_code".to_string())), "title")`. This enforces FR118's "not searchable/filterable" hard rule by test (AC #4).
  - [x] 4.5 Add DB integration test file `tests/find_by_location_dewey.rs` (or extend `tests/find_similar.rs` — but a dedicated file is cleaner because the seeding helpers differ). Use `#[sqlx::test(migrations = "./migrations")]`. Seed:
    - One storage location via `INSERT INTO storage_locations (name, node_type, label) VALUES ('test-loc', 'Shelf', 'L0001')`. Canonical `node_type` values are seeded in `migrations/20260401000001_seed_location_node_types.sql` as `'Room'`, `'Furniture'`, `'Shelf'`, `'Box'` (capitalized — the column is `VARCHAR(50)` with no FK, but use the app convention).
    - Four titles with `dewey_code` values `["200", "843.914", NULL, "843.2"]`.
    - Four volumes (one per title) with `location_id = <loc_id>` and a unique `label` each (`V0001`..`V0004`). Check `volumes.label` constraints in `migrations/20260329000000_initial_schema.sql:125-160`.
    - Call `VolumeModel::find_by_location(pool, loc_id, &Some("dewey_code".into()), &Some("asc".into()), 1).await.unwrap()`.
    - Assert `items.iter().map(|v| v.dewey_code.as_deref()).collect::<Vec<_>>() == vec![Some("200"), Some("843.2"), Some("843.914"), None]`.
    - Second test with `desc`: assert `vec![Some("843.914"), Some("843.2"), Some("200"), None]`.
  - [x] 4.6 In `src/routes/locations.rs::LocationDetailTemplate` (line 32-59), add `pub col_dewey: String,`. Populate at line 96 with `col_dewey: rust_i18n::t!("location.col_dewey").to_string(),`.
  - [x] 4.7 In `templates/pages/location_detail.html`:
    - Add a new `<th>` between the `col_genre` header (line 49-53) and `col_condition` (line 54) with the same sortable-link markup as the other columns (pattern at lines 44-48 is the canonical copy-paste target). Anchor `href` = `/location/{{ location.id }}?sort=dewey_code&dir={toggle-asc/desc}&page=1`. Sort arrow toggles on `volumes.sort == Some("dewey_code".to_string())`.
    - Add a new `<td>` in the row loop (between line 72 `genre_name` and line 73 `condition_name`) rendering `{% match vol.dewey_code %}{% when Some with (d) %}<code class="font-mono text-xs">{{ d }}</code>{% when None %}—{% endmatch %}` for AC #5. Use monospace (`font-mono text-xs`) because Dewey codes are numeric and right-aligned reading matters for shelf-order verification.
    - Also update the pagination anchor hrefs at lines 92-93, 99-100 — they already pass `sort` through via `{% match volumes.sort %}`, so no change needed there (the pattern is already dynamic). ✓ verify by re-reading the file after edits.

- [x] **Task 5 — i18n keys + proc-macro refresh** (AC: #8, #9)
  - [x] 5.1 In `locales/en.yml` under the `location:` block (around line 240-247), add `col_dewey: Dewey` after `col_genre`.
  - [x] 5.2 In `locales/fr.yml` under the `location:` block (around line 240-247), add `col_dewey: Dewey` after `col_genre`. The label is "Dewey" in both languages (it's a proper noun — Melvil Dewey). Do NOT translate to "Décimal" or similar.
  - [x] 5.3 Run `touch src/lib.rs && cargo build` to force the `rust_i18n` proc macro to re-read the YAML files — confirmed required by story 5-6 and 5-7 retros. `cargo check` alone is insufficient.
  - [x] 5.4 **Do NOT duplicate `metadata.field.dewey_code`** — it already exists in both `locales/en.yml:285` and `locales/fr.yml:285` from story 3-5. Reuse it for the title-detail display label in Task 6.

- [x] **Task 6 — Replace hardcoded `Dewey:` labels** (AC: #8)
  - [x] 6.1 In `templates/pages/title_detail.html` line 42-44, replace `Dewey: {{ dewey }}` with `{{ label_dewey_code }}: {{ dewey }}`. Add `pub label_dewey_code: String,` to the `TitleDetailTemplate` struct at **`src/routes/titles.rs:28`** (struct currently spans lines 28-63, directly before the `title_detail` handler at line 65). Populate it in the template init block at line 89+ with `label_dewey_code: rust_i18n::t!("metadata.field.dewey_code").to_string(),` (add alongside the other `label_*` populations — pattern matches `label_similar_titles` which was added in story 5-7).
  - [x] 6.2 In `src/routes/titles.rs:286-287`, replace the hardcoded `r#"<p class="mt-1 text-xs text-stone-400">Dewey: {}</p>"#` with `format!(r#"<p class="mt-1 text-xs text-stone-400">{}: {}</p>"#, rust_i18n::t!("metadata.field.dewey_code"), html_escape(d))`. Note: this function is `metadata_display_html` and is called from the HTMX fragment path after inline edit — it's a server-side format, not a template render, so the `rust_i18n::t!()` macro call is cheap and correct. Double-check `html_escape(d)` is still called on the value.
  - [x] 6.3 Grep gate: after edits, run `grep -rn "Dewey:" templates/ src/routes/ src/services/` — must return zero matches. The only remaining matches should be in docs (`_bmad-output/**`), tests, this story file, and source comments. A test is overkill; a manual grep + ensuring Task 7 clippy+test gate is green is sufficient.

- [x] **Task 7 — E2E tests** (AC: #12)
  - [x] 7.1 Create `tests/e2e/specs/journeys/dewey-code.spec.ts`. Use `loginAs(page)` in `beforeEach` (Foundation Rule #7 + CLAUDE.md hard rule — no cookie injection). Spec prefix `"DC"` (Dewey Code) — confirmed unused in `tests/e2e/specs/journeys/`.
  - [x] 7.2 **Test 1 — "librarian can edit and persist Dewey code":**
    1. `const ISBN = specIsbn("DC", 1);`
    2. `await page.goto("/catalog");` + `fill` + `press Enter` to scan. Wait for feedback with `page.waitForSelector(".feedback-entry, .feedback-skeleton", { timeout: 10000 })`.
    3. Navigate via search: `await page.goto("/?q=" + ISBN);` then click the first result link.
    4. Click the "Edit metadata" button (use `getByRole("button", { name: /Edit metadata|Modifier les métadonnées/i })` — verify the exact FR string against `locales/fr.yml:metadata.edit_metadata`).
    5. Wait for the edit form to swap in: `await expect(page.locator("#edit-dewey")).toBeVisible({ timeout: 5000 });`
    6. `await page.locator("#edit-dewey").fill("843.914");`
    7. Click "Save changes". Wait for the HTMX swap back to the metadata display: `await expect(page.locator("#title-metadata").getByText(/843\.914/)).toBeVisible({ timeout: 5000 });`
    8. Assert the i18n label is rendered (not the hardcoded literal): `await expect(page.locator("#title-metadata")).toContainText(/Dewey code|Code Dewey/i);`.
  - [x] 7.3 **Test 2 — "location view sorts by Dewey with NULL last":**
    1. Create 2 titles via scan: `specIsbn("DC", 2)` and `specIsbn("DC", 3)`.
    2. Edit the first title's Dewey code to `"200"` via the same edit flow as Test 1. Leave the second title's Dewey code empty.
    3. Reuse the shelving helper from `tests/e2e/specs/journeys/shelving.spec.ts` (find the canonical "assign volume to location" pattern — it involves scanning a volume + L-code on the catalog page). Assign both title's primary volume to the same location (create a new location first via `POST /locations/create` OR use a seeded one — check shelving.spec.ts for the pattern).
    4. Navigate to `/location/{loc_id}?sort=dewey_code&dir=asc`. Assert both title links are visible. Assert the row containing `"200"` appears BEFORE the row containing `specIsbn("DC", 3)` (find both via `page.locator('tr').filter({ hasText: ... })` and compare their bounding boxes: `y_of_row_with_200 < y_of_row_without_dewey`).
    5. Navigate to `/location/{loc_id}?sort=dewey_code&dir=desc`. Assert the same invariant: the row with `"200"` still appears before the row without Dewey (NULLs sink in both directions).
    6. Assert the column header is present and i18n-aware: `await expect(page.locator("th").filter({ hasText: /^Dewey$/ })).toBeVisible();` — label is the same in EN and FR so no regex alternation needed.
  - [x] 7.4 **Stable selectors & parallel-safety requirements** (hard rules from CLAUDE.md E2E Test Patterns):
    - Unique `specIsbn("DC", seq)` for every ISBN in the spec. Unique location name / L-code per test run (suffix with timestamp or use a fixture helper if shelving.spec.ts has one).
    - No `waitForTimeout(N)` anywhere. Every wait is a `expect(locator).toBeVisible()` or `.toContainText(regex)` on a specific DOM state.
    - `loginAs(page)` in `beforeEach` — do not inject `DEV_SESSION_COOKIE`.
    - Use `page.getByRole`, `page.locator("#id")`, or i18n-aware regex text matchers for all selectors. Tailwind class names are fragile.
  - [x] 7.5 Run the spec in isolation first: `cd tests/e2e && npx playwright test specs/journeys/dewey-code.spec.ts`. Then run the full suite to confirm no regressions: `cd tests/e2e && npm test`. The full suite must still hit the 131+/131+ green baseline from story 5-7.

- [x] **Task 8 — Verification gate** (Foundation Rule #5)
  - [x] 8.1 `cargo clippy -- -D warnings` passes (zero warnings).
  - [x] 8.2 `cargo test` — all unit tests green (expect 317+ tests passing; you'll add ~6 new unit tests).
  - [x] 8.3 `cargo sqlx prepare --check --workspace -- --all-targets` passes. The new `t.dewey_code IS NULL, t.dewey_code {dir}` ORDER BY is built via dynamic `sqlx::query()` (same pattern as `active_search`), so no `.sqlx/` cache regeneration should be needed. If `cargo check` or prepare complains, run `cargo sqlx prepare` and commit `.sqlx/`.
  - [x] 8.4 Start the dedicated MariaDB test DB: `docker compose -f tests/docker-compose.rust-test.yml up -d`. Run:
    - `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test --test find_similar` — still green (story 5-7 baseline, 12 tests in ~3s).
    - `SQLX_OFFLINE=true DATABASE_URL='...' cargo test --test metadata_fetch_dewey` — new, 2 tests (AC #2, #10).
    - `SQLX_OFFLINE=true DATABASE_URL='...' cargo test --test find_by_location_dewey` — new, 2 tests (AC #11).
  - [x] 8.5 `cd tests/e2e && npm test` — **131+/131+ passing** in parallel mode, no regressions.
  - [x] 8.6 Update story status to `review`, update `_bmad-output/implementation-artifacts/sprint-status.yaml` entry `5-8-dewey-code-management: ready-for-dev → review`, hand off to `code-review` workflow (fresh context, different LLM recommended).

## Dev Notes

### Why most of this story is "wiring", not new features

This story has an unusually high "already implemented" ratio because stories 1-3 (Title CRUD) and 3-5 (Metadata editing) built the skeleton assuming Dewey would land later:

| Component | State at start of 5-8 | What 5-8 changes |
|---|---|---|
| `titles.dewey_code` DB column | ✅ Exists (`VARCHAR(15) NULL`, not 32 as the epic text says) | Nothing — column is sufficient |
| `TitleModel.dewey_code` Rust field | ✅ Exists, read/write in all 4 SELECT queries + `update_metadata` | Nothing |
| Title edit form Dewey input | ✅ Exists at `templates/fragments/title_edit_form.html:54-58` | Nothing (verify AC #1 still works) |
| Title detail page Dewey display | ✅ Renders with hardcoded `"Dewey:"` literal at `templates/pages/title_detail.html:42-44` and `src/routes/titles.rs:286-287` | Replace hardcoded labels with `metadata.field.dewey_code` i18n key (Task 6) |
| i18n key `metadata.field.dewey_code` | ✅ Exists in both locales | Reuse — don't duplicate |
| BnF provider parses Dewey | ❌ Not implemented | **Task 1** — add `676$a` parsing |
| `MetadataResult.dewey_code` | ❌ Missing field | **Task 1** — add field |
| `metadata_fetch` UPDATE includes Dewey | ❌ UPDATE omits `dewey_code` | **Task 2** — add to COALESCE clause |
| Re-download conflict/auto-update treats Dewey | ❌ Missing from `all_fields`, `field_label`, `get_*_field_value` | **Task 3** — add to the 4 helpers |
| `VolumeModel::find_by_location` Dewey sort | ❌ Not in whitelist | **Task 4** — add + NULL-last SQL |
| `VolumeWithTitle.dewey_code` | ❌ Missing field | **Task 4** — add field |
| `location_detail.html` Dewey column | ❌ Not rendered | **Task 4** — add `<th>` + `<td>` |
| `location.col_dewey` i18n key | ❌ Missing | **Task 5** — add to both locales |
| Grep gate: no hardcoded `"Dewey:"` | ❌ 2 occurrences | **Task 6** |
| E2E spec for Dewey | ❌ None | **Task 7** — new spec |

Keep the story scope to **exactly** the ❌ rows plus verification of AC #1. Don't touch anything in the ✅ rows.

### UNIMARC field 676 — authoritative spec

From the [UNIMARC Bibliographic Manual](https://www.ifla.org/unimarc-manual-bibliographic/) field 676 reference (and consistent with the existing BnF records in `tests/e2e/mock-metadata-server/`):

- **Tag 676** = "Dewey Decimal Classification"
- **Subfield $a** = the classification number itself, e.g. `843.914`
- **Subfield $v** = edition of the DDC schedule (e.g. `"23"` for 23rd edition)
- **Subfield $z** = language of the schedule edition used

For v1, extract only `676$a`. The edition and language subfields are metadata-about-metadata and not worth the complexity in v1 — the user cares about the classification string, not which DDC edition it came from.

**Example UNIMARC XML:**

```xml
<mxc:datafield tag="676" ind1=" " ind2=" ">
  <mxc:subfield code="a">843.914</mxc:subfield>
  <mxc:subfield code="v">23</mxc:subfield>
  <mxc:subfield code="z">fre</mxc:subfield>
</mxc:datafield>
```

The existing `Self::extract_subfield(xml, "676", "a")` helper handles this without modification — it's already namespace-aware (`<mxc:datafield>` variant) and trims whitespace.

### MariaDB NULL-last ORDER BY idiom — why `IS NULL` not `CASE`

MariaDB (and MySQL) place `NULL` values first by default in `ASC` order, last in `DESC`. Neither matches our "NULL always last" requirement. Three common idioms:

1. **`ORDER BY col IS NULL, col ASC`** ← chosen for this story
2. `ORDER BY CASE WHEN col IS NULL THEN 1 ELSE 0 END, col ASC`
3. `ORDER BY COALESCE(col, 'ZZZ'), col ASC`

Option 1 is the idiom used across the MariaDB docs and is the most readable. It works because `col IS NULL` evaluates to `0` for non-NULLs and `1` for NULLs, making non-NULLs sort first in both ASC and DESC on the main column. Option 2 is semantically identical but more verbose. Option 3 is fragile (requires a sentinel value that's greater than any real value) and unacceptable for free-text Dewey codes.

**Implementation:** only emit the `col IS NULL,` prefix when `sort_col == "dewey_code"`. For other sort columns, keep the existing `ORDER BY {} {}` to avoid cosmetic diff noise in test fixtures. A helper `fn order_by_clause(sql_col, sort_dir) -> String` is cleaner than inline conditional string building.

**Alphanumeric sort is sufficient — don't implement natural sort.** Dewey codes in the real world are always 3 digits before the decimal (100-999), so `"200"` < `"843.2"` < `"843.914"` is correct as plain string comparison. Natural sort (treating `"200"` vs `"843"` as integers) would only matter if Dewey codes crossed into `< 100`, which they never do per the DDC schedule. AC #3 says "alphanumerically" — take that literally.

### Not searchable / not filterable — hard constraint from FR118

FR118 is unambiguous: *"not searchable, not filterable"*. This is a **product decision** not a technical limitation — the rationale per UX spec §1.4 is that Dewey codes are (a) cryptic to non-library-pros and (b) prone to typos that would make search worthless. They exist purely to drive physical shelf ordering.

**Enforced by:**

- `VALID_SORT_COLUMNS` in `src/models/title.rs:568` MUST NOT include `dewey_code`. Guarded by unit test `test_validated_sort_rejects_dewey_code_on_search` (Task 4.4).
- No filter chip / facet / form field for Dewey on `templates/pages/home.html`. Grep gate: `grep -n "dewey" templates/pages/home.html` must return zero matches.
- No Dewey option in the browse sort dropdown at `templates/pages/home.html:82-86`.

If a future story needs Dewey-based discovery (e.g. "show me all my 843.x novels"), it should go through a dedicated "shelf sorting" view, not the search page. Out of scope for v1.

### Pre-existing quirk in `confirm_metadata` — you will inherit it, not fix it

Every `final_<field>` computation in `confirm_metadata` (lines 694-775) follows this pattern:

```rust
let final_publisher = if use_new("publisher", &form.accept_publisher, &manually_edited) {
    let v = non_empty(&Some(form.new_publisher.clone())); // non_empty returns None for ""
    if v != title.publisher { updated_count += 1; }
    if form.accept_publisher.is_some() { manually_edited.remove("publisher"); }
    v
} else { kept_count += 1; title.publisher.clone() };
```

**The quirk:** if a field is NOT in `manually_edited_fields` (auto-pre-filled case, `use_new` returns `true`) AND BnF returns no value for that field on re-download (form value arrives as `""`), then `non_empty` returns `None` and the existing value is **silently overwritten with NULL** — even though `build_auto_updates` correctly skipped the field (empty `new_val` guard at `src/services/title.rs:363`).

This bug exists today for `publisher`, `subtitle`, `description`, `age_rating`, `page_count`, `track_count`, `total_duration`, `issue_number`. The safer branch `apply_metadata_to_title` at line 833 uses `.or()` fallbacks and is not affected.

**What you do in story 5-8:** implement `final_dewey_code` using the **same pattern** as `final_publisher` — do NOT add a defensive `.or(title.dewey_code.clone())` for Dewey only. Consistency across fields is more valuable than a one-field fix, and the failure mode is rare in practice (BnF rarely drops tags it returned before, and the user has to be mid-confirm-flow on an unrelated conflict).

**Retro item (add to epic 5 retrospective):** fix all 8 `final_*` computations to use `.or(title.<field>.clone())` after `non_empty`, plus a regression test. Not this story's scope.

### `dewey_code VARCHAR(15)` length ceiling — accepted limit

The schema shipped with `dewey_code VARCHAR(15) NULL` (see `migrations/20260329000000_initial_schema.sql:64`). The epic text mentions `VARCHAR(32)` but that's aspirational — story 5-8 explicitly accepts the 15-char limit because:

- Real French literature DDC codes fit easily (`"843.914"` = 7 chars; full schedule with period-only separators rarely exceeds 12 chars).
- The widest realistic code I've seen in BnF UNIMARC samples is `"843.91409440944"` (15 chars exactly — edge case).
- A migration to expand to 32 chars would be a breaking schema change late in epic 5 for a field that's never the limiting constraint.
- MariaDB silently truncates `VARCHAR` overflow without raising an error — **if** a future BnF record returns a 16+ char code, it would be truncated silently on insert. This is known and accepted for v1.

**Verification:** Task 2.2's integration test should seed a realistic 10-12 char Dewey (e.g. `"843.914094"`) to exercise the common case, not just the 3-digit minimal case. Task 1.3's BnF parse test can stay with `"843.914"` (shortest realistic).

If Guy sees truncation happen in production in v2, open a dedicated migration story to widen the column. Not in scope here.

### `manually_edited_fields` — cumulative, set-based

When a librarian edits the Dewey code from `NULL` to `"843.914"`, that single act adds `"dewey_code"` to the title's `manually_edited_fields` JSON array. On re-download:

- If BnF still returns `"843.914"`, no conflict (values match) — the field stays in `manually_edited_fields`.
- If BnF returns a different value, the confirmation screen shows a conflict. If the user accepts, `"dewey_code"` is **removed** from `manually_edited_fields` (see `manually_edited.remove("dewey_code")` pattern at `src/routes/titles.rs:697`, etc.). If the user keeps manual, it stays in the set.
- If BnF returns `None` and the current value is non-null, the field is neither a conflict nor an auto-update (because `build_auto_updates` filters `if !new_val.is_empty()`). Current value is preserved. ✓ correct.

**Edge case from test 5.1 (pre-fill → later manual edit):** if BnF pre-fills Dewey on creation (auto-populated, not manually edited), then the librarian later edits it manually, `manually_edited_fields` gains `"dewey_code"`. That's correct — the current UX path is that any edit via the title edit form tracks the field as manually edited (see `src/routes/titles.rs:457-460` manually_edited merge). No special handling needed for Dewey.

### DB-backed integration test crate pattern — established in 5-7

Story 5-7 set up the `tests/find_similar.rs` integration test crate with dedicated MariaDB 10.11 on port 3307. The pattern:

```rust
#[sqlx::test(migrations = "./migrations")]
async fn my_test(pool: MySqlPool) {
    // pool is a freshly provisioned DB with all migrations applied
    // and the CI runner drops it on teardown
}
```

Requirements that are already met:

- `tests/docker-compose.rust-test.yml` runs MariaDB 10.11 on port 3307 with root password `root_test`.
- `Cargo.toml` has `sqlx` feature `migrate` enabled for the test crate.
- Seed data for `genres`, `contributor_roles`, `volume_states`, `location_node_types` is auto-applied by the bootstrap migrations — you can rely on `genre_id = 1` and `role_id = 1` (Auteur) in tests without explicit INSERTs.
- `media_type` is an enum — valid values are `'book'`, `'bd'`, `'cd'`, `'dvd'`, `'magazine'`, `'report'` (from `migrations/20260329000000_initial_schema.sql:56`).

**New files for this story:**

- `tests/metadata_fetch_dewey.rs` — AC #10 integration test for `update_title_from_metadata` (Task 2.2).
- `tests/find_by_location_dewey.rs` — AC #11 NULL-last sort order (Task 4.5).

Both run via `cargo test --test <name>` with `DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test'` set. Each test gets a fresh DB so there's no ordering coupling.

**No Cargo.toml changes needed.** `Cargo.toml` has no `[[test]]` sections — any new file under `tests/*.rs` is auto-discovered by Cargo as an integration test crate. `sqlx` is already at `features = ["runtime-tokio", "mysql", "chrono", "json", "migrate"]` which is sufficient for `#[sqlx::test]`.

**Do NOT merge Dewey tests into `tests/find_similar.rs`.** That file is scoped to `TitleModel::find_similar`. Keep test files topic-focused for discoverability.

### Task 3 subtlety — `apply_metadata_to_title` vs `confirm_metadata`

The re-download flow has two paths through `src/routes/titles.rs`:

1. **No manual edits:** `redownload_metadata` → `apply_metadata_to_title` (helper fn, search for it near line 554) → `TitleModel::update_metadata`. This path runs when `manually_edited.is_empty()`.
2. **Has manual edits:** `redownload_metadata` → render `MetadataConfirmTemplate` → user clicks Apply → `confirm_metadata` handler → `TitleModel::update_metadata`. This path runs when `manually_edited` is non-empty.

**Both paths must propagate `dewey_code`:**

- Path 1 (`apply_metadata_to_title`): pass `metadata.dewey_code.as_deref()` as the `dewey_code` argument to `update_metadata` instead of `title.dewey_code.as_deref()`. This ensures the no-conflict re-download replaces stale Dewey with fresh BnF Dewey.
- Path 2 (`confirm_metadata`): compute `final_dewey_code` via the `use_new(...)` pattern and pass it through. User consent is required because the field is in `manually_edited_fields`.

**Test both paths with unit tests** (Task 3.4). The service-layer tests in `TitleService::build_*_updates` cover the conflict-detection logic; the route-layer tests cover the wiring. Don't skip either.

### Previous story intelligence — learnings from 5-6 and 5-7

From `_bmad-output/implementation-artifacts/5-7-similar-titles-section.md` and `5-6-browse-list-grid-toggle.md`:

- **i18n proc-macro refresh:** After adding YAML keys, `touch src/lib.rs && cargo build` is **required**. `cargo check` alone does not re-trigger the `rust_i18n` proc macro. Confirmed in stories 5-6 and 5-7.
- **MariaDB BIGINT UNSIGNED decoding:** Not relevant to this story — Dewey is VARCHAR. But if you add any `id` columns to the location-view SELECT, use `CAST(... AS SIGNED)` and `row.try_get::<i64, _>` per CLAUDE.md gotcha #3.
- **SQLx offline cache:** Dynamic `sqlx::query()` (no macros) does not regenerate `.sqlx/` cache. You won't need to run `cargo sqlx prepare` unless you introduce a new typed macro query — none required for this story.
- **E2E parallel mode:** `fullyParallel: true` is the baseline. Unique `specIsbn("DC", seq)` per ISBN, unique L-codes / location names per test. `loginAs(page)` in `beforeEach` (NEVER inject `DEV_SESSION_COOKIE`).
- **Dark mode parity:** Every new Tailwind class in `location_detail.html` needs a `dark:` variant. Follow the existing `bg-stone-50 dark:bg-stone-800/50`, `text-stone-600 dark:text-stone-400` pattern.
- **E2E parallel loan deadlocks:** Not relevant here — this story doesn't create loans. No retry logic needed.

### Git intelligence — recent commit patterns

```
89188fc Fix story 5-1c: Epic 4 loan/borrower spec parallel-mode flakes
700bff7 Implement story 5-7: Similar titles section
fcc4244 Fix code review findings for story 5-6: browse toggle
7922269 Implement story 5-6: Browse list/grid toggle with persistent preference
978425f Implement Epic 5 stories 5-1b through 5-5: series management & E2E stabilization
```

- Expect a code-review fix-up commit after initial implementation — the code-review workflow has caught ≥1 Medium-severity finding on every Epic 5 feature story (5-3 through 5-7). Plan for it in the schedule, don't be surprised.
- Stories 5-1b/5-1c stabilized the E2E suite at 131/131. **Don't regress this.** If the new `dewey-code.spec.ts` fails intermittently, root-cause it (parallel isolation? helper races?) rather than marking it flaky or moving to serial mode.

### Scope boundaries — OUT of scope

- **Not in scope:** Making Dewey searchable or filterable (FR118 hard rule).
- **Not in scope:** Adding Dewey sort to `home.html` browse dropdown.
- **Not in scope:** Adding Dewey parsing to Google Books / Open Library / other secondary providers. BnF is the only v1 source.
- **Not in scope:** Dewey validation (format check, DDC range check). Free text — users may enter custom schemes (`"800 [librarian-custom]"`) intentionally.
- **Not in scope:** Natural/numeric sort. Plain string sort is correct per DDC structure.
- **Not in scope:** Adding Dewey to the home-page title card or similar-titles card (both are minimal by design).
- **Not in scope:** Migration. The column already exists at `VARCHAR(15) NULL`. The epic text says `VARCHAR(32)` but that's stale — ignore it.
- **Not in scope:** Dewey edit history / audit log (general project concern, not Dewey-specific).

### Anti-patterns to avoid (disaster prevention)

1. ❌ **Adding `dewey_code` to `VALID_SORT_COLUMNS` in `src/models/title.rs`** — that whitelist is for the home search page, which is FR118-banned from Dewey. Guarded by unit test (Task 4.4). Only `LOCATION_SORT_COLUMNS` in `src/models/volume.rs` may gain `dewey_code`.
2. ❌ **NULL-last via `COALESCE(col, 'ZZZ')`** — fragile sentinel, breaks on values > `'ZZZ'`. Use `ORDER BY col IS NULL, col <dir>`.
3. ❌ **Natural/numeric sort** — not needed, and introducing it changes semantics ("100" vs "10" ordering would flip). String sort is correct for Dewey.
4. ❌ **Duplicating `metadata.field.dewey_code` as a new i18n key** — reuse the existing key. Check `locales/en.yml:285` and `fr.yml:285` before adding anything.
5. ❌ **Hardcoding `"Dewey:"` in any new template or route code** — always use `rust_i18n::t!("metadata.field.dewey_code")`. The grep gate in Task 6.3 enforces this.
6. ❌ **Touching the epics.md `VARCHAR(32)` text** — that's a stale comment; the initial schema shipped with `VARCHAR(15)` and it's been in prod for 2 weeks. Don't create a migration to "fix" it.
7. ❌ **Clobbering existing `dewey_code` when BnF returns `None`** — use `COALESCE(?, dewey_code)` in the UPDATE. Integration test in Task 2.2 verifies this.
8. ❌ **Unit-testing the UI sort dropdown wiring** — that's what the E2E test is for. Unit tests should cover the SQL and the helper functions only.
9. ❌ **Skipping the hardcoded-label cleanup "because it's cosmetic"** — AC #8 is an explicit acceptance criterion because the project-wide pattern is "ALL user-facing text goes through t!". This story is the right time to close the gap.
10. ❌ **Injecting `DEV_SESSION_COOKIE` in E2E tests** — use `loginAs(page)` (Foundation Rule #7). Story 5-1b/5-1c stabilized the suite; do not reintroduce the pollution.
11. ❌ **`waitForTimeout(N)` in E2E** — every wait is an explicit DOM-state assertion.

### AC #3 "catalog" wording — disambiguation

The epic AC text says "Given a catalog sort by Dewey code, when applied, then titles are sorted alphanumerically by dewey_code with NULL values last." The word "catalog" is used loosely here to mean "physical collection view" — the authoritative source is **FR24** and the UX spec:

- **FR24 (PRD line):** *"Any user can view the contents of a storage location sorted by title, author, genre, or Dewey code"*
- **UX spec §Location-contents-view line 1297:** *"Location content view: sortable by title, author, genre, and Dewey code. Dewey sort places titles with Dewey code first (ascending), titles without Dewey grouped at end (NULL last)"*

Both are unambiguous: **Dewey sort lives on the location-detail page `/location/{id}`, not on `/` (home/browse).** This story implements exactly that and adds the FR118-enforcement unit test to guarantee Dewey does NOT leak into home search sort.

If Guy decides later that a global "show my library sorted by Dewey" view is valuable, that's a new story. Do not pre-build it.

### Performance budget — not a concern

- `VolumeModel::find_by_location` returns paginated 25/page via LIMIT. The NULL-last ORDER BY adds negligible cost (`IS NULL` is a constant expression). No index on `dewey_code` exists — and none is needed: location views have at most a few hundred volumes per location in realistic libraries, and the query filters on `v.location_id = ?` first (indexed via `idx_volumes_location` — verify with `SHOW INDEX FROM volumes;` if unsure).
- Async metadata-fetch UPDATE is a single-row update keyed on `id`. Adding one more `COALESCE` column has zero measurable cost.
- Re-download conflict scan runs in-memory on the loaded title + metadata — no DB cost.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` lines 857-870 — Story 5.8 ACs]
- [Source: `_bmad-output/planning-artifacts/prd.md` line 784 — FR118 canonical definition (*"optional, pre-filled by BnF API when available, used for physical sort order only — not searchable, not filterable, NULL values sorted last"*)]
- [Source: `_bmad-output/planning-artifacts/prd.md` line 39 — FR24 canonical location-view sort definition]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` lines 1257, 1297, 1385 — Location contents view + Dewey data model note]
- [Source: `_bmad-output/planning-artifacts/architecture.md` line 1048 — Dewey wiring map: `routes/titles.rs → models/title.rs → pages/title_detail.html`]
- [Source: `migrations/20260329000000_initial_schema.sql` lines 50-78 — `titles` table with `dewey_code VARCHAR(15) NULL`]
- [Source: `src/metadata/provider.rs` lines 11-26 — `MetadataResult` struct to extend]
- [Source: `src/metadata/bnf.rs` lines 34-120 — `parse_sru_response` and the `extract_subfield` helper]
- [Source: `src/tasks/metadata_fetch.rs` lines 71-129 — `update_title_from_metadata` UPDATE statement to extend]
- [Source: `src/models/title.rs` lines 22-76 — `TitleModel` struct with `dewey_code` already wired]
- [Source: `src/models/title.rs` lines 267-315 — `update_metadata` signature (already accepts `dewey_code`)]
- [Source: `src/models/title.rs` lines 567-595, 785-796 — `validated_sort` whitelist + unit tests (AC #4 guard)]
- [Source: `src/models/volume.rs` lines 213-330 — `VolumeWithTitle`, `LOCATION_SORT_COLUMNS`, `find_by_location` method]
- [Source: `src/routes/titles.rs` lines 286-310 — `metadata_display_html` hardcoded `"Dewey:"` (Task 6.2)]
- [Source: `src/routes/titles.rs` lines 397-477 — `create_title_from_scan` (no changes needed; already passes `dewey_code` through)]
- [Source: `src/routes/titles.rs` lines 540-792 — `redownload_metadata` + `confirm_metadata` (Task 3.2)]
- [Source: `src/services/title.rs` lines 320-420 — `build_field_conflicts`, `build_auto_updates`, `field_label`, `get_*_field_value` helpers (Task 3.1)]
- [Source: `src/routes/locations.rs` lines 18-110 — `LocationDetailTemplate` + handler (Task 4.6)]
- [Source: `templates/pages/location_detail.html` lines 36-86 — table header + body (Task 4.7)]
- [Source: `templates/pages/title_detail.html` lines 42-44 — hardcoded `"Dewey:"` (Task 6.1)]
- [Source: `templates/fragments/title_edit_form.html` lines 54-58 — edit form Dewey input (verify AC #1)]
- [Source: `locales/en.yml` lines 240-247, 277-290 — `location:` block + `metadata.field.dewey_code` key]
- [Source: `locales/fr.yml` lines 240-247, 277-290 — same, French]
- [Source: `tests/find_similar.rs` lines 1-100 — DB-backed integration test pattern to replicate (Tasks 2.2, 4.5)]
- [Source: `tests/docker-compose.rust-test.yml` — dedicated MariaDB on port 3307]
- [Source: `CLAUDE.md` — Foundation Rules 1-9, MariaDB gotchas, E2E parallel patterns, i18n proc-macro rule]
- [Source: `_bmad-output/implementation-artifacts/5-7-similar-titles-section.md` — previous story, DB-test infra precedent]
- [Source: `_bmad-output/implementation-artifacts/5-6-browse-list-grid-toggle.md` — home sort dropdown pattern (read-only reference for AC #4 verification)]
- [Source: `_bmad-output/implementation-artifacts/3-5-metadata-editing-and-redownload.md` line 403 — acknowledged "Confirmation flow ignores dewey_code" patch history]
- [Source: `tests/e2e/helpers/isbn.ts` — `specIsbn` generator]
- [Source: `tests/e2e/helpers/auth.ts` — `loginAs` helper]
- [Source: `tests/e2e/specs/journeys/shelving.spec.ts` — canonical pattern for assigning a volume to a location via scan]
- [Source: `tests/e2e/specs/journeys/metadata-editing.spec.ts` — canonical pattern for the edit-metadata click + form fill + HTMX swap wait]

### Project Structure Notes

Alignment with unified project structure is clean — all changes land in existing modules with existing conventions:

- Metadata parsing → `src/metadata/bnf.rs` + `src/metadata/provider.rs` (one new field on `MetadataResult`).
- Async task UPDATE → `src/tasks/metadata_fetch.rs` (add one `COALESCE` clause + one bind).
- Service layer → `src/services/title.rs` (extend 4 helpers with one new match arm each).
- Route handler → `src/routes/titles.rs` (extend `MetadataConfirmForm`, `MetadataConfirmTemplate`, `confirm_metadata`, `apply_metadata_to_title`, and `TitleDetailTemplate`).
- Model + whitelist → `src/models/volume.rs` (add to `LOCATION_SORT_COLUMNS`, `map_location_sort_column`, `VolumeWithTitle`, SQL SELECT and ORDER BY).
- Route handler → `src/routes/locations.rs` (add `col_dewey` field + populate).
- Template → `templates/pages/location_detail.html` (add `<th>` + `<td>`).
- Template → `templates/pages/title_detail.html` (swap hardcoded `"Dewey:"` for label).
- Confirmation template → `templates/fragments/metadata_confirm.html` (add `new_dewey_code` hidden input + optional accept checkbox).
- i18n → `locales/en.yml`, `locales/fr.yml` (add `location.col_dewey` to both).
- Unit tests → colocated in each modified module.
- DB integration tests → `tests/metadata_fetch_dewey.rs` (new), `tests/find_by_location_dewey.rs` (new).
- E2E → `tests/e2e/specs/journeys/dewey-code.spec.ts` (new, unique `specId = "DC"`).

**No schema migration.** No new dependencies. No new shared utilities. No architectural deviations.

### Review Findings

Code review run on 2026-04-12 (3 parallel layers: Blind Hunter, Edge Case Hunter, Acceptance Auditor).

**Decisions resolved (2026-04-12):**

- [x] [Review][Decision] **[HIGH] VARCHAR(15) truncation risk on BnF Dewey codes** — resolved with option 3 (widen column). Added migration `migrations/20260412000001_widen_dewey_code.sql` expanding `titles.dewey_code` from VARCHAR(15) to VARCHAR(32). Added `test_extended_length_dewey_roundtrips` in `tests/metadata_fetch_dewey.rs` exercising a 22-char extended DDC notation.

**Patches applied (2026-04-12):**

- [x] [Review][Patch] **[HIGH] E2E Test 2 strengthened with 3 titles** [tests/e2e/specs/journeys/dewey-code.spec.ts] — now seeds 3 titles (Dewey "200", NULL, Dewey "900"), giving non-trivial assertions in both directions: ASC=[200,900,NULL], DESC=[900,200,NULL]. A buggy impl without the `IS NULL` prefix in DESC would fail. Test refactored with `scanAndShelve` / `setDeweyViaEdit` helper closures for readability.
- [x] [Review][Patch] **[MEDIUM] E2E column index 4 removed in favor of semantic selector** [tests/e2e/specs/journeys/dewey-code.spec.ts] — NULL-Dewey rows are now asserted via `await expect(rows.nth(N).locator("code")).toHaveCount(0)` (since the `<code>` element only renders for non-NULL Dewey). Resilient to future column-order changes.
- [x] [Review][Patch] **[LOW] `build_auto_updates` test assertion tightened** [src/services/title.rs] — assertion now requires `u.contains("Dewey")` in addition to the two value substrings, pinning to the Dewey update line.

- [x] [Review][Defer] **[MEDIUM] `confirm_metadata` clears `manually_edited_fields` flag when accept is checked regardless of empty value** [src/routes/titles.rs:784-789] — deferred, pre-existing pattern across all 8 confirm fields (publisher, subtitle, description, etc.). Fix requires suite-wide change; Dev Notes explicitly noted it as retro item.
- [x] [Review][Defer] **[MEDIUM] Background fetch overwrites manual edits during race** [src/tasks/metadata_fetch.rs:89-119] — deferred, pre-existing: `update_title_from_metadata` uses raw UPDATE with no `version` check and no `manually_edited_fields` guard. Affects all fields (publisher, subtitle, etc.), not just Dewey. Cross-cutting retro item.
- [x] [Review][Defer] **[MEDIUM] E2E page_count "fill 0" workaround papers over pre-existing 422 bug** [tests/e2e/specs/journeys/dewey-code.spec.ts:36-40, metadata-editing.spec.ts:48-53] — deferred, pre-existing: form handler treats empty numeric field as invalid i32 and returns 422. Pattern exists in metadata-editing.spec.ts. File as a separate bug.
- [x] [Review][Defer] **[MEDIUM] Pagination ORDER BY lacks secondary tiebreaker for stable paging** [src/models/volume.rs:287-306] — deferred, pre-existing: all location sorts (`title`, `genre_name`, `primary_contributor`, and now `dewey_code`) lack `v.id ASC` tiebreaker. Rows with identical sort keys can reorder across pages, causing skips/duplicates. Amplified by Dewey sort where NULLs are common. Cross-cutting retro item.
- [x] [Review][Defer] **[LOW] i18n labels not html_escaped in `format!`-based responses** [src/routes/titles.rs:288] — deferred, pre-existing pattern across many `format!` calls in `metadata_display_html` and elsewhere. Low risk (translator-controlled). Cross-cutting cleanup.
- [x] [Review][Defer] **[LOW] `extract_subfield` picks first 676 only when multiple DDC classifications present** [src/metadata/bnf.rs:113] — deferred, minor: BnF records with multiple 676 datafields (fiction + translation studies, etc.) get the arbitrary first. Deterministic but could be smarter in v2. Same behavior as all other UNIMARC fields.

**Dismissed as noise/false-positive (11 items):** whitespace-only trim already handled in `extract_subfield`; SQL `format!` inputs are whitelisted; `apply_metadata_to_title` manually_edited check unnecessary (upstream guard); Askama autoescape covers auto_updates XSS path; real DDC codes always 3-digit prefix (lex sort == numeric sort); `metadata_cache.rs` change is correct scope (spec File List was incomplete); AC #10 branch coverage is actually complete across the two tests; E2E loose regex and unique L-code are canonical patterns; cosmetic spec transcription errors.

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- **Empty numeric form fields cause 422:** E2E edit form submit via HTMX fails silently when `page_count` input is empty. Fix: fill with "0" before submit (canonical pattern from metadata-editing.spec.ts).
- **Docker container rebuild required:** Code changes require `docker compose up --build` for E2E tests.
- **Location tree link pattern:** `/locations` page uses edit links (`/locations/{id}/edit`), not detail links. Extract ID from edit link href.
- **Sort arrow in th text:** Column header includes ▲/▼ when active; loosened regex from `/^Dewey$/` to `/Dewey/`.
- **metadata_cache.rs update:** Adding field to `MetadataResult` required updating JSON round-trip in cache model.

### Completion Notes List

- Added `dewey_code: Option<String>` to `MetadataResult`, parsed UNIMARC 676$a in BnF provider, updated metadata_cache JSON round-trip.
- Added `COALESCE(?, dewey_code)` to async metadata-fetch UPDATE for BnF pre-fill without clobbering existing values.
- Extended re-download conflict flow: 4 service helper additions, `MetadataConfirmForm`/`MetadataConfirmTemplate`/`confirm_metadata`/`apply_metadata_to_title` wiring, hidden input in metadata_confirm.html.
- Added Dewey sort to location-view: `VolumeWithTitle` field, whitelist, `ORDER BY t.dewey_code IS NULL, t.dewey_code <dir>`, template column with monospace rendering.
- Replaced 2 hardcoded `"Dewey:"` literals with i18n label. Added `label_dewey_code` to `TitleDetailTemplate`.
- 326 unit tests (+9 new), 17 DB integration tests (+5 new), 133 E2E tests (+2 new). All green.

**Verification gate:** ✅ clippy, ✅ 326 unit tests, ✅ 17 integration tests, ✅ sqlx prepare check, ✅ 133/133 E2E

### File List

**Created:**
- `migrations/20260412000001_widen_dewey_code.sql` — widen `titles.dewey_code` from VARCHAR(15) to VARCHAR(32) (code review decision: accommodate extended BnF DDC notations)
- `tests/metadata_fetch_dewey.rs` — 4 DB integration tests (incl. 22-char extended-length roundtrip)
- `tests/find_by_location_dewey.rs` — 2 DB integration tests
- `tests/e2e/specs/journeys/dewey-code.spec.ts` — 2 E2E tests (Test 2 strengthened with 3 titles for non-trivial NULL-last verification)

**Modified:**
- `src/metadata/provider.rs` — `dewey_code` field on `MetadataResult`
- `src/metadata/bnf.rs` — 676$a extraction + 2 tests
- `src/models/metadata_cache.rs` — JSON round-trip for `dewey_code`
- `src/tasks/metadata_fetch.rs` — COALESCE dewey_code in UPDATE + pub visibility
- `src/services/title.rs` — 4 helper extensions + 2 tests
- `src/routes/titles.rs` — confirm form/template/handler + apply_metadata + label + test fixes
- `src/models/volume.rs` — whitelist + SQL + struct + 3 tests
- `src/routes/locations.rs` — col_dewey field + test fix
- `templates/pages/location_detail.html` — Dewey column
- `templates/pages/title_detail.html` — i18n label
- `templates/fragments/metadata_confirm.html` — hidden input
- `locales/en.yml` — `location.col_dewey`
- `locales/fr.yml` — `location.col_dewey`
