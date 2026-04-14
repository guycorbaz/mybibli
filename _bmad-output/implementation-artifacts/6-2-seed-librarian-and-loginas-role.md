# Story 6.2: Seeded librarian user + `loginAs(page, role?)`

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a test author,
I want a seeded librarian-role user and a role-aware `loginAs()` helper,
so that I can write multi-role E2E tests before Epic 7 (Accès multi-rôle & Sécurité) starts.

## Scope at a glance (read this first)

**Test infrastructure + one migration. No FR/NFR mapping.**

The app currently seeds exactly one user via `migrations/20260329000002_seed_dev_user.sql` (inserts `dev_librarian`) which is then overwritten to `admin/admin/role=admin` by `migrations/20260331000004_fix_dev_user_hash.sql`. All 133 E2E tests log in as `admin` via `tests/e2e/helpers/auth.ts::loginAs(page)`. Epic 7 will introduce role-aware flows (FR65-FR67, AR13) — this story unblocks that work by:

1. **Seeding a second user** `librarian/librarian` with `role='librarian'` in a new migration (NOT by editing the existing hash-fix migration — migrations are append-only; rewriting history would break any deployed dev DB).
2. **Extending `loginAs(page)` to `loginAs(page, role?)`** without breaking any of the 133 call sites that already pass a single `page` argument.
3. **Migrating one existing smoke test** to prove the librarian path works end-to-end.

**Explicitly NOT in scope:**
- Role-based route guards, access-denied UI, or role-conditional rendering (Epic 7 work).
- Removing the `DEV_SESSION_COOKIE` injection pattern from non-smoke tests (that cookie is bound to the admin seed and continues to work — no migration needed).
- Adding a `role` column to the `sessions` table (role is resolved from `users.role` via the existing session lookup — see `src/middleware/auth.rs`).
- Password policy, Argon2 tuning, or rotating the admin password.
- Third or Nth role seed — only `admin` (exists) and `librarian` (added here).

## Acceptance Criteria

1. **Fresh-DB seed contains both users:** Given a fresh MariaDB bootstrapped by `sqlx::migrate!("./migrations")` (auto-run at `src/main.rs` startup), when the app finishes migrating, then `SELECT username, role FROM users WHERE deleted_at IS NULL ORDER BY username` returns exactly two rows: `('admin', 'admin')` and `('librarian', 'librarian')`. Both users have `active = TRUE`.
2. **Known passwords:** Given the seeded users, when a login is attempted with `admin/admin` or `librarian/librarian`, then both logins succeed and the resulting session reflects the correct role (`Role::Admin` for admin, `Role::Librarian` for librarian) per `src/middleware/auth.rs::Role::from_db`.
3. **Migration is append-only and idempotent:** Given the repo migration set, when a new migration file (timestamp > `20260412000001_widen_dewey_code.sql`) is added, then (a) existing migrations are NOT edited, (b) the new migration uses `INSERT ... WHERE NOT EXISTS` (or equivalent) so re-running it against a DB that already has the librarian row is a no-op, (c) `cargo sqlx prepare --check --workspace -- --all-targets` remains green.
4. **`loginAs(page)` backward compatible:** Given the 133 existing E2E tests that call `loginAs(page)` with a single argument, when they run unchanged against the new helper signature, then they all pass (admin login as before). The default role, when no second argument is supplied, is `"admin"`.
5. **`loginAs(page, "admin")` and `loginAs(page, "librarian")` both work:** Given a test calls `loginAs(page, "admin")` or `loginAs(page, "librarian")`, when the helper runs, then it navigates to `/login`, fills `#username` + `#password` with the matching seed credentials (overridable via `TEST_ADMIN_PASSWORD` / `TEST_LIBRARIAN_PASSWORD` env vars), submits, and waits for a URL that is not `/login`. The resulting session cookie (`session`) resolves server-side to the requested role.
6. **Type-safe role argument:** The helper accepts a union type `"admin" | "librarian"` (TypeScript) rather than a free-form string, so a typo at a call site fails at compile time (`tsc` / the Playwright type-check in `tests/e2e/`).
7. **One smoke test migrated as proof:** Given one existing smoke test (pick `tests/e2e/specs/journeys/epic2-smoke.spec.ts` — already uses `loginAs()` for all 4 of its calls and covers navigation + catalog/shelf access that a librarian legitimately should see), when at least one of its tests is switched to `loginAs(page, "librarian")`, then it passes locally and in CI. The migration demonstrates the end-to-end pattern; it does NOT need to exercise any admin-only path.
8. **Role-unit regression test:** A Rust unit test in `src/middleware/auth.rs` (or a new `tests/` file if preferred) asserts that after the new migration runs, `Role::from_db("librarian") == Role::Librarian` AND a DB fetch of the seeded `librarian` user yields `role = "librarian"`. (Idea: extend the existing `#[sqlx::test]` integration-test crate in `tests/` with a tiny fixture verifying both rows exist after migrations run.)
9. **Full green suite:** Given the story is complete, when `cargo test`, `cargo clippy -- -D warnings`, `cargo sqlx prepare --check --workspace -- --all-targets`, and `cd tests/e2e && npm test` all run on parallel mode (`fullyParallel: true`, default workers), then every gate is green: all unit + DB-integration tests pass and all 133+ E2E tests pass. Foundation Rule #5 — no story merge until green.
10. **Docs updated:** CLAUDE.md's E2E Test Patterns section mentions the new `loginAs(page, role?)` signature and the default-to-admin behavior. The `tests/e2e/helpers/auth.ts` JSDoc reflects the new signature, including the env-var override names.

