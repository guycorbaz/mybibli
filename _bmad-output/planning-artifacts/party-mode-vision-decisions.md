---
title: "Party Mode Vision Decisions: mybibli"
type: decision-log
created: "2026-03-26"
purpose: "Comprehensive capture of all decisions made during PRD vision Party Mode session"
---

# Party Mode Vision Decisions — mybibli

## Classification (revised by team)

- Project Type: Web App
- Domain: Collection Management / Personal Asset Tracking
- Complexity: Medium (multi-API integration, rich data model, multi-media)
- Context: Greenfield

## Product Vision

- **Core insight**: mybibli makes possible what no simple tool can — transform a barcode into a complete record automatically, and find any item among thousands in a few keystrokes
- **Aha moment**: First volume registered in seconds (scan ISBN → scan label) + instant text search (e.g., "tocatta en mi bemol")
- **UX philosophy**: Zero wait, immediate feedback, user is never blocked
- **Key differentiator**: More user-friendly than a spreadsheet — and a spreadsheet can't auto-scan barcodes and fetch metadata

## Data Model Decisions

### Title
- One title = one work (a translated book is a new title with different ISBN)
- DVD boxset = one title
- Fields: title, subtitle, description, language (pre-filled by API, editable), genre, publication date, publisher/label/studio, ISBN/ISSN/UPC, cover image
- Type-specific fields shown/hidden based on media type:
  - Book/Magazine/Report: page count
  - BD: number in series
  - CD: track count, total duration
  - DVD: duration, age rating
- Single form with fields adapting to media type

### Volume (physical copy)
- Unique identifier: V0001 to V9999
- Fields: label (V prefix + 4 digits), condition/state, edition comment (pocket, hardcover, etc.), storage location
- Status "not shelved" for volumes awaiting shelving

### Contributors
- Unified entity: one person = one record (e.g., Clint Eastwood as both director and actor)
- Many-to-many relationship with titles via role
- Roles: author, illustrator, screenwriter, director, composer, performer, editor, colorist...
- A contributor can have multiple roles on the same title
- Unique key: (title_id, contributor_id, role)

### Series
- Populated via API if available, otherwise manual entry
- Open series (ongoing, last known number) and closed series (known total)
- Missing volume = title without physical volume in the series
- BD omnibus = special volume attached to the series (covers volumes X to Y)

### Borrowers
- Full entity: first name, last name, full address, email, mobile phone
- No loan history — only current loans tracked
- Deletion blocked if loans outstanding

### Storage Locations
- Tree structure with variable depth (e.g., room → bookcase → shelf → box)
- Node types configurable
- Unique identifier: L0001 to L9999
- Each location can have a barcode/QR code label
- Special "not shelved" status for unshelved volumes

### Genres
- One genre per title
- Configurable list (admin)

### Volume States/Conditions
- Configurable list with "loanable" flag (checkbox) per state
- Admin defines states and which ones allow lending

### Media Types (v1 fixed list)
- Book, BD (comics), CD, DVD, Magazine, Report
- Extensible in future versions (vinyl, video games...)
- Type determines which metadata API chain to use and which form fields to show

## Workflow Decisions

### Cataloging (at desk)
1. Pre-print labels via gLabel on standard label sheets
2. Stick labels on items
3. Scan ISBN/UPC → title created/found → metadata fetched in background queue
4. Scan label (V0001) → volume created and attached to title → status "not shelved"
5. Continue scanning next items without waiting
6. "Re-download metadata" button on title page for manual refresh
7. If metadata was manually modified, confirm before overwriting on re-download

### Shelving (at shelves, laptop or tablet)
1. Scan volume label (V0001) → displays title and volume info
2. Scan location QR code (L0012) → volume assigned to location → status "shelved"
3. Continue with next volume

### Scanning an existing ISBN
- Opens the existing title page (no automatic duplicate creation)
- User clicks "New volume" explicitly to add another physical copy

### Title without ISBN
- Manual creation path: "Add title without ISBN" → empty form → scan label

### Loan Management
- Dedicated Loans page showing all current loans
- Return via: scan volume label on Loans page → click "Return", or browse borrower's page → click "Return"
- Overdue loans highlighted in red on dashboard (configurable threshold in days)

