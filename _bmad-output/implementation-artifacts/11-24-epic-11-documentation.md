# Story 11.24: Epic 11 documentation

Status: ready-for-dev

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR126–FR147 (documentation), AR31. Epic 11 — twenty-fourth and last story before the retrospective. Docs-only story: no production code changes expected.

## Story

As an operator,
I want to know how to connect an agent and how to handle proposals,
so that I can use the feature without reading the code.

## Acceptance Criteria

1. **AC1 — a new manual chapter, English and French.** `docs/manual/{en,fr}/13-agents-proposals.tex`, input after chapter 12 (operations) and before the glossary, so the numbers of chapters 1–12 that release notes and Rule 19 refer to do not move. It covers: what an agent can and cannot do (it reads, it files proposals, it never writes the catalog — NFR44); creating a `propose` key (Admin → API keys, scope "Read + propose", `mybibli_pr_…`); the allowed hosts and origins (`MYBIBLI_MCP_ALLOWED_HOSTS`, `MYBIBLI_MCP_ALLOWED_ORIGINS`, LAN only); connecting Claude Code over streamable HTTP at `/api/mcp` with the exact `claude mcp add --transport http … --header "Authorization: Bearer …"` command and the `--mcp-config` JSON alternative; the twelve tools, one line each; the proposals page (list and filters, detail, sources, side-by-side comparison, edit and remove lines before accepting, accept, reject with an optional reason, outdated proposals); the nav entry with its pending badge and the dashboard indicator; the resubmission-delay setting in Admin → System (default 90 days); expected tomes on the series page and "Add to wish list"; covers staged in `<COVERS_DIR>/.staging` on the covers volume and their purge; revoking an agent's key.
2. **AC2 — the connection section stays generic.** One section lists the connection facts any MCP client needs — address, transport (streamable HTTP, stateless, JSON answers), the `Authorization: Bearer` header, the key scope, the two protocol revisions (`2025-11-25`, `2026-07-28`) — before the Claude Code commands, so it applies to Hermes Agent or any other MCP client.
3. **AC3 — MCP Inspector operator procedure.** The manual carries a short procedure to check the endpoint with the MCP Inspector (the CLI `tools/list` and `tools/call` commands of PR #512, then the UI in proxy mode), since the autonomous sessions could not run it (D-11-6-10, D-11-22-5).
4. **AC4 — existing chapters consistent.** Chapter 1 environment table gains the two MCP variables; chapter 2 lists the Proposals section of Admin → System and the third key scope; chapter 3 mentions the navigation bar (hamburger below 1280 px, D-11-21-1) and points to expected tomes from the wish list; chapter 6 role table gains the proposal and expected-tome rows; chapter 11 no longer says there are two scopes; the glossary gains *MCP*, *proposal*, *propose key*, *expected tome*. English and French stay structurally aligned.
5. **AC5 — PDFs rebuilt.** `docs/manual/build.sh` rebuilds `mybibli-manual-{en,fr}.pdf` without LaTeX errors or undefined references; the version line in `main.tex` is NOT bumped (Rule 19 — release PR).
6. **AC6 — `CLAUDE.md` patterns.** New Key Patterns entries for the `src/mcp/` layer (rmcp isolated, `McpTool` trait, guard order, `no_sql_under_src_mcp`), the proposal lifecycle (filing, statuses, staleness, acceptance, edits, history, resubmission delay), expected tomes (settle on catalogue events), and cover staging with its purge.
7. **AC7 — `docs/auth-threat-model.md`.** Covers the `propose` scope, `/api/mcp`, `Origin` / `Host` validation (already in § 4 / § 5.8 / § 5.9) and, new, agent content treated as untrusted (escaping on the review pages, sources as `rel="noopener noreferrer"` links to http(s) only, re-validation at acceptance, nothing applied without a librarian), plus a § 7 convention for PRs touching `src/mcp/` or the proposal pages.
8. **AC8 — `docs/route-role-matrix.md` complete again.** The ten routes added by 11-11 (expected tomes) and 11-19 (proposal line edit / remove) get their rows; the header count is updated.
9. **AC9 — README and Docker Hub overview feature lists** mention the MCP server (propose-only agents, proposals reviewed by a librarian) and expected tomes, and the README documentation section points to the manual chapter. No version number changes.
10. **AC10 — release-notes material prepared, not published.** This story file carries a draft "What's new" text the release PR can reuse (Rule 19 is done at release time, not here).
11. **AC11 — gates.** LaTeX build clean (no undefined reference, no overfull box worse than the existing chapters); markdown links and anchors of the touched `.md` files resolve; no code changed, so no `cargo test` needed (a `templates_audit`-style check is irrelevant). No mention of any sibling project.

## Tasks / Subtasks

- [ ] T1 — Chapter 13 EN (AC1–AC3).
- [ ] T2 — Chapter 13 FR (localized adaptation, same structure and labels).
- [ ] T3 — Chapters 1, 2, 3, 6, 11, glossary, `main.tex` input — EN + FR (AC4).
- [ ] T4 — `docs/route-role-matrix.md` (AC8).
- [ ] T5 — `docs/auth-threat-model.md` (AC7).
- [ ] T6 — `CLAUDE.md` (AC6).
- [ ] T7 — README, `docs/dockerhub-overview.md` (AC9).
- [ ] T8 — Rebuild PDFs (AC5); link / anchor check (AC11).
- [ ] T9 — Release-notes draft in this file (AC10); decisions log; sprint-status.

## Dev Notes

- **Source of truth for behaviour:** the [PRODUCT] decisions of `epic-11-autonomous-decisions.md` (D-11-6-2 hosts/origins as env vars; D-11-14-2 staging in `<COVERS_DIR>/.staging`; D-11-15-2/-3 equivalence library-wide; D-11-16-3 "Outdated" label; D-11-17-5 stale is a decision; D-11-18-5 accepted fields marked as hand-edited; D-11-19-3/-4 what can be edited / removed; D-11-20-3 change badges; D-11-21-1 `xl` switch; D-11-21-2 badge counts every pending proposal). UI wording from `locales/en.yml` / `fr.yml` (`proposals.*`, `expected_tome.*`, `admin.system.section_proposals`, `admin.api_keys.scope_propose`).
- **Tools (12):** read — `search_titles`, `get_title`, `search_contributors`, `list_series`, `get_series`, `list_titles_without_cover`, `list_titles_with_incomplete_metadata`, `list_my_proposals`; propose — `propose_series_create`, `propose_series_attach`, `propose_title_metadata`, `propose_cover`.
- **Claude Code syntax** checked against `claude mcp add --help` (Claude Code installed on the station): `claude mcp add --transport http <name> <url> --header "Authorization: Bearer …"`, `-s local|user|project` (project writes `.mcp.json` — the key would land in a shared file).
- **Not in this story:** version numbers, "What's new" sections, `ROADMAP.md`, `website/` (Rule 19, release PR). Screenshots: the manual carries two general screenshots only; no new ones (would need a seeded stack and add binary churn for a page that is still evolving — CR #547).
- **Manual build:** `docs/manual/build.sh` (xelatex + latexmk, installed). The PDFs are committed.

### References

- `_bmad-output/planning-artifacts/epics.md` § Story 11.24.
- Stories 11-5 … 11-23 and their decisions; PR #512 (Inspector commands, Claude Code session).

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

### File List
