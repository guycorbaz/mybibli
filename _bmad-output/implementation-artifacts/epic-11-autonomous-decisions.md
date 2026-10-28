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
