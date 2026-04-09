# Story 5.2: Contributor Deletion Guard

Status: done

## Story

As a librarian,
I want to be prevented from deleting a contributor still referenced by titles,
so that I don't leave orphaned references in my catalog.

## Acceptance Criteria

1. **Guard blocks deletion when titles exist:** Given a contributor referenced by at least one title, when librarian clicks delete, then deletion is blocked with a message showing the count of referencing titles
2. **Delete succeeds when no titles:** Given a contributor with zero title references, when librarian clicks delete, then native browser confirm dialog appears and soft-delete proceeds on confirmation
3. **Error message follows NFR38 pattern:** Given the error message, when displayed, then it follows the "What happened -> Why -> What you can do" pattern with i18n key `error.contributor.has_titles`
4. **Unit test:** `ContributorService::delete_contributor()` returns `AppError::Conflict` when referencing titles exist
5. **E2E smoke:** create contributor -> assign to title -> attempt delete -> see block message -> unassign -> delete succeeds

## Tasks / Subtasks

- [x] Task 1: Update error type and i18n keys (AC: #3, #4)
  - [x] 1.1 Change `ContributorService::delete_contributor()` to return `AppError::Conflict` instead of `AppError::BadRequest`
  - [x] 1.2 Update i18n key to `error.contributor.has_titles` with NFR38 pattern; kept legacy `contributor.delete_blocked` updated too
  - [x] 1.3 Update route handler `delete_contributor()` to match on `AppError::Conflict`
  - [x] 1.4 Force i18n proc macro recompilation — build clean
- [x] Task 2: Add delete button to contributor detail page (AC: #1, #2)
  - [x] 2.1 Added delete button with `hx-delete`, `hx-confirm`, `hx-target="#contributor-feedback"` (Librarian+ role guard)
  - [x] 2.2 Added feedback container `<div id="contributor-feedback" aria-live="polite">`
  - [x] 2.3 Added `delete_label` and `confirm_delete` fields to `ContributorDetailTemplate` struct and handler
- [x] Task 3: Add redirect on successful deletion (AC: #2)
  - [x] 3.1 Dual HTMX/non-HTMX redirect pattern: `HX-Redirect: /catalog` header or `Redirect::to("/catalog")`
- [x] Task 4: Unit tests (AC: #4)
  - [x] 4.1 Added `test_deletion_guard_returns_conflict_variant` verifying Conflict error construction
  - [x] 4.2 All 291 existing tests pass — zero regressions
- [x] Task 5: E2E smoke test (AC: #5)
  - [x] 5.1 Replaced stub with full deletion guard test: scan ISBN → add contributor → extract IDs → navigate to detail → delete blocked → verify error message
  - [x] 5.2 Unassign via `page.request.post()` → retry delete → verify redirect to /catalog
- [x] Task 6: Verification
  - [x] 6.1 `cargo clippy -- -D warnings` — clean
  - [x] 6.2 `cargo test` — 291 passed, 0 failed
  - [x] 6.3 No SQL query changes — sqlx prepare not needed
  - [x] 6.4 Full E2E suite — 120/120 passed

### Review Findings

- [x] [Review][Decision] NFR38 message structure — dismissed by Guy: message is clear and actionable as-is
- [x] [Review][Patch] Remove orphaned i18n key `contributor.delete_blocked` — removed from en.yml and fr.yml
- [x] [Review][Patch] Handle NotFound in delete_contributor error path — added match arm for `AppError::NotFound` [src/routes/catalog.rs:1340]
- [x] [Review][Defer] TOCTOU race between count check and soft_delete — no transaction wrapping. Pre-existing pattern shared by location/borrower guards. Low real-world risk for single-user app.
- [x] [Review][Defer] count_title_associations doesn't filter soft-deleted titles — documented OUT OF SCOPE in story, pre-existing bug.
- [x] [Review][Defer] HTMX fragment path missing delete button — contributor_detail_fragment() doesn't include delete button. Low priority: HTMX partial navigation is rare for detail pages.
- [x] [Review][Defer] waitForTimeout(1000) anti-pattern in existing duplicate-contributor test — pre-existing, not part of this story's changes.
- [x] [Review][Defer] No E2E for double-delete scenario — edge case, low priority.

## Dev Notes

### What Already Exists (DO NOT recreate)

The backend deletion guard is **already implemented** but needs refinement:

- **Service:** `src/services/contributor.rs:131-147` — `ContributorService::delete_contributor()` already checks `count_title_associations()` and blocks deletion. Currently returns `AppError::BadRequest` (needs to change to `AppError::Conflict` per AC#4).
- **Model:** `src/models/contributor.rs` — `ContributorModel::count_title_associations()` counts active title associations via `title_contributors` junction. `soft_delete()` method exists.
- **Route:** `src/routes/catalog.rs:1315-1337` — `delete_contributor()` handler exists, registered as `DELETE /catalog/contributors/{id}` (plural). Currently matches `AppError::BadRequest` (update to match `Conflict`).
- **i18n:** `locales/en.yml:91` has `contributor.delete_blocked`, `locales/fr.yml:91` has the French equivalent. These need to be renamed/restructured to `error.contributor.has_titles` with NFR38 pattern.
- **E2E:** `tests/e2e/specs/journeys/catalog-contributor.spec.ts:132-140` has a placeholder test "delete contributor with associations shows error" that is currently a no-op stub.

### What's Missing (the actual work)

1. **UI delete button** on `templates/pages/contributor_detail.html` — currently has NO delete button at all. Follow the borrower detail pattern: `templates/pages/borrower_detail.html:27` shows `<button hx-delete="/borrower/{{ borrower.id }}" hx-confirm="...">`.
2. **Error type change**: `BadRequest` -> `Conflict` in service and route handler.
3. **i18n key rename**: `contributor.delete_blocked` -> `error.contributor.has_titles` with NFR38 three-part message.
4. **HX-Redirect on success**: After successful soft-delete, redirect user away from the deleted contributor's page (currently returns only feedback_html with no redirect).
5. **Real E2E tests** replacing the stub.

### Patterns to Follow

**Delete button pattern** (from `templates/pages/borrower_detail.html:21-33`):
```html
{% if role == "admin" %}
<div class="mt-6 flex gap-3">
    <button hx-delete="/borrower/{{ borrower.id }}" hx-confirm="{{ confirm_delete }}"
            hx-target="body"
            class="px-3 py-1.5 text-sm font-medium text-red-600 ...">
        {{ delete_label }}
    </button>
</div>
{% endif %}
<div id="borrower-feedback" class="mt-4" aria-live="polite"></div>
```

For the contributor delete button:
- Use `hx-delete="/catalog/contributors/{{ contributor.id }}"` (plural path, matches `src/routes/mod.rs:53-56`)
- Use `hx-target="#contributor-feedback"` (NOT `body`) — blocked deletions return a feedback fragment
- Successful deletions are handled server-side via `HX-Redirect` header (HTMX follows the redirect automatically, so `hx-target` is irrelevant on success)
- Guard with `{% if role == "librarian" || role == "admin" %}` — the template already receives `role` from `ContributorDetailTemplate` at `src/routes/contributors.rs:16`

**Deletion guard pattern** (from `src/services/locations.rs`):
- Count query -> if > 0, return localized error -> else soft_delete
- Location service uses `AppError::BadRequest` but the epic AC specifies `AppError::Conflict` for contributor guard

**Redirect after delete pattern** (from `src/routes/borrowers.rs:315-336`):
```rust
// Dual HTMX/non-HTMX response — MUST handle both
if is_htmx {
    Ok((
        axum::http::StatusCode::OK,
        [(axum::http::header::HeaderName::from_static("hx-redirect"), "/catalog".to_string())],
        String::new(),
    ).into_response())
} else {
    Ok(Redirect::to("/catalog").into_response())
}
```
The handler needs `HxRequest(is_htmx): HxRequest` as a parameter (it currently doesn't have it).

**`hx-confirm` uses native browser `confirm()` dialog** — NOT a custom modal. This is consistent across all delete buttons in the project (borrower, location). Do NOT build a custom modal.

**NFR38 error message pattern** ("What happened -> Why -> What you can do"):
```yaml
# en.yml
error:
  contributor:
    has_titles: "Cannot delete %{name}. This contributor is associated with %{count} title(s). Remove the contributor from all titles first."
# fr.yml
error:
  contributor:
    has_titles: "Impossible de supprimer %{name}. Ce contributeur est associé à %{count} titre(s). Retirez d'abord le contributeur de tous les titres."
```

**CRITICAL — HTMX does NOT swap non-2xx responses into `hx-target`.** On 4xx/5xx, HTMX fires `htmx:responseError` which shows a generic "Server error (N)" message (see `static/js/mybibli.js:129-140`). The current handler already works around this: it catches the error in the `Err` branch and returns `Ok(Html(feedback_html("error", &message, "")))` — a **200 OK** with error-styled HTML. After changing the service to return `AppError::Conflict` (Task 1.1), the route handler (Task 1.3) must still match it and return `Ok(Html(...))`, NOT let the Conflict propagate as a 409 HTTP response.

**HTMX response for successful deletion:** Handled by the dual HTMX/non-HTMX redirect pattern described above.

**Contributor detail route handler** — template struct is in `src/routes/contributors.rs:12-29` (NOT in `catalog.rs`). It already has `role: String` at line 16. Add only:
- `confirm_delete: String` — i18n confirm text (e.g., "Delete this contributor?")
- `delete_label: String` — button label

Populate these in the handler at `src/routes/contributors.rs:31-68`.

### E2E Test Strategy

**specId:** `CC` (already allocated for catalog-contributor in `tests/e2e/specs/journeys/catalog-contributor.spec.ts`)

**Test: deletion guard blocks when titles assigned**
1. Login via `loginAs(page)`
2. Create a title by scanning `specIsbn("CC", 10)` on `/catalog`
3. Add a contributor (unique name like `"CC-DeleteGuard-Author"`) to that title via the contributor form
4. Navigate to `/title/{id}` (title detail page) -> find contributor link `a[href^="/contributor/"]` matching the contributor name -> extract contributor ID from href -> navigate to `/contributor/{id}`
5. Set up Playwright dialog handler BEFORE clicking: `page.on('dialog', d => d.accept())` — `hx-confirm` triggers the native browser `confirm()` which Playwright blocks by default
6. Click delete button -> native confirm auto-accepted -> verify `#contributor-feedback` shows block message: `toContainText(/Cannot delete|Impossible de supprimer/i)`

**Test: deletion succeeds after unassigning**
1. Continue from above (or start fresh with same setup)
2. Navigate to `/catalog` and set title context by scanning the same ISBN
3. Find the `×` remove button for the contributor — it's generated by `contributor_list_html()` in `src/routes/catalog.rs:1418-1426`, rendered on the catalog page (NOT on title detail page). Uses `hx-post="/catalog/contributors/remove"` with `junction_id` and `title_id` params via `hx-vals`. Target: `#feedback-list`.
4. Click the remove button -> wait for feedback in `#feedback-list` confirming removal (i18n: `contributor.removed`)
5. Navigate to `/contributor/{id}` -> set `page.on('dialog', d => d.accept())` -> click delete -> confirm auto-accepted
6. Verify redirect to `/catalog` via `page.waitForURL('**/catalog')`

**Test selector priority** (per CLAUDE.md):
1. `page.getByRole(...)` for buttons
2. `page.locator("#contributor-feedback")` for feedback container
3. `page.getByText(/Cannot delete|Impossible de supprimer/i)` for i18n-aware error messages

### Project Structure Notes

- All changes align with existing architecture: thin routes, service-layer business logic, model-layer queries
- No new files needed — all modifications to existing files
- The `DeletionBlocked(String, i64)` variant from `architecture.md` is NOT implemented in the actual `AppError` enum. Use `AppError::Conflict` as specified in the epic AC. Do NOT add `DeletionBlocked` — that would be scope creep.

### Known Pre-existing Issue (OUT OF SCOPE)

`ContributorModel::count_title_associations()` at `src/models/contributor.rs:128-137` filters `title_contributors.deleted_at IS NULL` but does NOT JOIN on `titles` to exclude soft-deleted titles. This means a contributor whose only associated title has been moved to trash would still be blocked from deletion. This is a pre-existing bug shared by the existing guard logic — do NOT fix in this story (would require changing the query semantics and updating tests). File as deferred work if encountered during testing.

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Story 5.2 AC]
- [Source: _bmad-output/planning-artifacts/architecture.md — Lines 847-860 (AppError), 941-964 (soft-delete lifecycle)]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md — Lines 1351-1357 (deletion block pattern), 1855-1885 (modal component)]
- [Source: src/services/contributor.rs:131-147 — existing guard implementation]
- [Source: src/routes/catalog.rs:1315-1337 — existing delete route handler]
- [Source: src/routes/contributors.rs:12-29 — ContributorDetailTemplate struct (has role field)]
- [Source: src/routes/contributors.rs:31-68 — contributor_detail handler]
- [Source: src/routes/catalog.rs:1418-1426 — contributor_list_html remove button (× on catalog page)]
- [Source: src/routes/borrowers.rs:330 — HX-Redirect after delete pattern]
- [Source: templates/pages/borrower_detail.html:21-33 — delete button UI pattern]
- [Source: templates/pages/contributor_detail.html — current template (no delete button)]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Changed deletion guard error from `AppError::BadRequest` to `AppError::Conflict` in service and route handler
- Added NFR38-pattern i18n key `error.contributor.has_titles` under `error:` section in both locale files; updated legacy `contributor.delete_blocked` text for consistency
- Added delete button with `hx-confirm` native dialog to contributor detail page (Librarian+ only)
- Added `#contributor-feedback` container with `aria-live="polite"` for accessible error feedback
- Implemented dual HTMX/non-HTMX redirect after successful deletion (HX-Redirect header + Redirect::to)
- E2E test uses timestamp-based contributor name for data isolation across repeated runs
- E2E unassign step uses `page.request.post()` to remove junction directly (avoids catalog page OOB reload complexity)

### File List

**Modified:**
- `src/services/contributor.rs` — Changed `BadRequest` → `Conflict`, updated i18n key reference, added unit test
- `src/routes/catalog.rs` — Updated error match to `Conflict`, added `HxRequest` param + dual redirect on success, added `Redirect` import
- `src/routes/contributors.rs` — Added `delete_label` and `confirm_delete` fields to template struct and handler
- `templates/pages/contributor_detail.html` — Added delete button, role guard, feedback container
- `locales/en.yml` — Added `error.contributor.has_titles`, `contributor_detail.delete`, `contributor_detail.confirm_delete`; updated `contributor.delete_blocked`
- `locales/fr.yml` — Same i18n additions as en.yml (French translations)
- `tests/e2e/specs/journeys/catalog-contributor.spec.ts` — Replaced stub with full deletion guard E2E test

