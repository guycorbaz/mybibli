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

- **Barcode-first cataloging.** Scan ISBN / EAN-13 → metadata resolves asynchronously through a provider chain (BnF → Google Books → Open Library → MusicBrainz → OMDb → TMDB → BDGest) with cover-image download and similar-title detection.
- **Multi-media.** Books, BD/comics with multi-position omnibus volumes, audio, films/series — each typed correctly with the right provider chosen automatically.
- **Series + collection awareness.** Gap detection on series volumes, Dewey-based browsing, similar-titles section.
- **Storage-location tracking.** Configurable hierarchy (room → shelf → row → …), barcode-on-shelf workflow, per-location volume list.
- **Loan management.** Borrower CRUD, loan registration with automatic location restoration on return, overdue threshold (admin-configurable), per-borrower history.
- **Multi-role auth.** Anonymous (read-only) · Librarian (catalog + loans) · Admin (everything). Session inactivity timeout with keep-alive toast. FR/EN language toggle with per-user preference.
- **Hardened by construction.** Strict CSP (no `unsafe-inline`/`unsafe-eval`), CSRF synchronizer-token middleware on every state-changing request with server-rendered "session expired" feedback, scanner-guard against burst-keyboard input leaking into modals.
- **Admin panel.** Health dashboard (entity counts, MariaDB version, disk usage, provider reachability), user management with last-active-admin guard, editable reference data (genres, volume states, contributor roles, location node types), system settings, trash view + restore + permanent delete, configurable auto-purge after 30 days.
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
    volumes:
      # Issue #213 — cover JPGs MUST persist across container upgrades.
      # Without this mount, every `docker compose up -d` after a pull
      # destroys the writable layer and the cataloged covers vanish.
      - mybibli-covers:/app/covers
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
```

Run it:

```bash
docker compose pull
docker compose up -d
```

Open `http://localhost:8080`. The first-launch wizard greets you. Create the admin account. You're in.

### Environment reference

All deployment-time settings are environment variables — no config file. The canonical reference with every variable commented is [`.env.example`](https://github.com/guycorbaz/mybibli/blob/main/.env.example) in the repo. Seven sections: database, http, language, auth, metadata providers, paths, runtime.

## Tags

- `:latest` — tracks the highest semver release tag.
- `:1.5.1` (current), `:1.5.0`, `:1.4.0`, `:1.3.1`, `:1.3.0`, `:1.2.2`, `:1.2.1`, `:1.2.0`, `:1.1.9`, `:1.1.8`, `:1.1.7`, `:1.1.6`, `:1.1.5`, `:1.1.4`, `:1.1.3`, `:1.1.2`, `:1.1.1`, `:1.1.0` — specific releases. Pin to a specific tag in production-style setups; track `:latest` is reasonable for homelab.
- **No `:dev`, no `:main`, no `:beta` published** — tagged releases only.

## Docs

- **End-user manual** (PDF, EN + FR) — attached to each [GitHub Release](https://github.com/guycorbaz/mybibli/releases).
- **Operator README** — [github.com/guycorbaz/mybibli](https://github.com/guycorbaz/mybibli) — install + configure + run-locally + dev-stack.
- **Auth threat model** — [docs/auth-threat-model.md](https://github.com/guycorbaz/mybibli/blob/main/docs/auth-threat-model.md) — what CSRF protects, what session cookies do, why the single-tenant LAN/NAS shape lets us simplify some auth surfaces.
- **Roadmap + release timeline** — [guycorbaz.github.io/mybibli/roadmap.html](https://guycorbaz.github.io/mybibli/roadmap.html).

## Contributing / issues / requests

- Bugs, feature requests, change requests: [github.com/guycorbaz/mybibli/issues](https://github.com/guycorbaz/mybibli/issues)
- Coding conventions, architecture rules, Foundation Rules: [CLAUDE.md](https://github.com/guycorbaz/mybibli/blob/main/CLAUDE.md)
- CI/CD pipeline + release procedure: [docs/ci-cd.md](https://github.com/guycorbaz/mybibli/blob/main/docs/ci-cd.md)

## License

[GNU AGPL v3 or later](https://github.com/guycorbaz/mybibli/blob/main/LICENSE). If you run a modified version (including hosted-as-a-service), you must offer the corresponding source to your users.
