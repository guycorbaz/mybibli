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

**Current stable: [`v1.7.5`](https://github.com/guycorbaz/mybibli/releases/tag/v1.7.5)** — the fifth patch on top of v1.7.0 ("Reach more users, debug more easily" — German + Italian UI translations and persistent log directory with rotation, shipped 2026-05-21). v1.7.5 bundles one user-visible fix (`/title/:id` contributor add/remove silently failing on missing `#feedback-list` slot — regression of v1.7.2) and seven issue-closure hardening commits. The originally-planned feature build for v1.1 through v1.6 is complete and v1.7 is currently the last themed minor on the roadmap; the project is in **GH-issue-driven polish mode** — no next themed minor is currently scoped. Patches ship independently for production bugs; new feature CRs are queued in the parking lot until a coherent theme emerges or a roadmap-slotted candidate (e.g., the v2.0 classification refactor) is greenlit.

Patch-line work currently open against the live line `1.7.x`:

- [#196](https://github.com/guycorbaz/mybibli/issues/196) — Flake in `home-search.spec.ts:224` under default-worker parallel mode (a long-standing known-failure; downgraded to fix-when-convenient).
- [#300](https://github.com/guycorbaz/mybibli/issues/300) — Volume-count mismatch — title shows multiple volumes when user owns only 1 (silent data corruption, pending triage).

## v1.7.0 — "Reach more users, debug more easily" *(shipped)*

Three CRs bundled together: two new UI languages and one operations
slice for production debuggability.

- [#275](https://github.com/guycorbaz/mybibli/issues/275) — **German (de) UI translation**. ~900 keys translated from `en.yml` to a new `de.yml`. Sie-form throughout (formal "you" — appropriate for library / small-association deployments).
- [#276](https://github.com/guycorbaz/mybibli/issues/276) — **Italian (it) UI translation**. ~900 keys translated from `en.yml` to a new `it.yml`. Tu-form informal throughout — appropriate for a home-library tool. Italian guillemets «…» used for inline quotes.
- [#301](https://github.com/guycorbaz/mybibli/issues/301) — **Persistent log directory + rotation + admin-controlled level**. Logs now persist across container restarts in a mounted volume; daily rotation with configurable retention; admin can flip log level (trace/debug/info/warn/error) from the System settings tab without a redeploy. New manual chapter 12 ("Operations & debugging") in EN + FR.

Foundation slice (not user-visible, prerequisite for the two UI translations):

- Extended `src/i18n/resolve.rs`, `src/middleware/locale.rs`, the admin → System → Default language selector, and the nav-bar language toggle to handle ≥3 languages. Locale parity test (`tests/locale_parity.rs`) ensures all four locale files stay key-aligned.

**Out of scope** (deferred — may land in a future minor): German and Italian translations of the user manual (`docs/manual/{de,it}/`). The UI is in four languages; the manual remains in EN + FR.

**v1.7.1 — "Finish 1.7.0 properly"** — closes the v1.7.0 shipped-incomplete log-level admin UI ([#308](https://github.com/guycorbaz/mybibli/issues/308) — release notes had promised it, manual chapter 12 had promised it, code didn't ship it) and bundles four production-surfaced bugfixes from the v1.7.0 NAS upgrade: [#310](https://github.com/guycorbaz/mybibli/issues/310) (provider-health probe timeout too tight for typical home-NAS DNS+TLS — new `MYBIBLI_PROVIDER_HEALTH_TIMEOUT_SECS` env var, default 10s), [#309](https://github.com/guycorbaz/mybibli/issues/309) (API-key permanent-delete 422 — HTMX `hx-delete` puts fields in query string, not body), [#311](https://github.com/guycorbaz/mybibli/issues/311) (bulk cover-refetch silently no-oped on cached titles — new `force_refresh` parameter), [#312](https://github.com/guycorbaz/mybibli/issues/312) (DB pool exhaustion during 64-title bulk-refetch — Semaphore-capped concurrency).

**v1.7.2 hotfix** — two workflow gaps surfaced after the v1.7.1 NAS upgrade: [#315](https://github.com/guycorbaz/mybibli/issues/315) (HTMX search swap now emits BOTH the list-mode `<table>` and grid-mode cards wrapper so both surfaces stay correct across mode toggle); [#316](https://github.com/guycorbaz/mybibli/issues/316) + [#318](https://github.com/guycorbaz/mybibli/issues/318) (`/title/:id` exposes proper Librarian+ Add / Remove affordances for the contributor block — backend routes existed since v1.0 but were only wired into the catalog scan flow).

**v1.7.3 hotfix** — one-fix patch for a latent regression that had shipped silently since v1.6.0: `/audit` returned 500 the moment a flagged volume existed ([#321](https://github.com/guycorbaz/mybibli/issues/321), `Option<u64>` vs `CAST AS SIGNED` type mismatch — textbook CLAUDE.md MariaDB gotcha #2). Empty-result branch never decoded a row and CI E2E coverage seeded zero flagged volumes, so the bug only fired the moment a librarian marked the first volume "À contrôler" on the v1.7.2 NAS. Same root cause also closed [#317](https://github.com/guycorbaz/mybibli/issues/317) — the "large black circle with cross" icon previously reported on `/audit` was the browser's default error glyph on the 500 page.

**v1.7.4 polish patch** — one user-visible fix and eight code-review-finding clean-ups from the issue queue: [#323](https://github.com/guycorbaz/mybibli/issues/323) (manual title 422 on empty numeric fields — third recurrence of the #238 / #296 gotcha pattern, now wired through `deserialize_optional_i32`); plus the clean-ups — [#97](https://github.com/guycorbaz/mybibli/issues/97) (setup-wizard recap fallback to EN for unknown locales), [#78](https://github.com/guycorbaz/mybibli/issues/78) (duplicate `version=N` in trash modal POST URL), [#77](https://github.com/guycorbaz/mybibli/issues/77) (auto-purge drain-cap counter — `tables_capped` instead of inflating `errors_count`), [#76](https://github.com/guycorbaz/mybibli/issues/76) (validate `entity_type` query filter in `permanent_delete_confirm`), [#95](https://github.com/guycorbaz/mybibli/issues/95) (consolidated `parse_setup_completed_at` helper across 3 sites), [#63](https://github.com/guycorbaz/mybibli/issues/63) (CI safety net: `ALLOWED_TABLES` ⊆ `PURGE_DELETION_ORDER`), [#72](https://github.com/guycorbaz/mybibli/issues/72) (drop unused `AdminAuditModel::list()`), [#147](https://github.com/guycorbaz/mybibli/issues/147) (`nav.js` burst-threshold 10ms floor), [#106](https://github.com/guycorbaz/mybibli/issues/106) (new CI grep gate against unscoped `a[href^=/title/]` selectors). Companion: [#320](https://github.com/guycorbaz/mybibli/issues/320) integration test locks the manually-edited Dewey-preserve-on-re-fetch contract.

**v1.7.5 polish patch** — one user-visible fix and seven hardening commits: [#325](https://github.com/guycorbaz/mybibli/issues/325) (`/title/:id` Add/Remove contributor silently failing with `htmx:targetError` — page was missing the `#feedback-list` slot the contributor form hardcodes; regression of #318 / v1.7.2; new `templates_audit` test pins the regression by panicking on any page that references the slot without declaring it); hardening — [#152](https://github.com/guycorbaz/mybibli/issues/152) (`/series` anonymous-readable contract locked via role-gate test), [#42](https://github.com/guycorbaz/mybibli/issues/42) (`forms_include_csrf_token` audit enforces strict first-child placement), [#220](https://github.com/guycorbaz/mybibli/issues/220) (`modal_confirm_retarget_guard` tolerates JSON-object HX-Trigger shape — defensive), [#31](https://github.com/guycorbaz/mybibli/issues/31) (`scanner-guard.js` honors `maxLength` and skips during IME composition), [#90](https://github.com/guycorbaz/mybibli/issues/90) (`admin_system::run_provider_update` consolidated onto `save_setting` — DRY), [#91](https://github.com/guycorbaz/mybibli/issues/91) (validation error re-renders form with user input + emits `HX-Trigger: validation-error`), [#48](https://github.com/guycorbaz/mybibli/issues/48) (new `rust_emitted_post_forms_include_csrf_token` audit walking `src/**/*.rs`).

## v1.6.0 — "Tighten the catalog" *(shipped)*

Four CRs aimed at catalog hygiene, plus the one-fix v1.6.1 hotfix that
unblocked the new organizational-location flow on root containers.

- [#280](https://github.com/guycorbaz/mybibli/issues/280) — Storage location can be marked **organizational** to refuse volume assignments (folders, not shelves). Checkbox on create/edit form, flip-with-volumes guard, volume-edit picker rejects organizational targets.
- [#237](https://github.com/guycorbaz/mybibli/issues/237) — **Shelf-audit workflow**: mark volumes "À contrôler" (single or bulk-per-shelf), amber home-dashboard indicator, dedicated `/audit` list sorted by location → V-code, audit-trail entries for every mark/clear. Surplus-detection deferred to a follow-up.
- [#279](https://github.com/guycorbaz/mybibli/issues/279) — **"Titles without volumes"** filter chip on the home dashboard (Librarian+ only, `NOT EXISTS` SQL guard).
- [#250](https://github.com/guycorbaz/mybibli/issues/250) — Home page's **list view becomes a sortable table** (Title · Author · Genre · Dewey · Volumes), replacing the cards-stacked-vertically layout. Grid view unchanged.

**v1.6.1 hotfix:**

- [#296](https://github.com/guycorbaz/mybibli/issues/296) — `Failed to deserialize form body: parent_id: cannot parse integer from empty string` when ticking "Emplacement organisationnel" on a root location. Latent bug since the location-edit feature shipped; #280 just gave users a new reason to edit root containers. New `deserialize_optional_u64` helper wired to `CreateLocationForm.parent_id`, `UpdateLocationForm.parent_id`, AND `VolumeEditForm.condition_state_id` (same shape, same latent bug).

## v1.5.0 — "Know what you have, and what it's worth" *(shipped)*

Five CRs around collection valuation + a new metadata provider, then
two same-day production-hygiene patches.

- [#243](https://github.com/guycorbaz/mybibli/issues/243) — Per-volume `purchase_price` + `current_value` + currency + `/stats/value` page (total, by-genre, by-series). CHF as the seeded default currency, admin-overridable.
- [#263](https://github.com/guycorbaz/mybibli/issues/263) — Library of Congress metadata provider (chain order: BDGest → BnF → Google Books → Library of Congress → Open Library).
- [#265](https://github.com/guycorbaz/mybibli/issues/265) — Donut chart for stats-by-genre on home (Chart.js v4 vendored, 5% "Other" bucket).
- [#271](https://github.com/guycorbaz/mybibli/issues/271) — Delete a title with zero volumes (UX-DR8 modal, volume-count guard).
- [#272](https://github.com/guycorbaz/mybibli/issues/272) — Edit ISBN/ISSN/UPC in the metadata-edit form, with ISBN-13 checksum + duplicate-collision guards.
- [#266](https://github.com/guycorbaz/mybibli/issues/266) — Real server-rendered PDF wish list export (`genpdf` + DejaVu Sans vendored, no browser print-to-PDF round-trip).

**v1.5.1 hygiene patch** — 5 production-feedback fixes: #281 (Dockerfile missing `COPY static/fonts/` → wish list PDF crash), #282 (Trash permanent-delete on titles failed because 4 child FKs weren't cascaded), #283 (Library valuation toggle missing from System admin tab), #284 (revoked API keys couldn't be hard-deleted), #285 (provider Health-tab probe marked everyone Unreachable — missing User-Agent + overly strict reachability rule).

**v1.5.2 hotfix** — #288 — `VolumeModel::find_by_id` and `find_by_label` crashed because SQLx 0.8 can't decode raw MariaDB `DECIMAL(10,2)` into `Option<f64>` without a feature flag. Fixed with `CAST(... AS DOUBLE)` (same pattern the aggregation queries already used).

## v1.4.0 — "Bring your own agent" *(shipped)*

JSON HTTP API at `/api/v1/*` protected by API keys, designed for an
external AI assistant (or any custom script) to read the catalog and
help with classification. Two coexisting access levels.

- [#241](https://github.com/guycorbaz/mybibli/issues/241) — HTTP API with API-key auth (argon2-hashed, 12-char prefix index, `Authorization: Bearer` or `X-API-Key`), read-only and read-write scopes, JSON endpoints for titles / contributors / volumes / locations / series, write allow-list `PATCH /api/v1/titles/{id}` over `subtitle`, `description`, `dewey_code`, `genre_id` (optimistic-locking via `version` in body, 409 on mismatch, full `admin_audit` row per change). Admin tab `/admin?tab=api_keys` to mint / list / revoke keys. New manual chapter 11 ("API & integrations", EN + FR) walks the LLM-classifier use case.

## v1.3.0 — "Plan your next purchase" *(shipped)*

Wish list as a first-class surface + per-volume polish.

- [#242](https://github.com/guycorbaz/mybibli/issues/242) — Wish list (ISBN / free-form / print to PDF / auto-link on catalog scan).
- [#209](https://github.com/guycorbaz/mybibli/issues/209) — Per-volume table on `/title/:id`.

**v1.3.1 polish patch** — 4 papercuts surfaced within hours of v1.3.0: #258 (spinner on wish list Rechercher button), #259 (Omnibus help-icon tooltip), #260 (wish list print page refactored to card layout, opens in new tab), #261 (home "Recent additions" folded by default with localStorage persistence).

## v1.2.0 — "See what you own, find it faster" *(shipped)*

UX polish round focused on visibility and quick wins. Six CRs grouped
into one coordinated release.

- [#236](https://github.com/guycorbaz/mybibli/issues/236) — Dewey code chip on the title-detail page, next to the genre.
- [#235](https://github.com/guycorbaz/mybibli/issues/235) — Sort series by Dewey or title (in addition to position); location detail had the same column since Epic 5.
- [#205](https://github.com/guycorbaz/mybibli/issues/205) — "Uncategorized" filter chip on home — surfaces titles with no real genre yet.
- [#200](https://github.com/guycorbaz/mybibli/issues/200) — Fold / unfold the `/locations` tree, with localStorage persistence.
- [#215](https://github.com/guycorbaz/mybibli/issues/215) — Spinner on "Apply selected changes" in the metadata re-fetch second phase.
- [#214](https://github.com/guycorbaz/mybibli/issues/214) — Bulk cover-fetch admin action — re-trigger the metadata-fetch chain for every title with a missing cover.

**v1.2.1 polish patch** — 4 fixes: #139 (series-delete shows meaningful "N titles assigned" instead of `error.internal`), #154 (help-icon button hoisted out of `<label>` for HTML5 + a11y), #133 (return-loan modal preserves trigger surface across login redirect), #219 (`AppError::Forbidden` now carries the request locale).

**v1.2.2 defense-in-depth patch** — 3 fixes around soft-deleted-genre semantics: #112 (stale `?filter=genre:N` drops the chip with localized notice), #107 (catalog SQL switches to LEFT JOIN + COALESCE so orphan-genre titles still appear), #111 (stats-by-genre denominator uses full active-catalog count).

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

## Past releases

The shipped history is documented release-by-release at
[github.com/guycorbaz/mybibli/releases](https://github.com/guycorbaz/mybibli/releases)
and in chapter 8 ("Upgrade & migration") of the user manual under
`docs/manual/{en,fr}/`. The website's [roadmap page](https://guycorbaz.github.io/mybibli/roadmap.html)
also tells the story in a more digestible form.
