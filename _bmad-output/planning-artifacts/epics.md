---
stepsCompleted: [1, 2, 3]
inputDocuments: [prd.md, architecture.md, ux-design-specification.md]
---

# mybibli - Epic Breakdown

## Changelog

- **2026-04-13** — Inserted new **Epic 6: Pipeline CI/CD et fiabilité** between Epic 5 closure and original Epic 6. Renumbered: original Epic 6 (Accès multi-rôle & Sécurité) → Epic 7; original Epic 7 (Administration & Configuration) → Epic 8; original Epic 8 (Polish UX & Accessibilité) → Epic 9. Historical documents (old story files, old retros, readiness report) updated in the same pass for consistency. FR assignments did not change — only the epic labels that hold them.

## Overview

This document provides the complete epic and story breakdown for mybibli, decomposing the requirements from the PRD, UX Design, and Architecture into implementable stories.

## Requirements Inventory

### Functional Requirements

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
- FR11: System can retrieve title metadata from multiple external APIs (Open Library, Google Books, BnF, BDGest, Comic Vine, MusicBrainz, TMDb, OMDb)
- FR12: System can execute a fallback chain across metadata providers when the primary source returns no result
- FR13: System can fetch metadata asynchronously in a background queue while the user continues scanning
- FR14: System can retrieve and store cover images from metadata providers
- FR15: System can resize cover images to a maximum width for efficient storage
- FR16: Librarian can re-download metadata for a title on demand
- FR17: System can detect manually edited fields and prompt for per-field confirmation before overwriting during re-download
- FR18: Librarian can manually edit all metadata fields on a title
- FR19: System can skip metadata providers whose API keys are not configured
- FR20: Any user can search titles as-you-type across title, subtitle, description, and contributor name
- FR21: Any user can filter search results by genre and volume state
- FR22: Any user can navigate between linked entities (title → volumes, contributor → titles, series → volumes, location → contents)
- FR23: Any user can paginate through result lists using classic pagination
- FR24: Any user can view the contents of a storage location sorted by title, author, genre, or Dewey code
- FR25: Librarian can assign a storage location to a volume by scanning the location label
- FR26: System can track volume status (not shelved, shelved, on loan)
- FR27: System can display the current location path for each volume (e.g., "Salon → Bibliothèque 1 → Étagère 3")
- FR28: Librarian can set a volume's condition/state from a configurable list
- FR29: Librarian can add an edition comment to a volume (pocket, hardcover, collector, etc.)
- FR30: System can validate and register volume identifiers (V0001–V9999) scanned from pre-printed labels
- FR31: System can validate and register location identifiers (L0001–L9999) scanned from pre-printed labels
- FR32: Admin can create, edit, and delete storage locations in a tree hierarchy of variable depth
- FR33: Admin can configure location node types (room, bookcase, shelf, box, etc.)
- FR34: System can prevent deletion of locations that contain volumes
- FR35: System can assign a "not shelved" status to volumes without a location
- FR36: Librarian can create a series (name, type open/closed, total volume count for closed series)
- FR37: Librarian can assign a title to a series with a position number
- FR38: System can detect and display missing volumes in a series (gap detection)
- FR39: System can display a series overview with owned volumes and gaps visually distinguished
- FR40: Librarian can register a BD omnibus as a special volume covering multiple positions in a series
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
- FR51: System can create and manage contributors as unique entities (one record per person)
- FR52: System can associate contributors with titles via roles (author, director, composer, performer, illustrator, screenwriter, colorist, translator, etc.)
- FR53: System can assign multiple roles to the same contributor on the same title
- FR54: System can prevent deletion of a contributor referenced by any title
- FR55: Any user can view global collection statistics (title count, volume count, loan count)
- FR56: Any user can view recent additions
- FR57: Any user can view collection statistics by genre
- FR58: Librarian can view actionable indicators with counts (unshelved volumes, overdue loans, series with gaps, recent cataloged, recent returns)
- FR59: Any user can see loan status on volume details ("on loan" without borrower name for anonymous, full details for Librarian/Admin)
- FR60: System can display a dynamic scan feedback list showing recent scan results
- FR61: System can auto-dismiss successful and informational scan entries (fade starts at 10 seconds, entry removed at 20 seconds). Warning and error entries persist until dismissed or resolved. Timing is hardcoded in v1, not admin-configurable
- FR62: System can persist error entries in the feedback list with clickable error details
- FR63: System can play configurable audio feedback for distinct scan outcomes (title found, volume created, error, existing ISBN)
- FR64: Dashboard can display a count of titles with unresolved metadata errors
- FR65: Any user can browse, search, and view the catalog without authentication
- FR66: Librarian can authenticate to access cataloging, loan, and editing capabilities
- FR67: Admin can authenticate to access system configuration and user management
- FR68: Admin can create, edit, and deactivate user accounts with role assignment (Librarian, Admin)
- FR69: System can maintain user sessions with two expiry mechanisms: (1) session expires when the browser closes, and (2) session expires after a configurable inactivity timeout (default 4 hours). A Toast notification warns the user 5 minutes before inactivity expiry with a "Stay connected" option
- FR70: Admin can configure the list of genres
- FR71: Admin can configure volume states with a loanable/not-loanable flag per state
- FR72: Admin can configure contributor roles
- FR73: Admin can configure storage location node types
- FR74: Admin can configure the overdue loan threshold (in days)
- FR75: Admin can configure API keys for metadata providers
- FR76: System can display a health page showing application version, MariaDB version, disk usage, entity counts, and API provider status
- FR77: Any user can switch the UI language between French and English
- FR78: Any user can toggle between light and dark display modes
- FR79: System can detect the user's system preference for color scheme and apply it by default
- FR80: System can prevent permanent deletion (from Trash) of any entity that is still referenced by active (non-deleted) entities
- FR81: System can preserve a title when its last physical volume is deleted
- FR82: System can enforce optimistic locking to prevent concurrent edit conflicts
- FR83: System can display contextual help on form fields and interactive elements (tooltips, help icons, placeholder text)
- FR84: System can support keyboard shortcuts for common actions during scan workflows (submit, cancel, navigate)
- FR85: System can operate in fully manual mode when no metadata API keys are configured
- FR86: System can automatically create the database schema on first launch
- FR87: System can present a first-launch setup wizard to create the initial admin account
- FR88: System can display a fixed-size placeholder with media-type icon while cover images are loading
- FR89: Librarian can view all active loans for a specific borrower from the borrower's detail page
- FR90: System can display volume count and status summary on the title detail page
- FR91: System can initialize default reference data (genres, volume states, contributor roles) on first launch
- FR92: Librarian can assign a media type to a title
- FR93: System can adapt title form fields based on the assigned media type
- FR94: Librarian can set and edit the language of a title (pre-filled by metadata API)
- FR95: Any user can view a list of all series with their completion status (owned/total, gap count)
- FR96: Any user can search for a volume by its label identifier (e.g., V0042) in the global search
- FR97: Librarian can edit contributor details (name, biography)
- FR98: Librarian can edit borrower contact details
- FR99: Librarian can edit series details (name, type, total count)
- FR100: System can prevent deletion of a genre, volume state, or contributor role that is currently assigned to any title or volume
- FR101: Librarian can assign a genre to a title from the configurable genre list
- FR102: System can complete the scan-to-catalog and scan-to-shelve workflows on a dedicated /catalog page without page navigation
- FR103: System can validate ISBN/ISSN checksums client-side before server submission and display immediate feedback on invalid codes
- FR104: System can reject already-assigned V/L labels at scan time with specific details
- FR105: System can display a current title banner on /catalog showing which title volumes are being attached to
- FR106: System can provide a dedicated cataloging page (/catalog) separate from the home page
- FR107: Librarian can navigate to /catalog via a global keyboard shortcut from any page
- FR108: System can display a session counter on /catalog showing items cataloged this session
- FR109: System can soft-delete all entity types — deleted items become invisible in all views but are retained for 30 days
- FR110: Admin can view all soft-deleted items on a Trash page (/admin → Trash tab)
- FR111: Admin can restore soft-deleted items, with conflict detection if associations have changed during deletion period
- FR112: Admin can permanently delete items from Trash (modal confirmation, irreversible)
- FR113: System can auto-purge soft-deleted items older than 30 days at application startup or daily check
- FR114: Any user can view a "Similar titles" section on the title detail page showing up to 8 related titles. Priority: same series > same author > same genre+decade
- FR115: Any user can toggle between list and grid browse display modes, with preference persisted per user
- FR116: Admin can generate a barcode display for any storage location (Code 128), printable or saveable as image
- FR117: System can permanently retire L-codes after location deletion (never recycled)
- FR118: Librarian can add a Dewey code to a title (optional field, pre-filled by BnF API)
- FR119: Admin can delete a borrower from /borrowers page (blocked if active loans, modal confirmation)
- FR120: Admin page can be organized as 5 tabs: Health, Users, Reference Data, Trash, System
- FR121: Setup wizard steps can be idempotent — if interrupted and resumed, each step detects existing data

