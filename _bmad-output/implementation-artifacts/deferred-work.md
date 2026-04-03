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
