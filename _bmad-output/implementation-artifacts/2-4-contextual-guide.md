# Story 2.4: Contextual Guide Strip

Status: done

## Story

As a librarian,
I want to see a persistent guide message on the catalog page that tells me what to do next,
so that I always know the current state and what action is expected.

## Acceptance Criteria (BDD)

### AC1: Guide Shows Initial State

**Given** I am on `/catalog` with no active context,
**When** the page loads,
**Then** a guide strip shows: "Scan an ISBN to start cataloging, or an L-code to set a shelving location."

### AC2: Guide After ISBN Scan

**Given** I scanned an ISBN and a title was created,
**When** the feedback appears,
**Then** the guide updates to: "Title active: {title}. Scan a V-code to add a volume."

### AC3: Guide After V-code Scan

**Given** I scanned a V-code and a volume was created,
**When** the feedback appears,
**Then** the guide updates to: "Volume {label} ready. Scan an L-code to shelve, or another V-code."

### AC4: Guide After L-code Shelving

**Given** I scanned an L-code and a volume was shelved,
**When** the feedback appears,
**Then** the guide updates to: "✅ {label} shelved at {path}. Scan another ISBN, V-code, or L-code."

### AC5: Guide In Batch Mode

**Given** I scanned an L-code without a volume context (batch mode),
**When** the feedback appears,
**Then** the guide updates to: "📍 Active location: {path}. Scan V-codes to shelve here."

### AC6: Guide Persists Across Page Navigation

**Given** the guide shows a state message,
**When** I navigate away and return to `/catalog`,
**Then** the guide reflects the current session state (not reset to initial).

## Explicit Scope Boundaries

**In scope:**
- Guide strip component below the context banner on `/catalog`
- OOB swap updates on each scan response
- State-aware messages based on session data (current_title, last_volume_label, active_location)
- i18n keys EN/FR for all guide messages

**NOT in scope:**
- Audio feedback
- Toast notifications
- Guide on pages other than `/catalog`

## Tasks / Subtasks

- [ ] Task 1: Guide strip HTML component (AC: 1-5)
  - [ ] 1.1 Add a `<div id="guide-strip">` element to `templates/pages/catalog.html` — below context banner, above scan field. Styled as a subtle info bar (stone background, small text, icon).
  - [ ] 1.2 Default content on page load: read session state and render appropriate message. If no context → initial message. If title active → "Scan V-code". Etc.

- [ ] Task 2: OOB updates on each scan (AC: 2-5)
  - [ ] 2.1 In `handle_scan()` ISBN branch: add OOB update for `guide-strip` with "Title active: {title}. Scan a V-code..."
  - [ ] 2.2 In `handle_scan()` V-code branch: add OOB update for `guide-strip` with "Volume {label} ready. Scan L-code to shelve..."
  - [ ] 2.3 In `handle_scan()` L-code shelving branch: add OOB update for `guide-strip` with "✅ Shelved. Scan another..."
  - [ ] 2.4 In `handle_scan()` L-code batch branch: add OOB update for `guide-strip` with "📍 Active location: {path}..."

- [ ] Task 3: Guide state on page load (AC: 6)
  - [ ] 3.1 In `catalog_page()` handler: read session state (current_title_id, last_volume_label, active_location) and compute the appropriate guide message. Pass as template variable `guide_message`.

- [ ] Task 4: i18n keys (AC: all)
  - [ ] 4.1 Add to `locales/en.yml` under `guide:`: initial, title_active, volume_ready, shelved, batch_active
  - [ ] 4.2 Add French translations
  - [ ] 4.3 `touch src/lib.rs` before build

- [ ] Task 5: E2E tests (AC: all)
  - [ ] 5.1 Test: Page load → initial guide message visible
  - [ ] 5.2 Test: Scan ISBN → guide changes to "Scan V-code"
  - [ ] 5.3 Test: Full flow ISBN → V-code → L-code → guide updates at each step

## Dev Notes

### Implementation Pattern

The guide strip is an OOB-swappable `<div>` just like context-banner and session-counter. Each scan response includes an OOB update targeting `#guide-strip`.

```rust
oob.push(OobUpdate {
    target: "guide-strip".to_string(),
    content: guide_html("title_active", &title.title),
});
```

### Helper Function

```rust
fn guide_html(state: &str, detail: &str) -> String {
    let message = match state {
        "initial" => t!("guide.initial"),
        "title_active" => t!("guide.title_active", title = detail),
        "volume_ready" => t!("guide.volume_ready", label = detail),
        "shelved" => t!("guide.shelved", detail = detail),
        "batch_active" => t!("guide.batch_active", path = detail),
        _ => t!("guide.initial"),
    };
    format!(
        r#"<p class="text-sm text-stone-500 dark:text-stone-400 flex items-center gap-2">
            <svg class="w-4 h-4 text-indigo-400" ...>info icon</svg>
            {}</p>"#,
        html_escape(&message.to_string())
    )
}
```

### What Already Exists

- `#context-banner` OOB pattern — same mechanism, already proven
- Session data: `current_title_id`, `last_volume_label`, `active_location_id` all available
- `catalog_page()` handler already renders template — add guide_message variable
- All scan branches already push OOB updates — just add one more

### References

- [Source: _bmad-output/implementation-artifacts/epic-2-retro-2026-04-01.md#Action-Items]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### Change Log

### File List