## Tasks / Subtasks

- [x] **Task 1 — Add `librarian` seed migration** (AC: #1, #2, #3)
  - [x] 1.1 Create `migrations/20260414000001_seed_librarian_user.sql` (today's date, sequence 000001, matches the existing YYYYMMDDNNNNNN naming convention used across the migrations/ folder). Content: `INSERT INTO users (username, password_hash, role, active) SELECT 'librarian', '<argon2id hash of "librarian">', 'librarian', TRUE FROM DUAL WHERE NOT EXISTS (SELECT 1 FROM users WHERE username = 'librarian' AND deleted_at IS NULL);`
  - [x] 1.2 Generate the Argon2id hash for password `librarian` using the **same parameters as the existing admin hash** (`m=19456,t=2,p=1`, matching `migrations/20260331000004_fix_dev_user_hash.sql:5`). Use `cargo run --bin <helper>` if a hashing bin exists, or a one-off `rust-argon2` snippet / `argon2` CLI (`argon2 somesalt -id -m 19456 -t 2 -p 1 -e`). Document the generation method in the migration's comment header so a future maintainer can regenerate.
  - [x] 1.3 Run `cargo sqlx prepare --workspace -- --all-targets` and commit updated `.sqlx/` if any typed query touches `users`. Verify with `cargo sqlx prepare --check --workspace -- --all-targets`.
  - [x] 1.4 Verify idempotency: drop DB → re-migrate → re-run migration manually → `SELECT COUNT(*) FROM users WHERE username = 'librarian'` still returns 1.
  - [x] 1.5 **Verify the hash actually validates against the app's verify path.** The app uses `argon2` crate's `PasswordHash::new(hash)` + `Argon2::default().verify_password(...)` in `src/routes/auth.rs:187-194`. A hash generated with a slightly-off variant (argon2i vs argon2id) will silently fail at login. Add a unit test that imports or mirrors the verify logic and asserts `verify("librarian", "<seeded_hash>") == true` AND `verify("wrongpass", "<seeded_hash>") == false`. Hardcode the hash string as a test constant. This catches the mismatch at `cargo test` time instead of at E2E run time.

- [x] **Task 2 — Extend `loginAs()` helper** (AC: #4, #5, #6)
  - [x] 2.1 Edit `tests/e2e/helpers/auth.ts`. Change signature to `export async function loginAs(page: Page, role: "admin" | "librarian" = "admin"): Promise<void>`.
  - [x] 2.2 Resolve username from role (`admin` → `"admin"`, `librarian` → `"librarian"`). Resolve password from env: `role === "admin" ? (process.env.TEST_ADMIN_PASSWORD ?? "admin") : (process.env.TEST_LIBRARIAN_PASSWORD ?? "librarian")`.
  - [x] 2.3 Keep the existing URL-wait logic (`page.waitForURL(/^(?!.*\/login).*$/, { timeout: 5000 })`) unchanged — both roles currently redirect to `/catalog` on success.
  - [x] 2.4 Update JSDoc to document the new signature, the default, and the env-var overrides. Reference both migrations (`20260331000004_fix_dev_user_hash.sql` for admin, the new librarian-seed migration for librarian).
  - [x] 2.5 Run `cd tests/e2e && npx tsc --noEmit` to verify the union type compiles and no call site broke.
  - [x] 2.6 **Wire `tsc --noEmit` into CI.** In the `e2e` job of `.github/workflows/_gates.yml`, add a step `- run: npx tsc --noEmit` before `npm test` (runs in ~3s, cheap). Prevents future typos like `loginAs(page, "librarrian")` from reaching runtime. If `tsc` is not yet a dev dependency in `tests/e2e/package.json`, add `typescript` as a devDependency first.
  - [x] 2.7 **Pass-through `TEST_LIBRARIAN_PASSWORD` in Docker E2E stack.** Check `tests/e2e/docker-compose.test.yml` for the service that runs Playwright. If it has an `environment:` block, add `TEST_LIBRARIAN_PASSWORD` alongside `TEST_ADMIN_PASSWORD`. If Playwright runs outside Docker (directly on the host via `npm test`), no change needed — env vars inherit naturally. If CI pass-through is non-trivial, explicitly document in CLAUDE.md: "override only works locally; CI always uses the seed default `librarian`".

- [x] **Task 3 — Migrate one smoke test to librarian role** (AC: #7)
  - [x] 3.0 **Audit the target spec before switching it.** Grep `tests/e2e/specs/journeys/epic2-smoke.spec.ts` for any action verbs that librarians may not yet be allowed to perform (e.g., creating/deleting locations, editing reference data, admin-only settings pages). Epic 7 has not yet introduced role guards, so in practice all routes work for librarian TODAY — but picking a navigation/read-only flow minimizes risk of a future Epic 7 gate retroactively breaking this smoke. Prefer a `loginAs(page)` call followed by a catalog navigation or shelf-browse assertion, not one that creates a location.
  - [x] 3.1 Pick `tests/e2e/specs/journeys/epic2-smoke.spec.ts` (Epic 2 smoke already uses `loginAs()` four times and exercises navigation + shelf browsing — safe for librarian). Change one of its `loginAs(page)` calls (ideally the most read-only one per 3.0) to `loginAs(page, "librarian")`.
  - [x] 3.2 Run the spec in isolation: `cd tests/e2e && npx playwright test specs/journeys/epic2-smoke.spec.ts` — expect green.
  - [x] 3.3 If the test fails because the chosen flow happens to require admin, pick a different navigation-only assertion or split the test so the librarian path covers only librarian-legal actions. Do NOT weaken assertions to paper over a real permission gap — if one exists, record it in the completion notes as an Epic 7 carry-over.

- [x] **Task 4 — Unit / DB-integration test for librarian seed** (AC: #8)
  - [x] 4.1 Add a `#[sqlx::test(migrations = "./migrations")]` test in an appropriate integration-test crate under `tests/` (create `tests/seeded_users.rs` or extend an existing crate). Mirror the attribute-with-migrations-path pattern from `tests/find_similar.rs` exactly — the `migrations = "./migrations"` argument is what runs the full migration chain including the new seed migration against a fresh DB per test. Assert (a) `SELECT username, role FROM users WHERE username IN ('admin','librarian') AND deleted_at IS NULL` returns both rows with the expected roles, AND (b) the librarian hash validates via the same `argon2::PasswordHash::new(...)` + `verify_password(...)` path used in `src/routes/auth.rs:187-194` — makes AC #2 a hard gate at `cargo test` time.
  - [x] 4.2 Verify via `docker compose -f tests/docker-compose.rust-test.yml up -d && SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' cargo test --test <crate_name>`.
  - [x] 4.3 Wire the new integration-test crate into `.github/workflows/_gates.yml` (the reusable gates file from story 6-1) so the `db-integration` job picks it up — add it to the `--test <name>` list alongside `find_similar`, `find_by_location_dewey`, `metadata_fetch_dewey`.

- [x] **Task 5 — Docs + CLAUDE.md** (AC: #10)
  - [x] 5.1 In CLAUDE.md, update the E2E Test Patterns → Login strategy subsection to mention `loginAs(page, role?)` with `"admin"` default and the `TEST_LIBRARIAN_PASSWORD` env-var override.
  - [x] 5.2 Do NOT change the Foundation Rule #7 language — smoke tests must still use `loginAs()` (role optional is fine); the rule is about "real browser login from blank context", unchanged.

- [x] **Task 6 — Verification & sprint-status flip** (AC: #9)
  - [x] 6.1 Run the full local gate: `cargo clippy -- -D warnings`, `cargo test`, `cargo sqlx prepare --check --workspace -- --all-targets`, the DB-integration crates via the rust-test compose, and `cd tests/e2e && npm test`. All green before pushing.
  - [x] 6.2 Push to a feature branch, open PR, confirm GitHub Actions (story 6-1's 3-job gate) passes. Update sprint-status.yaml `6-2-seed-librarian-and-loginas-role` → `review` when opening the PR; `done` after code review passes with no Medium+ findings (Foundation Rule #6).

## Dev Notes

### Architecture / code to touch

- **`src/middleware/auth.rs`** — defines `Role::{Anonymous, Librarian, Admin}` and `Role::from_db()` (src/middleware/auth.rs:9-34). No change needed; role mapping for "librarian" already works.
- **`src/main.rs:41`** — runs `sqlx::migrate!("./migrations")` at boot. The new migration file is picked up automatically.
- **`migrations/`** — append-only. Use a timestamp after `20260412000001_widen_dewey_code.sql`. Format mirrors `20260329000002_seed_dev_user.sql` for the `WHERE NOT EXISTS` idempotency pattern.
- **`tests/e2e/helpers/auth.ts`** — 32 lines, one exported `loginAs`, one `logout`. Small surgical change.
- **`tests/e2e/specs/journeys/epic2-smoke.spec.ts`** — pick ONE of its 4 `loginAs()` calls for the librarian migration.

### Argon2 hash generation

The existing admin hash uses parameters `m=19456,t=2,p=1`. Match them for the librarian hash so the login path doesn't hit any unexpected parameter mismatch (argon2-rs verifies params embedded in the hash string, but staying consistent avoids surprise if we later enforce a minimum-cost policy). Options:

- **One-off Rust snippet** (safest — same crate version as prod): create a throwaway binary under `src/bin/` that takes a password on stdin and prints the hash, run once, delete the binary before PR merge. Don't commit it.
- **`argon2` CLI**: `echo -n "librarian" | argon2 somesalt -id -m 19456 -t 2 -p 1 -e`. Accept it if you don't have rust tooling set up for this.
- **Online calculator**: acceptable ONLY for seed-test data — never for real user passwords. Document the tool in the migration comment.

### Testing standards

Per CLAUDE.md Foundation Rules:
- Rule #2: unit tests alongside implementation — Task 4's `#[sqlx::test]` covers the seed.
- Rule #3: E2E coverage — Task 3 provides the real-browser librarian login flow.
- Rule #7: smoke tests from blank context — epic2-smoke.spec.ts is already Rule-#7 compliant; Task 3 preserves that.
- Rule #5: all tests green before milestone transition — Task 6 enforces.

### E2E parallel-mode pitfalls to respect

From story 5-1b (CLAUDE.md "Known app quirks"): do NOT share session state between tests. The librarian-migrated smoke test MUST start from a blank context (no cookie injection, `loginAs(page, "librarian")` is the correct entry point). `fullyParallel: true` must remain enabled.

### Project Structure Notes

- No new directory needed. All edits fit existing structure: one migration file, one helper edit, one spec edit, one new `#[sqlx::test]` (in a new or existing `tests/*.rs` crate).
- If adding a new integration-test crate file, follow the naming pattern of `tests/find_similar.rs` so the CI gate globs pick it up consistently — and remember Task 4.3 to wire it into `_gates.yml`.

### Previous story intelligence (story 6-1)

- 6-1 delivered the reusable `.github/workflows/_gates.yml` with the `db-integration` job. When Task 4 adds a new integration-test crate, it MUST be added to the `--test <name>` list in that file, otherwise CI silently skips it and the story appears green locally but the new test isn't actually gating merges. Check `.github/workflows/_gates.yml` as written by 6-1.
- 6-1 enforced `actionlint` clean on all workflow files locally. Re-run `actionlint .github/workflows/*.yml` after Task 4.3 edits.

### References

- [Source: CLAUDE.md → E2E Test Patterns → Login strategy]
- [Source: _bmad-output/planning-artifacts/epics.md#Story-6.2]
- [Source: _bmad-output/implementation-artifacts/6-1-github-and-ci-cd-pipeline.md#Task-1.3]
- [Source: src/middleware/auth.rs:9-34 — Role enum + from_db]
- [Source: migrations/20260329000002_seed_dev_user.sql — INSERT ... WHERE NOT EXISTS pattern]
- [Source: migrations/20260331000004_fix_dev_user_hash.sql — admin Argon2id hash + parameters]
- [Source: tests/e2e/helpers/auth.ts — current loginAs implementation]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context), via `bmad-dev-story` workflow.

### Debug Log References

- Pre-existing parallel-mode E2E flakes confirmed by `git stash` experiment: `epic2-smoke.spec.ts` (3 tests) and `metadata-editing.spec.ts` (1 smoke) fail against HEAD (`ef51cf1`) before any change from this story was applied. These relate to `createLocation()` form submission not landing on `/locations` in parallel mode — pre-dating story 6-2 and tracked as the known baseline ahead of stories 6-3 / 6-4.

### Completion Notes List

**Delivered:**

- New migration `migrations/20260414000001_seed_librarian_user.sql` seeds `librarian/librarian` with `role='librarian'` using an Argon2id hash (`m=19456,t=2,p=1`) generated by a throwaway `src/bin/generate_librarian_hash.rs` (deleted after use). `INSERT ... WHERE NOT EXISTS` keeps it idempotent.
- `tests/e2e/helpers/auth.ts::loginAs(page, role?)` — union type `"admin" | "librarian"`, default `"admin"`. `TEST_LIBRARIAN_PASSWORD` env var mirrors `TEST_ADMIN_PASSWORD`. JSDoc updated.
- `tsc --noEmit` wired: `tests/e2e/tsconfig.json` + `typescript`/`@types/node` devDeps + `npm run typecheck`. `tests/e2e/global.d.ts` declares the `htmx` browser global used inside `page.evaluate`. The `e2e` job in `.github/workflows/_gates.yml` runs `npx tsc --noEmit` before `npm test`.
- `src/routes/auth.rs` — new unit test `test_librarian_seed_hash_verifies` hardcodes the seeded hash and checks both positive/negative verification against the prod `verify_password` path. Catches any future hash/variant drift at `cargo test` time.
- `tests/seeded_users.rs` — new `#[sqlx::test(migrations="./migrations")]` crate with two tests: (a) both `admin` and `librarian` seed rows exist with correct roles, (b) the stored librarian hash verifies `librarian/librarian` via the same `argon2` path as production. Added to `_gates.yml` `db-integration` list.
- `tests/e2e/specs/journeys/librarian-smoke.spec.ts` — new smoke (no cookie injection) that logs in as librarian, visits `/catalog` and `/locations`, and asserts the logout link is present. Passes locally and is Rule-#7 compliant.
- CLAUDE.md E2E → Login strategy updated with the new signature, env-var overrides, and the typecheck gate. Foundation Rule #7 language unchanged per story guidance.

**Deviations from story text (recorded per Task 3.3):**

- The story named `tests/e2e/specs/journeys/epic2-smoke.spec.ts` as the migration target. Every test in that spec calls `createLocation()`, and `create_location` in `src/routes/locations.rs:319-324` requires `Role::Admin`. Switching one of those tests to `loginAs(page, "librarian")` therefore hits a real permission gap, not a test bug. Per Task 3.3 ("Do NOT weaken assertions to paper over a real permission gap — record it in the completion notes as an Epic 7 carry-over"), I did not weaken or split the epic2-smoke tests. Instead I added a dedicated `librarian-smoke.spec.ts` that exercises a genuinely read-only path (catalog + locations GET). The spirit of AC #7 (prove the end-to-end librarian browser flow) is satisfied; the letter (modify an epic2-smoke test) is not.
- **Epic 7 carry-over:** `POST /locations`, `POST /locations/:id` (edit/update/delete/next_lcode) currently require Admin in `src/routes/locations.rs`. That looks over-restrictive for a library assistant in a co-librarian model — Epic 7 should re-evaluate whether Librarian should be able to create/edit locations, and either (a) downgrade these guards to `Role::Librarian`, or (b) document the Admin-only decision.

**AC #9 status — partial:** `cargo clippy --all-targets -- -D warnings`, `cargo test --lib --bins` (327 passed incl. the new test), `cargo sqlx prepare --check --workspace -- --all-targets`, and `cargo test --test seeded_users --test find_similar --test find_by_location_dewey --test metadata_fetch_dewey` (all DB integration tests) are all green. The new `librarian-smoke` spec is green. The full E2E suite reports 130 passed / 4 failed; all 4 failures are pre-existing flakes (verified via `git stash` against HEAD `ef51cf1`) and are the explicit scope of stories 6-3 / 6-4. Reviewer to confirm whether this "known baseline" counts as green for story 6-2's gate.

**Throwaway artifact cleanup:** `src/bin/generate_librarian_hash.rs` was created, run once to produce the hash, and deleted (per Dev Notes guidance). Not present in File List.

### File List

- Added: `migrations/20260414000001_seed_librarian_user.sql`
- Added: `tests/seeded_users.rs`
- Added: `tests/e2e/specs/journeys/librarian-smoke.spec.ts`
- Added: `tests/e2e/tsconfig.json`
- Added: `tests/e2e/global.d.ts`
- Modified: `src/routes/auth.rs` (new unit test only)
- Modified: `tests/e2e/helpers/auth.ts` (new signature + JSDoc)
- Modified: `tests/e2e/package.json` (typescript / @types/node devDeps, `typecheck` script)
- Modified: `tests/e2e/package-lock.json` (lockfile sync)
- Modified: `.github/workflows/_gates.yml` (`--test seeded_users` in db-integration; `tsc --noEmit` in e2e job)
- Modified: `CLAUDE.md` (E2E → Login strategy updated)
- Modified: `_bmad-output/implementation-artifacts/sprint-status.yaml` (6-2 → in-progress, then review)

### Review Findings

- [x] [Review][Decision] AC#7 deviation — `librarian-smoke.spec.ts` added instead of migrating `epic2-smoke.spec.ts`. **Resolved 2026-04-14: accepted** — spirit of AC satisfied, Epic 7 carry-over recorded for role-guard re-evaluation in `src/routes/locations.rs`.
- [x] [Review][Decision] AC#9 partial — 4 pre-existing E2E failures (stories 6-3/6-4 scope). **Resolved 2026-04-14: accepted as known baseline** for 6-2 gate only. Going forward, Guy confirmed: any failing test blocks PRs — baseline must be cleared by 6-3/6-4 before Epic 6 closes.
- [x] [Review][Patch] `tests/seeded_users.rs` missing `COUNT = 2` + `active = TRUE` assertions (AC#1 explicit) [tests/seeded_users.rs]
- [x] [Review][Patch] `loginAs` `??` leaks empty-string `TEST_LIBRARIAN_PASSWORD`/`TEST_ADMIN_PASSWORD` [tests/e2e/helpers/auth.ts]
- [x] [Review][Patch] `librarian-smoke.spec.ts` has no role-specific assertion — would pass as admin too [tests/e2e/specs/journeys/librarian-smoke.spec.ts]
- [x] [Review][Patch] Seed migration `WHERE NOT EXISTS (... deleted_at IS NULL)` is logically weaker than the UNIQUE(username) index (covers soft-deleted rows too) — add comment or widen guard [migrations/20260414000001_seed_librarian_user.sql]
- [x] [Review][Patch] Task 2.7 — CLAUDE.md missing explicit "override only works locally; CI always uses seed default `librarian`" disclaimer [CLAUDE.md]
- [x] [Review][Defer] `INSERT ... WHERE NOT EXISTS` seed pattern ignores hash/role drift — deferred, pre-existing convention shared with `seed_dev_user.sql`
- [x] [Review][Defer] `tsconfig.json` does not enable `noUncheckedIndexedAccess` / `exactOptionalPropertyTypes` — deferred, broader E2E TS hygiene pass

## Change Log

- 2026-04-14 — Story 6-2 implemented: librarian seed migration, role-aware `loginAs()`, TS typecheck gate, DB-integration test for seed presence, dedicated librarian smoke spec. Status → review.
- 2026-04-14 — Code review: 2 decisions, 5 patches, 2 deferred, 6 dismissed. Status stays `review` pending decisions.
