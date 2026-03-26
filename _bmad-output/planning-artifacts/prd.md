---
stepsCompleted: [step-01-init, step-02-discovery, step-02b-vision, step-02c-executive-summary, step-03-success, step-04-journeys, step-05-domain-skipped, step-06-innovation-skipped, step-07-project-type, step-08-scoping, step-09-functional, step-10-nonfunctional, step-11-polish, step-12-complete]
status: complete
completedAt: 2026-03-26
inputDocuments: [product-brief-mybibli.md, product-brief-mybibli-distillate.md]
documentCounts:
  briefs: 2
  research: 0
  brainstorming: 0
  projectDocs: 0
classification:
  projectType: web_app
  domain: collection_management
  complexity: medium
  projectContext: greenfield
workflowType: 'prd'
---

# Product Requirements Document - mybibli

**Author:** Guy
**Date:** 2026-03-26

## Executive Summary

mybibli is a self-hosted, open-source web application for managing personal collections of physical media — books, comics (BD), CDs, DVDs, magazines, and reports. It runs as a Docker container on consumer NAS devices (Synology, QNAP) using the NAS-native MariaDB for seamless backup integration.

The application solves a problem no existing tool addresses: unified cataloging of thousands of physical items across multiple media types with barcode scanning, automatic metadata retrieval, hierarchical storage tracking, loan management, and series gap detection — all from a single browser-based interface. The system transforms a barcode into a complete catalog entry in seconds.

The cataloging workflow is built for sustained throughput: scan an ISBN/UPC with a handheld barcode reader, scan a pre-printed label to register the physical volume, and move on. Metadata is fetched asynchronously from 6+ sources — including BnF and BDGest for French-language editions and Franco-Belgian comics, a gap most competing tools ignore entirely — while the user continues scanning. A single intelligent input field auto-detects code types by prefix (ISBN, volume label, storage location), eliminating mode switching. Optional audio feedback confirms each action without requiring eyes on screen. The target is 80+ items cataloged per hour.

The data model cleanly separates titles (metadata, cover art, series membership) from physical volumes (individual copies with condition, location, and loan status). This supports real collector scenarios: multiple editions of the same work, duplicate copies, and lending individual volumes while keeping others. Storage locations form a configurable tree of arbitrary depth (room → bookcase → shelf → box). Series track expected volumes and highlight gaps for targeted purchasing.

Three access levels serve different users: anonymous browsing requires no login — anyone on the network can search the catalog. Librarian accounts handle cataloging and loans. Admin accounts manage system configuration, user accounts, and reference data. The application targets 3–4 concurrent users managing 5,000–10,000 physical items.

Beyond personal collectors, mybibli's lightweight approach makes it suitable for micro-libraries (church libraries, neighborhood book exchanges, homeschool co-ops) that need cataloging and lending without the overwhelming complexity of enterprise systems like Koha.

Built in Rust, mybibli produces a Docker image under 100 MB with a runtime memory footprint under 100 MB — lightweight enough to run alongside Plex, Home Assistant, and other services on resource-constrained NAS hardware. The lightweight web interface ships bilingual (French/English) from day one with light and dark modes.

### What Makes This Special

No self-hosted tool combines physical multi-media cataloging, barcode scanning, automatic metadata retrieval, hierarchical storage, loan tracking, and series gap detection. Existing alternatives are either cloud-locked (Libib), platform-specific (BookBuddy), digital-only (Calibre, Komga), or wildly over-engineered for personal use (Koha).

mybibli's core differentiator is making the impossible simple — no spreadsheet can scan a barcode and auto-populate a complete catalog entry. The as-you-type search turns a collection of thousands into an instantly queryable database. The entire UX philosophy is zero wait, immediate feedback: the user is never blocked, every scan produces an instant response, and the interface stays out of the way during extended cataloging sessions.

First-class support for French-language metadata (BnF, BDGest) serves the underserved Francophone self-hosted community alongside full English support.

Built by a collector for his own 5,000+ item collection, then shared as open source for the self-hosted community.

## Project Classification

- **Project Type:** Web Application (server-rendered with lightweight dynamic updates)
- **Domain:** Collection Management / Personal Asset Tracking
- **Complexity:** Medium — rich relational data model, 6+ external API integrations, multi-media metadata handling, role-based access, internationalization
- **Project Context:** Greenfield — no existing codebase, no data to migrate

## Success Criteria

### User Success

- A collector can catalog their entire collection (5,000–10,000 items) within a few days of dedicated work
- The barcode scan-to-catalog workflow achieves 80+ items per hour sustained throughput
- The shelving workflow achieves 60+ volumes per hour (scan volume + scan location per item)
- Any item can be found via as-you-type search within seconds (search response < 500 ms on 10,000 items)
- A user standing in a bookstore can check via Tailscale whether they already own a title — in under 10 seconds
- Series gaps are immediately visible, enabling targeted purchasing decisions
- Outstanding loans are visible at a glance — who has what, since when, with overdue items highlighted
- No duplicate purchases: the system opens the existing title when scanning an already-owned ISBN/UPC
- A secondary user can search for a title and check its availability without any instruction or documentation
- A librarian can identify all actionable items (unshelved volumes, overdue loans, series gaps) within 5 seconds of opening the application

### Business Success

- mybibli is the creator's daily tool for managing his own 5,000+ item collection within 3 months of first release
- The project is installable by a technically comfortable user (Docker + NAS) in under 30 minutes
- The entire collection management workflow (catalog, shelve, lend, return) is handled exclusively through mybibli — no fallback to spreadsheets or memory

### Community Adoption Goals

- Featured on the Awesome Self-Hosted list within 6 months of public release
- Positive reception on r/selfhosted and Francophone self-hosted communities (forum-nas.fr, r/selfhosted_fr)
- At least 3 external users successfully running their own instances within 6 months of public release

### Technical Success

- Docker image under 100 MB, runtime memory under 100 MB
- Container startup time under 10 seconds (docker start to application available)
- Metadata hit rate > 85% across all media types combined (books, BD, CDs, DVDs, magazines)
- 3–4 concurrent users with no data conflicts (optimistic locking)
- All admin-configurable data stored in MariaDB — single backup target for the NAS
- Cover images resized and stored efficiently (< 100 KB average per image, ~1 GB total for 10,000 items)
- API rate limits respected, with graceful degradation when a source is unavailable

### Measurable Outcomes

