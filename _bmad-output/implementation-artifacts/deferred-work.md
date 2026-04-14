# Deferred Work

## Deferred from: code review of 1-1-project-skeleton-and-foundation (2026-03-29)

- DB pool has no connection limits or timeouts — configure `max_connections`, `acquire_timeout`, `idle_timeout` on the pool
- Health check does not verify database connectivity — add DB ping to `/health` endpoint
- `storage_locations` self-referencing FK allows cycles — add application-level cycle detection
- `loans` table allows multiple active loans per volume — enforce single active loan per volume at application level
- Soft-delete not enforced at FK level — by design, all queries must include `deleted_at IS NULL`
- `pending_metadata_updates.session_token` missing FK to `sessions.token` — add FK or document why it's intentionally absent

## Deferred from: code review of 1-2-scan-field-and-catalog-page (2026-03-29)

- No CSRF protection on POST /catalog/scan — add CSRF token validation when destructive endpoints are added
- Session token not validated for length/charset before DB lookup — add max length check
- OobUpdate target/content not sanitized in HtmxResponse — currently server-controlled, sanitize if user input ever flows into targets
- scan-field.js prefix overlap: ISSN (977) vs UPC starting with 977 — add disambiguation logic when media types are fully implemented
- Ctrl+K keyboard shortcut hijacks browser address bar shortcut — evaluate alternative shortcut or make configurable

## Deferred from: code review of 1-6-search-and-browsing (2026-03-31)

- Pagination renders all page numbers without truncation — with 10,000 titles (400 pages), all 400 buttons rendered. Implement windowed pagination (current ± 3 + first/last)
- Hardcoded French role name 'Auteur' in primary contributor SQL ORDER BY — use role ID or is_primary flag instead of localized name string. Same pattern exists in story 1-5
- context_banner.html href="#" not updated to /title/{id} — requires adding title_id parameter to context_banner_html() function and updating all call sites in catalog.rs

## Deferred from: code review pass 2 of 1-6-search-and-browsing (2026-03-31)

- Volume state filter JOIN excludes titles whose volumes have NULL condition_state_id — document behavior or add `state:unassigned` filter
- parse_filter does not validate state name against actual volume_states — invalid filter silently returns empty results
- page=999999 in URL renders huge pagination (same as pass 1 — implement windowed pagination)
- Missing aria-sort attributes on sortable column headers in home.html
- Hardcoded aria-label strings (Pagination, Remove filter, Breadcrumb) — should use i18n keys

## Deferred from: code review of 1-7-scan-feedback-and-async-metadata (2026-03-31)

- Race condition in contributor creation (SELECT then INSERT not atomic) — single-user NAS, accepted TOCTOU
- Concurrent identical ISBN scans may create duplicate pending_metadata_updates rows — single-user NAS
- XML parsing via naive string search is fragile with malformed/adversarial XML — acceptable for MVP with well-formed BnF responses
- Fire-and-forget spawned tasks with no backpressure or concurrency limit — single-user NAS
- Hardcoded "Auteur" role name in SQL queries for author contributor lookup — pre-existing DB seed pattern
- No body size limit on BnF external API response read — single-user NAS, BnF trusted
- reqwest::Client created per BnF request instead of reused — performance optimization deferred
- Spawned metadata task panic silently swallowed (JoinHandle dropped) — acceptable for MVP
- COALESCE with empty string metadata fields may update DB columns with empty values
- Raw error strings in template rendering AppError::Internal("Template rendering failed") — pre-existing across all routes

## Deferred from: code review of 1-8-cross-cutting-patterns (2026-03-31)

- Conflict error conflates version mismatch with soft-deleted entity — both return same message
- No HTTP endpoint calls update_with_locking yet — title edit form doesn't exist; infrastructure ready
- Session timeout JS timer drifts from server on failed HTMX requests — low impact
- Theme toggle aria-label selector fragile (matches onclick content) — works but brittle
- Soft-delete already-deleted returns 404 not idempotent 200 — acceptable REST semantics
- htmx might not be loaded when keepAlive() fires — fetch() fallback handles it
- resetTimer on every htmx:afterRequest without debounce — cheap operation

## Deferred from: code review of 3-1-provider-chain-and-fallback (2026-04-02)

