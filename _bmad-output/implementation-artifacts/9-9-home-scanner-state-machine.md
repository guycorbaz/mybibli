# Story 9.9: Home page scanner detection state machine

Status: in-progress

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a librarian on the home page,
I want the search field to distinguish between human typing (search) and a barcode-scanner burst (scan with intent to navigate),
so that I can scan from the home page and land on the right page (`/title/:id`, `/volume/:id`, `/location/:id`, or `/catalog?code=…`) without manually navigating to `/catalog` first.

## ⚠️ Major spec drift discovered up front

The 4-state machine described in the original epics.md spec text is **already shipped** in `static/js/search.js` (172 LOC) — IDLE → DETECTING → SEARCH_MODE → SCAN_PENDING transitions, inter-key timing thresholds (`scannerThreshold` + `debounceDelay` from `data-*` attributes on `#search-field`), Enter handling, Escape reset, `search-fire` custom event dispatch, debounce-reset-on-keystroke. The `tests/e2e/helpers/scanner.ts` already provides `simulateScan` (20ms inter-key) and `simulateTyping` (100ms inter-key) helpers. The `home-search.spec.ts` E2E already covers typing → search → results.

**What's NOT yet shipped** (the actual scope of this story):
1. **Server-side `GET /scan?code=…` endpoint** that does prefix-detection + DB lookup + `HX-Redirect` to the right destination. Today, when the JS state machine reaches SCAN_PENDING and fires `search-fire`, it just runs the regular search path (`hx-get="/"` from `home.html:24`) — there's no scan-prefix detection or routing.
2. **Wiring `search.js` SCAN_PENDING path to the new endpoint** — likely via a hidden HTMX-aware element or a different custom event (`scan-fire` vs `search-fire`).
3. **aria-live polite announcements** for state transitions ("Searching…" / "Scanning…").
4. **i18n keys** for the announcement copy.
5. **E2E tests** specifically exercising the SCAN_PENDING → server scan-handler → redirect flow (the existing E2E covers only the typing/search path).
6. **Server-side prefix-detection lookup tests** (DB-backed `#[sqlx::test]`).

The existing `detect_code_type(code)` helper at `src/routes/catalog.rs:366-410` is REUSED — it already classifies V-code / L-code / ISBN / ISSN / UPC. This story only adds the DB-LOOKUP step (matching the classified code against the relevant table) + the redirect routing, on top of the existing classification.

The original epics.md spec text mentioning "JS module: new home-scanner.js" is OUTDATED — the JS is already in `search.js`. AC11 (E2E) text mentioning `simulateScan from tests/e2e/helpers/scanner.ts` is correct (helper exists).

## Acceptance Criteria

1. **AC1 — NEW server endpoint `GET /scan?code=…`** that does the full prefix-detection + DB lookup + redirect chain. This is the load-bearing addition this story makes. Implementation:
   - Route registration in `src/routes/mod.rs` (verify the existing `/catalog/scan` POST stays untouched — it serves the cataloging workflow; the new endpoint is a sibling for the home-page navigation case).
   - Handler at `src/routes/catalog.rs::handle_home_scan` (or a sibling module if `catalog.rs` exceeds the project LOC norms — see AC14). Accepts `?code=<string>` query param.
   - Anonymous + Librarian + Admin all allowed (FR65 — search/scan-to-navigate is part of the public catalog browsing surface). The DESTINATION may be Librarian-gated (e.g., `/catalog?code=…` is Librarian-only — handled by the destination route's own gate; the scan endpoint itself is role-blind).
   - Returns one of:
     - **`HX-Redirect: /volume/:id` (200 OK)** when the code is a V-code AND matches `volumes.label` for an active (`deleted_at IS NULL`) volume.
     - **`HX-Redirect: /title/:id` (200 OK)** when the code is an ISBN AND matches `titles.isbn` for an active title.
     - **`HX-Redirect: /location/:id` (200 OK)** when the code is an L-code AND matches `storage_locations.label` for an active location.
     - **`HX-Redirect: /catalog?code=<URL-encoded>` (200 OK)** otherwise (unknown code, or known prefix but no DB match — the cataloging workflow on `/catalog` takes over). The `code` query param is URL-encoded to handle special characters.
   - Non-HTMX request (direct browser navigation to `/scan?code=…`): same routing logic, but returns `303 See Other` with a `Location:` header instead of `HX-Redirect`. Defense for direct URL-bar usage; not the primary entry point.
   - Soft-degrade on DB failure: returns `HX-Redirect: /catalog?code=…` with a `tracing::warn!` log. Never 500s — the home page must always recover.

2. **AC2 — Reuses `detect_code_type` from `catalog.rs:366-410`.** The classification logic is NOT duplicated. The new handler imports + calls `detect_code_type(code)`, then branches on the `code_type` field to decide which DB lookup to run. ISSN + UPC codes fall through to the `/catalog?code=…` arm in v1 (their cataloging flow uses the `/catalog/scan-with-type` POST handler, not the home navigation surface — keeps the home-scan flow narrowly scoped to the 3 navigation cases ISBN/V-code/L-code).

3. **AC3 — DB lookup queries are NEW narrow projections** (NOT existing model methods that fetch full structs). Three NEW model methods, one each on `TitleModel`, `VolumeModel`, `LocationModel`:
   - `TitleModel::find_id_by_isbn(pool: &DbPool, isbn: &str) -> Result<Option<u64>, AppError>` — SQL: `SELECT id FROM titles WHERE isbn = ? AND deleted_at IS NULL LIMIT 1`. Returns just the id (no full title load) since the handler only needs to redirect.
   - `VolumeModel::find_id_by_label(pool: &DbPool, label: &str) -> Result<Option<u64>, AppError>` — SQL: `SELECT id FROM volumes WHERE label = ? AND deleted_at IS NULL LIMIT 1`.
   - `LocationModel::find_id_by_label(pool: &DbPool, label: &str) -> Result<Option<u64>, AppError>` — SQL: `SELECT id FROM storage_locations WHERE label = ? AND deleted_at IS NULL LIMIT 1`.
   All 3 use **dynamic `query_scalar`** (NOT the macro `sqlx::query!`) per project convention. Return narrow `u64` results — no extra columns projected.
   - Existing call patterns to check before adding: `TitleModel::find_by_isbn` may already exist for the cataloging workflow (it returns the full `TitleModel` struct, used by `TitleService::create_from_isbn`); the new `find_id_by_isbn` is a narrow sibling. If a refactor opportunity emerges to consolidate them, file as a `type:code-review-finding` and DO NOT do the consolidation in this story (refactor-during-feature anti-pattern). Verify before adding `find_id_by_label` on `VolumeModel` and `LocationModel` that no equivalent narrow lookups exist already.

4. **AC4 — Wire `static/js/search.js` SCAN_PENDING to fire `/scan?code=…` instead of the search path.** Today (line 49, 82-86 of `search.js`) `SCAN_PENDING` calls `fireSearch(field)` which dispatches `search-fire` event → triggers the `hx-get="/"` from `home.html:24` → returns search results inline. **Change**: in the SCAN_PENDING transition (line 49 — Enter pressed during DETECTING with fast inter-key, OR a dedicated detection of the burst-end-with-Enter state), dispatch a NEW custom event `scan-fire` instead of `search-fire`. The home-page search field gains a SECOND HTMX wiring listening for `scan-fire` that targets the new `/scan?code=…` endpoint with `hx-get` and lets HTMX follow the `HX-Redirect` response automatically.
   - **`search.js` change shape:**
     ```js
     if (state === DETECTING && interKey < scannerThreshold) {
         // Fast burst + Enter = scanner scan
         fieldContentAtScan = field.value;
         state = SCAN_PENDING;
         fireScan(field);  // NEW — fires custom "scan-fire" event
     }
     ```
     ```js
     function fireScan(field) {
         field.dispatchEvent(new Event("scan-fire", { bubbles: true }));
     }
     ```
   - **`home.html` template change shape**: the `<input id="search-field">` already has one HTMX wiring (`hx-trigger="search-fire"`). HTMX supports comma-separated triggers: `hx-trigger="search-fire, scan-fire from:#search-field"`. **Cleaner alternative**: a sibling hidden element (`<button hx-get="/scan?..." hx-trigger="scan-fire from:#search-field" hx-vals='{"code": ...}' style="display:none">`) — but inline `style="display:none"` is CSP-clean since it's a static attribute (not runtime style mutation). Even cleaner: use Tailwind `class="hidden"` instead of inline style. **Pick the cleanest shape during implementation** — Task 4 details the trade-offs.
   - The `code` query param value comes from the search field's current value at SCAN_PENDING time (`fieldContentAtScan`). Use `hx-vals='{"code": "..."}'` syntax with a dynamic value via `hx-vals='js:{code: document.getElementById("search-field").value}'` — but `hx-vals='js:...'` may run afoul of strict CSP (requires inline script eval). **Defer the JS-eval question to Task 4** with a fallback plan: if `hx-vals` JS-eval is blocked, use `hx-get="/scan"` + `hx-include="#search-field"` (HTMX will include the search field's `name="q"` value in the request as `?q=…` — handler reads either `?code=` or `?q=` for graceful compat).

