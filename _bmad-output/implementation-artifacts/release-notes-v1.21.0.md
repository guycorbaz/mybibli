# mybibli v1.21.0 — AI agents, under supervision

The closing release of epic 11 ([#493](https://github.com/guycorbaz/mybibli/issues/493)). An AI agent can now read your catalog and **propose** improvements; a librarian decides. Its three leading fixes (ISBN-10 at scan, the Library of Congress through SRU, the *Recent additions* triangle) already shipped in v1.20.1.

**Upgrade impact: six additive migrations**, run automatically at the first boot (the `propose` key scope, proposals and their items, expected tomes, the equivalence key, the series a creation produced, line edits). No existing column changes. **Take a database backup first.** Nothing to configure if you do not use an AI agent.

## What's new

**AI agents, under supervision.** mybibli now includes an MCP server at `/api/mcp`. An AI agent — Claude Code, Hermes Agent or any MCP client over streamable HTTP — connects with a new *Read + propose* API key (Admin → API keys), reads the catalog through twelve tools, and **proposes**: new series and the tomes you are missing, additions to a series, missing subtitles, descriptions, publishers, years, Dewey codes and contributors, and covers — always citing its sources. It never changes the catalog itself.

- Read tools: `search_titles`, `get_title`, `search_contributors`, `list_series`, `get_series`, `list_titles_without_cover`, `list_titles_with_incomplete_metadata`, `list_my_proposals`.
- Propose tools: `propose_series_create`, `propose_series_attach`, `propose_title_metadata`, `propose_cover`.

**The Proposals page.** Librarians and Admins get a *Proposals* entry in the navigation bar with a pending-count badge, and a *Pending proposals* indicator on the home page. Each proposal shows its sources, the current and proposed values side by side (with *New* / *Changed* badges), or the current and proposed cover. You can edit or remove lines before accepting, accept, or reject with a reason the agent can read. A proposal whose subject changed in the meantime is marked *Outdated* and nothing is applied. Accepted values count as edited by hand, so automatic metadata downloads never overwrite them.

**Resubmission delay.** After a rejection, an equivalent proposal is refused for 90 days, whichever key files it (Admin → System → Proposals, 0–3650 days).

**Expected tomes.** A series can now name the tomes you do not own yet — by hand, or from an accepted proposal. They show in their gap on the series page, go to the wish list in one click, and take their position automatically when you scan the book.

**Sticky navigation bar** ([#496](https://github.com/guycorbaz/mybibli/issues/496)). The bar stays at the top while you scroll, and switches to the menu below 1280 px (was 1024 px) so every entry fits on one line in all four languages.

## For operators

- **LAN only.** The MCP endpoint answers only requests whose `Host` is listed in `MYBIBLI_MCP_ALLOWED_HOSTS` (default: `localhost,127.0.0.1,::1`) and refuses any `Origin` not listed in `MYBIBLI_MCP_ALLOWED_ORIGINS` (default: empty) — a DNS-rebinding defence. To let an agent on another machine connect, set `MYBIBLI_MCP_ALLOWED_HOSTS` to the NAS name/IP it uses and restart. The `docker-compose.yml` of this release passes both variables through; an older copy needs the two lines added. Never expose the endpoint to the Internet.
- **Proposed covers** are validated, re-encoded and kept in `.staging` inside the covers volume — never served by `/covers`, shown to librarians only while pending, cleaned up by the scheduled purge. The server never downloads an agent-supplied URL. Nothing to configure; covers-volume backups include the folder.
- **Revoking a key** marks its pending proposals outdated and deletes their staged covers.
- **New dependency:** `rmcp` pinned at `=3.8.0`, upgraded only for a security advisory.
- `cargo audit` at the release cut: no vulnerability; the usual unmaintained/unsound warnings on transitive crates only.

## Documentation

New manual chapter 13, "AI agents and proposals" (EN + FR): connecting an MCP client (Claude Code example), the tools, the review workflow, expected tomes, cover staging, and an MCP Inspector check for operators. Chapter 8 carries "What's new in 1.21.0". The manual PDFs (EN + FR) are attached to this release.

**Full changelog:** https://github.com/guycorbaz/mybibli/compare/v1.20.1...v1.21.0