## Interface Decisions

### Single Intelligent Scan Field
- One input field, auto-detection by prefix:
  - `978`/`979` → ISBN (book)
  - `977` → ISSN (magazine)
  - `V` + digits → volume label
  - `L` + digits → storage location
  - Other → UPC (CD/DVD) or unknown → ask user
- No WASM — HTML + vanilla JS + HTMX
- Autofocus returns to scan field after every server response (critical for scanning rhythm)

### Home Page
- **Anonymous visitor**: search bar (prominent), global stats, recent additions table, storage locations view, stats by genre
- **Librarian/Admin**: same + clickable tags with counters (unshelved volumes, overdue loans, recent cataloged, series with gaps, recent returns)

### Navigation
- Everything clickable and cross-navigable: contributor → their titles, series → its volumes, location → its contents, title → its volumes
- Location content view sortable by title/author/genre

### Feedback
- Dynamic list during cataloging: successful items fade after a few seconds, errors persist with clickable error details
- Optional audio feedback (configurable): different sounds for "title found", "volume created", "error"
- Keyboard shortcuts: Enter to validate, Escape to cancel, Tab to navigate

### Help System
- Tooltips on hover for every field, button, checkbox
- ? icon next to complex fields with detailed help bubble + example
- Placeholder text in empty fields (e.g., "978-2-01-210345-6")
- All help text bilingual (FR/EN) — part of i18n

### Theming
- Light mode + Dark mode (both in v1)
- CSS variables for easy theming, `prefers-color-scheme` + manual toggle

### Pagination
- Classic pagination (not infinite scroll)

## Access Model

- **Anonymous (no login)**: search, browse, view details, see "on loan" (without borrower name)
- **Librarian (login)**: all anonymous + catalog, edit, manage loans (sees borrower names), manage series
- **Admin (login)**: all librarian + user management, system config, storage hierarchy management, genre/state list config

## Technical Decisions

### Sessions
- No automatic timeout — session expires only when browser closes
- Multi-tab/window works natively (no global server-side state)

### Metadata Languages
- UI: bilingual FR/EN (user preference)
- Metadata: language of the original work (French book → French metadata via BnF, English book → English metadata via Google Books)
- Language field on title, pre-filled by API, editable

### API Fallback Chain
- Specialized API first (BnF for French books, BDGest for BD, MusicBrainz for CDs, TMDb for DVDs)
- General APIs second (Open Library, Google Books)
- Manual entry as last resort
- APIs without configured keys are skipped in the chain

### Configuration
- Docker environment variables for secrets (DB connection, API keys)
- All admin-configurable settings stored in MariaDB (genres, volume states, location node types, contributor roles...)
- Backup = NAS MariaDB backup covers everything except cover images
- Cover images: resized to max 400px wide, JPEG, stored on filesystem in configurable Docker volume

### Deletion Rules
- Never delete an entity referenced elsewhere
- Title with volumes → blocked
- Deleting last volume → volume deleted, title persists
- Loaned volume → return first
- Location with volumes → move volumes first
- Borrower with active loans → blocked
- Contributor referenced by titles → blocked
- Series with titles → blocked

### Database Migrations
- SQLx migrations tooling from day one
- Strict migration discipline starting from first public release only
- During development: schema can break freely

### System Health Page (v1)
- mybibli version, MariaDB version
- Cover image disk usage
- Entity counts (titles, volumes, borrowers, locations, series)
- API status (last successful/failed request per source)

### Search
- As-you-type with dynamic filtering (not fuzzy in v1)
- Search fields: title, subtitle, description, contributor name
- Filters: by genre, by volume state
- Target: < 500ms response on 10,000 items
- MariaDB FULLTEXT index on searchable fields

## Items Deferred to Post-v1

- Tags/keywords on titles
- Licence open source choice (before first release)
- Fuzzy search
- CSV import/export
- Camera-based barcode scanning
- Title-to-title linking (translations, related works)
- Additional media types (vinyl, video games)
- Plugin architecture
- Public catalog view for micro-libraries
