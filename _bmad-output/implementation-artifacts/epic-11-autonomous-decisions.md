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
