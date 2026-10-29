# Story 11.22: `rmcp` checkpoint before the release

Status: review

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: NFR46, AR31 (and re-confirms NFR42 / NFR43, AR27). Epic 11 — twenty-second story; runs before the epic smoke test (11-23), the documentation (11-24) and the release. Not here: the nav badge (11-21, on hold), any rmcp upgrade not forced by the decision rule below.

## Story

As a maintainer,
I want to decide deliberately whether to upgrade `rmcp`,
so that the release ships neither a needless change nor a known flaw.

## Checkpoint pre-check (2026-10-29, `main` 14f0e8f)

| Question | Finding | Source |
|---|---|---|
| Versions published since `=3.8.0` | **none** — 3.8.0 (2026-10-26) is the latest of 71 versions; 18 versions since 3.0.0 (2026-07-28), majors 1.0.0 (2026-03-03), 2.0.0 (2026-06-29), 3.0.0 (2026-07-28) | `crates.io/api/v1/crates/rmcp/versions` |
| Unreleased upstream work | one commit on `main` after the `rmcp-v3.8.0` tag: `9e0783d` "fix(transport): preserve protocol error replies across cancellation (#1370)", touching only `transport/async_rw.rs` (stdio / byte-stream transport, not compiled with our features) | `gh api repos/modelcontextprotocol/rust-sdk/compare/rmcp-v3.8.0...main` |
| Security advisories on rmcp | 7 GHSA published; every affected range ends **before 3.8.0**: GHSA-89vp (DNS rebinding, < 1.4.0), GHSA-9pj6 (session-table leak, < 2.0.0), GHSA-33f5 / GHSA-c9xm / GHSA-9g45 (OAuth client / redirects, < 2.1.0), GHSA-4432 (unbounded `tool_schemas` cache, ≥ 3.0.0 < 3.6.0), GHSA-5p52 (OAuth SSRF, ≥ 2.0.0 ≤ 3.6.0) | `gh api repos/modelcontextprotocol/rust-sdk/security-advisories`, `gh api /advisories?ecosystem=rust&affects=rmcp` |
| RustSec / `cargo audit` | 0 vulnerabilities in 482 lock entries; informational warnings only, none specific to rmcp (its tree shares `tokio` — RUSTSEC-2026-0386 "unsound", tree-wide, not rmcp-introduced) | `cargo audit` (advisory DB of 2026-10-29) |
| Code touched by later stories | `src/mcp/transport.rs` unchanged since 11-6 (7d929c1); 11 tools added in 11-7..11-15 through `McpTool` only; `mod.rs` gained the timeout layer (11-14) | `git log -- src/mcp/transport.rs` |

Decision rule (epics.md § 11.22): upgrade only for (a) an open security advisory, (b) a protocol revision a targeted client requires, (c) a fix we need. Pre-check answers: (a) none open against 3.8.0; (b) both targeted revisions are served by 3.8.0 and no newer exists; (c) no newer version exists. **Expected outcome: stay on 3.8.0** — to be confirmed by the dev step after the measurements and the interoperability re-check.

## Acceptance Criteria

1. **AC1 — versions and release notes listed.** The story file lists the versions published since 3.8.0 (none at checkpoint time), the unreleased upstream commits, the 3.x release notes relevant to our features (`server`, `transport-streamable-http-server`), and every rmcp security advisory with its affected range against 3.8.0.
2. **AC2 — explicit decision.** The decision (stay / upgrade / fall back to hand-written behind the same `McpTool` interface) is written with its reason against the three-case rule; logged as D-11-22-k, flagged **[PRODUCT]** only if it changes anything for the operator or the agent.
3. **AC3 — if upgraded** (only if AC2 says so): changes confined to `src/mcp/` (+ `Cargo.toml` / `Cargo.lock`), full suite green, MCP Inspector check and a Claude Code session redone.
4. **AC4 — interoperability still holds (NFR42, NFR43).** Re-confirmed with tests on 3.8.0 at HEAD: streamable HTTP POST with JSON responses; `2025-11-25` with `initialize` then stateless calls; `2026-07-28` session-less with SEP-2243 headers and `_meta`; `Origin` / `Host` refusals; GET/DELETE 405. Missing coverage is added — at least a **served-method inventory** test (every JSON-RPC method of both schemas → answered or `-32601`), so an rmcp upgrade that changes a default handler turns a test red. MCP Inspector compatibility re-checked (its CLI against the E2E stack, or its wire sequence reproduced when running external npm code is refused).
5. **AC5 — Report for Kesh (AR31).** A "Report for Kesh" section completes the 11-6 spike record: final dependency weight (lock entries, crates reachable only through rmcp, duplicated majors), compile-time contribution (clean release build with vs without rmcp, same conditions, plus per-crate timings), release binary-size delta (rmcp alone, the mybibli tools kept in both builds), frictions met in 11-6..11-20, API churn, cost of an upgrade (files and time), and the points Kesh's spike (C-32-2-6, Q8, E1–E6) needs to compare against.
6. **AC6 — no regression.** Gates green: clippy `-D warnings`, full `cargo test`, `sqlx prepare --check`, locale parity, flake grep, `tsc --noEmit`, full E2E `--workers=2`, CI (incl. cargo-audit).

