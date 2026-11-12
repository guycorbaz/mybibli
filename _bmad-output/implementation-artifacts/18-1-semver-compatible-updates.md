# Story 18.1: Semver-compatible updates

Status: review

No dedicated GitHub issue (dependency review of 2026-11-11). Covers: maintenance. Epic 18; ships in v1.28.0. Sprint-status key: `18-1-semver-compatible-updates`.

## Story

As the maintainer,
I want every dependency on its latest compatible version,
so that fixes published upstream reach mybibli without a code change.

## Context

Dependency review of 2026-11-11 (`cargo outdated --root-deps-only`, `npm outdated`, `cargo audit`): no vulnerability, 13 informational warnings, semver-compatible updates not taken, CI still on Node 20 (end of life April 2026). This story takes **only** what moves without a major version change of a library mybibli calls directly. Majors (sqlx 0.9, rust-i18n 4, askama 0.16, reqwest 0.13, axum-extra 0.12, tower-http 0.7, rand 0.10, argon2 0.6 / sha2 0.11, nix 0.31, base64 0.23, TypeScript 7, htmx 4) are **out** — their own epic later (htmx 4 ≈ February 2027). Build-image pinning is story 18-2; rmcp is story 18-3.

## Decisions

- **D1 — `cargo update`, nothing in `Cargo.toml`.** Version requirements in `Cargo.toml` are not raised (they already allow the new versions); only `Cargo.lock` moves. `rmcp = "=3.8.0"` stays (18-3). Dry run on 2026-11-12: 171 packages, among them `tokio` 1.52.3 → 1.54.1, `hyper` 1.9.0 → 1.12.0, `anyhow` 1.0.102 → 1.0.104, `rustix` 1.1.4 → 1.1.6, `event-listener` 5.4.1 → 5.4.2, `spin` 0.9.8 → 0.9.9, `openssl` 0.10.80 → 0.10.82, `serde` 1.0.229 — expected to clear the four "unsound" warnings (RUSTSEC-2026-0386 tokio, -0190 anyhow, -0486 rustix, -0221 event-listener) and the yanked `spin`. New transitive crates appear (`multiversion`, `core_detect`, `miniz_oxide` 0.9…): note in the story file which direct dependency pulls them in.
- **D2 — the warnings that remain are listed, not fixed.** After the update, `cargo audit` output is pasted in Dev Agent Record with, for each remaining warning, the crate that pulls it in (`cargo tree -i <crate>`). Expected leftovers: `genpdf` family (`rusttype`, `stb_truetype`, `lzw`, `stdweb`?), `encoding`, `paste`, `basic-toml`, `unsafe-libyaml` — each goes to the major-upgrade epic or the `genpdf` change request; none is silenced in `.cargo/audit.toml` by this story. The two existing ignores stay (18-3 handles RUSTSEC-2026-0462).
- **D3 — npm.** Root `package.json`: `tailwindcss` and `@tailwindcss/cli` → latest 4.x (4.3.5 on 2026-11-11), range raised to `^4.3.5`, lockfile regenerated with `npm install`. `tests/e2e/package.json`: `@playwright/test` → latest 1.x (1.65.0), `@axe-core/playwright` → latest 4.x (4.14.0), `@types/node` → `^24` (follows CI Node, D5), `typescript` **stays 5.9.3** (last 5.x; 7.0 is a major). Ranges raised to the installed versions so the manifest says what is tested.
- **D4 — vendored browser files, from npm, verified.** Never fetched from a CDN by hand:
  - Chart.js 4.4.7 → **4.5.1**: `npm pack chart.js@4.5.1` in a fresh empty scratch directory (npm checks the tarball integrity), extract, copy `package/dist/chart.umd.js` (or `chart.umd.min.js` if that is the minified build in 4.5.x — check which file the current one corresponds to: its header says jsDelivr served `dist/chart.umd.js` unminified-because-already-minified) to `static/js/vendor/chart.umd.js`. Update the version in the comment at `static/js/home-stats-donut.js:10`.
  - htmx 2.0.8 → **2.0.11** (latest 2.x on 2026-11-12): same procedure with `htmx.org@2.0.11`, file `package/dist/htmx.min.js` → `static/js/htmx.min.js`. Read the htmx 2.0.9–2.0.11 release notes (GitHub releases of `bigskysoftware/htmx`) and record anything touching what mybibli relies on: `htmx:configRequest` / `htmx:beforeSwap` (`csrf.js`, `modal.js`), `HX-Trigger` / `HX-Retarget` / `HX-Reswap`, OOB swaps, `hx-push-url`, `includeIndicatorStyles`, the indicator CSS copied into `static/css/browse.css:113-125` ("same values htmx 2.x uses internally — verified against htmx.min.js source": re-verify against the new file).
  - Both files are third-party code: diff size and header checked, no other file in `static/js/` changed by this step.
