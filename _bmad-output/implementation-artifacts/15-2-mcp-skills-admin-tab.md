# Story 15.2: MCP skills — admin tab

Status: ready-for-dev

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
