# Story 5.6: Browse List/Grid Toggle with Persistent Preference

Status: done

## Story

As a user,
I want to toggle between list and grid display modes when browsing titles,
so that I can see more titles at once (grid) or more detail per title (list) depending on my task.

## Acceptance Criteria

1. **BrowseToggle visible:** Given `/catalog` or any browse view, when the page loads, then a BrowseToggle radiogroup (list/grid) is visible at the top
2. **List mode:** Given list mode, when rendered, then each TitleCard shows cover on left + title/contributors/year/media icon on right (single row)
3. **Grid mode:** Given grid mode, when rendered, then each TitleCard shows cover on top + title below, with hover overlay revealing contributors + media icon + volume count + any status badge
4. **Touch device:** Given a touch device in grid mode, when user taps a card, then first tap shows overlay, second tap navigates to title detail
5. **Preference persistence:** Given a user changes the toggle, when navigating away and back, then the preference persists (cookie or localStorage, per-user session)
6. **ARIA:** BrowseToggle uses `role="radiogroup"` with keyboard arrow navigation per WCAG 2.2 AA
7. **Unit test:** TitleCard template rendering both modes with/without optional fields
8. **E2E:** Load catalog -> toggle grid -> verify layout -> reload -> verify grid persisted

## Tasks / Subtasks

