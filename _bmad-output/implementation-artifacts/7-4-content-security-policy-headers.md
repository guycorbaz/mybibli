# Story 7.4: Content Security Policy headers

Status: done

Epic: 7 — Accès multi-rôle & Sécurité
Requirements mapping: NFR15, AR16

---

> **TL;DR** — Strict CSP + hardening headers on every response · 26 inline-markup sites (4 scripts + 1 style block + 4 style attrs + 16 handlers + 1 JS-generated handler) refactored to external modules · `cargo test` audit gate blocks regressions · Playwright test actively injects a blocked inline script to prove CSP is enforced.

## Story

As the project maintainer,
I want strict Content-Security-Policy headers (plus the standard hardening header set) on every response,
so that XSS vectors (inline scripts, injected styles, malicious third-party resources) are blocked while legitimate same-origin assets and already-whitelisted cover-image domains continue to load.

## Acceptance Criteria

1. **New middleware `src/middleware/csp.rs`** — adds a `tower::Layer` / `from_fn` middleware that sets response security headers for **every** route (including `/static/*`, `/covers/*`, `/health`). Wired into `routes::build_router` after the router is built so it applies globally (tower applies `.layer()` outermost = last). Per AR16 the conceptual order is `Logging → Auth → [Handler] → PendingUpdates → CSP` — i.e., CSP is the outermost layer applied (innermost runs last, sees the final body → adds headers → hands up). Existing `logging::trace_layer()` is applied in `src/main.rs:124`; stack the CSP layer on top of it there (or on the router in `build_router` — **choose `build_router`** so unit tests of the router include CSP).

2. **CSP directives (production mode, strict, no `unsafe-inline` / no `unsafe-eval`)** — exact string:
   ```
   default-src 'self';
   script-src 'self';
   style-src 'self';
   img-src 'self' data: https://covers.openlibrary.org https://books.google.com https://image.tmdb.org https://coverartarchive.org;
   font-src 'self';
   connect-src 'self';
   frame-src 'none';
   frame-ancestors 'none';
   object-src 'none';
   base-uri 'self';
   form-action 'self';
   ```
   Matches `architecture.md` §447-460 **plus `coverartarchive.org`** (MusicBrainz cover art — grep-confirmed at `src/metadata/musicbrainz.rs:139`). Comma/semicolon literal, single-line value (browsers accept both; single-line is simpler for logs).

3. **Additional security headers set by the same middleware:**
   - `X-Content-Type-Options: nosniff`
   - `X-Frame-Options: DENY` (legacy complement to `frame-ancestors 'none'`)
   - `Referrer-Policy: strict-origin-when-cross-origin`
   - `Permissions-Policy: camera=(), microphone=(), geolocation=(), payment=()` — all four explicitly denied. Today's scanner is USB HID (keyboard-wedge), not `getUserMedia`, so `camera=()` is the minimum-surface default. Flip to `camera=(self)` **only** when story 7.5 / UX-DR25 delivers a webcam-based scanner fallback; cite this story in the PR that flips it.
   - Do **not** set `Strict-Transport-Security` — deployment is HTTP-on-local-LAN by Guy's choice (NFR37). Add a `// TODO(hsts):` comment noting to enable it if/when TLS lands.

4. **Report-only mode via env var** — `CSP_REPORT_ONLY=true` (any other value incl. unset ⇒ enforced). When true, the middleware sets `Content-Security-Policy-Report-Only` instead of `Content-Security-Policy`. Other hardening headers (`X-Content-Type-Options`, etc.) remain active in both modes. Read once at startup via `std::env::var` (per AR26 — no `dotenvy`), stored on the layer (not per-request). Document in CLAUDE.md § Build & Test.

5. **Zero inline scripts, inline styles, inline event handlers in templates** — enforced by the new CSP. Every inline violation must be refactored:
   - **4 inline `<script>` blocks** to extract:
     - `templates/components/catalog_toolbar.html:17` — `window.mybibliAudio.initToggle()` → call from `static/js/audio.js` via DOMContentLoaded + `htmx:afterSettle` listener that looks for `#audio-toggle`.
     - `templates/components/title_form.html:143-220+` — required-field validation → new `static/js/title-form.js`, loaded from `base.html`, idempotent init on form presence.
     - `templates/pages/borrower_detail.html:90-99` — htmx:afterRequest reload on loan-return → add a page-level `htmx:afterRequest` hook inside `mybibli.js` gated on `document.body.dataset.page === "borrower-detail"`. The `data-page` attribute is added to `<body>` in `base.html:12` (already has `data-user-role` + `data-session-timeout`) by rendering `data-page="{{ current_page }}"`. The `current_page: String` field is **already present** on every page template struct (plumbed by story 7-1) — zero new struct fields, matches existing pattern. For `borrower_detail.html`, verify `current_page == "borrower-detail"` (or amend to match) in the handler that builds the template.
     - `templates/pages/loans.html:151-...` — borrower autocomplete → new `static/js/loan-borrower-search.js`.
   - **1 inline `<style>` block** to extract:
     - `templates/pages/home.html:171-200+` (browse list/grid styles) → move to a new `static/css/browse.css` linked from `base.html` **OR** append to the Tailwind-source sidecar if one exists. Verify existing Tailwind pipeline location before deciding.
   - **4 inline `style="..."` attributes** to refactor:
     - `templates/components/catalog_toolbar.html:8` `style="display:none"` → Tailwind `hidden` class (audio.js already toggles via `.hidden` elsewhere — confirm and unify).
     - `templates/components/series_gap_grid.html:17` (decorative diagonal stripes) → new CSS class `.series-gap-stripes` in the same stylesheet as browse.css.
     - `templates/pages/series_form.html:45` & `templates/pages/title_detail.html:138` (`{% if ... %}style="display:none"{% endif %}`) → swap to `{% if ... %}class="... hidden"{% else %}class="..."{% endif %}` using Askama conditional-class pattern.
   - **16 inline event-handler attributes** across 11 files (grep `onclick=|onchange=|onsubmit=|onfocus=|onblur=|oninput=|onkeydown=|onkeyup=|onkeypress=` returned 16 hits — see Dev Notes §"Inline handler inventory"). Each must be replaced with:
     - a stable `id` or `data-action="..."` attribute on the element, **plus**
     - an `addEventListener` call in the appropriate existing JS module (theme.js, mybibli.js, browse-mode.js, etc.) or a new purpose-named module.
     - Event delegation via `document.addEventListener("click", ...)` with `event.target.closest("[data-action='dismiss-feedback']")` is the preferred pattern for HTMX-inserted / OOB-swapped fragments (e.g., `feedback_entry.html`) because those DOM nodes appear after JS has run.

