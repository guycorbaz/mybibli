# Story 11.15: Proposal history and resubmission delay

Status: done

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR135, FR136, AR30 (with FR134, AR29 inherited from 11-9). Epic 11 — fifteenth story. The three proposal kinds exist (series 11-12, metadata 11-13, cover 11-14); this story gives the agent its own history (`list_my_proposals`) and stops it from re-filing what a librarian rejected recently. Rejecting itself is story 11-16 (web page); here rejections are made through `ProposalService::transition` in tests.

## Story

As an AI agent, I want to know what was rejected and why; as an admin, I want to set how soon a rejected proposal may come back,
so that the librarian is not asked the same thing over and over.

## Existing-code reality check (code map, 2026-10-29, `main` 149ff6d)

| Place | Today | Consequence |
|---|---|---|
| `src/services/proposals.rs` (1390 lines) | `ProposalService::file_in(conn, proposal)` — validate → `lock_filing_key` (share lock, key row first) → target states → insert. Every kind files through it: series (`lock_filing_key_exclusive` + duplicate reads first, D-11-12-13), cover (`file_capped`, exclusive key lock + per-key cap, D-11-14-12), metadata (`ProposalService::file`, own transaction) | the delay check goes **inside `file_in`, right after the key lock** → one rule, one place, inside the filing transaction, key-first lock order kept for all three kinds. Rule #12: new code goes in a new module `services/proposal_history.rs` |
| `ProposalError` | `KeyNotAllowed` is the only non-input refusal the three kind error types map to `Conflict` (`Proposal(ProposalError::KeyNotAllowed{..}) => Conflict`, else `Internal`) | add `RecentlyRejected { … }` and `ProposalError::is_conflict()`; the three `class()` use it (otherwise the refusal would surface as an internal error) |
| `src/models/proposal.rs` | `proposals` has no equivalence information; `ProposalModel::list(filter{status, kind, api_key_id}, page)` exists (newest first, 25 per page) | migration adds `proposals.equivalence_key` (indexed); summary row gains `rejection_reason`; a light per-page item read gives each proposal's subject |
| Item shapes | series: line 1 `{"line":"series","name",…}` (create, no target) or `{"line":"series"}` → `series/<id>` (attach); metadata: one `{"line":"field","field",…}` per field → `titles/<id>`; cover: one `{"line":"cover",…}` → `titles/<id>` | the equivalence key is derived from these lines (kind + series id / folded name / title id + field set / title id) |
| `metadata_proposals::fold_name` | NFD, accents dropped, lower case, inner whitespace collapsed | reused for "normalised name" of a series creation |
| `settings` K/V (8-5) | `save_setting` (optimistic lock), panel `routes/admin_system.rs` (1773 lines), one form per section; e.g. Sessions (#418) | new seeded row `proposal_resubmission_delay_days = 90`; new "Proposals" section + form in a new file `routes/admin_system_proposals.rs` |
| DB sessions | `SET time_zone = '+00:00'` | dates computed from DB `NOW()` and shown as RFC 3339 UTC (`…Z`) |
| `src/mcp/tools/mod.rs`, `tests/common/mcp.rs`, `tests/e2e/helpers/mcp.ts` | 11 tools | 12 tools: `list_my_proposals` (read-only — it never files anything) |

## Acceptance Criteria

1. **AC1 — `list_my_proposals` (FR135).** A new read-only MCP tool (`readOnlyHint: true`, closed schema, every property described, description containing "Read-only"). Arguments: `status?` (`pending` / `accepted` / `rejected` / `stale`), `kind?` (`series_create` / `series_attach` / `metadata` / `cover`), `page?` (1-based, 25 per page, the shared `page` bounds). Returns only the **calling key's** live proposals, newest first, in the shared list-page shape (`items`, `page`, `page_size`, `total_items`, `total_pages`). Each item: `proposal_id`, `kind`, `status`, `filed_at`, `decided_at` (null while pending), `rejection_reason` (null unless rejected with a reason), `item_count`, `subject` (what it was about: `{"series_name"}` for a creation, `{"series_id"}` for an attach, `{"title_id", "fields"}` for metadata, `{"title_id"}` for a cover), and for a rejected proposal `resubmittable_from` (rejection date + current delay; null for other statuses). Dates are RFC 3339 UTC. Another key's proposals never appear. An unknown `status` / `kind` value is an argument error listing the valid ones.
2. **AC2 — the setting (AR30, 8-5 pattern).** Admin → System gains a "Proposals" section with one integer field "Delay before a rejected proposal can be resubmitted" (days), default 90, bounds 0..=3650 (0 = no delay), stored as the `settings` row `proposal_resubmission_delay_days` (seeded by migration), saved with the optimistic-lock `save_setting`, a validation error re-rendering the form with the typed value (400 + `HX-Trigger: validation-error`, #91 pattern), a stale version answering the usual conflict, success → OOB FeedbackEntry. Admin only (Librarian 403, Anonymous → login). A change applies to the very next filing (the filing reads the row in its transaction — no cache, no restart). An unreadable or out-of-range stored value falls back to 90 with a warning.
3. **AC3 — equivalence (FR136).** Two proposals are equivalent when they share the kind and the target: `series_create` — the proposed series name, normalised (`fold_name`: case, accents and inner whitespace ignored); `series_attach` — the series id; `metadata` — the title id **and** the same set of fields (the items' `field` keys; `{description}` and `{description, subtitle}` are not equivalent); `cover` — the title id. The key is computed at filing from the items as filed and stored on the proposal (`equivalence_key`, indexed); later edits of items (11-19) do not change it. Equivalence is library-wide: a rejection made on a proposal of another key blocks too (a new key must not reset the delay).
4. **AC4 — the delay at filing (FR136).** Inside the filing transaction, after the filing key is locked (11-12 lock order), a proposal equivalent to a live proposal **rejected** less than the delay ago is refused (`Refused`, nothing filed, a staged cover discarded as for any filing refusal). The message gives the rejection date and the date-time from which an equivalent proposal will be accepted again (rejection date + delay, RFC 3339 UTC); when the rejected proposal is the same key's, it also names it and its reason. Accepted, stale, pending, or soft-deleted proposals never block; a delay of 0 disables the rule; at or after the reopening instant the proposal is filed.
5. **AC5 — labels.** Every new web label and message exists in `locales/{en,fr,de,it}.yml` (`cargo test --test locale_parity`); MCP copy stays English (D-11-9-2). Error copy follows `docs/error-message-style.md` (field hint terse).
6. **AC6 — tests.** Unit: equivalence key per kind (folding, field-set order independence, malformed shapes → no key), the delay predicate with a simulated clock (inside / exactly at / after the reopening instant, delay 0), setting parsing and validation, tool argument parsing and document shape. DB integration (`#[sqlx::test]`): history (own key only, status / kind filter, decision date, reason, subject, `resubmittable_from`), the delay for each kind (refused inside — clock simulated by back-dating `decided_at` —, filed after, other field set filed, stale / accepted not blocking, another key's rejection blocking, delay 0, the setting change applied on the next filing), the admin form (save, validation, conflict, role guard) over the real router. E2E: an admin sets the delay in Admin → System from a blank browser; an agent lists its own proposals through MCP (filter by status, another key's proposal absent).

## Tasks / Subtasks

- [x] Migration `20261029120000_proposals_equivalence_key.sql`: `proposals.equivalence_key VARCHAR(700) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NULL` + `INDEX idx_proposals_equivalence (equivalence_key, status, decided_at)` (composite since review round 1); seed `proposal_resubmission_delay_days = '90'` (AC2, AC3)
- [x] `services/proposal_history.rs`: `ProposalSubject` (+ `from_items`, `equivalence_key`, JSON), `resubmission_open_from`, delay constants/parse, `list_mine` (AC1, AC3, AC4)
- [x] `ProposalError::RecentlyRejected` + `is_conflict()`; check in `ProposalService::file_in`; three `class()` updated (AC4)
- [x] Model: `insert_proposal` stores the key; `latest_rejection(conn, key)`; `resubmission_delay_days(conn)`; summary `rejection_reason`; `subject_lines(ids)` (AC1, AC4)
- [x] MCP tool `list_my_proposals`; registry + `tests/common/mcp.rs` + `tests/e2e/helpers/mcp.ts` (AC1)
- [x] Admin → System "Proposals" form + route `POST /admin/system/proposals`; validator in `services/admin_system.rs`; locale keys ×4 (AC2, AC5)
- [x] Tests: unit, `tests/proposal_history.rs` (DB + router), admin form tests, E2E spec (AC6)
- [x] `cargo sqlx prepare` if typed queries change; decisions D-11-15-* logged

## Dev Notes

- **Where the rule lives.** `file_in` is the single funnel of all three kinds, and it runs after each kind's own key lock (series and cover take the exclusive lock before calling it; metadata gets the share lock inside it), so the delay read — a plain read on the indexed `equivalence_key` — follows the key lock in every path. No new lock.
- **Clock.** `latest_rejection` returns the newest live rejected proposal's `decided_at` **and** the DB `NOW()`; the pure `resubmission_open_from(rejected_at, delay_days)` and the comparison `now < open_from` decide. Unit tests pass simulated instants; DB tests back-date `decided_at` (`UPDATE proposals SET decided_at = NOW() - INTERVAL 91 DAY`).
- **Key format** (`utf8mb4_bin`, built in Rust): `series_create:<fold_name(name)>`, `series_attach:series/<id>`, `metadata:titles/<id>:<sorted, deduplicated field keys joined by ','>`, `cover:titles/<id>`; at most 700 characters (a folded name is cut — two names identical over 680 folded characters are treated as equivalent, harmless). A shape that does not match (service tests filing arbitrary items) yields no key and no check.
- **History subject** is derived from the live items (light JSON-path read of `line`, `field`, `name`, target for the page's ids) through the same `ProposalSubject::from_items` as the key (Rule 1).
- **Not here:** rejecting from the web (11-16); a librarian-facing history; backfilling keys (proposal tables are unreleased — rows filed before the migration keep a NULL key and never block).
- **Regression watch (validation pass):** `tests/mcp_series_proposals.rs::…` (the "once decided, a new proposal can be filed" block, ~l. 523) rejects an attach proposal and re-files the same attach at once — that is now refused by design; adapt it (expect the refusal, then back-date the rejection and re-file). The stale / accepted / soft-deleted parts of that test must keep passing unchanged (they do not block). The cover purge test files five different images for one title before deciding any — unaffected. Admin → System panel tests that enumerate sections may need the new one.
- The refusal is a `Conflict`-class error in all three kinds: without `is_conflict()` the kinds' `Proposal(_) => Internal` arm would hide it as a server error (an agent would retry forever).
- Pitfalls: CI charset → `CAST(… AS CHAR)` on every JSON / string expression in dynamic queries; no repo-wide `cargo fmt`; French E2E copy; `mem-guard` on heavy commands.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.15] — FR135, FR136, AR30.
- [Source: _bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md] — D-11-9-7 (reason rules), D-11-12-5/-11/-13 (series duplicates, key-first lock order), D-11-13-2/-5 (metadata items, no pending rule), D-11-14-5/-6/-12 (cover item, `file_capped`).
- [Source: src/services/series_proposals.rs, metadata_proposals.rs, cover_proposals.rs module docs] — item shapes.

## Review amendments

- **AC1** (D-11-15-8): `resubmittable_from` is the newest live rejection of an equivalent proposal (any key) plus the current delay — what the filing check enforces — and null for a proposal without an equivalence key.
- **AC3** (D-11-15-8): the series-creation name is folded with NFKC first (fullwidth letters, ligatures), then `fold_name`.
- **AC4** (D-11-15-8): the check runs after the item target checks, still after the key lock and before the insert.
- Code review: round 1 (3 layers) — 2 Medium fixed, Lows fixed or tracked (#535, #536); round 2 — no Medium+, Lows fixed (D-11-15-9).

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5

### Debug Log References

### Completion Notes List

- The delay is checked in `ProposalService::file_in` right after `lock_filing_key` (series and cover already hold the exclusive key lock before calling it): one rule for the three kinds, no new lock. `check_resubmission` returns the key, stored by `insert_proposal`.
- No `AppSettings` field: the filing (and the history) read the `settings` row in their own transaction through `parse_resubmission_delay_days` (D-11-15-4). `fetch_setting_rows` (Admin → System) has a key whitelist — the new key had to be added, otherwise the panel always rendered version 1 (caught by the router test).
- Two existing tests re-filed right after a rejection and now meet the delay by design: `mcp_series_proposals::a_pending_proposal_for_the_same_series_blocks_a_new_one` (now expects the refusal, then back-dates) and `mcp_cover_proposals::a_key_with_too_many_pending_covers_is_refused` ("deciding one" now marks it stale).
- `docs/route-role-matrix.md`: the new route and the history / equivalence rule.
- Gates: clippy `-D warnings` clean; `cargo test` 1959 passed / 0 failed; `cargo sqlx prepare --check` clean (no typed query added); waitForTimeout grep clean; `tsc --noEmit` clean; Playwright `CI=true --workers=2` on a reset stack: 372 passed, 2 skipped.

### File List

- migrations/20261029120000_proposals_equivalence_key.sql (new)
- src/services/proposal_history.rs (new)
- src/mcp/tools/list_my_proposals.rs (new)
- src/routes/admin_system_proposals.rs (new)
- templates/fragments/admin_system_proposals_form.html (new)
- tests/proposal_history.rs (new)
- tests/e2e/specs/journeys/mcp-proposal-history.spec.ts (new)
- src/services/proposals.rs, src/models/proposal.rs, src/services/mod.rs
- src/services/series_proposals.rs, src/services/metadata_proposals.rs, src/services/cover_proposals.rs
- src/services/admin_system.rs, src/routes/admin_system.rs, src/routes/mod.rs, templates/fragments/admin_system_panel.html
- src/mcp/tools/mod.rs, tests/common/mcp.rs, tests/e2e/helpers/mcp.ts
- tests/mcp_series_proposals.rs, tests/mcp_cover_proposals.rs
- locales/{en,fr,de,it}.yml, docs/route-role-matrix.md
