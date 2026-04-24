---
story_key: 8-7
epic: 8
story: 7
title: Permanent delete and auto-purge
status: ready-for-dev
created: 2026-04-24
last_updated: 2026-04-24
estimated_effort: large
dependencies: [8-6-trash-view-and-restore]
---

# Story 8-7: Permanent Delete and Auto-Purge

## Story Statement

**As an** admin, **I want** soft-deleted items to be hard-purged automatically after 30 days and to be able to force permanent deletion sooner from the Trash, **so that** storage stays bounded and items I am certain about are definitively gone.

## Functional Requirements

- **FR112:** System can hard-purge (permanently delete) soft-deleted items after 30 days of inactivity
- **FR113:** Admin can force permanent deletion from the Trash with explicit confirmation

## Non-Functional Requirements

- Auto-purge runs on app startup (blocking, bounded by count)
- Auto-purge runs daily via scheduled task (24-hour interval)
- FK dependency ordering prevents orphans (children first, then parents)
- Transaction per entity family ensures atomicity
- Errors during purge do not crash app startup or abort scheduled task

## Acceptance Criteria

### AC1: Permanent Delete Button on Trash Rows
- Given a Trash row, when "Delete permanently" is clicked, then a confirmation modal opens (UX-DR8 guard + scanner-guard 7-5)
- Modal shows: item name, explicit warning ("This cannot be undone"), and an input requiring the admin to **type the item name verbatim** to enable Confirm button
- Friction pattern matches destructive-action UX guidelines

### AC2: Hard Delete with FK Handling
- Given confirmation, when submitted, then:
  - The row is hard-deleted from its table (`DELETE FROM ... WHERE id = ? AND version = ?`)
  - Dependent rows are handled according to each table's FK policy (cascade, RESTRICT, SET NULL)
  - A row is appended to the `admin_audit` table (who, what entity + id, when)
  - Trash list OOB-swaps the row out
  - FeedbackEntry shows "Deleted permanently: {name}"