- timeout_secs=0 causes instant global timeout with no validation or minimum — add settings validation
- Per-provider timeout 5s hardcoded, not configurable or related to global timeout — design simplification acceptable for single-user NAS
- Rate limit detection via string matching on "429" in error message, no generic rate limiter struct — proactive rate limiter planned for story 3-2 (MusicBrainz 1 req/sec)
- Open Library author resolution is sequential within 5s per-provider timeout — consider concurrent resolution (futures::join_all) for multi-author books

## Deferred from: code review of 3-2-media-type-scanning (2026-04-02)

- Rate limiter TOCTOU race — acquire() drops mutex before sleep, allowing concurrent bypass. Acceptable for single-user NAS; fix with token-bucket or semaphore pattern if multi-user support added
- OMDb provider makes 2 sequential HTTP requests (search + detail) within single 5s per-provider timeout — second request could exceed timeout. Acceptable for MVP
- UPC codes stored without checksum validation — no standard UPC-A/UPC-E check digit validation applied before storage

## Deferred from: code review of 3-3-cover-image-management (2026-04-02)

- SSRF: no URL host validation on cover download — cover_url comes from trusted metadata providers, not user input. Add host allowlist if user-provided cover URLs are ever added
- Race condition on concurrent cover file write for same title_id — write to temp file + atomic rename if multi-user support added
- No cache busting for re-downloaded covers — filename stays {title_id}.jpg, browsers may serve stale version. Add version query param or content-hash when re-download is implemented (story 3-5)
- Optimistic locking missing on cover_image_url UPDATE — pre-existing pattern gap, UPDATE without version check

## Deferred from: code review of 3-4-scan-feedback-polish (2026-04-02)

- Web Audio API oscillator/gain nodes not explicitly disconnected after playback — potential memory accumulation in marathon scanning sessions. Add osc.onended callback with disconnect() if needed
- Script loading order: catalog_toolbar.html inline initToggle() script may run before DOM button exists in edge cases — currently works because script is after button in template flow

## Deferred from: code review of 3-5-metadata-editing-and-redownload (2026-04-03)

- SSRF via cover URL in confirm form — cover_url comes from metadata providers (trusted); add host allowlist if user-provided cover URLs ever added
- genre_id=0 from malformed form submission causes DB foreign key constraint error — DB enforces FK correctly, acceptable for single-user NAS
- RwLock .unwrap() panic on poisoned lock — pre-existing pattern across all handlers, not specific to story 3-5
- Stale version in confirmation form (TOCTOU between redownload and confirm) — optimistic locking correctly prevents data loss; single-user NAS makes concurrent edits extremely unlikely

## Deferred from: code review of 4-1-borrower-crud-and-search (2026-04-03)

- LIKE search missing `ESCAPE '\'` clause — pre-existing pattern (contributor_search has same issue); backslash escaping works by default with MariaDB but not guaranteed
- count_active_loans doesn't JOIN volumes table — intentional, loan record exists independently of volume soft-deletion
- Page handlers don't differentiate is_htmx for fragment/full-page — consistent with existing location CRUD pattern; add when HTMX navigation is implemented for these pages
- PRG pattern (redirect after POST) doesn't support inline FeedbackEntry — would require flash sessions or HTMX form submission; consistent with location CRUD pattern

## Deferred from: code review of 5-1-e2e-stabilization (2026-04-06)

- Regression test creates data (borrower, loan) without cleanup — owned by story 5-1b (data isolation architecture)
- Serial mode (`fullyParallel: false`, `workers: 1`) is a workaround masking shared-data failures — owned by story 5-1b (will restore `fullyParallel: true` with per-spec data isolation)
- `logout()` helper in `tests/e2e/helpers/auth.ts` doesn't await navigation completion after `page.goto("/login")` — stub not currently used by any test; fix when logout flow is needed

## Deferred from: code review of 5-1-e2e-stabilization session 3 (2026-04-08)

