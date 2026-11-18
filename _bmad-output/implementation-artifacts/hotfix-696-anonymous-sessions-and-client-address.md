# Story hotfix-696: Requests that cannot use a session leave no trace in `sessions`

Status: review

GitHub issue: #696 (closes). Covers: fix. Outside any epic; ships in patch v1.28.1 (decided 2026-11-18, all three points). Sprint-status key: `hotfix-696-anonymous-sessions-and-client-address`.

## Story

As the operator of a mybibli instance,
I want a misbehaving client to cost no database write, to get a clear answer, and to be identifiable in the logs,
so that one misconfigured MCP client cannot put the pool under pressure while staying anonymous.

## Context

Production, v1.28.0, since 2026-11-07: an MCP client configured with `/mcp` instead of `/api/mcp` POSTs ~1 200–2 200 times a day. Each request has no cookie: `session_resolve_middleware` (before routing) inserts an anonymous `sessions` row, then `csrf_middleware` answers 403 `csrf_token_mismatch` for a path that is not routed. ~1 000 of ~1 110 `slow statement` warnings are those inserts; the pool times out several times a day. No log line carries the client address.

## Decisions

- **D1 — client address (point 1).** `src/middleware/client_addr.rs`: the peer address comes from `ConnectInfo<SocketAddr>` (`main.rs` serves with `into_make_service_with_connect_info`). `MYBIBLI_TRUSTED_PROXIES` (comma-separated IPs or CIDRs, read once; an entry that does not parse is ignored with a `warn` — it can only make the result fall back to the peer, never trust more). When the peer is trusted, `X-Forwarded-For` is walked right to left and the first address that is not a trusted proxy is the client; without `X-Forwarded-For`, `X-Real-IP`; a malformed entry stops the walk at the last good hop. Unset → the peer address (no header honoured). IPv4-mapped IPv6 addresses are shown as IPv4. Logged as `client` on CSRF rejections, API-key authentication (success and every 401 / 403) and MCP refusals.
- **D2 — no anonymous row (point 2).** The resolver persists nothing for: any `/api/*` path (none reads a `Session`; replaces the `/api/mcp` and cover-upload special cases); an unrouted path (the 404 fallback is registered after the application layers, so it runs none of them — review pass 2: testing for a missing `MatchedPath` would also have opened CSRF on any future `nest_service` mount); a request other than GET **carrying no `session` cookie**, `POST /login` included (review pass 5: a credential-stuffing bot posts without cookie; the route stays CSRF-exempt and a success inserts its own row) — it gets an in-memory `Session::anonymous_with_token`, no cookie (HEAD / OPTIONS included: probes and preflights store no cookie). A browser holding a cookie whose row is gone still gets a minted row and the rotated token (#37 unchanged).
- **D3 — 404 before CSRF (point 3).** A router `fallback` registered after the layers answers 404 (no session, CSRF or setup-wizard layer runs for it; a mistyped URL during the setup wizard now gets a 404 instead of the redirect to `/setup`). `POST`/`GET /mcp` (any method, normalized path) answers a JSON 404 naming `/api/mcp`, with a `warn` carrying method and client; other unknown paths keep the empty 404, logged at `debug`. A state-changing request without cookie on a routed path is refused as before #696 (HTMX: the 403 "session expired, reload the page" entry; plain anonymous form: 303 to `/login`), logged with reason `no_session_cookie` instead of `csrf_token_mismatch`. Review passes 1–3 tried a redirect to `/login?next=` (an expired cookie looped on 403s); pass 4 showed it loses unsaved text on the page, drops `no-store` and lets a cross-site `Referer` choose `next=`. The 403 names its remedy ("reload the page"), keeps the page, and is what users already met; it was restored.
- **D4 — DRY.** The four CSRF rejection sites share one `reject(...)` (log + locale + response) and `request_locale`; the CSRF exemption check is one `csrf::is_exempt`, used by the resolver too; the API-key 401s and 403s share one logging constructor each.
- **D5 — the cookieless signal is an extension, not a header read (found in dev).** The resolver marks its unstored session with `auth::CookielessRequest`; CSRF refuses on that marker. Reading the `Cookie` header in CSRF instead broke the unit harness that injects a `Session` directly, and would tie CSRF to the resolver's cookie parsing.

## Acceptance Criteria

1. **AC1.** `POST /mcp` without cookie → 404 whose body names `/api/mcp`; no `sessions` row inserted; one `warn` with the client address.
2. **AC2.** `GET /does-not-exist`, `POST /does-not-exist`, `GET /api/v1/titles` (no key) → no `sessions` row; the unknown paths answer 404.
3. **AC3.** `POST` to a routed path without cookie → HTMX: 403 `csrf-rejected` (`no-store`); plain form: 303 `/login`; reason `no_session_cookie`; no row, no cookie; `HEAD` / `OPTIONS` without cookie → no row. `POST /login` without cookie still logs in. `GET /` without cookie still mints a row and sets the cookie.
4. **AC4.** Unit cases of the address resolution: untrusted peer ignores headers; trusted peer + XFF chain (with trusted hops) → first untrusted; X-Real-IP fallback; malformed entry; IPv4-mapped; CIDR and invalid entries in the variable.
5. **AC5.** Docs: `MYBIBLI_TRUSTED_PROXIES` in `docker-compose.yml` comment, `docs/auth-threat-model.md`, installation chapter (en/fr), CLAUDE.md.

## Tasks / Subtasks

- [x] T1 — `middleware::client_addr` + `ConnectInfo` in `main.rs` + `ipnet` (D1, AC4)
- [x] T2 — resolver skips + `CookielessRequest` (D2, D5)
- [x] T3 — CSRF: unrouted pass-through, `no_session_cookie`, `reject` (D3, D4)
- [x] T4 — `routes::not_found::fallback` (D3)
- [x] T5 — client address on API-key and MCP log lines (D1)
- [x] T6 — `tests/cookieless_requests.rs` (AC1–AC3), unit tests (AC4)
- [x] T7 — docs (AC5)
- [x] T8 — review fixes
- [ ] T9 — gates

## Dev Notes

- E2E: `security/csrf.spec.ts` — the session cookie disappears (idle expiry, simulated by clearing cookies) on `/catalog`, the next scan shows "session expired" and the page stays; `journeys/mcp-endpoint.spec.ts` — `POST /mcp` answers 404 naming `/api/mcp`, no session cookie. The cookieless bot / unrouted / API journeys are covered through the whole router in `tests/cookieless_requests.rs`.
- Review (high, 9 candidates): fixed 1 (expired cookie → `/login`), 2 (HEAD / OPTIONS), 3 (`X-Forwarded-For` hops with port, brackets, empty elements — positive shapes), 4 (E2E), 5 (variable read at boot), 7 (client computed only inside the log macros), 8 (path normalized once), 9 (docs name exactly which MCP refusals carry `client`). Kept 6: one `warn` per refused API key is the signal #696 asks for.
- Review pass 2 (high, 9 candidates): fixed 1 (fallback after the layers, no `MatchedPath` test), 2 (no comment line in sprint-status), 4 (cookieless check before every other CSRF refusal), 5 (`/login?next=`), 6 (only a request with no `session=` cookie at all is cookieless; two cookies keep the old mint), 7 (unreadable rightmost hop → `X-Real-IP`; IPv4-mapped trusted networks); 3 (unresolvable cookie on a non-GET still mints) → #699, it changes the #37 recovery; kept 8 (one `warn` per `/mcp` hit is the signal asked for) and 9 (two small allocations per logged line).
- Review pass 3 (high, 9 candidates): fixed 1 (HTMX gets `200` + `HX-Redirect`, not a 303 — `middleware::htmx::{is_htmx, redirect}` now shared with `setup_gate` and `csrf`, three copies of the HTMX test removed), 2 (keepalive's `fetch` fallback follows a redirect to the login page), 5 (`utils::safe_path_of_url`, moved from `loans.rs`, replaces the hand-written parser), 6 (dead `/api` exact match removed), 8 (docs: when `X-Real-IP` is used), 9 (style). Kept 3 (an anonymous plain form with a stale token already went to `/login` before), 4 (a cookieless GET still mints: lazy minting is a redesign; the probe on `/` is already pointed at `/health`), 7 (the order is pinned by `tests/cookieless_requests.rs`, run by CI).
- Sharing `normalize_exempt_path` for the fallback keeps `//mcp` and `/mcp/` on the hint.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (`claude-opus-5-5`)

### File List

- `src/middleware/client_addr.rs` (new), `src/middleware/{mod,auth,csrf,api_key}.rs`, `src/mcp/mod.rs`, `src/main.rs`
- `src/routes/not_found.rs` (new), `src/routes/mod.rs`
- `tests/cookieless_requests.rs` (new)
- `Cargo.toml`, `Cargo.lock`
- `docker-compose.yml`, `.env.example`, `README.md`, `docs/auth-threat-model.md` (§ 5.12), `docs/manual/{en,fr}/01-installation.tex`, `CLAUDE.md`, `_bmad-output/implementation-artifacts/sprint-status.yaml`
- Review pass 4 (high, 10 candidates): fixed 1, 2, 3, 10 by restoring the standard refusal (see D3; `/login?next=`, `HX-Redirect` and the `safe_path_of_url` move dropped), 4 (`non_htmx_error_wrapper` uses the shared `htmx::is_htmx`), 5 (docs: list only proxies that set the headers), 7 (E2E for `/mcp`), 8 (`htmx::redirect` logs an invalid target); kept 6 (warn per refused request is the signal asked for) and 9 (the test-only `extract_session_cookie` composes the two production helpers).
- Review pass 5 (high, 10 candidates): fixed 1 (cookieless `POST /login` mints nothing), 3–4 (`pending_updates` and `wants_pending_count` read `HX-Request` through `htmx::is_htmx`; the modal guard's own reader was already case-insensitive), 5 (the cookieless check runs before every other refusal), 9 (`htmx::redirect` takes a `&'static str`, no failure branch); 2 (a cookieless GET still mints) added to #699; kept 6 (a hop beyond an unreadable entry cannot be trusted: the last trusted hop is the honest answer), 7 (documented: list only proxies that set the headers), 8 (the warn per refusal is the signal asked for), 10 (test-only composition of the two production helpers).
