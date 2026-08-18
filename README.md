<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/mybibli-logo/svg/mybibli-logo-dark.svg">
    <img src="docs/mybibli-logo/svg/mybibli-logo.svg" alt="mybibli" width="440">
  </picture>
</p>

![CI](https://github.com/guycorbaz/mybibli/actions/workflows/ci.yml/badge.svg?branch=main)
[![Version](https://img.shields.io/github/v/tag/guycorbaz/mybibli?label=version&sort=semver&color=blue)](https://github.com/guycorbaz/mybibli/releases)
[![License: AGPL v3](https://img.shields.io/badge/license-AGPL--3.0--or--later-lightgrey)](LICENSE)
[![Docker Pulls](https://img.shields.io/docker/pulls/gcorbaz/mybibli?logo=docker&label=docker%20pulls)](https://hub.docker.com/r/gcorbaz/mybibli)
[![Docker Image Size](https://img.shields.io/docker/image-size/gcorbaz/mybibli/1.17.0?logo=docker&label=image%20size)](https://hub.docker.com/r/gcorbaz/mybibli/tags)
[![Open Issues](https://img.shields.io/github/issues/guycorbaz/mybibli?logo=github)](https://github.com/guycorbaz/mybibli/issues)
[![Last Commit](https://img.shields.io/github/last-commit/guycorbaz/mybibli/main?logo=github)](https://github.com/guycorbaz/mybibli/commits/main)
[![Stars](https://img.shields.io/github/stars/guycorbaz/mybibli?logo=github&style=flat)](https://github.com/guycorbaz/mybibli/stargazers)
[![Rust](https://img.shields.io/badge/rust-2024-orange?logo=rust)](https://www.rust-lang.org/)

> Personal library cataloging for home collectors.

**Status:** in production since v1.1.1 (2026-05-14). 10 epics shipped; project is in GH-issue-driven polish mode. Current release: `v1.17.0`. Pre-built images on Docker Hub at [`gcorbaz/mybibli`](https://hub.docker.com/r/gcorbaz/mybibli). See [ROADMAP.md](ROADMAP.md) for what's coming next.

## What it is

`mybibli` is a self-hosted web app to catalog, locate, and loan your personal library. It is designed for a single household, running on your own hardware (typically a NAS or home server). No cloud sync, no telemetry — all data stays on your local network.

Built for collectors who want more than a spreadsheet:

- **Barcode-first cataloging.** Scan an ISBN / EAN-13 and the title resolves asynchronously through a metadata provider chain (BnF, Google Books, Open Library, Library of Congress, MusicBrainz, OMDb, TMDb, BDGest), with cover-image download and similar-title detection.
- **Multi-media support.** Books, BD/comics (with multi-position omnibus volumes), audio releases, films/series — each typed correctly and with the right metadata provider chosen automatically.
- **Series + collection awareness.** Gap detection on series volumes, Dewey-based browsing, similar-titles section.
- **Storage-location tracking.** Configurable hierarchy (room → shelf → row → …), barcode-on-shelf workflow, with a 30-second **Undo** on the last shelving or batch-location action.
- **Loan management.** Borrower CRUD, loan registration with automatic location restoration on return, overdue threshold (admin-configurable), per-borrower history.
- **Multi-role auth.** Anonymous (read-only), Librarian (catalog + loans), Admin (everything). Session inactivity timeout with keep-alive toast. FR/EN language toggle with per-user preference.
- **Hardened by construction.** Strict Content Security Policy (no `unsafe-inline`/`unsafe-eval`), CSRF synchronizer-token middleware on every state-changing request (with a server-rendered "session expired" feedback when the token drifts — see [`docs/auth-threat-model.md`](docs/auth-threat-model.md)), scanner-guard against burst-keyboard input leaking into modals.
- **Admin panel.** Health dashboard (entity counts, MariaDB version, disk usage, provider reachability), user management with last-active-admin guard, editable reference data (genres, volume states, contributor roles, location node types), system settings (overdue threshold, provider API keys, default language), trash view + restore + permanent delete, configurable auto-purge after 30 days.
- **First-launch setup wizard.** Fresh installs walk through Admin → Providers → Preferences → Done; the gate middleware redirects every route to `/setup` until completion. Idempotent — interruptions resume at the right step server-side.
- **Mobile-aware + WCAG 2.2 AA accessible.** Dual-surface mobile UX on data-dense pages (desktop tables collapse into mobile cards, admin tabs collapse into a `<select>` dropdown), full keyboard navigation with shortcuts cheat-sheet (`?`), contextual help-icon tooltips, and an axe-core CI gate that covers every reachable surface including entity-detail routes and the first-launch wizard.

## Screenshots

Live production install (`v1.17.0`, household NAS, 140+ volumes catalogued and growing):

<p align="center">
  <img src="docs/screenshots/01-home-desktop.png" alt="mybibli home page — search bar, genre filters, dashboard counters, and a recent-additions strip with cover thumbnails." width="780">
  <br>
  <em>Home — search, genre filters, dashboard counters ("À traiter" / "Aperçu de la collection"), recent additions with cover thumbnails.</em>
</p>

<p align="center">
  <img src="docs/screenshots/04-locations-desktop.png" alt="mybibli locations page — hierarchical tree of rooms, bookcases and shelves with per-node volume counts and edit/delete affordances." width="780">
  <br>
  <em>Locations — configurable hierarchy (room → bookcase → shelf …), per-node volume counts, inline create / edit / delete.</em>
</p>

<p align="center">
  <img src="docs/screenshots/09-audit-desktop.png" alt="mybibli shelf-audit page — list of volumes flagged for physical verification, with resolved location and V-code per row." width="780">
  <br>
  <em>Shelf-audit — volumes flagged "À contrôler" (single or bulk-per-shelf), sorted by location → V-code, with one-click clear per row.</em>
</p>

<p align="center">
  <img src="docs/screenshots/10-admin-health-desktop.png" alt="mybibli admin Health tab — entity counts, MariaDB version, disk usage, and per-provider reachability probes." width="780">
  <br>
  <em>Admin &gt; Health — entity counts, MariaDB version, disk usage, and metadata-provider reachability probes refreshed every 5 minutes in the background.</em>
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

Pre-built images are published to Docker Hub at [`gcorbaz/mybibli`](https://hub.docker.com/r/gcorbaz/mybibli) — `:latest` tracks the highest semver, individual tags pin to the exact release. For development against the source tree, see **Development** below.

### Installation notes

Install **v1.1.0 or later**. Pre-1.1.0 images (`v1.0.0` … `v1.0.5`) shipped seed migrations that created default `admin/admin` and `librarian/librarian` credentials on every fresh install, bypassing the first-launch wizard ([#173](https://github.com/guycorbaz/mybibli/issues/173)). The seed gate landed in v1.1.0 and is the install floor; pre-1.1.0 Docker Hub tags have been removed. `:latest` and every published tag from 1.1.0 onwards are safe — a fresh install greets you with the setup wizard. If you happen to have an older deployment, wipe the database and reinstall before adding any data.

**Skipping intermediate versions is supported.** mybibli ships releases at a brisk pace; you do not need to upgrade through every intermediate tag. The migration runner applies every pending migration in timestamp order at boot, schema migrations are purely additive (no `DROP COLUMN` / `DROP TABLE`), and the few data backfills are idempotent — so a jump from, say, `v1.3` directly to the latest tag is safe for the database. Take a backup before upgrading (there is no automatic rollback). See chapter 8 ("Upgrade and migration") of the user manual for the full procedure and the one pre-1.1.4 cover-JPG caveat.

### Persistent storage — volumes you need

`docker-compose.yml` declares **three named volumes**. The first two MUST survive container upgrades; the third is forensic-only:

- `mybibli_db_data` → `/var/lib/mysql` — your catalog (titles, volumes, loans, etc.) — **mandatory**
- `mybibli_covers` → `/app/covers` — downloaded cover JPGs (issue [#213](https://github.com/guycorbaz/mybibli/issues/213)) — **mandatory**
- `mybibli_logs` → `/var/log/mybibli` — daily-rotating log files (CR [#301](https://github.com/guycorbaz/mybibli/issues/301), v1.7.0+) — **optional but recommended** for production debuggability; can be wiped at any time

If you deploy `docker-compose.yml` from the repo unchanged (v1.7.0+), you already have all three. Back the data + covers volumes up together — losing one without the other leaves DB references pointing at missing files (or vice versa). The logs volume is forensic-only; it doesn't need backup.

**Upgrading from a pre-1.1.4 install?** Pre-1.1.4 `docker-compose.yml` did not declare `mybibli_covers`, so the cover JPGs lived inside the container's writable layer and were lost on every `docker compose up -d` after a `pull`. Adding the volume now preserves covers fetched from this point forward, but does NOT restore the ones that disappeared on prior upgrades. To recover, re-trigger metadata fetch from each affected title's detail page (the "Re-fetch metadata" button). A bulk-fetch admin action is tracked at issue [#214](https://github.com/guycorbaz/mybibli/issues/214).

**Upgrading from a pre-1.7.0 install?** The `mybibli_logs` volume is new in v1.7.0. Without it, log files write to the container's ephemeral writable layer and are lost on every `docker compose up -d` after a `pull` — which defeats the purpose of CR [#301](https://github.com/guycorbaz/mybibli/issues/301)'s persistent-log feature. Add this block to your existing `docker-compose.yml`:

```yaml
services:
  mybibli:
    volumes:
      - mybibli_logs:/var/log/mybibli   # ← add this line

volumes:
  mybibli_logs:                          # ← and this declaration
```

(Or use a bind mount: `- /your/host/path:/var/log/mybibli` if you prefer logs visible directly in DSM File Station / your journald shipper. See chapter 12 of the manual.)

**Synology DSM / bind-mount users:** comment out the `mybibli_covers:/app/covers` line in `docker-compose.yml` and uncomment the bind-mount line right below it, then set `COVERS_HOST_PATH` in your `.env` to the host path you want — Synology File Station / your rsync routine will see the covers directly. The same pattern applies to `mybibli_logs` via `LOGS_HOST_PATH`.

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
3. **Application** — `MYBIBLI_LOG_LEVEL` (v1.7.0+, tracing filter;
   prod-safe default `info`; also flippable at runtime from
   `/admin > System` without a redeploy), `MYBIBLI_LOG_DIR` (v1.7.0+,
   in-container path for daily-rotating log files; default
   `/var/log/mybibli`; mapped to the `mybibli_logs` named volume — see
   "Persistent storage" above. `LOGS_HOST_PATH` is an optional
   bind-mount override), `RUST_LOG` (legacy fallback, honored when
   `MYBIBLI_LOG_LEVEL` is unset), `APP_LANGUAGE` (`en`, `fr`, `de`, or
   `it` — v1.7.0 added DE + IT), `COVERS_DIR` (filesystem path for
   downloaded cover images — in Docker, the `/app/covers` directory is
   mapped to the persistent `mybibli_covers` named volume.
   `COVERS_HOST_PATH` is an optional bind-mount override).
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

mybibli has been live in production since v1.1.1 (2026-05-14) on the household NAS that drove the project. v1.0.0 shipped after Epic 9 close (2026-05-10) as the first production-ready build; v1.1.0 added the seed-gate + audit trio (mandatory install floor — see "Installation notes" above); the themed minors v1.2 through v1.8 then delivered the original feature roadmap (browse & find, wishlist, HTTP API, valuation & stats, catalog hygiene, de/it locales + runtime logging, cover handling), each followed by production-driven patch trains. Since v1.8 the project runs in GH-issue-driven polish mode; the current release is v1.17.0 (2026-08-18) — a metadata-clarity minor: [#202](https://github.com/guycorbaz/mybibli/issues/202) a failed metadata lookup now says *why* — every source searched and none holds the book, a source that was busy, or a source never asked for want of an API key — three situations that previously shared one message and call for opposite reactions; [#206](https://github.com/guycorbaz/mybibli/issues/206) genre and Dewey are grouped into a Classification section that states which of the two a metadata fetch may overwrite (the genre never, the Dewey code until you edit it). No migration. It follows v1.16.0 (2026-08-17) — a bibliographic-coverage and operability minor: [#450](https://github.com/guycorbaz/mybibli/issues/450) K10plus, the German union catalogue, joins the metadata chain as a zone completer and immediately becomes its leading contributor (45 titles / 100 UNIMARC zones, against 29 / 61 for the Library of Congress), gated by ISBN prefix; [#459](https://github.com/guycorbaz/mybibli/issues/459) `MYBIBLI_RESET_ADMIN`, a one-shot startup hatch that recovers a locked-out administrator without a database console; [#457](https://github.com/guycorbaz/mybibli/issues/457) the proposed L-code now counts soft-deleted rows so it is genuinely free; [#458](https://github.com/guycorbaz/mybibli/issues/458) four intermittently failing E2E specs deflaked at the source. No migration. It follows v1.15.0 (2026-08-13) — a production-log-review minor: [#202](https://github.com/guycorbaz/mybibli/issues/202) metadata provenance recorded and displayed per title, [#424](https://github.com/guycorbaz/mybibli/issues/424) light-mode contrast raised to the WCAG AA floor, [#419](https://github.com/guycorbaz/mybibli/issues/419) a third throttle-retry tier for bulk metadata runs, [#449](https://github.com/guycorbaz/mybibli/issues/449) a seven-character build commit in the startup log; one additive migration. It follows v1.14.0 (2026-07-28) — a cataloging-fix and bibliographic-coverage minor: [#440](https://github.com/guycorbaz/mybibli/issues/440)/[#441](https://github.com/guycorbaz/mybibli/issues/441)/[#442](https://github.com/guycorbaz/mybibli/issues/442) fix three defects in the scan flow (volume labels attaching to the previous title when cataloguing several UPC items in a row, a misleading "not found" when the active title had been deleted, and V-code labels staying locked after a volume was deleted), and [#439](https://github.com/guycorbaz/mybibli/issues/439) adds Library of Congress MARC 21 records so the UNIMARC-aligned fields also fill for English-language books the BnF does not hold. It follows v1.13.0 (2026-07-24) — a UNIMARC-themed feature minor: [#389](https://github.com/guycorbaz/mybibli/issues/389) six UNIMARC-aligned cataloging fields (statement of responsibility, edition statement, collection title/number, general note, original title) captured automatically from the BnF and shown on the title page, a new Health-tab *Backfill metadata from BnF* bulk action to fill them on already-cataloged titles, and [#434](https://github.com/guycorbaz/mybibli/issues/434) a per-title cataloging log summary; the field-to-zone mapping ships as [`docs/unimarc-mapping.md`](docs/unimarc-mapping.md), record import/export having since been dropped (2026-08-17) as out of scope for a household library. It follows v1.12.0 (2026-07-11) — a 2-issue feature minor: [#427](https://github.com/guycorbaz/mybibli/issues/427) two new cover sources (BnF Couvertures legal-deposit scans + Inventaire.io) that recover roughly half of the previously unfindable French/Swiss covers, and [#428](https://github.com/guycorbaz/mybibli/issues/428) a label-printing helper showing the highest V/L-codes in use on the catalog page — and v1.11.0 (same day) — a 4-issue minor driven by a production-log review: [#418](https://github.com/guycorbaz/mybibli/issues/418) persistent session cookie + admin-configurable inactivity timeout (tablet screen-locks no longer log you out mid-cataloguing), [#419](https://github.com/guycorbaz/mybibli/issues/419) bulk cover-refetch pacing + throttle back-off + completion summary, [#416](https://github.com/guycorbaz/mybibli/issues/416) daily auto-purge unblocked (orphan session rows), [#417](https://github.com/guycorbaz/mybibli/issues/417) dashboard-chip log-noise fix. It follows v1.10.0 (2026-07-01) — a single-feature minor closing [#9](https://github.com/guycorbaz/mybibli/issues/9) (undo the last scan action from the catalog feedback list within a 30-second window) — and the v1.9 line (2026-06-11): v1.9.0, the first issue-driven feature minor — [#367](https://github.com/guycorbaz/mybibli/issues/367) saved custom searches, [#396](https://github.com/guycorbaz/mybibli/issues/396) per-provider metadata-timeout overrides, [#405](https://github.com/guycorbaz/mybibli/issues/405)/[#406](https://github.com/guycorbaz/mybibli/issues/406) runtime log-level fixes — patched the same day by v1.9.1 ([#403](https://github.com/guycorbaz/mybibli/issues/403) German/Italian localization of the last two client-side messages, [#412](https://github.com/guycorbaz/mybibli/issues/412) CI test de-flake). The full release-by-release history lives in [ROADMAP.md](ROADMAP.md) and on the [GitHub releases page](https://github.com/guycorbaz/mybibli/releases). See [`epics.md`](_bmad-output/planning-artifacts/epics.md) for the epic breakdown and [`sprint-status.yaml`](_bmad-output/implementation-artifacts/sprint-status.yaml) for the story-by-story state.

## License

Licensed under the **GNU Affero General Public License v3.0 or later** (AGPL-3.0-or-later). See [`LICENSE`](LICENSE) for the full text.

AGPL was chosen deliberately to keep mybibli and any fork freely modifiable by end users, including forks that are hosted as a service: if you run a modified version, you must offer the corresponding source to your users.