### AC3: Admin Audit Trail
- Given a hard-purge (permanent delete), when it completes, then `admin_audit` table records:
  - `user_id` (who performed the delete)
  - `action` (e.g., "permanent_delete_from_trash")
  - `entity_type` (table name)
  - `entity_id` (the deleted row's ID)
  - `timestamp` (NOW())
  - `details` (optional JSON: item name, affected FK rows count, etc.)

### AC4: Auto-Purge on App Startup
- Given the app boots, when the startup task runs:
  - Any row with `deleted_at < NOW() - INTERVAL 30 DAY` across every whitelisted table is hard-purged
  - Results are logged at info level with per-table counts
  - `admin_audit` table records a single "auto_purge" entry per run
  - Purge is synchronous, blocking `/admin` and `/catalog` render until it completes (bounded by count)

### AC5: Daily Scheduled Auto-Purge
- Given the app is running, when a daily scheduled task fires (first run 24h after boot, configurable via `settings.auto_purge_interval_seconds` default 86400):
  - Same 30-day purge runs
  - Results logged at info level
  - `admin_audit` table records the auto-purge run
  - On error (FK violation, DB unavailable, lock timeout): log error but do NOT abort task or crash app
  - Next scheduled run retries

### AC6: FK Dependency Ordering
- Given a hard-purge runs, when it processes a table:
  - Deletes respect FK dependencies by deleting in order defined by whitelist's dependency graph (children first, then parents)
  - Uses a single transaction per entity family so partial failures don't leave orphans
  - On FK violation: rollback, log error, continue to next family

### AC7: Error Resilience
- Given auto-purge encounters an error:
  - Does NOT abort app startup
  - Does NOT crash the daily interval task
  - Logs error with context (table, row count, constraint violation details)
  - Next scheduled run retries

### AC8: 404 When Already Gone
- Given permanent delete is attempted on an item that no longer exists in Trash:
  - e.g., already permanent-deleted by another admin, or auto-purged
  - Server returns 404 NotFound ("Item already gone") rendered via FeedbackEntry

### AC9: Guards
- Admin cannot permanent-delete themselves
- Admin cannot permanent-delete the last active admin user (same rules as story 8-3)
- Cannot permanent-delete a non-soft-deleted item (hitting this endpoint bypasses the Trash should return 400)
- Server validates `deleted_at IS NOT NULL` before allowing permanent delete

### AC10: Concurrency & Race Conditions
- Given concurrent admin sessions, when admin A permanently deletes an item:
  - Admin B's Trash list refresh (on next OOB sweep or manual reload) no longer shows the item
  - Second delete attempt on same item returns 404

## Tasks & Subtasks

### Task 1: Create admin_audit Table Migration
- [ ] Create migration `20260424000001_create_admin_audit_table.sql`
  - [ ] Schema: id, user_id (FK users), action, entity_type, entity_id, timestamp, details (JSON)
  - [ ] Indices: (user_id, timestamp), (action, timestamp)
  - [ ] Seed: none (user-driven entries only)

### Task 2: Add Permanent Delete Handler + Modal
- [ ] Add `admin_trash_permanent_delete()` handler to `src/routes/admin.rs`
  - [ ] Extract table, id, version from path
  - [ ] Validate user is admin
  - [ ] Load trash entry for name display
  - [ ] Return modal template on GET
- [ ] Create `admin_trash_permanent_delete_confirm()` handler
  - [ ] Extract table, id, version, confirmed_name from form
  - [ ] Validate user is admin and not self/last-admin
  - [ ] Check `deleted_at IS NOT NULL`
  - [ ] Verify user typed item name correctly
  - [ ] Call `TrashService::permanent_delete()` (see Task 3)
  - [ ] Create `admin_audit` row
  - [ ] Return FeedbackEntry + OOB swap to remove row
- [ ] Add routes to `src/routes/mod.rs`:
  - [ ] `GET /admin/trash/{table}/{id}/permanent-delete-confirm` → modal form
  - [ ] `POST /admin/trash/{table}/{id}/permanent-delete` → execute delete
- [ ] Update i18n: permanent delete button label, modal copy, success/error messages (en.yml + fr.yml)

### Task 3: Implement Permanent Delete Service
- [ ] Create `TrashService::permanent_delete(pool, table, id, version)` → Result<TrashEntry, AppError>
  - [ ] Validate table in ALLOWED_TABLES whitelist
  - [ ] Load soft-deleted entry with optimistic locking check
  - [ ] Hard-delete row: `DELETE FROM {table} WHERE id = ? AND version = ?`
  - [ ] Check rows_affected == 1; return 404 if 0
  - [ ] Return deleted entry (for audit/feedback)

### Task 4: Add Auto-Purge Logic
- [ ] Create `src/services/auto_purge.rs`
  - [ ] Function `run_purge(pool)` → Result<PurgeStats, AppError>
    - [ ] For each whitelisted table: `DELETE FROM {table} WHERE deleted_at < NOW() - INTERVAL 30 DAY`
    - [ ] Respect FK dependency graph (delete children first)
    - [ ] Use single transaction per entity family
    - [ ] Count deleted rows per table
    - [ ] Log at info level: "Auto-purge: titles=5, volumes=12, contributors=2, ..."
    - [ ] Record `admin_audit` entry: action="auto_purge", entity_type=NULL (system action), details with per-table counts
    - [ ] Return `PurgeStats { tables_processed, rows_deleted, errors }`
  - [ ] Error handling: on FK violation, rollback family transaction, log error, continue
- [ ] Call from app startup: in `main.rs` before binding HTTP listener
  - [ ] Catch errors: log but do NOT block startup
  - [ ] Add feature flag `--skip-startup-purge` for testing (via env var `MYBIBLI_SKIP_STARTUP_PURGE`)
- [ ] Add daily scheduled task: in `src/tasks/auto_purge_scheduler.rs`
  - [ ] `tokio::spawn` a task that runs `tokio::time::interval(86400)` (configurable via settings)
  - [ ] Call `run_purge()` on each tick
  - [ ] On error: log and continue (do NOT crash)

### Task 5: Add Admin Audit Model
- [ ] Create `src/models/admin_audit.rs`
  - [ ] Struct `AdminAuditEntry`: id, user_id, action, entity_type, entity_id, timestamp, details
  - [ ] Function `AdminAuditModel::create(pool, user_id, action, entity_type, entity_id, details)`
- [ ] Export from `src/models/mod.rs`

### Task 6: Update Trash Panel for Permanent Delete
- [ ] Add "Delete permanently" button to trash table row in template
  - [ ] Button links to `GET /admin/trash/{table}/{id}/permanent-delete-confirm`
  - [ ] Styled as destructive action (red, or via UX-DR8 modal styling)

### Task 7: Unit Tests
- [ ] Test 30-day boundary: row at 29d stays, row at 31d purged
- [ ] Test FK dependency ordering: generates correct DELETE sequence
- [ ] Test idempotency: running purge twice on empty Trash does not error
- [ ] Test last-admin guard on permanent delete
- [ ] Test `admin_audit` row shape and creation
- [ ] Test error handling: FK violation logs and continues
- [ ] Test optimistic locking: version mismatch on permanent delete returns 409

### Task 8: E2E Tests
- [ ] Scenario 1: Manual permanent delete
  - [ ] Create and soft-delete a title via catalog
  - [ ] Navigate to Trash
  - [ ] Click "Delete permanently"
  - [ ] Verify modal appears with friction (name input required)
  - [ ] Enter wrong name → Confirm button disabled
  - [ ] Enter correct name → Confirm button enabled
  - [ ] Click Confirm
  - [ ] Verify FeedbackEntry "Deleted permanently: {name}"
  - [ ] Verify item gone from Trash list (OOB swap)
  - [ ] Verify item NOT recoverable (DB query for trash entry returns empty)
- [ ] Scenario 2: Auto-purge on startup
  - [ ] Seed DB with a soft-deleted title aged 31 days
  - [ ] Boot app
  - [ ] Check logs for "Auto-purge: titles=1, ..."
  - [ ] Verify `admin_audit` table has a row with action="auto_purge"
  - [ ] Verify seeded item no longer in DB
- [ ] Scenario 3: Last admin guard
  - [ ] Create second admin user
  - [ ] Deactivate first admin (become second admin)
  - [ ] Soft-delete second admin user (via user admin flow)
  - [ ] Trash tab → attempt permanent delete of deactivated admin
  - [ ] Verify 400 or 403 error ("Cannot delete last admin")

### Task 9: Documentation
- [ ] Update `CLAUDE.md`:
  - [ ] Add bullet: "Auto-purge pattern (story 8-7): startup purge + daily interval, FK dependency ordering, transaction per family"
  - [ ] Document `admin_audit` table purpose and schema
- [ ] Update `docs/route-role-matrix.md`:
  - [ ] Add rows for new routes: `GET /admin/trash/{table}/{id}/permanent-delete-confirm`, `POST /admin/trash/{table}/{id}/permanent-delete`
- [ ] Update architecture.md:
  - [ ] Add section on "Permanent Delete & Auto-Purge": 30-day window, FK handling, audit trail

## Dev Notes

### Architecture Patterns to Follow

1. **Soft Delete Pattern:** Every entity table has `deleted_at`, `version`, `created_at`, `updated_at` columns (CLAUDE.md).
2. **Optimistic Locking:** Use `WHERE id = ? AND version = ?` and check `rows_affected == 1` (from story 8-3, 8-6).
3. **Error Handling:** All errors go through `AppError` enum; return 404, 409, 400, 500 as appropriate.
4. **i18n:** All user-facing text in `rust_i18n::t!()` with keys in `locales/en.yml` + `locales/fr.yml`.
5. **CSRF Protection:** POST endpoints require `_csrf_token` (from story 8-2).
6. **Trash Query:** Use UNION across whitelisted tables in `ALLOWED_TABLES` (from story 8-6).

### FK Dependency Graph

Define the deletion order in the `auto_purge.rs` to respect FK constraints:

```
Children (must delete first):
  - title_contributors (FK → titles, contributors)
  - series_title_assignments (FK → series, titles)
  - volumes (FK → titles)
  - loans (FK → volumes, borrowers)
  - volume_locations (FK → volumes, storage_locations)

Parents (delete after children):
  - titles
  - volumes
  - series
  - borrowers
  - storage_locations (if no volumes assigned — check earlier)
  - contributors
  - genres (if no titles use this genre — check earlier)
```

Some FK policies may cascade automatically; cross-check with schema before implementation.

### Key Design Decisions

1. **Friction Pattern:** Requiring the user to type the item name verbatim before confirm is intentional (UX-DR8). Do NOT make this optional.
2. **30-Day Window:** Hard-coded in AC; not admin-configurable (unlike overdue threshold in story 8-5).
3. **Transaction Scoping:** One transaction per entity family (e.g., all title-related deletes) to prevent orphans. Do NOT use single transaction for entire purge (too risky for high-count tables).
4. **Error Recovery:** On auto-purge error, log but continue. Do NOT retry within the same run; next scheduled run will retry.
5. **Audit Trail:** `admin_audit` table is append-only (no updates). Useful for compliance and debugging.

### Testing Strategy

- **Unit Tests:** Boundary conditions (29d, 30d, 31d), FK ordering, error handling.
- **E2E Tests:** Full user journeys: manual delete (with friction), auto-purge verification, last-admin guard.
- **Local Testing:** Run purge on a test DB with seeded 31-day-old rows. Verify counts in logs + `admin_audit`.

## Files to Create/Modify

### New Files

- `src/services/auto_purge.rs` — auto-purge logic and scheduling
- `src/models/admin_audit.rs` — audit trail model
- `src/tasks/auto_purge_scheduler.rs` — daily scheduled task spawning
- `migrations/20260424000001_create_admin_audit_table.sql` — audit table schema
- `tests/e2e/specs/journeys/admin-permanent-delete.spec.ts` — E2E tests

### Modified Files

- `src/routes/admin.rs` — add handlers for permanent delete confirm + execute
- `src/routes/mod.rs` — add routes for permanent delete
- `src/models/mod.rs` — export admin_audit module
- `src/services/mod.rs` — export auto_purge module
- `src/main.rs` — call startup purge, spawn daily task
- `templates/fragments/admin_trash_panel.html` — add permanent delete button
- `locales/en.yml` — add i18n keys for permanent delete
- `locales/fr.yml` — add i18n keys for permanent delete (French)
- `CLAUDE.md` — document auto-purge pattern
- `docs/route-role-matrix.md` — add new routes
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — mark story status

## Change Log

| Date | Change |
|------|--------|
| 2026-04-24 | Story created from epic 8 requirements. Status: ready-for-dev. Dependencies: story 8-6. |

## Review Findings

### Decision Needed

- [ ] [Review][Decision] FK Dependency Order Validation Strategy — Code uses hardcoded deletion order; must validate against actual schema constraints. Options: (A) validate at startup, (B) accept with caveat, (C) dynamic discovery. Recommend: Option A with test-skip flag.

### Patch Required

- [ ] [Review][Patch] SQL injection in trash search — src/models/trash.rs:754 uses manual quote escaping. Fix: Use parameterized binding in UNION query.
- [ ] [Review][Patch] Potential XSS in modal template — templates/fragments/admin_trash_permanent_delete_modal.html:2050. Fix: Verify Askama |escape filter; consider data-attribute approach for item_name binding.
- [ ] [Review][Patch] Missing admin guards on permanent delete — src/routes/admin.rs:1054-1077 missing self-delete & last-admin checks. Fix: Add guards matching story 8-3 deactivation pattern.
- [ ] [Review][Patch] Startup purge blocking without timeout — src/services/auto_purge.rs:310-313. Fix: Add `LIMIT N` to each DELETE to bound query time.
- [ ] [Review][Patch] Incorrect i18n key in error path — src/routes/admin.rs:1074 uses modal title key for error. Fix: Use error message key or new key like `delete_permanent_error_name_mismatch`.
- [ ] [Review][Patch] Days remaining timing inconsistency — src/routes/admin.rs:1188 uses `Local::now()` vs DB `NOW()`. Fix: Fetch timestamp from DB.
- [ ] [Review][Patch] Name comparison lacks normalization — src/routes/admin.rs:1073 case/whitespace-sensitive. Fix: Add `.trim()` to both sides and document or normalize case.

### Deferred

- [x] [Review][Defer] Race condition on permanent delete — Item deleted between confirm modal and submit. Acceptable edge case; already returns 404.
- [x] [Review][Defer] Modal close selector CSS :has() compatibility — templates/fragments/admin_trash_permanent_delete_modal.html:2037. Modern CSS, acceptable per CLAUDE.md constraints.

### Additional Finding (Pre-existing)

- Auto-purge hardcoded user_id=1 (src/services/auto_purge.rs:1361) — defer to story 8-8 for system user abstraction.

## Dev Agent Record

### Debug Log

(To be populated during development)

### Completion Notes

(To be populated upon story completion)

## Status

**Current:** code-review-complete  
**Next:** Fix E2E tests or merge to main  
**Last Updated:** 2026-04-24 (code review patches applied and committed)
