# CI/CD — mybibli

This document describes the GitHub Actions pipeline, Docker Hub publishing, and repo configuration for mybibli. Target audience: future maintainers (and AI agents) bringing up a replica or debugging a failed run.

## Overview — the 3-gate model

Every push on any branch and every PR targeting `main` runs three parallel jobs that must all pass for the run to be green:

| Gate | Purpose | Typical duration |
|------|---------|------------------|
| `rust-tests`     | Lint + unit/bin tests + SQLx offline-cache check     | ~5 min |
| `db-integration` | `#[sqlx::test]` suites against a MariaDB 10.11 service container | ~5 min |
| `e2e`            | Full Playwright suite against the Docker Compose stack | ~15–20 min |

The three gates are exposed as a reusable workflow at `.github/workflows/_gates.yml` (called via `uses: ./.github/workflows/_gates.yml`). Both `ci.yml` and `release.yml` call the same gates so CI and release paths are guaranteed identical.

After the gates pass:

- **On push to `main`** — nothing else runs. `ci.yml` ends after the 3 gates. **No Docker Hub publish on main pushes** (per "version stricte" policy: images ship only at semver releases, never per mid-Epic story merge).
- **On `v*.*.*` tag push** — `release.yml` runs `verify-version` (Cargo.toml vs. tag), re-runs the 3 gates, then `publish` pushes `gcorbaz/mybibli:<semver>` and `gcorbaz/mybibli:latest`.

Docker Hub publishing is the exclusive responsibility of `release.yml`; both `<semver>` and `latest` tags are produced only on `v*.*.*` tag pushes.

## Job details

### `rust-tests`

- Runner: `ubuntu-latest`
- Toolchain: `dtolnay/rust-toolchain@stable` with `clippy`
- Cache: `Swatinem/rust-cache@v2`
- Commands:
  ```bash
  cargo install sqlx-cli --no-default-features --features mysql,rustls --locked
  cargo clippy --all-targets -- -D warnings
  cargo test --lib --bins
  cargo sqlx prepare --check --workspace -- --all-targets
  ```
- Env: `SQLX_OFFLINE=true`, `CARGO_TERM_COLOR=always`

### `db-integration`

- Runner: `ubuntu-latest`
- Service container: `mariadb:10.11` on host port `3307`, credentials match `tests/docker-compose.rust-test.yml` (`root_test` root password, `mybibli_rust_test` DB).
- Commands:
  ```bash
  cargo test --test find_similar --test find_by_location_dewey --test metadata_fetch_dewey
  ```
- Env: `DATABASE_URL=mysql://root:root_test@127.0.0.1:3307/mybibli_rust_test`, `SQLX_OFFLINE=true`
- On failure: MariaDB container logs (last 200 lines) are uploaded as artifact `mariadb-logs-<run-id>`.

### `e2e`

- Runner: `ubuntu-latest`, Node 20 LTS
- Stack: `docker compose -f tests/e2e/docker-compose.test.yml up -d --build --wait` brings up app + MariaDB + mock metadata provider.
- Commands:
  ```bash
  npm ci
  npx playwright install --with-deps chromium  # Chromium only per playwright.config.ts
  npm test
  ```
- Caches: npm (`cache: npm` on `setup-node`) + `~/.cache/ms-playwright` via `actions/cache@v4`.
- On failure: `tests/e2e/playwright-report/` + `tests/e2e/test-results/` uploaded as `playwright-report-<run-id>`; full compose logs uploaded as `compose-logs-<run-id>` (7-day retention).
- Post-step (`if: always()`): `docker compose ... down -v` cleans up containers and volumes.

## Secrets

| Name                 | Purpose                                                                        |
|----------------------|--------------------------------------------------------------------------------|
| `DOCKERHUB_USERNAME` | Docker Hub account (`guycorbaz`), used by `docker/login-action@v3`.            |
| `DOCKERHUB_TOKEN`    | Docker Hub Personal Access Token scoped to the `gcorbaz/mybibli` repo only.  |