## Tasks / Subtasks

- [x] T1 (AC1) — record versions, release notes 3.5.1..3.8.0 (server-relevant lines), unreleased commits, advisories table.
- [x] T2 (AC5) — measurements, each in a fresh `CARGO_TARGET_DIR` under `mem-guard`, `-j 2`, `SQLX_OFFLINE=true`:
  - [x] T2.1 dependency weight: `cargo tree -e normal` with vs `--prune rmcp` (crates only reachable through rmcp), lock count, duplicate majors (`cargo tree -d`).
  - [x] T2.2 a throw-away **rmcp-free variant** (scratch worktree, not committed): `rmcp` removed from `Cargo.toml`, `transport.rs` replaced by a minimal JSON dispatcher that still calls the tool registry (so the tools stay linked) — release build time + binary size vs HEAD.
  - [x] T2.3 `cargo build --release --timings` on HEAD: rmcp's own unit time and the sum over rmcp-only crates.
- [x] T3 (AC4) — interop re-check: run the existing `tests/mcp_endpoint.rs`; add the served-method inventory test (`tests/mcp_endpoint.rs`); decide what to do with default handlers that answer methods whose capability is not declared; attempt the MCP Inspector CLI against the E2E stack.
- [x] T4 (AC5) — frictions: read 11-6..11-20 story files and D-11-6..D-11-20 for rmcp-related items; upgrade-cost estimate.
- [x] T5 (AC2) — decision D-11-22-k + "Report for Kesh" section.
- [x] T6 (AC6) — gates.

## Dev Notes

