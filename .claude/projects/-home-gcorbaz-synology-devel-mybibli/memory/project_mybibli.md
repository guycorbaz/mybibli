---
name: Project mybibli
description: Personal library manager - Rust/Axum, MariaDB, Docker, self-hosted on NAS, open source, multi-media (books, BD, CD, DVD)
type: project
---

- Product Brief completed 2026-03-26, located at _bmad-output/planning-artifacts/product-brief-mybibli.md
- Product Brief Distillate (detail pack) at _bmad-output/planning-artifacts/product-brief-mybibli-distillate.md — contains competitive analysis, API details, Rust crate specifics, rejected ideas, open questions for PRD
- Stack: Rust + Axum + SQLx + MariaDB + HTMX (Alpine.js dropped)
- Docker deployment on Synology NAS
- Metadata APIs: Open Library, Google Books, BnF, BDGest, Comic Vine, MusicBrainz, TMDb, OMDb
- Access model: Anonymous read (no login), Librarian (login), Admin (login)
- Barcode: USB scanner (douchette) only, no camera scanning in v1
- Labels: unique IDs managed by mybibli, printed externally via gLabel
- Remote access via Tailscale (not built into app)
- Open source project, designed to be easily installable
- PRD completed and validated 2026-03-26, file at _bmad-output/planning-artifacts/prd.md (102 FRs, 37 NFRs, 6 journeys, 6 milestones)
- PRD validation report at _bmad-output/planning-artifacts/prd-validation-report.md — all checks pass
- Development model: 1 developer (Guy) + Claude Code (AI-assisted)
- CLAUDE.md rules: DRY (no duplicated code), unit tests on all functions, Playwright e2e tests on all features
- Party Mode vision session completed — extensive decisions captured in _bmad-output/planning-artifacts/party-mode-vision-decisions.md
- **Why:** Guy wants to catalog his entire collection in days, find items fast, track loans, avoid duplicates
- **How to apply:** All design decisions should favor simplicity and ease of use for 3-4 concurrent non-technical users
- License: GPL v3
- Versioning: semver
- Media types v1: Book, BD, CD, DVD, Magazine, Report
- Next steps: Create UX Design, then Create Architecture (in new context windows)
