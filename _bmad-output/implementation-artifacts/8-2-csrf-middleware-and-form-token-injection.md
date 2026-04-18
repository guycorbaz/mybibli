# Story 8.2: CSRF middleware and form-token injection

Status: ready-for-dev

Epic: 8 — Administration & Configuration
Requirements mapping: NFR13 (role isolation — defense-in-depth), NFR15 (strict-CSP defense chain), Epic 7 retro §7 Action 1 (closure), Foundation Rules #1 / #2 / #3 (DRY + unit + E2E coverage), AR15 (session semantics preserved)

---

> **TL;DR** — Closes the five-deferral CSRF debt. Ships (1) a **synchronizer-token** middleware keyed on a new `sessions.csrf_token` column (migration backfills via `HEX(RANDOM_BYTES(32))`, no heal-on-read race); (2) a **`base_context()` helper** that injects `csrf_token` into every full-page template struct (~15-18 structs, fragment templates skipped); (3) a **template-audit-enforced** hidden-input pattern for every `<form method="POST">`; (4) a **`static/js/csrf.js`** classic script with two listeners — `htmx:configRequest` (injects `X-CSRF-Token` header) and `htmx:beforeSwap` (force-swaps the server-rendered 403 feedback body into `#feedback-list` via `HX-Retarget`/`HX-Reswap`); (5) a patch to `static/js/session-timeout.js`'s `fetch()` fallback so it carries the token; (6) **rewires the nav-bar logout from GET-anchor to POST-form** (closes the `<img src="/logout">` surface flagged in Epic 7 retro); (7) **lazy anonymous-session rows** (first-hit INSERT) + **7-day daily purge task** to keep the `sessions` table bounded. Only exemption is `POST /login` (no session exists yet — `SameSite=Lax` handles login-CSRF). The middleware slots into the AR16 layer order between Auth and the handlers: `Logging → Auth → **CSRF** → [Handler] → PendingUpdates → CSP`. Foundation for every admin-mutation surface in 8-3..8-8.
>
> **Change log:**
> - 2026-04-18 (create-story): initial draft
> - 2026-04-18 (validate-story): 7 critical + 5 enhancement patches applied — drop `user_id` ALTER (no-op; would have been destructive), migration-time backfill (closes empty-token race), `csrf.js` rewritten without `window.i18n`/`window.mybibli`/`type="module"` (none existed), explicit empty-token rejection in middleware, `/session/keepalive` + `/debug/session-timeout` coverage clarified, `session-timeout.js` `fetch()` fallback patch added, anonymous-session-purge task added, `HX-Trigger` documented as NEW idiom, multipart caveat removed, template-blast-radius clarified (~15-18 full-page structs, not 29).

## Story

As the **project maintainer**,
I want every state-changing request to require a session-bound CSRF token using the synchronizer pattern,
so that cross-site requests from hostile pages cannot trigger logout, language toggle, or any Epic-8 admin mutation against an authenticated browser — closing the fifth-deferred security commitment before the admin-mutation stories begin (8-3 user admin, 8-4 reference data, 8-5 system settings, 8-6 trash restore, 8-7 permanent delete, 8-8 setup wizard).

## Scope Reality & What This Story Ships

**CSRF has been deferred 5× across Epics 1 → 7.** Story 1-9 (minimal-login) deferred it with the note *"CSRF tokens (deferred — noted in deferred-work.md)"*. Story 7-1 review carved it out again. Story 7-3 (language toggle) documented it in the handler docstring (`src/routes/auth.rs` lines 247-250) and left it unimplemented. Epic 6 retro §3 Action 1 was *"land CSRF with first auth-touching story"* — missed. Epic 7 retro §4 bullet 3 names it **"chronic deferral"**. Epic 7 retro §7 Action 1 was written with a hard deadline: *"Fifth deferral is not available."* This story is the implementation path (vs the alternative of `docs/auth-threat-model.md` formally accepting the risk, per Epic 8 kickoff decision 2026-04-18).

**Current security posture (what is already in place):**
- Session cookie: `HttpOnly`, `SameSite=Lax`, `Path=/`, no `Max-Age` (session-cookie). Set in `src/routes/auth.rs::login` lines 173-177. (Architecture docs still say `SameSite=Strict` — they're stale; actual code is `Lax`, changed to let the language toggle survive a same-site top-level POST.)
- Strict CSP from story 7-4 (`src/middleware/csp.rs`) — zero inline `<script>` / `<style>` / `onclick=` / `style="..."`. The templates-audit test (`src/templates_audit.rs`) enforces this at `cargo test` time.
- Scanner-guard on dialog/aria-modal surfaces from story 7-5.
- Route-role matrix at `docs/route-role-matrix.md` (58 handlers from story 7-1) — needs a new column for "CSRF-exempt?" after this story.

**Current CSRF surface — 24 mutation points across 17 templates** (grep `method="POST"` + `hx-(post|put|delete|patch)` in `templates/`) plus any HTMX `hx-post` in `.rs` code-generated HTML (check `feedback_html`, `pending_updates`, `locations` tree, `admin_*_panel.html` fragments). `GET /logout` is additionally a mutation (session delete on GET) — the `<a href="/logout">` link in `templates/components/nav_bar.html:42` is the load-bearing example flagged in Epic 7 retro §5. This story converts it to a POST form.

**Ships:**
1. **Migration: `migrations/20260418000000_add_csrf_token_to_sessions.sql`** — two statements in one file:
   - `ALTER TABLE sessions ADD COLUMN csrf_token VARCHAR(64) NOT NULL DEFAULT ''`
   - `UPDATE sessions SET csrf_token = LOWER(HEX(RANDOM_BYTES(32))) WHERE csrf_token = ''` — backfill at migration time using MariaDB's `RANDOM_BYTES()` (available since MariaDB 10.10 — the project's MariaDB version is ≥ 10.10 per `tests/docker-compose.rust-test.yml`; verify). `HEX(RANDOM_BYTES(32))` produces a 64-char hex string that fits the column. Rust-side tokens are 43-char base64; the two encodings coexist because constant-time compare is byte-level and `csrf_middleware` never cares about format. **Critical: the middleware MUST reject any request where the session's stored `csrf_token` is empty** — belt-and-braces against a row that somehow escapes the backfill. Migration is destructive-to-empty-only; existing sessions keep working with a fresh token.
   - Rationale for migration-time backfill (vs heal-on-read): closes a race window where a pre-existing session with `csrf_token=''` could — between deploy and first read — accept a forged request whose `X-CSRF-Token: ""` matches the stored empty string via `ConstantTimeEq`. Backfill at migration means no session row ever carries `''` in production.

