# Story 2.2: Shelving by Scan

Status: done

## Story

As a librarian,
I want to scan a volume V-code then a location L-code to shelve it, and optionally set the volume's condition and edition,
so that I know where each volume is physically stored.

## Acceptance Criteria (BDD)

### AC1: V-code then L-code Shelving

**Given** I scanned a V-code (volume context active),
**When** I scan an L-code,
**Then** the volume is assigned to that location, feedback shows "Volume V0042 shelved at Salon → Biblio 1 → Étagère 3", and the volume label is cleared from session (preventing re-shelving on next L-code). **Already implemented — verify it works end-to-end.**

### AC2: L-code without Volume Context — Batch Mode

**Given** I scan an L-code without a preceding V-code (no volume context),
**When** the scan is processed,
**Then** an info feedback shows "Active location: {path}. Scan V-codes to shelve here." The location is stored in session. Subsequent V-code scans auto-shelve at that location.

### AC3: L-code Not Found

**Given** I scan an L-code that doesn't exist in the database,
**When** the scan is processed,
**Then** a warning feedback shows "Location L0099 not found." **Already implemented — verify.**

### AC4: Volume Condition/State (FR28)

**Given** a volume exists,
**When** a librarian views or edits the volume,
**Then** they can set its condition from a configurable list (Neuf, Bon, Usé, Endommagé — already seeded in `volume_states` table).

### AC5: Edition Comment (FR29)

**Given** a volume exists,
**When** a librarian views or edits the volume,
**Then** they can add an edition comment (e.g., "poche", "relié", "collector").

### AC6: Not Shelved Status (FR35)

**Given** a volume has `location_id = NULL`,
**When** displayed in the UI,
**Then** it shows a "not shelved" indicator. Volumes with a location show the location path.

## Explicit Scope Boundaries

**In scope:**
- Batch shelving mode: scan L-code first → store in session → auto-shelve subsequent V-codes
- Replace "location contents — coming soon" stub with batch shelving info
- Volume detail/edit endpoint with condition state dropdown and edition comment field
- "Not shelved" visual indicator on volumes
- Context banner update showing active location in batch mode

**NOT in scope:**
- Browse shelf contents (story 2-3)
- Audio feedback (deferred to later epic)
- Drag-and-drop shelving UI
- Bulk shelving operations

## Tasks / Subtasks

- [ ] Task 1: Batch shelving mode (AC: 2)
  - [ ] 1.1 Add `set_active_location(pool, token, location_id)` and `get_active_location(pool, token)` to `SessionModel` — stores location_id in session JSON data
  - [ ] 1.2 Update L-code handler in `handle_scan()`: when no volume context, store location in session and return info feedback "Active location: {path}. Scan V-codes to shelve here." instead of "coming soon" stub
  - [ ] 1.3 Update V-code handler: after creating volume, if `active_location` is set in session, auto-assign location immediately. Show feedback "Volume V0042 created and shelved at {path}."
  - [ ] 1.4 Add i18n keys: `feedback.active_location`, `feedback.volume_created_and_shelved`
  - [ ] 1.5 Context banner: when active location is set, show location path in banner

- [ ] Task 2: Volume detail/edit page (AC: 4, 5, 6)
  - [ ] 2.1 Create `GET /volume/{id}` route and handler showing volume detail: label, title, condition state, edition comment, location path (or "not shelved")
  - [ ] 2.2 Create `GET /volume/{id}/edit` route with edit form: condition state dropdown (from `volume_states` table), edition comment text input
  - [ ] 2.3 Create `POST /volume/{id}` route to update volume (condition_state_id, edition_comment) with optimistic locking
  - [ ] 2.4 Add `VolumeModel::find_by_id(pool, id)` — does NOT exist yet. Add `VolumeModel::update_details(pool, id, version, condition_state_id, edition_comment)` with optimistic locking — does NOT exist yet.
  - [ ] 2.5 Add `VolumeModel::find_volume_states(pool) -> Vec<(u64, String)>` — query `volume_states` table (already seeded with Neuf, Bon, Usé, Endommagé). Does NOT exist yet.
  - [ ] 2.6 Create templates: `templates/pages/volume_detail.html`, `templates/pages/volume_edit.html`

- [ ] Task 3: "Not shelved" indicator (AC: 6)
  - [ ] 3.1 In volume detail page: show location path if shelved, "Not shelved" badge if `location_id` is None
  - [ ] 3.2 In catalog context banner: show shelving status when volume is active

- [ ] Task 4: i18n keys (AC: all)
  - [ ] 4.1 Add to `locales/en.yml`: `feedback.active_location`, `feedback.volume_created_and_shelved`, `volume.not_shelved`, `volume.condition_label`, `volume.edition_label`, `volume.detail_title`, `volume.edit_title`
  - [ ] 4.2 Add French translations
  - [ ] 4.3 `touch src/lib.rs` before build

- [ ] Task 5: Unit tests (AC: all)
  - [ ] 5.1 Session active location: set/get
  - [ ] 5.2 Batch shelving: V-code with active location → auto-shelve
  - [ ] 5.3 Volume detail template renders with/without location

- [ ] Task 6: E2E tests (AC: all)
  - [ ] 6.1 Test: Scan V-code → scan L-code → "shelved at {path}" feedback
  - [ ] 6.2 Test: Scan L-code alone → "Active location" info feedback
  - [ ] 6.3 Test: Scan L-code → scan V-code → auto-shelved at active location
  - [ ] 6.4 Test: Volume edit → condition and edition comment saved

## Dev Notes

### What Already Works (DO NOT recreate)

- **V-code → L-code shelving:** `handle_scan()` in catalog.rs already handles this flow completely
- **`VolumeService::assign_location()`** — assigns volume to location, returns path
- **`LocationModel::find_by_label()`** — L-code lookup
- **`SessionModel::get/set_last_volume_label()`** — V-code session tracking
- **`LocationModel::get_path()`** — breadcrumb path builder
- **i18n keys:** `feedback.volume_shelved`, `feedback.lcode_not_found` already exist
- **Volume states:** `volume_states` table already seeded with Neuf, Bon, Usé, Endommagé

### Key Changes to Existing Code

The main change is in the L-code branch of `handle_scan()`:
- **Current:** No volume context → "Location contents — coming soon" stub
- **New:** No volume context → store location in session, return batch shelving info

And in the V-code branch:
- **Current:** Create volume, set `last_volume_label` in session
- **New:** Also check `active_location` in session → if set, auto-shelve immediately

### MariaDB Type Gotchas (from CLAUDE.md)

- `JSON` columns → `CAST(col AS CHAR)` to read as String
- `BIGINT UNSIGNED NULL` → `CAST(col AS SIGNED)` and read as `Option<i64>`
- Session data is JSON in `sessions.data` column

### References

- [Source: _bmad-output/planning-artifacts/prd.md#FR25, #FR28, #FR29, #FR31, #FR35]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#UX-DR19]
- [Source: src/routes/catalog.rs#handle_scan L-code branch]
- [Source: src/services/volume.rs#assign_location]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### Change Log

### File List