> **Note:** Performance-related targets are defined authoritatively in [Non-Functional Requirements > Performance](#performance). This table consolidates all measurable outcomes for quick reference.

| Metric | Target | How to Measure | Source |
|--------|--------|---------------|--------|
| Cataloging throughput | ≥ 80 items/hour | Timed session with barcode scanner | User Success |
| Shelving throughput | ≥ 60 volumes/hour | Timed session scan volume + scan location | User Success |
| Search latency | < 500 ms on 10K items | Server-side timing on search endpoint | NFR1 |
| Dashboard glance test | < 5 seconds to actionable items | User observation test | User Success |
| Metadata hit rate | > 85% | Ratio of auto-filled vs manual entries over 200 items | Technical Success |
| Installation time | < 30 minutes | End-to-end from docker-compose to first scan | Business Success |
| Docker image size | < 100 MB | Image size after build | NFR32 |
| Runtime memory | < 100 MB | Container stats under normal load | NFR35 |
| Container startup | < 10 seconds | Time from docker start to HTTP 200 | NFR7 |
| Concurrent users | 3–4 without conflicts | Load test with simultaneous operations | NFR8 |

## Product Scope

### MVP - Minimum Viable Product

> **Note:** This section lists all MVP capabilities by domain. For delivery order, see [MVP Delivery Milestones](#mvp-delivery-milestones) in the Project Scoping section.

**Cataloging & Metadata**
- Six media types: Book, BD, CD, DVD, Magazine, Report
- Barcode input via USB scanner (douchette) acting as keyboard input
- Metadata retrieval from 8 APIs with fallback chain (Open Library, Google Books, BnF, BDGest, Comic Vine, MusicBrainz, TMDb, OMDb)
- Manual metadata entry and editing as fallback
- Cover image retrieval, resizing, and storage
- Re-download metadata button with manual edit protection (confirmation before overwriting)
- ISSN support for magazines

**Data Model**
- Title/volume separation: one title (metadata) → multiple physical volumes
- Unified contributor entity with roles (author, director, composer, performer, illustrator...)
- Many-to-many title-contributor relationships, multiple roles per contributor per title
- Volume fields: unique label (V0001–V9999), configurable condition/state with loanable flag, edition comment, storage location
- Title fields: type-adaptive form (fields shown/hidden based on media type), language field
- One genre per title, configurable genre list

**Storage & Organization**
- Hierarchical storage locations as configurable tree of arbitrary depth
- Storage location identifiers (L0001–L9999) scannable from pre-printed barcode/QR code labels (printed externally via gLabel)
- "Not shelved" status for volumes awaiting shelving
- Navigable location content view (sortable by title/author/genre)

**Series Management**
- Series definition (manual; API-populated on best-effort basis when metadata providers return series information)
- Open series (ongoing) and closed series (known total)
- Missing volume detection (title without physical volume = gap)
- BD omnibus as special volume within series

**Loans**
- Borrower entity with full contact details (name, address, email, phone)
- Dedicated loans page showing all current loans
- Loan return via borrower page or loans page (with scan-to-find)
- Overdue loan highlighting with configurable threshold (days)
- No loan history (current loans only)

**Interface & UX**
- Single intelligent scan field with prefix auto-detection (978/979 → ISBN, 977 → ISSN, V → volume, L → location, other → UPC/unknown)
- As-you-type search on title, subtitle, description, and contributor
- Filters by genre and volume state
- Classic pagination
- Cross-navigation: everything clickable (contributor → titles, series → volumes, location → contents, title → volumes)
- Dynamic scan feedback list (successful items fade, errors persist with clickable details)
- Optional audio feedback (configurable sounds for title found, volume created, error)
- Autofocus on scan field after every server response
- Keyboard shortcuts (Enter, Escape, Tab)
- Contextual help: tooltips, ? icons with help bubbles, placeholder text in fields
- Light mode and dark mode with manual toggle + prefers-color-scheme
- Bilingual UI: French and English (internationalization-ready)

**Home Page**
- Anonymous: search bar, global statistics, recent additions, storage locations view, stats by genre
- Librarian/Admin: clickable tags with counters (unshelved, overdue, series gaps, recent cataloged, recent returns)

**Access & Security**
- Anonymous read access (no login) — sees "on loan" without borrower name
- Librarian role (login): catalog, edit, manage loans (sees borrower details)
- Admin role (login): user management, system configuration, reference data management
- Argon2 password hashing, session-based auth (no auto-timeout, expires on browser close)

**Configuration & Deployment**
- Docker deployment with environment variables for secrets (DB connection, API keys)
- All admin settings in MariaDB (genres, volume states, location node types, contributor roles)
- API keys configurable, sources without keys skipped in fallback chain
- System health page: mybibli version, MariaDB version, disk usage, entity counts, API status

**Data Protection**
- Deletion rules: no entity deleted if referenced elsewhere
- Deleting last volume preserves the title
- Volume return required before deletion
- Optimistic locking for concurrent access

**Database**
- MariaDB backend compatible with Synology native MariaDB
- SQLx migrations tooling from day one (strict discipline from first public release)

### Growth Features (Post-MVP)

- CSV import (from Libib, Goodreads, spreadsheets)
- CSV export (collection data)
- Camera-based barcode scanning via browser (QuaggaJS/ZXing-js)
- Fuzzy/tolerant search
- Tags and free-form keywords on titles
- Title-to-title linking (translations, related works)
- Public catalog view for micro-libraries
- Synology SPK package for native app store distribution
- Batch series assignment (select multiple titles, assign to series in one operation)
- Additional media types (vinyl, video games)

### Vision (Future)

- Community-contributed metadata improvements and shared metadata database
- Integration with collection marketplaces (Discogs, BookFinder, AbeBooks)
- Community translation workflow for additional UI languages
- Mobile-optimized scanning companion (PWA)
- Federation of mybibli instances (network of micro-libraries sharing catalog visibility)

## User Journeys

### Journey 1: Guy Catalogs His Collection — The Saturday Marathon

**Persona:** Guy, 50s, serious collector with 5,000+ physical items accumulated over decades. Books overflow shelves, CDs fill cabinets, BD series have gaps he can't track. He's tried spreadsheets — abandoned after 200 entries. Today he's committed: mybibli is installed, his Inateck BCST-36 scanner is charged, and a fresh pack of label sheets is loaded in the Brother printer.

**Opening Scene:** Guy sits at his desk with a stack of 50 books pulled from the living room shelves. He's printed a sheet of labels (V0001–V0033 on this sheet) using gLabel. He peels V0001 and sticks it on the first book — "L'Écume des jours" by Boris Vian.

**Rising Action:** He picks up the scanner. Scans the ISBN barcode on the back cover — *beep*. The scan field detects `978-2-...`, identifies it as an ISBN, and creates a title entry. A spinner shows "fetching metadata from BnF..." while the title card appears. He doesn't wait — he scans the label V0001. *Beep* — a different, confirming tone. The volume is created and attached to the title. Status: "not shelved." He puts the book aside and picks up the next one.

By the time he scans the third book's ISBN, the first title's metadata has arrived: author "Boris Vian", cover image, publisher "Gallimard", 1947. He glances at it — correct. He continues scanning.

Book #8 is a second copy of "L'Étranger" — he already cataloged it last weekend. He scans the ISBN. Instead of the usual *beep*, a distinct alert tone plays and the title card appears highlighted in a different color: "Title already exists — L'Étranger, Albert Camus. 1 volume: V0034, shelved." Guy decides this is a genuine second copy. He clicks "New volume," scans label V0008, and the second volume is attached. If it had been an error, he simply would have picked up the next book.

Book #12 is a CD — Thelonious Monk. The UPC doesn't start with 978. The system asks: "Unrecognized code — is this a CD/DVD UPC?" He confirms "CD." MusicBrainz lookup starts. He scans label V0012, moves on.

Book #23 has no barcode — an old edition. He clicks "Add title without ISBN," types the title and author manually, scans label V0023. The rhythm barely breaks.

**Climax:** 45 minutes in, Guy has cataloged 48 items. The dynamic feedback list shows 3 items in orange — metadata fetch failed (an obscure Swiss publisher, a regional CD). Everything else resolved automatically. He'll fix the 3 errors later. The counter shows 48 titles, 50 volumes. He feels something he hasn't felt before: *this is actually going to work.*

**Resolution:** Guy moves to phase two. He carries the stack to the living room bookcase. He scans V0001, then scans the QR code on the shelf (L0003 — "Salon, Bibliothèque principale, Étagère 3"). *Beep*. V0001 is now shelved. He scans V0002, scans L0003 again. Repeat. The 50 books are shelved in 40 minutes. He opens the home page — 48 titles, 50 volumes, all located. The living room bookcase shows its full contents when he clicks L0003. Guy grabs the next stack.

**Requirements revealed:** Barcode scan input with auto-detection, async metadata queue, dynamic feedback list with persistent errors, manual entry fallback, two-phase workflow (catalog then shelve), volume counter, location content view, distinct feedback when scanning existing ISBN, explicit "New volume" action for duplicates.

### Journey 1b: Guy Fixes Metadata Errors — The Monday Evening Cleanup

**Persona:** Same Guy, Monday evening after the Saturday cataloging marathon. 3 titles have orange error indicators in the dynamic feedback list. He's also noticed that a title fetched the wrong cover image (a different edition of the same book).

**Opening Scene:** Guy logs in as Librarian and opens the home page. The dashboard shows "3 titles with metadata issues" as a clickable indicator. He clicks it.

**Rising Action:** The first title is "Traité de la ponctuation française" — an obscure Swiss publisher. The metadata fetch returned nothing from any API. Guy clicks into the title, sees empty fields. He types the metadata manually: author, publisher, year. No cover image available — he leaves it blank. He saves. The error indicator clears.

The second title came back with partial metadata — title and author correct, but no cover image. BnF had the record but no cover. Google Books had a cover but for a different edition. Guy clicks "Re-download metadata" and selects the Google Books result. The system warns: "Author field was manually edited. Overwrite? [Yes for each field / No]." Guy selects "No" for author (his version is more complete) and "Yes" for cover image. The correct cover appears.

The third title has completely wrong metadata — the ISBN matched a different book in Open Library's database (a known data quality issue). Guy clears all auto-filled fields and enters the correct information manually.

**Climax:** All 3 issues resolved in 8 minutes. The dashboard no longer shows any warnings.

**Resolution:** Guy learned that ~6% of his items need manual attention. That's within the 85% metadata hit rate target. The correction workflow is fast and doesn't require re-scanning anything — just editing the title page directly.

**Requirements revealed:** Error tracking and dashboard indicators for failed metadata, manual metadata editing, selective re-download with per-field overwrite confirmation, ability to clear and replace auto-filled data, error resolution workflow.

### Journey 2: Guy at the Bookstore — "Do I Already Own This?"

**Persona:** Same Guy, now standing in a bookstore on a Wednesday afternoon. He's holding a copy of "Le Désert des Tartares" by Dino Buzzati. He's pretty sure he owns it, but he's bought duplicates before — three times for "L'Étranger" alone.

**Opening Scene:** Guy pulls out his phone. He's connected to his home network via Tailscale. He opens mybibli in the browser — no login needed, anonymous access is enough for searching.

**Rising Action:** He types "desert tartares" in the search bar. As he types, results filter dynamically. After "desert t" — one result appears: "Le Désert des Tartares — Dino Buzzati." He taps it.

**Climax:** The title page shows: 1 volume (V0142), condition "Bon état", location "Bureau, Bibliothèque 2, Étagère 1." Status: shelved. He owns it. He puts the book back on the store shelf.

He browses the BD section. Sees Blacksad tome 6. He searches "blacksad" — the series page shows tomes 1-5 owned, tome 6 flagged as gap. He buys it.

**Resolution:** Guy leaves the store with one purchase instead of three. No duplicates. The Blacksad gap is filled. Total time: 30 seconds per lookup.

**Requirements revealed:** Anonymous read access, as-you-type search, responsive UI on mobile browser, series gap visibility, volume location display, Tailscale compatibility (standard HTTP, no special ports).

### Journey 3: Marie Wants to Watch a Movie

**Persona:** Marie, Guy's partner. She doesn't catalog anything and has never logged in. She knows mybibli exists because Guy won't stop talking about it. She wants to find a specific DVD for movie night.

**Opening Scene:** Saturday evening. Marie remembers they own "Le Fabuleux Destin d'Amélie Poulain" somewhere. The DVD shelves span two rooms and a storage box in the basement. She opens mybibli on her tablet.

**Rising Action:** No login screen — she sees the home page with a prominent search bar. She types "amelie poulain." One result. She taps it.

**Climax:** Title page shows: "Le Fabuleux Destin d'Amélie Poulain", director Jean-Pierre Jeunet, 2001. One volume: V0891, location "Salon, Meuble TV, Tiroir 2." Status: shelved. She walks to the TV cabinet, opens drawer 2, finds the DVD.

**Alternative path:** Marie searches for "Intouchables" next. The title page shows one volume: V1203. Status: "On loan." She doesn't see who borrowed it (anonymous access), but she knows it's not available. She tells Guy, who checks on his Librarian account and reminds their neighbor to return it.

**Resolution:** Marie found one DVD in under a minute and learned the other was lent out — without asking Guy and without digging through shelves. She doesn't need to understand how the system works. Search, find, done.

**Requirements revealed:** Zero-friction anonymous access, search-first interface, clear location display, loan status visible without login (without borrower name), non-technical-user friendly, tablet-compatible layout, clear distinction between "shelved" and "on loan" status.

### Journey 4: Guy Lends and Recovers a Book

**Persona:** Guy again. His friend Pierre asks to borrow "Dune" by Frank Herbert. Guy has two copies — one pristine hardcover and one well-read paperback.

**Opening Scene:** Guy logs in as Librarian. He searches "dune herbert." The title page shows 2 volumes: V0055 (hardcover, "Neuf", Étagère 4) and V0056 (paperback, "Bon état", Étagère 4). He decides to lend the paperback.

**Rising Action:** He clicks "Lend" on V0056. The system asks for the borrower. He starts typing "Pierre" — the autocomplete suggests matching borrowers from the existing list. No match — Pierre is new. Guy clicks "New borrower" and fills in Pierre's name, phone, and email. He confirms the loan. V0056's status changes to "On loan — Pierre Moreau — since March 26, 2026."

**Three months later.** Guy opens the home page. The librarian dashboard shows a red tag: "2 overdue loans." He clicks it. V0056 — Dune — lent to Pierre Moreau, 92 days ago. He opens Pierre's borrower page: one active loan. He sends Pierre a text (outside mybibli) reminding him.

**Climax:** Pierre returns the book a week later. Guy opens the Loans page, scans V0056's label with the scanner. The system highlights the matching row in the loan list. He clicks "Return." The loan is cleared. V0056's status returns to "shelved" — the system restores the previous location (Étagère 4) automatically.

**Second loan — a month later.** Guy's colleague Sophie wants to borrow "Sapiens." He clicks "Lend" on the volume, types "Soph" — the autocomplete immediately suggests "Sophie Laurent" from a previous loan. He selects her — no need to re-enter her contact details.

**Resolution:** No book lost. The overdue indicator caught it before Guy forgot entirely. Pierre's borrower page now shows zero active loans. The autocomplete made the second loan even faster. The entire workflow — lend, track, remind, recover — works without any external tool.

**Requirements revealed:** Loan creation workflow, borrower CRUD with autocomplete search, overdue threshold highlighting, loans page with scan-to-find, return workflow, volume status transitions (shelved → on loan → shelved), location restoration on loan return, borrower details visible only to Librarian.

### Journey 5: Guy Sets Up mybibli for the First Time

**Persona:** Guy, the day he decides to install mybibli. He has Docker running on his Synology DS920+. MariaDB is already installed (used by other apps). He's comfortable with docker-compose but not a software developer.

**Opening Scene:** Guy reads the README on GitHub. He copies the docker-compose.yml example, fills in his MariaDB credentials and a few environment variables (MYBIBLI_DB_HOST, MYBIBLI_DB_PASSWORD, API keys for Google Books and TMDb). He maps two Docker volumes: one for cover images (`./mybibli-covers:/data/covers`) and one for any local config. He adds the cover images volume to his Hyper Backup task so it's included in the NAS backup alongside MariaDB. He runs `docker-compose up -d`.

**Rising Action:** The container starts in under 10 seconds — including automatic database table creation on first launch. He opens `http://nas-ip:8080` in his browser. First launch: the system presents a setup screen: "Create your admin account." He enters a username and password.

He's now logged in as Admin. The dashboard is empty — zero titles, zero volumes. He navigates to Settings:
- **Storage locations:** He creates his hierarchy: Maison → Salon → Bibliothèque principale → Étagères 1-5. Then Bureau → Bibliothèque 2 → Étagères 1-3. Then Cave → Cartons 1-4. He prints QR code labels for each location using gLabel.
- **Genres:** The default list (Roman, SF, Thriller, BD, Classique, Jazz, Rock, Action, Comédie...) is pre-loaded. He adds "Philosophie" and "Reportage."
- **Volume states:** Default list (Neuf, Bon état, Acceptable, Usé, Endommagé, Hors service). "Hors service" has the "not loanable" flag checked. He keeps the defaults.
- **Contributor roles:** Default list (Auteur, Illustrateur, Réalisateur, Compositeur, Interprète, Scénariste, Coloriste). He adds "Traducteur."

He creates a Librarian account for Marie (even though she'll mostly browse anonymously, she might want to register a loan someday).

**Climax:** Setup complete. He checks the system health page: mybibli v1.0.0, MariaDB 10.11, disk usage 0 MB, all APIs green. He grabs his scanner and scans his first ISBN. *Beep*. "L'Étranger — Albert Camus" appears with cover art. He grins.

**Resolution:** Total setup time: 22 minutes from docker-compose to first successful scan. The system is ready for the Saturday marathon. MariaDB is backed up by the NAS. Cover images are backed up via Hyper Backup.

**Requirements revealed:** Docker deployment with environment variables, configurable cover image volume, automatic DB schema creation on first launch, admin account setup wizard, pre-loaded default reference data (genres, states, roles), configuration CRUD for all reference data, system health page, Librarian account creation, QR/barcode generation for locations, startup time under 10 seconds.

### Journey 6: Guy Organizes His Tintin Collection — Series Management

**Persona:** Guy, two weekends into his cataloging project. He's been cataloging his BD collection and has scanned 18 Tintin albums so far. He knows the complete series is 24 albums. Time to organize this as a series.

**Opening Scene:** Guy opens the title page for "Tintin au Tibet" — one of the 18 he's already cataloged. He notices there's no series attached. He clicks "Add to series."

**Rising Action:** No existing series matches "Tintin." Guy clicks "Create series": name "Les Aventures de Tintin", type "Closed", total volumes: 24. He assigns "Tintin au Tibet" as volume #20 in the series.

Now he needs to add the other 17 cataloged Tintin albums to the series. He opens the series page. He searches for "tintin" — all 18 titles appear. He assigns each to its position in the series: "Tintin au pays des Soviets" = #1, "Tintin au Congo" = #2, etc. The series page updates: 18 of 24 volumes present, 6 gaps highlighted.

**Climax:** The series view shows a clear visual: volumes 1-24 listed, owned volumes in green, gaps in red: #4 (Les Cigares du Pharaon), #9 (Le Crabe aux pinces d'or), #11 (Le Secret de la Licorne), #14 (Le Temple du Soleil), #17 (On a marché sur la Lune), #22 (Vol 714 pour Sydney). Guy takes a photo of the screen with his phone for his next trip to the bookstore.

He also creates the series "Blacksad" as an open series (ongoing, last known: #6) and assigns his 5 volumes.

**Resolution:** For the first time, Guy can see exactly what's missing from his collections. No more guessing in the store. When he buys "Les Cigares du Pharaon" next month, he'll scan the ISBN — it will create a new title. He assigns it to the Tintin series at position #4. The gap closes. 5 remaining.

**Requirements revealed:** Series CRUD (create, name, type open/closed, total count), title-to-series assignment with position number, series view with gap visualization, search-and-assign workflow for bulk series organization, gap count on dashboard.

### Journey Requirements Summary

| Capability Area | Revealed By Journeys |
|----------------|---------------------|
| Barcode scan input with auto-detection | 1, 2, 4 |
| Async metadata queue with feedback | 1, 1b |
| Manual entry fallback | 1 |
| Two-phase workflow (catalog + shelve) | 1 |
| Distinct feedback for existing ISBN | 1 |
| Explicit "New volume" for duplicates | 1 |
| Error tracking and metadata correction | 1b |
| Selective re-download with per-field confirmation | 1b |
| As-you-type search | 2, 3 |
| Anonymous read access | 2, 3 |
| Responsive UI (mobile/tablet) | 2, 3 |
| Series gap detection and visualization | 2, 6 |
| Series CRUD and title assignment | 6 |
| Loan management (create, return, overdue) | 4 |
| Borrower CRUD with autocomplete search | 4 |
| Loans page with scan-to-find | 4 |
| Location restoration on loan return | 4 |
| Docker deployment + first-launch wizard | 5 |
| Reference data configuration (genres, states, roles) | 5 |
| Storage hierarchy CRUD | 5 |
| System health page | 5 |
| Cover image Docker volume configuration | 5 |
| Cross-navigation (title ↔ contributor ↔ series ↔ location) | 1, 2, 3, 4, 6 |
| Dynamic feedback list (scan results) | 1 |
| Audio feedback | 1 |
| Contextual help | 3, 5 |
| Light/dark mode | All |
| Error handling and recovery | 1, 1b |
| Single-page workflow (no page change during scan sessions) | 1 |
| Startup time under 10 seconds | 5 |

## Web Application Specific Requirements

### Project-Type Overview

mybibli is a multi-page application (MPA) using server-rendered HTML enhanced with HTMX for dynamic updates. No single-page application framework, no WebAssembly, no WebSocket. The architecture prioritizes simplicity and minimal client-side complexity.

### Browser Support

| Browser | Minimum Version | Priority |
|---------|----------------|----------|
| Chrome | Last 2 major versions | Primary |
| Firefox | Last 2 major versions | Primary |
| Safari | Last 2 major versions | Primary (iOS/macOS tablets) |
| Edge | Last 2 major versions | Secondary |

No Internet Explorer support. No legacy browser polyfills.

### Responsive Design

- **Primary target:** Desktop/laptop (1024px+) — cataloging workflow with barcode scanner
- **Secondary target:** Tablet (768px+) — shelving workflow, browsing, loan management. Minimum touch target size: 44x44px for all interactive elements
- **Mobile (< 768px):** Functional but not optimized — search and browse via Tailscale works, no scanner workflow expected
- **No dedicated mobile layout** — responsive CSS breakpoints sufficient

### Performance Targets

> **Note:** Authoritative performance targets are defined in [Non-Functional Requirements > Performance](#performance) (NFR1–NFR8). This table provides web-specific context.

| Metric | Target | Context | NFR |
|--------|--------|---------|-----|
| Initial page load | < 1 second | Local network, minimal assets | NFR5 |
| Search response (as-you-type) | < 500 ms | 10,000 items, MariaDB FULLTEXT | NFR1 |
| Prefix detection | Immediate | Client-side JavaScript, no server round-trip | NFR2 |
| Server response (scan action) | < 500 ms | Title creation, queue metadata, volume attachment | NFR3 |
| Metadata fetch (background) | < 5 seconds | External API, async, non-blocking | NFR6 |
| Page navigation | < 500 ms | Server-rendered, minimal JS | NFR4 |
| Cover image loading | Lazy loaded | Progressive display with placeholder (fixed-size grey rectangle with media-type icon to prevent layout shift) | — |

### SEO Strategy

Not applicable. mybibli is a private application accessed on local network or via VPN. No public-facing pages, no search engine indexing, no sitemap, no meta tags optimization required. `robots.txt` disallows all crawlers as a precaution.

### Accessibility

- Basic accessibility without formal WCAG compliance in v1
- Sufficient color contrast in both light and dark modes
- Keyboard navigation support (Tab, Enter, Escape) — essential for scanner workflow
- ARIA attributes on interactive elements (buttons, form fields, modals)
- Semantic HTML (proper heading hierarchy, form labels, table structure)
- No screen reader optimization in v1

### Implementation Considerations

- **HTMX** for dynamic page updates (search results, scan feedback, form submissions) without full page reloads
- **Server-rendered templates** — Askama or Tera (decision deferred to architecture phase: Askama offers compile-time type safety, Tera offers runtime flexibility)
- **Minimal JavaScript** — vanilla JS only for: scan field autofocus, audio feedback, dark mode toggle, prefix detection before HTMX submission
- **CSS custom properties** for theming (light/dark mode)
- **No build pipeline** for frontend assets — no webpack, no bundler, no npm. Static CSS and JS files served directly by Axum via `tower-http::services::ServeDir` with appropriate `Cache-Control` headers
- **Content Security Policy** headers to prevent XSS. Note: `img-src` directive must allow external domains (Google Books, Open Library, etc.) if cover images are displayed directly from API sources before local download completes. Final CSP policy defined at architecture phase
- **Image placeholders** — fixed-size placeholder with media-type icon displayed while cover images lazy-load, preventing layout shift on search results and catalog pages

## Project Scoping & Phased Development

### MVP Strategy & Philosophy

**MVP Approach:** Problem-solving MVP — the minimum that makes a physical media collection manageable. The creator must be able to catalog, find, lend, and track series gaps from v1, or the tool provides no advantage over the status quo (spreadsheets and memory).

**Resource Requirements:** 1 developer (intermediate Rust) + Claude Code (AI-assisted development). No external team. No deadline pressure — the creator is also the primary user, so progress is visible and valuable at every increment.

**Development Model:** AI-assisted development with Claude Code significantly reduces implementation time for CRUD operations, API integrations, boilerplate, and tests. The bottleneck shifts from coding speed to specification clarity — this PRD and the architecture document are the critical inputs for productive Claude Code sessions.

### CLAUDE.md Foundation Rules

The following rules must be established in the project's CLAUDE.md before development begins:

- **DRY (Don't Repeat Yourself):** No duplicated code. Create functions, methods, or library modules for any logic used more than once.
- **Unit tests:** All functions must have unit tests when possible. Tests are written alongside the implementation, not as an afterthought.
- **E2E tests:** All features must have end-to-end tests using Playwright. Each milestone's validation criteria translate directly into Playwright test scenarios. Note: Playwright runs on Node.js — the test project requires a Node.js setup alongside the Rust project. Barcode scanner input is simulated by sending text to the scan input field.
- **Code language:** All code, comments, variable names, function names, and commit messages in English (open-source project).
- **Code consistency:** Maintain architecture document and coding conventions as reference for all Claude Code sessions to prevent pattern drift across sessions.
- **Gate rule:** No milestone transition until ALL tests (unit + e2e) are green. A milestone is considered complete only when its full Playwright test suite passes.
- **Retrospectives:** Mandatory at the end of each milestone/epic. Never postponed or skipped.
- **Pre-retrospective testing:** Run the complete test suite (all milestones, unit + e2e) before each retrospective. The test results feed into the retrospective discussion.

### Project Conventions

- **Versioning:** Semantic versioning (semver) — MAJOR.MINOR.PATCH
- **License:** GPL v3 — all derivative works must remain open source
- **API keys (FR75):** Configured via environment variables only in v1 (container restart required to apply changes). UI-based key configuration may be added post-MVP.

### MVP Delivery Milestones

> **Note:** This section orders MVP capabilities into delivery increments. For the complete capability list by domain, see [Product Scope > MVP](#mvp---minimum-viable-product).

Each milestone is an independently testable and deployable increment. A milestone is complete when all its Playwright e2e tests and unit tests pass.

**Milestone 1 — "I can catalog a book"**
- Project skeleton: Git repo, CLAUDE.md, Cargo.toml, Dockerfile, docker-compose.yml, project structure, CI basics (cargo test + cargo clippy)
- Docker + MariaDB + Axum skeleton with SQLx migrations
- Core data model (title, volume, contributor with roles)
- Scan ISBN → Open Library/Google Books lookup → display result
- Scan label (V prefix) → create volume and attach to title
- Basic as-you-type search (title, subtitle, description, contributor)
- Admin auth only (single role initially)
- Autofocus on scan field after every server response (critical for scan rhythm)
- *Validation (Playwright): catalog 50 books with simulated barcode input, find them all via search*

**Milestone 2 — "I know where my books are"**
- Storage location hierarchy (CRUD, variable depth, configurable node types)
- QR code / barcode location labels (L0001–L9999)
- Shelving workflow (scan volume → scan location)
- "Not shelved" / "shelved" status
- Location content view (sortable by title/author/genre)
- *Validation (Playwright): shelve all books, find a book by location, view shelf contents*

**Milestone 3 — "All my media types are handled"**
- BnF integration (French books)
- BDGest + Comic Vine (comics/BD)
- MusicBrainz (CDs via UPC)
- TMDb/OMDb (DVDs via UPC)
- Multi-API fallback chain with async metadata queue
- Type-adaptive form (fields shown/hidden per media type)
- ISSN support for magazines
- Dynamic scan feedback list (success fades, errors persist with clickable details)
- Distinct feedback for existing ISBN (opens title page, "New volume" button)
- *Validation (Playwright): catalog 50 mixed items (French books, BD, CDs, DVDs), verify metadata hit rate > 85%*

**Milestone 4 — "I manage my loans"**
- Borrower entity (CRUD with full contact details)
- Borrower autocomplete search
- Loan creation and return workflows
- Dedicated loans page with scan-to-find
- Location restoration on loan return
- Overdue highlighting with configurable threshold
- *Validation (Playwright): lend 5 volumes, verify overdue indicators, process returns, confirm location restored*

**Milestone 5 — "My series are tracked"**
- Series CRUD (open/closed, total count)
- Title-to-series assignment with position number
- Series view with gap visualization (owned in green, gaps in red)
- BD omnibus as special volume in series
- Series gap count on dashboard
- *Validation (Playwright): create Tintin series (24 volumes), assign 18, verify 6 gaps detected*

**Milestone 6 — "Ready for others"**
- Role-based access (Anonymous / Librarian / Admin)
- Anonymous read without login (borrower name hidden, "on loan" visible)
- Admin configuration CRUD (genres, volume states, contributor roles, location node types)
- Pre-loaded default reference data
- First-launch setup wizard (create admin account)
- System health page (mybibli version, MariaDB version, disk usage, entity counts, API status)
- Light/dark mode with toggle + prefers-color-scheme
- Optional audio feedback (configurable)
- Contextual help (tooltips, ? icons with help bubbles, placeholder text in fields)
- Keyboard shortcuts (Enter, Escape, Tab)
- i18n French/English
- Re-download metadata with per-field overwrite confirmation
- Cover image resizing, storage, and placeholder display
- *Validation (Playwright): fresh install from scratch, full workflow test across all roles, all previous milestone tests still green*

### Post-MVP and Vision

> **Note:** For the full Growth and Vision feature lists, see [Product Scope > Growth Features](#growth-features-post-mvp) and [Product Scope > Vision](#vision-future).

### Risk Mitigation Strategy

**Technical Risks:**

| Risk | Severity | Mitigation |
|------|----------|------------|
| Metadata API coverage gaps | High | Validate with 50 real items before heavy development. BnF + BDGest for French content. Manual entry always available |
| API rate limits | Medium | Implement one API at a time. Respect limits. Cache results. Queue async fetches |
| API provider changes or disappears | Low | Architecture modulaire: each metadata provider is an interchangeable module. Fallback chain degrades gracefully. Manual entry as ultimate fallback |
| MariaDB Synology compatibility | Medium | Test early with MariaDB 10.x. Document minimum version. Offer containerized MariaDB alternative |
| Concurrent access conflicts | Low | Optimistic locking on DB records. UI feedback on conflicts |

**Resource Risks:**

| Risk | Severity | Mitigation |
|------|----------|------------|
| Scope ambitious for solo developer | Medium | AI-assisted development with Claude Code. Each milestone independently useful. No external deadline |
| Code consistency across Claude Code sessions | Medium | CLAUDE.md with DRY rules, unit test requirement, and coding conventions. Architecture document as reference |
| Developer burnout / loss of motivation | Low | Each milestone delivers visible value. Creator is primary user — daily motivation from own usage |

**Market Risks:**

| Risk | Severity | Mitigation |
|------|----------|------------|
| No community adoption | Low | Primary user validates product-market fit by daily usage. Community adoption is aspirational, not required |
| Competing tool appears | Low | No current competitor in this niche. Open source community loyalty. First-mover advantage |

## Functional Requirements

### Cataloging & Barcode Input

- FR1: Librarian can scan a barcode (ISBN, UPC, ISSN) via USB barcode scanner into a single input field
- FR2: System can auto-detect the type of scanned code by prefix (978/979 → ISBN, 977 → ISSN, V → volume, L → location, other → UPC/unknown)
- FR3: System can create a new title from a scanned ISBN/UPC/ISSN and queue asynchronous metadata retrieval
- FR4: Librarian can scan a volume label to create a physical volume and attach it to the current title
- FR5: System can validate volume label uniqueness at scan time and reject duplicates
- FR6: When scanning an ISBN already in the database, system can open the existing title page instead of creating a duplicate
- FR7: Librarian can explicitly add a new physical volume to an existing title ("New volume" action)
- FR8: Librarian can create a title manually without a barcode (no-ISBN path)
- FR9: Librarian can specify the media type when a scanned code is not auto-detected (e.g., confirm UPC is a CD)
- FR10: System can maintain autofocus on the scan input field after every server interaction
- FR90: System can display volume count and status summary on the title detail page (e.g., "3 volumes: 2 shelved, 1 on loan")
- FR92: Librarian can assign a media type to a title
- FR101: Librarian can assign a genre to a title from the configurable genre list
- FR93: System can adapt title form fields based on the assigned media type
- FR94: Librarian can set and edit the language of a title (pre-filled by metadata API)

### Metadata Retrieval

- FR11: System can retrieve title metadata from multiple external APIs (Open Library, Google Books, BnF, BDGest, Comic Vine, MusicBrainz, TMDb, OMDb)
- FR12: System can execute a fallback chain across metadata providers when the primary source returns no result
- FR13: System can fetch metadata asynchronously in a background queue while the user continues scanning
- FR14: System can retrieve and store cover images from metadata providers
- FR15: System can resize cover images to a maximum width for efficient storage
- FR16: Librarian can re-download metadata for a title on demand
- FR17: System can detect manually edited fields and prompt for per-field confirmation before overwriting during re-download
- FR18: Librarian can manually edit all metadata fields on a title
- FR19: System can skip metadata providers whose API keys are not configured

### Search & Navigation

- FR20: Any user can search titles as-you-type across title, subtitle, description, and contributor name
- FR21: Any user can filter search results by genre and volume state
- FR22: Any user can navigate between linked entities (title → volumes, contributor → titles, series → volumes, location → contents)
- FR23: Any user can paginate through result lists using classic pagination
- FR24: Any user can view the contents of a storage location sorted by title, author, or genre
- FR96: Any user can search for a volume by its label identifier (e.g., V0042) in the global search

### Physical Volume Management

- FR25: Librarian can assign a storage location to a volume by scanning the location label
- FR26: System can track volume status (not shelved, shelved, on loan)
- FR27: System can display the current location path for each volume (e.g., "Salon → Bibliothèque 1 → Étagère 3")
- FR28: Librarian can set a volume's condition/state from a configurable list
- FR29: Librarian can add an edition comment to a volume (pocket, hardcover, collector, etc.)
- FR30: System can validate and register volume identifiers (V0001–V9999) scanned from pre-printed labels
- FR31: System can validate and register location identifiers (L0001–L9999) scanned from pre-printed labels

### Storage Location Management

- FR32: Admin can create, edit, and delete storage locations in a tree hierarchy of variable depth
- FR33: Admin can configure location node types (room, bookcase, shelf, box, etc.)
- FR34: System can prevent deletion of locations that contain volumes
- FR35: System can assign a "not shelved" status to volumes without a location

### Series Management

- FR36: Librarian can create a series (name, type open/closed, total volume count for closed series)
- FR37: Librarian can assign a title to a series with a position number
- FR38: System can detect and display missing volumes in a series (gap detection)
- FR39: System can display a series overview with owned volumes and gaps visually distinguished
- FR40: Librarian can register a BD omnibus as a special volume covering multiple positions in a series
- FR95: Any user can view a list of all series with their completion status (owned/total, gap count)

### Loan Management

- FR41: Librarian can create a borrower with full contact details (name, address, email, phone)
- FR42: Librarian can search borrowers with autocomplete
- FR43: Librarian can record a loan (associate a volume with a borrower and date)
- FR44: System can prevent loaning a volume whose state is flagged as not loanable
- FR45: Librarian can record a loan return and restore the volume's previous storage location
- FR46: System can display all current loans on a dedicated loans page
- FR47: Librarian can scan a volume label on the loans page to find and highlight that loan
- FR48: System can calculate loan duration and highlight overdue loans based on a configurable threshold
- FR49: System can prevent deletion of a volume that is currently on loan
- FR50: System can prevent deletion of a borrower with active loans
- FR89: Librarian can view all active loans for a specific borrower from the borrower's detail page

### Contributor Management

- FR51: System can create and manage contributors as unique entities (one record per person)
- FR52: System can associate contributors with titles via roles (author, director, composer, performer, illustrator, screenwriter, colorist, translator, etc.)
- FR53: System can assign multiple roles to the same contributor on the same title
- FR54: System can prevent deletion of a contributor referenced by any title

### Home Page & Dashboard

- FR55: Any user can view global collection statistics (title count, volume count, loan count)
- FR56: Any user can view recent additions
- FR57: Any user can view collection statistics by genre
- FR58: Librarian can view actionable indicators with counts (unshelved volumes, overdue loans, series with gaps, recent cataloged, recent returns)
- FR59: Any user can see loan status on volume details ("on loan" without borrower name for anonymous, full details for Librarian/Admin)

### Scan Feedback & Error Handling

- FR60: System can display a dynamic scan feedback list showing recent scan results
- FR61: System can auto-dismiss successful scan entries after a configurable delay
- FR62: System can persist error entries in the feedback list with clickable error details
- FR63: System can play configurable audio feedback for distinct scan outcomes (title found, volume created, error, existing ISBN)
- FR64: Dashboard can display a count of titles with unresolved metadata errors

### Access Control & User Management

- FR65: Any user can browse, search, and view the catalog without authentication
- FR66: Librarian can authenticate to access cataloging, loan, and editing capabilities
- FR67: Admin can authenticate to access system configuration and user management
- FR68: Admin can create, edit, and deactivate user accounts with role assignment (Librarian, Admin)
- FR69: System can maintain user sessions without automatic timeout (session expires on browser close)

### Configuration & Administration

- FR70: Admin can configure the list of genres
- FR71: Admin can configure volume states with a loanable/not-loanable flag per state
- FR72: Admin can configure contributor roles
- FR73: Admin can configure storage location node types
- FR74: Admin can configure the overdue loan threshold (in days)
- FR75: Admin can configure API keys for metadata providers
- FR76: System can display a health page showing application version, MariaDB version, disk usage, entity counts, and API provider status

### Internationalization & Theming

- FR77: Any user can switch the UI language between French and English
- FR78: Any user can toggle between light and dark display modes
- FR79: System can detect the user's system preference for color scheme and apply it by default

### Data Protection

- FR80: System can prevent deletion of any entity referenced by another entity
- FR81: System can preserve a title when its last physical volume is deleted
- FR82: System can enforce optimistic locking to prevent concurrent edit conflicts

### Contextual Help & Usability

- FR83: System can display contextual help on form fields and interactive elements (tooltips, help icons, placeholder text)
- FR84: System can support keyboard shortcuts for common actions during scan workflows (submit, cancel, navigate)
- FR85: System can operate in fully manual mode when no metadata API keys are configured
- FR102: System can complete the scan-to-catalog and scan-to-shelve workflows without page navigation (single-page workflow during scan sessions)
- FR88: System can display a fixed-size placeholder with media-type icon while cover images are loading

### First Launch & Setup

- FR86: System can automatically create the database schema on first launch
- FR87: System can present a first-launch setup wizard to create the initial admin account
- FR91: System can initialize default reference data (genres, volume states, contributor roles) on first launch

### Entity Editing & Reference Data Protection

- FR97: Librarian can edit contributor details (name, biography)
- FR98: Librarian can edit borrower contact details
- FR99: Librarian can edit series details (name, type, total count)
- FR100: System can prevent deletion of a genre, volume state, or contributor role that is currently assigned to any title or volume

## Non-Functional Requirements

### Performance

- NFR1: As-you-type search must return results within 500 ms with 10,000 titles in the database
- NFR2: Scan input prefix detection must be immediate (client-side, no server round-trip)
- NFR3: Server response to a scan action (title creation, volume attachment) must complete within 500 ms
- NFR4: Page navigation between views must complete within 500 ms
- NFR5: Initial page load must complete within 1 second on local network
- NFR6: Background metadata fetch must complete within 5 seconds per API source
- NFR7: Container startup (docker start to HTTP 200) must complete within 10 seconds
- NFR8: The system must support 3–4 concurrent users performing different operations without exceeding the defined response time targets (NFR1–NFR6)

### Security

- NFR9: User passwords must be hashed using Argon2 before storage
- NFR10: Session tokens must be cryptographically random (minimum 256-bit), transmitted via HttpOnly, SameSite=Strict cookies
- NFR11: Anonymous users must not be able to access borrower personal data (name, address, email, phone)
- NFR12: All write operations (create, update, delete) must require Librarian or Admin authentication
- NFR13: Admin-only operations (user management, system configuration) must be inaccessible to Librarian role
- NFR14: API keys must be stored as environment variables, never in the database or application code
- NFR15: Content Security Policy headers must be set to prevent XSS attacks. Specific CSP directives (including img-src for external cover image sources) to be defined in the architecture document

### Integration

- NFR16: Each metadata provider must be implemented as an independent, interchangeable module
- NFR17: The metadata fallback chain must continue to the next provider when one fails or times out
- NFR18: API rate limits must be respected (Google Books: 1,000/day, MusicBrainz: 1 req/sec, others per documentation)
- NFR19: The system must remain fully functional when all external APIs are unavailable (manual entry mode)
- NFR20: Failed metadata fetches must be logged and surfaced to the user without blocking the cataloging workflow

### Reliability

- NFR21: All data committed to MariaDB and cover images written to the filesystem volume must be durable across application restarts and Docker container recreation
- NFR22: Optimistic locking must prevent silent data overwrites when two users edit the same record concurrently
- NFR23: Database schema migrations must be applied automatically on application startup. All existing data must remain accessible and unmodified after migration
- NFR24: Cover image storage path must be configurable via Docker volume mapping for backup integration
- NFR25: The application must reconnect to MariaDB within 30 seconds of the database becoming available again, using exponential backoff with a maximum of 5 retries, without requiring an application restart

### Maintainability

- NFR26: All functions must have unit tests (DRY principle — no duplicated code)
- NFR27: All features must have Playwright end-to-end tests
- NFR28: Code, comments, variable names, and commit messages must be in English
- NFR29: The architecture must support adding new metadata providers without modifying existing provider code (open/closed principle)
- NFR30: Database schema changes must be managed through versioned migration files

### Operational & Resource Constraints

- NFR31: The application must log all significant events (startup, API calls, errors, authentication) to stdout in a structured format suitable for Docker log drivers
- NFR32: Docker image size must not exceed 100 MB. Cover image storage must average less than 100 KB per image
- NFR33: Audio feedback must play within 100 ms of the triggering scan event
- NFR34: Total size of static assets (CSS + JS, excluding cover images) must not exceed 500 KB uncompressed
- NFR35: Application runtime memory consumption must not exceed 100 MB under normal operation (3–4 concurrent users, 10,000 titles)
- NFR36: System must cache metadata lookup results for 24 hours to avoid redundant API calls for previously queried ISBN/UPC codes. Cache is invalidated when a user triggers a manual re-download for a specific title
- NFR37: All user data (catalog, loans, borrower details, cover images) must remain on the local network — no telemetry, no cloud sync, no external data transmission beyond metadata API lookups

## Title Fields by Media Type

The title form adapts based on the assigned media type. All types share a common field set; type-specific fields are shown only when relevant.

### Common Fields (all media types)

| Field | Required | Source |
|-------|----------|--------|
| Title | Yes | API / manual |
| Subtitle | No | API / manual |
| Description | No | API / manual |
| Language | Yes | API / manual |
| Genre | Yes | Manual (from configurable list) |
| Media type | Yes | Auto-detected or manual |
| Publication date | No | API / manual |
| Publisher / Label / Studio | No | API / manual |
| ISBN / ISSN / UPC | No | Scanned or manual |
| Cover image | No | API / manual |
| Contributors (with roles) | No | API / manual |
| Series + position | No | Manual |

### Type-Specific Fields

| Field | Book | BD | CD | DVD | Magazine | Report |
|-------|:----:|:--:|:--:|:---:|:--------:|:------:|
| Page count | Yes | Yes | — | — | Yes | Yes |
| Volume number in series | — | Yes | — | — | — | — |
| Track count | — | — | Yes | — | — | — |
| Total duration | — | — | Yes | Yes | — | — |
| Age rating | — | — | — | Yes | — | — |
| Issue number | — | — | — | — | Yes | — |
| ISSN | — | — | — | — | Yes | — |

## Identifier Ceiling Behavior

Volume identifiers (V0001–V9999) and location identifiers (L0001–L9999) are capped at 4 digits in v1.

- **V9999 reached:** System displays an error "Maximum volume identifier reached. Contact admin." The identifier format can be extended to 5 digits (V00001–V99999) in a future version via database migration and gLabel template update.
- **L9999 reached:** Same behavior. In practice, 9,999 locations is unlikely to be reached (typical home: 20–50 locations).
- **Validation:** The system rejects any scanned identifier that does not match the expected format (V + 4 digits, L + 4 digits).

## Application Upgrade Path

Users upgrade mybibli by pulling the latest Docker image and restarting the container:

1. `docker-compose pull`
2. `docker-compose up -d`
3. On startup, mybibli automatically applies any pending database migrations (NFR23)
4. Release notes document breaking changes, migration notes, and new configuration options
5. Semantic versioning (semver) signals the nature of changes: PATCH for fixes, MINOR for features, MAJOR for breaking changes

No manual database intervention required. Cover images and MariaDB data persist across upgrades via Docker volumes.

## Open Questions (Deferred to Architecture Phase)

| Question | Context | Impact |
|----------|---------|--------|
| Askama vs Tera template engine | Askama: compile-time type safety. Tera: runtime flexibility | Frontend rendering approach |
| CSP directive specifics | img-src must allow external cover sources during async fetch | Security headers configuration |
| Metadata fallback chain ordering | Specialized first (BnF, BDGest, MusicBrainz, TMDb) then general (Open Library, Google Books) — exact priority per media type | Metadata quality and speed |
| Cover image resize dimensions | Target max width (e.g., 300–400px), JPEG quality level | Storage and display trade-off |
| Cache storage mechanism | In-memory (lost on restart) vs MariaDB table (persistent) for NFR36 | Performance vs durability |
| Structured logging format | JSON lines? Key-value? Which fields per event type? | Observability |
| Scan feedback auto-dismiss delay | Default value (e.g., 5 seconds), configurable range (1–30s) | UX tuning |

## Glossary

| Term | Definition |
|------|-----------|
| **BD** | Bande dessinée — Franco-Belgian comic books (e.g., Tintin, Astérix, Blacksad) |
| **BnF** | Bibliothèque nationale de France — French national library, provides metadata API at data.bnf.fr |
| **BDGest** | French database specializing in Franco-Belgian comic metadata |
| **Comic Vine** | English-language comic book database (under Fandom) covering Western comics |
| **Douchette** | Handheld USB barcode scanner that acts as a keyboard input device (e.g., Inateck BCST-36) |
| **gLabel** | Open-source label design application for Linux, used to create and print barcode/QR code labels on standard label sheets |
| **ISBN** | International Standard Book Number — 13-digit identifier for books (prefix 978 or 979) |
| **ISSN** | International Standard Serial Number — 8-digit identifier for periodicals/magazines (barcode prefix 977) |
| **UPC** | Universal Product Code — barcode standard used on CDs, DVDs, and other retail products |
| **Omnibus** | A single physical volume collecting multiple issues/volumes of a series (e.g., Tintin tomes 1–3 in one book) |
| **Title** | A work in the catalog (book, album, film, etc.) with its metadata. One title can have multiple physical volumes |
| **Volume** | A physical copy of a title, identified by a unique label (V0001–V9999), with its own condition, location, and loan status |
| **Semver** | Semantic versioning — MAJOR.MINOR.PATCH version numbering scheme |
| **MPA** | Multi-Page Application — web architecture where each view is a separate server-rendered page (as opposed to SPA) |
| **HTMX** | Lightweight JavaScript library for dynamic page updates via HTML attributes, without a full SPA framework |

## Assumptions and Dependencies

### Assumptions

- The user has a Synology (or compatible) NAS with Docker support
- MariaDB 10.x or later is available on the NAS (native package or containerized)
- The user has a local network (LAN) where the NAS is accessible
- Remote access (if desired) is handled externally via Tailscale or similar VPN
- The user prints labels externally using gLabel on a standard printer (Brother or other)
- The barcode scanner (douchette) connects via USB or Bluetooth and acts as a keyboard input device
- The user's collection is primarily in French and/or English
- Internet access is available for metadata API lookups (not required for core functionality)

### Dependencies

| Dependency | Type | Risk |
|------------|------|------|
| MariaDB 10.x+ | Runtime | Medium — version compatibility must be validated early |
| Docker | Runtime | Low — standard on modern NAS devices |
| Open Library API | External service | Low — free, no key required |
| Google Books API | External service | Low — free tier with API key |
| BnF (data.bnf.fr) | External service | Low — government service, stable |
| BDGest | External service | Medium — commercial site, API terms may change |
| Comic Vine API | External service | Medium — under Fandom, uncertain long-term |
| MusicBrainz API | External service | Low — open source, community-maintained |
| TMDb API | External service | Low — free with attribution |
| OMDb API | External service | Low — free tier available |
| gLabel | External tool | Low — user's responsibility, not a runtime dependency |
| Tailscale | External tool | Low — optional, user's responsibility |
| Node.js | Test dependency | Low — required for Playwright e2e tests only, not runtime |

