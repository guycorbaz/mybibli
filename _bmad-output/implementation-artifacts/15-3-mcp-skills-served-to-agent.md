# Story 15.3: MCP skills — served to the agent

Status: ready-for-dev

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
