# Polish iteration 2: Undo for recent scan actions

Status: review

**Bundle type:** post-Epic-10 issue-driven polish iteration (per Epic 10 retro action A5 — production-driven polish, no formal epic). Single GitHub issue, single feature.

**Closes:** #9

**Target release:** `v1.10.0` (minor — new feature, backwards-compatible, **no schema migration**). Ships alongside the pending `docs/scanner-hardware-confirmed` manual commit bundled on the same branch to save a CI cycle (user decision 2026-07-01).

**Risk lens — mybibli is in production.** Foundation Rule #6 (code-review default) MUST be honored on this PR. The undoable actions touch the **/catalog scan workflow** — the single most-used librarian path. A regression that reverses the wrong volume's location, or that fires reversal outside the 30s window, is immediately user-visible and corrupts shelf data. The reversal must be **exact** (restore the precise prior `location_id`, never guess) and **bounded** (server-enforced 30s window, single last-action only).

## Story

As a **Librarian** cataloging on the `/catalog` page,
I want to **undo my last scan action** (shelving a volume, or activating a batch storage-location) directly from the feedback list within a **30-second window**,
so that an accidental shelf assignment or a wrong batch-location activation can be reversed in one click without hunting through the volume-edit form — a safety net that reduces the anxiety of barcode-driven bulk work.

## ⚠️ Existing-code reality check (code map, 2026-07-01)

**Correction to issue #9's framing:** the issue says "from the feedback list" without naming the page. The mutating scans that produce feedback entries live on **`/catalog`**, NOT the home page. `GET /scan` (`src/routes/home_scan.rs:67`) is scan-to-**navigate** only — it redirects and never mutates or emits a feedback entry. All undoable actions flow through **`POST /catalog/scan` → `handle_scan`** (`src/routes/catalog.rs:332`, route registered `src/routes/mod.rs:81`). Build the feature against `/catalog` and its `#feedback-list`.

### The two undoable forward actions

| # | Action | Forward call | Call site(s) | Feedback key |
|---|---|---|---|---|
| A | **Shelve / attach a volume to a location** | `VolumeModel::update_location(pool, volume_id, Some(loc_id))` (`src/models/volume.rs:145`) | `catalog.rs:516` (re-scan existing V-code while batch location active), `catalog.rs:668` (new V-code auto-shelved right after `create_volume`), `catalog.rs:~833` (L-code scan with a pending `last_volume_label` → `VolumeService::assign_location`, `src/services/volume.rs:73`, which internally calls `update_location`) | `feedback.volume_shelved` / `feedback.volume_created_and_shelved` |
| B | **Activate a batch storage-location** (session state) | `SessionModel::set_active_location(pool, token, location.id)` (`src/models/session.rs:202`) | `catalog.rs:827` | `feedback.active_location` |

### Reversibility facts (load-bearing)

- **Action A prior-state is NOT recoverable after the fact.** `VolumeModel::update_location` (`volume.rs:145`) is a blind `UPDATE volumes SET location_id = ? …` — it does NOT capture the previous `location_id`, does NOT bump the `version` optimistic-lock column, writes NO audit row. **Therefore the prior `location_id` MUST be captured at action time**, before the UPDATE fires. The volume id is already in scope at each call site (`existing_vol.id` / `volume.id`).
  - Sub-case A2 (new volume auto-shelved, `catalog.rs:668`): the volume was just created, so its prior `location_id` is `None`. **Undo detaches it (sets `location_id = NULL`) — it does NOT delete the created title/volume.** Reversing entity *creation* is explicitly out of scope (see Out of scope).
- **Action B prior-state IS recoverable.** `SessionModel::get_active_location` (`session.rs:212`) read BEFORE the mutation yields the prior value; `clear_active_location` (`session.rs:217`) already exists. When an L-code shelve (A, case 833) also clears `last_volume_label` (`session.rs:174`, set to `""`), undoing that shelve must ALSO restore the prior `last_volume_label`.

