---
title: "Product Brief: mybibli"
status: "complete"
created: "2026-03-26"
updated: "2026-03-26"
inputs: [user conversation, web research, competitive analysis, skeptic review, opportunity review]
---

# Product Brief: mybibli

## Executive Summary

mybibli is a self-hosted, open-source personal library manager that lets collectors catalog, locate, and lend their physical books, comics, CDs, and DVDs from a single web interface. Built in Rust and deployed as a Docker container on consumer NAS devices (Synology, QNAP), it fills a gap no existing tool addresses: a unified, lightweight, multi-media catalog with barcode scanning, automatic metadata retrieval, hierarchical storage tracking, and loan management — all running on hardware the user already owns.

The personal library management space is fragmented. Cloud services like Libib cap free tiers at 5,000 items and lock users into proprietary platforms. Desktop apps like BookBuddy are platform-specific and single-user. Enterprise systems like Koha are wildly over-engineered for home use. Digital-only tools like Calibre or Komga ignore physical media entirely. mybibli is purpose-built for the serious collector who owns thousands of physical items across multiple media types and wants full control of their data.

Physical media collecting is experiencing a resurgence — vinyl, manga, Blu-ray, and comics are all growing markets. Collectors need modern tooling that respects their investment in physical media without requiring cloud subscriptions or complex infrastructure. mybibli is built first and foremost by a collector for his own collection, then shared as open source for others facing the same frustrations.

## The Problem

A collector with 5,000–10,000 physical items across books, comics, CDs, and DVDs faces a daily set of frustrations:

- **"Where is it?"** — Items spread across rooms, shelves, and storage boxes. Finding a specific book means remembering where it was shelved months ago.
- **"Did I lend it?"** — Books loaned to friends disappear without a trace. There's no record of who borrowed what or when.
- **"Do I already own this?"** — Standing in a bookstore, unable to check whether a title is already in the collection. Duplicate purchases are common. With remote access via VPN (e.g., Tailscale), mybibli can be queried from anywhere.
- **"Which volumes am I missing?"** — Managing 50+ comic or manga series means manually tracking hundreds of numbered volumes. Gaps go unnoticed until discovered by accident.
- **"How do I even start cataloging?"** — Manual entry of thousands of items is prohibitively slow. Without barcode scanning and automatic metadata retrieval, the task feels impossible.

Today, collectors cobble together spreadsheets, note-taking apps, or simply rely on memory. No self-hosted solution unifies physical media cataloging with the features collectors actually need.

## The Solution

mybibli is a web application accessed through any browser on the local network (or remotely via VPN). It provides:

- **Fast cataloging via barcode scanner**: Scan an ISBN or UPC with a handheld barcode reader (e.g., Inateck BCST-36). mybibli automatically retrieves title, author, cover image, and metadata from multiple online sources — including French-specific providers.
- **Unified multi-media catalog**: Books, comics, CDs, and DVDs in a single searchable database. Each title holds metadata; each physical copy (volume) is tracked individually with a unique identifier.
- **Title/volume data model**: A single title (with all its metadata) can have multiple physical copies. This correctly handles real collector scenarios: owning a paperback and hardcover of the same book, or lending out one copy while keeping another.
- **Hierarchical storage locations**: Configure storage as deeply as needed — building → room → bookcase → shelf. Know exactly where every item lives.
- **Loan tracking**: Record who borrowed which volume and when. See all outstanding loans at a glance.
- **Series management with gap detection**: Define series and their expected volumes. mybibli highlights which volumes are missing from the collection.
- **Unique volume labeling**: Each physical copy receives a unique identifier. Labels are managed externally (printed via gLabel on standard label sheets), with mybibli ensuring identifier uniqueness.
- **Storage location labeling**: Each storage location can also receive a barcode or QR code label. Scanning a location label shows its contents or assigns the current volume to that location during cataloging.
- **Role-based access**: Browsing and searching requires no login — anyone on the network can consult the catalog. Modifying data (cataloging, loans) requires a Librarian account; system configuration requires an Admin account.

## What Makes This Different

- **No equivalent exists**: No self-hosted tool combines physical multi-media cataloging, barcode scanning, hierarchical storage, loan tracking, and series gap detection. mybibli is the first.
- **Runs on hardware you own**: Docker on a NAS — no cloud subscription, no vendor lock-in, no data leaving your network. MariaDB integrates with the NAS backup system.
- **Privacy-first**: A collector's library is a deeply personal dataset — reading habits, media tastes, lending circles. mybibli keeps that data under the user's full control, on their own hardware.
- **Minimal footprint**: Rust compiles to a single static binary producing a Docker image under 100 MB (application and runtime assets; cover images and database stored separately). This matters on resource-constrained NAS devices running alongside Plex, Home Assistant, and other services.
- **Built for bulk cataloging**: The barcode scanner workflow is optimized for speed — scan, confirm metadata, assign location, next. Targeting 80+ items per hour.
- **Bilingual from day one**: French and English UI with internationalization-ready architecture, serving both Francophone and Anglophone self-hosted communities.
- **Open source**: Free to use, inspect, and extend. Designed to be easily installable and configurable by anyone with a NAS and Docker.

## Who This Serves

**Primary user: The serious personal collector**
Owns 5,000–10,000 physical items across multiple media types. Has dedicated storage (bookshelves, media cabinets, storage rooms). Values organization but has been defeated by the scale of manual cataloging. Comfortable with Docker on a NAS but not a software developer. Wants a tool that "just works" after initial setup.

