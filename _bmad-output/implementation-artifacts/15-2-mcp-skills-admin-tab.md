# Story 15.2: MCP skills — admin tab

Status: done

GitHub issue: #614 (type:change-request), part 1 of 2. Covers: FR162, UX-DR40. Epic 15; ships in v1.25.0.

## Story

As an administrator,
I want to write the instructions my agent should follow, once, in mybibli,
so that I stop pasting them into every session.

## Decisions (project lead, 2026-11-04 — see #614)

Admin-only editing through a web tab; the agent can only read skills (story 15-3); no version history (current text + date and author of the last change); no proposal kind for skills.

## Code map (starting points)

- Admin tab pattern (story 8-1): `src/routes/admin.rs` `AdminTab` enum (~l. 41: Health, Users, ReferenceData, Trash, System, ApiKeys), `/admin?tab=<name>` + `/admin/<name>` HTMX panel, `session.require_role_with_return(Role::Admin, …)`; other tabs live in their own files (`admin_users.rs`, `admin_api_keys.rs`, `admin_system.rs`, `admin_reference_data.rs` + `admin_labels.rs`) — add `admin_skills.rs` (Rule #12).
- Closest CRUD to imitate: labels (CR #443) — `src/routes/admin_labels.rs`, `src/models/label.rs`; modal patterns (UX-DR8, Pattern B `#admin-modal-slot`, `HX-Trigger: modal-close`), inline-form.js.
- Trash: `services/soft_delete.rs::ALLOWED_TABLES` (+ FK purge ordering, Trash panel listing/restore/permanent delete, auto-purge).
- Optimistic locking: `services/locking.rs::check_update_result`.
- CSP/templates audit, CSRF hidden input on every POST form, i18n four locales, error copy per `docs/error-message-style.md`.

## Acceptance Criteria

1. **AC1 — table.** Migration `mcp_skills`: `id`, `name` VARCHAR(64) unique among live rows (slug `^[a-z0-9]+(-[a-z0-9]+)*$`, ≤ 64), `description` VARCHAR(200) NOT NULL (one line, no newline), `body` MEDIUMTEXT (Markdown, ≤ 64 KiB, not empty), `active` BOOLEAN default TRUE, `updated_by` FK users (nullable, SET NULL), plus `version`, `created_at`, `updated_at`, `deleted_at`. A soft-deleted name can be reused (reactivate or free the name — choose and document; mirror reference-data's `CreateOutcome::Reactivated` if simplest).
2. **AC2 — model/service.** `models/mcp_skill.rs` (list_all incl. inactive for admin, list_active for 15-3, find_by_name_active, create, update with version, set_active, soft delete) and a small service with validation (positive shapes, Rule #20: name slug regex, description single line ≤ 200, body ≤ 64 KiB UTF-8).
3. **AC3 — Admin → Skills tab.** New `AdminTab::Skills` (`/admin?tab=skills`, `/admin/skills`), Admin only (Librarian → 403 FeedbackEntry, Anonymous → login redirect). List: name, description, active state, last change (date in the instance zone — story 15-1 helper) and author. Create / edit form (name, description, Markdown textarea, active) — edit with optimistic locking (conflict → localized error). Deactivate / reactivate toggle. Delete through a UX-DR8 modal → soft delete → appears in Trash, restorable.
4. **AC4 — safety.** The body is displayed escaped (text, e.g. in a `<pre>`/whitespace-preserving block); no unescaped HTML and no Markdown-to-HTML rendering in this story. CSP clean, CSRF on every form.
5. **AC5 — i18n & copy.** All labels in four locales (`locale_parity`); errors follow the style guide.
6. **AC6 — tests.** Unit (validation shapes, accepted/refused names), DB integration (CRUD, uniqueness among live rows, version conflict, soft delete + restore, name reuse), E2E (new specId: admin creates, edits, deactivates, deletes and restores a skill; Librarian refused; axe on the tab and the form). Full suites green.

## Tasks / Subtasks

- [x] AC1 — migration `20261104150000_create_mcp_skills.sql` (table `mcp_skills`).
- [x] AC2 — `src/models/mcp_skill.rs` + `src/services/mcp_skills.rs` (positive-shape validation).
- [x] AC3 — `AdminTab::Skills`, `src/routes/admin_skills.rs`, four templates, Trash integration.
- [x] AC4 — body escaped (`<textarea>`, `<pre>`), no Markdown rendering, CSP / CSRF clean.
- [x] AC5 — `admin.skills.*`, `admin.tabs.skills`, `admin.trash.filter_entity_mcp_skills`, `error.mcp_skills.*` in en/fr/de/it.
- [x] AC6 — unit, DB integration (`tests/mcp_skills.rs`) and E2E (`admin-skills.spec.ts`); full suites run.

## Dev Agent Record

### Completion Notes

**AC1 — table.** `mcp_skills` (`id`, `name` VARCHAR(64), `description`
VARCHAR(200), `body` MEDIUMTEXT, `active` BOOLEAN default TRUE,
`updated_by` → `users(id)` ON DELETE SET NULL, `version`, `created_at`,
`updated_at`, `deleted_at`). **Name reuse — chosen: neither reactivate nor free: refuse** (review M1).
`name` is UNIQUE across all rows (so unique among live rows too).
Creating a skill, or renaming one, onto a name held by a trashed skill is
refused (`NameInTrash`; create copy `name_in_trash_on_create`: "restore it
from the Trash, or choose a different name"). The trashed text is never
overwritten, and a Trash restore can never collide with a live namesake.
No charset/collation pinned: the Trash UNIONs every trashable
table's name column, a different collation would break it.

**AC2 — model / service.** `McpSkillModel::{list_all, find_by_id,
list_active, find_active_by_name, create, update, set_active,
soft_delete}`; every write is version-guarded and returns
`SkillWriteError::{NameTaken, NameInTrash, Stale, Gone, Db}`; `create`
returns the new id.
`list_active` / `find_active_by_name` return `ActiveSkill {name,
description, body}`, live and active only — the two reads story 15-3
needs. `services::mcp_skills::validate` (Rule #20, refuse never rewrite):
name slug `^[a-z0-9]+(-[a-z0-9]+)*$` ≤ 64 (hand-written parser, uppercase
refused not lowercased), description one line (no control character)
≤ 200 characters, body not blank, ≤ 64 KiB of UTF-8, no control
character except `\n` `\r` `\t`. Only name and description are trimmed;
the body is stored exactly as typed.

**AC3 — Admin → Skills.** `/admin?tab=skills`, `/admin/skills` (panel),
`POST /admin/skills` (create), `GET /admin/skills/new`,
`GET /admin/skills/{id}/edit`, `POST /admin/skills/{id}` (save, version),
`POST /admin/skills/{id}/active`, `GET /admin/skills/{id}/delete-modal`,
`POST /admin/skills/{id}/delete`. Every handler starts with
`require_role_with_return(Role::Admin, "/admin?tab=skills")` (Librarian →
403, Anonymous → login). List: name, description (+ the body in a
`<details><pre>`), Active/Inactive chip, last change (date in the
instance zone through `format_local_date_time`, story 15-1) and author.
Editor above the list (create by default, filled by Edit, Cancel back to
create). Refused saves (validation 400, conflicts 409) re-render the
editor with what was typed plus the localized message
(`HX-Trigger: validation-error`). Delete: UX-DR8 macro in `#modal-slot`
→ soft delete → Trash (`mcp_skills` added to `ALLOWED_TABLES`,
`PURGE_DELETION_ORDER`, Trash filter + type chip) → restorable.
Focus (Rule #21): the submit and row buttons keep stable ids, so HTMX
restores focus to them after a swap — covered in E2E.

**AC4 — safety.** Askama escapes the body into the `<textarea>` and the
`<pre>`; no `|safe`, no Markdown renderer. Forms carry `_csrf_token`;
`templates_audit` passes (one fix: the `<pre>` scroller needed
`**:scroll-mt-0!`).

**AC5 — i18n.** Keys in four locales; `locale_parity` green. Field hints
are terse; write errors are tripartite (`name_taken`, `name_in_trash`,
`stale`, `gone`).

**AC6 — tests.** Unit: `services::mcp_skills` (accepted/refused names,
descriptions, bodies, byte limit, translations), `routes::admin_skills`
(escaping in list and textarea, instance-zone date, error copy),
`routes::admin` (tab resolution). DB integration `tests/mcp_skills.rs`
(9): CRUD + active reads, uniqueness, version conflicts, soft delete +
Trash restore, a trashed name refused on create and rename with the trashed
skill intact and restorable, 64 KiB body, auto-purge, role
gating, full route round trip. E2E `admin-skills.spec.ts` (specId SK,
names `sk-<random>`): create/edit/deactivate/delete/restore journey from
a blank browser with axe on the tab and the edit form, refused name, and
Librarian refused.

### Deviations

- `static/js/csrf.js`: the #91 `validation-error` swap opt-in now also
  accepts 409, so a conflict re-renders the editor instead of the generic
  "Server error (409)" toast.
- The author join (`LEFT JOIN users`) has no `deleted_at IS NULL`: it
  names who made a past change, and a deactivated admin still made it
  (commented in the model). After a purge `updated_by` is NULL → "author
  unknown".
- No admin_audit row for skill writes (not asked; `tracing::info!` only).

### Gates (2026-11-04)

- `cargo check`: clean.
- `mem-guard cargo clippy --all-targets -- -D warnings`: clean.
- `cargo sqlx prepare --check --workspace -- --all-targets`: clean (dynamic
  queries only, no `.sqlx/` change).
- `cargo test --test locale_parity`: 3 passed.
- Full Rust suite with the DB (`mem-guard cargo test -j 2 --no-fail-fast`;
  the first run without `-j 2` was killed at the memory cap during the
  build and retried): 2484 passed, 0 failed.
- `tsc --noEmit`: clean; `waitForTimeout` grep: no hit.
- Full Playwright suite on a freshly reset stack
  (`CI=true --workers=2`, port 18080): 447 passed, 2 skipped, 0 failed.

### File List

- `migrations/20261104150000_create_mcp_skills.sql` (new)
- `src/models/mcp_skill.rs` (new), `src/models/mod.rs`
- `src/services/mcp_skills.rs` (new), `src/services/mod.rs`
- `src/routes/admin_skills.rs` (new), `src/routes/admin.rs`, `src/routes/mod.rs`
- `src/services/soft_delete.rs`, `src/services/auto_purge.rs`, `src/services/admin_health.rs`
- `templates/fragments/admin_skills_{panel,editor,list,delete_modal}.html` (new),
  `templates/fragments/admin_trash_panel.html`
- `static/js/csrf.js`
- `locales/{en,fr,de,it}.yml`
- `tests/mcp_skills.rs` (new), `tests/e2e/specs/journeys/admin-skills.spec.ts` (new)
- `CLAUDE.md`

### Review Findings

Independent review (2026-11-04): one Medium, no High.

- [x] **M1 (Medium) — creating a trashed name overwrote the trashed skill.**
  Fixed: the reactivation branch of `McpSkillModel::create` is removed;
  a create onto a trashed name returns `NameInTrash` like a rename, with
  creation copy in four locales (`error.mcp_skills.name_in_trash_on_create`);
  `admin.skills.feedback_reactivated` removed. Test
  `a_trashed_name_is_refused_and_the_trashed_skill_is_kept` replaces the
  reactivation test (create and rename refused, trashed body intact,
  restore works). Migration comment, CLAUDE.md entry and AC1 note updated.
  The E2E did not rely on reactivation.
- [x] **L1 (Low) — author-join exception undocumented in CLAUDE.md.** Fixed:
  the skills entry now states the `LEFT JOIN users` without
  `deleted_at IS NULL` (like `decided_by_name`).
- [x] **L2 (Low)** — `src/routes/admin.rs` at 1 838 lines, under the Rule #12 limit; accepted — the next admin tab should move Health or Trash out of it.
- [x] **L3 (Low)** — toggling a skill bumps its `version`, so an editor open on the same skill gets a stale-version error on save; correct optimistic-locking behaviour, accepted.

Gates after the fixes are listed below under "Gates after review".

### Gates after review (2026-11-04)

- `cargo check`: clean; `mem-guard cargo clippy -j 2 --all-targets -- -D warnings`: clean.
- `cargo sqlx prepare --check --workspace -- --all-targets`: clean.
- `cargo test --lib`: 1694 passed; `--test mcp_skills`: 9 passed;
  `--test locale_parity`: 3 passed.
- `tsc --noEmit`: clean; `waitForTimeout` grep: no hit.
- `admin-skills.spec.ts` on a freshly reset stack (`CI=true --workers=2`):
  3 passed (the journey now also checks that a trashed name is refused on
  create and that the restored skill keeps its own text). Full Playwright
  not re-run: no shared code changed in this round.

#### Round 2 (32dbdfa)

No Medium+ finding. Low fixed: the E2E refusal check now matches the creation-refusal copy (four locales) instead of any "Trash" word, which the earlier "moved to the Trash" entry already contained. admin-skills.spec: 3 passed on a fresh stack.