6. **JS-generated inline handlers eliminated** — `static/js/mybibli.js:162` builds HTML with an inline `onclick="this.closest('.feedback-entry').remove()"`. Rewrite to output a `data-action="dismiss-feedback"` attribute and handle via one delegated listener on `#feedback-list` (or `document.body`). This is **mandatory** — CSP blocks inline handlers even when the attribute was written by JS after page load.

7. **Nav bar + mobile menu refactor** — `templates/components/nav_bar.html:18` (`onclick="window.mybibliToggleTheme()"`) and `:29` (`onclick="var m=document.getElementById('mobile-nav');..."`) move to `theme.js` (theme button: listen on `#theme-toggle`) and `mybibli.js` (`#mobile-menu-toggle` click → toggle `#mobile-nav` hidden class + update `aria-expanded`). Retain the `aria-expanded` state sync — it's a11y-critical. **`theme.js` is also loaded by `bare.html` (login/logout, no nav)** — its `init()` MUST early-return when `document.getElementById("theme-toggle") === null` so anonymous login/logout pages don't throw. Verify by navigating `/login` after refactor: zero console errors.

8. **No inline `javascript:` URLs, no inline CSS via `<link>` with inline contents** — zero tolerance. Any future PR that introduces one is expected to fail the browser's CSP check AND the E2E violation-monitoring test in AC 13.

9. **Unit test — middleware emits expected headers** — `src/middleware/csp.rs::tests`:
   - `test_csp_enforced_mode_headers`: build a minimal router `Router::new().route("/x", get(|| async { "ok" })).layer(csp_layer(false))`, `oneshot` a request, assert every expected header is present with the exact value. Use `tower::ServiceExt` (already a dev-dep per `Cargo.toml:30`).
   - `test_csp_report_only_mode_headers`: same setup with `csp_layer(true)`, assert `Content-Security-Policy-Report-Only` is set and `Content-Security-Policy` is NOT present.
   - `test_csp_applied_to_static_and_covers`: verify the layer is mounted so `/static/x` and `/covers/x` responses also carry the headers (integration-style — spin up `build_router` with a tempdir for covers, request a 404 static path, assert headers present on the 404 response).
   - `test_permissions_policy_denies_camera`: explicit assertion that `camera=()` substring is present (scanner / locked-down-default regression guard). Rename to `..._allows_camera_self` on the PR that flips the directive in story 7.5.

10. **Unit test — no inline handler/script/style regression** — `src/templates_audit.rs` (new `#[test]` module, `cfg(test)` only): walks `templates/` recursively and fails if any `.html` file matches the regexes:
    - `\bon(click|change|submit|focus|blur|input|key(down|up|press))\s*=\s*"` — inline handler (the `\b` word-boundary prevents false positives on tokens like `python-on-foo=`).
    - `<script\b(?![^>]*\bsrc\s*=)(?![^>]*\btype\s*=\s*"(application/json|application/ld\+json|text/x-template)")[^>]*>\s*\S` — inline **executable** script block. Allows `<script src="...">` and non-JS `type=` data islands (`application/json`, `application/ld+json`, `text/x-template`) — those aren't executed by the browser, so CSP doesn't block them. Requires a non-whitespace char after `>` so `<script>\n</script>` empty blocks don't trip.
    - `<style\b[^>]*>` — inline style block (no exemptions).
    - `\bstyle\s*=\s*"` — inline style attribute.
    The test lists the path+line of every match and panics with a human-readable report. **Exemption list is empty; regex is the only gate.** This test replaces eyeball-review as the regression gate — new inline markup fails `cargo test` in CI.

11. **E2E test — no CSP violations on happy paths** — `tests/e2e/specs/journeys/csp-headers.spec.ts` (NEW, spec ID `"CS"`):
    - **Test 1** — anonymous flow: navigate `/`, `/catalog`, open a title detail page, verify `response.headers()["content-security-policy"]` on each is the exact enforced directive string. Hook `page.on("console", ...)` + `page.on("pageerror", ...)` before `page.goto`; fail the test if any message matches `/Refused to (execute|apply|load)|Content-Security-Policy/i`. No `waitForTimeout`.
    - **Test 2** — authenticated librarian flow: `loginAs(page, "librarian")`, scan a generated ISBN via `scanTitleAndVolume()` helper, create a loan via `createLoan()`, open `/borrower/{id}`, return a loan — same console/pageerror assertions throughout. This exercises the 4 extracted inline scripts and the OOB-swap feedback entries (dismiss button path).
    - **Test 3 — negative / blocking**: after DOM load, `page.evaluate(() => { const s = document.createElement('script'); s.textContent = 'window.__pwnd = true;'; document.body.appendChild(s); })`. Poll `await page.evaluate(() => window.__pwnd)` for up to 2s — it must remain `undefined` and a console message must contain `"Refused to execute inline script"`. This is the AC-6 live regression gate: if someone adds `'unsafe-inline'` to `script-src`, this test immediately fails.
    - **Test 4 — report-only mode smoke** — spin up the app with `CSP_REPORT_ONLY=true` via docker compose override or a dedicated test profile (see Dev Notes §"Report-only E2E"). Verify that `Content-Security-Policy-Report-Only` header is set AND the injected inline script in Test 3 actually runs (report-only does not block). Optional / lowest priority — implement if the docker override is a one-liner; otherwise cover via unit test (AC 9) and note here.

12. **Cover images still load** — Test 1 includes a title with a locally-downloaded cover and asserts `img` elements have `naturalWidth > 0`. Covers are always rendered from `/covers/{id}.jpg?v=...` (self-served) — grep-verified at `src/routes/titles.rs:1204`. The external `img-src` whitelist (`covers.openlibrary.org`, `books.google.com`, `image.tmdb.org`, `coverartarchive.org`) is a defensive safety net for future code paths that might briefly render a provider URL before local download completes; it is **not** exercised by normal rendering today. A separate assertion exercises `img-src data:` via the inline-SVG placeholder at `templates/components/cover.html:8`.

