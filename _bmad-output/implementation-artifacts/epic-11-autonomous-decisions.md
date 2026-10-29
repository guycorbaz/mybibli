# Epic 11 — autonomous decisions registry

Decisions taken by story agents running without user interaction. Entries are appended, never
rewritten.

### D-11-1-1 — Sprint-status: touch only the story line
**Context:** `bmad-create-story` would flip `epic-11` to `in-progress` and rewrite `last_updated`; 11-1, 11-2 and 11-3 run in parallel branches.
**Chosen:** change only the `11-1-isbn10-at-scan` line (Foundation Rule #16); leave `epic-11` and `last_updated` to the orchestrator.
**Rejected:** editing `epic-11` / `last_updated` from this branch — guaranteed merge conflicts with the sibling story PRs.
**Reversible:** yes — a one-line chore commit on `main`.

### D-11-1-2 — Canonicalise the scanned code once, right after classification
**Context:** `handle_scan` and `handle_home_scan` pass `code` to five downstream consumers (duplicate check, title creation, wishlist auto-link, skeleton feedback, provider chain).
**Chosen:** `CodeDetection` gains `normalized_code: Option<String>` (Some only for an ISBN-10 rewritten to ISBN-13); both handlers rebind `code` to it immediately after `detect_code_type`.
**Rejected:** converting at each consumer (five edits, easy to miss one); converting before `detect_code_type` (the contract asks `detect_code_type` itself to use the helper and classify the ISBN-10 as `isbn`).
**Reversible:** yes.

### D-11-1-3 — Share the EAN-13 check-digit computation
**Context:** the ISBN-10→ISBN-13 conversion needs the EAN-13 check digit; two copies already exist (`TitleService::validate_isbn13_checksum`, `catalog.rs::validate_issn_from_ean`).
**Chosen:** `utils::ean13_check_digit` used by the conversion and by both existing validators (mechanical refactor, existing unit tests guard behaviour).
**Rejected:** a third private copy (Rule #1).
**Reversible:** yes.

### D-11-1-4 — Client-side scan JS and other ISBN inputs unchanged
**Context:** `static/js/scan-field.js::detectPrefix` labels an ISBN-10 `upc`/`unknown` but never blocks its submission; the manual title form, wishlist "By ISBN" form and title-edit ISBN field accept ISBN-13 only.
**Chosen:** no JS change (the server is authoritative, `data-detected-prefix` is read nowhere); other ISBN inputs stay out of scope (11.10 reuses the helper for expected tomes). User-manual wording is left to the epic documentation story 11-24.
**Rejected:** a JS mod-11 copy (second source of truth); widening the story to every ISBN input.
**Reversible:** yes.

### D-11-1-5 — Search: bad-checksum ISBN-10 shape keeps today's behaviour
**Context:** `search::detect_code` accepts the ISBN-10 shape without checksum.
**Chosen:** a valid ISBN-10 becomes `CodeType::Isbn(<ISBN-13>)`; an ISBN-10-shaped query with a bad checksum stays `CodeType::Isbn(<raw>)` (exact lookup, then fulltext fallback) as before.
**Rejected:** turning a bad-checksum query into `Text` — a behaviour change nobody asked for; the fulltext fallback already covers it.
**Reversible:** yes.

## Story 11-2 — Library of Congress through SRU (#495)

### D-11-2-1 — Sprint-status: the story line plus the line-2 header
**Context:** the orchestrator's brief for 11-2 allows the `# last_updated:` comment on line 2 in addition to the story line.
**Chosen:** change the `11-2-loc-via-sru` line and the line-2 comment only; `epic-11` and the YAML `last_updated:` key are left alone (Rule #16).
**Rejected:** touching `epic-11` from a story branch.
**Reversible:** yes.

### D-11-2-2 — Remove the JSON search outright, no fallback
**Context:** the issue offered "keep `?fo=json` as a secondary attempt that fails fast on `cf-mitigated`"; the epic AC says it "is no longer called".
**Chosen:** delete the JSON path, its parser, wire types, `LOC_API_BASE_URL`, the mock `/books/` handler and the compose variable.
**Rejected:** a fail-fast fallback — a guaranteed 403 per scan for as long as the Cloudflare policy holds, and dead code to maintain.
**Reversible:** yes (git history).

### D-11-2-3 — Fields mapped from MARC 21 beyond the AC minimum
**Context:** the AC names title, contributors, publisher and date; the JSON path also supplied language, description and a cover.
**Chosen:** also map subtitle (245$b), language (008/35–37), page count (300$a), description (520$a) and Dewey (082$a), so that the switch does not lose what the JSON gave (except the cover). The cover is left to the existing cover-only fallbacks (BnF cover service, Inventaire, Open Library by ISBN).
**Rejected:** the AC minimum only (a visible regression on language/description); a cover from 856$u (not an image link).
**Reversible:** yes.

### D-11-2-4 — Entity decoding + NFC in the LoC parser, via `unicode-normalization`
**Context:** LoC MARCXML encodes non-ASCII characters as numeric references, often decomposed (`e&#x301;`); `metadata::marc` never decodes entities.
**Chosen:** a `marc::decode_xml_text` helper (entities + NFC) applied to every value the LoC provider returns; direct dependency `unicode-normalization = "0.1"`, already in `Cargo.lock` through `sqlx-mysql` → `stringprep`, so no new crate is fetched. BnF and K10plus readers stay raw.
**Rejected:** no NFC (decomposed titles would not match a precomposed search query); decoding inside `extract_subfield` for every provider (changes BnF/K10plus output — out of scope).
**Reversible:** yes.

### D-11-2-5 — Per-provider timeout unchanged
**Context:** the default per-provider budget is 5 s; the SRU transport retry (2 s + 4 s sleeps) can exceed it now that SRU is primary.
**Chosen:** no default change; the chain records LoC as timed out and moves on, as the zone-completion pass already does. Admins can raise `provider_timeout.library_of_congress`.
**Rejected:** a migration seeding a larger LoC override (a settings change nobody asked for).
**Reversible:** yes.

### D-11-2-6 — Health probe on the SRU host; other providers' User-Agents untouched
**Context:** `health_check_url` pointed at `https://www.loc.gov/`, which the provider no longer calls. BnF, K10plus, MusicBrainz and the shared client also carry stale hard-coded User-Agents.
**Chosen:** probe `http://lx2.loc.gov:210/LCDB` (HEAD answers 400 → reachable under #285); fix the LoC User-Agent only, as the AC asks.
**Rejected:** a repo-wide User-Agent sweep in a production-fix story.
**Reversible:** yes.

### D-11-2-7 — Plain HTTP for the whole LoC description
**Context:** code review noted that title, authors and publisher now travel over plain HTTP (`http://lx2.loc.gov:210`), where only the six zones did before.
**Chosen:** accept it. The SRU service is only offered over HTTP (checked 2026-10-27: TLS on :210 fails, :443 answers 404) and the HTTPS JSON search is behind the Cloudflare challenge. Values are plain text, HTML-escaped at render time, and editable by the librarian.
**Rejected:** dropping LoC altogether (the regression #495 is about).
**Reversible:** yes — switch the base URL if LoC ever offers SRU over HTTPS.

### D-11-2-8 — Zone completion skips providers already settled in the run
**Context:** with SRU primary, the completion pass would re-send the identical LoC request after a miss or a failure (review finding).
**Chosen:** a trait hook `zones_come_with_lookup` (true for LoC and BnF); the chain skips a filler that failed in this run (any error or timeout) or that missed while its zones ride on that lookup. K10plus (zones behind a separate call) is unaffected. Side effect for the BnF: a BnF miss earlier in the chain is no longer re-queried for zones (it could only miss again).
**Rejected:** caching inside the LoC provider (state in a provider for one chain run).
**Reversible:** yes.

### D-11-2-9 — Zone completion skips only providers that FAILED (supersedes D-11-2-8)
**Context:** the re-review showed that skipping a provider after a primary MISS loses zones: a LoC record without 245$a is a miss that still carries zones.
**Chosen:** the hook becomes `zones_share_lookup_endpoint` (true for LoC and BnF); the chain skips a filler only when its primary lookup failed in the run (error, throttle, timeout — the Cloudflare-challenge case). A miss is re-asked, at the cost of one cheap request.
**Rejected:** keeping the miss-skip (silent zone loss); provider-side caching.
**Reversible:** yes.


### D-11-2-10 — Lockfile security bump of rustls / rustls-webpki in the 11-2 PR
**Context:** the `cargo audit` job runs on a PR only when `Cargo.toml` / `Cargo.lock` change. 11-2 adds a dependency, so it ran — and failed on five vulnerabilities in `rustls 0.23.45` (RUSTSEC-2026-0346..0349) and `rustls-webpki 0.103.15` (RUSTSEC-2026-0382), published 2026-10-14. Unrelated to the story: the daily scheduled scan on `main` has been red since then.
**Chosen:** `cargo update -p rustls -p rustls-webpki` (0.23.46 / 0.103.16, patch-level, lockfile only) in its own commit on this branch — the epic allows upgrades driven by the daily advisory scan (#481), and Rule #18 asks to fix the root cause of a red CI rather than merge over it. Local `cargo audit`: no vulnerability left.
**Rejected:** merging with the audit job red (not a required check, but red); a separate chore PR first (would block 11-2 behind another CI cycle for a two-line lock change).
**Reversible:** yes.

## § 11-3 — Rotate the "Recent additions" triangle (#494)

### D-11-3-1 — E2E asserts the computed `rotate`, not `transform`
**Context:** the AC and the issue ask to assert "the icon's computed transform". Tailwind v4 (`@tailwindcss/cli@4.2.2`, checked 2026-10-27) emits `rotate-90` as the individual CSS property `rotate: 90deg`; the computed `transform` stays `none` in both states.
**Chosen:** assert `getComputedStyle(icon).rotate` (`"90deg"` open, `"none"` closed) via `toHaveCSS("rotate", …)` — the property that actually carries the rotation.
**Rejected:** asserting `transform` (would never change — a test that passes or fails for the wrong reason); forcing a `transform:` rule in `input.css` (not the CSS-only Tailwind change the AC describes).
**Reversible:** yes.

### D-11-3-2 — Named group `group/fold` instead of a bare `group`
**Context:** the AC names `group` on `<details>` and `group-open:rotate-90` on the icon. The fold contains title cards that are themselves `.group`; Tailwind's `group-*` variants match any ancestor `.group`, so a bare `group` on `<details>` would make a future `group-hover:` inside a card fire whenever the pointer is anywhere in the fold.
**Chosen:** `group/fold` on `<details>`, `group-open/fold:rotate-90` on the `<svg>` — same CSS-only, CSP-clean, JS-free change, scoped to this element.
**Rejected:** the bare `group` (harmless today — no `group-*` variant is used in any template — but a latent trap).
**Reversible:** yes.

## § 11-4 — Sticky navigation bar (#496)

### D-11-4-1 — The `<header>` sticks, at `z-40`
**Context:** the bar (`<nav>`) and the mobile panel (`#mobile-nav`) are siblings inside `<header>`. In-content layers go up to `z-30` (saved-searches dropdown); every overlay is `z-50` or in the top layer.
**Chosen:** `sticky top-0 z-40` on `<header>` — the panel travels with the bar; content passes under, overlays stay above.
**Rejected:** `sticky` on `<nav>` only (the open panel would scroll away under the bar); `fixed` (needs a body offset, layout shift); `z-50` (ties with overlays, order would depend on DOM position).
**Reversible:** yes.

### D-11-4-2 — `scroll-padding-top` on `<html>` with Tailwind utilities
**Context:** the AC asks for 48 px below `md`, 56 px from `md`.
**Chosen:** `scroll-pt-12 md:scroll-pt-14` on `<html>` (= `h-12` / `md:h-14` with `--spacing: 4px`). Covers the skip link, any fragment link, `focus()` and `scrollIntoView`.
**Rejected:** `scroll-margin-top` per target (every future anchor would need it); a rule in `input.css` (duplicates the bar heights outside the template).
**Superseded by D-11-4-8** (re-review): the offset moved from the root scroller to `<main>`.
**Reversible:** yes.

### D-11-4-3 — The mobile panel is bounded to the viewport and scrolls internally
**Context:** not in the issue. Once the header is sticky, a panel taller than the viewport cannot be scrolled into reach: the admin menu (~430 px, and 11-21 adds an entry) overflows a landscape phone (375 px high), hiding Logout.
**Chosen:** `overflow-y-auto overscroll-contain` and a max-height of the viewport minus the bar on `#mobile-nav` — needed for AC3 "stays usable". After review: `max-h-[calc(100vh-48px)] md:max-h-[calc(100vh-56px)]` with `supports-[height:100dvh]:` overrides in `dvh` (fallback for engines without `dvh`; px like the project's `--spacing: 4px`).
**Rejected:** leaving it unbounded (a regression introduced by this story on short viewports); compacting the menu (forbidden by the epic).
**Reversible:** yes.

### D-11-4-4 — Print: `print:static`, the bar is not hidden
**Context:** the wishlist print page is a stand-alone document without the bar, so the AC already holds there. Other pages can still be printed with Ctrl-P.
**Chosen:** `print:static` on the header — printed once, at the top, as before this story.
**Rejected:** `print:hidden` (changes what users get today when printing a page — beyond the AC).
**Reversible:** yes.

### D-11-4-5 — "The feedback list renders above the bar" read as "never covered by it"
**Context:** `#feedback-list` is an in-flow `aria-live` region inside `<main>`, not an overlay; in-flow content necessarily passes under a sticky bar while scrolling.
**Chosen:** no z-index change for the list; it is covered by the AC2 mechanism (since D-11-4-8: `scroll-margin-top` on `<main>` and every descendant — `#feedback-list` sits inside `<main>` on every page that has one — keeps it below the bar when brought into view). The genuine overlays (modals, connection-lost overlay, toasts) are pinned above the bar by `templates_audit::fixed_surfaces_stack_above_sticky_nav` (every `fixed` class list in `templates/` and `static/js` ⇒ `z-50`) and by two E2E checks (top-layer cheat sheet; plain `fixed inset-0 z-50` connection-lost overlay).
**Rejected:** making the feedback list sticky/fixed (a product change nobody asked for).
**Reversible:** yes.

### D-11-4-6 — E2E long pages: series list instead of series detail
**Context:** the AC names "home, catalog, series detail, admin". A series detail page needs a seeded series, and shared-DB writes in a layout spec risk races with `empty-states.spec.ts` (`/series` empty list) and Trash counts.
**Chosen:** the E2E covers `/`, `/catalog`, `/series`, `/admin` and forces a scrollable document via CSSOM `min-height` on `#main-content`; every page extends the same `base.html`, and the render test pins the classes on the shared layout.
**Rejected:** seeding a series (shared-state flake risk for no extra coverage of a layout-level property).
**Reversible:** yes — add the detail page if a read-only series fixture appears.

### D-11-4-7 — Mobile language form loses its landmark name
**Context:** the new axe check with the mobile menu open failed on `landmark-unique`: the desktop language form (visible in the bar at every width) and its mobile-menu copy are two `<form aria-label="Change language">` landmarks. Pre-existing, but only observable with the menu open, which no test opened before.
**Chosen:** drop `aria-label` on the mobile copy — an unnamed `<form>` is not a landmark; its `<select>` keeps its sr-only label, so nothing is lost for screen readers. No visual change, no locale key.
**Rejected:** a distinct label (new key in four locales for a redundant landmark); removing the mobile copy (a UI change); excluding the axe rule in the test.
**Reversible:** yes.

### D-11-4-8 — `scroll-margin-top` on the content instead of a root `scroll-padding-top`
**Context:** the epic AC names `scroll-padding-top` (48 / 56 px). The re-review reproduced in Chromium that a root `scroll-padding-top` also applies to the controls inside the stuck bar: whenever one of them takes focus (Escape closing the mobile menu returns focus to the hamburger; Shift+Tab into the nav), the browser tries to bring it below the padding and scrolls the page up by roughly half a viewport — the reader loses their place, which defeats the purpose of the story.
**Chosen:** `scroll-mt-12 md:scroll-mt-14 **:scroll-mt-12 md:**:scroll-mt-14` on `<main id="main-content">` — the same 48 / 56 px offsets for the skip-link target, hash links, `focus()` and `scrollIntoView` of anything inside `<main>`; nothing on the root, so bar controls never trigger a scroll. Two E2E tests pin "no jump"; the skip-link tests still pin the offset.
**Rejected:** keeping the root padding and patching `nav.js` with `focus({ preventScroll: true })` (does not cover keyboard Tab into the bar); negative `scroll-margin` on header controls (fragile).
Because `scroll-margin` applies to every scroll-container ancestor, the three inner vertical scrollers inside `<main>` reset it with `**:scroll-mt-0!` (pinned by `templates_audit::inner_vertical_scrollers_reset_the_bar_scroll_margin` and a behavioural E2E). Accepted trade-off: the reset also removes the page-level offset for those rows (a saved-search row scrolled to the very top of the page could sit under the bar). And because the `**:` rule beats any `scroll-m*` utility inside `<main>`, no other scroll-offset utility may be used there (audit `scroll_offset_utilities_stay_on_main_and_inner_scrollers`).
**Departs from the epic wording** (property name only; the AC's offsets and intent are kept).
**Reversible:** yes.

## § 11-5 — The `propose` API-key scope (part of #493)

### D-11-5-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-5-propose-api-key-scope` and the `# last_updated:` header change on this branch; `epic-11` stays `in-progress`.
**Rejected:** touching sibling lines.
**Reversible:** yes.

### D-11-5-2 — Unknown stored scope still falls back to `Read`, but loudly
**Context:** the AC asks `from_db` to map `propose` explicitly because "today any unknown value silently falls back to `Read`". The column is an `ENUM`, so an unknown value can only come from a future migration or a manual edit.
**Chosen:** three explicit arms; the `_` arm keeps `Read` (the least-privileged scope: no write, no proposal) and emits `tracing::warn!` with the raw value.
**Rejected:** making `from_db` fallible (every caller is a row mapper that would have to drop the row or 500 — an authentication outage for one corrupt row); mapping unknown to `Propose` or `Write` (privilege escalation).
**Reversible:** yes.

### D-11-5-3 — Propose chip colour taken from classes already present in `templates/`
**Context:** the scope chip classes are chosen in `admin_api_keys.rs`, but the Dockerfile css stage only copies `templates/` into the Tailwind scan — a class literal that exists only in a `.rs` file is never emitted.
**Chosen:** `bg-purple-100 text-purple-800` (already used by a template), distinct from read (sky) and write (amber).
**Rejected:** violet (absent from `templates/`, would render unstyled in production); moving the chip mapping into the template (larger change to an unrelated rendering path).
**Reversible:** yes.

### D-11-5-4 — `ApiKeyPropose` pinned by a throw-away test route
**Context:** the AC requires the extractor now, but the only route that will use it (`/api/mcp`) arrives with 11-6.
**Chosen:** the extractor is `pub` in `middleware/api_key.rs`; an integration test mounts it on a test-only route to pin the full key matrix (missing / read / write / propose). No production route is added.
**Rejected:** a placeholder production route (dead surface, would need CSRF/route-matrix entries for nothing).
**Reversible:** yes.

## § 11-6 — MCP endpoint on `/api/mcp` with `rmcp` (part of #493)

### D-11-6-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-6-mcp-endpoint-rmcp-spike` and the `# last_updated:` header change on this branch.
**Rejected:** touching `epic-11` or sibling lines.
**Reversible:** yes.

### D-11-6-2 — Allowed hosts and origins come from environment variables
**Context:** the AC lets this story decide how the operator configures the allowed hosts (the NAS is not loopback).
**Chosen:** `MYBIBLI_MCP_ALLOWED_HOSTS` (comma-separated `host` or `host:port`; default `localhost,127.0.0.1,::1`; a set value replaces the default) and `MYBIBLI_MCP_ALLOWED_ORIGINS` (comma-separated `scheme://host:port`; default empty, so any request carrying an `Origin` is refused — MCP clients are not browsers; an origin is listed only for a client or proxy that forwards one — see D-11-6-13). Read once when the router is built, like `CSP_REPORT_ONLY`.
**Rejected:** a K/V `settings` row editable in Admin → System — the list is a deployment fact (which LAN name the NAS answers to) and a DNS-rebinding defence; making it editable from a web page weakens the defence for no gain.
**Reversible:** yes.

### D-11-6-3 — rmcp performs the Host/Origin check; the API-key check runs first
**Context:** rmcp 3.8.0 validates `Host` and `Origin` itself (403), configured explicitly from D-11-6-2. The key check is an axum layer around the rmcp service, so it runs before.
**Chosen:** configure rmcp's validator (`with_allowed_hosts`, `with_allowed_origins`, `enforce_origin_validation`) rather than write a second validator. Effective order: key (401/403) → protocol-version header (400, D-11-6-9) → Host/Origin (403); a request with neither a valid key nor an allowed host gets 401, one with a valid key and a bad host 403. Trade-off (review round 1): a refused-host request still costs one argon2 verification and bumps the key's `last_used_at`; documented in `docs/auth-threat-model.md` § 5.8. A hand-written transport (AR27 fallback) would have to re-implement the check — recorded in `src/mcp/transport.rs`.
**Rejected:** a mybibli validator in front of auth (duplicated parsing logic whose semantics could drift from rmcp's); disabling rmcp's validator.
**Reversible:** yes.

### D-11-6-4 — An `initialize` with an unknown revision is negotiated, not refused
**Context:** the AC asks that an unknown protocol revision be "refused cleanly". The MCP lifecycle says a server receiving an `initialize` for a version it does not support answers with a version it does support; the client then disconnects if it cannot speak it. rmcp implements exactly that.
**Chosen:** keep the spec behaviour for `initialize` (answer `2025-11-25`); "refused cleanly" applies to the two other channels — an unknown `MCP-Protocol-Version` header (400 with a plain-text reason) and a `_meta` version that does not match an accepted header (JSON-RPC `HEADER_MISMATCH` -32020, HTTP 400 — corrected in review round 1: since the header itself is restricted to the two revisions, an unknown `_meta` version can only surface as a mismatch). Tests pin all three.
**Rejected:** overriding `initialize` to return an error (breaks the spec negotiation every client relies on).
**Reversible:** yes.

### D-11-6-5 — Body limit derived from the cover bound
**Context:** NFR45 asks for an explicit limit consistent with the cover bounds (base64 inflates by a third); covers arrive in story 11-12. `services::cover::MAX_COVER_SIZE` is 10 MiB. The manual upload route is in fact capped lower, by axum's default 2 MB body limit on `Multipart`.
**Chosen:** `ceil(MAX_COVER_SIZE / 3) * 4 + 1 MiB` (≈ 14.3 MiB), computed from the constant so the two cannot drift. Story 11-12 may tighten it to the effective manual-upload bound.
**Rejected:** a small limit now that 11-12 must remember to raise.
**Reversible:** yes.

### D-11-6-6 — No anonymous session row for `/api/mcp`
**Context:** the session resolver mints an anonymous `sessions` row on every request without a cookie, `/api/*` included. An agent calling tools in a loop would write one row per call (purged after 7 days).
**Chosen:** add `/api/mcp` to `should_skip_session_resolve` (the endpoint authenticates by key only and reads no `Session`). `/api/v1` keeps today's behaviour; follow-up issue #513.
**Rejected:** skipping all `/api/*` here (changes `/api/v1` responses — out of scope).
**Reversible:** yes.

### D-11-6-7 — Title detail assembly moves to `services/`
**Context:** AR27 wants tools as thin adapters over `services/`; the title-detail assembly lives privately in `routes/api_v1.rs`.
**Chosen:** move the DTOs and the assembly to `services/title_detail.rs`; `api_v1` re-exports the DTO names, JSON unchanged. `get_title` returns the same document as `GET /api/v1/titles/{id}`; story 11-7 enriches it.
**Rejected:** calling the route module from `src/mcp/`; a second assembly.
**Reversible:** yes.

### D-11-6-8 — Own trait, no `#[tool]` macros
**Context:** rmcp offers `#[tool]` / `#[tool_router]` macros (feature `macros`), whose generated code names rmcp types in the tool signatures.
**Chosen:** `default-features = false`, features `server` + `transport-streamable-http-server`; tools implement `mcp::tool::McpTool` (serde_json in, serde_json out) and one hand-written `ServerHandler` in `transport.rs` dispatches to them.
**Rejected:** the macros (AR27: no rmcp type may cross the tool interface).
**Reversible:** yes.

### D-11-6-9 — The endpoint refuses `MCP-Protocol-Version` values other than the two served revisions
**Context:** rmcp refuses only versions it does not know; a header naming an older revision it knows (`2025-06-18`, `2025-03-26`, `2024-11-05`) is served, although `supported_protocol_versions` lists only `2025-11-25` and `2026-07-28` (that list bounds `initialize` negotiation and per-request `_meta` versions, not the header).
**Chosen:** the mybibli guard in `src/mcp/mod.rs` (which also checks the key) answers 400 with a plain-text reason when the header is present and names anything else. A request without the header keeps rmcp's backward-compatible handling (the 2025-11-25 spec has clients send it after `initialize`; absent means an older client, which then gets the 2025-11-25 semantics).
**Rejected:** relying on rmcp alone (older revisions slip through — contradicts NFR42's "accepts revisions 2025-11-25 and 2026-07-28"); `stateless_protocol_metadata_required` (would refuse every 2025-11-25 request, which carries no per-request `_meta`).
**Reversible:** yes.

### D-11-6-10 — Manual checks: a real Claude Code session transcript; the MCP Inspector left to the librarian [PRODUCT]
**Context:** the AC asks for the MCP Inspector check and a real Claude Code session to be "recorded in the PR with a screenshot". This story runs without a display or an interactive user, and the session's permission policy refused running the Inspector (`npx @modelcontextprotocol/inspector`, external code).
**Chosen:** a non-interactive `claude -p` session (Claude Code 2.1.312) with `/api/mcp` registered through `--mcp-config` against the local E2E stack called `get_title` successfully; its transcript and the server log lines (revision negotiated: `2026-07-28`) are pasted in the PR. The MCP Inspector check and the screenshots are listed in the PR as a manual step for the librarian (commands included) — to be done before the epic closes.
**Rejected:** skipping the checks; fabricating a screenshot; working around the permission refusal.
**Departs from the epic wording** (evidence format; Inspector check deferred to the librarian).
**Reversible:** yes.

### D-11-6-11 — `rmcp` logs capped at `warn` by default
**Context:** the manual session showed rmcp logging its per-request service lifecycle at INFO (four lines per stateless call, with the client's `initialize` parameters) — noise on the NAS log volume at the default `info` level.
**Chosen:** add `rmcp=warn` to `config::LOG_NOISE_CAPS` (the Fix #405 mechanism); mybibli's own `MCP tool call` line (tool, key id) stays at INFO. An operator can still set `MYBIBLI_LOG_LEVEL=rmcp=debug`.
**Rejected:** leaving the default verbose.
**Reversible:** yes.

### D-11-6-12 — An ordinary request without `MCP-Protocol-Version` is refused
**Context:** review round 1 (Edge Case Hunter): rmcp reads a non-`initialize` request without the header as revision `2025-03-26` and would serve it — a back door around D-11-6-9.
**Chosen:** `list_tools` and `call_tool` check `RequestContext::protocol_version()` against the two served revisions and answer JSON-RPC `UNSUPPORTED_PROTOCOL_VERSION` (-32022) otherwise. The 2025-11-25 specification requires clients to send the header on every request after `initialize`, and 2026-07-28 requests always carry it, so conforming clients are unaffected (the E2E and the manual Claude Code session send it).
**Rejected:** buffering the body in the axum guard to read the method (duplicates rmcp parsing); `stateless_protocol_metadata_required` (refuses every 2025-11-25 request).
**Reversible:** yes.

### D-11-6-13 — No CORS on `/api/mcp`
**Context:** review round 1: a browser page on an allowed origin cannot call the endpoint — its CORS preflight carries no key (401) and no `Access-Control-Allow-*` header is ever sent.
**Chosen:** keep it that way (the endpoint is for local agents, not browser pages; adding CORS would widen the surface of a LAN-only endpoint). `MYBIBLI_MCP_ALLOWED_ORIGINS` stays, for clients or proxies that forward an `Origin`; the docs no longer suggest a browser tool in direct mode. The MCP Inspector's default proxy mode is unaffected.
**Rejected:** a CORS layer bypassing the key check for `OPTIONS`.
**Reversible:** yes.

## § 11-7 — MCP catalog read tools (part of #493)

### D-11-7-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-7-mcp-catalog-read-tools` and the `# last_updated:` header change on this branch.
**Rejected:** touching `epic-11` or sibling lines.
**Reversible:** yes.

### D-11-7-2 — `get_title` enrichment is MCP-only, `/api/v1` unchanged
**Context:** the AC asks `get_title` to carry "cover present or not" and "manually edited fields"; `TitleDetail` is shared with `GET /api/v1/titles/{id}` (D-11-6-7).
**Chosen:** `AgentTitleDetail` in `services/title_detail.rs` flattens `TitleDetail` and adds `has_cover` and `manually_edited_fields`; the API v1 document stays byte-identical.
**Rejected:** adding the fields to `TitleDetail` (silently changes a public API contract outside this story's scope); a second assembly (Rule #1).
**Reversible:** yes.

### D-11-7-3 — One `query` argument with code detection; no separate volume tool
**Context:** FR128 says "search titles, volumes and contributors"; the AC names three tools (`search_titles`, `get_title`, `search_contributors`) and filters "text, ISBN, media type, genre".
**Chosen:** `search_titles.query` reuses `search::detect_code`: ISBN-10/13 → exact lookup, volume label `V0042` → its title, otherwise text. Volumes are read through `get_title` (labels, locations, condition). Filters also apply to exact hits; an L-code is treated as text (no redirect for an agent); an empty query browses the whole catalog (the web UI returns nothing there).
**Rejected:** a `search_volumes` tool (not in the AC); separate `isbn` / `text` arguments (two ways to say the same thing).
**Reversible:** yes.

### D-11-7-4 — Fixed page size 25, page capped at 10 000
**Context:** "pagination is capped server-side".
**Chosen:** the page size is `DEFAULT_PAGE_SIZE` (25) — no client-controlled size; `page` must be 1..=10000 (also keeps `(page-1)*25` inside `u32`).
**Rejected:** a `page_size` argument clamped to a maximum (more surface, no need at household scale).
**Reversible:** yes.

### D-11-7-5 — Genre filter by name
**Context:** an agent sees genre names (`get_title.genre_name`) and has no genre-listing tool.
**Chosen:** `genre` is a case-insensitive name; an unknown name returns `isError` listing the active genres, so the agent can correct itself.
**Rejected:** a genre id (the agent would need another tool to discover ids).
**Reversible:** yes.

### D-11-7-6 — `search_contributors` returns a live title count
**Context:** the AC gives no output shape.
**Chosen:** `{ id, name, title_count }` sorted by name, empty query lists all; the agent then uses `search_titles` with the name to list the titles.
**Rejected:** embedding each contributor's titles (unbounded payload); the biography (free text, not needed to identify).
**Reversible:** yes.

### D-11-7-7 — MCP test helpers shared through `tests/common/mcp.rs`
**Context:** the new integration test file needs the request builders of `tests/mcp_endpoint.rs`.
**Chosen:** move them to `tests/common/mcp.rs` (`mod common;` in both files); same for the E2E `mcpPost` → `tests/e2e/helpers/mcp.ts`.
**Rejected:** copying them (Rule #1); one 1 100-line test file.
**Reversible:** yes.

### D-11-7-8 — Non-object `arguments` are a transport-level JSON-RPC error
**Context:** the story's AC8 listed "non-object arguments" among the cases answered with `isError: true`. Review round 1 showed that rmcp 3.8.0 types `arguments` as a JSON object and answers `-32601` before any tool runs.
**Chosen:** accept the transport behaviour (the MCP schema itself types `arguments` as an object; nothing is executed) and pin it with an integration test; `args::parse_object` keeps its non-object guard for a future transport (AR27 seam).
**Rejected:** a custom `tools/call` deserializer in `transport.rs` to turn it into `isError` (fights the SDK for a malformed request no conforming client sends).
**Departs from the story's AC8 wording** (not from epics.md).
**Reversible:** yes.

### D-11-7-9 — Exact identifiers answer exactly (supersedes part of D-11-7-3)
**Context:** review round 1 — an ISBN or volume label that exists but fails the filters fell through to a text search on the digits; `titles.isbn` has no UNIQUE key, so `find_by_isbn … LIMIT 1` hid duplicates; agents copy hyphenated ISBNs; title pages had no tie-breaker.
**Chosen:** an identifier that exists returns only its titles (all live titles sharing an ISBN, via `TitleModel::list_by_isbn`), filtered, or an empty page — never a text match; an unknown identifier still falls through to text. Hyphens and spaces are stripped when what remains is ISBN-shaped. `active_search` orders `…, t.id ASC` — this also makes the web search order deterministic among equal titles (no visible change otherwise).
**Rejected:** leaving the web search order non-deterministic (agent paging repeats or skips titles).
**Reversible:** yes.

### D-11-7-10 — Genre names resolved by the column collation (refines D-11-7-5)
**Context:** review round 1 — Rust `to_lowercase` is accent-sensitive while the web UI (MariaDB `*_ci` collation) is not.
**Chosen:** `GenreModel::find_active_id_by_name` (`name = ?`, collation decides: case- and accent-insensitive); the list of names is only loaded for the "unknown genre" message. `genre` and `media_type` are capped at 200 characters like `query` (they are echoed in error messages); the bound applies to the value as sent, like the schema's `maxLength`.
**Reversible:** yes.

## § 11-8 — MCP series and gaps read tools (part of #493)

### D-11-8-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-8-mcp-series-gaps-read-tools` and the `# last_updated:` header change on this branch.
**Reversible:** yes.

### D-11-8-2 — Split `models/title.rs` first (#516)
**Context:** the story adds a title query; `src/models/title.rs` is at 1942 lines (Rule 12).
**Chosen:** move the search projection (`SearchResult`, sort whitelist, `active_search`, `list_recent_active`, their unit tests) to `src/models/title_search.rs` in its own behaviour-preserving commit, re-exported from `models::title` so no caller changes; the incomplete-metadata query goes to the new file. Closes #516.
**Reversible:** yes.

### D-11-8-3 — Gaps only for closed series, with the dashboard's formula
**Context:** the AC says "gaps computed as in `SeriesModel::list_with_gaps`", which only knows gaps for a closed series with a total.
**Chosen:** `gap_count = max(total - distinct held positions, 0)` for a closed series with `total > 0`, `null` otherwise; `missing_positions` lists the positions 1..=total not held. An open series reports `gap_count: null` and `missing_positions: []` — holes between held positions of an open series are not reported (no known end). A position above the total (legacy data only — assignment refuses it) counts as held, as on the dashboard, so `gap_count` can then be smaller than the number of missing positions.
**Rejected:** inventing gaps for open series (departs from the dashboard and from the AC's reference).
**Reversible:** yes.

### D-11-8-4 — `missing_positions` capped at 1000
**Context:** `series.total_volume_count` has no upper bound.
**Chosen:** at most 1000 missing positions listed, ascending, with `missing_positions_truncated: true` when cut; `gap_count` stays exact.
**Reversible:** yes.

### D-11-8-5 — `positions` lists assignment rows, not a grid
**Context:** the AC asks for "positions, omnibus flags".
**Chosen:** one entry per live assignment row (an omnibus yields one entry per covered position, `is_omnibus: true`), ordered by position then title id; the gaps are given separately in `missing_positions` instead of empty grid cells.
**Reversible:** yes.

### D-11-8-6 — Incomplete metadata: four fixed fields, one optional filter
**Context:** "a title is incomplete when it lacks a description, a publisher, a contributor or a publication year, and the list can be filtered by missing field".
**Chosen:** `missing` (optional, one of `description`, `publisher`, `contributor`, `publication_year`); blank strings count as missing; a contributor counts only through a live junction row, live contributor and live role (the `get_title` rule); the publication year is missing when `publication_date` is NULL. Each item lists all its missing fields. Both title lists also take the `media_type` and `genre` filters of `search_titles`.
**Rejected:** an array filter (any/all semantics to explain for little gain).
**Reversible:** yes.

### D-11-8-7 — `list_series` filters: name and `gaps_only`
**Context:** the AC gives no input shape for `list_series`.
**Chosen:** `query` (name substring) and `gaps_only` (closed series with at least one gap), sorted by name then id, 25 per page.
**Reversible:** yes.

### D-11-8-8 — Code review: a position above a closed series' total fills no slot (supersedes part of D-11-8-3)
**Context:** the review found `gap_count` (total − held positions, counting positions above the total) disagreeing with `missing_positions` (1..=total not held), and `list_series gaps_only` hiding a series that `get_series` reports gaps for. Positions above the total are not legacy-only: `update_series` checks the new total against the number of assignment rows, so lowering a total or closing an open series leaves them behind.
**Chosen:** for a closed series with a total, only positions within 1..=total count as held — in the shared SQL `filled_positions_join!` (dashboard `count_with_gaps` / `list_with_gaps` and `list_summaries`) and in the Rust `series_gaps`, so `gap_count` always equals the number of missing positions and every surface agrees (the series detail grid already counted that way). `get_series` still lists the out-of-range assignment rows in `positions`. The `/series` list page's own row count and the `update_series` guard are left as they are, tracked in #519.
**Rejected:** keeping the dashboard formula and documenting the mismatch (two tools contradicting each other on the same series); changing only the MCP side (dashboard and MCP would then disagree).
**Reversible:** yes.

## § 11-9 — Proposals schema and service (part of #493)

### D-11-9-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-9-proposals-schema-service` and the `# last_updated:` header change on this branch.
**Reversible:** yes.

### D-11-9-2 — Filing errors are a typed `ProposalError`, agent-facing English
**Context:** `From<AppError> for ToolError` maps every `AppError` to `Internal`; the proposal tools (11-12..11-14) must return an actionable refusal (FR134, NFR47 "explicit error").
**Chosen:** `services::proposals::ProposalError` (validation variants, `NotFound`, `NotPending`, `Database`) with English `Display` messages (MCP copy is not localized — D-11-7 precedent) and `From<ProposalError> for AppError` (validation → `BadRequest`, `NotPending` → `Conflict`); the web stories (11-16..11-19) localize what reaches a librarian.
**Rejected:** `AppError::BadRequest(String)` (the tool layer could not tell a refusal from a server error); localized keys now (no web surface yet).
**Reversible:** yes.

### D-11-9-3 — Item target: nullable ENUM table + id + version, no FK
**Context:** "one line per action: target table, target id, the target's version". A `series_create` line or an expected tome targets nothing that exists yet; titles and series are hard-purged 30 days after a soft delete.
**Chosen:** `target_table ENUM('titles','series') NULL`, `target_id`, `target_version`, all three NULL or all set (CHECK constraint); no FK (polymorphic, must not block the purge — a purged target reads as deleted, hence stale). The service reads the version itself at filing; the caller never supplies it. A later story extends the ENUM by migration if it needs another target.
**Rejected:** a free `VARCHAR` table name (SQL identifiers cannot be bound — the whitelist would live only in Rust); an `action` column (not in the AC; 11-12..11-14 define the `proposed_values` shapes).
**Reversible:** yes (migration).

### D-11-9-4 — Staging directory derived from `COVERS_DIR`, sibling `<name>-staging` [PRODUCT]
**Context:** "created and configured next to `covers/`, unused for now"; `AppState` is built literally in 36 files.
**Chosen:** `services::cover_staging::staging_dir_for(covers_dir)` → `./covers-staging` (Docker: `/app/covers-staging`), exposed by `AppState::cover_staging_dir()`, created at boot, never served (outside the `/covers` `ServeDir`, so an unreviewed agent image is never publicly reachable). No new env var: an operator moves it by moving `COVERS_DIR`.
**Consequence flagged for 11-14 (#522):** in the shipped `docker-compose.yml` only `/app/covers` is a volume, so staged covers would not survive a container re-creation (every upgrade). 11-14 must either add a volume (and update `docker-compose.yml` / `docs/dockerhub-overview.md`) or handle a missing staged file on acceptance.
Creation at boot is fail-open (warn, keep booting): bare-metal installs whose `COVERS_DIR` parent is not writable must not fail to start for a directory nothing uses yet; 11-14 refuses cover proposals when it is missing.
**Rejected:** a subdirectory of `covers/` (persistent, but served by `/covers` unless a guard route is added — untrusted images on the public origin); a new `AppState` field (36-file churn for an unused path).
**Reversible:** yes.

### D-11-9-5 — Source URLs: credentials refused, normalised, deduplicated, bounded [PRODUCT]
**Context:** NFR47 asks for `http`/`https` only, validated at filing; sources will be rendered as links on the proposals page (11-16).
**Chosen:** parse with `reqwest::Url` (no new crate); refuse a non-http(s) scheme, a missing host and embedded credentials (`https://bnf.fr@evil.example` disguises the real host in a link); at most 20 sources of at most 2048 characters; stored in parsed form, duplicates dropped. Items: at most 1000, values a JSON object of at most 64 KiB serialized.
**Rejected:** storing the raw string (an unparseable variant could slip through later rendering); unbounded lists.
**Reversible:** yes.

### D-11-9-6 — No E2E in this story
**Context:** Foundation Rule 3 asks E2E for every feature; this story has no route, page, tool or other user-visible behaviour (the service has no caller until 11-12 / 11-16).
**Chosen:** unit + DB integration tests only; the full Playwright suite still runs as the regression gate. The first E2E over proposals comes with 11-12 (agent files one) and 11-16 (librarian rejects one).
**Rejected:** a throw-away route only to drive an E2E (D-11-5-4 precedent).
**Reversible:** yes.

### D-11-9-7 — `stale` records `decided_at`, never `decided_by` [PRODUCT]
**Context:** the AC lists `decided-by user; decided-at` without saying what a system-made transition records.
**Chosen:** `pending → stale` sets `decided_at = NOW()` and leaves `decided_by` NULL; accept / reject require a user id. A rejection reason is accepted only on reject (trimmed, empty → NULL, ≤ 1000 characters).
**Rejected:** leaving `decided_at` NULL on stale (the history in 11-15 could not date it).
**Reversible:** yes.

### D-11-9-8 — Code review: staleness stays version-based; writers that skip the bump tracked in #521
**Context:** review round 1 found writes that do not bump the target's `version` (background cover / `metadata_source` fetch on `titles`; `title_series` attach / detach on neither `titles` nor `series`), so a proposal filed before them reads as fresh.
**Chosen:** keep the AR29 rule in this story; #521 (Medium) records the gap and the choice 11-17 / 11-18 must make before accepting anything (bump the parent `version`, or re-check the overwritten state at acceptance).
**Rejected:** bumping `titles.version` from the background fetch now — a librarian's open edit form would then fail with a version conflict, a user-visible change outside this story.
**Reversible:** yes.

### D-11-9-9 — Code review: accept enforces staleness and locks the targets [PRODUCT]
**Context:** review round 1 — `Decision::Accept` moved a proposal to `accepted` without any staleness check, and the check read target versions from the transaction snapshot without locking them, so "re-check then apply" (11-17) could apply onto a concurrently edited row.
**Chosen:** `transition_in(Accept)` locks the proposal, reads its targets with `FOR UPDATE` (current read, held to the end of the transaction), and when any moved marks the proposal `stale` and returns `ProposalError::Stale` (the pool wrapper commits that mark; a caller in its own transaction must commit it too). `check_staleness_in` uses the same locking read. Reject does not check staleness (rejecting a moved proposal is harmless).
**Rejected:** leaving FR140 to every caller (11-17 and 11-18 would each have to remember it).
**Reversible:** yes.

### D-11-9-10 — Code review: the service checks the filing key and the deciding user [PRODUCT]
**Context:** review round 1 — any `api_key_id` / user id was stored as given; an unknown id surfaced as a database error, a revoked key or a deactivated user was accepted.
**Chosen:** filing share-locks the key row and requires `scope = 'propose'`, not revoked, not deleted (`KeyNotAllowed`, → `Conflict`); a revocation (11-14) then waits for an in-flight filing and sees it. Accept / reject require an active user (`UnknownDecider`, → `BadRequest`). Also from the review: JSON nested deeper than 20 levels is refused (`ValuesTooDeep`; MariaDB refuses beyond 32), all items of a proposal together are capped at 1 MiB, the stored (normalised) source must itself fit 2048 characters, `ProposalError::Database` never shows database text, and a covers path without a final name (`.`) is resolved before deriving the staging sibling (`None` for `/`, so it is never inside the served directory).
**Reversible:** yes.

### D-11-9-11 — Code review round 2: accept outcome is a value, filing can pin the agent's read, the decider's name is kept [PRODUCT]
**Context:** review round 2 — (a) `transition_in` returned the stale case as an `Err`, so a caller in its own transaction (11-17) using `?` would roll back and lose the stale mark; (b) the staleness baseline is the version at filing, so an edit between the agent's read and its filing was silently absorbed; (c) after the 30-day user purge an accepted proposal no longer said who decided (FR146).
**Chosen:** (a) `transition_in` returns `Transition::{Decided(status), WentStale(targets)}` (`#[must_use]`); the pool wrapper `transition` still maps `WentStale` to `ProposalError::Stale` after committing; an accept with no live item left is refused (`NothingToApply`, as 11-19 requires). (b) `NewProposalItem.expected_version: Option<i32>` — when given and the row moved, filing is refused (`TargetChanged`, an agent-correctable error); the proposal tools (11-12..11-14) decide whether to expose it. (c) `proposals.decided_by_name` copies the username at decision time; reads show `COALESCE(live username, copy)`. Also: the active-user check uses the login predicate (`active = TRUE AND deleted_at IS NULL`); `UnknownDecider` is not agent input; the listing reads its count and page in one transaction; a failed rollback no longer hides the original error; `AppState::cover_staging_dir()` is `None` when the directory does not exist; the lock order a revocation (11-14) must follow is documented on `lock_filing_key`.
**Rejected:** a mandatory `expected_version` (the read tools of 11-7 / 11-8 do not expose `version` yet).
**Reversible:** yes.

### D-11-9-12 — Code review round 4: clean; one note for 11-17
**Context:** confirmation pass found no Medium+. Low: the locking item read on accept takes next-key locks on `idx_proposal_items_proposal`; when the proposal being accepted is the newest, a concurrent filing waits on its item INSERT until the accept transaction ends (no deadlock — each side waits once).
**Chosen:** accept as is; 11-17 must keep accept-and-apply transactions short (no network I/O inside — e.g. cover promotion happens after commit, as 11-18 already says).
**Reversible:** yes.

## § 11-10 — Expected tomes: data and auto-settle at scan (part of #493)

### D-11-10-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-10-expected-tomes-data-auto-settle` and the `# last_updated:` header change on this branch.
**Reversible:** yes.

### D-11-10-2 — Schema: FK CASCADE to the series, SET NULL to the settled title, live-only unique keys
**Context:** AR29 names the table; series and titles are hard-purged 30 days after a soft delete; nothing in the AC says how duplicates behave.
**Chosen:** `series_id` FK `ON DELETE CASCADE` (a tome has no meaning without its series), `settled_title_id` FK `ON DELETE SET NULL` (the purge of a title must not be blocked). A stored generated `live_marker` (1 when live, NULL when soft-deleted) backs `UNIQUE (series_id, position, live_marker)` and `UNIQUE (series_id, isbn, live_marker)`: at most one live tome per position and per ISBN in a series, soft-deleted rows never collide. `series_expected_tomes` joins `PURGE_DELETION_ORDER` before `titles`/`series`.
**Rejected:** no FK (orphans); RESTRICT (would block the 30-day purge); service-only uniqueness (a proposal acceptance racing a manual add could duplicate).
**Reversible:** yes (migration).

### D-11-10-3 — "Settled" means the settled title is live [PRODUCT]
**Context:** the AC says "settled title nullable"; a settled title can later be soft-deleted.
**Chosen:** a tome is settled when `settled_title_id` names a live title; once that title is soft-deleted (or purged, FK SET NULL) the tome reads as unsettled again — the position is a gap again — and the next catalogue of its ISBN settles it again.
**Rejected:** a sticky settled flag (a deleted copy would leave a named gap that never reappears).
**Reversible:** yes.

### D-11-10-4 — Validation rules for a tome [PRODUCT]
**Context:** the AC lists the fields, not the rules; 11-11 (manual) and 11-12 (proposals) share the service.
**Chosen:** live series; position ≥ 1 and ≤ total for a closed series (the `assign_title` rule); refused when the position is already held by a live title of the series (an expected tome names a gap) or by another live tome; ISBN unique among the series' live tomes; title trimmed, 1..=500 characters (the `titles.title` width); ISBN with hyphens/spaces stripped, ISBN-10 converted, ISBN-13 checksum checked. A settled tome cannot be edited (only deleted). Errors are a typed `ExpectedTomeError` with English `Display` (D-11-9-2 precedent); 11-11 localizes what reaches a librarian.
**Rejected:** allowing a tome on a held position (would show a name over an owned square and could never settle without a conflict).
**Reversible:** yes.

### D-11-10-5 — Auto-settle runs on every ISBN catalogue event, re-scans included [PRODUCT]
**Context:** FR145 says "when a title whose ISBN matches … is cataloged, same mechanism as the wishlist auto-link"; the wishlist auto-link fires for a new title and for a re-scan of an existing one.
**Chosen:** the `/catalog/scan` ISBN arm (new and existing title) and the manual title form when it carries an ISBN. Rules in order: title already at that position → settle only; title elsewhere in the series → conflict; closed series total now below the position → conflict; position held by another live title (single or omnibus) → conflict; else attach + settle. Several matching tomes (different series) are each handled. A settle failure is logged and never fails the scan. The title-edit form setting an ISBN later does not settle (not a catalogue event).
**Rejected:** new titles only (an owned-but-unattached copy rescanned would never settle); attaching a title at a second position of the same series.
**Reversible:** yes.

### D-11-10-6 — Settle does not bump `series` / `titles` versions
**Context:** attaching writes `title_series`, which today never bumps a parent version (#521).
**Chosen:** same as every `title_series` write; #521 already records the staleness gap for 11-17/11-18. The tome's own `version` is bumped.
**Reversible:** yes.

### D-11-10-7 — No scan-settle E2E in this story
**Context:** Rule 3; no web route or MCP tool creates an expected tome before 11-11 / 11-12, and 11-11's own AC is exactly that E2E (add a tome, scan its ISBN, see it owned).
**Chosen:** handler-level integration tests drive the real router (`/catalog/scan`, `POST /catalog/title`) with login + CSRF and assert the banner; the E2E `mcp-gaps-tools.spec.ts` asserts the new `expected_tomes` key of `get_series`; the full Playwright suite runs as regression gate.
**Rejected:** a throw-away seeding route (D-11-5-4, D-11-9-6 precedent).
**Reversible:** yes.

### D-11-10-8 — One banner helper for ISBN catalogue events; the manual form gains the wishlist auto-link [PRODUCT]
**Context:** `routes/catalog.rs` is already 3 093 lines (Rule #12); the manual title form showed no wishlist auto-link, while FR145 asks for "the same mechanism as the wishlist auto-link".
**Chosen:** `routes::catalog_isbn_banners::isbn_banners` returns the wishlist auto-link banner followed by one feedback entry per settle outcome; the ISBN scan arm swaps its auto-link call for it (net zero lines in `catalog.rs`), and `create_title` appends it when the new title has an ISBN — so a manually created title on the wishlist now also shows the "On your wish list" banner. Settle success is a `success` entry, a conflict a dismissable `warning` (what happened → why → what to do).
**Rejected:** a settle-only call on the manual form (two code paths for "an ISBN was catalogued").
**Reversible:** yes.

### D-11-10-9 — Router-level test helpers in `tests/common/web.rs`
**Context:** the scan / manual-creation tests need a real login + CSRF flow; it is written inline in `tests/scan_title_context.rs` and several other files.
**Chosen:** a small `Browser { cookie, csrf }` helper in `tests/common/web.rs` used by the new test crate; the existing files are left as they are (no churn outside the story).
**Reversible:** yes.

### D-11-10-10 — Code review round 1: one lock order, a stricter "settled", settle on every path that can fill a tome [PRODUCT]
**Context:** the three review layers found (Medium) a lock-order inversion between scan-settle and the tome writers (deadlock → the settle silently lost), a gap lock taken by every ISBN scan, a tome that stayed "settled" after its title was unassigned or moved, an unsettled tome that could no longer be edited once its position got taken, a restored title resurrecting a settlement contradicted by an edit, a manually typed ISBN-10 that never matched, and tomes left "missing" next to a title that fills them (tome entered for an ISBN already owned, title assigned by hand, ISBN added by an edit).
**Chosen (supersedes part of D-11-10-3 and D-11-10-5):**
- Lock order everywhere: series rows (by id) → tome rows. Settle first reads the matching series ids without a lock (a scan that matches nothing takes no lock), then locks those series in id order, then the tomes; delete locks the series first; the position-holder read is a locking read.
- A tome is settled when its settled title is live **and** still holds the tome's position (live `title_series` row); unassigning, moving or deleting the title makes the position a gap again, and the next catalogue event settles it again. An edit of an unsettled tome clears any stale `settled_title_id`.
- Update checks the range and the holder only when the position changes.
- `settle_for_title` normalises the ISBN it is given (ISBN-10, separators).
- Settle also runs when a tome is created or edited with the ISBN of a live title (first such title by id), after `SeriesService::assign_title` / `assign_omnibus`, and after a title edit that changes its ISBN — silently (logged), no feedback on those paths.
- A tome ISBN-13 must start with 978/979 (a book EAN, the only codes the scan treats as ISBNs).
- `PositionHeld`, `PositionTaken`, `DuplicateIsbn` map to `AppError::Conflict` (state conflicts, not malformed input).
- `create_title` calls the banner helper in one statement (`isbn_banners` takes `Option<&str>`), so `catalog.rs` grows by 4 lines only.
**Deferred:** a total lowered below a tome's position and a Trash restore onto a tome's position (#524, Low).
**Accepted (Low):** a conflict warning repeats on every rescan of the ISBN (by design, D-11-10-5); the non-HTMX manual form settles without showing the banner (the UI always posts through HTMX); `get_series` reads tomes in a second query (read-only tool); one transaction for all matching tomes (only database errors can abort it now); the unique-key fallback distinguishes the two keys by name in the MariaDB message.
**Reversible:** yes.

### D-11-10-11 — Code review round 2: only catalogue events attach; the other hooks only record [PRODUCT]
**Context:** round 2 (both layers, Medium): after D-11-10-10 the settle that follows a hand assignment, a title ISBN edit or a tome label fix ran the full attach logic across every series, so a title a librarian had removed from series A on purpose was silently put back into A by an unrelated action (assigning it to series B, fixing the tome's label). Low: a tome entered for a title already at that position was refused (`PositionHeld`); a title whose ISBN was typed as an ISBN-10 (or with hyphens, within the 13-character column) was not found when the tome was entered; no `(series_id, position_number)` index, so the holder locking read locked the whole series' `title_series` rows.
**Chosen (supersedes part of D-11-10-10):**
- Attach (write `title_series`) only on a catalogue event — scan, manual title creation — and when a tome is created or its ISBN changed. After a hand assignment (`assign_title` / `assign_omnibus`), a title ISBN edit, or a tome edit that keeps its ISBN, settle only **records** a settlement the series already shows (the title already holds the tome's position); it never attaches.
- Creating a tome on a position held by a title carrying the same ISBN is allowed and settles it (that title is preferred over other owners of the ISBN).
- Owners of an ISBN are matched on the stored value with hyphens/spaces removed, as ISBN-13 or its ISBN-10.
- `CREATE INDEX idx_title_series_series_position (series_id, position_number)` in the story migration.
- `get_series` description: `settled_title_id` is null while not settled, including when another title holds the position.
**Accepted (Low):** results of the hook paths stay silent (logged); InnoDB can still report a deadlock between two settles on adjacent series under REPEATABLE READ — the settle is then lost until the next catalogue event (single-librarian load); the title-edit hook is exercised through the service, not the route.
**Reversible:** yes.

### D-11-10-12 — Code review round 3: clean; Lows accepted
**Context:** the confirmation pass found no Medium+. Lows: the holder exemption looks at the first holder row only; whitespace other than a space is stripped in Rust but not in the SQL owner lookup; with duplicate ISBNs the first owner by id may yield `TitleElsewhere` where another could attach; the owner lookup scans `titles` (no index on the normalised value); `idx_title_series_series` is now a prefix of `idx_title_series_series_position`.
**Chosen:** accept all (rare data shapes at household scale; each degrades to "tome stays unsettled", never to a wrong attachment); the redundant index can go in a later cleanup migration.
**Reversible:** yes.

## § 11-11 — Expected tomes: series page and wishlist (part of #493)

### D-11-11-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-11-expected-tomes-series-page-wishlist` and the `# last_updated:` header change on this branch.
**Reversible:** yes.

### D-11-11-2 — #524 states are rendered, not prevented [PRODUCT]
**Context:** a lowered total or a Trash restore can leave an unsettled tome above the total or on a position a title holds again; 11-10 deferred it to this page.
**Chosen:** the series page lists such tomes in an "Expected tomes to review" section under the grid, each with its reason (above the total of T / held by «title») and the Edit / Delete actions. `update_series` and the Trash restore are unchanged.
**Rejected:** refusing to lower a total below a tome (a librarian correcting a wrong total would first have to delete tomes); silently hiding those tomes.
**Reversible:** yes.

### D-11-11-3 — Full-page forms for add / edit, UX-DR8 modal for delete
**Context:** FR147 asks for add, edit, delete; UX-DR8 only covers destructive confirmations.
**Chosen:** `GET/POST /series/{id}/expected-tomes/new|…/{tome}/edit` full-page forms (the series edit form pattern), a refusal re-renders the form with the typed values and the localized message (422); delete through `…/delete-modal` + `POST …/delete` (HX-Redirect to the series page).
**Rejected:** an inline HTMX form inside the grid card (cramped on 2-column phones, harder to make keyboard-accessible).
**Reversible:** yes.

### D-11-11-4 — Wishlist note and duplicate rule [PRODUCT]
**Context:** FR142 asks for "a note naming the series and the position"; the AC only defines the ISBN duplicate.
**Chosen:** note `Series «name», tome N` in the librarian's language at the time of the click; duplicate = a live item with the tome's ISBN; for a tome without ISBN, a live item without ISBN with the same title and the same note (so a double click does not create two rows). The `wishlist_add` audit row is written like the wishlist form does (shared helper).
**Rejected:** no dedup without ISBN (double clicks duplicate); dedup on title alone ("Tome 3" in two series would collide).
**Reversible:** yes.

### D-11-11-5 — Web messages localized, MCP stays English
**Context:** carry-over (b) — `ExpectedTomeError` messages were agent-facing English.
**Chosen:** `ExpectedTomeError::localized(loc)` with `expected_tome.error.*` keys in the four locales for every web surface; `Display` (used by MCP tools) unchanged (D-11-7 / D-11-9-2 precedent).
**Reversible:** yes.

### D-11-11-6 — E2E spec named to run after the specs that assume no series
**Context:** the first full run named the spec `expected-tomes.spec.ts`; the series it creates appeared while `empty-states.spec.ts` expected an empty `/series`, and while `dewey-code` / `classification-fieldset` clicked the last submit button of a title page (which becomes "Add to series" once a series exists) — 5 failures.
**Chosen:** rename to `series-expected-tomes.spec.ts`, next to `series.spec.ts`, which already relies on the same alphabetical schedule; documented in the spec header. Full suite green afterwards.
**Rejected:** rewriting the three older specs' selectors in this story (out of scope; the order dependency predates it).
**Reversible:** yes.

### D-11-11-7 — Code review round 1: errors stay on the page, conflicts refresh the form, wording aligned [PRODUCT]
**Context:** three review layers; Medium: wishlist errors lost (retarget to a `#feedback-list` the series page lacks), stale version kept after a conflict, anonymous test stopped by CSRF, French "liste d'envies" vs the UI's "wish list".
**Chosen:** the wishlist handler renders refusals into `#series-feedback`; a version conflict re-renders the form with the stored values and current version (the message still explains what happened); a vanished tome / series is a 404; a settled tome is never wished ("already in your catalog") and its edit form says it is read-only up front; French copy uses "wish list"; existing label keys reused; the ISBN hint is terse; the refusal page's layout links point at the GET form; the wishlist button disables itself during the request.
**Accepted (Low):** locale-dependent no-ISBN dedup and an ISBN added after a no-ISBN wish (D-11-11-4); check-then-insert race between two librarians; non-integer positions read as out of range; tome squares keep position order under the title sort; inherited `CAST(… AS UNSIGNED)` in the wishlist model; E2E stamp wrap.
**Reversible:** yes.

### D-11-11-8 — Code review round 2: clean; two Lows fixed, one deferred
**Context:** the confirmation pass found no Medium+. Lows: the version-conflict copy still told the librarian to reload; repeated tome controls had no accessible name; a failed delete-modal GET is invisible on the series page.
**Chosen:** new conflict wording ("the form now shows the latest version: make your change again") in the four locales; `aria-label` "<action> — <tome title>" on the three controls; the missing `#feedback-list` on the series page (which also affects the series delete modal and CSRF rejections, pre-existing) goes to #526.
**Reversible:** yes.

## § 11-12 — Series proposal tools (part of #493)

### D-11-12-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-12-series-proposal-tools` and the `# last_updated:` header change on this branch.
**Reversible:** yes.

### D-11-12-2 — Item shapes: a series line first, then titles, then expected tomes [PRODUCT]
**Context:** "one item per line"; 11-9 left the `proposed_values` shapes to the proposal stories; 11-16/11-17/11-19 consume them.
**Chosen:** a `"line"` discriminant. Line 1 is always the series: for `series_create` `{"line":"series","name","series_type","total_volume_count"}` without target; for `series_attach` `{"line":"series"}` targeting `series/<id>` (so the series' version is recorded even when only tomes are proposed, and the duplicate rule is one indexed lookup). Title lines `{"line":"title","position","is_omnibus"[, "end_position"]}` target `titles/<id>`; expected-tome lines `{"line":"expected_tome","position","title","isbn"}` have no target. The origin `proposal` follows from the line type (11-17 creates tomes with `ExpectedTomeOrigin::Proposal`), it is not stored. 11-19 must not let the series line be removed. Contract documented on `services/series_proposals.rs`.
**Rejected:** tome lines targeting the series in an attach (redundant version records; an attach with titles only would record no series version); storing `origin` in each tome line (constant).
**Reversible:** yes until 11-17 ships.

### D-11-12-3 — An omnibus is `is_omnibus` + `end_position`
**Context:** the AC says "position and omnibus flag"; an omnibus in the catalog covers a range (`assign_omnibus(start, end)`).
**Chosen:** `is_omnibus: true` requires `end_position ≥ position` (same rule as the web form); `end_position` without the flag is refused. Duplicate positions are found by comparing ranges (sorted, never expanded), so an omnibus `1..2^31` cannot blow up memory.
**Reversible:** yes.

### D-11-12-4 — Filing-time checks beyond the AC's list [PRODUCT]
**Context:** the AC lists unknown/deleted titles, position ≤ 0 and positions used twice; a proposal that cannot be accepted wastes the librarian's time.
**Chosen:** also refused at filing: positions above a closed total (the proposed total, or the series' total), the same title twice, tome title / ISBN by the expected-tome rules (D-11-10-4), the same tome ISBN twice; for attach, against the series as it is now: a title already in the series, a position held by a live title, a position or ISBN of a live expected tome. A closed series needs a total, an open one takes none, a name is 1..=255 characters.
**Not refused:** a title line on a position an expected tome waits for (acceptance attaches the title; the tome settles or shows in "to review", D-11-11-2); an expected tome whose ISBN a title of the catalog already carries (acceptance settles it, D-11-10-11).
**Reversible:** yes.

### D-11-12-5 — Duplicate rules, checked before filing, outside its transaction [PRODUCT]
**Context:** "an active series with the same name" and "a pending proposal already targeting the same series".
**Chosen:** same name = `SeriesModel::active_find_by_name` (the web form's rule) → `Refused` naming the id and `propose_series_attach`. Pending duplicate: attach = a live pending proposal with a live item targeting `series/<id>`; create = a live pending `series_create` whose series line has the same name under `utf8mb4_unicode_ci` (both sides `CONVERT … USING utf8mb4` so CI's server default charset cannot change the result). Decided or soft-deleted proposals never block. Both checks are plain reads before `ProposalService::file`: two concurrent filings for the same series can both pass (harmless — the librarian rejects one, and the second accept goes stale or conflicts).
**Rejected:** locking inside the filing transaction (a new lock order next to 11-9's key lock and 11-14's revocation, for a race a single agent does not produce).
**Reversible:** yes.

### D-11-12-6 — `get_series` returns `version`; `version` / `series_version` pins are optional
**Context:** D-11-9-11 gave filing an optional `expected_version`; `get_title` already returned `version`, `get_series` did not.
**Chosen:** `get_series` gains `version` (MCP-only document); title entries take an optional `version`, attach an optional `series_version`; a mismatch is refused naming the entity ("Title 42 changed since you read it…"), never 11-9's internal item index (target errors are restated by the series service).
**Reversible:** yes.

### D-11-12-7 — `ToolError::Refused`, propose tools not read-only
**Context:** `ToolError` had only `InvalidArguments`, `NotFound`, `Internal`; an existing series or a pending duplicate is neither a malformed argument nor a server error.
**Chosen:** `ToolError::Refused(String)` (message as is). `SeriesProposalError::class()` decides: input → `InvalidArguments`, unknown series/title → `NotFound`, state conflicts / revoked key → `Refused`, database → `Internal` (logged). The two tools return `read_only() = false` (annotations `readOnlyHint: false`, `idempotentHint: false`, `destructiveHint: false`); the registry test now checks nested schemas are closed (`additionalProperties: false`) and described, and that propose tools say they do not change the catalog.
**Reversible:** yes.

### D-11-12-8 — Success document
**Context:** "returns its id and status".
**Chosen:** `{ "proposal_id", "status": "pending", "kind", "item_count" }`.
**Reversible:** yes.

### D-11-12-9 — Code review round 1: bounded positions, omnibus span and line count [PRODUCT]
**Context:** review round 1 (High, edge-case layer): an omnibus `1..2147483647` passed filing, and accepting it (`assign_omnibus`, one row per position) would run away; a closed total of `i32::MAX` would make the series page grid unbounded. (Medium, blind layer): the 1000-item cap was only checked inside `ProposalService::file`, after an O(n²) validation of up to ~500 000 lines a 14 MiB body can carry.
**Chosen:** agent proposals accept positions and totals in 1..=10 000 (`MAX_POSITION`), an omnibus covering at most 100 positions (`MAX_OMNIBUS_SPAN`), and at most 999 title + tome lines together (`MAX_LINES`, checked first, before any per-line work); duplicate titles / ISBNs use hash sets. The schemas carry the same bounds; the descriptions quote them.
**Rejected:** the same caps on the web forms (librarian input, out of scope).
**Supersedes:** the `i32` bound of the story's dev notes.
**Reversible:** yes.

### D-11-12-10 — Code review round 1: a title line may not fill an expected tome's position [PRODUCT]
**Context:** review round 1 (Medium): an attach title line on a position named by a live unsettled expected tome was filed; accepting it would put a title next to an unsettled tome (the "to review" state of D-11-11-2), and the tool description already said it was refused.
**Chosen:** refused (`TomePositionTaken`), omnibus ranges included. If that title really is the tome, its ISBN would have settled the tome at catalogue time; otherwise the librarian resolves it on the series page.
**Supersedes:** the "Not refused" title-line case of D-11-12-4.
**Reversible:** yes.

### D-11-12-11 — Code review round 1: duplicate checks inside the filing transaction, serialised per key (supersedes part of D-11-12-5)
**Context:** review round 1 (Medium, two layers): agents issue parallel tool calls, so two filings for the same series could both pass the plain-read duplicate checks.
**Chosen:** `ProposalService::file_in` (the transaction-scoped body of `file`); the series service opens the transaction, locks the filing key row `FOR UPDATE` (same key-first lock order as the share lock and 11-14's revocation), runs the existing-series and pending checks as locking (current) reads, files, commits. Filings with the same key are serialised — the realistic race. Two different keys racing on the same series remain possible (tracked for 11-17 in #529). The existing-series check now uses the same `utf8mb4_unicode_ci` comparison as the pending check, independent of the server charset (the web form's own check is unchanged). Validated (normalised) sources are what is filed.
**Rejected:** a lock on the series row (deadlock-prone against an accept holding the proposal-item next-key locks, D-11-9-12); `GET_LOCK` (session-scoped, unreliable with a pool).
**Reversible:** yes.

### D-11-12-12 — Code review round 1: deferred Lows
**Chosen:** #528 (E2E specs creating a series depend on file order — pre-existing, D-11-11-6); #529 (checks 11-17 must repeat at acceptance: owned ISBN on a tome line also proposed as a title line, cross-key race, cross-proposal title conflicts). #514 (MCP body cap / timeout) re-pointed to 11-14 by comment. Fixed: refusals now covered end-to-end through the router (name length, title twice, omnibus rules, positions, totals, tome title/ISBN, line cap), unblocking by accept and soft delete, the concurrent filing, the accent-insensitive existing-series match, and the E2E checks a revoked key is refused.
**Reversible:** yes.

### D-11-12-13 — Code review round 2: plain duplicate reads after the key lock; attach pins the version it checked (supersedes part of D-11-12-11)
**Context:** round 2 (Medium, two layers): the duplicate reads were `LOCK IN SHARE MODE` on predicates no index serves (`CONVERT … COLLATE`, `JSON_EXTRACT`), so they share-locked every `series` row and whole `proposal_items` ranges — a deadlock with an acceptance locking a series (11-16/17) and between two keys inserting into the locked gaps. (Medium, blind layer): an attach without `series_version` recorded, as staleness baseline, the version read inside the filing transaction, newer than the state its position checks ran on.
**Chosen:** the three duplicate reads are plain reads. They run right after the exclusive key-row lock, which is a locking read and opens no snapshot, so the first plain read's snapshot already contains every filing committed before the lock was granted — same-key serialisation is kept (the concurrency test still passes) without range locks. The attach series line pins `series_version` or, when absent, the version read for the checks (a series edited in between is refused with `TargetChanged`). "Proposal filed" is logged after the commit (`proposals::log_filed`). `get_series` / `series_version` descriptions now say `version` moves with the series' own fields only (title / tome writes do not bump it, #521).
**Accepted (Low):** `utf8mb4_unicode_ci` gives every supplementary-plane character one weight (two names differing only by an emoji compare equal → a false "already exists"); `utf8mb4_unicode_520_ci` would need its own check on the CI server.
**Reversible:** yes.

### D-11-12-14 — Code review round 3: clean; Lows accepted
**Context:** confirmation pass, no Medium+; the BEGIN → key `FOR UPDATE` → plain-read order verified (sqlx sends a bare `BEGIN`).
**Chosen:** the `series_version` description now warns that an unpinned call is refused (retry) when the series is edited during it; accepted: the "changed since you read it" wording in that case, validation after `BEGIN` in `ProposalService::file`, the redundant share lock in `file_in`.
**Reversible:** yes.

## § 11-13 — Metadata proposal tool (part of #493)

### D-11-13-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-13-metadata-proposal-tool` and the `# last_updated:` header change on this branch.
**Reversible:** yes.

### D-11-13-2 — Item shapes: one `"line": "field"` item per changed field, current next to proposed [PRODUCT]
**Context:** "stores, for each field, the current value next to the proposed one"; 11-15 (equivalence = title + set of fields), 11-16 (display), 11-18 (accept), 11-19 (edit / remove an item), 11-20 (side by side) consume the shapes.
**Chosen:** one item per kept field, fixed order subtitle, description, publisher, publication_year, dewey_code, contributors; every item targets `titles/<id>` and records its version. Scalar: `{"line":"field","field","current","proposed"}` (strings; `publication_year` integers, current = year of `publication_date`, acceptance writes `YYYY-01-01`). Contributors: `{"line":"field","field":"contributors","current":[{"name","role"}],"proposed":[{"name","role","role_id"}]}` — `proposed` = contributors **to add**, `role_id` authoritative, `role` the name at filing. `current` stays as filed (11-19 edits `proposed` only). Contract documented on `services/metadata_proposals.rs`.
**Rejected:** one item for the whole proposal (11-19 could not remove one field, 11-15 could not read the field set); one item per contributor (a contributor has no single "current" value to show next to it).
**Reversible:** yes until 11-18 ships.

### D-11-13-3 — Field rules: the column bounds, no erasing, a description cap [PRODUCT]
**Context:** "a value the existing title-edit validation would reject"; the edit form only trims, turns empty into NULL, and silently drops an unparseable date — it relies on the column widths.
**Chosen:** trimmed strings; `null`, a wrong JSON type or a blank value refused (an agent completes a title, it never erases a value); `subtitle` ≤ 500, `publisher` ≤ 255, `dewey_code` ≤ 32 characters (columns); `description` ≤ 10 000 characters (the item, current value included, must fit 11-9's 64 KiB — an oversized item is refused naming the field); control characters refused in single-line values (line breaks / tabs allowed in `description`, NFR47); `dewey_code` by the API v1 rule (digits, `.`, `/`, extracted to `utils::dewey_code_chars_valid`) plus at least one digit; `publication_year` 1000..=next year.
**Rejected:** accepting the edit form's silent fallbacks (an invalid year would read as "no change").
**Reversible:** yes.

### D-11-13-4 — Contributors: additions only, role by name among live roles [PRODUCT]
**Context:** "contributors are proposed by name and role; whether they already exist is settled at acceptance".
**Chosen:** a proposal only adds contributors (≤ 50, none removed at acceptance). The role is matched case-insensitively (trimmed, Rust `to_lowercase`) against live `contributor_roles`, stored with its id and the library's spelling; an unknown or soft-deleted role is refused listing the valid roles. Names follow `ContributorService`'s rule (non-blank, ≤ 255 bytes — shared constant `MAX_CONTRIBUTOR_NAME_BYTES`). The same name + role twice is refused; a name + role already on the title (case-insensitive) is dropped as unchanged.
**Rejected:** replace semantics (a removal proposal is outside "missing information" and riskier); matching roles by the SQL collation (accent-insensitive; Rust matching is pure and testable, an accent mismatch only yields a refusal listing the spellings).
**Reversible:** yes.

### D-11-13-5 — No pending-duplicate rule for metadata [PRODUCT]
**Context:** 11-12 refused a second pending proposal for the same series (its AC); 11-13's AC says nothing.
**Chosen:** a second metadata proposal for the same title is filed. Accepting one bumps `titles.version`, so the other goes stale at its own acceptance (contributor-only proposals do not bump it — 11-18 must skip a contributor already present, D-11-13-6). FR136's resubmission delay (11-15) covers repeats of rejected proposals.
**Rejected:** blocking on any pending metadata proposal for the title (an agent could not propose a field it found later until a librarian decides).
**Reversible:** yes.

### D-11-13-6 — Staleness baseline: version read for the comparison, plus the stored `current` values (#521 left to 11-17/11-18)
**Context:** #521 — writes that do not bump `version` escape the staleness check.
**Chosen:** every item records the pinned `version`, or the version of the row the comparison read (a title edited between that read and the filing is refused with `TargetChanged`, D-11-12-13 precedent), so `current` matches the recorded version. All six scalar writers of these fields bump `titles.version` (edit form, API v1 PATCH, background fetch, UNIMARC backfill). Not covered: `title_contributors` writes (manual add / remove, the fetch adding an author). The stored `current` values give 11-18 a field-level baseline whichever option #521 picks; acceptance must skip a contributor + role already on the title.
**Reversible:** yes.

### D-11-13-7 — Field-by-field refusal: all problems at once, nothing filed
**Context:** "refused field by field with an explicit message".
**Chosen:** the service collects every problem (unknown field, type, bounds, each contributor entry with its 1-based path such as `contributors[2].role`) into one `Invalid arguments` refusal; nothing is filed when any field is refused. The tool receives `fields` as a map so an unknown field reaches the service and is named with the list of proposable fields (the advertised schema stays closed).
**Rejected:** filing the valid fields and reporting the others (a partial proposal the agent did not intend).
**Reversible:** yes.

### D-11-13-8 — Shared proposal pieces; the result lists `unchanged_fields`; no own transaction
**Context:** a second proposal kind; 11-14 is the third.
**Chosen:** `RefusalClass`, `FiledProposal` and a `Refusal` trait (`class()` + `Display`) move to `services::proposals` (re-exported by `series_proposals`); `src/mcp/tools/proposal_args.rs` holds `sources_property`, `filed_document` and a generic `tool_error`. Result: `{proposal_id, status, kind, item_count, unchanged_fields}`. Filing goes through `ProposalService::file` (its own transaction): no duplicate check needs 11-12's exclusive key lock, and the version pin makes the earlier reads consistent.
**Reversible:** yes.

### D-11-13-9 — Code review round 1: names folded like the collation, oversized current values truncated, full current date shown [PRODUCT]
**Context:** review round 1 (three layers). Medium: (a) contributor names were compared with `to_lowercase` only, while acceptance resolves them with the `utf8mb4_unicode_ci` collation — "Jose" / "José" passed the in-proposal dedupe (acceptance would then hit the junction's unique key) and an existing "José" read as a new contributor; (b) a title whose current description (or contributor list) is near 64 KiB could never receive a proposal for that field, and the refusal told the agent to shorten its own value; (c) `publication_year` showed only the current year, hiding the month and day acceptance (`YYYY-01-01`) erases; (d) a contributor removed from a title (soft-deleted junction row) cannot be added back — pre-existing in the web flow too.
**Chosen:** (a) `fold_name` (NFD, combining marks dropped, lower case, inner whitespace collapsed) for the dedupe, the "already on the title" check and role matching ("editeur" finds "Éditeur"); proposed text is stored NFC, one-line values with inner whitespace collapsed; the module contract tells 11-18 to re-check by contributor id after `find_or_create`. (b) `fit_item` shortens the item's `current` (string tail, list tail) and adds `"current_truncated": true` — the proposed value alone always fits; the `ItemTooLarge` refusal is gone. (c) the `publication_year` item carries `"current_date"`. (d) #531 (type:bug) — `add_to_title` must reactivate a soft-deleted junction row; 11-18 relies on it.
**Also fixed (Low):** zero-width, direction, isolate, line / paragraph separator and BOM characters refused (NFR47); echoed field keys bounded to 80 characters without backticks or control characters, at most 50 errors spelled out; valid roles listed once at the end of the refusal; a missing `name` / `role` says "is missing"; a year above `i64::MAX` reads as out of range; the year bound uses UTC; an unpinned `TargetChanged` says the title was edited during the call; the Dewey schema pattern requires a digit; the description names the key refusal and the normalisation; `tool_error` tested with metadata errors.
**Accepted (Low):** source errors reported after field errors (two round trips at worst); the Dewey check stays "lite" (API v1 rule); no revoked-key test for this tool (the transport guard refuses before any tool runs, covered by 11-6 / 11-12 tests); `description` safety depends on the review pages escaping it (11-16, NFR47).
**Reversible:** yes.

### D-11-13-10 — Code review round 2: one normal form for proposed and current text; no-break spaces kept [PRODUCT]
**Context:** round 2 (Medium): the round-1 whitespace collapse used `split_whitespace`, turning French no-break spaces (U+00A0, U+202F) into ordinary spaces, and the "unchanged" check compared the normalised proposal with the raw current value — a proposal identical to the stored text (double space, NFD, no-break space) was filed as a change.
**Chosen:** `normalize_text` — trim, NFC, CRLF / CR → LF; one-line values collapse runs of space / tab / line break only (no-break spaces kept). Proposed values are stored in that form and current values are compared through it, so a difference in normal form alone is no change. `fold_name` lower-cases before decomposing (Turkish "İ").
**Accepted (Low):** `fold_name` also drops dependent vowel signs of scripts such as Devanagari, which the collation weighs (a near-duplicate dropped or refused; 11-18 re-checks by contributor id) — noted in the module contract.
**Reversible:** yes.

### D-11-13-11 — Code review round 3: clean; Lows accepted
**Context:** confirmation pass, no Medium+.
**Chosen:** fixed the `text()` doc comment, the spec's dev notes and a test for a tab in the current value; accepted: a current value that differs only in form cannot be cleaned by a proposal (D-11-13-10), the length message counts characters after NFC, the residual Greek final-sigma fold.
**Reversible:** yes.

## § 11-14 — Cover proposal tool and staging purge (part of #493)

### D-11-14-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-14-cover-proposal-tool-staging-purge` and the `# last_updated:` header change on this branch.
**Reversible:** yes.

### D-11-14-2 — Staging inside the covers directory, `/covers` refuses dot-segments (#522) [PRODUCT]
**Context:** D-11-9-4 put the staging area next to `COVERS_DIR` (`/app/covers-staging`); the shipped compose file and the Docker Hub overview mount only `/app/covers`, so staged covers would vanish at every container re-creation and promotion by `rename` would fail with `EXDEV`. Existing installs must keep working without editing their compose file.
**Chosen:** `<COVERS_DIR>/.staging` — on the covers volume every install already has, same filesystem as `covers/` (atomic `rename`; a copy + fsync + rename fallback on `CrossesDevices` all the same). `/covers` is wrapped by a middleware answering 404 for any dot-segment, raw or `%2e`-encoded (`cover_staging::is_hidden_path`), so an unreviewed image is never served. Staged names are random (`<32 hex>.jpg`), written under `.tmp-…` then renamed, and re-validated before any path is built from an item. Boot creation stays fail-open; when the directory is missing `propose_cover` is refused as unavailable. The empty `<covers>-staging` sibling 11-9 created on unreleased `main` builds is left alone (nothing ever wrote to it). Docs: one comment line in `docker-compose.yml` and `docs/dockerhub-overview.md`; the user manual is story 11-24's.
**Rejected:** a second volume (every operator would have to edit their compose file, and forgetting it loses data silently); a guard route serving staged files to librarians now (11-16's job).
**Supersedes:** the placement of D-11-9-4.
**Reversible:** yes.

### D-11-14-3 — The image arrives as bytes; the server never fetches an agent-supplied URL (SSRF policy) [PRODUCT]
**Context:** the AC gives `propose_cover` the image (base64), its media type and the source URL; an agent-supplied URL fetched by the server would be an SSRF surface on the LAN.
**Chosen:** no outbound request at all: `sources` (the page / image URLs, 11-9 rules) are provenance only. Documented in `docs/auth-threat-model.md` § 5.9 with the rule any future fetching tool must follow (http/https, resolve once and refuse loopback / private / link-local / metadata addresses, no redirects or re-check each hop, #479 time and body bounds, magic bytes over `Content-Type`).
**Rejected:** an `image_url` argument the server downloads (the SSRF policy would have to be implemented and kept right for no gain — an agent can fetch).
**Reversible:** yes.

### D-11-14-4 — Image checks: 2 MiB, declared type must match the bytes, re-encoded before staging [PRODUCT]
**Context:** "the same checks as a manual upload (decode and size bounds of #479)"; #514 asks to confirm the body cap against the manual upload's effective bound (axum's 2 MiB `Multipart` default, not `MAX_COVER_SIZE`).
**Chosen:** `MAX_PROPOSED_COVER_BYTES = 2 MiB` decoded; base64 standard alphabet, padding optional, ASCII whitespace ignored, a `data:` prefix refused with a hint; `media_type` one of image/jpeg, png, webp, gif (case-insensitive) and equal to the format detected from the magic bytes (stricter than the manual upload, which ignores the claimed type — a mismatch means the agent mixed up files); then the shared `cover::encode_cover` (extracted from `process_and_save_bytes`, Rule 1): #479 decode limits, ≤ 400 px wide, JPEG 80, run in `spawn_blocking`. The staged file is that JPEG, so promotion is a move and the librarian reviews exactly what acceptance installs. Order: arguments → image → sources → title / version pin → staging available → encode → stage → file; a filing error discards the staged file.
**Rejected:** staging the original bytes (a polyglot would sit on disk and need its own review rendering); 10 MiB like `MAX_COVER_SIZE` (a 14 MiB body for an image the catalog shrinks to 400 px).
**Reversible:** yes.

### D-11-14-5 — Item shape: one `"line": "cover"` item [PRODUCT]
**Context:** contract for 11-15 (equivalence = same title), 11-16 (display / reject), 11-18 (accept), 11-20.
**Chosen:** one item targeting `titles/<id>` (pinned or read `version`): `{"line": "cover", "staged_file", "replaces_existing", "current", "media_type", "original_bytes", "width", "height", "bytes"}` — `current` is the cover URL at filing (cache-buster included) because the background cover fetch does not bump `titles.version` (#521); 11-18 compares it with the live value. The result is `{proposal_id, status, kind, item_count, replaces_existing}`; the staged name is never shown to the agent. Documented on `services/cover_proposals.rs`.
**Reversible:** yes until 11-16 / 11-18 ship.

### D-11-14-6 — No pending-duplicate rule for covers [PRODUCT]
**Context:** the AC says nothing; D-11-13-5 precedent.
**Chosen:** a second cover proposal for the same title is filed; the librarian picks one, the other goes stale when the accepted cover bumps `titles.version` (11-18). Disk use per proposal is one ≤ 400 px JPEG; 11-15's resubmission delay covers repeats of rejected ones.
**Reversible:** yes.

### D-11-14-7 — Revocation stales every pending proposal of the key in one transaction [PRODUCT]
**Context:** NFR48 / AC "revoking an API key marks its pending proposals stale and purges their staged covers"; 11-9 documented the lock order (key row first).
**Chosen:** `ProposalService::revoke_key`: `UPDATE api_keys` (optimistic `version`, locks the key row) → `SELECT … FOR UPDATE` of the key's live pending proposals → each to `stale` (`decided_at`, no decider, D-11-9-7) → their staged names read → commit; then `cover_staging::discard_all` (failures logged, left to the scheduled purge). Every kind goes stale, not only covers. The admin handler calls `cover_proposals::revoke_key`; its feedback copy is unchanged (no locale key — the stale marks show on 11-16's page).
**Reversible:** yes.

### D-11-14-8 — Scheduled staging purge on the 8-7 tick, with a one-hour grace
**Context:** "the scheduled purge (story 8-7) also removes orphaned staged files"; promotion (11-18) runs after the accept commit, and a filing writes its file before committing.
**Chosen:** `auto_purge_scheduler` runs `cover_staging::purge` after each `run_purge` (not at startup: it runs before the covers directory is configured, and the first tick follows a minute later). Kept: files of live pending cover proposals (live item), and of accepted ones decided less than an hour ago. Deleted: files referenced only by rejected / stale / soft-deleted / older accepted proposals; unreferenced files and `.tmp-` files older than an hour. Other names are left alone. Not recorded in `admin_audit` (no catalog row is touched); counts are logged.
**Reversible:** yes.

### D-11-14-9 — `/api/mcp`: body cap ≈ 3.7 MiB and a 30 s timeout (#514)
**Context:** #514 — 14.3 MiB cap sized for 10 MiB covers, and no request timeout.
**Chosen:** `max_request_body_bytes()` derived from `MAX_PROPOSED_COVER_BYTES` (2 796 204 + 1 MiB envelope); `tower_http::TimeoutLayer::with_status_code(408, 30 s)` outermost on the MCP router (key check and body read included). Closes #514.
**Reversible:** yes.

### D-11-14-10 — E2E scope
**Context:** Rule 3; no proposals page before 11-16.
**Chosen:** `mcp-cover-proposal.spec.ts` (spec id `PC`): the agent journey (list, file, refusal), the catalog unchanged through `get_title` (`version`, `has_cover`), `/covers/.staging/` 404 and the title page showing no staged image, revocation turning the agent away. The staged-file 404 on its real name, the revocation's stale mark and the purge are covered by `tests/mcp_cover_proposals.rs` over the real router.
**Reversible:** yes.

### D-11-14-11 — Code review round 1: purge read order, decoded guard, bounded decodes and pending covers, filing runs to completion [PRODUCT]
**Context:** review round 1 (three layers), six Medium findings and one deferred.
**Chosen:** (a) the purge reads `referenced` before `keep`, so a filing committing between the reads is kept or protected by the grace — never deleted; (b) `is_hidden_path` percent-decodes and splits on `/` and `\` (an encoded slash reached `covers/x%2F.staging%2F…`); (c) at most two image decodes at once (static semaphore, permit moved into the blocking task); (d) **at most 100 pending cover proposals per key** — refused, naming the count, until some are decided; (e) `ApiKeyModel::revoke` is test-only, production revokes through `cover_proposals::revoke_key`; (f) encode, stage and file run in a spawned task, so a dropped request (30 s timeout, disconnect) neither orphans a staged file nor cuts a filing — the description tells the agent a timed-out call may have filed. Also (Low): `promote` reports `Missing` only when the source is gone; a failed removal of the staged copy after a cross-device promotion is logged, not a failure; the purge also removes old `.tmp-` files in the covers directory; the schema drops `maxLength` on `image_base64` (line breaks are ignored); description figures checked against the constants. Deferred: refusing an acceptance whose staged file is gone (#522's third point) → #533 for 11-18.
**Rejected:** a per-title pending rule (D-11-14-6 stands; the per-key cap bounds disk); cancelling the encode on timeout (blocking work cannot be cancelled).
**Reversible:** yes.

### D-11-14-12 — Code review round 2: encode permit awaited in the request, cap bound under the key lock
**Context:** round 2 (Medium): with the round-1 spawned job, the semaphore was awaited inside the job — dropped requests left jobs (holding the decoded image) queued without bound — and the 100-per-key cap was a plain read before the job, so a burst of parallel calls passed it together.
**Chosen:** the request awaits the permit (bounded by the 30 s timeout, cancellable), then hands it to the job, which holds it to its end: at most two jobs exist. Filing runs in `file_capped`: `BEGIN` → `lock_filing_key_exclusive` → plain count → `ProposalService::file_in` → commit → `log_filed` (the 11-12 pattern, D-11-12-13); a refusal discards the staged file. Same-key filings are serialised, which the two-permit bound already implied.
**Reversible:** yes.

### D-11-14-13 — Code review round 3: clean; Lows accepted
**Context:** confirmation pass, no Medium+; the permit / key-lock order verified (no share-to-exclusive upgrade, revocation and filing both take the key row first).
**Chosen:** the permit is documented as bounding two filing jobs (decode, staging, filing and the key-lock wait); accepted: a filing stalled on one key's lock delays other keys' cover proposals (single tenant, at most `innodb_lock_wait_timeout`).
**Reversible:** yes.

## § 11-15 — Proposal history and resubmission delay (part of #493)

### D-11-15-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-15-proposal-history-resubmission-delay` and the `# last_updated:` header change on this branch.
**Reversible:** yes.

### D-11-15-2 — One equivalence rule, stored as a key at filing [PRODUCT]
**Context:** FR136 — "share the kind and the target: the series (by id, or by normalised name for a creation), the title plus the same set of fields for metadata, the title for a cover"; hints from 11-12..11-14.
**Chosen:** `ProposalSubject` derived from the items as filed — `series_create:<fold_name(name)>` (the 11-13 fold: case, accents, inner whitespace), `series_attach:series/<id>` (whatever title / tome lines it carries), `metadata:titles/<id>:<sorted field keys>` (exact set: `{description}` ≠ `{description, subtitle}`), `cover:titles/<id>` — stored in `proposals.equivalence_key` (`VARCHAR(700)`, `utf8mb4_bin`, indexed; a folded name is cut at the width). Items that match no shape (service tests) get no key and are never checked. Item edits by 11-19 do not change the key: what the agent asked is what must not come back. Rows filed before the migration keep NULL (proposals are unreleased).
**Rejected:** recomputing equivalence from items at check time (JSON predicates on an unindexed range, the D-11-12-13 lock problem); comparing series names with the SQL collation (charset-dependent; the Rust fold is testable and mirrors it).
**Reversible:** yes (migration + backfill).

### D-11-15-3 — Equivalence is library-wide, not per key [PRODUCT]
**Context:** the AC does not say whose rejection counts.
**Chosen:** a rejection of any key's proposal blocks an equivalent filing by every key, so creating a new key does not reset the delay. The refusal names the rejected proposal and its reason only when the same key filed it (another key's proposal is not in this agent's history).
**Rejected:** per-key delay (trivially bypassed by the admin creating a second key for the same agent).
**Reversible:** yes.

### D-11-15-4 — Checked in `file_in` after the key lock; the delay read from `settings` in the filing transaction
**Context:** "apply the resubmission delay at filing, inside the filing transaction / lock order of 11-12"; the three kinds reach `ProposalService::file_in` with the key already locked (series, cover: exclusive; metadata: the share lock inside `file_in`).
**Chosen:** `check_resubmission` runs inside `file_in` right after `lock_filing_key`: plain reads of the `settings` row and of the newest live rejection on the indexed key (with the database `NOW()`), compared in Rust (`blocked_until`, pure, unit-tested with simulated instants; DB tests back-date `decided_at`). No `AppSettings` field: the row is read where it is enforced, so an admin change applies to the next filing without cache reload; an invalid value reads as 90 (logged). Only `rejected` blocks — pending, accepted, stale or soft-deleted proposals never do; the reopening instant itself is accepted. The refusal is `ProposalError::RecentlyRejected` → `Conflict` (`ProposalError::is_conflict()` used by the three kinds' `class()`, else it would surface as an internal error).
**Rejected:** a check in each kind's service (three copies; metadata has no exclusive key lock); plumbing the cached value through every request struct.
**Reversible:** yes.

### D-11-15-5 — `list_my_proposals` document [PRODUCT]
**Context:** FR135 — own proposals, status filter, decision date, optional reason.
**Chosen:** read-only tool, `status?`, `kind?`, `page?`, the shared list-page shape; items `{proposal_id, kind, status, filed_at, decided_at, rejection_reason, item_count, subject, resubmittable_from}`, dates RFC 3339 UTC; `subject` from the live items through the same `ProposalSubject` (`{series_name}`, `{series_id}`, `{title_id, fields}`, `{title_id}`), `resubmittable_from` for rejected ones with the current delay (may be in the past). Stale proposals show their `decided_at` (key revoked or target moved).
**Reversible:** yes.

### D-11-15-6 — Setting bounds 0..=3650 days, own System section
**Context:** AR30, default 90.
**Chosen:** `proposal_resubmission_delay_days` seeded at 90; 0 disables the rule, at most ten years; a "Proposals" section at the bottom of Admin → System, handler in `routes/admin_system_proposals.rs` (Rule 12), shared `fetch_setting_rows` / `validation_error_response` / `success_feedback` made `pub(crate)`.
**Reversible:** yes.

### D-11-15-7 — E2E scope
**Context:** Rule 3; no rejection surface before 11-16.
**Chosen:** `mcp-proposal-history.spec.ts` (spec id `PH`): an admin sets and restores the delay from a blank browser; two agents file, each lists only its own proposal, filters work. The refusal inside the delay (all kinds, simulated clock, other key, setting change) is covered over the real router by `tests/proposal_history.rs`; 11-16 should add the E2E "reject, then the agent is refused".
**Reversible:** yes.

### D-11-15-8 — Code review round 1: reopening date as enforced, robust form, NFKC fold, check after targets [PRODUCT]
**Context:** three review layers; Medium: `resubmittable_from` computed from each row's own rejection while the filing check uses the newest equivalent rejection (and showed a date for keyless rows that block nothing); a non-number in the admin form failed in the `Form` extractor (no re-render, the message never shown); Blind layer: metadata field-set evasion, history subject from live items.
**Chosen:** (a) `resubmittable_from` = newest live rejection of the row's `equivalence_key` (any key) + delay, null without a key; description says so. (b) the form field is text, parsed and range-checked by `validate_resubmission_delay_days(&str) -> u32`, any failure re-renders with `HX-Trigger: validation-error`; the error copy takes `%{max}`. (c) Lows fixed: the key folds series names with NFKC first (fullwidth, ligatures); the check runs after the target checks (a deleted title is reported as such, not as "wait"); the librarian's reason is quoted as a JSON string in the agent message; index `(equivalence_key, status, decided_at)`; help text states the metadata rule exactly (title **and** fields); the E2E restores the setting in `finally`; tests for series-create and cover "filed after", the HX-Trigger header and the OOB success entry.
**Not changed (by the AC / D-11-15-2/-3), tracked in #535:** metadata equivalence stays "same set of fields" (a superset is filed); cover rejection blocks every key for the title; a rejection committing during a filing is not seen; a blocked cover is still encoded before refusal; the history subject reflects the live items (11-19 note). #536: the reject-then-refused E2E belongs to 11-16.
**Accepted (Low):** truncated keys of pathological names (> 680 folded characters) may collide; the settings row read lives in `ProposalModel` (pool-free variant needed in a transaction); a missing / soft-deleted settings row makes the form answer a conflict (the migration seeds it, same as every other System setting); dates assume the pool's UTC session (R3-N15).
**Reversible:** yes.

### D-11-15-9 — Code review round 2: clean; Lows fixed or accepted
**Context:** confirmation pass (blind + edge/acceptance), no Medium+.
**Chosen:** fixed — a regression test pins "deleted target reported before the delay"; the series name in the refusal is JSON-quoted like the reason; the `resubmittable_from` description says null only without a subject (with a delay of 0 it equals the rejection date); the `latest_rejection` doc states the snapshot limitation (#535); the E2E describe runs serially with a note on the coupling with 11-16 (#536); story ACs annotated. Accepted: when another key's equivalent rejection is newer, the refusal does not repeat this key's own reason (the history has it); the cover cap test now frees a slot with `stale` (rejection would start the delay for that title).
**Reversible:** yes.

## § 11-16 — Proposals page: list, detail, reject (part of #493)

### D-11-16-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-16-proposals-page-list-detail-reject` and the `# last_updated:` header change on this branch.
**Reversible:** yes.

### D-11-16-2 — Every proposals route is Librarian+ [PRODUCT]
**Context:** FR137 / FR138 say "Librarian and Admin"; the story asks anonymous visitors to be redirected to login.
**Chosen:** `/proposals`, `/proposals/{id}`, `/proposals/{id}/cover`, `/proposals/{id}/reject-modal`, `POST /proposals/{id}/reject` all require `Role::Librarian` (pages with a `next` back to themselves). No per-key or per-user scoping (single tenant).
**Reversible:** yes.

### D-11-16-3 — The list: pending by default, GET filters, live target names [PRODUCT]
**Context:** "filterable by status and kind"; FR137 speaks of listing *pending* proposals.
**Chosen:** a plain GET form (`status` = pending by default, `all`, or a status; `kind` = all or a kind; unknown values fall back to the defaults), 25 per page newest first with previous / next links keeping the filters. The "subject" column comes from 11-15's `ProposalSubject` over the live items: the proposed name for a creation, otherwise the live series / title name linked to its page, or "#id (no longer in the catalog)" unlinked when the target is soft-deleted or purged. The stale status is labelled "Outdated" (fr "Périmée") — "stale" is jargon.
**Rejected:** HTMX filtering (no gain for a short list; the GET form keeps the URL shareable and works without JS).
**Reversible:** yes.

### D-11-16-4 — Staged covers served by proposal id, pending only [PRODUCT]
**Context:** D-11-14-2 keeps `.staging` unreachable through `/covers`; 11-14 left a librarian-only route to this story.
**Chosen:** `GET /proposals/{id}/cover` reads the cover item of the proposal, re-validates its `staged_file` with `is_staged_name`, and answers the JPEG with `Cache-Control: no-store` (nosniff is global); 404 for a non-cover proposal, an invalid stored name, a missing staging directory or file. The file name never appears in a URL or a page. The detail page shows the image only while the proposal is pending (a decided proposal's file is discarded or promoted), next to the title's current cover (the side-by-side review of all fields stays 11-20's).
**Rejected:** a route taking the staged file name (a guessable-path surface and a second validation point).
**Reversible:** yes.

### D-11-16-5 — Reject: optimistic lock on the proposal, reason optional, discard after commit [PRODUCT]
**Context:** UX-DR8 modal, D-11-9-7 reason rules, D-11-9-12 (no file I/O inside a decision transaction), cover contract (11-16 calls `discard`).
**Chosen:** the modal (`remove` variant, amber — a decision, not a deletion) carries CSRF, the proposal `version` and an optional `<textarea maxlength="1000">`. The handler runs one transaction: `SELECT status, version … FOR UPDATE` → unknown (404) / already decided (409, names the status) / version moved (409, "reload, then decide again") → `ProposalService::transition_in(Reject)` (reason trimmed, empty → none, > 1000 characters → 400 field hint) → the proposal's staged names read → commit → `cover_staging::discard_all` (failures logged; the scheduled purge catches leftovers). Opening the modal of a decided proposal answers the same 409. Errors are localized web copy (`ReviewError`), MCP copy untouched.
**Reversible:** yes.

### D-11-16-6 — Success answer: re-rendered body, OOB entry, `HX-Trigger-After-Swap: modal-close`
**Context:** the AC asks modals to close via `HX-Trigger: modal-close`. E2E showed that with `HX-Trigger`, HTMX fires the event as soon as the response arrives; `modal.js` then wipes `#modal-slot`, the Confirm form (HTMX's context element) is detached, and the OOB success entry fails (`htmx:oobErrorNoTarget`).
**Chosen:** the same `modal-close` event, sent as `HX-Trigger-After-Swap` (new `HtmxResponse::into_response_with_hx_trigger_after_swap`); the main body replaces `#proposal-detail-body` (status, decider, reason, no Reject button) and the success entry lands in `#feedback-list`.
**Rejected:** `HX-Redirect` to the detail page (the AC wants the modal-close signal; a full reload loses the feedback entry); dropping the OOB entry.
**Note for 11-17 / 11-18:** an accept answering OOB updates from a Pattern A modal needs the after-swap variant too.
**Reversible:** yes.

### D-11-16-7 — Display: one card per item, escaped text, unknown shapes as JSON
**Context:** NFR47; four item contracts (11-12, 11-13, 11-14, 11-15).
**Chosen:** `services/proposal_review.rs::parse_line` turns each item into a display line (series name / type / total, target series, title + position + omnibus range, expected tome, metadata field current → proposed with `current_date` and the truncation note, contributors current / to add, cover); anything else is shown as pretty-printed JSON text, never dropped. Templates never use `|safe` on agent data. Dates as `YYYY-MM-DD HH:MM UTC`. Template labels go through `ProposalLabels::t(key)` (`proposals.*`); router tests assert no raw key leaks.
**Reversible:** yes.

### D-11-16-8 — `#feedback-list` on the new pages; #526 stays open
**Context:** #526 (series page without `#feedback-list`).
**Chosen:** both proposals pages carry a `#feedback-list`; the series page itself is not changed here (out of scope).
**Reversible:** yes.

### D-11-16-9 — E2E scope; closes #536
**Chosen:** `proposals-review.spec.ts` (spec id `PR`): a librarian from a blank browser opens a metadata proposal (escaped value, source in a new tab), rejects it with a reason through the modal (reason and decider shown, survive a reload), then the agent's equivalent re-filing is refused with the reopening date and `list_my_proposals` shows the reason and `resubmittable_from`; a cover proposal shows its staged image until a reason-less rejection discards it. Proposals target the spec's own titles (no cross-spec blocking); no series created (#528).
**Reversible:** yes.

### D-11-16-10 — Code review round 1: version from the page, pending-only covers, stale named as such [PRODUCT]
**Context:** three layers (blind, edge cases, acceptance); no High. Medium: a pending cover whose staged file is missing showed a broken image; the reject modal re-read the current version, so "changed since you opened it" could never fire; the anonymous-reject test accepted any non-200; leak checks did not cover every line kind.
**Chosen:** (a) the detail page shows the staged image only when the proposal is pending **and** the file exists, else "the proposed image is no longer available"; `/proposals/{id}/cover` also answers 404 for any decided proposal (a failed discard no longer leaves a rejected image reachable). (b) The Reject button sends the version the page showed (`/reject-modal?version=N`) and the modal posts it back. (c) Rejecting a stale proposal says it is outdated (subject changed or key revoked), not "someone else decided" (`proposals.error_stale`). (d) Lows fixed: CRLF line breaks count as one character (as the textarea's `maxlength` does); a page past the end links back to the last page; a post-commit re-render failure answers `HX-Redirect` to the detail instead of an error; only `http(s)` sources become links (defence in depth); a deleted cover target is named; "Series —" instead of a hard-coded French " :"; four unused locale keys removed; stale comments fixed. Tests: contributors lines, deleted-target text, missing / stale cover, version from the page, stale refusal, CRLF, page past the end, stricter anonymous assertion, leak checks on the rejected body and the cover detail.
**Deferred:** #538 (focus lands on `<body>` after the swap removes the Reject trigger — `modal.js`).
**Accepted (Low):** a forged non-numeric `version` gets Axum's plain 4xx; `UnknownDecider` answers 409; non-HTMX `reject-modal` answers 405 (project convention); error copy under `proposals.error_*` (precedent `expected_tome.errors.*`); no router test for a missing staging directory (the same `None` branch as a missing file); malformed `is_omnibus` / cover dimensions are not reachable through filing validation. Note for 11-18: after an acceptance the cover column will say "no longer available" — 11-18 should show the promoted cover instead.
**Reversible:** yes.

### D-11-16-11 — Code review round 2: the modal requires the page's version
**Context:** three layers (blind, edge cases, acceptance); no High. One Medium, raised by two layers: when `/proposals/{id}/reject-modal` had no `version`, or one that did not parse, it fell back to the current version, so the "changed since you opened it" check could never fire for that request.
**Chosen:** the modal refuses a missing or unparsable `version` with the localized "changed" conflict (409), as the POST already did. A version that has moved still opens the modal; the POST refuses it, as D-11-16-10(b) decided and tests. Lows fixed:
- the detail page and the cover route share one predicate (`servable_staged_name`: a pending **cover** proposal), so a non-cover proposal can never show a broken image;
- a rejection that unexpectedly comes back `WentStale` answers the localized stale refusal instead of a 500;
- a cover item's dimensions and size stay visible once the image is gone (AC3);
- the unused `media_type` read on the title brief is removed.

Tests: a missing, empty or garbled modal version answers 409; dimensions are shown on a stale cover; a unit test of the shared predicate.
**Dismissed:** "decided by the system" for a purged human decider (reads use `COALESCE(u.username, p.decided_by_name)`, the copy made at decision time, D-11-9-11); a duplicate `#feedback-list` (the base layout has none); several cover lines (a cover proposal holds exactly one item).
**Accepted (Low):**
- a non-HTMX POST to `/reject` gets the fragment (it is CSRF-guarded; there is no no-JS path);
- a non-numeric POST `version` gets Axum's 422 (as in round 1);
- the cover image can 404 in a second tab after a rejection elsewhere (the modal re-checks the status);
- the `HX-Redirect` fallback after a commit leads to "not found" if the proposal was also soft-deleted in between (nothing in the UI soft-deletes proposals);
- "Page N of M" with N > M when the page is past the end (the previous-page link already leads back);
- index-aligned side vectors in the template (built from the same `lines`).

Focus after the swap remains #538.

**Round 3** (on the round-2 delta) found no Medium+. Its Lows were fixed:
- the decided-modal and stale-modal tests now send the current version and assert the status-specific copy, so they no longer pass through the version branch;
- the cover dimensions line is hidden when `width`, `height` or `bytes` is missing;
- the unreachable `WentStale` arm and the `has_staged_cover` doc are commented accurately.
**Reversible:** yes.

## § 11-17 — Accept a series proposal (part of #493)

### D-11-17-1 — Sprint-status: the story line plus the line-2 header
**Context:** Rule 16 and the common brief.
**Chosen:** only `11-17-accept-series-proposal` and the `# last_updated:` header change on this branch.
**Reversible:** yes.

### D-11-17-2 — #521 for the series kinds: re-validate the catalog's state at acceptance, bump no version [PRODUCT]
**Context:** #521 — `title_series` attach / detach and expected-tome writes bump neither `titles.version` nor `series.version`, so a version comparison misses "a position was taken", "the title was attached by hand", "a tome was entered" since filing.
**Chosen:** acceptance keeps the version check (`check_staleness_in`) **and** re-runs the filing rules against the series as it is now, under the series row lock (locking reads of `title_series` and the live tomes): title already in the series, position held by a live title, position or ISBN of a live expected tome, position above a closed total; for a creation, an active series with the same name (the filing comparison, case- and accent-insensitive). Any such conflict marks the proposal `stale` (committed) with a reason naming the title / tome / position. The filing checks were extracted into one pure `series_proposals::check_against_series` used by both. No writer was changed to bump versions.
**For 11-18 (#521 narrowed to it):** a metadata acceptance compares the item's stored `current` values with the title's live values under the title lock (and skips contributors already present); a cover acceptance compares the cover state recorded at filing with the live `cover_image_url`. A mismatch = stale, same pattern.
**Rejected:** bumping `titles.version` / `series.version` from every `title_series` / tome writer (an open title or series edit form would then fail with a version conflict after a scan-settle or a hand assignment — the user-visible cost D-11-9-8 avoided).
**Reversible:** yes.

### D-11-17-3 — One short transaction; lock order proposal → items → series → titles
**Context:** D-11-9-9 / -11 / -12; auto-settle locks the series row first and its `title_series` insert share-locks the title through the FK.
**Chosen:** `lock_for_decision` (kind, status, version — optimistic lock on the version the page showed) → items (`LOCK IN SHARE MODE`) → for an attach `lock_series_in` → `check_staleness_in` (titles then series, already held) → re-validation → `transition_in(MarkStale)` + commit on a conflict, else `transition_in(Accept)` → apply (series, then titles, then tomes through `ExpectedTomeService::create_in`, origin `proposal`) → commit. Every read before the series lock is a locking read, so no snapshot predates it. No file or network I/O inside.
**Reversible:** yes.

### D-11-17-4 — #529: a tome whose book the series already holds elsewhere is skipped and reported [PRODUCT]
**Context:** #529-1 — a tome line whose ISBN is carried by a title that this acceptance (or the series already) places at another position; `create_in` would leave it unsettled next to its own book (`TitleElsewhere`).
**Chosen:** before creating each tome, `ExpectedTomeService::owner_elsewhere_in` applies the owner choice of `settle_with_owned_title`; when that owner sits at another position of the series the tome is not created and the success entry says so ("its book «T» is already at position N — fix the position on the series page if one is wrong"). #529-2 (two keys) and #529-3 (title in two proposals) are covered by D-11-17-2: the second acceptance goes stale with the reason. #529 closed.
**Rejected:** refusing the whole acceptance (the librarian would lose the rest of the proposal until 11-19 lets them edit it); creating the tome anyway (a gap marker for a book already in the series).
**Reversible:** yes.

### D-11-17-5 — Answers: stale is a decision, a failed apply is a refusal [PRODUCT]
**Context:** AC "nothing is applied, the proposal is marked stale, and the message explains why"; AC "a failure rolls back and the proposal stays pending"; D-11-16-6.
**Chosen:** accepted **and** went-stale both answer 200 with the re-rendered `#proposal-detail-body`, an OOB entry (success with "Open the series" link / warning "Proposal #N is now outdated and nothing was added: <why>. Ask the assistant for a new proposal…") and `HX-Trigger-After-Swap: modal-close`. A failed apply (database error, unexpected conflict) rolls back and answers 409 "could not be applied, nothing was changed and it is still pending" in the modal — never the database text. Already decided / changed since opened / unknown / unknown decider reuse 11-16's copy.
**Rejected:** a 409 for stale (the page would keep showing "pending" with an Accept button behind the modal).
**Reversible:** yes.

### D-11-17-6 — Stored lines that cannot be applied are refused, not marked stale [PRODUCT]
**Context:** filing validates every line, but 11-19 will let a librarian edit items; a malformed or self-contradictory proposal is not "the catalog changed".
**Chosen:** stored items are parsed back and re-validated (`validate_series_name`, `validate_total`, `validate_lines`); a failure answers 400 "cannot be applied as it is… reject it" and leaves the proposal pending. Only state conflicts (D-11-17-2) mark it stale; a position above a closed total that was lowered since filing is a state conflict.
**Reversible:** yes.

### D-11-17-7 — `proposals.applied_series_id` links the series a creation produced [PRODUCT]
**Context:** "show the applied result"; a `series_create` item names no target, so a reloaded page could not link the created series.
**Chosen:** migration `20261029180000` adds `applied_series_id BIGINT UNSIGNED NULL` (no FK — the 30-day purge must not be blocked), written in the acceptance transaction; the detail page's subject then links the live series. Accepting bumps no `series` / `titles` version (D-11-10-6 precedent); the created series has no description.
**Rejected:** looking the series up by name at display time (breaks on a rename); writing a target into the agent's item (mutates the proposal as filed).
**Reversible:** yes (migration).

### D-11-17-8 — Accept UI: primary button, `warning` modal variant, summary of what is applied
**Context:** UX-DR8; the modal macro's variants (red delete, amber remove, indigo warning).
**Chosen:** "Accept" sits before "Reject" (indigo primary button) on pending `series_create` / `series_attach` proposals only; the modal (`warning`, indigo confirm) says whether the series is created or completed, how many titles and expected tomes, and that a changed catalog means nothing is added. `accept-modal` requires the page's `version` like `reject-modal` (D-11-16-11). The reject handler now shares the post-decision answer (`decided_response`) with accept.
**Reversible:** yes.

### D-11-17-9 — E2E scope
**Chosen:** `series-proposal-accept.spec.ts` (spec id `SA`, sorts after the specs that assume no series — D-11-11-6, #528): an agent files a `series_create` over MCP after the titles' metadata fetch settled (it bumps their version); a librarian from a blank browser accepts it in the modal, sees "accepted", follows the subject link to the series page and finds both titles and both expected tomes; a stale path where the series is renamed before acceptance. State-based staleness, the #529 cases and the rollback are integration-tested (`tests/proposal_accept_series.rs`, rollback through a test trigger on `series_expected_tomes`).
**Reversible:** yes.

### D-11-17-10 — Code review round 1: creations serialised by name, validated values applied, transient conflicts retried [PRODUCT]
**Context:** three layers (blind, edge cases, acceptance); no High. Medium, raised by all three: two concurrent acceptances of `series_create` with one name both passed the plain same-name read and both created the series (#529-2 only held sequentially). Medium (blind): the apply re-derived the name (`trim`) and total (`try_from(..).ok()`) from the raw item instead of the validated values. Medium (auditor): no test of the second of two real proposals (attach on the same positions, create of the same name).
**Chosen:**
- the creation's same-name check is a locking read (`ProposalModel::lock_series_named`, `FOR UPDATE`, taken **before** the title rows — series before titles, like an attach): concurrent creations of one name wait for each other and the second goes stale with "a series named «X» now exists"; on an empty `series` table the two gap locks can deadlock instead, so `series_acceptance::accept` retries a deadlock / lock-wait timeout up to 3 times on a fresh transaction (`loans::is_transient_conflict`, now `pub(crate)`, the #16 pattern) — which also covers a scan-settle or hand assignment deadlocking with an acceptance;
- the series is created from the validated name and total;
- `owner_elsewhere_in` considers every live title carrying the ISBN (a second copy / edition already in the series skips the tome; one holding the tome's position lets it settle);
- a garbled or overflowing POST `version` answers the localized "changed" conflict (not Axum's 422); the modal query struct is shared as `VersionQuery`; the counts sentence starts with a capital.
Tests: second of two attaches / creates, concurrent creations (one accepted, one stale, one series), escaping of the agent's name in the success entry, exact counts sentence, garbled version, multi-owner skip.
**Accepted (Low):** a commit that fails on a lost connection is reported as "nothing changed" (rare, and the page reload shows the truth); an attach whose tome lines are all skipped is still recorded as accepted (the entry reports zero additions and each skip); `live_title_name_in` sits on the expected-tome model next to the other title reads it uses; `set_applied_series_in` relies on the proposal row lock held by the transaction; the modal opens on a moved version and the POST refuses it (D-11-16-10 decision); a non-HTMX POST gets the fragment (CSRF-guarded, as reject); a scan committing during a creation's acceptance can leave a new tome unsettled until the next catalogue event of its ISBN; the `#id` name fallbacks are unreachable (rows read under lock).
**Dismissed:** "soft-deleted titles' assignments are invisible" — a soft-deleted title holds no position anywhere in the app (`position_holder_in`, the grid), and `title_series` has no unique key on a series position.
**Reversible:** yes.

### D-11-17-11 — Code review round 2: retry deadlocks only; whole-table lock of a creation accepted
**Context:** two layers on the round-1 delta and the full diff. Medium (blind): a lock-wait timeout (1205) was retried, so one accept could hold a request ~150 s (3 × `innodb_lock_wait_timeout`). Other blind "Mediums" were checked and dismissed: deadlocks from `create_in` / `transition_in` keep their `AppError::Database` and are retried (confirmed by the edge layer); the manual series form's own unlocked name check predates this story (it now waits behind an accepting creation, but its check-then-insert race with itself is unchanged).
**Chosen:** `accept` retries only SQLSTATE 40001 (local `is_deadlock`; `loans::is_transient_conflict` back to private); a deadlock that will be retried is logged as a warning, not as "not applied"; `accept_in` is `pub(crate)` (callers go through `accept`, which owns the transaction and the retry); module, `active_series_named` and `owner_elsewhere_in` docs updated; a second concurrency test runs on a non-empty `series` table (the queueing path, not the deadlock path).
**Accepted (Low):** a creation's acceptance X-locks the scanned `series` rows for its short transaction (renames, attach acceptances and scan-settles wait for it) — the price of serialising creations without a unique key or a session-scoped `GET_LOCK`; a duplicated `version` field gets Axum's 422 (hand-built request only); a proposal both stale and holding a now-invalid name answers "cannot be applied" rather than "outdated" (D-11-17-6 order).
**Reversible:** yes.

### D-11-17-12 — Code review round 3: clean; Lows fixed or accepted
**Context:** confirmation pass on the round-2 delta (both skills); no Medium+. Confirmed: sqlx-mysql's `code()` is the SQLSTATE, MariaDB 10.11 reports a deadlock as 1213 / 40001, every path carries it to the retry as `AppError::Database`.
**Chosen:** `accept_in` private; the retry warning carries the error; the non-empty-table concurrency test says it checks the outcome, not which path ran.
**Accepted (Low):** no unit test can build a `MySqlDatabaseError` for the positive 40001 case (the DB-backed concurrency tests are the live check, same gap as `loans.rs`); a future MariaDB ≥ 11.8 with `innodb_snapshot_isolation` could surface REPEATABLE READ conflicts as 1020, not retried — to revisit if the pinned image moves.
**Reversible:** yes.