5. **AC5 — `aria-live="polite"` announcements for state transitions.** A NEW `<span aria-live="polite" id="search-state-announcement" class="sr-only"></span>` lives next to the search field (inside the `role="search"` container). The JS state machine writes user-facing copy on transitions:
   - DETECTING (no announcement — too quick to be useful).
   - SEARCH_MODE entered → announce "Searching…" / "Recherche…".
   - SCAN_PENDING entered → announce "Scan detected…" / "Scan détecté…".
   - IDLE (after Escape, blur, or HTMX response landing) → clear the announcement (set text content to empty string).
   - The announcement copy reads from `data-*` attributes on the search field (e.g., `data-search-announcement-searching` and `data-search-announcement-scanning`), pre-translated by the handler. Mirrors the `data-connection-lost` pattern already in `home.html:33`.
   - Screen reader testing matrix: announcements MUST be polite (NOT assertive — would interrupt navigation), MUST clear on IDLE transition (so a reset doesn't queue stale announcements), MUST NOT fire on every keystroke (only on state transitions).

6. **AC6 — Anonymous can use the scan endpoint** (FR65 — series/catalog browsing is anonymous-permitted; navigation-by-scan is part of that browsing surface). The endpoint itself is role-blind. The DESTINATION may be role-gated (e.g., `/catalog` is Librarian-only — Anonymous gets a 303 → `/login?next=/catalog?code=…`). The home-scan flow does NOT make any role-based decisions in the redirect target — the destination route's own gate handles that. Asserted by `home_scan_anonymous_isbn_redirects_to_title_detail` integration test (Anonymous user scanning a known ISBN gets redirected to `/title/:id` — `/title/:id` is anonymous-allowed per FR95).

7. **AC7 — i18n EN + FR (3 new keys).** Append to `locales/{en,fr}.yml` under a NEW `home_scan:` block (or extend the existing `home:` block — pick the cleaner placement based on existing convention):
   - `searching_announcement` — EN: `"Searching…"`, FR: `"Recherche…"`
   - `scanning_announcement` — EN: `"Scan detected, processing…"`, FR: `"Scan détecté, traitement…"`
   - `scan_failed_fallback` — EN: `"Scan failed. Try again or use the catalog page."`, FR: `"Le scan a échoué. Essayez à nouveau ou utilisez la page catalogue."` (used in the JS error path when `/scan` returns 4xx/5xx — soft-degrade announcement before the fallback redirect to `/catalog?code=…`).
   - **CRITICAL:** locale files have NO top-level `en:`/`fr:` wrapper — keys start at root. After editing, run `touch src/lib.rs && cargo build`. The `i18n::audit::tests::all_t_keys_have_both_locales` test enforces EN/FR mirror.

8. **AC8 — Coexistence with existing focus.js + scanner-guard.js.** The 3 JS modules MUST coexist without cycle:
   - `static/js/focus.js` (54 LOC) — UX-DR25 focus dual mechanism. Maintains focus on the scan field after HTMX swaps. Independent of scanner classification.
   - `static/js/scanner-guard.js` (177 LOC) — story 7-5 modal scanner-guard. Captures keystrokes at the document-capture phase WHILE a modal is open, blocking them from leaking to the background scan field. Independent of the home-scanner state machine (the modal-guard is on `dialog[open]`, not `#search-field`).
   - `static/js/search.js` (172 LOC, this story extends) — the 4-state machine for the home search field. Listens on `#search-field`'s `keydown`, dispatches `search-fire` / NEW `scan-fire` events.
   The 3 modules listen on different elements (focus.js: scan field on /catalog; scanner-guard.js: document while modal open; search.js: home search field). No overlap. This story does NOT modify focus.js or scanner-guard.js. Verified by: searching for any code that uses `#search-field` in focus.js or scanner-guard.js (expected: zero matches).

9. **AC9 — `prefers-reduced-motion` and screen-reader users still work.** The state machine is purely keystroke-timing-based — no animation, no transition CSS. The aria-live announcements are the ONLY a11y-visible UI for the state machine. A user with `prefers-reduced-motion` set OR a screen-reader-only user gets the same functionality (scan still works, announcements still fire). Verified by: no `@media (prefers-reduced-motion: reduce)` references in `search.js` or its CSS surface (none expected — no animation in this story).

10. **AC10 — CSP compliance.** The state-machine logic stays in `search.js` (already external module — CSP-clean). Any new HTMX wiring in `home.html` uses Tailwind classes + `hx-*` attributes only — NO inline `style="..."`, `<script>`, `<style>`, `onclick=`, etc. The hidden helper element (if used per AC4) uses `class="hidden"` (Tailwind utility), not inline `style="display:none"`. The `src/templates_audit.rs::no_inline_markup_in_templates` test (line 44) MUST stay green.

11. **AC11 — Server-side tests (DB-backed `#[sqlx::test]`) + handler render tests.**
    - **(a) NEW `tests/home_scan_redirect.rs`** with `#[sqlx::test]` cases:
      - `home_scan_isbn_known_redirects_to_title_detail` — seed: 1 title with isbn `"9782070360246"`. Hit `GET /scan?code=9782070360246` (HTMX request). Assert response status 200 + `HX-Redirect: /title/<id>` header.
      - `home_scan_isbn_unknown_redirects_to_catalog_with_code` — seed: empty DB. Hit `GET /scan?code=9999999999999`. Assert `HX-Redirect: /catalog?code=9999999999999`.
      - `home_scan_vcode_known_redirects_to_volume_detail` — seed: 1 title + 1 volume "V0042". Hit `GET /scan?code=V0042`. Assert `HX-Redirect: /volume/<id>`.
      - `home_scan_vcode_unknown_redirects_to_catalog_with_code` — seed: empty DB. Hit `GET /scan?code=V9999`. Assert `HX-Redirect: /catalog?code=V9999`.
      - `home_scan_lcode_known_redirects_to_location_detail` — seed: 1 storage_location "L0042". Hit `GET /scan?code=L0042`. Assert `HX-Redirect: /location/<id>`.
      - `home_scan_lcode_unknown_redirects_to_catalog_with_code` — seed: empty DB. Hit `GET /scan?code=L9999`. Assert `HX-Redirect: /catalog?code=L9999`.
      - `home_scan_unknown_prefix_redirects_to_catalog_with_code` — Hit `GET /scan?code=garbage`. Assert `HX-Redirect: /catalog?code=garbage`.
      - `home_scan_excludes_soft_deleted_title` — seed: 1 title with isbn `"9782070360246"`, soft-delete it. Hit `GET /scan?code=9782070360246`. Assert `HX-Redirect: /catalog?code=9782070360246` (soft-deleted titles MUST NOT match — locks the safety invariant).
      - `home_scan_excludes_soft_deleted_volume` — same shape for V-code.
      - `home_scan_excludes_soft_deleted_location` — same shape for L-code.
      - `home_scan_anonymous_isbn_redirects_to_title_detail` — Anonymous request (no session cookie). Assert behaves identically to Librarian (FR65 + AC6). Locks the role-blind contract.
      - `home_scan_non_htmx_returns_303_with_location_header` — Non-HTMX request (no `HX-Request` header). Assert 303 status + `Location: /title/<id>` header.
      - `home_scan_url_encodes_special_chars_in_catalog_fallback` — Hit `GET /scan?code=foo bar%26baz`. Assert `HX-Redirect: /catalog?code=foo%20bar%26baz` (proper URL encoding of the fallback target).
      - `home_scan_empty_code_returns_400_or_redirects_to_home` — Hit `GET /scan?code=` (empty). Assert: pick one — return 400 with i18n error, OR silently redirect to `/`. Decision in Task 1; document in spec at story close.
    - **(b) `find_id_by_*` model-method unit tests** — co-located with the existing model tests:
      - `TitleModel::find_id_by_isbn` returns `Some(id)` for active match; `None` for soft-deleted; `None` for non-existent.
      - `VolumeModel::find_id_by_label` — same matrix.
      - `LocationModel::find_id_by_label` — same matrix.
    - **(c) `detect_code_type` regression tests** — these already exist for the cataloging workflow (likely in `catalog.rs::tests`). NO new tests needed for the classifier; we reuse it as-is.

12. **AC12 — E2E (Foundation Rule #7).** Append a NEW `test.describe("Home page scanner detection — scan to navigate", ...)` block to `tests/e2e/specs/journeys/home-search.spec.ts` (already exists; logical home for the new tests):
    - **Test 1 — scanner burst of unknown ISBN → redirect to `/catalog?code=…`.** Anonymous context. Navigate to `/`. `await simulateScan(page, "#search-field", "9999999999999")`. Wait for URL change. Assert URL ends with `/catalog?code=9999999999999` OR redirects to `/login?next=...` if `/catalog` is gated for Anonymous (which it is — verify; if so, the scan-handler redirect lands on /login — that's the documented behavior since /catalog is Librarian-only).
    - **Test 2 — scanner burst of known V-code → redirect to `/volume/:id`.** Librarian context (`loginAs(page)`). Seed: create a volume "V0099" via existing helpers. `await simulateScan(page, "#search-field", "V0099")`. Wait for URL change. Assert URL matches `/^/volume/\d+$/`. Per-invocation V-code suffix to dodge retry collisions (story 9-8 catch).
    - **Test 3 — human typing → SEARCH_MODE results inline (no redirect).** Anonymous. `await simulateTyping(page, "#search-field", "tintin")`. Wait for `#browse-results` to update. Assert URL DID NOT change (still `/`) and `#browse-results` contains the search results.
    - **Test 4 — clear input → state machine resets.** After Test 3 setup, press Escape. Assert search field is empty, `#browse-results` reverts to home-default rendering.
    - All 4 tests use `simulateScan` / `simulateTyping` from `tests/e2e/helpers/scanner.ts` — DO NOT introduce manual `keyboard.type` + `waitForTimeout` sequences (CI flake gate forbids it).

13. **AC13 — Server-side scan endpoint runs anonymous-allowed.** The handler does NOT call `session.require_role(Role::Librarian)` — that gate is what makes the existing POST `/catalog/scan` Librarian-only. For the new GET `/scan` endpoint, there's NO role guard. Locked by `home_scan_anonymous_isbn_redirects_to_title_detail` test (AC11a).

14. **AC14 — Foundation Rule #12 LOC.** `src/routes/catalog.rs` is at **~2675 LOC** post-9-8 (per the 9-8 dev log) — already over the 2000 ceiling for that file (it's a known cross-cutting concern; CLAUDE.md doesn't enforce 2000 on `catalog.rs` specifically as far as I can see, but worth flagging). Adding the new handler + 3 new model methods + their tests adds ~200 LOC across 3 files (~80 LOC in `catalog.rs`, ~20 LOC each in the 3 model files, ~250 LOC in the new test file). **Mitigation:** if `catalog.rs` growth is a concern, the new `home_scan` handler can land in a NEW sibling module `src/routes/home_scan.rs` to avoid further bloat. Decide in Task 1 based on the actual file size at task start. Document the placement decision in the Dev Agent Record.

15. **AC15 — search.js JS unit tests deferred.** The project has no JS test runner configured (no Jest, no Vitest, no Mocha — verified via `grep -r 'jest\|vitest\|mocha' package.json` returning nothing). Per AC10 of the original spec text, "Unit tests (JS via the existing testing harness): timer thresholds; state transitions for each input pattern" — this requirement is OUT OF SCOPE for this story given the harness doesn't exist. The state-machine behavior is locked by the E2E tests in AC12. File as `type:code-review-finding` GH Issue at story close: "Set up JS unit test harness (Vitest? Bun's built-in test? jsdom?) and add unit tests for `static/js/search.js` state-machine transitions". Reference the cumulative test debt across `static/js/*.js` (focus.js, scanner-guard.js, csrf.js, etc. all currently lack JS unit coverage).

## Tasks / Subtasks

- [x] **Task 1 — Three NEW narrow `find_id_by_*` model methods + co-located unit tests (AC: 3, 11b)**
  - [ ] In `src/models/title.rs`, add `pub async fn find_id_by_isbn(pool: &DbPool, isbn: &str) -> Result<Option<u64>, AppError>` near `count_active` / `find_by_isbn` (verify what exists first via grep). Use `sqlx::query_scalar::<_, u64>("SELECT id FROM titles WHERE isbn = ? AND deleted_at IS NULL LIMIT 1").bind(isbn).fetch_optional(pool).await?`.
  - [ ] In `src/models/volume.rs`, add `pub async fn find_id_by_label(pool: &DbPool, label: &str) -> Result<Option<u64>, AppError>`. Same pattern. Verify via grep first that no `find_by_label` exists already; if a wider variant exists, the new narrow `find_id_by_label` is a focused sibling (do NOT consolidate — refactor-during-feature).
  - [ ] In `src/models/location.rs`, add `pub async fn find_id_by_label(pool: &DbPool, label: &str) -> Result<Option<u64>, AppError>`. Same pattern. Same verification step.
  - [ ] All 3 use **dynamic `query_scalar`** (NOT the macro `sqlx::query!`) per project convention.
  - [ ] Add unit tests at the bottom of each model file (per project convention — `#[cfg(test)] mod tests { ... }`). Each model gets 3 cases: returns `Some(id)` for active match; `None` for soft-deleted; `None` for non-existent. Total: 9 unit tests across the 3 model files.
  - [ ] Verify: `SQLX_OFFLINE=true cargo test models::title::tests::find_id_by_isbn_ models::volume::tests::find_id_by_label_ models::location::tests::find_id_by_label_` — all 9 green; lock as Commit 1.

- [x] **Task 2 — NEW `handle_home_scan` handler + route registration + integration tests (AC: 1, 2, 3, 6, 11a, 13)**
  - [ ] Decide handler placement: extend `src/routes/catalog.rs` (already 2675 LOC) OR create NEW `src/routes/home_scan.rs` (cleaner separation; new module). **Recommendation:** new module — `catalog.rs` is already large; the home-scan endpoint is logically separate from the cataloging workflow even though they share the `detect_code_type` helper. Document the decision in the Dev Agent Record at story close.
  - [ ] Add the handler signature:
    ```rust
    pub async fn handle_home_scan(
        session: Session,
        Extension(locale): Extension<Locale>,
        HxRequest(is_htmx): HxRequest,
        State(state): State<AppState>,
        axum::extract::Query(params): axum::extract::Query<ScanQuery>,
    ) -> Result<impl IntoResponse, AppError>
    ```
    where `ScanQuery { code: String }` is a NEW deserialization struct. Trim the `code`; if empty, decide per AC11a (return 400 with i18n message OR redirect to `/`) — pick one and document.
  - [ ] Implementation flow:
    1. Call `crate::routes::catalog::detect_code_type(&code)` — REUSE the existing classifier.
    2. Branch on `detection.code_type`:
       - `"isbn"` → `TitleModel::find_id_by_isbn(pool, &code).await?` → if `Some(id)` redirect to `/title/{id}`; else fall through to fallback.
       - `"vcode"` → `VolumeModel::find_id_by_label(pool, &code).await?` → if `Some(id)` redirect to `/volume/{id}`; else fall through.
       - `"lcode"` → `LocationModel::find_id_by_label(pool, &code).await?` → if `Some(id)` redirect to `/location/{id}`; else fall through.
       - `"issn"` / `"upc"` / `"unknown"` → fall through to fallback.
    3. Fallback: redirect to `/catalog?code=<URL-encoded>`. Use `urlencoding::encode` (verify the dependency is in `Cargo.toml`; if not, use `percent_encoding` or `urlencoding` crate — pick whichever the project already uses for URL encoding elsewhere).
  - [ ] Redirect mechanism:
    - HTMX request (`HxRequest(true)`): return `(StatusCode::OK, [(HeaderName::from_static("hx-redirect"), url)])` — HTMX follows the header.
    - Non-HTMX request (`HxRequest(false)`): return `(StatusCode::SEE_OTHER, [(header::LOCATION, url)])` — browser follows.
  - [ ] Soft-degrade: if any of the 3 model methods errors, `tracing::warn!(code = %code, error = %e, "find_id_by_* failed; falling back to /catalog?code=...")` + use the fallback redirect. NEVER 500.
  - [ ] No role gate (`session.require_role(Role::Librarian)` is NOT called — see AC6/AC13).
  - [ ] Register the route in `src/routes/mod.rs`: `.route("/scan", axum::routing::get(home_scan::handle_home_scan))` (or `catalog::handle_home_scan` if kept in `catalog.rs`).
  - [ ] Build the integration test file `tests/home_scan_redirect.rs` (NEW, sibling of `tests/dashboard_recent_activity.rs`):
    - Helpers: copy `first_genre_id`, `first_volume_state_id`, `insert_title`, `insert_volume`, plus a NEW `insert_storage_location(pool, label) -> u64` (UPSERT with `INSERT INTO storage_locations (label, name, ...) VALUES (?, ?, ...)`; verify the schema's required NOT NULL columns first).
    - Use Axum's `axum_test` or `tower::ServiceExt::oneshot` to exercise the handler — verify the project's existing pattern by checking how `setup_wizard.rs` or `csrf_integration.rs` integration tests build their test app. Mirror it.
    - 13 `#[sqlx::test(migrations = "./migrations")]` cases per AC11a.

- [ ] **Task 3 — `search.js` SCAN_PENDING dispatches `scan-fire` event (AC: 4)**
  - [ ] Add `function fireScan(field) { field.dispatchEvent(new Event("scan-fire", { bubbles: true })); }` near `fireSearch` (line 139).
  - [ ] In the SCAN_PENDING transition (line 47-49), replace `fireSearch(field)` with `fireScan(field)`.
  - [ ] In the SCAN_PENDING `htmx:afterSwap` handler (lines 100-110), add a check: if the response was an HX-Redirect (browser navigated), reset state to IDLE and clear the field. The current handler tries to detect "scan vs search" by comparing `field.value === fieldContentAtScan`; this still works but the SCAN_PENDING branch should also handle the case where HTMX navigated away (the page is unloading; no further action needed in JS).
  - [ ] CSP compliance: NO inline script changes; the new `fireScan` function lives in `search.js` (already external module).

- [ ] **Task 4 — Wire the `scan-fire` event to `/scan` endpoint in `home.html` (AC: 4, 10)**
  - [ ] Decide between two patterns (document the decision in Dev Agent Record):
    - **Pattern A (recommended):** add a SECOND HTMX trigger to the existing `<input id="search-field">`:
      ```html
      hx-trigger="search-fire, scan-fire from:#search-field"
      hx-get="/?{{ ... }}"  <!-- existing search target — but this would conflict with /scan -->
      ```
      Conflict: the existing `hx-get="/"` can't change based on event. Pattern A doesn't work cleanly with one element.
    - **Pattern B (cleaner):** add a SIBLING hidden element with its own HTMX wiring:
      ```html
      <a hx-get="/scan" hx-trigger="scan-fire from:#search-field" hx-include="#search-field" class="hidden" aria-hidden="true"></a>
      ```
      The `hx-include="#search-field"` includes the field's `name="q"` value in the request as `?q=…`. **The handler needs to accept BOTH `?code=…` AND `?q=…` as the input parameter** — handler reads whichever is present (defensive). Document in AC1.
    - **Pattern C (URL-rewrite via JS):** the JS does `window.location = "/scan?code=" + encodeURIComponent(field.value)` directly. Bypasses HTMX entirely; simpler but loses HTMX's `htmx:beforeRequest` etc. lifecycle.
  - [ ] **Recommendation:** Pattern B with `hx-include="#search-field"` — the handler accepts both `?code=` and `?q=` (whichever is present takes precedence; if both, prefer `?code=`).
  - [ ] CSP-clean: `class="hidden"` (Tailwind) NOT inline `style="display:none"`.

- [ ] **Task 5 — aria-live polite announcements (AC: 5, 7)**
  - [ ] Add `<span aria-live="polite" id="search-state-announcement" class="sr-only"></span>` to `home.html` inside the `role="search"` container (next to the search input).
  - [ ] Add 3 `data-*` attributes to the `<input id="search-field">`: `data-announce-searching="{{ searching_announcement }}"`, `data-announce-scanning="{{ scanning_announcement }}"`. Pre-translate in the home handler.
  - [ ] In `search.js`, add a helper `announce(field, key)` that reads the `data-announce-{key}` attribute and writes to `#search-state-announcement`'s `textContent`. Wire to state transitions:
    - `SEARCH_MODE` entered → `announce(field, "searching")`.
    - `SCAN_PENDING` entered → `announce(field, "scanning")`.
    - `IDLE` entered (Escape, blur, HTMX response) → set `#search-state-announcement` text to empty string.
  - [ ] In `home.rs::home` handler, add 2 new `HomeTemplate` fields: `searching_announcement: String` and `scanning_announcement: String`. Pre-translate via `rust_i18n::t!("home_scan.searching_announcement", locale = loc)` etc.
  - [ ] Add the i18n keys to `locales/en.yml` + `locales/fr.yml` per AC7.

- [ ] **Task 6 — E2E spec block (AC: 12)**
  - [ ] Append `test.describe("Home page scanner detection — scan to navigate", ...)` to `tests/e2e/specs/journeys/home-search.spec.ts`.
  - [ ] 4 tests per AC12. Use `simulateScan` / `simulateTyping` helpers exclusively (CI flake gate).
  - [ ] V-code uniqueness per invocation: derive from `Date.now()` last-4-digits, e.g. `V${("0000" + (Date.now() % 10000)).slice(-4)}` (story 9-8 catch — locks against retry collisions).

- [ ] **Task 7 — Verify and document (AC: 1–15)**
  - [ ] `wc -l src/routes/catalog.rs` (or `src/routes/home_scan.rs` per Task 2 placement decision) — verify no surprise growth.
  - [ ] `SQLX_OFFLINE=true cargo check && cargo clippy --all-targets -- -D warnings` — clean.
  - [ ] `SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test` — full suite green. Expected: ~728 lib tests + 9 new (`find_id_by_*` × 3 cases × 3 models) ≈ ~737; +13 new integration tests in `tests/home_scan_redirect.rs`.
  - [ ] `cargo sqlx prepare --check --workspace` — no diff (Tasks 1 + 2 use dynamic `query_scalar` / `query_as`).
  - [ ] Manual smoke (`MYBIBLI_SKIP_SETUP=1 cargo run`):
    - As anonymous: `curl "http://localhost:8080/scan?code=9782070360246"` → 303 + `Location: /title/<id>` (if seeded) or `/catalog?code=9782070360246`.
    - As anonymous: `curl -H 'HX-Request: true' "http://localhost:8080/scan?code=V0042"` → 200 + `HX-Redirect: /volume/<id>` (if seeded).
    - In a browser on `/`: type "tintin" slowly → search results appear inline. Type quickly + Enter → page navigates per scan classification.
  - [ ] **E2E** (Foundation Rule #13): `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/home-search.spec.ts`.
  - [ ] Update Dev Agent Record at the bottom of this file: list of files touched, decisions on placement (handler in `catalog.rs` vs new `home_scan.rs`; HTMX wiring Pattern A/B/C), drift discoveries (the 4-state machine was already shipped in `search.js`), JS unit tests deferred per AC15.
  - [ ] Update `_bmad-output/implementation-artifacts/sprint-status.yaml`: `9-9-home-scanner-state-machine: ready-for-dev → in-progress` at start, `→ review` at end (only this line + `last_updated`, per CLAUDE.md rule 16).
  - [ ] Open draft PR at first commit (Foundation Rule #15). Title: `Story 9-9: Home page scanner detection state machine (#NN)`.

## Dev Notes

### Source tree references

| Concern | File / location | Notes |
|---|---|---|
| **Already shipped (state machine)** | `static/js/search.js` (172 LOC) | 4-state machine + Enter/Escape handlers + debounce + HTMX listeners ALREADY exist; this story only ADDS the `fireScan` event dispatch + a11y announcements |
| Existing scan classifier | `src/routes/catalog.rs:336-410` (`detect_code_type` + `CodeDetection` struct + `validate_issn_from_ean`) | REUSE as-is; do NOT duplicate. The new home-scan handler imports + calls it. |
| Existing POST `/catalog/scan` | `src/routes/catalog.rs::handle_scan` (line 412+) | UNRELATED — this is the cataloging workflow handler. Untouched by this story. |
| Existing POST `/catalog/scan-with-type` | `src/routes/catalog.rs::handle_scan_with_type` (line 1109+) | UNRELATED. |
| Home search field | `templates/pages/home.html:15-34` | extend with the hidden HTMX-trigger element (Pattern B) + the `<span aria-live="polite">` + 2 new `data-announce-*` attributes |
| Home handler | `src/routes/home.rs::home` | extend with 2 new pre-translated label fields (`searching_announcement`, `scanning_announcement`) — minimal handler change |
| HomeTemplate | `src/routes/home.rs:32-126` (post-9-7 with ~80 fields) | +2 new fields |
| **NEW** home-scan handler | `src/routes/home_scan.rs` (NEW) OR `src/routes/catalog.rs` | Task 1 decision based on `catalog.rs` LOC. Recommendation: NEW module. |
| **NEW** route registration | `src/routes/mod.rs` (`/scan` GET) | next to existing `/catalog/scan` POST |
| Title model | `src/models/title.rs` | +`find_id_by_isbn` narrow lookup |
| Volume model | `src/models/volume.rs` | +`find_id_by_label` narrow lookup |
| Location model | `src/models/location.rs` | +`find_id_by_label` narrow lookup |
| Title schema | `migrations/20260329000000_initial_schema.sql:88-113` | `titles.isbn VARCHAR` (verify the type — could be `CHAR(13)` or VARCHAR; the existing `find_by_isbn` likely tells us) |
| Volume schema | `migrations/20260329000000_initial_schema.sql:120-145` | `volumes.label CHAR(5)` (UNIQUE constraint locks one volume per label) |
| Location schema | `migrations/20260329000000_initial_schema.sql:??` | `storage_locations.label CHAR(5)` (verify type) |
| Existing scanner E2E helpers | `tests/e2e/helpers/scanner.ts` (`simulateScan` 20ms, `simulateTyping` 100ms) | REUSE as-is for AC12 |
| Existing home-search E2E | `tests/e2e/specs/journeys/home-search.spec.ts` (10 tests) | EXTEND with the new `test.describe("scan to navigate")` block |
| Other JS modules (untouched) | `static/js/focus.js` (54 LOC), `static/js/scanner-guard.js` (177 LOC) | listed in AC8 — coexistence verified (no overlap with `#search-field` event handlers) |
| i18n locales | `locales/en.yml`, `locales/fr.yml` | append 3 keys under a NEW `home_scan:` block (or extend existing `home:` block — pick the cleaner placement) |
| i18n audit | `src/i18n/audit.rs::all_t_keys_have_both_locales` | enforces EN/FR mirror |
| Templates audit | `src/templates_audit.rs::no_inline_markup_in_templates` (line 44) | must stay green |

### Anti-patterns to avoid

- **Reimplementing the 4-state machine.** It's already in `static/js/search.js`. This story adds the `scan-fire` event dispatch + a11y announcements + server endpoint — NOT a full rewrite. Don't touch the IDLE/DETECTING/SEARCH_MODE/SCAN_PENDING transition logic.
- **Duplicating `detect_code_type` in the new handler.** Import from `crate::routes::catalog::detect_code_type` and reuse. The existing function has been used since story 1-3; its classification is the project canon.
- **Adding role gate to the new `/scan` handler.** FR65 says any user can search/scan-to-navigate. The DESTINATION may be role-gated (e.g., `/catalog` is Librarian-only — Anonymous gets 303 → /login from there); the scan endpoint itself is role-blind. AC6 + AC13 lock this.
- **Issuing `find_*_by_*` queries in parallel for "performance".** The classifier already narrows the code to ONE candidate type — only ONE DB lookup is needed per request. Parallel lookups would be wasteful and could leak codes that match accidentally across types (e.g., a 13-digit number could in theory be both an ISBN AND a UPC; the classifier already disambiguates).
- **`sqlx::query!` macro.** Forces `cargo sqlx prepare` regeneration in the PR. Use dynamic `query_scalar` / `query_as` (project convention).
- **Calling `t!()` from inside JS.** JS can't evaluate Rust macros. Pre-translate in the handler, pass as `data-*` attributes on `#search-field`. Project convention; mirrors the existing `data-connection-lost` attribute (`home.html:33`).
- **Inline `style="display:none"` on the new hidden HTMX element.** Use `class="hidden"` (Tailwind utility) — CSP-clean. Inline static style attributes ARE allowed by CSP but conflict with the project's UX-DR24 "Tailwind utility classes only" convention.
- **Adding `prefers-reduced-motion` CSS to this story.** No animation in this story (AC9). Adding the media query speculatively is YAGNI.
- **Setting up a JS test harness in this PR.** Out of scope per AC15. File as `type:code-review-finding` GH Issue.
- **Refactoring `search.js` to extract the state machine into a smaller module.** Refactor-during-feature is anti-pattern. The file is 172 LOC; manageable.
- **Removing the existing `data-debounce` and `data-scanner-threshold` attributes from `home.html`.** They're load-bearing for `search.js`; leave them as-is.

### Architecture compliance

- **Error handling:** Any DB failure in the 3 new `find_id_by_*` methods returns `AppError::Database` via `?`; the `handle_home_scan` handler soft-degrades with `tracing::warn!` + fallback redirect to `/catalog?code=...`. The home page MUST NOT 500 on a scan that hits a DB hiccup.
- **Logging:** `tracing::info!(code = %code, "scan classified as ...")` at the handler — useful for debugging scan misclassification. NEVER log session.token or borrower data on the scan path.
- **DB query discipline:** Every SELECT MUST include `deleted_at IS NULL`. The 3 new queries inherit this pattern. Locked by `home_scan_excludes_soft_deleted_*` integration tests.
- **CSP middleware:** Already wraps the handler outermost. No work needed.
- **Pool access:** The handler already has `state.pool: DbPool`. No new connection.
- **Foundation Rule #14 one-branch-one-story:** Verify `git status` clean + on `main` + `git pull --ff-only` before starting; cut `story/9-9-home-scanner-state-machine`. Open a draft PR (Rule #15) at the first commit.
- **Foundation Rule #12:** N/A for `volume_detail.html` / `home.html` (small additions). For `catalog.rs` (already 2675 LOC pre-9-9), if the new handler lands there it grows by ~80 LOC; the cleaner placement is a NEW `src/routes/home_scan.rs` module to avoid further bloat.

### Library / framework requirements

- **Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + Tailwind CSS v4 + HTMX 2.0** — no version changes, no new dependencies.
- **rust_i18n** — already wired. Pre-translate the 3 new keys in the handler. Use `%{}` interpolation if needed (no interpolation needed here — the 3 keys are static strings).
- **HTMX HX-Redirect header** — verified via existing `src/routes/home.rs::home` use of `HX-Redirect` (e.g., for L-code redirects in the search path). Same pattern.
- **URL encoding** — verify the project's existing URL encoding crate (likely `urlencoding` or `percent_encoding`) by grepping for `urlencoding::encode\|percent_encoding`. Use whichever is already in `Cargo.toml`.

### File structure requirements

| File | Action | Rough size |
|---|---|---|
| `src/models/title.rs` | **edit** | +20-30 LOC (`find_id_by_isbn` async fn + 3 unit tests) |
| `src/models/volume.rs` | **edit** | +20-30 LOC (same pattern for `find_id_by_label`) |
| `src/models/location.rs` | **edit** | +20-30 LOC (same pattern) |
| `src/routes/home_scan.rs` | **create** (recommended) | ~80-100 LOC (handler + ScanQuery struct) |
| `src/routes/mod.rs` | **edit** | +1 line route registration + 1 line `pub mod home_scan;` |
| `src/routes/home.rs` | **edit** | +2-4 LOC (HomeTemplate +2 fields + 2 pre-translation calls; minimal) |
| `static/js/search.js` | **edit** | +15-20 LOC (`fireScan` function + state-transition wiring + announce helper) |
| `templates/pages/home.html` | **edit** | +5-8 LOC (hidden HTMX-trigger element + aria-live span + 2 new data-* attrs) |
| `locales/en.yml` | **edit** | +3 lines under `home_scan:` (or `home:`) block |
| `locales/fr.yml` | **edit** | +3 lines mirror |
| `tests/home_scan_redirect.rs` | **create** | ~250-300 LOC (13 `#[sqlx::test]` cases + helpers) |
| `tests/e2e/specs/journeys/home-search.spec.ts` | **edit** | +60-80 LOC (1 new `test.describe` block, 4 tests) |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **edit** | only the `9-9-...` line + `last_updated` |
| `_bmad-output/implementation-artifacts/9-9-home-scanner-state-machine.md` | **edit** | this file — Status, Tasks checked, Dev Agent Record |

### Testing requirements

- **Coverage target:** every AC has at least one test (unit OR E2E). AC10 (CSP) is covered by `templates_audit.rs::no_inline_markup_in_templates` (existing — must stay green). AC15 (JS unit tests) is explicitly deferred.
- **AC11a integration tests** (13 cases) lock the server-side scan-redirect logic end-to-end. The soft-delete-exclusion tests (3 cases) are the privacy/data-integrity safety guards.
- **AC11b model unit tests** (9 cases) lock the narrow projection contracts.
- **AC12 E2E tests** (4 cases) lock the full scan-to-navigate journey from the user's perspective.
- **AC8 coexistence verification** — manual: grep for `#search-field` in `focus.js` and `scanner-guard.js`, expect zero matches. Document the result in the Dev Agent Record.

### Project structure notes

This story is mostly a "wire the existing pieces together" story rather than a from-scratch implementation. Three intentional design decisions:

1. **The 4-state machine is reused as-is.** The original spec implied building it from scratch; reality is `static/js/search.js` already has it. This story adds 2 things: the `scan-fire` event dispatch (Task 3) and the a11y announcements (Task 5). The state-machine logic itself is untouched.

2. **NEW server endpoint at `/scan`** (NOT extending POST `/catalog/scan`). The cataloging workflow handler is heavyweight (creates titles, manages session, fires async metadata fetches) and Librarian-gated. The home-scan endpoint is light (3 narrow lookups + redirect) and role-blind (FR65). Conflating them would entangle two surfaces with different contracts. Mirror of how the indicator subsystem (9-4..9-7) deliberately kept the dashboard separate from the catalog/loans pages.

3. **NEW narrow `find_id_by_*` model methods** — NOT existing wider lookups. Task 1 verifies that no existing narrow `find_id_by_*` methods are present (the existing `find_by_isbn` returns the full `TitleModel` struct — wider than what the redirect handler needs). New narrow methods avoid loading unused columns over the wire AND keep the contract focused. Mirror of 9-6's `SeriesWithGap` decision.

4. **Drift discoveries to document at story close**:
   - 4-state machine already shipped in `search.js` pre-9-9 (despite spec text implying it's new).
   - `tests/e2e/helpers/scanner.ts` already exists with `simulateScan` + `simulateTyping`.
   - Existing `detect_code_type` + `validate_issn_from_ean` ALREADY handle the prefix classification.
   - JS unit test framework not configured — AC15 deferred.

### Schema reality check (drift discoveries from spec text)

- The 4-state machine + scanner helpers are already shipped (see Drift section above). The story scope is much smaller than the spec text implied.
- `titles.isbn` type — verify (likely VARCHAR but could be CHAR). The existing `find_by_isbn` on `TitleModel` is the canonical reference.
- `storage_locations.label` schema — verify the column exists with `CHAR(5)` constraint.
- The handler MUST handle the case where `code` arrives URL-decoded already (Axum handles this for query strings). No double-decoding needed.

If a fresh schema drift is discovered during dev, document inline in the test helper AND in the Dev Agent Record's "drift discoveries" section.

## References

- [Story 9.9 spec — `_bmad-output/planning-artifacts/epics.md` lines 1353-1370](../planning-artifacts/epics.md)
- [PRD FR65 (anonymous browsing) + FR95 (anonymous catalog/series read) — `_bmad-output/planning-artifacts/prd.md`](../planning-artifacts/prd.md)
- [UX-DR26 (scanner detection state machine) — `_bmad-output/planning-artifacts/ux-design-specification.md`](../planning-artifacts/ux-design-specification.md)
- [`static/js/search.js` — already-shipped 4-state machine](../../static/js/search.js)
- [`src/routes/catalog.rs:336-410` — existing `detect_code_type` + `validate_issn_from_ean` (REUSED)](../../src/routes/catalog.rs)
- [`src/routes/catalog.rs:412+` — existing POST `/catalog/scan` (UNRELATED — cataloging workflow)](../../src/routes/catalog.rs)
- [`tests/e2e/helpers/scanner.ts` — `simulateScan` + `simulateTyping` (REUSED)](../../tests/e2e/helpers/scanner.ts)
- [`tests/e2e/specs/journeys/home-search.spec.ts` — existing E2E (EXTENDED)](../../tests/e2e/specs/journeys/home-search.spec.ts)
- [`templates/pages/home.html` — search field with `data-debounce` + `data-scanner-threshold` already wired](../../templates/pages/home.html)
- [`CLAUDE.md` — Foundation Rules + UX-DR24 (Tailwind only)](../../CLAUDE.md)
- [Story 9-8 spec (canonical patterns: NEW narrow projection structs, soft-degrade, CSP-clean macros) — `9-8-loan-status-role-aware.md`](./9-8-loan-status-role-aware.md)
- [Story 9-7 spec (canonical pattern: closes-the-chapter narrative, cumulative test counts) — `9-7-recent-activity-indicators.md`](./9-7-recent-activity-indicators.md)

## Dev Agent Record

### Agent Model Used

`claude-opus-4-7` (1M context).

### Debug Log References

_(to be filled by dev agent)_

### Completion Notes List

_(to be filled by dev agent)_

### File List

_(to be filled by dev agent)_

### Change Log

_(to be filled by dev agent)_