- `create_loan` handler catches only `BadRequest` for HTMX feedback, not `Conflict`/`Database` — register_loan only returns BadRequest currently; add catch-all if error contract expands
- `create_loan` success path: borrower lookup error propagated after loan already committed — pre-existing pattern, refactored not introduced; user sees error but loan exists
- `waitForTimeout` calls remain in several E2E specs despite documented "never use arbitrary waits" — pragmatic for async metadata resolution; eliminating requires polling mechanism
- Brute-force volume ID search limited to 100 in loans.spec.ts non-loanable test — works for current suite size; increase if test suite grows significantly
- Title ID extraction from skeleton element ID is fragile (metadata-editing.spec.ts) — pre-existing pattern; breaks silently if feedback ID scheme changes
- `INVALID_ISBN` generation may accidentally produce valid ISBN (check digit = 0 case) — unlikely with current specIsbn seeds but not guaranteed
- Accessibility `color-contrast` rule disabled in 3 catalog specs — known UX issue with placeholder text contrast; should be fixed and rules re-enabled
- Location contents/shelving tests use fragile parent traversal (`..` / `..`) selectors — pre-existing pattern; works but brittle to HTML structure changes
- No unit test for `create_loan` handler's new HTMX error path — project pattern: handlers tested via E2E not unit tests

## Deferred from: code review of 5-1b-e2e-data-isolation-architecture (2026-04-08)

- Brute-force volume ID search (1..100) in loans.spec.ts AC3 — pre-existing; breaks if volume IDs exceed 100 in parallel
- Inconsistent loan form submission strategies across specs (HTMX vs stripped HTMX vs button click) — design choice from parallel load workaround
- Hardcoded L-codes without generator function (unlike specIsbn) — documented design choice; manual coordination required
- `waitForTimeout` still present in smoke tests (borrower-loans, loan-returns, epic2-smoke, cross-cutting, catalog-metadata) — pre-existing; should be replaced with deterministic waits
- Unused variable `resultsHtml` in epic2-smoke.spec.ts home search step — pre-existing; variable assigned but never asserted
- `conditionSelect.selectOption({ label: "Endommagé" })` hardcodes French label in loans.spec.ts — pre-existing; use value-based selection for i18n safety

## Deferred from: code review of 5-2-contributor-deletion-guard (2026-04-09)

- TOCTOU race between count_title_associations and soft_delete — no transaction wrapping. Same pattern in location/borrower guards. Low real-world risk for single-user app.
- count_title_associations doesn't JOIN titles to filter soft-deleted titles — contributor blocked from deletion even when all associated titles are in trash.
- HTMX fragment path (contributor_detail_fragment) doesn't include delete button or feedback container — only affects HTMX partial navigation to contributor detail.
- waitForTimeout(1000) anti-pattern in existing duplicate-contributor E2E test (catalog-contributor.spec.ts:106) — should use DOM state wait.
- No E2E coverage for double-delete scenario (two tabs, same contributor) — returns generic error instead of meaningful message.

## Deferred from: code review of 5-3-series-crud-and-listing (2026-04-09)

- Soft delete doesn't check optimistic locking version — pre-existing pattern shared by all entities. Low real-world risk.
- TOCTOU race on series name uniqueness — application-level check only, no DB UNIQUE constraint. Pre-existing MariaDB limitation pattern.
- Delete series allows orphaned title_series assignments — story 5.4 will add assignments + deletion guard.

## Deferred from: cross-story code review (2026-04-09)

- borrowers.spec.ts and location-contents.spec.ts use manual login instead of loginAs() helper — pre-existing, should be migrated.
- 32 waitForTimeout instances across 9 E2E spec files — pre-existing, should be replaced with deterministic waits.
- Non-unique contributor names ("Albert Camus", "Boris Vian", "Test Author") in catalog-contributor.spec.ts existing tests — collision risk in parallel mode.

## Deferred from: code review of 5-4-title-series-assignment-and-gap-detection (2026-04-09)

- Non-existent series_id/title_id not validated in assign handler — FK constraint returns DB error instead of user-friendly 404. UX improvement only.
- Assignments beyond total_volume_count invisible after total reduction — edge case when total is lowered below existing assignments. Low priority.

## Deferred from: code review of 5-7-similar-titles-section (2026-04-10)