### Creating the Docker Hub PAT

1. Log in to https://hub.docker.com/settings/security
2. Click **New Access Token**
3. Name: `github-actions-mybibli` (or similar)
4. Scope: **Read, Write, Delete** on `gcorbaz/mybibli` only (not account-wide — least privilege).
5. Copy the token once (it is not shown again).

### Adding secrets to GitHub

1. Visit https://github.com/guycorbaz/mybibli/settings/secrets/actions
2. Click **New repository secret**
3. Add `DOCKERHUB_USERNAME` = `guycorbaz`
4. Add `DOCKERHUB_TOKEN` = the PAT from the step above

### Rotation

Rotate the PAT every **90 days** (or immediately if leaked):

1. Create a new PAT at Docker Hub (same scope).
2. Update `DOCKERHUB_TOKEN` in GitHub Secrets (**Update** button — keeps the name).
3. Revoke the old PAT at https://hub.docker.com/settings/security once a CI run with the new token succeeds.

### Leak-prevention invariant

The CI workflows MUST reference secrets exclusively via `${{ secrets.NAME }}`. No plaintext credentials in any `.yml`. Run:

```bash
grep -rE "(password|token|secret).*=" .github/workflows/ | grep -v "secrets\\."
```

Expected output: empty. If non-empty, a secret is hard-coded — fix before merging.

## Branch protection for `main`

Navigate to **Settings → Branches → Branch protection rules → Add rule** (or edit the existing rule for `main`):

1. **Branch name pattern:** `main`
2. **Require a pull request before merging:** ON
   - **Require approvals:** OFF (solo-maintainer mode — see rationale below).
3. **Require status checks to pass before merging:** ON
   - **Require branches to be up to date before merging:** ON
   - **Required status checks** — add EXACTLY these three (the parent job that calls the reusable workflow is named `gates`):
     - `gates / rust-tests`
     - `gates / db-integration`
     - `gates / e2e`

     > GitHub shows reusable-workflow jobs as `<parent-job-id> / <called-job-name>`. If the exact label does not appear in the autocomplete, push a dummy commit first so GitHub learns the check names, then edit the protection rule.
4. **Do NOT add release-only jobs to required checks.** `verify-version` and `publish` from `release.yml` only run on `v*.*.*` tag pushes; they never report a status on PRs, so requiring them would lock every PR out of merging. (Note: `ci.yml` no longer has a `docker-publish` job — Docker Hub publishing is now release-only per "version stricte" policy.)
5. **Require conversation resolution before merging:** ON (optional, nice-to-have).
6. **Require signed commits:** OFF (not enforced; opt-in only).
7. **Require linear history:** ON (prevents merge bubbles on `main`).
8. **Include administrators:** ON — so the maintainer cannot accidentally bypass.
9. **Allow force pushes:** OFF
10. **Allow deletions:** OFF

### Rationale — no required reviewers

Guy is the sole maintainer. Requiring a review would force either a second GitHub account or the "admin override" button on every merge — pure friction with no protection gain. The 3 required gates + "Include administrators" already enforce "no unreviewed code ships without passing CI". Revisit this setting as soon as a second committer joins.

### Default branch

Navigate to **Settings → General → Default branch**. Confirm `main` is selected. If `master` is still listed, click the switch icon and change it to `main`.

## Release procedure

1. **Bump `Cargo.toml`:**
   ```toml
   [package]
   version = "X.Y.Z"
   ```
2. **Update `Cargo.lock`:** `cargo build` (the lockfile stamp is refreshed).
3. **Commit and open a PR:**
   ```bash
   git checkout -b release/vX.Y.Z
   git add Cargo.toml Cargo.lock
   git commit -m "Release vX.Y.Z"
   git push -u origin release/vX.Y.Z
   ```