### NonFunctional Requirements

- NFR1: As-you-type search must return results within 500 ms with 10,000 titles
- NFR2: Scan input prefix detection must be immediate (client-side, no server round-trip)
- NFR3: Server response to a scan action must complete within 500 ms
- NFR4: Page navigation between views must complete within 500 ms
- NFR5: Initial page load must complete within 1 second on local network
- NFR6: Background metadata fetch must complete within 5 seconds per API source
- NFR7: Container startup (docker start to HTTP 200) must complete within 10 seconds
- NFR8: System must support 3–4 concurrent users without exceeding response time targets
- NFR9: User passwords must be hashed using Argon2 before storage
- NFR10: Session tokens must be cryptographically random (minimum 256-bit), HttpOnly, SameSite=Strict cookies
- NFR11: Anonymous users must not access borrower personal data
- NFR12: All write operations must require Librarian or Admin authentication
- NFR13: Admin-only operations must be inaccessible to Librarian role
- NFR14: API keys must be stored as environment variables, never in database or code
- NFR15: Content Security Policy headers must prevent XSS attacks (strict, no unsafe-inline)
- NFR16: Each metadata provider must be an independent, interchangeable module
- NFR17: Metadata fallback chain must continue to next provider on failure/timeout
- NFR18: API rate limits must be respected (Google Books: 1,000/day, MusicBrainz: 1 req/sec)
- NFR19: System must remain fully functional when all external APIs are unavailable
- NFR20: Failed metadata fetches must be logged and surfaced without blocking cataloging
- NFR21: All data must be durable across application restarts and container recreation
- NFR22: Optimistic locking must prevent silent data overwrites
- NFR23: Database migrations must be applied automatically on startup
- NFR24: Cover image storage path must be configurable via Docker volume
- NFR25: Application must reconnect to MariaDB within 30 seconds using exponential backoff (max 5 retries)
- NFR26: All functions must have unit tests (DRY principle)
- NFR27: All features must have Playwright end-to-end tests
- NFR28: Code, comments, variable names, and commit messages must be in English
- NFR29: Architecture must support adding new metadata providers without modifying existing code
- NFR30: Database schema changes must use versioned migration files
- NFR31: Application must log all significant events to stdout in structured JSON format
- NFR32: Docker image size must not exceed 100 MB. Cover images must average < 100 KB
- NFR33: Audio feedback must play within 100 ms of the triggering scan event
- NFR34: Total static assets (CSS + JS) must not exceed 500 KB uncompressed
- NFR35: Runtime memory must not exceed 100 MB under normal operation
- NFR36: System must cache metadata lookups for 24 hours, invalidated on manual re-download
- NFR37: All user data must remain on local network — no telemetry, no cloud sync
- NFR38: All error messages must be i18n keys with human-written FR/EN translations. Pattern: "What happened → Why → What you can do"
- NFR39: All list views must display 25 items per page (fixed in v1)
- NFR40: Metadata fetch must use configurable global timeout (default 30s), parallel execution, never block scan loop
- NFR41: Reference data (genres, states, roles) not translated in v1

### Additional Requirements

**From Architecture Document:**

- AR1: Custom project initialization (cargo new, no starter template). Full project structure with 60+ files defined
- AR2: Multi-stage Docker build: Stage 1 Rust binary (musl), Stage 2 runtime (alpine + binary + CSS + static)
- AR3: Tailwind v4 CSS pre-generated in CI, not in Docker build. Output.css committed or built as CI artifact
- AR4: SQLx offline mode: .sqlx/ directory committed to git. `cargo sqlx prepare` after every query change
- AR5: MariaDB utf8mb4 mandatory: `--character-set-server=utf8mb4 --collation-server=utf8mb4_unicode_ci` + `?charset=utf8mb4` in connection URL
- AR6: Askama + askama_axum for compile-time template rendering with Axum IntoResponse integration
- AR7: Spawn-and-track metadata pattern: Tokio::spawn per scan, results tracked in `pending_metadata_updates` table, delivered via PendingUpdates middleware as OOB swaps on next HTMX request
- AR8: HtmxResponse struct for composing main fragment + OOB swap fragments in a single HTTP response
- AR9: AppSettings loaded from MariaDB `settings` table into Arc<RwLock> cache. Invalidated on admin save
- AR10: Adjacency list with CTE recursive queries for storage location tree (parent_id pattern)
- AR11: Cover images served via tower-http ServeDir at /covers/{title_id}.jpg. 400px max, JPEG 80%
- AR12: Mock metadata server in docker-compose.test.yml for deterministic Playwright tests without real API calls
- AR13: Session storage in MariaDB `sessions` table with token, user_id, data JSON, last_activity timestamp
- AR14: Metadata cache in MariaDB `metadata_cache` table (code, response JSON, fetched_at). 24h TTL
- AR15: Error response pipeline: AppError enum → HTMX-aware rendering (FeedbackEntry on /catalog, inline on forms, StatusMessage on pages)
- AR16: Middleware stack order: Logging → Auth → [Handler] → PendingUpdates → CSP
- AR17: active_*/deleted_*/no-prefix query naming convention for soft-delete filtering
- AR18: All entity URLs use auto-increment integer IDs (no slugs, no UUIDs)
- AR19: Language toggle = full page reload (not HTMX swap) to preserve JavaScript state
- AR20: CI pipeline: 2 jobs (Build+Test with cargo, E2E with Docker+Playwright)
- AR21: x86_64 target platform. ARM Synology not tested in v1
- AR22: ISBN/codes stored as digits only (no dashes). V-codes and L-codes as CHAR(5)
- AR23: Loan lifecycle: row-based with returned_at (NULL = active, NOT NULL = returned)
- AR24: Database common columns: id BIGINT PK, created_at, updated_at, deleted_at, version INT
- AR25: Timestamps: MariaDB TIMESTAMP in UTC, conversion to local in templates for display
- AR26: No dotenvy crate — env vars injected by Docker, read via std::env::var()

### UX Design Requirements