2. **`src/middleware/csrf.rs`** (new module). Exports:
   - `pub const CSRF_EXEMPT_ROUTES: &[(&str, &str)] = &[("POST", "/login")];` — single entry, frozen by `src/templates_audit.rs` (below).
   - `pub async fn csrf_middleware(...) -> Response` — layered via `axum::middleware::from_fn_with_state`. Checks request method; for GET / HEAD / OPTIONS returns straight to handler. For state-changing methods:
     (1) Resolve expected token from `Session` extension (populated by the auth layer that runs before CSRF per AR16).
     (2) Pull token from request: `X-CSRF-Token` header wins; fallback `_csrf_token` form field (requires reading the body — use `axum::body::Bytes` buffered then re-attach; limit to `MAX_CSRF_BODY_BYTES = 1 MiB` to match Axum's default POST body limit — larger bodies (rare on mutation forms) return 413 before we even look at CSRF).
     (3) Constant-time compare via `subtle::ConstantTimeEq` (new dep — `subtle = "2.6"`). Use the `subtle` crate because `==` on strings is not constant-time and CSRF tokens are short enough that timing attacks are a stated concern.
     (4) On mismatch / missing: return `AppError::Forbidden` with a `HX-Trigger: csrf-rejected` header so the existing FeedbackEntry + client HTMX listener can show a UX-sane "Session expired — please refresh the page and retry" message (i18n keys below).
   - `pub fn generate_csrf_token() -> String` — 32-byte `rand::random::<[u8; 32]>()` encoded as URL-safe base64 (same entropy as session token per NFR10 analogy). Token length post-encoding: 43 chars.

3. **`src/routes/auth.rs::login`** — line 157 currently generates only the session token. Extend to generate a `csrf_token` alongside and persist both in the same INSERT:
   ```sql
   INSERT INTO sessions (token, user_id, csrf_token, data, last_activity)
   VALUES (?, ?, ?, '{}', UTC_TIMESTAMP())
   ```
   No behavior change to cookie handling — the CSRF token lives server-side, never in a cookie (it's a synchronizer-token, not double-submit).

4. **`src/routes/auth.rs::logout`** — drop the `axum::routing::get(auth::logout)` line in `src/routes/mod.rs:88` (currently `.route("/logout", axum::routing::get(auth::logout).post(auth::logout))`). After this change GET `/logout` returns 405 Method Not Allowed from Axum's router natively. No handler change — `logout` already handles POST.

5. **`src/middleware/auth.rs::Session`** — add `pub csrf_token: String` to the `Session` struct (next to `token`, `user_id`, `role`, `preferred_language`). Anonymous sessions get a fresh token too (see point 6 below). `Session::anonymous()` now requires a caller-provided token — change to `Session::anonymous_with_token(csrf_token: String)` constructed by the auth middleware, which either (a) reads the session row and reuses `csrf_token`, or (b) mints one for a first-hit anonymous visitor and persists it via a lazy anonymous-session INSERT (see point 6).

6. **Lazy anonymous session row** — currently `Session::anonymous()` returns a purely in-memory value with no DB row (verified: `src/middleware/auth.rs:96-98` / `:126`). To give anonymous visitors a CSRF token that survives across their anonymous requests (so anonymous `/language` POSTs and anonymous `/login` forms both work), create a session row on the **first GET from a browser with no `session` cookie**, with `user_id = NULL` and a fresh `csrf_token`. The cookie is set as an anonymous-session cookie (still `HttpOnly`, `SameSite=Lax`, no `Max-Age`). On login, the handler soft-deletes the anonymous row and INSERTs a fresh authenticated one (simpler than in-place UPDATE — the anonymous row becomes garbage and the daily purge — see below — removes it).

   **Pre-verified schema facts (confirmed 2026-04-18):** `sessions.user_id` is ALREADY `BIGINT UNSIGNED NULL` since the initial migration (`migrations/20260329000000_initial_schema.sql:224`). **No `ALTER user_id` migration needed** — any such ALTER would be destructive (would change the type from `BIGINT UNSIGNED` → `INT` and silently break the FK `fk_sessions_user`, same file line 234). Task 1 in §Tasks reflects this: schema migration is the `csrf_token` column only.

   **Anonymous session lifecycle + purge (NEW — previously implicit):**
   - Anonymous session rows accumulate as crawlers / drive-by scrapers visit the site. Left unbounded, the `sessions` table grows without feedback loop.
   - Add a daily purge task: `DELETE FROM sessions WHERE user_id IS NULL AND last_activity < UTC_TIMESTAMP() - INTERVAL 7 DAY`. Anonymous visitors who return after 7 days of absence simply get a fresh anonymous session row on their next hit — no user-visible impact.
   - The purge runs in the existing background-task scaffold (see `src/tasks/`). Reuse the `tokio::spawn` + `tokio::time::interval(24h)` idiom already used by `src/tasks/provider_health.rs` (story 8-1). New file: `src/tasks/anonymous_session_purge.rs`.
   - Authenticated sessions are **NOT** affected by this purge — the existing session-timeout (inactivity 4h default, story 7-2) already soft-deletes inactive authenticated rows.
   - GDPR posture: anonymous rows carry `user_id = NULL`, cookie token, `csrf_token`, timestamps. No PII. The 7-day purge satisfies "do not retain beyond necessary" for the narrow purpose of CSRF-token continuity across anonymous POSTs.

7. **`AppError::Forbidden`** — already exists, already renders FeedbackEntry. Confirm the existing `IntoResponse` impl emits a 403 status. The CSRF middleware's rejection path adds a `HX-Trigger: csrf-rejected` header on top of the existing response — use `.headers_mut().insert("HX-Trigger", HeaderValue::from_static("csrf-rejected"))` on the `Response` built from `AppError::Forbidden`.

8. **`static/js/csrf.js`** (new module). Registered in `layouts/base.html` alongside the other 6 JS modules (scan-field, feedback, audio, theme, focus, scanner-guard, mybibli). **Classic `<script src>`, NOT `type="module"`** — matches the existing module-loading convention (`base.html` currently loads all 6 scripts as classic, and mixing a module with classic scripts would perturb execution order).

   Behavior — exactly two listeners, nothing else:

   ```js
   // csrf.js (public shape; implementation mirrors session-timeout.js IIFE pattern)
   (function () {
       // Listener 1 — token injection. Covers every HTMX-driven mutation
       // (hx-post / hx-put / hx-patch / hx-delete) in the app.
       document.body.addEventListener("htmx:configRequest", function (evt) {
           var meta = document.querySelector('meta[name="csrf-token"]');
           if (meta) evt.detail.headers["X-CSRF-Token"] = meta.getAttribute("content");
       });

       // Listener 2 — force-swap the 403 feedback body into the page so the
       // user sees the server-rendered error without inventing any client-side
       // i18n. The middleware emits `HX-Retarget: #feedback-list` + `HX-Reswap:
       // beforeend` + `HX-Trigger: csrf-rejected` on 403; we opt-in to the
       // swap despite HTMX's default "don't swap on error" behaviour.
       document.body.addEventListener("htmx:beforeSwap", function (evt) {
           var xhr = evt.detail.xhr;
           if (xhr && xhr.status === 403 && xhr.getResponseHeader("HX-Trigger") === "csrf-rejected") {
               evt.detail.shouldSwap = true;
               evt.detail.isError = false; // let HTMX treat the body as a normal swap payload
           }
       });
   })();
   ```

   Design decisions captured above:
   - **No `window.i18n` access** — the pattern does NOT exist in this codebase (verified via `grep`; every JS module declares its own local `var i18n = {...}` block, e.g. `session-timeout.js:58-71`). The feedback body the middleware returns is already server-rendered in the user's language via `rust_i18n::t!()`, so `csrf.js` does NO string work — it just forces the DOM swap.
   - **No `window.mybibli.appendFeedback` call** — that API does not exist (`mybibli.js` exposes no public `window.mybibli` object; it manipulates `.feedback-entry` DOM nodes directly). Force-swapping the server-rendered 403 body into `#feedback-list` via `HX-Retarget` + `HX-Reswap: beforeend` achieves the same UX without inventing a new JS API.
   - **`fetch()` fallback in `static/js/session-timeout.js`** — the existing keep-alive path (`session-timeout.js:97-101`) uses `htmx.ajax(...)` when HTMX is loaded, else `fetch("/session/keepalive", { method: "POST" })`. The HTMX path is covered by listener 1 above (HTMX fires `htmx:configRequest` on `htmx.ajax` too). **The bare `fetch()` branch is NOT covered**, so this story patches `session-timeout.js` to set `X-CSRF-Token` manually in the fallback branch — see Task 6.6.

9. **`templates/layouts/base.html`** — add a single `<meta name="csrf-token" content="{{ csrf_token|e }}">` tag inside `<head>`. Use Askama's `|e` escape filter (already on several places in the codebase). This is the one HTML output of the token to the client; form-hidden-inputs reference the SAME value via the same template variable.

10. **`BaseContext` / every page-template struct** — add a `csrf_token: String` field to every struct in `src/routes/*.rs` that implements `askama::Template`. The field is populated from `session.csrf_token`. This is mechanical but repetitive — add a helper `base_context(&session) -> BaseContextFields` that returns the common fields (`lang`, `role`, `current_page`, `skip_label`, `nav_*`, `csrf_token`) and call it from every page handler. **This helper should NOT become a god-object** — if `BaseContextFields` grows beyond what the common templates actually read, split it. For this story the 7 existing common fields plus `csrf_token` is exactly what `base.html` + `nav_bar.html` need.

   Alternative considered and rejected: populate `csrf_token` via an Axum extractor that the Askama template reads from an Extension. Askama does not have access to Axum extensions — it renders from a struct the handler builds. A dedicated helper is the DRY move.

11. **Every `<form method="POST">` template in `templates/`** gets a hidden input immediately after the opening `<form>` tag:
    ```html
    <input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">
    ```
    17 templates x 24 form instances (current count per `templates/` grep). Each needs the field. **Enforcement is delegated to `src/templates_audit.rs`** — see point 12.

