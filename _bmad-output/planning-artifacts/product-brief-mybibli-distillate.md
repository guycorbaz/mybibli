---
title: "Product Brief Distillate: mybibli"
type: llm-distillate
source: "product-brief-mybibli.md"
created: "2026-03-26"
purpose: "Token-efficient context for downstream PRD creation"
---

# Product Brief Distillate: mybibli

## Competitive Intelligence

- **Calibre/Calibre-Web**: Ebook-only, no physical media, no barcode scanning, no loans, no storage locations. Python-based, heavy Docker images (~500MB). UI dated and complex.
- **Libib**: Cloud-only, free tier capped at 5,000 items. Has ISBN scanning and series grouping but no hierarchical storage, no label generation, no missing volume detection. Data portability concerns.
- **BookBuddy/DVD Buddy/CDpedia (Bruji)**: Apple-only, separate apps per media type, no web access, no Docker, no multi-user.
- **Koha/Evergreen**: Full ILS systems, massively over-engineered for personal use. Steep learning curve, heavy resource requirements, poor UX for home users.
- **Inventaire**: Wikidata-based, books only, complex self-hosting (CouchDB + Elasticsearch), no barcode labels, no storage/loan tracking.
- **Komga/Kavita**: Digital comics only (CBZ/CBR), no physical collection, no ISBN, no loans.
- **Grocy**: General household inventory, Docker-friendly, barcode scanning — but not media-specialized, no ISBN lookup, no series tracking. PHP/SQLite.
- **Delicious Library (macOS)**: Abandoned ~2017, no web, no self-hosting, no multi-user.
- **Key gap confirmed**: No self-hosted tool unifies physical multi-media cataloging + barcode scanning + hierarchical storage + loan tracking + series gap detection.

## Metadata APIs — Detailed Coverage

### Books
- **Open Library** (openlibrary.org/api): Free, no key, ~30M editions, JSON with covers. Gaps in non-English and recent titles. Unofficial rate limit ~100 req/min.
- **Google Books API**: Excellent coverage and search, 1,000 req/day free (10,000 with API key). Returns covers, descriptions, series info. Best overall quality for books.
- **ISBNdb** (isbndb.com): Largest ISBN database (30M+), fast, but paid ($9.99-$29.99/month). Best for bulk/commercial use. Not planned for v1.
- **BnF (data.bnf.fr)**: Essential for French editions. Authoritative source for French-language publications.
- **OpenBD**: Specialized for Japanese books — potentially useful for manga editions.

### Comics/BD
- **BDGest**: Essential for Franco-Belgian comics (Glénat, Dargaud, Dupuis, etc.). Series and volume data.
- **Comic Vine** (comicvine.gamespot.com): ~800K issues of Western comics. Under Fandom, uncertain long-term availability.
- **Metron API**: Newer alternative focused on comics, potentially more stable than Comic Vine.
- **MangaDex/AniList**: Manga series/volume data. Useful for Japanese manga series tracking.

### Music/CDs
- **MusicBrainz**: Free, open, 2M+ releases with UPC/barcode lookup. Rate limit: 1 req/sec with user-agent identification. Excellent for CDs.
- **Note**: CDs use UPC/EAN barcodes, NOT ISBN. The scanner must handle both ISBN-13 (EAN-13) and UPC-A formats.

### Movies/DVDs
- **TMDb**: Free with attribution, 800K+ movies with posters. Best overall coverage.
- **OMDb**: Free tier 1,000 req/day. Good as secondary source.
- **Note**: DVD UPC-to-movie mapping is imprecise — physical editions don't map 1:1 to digital entries. May need title-based search as fallback.

### Coverage Risk
- French-language editions, older CDs, niche comics have weakest coverage across all APIs
- **Validation plan**: Test 50 representative items from the creator's actual collection across all 4 media types before heavy development
- Fallback chain order: specialized API first (BnF for French books, BDGest for BD, MusicBrainz for CDs, TMDb for DVDs) → general APIs (Open Library, Google Books) → manual entry

## Rust Technical Stack — Specifics

### Web Framework
- **Axum** (by Tokio team): Recommended for new projects as of 2025. Better ergonomics than Actix-web, native Tokio integration, growing middleware ecosystem. Tower middleware compatibility.
- **Actix-web**: Still performant and stable but smaller active contributor base. Axum preferred for ecosystem momentum.
- For 3-4 users and 10K items, both are vastly overpowered — Axum chosen for community/ecosystem alignment.

### Database
- **SQLx**: Compile-time checked SQL queries against MariaDB/MySQL, async support. Most common choice for Axum projects.
- **Diesel**: Other major option but weaker async support.
- **SeaORM**: Builds on SQLx with ActiveRecord patterns — possible alternative.
- SQLx selected for compile-time safety and direct Axum ecosystem alignment.

### Frontend
- **HTMX or Alpine.js**: Lightweight interactivity without heavy JS framework. Server-rendered HTML from Rust templates (Askama or Tera).
- Decision between HTMX and Alpine.js deferred to architecture phase.