- UX-DR1: Implement ScanField component with collapsed/expanded modes, prefix detection, autofocus dual mechanism (hx-on::after-settle + focusout fallback), 3 variants (catalog, loans, home search)
- UX-DR2: Implement FeedbackEntry component with 4 color variants (success/info/warning/error), skeleton loading state, fade lifecycle (10s+10s via single setInterval), Cancel button on last resolved entry (implicit commit pattern), positional stability rule
- UX-DR3: Implement CatalogToolbar compound component: current title banner + active location line + UPC session type + session counter
- UX-DR4: Implement FilterTag dual-state component: clickable dashboard tag (pill with count) → active filter badge (pill with ✕). Single active filter at a time. Zero-count tags hidden
- UX-DR5: Implement DataTable component with sortable columns (▲/▼), responsive column hiding per breakpoint, clickable rows, classic pagination (25 fixed), HTMX tbody swap. LoanRow variant with scan-to-highlight (1s amber flash) and duration color coding
- UX-DR6: Implement NavigationBar with role-based link visibility (Anonymous/Librarian/Admin), active page indicator, theme toggle (sun/moon), language toggle (FR/EN → full page reload), hamburger menu on tablet/mobile with scanner burst auto-close
- UX-DR7: Implement AdminTabs (5 tabs: Health, Users, Reference Data, Trash with badge, System) with horizontal tab bar, HTMX tab panel swap, URL parameter persistence
- UX-DR8: Implement Modal component for destructive confirmations only (never during scan loop). Focus trap, Escape to close, scanner guard (tabindex=-1 on background). Variants: Delete, Delete Forever, Remove, Warning
- UX-DR9: Implement AutocompleteDropdown with type-ahead search (150ms debounce, min 1 char), match highlighting, "Create new" inline option, server caps at 20 results
- UX-DR10: Implement Cover component with 3 states: loading (shimmer in 2:3 container), missing (media-type SVG placeholder), loaded (img with object-fit cover). 4 size variants (thumbnail 40×60, card 120×180, detail 200×300, grid 150×225). Dark mode light shadow. Lazy loading below fold
- UX-DR11: Implement LocationBreadcrumb with clickable segments, truncation on mobile ("... → Parent → Current"), inline variant for feedback entries
- UX-DR12: Implement LocationTree with collapsible nodes, recursive volume counts, action buttons (add child, generate barcode, edit), keyboard navigation (arrow keys)
- UX-DR13: Implement StatusMessage for empty states (encouraging tone, role-aware action buttons) and connection lost overlay (aria-live assertive)
- UX-DR14: Implement Toast for session expiry warning (slide down, "Stay connected" + dismiss, 5 min before timeout)
- UX-DR15: Implement VolumeBadge status indicator (4 variants: shelved/green, on loan/blue, not shelved/amber, overdue/red) with color + icon + text triple channel
- UX-DR16: Implement SeriesGapGrid with filled/missing squares, hover tooltips, diagonal hatch pattern for colorblind accessibility, clickable filled squares → title detail. 8 per row desktop, 4 tablet
- UX-DR17: Implement TitleCard in list mode (cover left + info right) and grid mode (cover top + info bottom + hover overlay with media icon + count + badge). Touch: first tap overlay, second tap navigate
- UX-DR18: Implement BrowseToggle (list/grid radiogroup) with preference persistence
- UX-DR19: Implement BarcodeDisplay for Code 128 location labels: inline SVG (server-generated via barcoders crate) + L-code text + full path. @media print stylesheet (white bg, hide nav)
- UX-DR20: Implement SetupWizard with 4-step progress indicator (dots), Previous/Next navigation, data persistence per step (idempotent on resume), "Complete setup" on last step
- UX-DR21: Implement InlineForm for reference data CRUD (genres, states, roles): add/rename/delete inline, Enter saves, Escape cancels, loanable checkbox toggle with warning modal if active loans
- UX-DR22: Implement MediaTypeSelector inline button group for UPC disambiguation (6 media types with icons), session memory for last choice
- UX-DR23: Implement ContributorList: full variant (all contributors with roles, clickable names) and compact variant (primary contributor only)
- UX-DR24: Design token system via Tailwind v4 @theme: warm stone neutral palette, indigo primary, 4 feedback colors (green/blue/amber/red) with light/dark variants at WCAG AA 4.5:1 contrast. System font stack. 4px base spacing. 3 breakpoints (mobile <768, tablet 768-1023, desktop ≥1024)
- UX-DR25: Implement 7 JS modules: scan-field.js (prefix detection, scanner vs typing), feedback.js (lifecycle, fade timers), audio.js (Web Audio API 4 tones), theme.js (dark/light toggle), focus.js (focus attractor dual mechanism), scanner-guard.js (modal interception), mybibli.js (entry point, init all modules)
- UX-DR26: Implement scanner detection state machine on home page: 4 states (IDLE, DETECTING, SEARCH_MODE, SCAN_PENDING) with two independent timers (scanner_burst_threshold, search_debounce_delay)
- UX-DR27: Implement HTMX error handling: htmx:responseError and htmx:sendError handlers that restore UI state, display error feedback, preserve scan field input
- UX-DR28: Implement responsive per-page layouts as specified in UX spec: /catalog feedback above scan field on tablet, hamburger scanner auto-close, etc.
- UX-DR29: Implement WCAG 2.2 AA accessibility: semantic HTML, ARIA attributes per component spec, skip link, dynamic html lang, prefers-reduced-motion, axe-core in Playwright CI
- UX-DR30: Implement Similar Titles section on /title/:id: same author > same genre+decade > same series, max 8, section absent if 0. Titles without publication year excluded from decade matching

### FR Coverage Map

| FR | Epic | Brief |
|----|------|-------|
| FR1-FR10 | 1 | Scan field, prefix detection, title/volume creation, autofocus |
| FR11-FR12 | 3 | Multi-API metadata retrieval, fallback chain |
| FR13 | 1 | Async metadata queue (basic — 2 providers) |
| FR14-FR15 | 3 | Cover image retrieval, resize |
| FR16-FR19 | 3 | Re-download, per-field confirmation, manual edit, skip unconfigured APIs |
| FR20-FR23 | 1 | Search as-you-type, filters, cross-navigation, pagination |
| FR24 | 2 | Location content view sortable |
| FR25-FR31 | 2 | Shelving workflow, volume/location identifiers |
| FR32-FR35 | 2 | Storage location hierarchy CRUD |
| FR36-FR40 | 5 | Series CRUD, gap detection, omnibus |
| FR41-FR50 | 4 | Loan management (borrowers, loans, overdue, returns) |
| FR51-FR53 | 1 | Contributor management (create, roles, multi-role) |
| FR54 | 1 | Contributor deletion protection |
| FR55-FR57 | 9 | Dashboard: global stats, recent additions, genre stats |
| FR58-FR59 | 9 | Dashboard: actionable indicators, loan status visibility |
| FR60 | 1 | Dynamic scan feedback list (basic) |
| FR61-FR64 | 3 | Feedback lifecycle (fade, persist errors, audio, metadata error count) |
| FR65-FR67 | 7 | Anonymous browse, Librarian auth, Admin auth |
| FR68 | 8 | User account management |
| FR69 | 1+7 | Sessions: browser close (Epic 1), inactivity timeout + Toast (Epic 7) |
| FR70-FR76 | 8 | Admin configuration (genres, states, roles, node types, overdue threshold, API keys, health page) |
| FR77 | 7 | Language switch FR/EN |
| FR78-FR79 | 1 | Light/dark mode toggle + prefers-color-scheme (basic in Epic 1) |
| FR80 | 1 | Permanent deletion protection (foundation — soft-delete pattern) |
| FR81 | 1 | Preserve title when last volume deleted |
| FR82 | 1 | Optimistic locking |
| FR83-FR84 | 9 | Contextual help, keyboard shortcuts (complete) |
| FR85 | 3 | Manual mode without API keys |
| FR86 | 1 | Auto DB schema creation on first launch |
| FR87 | 8 | First-launch setup wizard |
| FR88 | 3 | Cover placeholder with media-type icon |
| FR89 | 4 | Borrower detail: active loans list |
| FR90 | 1 | Volume count/status on title detail |
| FR91 | 8 | Initialize default reference data on first launch |
| FR92-FR94 | 1 | Media type assignment, form adaptation, language field |
| FR95 | 5 | Series list with completion status |
| FR96 | 1 | Search by volume label |
| FR97 | 1 | Edit contributor details |
| FR98 | 4 | Edit borrower details |
| FR99 | 5 | Edit series details |
| FR100 | 8 | Reference data deletion protection |
| FR101 | 1 | Genre assignment to title |
| FR102-FR108 | 1 | Dedicated /catalog page, preventive validation, title banner, shortcut, session counter |
| FR109 | 1 | Soft-delete pattern (deleted_at on all tables, active_* queries) — foundation only |
| FR110-FR112 | 8 | Admin Trash page (view, restore, permanent delete) |
| FR113 | 8 | Auto-purge soft-deleted items (30 days) |
| FR114-FR115 | 5 | Similar titles, list/grid browse toggle |
| FR116-FR117 | 2 | Barcode generation for locations, L-code retirement |
| FR118 | 5 | Dewey code field |
| FR119 | 4 | Borrower deletion with guard |
| FR120 | 8 | Admin page 5 tabs structure |
| FR121 | 8 | Setup wizard idempotent steps |