- **D5 — CI Node 20 → 24.** `_gates.yml` lines ~164-167 and ~271-274 (`node-version: "20"`, step names "Setup Node.js 20"). Node 24 is the current LTS. The Dockerfile `node:alpine` stays as is here (pinned in 18-2).
- **D6 — Playwright and the `ubuntu-24.04` pin.** `_gates.yml:156-159` pins the two Playwright jobs to `ubuntu-24.04` because Playwright 1.59 could not install Chromium on `ubuntu-latest` = 26.04. After the upgrade to 1.65, check in Playwright's release notes whether 26.04 is supported. If it is, the pin and its comment are removed **only if** the decision is to follow `ubuntu-latest`; otherwise keep the pin and update the comment to say why (story 18-2 is about pinning, so keeping an explicit runner is acceptable — record the choice).
- **D7 — GitHub Actions: minor/patch taken, majors read first.** Latest releases on 2026-11-12 (keep SHA pin + `# vX.Y.Z` comment, update every occurrence in `_gates.yml`, `audit.yml`, `release.yml`, `pages.yml`):

  | Action | Current | Latest | Kind |
  |---|---|---|---|
  | actions/checkout | v6.1.0 | v7.0.1 | **major** |
  | actions/setup-node | v6.5.0 | v7.1.0 | **major** |
  | actions/cache | v5.1.0 | v6.1.0 | **major** |
  | actions/upload-artifact | v7.0.1 | v7.0.2 | patch |
  | actions/deploy-pages | v5.0.0 | v5.0.1 | patch |
  | actions/configure-pages, upload-pages-artifact | v6.0.0, v5.0.0 | same | — |
  | docker/build-push-action | v7.3.0 | v7.4.0 | minor |
  | docker/login-action | v4.6.0 | v4.8.0 | minor |
  | docker/setup-buildx-action | v4.2.0 | v4.4.1 | minor |
  | docker/metadata-action, peter-evans/dockerhub-description | v6.2.0, v5.0.0 | same | — |
  | Swatinem/rust-cache | v2.9.2 | v2.9.3 | patch |
  | dtolnay/rust-toolchain | `stable` branch @ 2026-08-18 | current `stable` branch head | refresh SHA + date comment |

  Resolve each SHA with `gh api repos/<owner>/<repo>/git/ref/tags/<tag>` (dereference annotated tags to the commit). For the three majors, read the release notes; take them unless a note names a breaking change that affects how this repo uses them (inputs used: `checkout` default, plus `fetch-depth: 0` in `release.yml:68`; `setup-node` `node-version`/`cache`/`cache-dependency-path`, `cache` `path`/`key`) — record the reading. `release.yml` and `pages.yml` only run on tag / pages events: their changes are verified at the v1.28.0 release, say so in the story file.

## Acceptance Criteria

