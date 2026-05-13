# mybibli

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

**Status:** first public release shipped — `v1.0.0`. All nine epics done. Pre-built images on Docker Hub at [`gcorbaz/mybibli`](https://hub.docker.com/r/gcorbaz/mybibli).

## What it is

`mybibli` is a self-hosted web app to catalog, locate, and loan your personal library. It is designed for a single household, running on your own hardware (typically a NAS or home server). No cloud sync, no telemetry — all data stays on your local network.

Built for collectors who want more than a spreadsheet:

- **Barcode-first cataloging.** Scan an ISBN / EAN-13 and the title resolves asynchronously through a metadata provider chain (BnF, Google Books, Open Library, MusicBrainz, OMDb, TMDB, BDGest), with cover-image download and similar-title detection.
- **Multi-media support.** Books, BD/comics (with multi-position omnibus volumes), audio releases, films/series — each typed correctly and with the right metadata provider chosen automatically.
- **Series + collection awareness.** Gap detection on series volumes, Dewey-based browsing, similar-titles section.
- **Storage-location tracking.** Configurable hierarchy (room → shelf → row → …), barcode-on-shelf workflow.
- **Loan management.** Borrower CRUD, loan registration with automatic location restoration on return, overdue threshold (admin-configurable), per-borrower history.
- **Multi-role auth.** Anonymous (read-only), Librarian (catalog + loans), Admin (everything). Session inactivity timeout with keep-alive toast. FR/EN language toggle with per-user preference.
- **Hardened by construction.** Strict Content Security Policy (no `unsafe-inline`/`unsafe-eval`), CSRF synchronizer-token middleware on every state-changing request, scanner-guard against burst-keyboard input leaking into modals.
- **Admin panel.** Health dashboard (entity counts, MariaDB version, disk usage, provider reachability), user management with last-active-admin guard, editable reference data (genres, volume states, contributor roles, location node types), system settings (overdue threshold, provider API keys, default language), trash view + restore + permanent delete, configurable auto-purge after 30 days.
- **First-launch setup wizard.** Fresh installs walk through Admin → Providers → Preferences → Done; the gate middleware redirects every route to `/setup` until completion. Idempotent — interruptions resume at the right step server-side.

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
   (filesystem path for downloaded cover images).
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

Coding conventions and architecture rules for contributors are in [`CLAUDE.md`](CLAUDE.md). CI/CD pipeline, Docker Hub publishing, and release procedure are documented in [`docs/ci-cd.md`](docs/ci-cd.md).

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

v1.0.0 shipped after Epic 9 close (2026-05-10). See [`epics.md`](_bmad-output/planning-artifacts/epics.md) for the full breakdown and [`sprint-status.yaml`](_bmad-output/implementation-artifacts/sprint-status.yaml) for the story-by-story state.

## License

Licensed under the **GNU Affero General Public License v3.0 or later** (AGPL-3.0-or-later). See [`LICENSE`](LICENSE) for the full text.

AGPL was chosen deliberately to keep mybibli and any fork freely modifiable by end users, including forks that are hosted as a service: if you run a modified version, you must offer the corresponding source to your users.
