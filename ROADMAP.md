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

**Current stable: [`v1.12.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.12.0)** — a 2-issue feature minor on v1.11.0 (shipped 2026-07-11, same day): [#427](https://github.com/guycorbaz/mybibli/issues/427) BnF Couvertures + Inventaire.io cover fallbacks (live-tested to recover ~half of the previously unfindable FR/CH covers), [#428](https://github.com/guycorbaz/mybibli/issues/428) highest used V/L-code line on /catalog for label printing; no migration; see the v1.12.0 section below. The prior release [`v1.11.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.11.0) — a 4-issue minor on v1.10.0 (shipped 2026-07-11), bundling the July production-log review: [#418](https://github.com/guycorbaz/mybibli/issues/418) persistent session cookie + admin-configurable inactivity timeout, [#419](https://github.com/guycorbaz/mybibli/issues/419) bulk cover-refetch pacing/back-off/summary, [#416](https://github.com/guycorbaz/mybibli/issues/416) auto-purge unblocked, [#417](https://github.com/guycorbaz/mybibli/issues/417) log-noise fix; one additive settings migration; see the v1.11.0 section below. The prior release [`v1.10.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.10.0) — a single-feature minor on v1.9.1 (shipped 2026-07-01): [#9](https://github.com/guycorbaz/mybibli/issues/9) undo the last scan action (shelving a volume or activating a batch location) from the catalog feedback list within a 30-second window; no migration; see the v1.10.0 section below. The prior line [`v1.9.1`](https://github.com/guycorbaz/mybibli/releases/tag/v1.9.1) — a 2-issue patch on v1.9.0 (shipped 2026-06-11): [#403](https://github.com/guycorbaz/mybibli/issues/403) the last two client-side messages (modal-busy guard, server-error feedback) now come from the #i18n-bundle data island and localize in German/Italian, [#412](https://github.com/guycorbaz/mybibli/issues/412) CI de-flake of the recent-activity boundary tests; see the v1.9.1 section below. The underlying feature minor [`v1.9.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.9.0) — 4 issues on v1.8.6 (same day): [#367](https://github.com/guycorbaz/mybibli/issues/367) saved custom searches (name and save a home search composition — free text + filter + sort — then re-run, rename or delete it from a dropdown next to the sort control), [#396](https://github.com/guycorbaz/mybibli/issues/396) per-provider metadata-timeout overrides in /admin > System, [#406](https://github.com/guycorbaz/mybibli/issues/406) admin log-level form no longer 409s on consecutive saves, and [#405](https://github.com/guycorbaz/mybibli/issues/405) transport-crate log noise capped at the default level; see the v1.9.0 section below. The feature build for v1.1 through v1.8 is complete; v1.8 was the last *themed* minor and the project is in **GH-issue-driven polish mode** — subsequent minors (v1.9.0 is the first) bundle the issue-driven work that crossed the feature threshold, patches ship independently for production bugs, and new feature CRs queue in the parking lot until a coherent theme emerges or a roadmap-slotted candidate (e.g., the v2.0 classification refactor) is greenlit. Release-by-release history: the per-version sections below, the [GitHub releases page](https://github.com/guycorbaz/mybibli/releases), and chapter 8 of the user manual.

Open issues against the live line:

- [#403](https://github.com/guycorbaz/mybibli/issues/403) — [Tech debt] Sweep inline-form.js + mybibli.js onto the #i18n-bundle pattern. Backlog; not slotted.
- [#389](https://github.com/guycorbaz/mybibli/issues/389) — [CR] UNIMARC conformance for title documentation. Backlog; not slotted.
- [#206](https://github.com/guycorbaz/mybibli/issues/206) / [#202](https://github.com/guycorbaz/mybibli/issues/202) — classification surface + provider-chain reliability; queued for the v2.0 exploration.

## v1.12.0 — cover sources + label helper *(shipped)*

A 2-issue feature minor on v1.11.0 (shipped 2026-07-11, same day). No migration, no operator action.

- [#427](https://github.com/guycorbaz/mybibli/issues/427) — **Two new cover sources.** When metadata resolves without a cover, the chain now tries the *BnF Couvertures* service (legal-deposit cover scans — the strongest source for FR/CH publishers, print-quality images) and *Inventaire.io* (libre community database, strong on FR comics/manga) before the Open Library fallback. Live-tested against the production catalog: 53 of the 112 missing covers (47%) become recoverable. Also fixes a latent bug where covers served from non-TLS hosts could never download (unconditional http→https rewrite).
- [#428](https://github.com/guycorbaz/mybibli/issues/428) — **Label-printing helper.** /catalog shows the highest V-code and L-code already in use (trash included — a printed sticker is never reissued), so fresh label sheets start after them.

## v1.11.0 — production-log review bundle *(shipped)*

A 4-issue minor on v1.10.0 (shipped 2026-07-11), driven end-to-end by the 2026-07-10 production-log review. One additive settings migration (seeds the bulk-refetch delay row); applies automatically — no operator action.

- [#418](https://github.com/guycorbaz/mybibli/issues/418) — **Sessions survive a tablet screen-lock.** The session cookie is now persistent (lifetime = the inactivity timeout) instead of a browser-session cookie, so iPads that discard session cookies on lock no longer log the librarian out mid-cataloguing. The inactivity timeout is admin-configurable in hours (1–720, default 4) under a new /admin > System > Sessions section.
- [#419](https://github.com/guycorbaz/mybibli/issues/419) — **Bulk cover-refetch actually recovers covers.** The *Re-fetch missing covers* action paces its requests (new `bulk_refetch_delay_ms` setting, default 1 s/title), retries 429/503-throttled lookups with 5 s → 20 s back-off (new typed 503 signal end-to-end through the provider chain), and reports a completion summary on the Health tab — covers recovered / provider errors / no cover available. Two prod runs over ~113 titles had previously tripped Google Books' 503 burst-throttling and silently recovered ~0.
- [#416](https://github.com/guycorbaz/mybibli/issues/416) — **Daily auto-purge unblocked.** Orphan `sessions` rows referencing a >30-day-soft-deleted user (any path outside the story 8-3 deactivation handler: seed state, direct SQL) blocked the user hard-purge forever via the RESTRICT FK — a daily ERROR since 2026-06-20. The purge now wipes those rows in the same transaction; the counts surface in the audit payload.
- [#417](https://github.com/guycorbaz/mybibli/issues/417) — **Log-noise fix.** The dashboard Gaps chips (`uncategorized` / `no_volumes` / `no_cover`) are valid search-side filters; the indicator-filter parser now ignores them silently instead of WARN-logging every click (genuinely unknown values still warn).

## v1.10.0 — undo recent scan actions *(shipped)*

A single-feature minor on v1.9.1 (shipped 2026-07-01). No migration, no operator action.

- [#9](https://github.com/guycorbaz/mybibli/issues/9) — **Undo a scan action.** When a librarian shelves a volume or activates a batch storage-location on the catalog page, the feedback message now carries an **Undo** button. Clicking it within a server-authoritative 30-second window reverses that last action: a shelving returns the volume to its previous location (or leaves it unshelved), and a batch-location activation is reverted. Only the most recent action is undoable, and undo never deletes a title or volume that was just created. Implemented via a per-session undo log in the existing `sessions.data` blob (no schema change) + a new `POST /catalog/undo` endpoint; the button is CSP-clean and CSRF-protected like every other state-changing request.

## v1.9.1 — i18n sweep + CI de-flake *(shipped)*

A 2-issue patch on v1.9.0 (shipped 2026-06-11). No migration, no operator action.

- [#403](https://github.com/guycorbaz/mybibli/issues/403) — **i18n-bundle sweep.** `inline-form.js` (modal-busy guard) and `mybibli.js` (htmx:responseError feedback) drop their hand-synced `{en, fr}` string objects and read the server-rendered `#i18n-bundle` data island — both messages localize in de/it for the first time. Completes the #386 follow-up.
- [#412](https://github.com/guycorbaz/mybibli/issues/412) — **Test de-flake.** The recent-activity boundary tests raced the second hand between two `NOW()` calls (backdate UPDATE vs count SELECT); boundary rows now carry clock-skew slack. Test-only.

## v1.9.0 — saved searches + per-provider timeouts *(shipped)*

A 4-issue feature minor on v1.8.6 (shipped 2026-06-11). One new database migration (seeds the per-provider timeout rows); applies automatically — no operator action required.

- [#367](https://github.com/guycorbaz/mybibli/issues/367) — **Saved custom searches.** A home-page search composition (free-text `q` + filter chip + sort column/direction) can be saved under a name and re-run in one click from a *Saved searches* dropdown next to the sort control; entries are renamed/deleted via UX-DR8 modals from the same dropdown. Instance-wide (single-tenant model, like the settings table) — no per-user scoping. The query-builder depth stays deferred to the v2.0 classification work (#206).
- [#396](https://github.com/guycorbaz/mybibli/issues/396) — **Per-provider metadata-timeout overrides.** The v1.7.9 scalar (`metadata_chain_per_provider_timeout_secs`) becomes the fallback default; /admin > System > Metadata Providers gains one timeout field per registered provider (`provider_timeout.<slug>` K/V rows, empty = use default, 1–60 s bounds). Resolution is hot — the chain reads the override map on every fetch. Split out of #384 during the v1.8.5 robustness sweep.
- [#406](https://github.com/guycorbaz/mybibli/issues/406) — **Admin log-level form 409 fix.** `fetch_setting_rows` omitted `log_level` (and the two #334 timeout keys) from its `IN` list, so the form re-rendered with a hardcoded version 1 and every save after the first 409'd forever. The keys are now in the list and the regression class is documented in the function contract.
- [#405](https://github.com/guycorbaz/mybibli/issues/405) — **Transport-crate log hygiene.** `combine_log_directives` prepends `hyper_util=warn,reqwest=warn,hyper=warn` to the operator's directive on both the boot path and the runtime-reload path, killing ~2 MB/day of connection-pool DEBUG chatter at global `debug`; an explicit operator directive for those crates still wins.

## Older shipped releases

v1.8.6 back to v1.2.0 — the seven themed minors ("See what you own, find it faster" → "Covers, honestly") and their patch trains — are documented release-by-release on the [GitHub releases page](https://github.com/guycorbaz/mybibli/releases) and in chapter 8 of the user manual. This file only keeps the current minor and its immediate predecessors.

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