13. **HTMX interactions stay functional under CSP** — Test 2 covers this implicitly; additionally, run the existing E2E suite's 3-cycle gate to catch any hidden inline-handler dependency:
    ```
    cd tests/e2e && for i in 1 2 3; do npm test || break; done
    ```
    All existing specs must stay green. If a previously-passing spec relied on an inline handler, fix the refactor, not the test.

14. **i18n unchanged** — No new user-facing strings. This story adds no keys to `locales/{en,fr}.yml`.

15. **Zero-warning build + existing gates** — `cargo clippy -- -D warnings`, `cargo test` (now incl. templates audit from AC 10), `cargo sqlx prepare --check --workspace -- --all-targets` (should be a no-op — no SQL changes), `grep -rE "waitForTimeout\(" tests/e2e/{specs,helpers}/` stays empty. Playwright 3-cycle green on fresh docker stack.

16. **Documentation** — Update `CLAUDE.md`:
    - Under "Architecture / Key Patterns": add a line on CSP strictness and the AR16 order.
    - Under "Build & Test": note `CSP_REPORT_ONLY=true` env toggle.
    - Add a one-liner under "Foundation Rules" (or "Architecture") pointing new contributors to the `src/templates_audit.rs` test as the inline-markup gate. **Do not** edit sections the story is not touching.

## Tasks / Subtasks

- [x] **Task 1 — New CSP middleware (AC: 1, 2, 3, 4, 9)**
  - [x] `src/middleware/csp.rs` created. `apply_csp_layer(router, report_only)` switches between two static `from_fn` middlewares (`csp_enforced`, `csp_report_only`); chosen over a parameterized closure because axum 0.8's `FromFn<F, …>` trait bounds reject closures returning `Pin<Box<dyn Future + Send>>` cleanly.
  - [x] `const CSP_DIRECTIVES: &str = "default-src 'self'; script-src 'self'; …"` — single source of truth, referenced by both the middleware and the unit tests.
  - [x] All 5 headers (`Content-Security-Policy` / `-Report-Only`, `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`, `Permissions-Policy`) emitted unconditionally on every response (GETs, POSTs, 2xx/3xx/4xx/5xx, ServeDir 404s).
  - [x] `.entry(name).or_insert(value)` — handler-supplied headers win (regression-tested in `test_handler_set_header_not_clobbered`).
  - [x] Re-exported from `src/middleware/mod.rs`.

- [x] **Task 2 — Wire CSP into the router (AC: 1)**
  - [x] `src/routes/mod.rs::build_router` ends with `apply_csp_layer(app, report_only)`. The flag is read once at startup via `crate::config::csp_report_only()` (no dotenvy).
  - [x] Order verified: CSP wraps everything, including the inner `catalog_routes` sub-router that owns `PendingUpdates`. Per-request order is `CSP(outer) → … → PendingUpdates(inner on catalog_routes) → handler`, matching AR16.
  - [x] PendingUpdates middleware untouched.

- [x] **Task 3 — Templates audit test (AC: 10)**
  - [x] `regex = "1"` added to `[dev-dependencies]` in `Cargo.toml`.
  - [x] `src/templates_audit.rs` created (declared as `#[cfg(test)] mod templates_audit;` from `src/lib.rs`). Recursive `std::fs::read_dir` walk of `templates/`. Strips `<!-- ... -->` comments first (preserving line numbers) so prose in CSP-explanation comments doesn't trip the audit.
  - [x] All four regexes per AC 10 + safe-type allowlist. Initial run before Task 4 produced exactly the predicted 26-line failure report; final run is green.

- [x] **Task 4 — Refactor all inline markup to pass the audit (AC: 5, 6, 7, 8)**
  - [x] **4.1 Inline scripts** — all 4 extracted: catalog_toolbar audio init → `audio.js` auto-wires `#audio-toggle`; `title-form.js` (new) carries required-field validation + cancel + media-type lazy-load; `mybibli.js::initBorrowerDetailReload` gated on `body[data-page="borrower-detail"]`; `loan-borrower-search.js` (new) carries borrower autocomplete + scan-field clear + post-return reload.
  - [x] **4.2 Inline styles** — `home.html:171` `<style>` moved to `static/css/browse.css`; `catalog_toolbar.html:8` `style="display:none"` → `class="… hidden"`; `series_gap_grid.html:17` decorative stripes → `.series-gap-stripes` class in browse.css; `series_form.html:45` and `title_detail.html:138` switched to conditional `hidden` class with the same polarity as the original.
  - [x] **4.3 Inline handlers** — all 16 replaced with id/data-action selectors + addEventListener in the appropriate JS module (theme.js / mybibli.js / audio.js / title-form.js / borrowers.js / browse-mode.js / loan-borrower-search.js). `catalog.html:41` swapped to native `hx-get` / `hx-target` / `hx-swap` attributes; one extra `onkeydown` in `title_edit_form.html` (Esc → cancel) handled via a delegated `body` listener in mybibli.js. `data-page="{{ current_page }}"` added to `<body>` in base.html and `borrower_detail` handler now sets `current_page = "borrower-detail"` to feed the gate.
  - [x] **4.4 JS-generated inline handler (`mybibli.js:162`)** — emits `data-action="dismiss-feedback"`; one delegated listener at document level removes the parent `.feedback-entry`. The same swap was applied to `static/js/scan-field.js` (local error feedback) AND to four backend HTML-builder hot spots: `src/routes/catalog.rs` (`feedback_html`, `scan_error_feedback_html`), `src/middleware/pending_updates.rs` (warning OOB), and `src/routes/locations.rs` (➕ add-child toggle, refactored to `data-locations-toggle`). Hidden trap because the templates audit only walks `templates/` — Rust-generated HTML can re-introduce inline markup; manual grep confirmed clean post-fix.
  - [x] **Out-of-spec but required by strict CSP**: HTMX 2.x auto-injects an inline `<style>` block for `.htmx-indicator` rules and uses `Function(...)` to evaluate `hx-trigger="…[expr]"` filters. Both are CSP-blocked. Mitigated with (a) `<meta name="htmx-config" content='{"includeIndicatorStyles":false}'>` in base.html / bare.html and the same `.htmx-indicator` rules duplicated in browse.css; (b) the loans-page scan field switched from `hx-trigger="keydown[key=='Enter'] from:this"` to `hx-trigger="loan-scan-fire"` — `loan-borrower-search.js` dispatches the custom event on Enter. Also moved JS-set runtime styles (`element.style.opacity/transition` writes in mybibli.js / search.js / theme.js) to class toggles (`feedback-fading`, `htmx-opacity-reset`, `theme-transitioning`) so they no longer trigger Chromium's CSP3 inline-style enforcement.