1. **AC1 — Rust.** `Cargo.lock` is the result of `cargo update` (D1); `Cargo.toml` unchanged; `cargo check`, `cargo clippy --all-targets -- -D warnings`, `cargo sqlx prepare --check --workspace -- --all-targets` pass.
2. **AC2 — Audit.** `cargo audit` still reports no vulnerability; the remaining warnings are listed in Dev Agent Record with the crate that pulls each in (D2); `.cargo/audit.toml` unchanged.
3. **AC3 — npm.** Root and `tests/e2e` manifests and lockfiles updated per D3; `npx tsc --noEmit` in `tests/e2e` passes; the Tailwind build (`npx @tailwindcss/cli -i static/css/input.css -o static/css/output.css`) succeeds and the generated CSS diff is skimmed for removed utilities (record anything surprising).
4. **AC4 — Vendored files.** `static/js/vendor/chart.umd.js` is Chart.js 4.5.1 and `static/js/htmx.min.js` is htmx 2.0.11, both taken from `npm pack` (D4); version comments updated; the htmx release-note reading and the `browse.css` indicator re-check are recorded.
5. **AC5 — CI.** `_gates.yml` uses Node 24; the Actions are updated per D7 with SHA + version comment everywhere; the `ubuntu-24.04` pin is decided per D6 and its comment is accurate.
6. **AC6 — No behaviour change.** No source change in `src/`, `templates/`, `locales/` except if a compatible update forces one (a new clippy lint, a deprecation): each such change is listed with its reason. No test is weakened or skipped.
7. **AC7 — Gates.** Targeted gates between iterations; **once, on the final code**: full `mem-guard cargo test`, DB integration tests (`tests/docker-compose.rust-test.yml`), `./scripts/e2e-reset.sh` then `CI=true npx playwright test --workers=2` (under `mem-guard`), all green. A manual visual check of the dashboard donut (Chart.js), a modal (htmx + scanner guard), the sidebar collapsed / expanded and the dark theme (Tailwind) is recorded in Dev Agent Record.

## Tasks / Subtasks

- [x] T1 — Rust (AC1, AC2, AC6)
  - [x] `cargo update`; `cargo check`; `cargo clippy --all-targets -- -D warnings` (fix any new lint at its root, list it)
  - [x] `cargo sqlx prepare --check --workspace -- --all-targets`
  - [x] `cargo audit` → table of remaining warnings + `cargo tree -i` origin
- [x] T2 — npm (AC3)
  - [x] root: Tailwind 4.3.x, range `^4.3.5`, `npm install`; rebuild CSS, skim diff
  - [x] `tests/e2e`: Playwright, axe, `@types/node@^24`, `npm install`; `npx playwright install chromium`; `npx tsc --noEmit`
- [x] T3 — Vendored files (AC4)
  - [x] Chart.js 4.5.1 via `npm pack` in an empty scratch dir; copy; update comment in `home-stats-donut.js`
  - [x] htmx 2.0.11 via `npm pack`; copy; read 2.0.9–2.0.11 notes; re-verify `browse.css` indicator block
- [x] T4 — CI (AC5)
  - [x] Node 24 in `_gates.yml` (both jobs, step names too)
  - [x] Actions per D7 in all four workflows; majors' release notes read and recorded
  - [x] `ubuntu-24.04` pin decided (D6)
- [x] T5 — Gates (AC7)
  - [x] full unit + DB integration + E2E (2 workers) once on the final code
  - [x] manual visual check recorded

## Dev Notes

### Constraints that bite

