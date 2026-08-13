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

**Current stable: [`v1.15.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.15.0)** — a 4-issue minor on v1.14.1 (shipped 2026-08-13), bundling the August production-log review: [#202](https://github.com/guycorbaz/mybibli/issues/202) titles now record and display which provider resolved their metadata, so a thin record can be told apart from one no provider answered; [#424](https://github.com/guycorbaz/mybibli/issues/424) light-mode contrast raised above the WCAG AA floor on the home and locations pages, where the offending elements only rendered once data existed; [#419](https://github.com/guycorbaz/mybibli/issues/419) a third retry tier for bulk metadata runs, measured from production logs — request spacing was shown NOT to be the lever; [#449](https://github.com/guycorbaz/mybibli/issues/449) the startup log shortens the build commit to 7 characters. **One additive migration** (`titles.metadata_source`, nullable — pre-existing rows read as "unknown provenance"); see the v1.15.0 section below. The prior release [`v1.14.1`](https://github.com/guycorbaz/mybibli/releases/tag/v1.14.1) — a one-issue patch on v1.14.0 (shipped 2026-07-28): [#447](https://github.com/guycorbaz/mybibli/issues/447) the application now logs its version, build commit and profile at startup, so a log file read in isolation identifies the build that produced it; no migration. The underlying minor [`v1.14.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.14.0) — a cataloging-fix and bibliographic-coverage minor on v1.13.0 (shipped 2026-07-28): [#440](https://github.com/guycorbaz/mybibli/issues/440) volume labels no longer attach to the previous title when several UPC items are catalogued in a row, [#441](https://github.com/guycorbaz/mybibli/issues/441) a deleted active title now says so instead of blaming the scanned label, [#442](https://github.com/guycorbaz/mybibli/issues/442) a deleted volume's V-code label is reusable without an admin purge (refused when it carries loan history), and [#439](https://github.com/guycorbaz/mybibli/issues/439) Library of Congress MARC 21 records fill the UNIMARC-aligned fields for English-language books that the BnF does not hold — the two national libraries now complete each other field by field, first source wins; **no migration**; note that French-prefix titles the BnF could not describe stay empty, LoC does not hold them either; see the v1.14.0 section below. The prior release [`v1.13.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.13.0) — a UNIMARC-themed feature minor on v1.12.0 (shipped 2026-07-24): [#389](https://github.com/guycorbaz/mybibli/issues/389) six UNIMARC-aligned cataloging fields (200$f, 205$a, 225$a/$v, 300$a, 500$a) captured from the BnF at scan time and shown on the title page, plus a Health-tab *Backfill metadata from BnF* bulk action for already-cataloged titles; [#434](https://github.com/guycorbaz/mybibli/issues/434) per-title cataloging log summary; one additive migration; record import/export (ISO 2709 / UNIMARC XML) queued separately as [#436](https://github.com/guycorbaz/mybibli/issues/436); see the v1.13.0 section below. The prior release [`v1.12.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.12.0) — a 2-issue feature minor on v1.11.0 (shipped 2026-07-11, same day as v1.11.0): [#427](https://github.com/guycorbaz/mybibli/issues/427) BnF Couvertures + Inventaire.io cover fallbacks (live-tested to recover ~half of the previously unfindable FR/CH covers), [#428](https://github.com/guycorbaz/mybibli/issues/428) highest used V/L-code line on /catalog for label printing; no migration; see the v1.12.0 section below. The prior release [`v1.11.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.11.0) — a 4-issue minor on v1.10.0 (shipped 2026-07-11), bundling the July production-log review: [#418](https://github.com/guycorbaz/mybibli/issues/418) persistent session cookie + admin-configurable inactivity timeout, [#419](https://github.com/guycorbaz/mybibli/issues/419) bulk cover-refetch pacing/back-off/summary, [#416](https://github.com/guycorbaz/mybibli/issues/416) auto-purge unblocked, [#417](https://github.com/guycorbaz/mybibli/issues/417) log-noise fix; one additive settings migration; see the v1.11.0 section below. The prior release [`v1.10.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.10.0) — a single-feature minor on v1.9.1 (shipped 2026-07-01): [#9](https://github.com/guycorbaz/mybibli/issues/9) undo the last scan action (shelving a volume or activating a batch location) from the catalog feedback list within a 30-second window; no migration; see the v1.10.0 section below. The prior line [`v1.9.1`](https://github.com/guycorbaz/mybibli/releases/tag/v1.9.1) — a 2-issue patch on v1.9.0 (shipped 2026-06-11): [#403](https://github.com/guycorbaz/mybibli/issues/403) the last two client-side messages (modal-busy guard, server-error feedback) now come from the #i18n-bundle data island and localize in German/Italian, [#412](https://github.com/guycorbaz/mybibli/issues/412) CI de-flake of the recent-activity boundary tests; see the v1.9.1 section below. The underlying feature minor [`v1.9.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.9.0) — 4 issues on v1.8.6 (same day): [#367](https://github.com/guycorbaz/mybibli/issues/367) saved custom searches (name and save a home search composition — free text + filter + sort — then re-run, rename or delete it from a dropdown next to the sort control), [#396](https://github.com/guycorbaz/mybibli/issues/396) per-provider metadata-timeout overrides in /admin > System, [#406](https://github.com/guycorbaz/mybibli/issues/406) admin log-level form no longer 409s on consecutive saves, and [#405](https://github.com/guycorbaz/mybibli/issues/405) transport-crate log noise capped at the default level; see the v1.9.0 section below. The feature build for v1.1 through v1.8 is complete; v1.8 was the last *themed* minor and the project is in **GH-issue-driven polish mode** — subsequent minors (v1.9.0 is the first) bundle the issue-driven work that crossed the feature threshold, patches ship independently for production bugs, and new feature CRs queue in the parking lot until a coherent theme emerges or a roadmap-slotted candidate (e.g., the v2.0 classification refactor) is greenlit. Release-by-release history: the per-version sections below, the [GitHub releases page](https://github.com/guycorbaz/mybibli/releases), and chapter 8 of the user manual.

Open issues against the live line:

- [#403](https://github.com/guycorbaz/mybibli/issues/403) — [Tech debt] Sweep inline-form.js + mybibli.js onto the #i18n-bundle pattern. Backlog; not slotted.
- [#436](https://github.com/guycorbaz/mybibli/issues/436) — [CR] UNIMARC Palier 2: record import/export (ISO 2709 `.mrc` / UNIMARC XML). Follow-up to #389 (Palier 1 shipped in v1.13.0). Backlog; not slotted.
- [#206](https://github.com/guycorbaz/mybibli/issues/206) — classification surface; queued for the v2.0 exploration.
- [#202](https://github.com/guycorbaz/mybibli/issues/202) — provider-chain reliability. Tier 1 (metadata provenance) shipped in v1.15.0; the structured failure surface and per-provider retry remain open.

## v1.15.0 — metadata provenance + accessibility *(shipped)*

Shipped 2026-08-13. Four issues, **one additive migration**.

The bundle came out of a review of the production logs on the household NAS covering 14 July to 13 August: 30 days of daily rotation, one process, no restart since the v1.14.1 deployment.

- [#202](https://github.com/guycorbaz/mybibli/issues/202) — the provider chain always knew which provider answered and then threw it away, so a librarian facing a thin record could not tell "the BnF answered but holds little" from "nothing answered at all". Titles now carry `metadata_source` and the detail page names it. The provider name is not translated — it is a proper noun. Tier 1 of three: the structured failure surface and per-provider retry stay open on the issue, and both depend on this one. A cache hit resolves metadata without naming a provider, so the write is skipped rather than erasing a previously recorded source.
- [#424](https://github.com/guycorbaz/mybibli/issues/424) — `text-stone-400` sat at 2.41:1 on the page background, below the WCAG 2.2 AA floor of 4.5:1. Measuring first changed the fix the issue proposed: `text-stone-500` still fails at 4.40:1 on the hover shade the locations rows take, so those controls went to `text-stone-600`. The home badge turned out to fail in **both** themes, not just light. Enforcement is now two-level — a static template audit plus a spec that seeds data before running axe, because both offending elements only render when the catalog is not empty and CI had been passing on scheduling luck.
- [#419](https://github.com/guycorbaz/mybibli/issues/419) — a third retry tier (60 s) for bulk metadata runs. Production evidence: of 25 throttled titles, 12 were rescued at 5 s and 4 at 20 s, leaving 9 written off with the schedule exhausted. The measurements also settled a standing hypothesis — **spacing requests further is not the lever**: the median gap before a 503 (4.24 s) and before a success (4.19 s) are indistinguishable, and the 503 rate does not follow the call rate. Roughly one unauthenticated Google Books call in two fails during a storm, independent of pacing. Per-title, so a clean run pays nothing.
- [#449](https://github.com/guycorbaz/mybibli/issues/449) — the startup line carried the full 40-character commit while its own documentation called it short. Shortened at emission, keeping the full hash on the struct for any future surface.

Upgrade: image swap plus one automatic additive migration. Rolling back to v1.14.1 is clean — the new column is nullable and unread by the previous binary.

## v1.14.1 — build identity in the log *(shipped)*

Shipped 2026-07-28. One issue, **no migration**.

- [#447](https://github.com/guycorbaz/mybibli/issues/447) — the startup log line now carries the version, the git commit the image was built from, and the build profile. Confirming which release is running previously meant leaving the log file for the host's `docker-compose.yml` — a detour made worse by releases with no migration, which leave no other trace to infer the version from. The commit is stamped in by CI as a Docker build argument; a locally-built binary reports `unknown` rather than claiming a commit it cannot verify.

## v1.14.0 — cataloging fixes + Library of Congress MARC 21 *(shipped)*

Shipped 2026-07-28. Four issues, **no migration** — reuses the columns added by #389, so the upgrade is an image swap against an unchanged schema and rolling back to v1.13.0 stays clean.

- [#440](https://github.com/guycorbaz/mybibli/issues/440) — `severity:high`. Cataloguing several UPC-identified items (typically CDs) in a row left the previous title active after the first, so the next volume label attached to the wrong item or failed with a misleading "not found". All scan paths now share one context activation.
- [#441](https://github.com/guycorbaz/mybibli/issues/441) — a V-code scan whose active title had been deleted reported "not found", blaming the label. It now reports that no item is active and clears the stale context.
- [#442](https://github.com/guycorbaz/mybibli/issues/442) — deleting a volume left its V-code locked until an admin purged it from the Trash, and the error named the owner "?". The label is now reusable; the previous copy's details are discarded and audited. Refused when the volume carries loan history, which reuse would re-attribute to a different copy.
- [#439](https://github.com/guycorbaz/mybibli/issues/439) — the Library of Congress provider now also fetches MARC 21 records over SRU, filling the six UNIMARC-aligned fields for English-language books. The two national libraries complete each other field by field, first source wins. Measured reach: ISBN prefixes 9780/9781 only — **French-prefix titles the BnF could not describe stay empty**. The Health-tab action is renamed *Backfill metadata from libraries*.

## v1.13.0 — UNIMARC-aligned cataloging *(shipped)*

A UNIMARC-themed feature minor on v1.12.0 (shipped 2026-07-24). One automatic, additive migration; no operator action required (the backfill below is optional and on-demand).

- [#389](https://github.com/guycorbaz/mybibli/issues/389) — **UNIMARC-aligned cataloging fields (Palier 1).** Titles carry six new bibliographic fields mapped to their UNIMARC zones: statement of responsibility (200$f), edition statement (205$a), collection title + number (225$a/$v), general note (300$a), original title (500$a). Captured automatically from the BnF at scan time, rendered on the title page when present, never overwriting manually edited values. The authoritative field⇄zone mapping ships as [`docs/unimarc-mapping.md`](docs/unimarc-mapping.md). A new Health-tab bulk action — **Backfill metadata from BnF** — re-runs the lookup for every coded title to fill the new fields on catalogs built before the upgrade (shares the pacing/back-off/lock of the cover refetch). Palier 2 — record import/export (ISO 2709 `.mrc` / UNIMARC XML) — is queued separately as [#436](https://github.com/guycorbaz/mybibli/issues/436).
- [#434](https://github.com/guycorbaz/mybibli/issues/434) — **Cataloging log summary.** Each metadata resolution logs one `info` summary line (cover resolved? how many UNIMARC fields filled?), making backfill runs auditable from the log file.

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