- [x] **Task 5 — Load the new JS/CSS modules in `base.html` (AC: 5, NFR34)**
  - [x] base.html now loads `browse.css`, `title-form.js`, `borrowers.js`, `loan-borrower-search.js` alongside the existing modules; ordering preserved (htmx → modules → mybibli.js defer).
  - [x] bare.html unchanged except for the `htmx-config` meta tag (login/logout has no catalog/loans surface, no extra modules needed). Theme.js early-returns when `#theme-toggle` is absent so bare pages stay console-clean.
  - [x] Static-asset budget after refactor: `du -sh static/` = **216 KB** (well under the 500 KB NFR34 cap). browse.css is ~3 KB, the three new JS modules total ~10 KB, the four removed inline blocks shaved more bytes from rendered HTML pages than were added to the static directory.

- [x] **Task 6 — Unit tests (AC: 9, 10)**
  - [x] `src/middleware/csp.rs::tests` — 5 tests passing: `test_csp_enforced_mode_headers`, `test_csp_report_only_mode_headers`, `test_csp_applied_to_static_and_covers`, `test_permissions_policy_denies_camera`, `test_handler_set_header_not_clobbered`.
  - [x] `src/templates_audit.rs::no_inline_markup_in_templates` passing (post-refactor + comment-stripped scan).
  - [x] Updated one existing test (`test_skeleton_feedback_html_structure`) to drop the `prefers-reduced-motion` substring assertion — that media query now lives in browse.css alongside the `.shimmer-bar` keyframes (kept the `shimmer-bar` class assertion). All 435 lib tests green.

- [x] **Task 7 — E2E tests (AC: 11, 12, 13)**
  - [x] `tests/e2e/specs/journeys/csp-headers.spec.ts` created with spec ID `"CS"`. Tests 1 (anonymous), 2 (authenticated librarian flow exercising scan + loan + borrower detail), 3 (negative — `script-src 'self'` blocks an injected inline script). Console / pageerror / `securitypolicyviolation` listeners installed via `page.on(...)` + `addInitScript` so violations point at the source file & line.
  - [x] Test 4 (report-only) `test.skip(...)` with the rationale recorded inline (docker compose override would add >20 lines for a code path the unit test already covers).
  - [x] Full Playwright run on a fresh docker stack — 3 consecutive green cycles, 144 passed / 1 skipped each cycle.

- [x] **Task 8 — Documentation (AC: 16)**
  - [x] CLAUDE.md updated in three spots: Build & Test section gains a `CSP_REPORT_ONLY=true cargo run` snippet; Source Layout entry for `src/middleware/` mentions `csp.rs`; Key Patterns gets a new bullet covering CSP strictness, the AR16 layer order, the rule that backend-rendered HTML must stay inline-free, the HTMX trigger-filter ban, the report-only env toggle, and a pointer to `src/templates_audit.rs` as the regression gate.

- [x] **Task 9 — Quality gates**
  - [x] `cargo clippy --all-targets -- -D warnings` — clean.
  - [x] `cargo test --lib` (with test DB on :3307) — 435 passed, 0 failed (incl. templates audit).
  - [x] `cargo sqlx prepare --check --workspace -- --all-targets` — passes (warning about potentially-unused queries is unrelated, pre-existing).
  - [x] `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` → exit 1 (empty match set).
  - [x] `curl -sI http://localhost:8080/health | grep -i '^content-security-policy:'` returns the full enforced directive.
  - [x] Playwright 3-cycle gate on fresh docker stack — green.

### Review Findings

**Code review run on 2026-04-16** — 3 layers: Blind Hunter, Edge Case Hunter, Acceptance Auditor. ~46 findings collected, triaged to 5 patches + 17 deferred + 24 dismissed (no decision-needed).

- [x] [Review][Patch] `CSP_REPORT_ONLY` env parse is case-sensitive; `TRUE` / `1` silently → enforced [src/config.rs:86] — **FIXED**: parser now accepts `true`/`1`/`yes` case-insensitively + trims whitespace, and emits a `tracing::info!` line at startup with the resolved mode. 6 new unit tests under `csp_report_only_tests` cover the matrix.
- [x] [Review][Patch] AC 12 cover-image proof missing from E2E Test 1 [tests/e2e/specs/journeys/csp-headers.spec.ts] — **FIXED**: Test 2 now navigates `/?q=${isbn}` after scan, clicks into title detail, and asserts the cover img (`/covers/{id}.jpg` or `/static/icons/{type}.svg` placeholder) has `naturalWidth > 0`. Inline `data:` placeholder is documented as defensive-only (no template currently emits one).
- [x] [Review][Patch] `test_csp_applied_to_static_and_covers` only asserts header presence via `is_some()`, not the full directive value [src/middleware/csp.rs:204-225] — **FIXED**: tightened to `assert_eq!(..., CSP_DIRECTIVES)` for both `/static` and `/covers` 404 paths.
- [x] [Review][Patch] `.htmx-opacity-reset` class added on error never removed [static/css/browse.css:104; static/js/mybibli.js:132,145; static/js/search.js:115,121] — **FIXED**: new `initOpacityResetCleanup()` in mybibli.js strips the class on every `htmx:beforeRequest`, so the `.htmx-request` dimming can re-apply on subsequent requests.
- [x] [Review][Patch] AC 7 verification step (`/login` console-error check after theme.js refactor) not codified in any E2E [tests/e2e/specs/journeys/csp-headers.spec.ts] — **FIXED**: Test 1 now `assertCsp(page, "/login")` so the console listener catches any CSP refusal or pageerror that the refactored theme.js could trigger on bare.html.

