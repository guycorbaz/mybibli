# Story 18.2: Pinned build images

Status: ready-for-dev

GitHub issue: #506 (closes). Covers: maintenance. Epic 18; ships in v1.28.0. Sprint-status key: `18-2-pinned-build-images`.

## Story

As the maintainer,
I want two builds of the same commit to produce the same image,
so that a dependency moves only when a commit says so.

## Context

Story 18-1 found that every version that reaches the production image without being written in a tracked file:

- `Dockerfile` stage images: `rust:alpine`, `node:alpine`, `alpine:latest` — whatever is current at build time.
- Tailwind: the `css` stage runs `npm install tailwindcss @tailwindcss/cli` (no version, #506); the root `package.json` / `package-lock.json` that pin it locally are **git-ignored** (`.gitignore:11` ignores every `package-lock.json` except `tests/e2e/`'s; `.gitignore:53` ignores `/package.json`, since story 1-1).
- Rust toolchain: CI installs `toolchain: stable` (`_gates.yml` ×2, `audit.yml`); local builds use whatever `rustup` default is; no `rust-toolchain.toml`.

Out of scope: the MariaDB image (`mariadb:10.11` — the database question is on hold), the mock metadata server image (`python:3.12-slim`, test-only), the major library upgrades (own epic), rmcp (18-3).

## Decisions

- **D1 — exact image tags, no digests.** Tags carry the version a human reads in review; a digest pin hides it and needs a tool to bump. Versions on 2026-11-12 (Docker Hub): `rust:1.99.1-alpine3.24`, `node:24.22.3-alpine3.24` (Node 24 = CI's, story 18-1), `alpine:3.24.4`. All three stages on the same Alpine minor (3.24) so the musl / OpenSSL static libs of the builder match the runtime family.
- **D2 — one Rust version everywhere: 1.99.1.** A `rust-toolchain.toml` at the root (`channel = "1.99.1"`, `components = ["clippy", "rustfmt"]`, `profile = "minimal"`) pins local builds (rustup honours it in the repository); CI passes `toolchain: 1.99.1` to `dtolnay/rust-toolchain` in `_gates.yml` (both jobs) and `audit.yml`, with the step name and comment saying so; the Dockerfile builder image is the same version (D1). A unit-free guard keeps them aligned: a test (Rust, in `src/templates_audit.rs` or a new `tests/toolchain_pins.rs`) reads `rust-toolchain.toml`, the Dockerfile `FROM rust:` line and the workflow `toolchain:` inputs and fails when they differ. Same test checks the Node major of the Dockerfile `css` stage equals `node-version` in `_gates.yml`.
- **D3 — Tailwind from a tracked manifest.** Track the root `package.json` and `package-lock.json` (remove `/package.json` from `.gitignore`, add `!/package-lock.json` next to the `tests/e2e` exception), versions **exact** (`"tailwindcss": "4.3.5"`, `"@tailwindcss/cli": "4.3.5"`, no caret — the lockfile pins transitive deps, the manifest says what we chose). The `css` stage copies `package.json` + `package-lock.json` and runs `npm ci` before the Tailwind build. Local `npx @tailwindcss/cli` uses the same versions.
- **D4 — inventory in `CLAUDE.md`.** A short "Where versions live" list under Build & Test Commands (Rust toolchain, Docker stages, Tailwind, E2E toolchain, vendored htmx / Chart.js, Actions, MariaDB / mock images and why they are not bumped here), so the next upgrade finds every place.

## Acceptance Criteria

1. **AC1.** The Dockerfile stages use `rust:1.99.1-alpine3.24`, `node:24.22.3-alpine3.24`, `alpine:3.24.4`; no `latest`, no bare `alpine`.
2. **AC2.** `rust-toolchain.toml` pins 1.99.1; CI's `dtolnay/rust-toolchain` steps install 1.99.1 (`_gates.yml` both jobs, `audit.yml`); `cargo --version` in the repository reports 1.99.1.
3. **AC3.** Root `package.json` (exact Tailwind 4.3.5) and `package-lock.json` are tracked; the Dockerfile `css` stage installs with `npm ci` from them; `.gitignore` updated with a comment saying why.
4. **AC4.** A test fails when the Rust version differs between `rust-toolchain.toml`, the Dockerfile and the workflows, or the Node major between the Dockerfile and CI; it passes on the pinned files.
5. **AC5.** `CLAUDE.md` lists where each version lives (D4).
6. **AC6.** The image builds (`docker build`), the E2E stack starts from it, and the full E2E suite passes (`CI=true --workers=2`); clippy `-D warnings` and the unit suite pass under 1.99.1. #506 is closed by the PR.

## Tasks / Subtasks

- [ ] T1 — Dockerfile stages pinned (AC1)
- [ ] T2 — `rust-toolchain.toml`; CI toolchain inputs (AC2)
- [ ] T3 — root `package.json` + lockfile tracked, exact Tailwind; `npm ci` in the `css` stage; `.gitignore` (AC3)
- [ ] T4 — alignment test (AC4)
- [ ] T5 — `CLAUDE.md` inventory (AC5)
- [ ] T6 — gates: clippy, unit, image build, E2E (AC6)

## Dev Notes

- Memory guard: `mem-guard` on clippy `--all-targets`, full `cargo test`, `./scripts/e2e-reset.sh`, Playwright; one heavy job at a time; `CARGO_BUILD_JOBS=2` for the full test run. The first build under a new toolchain rebuilds `target/` from scratch → heavy.
- No formatter (`cargo fmt` reformats the whole repository). `rustfmt` stays in the toolchain components only so editors keep working.
- E2E stack on port 18080 (`E2E_HOST_PORT=18080`); DB integration tests on 3307 (`tests/docker-compose.rust-test.yml`), not the `.env` URL.
- One push per story, after full local gates (project lead, 2026-10-29).
- The `tests/e2e/docker-compose.test.yml` app service builds from the root `Dockerfile` (`build: … dockerfile: Dockerfile`), so the E2E run exercises the pinned image.
- `release.yml` builds the published image from the same Dockerfile: the pins apply to the v1.28.0 image.

### References

- `_bmad-output/planning-artifacts/epics.md` § Story 18.2; issue #506
- Story 18-1 Dev Agent Record (root manifest git-ignored, Tailwind 4.2.2 → 4.3.5 comparison)
- `Dockerfile:1-40`, `.gitignore:9-11, 52-53`, `.github/workflows/_gates.yml:52-59, 121-124`, `.github/workflows/audit.yml:50-53`

## Dev Agent Record

### Agent Model Used

### Completion Notes List

### File List
