# Story 18.7: Documentation

Status: done

No GitHub issue. Covers: maintenance (documentation). Epic 18; ships in v1.28.0. Sprint-status key: `18-7-epic-18-documentation`.

## Story

As an operator,
I want the documentation to match what is built,
so that the next upgrade starts from an accurate picture.

## Acceptance Criteria

1. **AC1.** The API chapter describes the series endpoint as fixed in 18-4, and the versions named in the documentation are current.
2. **AC2.** The PDFs are rebuilt.
3. **AC3.** Release documentation (Rule #19, v1.28.0) is done at release time, not in this story.

## Decisions

- **D1 — what the manual names.** A sweep of `docs/manual/{en,fr}` finds no library or toolchain version (Chart.js, htmx, Tailwind, rmcp, Node, Rust) to update; the only user-visible changes of the epic are the series API (written in 18-4, ch. 11) and error messages now shown on every page (18-5) → a troubleshooting entry, *An action does nothing* / *Une action ne fait rien* (`sec:troubleshoot-action-silent`).
- **D2 — contributor prerequisites.** `CONTRIBUTING.md` and `README.md` said Node.js 20+ and "the Rust toolchain": now rustup with the version pinned in `rust-toolchain.toml` (18-2) and Node.js 24 (18-1, = CI and Dockerfile).
- **D3 — `CLAUDE.md`** was updated story by story (18-1 navigation name, 18-2 "Where versions live", 18-3 rmcp pin, 18-5 feedback region and AppError display); nothing left.
- **D4 — no release documentation** (version bump, *What's new in 1.28.0*, website, Docker Hub page): done with the release (Rule #19). "Since 1.28.0" is written in the body as previous documentation stories did.

## Tasks / Subtasks

- [x] T1 — ch. 10 EN + FR troubleshooting entry (AC1)
- [x] T2 — `CONTRIBUTING.md`, `README.md` prerequisites (AC1)
- [x] T3 — PDFs rebuilt (`docs/manual/build.sh`, under `flock /tmp/station-heavy.lock mem-guard`) (AC2)

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (`claude-opus-5-5`)

### Completion Notes List

- Developed on its own branch from `main` while the 18-5 CI ran (docs only, disjoint from 18-5 / 18-6).
- Build: both PDFs written (EN 100 → 103 pages across passes); no undefined label; the remaining warnings are the pre-existing ones (missing box-drawing glyphs in Latin Modern Mono, infinite glue shrinkage in four tables, font shape substitutions).
- Review inline (docs only): wording checked against the code — the region sits at the top of the content (`layouts/base.html`), the not-found copy is `error.not_found` in EN/FR, the connection-lost overlay is `connection-monitor.js`. No finding.

### File List

- `docs/manual/en/10-troubleshooting.tex`, `docs/manual/fr/10-troubleshooting.tex`
- `docs/manual/mybibli-manual-en.pdf`, `docs/manual/mybibli-manual-fr.pdf` (rebuilt; include 18-4's ch. 11 text)
- `CONTRIBUTING.md`, `README.md`
- `_bmad-output/implementation-artifacts/18-7-epic-18-documentation.md`, `sprint-status.yaml`