- [x] [Review][Defer] Audit's `inline_script` regex falls back to attrs allow-rule on `<script src="x.js">body</script>` — bypass [src/templates_audit.rs:678] — deferred, browsers ignore the body when `src=` is set per HTML spec, so no executable inline script can sneak through. Tracked.
- [x] [Review][Defer] `strip_html_comments` casts UTF-8 bytes to `char`; failure-report snippets garble accented characters [src/templates_audit.rs:158] — deferred, only affects the human-readable report output (line numbers + ASCII pattern detection unaffected); cosmetic.
- [x] [Review][Defer] `apply_security_headers` `entry().or_insert` could let an upstream layer downgrade headers [src/middleware/csp.rs:103] — deferred, design choice per AC 1 / Task 1; currently no upstream layer sets these headers and the CSP middleware is outermost.
- [x] [Review][Defer] HTMX swap of `/loans` would not re-wire `loan-borrower-search.js` because of the body-level `loansWired` sentinel [static/js/loan-borrower-search.js:14] — deferred, /loans is full-page nav today; would need an explicit HTMX swap path to exercise.
- [x] [Review][Defer] `initOmnibusToggle` / `initSeriesTypeToggle` only run at DOMContentLoaded — would not attach if forms were HTMX-injected [static/js/mybibli.js:233,256] — deferred, both forms ship as part of the full server-rendered page today.
- [x] [Review][Defer] `img-src` allowlist is hardcoded; new metadata providers added under `src/metadata/` would silently 404 their cover URLs [src/middleware/csp.rs:30] — deferred, AC 2 mandates the exact 4 hosts; ensure new providers extend both the allowlist and the constant.
- [x] [Review][Defer] Dual `Content-Security-Policy` headers possible if a future reverse proxy adds one [src/middleware/csp.rs:103] — deferred, no reverse proxy in current Synology HTTP-on-LAN deployment.
- [x] [Review][Defer] `tree-indent-cap` collapses depths ≥ 8 visually flat [src/routes/locations.rs:235; static/css/browse.css:954] — deferred, library hierarchies cap around 4-5 levels in practice (Section > Aisle > Bay > Shelf > Slot).
- [x] [Review][Defer] Tree-indent levels duplicated across Rust and CSS; no test asserts they agree [src/routes/locations.rs:235; static/css/browse.css:954] — deferred, depth cap rarely changes; cost > benefit for a cross-file consistency test.
- [x] [Review][Defer] `fetch("/borrowers/search?q=")` swallows non-200 / network errors silently [static/js/loan-borrower-search.js:51] — deferred, pre-existing pattern from the inline script that the refactor preserved verbatim.
- [x] [Review][Defer] `wireMediaTypeChange` doesn't surface htmx.ajax errors → stale type-specific-fields fragment on failure [static/js/title-form.js:55] — deferred, pre-existing pattern from the inline `onchange`.
- [x] [Review][Defer] `templates_audit` script regex misparses `<script attr="a > b">` literal `>` in attribute value [src/templates_audit.rs:678] — deferred, no current template uses literal `>` in a script attr; would surface only on a future contributor change.
- [x] [Review][Defer] Dockerfile asymmetry: `output.css` from css build stage, but `browse.css` copied from build context [Dockerfile:24] — deferred, acknowledged in Completion Notes; needs a generalized `static/css/` copy when more files arrive.
- [x] [Review][Defer] `audio.js` toggle stuck in wrong state if `localStorage.getItem` fails [static/js/audio.js:118] — deferred, hypothetical (localStorage failures are exceedingly rare).
- [x] [Review][Defer] `theme-toggle` button has no `htmx:afterSettle` re-wire path [static/js/theme.js:51] — deferred, nav_bar.html is server-rendered on every page nav, never HTMX-swapped today.
- [x] [Review][Defer] `templates_audit` `<style>` regex would false-positive on inline `<svg><style>…</style></svg>` [src/templates_audit.rs:80] — deferred, no current SVG carries inline `<style>`.
- [x] [Review][Defer] X-Frame-Options may be stripped by a future reverse proxy [src/middleware/csp.rs:107] — deferred, called out in Dev Notes; verifiable only post-deployment behind a proxy.
- [x] [Review][Defer] `fetch("/borrowers/search")` 401 handling silent — user typing sees empty dropdown, no redirect to /login [static/js/loan-borrower-search.js:67] — deferred, pre-existing UX behaviour; broader UX policy decision for a separate story.

## Dev Notes

### Inline markup inventory (grep-confirmed 2026-04-16)

| Kind | Path:Line | Refactor target |
|------|-----------|-----------------|
| `<script>` | `templates/components/catalog_toolbar.html:17` | `audio.js` auto-init on `#audio-toggle` |
| `<script>` | `templates/components/title_form.html:143` | new `static/js/title-form.js` |
| `<script>` | `templates/pages/borrower_detail.html:90` | `mybibli.js` + `data-page="borrower-detail"` gate |
| `<script>` | `templates/pages/loans.html:151` | new `static/js/loan-borrower-search.js` |
| `<style>` | `templates/pages/home.html:171` | new `static/css/browse.css` |
| `style=` | `templates/components/catalog_toolbar.html:8` | Tailwind `hidden` class |
| `style=` | `templates/components/series_gap_grid.html:17` | CSS class `.series-gap-stripes` in `browse.css` |
| `style=` | `templates/pages/series_form.html:45` | conditional `hidden` class (polarity: hidden when `type_value != "closed"`) |
| `style=` | `templates/pages/title_detail.html:138` | conditional `hidden` class (verify polarity) |
| `onclick=` | `templates/components/nav_bar.html:18` | `#theme-toggle` → listener in `theme.js` |
| `onclick=` | `templates/components/nav_bar.html:29` | `#mobile-menu-toggle` → listener in `mybibli.js` |
| `onclick=` | `templates/components/catalog_toolbar.html:6` | `#audio-toggle` → listener in `audio.js` |
| `onclick=` | `templates/components/title_form.html:133` | `#title-form-cancel` → listener in `title-form.js` |
| `onclick=` | `templates/components/feedback_entry.html:27` | `data-action="dismiss-feedback"` → delegated listener in `mybibli.js` |
| `onclick=` | `templates/pages/borrowers.html:10` | `#borrowers-show-add-form` → listener in new `static/js/borrowers.js` |
| `onclick=` | `templates/pages/borrowers.html:41` | `#borrowers-hide-add-form` → same module |
| `onclick=` | `templates/pages/catalog.html:41` | replace with `hx-get`/`hx-target`/`hx-swap` (native HTMX) |
| `onclick=` | `templates/pages/home.html:90` | `data-browse-mode="list"` → listener in `browse-mode.js` |
| `onclick=` | `templates/pages/home.html:95` | `data-browse-mode="grid"` → same listener |
| `onclick=` | `templates/pages/loans.html:10` | `#loans-new-loan-toggle` → listener in `loan-borrower-search.js` |
| `onclick=` (×1 more) | `templates/fragments/title_edit_form.html` | locate via grep during Task 4, refactor to match `title_form.html` pattern |
| JS-generated `onclick=` | `static/js/mybibli.js:162` | emit `data-action="dismiss-feedback"`, handled by the same delegated listener |