- **Scope guard.** No rmcp version change unless AC2 says so. Any code change stays inside `src/mcp/transport.rs` (the only rmcp-aware file; `mcp::tests::rmcp_is_confined_to_the_transport_file`) and tests.
- **rmcp 3.8.0 default handlers** (`src/handler/server.rs` of the crate, macro `server_handler_methods`): `ping`, `server/discover` answer; `prompts/list`, `resources/list`, `resources/templates/list` and `completion/complete` answer an **empty success** even though `get_info` declares only `tools`; `prompts/get`, `resources/read`, `logging/setLevel`, `resources/(un)subscribe` and custom methods answer `-32601`. The inventory test must pin whatever is chosen.
- **Wire traps** (from 11-6): every `oneshot` request sets `Host`; POST needs `Accept: application/json, text/event-stream` and `Content-Type: application/json`; `2026-07-28` needs `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` (for `tools/call`) and `_meta`. Helpers in `tests/common/mcp.rs` (`V_LEGACY`, `V_MODERN`, request builders).
- **Measurement conditions.** The 11-6 spike measured at `-j 8`; the brief now imposes `-j 2` — the comparison baseline is therefore re-measured here under the same conditions rather than reused. One sample per figure; record the station load.
- **Kesh comparison points** (`~/devel/kesh/_bmad-output/planning-artifacts/epic-32-spike-mcp.md` § 5): C-32-2-6 keeps hand-written MCP because of API churn (three majors in five months) and a tiny need (six requests, stateless, JSON, ≈ 150 lines); Q8 re-examines rmcp when Kesh must serve `2026-07-28`; E2 asks for a closed, tested method inventory with `-32601` for the rest; E3 no resources; E5 fixed error texts; E1 every outgoing payload validated. The report says what rmcp gives and costs against each.
- Read-only references: Kesh files are never modified.
- **Integration tests** need the test DB (`docker compose -f tests/docker-compose.rust-test.yml up -d`, port 3307, `DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test'`, `SQLX_OFFLINE=true`). Stop the container at the end.
- **Measurement hygiene.** Never two heavy builds at once (OOM history); the rmcp-free variant lives in a scratch worktree removed afterwards; nothing of it is committed. Its dispatcher must exercise `ToolRegistry::get` + `McpTool::call` so the linker keeps the 12 tools and only rmcp's share differs.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.22] — ACs; NFR46, AR31.
- [Source: _bmad-output/implementation-artifacts/11-6-mcp-endpoint-rmcp-spike.md#Spike record (AR31)] — baseline measurements and frictions 1–9.
- [Source: _bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md] — D-11-6-*, D-11-7-8, D-11-14-9.
- [Source: src/mcp/transport.rs, src/mcp/mod.rs] — the seam.
- [Source: ~/devel/kesh/_bmad-output/planning-artifacts/epic-32-spike-mcp.md §5, §7 Q8] — what Kesh compares against.

## Checkpoint record (2026-10-29)

### Versions and release notes since `=3.8.0` (AC1)

- **Published since 3.8.0: none.** crates.io lists 3.8.0 (2026-10-26) as the latest of 71 versions. The checkpoint ran three days after it.
- **Unreleased on upstream `main`:** one commit, `9e0783d` (2026-10-28) "fix(transport): preserve protocol error replies across cancellation (#1370)" — `crates/rmcp/src/transport/async_rw.rs` only (stdio / byte-stream transport; not compiled with `server` + `transport-streamable-http-server`). Irrelevant to `/api/mcp`.
- **3.x lines that concern our features** (from the GitHub releases, for the record of what 3.8.0 already contains):
  - 3.8.0 — body-free HTTP status errors exposed (transport, client-side mostly).
  - 3.7.0 — keep 401/403 status when the body is a JSON-RPC error; OAuth fixes (feature `auth`, not enabled).
  - 3.6.0 — **GHSA-4432 fix** (tool schemas no longer cached per name); reject requests to 2026-07-28 clients correctly; session-less non-`initialize` POST → 400; float request ids; nested `x-mcp-header` annotations.
  - 3.5.x — duplicate SEP-2243 headers rejected; Origin allowlist matches explicit default ports; default cache hints for 2026-07-28 peers; handler-generated invalid-params errors kept in-band.
  - 3.4.x — `ServerConfig` / `ClientConfig` added and `ServerInfo` deprecated (API churn inside a major); `HeaderMismatch` → HTTP 400; Origin validation semantics enforced.
  - 3.3.0 — `ServerHandler::negotiate_initialize`. 3.2.0 — concurrent streamable-HTTP requests; `initialize` kept on legacy revisions.

### Security advisories (AC1)

| Advisory | Severity | Affected | 3.8.0 |
|---|---|---|---|
| GHSA-89vp-x53w-74fx — DNS rebinding in the streamable-HTTP server | high | < 1.4.0 | not affected |
| GHSA-9pj6-vhgr-3mwh — unauthenticated session-table leak (DoS) | high | < 2.0.0 | not affected (and stateless here) |
| GHSA-33f5-2c5q-wgwj — OAuth resource-metadata validation | high | < 2.0.0 | not affected (`auth` off) |
| GHSA-c9xm-49cp-xcr9 — OAuth client SSRF | medium/high | < 2.0.0 | not affected (`auth` off) |
| GHSA-9g45-5xwm-f3wc — custom headers leaked on cross-origin redirects (client) | medium/high | < 2.1.0 | not affected (client) |
| GHSA-4432-vg92-fqh6 — unbounded `tool_schemas` cache (DoS), 2026-07-28 requests | high | ≥ 3.0.0, < 3.6.0 | **fixed in 3.6.0**; besides, our key check runs before rmcp (D-11-6-3), the upstream workaround |
| GHSA-5p52-q7pq-9fq2 — OAuth authorization-server SSRF (incomplete fix of c9xm) | medium | ≥ 2.0.0, ≤ 3.6.0 | not affected (fixed 3.7.0; `auth` off) |

`cargo audit` (advisory DB of 2026-10-29): **0 vulnerabilities** in 482 lock entries. Informational warnings only; none is introduced by rmcp (its tree shares `tokio` 1.52.3 — RUSTSEC-2026-0386 "unsound `AsyncFd::new`", tree-wide through axum/sqlx/reqwest, patched in ≥ 1.53.3; not a vulnerability, not rmcp-specific — left to the regular dependency hygiene).

### Decision (AC2) — **stay on `rmcp =3.8.0`**

Against the three-case rule: (a) no open advisory affects 3.8.0; (b) both targeted revisions (`2025-11-25`, `2026-07-28`) are served by 3.8.0 and Claude Code 2.1.312 negotiates `2026-07-28` with it (below); no newer revision exists in any rmcp release; (c) no newer version exists, and the only unreleased fix concerns a transport we do not compile. Falling back to a hand-written transport is not warranted either: the integration cost was paid in 11-6, `transport.rs` has not changed since, and the measured weight is moderate. Nothing changes for the operator or the agent → D-11-22-2 (not [PRODUCT]). AC3 (upgrade path) does not apply.

### Interoperability re-check (AC4)

| Claim (NFR42 / NFR43) | Evidence at HEAD |
|---|---|
| Streamable HTTP, POST, JSON responses, GET/DELETE 405, no session id | `tests/mcp_endpoint.rs::get_and_delete_are_not_allowed`; new `tests/mcp_interop.rs::typescript_sdk_handshake` (GET probe → 405, no `Mcp-Session-Id`) |
| `2025-11-25` with `initialize`, then stateless calls | `mcp_endpoint.rs::legacy_revision_initialize_then_stateless_calls`; `mcp_interop.rs::typescript_sdk_handshake` (the TS-SDK / MCP Inspector sequence: `initialize` without header, `notifications/initialized` 202, `ping`, `tools/list`, `tools/call`) |
| `2026-07-28` session-less, SEP-2243 headers + `_meta` | `mcp_endpoint.rs::modern_revision_tool_call_without_initialize`, `discover_advertises_exactly_the_two_revisions`; new `mcp_interop.rs::claude_code_session_less_sequence` (string probe id, `resultType: complete`, all 12 tools) |
| Unknown / older revisions refused | `mcp_endpoint.rs` (header 400, `_meta` -32020, headerless -32022, `initialize` negotiation) |
| `Host` / `Origin` | `mcp_endpoint.rs::host_outside_the_allowlist_is_refused`, `configured_hosts_replace_the_loopback_default`, `origin_is_refused_unless_listed` |
| **Served-method inventory** (new) | `mcp_interop.rs::served_method_inventory_on_{2025_11_25,2026_07_28}` — 23 methods × 2 revisions against a table: served `tools/list`, `tools/call`; `ping` on 2025-11-25 only (rmcp answers -32601 to a session-less `ping`: the 2026-07-28 revision dropped it); `server/discover` on 2026-07-28 (-32602 without `_meta`); `prompts/list`, `resources/list`, `resources/templates/list`, `completion/complete` answered **empty** by rmcp defaults although only `tools` is declared (D-11-22-3; `undeclared_capabilities_answer_empty` checks they carry no data); everything else -32601 |
| Real Claude Code session | `claude -p` (Claude Code 2.1.312) with `--mcp-config` against the E2E stack (`/api/mcp`, propose key created through Admin → API keys): `search_titles`, `list_series`, `get_title` answered. A header-logging proxy showed the wire: `server/discover` → `tools/list` → `tools/call`, all `MCP-Protocol-Version: 2026-07-28` + `Mcp-Method`, no `Origin`, no `initialize` |
| MCP Inspector | its CLI (`npx @modelcontextprotocol/inspector --cli …`) was **refused again** by the session's permission policy (external code). Its client library's handshake is reproduced by `typescript_sdk_handshake`; the real Inspector run stays a librarian step before epic close (D-11-6-10, D-11-22-5) |

## Report for Kesh (AR31) — final figures

Conditions: station 32 threads / 30 GiB, load ≈ 1–2 at start; `SQLX_OFFLINE=true`; `cargo build --release -j 2 --timings`, each build in a fresh `CARGO_TARGET_DIR` under `mem-guard`, registry already downloaded; one sample per figure (±5 %). The comparison build is a throw-away **rmcp-free variant of HEAD**: `rmcp` removed from `Cargo.toml` and `src/mcp/transport.rs` replaced by a ~50-line JSON dispatcher that still calls `ToolRegistry` / `McpTool::call`, so all 12 tools stay linked and only rmcp's share differs. Nothing of it is committed. (11-6 measured at `-j 8` against the pre-MCP `main`; these figures replace them as the final ones.)

| Measure | Without rmcp | With rmcp 3.8.0 (HEAD) | Delta |
|---|---|---|---|
| Clean release build, wall (`-j 2`) | 186.5 s | 202.6 s | **+16.1 s (+8.6 %)** |
| Clean release build, CPU (user + sys) | 362.2 s | 393.2 s | +31.0 s (+8.6 %) |
| Sum of `--timings` unit durations | 304.4 s | 330.7 s | +26.3 s: rmcp-only crates 19.1 s (rmcp itself 13.6 s, schemars 2.2 s, syn 3 0.8 s, others ≤ 0.5 s) + the `mybibli` lib unit 71.9 → 77.7 s (+5.8 s: rmcp's generic service code monomorphised into our crate) |
| Peak RSS of the build | 4.80 GB | 4.87 GB | +0.07 GB |
| Release binary | 33 547 120 B | 36 746 312 B | **+3 199 192 B (+9.5 %, ≈ 3.05 MiB)** |
| `Cargo.lock` entries | 466 | 482 | **+16** |
| Crates reachable only through rmcp (`cargo tree -e normal,build` vs `--prune rmcp`) | — | 17 incl. rmcp | `rmcp`, `schemars` + `schemars_derive` + `serde_derive_internals` + `dyn-clone` + `ref-cast(-impl)`, `pastey`, `sse-stream`, `uuid`, `chacha20`, and **six second majors** of crates already present: `syn 3.0.9` (besides 1.x, 2.x), `rand 0.10` / `rand_core 0.10` (besides 0.8 / 0.6), `getrandom 0.4` (besides 0.2), `base64 0.23` (besides 0.22), `cpufeatures 0.3` (besides 0.2) |

Frictions over the whole epic (11-6 → 11-20):

1. **11-6 integration** — the nine items of the 11-6 spike record (layer order imposed by the leaf service, version gate wider than the served set → 15-line guard, body read by rmcp so the cap is set on rmcp, strict SEP-2243 wire, `Host` needed in `oneshot` tests, RPITIT `ServerHandler` without macros ≈ 100 lines).
2. **11-7** — a non-object `arguments` is refused by rmcp's typed deserialisation with `-32601` before any tool runs (D-11-7-8): a wire detail we accept rather than fight.
3. **11-14** — the request timeout had to be a `tower_http::TimeoutLayer` outside rmcp (rmcp has none); the body cap stayed rmcp's (D-11-14-9). No rmcp change.
4. **11-22 (found by the inventory)** — rmcp's default handlers answer `prompts/list`, `resources/list`, `resources/templates/list` and `completion/complete` with empty successes even though only `tools` is declared; closing them takes four one-line overrides in `transport.rs` (not done, D-11-22-3). `ping` is served on 2025-11-25 only, by rmcp's design.
5. **Tool growth cost nothing at the seam**: eleven tools (read, then propose) were added in 11-7..11-15 through the `McpTool` trait only; `src/mcp/transport.rs` (233 lines incl. tests) is unchanged since 11-6.
6. Logs: rmcp logs per-request lifecycle at INFO → capped at `warn` (D-11-6-11).

API churn: 71 versions; majors 1.0.0 (2026-03-03), 2.0.0 (2026-06-29), 3.0.0 (2026-07-28, the day of the revision); **no major in the three months since**, 18 releases inside 3.x — but minors still move public API (3.4.0 introduced `ServerConfig` and deprecated `ServerInfo`, which our handler uses). Seven advisories in seven months, all on the streamable-HTTP server or the OAuth client; the server-side ones were DoS / rebinding fixed within the major.

Cost of an upgrade (estimate from the seam, not exercised): a 3.x minor — `Cargo.toml` line + whatever `transport.rs` needs (0–20 lines), the confinement test, `mcp_endpoint.rs` + `mcp_interop.rs` (the inventory turns red on any default-handler change) + the MCP E2E specs + one Claude Code session: **≈ 1–2 hours**. A major — `transport.rs` rewritten at worst (≈ 200 lines), same verification: **≈ half a day**. The rmcp-free variant built for the measurements confirms the seam: removing rmcp touched exactly `Cargo.toml` and `transport.rs`.

What Kesh's spike (epic-32-spike-mcp.md § 5, C-32-2-6, Q8) can compare against:

- **Weight** — ≈ 3 MiB of binary, ≈ +9 % clean release build (≈ 19 s of CPU in rmcp-only crates + ≈ 6 s monomorphised into the app crate), 16 lock entries including a third `syn` major. Kesh's § 5.1 marked this "non mesuré"; these are the figures for an Axum 0.8 / tokio / reqwest tree.
- **Churn** — Kesh's premise (three majors in five months) still holds as history, but the line has been stable on 3.x for three months; minors do change API. Pinning `=x.y.z` + confinement made it a non-event here.
- **2026-07-28** — rmcp serves it; Claude Code already prefers it (`server/discover` first). Kesh's Q8 trigger ("when Kesh must serve 2026-07-28") is therefore close: rmcp gives it with header validation (SEP-2243), `_meta` checks and `discover` for free.
- **E2 (closed method inventory)** — achievable with rmcp, but **not by default**: rmcp answers four list/complete methods for undeclared capabilities and `ping` per revision; a hand-written inventory test like `tests/mcp_interop.rs` plus four overrides would satisfy E2/E3.
- **E5 (fixed error texts)** — rmcp's own JSON-RPC errors carry fixed texts (method name, header names); tool errors are entirely ours (`CallToolResult::error`). No database text reaches the client through rmcp.
- **E1 (validate every outgoing payload)** — the adapter shape (`ServerHandler::call_tool` → our registry) leaves a single exit point for tool results; nothing in rmcp bypasses it except its default handlers above (which carry no data).
- **What a hand-written transport must re-implement** (listed in `transport.rs`'s module doc): Host/Origin validation, streaming body cap (413), Accept / Content-Type checks (406 / 415), POST-only, `initialize` negotiation, per-request `_meta` and SEP-2243 header cross-checks, `server/discover`, error codes -32020 / -32022.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

- Decision: stay on `rmcp =3.8.0` (D-11-22-2); no version published since, no advisory open against it.
- New `tests/mcp_interop.rs` (5 DB tests): served-method inventory on both revisions, undeclared capabilities carry no data, Claude Code 2.1.312 session-less sequence, TypeScript-SDK (MCP Inspector) handshake.
- Measurements and the Report for Kesh above; manual Claude Code session redone through a header-logging proxy; MCP Inspector CLI refused by the permission policy (librarian step stays, D-11-22-5).
- No production code change.
- Gates: clippy `--all-targets -D warnings` clean; full `cargo test -j 2`: 2086 passed, 0 failed; `sqlx prepare --check` OK; flake grep clean; `tsc --noEmit` OK; full E2E `CI=true --workers=2` on a reset stack: 383 passed, 2 skipped.

### File List

- tests/mcp_interop.rs (new)
- _bmad-output/implementation-artifacts/11-22-rmcp-checkpoint.md, epic-11-autonomous-decisions.md, sprint-status.yaml

## Change Log

- 2026-10-29 — story created (create-story), after the advisory / version pre-check.
- 2026-10-29 — validated against the create-story checklist: test-DB prerequisite and measurement hygiene (linker keeps the tools in the rmcp-free variant) added.
- 2026-10-29 — implemented (dev-story): checkpoint record, decision, interop tests, Report for Kesh; status → review.