### Barcode Generation
- **`barcoders` crate**: Supports EAN-13, Code 128, Code 39. Output to SVG, PNG (via image crate).
- **`rxing` crate**: Rust port of ZXing — supports both generation and reading of 1D/2D barcodes including EAN-13, Code 128, QR codes, DataMatrix.
- QR codes needed for storage location labels (confirmed by user).

### Label/PDF Generation
- **Not needed in v1**: Labels printed externally via gLabel on standard label sheets.
- mybibli only needs to manage unique identifiers and generate barcode/QR code images that the user can reference when creating gLabel templates.

### Docker
- Rust static binary (with musl) produces minimal Docker images.
- Target: under 100 MB for the application image (user confirmed 80-100 MB acceptable).
- Cover images stored on filesystem, mounted as Docker volume, included in NAS backup scope.

## User Scenarios — Detailed

### Bulk Cataloging Session (primary workflow)
1. User logs in as Librarian
2. Scans a storage location barcode/QR code (sets current location context)
3. Scans ISBN/UPC of first item with douchette
4. System retrieves metadata + cover → displays for confirmation
5. User confirms or edits → volume created with unique ID, assigned to scanned location
6. Repeat for next item (target: 80+ items/hour)
7. If ISBN not found: manual entry form with same fields

### "Do I Own This?" Check (bookstore scenario)
1. User connects remotely via Tailscale
2. Searches by title, author, or ISBN from phone browser
3. System shows if owned, how many copies, where stored, if lent out
4. No login required (anonymous read access)

### Loan Management
1. Librarian scans volume barcode
2. Records borrower name and date
3. System tracks outstanding loans
4. Dashboard shows all loans at a glance with duration

### Series Gap Detection
1. User defines a series (e.g., "Tintin" with 24 expected volumes)
2. As volumes are cataloged and assigned to the series, system tracks which numbers are present
3. Dashboard shows gaps per series (e.g., "Tintin: missing volumes 5, 12, 18")

## Rejected Ideas and Out-of-Scope Decisions

- **Camera-based barcode scanning in browser**: Explicitly rejected for v1. User wants douchette only. Possible future feature.
- **Direct label printing from app**: Rejected. User uses gLabel on standard label sheets with Brother printer. mybibli manages IDs and generates barcode/QR images only.
- **Mobile native app**: Out of scope. Responsive web UI sufficient.
- **Data import (CSV, Libib, Goodreads)**: Out of scope v1. User has no existing data to import.
- **CSV export**: Removed from v1 scope per user decision.
- **E-book/digital media management**: Out of scope. Physical media only.
- **Wishlist/acquisition workflow**: Out of scope v1.
- **Social features**: Out of scope v1.
- **Plugin architecture**: Removed from vision — premature for v1, would add complexity without justification.
- **20 MB Docker image target**: Rejected as unrealistic. 80-100 MB accepted.

## Access Model — Detailed

- **Anonymous (no login)**: Search, browse catalog, view item details, view loan status, view series/gaps. Anyone on the network (or via Tailscale VPN).
- **Librarian (login required)**: All anonymous capabilities + add/edit titles and volumes, manage loans (create, return), assign storage locations, manage series.
- **Admin (login required)**: All Librarian capabilities + user management (create/edit/delete accounts), system configuration, storage location hierarchy management.
- **Authentication**: Argon2 password hashing, session-based auth. No external identity provider.

## Open Questions for PRD Phase

- **HTMX vs Alpine.js**: Which frontend approach? Decision impacts development patterns significantly.
- **Template engine**: Askama (compile-time, type-safe) vs Tera (runtime, more flexible)?
- **Barcode format for volume labels**: Code 128? EAN-13 with custom prefix? QR code?
- **QR code vs barcode for storage locations**: User mentioned both — need to decide on format.
- **Series data model**: Is a series a user-defined ordered list? How to handle omnibus editions (containing volumes 1-3)? Renumbered series? Variant editions?
- **Metadata conflict resolution**: When multiple APIs return different data for the same ISBN, which takes priority?
- **Cover image storage strategy**: Original resolution vs thumbnails? Max size? Cleanup policy?
- **MariaDB minimum version**: Synology ships various versions depending on DSM. Need to establish minimum (10.x).
- **Concurrency model**: Optimistic locking mentioned but details needed — what happens when two users scan the same ISBN simultaneously?
- **i18n approach**: Which Rust i18n crate? How are translations managed? Community contribution workflow for additional languages?

## Market Opportunity Notes

- Physical media resurgence: vinyl up 20%+ YoY, manga market doubled since 2020, Blu-ray collecting communities growing.
- r/selfhosted (800K+ members), r/datacurator, r/manga, r/vinyl, r/dvdcollection — all potential launch communities.
- Francophone self-hosted community (forum-nas.fr, r/selfhosted_fr, French tech YouTube) underserved by English-only tools.
- Adjacent market: micro-libraries (church, neighborhood, homeschool co-ops) need lightweight cataloging + lending. Minimal additions needed (public catalog view).
- NAS vendors (Synology, QNAP) actively seek compelling consumer use cases — potential for featured placement in app stores.
