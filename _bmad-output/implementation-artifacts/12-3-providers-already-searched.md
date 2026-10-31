# Story 12.3: Tell the agent where mybibli already searched

Status: done

GitHub issue: #560 (type:change-request) — closes it. Covers: FR150. Epic 12 — third story; ships in v1.22.0.

## Story

As a librarian working with an agent,
I want the agent to know which sources mybibli already queried,
so that it looks elsewhere first instead of repeating the same lookups.

## Decision (project lead, 2026-10-30)

Static option (no migration): the agent is told which providers mybibli queries for each media type. **Wording constraint:** those providers' records improve over time — a provider that found nothing at the last lookup is a **lower priority, never an exclusion**. Per-title lookup attempts (option 2 of #560) stay a possible follow-up.

## Existing-code reality check (2026-10-31)

| Place | Today |
|---|---|
| `src/mcp/transport.rs` `INSTRUCTIONS` (l. 73) + `ServerHandler::get_info` (l. 124, sync) | three static sentences; no provider named |
| `src/metadata/registry.rs::ProviderRegistry::chain_for(&MediaType)` | the chain per media type, in priority order; `AppState.registry: Arc<ProviderRegistry>` (`src/lib.rs` l. 130) — the server already holds `AppState` |
| `src/metadata/provider.rs` `MetadataProvider::name()` (slugs such as `google_books`, `BnF`) + `metadata_source_display_name(&str)` | readable names already exist |
| Keyed providers: `google_books` (`settings` Arc), `omdb`, `tmdb` (registered unconditionally, skip themselves without a key — story 8-5); `comic_vine` (registered only with its env key) | an unconfigured keyed provider must not be named |
| `bdgest` | a stub: `lookup_by_isbn` logs "not yet implemented" and returns `None` — queries nothing; must not be named |
| Cover fallbacks `services/cover.rs::resolve_cover_url_with_fallback` (l. 228): BnF legal-deposit scans, Inventaire, Open Library covers | not providers of the registry; the agent should hear of them for covers |
| `src/mcp/tools/list_titles_with_incomplete_metadata.rs`, `list_titles_without_cover.rs` `description()` (`&'static str`) | describe the gaps, not the searches |
| `AppState::{google_books_api_key, omdb_api_key, tmdb_api_key}` (`src/lib.rs` l. 196–228) | sync reads of the settings cache (std `RwLock`) — usable from `get_info` |

## Acceptance Criteria

1. **AC1 — instructions name the chain.** The `initialize` result's `instructions` name, for each media type, the metadata providers mybibli queries, in chain order, with readable names (`metadata_source_display_name`), built from the registry (`chain_for`) — not a hand-kept list — plus the cover sources (BnF legal-deposit scans, Inventaire, Open Library covers) for covers. English.
2. **AC2 — only providers that actually query.** A keyed provider is named only when its key is configured (read at request time, so an admin key change shows on the next `initialize`); a stub provider (`bdgest`) is never named. Mechanism: a `MetadataProvider` method (default "active") overridden by the keyed providers and the stub — not a list of names in the MCP layer.
3. **AC3 — priority, not exclusion.** The text says that titles listed as incomplete or without a cover went through that chain without a usable result **at their last lookup**, that other sources should be tried **first**, and that these providers **may hold the record now** (their catalogs are updated). It never says "do not use" / "never" / "skip".
4. **AC4 — list tools point at it.** `list_titles_with_incomplete_metadata` and `list_titles_without_cover` descriptions add one sentence: the titles went through mybibli's metadata (resp. cover) sources, named in the server instructions, without a usable result at their last lookup; look elsewhere first; those sources may hold the record now. The registry test (closed schemas, read tools `read_only`) stays green.
5. **AC5 — tests.** Unit: the instructions builder (pure: media types × provider display names × cover sources → text) — providers present, chain order kept, an inactive provider absent, the "may"/"first" wording present and no exclusion wording. Unit: `is_active` of the keyed providers with and without a key, and of the stub. E2E (`mcp-read-tools.spec.ts`): `initialize` instructions contain "BnF" and "Open Library", and the "may hold" wording.
6. **AC6 — no new SQL, no new crate**, rmcp still confined to `transport.rs`, no SQL under `src/mcp/` (existing tests).

## Tasks / Subtasks

- [x] T1 — `MetadataProvider::is_active(&self) -> bool { true }`; overrides: `GoogleBooksProvider`, `OmdbProvider`, `TmdbProvider` (key present in their settings Arc), `BdgestProvider` (false); unit tests
- [x] T2 — pure builder `mcp::instructions::server_instructions(chains: &[(MediaType, Vec<String>)], cover_sources: &[&str]) -> String` (new small module, or in `transport.rs` if it stays tiny — keep rmcp confined), and `ProviderRegistry::active_chain_names(&MediaType) -> Vec<String>`; `get_info` builds it from `self.state.registry`; cover sources as a `pub const COVER_FALLBACK_SOURCES` next to `resolve_cover_url_with_fallback`
- [x] T3 — list tool descriptions (AC4)
- [x] T4 — E2E assertion in `mcp-read-tools.spec.ts` (AC5)
- [x] T5 — manual ch. 13 (EN + FR): one paragraph on what the agent is told (or note it for Story 12.7)
- [x] T6 — gates (targeted, then full once before the single push)

