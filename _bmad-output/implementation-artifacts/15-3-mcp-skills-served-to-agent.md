# Story 15.3: MCP skills — served to the agent

Status: done

GitHub issue: #614 (type:change-request), part 2 of 2. Covers: FR163. Epic 15; ships in v1.25.0.

## Story

As a librarian working with an agent,
I want the agent to know the library's skills and me to trigger one from my client,
so that the agent follows the house conventions.

## Decisions (project lead, 2026-11-04 — #614)

Both access paths: MCP **prompts** (the librarian triggers a skill from the client, e.g. `/mybibli:<name>` in Claude) and **consultation by the agent itself** (the `initialize` instructions list name + description; a read-only `get_skill` returns the body). The agent can only read skills.

## Starting point

- Story 15-2 (PR #624): table `mcp_skills`, `models::mcp_skill::{list_active, find_active_by_name}` (live + active only), Admin → Skills tab; see its story file and the CLAUDE.md "MCP skills" entry.
- MCP layer (CLAUDE.md "MCP layer (epic 11)"): `src/mcp/transport.rs` is the only file naming `rmcp` (`rmcp_is_confined_to_the_transport_file`); `MybibliMcpServer` implements `ServerHandler` (l. 133): `get_info` (sync!) builds `ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(instructions(&self.state))`; `list_tools`, `call_tool`. rmcp 3.8.0 `ServerHandler` also has `list_prompts` / `get_prompt` (defaults in `rmcp-3.8.0/src/handler/server.rs` ~l. 415) and the builder has `enable_prompts()`.
- Instructions: `src/mcp/instructions.rs::server_instructions(...)` built per request from live data (story 12-3).
- Tools: `McpTool` trait (`src/mcp/tool.rs`), registry `src/mcp/tools/mod.rs` (read tools `read_only`, closed schemas at every level — registry test), `tests/common/mcp.rs`, `MCP_TOOLS` in `tests/e2e/helpers/mcp.ts`; no SQL under `src/mcp/` (`no_sql_under_src_mcp`) — go through a service.
- Threat model: `docs/auth-threat-model.md` § 5.8–5.10.

## Design point to settle first

`get_info` is synchronous, but the active skills live in the DB. Options: (a) an in-memory snapshot of active skills' (name, description) in `AppState` (`Arc<RwLock<…>>`, loaded at boot, refreshed by every Admin → Skills write — same shape as `AppSettings` reload), read by `get_info`; (b) build the instructions in an async hook rmcp offers for `initialize`, if any. Prefer the simplest that keeps the instructions exact right after an admin edit; document the choice. Prompts and `get_skill` read the DB per request (always current).

## Acceptance Criteria

1. **AC1 — prompts.** The server declares the `prompts` capability. `prompts/list` returns every active skill (name, description; no arguments). `prompts/get` returns the body as one user message (text content); unknown, inactive or deleted name → MCP error (invalid params / not found), never a body. Pagination not needed below a sane cap (document the cap, e.g. 200 skills).
2. **AC2 — instructions.** The `initialize` instructions add a short "Library skills" section listing the active skills (name + description, bounded: at most N entries / M characters, with "… and K more — call get_skill" beyond) and telling the agent to read the relevant skill with `get_skill` before acting. No skill → no section. Agent-facing text stays English (skill content is whatever the admin wrote).
3. **AC3 — `get_skill` tool.** Read-only tool `get_skill { name }` (closed schema, name pattern = the 15-2 slug), returns `{ name, description, body, updated_at }` (UTC ISO 8601); unknown / inactive / deleted → `NotFound`. Registered in `registry()`, `tests/common/mcp.rs`, `MCP_TOOLS` (`tests/e2e/helpers/mcp.ts`); registry tests stay green (tool count updated wherever asserted).
4. **AC4 — confinement & scope.** `rmcp` stays confined to `transport.rs`; no SQL under `src/mcp/`; a propose-scoped key can read skills, nothing more (skills grant no capability). Threat model § 5.8–5.10 gains a paragraph: skills are admin-trusted text delivered to the agent; they cannot widen the key's scope; an admin should treat them like any instruction given to the agent.
5. **AC5 — tests.** Unit (instructions section bounds/absence, prompt/tool mapping), integration (MCP over HTTP: prompts/list, prompts/get, get_skill, inactive skill hidden, instructions reflect an admin edit immediately), E2E (an admin creates a skill in the tab; an MCP client lists it as a prompt, fetches it, and reads it with `get_skill`). Full suites green.

## Dev Agent Record

### Completion Notes

**Design point — chosen: (b), the async rmcp hooks.** rmcp 3.8.0 lets the
server override the async `initialize` (2025-11-25) and `discover`
(2026-07-28). Both rebuild the instructions and append the "Library
skills" section read from the database on each call; `get_info` (sync)
keeps the instructions without skills — rmcp reads it elsewhere only for
the capabilities and the server identity. No snapshot, nothing to
invalidate: an admin edit shows on the agent's next request. A failed
skills read is logged and the handshake is served without the section.

**AC1 — prompts.** Capabilities `tools` + `prompts`. `list_prompts` →
every served skill (name, description, no arguments), no `nextCursor`.
`get_prompt` → one user text message (the body as typed) plus the
description; unknown / inactive / trashed → -32602 without a body (the
name is echoed only when it has the slug shape). Cap
`services::mcp_skills::MAX_SERVED_SKILLS` = 200.

**AC2 — instructions.** `mcp::instructions::skills_section(skills, total)`
(pure): ≤ `SKILLS_LISTED_MAX` (20) entries and ≤
`SKILLS_SECTION_MAX_CHARS` (4 000) characters of skill lines, then
"… and K more" pointing to `get_skill`; no active skill → no section.

**AC3 — `get_skill`.** `src/mcp/tools/get_skill.rs`, read-only, closed
schema, `name` pattern = the 15-2 slug, answer `{name, description, body,
updated_at}` (UTC, RFC 3339 `Z`); unknown → `NotFound` listing the active
names. Registered in `registry()`, `tests/common/mcp.rs`, `MCP_TOOLS`.

**AC4 — confinement.** rmcp stays in `transport.rs`; the reads go through
`services::mcp_skills::{served_skills, served_skill}` →
`McpSkillModel::{list_active(limit), count_active, find_active_by_name}`
(list without bodies: `ActiveSkillSummary`). Threat model § 5.11 added;
the § 5.x pointer list now reads § 5.8 – § 5.11.

**AC5 — tests.** Unit: `instructions` (absence, listing, entry and
character bounds), `transport::skills_map_to_prompts`, `get_skill`
(arguments, schema pattern, answer, not-found list). Integration
`tests/mcp_skills_served.rs` (5, over HTTP): prompts list/get with
inactive and trashed hidden, `get_skill`, instructions with and without
skills on both protocol revisions, an admin edit visible on the next
request, only a propose key reads skills. `mcp_interop` capabilities
updated; method inventory fixture updated. E2E `mcp-skills.spec.ts`: an
admin writes a skill in the tab from a blank browser; an MCP client
lists it as a prompt, gets it, reads it with `get_skill`.

### Gates (2026-11-05)

- `cargo check --all-targets`, `cargo clippy --all-targets -D warnings`: clean.
- `cargo sqlx prepare --check --workspace -- --all-targets`: clean.
- `cargo test --lib mcp`: 81 passed. `--test mcp_skills_served --test mcp_skills --test mcp_interop`: 5 + 9 + 5 passed.
- E2E (stack on 18080, `CI=true --workers=2`): the 13 MCP and skills specs passed.
- Full suites: run once on the final code, before the push.

### File List

- `src/mcp/transport.rs`, `src/mcp/instructions.rs`, `src/mcp/tools/mod.rs`, `src/mcp/tools/get_skill.rs` (new)
- `src/services/mcp_skills.rs`, `src/models/mcp_skill.rs`
- `docs/auth-threat-model.md`, `CLAUDE.md`
- `tests/mcp_skills_served.rs` (new), `tests/mcp_skills.rs`, `tests/mcp_interop.rs`, `tests/common/mcp.rs`, `tests/fixtures/mcp_method_inventory.json`
- `tests/e2e/helpers/mcp.ts`, `tests/e2e/specs/journeys/mcp-skills.spec.ts` (new)

### Review Findings (2026-11-05 — Blind Hunter, Edge Case Hunter, Acceptance Auditor)

No High; one Medium; all fixed in this story.

- [x] [Review][Patch] (Medium) The E2E spec left its `0ss-` skill active when it failed before the deactivation — later runs on the same database would fill the instructions' bounded list [tests/e2e/specs/journeys/mcp-skills.spec.ts] — `finally` now deactivates it if still active; status checks added on `tools/list` and the refused `prompts/get`.
- [x] [Review][Patch] (Low) The "… and K more" line promised the names of *all* active skills, but `get_skill` lists at most 200 [src/mcp/instructions.rs]
- [x] [Review][Patch] (Low) Threat model § 5.11 stated a 4 000-character bound on the whole section; it bounds the entry lines [docs/auth-threat-model.md]
- [x] [Review][Patch] (Low) `ServedSkills.total` could fall below `listed.len()` (two reads, no snapshot) [src/services/mcp_skills.rs] — clamped.
- [x] [Review][Patch] (Low) A description could contain U+2028 / U+2029 and break the one-line list [src/services/mcp_skills.rs] — refused like control characters (positive shape, Rule #20).
- [x] [Review][Patch] (Low) `get_skill` on an unknown name turned a failed name-list read into an internal error [src/mcp/tools/get_skill.rs] — degrades to a plain `NotFound`.
- [x] [Review][Patch] (Low) The schema-pattern test was tautological [src/mcp/tools/get_skill.rs] — now checks the regex against `validate_name` on sample names.
- [x] [Review][Patch] (Low) `undeclared_capabilities_answer_empty` no longer described its test [tests/mcp_interop.rs] — renamed.

Dismissed (6): `updated_at` time zone (the pool sets `time_zone = '+00:00'`, `src/db.rs`); `initialize` override (identical to rmcp 3.8.0's default, pinned); handshake stall on a saturated pool (same as every DB-backed request, 30 s endpoint timeout); `prompts/get` HTTP 200 in the fixture (the method is now served, the error is JSON-RPC -32602); `InvalidArguments` for a non-slug name (the schema already refuses it); § 5.11 rather than a paragraph in § 5.8–5.10 (content as asked).

### Review pass 2 (2026-11-05, after the fixes)

No Medium+. One hardening applied: the E2E `finally` isolates each cleanup step (agent dispose, key revocation, skill deactivation) so a failing step neither masks the test's failure nor skips the next one; the key is created inside `try` so a failure before it still deactivates the skill.
