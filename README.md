# mybibli

> Personal library cataloging for home collectors.

**Status:** active development — pre-v1 (current version: `0.1.0`). First release targeted after Epic 8 (admin page) lands.

## What it is

`mybibli` is a self-hosted web app to catalog, locate, and loan your personal library. It is designed for a single household, running on your own hardware (typically a NAS or home server). No cloud sync, no telemetry — all data stays on your local network.

Built for collectors who want more than a spreadsheet: barcode-first cataloging workflow, multi-provider metadata resolution (BnF, Google Books, Open Library, MusicBrainz, OMDb, TMDB, BDGest), series gap detection, storage-location tracking, and loan management.

## Tech stack

- **Backend:** Rust 2024 edition + [Axum](https://github.com/tokio-rs/axum) 0.8
- **Database:** MariaDB via [SQLx](https://github.com/launchbadge/sqlx) 0.8 (offline query cache)
- **Templates:** [Askama](https://github.com/djc/askama) 0.15 (compile-time type-checked)
- **Frontend:** [HTMX](https://htmx.org/) 2.0 + [Tailwind CSS](https://tailwindcss.com/) v4 — no SPA framework
- **i18n:** [rust-i18n](https://github.com/longbridgeapp/rust-i18n) — French + English
- **Testing:** `cargo test` (326 unit), `#[sqlx::test]` (18 DB integration), [Playwright](https://playwright.dev/) (133 E2E)

## Quick start (end users)

Pre-built images are published to Docker Hub once v1 ships. Until then, see **Development** below.

## Development

### Prerequisites

- Docker + Docker Compose
- Rust toolchain (rustup, Rust 2024 edition)
- Node.js 20+ (for Playwright E2E tests)

### Run the app locally

```bash
# Start the full stack (app + MariaDB + mock metadata providers)
cd tests/e2e
docker compose -f docker-compose.test.yml up --build
```

The app listens on `http://localhost:8080`. Default admin credentials are seeded by `migrations/20260331000004_fix_dev_user_hash.sql` (username `admin`, password `admin` — dev only).

### Build & check (native)

```bash
cargo check                          # Fast type-check
cargo build                          # Full debug build
cargo clippy -- -D warnings          # Lint (zero-warnings policy)
```

### Unit tests

```bash
cargo test                           # All unit tests (326)
cargo test config::                  # Module-scoped
cargo test <name> -- --nocapture     # Single test with output
```

### DB integration tests

```bash
docker compose -f tests/docker-compose.rust-test.yml up -d
SQLX_OFFLINE=true \
DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
cargo test --test find_similar \
           --test metadata_fetch_dewey \
           --test find_by_location_dewey
```

Each test gets a fresh DB via `#[sqlx::test(migrations = "./migrations")]`.

### E2E tests

```bash
cd tests/e2e
docker compose -f docker-compose.test.yml up --build -d
npm test                             # Full suite (133 tests, parallel mode)
npx playwright test specs/journeys/<spec>.spec.ts  # Single spec
```

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
├── services/        # Business logic, domain rules
├── models/          # DB models + queries (SQLx)
├── metadata/        # External metadata providers (BnF, Google Books, etc.)
├── tasks/           # Background tokio tasks (async metadata fetch)
├── middleware/      # Axum middleware (auth, HTMX, logging)
└── error/           # AppError enum + IntoResponse

templates/
├── layouts/         # base.html
├── pages/           # Full-page templates
├── components/      # Reusable Askama macros (cover, similar_titles, etc.)
└── fragments/       # HTMX partial responses

migrations/          # SQLx migrations (timestamped)
locales/             # rust-i18n YAML files
tests/
├── *.rs             # DB integration tests (#[sqlx::test])
└── e2e/             # Playwright specs + Docker test stack
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

Coding conventions and architecture rules for contributors are in [`CLAUDE.md`](CLAUDE.md).

## Roadmap

| Epic | Title | Status |
|------|-------|--------|
| 1 | Je catalogue mon premier livre | ✅ done |
| 2 | Je sais où sont mes livres | ✅ done |
| 3 | Tous mes médias sont gérés | ✅ done |
| 4 | Je gère mes prêts | ✅ done |
| 5 | Mes séries et ma collection | ✅ done |
| 6 | Pipeline CI/CD et fiabilité | 🚧 in planning |
| 7 | Accès multi-rôle & Sécurité | ⏳ backlog |
| 8 | Administration & Configuration | ⏳ backlog |
| 9 | Polish UX & Accessibilité | ⏳ backlog |

v1 release will ship after Epic 8. See [`epics.md`](_bmad-output/planning-artifacts/epics.md) for the full breakdown.

## License

Not yet licensed — all rights reserved. A permissive license will be added before v1 release.