**NFR Distribution:**
- NFR1-NFR5, NFR7-NFR10, NFR12, NFR14, NFR22-NFR24, NFR26-NFR28, NFR30-NFR32, NFR34-NFR35, NFR38 (foundation) → Epic 1
- NFR6, NFR16-NFR20, NFR33, NFR36, NFR40 → Epic 3
- NFR11 → Epic 4
- NFR13, NFR15 → Epic 7
- NFR37, NFR39, NFR41 → Epic 8
- NFR8, NFR21, NFR25, NFR29 → Cross-cutting (verified per epic)

**AR Distribution:**
- AR1-AR6, AR8, AR12 (mock basic), AR16-AR18, AR20-AR22, AR24-AR26 → Epic 1
- AR10, AR11 → Epic 2
- AR7, AR12 (mock extended), AR14 → Epic 3
- AR9, AR13 → Epic 7/8
- AR15, AR19 → Epic 7

**UX-DR Distribution:**
- UX-DR1, UX-DR2 (basic), UX-DR3, UX-DR5 (basic), UX-DR6 (basic), UX-DR10 (basic), UX-DR15, UX-DR23, UX-DR24, UX-DR25 (scan-field, feedback, focus, theme), UX-DR29 (foundation) → Epic 1
- UX-DR11, UX-DR12, UX-DR19 → Epic 2
- UX-DR2 (complete), UX-DR10 (complete), UX-DR22, UX-DR25 (audio.js), UX-DR27 → Epic 3
- UX-DR5 (LoanRow), UX-DR9 → Epic 4
- UX-DR16, UX-DR17, UX-DR18, UX-DR30 → Epic 5
- UX-DR14 (Toast), UX-DR25 (scanner-guard.js) → Epic 7
- UX-DR7, UX-DR20, UX-DR21 → Epic 8
- UX-DR4, UX-DR6 (complete), UX-DR8, UX-DR13, UX-DR26, UX-DR28 → Epic 9

**Coverage: 121/121 FRs, 41/41 NFRs, 26/26 ARs, 30/30 UX-DRs — ZERO orphans.**

## Epic List

### Epic 1: Je catalogue mon premier livre
The cataloger can scan ISBNs, create titles and volumes, search the catalog, and see scan feedback. The first successful scan validates the entire tool. Project foundation: Docker, DB, Axum server, CI pipeline, design tokens, soft-delete pattern, mock metadata server (2 providers).

**FRs:** FR1-FR10, FR13, FR20-FR23, FR30, FR51-FR54, FR60, FR69 (browser close only), FR78-FR82, FR86, FR88 (basic), FR90, FR92-FR94, FR96-FR97, FR101-FR109 (pattern only)
**ARs:** AR1-AR6, AR8, AR12 (basic), AR16-AR18, AR20-AR22, AR24-AR26
**UX-DRs:** UX-DR1, UX-DR2 (basic), UX-DR3, UX-DR5 (basic), UX-DR6 (basic), UX-DR10 (basic), UX-DR15, UX-DR23, UX-DR24, UX-DR25 (scan-field, feedback, focus, theme), UX-DR29 (foundation)
**NFRs:** NFR1-NFR5, NFR7-NFR10, NFR12, NFR14, NFR22-NFR24, NFR26-NFR28, NFR30-NFR32, NFR34-NFR35, NFR38 (foundation)

#### Story 1.1: Project Skeleton & Foundation (DONE)

As a developer,
I want a fully configured Rust project skeleton with Docker, MariaDB, Axum, Askama, Tailwind, CI pipeline, and initial database schema,
so that all subsequent stories can build on a solid, tested, and deployable foundation.

**FRs:** FR86
**ARs:** AR1-AR6, AR24-AR26
**NFRs:** NFR7, NFR22-NFR24, NFR26-NFR28, NFR30-NFR32, NFR34-NFR35
**UX-DRs:** UX-DR24

#### Story 1.2: Scan Field & Catalog Page

As a librarian,
I want a dedicated /catalog page with a scan input field that detects ISBN/V-code/L-code prefixes,
so that I can begin the scanning workflow with immediate visual feedback.

**FRs:** FR1, FR2, FR10, FR102, FR105, FR106, FR107
**ARs:** AR8, AR16
**NFRs:** NFR2, NFR3, NFR5
**UX-DRs:** UX-DR1, UX-DR3, UX-DR6(basic), UX-DR25(scan-field, focus), UX-DR29(foundation)

**Acceptance Criteria:**

**Given** the application is running and I navigate to `/catalog`,
**When** the page loads,
**Then** I see a scan input field with autofocus, a placeholder "ISBN, V-code, L-code...", and a navigation bar with a link to /catalog.

**Given** I am on any page,
**When** I press the global keyboard shortcut (Ctrl+K or Cmd+K),
**Then** I am navigated to the /catalog page with the scan field focused.

**Given** I type "9782070360246" into the scan field,
**When** the client-side prefix detection runs,
**Then** the system identifies it as an ISBN (978 prefix) before sending to the server.

**Given** I type "V0042" into the scan field,
**When** the prefix detection runs,
**Then** the system identifies it as a V-code (V prefix + 4 digits).

**Given** I type "L0001" into the scan field,
**When** the prefix detection runs,
**Then** the system identifies it as an L-code (L prefix + 4 digits).

**Given** an HTMX response is returned after a scan action,
**When** the response settles,
**Then** the scan field regains focus automatically (via hx-on::after-settle).

**Given** the scan field receives input,
**When** I press Enter,
**Then** the form submits via HTMX POST to /catalog/scan.

**Given** I access /catalog without authentication (no session),
**When** the page loads,
**Then** I am redirected to /login or shown an access denied message (Librarian role required per NFR12).

#### Story 1.3: Title CRUD & ISBN Scanning

As a librarian,
I want to scan an ISBN to create a new title or open an existing one, and optionally create titles manually,
so that I can catalog books efficiently with minimal typing.

**FRs:** FR3, FR6, FR8, FR92, FR93, FR94, FR101
**ARs:** AR17, AR18, AR22
**NFRs:** NFR3, NFR12, NFR38
**UX-DRs:** UX-DR2(basic), UX-DR10(basic)

**Acceptance Criteria:**

**Given** I scan an ISBN that does not exist in the database,
**When** the server processes the scan,
**Then** a new title is created with the ISBN and a default media type (book for 978/979), and a success FeedbackEntry appears in the feedback list.

**Given** I scan an ISBN that already exists in the database,
**When** the server processes the scan,
**Then** the existing title is opened (info FeedbackEntry) instead of creating a duplicate.

**Given** I click the "New title" button (or Ctrl+N) on /catalog,
**When** the title creation form appears,
**Then** I can fill in title, media type (required), genre, language, subtitle, publisher, publication date, and optional ISBN/ISSN/UPC fields.

**Given** I select a media type on the title form,
**When** the media type changes,
**Then** the form adapts to show/hide fields relevant to that media type (e.g., page_count for books, track_count for CDs).

**Given** I submit the title creation form with valid data,
**When** the server processes the request,
**Then** the title is created, the form closes, the title becomes the "current title" in the catalog session, and the context banner updates.

**Given** a title is created,
**When** it is displayed anywhere,
**Then** a media-type placeholder SVG icon is shown as the cover image (since no cover is fetched yet).

**Given** the server encounters an error during title creation,
**When** the error is returned,
**Then** a red FeedbackEntry appears with a localized error message (i18n key, not raw error).

#### Story 1.4: Volume Management

As a librarian,
I want to scan V-code labels to create physical volumes and attach them to the current title,
so that I can track individual copies of each title in my collection.

**FRs:** FR4, FR5, FR7, FR30, FR90
**ARs:** AR22
**NFRs:** NFR3
**UX-DRs:** UX-DR15

