# Story 11.6: MCP endpoint on `/api/mcp` with `rmcp` (spike + foundation)

Status: ready-for-dev

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR127, NFR42, NFR43, NFR45, NFR46, AR27, AR28, AR31. Epic 11 — sixth story; blocks every MCP tool story (11-7 onwards). Prerequisite delivered by 11-5: `ApiKeyScope::Propose` + the `ApiKeyPropose` extractor (`src/middleware/api_key.rs`), today mounted only on a test route.

## Story

As a librarian using Claude Code,
I want to connect my AI agent to mybibli,
so that it can start exploring the catalog.

## Spike pre-check (2026-10-28) — `rmcp 3.8.0` is usable as the epic assumed

Verified on crates.io and in the crate source (`~/.cargo/registry/src/*/rmcp-3.8.0`) before writing this spec:

| Epic assumption | What 3.8.0 actually offers | Consequence |
|---|---|---|
| version `=3.8.0` exists | yes (`cargo search rmcp` → 3.8.0, MSRV 1.88, Apache-2.0) | pin `rmcp = { version = "=3.8.0", default-features = false, features = ["server", "transport-streamable-http-server"] }` — no `macros` (tools are declared through our own trait, not `#[tool]`) |
| streamable HTTP server, axum integration | `transport::streamable_http_server::StreamableHttpService<S, M>` is a `tower::Service<http::Request<B>>`; mounted with `Router::nest_service` | mount at `/api/mcp` |
| stateless mode | `StreamableHttpServerConfig::with_legacy_session_mode(false)` + `session::never::NeverSessionManager` (or `LocalSessionManager`, unused when stateless); `with_json_response(true)` answers plain JSON for request/response tools | no in-memory session table; one fresh handler per request |
| revisions `2025-11-25` (with `initialize`) and `2026-07-28` (session-less) | `ProtocolVersion::V_2025_11_25` / `V_2026_07_28`; `ServerHandler::supported_protocol_versions()` bounds negotiation; 2026-07-28 requests carry `_meta["io.modelcontextprotocol/protocolVersion"]` + `clientCapabilities` and the `MCP-Protocol-Version` / `Mcp-Method` / `Mcp-Name` headers (SEP-2243, validated by rmcp) | override `supported_protocol_versions` to exactly these two |
| unknown revision refused | an unknown `MCP-Protocol-Version` header → HTTP 400 `Unsupported MCP-Protocol-Version`; an unknown per-request `_meta` version → JSON-RPC error `UNSUPPORTED_PROTOCOL_VERSION`; an `initialize` naming an unknown version is answered with `2025-11-25` (the spec's negotiation rule — the client then decides) | "refused cleanly" = the two first cases; the third is the spec-mandated negotiation (D-11-6-4) |
| `Origin` / `Host` validation | built in: `allowed_hosts` (default loopback only; entry without port = any port), `allowed_origins` + `enforce_origin_validation()` (empty list ⇒ every present `Origin` refused; missing `Origin` passes) → 403 | configured explicitly from mybibli config (D-11-6-2) |
| explicit max body size | `with_max_request_body_bytes(n)`, enforced while streaming → 413 | set explicitly (D-11-6-5) |
| per-request HTTP data in handlers | rmcp inserts `http::request::Parts` into `RequestContext.extensions` | the `ApiKeyContext` inserted by our auth layer reaches the tool adapter |

Conclusion: no fallback to a hand-written transport is needed. The fallback seam (AR27) still exists by construction: everything rmcp-specific lives in `src/mcp/transport.rs`.

## Existing-code reality check (code map, 2026-10-28)

| Place | Today | Consequence |
|---|---|---|
| `src/middleware/api_key.rs` | `ApiKeyPropose` extractor (401 missing/invalid, 403 `insufficient_scope` / `scope_required: propose`) | reused as-is in an axum `from_fn_with_state` layer wrapping the rmcp service; on success inserts `ApiKeyContext` into request extensions |
| `src/middleware/csrf.rs:140` | `path.starts_with("/api/")` short-circuit | `/api/mcp` inherits it; `csrf_exempt_routes_frozen` untouched (AR28) |
| `src/middleware/auth.rs::should_skip_session_resolve` | skips `/health`, `/static`, `/covers`, `/logo` only | every `/api/*` request mints an anonymous `sessions` row; an agent calling tools in a loop would write one row per call → add `/api/mcp` to the skip list (D-11-6-6); `/api/v1` keeps today's behaviour (follow-up issue) |
| `src/middleware/setup_gate.rs` | redirects every non-whitelisted path while the wizard is active | `/api/mcp` behaves like `/api/v1` (303 to `/setup` before first launch) — no change |
| `src/middleware/non_htmx_error_wrapper.rs` | rewraps only `text/html` error bodies | rmcp's `text/plain` 4xx bodies pass through untouched |
| `src/routes/api_v1.rs` | `ApiTitleDetail` DTOs + private `build_title_detail` (GET + PATCH) — the assembly lives in a route file | move the DTOs + assembly to `src/services/title_detail.rs` (`TitleDetailService::find`); `api_v1` re-exports the DTO names so its JSON shape and tests stay identical; the MCP `get_title` tool calls the service (Rule #1, AR27 "through `services/`") |
| `src/services/cover.rs` | `MAX_COVER_SIZE = 10 MiB` (private) | made `pub(crate)`; the MCP body limit is derived from it (base64 ×4/3 + 1 MiB envelope) |
| `src/config.rs` | `Config::from_env` (DATABASE_URL, HOST, PORT, APP_LANGUAGE) + one-shot env readers (`csp_report_only`, `cookie_secure`) | new `McpConfig::from_env()` in `src/mcp/config.rs` (`MYBIBLI_MCP_ALLOWED_HOSTS`, `MYBIBLI_MCP_ALLOWED_ORIGINS`), read once in `build_router` |
| `src/routes/mod.rs::build_router(state)` | used by `main.rs` and ~30 integration tests | becomes a thin wrapper over `build_router_with_mcp(state, McpConfig::from_env())`; tests that need specific hosts call the latter (no `set_var` in parallel tests) |

## Acceptance Criteria

1. **AC1 — confinement (AR27, NFR46).** `rmcp = "=3.8.0"` is referenced only from `src/mcp/` (and its own `Cargo.toml` line). Tools implement the mybibli-owned `McpTool` trait (`src/mcp/tool.rs`), whose signature uses only mybibli and `serde_json::Value` types — no `rmcp` or JSON-RPC type crosses it. A unit test walks `src/` and fails if `rmcp::` appears outside `src/mcp/`; inside `src/mcp/`, only `transport.rs` may name `rmcp`.
2. **AC2 — both revisions (NFR42).** With a `propose` key, `POST /api/mcp`: (a) `initialize` with `protocolVersion: "2025-11-25"` → result `protocolVersion: "2025-11-25"`, server info and the `tools` capability; then `tools/list` and `tools/call get_title` without any session id (`MCP-Protocol-Version: 2025-11-25`) succeed; (b) a `tools/call get_title` with `MCP-Protocol-Version: 2026-07-28`, `Mcp-Method` / `Mcp-Name` headers and the per-request `_meta` (protocol version + client capabilities), with no `initialize` beforehand, succeeds; `server/discover` lists exactly the two supported revisions.
3. **AC3 — unknown revision refused cleanly.** An unknown `MCP-Protocol-Version` header → 400 with a plain-text reason; an unknown `_meta` protocol version → a JSON-RPC error (no tool executed); an `initialize` naming an unknown version is answered with `2025-11-25` (spec negotiation, D-11-6-4).
4. **AC4 — scope matrix.** No key → 401 `missing_api_key`; an invalid key → 401; a `read` or `write` key → 403 `insufficient_scope` / `scope_required: propose`; a revoked key → 401. The refusal happens before any MCP processing (JSON envelope of `api_key.rs`).
5. **AC5 — `Origin` / `Host` (NFR43).** A `Host` outside the allowed hosts → 403; an `Origin` header outside the allowed origins → 403 (default: any present `Origin` refused, since MCP clients are not browsers); a request without `Origin` passes. The operator configures `MYBIBLI_MCP_ALLOWED_HOSTS` (comma-separated host or `host:port`; default `localhost,127.0.0.1,::1`; replaces the default when set — the NAS deployment lists its LAN name/IP) and `MYBIBLI_MCP_ALLOWED_ORIGINS` (comma-separated `scheme://host:port`; default empty). Documented in `docs/auth-threat-model.md`, `docs/route-role-matrix.md`, `README.md` (env table) and the commented `docker-compose.yml`.
6. **AC6 — one real read tool.** `get_title` (input `{ "id": integer ≥ 1 }`) returns the title detail JSON (same shape as `GET /api/v1/titles/{id}`) as `structuredContent` plus a text block; it goes through `services::title_detail` (no SQL in `src/mcp/`). An unknown or soft-deleted id → a tool result with `isError: true` and a plain message; a missing/invalid `id` → `isError: true` too (the agent can self-correct). An unknown tool name → JSON-RPC error.
7. **AC7 — body size (NFR45).** The maximum request body on `/api/mcp` is set explicitly: `MAX_COVER_SIZE` (10 MiB) base64-inflated (×4/3, rounded up) + 1 MiB JSON envelope ≈ 14.3 MiB; an oversized POST → 413.
8. **AC8 — CSRF (AR28).** `/api/mcp` works without a CSRF token through the `/api/*` short-circuit; `csrf_exempt_routes_frozen` stays green, the allowlist is unchanged.
9. **AC9 — tests.** Rust integration tests (`tests/mcp_endpoint.rs`, full `build_router_with_mcp`, isolated DB) cover AC2–AC8; unit tests cover config parsing, the body-limit arithmetic, the confinement walk and the tool registry; one E2E journey creates a `propose` key in Admin → API keys and drives `initialize` → `tools/list` → `tools/call get_title` through `page.request`. The MCP Inspector check and a real Claude Code session are manual steps recorded in the PR (D-11-6-8).
10. **AC10 — spike record (AR31).** The story file records crates added, clean release build-time delta, release binary-size delta, and the frictions met with Axum 0.8 and the middleware stack — input for story 11-22 and Kesh Q8.

## Tasks / Subtasks

- [ ] T1 — Dependency (AC1): add `rmcp` pinned `=3.8.0`, `default-features = false`, features `["server", "transport-streamable-http-server"]`; record the `Cargo.lock` package delta. Run `cargo audit` locally if available.
- [ ] T2 — `src/services/title_detail.rs` (AC6): move `ApiTitleDetail`, `ApiContributor`, `ApiVolume`, `ApiSeriesAssignment` and `build_title_detail` from `api_v1.rs`; add `TitleDetailService::find(pool, id) -> Result<Option<TitleDetail>, AppError>` (`TitleModel::find_by_id` already filters `deleted_at IS NULL`). `api_v1.rs` re-exports the names and calls the service; JSON shape unchanged (existing `tests/api_v1_*.rs` guard it).
- [ ] T3 — `src/mcp/` module (AC1, AC6):
  - `mod.rs` — module doc (architecture, confinement rule, fallback seam); `pub fn router(state: AppState, config: McpConfig) -> axum::Router<AppState>` returning the mounted service with its auth layer; `pub use config::McpConfig`.
  - `tool.rs` — `McpTool` trait (`name`, `description`, `input_schema() -> serde_json::Value`, `async call(&self, ctx: &ToolContext, args: serde_json::Value) -> Result<serde_json::Value, ToolError>` via boxed future), `ToolContext { state: AppState, api_key_id: u64 }`, `ToolError { InvalidArguments(String), NotFound(String), Internal(String) }`, `registry() -> Vec<Arc<dyn McpTool>>`.
  - `tools/get_title.rs` — the adapter: parse `id`, call `TitleDetailService::find`, serialize.
  - `config.rs` — `McpConfig { allowed_hosts, allowed_origins, max_request_body_bytes }`, `from_env()`, pure `parse_list()`; `max_request_body_bytes()` derived from `MAX_COVER_SIZE`.
  - `transport.rs` — the only rmcp-aware file: `MybibliMcpServer` implementing `rmcp::ServerHandler` (`get_info` with name/version/instructions + tools capability, `supported_protocol_versions` = [2025-11-25, 2026-07-28], `list_tools`, `get_tool`, `call_tool` translating `ToolError` into `CallToolResult::error`), and `StreamableHttpService` construction (stateless, JSON responses, hosts/origins/body limit from `McpConfig`, `enforce_origin_validation`, no SSE keep-alive).
- [ ] T4 — Auth layer (AC4): `from_fn_with_state` middleware in `src/mcp/mod.rs` that runs `ApiKeyPropose::from_request_parts` and inserts `ApiKeyContext` into the request extensions; rejection returned unchanged.
- [ ] T5 — Wiring (AC5, AC8): `routes::build_router_with_mcp(state, mcp_config)`; `build_router` delegates with `McpConfig::from_env()`; `.nest_service`/`.nest` at `/api/mcp`; `should_skip_session_resolve` gains `/api/mcp` (+ unit test).
- [ ] T6 — Docs (AC5): `docs/auth-threat-model.md` (MCP surface: bearer `propose` key, Host/Origin allowlists, body limit, LAN-only), `docs/route-role-matrix.md` (§ MCP), `README.md` env table, `docker-compose.yml` commented example. User manual = story 11-24.
- [ ] T7 — Unit tests (AC1, AC5, AC7, AC9): config parsing (default, override, trimming, empty items dropped), body-limit arithmetic, confinement walk over `src/`, registry names unique and schemas are objects, `ToolError` mapping, session-resolve skip.
- [ ] T8 — Integration tests (AC2–AC8): `tests/mcp_endpoint.rs` with `#[sqlx::test(migrations = "./migrations")]`, `build_router_with_mcp` + `tower::ServiceExt::oneshot`, keys minted through `ApiKeyModel` (pattern of `tests/api_key_propose_scope.rs`): both revisions, discover, unknown revision ×3, scope matrix (missing, invalid, read, write, revoked, propose), Host/Origin (bad host, bad origin, allowed origin, no origin), oversized body 413, get_title found / unknown / soft-deleted / bad args, unknown tool, no CSRF token needed, no `sessions` row created.
- [ ] T9 — E2E (AC9): `tests/e2e/specs/journeys/mcp-endpoint.spec.ts` — `loginAs(page, "admin")`, create a `propose` key via the UI, `page.request.post("/api/mcp", …)` initialize (2025-11-25) → tools/list contains `get_title` → tools/call `get_title` on a title created in the spec via the scan flow (`specIsbn("MC", n)`) → `structuredContent.title` matches; a `read` key gets 403; revoke the propose key at the end.
- [ ] T10 — Spike measurements (AC10): clean `cargo build --release -j 8` time and binary size on `main` (baseline) vs. the branch; `Cargo.lock` package count delta; frictions → § Spike record below.
- [ ] T11 — Manual checks (AC9): MCP Inspector (CLI mode) against the E2E stack and a real `claude` CLI session with the endpoint registered; transcripts in the PR.
- [ ] T12 — Gates: clippy `--all-targets -D warnings`, full `cargo test -j 1` (mem-guard), `cargo sqlx prepare --check`, locale parity (no new keys expected), flake grep, `tsc --noEmit`, full E2E `--workers=2`, CI incl. cargo-audit.

## Dev Notes

### Architecture (AR27)

```
HTTP ─► CSP ─► SetupGate ─► SessionResolve (skipped for /api/mcp) ─► Locale ─► … ─► CSRF (/api/* short-circuit)
     ─► /api/mcp: mcp_auth layer (ApiKeyPropose → Extension<ApiKeyContext>)
     ─► rmcp StreamableHttpService (Host/Origin 403, body 413, protocol checks, JSON-RPC)
     ─► MybibliMcpServer (transport.rs, ServerHandler) ─► McpTool registry (tool.rs) ─► services/
```

- Auth runs **before** the Host/Origin check (it is an axum layer around the rmcp leaf service). Acceptable: DNS rebinding targets unauthenticated local servers; here an attacker's page holds no bearer key, and a request without a key never reaches rmcp. Host/Origin remains defence in depth for a leaked key (D-11-6-3).
- The fallback seam: a hand-written transport would replace `transport.rs` only; `tool.rs`, `tools/`, `config.rs` and the auth layer stay.
- `ToolContext` carries `AppState` and the `api_key_id` (later stories attribute proposals to the key).

### Wire details to respect (rmcp 3.8.0)

- POST must send `Accept: application/json, text/event-stream` and `Content-Type: application/json` (rmcp answers 406 / 415 otherwise).
- `json_response(true)`: a request/response tool answers `Content-Type: application/json`; notifications → 202.
- Stateless: GET and DELETE answer 405 (`Allow: POST`). Fine for both revisions (no server-initiated stream needed).
- `2026-07-28` requests need the `Mcp-Method` header (`tools/call`) and `Mcp-Name` (tool name) matching the body (SEP-2243), else a JSON-RPC header-mismatch error.

- `tower::ServiceExt::oneshot` requests carry no `Host` header and an origin-form URI: rmcp then answers 400 `missing Host header`. Every integration-test request must set `Host: localhost` (or the host under test) explicitly.
- `nest_service("/api/mcp", …)`: rmcp ignores the path, so `/api/mcp` and `/api/mcp/` both reach it; only POST is served.
- The E2E journey reuses the key-creation steps of `tests/e2e/specs/journeys/admin-api-keys.spec.ts` (same selectors); if they are needed twice, extract a helper into `tests/e2e/helpers/` instead of copying (Rule #1).

### Configuration mechanism (decided here — AC5)

Env vars, read once at router build: they are deployment facts (which LAN name the NAS answers to), security-relevant, and must not be editable from a web page an attacker could reach. Not a `settings` row (D-11-6-2).

### Testing standards

Integration tests drive the full router (`build_router_with_mcp`) with `oneshot`; keys minted via `ApiKeyModel::create` / `mint_plaintext_key`; DB tests `#[sqlx::test(migrations = "./migrations")]`. E2E: `loginAs`, stable selectors, unique ISBN spec id `MC`, no `waitForTimeout`. Heavy commands under `mem-guard`, `cargo test -j 1`.

### Regression map

- `tests/api_v1_*.rs` (title detail shape after the move), `tests/api_key_propose_scope.rs`, `csrf_exempt_routes_frozen`, `templates_audit`, every test calling `build_router` (signature unchanged).

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.6] — ACs; FR127, NFR42–NFR46, AR27, AR28, AR31.
- [Source: src/middleware/api_key.rs] — `ApiKeyPropose`.
- [Source: src/middleware/csrf.rs:140] — `/api/*` short-circuit.
- [Source: rmcp-3.8.0 src/transport/streamable_http_server/tower.rs] — config, Host/Origin, stateless handling.
- [Source: ~/devel/kesh/_bmad-output/planning-artifacts/epic-32-spike-mcp.md §5] — Kesh's hand-written choice and Q8 (read only).
- [Source: _bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md] — D-11-6-*.

## Spike record (AR31) — filled during dev-story

| Measure | Baseline (`main` a63aebd) | With rmcp | Delta |
|---|---|---|---|
| Clean `cargo build --release -j 8` (wall) | 1 m 20.8 s | | |
| Release binary size | 30 000 992 B | | |
| `Cargo.lock` packages | 466 | | |
| Crates compiled in a clean release build | 351 | | |

Frictions: _to record_.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-10-28 — story created (create-story), after the rmcp 3.8.0 source pre-check.
- 2026-10-28 — validated against the create-story checklist: oneshot `Host` header trap, nest path note, E2E helper reuse added.
