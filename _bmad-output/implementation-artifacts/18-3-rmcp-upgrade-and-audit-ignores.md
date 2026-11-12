# Story 18.3: rmcp 3.9 and the audit ignores

Status: done

GitHub issue: #681 (the rmcp half; the sqlx half stays open). Covers: maintenance (NFR46 kept). Epic 18; ships in v1.28.0. Sprint-status key: `18-3-rmcp-upgrade-and-audit-ignores`.

## Story

As the maintainer,
I want the MCP server on a fixed rmcp and the Audit workflow green without the rmcp ignore,
so that a real advisory is no longer hidden behind an exception.

## Context

`.cargo/audit.toml` ignores RUSTSEC-2026-0462 (rmcp 3.8.0, unbounded response buffering in the Streamable HTTP **clients**; fixed in 3.9.0) since v1.27.0 — mybibli compiles no client transport, but the pin `=3.8.0` (NFR46, epic 11) blocked the fix. RUSTSEC-2026-0459 (`libsqlite3-sys` through sqlx's unused SQLite driver) needs the sqlx upgrade (major-upgrade epic) and stays.

rmcp 3.9.0 (2026-11-05) release notes, server-relevant items: stricter HTTP media-type / `Accept` quality matching (#1382), rejection of server-to-client requests on `discover` sessions (#1397), protocol error replies preserved across cancellation (#1370), stateful session workers honouring cancellation (#1427, we run stateless), `_meta`-only results not treated as `CallToolResult` (#1405), Skills extension SEP-2640 (#1392, opt-in; we serve skills as prompts + a tool, story 15-3).

## Decisions

- **D1 — re-pin exactly on the latest 3.x:** `rmcp = { version = "=3.9.0", … }`, same features (`server`, `transport-streamable-http-server`, default features off). NFR46 stays: exact pin, confined to `src/mcp/transport.rs` (`rmcp_is_confined_to_the_transport_file`).
- **D2 — API changes absorbed in `src/mcp/transport.rs` only.** If 3.9 changes a signature mybibli uses (`initialize` / `discover` overrides, `get_info`, the streamable HTTP service config), adapt there; no tool or service changes.
- **D3 — `Accept` strictness.** The E2E helper (`tests/e2e/helpers/mcp.ts`) and the Rust MCP test client (`tests/common/mcp.rs`) send what real clients send; if 3.9 refuses a header shape our tests used, align the tests only when the new behaviour matches the MCP spec, and record it (a real agent would hit it too).
- **D4 — audit.** Remove RUSTSEC-2026-0462 from `.cargo/audit.toml`; RUSTSEC-2026-0459 stays with its reason, reworded to point at the sqlx upgrade epic; #681 gets a comment and stays open for that half.

## Acceptance Criteria

1. **AC1.** `Cargo.toml` pins `rmcp = "=3.9.0"` with unchanged features; `Cargo.lock` updated; rmcp still named only in `src/mcp/transport.rs`.
2. **AC2.** RUSTSEC-2026-0462 removed from `.cargo/audit.toml`; `cargo audit` reports no vulnerability; RUSTSEC-2026-0459 remains, its comment updated.
3. **AC3.** MCP unit and integration suites green (registry, guard, protocol versions `2025-11-25` and `2026-07-28`, instructions + skills section, prompts, tools); MCP E2E specs green.
4. **AC4.** Full gates on the final code: clippy `-D warnings`, `sqlx prepare --check`, full `cargo test` with DB, E2E `CI=true --workers=2`.
5. **AC5.** #681 commented (rmcp half done in this PR, sqlx half pending); the PR does not close it.

## Tasks / Subtasks

- [x] T1 — bump rmcp, `cargo update -p rmcp` (AC1)
- [x] T2 — adapt `src/mcp/transport.rs` if needed (AC1, D2)
- [x] T3 — `.cargo/audit.toml` (AC2)
- [x] T4 — MCP suites, then full gates (AC3, AC4)
- [x] T5 — #681 comment (AC5)

## Dev Notes

- Memory guard, port 18080 for E2E, test DB on 3307, one push per story, no formatter — as in 18-1 / 18-2.
- MCP test surfaces: `src/mcp/**` unit tests, `tests/mcp_*.rs` + `tests/common/mcp.rs`, `tests/e2e/specs/**/mcp*.spec.ts` + `tests/e2e/helpers/mcp.ts`.
- Threat model: `docs/auth-threat-model.md` § 5.8–5.10 (no change expected; re-read if the `Accept` / discover behaviour changes what is refused).

### References

- `_bmad-output/planning-artifacts/epics.md` § Story 18.3; issue #681; `.cargo/audit.toml`; CLAUDE.md "MCP layer"

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (`claude-opus-5-5`)

### Completion Notes List

- **T1/T2** — `rmcp = "=3.9.0"` (same features, default features off); `cargo update -p rmcp` brings `sha2` 0.11 / `digest` 0.11 / `hybrid-array` as rmcp's own new dependencies (sha2 0.10 stays for mybibli). **No source change**: rmcp 3.9 compiles against `src/mcp/transport.rs` as is; clippy `--all-targets -D warnings` clean. rmcp still named only in `transport.rs` (the confinement unit test is in the suite).
- **T3** — RUSTSEC-2026-0462 removed from `.cargo/audit.toml`; RUSTSEC-2026-0459 kept, its comment now points at the sqlx 0.9 upgrade of the major-upgrade epic. `cargo audit`: no vulnerability.
- **D3 (`Accept` strictness)** — nothing to align: the MCP integration tests (`tests/mcp_*.rs`, `tests/common/mcp.rs`) and the MCP E2E specs pass unchanged under 3.9.
- **T4 — Gates.** `cargo sqlx prepare --check` green. First full `cargo test` killed by systemd-oomd at 13:27 (station-wide pressure 75 %, a kesh job killed at the same time) — exit 137, not a test failure; re-run once alone, killed again by oomd while linking the test binaries (10 G peak, swap already loaded by another project); third run with a lighter build — `cargo test --no-run` on one job with `CARGO_PROFILE_{DEV,TEST}_DEBUG=line-tables-only`, then the run — **2727 passed, 0 failed**. E2E on the rebuilt stack, `CI=true --workers=2`: 475 passed, 1 flaky (`shelving.spec.ts:193`, known #499), 2 skipped.
- **T5** — #681 commented; stays open for RUSTSEC-2026-0459.

### File List

- `Cargo.toml`, `Cargo.lock`, `.cargo/audit.toml`, `CLAUDE.md` (MCP layer: `=3.9.0`)
- `_bmad-output/implementation-artifacts/18-3-rmcp-upgrade-and-audit-ignores.md`, `sprint-status.yaml`

### Review Findings

Code review 2026-11-12, inline (the diff is one `Cargo.toml` line, one removed audit ignore and a comment; a three-layer review would have nothing to bite): rmcp features unchanged; confinement test green; no stale `=3.8.0` left except `CLAUDE.md` — fixed; the remaining ignore's comment is accurate (`cargo tree --target all -i libsqlite3-sys -e all` still empty). No finding of any severity. Review clean.