**Totals:** 4 inline `<script>` + 1 inline `<style>` + 4 inline `style=` + 16 inline handlers + 1 JS-generated handler = **26 refactor sites**.

### CSP decisions & rationale

- **No nonces** — architecture §368 says "start strict without `unsafe-inline`, add nonces only if needed". Task 4's refactor removes the need; nonces cost per-request threading and add nothing once inline markup is gone.
- **`frame-ancestors 'none'` + `X-Frame-Options: DENY`** — belt-and-suspenders; older scanners still flag missing `X-Frame-Options`.
- **`connect-src 'self'`** — HTMX + `fetch()` are same-origin today. If a CDN fetch lands later, widen the directive explicitly.
- **`img-src` external allowlist** — `covers.openlibrary.org`, `books.google.com`, `image.tmdb.org`, `coverartarchive.org` (grep-confirmed provider hosts). Defensive only (see AC 12); do **not** remove.
- **`camera=()`** — min-surface default. Flip to `camera=(self)` only when 7.5 ships a webcam-based scanner.
- **Report-only** (`CSP_REPORT_ONLY=true`) — optional shipping-time opt-in to collect violations without blocking. Default **off**; production ships enforced.

### Infrastructure inventory

| Piece | Location | Status |
|-------|----------|--------|
| `tower-http` | `Cargo.toml:17` | ✅ present — no new dep needed for middleware |
| `axum::middleware::from_fn` | stdlib of axum 0.8 | ✅ |
| `tower::ServiceExt` (for tests) | `[dev-dependencies]` in `Cargo.toml:30` | ✅ |
| `regex` crate | not yet in `Cargo.toml` | ⚠️ add to `[dev-dependencies]` for Task 3 |
| `routes::build_router` | `src/routes/mod.rs:17-211` | ⚠️ wrap with `.layer(csp::csp_layer(...))` |
| `main.rs` router composition | `src/main.rs:124` | ✅ `logging::trace_layer()` applied here — do NOT move CSP here (keep it inside `build_router` so router-level unit tests cover it) |
| `AR26` env var pattern | `src/config.rs` | ✅ `std::env::var("CSP_REPORT_ONLY")` — no dotenvy |
| Existing JS modules | `static/js/*.js` | ✅ pattern is IIFE + init-on-DOMContentLoaded; new modules follow it |
| Tailwind `hidden` class | used throughout | ✅ `.hidden { display: none }` — swap for `style="display:none"` literally |
| CLAUDE.md | `/CLAUDE.md` | ⚠️ three small additions (AC 16) |

### Report-only E2E (AC 11, Test 4)

Two implementation paths:
1. **Docker compose override**: `docker-compose.test.yml` gets a second service `app-report-only` with `CSP_REPORT_ONLY=true` on a different port. Playwright config grows a second project pointing at that port.
2. **Skip with documentation**: the unit test (AC 9) covers the code path; the E2E delta is marginal. `test.skip("report-only covered by unit test — docker override not worth the maint cost", ...)`.

**Decision gate:** count the lines the override adds to `docker-compose.test.yml` + `playwright.config.ts`. If the combined diff is **≤ 20 lines**, implement #1. Otherwise implement #2. Record the line count + choice in Completion Notes so reviewers can verify.

### LLM-proofing traps