- **Memory guard (CLAUDE.md, station memory rules):** `free -g` ≥ 8 GB available before any heavy command, one heavy job at a time, `mem-guard` on full `cargo test`, `cargo clippy --all-targets`, `./scripts/e2e-reset.sh`, Playwright. A fresh `cargo update` invalidates most of `target/`: the first build is heavy → under `mem-guard`.
- **No formatter.** Never run `cargo fmt` (it reformats the whole repository, even with a file argument). Not expected to be needed here.
- **One push per story,** after review and full local gates; no draft PR at story start (project lead's instruction of 2026-10-29, overrides Rule #15). Commit after each BMAD step (Rule #10).
- **E2E stack on port 18080** locally (port 8080 is taken on the workstation); `SQLX_OFFLINE=true` for `cargo check` without a DB.
- **Flake history:** #554 and #499 are known intermittent E2E failures. If one shows up, re-run that spec alone before suspecting the upgrade; record it.

### Where versions live (inventory for this story)

| What | File(s) |
|---|---|
| Rust crates | `Cargo.toml` (requirements), `Cargo.lock` |
| Tailwind | `package.json`, `package-lock.json`; Dockerfile `css` stage installs **unpinned** (`npm install tailwindcss @tailwindcss/cli`) — fixed in 18-2 (#506), not here. Consequence: the E2E stack (built from the Dockerfile) already compiles CSS with the latest Tailwind, while a local `npx @tailwindcss/cli` uses the lockfile; after this story both are 4.3.x |
| Playwright, axe, TypeScript, @types/node | `tests/e2e/package.json`, `tests/e2e/package-lock.json` ; the CI browser cache key hashes this lockfile, so the first CI run re-downloads Chromium (expected) |
| Chart.js | `static/js/vendor/chart.umd.js` (+ comment `static/js/home-stats-donut.js:10`), loaded by `templates/pages/home.html:811` |
| htmx | `static/js/htmx.min.js`, loaded by `templates/layouts/base.html:28`, `templates/layouts/bare.html:13`; excluded from Tailwind scan in `static/css/input.css:13`; indicator CSS mirrored in `static/css/browse.css:113-125` |
| Node (CI) | `.github/workflows/_gates.yml` (two Playwright jobs) |
| Actions | `.github/workflows/{_gates,audit,release,pages}.yml` (SHA + `# vX` comment) |
| Rust toolchain (CI) | `dtolnay/rust-toolchain` with `toolchain: stable` (pinning = 18-2) |
| MariaDB, Python mock | compose files — **out of scope** (database question on hold) |

### htmx surface to watch (from CLAUDE.md Key Patterns)

CSRF token via `htmx:configRequest` (`static/js/csrf.js`) and the `csrf-rejected` `htmx:beforeSwap` override; modal lifecycle (`modal.js`, `inline-form.js`, `HX-Trigger: modal-close`, `X-Modal-Confirm`); `HX-Retarget` / `HX-Reswap` on every `AppError`; OOB swaps (`HtmxResponse`, pending-updates middleware); `hx-push-url` on admin tabs; `scan-field.js` custom events; scanner guard. The E2E suite covers all of these; a patch release should not change them, but read the notes.

### Previous story intelligence (epic 17)

- 17-4 lost a push to the E2E typecheck → `npx tsc --noEmit` is part of AC3 (retro A1, now in CLAUDE.md).
- The truth gate for E2E is `CI=true npx playwright test --workers=2` (Rule #13 item 4).
- The `ubuntu-24.04` pin came from the v1.27.0 cycle (commit `46c82e5`, "ci: pin Playwright jobs to ubuntu-24.04").

### References

- `_bmad-output/planning-artifacts/epics.md` § Epic 18, Story 18.1
- `.github/workflows/_gates.yml:155-225` (Playwright job, Node, pin comment)
- `.cargo/audit.toml` (ignore policy)
- Memory notes: dependency review 2026-11-11 (versions table above)

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (`claude-opus-5-5`)

### Debug Log References

- `cargo update`: 171 packages; `Cargo.toml` unchanged. New transitive crates (`multiversion`, `core_detect`, `miniz_oxide` 0.9) come from `image` → `exr` 1.74.
- First `cargo test --lib` run: 197 failures, all `#[sqlx::test]` connecting to the `.env` `DATABASE_URL` (port 3306 = another project's MariaDB). Re-run against `tests/docker-compose.rust-test.yml` (3307): green.
- `tests/e2e/node_modules` was partly owned by `root` (an April `npm install` from Docker): `npm install` failed with EACCES. Moved aside to `~/.cache/mybibli-e2e-node_modules-root-old-2026-11-12` (not deleted; needs `sudo rm -rf` to remove) and reinstalled.

### Completion Notes List

**T1 — Rust.** `cargo update` only. `cargo clippy --all-targets -- -D warnings` clean with no source change (AC6: no file under `src/`, `templates/`, `locales/` changed by the update itself — the one forced change is below). `cargo sqlx prepare --check --workspace -- --all-targets` passes against the migrated test DB.

`cargo audit` after the update — no vulnerability; the four *unsound* warnings (tokio RUSTSEC-2026-0386, anyhow -0190, rustix -0486, event-listener -0221) and the yanked `spin` are gone. Remaining (8, all *unmaintained*, none silenced):

| Crate | Advisory | Pulled in by | Where it goes |
|---|---|---|---|
| `basic-toml` | RUSTSEC-2026-0374 | `askama` 0.15 (derive) | askama 0.16 (major-upgrade epic) |
| `encoding` | RUSTSEC-2021-0153 | `genpdf` → `printpdf` → `lopdf` | `genpdf` replacement CR |
| `lzw` | RUSTSEC-2020-0144 | `genpdf` → `printpdf` → `lopdf` | `genpdf` replacement CR |
| `rusttype` | RUSTSEC-2021-0140 | `genpdf`, `printpdf` | `genpdf` replacement CR |
| `stb_truetype` | RUSTSEC-2020-0020 | `genpdf` → `rusttype` | `genpdf` replacement CR |
| `stdweb` | RUSTSEC-2020-0056 | no edge on the host target (wasm-only branch of a dependency) | — |
| `paste` | RUSTSEC-2024-0436 | `image` → `ravif` → `rav1e` | upstream (`image`) |
| `unsafe-libyaml` | RUSTSEC-2024-0449 | `rust-i18n` 3 (`serde_yaml`), `serde_yaml_ng` (dev) | rust-i18n 4 (major-upgrade epic) |

**T2 — npm.** `tests/e2e`: `@playwright/test` ^1.65.0 (1.65.0), `@axe-core/playwright` ^4.14.0, `@types/node` ^24.19.2, `typescript` range ^5.9.3 (5.9.3, unchanged). `npx tsc --noEmit` passes. Root `package.json` / `package-lock.json` are **git-ignored** (`.gitignore:11`, `:53`): Tailwind was upgraded locally to 4.3.5 but no tracked file records it — the Dockerfile installs whatever is latest. This is exactly #506; handled in 18-2. CSS comparison on the same source, Tailwind 4.2.2 vs 4.3.5 (minified): only candidate artefacts disappear (`.start`, `.end`, `.shadow/md`, `.shadow/sm` — words from the code, no template uses them as classes); `group-open:rotate-90` and `group-hover:opacity-100` are emitted with a rewritten selector of the same meaning (the 11-3 E2E asserting the computed `rotate` covers it).

**T3 — vendored files.** From `npm pack` in an empty scratch directory (tarball integrity checked by npm): `chart.js@4.5.1` `dist/chart.umd.js` (same build as the 4.4.7 file: minified UMD + `sourceMappingURL` line) → `static/js/vendor/chart.umd.js`, SHA-256 `ecc3cd1e…7652a`, comment in `home-stats-donut.js` updated. `htmx.org@2.0.11` `dist/htmx.min.js` → `static/js/htmx.min.js`. htmx CHANGELOG 2.0.9–2.0.11 read: fixes to `HX-Location` replace, history path normalisation, empty `class=""` after utility classes, `hx-disabled-elt` keeping originally-disabled elements, OOB targets with a detached context element, `hasHeader()` substring match; new `<hx-partial>` (unused). None changes what mybibli relies on; the OOB fix can only help. The injected indicator CSS string is byte-identical between 2.0.8 and 2.0.11, so `static/css/browse.css:113-125` still mirrors it.

**T4 — CI.** Node 20 → 24 in both Playwright jobs. Actions per D7, SHA dereferenced from the tag + `# vX.Y.Z` comment, all four workflows: checkout v7.0.1, setup-node v7.1.0, cache v6.1.0 (majors: release notes read — ESM migration and dependency bumps; checkout v7 blocks fork checkout on `pull_request_target` / `workflow_run`, unused here; no input we use changed), upload-artifact v7.0.2, deploy-pages v5.0.1, build-push-action v7.4.0, login-action v4.8.0, setup-buildx-action v4.4.1, rust-cache v2.9.3, rust-toolchain `stable` @ 2026-10-16. `release.yml` / `pages.yml` changes are exercised only at the v1.28.0 release. **D6:** Playwright supports Ubuntu 26.04 since 1.61 → the two Playwright jobs go back to `ubuntu-latest`, as the pin comment asked; all CI jobs now run on the same image.

**Forced change (AC6) — the Proposals nav entry's accessible name.** The first full E2E run (476 tests, 2 workers) had 8 failures, all the same: `@axe-core/playwright` 4.14 applies `label-content-name-mismatch` (WCAG 2.5.3) and flags the Proposals entry whenever a proposal is pending — its `aria-label` "Proposals, 2 pending" did not contain the visible text, which is "Proposals" + the badge with no space ("Proposals2"), or "Proposals99+" past the cap. A real defect the newer axe reveals, not a flake. Probed on a static page with the real CSS, axe 4.14 and Playwright's name computation: an `sr-only` suffix with a leading comma yields "Proposals , 2 pending" (Chromium spaces around the absolutely positioned span); a comma-free suffix yields exactly "Proposals 2 pending" and passes axe. Applied: no `aria-label` on nav entries; `<span class="sr-only">{count} pending</span>` after the label; badge still `aria-hidden`. `utils::proposals_badge_aria` → `proposals_pending_sr`, `NavEntry::aria_label` → `pending_sr`, locale key `nav.proposals_pending_aria` → `nav.proposals_pending_sr` ("%{count} pending" / "en attente" / "ausstehend" / "in attesa", four locales), unit tests in `utils.rs`, `utils/nav.rs`, `routes/nav_sticky_tests.rs` (now also asserting no `aria-label` on the entry), the epic 14 smoke regex, `CLAUDE.md` navigation paragraph. Collapsed, the label span is `sr-only` too, so the name is unchanged.

**T5 — Gates.** Full `cargo test --no-fail-fast` (unit + bins + integration, DB on 3307, `CARGO_BUILD_JOBS=2`, mem-guard): 2722 passed, 0 failed (before the nav change; after it: `cargo clippy --all-targets -D warnings` clean, `--lib` nav / proposals / templates_audit / `--test locale_parity` green). E2E (`./scripts/e2e-reset.sh` on 18080, `CI=true npx playwright test --workers=2`): run 1 — 8 failed (axe rule above); run 2 — 3 failed (helpers still reading the `aria-label`: `helpers/proposals.ts::shownPendingCount`, `sidebar.spec.ts`, `proposals-access.spec.ts` → now `data-proposals-pending` + `toHaveAccessibleName`), 1 flaky (`form-refusals.spec.ts`, passed on retry, untouched code); run 3 on the final code — **476 passed, 2 skipped, 0 failed, 0 flaky**. `npx tsc --noEmit` green.

**Visual check (AC7)** — screenshots from Playwright against the E2E stack, OS dark preference emulated, real Collapse button: dashboard donut renders with Chart.js 4.5.1 (legend, centre text); dark theme cards and chips readable; collapsed sidebar shows the icon rail with pills. Rendering compared with the Tailwind 4.2.2 CSS on the same page: identical. Modals (htmx 2.0.11 + scanner guard) are covered by `admin-modal-lifecycle.spec.ts` and the modal specs, green.

### File List

- `Cargo.lock`
- `src/utils.rs`, `src/utils/nav.rs`, `src/routes/nav_sticky_tests.rs`, `templates/components/nav_entries.html`, `locales/{en,fr,de,it}.yml` (Proposals accessible name)
- `tests/e2e/specs/journeys/epic14-smoke.spec.ts`
- `CLAUDE.md`
- `tests/e2e/package.json`, `tests/e2e/package-lock.json`
- `static/js/vendor/chart.umd.js`, `static/js/htmx.min.js`, `static/js/home-stats-donut.js` (version comment)
- `.github/workflows/_gates.yml`, `audit.yml`, `release.yml`, `pages.yml`
- `_bmad-output/implementation-artifacts/18-1-semver-compatible-updates.md`, `sprint-status.yaml`
