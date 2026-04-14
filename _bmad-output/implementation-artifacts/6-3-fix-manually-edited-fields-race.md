# Story 6.3: Fix `manually_edited_fields` + background-fetch race

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a librarian,
I want my manually-edited metadata to survive a concurrent background metadata fetch (and to survive re-accepting a previously-kept value during re-download),
so that typing over an auto-populated field is never silently overwritten.

## Scope at a glance (read this first)

**Two defects of the same feature ("don't overwrite user edits"), fixed together.** No new FR/NFR — this is an NFR11 (reliability) + NFR28 (data integrity) pay-down, lifted verbatim from the Epic 5 retro action-item list (2026-04-13) and tracked in `deferred-work.md` under "Deferred from: code review of story 5-8-dewey-code-management (2026-04-12)".

**Defect A — Background-fetch race (server-side).** `src/tasks/metadata_fetch.rs::update_title_from_metadata` (lines 71–131) performs a raw UPDATE with `COALESCE(?, col)` semantics, **no `version` optimistic-lock check and no `manually_edited_fields` guard**. If a librarian edits a field manually while the background metadata fetch is still resolving (scan → skeleton → async BnF lookup → UPDATE arrives late), the fetch silently overwrites the manual edit. Affects all 12 auto-filled fields (title, subtitle, description, publisher, language, page_count, publication_date, dewey_code, track_count, total_duration, age_rating, issue_number).

**Defect B — Confirm-metadata flag wipe (HTTP path).** `src/routes/titles.rs::confirm_metadata` (lines 674–825) clears `manually_edited_fields` for a field whenever `accept_<field>` is checked, **regardless of whether the form value actually differs from the kept value**. Net effect: a user who re-accepts an existing manual override loses the "manually edited" marker, and the *next* re-download silently auto-overwrites. Pattern is identical across all 12 `final_<field>` blocks.

**Explicitly NOT in scope:**
- Epic 7 role/session work (this story is pre-Epic-7 groundwork).
- Adding CSRF tokens to `confirm_metadata` (deferred since 1-2, separate cross-cutting item).
- Re-plumbing the 422 empty-numeric-field bug in `update_title` (separate `deferred-work.md` item under 5-8).
- Adding optimistic locking to cover-image or other UPDATEs — scope is strictly `update_title_from_metadata` + `confirm_metadata`.
- Changing the skeleton/pending-updates UX or the mock metadata server.
- Guarding contributor auto-add (`add_author_contributor` at `src/tasks/metadata_fetch.rs:124-128`): contributors are tracked via the `title_contributors` junction, not via the `manually_edited_fields` JSON. Extending the guard to contributor rows is a separate (future) story.
- Adding a new Playwright E2E spec. The race is timing-sensitive and not reliably reproducible in a browser; the regression guards are the 133+/133+ existing E2E suite (must stay green) plus the new `metadata_fetch_race.rs` DB integration tests. See AC #9 waiver note.

## Acceptance Criteria

