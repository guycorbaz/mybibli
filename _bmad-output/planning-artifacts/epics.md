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

**Scope note — anonymous visibility (2026-04-15):** Anonymous users (FR65) can read the public catalog — titles, volumes, series, contributors, locations — but NOT loan-related data (loans, loan history, borrowers). Borrower records and loan data stay behind librarian/admin auth for privacy reasons.

**Stories:**

#### Story 7.1: Anonymous browsing + role gating
**As a** visitor, **I want** to browse and search the catalog without logging in, **so that** I can explore Guy's library before deciding to request access; **and as a** librarian/admin, **I want** cataloging, editing, loan, and admin operations strictly gated by role, **so that** unauthorized users cannot mutate state or access private data.

**FRs:** FR65, FR66, FR67
**NFRs:** NFR13

**Acceptance Criteria:**
- Given an anonymous visitor (no session cookie), when they navigate to `/catalog`, `/series`, a title detail page, a volume detail page, a contributor page, or a location browse page, then the page renders with read-only affordances (no edit/delete/create buttons, no loan actions, no scan field)
- Given an anonymous visitor, when they attempt to access `/loans`, `/borrowers`, or any borrower/loan detail route, then the middleware redirects them to `/login` with a return URL
- Given an anonymous visitor, when they attempt a write route (POST/PUT/DELETE to titles, volumes, contributors, locations, series, loans, borrowers), then the server returns 303 redirect to `/login` (HX-Redirect for HTMX) and no state change occurs
- Given a librarian-role user, when they attempt admin-only routes (user management, system configuration, settings), then the server returns 403 Forbidden rendered via AppError → FeedbackEntry
- Given an admin-role user, when they access any route, then all operations are permitted
- Given the nav bar, when rendered for an anonymous user, then the "Login" link is visible and cataloging/loan/admin nav items are hidden; for librarian, admin items are hidden; for admin, all items are visible
- Route audit: every existing route in `src/routes/` is annotated with its required role (Anonymous / Librarian / Admin) in a single reference table committed to the repo
- Unit tests: role-gating middleware rejects/allows for each of the 3 roles × at least 2 representative routes per role
- E2E smoke (Foundation Rule #7): blank browser → browse catalog anonymously → click a title → verify read-only → attempt `/loans` → verify redirect → login as librarian → verify cataloging unlocked → verify admin route still 403

#### Story 7.2: Session inactivity timeout + Toast warning
**As a** logged-in user, **I want** my session to expire after a period of inactivity with a 5-minute Toast warning, **so that** an unattended browser does not leave the app open indefinitely, and I never lose work silently.

**FRs:** FR69 (inactivity timeout + Toast — browser-close side already delivered in Epic 1)
**ARs:** AR13
**UX-DRs:** UX-DR14

**Acceptance Criteria:**
- Given the `sessions` table, when the migration runs, then a `last_activity TIMESTAMP NOT NULL` column is added (default `CURRENT_TIMESTAMP`), backfilled for existing rows, and indexed for the cleanup query
- Given any authenticated request, when it passes through the session middleware, then `last_activity` is updated to `NOW()` before the handler runs
- Given an authenticated request, when `NOW() - last_activity > inactivity_timeout` (from `AppSettings`, default 4 hours), then the session is invalidated (soft-delete or row removal) and the middleware returns 303 redirect to `/login` (HX-Redirect for HTMX)
- Given `AppSettings`, when `inactivity_timeout_seconds` is configurable via the settings table (read into `Arc<RwLock<AppSettings>>`), then changing it takes effect for new session checks without restart
- Given a logged-in user on any page, when the remaining time before expiry drops to 5 minutes, then a Toast slides down with i18n-aware text (EN/FR), a "Stay connected" button that issues a keep-alive POST to refresh `last_activity`, and a dismiss affordance
- Given the "Stay connected" button is clicked, when the keep-alive returns success, then the Toast hides and the countdown resets
- Given the Toast is dismissed without action, when the 5 minutes elapse, then the next request returns a redirect to `/login` and the page shows a "Session expired" feedback entry after re-login (optional)
- JS: new `toast.js` module (or extend existing JS) — no new framework dependency, follows the UX-DR25 7-module pattern
- i18n: EN + FR keys for Toast text, "Stay connected", "Session expired"
- Unit tests: middleware invalidation boundary (just before vs just after timeout); keep-alive handler; Toast time calculation
- E2E: shorten `inactivity_timeout` to e.g. 90 s via settings seed for the test, log in, wait, verify Toast appears, click "Stay connected", verify session extended; second test: wait through timeout, verify redirect

#### Story 7.3: Language toggle FR/EN
**As a** user (anonymous or authenticated), **I want** to switch the UI language between French and English from a visible toggle, **so that** I can use the app in my preferred language and the choice persists across sessions.

**FRs:** FR77
**ARs:** AR19

**Acceptance Criteria:**
- Given the nav bar, when rendered, then a language toggle (FR / EN) is visible for all roles including anonymous
- Given the toggle is clicked, when the browser navigates, then it performs a full page reload (not HTMX swap) to the same route with the new language applied (AR19)
- Given no explicit preference, when the app renders, then the default language is French (Guy's primary language) unless `Accept-Language` strongly prefers English
- Given a language choice is made, when the response is returned, then the preference is stored in a cookie `lang=fr|en` (SameSite=Lax, 1-year max-age) and honored on subsequent requests
- Given an authenticated user with a `users.preferred_language` column, when logged in, then the cookie is synced to the DB preference on login and writes to both on toggle
- Given `rust_i18n::t!()` wiring, when a request arrives, then the locale is set from (1) query param override `?lang=`, (2) cookie, (3) user preference, (4) `Accept-Language`, (5) default `fr`
- Given the toggle on any page, when clicked, then the user returns to the same URL (not the home page) with the new language
- i18n key audit: every user-visible string in templates and JS has both EN and FR translations (grep gate in CI: zero `t!("key")` calls without matching EN+FR entries)
- Unit tests: locale resolution priority chain; cookie round-trip
- E2E: anonymous visitor toggles FR→EN on `/catalog`, verify page reloads with EN strings, verify cookie set, navigate, verify EN persists; login, verify DB preference updated

#### Story 7.4: Content Security Policy headers
**As the** project maintainer, **I want** strict CSP headers on every response, **so that** XSS vectors (inline scripts, malicious external resources) are blocked while legitimate cover-image sources still load.

**NFRs:** NFR15

**Acceptance Criteria:**
- Given an Axum middleware layer, when any response is produced, then the `Content-Security-Policy` header is set with directives: `default-src 'self'`; `script-src 'self'` (no `'unsafe-inline'`, no `'unsafe-eval'`); `style-src 'self'` (Tailwind is precompiled — no inline styles); `img-src 'self' data: https://<bnf-cover-host> https://books.google.com https://*.googleusercontent.com` (exact hostnames defined in the architecture doc); `connect-src 'self'` (HTMX same-origin); `font-src 'self'`; `frame-ancestors 'none'`; `base-uri 'self'`; `form-action 'self'`
- Given any HTMX interaction, when it runs, then no CSP violation is logged in the browser console (test manually via E2E + DevTools assertion)
- Given a cover-image URL from BnF or Google Books, when rendered on a title detail page, then it loads successfully under the CSP
- Given the CSP, when a developer accidentally introduces an inline `<script>` or `style`, then the browser blocks it AND the E2E test fails
- Additional security headers set by the same middleware: `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: strict-origin-when-cross-origin`, `Permissions-Policy: camera=(self)` (scanner needs camera on same origin)
- CSP report-only mode available via env var `CSP_REPORT_ONLY=true` for debugging, default off in production
- Unit test: middleware sets expected headers on a sample response
- E2E: load `/catalog`, title detail with external cover, login flow — verify no CSP violations reported; inject a test inline script via DOM mutation and verify it is blocked

#### Story 7.5: Scanner-guard modal interception
**As a** librarian scanning barcodes, **I want** the scanner input to be captured by any open modal, **so that** a scan performed while a confirmation dialog is open does not leak into the background scan field and trigger an unintended catalog action.

**UX-DRs:** UX-DR25 (scanner-guard.js — last of the 7 JS modules)

**Acceptance Criteria:**
- Given `tests/e2e/helpers/scanner.ts` stub (noted as tech debt in CLAUDE.md), when this story runs, then the stub is either completed to a functional helper or the story explicitly reuses the existing `scan-field.js` test hooks
- Given a new `scanner-guard.js` module, when any modal (Askama `<dialog>` or the existing confirmation components) opens, then the module installs a keydown capture listener at `document` level that routes scanner-pattern events (fast sequence ending in Enter, per `scan-field.js` heuristic) to the modal's focused input instead of the background `#scan-field`
- Given a modal closes, when the event fires, then the guard listener is removed and the background scan field resumes receiving events
- Given a scan occurs with no modal open, when detected, then behavior is unchanged (scan field receives it)
- Given nested modals (unlikely but possible), when opened, then the guard stacks correctly (LIFO) and the topmost modal captures scans
- Integration with `mybibli.js`: `scanner-guard.js` is imported and initialized in the entry point alongside the other 6 modules (scan-field, feedback, audio, theme, focus, scanner-guard, mybibli)
- Unit tests (JS): simulate keydown sequences with/without modal open, verify routing
- E2E: open a confirmation modal (e.g., delete volume confirmation), simulate a barcode scan via keyboard events, verify the scan is consumed by the modal or dropped — never leaks to background `#scan-field`; close modal, perform scan, verify normal flow resumes

### Epic 8: Administration & Configuration
The admin can manage users, configure reference data (genres, states, roles, location node types), manage system settings and metadata provider API keys, view system health, and manage the Trash (soft-deleted items including restore and permanent delete). The setup wizard guides first-time configuration.

**FRs:** FR68, FR70-FR76, FR87, FR91, FR100, FR110-FR113, FR120-FR121
**NFRs:** NFR37, NFR39, NFR41
**ARs:** AR9
**UX-DRs:** UX-DR7, UX-DR20, UX-DR21

**Scope note — cross-cutting constraints (2026-04-17, revised 2026-04-18):** NFR37 (no telemetry, all data local) and NFR39 (25 items per page, not user-configurable) are global constraints of the project, not Epic 8 inventions — they surface here because the new admin list views (users, reference data, trash) must comply. NFR41 (reference data not translated in v1) is specific to Epic 8 story 8-4. The original Epic 8 wording "storage hierarchy" was ambiguous: location **node types** (FR73) are configured here (admin sub-table), while the location **tree** itself was delivered by Epic 2. FR91 seeds initial reference data on first boot; the setup wizard (8-8) uses this seed plus admin-specified provider keys. **Story 8-2 (CSRF middleware + form-token injection) was inserted 2026-04-18 to close Epic 7 retro Action 1 — all admin-mutation surfaces (user admin, ref data, settings, trash, setup wizard) depend on it and are renumbered 8-3..8-8.**

**Stories:**

#### Story 8.1: Admin page shell + Health tab
**As an** admin, **I want** a single `/admin` entry point organized as tabs with a Health dashboard as the landing tab, **so that** I can reach all admin operations without nested menus and see at a glance whether the system is healthy.

**FRs:** FR76, FR120
**UX-DRs:** UX-DR7

**Acceptance Criteria:**
- Given an admin-role user, when they navigate to `/admin`, then the page renders with a horizontal tab bar showing 5 tabs: Health (default), Users, Reference Data, Trash, System
- Given a librarian-role user, when they access `/admin` or any `/admin/*` route, then the server returns 403 Forbidden rendered via AppError → FeedbackEntry; anonymous users get the existing 303 redirect to `/login`
- Given the tab bar, when a tab is clicked, then HTMX swaps only the tab panel content (not full page reload), the URL updates with a `?tab=users` query parameter (via `hx-push-url`), and browser Back/Forward navigation restores the correct tab
- Given a deep-link to `/admin?tab=trash`, when the page loads fresh, then the Trash tab is pre-selected and its panel is server-rendered (not fetched via HTMX after load) so JS-disabled users still see content
- Given the Health tab is active, when it renders, then it displays: application version (from build metadata), MariaDB version (from `SELECT VERSION()`), disk usage on the data volume (free/total bytes), entity counts (titles, volumes, contributors, borrowers, active loans — all excluding `deleted_at IS NOT NULL`), and per-provider status (BnF, Google Books — HTTP reachability check with color indicator + last-check timestamp)
- Given the Trash tab label, when rendered, then it shows a badge with the count of soft-deleted items (hidden if 0, red pill with count if > 0) — badge refreshes when tab content is re-fetched
- Given the AdminTabs component, when factored, then it lives at `components/admin_tabs.html` with a `{% block panel %}` slot and is reused by every admin tab (no duplication across 5 handlers)
- Accessibility: tab bar uses `role="tablist"`, each tab has `role="tab"` + `aria-selected`, each panel has `role="tabpanel"` + `aria-labelledby`
- CSP compliance (story 7-4): zero inline styles/scripts in the tab panels or Health rendering — all interactivity via `data-*` attributes routed through existing JS modules
- i18n: EN + FR keys for all 5 tab labels (Santé / Utilisateurs / Données de référence / Corbeille / Système) and all Health labels
- Unit tests: admin-middleware 403 for librarian, 303 for anonymous; tab parameter resolution defaults to `health` when missing or invalid; Health counts exclude soft-deleted
- E2E smoke (Foundation Rule #7): blank browser → login admin → navigate `/admin` → verify 5 tabs visible, Health selected by default, version + MariaDB + counts populated → click each tab → verify URL `?tab=` updates + panel content swaps → attempt `/admin` as librarian → verify 403 feedback entry

#### Story 8.2: CSRF middleware and form-token injection
**As the** project maintainer, **I want** every state-changing request to require a session-bound CSRF token (synchronizer pattern), **so that** cross-site requests from hostile pages cannot trigger logout, language toggle, or any future admin mutation against an authenticated browser — closing the fifth-deferred security commitment before Epic 8's admin-mutation stories land.

**NFRs:** NFR13, NFR15 (defense-in-depth alongside CSP)
**Epic 7 retro Action 1:** This story is the concrete closure option picked over the alternative ("write `docs/auth-threat-model.md` formally accepting the risk"). Four prior deferrals (stories 1-2, 6-x, 7-1, 7-3, 7-4 reviews); the Epic 7 retro flagged a fifth deferral as unacceptable now that admin-mutation surfaces begin in Epic 8.

**Acceptance Criteria:**
- Given a migrated DB, when the migration runs, then `sessions` gets a `csrf_token VARCHAR(64) NOT NULL` column; the login handler generates a 32-byte random base64url token alongside the session token and persists both atomically; a backfill updates any existing session rows with a fresh token so no deployed session breaks
- Given an anonymous visitor (no session row yet), when they first GET any page, then a lazy anonymous session is created with its own `csrf_token` so `/login` and `/language` posts from anonymous users carry a token just like authenticated posts (no "pre-session" special case)
- Given any Askama page template, when rendered, then `BaseContext` (or equivalent common template context) receives a `csrf_token: String` field; `layouts/base.html` emits `<meta name="csrf-token" content="{{ csrf_token|escape }}">` so both form-hidden-input and HTMX-header paths can read it
- Given any `<form method="POST">` template, when rendered, then it includes a hidden `<input type="hidden" name="_csrf_token" value="{{ csrf_token|escape }}">` — this is enforced by `src/templates_audit.rs` which scans `templates/` for `method="post"` / `method="POST"` forms lacking the hidden input and fails `cargo test`
- Given any HTMX request (`hx-post` / `hx-put` / `hx-patch` / `hx-delete`), when submitted, then `static/js/csrf.js` (new module, loaded from `layouts/base.html`) listens for `htmx:configRequest` and sets the `X-CSRF-Token` header from the `<meta name="csrf-token">` element — no inline script, CSP-compliant
- Given a POST / PUT / PATCH / DELETE request, when the CSRF middleware runs, then it validates the token with constant-time comparison against `sessions.csrf_token` from one of: (a) `X-CSRF-Token` header, (b) `_csrf_token` form field — if both are present the header wins; if neither is present or values mismatch the middleware returns 403 Forbidden rendered via `AppError::Forbidden` → existing FeedbackEntry pipeline
- Given the CSRF middleware, when it decides to validate, then GET / HEAD / OPTIONS requests are never validated; only state-changing methods are; all `/static/*` and `/covers/*` requests pass through untouched
- Given a carved-out endpoint, when the routing table declares it, then the CSRF check is skipped via explicit route-level opt-out (central allowlist in `src/middleware/csrf.rs::CSRF_EXEMPT_ROUTES`) — initial allowlist: `POST /login` only (unauthenticated token cannot be obtained before the session exists; SameSite=Lax on the session cookie is the login-CSRF mitigation, documented in the middleware doc). No other route is exempt; adding one requires editing the allowlist (review signal).
- Given `GET /logout`, when the template renders the nav bar, then the `<a href="/logout">` becomes `<form method="POST" action="/logout">` with the hidden token input; the route table drops the GET-method variant so a bare `<img src="/logout">` attack is no longer viable (closes Epic 7 deferred finding)
- Given the middleware wiring in `src/routes/mod.rs::build_router`, when CSRF is added, then the layer order becomes `Logging → Auth (Session extractor) → CSRF → [Handler] → PendingUpdates → CSP` so the CSRF layer runs AFTER Session extraction (it needs `sessions.csrf_token`) but BEFORE handlers (so handlers never see invalid requests); wired via `axum::middleware::from_fn_with_state`
- Given a token lifecycle, when a user logs in / logs out, then the CSRF token rotates (new token on login, old session row soft-deleted on logout so its token is dead); within a logged-in session the token stays stable (no per-request rotation — minimizes HTMX complexity)
- Given a 403 CSRF rejection on HTMX, when returned, then the response includes `HX-Trigger: csrf-rejected` so client-side JS (new listener in `static/js/mybibli.js`) shows a FeedbackEntry "Please refresh the page and retry" — recovers from stale tokens after long idle without a page reload panic
- Unit tests: token generation is 32 bytes base64url (same entropy as session token); middleware accepts matching header, accepts matching form field, rejects missing, rejects mismatch (constant-time); exempt-route list (`/login`) bypasses validation; non-state-changing methods (GET/HEAD/OPTIONS) bypass; `src/templates_audit.rs` integration test adds a form without the hidden input to a test fixture and fails
- Integration tests (`#[sqlx::test]`): login persists `csrf_token`; logout POST invalidates it; language-toggle POST requires it; a second /login from the same browser rotates the token and the old token no longer validates
- E2E smoke: login → navigate to a page with a form → submit with valid token succeeds → tamper the token in DevTools / via `page.evaluate` to a wrong value → resubmit → verify 403 + the "refresh and retry" feedback; GET /logout returns 405 Method Not Allowed (or 404); hitting `/admin` routes with a valid admin session but a stripped token returns 403 (not 500)
- Documentation: `CLAUDE.md` "Key Patterns" section gains a "CSRF token (story 8-2)" bullet describing the token source-of-truth (`sessions.csrf_token`), the template contract (`BaseContext.csrf_token` + base.html meta + form input), the HTMX header contract (`X-CSRF-Token` via csrf.js), and the exempt-route allowlist (`/login` only); architecture Authentication & Security section gets the synchronizer-token diagram

**Out of scope (explicit):**
- Token rotation per-request (stays stable within a session — we would burn HTMX UX for minimal threat-model benefit on a single-user NAS)
- Double-submit cookie pattern (the synchronizer-token pattern is simpler given we already persist sessions server-side)
- CSRF protection for `GET` side-effects that are known to exist (there are none; `GET /logout` is being removed by this story; any future GET-with-side-effect is a separate bug)

#### Story 8.3: User administration
**As an** admin, **I want** to create, edit, deactivate, and reactivate user accounts and assign roles (Librarian, Admin), **so that** I control who can access the app and at what privilege level.

**FRs:** FR68

**Acceptance Criteria:**
- Given the Users tab, when it renders, then it shows a paginated list (25 / page per NFR39) of users with columns: username, role, status (active / deactivated), created date, last login — sorted by username ascending, with filters for role and status
- Given the Users tab, when "New user" is clicked, then a modal (UX-DR8 guard, inherits scanner-guard from story 7-5) opens with fields: username (required, unique, validated server-side), password (required, min 8 chars), role (select: Librarian / Admin), optional full name
- Given an existing user row, when "Edit" is clicked, then a modal opens with fields pre-filled; the password field is blank and is only updated if a new value is submitted (empty input leaves the stored hash untouched)
- Given an existing user, when "Deactivate" is clicked and confirmed, then `users.deleted_at` is set (soft-delete — no hard delete), all of that user's active sessions are invalidated immediately, and the row disappears from the default list; a "Show deactivated" filter restores visibility
- Given a deactivated user, when "Reactivate" is clicked, then `deleted_at` is cleared and the user can log in again on the next attempt
- Given an admin tries to deactivate their own account, when the confirm is submitted, then the server returns 409 Conflict ("Cannot deactivate your own account") rendered via AppError → FeedbackEntry
- Given the last remaining active admin tries to deactivate themselves OR demote their role to Librarian, when submitted, then the server returns 409 Conflict ("At least one active admin must remain") — counted via `COUNT(*) WHERE role='admin' AND deleted_at IS NULL`
- Given a password change, when submitted, then it is hashed through the same argon2 chain established by story 1-9 (minimal-login) — no custom hashing in this story
- Given the form, when it validates, then username uniqueness is checked against active AND deactivated users (reactivating a deactivated username is allowed; creating a new user with a deactivated username is blocked to avoid audit confusion)
- i18n: EN + FR for all field labels, validation errors, and confirm-modal copy
- Unit tests: uniqueness constraint across active + deactivated; last-admin guard (deactivate + demote); self-deactivate guard; password-hash round-trip on edit (empty input leaves hash unchanged); session invalidation on deactivate
- E2E: admin logged in → Users tab → create librarian (verify 25/page list) → log out → log in as new librarian → verify librarian access scope → log back as admin → deactivate librarian → attempt librarian login → verify rejected → reactivate → verify login works; attempt self-deactivate → verify 409 feedback

#### Story 8.4: Reference data management
**As an** admin, **I want** to configure the lists of genres, volume states, contributor roles, and location node types used across the catalog, **so that** the taxonomy matches my library's needs and I can evolve it over time.

**FRs:** FR70, FR71, FR72, FR73, FR91, FR100
**NFRs:** NFR41
**UX-DRs:** UX-DR21

**Acceptance Criteria:**
- Given a fresh install (or migration run on an empty DB), when the app starts for the first time, then default reference data is seeded via migration: genres (fiction, essai, BD, manga, jeunesse, documentaire, poésie, théâtre), volume states (neuf, très bon, bon, moyen, mauvais — with `loanable` flag true/true/true/true/false), contributor roles (auteur, illustrateur, traducteur, préfacier, éditeur scientifique), location node types (bibliothèque, étagère, rayon, case) — seed is idempotent (re-running the migration does not duplicate rows)
- Given the Reference Data tab, when it renders, then it shows 4 sub-sections (one per entity type) each using the `InlineForm` component (UX-DR21): list of current entries, add-new input (Enter to save, Escape to cancel), rename (click-to-edit), delete (icon button) — all sub-sections follow the same keyboard + HTMX behavior
- Given the Volume States sub-section, when a row renders, then it additionally shows a "Loanable" checkbox; toggling it persists immediately via HTMX POST; if the admin disables `loanable` on a state that currently covers at least one volume on an active loan, then a confirm modal lists the affected loans before applying — confirming applies the change AND does NOT auto-return active loans (state change is forward-only)
- Given a user attempts to delete a reference entry that is currently assigned to at least one entity (e.g., genre "fiction" assigned to 42 titles), when the delete is submitted, then the server returns 409 Conflict with a message showing the usage count ("Cannot delete: assigned to 42 titles") and a link to the filtered list filtered by that reference value; the entry remains
- Given a rename, when submitted, then the rename is applied in-place on the reference table row (these tables use surrogate integer keys, so dependent rows keep their FK — no cascading update required)
- Given reference data text (genre names, role names, etc.), when rendered in any template or dropdown, then values are shown as-is in the language of entry regardless of UI language — NFR41 explicitly: v1 does not localize reference data
- Given the InlineForm component, when factored, then it lives at `components/inline_form.html` parameterized by: entity label (singular + plural), list endpoint, save endpoint, delete endpoint — reusable outside Epic 8 for future reference tables
- Given the HTMX save/delete endpoints, when called, then they return only the updated row fragment (not the full list), matching existing patterns (`HtmxResponse` + OOB swap for counters)
- CSP compliance: all checkboxes + inline edit UI use `data-action="..."` delegated handlers — no `onclick=`, no `<style>` inline
- Unit tests: seed idempotency (run seed migration twice, expect same row count); delete guard with usage count per entity type (4 cases); loanable toggle with active-loan detection; rename round-trip (FK referential integrity preserved)
- E2E: admin → Reference Data → add a genre "science-fiction" → verify it appears in the title edit dropdown (EN and FR UI both show "science-fiction" unchanged) → assign genre to a title → attempt to delete "science-fiction" → verify 409 + usage count + link → remove from title → delete genre → verify gone; toggle a volume state from loanable to not-loanable with an active loan → verify warning modal → confirm → verify change applied

#### Story 8.5: System settings
**As an** admin, **I want** to configure application-wide settings (overdue loan threshold, metadata provider API keys, default language), **so that** I can tune behavior without redeploying and updates take effect without restart.

**FRs:** FR74, FR75
**ARs:** AR9

**Acceptance Criteria:**
- Given the System tab, when it renders, then it shows a form with: overdue loan threshold (integer, days, default 30), per-provider API key fields (one row per provider enumerated by `metadata/` — currently BnF, Google Books), default app language (FR / EN per story 7-3), and a "Save" button per logical group
- Given a setting is saved, when the handler completes, then the `settings` table row is updated (optimistic locking via `version` per services/locking.rs) AND the `Arc<RwLock<AppSettings>>` cache held by `AppState` is invalidated + reloaded so the new value takes effect on the next request without restart — AR9 is the load-bearing requirement here
- Given API key fields on read, when rendered, then existing values are shown masked (e.g., `••••••••ab12` — last 4 chars only); the unmasked value is never sent to the browser; on submit, an unchanged field (masked value returned verbatim) is detected server-side and skipped (no re-write, no re-hash)
- Given an invalid value (e.g., negative overdue threshold, or `0`), when saved, then the server returns 400 BadRequest with a field-level error rendered via FeedbackEntry, the stored value stays, and the form re-renders with the invalid input preserved for correction
- Given concurrent admin edits on the same setting row, when optimistic locking detects a conflict (`WHERE version = ?` affected-rows = 0), then the second save returns 409 Conflict with a message "Settings were modified by another admin — reload and retry" (reuses services/locking.rs error path)
- Given the overdue threshold is changed from 30 to 14, when the next `/loans` render runs, then loans with age > 14 days are flagged overdue (threshold is computed at query time, not stored denormalized — no migration, no background recalc)
- Given API keys, when stored in `settings`, then they are stored in plaintext (no encryption at rest — this is documented as an accepted trade-off in the architecture because of NFR37: single-host local deployment, no network egress except metadata fetches)
- Given the language default change, when saved, then it affects only new anonymous sessions with no `lang=` cookie (per story 7-3 locale resolution chain) — existing users' preferences are untouched
- Unit tests: save → subsequent `AppSettings` read returns new value without process restart; optimistic-locking conflict path; API key masking on read; masked-value-unchanged detection on write
- E2E: admin → System → change overdue threshold from 30 to 14 → save → navigate to `/loans` → verify a loan aged 20 days is now flagged overdue (was not before) → change back to 30 → verify no longer flagged; enter a fake BnF key → save → reload → verify masked → clear input and re-save → verify cleared; trigger concurrent save via two browser tabs → verify second returns 409 feedback

#### Story 8.6: Trash view and restore
**As an** admin, **I want** to see all soft-deleted items across the app and restore any of them within the 30-day retention window, **so that** accidental deletions are recoverable without restoring from a DB backup.

**FRs:** FR110, FR111

**Acceptance Criteria:**
- Given the Trash tab, when it renders, then it shows a paginated list (25 / page per NFR39) of all soft-deleted items across entity types with columns: item name (title / volume / contributor / borrower / location / series / genre / etc.), entity type, deletion date, days remaining before purge (`30 - DATEDIFF(NOW(), deleted_at)`, floored at 0) — sorted by most-recently-deleted first
- Given the Trash query, when it runs, then it UNIONs SELECTs across all tables enumerated in the `services/soft_delete.rs` whitelist — no table is queried that is not whitelisted (the query is generated from the whitelist to keep them in sync)
- Given the Trash filter bar, when the admin filters by entity type (dropdown) or searches by name (debounced HTMX input), then only matching rows are shown; filters combine (type AND name)
- Given an item row, when "Restore" is clicked, then the server clears `deleted_at`, bumps `version`, and returns a FeedbackEntry "Restored: {name}" — the row is removed from the Trash list via OOB swap; the Trash badge count in the tab bar decrements via OOB swap too
- Given a restore request, when the entity has associations that changed during the item's soft-delete (e.g., a volume's series position was reassigned to a different volume, a title's genre was renamed or deleted, a contributor was merged), when processed, then the server detects the conflict and returns a conflict-resolution modal listing: which associations cannot be re-established verbatim, and an explicit choice — "Restore with conflicts cleared" (nullifies the conflicting FKs and restores the entity) or "Cancel"
- Given a restore is attempted on an item whose parent was hard-purged (e.g., a volume whose parent title was permanent-deleted — scenario only possible after story 8-7 ships), when processed, then the server returns 409 Conflict ("Parent no longer exists — cannot restore") rendered via FeedbackEntry
- Given concurrent admin sessions, when admin A restores an item that admin B has open in their Trash, then admin B's list refresh (on next OOB sweep or manual reload) no longer shows the restored item
- CSP compliance: conflict modal uses the UX-DR8 modal component (inherits scanner-guard from 7-5), no inline handlers
- Unit tests: UNION query covers every whitelisted table and only whitelisted tables; series-position conflict detection; reference-data rename/delete conflict detection; parent-hard-purge detection; `version` bump on restore
- E2E: admin → catalog → soft-delete a title → Trash tab → verify title appears with correct days-remaining + type badge → click Restore → verify title back in catalog, gone from Trash, Trash badge decremented; delete a series with 3 titles → reassign 1 title to another series → attempt restore of original series → verify conflict modal lists the reassigned title → "Restore with conflicts cleared" → verify original series restored with 2 titles (the 3rd stays on the new series)

#### Story 8.7: Permanent delete and auto-purge
**As an** admin, **I want** soft-deleted items to be hard-purged automatically after 30 days and to be able to force permanent deletion sooner from the Trash, **so that** storage stays bounded and items I am certain about are definitively gone.

**FRs:** FR112, FR113

**Acceptance Criteria:**
- Given a Trash row, when "Delete permanently" is clicked, then a confirmation modal opens (UX-DR8 guard + scanner-guard 7-5) with the item name, an explicit warning ("This cannot be undone"), and an input requiring the admin to **type the item name verbatim** to enable the Confirm button (friction pattern matching destructive-action UX)
- Given confirmation, when submitted, then the row is hard-deleted from its table (`DELETE FROM ... WHERE id = ? AND version = ?`), dependent rows are handled according to each table's FK policy (documented in architecture per table — some cascade, some RESTRICT), a row is appended to the `admin_audit` table (who, what entity + id, when — migration creates this table in this story), and the Trash list OOB-swaps the row out
- Given the app boots, when the startup task runs (synchronous, blocking `/admin` + `/catalog` render until it completes — bounded by count), then any row with `deleted_at < NOW() - INTERVAL 30 DAY` across every whitelisted table is hard-purged — results are logged at info level with per-table counts, and the `admin_audit` table records a single "auto-purge" entry per run
- Given a daily scheduled task (`tokio::spawn` + `tokio::time::interval(24h)` started from `main.rs`), when it fires (first run 24h after boot, configurable via `settings.auto_purge_interval_seconds` default 86400), then the same 30-day purge runs — same audit log
- Given a hard-purge runs, when it processes a table, then it respects FK dependencies by deleting in the order defined by the whitelist's declared dependency graph (children first, then parents) and uses a single transaction per entity family so partial failures don't leave orphans; on FK violation it rolls back, logs the error, and continues to the next family
- Given the auto-purge encounters an error (FK violation, DB unavailable, lock timeout), when it fails, then it logs an error but does NOT abort app startup and does NOT crash the daily interval task; the next scheduled run retries
- Given permanent delete is attempted on an item that no longer exists in Trash (e.g., already permanent-deleted by another admin, or auto-purged), when submitted, then the server returns 404 NotFound ("Item already gone") rendered via FeedbackEntry
- Guards: admin cannot permanent-delete themselves, cannot permanent-delete the last active admin user (same rules as story 8-3 but enforced here too); cannot permanent-delete a non-soft-deleted item (hitting this endpoint bypasses the Trash should return 400)
- Unit tests: 30-day boundary (row at 29d stays, row at 31d purged); FK dependency ordering generates correct DELETE sequence; idempotency (running purge twice on empty Trash does not error); last-admin guard on permanent delete; `admin_audit` row shape
- E2E: admin → Trash → a soft-deleted title → "Delete permanently" → verify friction modal requires typing the title name → confirm → verify gone from Trash AND not recoverable (no DB row); boot app with a seeded 31-day-old soft-deleted fixture → verify auto-purge removed it AND `admin_audit` has a row AND logs show the count

#### Story 8.8: First-launch setup wizard
**As a** first-time user installing mybibli, **I want** a setup wizard that guides me through creating the admin account and initial configuration, **so that** I can start using the app without editing migrations or seed files by hand, and resuming after an interruption does not destroy what I already entered.

**FRs:** FR87, FR121
**UX-DRs:** UX-DR20

**Acceptance Criteria:**
- Given a fresh install (no user with role `admin` exists AND `settings.setup_completed_at` IS NULL), when any route is requested, then the setup middleware intercepts and redirects to `/setup` — the wizard takes over the entire session until completion (except `/static/*` assets and `/healthz`)
- Given `/setup`, when it renders, then it shows a 4-dot progress indicator (Admin → Providers → Preferences → Done), Previous / Next buttons, and one panel per step; the current step is determined server-side (not client-side) from the resume-detection logic below
- Step 1 — Admin account: Given step 1, when the admin submits username + password (min 8 chars) + optional full name, then an admin user is created, the session is authenticated as this user, and the wizard advances to step 2
- Step 2 — Providers: Given step 2, when rendered, then it lists each provider enumerated by `metadata/` with an optional API key input and a "Skip" checkbox per provider — submitting writes the keys to `settings` (plaintext per 8-5 trade-off) and advances; no key is mandatory (the app works without any key, using anonymous provider access)
- Step 3 — Preferences: Given step 3, when rendered, then it shows default language (FR / EN radio) and overdue threshold (integer input, default 30) — submitting writes to `settings` and advances
- Step 4 — Done: Given step 4, when rendered, then it shows a read-only recap of the choices made (admin username, providers with keys set — masked, language, threshold) and a "Complete setup" button — clicking writes `settings.setup_completed_at = NOW()` and redirects to `/catalog`
- Idempotent resume (FR121): Given the admin interrupts the wizard after step 2 (closes the browser), when they restart the app and hit any route, then the setup middleware detects: admin user exists AND `setup_completed_at` IS NULL AND at least one provider key set → resumes at step 3, showing the partially-entered state in editable form (not a blank form); step 1 resume re-uses the existing admin row in edit mode rather than creating a duplicate
- Given the wizard has completed once (`setup_completed_at` IS NOT NULL), when any user later navigates to `/setup`, then the server returns 404 NotFound (the wizard is first-launch-only; ongoing config happens via `/admin`)
- Given the E2E / dev environment needs to bypass the wizard to test other features, when `MYBIBLI_SKIP_SETUP=true` env var is set, then the setup middleware is bypassed and routes render normally — documented in CLAUDE.md, used by the Playwright seed chain
- CSP compliance: the wizard's step transitions and progress-dot highlighting use `data-*` attributes + delegated handlers — no inline scripts or styles
- i18n: every label, button, validation error, and recap in EN + FR; the wizard respects `Accept-Language` on first load (before the user picks a language in step 3)
- Unit tests: resume-detection decides the correct landing step for every partial state (no admin, admin-only, admin + providers, admin + providers + prefs); `setup_completed_at` gates the 404 path; `MYBIBLI_SKIP_SETUP` bypass
- E2E smoke (Foundation Rule #7): start the app with a clean DB (no admin, no `setup_completed_at`) → navigate to `/catalog` → verify redirect to `/setup` step 1 → complete steps 1-4 → verify redirect to `/catalog` → verify `/setup` now returns 404 → verify the new admin can log out and log back in; second test: start at step 3 partial state, close browser, restart app → verify resumes at step 3 with admin username + providers shown in edit mode (not blank)

### Epic 9: Polish UX & Accessibilité
The dashboard shows actionable indicators with counts. Every page has encouraging empty states. Contextual help and keyboard shortcuts are complete. Responsive layouts are optimized per page. The home page scanner state machine handles dual detection. Modals guard destructive actions. WCAG 2.2 AA compliance is verified end-to-end.

**FRs:** FR55-FR59, FR83-FR84
**UX-DRs:** UX-DR4, UX-DR6 (complete — roles, hamburger, scanner auto-close), UX-DR8, UX-DR13, UX-DR26, UX-DR28

**Scope note — split philosophy (2026-04-30):** Epic 9 is decomposed into 22 small, independently shippable stories rather than the original 9. The two patterns that drive the count are (a) **incremental indicator delivery** — story 9.4 lands the FilterTag component end-to-end with the first indicator (unshelved volumes); stories 9.5/9.6/9.7 add the remaining four indicators on the same plumbing as small follow-ups; and (b) **one PR per `hx-confirm=` migration** — story 9.10 lands the Modal component foundation with the first migration (delete borrower); stories 9.11–9.14 each migrate one of the four remaining grandfathered sites, emptying `ALLOWED_HX_CONFIRM_SITES` to `&[]` at 9.14 close. Choosing many small stories over fewer large ones trades retro/review overhead for tighter blast-radius and clearer per-story acceptance — appropriate for an Epic that is fundamentally polish + completion of partial patterns rather than new feature surface.

**Stories:**

#### Story 9.1: Dashboard — global stats card
**As any** user (anonymous or authenticated), **I want** to see global collection stats on the home page, **so that** I get an immediate sense of the catalog's size at a glance.

**FRs:** FR55

**Acceptance Criteria:**
- Given the home page (`/`), when it renders for any role, then a "Collection at a glance" card displays three counts: total active titles, total active volumes, total active loans — each excluding `deleted_at IS NOT NULL` rows
- Given the counts, when computed, then they come from a single SQL round-trip (one query with three sub-counts via UNION ALL or three SELECTs in a single transaction) — no N+1, no per-entity lookup
- Given a fresh DB, when the counts are zero, then the card still renders with "0 titles", "0 volumes", "0 loans" — no empty-state hiding (the card is always present; the StatusMessage empty-state for an empty catalog is a separate concern handled by 9.15)
- Given the card, when rendered, then each count line is a clickable link: title count → `/catalog`, volume count → `/catalog?view=volumes` (or equivalent existing route), loan count → `/loans` for librarian/admin, and rendered as plain text with a tooltip "Sign in to view loans" for anonymous users (since `/loans` is gated)
- Given the role-aware link generation, when tested, then anonymous users never receive a link to `/loans` in the rendered HTML (regression guard against accidental over-rendering)
- CSP compliance: card uses Tailwind classes only, no inline styles
- i18n: EN + FR labels ("Collection at a glance" / "Aperçu de la collection", "titles" / "titres", "volumes", "active loans" / "prêts en cours")
- Unit tests: count query returns correct values across scenarios (empty DB, mixed soft-deleted/active); role-aware link generation; soft-delete exclusion
- E2E smoke: anonymous → `/` → verify card with three counts, loan count is plain text; login as librarian → `/` → verify loan count becomes a link → click → `/loans` opens

#### Story 9.2: Dashboard — recent additions
**As any** user, **I want** to see the most recent titles added to the catalog on the home page, **so that** I can quickly browse what is new.

**FRs:** FR56

**Acceptance Criteria:**
- Given the home page, when it renders, then a "Recent additions" section shows the 10 most recently created active titles (sorted by `created_at DESC`, excluding soft-deleted), each displayed via the existing TitleCard component (UX-DR17, list-mode by default)
- Given the section, when fewer than 10 titles exist, then only the existing titles are shown (no padding); if zero titles exist, the section reuses the StatusMessage empty-state (story 9.15) instead of disappearing entirely
- Given a TitleCard, when clicked, then it navigates to `/title/:id` (existing route)
- Given the query, when computed, then it runs as a single SELECT with `LIMIT 10` and joins what is needed for TitleCard rendering (primary contributor, cover URL) — no per-row N+1 metadata fetch; missing covers fall back to UX-DR10 placeholder
- Given the section is visible to anonymous users, when rendered, then no role-gated information leaks (the query SELECTs only public columns)
- CSP compliance: TitleCard is already CSP-clean from Epic 1, no new inline markup
- i18n: EN + FR for section heading ("Recent additions" / "Ajouts récents")
- Unit tests: query returns correct LIMIT and ORDER, soft-deleted titles excluded; query is role-agnostic (no SQL branch on role)
- E2E: anonymous → `/` → verify Recent additions section with up to 10 cards → click first card → verify `/title/:id` opens; create a new title → reload `/` → verify it appears at the top

#### Story 9.3: Dashboard — stats by genre
**As any** user, **I want** to see the catalog distribution by genre, **so that** I can understand the composition of the library.

**FRs:** FR57

**Acceptance Criteria:**
- Given the home page, when it renders, then a "By genre" section displays a list of all genres assigned to at least one active title, with: genre name, title count for that genre, percentage of total active titles with at least one genre (rounded to 1 decimal) — sorted by count descending
- Given the query, when computed, then it joins `titles` with `genres` (or the title↔genre association table) and aggregates with `COUNT(*) GROUP BY genres.id WHERE titles.deleted_at IS NULL AND genres.deleted_at IS NULL` — single round-trip, no N+1
- Given a row, when clicked, then it navigates to `/catalog?genre=<id>` showing only titles of that genre (filter route established by Epic 1 search)
- Given the section, when no genres are assigned anywhere (fresh install), then the section is hidden entirely; the broader empty-catalog UX is handled by StatusMessage in 9.15
- Given a title with no genre, when counts are computed, then it is NOT counted (we count active title-genre assignments, not active titles); the percentage denominator is "active titles with at least one genre" so percentages always sum to 100% across the displayed list
- Given dark mode, when rendered, then the bar/visual uses tokens from UX-DR24 (no hardcoded colors)
- CSP compliance: bar visualization uses predefined Tailwind width classes (e.g., `w-1/4`, `w-1/3`, or arbitrary-value classes via Tailwind v4 — class-based not `style="width: ..."`)
- i18n: EN + FR for section heading ("By genre" / "Par genre"); count + percentage formatting respects locale (FR uses comma decimal separator)
- Unit tests: aggregation query (group + count + percent rounding); soft-delete exclusion; genre with 0 active titles excluded from results
- E2E: seed catalog with mixed genres → `/` → verify By genre section sorted by count desc → click a genre row → verify `/catalog?genre=<id>` filter applies

#### Story 9.4: FilterTag component + first actionable indicator (unshelved volumes)
**As a** librarian, **I want** to see the count of unshelved volumes as a clickable tag on the home page, **so that** I can immediately jump to the list of volumes that need shelving.

**FRs:** FR58 (partial — unshelved indicator)
**UX-DRs:** UX-DR4

**Acceptance Criteria:**
- Given the home page seen by a librarian or admin, when it renders, then a "What needs attention" section displays one or more FilterTag pills; this story delivers the first one — "Unshelved volumes — N" where N is the count of active volumes whose `storage_location_id IS NULL`
- Given the count is zero, when computed, then the tag is hidden entirely (UX-DR4 zero-count rule); the section heading also hides if all tags are zero
- Given the tag is clicked, when activated, then the URL becomes `/?filter=unshelved` and the home page swaps the Recent additions section for a filtered list of the unshelved volumes; the tag morphs into the active filter badge (pill with ✕) per UX-DR4 dual-state
- Given the active filter badge, when its ✕ is clicked, then the URL returns to `/` and the home page restores the default sections (stats card, recent additions, by genre) — HTMX swap only, no full page reload
- Given the FilterTag component, when factored, then it lives at `components/filter_tag.html` parameterized by: label, count, target URL, active-state flag — reusable for the 4 remaining indicators (9.5/9.6/9.7) without modification
- Given URL filter parsing, when handled server-side, then `?filter=<name>` is a closed enum (case-sensitive); unknown values fall back to no-filter (no 400, just ignored) and a warning is logged
- Given the single-active-filter constraint, when one filter is active and another is clicked, then the new filter replaces the old (no AND/OR composition in v1) — matches UX-DR4
- Given an anonymous user crafts `/?filter=unshelved`, when handled, then the filter is ignored (no role-gated leak) and default sections render — anonymous users do not see the "What needs attention" section at all
- CSP compliance: FilterTag is class-driven, no inline styles; clickable behavior is via plain `<a href>` links (HTMX boost-style for partial swap, full-page navigation as JS-disabled fallback)
- i18n: EN + FR for "What needs attention" / "À traiter", "Unshelved volumes" / "Volumes à ranger"
- Unit tests: FilterTag rendering (default + active states); zero-count hiding; unshelved query (active volumes WHERE `storage_location_id IS NULL AND deleted_at IS NULL`); URL filter enum parsing
- E2E smoke (Foundation Rule #7, librarian role): blank browser → login → `/` → verify Unshelved tag with non-zero count → click → verify URL `/?filter=unshelved` and list shows only unshelved → click ✕ → verify default home; create a volume without location → reload → verify count incremented

#### Story 9.5: Indicator — overdue loans
**As a** librarian, **I want** an overdue loans tag on the home page, **so that** I can quickly see and address late returns.

**FRs:** FR58 (partial — overdue indicator)
**UX-DRs:** UX-DR4 (reuses FilterTag from 9.4)

**Acceptance Criteria:**
- Given the home page seen by a librarian or admin, when it renders, then the "What needs attention" section additionally displays an "Overdue loans — N" FilterTag where N is the count of active loans whose age in days exceeds the configured `overdue_threshold` (from `AppSettings`, default 30)
- Given the count is zero, when computed, then the tag is hidden (UX-DR4 zero-count rule)
- Given the tag is clicked, when activated, then the URL becomes `/?filter=overdue` and the home page swaps the default sections for a list of overdue loans — each row uses the LoanRow variant of UX-DR5 with its existing duration color coding
- Given the overdue threshold is changed via `/admin/system` (story 8-5), when the home page is reloaded, then the count reflects the new threshold immediately (read-from-cache pattern via `state.overdue_threshold_days()`)
- Given the query, when computed, then it filters `loans WHERE returned_at IS NULL AND DATEDIFF(NOW(), borrowed_at) > <threshold>` — single round-trip, indexed on `(returned_at, borrowed_at)` if not already
- Given an anonymous user crafts `/?filter=overdue`, when handled, then the filter is ignored (loans are Librarian-gated content)
- Given the StatusMessage empty-state when filter is applied but list is empty, when rendered, then it reuses the StatusMessage component from 9.15 with copy "No overdue loans — well done!" / "Aucun prêt en retard — bien joué !"
- CSP compliance: reuses FilterTag from 9.4, no new inline markup
- i18n: EN + FR for "Overdue loans" / "Prêts en retard"
- Unit tests: query with threshold parameterization; threshold change reflected without restart (uses live `AppSettings`); zero-count hiding
- E2E (librarian): login → `/` → verify Overdue tag with count → click → verify URL `/?filter=overdue` and only overdue loans listed → adjust threshold via `/admin?tab=system` → reload `/` → verify count updated

#### Story 9.6: Indicator — series with gaps
**As a** librarian, **I want** a series-with-gaps tag on the home page, **so that** I can see at a glance how many series are incomplete and plan acquisitions.

**FRs:** FR58 (partial — gaps indicator)
**UX-DRs:** UX-DR4 (reuses FilterTag from 9.4)

**Acceptance Criteria:**
- Given the home page seen by a librarian or admin, when it renders, then the "What needs attention" section additionally displays a "Series with gaps — N" FilterTag where N is the count of active series for which the gap detection logic (Epic 5 story 5-4) reports at least one missing position
- Given the count is zero, when computed, then the tag is hidden (zero-count rule)
- Given the tag is clicked, when activated, then the URL becomes `/?filter=gaps` and the home page swaps the default sections for the list of series with gaps — each row is a SeriesCard (existing component from Epic 5) showing the gap count and a SeriesGapGrid preview (UX-DR16)
- Given the gap-count query, when computed, then it reuses the existing series-with-gaps service function from `src/services/series.rs` (extracted in Epic 5) — no SQL duplication; if the function is private, it is made `pub(crate)` in this story
- Given series of type "open" (no declared total), when evaluated, then they are NEVER counted as having gaps (open series have no defined "completeness")
- Given series of type "closed" with declared total N, when evaluated, then a gap exists if any position 1..N is unowned
- Given an anonymous user crafts `/?filter=gaps`, when handled, then the filter is allowed (series browsing is anonymous-permitted per FR65) — but the FilterTag itself is hidden from anonymous on `/`
- CSP compliance: reuses FilterTag, no new inline markup
- i18n: EN + FR for "Series with gaps" / "Séries incomplètes"
- Unit tests: gap-count query (open vs closed series); zero-count hiding; reuse of existing service function (not a re-implementation)
- E2E (librarian): seed a closed series of 5 with positions 1, 2, 4 owned → login → `/` → verify Series-with-gaps count = 1 → click → verify list shows that series with gap markers at positions 3 and 5

#### Story 9.7: Indicators — recent cataloged + recent returns
**As a** librarian, **I want** to see how many titles I have just cataloged and loans I have just returned, **so that** I can review the most recent activity in one click.

**FRs:** FR58 (partial — recent cataloged + recent returns)
**UX-DRs:** UX-DR4 (reuses FilterTag from 9.4)

**Acceptance Criteria:**
- Given the home page seen by a librarian or admin, when it renders, then the "What needs attention" section displays two additional FilterTags: "Recent cataloged — N" (active titles `created_at >= NOW() - INTERVAL 7 DAY`) and "Recent returns — N" (loans `returned_at >= NOW() - INTERVAL 7 DAY`)
- Given the 7-day window, when computed, then the cutoff is hardcoded in v1 (not admin-configurable per scope freeze) — documented in CLAUDE.md as a known constant; if the user later requests configurability, it becomes a settings story
- Given a count is zero, when computed, then that tag is hidden (zero-count rule)
- Given "Recent cataloged" is clicked, when activated, then the URL becomes `/?filter=recent-cataloged` and the home page swaps to a TitleCard list ordered by `created_at DESC` within the 7-day window
- Given "Recent returns" is clicked, when activated, then the URL becomes `/?filter=recent-returns` and the home page swaps to a list of loans (LoanRow) ordered by `returned_at DESC` within the 7-day window
- Given anonymous users craft these URLs, when handled, then the filter is ignored (recent activity is Librarian-gated)
- Given the section ordering, when all 5 tags are rendered together, then the visual order is: Unshelved → Overdue → Series with gaps → Recent cataloged → Recent returns (priority by actionability — "needs action" before "review")
- CSP compliance: no new inline markup
- i18n: EN + FR for "Recent cataloged" / "Catalogués récemment", "Recent returns" / "Retours récents"
- Unit tests: 7-day cutoff query for both indicators; zero-count hiding; tag ordering across all 5 indicators
- E2E (librarian): seed a title created today + a loan returned today → login → `/` → verify both tags present → click each → verify the respective filtered list

#### Story 9.8: Loan status role-aware on volume detail
**As an** anonymous user, **I want** to see whether a volume is on loan without seeing the borrower's name, **so that** privacy is preserved while I can still tell whether the item is currently available.

**FRs:** FR59

**Acceptance Criteria:**
- Given the volume row on `/title/:id` (or any volume-detail rendering), when an anonymous user views it, then the loan status displays as "On loan" / "En prêt" with no borrower name, no borrower link, and no return-date hint
- Given the same view, when a librarian or admin views it, then the loan status displays "On loan to {borrower name} since {date}" / "En prêt à {nom} depuis le {date}" with the borrower name as a clickable link to `/borrower/:id`
- Given a volume that is not on loan, when rendered, then the same field shows the existing VolumeBadge (UX-DR15) for shelved / unshelved — unchanged behavior for any role
- Given the templating, when factored, then the role-aware split lives in a single shared partial (`components/loan_status_badge.html`) parameterized by `role` so any caller (volume row, title detail, search result) renders consistently — no duplicated role check across templates
- Given the SQL that drives volume detail, when fetching loan info, then for an anonymous request the query SELECTs only the existence of an active loan + `borrowed_at` (no JOIN to `borrowers`); for librarian/admin the JOIN is added — minimizes data leak surface
- Given the role split, when tested, then the rendered HTML for an anonymous request is byte-asserted to NOT contain the borrower's name (regression guard against accidental over-rendering)
- CSP compliance: no inline markup
- i18n: EN + FR for both role paths
- Unit tests: anonymous query path returns no borrower data; librarian query path returns borrower data; component parameterization renders correct variant per role; HTML-name-leak regression guard
- E2E: seed a volume with an active loan to "Alice" → anonymous → `/title/<id>` → verify "On loan" without "Alice" appearing anywhere in the HTML → login as librarian → reload → verify "On loan to Alice" with borrower link

#### Story 9.9: Home page scanner detection state machine
**As a** librarian on the home page, **I want** the search field to distinguish between human typing (search) and a barcode-scanner burst (scan with intent to navigate), **so that** I can scan from the home page and land on the right page without manually navigating to `/catalog` first.

**UX-DRs:** UX-DR26

**Acceptance Criteria:**
- Given the home page search field, when it receives input, then a 4-state machine governs behavior: IDLE (no recent input), DETECTING (input started, deciding scanner-vs-typing), SEARCH_MODE (typing pace confirmed, debounced search active), SCAN_PENDING (scanner burst detected, awaiting submit)
- Given two independent timers, when the state machine runs, then `scanner_burst_threshold` (default 50ms inter-keypress, hardcoded in v1) classifies a burst, and `search_debounce_delay` (default 150ms after last keypress) triggers as-you-type search; both are JS-side constants documented in `static/js/home-scanner.js` (new module) or the existing `scan-field.js` (extension)
- Given DETECTING is entered on the first keypress, when subsequent keypresses arrive within the burst threshold, then the state advances to SCAN_PENDING; if instead the gap between keypresses exceeds the burst threshold, then the state advances to SEARCH_MODE
- Given SCAN_PENDING is reached and an Enter or final keypress lands within the burst window, when handled, then the input is submitted to the server scan-handler which decides the destination based on prefix detection: ISBN known → `/title/:id`, V-code known → `/volume/:id` (or volume detail), L-code known → `/location/:id`, unknown → redirect to `/catalog?code=<value>` so the cataloging workflow takes over (no creation logic on `/`)
- Given SEARCH_MODE is reached, when input continues, then debounced HTMX as-you-type search runs against the existing catalog search endpoint (Epic 1) and renders results inline below the search field
- Given the user clears the input or blurs the field, when the state machine resets, then it returns to IDLE; the next keypress restarts at DETECTING
- Given the state machine and the existing focus dual mechanism (UX-DR25 `focus.js`), when modeled, then they coexist without cycle (focus.js maintains focus, scanner state machine classifies input — orthogonal concerns)
- Given prefers-reduced-motion or screen-reader users, when interacting, then the state machine still works (it is purely keystroke-timing based, not animated); visual hints are aria-live polite announcements
- CSP compliance: state machine logic ships in an external JS module — no inline scripts
- i18n: EN + FR for any user-facing copy ("Searching..." / "Recherche...", "Scanning..." / "Scan détecté...")
- Unit tests (JS via the existing testing harness): timer thresholds; state transitions for each input pattern (slow typing, scanner burst, mixed); reset on clear/blur
- E2E: home page → simulate scanner burst (helper `simulateScan` from `tests/e2e/helpers/scanner.ts`) of an unknown ISBN → verify redirect to `/catalog?code=...` → home page again → simulate human typing → verify SEARCH_MODE results appear inline → clear input → verify reset

#### Story 9.10: Modal component foundation + migration #1 (delete borrower)
**As the** project maintainer, **I want** a CSP-clean Modal component with focus trap, scanner-guard integration, and 4 destructive variants, plus the first concrete migration (delete borrower) to prove it in production, **so that** subsequent migrations are mechanical and the UX-DR8 contract is exercised end-to-end.

**UX-DRs:** UX-DR8 (foundation + 1st migration)

**Acceptance Criteria:**
- Given the Modal component, when factored, then it lives at `components/modal.html` with parameters: variant (`delete` / `delete-forever` / `remove` / `warning`), title, body (HTML-escaped), confirm-label, cancel-label, action-url, action-method (`DELETE` / `POST`); rendered as a `<dialog>` element with `aria-modal="true"`
- Given the Modal opens, when triggered (via an HTMX `hx-get` that swaps the modal slot, or a click on a `data-modal-trigger` button), then keyboard focus moves to the Cancel button (UX-DR8 default — Cancel never destroys), Tab cycles within the modal only (focus trap), Escape closes the modal restoring focus to the trigger
- Given the scanner-guard from story 7-5 is in effect, when the modal is open (`dialog[open]`, `aria-modal="true"`), then printable keystrokes are routed to the modal's focused text input (if any) or are blocked — never leaking to a background scan field
- Given the modal background, when the modal is open, then background interactive elements get `tabindex="-1"` and `aria-hidden="true"` (focus + AT exclusion) and page scroll is locked
- Given the Confirm button, when activated, then it submits the action via HTMX (`hx-{method}` on the button), the modal closes on success, and a FeedbackEntry is rendered via the standard pipeline (`HtmxResponse` with OOB swaps)
- Given `templates/pages/borrower_detail.html` line 27 (delete borrower), when migrated, then the existing `<button hx-delete=".." hx-confirm="..">` is replaced by a `<button data-modal-trigger=".." data-variant="delete">` that opens a Modal of variant `delete` with copy "Delete borrower {name}? This will move the record to Trash." / "Supprimer l'emprunteur {nom} ? L'enregistrement sera déplacé vers la corbeille." — the `hx-confirm` attribute is removed from this template
- Given the audit allowlist `ALLOWED_HX_CONFIRM_SITES` in `src/templates_audit.rs`, when this story commits, then the entry `("templates/pages/borrower_detail.html", 2)` becomes `("templates/pages/borrower_detail.html", 1)` (the second `hx-confirm=` is the return-loan one, migrated by 9.11) — total grandfathered count goes from 6 to 5
- Given the Modal component, when reusable across all 4 variants, then a smoke unit test validates the rendering of each variant; a JS unit test asserts the focus-trap behavior (Tab cycles, Shift+Tab cycles backwards, focus does not escape)
- Given the existing E2E for borrower deletion (Epic 4), when re-run after migration, then it passes with at most a selector update (server contract DELETE `/borrower/:id` is unchanged)
- CSP compliance: Modal uses `<dialog>` + `data-*` attributes for triggering, no `onclick=`, no inline `style`; CSS-only animations
- i18n: EN + FR for default cancel/confirm labels and the borrower-delete copy
- Unit tests: variant rendering (4 variants); focus trap; Escape closes; scanner-guard integration (a printable burst while modal is open does not reach `#scan-field`)
- E2E: librarian → `/borrower/:id` → click "Delete" → verify Modal opens with focus on Cancel → press Escape → verify closes → click "Delete" again → click Confirm → verify borrower soft-deleted, FeedbackEntry rendered, redirect to /borrowers list; verify the audit test (`cargo test hx_confirm_matches_allowlist`) passes after the migration

#### Story 9.11: Migrate hx-confirm — return loan (loans.html + borrower_detail.html)
**As the** project maintainer, **I want** the two "return loan" confirmation flows (on `/loans` and on borrower detail) migrated from `hx-confirm=` to the Modal component, **so that** the return-loan UX is consistent with the destructive-action pattern and two grandfathered sites are removed in lockstep.

**UX-DRs:** UX-DR8 (migration #2)

**Acceptance Criteria:**
- Given `templates/pages/loans.html` line 123 (`hx-confirm="{{ confirm_label }}"`), when migrated, then it becomes a `data-modal-trigger` button using Modal variant `warning` (return is reversible — the volume can be re-loaned — so this is `warning`, not `delete`); the action remains the existing POST that closes the loan
- Given `templates/pages/borrower_detail.html` line 72 (the second "return loan" confirmation), when migrated, then the same Modal variant `warning` is applied with identical copy
- Given the two migrations share identical copy, when factored, then the Modal trigger pattern is the same (no copy-pasted HTML); the variant + i18n keys are identical across both files
- Given `ALLOWED_HX_CONFIRM_SITES`, when updated, then `("templates/pages/loans.html", 1)` is removed entirely (count → 0, allowlist entry deleted) and `("templates/pages/borrower_detail.html", 1)` is removed (the only remaining occurrence — delete-borrower — was already removed by 9.10, so `borrower_detail.html` exits the allowlist completely); remaining grandfathered sites: 3 (`contributor_detail.html`, `series_detail.html`, `admin_users_row.html`)
- Given the Modal copy for return, when rendered, then EN copy is "Mark loan as returned? The volume will be available again." / FR "Marquer le prêt comme retourné ? Le volume redevient disponible."
- Given the existing E2E for loan-return on `/loans`, when re-run, then it passes (server contract unchanged); same for borrower-detail return flow
- CSP compliance: no new inline markup, reuses 9.10 component
- i18n: EN + FR for return-loan modal copy
- Unit tests: audit test passes with updated allowlist; Modal variant-warning rendering
- E2E: librarian → `/loans` → click "Return" on a loan → verify Modal opens with `warning` variant → confirm → verify loan returned and feedback rendered; same flow from `/borrower/:id` → verify identical behavior

#### Story 9.12: Migrate hx-confirm — delete contributor
**As the** project maintainer, **I want** the delete-contributor flow migrated from `hx-confirm=` to the Modal component, **so that** the destructive-action pattern is enforced and one more grandfathered site is removed.

**UX-DRs:** UX-DR8 (migration #3)

**Acceptance Criteria:**
- Given `templates/pages/contributor_detail.html` line 15 (`<button hx-delete="..." hx-confirm="..." ...>`), when migrated, then it becomes a `data-modal-trigger` button using Modal variant `delete`; the existing FR54 protection (cannot delete a contributor with active title references) remains server-side and still returns 409 Conflict on attempt
- Given the Modal copy, when rendered, then EN copy is "Delete contributor {name}? Linked titles will lose this contributor unless re-assigned." / FR "Supprimer le contributeur {nom} ? Les titres liés perdront ce contributeur sauf s'il est réassigné."
- Given `ALLOWED_HX_CONFIRM_SITES`, when updated, then `("templates/pages/contributor_detail.html", 1)` is removed (count → 0); remaining grandfathered sites: 2 (`series_detail.html`, `admin_users_row.html`)
- Given the existing E2E for contributor delete, when re-run, then it passes (server contract unchanged)
- CSP compliance: no new inline markup
- i18n: EN + FR for delete-contributor modal copy
- Unit tests: audit test passes with updated allowlist
- E2E: librarian → `/contributor/:id` (no title references) → click "Delete" → Modal opens → confirm → verify soft-deleted, redirect; same with active references → verify 409 feedback (no Modal regression on the conflict path)

#### Story 9.13: Migrate hx-confirm — delete series
**As the** project maintainer, **I want** the delete-series flow migrated from `hx-confirm=` to the Modal component, **so that** the destructive-action pattern is enforced and one more grandfathered site is removed.

**UX-DRs:** UX-DR8 (migration #4)

**Acceptance Criteria:**
- Given `templates/pages/series_detail.html` line 35, when migrated, then it becomes a `data-modal-trigger` button using Modal variant `delete`; existing protections (cannot delete a series with assigned titles, or whatever Epic 5 enforces) remain server-side
- Given the Modal copy, when rendered, then EN copy is "Delete series {name}? Assigned titles must be re-attached or detached first." / FR "Supprimer la série {nom} ? Les titres associés doivent être détachés ou réaffectés au préalable."
- Given `ALLOWED_HX_CONFIRM_SITES`, when updated, then `("templates/pages/series_detail.html", 1)` is removed (count → 0); remaining grandfathered sites: 1 (`admin_users_row.html`)
- Given the existing E2E for series delete, when re-run, then it passes
- CSP compliance: no new inline markup
- i18n: EN + FR for delete-series modal copy
- Unit tests: audit test passes with updated allowlist
- E2E: librarian → `/series/:id` (empty series) → "Delete" → Modal → confirm → verify soft-deleted; with assigned titles → verify 409 feedback

#### Story 9.14: Migrate hx-confirm — admin user deactivation (final cleanup)
**As the** project maintainer, **I want** the admin-user-deactivate flow migrated from `hx-confirm=` to the Modal component, **so that** UX-DR8 is fully implemented and the grandfathered allowlist is empty — the constraint becomes "no `hx-confirm=` anywhere in templates."

**UX-DRs:** UX-DR8 (final migration)

**Acceptance Criteria:**
- Given `templates/fragments/admin_users_row.html` line 23, when migrated, then the deactivation button becomes a `data-modal-trigger` using Modal variant `delete`; the existing self-deactivate guard + last-active-admin guard logic (story 8-3) is preserved server-side
- Given the Modal copy, when rendered, then EN copy is "Deactivate user {username}? They will be logged out immediately and cannot log back in until reactivated." / FR "Désactiver l'utilisateur {nom} ? Sa session sera fermée immédiatement et il ne pourra plus se reconnecter avant réactivation."
- Given `ALLOWED_HX_CONFIRM_SITES`, when updated, then `("templates/fragments/admin_users_row.html", 1)` is removed and the constant becomes an empty slice `&[]`; the test continues to fail on any new `hx-confirm=` in templates (the allowlist mechanism stays as a safety net for the future, just empty in steady state)
- Given the audit doc-comment in `src/templates_audit.rs`, when updated, then it reflects the new state ("All destructive actions use the UX-DR8 Modal component — no `hx-confirm=` anywhere"), removing the "five grandfathered sites" wording introduced in story 7-5
- Given the CLAUDE.md "Modal scanner-guard invariant" section, when updated, then the line "the allowlist is frozen at 5 grandfathered sites … and only changes through explicit review" becomes "the allowlist is empty post Epic 9 — any new `hx-confirm=` is BLOCKED outright by `templates_audit.rs`"
- Given the existing 8-3 E2E for admin user deactivation, when re-run, then it passes (server contract + guards unchanged)
- CSP compliance: no new inline markup
- i18n: EN + FR for deactivate-user modal copy
- Unit tests: audit test passes with empty allowlist; `ALLOWED_HX_CONFIRM_SITES` is `&[]`
- E2E: admin → `/admin?tab=users` → "Deactivate" on a librarian → Modal → confirm → verify user deactivated and session killed; attempt self-deactivate → verify 409 (Modal closed, FeedbackEntry shown — no regression of the 8-3 guards)

**Out of scope (explicit):** removing the `hx-confirm` audit infrastructure itself — the test stays in place as a permanent CSP-discipline guard; only the allowlist contents are emptied.

#### Story 9.15: StatusMessage — empty states (encouraging, role-aware)
**As any** user, **I want** clear, encouraging empty-state messages on every list / search / dashboard view, **so that** an empty result feels like a starting point, not a dead end.

**UX-DRs:** UX-DR13 (empty states)

**Acceptance Criteria:**
- Given the StatusMessage component, when factored, then it lives at `components/status_message.html` parameterized by: variant (`empty` / `info`), heading, body (HTML-escaped), CTA label (optional), CTA URL (optional), CTA role-gate (optional: `librarian` / `admin` to suppress for anonymous)
- Given a list view that has zero items, when rendered, then it shows a StatusMessage with copy tailored per surface: empty catalog → "No titles yet — start by scanning a barcode." / "Aucun titre pour l'instant — commencez par scanner un code-barres."; empty loans (librarian) → "No active loans" / "Aucun prêt en cours"; empty borrowers → "No borrowers yet" / "Aucun emprunteur"; empty series → "No series yet" / "Aucune série"
- Given the empty state has a CTA, when rendered, then the CTA shows only if the user has the role to act (librarian/admin can "Start cataloging" → `/catalog`; anonymous sees only the message, no CTA)
- Given the encouraging tone, when copy is written, then no negative phrasing ("nothing found", "no data", "no results") — instead inviting verbs ("Start", "Add", "Scan", "Try a different search")
- Given a search returns zero results, when rendered, then the StatusMessage adapts: "No matches for '{query}' — try a broader term or scan a barcode." / "Aucun résultat pour « {query} » — essayez un terme plus large ou scannez un code-barres."
- Given the StatusMessage variant `empty`, when styled, then it uses the calm, non-alarming visual treatment from UX-DR24 (warm stone neutral, illustrative icon if any — no red, no warning amber)
- Given anonymous + librarian role-aware CTA paths, when tested, then anonymous never sees a CTA that would link to a Librarian-gated route
- Given pages covered, when audited, then the following surfaces emit StatusMessage on empty (this is the contract): `/catalog` (zero titles), `/loans` (zero active), `/borrowers`, `/series`, `/contributors`, `/title/:id` no volumes, `/borrower/:id` no loan history, `/?filter=...` filtered home page with zero matches, search-no-results
- CSP compliance: component uses CSS classes only
- i18n: EN + FR for every copy variant emitted; i18n key naming follows `empty.<surface>` (e.g., `empty.catalog`, `empty.loans`)
- Unit tests: component rendering across variants; role-gating of CTA; HTML escaping of body
- E2E: anonymous → `/catalog` with empty DB → verify StatusMessage with no CTA; login as librarian → `/catalog` (still empty) → verify StatusMessage with "Start cataloging" CTA → click → verify navigates to `/catalog` with focused scan field

#### Story 9.16: StatusMessage — connection-lost overlay
**As any** user, **I want** a clear overlay when the server connection is lost, **so that** I know my actions are not being saved and can recover when connectivity returns.

**UX-DRs:** UX-DR13 (connection-lost overlay)

**Acceptance Criteria:**
- Given the application loads its base layout, when included, then a hidden `<div id="connection-lost-overlay" role="alert" aria-live="assertive" aria-atomic="true">` is present in `layouts/base.html`, with a "Connection lost" / "Connexion perdue" heading, body copy "Trying to reconnect..." / "Tentative de reconnexion en cours...", and a "Retry now" / "Réessayer" button
- Given an HTMX request fails with a network error (`htmx:sendError` event — server unreachable, NOT a 4xx/5xx), when caught by `static/js/connection-monitor.js` (new module), then the overlay is shown by toggling its open/hidden state — visually a fixed full-viewport semi-transparent overlay
- Given the overlay is shown, when a periodic health-check timer (5s interval) issues a `GET /health` (existing endpoint, exempt from CSRF / auth / setup-gate), then on success the overlay is dismissed automatically with a brief "Connection restored" / "Connexion rétablie" toast
- Given the user clicks "Retry now", when handled, then the health check is fired immediately (resets the timer); on success the overlay closes; on failure the overlay stays
- Given a 4xx / 5xx response (server reachable but errored), when received, then the overlay is NOT shown (these are application errors, handled by FeedbackEntry); the overlay is strictly for `htmx:sendError` (network failure) per UX-DR27 contract
- Given an aria-live assertive surface, when the overlay shows, then screen readers announce immediately (assertive priority)
- Given the connection is lost during a scan loop, when the overlay is shown, then the scan field is disabled (`disabled` attribute) so subsequent scans don't queue blind, and dismissal restores focus + enabled state
- Given the prefers-reduced-motion media query, when honored, then the overlay appears/disappears without transition; otherwise a 200ms fade is allowed
- CSP compliance: overlay markup is in `base.html`, JS is in an external module, no inline handlers
- i18n: EN + FR for all overlay copy and toast
- Unit tests (JS): overlay show/hide on simulated `htmx:sendError`; health-check polling; retry button; scan field disable/enable
- E2E: load app → simulate network drop (Playwright `--offline` or `page.context().setOffline(true)`) → trigger an HTMX action → verify overlay appears with assertive aria-live → restore network → verify overlay auto-dismisses within 5s + "Connection restored" toast

#### Story 9.17: NavBar — hamburger menu + scanner auto-close
**As a** user on a tablet or mobile device, **I want** the navigation bar to collapse into a hamburger menu, and any open menu to auto-close when a scanner burst arrives, **so that** the menu does not interfere with cataloging on small screens.

**UX-DRs:** UX-DR6 (partial — hamburger + scanner auto-close)

**Acceptance Criteria:**
- Given a viewport width below the desktop breakpoint (per UX-DR24: < 1024px), when the navbar renders, then the inline link list collapses into a hamburger button (☰ icon, `aria-label="Open menu"` / "Ouvrir le menu") at the right of the brand
- Given the hamburger is clicked or activated via Enter/Space, when toggled, then a `<dialog>` or accessible disclosure panel opens listing the navigation links vertically; `aria-expanded` toggles on the trigger
- Given the panel is open, when the user clicks outside it OR presses Escape OR clicks a link, then the panel closes; focus returns to the hamburger trigger
- Given the panel is open and the user starts a scanner burst (multiple keystrokes within `scanner_burst_threshold` from 9.9), when detected by an extension to `scanner-guard.js`, then the panel auto-closes immediately and the keystrokes are routed to the scan field if any (or just dismissed if no scan target on the current page)
- Given the desktop breakpoint (≥ 1024px), when the navbar renders, then the hamburger is hidden and the inline link list is shown — same links, same active-page indicator (existing Epic 1 nav bar)
- Given the panel uses a `<dialog>` element, when implemented, then focus is trapped inside it (reuse the same focus-trap helper as Modal from 9.10); Escape closes
- Given a route change (HTMX `hx-push-url` or full page nav), when the URL changes, then the panel closes if open
- Given the role-based link visibility logic, when applied (already in the existing nav), then it still works inside the hamburger panel — no role-logic regression
- CSP compliance: hamburger logic in `static/js/nav.js` (new) or `mybibli.js` extension, no inline handlers; visual states use Tailwind responsive classes (`md:hidden`, `lg:flex`, etc.)
- i18n: EN + FR for hamburger label and panel heading (if any)
- Unit tests (JS): toggle on click; close on outside click; close on Escape; close on link click; close on scanner burst (mock burst, verify `[open]` removed)
- E2E: tablet viewport → load app → verify hamburger visible, links collapsed → click hamburger → verify panel opens, focus inside → click a link → verify navigates and panel closed; tablet → open panel → simulate scanner burst → verify panel closes; desktop viewport → verify hamburger hidden, links inline

#### Story 9.18: NavBar — role-based visibility polish
**As any** user, **I want** the navigation links to reflect exactly what my role can do, **so that** the navigation is honest about what is accessible and the UI does not show dead-end links.

**UX-DRs:** UX-DR6 (role visibility polish — completion of Epic 1's basic nav)

**Acceptance Criteria:**
- Given the nav bar (desktop or hamburger), when rendered for an anonymous user, then the visible links are exactly: Home (/), Catalog (read-only — clicking takes them to `/login` per existing gate), Sign in (/login), Theme toggle, Language toggle — NO Loans, NO Borrowers, NO Admin, NO Sign out
- Given the nav bar, when rendered for a librarian, then the visible links are: Home, Catalog, Loans, Borrowers, Theme, Language, Sign out — NO Admin
- Given the nav bar, when rendered for an admin, then the visible links are: Home, Catalog, Loans, Borrowers, Admin, Theme, Language, Sign out (all links)
- Given a role downgrade (e.g., admin demoted to librarian by another admin via 8-3), when the next page is rendered, then the nav reflects the new role on the very next request — no stale "Admin" link from a cached template
- Given the existing nav-bar template (`components/nav_bar.html`) with role conditionals, when audited in this story, then any inconsistencies (e.g., a link visible to librarian but routing to a 403) are corrected; the audit is documented in the story spec
- Given the active-page indicator from Epic 1, when rendered, then it works inside the hamburger panel (9.17) too — the same `current_page` value drives both desktop and mobile presentations
- Given accessibility, when the nav renders, then it uses `<nav aria-label="Main navigation">`, links are real `<a href>` elements (no JS-only navigation), and the active page link has `aria-current="page"`
- Given Sign out, when present, then it's the POST form variant from story 8-2 (CSRF-protected) — no GET `/logout` link; this is a re-verification not a change
- CSP compliance: nav already CSP-clean from Epic 1, no new inline markup
- i18n: EN + FR for every nav label (already largely in place — this story verifies completeness across all 3 roles)
- Unit tests: render nav bar HTML for each role and assert exact link list; active-page rendering; `aria-current` on the matched link
- E2E: anonymous → load any page → verify exact nav link list; login as librarian → verify exact list; promote a user to admin → verify Admin link appears; demote → verify Admin link gone

#### Story 9.19: Contextual help — tooltips, help icons, aria-describedby
**As any** user encountering a non-obvious form field or interactive element, **I want** a discoverable tooltip or help icon that explains it, **so that** I do not have to consult docs or guess.

**FRs:** FR83

**Acceptance Criteria:**
- Given the Tooltip component, when factored, then it lives at `components/tooltip.html` and renders a small `<button type="button" class="help-icon" aria-describedby="tip-{id}" aria-label="Help: {summary}">?</button>` plus a hidden `<span role="tooltip" id="tip-{id}">{full text}</span>`; show on hover, focus, or tap (touch); Escape dismisses if focus-shown
- Given the help-icon-trigger pattern, when extended for placeholder-only hints, then a parallel `placeholder=` + `aria-describedby` pattern is documented for inputs that don't need a clickable icon (e.g., the scan field placeholder is sufficient — no help icon there)
- Given the form fields enumerated in this story, when rendered, then each has either a tooltip icon or aria-describedby pointing to inline help text. Coverage list (this is the contract for "complete" — anything else is a follow-up):
  - **/catalog scan field**: aria-describedby explaining accepted prefixes (ISBN/V-code/L-code) — placeholder + tooltip-on-focus, no icon
  - **Volume condition state** (loan + edit forms): tooltip explaining the configured states and the `loanable` flag impact
  - **Series type (open / closed)**: tooltip explaining gap detection only applies to closed series with declared total
  - **Overdue threshold** (`/admin?tab=system`): tooltip explaining computation cutoff behavior
  - **Provider API keys** (`/admin?tab=system`): tooltip explaining "leave blank to skip provider"
  - **First-launch wizard** (each step input): tooltip explaining what's being asked and what happens if skipped
  - **Search field on `/`**: aria-describedby explaining "Type to search, scan a barcode to navigate"
  - **Borrower contact fields**: tooltip on phone/email explaining optional, no validation beyond format
- Given Tooltip on touch devices, when tapped, then it toggles open and stays open until tapped outside or another tooltip is opened
- Given prefers-reduced-motion, when honored, then no fade-in transitions; tooltip appears/disappears instantly
- CSP compliance: tooltip toggle in `static/js/tooltip.js` (new), no inline handlers; visual styles via Tailwind classes
- i18n: EN + FR for every help-text string emitted; the i18n key naming follows the convention `help.<surface>.<field>` (e.g., `help.catalog.scan-field`, `help.admin.system.overdue-threshold`) for traceability
- Unit tests (JS): toggle on click; close on outside click; close on Escape; aria-describedby linkage
- E2E: navigate each surface in the coverage list → hover/focus the help icon → verify tooltip text appears in the correct language → press Escape → verify dismissed; tablet → tap help icon → verify toggle behavior

#### Story 9.20: Keyboard shortcuts complete + cheat-sheet dialog
**As a** keyboard-driven librarian, **I want** consistent keyboard shortcuts during the scan workflow plus a discoverable "?" cheat sheet, **so that** I can move at speed without reaching for the mouse.

**FRs:** FR84

**Acceptance Criteria:**
- Given the existing global shortcut Ctrl+K / Cmd+K (Epic 1 — focus scan field on `/catalog`), when this story extends shortcuts, then the following are added globally (ignored when typing in non-scan inputs unless explicitly Esc-aware): `?` (open cheat-sheet dialog), `Esc` (close any open modal / cheat-sheet / focused dropdown), `g` then `c` (go to catalog), `g` then `l` (go to loans, librarian only), `g` then `h` (go to home), `g` then `b` (go to borrowers, librarian only), `g` then `a` (go to admin, admin only) — chord pattern with 800ms timeout
- Given the cheat-sheet dialog, when opened via `?`, then it lists every active shortcut grouped by category (Navigation / Catalog / Modal / Search) — only shortcuts the user has access to (no admin shortcuts for librarian)
- Given the dialog uses `<dialog>` + focus trap (reusing Modal infrastructure from 9.10), when opened, then Escape closes; clicking outside closes
- Given the user is typing in a text input, when a shortcut key is pressed, then the global shortcut does NOT fire (e.g., `?` typed in a search box stays as text); the only exception is Esc (always handled, since it's the universal "escape from this context" key)
- Given the chord pattern (`g` then `c`), when implemented, then the first key starts a 800ms window during which the second key triggers the action; if the window expires or any other key is pressed, the chord is cancelled
- Given prefers-reduced-motion, when honored, then no animated dialog open; instant reveal
- Given the cheat-sheet dialog has a "?" affordance, when discoverable, then a small footer link on every page reads "Press `?` for shortcuts" / "Appuyez sur `?` pour les raccourcis"
- CSP compliance: shortcut handler in `static/js/shortcuts.js` (new), no inline handlers; uses delegated `keydown` listener on `document`
- i18n: EN + FR for cheat-sheet content and footer link
- Unit tests (JS): each shortcut fires when not in input; ignored when in input; chord timeout cancels; role-gated shortcuts respect role; cheat-sheet renders correct subset per role
- E2E: anonymous → press `?` → verify cheat-sheet limited to anonymous shortcuts (no `g l`, no `g a`); login as librarian → press `?` → verify additional shortcuts; type `g` then `c` → verify navigates to `/catalog`; press Esc → verify dialog closes; focus a search input, type `?` → verify dialog does NOT open

#### Story 9.21: Responsive per-page layouts
**As a** user on a tablet or mobile device, **I want** each page to adapt its layout to the viewport, **so that** the most important elements are reachable and usable without horizontal scrolling.

**UX-DRs:** UX-DR28

**Acceptance Criteria:**
- Given the breakpoints from UX-DR24 (mobile < 768, tablet 768–1023, desktop ≥ 1024), when each surface adapts, then the following per-page rules apply:
  - `/catalog`: tablet — feedback list moves above the scan field (so virtual keyboard does not obscure it); mobile — feedback list is collapsible (latest entry visible, "show more" expands)
  - `/loans`: tablet — DataTable hides "Created date" + "Borrowed_at" columns, shows only Volume, Borrower, Duration, Action; mobile — DataTable becomes a card list (one card per loan with full info stacked)
  - `/borrowers`: similar DataTable → card transformation on mobile
  - `/title/:id`: tablet — volumes table responsive column hiding; mobile — volumes become a card list
  - `/`: tablet — dashboard sections stack vertically; mobile — same with reduced padding
  - `/admin`: tablet — tabs wrap to two rows if needed; mobile — tabs become a select dropdown (single visible tab name + chevron)
- Given the responsive transformations, when implemented, then they use Tailwind responsive prefixes (`md:hidden`, `lg:table-cell`, etc.) only — no JavaScript layout switching, so the layout is correct on initial server render (no flash of wrong layout)
- Given the DataTable card-on-mobile transformation, when implemented, then the existing DataTable component (UX-DR5) gains a `mobile-cards` variant prop (or a sibling rendering) that emits `<dl>`-based card markup; the same data, the same sorting, the same pagination work in card mode
- Given orientation changes (landscape → portrait on tablet), when triggered, then the layout updates without a page reload (CSS-driven); no JS event listener required
- Given the print stylesheet from UX-DR19, when honored, then it remains compatible — printable views are not affected by mobile layouts
- Given prefers-reduced-motion + reduced-data, when honored, then no entrance animations on layout transitions
- CSP compliance: pure CSS transformations, no inline styles
- i18n: every label that adapts (e.g., "Show more" on mobile catalog feedback collapse) has EN + FR keys
- Unit tests: snapshot rendering of each surface at 3 viewport widths (mobile / tablet / desktop) — snapshots assert the presence of expected responsive class hints
- E2E: each viewport (mobile 375px, tablet 768px, desktop 1280px) → load each surface in the coverage list → verify the expected layout transformation applies (key elements visible, no horizontal scroll, columns collapsed/expanded correctly)

#### Story 9.22: WCAG 2.2 AA — final audit + axe-core full coverage
**As the** project maintainer, **I want** every page in the app to pass WCAG 2.2 AA via automated axe-core checks in CI plus verified manual contrast/keyboard audits, **so that** the accessibility commitment from the project brief is closed end-to-end and regressions are caught on every PR.

**UX-DRs:** UX-DR29 (finalization)

**Acceptance Criteria:**
- Given the existing axe-core helper in `tests/e2e/helpers/accessibility.ts`, when extended, then a new spec `tests/e2e/specs/accessibility-full.spec.ts` iterates over every URL in a list (`/`, `/catalog`, `/loans`, `/borrowers`, `/title/:id`, `/borrower/:id`, `/contributor/:id`, `/series/:id`, `/setup`, `/login`, `/admin?tab=health`, `/admin?tab=users`, `/admin?tab=reference_data`, `/admin?tab=trash`, `/admin?tab=system`) and runs axe-core's `runOnly: ['wcag2a', 'wcag2aa', 'wcag22aa']` configuration, asserting zero violations
- Given a violation is found, when it surfaces, then the test fails with the violation rule id, target selector, and the failing element's accessible name — the developer can copy-paste the failing nodes from the CI log
- Given keyboard-only navigation is verified, when audited, then a manual checklist is added to `docs/accessibility-audit.md` covering: skip-link presence on every page, focus visible on every focusable element (UX-DR24 token), focus order matches visual order, all interactive elements reachable, modal/dialog focus traps work, scan field stays focused after submit
- Given color contrast is verified, when audited, then the audit produces a section in the same doc with: every text/background pairing measured (foreground/background hex + computed ratio), every pairing ≥ 4.5:1 for normal text and ≥ 3:1 for large text — both light and dark themes; flagged failures are filed as separate GitHub Issues (label `type:bug`) if found
- Given screen reader smoke-tests are documented, when audited, then a section walks through one critical journey (cataloging a title) with VoiceOver / NVDA notes — what each landmark is announced as, what each scan-feedback entry sounds like with aria-live polite, what the modal sounds like with `aria-modal`
- Given the CI integration, when the new axe-core spec runs, then it is added to the `e2e` job; it must pass for the PR to merge (gate); existing axe-core spot tests are kept (no regression in coverage)
- Given the audit doc is signed, when complete, then the project README's Accessibility section links to `docs/accessibility-audit.md` and states "WCAG 2.2 AA verified at Epic 9 close — see audit doc for evidence"
- Given the future contract, when established, then any new page added after Epic 9 must include itself in the axe-core URL list — this is enforced by a `templates_audit.rs` test that walks `src/routes/` for handlers returning HTML and asserts they are in the URL list (or explicitly opted out with a doc-commented reason)
- CSP compliance: no template changes in this story (verification-only); any failing surface gets a follow-up issue rather than an in-story fix unless it is < 30min trivial
- i18n: EN + FR for any new copy in the audit doc + README
- Unit tests: the URL list is loaded from a single source of truth (no duplicated lists between the spec and a registry); the `templates_audit.rs` regression test detects a new handler not in the URL list
- E2E smoke (Foundation Rule #7, Epic 9 closure): the new accessibility-full spec runs in CI on PR; the run passes locally + in CI; manual sign-off doc is committed to the repo