- **CSP is applied per response, not per-request** — middleware runs after the handler returns. Do not try to read request state to decide which headers to set. All routes get the same headers.
- **`script-src 'self'` blocks EVERY inline script, including `<script>/* comment */</script>`**. If Task 3's audit regex catches something that isn't functional code (e.g. a server-rendered `<script type="application/json">`), relax the regex to allow `type="application/json"` (which is not executable), not to allow all inline.
- **HTMX + CSP**: HTMX itself works fine under strict CSP. `hx-on` attributes would be blocked (they're inline event handlers); grep confirmed zero `hx-on=` in templates on 2026-04-16, but the templates audit test keeps it that way going forward.
- **`frame-src 'none'` vs `frame-ancestors 'none'`** — different directives. `frame-src` = what WE can frame (embed). `frame-ancestors` = who can frame US. Both are set; don't collapse them.
- **`X-Frame-Options: DENY`** — some reverse proxies strip headers starting with `X-`. If the production deployment sits behind a proxy (Traefik/Nginx on Synology), verify headers survive. Manual DevTools check in Task 9 catches this.
- **Refactored JS modules must be idempotent** — HTMX swaps can re-insert DOM nodes that the module already wired; double-wiring causes double-firing. Pattern: check a sentinel attribute like `data-wired="true"` before adding a listener, OR use event delegation at `document` level (preferred for newly-inserted nodes).
- **Askama `{% if %}style="..."{% endif %}` pattern** produces raw string output, not attribute-level conditionality. Replace with `class="base-classes {% if cond %}hidden{% endif %}"` — verify the `cond` inversion direction matches the original behavior (original: `{% if type_value != "closed" %}style="display:none"{% endif %}` — i.e., hidden when NOT closed. The `hidden` class replacement must carry the same polarity).
- **Permissions-Policy syntax is pedantic** — `camera=()` (empty allowlist) NOT `camera 'none'`. Parens required; the `self` token (when used) is bare, no quotes: `camera=(self)`. Test with `test_permissions_policy_denies_camera` in AC 9.
- **Do NOT set CSP via `<meta http-equiv>`** — header-based only. Meta-based CSP is weaker (no `frame-ancestors`, no `report-to`).
- **`cargo sqlx prepare --check` is green already** — this story adds no SQL. If it fails, something else changed; investigate rather than re-running `prepare`.
- **E2E console listener must be registered BEFORE `page.goto`** — Playwright misses early-load violations otherwise. Use `page.on("console", ...)` in `beforeEach`.
- **The templates audit test runs in the `rust-tests` CI job, not `e2e`** — it's pure Rust. Make sure the regex walk doesn't accidentally include `templates/_bmad-output/` or any generated content; scope it to `templates/`.

### Ordering within the story

Strict ordering matters to avoid breaking dev-loop page loads:
1. Task 1 — write middleware (not wired yet).
2. Task 3 — add audit test (fails loudly listing ~22 inline markup hits).
3. Task 4 — refactor templates + JS + CSS (makes audit pass).
4. Task 5 — wire new assets into `base.html`.
5. Task 6 — unit tests green.
6. Task 2 — wire CSP middleware into router (production-mode headers now active).
7. Task 7 — E2E tests on the now-active CSP.
8. Task 8 — docs.
9. Task 9 — quality gates.

If a developer tries Task 2 before Task 4, the next `cargo run` serves pages where every inline handler is blocked — a broken dev server. Don't.

### References

- Epic & AC: `_bmad-output/planning-artifacts/epics.md` Story 7.4 (lines ~997–1010); AR16 (line 205); NFR15 (line 158)
- PRD: `_bmad-output/planning-artifacts/prd.md` §NFR15 (line 821), §CSP decision (line 465)
- Architecture: `_bmad-output/planning-artifacts/architecture.md` §CSP Directives (lines 447-462), §Middleware Stack Order (lines 830-836 + 1055-1060 + 1108-1118), §Deferred nonces decision (line 368)
- UX: `_bmad-output/planning-artifacts/ux-design-specification.md` §scanner (UX-DR25) — relevant to `Permissions-Policy: camera`
- Previous stories (for middleware + template-plumbing patterns): `_bmad-output/implementation-artifacts/7-1-anonymous-browsing-and-role-gating.md`, `7-2-session-inactivity-timeout-and-toast.md`, `7-3-language-toggle-fr-en.md`
- CLAUDE.md: middleware pattern, AR26 no-dotenvy env var rule, E2E selector policy, `waitForTimeout` ban, zero-warning build rule, NFR34 static-asset budget

## Dev Agent Record

### Agent Model Used

claude-opus-4-6 (1M context) via bmad-dev-story workflow.

### Debug Log References

- Reset E2E DB volumes between full-suite runs to clear cross-spec data pollution (`docker compose down -v` + `up -d`); the failing borrowers / loans selectors cleared up immediately. Underlying behaviour is a known parallel-mode trade-off documented in story 5-1.
- HTMX `<style>` injection (hash `faU7yAF8…`) and HTMX `Function(...)` eval for trigger filters were the two hidden blockers — neither lives in our codebase. Disabled via `htmx-config: includeIndicatorStyles=false` and a `loan-scan-fire` CustomEvent rewrite respectively.
- Chromium under strict `style-src 'self'` blocks **runtime** `element.style.foo = …` writes, not just inline `style="…"` attributes. Refactored mybibli.js / search.js / theme.js to class toggles after seeing the `Applying inline style violates …` console events.

### Completion Notes List

- ✅ All 16 ACs satisfied. `src/middleware/csp.rs` ships the strict directive + 4 hardening headers; wiring is in `src/routes/mod.rs::build_router`; templates audit (`src/templates_audit.rs`) panics on regressions; E2E spec covers anonymous + authenticated + negative paths.
- ✅ AC 5 expanded scope: 4 backend HTML-builder hot spots (`feedback_html`, `scan_error_feedback_html`, `pending_updates`, `locations` tree) also carried inline `onclick=` / `<style>` / `style="…"` strings. Refactored to `data-action="dismiss-feedback"` (delegated handler) and CSS classes (`.shimmer-bar`, `.tree-indent-N`, `.tree-margin-N`, `.htmx-indicator`, `.feedback-fading`, `.htmx-opacity-reset`, `.theme-transitioning`, `.series-gap-stripes`) in browse.css.
- ✅ AC 11 Test 4 (report-only E2E) explicitly skipped with `test.skip(...)` — docker compose override would add >20 lines for a path the unit test covers; decision rationale recorded inline per Dev Notes "Report-only E2E" gate.
- ✅ Static asset budget after refactor: 216 KB total (NFR34 cap = 500 KB). Deleted inline `<script>` / `<style>` from rendered HTML pages outweighs the new browse.css + 3 JS modules.
- ⚠️ Existing test `test_skeleton_feedback_html_structure` updated to drop the `prefers-reduced-motion` substring assertion — that media query lives in browse.css now alongside `.shimmer-bar` keyframes (kept the `shimmer-bar` class assertion). Behaviour-preserving.
- ⚠️ Dockerfile updated to `COPY static/css/browse.css` into the runtime image — the existing CSS copy line only carried `output.css` (Tailwind-generated), so any new CSS file in `static/css/` needs an explicit Dockerfile entry until that's generalised.
- 🔁 3-cycle Playwright gate on a fresh docker stack: 144 passed / 1 skipped (Test 4 report-only) on every cycle. Manual `curl -sI http://localhost:8080/health` confirms the directive lands on every route (incl. `/static/*`, `/health`).

### File List

**Added**
- `src/middleware/csp.rs` — strict CSP + hardening headers middleware (5 unit tests)
- `src/templates_audit.rs` — regex-based regression gate (HTML-comment-stripped scan)
- `static/css/browse.css` — relocated styles + new utility classes (htmx-indicator, shimmer, tree indents, theme transition, feedback fade, opacity reset, series gap stripes)
- `static/js/title-form.js` — required-field validation + cancel + media-type lazy-load (delegated submit guard at body level, capture phase)
- `static/js/borrowers.js` — show/hide add-borrower form
- `static/js/loan-borrower-search.js` — borrower autocomplete + new-loan toggle + scan-field clear + post-return reload + `loan-scan-fire` Enter dispatcher
- `tests/e2e/specs/journeys/csp-headers.spec.ts` — spec ID `"CS"`, 3 live tests + 1 skipped (report-only)

**Modified**
- `src/lib.rs` — registers `templates_audit` test module under `#[cfg(test)]`
- `src/middleware/mod.rs` — re-exports `csp`
- `src/config.rs` — adds `csp_report_only()` env helper (no dotenvy, per AR26)
- `src/routes/mod.rs` — wires `apply_csp_layer` outermost in `build_router`
- `src/routes/borrowers.rs` — `borrower_detail` handler sets `current_page = "borrower-detail"` for the body[data-page] gate
- `src/routes/catalog.rs` — `feedback_html`, `scan_error_feedback_html`, `skeleton_feedback_html` rebuilt without inline handlers / `<style>`; updated existing unit test for the relocated keyframes
- `src/routes/locations.rs` — tree node renderer uses `tree-indent-N`/`tree-margin-N` CSS classes + `data-locations-toggle` (handler in mybibli.js)
- `src/middleware/pending_updates.rs` — async-metadata feedback OOB now emits `data-action="dismiss-feedback"` instead of inline `onclick`
- `Cargo.toml` — `regex = "1"` added to `[dev-dependencies]`
- `Dockerfile` — runtime stage now copies `static/css/browse.css`
- `templates/layouts/base.html` — links browse.css + the three new JS modules; adds `data-page="{{ current_page }}"` on `<body>`; adds `htmx-config` meta to disable the inline indicator styles
- `templates/layouts/bare.html` — adds the same `htmx-config` meta
- `templates/components/nav_bar.html` — `#theme-toggle` and `#mobile-menu-toggle` IDs replace inline `onclick` attributes
- `templates/components/catalog_toolbar.html` — removed inline `<script>`, `onclick`, `style="display:none"`
- `templates/components/title_form.html` — removed inline `<script>`, gave the form `id="title-create-form"`, gave the cancel button `id="title-form-cancel"`, removed `onchange` from media-type select
- `templates/components/feedback_entry.html` — dismiss button now uses `data-action="dismiss-feedback"`
- `templates/components/series_gap_grid.html` — `style="…"` → `.series-gap-stripes` class
- `templates/fragments/title_edit_form.html` — removed `onkeydown="…Escape…"`, gave the form `id="title-edit-form"` (handler delegated in mybibli.js)
- `templates/pages/home.html` — removed `<style>` block (moved to browse.css), `onchange` on sort dropdown (now `id="browse-sort-select"` + `data-base-q` + `data-active-filter` consumed by browse-mode.js), `onclick` on browse-mode buttons
- `templates/pages/catalog.html` — `onclick="htmx.ajax(…)"` swapped for native `hx-get` / `hx-target` / `hx-swap` attributes
- `templates/pages/borrowers.html` — show/hide add-form `onclick`s replaced with `id="borrowers-show-add-form"` / `id="borrowers-hide-add-form"` (handled by borrowers.js)
- `templates/pages/loans.html` — removed inline `<script>` (moved to loan-borrower-search.js), new-loan toggle now uses `id="loans-new-loan-toggle"`, scan-field switched from `hx-trigger="keydown[key=='Enter']"` to `hx-trigger="loan-scan-fire"`
- `templates/pages/borrower_detail.html` — removed inline `<script>` (moved to mybibli.js gated on `body[data-page="borrower-detail"]`)
- `templates/pages/series_form.html` — `onchange` removed (handler in mybibli.js), conditional `style="display:none"` swapped for conditional `hidden` class
- `templates/pages/title_detail.html` — `onchange` removed (handler in mybibli.js), inline `style="display:none"` swapped for `hidden` class
- `static/js/audio.js` — auto-wires `#audio-toggle` (idempotent + htmx:afterSettle re-bind), uses `.hidden` class instead of `.style.display`
- `static/js/theme.js` — wires `#theme-toggle` (idempotent), early-returns when button absent (bare.html), uses `.theme-transitioning` class instead of `.style.transition`
- `static/js/mybibli.js` — JS-generated dismiss button now emits `data-action`, adds delegated dismiss / mobile-menu-toggle / borrower-detail-reload / title-edit-Esc / omnibus-toggle / series-type-toggle / locations-tree-toggle handlers, uses `.htmx-opacity-reset` / `.feedback-fading` classes instead of inline style writes
- `static/js/browse-mode.js` — wires browse-mode buttons via delegation, wires sort dropdown via `data-base-q` / `data-active-filter`
- `static/js/scan-field.js` — JS-generated dismiss button now emits `data-action="dismiss-feedback"`
- `static/js/search.js` — error handlers use `.htmx-opacity-reset` class instead of `.style.opacity`
- `tests/e2e/specs/journeys/cross-cutting.spec.ts` — theme toggle selectors now use `#theme-toggle` instead of the obsolete `[onclick*='mybibliToggleTheme']` matcher
- `CLAUDE.md` — three additions per AC 16

### Change Log

- 2026-04-16 — Story 7-4 created from epics.md AC. Status → ready-for-dev. Grep-confirmed inline-markup inventory (4 scripts + 1 style block + 4 style attrs + 16 handlers + 1 JS-generated handler) included in Dev Notes.
- 2026-04-16 — Implementation complete. CSP middleware shipped, 26 inline-markup sites refactored (+ 4 backend HTML-builder hot spots, + HTMX indicator-style / trigger-eval workarounds, + JS runtime-style writes converted to class toggles). 5 new CSP unit tests, 1 templates audit test, 1 new E2E spec. 3-cycle Playwright gate green (144 passed / 1 skipped). Status → review.
- 2026-04-16 — Code review pass (3 layers: Blind Hunter, Edge Case Hunter, Acceptance Auditor). 5 patches applied: case-insensitive `CSP_REPORT_ONLY` + startup log (+ 6 unit tests), AC 12 cover-image `naturalWidth > 0` proof in Test 2, tightened static/covers test to assert exact directive, `.htmx-opacity-reset` cleanup on `htmx:beforeRequest`, AC 7 `/login` console-clean assertion in Test 1. 18 deferred to `deferred-work.md`. 441 lib tests green, 3-cycle Playwright gate green (144 passed / 1 skipped). Status → done.
