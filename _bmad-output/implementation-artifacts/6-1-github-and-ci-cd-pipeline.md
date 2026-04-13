# Story 6.1: GitHub repo + CI/CD pipeline + Docker Hub publishing

Status: in-progress

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a project maintainer,
I want every push validated by an automated GitHub Actions pipeline and every tagged release producing a Docker Hub image,
so that I can ship mybibli with confidence, merge-gated on green tests, and without manually building/pushing images.

## Scope at a glance (read this first)

**Mostly infrastructure, not code.** The current repo already has:

- A `main` branch tracking `origin/main` at `github.com/guycorbaz/mybibli.git` (verify via `git remote -v`) — **no `master` rename is needed** (the epic acceptance criterion was written before the repo was already on `main`; update wording accordingly but skip the branch-rename work).
- A minimal single-job CI at `.github/workflows/ci.yml` that runs `cargo test`, `clippy`, `sqlx prepare --check` in one ubuntu-latest job. This file is **replaced wholesale** (or renamed) by this story.
- A production `Dockerfile` at repo root and a working E2E stack at `tests/e2e/docker-compose.test.yml` (+ `tests/docker-compose.rust-test.yml` for the DB-integration crates). Both compose stacks are already parametrized and work locally per CLAUDE.md.
- `Cargo.toml` at version `0.1.0` — the tag-version verification step must read this value.

**Four pieces this story delivers:**

1. **3-job gate workflow** (`.github/workflows/ci.yml`, replaces existing) with `rust-tests`, `db-integration`, `e2e` jobs running in parallel on every push and PR. A PR cannot merge unless all three are green (enforced via branch protection — see Task 5).
2. **Docker Hub publishing on `main`** — a 4th job `docker-publish` that runs only on push to `main` AFTER the 3 gates pass, builds the root `Dockerfile`, tags as `guycorbaz/mybibli:main-<sha7>`, and pushes using `DOCKERHUB_USERNAME` + `DOCKERHUB_TOKEN` repo secrets.
3. **Tag release flow** — a separate `.github/workflows/release.yml` triggered by `v*` tags that (a) verifies `Cargo.toml` version matches the tag, (b) runs the same 3 gates, (c) builds and pushes `guycorbaz/mybibli:<semver>` + `guycorbaz/mybibli:latest`.
4. **Artifact upload on failure** — Playwright traces/screenshots and DB-integration logs uploaded as GitHub artifacts when the respective job fails, for remote debugging.

**Explicitly NOT in scope:**

- Multi-arch Docker builds (amd64 only for v1 — arm64 deferred)
- Container image signing / SBOM
- Deploying anywhere (Docker Hub publish only)
- Renaming `master` → `main` (already done — local and remote already on `main`)
- `waitForTimeout` grep gate (story 6-4 wires it into the workflow this story creates)
- Seeded librarian user (story 6-2)
- Fixing `manually_edited_fields` race (story 6-3)
- Dependabot config (see Open Question Q5)

## Decisions to Resolve BEFORE Implementation

**Resolve Q1–Q3 with Guy before starting Task 2.** Q4–Q5 are informational.

1. **`latest` tag policy** — this story reserves `latest` for tag releases only. If Guy prefers `latest` → `main` (common solo-project pattern), swap: `main` push → `:latest`, tag release → `:<semver>` + `:stable`. **BLOCKS Task 2.**
2. **Docker Hub repo privacy** — public (current assumption) or private? Public simplifies `docker pull` testing but exposes image layers. **BLOCKS first `docker-publish` run.**
3. **Reusable workflow vs duplication for `release.yml`** — story mandates reusable (see Task 3.3). Confirm before touching `release.yml`.
4. **Multi-arch (amd64 + arm64)** — out of scope v1. Future story if Guy deploys to arm64.
5. **Dependabot** — not in this story. Candidate follow-up story in Epic 6 retro.

## Acceptance Criteria