- `primary_contributor` subquery hardcodes the French role name `'Auteur'` — pre-existing pattern in `active_search` (src/models/title.rs:506-511). Fix should span both sites in a dedicated story.
- Arm 3 of `find_similar` matches `anchor.genre_id` without excluding a potential "Unknown" sentinel — system-wide issue, not introduced by story 5-7.
- Unknown `media_type` values render a broken icon 404 via `cover.html` — same bug on home page; fix belongs to the cover macro itself.
- E2E spec leaks `ST Shared Author 2026` / `ST Anon Author 2026` contributor rows on repeated local runs (no afterEach cleanup) — shared-DB accretion is a suite-wide pattern.
- E2E `selectOption({ index: 1 })` for contributor role is ordering-sensitive — same pattern used in `catalog-contributor.spec.ts`; fix should be suite-wide.

## Deferred from: code review of 5-1c-epic-4-loan-spec-parallel-flakes (2026-04-11)

- `LoanService::register_loan` retry loop has no terminal "exhausted retries" error log — per-attempt `tracing::warn!` is emitted but the final return on `Err(e)` does not distinguish exhaustion from a non-deadlock error. Observability enhancement only; not urgent since log count reconstructs the history.
- `tests/e2e/helpers/loans.ts:returnLoanFromLoansPage` — `page.once("dialog", ...)` is not awaited nor verified. If the `/loans` template ever drops the `confirm()` prompt, the click proceeds silently and the helper still passes. Implicit contract; no current regression.
- No deterministic E2E test injects a synthetic MariaDB deadlock to exercise `is_deadlock_error` + `register_loan` retry path. Probabilistic coverage from parallel runs is acceptable for now; a dedicated concurrency harness would require a DB-level fault injection layer.
- `createLoan` helper in `tests/e2e/helpers/loans.ts` throws `Failed to create loan …` on any non-2xx response instead of surfacing the feedback body for assertions. No current caller uses it for the negative path (the double-loan spec uses the HTMX form directly), but this is an implicit contract to watch when the helper gets reused.

## Deferred from: code review pass 2 of 5-1c-epic-4-loan-spec-parallel-flakes (2026-04-11)

- `getBorrowerIdByName` helper in `tests/e2e/helpers/loans.ts` fails once active borrower count exceeds `/borrowers` default page size (25). `BorrowerModel::list_active` renders page 1 only. Currently ~12 borrowers per full E2E run and no `afterEach` cleanup, so accumulated data will breach the threshold. Pre-existing limitation — the old `.first().getAttribute("href")` had the same blindspot. Fix likely requires a borrower-search helper or a paginated walk.
- `LoanService::register_loan` retry log trail is misleading when a concurrent soft-delete between attempts flips the second attempt into a `BadRequest` — logs show `warn!("retrying")` immediately followed by a business-rule error, implying the retry caused the error. Observability debt; carry `attempt` / `prior_transient` through the tracing span on final error.
- `LoanService::register_loan` retry loop does NOT retry `sqlx::Error::PoolTimedOut` — by design (retrying on pool exhaustion could worsen contention), but the CLAUDE.md "auto-retries" phrasing is broader than the actual behavior. Defer doc refinement.
- `returnLoanFromBorrowerDetail` helper in `borrower-loans.spec.ts` asserts row disappearance in `#active-loans-section`, which assumes the loan-return HTMX handler swaps that section. Current specs add `page.reload()` after so the risk is masked. Future reusers who skip the reload will hit a false flake if the handler's OOB targets diverge. Verify return handler targets.

## Deferred from: code review of story 5-8-dewey-code-management (2026-04-12)