4. **Merge to `main`** after the 3 gates are green.
5. **Tag and push:**
   ```bash
   git checkout main && git pull
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   git push origin vX.Y.Z
   ```
6. **Observe `release.yml`** at https://github.com/guycorbaz/mybibli/actions. On success, `gcorbaz/mybibli:X.Y.Z` and `gcorbaz/mybibli:latest` are pushed to Docker Hub.

The tag regex is `^v[0-9]+\.[0-9]+\.[0-9]+$` — pre-release suffixes (`v1.0.0-rc1`, `v1.0.0-beta`) are rejected in v1. Add them in a future story if release-candidate flow is needed.

## Retrieving artifacts from a failed run

1. Open the failing run: **Actions** tab → click the workflow run.
2. Scroll to the bottom of the summary page.
3. **Artifacts** section lists:
   - `playwright-report-<run-id>` — unzip, open `playwright-report/index.html` in a browser for embedded screenshots, DOM snapshots, and traces.
   - `compose-logs-<run-id>` — full `docker compose logs` output.
   - `mariadb-logs-<run-id>` — last 200 lines of the MariaDB service container.
4. Artifacts are retained for **7 days** (`retention-days: 7`).

## Known gotchas

- **Service-container DNS:** the `db-integration` job reaches MariaDB at `127.0.0.1:3307` (GitHub maps service ports to the host), NOT via a service hostname. The `e2e` job runs its own `docker compose` stack where containers resolve `db` / `mock-metadata` via Compose DNS.
- **Mock-metadata warmup:** `--wait` blocks until all Compose healthchecks pass. The mock-metadata service has no healthcheck (pure `python server.py`), so it's gated by the app's own readiness probing. If tests fail on mock-metadata timeouts, add a `healthcheck` to the mock-metadata service in `tests/e2e/docker-compose.test.yml`.
- **Playwright browser install on first run:** adds ~90s on a cold cache. The `~/.cache/ms-playwright` cache key is fingerprinted against `tests/e2e/package-lock.json`; changes to the Playwright version bust the cache.
- **Migration bootstrap:** the app auto-runs `sqlx::migrate!("./migrations")` at boot (`src/main.rs:41`), so no explicit `sqlx migrate run` step is needed after `docker compose up --wait`.
- **GitHub Actions reusable-workflow check names:** the reported check is `<parent-job-id> / <called-job-name>`, not just `<called-job-name>`. Configure branch protection against the composite name.
- **Secrets + reusable workflows:** reusable workflows do not auto-inherit secrets. The current `_gates.yml` does not use any secrets, so this is a non-issue — but if a future gate needs one, pass `secrets: inherit` on the calling `uses:` block in `ci.yml` / `release.yml`.

## Timing baselines

_To be filled during Task 5.4 smoke test (recorded from the first green `main` PR)._

| Gate             | First cold run | Steady-state (cache warm) |
|------------------|----------------|---------------------------|
| `rust-tests`     | TBD            | TBD                       |
| `db-integration` | TBD            | TBD                       |
| `e2e`            | TBD            | TBD                       |
| `release.publish` | TBD            | TBD                       |

Future stories that add CI steps should measure their overhead against these numbers and justify any regression > 30 s.

## What to do when a gate fails

- **`rust-tests` clippy fail:** new warning in a PR-introduced file. Fix locally, re-push.
- **`rust-tests` sqlx-prepare fail:** schema-migration drift between `.sqlx/` and the live DB. Run `cargo sqlx prepare` locally, commit `.sqlx/`, re-push.
- **`db-integration` fail:** open the `mariadb-logs-<run-id>` artifact first — deadlock SQLSTATE 40001 is a known flake under concurrency (see `src/services/loans.rs` for the retry precedent from story 5-1c).
- **`e2e` fail:** open `playwright-report-<run-id>`, browse the failing spec's trace. If the root cause is a race or flake, check `compose-logs-<run-id>` for app-side errors. If it's a deliberate assertion change, fix the spec.