### Session-state mechanism to reuse (NO migration)

Ephemeral per-session state already lives in the `sessions.data` JSON blob, loaded/saved via `SessionModel::load_session_data` (`session.rs:233`) + the `UPDATE sessions SET data = ?` save path (`session.rs:256-259`). Existing keys: `current_title_id`, `last_volume_label`, `session_item_count`, `active_location_id`. **Add one more key, `last_undoable_action`** — no schema change, same read/write plumbing as `set_active_location`.

### Feedback rendering + client patterns to reuse

- `feedback_html(variant, message, suggestion) -> String` (`src/utils.rs:85`) renders `<div class="… feedback-entry" role="status" data-feedback-variant="{variant}">`. Today it carries only `data-feedback-variant`. The undo affordance is appended to the entry (see AC3). `html_escape` (`src/utils.rs:63`) escapes `& < > " '` — every dynamic string reaching HTML MUST pass through it.
- **Delegated `data-action` handlers** live in `static/js/mybibli.js` (document-level `click` delegation, CSP-clean, zero inline JS): `dismiss-feedback` (`mybibli.js:195-201`), `provider-key-clear-toggle` (`:209`). A new `data-action="undo-scan"` handler slots in here.
- A **`MutationObserver` on `#feedback-list`** already exists (`mybibli.js:99-124`, reads `data-feedback-variant` for scan audio). Reuse/extend it to start the 30s client timer when a new entry carrying an undo button lands.
- **Revert-via-endpoint precedent:** `static/js/inline-form.js:156-190` (`admin-modal-close-revert-row`) fires an HTMX call to a server revert endpoint. Closest existing "undo" pattern — mirror its shape.
- **CSRF:** any new POST through HTMX is auto-covered — `static/js/csrf.js` injects `X-CSRF-Token` on `htmx:configRequest`; `src/middleware/csrf.rs` validates. No extra work.

## Acceptance Criteria

### AC1 — Server-authoritative undo log in `sessions.data`

- Add `SessionModel` methods (mirroring `set/get/clear_active_location`, `session.rs:202-221`):
  - `set_last_undoable_action(pool, token, action: &UndoableAction)` — serializes into `data["last_undoable_action"]`.
  - `get_last_undoable_action(pool, token) -> Option<UndoableAction>` — deserializes; returns `None` if absent or corrupt (log a `warn!` on corrupt, like `load_session_data` at `session.rs:247`).
  - `clear_last_undoable_action(pool, token)` — removes the key.
- `UndoableAction` (new type, e.g. `src/models/session.rs` or a small `src/services/scan_undo.rs`) is a `serde` struct:
  ```
  { kind: "shelve_volume" | "activate_location",
    volume_id: Option<u64>,          // set for shelve_volume
    prev_location_id: Option<u64>,   // volume's location BEFORE the shelve (None = was unshelved / freshly created)
    prev_active_location: Option<u64>,// session active-location BEFORE activation (for activate_location)
    prev_last_volume_label: Option<String>, // restored if the shelve cleared it
    at: NaiveDateTime }              // action timestamp, for the 30s window
  ```