1. **Remote is `main` on GitHub:** Given the current local branch is `main`, when `git remote -v` is run, then `origin` points to `https://github.com/guycorbaz/mybibli.git` for both fetch and push. If the remote's default branch on GitHub is still `master`, it is switched to `main` via GitHub repo settings (one-time click, documented in the completion notes — no CLI rename step required because the local branch is already `main`).
2. **3-job parallel gate — green path:** Given `.github/workflows/ci.yml`, when a push or PR event fires, then GitHub Actions runs exactly 3 jobs in parallel with `needs:` relationships **only** on the publish job (see AC #6): `rust-tests` (runs `cargo clippy -- -D warnings`, `cargo test` for lib + bins, `cargo sqlx prepare --check --workspace -- --all-targets`), `db-integration` (spins a MariaDB 10.11 service container with credentials matching `tests/docker-compose.rust-test.yml`, runs `cargo test --test find_similar --test find_by_location_dewey --test metadata_fetch_dewey` against it), `e2e` (uses `docker compose -f tests/e2e/docker-compose.test.yml up -d --build --wait`, runs `cd tests/e2e && npm ci && npx playwright install --with-deps chromium && npm test`, then `docker compose down -v`). All three report as separate required checks on the PR.
3. **3-job parallel gate — red path blocks merge:** Given a PR where any of the 3 gate jobs fails (e.g., clippy warning, DB test failure, Playwright test failure), when GitHub branch-protection rules are enforced, then the "Merge" button is disabled until the failing job passes. Branch protection for `main` requires: all 3 gate jobs passing + at least 1 review OR the author approving their own PR if solo-maintainer mode is accepted (Guy is solo maintainer — this story documents the exact protection policy in `docs/ci-cd.md`, see AC #8).
4. **Artifacts on failure:** Given a failed `e2e` job, when the job finishes, then the `tests/e2e/playwright-report/` directory AND `tests/e2e/test-results/` directory are uploaded as a GitHub Actions artifact named `playwright-report-<run-id>` with 7-day retention. Similarly, a failed `db-integration` job uploads the MariaDB container's last-50-lines log via `docker logs`. Artifact upload steps use `if: failure()` so green runs don't bloat storage.
5. **Secrets present & not leaked:** Given repo secrets `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` are configured in the GitHub repo Settings → Secrets → Actions, when the publish jobs run, then `docker login` succeeds. The workflow file references them strictly via `${{ secrets.DOCKERHUB_USERNAME }}` / `${{ secrets.DOCKERHUB_TOKEN }}` — zero plaintext credentials in any `.yml`. A grep gate in the completion notes verifies `grep -rE "(password|token|secret).*=" .github/workflows/ | grep -v "secrets\\."` returns zero matches.
6. **Docker Hub publish on `main` merge:** Given a push to `main` that causes all 3 gate jobs to complete successfully, when the `docker-publish` job runs (it has `needs: [rust-tests, db-integration, e2e]` + `if: github.ref == 'refs/heads/main' && github.event_name == 'push'`), then it builds from the root `Dockerfile`, logs in to Docker Hub with the two secrets, tags the image as `guycorbaz/mybibli:main-<sha7>` (where `<sha7>` is `${{ github.sha }}` truncated to 7 chars via `echo ${GITHUB_SHA:0:7}`), and pushes it. The image is visible at `https://hub.docker.com/r/guycorbaz/mybibli/tags`.
7. **Tag release flow — version match required:** Given a git tag matching regex `^v[0-9]+\.[0-9]+\.[0-9]+$` (e.g. `v0.1.0`, `v1.2.3`; no pre-release suffixes in v1), when the tag is pushed, then `.github/workflows/release.yml` triggers on `push.tags`. Its first step extracts the tag (e.g. `v0.1.0` → `0.1.0`) and reads `Cargo.toml` `version = "..."` (via `grep -E '^version' Cargo.toml | cut -d'"' -f2` or a small action). **If they mismatch, the workflow fails immediately** with a clear error message `Cargo.toml version (X.Y.Z) does not match tag (A.B.C) — bump Cargo.toml and re-tag`. No Docker build happens on mismatch.
8. **Tag release flow — happy path tags:** Given a tag push where Cargo.toml matches, when the release workflow runs, then it (a) re-runs the 3 gates (rust-tests + db-integration + e2e as reusable workflow OR duplicated jobs — implementer's choice, prefer reusable via `workflow_call` in `ci.yml`), (b) builds the Docker image once, (c) pushes two tags: `guycorbaz/mybibli:<semver>` AND `guycorbaz/mybibli:latest`. Both tags are visible on Docker Hub. The `latest` tag is ONLY updated on tag releases, never on `main` pushes.
9. **Documentation — `docs/ci-cd.md`:** A new markdown file at `docs/ci-cd.md` documents (for future maintainers + AI agents): (i) the 3-job layout and what each runs, (ii) required GitHub repo secrets + how to rotate them, (iii) branch-protection settings applied to `main` (screenshots optional but a precise textual checklist is mandatory), (iv) the release procedure ("bump Cargo.toml, commit, tag `v<semver>`, push tag"), (v) how to retrieve Playwright artifacts from a failed run, (vi) known CI-only gotchas (e.g., service-container DNS uses `db` hostname not `localhost`, mock-metadata container warmup time).
10. **README badge:** The `README.md` top section gets a CI status badge: `![CI](https://github.com/guycorbaz/mybibli/actions/workflows/ci.yml/badge.svg)`. Optional: a Docker Hub pulls badge (skip if it bloats the README — Guy's call).
11. **Local parity — no new install steps:** Running `cargo test` and `docker compose -f tests/e2e/docker-compose.test.yml up -d` + `cd tests/e2e && npm test` locally continues to work unchanged. The CI workflow MUST NOT assume any new local tooling (no `cargo-make`, no `just`, no custom shell scripts outside `.github/workflows/`). If a helper script is genuinely needed, it goes into `.github/scripts/` with executable bit set and a short header comment.
12. **Verification gate (Foundation Rule #5):** On a draft PR opened from a feature branch, all 3 gate jobs go green within the GitHub Actions UI within ~25 min total wall-clock (rust-tests ~5 min, db-integration ~5 min, e2e ~20 min — the long pole). `cargo clippy -- -D warnings`, `cargo test`, `cargo sqlx prepare --check --workspace -- --all-targets`, full integration-test crates, and `cd tests/e2e && npm test` all green. A test-only PR that deliberately breaks a spec is used to verify the red path (AC #3) produces a non-mergeable PR; the break is then reverted. Update story status to `review`.

## Tasks / Subtasks

- [x] **Task 1 — Replace `.github/workflows/ci.yml` with 3-job parallel gate** (AC: #2, #4)
  - [x] 1.1 Delete or rewrite the current single-job `ci.yml`. Keep `on: [push, pull_request]` triggers (all branches on push, PR targets `main`). Add workflow-level `permissions: { contents: read }` (hardening — least privilege). Add workflow-level `concurrency: { group: ci-${{ github.ref }}, cancel-in-progress: true }` to auto-cancel superseded runs on the same branch (saves minutes on push-heavy days).
  - [x] 1.1b Set `timeout-minutes` per job: `rust-tests: 15`, `db-integration: 15`, `e2e: 30`, `docker-publish: 20` (Task 2). Without these, a hung job burns GitHub's 6-hour default.
  - [x] 1.2 Define job `rust-tests` on `ubuntu-latest`: checkout + `dtolnay/rust-toolchain@stable` with `clippy` + `Swatinem/rust-cache@v2` for target/ caching (faster than the manual `actions/cache` currently in place) + `cargo install sqlx-cli --no-default-features --features mysql,rustls --locked` + run `cargo clippy -- -D warnings`, `cargo test --lib --bins`, `cargo sqlx prepare --check --workspace -- --all-targets`. Env: `SQLX_OFFLINE: "true"`, `CARGO_TERM_COLOR: always`.
  - [x] 1.3 Define job `db-integration` on `ubuntu-latest` with a `services.mariadb` container: image `mariadb:10.11`, env `MARIADB_ROOT_PASSWORD: root_test`, `MARIADB_DATABASE: mybibli_rust_test`, `MARIADB_USER: test`, `MARIADB_PASSWORD: test`, port `3307:3306`, healthcheck mirroring `tests/docker-compose.rust-test.yml`. Env: `DATABASE_URL: mysql://root:root_test@127.0.0.1:3307/mybibli_rust_test`, `SQLX_OFFLINE: "true"`. Steps: checkout + rust-toolchain + rust-cache + `cargo test --test find_similar --test find_by_location_dewey --test metadata_fetch_dewey`. **Do NOT run `cargo test` without `--test` filters** — that would re-run all lib/bin tests already covered by `rust-tests`, doubling CI minutes for no benefit.
  - [x] 1.4 Define job `e2e` on `ubuntu-latest`: checkout + setup-node@v4 (Node 20 LTS) + `docker compose -f tests/e2e/docker-compose.test.yml up -d --build --wait` + `cd tests/e2e && npm ci && npx playwright install --with-deps chromium && npm test`. **Chromium only** per `tests/e2e/playwright.config.ts` (`use: { browserName: "chromium" }`) — do NOT install firefox/webkit. Cache Playwright browsers via `actions/cache@v4` with key `playwright-${{ hashFiles('tests/e2e/package-lock.json') }}` and path `~/.cache/ms-playwright` (saves ~90s per run). After-steps (with `if: always()`): `docker compose -f tests/e2e/docker-compose.test.yml logs --no-color > /tmp/compose-logs.txt` + `docker compose -f tests/e2e/docker-compose.test.yml down -v`.
  - [x] 1.4b **Verify migration bootstrap before Task 1 goes green.** Confirmed: `src/main.rs:41` auto-runs `sqlx::migrate!("./migrations")` at boot — no extra CI step needed.
  - [x] 1.5 Add `if: failure()` upload steps: `actions/upload-artifact@v4` for `tests/e2e/playwright-report/` AND `tests/e2e/test-results/` (name: `playwright-report-${{ github.run_id }}`, retention: 7 days), and a second upload for `/tmp/compose-logs.txt` as `compose-logs-${{ github.run_id }}`. For `db-integration` failures, MariaDB container log tail uploaded as `mariadb-logs-${{ github.run_id }}`.
  - [x] 1.6 Made `rust-tests`, `db-integration`, `e2e` parallel via reusable `_gates.yml` — no `needs:` between them; `docker-publish` alone declares `needs: [rust-tests, db-integration, e2e]`.
  - [x] 1.7 Verified `actionlint 1.7.1` clean on all three workflow files locally; did NOT add a CI-side lint job (kept feedback loop tight per story note).

- [x] **Task 2 — Add `docker-publish` job for `main` pushes** (AC: #5, #6)
  - [x] 2.1 `docker-publish` job added in `ci.yml` with `needs: [rust-tests, db-integration, e2e]` and `if: github.ref == 'refs/heads/main' && github.event_name == 'push'`.
  - [x] 2.2 `docker/metadata-action@v5` with `type=sha,format=short,prefix=main-` handles sha7 natively → tag `guycorbaz/mybibli:main-<sha7>`.
  - [x] 2.3 `cache-from: type=gha` + `cache-to: type=gha,mode=max` wired on `docker/build-push-action@v5`.
  - [x] 2.4 Summary step prints `Pushed ${{ steps.meta.outputs.tags }}`.

- [x] **Task 3 — Tag release workflow** (AC: #7, #8)
  - [x] 3.1 `.github/workflows/release.yml` created with `on: push: tags: ['v*.*.*']`.
  - [x] 3.2 `verify-version` job parses `${GITHUB_REF_NAME#v}`, reads Cargo.toml via `grep -E '^version = "[0-9]+\.[0-9]+\.[0-9]+"' | head -1 | cut -d'"' -f2`, rejects pre-release suffixes explicitly, fails with clear `::error::` message on mismatch, outputs `version` for downstream.
  - [x] 3.3 Reusable gates via `.github/workflows/_gates.yml` (`on: workflow_call`) — called by both `ci.yml` and `release.yml`. actionlint validated.
  - [x] 3.3b `_gates.yml` uses no secrets (confirmed — only rust-tests, db-integration, e2e). `publish` job in `release.yml` is a NORMAL job (not a reusable-workflow call), so `${{ secrets.DOCKERHUB_USERNAME }}` / `DOCKERHUB_TOKEN` resolve directly without needing `secrets: inherit`.
  - [x] 3.4 `publish` job has `needs: [verify-version, rust-tests, db-integration, e2e]`. `docker/metadata-action@v5` emits two raw tags: `${{ needs.verify-version.outputs.version }}` + `latest`. Single `build-push-action` call pushes both.
  - [ ] 3.5 Tag-mismatch smoke (DEFERRED to post-merge verification — requires push access to remote). Documented expected failure mode in `docs/ci-cd.md`.

- [ ] **Task 4 — Configure GitHub repo** (AC: #1, #3, #5)
  - [ ] 4.1 In GitHub Settings → Secrets and variables → Actions, add `DOCKERHUB_USERNAME` (value: `guycorbaz`) and `DOCKERHUB_TOKEN` (value: a Docker Hub PAT with push scope, created at https://hub.docker.com/settings/security). Rotate-and-document procedure goes into `docs/ci-cd.md` (Task 6).
  - [ ] 4.2 In GitHub Settings → Branches → Add rule for `main`: require status checks to pass — **EXACTLY these three**: `rust-tests`, `db-integration`, `e2e`. **🚨 DO NOT add `docker-publish` to required checks** — it is skipped on PRs (`if: github.ref == 'refs/heads/main' && github.event_name == 'push'`). A skipped check never reports a status, so marking it required would make every PR permanently unmergeable. Same rule applies to release-only jobs in `release.yml`. Require branches to be up to date. Do NOT require reviews (solo-maintainer mode — Guy approves their own PRs). Allow force-push: OFF. Allow deletion: OFF. Include administrators: ON (so Guy can't bypass accidentally).
  - [ ] 4.3 In Settings → General → Default branch, confirm `main`. If it's still `master`, switch it (single button click — no CLI needed since local is already on `main`).
  - [ ] 4.4 Document every setting from 4.1-4.3 in `docs/ci-cd.md` (Task 6) with exact navigation paths. Screenshots optional; **textual precision is mandatory** because Guy will re-do this from scratch on a second GitHub account eventually.

- [ ] **Task 5 — Smoke-test the red path** (AC: #3, #12)
  - [ ] 5.1 On a feature branch `ci-red-path-test`, deliberately break one Playwright spec (e.g., change a selector to a nonexistent ID). Push, open a draft PR. Observe: `e2e` fails, PR merge button is disabled with "Required status checks must pass before merging".
  - [ ] 5.2 Download the uploaded `playwright-report-*` artifact, verify the HTML report opens and shows the failing test trace.
  - [ ] 5.3 Revert the deliberate break, push again, observe all 3 gates go green, merge button re-enables. Close the PR without merging (it was a smoke test).
  - [ ] 5.4 Record the total wall-clock time for each gate job in `docs/ci-cd.md` "Timing baselines" section (rust-tests, db-integration, e2e). Future stories can measure regressions against these numbers.

- [x] **Task 6 — Write `docs/ci-cd.md`** (AC: #9)
  - [x] 6.1 `docs/ci-cd.md` created with all mandated sections: Overview, Job details, Secrets (PAT creation + rotation cadence of 90 days + leak-prevention grep gate), Branch protection (exact GitHub UI checklist matching Task 4 including the reusable-workflow check-name gotcha), Release procedure, Retrieving artifacts, Known gotchas (service-container DNS, mock-metadata warmup, Playwright install, migration bootstrap, reusable-workflow check names, reusable-workflow secrets).
  - [ ] 6.2 Timing baselines subsection — placeholder table added; values filled during Task 5 smoke test (pending remote push).
  - [x] 6.3 `docs/ci-cd.md` linked from README under Documentation section.

- [~] **Task 7 — README badge + verification gate** (AC: #10, #12)
  - [x] 7.1 CI badge added at top of `README.md` on its own line. No Docker Hub pulls badge.
  - [ ] 7.2 Full Foundation Rule #5 verification — clippy + sqlx-prepare --check passed locally. Full `cargo test` + DB integration + E2E + green CI run on a draft PR pending remote push. Local run intentionally scoped to no-infra verifications (no Rust source changed in this story).

## Dev Notes

### Current CI state (as of 2026-04-13)

- `.github/workflows/ci.yml` currently runs a single `build-and-test` job: clippy + `cargo test` + `cargo sqlx prepare --check`. **Replace wholesale.** Commit history: single commit from Epic 1 (`1-1-project-skeleton-and-foundation`). No existing workflow callers to worry about.
- Branch is already `main`; `origin` already points to `guycorbaz/mybibli.git`. The epic acceptance criterion about renaming `master` is stale — the actual work is only a GitHub UI "set default branch" confirmation (Task 4.3).

### GitHub Actions patterns to follow

- **Caching:** Use `Swatinem/rust-cache@v2` not the hand-rolled `actions/cache@v4` in the current workflow. It caches `~/.cargo/registry`, `~/.cargo/git`, and `target/` with a fingerprint of `Cargo.lock` + rustc version — handles cache-busting correctly and shaves ~2-3 min off cold runs.
- **Service containers:** Use the GitHub Actions `services:` YAML block (not `docker compose up` in a step) for `db-integration`. Service containers get the right DNS (`mariadb` → container hostname) and healthcheck wiring for free. Reference the official docs pattern at https://docs.github.com/en/actions/using-containerized-services/creating-mysql-service-containers (MariaDB is drop-in compatible).
- **E2E stack:** Full `docker compose up` IS the right pattern for E2E (not services) because the stack is more than just DB — it's app + DB + mock-metadata. Use `--wait` flag to block until all healthchecks pass (requires Compose V2.1+, which ubuntu-latest runners have).
- **Reusable workflows:** `.github/workflows/_gates.yml` with `on: workflow_call` exposes `rust-tests`, `db-integration`, `e2e` as callable jobs from both `ci.yml` and `release.yml`. If this refactor gets hairy, fallback to duplication + document as tech debt.

### Docker Hub secret creation

1. Visit https://hub.docker.com/settings/security
2. Click "New Access Token", scope: "Read, Write, Delete" on `guycorbaz/mybibli` repo only (not account-wide — least privilege).
3. Copy the token once (it's not shown again) into GitHub repo Settings → Secrets → Actions as `DOCKERHUB_TOKEN`. Add `DOCKERHUB_USERNAME=guycorbaz` as a companion secret.
4. Document the 30/60/90-day rotation cadence in `docs/ci-cd.md` — whichever Guy picks, write it down.

### Tag version parsing

Safest grep for `Cargo.toml` version: `grep -E '^version = "[0-9]+\.[0-9]+\.[0-9]+"' Cargo.toml | head -1 | cut -d'"' -f2`. The leading-`^` and quotes-required pattern avoids matching `version = "0.1.0-beta"` (v1 rejects pre-release suffixes) or a nested dependency `version =` line. Alternative using `cargo metadata --format-version 1 --no-deps | jq -r '.packages[0].version'` is more robust but adds a `jq` + full cargo-resolve round-trip — **prefer grep** for the CI step (it's 50ms vs 5s).

### Branch protection for solo maintainers

Guy is the only committer. Do NOT require reviews — it would make every merge require either a second GitHub account or the "admin override" button, which is friction. Instead: require the 3 status checks + "include administrators" so Guy can't accidentally push to `main` without going through a PR. This matches the spirit of "no unreviewed code" without actually needing a reviewer. Document the trade-off in `docs/ci-cd.md` so a future contributor understands why reviews aren't enforced.

### Dockerfile build target

The `Dockerfile` uses a 3-stage build with stage-1 `rust:alpine` cross-compiling to `x86_64-unknown-linux-musl` (fully static binary). `docker/build-push-action@v5` handles this natively for amd64 with no `platforms:` flag. **Do NOT add `platforms: linux/amd64,linux/arm64` without first testing musl cross-compile for arm64** — it's a non-trivial toolchain setup and explicitly out of scope for v1 (see "Decisions to Resolve" Q4).

### E2E timing in CI

Full Playwright suite currently runs 133 tests in ~6-8 min locally on fullyParallel mode (per CLAUDE.md and story 5-1c retrospective). In CI on ubuntu-latest (2 vCPU, no GPU), expect ~15-20 min including Docker build + `playwright install --with-deps chromium` (~2 min first run, cached after). Budget the `e2e` job timeout at 30 min; the other two at 15 min each.

### What to do when a gate fails

- **`rust-tests` clippy fail:** nearly always a new warning in a PR-introduced file. Fix locally, re-push.
- **`db-integration` fail:** often a schema-migration drift between the `.sqlx/` offline cache and the live DB. Run `cargo sqlx prepare` locally and commit the updated `.sqlx/` directory.
- **`e2e` fail:** download the `playwright-report-*` artifact from the failed run, open `index.html` in a browser — Playwright embeds screenshots + DOM snapshots at each step. MariaDB deadlock SQLSTATE 40001 occasionally bubbles up under load — `src/services/loans.rs` already retries 3x but if a new service hits it, apply the same retry pattern (story 5-1c precedent).

### Project Structure Notes

- New files: `.github/workflows/release.yml`, `.github/workflows/_gates.yml` (if reusable-workflow route taken), `docs/ci-cd.md`.
- Modified files: `.github/workflows/ci.yml` (wholesale replacement), `README.md` (one-line badge).
- No Rust source changes. No `migrations/` changes. No `locales/` changes. No template changes. This is a pure tooling + docs story.
- The `docs/` directory does not exist yet at repo root. Create it as part of Task 6.1 — no `.gitkeep` needed because `ci-cd.md` is its first file.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic 6: Pipeline CI/CD et fiabilité] — Story 6.1 AC source
- [Source: CLAUDE.md#Build & Test Commands] — canonical commands that the CI jobs invoke
- [Source: CLAUDE.md#E2E Test Patterns] — `docker compose -f tests/e2e/docker-compose.test.yml` is the canonical E2E stack composer
- [Source: tests/docker-compose.rust-test.yml] — MariaDB service-container credentials to mirror in `db-integration` job
- [Source: tests/e2e/docker-compose.test.yml] — full E2E stack spec
- [Source: Dockerfile at repo root] — production image built by `docker-publish` and `release.publish` jobs
- [Source: .github/workflows/ci.yml] — current single-job workflow being replaced
- [Source: _bmad-output/implementation-artifacts/epic-5-retro-2026-04-13.md] — Epic 5 retrospective that surfaced CI/CD as the next epic's priority (full read recommended before starting implementation for context on why this sequencing was chosen)

## Dev Agent Record

### Agent Model Used

claude-opus-4-6 (1M context) — dev-story session 2026-04-13

### Debug Log References

- `actionlint 1.7.1` validated all three workflow files — zero findings.
- Grep gate `grep -rE "(password|token|secret).*=" .github/workflows/ | grep -v "secrets\."` → empty (AC #5 satisfied in-code).
- Local `SQLX_OFFLINE=true cargo clippy --all-targets -- -D warnings` → green.
- Local `SQLX_OFFLINE=true cargo sqlx prepare --check --workspace -- --all-targets` → green (benign "potentially unused queries" warning, not a failure).
- Migration bootstrap question (Task 1.4b) resolved by reading `src/main.rs:41` → `sqlx::migrate!("./migrations")` runs at boot; no explicit migrate step needed in CI.

### Completion Notes List

**Scope shipped autonomously (no code changes to Rust source):**

1. `.github/workflows/_gates.yml` — reusable workflow (`on: workflow_call`) exposing three parallel jobs: `rust-tests`, `db-integration`, `e2e`. No `needs:` between them. MariaDB 10.11 service container for `db-integration` with port `3307:3306` and credentials matching `tests/docker-compose.rust-test.yml`. E2E job uses `docker compose ... up -d --build --wait`, caches `~/.cache/ms-playwright`, uploads `playwright-report/` + `test-results/` + compose logs on failure.
2. `.github/workflows/ci.yml` — replaces the old single-job workflow. Calls `_gates.yml` three times (one per gate) + adds `docker-publish` guarded by `if: github.ref == 'refs/heads/main' && github.event_name == 'push'` with `needs: [rust-tests, db-integration, e2e]`. Uses `docker/metadata-action@v5` `type=sha,format=short,prefix=main-` to generate `main-<sha7>` tag; buildx GHA cache enabled.
3. `.github/workflows/release.yml` — triggered by `v*.*.*` tags. `verify-version` job enforces `^[0-9]+\.[0-9]+\.[0-9]+$` tag regex AND Cargo.toml ↔ tag equality with a clear `::error::` message on mismatch. Then re-calls `_gates.yml` (three jobs) + a `publish` job pushing both `<semver>` and `latest` via `docker/metadata-action@v5` `type=raw`. Per Guy's Q1 decision, `latest` is updated ONLY on tag releases (never on `main` pushes).
4. `docs/ci-cd.md` — full maintainer guide including the Docker Hub PAT creation + 90-day rotation procedure, branch-protection checklist (with the reusable-workflow composite check-name gotcha called out), release procedure, artifact retrieval steps, and a failure playbook.
5. `README.md` — CI status badge + link to `docs/ci-cd.md`.

**Decisions resolved with user (2026-04-13 session):**

- Q1 `latest` tag policy → **semver only** (tag releases). `main` push tags as `main-<sha7>` only.
- Q2 Docker Hub repo → **public**.
- Q3 Reusable workflow → **OK**, implemented as `_gates.yml`.

**User completed manually (confirmed 2026-04-13):**

- GitHub repo secrets `DOCKERHUB_USERNAME` + `DOCKERHUB_TOKEN` created at https://github.com/guycorbaz/mybibli/settings/secrets/actions.

**Pending handoff — blocks story → review transition:**

- **Task 4.2** — Configure branch protection on `main` per `docs/ci-cd.md#branch-protection-for-main`. The three required checks must be selected as `rust-tests / rust-tests`, `db-integration / db-integration`, `e2e / e2e` (GitHub reports reusable-workflow checks as `<parent-job-id> / <called-job-name>`). Do NOT add `docker-publish` or release-only jobs to required checks — they skip on PRs and would lock merges.
- **Task 4.3** — Verify default branch is `main` in GitHub Settings → General (no-op if already `main`).
- **Task 5** — Red-path smoke (deliberately break a Playwright spec on a branch `ci-red-path-test`, verify PR merge button is disabled, retrieve the Playwright artifact, revert). Record per-gate wall-clock times → fill `docs/ci-cd.md#timing-baselines` and Task 6.2.
- **Task 7.2** — Observe first green CI run on a PR (AC #12 verification gate). Then transition story → `review`.
- **Task 3.5** — Optional tag-mismatch smoke test. Can be deferred to a later release-cadence story; not a merge blocker.

**Architectural note (for future contributors):** `_gates.yml` takes no inputs and no secrets. If a future gate needs a secret, the caller must pass `secrets: inherit` on its `uses:` line (both `ci.yml` and `release.yml`). Documented in `docs/ci-cd.md#known-gotchas`.

### File List

- `.github/workflows/_gates.yml` (new)
- `.github/workflows/ci.yml` (replaced — previously single-job)
- `.github/workflows/release.yml` (new)
- `docs/ci-cd.md` (new — `docs/` directory also new)
- `README.md` (modified — CI badge + docs link)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (modified — 6-1 → in-progress)
- `_bmad-output/implementation-artifacts/6-1-github-and-ci-cd-pipeline.md` (modified — status, task checkboxes, Dev Agent Record)

### Change Log

- 2026-04-13 — Story 6-1 in-progress: authored `_gates.yml`, `ci.yml` (replaced), `release.yml`, `docs/ci-cd.md`, README badge + docs link. Tasks 1/2/3/6/7.1 complete autonomously; Tasks 4/5/7.2 pending manual GitHub UI + remote push verification before story → review.

<!-- Open Questions moved to "Decisions to Resolve BEFORE Implementation" at the top of the story. -->