**Acceptance Criteria:**

**Given** a title is set as "current title" in the catalog session,
**When** I scan a V-code (e.g., V0042) that does not exist,
**Then** a new volume is created with that label attached to the current title, and a success FeedbackEntry appears.

**Given** I scan a V-code that already exists in the database,
**When** the server processes the scan,
**Then** an error FeedbackEntry appears with "V0042 is already assigned to {title_name}" and the volume is not created.

**Given** no title is set as "current title",
**When** I scan a V-code,
**Then** a warning FeedbackEntry appears indicating I must first scan an ISBN to establish a title context.

**Given** I am on a title detail page,
**When** I click "Add volume",
**Then** a form appears where I can enter a V-code manually to create a new volume for that title.

**Given** a title has volumes,
**When** I view the title detail page,
**Then** I see a volume count and status summary (e.g., "3 volumes: 2 shelved, 1 not shelved") with VolumeBadge status indicators.

**Given** V-codes are entered,
**When** they are validated,
**Then** only the format V followed by exactly 4 digits (V0001-V9999) is accepted.

#### Story 1.5: Contributor Management

As a librarian,
I want to manage contributors (authors, illustrators, etc.) and associate them with titles via roles,
so that I can find titles by contributor and maintain accurate bibliographic data.

**FRs:** FR51, FR52, FR53, FR54, FR97
**NFRs:** NFR12
**UX-DRs:** UX-DR23

**Acceptance Criteria:**

**Given** I am on a title detail page,
**When** I click "Add contributor",
**Then** I can search for an existing contributor by name (autocomplete) or create a new one inline.

**Given** I add a contributor to a title,
**When** I select a role (e.g., author, illustrator, translator),
**Then** a title_contributors junction record is created linking the title, contributor, and role.

**Given** a contributor is already associated with a title in a specific role,
**When** I try to add the same contributor with the same role again,
**Then** the system rejects the duplicate with an error message.

**Given** a contributor is associated with a title,
**When** I try to add the same contributor with a different role (e.g., also translator),
**Then** the system accepts it, allowing multiple roles per contributor per title.

**Given** a contributor is referenced by at least one title,
**When** I try to delete that contributor,
**Then** the system prevents deletion with an error message listing the referencing titles.

**Given** I am viewing a contributor,
**When** I click "Edit",
**Then** I can modify the contributor's name and biography.

**Given** the title detail page is displayed,
**When** contributors are listed,
**Then** they appear in the ContributorList format with full variant (all contributors with roles, clickable names linking to contributor detail).

#### Story 1.6: Search & Browsing

As any user,
I want to search titles as-I-type across multiple fields and browse results with filters and pagination,
so that I can quickly find items in my collection.

**FRs:** FR20, FR21, FR22, FR23, FR96
**NFRs:** NFR1, NFR4
**UX-DRs:** UX-DR5(basic), UX-DR29(foundation)

**Acceptance Criteria:**

**Given** I type at least 2 characters in the home page search field,
**When** I pause typing for the debounce delay (configurable, default 300ms),
**Then** an HTMX request fires and results appear below, searching across title, subtitle, description, and contributor name.

**Given** search results are displayed,
**When** I click on a title,
**Then** I navigate to the title detail page.

**Given** search results are displayed,
**When** I click a genre filter or volume state filter,
**Then** the results are filtered accordingly and the active filter is visually indicated.

**Given** more than 25 results match my search,
**When** the results are displayed,
**Then** classic pagination controls appear (Previous/Next/page numbers) and each page shows 25 items.

**Given** I am on a title detail page,
**When** I click on a contributor name, volume, or other linked entity,
**Then** I navigate to that entity's detail page (cross-entity navigation).

**Given** I search for a V-code (e.g., "V0042"),
**When** the search runs,
**Then** the volume matching that label is found and its parent title is displayed in results.

**Given** 10,000 titles exist in the database,
**When** I perform an as-you-type search,
**Then** results appear within 500ms (NFR1).

#### Story 1.7: Scan Feedback & Async Metadata

As a librarian,
I want to see immediate scan feedback and have metadata fetched asynchronously from external APIs,
so that I can continue scanning without waiting for metadata resolution.

**FRs:** FR13, FR60, FR88(basic), FR103, FR104, FR108
**ARs:** AR7, AR8, AR12(basic)
**NFRs:** NFR3, NFR14, NFR38(foundation), NFR40
**UX-DRs:** UX-DR2(basic), UX-DR3, UX-DR10(basic)

**Acceptance Criteria:**

**Given** I scan an ISBN on /catalog,
**When** the scan is processed,
**Then** a skeleton FeedbackEntry appears immediately (< 500ms) showing "Fetching metadata..." while the async task runs.

**Given** the async metadata task completes,
**When** I perform the next HTMX action (e.g., another scan),
**Then** the PendingUpdates middleware delivers the resolved metadata as an OOB swap, replacing the skeleton with a success FeedbackEntry showing the title name and author.

**Given** the async metadata task fails or times out (30s),
**When** the result is delivered,
**Then** a warning FeedbackEntry appears indicating metadata was not found, and the title remains with only the ISBN.

**Given** I scan the same ISBN that was fetched within the last 24 hours,
**When** the metadata is looked up,
**Then** the cached response from metadata_cache is used instead of calling the external API again.

**Given** I scan an invalid ISBN (checksum fails),
**When** client-side validation runs,
**Then** an error FeedbackEntry appears immediately without making a server request (FR103).

**Given** I scan a V-code or L-code that is already assigned,
**When** the scan is processed,
**Then** an error FeedbackEntry appears with specific details about the existing assignment (FR104).

**Given** I have cataloged items during this session,
**When** I look at the catalog page,
**Then** a session counter displays the number of items cataloged (FR108).

**Given** the mock metadata server is running (docker-compose.test.yml),
**When** Playwright e2e tests run,
**Then** metadata responses are deterministic and do not depend on real external APIs (AR12).

#### Story 1.8: Cross-cutting Patterns

As a developer,
I want the application to implement soft-delete, optimistic locking, dark/light mode, session management, and the navigation bar,
so that all entity operations follow consistent patterns and the UI is usable.

**FRs:** FR69(browser close), FR78, FR79, FR80, FR81, FR82, FR86, FR109
**ARs:** AR17, AR20, AR21
**NFRs:** NFR9, NFR10, NFR12, NFR22, NFR31
**UX-DRs:** UX-DR6(basic), UX-DR25(theme), UX-DR29(foundation)

**Acceptance Criteria:**

**Given** a user deletes any entity (title, volume, contributor),
**When** the delete is processed,
**Then** the entity's `deleted_at` is set (soft-delete) and it becomes invisible in all normal views but remains in the database.

**Given** a soft-deleted entity is referenced by active entities,
**When** an admin tries to permanently delete it from Trash,
**Then** the system prevents permanent deletion with an error listing the referencing entities (FR80).

**Given** a title has its last volume deleted,
**When** the delete is processed,
**Then** the title itself is preserved (not cascade deleted) per FR81.

**Given** two users edit the same title simultaneously,
**When** the second user submits their changes,
**Then** the system detects the version mismatch and returns a Conflict error with a "Reload" action (FR82).

**Given** a user's browser theme preference is "dark",
**When** they first visit the application,
**Then** dark mode is applied automatically via `prefers-color-scheme` detection (FR79).

**Given** a user clicks the theme toggle,
**When** the toggle is clicked,
**Then** the theme switches between light and dark mode and the preference is persisted in localStorage (FR78).

**Given** a librarian is authenticated,
**When** they close the browser and reopen it,
**Then** their session is expired (session cookie with no max-age) and they must re-authenticate (FR69).

**Given** a user authenticates,
**When** the session is created,
**Then** the session token is cryptographically random (256-bit), stored as HttpOnly SameSite=Strict cookie (NFR9, NFR10).

**Given** the navigation bar is rendered,
**When** the user views any page,
**Then** it shows links to Home, Catalog (if Librarian/Admin), and a theme toggle, with the current page highlighted.