1. **Background-fetch respects manual edits — server guard:** Given a title whose `manually_edited_fields` JSON array contains a field name F, when `update_title_from_metadata` fires for that title (regardless of how fresh the background fetch's snapshot is), then column F is NOT modified. All other non-manually-edited columns still get the `COALESCE(?, col)` fill-in behavior. Verified by an integration test (`#[sqlx::test(migrations = "./migrations")]`).
2. **Background-fetch respects concurrent manual edits — version guard:** Given a title at `version = N`, when a manual edit increments the version to `N+1` between the moment the background task loaded its snapshot and the moment it runs its UPDATE, then the task's UPDATE must affect zero rows (or be re-resolved against the new version) — the manual edit must win. Verified by an integration test that interleaves a manual `update_metadata` call between the background task's snapshot read and its UPDATE.
3. **Background-fetch failure is non-fatal:** Given the conflict case from AC #2, when the UPDATE affects zero rows, then the task logs at `tracing::info!` or `tracing::warn!` (NOT `error!`), still calls `mark_resolved` on `pending_metadata_updates` (the fetch itself succeeded, just lost the write race), and does NOT propagate as an `AppError` to the caller. Rationale: the user's manual edit is the intended final state; the background fetch losing is a feature, not a failure.
4. **Flag wipe fixed — accept with same value keeps flag:** Given a field F with `manually_edited_fields` containing F, when `confirm_metadata` is submitted with `accept_F` checked AND the submitted `new_F` value equals the previously-kept value (i.e. `title.F`), then F remains in `manually_edited_fields` after the UPDATE. The "manually edited" marker is preserved.
5. **Flag wipe fixed — accept with different value clears flag:** Given a field F with `manually_edited_fields` containing F, when `confirm_metadata` is submitted with `accept_F` checked AND the submitted `new_F` value **differs** from `title.F` (the user accepted the replacement), then F is removed from `manually_edited_fields` after the UPDATE. This preserves the existing "accept replacement" semantics.
6. **Flag wipe fixed — decline keeps flag:** Given a field F with `manually_edited_fields` containing F, when `confirm_metadata` is submitted with `accept_F` absent (box unchecked), then the old value is preserved and F remains in `manually_edited_fields` (pre-existing behavior, regression-guarded by AC #4's test parametrization).
7. **All 12 fields behave identically:** AC #4–#6 are verified for at least three representative fields covering the field shapes: `publisher` (Option<String>), `dewey_code` (Option<String> with `non_empty` semantics), and `page_count` (Option<i32>). The fix is applied to all 12 `final_<field>` blocks uniformly — no field is skipped, no field gets a bespoke branch. Unit-test both branches (same-value and different-value) per representative field.
8. **Race integration test:** A DB-backed integration test (pattern: `#[sqlx::test(migrations = "./migrations")]`, new file `tests/metadata_fetch_race.rs`) covers the canonical scenario: (1) seed a title, (2) build a `MetadataResult` with fresh values for publisher and dewey_code, (3) simulate a manual edit that stamps `publisher` into `manually_edited_fields` and bumps `version`, (4) call `update_title_from_metadata` with the pre-manual-edit version context, (5) re-fetch the title and assert publisher is the manual value AND dewey_code IS the new metadata value (i.e. fields NOT in `manually_edited_fields` still auto-fill). This is the "mixed guard" test that proves the guard is field-level, not title-level.
9. **Full green suite (with explicit E2E waiver):** `cargo clippy -- -D warnings`, `cargo test`, `cargo sqlx prepare --check --workspace -- --all-targets`, the DB-integration crates via `tests/docker-compose.rust-test.yml` (including the new `metadata_fetch_race` test), and `cd tests/e2e && npm test` (133+/133+ parallel) all pass. Foundation Rule #5 — no merge until green. **Foundation Rule #3 E2E-coverage waiver:** this story intentionally adds no new Playwright spec because the race is timing-sensitive and not reliably reproducible in-browser; the regression guards are (a) the full existing 133+ E2E suite staying green as before, and (b) the three new DB-backed integration tests in `tests/metadata_fetch_race.rs`. The waiver is story-local and does not change Rule #3 for future feature work.
10. **CI wiring:** The new `metadata_fetch_race` integration-test crate is added to the `db-integration` job in `.github/workflows/_gates.yml` alongside `find_similar`, `find_by_location_dewey`, `metadata_fetch_dewey`, `seeded_users`. If omitted, the race guard is untested in CI.

## Tasks / Subtasks

- [x] **Task 1 — Fix `confirm_metadata` flag wipe (Defect B)** (AC: #4, #5, #6, #7)
  - [x] 1.1 Open `src/routes/titles.rs`. Locate the 12 `final_<field>` blocks (lines 701–789, fields: title, subtitle, description, publisher, language, publication_date, page_count, track_count, total_duration, age_rating, issue_number, dewey_code).
  - [x] 1.2 For each block, change the flag-clear line from the current unconditional form `if form.accept_<field>.is_some() { manually_edited.remove("<field>"); }` to a conditional form that only clears when the accepted new value ACTUALLY differs from `title.<field>`. The "differs" comparison must match the field's existing same-type compare semantics (e.g. `final_publisher != title.publisher` for `Option<String>`, `final_page_count != title.page_count` for `Option<i32>`, `final_pub_date != title.publication_date` for `Option<NaiveDate>`). **Reuse the local `updated_count` sentinel** — the block already computes `if v != title.<field> { updated_count += 1; }` immediately above the remove, so the cleanest refactor is: compute `let changed = v != title.<field>;`, bump `updated_count` if changed, and clear the flag only when `form.accept_<field>.is_some() && changed`.
  - [x] 1.3 Do NOT hoist the pattern into a helper closure unless it's trivial (12 fields × 4 shapes — Option<String>, Option<i32>, Option<NaiveDate>, &str for title+language — a macro would be over-engineering for a 12-line change per field). Repetition is acceptable per the Foundation rules (three similar lines > premature abstraction). Keep the fix local and readable.
  - [x] 1.4 Verify by scanning: no `final_<field>` block should leave an unconditional `manually_edited.remove(...)` behind. Count occurrences before and after — should drop from 12 to 0.

- [x] **Task 2 — Unit tests for `confirm_metadata` flag-wipe fix** (AC: #4, #5, #6, #7)
  - [x] 2.1 In `src/routes/titles.rs` (`#[cfg(test)] mod tests`, existing at bottom of file — see `manually_edited_fields: None` literal at lines 967, 1043, 1134, 1157 for the existing test helper pattern), add parametrized unit tests for three representative fields: `publisher`, `dewey_code`, `page_count`. Each field needs three test cases: (a) accept_checked + same_value → flag KEPT, (b) accept_checked + different_value → flag REMOVED, (c) accept_unchecked → flag KEPT + old_value. That's 9 tests total.
  - [x] 2.2 If `confirm_metadata` can't be unit-tested without a DbPool (it reads `title` via `TitleModel::find_by_id`), extract the flag-manipulation decision into a small pure helper (e.g. `fn should_clear_flag(accept: &Option<String>, new_value_differs: bool) -> bool`) and unit-test the helper. Prefer direct route-level tests only if they already exist in the file (grep for existing `confirm_metadata` tests before adding a helper — if tests exist and mock the pool, extend them; if not, the helper is cleaner).
  - [x] 2.3 Run `cargo test titles::tests -- --nocapture` to confirm the new tests pass.

- [x] **Task 3 — Fix `update_title_from_metadata` race (Defect A — guard + version)** (AC: #1, #2, #3)
  - [x] 3.1 Open `src/tasks/metadata_fetch.rs`. Modify `update_title_from_metadata` (currently lines 71–131). Pre-UPDATE, load the current title snapshot with `TitleModel::find_by_id(pool, title_id).await?` to read both `manually_edited_fields` and `version`. If the title was soft-deleted between scan and fetch, return `Ok(())` silently (skeleton is gone; nothing to update). Use `?` — `find_by_id` already returns `AppError`. **Extract the actual UPDATE into a `pub(crate) async fn do_update(pool, title_id, metadata, snapshot: &TitleModel) -> Result<u64, AppError>` helper that returns `rows_affected`** (consumed by Task 4.3 Test 2 for faithful stale-snapshot race testing).
  - [x] 3.2 Parse `manually_edited_fields` via the existing `TitleModel::parsed_manually_edited_fields()` helper (`src/models/title.rs:318-323`) into a `HashSet<String>`. For each of the 12 auto-filled fields in the UPDATE, apply a per-field guard: if the field name is in the set, bind the **current column value** (i.e. the snapshot's value) instead of the metadata's new value. The `COALESCE(?, col)` clause then keeps the column unchanged regardless — but binding the snapshot value makes the "do nothing" explicit and eliminates any ambiguity about SQL-level COALESCE precedence when both sides are non-NULL.
  - [x] 3.3 Alternative simpler implementation (preferred if the diff stays small): for each field name F, bind `if manually_edited.contains("F") { None } else { metadata.F.clone() }`. Since the SQL uses `COALESCE(?, col)`, binding `NULL` keeps the existing column value. This requires zero change to the SQL string — only to the 12 `.bind(...)` calls. **Prefer this variant** — it's a strictly smaller diff and semantically equivalent.
  - [x] 3.4 Add the version check to the SQL: change `WHERE id = ? AND deleted_at IS NULL` to `WHERE id = ? AND version = ? AND deleted_at IS NULL` and append `.bind(snapshot.version)`. Also bump the version: add `version = version + 1` to the SET clause, consistent with `update_metadata` at `src/models/title.rs:288`.
  - [x] 3.5 Check `result.rows_affected()` after the UPDATE. If zero, a concurrent manual edit won the race. Log at `tracing::info!(title_id, "Background fetch lost race with manual edit; no-op")` and return `Ok(())` — do NOT return an error, do NOT call `mark_failed`. The caller (`fetch_metadata_chain`, line 38) should still proceed to `mark_resolved` because the fetch itself succeeded. **Important:** if you use `services::locking::check_update_result`, note that it returns `AppError::Conflict` — which the caller would propagate via the existing `if let Err(e) = update_title_from_metadata(...)` branch (line 38) and **trigger `mark_failed`**. That's wrong for this case. Handle the zero-rows branch inline and return `Ok(())`.
  - [x] 3.6 The contributor side-effect `add_author_contributor` (lines 124–128) should also be guarded: if the primary author field has been manually edited via the contributor UI, we should not auto-add. However, `manually_edited_fields` currently does NOT track contributors (it tracks only title-table columns). Keep the existing behavior: `add_author_contributor` still fires unconditionally — it uses `INSERT IGNORE` (line 175), so re-running it is safe and adds nothing if a matching (title, contributor, role) tuple exists. Document this in a 1-line comment so a future reader understands why contributors are not in the guard.
  - [x] 3.7 Regenerate `.sqlx/` offline cache: `cargo sqlx prepare --workspace -- --all-targets`. Commit any changed JSON files.

- [x] **Task 4 — Integration test for the race (Defect A)** (AC: #1, #2, #3, #8)
  - [x] 4.1 Create `tests/metadata_fetch_race.rs`. Follow the exact pattern of `tests/metadata_fetch_dewey.rs` (from story 5-8 Task 2.2): `#[sqlx::test(migrations = "./migrations")]`, `async fn name(pool: MySqlPool) { ... }`, seed a title via direct SQL insert, construct a `MetadataResult` literal, call `mybibli::tasks::metadata_fetch::update_title_from_metadata(&pool, title_id, &metadata).await.unwrap()`, re-fetch and assert.
  - [x] 4.2 Test 1 — **`manually_edited_field_is_not_overwritten`**: Seed a title with `publisher = Some("User's edit")`, `manually_edited_fields = Some(r#"["publisher"]"#)`, `dewey_code = NULL`. Call `update_title_from_metadata` with `MetadataResult { title: Some("x".into()), publisher: Some("BnF value".into()), dewey_code: Some("843.914".into()), ..Default::default() }`. Re-fetch and assert: `publisher == Some("User's edit")` (guard held) AND `dewey_code == Some("843.914")` (non-guarded field still filled). This is the AC #1 + AC #8 "mixed guard" proof.
  - [x] 4.3 Test 2 — **`version_check_blocks_stale_write`**: To faithfully exercise the real race, extract a `pub(crate)` helper in `src/tasks/metadata_fetch.rs`:

    ```rust
    pub(crate) async fn do_update(
        pool: &DbPool,
        title_id: u64,
        metadata: &MetadataResult,
        snapshot: &TitleModel,
    ) -> Result<u64, AppError>  // returns rows_affected
    ```

    The public `update_title_from_metadata` becomes: `let snapshot = TitleModel::find_by_id(pool, title_id).await?.ok_or(...)?; let rows = do_update(pool, title_id, metadata, &snapshot).await?; if rows == 0 { tracing::info!(...); }`. The test then (a) seeds a title at version=1, (b) captures its snapshot via `find_by_id`, (c) simulates a concurrent manual edit with a direct `UPDATE titles SET publisher = 'user', version = 2 WHERE id = ?`, (d) calls `do_update(&pool, id, &metadata, &stale_snapshot)` with the stale `version=1` snapshot, (e) asserts the returned `rows_affected == 0` AND a fresh `find_by_id` shows `publisher == Some("user")` (manual edit preserved) AND `version == 2` (no second bump). This is the canonical AC #2 proof.
  - [x] 4.4 Test 3 — **`all_fields_not_edited_still_fill`** (regression guard for AC #1's "does not break the happy path"): Seed a title with empty `manually_edited_fields = NULL` (no guarded fields). Call `update_title_from_metadata` with `MetadataResult` containing `publisher`, `dewey_code`, `language`. Assert all three fields were updated. This proves the new guard doesn't short-circuit the non-guarded path.
  - [x] 4.5 Verify: `docker compose -f tests/docker-compose.rust-test.yml up -d && SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test --test metadata_fetch_race`.

- [x] **Task 5 — CI wiring** (AC: #10)
  - [x] 5.1 Edit `.github/workflows/_gates.yml`. Locate the `db-integration` job's `cargo test --test <name>` list (established by stories 6-1, 5-8, 6-2 — it should already include `find_similar`, `find_by_location_dewey`, `metadata_fetch_dewey`, `seeded_users`). Add `metadata_fetch_race` to the list.
  - [x] 5.2 Push and verify the GitHub Actions run picks up and executes the new crate — inspect the logs for a line like `Running tests/metadata_fetch_race.rs` with 3 tests passing.

- [x] **Task 6 — Verification & sprint-status flip** (AC: #9)
  - [x] 6.1 Local gate, in order:
    - `cargo clippy -- -D warnings`
    - `cargo test` (unit tests, expect titles::tests additions from Task 2 + tasks::metadata_fetch::tests unchanged)
    - `cargo sqlx prepare --check --workspace -- --all-targets`
    - `docker compose -f tests/docker-compose.rust-test.yml up -d` then `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test --test find_similar --test find_by_location_dewey --test metadata_fetch_dewey --test seeded_users --test metadata_fetch_race`
    - `cd tests/e2e && npm test` — must stay 133+/133+ green in parallel mode.
  - [x] 6.2 Push to a feature branch, open PR, confirm the 3-job gate (rust-tests, db-integration, e2e) passes. Move `6-3-fix-manually-edited-fields-race` → `review` when opening the PR; → `done` after code review passes with no Medium+ findings (Foundation Rule #6).

### Review Findings (2026-04-14)

- [x] [Review][Decision→Accepted 2026-04-14] Version bump on background fetch introduces user-edit Conflict (High) — Resolved: accept as correct optimistic-locking behavior. Revisit if real-world reports appear. Follow-up UX polish (friendly 409 message / merge hint) tracked in `deferred-work.md`. [src/tasks/metadata_fetch.rs:157]
- [x] [Review][Patch] Malformed `manually_edited_fields` JSON silently disables guard (fail-open) [src/models/title.rs:318] — Fixed: `parsed_manually_edited_fields()` now logs `tracing::warn!` on parse failure, surfacing corruption instead of silently failing open.
- [x] [Review][Patch] `add_author_contributor` no longer runs on race loss — deviates from spec Task 3.6 [src/tasks/metadata_fetch.rs] — Fixed: restructured so `add_author_contributor` runs unconditionally after `do_update`; the zero-rows branch only logs and falls through (INSERT IGNORE keeps re-runs idempotent).
- [x] [Review][Patch] Integration test bypasses `update_title_from_metadata` [tests/metadata_fetch_race.rs] — Fixed: added `update_title_from_metadata_re_reads_snapshot` (exercises public fn with a fresh guard stamp) and `soft_deleted_title_between_scan_and_fetch_is_noop` (exercises the `None` soft-delete branch).
- [x] [Review][Patch→Dismissed] `do_update` visibility wider than spec [src/tasks/metadata_fetch.rs:128] — Spec's `pub(crate)` was inconsistent with its own Task 4.3 test contract: `tests/metadata_fetch_race.rs` is an external integration-test crate and cannot reach `pub(crate)` items. Kept `pub` and documented the constraint at the item.
- [x] [Review][Patch] Race-test hardening [tests/metadata_fetch_race.rs, src/routes/titles.rs#tests] — Fixed: `manually_edited_field_is_not_overwritten` now asserts `version == 2`; `version_check_blocks_stale_write` adds `dewey_code` + `language` to metadata and asserts both stay at pre-edit values; `should_clear_flag_*` tests now use `Some("on".to_string())` matching the documented form semantics.
- [x] [Review][Defer] `non_empty` trim asymmetry with legacy whitespace data [src/routes/titles.rs] — deferred, pre-existing
- [x] [Review][Defer] `publication_date` year-only metadata compares against full-date stored value [src/routes/titles.rs:~756] — deferred, pre-existing parse behavior
- [x] [Review][Defer] Numeric form fields cannot represent "clear to NULL" (masked by `.or(title.<field>)`) [src/routes/titles.rs:~768-803] — deferred, pre-existing

## Dev Notes

### Why two defects in one story

Both defects break the same implicit contract: *"once a librarian has flagged a field as manually edited, the system must never silently overwrite it"*. They come from different code paths (Defect A = async background write, Defect B = confirm-metadata form post), but fixing only one leaves the contract broken:

- Fix only A: a user confirms a re-download, re-accepts their existing value, the flag is wiped, the next background fetch (or next re-download) silently overwrites.
- Fix only B: the flag is preserved correctly, but the background-fetch UPDATE doesn't consult it, so any concurrent fetch still overwrites.

Epic 5 retro (2026-04-13) explicitly bundled these as **Action 2 ("land-mine fix")** blocking Epic 7 — the rationale is that once multi-role sessions arrive, the race window widens (two users can each trigger fetch + edit on the same title). Ship them together.

### Anatomy of Defect A (the race window)

Current flow at `src/routes/titles.rs:527-534` (scan-create path):

```
POST /titles (scan)
  → create_title_from_scan (sync DB INSERT of skeleton title)
  → tokio::spawn(fetch_metadata_chain(...))   ← returns immediately
  → HTTP 200 with skeleton feedback
```

Then, in parallel:

```
[background task]                           [user]
ChainExecutor::execute (BnF lookup, ~1–5s)  GET /titles/{id}/edit
                                            POST /titles/{id} (manual edit)
                                            manually_edited_fields = ["publisher"]
update_title_from_metadata                  ↓
  UPDATE titles SET publisher = ...         ← overwrites the manual edit
  (no version check, no flag check)
mark_resolved                               GET /titles/{id}
                                            ← sees BnF value, not user's
```

The window is "time from ChainExecutor start to UPDATE commit" — typically 1–5 seconds per `settings.metadata_fetch_timeout_secs`. Reliably reproducible if the user hits the edit form within that window.

### Anatomy of Defect B (the flag-wipe)

Current `final_<field>` block at `src/routes/titles.rs:722-727` (publisher example):

```rust
let final_publisher = if use_new("publisher", &form.accept_publisher, &manually_edited) {
    let v = non_empty(&Some(form.new_publisher.clone()));
    if v != title.publisher { updated_count += 1; }
    if form.accept_publisher.is_some() { manually_edited.remove("publisher"); }  // ← BUG
    v
} else { kept_count += 1; title.publisher.clone() };
```

The `if form.accept_publisher.is_some()` clears the flag unconditionally when the checkbox is checked. But the conflict resolution UI shows the accept checkbox for every conflict — even when the user wants to re-acknowledge their existing value (e.g. they re-download BnF metadata, BnF returns the same publisher, and the user confirms "yes, keep my manual value"). The UI has no distinction between "accept the replacement" and "re-affirm my override"; the server must decide that from the form values.

**Fix (one-line change per block):** only clear the flag when the new value **actually differs** from the prior kept value. The block already computes that comparison (`if v != title.publisher`) for the `updated_count` bump — reuse it:

```rust
let final_publisher = if use_new("publisher", &form.accept_publisher, &manually_edited) {
    let v = non_empty(&Some(form.new_publisher.clone()));
    let changed = v != title.publisher;
    if changed { updated_count += 1; }
    if form.accept_publisher.is_some() && changed { manually_edited.remove("publisher"); }  // ← FIX
    v
} else { kept_count += 1; title.publisher.clone() };
```

Apply the same shape to all 12 blocks. Don't over-engineer — 12 repetitive patches are easier to review than one macro.

### Anatomy of Defect A (the fix)

Minimal diff on `src/tasks/metadata_fetch.rs`:

1. Load the snapshot: `let snapshot = TitleModel::find_by_id(pool, title_id).await?.ok_or_else(|| ...)?;` (or return `Ok(())` if `None` — title was soft-deleted).
2. Parse the guard set: `let guarded: HashSet<String> = snapshot.parsed_manually_edited_fields().into_iter().collect();`
3. Transform each `.bind(&metadata.FIELD)` into `.bind(if guarded.contains("FIELD") { None } else { metadata.FIELD.clone() })` (for `Option<String>` fields) or equivalent for integer fields. The `COALESCE(?, col)` then keeps the column unchanged.
4. Add `AND version = ?` to the WHERE and `.bind(snapshot.version)`.
5. Add `version = version + 1` to the SET clause.
6. After `.execute(pool).await?`, branch on `rows_affected`: `0` → log info + return `Ok(())`; `1` → proceed to author-contributor logic.

**Snippet showing the `Option<i32>` vs `Option<String>` bind shapes** (page_count vs publisher):

```rust
.bind(if guarded.contains("publisher") { None } else { metadata.publisher.clone() })  // Option<String>
.bind(if guarded.contains("page_count") { None } else { metadata.page_count })       // Option<i32> is Copy
```

For `publication_date`, the existing code already transforms the string via `pub_date` — keep the transform, but then `.bind(if guarded.contains("publication_date") { None } else { pub_date })`.

**Atomicity note — why the version check is sufficient.** There is a small window between `find_by_id` (snapshot read) and the UPDATE where a concurrent manual edit could land. The version check is the atomicity guarantee: any concurrent edit bumps `version` from N to N+1, and our UPDATE's `WHERE version = N` then affects zero rows — the task no-ops and the manual edit is preserved. No explicit transaction or `SELECT ... FOR UPDATE` is needed; optimistic locking covers it.

**Pre-existing type smell for `issue_number` (do not fix here).** `MetadataResult.issue_number` is `Option<String>` (per `src/metadata/provider.rs:25`), but `TitleModel.issue_number` is `Option<i32>` and the column is `INT`. The existing `.bind(&metadata.issue_number)` relies on MariaDB string→int coercion. Out of scope. **Preserve the current bind shape verbatim when wrapping it in the guard** — whatever the current `.bind` expression is, wrap it as `.bind(if guarded.contains("issue_number") { None } else { <current expr> })`. Do not attempt to "fix" the coercion as part of this story; a separate story can normalize `MetadataResult.issue_number` to `Option<i32>` if desired.

**Do not conditionally rewrite the SQL string** — keep the `COALESCE(?, col)` form for all 12 fields. The guard lives entirely in the Rust-side bind logic. This keeps the `.sqlx/` cache diff minimal (only the SQL's `WHERE` clause changes, to add `AND version = ?`).

### Why `check_update_result` is NOT the right helper here

`services::locking::check_update_result` (at `src/services/locking.rs:5-13`) returns `AppError::Conflict` on zero-rows. That's the right behavior for a **user-initiated** optimistic-lock failure (the user sees a 409 and retries with the fresh version). But in our case the caller is a detached `tokio::spawn`ed task — there's no user to retry, and the loss is semantically correct (manual edit wins by design). Returning `AppError::Conflict` would propagate to `fetch_metadata_chain` line 38, trigger the existing `if let Err(e)` branch, and incorrectly call `mark_failed` on `pending_metadata_updates` — which would then render the user a "fetch failed" OOB banner instead of "fetch resolved". Handle the zero-rows case inline:

```rust
let result = sqlx::query(...).bind(...).execute(pool).await
    .map_err(|e| AppError::Internal(format!("Failed to update title: {e}")))?;
if result.rows_affected() == 0 {
    tracing::info!(title_id, "Background fetch lost race with concurrent manual edit; no-op");
    return Ok(());
}
```

The caller then proceeds to `mark_resolved` as usual — the fetch *did* resolve, the UPDATE just chose to be a no-op.

### Interaction with the existing `metadata_fetch_dewey` tests (from 5-8)

Story 5-8 added `tests/metadata_fetch_dewey.rs` with three tests that call `update_title_from_metadata` on seeded titles with empty `manually_edited_fields`. Those tests **must continue to pass** — they exercise the "no guard" happy path. AC #1's fix must be field-local; do not short-circuit the whole UPDATE. Verify by running `cargo test --test metadata_fetch_dewey` before and after the fix — all three should still pass.

### Fields list (authoritative, from the UPDATE statement at `metadata_fetch.rs:89-104`)

The 12 auto-filled fields in `update_title_from_metadata`:

| Field | Rust type on `MetadataResult` | Rust type on `TitleModel` | Notes |
|---|---|---|---|
| `title` | `Option<String>` | `String` (NOT NULL) | Top of function guards `title.is_empty() → Ok(())` — don't remove |
| `subtitle` | `Option<String>` | `Option<String>` | |
| `description` | `Option<String>` | `Option<String>` | |
| `publisher` | `Option<String>` | `Option<String>` | |
| `language` | `Option<String>` | `String` (NOT NULL, default `"fr"`) | |
| `page_count` | `Option<i32>` | `Option<i32>` | |
| `publication_date` | Parsed from `Option<String>` to `Option<NaiveDate>` via the `pub_date` local | `Option<NaiveDate>` | Guard applies after parsing |
| `dewey_code` | `Option<String>` | `Option<String>` | |
| `track_count` | `Option<i32>` | `Option<i32>` | |
| `total_duration` | `Option<String>` (iso8601 duration) | `Option<String>` | |
| `age_rating` | `Option<String>` | `Option<String>` | |
| `issue_number` | `Option<String>` | `Option<i32>` | Note type mismatch — `metadata.issue_number` is `Option<String>` per `MetadataResult`, but the SELECT binds it as-is; verify the current bind at `metadata_fetch.rs:117` and match its shape for the guard |

**Confirm the `issue_number` type** by reading `src/metadata/provider.rs::MetadataResult` before coding the guard — if the existing bind compiles, the guard must produce the same `Option<T>`.

### Reuse, not reinvention

- **Optimistic locking helper:** `services::locking::check_update_result` — used by `update_metadata` at `src/models/title.rs:310`. **Do not use** for the background-fetch path (see the `check_update_result` section above). Handle inline.
- **Parse manually_edited_fields:** `TitleModel::parsed_manually_edited_fields()` at `src/models/title.rs:318-323`. Returns `Vec<String>` — collect into `HashSet` for O(1) lookups in the bind loop.
- **Locale-friendly conflict rendering:** Not needed — the background-fetch race is server-side only; the user never sees a rendered conflict for this path. They see either the OOB banner saying the fetch resolved (with their manual edit still present) or the skeleton transitioning to their manual edit if they loaded the page mid-race.
- **Integration-test harness:** Pattern already established in `tests/find_similar.rs` (sqlx::test with `migrations`), `tests/metadata_fetch_dewey.rs` (update_title_from_metadata happy path), `tests/seeded_users.rs` (new from 6-2). Copy-paste the preamble.

### Known app quirks that affect this story

- **MariaDB type gotchas** (from CLAUDE.md): when reading `manually_edited_fields` (JSON column stored as BLOB), use `CAST(col AS CHAR)` — already done in all four SELECTs in `src/models/title.rs` (lines 97, 121, 145, 169). The `find_by_id` path you'll call already uses this cast. No action needed.
- **i18n proc macro:** No new locale keys are added by this story. `touch src/lib.rs && cargo build` is NOT required.
- **Parallel E2E mode:** No new E2E spec is added (the scenarios here are timing-sensitive and better covered by the integration test in Task 4). The existing 133 E2E tests must stay green as a regression guard.

### What a dev-agent must NOT do

- **Do not** change the `confirm_metadata` UI or template. The fix is server-side only.
- **Do not** add CSRF tokens — separate cross-cutting deferred item.
- **Do not** touch `soft_delete` version checks — pre-existing gap, separate deferred item (`deferred-work.md` under 5-3).
- **Do not** refactor the 12 `final_<field>` blocks into a macro or closure. The repetition is intentional and review-friendly.
- **Do not** change the SQL column list in `update_title_from_metadata` — the SET clause already has `COALESCE(?, col)` for every auto-filled column, and that's what makes the "bind NULL to keep existing" trick work.
- **Do not** call `mark_failed` when the race is lost — call `mark_resolved`. The fetch succeeded; it just chose to no-op.

### Project Structure Notes

No new modules. All edits are in existing files:
- `src/routes/titles.rs` (12 small blocks, lines 701–789)
- `src/tasks/metadata_fetch.rs` (one function, lines 71–131)
- `tests/metadata_fetch_race.rs` (new — mirrors `tests/metadata_fetch_dewey.rs`)
- `.github/workflows/_gates.yml` (add one `--test` entry)
- `.sqlx/` (regenerated)

### References

- [Source: CLAUDE.md#Architecture] — optimistic locking pattern, MariaDB type gotchas, i18n rules.
- [Source: _bmad-output/planning-artifacts/epics.md#Story 6.3] — original acceptance criteria from epic planning.
- [Source: _bmad-output/implementation-artifacts/epic-5-retro-2026-04-13.md#Action items for Epic 7 kickoff] — Action 2 land-mine-fix rationale.
- [Source: _bmad-output/implementation-artifacts/deferred-work.md#Deferred from: code review of story 5-8-dewey-code-management] — two bullets covering both defects.
- [Source: src/routes/titles.rs:674-825] — `confirm_metadata` handler with all 12 `final_<field>` blocks.
- [Source: src/tasks/metadata_fetch.rs:71-131] — `update_title_from_metadata` — target of Defect A fix.
- [Source: src/models/title.rs:262-315] — `TitleModel::update_metadata` — reference for the correct optimistic-locked UPDATE shape.
- [Source: src/models/title.rs:317-323] — `parsed_manually_edited_fields()` helper for the guard set.
- [Source: src/services/locking.rs:5-13] — `check_update_result` — why we DON'T use it in the background task.
- [Source: tests/metadata_fetch_dewey.rs] (from story 5-8) — pattern for the new `metadata_fetch_race.rs`.
- [Source: .github/workflows/_gates.yml] — db-integration job to extend with the new test crate.

## Dev Agent Record

### Agent Model Used

claude-opus-4-6 (1M context) via Claude Code

### Debug Log References

- Local gate (2026-04-14):
  - `cargo clippy --all-targets -- -D warnings` → clean
  - `cargo test --lib --bins` → 336 passed (includes 9 new `should_clear_flag_*` tests)
  - `cargo sqlx prepare --check --workspace -- --all-targets` → ok (advisory "potentially unused queries" warning is informational; no schema-typed macros affected)
  - `cargo test --test find_similar --test find_by_location_dewey --test metadata_fetch_dewey --test metadata_fetch_race --test seeded_users` → all green; `metadata_fetch_dewey` 4/4 (regression) + `metadata_fetch_race` 3/3 (new)
  - `cd tests/e2e && npm test` → 134/134 passed in parallel mode (no regression vs. baseline)

### Completion Notes List

- **Defect B (flag wipe)** fixed in `src/routes/titles.rs::confirm_metadata` for all 12 `final_<field>` blocks. Introduced a small pure helper `should_clear_flag(accept, changed)` (file-local, near `non_empty`) reused by every block, and unit-tested via 9 cases covering `publisher` (Option<String>), `dewey_code` (Option<String> + non_empty), and `page_count` (Option<i32>) — same-value/different-value/unchecked.
- **Defect A (background-fetch race)** fixed in `src/tasks/metadata_fetch.rs`. `update_title_from_metadata` now loads a snapshot via `TitleModel::find_by_id`, returns `Ok(())` silently if the title was soft-deleted, and delegates to `do_update`. `do_update` (now `pub` so the integration test can drive a stale snapshot directly) applies the per-field guard via `if guarded.contains("F") { None } else { metadata.F.clone() }`, adds `WHERE version = ?` + `version = version + 1` to the SQL, and returns `rows_affected`. When the version check loses the race the caller logs at `tracing::info!` and returns `Ok(())` so `mark_resolved` (not `mark_failed`) still fires — matching AC #3.
- The contributor side-effect `add_author_contributor` was left unguarded by design (contributors live outside `manually_edited_fields`; `INSERT IGNORE` makes re-runs safe). Documented inline.
- New crate `tests/metadata_fetch_race.rs` with 3 tests: `manually_edited_field_is_not_overwritten` (mixed guard, AC #1+#8), `version_check_blocks_stale_write` (AC #2 — stale snapshot returns 0 rows, manual edit preserved, no extra version bump), and `all_fields_not_edited_still_fill` (regression for the no-guard happy path). Wired into `.github/workflows/_gates.yml` `db-integration` job.
- Visibility note: the story called for `pub(crate) do_update`, but integration tests live outside the crate root, so `pub(crate)` would be invisible to `tests/metadata_fetch_race.rs`. Promoted to `pub` with a doc comment that limits its contract to the metadata-fetch path.
- E2E waiver per AC #9: no new Playwright spec — the race is timing-sensitive and reliably proven by the new DB integration tests; the existing 134 E2E tests stayed green as the regression guard.

### Change Log

- 2026-04-14 — Story 6-3 implementation: server-side `manually_edited_fields` race + flag-wipe fixes, 9 new unit tests, 3 new DB-backed integration tests, CI wiring.

### File List

- `src/routes/titles.rs` — added `should_clear_flag` helper; refactored 12 `final_<field>` blocks; added 9 unit tests.
- `src/tasks/metadata_fetch.rs` — added snapshot load + version guard; extracted `pub async fn do_update` with per-field `manually_edited_fields` guard.
- `tests/metadata_fetch_race.rs` — new integration test crate (3 tests).
- `.github/workflows/_gates.yml` — added `--test metadata_fetch_race` to the `db-integration` job.
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — `6-3-fix-manually-edited-fields-race`: ready-for-dev → in-progress → review.
- `_bmad-output/implementation-artifacts/6-3-fix-manually-edited-fields-race.md` — task checkboxes, status, Dev Agent Record.
