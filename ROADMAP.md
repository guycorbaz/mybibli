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

**Current stable: [`v1.2.0`](https://github.com/guycorbaz/mybibli/releases/tag/v1.2.0)** — published 2026-05-18. The first minor release of the post-v1 era, themed "See what you own, find it faster" (full list of CRs below). Concurrently, the `1.1.x` patch line stays in maintenance for bug fixes and known-failure resolutions only.

Patch-line work currently in flight or planned (will ship as `1.1.10+`, NOT in `1.2.x`):

- [#219](https://github.com/guycorbaz/mybibli/issues/219) — `AppError::Forbidden` rendered in the default locale instead of the user's locale.
- [#196](https://github.com/guycorbaz/mybibli/issues/196) — Flake in `home-search.spec.ts:224` under default-worker parallel mode (a long-standing known-failure; downgraded to fix-when-convenient).

## v1.2.0 — "See what you own, find it faster" *(shipped)*

UX polish round focused on visibility and quick wins. Six CRs grouped
into one coordinated release.

- [#236](https://github.com/guycorbaz/mybibli/issues/236) — Dewey code chip on the title-detail page, next to the genre.
- [#235](https://github.com/guycorbaz/mybibli/issues/235) — Sort series by Dewey or title (in addition to position); location detail had the same column since Epic 5.
- [#205](https://github.com/guycorbaz/mybibli/issues/205) — "Uncategorized" filter chip on home — surfaces titles with no real genre yet.
- [#200](https://github.com/guycorbaz/mybibli/issues/200) — Fold / unfold the `/locations` tree, with localStorage persistence.
- [#215](https://github.com/guycorbaz/mybibli/issues/215) — Spinner on "Apply selected changes" in the metadata re-fetch second phase.
- [#214](https://github.com/guycorbaz/mybibli/issues/214) — Bulk cover-fetch admin action — re-trigger the metadata-fetch chain for every title with a missing cover.

## v1.3.0 — "Plan your next purchase"

Wish list as a first-class surface, with a bookstore-friendly print
flow. Pairs with the per-volume detail polish.

- [#242](https://github.com/guycorbaz/mybibli/issues/242) — Wish list: add books by ISBN or free-form title, browse on mobile, print to PDF for the bookstore, **auto-remove** entries from the wish list when the same ISBN gets cataloged.
- [#209](https://github.com/guycorbaz/mybibli/issues/209) — Per-volume table on `/title/:id` (between contributors and similar-titles), with inline edit and delete actions — useful when a redundant copy is given away.

## v1.4.0 — "Open the catalog to your AI"

A new surface: a JSON HTTP API protected by API keys, designed for an
external AI assistant (or any custom script) to read the catalog and
help with classification. Two coexisting access levels.

- [#241](https://github.com/guycorbaz/mybibli/issues/241) — HTTP API with API-key auth, read-only and read-write scopes, JSON endpoints for titles / contributors / volumes / locations / series, write allow-list focused on classification fields (`genre_id`, `dewey_code`, description, subtitle).

## v1.5.0 — "Know what your collection is worth"

Per-volume purchase price and current value, with aggregations by
genre and by series — useful for collectors of BD or signed editions,
and for insurance / inheritance paperwork.

- [#243](https://github.com/guycorbaz/mybibli/issues/243) — Per-volume `purchase_price` + `current_value` + currency + `/stats/value` page (total, by-genre, by-series).

## v1.6.0 — "Audit your shelves"

A library-audit workflow built around a persistent "À contrôler" flag
on each volume. Mark a shelf or a search-result set, walk the rows,
clear the flag as you verify.

- [#237](https://github.com/guycorbaz/mybibli/issues/237) — Audit flag with bulk-by-location, bulk-by-search-result, per-volume marking; dedicated filter view; manual-only flag clearing; surplus-detection deferred to a follow-up.

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
- [#154](https://github.com/guycorbaz/mybibli/issues/154) — Help-icon button nested inside `<label>` creates HTML validity issues.
- [#152](https://github.com/guycorbaz/mybibli/issues/152) — Add `anonymous_gets_200_on_series` test to `role_gating.rs`.
- [#151](https://github.com/guycorbaz/mybibli/issues/151) — Align UX spec + 9.18 epic spec nav-link tables with shipped implementation.
- [#148](https://github.com/guycorbaz/mybibli/issues/148) — `nav.js` burst detector — Bluetooth Android scanners emit `Unidentified`.
- [#147](https://github.com/guycorbaz/mybibli/issues/147) — `nav.js` `getBurstThresholdMs` accepts `n=1` (1ms) — pathological admin-config protection.
- [#146](https://github.com/guycorbaz/mybibli/issues/146) — E2E `navbar-hamburger` Test 5 — `simulateScan` trailing Enter race on `/login`.
- [#145](https://github.com/guycorbaz/mybibli/issues/145) — `nav.js` burst threshold (50ms) may be tight for very fast typists / mechanical keyboards.
- [#139](https://github.com/guycorbaz/mybibli/issues/139) — `series::delete_series` renders generic `error.internal` on Conflict instead of `series.delete_has_titles`.

### Larger CRs awaiting prioritization

- [#202](https://github.com/guycorbaz/mybibli/issues/202) — Broader per-provider visibility (metadata-source badge, per-provider retry, structured error surfacing). The docs slice already shipped in v1.5; the UX slice is deeper work.

## Past releases

The shipped history is documented release-by-release at
[github.com/guycorbaz/mybibli/releases](https://github.com/guycorbaz/mybibli/releases)
and in chapter 8 ("Upgrade & migration") of the user manual under
`docs/manual/{en,fr}/`. The website's [roadmap page](https://guycorbaz.github.io/mybibli/roadmap.html)
also tells the story in a more digestible form.
