<p align="center">
  <img src="https://raw.githubusercontent.com/guycorbaz/mybibli/main/docs/mybibli-logo/png/mybibli-logo-800w.png" alt="mybibli" width="440">
</p>

# mybibli

> **Self-hosted personal library catalog** — Rust + Axum + MariaDB. Single-tenant, single household, runs on your NAS.

[![GitHub](https://img.shields.io/badge/source-github.com%2Fguycorbaz%2Fmybibli-blue?logo=github)](https://github.com/guycorbaz/mybibli) · [![Roadmap](https://img.shields.io/badge/roadmap-guycorbaz.github.io%2Fmybibli-purple)](https://guycorbaz.github.io/mybibli/roadmap.html) · [![License: AGPL v3+](https://img.shields.io/badge/license-AGPL--3.0%20or%20later-orange)](https://github.com/guycorbaz/mybibli/blob/main/LICENSE)

## 🛑 Install 1.1.0 or later — required

Tags below `1.1.0` (`v1.0.0` … `v1.0.5`) shipped with a hard-coded `admin/admin` seed. **Anyone reaching that container's URL gained admin.** Every pre-1.1.0 tag has been removed from Docker Hub to prevent accidental installs. Fresh installs at 1.1.0+ greet you with the first-launch setup wizard (you create the admin account, the seed is gated out).

If you ran a pre-1.1.0 build, wipe the database before pulling 1.1.0+. See [the install warning](https://github.com/guycorbaz/mybibli#-do-not-install-versions-below-110) for the full writeup.

## What it is

- **Barcode-first cataloging.** Scan ISBN / EAN-13 → metadata resolves asynchronously through a provider chain (BDGest → BnF → Google Books → Library of Congress → Open Library → MusicBrainz → OMDb → TMDB) with cover-image download and similar-title detection.
- **Multi-media.** Books, BD/comics with multi-position omnibus volumes, audio, films/series — each typed correctly with the right provider chosen automatically.
- **Series + collection awareness.** Gap detection on series volumes, Dewey-based browsing, similar-titles section.
- **Storage-location tracking.** Configurable hierarchy (room → shelf → row → …), barcode-on-shelf workflow, per-location volume list, optional organizational containers (folders, not shelves), shelf-audit workflow ("À contrôler") with home-dashboard indicator.
- **Loan management.** Borrower CRUD, loan registration with automatic location restoration on return, overdue threshold (admin-configurable), per-borrower history.
- **Wishlist + valuation.** First-class `/wishlist` with provider-chain ISBN preview + free-form add, mark-as-bought, server-rendered PDF export. Optional per-volume `purchase_price` + `current_value` with per-currency totals and a `/stats/value` page (default OFF, admin opt-in).
- **JSON HTTP API.** `/api/v1/*` with API-key auth (argon2-hashed, `Authorization: Bearer` or `X-API-Key`), read-only and read-write scopes. Mint / revoke / hard-delete keys from `/admin?tab=api_keys`. CSRF short-circuits on `/api/*` because bearer auth doesn't ride on cookies.
- **Multi-role auth.** Anonymous (read-only) · Librarian (catalog + loans) · Admin (everything). Session inactivity timeout with keep-alive toast. **Four UI languages** — English, French, German, Italian — with per-user preference toggle.
- **Hardened by construction.** Strict CSP (no `unsafe-inline`/`unsafe-eval`), CSRF synchronizer-token middleware on every state-changing request with server-rendered "session expired" feedback, scanner-guard against burst-keyboard input leaking into modals.
- **Admin panel.** Health dashboard (entity counts, MariaDB version, disk usage, provider reachability), user management with last-active-admin guard, editable reference data (genres, volume states, contributor roles, location node types), system settings (loans / providers / language / valuation / **logging level**), trash view + restore + permanent delete, configurable auto-purge after 30 days.
- **Production observability** (v1.7.0+, completed in v1.7.1). Persistent daily-rotating log files with 30-day in-process purge. Admin-controlled log level (`trace` / `debug` / `info` / `warn` / `error` or full `tracing-subscriber` `EnvFilter` directives) flippable from `/admin > System` without a redeploy.
- **First-launch setup wizard.** Fresh installs walk through Admin → Providers → Preferences → Done.
- **Mobile-aware + WCAG 2.2 AA accessible.** Dual-surface mobile UX (desktop tables collapse into cards, admin tabs into `<select>`), full keyboard navigation with shortcuts cheat-sheet (`?`), contextual help-icon tooltips, axe-core CI gate over every reachable surface.

## Quick start

### Minimal `docker-compose.yml`

```yaml
services:
  mybibli:
    image: gcorbaz/mybibli:latest
    ports:
      - "8080:8080"
    environment:
      DATABASE_URL: mysql://mybibli:mybibli@db:3306/mybibli?charset=utf8mb4
      HOST: "0.0.0.0"
      PORT: "8080"
      # v1.7.0+: persistent file logging + admin-controlled level.
      # Defaults below are production-safe; admins can flip the live
      # filter from /admin > System without restarting the container.
      MYBIBLI_LOG_LEVEL: info
      MYBIBLI_LOG_DIR: /var/log/mybibli
      # v1.7.1+: per-probe timeout for the /admin > Health
      # reachability check. Bump on fragile uplinks (default 10s
      # is conservative for home-NAS networks).
      MYBIBLI_PROVIDER_HEALTH_TIMEOUT_SECS: "10"
    volumes:
      # Issue #213 — cover JPGs MUST persist across container upgrades.
      # Without this mount, every `docker compose up -d` after a pull
      # destroys the writable layer and the cataloged covers vanish.
      - mybibli-covers:/app/covers
      # v1.7.0+ — persistent daily-rotating log files (Operator can
      # `docker compose exec mybibli tail -f /var/log/mybibli/mybibli.log.$(date -u +%Y-%m-%d)`).
      # Forensic-only; safe to wipe at any time. Drop this mount if
      # you only need `docker compose logs` (no on-disk retention).
      - mybibli-logs:/var/log/mybibli
    depends_on:
      db:
        condition: service_healthy

  db:
    image: mariadb:11
    environment:
      MARIADB_ROOT_PASSWORD: changeme
      MARIADB_DATABASE: mybibli
      MARIADB_USER: mybibli
      MARIADB_PASSWORD: mybibli
    volumes:
      - mybibli-db:/var/lib/mysql
    healthcheck:
      test: ["CMD", "healthcheck.sh", "--connect", "--innodb_initialized"]
      interval: 10s
      timeout: 5s
      retries: 5

volumes:
  mybibli-db:
  mybibli-covers:
  mybibli-logs:
```

Run it:

```bash
docker compose pull
docker compose up -d
```

Open `http://localhost:8080`. The first-launch wizard greets you. Create the admin account. You're in.

### Environment reference

All deployment-time settings are environment variables — no config file. The canonical reference with every variable commented is [`.env.example`](https://github.com/guycorbaz/mybibli/blob/main/.env.example) in the repo. Grouped sections: database, HTTP server, application (incl. logging — `MYBIBLI_LOG_LEVEL` / `MYBIBLI_LOG_DIR` / `LOGS_HOST_PATH` / `MYBIBLI_PROVIDER_HEALTH_TIMEOUT_SECS`), cookie & CSP hardening, metadata providers, dev / test overrides.

### Bind-mount alternatives (Synology DSM, journald shipping, etc.)

Both `mybibli-covers` and `mybibli-logs` can be swapped from Docker-named volumes to host bind mounts. Pick a host directory (e.g. `/volume1/docker/mybibli/covers`, `/volume1/docker/mybibli/logs`) and replace the volume line with `- /your/host/path:/app/covers` / `- /your/host/path:/var/log/mybibli`. The full operator manual (chapter 1 install + chapter 12 operations) walks through it — see [the GitHub release page](https://github.com/guycorbaz/mybibli/releases/latest) for the PDF.

## Tags

- `:latest` — tracks the highest semver release tag.
- `:1.16.0` (current), `:1.15.0`, `:1.14.1` and every prior release tag back to `:1.1.0` — specific releases. Pin to a specific tag in production-style setups; tracking `:latest` is reasonable for homelab. The full tag list is on the [Tags tab](https://hub.docker.com/r/gcorbaz/mybibli/tags).
- **No `:dev`, no `:main`, no `:beta` published** — tagged releases only.

## Docs

- **End-user manual** (PDF, EN + FR) — attached to each [GitHub Release](https://github.com/guycorbaz/mybibli/releases), and committed to the repo at `docs/manual/mybibli-manual-{en,fr}.pdf` so a `git checkout vX.Y.Z` always carries the matching manual.
- **Operator README** — [github.com/guycorbaz/mybibli](https://github.com/guycorbaz/mybibli) — install + configure + run-locally + dev-stack.
- **Operations & debugging** (chapter 12 of the manual) — log location, tailing, log levels, structured JSON parsing, post-mortem grepping.
- **Auth threat model** — [docs/auth-threat-model.md](https://github.com/guycorbaz/mybibli/blob/main/docs/auth-threat-model.md) — what CSRF protects, what session cookies do, why the single-tenant LAN/NAS shape lets us simplify some auth surfaces.
- **Roadmap + release timeline** — [guycorbaz.github.io/mybibli/roadmap.html](https://guycorbaz.github.io/mybibli/roadmap.html).

## Contributing / issues / requests

- Bugs, feature requests, change requests: [github.com/guycorbaz/mybibli/issues](https://github.com/guycorbaz/mybibli/issues)
- Coding conventions, architecture rules, Foundation Rules: [CLAUDE.md](https://github.com/guycorbaz/mybibli/blob/main/CLAUDE.md)
- CI/CD pipeline + release procedure: [docs/ci-cd.md](https://github.com/guycorbaz/mybibli/blob/main/docs/ci-cd.md)

## License

[GNU AGPL v3 or later](https://github.com/guycorbaz/mybibli/blob/main/LICENSE). If you run a modified version (including hosted-as-a-service), you must offer the corresponding source to your users.