12. **`src/templates_audit.rs` extension** — add a test `forms_include_csrf_token`: walk `templates/`, regex-match every `<form method="(post|POST)"...>` and assert the very next occurrence of `<input` inside the same form carries `name="_csrf_token"` or `name='_csrf_token'`. Panics on regression. Complementary to the CSP-guard test already there. Also add `csrf_exempt_routes_frozen`: parse `src/middleware/csrf.rs` source, extract `CSRF_EXEMPT_ROUTES`, assert its length is exactly 1 and its single entry is `("POST", "/login")`. Growing the allowlist requires deliberate removal of this guard — review signal, same frozen-allowlist pattern as `hx-confirm` (story 7-5).

13. **HTMX mutation sites in Rust-generated HTML** — check `src/utils.rs::feedback_html`, `src/middleware/pending_updates.rs`, and any `format!("<form ...")` in route modules. If any of them render `hx-post` / `hx-delete` etc., the JS `htmx:configRequest` listener covers the header side (no template change needed). But if any of them render server-side POST forms (without HTMX), they need the hidden input too. Grep: `format!.*<form.*method.*post` across `src/` → expected zero hits; confirm, add to Dev Notes as a pre-flight check. (Multipart/file-upload: verified 2026-04-18 that no route accepts multipart — `grep -i "multipart" src/` returns zero hits; covers are fetched server-side, not uploaded. So the middleware's body-buffer limit of 1 MiB does not impact any existing route.)

14. **Nav-bar logout** — `templates/components/nav_bar.html:42` becomes (desktop and mobile variants):
    ```html
    <form method="POST" action="/logout" class="contents">
      <input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">
      <button type="submit" class="text-stone-600 dark:text-stone-400 hover:text-stone-900 dark:hover:text-stone-100 bg-transparent border-none p-0 cursor-pointer">{{ nav_logout }}</button>
    </form>
    ```
    Tailwind `display: contents` (form `class="contents"`) makes the form element disappear from the box-model — the button inherits the parent flex/inline layout directly, so the logout link stays visually indistinguishable from siblings (no width/gap/alignment shift). The `bg-transparent border-none p-0 cursor-pointer` on the button strips default browser button styling (browsers render `<button>` with grey backgrounds and borders that would otherwise break the nav strip). Inline `style=` is CSP-blocked (story 7-4), so all of this is plain class-based. Visual regression check via a Playwright screenshot diff is part of Task 9.

15. **i18n keys — server-side only** (JS touches no strings in this story; see §Ships 8 rationale):
    - `error.csrf_rejected_title`: EN `"Session expired"` / FR `"Session expirée"` — rendered as the FeedbackEntry heading.
    - `error.csrf_rejected_message`: EN `"Your CSRF token is missing or expired. Please refresh the page and retry your action."` / FR `"Votre jeton CSRF est manquant ou expiré. Actualisez la page et réessayez."` — rendered as the FeedbackEntry body.

    The CSRF middleware builds the 403 response body by calling `feedback_html("error", &t!("error.csrf_rejected_title", locale = loc), &t!("error.csrf_rejected_message", locale = loc))` — same helper used everywhere else in the codebase (DRY per Foundation Rule #1). Locale resolved via the existing `middleware::locale::Locale` extension.

    **Remember:** `touch src/lib.rs && cargo build` after YAML edits (rust-i18n proc macro re-read).

16. **Unit tests (new, `src/middleware/csrf.rs::tests`):**
    - `generate_csrf_token` produces a 43-char URL-safe base64 (32 bytes random).
    - Two calls produce distinct tokens.
    - Middleware on GET → calls handler, never checks token.
    - Middleware on HEAD / OPTIONS → same.
    - Middleware on POST with matching header → calls handler.
    - Middleware on POST with matching form field (no header) → calls handler.
    - Middleware on POST with BOTH header and form field mismatching → 403 (header wins, so this hits the header-mismatch path).
    - Middleware on POST with neither → 403.
    - Middleware on POST with mismatch only → 403; assert ALL THREE response headers: `HX-Trigger: csrf-rejected`, `HX-Retarget: #feedback-list`, `HX-Reswap: beforeend`.
    - **Empty-token guard (critical per validation):** Middleware on POST where `session.csrf_token == ""` returns `AppError::Internal`, NOT 403 and NOT a match against the empty client value. Asserts the internal-error path is taken even when client sends `X-CSRF-Token: ""`.
    - Constant-time compare (smoke test: feed a loop of tokens varying from prefix-match to full-match to no-match, assert `ConstantTimeEq` returns the correct bit without panicking).
    - Exempt-route allowlist bypass: `POST /login` with no token → handler is called (the wrapping router has the CSRF layer, but the middleware checks the `(method, path)` tuple against the frozen allowlist before validation).
    - `CSRF_EXEMPT_ROUTES.len() == 1` and the single entry is `("POST", "/login")` — same assertion as `src/templates_audit.rs::csrf_exempt_routes_frozen` but co-located with the middleware for defense in depth.

17. **Integration tests (new, `tests/csrf_integration.rs`, uses `#[sqlx::test]`):**
    - Login persists a valid CSRF token in `sessions.csrf_token` (non-empty, 43+ chars).
    - POST `/logout` without the token → 403; with the token → 303 to `/`.
    - POST `/language` without the token → 403; with the token → 303 + cookie set (preserves all story 7-3 behaviors).
    - POST `/session/keepalive` without the token → 403; with the token → 200 (or whatever the current success status is — verify).
    - POST `/login` with no token (and no session) → handler runs; on success the new session row has a fresh CSRF token; the old anonymous row is soft-deleted.
    - Token rotation on re-login: submit two `/login` POSTs back-to-back from the same browser (same cookie), assert the second session has a distinct `csrf_token` and the first token no longer validates on any subsequent mutation.
    - GET `/logout` → 405 Method Not Allowed (router no longer wires the GET method).
    - Anonymous first-hit: clear cookies, GET `/catalog` → response sets a `session` cookie; the DB now has a row with `user_id=NULL` and `csrf_token != ''`; the rendered HTML contains the `<meta name="csrf-token" content="...">`; a subsequent POST `/language` with that token succeeds.
    - Migration backfill: after running migration on a DB pre-populated with 5 session rows, `SELECT COUNT(*) FROM sessions WHERE csrf_token = ''` returns 0, and each row has a distinct `csrf_token` value (no duplicates from RAND collision).
    - Anonymous session purge: seed 3 anonymous rows with `last_activity = NOW() - INTERVAL 8 DAY` + 2 with `NOW() - INTERVAL 3 DAY` + 1 authenticated with `NOW() - INTERVAL 8 DAY`; run the purge; assert 3 rows deleted (only the old anonymous ones), 2 anonymous + 1 authenticated remain.

18. **E2E test (`tests/e2e/specs/security/csrf.spec.ts`, spec ID `"CS"`):**
    - **Smoke path (Foundation Rule #7 for this story):** blank browser → login admin → navigate to `/catalog` → grab `meta[name=csrf-token]` content via `page.locator('meta[name="csrf-token"]').getAttribute('content')` → submit the language toggle form with the valid token → assert 303 + cookie set. Mutate the token in DevTools via `page.evaluate((bad) => document.querySelector('meta[name=csrf-token]').setAttribute('content', bad), 'bogus')` → retry the toggle → assert the response is 403 and the on-page FeedbackEntry says "Session expired" (i18n-aware regex per CLAUDE.md). Confirm the `HX-Trigger: csrf-rejected` path fired by checking the FeedbackEntry appeared without a full-page reload (HTMX swap).
    - **Logout flow:** navigate to the nav-bar "Logout" button (not link — it's now a form-submit); click → assert POST fires (check via `page.waitForResponse`) → assert redirect to `/`. Then `page.goto('/logout')` (bare GET) → assert response status === 405.
    - **Stripped-token admin:** log in as admin, navigate to `/admin`, grab a valid-session cookie jar, then `page.request.post('/admin/health/something-mutating-later', { headers: {} })` (no X-CSRF-Token) → 403. (This test is future-proofing for 8-3..8-8 which will introduce admin mutations; for 8-2 itself there are no admin mutation endpoints yet — swap this assertion for the language toggle as a proxy if needed.)
    - **i18n coverage:** run the smoke path in FR (`loginAs(page)` defaults to admin; set `lang=fr` cookie first) → assert the FeedbackEntry text matches the FR key.
    - **Data isolation:** spec uses `specIsbn("CS", ...)` for any ISBNs scanned; no shared V-codes (the mutations in this spec are language + logout, not scan-based, so this is a no-op but keep the pattern).

19. **Documentation updates:**
    - `CLAUDE.md` "Key Patterns" gains a new bullet:
      > **CSRF token (story 8-2):** Every session row carries `csrf_token` (synchronizer-token pattern). Every page template receives `csrf_token: String` via the shared `base_context()` helper. `<form method="POST">` templates include `<input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">`; HTMX requests carry `X-CSRF-Token` set by `static/js/csrf.js` on the `htmx:configRequest` event from `<meta name="csrf-token">`. Middleware `src/middleware/csrf.rs` validates on every state-changing method (POST/PUT/PATCH/DELETE) and rejects with 403 + `HX-Trigger: csrf-rejected` on mismatch. Exempt routes: `POST /login` only (no session exists yet). Frozen allowlist enforced by `src/templates_audit.rs::csrf_exempt_routes_frozen`.
    - `_bmad-output/planning-artifacts/architecture.md` Authentication & Security section: add a short "CSRF Synchronizer Token" subsection describing the token source-of-truth (`sessions.csrf_token`), the 32-byte entropy (matches NFR10 session-token analogy), and the layer order `Logging → Auth → CSRF → [Handler] → PendingUpdates → CSP`. Also correct the existing `SameSite=Strict` language (it's `Lax` in code since story 7-3) — pre-existing drift, worth fixing while in the file.
    - `docs/route-role-matrix.md`: add a `csrf_exempt` column; exactly one row is marked "yes" (`POST /login`).

**Does NOT ship:**
- **Per-request token rotation.** Token stays stable within a logged-in session; rotates only on login / logout / re-login. Per-request rotation would force HTMX clients to parse response meta refreshes on every request and would burn UX / complicate `htmx:configRequest` for minimal threat-model benefit on a single-user NAS.
- **Double-submit cookie pattern.** We already persist sessions server-side — synchronizer-token is strictly simpler and avoids a second cookie. Documented in Dev Notes §Approach rationale.
- **CSRF for GET side-effects.** There are no intentional GET side-effects after this story (we're removing `GET /logout`). If a future GET handler becomes state-changing, it's a separate bug to fix in that story — out of scope here.
- **Origin / Referer validation.** These headers can be spoofed/absent depending on browser + privacy settings. The synchronizer token is the authoritative check. (If we later want defense-in-depth, add Origin validation as an optional belt-and-braces layer in a follow-up.)
- **CSRF protection for the `/healthz`-style unauthenticated JSON endpoints.** `routes::mod::health_check` at `/health` returns a static `"ok"` string for Docker healthcheck — it's GET-only and carries no state; no CSRF applicable.
- **Replacement of `axum_extra::extract::Form`.** Forms continue to deserialize with the existing extractor; the CSRF middleware buffers the body once for form-field fallback and re-injects it for the handler. (If this proves to be a performance hotspot — unlikely on a single-user NAS — we can optimize in a follow-up.)
- **A crate dependency on `axum_csrf` or `tower-csrf`.** Evaluated and rejected — both pull in cookie-based double-submit semantics that would conflict with our server-stored synchronizer approach; their API surface is also Axum-0.7-era and would drag along ecosystem noise. Rolling the ~80 lines of middleware ourselves is the cleaner move.

## Cross-cutting decisions this story depends on

**Confirmed at Epic 8 kickoff (2026-04-18):** Guy selected option 1.1 ("ship CSRF now, do not defer again") over option 1.2 ("write the threat-model doc accepting the risk"). Confirmed via the sprint-status exchange at the start of this session.

**Frozen allowlist pattern:** Mirrors story 7-5's `hx-confirm=` allowlist frozen at 5 grandfathered sites. Same enforcement mechanism (`src/templates_audit.rs` integration test). Same review signal — adding a new exempt route requires a PR that visibly edits the constant AND the test.

**SameSite=Lax is a necessary condition for login-CSRF safety, not a sufficient one for mutation-CSRF safety.** `SameSite=Lax` on the session cookie means cross-site top-level GET navigations carry the cookie (needed for post-login redirects and email-link flows), but cross-site POSTs do NOT. This is exactly the mitigation for login-CSRF. It is NOT sufficient for admin-mutation CSRF because a same-site page (e.g., an XSS-injected blob on a future BnF-metadata response — currently CSP-prevented, but defense-in-depth) could still POST with the cookie attached. The synchronizer token closes that gap.

## Acceptance Criteria

1. **Migration: `sessions.csrf_token` column exists and every row is backfilled.**
   - File: `migrations/20260418000000_add_csrf_token_to_sessions.sql`. Two statements: `ALTER TABLE sessions ADD COLUMN csrf_token VARCHAR(64) NOT NULL DEFAULT ''` followed by `UPDATE sessions SET csrf_token = LOWER(HEX(RANDOM_BYTES(32))) WHERE csrf_token = ''`.
   - Verify via `SHOW COLUMNS FROM sessions` in a fresh DB — `varchar(64), NO (NULL), ''` for (Type, Null, Default).
   - Verify via `SELECT COUNT(*) FROM sessions WHERE csrf_token = ''` post-migration — exactly zero.
   - MariaDB `RANDOM_BYTES()` requires MariaDB ≥ 10.10 — confirm against `tests/docker-compose.rust-test.yml` target version before merge; if the prod image pins an older version, substitute `SUBSTRING(MD5(CONCAT(RAND(), token, UUID())), 1, 43)` (weaker entropy, acceptable one-off for backfill only — still 172 bits).
   - No additional index on `csrf_token` — the middleware reads it alongside the session row (already indexed on `token`), no join path needs the extra index.

2. **No migration on `sessions.user_id` — pre-verified already nullable.**
   - Current schema: `migrations/20260329000000_initial_schema.sql:224` declares `user_id BIGINT UNSIGNED NULL`. Confirmed via file read 2026-04-18.
   - FK constraint `fk_sessions_user` (same file line 234) has no explicit `ON DELETE` clause — default `RESTRICT`. Anonymous sessions (`user_id=NULL`) are exempt from FK enforcement (standard MySQL/MariaDB FK behavior with NULL).
   - Any `ALTER` against `user_id` in this story is **forbidden** — it would change the type from `BIGINT UNSIGNED` to `INT` and silently break the FK.

3. **Session row lifecycle covers anonymous and authenticated flows.**
   - Given a request with no `session` cookie, when the auth middleware runs, then it INSERTs an anonymous session row (`user_id = NULL`, `token` = new session token, `csrf_token` = new CSRF token), sets the `session` cookie, and the request continues as anonymous. **Idempotent under concurrency:** use `INSERT` with unique constraint on `token` + random 32-byte value → collision probability negligible (2^-128); if two concurrent first-hits from the same browser both generate DIFFERENT tokens, the server writes two rows but only the latest cookie wins on the client — the other row is orphaned and the daily purge (below) collects it.
   - Given a request with a valid anonymous-session cookie, when the user POSTs `/login` successfully, then: (a) the anonymous session row is soft-deleted (`deleted_at = NOW()`), (b) a new authenticated session row is INSERTed with a fresh `csrf_token`, (c) the `session` cookie is overwritten with the new token. The CSRF token rotates on login.
   - Given POST `/logout`, when it succeeds, then the authenticated session row is soft-deleted (existing behavior) and no new row is created — next GET request is a fresh anonymous-session INSERT.
   - Given the daily purge task runs (new, per §Ships 6), when it fires, then `DELETE FROM sessions WHERE user_id IS NULL AND last_activity < UTC_TIMESTAMP() - INTERVAL 7 DAY` executes. Authenticated session rows are NOT touched (they are managed by story 7-2's inactivity timeout). Logged at `tracing::info!(rows_deleted = n, "anonymous session purge completed")`.

4. **Session extractor propagates `csrf_token` into the `Session` struct.**
   - `src/middleware/auth.rs::Session` gains `pub csrf_token: String`.
   - `SessionModel::lookup_by_token` returns the token along with the other fields.
   - `Session::anonymous_with_token(csrf_token: String)` replaces the parameter-less `Session::anonymous()` call sites. The three existing call sites (audit via `grep -rn "Session::anonymous" src/`) are updated; if any call site does NOT have a readily available token, that's a bug — the auth middleware should have minted one before anyone called `Session::anonymous_with_token`.

5. **Middleware `csrf_middleware` validates state-changing requests.**
   - Wired in `src/routes/mod.rs::build_router` between the auth middleware and the handlers, per AR16 updated layer order `Logging → Auth → CSRF → [Handler] → PendingUpdates → CSP`.
   - Given a GET / HEAD / OPTIONS request, when the middleware runs, then it passes through unconditionally.
   - Given a state-changing request with no body + no header, when middleware runs, then `AppError::Forbidden` is returned.
   - Given a state-changing request with `X-CSRF-Token` header matching `session.csrf_token` (constant-time), when the middleware runs, then the handler is called; the request body is unmodified.
   - Given a state-changing request with no header but `_csrf_token` form field matching, when the middleware runs, then the handler is called; the body is buffered once and re-attached to the request (Axum's `Request::from_parts` + body re-injection pattern).
   - Given a state-changing request with both header AND form field, when the middleware runs, then the header wins; if header mismatches, 403 is returned regardless of form field value.
   - Given the session's stored `csrf_token` is empty string (should never happen post-migration; defense in depth), when the middleware runs on a state-changing request, then it returns `AppError::Internal("session CSRF token unset")` — NOT a 403 match against `""`. This closes the heal-on-read race identified in validation.
   - Given the request's `(method, path)` tuple is present in `CSRF_EXEMPT_ROUTES`, when the middleware runs, then validation is skipped and the handler is called.
   - **`POST /session/keepalive`** is NOT exempt. It is HTMX-driven via `htmx.ajax(...)` in `static/js/session-timeout.js:97-98` (covered by listener 1 of `csrf.js`) and via the `fetch()` fallback on line 100 (covered by Task 6.6 which patches that fallback to include `X-CSRF-Token` manually from the meta tag). Token validates against the authenticated session's `csrf_token`.
   - **`POST /debug/session-timeout`** is NOT exempt. It is guarded at runtime by `std::env::var("TEST_MODE") == "1"` + `require_role(Admin)` (`src/routes/catalog.rs:2086-2089`), so in production it returns 404 before CSRF runs anyway. In test mode it must still carry a valid token (tests drive it via HTMX or include the header explicitly). No additional changes needed — it inherits CSRF validation from the middleware layer.
   - Given a mismatch/missing-token rejection, when the response is built, then it is `AppError::Forbidden` with status 403, a server-rendered FeedbackEntry body localized via `rust_i18n::t!("error.csrf_rejected_*")`, AND three HTMX-coordination headers: `HX-Trigger: csrf-rejected`, `HX-Retarget: #feedback-list`, `HX-Reswap: beforeend`. Together these let `csrf.js`'s force-swap listener (§Ships 8 listener 2) inject the server-rendered body into the page's FeedbackEntry list without any client-side i18n.

6. **CSRF-rejection 403 response carries CSRF-specific coordination headers.**
   - The middleware builds the response by calling `feedback_html("error", title, message)` (via `rust_i18n::t!()` for the two new `error.csrf_rejected_*` keys) wrapped in `Response::builder().status(403)`, then sets three headers: `HX-Trigger: csrf-rejected`, `HX-Retarget: #feedback-list`, `HX-Reswap: beforeend`. Non-CSRF `Forbidden` paths (e.g., librarian hitting `/admin`) do NOT carry these headers — they still go through `AppError::Forbidden.into_response()` with the generic feedback.
   - **New pattern alert:** The `HX-Trigger` → JS-listener idiom is NEW for this codebase (verified via `grep "HX-Trigger" src/` → zero hits). Document it in Dev Notes as a project pattern worth reusing for future server-driven UI coordination.

7. **`templates/layouts/base.html` emits `<meta name="csrf-token" content="{{ csrf_token|e }}">` in `<head>`.**
   - Placed right after `<meta charset="utf-8">` / `<meta name="viewport">` and before any stylesheet link, so the value is available before any external resource loads.
   - The `|e` escape filter is mandatory — the token is base64 URL-safe (no HTML-unsafe chars expected) but belt-and-braces.

8. **`static/js/csrf.js` — exactly two listeners, no client-side i18n, no new JS API.**
   - File: `static/js/csrf.js`. Loaded from `layouts/base.html` via **classic** `<script src="/static/js/csrf.js"></script>` (NOT `type="module"` — matches the existing convention for all 6 other JS modules).
   - **Listener 1 — token injection:** `document.body.addEventListener("htmx:configRequest", evt => evt.detail.headers["X-CSRF-Token"] = document.querySelector('meta[name="csrf-token"]').content)`. Runs on every HTMX-driven mutation.
   - **Listener 2 — force-swap the 403 body:** `document.body.addEventListener("htmx:beforeSwap", evt => { if (evt.detail.xhr.status === 403 && evt.detail.xhr.getResponseHeader("HX-Trigger") === "csrf-rejected") { evt.detail.shouldSwap = true; evt.detail.isError = false; } })`. Lets HTMX inject the server-rendered feedback body into `#feedback-list` (via the `HX-Retarget` + `HX-Reswap` headers the middleware also emits) — default HTMX behavior on 403 would be to discard the body.
   - **Zero local i18n strings** — the 403 body arrives pre-localized by `rust_i18n::t!()` in the middleware.
   - **Zero new `window.*` APIs** — the module is an IIFE with no exports (same shape as `session-timeout.js`).
   - CSP compliance: `script-src 'self'`, no inline, no `eval`.

9. **Every `<form method="POST">` in `templates/` includes the hidden CSRF input.**
   - The hidden input is placed as the **first child** of the `<form>` element (immediately after the opening tag) for readability and to match the regex pattern that the audit test uses.
   - The audit test `src/templates_audit.rs::forms_include_csrf_token` walks `templates/` and asserts the pattern. A new form without the input fails `cargo test` — same guarantee as the CSP audit.

10. **`src/templates_audit.rs::csrf_exempt_routes_frozen` enforces the allowlist.**
    - Parse the source of `src/middleware/csrf.rs` (or import the `CSRF_EXEMPT_ROUTES` const via a reachable module path). Assert `len() == 1` and the single entry is `("POST", "/login")`. Same freezing approach as story 7-5's `hx_confirm_matches_allowlist`.

11. **Nav-bar logout is a POST form with CSRF token.**
    - `templates/components/nav_bar.html`: the existing `<a href="/logout">` becomes a `<form method="POST" action="/logout" class="inline">` with the hidden input and a `<button type="submit">` styled to match the old link.
    - Desktop and mobile variants both converted.
    - `src/routes/mod.rs:88` drops the GET variant — `/logout` route is POST-only.
    - Visual regression: the nav-bar Playwright screenshot for the logout label should render identically to the pre-change screenshot (button vs anchor pixel-diff within tolerance).

12. **Login remains CSRF-exempt, documented.**
    - The `POST /login` route is listed in `CSRF_EXEMPT_ROUTES` with a comment explaining why (no authenticated session exists at request time; `SameSite=Lax` is the login-CSRF mitigation).
    - All other POST routes (including `POST /language` from anonymous users) go through CSRF validation — anonymous visitors have a session row (per AC 3) and therefore a token.

13. **Unit tests pass (new) — see §Ships point 16 for the list.**

14. **Integration tests pass (new) — see §Ships point 17 for the list.**

15. **i18n keys added — server-side only, per §Ships 15.**
    - New keys: `error.csrf_rejected_title`, `error.csrf_rejected_message` in both `locales/en.yml` and `locales/fr.yml`.
    - No JS-side i18n keys (the 403 body is server-rendered; `csrf.js` is string-free).
    - Post-edit: `touch src/lib.rs && cargo build` to force proc-macro re-read (per CLAUDE.md i18n §).

16. **E2E tests pass (new) — see §Ships point 18 for the list.** Foundation Rule #7 smoke path included: blank browser → login → submit form with valid token → tamper token → verify 403 + localized feedback injected without page reload.

17. **All 24 existing mutation points continue to work in the happy path.**
    - Login → catalog scan → title/volume/contributor edits → location CRUD → series CRUD → borrower CRUD → loan registration + return → language toggle → logout. No regression; every flow carries a valid token and is accepted.
    - Verified via the existing Playwright suite (`cd tests/e2e && npm test`) — zero new flakes; the 3-cycle fresh-stack gate is green.

18. **Documentation complete — CLAUDE.md bullet, architecture subsection, route-role-matrix column — per §Ships point 19.**

## Tasks / Subtasks

- [ ] **Task 1: Schema migration — csrf_token column only (AC: 1, 2)**
  - [ ] 1.1 Confirm MariaDB target version supports `RANDOM_BYTES()` (≥ 10.10); if not, use `MD5(CONCAT(RAND(), token, UUID()))` backfill fallback
  - [ ] 1.2 Write `migrations/20260418000000_add_csrf_token_to_sessions.sql` with TWO statements: `ALTER ADD csrf_token VARCHAR(64) NOT NULL DEFAULT ''` + `UPDATE sessions SET csrf_token = LOWER(HEX(RANDOM_BYTES(32))) WHERE csrf_token = ''`
  - [ ] 1.3 **Do NOT touch `sessions.user_id`** — it is already `BIGINT UNSIGNED NULL` per `migrations/20260329000000_initial_schema.sql:224`; any ALTER is destructive to the FK
  - [ ] 1.4 Run migration on a seeded DB; verify `SELECT COUNT(*) FROM sessions WHERE csrf_token = ''` returns 0
  - [ ] 1.5 Update `SessionModel` (`src/models/session.rs`) to read/write `csrf_token`
  - [ ] 1.6 `cargo sqlx prepare` to regenerate `.sqlx/` offline cache

- [ ] **Task 2: `src/middleware/csrf.rs` (AC: 5, 6, 10, 12)**
  - [ ] 2.1 Add `subtle = "2.6"` to `Cargo.toml`
  - [ ] 2.2 Implement `csrf_middleware`, `CSRF_EXEMPT_ROUTES` (single entry `("POST", "/login")`), `generate_csrf_token`
  - [ ] 2.3 Handle body buffering + re-injection for form-field fallback (1 MiB cap via `axum::body::to_bytes`)
  - [ ] 2.4 Explicit guard: reject with `AppError::Internal` if the session's stored `csrf_token` is empty (never match on `""`) — AC 5 empty-token clause
  - [ ] 2.5 403 response emits `HX-Trigger: csrf-rejected` + `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` alongside the localized FeedbackEntry body
  - [ ] 2.6 Wire into `src/routes/mod.rs::build_router` in the correct layer order (`Logging → Auth → CSRF → [Handler] → PendingUpdates → CSP`)
  - [ ] 2.7 Unit tests (§Ships 16) — include empty-token-rejection case

- [ ] **Task 3: Session struct propagation + lazy anonymous row (AC: 3, 4)**
  - [ ] 3.1 Add `csrf_token: String` to `Session`
  - [ ] 3.2 Rename/replace `Session::anonymous()` → `Session::anonymous_with_token(...)`; update call sites (audit via `grep -rn "Session::anonymous" src/`)
  - [ ] 3.3 Implement anonymous-session INSERT in auth middleware on first-hit (no cookie → INSERT + set cookie)
  - [ ] 3.4 Adjust `src/routes/auth.rs::login` to soft-delete the anonymous row and INSERT a fresh authenticated row

- [ ] **Task 4: Anonymous session purge task (AC: 3)**
  - [ ] 4.1 New file `src/tasks/anonymous_session_purge.rs` — mirrors `src/tasks/provider_health.rs` spawn+interval pattern
  - [ ] 4.2 Query: `DELETE FROM sessions WHERE user_id IS NULL AND last_activity < UTC_TIMESTAMP() - INTERVAL 7 DAY`
  - [ ] 4.3 Interval: `tokio::time::interval(Duration::from_secs(86400))` — first run 24h after boot
  - [ ] 4.4 Log at `info` with `rows_deleted` count
  - [ ] 4.5 Spawned from `src/main.rs` next to the existing provider-health spawn

- [ ] **Task 5: Login / logout wiring (AC: 3, 11)**
  - [ ] 5.1 `login` handler: generate `csrf_token` alongside session token, persist both in the same INSERT
  - [ ] 5.2 Drop GET-method variant of `/logout` in `src/routes/mod.rs`
  - [ ] 5.3 Convert nav-bar logout anchor to POST form (desktop + mobile) with hidden CSRF input
  - [ ] 5.4 Delete the obsolete "No CSRF token" docstring paragraph in `src/routes/auth.rs::change_language` (lines 247-250)

- [ ] **Task 6: Base template + BaseContext + JS + per-template propagation (AC: 7, 8, 9, 15)**
  - [ ] 6.1 Add `<meta name="csrf-token" content="{{ csrf_token|e }}">` to `templates/layouts/base.html`
  - [ ] 6.2 Design `base_context(&session, locale, current_page)` helper returning common fields (incl. `csrf_token`)
  - [ ] 6.3 Add `csrf_token: String` to every **full-page** template struct that extends `base.html` (filter via `grep "extends \"layouts/base.html\"" templates/` — approx. 15-18 structs, not the full 29; fragment-only templates don't need the field)
  - [ ] 6.4 Walk all `<form method="POST">` in `templates/` — add hidden input `<input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">` as the first child
  - [ ] 6.5 Create `static/js/csrf.js` with the two listeners specified in §Ships 8 — classic script, no `window.*` exports, no local i18n
  - [ ] 6.6 Patch `static/js/session-timeout.js` `fetch()` fallback (line 100) to include `X-CSRF-Token` from the meta tag: `fetch("/session/keepalive", { method: "POST", headers: { "X-CSRF-Token": document.querySelector('meta[name="csrf-token"]').content } })`
  - [ ] 6.7 Register `csrf.js` in `templates/layouts/base.html` via classic `<script src="/static/js/csrf.js"></script>`
  - [ ] 6.8 Add i18n keys `error.csrf_rejected_title` + `error.csrf_rejected_message` in EN + FR
  - [ ] 6.9 `touch src/lib.rs && cargo build` (i18n proc macro re-read)

- [ ] **Task 7: Template audit hardening (AC: 9, 10)**
  - [ ] 7.1 Add `forms_include_csrf_token` test in `src/templates_audit.rs` — walks `templates/`, regex every `<form method="(post|POST)"...>`, asserts next `<input>` is the CSRF hidden field
  - [ ] 7.2 Add `csrf_exempt_routes_frozen` test — asserts `CSRF_EXEMPT_ROUTES.len() == 1` and entry is `("POST", "/login")`
  - [ ] 7.3 Ensure both audit tests fail loudly on regression

- [ ] **Task 8: Integration tests (AC: 14)**
  - [ ] 8.1 New file `tests/csrf_integration.rs` with `#[sqlx::test]` cases per §Ships 17
  - [ ] 8.2 Reuse `seed_user_and_session` pattern from `src/routes/auth.rs::language_tests`
  - [ ] 8.3 Include: `GET /logout` → 405 (router no longer wires the GET method); `POST /session/keepalive` with token → 200, without → 403; anonymous first-hit creates session row with fresh token

- [ ] **Task 9: E2E spec (AC: 16)**
  - [ ] 9.1 `tests/e2e/specs/security/csrf.spec.ts`, spec ID `"CS"`
  - [ ] 9.2 Smoke (Foundation Rule #7), logout-GET-blocked, i18n-FR, feedback-force-swap paths
  - [ ] 9.3 Verify `scripts/e2e-reset.sh` produces a fresh DB with the new migration applied (backfill runs clean)

- [ ] **Task 10: Documentation (AC: 18)**
  - [ ] 10.1 CLAUDE.md — "Key Patterns" bullet for CSRF (include: synchronizer-token bound to `sessions.csrf_token`, `base_context()` helper, `csrf.js` two-listener design, `HX-Trigger: csrf-rejected` as NEW pattern for the project, frozen exempt-route allowlist)
  - [ ] 10.2 `_bmad-output/planning-artifacts/architecture.md` — Authentication & Security subsection + fix stale `SameSite=Strict` language (code is `Lax` since 7-3)
  - [ ] 10.3 `docs/route-role-matrix.md` — add `csrf_exempt` column

- [ ] **Task 11: Regression gate (AC: 17)**
  - [ ] 11.1 Run `cargo test` — all unit + integration green
  - [ ] 11.2 Run `cargo clippy -- -D warnings` — zero warnings
  - [ ] 11.3 Run `./scripts/e2e-reset.sh` + `cd tests/e2e && npm test` — 3 clean cycles
  - [ ] 11.4 Run the flake gate `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` — zero hits (new spec uses DOM-state assertions)

## Dev Notes

### Approach rationale — synchronizer token vs double-submit

Decision is synchronizer-token (server-stored, bound to session). Alternatives considered:

- **Double-submit cookie:** issue a second non-HttpOnly cookie `csrf_cookie=<random>`; forms/HTMX include the same value; the middleware compares header-or-field against the cookie value. No server storage. **Rejected** because (a) we already persist sessions server-side (no storage savings), (b) an attacker who can set cookies via a sibling subdomain can bypass it (not a current threat but a smell on single-user NAS with future multi-subdomain plans), and (c) it means two cookies to reason about.
- **Origin / Referer validation only:** lightweight, no token. **Rejected** because browsers (Firefox strict privacy mode, Safari in some configs) strip these; CSP `form-action 'self'` plus SameSite=Lax plus same-origin check is roughly what we have today and Epic 7 retro called this insufficient.
- **`axum_csrf` / `tower-csrf` crate:** external dep. **Rejected** because both are Axum-0.7-era with cookie-centric APIs that conflict with server-stored tokens; rolling ~80 lines of middleware is clearer.

### Layer order — AR16 update

AR16 currently reads `Logging → Auth → [Handler] → PendingUpdates → CSP` (per CLAUDE.md). This story updates it to `Logging → Auth → CSRF → [Handler] → PendingUpdates → CSP`. CSRF runs AFTER auth (it needs the `Session` struct to know the expected token) and BEFORE the handler (so the handler never sees forged requests). It runs BEFORE PendingUpdates because on a 403 rejection we don't want OOB metadata leaking into the error response; PendingUpdates only fires on successful HTMX handler responses. Record the AR16 update in `architecture.md`.

### Body buffering for form-field fallback

Axum's default body limit is ~2 MB. Buffer the body once with `axum::body::to_bytes(body, MAX_CSRF_BODY_BYTES)` (use a sane limit like 1 MiB — mutations are tiny forms). **Multipart upload handling is not a concern**: verified 2026-04-18 that no route in this codebase accepts multipart (`grep -i multipart src/` → zero hits; covers are fetched server-side from provider APIs, never uploaded by the user). Re-attach the body via `Request::from_parts(parts, Body::from(bytes))`. If the body exceeds the limit, return 413 Payload Too Large without even looking at CSRF (saves reading megabytes of garbage before rejecting). Document the limit in the middleware doc-comment.

### Template refactor concern — `base_context` helper

**Blast radius (verified 2026-04-18):** `grep "#\[derive(Template)\]" src/routes/*.rs` counts 29 structs. **But only full-page structs that `{% extends "layouts/base.html" %}` need the `csrf_token` field** — fragment-only templates (feedback entries, metadata OOB fragments, admin sub-panels rendered via HTMX into a parent page) do not. Before Task 6.3 starts, run `grep -l 'extends "layouts/base.html"' templates/**/*.html` to get the exact subset (expected ~15-18 templates, filter correspondingly in the Rust struct side).

Rather than mechanically add `csrf_token: session.csrf_token.clone()` to every full-page struct, build a helper:

```rust
pub struct BaseContextFields {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String, pub nav_loans: String, /* ... */ pub nav_admin: String,
    pub nav_login: String, pub nav_logout: String,
    pub lang_toggle_aria: String,
    pub csrf_token: String,
}

pub fn base_context(session: &Session, locale: &str, current_page: &'static str) -> BaseContextFields { /* ... */ }
```

Each handler flattens the fields into its local template struct. This is the DRY move; the alternative (a "god template context" inherited via a trait) fights Askama's strict-typing model. If a future story wants to extend the common fields, it extends `BaseContextFields` in one place and every template gets the field automatically.

### Existing middleware pattern reference

`src/middleware/pending_updates.rs` is the closest existing analog of the middleware we're building. It reads the response body, appends OOB fragments, and returns a modified response. Our CSRF middleware reads the REQUEST body (for form-field fallback), validates, and either passes through or rejects. Use its body-handling idioms as the template (no pun intended) for our middleware.

### What the existing `src/routes/auth.rs::change_language` docstring already says

Lines 247-250 of `src/routes/auth.rs` document the current CSRF posture explicitly:
> *"No CSRF token: same-origin form POST with `SameSite=Lax` on the session cookie matches the `/login` and `/logout` handler pattern."*

This story's final commit must DELETE that paragraph from the docstring. Leaving it creates a contradiction between the code (token now required) and the comment (token "not needed").

### Running tests / build steps specific to this story

```bash
# After schema migration:
cargo sqlx prepare --check --workspace -- --all-targets

# After locale edits (per CLAUDE.md rule):
touch src/lib.rs && cargo build

# After templates changes:
cargo test templates_audit -- --nocapture

# Full unit gate:
cargo test

# Integration gate (new):
cargo test --test csrf_integration

# E2E 3-cycle gate (Foundation Rule #3 + #7):
./scripts/e2e-reset.sh
cd tests/e2e && for i in 1 2 3; do npm test || exit 1; done
```

### Project Structure Notes

- New files: `src/middleware/csrf.rs`, `src/tasks/anonymous_session_purge.rs`, `static/js/csrf.js`, `tests/csrf_integration.rs`, `migrations/20260418000000_add_csrf_token_to_sessions.sql`, `tests/e2e/specs/security/csrf.spec.ts`.
- Modified files: `src/middleware/mod.rs` (register new module), `src/middleware/auth.rs` (Session struct), `src/models/session.rs` (csrf_token row field + anonymous-session INSERT), `src/routes/auth.rs` (login handler — persist CSRF token; DELETE the obsolete "No CSRF token" docstring at lines 247-250), `src/routes/mod.rs` (middleware wiring; drop GET /logout method), every `src/routes/*.rs` that owns a full-page template struct (`csrf_token` field via `base_context` helper — approx. 15-18 structs, NOT all 29 `#[derive(Template)]`), `src/tasks/mod.rs` (register anonymous-session purge task), `src/main.rs` (spawn the purge task), `src/templates_audit.rs` (two new assertions), `templates/layouts/base.html` (meta tag + classic `<script src>` for csrf.js), `templates/components/nav_bar.html` (logout anchor → POST form), every `templates/**/*.html` with `method="POST"` (hidden `_csrf_token` input), `static/js/session-timeout.js` (patch `fetch()` fallback to include `X-CSRF-Token`), `locales/en.yml` + `locales/fr.yml` (two keys — `error.csrf_rejected_title`, `error.csrf_rejected_message`), `Cargo.toml` (`subtle = "2.6"` dependency), `CLAUDE.md` (Key Patterns bullet — include the NEW HX-Trigger idiom), `_bmad-output/planning-artifacts/architecture.md` (Auth & Security subsection + AR16 update + SameSite correction), `docs/route-role-matrix.md` (csrf_exempt column).
- **NOT modified (explicitly):** `sessions.user_id` schema (already `BIGINT UNSIGNED NULL` per initial migration), FK `fk_sessions_user` (untouched; NULL `user_id` rows are FK-exempt by MySQL/MariaDB default behavior).
- Detected conflicts / variances: none expected — this is additive. Two pre-existing doc drifts are opportunistically fixed here (`SameSite=Strict` → `Lax` in architecture.md; obsolete "No CSRF token" docstring removed from `change_language`).

### References

- [Source: CLAUDE.md — Foundation Rules #1/#2/#3/#4/#5/#6/#7]
- [Source: CLAUDE.md — Key Patterns — Session, HTMX OOB Swap Pattern, CSP & hardening headers]
- [Source: _bmad-output/planning-artifacts/architecture.md — Authentication & Security, Session Lifecycle, CSP Directives (lines 445–474)]
- [Source: _bmad-output/planning-artifacts/architecture.md — AR16 middleware order]
- [Source: _bmad-output/planning-artifacts/epics.md — Epic 8 Story 8.2 (renumbered 2026-04-18) and Scope Note on cross-cutting constraints]
- [Source: _bmad-output/implementation-artifacts/epic-7-retro-2026-04-17.md — §4 bullet 3 (chronic deferral), §7 Action 1 (hard deadline), §5 logout-CSRF surface]
- [Source: _bmad-output/implementation-artifacts/1-9-minimal-login.md — CSRF deferral note, Logout Pattern §]
- [Source: _bmad-output/implementation-artifacts/7-1-anonymous-browsing-and-role-gating.md — `require_role_with_return` pattern, `docs/route-role-matrix.md` creation]
- [Source: _bmad-output/implementation-artifacts/7-3-language-toggle-fr-en.md — language toggle POST pattern; local per-module `var i18n = {...}` JS pattern (see `static/js/session-timeout.js:58-71` — no `window.i18n` global exists)]
- [Source: _bmad-output/implementation-artifacts/7-4-content-security-policy-headers.md — `src/templates_audit.rs` pattern, strict CSP constraints]
- [Source: _bmad-output/implementation-artifacts/7-5-scanner-guard-modal-interception.md — frozen-allowlist pattern in `src/templates_audit.rs`]
- [Source: src/routes/auth.rs lines 173-177 (SameSite=Lax), lines 247-250 (current CSRF docstring — to be deleted)]
- [Source: src/routes/mod.rs lines 86-90 (`/logout` GET+POST; `/language` POST)]
- [Source: src/middleware/auth.rs lines 36-54 (Session struct)]
- [Source: templates/components/nav_bar.html line 42 (logout anchor — to be converted)]
- [Source: src/templates_audit.rs (existing CSP audit pattern + `hx-confirm` frozen allowlist)]
- [Source: subtle crate docs — https://docs.rs/subtle/2.6/subtle/ for `ConstantTimeEq`]

### Previous Story Intelligence

**Story 8-1 (Admin shell + Health tab, done 2026-04-17) — what carries forward:**
- `Session::require_role_with_return(Role::Admin, "/admin...")` pattern works and is unchanged by this story.
- `ProviderRegistry::iter()` was added in 8-1 — not touched here.
- `soft_delete::ALLOWED_TABLES` was promoted to `pub` in 8-1 — still needed in 8-6, not here.
- 8-1 code-review patches (P1–P5, committed 2026-04-18 as `aff102e`) have stabilized the Health tab; this story adds no new admin surface, only fortifies the mutation plumbing for 8-3..8-8.
- The `<meta name="csrf-token">` injection point (this story's AC 7) must be compatible with every page that extends `layouts/base.html`, including the `/admin` page from 8-1 (which DOES extend base.html — verified in `templates/pages/admin.html`).

**Story 7-3 (language toggle) — load-bearing trap to avoid:**
- The `change_language` handler in `src/routes/auth.rs` has a docstring (lines 247-250) that explicitly says "No CSRF token". This story DELETES that paragraph (Task 5.4). Do not just amend — delete it, because the wording "matches the `/login` and `/logout` handler pattern" becomes actively wrong after this story ships (login remains exempt; logout becomes protected).
- The language toggle form in `templates/components/nav_bar.html` already wraps `<form method="POST" action="/language">` — just add the hidden CSRF input, no structural change.
- The authenticated-persistence branch (optimistic-locking UPDATE on `users.preferred_language`) is unchanged by this story.
- **JS i18n pattern clarification:** 7-3's references list mentions "embedded `window.i18n` JS string map" but the reality is each JS module declares its OWN local `var i18n = {...}` block (see `static/js/session-timeout.js:58-71`). There is no global `window.i18n`. Corrected in this story's AC 8 / §Ships 8: `csrf.js` has no i18n at all (feedback body is server-rendered); no new JS-side string keys are needed.

**Story 7-5 (scanner-guard) — frozen-allowlist pattern to reuse:**
- `src/templates_audit.rs::hx_confirm_matches_allowlist` is the template for `csrf_exempt_routes_frozen`. Copy the structure, import the const via `use crate::middleware::csrf::CSRF_EXEMPT_ROUTES;` (same crate, same `src/` tree — direct import is cleaner than re-parsing source).
- 7-5's modal-guard pattern is not load-bearing here (no new modals in this story).
- **The `HX-Trigger` → JS-listener idiom is NEW for this codebase** — verified via `grep "HX-Trigger" src/` → zero hits and `grep "HX-Retarget" src/` → zero hits. Earlier drafts claimed this "mirrors 7-5's out-of-band event model" — corrected: 7-5 uses JS-side `CustomEvent` for modal-scoped coordination, not server-driven HTMX triggers. Document the new idiom in the CLAUDE.md bullet so future admin-mutation stories (8-3..8-8) can reuse it for optimistic-locking conflicts, session expiry, etc.

**Story 7-4 (CSP) — constraint boundaries:**
- Strict CSP (`script-src 'self'`, no `unsafe-inline`) means every JS behavior we add goes through a loaded `.js` module and event listeners. The new `csrf.js` module follows the 6 existing modules' pattern.
- The `<meta name="csrf-token">` tag is a plain `<meta>` — no inline script, no style — CSP-inert.

**Story 6-2 (librarian seed + `loginAs(page, role)`) — reusable for E2E:**
- The E2E spec for CSRF will use `loginAs(page, "admin")` (default) and for the FR path use `loginAs(page)` after setting `lang=fr` cookie. Both `admin` and `librarian` seeds are already available.

### Git Intelligence Summary

Last 5 commits (as of 2026-04-18 15:00):
- `aff102e` Story 8-1: apply code-review patches (P1-P5) and mark done — **stabilizes 8-1 before 8-2 starts**
- `c606513` docs(epics): decompose Epic 8 into 7 stories — **scope baseline (since amended to 8 stories 2026-04-18 with CSRF insertion)**
- `454e0ce` docs(claude-md): add Foundation Rule #10 — commit/push cadence — **Foundation Rule #10 applies to this story: commit after create-story, after dev-story, after code-review; push at epic close / on demand**
- `b7dd690` Epic 8 Story 8-1: Admin page shell + Health tab — **8-1 main commit, landed 2026-04-17 pre-review**
- `1d36d2b` chore: add scripts/e2e-reset.sh for fresh-stack dev loop (#34) — **Epic 7 retro Action 4; use this script for the 3-cycle E2E gate**

**Pattern signal:** Each recent epic has shipped a "templates_audit" enforcement (CSP in 7-4, hx-confirm allowlist in 7-5). This story continues that pattern with two new guards (form CSRF inputs + exempt-routes allowlist). The audit test is a project-defining architectural gate — invest in it here rather than deferring to human review.

### Latest Technical Information

**Axum 0.8.8 — middleware body-reading idiom:**
```rust
use axum::{body::Body, http::Request, middleware::Next, response::Response};
use http_body_util::BodyExt; // for .collect()

pub async fn csrf_middleware(
    State(state): State<AppState>,
    session: Session, // from the Auth layer (ran before us)
    req: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let (parts, body) = req.into_parts();
    let bytes = body
        .collect()
        .await
        .map_err(|_| AppError::BadRequest("body read error".into()))?
        .to_bytes();

    // Validation goes here — read parts.method, parts.headers, `bytes` for form body.
    // On success rebuild: Request::from_parts(parts, Body::from(bytes)) and call next.run(req).await
    // On failure return AppError::Forbidden with HX-Trigger header.
    unimplemented!()
}
```

**subtle crate `ConstantTimeEq`:**
```rust
use subtle::ConstantTimeEq;
let a: &[u8] = expected_token.as_bytes();
let b: &[u8] = received_token.as_bytes();
let eq: subtle::Choice = a.ct_eq(b);
if bool::from(eq) { /* match */ } else { /* reject */ }
```

**rust-i18n 3.x (existing dep):** Key additions go under root-level namespaces; file-per-locale (no `en:` / `fr:` wrapper). `touch src/lib.rs && cargo build` after edits forces proc-macro re-read (per CLAUDE.md i18n §).

### Project Context Reference

`docs/project-context.md` does not currently exist in this repo (checked 2026-04-18). Project conventions are sourced from CLAUDE.md + architecture.md + epics.md + previous story artifacts. This story conforms to all three.

## Dev Agent Record

### Agent Model Used

_To be filled by dev-story._

### Debug Log References

_To be filled by dev-story._

### Completion Notes List

_To be filled by dev-story._

### File List

_To be filled by dev-story._