- `confirm_metadata` in `src/routes/titles.rs:784-789` (and all 7 other `final_<field>` blocks) removes the `manually_edited_fields` flag whenever the accept checkbox is checked, even if the form value is empty. Net effect: a user who re-accepts an existing manual override loses the "manually edited" marker, and the next re-download silently auto-overwrites. Pre-existing pattern across 8 fields. Fix requires suite-wide change.
- `tasks/metadata_fetch.rs:update_title_from_metadata` performs a raw UPDATE with no `version` optimistic-lock check and no `manually_edited_fields` guard. If a user edits a field manually while the background metadata fetch is still resolving, the fetch can silently overwrite the manual edit. Affects all auto-filled fields (publisher, subtitle, Dewey, etc.), not just Dewey. Cross-cutting retro item.
- Pre-existing 422 bug in `update_title` handler: empty numeric form fields (e.g. `page_count=""`) fail to parse as `Option<i32>` via serde, returning 422 without a friendly validation message. E2E specs (`metadata-editing.spec.ts:48-53`, now `dewey-code.spec.ts:36-40`) paper over it by filling "0" before submit. Fix: custom serde deserializer that treats empty string as `None`, or handle at route level.
- `VolumeModel::find_by_location` (and parallel sorts in `home.rs` / `active_search`) lacks a secondary `id ASC` tiebreaker in `ORDER BY`. MariaDB's order within equal sort keys is implementation-defined, so rows with identical values can reorder across paginated requests, causing volumes to be skipped or duplicated. Amplified on Dewey sort (NULLs are common). Cross-cutting retro item.
- `rust_i18n::t!(...)` output is not HTML-escaped when inserted into `format!`-based response strings (e.g. `src/routes/titles.rs:288`). Low risk (translator-controlled), inconsistent with `html_escape(d)` applied to user-entered values. Pattern appears across many `format!` sites. Cross-cutting cleanup.
- `BnfProvider::extract_subfield("676", "a")` returns only the first 676 datafield when a record has multiple DDC classifications (fiction + translation studies, etc.). Deterministic but arbitrary choice. Same pattern used for all other UNIMARC fields; not a regression.

## Deferred from: code review of story-6-1 (2026-04-14)

- Cargo.toml version parsing in `release.yml:30` (`grep '^version = "' | head -1`) is fragile against `[workspace.package]` tables, UTF-8 BOM, CRLF line endings, and single-quoted values. Today Cargo.toml has none of these. Future fix: migrate to `cargo pkgid` or `cargo metadata --format-version 1 --no-deps | jq -r '.packages[0].version'`.
- `_gates.yml` callers do not pass `secrets: inherit`. No current gate needs secrets, but adding one later will silently produce empty env vars. Documented in `docs/ci-cd.md#known-gotchas`; address when a gate first needs a secret.
- Task 3.5 tag-mismatch smoke test (push a tag whose semver differs from Cargo.toml, observe workflow fails fast) was deferred in the story itself; tracked here for traceability.

## Deferred from: code review of 6-2-seed-librarian-and-loginas-role (2026-04-14)

- Seed migration `INSERT ... WHERE NOT EXISTS` pattern ignores hash/role drift. If a prior env has a `librarian` row with stale hash or role, the new migration is a no-op and login silently uses stale creds. Shared with `seed_dev_user.sql` — worth a project-wide revisit.
- `tests/e2e/tsconfig.json` does not enable `noUncheckedIndexedAccess` or `exactOptionalPropertyTypes`. `strict: true` is on but these stricter flags would harden the new typecheck gate.

## Deferred from: code review of 6-3-fix-manually-edited-fields-race (2026-04-14)

- `non_empty` trim asymmetry: `confirm_metadata` trims form input via `non_empty()` but compares the result against untrimmed `title.<field>`. Legacy rows with trailing whitespace will compute `changed = true` on re-accept, clearing the manually-edited flag. Low risk (no known whitespace legacy data); revisit if any such rows appear.
- `publication_date` form handling parses `"2024"` as `2024-01-01`. If stored value is a full date and metadata returns year-only (or vice-versa), the same-value comparison reports `changed` and clears the flag even though the user sees "same year". Pre-existing parse behavior; fix requires a normalization helper shared with the parser.
- Numeric form fields (`page_count`, `track_count`, `total_duration`, `issue_number`) use `form.new_X.parse().ok().or(title.X)`, which masks an empty submit back to the stored value. Users cannot semantically "clear to NULL" via the confirm-metadata form. Pre-existing; tracked for a dedicated form-UX pass.
- Background-fetch `version` bump can cause 409 Conflict for users who opened the edit form before the BnF fetch landed. Decision 2026-04-14: accept as correct optimistic-locking semantics. If real-world reports appear, revisit — options: (a) friendly 409 UX + merge hint, (b) drop the version bump and rely purely on `manually_edited_fields` guard, (c) client-side retry/merge flow.
