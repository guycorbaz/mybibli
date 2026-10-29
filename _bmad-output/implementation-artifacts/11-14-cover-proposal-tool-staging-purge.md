# Story 11.14: Cover proposal tool and staging purge

Status: review

GitHub issue: part of #493 (MCP server — do NOT close it here). Closes #522 (cover staging directory outside the Docker covers volume) and #514 (MCP body cap and request timeout). Covers: FR133, NFR45, NFR48 (with FR134, NFR44, NFR47, AR27, AR29 inherited from 11-5 / 11-6 / 11-9). Epic 11 — fourteenth story; the third and last kind of proposal an agent files. The agent never writes the catalog: each call files one `cover` proposal through `ProposalService::file` (story 11-9) and leaves one re-encoded image in the staging area. Rejection is story 11-16, acceptance 11-18; both call the *discard* / *promote* this story provides.

## Story

As an AI agent,
I want to submit a cover I found,
so that the librarian can approve it without downloading it.

## Existing-code reality check (code map, 2026-10-29, `main` 68c794d)

| Place | Today | Consequence |
|---|---|---|
| `src/services/cover_staging.rs` (11-9) | `staging_dir_for(covers_dir)` → sibling `<covers>-staging` (`/app/covers-staging` in Docker); `AppState::cover_staging_dir()` (`None` when missing); created at boot by `main.rs`, fail-open | **#522**: the shipped `docker-compose.yml` and `docs/dockerhub-overview.md` mount only `/app/covers` → staged covers lost at every container re-creation and promotion by `rename` crosses filesystems (`EXDEV`). Move the staging area **inside** the covers directory (`<COVERS_DIR>/.staging`), which is on the covers volume of every existing install, and stop `/covers` from serving it |
| `src/routes/mod.rs:728` | `.nest_service("/covers", ServeDir::new(&state.covers_dir))` — `ServeDir` serves dot-directories | wrap it so any path segment starting with `.` (raw or `%2e`) answers 404 |
| `src/services/cover.rs` | `CoverService::process_and_save_bytes(bytes, title_id, covers_dir)`: empty / `> MAX_COVER_SIZE` (10 MiB) refused, magic-byte format detection, decode under the #479 limits (`decode_limits`: 64 MiB alloc, 10 000 px per side), resize to ≤ 400 px wide, JPEG 80, written to `covers/{id}.jpg` | split into a pure `encode_cover(bytes) -> EncodedCover` (the checks + re-encode) used by the upload path and by the proposal (Rule 1); the staged file is the re-encoded JPEG, so promotion is a move and the review page shows exactly what acceptance installs |
| `src/routes/titles.rs::upload_cover` | manual upload; axum's default 2 MiB body limit on `Multipart` is the effective bound | proposed images capped at 2 MiB decoded (`MAX_PROPOSED_COVER_BYTES`), aligned on the manual upload (#514) |
| `src/mcp/config.rs` | `max_request_body_bytes()` = base64 of `MAX_COVER_SIZE` + 1 MiB ≈ 14.3 MiB (D-11-6-5); no request timeout | derive from `MAX_PROPOSED_COVER_BYTES` (≈ 3.7 MiB) and add a 30 s `TimeoutLayer` on `/api/mcp` (#514) |
| `src/models/api_key.rs::revoke` + `routes/admin_api_keys.rs::admin_api_keys_revoke` | soft-revoke by pool, optimistic lock on `version` | revocation becomes one transaction: key row first (lock order documented on `lock_filing_key`), then the key's pending proposals → `stale`; staged covers deleted after commit |
| `src/tasks/auto_purge_scheduler.rs` (8-7) | `spawn(pool, settings)` runs `AutoPurgeService::run_purge` on the configured cadence | also runs the staging purge (orphaned staged files) on each tick |
| `proposals.kind` ENUM | already has `cover`; `ProposalKind::Cover` exists | no migration |
| `src/services/proposals.rs` | `ProposalService::file`, `Refusal`, `RefusalClass`, `FiledProposal`; `NewProposalItem.expected_version` | the cover service builds one item and calls `file` (no duplicate rule needs 11-12's exclusive key lock) |
| `src/mcp/tools/mod.rs`, `tests/common/mcp.rs`, `tests/e2e/helpers/mcp.ts` | 10 tools, `PROPOSE_TOOLS` (3) | 11 tools; `propose_cover` added in the three places |

## Acceptance Criteria

1. **AC1 — one tool, `propose` key only (FR133, NFR44).** `tools/list` adds `propose_cover` (11 tools). Description precise enough to use without external docs (what is filed, that the catalog does not change until a librarian accepts, every refusal and bound, that the server never downloads anything); input schema closed (`additionalProperties: false`) with a description on every property; annotations `readOnlyHint: false`, `destructiveHint: false`, `idempotentHint: false`. A `read` key is still refused by `/api/mcp`.
2. **AC2 — arguments.** `title_id` (required), `version?` (the title's `version` from `get_title`; mismatch refused), `image_base64` (required; standard base64, padding optional, ASCII whitespace ignored; a `data:` URL prefix is refused with a hint), `media_type` (required; one of `image/jpeg`, `image/png`, `image/webp`, `image/gif`), `sources` (required, http(s) URLs by 11-9's rules — the page or image URL where the cover was found).
3. **AC3 — the same checks as a manual upload, before anything is written (NFR45).** In this order: base64 decodes; decoded size 1..=`MAX_PROPOSED_COVER_BYTES` (2 MiB, the manual upload's effective bound); the format detected from the magic bytes is one of the four and equals `media_type`; the image decodes under the #479 limits (64 MiB allocation, 10 000 px per side) and is re-encoded by the shared pipeline (≤ 400 px wide, JPEG 80). Any failure → `Invalid arguments: …` refusal naming the problem; **nothing is written to the staging directory** and no proposal is filed.
4. **AC4 — replacement.** A title that already has a cover → filed, item flagged `"replaces_existing": true` with the current cover URL; the result echoes `replaces_existing`.
5. **AC5 — what is filed (FR134, AR29).** One proposal per call, kind `cover`, attributed to the calling key, `pending`, sources by 11-9's rules, **one item** targeting `titles/<id>` recording its `version` (pinned or read; a title edited in between → `TargetChanged`). Item shape (contract for 11-15, 11-16, 11-18, 11-20; module doc + decision): `{"line": "cover", "staged_file": "<32 hex>.jpg", "replaces_existing": bool, "current": "<cover url>"|null, "media_type": "<declared>", "original_bytes": n, "width": w, "height": h, "bytes": n}` (`width`/`height`/`bytes` describe the staged JPEG).
6. **AC6 — staging placement (#522).** The staging area is `<COVERS_DIR>/.staging` (Docker: `/app/covers/.staging`, on the existing covers volume — no compose change, no manual step). `/covers/.staging/…` (and any dot-segment, `%2e` included) answers 404. Created at boot (fail-open; when missing, `propose_cover` is refused as unavailable and nothing is filed). Staged file names are generated (`<32 lowercase hex>.jpg`) and validated before any path is built from an item.
7. **AC7 — promote and discard (NFR48).** `CoverStaging::promote(staging, covers, staged_file, title_id)` moves the staged JPEG to `covers/{title_id}.jpg` (rename; copy + fsync + rename fallback across filesystems) and returns `/covers/{title_id}.jpg`; a missing staged file is a typed error (`Missing`), not a panic. `discard(staging, staged_file)` deletes it; already gone = success. Called by 11-16 (reject) and 11-18 (accept) — not wired to a route here.
8. **AC8 — revocation (NFR48).** Revoking an API key (Admin → API keys) marks its pending proposals `stale` (`decided_at` set, no decider, 11-9's rule) in the revocation transaction, then deletes the staged covers of those proposals. A failed file delete is logged and left to the scheduled purge.
9. **AC9 — scheduled purge (NFR48, 8-7 pattern).** Each auto-purge tick also purges the staging area: a staged file is kept while a live `pending` cover proposal references it, or while an `accepted` one decided less than an hour ago does (promotion runs after the accept commit); a file referenced only by rejected / stale / soft-deleted / older accepted proposals is deleted; an unreferenced file is deleted once older than an hour (an in-flight filing writes before it commits). Non-matching names are left alone. Counts are logged.
10. **AC10 — `/api/mcp` bounds (#514, NFR45).** Body cap derived from `MAX_PROPOSED_COVER_BYTES` (base64 + 1 MiB envelope ≈ 3.7 MiB; 413 above); 30 s request timeout on the route (408).
11. **AC11 — no server-side fetch (SSRF policy).** The server never downloads an agent-supplied URL: the image arrives as bytes, `sources` are stored as provenance only (validated, never dereferenced). Documented in `docs/auth-threat-model.md` with the rule for any future fetching tool.
12. **AC12 — layering (AR27).** Thin tool `src/mcp/tools/propose_cover.rs` over `services::cover_proposals`; no SQL under `src/mcp/`; English agent-facing messages; refusals through the shared `Refusal` / `tool_error`.
13. **AC13 — tests.** Unit: argument parsing, schema, base64 / size / type / mismatch rules, name validation, promote / discard / purge on temp dirs, the `/covers` dot-segment guard, error mapping, registry (11 tools). Integration `tests/mcp_cover_proposals.rs` over the real router with a temp covers dir: a valid image (item, staged file, re-encoded size), a refused image (garbage, oversized, wrong type, mismatch — nothing staged, nothing filed), a replacement, key revocation (stale + staged file gone), orphan purge (old unreferenced and decided files gone, pending and fresh kept), promote / discard, `/covers/.staging/…` 404. E2E `mcp-cover-proposal.spec.ts` (spec id `PC`): from a blank browser, admin scans a title and creates a `propose` key; the agent lists 11 tools, files a cover proposal, gets a refusal for a non-image; the title page still shows no cover; the staged file is not reachable under `/covers/.staging/`; key revoked.

## Tasks / Subtasks

- [x] T1 — `services::cover`: `encode_cover` (pure, shared), `MAX_PROPOSED_COVER_BYTES`, `ProposedImageFormat` detection; `process_and_save_bytes` reuses it.
- [x] T2 — `services::cover_staging`: `.staging` inside covers dir; name generation / validation; `stage`, `promote`, `discard`, `purge` (pure listing logic + DB reads through `ProposalModel`); `/covers` dot-segment guard; `main.rs` boot creation.
- [x] T3 — `services::cover_proposals`: request, `CoverProposalError` (`Refusal`), `propose` (validate → title → encode in `spawn_blocking` → stage → file → discard on failure).
- [x] T4 — Revocation: `ApiKeyModel::revoke_in`, `ProposalModel` stale-by-key + staged names, service `revoke_api_key`; admin handler uses it.
- [x] T5 — Scheduler: staging purge per tick.
- [x] T6 — MCP: `propose_cover` tool, registry, body cap from the new bound, `TimeoutLayer`.
- [x] T7 — Tests (unit, `tests/mcp_cover_proposals.rs`, tool counts in helpers), E2E `mcp-cover-proposal.spec.ts`, `MCP_TOOLS`.
- [x] T8 — Docs: `docs/auth-threat-model.md` (SSRF policy, staging not served), `docs/route-role-matrix.md` (tool, body cap, timeout), `docs/dockerhub-overview.md` / `docker-compose.yml` comment (covers volume also holds pending agent covers); decisions D-11-14-*.

## Dev Notes

- **Order inside `propose`** (nothing written on refusal; as implemented, D-11-14-4 / D-11-14-11): arguments → image checks (type, base64, size, signature) → sources → title (live) + version pin → staging available? → pending cap → [own task] re-encode (`spawn_blocking`, at most two at once) → write staged file (temp name + rename, so a half-written file never carries a valid name) → `ProposalService::file` → on any filing error delete the staged file. A crash or timeout between write and commit leaves an unreferenced file, removed by the purge after its hour of grace.
- **Why re-encode before staging:** the staged file is what acceptance installs (promotion = move), and the librarian reviews the image the catalog would show; a polyglot or oversized original never reaches disk.
- **`media_type` mismatch is refused** (stricter than the manual upload, which ignores the claimed type): an agent sending a PNG as `image/jpeg` has likely mixed up two downloads.
- **No pending-duplicate rule** (D-11-13-5 precedent): a second cover proposal for the same title is filed; the librarian picks one, the other goes stale on acceptance (the cover write bumps `titles.version` in 11-18). 11-15's resubmission delay covers repeats of rejected ones. Disk use per proposal is one ≤ 400 px JPEG.
- **Staleness (#521):** the background cover fetch does not bump `titles.version`; the item's `current` records the cover URL at filing so 11-18 can compare it with the live value.
- **Lock order on revocation:** `UPDATE api_keys` (key row) first, then `SELECT … FOR UPDATE` of the key's pending proposals, then the update to `stale` — the order `lock_filing_key` documents; an in-flight filing commits first and its proposal is then marked stale.
- **Admin copy unchanged:** the revoke feedback keeps its wording (no new locale key); the stale marks are visible on the proposals page (11-16). The E2E checks `/covers/.staging/` is not served; the router test checks a real staged file answers 404.
- **The agent never learns `staged_file`:** the result is `{proposal_id, status, kind, item_count, replaces_existing}`; the staging layout stays internal.
- **Legacy sibling directory:** 11-9 created an empty `<covers>-staging` at boot on `main` (never released); it is left as is (nothing writes to it).
- Heavy commands under `mem-guard`, cargo `-j 2`; `rustfmt --edition 2024` on touched leaf files only.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.14] — FR133, NFR45, NFR48; consumers 11.15, 11.16, 11.18, 11.20.
- [Source: src/services/metadata_proposals.rs], [Source: src/mcp/tools/propose_title_metadata.rs] — structure to mirror.
- [Source: src/services/cover.rs] — #479 decode limits, upload pipeline.
- [Source: epic-11-autonomous-decisions.md] — D-11-6-5 (body cap), D-11-9-4 (staging placement), D-11-9-10/11 (lock order), D-11-13-8 (shared pieces).
- Issues #522, #514.

### Review Findings

Code review 2026-10-29 (bmad-code-review: Blind Hunter, Edge Case Hunter, Acceptance Auditor).

Round 1:
- [x] [Review][Patch] (Medium) Staging purge read `keep` before `referenced`: a filing committing between the two reads had its file deleted — `referenced` read first [src/services/cover_staging.rs]
- [x] [Review][Patch] (Medium) `/covers` guard checked raw segments; `ServeDir` decodes `%2F` — the path is percent-decoded and split on `/` and `\` before the check; router test with an encoded slash
- [x] [Review][Patch] (Medium) Unbounded concurrent decodes (a timed-out `spawn_blocking` keeps running) — semaphore of two permits held by the blocking task
- [x] [Review][Patch] (Medium) No bound on staged files per key — at most 100 pending cover proposals per key (D-11-14-11)
- [x] [Review][Patch] (Medium) `ApiKeyModel::revoke` (pool) stayed public and bypassed NFR48 — test-only now
- [x] [Review][Patch] (Medium) A timeout after the commit answered 408 for a filed proposal, a timeout before it could orphan a staged file — the encode / stage / file part runs in its own task to completion; the description warns a timed-out call may have filed
- [x] [Review][Defer] (Medium) #522's "refuse acceptance cleanly when the staged file is missing" belongs to 11-18 — #533
- [x] [Review][Patch] (Low) `promote` called a missing covers directory `Missing`; a failed staged-copy removal after a cross-device promotion failed the promotion; `.tmp-` files of that fallback were never purged; the schema `maxLength` contradicted "line breaks ignored"; description figures untested; dev-note order stale; timeout constant untested
- Accepted (Low): a commit reported as failed although it reached the server discards the staged file (rare; acceptance then hits `Missing`, #533); `staged_cover_files_referenced` scans every cover item (small table); `StagingUnavailable` stays a refusal (its message must reach the agent); sync `is_dir` in `cover_staging_dir` (one stat); the empty 11-9 sibling directory is left; E2E checks the title through `get_title` rather than the page's cover (the mock may supply one).

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test` (unit + DB integration on the 3307 test DB): 1935 passed, 0 failed.
- `cargo sqlx prepare --check --workspace -- --all-targets`: up to date (no macro query added).
- `tsc --noEmit`: clean; `waitForTimeout` grep: clean.
- Playwright on a reset stack (`E2E_HOST_PORT=18080`, `CI=true --workers=2`): 370 passed, 2 skipped.

### Completion Notes List

- New service `services::cover_proposals` (item-shape contract in its module doc, D-11-14-5): media type + base64 + size + signature checks, the shared `cover::encode_cover` (extracted from `process_and_save_bytes`) in `spawn_blocking`, staging, filing; typed `CoverProposalError` implementing `Refusal`.
- `services::cover_staging` rewritten: `<COVERS_DIR>/.staging` (#522), random names, `stage` / `promote` / `discard` / `purge`; `/covers` refuses dot-segments.
- Revocation: `ProposalService::revoke_key` (key row → pending proposals stale, one transaction) + `cover_proposals::revoke_key` (discard after commit); admin handler uses it.
- Auto-purge scheduler runs the staging purge on each tick.
- MCP: `propose_cover` (11 tools); body cap ≈ 3.7 MiB and 30 s timeout (#514).
- Docs: threat model § 5.9 (no server-side fetch), route-role matrix, compose / Docker Hub comment. Decisions D-11-14-1..10.

### File List

- `src/services/cover_proposals.rs` (new), `src/services/cover_staging.rs`, `src/services/cover.rs`, `src/services/proposals.rs`, `src/services/mod.rs`
- `src/models/proposal.rs`, `src/models/api_key.rs`
- `src/routes/admin_api_keys.rs`, `src/routes/mod.rs`, `src/lib.rs`, `src/main.rs`, `src/tasks/auto_purge_scheduler.rs`
- `src/mcp/tools/propose_cover.rs` (new), `src/mcp/tools/mod.rs`, `src/mcp/config.rs`, `src/mcp/mod.rs`
- `tests/mcp_cover_proposals.rs` (new), `tests/common/mcp.rs`
- `tests/e2e/specs/journeys/mcp-cover-proposal.spec.ts` (new), `tests/e2e/helpers/mcp.ts`
- `docs/auth-threat-model.md`, `docs/route-role-matrix.md`, `docs/dockerhub-overview.md`, `docker-compose.yml`
- `_bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md`, `sprint-status.yaml`

## Change Log

- 2026-10-29 — story created (ready-for-dev).
- 2026-10-29 — validated against the create-story checklist: result shape and the admin-copy / E2E scope made explicit (dev notes); no AC change.
- 2026-10-29 — implemented (dev-story); status review.
- 2026-10-29 — code review round 1 (D-11-14-11); fixes applied.
