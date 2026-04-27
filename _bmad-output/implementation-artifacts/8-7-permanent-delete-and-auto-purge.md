---
story_key: 8-7
epic: 8
story: 7
title: Permanent delete and auto-purge
status: review
created: 2026-04-24
last_updated: 2026-04-27
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

### Review Findings

Round 2 code review (2026-04-27, post code-review-fixes commit `1a379cf`). Sources: blind+edge+auditor.

**Decision-needed (resolved 2026-04-27 — all converted to patches P29/P30/P31):**

- [x] [Review][Decision→Patch] D1 → option (a): create dedicated SYSTEM user via migration; introduce `SYSTEM_USER_ID` constant; replace hardcoded `user_id=1` in auto_purge audit. → see P29
- [x] [Review][Decision→Patch] D2 → option (a): add `users` to `ALLOWED_TABLES` so deactivated admins surface in Trash and the existing self-delete + last-active-admin guards become reachable per Task 8 Scenario 3. → see P30
- [x] [Review][Decision→Patch] D3 → option (a): migration to switch `admin_audit.user_id` FK from `CASCADE` to `SET NULL`; capture `user_email` snapshot in audit `details` JSON at action time to preserve forensics. → see P31

**Patch (unambiguous fixes):**

- [x] [Review][Patch] P1: AC4 violated — startup purge writes no `admin_audit` row; only scheduler does. [src/services/auto_purge.rs:1393, src/main.rs:521]
- [x] [Review][Patch] P2: AC5 violated — daily cadence hardcoded `Duration::from_secs(86400)`, not from `settings.auto_purge_interval_seconds`. [src/tasks/auto_purge_scheduler.rs:2002]
- [ ] [Review][Patch] P3: AC6 violated — `deletion_order` lists child tables (`title_contributors`, `series_title_assignments`, `volume_locations`, `loans`) but `if !ALLOWED_TABLES.contains(table) { continue; }` skips them all. Children-first FK ordering is a no-op; parent DELETEs hit FK violations and roll back silently → auto-purge effectively non-functional. [src/services/auto_purge.rs:1366-1416 vs src/services/soft_delete.rs:12]
- [x] [Review][Patch] P4: Task 4 missing — `MYBIBLI_SKIP_STARTUP_PURGE` env-var feature flag not implemented. [src/main.rs]
- [x] [Review][Patch] P5: NFR violated — `validate_schema(...).expect("FK schema validation failed")` panics on any missing whitelisted table; turns schema evolution into a hard crash. [src/main.rs:516-518]
- [ ] [Review][Patch] P6: AC1 violated — modal Cancel button `hx-delete="{{ modal_close_target }}"` issues HTTP DELETE to a CSS selector string → 405/404, modal never closes. [templates/fragments/admin_trash_permanent_delete_modal.html:2216, src/routes/admin.rs:1104]
- [x] [Review][Patch] P7: `LIMIT 10000` per DELETE with no drain loop → if >10k stale rows accumulate they're never purged. [src/services/auto_purge.rs:1432]
- [x] [Review][Patch] P8: CI gate violation — 3× `waitForTimeout(3000)` in E2E tests; will fail the `e2e` job grep gate. [tests/e2e/specs/journeys/admin-permanent-delete.spec.ts:2246,2287,2333]
- [ ] [Review][Patch] P9: E2E tests are conditional smoke (`if (tableExists) { if (btnExists) {...} }`); pass silently when no trash items exist. Spec scenarios 1, 2, 3 are uncovered. Foundation Rules #3 + #7 violated. [tests/e2e/specs/journeys/admin-permanent-delete.spec.ts]
- [x] [Review][Patch] P10: AC3 partial — audit `details` JSON omits per-table counts (only `tables_processed`, `rows_deleted`, `errors_count`). Spec example shows `titles=5, volumes=12, contributors=2`. [src/services/auto_purge.rs:1471]
- [x] [Review][Patch] P11: Task 9 missing — `docs/architecture.md` "Permanent Delete & Auto-Purge" section not added. [docs/architecture.md]
- [x] [Review][Patch] P12: Permanent delete handler resets `entity_type=None, search=None, page=1` — admin loses pagination/filter context after every delete. [src/routes/admin.rs:1190-1191]
- [x] [Review][Patch] P13: `AdminAuditModel::create` returns `timestamp: chrono::Local::now().naive_local()` instead of DB-assigned `NOW()`; in-memory struct drifts from stored row. [src/models/admin_audit.rs:599-607]
- [x] [Review][Patch] P14: `AdminAuditModel::list()` builds a dead `Vec<String>` of bindings (never used) + reads `details` JSON column without `CAST(... AS CHAR)` per CLAUDE.md MariaDB JSON-as-BLOB rule → decode error possible. [src/models/admin_audit.rs:618-660]
- [ ] [Review][Patch] P15: `deletion_order` is hand-curated separately from `ALLOWED_TABLES` — drift is inevitable; any new whitelisted table is silently skipped from auto-purge. Need single source of truth. [src/services/auto_purge.rs:1366]
- [ ] [Review][Patch] P16: After delete, panel re-render targets `#admin-trash-panel` but the open `<dialog>` is left in the DOM (panel isn't its parent). User sees the panel update behind a still-open modal. [src/routes/admin.rs:1190, modal hx-target]
- [ ] [Review][Patch] P17: Modal `<dialog open>` injected via `hx-swap="beforeend"` is never opened via JS `showModal()` → no top-layer / no inert backdrop / no Esc-to-close. Scanner-guard MutationObserver may not pick it up. [templates/fragments/admin_trash_permanent_delete_modal.html:2191]
- [ ] [Review][Patch] P18: Series restore SQL invalid — `UPDATE series_title_assignments SET series_id = NULL` violates a NOT NULL constraint, and the correlated subquery referencing alias `sta` in SET is rejected by MariaDB ("can't specify target table for update"). Will throw on any actual restore conflict. [src/services/trash.rs:1761-1773] — *belongs to 8-6 carryover, see DF1 below*
- [x] [Review][Patch] P19: `tokio::time::interval(86400)` uses default `MissedTickBehavior::Burst` → rapid-fire purges if system clock jumps forward (NTP, suspend/resume). Use `Skip` or `Delay`. [src/tasks/auto_purge_scheduler.rs:2002]
- [x] [Review][Patch] P20: `user_id` shadowed inconsistently in same handler: `unwrap_or(0)` for self-delete check, `unwrap_or(1)` for audit attribution. [src/routes/admin.rs:1126,1174]
- [x] [Review][Patch] P21: `days_remaining = (now - deleted_at).num_days()` truncates toward zero — row deleted 29d23h ago shows "1 day remaining" then is purged within the hour. Use ceiling or surface hours when <1d. [src/routes/admin.rs:1281]
- [x] [Review][Patch] P22: `SELECT NOW()` fired on every trash-panel render to compute `days_remaining` — unnecessary roundtrip. [src/routes/admin.rs:1271]
- [x] [Review][Patch] P23: Pagination uses global `total` count but `entries` is filter-scoped → "page 1 / 47" on filtered empty result; "next page" leads to empty pages. [src/routes/admin.rs:1268]
- [x] [Review][Patch] P24: LIKE wildcards (`%`, `_`, `\`) inside `search` not escaped before `format!("%{}%", search)` → search for `100%` matches everything. [src/models/trash.rs:807]
- [x] [Review][Patch] P25: Pagination URLs in template emit `entity_type=...&search=...` without URL-encoding → search containing `&`, `#`, space, or `+` produces broken pagination links. [templates/fragments/admin_trash_panel.html:2174]
- [x] [Review][Patch] P26: CLAUDE.md auto-purge bullet overstates behavior — claims "Both auto-purge and admin delete block self-deletion and preserve the last active admin"; auto-purge has no such guard (deletes any row >30d regardless of role). Fix the doc OR add the guard. [CLAUDE.md]
- [ ] [Review][Patch] P27: Modal duplicates if "Delete permanently" clicked multiple times — no cleanup of prior `<dialog>` before injecting next one. [admin_trash_panel.html / modal hx-swap]
- [x] [Review][Patch] P28: Hardcoded `⚠️` emoji in modal template violates CLAUDE.md "Avoid emojis... unless asked"; also bypasses i18n. [templates/fragments/admin_trash_permanent_delete_modal.html:2195]
- [ ] [Review][Patch] P29 (from D1): create migration adding a `SYSTEM` user (role=system, cannot login, deterministic id reserved e.g. `SYSTEM_USER_ID = 0` or `i64::MAX`); introduce `pub const SYSTEM_USER_ID` in `src/services/auto_purge.rs` (or shared constants); replace hardcoded `1` in `record_purge_audit`. Verify FK still satisfied. [src/services/auto_purge.rs:1483, new migration]
- [ ] [Review][Patch] P30 (from D2): add `"users"` to `ALLOWED_TABLES` in `src/services/soft_delete.rs:12-19`; verify trash UNION list includes deactivated users (already supported by story 8-3 deletion_at semantics); ensure self-delete + last-active-admin guards in `src/routes/admin.rs:1136-1156` now activate; add E2E coverage for Task 8 Scenario 3 (admin attempts to permanent-delete the only remaining admin → blocked with FeedbackEntry). [src/services/soft_delete.rs:12, src/routes/admin.rs:1136-1156]
- [ ] [Review][Patch] P31 (from D3): new migration altering `admin_audit.user_id` FK from `ON DELETE CASCADE` to `ON DELETE SET NULL`; modify `record_purge_audit` and `permanent_delete` audit insertion to capture `user_email` (and `user_role` for completeness) inside `details` JSON at action time so attribution survives even if the user row is later purged. [migrations/<new>, src/services/auto_purge.rs:1467-1491, src/routes/admin.rs:1175-1183]

**Deferred (out of scope for 8-7 — track via GitHub Issues per Foundation Rule #11):**

- [x] [Review][Defer] DF1: Story 8-6 code (trash list, restore, restore_with_conflicts_cleared, verify_parent_exists) is included in the 8-7 diff because 8-6 wasn't shipped separately. PR-strategy question, not a code defect — discuss whether to split the PR. [src/services/trash.rs, src/models/trash.rs, src/routes/admin.rs trash-list handler]
- [x] [Review][Defer] DF2: `AdminAuditModel::list()` (160+ lines, no AC requires it) — pre-built audit log lister with no consumer in this story. Either remove or leave for 8-8 / future audit-log UI. [src/models/admin_audit.rs:618-660]
- [x] [Review][Defer] DF3: Restore TOCTOU between UPDATE-returning-zero and follow-up SELECT for 409 vs 404 distinction — story 8-6 issue, not 8-7. [src/services/trash.rs:1645-1656]

**Note on `deferred-work.md`:** Per Foundation Rule #11, defer items should be tracked as GitHub Issues with label `type:code-review-finding`, NOT in a local markdown doc. The skill's default `deferred-work.md` write was skipped accordingly.

### GitHub Issues for un-applied findings (created 2026-04-27)

The 11 skipped patches and 3 deferred items are tracked as separate GitHub issues (label `type:code-review-finding`):

| Finding | Issue | Title (short) |
|---|---|---|
| P3  | [#60](https://github.com/guycorbaz/mybibli/issues/60) | Auto-purge FK ordering broken (children skipped) |
| P6  | [#61](https://github.com/guycorbaz/mybibli/issues/61) | Modal Cancel button uses hx-delete with CSS selector |
| P9  | [#62](https://github.com/guycorbaz/mybibli/issues/62) | E2E tests are conditional smoke (no real assertions) |
| P15 | [#63](https://github.com/guycorbaz/mybibli/issues/63) | deletion_order vs ALLOWED_TABLES drift |
| P16 | [#64](https://github.com/guycorbaz/mybibli/issues/64) | Modal stays in DOM after delete |
| P17 | [#65](https://github.com/guycorbaz/mybibli/issues/65) | `<dialog>` rendered without showModal() |
| P18 | [#66](https://github.com/guycorbaz/mybibli/issues/66) | Series restore SQL invalid (NULL on NOT NULL) |
| P27 | [#67](https://github.com/guycorbaz/mybibli/issues/67) | Modal duplicates on rapid double-click |
| P29 | [#68](https://github.com/guycorbaz/mybibli/issues/68) | Create dedicated SYSTEM user (D1) |
| P30 | [#69](https://github.com/guycorbaz/mybibli/issues/69) | Add `users` to ALLOWED_TABLES (D2) |
| P31 | [#70](https://github.com/guycorbaz/mybibli/issues/70) | FK CASCADE → SET NULL + capture user_email (D3) |
| DF1 | [#71](https://github.com/guycorbaz/mybibli/issues/71) | 8-6 code in 8-7 diff (PR strategy) |
| DF2 | [#72](https://github.com/guycorbaz/mybibli/issues/72) | AdminAuditModel::list out of scope |
| DF3 | [#73](https://github.com/guycorbaz/mybibli/issues/73) | Restore TOCTOU 409 vs 404 (8-6 issue) |

### Patch batch outcome (2026-04-27)

20 patches applied automatically by sub-agent: P1, P2, P4, P5, P7, P8, P10–P14, P19–P26, P28.
Build status: `cargo check` + `cargo clippy --all-targets -- -D warnings` clean. `cargo test --lib` 531 passed against the rust-test docker DB on port 3307.
Cross-cutting refactors introduced (see commit pending): `PurgeStats.per_table` field, `AppSettings.auto_purge_interval_seconds` setting, `auto_purge_scheduler::spawn(pool, settings)` signature change, `TrashModel::trash_count(pool, filter, search)` signature change, `Cargo.toml` askama urlencode feature, new `docs/architecture.md`.

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
