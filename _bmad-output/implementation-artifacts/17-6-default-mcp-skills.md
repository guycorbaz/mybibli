# Story 17.6: Default MCP skills

Status: done

GitHub issue: #656 (type:change-request; supersedes #654, whose comment holds the four skill texts). Covers: FR177, UX-DR44. Epic 17; ships in v1.27.0.

## Story

As an admin,
I want mybibli to come with a few skills,
so that I can use them as they are or as models for my own.

## Decisions

- **Project lead, 2026-11-09:** defaults are inserted **inactive**; four small independent skills (`comics-covers`, `comics-series-create`, `comics-series-attach`, `comics-title-spelling`), texts from #654.
- **D1 — versioned files, embedded.** Each body lives in `skills/defaults/<name>.md`, exactly as written in #654, embedded with `include_str!`; the one-line description sits beside it in the table `services::default_skills::DEFAULT_SKILLS` (the "Purpose" column of #656, as a sentence). The files double as examples for writing other skills. The Dockerfile's builder stage copies `skills/`.
- **D2 — once per database, per skill.** At startup (after migrations, non-fatal like the other one-shot steps), each default goes through `data_repairs::run_once` with its own marker `default-skill:<name>`: a default the admin deletes (or purges) never comes back, and a default added by a later release is still inserted on an existing database.
- **D3 — never overwrite.** A name already held by any skill, live or in the Trash, is skipped with an `info` log naming it; the marker is written anyway (the default was offered once, so the log does not repeat on every boot). The insert is a plain `INSERT` whose duplicate-key error is the "taken" signal — no locking read on `mcp_skills` (Rule #22).
- **D4 — "provided by mybibli".** New column `mcp_skills.provided_by_mybibli BOOLEAN NOT NULL DEFAULT FALSE`, set only by the default insertion and cleared by every admin write (`update`, `set_active`), so it means "last change: mybibli itself". Admin → Skills shows "provided by mybibli" in the last-change cell instead of "author unknown" while it is set. A Trash restore leaves it as it was.
- **D5 — a later release never updates a default automatically.** Once inserted, the row belongs to the admin. An improved text ships as the new version of the file in the repository and is announced in the release notes; the admin copies it if they want it. Why: an automatic update needs a fingerprint of every shipped text and a rule for each edited/unedited/deactivated case, for a handful of short texts an admin can paste in a minute; the marker per skill keeps "inserted once" simple and predictable. A *new* default in a later release is inserted (new marker). Revisit only if defaults grow numerous or long.
- **D6 — validation like any skill.** Each default goes through `services::mcp_skills::validate`; a unit test validates every embedded file. At startup an invalid default (impossible once the test passes) is logged as an error and skipped without a marker.

## Acceptance Criteria

1. **AC1.** On a fresh database, startup inserts the four defaults, inactive, `updated_by` NULL, `provided_by_mybibli` set; the agent is not served them (`prompts/list`, `get_skill`, instructions).
2. **AC2.** A name already held by a live or trashed skill is skipped with an `info` log; the admin's text, state and author are untouched.
3. **AC3.** A default deleted (Trash or purged) is not re-created on the next startup; running the startup step twice inserts nothing new.
4. **AC4.** Admin → Skills shows "provided by mybibli" for an untouched default, in en/fr/de/it; an admin edit or (de)activation replaces it with the admin's name.
5. **AC5.** Every embedded default passes `services::mcp_skills` validation (unit test).
6. **AC6.** D5 is documented here and in the module docs.
7. **AC7.** Unit (`default_skills`, `admin_skills` row label), DB integration (`tests/default_skills.rs`: fresh DB, name taken live / in Trash, deleted then restarted, edit clears the mark, not served while inactive), E2E (`default-skills.spec.ts`: the four defaults listed inactive with the mention, body readable, not served over MCP).

## Tasks

- [x] Migration: `mcp_skills.provided_by_mybibli`.
- [x] `skills/defaults/*.md` (four texts from #654); Dockerfile `COPY skills/`.
- [x] `src/services/default_skills.rs`: table, `insert_defaults(pool)`, unit tests.
- [x] `McpSkillModel::insert_default_in`; `update` / `set_active` clear the mark; `McpSkill.provided_by_mybibli`.
- [x] `main.rs` startup call (non-fatal).
- [x] `admin_skills.rs` last-change label; `admin.skills.last_change_provided` in four locales.
- [x] `cargo sqlx prepare` if typed queries change; DB integration; E2E.

## Dev Notes

- One-shot pattern: `services::data_repairs::run_once` (marker check `FOR UPDATE` on the `data_repairs` PK, repair + marker in one transaction).
- Skills model `src/models/mcp_skill.rs` (dynamic `sqlx::query`, no offline cache entry); validation `src/services/mcp_skills.rs`; admin UI `src/routes/admin_skills.rs`, `templates/fragments/admin_skills_list.html`.
- DB integration tests use fresh databases and never run `main`, so existing skill tests keep empty lists.
- E2E: the stack's database is fresh after `e2e-reset.sh`, so the defaults exist; the spec only reads them (Rule #23: no other spec asserts an empty skills list).

## Dev Agent Record

### File List

- `skills/defaults/{comics-covers,comics-series-attach,comics-series-create,comics-title-spelling}.md` — the four bodies of #654, as written.
- `src/services/default_skills.rs` — `DEFAULT_SKILLS`, `insert_defaults` (one `run_once` per default), unit tests (validation, markers, tools named are registered).
- `src/models/mcp_skill.rs` — `provided_by_mybibli` read; `insert_default_in` (1062 = name taken); `update` / `set_active` clear the mark.
- `migrations/20261110090000_mcp_skills_provided_by_mybibli.sql` — the column.
- `src/main.rs` — startup call, non-fatal.
- `src/routes/admin_skills.rs` — "provided by mybibli" in the last-change cell + unit test; `locales/{en,fr,de,it}.yml` — `admin.skills.last_change_provided`.
- `src/mcp/mod.rs` — `tools` is `pub(crate)` (the unit test reads the registry).
- `Dockerfile` — `COPY skills/` in the builder stage; `CLAUDE.md` — pattern paragraph.
- `tests/default_skills.rs` — DB integration; `tests/e2e/specs/journeys/default-skills.spec.ts` — E2E (spec id `DS`).

### Review

- **Round 1** (Blind Hunter, Edge Case Hunter, Acceptance Auditor). Medium, fixed: the story file was not updated after dev. Medium, dismissed: the skill files "missing" from the diff (excluded from the review diff on purpose; tracked) and a `.dockerignore` risk (the repo has none; both build contexts include `skills/`); "existing specs may assert an empty skills list" (checked: none does; the skills specs pass on the new baseline). Medium, kept as designed and deferred: activation clears the mention, so a stock default reads "by <admin>" once activated — the column is "last change", D4; recorded in #665 for the day a later release changes a default text. Low, fixed: two descriptions now follow #656 ("titles"); only a duplicate entry (1062) counts as "name taken", not the whole 23000 class; the lock comment states the shared lock a duplicate insert takes; a unit test checks every tool a default names is registered; the body test is scoped to the comic defaults; the edit and deactivation run through the admin routes with the admin as author, the admin's author is checked untouched on a skip, and a Trash restore keeps the mention (tests). Low, deferred to #665: concurrent boots, restore date credited to mybibli, E2E on a non-reset local stack.

- **Round 2** (combined adversarial + edge pass): round-1 fixes verified (1062 only; every tool and argument the defaults name exists); no Medium+. Low: sprint-status line aligned at story close; the report has no `failed` field (a non-1062 error is only logged, no marker — correct by reading, not locked by a test; left as is). Story clean (Rule #6).

### Gates (final code)

`cargo clippy --all-targets -- -D warnings` clean; full `cargo test` (unit + DB integration, `locale_parity` included) green; `cargo sqlx prepare --check` clean (dynamic queries only); `waitForTimeout` gate clean; full Playwright suite `CI=true --workers=2`: 464 passed, 2 skipped.

### Change Log

- 2026-11-10: implementation, review rounds 1–2, done.
