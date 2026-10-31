# Story 12.6: Epic 12 smoke test (Foundation Rule #7)

Status: done

Covers: Foundation Rule #7 (FR148–FR152 end to end). Epic 12 — sixth story; ships in v1.22.0.

## Story

As the project,
I want one end-to-end journey from a blank browser through the epic's core flow,
so that the epic is proven done.

## Acceptance Criteria

1. **AC1 — blank browser, real logins.** No injected cookie; `loginAs` for admin (key creation) and librarian.
2. **AC2 — the chain.** A hyphenated ISBN typed on `/catalog` creates the title under its bare ISBN-13 (12.1) → a `propose` key → a scripted MCP client reads the `initialize` instructions (book providers named, "search other sources first" — 12.3) → files a metadata proposal correcting the **title** and adding a description (12.5), and a cover proposal → on `/proposals` the cover row shows its thumbnail and the metadata row its fields; the subject opens the proposal (12.4) → the cover is accepted, then the metadata proposal, which is **not** outdated (12.2) → the title page shows the corrected title and the installed cover.
3. **AC3 — isolation.** Unique data (`specIsbn("EZ", <run stamp>)`), no `waitForTimeout`, key revoked in `afterEach` from a fresh context; green under `CI=true npx playwright test --workers=2` (Rule #13); rerunnable on a non-reset stack.
4. **AC4 — if this test fails, the epic is not done.**

## Tasks / Subtasks

- [x] T1 — `tests/e2e/specs/journeys/proposals-epic12-smoke.spec.ts`
- [x] T2 — targeted run on a reset stack and again on the non-reset stack; full suite with `CI=true --workers=2` before the push

## Dev Agent Record

### Agent Model Used

claude-opus-5-5

### Completion Notes List

- One test, ~3 s: hyphenated ISBN → `get_title` polled until the background fetch settled (publisher "Test Publisher", `isbn` bare) → `initialize` instructions (`/^- book: BnF, /m`, "Search other sources first") → `propose_title_metadata` (title + description, pinned `version`) and `propose_cover` (1×1 PNG) → list: thumbnail loaded (`naturalWidth > 0`), `[data-proposed-fields]` names Title and Description → subject link (accessible name "… proposal #N") opens the cover, accept → back to the list, subject link opens the metadata proposal (still pending), accept → title page `h1` shows the corrected title, the cover `img[src^="/covers/{id}.jpg?v="]` is visible.
- Green on a reset stack and again on the same stack (per-run ISBN stamp).

### Review Findings

Code review 2026-10-31 (one pass): **0 Medium+**, 7 Low. Patched: the accepted description is asserted on the title page too; the mock-server assumption (no cover for an "EZ" ISBN) is documented in the spec header. Dismissed: key left active if `newProposeKey` fails after creation (same as the epic-11 smoke); page-1 presence under heavy parallel filing (needs > 23 filings in between); lazy thumbnail below the fold (same root); key/scan order vs the epic wording (cosmetic); a 25 h-multiple re-run collision (negligible).

### CI finding (2026-10-31) — a real deadlock, fixed here

The first CI run of this branch failed `proposals-review.spec.ts:179` (the reject modal stayed open): the server answered **500 — MariaDB 1213 deadlock**. Running the new smoke in parallel exposed it: accepting a title correction (12-5) ran `carry_identity_title` as `UPDATE proposal_items … JOIN proposals … WHERE status = 'pending'`, which share-locked **every pending proposal of the library** through `idx_proposals_status`; a rejection on another title (not retried) deadlocked with it. `cover_accepted_since` (12-2) had the same JOIN shape. Fix: both queries start on the title's items (`FORCE INDEX (idx_proposal_items_target)` + `STRAIGHT_JOIN`), and the carry-forward is a plain id read followed by an `UPDATE … WHERE id IN (…)` on those items only. Regression test `an_acceptance_on_one_title_does_not_lock_proposals_of_another` (`tests/proposal_accept_metadata_cover.rs`): with an acceptance's locks held on title A, rejecting a proposal of title B completes — verified to fail (1205 lock wait timeout) without the fix.

### File List

- `tests/e2e/specs/journeys/proposals-epic12-smoke.spec.ts` (new)
- `src/models/proposal.rs` — lock scope of `carry_identity_title` / `cover_accepted_since` (CI deadlock fix)
- `tests/proposal_accept_metadata_cover.rs` — regression test
- `_bmad-output/implementation-artifacts/12-6-epic-12-smoke-test.md` (new)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — 12-6 line