- **Single-action semantics:** each new undoable forward action OVERWRITES `last_undoable_action`. Only the most recent action is ever undoable (fidelity to issue #9 "the last scan action").
- **The 30s window is decided by a pure, unit-testable helper:** `fn undo_is_within_window(at: NaiveDateTime, now: NaiveDateTime, window_secs: i64) -> bool`. The window constant is `pub(crate) const SCAN_UNDO_WINDOW_SECS: i64 = 30;` (v1 freeze, NOT admin-configurable — mirrors the `RECENT_ACTIVITY_DAYS` const pattern in `home_indicators.rs`). A unit test locks the value at 30.

### AC2 — The four call sites record the action (prior-state captured BEFORE mutation)

- `catalog.rs:516` (re-scan existing V-code, shelve at active location): read `existing_vol.location_id` (prior), fire `update_location`, then `set_last_undoable_action(kind=shelve_volume, volume_id=existing_vol.id, prev_location_id=<prior>, at=now)`.
- `catalog.rs:668` (new V-code auto-shelved): `prev_location_id = None` (freshly created), record after the successful shelve.
- `catalog.rs:~833` (L-code shelve of pending volume via `VolumeService::assign_location`): read the volume's prior `location_id`, and capture `prev_last_volume_label` (the label being consumed) so undo restores it. Record `kind=shelve_volume`.
- `catalog.rs:827` (activate batch location): read `get_active_location` (prior) BEFORE `set_active_location`, then record `kind=activate_location, prev_active_location=<prior>, at=now`.
- **Failure isolation:** recording the undo log MUST NOT break the scan flow if it fails — wrap in a best-effort path that logs a `warn!` and proceeds (the scan itself already succeeded; a missing undo log degrades gracefully to "no undo available"). Do NOT hold the DB write inside the same transaction as the scan mutation in a way that could roll the scan back.
- `handle_scan_with_type` (`catalog.rs:1163`) does NOT call `update_location` / `set_active_location` directly (verified — no such call after `:833`); no instrumentation needed there. If Task 1 finds a shelve path reachable only through it, record it too and note in Dev Agent Record.

### AC3 — Undo affordance rendered on the two undoable feedback entries only

- Introduce a DRY helper (e.g. `feedback_html_undoable(variant, message, suggestion, undo_label) -> String`) that calls `feedback_html` and appends an undo control INSIDE the `.feedback-entry` div:
  `<button type="button" data-action="undo-scan" hx-post="/catalog/undo" hx-target="#feedback-list" hx-swap="afterbegin" class="…">{undo_label}</button>`
  (No inline JS, no `onclick`, no `hx-confirm` — CSP-clean and audit-clean. The `hx-confirm=` allowlist is frozen empty; do NOT add one.)
- Only the `feedback.volume_shelved`, `feedback.volume_created_and_shelved`, and `feedback.active_location` entries get the button. All other feedback entries render via plain `feedback_html`, unchanged.
- `undo_label` comes from i18n `action.undo` (AC6). The button is keyboard-focusable and screen-reader labelled (`aria-label` via the same key or an explicit `feedback.undo_aria`).

### AC4 — `POST /catalog/undo` endpoint applies the reversal, server-enforced window

- New route `POST /catalog/undo` → `catalog::handle_undo`, registered in `src/routes/mod.rs` next to `/catalog/scan` (`:81`). Requires `Role::Librarian` (same gate as `handle_scan` — verify the exact extractor `handle_scan` uses and match it).
- Handler logic:
  1. `get_last_undoable_action`. If `None` → return a friendly info feedback `feedback.undo_nothing` ("Rien à annuler.") — HTTP 200, NOT an error. `clear` is a no-op.
  2. If `!undo_is_within_window(action.at, now, SCAN_UNDO_WINDOW_SECS)` → clear the log, return info feedback `feedback.undo_too_late` ("Trop tard pour annuler cette action.") — HTTP 200.
  3. Apply reversal by `kind`:
     - `shelve_volume`: `VolumeModel::update_location(pool, volume_id, action.prev_location_id)` (re-attach to prior, or detach if `None`). **Guard:** if `prev_location_id` is `Some(id)` but that location is now soft-deleted / gone, detach instead (`None`) and use a `feedback.undo_success_shelve` variant noting the volume was unshelved. Restore `prev_last_volume_label` into the session if present.
     - `activate_location`: restore the prior active location — `set_active_location(prev)` if `Some`, else `clear_active_location`. Restore `prev_last_volume_label` if it was captured.
  4. `clear_last_undoable_action` (the undo is itself NOT re-undoable — do NOT record a new undoable action for the reversal).
  5. Return success feedback (`feedback.undo_success_shelve` / `feedback.undo_success_activate_location`) into `#feedback-list` (afterbegin) **plus OOB refresh** of the scan-context surfaces that the reversal changed: `#context-banner` and `#session-counter` (the same OOB targets `handle_scan` updates — reuse its builders so counts/banner reflect the reverted state). Use `HtmxResponse { main, oob }`.
- **CSRF** is enforced automatically (POST + `csrf.js`). No exemption — the exempt allowlist stays frozen at `[("POST", "/login")]`.

### AC5 — Client behavior: 30s auto-expiry + single-use

- In `static/js/mybibli.js`:
  - Extend the existing `#feedback-list` MutationObserver (`:99-124`) OR add a sibling observer: when a new `.feedback-entry` containing a `[data-action="undo-scan"]` button is inserted, start a `setTimeout(30000)` that removes/hides the button (visual parity with the server window; the server is still authoritative).
  - The `data-action="undo-scan"` delegated `click` handler: HTMX already wired via the button's `hx-post` attributes, so the handler's job is to **disable the button immediately on click** (prevent double-undo) — set `disabled` + a busy state. If a second click somehow reaches the server, AC4 step 1/2 returns `undo_nothing` gracefully.
  - CSP-clean: no inline handlers; all logic in the module.
- When the undo response lands (a fresh feedback entry via afterbegin), the original entry's button is already disabled; no further coordination needed.

### AC6 — i18n in ALL FOUR locales (de / en / fr / it)

New keys (exact names at implementer's discretion, but consistent across locales) — `locale_parity` test is the gate (`cargo test --test locale_parity`, runs in the DB-integration job):
- `action.undo` — button label ("Undo" / "Annuler" / "Rückgängig" / "Annulla").
- `feedback.undo_success_shelve` — volume un-shelved / re-shelved to prior location.
- `feedback.undo_success_activate_location` — batch location activation reverted.
- `feedback.undo_too_late` — outside the 30s window.
- `feedback.undo_nothing` — nothing to undo.
- (optional) `feedback.undo_aria` — screen-reader label for the button.

Reference-data values are never translated (NFR41) — these are UI labels, so they ARE translated. After adding keys: `touch src/lib.rs && cargo build` to force the i18n proc-macro to re-read the YAML (per CLAUDE.md).

### AC7 — Unit tests (alongside implementation)

- `src/models/session.rs` (or the new `scan_undo.rs`): round-trip `set/get/clear_last_undoable_action`; corrupt-JSON → `None`; overwrite semantics (second set replaces first).
- Pure window helper: `undo_within_window` at 0s / 29s / 30s / 31s boundaries; and the `SCAN_UNDO_WINDOW_SECS == 30` freeze test.
- `src/routes/catalog.rs` handler tests (mirror the existing test module ~`catalog.rs:2530`):
  - `handle_scan` shelve path records an undoable action with the correct `prev_location_id`.
  - `handle_undo` reverses a shelve (asserts `volumes.location_id` back to prior), reverses an activate-location (asserts session active-location restored), and clears the log.
  - `handle_undo` outside the window returns `undo_too_late` and does NOT mutate.
  - `handle_undo` with empty log returns `undo_nothing`.
  - Reversal to a now-deleted prior location detaches instead (guard from AC4.3).

### AC8 — E2E test (Playwright)

- Extend `tests/e2e/specs/journeys/shelving.spec.ts` (primary shelving spec) with an undo journey (use `loginAs`, `specIsbn`, unique V-/L-codes per the data-isolation rules):
  - **Shelve → undo:** scan a V-code + L-code to shelve a volume at a location → assert `feedback.volume_shelved` + an Undo button is present → click Undo → assert `feedback.undo_success_*` appears AND the volume is no longer at that location (verify via the location-contents view or the volume detail page).
  - **Activate-location → undo:** scan an L-code to activate a batch location → assert active-location banner → click Undo → assert the banner clears / reverts.
  - **(optional but recommended) window backstop:** assert the Undo button is gone after the client timeout — but do NOT use `waitForTimeout` (CI grep-gate blocks it). Use a DOM-state wait (`expect(locator).toBeHidden({ timeout: 35000 })`) so the assertion is event-driven, not a fixed sleep.
- Per Foundation Rule #13 §4: validate the spec with `CI=true npx playwright test --workers=2 specs/journeys/shelving.spec.ts` before push (shared-DB-state spec).

### AC9 — Documentation

- **User manual** (`docs/manual/{en,fr}/` — the scanning/cataloging chapter; verify which chapter covers the scan-to-shelve flow, likely ch. 5 or 6): add a short "Undo a scan action" subsection (30s window, which actions are reversible, that it undoes only the *last* action and never deletes a created title/volume). Rebuild PDFs and commit them (Rule 19 — release docs must be in sync). **These edits ride the same branch as the pending `docs/scanner-hardware-confirmed` commit** — bundle, don't re-branch.
- **CLAUDE.md "Key Patterns"** — add a paragraph: the **scan-undo session-log pattern** (`sessions.data["last_undoable_action"]`, server-authoritative 30s window via `SCAN_UNDO_WINDOW_SECS`, single-last-action semantics, `POST /catalog/undo` reversal + OOB refresh). Note the deliberate reuse of the `set_active_location` session-blob mechanism and that undo reverses *location state only*, never entity creation.
- **Release doc sync (Rule 19)** for `v1.10.0`: manual ch.1 version line + "What's new", README status/version, ROADMAP (move #9 planned→shipped, add the v1.10.0 section), `docs/dockerhub-overview.md` tags, `website/index.html` + `website/roadmap.html`, GitHub Release page, `sprint-status.yaml`. Grep `v1.9.1` before declaring done.

## Dev Notes (planning hints, NOT prescriptive)

- **Task 1 starts with reading** `handle_scan` (`catalog.rs:332`) end-to-end to see the exact shape of the three shelve branches (516 / 668 / 833) and the activate branch (827), how each builds its `HtmxResponse` + OOB (`#context-banner`, `#session-counter`, `#guide-strip`), and which role extractor guards the route. Do NOT write code before mapping how the OOB builders are structured — `handle_undo` must reuse them so the reverted state's counts/banner are consistent (DRY — Foundation Rule #1).
- **Prior-state capture ordering is the #1 correctness risk.** For each shelve site, read the volume's `location_id` BEFORE calling `update_location`. `existing_vol` at `:516` already holds it; at `:833` the volume is fetched by label inside `VolumeService::assign_location` — you may need to read it in the handler before delegating, or have the service return the prior location. Record the decision in Dev Agent Record.
- **Timestamp source:** `at` is a `NaiveDateTime`. Match how the codebase stamps "now" elsewhere (e.g. `chrono::Utc::now().naive_utc()` or a DB `NOW()`); the window comparison uses the same clock on both ends. Keep the comparison in a pure helper so tests don't depend on wall-clock.
- **`feedback_html_undoable` placement:** keep it in `src/utils.rs` next to `feedback_html` (DRY, single feedback-rendering home). Do NOT inline button HTML at call sites.
- **OOB reuse:** if `handle_scan`'s context-banner / session-counter builders are private helpers, lift them to `pub(crate)` (or a shared fn) so `handle_undo` calls the same code — avoid a second, drifting copy of the banner markup.
- **No migration.** If you find yourself writing SQL DDL, stop — the whole feature rides `sessions.data` + existing columns.
- **Templates audit:** the new undo button must pass `src/templates_audit.rs` (no inline `<script>`/`style`/`onclick`, no new `hx-confirm=`). The button is emitted from Rust (`feedback_html_undoable`), so also grep `src/` for the produced HTML string per the CSP manual-grep guidance in CLAUDE.md.

## Out of scope (deliberately deferred)

- **Undoing entity creation.** When a new volume/title was created-and-shelved (`catalog.rs:668`), undo detaches the volume from the shelf; it does NOT delete the created title/volume. Reversing creation is a larger, more destructive operation outside issue #9's "detach volume / cancel location assignment" scope. If a user wants it, file a follow-up `type:change-request`.
- **Multi-step / history undo.** Only the single most-recent action is undoable. No undo stack, no redo. (Issue #9 says "the last scan action".)
- **Undo surviving logout or a different browser.** The log is session-scoped in `sessions.data`; it survives a page reload (same session) but not a new session. Acceptable — the 30s window makes cross-session undo irrelevant.
- **Admin-configurable window.** `SCAN_UNDO_WINDOW_SECS` is a v1 freeze at 30s, matching the `RECENT_ACTIVITY_DAYS` precedent. The model methods take the window so a future extract-to-`AppSettings` is a focused diff.
- **Undo of loan register/return or reference-data edits.** Those are not scan actions and are out of scope; loan return already has its own modal flow.

## Test plan (run order)

1. `SQLX_OFFLINE=true cargo clippy --bin mybibli --tests -- -D warnings` — clean (zero-warnings policy).
2. `touch src/lib.rs && cargo build` — force i18n proc-macro to pick up new keys.
3. `cargo test --lib` — existing + new unit tests green.
4. `cargo test --test locale_parity` — 4-locale parity gate green.
5. DB-integration tests if any handler test is `#[sqlx::test]` (per CLAUDE.md docker-compose on port 3307).
6. `./scripts/e2e-reset.sh && cd tests/e2e && CI=true npx playwright test --workers=2 specs/journeys/shelving.spec.ts` — new undo journey green.
7. Full CI-shape Playwright (`CI=true npx playwright test --workers=2`) — no regressions in the scan/shelving specs.
8. Manual smoke: `/catalog`, shelve a volume → Undo appears → click → volume unshelved + counts update; activate a batch location → Undo → reverts; wait >30s → Undo button gone; click a stale Undo (via a second tab) → "Rien à annuler".

## Code-review checkpoint (Foundation Rule #6)

Before marking the implementation PR ready, invoke `/bmad-code-review` (3-layer adversarial: Blind Hunter, Edge Case Hunter, Acceptance Auditor). Story is clean only when a full pass surfaces 0 Medium+ findings; on any Medium+ finding, fix in-branch and re-run from scratch.

### Probes the reviewer must walk explicitly

- **Prior-state capture correctness (Blind/Edge).** For each of the 3 shelve sites, confirm the prior `location_id` is read BEFORE `update_location` fires, and that the recorded `volume_id` is the volume actually shelved (not a stale/adjacent one). Walk `:833` specifically — the volume is resolved by label inside the service; confirm the prior location isn't read AFTER the service already overwrote it.
- **30s window is server-authoritative (Edge/Acceptance).** Confirm the endpoint rejects an expired undo even if the client button is still present (simulate by POSTing `/catalog/undo` directly after backdating `at`). The client timer is UX-only.
- **Single-use / no re-undo (Edge).** After a successful undo, `last_undoable_action` is cleared and the reversal itself is NOT recorded as a new undoable action (no undo-the-undo loop). A second `/catalog/undo` returns `undo_nothing`.
- **Deleted-prior-location guard (Edge).** Re-attaching to a soft-deleted location must not violate FK/soft-delete invariants — confirm it detaches instead and messages clearly.
- **OOB consistency (Acceptance).** After undo, `#context-banner` and `#session-counter` reflect the reverted state (reused `handle_scan` builders, not a divergent copy). No stale count.
- **Scan flow not broken by undo-log failure (Blind).** If `set_last_undoable_action` errors, the scan still succeeds (best-effort record); confirm no path lets the undo-log write roll back the shelve.
- **CSP / audit (Acceptance).** `src/templates_audit.rs` passes; the undo button carries no inline JS and no new `hx-confirm=`. `forms_include_csrf_token` and the CSRF exempt-route freeze are untouched.
- **CSRF on the new POST (Blind).** `/catalog/undo` is a state-changing POST — confirm it is NOT added to the exempt allowlist and that `csrf.js` covers it.
- **i18n parity (Acceptance).** All new keys exist in de/en/fr/it; `locale_parity` green; `touch src/lib.rs` step documented.
- **E2E truth-gate (Acceptance).** The shelving spec was validated with `CI=true --workers=2`, not just default workers (Rule #13 §4). Confirm via Dev Agent Record.

### Severity triage

Same table as polish-1: Critical/High = in-branch blockers; Medium = blocker per Rule #6 unless action-irrelevant (split to a `type:code-review-finding` issue, documented in PR body); Low = file as deferred GH issue.

## Tasks/Subtasks

- [x] **Task 1 — Code map** of the 4 scan sites + OOB builders + role guard (done in spec; verified against source).
- [x] **Task 2 — Session undo-log store (AC1):** `src/services/scan_undo.rs` (`UndoableAction`, `UndoKind`, `SCAN_UNDO_WINDOW_SECS`, pure `undo_is_within_window`) + `SessionModel::{set,get,clear}_last_undoable_action`; 6 pure unit tests.
- [x] **Task 3 — Instrument the 4 call sites (AC2):** prior-state captured BEFORE each mutation at `catalog.rs` :516 / :668 / L-code-assign / L-code-activate; best-effort record.
- [x] **Task 4 — Undo affordance (AC3):** `feedback_html_undoable` in `src/utils.rs` (shared inner `feedback_html_action`); wired to the shelve + activate feedback entries only.
- [x] **Task 5 — `POST /catalog/undo` (AC4):** `src/routes/catalog_undo.rs::handle_undo` + route in `mod.rs`; window guard, reversal, deleted-prior guard, single-use clear, guide-strip OOB.
- [x] **Task 6 — Client JS (AC5):** `initScanUndo` in `static/js/mybibli.js` (disable-on-click + 30s auto-remove observer).
- [x] **Task 7 — i18n (AC6):** 6 keys × 4 locales (de/en/fr/it); `locale_parity` green.
- [x] **Task 8 — Unit + integration tests (AC7):** `tests/scan_undo_integration.rs` (6 `#[sqlx::test]` cases) — all green.
- [x] **Task 9 — E2E (AC8):** 2 undo journeys added to `tests/e2e/specs/journeys/shelving.spec.ts`. *(To be validated via `CI=true --workers=2` on the full stack before push — see Completion Notes.)*
- [x] **Task 10 — Docs (AC9):** manual subsection (en+fr) + PDFs rebuilt + CLAUDE.md pattern note. *(Full v1.10.0 release doc sync deferred to the release cut — see Completion Notes.)*

## Dev Agent Record

### Decisions

- **`:833` prior-location read path** — read the volume's prior `location_id` in the handler via `VolumeModel::find_by_label(&vol_label)` BEFORE calling `VolumeService::assign_location` (the service overwrites it). The returned `volume.id` from `assign_location` is authoritative for the recorded `volume_id`.
- **Timestamp clock source** — `chrono::Utc::now().naive_utc()` at each record site and in `handle_undo`; the window comparison is the pure `undo_is_within_window` helper so tests are wall-clock-independent.
- **OOB scope on undo** — refresh only `guide-strip`. Deliberately NOT `context-banner` / `session-counter`: undoing a location change alters neither the active title context nor any counter, so re-rendering them would be a pointless DB round-trip and risk a drifting second copy of that markup. Made `guide_strip_html` `pub(crate)` for reuse (DRY).
- **Handler placement** — `handle_undo` lives in a NEW `src/routes/catalog_undo.rs`, not `catalog.rs` (already 2800 lines, over Foundation-Rule-#12's 2000 budget). The 4 site instrumentations unavoidably edit `catalog.rs` but add only a handful of lines each.
- **Role guard** — `session.require_role(Role::Librarian, locale.0)?`, identical to `handle_scan`.
- **i18n placement** — new keys under the existing `feedback:` / `guide:` trees (there is no `action:` top-level group), so no new locale sub-tree.
- **Raw-string gotcha** — the undo button literal contains `="#feedback-list"`, whose `"#` closes an `r#"…"#` raw string early; used `r##"…"##`.

### Completion Notes

- Rust green locally: `cargo clippy --workspace --all-targets -D warnings` clean; `cargo test --lib` 1055 passed; `tests/scan_undo_integration.rs` 6/6; `locale_parity` 3/3; `templates_audit` 16/16 (CSP no-inline + hx-confirm allowlist still empty + CSRF form coverage); adjacent suites (home_scan_redirect, csrf_integration, role_gating, saved_searches_integration, connection_lost_overlay) green — no regression from the `feedback_html` refactor.
- **E2E validated on the full stack (Rule #13 §4).** Built the app image + fresh DB via `E2E_HOST_PORT=8091 ./scripts/e2e-reset.sh` (host 8080 was taken by an unrelated container). `CI=true --workers=2` shelving spec → 8/8 pass (6 existing + 2 new undo journeys). Full suite on a pristine DB → **294 passed**, 1 flaky (recovered), 1 pre-existing shared-state-flaky failure `title-detail-volumes.spec.ts:22` that PASSES in isolation and is unrelated to this change (doesn't touch feedback rendering).
- **v1.10.0 release doc sync deferred to the release cut** (Rule 19): README status/version, ROADMAP (move #9 planned→shipped + add v1.10.0 section), `docs/dockerhub-overview.md`, `website/{index,roadmap}.html`, GitHub Release page, `Cargo.toml` bump, manual ch.1 version line + "What's new". The manual **source** subsection + rebuilt PDFs are already on-branch; the version-string bumps happen when the tag is cut.
- **Bundling:** this branch (`polish-2/undo-recent-scan-actions`) also carries the pending `docs/scanner-hardware-confirmed` commit; docs + feature push together in one PR / one CI run (user decision).

## File List

- `src/services/scan_undo.rs` (new) — `UndoableAction`/`UndoKind`, window const + pure helper, 6 unit tests.
- `src/services/mod.rs` — register `scan_undo`.
- `src/models/session.rs` — `set/get/clear_last_undoable_action`.
- `src/utils.rs` — `feedback_html_undoable` + shared `feedback_html_action`.
- `src/routes/catalog.rs` — instrument 4 scan sites; `guide_strip_html` → `pub(crate)`; undoable feedback.
- `src/routes/catalog_undo.rs` (new) — `POST /catalog/undo` handler.
- `src/routes/mod.rs` — register module + route.
- `static/js/mybibli.js` — `initScanUndo` (+ wired into `init`).
- `locales/{de,en,fr,it}.yml` — 6 new keys each.
- `tests/scan_undo_integration.rs` (new) — 6 `#[sqlx::test]` cases.
- `tests/e2e/specs/journeys/shelving.spec.ts` — 2 undo journeys.
- `docs/manual/{en,fr}/03-usage.tex` + rebuilt `docs/manual/*.pdf`.
- `CLAUDE.md` — scan-undo pattern note.

## Change Log

- 2026-07-01 — Implemented undo for recent scan actions (#9). Server-authoritative 30s window via `sessions.data["last_undoable_action"]`, no migration. New `POST /catalog/undo`, undoable feedback affordance, 4-locale i18n, unit + DB-integration + E2E tests, manual + CLAUDE.md docs. Status → review.
