# Contributing to mybibli

Thank you for looking. Before anything else, the honest framing:

**mybibli is a personal project.** It was written by one person to catalog one
household's library, and it is published under AGPL-3.0-or-later because software like
this is more useful shared than hoarded — not because it is seeking a team. There is no
roadmap commitment, no support obligation, and no service-level agreement. Issues may
sit for weeks. A pull request may be declined for reasons of taste.

None of that is a reason to stay away. It is a reason to talk before you build.

## The most useful things you can do

**In rough order of usefulness to this project:**

1. **Tell us it broke.** A bug report with a version number and a way to reproduce is worth more than a patch that guesses. → [Bug Report](https://github.com/guycorbaz/mybibli/issues/new?template=bug_report.yml)
2. **Report a security defect — privately.** See [SECURITY.md](SECURITY.md). Never in a public issue.
3. **Tell us what you catalog.** mybibli was shaped by one collection. Media types, series conventions, and cataloging habits it has never met are genuinely interesting. → [Change Request](https://github.com/guycorbaz/mybibli/issues/new?template=change_request.yml)
4. **Improve the translations.** The UI is French and English. Wrong, awkward, or missing strings are easy to fix and immediately visible.
5. **Send code** — after an issue exists and has been discussed.

Questions that are not defects belong in
[Discussions](https://github.com/guycorbaz/mybibli/discussions), not in the issue
tracker.

## Filing an issue

Blank issues are disabled on purpose. Four templates cover what this tracker holds:

| Template | For |
| --- | --- |
| 🐛 **Bug Report** | a defect or regression in the app |
| 🔀 **Change Request** | a change to requirements, architecture, or implementation |
| ⚠️ **Known Failure** | a known, non-blocking quirk being recorded as accepted |
| 🔍 **Code Review Finding** | a deferred finding from an adversarial review (internal use) |

For a bug, always include the **version** — it is printed in the log at startup, along
with the build commit — and say whether you run the Docker image or a source build.

## Before you write code

**Open an issue first, and wait for a reply.** This is not bureaucracy; it is the
cheapest way to avoid throwing your evening away. A change that conflicts with the
architecture, or that expands the product beyond a single household, will be declined —
and it is much better to learn that in a paragraph than in a diff.

Two shapes of contribution are declined by default, so that you do not discover it late:

- **Multi-tenancy.** mybibli is single-tenant by construction: one household, one catalog, admins trusted with everything. See `CLAUDE.md` § Single-tenant.
- **A frontend framework.** Server-rendered Askama templates plus HTMX, with a strict CSP and zero inline scripts or styles, is a design decision and not an accident.

## Development setup

Full instructions are in the [README](README.md#development). The short version:

```bash
# Prerequisites: Docker + Compose, the Rust toolchain (2024 edition), Node.js 20+

# Full stack — app, MariaDB, mock metadata providers
cd tests/e2e && docker compose -f docker-compose.test.yml up --build
# The app listens on http://localhost:8080
```

Checks, in the order CI runs them:

```bash
cargo check                          # fast type-check
cargo clippy -- -D warnings          # zero-warnings policy — CI fails on any warning
SQLX_OFFLINE=true cargo test --lib   # ~525 unit tests, a few seconds
cargo sqlx prepare --check --workspace -- --all-targets   # offline query cache in sync
```

Database integration tests and the two Playwright E2E lanes are described in the README;
each integration test gets a fresh database through
`#[sqlx::test(migrations = "./migrations")]`.

`CLAUDE.md` at the repository root carries the coding standards, the architecture
overview, and the app's known quirks. It is long, and it is the file to read before
touching anything structural.

## House rules for a patch

- **English everywhere in the source** — code, comments, commit messages, test names. The *user interface* is bilingual; the repository is not.
- **No hard-coded user-facing strings.** Every label goes through `rust-i18n`, with both the French and the English translation supplied. A patch that adds an English string only is incomplete.
- **No inline scripts or styles.** The CSP is `script-src 'self'` / `style-src 'self'` with no `unsafe-inline` and no `unsafe-eval`. Behaviour goes in `static/js/`, styling in Tailwind classes.
- **Run `cargo sqlx prepare` after any query change** and commit the resulting `.sqlx/`.
- **Tests come with the change.** A bug fix carries the test that fails without it. A feature carries unit tests, and an E2E spec if it has a reachable surface.
- **Accessibility is gated in CI.** An axe-core job covers every reachable page; WCAG 2.2 AA is the target, not a stretch goal.
- **Keep `sprint-status.yaml` out of your PR.** That file is the internal source of truth for the development plan and is only edited by the story that transits.

## Pull requests

Work from a fork, on a branch off `main`. (The `story/N-M-slug` convention you will see
in the history is the maintainer's internal workflow; you are not expected to follow
it.)

Commit messages follow the repository's existing form — a type, the issue number, and a
sentence in the imperative:

```
fix(#470): serialise the language-toggle describe, and fix what that exposed
feat(#443): apply labels to titles and volumes
```

Before you open the PR:

- [ ] `cargo clippy -- -D warnings` is clean
- [ ] `SQLX_OFFLINE=true cargo test --lib` passes
- [ ] `.sqlx/` is regenerated if you touched a query
- [ ] both `en` and `fr` translations exist for any new string
- [ ] the PR body names the issue it closes and says what you tested

CI must be green before a merge, and it is not bypassed. Expect review comments — the
codebase has strong conventions and they are enforced kindly but consistently.

## Licensing of contributions

mybibli is licensed **AGPL-3.0-or-later**. By submitting a pull request you agree that
your contribution is licensed under the same terms. There is no CLA and no copyright
assignment: your commits stay yours in the history.

Note what the AGPL asks of *operators*, not just of distributors: if you run a modified
version and let other people use it over a network, they are entitled to your source.

## Conduct

Everyone taking part is expected to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).