**Given** all queries in the codebase,
**When** they select from entity tables,
**Then** they follow the `active_*/deleted_*/no-prefix` naming convention and include `deleted_at IS NULL` on every table in JOINs.

### Epic 2: Je sais où sont mes livres
The cataloger can create a storage location hierarchy, generate and print barcode labels, shelve volumes by scanning volume + location, and browse shelf contents.

**FRs:** FR24-FR29, FR31-FR35, FR116-FR117
**ARs:** AR10, AR11
**UX-DRs:** UX-DR11, UX-DR12, UX-DR19

### Epic 3: Tous mes médias sont gérés
The cataloger can scan CDs, DVDs, BD, magazines. Metadata arrives from 8 API sources with intelligent fallback. Cover images download and resize automatically. The feedback list operates at full capacity with audio, fading, and error persistence.

**FRs:** FR9, FR11-FR12, FR14-FR19, FR61-FR64, FR85, FR88 (complete), FR93
**ARs:** AR7, AR12 (extended), AR14
**NFRs:** NFR6, NFR16-NFR20, NFR33, NFR36, NFR40
**UX-DRs:** UX-DR2 (complete), UX-DR10 (complete), UX-DR22, UX-DR25 (audio.js), UX-DR27

### Epic 4: Je gère mes prêts
The cataloger can register borrowers, lend volumes, track overdue loans, and process returns with automatic location restoration. The loans page supports scan-to-find.

**FRs:** FR41-FR50, FR89, FR98, FR119
**NFRs:** NFR11
**UX-DRs:** UX-DR5 (LoanRow variant), UX-DR9

#### Story 4.1: Borrower CRUD & Search
**As a** librarian, **I want** to create, edit, search, and delete borrowers, **so that** I can manage the people who borrow from my library.

**FRs:** FR41, FR42, FR98, FR119, FR50
**NFRs:** NFR11

**Acceptance Criteria:**
- Given /borrowers page, when librarian adds a borrower with name/address/email/phone, then the borrower is created and appears in the list
- Given a borrower exists, when librarian searches by name with autocomplete, then matching borrowers appear after 2+ characters
- Given a borrower detail page, when librarian edits contact details and saves, then changes are persisted with optimistic locking
- Given a borrower with no active loans, when admin clicks delete, then a confirmation modal appears and the borrower is soft-deleted
- Given a borrower with active loans, when admin clicks delete, then deletion is blocked with a message showing active loan count
- Given an anonymous user, when they access /borrowers or /borrower/{id}, then they are redirected to login (NFR11)

#### Story 4.2: Loan Registration & Validation
**As a** librarian, **I want** to lend a volume to a borrower, **so that** I can track who has which books.

**FRs:** FR43, FR44, FR47
**NFRs:** NFR11

**Acceptance Criteria:**
- Given a volume and a borrower, when librarian clicks "Lend" on the volume (from /title/{id} or /loans), then a borrower autocomplete appears, and selecting a borrower creates the loan with loaned_at = NOW()
- Given a volume whose condition state is flagged as not loanable, when librarian attempts to lend it, then the loan is blocked with a warning message
- Given the /loans page with a scan field, when librarian scans a V-code, then the matching loan row is highlighted (or "not on loan" feedback if volume is available)
- Given a volume already on loan, when librarian attempts to lend it again, then the loan is blocked with "already on loan" message

#### Story 4.3: Loan Return & Location Restoration
**As a** librarian, **I want** to process book returns with automatic location restoration, **so that** returned books go back where they belong.

**FRs:** FR45, FR46, FR48, FR49

**Acceptance Criteria:**
- Given the /loans page showing all active loans, when librarian clicks "Return" on a loan row, then returned_at is set to NOW() and the volume's location is restored to its previous_location_id
- Given active loans exist, when the /loans page loads, then each loan shows: borrower name, volume label, title, loan duration in days, and a "Return" button
- Given a configurable overdue threshold (default 30 days), when a loan exceeds the threshold, then it is highlighted in red with "overdue" badge
- Given a volume currently on loan, when librarian attempts to delete it, then deletion is blocked with "volume currently on loan" message
- Given the loans page, when loans are displayed, then they are paginated (25 per page) and sortable by borrower/title/date/duration

#### Story 4.4: Borrower Detail & Loan History
**As a** librarian, **I want** to view a borrower's active loans and loan history, **so that** I can manage individual borrower relationships.

**FRs:** FR89

**Acceptance Criteria:**
- Given a borrower detail page at /borrower/{id}, when it loads, then it displays the borrower's contact details and a list of their active loans
- Given a borrower with active loans, when viewing their detail page, then each active loan shows volume label, title, loaned_at date, and duration
- Given the borrower detail page, when librarian clicks "Return" on a loan, then the loan is returned (same behavior as /loans page return)

### Epic 5: Mes séries et ma collection
The cataloger can organize titles into series, visualize gaps, browse the collection with list/grid modes, discover similar titles, and track Dewey codes for physical shelf order.

**FRs:** FR36-FR40, FR54, FR95, FR99, FR114-FR115, FR118
**UX-DRs:** UX-DR16, UX-DR17, UX-DR18, UX-DR30

#### Story 5.1: E2E Stabilization & Test Pattern Documentation
**As a** developer, **I want** a reliable E2E test suite running green against Docker, **so that** feature work on Epic 5+ can trust automated regression detection.

**Source:** Epic 4 retrospective (2026-04-04) action items — carried items #1 (stabilize 6 failing E2E tests) and #2 (document E2E patterns in CLAUDE.md). Team agreement: no Epic 5 feature story enters in-progress until 5-1 is done.

**Scope (technical debt, not FRs):**
- Fix 6 fragile E2E tests: HTMX timing, data isolation between parallel tests, volume edit navigation for non-loanable test
- Document E2E patterns in CLAUDE.md: data isolation, HTMX wait strategies, login fixtures vs cookie injection, shared-DB test ordering
- Verify `cargo sqlx prepare --check` runs clean and add it as a CI gate

**Acceptance Criteria:**
- Given the full E2E suite running against `docker compose -f docker-compose.test.yml`, when `npm test` runs, then 100% of tests pass reliably across 3 consecutive runs (zero flakes)
- Given a developer reading CLAUDE.md, when they look for E2E guidance, then they find documented patterns for data isolation, HTMX response waiting, login vs cookie fixtures, and shared-DB test ordering
- Given the 6 previously-fragile tests (loan flows, volume edit, parallel isolation), when each is run 10 times consecutively, then none flakes
- Given a CI pipeline, when `cargo sqlx prepare --check` is added as a gate, then it passes on current `.sqlx/` cache
- Blocker rule: stories 5-2 through 5-8 must not enter in-progress until 5-1 is done

#### Story 5.1b: E2E Data Isolation Architecture
**As a** developer, **I want** the E2E test suite to reach 100% passing with `fullyParallel: true` restored, **so that** feature work on Epic 5+ has trustworthy regression coverage and fast feedback loops.

**Source:** Discovered during story 5-1 implementation (2026-04-05). Baseline audit revealed 47 failures (not ~6 as estimated in Epic 4 retro). Root cause identified: 11+ spec files share the ISBN constant `9782070360246` and related seed data, causing cascading "already exists" failures regardless of parallel/serial mode. Story 5-1 recovered 11 tests (73 → 84 passing) via serial mode + `loginAs()` helper; 36 failures remain owned by this story. Full audit in `tests/e2e/FLAKY_AUDIT.md`.

**Replaces story 5-1 as the blocker** for Epic 5 feature stories (5-2 through 5-8) until the suite is 100% green and `fullyParallel: true` is restored.