- [ ] Task 1: Create BrowseToggle component (AC: #1, #6)
  - [ ] 1.1 Create `templates/components/browse_toggle.html` — radiogroup with list/grid icons, `role="radiogroup"`, `aria-label="Display mode"`. Each button: `role="radio"`, `aria-checked`, `aria-label="List view"/"Grid view"`.
  - [ ] 1.2 Create `static/js/browse-mode.js` — handles toggle clicks, saves preference to localStorage key `"mybibli_browse_mode"` (values: "list"/"grid"), applies mode by toggling CSS class on results container. Follow pattern from `static/js/theme.js`.
  - [ ] 1.3 On page load: read localStorage, apply saved mode immediately (before render to prevent flash). Default to "list" if no preference saved.
  - [ ] 1.4 Keyboard: arrow keys toggle between list/grid, Enter/Space activates.
- [ ] Task 2: Create TitleCard component (AC: #2, #3)
  - [ ] 2.1 Create `templates/components/title_card.html` — dual-mode card rendering. Use CSS classes to switch between list/grid layout (e.g., `browse-list`/`browse-grid` on parent container controls child layout).
  - [ ] 2.2 **List mode layout:** Cover 120x180px (left, `object-cover rounded`), right side: title (font-medium), primary contributor, genre + year, volume count + media icon. Wrapped in `<article><a href="/title/{id}">`.
  - [ ] 2.3 **Grid mode layout:** Cover 150x225px (top, `object-cover rounded-t`), title + contributor below cover. Hover overlay: semi-transparent dark background on cover area with media icon + volume count + status badge.
  - [ ] 2.4 **Hover overlay (grid):** CSS `opacity-0 group-hover:opacity-100 transition-opacity`. Show media type icon, volume count, status.
  - [ ] 2.5 Touch device support (AC#4): CSS `:hover` already handles touch first-tap on mobile (shows overlay). Second tap on `<a>` navigates. No JS needed for basic touch — CSS `:hover` sticks on touch.
- [ ] Task 3: Integrate into home/search results (AC: #1, #2, #3)
  - [ ] 3.1 Update `templates/pages/home.html` — replace the `<table>` search results with a container `<div id="browse-results">` that renders TitleCard components. Add BrowseToggle above.
  - [ ] 3.2 Update `src/routes/home.rs` — pass `browse_mode` (from query param or default "list") to the template. The HTMX search endpoint also needs to return cards instead of table rows.
  - [ ] 3.3 Update `render_search_row()` in `src/routes/home.rs` to render TitleCard HTML instead of table rows. Both list and grid modes use the same HTML — CSS handles the layout switch.
  - [ ] 3.4 Add `<script src="/static/js/browse-mode.js">` to `layouts/base.html` (or only to pages with browse).
- [ ] Task 4: Add i18n keys
  - [ ] 4.1 Add to en.yml/fr.yml: `browse.list_view`, `browse.grid_view`, `browse.display_mode`
  - [ ] 4.2 Run `touch src/lib.rs && cargo build`
- [ ] Task 5: E2E test (AC: #8)
  - [ ] 5.1 Add test to existing home or catalog spec: navigate to `/` → verify BrowseToggle visible → click grid icon → verify layout changes (cards have grid class) → reload page → verify grid mode persists (localStorage)
  - [ ] 5.2 Verify ARIA: radiogroup role, aria-checked on active option
- [ ] Task 6: Verification
  - [ ] 6.1 `cargo clippy -- -D warnings` passes
  - [ ] 6.2 `cargo test` all green
  - [ ] 6.3 Full E2E suite passes

## Dev Notes

### Current Search Results Implementation

**Home page** (`src/routes/home.rs`) renders search results as a `<table>` with `<tbody id="search-results-body">`. Each row is rendered by `render_search_row()` (lines 227-257) as a `<tr>` with columns: cover, title, contributor, genre, volume count.

**SearchResult struct** (lines 357-366): `id`, `title`, `subtitle`, `media_type`, `genre_name`, `primary_contributor`, `volume_count`, `cover_image_url`.

**Key challenge:** The current table-based layout needs to be replaced with a flex/grid-based layout that supports both list and grid modes via CSS class toggling. The HTMX search fragment (`hx-target="#search-results-body"`) also needs updating.

### Approach: CSS-Only Layout Switch

Both list and grid modes render the SAME HTML structure (TitleCard). The parent container switches between modes:

```html
<div id="browse-results" class="browse-list"> <!-- or "browse-grid" -->
  <!-- TitleCard components here -->
</div>
```

```css
/* List mode: horizontal cards */
.browse-list { display: flex; flex-direction: column; gap: 0.5rem; }
.browse-list .title-card { display: flex; flex-direction: row; }

/* Grid mode: cards in grid */
.browse-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 1rem; }
.browse-grid .title-card { display: flex; flex-direction: column; }
.browse-grid .title-card .card-overlay { /* visible on hover */ }
```

This approach means: one HTML structure, one Rust render function, CSS handles the visual switch. The JS only toggles the class and saves preference.

### localStorage Persistence Pattern

Follow the existing `theme.js` pattern:

```js
// browse-mode.js
(function() {
  const KEY = "mybibli_browse_mode";
  const DEFAULT = "list";
  
  function getMode() {
    return localStorage.getItem(KEY) || DEFAULT;
  }
  
  function setMode(mode) {
    localStorage.setItem(KEY, mode);
    applyMode(mode);
  }
  
  function applyMode(mode) {
    const container = document.getElementById("browse-results");
    if (!container) return;
    container.classList.remove("browse-list", "browse-grid");
    container.classList.add("browse-" + mode);
    // Update radiogroup
    document.querySelectorAll('[role="radio"]').forEach(btn => {
      btn.setAttribute("aria-checked", btn.dataset.mode === mode ? "true" : "false");
    });
  }
  
  // Apply on load
  document.addEventListener("DOMContentLoaded", () => applyMode(getMode()));
  
  // Expose for toggle clicks
  window.mybibliSetBrowseMode = setMode;
})();
```

### BrowseToggle Accessibility

Per WCAG 2.2 AA and UX spec:
- Container: `role="radiogroup"`, `aria-label="Display mode"`
- Each option: `role="radio"`, `aria-checked="true"/"false"`, `aria-label="List view"/"Grid view"`
- Keyboard: Left/Right arrow keys toggle, Enter/Space activates
- Tab: focuses the radiogroup, not individual radio buttons (single-tab-stop pattern)

### HTMX Integration — Critical Details

The search results are loaded via HTMX using a **custom event** `search-fire` dispatched by `static/js/search.js` (not Enter key). The search input has `hx-trigger="search-fire"`.

After switching to card-based rendering:
- The HTMX target changes from `#search-results-body` (tbody at `home.html:100`) to `#browse-results` (div)
- The server returns card HTML instead of table rows
- `search.js` dispatches `search-fire` which triggers the `hx-get` — this mechanism doesn't change
- The `hx-target` on the search input AND the sort/filter links must all update to `#browse-results`
- The CSS class on the container determines list vs grid — the server doesn't need to know the mode

### Sort Controls Migration

The current table has sortable column headers (`<th>` with `hx-get` links for sort). Cards don't have column headers. Options:
1. Add a separate sort dropdown above the cards (next to the BrowseToggle)
2. Keep sort via URL params (`?sort=title&dir=asc`) but change the UI from column headers to a dropdown
3. Grid mode hides sort controls (grid is visual-first); list mode shows a compact sort bar

Simplest approach: add a sort dropdown next to the BrowseToggle. Both modes use the same sort mechanism.

### Template Struct Changes

**HomeTemplate** (`src/routes/home.rs`) needs:
- `browse_list_label: String` — i18n for "List view"
- `browse_grid_label: String` — i18n for "Grid view"
- `browse_mode_label: String` — i18n for "Display mode"

No backend browse mode tracking needed — it's entirely client-side via localStorage.

### Scope Boundaries

- **In scope:** Home/search browse results only (the table at `/?q=...`)
- **Out of scope:** Catalog page (/catalog) — it has a different UI (scan-focused, not browse-focused)
- **Out of scope:** Series list, borrower list, location list — these keep their current table format
- **Touch device (AC#4):** CSS `:hover` on mobile naturally provides "first tap shows overlay" behavior. No special JS needed.

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Story 5.6 AC, FR115, UX-DR17, UX-DR18]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md — Lines 2314-2417 (TitleCard + BrowseToggle)]
- [Source: src/routes/home.rs — current search rendering, SearchResult struct]
- [Source: templates/pages/home.html — current table-based search results]
- [Source: static/js/theme.js — localStorage persistence pattern]
- [Source: _bmad-output/planning-artifacts/architecture.md — Line 1046 (browse component mapping)]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Completion Notes List

- Replaced table-based search results with card-based TitleCard articles
- CSS-only layout switch: `.browse-list` (flex column) vs `.browse-grid` (CSS grid auto-fill)
- BrowseToggle radiogroup with list/grid SVG icons, ARIA roles, keyboard nav
- localStorage persistence via `browse-mode.js` (key: `mybibli_browse_mode`)
- Grid hover overlay: semi-transparent dark bg with media icon + volume count
- Sort controls migrated from table column headers to dropdown select
- Added `publication_date` to SearchResult struct for year display in list mode
- Updated `render_search_row()` from `<tr>` to `<article>` card HTML
- Updated HTMX target from `#search-results-body` to `#browse-results`
- Updated search.js error handling references
- Updated home-search.spec.ts and epic2-smoke.spec.ts selectors
- 3 new E2E tests: smoke toggle+persist, card rendering, ARIA radiogroup
- Dark mode CSS support for both list and grid modes

### File List

**Created:**
- `static/js/browse-mode.js` — localStorage persistence + toggle + keyboard + HTMX reinit
- `tests/e2e/specs/journeys/browse-toggle.spec.ts` — 3 E2E tests

**Modified:**
- `templates/pages/home.html` — Table → card layout, BrowseToggle, sort dropdown, CSS styles
- `templates/layouts/base.html` — Added browse-mode.js script
- `src/routes/home.rs` — Added browse labels to HomeTemplate, updated render_search_row to cards
- `src/models/title.rs` — Added publication_date to SearchResult struct + query
- `src/services/search.rs` — Added publication_date to SearchResult construction
- `static/js/search.js` — Updated element ID references
- `locales/en.yml` — Added browse.* i18n keys
- `locales/fr.yml` — Same (French)
- `tests/e2e/specs/journeys/home-search.spec.ts` — Updated selectors for card layout
- `tests/e2e/specs/journeys/epic2-smoke.spec.ts` — Updated search results selector
