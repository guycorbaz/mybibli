# mybibli Roadmap

Living plan for what ships next. The version slots are intentional, the
order is broadly stable, and the dates are deliberately absent — mybibli
is a side-project; we ship versions when they're ready, not when a
quarter ends.

The companion site at
[guycorbaz.github.io/mybibli/roadmap.html](https://guycorbaz.github.io/mybibli/roadmap.html)
gives the marketing-friendly view of the same plan. This file is the
canonical, complete one — including the patch-line maintenance policy
and the tech-debt parking lot.

## Versioning policy

mybibli follows [Semantic Versioning](https://semver.org/) 2.0.

- **Patch releases (`1.1.x`)** — bug fixes and known-failure
  resolutions only. No new features land on a patch line, no breaking
  changes, no schema migrations beyond what the fix strictly needs.
  Routine `docker compose pull && docker compose up -d` is always
  enough.
- **Minor releases (`1.2.0`, `1.3.0`, …)** — bundles of new features
  grouped by theme. Backwards-compatible; may ship additive schema
  migrations. The roadmap below is structured around these minors.
- **Major releases (`2.0.0`, …)** — reserved for breaking changes:
  schema migrations that drop columns, route renames, env-var removals.
  None planned at the time of writing — the
  [#206](https://github.com/guycorbaz/mybibli/issues/206) classification
  refactor is the only candidate, and even that may land on a minor if
  we find a backwards-compatible path.

## Now

**Current stable: [`v1.9.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.9.0)** — a 4-issue feature minor on v1.8.6 (shipped 2026-06-11): [#367](https://github.com/guycorbaz/mybibli/issues/367) saved custom searches (name and save a home search composition — free text + filter + sort — then re-run, rename or delete it from a dropdown next to the sort control), [#396](https://github.com/guycorbaz/mybibli/issues/396) per-provider metadata-timeout overrides in /admin > System, [#406](https://github.com/guycorbaz/mybibli/issues/406) admin log-level form no longer 409s on consecutive saves, and [#405](https://github.com/guycorbaz/mybibli/issues/405) transport-crate log noise capped at the default level; see the v1.9.0 section below. The feature build for v1.1 through v1.8 is complete; v1.8 was the last *themed* minor and the project is in **GH-issue-driven polish mode** — subsequent minors (v1.9.0 is the first) bundle the issue-driven work that crossed the feature threshold, patches ship independently for production bugs, and new feature CRs queue in the parking lot until a coherent theme emerges or a roadmap-slotted candidate (e.g., the v2.0 classification refactor) is greenlit. Release-by-release history: the per-version sections below, the [GitHub releases page](https://github.com/guycorbaz/mybibli/releases), and chapter 8 of the user manual.

Open issues against the live line:

- [#403](https://github.com/guycorbaz/mybibli/issues/403) — [Tech debt] Sweep inline-form.js + mybibli.js onto the #i18n-bundle pattern. Backlog; not slotted.
- [#389](https://github.com/guycorbaz/mybibli/issues/389) — [CR] UNIMARC conformance for title documentation. Backlog; not slotted.
- [#206](https://github.com/guycorbaz/mybibli/issues/206) / [#202](https://github.com/guycorbaz/mybibli/issues/202) — classification surface + provider-chain reliability; queued for the v2.0 exploration.
- [#9](https://github.com/guycorbaz/mybibli/issues/9) — [CR] Undo for recent scan actions. Deferred post-MVP.

## v1.9.0 — saved searches + per-provider timeouts *(shipped)*

A 4-issue feature minor on v1.8.6 (shipped 2026-06-11). One new database migration (seeds the per-provider timeout rows); applies automatically — no operator action required.

- [#367](https://github.com/guycorbaz/mybibli/issues/367) — **Saved custom searches.** A home-page search composition (free-text `q` + filter chip + sort column/direction) can be saved under a name and re-run in one click from a *Saved searches* dropdown next to the sort control; entries are renamed/deleted via UX-DR8 modals from the same dropdown. Instance-wide (single-tenant model, like the settings table) — no per-user scoping. The query-builder depth stays deferred to the v2.0 classification work (#206).
- [#396](https://github.com/guycorbaz/mybibli/issues/396) — **Per-provider metadata-timeout overrides.** The v1.7.9 scalar (`metadata_chain_per_provider_timeout_secs`) becomes the fallback default; /admin > System > Metadata Providers gains one timeout field per registered provider (`provider_timeout.<slug>` K/V rows, empty = use default, 1–60 s bounds). Resolution is hot — the chain reads the override map on every fetch. Split out of #384 during the v1.8.5 robustness sweep.
- [#406](https://github.com/guycorbaz/mybibli/issues/406) — **Admin log-level form 409 fix.** `fetch_setting_rows` omitted `log_level` (and the two #334 timeout keys) from its `IN` list, so the form re-rendered with a hardcoded version 1 and every save after the first 409'd forever. The keys are now in the list and the regression class is documented in the function contract.
- [#405](https://github.com/guycorbaz/mybibli/issues/405) — **Transport-crate log hygiene.** `combine_log_directives` prepends `hyper_util=warn,reqwest=warn,hyper=warn` to the operator's directive on both the boot path and the runtime-reload path, killing ~2 MB/day of connection-pool DEBUG chatter at global `debug`; an explicit operator directive for those crates still wins.

## v1.8.6 — 2-issue tech-debt closure *(shipped)*

A tech-debt patch on v1.8.5 (shipped 2026-06-04). No user-facing behavior change; no operator action required. The two issues are coupled — #398 is the enabler for #386.

- [#398](https://github.com/guycorbaz/mybibli/issues/398) — **Embed `BaseContextFields` as a single `base` field.** Page-template structs flattened the ~20 shared base-context fields (`lang`, `role`, `csrf_token`, `nav_*`, …) one by one — 23 struct definitions + 23 construction sites, so adding any document-wide field to `layouts/base.html` was a 23×2 edit. They now embed the context as one `base: BaseContextFields` field; `base.html` + `nav_bar.html` + the inline-form pages read it via Askama nested-field access (`{{ base.lang }}`). **Net −1000 LOC.** Adding a global field is now one line in the struct + one in `base_context()`. Standalone HTMX fragment/modal structs keep their own flat `csrf_token` (rendered by their own structs, never via `base.html`).
- [#386](https://github.com/guycorbaz/mybibli/issues/386) — **Server-rendered i18n bundle for client JS.** `static/js/*.js` carried hand-synced `{en: …, fr: …}` objects that drifted from `locales/*.yml` and never covered de/it. `utils::build_js_i18n_bundle()` now resolves the JS-needed strings in the request locale and `layouts/base.html` emits them as a `<script type="application/json" id="i18n-bundle">` data island (`< > &` escaped to `\uXXXX` so a translated value can't break out). The session-timeout toast was migrated as the proof-of-concept — it now localizes in de/it for free. A follow-up ([#403](https://github.com/guycorbaz/mybibli/issues/403)) tracks sweeping the remaining two JS modules (needs net-new de/it locale copy).

## v1.8.5 — 3-issue closure patch *(shipped)*

A third consecutive open-issue-reduction patch on v1.8.4 (shipped 2026-06-04). Closes 3 open issues; no operator action required.

- [#10](https://github.com/guycorbaz/mybibli/issues/10) — **Error-message quality.** New [`docs/error-message-style.md`](https://github.com/guycorbaz/mybibli/blob/main/docs/error-message-style.md) style guide: field-validation hints stay terse; operation / conflict / system errors follow "What happened → Why → What you can do" in plain language — no HTTP codes, no jargon, no stack traces. The cryptic operation/system `error.*` copy was rewritten against the guide across all four locales (de/en/fr/it), and `AppError::Internal` now renders a generic, reassuring message instead of leaking internals. Foundation Rule #9 codifies the per-epic retrospective review of touched `error.*` keys.
- [#384](https://github.com/guycorbaz/mybibli/issues/384) — **Metadata-chain robustness (the 3 deferred sub-items from #23).** Open Library author resolution now fans out concurrently via `join_all` instead of resolving author records one at a time (was a latency cliff on multi-author titles); UPC-A checksum validation added to the barcode parser so a mistyped/misread 12-digit UPC is rejected at the boundary rather than producing a phantom lookup. (The per-provider configurable-timeout sub-item is tracked separately as [#396](https://github.com/guycorbaz/mybibli/issues/396).)
- [#391](https://github.com/guycorbaz/mybibli/issues/391) — **Maintainability — admin users handler extraction.** The admin users-panel handlers were extracted out of `admin.rs` into a focused `src/routes/admin_users.rs` module, resolving the pre-existing > 2000-line drift (Foundation Rule #12) that the #55 hardening work surfaced. Pure refactor — no behavior change, no schema migration.

## Older shipped releases

v1.8.4 back to v1.2.0 — the seven themed minors ("See what you own, find it faster" → "Covers, honestly") and their patch trains — are documented release-by-release on the [GitHub releases page](https://github.com/guycorbaz/mybibli/releases) and in chapter 8 of the user manual. This file only keeps the current minor and its immediate predecessors.

## v2.0.0 candidate — "Classification, reinvented"

The only currently-imagined breaking change. May land on a minor if a
backwards-compatible path is found.

- [#206](https://github.com/guycorbaz/mybibli/issues/206) — Split `genre` from auto-fetched metadata; introduce a "classification" surface combining Dewey + manual genre, with a migration that preserves existing data.

## Parking lot — no scheduled version

Tracked but not slotted. Will be picked up opportunistically when a
related area is being worked on, or rolled into a v1.x release if they
happen to fit a theme.

### Code-review findings (technical debt)

- [#220](https://github.com/guycorbaz/mybibli/issues/220) — CSRF whitelist parser handles comma-separated HX-Trigger only.
- [#217](https://github.com/guycorbaz/mybibli/issues/217) — `originatesFromConfirm` would tag `X-Modal-Confirm` on nested-form submits inside modals.
- [#216](https://github.com/guycorbaz/mybibli/issues/216) — `AppError::IntoResponse` returns HTML fragment for direct browser nav (non-HTMX).
- [#152](https://github.com/guycorbaz/mybibli/issues/152) — Add `anonymous_gets_200_on_series` test to `role_gating.rs`.
- [#151](https://github.com/guycorbaz/mybibli/issues/151) — Align UX spec + 9.18 epic spec nav-link tables with shipped implementation.
- [#148](https://github.com/guycorbaz/mybibli/issues/148) — `nav.js` burst detector — Bluetooth Android scanners emit `Unidentified`.
- [#147](https://github.com/guycorbaz/mybibli/issues/147) — `nav.js` `getBurstThresholdMs` accepts `n=1` (1ms) — pathological admin-config protection.
- [#146](https://github.com/guycorbaz/mybibli/issues/146) — E2E `navbar-hamburger` Test 5 — `simulateScan` trailing Enter race on `/login`.
- [#145](https://github.com/guycorbaz/mybibli/issues/145) — `nav.js` burst threshold (50ms) may be tight for very fast typists / mechanical keyboards.

### Larger CRs awaiting prioritization

- [#202](https://github.com/guycorbaz/mybibli/issues/202) — Broader per-provider visibility (metadata-source badge, per-provider retry, structured error surfacing). The docs slice already shipped in v1.5; the UX slice is deeper work.
- [#389](https://github.com/guycorbaz/mybibli/issues/389) — UNIMARC conformance for title cataloging. Align the title data model on the UNIMARC standard (zone/subfield mapping) so records are standard-conformant and interoperable with library systems (BnF, Koha, PMB). Deep data-model + architecture work; a documented field⇄zone mapping is the first slice. Periphery: an eventual UNIMARC import/export path (ISO 2709 / UNIMARC XML).

## Past releases

The shipped history is documented release-by-release at
[github.com/guycorbaz/mybibli/releases](https://github.com/guycorbaz/mybibli/releases)
and in chapter 8 ("Upgrade & migration") of the user manual under
`docs/manual/{en,fr}/`. The website's [roadmap page](https://guycorbaz.github.io/mybibli/roadmap.html)
also tells the story in a more digestible form.