**Acceptance Criteria:**
- Given the E2E test suite, when run against fresh Docker with `fullyParallel: true` restored in `playwright.config.ts`, then all tests pass 120/120 across 3 consecutive fresh-Docker runs (same criterion as story 5-1 AC1)
- Given any two spec files that scan ISBNs, when they run in any order (parallel or serial), then neither depends on the other having or not having scanned the ISBN first (data independence)
- Solution approach: implement one of (or combine) the following, documented in CLAUDE.md:
  - **Option A — Per-spec unique ISBN generator**: introduce `tests/e2e/helpers/isbn.ts` with a function that produces valid EAN-13 ISBNs from a spec-scoped seed; migrate all 11+ specs to use it; extend `e2e-mock-metadata-1` to respond to arbitrary ISBNs with synthetic metadata
  - **Option B — DB reset between spec files**: globalSetup or per-describe `beforeAll` hook that truncates `titles`, `volumes`, `loans`, `borrowers`, `locations` tables via direct DB connection from the test runner
  - **Option C — Idempotent test assertions**: rewrite tests to accept either "success" or "info" feedback variants (loss of specificity, not recommended)
- Delete `tests/e2e/FLAKY_AUDIT.md` once suite reaches 100% green
- Remove the "Known suite state" paragraph from CLAUDE.md's E2E Test Patterns section once resolved
- Restore `fullyParallel: true` and `workers: undefined` (Playwright default) in `playwright.config.ts`
- Verify smoke tests continue to use `loginAs()` helper (do not regress the Rule #7 compliance delivered by 5-1)
- Known remaining failures breakdown (from story 5-1 final audit — 36 tests):
  - ~30 tests: shared ISBN pollution (catalog-title, catalog-volume, catalog-metadata, cover-image, cross-cutting, loan-*, metadata-editing, shelving, location-*, locations, etc.)
  - ~4 tests: smoke tests with downstream state dependencies (epic2-smoke SmokeTestRoom location, borrower-loans smoke lifecycle, metadata-editing smoke, media-type-scanning smoke)
  - ~2 tests: accessibility tests timing out as secondary effects


**As a** librarian, **I want** to be prevented from deleting a contributor still referenced by titles, **so that** I don't leave orphaned references in my catalog.

**FRs:** FR54

**Acceptance Criteria:**
- Given a contributor referenced by at least one title, when librarian clicks delete, then deletion is blocked with a message showing the count of referencing titles
- Given a contributor with zero title references, when librarian clicks delete, then soft-delete proceeds normally via the existing confirmation modal
- Given the error message, when displayed, then it follows the "What happened → Why → What you can do" pattern (NFR38) with i18n key `error.contributor.has_titles`
- Unit test: `ContributorService::delete()` returns `AppError::Conflict` when referencing titles exist
- E2E smoke: create contributor → assign to title → attempt delete → see block message → unassign → delete succeeds

#### Story 5.3: Series CRUD & Listing
**As a** librarian, **I want** to create, edit, and browse series, **so that** I can organize my titles into coherent collections.

**FRs:** FR36, FR95, FR99

**Acceptance Criteria:**
- Given `/series` page, when librarian creates a series with name, type (open/closed), and (if closed) total volume count, then the series is created and appears in the series list
- Given series exist, when any user visits `/series`, then the list shows name, type, owned count, total count (for closed), and gap count, paginated 25/page per NFR39
- Given a series detail page `/series/{id}`, when librarian edits name/type/total count with optimistic locking, then changes are persisted (409 on version mismatch)
- Given a closed series, when librarian tries to set total count below owned count, then the edit is blocked with a preventive validation message
- Given an anonymous user, when they visit `/series` or `/series/{id}`, then they see the list (public read per FR95) — no auth required
- Soft delete pattern: `series` table gets `deleted_at`, `version`, `created_at`, `updated_at` columns; unique(name) WHERE deleted_at IS NULL
- Unit tests: SeriesModel CRUD, optimistic locking
- E2E smoke: create closed series → visit detail → edit → verify persistence

#### Story 5.4: Title-to-Series Assignment & Gap Detection
**As a** librarian, **I want** to assign titles to a series with a position number and see which volumes are missing, **so that** I can identify gaps in my collection.

**FRs:** FR37, FR38, FR39
**UX-DRs:** UX-DR16 (SeriesGapGrid)

**Acceptance Criteria:**
- Given a title detail page, when librarian assigns the title to a series with a position number, then the assignment is persisted with unique(series_id, position) constraint
- Given a series with assigned titles, when viewing `/series/{id}`, then SeriesGapGrid displays filled squares for owned positions and empty squares (with diagonal hatch pattern for colorblind accessibility) for missing positions, 8 per row desktop / 4 tablet
- Given a filled square, when clicked, then it navigates to the title detail page
- Given a square is hovered, when the user waits, then a tooltip shows the position number and title name (or "Missing" for empty)
- Given a closed series with total=10 and titles at positions [1,2,4,7], when `/series/{id}` loads, then gap count displays "6 missing" and the grid shows 4 filled + 6 empty squares
- Given an open series, when viewed, then no total/gap count is shown (only owned titles list)
- Unit test: gap detection algorithm for closed series
- E2E smoke: create closed series → assign titles at positions 1,3 → verify gap grid shows position 2 as missing

#### Story 5.5: BD Omnibus Multi-Position Volume
**As a** librarian, **I want** to register a BD omnibus as a volume covering multiple positions in a series, **so that** my gap detection accurately reflects reality when I own an omnibus instead of individual issues.

**FRs:** FR40

**Acceptance Criteria:**
- Given a title assigned to a series, when librarian creates a volume and marks it as "omnibus", then they can specify a position range (e.g., positions 1-3) instead of a single position
- Given an omnibus volume covering positions [5,6,7] in a series, when `/series/{id}` renders the gap grid, then positions 5, 6, 7 all display as filled
- Given a filled square backed by an omnibus, when clicked, then it navigates to the omnibus volume's title detail
- Given a series where the same position is covered by both an individual title and an omnibus, when rendered, then both contribute to "filled" (idempotent, no error)
- Migration: add `volume_series_positions` link table supporting N positions per volume
- Unit test: gap calculation with mixed individual + omnibus assignments
- E2E: create series → add omnibus covering 3 positions → verify grid filled

#### Story 5.6: Browse List/Grid Toggle with Persistent Preference
**As a** user, **I want** to toggle between list and grid display modes when browsing titles, **so that** I can see more titles at once (grid) or more detail per title (list) depending on my task.

**FRs:** FR115
**UX-DRs:** UX-DR17 (TitleCard), UX-DR18 (BrowseToggle)

**Acceptance Criteria:**
- Given `/catalog` or any browse view, when the page loads, then a BrowseToggle radiogroup (list/grid) is visible at the top
- Given list mode, when rendered, then each TitleCard shows cover on left + title/contributors/year/media icon on right (single row)
- Given grid mode, when rendered, then each TitleCard shows cover on top + title below, with hover overlay revealing contributors + media icon + volume count + any status badge
- Given a touch device in grid mode, when user taps a card, then first tap shows overlay, second tap navigates to title detail
- Given a user changes the toggle, when navigating away and back, then the preference persists (cookie or localStorage, per-user session)
- ARIA: BrowseToggle uses `role="radiogroup"` with keyboard arrow navigation per WCAG 2.2 AA
- Unit test: TitleCard template rendering both modes with/without optional fields
- E2E: load catalog → toggle grid → verify layout → reload → verify grid persisted

#### Story 5.7: Similar Titles Section
**As a** user, **I want** to see similar titles on a title detail page, **so that** I can discover related books in my own collection.

**FRs:** FR114
**UX-DRs:** UX-DR30

**Acceptance Criteria:**
- Given a title detail page, when it loads, then a "Similar titles" section displays up to 8 related titles using the priority order: same series > same author/contributor > same genre+publication decade
- Given fewer than 8 candidates across all criteria, when rendered, then the section shows only the matches (no padding)
- Given zero candidates, when rendered, then the "Similar titles" section is entirely absent (not shown as empty)
- Given a title without a publication year, when candidates are computed, then that title is excluded from genre+decade matching (series and contributor matching still apply)
- Given a similar title card, when clicked, then navigation goes to that title's detail page
- Performance: similar titles query must complete in < 200ms for a catalog of 10k titles (single query with UNION, not N+1)
- Unit test: priority algorithm with mixed candidate sources
- E2E: create 3 titles by same author → view one → verify other 2 appear in Similar Titles

#### Story 5.8: Dewey Code Management
**As a** librarian, **I want** to assign a Dewey code to a title, **so that** I can sort my physical shelves by classification.

**FRs:** FR118

**Acceptance Criteria:**
- Given the title detail/edit form, when librarian enters a Dewey code (optional free-text field), then it is persisted on the title
- Given a title created via ISBN scan with BnF metadata that includes a Dewey code, when the title is created, then the Dewey field is pre-filled
- Given a catalog sort by Dewey code, when applied, then titles are sorted alphanumerically by dewey_code with NULL values last
- Given search/filter UI, when user searches, then Dewey code is NOT a searchable or filterable field (physical sort order only, per FR118)
- Migration: add `dewey_code VARCHAR(32) NULL` to titles table
- Unit test: sort order with NULL last
- E2E: edit title → set Dewey "843.914" → verify persisted → sort catalog by Dewey → verify ordering

### Epic 6: Pipeline CI/CD et fiabilité
Inserted 2026-04-13 (between Epic 5 closure and original Epic 6/auth work). Groups the infrastructure and debt-cleanup needed before v1 release can be contemplated: a GitHub Actions CI/CD pipeline with automated Docker Hub publishing, plus the three carry-over action items from the Epic 5 retrospective (seeded librarian user + `loginAs(role)`; `manually_edited_fields` race fix; `waitForTimeout` E2E cleanup with a grep gate). Closing this epic produces a pushable GitHub repo with gated merges to `main`, plus the prerequisites that unblock Epic 7 (multi-role auth) E2E stories.

**Source:** Sprint planning decision 2026-04-13 after Epic 5 retrospective. No FR/NFR mapping — this is tooling + test-debt work.

**Stories:**

#### Story 6.1: GitHub repo + CI/CD pipeline + Docker Hub publishing
**As a** project maintainer, **I want** every push validated by an automated pipeline and every tagged release producing a Docker Hub image, **so that** I can ship mybibli with confidence and without manual image-building.

**Acceptance Criteria:**
- Given the `github.com/guycorbaz/mybibli` repo exists, when the current `master` branch is renamed to `main` and pushed via SSH, then the remote tracks `origin/main` and all existing history is preserved
- Given a GitHub Actions workflow file, when any push or PR runs, then 3 jobs execute in parallel: `rust-tests` (clippy + cargo test lib/bins + sqlx prepare --check), `db-integration` (MariaDB 10.11 service container + the 3 integration-test crates), `e2e` (Docker Compose stack + Playwright full suite)
- Given a PR, when any of the 3 gate jobs fails, then the PR cannot merge
- Given a push to `main` that passes all 3 gates, when the `docker-publish` job runs, then a `mybibli:main-<sha7>` image is pushed to Docker Hub
- Given a git tag matching `v<semver>` (e.g. `v0.1.0`), when the tag is pushed, then the pipeline verifies `Cargo.toml` version matches the tag and fails otherwise; on match, it builds and pushes `mybibli:<semver>` + `mybibli:latest`
- Given an E2E or integration-test failure, when the job completes, then Playwright traces and screenshots are stored as GitHub artifacts
- Given the Docker Hub secret `DOCKERHUB_TOKEN`, when configured in GitHub repo secrets, then the publish step authenticates and succeeds (not committed to repo)

#### Story 6.2: Seeded librarian user + `loginAs(page, role?)`
**As a** test author, **I want** a seeded librarian-role user and a role-aware `loginAs()` helper, **so that** I can write multi-role E2E tests before Epic 7 starts.

**FRs touched:** none (test infrastructure)

**Acceptance Criteria:**
- Given the dev migration set, when a fresh DB is bootstrapped, then both an `admin` user (existing) and a `librarian` user are seeded with known passwords
- Given `loginAs(page, "admin")` or `loginAs(page, "librarian")` is called in a test, when the helper runs, then the real browser login flow completes and the session cookie reflects the requested role
- Given `loginAs(page)` without a role argument, when called, then behavior is unchanged (logs in as admin) for backward compatibility across existing 133 tests
- Given one existing smoke test is migrated to librarian role, when it runs, then it passes and demonstrates the end-to-end pattern
- Full E2E suite remains green (133+/133+) on parallel mode

#### Story 6.3: Fix `manually_edited_fields` + background-fetch race
**As a** librarian, **I want** my manually-edited metadata to survive a concurrent background metadata fetch, **so that** typing over an auto-populated field is not silently overwritten.

**FRs touched:** NFR11 (reliability), NFR28 (data integrity)

**Acceptance Criteria:**
- Given `tasks/metadata_fetch.rs::update_title_from_metadata`, when it runs, then it respects both the current `manually_edited_fields` JSON and the optimistic `version` column — a concurrent manual edit cannot be silently overwritten
- Given `src/routes/titles.rs::confirm_metadata`, when the `accept_<field>` checkbox is checked but the form value equals the kept value, then the `manually_edited_fields` flag is NOT cleared for that field
- Given `src/routes/titles.rs::confirm_metadata`, when the accepted form value differs from the previously-edited value, then the flag IS cleared (existing behavior for the true "accept replacement" case)
- Unit tests: both branches per field for at least 3 representative fields (publisher, dewey_code, subtitle)
- Integration test via `#[sqlx::test]`: race scenario where a manual edit + background fetch both target the same title; the manual edit wins

#### Story 6.4: Cleanup `waitForTimeout` + grep gate
**As a** test author, **I want** every E2E wait expressed as a DOM-state assertion and a CI gate that prevents `waitForTimeout` regressions, **so that** test flakes are bounded and new contributors cannot reintroduce the anti-pattern.

**FRs touched:** none (test infrastructure)

**Acceptance Criteria:**
- Given the current 32 `waitForTimeout` occurrences across 9 specs, when the story completes, then zero remain (`grep -rE "waitForTimeout\\(" tests/e2e/specs/ | wc -l` returns 0)
- Given each replaced wait, when executed, then the test uses an explicit `expect(locator).toBeVisible()`, `.toContainText(/.../i)`, or equivalent DOM-state assertion
- Given CLAUDE.md Build & Test Commands, when read, then it documents the grep gate command as a pre-commit / pre-PR check
- Given the GitHub Actions pipeline from story 6.1, when a PR introduces a new `waitForTimeout`, then the `rust-tests` or `e2e` job fails (pick the cheapest host for the grep)
- Full E2E suite runs green on 5 consecutive fresh-Docker cycles with zero flakes

### Epic 7: Accès multi-rôle & Sécurité
Anonymous users can browse and search without login. Librarian and Admin roles enforce access control. Sessions include inactivity timeout with Toast warning. Language toggle switches between FR/EN.

**FRs:** FR65-FR67, FR69 (timeout + Toast), FR77
**NFRs:** NFR13, NFR15
**ARs:** AR13, AR15, AR19
**UX-DRs:** UX-DR14, UX-DR25 (scanner-guard.js)

### Epic 8: Administration & Configuration
The admin can manage users, configure reference data (genres, states, roles), manage the storage hierarchy, view system health, and manage the Trash (soft-deleted items). The setup wizard guides first-time configuration.

**FRs:** FR68, FR70-FR76, FR87, FR91, FR100, FR110-FR113, FR120-FR121
**NFRs:** NFR37, NFR39, NFR41
**ARs:** AR9
**UX-DRs:** UX-DR7, UX-DR20, UX-DR21

### Epic 9: Polish UX & Accessibilité
The dashboard shows actionable indicators with counts. Every page has encouraging empty states. Contextual help and keyboard shortcuts are complete. Responsive layouts are optimized per page. The home page scanner state machine handles dual detection. Modals guard destructive actions. WCAG 2.2 AA compliance is verified end-to-end.

**FRs:** FR55-FR59, FR83-FR84
**UX-DRs:** UX-DR4, UX-DR6 (complete — roles, hamburger, scanner auto-close), UX-DR8, UX-DR13, UX-DR26, UX-DR28