**Secondary users: Household members**
Family members or housemates who browse the catalog, check availability, or borrow items. Need a simple, intuitive interface — they won't read documentation. Can browse without logging in; those who help with cataloging get a Librarian account.

**Adjacent potential: Micro-libraries**
Small community libraries (church libraries, neighborhood book exchanges, homeschool co-ops) that need cataloging and lending but find Koha absurdly overbuilt. mybibli covers 90% of their needs at 1% of the complexity. This audience requires minimal additional features (e.g., a public catalog view) and could drive significant adoption.

## Success Criteria

- A collection of 5,000–10,000 items can be fully cataloged within a few days of dedicated work (target: 80+ items/hour with barcode scanner)
- Any item can be located (which shelf, which room) within seconds via search (target: search response < 500 ms on 10,000 items)
- All outstanding loans are visible at a glance — who has what, since when
- Duplicate purchases are prevented: the system warns when scanning an already-owned ISBN/UPC
- Series gaps are immediately visible, enabling targeted purchasing
- The application is installable by a technically comfortable user in under 30 minutes (Docker + NAS MariaDB)
- 3–4 users can work concurrently without conflicts
- Metadata hit rate > 85% across the collection (books, comics, CDs, DVDs combined)

## Scope

### In scope (v1)

- Web UI (responsive, mobile-friendly for checking loans/locations on the go)
- Barcode input via USB scanner (douchette) acting as keyboard input
- Multi-source metadata lookup:
  - Books: Open Library, Google Books, BnF (data.bnf.fr) for French editions
  - Comics/BD: BDGest for Franco-Belgian comics, Comic Vine for Western comics
  - Music/CDs: MusicBrainz (UPC/barcode lookup)
  - Movies/DVDs: TMDb, OMDb
- Manual metadata entry and editing as fallback
- Cover image retrieval and storage
- Title/volume data model: one title (metadata) → multiple physical copies
- Hierarchical, configurable storage locations
- Loan management (who, what, when)
- Series definition and missing volume detection
- Unique volume identifier management (for external label printing via gLabel)
- Storage location identifier management with barcode/QR code (for external label printing via gLabel)
- Role-based access: Anonymous (search, browse, view — no login required), Librarian (add/edit titles, manage loans and locations — requires login), Admin (full access, user management, configuration — requires login)
- MariaDB backend (compatible with Synology native MariaDB)
- Docker deployment
- French and English UI (internationalization-ready)

### Out of scope (v1)

- Camera-based barcode scanning in the browser
- Direct label printing from the application
- Mobile native app
- Data import from external tools (CSV, Libib, Goodreads)
- Social features (sharing collections between users)
- E-book or digital media management
- Advanced analytics or collection valuation
- Wishlist or acquisition workflow

## Technical Approach

- **Backend**: Rust with Axum web framework, SQLx for compile-time checked queries against MariaDB
- **Frontend**: Server-rendered HTML with HTMX or Alpine.js for interactivity (lightweight, no heavy JS framework)
- **Database**: MariaDB (NAS-native, backup-integrated). Cover images stored on filesystem (included in NAS backup scope)
- **Metadata sources**: Open Library, Google Books API, BnF, BDGest, MusicBrainz, TMDb/OMDb — with fallback chain and manual entry when APIs miss
- **Barcode handling**: Server-side generation via `barcoders`/`rxing` crate for unique volume identifiers; client-side input from USB scanner received as keyboard events in a text field
- **Deployment**: Single Docker image, minimal configuration, documented for Synology/QNAP
- **Authentication**: Built-in user management with Argon2 password hashing, session-based auth. Anonymous read access (no login for browsing/searching), two authenticated roles (Librarian, Admin)
- **Remote access**: Not built into the application; users leverage existing VPN solutions (e.g., Tailscale) for remote queries

## Known Risks and Mitigations

| Risk | Severity | Mitigation |
|------|----------|------------|
| Metadata API coverage gaps (especially French editions, older CDs, niche comics) | High | Multi-source fallback chain; BnF and BDGest for French content; manual entry as last resort; validate with a 50-item sample before heavy development |
| API rate limits (Google Books 1K/day, MusicBrainz 1 req/sec) | Medium | Client-side caching of results; batch scanning sessions with queued lookups; respect rate limits in the API client layer |
| Ambitious scope for a solo developer (Rust + web UI + 6 APIs + Docker + i18n + auth) | High | Iterative delivery — core cataloging and search first, then layers (loans, series, roles); leverage AI-assisted development |
| Synology MariaDB version compatibility | Medium | Test against MariaDB 10.x early; document minimum version; offer option to use a containerized MariaDB as alternative |
| Concurrent access conflicts (two users editing the same item) | Low | Optimistic locking on database records; UI feedback on conflicts |

## Vision

mybibli starts as a personal tool solving a real problem for its creator. The open-source release targets the self-hosted community — a growing audience that actively promotes tools filling genuine gaps. With Docker-first deployment and minimal configuration, mybibli aims to become the default answer when someone asks "how do you manage your physical media collection?"

The Francophone self-hosted community is large and underserved by English-only tools. A French-first launch in communities like forum-nas.fr and French tech channels, combined with full English support, establishes mybibli in both markets from day one.

Long-term possibilities include community-contributed metadata improvements, collection import from existing tools, camera-based barcode scanning, and integration with collection marketplaces — but the core mission remains simple: **know what you own, where it is, and who has it.**
