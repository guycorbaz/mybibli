# Story 11.22: `rmcp` checkpoint before the release

Status: ready-for-dev

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

- [ ] T1 (AC1) — record versions, release notes 3.5.1..3.8.0 (server-relevant lines), unreleased commits, advisories table.
- [ ] T2 (AC5) — measurements, each in a fresh `CARGO_TARGET_DIR` under `mem-guard`, `-j 2`, `SQLX_OFFLINE=true`:
  - [ ] T2.1 dependency weight: `cargo tree -e normal` with vs `--prune rmcp` (crates only reachable through rmcp), lock count, duplicate majors (`cargo tree -d`).
  - [ ] T2.2 a throw-away **rmcp-free variant** (scratch worktree, not committed): `rmcp` removed from `Cargo.toml`, `transport.rs` replaced by a minimal JSON dispatcher that still calls the tool registry (so the tools stay linked) — release build time + binary size vs HEAD.
  - [ ] T2.3 `cargo build --release --timings` on HEAD: rmcp's own unit time and the sum over rmcp-only crates.
- [ ] T3 (AC4) — interop re-check: run the existing `tests/mcp_endpoint.rs`; add the served-method inventory test (`tests/mcp_endpoint.rs`); decide what to do with default handlers that answer methods whose capability is not declared; attempt the MCP Inspector CLI against the E2E stack.
- [ ] T4 (AC5) — frictions: read 11-6..11-20 story files and D-11-6..D-11-20 for rmcp-related items; upgrade-cost estimate.
- [ ] T5 (AC2) — decision D-11-22-k + "Report for Kesh" section.
- [ ] T6 (AC6) — gates.

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

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-10-29 — story created (create-story), after the advisory / version pre-check.
- 2026-10-29 — validated against the create-story checklist: test-DB prerequisite and measurement hygiene (linker keeps the tools in the rmcp-free variant) added.
