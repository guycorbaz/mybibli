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

**Current stable: [`v1.7.11`](https://github.com/guycorbaz/mybibli/releases/tag/v1.7.11)** — the eleventh patch on top of v1.7.0 ("Reach more users, debug more easily" — German + Italian UI translations and persistent log directory with rotation, shipped 2026-05-21). v1.7.11 bundles one user-visible WCAG AA fix and three code-review-finding clean-ups: trash-count admin badge contrast bumped `bg-red-500` → `bg-red-700` (#345, plus a new `templates_audit` regression guard against the inaccessible `bg-red-500 + text-white` pair); companion contrast fix on the locations tree L-code + node_type spans (#352, surfaced during the v1.7.11 local Rule 13 gate as the next-highest axe-core violation after #345 cleared); CI/tooling hardening — `release.yml` now extracts the Cargo.toml version via Python's stdlib `tomllib` instead of a fragile `grep`, and `tests/e2e/tsconfig.json` adds `noUncheckedIndexedAccess` + `exactOptionalPropertyTypes` with the 12 surfaced narrow assertions backfilled (#30); TEST_MODE `POST /debug/seed-overdue-loan` endpoint + Playwright `seedOverdueLoan()` helper closes the overdue-loan E2E gap of #119, removing the silent `test.skip` in `home.spec.ts` (#340); and a slice of the long-deferred `base_context()` DRY refactor — new `BaseContextFields` struct + `base_context()` helper in `src/utils.rs` applied to the `loans` / `borrowers` / `series` list handlers as proof-of-pattern (#35 slice, follow-up issue tracks the remaining ~14 page handlers). The originally-planned feature build for v1.1 through v1.6 is complete and v1.7 is currently the last themed minor on the roadmap; the project is in **GH-issue-driven polish mode** — no next themed minor is currently scoped. Patches ship independently for production bugs; new feature CRs are queued in the parking lot until a coherent theme emerges or a roadmap-slotted candidate (e.g., the v2.0 classification refactor) is greenlit.

Patch-line work currently open against the live line `1.7.x`:

- [#300](https://github.com/guycorbaz/mybibli/issues/300) — Volume-count mismatch — title shows multiple volumes when user owns only 1 (silent data corruption, pending triage).
- [#333](https://github.com/guycorbaz/mybibli/issues/333) — Cover-refetch bulk action insuffisante : 73/147 prod titles without a cover. Piste 1 (Google Books `imageLinks` higher-res fallback) shipped in v1.7.9 but **prod-validated as ineffective** — the v1.7.9 NAS upgrade left the count unchanged. Audit on 2026-05-27 confirmed the gap is structural: the missing titles are dominated by FR/CH/DE tech publishers (Dunod, Eyrolles, Slatkine, Mardaga) for which neither Google Books (returns `totalItems=0`) nor Open Library Covers indexes the cover. Piste 2 (BDGest cover-only by ISBN) is API-less so requires scraping — declined per "APIs only, no scraping" preference. **Pivoting** to a v1.8 UX track that makes the manual upload path (shipped v1.7.6 #335) more visible and honest about the FR-publisher gap.
- [#340](https://github.com/guycorbaz/mybibli/issues/340) — Backfill overdue-loan seeding for home E2E (follow-up to #119: TEST_MODE endpoint vs DB port exposure).
- [#345](https://github.com/guycorbaz/mybibli/issues/345) — Trash count badge fails WCAG AA contrast (3.8 vs 4.5) when badge renders — pre-existing, surfaces under test-order data pollution in the full E2E suite. `bg-red-500` → `bg-red-700` (admin-tabs badge) plus accessibility-full beforeEach isolation.

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

**v1.7.6 — "cover saga v2 + BD edit"** — two user-visible feature fixes and three hardening commits. [#331](https://github.com/guycorbaz/mybibli/issues/331) (editable `media_type` on `/title/:id` — a BD scanned via ISBN that BnF resolved as a "book" can finally be reclassified through a dropdown on the metadata edit form; pre-fix the field rendered as a read-only `<p>` and the form struct had no `media_type` field at all; `admin_audit` records `title_media_type_edit` with before/after); [#335](https://github.com/guycorbaz/mybibli/issues/335) (manual cover upload — `POST /title/:id/cover` multipart, Librarian+ drops a JPG/PNG/WebP/GIF up to 10 MiB; goes through the EXACT same decode/resize/JPEG-encode pipeline as provider-downloaded covers via a new factored `CoverService::process_and_save_bytes`; `cover_image_url` marked `manually_edited` so bulk-refetch won't clobber it; safety net for titles where no provider has a cover — diagnosed in [#333](https://github.com/guycorbaz/mybibli/issues/333) at 73/146 prod miss rate on the FR catalog). Hardening — [#109](https://github.com/guycorbaz/mybibli/issues/109) (`fake_search_result_full` test factory closes the `Some(d)/Some(c)` home recent-additions card render branches), [#40](https://github.com/guycorbaz/mybibli/issues/40) (CSRF exempt-route comparison tolerates path variants Axum routes identically — `POST /login/` and `POST //login` now bypass as intended; percent-encoded forms intentionally NOT decoded — `/login%2F..%2Fadmin` bypass attempt stays rejected), [#36](https://github.com/guycorbaz/mybibli/issues/36) (`session_resolve_middleware` skips `/static`, `/covers`, `/logo`, `/health` prefixes — every home-page load previously fanned out dozens of session-row reads on the asset fetches). Infra change: Axum `multipart` feature flag enabled in `Cargo.toml` + CSRF middleware short-circuits body buffer for `multipart/form-data` when the `X-CSRF-Token` header is present (otherwise the 1 MiB body cap would 413 every cover upload).

**v1.7.7 — "test rigor + series cards"** — one user-visible enhancement and seven code-review-finding clean-ups bundled as a test-hardening patch. [#336](https://github.com/guycorbaz/mybibli/issues/336) (series detail card layout on `/series/:id` — cover thumbnail + "Vol. N" prefix + title + primary contributor for every filled position, with dashed-border gap cards carrying the same "Vol. N" so collectors see at a glance which slots are missing; responsive 2-col mobile → 6-col xl; preserves the existing ARIA `role="grid"`/`role="gridcell"` contract so the 3 pre-existing E2E tests on series.spec.ts keep passing). Test rigor sweep — [#43](https://github.com/guycorbaz/mybibli/issues/43) (DB-snapshot helper + assertion on the anonymous-contributor-POST spec — `captureEntityCounts(baseURL)` proves the 403/303 rejection didn't mutate the DB), [#62](https://github.com/guycorbaz/mybibli/issues/62) (rewrite of 8-7 P9 spec — real seeded items + real assertions; drops the silent `if (tableExists)` no-op pattern), [#88](https://github.com/guycorbaz/mybibli/issues/88) (cross-row concurrency E2E for admin system settings — two `browser.newContext()` admin sessions race on different settings rows, asserting no 409 — AC #6 of story 8-5), [#89](https://github.com/guycorbaz/mybibli/issues/89) (backfill 2 missing locale-resolution branches — Accept-Language wins over default, authenticated user pref wins over default — AC #4 of story 8-5; serializes the spec describe to prevent shared-settings collisions), [#96](https://github.com/guycorbaz/mybibli/issues/96) (Rust integration test for AC13 concurrent first-launch race — two `tokio::spawn` Step 1 POSTs with same username assert exactly one admin row), [#119](https://github.com/guycorbaz/mybibli/issues/119) (dashboard indicator E2E no longer a silent no-op on clean DB — unshelved indicator seeded via `scanTitleAndVolume`; overdue gap explicitly tracked as follow-up [#340](https://github.com/guycorbaz/mybibli/issues/340) — TEST_MODE seed endpoint or DB port exposure required), [#120](https://github.com/guycorbaz/mybibli/issues/120) (template-layer lock for AC5 single-active-filter contract — two render unit tests assert `#browse-results` carries no `.browse-table` / `.title-card` content when an indicator filter is active).

**v1.7.8 — "concurrent-delete UX + hardening trio"** — one user-visible UX fix and three code-review-finding hardenings. [#136](https://github.com/guycorbaz/mybibli/issues/136) (concurrent-delete modals on `/borrower/:id`, `/contributor/:id`, and the return-loan modal — when two librarians had the same entity page open and one deleted the row, the other's click silently no-op'd because the 404 retargeted to `#feedback-list` which these pages don't declare; fix: handlers return 200 + inline feedback + HX-Retarget to the page's existing slot — `#borrower-feedback` / `#contributor-feedback` / `?target=` resolved slot for the return-loan flow — so the user sees a localized "already deleted by another session" toast in all 4 supported locales; new shared helper `routes::build_already_deleted_response`). Hardening — [#39](https://github.com/guycorbaz/mybibli/issues/39) (`services::auth::authenticate_session` wraps the INSERT new session + UPDATE prior anonymous session in a single sqlx transaction so a partial failure cannot leave an orphaned anonymous-session row alongside the new authenticated one; new `tests/auth_service.rs` integration tests lock the atomic-swap contract), [#47](https://github.com/guycorbaz/mybibli/issues/47) (4 high-value CSRF coverage gaps backfilled: HTMX-branch language POST, form-field-token logout, stale-token replay after rotation, 403 body asserts FeedbackEntry markup; the remaining 5 E2E items deferred), [#217](https://github.com/guycorbaz/mybibli/issues/217) (modal.js `originatesFromConfirm` was tagging X-Modal-Confirm on ANY form descendant of the open dialog — fix tightens to match only `dialog.querySelector("form")` so a future modal with a nested form won't accidentally trip the server-side `ModalConfirmRetargetGuard`; E2E regression-guard in admin-modal-lifecycle.spec.ts injects a secondary form and asserts the header is NOT sent).

**v1.7.11 — "trash badge contrast + 3 CR-findings + companion contrast fix"** — one user-visible WCAG AA fix (#345 — admin trash count badge `bg-red-500` (#fb2c36) on `text-white` was 3.8:1, below the AA 4.5:1 threshold; switched to `bg-red-700` (#b91c1c, 6.61:1) and added a new `templates_audit::templates_forbid_bg_red_500_with_text_white` regression guard that panics on any future template pairing the two — combo is intrinsically inaccessible so a static audit is the right level), plus a companion contrast fix (#352 — locations tree L-code + node_type spans + the V-code span on location detail used naked `text-stone-400` on `bg-stone-50` light mode = 2.41:1; switched to `text-stone-600 dark:text-stone-400` for 7.5:1 light + 8:1 dark; surfaced during the v1.7.11 local Rule 13 gate as the next-highest axe-core violation after #345 cleared — `#345`'s issue body misattributed the failure URL but the trash-badge fix was correct and load-bearing on its own), and three code-review-finding clean-ups. #30 (CI/tooling — `release.yml` migrates `grep`-based Cargo.toml version extraction to `python3 -c "import tomllib; …"` so it stays robust against UTF-8 BOM / CRLF / single-quoted values / future `[workspace.package]` sections; `tests/e2e/tsconfig.json` enables `noUncheckedIndexedAccess` + `exactOptionalPropertyTypes` and the 12 surfaced narrow assertions are backfilled via non-null `!` on regex-match indexing where the surrounding `expect(match).toBeTruthy()` already proves the value exists; sub-items 2 + 3 — tag-mismatch smoke test and seed-migration drift audit — deferred to their own focused issues). #340 (TEST_MODE `POST /debug/seed-overdue-loan` endpoint mirrors the existing `/debug/session-timeout` pattern: TEST_MODE-gated + Admin role check as defense-in-depth, accepts `{volume_label, borrower_name, days_overdue}` and resolves labels to ids server-side, direct `INSERT INTO loans … VALUES (?, ?, NOW() - INTERVAL ? DAY)` bypassing `LoanService::register_loan` because the whole point is to backdate; new `seedOverdueLoan()` Playwright helper opens a private `APIRequestContext`, logs in as admin, grabs the post-login CSRF off the meta tag, POSTs the form; `home.spec.ts` overdue branch removes the silent `test.skip` and now seeds + asserts end-to-end). #35 slice (new `BaseContextFields` struct in `src/utils.rs` carrying the 20 fields every full-page template struct repeats — `lang`, `role`, `current_page`, `skip_label`, `connection_status`, `shortcuts_cheat_sheet`, `session_timeout_secs`, `csrf_token`, 10 `nav_*` strings, `current_url`, `lang_toggle_aria` — built by `base_context(&session, locale, current_page, &uri, session_timeout_secs)`; applied to `loans_page` / `borrowers_page` / `series_index` as proof-of-pattern; helper takes `session_timeout_secs` as a scalar rather than `&AppState` so it stays unit-testable without a live DbPool, with 4 unit tests locking the field-population contract + the EN/FR locale routing; remaining ~14 page-template handlers + the two deferred sub-items — `generate_session_token` deduplication and `Session::anonymous_with_token` fallback removal — tracked in a follow-up issue post-merge for incremental migration).

**v1.7.10 — "protect manual covers"** — one-fix patch closing a silent-data-loss vulnerability in the metadata-fetch chain. [#347](https://github.com/guycorbaz/mybibli/issues/347) (`update_cover_image_url` did an unconditional UPDATE that ignored `manually_edited_fields`, so a manually uploaded cover — the v1.7.6 #335 safety net — could be silently overwritten by the async background fetch after initial scan or by the bulk-refetch admin button in a race with a fresh manual upload; failure path was equally vulnerable, blanking the manual cover to NULL when the provider download failed). Fix: guard pattern identical to `do_update`'s — a fresh snapshot read inside `update_cover_image_url` checks `parsed_manually_edited_fields()` and skips the UPDATE if `cover_image_url` is in the set. Fresh snapshot rather than one passed down from `update_title_from_metadata` because the cover-download phase runs after `do_update`, leaving a race window where a manual upload would be missed by an upstream snapshot. 4 integration tests in `tests/cover_manual_survives_refetch.rs` cover the success / failure / regression / specificity paths; no template, CSS, locale, or E2E touch (per-title `Re-fetch metadata` button is already protected upstream by the conflict-resolution branch in `redownload_metadata`).

**v1.7.9 — "cover quality + admin timeouts + 4 hardenings"** — one user-visible cover-quality fix, one admin-visible config surface, and four code-review-finding hardenings. [#333 piste 1](https://github.com/guycorbaz/mybibli/issues/333) (Google Books cover URL — `parse_response` now picks the highest-resolution `imageLinks` variant `extraLarge → large → medium → small → thumbnail → smallThumbnail` so the downstream 400 px Lanczos resize in `CoverService::process_and_save_bytes` works from the best source pixels; existing titles get a sharper cover after re-fetch, and titles where Google Books only ships `medium`+ but no `thumbnail` finally attach a cover — closes the (1) audit piste of the 50 % prod miss-rate investigation, piste 3 manual upload already shipped in v1.7.6 #335, piste 2 BDGest cover-only fetch still open). [#334](https://github.com/guycorbaz/mybibli/issues/334) (admin UI for the per-provider metadata-chain timeout + provider-health probe timeout, surfaced inside `/admin > System` Metadata Providers block; pattern v1.7.1 #308 — both settings persist in the K/V `settings` table, bounded 1..=60 s, hot-reload through `Arc<RwLock<AppSettings>>` so the next chain run / next ping round picks up the new value with no restart; env vars `MYBIBLI_METADATA_CHAIN_PROVIDER_TIMEOUT_SECS` + `MYBIBLI_PROVIDER_HEALTH_TIMEOUT_SECS` honored as one-shot bootstrap via `config::migrate_legacy_env_vars`). Hardenings — [#46](https://github.com/guycorbaz/mybibli/issues/46) (anonymous-session purge persists `last_anonymous_session_purge_at`; the 24 h cadence now survives restart loops within a 24 h window — boot computes `24h − elapsed` instead of always re-arming a fresh 24 h sleep that resets on every restart), [#28](https://github.com/guycorbaz/mybibli/issues/28) (observability trio — `register_loan` retry trail now distinguishes "non-transient error after a prior transient retry" so a concurrent soft-delete that flips the second attempt into a BadRequest no longer reads as "the retry caused the error"; new `tasks::metadata_fetch::spawn_fetch` wrapper captures panics from the 4 catalog fire-and-forget spawn sites instead of dropping the JoinHandle silently; CLAUDE.md retry note now explicit that `sqlx::Error::PoolTimedOut` is intentionally NOT retried), [#24](https://github.com/guycorbaz/mybibli/issues/24) (`templates_audit` 5 edge cases — `strip_html_comments` rewritten as a state machine that bails on unterminated `<!--` instead of consuming the rest of the file + slices `&str` so UTF-8 in failure-report snippets stays readable; `strip_svg_inner_blocks` masks SVG inner content so an inline `<svg><style>...</style></svg>` doesn't false-positive the bare-`<style>` check; quote-aware grammar `(?:[^>"']|"[^"]*"|'[^']*')*` for the `<script>` and `<style>` attribute regex so literal `>` inside a quoted attribute can't terminate the match early), [#196](https://github.com/guycorbaz/mybibli/issues/196) (6-retro-old typing-race flake in `home-search.spec.ts:224` closed — `simulateTyping` switched from `locator.pressSequentially` to `keyboard.type` after an explicit `focus()` to eliminate the per-key re-focus that dropped a keystroke under default-worker parallelism; URL-state poll bumped 2 s → 5 s for headroom under load).

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
