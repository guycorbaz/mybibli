<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/mybibli-logo/svg/mybibli-logo-dark.svg">
    <img src="docs/mybibli-logo/svg/mybibli-logo.svg" alt="mybibli" width="440">
  </picture>
</p>

![CI](https://github.com/guycorbaz/mybibli/actions/workflows/ci.yml/badge.svg?branch=main)

> Personal library cataloging for home collectors.

> ## 🛑 DO NOT INSTALL VERSIONS BELOW 1.1.0
>
> Every published image with a tag below `1.1.0` (including `v1.0.0` …
> `v1.0.5` and any `:latest` snapshot taken before the 1.1.0 release)
> ships seed migrations that create default credentials
> (`admin/admin`, `librarian/librarian`) on EVERY fresh install,
> including production deployments. The first-launch setup wizard at
> `/setup` is silently bypassed because the seed creates an admin
> before the gate predicate is evaluated. See
> [#173](https://github.com/guycorbaz/mybibli/issues/173) for the full
> writeup. The pre-1.1.0 Docker Hub tags will be removed to prevent
> accidental installs.
>
> **Install 1.1.0 or later.** A fresh install at 1.1.0+ greets you with
> the setup wizard and forces you to create your own admin account. If
> you already deployed an earlier version, wipe the database and
> reinstall on 1.1.0+ before adding any data.

**Status:** first public release shipped — `v1.0.0`. All nine epics done. Pre-built images on Docker Hub at [`gcorbaz/mybibli`](https://hub.docker.com/r/gcorbaz/mybibli). See [ROADMAP.md](ROADMAP.md) for what's coming next.

## What it is

`mybibli` is a self-hosted web app to catalog, locate, and loan your personal library. It is designed for a single household, running on your own hardware (typically a NAS or home server). No cloud sync, no telemetry — all data stays on your local network.

Built for collectors who want more than a spreadsheet:

- **Barcode-first cataloging.** Scan an ISBN / EAN-13 and the title resolves asynchronously through a metadata provider chain (BnF, Google Books, Open Library, MusicBrainz, OMDb, TMDB, BDGest), with cover-image download and similar-title detection.
- **Multi-media support.** Books, BD/comics (with multi-position omnibus volumes), audio releases, films/series — each typed correctly and with the right metadata provider chosen automatically.
- **Series + collection awareness.** Gap detection on series volumes, Dewey-based browsing, similar-titles section.
- **Storage-location tracking.** Configurable hierarchy (room → shelf → row → …), barcode-on-shelf workflow.
- **Loan management.** Borrower CRUD, loan registration with automatic location restoration on return, overdue threshold (admin-configurable), per-borrower history.
- **Multi-role auth.** Anonymous (read-only), Librarian (catalog + loans), Admin (everything). Session inactivity timeout with keep-alive toast. FR/EN language toggle with per-user preference.
- **Hardened by construction.** Strict Content Security Policy (no `unsafe-inline`/`unsafe-eval`), CSRF synchronizer-token middleware on every state-changing request (with a server-rendered "session expired" feedback when the token drifts — see [`docs/auth-threat-model.md`](docs/auth-threat-model.md)), scanner-guard against burst-keyboard input leaking into modals.
- **Admin panel.** Health dashboard (entity counts, MariaDB version, disk usage, provider reachability), user management with last-active-admin guard, editable reference data (genres, volume states, contributor roles, location node types), system settings (overdue threshold, provider API keys, default language), trash view + restore + permanent delete, configurable auto-purge after 30 days.
- **First-launch setup wizard.** Fresh installs walk through Admin → Providers → Preferences → Done; the gate middleware redirects every route to `/setup` until completion. Idempotent — interruptions resume at the right step server-side.
- **Mobile-aware + WCAG 2.2 AA accessible.** Dual-surface mobile UX on data-dense pages (desktop tables collapse into mobile cards, admin tabs collapse into a `<select>` dropdown), full keyboard navigation with shortcuts cheat-sheet (`?`), contextual help-icon tooltips, and an axe-core CI gate that covers every reachable surface including entity-detail routes and the first-launch wizard.

## Screenshots

Live production install (`v1.6.1`, household NAS, ~88 titles cataloged):

<p align="center">
  <img src="docs/screenshots/home.png" alt="mybibli home page — search bar, genre filters, dashboard counters, and a recent-additions strip with cover thumbnails." width="780">
  <br>
  <em>Home — search, genre filters, dashboard counters ("À traiter" / "Aperçu de la collection"), recent additions with cover thumbnails.</em>
</p>

<p align="center">
  <img src="docs/screenshots/locations.png" alt="mybibli locations page — hierarchical tree of rooms, bookcases and shelves with per-node volume counts and edit/delete affordances." width="780">
  <br>
  <em>Locations — configurable hierarchy (room → bookcase → shelf …), per-node volume counts, inline create / edit / delete.</em>
</p>

## Tech stack

- **Backend:** Rust 2024 edition + [Axum](https://github.com/tokio-rs/axum) 0.8
- **Database:** MariaDB via [SQLx](https://github.com/launchbadge/sqlx) 0.8 (offline query cache committed in `.sqlx/`)
- **Templates:** [Askama](https://github.com/djc/askama) 0.15 (compile-time type-checked)
- **Frontend:** [HTMX](https://htmx.org/) 2.0 + [Tailwind CSS](https://tailwindcss.com/) v4 — no SPA framework, zero inline scripts/styles (CSP `script-src 'self'`, `style-src 'self'`)
- **i18n:** [rust-i18n](https://github.com/longbridgeapp/rust-i18n) — French + English
- **Auth:** session cookie (`HttpOnly`, `SameSite=Lax`) + per-session CSRF synchronizer token; argon2 password hashing
- **Testing:** `cargo test` (~525 lib unit), `#[sqlx::test]` (~95 DB integration tests across 10+ files), [Playwright](https://playwright.dev/) (~160 E2E specs across two CI lanes — the seeded-stack suite and a dedicated wizard-E2E lane that runs on a fresh empty database)

## Quick start (end users)

Pre-built images are published to Docker Hub at [`gcorbaz/mybibli`](https://hub.docker.com/r/gcorbaz/mybibli) — `:latest` tracks the highest semver, individual tags pin to the exact release. Honor the **1.1.0 minimum** banner above. For development against the source tree, see **Development** below.

### Persistent storage — both volumes are mandatory

`docker-compose.yml` declares **two named volumes** that MUST survive container upgrades:

- `mybibli_db_data` → `/var/lib/mysql` — your catalog (titles, volumes, loans, etc.)
- `mybibli_covers` → `/app/covers` — downloaded cover JPGs (issue [#213](https://github.com/guycorbaz/mybibli/issues/213))

If you deploy `docker-compose.yml` from the repo unchanged, you already have both. Back them up together — losing one without the other leaves DB references pointing at missing files (or vice versa).

**Upgrading from a pre-1.1.4 install?** Pre-1.1.4 `docker-compose.yml` did not declare `mybibli_covers`, so the cover JPGs lived inside the container's writable layer and were lost on every `docker compose up -d` after a `pull`. Adding the volume now preserves covers fetched from this point forward, but does NOT restore the ones that disappeared on prior upgrades. To recover, re-trigger metadata fetch from each affected title's detail page (the "Re-fetch metadata" button). A bulk-fetch admin action is tracked at issue [#214](https://github.com/guycorbaz/mybibli/issues/214).

**Synology DSM / bind-mount users:** comment out the `mybibli_covers:/app/covers` line in `docker-compose.yml` and uncomment the bind-mount line right below it, then set `COVERS_HOST_PATH` in your `.env` to the host path you want — Synology File Station / your rsync routine will see the covers directly.

## Configuration

All deployment-time settings are environment variables — there is no
config file. `.env.example` is the canonical reference: every variable
the Rust binary reads or that `docker-compose.yml` interpolates is
listed and commented there. Copy it to `.env` and adjust for your
deployment:

```bash
cp .env.example .env
$EDITOR .env
docker compose up
```

The variables are grouped in seven sections:

1. **Database connection** — `DATABASE_URL` plus the `MYSQL_*` parts
   used by the bundled `db` service.
2. **HTTP server** — `HOST`, `PORT`, `HOST_PORT` (the host-side port
   published by Docker).
3. **Application** — `RUST_LOG` (tracing filter; prod-safe default
   `mybibli=info`), `APP_LANGUAGE` (`en` or `fr`), `COVERS_DIR`
   (filesystem path for downloaded cover images — in Docker, the
   `/app/covers` directory is mapped to the persistent `mybibli_covers`
   named volume; see "Persistent storage" above. `COVERS_HOST_PATH`
   is an optional bind-mount override for users who prefer a host path
   visible in their file manager).
4. **Cookie & CSP hardening** — `MYBIBLI_COOKIE_SECURE` (set to `true`
   only behind HTTPS, see issue
   [#94](https://github.com/guycorbaz/mybibli/issues/94)),
   `CSP_REPORT_ONLY`.
5. **Metadata provider API keys** — `GOOGLE_BOOKS_API_KEY`,
   `OMDB_API_KEY`, `TMDB_API_KEY`. Migrated ONCE into the `settings`
   table at boot; afterwards the admin can rotate or clear them via
   `/admin > System > Metadata Providers`. Re-set them in `.env` only
   when you want the deployment-time value to win on the next reboot.
6. **Metadata provider base URL overrides** — used exclusively by the
   E2E test stack to point each provider at the in-tree mock server.
   Leave unset in production.
7. **Optional dev/test overrides** — `MYBIBLI_SKIP_SETUP`,
   `MYBIBLI_SKIP_STARTUP_PURGE`. Strict accept-set: only `1` / `true`
   / `TRUE` count as "on"; anything else is ignored.

Boolean variables across the codebase use the same strict accept-set,
which avoids the classic footgun where a stale shell value like `0`
reads as "set" and silently flips an opt-out.

Shell-level env vars used for build / test commands (`SQLX_OFFLINE`,
`TEST_ADMIN_PASSWORD`, `MYBIBLI_SETUP_E2E`, …) are documented inline
in the **Development** section below — they do not belong in `.env`.

## Development

### Prerequisites

- Docker + Docker Compose
- Rust toolchain (rustup, Rust 2024 edition)
- Node.js 20+ (for Playwright E2E tests)

### Run the app locally

```bash
# Start the full stack (app + MariaDB + mock metadata providers).
# `MYBIBLI_SKIP_SETUP=1` is baked into the test compose so existing
# seeded specs reach their target routes without going through the
# first-launch wizard.
cd tests/e2e
docker compose -f docker-compose.test.yml up --build
```

The app listens on `http://localhost:8080`. The seed migrations create an admin user (`admin` / `admin`, role `admin`) and a librarian (`librarian` / `librarian`, role `librarian`) **only when `MYBIBLI_SEED_DEV_USERS=1` is set** — which is baked into both `docker-compose.dev.yml` and `tests/e2e/docker-compose.test.yml`.

> ℹ️ **Seed users are now gated (issue #173, fixed in 1.1.0).**
> On a fresh install where `MYBIBLI_SEED_DEV_USERS` is unset, the
> seed migrations still apply but the gate in `src/services/seed_gate.rs`
> immediately soft-deletes any user whose hash still matches the
> documented seed value. The first-launch wizard at `/setup` is
> therefore reachable on every fresh production deployment. Set the
> env var to `1` only for local development and the E2E test stack —
> never in production.

**Fresh-install wizard.** Story 8-8 introduced a first-launch wizard at `/setup` whose gate predicate is `(active_admin_count == 0) AND (settings.setup_completed_at IS NONE)`. Because the seed migrations create an admin before the gate is first evaluated, the wizard never triggers in practice on a default install — the password-rotation step above is the effective onboarding flow. The wizard can still be exercised by running `cargo run` against an empty DB with the seed migrations skipped. `MYBIBLI_SKIP_SETUP=1` (strict accept-set: `1` / `true` / `TRUE`) is the explicit bypass.

### Build & check (native)

```bash
cargo check                          # Fast type-check
cargo build                          # Full debug build
cargo clippy -- -D warnings          # Lint (zero-warnings policy)
```

### Unit tests

```bash
SQLX_OFFLINE=true cargo test --lib   # ~525 unit tests, ~5 s
cargo test config::                  # Module-scoped
cargo test <name> -- --nocapture     # Single test with output
```

### DB integration tests

```bash
docker compose -f tests/docker-compose.rust-test.yml up -d
SQLX_OFFLINE=true \
DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
cargo test --test find_similar \
           --test find_by_location_dewey \
           --test metadata_fetch_dewey \
           --test metadata_fetch_race \
           --test seeded_users \
           --test setup_wizard
```

Each test gets a fresh DB via `#[sqlx::test(migrations = "./migrations")]`. The CI `db-integration` job runs the same allowlist — when adding a new `tests/*.rs` file, append `--test <name>` to both this command and `.github/workflows/_gates.yml::db-integration`.

### E2E tests

The Playwright suite has **two CI lanes**:

```bash
cd tests/e2e

# Lane 1 — seeded-stack (most specs). MYBIBLI_SKIP_SETUP=1 baked in.
docker compose -f docker-compose.test.yml up --build -d
npm test                             # Full suite, parallel mode

# Lane 2 — wizard E2E (story 8-8). Fresh DB, MYBIBLI_SKIP_SETUP unset.
docker compose -f docker-compose.test.yml -f docker-compose.wizard.yml up -d --build --wait
docker compose -f docker-compose.test.yml -f docker-compose.wizard.yml exec -T db \
    mariadb -uroot -proot_test mybibli_test -e "DELETE FROM sessions; DELETE FROM users;"
docker compose -f docker-compose.test.yml -f docker-compose.wizard.yml restart mybibli
MYBIBLI_SETUP_E2E=1 npx playwright test specs/journeys/setup-wizard.spec.ts

# Single spec from the seeded suite
npx playwright test specs/journeys/<spec>.spec.ts
```

A `waitForTimeout(...)` grep gate (`tests/e2e` only) blocks any new arbitrary-sleep call — use DOM-state assertions instead. Enforced both locally and in the CI `e2e` job.

### Stack reset

`./scripts/e2e-reset.sh` does a single-command teardown + rebuild + wait-for-ready when local DB state is polluted. Use after long-running dev sessions where E2E specs see stale rows from prior runs.

### Database migrations

Migrations live in `migrations/`. SQLx offline cache in `.sqlx/` is checked into the repo and must stay in sync:

```bash
cargo sqlx prepare                   # Regenerate after query changes
cargo sqlx prepare --check --workspace -- --all-targets
```

### i18n

Locale files in `locales/{en,fr}.yml`. After adding or renaming keys:

```bash
touch src/lib.rs && cargo build      # Force proc-macro rebuild (rust-i18n)
```

## Repository layout

```
src/
├── routes/          # HTTP handlers — thin, delegate to services
│   ├── admin.rs            # Admin shell + tab routing + user management (8-1, 8-3)
│   ├── admin_reference_data.rs  # Genres / states / roles / node types CRUD (8-4)
│   ├── admin_system.rs     # System settings forms (8-5)
│   ├── auth.rs             # Login / logout
│   ├── catalog.rs          # Cataloging routes
│   ├── locations.rs        # Storage location tree
│   ├── loans.rs            # Loans + borrowers
│   ├── setup.rs            # First-launch setup wizard (8-8)
│   └── …
├── services/        # Business logic, domain rules
│   ├── admin_health.rs     # Health-tab data builders (8-1)
│   ├── admin_system.rs     # K/V settings save + cache reload (8-5/8-8)
│   ├── auth.rs             # Shared session-rotation chain (8-8)
│   ├── auto_purge.rs       # 30-day soft-delete hard-purge (8-7)
│   ├── locking.rs          # Optimistic-lock check helpers
│   ├── password.rs         # argon2 hashing
│   ├── setup.rs            # Setup wizard step resolution + writers (8-8)
│   ├── soft_delete.rs      # Soft-delete with table whitelist
│   └── …
├── middleware/      # Axum middleware
│   ├── auth.rs             # Session extractor + role gating
│   ├── csp.rs              # Content-Security-Policy + hardening headers (7-4)
│   ├── csrf.rs             # CSRF synchronizer-token middleware (8-2)
│   ├── htmx.rs             # HTMX request/response helpers
│   ├── locale.rs           # Locale resolution chain (7-3)
│   ├── logging.rs          # tracing layer
│   ├── pending_updates.rs  # OOB metadata-update delivery
│   └── setup_gate.rs       # First-launch wizard gate (8-8)
├── models/          # DB models + queries (SQLx)
├── metadata/        # External metadata providers + KEYED_PROVIDERS const
├── tasks/           # Background tokio tasks
│   ├── anonymous_session_purge.rs  # Daily purge of stale anon sessions (8-2)
│   ├── auto_purge_scheduler.rs     # Daily soft-delete hard-purge (8-7)
│   ├── metadata_fetch.rs           # Async ISBN→metadata resolution
│   └── provider_health.rs          # 5-min provider reachability pings (8-1)
├── config.rs        # Env vars + `AppSettings` (DB-backed K/V cache)
├── lib.rs           # `AppState` definition
├── main.rs          # Startup chain (migrations → settings → registry → routes)
├── templates_audit.rs  # Architectural-invariant tests (CSP / CSRF / hx-confirm)
└── error/           # AppError enum + IntoResponse

templates/
├── layouts/         # base.html (admin + library) and bare.html (login + setup)
├── pages/           # Full-page templates (catalog, admin, setup, …)
├── components/      # Reusable Askama macros (cover, similar_titles, setup_progress, …)
└── fragments/       # HTMX partial responses + admin form fragments

static/
├── css/             # Tailwind output
└── js/              # ES modules (csrf.js, scanner-guard.js, inline-form.js, …)

migrations/          # SQLx migrations (timestamped)
locales/             # rust-i18n YAML files (en.yml, fr.yml — keys at root, no language wrapper)
docs/                # Coding conventions + architectural references
tests/
├── *.rs             # DB integration tests (#[sqlx::test])
└── e2e/             # Playwright specs + Docker test stacks
    ├── docker-compose.test.yml     # Seeded-stack lane (MYBIBLI_SKIP_SETUP=1)
    └── docker-compose.wizard.yml   # Wizard-E2E override (MYBIBLI_SKIP_SETUP="")
```

## Documentation

Product and planning documents are versioned under `_bmad-output/`:

- [`planning-artifacts/product-brief-mybibli.md`](_bmad-output/planning-artifacts/product-brief-mybibli.md) — product vision
- [`planning-artifacts/prd.md`](_bmad-output/planning-artifacts/prd.md) — functional requirements (121 FRs), NFRs, user journeys
- [`planning-artifacts/architecture.md`](_bmad-output/planning-artifacts/architecture.md) — technical decisions + ARs
- [`planning-artifacts/ux-design-specification.md`](_bmad-output/planning-artifacts/ux-design-specification.md) — UX design (30 UX-DRs)
- [`planning-artifacts/epics.md`](_bmad-output/planning-artifacts/epics.md) — epic breakdown + FR coverage map
- [`implementation-artifacts/sprint-status.yaml`](_bmad-output/implementation-artifacts/sprint-status.yaml) — live sprint state
- [`implementation-artifacts/epic-*-retro-*.md`](_bmad-output/implementation-artifacts/) — per-epic retrospectives

Coding conventions and architecture rules for contributors are in [`CLAUDE.md`](CLAUDE.md). CI/CD pipeline, Docker Hub publishing, and release procedure are documented in [`docs/ci-cd.md`](docs/ci-cd.md). The auth surface (CSRF, cookies, session policy) and its accepted posture for the single-tenant LAN/NAS deployment shape are formalized in [`docs/auth-threat-model.md`](docs/auth-threat-model.md).

## Roadmap

| Epic | Title | Status |
|------|-------|--------|
| 1 | Je catalogue mon premier livre | ✅ done |
| 2 | Je sais où sont mes livres | ✅ done |
| 3 | Tous mes médias sont gérés | ✅ done |
| 4 | Je gère mes prêts | ✅ done |
| 5 | Mes séries et ma collection | ✅ done |
| 6 | Pipeline CI/CD et fiabilité | ✅ done |
| 7 | Accès multi-rôle & Sécurité | ✅ done |
| 8 | Administration & Configuration | ✅ done |
| 9 | Polish UX & Accessibilité | ✅ done |
| 10 | Mobile UX & sécurité closeout | ✅ done |

mybibli has been live in production since v1.1.1 (2026-05-14) on the household NAS that drove the project. v1.0.0 shipped after Epic 9 close (2026-05-10) as the first production-ready build; v1.1.0 added the seed-gate + audit trio (mandatory install floor — see banner at the top of this file); v1.1.2 closed Epic 10 (mobile UX dual-surface pattern, CSRF rejection UX, axe-core entity-detail coverage); v1.1.3 was the first production-driven-feedback patch (home-page layout reorder so filter results land above the fold; locations tree rows clickable); v1.1.4 fixed issue [#213](https://github.com/guycorbaz/mybibli/issues/213) — the published `docker-compose.yml` template now persists cover JPGs in the `mybibli_covers` named volume so they survive container upgrades; v1.1.5 shipped the production-feedback iteration: [#203](https://github.com/guycorbaz/mybibli/issues/203) defensive genre fallback when metadata fetch returns nothing (the title save flow no longer trips an FK violation), [#218](https://github.com/guycorbaz/mybibli/issues/218) defense-in-depth hardening of the modal-Confirm retarget guard (now requires the `HX-Request` pairing HTMX always sets), and the docs slice of [#202](https://github.com/guycorbaz/mybibli/issues/202) — the user manual's metadata-providers chapter gets a step-by-step Google Cloud Console walk-through for the Google Books API key plus a Troubleshooting section for empty-metadata scans; v1.1.6 closed [#225](https://github.com/guycorbaz/mybibli/issues/225) — when the metadata provider chain resolves a title but the resolving provider returned no cover URL (BnF never ships image URLs in its UNIMARC payload, and Google Books occasionally lacks `imageLinks` for self-published titles), mybibli falls back to the Open Library Covers API by ISBN so the title gets a jacket image instead of a blank placeholder; v1.1.7 closed [#228](https://github.com/guycorbaz/mybibli/issues/228) — same fallback now also runs on the user-triggered "Re-fetch metadata" button (it was only wired into the background scan in 1.1.6), so existing FR titles cataloged before the cover fix can now actually receive their covers on re-fetch; v1.1.8 closed [#232](https://github.com/guycorbaz/mybibli/issues/232) — third and last bug of the cover-fallback saga. mybibli used to record every change to a title's genre in `manually_edited_fields`, which permanently pinned re-fetches to the conflict-confirmation flow and silently dropped the cover (because the form's "accept cover" checkbox defaulted to unchecked). The release stops tracking `genre_id` there, ships a one-shot migration that strips the spurious entry from existing rows, and adds the first illustrations to the README, the manual and the website; v1.1.9 closed [#238](https://github.com/guycorbaz/mybibli/issues/238) — saving a title where any numeric field (page count, track count, total duration, issue number) was empty used to return a silent HTTP 422 to the browser (no UI feedback). The four `Option<i32>` form fields now treat empty input as `None` instead of refusing to deserialize. v1.2.0 is the first minor release of the post-v1 era, themed "See what you own, find it faster" — six CRs bundled together: a chip showing each title's Dewey code next to its genre ([#236](https://github.com/guycorbaz/mybibli/issues/236)), a sort-link row to sort series by Dewey or title in addition to position ([#235](https://github.com/guycorbaz/mybibli/issues/235)), a fold/unfold toggle on the `/locations` tree with localStorage persistence ([#200](https://github.com/guycorbaz/mybibli/issues/200)), an "Uncategorized" filter chip on home that surfaces titles with no real genre yet ([#205](https://github.com/guycorbaz/mybibli/issues/205)), a spinner on the metadata-conflict "Apply" button ([#215](https://github.com/guycorbaz/mybibli/issues/215)), and a one-click admin action to re-fetch covers on every title that's missing one ([#214](https://github.com/guycorbaz/mybibli/issues/214)). v1.2.1 is a four-fix patch on top of 1.2.0: deleting a series with assigned titles now surfaces the meaningful "cannot delete: N titles assigned" message instead of a generic internal-error copy ([#139](https://github.com/guycorbaz/mybibli/issues/139)); the help-icon button no longer sits inside its associated `<label>` element (HTML5 compliance + screen-reader cleanup, [#154](https://github.com/guycorbaz/mybibli/issues/154)); an anonymous user clicking the Return-loan modal trigger from `/borrower/:id` now bounces back to that borrower page after login instead of always landing on `/loans` ([#133](https://github.com/guycorbaz/mybibli/issues/133)); and the 403 Forbidden error body finally renders in the user's UI language instead of falling back to the default locale ([#219](https://github.com/guycorbaz/mybibli/issues/219)). v1.2.2 is a three-fix defense-in-depth patch around the soft-deleted-genre semantics: a stale `?filter=genre:N` link (from a tab opened before an admin deleted that genre) now drops the chip and surfaces a localized "this genre no longer exists" notice instead of returning an empty list ([#112](https://github.com/guycorbaz/mybibli/issues/112)); SearchService SQL switches `JOIN genres` to `LEFT JOIN` + `COALESCE` so a title whose genre row got soft-deleted (an orphan-FK state that story 8-4's guard prevents via the supported path, but could be reached by a future migration or manual `UPDATE`) still appears in the catalog with a "(Genre supprimé)" placeholder ([#107](https://github.com/guycorbaz/mybibli/issues/107)); and the home dashboard's "Stats by genre" panel now uses the full active-catalog count as its denominator so per-row percentages reflect the true share even if orphan-FK titles exist (the displayed total then sums to <100%, the gap being the orphan delta — [#111](https://github.com/guycorbaz/mybibli/issues/111)). v1.3.0 is the second themed minor — "Plan your next purchase". It ships two roadmap-slotted CRs: an inline per-volume table on `/title/:id` (between the contributor block and the similar-titles strip) with the V-code, the resolved location, the condition state, and Librarian-gated Edit + Delete affordances via a UX-DR8 modal ([#209](https://github.com/guycorbaz/mybibli/issues/209)); and the first-class wish list at `/wishlist` — add books by scanning an ISBN (with provider-chain preview) OR typing a free-form title; mark-as-bought via UX-DR8 modal; auto-link banner on the catalog scan when a freshly cataloged ISBN matches a wish list entry; bookstore-friendly print view at `/wishlist/print` for the browser PDF dialog; mobile-card dual-surface; new home-dashboard counter; full audit trail ([#242](https://github.com/guycorbaz/mybibli/issues/242)). v1.3.1 is a four-papercut polish patch surfaced within hours of v1.3.0's production deployment: a loading spinner on the wish list "Rechercher" ISBN-lookup button so the 2–5s provider chain feedback isn't silent ([#258](https://github.com/guycorbaz/mybibli/issues/258)); a help-icon tooltip explaining what the **Omnibus** checkbox does in the title-detail series-assignment form ([#259](https://github.com/guycorbaz/mybibli/issues/259)); the wish list print page refactored from a dense table to a card-based layout with cover thumbnails + page footer counter, and opening in a new tab so the user doesn't lose context ([#260](https://github.com/guycorbaz/mybibli/issues/260)); and the home page's "Recent additions" section folded by default under a native `<details>` (localStorage-persisted choice) to reduce vertical clutter as the catalog grows ([#261](https://github.com/guycorbaz/mybibli/issues/261)). v1.4.0 is the third themed minor — "Bring your own agent". It ships a JSON HTTP API at `/api/v1/*` ([#241](https://github.com/guycorbaz/mybibli/issues/241)) with API-key authentication (argon2-hashed, 12-char prefix index, `Authorization: Bearer` or `X-API-Key`), two coexisting scopes (read-only / read-and-write), a focused four-field write surface (`PATCH /api/v1/titles/{id}` for `subtitle`, `description`, `dewey_code`, `genre_id` with optimistic-lock 409 + audit trail), and an admin tab `/admin?tab=api_keys` to mint keys (plaintext shown exactly once in a UX-DR8 modal), see them in a six-column list with last-used timestamps, and revoke them. The CSRF middleware short-circuits on `/api/*` because bearer-token auth doesn't ride on cookies. New manual chapter 11 ("API & integrations", EN + FR) walks through the LLM-classifier use case with curl, Python, and pseudocode examples. v1.5.0 is the fourth themed minor — "Know what you have, and what it's worth". It bundles five CRs: a Library of Congress metadata provider for English-language books ([#263](https://github.com/guycorbaz/mybibli/issues/263), slotted between Google Books and Open Library in the chain — fills the older / academic / US-government gap); a donut chart replacing the bar-chart stats-by-genre on home ([#265](https://github.com/guycorbaz/mybibli/issues/265), Chart.js v4 vendored, 5% threshold rolls small genres into an "Other" bucket with a `<details>` listing the rolled-up names); two title-lifecycle affordances ([#271](https://github.com/guycorbaz/mybibli/issues/271) delete a title with zero volumes via a UX-DR8 modal, [#272](https://github.com/guycorbaz/mybibli/issues/272) edit ISBN/ISSN/UPC in the metadata-edit form with checksum + duplicate-collision guards); a real server-rendered PDF export for the wish list ([#266](https://github.com/guycorbaz/mybibli/issues/266), genpdf + DejaVu Sans vendored, no browser print-to-PDF round-trip); and the headline feature — per-volume **collection valuation** with purchase-price + estimated-current-value columns, per-currency totals + breakdowns by genre + by series on a new `/stats/value` page, and an opt-in home-dashboard indicator (default OFF, admin toggle) ([#243](https://github.com/guycorbaz/mybibli/issues/243), CHF as the seeded default currency, admin-overridable). v1.5.1 is a five-fix production-hygiene patch shipped within hours of the v1.5.0 NAS upgrade after Guy validated the new features and surfaced the bugs: the Dockerfile was missing `COPY static/fonts/` so `/wishlist/export.pdf` crashed in production (#281); Trash permanent-delete on titles failed because the 4 child FKs weren't cascaded, fixed by extending `TrashService::permanent_delete` with the same FK-ordered chain `auto_purge` already maintained (#282); `/admin?tab=system` gained the missing "Library valuation" form section to toggle the v1.5.0 #243 settings that previously existed in code but had no UI (#283); revoked API keys can now be hard-deleted from the admin panel (Active keys still require Revoke first; soft-delete via direct UPDATE, separate from the Trash flow because a deleted plaintext is gone forever and "restore" is meaningless — #284); and the admin Health-tab provider probe stopped marking every provider Unreachable — `probe_once` now sends a User-Agent and treats any HTTP response as Reachable (only network-level errors → Unreachable, #285). v1.5.2 is a one-fix hotfix on top of v1.5.1: the v1.5.0 #243 valuation workflow was partially broken — UPDATE landed correctly, but every subsequent volume read crashed because SQLx 0.8 can't decode a raw MariaDB `DECIMAL(10,2)` into `Option<f64>` without the `bigdecimal`/`rust_decimal` feature flag. `VolumeModel::find_by_id` and `find_by_label` now wrap the two DECIMAL columns with `CAST(... AS DOUBLE)` — same pattern the aggregation queries already used and that kept `/stats/value` working. Two new sqlx::test regression guards INSERT a volume with non-NULL valuation columns and assert round-trip ([#288](https://github.com/guycorbaz/mybibli/issues/288)). v1.6.0 is the fifth themed minor — "Tighten the catalog". It ships four CRs targeting catalog hygiene: a storage location can now be marked **organizational** to refuse volume assignments — useful for nested containers that act as folders, not shelves ([#280](https://github.com/guycorbaz/mybibli/issues/280), checkbox on the create/edit form, flip-with-volumes guard, volume-edit picker rejects organizational targets); a **shelf-audit workflow** lets a librarian mark a volume "À contrôler" (single or in bulk for an entire shelf), surfaced via an amber home-dashboard indicator and a dedicated `/audit` list sorted by location → V-code ([#237](https://github.com/guycorbaz/mybibli/issues/237), `volumes.under_audit_since DATETIME NULL` migration, audit-trail entries for every mark/clear); a **"Titles without volumes"** filter chip on the home dashboard surfaces titles that have no active volume row — either soft-deleted away or never shelved ([#279](https://github.com/guycorbaz/mybibli/issues/279), Librarian+ only, `NOT EXISTS` SQL guard, mirrors the #205 Uncategorized chip shape); and the home page's list view is now a real **sortable table** (Title · Author · Genre · Dewey · Volumes), replacing the cards-stacked-vertically layout — column headers issue HTMX `hx-get` with `hx-push-url=true`, mirroring `/location/:id` ([#250](https://github.com/guycorbaz/mybibli/issues/250), grid view unchanged, sort dropdown hidden in list mode). **v1.6.1 (current)** is a one-fix hotfix on v1.6.0 surfaced within hours of the prod NAS upgrade: ticking the new #280 "Emplacement organisationnel" checkbox on a *root* location returned `400 Failed to deserialize form body: parent_id: cannot parse integer from empty string`. The location-edit form's `<select name="parent_id"><option value="">None</option>...</select>` submits `parent_id=` (empty) for root locations, and `#[serde(default)] Option<u64>` only short-circuits ABSENT fields — empty strings still hit the u64 parser. New `deserialize_optional_u64` helper (sibling of the existing `_i32` and `_f64` ones) treats empty as `None`; wired to `CreateLocationForm.parent_id`, `UpdateLocationForm.parent_id`, AND `VolumeEditForm.condition_state_id` (latent there too — picking "no condition" would 400 the same way) ([#296](https://github.com/guycorbaz/mybibli/issues/296)). Latent since the location-edit feature shipped; only surfaced now because CR #280 gave users a new reason to edit root containers. The planned-feature build phase is complete — future work is GH-issue-driven polish iterations and production-driven fixes. See [`epics.md`](_bmad-output/planning-artifacts/epics.md) for the full breakdown and [`sprint-status.yaml`](_bmad-output/implementation-artifacts/sprint-status.yaml) for the story-by-story state.

## License

Licensed under the **GNU Affero General Public License v3.0 or later** (AGPL-3.0-or-later). See [`LICENSE`](LICENSE) for the full text.

AGPL was chosen deliberately to keep mybibli and any fork freely modifiable by end users, including forks that are hosted as a service: if you run a modified version, you must offer the corresponding source to your users.