### Review Findings

Code review 2026-10-31, round 1 (one combined pass: adversarial, edge cases, acceptance): 1 Medium, 8 Low.

- [x] [Review][Patch] **Medium** — the text claimed a listed title "went through" every source; the chain stops at the first source that has a record (and a title without a code is looked up nowhere) → instructions and both list descriptions reworded ("stops at the first source…", "those after the first may never have been asked") [src/mcp/instructions.rs, list tools]
- [x] [Review][Patch] Low — cover fallbacks run only for an ISBN → "covers (books and comics with an ISBN)"
- [x] [Review][Patch] Low — doc comments displaced (`ProviderRegistry::len`, `resolve_cover_url_with_fallback`) → restored
- [x] [Review][Patch] Low — E2E "Open Library" matched the covers line → anchored regex on the book line
- [x] [Review][Patch] Low — manual wording (one covers line, ISBN only, "checks them again" is advice) → EN + FR fixed, PDFs rebuilt
- [x] [Review][Patch] Low — Completion Notes: Comic Vine is not registered at all → fixed
- [x] [Review][Patch] Low — `last_updated` date → 2026-10-31
- [Review][Dismiss] Low — K10plus named although it only completes zones (it is queried; the Dev Notes ask for "queried" without internals)
- [Review][Dismiss] Low — builder takes `String` media labels instead of `MediaType` (no effect)

Round 2 (2026-10-31): 1 Medium, 2 Low — all wording, all patched: the cover fallbacks run only once a metadata source found the title (covers line says so); the list-tool sentences now say the sources "may already have been asked" (a title without a code was asked nothing); the chain line mentions that later sources may still complete a few cataloguing fields (#439 zone pass). No code path changed; the builder's unit tests updated.

## Dev Notes

- `get_info` is synchronous and called per request (stateless server) — the builder must be cheap (a few string joins); no `.await`.
- Media-type labels in the instructions: the `MediaType` values as the MCP tools expose them (`book`, `bd`, `cd`, `dvd`, `magazine`, `report`) so the agent can match them with `media_type` in tool results.
- Do not mention internal facts (rate limits, zones-only K10plus) — just "queried".
- Keep the instructions short: one line per media type.

### References

- `_bmad-output/planning-artifacts/epics.md` § Story 12.3, FR150; issue #560 (decision comment)
- `CLAUDE.md` § MCP layer (rmcp confined to `transport.rs`, no SQL under `src/mcp/`, agent-facing English)

## Dev Agent Record

### Agent Model Used

claude-opus-5-5

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created
- T1: `MetadataProvider::is_active` (default `true`); `false` for `OmdbProvider` / `TmdbProvider` without a key and for the `BdgestProvider` stub. Google Books works without a key (lower quota) and stays active; Comic Vine is not registered at all (commented out in `main.rs`). Unit tests per provider.
- T2: new `src/mcp/instructions.rs` (pure `server_instructions`), `ProviderRegistry::active_chain_names` (display names via `metadata_source_display_name`, chain order, dedup), `services::cover::COVER_FALLBACK_SOURCES`; `transport.rs::instructions(&AppState)` builds the text on every `get_info` (sync; settings read through the providers' own std `RwLock`). The static `INSTRUCTIONS` const is gone; its unit test now checks the builder.
- T3: `list_titles_with_incomplete_metadata` / `list_titles_without_cover` descriptions point at the instructions ("look elsewhere first — those sources may hold the record now").
- T4: `mcp-endpoint.spec.ts` (the spec that sends `initialize`; `mcp-read-tools.spec.ts` does not) asserts the instructions name `- book: BnF`, Open Library, "Search other sources first", "may hold the record now".
- T5: manual ch. 13 EN + FR, tools section intro; PDFs rebuilt.
- T6 (targeted): lib `metadata::` + `mcp::` 234 passed; DB `mcp_endpoint`, `mcp_interop`, `mcp_catalog_read_tools` green; `tsc --noEmit` clean.

### File List

- `src/metadata/provider.rs` — `is_active`
- `src/metadata/{omdb,tmdb,bdgest}.rs` — overrides + tests
- `src/metadata/registry.rs` — `active_chain_names` + test
- `src/services/cover.rs` — `COVER_FALLBACK_SOURCES`
- `src/mcp/instructions.rs` (new), `src/mcp/mod.rs`, `src/mcp/transport.rs`
- `src/mcp/tools/list_titles_with_incomplete_metadata.rs`, `list_titles_without_cover.rs` — descriptions
- `tests/e2e/specs/journeys/mcp-endpoint.spec.ts`
- `docs/manual/{en,fr}/13-agents-proposals.tex`, PDFs
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — 12-3 line
